//! Chat server functions: session CRUD, message loading, and streaming
//! replies. Streams use Leptos `StreamingText`: deltas are forwarded as text
//! chunks while the full reply is accumulated and persisted on completion.

use leptos::prelude::*;
use leptos::server_fn::codec::{Json, StreamingText, TextStream};
use uuid::Uuid;

use crate::models::chat::{ChatSession, ChatSummary};
use crate::models::message::{ImageUpload, Message};

#[server(CreateChatSession, "/api")]
pub async fn create_chat_session(character_id: Uuid) -> Result<ChatSession, ServerFnError> {
    use crate::db::Db;
    use crate::models::chat::NewChatSession;
    use crate::services::character_service::CharacterService;
    use crate::services::chat_service::ChatService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || {
        let character = CharacterService::new(db.clone())
            .get(character_id)?
            .ok_or_else(|| anyhow::anyhow!("Character not found"))?;
        ChatService::new(db).create_session(
            &NewChatSession {
                character_id,
                title: None,
            },
            character.greeting.clone(),
        )
    })
    .await
    .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
    .map_err(|e| ServerFnError::new(format!("Failed to create session: {e}")))
}

#[server(GetChatSession, "/api")]
pub async fn get_chat_session(id: Uuid) -> Result<Option<ChatSession>, ServerFnError> {
    use crate::db::Db;
    use crate::services::chat_service::ChatService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || ChatService::new(db).get_session(id))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to load session: {e}")))
}

/// One newest-first page of conversations. `query` searches character names,
/// session titles, and message previews across *all* sessions, not just the
/// page the client currently holds.
#[server(ListChatSessions, "/api")]
pub async fn list_chat_sessions(
    character_id: Option<Uuid>,
    query: Option<String>,
    limit: u32,
    offset: u32,
) -> Result<Vec<ChatSummary>, ServerFnError> {
    use crate::db::Db;
    use crate::services::chat_service::ChatService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || {
        ChatService::new(db).list_sessions(character_id, query.as_deref(), limit, offset)
    })
    .await
    .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
    .map_err(|e| ServerFnError::new(format!("Failed to list sessions: {e}")))
}

#[server(GetChatMessages, "/api")]
pub async fn get_chat_messages(session_id: Uuid) -> Result<Vec<Message>, ServerFnError> {
    use crate::db::Db;
    use crate::services::chat_service::ChatService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || ChatService::new(db).list_messages(session_id))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to load messages: {e}")))
}

#[server(DeleteChatSession, "/api")]
pub async fn delete_chat_session(id: Uuid) -> Result<(), ServerFnError> {
    use crate::db::Db;
    use crate::services::chat_service::ChatService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || ChatService::new(db).delete_session(id))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to delete session: {e}")))
}

/// Stream an assistant reply to a newly sent user message.
///
/// Persists the user message (with any images), builds the prompt from history,
/// streams tokens from the selected provider, and saves the assistant message
/// on completion.
#[server(StreamChatReply, "/api", input = Json, output = StreamingText)]
pub async fn stream_chat_reply(
    session_id: Uuid,
    content: String,
    images: Vec<ImageUpload>,
    provider_id: Uuid,
    model: String,
) -> Result<TextStream, ServerFnError> {
    use crate::db::Db;
    use crate::models::message::{MessageRole, NewMessage};
    use crate::services::chat_service::ChatService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    // Persist the user's message first.
    {
        let db = db.clone();
        let content = content.clone();
        tokio::task::spawn_blocking(move || {
            ChatService::new(db).add_message(&NewMessage {
                session_id,
                role: MessageRole::User,
                content,
                image_attachments: images,
                model_used: None,
                provider_id: None,
            })
        })
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to save message: {e}")))?;
    }

    stream_reply(db, session_id, provider_id, model).await
}

