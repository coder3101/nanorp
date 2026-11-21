use crate::components::chat::message_bubble::MessageBubble;
use crate::components::ui::scroll_area::ScrollArea;
use crate::models::message::{Message, MessageRole};
use leptos::html;
use leptos::prelude::*;
use std::hash::{Hash, Hasher};
use uuid::Uuid;

/// Follow new output only while the user remains pinned to the latest reply.
/// An upward position change always wins, even if the browser's scroll event
/// has not yet updated `is_at_bottom`.
fn should_follow(top: f64, last_pinned_top: f64, is_at_bottom: bool) -> bool {
    is_at_bottom && top + 1.0 >= last_pinned_top
}

fn message_key(message: &Message) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    message.id.hash(&mut hash);
    // Hash the actual contents, not their length: a same-length edit must not
    // reuse a bubble that captured the previous string.
    message.content.hash(&mut hash);
    message.model_used.hash(&mut hash);
    for attachment in &message.attachments {
        attachment.id.hash(&mut hash);
        attachment.file_path.hash(&mut hash);
    }
    hash.finish()
}

/// Only the latest completed assistant response after a real user turn is a
/// valid target for the backend's regenerate-from-last-user operation.
fn regeneratable_message(messages: &[Message]) -> Option<Uuid> {
    let (last_index, last) = messages.iter().enumerate().next_back()?;
    if last.role != MessageRole::Assistant {
        return None;
    }
    (last_index > 0 && messages[last_index - 1].role == MessageRole::User).then_some(last.id)
}

