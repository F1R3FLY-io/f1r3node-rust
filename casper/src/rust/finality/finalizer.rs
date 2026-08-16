// See casper/src/main/scala/coop/rchain/casper/finality/Finalizer.scala

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::validator::Validator;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::errors::CasperError;
use crate::rust::finality::floor::{
    is_state_ancestor_from_cached_floors, materialize_state_lineage,
};
use crate::rust::safety::clique_oracle::{ft_decides_exact, CliqueOracle, FtThreshold};

/// Block can be recorded as last finalized block (LFB) if Safety oracle outputs fault tolerance (FT)
/// for this block greater then some predefined threshold. This is defined by [`CliqueOracle::compute_output`]
/// function, which requires some target block as input arg.
///
/// Therefore: Finalizer has a scope of search, defined by tips and previous LFB - each of this blocks can be next LFB.
///
/// We know that LFB advancement is not necessary continuous, next LFB might not be direct child of current one.
///
/// Therefore: we cannot start from current LFB children and traverse DAG from the bottom to the top, calculating FT
/// for each block. Also its computationally ineffective.
///
/// But we know that scope of search for potential next LFB is constrained. Block A can be finalized only
/// if it has more then half of total stake in bonds map of A translated from tips throughout main parent ancestry.
///
/// Therefore: Finalizer should seek for next LFB going through 2 steps:
///   1. Find messages in scope of search that have more then half of the stake translated through main parent ancestry
///     from tips down to the message.
///   2. Execute [`CliqueOracle::compute_output`] on these targets.
///   3. First message passing FT threshold becomes the next LFB.
pub struct Finalizer;
const FINALIZER_CATCHUP_LAG_THRESHOLD_BLOCKS: i64 = 1_024;

type WeightMap = HashMap<Validator, i64>;
type SharedWeightMap = Arc<WeightMap>;

impl Finalizer {
    fn checked_stake_sum(weight_map: &WeightMap) -> Option<i64> {
        weight_map
            .values()
            .try_fold(0_i64, |acc, stake| acc.checked_add(*stake))
    }

    /// weight map as per message, look inside [`CliqueOracle::get_corresponding_weight_map`] description for more info
    async fn message_weight_map_f(
        message: &BlockMetadata,
        dag: &KeyValueDagRepresentation,
    ) -> Result<WeightMap, KvStoreError> {
        CliqueOracle::get_corresponding_weight_map(&message.block_hash, dag).await
    }

    /// If more then half of total stake agree on message - it is considered to be safe from orphaning.
    pub fn cannot_be_orphaned(
        message_weight_map: &WeightMap,
        agreeing_weight_map: &WeightMap,
    ) -> bool {
        if agreeing_weight_map.values().any(|&stake| stake <= 0) {
            tracing::error!(
                target: "f1r3fly.finalizer",
                "cannot_be_orphaned skipped due to non-positive agreeing stake entries"
            );
            return false;
        }

        let Some(active_stake_total) = Self::checked_stake_sum(message_weight_map) else {
            tracing::warn!(
                target: "f1r3fly.finalizer",
                "cannot_be_orphaned skipped due to total stake overflow"
            );
            return false;
        };

        let Some(active_stake_agreeing) = Self::checked_stake_sum(agreeing_weight_map) else {
            tracing::warn!(
                target: "f1r3fly.finalizer",
                "cannot_be_orphaned skipped due to agreeing stake overflow"
            );
            return false;
        };

        if active_stake_total <= 0 || active_stake_agreeing <= 0 {
            tracing::warn!(
                target: "f1r3fly.finalizer",
                "cannot_be_orphaned skipped due to non-positive stake totals: total={}, agreeing={}",
                active_stake_total,
                active_stake_agreeing
            );
            return false;
        }

        // Compare in integer space to avoid fp precision/rounding edge cases.
        (active_stake_agreeing as i128) * 2 > active_stake_total as i128
    }

    /// Cheap upper bound on FT without clique search.
    /// Since max clique weight <= sum(agreeing stake), this is a safe prune bound.
    fn fault_tolerance_upper_bound(
        message_weight_map: &WeightMap,
        agreeing_weight_map: &WeightMap,
    ) -> f64 {
        let Some(total_stake) = Self::checked_stake_sum(message_weight_map) else {
            return f64::MIN;
        };
        let Some(agreeing_stake) = Self::checked_stake_sum(agreeing_weight_map) else {
            return f64::MIN;
        };
        if total_stake <= 0 {
            return f64::MIN;
        }
        (((agreeing_stake as i128) * 2 - (total_stake as i128)) as f64) / (total_stake as f64)
    }

