// See casper/src/main/scala/coop/rchain/casper/Estimator.scala

//! Fork-choice estimator — GHOST-style heaviest-subtree selection over a
//! certified finalized-floor context.
//!
//! ## Responsibilities
//!
//! * Consume the sole eligible-vote projection certified by
//!   `CertifiedConsensusContext`.
//! * Rank surviving tips by cumulative frozen finalized-floor stake,
//!   breaking ties on hash for cross-node determinism.
//! * Apply `max_parent_depth` truncation so old parents do not delay
//!   finalization.
//!
//! ## Slashing-protocol position
//!
//! See `docs/casper/theory/slashing/slashing-verification.md` §6.4 (T-10) and
//! `docs/casper/theory/fork-choice/fork-choice-verification.md`.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
use rayon::prelude::*;
use shared::rust::shared::list_ops::ListOps;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::causal_equivocation::CertifiedConsensusContext;
use crate::rust::util::dag_operations::DagOperations;

/// Tips of the DAG, ranked against LCA.
#[derive(Debug, Clone, PartialEq)]
pub struct ForkChoice {
    pub tips: Vec<BlockHash>,
    pub lca: BlockHash,
    pub scores: HashMap<BlockHash, i64>,
}

#[derive(Debug, Clone)]
pub struct Estimator {
    max_number_of_parents: i32,
    max_parent_depth_opt: Option<i32>,
}

pub(crate) fn retained_parent_indices(
    block_numbers: &[i64],
    max_parent_depth: i64,
) -> Result<Vec<usize>, KvStoreError> {
    let Some(max_block_number) = block_numbers.iter().copied().max() else {
        return Err(KvStoreError::InvalidArgument(
            "consensus invariant: ranked fork choice has no parents".to_string(),
        ));
    };
    let mut retained = Vec::with_capacity(block_numbers.len());
    retained.push(0);
    for (index, block_number) in block_numbers.iter().copied().enumerate().skip(1) {
        let depth = max_block_number.checked_sub(block_number).ok_or_else(|| {
            KvStoreError::InvalidArgument(
                "parent depth overflow while bounding fork-choice tips".to_string(),
            )
        })?;
        if depth <= max_parent_depth {
            retained.push(index);
        }
    }
    Ok(retained)
}

pub(crate) fn declared_parent_depths_valid(
    block_numbers: &[i64],
    genesis_slots: &[bool],
    max_parent_depth: i64,
) -> Result<bool, KvStoreError> {
    if block_numbers.is_empty() || block_numbers.len() != genesis_slots.len() {
        return Err(KvStoreError::InvalidArgument(
            "declared parent depths require one height and genesis flag per parent".to_string(),
        ));
    }
    let max_block_number = block_numbers.iter().copied().max().ok_or_else(|| {
        KvStoreError::InvalidArgument("declared parent set has no maximum height".to_string())
    })?;
    for (index, block_number) in block_numbers.iter().copied().enumerate().skip(1) {
        if genesis_slots[index] {
            continue;
        }
        let depth = max_block_number.checked_sub(block_number).ok_or_else(|| {
            KvStoreError::InvalidArgument(
                "parent depth overflow while validating declared parents".to_string(),
            )
        })?;
        if depth > max_parent_depth {
            return Ok(false);
        }
    }
    Ok(true)
}

impl Estimator {
    pub const UNLIMITED_PARENTS: i32 = i32::MAX;

    pub fn apply(max_number_of_parents: i32, max_parent_depth_opt: Option<i32>) -> Self {
        Self {
            max_number_of_parents,
            max_parent_depth_opt,
        }
    }

