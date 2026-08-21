use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use casper::rust::blocks::proposer::propose_result::{ProposeFailure, ProposeStatus};
use casper::rust::blocks::proposer::proposer::ProposerResult;
use casper::rust::casper::{CasperSnapshot, MultiParentCasper};
use casper::rust::casper_conf::HeartbeatConf;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::heartbeat_signal::{
    install_heartbeat_signal, HeartbeatSignal, HeartbeatSignalRef,
};
use casper::rust::system_deploy::is_system_deploy_id;
use casper::rust::validator_identity::ValidatorIdentity;
use casper::rust::ProposeFunction;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::validator::Validator;
use rand::Rng;
use tokio::sync::Notify;

/// Implementation of HeartbeatSignal using tokio::sync::Notify.
/// This allows external callers (like deploy submission) to wake the heartbeat immediately.
struct NotifyHeartbeatSignal {
    notify: Arc<Notify>,
}

impl HeartbeatSignal for NotifyHeartbeatSignal {
    fn trigger_wake(&self) { self.notify.notify_one(); }
}

/// Heartbeat proposer that periodically checks if a block
/// needs to be proposed to maintain liveness.
pub struct HeartbeatProposer;

#[derive(Debug, Clone, Copy, Default)]
struct HeartbeatCheckResult {
    bug_failure: bool,
    refresh_deploy_grace_window: bool,
    finality_recovery_attempted: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct EmptyFrontierPressure {
    unfinalized_blocks: usize,
    max_unfinalized_blocks: usize,
    backpressure: bool,
}

#[derive(Debug)]
struct FinalityProgress {
    last_finalized_block: Option<BlockHash>,
    last_progress_at: Instant,
    last_recovery_attempt_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
struct FinalityProgressStatus {
    stalled_for: Duration,
    stalled: bool,
    recovery_round_due: bool,
}

impl FinalityProgress {
    fn new(now: Instant) -> Self {
        Self {
            last_finalized_block: None,
            last_progress_at: now,
            last_recovery_attempt_at: None,
        }
    }

    fn observe(
        &mut self,
        last_finalized_block: &BlockHash,
        now: Instant,
        timeout: Duration,
    ) -> FinalityProgressStatus {
        if self.last_finalized_block.as_ref() != Some(last_finalized_block) {
            self.last_finalized_block = Some(last_finalized_block.clone());
            self.last_progress_at = now;
            self.last_recovery_attempt_at = None;
        }

        let stalled_for = now.saturating_duration_since(self.last_progress_at);
        let stalled = stalled_for >= timeout;
        let recovery_round_due = stalled && self.last_recovery_attempt_at.is_none();

        FinalityProgressStatus {
            stalled_for,
            stalled,
            recovery_round_due,
        }
    }

