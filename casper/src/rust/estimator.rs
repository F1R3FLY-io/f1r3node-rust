// See casper/src/main/scala/coop/rchain/casper/Estimator.scala

//! Fork-choice estimator — GHOST-style heaviest-subtree selection
//! with the slashing-aware invalid-message filter.
//!
//! ## Responsibilities
//!
//! * Project the DAG's `latest_message_hashes` through the
//!   `invalid_latest_messages` filter so slashed validators contribute
//!   zero weight to fork choice (T-10).
//! * Rank surviving tips by their cumulative validator-weight score
//!   (`build_scores_map`), breaking ties on hash for cross-node
//!   determinism.
//! * Apply `max_parent_depth` truncation so old parents do not delay
//!   finalization.
//!
//! ## Slashing-protocol position
//!
//! See `docs/theory/slashing/slashing-verification.md` §6.4 (T-10) for
//! the abstract filter property. The operational realization is the
//! conjunction `(invalid-block-flag) ∧ (bond=0 ⇒ zero weight)` — see
//! `docs/theory/slashing/design/07-fork-choice-and-lifecycle.md`.

use std::collections::{HashMap, HashSet, VecDeque};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use futures::stream::{self, StreamExt, TryStreamExt};
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
use shared::rust::shared::list_ops::ListOps;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::util::dag_operations::DagOperations;
use crate::rust::util::proto_util;

/// Tips of the DAG, ranked against LCA
#[derive(Debug, Clone, PartialEq)]
pub struct ForkChoice {
    pub tips: Vec<BlockHash>,
    pub lca: BlockHash,
}

#[derive(Debug, Clone)]
pub struct Estimator {
    max_number_of_parents: i32,
    max_parent_depth_opt: Option<i32>,
}

impl Estimator {
    pub const UNLIMITED_PARENTS: i32 = i32::MAX;
    const LATEST_MESSAGE_MAX_DEPTH: i64 = 1000;

    pub fn apply(max_number_of_parents: i32, max_parent_depth_opt: Option<i32>) -> Self {
        Self {
            max_number_of_parents,
            max_parent_depth_opt,
        }
    }

    #[tracing::instrument(name = "tips0", target = "f1r3fly.casper.estimator.tips0", skip_all)]
    pub async fn tips(
        &self,
        dag: &mut KeyValueDagRepresentation,
        genesis: &BlockMessage,
    ) -> Result<ForkChoice, KvStoreError> {
        // Phase 12 (PERF-5): `latest_message_hashes()` returns an owned
        // `imbl::HashMap` (refcount-bump clone). Use `into_iter` to collect
        // by ownership rather than re-cloning every key/value pair.
        let latest_message_hashes: HashMap<Validator, BlockHash> =
            dag.latest_message_hashes().into_iter().collect();
        tracing::debug!(target: "f1r3fly.casper.estimator.tips0", "latest-message-hashes");
        self.tips_with_latest_messages(dag, genesis, latest_message_hashes)
            .await
    }

    /// When the BlockDag has an empty latestMessages, tips will return IndexedSeq(genesis.blockHash)
    #[tracing::instrument(name = "tips1", target = "f1r3fly.casper.estimator.tips1", skip_all)]
    pub async fn tips_with_latest_messages(
        &self,
        dag: &mut KeyValueDagRepresentation,
        genesis: &BlockMessage,
        latest_messages_hashes: HashMap<Validator, BlockHash>,
    ) -> Result<ForkChoice, KvStoreError> {
        let invalid_latest_messages =
            dag.invalid_latest_messages_from_hashes(latest_messages_hashes.clone())?;

        let mut filtered_latest_messages_hashes = latest_messages_hashes;
        filtered_latest_messages_hashes
            .retain(|validator, _| !invalid_latest_messages.contains_key(validator));

        let genesis_metadata = BlockMetadata::from_block(genesis, false, None, None);

        tracing::debug!(target: "f1r3fly.casper.estimator.tips1", "lca");
        let lca =
            Self::calculate_lca(dag, &genesis_metadata, &filtered_latest_messages_hashes).await?;

        tracing::debug!(target: "f1r3fly.casper.estimator.tips1", "score-map");
        let scores_map =
            Self::build_scores_map(dag, &filtered_latest_messages_hashes, &lca).await?;

        tracing::debug!(target: "f1r3fly.casper.estimator.tips1", "ranked-latest-messages-hashes");
        let ranked_latest_messages_hashes =
            Self::rank_forkchoices(vec![lca.clone()], dag, &scores_map).await?;

        tracing::debug!(target: "f1r3fly.casper.estimator.tips1", "filtered-deep-parents");
        let ranked_shallow_hashes = self
            .filter_deep_parents(ranked_latest_messages_hashes, dag)
            .await?;

        Ok(ForkChoice {
            tips: ranked_shallow_hashes
                .into_iter()
                .take(self.max_number_of_parents as usize)
                .collect(),
            lca,
        })
    }

