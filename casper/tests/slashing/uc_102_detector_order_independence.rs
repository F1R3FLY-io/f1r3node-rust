// UC-102 — Detector classification is independent of justification order.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-102.
// Theorems: T-9.11, `detector_permutation_invariant` in
// formal/rocq/slashing/theories/EquivocationDetector.v.
//
// Scenario: two blocks cite the same justification set but in different
// orders. The detector must classify both as Neglected — order
// independence is consensus-critical because protobuf does not guarantee
// repeated-field iteration order across encoders/decoders.

use super::detector_totality_helpers::{assert_neglected, block, justification, DetectorFixture};

#[tokio::test]
async fn uc_102_detector_traversal_is_permutation_independent() {
    let fixture = DetectorFixture::new().await;
    fixture.add_record(0, 0, &[]);

    let missing_pointer = block(
        10,
        fixture.validators[1].clone(),
        1,
        vec![justification(
            fixture.validators[1].clone(),
            fixture.genesis.block_hash.clone(),
        )],
        fixture.validators.clone(),
    );
    let child_a = block(
        11,
        fixture.validators[0].clone(),
        1,
        vec![],
        fixture.validators.clone(),
    );
    let child_b = block(
        12,
        fixture.validators[0].clone(),
        1,
        vec![],
        fixture.validators.clone(),
    );
    fixture.add_block(&missing_pointer);
    fixture.add_block(&child_a);
    fixture.add_block(&child_b);

    let first_order = block(
        20,
        fixture.validators[4].clone(),
        2,
        vec![
            justification(
                fixture.validators[1].clone(),
                missing_pointer.block_hash.clone(),
            ),
            justification(fixture.validators[2].clone(), child_a.block_hash.clone()),
            justification(fixture.validators[3].clone(), child_b.block_hash.clone()),
        ],
        fixture.validators.clone(),
    );
    let second_order = block(
        21,
        fixture.validators[4].clone(),
        3,
        vec![
            justification(fixture.validators[3].clone(), child_b.block_hash.clone()),
            justification(
                fixture.validators[1].clone(),
                missing_pointer.block_hash.clone(),
            ),
            justification(fixture.validators[2].clone(), child_a.block_hash.clone()),
        ],
        fixture.validators.clone(),
    );

    assert_neglected(fixture.check(&first_order).await);
    assert_neglected(fixture.check(&second_order).await);
}
