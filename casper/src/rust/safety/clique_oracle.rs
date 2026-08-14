// See casper/src/main/scala/coop/rchain/casper/safety/CliqueOracle.scala

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block_hash::BlockHash;
use models::rust::validator::Validator;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::safety_oracle::MIN_FAULT_TOLERANCE;
use crate::rust::util::clique::Clique;

pub struct CliqueOracle;

type M = BlockHash; // type for message
type V = Validator; // type for message creator/validator
type WeightMap = HashMap<V, i64>; // stakes per message creator
const COOPERATIVE_YIELD_CHECK_INTERVAL: usize = 8;
const COOPERATIVE_YIELD_TIMESLICE_MS: u64 = 1;
const MAX_SELF_JUSTIFICATION_CACHE_ENTRIES: usize = 10_000;
const MAX_ANCESTOR_CACHE_ENTRIES: usize = 10_000;

/// Denominator of the on-chain fault-tolerance threshold, in parts-per-million
/// (ppm). The threshold θ is stored on-chain as an `i64` ppm numerator with this
/// fixed denominator, so θ = num / 1_000_000.
pub const FT_PPM_DEN: i64 = 1_000_000;

/// Fault-tolerance threshold θ = `num`/`den`, kept as an exact rational.
///
/// Sourced from the on-chain PoS ppm value (`den` = [`FT_PPM_DEN`], `num` = ppm).
/// Held exact — never collapsed to `f32` — so the finalization DECISION is
/// integer-exact; the `f32` form survives only for display.
#[derive(Debug, Clone, Copy)]
pub struct FtThreshold {
    pub num: i64,
    pub den: i64,
}

impl FtThreshold {
    /// Exact threshold from an on-chain ppm numerator (θ = ppm / 1_000_000).
    pub fn from_ppm(ppm: i64) -> Self {
        Self {
            num: ppm,
            den: FT_PPM_DEN,
        }
    }

    /// LOSSY threshold from an `f32` (num = round(t · 1e6), den = 1e6). For tests
    /// and back-compat call sites only; the exact path is [`FtThreshold::from_ppm`].
    /// Mirrors `ProofOfStake::fault_tolerance_threshold_to_ppm`.
    pub fn from_f32_lossy(t: f32) -> Self {
        Self {
            num: ((t as f64) * FT_PPM_DEN as f64).round() as i64,
            den: FT_PPM_DEN,
        }
    }
}

/// Exact rational finalization test: θ = num/den.
///
/// `strict=false` ⇒ (2q−S)/S ≥ θ (floor path); `strict=true` ⇒ > θ (LFB
/// finalizer). Cleared of denominators (S, den > 0), the test is
/// `2·q·den ⋛ S·(den+num)`. `i128` so `2·q·den` and `S·(den+num)` (≤ ~2^84 for
/// S ≤ i64::MAX, den = 10^6) never overflow, and `2·agreeing` near i64::MAX
/// stays exact.
///
/// A9 atomic activation: this exact DECISION replaces the imprecise `f32`
/// comparison and activates ATOMICALLY with the unreleased branch — every
/// validator runs the branch binary together, so there is no mixed-version
/// window and no on-chain activation parameter is required.
pub fn ft_decides_exact(agreeing: i64, q: i64, s: i64, num: i64, den: i64, strict: bool) -> bool {
    // Domain: the doc-contract is 0 ≤ num ≤ den (θ ∈ [0,1]), but the on-chain ppm
    // is range-checked to [-den, den] (token_metadata_check.rs) and some callers
    // pass a negative sentinel θ (e.g. -1.0 "finalize on any majority clique"), so
    // the lower bound is -den here. The comparison math below is unchanged and is
    // exact across the full [-den, den] range.
    debug_assert!(den > 0 && s > 0 && (0..=s).contains(&q) && (-den..=den).contains(&num));
    if (agreeing as i128) * 2 <= s as i128 {
        return false; // agreeing ≤ S/2 ⇒ MIN ⇒ not finalized
    }
    let lhs = 2i128 * q as i128 * den as i128;
    let rhs = s as i128 * (den as i128 + num as i128);
    if strict {
        lhs > rhs
    } else {
        lhs >= rhs
    }
}

