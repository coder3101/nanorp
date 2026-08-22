//! App-scoped tracker of in-flight streaming replies.
//!
//! The tracker lives at the application level (above the Router) and is the
//! single consumer of a reply's text stream. Because it is not owned by the
//! chat page, the stream keeps running when the user navigates away — the
//! reply finishes in the background and the user is toasted on whatever page
//! they're on. The assistant message is persisted server-side regardless of
//! the client connection, so history always reflects the finished reply.

use crate::components::ui::toast::UseToast;
use futures::StreamExt;
use leptos::prelude::*;
use leptos::server_fn::codec::TextStream;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

/// Live streaming state for one in-flight generation, keyed by chat session.
#[derive(Clone)]
pub struct ActiveGeneration {
    /// The accumulated reply text, updated as chunks arrive.
    pub content: RwSignal<String>,
}

/// Global tracker for streaming replies. Provide this as context once, in the
/// app shell, so it survives page navigation. The `toast` context is captured
/// at construction time (inside `ToastProvider`) so completion toasts can be
/// shown from spawned tasks on any page.
#[derive(Clone)]
pub struct GenerationTracker {
    /// Session id -> active generation.
    pub active: RwSignal<HashMap<Uuid, ActiveGeneration>>,
    /// App-level toast handle, used to notify when a reply finishes.
    pub toast: Option<UseToast>,
}

impl GenerationTracker {
    /// Begin consuming a streaming reply for `session_id`. The future runs at
    /// app scope, so it is not cancelled when the chat page unmounts. `content`
    /// is updated live for in-page rendering; when the reply ends the entry is
    /// removed, a completion toast is shown, and `on_finish(session_id)` fires
    /// so the caller can refresh the message history.
    pub fn start(
        &self,
        session_id: Uuid,
        fut: Pin<Box<dyn Future<Output = Result<TextStream, ServerFnError>>>>,
        on_finish: Callback<Uuid>,
    ) {
        let entry = ActiveGeneration {
            content: RwSignal::new(String::new()),
        };
        self.active.update(|m| {
            let _ = m.insert(session_id, entry.clone());
        });
        let content = entry.content;
        let active = self.active;
        let toast = self.toast.clone();

        leptos::task::spawn_local(async move {
            let mut stream = match fut.await {
                Ok(stream) => stream.into_inner(),
                Err(e) => {
                    if let Some(t) = toast.as_ref() {
                        t.error(format!("Generation failed: {e}"));
                    }
                    active.update(|m| {
                        let _ = m.remove(&session_id);
                    });
                    on_finish.run(session_id);
                    return;
                }
            };

            let mut errored = false;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(text) => content.update(|s| s.push_str(&text)),
                    Err(e) => {
                        errored = true;
                        if let Some(t) = toast.as_ref() {
                            t.error(format!("Stream error: {e}"));
                        }
                        break;
                    }
                }
            }

            active.update(|m| {
                let _ = m.remove(&session_id);
            });

            if let Some(t) = toast.as_ref() {
                if errored {
                    t.error("Reply stopped");
                } else {
                    t.success("Reply finished");
                }
            }
            on_finish.run(session_id);
        });
    }
}

/// Retrieve the app-scoped generation tracker from context.
pub fn use_generation_tracker() -> GenerationTracker {
    expect_context::<GenerationTracker>()
}
