use leptos::prelude::*;
use leptos::callback::Callable;
use leptos_router::components::A;
use leptos_router::hooks::use_location;

#[component]
pub fn Nav(
    #[prop(optional)] on_navigate: Option<Callback<()>>,
) -> impl IntoView {
    let location = use_location();

    let on_click = move |_| {
        if let Some(cb) = on_navigate {
            cb.run(());
        }
    };

    let item_class = move |target: &'static str| {
        let pathname = location.pathname.get();
        let base = "flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium \
                    transition-colors hover:bg-accent hover:text-accent-foreground";
        if pathname.starts_with(target) {
            format!("{} bg-accent text-accent-foreground", base)
        } else {
            format!("{} text-muted-foreground", base)
        }
    };

    view! {
        <nav class="flex flex-col gap-1">
            <A href="/characters" attr:class=move || item_class("/characters") on:click=on_click>
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24"
                     fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
                     stroke-linejoin="round" class="shrink-0">
                    <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/>
                    <circle cx="9" cy="7" r="4"/>
                    <path d="M22 21v-2a4 4 0 0 0-3-3.87"/>
                    <path d="M16 3.13a4 4 0 0 1 0 7.75"/>
                </svg>
                "Characters"
            </A>
            <A href="/settings" attr:class=move || item_class("/settings") on:click=on_click>
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24"
                     fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
                     stroke-linejoin="round" class="shrink-0">
                    <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/>
                    <circle cx="12" cy="12" r="3"/>
                </svg>
                "Settings"
            </A>
        </nav>
    }
}
