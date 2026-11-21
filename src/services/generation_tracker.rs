//! App-scoped state for in-flight replies and per-conversation drafts.
//!
//! The tracker lives above the router. Generation tasks therefore survive page
//! navigation, while the visible chat controller remains scoped to one session.

use crate::components::ui::toast::UseToast;
use crate::models::message::{
    GenerationOutcome, GenerationReceipt, ImageUpload, Message, MessageRole,
    GENERATION_TERMINAL_PREFIX,
};
use futures::StreamExt;
use leptos::prelude::*;
use leptos::server_fn::codec::TextStream;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationPhase {
    Starting,
    Streaming,
    Stopping,
}

#[derive(Clone)]
pub struct ActiveGeneration {
    pub content: RwSignal<String>,
    pub phase: RwSignal<GenerationPhase>,
    pub replacing_response: bool,
}

#[derive(Clone)]
pub struct FinishedGeneration {
    pub receipt: GenerationReceipt,
    /// Retained for diagnostics/recovery if refreshing persisted history fails.
    pub content: String,
}

#[derive(Clone, Default)]
pub struct ChatDraft {
    pub content: String,
    pub images: Vec<ImageUpload>,
}

#[derive(Clone)]
pub struct PendingSubmission {
    pub content: String,
    pub image_count: usize,
    pub previous_user_id: Option<Uuid>,
}

pub(crate) fn submission_was_persisted(
    submission: &PendingSubmission,
    messages: &[Message],
) -> bool {
    messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::User)
        .is_some_and(|message| {
            Some(message.id) != submission.previous_user_id
                && message.content == submission.content
                && message.attachments.len() == submission.image_count
        })
}

/// Global generation and draft state. Drafts deliberately live in memory only:
/// they survive SPA navigation but are not written to another browser store.
#[derive(Clone)]
pub struct GenerationTracker {
    pub active: RwSignal<HashMap<Uuid, ActiveGeneration>>,
    pub finished: RwSignal<HashMap<Uuid, FinishedGeneration>>,
    pub drafts: RwSignal<HashMap<Uuid, ChatDraft>>,
    pub submissions: RwSignal<HashMap<Uuid, PendingSubmission>>,
    pub viewing: RwSignal<Option<Uuid>>,
    /// The app-scoped reactive owner. A live generation's `content`/`phase`
    /// signals are created here, not under the page that started it, so they
    /// keep streaming (and remain readable) after that page is disposed on
    /// navigation and re-rendered when the user returns.
    owner: Owner,
}

impl GenerationTracker {
    /// Construct the tracker, capturing the current reactive owner as the
    /// app-scoped owner for all live-generation signals. Call this from the
    /// app shell (above the router) so generations outlive page navigation.
    pub fn new(viewing: RwSignal<Option<Uuid>>) -> Self {
        Self {
            active: RwSignal::new(HashMap::new()),
            finished: RwSignal::new(HashMap::new()),
            drafts: RwSignal::new(HashMap::new()),
            submissions: RwSignal::new(HashMap::new()),
            viewing,
            owner: Owner::current().unwrap_or_default(),
        }
    }