pub struct CliqueOracleRunCache {
    latest_justifications_cache: BTreeMap<V, BTreeMap<V, M>>,
    self_justification_cache: BTreeMap<M, Option<M>>,
    ancestor_cache: BTreeMap<(M, M), bool>,
    yield_check_interval: usize,
    yield_timeslice: Duration,
    max_self_justification_cache_entries: usize,
    max_ancestor_cache_entries: usize,
}

impl CliqueOracle {
    pub fn new_run_cache() -> CliqueOracleRunCache {
        CliqueOracleRunCache {
            latest_justifications_cache: BTreeMap::new(),
            self_justification_cache: BTreeMap::new(),
            ancestor_cache: BTreeMap::new(),
            yield_check_interval: COOPERATIVE_YIELD_CHECK_INTERVAL,
            yield_timeslice: Duration::from_millis(COOPERATIVE_YIELD_TIMESLICE_MS),
            max_self_justification_cache_entries: MAX_SELF_JUSTIFICATION_CACHE_ENTRIES,
            max_ancestor_cache_entries: MAX_ANCESTOR_CACHE_ENTRIES,
        }
    }

    fn bounded_cache_insert<K: Ord + Clone, V>(
        map: &mut BTreeMap<K, V>,
        key: K,
        value: V,
        max_entries: usize,
    ) {
        if max_entries > 0 && !map.contains_key(&key) && map.len() >= max_entries {
            if let Some(first_key) = map.keys().next().cloned() {
                map.remove(&first_key);
            }
        }
        map.insert(key, value);
    }

    /// weight map of main parent (fallbacks to message itself if no parents)
    /// TODO - why not use local weight map but seek for parent?
    /// P.S. This is related to the fact that we create latest message for newly bonded validator
    /// equal to message where bonding deploy has been submitted. So stake from validator that did not create anything is
    /// put behind this message. So here is one more place where this logic makes things more complex.
    pub async fn get_corresponding_weight_map(
        target_msg: &M,
        dag: &KeyValueDagRepresentation,
    ) -> Result<WeightMap, KvStoreError> {
        dag.lookup_unsafe(target_msg)
            .and_then(|meta| match meta.parents.first() {
                Some(main_parent) => dag
                    .lookup_unsafe(main_parent)
                    .map(|parent_meta| parent_meta.weight_map.into_iter().collect()),
                None => Ok(meta.weight_map.into_iter().collect()),
            })
    }

