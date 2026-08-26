//! Dependency-free pendants and buffer queries.
//!
//! Phase 3 Step 5 — extracted from `engine::multi_parent_casper`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference; the trait method is a one-line delegate in `traits.rs`.

use std::collections::HashSet;

use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;

use super::types::MultiParentCasperImpl;
use crate::rust::errors::CasperError;
use crate::rust::util::proto_util;

fn select_dependency_free<K, V, O, E>(
    candidate_hashes: HashSet<K>,
    mut load: impl FnMut(&K) -> Result<Option<V>, E>,
    mut dependencies_available: impl FnMut(&V) -> Result<bool, E>,
    mut project: impl FnMut(K, V) -> O,
) -> Result<Vec<O>, E>
where
    K: Eq + std::hash::Hash + Ord,
{
    let mut candidate_hashes: Vec<K> = candidate_hashes.into_iter().collect();
    candidate_hashes.sort();
    let mut selected = Vec::new();
    for candidate_hash in candidate_hashes {
        let Some(value) = load(&candidate_hash)? else {
            continue;
        };
        if dependencies_available(&value)? {
            selected.push(project(candidate_hash, value));
        }
    }
    Ok(selected)
}

pub(crate) fn buffer_get_dependency_free_from_buffer<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<Vec<BlockMessage>, CasperError> {
    select_dependency_free_from_buffer(this, |_, block| block)
}

pub(crate) fn buffer_get_dependency_free_hashes_from_buffer<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<Vec<BlockHash>, CasperError> {
    select_dependency_free_from_buffer(this, |hash, _| hash)
}

fn select_dependency_free_from_buffer<T, O>(
    this: &MultiParentCasperImpl<T>,
    project: impl FnMut(BlockHash, BlockMessage) -> O,
) -> Result<Vec<O>, CasperError>
where
    T: TransportLayer + Send + Sync,
{
    let dag = this.block_dag_storage.get_representation()?;

    let mut candidate_hashes: HashSet<BlockHash> = HashSet::new();

    let pendants = this.casper_buffer_storage.get_pendants();
    for pendant_serde in pendants.iter() {
        candidate_hashes.insert(BlockHash::from(pendant_serde.0.clone()));
    }

    let buffer_dag = this.casper_buffer_storage.to_doubly_linked_dag();
    for (child_hash, _) in buffer_dag.child_to_parent_adjacency_list.iter() {
        candidate_hashes.insert(BlockHash::from(child_hash.0.clone()));
    }

    select_dependency_free(
        candidate_hashes,
        |candidate_hash| -> Result<Option<BlockMessage>, CasperError> {
            Ok(this.block_store.get(candidate_hash)?)
        },
        |block| {
            proto_util::all_dependencies_have_admitted_metadata(block, &dag).map_err(Into::into)
        },
        project,
    )
}

pub(crate) fn buffer_get_all_from_buffer<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<Vec<BlockMessage>, CasperError> {
    let dag = this.casper_buffer_storage.to_doubly_linked_dag();
    let all_hashes = dag
        .child_to_parent_adjacency_list
        .keys()
        .map(|hash| BlockHash::from(hash.clone()));

    let mut blocks = Vec::new();
    for hash in all_hashes {
        if let Some(block) = this.block_store.get(&hash)? {
            blocks.push(block);
        }
    }

    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use proptest::prelude::*;

    use super::select_dependency_free;

    struct LiveValue {
        live: Arc<AtomicUsize>,
    }

    impl Drop for LiveValue {
        fn drop(&mut self) { self.live.fetch_sub(1, Ordering::AcqRel); }
    }

    #[test]
    fn selection_materializes_at_most_one_candidate_and_is_deterministic() {
        let live = Arc::new(AtomicUsize::new(0));
        let max_live = Arc::new(AtomicUsize::new(0));
        let candidates = HashSet::from([4u8, 1, 3, 2]);
        let selected = select_dependency_free(
            candidates,
            {
                let live = live.clone();
                let max_live = max_live.clone();
                move |_| -> Result<Option<LiveValue>, ()> {
                    let current = live.fetch_add(1, Ordering::AcqRel) + 1;
                    max_live.fetch_max(current, Ordering::AcqRel);
                    Ok(Some(LiveValue { live: live.clone() }))
                }
            },
            |_| Ok(true),
            |candidate, _| candidate,
        )
        .expect("candidate selection");

        assert_eq!(selected, vec![1, 2, 3, 4]);
        assert_eq!(live.load(Ordering::Acquire), 0);
        assert_eq!(max_live.load(Ordering::Acquire), 1);
    }

    #[test]
    fn selection_propagates_dependency_metadata_lookup_failure() {
        let result = select_dependency_free(
            HashSet::from([1u8]),
            |_| Ok::<_, &'static str>(Some(1u8)),
            |_| Err("metadata lookup failed"),
            |candidate, _| candidate,
        );

        assert_eq!(result, Err("metadata lookup failed"));
    }

    proptest! {
        #[test]
        fn selection_returns_the_sorted_available_ready_subset(
            candidates in proptest::collection::hash_set(any::<u16>(), 0..256),
            unavailable in proptest::collection::hash_set(any::<u16>(), 0..256),
            not_ready in proptest::collection::hash_set(any::<u16>(), 0..256),
        ) {
            let selected = select_dependency_free(
                candidates.clone(),
                |candidate| -> Result<Option<u16>, ()> {
                    Ok((!unavailable.contains(candidate)).then_some(*candidate))
                },
                |candidate| Ok(!not_ready.contains(candidate)),
                |candidate, _| candidate,
            ).expect("candidate selection");
            let mut expected: Vec<u16> = candidates
                .difference(&unavailable)
                .filter(|candidate| !not_ready.contains(candidate))
                .copied()
                .collect();
            expected.sort();
            prop_assert_eq!(selected, expected);
        }
    }
}
