use leptos::prelude::*;
use leptos_router::components::A;
use uuid::Uuid;
use crate::models::character::Character;
use crate::components::chat::model_selector::ModelSelector;

#[component]
pub fn ChatHeader(
    #[prop(into)] character: Signal<Option<Character>>,
    selected_provider: RwSignal<Option<Uuid>>,
    selected_model: RwSignal<Option<String>>,
    #[prop(optional, into)] preferred_model: MaybeProp<String>,
) -> impl IntoView {
    let char_name = move || character.get().map(|c| c.name).unwrap_or_default();
    let char_role = move || character.get().and_then(|c| c.role);

    // Open the app sidebar drawer on mobile (context provided by MainLayout).
    let sidebar = use_context::<crate::components::layout::SidebarState>();

    let avatar = move || {
        let name = char_name();
        match character.get().and_then(|c| c.avatar_path) {
            Some(rel) => view! {
                <img
                    src=format!("/{rel}")
                    alt=""
                    class="h-8 w-8 shrink-0 rounded-full object-cover shadow-sm"
                />
            }.into_any(),
            None => view! {
                <div class=format!(
                    "flex h-8 w-8 shrink-0 items-center justify-center rounded-full \
                     bg-gradient-to-br {} text-xs font-medium text-white shadow-sm",
                    crate::components::avatar::gradient(&name)
                )>
                    {crate::components::avatar::initial(&name)}
                </div>
            }.into_any(),
        }
    };

    view! {
        <div class="sticky top-0 z-30 flex h-14 items-center justify-between gap-2 \
                    border-b border-border bg-background/95 backdrop-blur \
                    supports-[backdrop-filter]:bg-background/60 px-4">
            <div class="flex items-center gap-3 min-w-0">
                {sidebar.map(|s| {
                    let open = s.open;
                    view! {
                        <button
                            class="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md \
                                   text-foreground transition-colors hover:bg-accent md:hidden"
                            aria-label="Open menu"
                            on:click=move |_| open.set(true)
                        >
                            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24"
                                 fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
                                 stroke-linejoin="round">
                                <line x1="4" x2="20" y1="6" y2="6"/>
                                <line x1="4" x2="20" y1="12" y2="12"/>
                                <line x1="4" x2="20" y1="18" y2="18"/>
                            </svg>
                        </button>
                    }
                })}
                {avatar}

                <div class="min-w-0">
                    <p class="text-sm font-semibold truncate">{char_name}</p>
                    {move || char_role().map(|role| view! {
                        <p class="text-xs text-muted-foreground truncate">{role}</p>
                    })}
                </div>
            </div>

            <div class="flex items-center gap-2 shrink-0">
                <ModelSelector
                    selected_provider=selected_provider
                    selected_model=selected_model
                    preferred_model=preferred_model
                />
                <A
                    href="/settings"
                    attr:class="inline-flex items-center justify-center rounded-md text-sm font-medium \
                           transition-colors hover:bg-accent hover:text-accent-foreground h-8 w-8 shrink-0"
                    attr:aria-label="Settings"
                >
                    <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24"
                         fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
                         stroke-linejoin="round">
                        <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/>
                        <circle cx="12" cy="12" r="3"/>
                    </svg>
                </A>
            </div>
        </div>
    }
}