    fn record_recovery_attempt(&mut self, now: Instant) {
        self.last_recovery_attempt_at = Some(now);
    }
}

/// `self_recovery_throttled` must combine "ahead of the LFB" with "minted
/// within the stale-recovery interval": a validator that has been silent for
/// a full interval is exempt from the width cap, so a finalization stall can
/// never silence every validator permanently (the cap bounds churn to the
/// recovery cadence; it is not a proposal deadline the shard can miss forever).
/// `recovery_leader_window_open` widens the exemption for the selected lag
/// leader's one-shot recovery round while finality is stalled.
fn empty_frontier_pressure(
    snapshot: &CasperSnapshot,
    max_unfinalized_blocks: i64,
    has_pending_deploys: bool,
    has_new_parent_with_user_deploys: bool,
    deploy_grace_active: bool,
    self_recovery_throttled: bool,
    recovery_leader_window_open: bool,
) -> Result<EmptyFrontierPressure, casper::rust::errors::CasperError> {
    let max_unfinalized_blocks = usize::try_from(max_unfinalized_blocks).unwrap_or(usize::MAX);
    if has_pending_deploys
        || has_new_parent_with_user_deploys
        || deploy_grace_active
        || !self_recovery_throttled
        || recovery_leader_window_open
    {
        return Ok(EmptyFrontierPressure {
            max_unfinalized_blocks,
            ..EmptyFrontierPressure::default()
        });
    }

    let unfinalized_blocks = snapshot.dag.non_finalized_blocks()?.len();
    Ok(EmptyFrontierPressure {
        unfinalized_blocks,
        max_unfinalized_blocks,
        backpressure: unfinalized_blocks > max_unfinalized_blocks,
    })
}

impl HeartbeatProposer {
    /// Create a heartbeat proposer stream that periodically checks if a block
    /// needs to be proposed to maintain liveness.
    ///
    /// This integrates with the existing propose queue mechanism for thread safety.
    /// The heartbeat simply calls the same triggerPropose function that user deploys
    /// and explicit propose calls use, ensuring serialization through ProposerInstance.
    ///
    /// To prevent lock-step behavior between validators, the stream waits a random
    /// amount of time (0 to checkInterval) before starting the periodic checks.
    ///
    /// The heartbeat only runs on bonded validators. It checks the active validators
    /// set before proposing to avoid unnecessary attempts by unbonded nodes.
    ///
    /// # Arguments
    ///
    /// * `engine_cell` - The EngineCell to read the current Casper instance from
    /// * `trigger_propose_f` - The propose function that integrates with the propose queue
    /// * `validator_identity` - The validator identity to check if bonded
    /// * `config` - Heartbeat configuration (enabled, check_interval, max_lfb_age)
    /// * `max_number_of_parents` - Maximum number of parents allowed for blocks
    /// * `heartbeat_signal_ref` - Shared reference where the signal will be stored
    ///
    /// # Returns
    ///
    /// Returns `Some(JoinHandle)` when the heartbeat is spawned, or `None` when
    /// disabled, trigger function is not available, or max_number_of_parents == 1.
    ///
    /// # Safety
    ///
    /// Heartbeat requires max-number-of-parents > 1. With only 1 parent allowed,
    /// empty heartbeat blocks would fail InvalidParents validation when other
    /// validators have newer blocks.
    pub fn create(
        engine_cell: Arc<EngineCell>,
        trigger_propose_f: Option<Arc<ProposeFunction>>, // same queue/function used by user-triggered proposes
        validator_identity: ValidatorIdentity,
        config: HeartbeatConf,
        max_number_of_parents: i32,
        heartbeat_signal_ref: HeartbeatSignalRef,
        standalone: bool,
    ) -> Option<tokio::task::JoinHandle<()>> {
        // CRITICAL: Heartbeat cannot work with max-number-of-parents = 1
        // Empty blocks would fail InvalidParents validation when other validators have newer blocks
        if max_number_of_parents == 1 {
            tracing::error!(
                "\n\
============================================================================\n\
  CONFIGURATION ERROR: Heartbeat incompatible with max-number-of-parents=1\n\
============================================================================\n\
\n\
  The heartbeat proposer cannot function when max-number-of-parents is 1.\n\
  With single-parent mode, empty heartbeat blocks fail InvalidParents\n\
  validation when other validators have newer blocks, causing the shard\n\
  to stall after the first few blocks.\n\
\n\
  SOLUTION: Set max-number-of-parents to at least 3x your shard size.\n\
            Example: For a 3-validator shard, use max-number-of-parents = 9\n\
\n\
  The heartbeat thread is now DISABLED.\n\
  Your shard will NOT make automatic progress without user deploys.\n\
============================================================================"
            );
            return None;
        }

        if !config.enabled {
            tracing::warn!("Heartbeat: config is not enabled!");
            return None;
        }

        let trigger = match trigger_propose_f {
            Some(f) => f,
            None => {
                tracing::warn!("Heartbeat: trigger_propose function not available, skipping spawn");
                return None;
            }
        };

        // Create the signal mechanism using tokio::sync::Notify
        let notify = Arc::new(Notify::new());
        let signal: Arc<dyn HeartbeatSignal> = Arc::new(NotifyHeartbeatSignal {
            notify: notify.clone(),
        });

        // Store the signal in the shared reference so Casper can use it.
        if !install_heartbeat_signal(&heartbeat_signal_ref, signal) {
            tracing::warn!(
                "Heartbeat: signal ref already initialized; keeping existing signal handle"
            );
        }

        let initial_delay = random_initial_delay(config.check_interval);
        tracing::info!(
            "Heartbeat: Starting with random initial delay of {}s (check interval: {}s, max LFB age: {}s, signal-based wake enabled)",
            initial_delay.as_secs(),
            config.check_interval.as_secs(),
            config.max_lfb_age.as_secs()
        );

        let handle = tokio::spawn(async move {
            tokio::time::sleep(initial_delay).await;
            let mut consecutive_failures: u32 = 0;
            let mut backoff_until: Option<std::time::Instant> = None;
            let mut deploy_grace_until: Option<std::time::Instant> = None;
            let mut finality_progress = FinalityProgress::new(Instant::now());

            loop {
                // Race between timer and signal - whichever completes first triggers wake
                let wake_source = tokio::select! {
                    _ = tokio::time::sleep(config.check_interval) => "timer",
                    _ = notify.notified() => "signal",
                };

                tracing::debug!("Heartbeat: Woke from {}", wake_source);
                let eng = engine_cell.get().await;

                // Access Casper if available and run the check
                // Errors are logged but don't stop the heartbeat loop - transient errors
                // (DB contention, lock timeouts) should not kill the heartbeat
                if let Some(casper) = eng.with_casper() {
                    let now = std::time::Instant::now();
                    if backoff_until.is_some_and(|deadline| now < deadline) {
                        continue;
                    }
                    if deploy_grace_until.is_some_and(|deadline| now >= deadline) {
                        deploy_grace_until = None;
                    }
                    let deploy_grace_active = deploy_grace_until.is_some();

                    match do_heartbeat_check(
                        casper,
                        &*trigger,
                        &validator_identity,
                        &config,
                        standalone,
                        deploy_grace_active,
                        &mut finality_progress,
                    )
                    .await
                    {
                        Ok(outcome) => {
                            if outcome.refresh_deploy_grace_window {
                                let grace_ms = config.deploy_finalization_grace.as_millis();
                                let grace_duration = Duration::from_millis(std::cmp::min(
                                    grace_ms,
                                    u128::from(u64::MAX),
                                )
                                    as u64);
                                let deadline = std::time::Instant::now() + grace_duration;
                                deploy_grace_until = Some(deadline);
                                tracing::debug!(
                                    "Heartbeat: refreshed deploy finalization grace window for {:?}",
                                    grace_duration
                                );
                            }

                            if !outcome.bug_failure {
                                consecutive_failures = 0;
                                backoff_until = None;
                                continue;
                            }

                            consecutive_failures = consecutive_failures.saturating_add(1);
                            // Exponential backoff capped at 60s to avoid invalid-propose churn.
                            let shift = consecutive_failures.min(4);
                            let scale = 1u32 << shift;
                            let mut delay = config.check_interval.saturating_mul(scale);
                            let max_delay = Duration::from_secs(60);
                            if delay > max_delay {
                                delay = max_delay;
                            }
                            backoff_until = Some(std::time::Instant::now() + delay);
                            tracing::warn!(
                                "Heartbeat: Entering backoff for {:?} after {} consecutive failures",
                                delay,
                                consecutive_failures
                            );
                        }
                        Err(err) => {
                            tracing::warn!(
                                "Heartbeat: Check failed with error: {:?}, will retry next cycle",
                                err
                            );
                        }
                    }
                } else {
                    tracing::debug!("Heartbeat: Casper not available yet, skipping check");
                }
            }
        });

        Some(handle)
    }
}

fn random_initial_delay(check_interval: Duration) -> Duration {
    let max_millis = check_interval.as_millis() as u64;
    let random_millis = rand::rng().random_range(0..=max_millis);
    Duration::from_millis(random_millis)
}

/// Check if a heartbeat propose is needed and trigger one if so.
///
/// This is the core decision logic for heartbeat proposals. It:
/// 1. Gets the current Casper snapshot
/// 2. Checks if the validator is bonded
/// 3. Checks for pending deploys or stale LFB with new parents
/// 4. Triggers a propose if conditions are met
///
/// Exposed for testing - allows direct testing of decision logic without spawning tasks.
///
/// # Arguments
/// * `standalone` - If true, skips hasNewParents check (single validator can always propose)
async fn do_heartbeat_check(
    casper: Arc<dyn MultiParentCasper + Send + Sync>,
    trigger_propose: &ProposeFunction,
    validator_identity: &ValidatorIdentity,
    config: &HeartbeatConf,
    standalone: bool,
    deploy_grace_active: bool,
    finality_progress: &mut FinalityProgress,
) -> Result<HeartbeatCheckResult, casper::rust::errors::CasperError> {
    let snapshot: CasperSnapshot = casper.get_snapshot().await?;
    let progress_status = finality_progress.observe(
        &snapshot.last_finalized_block,
        Instant::now(),
        config.finality_progress_timeout,
    );

    let is_bonded = snapshot
        .parents
        .first()
        .map(|parent| {
            parent
                .body
                .state
                .bonds
                .iter()
                .any(|bond| bond.validator == validator_identity.public_key.bytes)
        })
        .unwrap_or(false);

    if !is_bonded {
        tracing::info!("Heartbeat: Validator is not bonded, skipping heartbeat propose");
        Ok(HeartbeatCheckResult::default())
    } else {
        tracing::debug!("Heartbeat: Validator is bonded, checking LFB age");
        let outcome = check_lfb_and_propose(
            casper.clone(),
            snapshot,
            trigger_propose,
            validator_identity,
            config,
            standalone,
            deploy_grace_active,
            progress_status,
        )
        .await?;
        if outcome.finality_recovery_attempted {
            finality_progress.record_recovery_attempt(Instant::now());
        }
        Ok(outcome)
    }
}

async fn check_lfb_and_propose(
    casper: Arc<dyn MultiParentCasper + Send + Sync>,
    snapshot: CasperSnapshot,
    trigger_propose: &ProposeFunction,
    validator_identity: &ValidatorIdentity,
    config: &HeartbeatConf,
    standalone: bool,
    deploy_grace_active: bool,
    finality_progress: FinalityProgressStatus,
) -> Result<HeartbeatCheckResult, casper::rust::errors::CasperError> {
    // Tuning thresholds for lag caps and recovery timing. Read once into
    // locals to keep the predicate sites below readable.
    let frontier_chase_max_lag = config.advanced.frontier_chase_max_lag;
    let pending_deploy_max_lag = config.advanced.pending_deploy_max_lag;
    let advanced_deploy_recovery_max_lag = config.advanced.deploy_recovery_max_lag;
    let stale_recovery_min_interval_ms = config.stale_recovery_min_interval.as_millis();

    // Check if we have pending user deploys in storage (not yet included in blocks)
    let has_pending_deploys = casper
        .has_pending_deploys_in_storage_for_snapshot(&snapshot)
        .await?;

    // Check if LFB is stale
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    // Avoid running heavyweight finalizer path from heartbeat loop.
    // Use the snapshot's latest finalized block hash and read it from block store directly.
    let lfb_timestamp_ms = match casper.block_store().get(&snapshot.last_finalized_block) {
        Ok(Some(lfb)) => lfb.header.timestamp as u128,
        Err(err) => {
            tracing::warn!(
                "Heartbeat: Failed to read latest finalized block {} for timestamp: {:?}, treating as stale",
                PrettyPrinter::build_string_bytes(&snapshot.last_finalized_block),
                err
            );
            0
        }
        Ok(None) => {
            tracing::warn!(
                "Heartbeat: Finalized block {} missing in block store, treating as stale",
                PrettyPrinter::build_string_bytes(&snapshot.last_finalized_block)
            );
            0
        }
    };
    let time_since_lfb = if now >= lfb_timestamp_ms {
        now - lfb_timestamp_ms
    } else {
        tracing::warn!(
            "LFB timestamp {} is in the future (now: {}), possible clock skew",
            lfb_timestamp_ms,
            now
        );
        0
    };
    let lfb_is_stale = time_since_lfb > config.max_lfb_age.as_millis();
    // Use latest observed frontier timestamp as a second staleness signal.
    // LFB may remain behind head under healthy operation; proposing recovery blocks
    // while the frontier is already active creates avoidable empty-block churn.
    let frontier_latest_timestamp_ms = snapshot
        .dag
        .latest_message_hashes()
        .iter()
        .filter_map(|(_, block_hash)| {
            casper
                .block_store()
                .get(block_hash)
                .ok()
                .flatten()
                .map(|block| block.header.timestamp as u128)
        })
        .max()
        .unwrap_or(lfb_timestamp_ms);
    let frontier_age_ms = now.saturating_sub(frontier_latest_timestamp_ms);
    let last_finalized_block_number = snapshot
        .dag
        .lookup(&snapshot.last_finalized_block)?
        .map(|meta| meta.block_number)
        .unwrap_or(0);
    let latest_block_number = snapshot.dag.latest_block_number();
    let lfb_lag_blocks = latest_block_number.saturating_sub(last_finalized_block_number);

    // If this validator is already ahead of finalized height, avoid repeatedly proposing
    // stale-LFB recovery blocks every heartbeat tick. Keep 1s heartbeat checks, but gate
    // recovery proposals unless finalized catches up or pending deploys exist.
    let self_recently_proposed = match (
        snapshot
            .dag
            .latest_message(&validator_identity.public_key.bytes)?,
        snapshot.dag.lookup(&snapshot.last_finalized_block)?,
    ) {
        (Some(latest_self), Some(last_finalized)) => {
            latest_self.block_number > last_finalized.block_number
        }
        _ => false,
    };
    let self_latest_block_timestamp_ms = snapshot
        .dag
        .latest_message_hash(&validator_identity.public_key.bytes)
        .and_then(|hash| match casper.block_store().get(&hash) {
            Ok(Some(block)) => Some(block.header.timestamp as u128),
            _ => None,
        });
    let self_proposed_too_recently = self_latest_block_timestamp_ms.is_some_and(|timestamp_ms| {
        now.saturating_sub(timestamp_ms) < config.self_propose_cooldown.as_millis()
    });

    // Check if we have new parents (new blocks since our last block) and whether
    // they include user deploys (to keep deploy-driven finality fast).
    let parent_update = inspect_parent_updates(&snapshot, validator_identity, &casper);
    let has_new_parents = parent_update.has_new_parents;
    let has_new_parent_with_user_deploys = parent_update.has_new_parent_with_user_deploys;
    // Treat "deploy recovery" as actively deploy-driven conditions only.
    // Keeping grace-only mode out of this hint avoids prolonged frontier-chase churn
    // once deploy pressure is gone.
    let deploy_recovery_hint = has_pending_deploys || has_new_parent_with_user_deploys;
    let deploy_recovery_max_lag =
        std::cmp::max(pending_deploy_max_lag, advanced_deploy_recovery_max_lag);
    let idle_recovery_window_open =
        finality_progress.stalled && finality_progress.recovery_round_due;
    let lag_recovery_leader = is_lag_recovery_leader(&snapshot, validator_identity);
    let effective_frontier_chase_cap = effective_frontier_chase_cap(
        frontier_chase_max_lag,
        deploy_recovery_max_lag,
        deploy_recovery_hint,
    );
    // The backpressure exemption key must be TEMPORAL, not height-based:
    // "my latest block is above the LFB" is permanently true for every
    // validator during a finalization stall, so keying the exemption on it
    // deadlocks the shard once the cap is exceeded — no proposals, no
    // witnessing rounds, no floor advance, no drain. A validator idle for a
    // full stale-recovery interval gets one proposal through the cap: the
    // cap bounds empty-block churn to the recovery cadence, never to zero.
    let self_minted_within_recovery_interval =
        self_latest_block_timestamp_ms.is_some_and(|timestamp_ms| {
            now.saturating_sub(timestamp_ms) < stale_recovery_min_interval_ms
        });
    let empty_frontier_pressure = empty_frontier_pressure(
        &snapshot,
        config.advanced.empty_frontier_max_unfinalized_blocks,
        has_pending_deploys,
        has_new_parent_with_user_deploys,
        deploy_grace_active,
        self_recently_proposed && self_minted_within_recovery_interval,
        idle_recovery_window_open && lag_recovery_leader,
    )?;
    let empty_frontier_backpressure = empty_frontier_pressure.backpressure;
    if empty_frontier_backpressure {
        tracing::info!(
            target: "f1r3fly.casper.heartbeat.backpressure",
            "Heartbeat: Empty frontier backpressure active (unfinalized_blocks={}, cap={}, lag={}, latest_height={}, lfb_height={})",
            empty_frontier_pressure.unfinalized_blocks,
            empty_frontier_pressure.max_unfinalized_blocks,
            lfb_lag_blocks,
            latest_block_number,
            last_finalized_block_number
        );
    }
    let stale_recovery_interval_elapsed = stale_recovery_window_is_open(
        time_since_lfb,
        self_latest_block_timestamp_ms.map(|timestamp_ms| now.saturating_sub(timestamp_ms)),
        stale_recovery_min_interval_ms,
    );

    let lane_inputs = LaneInputs {
        lfb_is_stale,
        lfb_lag_blocks,
        has_pending_deploys,
        has_new_parents,
        has_new_parent_with_user_deploys,
        deploy_grace_active,
        self_recently_proposed,
        self_proposed_too_recently,
        self_idle_for_recovery_interval: self_latest_block_timestamp_ms
            .map(|timestamp_ms| now.saturating_sub(timestamp_ms) >= stale_recovery_min_interval_ms)
            .unwrap_or(true),
        stale_recovery_interval_elapsed,
        idle_recovery_window_open,
        lag_recovery_leader,
        empty_frontier_backpressure,
        pending_deploy_max_lag,
        deploy_recovery_max_lag,
        effective_frontier_chase_cap,
    };
    let LaneDecision {
        pending_deploys_due,
        pending_deploy_backstop_due,
        frontier_follow_due,
        stale_lfb_recovery_due,
        convergence_recovery_selected,
        should_propose,
        can_propose_pending_deploys_while_ahead,
        can_chase_frontier_while_ahead,
        can_follow_frontier_without_pending_deploys,
        allow_cooldown_override_for_deploy_recovery,
        allow_frontier_follow_while_ahead_for_deploy_parent,
    } = decide_lanes(&lane_inputs);

    if should_propose {
        let reason = if pending_deploy_backstop_due {
            format!(
                "pending deploy recovery backstop: lag={} exceeds cap={} while ahead; forcing propose after {}ms",
                lfb_lag_blocks,
                if deploy_grace_active {
                    deploy_recovery_max_lag
                } else {
                    pending_deploy_max_lag
                },
                stale_recovery_min_interval_ms
            )
        } else if has_pending_deploys && !pending_deploys_due {
            format!(
                "pending deploys exist but lag={} exceeds pending-deploy cap={} while already ahead of finalized (throttling)",
                lfb_lag_blocks,
                pending_deploy_max_lag
            )
        } else if has_pending_deploys {
            "pending user deploys in storage".to_string()
        } else if frontier_follow_due {
            format!(
                "new parents observed (lag={}, self_recently_proposed={}, cooldown_active={}, cooldown_ms={}, cooldown_override_for_deploy_recovery={}, frontier_chase_cap={}, user_deploy_parent={}, deploy_grace_active={}, stale_recovery_interval_ms={}); proposing to keep frontier moving",
                lfb_lag_blocks,
                self_recently_proposed,
                self_proposed_too_recently,
                config.self_propose_cooldown.as_millis(),
                allow_cooldown_override_for_deploy_recovery,
                effective_frontier_chase_cap,
                has_new_parent_with_user_deploys,
                deploy_grace_active,
                stale_recovery_min_interval_ms
            )
        } else if convergence_recovery_selected {
            format!(
                "convergence recovery: finality stalled for {}ms at lag={}; selected recovery leader proposing one multi-parent convergence block",
                finality_progress.stalled_for.as_millis(),
                lfb_lag_blocks
            )
        } else if self_recently_proposed && has_new_parents && !can_chase_frontier_while_ahead {
            format!(
                "LFB is stale but frontier-follow is throttled (lag={}, cooldown_active={}, frontier_chase_cap={})",
                lfb_lag_blocks,
                self_proposed_too_recently,
                effective_frontier_chase_cap
            )
        } else if self_recently_proposed && !has_new_parents {
            "LFB is stale but validator is already ahead of finalized height (cooling down stale-LFB recovery)".to_string()
        } else if !standalone && !has_new_parents {
            format!(
                "LFB is stale ({}ms old, threshold: {}ms) and no new parents (recovery heartbeat)",
                time_since_lfb,
                config.max_lfb_age.as_millis()
            )
        } else {
            format!(
                "LFB is stale ({}ms old, threshold: {}ms) and new parents exist",
                time_since_lfb,
                config.max_lfb_age.as_millis()
            )
        };

        tracing::info!("Heartbeat: Proposing block - reason: {}", reason);

        // Heartbeat proposals are liveness-driven and may need empty-block capability.
        // We route them through async propose mode to enable empty blocks only for heartbeat.
        let result = trigger_propose(casper.clone(), true).await?;
        match result {
            ProposerResult::Empty => {
                tracing::debug!("Heartbeat: Propose already in progress, will retry next check");
                Ok(HeartbeatCheckResult {
                    bug_failure: false,
                    refresh_deploy_grace_window: has_pending_deploys
                        || has_new_parent_with_user_deploys,
                    finality_recovery_attempted: false,
                })
            }
            ProposerResult::Failure(status, seq_num) => {
                tracing::warn!(
                    "Heartbeat: Propose failed with {} (seqNum {})",
                    status,
                    seq_num
                );
                // Only escalate backoff for explicit bug failures.
                // Recoverable propose races should retry on the normal heartbeat cadence.
                Ok(HeartbeatCheckResult {
                    bug_failure: matches!(status, ProposeStatus::Failure(ProposeFailure::BugError)),
                    refresh_deploy_grace_window: has_pending_deploys
                        || has_new_parent_with_user_deploys,
                    finality_recovery_attempted: false,
                })
            }
            ProposerResult::Success(_, _) => {
                tracing::info!("Heartbeat: Successfully created block");
                Ok(HeartbeatCheckResult {
                    bug_failure: false,
                    refresh_deploy_grace_window: has_pending_deploys
                        || has_new_parent_with_user_deploys,
                    finality_recovery_attempted: convergence_recovery_selected,
                })
            }
            ProposerResult::Started(seq_num) => {
                tracing::info!("Heartbeat: Async propose started (seqNum {})", seq_num);
                Ok(HeartbeatCheckResult {
                    bug_failure: false,
                    refresh_deploy_grace_window: has_pending_deploys
                        || has_new_parent_with_user_deploys,
                    finality_recovery_attempted: convergence_recovery_selected,
                })
            }
        }
    } else {
        let reason = if empty_frontier_backpressure {
            format!(
                "empty frontier backpressure: unfinalized_blocks={} exceeds cap {}; no pending deploys or deploy-carrying peer parent",
                empty_frontier_pressure.unfinalized_blocks,
                empty_frontier_pressure.max_unfinalized_blocks
            )
        } else if !has_pending_deploys
            && !has_new_parent_with_user_deploys
            && !finality_progress.stalled
        {
            format!(
                "finality advanced {}ms ago; idle recovery waits {}ms",
                finality_progress.stalled_for.as_millis(),
                config.finality_progress_timeout.as_millis()
            )
        } else if finality_progress.stalled && !finality_progress.recovery_round_due {
            "bounded idle recovery round already attempted for current finalized block".to_string()
        } else if !lfb_is_stale {
            if has_pending_deploys
                && self_recently_proposed
                && !can_propose_pending_deploys_while_ahead
            {
                let pending_backstop_remaining_ms = self_latest_block_timestamp_ms
                    .map(|timestamp_ms| {
                        stale_recovery_min_interval_ms
                            .saturating_sub(now.saturating_sub(timestamp_ms))
                    })
                    .unwrap_or(0);
                format!(
                    "pending deploy lag throttle active: lag {} exceeds cap {} while already ahead (next backstop in {}ms)",
                    lfb_lag_blocks,
                    if deploy_grace_active {
                        deploy_recovery_max_lag
                    } else {
                        pending_deploy_max_lag
                    },
                    pending_backstop_remaining_ms
                )
            } else if has_new_parents && self_recently_proposed && !can_chase_frontier_while_ahead {
                format!(
                    "frontier-follow throttled: lag {}, cooldown_active={}, cooldown_override_for_deploy_recovery={}, deploy_parent_override={}, cap {} while already ahead",
                    lfb_lag_blocks,
                    self_proposed_too_recently,
                    allow_cooldown_override_for_deploy_recovery,
                    allow_frontier_follow_while_ahead_for_deploy_parent,
                    effective_frontier_chase_cap
                )
            } else if has_new_parents && !can_follow_frontier_without_pending_deploys {
                format!(
                    "frontier-follow throttled by stale-recovery cadence: frontier_age_ms={}, min_interval_ms={}, user_deploy_parent={}, deploy_grace_active={}",
                    frontier_age_ms,
                    stale_recovery_min_interval_ms,
                    has_new_parent_with_user_deploys,
                    deploy_grace_active
                )
            } else {
                format!(
                    "LFB age is {}ms (threshold: {}ms)",
                    time_since_lfb,
                    config.max_lfb_age.as_millis()
                )
            }
        } else if lfb_is_stale && !stale_recovery_interval_elapsed {
            format!(
                "LFB is stale but the stale-recovery interval has not elapsed (own-proposal or LFB age below min_interval_ms={}): frontier_age_ms={}, user_deploy_parent={}, deploy_grace_active={}",
                stale_recovery_min_interval_ms,
                frontier_age_ms,
                has_new_parent_with_user_deploys,
                deploy_grace_active
            )
        } else if self_recently_proposed && has_new_parents && !can_chase_frontier_while_ahead {
            format!(
                "frontier-follow throttled while ahead (lag {}, cooldown_active={}, cap {})",
                lfb_lag_blocks, self_proposed_too_recently, effective_frontier_chase_cap
            )
        } else if !standalone && !has_new_parents {
            "no new parents".to_string()
        } else {
            "unknown".to_string()
        };
        // A STALE shard declining to act is the signal operators need at
        // INFO — a pacified shard must not be silent in the logs. Healthy
        // declines stay at DEBUG (once-per-tick noise).
        if lfb_is_stale {
            tracing::info!(
                "Heartbeat: No action despite stale LFB - reason: {}",
                reason
            );
        } else {
            tracing::debug!("Heartbeat: No action needed - reason: {}", reason);
        }
        Ok(HeartbeatCheckResult {
            bug_failure: false,
            refresh_deploy_grace_window: has_pending_deploys || has_new_parent_with_user_deploys,
            finality_recovery_attempted: false,
        })
    }
}

/// Check if new blocks exist since this validator's last block.
/// Returns parent update details where:
/// - Validator has no blocks yet (can propose)
/// - Validator's last block is genesis (allows breaking post-genesis deadlock)
/// - Any latest message hash diverges from what this validator observed in its last justifications
#[derive(Default)]
struct ParentUpdate {
    has_new_parents: bool,
    has_new_parent_with_user_deploys: bool,
}

fn inspect_parent_updates(
    snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    casper: &Arc<dyn MultiParentCasper + Send + Sync>,
) -> ParentUpdate {
    let validator_id = &validator_identity.public_key.bytes;

    // Get validator's last block
    let last_block_hash = match snapshot.dag.latest_message_hash(validator_id) {
        Some(hash) => hash,
        None => {
            // Validator has no blocks yet - can propose
            return ParentUpdate {
                has_new_parents: true,
                has_new_parent_with_user_deploys: false,
            };
        }
    };

    // Check if this is genesis block (allows breaking deadlock after genesis)
    let block_meta = match snapshot.dag.lookup(&last_block_hash) {
        Ok(Some(meta)) => meta,
        _ => {
            // Can't find block metadata, allow proposal
            return ParentUpdate {
                has_new_parents: true,
                has_new_parent_with_user_deploys: false,
            };
        }
    };

    if block_meta.parents.is_empty() {
        tracing::debug!("Heartbeat: Validator's last block is genesis, allowing proposal");
        return ParentUpdate {
            has_new_parents: true,
            has_new_parent_with_user_deploys: false,
        };
    }

    // Fast path: compare current validator latest messages against the latest messages
    // referenced in this validator's own latest block justifications.
    // If any validator advanced since then (or newly appeared), we have new parents.
    let justified_latest: std::collections::HashMap<Vec<u8>, BlockHash> = block_meta
        .justifications
        .iter()
        .map(|j| (j.validator.to_vec(), j.latest_block_hash.clone()))
        .collect();

    let mut update = ParentUpdate::default();

    for (validator, current_hash) in snapshot.dag.latest_message_hashes().iter() {
        let known_hash_opt = if *validator == *validator_id {
            Some(&last_block_hash)
        } else {
            justified_latest.get(validator.as_ref())
        };

        if known_hash_opt != Some(current_hash) {
            update.has_new_parents = true;
            if !update.has_new_parent_with_user_deploys {
                if let Ok(Some(block)) = casper.block_store().get(current_hash) {
                    let has_user_deploys = block
                        .body
                        .deploys
                        .iter()
                        .any(|processed| !is_system_deploy_id(&processed.deploy.sig));
                    if has_user_deploys {
                        update.has_new_parent_with_user_deploys = true;
                    }
                }
            }
            if update.has_new_parent_with_user_deploys {
                break;
            }
        }
    }

    update
}

/// The stale-LFB recovery pacing window: whether enough time has passed
/// for this validator to attempt a recovery proposal.
///
/// Keyed on FINALIZATION age and this validator's OWN proposal cadence —
/// never on frontier freshness: any block source arriving faster than the
/// interval refreshes the frontier and would hold every validator's
/// recovery window closed while finalization stays stale (one
/// non-finalizing chain pacified a whole shard for 107s). Finalization
/// progress closes the window through `time_since_lfb`; the own-proposal
/// age keeps a stale shard from re-firing every heartbeat tick (at most
/// one attempt per interval per validator — the same cadence pacing the
/// pending-deploy backstop uses).
/// Everything the heartbeat's per-tick lane decision reads, as plain data.
/// One row of this struct is one observable proposer state — extracted so the
/// lane predicates are a pure function that CI-harvested rows can pin
/// directly (see the `lane_decision_rows` tests).
struct LaneInputs {
    lfb_is_stale: bool,
    lfb_lag_blocks: i64,
    has_pending_deploys: bool,
    has_new_parents: bool,
    has_new_parent_with_user_deploys: bool,
    deploy_grace_active: bool,
    /// The HEIGHT key: this validator's latest message sits above the LFB.
    /// During a finality stall this is permanently true for every validator.
    self_recently_proposed: bool,
    /// The cooldown key: this validator minted within `self-propose-cooldown`.
    self_proposed_too_recently: bool,
    /// The temporal backstop key: this validator has not minted for a full
    /// `stale-recovery-min-interval` (true when it has never minted).
    self_idle_for_recovery_interval: bool,
    /// `stale_recovery_window_is_open`: LFB age AND own-proposal age both
    /// reached `stale-recovery-min-interval`.
    stale_recovery_interval_elapsed: bool,
    idle_recovery_window_open: bool,
    lag_recovery_leader: bool,
    empty_frontier_backpressure: bool,
    pending_deploy_max_lag: i64,
    deploy_recovery_max_lag: i64,
    effective_frontier_chase_cap: i64,
}

/// The lane verdicts plus every intermediate the reason ladders report.
struct LaneDecision {
    pending_deploys_due: bool,
    pending_deploy_backstop_due: bool,
    frontier_follow_due: bool,
    stale_lfb_recovery_due: bool,
    convergence_recovery_selected: bool,
    should_propose: bool,
    can_propose_pending_deploys_while_ahead: bool,
    can_chase_frontier_while_ahead: bool,
    can_follow_frontier_without_pending_deploys: bool,
    allow_cooldown_override_for_deploy_recovery: bool,
    allow_frontier_follow_while_ahead_for_deploy_parent: bool,
}

/// Proposal logic:
/// - Prioritize pending deploys, but avoid lag-amplification loops:
///   - when this validator is already ahead of finalized and lag is above cap,
///     temporarily stop heartbeat-driven pending-deploy proposes.
/// - Keep frontier moving on peer progress:
///   - when new parents are observed, allow follow-up propose even before LFB turns stale;
///   - when already ahead, guard this with the frontier chase lag cap.
/// - For stale-LFB recovery: EVERY bonded validator proposes, paced only by
///   the temporal window (`stale-recovery-min-interval` on the LFB's age and
///   its own silence) — certification needs mutual witnessing, so recovery
///   is never gated on a leader or on height relations.
/// - The convergence one-shot stays leader-only and once per finalized block.
fn decide_lanes(i: &LaneInputs) -> LaneDecision {
    let deploy_recovery_hint = i.has_pending_deploys || i.has_new_parent_with_user_deploys;
    let can_propose_pending_deploys_while_ahead = if i.deploy_grace_active {
        i.lfb_lag_blocks <= i.deploy_recovery_max_lag
    } else {
        i.lfb_lag_blocks <= i.pending_deploy_max_lag
    };
    let pending_deploys_due = i.has_pending_deploys
        && (!i.self_recently_proposed || can_propose_pending_deploys_while_ahead);
    // Backstop: even when high lag throttles pending-deploy proposals, force a bounded
    // retry based on local self-proposal cadence so deploys cannot starve indefinitely.
    let pending_deploy_backstop_due = i.has_pending_deploys
        && i.self_recently_proposed
        && !can_propose_pending_deploys_while_ahead
        && i.self_idle_for_recovery_interval
        && (!i.self_proposed_too_recently || i.deploy_grace_active);
    let can_follow_frontier_without_pending_deploys =
        deploy_recovery_hint || i.stale_recovery_interval_elapsed;
    // Cooldown protects idle clusters from empty-block churn, but during deploy-driven
    // recovery/finalization we should not wait out the full cooldown before advancing finality.
    let allow_cooldown_override_for_deploy_recovery =
        i.has_pending_deploys || i.has_new_parent_with_user_deploys;
    // When a peer parent with user deploys is observed, allow one frontier-follow step
    // while ahead (bounded by pending-deploy lag threshold) to unblock synchrony progress.
    let allow_frontier_follow_while_ahead_for_deploy_parent =
        i.has_new_parent_with_user_deploys && i.lfb_lag_blocks <= i.deploy_recovery_max_lag;
    let can_chase_frontier_while_ahead = i.lfb_lag_blocks <= i.effective_frontier_chase_cap
        && i.has_new_parents
        && !i.empty_frontier_backpressure
        && (!i.self_proposed_too_recently || allow_cooldown_override_for_deploy_recovery);
    let frontier_follow_due = !i.has_pending_deploys
        && i.has_new_parents
        && !i.empty_frontier_backpressure
        && can_follow_frontier_without_pending_deploys
        && (!i.self_recently_proposed
            || can_chase_frontier_while_ahead
            || allow_frontier_follow_while_ahead_for_deploy_parent);
    // The stale-recovery lane is open to EVERY bonded validator, paced
    // solely by the temporal window (`stale-recovery-min-interval` on both
    // the LFB's age and this validator's own silence). There is no height
    // condition here: "my latest block is above the LFB" is permanently
    // true for every validator during a finality stall, so a height-keyed
    // exemption silences the whole committee exactly when mutual witnessing
    // is the only way the certification clique can re-form — the
    // "waiting for selected recovery leader" silence in every CI stall.
    // (The same trap is documented for the backpressure exemption above.)
    // Deploy-driven states do not need this lane's hint escape either: the
    // pending and frontier-follow lanes own those, so empty recovery blocks
    // key on the interval alone. The former leader-only stale/high-lag
    // lanes are gone with it: with the general lane open on the temporal
    // window, their conditions were a strict subset and could never fire.
    // Leader selection remains for the convergence one-shot only.
    let stale_lfb_recovery_due =
        i.lfb_is_stale && i.stale_recovery_interval_elapsed && !i.empty_frontier_backpressure;
    // Convergence recovery: when the LFB is stale and we have unjustified peer blocks,
    // propose a convergence block that references all known tips. This breaks the deadlock
    // where validators diverge into independent forks and normal throttling prevents any
    // validator from proposing a multi-parent convergence block.
    let convergence_recovery_due =
        i.idle_recovery_window_open && i.lag_recovery_leader && !i.empty_frontier_backpressure;
    let routine_proposal_due = pending_deploys_due
        || pending_deploy_backstop_due
        || frontier_follow_due
        || stale_lfb_recovery_due;
    let convergence_recovery_selected = convergence_recovery_due && !routine_proposal_due;
    let should_propose = routine_proposal_due || convergence_recovery_selected;
    LaneDecision {
        pending_deploys_due,
        pending_deploy_backstop_due,
        frontier_follow_due,
        stale_lfb_recovery_due,
        convergence_recovery_selected,
        should_propose,
        can_propose_pending_deploys_while_ahead,
        can_chase_frontier_while_ahead,
        can_follow_frontier_without_pending_deploys,
        allow_cooldown_override_for_deploy_recovery,
        allow_frontier_follow_while_ahead_for_deploy_parent,
    }
}

fn stale_recovery_window_is_open(
    time_since_lfb_ms: u128,
    self_latest_block_age_ms: Option<u128>,
    stale_recovery_min_interval_ms: u128,
) -> bool {
    time_since_lfb_ms >= stale_recovery_min_interval_ms
        && self_latest_block_age_ms
            .map(|age_ms| age_ms >= stale_recovery_min_interval_ms)
            .unwrap_or(true)
}

fn is_lag_recovery_leader(
    snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
) -> bool {
    let validators: Vec<Validator> = match snapshot.parents.first() {
        Some(parent) => parent
            .body
            .state
            .bonds
            .iter()
            .map(|bond| bond.validator.clone())
            .collect(),
        None => return true,
    };
    if validators.is_empty() {
        return true;
    }

    select_lag_recovery_leader(validators, &snapshot.last_finalized_block)
        .is_none_or(|leader| leader == validator_identity.public_key.bytes)
}

fn effective_frontier_chase_cap(
    frontier_chase_max_lag: i64,
    deploy_recovery_max_lag: i64,
    deploy_recovery_hint: bool,
) -> i64 {
    if deploy_recovery_hint {
        std::cmp::max(
            frontier_chase_max_lag,
            std::cmp::max(2, deploy_recovery_max_lag),
        )
    } else {
        frontier_chase_max_lag
    }
}

fn select_lag_recovery_leader(
    validators: Vec<Validator>,
    _last_finalized_block: &BlockHash,
) -> Option<Validator> {
    validators.into_iter().min()
}

/// Unit tests for HeartbeatProposer configuration validation.
///
/// These tests verify the create() function properly handles configuration:
/// - Disabled config returns None
/// - Invalid max-number-of-parents returns None (with error log)
/// - Valid config returns Some(JoinHandle)
///
/// Note: Actual proposal behavior is tested via integration tests (Python/Docker)
/// which can properly set up a full Casper environment.
#[cfg(test)]
mod tests {
    use casper::rust::heartbeat_signal::new_heartbeat_signal_ref;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn deploy_recovery_frontier_chase_cap_is_widened() {
        assert_eq!(effective_frontier_chase_cap(20, 8, true), 20);
        assert_eq!(effective_frontier_chase_cap(1, 8, true), 8);
        assert_eq!(effective_frontier_chase_cap(1, 1, true), 2);
        assert_eq!(effective_frontier_chase_cap(20, 8, false), 20);
    }

