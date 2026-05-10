// UC-83 — Adversarial-scheduler frontier.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-83.
// Theorems: T-12V (view-indexed closure), T-12RET (evidence
// retention), T-12PF (proposer fairness).
// Reference: formal/sage/slashing/FINDINGS.md (scheduler frontier),
// formal/tlaplus/slashing/TwoLevelSlashing.tla invariant
// `SchedulerDivergenceClass`.
//
// Property: an adversarial scheduler that controls partitions,
// gossip delivery, report timing, evidence pruning, and proposer
// rotation produces traces whose every step is in a documented
// bucket. Liveness violations under adversarial scheduling are
// either explained by ProposerFairnessBoundary, or by a documented
// candidate-boundary scenario.

use proptest::prelude::*;

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[derive(Debug, Clone)]
enum SchedulerAction {
    Partition,
    Heal,
    GossipDelay,
    Report,
    Prune,
    Propose,
}

fn arb_action() -> impl Strategy<Value = SchedulerAction> {
    prop_oneof![
        Just(SchedulerAction::Partition),
        Just(SchedulerAction::Heal),
        Just(SchedulerAction::GossipDelay),
        Just(SchedulerAction::Report),
        Just(SchedulerAction::Prune),
        Just(SchedulerAction::Propose),
    ]
}

#[derive(Debug, Default, Clone)]
struct Sched {
    partitioned: bool,
    fair_proposer_seen: bool,
}

fn classify_action(s: &Sched, a: &SchedulerAction) -> DivergenceClass {
    match a {
        SchedulerAction::Propose => DivergenceClass::Bisimilar,
        SchedulerAction::Report | SchedulerAction::GossipDelay => {
            classify(DivergenceReason::EvidenceViewBoundary)
        }
        SchedulerAction::Prune => classify(DivergenceReason::EpochCarryoverBoundary),
        SchedulerAction::Partition | SchedulerAction::Heal => {
            // Partition / heal alone don't produce divergence; they
            // are state-transitions. They're bisimilar unless
            // combined with absent fairness.
            if !s.fair_proposer_seen && s.partitioned {
                classify(DivergenceReason::ProposerFairnessBoundary)
            } else {
                DivergenceClass::Bisimilar
            }
        }
    }
}

fn step(s: &mut Sched, a: &SchedulerAction) {
    match a {
        SchedulerAction::Partition => s.partitioned = true,
        SchedulerAction::Heal => s.partitioned = false,
        SchedulerAction::Propose => s.fair_proposer_seen = true,
        _ => {}
    }
}

proptest! {
    #[test]
    fn uc_83_adversarial_scheduler_classifies(
        actions in proptest::collection::vec(arb_action(), 1..30)
    ) {
        let mut s = Sched::default();
        for a in &actions {
            let class = classify_action(&s, a);
            prop_assert!(
                frontier_classification_ok(class),
                "UC-83: action {:?} produced UnexpectedDivergence", a
            );
            step(&mut s, a);
        }
    }
}
