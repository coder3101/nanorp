use super::overlay::OverlayGuard;
use leptos::html;
use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SheetSide {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

impl SheetSide {
    fn enter_animation(self) -> &'static str {
        match self {
            SheetSide::Left => "animate-slide-in-from-left",
            SheetSide::Right => "animate-slide-in-from-right",
            SheetSide::Top => "animate-slide-in-from-top",
            SheetSide::Bottom => "animate-slide-in-from-bottom",
        }
    }

    fn fixed_classes(self) -> &'static str {
        match self {
            SheetSide::Left => {
                "fixed inset-y-0 left-0 z-50 h-viewport w-80 max-w-[85vw] py-safe pl-safe"
            }
            SheetSide::Right => {
                "fixed inset-y-0 right-0 z-50 h-viewport w-80 max-w-[85vw] py-safe pr-safe"
            }
            SheetSide::Top => "fixed inset-x-0 top-0 z-50 max-h-viewport w-full px-safe pt-safe",
            SheetSide::Bottom => {
                "fixed inset-x-0 bottom-0 z-50 max-h-viewport w-full px-safe pb-safe"
            }
        }
    }
}

#[component]
pub fn Sheet(
    open: RwSignal<bool>,
    #[prop(optional)] side: SheetSide,
    /// Accessible dialog name. Defaults to a generic, non-empty name.
    #[prop(into, optional)]
    label: MaybeProp<String>,
    /// Adds a compact internal header with an explicit 44px close target.
    #[prop(optional)]
    show_close: bool,
    children: ChildrenFn,
) -> impl IntoView {
    let children = std::sync::Arc::new(children);

    view! {
        <Show when=move || open.get()>
            {
                let children = children.clone();
                view! {
                    <SheetContent side=side open=open label=label show_close=show_close>
                        {children()}
                    </SheetContent>
                }
            }
        </Show>
    }
}

#[component]
fn SheetContent(
    side: SheetSide,
    open: RwSignal<bool>,
    #[prop(into, optional)] label: MaybeProp<String>,
    show_close: bool,
    children: Children,
) -> impl IntoView {
    let content_ref = NodeRef::<html::Div>::new();
    let overlay = OverlayGuard::new();
    let fixed = side.fixed_classes();
    let enter = side.enter_animation();
    let request_close = move || open.set(false);

    let escape_handle = leptos::leptos_dom::helpers::window_event_listener(
        leptos::ev::keydown,
        move |event: leptos::web_sys::KeyboardEvent| {
            if event.key() == "Escape" && !event.default_prevented() && overlay.is_topmost() {
                event.prevent_default();
                event.stop_immediate_propagation();
                request_close();
            }
        },
    );
    on_cleanup(move || escape_handle.remove());

    #[cfg(feature = "hydrate")]
    {
        use leptos::wasm_bindgen::JsCast;

        let previously_focused = StoredValue::new_local(
            leptos::web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.active_element()),
        );
        Effect::new(move |_| {
            if let Some(content) = content_ref.get() {
                overlay.activate_background(&content);
                super::overlay::focus_initial(&content);
            }
        });
        on_cleanup(move || {
            if overlay.close() {
                if let Some(element) = previously_focused.get_value() {
                    if let Some(element) = element.dyn_ref::<leptos::web_sys::HtmlElement>() {
                        let _ = element.focus();
                    }
                }
            }
        });
    }

    #[cfg(not(feature = "hydrate"))]
    on_cleanup(move || {
        overlay.close();
    });

    let on_keydown = move |event: leptos::web_sys::KeyboardEvent| {
        if !overlay.is_topmost() || event.default_prevented() {
            return;
        }
        match event.key().as_str() {
            "Escape" => {
                event.prevent_default();
                event.stop_propagation();
                request_close();
            }
            "Tab" => {
                #[cfg(feature = "hydrate")]
                if let Some(content) = content_ref.get_untracked() {
                    super::overlay::trap_tab(&event, &content);
                }
                #[cfg(not(feature = "hydrate"))]
                let _ = &event;
            }
            _ => {}
        }
    };

    view! {
        <div class="fixed inset-0 z-50">
            <div
                class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm animate-fade-in"
                aria-hidden="true"
                data-overlay-backdrop
                on:click=move |_| {
                    if overlay.is_topmost() {
                        request_close();
                    }
                }
            />
            <div
                class=format!("{} flex flex-col overflow-hidden overscroll-contain bg-background shadow-xl {}", fixed, enter)
                node_ref=content_ref
                role="dialog"
                aria-modal="true"
                aria-label=move || label.get().unwrap_or_else(|| "Dialog".to_string())
                tabindex="-1"
                on:keydown=on_keydown
            >
                {show_close.then(|| view! {
                    <div class="flex h-14 shrink-0 items-center justify-end border-b border-border px-3">
                        <button
                            type="button"
                            class="inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                            aria-label="Close panel"
                            on:click=move |_| request_close()
                        >
                            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24"
                                 fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
                                 stroke-linejoin="round" aria-hidden="true">
                                <path d="M18 6 6 18"/>
                                <path d="m6 6 12 12"/>
                            </svg>
                        </button>
                    </div>
                })}
                <div class="scroll-area min-h-0 flex-1 overflow-y-auto overscroll-contain">
                    {children()}
                </div>
            </div>
        </div>
    }
}