    pub fn start(
        &self,
        session_id: Uuid,
        replacing_response: bool,
        fut: Pin<Box<dyn Future<Output = Result<TextStream, ServerFnError>>>>,
        toast: UseToast,
        on_accepted: Callback<()>,
        on_finish: Callback<Uuid>,
    ) {
        // Starting another generation replaces both the visible live reply and
        // the previous terminal notification for this session.
        self.finished.update(|m| {
            m.remove(&session_id);
        });
        // Create the live signals under the app-scoped owner. Otherwise they
        // would belong to the page that called `start`, and navigating away
        // would dispose them mid-stream — the generation would keep running on
        // the server but the returning page would read dead signals and render
        // an empty, frozen bubble.
        let entry = self.owner.with(|| ActiveGeneration {
            content: RwSignal::new(String::new()),
            phase: RwSignal::new(GenerationPhase::Starting),
            replacing_response,
        });
        self.active.update(|m| {
            m.insert(session_id, entry.clone());
        });

        let content = entry.content;
        let phase = entry.phase;
        let active = self.active;
        let finished = self.finished;
        let viewing = self.viewing;

        leptos::task::spawn_local(async move {
            let mut receipt = match fut.await {
                Ok(stream) => {
                    // The server persists the user turn before returning the
                    // assistant stream, so this is the submission acknowledgement.
                    on_accepted.run(());
                    phase.set(GenerationPhase::Streaming);
                    consume_stream(stream, content, phase).await
                }
                Err(e) => GenerationReceipt {
                    outcome: GenerationOutcome::Failed,
                    persisted: false,
                    error: Some(e.to_string()),
                },
            };

            if receipt.outcome == GenerationOutcome::Failed && receipt.error.is_none() {
                receipt.error = Some("The provider stopped unexpectedly.".to_string());
            }

            let streamed = content.get_untracked();
            active.update(|m| {
                m.remove(&session_id);
            });
            finished.update(|m| {
                m.insert(
                    session_id,
                    FinishedGeneration {
                        receipt: receipt.clone(),
                        content: streamed,
                    },
                );
            });

            // Toasts supplement the durable inline state when the reply ended
            // on another screen. They are not the only error reporting path.
            if viewing.get_untracked() != Some(session_id) {
                match receipt.outcome {
                    GenerationOutcome::Completed => toast.success("Reply finished"),
                    GenerationOutcome::Stopped => toast.show(
                        "Generation stopped",
                        crate::components::ui::toast::ToastVariant::Default,
                    ),
                    GenerationOutcome::Failed => toast.error(format!(
                        "Reply failed: {}",
                        receipt.error.as_deref().unwrap_or("Unknown provider error")
                    )),
                }
            }
            on_finish.run(session_id);
        });
    }

    pub fn request_stopping(&self, session_id: Uuid) {
        let entry = self
            .active
            .with_untracked(|active| active.get(&session_id).cloned());
        if let Some(entry) = entry {
            entry.phase.set(GenerationPhase::Stopping);
        }
    }

    pub fn stop_request_failed(&self, session_id: Uuid) {
        let entry = self
            .active
            .with_untracked(|active| active.get(&session_id).cloned());
        if let Some(entry) = entry {
            entry
                .phase
                .set(if entry.content.get_untracked().is_empty() {
                    GenerationPhase::Starting
                } else {
                    GenerationPhase::Streaming
                });
        }
    }

    pub fn begin_submission(&self, session_id: Uuid, submission: PendingSubmission) {
        self.submissions.update(|pending| {
            pending.insert(session_id, submission);
        });
    }

    pub fn accept_submission(&self, session_id: Uuid) {
        self.submissions.update(|pending| {
            pending.remove(&session_id);
        });
        self.clear_draft(session_id);
    }

    /// Reconcile even after the originating chat controller has unmounted.
    /// A matching *new* user id is required, so repeated text cannot confuse an
    /// old turn with the submission that just failed or succeeded.
    pub fn reconcile_submission(&self, session_id: Uuid, messages: &[Message]) -> bool {
        let submission = self
            .submissions
            .with_untracked(|pending| pending.get(&session_id).cloned());
        let Some(submission) = submission else {
            return false;
        };
        if !submission_was_persisted(&submission, messages) {
            return false;
        }
        self.accept_submission(session_id);
        true
    }

