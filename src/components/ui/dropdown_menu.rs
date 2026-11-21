//! Dropdown menu with pointer + keyboard support (arrows, Home/End, Escape)
//! and menu ARIA roles.

use leptos::callback::Callable;
use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

#[derive(Clone)]
struct DropdownMenuContext {
    open: RwSignal<bool>,
    trigger_ref: NodeRef<html::Button>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum DropdownAlign {
    #[default]
    Start,
    Center,
    End,
}

#[component]
pub fn DropdownMenu(#[prop(optional)] open: RwSignal<bool>, children: Children) -> impl IntoView {
    provide_context(DropdownMenuContext {
        open,
        trigger_ref: NodeRef::new(),
    });
    view! {
        <div class="relative inline-block">
            {children()}
        </div>
    }
}

#[component]
pub fn DropdownMenuTrigger(
    #[prop(into, optional)] class: MaybeProp<String>,
    children: Children,
) -> impl IntoView {
    let ctx = expect_context::<DropdownMenuContext>();

    let open = ctx.open;
    let on_click = move |_| {
        open.update(|v| *v = !*v);
    };
    // ArrowDown opens the menu and moves focus to the first item.
    let on_keydown = move |ev: leptos::web_sys::KeyboardEvent| {
        if ev.key() == "ArrowDown" && !open.get_untracked() {
            ev.prevent_default();
            open.set(true);
        }
    };

    let base = "inline-flex items-center justify-center pointer-coarse:min-h-11";
    let classes = move || {
        let extra = class.get().unwrap_or_default();
        format!("{} {}", base, extra)
    };

    view! {
        <button
            type="button"
            class=classes
            node_ref=ctx.trigger_ref
            aria-haspopup="menu"
            aria-expanded=move || open.get().to_string()
            on:click=on_click
            on:keydown=on_keydown
        >
            {children()}
        </button>
    }
}

#[component]
pub fn DropdownMenuContent(
    #[prop(into, optional)] class: MaybeProp<String>,
    #[prop(optional)] align: DropdownAlign,
    children: ChildrenFn,
) -> impl IntoView {
    let ctx = expect_context::<DropdownMenuContext>();
    let content_ref = NodeRef::<html::Div>::new();
    #[cfg(not(feature = "hydrate"))]
    let _ = align;

    let open = ctx.open;
    let trigger_ref = ctx.trigger_ref;

    // Close when clicking outside the menu.
    let content_ref_for_listener = content_ref;
    let mousedown_handle = leptos::leptos_dom::helpers::window_event_listener(
        leptos::ev::mousedown,
        move |e: leptos::web_sys::MouseEvent| {
            // Guard against firing after the component (and its signals) have
            // been disposed — `try_get_untracked` returns None post-dispose.
            let Some(is_open) = open.try_get_untracked() else {
                return;
            };
            if !is_open {
                return;
            }
            if let Some(el) = content_ref_for_listener.get_untracked() {
                if let Some(target) = e.target() {
                    let node = target.unchecked_ref::<leptos::web_sys::Node>();
                    let in_content = el.contains(Some(node));
                    let in_trigger = trigger_ref
                        .get_untracked()
                        .map(|trigger| trigger.contains(Some(node)))
                        .unwrap_or(false);
                    // The trigger's click toggles the menu itself. Closing on
                    // its mousedown would make that click immediately reopen.
                    if !in_content && !in_trigger {
                        open.set(false);
                    }
                }
            }
        },
    );
    on_cleanup(move || mousedown_handle.remove());

    // Escape closes the menu and returns focus to the trigger.
    let keydown_handle = leptos::leptos_dom::helpers::window_event_listener(
        leptos::ev::keydown,
        move |e: leptos::web_sys::KeyboardEvent| {
            let Some(is_open) = open.try_get_untracked() else {
                return;
            };
            if is_open && e.key() == "Escape" && !e.default_prevented() {
                e.prevent_default();
                e.stop_immediate_propagation();
                open.set(false);
                if let Some(btn) = trigger_ref.get_untracked() {
                    let _ = btn.focus();
                }
            }
        },
    );
    on_cleanup(move || keydown_handle.remove());

    // Arrow-key navigation between menu items while focus is in the menu.
    let on_keydown = move |ev: leptos::web_sys::KeyboardEvent| {
        let key = ev.key();
        if key == "Escape" {
            ev.prevent_default();
            ev.stop_propagation();
            open.set(false);
            if let Some(trigger) = trigger_ref.get_untracked() {
                let _ = trigger.focus();
            }
            return;
        }
        if key == "Tab" {
            // Menus do not trap focus; let the dialog/browser continue the
            // normal Tab sequence instead of leaving an orphaned popup open.
            open.set(false);
            return;
        }
        let step = match key.as_str() {
            "ArrowDown" => ItemFocus::Next,
            "ArrowUp" => ItemFocus::Prev,
            "Home" => ItemFocus::First,
            "End" => ItemFocus::Last,
            _ => return,
        };
        ev.prevent_default();
        #[cfg(feature = "hydrate")]
        if let Some(el) = content_ref.get_untracked() {
            focus_menu_item(&el, step);
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = step;
    };

    // Move focus to the first item when the menu opens via keyboard (or click).
    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        leptos::leptos_dom::helpers::set_timeout(
            move || {
                if let Some(el) = content_ref.get_untracked() {
                    focus_menu_item(&el, ItemFocus::First);
                }
            },
            std::time::Duration::ZERO,
        );
    });

