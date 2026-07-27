use leptos::callback::Callable;
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
    fn enter_animation(&self) -> &'static str {
        match self {
            SheetSide::Left => "animate-slide-in-from-left",
            SheetSide::Right => "animate-slide-in-from-right",
            SheetSide::Top => "animate-slide-in-from-top",
            SheetSide::Bottom => "animate-slide-in-from-bottom",
        }
    }

    fn fixed_classes(&self) -> &'static str {
        match self {
            SheetSide::Left => "fixed inset-y-0 left-0 z-50 h-full w-80 max-w-[85vw]",
            SheetSide::Right => "fixed inset-y-0 right-0 z-50 h-full w-80 max-w-[85vw]",
            SheetSide::Top => "fixed inset-x-0 top-0 z-50 w-full",
            SheetSide::Bottom => "fixed inset-x-0 bottom-0 z-50 w-full",
        }
    }
}

#[component]
pub fn Sheet(
    open: RwSignal<bool>,
    #[prop(optional)] side: SheetSide,
    children: ChildrenFn,
) -> impl IntoView {
    let open_clone = open;
    let children = std::sync::Arc::new(children);

    view! {
        <Show when=move || open_clone.get()>
            {
                let children = children.clone();
                view! {
                    <SheetOverlay on_close=Callback::new(move |_| open_clone.set(false)) />
                    <SheetContent side=side on_close=Callback::new(move |_| open_clone.set(false))>
                        {children()}
                    </SheetContent>
                }
            }
        </Show>
    }
}

#[component]
fn SheetOverlay(on_close: Callback<()>) -> impl IntoView {
    view! {
        <div
            class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm animate-fade-in"
            on:click=move |_| on_close.run(())
        />
    }
}

#[component]
fn SheetContent(side: SheetSide, on_close: Callback<()>, children: Children) -> impl IntoView {
    let content_ref = NodeRef::<html::Div>::new();
    let fixed = side.fixed_classes();
    let enter = side.enter_animation();

    let on_close_for_effect = on_close;
    let handle = leptos::leptos_dom::helpers::window_event_listener(
        leptos::ev::keydown,
        move |e: leptos::web_sys::KeyboardEvent| {
            if e.key() == "Escape" {
                on_close_for_effect.run(());
            }
        },
    );
    // Remove the global listener when the sheet unmounts.
    on_cleanup(move || handle.remove());

    view! {
        // Drawer surface. No inner padding — the wrapped content (sidebar)
        // manages its own layout. `overflow-hidden` keeps children clipped.
        <div
            class=format!("{} overflow-hidden bg-background shadow-xl {}", fixed, enter)
            node_ref=content_ref
            role="dialog"
            aria-modal="true"
        >
            {children()}
        </div>
    }
}
