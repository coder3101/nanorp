use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;
use uuid::Uuid;
use crate::models::message::{Attachment, EditPayload, ImageUpload, Message, MessageRole};

/// A newly-picked image (during message edit) held in memory before saving.
#[derive(Clone, PartialEq)]
struct PendingImg {
    id: Uuid,
    data: String,
    content_type: String,
    original_name: Option<String>,
}

/// Read a picked image `File` into base64 and append it to `pending`.
///
/// `kept` is the number of existing attachments being retained, which counts
/// against the same per-message cap as the newly added ones.
#[cfg(feature = "hydrate")]
fn read_image_file(
    file: leptos::web_sys::File,
    pending: RwSignal<Vec<PendingImg>>,
    kept: usize,
    toast: &crate::components::ui::toast::UseToast,
) {
    use crate::models::message::{MAX_IMAGES_PER_MESSAGE, MAX_IMAGE_BYTES};
    use leptos::wasm_bindgen::closure::Closure;

    let content_type = file.type_();
    if !content_type.starts_with("image/") {
        toast.warning(format!("\"{}\" isn't an image — only images can be attached", file.name()));
        return;
    }
    if file.size() as u64 > MAX_IMAGE_BYTES as u64 {
        toast.warning(format!("\"{}\" is too large (max 10 MB)", file.name()));
        return;
    }
    if kept + pending.get().len() >= MAX_IMAGES_PER_MESSAGE {
        toast.warning(format!(
            "You can attach up to {MAX_IMAGES_PER_MESSAGE} images per message"
        ));
        return;
    }
    let name = file.name();
    let reader = leptos::web_sys::FileReader::new().unwrap();
    let reader_clone = reader.clone();
    let onload = Closure::wrap(Box::new(move |_: leptos::web_sys::Event| {
        if let Ok(result) = reader_clone.result() {
            if let Some(data_url) = result.as_string() {
                let base64 = data_url.split(',').nth(1).unwrap_or("").to_string();
                pending.update(|v| {
                    // Re-checked here because reads complete asynchronously, so
                    // a multi-file selection could otherwise overshoot the cap.
                    if kept + v.len() < MAX_IMAGES_PER_MESSAGE {
                        v.push(PendingImg {
                            id: Uuid::new_v4(),
                            data: base64,
                            content_type: content_type.clone(),
                            original_name: Some(name.clone()),
                        });
                    }
                });
            }
        }
    }) as Box<dyn FnMut(_)>);
    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
    onload.forget();
    let _ = reader.read_as_data_url(&file);
}

use crate::components::avatar::{gradient as avatar_gradient, initial};

fn format_time(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.with_timezone(&chrono::Local).format("%-I:%M %p").to_string()
}

/// The parsed pieces of an assistant message: an optional reasoning ("thinking")
/// segment and the visible answer.
struct Parsed {
    reasoning: Option<String>,
    /// True while a `<think>` block is still open (reasoning is streaming and
    /// the answer hasn't started yet).
    reasoning_open: bool,
    answer: String,
}

/// Split reasoning models' `<think>...</think>` prefix from the answer. Handles
/// the in-progress case where the closing tag hasn't streamed in yet.
fn parse_thinking(text: &str) -> Parsed {
    let trimmed_start = text.trim_start();
    if let Some(rest) = trimmed_start.strip_prefix("<think>") {
        if let Some(end) = rest.find("</think>") {
            let reasoning = rest[..end].trim().to_string();
            let answer = rest[end + "</think>".len()..].trim_start().to_string();
            Parsed {
                reasoning: (!reasoning.is_empty()).then_some(reasoning),
                reasoning_open: false,
                answer,
            }
        } else {
            // Still streaming the reasoning; no closing tag yet.
            Parsed {
                reasoning: Some(rest.trim_start().to_string()),
                reasoning_open: true,
                answer: String::new(),
            }
        }
    } else {
        Parsed {
            reasoning: None,
            reasoning_open: false,
            answer: text.to_string(),
        }
    }
}