    // Position in viewport coordinates so menus near an edge never extend
    // beyond it. Re-run after layout, scrolling, and viewport resizes.
    #[cfg(feature = "hydrate")]
    {
        let reposition = move || {
            if open.try_get_untracked().unwrap_or(false) {
                if let (Some(trigger), Some(menu)) =
                    (trigger_ref.get_untracked(), content_ref.get_untracked())
                {
                    position_menu(&trigger, &menu, align);
                }
            }
        };

        let reposition_on_open = reposition;
        Effect::new(move |_| {
            if !open.get() {
                return;
            }
            leptos::leptos_dom::helpers::set_timeout(reposition_on_open, std::time::Duration::ZERO);
        });

        let resize_handle =
            leptos::leptos_dom::helpers::window_event_listener(leptos::ev::resize, move |_| {
                reposition()
            });
        on_cleanup(move || resize_handle.remove());

        let scroll_handle =
            leptos::leptos_dom::helpers::window_event_listener(leptos::ev::scroll, move |_| {
                if open.try_get_untracked().unwrap_or(false) {
                    if let (Some(trigger), Some(menu)) =
                        (trigger_ref.get_untracked(), content_ref.get_untracked())
                    {
                        position_menu(&trigger, &menu, align);
                    }
                }
            });
        on_cleanup(move || scroll_handle.remove());
    }

    let base = "fixed left-0 top-0 z-50 min-w-[8rem] w-max max-w-[calc(100vw-1rem)] \
                overflow-y-auto overflow-x-hidden overscroll-contain scroll-area rounded-md \
                border border-border bg-popover p-1 text-popover-foreground shadow-md";

    // Portal the panel to <body>. A `position: fixed` element is positioned
    // relative to the nearest ancestor with a `transform`, `filter`, or
    // `backdrop-filter` (e.g. the chat header's backdrop-blur), not the
    // viewport. Rendering outside that subtree lets the viewport-coordinate
    // positioner place the menu correctly instead of drifting and clipping.
    let children = std::sync::Arc::new(children);
    let classes = move || format!("{} {}", base, class.get().unwrap_or_default());
    let show = move || {
        if open.get() {
            let children = children.clone();
            Some(view! {
                <Portal>
                    <div class=classes node_ref=content_ref role="menu" on:keydown=on_keydown>
                        {children()}
                    </div>
                </Portal>
            })
        } else {
            None
        }
    };

