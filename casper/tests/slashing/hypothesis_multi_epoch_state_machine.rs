// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-76 — Rule-based multi-epoch state-machine frontier.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-76.
// Theorems: T-12EID (epoch eligibility), T-12HYP (hypothesis bound).
// Reference: formal/sage/slashing/FINDINGS.md row 12 (multi-epoch
// state machine), formal/tlaplus/slashing/TwoLevelSlashing.tla
// invariants `Inv_EpochEligibleInCurrent` /
// `Inv_StaleEvidenceNotEligible`.
//
// Property: a randomized rule-based state machine that drives
// validator churn (bond / unbond / slash / epoch-rollover) plus
// evidence accumulation produces traces whose every step lands in
// one of the documented buckets (Bisimilar, PermittedBugFix, or
// CandidateBoundaryDivergence). Stale / off-era / rebonded evidence
// is filtered before it can seed the current-epoch closure.

use proptest::prelude::*;

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[derive(Debug, Clone)]
enum Step {
    Bond(u8),
    Unbond(u8),
    Slash(u8),
    AccrueEvidence { offender: u8, epoch: u8 },
    EpochRoll,
}

fn arb_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0u8..6).prop_map(Step::Bond),
        (0u8..6).prop_map(Step::Unbond),
        (0u8..6).prop_map(Step::Slash),
        (0u8..6, 0u8..4).prop_map(|(o, e)| Step::AccrueEvidence {
            offender: o,
            epoch: e
        }),
        Just(Step::EpochRoll),
    ]
}

#[derive(Debug, Clone, Default)]
struct EpochState {
    bonded: std::collections::BTreeSet<u8>,
    slashed: std::collections::BTreeSet<u8>,
    evidence: Vec<(u8, u8)>, // (offender, epoch_of_evidence)
    current_epoch: u8,
}

fn classify_step(state: &EpochState, step: &Step) -> DivergenceClass {
    match step {
        Step::AccrueEvidence { epoch, .. } if *epoch < state.current_epoch => {
            // Off-era evidence is filtered: this is the
            // current-validator boundary case (T-12EID).
            classify(DivergenceReason::EpochCarryoverBoundary)
        }
        Step::Slash(offender) if !state.bonded.contains(offender) => {
            // Slashing an unbonded validator: T-9.5 + projection
            // boundary (the active-set filter would reject this in
            // production).
            classify(DivergenceReason::CurrentValidatorBoundary)
        }
        _ => DivergenceClass::Bisimilar,
    }
}

fn apply(state: &mut EpochState, step: &Step) {
    match step {
        Step::Bond(v) => {
            if !state.slashed.contains(v) {
                state.bonded.insert(*v);
            }
        }
        Step::Unbond(v) => {
            state.bonded.remove(v);
        }
        Step::Slash(v) => {
            if state.bonded.contains(v) {
                state.bonded.remove(v);
                state.slashed.insert(*v);
            }
        }
        Step::AccrueEvidence { offender, epoch } if *epoch >= state.current_epoch => {
            state.evidence.push((*offender, *epoch));
        }
        Step::AccrueEvidence { .. } => {}
        Step::EpochRoll => {
            state.current_epoch = state.current_epoch.saturating_add(1);
            // Stale-evidence filter: keep only evidence at or after
            // the new current_epoch.
            state.evidence.retain(|(_, e)| *e >= state.current_epoch);
        }
    }
}

proptest! {
    #[test]
    fn uc_76_multi_epoch_state_machine_classifies(
        steps in proptest::collection::vec(arb_step(), 1..40)
    ) {
        let mut state = EpochState::default();
        for step in &steps {
            let class = classify_step(&state, step);
            prop_assert!(
                frontier_classification_ok(class),
                "UC-76: step {:?} produced UnexpectedDivergence", step
            );
            apply(&mut state, step);
        }
        // Invariant: no slashed validator is in bonded.
        prop_assert!(
            state.bonded.is_disjoint(&state.slashed),
            "UC-76: bonded ∩ slashed must stay empty"
        );
        // Invariant: every retained evidence is at or after current_epoch.
        for (_, e) in &state.evidence {
            prop_assert!(
                *e >= state.current_epoch,
                "UC-76: stale evidence escaped the epoch-rollover filter"
            );
        }
    }
}
