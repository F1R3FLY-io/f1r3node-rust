use super::detector_totality_helpers::{
    assert_neglected, block, hash, justification, DetectorFixture,
};

#[tokio::test]
async fn uc_104_missing_direct_lookup_does_not_abort_detector() {
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

    let current = block(
        20,
        fixture.validators[4].clone(),
        2,
        vec![
            justification(fixture.validators[1].clone(), hash(250)),
            justification(fixture.validators[2].clone(), child_a.block_hash.clone()),
            justification(fixture.validators[3].clone(), child_b.block_hash.clone()),
        ],
        fixture.validators.clone(),
    );

    assert_neglected(fixture.check(&current).await);
}
