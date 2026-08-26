// See casper/src/main/scala/coop/rchain/casper/finality/Finalizer.scala

use std::collections::HashMap;
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::validator::Validator;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::errors::CasperError;
use crate::rust::finality::floor::{
    is_state_preserved, latest_message_coverage_above, materialize_finalized_floor,
    state_witnessed_exact,
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
/// if it has more then half of total stake in bonds map of A translated from tips throughout causal ancestry.
///
/// Therefore: Finalizer should seek for next LFB going through 2 steps:
///   1. Find messages in scope of search that have more then half of the stake translated through all parent edges
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
        new_lfb_found_effect: F,
        finalizer_conf: &crate::rust::casper_conf::FinalizerConf,
    ) -> Result<Option<(BlockHash, f32)>, CasperError>
    where
        F: FnMut((BlockHash, f32)) -> Fut,
        Fut: std::future::Future<Output = Result<(), KvStoreError>>,
    {
        let context =
            crate::rust::causal_equivocation::CertifiedConsensusContext::for_finalized_floor(
                dag,
                curr_lfb_hash.clone(),
            )?;
        Self::run_with_context(
            dag,
            ftt,
            curr_lfb_hash,
            curr_lfb_height,
            &context,
            new_lfb_found_effect,
            finalizer_conf,
        )
        .await
    }

    pub async fn run_with_context<F, Fut>(
        dag: &KeyValueDagRepresentation,
        ftt: FtThreshold,
        curr_lfb_hash: &BlockHash,
        curr_lfb_height: i64,
        context: &crate::rust::causal_equivocation::CertifiedConsensusContext,
        mut new_lfb_found_effect: F,
        finalizer_conf: &crate::rust::casper_conf::FinalizerConf,
    ) -> Result<Option<(BlockHash, f32)>, CasperError>
    where
        F: FnMut((BlockHash, f32)) -> Fut,
        Fut: std::future::Future<Output = Result<(), KvStoreError>>,
    {
        if context.incoming_finalized_floor() != curr_lfb_hash
            || !context.has_complete_latest_message_slots()
        {
            return Err(CasperError::RuntimeError(
                "finalizer consensus context does not exactly bind the current finalized floor"
                    .to_string(),
            ));
        }
        let total_started = std::time::Instant::now();
        let lfb_lag = dag.latest_block_number().saturating_sub(curr_lfb_height);
        let catchup_mode = lfb_lag > FINALIZER_CATCHUP_LAG_THRESHOLD_BLOCKS;
        let yield_interval = if catchup_mode {
            finalizer_conf.catchup_yield_interval
        } else {
            finalizer_conf.yield_interval
        };
        let latest_messages_snapshot = context
            .vote_projection()
            .eligible_latest_messages()
            .iter()
            .map(|(validator, hash)| (validator.clone(), hash.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let latest_messages_count = latest_messages_snapshot.len();
        for latest in latest_messages_snapshot.values() {
            let materialize_started = std::time::Instant::now();
            materialize_finalized_floor(dag, latest, ftt).await?;
            if dag.get_cached_floor(latest)?.is_none() {
                return Err(CasperError::Other(format!(
                    "finalized-floor provenance was not materialized for latest message {}",
                    hex::encode(latest)
                )));
            }
            tracing::debug!(
                target: "f1r3fly.finalizer.timing",
                latest = %hex::encode(latest),
                elapsed_ms = materialize_started.elapsed().as_millis(),
                "Finalizer materialized latest-message finalized-floor provenance"
            );
        }

        let coverage_started = std::time::Instant::now();
        let latest_coverage =
            latest_message_coverage_above(dag, &latest_messages_snapshot, curr_lfb_height)?;
        let coverage_ms = coverage_started.elapsed().as_millis();
        let coverage_blocks = latest_coverage.len();
        let mut aggregated_agreements: HashMap<
            BlockHash,
            (BlockMetadata, SharedWeightMap, WeightMap),
        > = HashMap::with_capacity(coverage_blocks);
        let mut message_weight_map_error_count: usize = 0;
        let mut agreements_count: usize = 0;
        let mut weight_map_phase_ns: u128 = 0;
        let mut agreement_record_phase_ns: u128 = 0;
        let mut last_yield = std::time::Instant::now();
        for (block_hash, supporters) in latest_coverage {
            if last_yield.elapsed() >= yield_interval {
                tokio::task::yield_now().await;
                last_yield = std::time::Instant::now();
            }
            let message = dag.lookup_unsafe(&block_hash)?;
            if message.block_number <= curr_lfb_height {
                continue;
            }
            let phase_t = std::time::Instant::now();
            let message_weight_map = Arc::new(
                Self::message_weight_map_f(&message, dag)
                    .await
                    .inspect_err(|_| message_weight_map_error_count += 1)?,
            );
            weight_map_phase_ns += phase_t.elapsed().as_nanos();

            let phase_t = std::time::Instant::now();
            let agreeing_weight_map = supporters
                .into_iter()
                .filter_map(|validator| Self::record_agreement(&message_weight_map, &validator))
                .collect::<WeightMap>();
            agreements_count += agreeing_weight_map.len();
            agreement_record_phase_ns += phase_t.elapsed().as_nanos();
            aggregated_agreements.insert(
                block_hash,
                (message, message_weight_map, agreeing_weight_map),
            );
        }

        // Step 2: Filter blocks that cannot be orphaned and precompute sort keys.
        let mut ordered_agreements: Vec<(BlockMetadata, SharedWeightMap, WeightMap)> =
            aggregated_agreements
                .into_values()
                .filter_map(|(message, message_weight_map, agreeing_weight_map)| {
                    Self::cannot_be_orphaned(&message_weight_map, &agreeing_weight_map).then_some((
                        message,
                        message_weight_map,
                        agreeing_weight_map,
                    ))
                })
                .collect();
        let filtered_agreements_count = ordered_agreements.len();
        // Sort candidates by the same total order used by finalized-floor derivation.
        ordered_agreements.sort_by(|(msg_l, ..), (msg_r, ..)| {
            msg_r
                .block_number
                .cmp(&msg_l.block_number)
                .then_with(|| msg_r.block_hash.cmp(&msg_l.block_hash))
        });

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
            )
            .await?;

            if finalized {
                materialize_finalized_floor(dag, &message.block_hash, ftt).await?;
                if !is_state_preserved(dag, curr_lfb_hash, &message.block_hash)? {
                    tracing::debug!(
                        target: "f1r3fly.finalizer",
                        candidate = %hex::encode(&message.block_hash[..]),
                        current_lfb = %hex::encode(&curr_lfb_hash[..]),
                        "Finalizer candidate does not preserve the current LFB state effects"
                    );
                    continue;
                }
                if !state_witnessed_exact(dag, &message.block_hash, &latest_messages_snapshot, ftt)
                    .await?
                {
                    tracing::debug!(
                        target: "f1r3fly.finalizer",
                        candidate = %hex::encode(&message.block_hash[..]),
                        "Finalizer candidate lacks a state-preserving clique certificate"
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
            "Finalizer timing: latest_messages={}, coverage_blocks={}, coverage_ms={}, agreements={}, filtered_agreements={}, message_weight_map_errors={}, ranking_strategy={}, upper_bound_pruned={}, upper_bound_passed={}, max_ft_upper_bound={:.6}, clique_evals={}, clique_ms={}, total_ms={}, yield_interval_ms={}, lfb_lag={}, catchup_mode={}, found_new_lfb={}, weight_map_ns={}, agreement_ns={}",
            latest_messages_count,
            coverage_blocks,
            coverage_ms,
            agreements_count,
            filtered_agreements_count,
            message_weight_map_error_count,
            "recency_hash",
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
            agreement_record_phase_ns
        );
        metrics::histogram!(
            crate::rust::metrics_constants::FINALIZER_RUN_TIME_METRIC,
            "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
        )
        .record(total_started.elapsed().as_secs_f64());

        Ok(lfb_result)
    }
}
