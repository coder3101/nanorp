use crate::components::chat::chat_header::ChatHeader;
use crate::components::chat::chat_input::ChatInput;
use crate::components::chat::message_list::MessageList;
use crate::components::layout::CurrentSession;
use crate::components::ui::toast::use_toast;
use crate::models::character::Character;
use crate::models::message::{GenerationOutcome, ImageUpload, Message, MessageRole};
use crate::server::character::get_character;
use crate::server::chat::{
    edit_user_message, get_chat_messages, get_chat_session, stop_generation as request_stop,
    stream_chat_reply, stream_regenerate,
};
use crate::server::settings::get_settings;
use crate::services::generation_tracker::{use_generation_tracker, PendingSubmission};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use uuid::Uuid;

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

/// Router wrapper. Keying the actual session controller by UUID is the central
/// isolation guarantee: Leptos must dispose A's local state before mounting B.
#[component]
pub fn ChatPage() -> impl IntoView {
    let params = use_params_map();
    let session_id = Signal::derive(move || {
        params.with(|params| params.get("id").and_then(|id| Uuid::parse_str(&id).ok()))
    });

    view! {
        <Show
            when=move || session_id.get().is_some()
            fallback=|| view! { <leptos_router::components::Redirect path="/characters"/> }
        >
            <For
                each={move || session_id.get().into_iter().collect::<Vec<_>>()}
                key=|id| *id
                let:id
            >
                <ChatSession session_id=id />
            </For>
        </Show>
    }
}

