use leptos::prelude::*;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum ToastVariant {
    Default,
    Success,
    Error,
    Warning,
}

impl ToastVariant {
    fn classes(&self) -> &'static str {
        match self {
            ToastVariant::Default => "border-border bg-background text-foreground",
            ToastVariant::Success => "border-green-500/30 bg-green-50 text-green-900 dark:bg-green-950 dark:text-green-50",
            ToastVariant::Error => "border-destructive/30 bg-red-50 text-red-900 dark:bg-red-950 dark:text-red-50",
            ToastVariant::Warning => "border-yellow-500/30 bg-yellow-50 text-yellow-900 dark:bg-yellow-950 dark:text-yellow-50",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToastData {
    pub id: u64,
    pub title: String,
    pub description: Option<String>,
    pub variant: ToastVariant,
    pub duration_ms: u64,
}

#[derive(Clone)]
pub struct ToastContext {
    pub toasts: RwSignal<Vec<ToastData>>,
    next_id: RwSignal<u64>,
}

impl ToastContext {
    fn add(
        &self,
        title: String,
        description: Option<String>,
        variant: ToastVariant,
        duration_ms: u64,
    ) -> u64 {
        let id = self.next_id.with(|n| *n);
        self.next_id.update(|n| *n += 1);
        let toast = ToastData {
            id,
            title,
            description,
            variant,
            duration_ms,
        };
        self.toasts.update(|t| t.push(toast));
        id
    }

    fn dismiss(&self, id: u64) {
        self.toasts.update(|t| t.retain(|toast| toast.id != id));
    }
}

#[derive(Clone)]
pub struct UseToast {
    ctx: ToastContext,
}

impl UseToast {
    pub fn show(&self, title: impl Into<String>, variant: ToastVariant) {
        self.ctx.add(title.into(), None, variant, 5000);
    }

    pub fn success(&self, title: impl Into<String>) {
        self.show(title, ToastVariant::Success);
    }

    pub fn error(&self, title: impl Into<String>) {
        self.show(title, ToastVariant::Error);
    }

    pub fn warning(&self, title: impl Into<String>) {
        self.show(title, ToastVariant::Warning);
    }

    pub fn custom(
        &self,
        title: impl Into<String>,
        description: Option<String>,
        variant: ToastVariant,
        duration_ms: u64,
    ) -> u64 {
        self.ctx
            .add(title.into(), description, variant, duration_ms)
    }

    pub fn dismiss(&self, id: u64) {
        self.ctx.dismiss(id);
    }
}

pub fn use_toast() -> UseToast {
    let ctx = expect_context::<ToastContext>();
    UseToast { ctx }
}

#[component]
pub fn ToastProvider(children: Children) -> impl IntoView {
    let ctx = ToastContext {
        toasts: RwSignal::new(Vec::new()),
        next_id: RwSignal::new(1),
    };
    provide_context(ctx);
    children()
}

#[component]
pub fn Toaster() -> impl IntoView {
    let ctx = expect_context::<ToastContext>();

    view! {
        <div
            class="fixed top-4 right-4 z-[100] flex flex-col gap-2 w-full max-w-[420px] pointer-events-none"
            aria-live="polite"
        >
            <For
                each=move || ctx.toasts.get()
                key=|toast| toast.id
                let:toast
            >
                <ToastItem toast=toast.clone() />
            </For>
        </div>
    }
}

#[component]
fn ToastItem(toast: ToastData) -> impl IntoView {
    let ctx = expect_context::<ToastContext>();
    let id = toast.id;
    let duration = toast.duration_ms;

    // Schedule auto-dismiss once, after the item is mounted on the client.
    // `set_timeout` is a no-op on the server, and the returned handle is
    // cancelled if the toast is removed (unmounted) before it fires.
    if duration > 0 {
        let ctx_clone = ctx.clone();
        let handle = leptos::leptos_dom::helpers::set_timeout_with_handle(
            move || ctx_clone.dismiss(id),
            Duration::from_millis(duration),
        )
        .ok();
        on_cleanup(move || {
            if let Some(handle) = handle {
                handle.clear();
            }
        });
    }

    let variant_classes = toast.variant.classes().to_string();

    view! {
        <div
            class=format!(
                "group pointer-events-auto relative flex w-full items-center justify-between \
                 space-x-2 overflow-hidden rounded-md border p-4 pr-8 shadow-lg \
                 transition-all animate-slide-in-from-top {}",
                variant_classes
            )
        >
            <div class="flex flex-col gap-1">
                <p class="text-sm font-semibold">{toast.title.clone()}</p>
                {move || toast.description.clone().map(|desc| view! {
                    <p class="text-xs opacity-90">{desc}</p>
                })}
            </div>
            <button
                class="absolute right-2 top-2 rounded-md p-1 opacity-0 transition-opacity \
                       group-hover:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100 \
                       hover:bg-black/10 dark:hover:bg-white/10"
                aria-label="Dismiss notification"
                on:click=move |_| ctx.dismiss(id)
            >
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24"
                     fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
                     stroke-linejoin="round">
                    <path d="M18 6 6 18"/>
                    <path d="m6 6 12 12"/>
                </svg>
            </button>
        </div>
    }
}
