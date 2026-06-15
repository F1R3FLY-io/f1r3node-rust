// UC-108 — Duplicate paths to the same child do not count as two children.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-108.
// Theorems: T-9.11.
//
// Scenario: two justifications independently route to the *same*
// equivocation-child hash. The detector must deduplicate by child hash
// before checking the "len > 1" two-children condition — pre-fix it
// counted paths and could classify Neglected on a single distinct
// child (a false positive that would slash an honest validator).

use super::detector_totality_helpers::{assert_valid, block, justification, DetectorFixture};

#[tokio::test]
async fn uc_108_duplicate_child_paths_do_not_create_two_child_evidence() {
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
        20,
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