    pub fn draft(&self, session_id: Uuid) -> ChatDraft {
        self.drafts
            .get_untracked()
            .get(&session_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn save_draft(&self, session_id: Uuid, draft: ChatDraft) {
        self.drafts.update(|drafts| {
            if draft.content.is_empty() && draft.images.is_empty() {
                drafts.remove(&session_id);
            } else {
                drafts.insert(session_id, draft);
            }
        });
    }

    /// Update only the text of a draft. Kept separate from image data so that
    /// typing never re-clones (potentially many MB of) attached image base64.
    pub fn set_draft_content(&self, session_id: Uuid, content: String) {
        self.drafts
            .update(|drafts| match drafts.get_mut(&session_id) {
                Some(existing) => {
                    if content.is_empty() && existing.images.is_empty() {
                        drafts.remove(&session_id);
                    } else {
                        existing.content = content;
                    }
                }
                None => {
                    if !content.is_empty() {
                        drafts.insert(
                            session_id,
                            ChatDraft {
                                content,
                                images: Vec::new(),
                            },
                        );
                    }
                }
            });
    }

    /// Update only the attached images of a draft, leaving the text untouched.
    pub fn set_draft_images(&self, session_id: Uuid, images: Vec<ImageUpload>) {
        self.drafts
            .update(|drafts| match drafts.get_mut(&session_id) {
                Some(existing) => {
                    if images.is_empty() && existing.content.is_empty() {
                        drafts.remove(&session_id);
                    } else {
                        existing.images = images;
                    }
                }
                None => {
                    if !images.is_empty() {
                        drafts.insert(
                            session_id,
                            ChatDraft {
                                content: String::new(),
                                images,
                            },
                        );
                    }
                }
            });
    }

    pub fn clear_draft(&self, session_id: Uuid) {
        self.drafts.update(|drafts| {
            drafts.remove(&session_id);
        });
    }
}

async fn consume_stream(
    stream: TextStream,
    content: RwSignal<String>,
    phase: RwSignal<GenerationPhase>,
) -> GenerationReceipt {
    let mut stream = stream.into_inner();
    let mut terminal = None;
    let mut transport_error = None;

    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(text) => {
                if let Some(encoded) = text.strip_prefix(GENERATION_TERMINAL_PREFIX) {
                    match serde_json::from_str::<GenerationReceipt>(encoded) {
                        Ok(receipt) => terminal = Some(receipt),
                        Err(_) => {
                            transport_error =
                                Some("Invalid generation status from server".to_string())
                        }
                    }
                } else {
                    content.update(|s| s.push_str(&text));
                }
            }
            Err(e) => {
                transport_error = Some(e.to_string());
                break;
            }
        }
    }

    if let Some(error) = transport_error {
        return GenerationReceipt {
            outcome: GenerationOutcome::Failed,
            persisted: false,
            error: Some(error),
        };
    }

    terminal.unwrap_or(GenerationReceipt {
        // Compatibility fallback for an older server or an abruptly closed
        // stream without a control frame. The visible content is still kept.
        outcome: if phase.get_untracked() == GenerationPhase::Stopping {
            GenerationOutcome::Stopped
        } else {
            GenerationOutcome::Completed
        },
        persisted: true,
        error: None,
    })
}

pub fn use_generation_tracker() -> GenerationTracker {
    expect_context::<GenerationTracker>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_message(id: Uuid, content: &str) -> Message {
        Message {
            id,
            session_id: Uuid::new_v4(),
            role: MessageRole::User,
            content: content.to_string(),
            attachments: Vec::new(),
            model_used: None,
            provider_id: None,
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn offscreen_reconcile_clears_only_a_matching_new_submission() {
        let owner = Owner::new();
        owner.set();
        let tracker = GenerationTracker::new(RwSignal::new(None));
        let session = Uuid::new_v4();
        let previous = Uuid::new_v4();
        tracker.save_draft(
            session,
            ChatDraft {
                content: "same text".to_string(),
                images: Vec::new(),
            },
        );
        tracker.begin_submission(
            session,
            PendingSubmission {
                content: "same text".to_string(),
                image_count: 0,
                previous_user_id: Some(previous),
            },
        );
        assert!(!tracker.reconcile_submission(session, &[user_message(previous, "same text")]));
        assert_eq!(tracker.draft(session).content, "same text");
        assert!(tracker.reconcile_submission(
            session,
            &[
                user_message(previous, "same text"),
                user_message(Uuid::new_v4(), "same text")
            ]
        ));
        assert!(tracker.draft(session).content.is_empty());
        assert!(!tracker.submissions.get_untracked().contains_key(&session));
    }
}
