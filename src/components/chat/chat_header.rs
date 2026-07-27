use leptos::prelude::*;
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

            <div class="flex items-center shrink-0">
                <ModelSelector
                    selected_provider=selected_provider
                    selected_model=selected_model
                    preferred_model=preferred_model
                />
            </div>
        </div>
    }
}
