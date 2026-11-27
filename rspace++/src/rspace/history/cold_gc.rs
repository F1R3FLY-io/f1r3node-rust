//! Online garbage collection for the cold (leaf) store (issue #1181).
//!
//! `rspace/cold` is content-addressed storage for the tuple-space leaves
//! that each state root's radix tree references by Blake2b-256 hash. It
//! grows with every checkpoint because each mutated channel writes a new
//! leaf whose hash supersedes the previous one. Nothing today deletes a
//! superseded leaf, so the file grows without bound even with
//! `enable-history-pruning` on — the history pruner's mark walk stops at
//! `Item::Leaf` because those values point into cold, not into history.
//!
//! This module hangs the hook points for the fix. A future commit will
//! land an LMDB-backed implementation that keeps a per-leaf refcount,
//! incremented when a checkpoint adds a new reference and decremented
//! when a retained root falls out of the retention window declared by
//! [`casper::rust::util::rspace_history_horizon::state_retention_floor`].
//! The GC worker then deletes cold entries whose refcount has reached
//! zero — in-place, under the same [`PruneGuard`] fence the history
//! pruner uses, so the node does not pause or restart.
//!
//! This first commit introduces only the trait and the tree-walk
//! helper. Default wiring is [`NoopColdGc`]: no sub-DB is created and
//! no behavior changes on the hot path.
//!
//! [`PruneGuard`]: crate::rspace::history::prune_guard::PruneGuard

use std::collections::HashSet;
use std::sync::Arc;

use shared::rust::store::key_value_store::KeyValueStore;

use crate::rspace::errors::HistoryError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::prune_guard::PruneGuard;
use crate::rspace::history::radix_tree::{Item, decode};

/// Hook invoked by the history checkpoint commit path and by the casper
/// retention-window driver. Implementations decide whether to track
/// per-leaf refcounts, run audits, delete orphaned cold entries, or do
/// nothing.
///
/// Every call happens under the rspace-level [`PruneGuard`] shared side
/// (for `on_checkpoint`) or under coordination with it (for `on_expire`),
/// so implementations do not need their own fence against the history
/// pruner.
///
/// [`PruneGuard`]: crate::rspace::history::prune_guard::PruneGuard
pub trait ColdGc: Send + Sync {
    /// Called after a new root's trie nodes have been durably written.
    /// `new_root` is retained; `prev_root` is the root this checkpoint
    /// derived from. `history_store` exposes the trie-node store so the
    /// implementation can walk `new_root`'s tree to collect
    /// [`leaves(new_root)`][leaves_from_roots] if it maintains refcounts.
    ///
    /// Must not panic on a missing node (the joiner / LFS-bootstrap case
    /// leaves subtrees unresolvable); treat the walk's partial result as
    /// authoritative and return without error.
    ///
    /// [leaves_from_roots]: leaves_from_roots
    fn on_checkpoint(
        &self,
        new_root: &Blake2b256Hash,
        prev_root: &Blake2b256Hash,
        history_store: &dyn KeyValueStore,
    ) -> Result<(), HistoryError>;

    /// Called when a root falls out of the retention window. The casper
    /// driver computes the set of roots retiring since the previous pass
    /// by diffing retention sets across GC invocations.
    ///
    /// The implementation can walk the retiring root's tree to decrement
    /// refcounts for each leaf it referenced, but MUST do so BEFORE the
    /// history pruner sweeps the root's subtree — otherwise the walk
    /// races a tree it is reading.
    fn on_expire(
        &self,
        expired_root: &Blake2b256Hash,
        history_store: &dyn KeyValueStore,
    ) -> Result<(), HistoryError>;

    /// Observational read of a leaf's refcount. Audit comparators and
    /// tests use it to verify the invariant `refcount[H] = |{retained R
    /// : H ∈ leaves(R)}|`. Implementations that track no state return
    /// 0.
    fn refcount(&self, leaf_hash: &Blake2b256Hash) -> Result<u64, HistoryError>;

    /// Called by the casper GC driver immediately before
    /// `HistoryRepository::prune_history_atomic`. Diffs
    /// `new_retention_set` against the implementation's record of the
    /// previous pass's retention set and invokes `on_expire` for each
    /// root that is retiring. Persists `new_retention_set` as the new
    /// record.
    ///
    /// Returns the retiring root hashes for operator-visible metrics.
    /// Implementations that track no state return an empty Vec and
    /// perform no side effects.
    ///
    /// Runs BEFORE the history pruner's sweep (not during), so each
    /// retiring root's trie nodes are still readable when `on_expire`
    /// walks them. The caller is responsible for ordering.
    fn advance_retention(
        &self,
        new_retention_set: &[Blake2b256Hash],
        history_store: &dyn KeyValueStore,
    ) -> Result<Vec<Blake2b256Hash>, HistoryError>;

