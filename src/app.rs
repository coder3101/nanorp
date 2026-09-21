use crate::components::layout::{CurrentSession, MainLayout};
use crate::components::ui::toast::{ToastProvider, Toaster};
use crate::pages::characters_page::CharactersPage;
use crate::pages::chat_page::ChatPage;
use crate::pages::home_page::HomePage;
use crate::pages::settings_page::SettingsPage;
use crate::services::generation_tracker::GenerationTracker;
use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{ParentRoute, Route, Router, Routes},
    ParamSegment, StaticSegment,
};
use std::collections::HashMap;
use uuid::Uuid;

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                // Apply persisted theme before first paint to avoid a flash.
                <script>{crate::theme::theme_init_script()}</script>
                <AutoReload options=options.clone() />
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body class="min-h-screen bg-background font-sans antialiased">
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/nanorp.css"/>
        <Title text="NanoRP"/>

        <ToastProvider>
            <AppRoutes/>
        </ToastProvider>
    }
}

/// App routes plus the app-scoped services that must survive page navigation:
/// the current-session marker and the streaming-generation tracker, whose
/// stream-consumption tasks outlive individual pages.
#[component]
fn AppRoutes() -> impl IntoView {
    let viewing = RwSignal::new(None::<Uuid>);
    provide_context(CurrentSession(viewing));
    provide_context(GenerationTracker {
        active: RwSignal::new(HashMap::new()),
        viewing,
    });

    view! {
        <Router>
            <Routes fallback=|| view! {
                <div class="flex h-screen items-center justify-center">
                    <p class="text-muted-foreground">"Page not found."</p>
                </div>
            }>
                <ParentRoute path=StaticSegment("") view=MainLayout>
                    <Route path=StaticSegment("") view=HomePage/>
                    <Route path=(StaticSegment("chat"), ParamSegment("id")) view=ChatPage/>
                    <Route path=StaticSegment("characters") view=CharactersPage/>
                    <Route path=StaticSegment("settings") view=SettingsPage/>
                </ParentRoute>
            </Routes>
        </Router>
        <Toaster/>
    }
}
