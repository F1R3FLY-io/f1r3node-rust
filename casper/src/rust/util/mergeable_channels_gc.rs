use std::collections::HashSet;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block_hash::BlockHash;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::casper::CasperShardConf;
use crate::rust::finality::floor::Floor;

mod job;
#[cfg(any(test, feature = "test-utils"))]
pub use job::collect_garbage_with;
pub use job::{collect_garbage, GcProgress, GcSweep};

/// Conditions 1 and 2 of `is_safe_to_delete`, plus the "has children" half of
/// condition 3 — everything that can be decided about a candidate WITHOUT
/// consulting the moved-past set. Shared with the caller that bounds
/// the ancestor walk: a candidate that fails here never reaches the
/// ancestor-membership test, so it must never be allowed to pull that walk's
/// depth down either. One predicate, so the two can't drift apart.
fn reaches_ancestor_check(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    floor: &Floor,
    casper_shard_conf: &CasperShardConf,
) -> Result<bool, KvStoreError> {
    // 1. Check if block is finalized. An orphan on a losing fork never
    //    satisfies this — permanently, not just for this pass — so the pass
    //    drops it from the retry queue, and finalization queues it again if
    //    it is ever finalized later. It never blocks any other block's own
    //    check from succeeding.
    if !dag.is_finalized(block_hash)? {
        return Ok(false);
    }

    // 2. Depth is measured from the FLOOR, not from the tip. The data being
    //    deleted is what merges need to compute number-channel diffs, and the
    //    floor is what bounds how deep a merge reaches: the base's own lineage
    //    walk stops at it, and a base never sits below it. The floor trails the
    //    tip by the time mutual citation takes (~3 blocks healthy, >100 during
    //    the observed pacification stall), so a tip-anchored bound put the
    //    whole span between the floor and `tip - max_allowed_depth` inside the
    //    deletable set while merges still needed it. Anchoring on the floor is
    //    strictly more conservative: the floor never leads the tip, so this can
    //    only delete less than before, never more.
    let block_meta = dag.lookup_unsafe(block_hash)?;
    let depth_from_floor = floor.block_number - block_meta.block_number;
    let max_allowed_depth = (casper_shard_conf.max_parent_depth as i64)
        + (casper_shard_conf.mergeable_channels_gc_depth_buffer as i64);

    if depth_from_floor <= max_allowed_depth {
        return Ok(false);
    }

    // Every validator must have moved past this block — but that is only
    // checkable once it has children at all.
    dag.has_children(block_hash)
}

/// Check if a block's mergeable data is safe to delete.
fn is_safe_to_delete(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    floor: &Floor,
    casper_shard_conf: &CasperShardConf,
    moved_past: Option<&HashSet<BlockHash>>,
) -> Result<bool, KvStoreError> {
    if !reaches_ancestor_check(dag, block_hash, floor, casper_shard_conf)? {
        return Ok(false);
    }

    // `moved_past` is computed once per pass, not once per candidate. No
    // latest messages means nothing is known to have moved past this block,
    // which is a reason to keep the data rather than to delete it.
    let Some(moved_past) = moved_past else {
        return Ok(false);
    };

    Ok(moved_past.contains(block_hash))
}

/// The blocks every validator has moved past, down to `min_height`: the
/// deepest block on every validator's strict main-parent lineage, and its
/// ancestors through any parent. A block merged in as a secondary parent is
/// as far behind every validator as the main-chain block that merged it.
/// Computed once per pass; heights and parents are read through the DAG, from
/// disk below the memory window.
///
/// Stops the walk once it reaches `min_height` — the lowest height any
/// candidate in `pending` can have. A candidate's own height is always >=
/// that minimum by construction, so nothing that could ever be tested
/// against this set sits below where the walk stops.
///
/// This is NOT the retention-window ceiling: unlike the old watermark, which
/// only ever pointed at heights already fully resolved, `pending` here can
/// hold a block arbitrarily far below the current window — an orphan that
/// has sat there for the chain's whole life. Bounding by the window instead
/// of by `pending`'s own floor would make such a block un-checkable forever,
/// silently defeating the sweep's entire reason for retrying it. `min_height`
/// is derived by the caller from `reaches_ancestor_check`-eligible candidates
/// only, so a permanently-unfinalized orphan — refused before this set is
/// ever consulted — does not drag the walk down either.
fn moved_past_by_every_validator(
    dag: &KeyValueDagRepresentation,
    min_height: i64,
) -> Result<Option<HashSet<BlockHash>>, KvStoreError> {
    // Validators sharing the same latest message (common on a healthy,
    // synchronized chain) would otherwise seed the frontier with that same
    // lineage once per validator instead of once total.
    //
    // A slot holding a block its validator never signed is the genesis
    // placeholder, which has no main parent: counting it collapses the anchor
    // search and the pass keeps everything.
    let mut latest_messages: HashSet<BlockHash> = HashSet::new();
    for (validator, hash) in dag.latest_message_hashes() {
        if dag.own_testimony(&validator, &hash)?.is_some() {
            latest_messages.insert(hash);
        }
    }

    // The walks below take infallible lookups, where `None` means "not held".
    // A failed read is not that: the first one is kept and fails the pass, so
    // nothing is collected on a guess.
    let read_error = std::cell::RefCell::new(None);

    // Main-parent chains are linear, so the intersection of N strict-ancestor
    // paths is just the path below their deepest common point.
    // `common_strict_ancestors` below is that intersection, kept as the
    // differential oracle for the anchor.
    let anchor = deepest_common_strict_ancestor(
        latest_messages,
        |block_hash| recorded(&read_error, dag.main_parent(block_hash)),
        |block_hash| recorded(&read_error, dag.block_number(block_hash)),
    );
    if let Some(error) = read_error.take() {
        return Err(error);
    }
    let Some(anchor) = anchor else {
        return Ok(None);
    };

    let ancestry = ancestry_down_to(
        anchor,
        |block_hash| recorded(&read_error, dag.block_number(block_hash)),
        |block_hash| dag.lookup_unsafe(block_hash).map(|meta| meta.parents),
        min_height,
    )?;
    match read_error.take() {
        Some(error) => Err(error),
        None => Ok(Some(ancestry)),
    }
}