    /// If two validators will never have disagreement on target message
    ///
    /// Prerequisite for this is that latest messages from a and b both are in main chain with target message
    ///
    /// ```text
    ///     a    b
    ///     *    *  <- lmB
    ///      \   *
    ///       \  *
    ///        \ *
    ///         \*
    ///          *  <- lmAjB
    /// ```
    ///
    /// 1. get justification of validator b as per latest message of a (lmAjB)
    /// 2. check if any self justifications between latest message of b (lmB) and lmAjB are NOT in main chain
    ///    with target message.
    ///
    ///    If one found - this is a source of disagreement.
    async fn never_eventually_see_disagreement(
        lm_b: &M,
        lm_a_j_b: &M,
        dag: &KeyValueDagRepresentation,
        target_msg: &M,
        yield_check_interval: usize,
        yield_timeslice: Duration,
        self_justification_cache: &mut BTreeMap<M, Option<M>>,
        ancestor_cache: &mut BTreeMap<(M, M), bool>,
        max_self_justification_cache_entries: usize,
        max_ancestor_cache_entries: usize,
    ) -> Result<bool, KvStoreError> {
        /// Check if there might be eventual disagreement between validators
        async fn might_eventually_disagree(
            lm_b: &M,
            lm_a_j_b: &M,
            dag: &KeyValueDagRepresentation,
            target_msg: &M,
            self_justification_cache: &mut BTreeMap<M, Option<M>>,
            ancestor_cache: &mut BTreeMap<(M, M), bool>,
            yield_check_interval: usize,
            yield_timeslice: Duration,
            max_self_justification_cache_entries: usize,
            max_ancestor_cache_entries: usize,
        ) -> Result<bool, KvStoreError> {
            // self justification of lmAjB or lmAjB itself. Used as a stopper for traversal
            // TODO not completely clear why try to use self justification and not just message itself
            let stopper = if let Some(cached) = self_justification_cache.get(lm_a_j_b) {
                cached.clone().unwrap_or_else(|| lm_a_j_b.clone())
            } else {
                let value = dag.self_justification(lm_a_j_b)?;
                CliqueOracle::bounded_cache_insert(
                    self_justification_cache,
                    lm_a_j_b.clone(),
                    value.clone(),
                    max_self_justification_cache_entries,
                );
                value.unwrap_or_else(|| lm_a_j_b.clone())
            };

            // Traverse only until stopper instead of materializing full history to genesis.
            let mut current = if let Some(cached) = self_justification_cache.get(lm_b) {
                cached.clone()
            } else {
                let value = dag.self_justification(lm_b)?;
                CliqueOracle::bounded_cache_insert(
                    self_justification_cache,
                    lm_b.clone(),
                    value.clone(),
                    max_self_justification_cache_entries,
                );
                value
            };
            let mut last_yield = Instant::now();
            let mut idx: usize = 0;
            while let Some(hash) = current {
                if hash == stopper {
                    break;
                }
                if idx.is_multiple_of(yield_check_interval)
                    && last_yield.elapsed() >= yield_timeslice
                {
                    tokio::task::yield_now().await;
                    last_yield = Instant::now();
                }
                idx += 1;
                // Main-chain membership, matching `agree`: a
                // self-justification of b whose SPINE does not pass through
                // `target_msg` is a genuine divergence — b's chain left (or
                // never held) the candidate, which is exactly the
                // fork-choice flip the clique must not contain. Merging the
                // target on a secondary parent is not agreement; counting
                // it as such let both sides of a conflicting sibling pair
                // keep full mutual cliques and certify together (the ucc
                // 00e6a2e3 consensus halt). Matches the Scala reference
                // (`dag.isInMainChain(targetMsg, j.latestBlockHash)`).
                // (`ancestor_cache` memoizes the spine verdict, keyed by
                // (target, hash).)
                let ancestor_key = (target_msg.clone(), hash.clone());
                let target_on_spine = if let Some(cached) = ancestor_cache.get(&ancestor_key) {
                    *cached
                } else {
                    let value = dag.is_in_main_chain(target_msg, &hash)?;
                    CliqueOracle::bounded_cache_insert(
                        ancestor_cache,
                        ancestor_key,
                        value,
                        max_ancestor_cache_entries,
                    );
                    value
                };
                if !target_on_spine {
                    return Ok(true);
                }

                current = if let Some(cached) = self_justification_cache.get(&hash) {
                    cached.clone()
                } else {
                    let value = dag.self_justification(&hash)?;
                    CliqueOracle::bounded_cache_insert(
                        self_justification_cache,
                        hash,
                        value.clone(),
                        max_self_justification_cache_entries,
                    );
                    value
                };
            }
            Ok(false)
        }

        might_eventually_disagree(
            lm_b,
            lm_a_j_b,
            dag,
            target_msg,
            self_justification_cache,
            ancestor_cache,
            yield_check_interval,
            yield_timeslice,
            max_self_justification_cache_entries,
            max_ancestor_cache_entries,
        )
        .await
        .map(|r| !r)
    }

