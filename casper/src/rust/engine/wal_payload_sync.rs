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

/// Returned by the (future) `spawn_periodic_tick`.  Carries both
/// the tick task's `JoinHandle` (so the caller can `abort()` on
/// shutdown) and a [`WalPayloadTickStop`] handle (so the block-
/// processing catch-up path can raise the graceful-stop signal).
///
/// Fields are `pub` so `spawn_periodic_tick` (follow-up slice)
/// constructs via struct-literal; no accessor churn needed when
/// the spawn site lands.
#[allow(dead_code)]
pub struct WalPayloadTickHandle {
    pub join_handle: tokio::task::JoinHandle<()>,
    pub stop: WalPayloadTickStop,
}

/// Counters produced by the (future) enumerator — how many of the
/// unique payload hashes extracted from a WAL slice the local
/// reducer resolved vs. how many got enqueued for peer fetch.
/// Telemetry carries these so operators can distinguish "reducer
/// worked, no wire traffic needed" from "we're depending on peers
/// to serve every byte."
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnumerateStats {
    /// Payload hashes handed to the retriever with reproduced
    /// bytes (via `mark_resolved`).  Fetch protocol will NOT
    /// contact peers for these.
    pub resolved_locally: usize,
    /// Payload hashes enqueued for peer fetch (via
    /// `enqueue_payload`) — either because the reducer returned
    /// `None` or because its reproduced bytes failed the hash
    /// check.
    pub enqueued_for_fetch: usize,
}

/// Report from the (future) `apply_wal_slice_after_fetch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootApplyReport {
    pub enumerated: EnumerateStats,
    /// Number of unique payload hashes populated in the sidecar
    /// (equal to `enumerated.resolved_locally +
    /// enumerated.enqueued_for_fetch` on the happy path; less if
    /// peers failed to serve some).
    pub sidecar_populated: usize,
    /// Number of WAL entries in the applied slice (informational —
    /// includes observation-only variants the applier skips).
    pub wal_entries: usize,
}

/// Reasons the (future) `apply_wal_slice_after_fetch` can fail.
/// Callers pattern-match to distinguish "byzantine input, log +
/// skip" from "genuine peer/network shortfall, retry later".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootApplyError {
    /// `driver.is_complete()` did not return true within the
    /// configured timeout.  `pending_count` is the number of
    /// hashes still outstanding when the timeout fired.  Callers
    /// may re-issue the flow after peer conditions improve.
    PayloadFetchTimeout { pending_count: usize },
    /// A hash the enumerator populated is missing from the driver
    /// by the time `take_bytes` ran — indicates the driver dropped
    /// a resolved entry between `is_complete()` and the sidecar
    /// build (stale-eviction races with the poll loop, or a
    /// `driver.stop()` called mid-collect).
    MissingResolvedHash { hash_hex: String },
    /// The applier returned an `ApplierError` variant (missing
    /// sidecar / unsupported PayloadRef / out-of-allowed-roots
    /// path / NSS failure / IO error / etc).  Byzantine or
    /// misconfigured input; the subscriber logs + continues to
    /// the next snapshot.
    ApplierFailed { message: String },
    /// The `spawn_blocking` task carrying the applier panicked.
    /// Defense-in-depth variant: today's applier is Result-based
    /// with no panic paths of its own, but a future refactor that
    /// reintroduces a panic will surface here rather than killing
    /// the subscriber loop.
    ApplierPanic { message: String },
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

    #[test]
    fn enumerate_stats_default_is_zero() {
        let stats = EnumerateStats::default();
        assert_eq!(stats.resolved_locally, 0);
        assert_eq!(stats.enqueued_for_fetch, 0);
    }

    #[test]
    fn enumerate_stats_eq_and_copy() {
        let a = EnumerateStats {
            resolved_locally: 3,
            enqueued_for_fetch: 7,
        };
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn boot_apply_report_builds_and_compares() {
        let a = BootApplyReport {
            enumerated: EnumerateStats {
                resolved_locally: 5,
                enqueued_for_fetch: 2,
            },
            sidecar_populated: 7,
            wal_entries: 12,
        };
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(
            a.sidecar_populated,
            a.enumerated.resolved_locally + a.enumerated.enqueued_for_fetch,
            "happy-path invariant: sidecar == resolved + enqueued"
        );
    }

    #[test]
    fn boot_apply_error_variants_distinct_under_eq() {
        let timeout = BootApplyError::PayloadFetchTimeout { pending_count: 3 };
        let missing = BootApplyError::MissingResolvedHash {
            hash_hex: "deadbeef".to_string(),
        };
        let failed = BootApplyError::ApplierFailed {
            message: "io".to_string(),
        };
        let panicked = BootApplyError::ApplierPanic {
            message: "boom".to_string(),
        };
        assert_ne!(timeout, missing);
        assert_ne!(missing, failed);
        assert_ne!(failed, panicked);
        assert_eq!(timeout, BootApplyError::PayloadFetchTimeout {
            pending_count: 3
        });
    }
}
