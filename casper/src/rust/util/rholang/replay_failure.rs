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

    EffectStateMismatch {
        effect: String,
        boundary: String,
        expected: String,
        actual: String,
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

    /// Cost-Accounted Rho item 2494 (overflow → deterministic rejection): a
    /// per-validator / per-client supply CREDIT in
    /// `CloseBlockDeploy::dual_write_supply` (the `Σ⟦v⟧` mint, the fee-carve
    /// running total, the `F_v` collection credit, the fee→`Σ⟦v⟧` convert, or the
    /// genesis client `Σ⟦c⟧` funding-slot seed) overflowed the `i64` phlogiston
    /// ceiling — `old_balance + addend` would exceed `i64::MAX`. The `i64` bound
    /// IS the supply cap (no economic `SUPPLY_MAX` parameter); a `checked_add`
    /// that would wrap is a DETERMINISTIC block rejection — every node computes the
    /// same sum from the same block + pre-state, so all nodes reject identically —
    /// NEVER a panic (a panic is non-deterministic across nodes ⇒ network halt).
    /// This is the OVERFLOW-side sibling of the underflow / write-readback guards
    /// [`ReplayFailure::ReplayAdmissionMismatch`] / [`ReplayFailure::ReplaySupplyMismatch`]:
    /// the ledger sum is conserved OR the block is deterministically rejected on
    /// overflow (mirrored formally by `MintingInjection`/`BoundedLedger`
    /// `supply_credit_conserved_or_rejected`). Returned on BOTH the play
    /// (block-creation) and replay (validation) paths — the overflow is a pure
    /// function of the block, so both reject the same block the same way.
    ReplaySupplyOverflow {
        channel: String,
        old_balance: i64,
        addend: i64,
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
    pub fn internal_error(msg: String) -> Self { ReplayFailure::InternalError { msg } }

    pub fn replay_status_mismatch(initial_failed: bool, replay_failed: bool) -> Self {
        ReplayFailure::ReplayStatusMismatch {
            initial_failed,
            replay_failed,
        }
    }

    pub fn unused_comm_event(msg: String) -> Self { ReplayFailure::UnusedCOMMEvent { msg } }

    pub fn replay_cost_mismatch(initial_cost: u64, replay_cost: u64) -> Self {
        ReplayFailure::ReplayCostMismatch {
            initial_cost,
            replay_cost,
        }
    }

    pub fn effect_state_mismatch(
        effect: String,
        boundary: String,
        expected: String,
        actual: String,
    ) -> Self {
        ReplayFailure::EffectStateMismatch {
            effect,
            boundary,
            expected,
            actual,
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

    pub fn replay_supply_overflow(channel: String, old_balance: i64, addend: i64) -> Self {
        ReplayFailure::ReplaySupplyOverflow {
            channel,
            old_balance,
            addend,
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
            ReplayFailure::EffectStateMismatch {
                effect,
                boundary,
                expected,
                actual,
            } => write!(
                f,
                "Effect state mismatch: effect={}, boundary={}, expected={}, actual={}",
                effect, boundary, expected, actual
            ),
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
            ReplayFailure::ReplaySupplyOverflow {
                channel,
                old_balance,
                addend,
            } => {
                write!(
                    f,
                    "Phlogiston supply overflow on {}: balance {} + {} exceeds i64::MAX \
                     (deterministic block rejection, not a panic)",
                    channel, old_balance, addend
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
                    expected_admitted, replay_admitted, expected_rejected, replay_rejected, detail
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