#[component]
pub fn MessageBubble(
    message: Message,
    #[prop(into, optional)] character_name: MaybeProp<String>,
    #[prop(into, optional)] character_avatar: MaybeProp<String>,
    #[prop(optional)] is_streaming: Signal<bool>,
    #[prop(optional)] streaming_content: Signal<String>,
    /// Whether to render the model's reasoning block. Defaults to shown.
    #[prop(optional, into)] render_thinking: MaybeProp<bool>,
    /// Called with the edit payload when a user message edit is saved.
    on_edit: Option<Callback<EditPayload>>,
    /// Called with the assistant message id to regenerate it.
    on_regenerate: Option<Callback<Uuid>>,
) -> impl IntoView {
    use leptos::callback::Callable;

    let can_edit = on_edit.is_some();
    let can_regen = on_regenerate.is_some();
    let is_user = message.role == MessageRole::User;
    let content = message.content.clone();
    let attachments = message.attachments.clone();
    let model_used = message.model_used.clone();
    let time = format_time(&message.created_at);
    let msg_id = message.id;
    let show_thinking = Signal::derive(move || render_thinking.get().unwrap_or(true));

    if is_user {
        let toast = crate::components::ui::toast::use_toast();
        let editing = RwSignal::new(false);
        let draft = RwSignal::new(content.clone());
        let content_for_view = content.clone();
        // Copyable handle to the original text for the (re-rendered) Edit button.
        let original = StoredValue::new(content.clone());
        let original_attachments = StoredValue::new(attachments.clone());

        // Edit-mode image state.
        let kept = RwSignal::new(Vec::<Attachment>::new());
        let pending = RwSignal::new(Vec::<PendingImg>::new());
        let file_input = NodeRef::<leptos::html::Input>::new();

        let start_edit = move || {
            draft.set(original.get_value());
            kept.set(original_attachments.get_value());
            pending.set(Vec::new());
            editing.set(true);
        };

        view! {
            <div class="group flex items-start justify-end gap-2 px-2 py-1.5 sm:gap-3 sm:px-4 sm:py-2">
                <div class="flex max-w-[85%] flex-col items-end sm:max-w-[80%]">
                    <Show
                        when=move || editing.get()
                        fallback={
                            let content_for_view = content_for_view.clone();
                            move || view! {
                                <div class="rounded-2xl rounded-tr-md bg-primary px-3 py-2 text-primary-foreground shadow-sm sm:px-4 sm:py-2.5">
                                    <ImageGrid attachments=attachments.clone() is_user=true />
                                    <p class="whitespace-pre-wrap break-words text-[13px] leading-relaxed sm:text-sm">
                                        {content_for_view.clone()}
                                    </p>
                                </div>
                            }
                        }
                    >
                        // Inline edit mode.
                        <div class="w-[min(30rem,80vw)] rounded-2xl border border-border bg-card p-2 shadow-sm">
                            // Image strip: kept existing + newly added, each removable.
                            <Show when=move || !kept.get().is_empty() || !pending.get().is_empty()>
                                <div class="mb-2 flex flex-wrap gap-2">
                                    <For each=move || kept.get() key=|a| a.id let:att>
                                        {
                                            let att_id = att.id;
                                            let src = format!("/{}", att.file_path);
                                            view! {
                                                <div class="relative">
                                                    <img src=src class="h-16 w-16 rounded-lg border border-border object-cover" />
                                                    <button
                                                        class="absolute -right-1.5 -top-1.5 flex h-5 w-5 items-center justify-center rounded-full bg-destructive text-xs text-destructive-foreground shadow"
                                                        title="Remove image"
                                                        aria-label="Remove image"
                                                        on:click=move |_| kept.update(|v| v.retain(|a| a.id != att_id))
                                                    >
                                                        "✕"
                                                    </button>
                                                </div>
                                            }
                                        }
                                    </For>
                                    <For each=move || pending.get() key=|p| p.id let:p>
                                        {
                                            let pid = p.id;
                                            let src = format!("data:{};base64,{}", p.content_type, p.data);
                                            view! {
                                                <div class="relative">
                                                    <img src=src class="h-16 w-16 rounded-lg border border-border object-cover" />
                                                    <button
                                                        class="absolute -right-1.5 -top-1.5 flex h-5 w-5 items-center justify-center rounded-full bg-destructive text-xs text-destructive-foreground shadow"
                                                        title="Remove image"
                                                        aria-label="Remove image"
                                                        on:click=move |_| pending.update(|v| v.retain(|x| x.id != pid))
                                                    >
                                                        "✕"
                                                    </button>
                                                </div>
                                            }
                                        }
                                    </For>
                                </div>
                            </Show>

                            <textarea
                                class="min-h-[70px] w-full resize-y rounded-md border border-input bg-background px-3 py-2 text-sm leading-relaxed focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                                prop:value=move || draft.get()
                                on:input=move |ev| {
                                    let ta = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlTextAreaElement>();
                                    draft.set(ta.value());
                                }
                            />

                            <input
                                type="file" accept="image/*" multiple=true class="hidden"
                                node_ref=file_input
                                on:change={
                                    let toast = toast.clone();
                                    move |_ev| {
                                        #[cfg(feature = "hydrate")]
                                        {
                                            let input = _ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                                            if let Some(files) = input.files() {
                                                let kept_count = kept.get().len();
                                                for i in 0..files.length() {
                                                    if let Some(file) = files.get(i) {
                                                        read_image_file(file, pending, kept_count, &toast);
                                                    }
                                                }
                                            }
                                            input.set_value("");
                                        }
                                        #[cfg(not(feature = "hydrate"))]
                                        let _ = &toast;
                                    }
                                }
                            />

                            <div class="mt-2 flex items-center justify-between gap-2">
                                <button
                                    class="inline-flex h-8 items-center gap-1 rounded-md border border-input bg-background px-2.5 text-xs font-medium hover:bg-accent"
                                    title="Add image"
                                    on:click=move |_| { if let Some(i) = file_input.get() { i.click(); } }
                                >
                                    <svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="18" x="3" y="3" rx="2" ry="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/></svg>
                                    "Image"
                                </button>
                                <div class="flex gap-2">
                                    <button
                                        class="inline-flex h-8 items-center rounded-md border border-input bg-background px-3 text-xs font-medium hover:bg-accent"
                                        on:click=move |_| {
                                            let kept_ids: Vec<Uuid> = kept.get().iter().map(|a| a.id).collect();
                                            let original_ids: Vec<Uuid> =
                                                original_attachments.get_value().iter().map(|a| a.id).collect();
                                            let dirty = draft.get() != original.get_value()
                                                || kept_ids != original_ids
                                                || !pending.get().is_empty();
                                            if !dirty
                                                || crate::components::ui::confirm::confirm(
                                                    "Discard your changes to this message?",
                                                )
                                            {
                                                editing.set(false);
                                            }
                                        }
                                    >
                                        "Cancel"
                                    </button>
                                    <button
                                        class="inline-flex h-8 items-center rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground hover:bg-primary/90"
                                        on:click=move |_| {
                                            let text = draft.get();
                                            let keep_ids: Vec<Uuid> = kept.get().iter().map(|a| a.id).collect();
                                            let new_images: Vec<ImageUpload> = pending.get().into_iter().map(|p| ImageUpload {
                                                data: p.data,
                                                content_type: p.content_type,
                                                original_name: p.original_name,
                                            }).collect();
                                            let has_content = !text.trim().is_empty() || !keep_ids.is_empty() || !new_images.is_empty();
                                            if has_content {
                                                if let Some(cb) = on_edit {
                                                    cb.run(EditPayload { message_id: msg_id, content: text, keep_attachment_ids: keep_ids, new_images });
                                                }
                                                editing.set(false);
                                            }
                                        }
                                    >
                                        "Save & submit"
                                    </button>
                                </div>
                            </div>
                        </div>
                    </Show>

                    <div class="mt-1 flex items-center gap-2 px-1 opacity-0 transition-opacity \
                                group-hover:opacity-100 focus-within:opacity-100 pointer-coarse:opacity-100">
                        <Show when=move || can_edit && !editing.get()>
                            <button
                                class="inline-flex items-center gap-1 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
                                title="Edit message"
                                on:click=move |_| start_edit()
                            >
                                <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.12 2.12 0 0 1 3 3L12 15l-4 1 1-4Z"/></svg>
                                "Edit"
                            </button>
                        </Show>
                        <span class="text-[11px] text-muted-foreground">{time}</span>
                    </div>
                </div>
                // "You" avatar
                <div class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-secondary text-[10px] font-semibold text-secondary-foreground sm:h-8 sm:w-8 sm:text-xs">
                    "You"
                </div>
            </div>
        }.into_any()
    } else {
        let char_name = character_name.get().unwrap_or_else(|| "Assistant".to_string());
        let avatar_url = character_avatar.get().filter(|s| !s.is_empty()).map(|rel| format!("/{rel}"));
        let gradient = avatar_gradient(&char_name);
        let av_initial = initial(&char_name);

        let avatar = if let Some(url) = avatar_url {
            view! {
                <img src=url class="h-7 w-7 shrink-0 rounded-full object-cover shadow-sm sm:h-8 sm:w-8" />
            }.into_any()
        } else {
            view! {
                <div class=format!(
                    "flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-gradient-to-br {} text-[10px] font-semibold text-white shadow-sm sm:h-8 sm:w-8 sm:text-xs",
                    gradient
                )>
                    {av_initial}
                </div>
            }.into_any()
        };

        view! {
            <div class="group flex items-start gap-2 px-2 py-1.5 sm:gap-3 sm:px-4 sm:py-2">
                {avatar}
                <div class="flex max-w-[85%] flex-col items-start sm:max-w-[80%]">
                    <div class="rounded-2xl rounded-tl-md border border-border bg-card px-3 py-2 text-card-foreground shadow-sm sm:px-4 sm:py-2.5">
                        <ImageGrid attachments=attachments is_user=false />

                        // Raw text (streaming vs stored), then parsed into
                        // reasoning + answer.
                        {
                            let content = content.clone();
                            let raw = move || {
                                if is_streaming.get() {
                                    streaming_content.get()
                                } else {
                                    content.clone()
                                }
                            };
                            let parsed = Signal::derive(move || {
                                let p = parse_thinking(&raw());
                                (p.reasoning, p.reasoning_open, p.answer)
                            });

                            view! {
                                // Reasoning ("thinking") block, when present and enabled.
                                {move || {
                                    if !show_thinking.get() {
                                        return None;
                                    }
                                    let (reasoning, open, _) = parsed.get();
                                    reasoning.map(|r| view! {
                                        <ThinkingBlock text=r streaming=open />
                                    })
                                }}

                                // The visible answer, rendered from markdown.
                                // `render_markdown` escapes any raw HTML in the
                                // model output, so this is safe for inner_html.
                                <div
                                    class="prose max-w-none break-words text-[13px] sm:text-sm"
                                    inner_html=move || crate::markdown::render_markdown(&parsed.get().2)
                                ></div>
                            }
                        }

                        // Thinking dots (before any token) / streaming cursor.
                        {move || {
                            if is_streaming.get() && streaming_content.get().is_empty() {
                                Some(view! {
                                    <div class="flex items-center gap-1 py-1">
                                        <div class="h-1.5 w-1.5 rounded-full bg-muted-foreground animate-bounce [animation-delay:0ms]" />
                                        <div class="h-1.5 w-1.5 rounded-full bg-muted-foreground animate-bounce [animation-delay:150ms]" />
                                        <div class="h-1.5 w-1.5 rounded-full bg-muted-foreground animate-bounce [animation-delay:300ms]" />
                                    </div>
                                }.into_any())
                            } else if is_streaming.get() {
                                Some(view! {
                                    <span class="ml-0.5 inline-block h-4 w-1.5 animate-pulse rounded-sm bg-foreground align-middle" />
                                }.into_any())
                            } else {
                                None
                            }
                        }}
                    </div>

                    // Meta row: actions + model badge + timestamp (on hover, not streaming).
                    {
                        let content_for_copy = content.clone();
                        move || {
                            if is_streaming.get() {
                                None
                            } else {
                                let copy_text = {
                                    // Copy the visible answer (strip <think>).
                                    parse_thinking(&content_for_copy).answer
                                };
                                Some(view! {
                                    <div class="mt-1 flex items-center gap-2 px-1 opacity-0 transition-opacity \
                                                group-hover:opacity-100 focus-within:opacity-100 pointer-coarse:opacity-100">
                                        <CopyButton text=copy_text />
                                        <Show when=move || can_regen>
                                            <button
                                                class="inline-flex items-center gap-1 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
                                                title="Regenerate response"
                                                on:click=move |_| {
                                                    if let Some(cb) = on_regenerate { cb.run(msg_id); }
                                                }
                                            >
                                                <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/></svg>
                                                "Regenerate"
                                            </button>
                                        </Show>
                                        {model_used.clone().map(|m| view! {
                                            <span class="rounded bg-muted px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
                                                {m}
                                            </span>
                                        })}
                                        <span class="text-[11px] text-muted-foreground">{time.clone()}</span>
                                    </div>
                                })
                            }
                        }
                    }
                </div>
            </div>
        }.into_any()
    }
}