/// Regenerate the assistant's reply: remove the trailing assistant message(s)
/// after the last user message, then re-stream from the existing history.
#[server(StreamRegenerate, "/api", input = Json, output = StreamingText)]
pub async fn stream_regenerate(
    session_id: Uuid,
    provider_id: Uuid,
    model: String,
) -> Result<TextStream, ServerFnError> {
    use crate::db::Db;
    use crate::models::message::MessageRole;
    use crate::services::chat_service::ChatService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    // Delete trailing assistant messages so the prompt ends on the user turn.
    {
        let db = db.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let chat = ChatService::new(db);
            let msgs = chat.list_messages(session_id)?;
            // Remove trailing assistant messages from the end.
            for msg in msgs.iter().rev() {
                if msg.role == MessageRole::Assistant {
                    chat.delete_message(msg.id)?;
                } else {
                    break;
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to prepare regenerate: {e}")))?;
    }

    stream_reply(db, session_id, provider_id, model).await
}

/// Edit a user message: update its content and delete everything after it
/// (so a fresh reply can be generated). Returns nothing; the client then calls
/// `stream_regenerate` to produce the new reply.
#[server(EditUserMessage, "/api", input = Json)]
pub async fn edit_user_message(
    message_id: Uuid,
    content: String,
    /// Ids of existing attachments to keep. Any current attachment not in this
    /// list is removed.
    keep_attachment_ids: Vec<Uuid>,
    /// New images (base64) to add to the message.
    new_images: Vec<ImageUpload>,
) -> Result<(), ServerFnError> {
    use crate::db::Db;
    use crate::services::chat_service::ChatService;

    // Allow empty text only if there are (kept or new) images.
    if content.trim().is_empty() && keep_attachment_ids.is_empty() && new_images.is_empty() {
        return Err(ServerFnError::new("Message cannot be empty"));
    }

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let chat = ChatService::new(db);
        let msg = chat
            .get_message(message_id)?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;
        chat.update_message_content(message_id, &content)?;
        chat.replace_message_attachments(message_id, &keep_attachment_ids, &new_images)?;
        chat.delete_messages_after(msg.session_id, message_id)?;
        Ok(())
    })
    .await
    .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
    .map_err(|e| ServerFnError::new(format!("Failed to edit message: {e}")))
}

/// Cancel the in-flight generation for a session, if any. The server aborts
/// the provider request and persists whatever partial reply was streamed so
/// far; the client's stream then ends normally. Returns whether a generation
/// was active.
#[server(StopGeneration, "/api")]
pub async fn stop_generation(session_id: Uuid) -> Result<bool, ServerFnError> {
    use crate::services::generation;
    Ok(generation::cancel(session_id))
}

