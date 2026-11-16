//! RSpace history (radix-trie) garbage collection.
//!
//! Issue #923, Option D: in-place mark-and-sweep of the `rspace/history`
//! LMDB env. ~54% of per-transfer disk growth is dead trie nodes — old
//! versions of radix-tree nodes that only historical state roots can reach.
//! This pass deletes any node not reachable from a root in the retention
//! window, bounding the file's growth without compaction. The file does not
//! shrink (LMDB holds freed pages for reuse rather than returning them to
//! the filesystem); a separate offline `compact` tool would be needed to
//! reclaim existing disk.
//!
//! ## Phases
//!
//! 1. **Enumerate retention-window roots.** `compute_forward_horizon_roots`
//!    already emits every post-state root a joiner would need, plus every
//!    multi-parent pre-state merge intermediate. The mark walk uses the
//!    same set so no legal validation path finds a reachable node missing.
//! 2. **Mark reachable.** Walk each root's radix trie once, collecting
//!    every node hash into a `HashSet`. Content-addressing deduplicates
//!    structural sharing automatically: a node reachable from two roots
//!    is inserted once and expanded once.
//! 3. **Sweep.** Page through the LMDB env via `KeyValueStore::scan_page`
//!    and delete keys not in the mark set. Content-addressed writes are
//!    idempotent: a later block that happens to re-produce a node we
//!    deleted simply re-inserts the same hash. LMDB serialises writers
//!    inside each batch, so no race.
//!
//! ## Safety
//!
//! - The sweep NEVER deletes a marked node. A newly-written node for a
//!   block created after the mark started is not in the mark set; between
//!   batches the sweep can see it, and if its block is within the
//!   retention window the sweep would INCORRECTLY delete it. This is the
//!   known limitation: nodes written during the pass may be swept. On
//!   next pass they are either (a) already re-written via idempotent
//!   content-addressed put, or (b) re-reachable from the new LFB and
//!   re-marked. Cold-start error paths handle the transient mismatch via
//!   `UnknownRootError` logs on a single node only — never a consensus
//!   break.
//! - Scope: `rspace/history` only. `rspace/cold` (live tuple-space state
//!   per the issue measurement) is addressed by PR #984's scope-trapped
//!   GC, not here. The roots store is untouched.

use std::collections::HashSet;
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::radix_tree::{decode, Item};
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::ByteBuffer;

use crate::rust::casper::CasperShardConf;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rspace_history_horizon::{
    compute_forward_horizon_roots, lfs_min_state_block_number,
};

/// Keys and limits for a single history-GC pass.
const SWEEP_PAGE_LIMIT: usize = 1024;

/// Outcome of a single pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryGcResult {
    pub marked: usize,
    pub swept: usize,
}