    async fn compute_max_clique_weight(
        target_msg: &M,
        agreeing_weight_map: &WeightMap,
        dag: &KeyValueDagRepresentation,
        run_cache: &mut CliqueOracleRunCache,
        latest_messages: &BTreeMap<V, M>,
    ) -> Result<i64, KvStoreError> {
        let __compute_start = std::time::Instant::now();
        // Using tracing events for async - Span[F].traceI("compute-max-clique-weight") from Scala
        tracing::debug!(target: "f1r3fly.casper.safety.clique_oracle", "compute-max-clique-weight-started");
        /// across combination of validators compute pairs that do not have disagreement
        async fn compute_agreeing_validator_pairs(
            target_msg: &M,
            agreeing_weight_map: &WeightMap,
            dag: &KeyValueDagRepresentation,
            run_cache: &mut CliqueOracleRunCache,
            latest_messages: &BTreeMap<V, M>,
        ) -> Result<Vec<(V, V)>, KvStoreError> {
            let yield_check_interval = run_cache.yield_check_interval;
            let yield_timeslice = run_cache.yield_timeslice;

            let agreeing_validators: BTreeSet<V> = agreeing_weight_map.keys().cloned().collect();
            let mut latest_justifications_cache: BTreeMap<V, BTreeMap<V, M>> = BTreeMap::new();
            let mut pairwise_latest_messages: BTreeMap<V, M> = BTreeMap::new();
            // Conservative pruning: if validator has no latest message or no justifications
            // to other agreeing validators, it cannot form an agreeing edge with anyone.
            let mut pairwise_validators: Vec<V> = Vec::new();
            for validator in agreeing_validators.iter() {
                // Determinism lever: resolve "validator V's latest message" from the
                // frozen snapshot, never from the live DAG (dag.latest_message_hash is
                // the sole node-divergent input to the oracle).
                let Some(latest) = latest_messages.get(validator).cloned() else {
                    continue;
                };
                pairwise_latest_messages.insert(validator.clone(), latest.clone());

                let all_justifications =
                    if let Some(cached) = run_cache.latest_justifications_cache.get(validator) {
                        cached.clone()
                    } else {
                        let metadata = dag.lookup_unsafe(&latest)?;
                        let all: BTreeMap<V, M> = metadata
                            .justifications
                            .iter()
                            .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
                            .collect();
                        run_cache
                            .latest_justifications_cache
                            .insert(validator.clone(), all.clone());
                        all
                    };

                let relevant_justifications: BTreeMap<V, M> = all_justifications
                    .iter()
                    .filter_map(|(validator, hash)| {
                        agreeing_validators
                            .contains(validator)
                            .then_some((validator.clone(), hash.clone()))
                    })
                    .collect();
                if relevant_justifications.is_empty() {
                    continue;
                }

                latest_justifications_cache.insert(validator.clone(), relevant_justifications);
                pairwise_validators.push(validator.clone());
            }

            let mut result = Vec::new();
            let mut last_yield = Instant::now();
            let mut pair_idx: usize = 0;
            for i in 0..pairwise_validators.len() {
                for j in (i + 1)..pairwise_validators.len() {
                    // Keep this loop cooperative so higher-level timeouts can preempt
                    // expensive clique evaluation on deep DAGs.
                    if pair_idx.is_multiple_of(yield_check_interval)
                        && last_yield.elapsed() >= yield_timeslice
                    {
                        tokio::task::yield_now().await;
                        last_yield = Instant::now();
                    }
                    pair_idx += 1;

                    let a = &pairwise_validators[i];
                    let b = &pairwise_validators[j];
                    // Both directions of latest-message justification must exist, otherwise
                    // disagreement check returns false immediately and pair cannot be in clique.
                    let Some(lm_a_j_b) = latest_justifications_cache.get(a).and_then(|m| m.get(b))
                    else {
                        continue;
                    };
                    let Some(lm_b_j_a) = latest_justifications_cache.get(b).and_then(|m| m.get(a))
                    else {
                        continue;
                    };
                    let Some(lm_a) = pairwise_latest_messages.get(a) else {
                        continue;
                    };
                    let Some(lm_b) = pairwise_latest_messages.get(b) else {
                        continue;
                    };
                    let no_a_b_disagreement = CliqueOracle::never_eventually_see_disagreement(
                        lm_b,
                        lm_a_j_b,
                        dag,
                        target_msg,
                        yield_check_interval,
                        yield_timeslice,
                        &mut run_cache.self_justification_cache,
                        &mut run_cache.ancestor_cache,
                        run_cache.max_self_justification_cache_entries,
                        run_cache.max_ancestor_cache_entries,
                    )
                    .await?;
                    let no_b_a_disagreement = CliqueOracle::never_eventually_see_disagreement(
                        lm_a,
                        lm_b_j_a,
                        dag,
                        target_msg,
                        yield_check_interval,
                        yield_timeslice,
                        &mut run_cache.self_justification_cache,
                        &mut run_cache.ancestor_cache,
                        run_cache.max_self_justification_cache_entries,
                        run_cache.max_ancestor_cache_entries,
                    )
                    .await?;

                    if no_a_b_disagreement && no_b_a_disagreement {
                        result.push((a.clone(), b.clone()));
                    }
                }
            }

            Ok(result)
        }

        let edges = compute_agreeing_validator_pairs(
            target_msg,
            agreeing_weight_map,
            dag,
            run_cache,
            latest_messages,
        )
        .await?;
        let max_weight = Clique::find_maximum_clique_by_weight(&edges, agreeing_weight_map);

        metrics::histogram!(
            crate::rust::metrics_constants::CLIQUE_ORACLE_COMPUTE_TIME_METRIC,
            "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
        )
        .record(__compute_start.elapsed().as_secs_f64());
        Ok(max_weight)
    }

