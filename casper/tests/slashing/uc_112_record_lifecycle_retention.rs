// UC-112 — Detector passes retain pre-existing detected-hash entries.
//
// Maps to: docs/casper/theory/slashing/slashing-specification.md §12 UC-112.
// Theorems: T-5 (record monotonicity / no-overwrite), T-9.1a (FV audit #6).
//
// Scenario: a record already carries a detected hash from an earlier run.
// When a new block drives `check_neglected_equivocations_with_update` the
// tracker must not clobber that record — the pre-existing hash survives.
//
// FV audit #6 remediation: the offender here is UNBONDED in the validating
// block (its bonds are `validators[1..]`, which excludes the offender
// `validators[0]`). Pre-fix, the unbonded offender resolved to
// `EquivocationDetected` and the caller STAMPED the current block's hash into
// the record (len grew to 2) — the observation-order-dependent pollution that
// forked consensus. Post-fix the unbonded offender resolves to
// `EquivocationOblivious`, so the detector performs NO write: the pre-existing
// hash is retained (no-overwrite, T-5) and the current block is NOT appended
// (no unbonded stamp). The witness set stays at exactly its seeded contents.

use super::detector_totality_helpers::{assert_valid, block, DetectorFixture};

#[tokio::test]
async fn uc_112_detector_pass_retains_seed_and_never_stamps_unbonded() {
    let fixture = DetectorFixture::new().await;
    let old_detector = block(
        10,
        fixture.validators[2].clone(),
        1,
        vec![],
        fixture.validators.clone(),
    );
    fixture.add_block(&old_detector);
    // Record for the offender validators[0], pre-seeded with one detected hash.
    fixture.add_record(0, 0, std::slice::from_ref(&old_detector.block_hash));

    // The validating block bonds validators[1..] — i.e. the offender
    // validators[0] is UNBONDED (absent from the bond map).
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

    // Retention (T-5, no-overwrite): the pre-existing hash survives the pass.
    assert!(record
        .equivocation_detected_block_hashes
        .contains(&old_detector.block_hash));
    // FV audit #6: the unbonded offender's record is NOT stamped — the current
    // block's hash is never appended, so the witness set stays at its seed.
    assert!(!record
        .equivocation_detected_block_hashes
        .contains(&current.block_hash));
    assert_eq!(record.equivocation_detected_block_hashes.len(), 1);
}