    #[test]
    fn lag_recovery_leader_is_stable_across_local_dag_and_lfb_views() {
        let first = Validator::from(vec![1]);
        let selected = Validator::from(vec![2]);
        let third = Validator::from(vec![3]);
        let first_lfb = BlockHash::from(vec![4; 32]);
        let second_lfb = BlockHash::from(vec![5; 32]);

        let leader = select_lag_recovery_leader(
            vec![third.clone(), first.clone(), selected.clone()],
            &first_lfb,
        );
        let reordered =
            select_lag_recovery_leader(vec![selected, third, first.clone()], &second_lfb);

        assert_eq!(leader, Some(first.clone()));
        assert_eq!(reordered, Some(first));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn lag_recovery_leader_is_cross_view_deterministic(
            validator_bytes in prop::collection::vec(
                prop::collection::vec(any::<u8>(), 1..16),
                1..32,
            ),
            first_lfb in prop::collection::vec(any::<u8>(), 32..=32),
            second_lfb in prop::collection::vec(any::<u8>(), 32..=32),
            rotation in any::<usize>(),
        ) {
            let validators: Vec<Validator> = validator_bytes
                .into_iter()
                .map(Validator::from)
                .collect();
            let mut alternate_order = validators.clone();
            let validator_count = alternate_order.len();
            alternate_order.rotate_left(rotation % validator_count);

            let first = select_lag_recovery_leader(
                validators,
                &BlockHash::from(first_lfb),
            );
            let second = select_lag_recovery_leader(
                alternate_order,
                &BlockHash::from(second_lfb),
            );

            prop_assert_eq!(first, second);
        }
    }

