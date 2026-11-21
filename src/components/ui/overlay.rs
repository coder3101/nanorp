//! Shared focus and dismissal infrastructure for modal overlays.

#[derive(Clone, Copy, Debug)]
pub(crate) struct OverlayGuard {
    #[allow(dead_code)] // SSR has no overlay runtime to query by id.
    id: u64,
}

#[cfg(any(feature = "hydrate", test))]
#[derive(Debug, Default)]
struct OverlayStack {
    next_id: u64,
    entries: Vec<u64>,
}

#[cfg(any(feature = "hydrate", test))]
impl OverlayStack {
    fn push(&mut self) -> u64 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        self.entries.push(id);
        id
    }

    fn is_topmost(&self, id: u64) -> bool {
        self.entries.last() == Some(&id)
    }

    /// Removes an overlay even when an ancestor is dismissed before its child.
    /// Returns whether it was topmost and whether the stack is now empty.
    fn remove(&mut self, id: u64) -> (bool, bool) {
        let was_topmost = self.is_topmost(id);
        if let Some(index) = self.entries.iter().rposition(|entry| *entry == id) {
            self.entries.remove(index);
        }
        (was_topmost, self.entries.is_empty())
    }
}

#[cfg(feature = "hydrate")]
#[derive(Default)]
struct Runtime {
    stack: OverlayStack,
    body_lock: Option<BodyLock>,
    inert: Vec<InertLock>,
}

#[cfg(feature = "hydrate")]
struct InertLock {
    element: leptos::web_sys::Element,
    overlays: Vec<u64>,
    was_inert: bool,
}

#[cfg(feature = "hydrate")]
impl Runtime {
    fn activate_background(&mut self, id: u64, panel: &leptos::web_sys::HtmlElement) {
        let mut current: leptos::web_sys::Element = panel.clone().into();
        let mut background = Vec::new();

        // Mark siblings inert along the ancestor chain. The path containing
        // this dialog stays active, including its own backdrop. Nested overlays
        // therefore inert the older dialog without disabling their ancestors.
        while let Some(parent) = current.parent_element() {
            let mut sibling = parent.first_element_child();
            while let Some(element) = sibling {
                sibling = element.next_element_sibling();
                if element != current && !element.has_attribute("data-overlay-backdrop") {
                    background.push(element);
                }
            }
            current = parent;
        }

        for element in background {
            if let Some(lock) = self.inert.iter_mut().find(|lock| lock.element == element) {
                if !lock.overlays.contains(&id) {
                    lock.overlays.push(id);
                }
            } else {
                let was_inert = element.has_attribute("inert");
                element.set_attribute("inert", "").ok();
                self.inert.push(InertLock {
                    element,
                    overlays: vec![id],
                    was_inert,
                });
            }
        }
    }

    fn release_background(&mut self, id: u64) {
        self.inert.retain_mut(|lock| {
            lock.overlays.retain(|overlay| *overlay != id);
            if lock.overlays.is_empty() {
                if !lock.was_inert {
                    lock.element.remove_attribute("inert").ok();
                }
                false
            } else {
                true
            }
        });
    }
}

#[cfg(feature = "hydrate")]
struct BodyLock {
    overflow: String,
    padding_right: String,
}

#[cfg(feature = "hydrate")]
impl BodyLock {
    fn acquire() -> Option<Self> {
        let window = leptos::web_sys::window()?;
        let document = window.document()?;
        let body = document.body()?;
        let style = body.style();
        let overflow = style.get_property_value("overflow").unwrap_or_default();
        let padding_right = style
            .get_property_value("padding-right")
            .unwrap_or_default();

        let scrollbar_width = window
            .inner_width()
            .ok()
            .and_then(|width| width.as_f64())
            .and_then(|width| {
                document
                    .document_element()
                    .map(|root| (width - f64::from(root.client_width())).max(0.0))
            })
            .unwrap_or(0.0);
        if scrollbar_width > 0.0 {
            let current_padding = padding_right
                .trim_end_matches("px")
                .trim()
                .parse::<f64>()
                .unwrap_or(0.0);
            let _ = style.set_property(
                "padding-right",
                &format!("{}px", current_padding + scrollbar_width),
            );
        }
        let _ = style.set_property("overflow", "hidden");

        Some(Self {
            overflow,
            padding_right,
        })
    }

    fn restore(self) {
        let Some(body) = leptos::web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.body())
        else {
            return;
        };
        let style = body.style();
        let _ = style.set_property("overflow", &self.overflow);
        let _ = style.set_property("padding-right", &self.padding_right);
    }
}

#[cfg(feature = "hydrate")]
thread_local! {
    static RUNTIME: std::cell::RefCell<Runtime> = std::cell::RefCell::new(Runtime::default());
}

impl OverlayGuard {
    pub(crate) fn new() -> Self {
        #[cfg(feature = "hydrate")]
        {
            let id = RUNTIME.with(|runtime| {
                let mut runtime = runtime.borrow_mut();
                let id = runtime.stack.push();
                if runtime.stack.entries.len() == 1 {
                    runtime.body_lock = BodyLock::acquire();
                }
                id
            });
            Self { id }
        }

        #[cfg(not(feature = "hydrate"))]
        {
            Self { id: 0 }
        }
    }

