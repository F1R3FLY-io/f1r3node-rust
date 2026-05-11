// TLA+ → SlashingTestHarness projection.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.6
// (TLA+ trace replay), Item 4 (Track 7) of the principled-resolution
// session.
//
// Each TLA+ spec under formal/tlaplus/slashing/ has a small set of
// actions that mutate global variables (bonds, blocks, slashedSet,
// equivocationRecords, etc.). This module defines a projection from
// each TLA action to a sequence of `SlashingTestHarness` operations.
// Replaying a TLC-emitted (or hand-derived) trace through this
// projection drives the harness identically to the TLA+ model, and
// asserting the final state proves bisimilarity at the example-trace
// level. Property bisim is covered by the proptests at
// `prop_t_triple_bisim_*.rs`; this is the model-checker-tier
// counterpart.
//
// Spec coverage (5 specs):
//   • MC_EquivocationDetector  — sign-honest / sign-equivocating /
//     detect / record
//   • MC_ConcurrentTracker     — concurrent record insertion (post-
//     fix RMW-atomicity)
//   • MC_SlashFlow             — full pipeline:
//     sign-honest → sign-equivocating → record → slash
//   • MC_TwoLevelSlashing      — neglect closure: A equivocates,
//     B cites A's invalid block without slashing → both slashed
//   • MC_WithdrawFlow          — Bug-#10 withdrawal flow:
//     withdraw-succeeds / withdraw-fails / retry-from-failed

#![allow(dead_code)]

use serde::Deserialize;

use super::harness::SlashingTestHarness;
use super::types::{base_seq_from_seq, Status};

/// One step of a TLA+ trace.
///
/// Each `action` corresponds to a TLA+ Action symbol (e.g.
/// `SignHonest`, `SignEquivocating`, `ExecuteSlash`); the `args`
/// give the existential witnesses TLC chose for the action's
/// quantified parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct TraceStep {
    pub action: String,
    #[serde(default)]
    pub args: TraceArgs,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TraceArgs {
    /// Validator identifier, e.g. "v0".
    pub v: Option<String>,
    /// Sequence number.
    pub s: Option<u64>,
    /// Offender — used by ExecuteSlash to denote the slashed validator.
    pub o: Option<String>,
    /// Withdrawer — used by WithdrawSucceeds / WithdrawFails.
    pub w: Option<String>,
    /// Witness — used by RecordEvidence to denote the witnessing block hash.
    pub witness: Option<u64>,
    /// Validator B who cites the offender without slashing
    /// (TwoLevelSlashing).
    pub b: Option<String>,
    /// Block-num — used by SignHonestB-style actions where two seq
    /// numbers are needed.
    pub s2: Option<u64>,
}

/// Result of a trace-step application.
#[derive(Debug, Clone)]
pub enum StepResult {
    /// The action ran successfully.
    Ok,
    /// The action's preconditions were not met. Mirrors TLA's
    /// "action not enabled" semantics — the trace would have
    /// branched away from this state. Test code typically asserts
    /// that traces are well-formed (no Skipped steps).
    Skipped(String),
}

/// Apply one trace step to the harness.
///
/// The mapping is per-spec: a single action name (e.g. "SignHonest")
/// has the same projection across MC_EquivocationDetector and
/// MC_SlashFlow because the actions are defined identically in both
/// specs (the SlashFlow spec EXTENDS the same action set). When two
/// specs use the same name with different semantics, the trace JSON
/// disambiguates via the wrapping `spec` field at the top level —
/// callers route to the right projection based on `spec`.
pub fn apply_step(harness: &mut SlashingTestHarness, step: &TraceStep) -> StepResult {
    let validator = step.args.v.as_deref().unwrap_or("");
    let offender = step.args.o.as_deref().unwrap_or("");
    let other = step.args.b.as_deref().unwrap_or("");
    let withdrawer = step.args.w.as_deref().unwrap_or("");
    let seq = step.args.s.unwrap_or(0);

    match step.action.as_str() {
        // ─── EquivocationDetector / SlashFlow / TwoLevelSlashing ───
        "SignHonest" => {
            let _hash = harness.sign_block(validator, seq);
            StepResult::Ok
        }
        "SignEquivocating" => {
            // Two distinct blocks at the same (validator, seq).
            let h1 = harness.sign_block(validator, seq);
            let h2 = harness.sign_block_distinct(validator, seq);
            let status = harness.detect(h2);
            // Mirror the TLA Action: an equivocation block immediately
            // mints an EquivocationRecord at (v, seq - 1).
            if let Some(base_seq) = base_seq_from_seq(seq) {
                harness.record_equivocation(validator, base_seq, h2);
            }
            let _ = (h1, status); // silence unused
            StepResult::Ok
        }
        "RecordEvidence" => {
            // Standalone record-evidence action used by ConcurrentTracker.
            let witness = step.args.witness.unwrap_or(0);
            harness.record_equivocation(validator, seq, witness);
            StepResult::Ok
        }
        "ExecuteSlash" => {
            let result = harness.execute_slash(offender);
            if result.success {
                StepResult::Ok
            } else {
                StepResult::Skipped(format!("ExecuteSlash on {offender} returned {result:?}"))
            }
        }
        "SignNeglecting" => {
            // TwoLevelSlashing: B cites the offender's invalid block
            // without slashing. The harness flags B as itself
            // slashable via the catch-all (status = NeglectedEquivocation
            // semantics at the harness projection).
            let neglecter_hash = harness.sign_block(other, seq);
            let _ = harness.dispatch_with_status(neglecter_hash, Status::SlashableOther);
            StepResult::Ok
        }
        // ─── WithdrawFlow ───
        "WithdrawSucceeds" => {
            // Replay-side projection: WithdrawSucceeds on `w` is a
            // dispatch-tier no-op for the slashing-harness state —
            // it changes withdrawer-map / pos-balance which the
            // harness does not project. We only track that the
            // step was applied (the TLA-side invariants are checked
            // by TLC via WithdrawFlow.cfg). Returning Ok satisfies
            // the trace-replay assertion that the schedule is
            // achievable in the harness's projection.
            let _ = withdrawer;
            StepResult::Ok
        }
        "WithdrawFails" => {
            let _ = withdrawer;
            StepResult::Ok
        }
        "RetryFromFailed" => {
            let _ = withdrawer;
            StepResult::Ok
        }
        unknown => StepResult::Skipped(format!("unknown action: {unknown}")),
    }
}

/// Top-level trace structure parsed from JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct Trace {
    pub spec: String,
    #[serde(default)]
    pub validators: usize,
    #[serde(default)]
    pub stake_per_validator: i64,
    pub schedule: Vec<TraceStep>,
    #[serde(default)]
    pub expected_final: ExpectedFinal,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ExpectedFinal {
    pub slashed: Vec<String>,
    pub active: Vec<String>,
    pub coop_vault: Option<i64>,
    pub records: Vec<ExpectedRecord>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExpectedRecord {
    pub validator: String,
    pub base_seq: u64,
}
