//! Accessible modal dialog: overlay, Escape-to-close, focus trap, and focus
//! restoration. Panel width/padding come from the caller via `class`.

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

/// Rendered only while the modal is open, so its listeners and focus
/// management mount/unmount together with the dialog itself.
#[component]
fn ModalPanel(
    open: RwSignal<bool>,
    #[prop(into)] label: Signal<String>,
    #[prop(into, optional)] class: MaybeProp<String>,
    on_close: Option<Callback<()>>,
    children: Children,
) -> impl IntoView {
    let panel_ref = NodeRef::<html::Div>::new();

    let request_close = move || match on_close {
        Some(cb) => cb.run(()),
        None => open.set(false),
    };

    let escape_handle = leptos::leptos_dom::helpers::window_event_listener(
        leptos::ev::keydown,
        move |e: leptos::web_sys::KeyboardEvent| {
            if e.key() == "Escape" {
                request_close();
            }
        },
    );
    on_cleanup(move || escape_handle.remove());

    // Focus the panel on open; restore focus to the opener on close.
    #[cfg(feature = "hydrate")]
    {
        use leptos::wasm_bindgen::JsCast;

        let previously_focused = StoredValue::new_local(
            leptos::web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.active_element()),
        );
        Effect::new(move |_| {
            if let Some(el) = panel_ref.get() {
                let _ = el.focus();
            }
        });
        on_cleanup(move || {
            if let Some(el) = previously_focused.get_value() {
                if let Some(h) = el.dyn_ref::<leptos::web_sys::HtmlElement>() {
                    let _ = h.focus();
                }
            }
        });
    }

    let on_keydown = move |ev: leptos::web_sys::KeyboardEvent| {
        if ev.key() == "Tab" {
            #[cfg(feature = "hydrate")]
            if let Some(panel) = panel_ref.get_untracked() {
                trap_tab(&ev, &panel);
            }
            #[cfg(not(feature = "hydrate"))]
            let _ = &ev;
        }
    };

    let panel_class = move || {
        format!(
            "relative z-50 max-h-[90vh] w-full overflow-y-auto scroll-area rounded-xl \
             border border-border bg-card text-card-foreground shadow-2xl animate-fade-in \
             focus:outline-none {}",
            class.get().unwrap_or_default()
        )
    };

    view! {
        <div class="fixed inset-0 z-50 flex items-center justify-center p-4">
            <div
                class="fixed inset-0 bg-black/60 backdrop-blur-sm animate-fade-in"
                on:click=move |_| request_close()
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

/// Keep Tab / Shift+Tab cycling within the dialog panel.
#[cfg(feature = "hydrate")]
fn trap_tab(ev: &leptos::web_sys::KeyboardEvent, panel: &leptos::web_sys::HtmlElement) {
    use leptos::wasm_bindgen::JsCast;

    const FOCUSABLE: &str = "a[href], button:not([disabled]), textarea:not([disabled]), \
         input:not([disabled]):not([type='hidden']), select:not([disabled]), \
         [tabindex]:not([tabindex='-1'])";

    let Ok(nodes) = panel.query_selector_all(FOCUSABLE) else {
        return;
    };
    let mut focusable: Vec<leptos::web_sys::HtmlElement> = Vec::new();
    for i in 0..nodes.length() {
        if let Some(el) = nodes
            .item(i)
            .and_then(|n| n.dyn_into::<leptos::web_sys::HtmlElement>().ok())
        {
            // `offset_parent` is None for display:none elements.
            if el.offset_parent().is_some() {
                focusable.push(el);
            }
        }
    }
    let (Some(first), Some(last)) = (focusable.first(), focusable.last()) else {
        ev.prevent_default();
        return;
    };

    let active = leptos::web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .and_then(|a| a.dyn_into::<leptos::web_sys::HtmlElement>().ok());

    if ev.shift_key() {
        if active.as_ref().map(|a| a == first).unwrap_or(true) {
            ev.prevent_default();
            let _ = last.focus();
        }
    } else if active.as_ref().map(|a| a == last).unwrap_or(true) {
        ev.prevent_default();
        let _ = first.focus();
    }
}
