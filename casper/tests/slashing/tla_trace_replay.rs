// TLA+ trace-replay test driver.
//
// Reference: docs/casper/theory/slashing/design/14-test-plan.md §14.6
// (TLA+ trace replay), Item 4 (Track 7) of the principled-resolution
// session.
//
// Reads a JSON trace file (a hand-authored canonical schedule
// mirroring a TLC counter-example or a representative invariant-
// exercising path; see `scripts/ci/dump-tla-traces.sh` for the
// curation workflow), applies each step to a `SlashingTestHarness`
// via `tla_projection::apply_step`, and asserts the projected final
// state matches the TLA+ model's final state for that schedule.
//
// Trace JSONs at `tla_traces/*.json` are **hand-authored**, NOT
// auto-generated. When a TLA+ action signature changes, the
// affected JSON must be edited manually using the workflow in
// `scripts/ci/dump-tla-traces.sh` (deliberately-false-invariant
// trick → copy TLC counter-example → restore .cfg).
//
// Trace files are stored under
// `casper/tests/slashing/tla_traces/*.json`. One file per spec.
// Each spec has a representative schedule that exercises the
// invariants of interest:
//   • MC_EquivocationDetector — admissible + ignorable equivocation
//   • MC_ConcurrentTracker    — two threads racing on (v, base_seq)
//   • MC_SlashFlow            — full pipeline including ExecuteSlash
//   • MC_TwoLevelSlashing     — direct economic evidence only
//   • MC_WithdrawFlow         — Bug-#10 withdrawal flow
//
// Why trace replay (in addition to property tests):
//   Property tests randomly sample a trace space; trace replay
//   pins specific TLC-discovered schedules so a regression that
//   only surfaces along that exact schedule is caught
//   deterministically. The two layers are complementary.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use casper::rust::epoch::Epoch;
use casper::rust::slashing_authorization::received_slash_deploy_authorized;
use models::rust::bond_generation::BondGeneration;
use serde::Deserialize;

use super::harness::SlashingTestHarness;
use super::tla_projection::{self, ExpectedFinal, StepResult, Trace};

pub fn trace_path(filename: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("slashing");
    p.push("tla_traces");
    p.push(filename);
    p
}

pub fn load_trace(filename: &str) -> Trace {
    let p = trace_path(filename);
    let bytes =
        std::fs::read(&p).unwrap_or_else(|e| panic!("failed to read trace {}: {}", p.display(), e));
    serde_json::from_slice::<Trace>(&bytes)
        .unwrap_or_else(|e| panic!("failed to parse trace {}: {}", p.display(), e))
}

pub fn replay_trace(trace: &Trace) -> SlashingTestHarness {
    let validators = if trace.validators == 0 {
        3
    } else {
        trace.validators
    };
    let stake = if trace.stake_per_validator == 0 {
        100
    } else {
        trace.stake_per_validator
    };
    let mut harness = SlashingTestHarness::new(validators, stake);

    for (idx, step) in trace.schedule.iter().enumerate() {
        match tla_projection::apply_step(&mut harness, step) {
            StepResult::Ok => {}
            StepResult::Skipped(msg) => {
                panic!(
                    "trace `{}`: step #{} ({}) skipped: {}. Trace must be well-formed.",
                    trace.spec, idx, step.action, msg
                );
            }
        }
    }
    harness
}

pub fn assert_final_matches(harness: &SlashingTestHarness, expected: &ExpectedFinal, spec: &str) {
    for v in &expected.slashed {
        assert!(
            harness.pos_state.slashed.contains(v),
            "[{spec}] expected validator {v} to be slashed; pos_state.slashed = {:?}",
            harness.pos_state.slashed
        );
    }
    for v in &expected.active {
        assert!(
            harness.pos_state.active.contains(v),
            "[{spec}] expected validator {v} to be active; pos_state.active = {:?}",
            harness.pos_state.active
        );
    }
    if let Some(cv) = expected.coop_vault {
        assert_eq!(
            harness.coop_vault(),
            cv,
            "[{spec}] coop vault mismatch: expected {cv}, got {}",
            harness.coop_vault()
        );
    }
    for r in &expected.records {
        assert!(
            harness.has_record(&r.validator, r.base_seq),
            "[{spec}] expected record at ({}, {}); records = {:?}",
            r.validator,
            r.base_seq,
            harness.tracker
        );
    }
}

// ─── Per-spec replay tests ────────────────────────────────────────