/// A small copy-to-clipboard button that shows a checkmark once the clipboard
/// write actually succeeds.
#[component]
fn CopyButton(text: String) -> impl IntoView {
    let copied = RwSignal::new(false);
    let toast = crate::components::ui::toast::use_toast();
    view! {
        <button
            class="inline-flex items-center gap-1 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
            title="Copy"
            on:click=move |_| {
                #[cfg(feature = "hydrate")]
                {
                    let text = text.clone();
                    let toast = toast.clone();
                    leptos::task::spawn_local(async move {
                        let Some(win) = leptos::web_sys::window() else { return };
                        let promise = win.navigator().clipboard().write_text(&text);
                        match wasm_bindgen_futures::JsFuture::from(promise).await {
                            Ok(_) => {
                                copied.set(true);
                                leptos::leptos_dom::helpers::set_timeout(
                                    move || copied.set(false),
                                    std::time::Duration::from_millis(1200),
                                );
                            }
                            Err(_) => toast.error("Couldn't copy to clipboard"),
                        }
                    });
                }
                #[cfg(not(feature = "hydrate"))]
                {
                    let _ = (&text, &toast);
                }
            }
        >
            {move || if copied.get() {
                view! {
                    <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
                    "Copied"
                }.into_any()
            } else {
                view! {
                    <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>
                    "Copy"
                }.into_any()
            }}
        </button>
    }
}

