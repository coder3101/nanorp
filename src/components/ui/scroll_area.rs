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
    children: Children,
) -> impl IntoView {
    let base = "scroll-area relative overflow-y-auto overflow-x-hidden";
    let classes = move || {
        let extra = class.get().unwrap_or_default();
        format!("{} {}", base, extra)
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
        >
            {children()}
        </div>
    }
}
