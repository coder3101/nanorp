use crate::models::chat::ChatSummary;
use crate::server::character::list_characters;
use crate::server::chat::list_chat_sessions;
use crate::server::provider::list_providers;
use crate::server::update::check_for_update;
use leptos::callback::Callable;
use leptos::prelude::*;
use leptos_router::components::A;

/// A lightweight landing page that reflects what is already configured. New
/// users get their next useful step; returning users get back into a recent
/// conversation without being taken through a wizard.
#[component]
pub fn HomePage() -> impl IntoView {
    let refresh = RwSignal::new(0u32);
    let home_data = Resource::new(
        move || refresh.get(),
        |_| async move {
            futures::join!(
                list_providers(),
                list_characters(),
                list_chat_sessions(None, None, 3, 0),
            )
        },
    );
    let update_check = Resource::new(|| (), |_| check_for_update());

    view! {
        <div class="h-full overflow-y-auto scroll-area">
            <div class="mx-auto flex w-full max-w-5xl flex-col px-4 py-6 sm:px-6 sm:py-10 lg:px-8">
                <Suspense fallback=|| view! { <HomeSkeleton /> }>
                    {move || home_data.get().map(|(providers, characters, chats)| {
                        view! {
                            <HomeContent
                                provider_count=providers.map(|items| items.len()).map_err(|e| e.to_string())
                                character_count=characters.map(|items| items.len()).map_err(|e| e.to_string())
                                chats=chats.map_err(|e| e.to_string())
                                on_retry=Callback::new(move |_| refresh.update(|value| *value += 1))
                            />
                        }
                    })}
                </Suspense>

                <footer class="mt-10 flex flex-col items-center gap-2 border-t border-border pt-5 text-center">
                    // Baked in at compile time, so it always matches the running build.
                    <p class="text-xs text-muted-foreground">
                        {concat!("NanoRP v", env!("CARGO_PKG_VERSION"))}
                    </p>

                    <Suspense fallback=|| ()>
                        {move || {
                            update_check.get().map(|result| {
                                match result {
                                    Ok(Some(info)) if info.update_available => {
                                        view! {
                                            <a
                                                href={info.release_url}
                                                target="_blank"
                                                rel="noopener noreferrer"
                                                class="inline-flex min-h-11 items-center gap-1.5 rounded-md px-3 text-xs text-primary no-underline transition-colors hover:bg-primary/10"
                                            >
                                                <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14"
                                                     viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                                     stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                                                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
                                                    <polyline points="7 10 12 15 17 10"/>
                                                    <line x1="12" x2="12" y1="15" y2="3"/>
                                                </svg>
                                                {format!("{} available", info.latest_tag)}
                                            </a>
                                        }.into_any()
                                    }
                                    _ => view! { <span/> }.into_any(),
                                }
                            })
                        }}
                    </Suspense>
                </footer>
            </div>
        </div>
    }
}