/// Shared streaming implementation: build the prompt from the current session
/// history, stream from the provider, and persist the assistant reply.
#[cfg(feature = "ssr")]
async fn stream_reply(
    db: crate::db::Db,
    session_id: Uuid,
    provider_id: Uuid,
    model: String,
) -> Result<TextStream, ServerFnError> {
    use crate::models::message::{MessageRole, NewMessage};
    use crate::providers::registry::build_provider;
    use crate::providers::traits::StreamEvent;
    use crate::services::character_service::CharacterService;
    use crate::services::chat_service::ChatService;
    use crate::services::provider_service::ProviderService;
    use crate::services::settings_service::SettingsService;
    use futures::StreamExt;

    // Gather prompt + provider config + sampling params on the blocking pool.
    let (prompt, provider, sampling) = {
        let db = db.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let chat = ChatService::new(db.clone());
            let session = chat
                .get_session(session_id)?
                .ok_or_else(|| anyhow::anyhow!("Session not found"))?;
            let character = CharacterService::new(db.clone())
                .get(session.character_id)?
                .ok_or_else(|| anyhow::anyhow!("Character not found"))?;
            let settings = SettingsService::new(db.clone()).get_all()?;
            let prompt = chat.build_prompt(
                session_id,
                &character,
                &settings.user_name,
                &settings.default_system_prompt,
            )?;
            let provider = ProviderService::new(db)
                .get(provider_id)?
                .ok_or_else(|| anyhow::anyhow!("Provider not found"))?;
            Ok((prompt, provider, settings.sampling()))
        })
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to prepare chat: {e}")))?
    };

    let llm = build_provider(&provider);
    let mut token_stream = llm
        .stream_chat(prompt, &model, &sampling)
        .await
        .map_err(|e| ServerFnError::new(format!("Provider error: {e}")))?;

    let db_for_save = db.clone();
    let model_for_save = model.clone();

    // Register with the cancellation registry so `stop_generation` can abort
    // this stream (and so a newer generation for the session replaces it).
    let (generation_id, mut cancel_rx) = crate::services::generation::begin(session_id);

    let text_stream = async_stream::stream! {
        // Deregisters this generation when the stream ends OR is dropped
        // mid-flight (e.g. the client disconnects).
        let _finish_guard =
            crate::services::generation::FinishGuard::new(session_id, generation_id);

        let mut accumulated = String::new();
        // Reasoning tokens from a dedicated API field are wrapped in
        // <think>...</think> so the client renders them as a collapsible block.
        let mut in_reasoning = false;

        macro_rules! emit {
            ($text:expr) => {{
                let t = $text;
                accumulated.push_str(&t);
                yield Ok(t);
            }};
        }

        loop {
            // Race the next provider token against cancellation. `changed()`
            // resolves on an explicit stop request, or with Err when a newer
            // generation for this session drops our sender — both mean stop.
            // (Yields live outside the select arms: async-stream can't expand
            // `yield` inside another macro's body.)
            let next = tokio::select! {
                biased;
                _ = cancel_rx.changed() => None,
                ev = token_stream.next() => Some(ev),
            };

            let Some(ev) = next else {
                tracing::info!("generation cancelled (session {session_id})");
                break;
            };
            let Some(ev) = ev else { break };

            match ev {
                Ok(StreamEvent::Reasoning(chunk)) => {
                    if !in_reasoning {
                        in_reasoning = true;
                        emit!("<think>".to_string());
                    }
                    emit!(chunk);
                }
                Ok(StreamEvent::Delta(chunk)) => {
                    if in_reasoning {
                        in_reasoning = false;
                        emit!("</think>".to_string());
                    }
                    emit!(chunk);
                }
                Ok(StreamEvent::Done) => break,
                // Error text is shown to the client but intentionally NOT
                // accumulated, so it never gets persisted as message content.
                Ok(StreamEvent::Error(e)) => {
                    tracing::error!("provider stream error (session {session_id}): {e}");
                    yield Ok(format!("\n\n[error: {e}]"));
                    break;
                }
                Err(e) => {
                    tracing::error!("provider stream error (session {session_id}): {e}");
                    yield Ok(format!("\n\n[error: {e}]"));
                    break;
                }
            }
        }

        // Dropping the provider stream aborts the upstream HTTP request, so
        // the provider stops generating immediately after cancellation.
        drop(token_stream);

        if in_reasoning {
            emit!("</think>".to_string());
        }

        if !accumulated.trim().is_empty() {
            let db = db_for_save.clone();
            let content = accumulated.clone();
            let model = model_for_save.clone();
            let save_result = tokio::task::spawn_blocking(move || {
                ChatService::new(db).add_message(&NewMessage {
                    session_id,
                    role: MessageRole::Assistant,
                    content,
                    image_attachments: Vec::new(),
                    model_used: Some(model),
                    provider_id: Some(provider_id),
                })
            })
            .await;
            match save_result {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    tracing::error!("failed to save assistant message (session {session_id}): {e}");
                }
                Err(e) => {
                    tracing::error!("assistant message save task panicked (session {session_id}): {e}");
                }
            }
        }
    };

    Ok(TextStream::new(text_stream))
}
