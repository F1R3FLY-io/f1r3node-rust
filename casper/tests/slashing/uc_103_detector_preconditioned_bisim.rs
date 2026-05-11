// UC-103 — Detector behaves identically to the pre-fix path when all
// justification pointers are complete (preconditioned bisimulation).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-103.
// Theorems: T-9.11, `detector_bisim_under_complete_pointers` in
// formal/rocq/slashing/theories/EquivocationDetector.v.
//
// Scenario: complete-pointer view => post-fix detector classifies a
// 2-child equivocation the same way the pre-fix detector would. This is
// the "regression-free" half of the totality fix — we only changed
// behavior on the missing-pointer edge.

use super::detector_totality_helpers::{assert_neglected, block, justification, DetectorFixture};

#[tokio::test]
async fn uc_103_complete_pointer_view_keeps_pre_fix_behavior() {
    let fixture = DetectorFixture::new().await;
    fixture.add_record(0, 0, &[]);

    let child_a = block(
        10,
        fixture.validators[0].clone(),
        1,
        vec![],
        fixture.validators.clone(),
    );
    let child_b = block(
        11,
        fixture.validators[0].clone(),
        1,
        vec![],
        fixture.validators.clone(),
    );
    let observer_a = block(
        12,
        fixture.validators[1].clone(),
        1,
        vec![justification(
            fixture.validators[0].clone(),
            child_a.block_hash.clone(),
        )],
        fixture.validators.clone(),
    );
    let observer_b = block(
        13,
        fixture.validators[2].clone(),
        1,
        vec![justification(
            fixture.validators[0].clone(),
            child_b.block_hash.clone(),
        )],
        fixture.validators.clone(),
    );
    fixture.add_block(&child_a);
    fixture.add_block(&child_b);
    fixture.add_block(&observer_a);
    fixture.add_block(&observer_b);

    let current = block(
        20,
        fixture.validators[3].clone(),
        2,
        vec![
            justification(fixture.validators[2].clone(), observer_b.block_hash.clone()),
            justification(fixture.validators[1].clone(), observer_a.block_hash.clone()),
        ],
        fixture.validators.clone(),
    );

    assert_neglected(fixture.check(&current).await);
}
