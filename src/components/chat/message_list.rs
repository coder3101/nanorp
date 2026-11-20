use crate::components::chat::message_bubble::MessageBubble;
use crate::components::ui::scroll_area::ScrollArea;
use crate::models::message::Message;
use leptos::html;
use leptos::prelude::*;
use uuid::Uuid;

/// Whether a content update should pull the view back to the bottom.
///
/// `scrollTop` changes synchronously when the user scrolls, but the `scroll`
/// event that updates `is_at_bottom` can arrive a frame later. A position above
/// the one we last pinned to is therefore an intentional scroll-up regardless
/// of the (possibly stale) flag.
fn should_follow(top: f64, last_pinned_top: f64, is_at_bottom: bool) -> bool {
    is_at_bottom && top + 1.0 >= last_pinned_top
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
    #[prop(optional)] on_edit: Option<Callback<crate::models::message::EditPayload>>,
    #[prop(optional)] on_regenerate: Option<Callback<Uuid>>,
) -> impl IntoView {
    let scroll_ref = NodeRef::<html::Div>::new();
    let is_at_bottom = RwSignal::new(true);
    // The `scrollTop` we last pinned the view to. Streaming updates can outrun
    // the browser's async `scroll` event, so checking this position lets us
    // notice an upward scroll before `is_at_bottom` catches up.
    let last_pinned_top = StoredValue::new(0.0_f64);
    // ChatPage is reused when navigating between sessions, so the follow state
    // has to be reset whenever the session changes.
    let last_session = StoredValue::new(None::<Uuid>);
    let threshold = 100.0;

    let on_scroll = Callback::new(move |_: ()| {
        if let Some(el) = scroll_ref.get_untracked() {
            let top = el.scroll_top() as f64;
            let max = (el.scroll_height() - el.client_height()).max(0) as f64;
            let at_bottom = (max - top) < threshold;
            is_at_bottom.set(at_bottom);
            // Keep the pinned position current whenever the user settles at the
            // bottom, so a later upward scroll is measured against it.
            if at_bottom {
                last_pinned_top.set_value(top);
            }
        }
    });

    // ScrollArea reports upward wheel/touch intent before the deferred
    // `scroll` event, so pinning is released immediately.
    let on_scroll_up = Callback::new(move |_: ()| is_at_bottom.set(false));

    let scroll_ref_for_effect = scroll_ref;
    let is_at_bottom_for_effect = is_at_bottom;
    let last_pinned_top_for_effect = last_pinned_top;
    let last_session_for_effect = last_session;
    let session_id_for_effect = session_id;
    Effect::new(move |_| {
        let session = session_id_for_effect.get();
        let _ = messages.get();
        let _ = streaming_content.get();

        if last_session_for_effect.get_value() != session {
            last_session_for_effect.set_value(session);
            is_at_bottom_for_effect.set(true);
            last_pinned_top_for_effect.set_value(0.0);
        }

        if let Some(el) = scroll_ref_for_effect.get() {
            let top = el.scroll_top() as f64;
            let max = (el.scroll_height() - el.client_height()).max(0);
            if should_follow(
                top,
                last_pinned_top_for_effect.get_value(),
                is_at_bottom_for_effect.get_untracked(),
            ) {
                el.set_scroll_top(max);
                last_pinned_top_for_effect.set_value(max as f64);
            }
        }
    });

    let scroll_to_bottom = move |_| {
        if let Some(el) = scroll_ref.get() {
            let max = (el.scroll_height() - el.client_height()).max(0);
            el.set_scroll_top(max);
            last_pinned_top.set_value(max as f64);
            is_at_bottom.set(true);
        }
    };

    let has_messages = Signal::derive(move || !messages.get().is_empty());
    let cname = character_name;

    view! {
        <div class="relative min-h-0 flex-1">
            <ScrollArea class="h-full" node_ref=scroll_ref on_scroll=on_scroll on_scroll_up=on_scroll_up>
                <div class="mx-auto w-full max-w-3xl py-6">
                    <Show
                        when=move || has_messages.get()
                        fallback={
                            let cname = cname;
                            move || {
                                let name = cname.get().filter(|s| !s.is_empty());
                                let sub = match name {
                                    Some(n) => format!("Say hello to {n} to begin."),
                                    None => "Send a message to begin chatting.".to_string(),
                                };
                                view! {
                                    <div class="flex min-h-[50vh] flex-col items-center justify-center px-4 text-center">
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
                            // Key includes a fingerprint of the mutable parts so an
                            // edited message (same id, changed content/attachments)
                            // is re-rendered rather than reused by <For>.
                            key=|msg| {
                                let atts: String = msg
                                    .attachments
                                    .iter()
                                    .map(|a| a.id.to_string())
                                    .collect::<Vec<_>>()
                                    .join(",");
                                format!("{}:{}:{}", msg.id, msg.content.len(), atts)
                            }
                            let:msg
                        >
                            <MessageBubble
                                message=msg.clone()
                                character_name=character_name.get().unwrap_or_default()
                                character_avatar=character_avatar.get().unwrap_or_default()
                                is_streaming=Signal::derive(move || {
                                    streaming_msg_id.get() == Some(msg.id)
                                })
                                streaming_content=streaming_content
                                render_thinking=render_thinking.get().unwrap_or(true)
                                on_edit=on_edit
                                on_regenerate=on_regenerate
                            />
                        </For>
                    </Show>
                </div>
            </ScrollArea>

            <Show when=move || !is_at_bottom.get() && has_messages.get()>
                <div class="absolute bottom-4 left-1/2 z-10 -translate-x-1/2">
                    <button
                        class="inline-flex h-9 w-9 items-center justify-center rounded-full border border-border \
                               bg-background text-foreground shadow-md transition-colors hover:bg-accent"
                        aria-label="Scroll to bottom"
                        on:click=scroll_to_bottom
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24"
                             fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M12 5v14"/>
                            <path d="m19 12-7 7-7-7"/>
                        </svg>
                    </button>
                </div>
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::should_follow;

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
}
