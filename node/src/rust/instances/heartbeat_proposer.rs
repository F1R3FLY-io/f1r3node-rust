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
    idle_recovery_round_attempted: Option<u64>,
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
    last_recovery_round: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct FinalityProgressStatus {
    stalled_for: Duration,
    recovery_round: Option<u64>,
    recovery_round_due: bool,
}

impl FinalityProgress {
    fn new(now: Instant) -> Self {
        Self {
            last_finalized_block: None,
            last_progress_at: now,
            last_recovery_round: None,
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
            self.last_recovery_round = None;
        }

        let stalled_for = now.saturating_duration_since(self.last_progress_at);
        let timeout_nanos = timeout.as_nanos().max(1);
        let elapsed_periods = stalled_for.as_nanos() / timeout_nanos;
        let recovery_round = elapsed_periods
            .checked_sub(1)
            .map(|round| u64::try_from(round).unwrap_or(u64::MAX));
        let recovery_round_due =
            recovery_round.is_some_and(|round| self.last_recovery_round != Some(round));

        FinalityProgressStatus {
            stalled_for,
            recovery_round,
            recovery_round_due,
        }
    }

    fn record_recovery_attempt(&mut self, recovery_round: u64) {
        self.last_recovery_round = Some(recovery_round);
    }
}

