use leptos::prelude::*;
use leptos::callback::Callable;
use leptos_router::components::A;
use uuid::Uuid;
use crate::models::chat::ChatSummary;
use crate::components::ui::scroll_area::ScrollArea;

fn format_relative_time(dt: &chrono::DateTime<chrono::Utc>) -> String {
    let duration = chrono::Utc::now() - *dt;
    if duration.num_minutes() < 1 {
        return "now".to_string();
    }
    if duration.num_minutes() < 60 {
        return format!("{}m", duration.num_minutes());
    }
    if duration.num_hours() < 24 {
        return format!("{}h", duration.num_hours());
    }
    if duration.num_days() < 7 {
        return format!("{}d", duration.num_days());
    }
    if duration.num_weeks() < 4 {
        return format!("{}w", duration.num_weeks());
    }
    format!("{}mo", duration.num_days() / 30)
}

#[component]
pub fn ChatList(
    #[prop(optional)] current_session_id: Signal<Option<Uuid>>,
    /// Called when a chat is selected (e.g. to close the mobile sidebar).
    #[prop(optional)] on_select: Option<Callback<()>>,
    /// Called with the session id when the user confirms deletion.
    #[prop(optional)] on_delete: Option<Callback<Uuid>>,
    #[prop(into)] chats: Signal<Vec<ChatSummary>>,
    /// Message shown when the list is empty.
    #[prop(optional, into)] empty_text: MaybeProp<String>,
) -> impl IntoView {
    view! {
        <ScrollArea class="flex-1 h-full">
            <Show
                when=move || !chats.get().is_empty()
                fallback=move || view! {
                    <p class="text-sm text-muted-foreground text-center py-8 px-4">
                        {move || empty_text.get().unwrap_or_else(|| {
                            "No conversations yet. Select a character to start chatting.".to_string()
                        })}
                    </p>
                }
            >
                <div class="space-y-1 p-2">
                    <For
                        each=move || chats.get()
                        key=|chat| chat.session_id
                        let:chat
                    >
                        <ChatRow chat=chat current_session_id=current_session_id on_select=on_select on_delete=on_delete />
                    </For>
                </div>
            </Show>
        </ScrollArea>
    }
}

#[component]
fn ChatRow(
    chat: ChatSummary,
    current_session_id: Signal<Option<Uuid>>,
    on_select: Option<Callback<()>>,
    on_delete: Option<Callback<Uuid>>,
) -> impl IntoView {
    let session_id = chat.session_id;
    let is_active = Signal::derive(move || current_session_id.get() == Some(session_id));
    let time_str = format_relative_time(&chat.updated_at);
    let initial = crate::components::avatar::initial(&chat.character_name);
    let avatar_url = chat.character_avatar_path.clone().map(|rel| format!("/{rel}"));
    // Prefer the session title (auto-generated or user-set); fall back to the
    // character's name.
    let title = chat
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| chat.character_name.clone());
    let last_message = chat.last_message.clone().unwrap_or_default();

    // Two-step inline delete confirmation. Auto-reverts after a few seconds
    // so a stray click doesn't leave the row stuck in the confirm state.
    let confirming = RwSignal::new(false);
    let arm_confirm = move || {
        confirming.set(true);
        leptos::leptos_dom::helpers::set_timeout(
            move || confirming.set(false),
            std::time::Duration::from_secs(4),
        );
    };

    let on_click = move |_| {
        if let Some(cb) = on_select {
            cb.run(());
        }
    };

    view! {
        <div class="group/row relative">
            <A
                href=format!("/chat/{}", session_id)
                attr:class=move || {
                    let base = "flex items-start gap-3 rounded-lg p-3 pr-9 text-left text-sm \
                                transition-colors hover:bg-accent cursor-pointer no-underline";
                    if is_active.get() {
                        format!("{} bg-accent", base)
                    } else {
                        base.to_string()
                    }
                }
                on:click=on_click
            >
                {match avatar_url {
                    Some(url) => view! {
                        <img src=url alt="" class="h-8 w-8 shrink-0 rounded-full object-cover" />
                    }.into_any(),
                    None => view! {
                        <div class=format!(
                            "flex h-8 w-8 shrink-0 items-center justify-center rounded-full \
                             bg-gradient-to-br {} text-xs font-medium text-white",
                            crate::components::avatar::gradient(&chat.character_name)
                        )>
                            {initial}
                        </div>
                    }.into_any(),
                }}
                <div class="flex-1 overflow-hidden">
                    <div class="flex items-center justify-between">
                        <p class="font-medium leading-none truncate">{title}</p>
                        <span class="text-xs text-muted-foreground shrink-0 ml-2">{time_str}</span>
                    </div>
                    <p class="text-xs text-muted-foreground line-clamp-2 mt-1">
                        {last_message}
                    </p>
                </div>
            </A>

            // Delete affordance (top-right). Shows a trash icon on hover; a
            // second click confirms.
            <Show when=move || on_delete.is_some()>
                <div class="absolute right-1.5 top-1.5 flex items-center gap-0.5">
                    <Show
                        when=move || confirming.get()
                        fallback=move || view! {
                            <button
                                class="flex h-6 w-6 items-center justify-center rounded-md text-muted-foreground \
                                       opacity-0 transition-opacity hover:bg-destructive hover:text-destructive-foreground \
                                       group-hover/row:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100"
                                title="Delete conversation"
                                aria-label="Delete conversation"
                                on:click=move |ev| {
                                    ev.prevent_default();
                                    ev.stop_propagation();
                                    arm_confirm();
                                }
                            >
                                <svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
                            </button>
                        }
                    >
                        // Confirm / cancel pair.
                        <button
                            class="flex h-6 w-6 items-center justify-center rounded-md bg-destructive text-destructive-foreground shadow-sm hover:bg-destructive/90"
                            title="Confirm delete"
                            aria-label="Confirm delete"
                            on:click=move |ev| {
                                ev.prevent_default();
                                ev.stop_propagation();
                                confirming.set(false);
                                if let Some(cb) = on_delete {
                                    cb.run(session_id);
                                }
                            }
                        >
                            <svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
                        </button>
                        <button
                            class="flex h-6 w-6 items-center justify-center rounded-md border border-border bg-background text-muted-foreground shadow-sm hover:bg-accent"
                            title="Cancel"
                            aria-label="Cancel delete"
                            on:click=move |ev| {
                                ev.prevent_default();
                                ev.stop_propagation();
                                confirming.set(false);
                            }
                        >
                            <svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
                        </button>
                    </Show>
                </div>
            </Show>
        </div>
    }
}
