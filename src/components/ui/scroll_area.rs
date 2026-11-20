use leptos::callback::Callable;
use leptos::html;
use leptos::prelude::*;

#[component]
pub fn ScrollArea(
    #[prop(into, optional)] class: MaybeProp<String>,
    #[prop(optional)] node_ref: NodeRef<html::Div>,
    /// Fired on scroll of the scrolling element itself.
    #[prop(optional)]
    on_scroll: Option<Callback<()>>,
    /// Fired when the user scrolls toward the top (wheel up / downward touch
    /// drag). This arrives before the browser's asynchronous `scroll` event,
    /// so callers can react to the intent without waiting for that event.
    #[prop(optional)]
    on_scroll_up: Option<Callback<()>>,
    children: Children,
) -> impl IntoView {
    let base = "scroll-area relative overflow-y-auto overflow-x-hidden";
    let classes = move || {
        let extra = class.get().unwrap_or_default();
        format!("{} {}", base, extra)
    };

    // Vertical position where the current touch gesture began, so a downward
    // drag can be recognized as a scroll-up.
    #[cfg(feature = "hydrate")]
    let touch_start_y = StoredValue::new(0.0_f64);

    let handle_wheel = move |ev: leptos::web_sys::WheelEvent| {
        if let Some(cb) = on_scroll_up {
            #[cfg(feature = "hydrate")]
            if ev.delta_y() < 0.0 {
                cb.run(());
            }
            #[cfg(not(feature = "hydrate"))]
            let _ = (&ev, &cb);
        }
    };

    let handle_touch_start = move |ev: leptos::web_sys::TouchEvent| {
        #[cfg(feature = "hydrate")]
        if let Some(touch) = ev.touches().item(0) {
            touch_start_y.set_value(touch.client_y() as f64);
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = &ev;
    };

    let handle_touch_move = move |ev: leptos::web_sys::TouchEvent| {
        if let Some(cb) = on_scroll_up {
            #[cfg(feature = "hydrate")]
            {
                if let Some(touch) = ev.touches().item(0) {
                    // Finger moving down scrolls toward older messages.
                    if touch.client_y() as f64 - touch_start_y.get_value() > 4.0 {
                        cb.run(());
                    }
                }
            }
            #[cfg(not(feature = "hydrate"))]
            let _ = (&ev, &cb);
        }
    };

    view! {
        <div
            class=classes
            node_ref=node_ref
            on:scroll=move |_| {
                if let Some(cb) = on_scroll {
                    cb.run(());
                }
            }
            on:wheel=handle_wheel
            on:touchstart=handle_touch_start
            on:touchmove=handle_touch_move
        >
            {children()}
        </div>
    }
}