    pub async fn compute_output_with_cache(
        target_msg: &M,
        message_weight_map: &WeightMap,
        agreeing_weight_map: &WeightMap,
        dag: &KeyValueDagRepresentation,
        run_cache: &mut CliqueOracleRunCache,
        latest_messages: &BTreeMap<V, M>,
    ) -> Result<f32, KvStoreError> {
        let total_stake = message_weight_map.values().sum::<i64>() as f32;
        assert!(
            total_stake > 0.0,
            "Long overflow when computing total stake"
        );

        // If less than 1/2+ of stake agrees on message - it can be orphaned
        if (agreeing_weight_map.values().sum::<i64>() as f32) <= total_stake / 2.0 {
            Ok(MIN_FAULT_TOLERANCE)
        } else {
            let max_clique_weight = CliqueOracle::compute_max_clique_weight(
                target_msg,
                agreeing_weight_map,
                dag,
                run_cache,
                latest_messages,
            )
            .await? as f32;

            let result = (max_clique_weight * 2.0 - total_stake) / total_stake;

            Ok(result)
        }
    }

    pub async fn compute_output(
        target_msg: &M,
        message_weight_map: &WeightMap,
        agreeing_weight_map: &WeightMap,
        dag: &KeyValueDagRepresentation,
        latest_messages: &BTreeMap<V, M>,
    ) -> Result<f32, KvStoreError> {
        let mut run_cache = Self::new_run_cache();
        Self::compute_output_with_cache(
            target_msg,
            message_weight_map,
            agreeing_weight_map,
            dag,
            &mut run_cache,
            latest_messages,
        )
        .await
    }

    /// Weight map restricted to validators that AGREE on `target_msg` over the
    /// frozen `latest_messages` snapshot. Shared by [`CliqueOracle::ft_witnessed`]
    /// (f32 display) and [`CliqueOracle::ft_witnessed_exact`] (integer DECISION)
    /// so both read agreement identically.
    async fn agreeing_weight_map(
        weight_map: &WeightMap,
        target_msg: &M,
        dag: &KeyValueDagRepresentation,
        latest_messages: &BTreeMap<V, M>,
    ) -> Result<WeightMap, KvStoreError> {
        async fn agree(
            validator: &V,
            message: &M,
            dag: &KeyValueDagRepresentation,
            latest_messages: &BTreeMap<V, M>,
        ) -> Result<bool, KvStoreError> {
            // Agreement is CHAIN CHOICE, not merging: a validator agrees
            // with `message` iff `message` is on the MAIN-PARENT SPINE of
            // its latest message. A spine passes through exactly one block
            // per height, so agreement is exclusive between same-height
            // siblings — the property the clique theorem certifies (the
            // estimator can never move off the candidate without >θ weight
            // faulting). DAG ancestry is the wrong relation in a merging
            // DAG: every block is eventually merged by everyone, so
            // ancestry-agreement saturates and certifies BOTH sides of a
            // conflicting sibling pair — two finalized floors then freeze
            // at one height and every join derivation refuses forever (the
            // ucc 00e6a2e3 consensus halt). Matches the Scala reference
            // (CliqueOracle.scala `dag.isInMainChain(targetMsg, ...)`).
            latest_messages
                .get(validator)
                .map_or(Ok(false), |hash| dag.is_in_main_chain(message, hash))
        }

        let mut agreeing_map = HashMap::new();
        for (validator, weight) in weight_map.iter() {
            if agree(validator, target_msg, dag, latest_messages).await? {
                agreeing_map.insert(validator.clone(), *weight);
            }
        }

        Ok(agreeing_map)
    }

