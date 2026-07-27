use crate::components::chat::chat_header::ChatHeader;
use crate::components::chat::chat_input::ChatInput;
use crate::components::chat::message_list::MessageList;
use crate::components::layout::CurrentSession;
use crate::components::ui::toast::use_toast;
use crate::models::character::Character;
use crate::models::message::{ImageUpload, Message, MessageRole};
use crate::server::character::get_character;
use crate::server::chat::{
    edit_user_message, get_chat_messages, get_chat_session, stop_generation as request_stop,
    stream_chat_reply, stream_regenerate,
};
use crate::server::settings::get_settings;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use uuid::Uuid;

/// Data loaded for a chat session.
#[derive(Clone)]
struct SessionData {
    character: Option<Character>,
    messages: Vec<Message>,
}

async fn load_session(id: Uuid) -> Result<SessionData, String> {
    let session = get_chat_session(id).await.map_err(|e| e.to_string())?;
    let Some(session) = session else {
        return Err("Session not found".to_string());
    };
    let character = get_character(session.character_id)
        .await
        .map_err(|e| e.to_string())?;
    let messages = get_chat_messages(id).await.map_err(|e| e.to_string())?;
    Ok(SessionData {
        character,
        messages,
    })
}

#[component]
pub fn ChatPage() -> impl IntoView {
    let toast = use_toast();

    let params = use_params_map();
    let session_id = Signal::derive(move || {
        params.with(|p| p.get("id").and_then(|id| Uuid::parse_str(id.as_str()).ok()))
    });

    // Local, reactive chat state.
    let messages = RwSignal::new(Vec::<Message>::new());
    let character = RwSignal::new(Option::<Character>::None);
    let is_streaming = RwSignal::new(false);
    let streaming_content = RwSignal::new(String::new());
    let streaming_msg_id = RwSignal::new(Option::<Uuid>::None);
    let selected_provider = RwSignal::new(Option::<Uuid>::None);
    let selected_model = RwSignal::new(Option::<String>::None);

    let current_session = expect_context::<CurrentSession>();

    // Load persisted default model preference for the selector (client-only).
    let settings_res = LocalResource::new(move || async move { get_settings().await });
    let preferred_model = Signal::derive(move || {
        settings_res
            .get()
            .and_then(|r| r.ok())
            .and_then(|s| s.default_model)
    });
    let render_thinking = Signal::derive(move || {
        settings_res
            .get()
            .and_then(|r| r.ok())
            .map(|s| s.render_thinking)
            .unwrap_or(true)
    });

    // Load the session whenever the id changes (client-only resource).
    let session_res = LocalResource::new(move || async move {
        match session_id.get() {
            Some(id) => Some(load_session(id).await),
            None => None,
        }
    });

    // Sync loaded data into the reactive working state.
    let load_error = RwSignal::new(Option::<String>::None);
    Effect::new(move |_| {
        if let Some(sid) = session_id.get() {
            current_session.0.set(Some(sid));
        }
        match session_res.get() {
            Some(Some(Ok(data))) => {
                load_error.set(None);
                character.set(data.character.clone());
                messages.set(data.messages.clone());
            }
            Some(Some(Err(e))) => {
                load_error.set(Some(e.clone()));
                character.set(None);
                messages.set(Vec::new());
            }
            _ => {}
        }
    });

    // Consume a TextStream future: show a live placeholder, append tokens, then
    // reload the authoritative message list from the server.
    let toast_for_stream = toast.clone();
    let consume_stream = move |fut: std::pin::Pin<
        Box<
            dyn std::future::Future<
                Output = Result<leptos::server_fn::codec::TextStream, ServerFnError>,
            >,
        >,
    >| {
        is_streaming.set(true);
        streaming_content.set(String::new());
        streaming_msg_id.set(Some(Uuid::new_v4()));

        let toast_stream = toast_for_stream.clone();
        leptos::task::spawn_local(async move {
            use futures::StreamExt;
            match fut.await {
                Ok(text_stream) => {
                    let mut stream = text_stream.into_inner();
                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(text) => streaming_content.update(|s| s.push_str(&text)),
                            Err(e) => {
                                toast_stream.error(format!("Stream error: {e}"));
                                break;
                            }
                        }
                    }
                }
                Err(e) => toast_stream.error(format!("Failed: {e}")),
            }

            is_streaming.set(false);
            streaming_msg_id.set(None);
            streaming_content.set(String::new());
            if let Some(sid) = session_id.get_untracked() {
                if let Ok(msgs) = get_chat_messages(sid).await {
                    messages.set(msgs);
                }
            }
        });
    };

    let toast_err = toast.clone();
    let consume_send = consume_stream.clone();
    let send_message = Callback::new(move |(content, images): (String, Vec<ImageUpload>)| {
        let Some(sid) = session_id.get() else { return };
        let Some(pid) = selected_provider.get() else {
            toast_err.error("Select a model first");
            return;
        };
        let Some(model) = selected_model.get() else {
            toast_err.error("Select a model first");
            return;
        };

        // Optimistically show the user's message.
        messages.update(|m| {
            m.push(Message {
                id: Uuid::new_v4(),
                session_id: sid,
                role: MessageRole::User,
                content: content.clone(),
                attachments: Vec::new(),
                model_used: None,
                provider_id: None,
                created_at: chrono::Utc::now(),
            })
        });

        consume_send(Box::pin(stream_chat_reply(
            sid, content, images, pid, model,
        )));
    });

    // Regenerate the last assistant reply.
    let toast_regen = toast.clone();
    let consume_regen = consume_stream.clone();
    let regenerate = Callback::new(move |_assistant_id: Uuid| {
        let Some(sid) = session_id.get() else { return };
        let Some(pid) = selected_provider.get() else {
            toast_regen.error("Select a model first");
            return;
        };
        let Some(model) = selected_model.get() else {
            toast_regen.error("Select a model first");
            return;
        };
        // Optimistically drop the trailing assistant message from the view.
        messages.update(|m| {
            while m
                .last()
                .map(|x| x.role == MessageRole::Assistant)
                .unwrap_or(false)
            {
                m.pop();
            }
        });
        consume_regen(Box::pin(stream_regenerate(sid, pid, model)));
    });

    // Edit a user message: persist the edit + attachments + truncate, then regenerate.
    let toast_edit = toast.clone();
    let edit_message = Callback::new(move |payload: crate::models::message::EditPayload| {
        let Some(sid) = session_id.get() else { return };
        let Some(pid) = selected_provider.get() else {
            toast_edit.error("Select a model first");
            return;
        };
        let Some(model) = selected_model.get() else {
            toast_edit.error("Select a model first");
            return;
        };
        let toast_inner = toast_edit.clone();
        let consume = consume_stream.clone();
        leptos::task::spawn_local(async move {
            // First persist the edit (content + attachments) + truncate after.
            if let Err(e) = edit_user_message(
                payload.message_id,
                payload.content,
                payload.keep_attachment_ids,
                payload.new_images,
            )
            .await
            {
                toast_inner.error(format!("Edit failed: {e}"));
                return;
            }
            // Reflect the truncation locally, then stream a fresh reply.
            if let Ok(msgs) = get_chat_messages(sid).await {
                messages.set(msgs);
            }
            consume(Box::pin(stream_regenerate(sid, pid, model)));
        });
    });

    let stop_generation = Callback::new(move |_: ()| {
        let Some(sid) = session_id.get_untracked() else {
            return;
        };
        leptos::task::spawn_local(async move {
            // Ask the server to abort the provider request. On success the
            // stream ends on its own: the consume loop finishes, the partial
            // reply is persisted server-side, and the message list reloads.
            match request_stop(sid).await {
                Ok(true) => {}
                _ => {
                    // Nothing active server-side (or the call failed) — just
                    // clear the local streaming UI.
                    is_streaming.set(false);
                    streaming_msg_id.set(None);
                    streaming_content.set(String::new());
                }
            }
        });
    });

    let has_session = Signal::derive(move || session_id.get().is_some());
    let loading = Signal::derive(move || {
        // Loaded value is Some(Some(_)) once the fetch resolves for a real id.
        session_id.get().is_some() && !matches!(session_res.get(), Some(Some(_)))
    });

    // Messages shown in the list: the persisted history plus a live streaming
    // assistant placeholder while a reply is being generated.
    let display_messages = Signal::derive(move || {
        let mut list = messages.get();
        if is_streaming.get() {
            if let Some(id) = streaming_msg_id.get() {
                list.push(Message {
                    id,
                    session_id: session_id.get().unwrap_or_default(),
                    role: MessageRole::Assistant,
                    content: String::new(),
                    attachments: Vec::new(),
                    model_used: selected_model.get(),
                    provider_id: selected_provider.get(),
                    created_at: chrono::Utc::now(),
                });
            }
        }
        list
    });

    view! {
        <Show
            when=move || has_session.get()
            // Reachable only for a URL with no usable session id (e.g. a
            // malformed /chat/<id>); send those to the character list rather
            // than showing an interstitial.
            fallback=|| view! { <leptos_router::components::Redirect path="/characters"/> }
        >
            <Show
                when=move || load_error.get().is_none()
                fallback=move || view! {
                    // The session failed to load — say so instead of silently
                    // presenting what looks like an empty new chat.
                    <div class="flex h-full flex-col items-center justify-center p-6 text-center">
                        <div class="mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-destructive/10 text-destructive">
                            <svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 24 24"
                                 fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/>
                                <path d="M12 9v4"/>
                                <path d="M12 17h.01"/>
                            </svg>
                        </div>
                        <h2 class="text-lg font-semibold">"Couldn't load this conversation"</h2>
                        <p class="mt-1.5 max-w-sm break-words text-sm text-muted-foreground">
                            {move || load_error.get().unwrap_or_default()}
                        </p>
                        <div class="mt-6 flex items-center gap-2">
                            <button
                                class="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-input \
                                       bg-background px-4 text-sm font-medium shadow-sm transition-colors hover:bg-accent"
                                on:click=move |_| session_res.refetch()
                            >
                                "Try again"
                            </button>
                            <leptos_router::components::A
                                href="/characters"
                                attr:class="inline-flex h-10 items-center justify-center gap-2 rounded-lg bg-primary \
                                       px-4 text-sm font-medium text-primary-foreground shadow transition-colors \
                                       hover:bg-primary/90 no-underline"
                            >
                                "Start a new chat"
                            </leptos_router::components::A>
                        </div>
                    </div>
                }
            >
            <div class="flex h-full flex-col bg-muted/20">
                <ChatHeader
                    character=Signal::derive(move || character.get())
                    selected_provider=selected_provider
                    selected_model=selected_model
                    preferred_model=preferred_model
                />
                <Show when=move || loading.get()>
                    <div class="h-0.5 w-full overflow-hidden bg-primary/20">
                        <div class="h-full w-1/3 animate-pulse bg-primary"></div>
                    </div>
                </Show>
                <MessageList
                    messages=display_messages
                    character_name=Signal::derive(move || {
                        character.get().map(|c| c.name).unwrap_or_default()
                    })
                    character_avatar=Signal::derive(move || {
                        character.get().and_then(|c| c.avatar_path)
                    })
                    streaming_msg_id=Signal::derive(move || streaming_msg_id.get())
                    streaming_content=Signal::derive(move || streaming_content.get())
                    render_thinking=render_thinking
                    on_edit=edit_message
                    on_regenerate=regenerate
                />
                <ChatInput
                    on_send=send_message
                    on_stop=stop_generation
                    is_streaming=Signal::derive(move || is_streaming.get())
                    disabled=Signal::derive(move || selected_model.get().is_none())
                />
            </div>
            </Show>
        </Show>
    }
}
