use super::detector_totality_helpers::{assert_valid, block, justification, DetectorFixture};

#[tokio::test]
async fn uc_107_validator_set_churn_missing_pointer_is_deterministic() {
    let fixture = DetectorFixture::new().await;
    fixture.add_record(0, 0, &[]);

    let historical_without_offender = block(
        10,
        fixture.validators[1].clone(),
        1,
        vec![justification(
            fixture.validators[1].clone(),
            fixture.genesis.block_hash.clone(),
        )],
        fixture.validators[1..].to_vec(),
    );
    fixture.add_block(&historical_without_offender);

    let current = block(
        20,
        fixture.validators[2].clone(),
        2,
        vec![justification(
            fixture.validators[1].clone(),
            historical_without_offender.block_hash.clone(),
        )],
        fixture.validators.clone(),
    );

    assert_valid(fixture.check(&current).await);
}