    /// Create an agreement given validator that agrees on a message and weight map of a message.
    /// If validator is not present in message bonds map or its stake is zero, None is returned
    fn record_agreement(
        message_weight_map: &WeightMap,
        agreeing_validator: &Validator,
    ) -> Option<(Validator, i64)> {
        // if validator is not bonded according to message weight map - there is no agreement translated.
        let stake_agreed = message_weight_map
            .get(agreeing_validator)
            .copied()
            .unwrap_or(0);
        if stake_agreed > 0 {
            Some((agreeing_validator.clone(), stake_agreed))
        } else {
            None
        }
    }

    /// Find the highest finalized message.
    /// Scope of the search is constrained by the lowest height (height of current last finalized message).
    pub async fn run<F, Fut>(
        dag: &KeyValueDagRepresentation,
        ftt: FtThreshold,
        curr_lfb_hash: &BlockHash,
        curr_lfb_height: i64,
        mut new_lfb_found_effect: F,
        finalizer_conf: &crate::rust::casper_conf::FinalizerConf,
    ) -> Result<Option<(BlockHash, f32)>, CasperError>
    where
        F: FnMut((BlockHash, f32)) -> Fut,
        Fut: std::future::Future<Output = Result<(), KvStoreError>>,
    {
        let total_started = std::time::Instant::now();
        let lfb_lag = dag.latest_block_number().saturating_sub(curr_lfb_height);
        let catchup_mode = lfb_lag > FINALIZER_CATCHUP_LAG_THRESHOLD_BLOCKS;
        let yield_interval = if catchup_mode {
            finalizer_conf.catchup_yield_interval
        } else {
            finalizer_conf.yield_interval
        };
        /*
         * Stream of agreements passed down from all latest messages to main parents.
         * Starts with agreements of latest message on themselves.
         *
         * The goal here is to create stream of agreements breadth first, so on each step agreements by all
         * validator are recorded, and only after that the next level of main parents is visited.
         */
        let lms = dag.latest_messages()?;
        let latest_messages_count = lms.len();
        let latest_messages_snapshot = lms
            .iter()
            .map(|(validator, message)| (validator.clone(), message.block_hash.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();

        // sort latest messages by agreeing validator to ensure random ordering does not change output
        let mut sorted_latest_messages: Vec<(Validator, BlockMetadata)> = lms.into_iter().collect();
        sorted_latest_messages.sort_by(|(v1, _), (v2, _)| v1.cmp(v2));
        let mut scheduled_agreements: HashSet<(Validator, BlockHash)> = sorted_latest_messages
            .iter()
            .map(|(validator, message)| (validator.clone(), message.block_hash.clone()))
            .collect();

        // Step 1: Traverse agreement layers and aggregate agreements per target block.
        // This avoids materializing a large stream of duplicate (message, weight-map, agreement)
        // tuples that would later be deduplicated by block hash.
        let mut aggregated_agreements: HashMap<
            BlockHash,
            (BlockMetadata, SharedWeightMap, WeightMap),
        > = HashMap::new();
        let mut message_weight_map_cache: HashMap<BlockHash, SharedWeightMap> = HashMap::new();
        let mut parent_cache: HashMap<BlockHash, BlockMetadata> = HashMap::new();
        let mut message_weight_map_cache_hit: usize = 0;
        let mut message_weight_map_cache_miss: usize = 0;
        let mut message_weight_map_error_count: usize = 0;
        let mut main_parent_cache_hit: usize = 0;
        let mut main_parent_cache_miss: usize = 0;
        let mut current_layer = sorted_latest_messages;
        let mut layers_visited: usize = 0;
        let mut agreements_count: usize = 0;
        let mut weight_map_phase_ns: u128 = 0;
        let mut agreement_record_phase_ns: u128 = 0;
        let mut parent_lookup_phase_ns: u128 = 0;
        let mut next_layer_push_phase_ns: u128 = 0;

        let mut last_yield = std::time::Instant::now();
        loop {
            layers_visited += 1;
            let mut next_layer: Vec<(Validator, BlockMetadata)> = Vec::new();

            for (agreeing_validator, message) in current_layer.into_iter() {
                if last_yield.elapsed() >= yield_interval {
                    tokio::task::yield_now().await;
                    last_yield = std::time::Instant::now();
                }
                let phase_t = std::time::Instant::now();
                let message_weight_map = if let Some(cached) =
                    message_weight_map_cache.get(&message.block_hash).cloned()
                {
                    message_weight_map_cache_hit += 1;
                    cached
                } else {
                    message_weight_map_cache_miss += 1;
                    let fetched = Self::message_weight_map_f(&message, dag)
                        .await
                        .inspect_err(|_| message_weight_map_error_count += 1)?;
                    let fetched = Arc::new(fetched);
                    message_weight_map_cache.insert(message.block_hash.clone(), fetched.clone());
                    fetched
                };
                weight_map_phase_ns += phase_t.elapsed().as_nanos();

                let phase_t = std::time::Instant::now();
                let (_, _, agreeing_weight_map) = aggregated_agreements
                    .entry(message.block_hash.clone())
                    .or_insert_with(|| {
                        (
                            message.clone(),
                            message_weight_map.clone(),
                            WeightMap::new(),
                        )
                    });
                if let Some((agreeing_validator, stake_agreed)) =
                    Self::record_agreement(&message_weight_map, &agreeing_validator)
                {
                    if agreeing_weight_map.contains_key(&agreeing_validator) {
                        tracing::warn!(
                            target: "f1r3fly.finalizer",
                            "Duplicate agreement observed while aggregating finalizer candidate; keeping first value. message={:?}, validator={:?}",
                            message.block_hash,
                            agreeing_validator
                        );
                    } else {
                        agreeing_weight_map.insert(agreeing_validator, stake_agreed);
                        agreements_count += 1;
                    }
                }
                agreement_record_phase_ns += phase_t.elapsed().as_nanos();

                if let Some(parent_hash) = message.parents.first() {
                    let phase_t = std::time::Instant::now();
                    let parent_meta = if let Some(cached) = parent_cache.get(parent_hash) {
                        main_parent_cache_hit += 1;
                        cached.clone()
                    } else {
                        main_parent_cache_miss += 1;
                        let fetched = dag.lookup_unsafe(parent_hash)?;
                        parent_cache.insert(parent_hash.clone(), fetched.clone());
                        fetched
                    };
                    parent_lookup_phase_ns += phase_t.elapsed().as_nanos();

                    let phase_t = std::time::Instant::now();
                    if parent_meta.block_number > curr_lfb_height
                        && scheduled_agreements
                            .insert((agreeing_validator.clone(), parent_hash.clone()))
                    {
                        next_layer.push((agreeing_validator.clone(), parent_meta));
                    }
                    next_layer_push_phase_ns += phase_t.elapsed().as_nanos();
                }
            }

            if next_layer.is_empty() {
                break;
            }

            current_layer = next_layer;
        }

        // Step 2: Filter blocks that cannot be orphaned and precompute sort keys.
        let filtered_agreements: Vec<(BlockMetadata, SharedWeightMap, WeightMap, i64, usize)> =
            aggregated_agreements
                .into_values()
                .filter_map(|(message, message_weight_map, agreeing_weight_map)| {
                    Self::cannot_be_orphaned(&message_weight_map, &agreeing_weight_map).then(|| {
                        let stake_sum = agreeing_weight_map.values().sum::<i64>();
                        let agreeing_size = agreeing_weight_map.len();
                        (
                            message,
                            message_weight_map,
                            agreeing_weight_map,
                            stake_sum,
                            agreeing_size,
                        )
                    })
                })
                .collect();
        let filtered_agreements_count = filtered_agreements.len();
        let mut deduped_filtered_agreements: Vec<(
            BlockMetadata,
            SharedWeightMap,
            WeightMap,
            i64,
            usize,
        )> = filtered_agreements;
        // Sort candidates by recency (block height desc), then stake (desc), then set size (asc).
        deduped_filtered_agreements.sort_by(
            |(msg_l, _, _, stake_l, size_l), (msg_r, _, _, stake_r, size_r)| {
                msg_r
                    .block_number
                    .cmp(&msg_l.block_number)
                    .then_with(|| stake_r.cmp(stake_l))
                    .then_with(|| size_l.cmp(size_r))
                    .then_with(|| msg_l.block_hash.cmp(&msg_r.block_hash))
            },
        );
        let deduped_filtered_agreements_count = deduped_filtered_agreements.len();
        let ordered_agreements: Vec<(BlockMetadata, SharedWeightMap, WeightMap)> =
            deduped_filtered_agreements
                .into_iter()
                .map(|(message, message_weight_map, agreeing_weight_map, _, _)| {
                    (message, message_weight_map, agreeing_weight_map)
                })
                .collect();

        // Compute fault tolerance lazily and stop at the first candidate that satisfies
        // finalization criteria. Preserves original candidate order while avoiding
        // expensive full-scan FT computation on long chains.
        let clique_started = std::time::Instant::now();
        let mut clique_run_cache = CliqueOracle::new_run_cache();
        // Snapshot the DAG's latest messages once for this finalization pass; the
        // FT computation reads agreement from this frozen snapshot, matching the
        // node-deterministic `ft_witnessed` parametrization.
        let mut clique_eval_count: usize = 0;
        let mut upper_bound_pruned_count: usize = 0;
        let mut upper_bound_passed_count: usize = 0;
        let mut max_ft_upper_bound: f64 = f64::MIN;
        let mut lfb_result: Option<(BlockHash, f32)> = None;
        for (message, message_weight_map, agreeing_weight_map) in ordered_agreements {
            if last_yield.elapsed() >= yield_interval {
                tokio::task::yield_now().await;
                last_yield = std::time::Instant::now();
            }
            if message.block_hash == *curr_lfb_hash {
                continue;
            }
            let ft_upper_bound =
                Self::fault_tolerance_upper_bound(&message_weight_map, &agreeing_weight_map);
            max_ft_upper_bound = max_ft_upper_bound.max(ft_upper_bound);
            // A9 exact conservative prune: the max clique weight q is bounded above
            // by the total agreeing stake, so if the strict exact test fails even at
            // q = agreeing, no real clique can finalize this candidate. This NEVER
            // prunes a true finalizer at the θ margin (the f32 rounding hazard the
            // exact decision removes). The f32 `ft_upper_bound` above is retained
            // for the telemetry metric only.
            let prune = match (
                Self::checked_stake_sum(&message_weight_map),
                Self::checked_stake_sum(&agreeing_weight_map),
            ) {
                (Some(total_stake), Some(agreeing_stake)) if total_stake > 0 => !ft_decides_exact(
                    agreeing_stake,
                    agreeing_stake,
                    total_stake,
                    ftt.num,
                    ftt.den,
                    true,
                ),
                _ => true,
            };
            if prune {
                upper_bound_pruned_count += 1;
                continue;
            }
            upper_bound_passed_count += 1;
            clique_eval_count += 1;
            let (finalized, ft_value) = CliqueOracle::compute_decision_with_cache(
                &message.block_hash,
                &message_weight_map,
                &agreeing_weight_map,
                dag,
                &mut clique_run_cache,
                &latest_messages_snapshot,
                ftt.num,
                ftt.den,
                true,
            )
            .await?;

            if finalized {
                materialize_state_lineage(dag, &message.block_hash, ftt).await?;
                if !is_state_ancestor_from_cached_floors(dag, curr_lfb_hash, &message.block_hash)? {
                    tracing::debug!(
                        target: "f1r3fly.finalizer",
                        candidate = %hex::encode(&message.block_hash[..]),
                        current_lfb = %hex::encode(&curr_lfb_hash[..]),
                        "Finalizer candidate does not preserve the current LFB state lineage"
                    );
                    continue;
                }
                let lfb_hash = message.block_hash.clone();
                new_lfb_found_effect((lfb_hash.clone(), ft_value)).await?;
                lfb_result = Some((lfb_hash, ft_value));
                break;
            } else {
                tracing::debug!(
                    target: "f1r3fly.finalizer.timing",
                    "Finalizer candidate rejected by threshold: hash={:?}, fault_tolerance={:.6}, threshold_ppm={}/{}",
                    message.block_hash,
                    ft_value,
                    ftt.num,
                    ftt.den
                );
            }
        }
        tracing::debug!(
            target: "f1r3fly.finalizer.timing",
            "Finalizer timing: latest_messages={}, layers_visited={}, agreements={}, filtered_agreements={}, deduped_filtered_agreements={}, message_weight_map_cache_hit={}, message_weight_map_cache_miss={}, message_weight_map_errors={}, main_parent_cache_hit={}, main_parent_cache_miss={}, ranking_strategy={}, upper_bound_pruned={}, upper_bound_passed={}, max_ft_upper_bound={:.6}, clique_evals={}, clique_ms={}, total_ms={}, yield_interval_ms={}, lfb_lag={}, catchup_mode={}, found_new_lfb={}, weight_map_ns={}, agreement_ns={}, parent_ns={}, next_push_ns={}",
            latest_messages_count,
            layers_visited,
            agreements_count,
            filtered_agreements_count,
            deduped_filtered_agreements_count,
            message_weight_map_cache_hit,
            message_weight_map_cache_miss,
            message_weight_map_error_count,
            main_parent_cache_hit,
            main_parent_cache_miss,
            "recency_stake",
            upper_bound_pruned_count,
            upper_bound_passed_count,
            max_ft_upper_bound,
            clique_eval_count,
            clique_started.elapsed().as_millis(),
            total_started.elapsed().as_millis(),
            yield_interval.as_millis(),
            lfb_lag,
            catchup_mode,
            lfb_result.is_some(),
            weight_map_phase_ns,
            agreement_record_phase_ns,
            parent_lookup_phase_ns,
            next_layer_push_phase_ns
        );
        metrics::histogram!(
            crate::rust::metrics_constants::FINALIZER_RUN_TIME_METRIC,
            "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
        )
        .record(total_started.elapsed().as_secs_f64());

        Ok(lfb_result)
    }
}
