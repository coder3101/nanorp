use leptos::prelude::*;
use leptos_router::components::A;

/// A simple landing page shown at `/`. Introduces the app and points the user
/// toward starting a chat or managing characters.
#[component]
pub fn HomePage() -> impl IntoView {
    view! {
        <div class="flex h-full flex-col overflow-y-auto scroll-area p-6">
            <div class="mx-auto my-auto flex max-w-xl flex-col items-center py-4 text-center">
                // Logo mark
                <img src="/logo.png" alt="" class="mb-6 h-20 w-20 rounded-2xl object-contain" />

                <h1 class="text-4xl font-bold tracking-tight">"Welcome to NanoRP"</h1>
                <p class="mt-3 max-w-md text-base text-muted-foreground">
                    "A cozy home for AI roleplay. Create characters with their own \
                     personalities, then chat with them using your favorite local or hosted models."
                </p>

                <div class="mt-8 flex flex-wrap items-center justify-center gap-3">
                    <A
                        href="/characters"
                        attr:class="inline-flex h-11 items-center justify-center gap-2 rounded-lg \
                               bg-primary px-5 text-sm font-medium text-primary-foreground shadow \
                               transition-colors hover:bg-primary/90 focus-visible:outline-none \
                               focus-visible:ring-2 focus-visible:ring-ring no-underline"
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" width="17" height="17" viewBox="0 0 24 24"
                             fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/>
                            <circle cx="9" cy="7" r="4"/>
                            <path d="M22 21v-2a4 4 0 0 0-3-3.87"/>
                            <path d="M16 3.13a4 4 0 0 1 0 7.75"/>
                        </svg>
                        "Start chatting"
                    </A>
                    <A
                        href="/settings"
                        attr:class="inline-flex h-11 items-center justify-center gap-2 rounded-lg \
                               border border-input bg-background px-5 text-sm font-medium shadow-sm \
                               transition-colors hover:bg-accent hover:text-accent-foreground \
                               focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring no-underline"
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" width="17" height="17" viewBox="0 0 24 24"
                             fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/>
                            <circle cx="12" cy="12" r="3"/>
                        </svg>
                        "Settings"
                    </A>
                </div>

                // Quick feature highlights
                <div class="mt-12 grid w-full grid-cols-1 gap-4 sm:grid-cols-3">
                    <HomeFeature
                        title="Your characters"
                        body="Craft personas with avatars, personalities, and custom prompts."
                        icon=view! {
                            <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></svg>
                        }.into_any()
                    />
                    <HomeFeature
                        title="Any model"
                        body="Connect Ollama or any OpenAI-compatible endpoint and switch freely."
                        icon=view! {
                            <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="8" x="2" y="2" rx="2"/><rect width="20" height="8" x="2" y="14" rx="2"/><path d="M6 6h.01M6 18h.01"/></svg>
                        }.into_any()
                    />
                    <HomeFeature
                        title="Private & local"
                        body="Everything is stored on your machine. Your chats stay yours."
                        icon=view! {
                            <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>
                        }.into_any()
                    />
                </div>

                // Baked in at compile time, so it always matches the running build.
                <p class="mt-10 text-xs text-muted-foreground">
                    {concat!("v", env!("CARGO_PKG_VERSION"))}
                </p>
            </div>
        </div>
    }
}

#[component]
fn HomeFeature(title: &'static str, body: &'static str, icon: AnyView) -> impl IntoView {
    view! {
        <div class="rounded-xl border border-border bg-card p-4 text-left shadow-sm">
            <div class="mb-2 flex h-9 w-9 items-center justify-center rounded-lg bg-muted text-foreground">
                {icon}
            </div>
            <h3 class="text-sm font-semibold">{title}</h3>
            <p class="mt-1 text-xs text-muted-foreground">{body}</p>
        </div>
    }
}