#[component]
fn HomeContent(
    provider_count: Result<usize, String>,
    character_count: Result<usize, String>,
    chats: Result<Vec<ChatSummary>, String>,
    on_retry: Callback<()>,
) -> impl IntoView {
    let has_providers = matches!(&provider_count, Ok(count) if *count > 0);
    let has_characters = matches!(&character_count, Ok(count) if *count > 0);
    let setup_known = provider_count.is_ok() && character_count.is_ok();
    let recent = chats.as_ref().ok().cloned().unwrap_or_default();
    let latest_href = recent
        .first()
        .map(|chat| format!("/chat/{}", chat.session_id))
        .unwrap_or_else(|| "/characters".to_string());
    let errors = [provider_count.err(), character_count.err(), chats.err()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let has_errors = !errors.is_empty();
    let error_text = errors.join(" · ");
    let has_recent = !recent.is_empty();
    let recent = StoredValue::new(recent);
    let show_setup = setup_known && (!has_providers || !has_characters);

    view! {
        <section class="pt-2 sm:pt-6">
            <div class="flex items-center gap-3">
                <img src="/logo.png" alt="" class="h-12 w-12 rounded-xl object-contain sm:h-14 sm:w-14" />
                <div>
                    <p class="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                        {if !has_recent { "A cozy home for AI roleplay" } else { "Welcome back" }}
                    </p>
                    <h1 class="mt-0.5 text-2xl font-bold tracking-tight sm:text-3xl">
                        {if !has_recent { "Make yourself at home" } else { "Jump back into the story" }}
                    </h1>
                </div>
            </div>
            <p class="mt-4 max-w-2xl text-sm leading-6 text-muted-foreground sm:text-base">
                {if !has_recent {
                    "Create a character, connect a local or hosted model, and start a conversation. Your data is stored locally; your selected provider processes messages for inference."
                } else {
                    "Continue a recent conversation, or choose a character when you want a fresh start."
                }}
            </p>
            <div class="mt-5 flex flex-col gap-2 sm:flex-row">
                <A
                    href=latest_href
                    attr:class="inline-flex min-h-11 items-center justify-center gap-2 rounded-lg bg-primary px-5 text-sm font-medium text-primary-foreground shadow transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring no-underline"
                >
                    {if !has_recent { "Choose a character" } else { "Continue latest chat" }}
                    <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12h14"/><path d="m12 5 7 7-7 7"/></svg>
                </A>
                <A
                    href="/characters"
                    attr:class="inline-flex min-h-11 items-center justify-center rounded-lg border border-input bg-background px-5 text-sm font-medium shadow-sm transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring no-underline"
                >
                    {if !has_recent { "Browse characters" } else { "Start a new chat" }}
                </A>
            </div>
        </section>

        <Show when=move || has_errors>
            <div role="alert" class="mt-6 flex flex-col gap-3 rounded-xl border border-destructive/30 bg-destructive/5 p-4 sm:flex-row sm:items-center sm:justify-between">
                <div>
                    <p class="text-sm font-medium text-destructive">"Some information couldn't be loaded"</p>
                    <p class="mt-1 break-words text-xs text-muted-foreground">{error_text.clone()}</p>
                </div>
                <button
                    type="button"
                    class="inline-flex min-h-11 shrink-0 items-center justify-center rounded-lg border border-input bg-background px-4 text-sm font-medium hover:bg-accent"
                    on:click=move |_| on_retry.run(())
                >
                    "Try again"
                </button>
            </div>
        </Show>

        <Show when=move || has_recent>
            <section class="mt-8" aria-labelledby="recent-heading">
                <div class="mb-3 flex items-center justify-between gap-3">
                    <h2 id="recent-heading" class="text-lg font-semibold">"Recent conversations"</h2>
                    <span class="text-xs text-muted-foreground">"Newest first"</span>
                </div>
                <div class="grid gap-3 md:grid-cols-3">
                    {recent.get_value().into_iter().map(|chat| view! { <RecentConversation chat=chat /> }).collect::<Vec<_>>()}
                </div>
            </section>
        </Show>

        <Show when=move || show_setup>
            <section class="mt-8 rounded-2xl border border-border bg-card p-4 shadow-sm sm:p-6" aria-labelledby="setup-heading">
                <div class="flex flex-col gap-1">
                    <h2 id="setup-heading" class="text-lg font-semibold">"A couple of things to set up"</h2>
                    <p class="text-sm text-muted-foreground">"Use the steps you need, in any order. You can always come back here later."</p>
                </div>
                <div class="mt-5 grid gap-3 md:grid-cols-3">
                    <SetupStep
                        number="1"
                        title="Add a provider"
                        body="Connect Ollama or an OpenAI-compatible endpoint."
                        action="Open settings"
                        href="/settings"
                        complete=has_providers
                        current=!has_providers
                    />
                    <SetupStep
                        number="2"
                        title="Create or import a character"
                        body="Start manually, generate with AI, or import a text-only JSON file."
                        action="Manage characters"
                        href="/characters"
                        complete=has_characters
                        current=has_providers && !has_characters
                    />
                    <SetupStep
                        number="3"
                        title="Start chatting"
                        body="Pick a character and begin a new conversation."
                        action="Choose a character"
                        href="/characters"
                        complete=has_recent
                        current=has_providers && has_characters
                    />
                </div>
            </section>
        </Show>

        <Show when=move || setup_known && has_providers && has_characters && !has_recent>
            <section class="mt-8 rounded-2xl border border-border bg-card p-5 sm:p-6">
                <h2 class="text-lg font-semibold">"You're ready to chat"</h2>
                <p class="mt-1 text-sm text-muted-foreground">"Your provider and characters are set up. Choose a character to begin your first conversation."</p>
                <A href="/characters" attr:class="mt-4 inline-flex min-h-11 items-center text-sm font-medium text-primary no-underline hover:underline">"Choose a character →"</A>
            </section>
        </Show>
    }
}

#[component]
fn SetupStep(
    number: &'static str,
    title: &'static str,
    body: &'static str,
    action: &'static str,
    href: &'static str,
    complete: bool,
    current: bool,
) -> impl IntoView {
    view! {
        <div class=move || format!(
            "flex flex-col rounded-xl border p-4 {}",
            if current { "border-primary/40 bg-primary/5" } else { "border-border bg-background" }
        )>
            <div class="flex items-center gap-2">
                <span class=move || format!(
                    "flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold {}",
                    if complete { "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400" } else { "bg-muted text-muted-foreground" }
                )>
                    {if complete { "✓".to_string() } else { number.to_string() }}
                </span>
                <h3 class="text-sm font-semibold">{title}</h3>
            </div>
            <p class="mt-2 flex-1 text-xs leading-5 text-muted-foreground">{body}</p>
            <A href=href attr:class="mt-3 inline-flex min-h-11 items-center text-sm font-medium text-primary no-underline hover:underline">
                {if complete { "Review" } else { action }}
            </A>
        </div>
    }
}

#[component]
fn RecentConversation(chat: ChatSummary) -> impl IntoView {
    let title = chat
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| chat.character_name.clone());
    let preview = chat
        .last_message
        .clone()
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| "No messages yet — pick up where you left off.".to_string());
    let initial = crate::components::avatar::initial(&chat.character_name);
    let avatar = chat
        .character_avatar_path
        .clone()
        .map(|path| format!("/{path}"));
    let timestamp = chat.updated_at.format("%b %d, %Y").to_string();

    view! {
        <A
            href=format!("/chat/{}", chat.session_id)
            attr:class="flex min-h-28 gap-3 rounded-xl border border-border bg-card p-4 no-underline shadow-sm transition-colors hover:border-primary/40 hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
            {match avatar {
                Some(url) => view! { <img src=url alt="" class="h-10 w-10 shrink-0 rounded-full object-cover" /> }.into_any(),
                None => view! {
                    <span class=format!("flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-gradient-to-br {} text-sm font-medium text-white", crate::components::avatar::gradient(&chat.character_name))>
                        {initial}
                    </span>
                }.into_any(),
            }}
            <div class="min-w-0 flex-1">
                <div class="flex items-baseline justify-between gap-2">
                    <h3 class="truncate text-sm font-semibold">{title}</h3>
                    <time class="shrink-0 text-[11px] text-muted-foreground">{timestamp}</time>
                </div>
                <p class="mt-0.5 truncate text-xs font-medium text-muted-foreground">{chat.character_name.clone()}</p>
                <p class="mt-2 line-clamp-2 text-xs leading-5 text-muted-foreground">{preview}</p>
            </div>
        </A>
    }
}

#[component]
fn HomeSkeleton() -> impl IntoView {
    view! {
        <div class="animate-pulse pt-2 sm:pt-6" aria-label="Loading home">
            <div class="h-14 w-3/4 rounded-lg bg-muted"></div>
            <div class="mt-5 h-4 w-full max-w-xl rounded bg-muted"></div>
            <div class="mt-2 h-4 w-2/3 max-w-md rounded bg-muted"></div>
            <div class="mt-6 h-11 w-48 rounded-lg bg-muted"></div>
            <div class="mt-8 grid gap-3 md:grid-cols-3">
                {(0..3).map(|_| view! { <div class="h-32 rounded-xl bg-muted"></div> }).collect::<Vec<_>>()}
            </div>
        </div>
    }
}