    /// Audit the refcount invariant by computing
    /// `expected[H] = |{R ∈ retention_set_store : H ∈ leaves(R)}|` via
    /// mark walk and comparing against the actual refcount store.
    ///
    /// Intended as a periodic canary (Phase 2 of issue #1181). The
    /// invariant should always hold in the absence of bugs; a non-
    /// zero divergence count indicates drift and should alert
    /// operators. The return value is intentionally small — counts
    /// plus a bounded sample — so it is safe to emit as log fields
    /// and metrics.
    ///
    /// Implementations that track no state return an empty report
    /// and perform no walks.
    fn audit(&self, history_store: &dyn KeyValueStore) -> Result<AuditReport, HistoryError>;

    /// Delete every cold entry whose key is not in the refcount
    /// store — that is, every leaf no retained root currently
    /// references (issue #1181, Phase 4). The caller passes the
    /// cold leaf store; the implementation batches `scan_page` +
    /// batch-delete pairs under its own fence so concurrent
    /// `on_checkpoint` cannot add a reference the sweep then
    /// strands.
    ///
    /// Safety precondition: a `ColdGc` implementation that has
    /// never been bootstrapped (empty retention set AND empty
    /// refcount store) must return a zero report without scanning,
    /// because every entry in the cold store would otherwise look
    /// orphaned and be deleted. The LMDB implementation enforces
    /// this; `NoopColdGc` has nothing to delete.
    fn sweep(&self, cold_store: &dyn KeyValueStore) -> Result<SweepReport, HistoryError>;

    /// Return the [`PruneGuard`] this implementation uses to fence
    /// its sweep against runtime writers, if any. The
    /// `HistoryRepository` constructor adopts the returned guard as
    /// its own, so `do_checkpoint`'s `enter_write` fences the same
    /// lock the sweep's `enter_sweep` holds — eliminating the
    /// cold-write / refcount-write race.
    ///
    /// `None` means the implementation needs no shared fence; the
    /// repository creates its own `PruneGuard`. `NoopColdGc`
    /// returns `None`.
    fn prune_guard(&self) -> Option<Arc<PruneGuard>> { None }
}

/// Outcome of a single cold-GC sweep pass (issue #1181, Phase 4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Cold-store rows read via `scan_page`. A single pass walks the
    /// whole store.
    pub scanned: usize,
    /// Cold-store rows actually deleted.
    pub deleted: usize,
    /// Number of `scan_page`+delete batches executed. Each batch
    /// takes the implementation's mutex briefly, so per-batch
    /// writer stalls stay bounded.
    pub batches: usize,
    /// `true` when the sweep refused to run because its retention-
    /// set preconditions were not met (empty retention store →
    /// pre-init-pass or fresh node). Operators should see this as a
    /// non-fatal "skipped" status rather than a zero-delete result.
    pub skipped_preconditions_not_met: bool,
}

/// Summary of a single audit pass over the cold-GC bookkeeping
/// tables (issue #1181, Phase 2). Shape is stable across
/// implementations so the casper driver can treat it uniformly.
#[derive(Debug, Clone, Default)]
pub struct AuditReport {
    /// Retained roots the audit walked. Equals the number of entries
    /// in the retention-set store at audit time.
    pub retention_roots: usize,
    /// Retained roots the audit could not walk end-to-end because at
    /// least one trie node was missing from the history store
    /// (joiner mid-sync, prior pruning, LFS-bootstrap gap). Their
    /// leaf contributions are excluded from `expected_entries` and
    /// their leaves do not count against divergence.
    pub unwalkable_roots: usize,
    /// Non-zero entries scanned in the refcount store at audit time.
    pub refcount_entries: usize,
    /// Distinct leaves reachable from the fully-walkable subset of
    /// the retention set.
    pub expected_entries: usize,
    /// Total leaves where (actual, expected) disagree, combining
    /// mismatched counts, missing entries, and extra entries.
    pub divergence_total: usize,
    /// A bounded sample of divergences for operator debugging. Not
    /// a complete list — divergence_total is the authoritative count.
    pub samples: Vec<AuditDivergence>,
}

/// Classification of an audit divergence. The casper driver uses this
/// to pick the right alerting cue; operators triage differently by
/// kind (mismatched count ≈ bookkeeping bug; missing ≈ leaked
/// decrement; extra ≈ leaked increment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditDivergenceKind {
    /// Both the refcount store and the mark walk saw the leaf, but
    /// at different counts.
    MismatchedCount,
    /// The mark walk reached the leaf but the refcount store has no
    /// entry for it — a dropped increment or an over-decrement.
    MissingFromRefcount,
    /// The refcount store carries a non-zero entry but the mark
    /// walk reaches no retained root referencing the leaf — a
    /// leaked increment or a dropped decrement.
    ExtraInRefcount,
}

