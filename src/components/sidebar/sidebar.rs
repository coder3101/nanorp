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

/// Conversations per request, and the step size of "load more".
const PAGE_SIZE: u32 = 30;

/// How long typing has to settle before a search reaches the server.
const SEARCH_DEBOUNCE_MS: u64 = 250;

#[component]
pub fn Sidebar(
    #[prop(optional)] on_navigate: Option<Callback<()>>,
    #[prop(optional)] current_session_id: Signal<Option<Uuid>>,
) -> impl IntoView {
    let toast = use_toast();

    // The pages loaded so far, newest first.
    let chats = RwSignal::new(Vec::<ChatSummary>::new());
    let load_error = RwSignal::new(None::<String>);
    let loading = RwSignal::new(false);
    // The last response filled the page it asked for, so assume there's more.
    let has_more = RwSignal::new(false);
    // Each request claims the next id and a response carrying a stale one is
    // discarded, so a slow early search can't land on top of a later one.
    let request_id = StoredValue::new(0u32);
    // Rows *returned* so far, which is where the next page starts. Counting the
    // rows we kept instead would stall if a whole page were deduped away.
    let next_offset = StoredValue::new(0u32);

    let query = RwSignal::new(String::new());
    // `query` once typing settles — this is what actually gets sent.
    let applied_query = RwSignal::new(String::new());

    // Bumped to force a refresh: after a deletion, or from the error retry.
    let refetch = RwSignal::new(0u32);

    let fetch = move |offset: u32, limit: u32, replace: bool| {
        let id = request_id.get_value() + 1;
        request_id.set_value(id);
        loading.set(true);

        let search = applied_query.get_untracked();
        let search = (!search.is_empty()).then_some(search);

        leptos::task::spawn_local(async move {
            let result = list_chat_sessions(None, search, limit, offset).await;
            if request_id.get_value() != id {
                return;
            }
            loading.set(false);
            match result {
                Ok(page) => {
                    load_error.set(None);
                    has_more.set(page.len() as u32 == limit);
                    if replace {
                        next_offset.set_value(page.len() as u32);
                        chats.set(page);
                    } else {
                        next_offset.update_value(|o| *o += page.len() as u32);
                        chats.update(|list| {
                            for chat in page {
                                // Rows shift position as `updated_at` changes,
                                // so an offset page can hand back a row we
                                // already hold — and `For` needs unique keys.
                                if !list.iter().any(|c| c.session_id == chat.session_id) {
                                    list.push(chat);
                                }
                            }
                        });
                    }
                }
                Err(e) => load_error.set(Some(e.to_string())),
            }
        });
    };

    // Re-request as many rows as are already on screen instead of collapsing to
    // the first page, so a refresh doesn't undo the user's scrolling.
    let reload = move || {
        let loaded = chats.get_untracked().len() as u32;
        fetch(0, loaded.max(PAGE_SIZE), true);
    };

    // Initial load, then whenever the active session changes (a new
    // conversation has to appear, and previews and ordering shift), a search
    // settles, or something asks for a refresh.
    Effect::new(move |_| {
        let _ = current_session_id.get();
        let _ = applied_query.get();
        let _ = refetch.get();
        reload();
    });

    let load_more = Callback::new(move |_| {
        if loading.get_untracked() || !has_more.get_untracked() {
            return;
        }
        fetch(next_offset.get_value(), PAGE_SIZE, false);
    });

    // Debounced so a search doesn't fire a request per keystroke.
    let debounce = StoredValue::new_local(None::<leptos::leptos_dom::helpers::TimeoutHandle>);
    let on_search_input = move |ev: leptos::ev::Event| {
        let input = ev
            .target()
            .unwrap()
            .unchecked_into::<leptos::web_sys::HtmlInputElement>();
        query.set(input.value());

        if let Some(handle) = debounce.get_value() {
            handle.clear();
        }
        debounce.set_value(
            leptos::leptos_dom::helpers::set_timeout_with_handle(
                move || applied_query.set(query.get_untracked().trim().to_string()),
                std::time::Duration::from_millis(SEARCH_DEBOUNCE_MS),
            )
            .ok(),
        );
    };

    // Keep the box mounted while a search is active even if it matched nothing,
    // otherwise there'd be no way to clear it.
    let show_search =
        Signal::derive(move || !chats.get().is_empty() || !query.get().trim().is_empty());

    let empty_text = Signal::derive(move || {
        if loading.get() {
            "Loading conversations…".to_string()
        } else if query.get().trim().is_empty() {
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

            // Searches every conversation on the server, not just the pages
            // already loaded.
            <Show when=move || show_search.get()>
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
                            on:input=on_search_input
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
                        has_more=has_more
                        loading=loading
                        on_load_more=load_more
                    />
                </Show>
            </div>

            <div class="border-t border-border p-2">
                <Nav on_navigate=notify />
            </div>
        </div>
    }
}