#[component]
pub fn MessageList(
    messages: Signal<Vec<Message>>,
    session_id: Signal<Option<Uuid>>,
    #[prop(into, optional)] character_name: MaybeProp<String>,
    #[prop(into, optional)] character_avatar: MaybeProp<String>,
    #[prop(optional)] streaming_msg_id: Signal<Option<Uuid>>,
    #[prop(optional)] streaming_content: Signal<String>,
    #[prop(optional, into)] render_thinking: MaybeProp<bool>,
    #[prop(optional)] loading: Signal<bool>,
    #[prop(optional)] actions_disabled: Signal<bool>,
    #[prop(optional)] on_edit: Option<Callback<crate::models::message::EditPayload>>,
    #[prop(optional)] on_regenerate: Option<Callback<Uuid>>,
) -> impl IntoView {
    let scroll_ref = NodeRef::<html::Div>::new();
    let is_at_bottom = RwSignal::new(true);
    // True when content arrived below the fold while the reader was scrolled up,
    // so the jump button can advertise that there is something new to see.
    let new_below = RwSignal::new(false);
    let last_pinned_top = StoredValue::new(0.0_f64);
    let threshold = 100.0;

    let on_scroll = Callback::new(move |_: ()| {
        if let Some(el) = scroll_ref.get_untracked() {
            let top = el.scroll_top() as f64;
            let max = (el.scroll_height() - el.client_height()).max(0) as f64;
            let at_bottom = (max - top) < threshold;
            is_at_bottom.set(at_bottom);
            if at_bottom {
                last_pinned_top.set_value(top);
                new_below.set(false);
            }
        }
    });

    // Wheel/touch intent arrives before the asynchronous scroll event. Release
    // following immediately so streaming can never drag the reader back down.
    let on_scroll_up = Callback::new(move |_: ()| {
        is_at_bottom.set(false);
    });

    Effect::new(move |_| {
        let _ = session_id.get();
        let _ = messages.get();
        let _ = streaming_content.get();

        if let Some(el) = scroll_ref.get() {
            let top = el.scroll_top() as f64;
            let max = (el.scroll_height() - el.client_height()).max(0);
            if should_follow(
                top,
                last_pinned_top.get_value(),
                is_at_bottom.get_untracked(),
            ) {
                el.set_scroll_top(max);
                last_pinned_top.set_value(max as f64);
            } else {
                // Content grew below the fold while the reader was scrolled up.
                new_below.set(true);
            }
        }
    });

    let scroll_to_bottom = move |_| {
        if let Some(el) = scroll_ref.get() {
            let max = (el.scroll_height() - el.client_height()).max(0);
            el.set_scroll_top(max);
            last_pinned_top.set_value(max as f64);
            is_at_bottom.set(true);
            new_below.set(false);
        }
    };

    let has_messages = Signal::derive(move || !messages.get().is_empty());
    let regeneratable = Signal::derive(move || regeneratable_message(&messages.get()));

    view! {
        <div class="relative min-h-0 flex-1">
            <ScrollArea class="h-full" node_ref=scroll_ref on_scroll=on_scroll on_scroll_up=on_scroll_up>
                <div class="mx-auto w-full max-w-3xl py-3 sm:py-6">
                    <Show when=move || loading.get()>
                        <div class="space-y-4 px-4 py-6" role="status" aria-label="Loading conversation">
                            <div class="ml-auto h-16 w-3/4 animate-pulse rounded-2xl bg-muted"></div>
                            <div class="h-24 w-5/6 animate-pulse rounded-2xl bg-muted"></div>
                            <div class="ml-auto h-12 w-2/3 animate-pulse rounded-2xl bg-muted"></div>
                        </div>
                    </Show>
                    <Show when=move || !loading.get()>
                        <Show
                            when=move || has_messages.get()
                            fallback={
                                let cname = character_name;
                                move || {
                                    let name = cname.get().filter(|s| !s.is_empty());
                                    let sub = match name {
                                        Some(n) => format!("Say hello to {n} to begin."),
                                        None => "Send a message to begin chatting.".to_string(),
                                    };
                                    view! {
                                        <div class="flex min-h-[45dvh] flex-col items-center justify-center px-4 text-center">
                                            <div class="mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-muted text-muted-foreground">
                                                <svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7.9 20A9 9 0 1 0 4 16.1L2 22Z"/></svg>
                                            </div>
                                            <p class="text-base font-medium">"Start the conversation"</p>
                                            <p class="mt-1 text-sm text-muted-foreground">{sub}</p>
                                        </div>
                                    }
                                }
                            }
                        >
                            <For
                                each=move || messages.get()
                                key=|message| message_key(message)
                                let:message
                            >
                                {
                                    let message_id = message.id;
                                    let later_count = messages.get_untracked()
                                        .iter()
                                        .position(|candidate| candidate.id == message_id)
                                        .map(|index| messages.get_untracked().len().saturating_sub(index + 1))
                                        .unwrap_or(0);
                                    view! {
                                        <MessageBubble
                                            message=message.clone()
                                            character_name=character_name.get().unwrap_or_default()
                                            character_avatar=character_avatar.get().unwrap_or_default()
                                            is_streaming=Signal::derive(move || streaming_msg_id.get() == Some(message_id))
                                            streaming_content=streaming_content
                                            render_thinking=render_thinking.get().unwrap_or(true)
                                            later_message_count=if actions_disabled.get_untracked() { 0 } else { later_count }
                                            on_edit=if actions_disabled.get_untracked() { None } else { on_edit }
                                            on_regenerate=if actions_disabled.get_untracked() || regeneratable.get_untracked() != Some(message_id) {
                                                None
                                            } else {
                                                on_regenerate
                                            }
                                        />
                                    }
                                }
                            </For>
                        </Show>
                    </Show>
                </div>
            </ScrollArea>

            <Show when=move || !is_at_bottom.get() && has_messages.get()>
                <div class="absolute bottom-4 left-1/2 z-10 -translate-x-1/2">
                    <Show
                        when=move || new_below.get()
                        fallback=move || view! {
                            <button
                                type="button"
                                class="inline-flex h-11 w-11 items-center justify-center rounded-full border border-border \
                                       bg-background text-foreground shadow-md transition-colors hover:bg-accent \
                                       focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                                aria-label="Scroll to latest message"
                                on:click=scroll_to_bottom
                            >
                                <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24"
                                     fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                    <path d="M12 5v14"/>
                                    <path d="m19 12-7 7-7-7"/>
                                </svg>
                            </button>
                        }
                    >
                        <button
                            type="button"
                            class="inline-flex h-11 items-center gap-1.5 rounded-full border border-border \
                                   bg-primary pl-4 pr-3 text-sm font-medium text-primary-foreground shadow-md \
                                   transition-colors hover:bg-primary/90 animate-fade-in \
                                   focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                            aria-label="Scroll to new messages"
                            on:click=scroll_to_bottom
                        >
                            "New messages"
                            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24"
                                 fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                <path d="M12 5v14"/>
                                <path d="m19 12-7 7-7-7"/>
                            </svg>
                        </button>
                    </Show>
                </div>
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn message(role: MessageRole, content: &str) -> Message {
        Message {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            role,
            content: content.to_string(),
            attachments: Vec::new(),
            model_used: None,
            provider_id: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn follows_while_pinned_to_bottom() {
        assert!(should_follow(1000.0, 1000.0, true));
    }

    #[test]
    fn stops_when_user_scrolled_up() {
        assert!(!should_follow(900.0, 1000.0, true));
    }

    #[test]
    fn stops_when_not_at_bottom() {
        assert!(!should_follow(1000.0, 1000.0, false));
    }

    #[test]
    fn tolerates_a_pixel_of_rounding() {
        assert!(should_follow(999.0, 1000.0, true));
    }

    #[test]
    fn same_length_content_changes_the_render_key() {
        let original = message(MessageRole::User, "one");
        let mut edited = original.clone();
        edited.content = "two".to_string();
        assert_ne!(message_key(&original), message_key(&edited));
    }

    #[test]
    fn only_latest_user_driven_response_can_regenerate() {
        let greeting = message(MessageRole::Assistant, "hello");
        let user = message(MessageRole::User, "question");
        let answer = message(MessageRole::Assistant, "answer");
        assert_eq!(
            regeneratable_message(&[greeting, user, answer.clone()]),
            Some(answer.id)
        );
        assert_eq!(
            regeneratable_message(&[message(MessageRole::Assistant, "greeting")]),
            None
        );
        assert_eq!(
            regeneratable_message(&[message(MessageRole::User, "pending")]),
            None
        );
        assert_eq!(
            regeneratable_message(&[
                message(MessageRole::User, "question"),
                message(MessageRole::Assistant, "first"),
                message(MessageRole::Assistant, "second")
            ]),
            None
        );
    }
}