/// One divergence record. The `kind` label is derived from the
/// actual/expected pair at construction so callers do not have to
/// re-derive it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditDivergence {
    pub leaf: Blake2b256Hash,
    pub actual: u64,
    pub expected: u64,
    pub kind: AuditDivergenceKind,
}

impl AuditDivergence {
    pub fn classify(leaf: Blake2b256Hash, actual: u64, expected: u64) -> Self {
        let kind = match (actual, expected) {
            (0, _) => AuditDivergenceKind::MissingFromRefcount,
            (_, 0) => AuditDivergenceKind::ExtraInRefcount,
            _ => AuditDivergenceKind::MismatchedCount,
        };
        Self {
            leaf,
            actual,
            expected,
            kind,
        }
    }
}

/// Cap on the number of divergence samples a single audit returns.
/// Enough to surface a few representative cases in logs without
/// bloating memory when a systemic bug makes every leaf diverge.
pub const AUDIT_SAMPLE_CAP: usize = 16;

/// No-op implementation. The repository defaults to this; wiring a real
/// implementation is opt-in behind config.
pub struct NoopColdGc;

impl ColdGc for NoopColdGc {
    fn on_checkpoint(
        &self,
        _new_root: &Blake2b256Hash,
        _prev_root: &Blake2b256Hash,
        _history_store: &dyn KeyValueStore,
    ) -> Result<(), HistoryError> {
        Ok(())
    }

    fn on_expire(
        &self,
        _expired_root: &Blake2b256Hash,
        _history_store: &dyn KeyValueStore,
    ) -> Result<(), HistoryError> {
        Ok(())
    }

    fn refcount(&self, _leaf_hash: &Blake2b256Hash) -> Result<u64, HistoryError> { Ok(0) }

    fn advance_retention(
        &self,
        _new_retention_set: &[Blake2b256Hash],
        _history_store: &dyn KeyValueStore,
    ) -> Result<Vec<Blake2b256Hash>, HistoryError> {
        Ok(Vec::new())
    }

    fn audit(&self, _history_store: &dyn KeyValueStore) -> Result<AuditReport, HistoryError> {
        Ok(AuditReport::default())
    }

    fn sweep(&self, _cold_store: &dyn KeyValueStore) -> Result<SweepReport, HistoryError> {
        Ok(SweepReport::default())
    }
}

/// Walk the radix-tree subtrees rooted at each hash in `roots` and
/// collect the union of distinct `Item::Leaf.value` hashes — the cold
/// keys those roots reference.
///
/// This is the pruner's mark walk extended to descend one more level:
/// `walk_and_mark` in `history_repository_impl.rs` records visited node
/// hashes and ignores leaves on purpose (leaves point into cold, not
/// into history). Here we record the leaf hashes instead.
///
/// A missing trie node short-circuits that frontier entry — same
/// behavior as [`walk_and_mark`][walk_and_mark]. Shared subtrees across
/// roots are walked once because the internal `visited` set
/// short-circuits.
///
/// [walk_and_mark]: crate::rspace::history::history_repository_impl
pub fn leaves_from_roots(
    store: &dyn KeyValueStore,
    roots: &[Blake2b256Hash],
) -> Result<HashSet<Blake2b256Hash>, HistoryError> {
    leaves_from_roots_strict(store, roots).map(|(set, _)| set)
}

