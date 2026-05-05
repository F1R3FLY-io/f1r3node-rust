// TLA+ trace-replay test driver.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.6
// (TLA+ trace replay), Item 4 (Track 7) of the principled-resolution
// session.
//
// Reads a JSON trace file (produced by hand from a TLC schedule, or
// dumped via the `scripts/ci/dump-tla-traces.sh` tool), applies each
// step to a `SlashingTestHarness` via `tla_projection::apply_step`,
// and asserts the projected final state matches the TLA+ model's
// final state for that schedule.
//
// Trace files are stored under
// `casper/tests/slashing/tla_traces/*.json`. One file per spec.
// Each spec has a representative schedule that exercises the
// invariants of interest:
//   • MC_EquivocationDetector — admissible + ignorable equivocation
//   • MC_ConcurrentTracker    — two threads racing on (v, base_seq)
//   • MC_SlashFlow            — full pipeline including ExecuteSlash
//   • MC_TwoLevelSlashing     — neglect closure (A equivocates,
//                               B neglects, both slashed)
//   • MC_WithdrawFlow         — Bug-#10 withdrawal flow
//
// Why trace replay (in addition to property tests):
//   Property tests randomly sample a trace space; trace replay
//   pins specific TLC-discovered schedules so a regression that
//   only surfaces along that exact schedule is caught
//   deterministically. The two layers are complementary.

#![allow(dead_code)]

use std::path::PathBuf;

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
    let bytes = std::fs::read(&p)
        .unwrap_or_else(|e| panic!("failed to read trace {}: {}", p.display(), e));
    serde_json::from_slice::<Trace>(&bytes)
        .unwrap_or_else(|e| panic!("failed to parse trace {}: {}", p.display(), e))
}

pub fn replay_trace(trace: &Trace) -> SlashingTestHarness {
    let validators = if trace.validators == 0 { 3 } else { trace.validators };
    let stake = if trace.stake_per_validator == 0 { 100 } else { trace.stake_per_validator };
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
