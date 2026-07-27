use leptos::prelude::*;
use leptos::html;
use uuid::Uuid;
use crate::models::message::Message;
use crate::components::ui::scroll_area::ScrollArea;
use crate::components::chat::message_bubble::MessageBubble;

#[component]
pub fn MessageList(
    messages: Signal<Vec<Message>>,
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
    let threshold = 100.0;

    let on_scroll = Callback::new(move |_: ()| {
        if let Some(el) = scroll_ref.get_untracked() {
            let at_bottom = (el.scroll_height() as f64 - el.scroll_top() as f64 - el.client_height() as f64) < threshold;
            is_at_bottom.set(at_bottom);
        }
    });

    let scroll_ref_for_effect = scroll_ref;
    let is_at_bottom_for_effect = is_at_bottom;
    Effect::new(move |_| {
        let _ = messages.get();
        let _ = streaming_content.get();
        if is_at_bottom_for_effect.get_untracked() {
            if let Some(el) = scroll_ref_for_effect.get() {
                el.set_scroll_top(el.scroll_height());
            }
        }
    });

    let scroll_to_bottom = move |_| {
        if let Some(el) = scroll_ref.get() {
            el.set_scroll_top(el.scroll_height());
            is_at_bottom.set(true);
        }
    };

    let has_messages = Signal::derive(move || !messages.get().is_empty());
    let cname = character_name;

    view! {
        <div class="relative min-h-0 flex-1">
            <ScrollArea class="h-full" node_ref=scroll_ref on_scroll=on_scroll>
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
