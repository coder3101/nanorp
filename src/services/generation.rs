//! Registry of in-flight streaming generations, keyed by chat session, so
//! `stop_generation` can cancel one mid-stream. Cancellation is signalled
//! through a `watch` channel the streaming loop selects on. A session has at
//! most one active generation: starting a new one cancels and replaces the
//! previous, and each generation only ever deregisters itself.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tokio::sync::watch;
use uuid::Uuid;

struct Entry {
    generation_id: Uuid,
    cancel_tx: watch::Sender<bool>,
}

static ACTIVE: OnceLock<Mutex<HashMap<Uuid, Entry>>> = OnceLock::new();

fn active() -> &'static Mutex<HashMap<Uuid, Entry>> {
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a new generation for a session, cancelling any generation already
/// running for it. Returns the generation id (pass it to [`finish`]) and the
/// cancellation receiver: it resolves once cancellation is requested — either
/// explicitly via [`cancel`] or implicitly when a newer generation replaces
/// this one (the old sender is dropped).
pub fn begin(session_id: Uuid) -> (Uuid, watch::Receiver<bool>) {
    let generation_id = Uuid::new_v4();
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let mut map = active().lock().expect("generation registry poisoned");
    if let Some(old) = map.insert(session_id, Entry { generation_id, cancel_tx }) {
        let _ = old.cancel_tx.send(true);
    }
    (generation_id, cancel_rx)
}

/// Remove a generation from the registry once its stream ends (naturally or
/// after cancellation). No-op if a newer generation has replaced this one.
pub fn finish(session_id: Uuid, generation_id: Uuid) {
    let mut map = active().lock().expect("generation registry poisoned");
    if map.get(&session_id).map(|e| e.generation_id) == Some(generation_id) {
        map.remove(&session_id);
    }
}

/// RAII wrapper around [`finish`]: deregisters the generation when dropped,
/// which also covers streams that are dropped mid-flight (client disconnect)
/// and would otherwise leave a stale registry entry.
pub struct FinishGuard {
    session_id: Uuid,
    generation_id: Uuid,
}

impl FinishGuard {
    pub fn new(session_id: Uuid, generation_id: Uuid) -> Self {
        Self { session_id, generation_id }
    }
}

impl Drop for FinishGuard {
    fn drop(&mut self) {
        finish(self.session_id, self.generation_id);
    }
}

/// Request cancellation of the session's active generation, if any. Returns
/// whether a generation was active.
pub fn cancel(session_id: Uuid) -> bool {
    let map = active().lock().expect("generation registry poisoned");
    match map.get(&session_id) {
        Some(entry) => {
            let _ = entry.cancel_tx.send(true);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_signals_the_active_generation() {
        let session = Uuid::new_v4();
        let (gen_id, rx) = begin(session);

        assert!(cancel(session));
        assert!(*rx.borrow());

        finish(session, gen_id);
        assert!(!cancel(session), "no active generation after finish");
    }

    #[test]
    fn new_generation_cancels_and_replaces_previous() {
        let session = Uuid::new_v4();
        let (old_id, old_rx) = begin(session);
        let (new_id, new_rx) = begin(session);

        // Old generation is cancelled (sender dropped counts as cancellation
        // for `changed()`, and the map now points at the new generation).
        assert!(old_rx.has_changed().is_err() || *old_rx.borrow());
        assert!(!*new_rx.borrow());

        // The old stream finishing must not evict the new entry.
        finish(session, old_id);
        assert!(cancel(session));
        assert!(*new_rx.borrow());

        finish(session, new_id);
    }

    #[test]
    fn cancel_without_active_generation_returns_false() {
        assert!(!cancel(Uuid::new_v4()));
    }
}