    #[test]
    fn finality_progress_resets_recovery_budget_on_lfb_change() {
        let start = Instant::now();
        let timeout = Duration::from_secs(30);
        let first = BlockHash::from(vec![1; 32]);
        let second = BlockHash::from(vec![2; 32]);
        let mut progress = FinalityProgress::new(start);

        let initial = progress.observe(&first, start, timeout);
        assert!(!initial.stalled);
        assert!(!initial.recovery_round_due);

        let stalled = progress.observe(&first, start + timeout, timeout);
        assert!(stalled.stalled);
        assert!(stalled.recovery_round_due);

        progress.record_recovery_attempt(start + timeout);
        let bounded = progress.observe(&first, start + timeout + Duration::from_secs(1), timeout);
        assert!(bounded.stalled);
        assert!(!bounded.recovery_round_due);

        let still_bounded = progress.observe(&first, start + timeout * 3, timeout);
        assert!(still_bounded.stalled);
        assert!(!still_bounded.recovery_round_due);

        let advanced = progress.observe(&second, start + timeout + Duration::from_secs(1), timeout);
        assert!(!advanced.stalled);
        assert!(!advanced.recovery_round_due);
    }

    fn create_test_validator_identity() -> ValidatorIdentity {
        let secp = Secp256k1;
        let (sk, pk) = secp.new_key_pair();
        ValidatorIdentity {
            public_key: pk,
            private_key: sk,
            signature_algorithm: "secp256k1".to_string(),
        }
    }

