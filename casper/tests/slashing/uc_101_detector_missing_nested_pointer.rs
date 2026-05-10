use super::detector_totality_helpers::{assert_valid, block, justification, DetectorFixture};

#[tokio::test]
async fn uc_101_missing_nested_offender_pointer_is_non_contributing() {
    let fixture = DetectorFixture::new().await;
    fixture.add_record(0, 0, &[]);

    let nested = block(
        10,
        fixture.validators[1].clone(),
        1,
        vec![justification(
            fixture.validators[1].clone(),
            fixture.genesis.block_hash.clone(),
        )],
        fixture.validators.clone(),
    );
    fixture.add_block(&nested);

    let current = block(
        20,
        fixture.validators[2].clone(),
        2,
        vec![justification(
            fixture.validators[1].clone(),
            nested.block_hash.clone(),
        )],
        fixture.validators.clone(),
    );

    assert_valid(fixture.check(&current).await);
}