/// Collapsible "thinking" / reasoning panel for reasoning models. Auto-expanded
/// while streaming, collapsed by default once the answer is available.
#[component]
fn ThinkingBlock(
    #[prop(into)] text: String,
    /// True while reasoning is still streaming (no answer yet).
    streaming: bool,
) -> impl IntoView {
    let expanded = RwSignal::new(streaming);

    view! {
        <div class="mb-2 rounded-lg border border-border/70 bg-muted/40">
            <button
                class="flex w-full items-center gap-1.5 px-2.5 py-1.5 text-left text-xs font-medium text-muted-foreground transition-colors hover:text-foreground"
                aria-expanded=move || expanded.get().to_string()
                on:click=move |_| expanded.update(|e| *e = !*e)
            >
                <svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2a7 7 0 0 0-7 7c0 2.38 1.19 4.47 3 5.74V17a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1v-2.26c1.81-1.27 3-3.36 3-5.74a7 7 0 0 0-7-7Z"/><path d="M9 21h6"/></svg>
                <span>{if streaming { "Thinking..." } else { "Reasoning" }}</span>
                <svg
                    xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24"
                    fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
                    class=move || {
                        let base = "ml-auto transition-transform";
                        if expanded.get() { format!("{base} rotate-180") } else { base.to_string() }
                    }
                >
                    <path d="m6 9 6 6 6-6"/>
                </svg>
            </button>
            <Show when=move || expanded.get()>
                <div class="border-t border-border/60 px-2.5 py-2">
                    <p class="whitespace-pre-wrap break-words text-xs italic leading-relaxed text-muted-foreground">
                        {text.clone()}
                    </p>
                </div>
            </Show>
        </div>
    }
}

