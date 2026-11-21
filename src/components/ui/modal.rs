//! Accessible modal dialog with nested-overlay dismissal, focus management,
//! scroll locking, and a full-height phone layout. Width and content padding
//! remain caller-controlled through `class`.

use super::overlay::OverlayGuard;
use leptos::callback::Callable;
use leptos::html;
use leptos::prelude::*;

#[component]
pub fn Modal(
    open: RwSignal<bool>,
    /// Accessible name for the dialog.
    #[prop(into)]
    label: Signal<String>,
    /// Extra classes for the panel (typically a max-width and padding).
    #[prop(into, optional)]
    class: MaybeProp<String>,
    /// Invoked when the user tries to dismiss the modal (overlay click or
    /// Escape). Defaults to simply closing; pass a callback to intercept,
    /// e.g. to confirm discarding unsaved changes.
    #[prop(optional)]
    on_close: Option<Callback<()>>,
    children: ChildrenFn,
) -> impl IntoView {
    let children = std::sync::Arc::new(children);

    view! {
        <Show when=move || open.get()>
            {
                let children = children.clone();
                let class = class;
                view! {
                    <ModalPanel open=open label=label class=class on_close=on_close>
                        {children()}
                    </ModalPanel>
                }
            }
        </Show>
    }
}

/// Rendered only while the modal is open, so listeners and guards always
/// mount/unmount together with the dialog itself.
#[component]
fn ModalPanel(
    open: RwSignal<bool>,
    #[prop(into)] label: Signal<String>,
    #[prop(into, optional)] class: MaybeProp<String>,
    on_close: Option<Callback<()>>,
    children: Children,
) -> impl IntoView {
    let panel_ref = NodeRef::<html::Div>::new();
    let overlay = OverlayGuard::new();

    let request_close = move || match on_close {
        Some(callback) => callback.run(()),
        None => open.set(false),
    };

    // This is a fallback for focus briefly being outside the panel. Normal
    // Escape handling happens on the panel so an inner menu can consume it.
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
            if let Some(panel) = panel_ref.get() {
                overlay.activate_background(&panel);
                super::overlay::focus_initial(&panel);
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
                if let Some(panel) = panel_ref.get_untracked() {
                    super::overlay::trap_tab(&event, &panel);
                }
                #[cfg(not(feature = "hydrate"))]
                let _ = &event;
            }
            _ => {}
        }
    };

    let panel_class = move || {
        format!(
            "modal-panel scroll-area relative z-50 w-full overflow-y-auto overscroll-contain \
             rounded-xl border border-border bg-card text-card-foreground shadow-2xl \
             animate-fade-in focus:outline-none {}",
            class.get().unwrap_or_default()
        )
    };

    view! {
        <div class="overlay-viewport fixed inset-0 z-50 flex items-stretch justify-center sm:items-center">
            <div
                class="fixed inset-0 bg-black/60 backdrop-blur-sm animate-fade-in"
                aria-hidden="true"
                data-overlay-backdrop
                on:click=move |_| {
                    if overlay.is_topmost() {
                        request_close();
                    }
                }
            />
            <div
                class=panel_class
                node_ref=panel_ref
                role="dialog"
                aria-modal="true"
                aria-label=move || label.get()
                tabindex="-1"
                on:keydown=on_keydown
            >
                {children()}
            </div>
        </div>
    }
}