    #[tracing::instrument(name = "tips", target = "f1r3fly.casper.estimator.tips", skip_all)]
    pub async fn tips_with_context(
        &self,
        dag: &KeyValueDagRepresentation,
        genesis: &BlockMessage,
        context: &CertifiedConsensusContext,
    ) -> Result<ForkChoice, KvStoreError> {
        if !context.has_complete_latest_message_slots() {
            return Err(KvStoreError::InvalidArgument(
                "fork choice requires one exact latest-message slot for every active finalized-floor validator"
                    .to_string(),
            ));
        }
        let latest_messages_hashes = context
            .vote_projection()
            .eligible_latest_messages()
            .iter()
            .map(|(validator, hash)| (validator.clone(), hash.clone()))
            .collect::<BTreeMap<_, _>>();

        let genesis_metadata = BlockMetadata::from_block(genesis, None, None);

        tracing::debug!(target: "f1r3fly.casper.estimator", "lca");
        let lca = Self::calculate_lca(dag, &genesis_metadata, &latest_messages_hashes).await?;

        tracing::debug!(target: "f1r3fly.casper.estimator", "score-map");
        let scores_map = Self::build_scores_map(
            dag,
            &latest_messages_hashes,
            context.authority_stakes(),
            &lca,
        )?;

        tracing::debug!(target: "f1r3fly.casper.estimator", "ranked-latest-messages-hashes");
        let ranked_latest_messages_hashes =
            Self::rank_forkchoices(lca.clone(), &latest_messages_hashes, dag, &scores_map)?;

        tracing::debug!(target: "f1r3fly.casper.estimator", "filtered-deep-parents");
        let ranked_shallow_hashes = self
            .filter_deep_parents(ranked_latest_messages_hashes, dag)
            .await?;

        // B2: treat BOTH "unlimited" sentinels EXPLICITLY rather than relying on
        // `-1 as usize` wrapping to usize::MAX. The estimator's own sentinel is
        // `Self::UNLIMITED_PARENTS` (i32::MAX); the config wire convention
        // (`casper::UNLIMITED_PARENTS`) is `-1`, and that config value reaches this
        // field directly (node setup passes `conf.casper.max_number_of_parents`). A
        // genuine positive cap truncates; any negative value or i32::MAX means
        // unlimited (take all). Behaviour is unchanged; the cast is now cast-safe and
        // the two conventions are no longer silently conflated by two's-complement.
        let tips = if self.max_number_of_parents == crate::rust::casper::UNLIMITED_PARENTS
            || self.max_number_of_parents == Self::UNLIMITED_PARENTS
        {
            ranked_shallow_hashes
        } else {
            ranked_shallow_hashes
                .into_iter()
                .take(self.max_number_of_parents as usize)
                .collect()
        };
        Ok(ForkChoice {
            tips,
            lca,
            scores: scores_map,
        })
    }

    async fn filter_deep_parents(
        &self,
        ranked_latest_hashes: Vec<BlockHash>,
        dag: &KeyValueDagRepresentation,
    ) -> Result<Vec<BlockHash>, KvStoreError> {
        match self.max_parent_depth_opt {
            Some(max_parent_depth) => {
                if ranked_latest_hashes.is_empty() {
                    return Err(KvStoreError::InvalidArgument(
                        "consensus invariant: rank_forkchoices returned no tips".to_string(),
                    ));
                }
                let ranked_metadata = ranked_latest_hashes
                    .iter()
                    .map(|hash| dag.lookup_unsafe(hash))
                    .collect::<Result<Vec<_>, _>>()?;
                let block_numbers = ranked_metadata
                    .iter()
                    .map(|metadata| metadata.block_number)
                    .collect::<Vec<_>>();
                retained_parent_indices(&block_numbers, i64::from(max_parent_depth)).map(
                    |indices| {
                        indices
                            .into_iter()
                            .map(|index| ranked_metadata[index].block_hash.clone())
                            .collect()
                    },
                )
            }
            None => Ok(ranked_latest_hashes),
        }
    }

    async fn calculate_lca(
        block_dag: &KeyValueDagRepresentation,
        genesis: &BlockMetadata,
        latest_messages_hashes: &BTreeMap<Validator, BlockHash>,
    ) -> Result<BlockHash, KvStoreError> {
        let latest_messages: Vec<BlockMetadata> = latest_messages_hashes
            .values()
            .map(|hash| block_dag.lookup(hash))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();

        let result = if latest_messages.is_empty() {
            genesis.block_hash.clone()
        } else {
            DagOperations::lowest_universal_common_ancestor_many(
                &latest_messages,
                block_dag,
                genesis,
            )
            .await?
            .block_hash
        };

        Ok(result)
    }

