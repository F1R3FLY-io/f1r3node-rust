// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Atomic buffer-DAG transition (Bug #17 / T-9.20).
//
// Background. `BlockDagKeyValueStorage` and `CasperBufferKeyValueStorage`
// live in distinct LMDB environments (`dagstorage` and `casperbuffer`,
// see `rnode_key_value_store_manager.rs`). No single LMDB write
// transaction can span them; strict cross-store ACID is physically
// impossible without a journal.
//
// Hazard. Multiple validation paths execute a non-atomic pair:
//   1. `block_dag_storage.insert(block, mode)`   // commits to DAG store
//   2. `casper_buffer_storage.remove(hash)`      // commits to buffer store
// A process crash between (1) and (2) leaves the system in a drift
// state: block in DAG (as Invalid/Normal/Approved) but still in the
// casper buffer as a pending dependency. The launch-time reconciliation
// at `casper_launch.rs::send_buffer_pendants_to_casper` purges such
// stale pendants, so the **observable behavior** is preserved by the
// idempotent dispatch primitives — but the contract was implicit and
// vulnerable to silent regression from future mutators of the pair.
//
// This module promotes the implicit contract to a typed, documented
// chokepoint. Maps to:
//   - Theorem T-9.20 (`formal/rocq/slashing/theories/
//     BugFixAtomicBufferDagTransition.v`): for every crash point during
//     `atomic_insert_then_buffer`, applying `reconcile_buffer_against_dag`
//     on resume yields the same slashing projection as the no-crash run.
//   - Design §9.20 in `docs/theory/slashing/design/09-bug-fixes-and-rationale.md`
//     ("Bug #17 — Non-transactional buffer-DAG transition").
//
// Lock-order contract.
//   Lock A (acquired FIRST): `BlockDagKeyValueStorage::global_lock.write()`.
//   Lock B (acquired SECOND): `CasperBufferKeyValueStorage::state_lock.write()`
//     (via `CasperBufferKeyValueStorage::write_guard()`).
// All code paths that hold both must acquire them in this order to
// prevent process-local deadlock. Existing call sites either take both
// in this order (via this helper) or take exactly one (validation read
// paths, isolated buffer mutations). No code path takes B then A.

use models::rust::block_hash::BlockHashSerde;
use models::rust::casper::protocol::casper_message::BlockMessage;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use crate::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, InsertMode, KeyValueDagRepresentation,
};

/// Describes what (if anything) the atomic helper does to the casper
/// buffer after committing the DAG insert. Two variants — extend only
/// when a new code path genuinely needs a different buffer mutation
/// paired with a DAG insert.
pub enum BufferTransition {
    /// Remove the block's hash from the casper buffer (the
    /// `(insert, buffer.remove)` pattern used by valid-block admission
    /// and invalid-block recording).
    RemoveFromBuffer(BlockHashSerde),
    /// No buffer mutation (bootstrap / genesis paths that do not
    /// participate in the buffer's pending-dependency lifecycle).
    Skip,
}

/// Atomically perform a DAG insert followed by a buffer operation.
///
/// Acquires `dag.global_lock.write()` (lock A) then
/// `buffer.write_guard()` (lock B) under the documented lock order
/// at the top of this module. Performs the dag insert via
/// `insert_internal` (which expects the global_lock already held),
/// then the buffer op via `remove_unlocked` (which expects the buffer
/// state_lock already held), then releases both locks.
///
/// Idempotence guarantees ensure the helper is safe to call repeatedly:
/// * `BlockDagKeyValueStorage::insert_internal` short-circuits if the
///   block is already present.
/// * `CasperBufferKeyValueStorage::remove_unlocked` tolerates absent
///   hashes via `KvStoreError::InvalidArgument`, which this helper
///   filters out.
///
/// Crash semantics. A process crash during the pair leaves the system
/// in one of three states (post-restart):
///   (a) Pre-transition: neither operation committed. Resume replays
///       from buffer; validation reruns; same end state.
///   (b) Steady state: both ops committed. No reconciliation needed.
///   (c) Drift: dag insert committed, buffer remove did not.
///       `reconcile_buffer_against_dag` on resume closes the drift.
/// See T-9.20 (`BugFixAtomicBufferDagTransition.v`) for the
/// observational-equivalence theorem.
pub fn atomic_insert_then_buffer(
    dag: &BlockDagKeyValueStorage,
    block: &BlockMessage,
    mode: InsertMode,
    buffer: &CasperBufferKeyValueStorage,
    buffer_op: BufferTransition,
) -> Result<KeyValueDagRepresentation, KvStoreError> {
    let _dag_guard = dag.global_lock.write();
    let _buf_guard = buffer.write_guard();

    let updated = dag.insert_internal(block, mode)?;

    match buffer_op {
        BufferTransition::RemoveFromBuffer(hash) => match buffer.remove_unlocked(hash) {
            Ok(()) => {}
            // Idempotent: removing a hash that was never in the buffer
            // (or already removed by a concurrent path before lock
            // acquisition) is not an error.
            Err(KvStoreError::InvalidArgument(_)) => {}
            Err(e) => return Err(e),
        },
        BufferTransition::Skip => {}
    }

    Ok(updated)
}

/// On-resume reconciliation: walks every pendant in the buffer; for
/// any pendant whose hash is present in the DAG, removes it from the
/// buffer. Closes the (c) drift state from `atomic_insert_then_buffer`.
///
/// Promotes the launch-time logic at
/// `casper_launch.rs::send_buffer_pendants_to_casper` (lines 280-301
/// at time of writing) to a documented contract function. Existing
/// callers that need reconciliation should call this rather than
/// re-implementing the walk.
///
/// Returns the number of pendants purged (useful for metrics /
/// diagnostics).
pub fn reconcile_buffer_against_dag(
    buffer: &CasperBufferKeyValueStorage,
    dag: &KeyValueDagRepresentation,
) -> Result<usize, KvStoreError> {
    let pendants = buffer.get_pendants();
    let mut purged = 0;
    for pendant in pendants {
        if dag.contains(&pendant.0) {
            // remove() takes the buffer's write lock internally; safe
            // because reconcile_buffer_against_dag is called outside
            // any atomic_insert_then_buffer critical section (only at
            // resume, when no other paths hold the buffer's lock).
            match buffer.remove(pendant) {
                Ok(()) => purged += 1,
                Err(KvStoreError::InvalidArgument(_)) => {} // already removed
                Err(e) => return Err(e),
            }
        }
    }
    Ok(purged)
}