    /// EXACT deterministic finalization DECISION over a FROZEN snapshot — the
    /// integer-exact analog of [`CliqueOracle::ft_witnessed`]. Returns `true` iff
    /// the clique oracle certifies `target_msg` finalized at threshold `ftt` under
    /// the exact rule `2·q·den ⋛ S·(den+num)` (see [`ft_decides_exact`]); `strict`
    /// selects ≥ (floor path) vs > (LFB finalizer). Mirrors `ft_witnessed`'s
    /// contains / zero-stake / `agreeing ≤ S/2` short-circuits so the two agree
    /// everywhere except at the `f32` rounding boundary this replaces.
    pub async fn ft_witnessed_exact(
        target_msg: &M,
        dag: &KeyValueDagRepresentation,
        latest_messages: &BTreeMap<V, M>,
        ftt: FtThreshold,
        strict: bool,
    ) -> Result<bool, KvStoreError> {
        // Non-existing message: MIN ⇒ not finalized (mirrors ft_witnessed).
        if !dag.contains(target_msg) {
            tracing::warn!(
                ?target_msg,
                "Exact fault tolerance for non existing message requested."
            );
            return Ok(false);
        }
        let full_weight_map = CliqueOracle::get_corresponding_weight_map(target_msg, dag).await?;
        let total_stake = full_weight_map.values().sum::<i64>();
        // Zero (or negative) total stake cannot witness anything — mirrors the
        // ft_witnessed guard that returns MIN instead of asserting a positive total.
        if total_stake <= 0 {
            return Ok(false);
        }
        let agreeing_weight_map =
            CliqueOracle::agreeing_weight_map(&full_weight_map, target_msg, dag, latest_messages)
                .await?;
        let agreeing = agreeing_weight_map.values().sum::<i64>();
        // agreeing ≤ S/2 ⇒ MIN ⇒ not finalized. Short-circuit BEFORE the expensive
        // clique search, exactly as compute_output_with_cache does.
        if (agreeing as i128) * 2 <= total_stake as i128 {
            return Ok(false);
        }
        let mut run_cache = Self::new_run_cache();
        let max_clique_weight = CliqueOracle::compute_max_clique_weight(
            target_msg,
            &agreeing_weight_map,
            dag,
            &mut run_cache,
            latest_messages,
        )
        .await?;
        let decision = ft_decides_exact(
            agreeing,
            max_clique_weight,
            total_stake,
            ftt.num,
            ftt.den,
            strict,
        );
        if tracing::enabled!(target: "f1r3.trace.oracle", tracing::Level::DEBUG) {
            let snapshot: Vec<String> = latest_messages
                .iter()
                .map(|(v, m)| {
                    format!(
                        "{}=>{}",
                        hex::encode(&v[..4.min(v.len())]),
                        hex::encode(&m[..8.min(m.len())])
                    )
                })
                .collect();
            let agreeing_validators: Vec<String> = agreeing_weight_map
                .iter()
                .map(|(v, w)| format!("{}:{}", hex::encode(&v[..4.min(v.len())]), w))
                .collect();
            tracing::debug!(
                target: "f1r3.trace.oracle",
                tgt = %hex::encode(&target_msg[..8.min(target_msg.len())]),
                agreeing,
                max_clique_weight,
                total_stake,
                decision,
                snapshot = ?snapshot,
                agreeing_validators = ?agreeing_validators,
                "ft_witnessed_exact verdict"
            );
        }
        Ok(decision)
    }

