use super::detector_totality_helpers::{
    assert_neglected, assert_valid, block, hash, justification, DetectorFixture,
};

#[tokio::test]
async fn pre_fix_bug_11_missing_pointer_is_non_contributing() {
    let fixture = DetectorFixture::new().await;
    fixture.add_record(0, 0, &[]);

    let current = block(
        20,
        fixture.validators[2].clone(),
        2,
        vec![justification(fixture.validators[1].clone(), hash(99))],
        fixture.validators.clone(),
    );

    assert_valid(fixture.check(&current).await);
}

#[tokio::test]
async fn pre_fix_bug_11_duplicate_child_does_not_count_twice() {
    let fixture = DetectorFixture::new().await;
    fixture.add_record(0, 0, &[]);

    let child = block(
        10,
        fixture.validators[0].clone(),
        1,
        vec![],
        fixture.validators.clone(),
    );
    fixture.add_block(&child);

    let current = block(
        21,
        fixture.validators[3].clone(),
        2,
        vec![
            justification(fixture.validators[1].clone(), child.block_hash.clone()),
            justification(fixture.validators[2].clone(), child.block_hash.clone()),
        ],
        fixture.validators.clone(),
    );

    assert_valid(fixture.check(&current).await);
}

#[tokio::test]
async fn pre_fix_bug_11_two_distinct_children_are_decisive() {
    let fixture = DetectorFixture::new().await;
    fixture.add_record(0, 0, &[]);

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
        2,
        vec![],
        fixture.validators.clone(),
    );
    fixture.add_block(&child_a);
    fixture.add_block(&child_b);

    let current = block(
        22,
        fixture.validators[3].clone(),
        2,
        vec![
            justification(fixture.validators[1].clone(), child_a.block_hash.clone()),
            justification(fixture.validators[2].clone(), child_b.block_hash.clone()),
        ],
        fixture.validators.clone(),
    );

    assert_neglected(fixture.check(&current).await);
}
