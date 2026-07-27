use crate::components::sidebar::chat_list::ChatList;
use crate::components::sidebar::nav::Nav;
use crate::components::ui::toast::use_toast;
use crate::models::chat::ChatSummary;
use crate::server::chat::{delete_chat_session, list_chat_sessions};
use leptos::callback::Callable;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;
use leptos_router::components::A;
use uuid::Uuid;

#[component]
pub fn Sidebar(
    #[prop(optional)] on_navigate: Option<Callback<()>>,
    #[prop(optional)] current_session_id: Signal<Option<Uuid>>,
) -> impl IntoView {
    let toast = use_toast();

    // Bumped to force the list to refetch after a deletion.
    let refetch = RwSignal::new(0u32);

    // Reload the chat list whenever the active session changes (covers newly
    // created sessions and updated previews on send) or after a deletion.
    let sessions = LocalResource::new(move || {
        let _ = current_session_id.get();
        let _ = refetch.get();
        async move {
            list_chat_sessions(None, 100, 0)
                .await
                .map_err(|e| e.to_string())
        }
    });
    let load_error = Signal::derive(move || sessions.get().and_then(|r| r.err()));
    let all_chats = Signal::derive(move || {
        sessions.get().and_then(|r| r.ok()).unwrap_or_default() as Vec<ChatSummary>
    });

    // Client-side filter over the loaded conversations.
    let query = RwSignal::new(String::new());
    let chats = Signal::derive(move || {
        let q = query.get().trim().to_lowercase();
        let list = all_chats.get();
        if q.is_empty() {
            return list;
        }
        list.into_iter()
            .filter(|c| {
                c.character_name.to_lowercase().contains(&q)
                    || c.title
                        .as_deref()
                        .is_some_and(|t| t.to_lowercase().contains(&q))
                    || c.last_message
                        .as_deref()
                        .is_some_and(|m| m.to_lowercase().contains(&q))
            })
            .collect()
    });
    let empty_text = Signal::derive(move || {
        if query.get().trim().is_empty() {
            "No conversations yet. Select a character to start chatting.".to_string()
        } else {
            "No conversations match your search.".to_string()
        }
    });

    let notify = on_navigate.unwrap_or_else(|| Callback::new(|_| {}));

    let on_new_chat = move |_| {
        notify.run(());
    };

    // Delete a conversation: call the backend, refetch, and navigate away if
    // the deleted conversation is the one currently open.
    let on_delete = Callback::new(move |session_id: Uuid| {
        let toast = toast.clone();
        leptos::task::spawn_local(async move {
            match delete_chat_session(session_id).await {
                Ok(()) => {
                    toast.success("Conversation deleted");
                    if current_session_id.get_untracked() == Some(session_id) {
                        let navigate = leptos_router::hooks::use_navigate();
                        navigate("/characters", Default::default());
                    }
                    refetch.update(|v| *v += 1);
                }
                Err(e) => toast.error(format!("Delete failed: {e}")),
            }
        });
    });

    view! {
        <div class="flex h-full flex-col border-r border-border bg-muted/40">
            <A href="/" attr:class="flex h-14 items-center gap-2 border-b border-border px-4 no-underline">
                // Simplified mark, not the full logo: at nav size the detailed
                // artwork is illegible.
                <img src="/mark.png" alt="" class="h-5 w-auto shrink-0" />
                <h1 class="font-semibold text-lg tracking-tight text-foreground">"NanoRP"</h1>
            </A>

            <div class="p-3">
                <A
                    href="/characters"
                    attr:class="inline-flex w-full items-center justify-center gap-2 rounded-lg text-sm font-medium \
                           transition-colors focus-visible:outline-none focus-visible:ring-2 \
                           focus-visible:ring-ring bg-primary text-primary-foreground shadow \
                           hover:bg-primary/90 h-10 px-4 no-underline"
                    on:click=on_new_chat
                >
                    <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24"
                         fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M5 12h14M12 5v14"/>
                    </svg>
                    "New chat"
                </A>
            </div>

            // Search over conversations (shown once there's anything to filter).
            <Show when=move || !all_chats.get().is_empty()>
                <div class="px-3 pb-2">
                    <div class="relative">
                        <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24"
                             fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
                             class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground">
                            <circle cx="11" cy="11" r="8"/>
                            <path d="m21 21-4.3-4.3"/>
                        </svg>
                        <input
                            type="search"
                            class="h-9 w-full rounded-lg border border-input bg-background pl-8 pr-3 text-sm \
                                   shadow-sm transition-colors placeholder:text-muted-foreground \
                                   focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                            placeholder="Search chats..."
                            aria-label="Search conversations"
                            prop:value=move || query.get()
                            on:input=move |ev| {
                                let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                                query.set(input.value());
                            }
                        />
                    </div>
                </div>
            </Show>

            <div class="flex-1 overflow-hidden">
                <Show
                    when=move || load_error.get().is_none()
                    fallback=move || view! {
                        <div class="mx-3 mt-2 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm">
                            <p class="font-medium text-destructive">"Couldn't load conversations"</p>
                            <p class="mt-0.5 break-words text-xs text-muted-foreground">
                                {move || load_error.get().unwrap_or_default()}
                            </p>
                            <button
                                class="mt-2 inline-flex h-7 items-center rounded-md border border-input bg-background \
                                       px-2.5 text-xs font-medium shadow-sm transition-colors hover:bg-accent"
                                on:click=move |_| refetch.update(|v| *v += 1)
                            >
                                "Retry"
                            </button>
                        </div>
                    }
                >
                    <ChatList
                        current_session_id=current_session_id
                        on_select=notify
                        on_delete=on_delete
                        chats=chats
                        empty_text=empty_text
                    />
                </Show>
            </div>

            <div class="border-t border-border p-2">
                <Nav on_navigate=notify />
            </div>
        </div>
    }
}