    async fn filter_deep_parents(
        &self,
        ranked_latest_hashes: Vec<BlockHash>,
        dag: &KeyValueDagRepresentation,
    ) -> Result<Vec<BlockHash>, KvStoreError> {
        match self.max_parent_depth_opt {
            Some(max_parent_depth) => {
                // P2-8: avoid `split_first().unwrap()` panic when
                // `rank_forkchoices` returns an empty list (e.g.,
                // genesis-only DAG). Surface as a typed error so the
                // consensus hot path doesn't panic on an empty tip set.
                let Some((main_hash, secondary_hashes)) = ranked_latest_hashes.split_first()
                else {
                    return Err(KvStoreError::InvalidArgument(
                        "rank_forkchoices returned no tips".to_string(),
                    ));
                };

                let max_block_number = dag.lookup_unsafe(main_hash)?.block_number;

                let secondary_parents: Vec<BlockMetadata> = secondary_hashes
                    .iter()
                    .map(|hash| dag.lookup_unsafe(hash))
                    .collect::<Result<Vec<_>, _>>()?;

                let shallow_parents: Vec<BlockMetadata> = secondary_parents
                    .into_iter()
                    .filter(|p| max_block_number - p.block_number <= max_parent_depth as i64)
                    .collect();

                Ok(std::iter::once(main_hash.clone())
                    .chain(shallow_parents.into_iter().map(|p| p.block_hash))
                    .collect())
            }
            None => Ok(ranked_latest_hashes),
        }
    }

    async fn calculate_lca(
        block_dag: &KeyValueDagRepresentation,
        genesis: &BlockMetadata,
        latest_messages_hashes: &HashMap<Validator, BlockHash>,
    ) -> Result<BlockHash, KvStoreError> {
        let latest_messages: Vec<BlockMetadata> = latest_messages_hashes
            .values()
            .map(|hash| block_dag.lookup(hash))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();

        let top_block_number = block_dag.latest_block_number();

        let filtered_lm: Vec<BlockMetadata> = latest_messages
            .into_iter()
            .filter(|msg| msg.block_number > top_block_number - Self::LATEST_MESSAGE_MAX_DEPTH)
            .collect();

        let result = if filtered_lm.is_empty() {
            genesis.block_hash.clone()
        } else {
            DagOperations::lowest_universal_common_ancestor_many(&filtered_lm, block_dag)
                .await?
                .block_hash
        };

        Ok(result)
    }

