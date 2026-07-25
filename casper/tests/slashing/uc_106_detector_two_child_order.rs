// UC-106 — Two *distinct* equivocation children are required to neglect.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-106.
// Theorems: T-9.11, T-2 (detection complete).
//
// Scenario: one equivocation child alone is not neglect — only two
// distinct children above the same base seq trigger NeglectedEquivocation.
// Order of discovery of the two children must not matter; flipping the
// justification order must produce the same classification.

use super::detector_totality_helpers::{
    assert_neglected, assert_valid, block, justification, DetectorFixture,
};

#[tokio::test]
async fn uc_106_two_distinct_children_are_required() {
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
    fixture.add_block(&child_a);
    fixture.add_block(&child_b);

    let duplicate_child = block(
        20,
        fixture.validators[3].clone(),
        2,
        vec![
            justification(fixture.validators[1].clone(), child_a.block_hash.clone()),
            justification(fixture.validators[2].clone(), child_a.block_hash.clone()),
        ],
        fixture.validators.clone(),
    );
    let distinct_children = block(
        21,
        fixture.validators[3].clone(),
        3,
        vec![
            justification(fixture.validators[2].clone(), child_b.block_hash.clone()),
            justification(fixture.validators[1].clone(), child_a.block_hash.clone()),
        ],
        fixture.validators.clone(),
    );

    assert_valid(fixture.check(&duplicate_child).await);
    assert_neglected(fixture.check(&distinct_children).await);
}
