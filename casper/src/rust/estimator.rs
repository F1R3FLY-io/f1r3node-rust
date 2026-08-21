// See casper/src/main/scala/coop/rchain/casper/Estimator.scala

//! Fork-choice estimator — GHOST-style heaviest-subtree selection
//! with the slashing-aware invalid-message filter.
//!
//! ## Responsibilities
//!
//! * Project the DAG's `latest_message_hashes` through the
//!   `invalid_latest_messages` filter so slashed validators contribute
//!   zero weight to fork choice (T-10).
//! * Choose the head by a heaviest-subtree DESCENT over the scored
//!   main-parent tree (`build_scores_map` + `rank_forkchoices`), ties
//!   by ascending hash for cross-node determinism; rank the remaining
//!   frontier tips behind it.
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
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
use shared::rust::shared::list_ops::ListOps;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::util::dag_operations::DagOperations;
use crate::rust::util::proto_util;

/// Tips of the DAG, ranked against LCA. `scores` carries the LMD-GHOST
/// cumulative-weight score per block so callers can distinguish a decisive
/// fork-choice winner from a tie (parent ordering may reorder only within
/// equal scores).
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
        tracing::debug!(target: "f1r3fly.casper.estimator.tips_primary", "latest-message-hashes");
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
            dag.invalid_latest_messages_from_hashes(&latest_messages_hashes)?;

        let mut filtered_latest_messages_hashes = latest_messages_hashes;
        filtered_latest_messages_hashes
            .retain(|validator, _| !invalid_latest_messages.contains_key(validator));

        let genesis_metadata = BlockMetadata::from_block(genesis, false, None, None);

        tracing::debug!(target: "f1r3fly.casper.estimator.tips_fallback", "lca");
        let lca =
            Self::calculate_lca(dag, &genesis_metadata, &filtered_latest_messages_hashes).await?;

        tracing::debug!(target: "f1r3fly.casper.estimator.tips_fallback", "score-map");
        let scores_map =
            Self::build_scores_map(dag, &filtered_latest_messages_hashes, &lca).await?;

        tracing::debug!(target: "f1r3fly.casper.estimator.tips_fallback", "ranked-latest-messages-hashes");
        let ranked_latest_messages_hashes = Self::rank_forkchoices(
            lca.clone(),
            &filtered_latest_messages_hashes,
            dag,
            &scores_map,
        )?;

        tracing::debug!(target: "f1r3fly.casper.estimator.tips_fallback", "filtered-deep-parents");
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
        let tips = if self.max_number_of_parents < 0
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
                // P2-8: avoid `split_first().unwrap()` panic when
                // `rank_forkchoices` returns an empty list (e.g.,
                // genesis-only DAG). Surface as a typed error so the
                // consensus hot path doesn't panic on an empty tip set.
                //
                // The variant choice — `KvStoreError::InvalidArgument` —
                // is a layering compromise: this function returns
                // `Result<_, KvStoreError>` (from the surrounding
                // estimator API), so we encode the consensus-invariant
                // violation as a precondition-violation on this function's
                // input. The error message identifies the source clearly
                // for operator diagnosis. A future cross-crate error
                // refactor could promote this to a typed
                // `CasperError::ConsensusInvariant` variant, but doing so
                // would ripple through the estimator's call graph;
                // documented as a follow-up.
                let Some((main_hash, secondary_hashes)) = ranked_latest_hashes.split_first() else {
                    return Err(KvStoreError::InvalidArgument(
                        "consensus invariant: rank_forkchoices returned no tips \
                         (genesis-only DAG?)"
                            .to_string(),
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
            DagOperations::lowest_universal_common_ancestor_many(&filtered_lm, block_dag, genesis)
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
                // MAIN parent only. Crediting a validator's weight to every DAG
                // ancestor saturates merged same-height siblings to equal scores
                // permanently — every latest message descends from both once the
                // race is merged — leaving the choice between them to a
                // tie-break rather than to validator support. A block has
                // exactly one main parent, so weight flows up exactly one chain
                // and same-height siblings are mutually exclusive by
                // construction, which is the exclusivity the clique theorem
                // assumes. `main_parent` is `parents.first()`
                // (block_metadata_store.rs:119).
                Ok(meta.parents.into_iter().take(1).collect())
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
                // B3: fail loudly on score overflow rather than wrapping. Reachable
                // only if the cumulative bonded weight on a block exceeds i64::MAX —
                // a supply-cap violation (total bonded stake ≤ i64::MAX by construction),
                // so this can only ever reject an already-invalid state, never a valid one.
                let entry = result.entry(hash.clone()).or_insert(0);
                *entry = entry.checked_add(validator_weight).ok_or_else(|| {
                    KvStoreError::InvalidArgument(
                        "fork-choice score overflow: cumulative validator weight exceeds i64 \
                         (total bonded stake must be ≤ i64::MAX by the supply cap)"
                            .to_string(),
                    )
                })?;
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

    /// The GHOST head plus the ranked frontier.
    ///
    /// The HEAD comes from a heaviest-subtree DESCENT: starting at the LCA,
    /// each step commits to the scored MAIN-parent child carrying the greatest
    /// cumulative score (ties by ascending hash) before descending further,
    /// and stops at the first block with no scored main-parent children — a
    /// latest-message tip. Scores accumulate up main-parent chains
    /// (`build_scores_map`), so a child's score IS its subtree's
    /// latest-message weight and the head can only leave a branch for one
    /// carrying strictly more support. Only MAIN-parent children are
    /// followed: a merge is a main-parent child of exactly one of its parents
    /// and a secondary child of the rest, so weight flows up exactly one
    /// chain and same-height siblings stay mutually exclusive. An unscored
    /// child is beyond the latest messages and bounds the walk.
    ///
    /// Ranking the TIPS by their own scores instead is NOT GHOST: a tip's own
    /// score is only its owner's weight, so under concurrent proposal every
    /// tip ties and the head falls to hash order — the spine then abandons
    /// majority branches, which is how a finality certificate was reverted
    /// with zero equivocations in production (the ucc-i6 divergence; see
    /// tests/fork_choice/heaviest_subtree_descent.rs).
    ///
    /// The tail is the remaining latest-message frontier — every other latest
    /// message with no scored main-parent child (one that HAS such a child is
    /// a superseded ancestor of another tip on its own chain) — ordered
    /// (score DESC, hash ASC) for callers that consume the full frontier.
    fn rank_forkchoices(
        lca: BlockHash,
        latest_messages_hashes: &HashMap<Validator, BlockHash>,
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
        loop {
            let mut children = scored_main_children(&head, block_dag, scores);
            if children.is_empty() {
                break;
            }
            children.sort_by(|a, b| {
                let score_a = scores.get(a).copied().unwrap_or(0);
                let score_b = scores.get(b).copied().unwrap_or(0);
                score_b.cmp(&score_a).then_with(|| a.cmp(b))
            });
            head = children.swap_remove(0);
        }

        let frontier: Vec<BlockHash> = latest_messages_hashes
            .values()
            .filter(|hash| {
                **hash != head && scored_main_children(hash, block_dag, scores).is_empty()
            })
            .cloned()
            .collect::<HashSet<_>>() // distinct
            .into_iter()
            .collect();
        let mut ranked = ListOps::sort_by_with_decreasing_order(frontier, scores);

        let mut tips = Vec::with_capacity(ranked.len() + 1);
        tips.push(head);
        tips.append(&mut ranked);
        Ok(tips)
    }
}
