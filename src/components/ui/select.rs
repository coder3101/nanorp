use leptos::callback::Callable;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

#[derive(Debug, Clone)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
    pub disabled: bool,
}

impl SelectOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            disabled: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[component]
pub fn Select(
    value: RwSignal<String>,
    #[prop(optional)] on_change: Option<Callback<String>>,
    options: Vec<SelectOption>,
    #[prop(into, optional)] placeholder: MaybeProp<String>,
    #[prop(into, optional)] disabled: MaybeProp<bool>,
    #[prop(into, optional)] class: MaybeProp<String>,
    /// `id` for associating an external `<label for=...>`.
    #[prop(optional)]
    id: &'static str,
) -> impl IntoView {
    let handle_change = move |ev: leptos::web_sys::Event| {
        let target = ev.target().unwrap();
        let input = target.unchecked_into::<leptos::web_sys::HtmlSelectElement>();
        let new_val = input.value();
        value.set(new_val.clone());
        if let Some(cb) = on_change {
            cb.run(new_val);
        }
    };

    let base = "flex h-10 w-full items-center justify-between whitespace-nowrap \
                rounded-md border border-input bg-background px-3 py-2 text-sm \
                shadow-sm ring-offset-background placeholder:text-muted-foreground \
                focus:outline-none focus:ring-1 focus:ring-ring \
                disabled:cursor-not-allowed disabled:opacity-50 \
                [&>option]:bg-background";
    let classes = move || {
        let extra = class.get().unwrap_or_default();
        format!("{} {}", base, extra)
    };

    let placeholder_text = placeholder.get().unwrap_or_default();

    view! {
        <select
            id=id
            class=classes
            prop:value=move || value.get()
            on:change=handle_change
            disabled=move || disabled.get().unwrap_or(false)
        >
            {if !placeholder_text.is_empty() {
                let ph = placeholder_text.clone();
                Some(view! {
                    <option value="" disabled selected={value.get().is_empty()}>{ph}</option>
                })
            } else {
                None
            }}
            {options.into_iter().map(|opt| {
                let opt_val = opt.value.clone();
                let is_selected = value.get() == opt_val;
                view! {
                    <option
                        value=opt_val.clone()
                        disabled=opt.disabled
                        selected=is_selected
                    >
                        {opt.label.clone()}
                    </option>
                }
            }).collect::<Vec<_>>()}
        </select>
    }
}