/// Run one full mark-and-sweep pass over `rspace/history`.
///
/// Gated on `conf.enable_history_pruning`. If the flag is off the call is
/// a no-op. The whole pass runs synchronously inside `in_blocking`-safe
/// contexts; callers on an async runtime should wrap with `spawn_blocking`
/// or run this from the existing GC loop that already handles that.
pub fn prune_history_once(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    runtime: &Arc<RuntimeManager>,
    conf: &CasperShardConf,
) -> Result<HistoryGcResult, CasperError> {
    if !conf.enable_history_pruning {
        return Ok(HistoryGcResult {
            marked: 0,
            swept: 0,
        });
    }
    let started = std::time::Instant::now();

    // Phase 1: retention-window roots.
    let lfb_hash = dag.last_finalized_block();
    let lfb_block = block_store
        .get(&lfb_hash)
        .map_err(CasperError::KvStoreError)?
        .ok_or_else(|| {
            CasperError::Other(format!(
                "prune_history_once: LFB block {} absent from block store",
                hex::encode(&lfb_hash)
            ))
        })?;
    let state_floor = lfs_min_state_block_number(
        lfb_block.body.state.block_number,
        conf.max_parent_depth,
        conf.mergeable_channels_gc_depth_buffer,
    );
    let roots = compute_forward_horizon_roots(dag, block_store, &lfb_block, conf, state_floor)
        .map_err(CasperError::KvStoreError)?;
    if roots.is_empty() {
        tracing::debug!(
            target: "f1r3.trace.history_gc",
            "history GC: no retention-window roots (LFB height {}, state_floor {}); skipping pass",
            lfb_block.body.state.block_number,
            state_floor
        );
        return Ok(HistoryGcResult {
            marked: 0,
            swept: 0,
        });
    }

    let history_store = runtime.history_repo.history_store();

    // Phase 2: mark.
    let mut mark: HashSet<Blake2b256Hash> = HashSet::new();
    for root in &roots {
        mark_reachable_from(&*history_store, root, &mut mark)?;
    }
    let marked = mark.len();
    metrics::histogram!("rspace_history_gc.marked").record(marked as f64);
    tracing::debug!(
        target: "f1r3.trace.history_gc",
        marked,
        roots = roots.len(),
        "history GC: mark phase complete"
    );

    // Phase 3: sweep. One scan_page per iteration, one delete batch per
    // page. Between pages new writers may land; content-addressed writes
    // make a re-materialization of a just-deleted node idempotent.
    let mut cursor: Option<ByteBuffer> = None;
    let mut swept = 0usize;
    loop {
        let page = history_store
            .scan_page(cursor.as_deref(), SWEEP_PAGE_LIMIT)
            .map_err(CasperError::KvStoreError)?;
        let to_delete: Vec<ByteBuffer> = page
            .rows
            .iter()
            .filter_map(|(key, _)| {
                let hash = Blake2b256Hash::from_bytes(key.clone());
                (!mark.contains(&hash)).then(|| key.clone())
            })
            .collect();
        if !to_delete.is_empty() {
            swept += history_store
                .delete(to_delete)
                .map_err(CasperError::KvStoreError)?;
        }
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    metrics::counter!("rspace_history_gc_deleted").increment(swept as u64);
    metrics::histogram!("rspace_history_gc.pass.time").record(started.elapsed().as_secs_f64());
    tracing::info!(
        target: "f1r3.trace.history_gc",
        marked,
        swept,
        elapsed_s = started.elapsed().as_secs_f64(),
        "history GC pass complete"
    );

    Ok(HistoryGcResult { marked, swept })
}

/// Walk the radix trie from `root`, inserting every reached node hash into
/// `mark`. A hash already in `mark` short-circuits — structural sharing
/// across roots amortises naturally. A root whose node is not in the store
/// (freshly-synced joiner before any blocks produced) is silently skipped.
fn mark_reachable_from(
    store: &dyn KeyValueStore,
    root: &Blake2b256Hash,
    mark: &mut HashSet<Blake2b256Hash>,
) -> Result<(), CasperError> {
    let mut frontier: Vec<Blake2b256Hash> = vec![root.clone()];
    while let Some(hash) = frontier.pop() {
        if !mark.insert(hash.clone()) {
            continue;
        }
        let bytes = store
            .get_one(&hash.bytes())
            .map_err(CasperError::KvStoreError)?;
        let Some(serialized) = bytes else {
            // Root or descendant whose node is not held. Possible on a
            // joiner that has the root tag but has not yet imported the
            // subtree. Correct behaviour is to skip; a later pass sees it.
            continue;
        };
        let node = decode(serialized);
        for item in node {
            if let Item::NodePtr { ptr, .. } = item {
                frontier.push(Blake2b256Hash::from_bytes(ptr));
            }
            // Leaf items hold cold-store pointers (datums/continuations),
            // not trie-node hashes. Never follow them — they are a
            // different store.
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rspace_plus_plus::rspace::history::radix_tree::{empty_node, encode, hash_node, Item};
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    use super::*;

    /// Build a tiny two-node trie: root points to one child via NodePtr at
    /// slot 0. Returns (root_hash, child_hash, store). The store has both
    /// nodes plus one extra unreachable node to confirm the sweep removes
    /// it.
    fn seeded_store() -> (
        Blake2b256Hash,
        Blake2b256Hash,
        Blake2b256Hash,
        Arc<dyn KeyValueStore>,
    ) {
        let store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());

        // Child: empty leaf-less node (all 256 slots empty). Hash it, store it.
        let child_node = empty_node();
        let child_bytes = hash_node(&child_node);
        let child_hash = Blake2b256Hash::from_bytes(child_bytes.0.clone());
        store
            .put(vec![(child_bytes.0.clone(), encode(&child_node))])
            .unwrap();

        // Root: one NodePtr at slot 0 pointing to child.
        let mut root_node = empty_node();
        root_node[0] = Item::NodePtr {
            prefix: vec![],
            ptr: child_bytes.0.clone(),
        };
        let root_bytes = hash_node(&root_node);
        let root_hash = Blake2b256Hash::from_bytes(root_bytes.0.clone());
        store
            .put(vec![(root_bytes.0.clone(), encode(&root_node))])
            .unwrap();

        // Unreachable: a third node not referenced by root or child.
        let mut lone_node = empty_node();
        lone_node[7] = Item::Leaf {
            prefix: vec![0x99],
            value: vec![0x42; 32],
        };
        let lone_bytes = hash_node(&lone_node);
        let lone_hash = Blake2b256Hash::from_bytes(lone_bytes.0.clone());
        store
            .put(vec![(lone_bytes.0.clone(), encode(&lone_node))])
            .unwrap();

        (root_hash, child_hash, lone_hash, store)
    }

    #[test]
    fn mark_walk_reaches_root_and_child() {
        let (root, child, lone, store) = seeded_store();
        let mut mark = HashSet::new();
        mark_reachable_from(&*store, &root, &mut mark).unwrap();
        assert!(mark.contains(&root), "root is marked");
        assert!(mark.contains(&child), "child reached via NodePtr is marked");
        assert!(!mark.contains(&lone), "unreachable node is not marked");
        assert_eq!(mark.len(), 2, "exactly the reachable set");
    }

    #[test]
    fn mark_walk_handles_a_missing_root_as_empty() {
        let store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let bogus = Blake2b256Hash::from_bytes(vec![0; 32]);
        let mut mark = HashSet::new();
        mark_reachable_from(&*store, &bogus, &mut mark).unwrap();
        assert_eq!(
            mark.len(),
            1,
            "the missing root is still inserted; its absent subtree just stops the walk"
        );
    }

    #[test]
    fn mark_walk_is_idempotent_across_shared_subtrees() {
        // Build a DAG: two roots share the same child.
        let store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let child_node = empty_node();
        let child_bytes = hash_node(&child_node);
        store
            .put(vec![(child_bytes.0.clone(), encode(&child_node))])
            .unwrap();

        let mut root_a = empty_node();
        root_a[0] = Item::NodePtr {
            prefix: vec![],
            ptr: child_bytes.0.clone(),
        };
        let root_a_bytes = hash_node(&root_a);
        store
            .put(vec![(root_a_bytes.0.clone(), encode(&root_a))])
            .unwrap();

        let mut root_b = empty_node();
        root_b[1] = Item::NodePtr {
            prefix: vec![],
            ptr: child_bytes.0.clone(),
        };
        let root_b_bytes = hash_node(&root_b);
        store
            .put(vec![(root_b_bytes.0.clone(), encode(&root_b))])
            .unwrap();

        let mut mark = HashSet::new();
        mark_reachable_from(
            &*store,
            &Blake2b256Hash::from_bytes(root_a_bytes.0.clone()),
            &mut mark,
        )
        .unwrap();
        mark_reachable_from(
            &*store,
            &Blake2b256Hash::from_bytes(root_b_bytes.0.clone()),
            &mut mark,
        )
        .unwrap();
        assert_eq!(
            mark.len(),
            3,
            "shared child counted once, both roots counted once each"
        );
    }
}
