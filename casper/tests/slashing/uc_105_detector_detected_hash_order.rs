// UC-105 — `equivocation_detected_block_hashes` membership is order-independent.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-105.
// Theorems: T-9.11.
//
// Scenario: when a record already contains a detected-hash entry, the
// detector short-circuits to `Neglected` regardless of where in the
// justification list the detected hash appears. This is the symmetric
// fix to UC-102 (justification iteration order) — the record's
// detected-hash set must also be a *set*, not order-sensitive.

use super::detector_totality_helpers::{assert_neglected, block, justification, DetectorFixture};

#[tokio::test]
async fn uc_105_detected_hash_evidence_is_order_independent() {
    let fixture = DetectorFixture::new().await;

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
    let detector = block(
        11,
        fixture.validators[2].clone(),
        1,
        vec![],
        fixture.validators.clone(),
    );
    fixture.add_block(&missing_pointer);
    fixture.add_block(&detector);
    fixture.add_record(0, 0, &[detector.block_hash.clone()]);

    let current = block(
        20,
        fixture.validators[3].clone(),
        2,
        vec![
            justification(
                fixture.validators[1].clone(),
                missing_pointer.block_hash.clone(),
            ),
            justification(fixture.validators[2].clone(), detector.block_hash.clone()),
        ],
        fixture.validators.clone(),
    );

    assert_neglected(fixture.check(&current).await);
}