    #[cfg(feature = "hydrate")]
    pub(crate) fn activate_background(self, panel: &leptos::web_sys::HtmlElement) {
        RUNTIME.with(|runtime| {
            runtime.borrow_mut().activate_background(self.id, panel);
        });
    }

    pub(crate) fn is_topmost(self) -> bool {
        #[cfg(feature = "hydrate")]
        {
            RUNTIME.with(|runtime| runtime.borrow().stack.is_topmost(self.id))
        }
        #[cfg(not(feature = "hydrate"))]
        {
            true
        }
    }

    /// Returns true only when this was the topmost overlay, which is the only
    /// case in which closing an overlay should restore focus.
    pub(crate) fn close(self) -> bool {
        #[cfg(feature = "hydrate")]
        {
            RUNTIME.with(|runtime| {
                let mut runtime = runtime.borrow_mut();
                let (was_topmost, is_empty) = runtime.stack.remove(self.id);
                runtime.release_background(self.id);
                if is_empty {
                    if let Some(body_lock) = runtime.body_lock.take() {
                        body_lock.restore();
                    }
                }
                was_topmost
            })
        }
        #[cfg(not(feature = "hydrate"))]
        {
            true
        }
    }
}

#[cfg(feature = "hydrate")]
const FOCUSABLE: &str = "a[href], button:not([disabled]), textarea:not([disabled]), \
     input:not([disabled]):not([type='hidden']), select:not([disabled]), \
     [contenteditable='true'], [tabindex]:not([tabindex='-1'])";

#[cfg(feature = "hydrate")]
fn focusable(panel: &leptos::web_sys::HtmlElement) -> Vec<leptos::web_sys::HtmlElement> {
    use leptos::wasm_bindgen::JsCast;

    let Ok(nodes) = panel.query_selector_all(FOCUSABLE) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for index in 0..nodes.length() {
        let Some(element) = nodes
            .item(index)
            .and_then(|node| node.dyn_into::<leptos::web_sys::HtmlElement>().ok())
        else {
            continue;
        };
        let hidden = element
            .closest("[hidden], [inert], [aria-hidden='true']")
            .ok()
            .flatten()
            .is_some();
        if !hidden && element.offset_parent().is_some() {
            result.push(element);
        }
    }
    result
}

/// Prefer an explicitly requested autofocus target, then the first usable
/// control, and finally the dialog itself so focus never remains behind it.
#[cfg(feature = "hydrate")]
pub(crate) fn focus_initial(panel: &leptos::web_sys::HtmlElement) {
    use leptos::wasm_bindgen::JsCast;

    let autofocus = panel
        .query_selector("[autofocus]")
        .ok()
        .flatten()
        .and_then(|element| element.dyn_into::<leptos::web_sys::HtmlElement>().ok());
    if let Some(element) = autofocus {
        let _ = element.focus();
    } else if let Some(element) = focusable(panel).first() {
        let _ = element.focus();
    } else {
        let _ = panel.focus();
    }
}

/// Keep Tab / Shift+Tab cycling within the topmost dialog panel.
#[cfg(feature = "hydrate")]
pub(crate) fn trap_tab(
    event: &leptos::web_sys::KeyboardEvent,
    panel: &leptos::web_sys::HtmlElement,
) {
    use leptos::wasm_bindgen::JsCast;

    let focusable = focusable(panel);
    let (Some(first), Some(last)) = (focusable.first(), focusable.last()) else {
        event.prevent_default();
        let _ = panel.focus();
        return;
    };
    let active = leptos::web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element())
        .and_then(|active| active.dyn_into::<leptos::web_sys::HtmlElement>().ok());

    if event.shift_key() {
        if active
            .as_ref()
            .map(|active| active == first)
            .unwrap_or(true)
        {
            event.prevent_default();
            let _ = last.focus();
        }
    } else if active.as_ref().map(|active| active == last).unwrap_or(true) {
        event.prevent_default();
        let _ = first.focus();
    }
}

#[cfg(test)]
mod tests {
    use super::OverlayStack;

    #[test]
    fn overlay_stack_uses_lifo_order() {
        let mut stack = OverlayStack::default();
        let parent = stack.push();
        let child = stack.push();

        assert!(stack.is_topmost(child));
        assert!(!stack.is_topmost(parent));
        assert_eq!(stack.remove(child), (true, false));
        assert!(stack.is_topmost(parent));
        assert_eq!(stack.remove(parent), (true, true));
    }

    #[test]
    fn removing_parent_does_not_disturb_nested_overlay() {
        let mut stack = OverlayStack::default();
        let parent = stack.push();
        let child = stack.push();

        assert_eq!(stack.remove(parent), (false, false));
        assert!(stack.is_topmost(child));
        assert_eq!(stack.remove(child), (true, true));
    }
}
