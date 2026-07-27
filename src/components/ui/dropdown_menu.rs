//! Dropdown menu with pointer + keyboard support (arrows, Home/End, Escape)
//! and menu ARIA roles.

use leptos::prelude::*;
use leptos::html;
use leptos::callback::Callable;
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

impl DropdownAlign {
    fn classes(&self) -> &'static str {
        match self {
            DropdownAlign::Start => "left-0",
            DropdownAlign::Center => "left-1/2 -translate-x-1/2",
            DropdownAlign::End => "right-0",
        }
    }
}

#[component]
pub fn DropdownMenu(
    #[prop(optional)] open: RwSignal<bool>,
    children: Children,
) -> impl IntoView {
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

    let base = "inline-flex items-center justify-center";
    let classes = move || {
        let extra = class.get().unwrap_or_default();
        format!("{} {}", base, extra)
    };

    view! {
        <button
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
    let align_class = align.classes();

    let open = ctx.open;
    let trigger_ref = ctx.trigger_ref;

    // Close when clicking outside the menu.
    let content_ref_for_listener = content_ref;
    let mousedown_handle = leptos::leptos_dom::helpers::window_event_listener(
        leptos::ev::mousedown,
        move |e: leptos::web_sys::MouseEvent| {
            // Guard against firing after the component (and its signals) have
            // been disposed — `try_get_untracked` returns None post-dispose.
            let Some(is_open) = open.try_get_untracked() else { return };
            if !is_open {
                return;
            }
            if let Some(el) = content_ref_for_listener.get_untracked() {
                if let Some(target) = e.target() {
                    let node = target.unchecked_ref::<leptos::web_sys::Node>();
                    if !el.contains(Some(node)) {
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
            let Some(is_open) = open.try_get_untracked() else { return };
            if is_open && e.key() == "Escape" {
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

    let base = format!(
        "z-50 min-w-[8rem] overflow-hidden rounded-md border border-border \
         bg-popover p-1 text-popover-foreground shadow-md absolute mt-1 {}",
        align_class
    );

    let show = move || {
        if open.get() {
            let extra = class.get().unwrap_or_default();
            let classes = format!("{} {}", base, extra);
            Some(view! {
                <div class=classes node_ref=content_ref role="menu" on:keydown=on_keydown>
                    {children()}
                </div>
            })
        } else {
            None
        }
    };

    view! {
        {show}
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
    let Ok(nodes) = menu.query_selector_all("[role='menuitem']") else {
        return;
    };
    let mut items: Vec<leptos::web_sys::HtmlElement> = Vec::new();
    for i in 0..nodes.length() {
        if let Some(el) = nodes.item(i).and_then(|n| n.dyn_into::<leptos::web_sys::HtmlElement>().ok()) {
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
                px-2 py-1.5 text-sm outline-none transition-colors \
                hover:bg-accent hover:text-accent-foreground \
                focus:bg-accent focus:text-accent-foreground \
                data-[disabled]:pointer-events-none data-[disabled]:opacity-50";
    let classes = move || {
        let extra = class.get().unwrap_or_default();
        format!("{} {}", base, extra)
    };

    view! {
        <div
            class=classes
            role="menuitem"
            tabindex="0"
            on:click=move |_| select()
            on:keydown=on_keydown
            data-disabled=move || disabled.get().unwrap_or(false).to_string()
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
