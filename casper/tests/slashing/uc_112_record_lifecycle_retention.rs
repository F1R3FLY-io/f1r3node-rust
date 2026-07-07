// UC-112 — Detector updates retain pre-existing detected-hash entries.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-112.
// Theorems: T-9.11, T-5 (record monotonicity).
//
// Scenario: a record already carries a detected hash from an earlier
// run. When a new block triggers `check_neglected_equivocations_with_update`
// the tracker.add(updated) call must *append*, not overwrite — the
// existing hashes survive. Combined with T-5, this is what gives records
// monotonic growth: once detected, always recorded.

use super::detector_totality_helpers::{assert_valid, block, DetectorFixture};

#[tokio::test]
async fn uc_112_detector_update_retains_existing_detected_hashes() {
    let fixture = DetectorFixture::new().await;
    let old_detector = block(
        10,
        fixture.validators[2].clone(),
        1,
        vec![],
        fixture.validators.clone(),
    );
    fixture.add_block(&old_detector);
    fixture.add_record(0, 0, &[old_detector.block_hash.clone()]);

    let current = block(
        20,
        fixture.validators[3].clone(),
        2,
        vec![],
        fixture.validators[1..].to_vec(),
    );

    assert_valid(fixture.check(&current).await);

    let records = fixture
        .dag_storage
        .equivocation_records()
        .expect("equivocation records");
    let record = records
        .iter()
        .find(|record| {
            record.equivocator == fixture.validators[0]
                && record.equivocation_base_block_seq_num == 0
        })
        .expect("retained record");

    assert!(record
        .equivocation_detected_block_hashes
        .contains(&old_detector.block_hash));
    assert!(record
        .equivocation_detected_block_hashes
        .contains(&current.block_hash));
    assert_eq!(record.equivocation_detected_block_hashes.len(), 2);
}
