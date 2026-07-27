//! Labeled form field with an optional hint and inline validation error.

use leptos::prelude::*;

#[component]
pub fn Field(
    label: &'static str,
    /// The `id` of the wrapped control, wired to the label's `for`.
    for_id: &'static str,
    #[prop(optional)] hint: &'static str,
    /// Inline validation error, rendered below the control when non-empty.
    #[prop(optional, into)] error: MaybeProp<String>,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="space-y-1.5">
            <label class="text-sm font-medium" r#for=for_id>{label}</label>
            {(!hint.is_empty()).then(|| view! {
                <p class="text-xs text-muted-foreground">{hint}</p>
            })}
            {children()}
            {move || error.get().filter(|e| !e.is_empty()).map(|e| view! {
                <p class="text-xs font-medium text-destructive" role="alert">{e}</p>
            })}
        </div>
    }
}
