//! Labeled form field with an optional hint and inline validation error.

use leptos::prelude::*;

#[cfg(any(feature = "hydrate", test))]
fn description_ids(for_id: &str, hint: &str, has_error: bool) -> Vec<String> {
    let mut ids = Vec::with_capacity(2);
    if !hint.is_empty() {
        ids.push(format!("{for_id}-hint"));
    }
    if has_error {
        ids.push(format!("{for_id}-error"));
    }
    ids
}

#[component]
pub fn Field(
    label: &'static str,
    /// The `id` of the wrapped control, wired to the label's `for` and to
    /// generated hint/error IDs. No new prop is required on the child control.
    for_id: &'static str,
    #[prop(optional)] hint: &'static str,
    /// Inline validation error, rendered below the control when non-empty.
    #[prop(optional, into)]
    error: MaybeProp<String>,
    children: Children,
) -> impl IntoView {
    let hint_id = format!("{for_id}-hint");
    let error_id = format!("{for_id}-error");

    // `for_id` already provides the child's identity, so ARIA can be wired
    // without changing Field's API or imposing a particular control type.
    #[cfg(feature = "hydrate")]
    {
        let hint_id = hint_id.clone();
        let error_id = error_id.clone();
        Effect::new(move |_| {
            let has_error = error.get().map(|error| !error.is_empty()).unwrap_or(false);
            let Some(control) = leptos::web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id(for_id))
            else {
                return;
            };

            let described_by = control
                .get_attribute("aria-describedby")
                .unwrap_or_default()
                .split_whitespace()
                .filter(|id| *id != hint_id && *id != error_id)
                .map(str::to_string)
                .collect::<Vec<_>>();
            let mut ids = described_by;
            ids.extend(description_ids(for_id, hint, has_error));
            if ids.is_empty() {
                control.remove_attribute("aria-describedby").ok();
            } else {
                control
                    .set_attribute("aria-describedby", &ids.join(" "))
                    .ok();
            }
            if has_error {
                control.set_attribute("aria-invalid", "true").ok();
            } else {
                control.remove_attribute("aria-invalid").ok();
            }
        });
    }

    view! {
        <div class="space-y-1.5">
            <label class="text-sm font-medium" r#for=for_id>{label}</label>
            {(!hint.is_empty()).then(|| view! {
                <p id=hint_id class="text-xs leading-relaxed text-muted-foreground">{hint}</p>
            })}
            {children()}
            {move || error.get().filter(|error| !error.is_empty()).map(|error| {
                let error_id = error_id.clone();
                view! {
                    <p id=error_id class="text-xs font-medium leading-relaxed text-destructive" role="alert">{error}</p>
                }
            })}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::description_ids;

    #[test]
    fn associates_only_visible_descriptions() {
        assert_eq!(
            description_ids("name", "Displayed name", true),
            vec!["name-hint", "name-error"]
        );
        assert!(description_ids("name", "", false).is_empty());
        assert_eq!(description_ids("name", "", true), vec!["name-error"]);
    }
}