fn recorded<T>(
    read_error: &std::cell::RefCell<Option<KvStoreError>>,
    result: Result<Option<T>, KvStoreError>,
) -> Option<T> {
    result.unwrap_or_else(|error| {
        read_error.borrow_mut().get_or_insert(error);
        None
    })
}

/// The deepest block that is a strict main-chain ancestor of every latest
/// message, or `None` when there is no such block.
///
/// Each walk starts at a latest message's main parent rather than at the message
/// itself, which is what makes the result strict: a validator sitting on a block
/// has not moved past it, so that block must not qualify.
///
/// Convergence advances only the highest frontier entries, so the work is
/// proportional to how far the validators have diverged — a few blocks when
/// healthy, and bounded by the stall when not — rather than to the chain length.
fn deepest_common_strict_ancestor(
    latest_messages: impl IntoIterator<Item = BlockHash>,
    main_parent: impl Fn(&BlockHash) -> Option<BlockHash>,
    height_of: impl Fn(&BlockHash) -> Option<i64>,
) -> Option<BlockHash> {
    let mut frontier: Vec<BlockHash> = latest_messages
        .into_iter()
        .map(|latest_message| main_parent(&latest_message))
        .collect::<Option<Vec<_>>>()?;

    if frontier.is_empty() {
        return None;
    }

    loop {
        if frontier.iter().all(|hash| *hash == frontier[0]) {
            return frontier.into_iter().next();
        }

        // A height this walk cannot read leaves the frontier unable to advance,
        // so refuse rather than spin.
        let heights: Vec<i64> = frontier
            .iter()
            .map(&height_of)
            .collect::<Option<Vec<_>>>()?;
        let deepest_to_advance = *heights.iter().max()?;

        for (slot, height) in frontier.iter_mut().zip(heights) {
            if height == deepest_to_advance {
                *slot = main_parent(&*slot)?;
            }
        }
    }
}

/// The anchor and its ancestors through any parent, not walking past a block
/// at or below `lowest_needed` — no candidate sits below that, so nothing
/// deeper can change an answer. `height_of` is `None` for a block this node
/// does not hold; `parents_of` is read only for blocks the walk expands.
///
/// A block the node does not hold is in no candidate set, so it is kept in
/// the result but not expanded: stopping there can only keep data, never
/// delete it.
fn ancestry_down_to<E>(
    anchor: BlockHash,
    height_of: impl Fn(&BlockHash) -> Option<i64>,
    parents_of: impl Fn(&BlockHash) -> Result<Vec<BlockHash>, E>,
    lowest_needed: i64,
) -> Result<HashSet<BlockHash>, E> {
    let expands = |block_hash: &BlockHash| height_of(block_hash).is_some_and(|h| h > lowest_needed);
    let mut frontier: Vec<BlockHash> = expands(&anchor)
        .then(|| anchor.clone())
        .into_iter()
        .collect();
    let mut ancestry = HashSet::from([anchor]);

    while let Some(block_hash) = frontier.pop() {
        for parent in parents_of(&block_hash)? {
            if ancestry.insert(parent.clone()) && expands(&parent) {
                frontier.push(parent);
            }
        }
    }

    Ok(ancestry)
}