    fn build_scores_map(
        block_dag: &KeyValueDagRepresentation,
        latest_messages_hashes: &BTreeMap<Validator, BlockHash>,
        authority_stakes: &BTreeMap<Validator, i64>,
        lowest_common_ancestor: &BlockHash,
    ) -> Result<HashMap<BlockHash, i64>, KvStoreError> {
        fn hash_parents(
            hash: &BlockHash,
            last_finalized_block_number: i64,
            block_dag: &KeyValueDagRepresentation,
        ) -> Result<Vec<BlockHash>, KvStoreError> {
            // Phase 12 (PERF-1): one `lookup_unsafe` call per node, not two.
            // The prior version read `block_number` and then re-read the
            // whole `BlockMetadata` for `parents` — doubling lock
            // acquisitions on the BFS-bound fork-choice path.
            let meta = block_dag.lookup_unsafe(hash)?;
            if meta.block_number < last_finalized_block_number {
                Ok(Vec::new())
            } else {
                Ok(meta.parents.into_iter().take(1).collect())
            }
        }

        fn validator_support(
            latest_block_hash: &BlockHash,
            validator_weight: i64,
            lca_block_num: i64,
            block_dag: &KeyValueDagRepresentation,
        ) -> Result<BTreeMap<BlockHash, i64>, KvStoreError> {
            let mut result = BTreeMap::new();
            let mut queue: VecDeque<BlockHash> = VecDeque::from(vec![latest_block_hash.clone()]);
            let mut visited: HashSet<BlockHash> = HashSet::with_capacity(64);

            while let Some(hash) = queue.pop_front() {
                if !visited.insert(hash.clone()) {
                    continue;
                }
                result.insert(hash.clone(), validator_weight);
                for parent in hash_parents(&hash, lca_block_num, block_dag)? {
                    if !visited.contains(&parent) {
                        queue.push_back(parent);
                    }
                }
            }

            Ok(result)
        }

        let lca_block_num = block_dag
            .lookup_unsafe(lowest_common_ancestor)?
            .block_number;
        let inputs = latest_messages_hashes
            .iter()
            .map(|(validator, hash)| {
                authority_stakes
                    .get(validator)
                    .copied()
                    .filter(|stake| *stake > 0)
                    .map(|stake| (validator.clone(), hash.clone(), stake))
                    .ok_or_else(|| {
                        KvStoreError::InvalidArgument(format!(
                            "eligible validator {} has no positive frozen authority stake",
                            hex::encode(validator)
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let contributions = inputs
            .into_par_iter()
            .map(|(validator, latest_hash, stake)| {
                validator_support(&latest_hash, stake, lca_block_num, block_dag)
                    .map(|scores| (validator, scores))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut scores_map = BTreeMap::<BlockHash, i64>::new();
        for (_, contribution) in contributions {
            for (hash, weight) in contribution {
                let entry = scores_map.entry(hash).or_insert(0);
                *entry = entry.checked_add(weight).ok_or_else(|| {
                    KvStoreError::InvalidArgument(
                        "fork-choice score overflow: frozen authority stake exceeds i64"
                            .to_string(),
                    )
                })?;
            }
        }

        Ok(scores_map.into_iter().collect())
    }

    fn rank_forkchoices(
        lca: BlockHash,
        latest_messages_hashes: &BTreeMap<Validator, BlockHash>,
        block_dag: &KeyValueDagRepresentation,
        scores: &HashMap<BlockHash, i64>,
    ) -> Result<Vec<BlockHash>, KvStoreError> {
        fn scored_main_children(
            block: &BlockHash,
            block_dag: &KeyValueDagRepresentation,
            scores: &HashMap<BlockHash, i64>,
        ) -> Vec<BlockHash> {
            match block_dag.children(block) {
                Some(children_set) => children_set
                    .iter()
                    .filter(|child| {
                        scores.contains_key(*child)
                            && block_dag.main_parent(child).as_ref() == Some(block)
                    })
                    .cloned()
                    .collect(),
                None => Vec::new(),
            }
        }

        let mut head = lca;
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(head.clone()) {
                return Err(KvStoreError::InvalidArgument(
                    "fork-choice child graph contains a cycle".to_string(),
                ));
            }
            let mut children = scored_main_children(&head, block_dag, scores);
            if children.is_empty() {
                break;
            }
            children.sort_by(|a, b| {
                let score_a = scores.get(a).copied().unwrap_or(0);
                let score_b = scores.get(b).copied().unwrap_or(0);
                score_b.cmp(&score_a).then_with(|| a.cmp(b))
            });
            let next = children.swap_remove(0);
            let current_number = block_dag.block_number_unsafe(&head)?;
            let next_number = block_dag.block_number_unsafe(&next)?;
            if next_number <= current_number {
                return Err(KvStoreError::InvalidArgument(
                    "fork-choice child does not advance DAG height".to_string(),
                ));
            }
            head = next;
        }

        let frontier = latest_messages_hashes
            .values()
            .filter(|hash| {
                **hash != head && scored_main_children(hash, block_dag, scores).is_empty()
            })
            .cloned()
            .collect::<HashSet<_>>();
        let mut ranked = ListOps::sort_by_with_decreasing_order(
            frontier.into_iter().collect::<Vec<_>>(),
            scores,
        );
        ranked.insert(0, head);
        Ok(ranked)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{declared_parent_depths_valid, retained_parent_indices};

    #[test]
    fn taller_secondary_never_removes_the_ranked_head() {
        let retained = retained_parent_indices(&[10, 100, 95, 20], 10).unwrap();
        assert_eq!(retained, vec![0, 1, 2]);
    }

    #[test]
    fn count_truncation_after_depth_filter_keeps_the_ranked_head() {
        let retained = retained_parent_indices(&[10, 100, 95, 20], 10).unwrap();
        assert_eq!(retained.into_iter().take(1).collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn empty_ranked_parent_set_is_a_typed_error() {
        let error = retained_parent_indices(&[], 10).unwrap_err();
        assert!(matches!(error, super::KvStoreError::InvalidArgument(_)));
    }

    #[test]
    fn approved_genesis_secondary_is_receiver_exempt() {
        assert!(declared_parent_depths_valid(&[100, 0], &[false, true], 0).unwrap());
    }

    proptest! {
        #[test]
        fn depth_filter_preserves_head_and_exactly_bounds_the_tail(
            block_numbers in prop::collection::vec(0i64..=10_000, 1..=32),
            max_parent_depth in 0i64..=1_000,
        ) {
            let retained = retained_parent_indices(&block_numbers, max_parent_depth).unwrap();
            let max_block_number = block_numbers.iter().copied().max().unwrap();
            prop_assert_eq!(retained.first(), Some(&0));
            let expected = std::iter::once(0)
                .chain(
                    block_numbers
                        .iter()
                        .copied()
                        .enumerate()
                        .skip(1)
                        .filter(move |(_, height)| max_block_number - height <= max_parent_depth)
                        .map(|(index, _)| index),
                )
                .collect::<Vec<_>>();
            prop_assert_eq!(retained, expected);
        }

        #[test]
        fn proposer_output_satisfies_buffered_receiver_depth_check(
            block_numbers in prop::collection::vec(0i64..=10_000, 1..=32),
            max_parent_depth in 0i64..=1_000,
            depth_buffer in 0i64..=1_000,
            count_cap in 1usize..=32,
        ) {
            let retained = retained_parent_indices(&block_numbers, max_parent_depth).unwrap();
            let declared = retained
                .into_iter()
                .take(count_cap)
                .map(|index| block_numbers[index])
                .collect::<Vec<_>>();
            let genesis_slots = vec![false; declared.len()];
            prop_assert!(declared_parent_depths_valid(
                &declared,
                &genesis_slots,
                max_parent_depth + depth_buffer,
            ).unwrap());
        }
    }
}