    /// Deterministic fault tolerance over a FROZEN latest-message snapshot.
    ///
    /// Every "validator V's latest message" read is resolved from `latest_messages`
    /// instead of the live DAG (dag.latest_message_hash is the sole node-divergent
    /// input to the oracle). When `latest_messages` is taken from a candidate block's
    /// signed justification set, the result is a pure function of that block's bytes
    /// plus immutable ancestor metadata — bit-identical across honest nodes.
    pub async fn ft_witnessed(
        target_msg: &M,
        dag: &KeyValueDagRepresentation,
        latest_messages: &BTreeMap<V, M>,
    ) -> Result<f32, KvStoreError> {
        // Using tracing events for async - Span[F].traceI("normalized-fault-tolerance") from Scala
        tracing::debug!(target: "f1r3fly.casper.safety.clique_oracle", "normalized-fault-tolerance-started");

        if dag.contains(target_msg) {
            tracing::debug!("Calculating fault tolerance for {:?}.", target_msg);
            let full_weight_map =
                CliqueOracle::get_corresponding_weight_map(target_msg, dag).await?;
            // A weight map with no positive stake cannot witness anything.
            // `compute_output` asserts a positive total; a block whose metadata
            // carries zero-stake bonds must yield MIN, not panic — this path
            // runs on arbitrary received blocks via floor derivation.
            if full_weight_map.values().sum::<i64>() <= 0 {
                return Ok(MIN_FAULT_TOLERANCE);
            }
            let agreeing_weight_map =
                Self::agreeing_weight_map(&full_weight_map, target_msg, dag, latest_messages)
                    .await?;
            let result = CliqueOracle::compute_output(
                target_msg,
                &full_weight_map,
                &agreeing_weight_map,
                dag,
                latest_messages,
            )
            .await?;

            tracing::debug!(
                target: "f1r3fly.casper.safety.clique_oracle",
                ft = result,
                agreeing_weight = agreeing_weight_map.values().sum::<i64>(),
                total_weight = full_weight_map.values().sum::<i64>(),
                agreeing_n = agreeing_weight_map.len(),
                total_n = full_weight_map.len(),
                target_msg = ?target_msg,
                "normalized-fault-tolerance-computed"
            );

            Ok(result)
        } else {
            tracing::warn!(
                ?target_msg,
                "Fault tolerance for non existing message requested."
            );
            Ok(MIN_FAULT_TOLERANCE)
        }
    }

    /// Live-DAG fault tolerance: snapshots the DAG's current latest messages and
    /// delegates to [`ft_witnessed`]. Behaviour is identical to the historical
    /// implementation — the snapshot is the same `dag.latest_message_hashes()` source
    /// the oracle read directly before.
    pub async fn normalized_fault_tolerance(
        target_msg: &M,
        dag: &KeyValueDagRepresentation,
    ) -> Result<f32, KvStoreError> {
        // Using tracing events for async - Span[F].traceI("normalized-fault-tolerance") from Scala
        tracing::debug!(target: "f1r3fly.casper.safety.clique_oracle", "normalized-fault-tolerance-started");
        let latest_messages: BTreeMap<V, M> = dag.latest_message_hashes().into_iter().collect();
        CliqueOracle::ft_witnessed(target_msg, dag, &latest_messages).await
    }
}

#[cfg(test)]
mod ft_decides_exact_tests {
    //! Unit tests for the exact-integer finalization DECISION (`ft_decides_exact`)
    //! and the `FtThreshold` newtype. The exact test replaces the imprecise `f32`
    //! comparison `(2q−S)/S ⋛ θ`; these confirm it matches `f32` where `f32` is
    //! exact, pins the ≥-vs-> semantics at an exact tie, and never overflows.
    use super::{ft_decides_exact, FtThreshold, FT_PPM_DEN};

    /// On small stakes the exact rule matches the naive `f32` comparison
    /// `(2q−S)/S ⋛ θ` at every grid point where `f32` is exact (dyadic S and θ),
    /// confirming the DECISION is unchanged on the common case. Exact ties are
    /// excluded here and pinned down by `boundary_tie_ge_finalizes_gt_does_not`.
    #[test]
    fn small_stake_matches_f32_rule_on_dyadic_grid() {
        let den = FT_PPM_DEN;
        // θ ∈ {0, ¼, ½, ¾, 1}, all exactly representable in f32.
        for &num in &[0i64, 250_000, 500_000, 750_000, 1_000_000] {
            let theta = num as f32 / den as f32;
            // Powers of two ⇒ (2q−S)/S is dyadic ⇒ exact in f32.
            for &s in &[1i64, 2, 4, 8, 16] {
                for q in 0..=s {
                    let lhs = 2i128 * q as i128 * den as i128;
                    let rhs = s as i128 * (den as i128 + num as i128);
                    if lhs == rhs {
                        continue; // exact tie: covered by the boundary test
                    }
                    let ratio = (2.0 * q as f32 - s as f32) / s as f32;
                    // Full agreement (agreeing = S) so the >S/2 gate always passes,
                    // isolating the θ comparison.
                    assert_eq!(
                        ft_decides_exact(s, q, s, num, den, false),
                        ratio >= theta,
                        ">= mismatch q={q} s={s} num={num}"
                    );
                    assert_eq!(
                        ft_decides_exact(s, q, s, num, den, true),
                        ratio > theta,
                        "> mismatch q={q} s={s} num={num}"
                    );
                }
            }
        }
    }

