// Phase 7b-2 WAL payload sync driver (joiner-side).
//
// Seed module for the eventual full `WalPayloadSyncDriver` port.
// This slice lands only the graceful-stop handle used by the
// future tick-loop; the driver + spawn_periodic_tick + enumeration
// pipeline land in follow-up slices.
//
// The handle lives in a dedicated module (rather than inside
// `wal_payload_server.rs`) because its ownership shape is
// joiner-side: it's created by `spawn_periodic_tick` and raised by
// the block-processing catch-up path on the joiner, never touched
// by the serving validator.  Keeping it adjacent to the eventual
// `WalPayloadSyncDriver` reduces churn when the driver lands.

use std::sync::Arc;

/// Graceful-stop handle for the (future) periodic tick task.  Held
/// by the block-processing catch-up path on the joiner; `.stop()`
/// raises a `tokio::sync::Notify` that the tick loop selects on,
/// causing it to exit cleanly at its next select boundary rather
/// than being hard-aborted mid-`.await`.
///
/// `.stop()` is idempotent — tokio's `Notify` collapses multiple
/// pending notifications into a single permit, so a second call
/// before the loop has consumed the first is a no-op.
///
/// `Clone` is deliberate: the catch-up path and the shutdown
/// orchestrator both need to hold a handle, and the shared
/// `Arc<Notify>` makes multiple-waker semantics free.
#[derive(Clone)]
pub struct WalPayloadTickStop {
    signal: Arc<tokio::sync::Notify>,
}

impl WalPayloadTickStop {
    /// Construct a fresh stop handle.  The eventual
    /// `spawn_periodic_tick` constructs one of these and hands
    /// both ends out — one to its internal loop (via
    /// `signal.notified().await` in the `select!`) and one to the
    /// returned `WalPayloadTickHandle::stop`.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            signal: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Raise the stop signal.  The tick loop selecting on this
    /// signal will exit at its next select boundary (immediately
    /// if idle, or after the current in-flight `driver.tick(...)`
    /// completes).  Idempotent per the type-level docstring.
    pub fn stop(&self) { self.signal.notify_one(); }

    /// Expose the underlying `Arc<Notify>` to the (future) tick
    /// loop so it can `.notified().await` on the same signal
    /// `.stop()` raises.  Private within the engine subtree —
    /// external callers go through `.stop()`.
    #[allow(dead_code)]
    pub(crate) fn signal(&self) -> &Arc<tokio::sync::Notify> { &self.signal }
}

impl Default for WalPayloadTickStop {
    fn default() -> Self { Self::new() }
}

impl std::fmt::Debug for WalPayloadTickStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WalPayloadTickStop")
            .field("signal", &"tokio::sync::Notify")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stop_wakes_a_waiter_on_the_shared_signal() {
        let handle = WalPayloadTickStop::new();
        let signal = handle.signal().clone();
        let waiter = tokio::spawn(async move {
            signal.notified().await;
        });
        handle.stop();
        tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter did not resume within 1s")
            .expect("waiter task panicked");
    }

    #[tokio::test]
    async fn stop_is_idempotent_when_pending() {
        // Two calls to .stop() before the waiter consumes the
        // permit collapse into one — the first .notified().await
        // resumes; a second .notified().await then re-blocks (no
        // orphaned permit).
        let handle = WalPayloadTickStop::new();
        handle.stop();
        handle.stop();
        let signal = handle.signal().clone();
        // First wait: consumes the single permit.
        tokio::time::timeout(std::time::Duration::from_millis(50), signal.notified())
            .await
            .expect("first notified() did not resume");
        // Second wait: no orphan permit, must time out.
        let second =
            tokio::time::timeout(std::time::Duration::from_millis(50), signal.notified()).await;
        assert!(second.is_err(), "idempotent stop leaked a second permit");
    }

    #[test]
    fn clone_shares_the_signal() {
        let a = WalPayloadTickStop::new();
        let b = a.clone();
        assert!(
            Arc::ptr_eq(a.signal(), b.signal()),
            "Clone must share the underlying Arc<Notify>"
        );
    }

    #[test]
    fn debug_impl_describes_shape() {
        let handle = WalPayloadTickStop::new();
        let formatted = format!("{handle:?}");
        assert!(formatted.contains("WalPayloadTickStop"));
        assert!(formatted.contains("tokio::sync::Notify"));
    }
}