#[component]
fn ChatSession(session_id: Uuid) -> impl IntoView {
    let sid = session_id;
    let toast = use_toast();
    let tracker = use_generation_tracker();
    let messages = RwSignal::new(Vec::<Message>::new());
    let character = RwSignal::new(Option::<Character>::None);
    let selected_provider = RwSignal::new(Option::<Uuid>::None);
    let selected_model = RwSignal::new(Option::<String>::None);
    let load_error = RwSignal::new(Option::<String>::None);
    let history_error = RwSignal::new(Option::<String>::None);
    let clear_revision = RwSignal::new(0_u64);
    let notice_dismissed = RwSignal::new(false);
    let mounted = RwSignal::new(true);

    let current_session = expect_context::<CurrentSession>();
    current_session.0.set(Some(sid));
    on_cleanup(move || {
        mounted.set(false);
        if current_session.0.get_untracked() == Some(sid) {
            current_session.0.set(None);
        }
    });

    let settings_res = LocalResource::new(move || async move { get_settings().await });
    let preferred_model = Signal::derive(move || {
        settings_res
            .get()
            .and_then(|result| result.ok())
            .and_then(|settings| settings.default_model)
    });
    let render_thinking = Signal::derive(move || {
        settings_res
            .get()
            .and_then(|result| result.ok())
            .map(|settings| settings.render_thinking)
            .unwrap_or(true)
    });

    let session_res = LocalResource::new(move || load_session(sid));
    let loading = Signal::derive(move || session_res.get().is_none());
    let load_tracker = tracker.clone();
    Effect::new(move |_| match session_res.get() {
        Some(Ok(data)) => {
            load_error.set(None);
            if load_tracker.reconcile_submission(sid, &data.messages) {
                clear_revision.update(|revision| *revision += 1);
            }
            character.set(data.character.clone());
            messages.set(data.messages.clone());
        }
        Some(Err(error)) => {
            load_error.set(Some(error));
            character.set(None);
            messages.set(Vec::new());
        }
        None => {}
    });

    let active_for_session = Signal::derive(move || tracker.active.get().get(&sid).cloned());
    let is_generating = Signal::derive(move || active_for_session.get().is_some());
    let streaming_msg_id = Signal::derive(move || is_generating.get().then_some(sid));
    let streaming_content = Signal::derive(move || {
        active_for_session
            .get()
            .map(|active| active.content.get())
            .unwrap_or_default()
    });
    let finished = Signal::derive(move || tracker.finished.get().get(&sid).cloned());

    // Reload only this mounted session. A generation finishing for another
    // route can therefore never replace the visible history.
    let tracker_finish = tracker.clone();
    let toast_finish = toast.clone();
    let on_generation_finish = Callback::new(move |finished_sid: Uuid| {
        if finished_sid != sid {
            return;
        }
        let visible = mounted.get_untracked();
        let tracker_finished = tracker_finish.clone();
        let toast_finished = toast_finish.clone();
        leptos::task::spawn_local(async move {
            match get_chat_messages(sid).await {
                Ok(loaded) => {
                    // This runs even off-screen because the pending submission
                    // and draft belong to the app-scoped session, not the page.
                    let accepted = tracker_finished.reconcile_submission(sid, &loaded);
                    if !visible {
                        return;
                    }
                    history_error.set(None);
                    if accepted {
                        clear_revision.update(|revision| *revision += 1);
                    }
                    messages.set(loaded);
                }
                Err(error) => {
                    if !visible {
                        return;
                    }
                    let text = error.to_string();
                    history_error.set(Some(text.clone()));
                    // The tracker retains the streamed text off-screen; never
                    // pretend a failed refresh means an empty conversation.
                    if tracker_finished
                        .finished
                        .get_untracked()
                        .get(&sid)
                        .is_some_and(|entry| !entry.content.is_empty())
                    {
                        toast_finished
                            .warning("Reply finished, but history could not be refreshed");
                    }
                }
            }
        });
    });

    let refresh_after_accept = Callback::new(move |_: ()| {
        if !mounted.get_untracked() {
            return;
        }
        leptos::task::spawn_local(async move {
            if let Ok(loaded) = get_chat_messages(sid).await {
                messages.set(loaded);
            }
        });
    });

    // Created once under the component's stable owner, never inside the send
    // handler: the Send button lives in a `<Show>` fallback that is disposed the
    // instant generation starts, so a callback created there would be gone by
    // the time the stream resolves and runs it — panicking the whole app.
    let on_accepted = {
        let tracker = tracker.clone();
        Callback::new(move |_| {
            // Acceptance belongs to the submitted session even when its
            // controller has already unmounted during navigation.
            tracker.accept_submission(sid);
            if mounted.get_untracked() {
                clear_revision.update(|revision| *revision += 1);
                refresh_after_accept.run(());
            }
        })
    };

    let send_message = {
        let tracker = tracker.clone();
        let toast = toast.clone();
        Callback::new(move |(content, images): (String, Vec<ImageUpload>)| {
            if is_generating.get_untracked() || loading.get_untracked() {
                return;
            }
            let Some(provider_id) = selected_provider.get_untracked() else {
                toast.error("Select a model first");
                return;
            };
            let Some(model) = selected_model.get_untracked() else {
                toast.error("Select a model first");
                return;
            };

            notice_dismissed.set(false);
            let previous_user_id = messages.with_untracked(|items| {
                items
                    .iter()
                    .rev()
                    .find(|message| message.role == MessageRole::User)
                    .map(|message| message.id)
            });
            tracker.begin_submission(
                sid,
                PendingSubmission {
                    content: content.clone(),
                    image_count: images.len(),
                    previous_user_id,
                },
            );
            messages.update(|items| {
                items.push(Message {
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

            let toast_send = toast.clone();
            tracker.start(
                sid,
                false,
                Box::pin(stream_chat_reply(sid, content, images, provider_id, model)),
                toast_send.clone(),
                on_accepted,
                on_generation_finish,
            );
        })
    };

    let start_regeneration = {
        let tracker = tracker.clone();
        let toast = toast.clone();
        Callback::new(move |_: ()| {
            if is_generating.get_untracked() || loading.get_untracked() {
                return;
            }
            let Some(provider_id) = selected_provider.get_untracked() else {
                toast.error("Select a model first");
                return;
            };
            let Some(model) = selected_model.get_untracked() else {
                toast.error("Select a model first");
                return;
            };
            notice_dismissed.set(false);
            tracker.start(
                sid,
                true,
                Box::pin(stream_regenerate(sid, provider_id, model)),
                toast.clone(),
                refresh_after_accept,
                on_generation_finish,
            );
        })
    };

    let toast_regenerate = toast.clone();
    let regenerate = Callback::new(move |assistant_id: Uuid| {
        let valid_target = messages.get_untracked().last().is_some_and(|message| {
            message.id == assistant_id && message.role == MessageRole::Assistant
        });
        if !valid_target {
            toast_regenerate.warning("Only the latest response can be regenerated");
            return;
        }
        start_regeneration.run(());
    });

    let toast_for_edit = toast.clone();
    let edit_message = Callback::new(move |payload: crate::models::message::EditPayload| {
        let toast_edit = toast_for_edit.clone();
        leptos::task::spawn_local(async move {
            if let Err(error) = edit_user_message(
                payload.message_id,
                payload.content,
                payload.keep_attachment_ids,
                payload.new_images,
            )
            .await
            {
                toast_edit.error(format!("Edit failed: {error}"));
                return;
            }
            match get_chat_messages(sid).await {
                Ok(loaded) => messages.set(loaded),
                Err(error) => {
                    history_error.set(Some(error.to_string()));
                    return;
                }
            }
            start_regeneration.run(());
        });
    });

    let stop_generation = {
        let tracker = tracker.clone();
        let toast = toast.clone();
        Callback::new(move |_: ()| {
            tracker.request_stopping(sid);
            let tracker = tracker.clone();
            let toast = toast.clone();
            leptos::task::spawn_local(async move {
                match request_stop(sid).await {
                    Ok(true) => {}
                    Ok(false) => tracker.stop_request_failed(sid),
                    Err(error) => {
                        tracker.stop_request_failed(sid);
                        toast.error(format!("Couldn't stop generation: {error}"));
                    }
                }
            });
        })
    };

    let display_messages = Signal::derive(move || {
        let mut list = messages.get();
        let active = active_for_session.get();
        if let Some(active_generation) = active.as_ref() {
            // A regeneration removes the previous response the moment it starts,
            // so the old bubble is immediately replaced by the loading/streaming
            // one instead of both showing at once. The server deletes it too; if
            // regeneration fails, refreshing history restores it.
            if active_generation.replacing_response {
                while list
                    .last()
                    .is_some_and(|message| message.role == MessageRole::Assistant)
                {
                    list.pop();
                }
            }
            list.push(Message {
                id: sid,
                session_id: sid,
                role: MessageRole::Assistant,
                content: String::new(),
                attachments: Vec::new(),
                model_used: selected_model.get(),
                provider_id: selected_provider.get(),
                created_at: chrono::Utc::now(),
            });
        }
        list
    });

    let input_disabled = Signal::derive(move || selected_model.get().is_none() || loading.get());

    view! {
        <Show
            when=move || load_error.get().is_none()
            fallback=move || view! {
                <div class="flex h-full flex-col items-center justify-center p-6 text-center">
                    <div class="mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-destructive/10 text-destructive">
                        <svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a1.73 1.73 0 0 0 1.73-3Z"/><path d="M12 9v4"/><path d="M12 17h.01"/></svg>
                    </div>
                    <h2 class="text-lg font-semibold">"Couldn't load this conversation"</h2>
                    <p class="mt-1.5 max-w-sm break-words text-sm text-muted-foreground">{move || load_error.get().unwrap_or_default()}</p>
                    <div class="mt-6 flex flex-wrap justify-center gap-2">
                        <button type="button" class="inline-flex h-11 items-center rounded-lg border border-input px-4 text-sm font-medium hover:bg-accent" on:click=move |_| session_res.refetch()>"Try again"</button>
                        <leptos_router::components::A href="/characters" attr:class="inline-flex h-11 items-center rounded-lg bg-primary px-4 text-sm font-medium text-primary-foreground no-underline">"Start a new chat"</leptos_router::components::A>
                    </div>
                </div>
            }
        >
            <div class="flex h-full min-h-0 flex-col bg-muted/20">
                <ChatHeader character=Signal::derive(move || character.get()) selected_provider=selected_provider selected_model=selected_model preferred_model=preferred_model />
                <MessageList
                    messages=display_messages
                    session_id=Signal::derive(move || Some(sid))
                    character_name=Signal::derive(move || character.get().map(|c| c.name).unwrap_or_default())
                    character_avatar=Signal::derive(move || character.get().and_then(|c| c.avatar_path))
                    streaming_msg_id=streaming_msg_id
                    streaming_content=streaming_content
                    render_thinking=render_thinking
                    loading=loading
                    actions_disabled=is_generating
                    on_edit=edit_message
                    on_regenerate=regenerate
                />

                <Show when=move || history_error.get().is_some()>
                    <div class="border-t border-destructive/30 bg-destructive/10 px-4 py-2 text-sm" role="alert">
                        <div class="mx-auto flex max-w-3xl flex-wrap items-center justify-between gap-2">
                            <span>"Couldn't refresh conversation history."</span>
                            <button type="button" class="min-h-9 rounded-md px-3 font-medium underline" on:click=move |_| on_generation_finish.run(sid)>"Retry"</button>
                        </div>
                    </div>
                </Show>

                <Show when=move || finished.get().is_some() && !notice_dismissed.get()>
                    {move || finished.get().map(|result| {
                        let failed = result.receipt.outcome == GenerationOutcome::Failed;
                        let stopped = result.receipt.outcome == GenerationOutcome::Stopped;
                        let message = if failed {
                            result.receipt.error.unwrap_or_else(|| "Generation failed.".to_string())
                        } else if stopped {
                            if result.receipt.persisted { "Stopped. The partial response was saved.".to_string() } else { "Generation stopped.".to_string() }
                        } else {
                            String::new()
                        };
                        view! {
                            <Show when=move || failed || stopped>
                                <div class=format!("border-t px-4 py-2 text-sm {}", if failed { "border-destructive/30 bg-destructive/10" } else { "border-border bg-muted/40" }) role=move || if failed { "alert" } else { "status" }>
                                    <div class="mx-auto flex max-w-3xl items-start gap-2">
                                        <span class="min-w-0 flex-1 break-words py-2">{message.clone()}</span>
                                        <Show when=move || failed>
                                            <button type="button" class="min-h-11 shrink-0 rounded-md px-3 font-medium hover:bg-accent" on:click=move |_| start_regeneration.run(())>"Retry"</button>
                                        </Show>
                                        <button type="button" class="min-h-11 shrink-0 rounded-md px-2" aria-label="Dismiss generation status" on:click=move |_| notice_dismissed.set(true)>"✕"</button>
                                    </div>
                                </div>
                            </Show>
                        }
                    })}
                </Show>

                <ChatInput
                    session_id=sid
                    on_send=send_message
                    on_stop=stop_generation
                    is_streaming=is_generating
                    disabled=input_disabled
                    clear_revision=clear_revision.into()
                />
            </div>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::generation_tracker::submission_was_persisted;

    fn user_message(id: Uuid, content: &str) -> Message {
        Message {
            id,
            session_id: Uuid::new_v4(),
            role: MessageRole::User,
            content: content.to_string(),
            attachments: Vec::new(),
            model_used: None,
            provider_id: None,
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn repeated_text_does_not_count_an_old_message_as_the_new_submission() {
        let old_id = Uuid::new_v4();
        let submission = PendingSubmission {
            content: "same text".to_string(),
            image_count: 0,
            previous_user_id: Some(old_id),
        };
        assert!(!submission_was_persisted(
            &submission,
            &[user_message(old_id, "same text")]
        ));
        assert!(submission_was_persisted(
            &submission,
            &[
                user_message(old_id, "same text"),
                user_message(Uuid::new_v4(), "same text")
            ]
        ));
    }
}
