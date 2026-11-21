use crate::components::ui::modal::Modal;
use crate::components::ui::scroll_area::ScrollArea;
use crate::components::ui::toast::use_toast;
use crate::models::chat::{ChatExportFile, ChatExportFormat, ChatSummary, MAX_CHAT_TITLE_CHARS};
use crate::server::chat::{export_chat_session, rename_chat_session};
use crate::services::generation_tracker::use_generation_tracker;
use leptos::callback::Callable;
use leptos::html;
use leptos::prelude::*;
use leptos_router::components::A;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingRowAction {
    Rename,
    Json,
    Markdown,
}

#[cfg(feature = "hydrate")]
fn download_export(file: ChatExportFile) -> Result<(), String> {
    use leptos::wasm_bindgen::{JsCast, JsValue};

    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(&file.content));
    let blob = web_sys::Blob::new_with_str_sequence(&parts)
        .map_err(|_| "Could not create the download".to_string())?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "Could not create the download URL".to_string())?;
    let result = (|| -> Result<(), String> {
        let document = leptos::prelude::document();
        let anchor = document
            .create_element("a")
            .map_err(|_| "Could not start the download".to_string())?;
        anchor
            .set_attribute("href", &url)
            .map_err(|_| "Could not start the download".to_string())?;
        anchor
            .set_attribute("download", &file.filename)
            .map_err(|_| "Could not start the download".to_string())?;
        anchor
            .set_attribute("aria-hidden", "true")
            .map_err(|_| "Could not start the download".to_string())?;
        let body = document
            .body()
            .ok_or_else(|| "The page is not ready for downloads".to_string())?;
        body.append_child(&anchor)
            .map_err(|_| "Could not start the download".to_string())?;
        anchor.unchecked_ref::<web_sys::HtmlElement>().click();
        anchor.remove();
        Ok(())
    })();
    let _ = web_sys::Url::revoke_object_url(&url);
    result
}

#[cfg(not(feature = "hydrate"))]
fn download_export(_file: ChatExportFile) -> Result<(), String> {
    Err("Downloads are available in the browser".to_string())
}

fn format_relative_time(dt: &chrono::DateTime<chrono::Utc>) -> String {
    format_elapsed(chrono::Utc::now() - *dt)
}

fn format_elapsed(duration: chrono::TimeDelta) -> String {
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
    if duration.num_days() < 365 {
        return format!("{}mo", (duration.num_days() / 30).max(1));
    }
    format!("{}y", (duration.num_days() / 365).max(1))
}