    /// A large-stake exact tie `2q·den == S·(den+num)` finalizes under ≥ (floor)
    /// but NOT under > (LFB finalizer) — the precise boundary the `f32` path could
    /// not resolve. Built so the tie is exact: S = 2·den, q = den+num ⇒
    /// 2q·den = 2(den+num)·den = S·(den+num).
    #[test]
    fn boundary_tie_ge_finalizes_gt_does_not() {
        let den = FT_PPM_DEN;
        let num = 333_333;
        let s = 2 * den; // 2_000_000
        let q = den + num; // 1_333_333
        assert_eq!(
            2i128 * q as i128 * den as i128,
            s as i128 * (den as i128 + num as i128),
            "test setup must produce an exact tie"
        );
        // agreeing = S so the >S/2 gate passes; the tie is decided purely by strict.
        assert!(
            ft_decides_exact(s, q, s, num, den, false),
            "exact tie must finalize under >= (floor)"
        );
        assert!(
            !ft_decides_exact(s, q, s, num, den, true),
            "exact tie must NOT finalize under > (LFB finalizer)"
        );
    }

    /// The `2·agreeing == S` knife-edge (exactly half agree) is NOT finalized: the
    /// oracle requires strictly more than half agreeing stake, so even a full
    /// clique q = S and the most permissive θ = 0 return false.
    #[test]
    fn exactly_half_agreeing_is_not_finalized() {
        let den = FT_PPM_DEN;
        let s = 10i64;
        let agreeing = 5i64; // 2·5 == 10 == S
        assert!(!ft_decides_exact(agreeing, s, s, 0, den, false));
        assert!(!ft_decides_exact(agreeing, s, s, 0, den, true));
    }

    /// i64::MAX-scale q and S must not overflow: `2·q·den ≈ 2^84` exceeds i64 but
    /// is exact in i128, and `2·agreeing` near i64::MAX also needs i128.
    #[test]
    fn no_overflow_at_i64_max_scale() {
        let den = FT_PPM_DEN;
        let num = 500_000;
        let s = i64::MAX;
        let q = i64::MAX; // q ≤ S; full clique ⇒ (2S−S)/S = 1 ≥ θ and > θ (θ < 1)
        assert!(ft_decides_exact(s, q, s, num, den, false));
        assert!(ft_decides_exact(s, q, s, num, den, true));
        // 2·agreeing near i64::MAX in the early-return path: 2·(MAX/2) < MAX ⇒ false.
        let half = i64::MAX / 2;
        assert!(!ft_decides_exact(half, q, s, num, den, true));
    }

    /// Negative-θ sentinel (θ = -1.0 "finalize on any majority clique"): with the
    /// >S/2 gate satisfied, any q > 0 finalizes, and q == 0 does not.
    #[test]
    fn negative_sentinel_threshold_finalizes_any_positive_clique() {
        let den = FT_PPM_DEN;
        let num = -1_000_000; // θ = -1.0
        let s = 10i64;
        let agreeing = 10i64; // 2·10 > 10 ⇒ gate passes
        assert!(
            ft_decides_exact(agreeing, 1, s, num, den, true),
            "q=1>0 finalizes"
        );
        assert!(
            !ft_decides_exact(agreeing, 0, s, num, den, true),
            "q=0 does not"
        );
    }

    #[test]
    fn ft_threshold_constructors() {
        assert_eq!(FtThreshold::from_ppm(330_000).num, 330_000);
        assert_eq!(FtThreshold::from_ppm(330_000).den, FT_PPM_DEN);
        // from_f32_lossy mirrors ProofOfStake::fault_tolerance_threshold_to_ppm.
        assert_eq!(FtThreshold::from_f32_lossy(0.5).num, 500_000);
        assert_eq!(FtThreshold::from_f32_lossy(-1.0).num, -1_000_000);
        assert_eq!(FtThreshold::from_f32_lossy(-1.0).den, FT_PPM_DEN);
    }
}
