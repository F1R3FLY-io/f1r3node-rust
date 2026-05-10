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
