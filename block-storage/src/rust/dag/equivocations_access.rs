// Read-modify-write contract for the equivocation tracker.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.5,
// design/14a-tier-architecture.md §6 (Plan-agent Item 7 of the
// principled-resolution session).
//
// The trait is the **contract** that a tracker-RMW must run under
// some lock-protected critical section, excluding all other RMW
// calls on the same instance. Implementors choose their own lock:
//   * `BlockDagKeyValueStorage` (production) uses `std::sync::Mutex`
//     via its `global_lock` field.
//   * Test harnesses can use `loom::sync::Mutex` to exhaustively
//     enumerate thread interleavings (T-9.2 verification).
//
// The shape (FnOnce, returning Result<A, KvStoreError>) matches
// the existing inherent method on `BlockDagKeyValueStorage`. The
// inherent method continues to exist as a transitional shim that
// delegates to the trait impl; this avoids breaking unqualified
// call sites that don't import the trait.
//
// Theorem citation: T-9.2 (atomic record insert,
// `formal/rocq/slashing/theories/BugFixAtomicTracker.v`) is the
// formal-verification anchor for this contract. The trait is an
// additive refinement: it adds a checkable type-level contract
// without changing any production behaviour.

use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::dag::equivocation_tracker_store::EquivocationTrackerStore;

/// Atomic read-modify-write under the implementor's chosen lock.
///
/// Implementations MUST hold a critical section spanning the entire
/// `f` invocation. No other call to `access_equivocations_tracker`
/// on the same instance may interleave with the closure. This is
/// the type-level expression of the bug-#2 fix: lock-free RMW on
/// the tracker is forbidden; every RMW routes through this trait.
pub trait EquivocationsAccess {
    fn access_equivocations_tracker<A>(
        &self,
        f: impl FnOnce(&EquivocationTrackerStore) -> Result<A, KvStoreError>,
    ) -> Result<A, KvStoreError>;
}
