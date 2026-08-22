//! App-scoped tracker of in-flight streaming replies.
//!
//! The tracker lives at the application level (above the Router) and is the
//! single consumer of a reply's text stream. Because it is not owned by the
//! chat page, the stream keeps running when the user navigates away — the
//! reply finishes in the background and, if they're on some other page, the
//! user is toasted. (The assistant message is persisted server-side regardless
//! of the client connection, so history always reflects the finished reply.)

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
/// app shell, so it survives page navigation.
#[derive(Clone)]
pub struct GenerationTracker {
    /// Session id -> active generation.
    pub active: RwSignal<HashMap<Uuid, ActiveGeneration>>,
    /// The session currently shown on screen; used to suppress the completion
    /// toast when the user is already watching the reply stream in.
    pub viewing: RwSignal<Option<Uuid>>,
}

impl GenerationTracker {
    /// Begin consuming a streaming reply for `session_id`. The future runs at
    /// app scope, so it is not cancelled when the chat page unmounts. `content`
    /// is updated live for in-page rendering; when the reply ends the entry is
    /// removed and `on_finish(session_id)` fires so the caller can refresh the
    /// message history. `toast` announces completion — but only when the user
    /// has navigated away from the session meanwhile.
    pub fn start(
        &self,
        session_id: Uuid,
        fut: Pin<Box<dyn Future<Output = Result<TextStream, ServerFnError>>>>,
        toast: UseToast,
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
        let viewing = self.viewing;

        leptos::task::spawn_local(async move {
            let mut stream = match fut.await {
                Ok(stream) => stream.into_inner(),
                Err(e) => {
                    toast.error(format!("Generation failed: {e}"));
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
                        toast.error(format!("Stream error: {e}"));
                        break;
                    }
                }
            }

            active.update(|m| {
                let _ = m.remove(&session_id);
            });

            // Only announce when it finished off-screen; while the user is
            // watching, the bubble itself shows the result.
            if errored {
                toast.error("Reply stopped");
            } else if viewing.get_untracked() != Some(session_id) {
                toast.success("Reply finished");
            }
            on_finish.run(session_id);
        });
    }
}

/// Retrieve the app-scoped generation tracker from context.
pub fn use_generation_tracker() -> GenerationTracker {
    expect_context::<GenerationTracker>()
}