/// Strict variant of [`leaves_from_roots`]: returns the collected
/// leaf hashes AND a `bool` that is `true` only if every reachable
/// node was present in `store`. A `false` signal means at least one
/// frontier hash resolved to no bytes (joiner mid-sync, history
/// pruner removed the subtree, LFS-bootstrap gap); the returned
/// leaf set is a partial result, not the full reachable set.
///
/// The audit comparator uses this to tell "the bookkeeping is
/// inconsistent" from "we couldn't walk the whole tree," and to
/// avoid flagging unwalkable-root cases as refcount drift.
pub fn leaves_from_roots_strict(
    store: &dyn KeyValueStore,
    roots: &[Blake2b256Hash],
) -> Result<(HashSet<Blake2b256Hash>, bool), HistoryError> {
    let mut leaves: HashSet<Blake2b256Hash> = HashSet::new();
    let mut visited: HashSet<Blake2b256Hash> = HashSet::new();
    let mut frontier: Vec<Blake2b256Hash> = roots.to_vec();
    let mut complete = true;
    while let Some(hash) = frontier.pop() {
        if !visited.insert(hash.clone()) {
            continue;
        }
        let bytes = store.get_one(&hash.bytes()).map_err(HistoryError::from)?;
        let Some(serialized) = bytes else {
            complete = false;
            continue;
        };
        let node = decode(serialized);
        for item in node {
            match item {
                Item::NodePtr { ptr, .. } => {
                    frontier.push(Blake2b256Hash::from_bytes(ptr));
                }
                Item::Leaf { value, .. } => {
                    leaves.insert(Blake2b256Hash::from_bytes(value));
                }
                Item::EmptyItem => {}
            }
        }
    }
    Ok((leaves, complete))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use shared::rust::store::key_value_store::KeyValueStore;

    use super::*;
    use crate::rspace::history::radix_tree::{Node, empty_node, encode, hash_node};
    use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    /// Put a single radix-tree node into `store`, keyed by its own hash,
    /// and return the hash.
    fn put_node(store: &dyn KeyValueStore, node: &Node) -> Blake2b256Hash {
        let node_bytes = hash_node(node);
        store
            .put(vec![(node_bytes.0.clone(), encode(node))])
            .unwrap();
        Blake2b256Hash::from_bytes(node_bytes.0)
    }

    /// Build a trie with a root pointing to a child, and the child
    /// holding two leaves whose values are the fixed cold-hash bytes
    /// below. Returns (root_hash, leaf_a, leaf_b).
    fn seeded_trie() -> (Blake2b256Hash, Blake2b256Hash, Blake2b256Hash, Arc<dyn KeyValueStore>) {
        let store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());

        let leaf_a_bytes = vec![0x11; 32];
        let leaf_b_bytes = vec![0x22; 32];

        let mut child = empty_node();
        child[0] = Item::Leaf {
            prefix: vec![0xAA],
            value: leaf_a_bytes.clone(),
        };
        child[1] = Item::Leaf {
            prefix: vec![0xBB],
            value: leaf_b_bytes.clone(),
        };
        let child_hash = put_node(store.as_ref(), &child);

        let mut root = empty_node();
        root[0] = Item::NodePtr {
            prefix: vec![],
            ptr: child_hash.bytes(),
        };
        let root_hash = put_node(store.as_ref(), &root);

        (
            root_hash,
            Blake2b256Hash::from_bytes(leaf_a_bytes),
            Blake2b256Hash::from_bytes(leaf_b_bytes),
            store,
        )
    }

    /// The core contract: walking from a root yields every reachable leaf
    /// hash and nothing else.
    #[test]
    fn leaves_from_roots_collects_reachable_cold_hashes() {
        let (root, leaf_a, leaf_b, store) = seeded_trie();
        let leaves = leaves_from_roots(store.as_ref(), &[root]).unwrap();
        assert_eq!(leaves.len(), 2);
        assert!(leaves.contains(&leaf_a));
        assert!(leaves.contains(&leaf_b));
    }

    /// A missing frontier hash must short-circuit, not panic. Matches
    /// `walk_and_mark`'s behavior for the joiner / LFS-bootstrap case.
    #[test]
    fn leaves_from_roots_tolerates_missing_node() {
        let store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let phantom = Blake2b256Hash::from_bytes(vec![0x99; 32]);
        let leaves = leaves_from_roots(store.as_ref(), &[phantom]).unwrap();
        assert!(leaves.is_empty());
    }

    /// Shared subtrees across multiple roots are walked once: the
    /// returned set dedups by hash, so passing the same root twice
    /// returns the same leaves.
    #[test]
    fn leaves_from_roots_deduplicates_shared_subtrees() {
        let (root, _leaf_a, _leaf_b, store) = seeded_trie();
        let single = leaves_from_roots(store.as_ref(), &[root.clone()]).unwrap();
        let doubled = leaves_from_roots(store.as_ref(), &[root.clone(), root]).unwrap();
        assert_eq!(single, doubled);
    }

    /// NoopColdGc never errors and is cheap enough to call on every
    /// checkpoint even in production.
    #[test]
    fn noop_cold_gc_is_a_noop() {
        let store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let gc = NoopColdGc;
        let r = Blake2b256Hash::from_bytes(vec![0; 32]);
        let r2 = Blake2b256Hash::from_bytes(vec![1; 32]);
        gc.on_checkpoint(&r, &r2, store.as_ref()).unwrap();
        gc.on_expire(&r, store.as_ref()).unwrap();
        assert_eq!(gc.refcount(&r).unwrap(), 0);
        assert!(
            gc.advance_retention(&[r.clone()], store.as_ref())
                .unwrap()
                .is_empty()
        );
    }
}