#[component]
fn ImageGrid(
    attachments: Vec<Attachment>,
    is_user: bool,
) -> impl IntoView {
    if attachments.is_empty() {
        return None;
    }

    // Clicking an image opens it full-size in a lightbox overlay.
    let lightbox = RwSignal::new(Option::<String>::None);

    let border_class = if is_user {
        "border-primary-foreground/20"
    } else {
        "border-border"
    };

    let count = attachments.len();
    let grid_class = if count == 1 {
        "mb-2"
    } else if count <= 4 {
        "grid grid-cols-2 gap-1.5 mb-2"
    } else {
        "grid grid-cols-3 gap-1.5 mb-2"
    };

    let imgs: Vec<_> = attachments.iter().map(|att| {
        // `file_path` already includes the `attachments/` prefix; the static
        // route serves it at `/attachments/...`, so just prepend a slash.
        let src = format!("/{}", att.file_path);
        let alt = att.original_name.clone().unwrap_or_else(|| "attached image".to_string());
        let img_class = if count == 1 {
            format!("rounded-lg max-w-full max-h-64 object-contain border {}", border_class)
        } else {
            format!("rounded-lg w-full h-32 object-cover border {}", border_class)
        };
        let src_for_click = src.clone();
        view! {
            <button
                type="button"
                class="block cursor-zoom-in focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded-lg"
                aria-label="View image full size"
                on:click=move |_| lightbox.set(Some(src_for_click.clone()))
            >
                <img src=src alt=alt class=img_class />
            </button>
        }
    }).collect();

    Some(view! {
        <div class=grid_class>
            {imgs}
        </div>
        <Show when=move || lightbox.get().is_some()>
            <Lightbox
                src=Signal::derive(move || lightbox.get().unwrap_or_default())
                on_close=Callback::new(move |_| lightbox.set(None))
            />
        </Show>
    })
}