fn empty_frontier_pressure(
    snapshot: &CasperSnapshot,
    max_unfinalized_blocks: i64,
    has_new_parent_with_user_deploys: bool,
    self_recently_proposed: bool,
) -> Result<EmptyFrontierPressure, casper::rust::errors::CasperError> {
    let max_unfinalized_blocks = usize::try_from(max_unfinalized_blocks).unwrap_or(usize::MAX);
    if has_new_parent_with_user_deploys || !self_recently_proposed {
        return Ok(EmptyFrontierPressure {
            max_unfinalized_blocks,
            ..EmptyFrontierPressure::default()
        });
    }

    let unfinalized_blocks = snapshot.dag.non_finalized_blocks()?.len();
    Ok(EmptyFrontierPressure {
        unfinalized_blocks,
        max_unfinalized_blocks,
        backpressure: unfinalized_blocks >= max_unfinalized_blocks,
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
        _standalone: bool,
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
                        deploy_grace_active,
                        &mut finality_progress,
                    )
                    .await
                    {
                        Ok(outcome) => {
                            if let Some(recovery_round) = outcome.idle_recovery_round_attempted {
                                finality_progress.record_recovery_attempt(recovery_round);
                            }
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
/// 3. Checks for pending deploys, newly observed user-deploy parents, or stalled finality
/// 4. Triggers a propose if conditions are met
///
/// Exposed for testing - allows direct testing of decision logic without spawning tasks.
async fn do_heartbeat_check(
    casper: Arc<dyn MultiParentCasper + Send + Sync>,
    trigger_propose: &ProposeFunction,
    validator_identity: &ValidatorIdentity,
    config: &HeartbeatConf,
    deploy_grace_active: bool,
    finality_progress: &mut FinalityProgress,
) -> Result<HeartbeatCheckResult, casper::rust::errors::CasperError> {
    let snapshot: CasperSnapshot = casper.get_snapshot().await?;
    let progress_timeout = std::cmp::max(config.max_lfb_age, config.check_interval);
    let progress_status = finality_progress.observe(
        &snapshot.last_finalized_block,
        Instant::now(),
        progress_timeout,
    );

    let proposal_validators = snapshot.current_proposal_validators();
    let is_bonded = proposal_validators.contains(&validator_identity.public_key.bytes);

    if !is_bonded {
        tracing::info!("Heartbeat: Validator is not bonded, skipping heartbeat propose");
        Ok(HeartbeatCheckResult::default())
    } else {
        tracing::debug!("Heartbeat: Validator is bonded, checking LFB age");
        return check_lfb_and_propose(
            casper.clone(),
            snapshot,
            trigger_propose,
            validator_identity,
            config,
            deploy_grace_active,
            progress_status,
            proposal_validators,
        )
        .await;
    }
}

async fn check_lfb_and_propose(
    casper: Arc<dyn MultiParentCasper + Send + Sync>,
    snapshot: CasperSnapshot,
    trigger_propose: &ProposeFunction,
    validator_identity: &ValidatorIdentity,
    config: &HeartbeatConf,
    deploy_grace_active: bool,
    finality_progress: FinalityProgressStatus,
    proposal_validators: Vec<Validator>,
) -> Result<HeartbeatCheckResult, casper::rust::errors::CasperError> {
    let pending_deploy_max_lag = config.advanced.pending_deploy_max_lag;
    let advanced_deploy_recovery_max_lag = config.advanced.deploy_recovery_max_lag;
    let stale_recovery_min_interval_ms = config.stale_recovery_min_interval.as_millis();

    let has_pending_deploys = casper
        .has_pending_deploys_in_storage_for_snapshot(&snapshot)
        .await?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let last_finalized_block_number = snapshot
        .dag
        .lookup(&snapshot.last_finalized_block)?
        .map(|meta| meta.block_number)
        .unwrap_or(0);
    let latest_block_number = snapshot.dag.latest_block_number();
    let lfb_lag_blocks = latest_block_number.saturating_sub(last_finalized_block_number);

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

    let has_new_parent_with_user_deploys =
        inspect_parent_updates(&snapshot, validator_identity, &casper);
    let deploy_recovery_max_lag =
        std::cmp::max(pending_deploy_max_lag, advanced_deploy_recovery_max_lag);
    let empty_frontier_pressure = empty_frontier_pressure(
        &snapshot,
        config.advanced.empty_frontier_max_unfinalized_blocks,
        has_new_parent_with_user_deploys,
        self_recently_proposed,
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
    let can_propose_pending_deploys_while_ahead = if deploy_grace_active {
        lfb_lag_blocks <= deploy_recovery_max_lag
    } else {
        lfb_lag_blocks <= pending_deploy_max_lag
    };
    let pending_deploys_due = has_pending_deploys
        && (!self_recently_proposed
            || (can_propose_pending_deploys_while_ahead && !self_proposed_too_recently));
    let pending_deploy_backstop_due = has_pending_deploys
        && self_recently_proposed
        && !can_propose_pending_deploys_while_ahead
        && self_latest_block_timestamp_ms
            .map(|timestamp_ms| now.saturating_sub(timestamp_ms) >= stale_recovery_min_interval_ms)
            .unwrap_or(true)
        && !self_proposed_too_recently;
    let frontier_follow_due = has_new_parent_with_user_deploys && !self_proposed_too_recently;
    let idle_recovery_round = finality_progress
        .recovery_round
        .filter(|_| finality_progress.recovery_round_due);
    let idle_recovery_leader = idle_recovery_round.is_some_and(|round| {
        is_finality_recovery_leader(
            proposal_validators.clone(),
            validator_identity,
            last_finalized_block_number,
            round,
        )
    });
    let idle_recovery_due = !has_pending_deploys
        && idle_recovery_round.is_some()
        && idle_recovery_leader
        && (!empty_frontier_backpressure || !self_recently_proposed)
        && !self_proposed_too_recently;
    let should_propose = pending_deploys_due
        || pending_deploy_backstop_due
        || frontier_follow_due
        || idle_recovery_due;

    if should_propose {
        let idle_recovery_round_attempted =
            idle_recovery_due.then_some(idle_recovery_round).flatten();
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
                "new user-deploy parent requires one bounded support proposal (lag={}, self_recently_proposed={}, cooldown_ms={})",
                lfb_lag_blocks,
                self_recently_proposed,
                config.self_propose_cooldown.as_millis()
            )
        } else {
            format!(
                "actual LFB hash has not advanced for {}ms; recovery round {} selected this validator (lag={})",
                finality_progress.stalled_for.as_millis(),
                idle_recovery_round.unwrap_or_default(),
                lfb_lag_blocks
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
                    idle_recovery_round_attempted: None,
                })
            }
            ProposerResult::Failure(status, seq_num) => {
                if matches!(
                    status,
                    ProposeStatus::Failure(ProposeFailure::RecoveryDeferred)
                ) {
                    tracing::debug!(
                        "Heartbeat: Propose deferred with {} (seqNum {})",
                        status,
                        seq_num
                    );
                } else {
                    tracing::warn!(
                        "Heartbeat: Propose failed with {} (seqNum {})",
                        status,
                        seq_num
                    );
                }
                // Only escalate backoff for explicit bug failures.
                // Recoverable propose races should retry on the normal heartbeat cadence.
                Ok(HeartbeatCheckResult {
                    bug_failure: matches!(status, ProposeStatus::Failure(ProposeFailure::BugError)),
                    refresh_deploy_grace_window: has_pending_deploys
                        || has_new_parent_with_user_deploys,
                    idle_recovery_round_attempted,
                })
            }
            ProposerResult::Success(_, _) => {
                tracing::info!("Heartbeat: Successfully created block");
                Ok(HeartbeatCheckResult {
                    bug_failure: false,
                    refresh_deploy_grace_window: has_pending_deploys
                        || has_new_parent_with_user_deploys,
                    idle_recovery_round_attempted,
                })
            }
            ProposerResult::Started(seq_num) => {
                tracing::info!("Heartbeat: Async propose started (seqNum {})", seq_num);
                Ok(HeartbeatCheckResult {
                    bug_failure: false,
                    refresh_deploy_grace_window: has_pending_deploys
                        || has_new_parent_with_user_deploys,
                    idle_recovery_round_attempted,
                })
            }
        }
    } else {
        let reason = if has_pending_deploys
            && self_recently_proposed
            && !can_propose_pending_deploys_while_ahead
        {
            let pending_backstop_remaining_ms = self_latest_block_timestamp_ms
                .map(|timestamp_ms| {
                    stale_recovery_min_interval_ms.saturating_sub(now.saturating_sub(timestamp_ms))
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
        } else if has_new_parent_with_user_deploys && self_proposed_too_recently {
            format!(
                "bounded user-deploy support waits for the {}ms self-proposal cooldown",
                config.self_propose_cooldown.as_millis()
            )
        } else if idle_recovery_round.is_some() && !idle_recovery_leader {
            format!(
                "actual LFB hash is stalled in recovery round {}; waiting for the selected validator",
                idle_recovery_round.unwrap_or_default()
            )
        } else if idle_recovery_round.is_some() && empty_frontier_backpressure {
            format!(
                "idle recovery is backpressured: unfinalized_blocks={} reached cap {}",
                empty_frontier_pressure.unfinalized_blocks,
                empty_frontier_pressure.max_unfinalized_blocks
            )
        } else if finality_progress.recovery_round.is_some()
            && !finality_progress.recovery_round_due
        {
            format!(
                "idle recovery round {} was already attempted for the current LFB",
                finality_progress.recovery_round.unwrap_or_default()
            )
        } else {
            format!(
                "actual LFB hash advanced {}ms ago; idle recovery is not due",
                finality_progress.stalled_for.as_millis()
            )
        };
        tracing::debug!("Heartbeat: No action needed - reason: {}", reason);
        Ok(HeartbeatCheckResult {
            bug_failure: false,
            refresh_deploy_grace_window: has_pending_deploys || has_new_parent_with_user_deploys,
            idle_recovery_round_attempted: None,
        })
    }
}

/// Detect a newly observed latest message carrying a user deploy.
/// Missing self history, genesis, metadata gaps, and system-only latest-message
/// churn do not authorize support proposals; stalled-finality recovery handles
/// progress independently through its bounded rotating leader.
fn inspect_parent_updates(
    snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    casper: &Arc<dyn MultiParentCasper + Send + Sync>,
) -> bool {
    let validator_id = &validator_identity.public_key.bytes;

    let last_block_hash = match snapshot.dag.latest_message_hash(validator_id) {
        Some(hash) => hash,
        None => return false,
    };

    let block_meta = match snapshot.dag.lookup(&last_block_hash) {
        Ok(Some(meta)) => meta,
        _ => return false,
    };

    if block_meta.parents.is_empty() {
        return false;
    }

    let justified_latest: std::collections::HashMap<Vec<u8>, BlockHash> = block_meta
        .justifications
        .iter()
        .map(|j| (j.validator.to_vec(), j.latest_block_hash.clone()))
        .collect();

    for (validator, current_hash) in snapshot.dag.latest_message_hashes().iter() {
        let known_hash_opt = if *validator == *validator_id {
            Some(&last_block_hash)
        } else {
            justified_latest.get(validator.as_ref())
        };

        if known_hash_opt != Some(current_hash) {
            if let Ok(Some(block)) = casper.block_store().get(current_hash) {
                if block
                    .body
                    .deploys
                    .iter()
                    .any(|processed| !is_system_deploy_id(&processed.deploy.sig))
                {
                    return true;
                }
            }
        }
    }

    false
}

fn is_finality_recovery_leader(
    validators: Vec<Validator>,
    validator_identity: &ValidatorIdentity,
    last_finalized_block_number: i64,
    recovery_round: u64,
) -> bool {
    finality_recovery_leader(validators, last_finalized_block_number, recovery_round)
        .is_some_and(|leader| leader == validator_identity.public_key.bytes)
}

fn finality_recovery_leader(
    mut validators: Vec<Validator>,
    last_finalized_block_number: i64,
    recovery_round: u64,
) -> Option<Validator> {
    if validators.is_empty() {
        return None;
    }

    validators.sort_unstable();
    validators.dedup();

    let finalized_height = u128::try_from(last_finalized_block_number).ok()?;
    let validator_count = u128::try_from(validators.len()).ok()?;
    let leader_index =
        usize::try_from((finalized_height + u128::from(recovery_round)) % validator_count).ok()?;
    validators.get(leader_index).cloned()
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
    use std::collections::BTreeSet;

    use casper::rust::heartbeat_signal::new_heartbeat_signal_ref;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use proptest::prelude::*;
    use prost::bytes::Bytes;

    use super::*;

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

    #[test]
    fn finality_progress_opens_each_recovery_round_once_and_resets_on_progress() {
        let base = Instant::now();
        let timeout = Duration::from_secs(5);
        let first_lfb = BlockHash::from_static(b"first");
        let second_lfb = BlockHash::from_static(b"second");
        let mut progress = FinalityProgress::new(base);

        let initial = progress.observe(&first_lfb, base, timeout);
        assert_eq!(initial.recovery_round, None);
        assert!(!initial.recovery_round_due);

        let round_zero = progress.observe(&first_lfb, base + timeout, timeout);
        assert_eq!(round_zero.recovery_round, Some(0));
        assert!(round_zero.recovery_round_due);
        progress.record_recovery_attempt(0);

        let duplicate = progress.observe(&first_lfb, base + timeout, timeout);
        assert_eq!(duplicate.recovery_round, Some(0));
        assert!(!duplicate.recovery_round_due);

        let round_one = progress.observe(&first_lfb, base + timeout * 2, timeout);
        assert_eq!(round_one.recovery_round, Some(1));
        assert!(round_one.recovery_round_due);

        let reset = progress.observe(&second_lfb, base + timeout * 2, timeout);
        assert_eq!(reset.recovery_round, None);
        assert!(!reset.recovery_round_due);
        assert_eq!(reset.stalled_for, Duration::ZERO);
    }

    proptest! {
        #[test]
        fn recovery_leader_is_unique_and_permutation_invariant(
            validator_seeds in prop::collection::vec(any::<u8>(), 1..64),
            finalized_height in 0i64..i64::MAX,
            recovery_round in any::<u64>(),
        ) {
            let validators: Vec<Validator> = validator_seeds
                .iter()
                .map(|seed| Bytes::from(vec![*seed; models::rust::validator::LENGTH]))
                .collect();
            let mut reversed = validators.clone();
            reversed.reverse();

            let leader = finality_recovery_leader(
                validators,
                finalized_height,
                recovery_round,
            );
            let reversed_leader = finality_recovery_leader(
                reversed,
                finalized_height,
                recovery_round,
            );

            prop_assert!(leader.is_some());
            prop_assert_eq!(leader, reversed_leader);
        }

        #[test]
        fn recovery_leader_rotation_visits_every_unique_validator(
            validator_seeds in prop::collection::vec(any::<u8>(), 1..64),
            finalized_height in 0i64..i64::MAX,
        ) {
            let validators: Vec<Validator> = validator_seeds
                .iter()
                .map(|seed| Bytes::from(vec![*seed; models::rust::validator::LENGTH]))
                .collect();
            let expected: BTreeSet<Validator> = validators.iter().cloned().collect();
            let observed: BTreeSet<Validator> = (0..expected.len())
                .map(|round| {
                    finality_recovery_leader(
                        validators.clone(),
                        finalized_height,
                        u64::try_from(round).expect("test round fits u64"),
                    )
                    .expect("non-empty validator set has a leader")
                })
                .collect();

            prop_assert_eq!(observed, expected);
        }
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

        fn create_recovery_deferred_propose_function() -> (Arc<AtomicUsize>, Arc<ProposeFunction>) {
            use casper::rust::blocks::proposer::propose_result::{ProposeFailure, ProposeStatus};
            use casper::rust::blocks::proposer::proposer::ProposerResult;

            let count = Arc::new(AtomicUsize::new(0));
            let count_clone = count.clone();
            let func: Arc<ProposeFunction> = Arc::new(move |_casper, _is_async| {
                count_clone.fetch_add(1, Ordering::SeqCst);
                Box::pin(async {
                    Ok(ProposerResult::Failure(
                        ProposeStatus::Failure(ProposeFailure::RecoveryDeferred),
                        7,
                    ))
                })
            });
            (count, func)
        }

        fn test_hash(byte: u8) -> BlockHash { Bytes::from(vec![byte; 32]) }

        fn test_validator(byte: u8) -> Bytes {
            Bytes::from(vec![byte; models::rust::validator::LENGTH])
        }

        fn initial_finality_progress() -> FinalityProgress { FinalityProgress::new(Instant::now()) }

        fn bond_only_validator_in_snapshot(snapshot: &mut CasperSnapshot, validator: &Validator) {
            casper::rust::casper::test_helpers::TestCasperWithSnapshot::bond_validator_in_snapshot(
                snapshot,
                validator.clone(),
            );
            snapshot.parents[0]
                .body
                .state
                .bonds
                .retain(|bond| bond.validator == *validator);
            snapshot.on_chain_state.active_validators = vec![validator.clone()];
            snapshot.on_chain_state.bonds_map.clear();
            snapshot
                .on_chain_state
                .bonds_map
                .insert(validator.clone(), 100);
        }

        fn stalled_finality_progress(
            last_finalized_block: BlockHash,
            timeout: Duration,
        ) -> FinalityProgress {
            FinalityProgress {
                last_finalized_block: Some(last_finalized_block),
                last_progress_at: Instant::now()
                    .checked_sub(timeout.saturating_add(Duration::from_millis(1)))
                    .expect("test timeout must fit Instant"),
                last_recovery_round: None,
            }
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
                successful_state_effect_indices: Default::default(),
                rejected_state_effects: Default::default(),
                protocol_version: casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
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
            bond_only_validator_in_snapshot(&mut snapshot, &validator_id);

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
            bond_only_validator_in_snapshot(&mut snapshot, &validator_id);

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
            let mut finality_progress = initial_finality_progress();

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
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
        async fn do_heartbeat_check_triggers_one_recovery_proposal_after_observed_lfb_stall() {
            // Create validator identity
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();

            // Create snapshot with no deploys but validator is bonded
            let mut snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            bond_only_validator_in_snapshot(&mut snapshot, &validator_id);
            let last_finalized_block = snapshot.last_finalized_block.clone();

            // Stale LFB (60 seconds old)
            let lfb = create_lfb_with_age(60000);

            // Create counting propose function
            let (propose_count, propose_func) = create_counting_propose_function();

            // Config with short max_lfb_age so LFB IS stale
            let config = HeartbeatConf {
                enabled: true,
                check_interval: Duration::from_secs(1),
                max_lfb_age: Duration::from_secs(1),
                self_propose_cooldown: Duration::from_secs(15),
                ..HeartbeatConf::default()
            };
            let mut finality_progress =
                stalled_finality_progress(last_finalized_block, config.max_lfb_age);
            let preview = finality_progress.observe(
                &snapshot.last_finalized_block,
                Instant::now(),
                std::cmp::max(config.max_lfb_age, config.check_interval),
            );
            assert!(preview.recovery_round_due);
            assert!(is_finality_recovery_leader(
                snapshot.current_proposal_validators(),
                &validator,
                0,
                preview.recovery_round.expect("stalled fixture has a round"),
            ));
            assert!(snapshot
                .dag
                .latest_message_hash(&validator.public_key.bytes)
                .is_none());
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new(snapshot, lfb),
            );

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                &mut finality_progress,
            )
            .await;

            assert!(result.is_ok(), "do_heartbeat_check should succeed");
            assert_eq!(
                propose_count.load(Ordering::SeqCst),
                1,
                "Should trigger one selected-validator proposal after observed LFB stagnation"
            );
        }

        #[tokio::test]
        async fn do_heartbeat_check_treats_recovery_deferred_as_non_bug() {
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();
            let mut snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            bond_only_validator_in_snapshot(&mut snapshot, &validator_id);
            let last_finalized_block = snapshot.last_finalized_block.clone();
            let lfb = create_lfb_with_age(60000);
            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new(snapshot, lfb),
            );
            let (propose_count, propose_func) = create_recovery_deferred_propose_function();
            let config = HeartbeatConf {
                enabled: true,
                check_interval: Duration::from_secs(1),
                max_lfb_age: Duration::from_secs(1),
                self_propose_cooldown: Duration::from_secs(15),
                ..HeartbeatConf::default()
            };
            let mut finality_progress =
                stalled_finality_progress(last_finalized_block, config.max_lfb_age);

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                &mut finality_progress,
            )
            .await
            .expect("heartbeat check");

            assert_eq!(propose_count.load(Ordering::SeqCst), 1);
            assert!(!result.bug_failure);
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
            let mut finality_progress = initial_finality_progress();

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
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
        async fn do_heartbeat_check_does_not_propose_from_missing_latest_message_alone() {
            // Create validator identity
            let validator = create_test_validator_identity();
            let validator_id = validator.public_key.bytes.clone();

            // Create snapshot with no deploys but validator is bonded
            let mut snapshot =
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            bond_only_validator_in_snapshot(&mut snapshot, &validator_id);

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
            let mut finality_progress = initial_finality_progress();

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
                false,
                &mut finality_progress,
            )
            .await;

            assert!(result.is_ok(), "do_heartbeat_check should succeed");
            assert_eq!(
                propose_count.load(Ordering::SeqCst),
                0,
                "Missing latest-message state alone must not create an empty-block feedback loop"
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
            bond_only_validator_in_snapshot(&mut snapshot, &validator_id);

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
            let mut finality_progress = initial_finality_progress();

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
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
            let last_finalized_block = snapshot.last_finalized_block.clone();

            let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(
                casper::rust::casper::test_helpers::TestCasperWithSnapshot::new(snapshot, lfb),
            );
            let (propose_count, propose_func) = create_counting_propose_function();
            let config = empty_frontier_backpressure_config();
            let mut finality_progress =
                stalled_finality_progress(last_finalized_block, config.max_lfb_age);

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
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
            let mut finality_progress = initial_finality_progress();

            let result = do_heartbeat_check(
                casper,
                &*propose_func,
                &validator,
                &config,
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
    }
}