    fn create_mock_propose_function() -> Arc<ProposeFunction> {
        Arc::new(|_casper, _is_async| {
            Box::pin(async { Ok(casper::rust::blocks::proposer::proposer::ProposerResult::Empty) })
        })
    }

    // ==================== Stale-recovery window pacing ====================

    /// The pacification class (gate session 43d9f798): a busy block source
    /// keeps the frontier fresh, but finalization is long stale. Frontier
    /// freshness is not finalization progress — the window must open.
    #[test]
    fn pacified_shard_opens_the_stale_recovery_window() {
        assert!(
            stale_recovery_window_is_open(100_000, Some(10_000), 3_000),
            "finalization stale for 100s with our own last proposal 10s old: \
             a 1s-fresh frontier must not hold the recovery window closed"
        );
    }

    /// A healthy shard (finalization tracking the tip) keeps the window
    /// closed even when block production goes quiet — quiet is not
    /// staleness.
    #[test]
    fn healthy_shard_keeps_the_window_closed_even_when_quiet() {
        assert!(
            !stale_recovery_window_is_open(1_000, Some(60_000), 3_000),
            "finalization 1s old: a quiet frontier alone must not open the \
             recovery window"
        );
    }

    /// The window paces on this validator's OWN proposal cadence: a stale
    /// shard re-fires at most once per interval per validator, never every
    /// heartbeat tick. A validator with no block yet is not paced.
    #[test]
    fn own_recent_proposal_paces_the_window() {
        assert!(
            !stale_recovery_window_is_open(100_000, Some(1_000), 3_000),
            "our own recovery proposal 1s ago must close the window until \
             the interval elapses"
        );
        assert!(
            stale_recovery_window_is_open(100_000, None, 3_000),
            "a validator with no block of its own yet is not paced"
        );
    }

    // ==================== Configuration validation tests ====================

    #[tokio::test]
    async fn heartbeat_create_returns_none_when_config_disabled() {
        use casper::rust::engine::engine_cell::EngineCell;

        let config = HeartbeatConf {
            enabled: false,
            check_interval: Duration::from_secs(10),
            max_lfb_age: Duration::from_secs(60),
            self_propose_cooldown: Duration::from_secs(15),
            ..HeartbeatConf::default()
        };
        let validator = create_test_validator_identity();
        let heartbeat_signal_ref = new_heartbeat_signal_ref();
        let engine_cell = Arc::new(EngineCell::init());
        let propose_f = create_mock_propose_function();

        let result = HeartbeatProposer::create(
            engine_cell,
            Some(propose_f),
            validator,
            config,
            10,
            heartbeat_signal_ref,
            false,
        );

        assert!(
            result.is_none(),
            "Should return None when heartbeat is disabled"
        );
    }

