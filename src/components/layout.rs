use crate::components::sidebar::sidebar::Sidebar;
use crate::components::ui::sheet::{Sheet, SheetSide};
use leptos::prelude::*;
use leptos_router::components::Outlet;

#[derive(Clone)]
pub struct SidebarState {
    pub open: RwSignal<bool>,
}

#[derive(Clone)]
pub struct CurrentSession(pub RwSignal<Option<uuid::Uuid>>);

#[component]
pub fn MainLayout() -> impl IntoView {
    let sidebar_open = RwSignal::new(false);
    // Provided at app scope (see AppRoutes) so services like the generation
    // tracker can tell which session is currently on screen.
    let CurrentSession(current_session) = expect_context::<CurrentSession>();

    provide_context(SidebarState { open: sidebar_open });

    // A resize across the desktop breakpoint must not leave a hidden drawer
    // holding focus or the body's scroll lock.
    #[cfg(feature = "hydrate")]
    {
        use leptos::wasm_bindgen::closure::Closure;
        use leptos::wasm_bindgen::JsCast;

        if let Some(media) = leptos::web_sys::window()
            .and_then(|window| window.match_media("(min-width: 1024px)").ok().flatten())
        {
            if media.matches() {
                sidebar_open.set(false);
            }
            let media_for_callback = media.clone();
            let callback = Closure::<dyn FnMut()>::new(move || {
                if media_for_callback.matches() {
                    sidebar_open.set(false);
                }
            });
            media.set_onchange(Some(callback.as_ref().unchecked_ref()));
            // Stored with this layout's owner, so both the media query object
            // and its callback are released together on navigation/disposal.
            let _media_listener = StoredValue::new_local((media, callback));
        }
    }

    // Chat sessions render their own header (with a menu button on mobile),
    // so the generic mobile top bar is hidden there to avoid stacked chrome.
    let location = leptos_router::hooks::use_location();
    let on_chat_session = Signal::derive(move || location.pathname.get().starts_with("/chat/"));

    view! {
        <div class="relative flex h-viewport overflow-hidden bg-background text-foreground">
            // Desktop sidebar. Keeping it below lg in a drawer also prevents
            // landscape phones from losing most of their content width.
            <aside class="hidden lg:fixed lg:inset-y-0 lg:z-40 lg:flex lg:w-80 lg:flex-col lg:pb-safe">
                <Sidebar current_session_id=current_session.into() />
            </aside>

            // Mobile sidebar drawer (Sheet). Closes on navigate / overlay / Escape.
            <Sheet open=sidebar_open side=SheetSide::Left label="Navigation menu">
                <Sidebar
                    current_session_id=current_session.into()
                    on_navigate=Callback::new(move |_| sidebar_open.set(false))
                    on_close=Callback::new(move |_| sidebar_open.set(false))
                />
            </Sheet>

            // Content column. Offset by the fixed sidebar width on desktop.
            <div class="flex min-w-0 flex-1 flex-col lg:pl-80">
                // Mobile top bar — provides the sidebar toggle on every page
                // except chat sessions, whose own header has one.
                <header class=move || format!(
                    "{} h-14 shrink-0 items-center gap-3 border-b border-border \
                     bg-background/95 px-4 pt-safe backdrop-blur lg:hidden",
                    if on_chat_session.get() { "hidden" } else { "flex" }
                )>
                    <button
                        class="inline-flex h-9 w-9 items-center justify-center rounded-md \
                               text-foreground transition-colors hover:bg-accent"
                        aria-label="Open menu"
                        on:click=move |_| sidebar_open.set(true)
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24"
                             fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
                             stroke-linejoin="round">
                            <line x1="4" x2="20" y1="6" y2="6"/>
                            <line x1="4" x2="20" y1="12" y2="12"/>
                            <line x1="4" x2="20" y1="18" y2="18"/>
                        </svg>
                    </button>
                    <img src="/mark.png" alt="" class="h-5 w-auto shrink-0" />
                    <span class="font-semibold">"NanoRP"</span>
                </header>

                <main class="min-h-0 flex-1 overflow-hidden pb-safe">
                    <Outlet />
                </main>
            </div>
        </div>
    }
}