/// Fullscreen image overlay. Closes on click or Escape.
#[component]
fn Lightbox(
    #[prop(into)] src: Signal<String>,
    on_close: Callback<()>,
) -> impl IntoView {
    use leptos::callback::Callable;

    let handle = leptos::leptos_dom::helpers::window_event_listener(
        leptos::ev::keydown,
        move |e: leptos::web_sys::KeyboardEvent| {
            if e.key() == "Escape" {
                on_close.run(());
            }
        },
    );
    on_cleanup(move || handle.remove());

    view! {
        <div
            class="fixed inset-0 z-[90] flex cursor-zoom-out items-center justify-center bg-black/80 p-4 animate-fade-in"
            role="dialog"
            aria-modal="true"
            aria-label="Image preview"
            on:click=move |_| on_close.run(())
        >
            <img
                src=move || src.get()
                alt="Full size preview"
                class="max-h-[90vh] max-w-[90vw] rounded-lg object-contain shadow-2xl"
            />
            <button
                class="absolute right-4 top-4 flex h-9 w-9 items-center justify-center rounded-full \
                       bg-black/60 text-white transition-colors hover:bg-black/80"
                aria-label="Close preview"
                on:click=move |ev| {
                    ev.stop_propagation();
                    on_close.run(());
                }
            >
                <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24"
                     fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M18 6 6 18"/>
                    <path d="m6 6 12 12"/>
                </svg>
            </button>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::parse_thinking;

    #[test]
    fn plain_text_has_no_reasoning() {
        let p = parse_thinking("Hello there!");
        assert!(p.reasoning.is_none());
        assert!(!p.reasoning_open);
        assert_eq!(p.answer, "Hello there!");
    }

    #[test]
    fn completed_think_block_is_split() {
        let p = parse_thinking("<think>pondering deeply</think>The answer is 42.");
        assert_eq!(p.reasoning.as_deref(), Some("pondering deeply"));
        assert!(!p.reasoning_open);
        assert_eq!(p.answer, "The answer is 42.");
    }

    #[test]
    fn unclosed_think_block_is_streaming() {
        let p = parse_thinking("<think>still thinking");
        assert_eq!(p.reasoning.as_deref(), Some("still thinking"));
        assert!(p.reasoning_open);
        assert!(p.answer.is_empty());
    }

    #[test]
    fn empty_reasoning_is_dropped() {
        let p = parse_thinking("<think>  </think>Answer.");
        assert!(p.reasoning.is_none());
        assert_eq!(p.answer, "Answer.");
    }
}
