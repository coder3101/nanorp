use crate::components::ui::toast::use_toast;
use crate::models::message::ImageUpload;
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
use crate::models::message::{MAX_IMAGES_PER_MESSAGE, MAX_IMAGE_BYTES};

#[component]
pub fn ChatInput(
    on_send: Callback<(String, Vec<ImageUpload>)>,
    #[prop(optional)] on_stop: Option<Callback<()>>,
    #[prop(optional)] is_streaming: Signal<bool>,
    #[prop(optional)] disabled: Signal<bool>,
) -> impl IntoView {
    let toast = use_toast();
    let message = RwSignal::new(String::new());
    let pending_images = RwSignal::new(Vec::<PendingImage>::new());
    let textarea_ref = NodeRef::<html::Textarea>::new();
    let file_input_ref = NodeRef::<html::Input>::new();

    let can_send = Signal::derive(move || {
        if is_streaming.get() || disabled.get() {
            return false;
        }
        let has_text = !message.get().trim().is_empty();
        let has_images = !pending_images.get().is_empty();
        has_text || has_images
    });

    let remove_image = move |id: Uuid| {
        pending_images.update(|imgs| imgs.retain(|img| img.id != id));
    };

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
        on_send.run((text, imgs));
        message.set(String::new());
        pending_images.set(Vec::new());
        if let Some(ta) = textarea_ref.get() {
            ta.set_value("");
        }
    };

    let handle_keydown = move |ev: leptos::web_sys::KeyboardEvent| {
        if ev.key() == "Enter" && !ev.shift_key() && !ev.ctrl_key() {
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
        <div class="sticky bottom-0 border-t border-border bg-background/95 px-4 pb-4 pt-3 backdrop-blur \
                    supports-[backdrop-filter]:bg-background/60">
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
                                    class="absolute -right-1.5 -top-1.5 flex h-5 w-5 items-center justify-center \
                                           rounded-full bg-destructive text-xs text-destructive-foreground shadow \
                                           transition-opacity"
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
                        accept="image/*"
                        multiple=true
                        class="hidden"
                        node_ref=file_input_ref
                        on:change=handle_file_select
                    />
                    <button
                        class="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-xl \
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
                        class="max-h-[160px] min-h-[36px] flex-1 resize-none border-0 bg-transparent px-1 py-2 \
                               text-sm leading-relaxed placeholder:text-muted-foreground \
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
                        disabled=move || is_streaming.get()
                    />

                    <Show
                        when=move || is_streaming.get()
                        fallback=move || view! {
                            <button
                                class="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-xl \
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
                            class="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-xl \
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
fn process_file(
    file: web_sys::File,
    pending_images: &RwSignal<Vec<PendingImage>>,
    toast: &crate::components::ui::toast::UseToast,
) {
    let content_type = file.type_();
    if !content_type.starts_with("image/") {
        toast.warning(format!(
            "\"{}\" isn't an image — only images can be attached",
            file.name()
        ));
        return;
    }

    let size = file.size() as u64;
    if size > MAX_IMAGE_BYTES as u64 {
        toast.warning(format!("\"{}\" is too large (max 10 MB)", file.name()));
        return;
    }

    let current_count = pending_images.get().len();
    if current_count >= MAX_IMAGES_PER_MESSAGE {
        toast.warning(format!(
            "You can attach up to {MAX_IMAGES_PER_MESSAGE} images per message"
        ));
        return;
    }

    let name = file.name();
    let pending = *pending_images;

    let reader = leptos::web_sys::FileReader::new().unwrap();
    let reader_clone = reader.clone();

    let onload =
        leptos::wasm_bindgen::prelude::Closure::wrap(Box::new(move |_: leptos::web_sys::Event| {
            let result = reader_clone.result().unwrap();
            let data_url = result.as_string().unwrap();
            let base64 = data_url.split(',').nth(1).unwrap_or("").to_string();
            pending.update(|imgs| {
                // Re-checked here because reads complete asynchronously, so a
                // multi-file selection could otherwise overshoot the cap.
                if imgs.len() < MAX_IMAGES_PER_MESSAGE {
                    imgs.push(PendingImage {
                        id: Uuid::new_v4(),
                        data: base64,
                        content_type: content_type.clone(),
                        original_name: Some(name.clone()),
                        file_size: size,
                    });
                }
            });
        }) as Box<dyn FnMut(_)>);

    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
    onload.forget();
    reader.read_as_data_url(&file).unwrap();
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
