//! Native confirmation prompt, used to guard against losing unsaved input.

/// Ask the user to confirm an action. Returns `true` on the server (SSR),
/// where there is no user to ask and the markup is thrown away on hydration.
pub fn confirm(message: &str) -> bool {
    #[cfg(feature = "hydrate")]
    {
        leptos::web_sys::window()
            .map(|w| w.confirm_with_message(message).unwrap_or(true))
            .unwrap_or(true)
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = message;
        true
    }
}
