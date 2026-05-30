// See casper/src/main/scala/coop/rchain/casper/util/rholang/ReplayFailure.scala

#[derive(Debug, Clone, PartialEq)]
pub enum ReplayFailure {
    InternalError {
        msg: String,
    },

    ReplayStatusMismatch {
        initial_failed: bool,
        replay_failed: bool,
    },

    UnusedCOMMEvent {
        msg: String,
    },

    ReplayCostMismatch {
        initial_cost: u64,
        replay_cost: u64,
    },

    /// Cost-Accounted Rho Stage B (Decision 6.3): the per-validator supply
    /// balance `Σ⟦v⟧` written by `CloseBlockDeploy::post_eval` on replay did not
    /// match the expected `new_n` (write-readback integrity). A divergence here
    /// signals a non-deterministic supply mint between play and replay — a
    /// consensus fork — and is a sibling of [`ReplayFailure::ReplayCostMismatch`].
    ReplaySupplyMismatch {
        validator: String,
        expected_balance: i64,
        replay_balance: i64,
    },

    /// Cost-Accounted Rho WD-D2 (acceptance gate): the per-signature acceptance
    /// gate RECOMPUTED on replay (over `block.body.deploys` against the block's
    /// start state) disagreed with what the block actually committed. A
    /// divergence here means a proposer admitted a deploy the funding gate would
    /// reject (a double-spend / oversubscription — TM-CA-153), or the recomputed
    /// settlement-debit total differs from what the block applied — either of
    /// which is a CONSENSUS FORK. Sibling of [`ReplayFailure::ReplayCostMismatch`]
    /// / [`ReplayFailure::ReplaySupplyMismatch`]; the three guard the three views
    /// of the supply quantity (pre-state read, in-pass residual, post-state
    /// balance). `detail` carries a human-readable cause; the counts pin the
    /// admitted/rejected set sizes for diagnosis.
    ReplayAdmissionMismatch {
        expected_admitted: usize,
        replay_admitted: usize,
        expected_rejected: usize,
        replay_rejected: usize,
        detail: String,
    },

    SystemDeployErrorMismatch {
        play_error: String,
        replay_error: String,
    },
}

impl ReplayFailure {
    pub fn internal_error(msg: String) -> Self {
        ReplayFailure::InternalError { msg }
    }

    pub fn replay_status_mismatch(initial_failed: bool, replay_failed: bool) -> Self {
        ReplayFailure::ReplayStatusMismatch {
            initial_failed,
            replay_failed,
        }
    }

    pub fn unused_comm_event(msg: String) -> Self {
        ReplayFailure::UnusedCOMMEvent { msg }
    }

    pub fn replay_cost_mismatch(initial_cost: u64, replay_cost: u64) -> Self {
        ReplayFailure::ReplayCostMismatch {
            initial_cost,
            replay_cost,
        }
    }

    pub fn replay_supply_mismatch(
        validator: String,
        expected_balance: i64,
        replay_balance: i64,
    ) -> Self {
        ReplayFailure::ReplaySupplyMismatch {
            validator,
            expected_balance,
            replay_balance,
        }
    }

    pub fn replay_admission_mismatch(
        expected_admitted: usize,
        replay_admitted: usize,
        expected_rejected: usize,
        replay_rejected: usize,
        detail: String,
    ) -> Self {
        ReplayFailure::ReplayAdmissionMismatch {
            expected_admitted,
            replay_admitted,
            expected_rejected,
            replay_rejected,
            detail,
        }
    }

    pub fn system_deploy_error_mismatch(play_error: String, replay_error: String) -> Self {
        ReplayFailure::SystemDeployErrorMismatch {
            play_error,
            replay_error,
        }
    }
}

impl std::fmt::Display for ReplayFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplayFailure::InternalError { msg } => {
                write!(f, "Internal error: {}", msg)
            }
            ReplayFailure::ReplayStatusMismatch {
                initial_failed,
                replay_failed,
            } => {
                write!(
                    f,
                    "Replay status mismatch: initial_failed={}, replay_failed={}",
                    initial_failed, replay_failed
                )
            }
            ReplayFailure::UnusedCOMMEvent { msg } => {
                write!(f, "Unused COMM event: {}", msg)
            }
            ReplayFailure::ReplayCostMismatch {
                initial_cost,
                replay_cost,
            } => {
                write!(
                    f,
                    "Replay cost mismatch: initial_cost={}, replay_cost={}",
                    initial_cost, replay_cost
                )
            }
            ReplayFailure::ReplaySupplyMismatch {
                validator,
                expected_balance,
                replay_balance,
            } => {
                write!(
                    f,
                    "Replay supply mismatch for validator {}: expected_balance={}, replay_balance={}",
                    validator, expected_balance, replay_balance
                )
            }
            ReplayFailure::ReplayAdmissionMismatch {
                expected_admitted,
                replay_admitted,
                expected_rejected,
                replay_rejected,
                detail,
            } => {
                write!(
                    f,
                    "Replay admission mismatch: expected_admitted={}, replay_admitted={}, \
                     expected_rejected={}, replay_rejected={}; {}",
                    expected_admitted,
                    replay_admitted,
                    expected_rejected,
                    replay_rejected,
                    detail
                )
            }
            ReplayFailure::SystemDeployErrorMismatch {
                play_error,
                replay_error,
            } => {
                write!(
                    f,
                    "System deploy error mismatch:\n  Play error: {}\n  Replay error: {}",
                    play_error, replay_error
                )
            }
        }
    }
}