#[component]
pub fn ChatList(
    #[prop(optional)] current_session_id: Signal<Option<Uuid>>,
    /// Called when a chat is selected (e.g. to close the mobile sidebar).
    #[prop(optional)]
    on_select: Option<Callback<()>>,
    /// Called with the session id when the user confirms deletion.
    #[prop(optional)]
    on_delete: Option<Callback<Uuid>>,
    /// The conversation currently being deleted by the parent, if any.
    #[prop(optional, into)]
    deleting_id: Signal<Option<Uuid>>,
    /// Refreshes the parent list after a successful rename.
    #[prop(optional)]
    on_renamed: Option<Callback<()>>,
    #[prop(into)] chats: Signal<Vec<ChatSummary>>,
    /// Message shown when the list is empty.
    #[prop(optional, into)]
    empty_text: MaybeProp<String>,
    /// Whether another page exists, which shows the footer affordance.
    #[prop(optional, into)]
    has_more: Signal<bool>,
    /// True while a page is in flight, so scrolling can't pile up requests.
    #[prop(optional, into)]
    loading: Signal<bool>,
    /// Asks the parent for the next page.
    #[prop(optional)]
    on_load_more: Option<Callback<()>>,
) -> impl IntoView {
    let scroll_ref = NodeRef::<html::Div>::new();

    // Fetch the next page a little before the list bottoms out, so scrolling
    // feels continuous instead of stopping at a button.
    let on_scroll = Callback::new(move |_: ()| {
        let Some(cb) = on_load_more else { return };
        if !has_more.get_untracked() || loading.get_untracked() {
            return;
        }
        if let Some(el) = scroll_ref.get_untracked() {
            let remaining = el.scroll_height() - el.scroll_top() - el.client_height();
            if remaining < 240 {
                cb.run(());
            }
        }
    });

    view! {
        <ScrollArea class="flex-1 h-full" node_ref=scroll_ref on_scroll=on_scroll>
            <Show
                when=move || !chats.get().is_empty()
                fallback=move || view! {
                    <p class="px-4 py-8 text-center text-sm leading-6 text-muted-foreground">
                        {move || empty_text.get().unwrap_or_else(|| {
                            "No conversations yet. Select a character to start chatting.".to_string()
                        })}
                    </p>
                }
            >
                <div class="space-y-1 p-2">
                    <For
                        each=move || chats.get()
                        // Changed summaries must remount their row so refreshed
                        // titles, previews, and timestamps replace StoredValues.
                        key=|chat| (chat.session_id, chat.updated_at)
                        let:chat
                    >
                        <ChatRow
                            chat=chat
                            current_session_id=current_session_id
                            on_select=on_select
                            on_delete=on_delete
                            deleting_id=deleting_id
                            on_renamed=on_renamed
                        />
                    </For>

                    // Scrolling normally triggers the next page, but keep the
                    // button: it's the keyboard path, and it's the only way in
                    // if the list somehow doesn't overflow.
                    <Show when=move || has_more.get() || loading.get()>
                        <div class="py-2 text-center">
                            <Show
                                when=move || !loading.get()
                                fallback=|| view! {
                                    <span class="inline-flex min-h-11 items-center gap-2 text-xs text-muted-foreground">
                                        <span class="h-3 w-3 animate-spin rounded-full border-2 border-muted-foreground/30 border-t-muted-foreground" aria-hidden="true"></span>
                                        "Loading conversations…"
                                    </span>
                                }
                            >
                                <button
                                    type="button"
                                    class="inline-flex min-h-11 items-center rounded-md border border-input bg-background px-4 text-xs font-medium text-muted-foreground shadow-sm transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                                    on:click=move |_| {
                                        if let Some(cb) = on_load_more {
                                            cb.run(());
                                        }
                                    }
                                >
                                    "Load more"
                                </button>
                            </Show>
                        </div>
                    </Show>
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
    deleting_id: Signal<Option<Uuid>>,
    on_renamed: Option<Callback<()>>,
) -> impl IntoView {
    let session_id = chat.session_id;
    let is_active = Signal::derive(move || current_session_id.get() == Some(session_id));
    let tracker = use_generation_tracker();
    let is_generating = Signal::derive(move || {
        tracker
            .active
            .with(|active| active.contains_key(&session_id))
    });
    let time_str = StoredValue::new(format_relative_time(&chat.updated_at));
    let full_time = StoredValue::new(chat.updated_at.format("%b %d, %Y at %H:%M UTC").to_string());
    let machine_time = StoredValue::new(chat.updated_at.to_rfc3339());
    let initial = StoredValue::new(crate::components::avatar::initial(&chat.character_name));
    let gradient = StoredValue::new(crate::components::avatar::gradient(&chat.character_name));
    let actions_label = StoredValue::new(format!(
        "Conversation actions for {}",
        chat.title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .unwrap_or(&chat.character_name)
    ));
    let is_deleting = Signal::derive(move || deleting_id.get() == Some(session_id));
    let avatar_url = StoredValue::new(
        chat.character_avatar_path
            .clone()
            .map(|rel| format!("/{rel}")),
    );
    // Prefer the session title (auto-generated or user-set); fall back to the
    // character's name.
    let title = RwSignal::new(
        chat.title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| chat.character_name.clone()),
    );
    let last_message = StoredValue::new(
        chat.last_message
            .clone()
            .filter(|message| !message.trim().is_empty())
            .unwrap_or_else(|| "No messages yet".to_string()),
    );

    // Two-step inline delete confirmation. Instead of covering the title with
    // tiny icon targets, confirmation gets its own full row and 44px actions.
    let confirming = RwSignal::new(false);
    let arm_confirm = move || {
        confirming.set(true);
        leptos::leptos_dom::helpers::set_timeout(
            move || confirming.set(false),
            std::time::Duration::from_secs(6),
        );
    };

    let on_click = move |_| {
        if let Some(cb) = on_select {
            cb.run(());
        }
    };

    let toast = use_toast();
    let actions_open = RwSignal::new(false);
    let renaming = RwSignal::new(false);
    let rename_value = RwSignal::new(title.get_untracked());
    let pending = RwSignal::new(None::<PendingRowAction>);
    let action_error = RwSignal::new(None::<String>);

    let close_actions = Callback::new(move |_| {
        if pending.get_untracked().is_some() {
            return;
        }
        actions_open.set(false);
        renaming.set(false);
        action_error.set(None);
    });

    let toast_for_rename = toast.clone();
    let save_rename = Callback::new(move |_: ()| {
        if pending.get_untracked().is_some() {
            return;
        }
        let requested = rename_value.get_untracked();
        let requested = requested.trim().to_string();
        if requested.is_empty() {
            action_error.set(Some("Enter a conversation title.".to_string()));
            return;
        }
        if requested.chars().count() > MAX_CHAT_TITLE_CHARS {
            action_error.set(Some(format!(
                "Titles cannot exceed {MAX_CHAT_TITLE_CHARS} characters."
            )));
            return;
        }
        if requested == title.get_untracked() {
            close_actions.run(());
            return;
        }

        action_error.set(None);
        pending.set(Some(PendingRowAction::Rename));
        let toast = toast_for_rename.clone();
        leptos::task::spawn_local(async move {
            let result = rename_chat_session(session_id, requested).await;
            pending.set(None);
            match result {
                Ok(session) => {
                    title.set(
                        session
                            .title
                            .filter(|value| !value.trim().is_empty())
                            .unwrap_or_else(|| title.get_untracked()),
                    );
                    actions_open.set(false);
                    renaming.set(false);
                    if let Some(callback) = on_renamed {
                        callback.run(());
                    }
                    toast.success("Conversation renamed");
                }
                Err(error) => action_error.set(Some(error.to_string())),
            }
        });
    });

    let start_export = Callback::new(move |format: ChatExportFormat| {
        if pending.get_untracked().is_some() {
            return;
        }
        let action = match format {
            ChatExportFormat::Json => PendingRowAction::Json,
            ChatExportFormat::Markdown => PendingRowAction::Markdown,
        };
        action_error.set(None);
        pending.set(Some(action));
        let toast = toast.clone();
        leptos::task::spawn_local(async move {
            let result = export_chat_session(session_id, format).await;
            pending.set(None);
            match result {
                Ok(file) => match download_export(file) {
                    Ok(()) => {
                        actions_open.set(false);
                        renaming.set(false);
                        toast.success("Conversation exported");
                    }
                    Err(error) => action_error.set(Some(error)),
                },
                Err(error) => action_error.set(Some(error.to_string())),
            }
        });
    });

    view! {
        <div class="group/row relative">
            <Show
                when=move || confirming.get()
                fallback=move || view! {
                    <A
                        href=format!("/chat/{}", session_id)
                        attr:class=move || {
                            let base = "flex min-h-[76px] items-center gap-3 rounded-lg py-3 pl-3 pr-14 text-left text-sm transition-colors hover:bg-accent cursor-pointer no-underline";
                            if is_active.get() {
                                format!("{} bg-accent", base)
                            } else {
                                base.to_string()
                            }
                        }
                        attr:aria-current=move || is_active.get().then_some("page")
                        on:click=on_click
                    >
                        {match avatar_url.get_value() {
                            Some(url) => view! {
                                <img src=url alt="" class="h-10 w-10 shrink-0 rounded-full object-cover" />
                            }.into_any(),
                            None => view! {
                                <div class=format!(
                                    "flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-gradient-to-br {} text-xs font-medium text-white",
                                    gradient.get_value()
                                )>
                                    {initial.get_value()}
                                </div>
                            }.into_any(),
                        }}
                        <div class="min-w-0 flex-1">
                            <div class="flex items-center justify-between gap-2">
                                <p class="truncate font-medium leading-5">{title.get()}</p>
                                <time datetime=machine_time.get_value() title=full_time.get_value() class="shrink-0 text-[11px] text-muted-foreground">{time_str.get_value()}</time>
                            </div>
                            <Show
                                when=move || is_generating.get()
                                fallback=move || view! {
                                    <p class="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground">{last_message.get_value()}</p>
                                }
                            >
                                <p class="mt-1 flex items-center gap-1.5 text-xs font-medium leading-5 text-primary" role="status">
                                    <span class="h-3 w-3 shrink-0 animate-spin rounded-full border-2 border-primary/30 border-t-primary" aria-hidden="true"></span>
                                    "Generating reply…"
                                </p>
                            </Show>
                        </div>
                    </A>

                    <button
                        type="button"
                        class="absolute right-1 top-1/2 flex h-11 w-11 -translate-y-1/2 items-center justify-center rounded-md text-muted-foreground opacity-0 transition-opacity hover:bg-accent hover:text-foreground focus-visible:opacity-100 group-hover/row:opacity-100 pointer-coarse:opacity-100 disabled:opacity-60"
                        title="Conversation actions"
                        aria-label=actions_label.get_value()
                        aria-haspopup="dialog"
                        disabled=move || is_deleting.get()
                        on:click=move |ev| {
                            ev.prevent_default();
                            ev.stop_propagation();
                            action_error.set(None);
                            renaming.set(false);
                            actions_open.set(true);
                        }
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/></svg>
                    </button>
                }
            >
                <div role="alert" class="rounded-lg border border-destructive/30 bg-destructive/5 p-3">
                    <p class="text-sm font-medium">"Delete this conversation?"</p>
                    <p class="mt-0.5 truncate text-xs text-muted-foreground" title=title.get()>{title.get()}</p>
                    <div class="mt-3 grid grid-cols-2 gap-2">
                        <button
                            type="button"
                            class="inline-flex min-h-11 items-center justify-center rounded-md border border-input bg-background px-3 text-xs font-medium shadow-sm hover:bg-accent disabled:opacity-60"
                            disabled=move || is_deleting.get()
                            on:click=move |_| confirming.set(false)
                        >
                            "Cancel"
                        </button>
                        <button
                            type="button"
                            class="inline-flex min-h-11 items-center justify-center rounded-md bg-destructive px-3 text-xs font-medium text-destructive-foreground shadow-sm hover:bg-destructive/90 disabled:opacity-60"
                            disabled=move || is_deleting.get()
                            on:click=move |_| {
                                if is_deleting.get_untracked() {
                                    return;
                                }
                                confirming.set(false);
                                if let Some(cb) = on_delete {
                                    cb.run(session_id);
                                }
                            }
                        >
                            {move || if is_deleting.get() { "Deleting…" } else { "Delete" }}
                        </button>
                    </div>
                </div>
            </Show>

            <Modal
                open=actions_open
                label=Signal::derive(move || {
                    if renaming.get() {
                        "Rename conversation".to_string()
                    } else {
                        "Conversation actions".to_string()
                    }
                })
                class="max-w-sm p-4 sm:m-4 sm:p-6"
                on_close=close_actions
            >
                <Show
                    when=move || renaming.get()
                    fallback=move || view! {
                        <h2 class="pr-8 text-lg font-semibold">"Conversation actions"</h2>
                        <p class="mt-1 truncate text-sm text-muted-foreground" title=title.get()>{title.get()}</p>
                        <div class="mt-5 flex flex-col gap-2">
                            <button
                                type="button"
                                class="inline-flex min-h-11 w-full items-center rounded-md border border-input bg-background px-4 text-left text-sm font-medium shadow-sm hover:bg-accent disabled:opacity-60"
                                disabled=move || pending.get().is_some()
                                on:click=move |_| {
                                    rename_value.set(title.get_untracked());
                                    action_error.set(None);
                                    renaming.set(true);
                                }
                            >
                                "Rename"
                            </button>
                            <button
                                type="button"
                                class="inline-flex min-h-11 w-full items-center rounded-md border border-input bg-background px-4 text-left text-sm font-medium shadow-sm hover:bg-accent disabled:opacity-60"
                                disabled=move || pending.get().is_some()
                                on:click=move |_| start_export.run(ChatExportFormat::Json)
                            >
                                {move || if pending.get() == Some(PendingRowAction::Json) {
                                    "Exporting JSON…"
                                } else {
                                    "Export JSON"
                                }}
                            </button>
                            <button
                                type="button"
                                class="inline-flex min-h-11 w-full items-center rounded-md border border-input bg-background px-4 text-left text-sm font-medium shadow-sm hover:bg-accent disabled:opacity-60"
                                disabled=move || pending.get().is_some()
                                on:click=move |_| start_export.run(ChatExportFormat::Markdown)
                            >
                                {move || if pending.get() == Some(PendingRowAction::Markdown) {
                                    "Exporting Markdown…"
                                } else {
                                    "Export Markdown"
                                }}
                            </button>
                            <Show when=move || on_delete.is_some()>
                                <button
                                    type="button"
                                    class="inline-flex min-h-11 w-full items-center rounded-md border border-destructive/30 bg-background px-4 text-left text-sm font-medium text-destructive shadow-sm hover:bg-destructive/10 disabled:opacity-60"
                                    disabled=move || pending.get().is_some()
                                    on:click=move |_| {
                                        actions_open.set(false);
                                        action_error.set(None);
                                        arm_confirm();
                                    }
                                >
                                    "Delete"
                                </button>
                            </Show>
                        </div>
                    }
                >
                    <form on:submit=move |event| {
                        event.prevent_default();
                        save_rename.run(());
                    }>
                        <h2 class="text-lg font-semibold">"Rename conversation"</h2>
                        <label for=format!("conversation-title-{session_id}") class="mt-5 block text-sm font-medium">
                            "Conversation title"
                        </label>
                        <input
                            id=format!("conversation-title-{session_id}")
                            type="text"
                            required
                            maxlength=MAX_CHAT_TITLE_CHARS
                            autocomplete="off"
                            class="mt-2 h-11 w-full rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60"
                            prop:value=move || rename_value.get()
                            on:input=move |event| rename_value.set(event_target_value(&event))
                            disabled=move || pending.get().is_some()
                        />
                        <p class="mt-1.5 text-xs text-muted-foreground">"Up to 100 characters."</p>
                        <div class="mt-5 grid grid-cols-2 gap-2">
                            <button
                                type="button"
                                class="inline-flex min-h-11 items-center justify-center rounded-md border border-input bg-background px-4 text-sm font-medium shadow-sm hover:bg-accent disabled:opacity-60"
                                disabled=move || pending.get().is_some()
                                on:click=move |_| close_actions.run(())
                            >
                                "Cancel"
                            </button>
                            <button
                                type="submit"
                                class="inline-flex min-h-11 items-center justify-center rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground shadow-sm hover:bg-primary/90 disabled:opacity-60"
                                disabled=move || pending.get().is_some()
                            >
                                {move || if pending.get() == Some(PendingRowAction::Rename) {
                                    "Saving…"
                                } else {
                                    "Save"
                                }}
                            </button>
                        </div>
                    </form>
                </Show>
                <Show when=move || action_error.get().is_some()>
                    <p role="alert" class="mt-3 break-words rounded-md border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
                        {move || action_error.get().unwrap_or_default()}
                    </p>
                </Show>
                <Show when=move || pending.get().is_some()>
                    <p role="status" class="sr-only" aria-live="polite">"Conversation action in progress."</p>
                </Show>
            </Modal>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::format_elapsed;
    use chrono::TimeDelta;

    #[test]
    fn relative_time_handles_future_and_month_boundaries() {
        assert_eq!(format_elapsed(TimeDelta::seconds(-5)), "now");
        assert_eq!(format_elapsed(TimeDelta::days(27)), "3w");
        assert_eq!(format_elapsed(TimeDelta::days(29)), "1mo");
        assert_eq!(format_elapsed(TimeDelta::days(365)), "1y");
    }

    #[test]
    fn relative_time_uses_compact_short_units() {
        assert_eq!(format_elapsed(TimeDelta::minutes(4)), "4m");
        assert_eq!(format_elapsed(TimeDelta::hours(8)), "8h");
        assert_eq!(format_elapsed(TimeDelta::days(6)), "6d");
    }
}