    #[tokio::test]
    async fn heartbeat_create_returns_none_when_max_parents_is_one() {
        use casper::rust::engine::engine_cell::EngineCell;

        let config = HeartbeatConf {
            enabled: true,
            check_interval: Duration::from_secs(10),
            max_lfb_age: Duration::from_secs(60),
            self_propose_cooldown: Duration::from_secs(15),
            ..HeartbeatConf::default()
        };
        let validator = create_test_validator_identity();
        let heartbeat_signal_ref = new_heartbeat_signal_ref();
        let engine_cell = Arc::new(EngineCell::init());
        let propose_f = create_mock_propose_function();

        // max_number_of_parents = 1 triggers safety check
        let result = HeartbeatProposer::create(
            engine_cell,
            Some(propose_f),
            validator,
            config,
            1,
            heartbeat_signal_ref,
            false,
        );

        assert!(
            result.is_none(),
            "Should return None when max_number_of_parents == 1 (safety check)"
        );
    }

    #[tokio::test]
    async fn heartbeat_create_returns_some_when_all_conditions_met() {
        use casper::rust::engine::engine_cell::EngineCell;

        let config = HeartbeatConf {
            enabled: true,
            check_interval: Duration::from_secs(1),
            max_lfb_age: Duration::from_secs(60),
            self_propose_cooldown: Duration::from_secs(15),
            ..HeartbeatConf::default()
        };
        let validator = create_test_validator_identity();
        let heartbeat_signal_ref = new_heartbeat_signal_ref();
        let engine_cell = Arc::new(EngineCell::init());
        let propose_f = create_mock_propose_function();

        let result = HeartbeatProposer::create(
            engine_cell,
            Some(propose_f),
            validator,
            config,
            10,
            heartbeat_signal_ref,
            false,
        );

        assert!(
            result.is_some(),
            "Should return Some(JoinHandle) when all conditions are met"
        );

        // Clean up: abort the spawned task
        if let Some(handle) = result {
            handle.abort();
        }
    }

    // ==================== Decision Logic Tests (Direct Method Calls) ====================
    // Tests that call do_heartbeat_check directly for deterministic behavior

    mod decision_logic_tests {
        use std::collections::BTreeMap;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use casper::rust::casper::MultiParentCasper;
        use models::rust::block_metadata::BlockMetadata;
        use models::rust::casper::protocol::casper_message::Justification;
        use prost::bytes::Bytes;

        use super::*;

        // Helper to create LFB with controllable timestamp (age in ms)
        fn create_lfb_with_age(
            age_ms: u64,
        ) -> models::rust::casper::protocol::casper_message::BlockMessage {
            let mut block = models::rust::block_implicits::get_random_block_default();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            block.header.timestamp = now - (age_ms as i64);
            block
        }

        // Helper to create a propose function that tracks call count
        /// A propose function that always fails benignly (`NoNewDeploys`):
        /// exercises the paths where a propose ATTEMPT happens but produces
        /// no block — a failed attempt must not consume the one-shot
        /// finality-recovery budget and must not read as a bug.
        fn create_failing_propose_function() -> (Arc<AtomicUsize>, Arc<ProposeFunction>) {
            use casper::rust::blocks::proposer::propose_result::{ProposeFailure, ProposeStatus};
            use casper::rust::blocks::proposer::proposer::ProposerResult;

            let count = Arc::new(AtomicUsize::new(0));
            let count_clone = count.clone();
            let func: Arc<ProposeFunction> = Arc::new(move |_casper, _is_async| {
                count_clone.fetch_add(1, Ordering::SeqCst);
                Box::pin(async {
                    Ok(ProposerResult::Failure(
                        ProposeStatus::Failure(ProposeFailure::NoNewDeploys),
                        7,
                    ))
                })
            });
            (count, func)
        }

        fn create_counting_propose_function() -> (Arc<AtomicUsize>, Arc<ProposeFunction>) {
            use casper::rust::blocks::proposer::propose_result::{ProposeStatus, ProposeSuccess};
            use casper::rust::blocks::proposer::proposer::ProposerResult;

            let count = Arc::new(AtomicUsize::new(0));
            let count_clone = count.clone();
            let func: Arc<ProposeFunction> = Arc::new(move |_casper, _is_async| {
                count_clone.fetch_add(1, Ordering::SeqCst);
                Box::pin(async {
                    Ok(ProposerResult::Success(
                        ProposeStatus::Success(ProposeSuccess {
                            result: casper::rust::block_status::ValidBlock::Valid,
                        }),
                        models::rust::block_implicits::get_random_block_default(),
                    ))
                })
            });
            (count, func)
        }

        fn test_hash(byte: u8) -> BlockHash { Bytes::from(vec![byte; 32]) }

        fn test_validator(byte: u8) -> Bytes {
            Bytes::from(vec![byte; models::rust::validator::LENGTH])
        }

        fn add_test_metadata(
            snapshot: &mut CasperSnapshot,
            hash: BlockHash,
            sender: Bytes,
            parents: Vec<BlockHash>,
            block_number: i64,
            finalized: bool,
            justifications: Vec<Justification>,
        ) {
            let metadata = BlockMetadata {
                block_hash: hash.clone(),
                parents: parents.clone(),
                sender,
                justifications,
                weight_map: BTreeMap::new(),
                block_number,
                sequence_number: block_number as i32,
                invalid: false,
                directly_finalized: finalized,
                finalized,
                fault_tolerance_value: 1.0,
                merge_base: Bytes::new(),
            };

            snapshot.dag.dag_set.insert(hash.clone());
            let mut height_hashes = snapshot
                .dag
                .height_map
                .get(&block_number)
                .cloned()
                .unwrap_or_default();
            height_hashes.insert(hash.clone());
            snapshot.dag.height_map.insert(block_number, height_hashes);
            snapshot
                .dag
                .block_number_map
                .insert(hash.clone(), block_number);
            for parent in &parents {
                let mut children = snapshot
                    .dag
                    .child_map
                    .get(parent)
                    .cloned()
                    .unwrap_or_default();
                children.insert(hash.clone());
                snapshot.dag.child_map.insert(parent.clone(), children);
            }
            if let Some(parent) = parents.first() {
                snapshot
                    .dag
                    .main_parent_map
                    .insert(hash.clone(), parent.clone());
            }
            if finalized {
                snapshot.dag.finalized_blocks_set.insert(hash.clone());
                snapshot.dag.last_finalized_block_hash = hash.clone();
                snapshot.last_finalized_block = hash.clone();
            }
            snapshot
                .dag
                .block_metadata_index
                .write()
                .add(metadata)
                .expect("insert test block metadata");
        }

        fn add_test_branch(
            snapshot: &mut CasperSnapshot,
            sender: Bytes,
            start_byte: u8,
            parent: BlockHash,
            blocks: i64,
            tip_justifications: Vec<Justification>,
        ) -> BlockHash {
            let mut parent = parent;
            for height in 1..=blocks {
                let hash = test_hash(start_byte + height as u8);
                let justifications = if height == blocks {
                    tip_justifications.clone()
                } else {
                    Vec::new()
                };
                add_test_metadata(
                    snapshot,
                    hash.clone(),
                    sender.clone(),
                    vec![parent],
                    height,
                    false,
                    justifications,
                );
                parent = hash;
            }
            parent
        }

        fn wide_unfinalized_snapshot(
            validator_id: Bytes,
        ) -> (
            CasperSnapshot,
            models::rust::casper::protocol::casper_message::BlockMessage,
        ) {
            let mut snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            casper::rust::casper::test_helpers::TestCasperWithSnapshot::bond_validator_in_snapshot(
                &mut snapshot,
                validator_id.clone().into(),
            );

            let lfb_hash = test_hash(1);
            add_test_metadata(
                &mut snapshot,
                lfb_hash.clone(),
                validator_id.clone(),
                Vec::new(),
                0,
                true,
                Vec::new(),
            );

            let self_tip_hash = test_hash(0x18);
            let self_tip = add_test_branch(
                &mut snapshot,
                validator_id.clone(),
                0x10,
                lfb_hash.clone(),
                8,
                vec![Justification {
                    validator: validator_id.clone(),
                    latest_block_hash: self_tip_hash,
                }],
            );
            let peer_validator = test_validator(0x44);
            let peer_tip = add_test_branch(
                &mut snapshot,
                peer_validator.clone(),
                0x30,
                lfb_hash.clone(),
                8,
                Vec::new(),
            );

            snapshot
                .dag
                .latest_messages_map
                .insert(validator_id, self_tip);
            snapshot
                .dag
                .latest_messages_map
                .insert(peer_validator, peer_tip);

            assert!(
                snapshot.dag.non_finalized_blocks().unwrap().len() > 4,
                "test fixture should exceed empty-frontier pressure cap"
            );

            let mut lfb = create_lfb_with_age(60000);
            lfb.block_hash = lfb_hash;
            (snapshot, lfb)
        }

        fn empty_frontier_backpressure_config() -> HeartbeatConf {
            HeartbeatConf {
                enabled: true,
                check_interval: Duration::from_secs(1),
                max_lfb_age: Duration::from_millis(1),
                self_propose_cooldown: Duration::from_secs(15),
                stale_recovery_min_interval: Duration::from_millis(0),
                advanced: casper::rust::casper_conf::HeartbeatAdvancedConf {
                    frontier_chase_max_lag: 20,
                    pending_deploy_max_lag: 20,
                    deploy_recovery_max_lag: 64,
                    empty_frontier_max_unfinalized_blocks: 4,
                },
                ..HeartbeatConf::default()
            }
        }

