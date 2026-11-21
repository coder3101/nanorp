//! UI theme (light / dark / system) persistence in `localStorage`.
//!
//! An inline script injected into `<head>` applies the saved theme before
//! first paint, so the page doesn't flash the wrong theme during hydration.

pub const STORAGE_KEY: &str = "nanorp-theme";
pub const DEFAULT_THEME: &str = "system";

/// Inline `<script>` body that reads the persisted theme and toggles the
/// `dark` class on `<html>` *before* the page renders. Rendered into `<head>`.
pub fn theme_init_script() -> &'static str {
    r#"
    (function () {
        try {
            var t = localStorage.getItem('nanorp-theme') || 'system';
            var mq = window.matchMedia('(prefers-color-scheme: dark)');
            var dark = t === 'dark' || (t === 'system' && mq.matches);
            document.documentElement.classList.toggle('dark', dark);
            document.documentElement.style.colorScheme = dark ? 'dark' : 'light';
        } catch (e) {}
    })();
    "#
}

#[cfg(feature = "hydrate")]
mod client {
    use super::*;

    fn local_storage() -> Option<web_sys::Storage> {
        web_sys::window().and_then(|w| w.local_storage().ok().flatten())
    }

    /// Read the persisted theme, defaulting to "system".
    pub fn get_theme() -> String {
        local_storage()
            .and_then(|s| s.get_item(STORAGE_KEY).ok().flatten())
            .unwrap_or_else(|| DEFAULT_THEME.to_string())
    }

    /// Whether the OS currently prefers a dark color scheme.
    fn prefers_dark() -> bool {
        web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
            .map(|m| m.matches())
            .unwrap_or(false)
    }

    /// Toggle the `dark` class on `<html>` for the given theme value.
    pub fn apply(theme: &str) {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let Some(root) = doc.document_element() else {
            return;
        };
        let dark = match theme {
            "dark" => true,
            "light" => false,
            _ => prefers_dark(),
        };
        let _ = root.class_list().toggle_with_force("dark", dark);
        use leptos::wasm_bindgen::JsCast;
        if let Some(root) = root.dyn_ref::<web_sys::HtmlElement>() {
            let _ = root
                .style()
                .set_property("color-scheme", if dark { "dark" } else { "light" });
        }
    }

    /// Persist the theme and apply it immediately.
    pub fn set_theme(theme: &str) {
        if let Some(storage) = local_storage() {
            let _ = storage.set_item(STORAGE_KEY, theme);
        }
        apply(theme);
    }
}

/// Owns the OS color-scheme listener. Dropping it detaches the DOM callback,
/// including the closure reference held by `MediaQueryList`.
pub struct SystemThemeListener {
    #[cfg(feature = "hydrate")]
    media: Option<leptos::web_sys::MediaQueryList>,
    #[cfg(feature = "hydrate")]
    _callback: Option<leptos::wasm_bindgen::closure::Closure<dyn FnMut()>>,
}

#[cfg(feature = "hydrate")]
impl Drop for SystemThemeListener {
    fn drop(&mut self) {
        if let Some(media) = &self.media {
            media.set_onchange(None);
        }
    }
}

/// Follow OS theme changes while the persisted preference is `system`.
/// Calling this repeatedly is unnecessary; the owner should retain and drop
/// the returned guard with its normal lifecycle.
pub fn listen_system_theme() -> SystemThemeListener {
    apply(&get_theme());

    #[cfg(feature = "hydrate")]
    {
        use leptos::wasm_bindgen::closure::Closure;
        use leptos::wasm_bindgen::JsCast;

        let media = web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten());
        let callback = media.as_ref().map(|_| {
            Closure::<dyn FnMut()>::new(move || {
                if get_theme() == DEFAULT_THEME {
                    apply(DEFAULT_THEME);
                }
            })
        });
        if let (Some(media), Some(callback)) = (&media, &callback) {
            media.set_onchange(Some(callback.as_ref().unchecked_ref()));
        }
        SystemThemeListener {
            media,
            _callback: callback,
        }
    }

    #[cfg(not(feature = "hydrate"))]
    {
        SystemThemeListener {}
    }
}

// Server-side no-op fallbacks so the same API compiles under SSR.
#[cfg(not(feature = "hydrate"))]
mod client {
    pub fn get_theme() -> String {
        super::DEFAULT_THEME.to_string()
    }
    pub fn apply(_theme: &str) {}
    pub fn set_theme(_theme: &str) {}
}

pub use client::{apply, get_theme, set_theme};