    view! {
        {show}
    }
}

#[cfg(feature = "hydrate")]
#[derive(Clone, Copy)]
struct Bounds {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

#[cfg(feature = "hydrate")]
impl Bounds {
    fn width(self) -> f64 {
        self.right - self.left
    }

    fn height(self) -> f64 {
        self.bottom - self.top
    }
}

#[cfg(feature = "hydrate")]
fn element_bounds(element: &leptos::web_sys::HtmlElement) -> Option<Bounds> {
    use leptos::wasm_bindgen::{JsCast, JsValue};

    // Using the standard DOM method through reflection keeps this primitive
    // independent of additional web-sys feature flags.
    let function = js_sys::Reflect::get(
        element.as_ref(),
        &JsValue::from_str("getBoundingClientRect"),
    )
    .ok()?
    .dyn_into::<js_sys::Function>()
    .ok()?;
    let rect = function.call0(element.as_ref()).ok()?;
    let number = |name: &str| {
        js_sys::Reflect::get(&rect, &JsValue::from_str(name))
            .ok()
            .and_then(|value| value.as_f64())
    };
    Some(Bounds {
        left: number("left")?,
        right: number("right")?,
        top: number("top")?,
        bottom: number("bottom")?,
    })
}

#[cfg(feature = "hydrate")]
fn position_menu(
    trigger: &leptos::web_sys::HtmlElement,
    menu: &leptos::web_sys::HtmlElement,
    align: DropdownAlign,
) {
    const MARGIN: f64 = 8.0;
    const GAP: f64 = 4.0;

    let (Some(trigger_rect), Some(menu_rect), Some(window)) = (
        element_bounds(trigger),
        element_bounds(menu),
        leptos::web_sys::window(),
    ) else {
        return;
    };
    let Some(viewport_width) = window.inner_width().ok().and_then(|value| value.as_f64()) else {
        return;
    };
    let Some(viewport_height) = window.inner_height().ok().and_then(|value| value.as_f64()) else {
        return;
    };

    let desired_left = match align {
        DropdownAlign::Start => trigger_rect.left,
        DropdownAlign::Center => {
            trigger_rect.left + trigger_rect.width() / 2.0 - menu_rect.width() / 2.0
        }
        DropdownAlign::End => trigger_rect.right - menu_rect.width(),
    };
    let max_left = (viewport_width - menu_rect.width() - MARGIN).max(MARGIN);
    let left = desired_left.clamp(MARGIN, max_left);

    let below_space = (viewport_height - trigger_rect.bottom - GAP - MARGIN).max(0.0);
    let above_space = (trigger_rect.top - GAP - MARGIN).max(0.0);
    let place_above = menu_rect.height() > below_space && above_space > below_space;
    let (mut top, available_height) = if place_above {
        (trigger_rect.top - GAP - menu_rect.height(), above_space)
    } else {
        (trigger_rect.bottom + GAP, below_space)
    };
    top = top.max(MARGIN);

    let style = menu.style();
    let _ = style.set_property("left", &format!("{left}px"));
    let _ = style.set_property("top", &format!("{top}px"));
    let _ = style.set_property("right", "auto");
    let _ = style.set_property("bottom", "auto");
    let _ = style.set_property("transform", "none");
    let _ = style.set_property("margin-top", "0");
    if menu_rect.height() > available_height {
        let max_height = available_height.max(1.0);
        let _ = style.set_property("max-height", &format!("{max_height}px"));
    } else {
        // Restore a caller-provided max-height class when there is room again.
        let _ = style.remove_property("max-height");
    }
}

#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
#[derive(Clone, Copy)]
enum ItemFocus {
    First,
    Last,
    Next,
    Prev,
}

/// Move focus between `[role="menuitem"]` descendants of the menu.
#[cfg(feature = "hydrate")]
fn focus_menu_item(menu: &leptos::web_sys::HtmlElement, step: ItemFocus) {
    let Ok(nodes) = menu.query_selector_all("[role='menuitem']:not([data-disabled='true'])") else {
        return;
    };
    let mut items: Vec<leptos::web_sys::HtmlElement> = Vec::new();
    for i in 0..nodes.length() {
        if let Some(el) = nodes
            .item(i)
            .and_then(|n| n.dyn_into::<leptos::web_sys::HtmlElement>().ok())
        {
            items.push(el);
        }
    }
    if items.is_empty() {
        return;
    }

    let active = leptos::web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .and_then(|a| a.dyn_into::<leptos::web_sys::HtmlElement>().ok());
    let current = active.and_then(|a| items.iter().position(|el| *el == a));

    let target = match (step, current) {
        (ItemFocus::First, _) => 0,
        (ItemFocus::Last, _) => items.len() - 1,
        (ItemFocus::Next, Some(i)) => (i + 1) % items.len(),
        (ItemFocus::Prev, Some(i)) => (i + items.len() - 1) % items.len(),
        (ItemFocus::Next, None) => 0,
        (ItemFocus::Prev, None) => items.len() - 1,
    };
    let _ = items[target].focus();
}

#[component]
pub fn DropdownMenuItem(
    #[prop(optional)] on_select: Option<Callback<()>>,
    #[prop(into, optional)] disabled: MaybeProp<bool>,
    #[prop(into, optional)] class: MaybeProp<String>,
    children: Children,
) -> impl IntoView {
    let ctx = expect_context::<DropdownMenuContext>();

    let open = ctx.open;
    let select = move || {
        if !disabled.get().unwrap_or(false) {
            if let Some(cb) = on_select {
                cb.run(());
            }
            open.set(false);
            if let Some(trigger) = ctx.trigger_ref.get_untracked() {
                let _ = trigger.focus();
            }
        }
    };
    let select_for_key = select;
    let on_keydown = move |ev: leptos::web_sys::KeyboardEvent| {
        let key = ev.key();
        if key == "Enter" || key == " " {
            ev.prevent_default();
            select_for_key();
        }
    };

    let base = "relative flex cursor-pointer select-none items-center rounded-sm \
                px-2 py-1.5 text-sm outline-none transition-colors pointer-coarse:min-h-11 \
                hover:bg-accent hover:text-accent-foreground \
                focus:bg-accent focus:text-accent-foreground \
                data-[disabled=true]:pointer-events-none data-[disabled=true]:opacity-50";
    let classes = move || {
        let extra = class.get().unwrap_or_default();
        format!("{} {}", base, extra)
    };

    view! {
        <div
            class=classes
            role="menuitem"
            tabindex=move || if disabled.get().unwrap_or(false) { "-1" } else { "0" }
            aria-disabled=move || disabled.get().unwrap_or(false).then_some("true")
            on:click=move |_| select()
            on:keydown=on_keydown
            // Omitted entirely when enabled: `data-disabled="false"` would still
            // match an attribute-presence selector and grey the item out.
            data-disabled=move || disabled.get().unwrap_or(false).then_some("true")
        >
            {children()}
        </div>
    }
}

#[component]
pub fn DropdownMenuSeparator() -> impl IntoView {
    view! {
        <div class="-mx-1 my-1 h-px bg-border" role="separator" />
    }
}

#[component]
pub fn DropdownMenuLabel(
    #[prop(into, optional)] class: MaybeProp<String>,
    children: Children,
) -> impl IntoView {
    let base = "px-2 py-1.5 text-xs font-semibold text-muted-foreground";
    let classes = move || {
        let extra = class.get().unwrap_or_default();
        format!("{} {}", base, extra)
    };

    view! {
        <div class=classes>
            {children()}
        </div>
    }
}