        #[tokio::test]
        async fn do_heartbeat_check_triggers_propose_with_pending_deploys() {
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();

            // Snapshot with bonded validator
            let mut snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            casper::rust::casper::test_helpers::TestCasperWithSnapshot::bond_validator_in_snapshot(
                &mut snapshot,
                validator_id.into(),
            );

            // Fresh LFB (100ms old)
            let lfb = create_lfb_with_age(100);

            // Casper with 1 pending deploy in storage
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new_with_pending_deploys(
                    snapshot, lfb, 1,
                ),
            );

            // Create counting propose function
            let (propose_count, propose_func) = create_counting_propose_function();

            // Config with long max_lfb_age so LFB is NOT stale
            let config = HeartbeatConf {
                enabled: true,
                check_interval: Duration::from_secs(1),
                max_lfb_age: Duration::from_secs(10),
                self_propose_cooldown: Duration::from_secs(15),
                ..HeartbeatConf::default()
            };
            let mut finality_progress = FinalityProgress::new(Instant::now());

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await;

            assert!(result.is_ok(), "do_heartbeat_check should succeed");
            assert_eq!(
                propose_count.load(Ordering::SeqCst),
                1,
                "Should trigger propose when pending deploys exist"
            );
        }

        #[tokio::test]
        async fn do_heartbeat_check_triggers_propose_when_lfb_stale() {
            // Create validator identity
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();

            // Create snapshot with no deploys but validator is bonded
            let mut snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            casper::rust::casper::test_helpers::TestCasperWithSnapshot::bond_validator_in_snapshot(
                &mut snapshot,
                validator_id.into(),
            );

            // Stale LFB (60 seconds old)
            let lfb = create_lfb_with_age(60000);

            // Create casper with snapshot
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new(snapshot, lfb),
            );

            // Create counting propose function
            let (propose_count, propose_func) = create_counting_propose_function();

            // Config with short max_lfb_age so LFB IS stale
            let config = HeartbeatConf {
                enabled: true,
                check_interval: Duration::from_secs(1),
                max_lfb_age: Duration::from_secs(1),
                self_propose_cooldown: Duration::from_secs(15),
                finality_progress_timeout: Duration::from_secs(30),
                ..HeartbeatConf::default()
            };
            let mut finality_progress = FinalityProgress::new(Instant::now());

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await;

            assert!(result.is_ok(), "do_heartbeat_check should succeed");
            assert_eq!(
                propose_count.load(Ordering::SeqCst),
                1,
                "Should trigger routine stale-LFB recovery before the finality-progress timeout"
            );
        }

        #[tokio::test]
        async fn routine_stale_lfb_recovery_remains_active_after_finality_stalls() {
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();
            let mut snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            casper::rust::casper::test_helpers::TestCasperWithSnapshot::bond_validator_in_snapshot(
                &mut snapshot,
                validator_id.into(),
            );
            let last_finalized_block = snapshot.last_finalized_block.clone();
            let lfb = create_lfb_with_age(60000);
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new(snapshot, lfb),
            );
            let (propose_count, propose_func) = create_counting_propose_function();
            let config = HeartbeatConf {
                enabled: true,
                check_interval: Duration::from_secs(1),
                max_lfb_age: Duration::from_secs(1),
                self_propose_cooldown: Duration::from_secs(15),
                finality_progress_timeout: Duration::from_secs(30),
                ..HeartbeatConf::default()
            };
            let now = Instant::now();
            let mut finality_progress = FinalityProgress {
                last_finalized_block: Some(last_finalized_block),
                last_progress_at: now - Duration::from_secs(31),
                last_recovery_attempt_at: Some(now - Duration::from_secs(1)),
            };

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await
            .expect("heartbeat check");

            assert_eq!(propose_count.load(Ordering::SeqCst), 1);
            assert!(!result.finality_recovery_attempted);
        }

        #[tokio::test]
        async fn do_heartbeat_check_treats_benign_propose_failure_as_non_bug() {
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();
            let mut snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            casper::rust::casper::test_helpers::TestCasperWithSnapshot::bond_validator_in_snapshot(
                &mut snapshot,
                validator_id.into(),
            );
            let lfb = create_lfb_with_age(60000);
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new(snapshot, lfb),
            );
            let (propose_count, propose_func) = create_failing_propose_function();
            let config = HeartbeatConf {
                enabled: true,
                check_interval: Duration::from_secs(1),
                max_lfb_age: Duration::from_secs(1),
                self_propose_cooldown: Duration::from_secs(15),
                finality_progress_timeout: Duration::from_secs(30),
                ..HeartbeatConf::default()
            };
            let mut finality_progress = FinalityProgress::new(Instant::now());

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await
            .expect("heartbeat check");

