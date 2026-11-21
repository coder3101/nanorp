use crate::components::ui::toast::use_toast;
use crate::models::message::ImageUpload;
use crate::services::generation_tracker::use_generation_tracker;
use leptos::callback::Callable;
use leptos::html;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct PendingImage {
    id: Uuid,
    data: String,
    content_type: String,
    original_name: Option<String>,
    file_size: u64,
}

#[cfg(feature = "hydrate")]
use crate::models::message::{MAX_IMAGES_PER_MESSAGE, MAX_IMAGE_BYTES, MAX_TOTAL_IMAGE_BYTES};

#[component]
pub fn ChatInput(
    session_id: Uuid,
    on_send: Callback<(String, Vec<ImageUpload>)>,
    #[prop(optional)] on_stop: Option<Callback<()>>,
    #[prop(optional)] is_streaming: Signal<bool>,
    #[prop(optional)] disabled: Signal<bool>,
    /// Incremented only after the server has accepted/persisted this draft.
    #[prop(optional)]
    clear_revision: Signal<u64>,
) -> impl IntoView {
    let toast = use_toast();
    let tracker = use_generation_tracker();
    let initial_draft = tracker.draft(session_id);
    let message = RwSignal::new(initial_draft.content);
    let pending_images = RwSignal::new(
        initial_draft
            .images
            .into_iter()
            .map(|img| PendingImage {
                id: Uuid::new_v4(),
                file_size: img.data.len() as u64 * 3 / 4,
                data: img.data,
                content_type: img.content_type,
                original_name: img.original_name,
            })
            .collect::<Vec<_>>(),
    );
    let textarea_ref = NodeRef::<html::Textarea>::new();
    let file_input_ref = NodeRef::<html::Input>::new();
    #[cfg(not(feature = "hydrate"))]
    let coarse_pointer = StoredValue::new(false);
    #[cfg(feature = "hydrate")]
    let coarse_pointer = StoredValue::new(
        leptos::web_sys::window()
            .and_then(|w| w.match_media("(pointer: coarse)").ok().flatten())
            .map(|m| m.matches())
            .unwrap_or(false),
    );

    let can_send = Signal::derive(move || {
        if is_streaming.get() || disabled.get() {
            return false;
        }
        let has_text = !message.get().trim().is_empty();
        let has_images = !pending_images.get().is_empty();
        has_text || has_images
    });

    // Drafts, including newly selected images, belong to this conversation.
    // They follow a remount of the same chat but never leak into another one.
    // Content and images are saved by separate effects so that typing never
    // re-clones the (potentially multi-MB) attached image base64 on every key.
    let content_tracker = tracker.clone();
    Effect::new(move |_| {
        content_tracker.set_draft_content(session_id, message.get());
    });
    let image_tracker = tracker.clone();
    Effect::new(move |_| {
        let images = pending_images
            .get()
            .into_iter()
            .map(|img| ImageUpload {
                data: img.data,
                content_type: img.content_type,
                original_name: img.original_name,
            })
            .collect();
        image_tracker.set_draft_images(session_id, images);
    });

    // A send is cleared only after acknowledgement, not optimistically. The
    // explicit revision also coalesces a stream acknowledgement and history
    // reconciliation into one harmless clear operation.
    let last_clear_revision = StoredValue::new(clear_revision.get_untracked());
    Effect::new(move |_| {
        let revision = clear_revision.get();
        if revision == last_clear_revision.get_value() {
            return;
        }
        last_clear_revision.set_value(revision);
        message.set(String::new());
        pending_images.set(Vec::new());
        tracker.clear_draft(session_id);
        if let Some(textarea) = textarea_ref.get_untracked() {
            textarea.set_value("");
            let el: &leptos::web_sys::HtmlElement = textarea.as_ref();
            let _ = el.style().set_property("height", "auto");
        }
    });

    let remove_image = move |id: Uuid| {
        pending_images.update(|imgs| imgs.retain(|img| img.id != id));
    };

    // Restored drafts should open at their content height, not as a one-line
    // field with hidden text.
    Effect::new(move |_| {
        if let Some(textarea) = textarea_ref.get() {
            if !message.get_untracked().is_empty() {
                let scroll_height = textarea.scroll_height();
                let el: &leptos::web_sys::HtmlElement = textarea.as_ref();
                let _ = el
                    .style()
                    .set_property("height", &format!("{scroll_height}px"));
            }
        }
    });

    let toast_for_files = toast.clone();
    let handle_file_select = move |_ev: leptos::web_sys::Event| {
        #[cfg(feature = "hydrate")]
        {
            let target = _ev.target().unwrap();
            let input = target.unchecked_into::<web_sys::HtmlInputElement>();
            if let Some(files) = input.files() {
                for i in 0..files.length() {
                    if let Some(file) = files.get(i) {
                        process_file(file, &pending_images, &toast_for_files);
                    }
                }
            }
            input.set_value("");
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = &toast_for_files;
    };

    let toast_for_paste = toast.clone();
    let handle_paste = move |_ev: leptos::web_sys::ClipboardEvent| {
        #[cfg(feature = "hydrate")]
        {
            if let Some(data) = _ev.clipboard_data() {
                let items = data.items();
                for i in 0..items.length() {
                    if let Some(item) = items.get(i) {
                        if item.type_().starts_with("image/") {
                            _ev.prevent_default();
                            if let Ok(Some(blob)) = item.get_as_file() {
                                process_file(blob, &pending_images, &toast_for_paste);
                            }
                        }
                    }
                }
            }
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = &toast_for_paste;
    };

    let send_message = move || {
        if !can_send.get() {
            return;
        }
        let text = message.get();
        let imgs: Vec<ImageUpload> = pending_images
            .get()
            .into_iter()
            .map(|pi| ImageUpload {
                data: pi.data,
                content_type: pi.content_type,
                original_name: pi.original_name,
            })
            .collect();
        // Keep the draft and image previews visible until the server confirms
        // that the user turn is persisted. Failures can then be retried intact.
        on_send.run((text, imgs));
    };

    let handle_keydown = move |ev: leptos::web_sys::KeyboardEvent| {
        let is_enter = ev.key() == "Enter";
        let composing = ev.is_composing() || ev.key_code() == 229;
        if !is_enter || ev.shift_key() || composing {
            return;
        }
        // Desktop keeps the familiar Enter-to-send behavior. On touch/software
        // keyboards Enter creates a newline; the explicit Send button and
        // Ctrl/Cmd+Enter remain available for submission.
        let modified_send = ev.ctrl_key() || ev.meta_key();
        if modified_send || (!coarse_pointer.get_value() && !ev.alt_key()) {
            ev.prevent_default();
            send_message();
        }
    };

    let stop_generation = move || {
        if let Some(cb) = on_stop {
            cb.run(());
        }
    };

    view! {
        <div class="sticky bottom-0 border-t border-border bg-background/95 px-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] pt-3 backdrop-blur \
                    supports-[backdrop-filter]:bg-background/60 sm:px-4">
            <div class="mx-auto max-w-3xl">
                <Show when=move || !pending_images.get().is_empty()>
                    <div class="mb-2 flex gap-2 overflow-x-auto pb-1">
                        <For
                            each=move || pending_images.get()
                            key=|img| img.id
                            let:img
                        >
                            <div class="group relative shrink-0">
                                <img
                                    src=format!("data:{};base64,{}", img.content_type, img.data)
                                    class="h-20 w-20 rounded-lg border border-border object-cover"
                                />
                                <button
                                    class="absolute -right-2 -top-2 flex h-8 w-8 items-center justify-center \
                                           rounded-full bg-destructive text-sm text-destructive-foreground shadow \
                                           transition-opacity sm:h-6 sm:w-6"
                                    aria-label="Remove image"
                                    on:click=move |_| remove_image(img.id)
                                >
                                    "✕"
                                </button>
                                <span class="absolute bottom-1 left-1 rounded bg-black/60 px-1 text-[10px] text-white">
                                    {format_file_size(img.file_size)}
                                </span>
                            </div>
                        </For>
                    </div>
                </Show>

                // Unified input pill.
                <div class="flex items-end gap-1.5 rounded-2xl border border-input bg-background p-1.5 shadow-sm \
                            transition-colors focus-within:ring-2 focus-within:ring-ring">
                    <input
                        type="file"
                        accept="image/png,image/jpeg,image/webp,image/gif,.heic,.heif"
                        multiple=true
                        class="hidden"
                        node_ref=file_input_ref
                        on:change=handle_file_select
                    />
                    <button
                        class="inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-xl \
                               text-muted-foreground transition-colors hover:bg-accent hover:text-foreground \
                               disabled:opacity-50"
                        title="Attach image"
                        aria-label="Attach image"
                        disabled=move || is_streaming.get()
                        on:click=move |_| {
                            if let Some(input) = file_input_ref.get() {
                                input.click();
                            }
                        }
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24"
                             fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
                             stroke-linejoin="round">
                            <path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/>
                        </svg>
                    </button>

                    <textarea
                        class="max-h-[35dvh] min-h-[40px] flex-1 resize-none overflow-y-auto border-0 bg-transparent px-1 py-2 \
                               text-base leading-relaxed placeholder:text-muted-foreground sm:max-h-[160px] sm:text-sm \
                               focus:outline-none focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
                        placeholder=move || if disabled.get() { "Select a model to start chatting..." } else { "Type a message..." }
                        aria-label="Message"
                        rows="1"
                        node_ref=textarea_ref
                        prop:value=move || message.get()
                        on:input=move |ev| {
                            let target = ev.target().unwrap();
                            let input = target.unchecked_into::<leptos::web_sys::HtmlTextAreaElement>();
                            message.set(input.value());
                            let el: &leptos::web_sys::HtmlElement = input.as_ref();
                            let style = el.style();
                            let _ = style.set_property("height", "auto");
                            let _ = style.set_property("height", &format!("{}px", input.scroll_height()));
                        }
                        on:keydown=handle_keydown
                        on:paste=handle_paste
                        // Keep drafting the next message available during generation.
                        disabled=move || disabled.get()
                    />

                    <Show
                        when=move || is_streaming.get()
                        fallback=move || view! {
                            <button
                                class="inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-xl \
                                       bg-primary text-primary-foreground shadow transition-all \
                                       hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-40"
                                title="Send"
                                aria-label="Send message"
                                disabled=move || !can_send.get()
                                on:click=move |_| send_message()
                            >
                                <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24"
                                     fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
                                     stroke-linejoin="round">
                                    <path d="m5 12 7-7 7 7"/>
                                    <path d="M12 19V5"/>
                                </svg>
                            </button>
                        }
                    >
                        <button
                            class="inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-xl \
                                   bg-destructive text-destructive-foreground shadow transition-colors \
                                   hover:bg-destructive/90"
                            title="Stop generating"
                            aria-label="Stop generating"
                            on:click=move |_| stop_generation()
                        >
                            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24"
                                 fill="currentColor" stroke="none">
                                <rect x="6" y="6" width="12" height="12" rx="2"/>
                            </svg>
                        </button>
                    </Show>
                </div>
            </div>
        </div>
    }
}

#[cfg(feature = "hydrate")]
/// Returns `true` if the MIME type is HEIF/HEIC (iPhone camera format) and
/// needs conversion to JPEG before upload.
fn needs_heif_conversion(content_type: &str) -> bool {
    matches!(
        content_type,
        "image/heic" | "image/heif" | "image/heic-sequence" | "image/heif-sequence"
    )
}

#[cfg(feature = "hydrate")]
fn process_file(
    file: web_sys::File,
    pending_images: &RwSignal<Vec<PendingImage>>,
    toast: &crate::components::ui::toast::UseToast,
) {
    let content_type = file.type_();

    // Some browsers report an empty MIME type for HEIC files; detect by
    // extension in that case.
    let is_heif_by_ext = {
        let lower = file.name().to_lowercase();
        lower.ends_with(".heic") || lower.ends_with(".heif")
    };
    let is_heif =
        needs_heif_conversion(&content_type) || (content_type.is_empty() && is_heif_by_ext);

    let supported = matches!(
        content_type.as_str(),
        "image/png" | "image/jpeg" | "image/jpg" | "image/webp" | "image/gif"
    );
    if !supported && !is_heif {
        toast.warning(format!(
            "\"{}\" isn't supported — choose PNG, JPEG, WebP, GIF, or HEIC",
            file.name()
        ));
        return;
    }

    let size = file.size() as u64;
    if size > MAX_IMAGE_BYTES as u64 {
        toast.warning(format!("\"{}\" is too large (max 10 MB)", file.name()));
        return;
    }

    if !within_combined_limits(&pending_images.get_untracked(), size, 1) {
        toast.warning(format!(
            "Attachments are limited to {MAX_IMAGES_PER_MESSAGE} images and {} MB in total",
            MAX_TOTAL_IMAGE_BYTES / (1024 * 1024)
        ));
        return;
    }

    if is_heif {
        // Convert HEIF/HEIC → JPEG via Canvas. Safari natively decodes HEIC;
        // other browsers may not, in which case the image load will fail and
        // the user will see an error toast.
        convert_heif_to_jpeg(file, pending_images, toast);
    } else {
        read_file_directly(file, content_type, pending_images, toast);
    }
}

#[cfg(feature = "hydrate")]
fn within_combined_limits(
    images: &[PendingImage],
    additional_bytes: u64,
    additional_count: usize,
) -> bool {
    let count = images.len().saturating_add(additional_count);
    let total = images.iter().try_fold(additional_bytes, |sum, image| {
        sum.checked_add(image.file_size)
    });
    count <= MAX_IMAGES_PER_MESSAGE
        && total.is_some_and(|bytes| bytes <= MAX_TOTAL_IMAGE_BYTES as u64)
}

#[cfg(feature = "hydrate")]
fn read_file_directly(
    file: web_sys::File,
    content_type: String,
    pending_images: &RwSignal<Vec<PendingImage>>,
    toast: &crate::components::ui::toast::UseToast,
) {
    let name = file.name();
    let size = file.size() as u64;
    let pending = *pending_images;
    let reader = match leptos::web_sys::FileReader::new() {
        Ok(reader) => reader,
        Err(_) => {
            toast.error("Couldn't start reading this image");
            return;
        }
    };
    let reader_clone = reader.clone();
    let toast_for_load = toast.clone();
    let name_for_load = name.clone();
    let onload =
        leptos::wasm_bindgen::prelude::Closure::wrap(Box::new(move |_: leptos::web_sys::Event| {
            let data_url = reader_clone
                .result()
                .ok()
                .and_then(|result| result.as_string());
            let Some(data_url) = data_url else {
                toast_for_load.error(format!("Couldn't read \"{name_for_load}\""));
                return;
            };
            let base64 = data_url.split(',').nth(1).unwrap_or("").to_string();
            if base64.is_empty() {
                toast_for_load.error(format!("\"{name_for_load}\" contains no image data"));
                return;
            }
            pending.update(|imgs| {
                if within_combined_limits(imgs, size, 1) {
                    imgs.push(PendingImage {
                        id: Uuid::new_v4(),
                        data: base64,
                        content_type: content_type.clone(),
                        original_name: Some(name_for_load.clone()),
                        file_size: size,
                    });
                }
            });
        }) as Box<dyn FnMut(_)>);

    let toast_for_error = toast.clone();
    let onerror =
        leptos::wasm_bindgen::prelude::Closure::wrap(Box::new(move |_: leptos::web_sys::Event| {
            toast_for_error.error(format!("Couldn't read \"{name}\""));
        }) as Box<dyn FnMut(_)>);
    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
    reader.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onload.forget();
    onerror.forget();
    if reader.read_as_data_url(&file).is_err() {
        toast.error("Couldn't start reading this image");
    }
}

#[cfg(feature = "hydrate")]
fn convert_heif_to_jpeg(
    file: web_sys::File,
    pending_images: &RwSignal<Vec<PendingImage>>,
    toast: &crate::components::ui::toast::UseToast,
) {
    let pending = *pending_images;
    let toast = toast.clone();
    let name = file.name();

    // Create an object URL for the file so the browser's native image decoder
    // can handle it (Safari decodes HEIC natively).
    let blob: &web_sys::Blob = file.as_ref();
    let obj_url = web_sys::Url::create_object_url_with_blob(blob).unwrap();

    let document = leptos::prelude::document();
    let img: web_sys::HtmlImageElement = document.create_element("img").unwrap().unchecked_into();

    let obj_url_for_cleanup = obj_url.clone();
    let name_for_err = name.clone();
    let toast_for_err = toast.clone();

    // On error — the browser can't decode this HEIC file.
    let onerror = leptos::wasm_bindgen::prelude::Closure::wrap(Box::new(
        move |_: leptos::web_sys::Event| {
            let _ = web_sys::Url::revoke_object_url(&obj_url_for_cleanup);
            toast_for_err.warning(format!(
            "Could not decode \"{name_for_err}\". Your browser may not support HEIC — try converting to JPEG first."
        ));
        },
    ) as Box<dyn FnMut(_)>);

    let img_clone = img.clone();
    let obj_url_for_load = obj_url.clone();

    let onload =
        leptos::wasm_bindgen::prelude::Closure::wrap(Box::new(move |_: leptos::web_sys::Event| {
            let width = img_clone.natural_width();
            let height = img_clone.natural_height();

            // Downscale very large images (e.g. 12MP iPhone photos) to max
            // 2048px on the longest side to keep payload reasonable.
            let max_dim: u32 = 2048;
            let (out_w, out_h) = if width > max_dim || height > max_dim {
                let scale = max_dim as f64 / width.max(height) as f64;
                (
                    (width as f64 * scale) as u32,
                    (height as f64 * scale) as u32,
                )
            } else {
                (width, height)
            };

            let document = leptos::prelude::document();
            let canvas: web_sys::HtmlCanvasElement =
                document.create_element("canvas").unwrap().unchecked_into();
            canvas.set_width(out_w);
            canvas.set_height(out_h);

            let ctx: web_sys::CanvasRenderingContext2d =
                canvas.get_context("2d").unwrap().unwrap().unchecked_into();
            ctx.draw_image_with_html_image_element_and_dw_and_dh(
                &img_clone,
                0.0,
                0.0,
                out_w as f64,
                out_h as f64,
            )
            .unwrap();

            // Export as JPEG with quality 0.85
            let quality = leptos::wasm_bindgen::JsValue::from_f64(0.85);
            let data_url = canvas
                .to_data_url_with_type_and_encoder_options("image/jpeg", &quality)
                .unwrap_or_default();
            let base64 = data_url.split(',').nth(1).unwrap_or("").to_string();
            let byte_len = base64.len() * 3 / 4; // approximate decoded size

            let _ = web_sys::Url::revoke_object_url(&obj_url_for_load);

            pending.update(|imgs| {
                if within_combined_limits(imgs, byte_len as u64, 1) {
                    imgs.push(PendingImage {
                        id: Uuid::new_v4(),
                        data: base64,
                        content_type: "image/jpeg".to_string(),
                        original_name: Some(name.clone()),
                        file_size: byte_len as u64,
                    });
                }
            });
        }) as Box<dyn FnMut(_)>);

    img.set_onload(Some(onload.as_ref().unchecked_ref()));
    img.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onload.forget();
    onerror.forget();
    img.set_src(&obj_url);
}

fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