#[test]
fn replay_mc_equivocation_detector() {
    let trace = load_trace("mc_equivocation_detector.json");
    let harness = replay_trace(&trace);
    assert_final_matches(&harness, &trace.expected_final, &trace.spec);
}

#[test]
fn replay_mc_concurrent_tracker() {
    let trace = load_trace("mc_concurrent_tracker.json");
    let harness = replay_trace(&trace);
    assert_final_matches(&harness, &trace.expected_final, &trace.spec);
}

#[test]
fn replay_mc_slash_flow() {
    let trace = load_trace("mc_slash_flow.json");
    let harness = replay_trace(&trace);
    assert_final_matches(&harness, &trace.expected_final, &trace.spec);
}

#[test]
fn replay_mc_two_level_slashing() {
    let trace = load_trace("mc_two_level_slashing.json");
    let harness = replay_trace(&trace);
    assert_final_matches(&harness, &trace.expected_final, &trace.spec);
}

#[test]
fn replay_mc_withdraw_flow() {
    let trace = load_trace("mc_withdraw_flow.json");
    let harness = replay_trace(&trace);
    assert_final_matches(&harness, &trace.expected_final, &trace.spec);
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct CertifiedRejectionState {
    dependency_state: String,
    dependency_buffered: bool,
    dependency_certified: bool,
    invalid_index: bool,
    slash_evidence: bool,
    child_state: String,
    child_buffered: bool,
    request_pending: bool,
    deliveries: u8,
}

#[derive(Debug, Deserialize)]
struct CertifiedRejectionTrace {
    spec: String,
    schedule: Vec<tla_projection::TraceStep>,
    expected_rejection: CertifiedRejectionState,
}

#[test]
fn replay_mc_certified_rejection_dependency() {
    let path = trace_path("mc_certified_rejection_dependency.json");
    let bytes = std::fs::read(&path).unwrap();
    let trace: CertifiedRejectionTrace = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(trace.spec, "CertifiedRejectionDependency");

    let mut state = CertifiedRejectionState {
        dependency_state: "Absent".to_string(),
        dependency_buffered: true,
        dependency_certified: false,
        invalid_index: false,
        slash_evidence: false,
        child_state: "Waiting".to_string(),
        child_buffered: true,
        request_pending: true,
        deliveries: 1,
    };

    for step in trace.schedule {
        match step.action.as_str() {
            "PublishInvalidIndex" => state.invalid_index = true,
            "RedeliverDependency" => state.deliveries += 1,
            "CertifyNonSlashableRejection" => {
                state.dependency_state = "Rejected".to_string();
                state.dependency_buffered = false;
                state.dependency_certified = true;
                state.invalid_index = true;
                state.slash_evidence = false;
                state.request_pending = false;
            }
            "ResolveChild" => {
                assert_eq!(state.dependency_state, "Rejected");
                state.child_state = "Rejected".to_string();
                state.child_buffered = false;
                state.request_pending = false;
            }
            action => panic!("unknown CertifiedRejectionDependency action {action}"),
        }
    }

    assert_eq!(state, trace.expected_rejection);
}

#[derive(Clone, Debug, Deserialize)]
struct AuthorizedSlashGenerationStep {
    action: String,
    #[serde(default)]
    generation: Option<i64>,
    #[serde(default)]
    evidence_block_number: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct AuthorizedSlashGenerationExpected {
    phase: String,
    generation: i64,
    bond: i64,
    pending_generation: i64,
    stale_rejections: usize,
    current_authorizations: usize,
}

#[derive(Debug, Deserialize)]
struct AuthorizedSlashGenerationTrace {
    spec: String,
    epoch_length: i32,
    reference_block_number: i64,
    initial_bond: i64,
    schedule: Vec<AuthorizedSlashGenerationStep>,
    expected_final: AuthorizedSlashGenerationExpected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthorizedSlashGenerationPhase {
    Bonded,
    PendingWithdraw,
    Withdrawing,
    Withdrawn,
}

impl AuthorizedSlashGenerationPhase {
    fn name(self) -> &'static str {
        match self {
            Self::Bonded => "Bonded",
            Self::PendingWithdraw => "PendingWithdraw",
            Self::Withdrawing => "Withdrawing",
            Self::Withdrawn => "Withdrawn",
        }
    }
}

#[derive(Debug)]
struct AuthorizedSlashGenerationState {
    phase: AuthorizedSlashGenerationPhase,
    generation: BondGeneration,
    bond: i64,
    evidence: BTreeMap<BondGeneration, i64>,
    pending_generation: Option<BondGeneration>,
    stale_rejections: usize,
    current_authorizations: usize,
}

fn generation_authorized(
    trace: &AuthorizedSlashGenerationTrace,
    state: &AuthorizedSlashGenerationState,
    generation: BondGeneration,
    evidence_block_number: i64,
) -> bool {
    let current_epoch = Epoch::new(trace.reference_block_number / i64::from(trace.epoch_length));
    received_slash_deploy_authorized(
        trace.reference_block_number,
        evidence_block_number,
        current_epoch,
        trace.epoch_length,
        Some(generation),
        generation,
        Some(state.generation),
        state.bond,
        true,
    )
    .expect("valid trace domain")
}

fn refresh_generation_pending(
    trace: &AuthorizedSlashGenerationTrace,
    state: &mut AuthorizedSlashGenerationState,
) {
    let pending_generation = state
        .evidence
        .iter()
        .find(|(generation, block_number)| {
            generation_authorized(trace, state, **generation, **block_number)
        })
        .map(|(generation, _)| *generation);
    state.pending_generation = pending_generation;
}

#[test]
fn replay_mc_authorized_slash_generation_lifecycle() {
    let path = trace_path("mc_authorized_slash_generation_lifecycle.json");
    let bytes = std::fs::read(&path).expect("generation trace");
    let trace: AuthorizedSlashGenerationTrace =
        serde_json::from_slice(&bytes).expect("generation trace schema");
    assert_eq!(trace.spec, "AuthorizedSlashFlow");
    let mut state = AuthorizedSlashGenerationState {
        phase: AuthorizedSlashGenerationPhase::Bonded,
        generation: BondGeneration::GENESIS,
        bond: trace.initial_bond,
        evidence: BTreeMap::new(),
        pending_generation: None,
        stale_rejections: 0,
        current_authorizations: 0,
    };

    for step in &trace.schedule {
        match step.action.as_str() {
            "RecordSlashableInvalid" => {
                let generation = BondGeneration::new(step.generation.expect("evidence generation"))
                    .expect("nonnegative evidence generation");
                state.evidence.insert(
                    generation,
                    step.evidence_block_number.expect("evidence block number"),
                );
                refresh_generation_pending(&trace, &mut state);
            }
            "RequestWithdraw" => {
                assert_eq!(state.phase, AuthorizedSlashGenerationPhase::Bonded);
                state.phase = AuthorizedSlashGenerationPhase::PendingWithdraw;
            }
            "BeginWithdraw" => {
                assert_eq!(state.phase, AuthorizedSlashGenerationPhase::PendingWithdraw);
                state.phase = AuthorizedSlashGenerationPhase::Withdrawing;
            }
            "CompleteWithdrawal" => {
                assert_eq!(state.phase, AuthorizedSlashGenerationPhase::Withdrawing);
                state.phase = AuthorizedSlashGenerationPhase::Withdrawn;
                state.bond = 0;
                refresh_generation_pending(&trace, &mut state);
                assert_eq!(state.pending_generation, None);
            }
            "FreshBond" => {
                assert_eq!(state.phase, AuthorizedSlashGenerationPhase::Withdrawn);
                state.generation = state.generation.next().expect("next bond generation");
                state.phase = AuthorizedSlashGenerationPhase::Bonded;
                state.bond = trace.initial_bond;
                refresh_generation_pending(&trace, &mut state);
            }
            "AssertSlashDisabled" => {
                let generation = BondGeneration::new(step.generation.expect("disabled generation"))
                    .expect("nonnegative disabled generation");
                assert!(!generation_authorized(
                    &trace,
                    &state,
                    generation,
                    step.evidence_block_number.expect("disabled evidence block"),
                ));
                state.stale_rejections += 1;
            }
            "AssertSlashEnabled" => {
                let generation = BondGeneration::new(step.generation.expect("enabled generation"))
                    .expect("nonnegative enabled generation");
                assert!(generation_authorized(
                    &trace,
                    &state,
                    generation,
                    step.evidence_block_number.expect("enabled evidence block"),
                ));
                state.current_authorizations += 1;
            }
            action => panic!("unknown AuthorizedSlashFlow action {action}"),
        }
    }

    let actual = AuthorizedSlashGenerationExpected {
        phase: state.phase.name().to_string(),
        generation: state.generation.get(),
        bond: state.bond,
        pending_generation: state
            .pending_generation
            .expect("current generation pending")
            .get(),
        stale_rejections: state.stale_rejections,
        current_authorizations: state.current_authorizations,
    };
    assert_eq!(actual, trace.expected_final);
}