            assert_eq!(propose_count.load(Ordering::SeqCst), 1);
            assert!(!result.bug_failure);
        }

        #[tokio::test]
        async fn failed_convergence_propose_does_not_consume_recovery_budget() {
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();
            let mut snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            casper::rust::casper::test_helpers::TestCasperWithSnapshot::bond_validator_in_snapshot(
                &mut snapshot,
                validator_id.clone().into(),
            );
            snapshot.parents[0]
                .body
                .state
                .bonds
                .retain(|bond| bond.validator == validator_id);
            let last_finalized_block = snapshot.last_finalized_block.clone();
            let lfb = create_lfb_with_age(100);
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new(snapshot, lfb),
            );
            let (propose_count, propose_func) = create_failing_propose_function();
            let config = HeartbeatConf {
                enabled: true,
                check_interval: Duration::from_secs(1),
                max_lfb_age: Duration::from_secs(3600),
                self_propose_cooldown: Duration::from_secs(15),
                finality_progress_timeout: Duration::from_secs(30),
                ..HeartbeatConf::default()
            };
            let now = Instant::now();
            let mut finality_progress = FinalityProgress {
                last_finalized_block: Some(last_finalized_block),
                last_progress_at: now - Duration::from_secs(31),
                last_recovery_attempt_at: None,
            };

            let first = do_heartbeat_check(
                casper.clone(),
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await
            .expect("first heartbeat check");
            let second = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await
            .expect("second heartbeat check");

            assert_eq!(propose_count.load(Ordering::SeqCst), 2);
            assert!(!first.finality_recovery_attempted);
            assert!(!second.finality_recovery_attempted);
            assert!(finality_progress.last_recovery_attempt_at.is_none());
        }

        #[tokio::test]
        async fn do_heartbeat_check_skips_when_not_bonded() {
            // Create validator identity
            let validator = create_test_validator_identity();

            // Create snapshot with NO active validators (validator not bonded)
            let snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();

            // Stale LFB
            let lfb = create_lfb_with_age(60000);

            // Create casper with snapshot
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new(snapshot, lfb),
            );

            // Create counting propose function
            let (propose_count, propose_func) = create_counting_propose_function();

            let config = HeartbeatConf {
                enabled: true,
                check_interval: Duration::from_secs(1),
                max_lfb_age: Duration::from_secs(1),
                self_propose_cooldown: Duration::from_secs(15),
                ..HeartbeatConf::default()
            };
            let mut finality_progress = FinalityProgress::new(Instant::now());

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await;

            assert!(result.is_ok(), "do_heartbeat_check should succeed");
            assert_eq!(
                propose_count.load(Ordering::SeqCst),
                0,
                "Should NOT trigger propose when validator is not bonded"
            );
        }

        #[tokio::test]
        async fn do_heartbeat_check_skips_idle_propose_while_finality_progress_is_fresh() {
            // Create validator identity
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();

            // Create snapshot with no deploys but validator is bonded
            let mut snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            casper::rust::casper::test_helpers::TestCasperWithSnapshot::bond_validator_in_snapshot(
                &mut snapshot,
                validator_id.into(),
            );

            // Fresh LFB (100ms old)
            let lfb = create_lfb_with_age(100);

            // Create casper with snapshot
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new(snapshot, lfb),
            );

            // Create counting propose function
            let (propose_count, propose_func) = create_counting_propose_function();

            // Config with long max_lfb_age so LFB is NOT stale
            let config = HeartbeatConf {
                enabled: true,
                check_interval: Duration::from_secs(1),
                max_lfb_age: Duration::from_secs(10),
                self_propose_cooldown: Duration::from_secs(15),
                ..HeartbeatConf::default()
            };
            let mut finality_progress = FinalityProgress::new(Instant::now());

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await;

            assert!(result.is_ok(), "do_heartbeat_check should succeed");
            assert_eq!(
                propose_count.load(Ordering::SeqCst),
                0,
                "Should not trigger an idle frontier-follow proposal while finality progress is fresh"
            );
        }

        #[tokio::test]
        async fn do_heartbeat_check_proposes_when_storage_has_deploys_but_deploys_in_scope_empty() {
            // Reproduces bug: deploys in storage but deploysInScope empty (aged out).
            // Current: checks deploysInScope -> empty -> no propose (BUG)
            // Fixed: checks storage -> has deploy -> propose

            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();

            // Snapshot with EMPTY deploys_in_scope but validator is bonded
            let mut snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            casper::rust::casper::test_helpers::TestCasperWithSnapshot::bond_validator_in_snapshot(
                &mut snapshot,
                validator_id.into(),
            );

            // Fresh LFB so LFB is NOT stale
            let lfb = create_lfb_with_age(100);

            // Casper with pending deploy in storage (but deploys_in_scope is empty)
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new_with_pending_deploys(
                    snapshot, lfb, 1,
                ),
            );

            let (propose_count, propose_func) = create_counting_propose_function();

            let config = HeartbeatConf {
                enabled: true,
                check_interval: Duration::from_secs(1),
                max_lfb_age: Duration::from_secs(10),
                self_propose_cooldown: Duration::from_secs(15),
                ..HeartbeatConf::default()
            };
            let mut finality_progress = FinalityProgress::new(Instant::now());

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await;

            assert!(result.is_ok(), "do_heartbeat_check should succeed");
            // FAILS before fix: heartbeat checks deploys_in_scope (empty) instead of storage
            assert_eq!(
                propose_count.load(Ordering::SeqCst),
                1,
                "Should propose when storage has pending deploys, even if deploys_in_scope is empty"
            );
        }

        #[tokio::test]
        async fn do_heartbeat_check_suppresses_empty_frontier_when_unfinalized_width_is_high() {
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();
            let (snapshot, lfb) = wide_unfinalized_snapshot(validator_id);

            let casper_impl =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new(snapshot, lfb);
            // The validator minted moments ago: still inside the
            // stale-recovery interval, so the width cap applies. (An idle
            // validator is exempt — see the deadlock test below.)
            let mut self_tip = models::rust::block_implicits::get_random_block_default();
            self_tip.block_hash = test_hash(0x18);
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            self_tip.header.timestamp = now_ms;
            casper_impl.insert_block(&self_tip);
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(casper_impl);
            let (propose_count, propose_func) = create_counting_propose_function();
            let mut config = empty_frontier_backpressure_config();
            config.stale_recovery_min_interval = Duration::from_secs(60);
            let mut finality_progress = FinalityProgress::new(Instant::now());

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await;

            assert!(result.is_ok(), "do_heartbeat_check should succeed");
            assert_eq!(
                propose_count.load(Ordering::SeqCst),
                0,
                "Should not create empty frontier-follow proposals when unresolved DAG width exceeds cap"
            );
        }

        #[tokio::test]
        async fn do_heartbeat_check_allows_pending_deploys_under_empty_frontier_pressure() {
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();
            let (snapshot, lfb) = wide_unfinalized_snapshot(validator_id);

            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new_with_pending_deploys(
                    snapshot, lfb, 1,
                ),
            );
            let (propose_count, propose_func) = create_counting_propose_function();
            let config = empty_frontier_backpressure_config();
            let mut finality_progress = FinalityProgress::new(Instant::now());

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await;

            assert!(result.is_ok(), "do_heartbeat_check should succeed");
            assert_eq!(
                propose_count.load(Ordering::SeqCst),
                1,
                "Pending deploys should bypass empty-frontier backpressure"
            );
        }

        /// The terminal ucc-stall state (session 3cd723b6): finalization
        /// stalled long enough that unfinalized width exceeded the cap, and
        /// every validator's latest block sits above the frozen LFB — so a
        /// height-based "recently proposed" exemption never fires, every
        /// recovery arm is vetoed by backpressure, no one ever proposes
        /// again, and the witnessing rounds finalization needs can never
        /// happen. A validator that has been TEMPORALLY idle for a full
        /// stale-recovery interval must get one recovery proposal through
        /// the cap; the cap bounds churn, it must not be a deadlock.
        #[tokio::test]
        async fn stale_recovery_breaks_the_empty_frontier_deadlock() {
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();
            let (snapshot, lfb) = wide_unfinalized_snapshot(validator_id);

            let casper_impl =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new(snapshot, lfb);
            // The validator's own tip was minted an hour ago — the whole
            // shard has been silent while width sits above the cap.
            let mut self_tip = models::rust::block_implicits::get_random_block_default();
            self_tip.block_hash = test_hash(0x18);
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            self_tip.header.timestamp = now_ms - 3_600_000;
            casper_impl.insert_block(&self_tip);
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(casper_impl);

            let (propose_count, propose_func) = create_counting_propose_function();
            let mut config = empty_frontier_backpressure_config();
            config.stale_recovery_min_interval = Duration::from_secs(60);
            let mut finality_progress = FinalityProgress::new(Instant::now());

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                false,
                &mut finality_progress,
            )
            .await;

            assert!(result.is_ok(), "do_heartbeat_check should succeed");
            assert_eq!(
                propose_count.load(Ordering::SeqCst),
                1,
                "a temporally idle validator under a stale LFB must get one \
                 stale-recovery proposal through the unfinalized cap — a cap \
                 that silences every validator forever is a consensus deadlock"
            );
        }
    }

    /// Table pins for the pure lane decision, one row per observable
    /// proposer state. The green rows pin behavior harvested from healthy
    /// CI shards; the red row is the silence measured in every CI stall:
    /// non-leaders logged "waiting for selected recovery leader" 81/114
    /// times in instance i1 (run 32284324989), 288/289 times in i5
    /// (run 32397055615), and 464 times on the ucc-i6 wedged node — each
    /// a tick where a stalled, temporally idle validator proposed nothing.
    mod lane_decision_rows {
        use super::super::{decide_lanes, LaneInputs};

        /// A healthy idle shard: fresh LFB, no work, nothing due.
        fn baseline() -> LaneInputs {
            LaneInputs {
                lfb_is_stale: false,
                lfb_lag_blocks: 0,
                has_pending_deploys: false,
                has_new_parents: false,
                has_new_parent_with_user_deploys: false,
                deploy_grace_active: false,
                self_recently_proposed: false,
                self_proposed_too_recently: false,
                self_idle_for_recovery_interval: true,
                stale_recovery_interval_elapsed: false,
                idle_recovery_window_open: false,
                lag_recovery_leader: false,
                empty_frontier_backpressure: false,
                pending_deploy_max_lag: 20,
                deploy_recovery_max_lag: 64,
                effective_frontier_chase_cap: 20,
            }
        }

        #[test]
        fn a_healthy_idle_shard_proposes_nothing() {
            let d = decide_lanes(&baseline());
            assert!(!d.should_propose);
        }

        #[test]
        fn pending_deploys_fire_the_deploy_lane() {
            let d = decide_lanes(&LaneInputs {
                has_pending_deploys: true,
                ..baseline()
            });
            assert!(d.pending_deploys_due && d.should_propose);
        }

        #[test]
        fn the_pending_backstop_forces_one_propose_over_the_lag_cap() {
            let over_cap_idle = LaneInputs {
                has_pending_deploys: true,
                self_recently_proposed: true,
                lfb_lag_blocks: 30,
                ..baseline()
            };
            let d = decide_lanes(&over_cap_idle);
            assert!(
                !d.pending_deploys_due && d.pending_deploy_backstop_due && d.should_propose,
                "an idle validator over the cap gets exactly the backstop lane"
            );
            let d = decide_lanes(&LaneInputs {
                self_idle_for_recovery_interval: false,
                ..over_cap_idle
            });
            assert!(
                !d.should_propose,
                "before the recovery interval elapses the backstop stays shut"
            );
        }

        #[test]
        fn new_parents_drive_frontier_follow_inside_the_chase_cap() {
            let d = decide_lanes(&LaneInputs {
                has_new_parents: true,
                self_recently_proposed: true,
                lfb_lag_blocks: 5,
                stale_recovery_interval_elapsed: true,
                ..baseline()
            });
            assert!(d.frontier_follow_due && d.should_propose);
        }

        #[test]
        fn backpressure_stops_empty_lanes_but_never_the_deploy_lane() {
            let d = decide_lanes(&LaneInputs {
                empty_frontier_backpressure: true,
                lfb_is_stale: true,
                stale_recovery_interval_elapsed: true,
                ..baseline()
            });
            assert!(!d.should_propose, "empty recovery yields to backpressure");
            let d = decide_lanes(&LaneInputs {
                empty_frontier_backpressure: true,
                has_pending_deploys: true,
                ..baseline()
            });
            assert!(
                d.pending_deploys_due && d.should_propose,
                "user work is never held behind the empty-block cap"
            );
        }

        #[test]
        fn a_not_yet_proposed_validator_gets_the_stale_recovery_lane() {
            let d = decide_lanes(&LaneInputs {
                lfb_is_stale: true,
                stale_recovery_interval_elapsed: true,
                lfb_lag_blocks: 30,
                ..baseline()
            });
            assert!(d.stale_lfb_recovery_due && d.should_propose);
        }

        #[test]
        fn grace_reopens_the_stale_recovery_lane_while_ahead() {
            let d = decide_lanes(&LaneInputs {
                lfb_is_stale: true,
                stale_recovery_interval_elapsed: true,
                self_recently_proposed: true,
                deploy_grace_active: true,
                lfb_lag_blocks: 30,
                ..baseline()
            });
            assert!(d.stale_lfb_recovery_due && d.should_propose);
        }

        /// THE CI STALL ROW. Every stalled shard converges to exactly this
        /// state on every non-leader: the LFB is stale, the temporal window
        /// is open (this validator has been silent for a full recovery
        /// interval), the deploy pools are empty, no new parents arrive
        /// because every peer is equally silenced — and the height key
        /// (`self_recently_proposed`: its old block sits above the frozen
        /// LFB) is permanently true, so the general stale lane never fires.
        /// A validator in this state MUST propose: the recovery interval is
        /// the pacing, and mutual witnessing is the only way the clique can
        /// re-form. This is the silence behind every "waiting for selected
        /// recovery leader" line in the CI stalls.
        #[test]
        fn a_stalled_idle_non_leader_must_get_its_recovery_proposal() {
            let d = decide_lanes(&LaneInputs {
                lfb_is_stale: true,
                stale_recovery_interval_elapsed: true,
                self_recently_proposed: true,
                lfb_lag_blocks: 30,
                ..baseline()
            });
            assert!(
                d.should_propose,
                "a stalled, temporally idle validator proposed nothing: the \
                 height key suppresses the one lane whose pacing (the \
                 stale-recovery interval) already permits it to speak"
            );
        }

        #[test]
        fn the_stalled_leader_still_proposes() {
            let d = decide_lanes(&LaneInputs {
                lfb_is_stale: true,
                stale_recovery_interval_elapsed: true,
                self_recently_proposed: true,
                lag_recovery_leader: true,
                lfb_lag_blocks: 30,
                ..baseline()
            });
            assert!(d.should_propose);
        }

        #[test]
        fn convergence_fires_for_the_leader_when_nothing_routine_is_due() {
            let d = decide_lanes(&LaneInputs {
                idle_recovery_window_open: true,
                lag_recovery_leader: true,
                ..baseline()
            });
            assert!(d.convergence_recovery_selected && d.should_propose);
        }
    }
}
