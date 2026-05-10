use super::divergence_class::{classify, DivergenceClass, DivergenceReason};

fn threat_priority(class: DivergenceClass, extra_stake: i64, slash_delay: i64) -> i64 {
    let class_score = match class {
        DivergenceClass::UnexpectedDivergence => 1_000,
        DivergenceClass::CandidateBoundaryDivergence => 100,
        DivergenceClass::PermittedBugFix => 20,
        DivergenceClass::Bisimilar => 0,
    };
    class_score + extra_stake + slash_delay
}

#[test]
fn uc_99_projection_and_assumption_boundaries_rank_above_bisimilar_rows() {
    let projection = threat_priority(classify(DivergenceReason::ProjectionBoundary), 0, 10);
    let assumption = threat_priority(
        classify(DivergenceReason::PreconditionFuzzingBoundary),
        8,
        0,
    );
    let bisimilar = threat_priority(DivergenceClass::Bisimilar, 50, 0);
    assert!(projection > bisimilar);
    assert!(assumption > bisimilar);
}

#[test]
fn uc_99_unexpected_would_rank_first_and_fail_policy_elsewhere() {
    let unexpected = threat_priority(DivergenceClass::UnexpectedDivergence, 0, 0);
    let boundary = threat_priority(classify(DivergenceReason::ProjectionBoundary), 99, 99);
    assert!(unexpected > boundary);
}