    async fn build_scores_map(
        block_dag: &mut KeyValueDagRepresentation,
        latest_messages_hashes: &HashMap<Validator, BlockHash>,
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
                Ok(meta.parents)
            }
        }

        async fn add_validator_weight_down_supporting_chain(
            score_map: HashMap<BlockHash, i64>,
            validator: &Validator,
            latest_block_hash: &BlockHash,
            block_dag: &mut KeyValueDagRepresentation,
            lowest_common_ancestor: &BlockHash,
        ) -> Result<HashMap<BlockHash, i64>, KvStoreError> {
            let lca_block_num = block_dag
                .lookup_unsafe(lowest_common_ancestor)?
                .block_number;

            // Phase 12 (PERF-2): merge BFS traversal with weight accumulation
            // instead of building a Vec of traversed hashes then re-iterating.
            // Saves one clone per node and one Vec allocation. Preallocate
            // visited/result to a reasonable capacity for typical fork-choice
            // BFS sizes (≤ ~few hundred blocks).
            let mut result = score_map;
            let mut queue: VecDeque<BlockHash> = VecDeque::from(vec![latest_block_hash.clone()]);
            let mut visited: HashSet<BlockHash> = HashSet::with_capacity(64);

            while let Some(hash) = queue.pop_front() {
                if !visited.insert(hash.clone()) {
                    continue;
                }
                let validator_weight =
                    proto_util::weight_from_validator_by_dag(block_dag, &hash, validator)?;
                *result.entry(hash.clone()).or_insert(0) += validator_weight;
                for parent in hash_parents(&hash, lca_block_num, block_dag)? {
                    if !visited.contains(&parent) {
                        queue.push_back(parent);
                    }
                }
            }

            Ok(result)
        }

        // TODO: Scala message - Since map scores are additive it should be possible to do this in parallel
        let mut scores_map: HashMap<BlockHash, i64> = HashMap::new();
        for (validator, latest_block_hash) in latest_messages_hashes.iter() {
            scores_map = add_validator_weight_down_supporting_chain(
                scores_map,
                validator,
                latest_block_hash,
                block_dag,
                lowest_common_ancestor,
            )
            .await?;
        }

        Ok(scores_map)
    }

    async fn rank_forkchoices(
        blocks: Vec<BlockHash>,
        block_dag: &KeyValueDagRepresentation,
        scores: &HashMap<BlockHash, i64>,
    ) -> Result<Vec<BlockHash>, KvStoreError> {
        let unsorted_new_blocks: Vec<BlockHash> = stream::iter(blocks.iter())
            .then(|block| Self::replace_block_hash_with_children(block, block_dag, scores))
            .try_fold(Vec::new(), |mut acc, children| async move {
                acc.extend(children);
                Ok(acc)
            })
            .await?;

        let unique_blocks: Vec<BlockHash> = unsorted_new_blocks
            .into_iter()
            .collect::<HashSet<_>>() // distinct
            .into_iter()
            .collect();

        let new_blocks = ListOps::sort_by_with_decreasing_order(unique_blocks, scores);

        if Self::still_same(&blocks, &new_blocks) {
            Ok(blocks)
        } else {
            Box::pin(Self::rank_forkchoices(new_blocks, block_dag, scores)).await
        }
    }

    fn non_empty_list(elements: &HashSet<BlockHash>) -> Option<Vec<BlockHash>> {
        if elements.is_empty() {
            None
        } else {
            Some(elements.iter().cloned().collect())
        }
    }

    /// Only include children that have been scored,
    /// this ensures that the search does not go beyond
    /// the messages defined by blockDag.latestMessages
    async fn replace_block_hash_with_children(
        b: &BlockHash,
        block_dag: &KeyValueDagRepresentation,
        scores: &HashMap<BlockHash, i64>,
    ) -> Result<Vec<BlockHash>, KvStoreError> {
        match block_dag.children(b) {
            Some(children_set) => {
                let scored_children: HashSet<BlockHash> = children_set
                    .iter()
                    .filter_map(|child| {
                        let child_hash = child.clone();
                        if scores.contains_key(&child_hash) {
                            Some(child_hash)
                        } else {
                            None
                        }
                    })
                    .collect();

                match Self::non_empty_list(&scored_children) {
                    Some(non_empty_children) => Ok(non_empty_children),
                    None => Ok(vec![b.clone()]),
                }
            }
            None => Ok(vec![b.clone()]),
        }
    }

    fn still_same(blocks: &[BlockHash], new_blocks: &[BlockHash]) -> bool { new_blocks == blocks }
}