/// Every validator's strict main-parent ancestors, intersected: the main
/// chain below the anchor `deepest_common_strict_ancestor` finds. Slow, and
/// obviously correct.
///
/// Kept as the oracle for the differential property in the tests, which is a
/// standing constraint rather than a one-time proof — it is what tells anyone
/// editing the convergence loop that they have changed the anchor.
#[cfg(test)]
fn common_strict_ancestors(
    latest_messages: impl IntoIterator<Item = BlockHash>,
    main_parent: impl Fn(&BlockHash) -> Option<BlockHash>,
) -> Option<HashSet<BlockHash>> {
    latest_messages
        .into_iter()
        .map(|latest_message| {
            let mut ancestors = HashSet::new();
            let mut current = latest_message;
            while let Some(parent) = main_parent(&current) {
                if !ancestors.insert(parent.clone()) {
                    break;
                }
                current = parent;
            }
            ancestors
        })
        .reduce(|common, ancestors| common.intersection(&ancestors).cloned().collect())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use models::rust::block_metadata::BlockMetadata;
    use parking_lot::RwLock as PlRwLock;
    use proptest::prelude::*;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    // -- common_strict_ancestors --------------------------------------------

    fn named_hash(value: &'static [u8]) -> BlockHash { BlockHash::from_static(value) }

    #[test]
    fn intersects_strict_main_chain_ancestors() {
        use std::collections::HashMap;

        let a = named_hash(b"a");
        let b = named_hash(b"b");
        let c = named_hash(b"c");
        let genesis = named_hash(b"genesis");

        // validator 1's lineage: c -> b -> a -> genesis
        // validator 2's lineage: c -> b -> genesis (diverges below b)
        let parents: HashMap<BlockHash, BlockHash> =
            [(c.clone(), b.clone()), (b.clone(), genesis.clone())]
                .into_iter()
                .collect();
        let parents2: HashMap<BlockHash, BlockHash> =
            [(a.clone(), genesis.clone())].into_iter().collect();

        let ancestors = common_strict_ancestors([c.clone(), c.clone()], |h| {
            parents.get(h).or_else(|| parents2.get(h)).cloned()
        })
        .expect("non-empty input yields a set");

        assert!(ancestors.contains(&b));
        assert!(ancestors.contains(&genesis));
    }

    #[test]
    fn excludes_latest_messages_from_strict_ancestors() {
        let tip = named_hash(b"tip");
        let parent = named_hash(b"parent");
        let parents: std::collections::HashMap<BlockHash, BlockHash> =
            [(tip.clone(), parent.clone())].into_iter().collect();

        let ancestors = common_strict_ancestors([tip.clone()], |h| parents.get(h).cloned())
            .expect("non-empty input yields a set");

        assert!(
            !ancestors.contains(&tip),
            "the latest message itself is not its own ancestor"
        );
        assert!(ancestors.contains(&parent));
    }

    #[test]
    fn returns_none_without_latest_messages() {
        assert!(common_strict_ancestors(Vec::new(), |_: &BlockHash| None).is_none());
    }

    // -- the anchor walk ------------------------------------------------------

    /// genesis(0) <- common(1) <- {left(2), right(2)}, a two-validator fork.
    fn forked_chain() -> (
        std::collections::HashMap<BlockHash, BlockHash>,
        std::collections::HashMap<BlockHash, i64>,
    ) {
        let parents = std::collections::HashMap::from([
            (named_hash(b"common"), named_hash(b"genesis")),
            (named_hash(b"left"), named_hash(b"common")),
            (named_hash(b"right"), named_hash(b"common")),
        ]);
        let heights = std::collections::HashMap::from([
            (named_hash(b"genesis"), 0),
            (named_hash(b"common"), 1),
            (named_hash(b"left"), 2),
            (named_hash(b"right"), 2),
        ]);
        (parents, heights)
    }

    #[test]
    fn converges_on_the_deepest_block_every_validator_is_past() {
        let (parents, heights) = forked_chain();
        let anchor = deepest_common_strict_ancestor(
            [named_hash(b"left"), named_hash(b"right")],
            |h| parents.get(h).cloned(),
            |h| heights.get(h).copied(),
        );
        assert_eq!(anchor, Some(named_hash(b"common")));
    }

    #[test]
    fn a_validator_sitting_on_a_block_keeps_it_out_of_the_answer() {
        let (parents, heights) = forked_chain();
        // One validator is still ON `common`, so `common` is not strictly
        // behind everyone and must not be admitted.
        let anchor = deepest_common_strict_ancestor(
            [named_hash(b"left"), named_hash(b"common")],
            |h| parents.get(h).cloned(),
            |h| heights.get(h).copied(),
        );
        assert_eq!(anchor, Some(named_hash(b"genesis")));
    }

    #[test]
    fn returns_none_when_a_latest_message_has_no_parent() {
        let (parents, heights) = forked_chain();
        assert!(deepest_common_strict_ancestor(
            [named_hash(b"left"), named_hash(b"genesis")],
            |h| parents.get(h).cloned(),
            |h| heights.get(h).copied(),
        )
        .is_none());
    }

    /// genesis(0) <- a(1) <- b(2) <- c(3), and side(2) on a, merged by c as
    /// its second parent.
    fn merged_chain() -> std::collections::HashMap<BlockHash, (i64, Vec<BlockHash>)> {
        std::collections::HashMap::from([
            (named_hash(b"genesis"), (0, vec![])),
            (named_hash(b"a"), (1, vec![named_hash(b"genesis")])),
            (named_hash(b"b"), (2, vec![named_hash(b"a")])),
            (named_hash(b"side"), (2, vec![named_hash(b"a")])),
            (
                named_hash(b"c"),
                (3, vec![named_hash(b"b"), named_hash(b"side")]),
            ),
        ])
    }

    fn walk(
        dag: &std::collections::HashMap<BlockHash, (i64, Vec<BlockHash>)>,
        anchor: BlockHash,
        lowest_needed: i64,
    ) -> Result<HashSet<BlockHash>, String> {
        ancestry_down_to(
            anchor,
            |h| dag.get(h).map(|entry| entry.0),
            |h| {
                dag.get(h)
                    .map(|entry| entry.1.clone())
                    .ok_or_else(|| format!("{h:?} has no metadata"))
            },
            lowest_needed,
        )
    }

    #[test]
    fn the_ancestry_walk_follows_every_parent() {
        let dag = merged_chain();
        let ancestry = walk(&dag, named_hash(b"c"), 0).expect("walk");
        assert!(
            ancestry.contains(&named_hash(b"side")),
            "a block merged as a second parent is behind its merger"
        );
        assert!(ancestry.contains(&named_hash(b"genesis")));
    }

    #[test]
    fn the_ancestry_walk_stops_at_the_lowest_block_still_pending() {
        let dag = merged_chain();
        let ancestry = walk(&dag, named_hash(b"c"), 1).expect("walk");
        assert!(ancestry.contains(&named_hash(b"a")));
        assert!(
            !ancestry.contains(&named_hash(b"genesis")),
            "nothing below the lowest pending candidate needs walking"
        );
    }

    /// `deep` is held, but only `side` cites it, and this node does not hold
    /// `side`.
    #[test]
    fn a_held_block_reachable_only_through_an_unheld_parent_is_kept() {
        let mut dag = merged_chain();
        dag.insert(named_hash(b"deep"), (1, vec![named_hash(b"genesis")]));
        dag.remove(&named_hash(b"side"));
        let ancestry = walk(&dag, named_hash(b"c"), 0).expect("walk");
        assert!(ancestry.contains(&named_hash(b"side")));
        assert!(
            !ancestry.contains(&named_hash(b"deep")),
            "nothing is known about what an unheld block descends from, so its \
             ancestors stay out of the set and keep their data"
        );
    }

    #[test]
    fn a_held_block_without_metadata_fails_the_walk() {
        let dag = merged_chain();
        let result = ancestry_down_to(
            named_hash(b"c"),
            |h| dag.get(h).map(|entry| entry.0),
            |h| match h == &named_hash(b"side") {
                true => Err("side is held but has no metadata".to_string()),
                false => Ok(dag[h].1.clone()),
            },
            0,
        );
        assert!(result.is_err());
    }

    prop_compose! {
        /// A random DAG: each block picks one to three earlier blocks as its
        /// parents, the first being its main parent, so shapes from a single
        /// straight chain to a wide, merged fan are all reachable, and latest
        /// messages may sit anywhere in it — including on genesis, or on one
        /// another's ancestors.
        fn merged_dag()(
            parent_picks in prop::collection::vec(prop::collection::vec(0usize..64, 1..4), 1..16),
            latest_picks in prop::collection::vec(0usize..64, 1..5),
        ) -> (
            std::collections::HashMap<BlockHash, (i64, Vec<BlockHash>)>,
            Vec<BlockHash>,
        ) {
            fn block(index: usize) -> BlockHash {
                BlockHash::from(format!("b{}", index).into_bytes())
            }

            let count = parent_picks.len() + 1;
            let mut dag = std::collections::HashMap::from([(block(0), (0i64, Vec::new()))]);

            for (offset, picks) in parent_picks.iter().enumerate() {
                let child = offset + 1;
                let mut parents: Vec<BlockHash> = Vec::new();
                for pick in picks {
                    let parent = block(pick % child);
                    if !parents.contains(&parent) {
                        parents.push(parent);
                    }
                }
                let height = parents.iter().map(|p| dag[p].0).max().expect("one parent") + 1;
                dag.insert(block(child), (height, parents));
            }

            let latest = latest_picks.iter().map(|pick| block(pick % count)).collect();
            (dag, latest)
        }
    }

    /// Every block each latest message descends from through any parent,
    /// the message itself excluded, intersected.
    fn strict_ancestors_of_every(
        dag: &std::collections::HashMap<BlockHash, (i64, Vec<BlockHash>)>,
        latest: &[BlockHash],
    ) -> HashSet<BlockHash> {
        latest
            .iter()
            .map(|latest_message| {
                let mut ancestors = HashSet::new();
                let mut frontier = dag[latest_message].1.clone();
                while let Some(block_hash) = frontier.pop() {
                    if ancestors.insert(block_hash.clone()) {
                        frontier.extend(dag[&block_hash].1.iter().cloned());
                    }
                }
                ancestors
            })
            .reduce(|common, ancestors| common.intersection(&ancestors).cloned().collect())
            .expect("the generator gives at least one latest message")
    }

    proptest! {
        /// The anchor is the deepest block on every validator's strict main
        /// chain: walking the main chain down from it admits exactly what
        /// intersecting every validator's strict main-parent lineage admits.
        ///
        /// Membership is compared rather than the containers themselves. A
        /// validator sitting on genesis has no strict ancestors, which the
        /// intersection reports as an empty set and the walk reports as no
        /// anchor at all; both admit nothing, which is what the predicate reads.
        #[test]
        fn the_anchor_is_the_deepest_block_on_every_main_chain((dag, latest) in merged_dag()) {
            let parent_of = |block_hash: &BlockHash| dag[block_hash].1.first().cloned();
            let height_of = |block_hash: &BlockHash| dag.get(block_hash).map(|entry| entry.0);

            let intersected = common_strict_ancestors(latest.clone(), parent_of);
            let walked = deepest_common_strict_ancestor(latest.clone(), parent_of, height_of)
                .map(|anchor| {
                    std::iter::successors(Some(anchor), |h| parent_of(h)).collect::<HashSet<_>>()
                });

            for candidate in dag.keys() {
                let by_intersection =
                    intersected.as_ref().is_some_and(|set| set.contains(candidate));
                let by_walk = walked.as_ref().is_some_and(|set| set.contains(candidate));
                prop_assert_eq!(by_intersection, by_walk, "disagreement on {:?}", candidate);
            }
        }

        /// What makes deletion safe: every block the walk admits is a strict
        /// ancestor, through some parent, of every latest message.
        #[test]
        fn every_admitted_block_is_behind_every_latest_message((dag, latest) in merged_dag()) {
            let parent_of = |block_hash: &BlockHash| dag[block_hash].1.first().cloned();
            let height_of = |block_hash: &BlockHash| dag.get(block_hash).map(|entry| entry.0);

            if let Some(anchor) = deepest_common_strict_ancestor(latest.clone(), parent_of, height_of) {
                let admitted = walk(&dag, anchor, 0).expect("walk");
                let behind_everyone = strict_ancestors_of_every(&dag, &latest);
                for block_hash in &admitted {
                    prop_assert!(
                        behind_everyone.contains(block_hash),
                        "{:?} admitted, but some latest message does not descend from it",
                        block_hash
                    );
                }
            }
        }
    }

    // -- is_safe_to_delete ----------------------------------------------------

    fn hash(n: u8) -> Bytes { Bytes::from(vec![n; 32]) }

    /// A linear finalized chain 0..=TOP by one validator, whose latest message
    /// is the tip. Everything `is_safe_to_delete` reads is populated: heights
    /// (so `latest_block_number` is real), main parents, children, the
    /// finalized set, and the latest-message map.
    const TOP: u8 = 20;

    fn linear_chain_dag() -> KeyValueDagRepresentation {
        let store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut bms = BlockMetadataStore::new(store);
        let validator = Bytes::from(vec![0xEEu8; 65]);

        let mut dag_set = imbl::HashSet::new();
        let mut block_number_map = imbl::HashMap::new();
        let mut main_parent_map = imbl::HashMap::new();
        let mut child_map: imbl::HashMap<Bytes, imbl::HashSet<Bytes>> = imbl::HashMap::new();
        let mut height_map: imbl::OrdMap<i64, imbl::HashSet<Bytes>> = imbl::OrdMap::new();
        let mut finalized_blocks_set = imbl::HashSet::new();

        for n in 0..=TOP {
            let h = hash(n);
            dag_set.insert(h.clone());
            block_number_map.insert(h.clone(), n as i64);
            finalized_blocks_set.insert(h.clone());
            let mut at_height = imbl::HashSet::new();
            at_height.insert(h.clone());
            height_map.insert(n as i64, at_height);

            let parents = if n == 0 {
                Vec::new()
            } else {
                let parent = hash(n - 1);
                main_parent_map.insert(h.clone(), parent.clone());
                let mut kids = child_map.get(&parent).cloned().unwrap_or_default();
                kids.insert(h.clone());
                child_map.insert(parent.clone(), kids);
                vec![parent]
            };

            bms.add(BlockMetadata {
                block_hash: h.clone(),
                parents,
                sender: validator.clone(),
                justifications: vec![],
                weight_map: BTreeMap::new(),
                block_number: n as i64,
                sequence_number: n as i32,
                invalid: false,
                directly_finalized: true,
                finalized: true,
                fault_tolerance_value: 1.0,
                merge_base: Bytes::new(),
            })
            .expect("add metadata");
        }

        let mut latest_messages_map = imbl::HashMap::new();
        latest_messages_map.insert(validator, hash(TOP));

        block_storage::rust::dag::block_dag_key_value_storage::DagFixture {
            history: None,
            view_sequence: u64::MAX,
            view_max_height: None,
            dag_set,
            latest_messages_map,
            child_map,
            height_map,
            block_number_map,
            main_parent_map,
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: hash(TOP),
            finalized_blocks_set,
            block_metadata_index: Arc::new(PlRwLock::new(bms)),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            lifecycle: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(
                ),
            )),
            carrier_index: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::carrier_index::CarrierIndex::in_memory(),
            )),
        }
        .build()
    }

    /// max_allowed_depth = max_parent_depth + gc buffer = 4.
    pub(super) fn conf() -> CasperShardConf {
        let mut conf = CasperShardConf::new();
        conf.max_parent_depth = 3;
        conf.mergeable_channels_gc_depth_buffer = 1;
        conf
    }

    pub(super) fn floor_at(n: u8) -> Floor {
        Floor {
            hash: hash(n),
            block_number: n as i64,
        }
    }

    fn is_safe_to_delete_at_floor(
        dag: &KeyValueDagRepresentation,
        block_hash: &BlockHash,
        floor: &Floor,
        conf: &CasperShardConf,
    ) -> Result<bool, KvStoreError> {
        // Derived from the DAG exactly as `collect_garbage` does, so the depth
        // clause is exercised against a real citation set rather than a stub
        // that could refuse for the wrong reason. Bounded at this single
        // candidate's own height — the same rule `collect_garbage` applies
        // to a whole `pending` set collapses to "this one height" here.
        let candidate_height = dag.lookup_unsafe(block_hash)?.block_number;
        let moved_past = moved_past_by_every_validator(dag, candidate_height)?;
        is_safe_to_delete(dag, block_hash, floor, conf, moved_past.as_ref())
    }

    /// THE regression. Mergeable data serves merges, and a merge reads it for
    /// the blocks above its BASE — the floor. A block ABOVE the floor is inside
    /// that span no matter how far the tip has run ahead, so its data must
    /// survive. Measuring depth from the tip deletes exactly the span between
    /// the floor and `tip - max_allowed_depth`, which is the data floor-based
    /// merges are still reading, and the floor can trail the tip by a hundred
    /// blocks during a stall.
    #[test]
    fn a_block_above_the_floor_is_never_collected_however_far_the_tip_has_run() {
        let dag = linear_chain_dag();
        let conf = conf();
        // Tip is TOP + 1 = 21, so block 12 is 9 below it — past the 4-block
        // allowance on the tip clock — but 2 ABOVE the floor at 10.
        assert_eq!(dag.latest_block_number(), TOP as i64 + 1);
        assert!(
            !is_safe_to_delete_at_floor(&dag, &hash(12), &floor_at(10), &conf)
                .expect("safety check"),
            "block 12 sits above the floor at 10, so a merge based on that floor \
             still reads its mergeable data. Anchored on the tip it reads as \
             depth 9 and is collected, and the merge then fails to find history \
             it needs",
        );
    }

    /// The discriminator against a never-collect over-correction: below the
    /// floor by more than the allowance, the data can no longer be reached by
    /// any merge and must be released.
    #[test]
    fn a_block_far_below_the_floor_is_still_collected() {
        let dag = linear_chain_dag();
        let conf = conf();
        assert!(
            is_safe_to_delete_at_floor(&dag, &hash(5), &floor_at(10), &conf).expect("safety check"),
            "block 5 is 5 below the floor at 10, past the 4-block allowance, so \
             no merge can still need it and holding it forever leaks",
        );
    }

    /// A bonded validator that never proposed has the genesis placeholder in
    /// its slot, and genesis has no main parent, so the anchor search
    /// short-circuits and every candidate is refused.
    #[test]
    fn a_validator_that_never_proposed_does_not_stop_collection() {
        let mut dag = linear_chain_dag();
        dag.fixture_mut()
            .latest_messages_map
            .insert(Bytes::from(vec![0x07u8; 65]), hash(0));
        let conf = conf();
        assert!(
            is_safe_to_delete_at_floor(&dag, &hash(5), &floor_at(10), &conf).expect("safety check"),
            "a slot holding a block its validator never signed is bookkeeping, \
             not a position anything moved past; counting it collects nothing \
             for as long as that validator stays silent",
        );
    }

    /// The allowance is measured on the floor clock, so the boundary is exact:
    /// one block closer than the discriminator above must be retained.
    #[test]
    fn the_allowance_boundary_is_measured_from_the_floor() {
        let dag = linear_chain_dag();
        let conf = conf();
        assert!(
            !is_safe_to_delete_at_floor(&dag, &hash(6), &floor_at(10), &conf)
                .expect("safety check"),
            "block 6 is exactly the 4-block allowance below the floor — retained",
        );
    }

    /// Finality is still a precondition: an unfinalized block is never
    /// collected regardless of where the floor sits.
    #[test]
    fn an_unfinalized_block_is_never_collected() {
        let mut dag = linear_chain_dag();
        dag.fixture_mut().finalized_blocks_set.remove(&hash(5));
        {
            let mut bms = dag.block_metadata_index.write();
            let mut meta = bms.get(&hash(5)).expect("lookup").expect("metadata");
            meta.finalized = false;
            meta.directly_finalized = false;
            bms.add(meta).expect("re-add metadata");
        }
        let conf = conf();
        assert!(
            !is_safe_to_delete_at_floor(&dag, &hash(5), &floor_at(10), &conf)
                .expect("safety check"),
            "an unfinalized block's data may still be needed by a branch that wins",
        );
    }

    /// The spine of `linear_chain_dag`, plus a two-block branch off height 4 that
    /// a second validator sits on.
    ///
    /// `linear_chain_dag` has one validator on one chain, so every block is an
    /// ancestor of the only latest message and the citation clause cannot be
    /// observed rejecting anything. Here validator B is on `side6`, so the
    /// deepest block both validators have moved past is block 4 — everything
    /// above it on the spine is still cited by A but not by B.
    fn forked_chain_dag() -> KeyValueDagRepresentation {
        const FORK_POINT: u8 = 4;
        let store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut bms = BlockMetadataStore::new(store);
        let validator_a = Bytes::from(vec![0xEEu8; 65]);
        let validator_b = Bytes::from(vec![0xDDu8; 65]);
        let side = |n: u8| Bytes::from(format!("side{}", n).into_bytes());

        let mut dag_set = imbl::HashSet::new();
        let mut block_number_map = imbl::HashMap::new();
        let mut main_parent_map = imbl::HashMap::new();
        let mut child_map: imbl::HashMap<Bytes, imbl::HashSet<Bytes>> = imbl::HashMap::new();
        let mut height_map: imbl::OrdMap<i64, imbl::HashSet<Bytes>> = imbl::OrdMap::new();
        let mut finalized_blocks_set = imbl::HashSet::new();

        let record = |hash: Bytes,
                      parent: Option<Bytes>,
                      height: i64,
                      sender: Bytes,
                      bms: &mut BlockMetadataStore,
                      dag_set: &mut imbl::HashSet<Bytes>,
                      block_number_map: &mut imbl::HashMap<Bytes, i64>,
                      main_parent_map: &mut imbl::HashMap<Bytes, Bytes>,
                      child_map: &mut imbl::HashMap<Bytes, imbl::HashSet<Bytes>>,
                      height_map: &mut imbl::OrdMap<i64, imbl::HashSet<Bytes>>,
                      finalized: &mut imbl::HashSet<Bytes>| {
            dag_set.insert(hash.clone());
            block_number_map.insert(hash.clone(), height);
            finalized.insert(hash.clone());
            let mut at_height = height_map.get(&height).cloned().unwrap_or_default();
            at_height.insert(hash.clone());
            height_map.insert(height, at_height);

            let parents = match parent {
                Some(parent) => {
                    main_parent_map.insert(hash.clone(), parent.clone());
                    let mut kids = child_map.get(&parent).cloned().unwrap_or_default();
                    kids.insert(hash.clone());
                    child_map.insert(parent, kids);
                    vec![main_parent_map[&hash].clone()]
                }
                None => Vec::new(),
            };

            bms.add(BlockMetadata {
                block_hash: hash,
                parents,
                sender,
                justifications: vec![],
                weight_map: BTreeMap::new(),
                block_number: height,
                sequence_number: height as i32,
                invalid: false,
                directly_finalized: true,
                finalized: true,
                fault_tolerance_value: 1.0,
                merge_base: Bytes::new(),
            })
            .expect("add metadata");
        };

        for n in 0..=TOP {
            let parent = (n > 0).then(|| hash(n - 1));
            record(
                hash(n),
                parent,
                n as i64,
                validator_a.clone(),
                &mut bms,
                &mut dag_set,
                &mut block_number_map,
                &mut main_parent_map,
                &mut child_map,
                &mut height_map,
                &mut finalized_blocks_set,
            );
        }

        for n in 5..=6u8 {
            let parent = if n == 5 {
                hash(FORK_POINT)
            } else {
                side(n - 1)
            };
            record(
                side(n),
                Some(parent),
                n as i64,
                validator_b.clone(),
                &mut bms,
                &mut dag_set,
                &mut block_number_map,
                &mut main_parent_map,
                &mut child_map,
                &mut height_map,
                &mut finalized_blocks_set,
            );
        }

        let mut latest_messages_map = imbl::HashMap::new();
        latest_messages_map.insert(validator_a, hash(TOP));
        latest_messages_map.insert(validator_b, side(6));

        block_storage::rust::dag::block_dag_key_value_storage::DagFixture {
            history: None,
            view_sequence: u64::MAX,
            view_max_height: None,
            dag_set,
            latest_messages_map,
            child_map,
            height_map,
            block_number_map,
            main_parent_map,
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: hash(TOP),
            finalized_blocks_set,
            block_metadata_index: Arc::new(PlRwLock::new(bms)),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            lifecycle: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(
                ),
            )),
            carrier_index: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::carrier_index::CarrierIndex::in_memory(),
            )),
        }
        .build()
    }

    /// `linear_chain_dag` plus `side3`, a finalized block at height 3 that
    /// block 4 merges in as its second parent: every validator has moved past
    /// it, but not along the main chain.
    fn chain_with_a_merged_side_block() -> (KeyValueDagRepresentation, BlockHash) {
        let mut dag = linear_chain_dag();
        let side = Bytes::from_static(b"side3");
        dag.fixture_mut().dag_set.insert(side.clone());
        dag.fixture_mut().block_number_map.insert(side.clone(), 3);
        dag.fixture_mut()
            .main_parent_map
            .insert(side.clone(), hash(2));
        dag.fixture_mut().finalized_blocks_set.insert(side.clone());
        let mut at_three = dag
            .fixture_mut()
            .height_map
            .get(&3)
            .cloned()
            .unwrap_or_default();
        at_three.insert(side.clone());
        dag.fixture_mut().height_map.insert(3, at_three);
        let mut below = dag
            .fixture_mut()
            .child_map
            .get(&hash(2))
            .cloned()
            .unwrap_or_default();
        below.insert(side.clone());
        dag.fixture_mut().child_map.insert(hash(2), below);
        dag.fixture_mut()
            .child_map
            .insert(side.clone(), std::iter::once(hash(4)).collect());
        {
            let mut bms = dag.block_metadata_index.write();
            bms.add(BlockMetadata {
                block_hash: side.clone(),
                parents: vec![hash(2)],
                sender: Bytes::from(vec![0xDDu8; 65]),
                justifications: vec![],
                weight_map: BTreeMap::new(),
                block_number: 3,
                sequence_number: 1,
                invalid: false,
                directly_finalized: false,
                finalized: true,
                fault_tolerance_value: 1.0,
                merge_base: Bytes::new(),
            })
            .expect("add metadata");
            let mut four = bms.get(&hash(4)).expect("lookup").expect("metadata");
            four.parents.push(side.clone());
            bms.add(four).expect("re-add metadata");
        }
        (dag, side)
    }

    #[test]
    fn a_secondary_parent_this_node_does_not_hold_does_not_fail_the_pass() {
        let dag = linear_chain_dag();
        {
            let mut bms = dag.block_metadata_index.write();
            let mut eight = bms.get(&hash(8)).expect("lookup").expect("metadata");
            eight
                .parents
                .push(Bytes::from_static(b"below-the-restored-band"));
            bms.add(eight).expect("re-add metadata");
        }
        assert!(
            moved_past_by_every_validator(&dag, 5).is_ok(),
            "the walk stops at height 5; a secondary parent of block 8 that this \
             node does not hold cannot be a candidate and must not fail the pass"
        );
    }

    #[test]
    fn a_finalized_block_merged_off_the_main_chain_is_collected_far_below_the_floor() {
        let (dag, side) = chain_with_a_merged_side_block();
        assert!(
            is_safe_to_delete_at_floor(&dag, &side, &floor_at(10), &conf()).expect("safety check"),
            "side3 is finalized, 7 below the floor at 10 and an ancestor of every \
             latest message through block 4, so no merge can still read its data; \
             keeping it forever leaks one block's mergeable data per merged sibling"
        );
    }

    #[test]
    fn below_a_fork_both_validators_are_past_the_block_and_it_collects() {
        let dag = forked_chain_dag();
        assert!(
            is_safe_to_delete_at_floor(&dag, &hash(3), &floor_at(TOP), &conf())
                .expect("safety check"),
            "block 3 is below the fork point, so both validators have moved past it"
        );
    }

    #[test]
    fn above_a_fork_one_validator_is_still_behind_and_the_block_is_kept() {
        let dag = forked_chain_dag();
        // Deep enough and finalized, so only the citation clause can refuse it:
        // validator B branched at 4 and has never cited block 6.
        assert!(
            !is_safe_to_delete_at_floor(&dag, &hash(6), &floor_at(TOP), &conf())
                .expect("safety check"),
            "validator B sits on the side branch, so it has not moved past block 6 \
             and a merge from its view still reads that data"
        );
    }

    #[test]
    fn a_side_branch_block_is_kept_even_though_it_is_deep() {
        let dag = forked_chain_dag();
        let side5 = Bytes::from(b"side5".to_vec());
        assert!(
            !is_safe_to_delete_at_floor(&dag, &side5, &floor_at(TOP), &conf())
                .expect("safety check"),
            "validator A never left the spine, so it has not moved past the branch"
        );
    }
}
