// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-101 — Detector tolerates a missing nested offender pointer.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-101.
// Theorems: T-9.11 (detector totality), via `detector_no_unsafe_lookup`
// in formal/rocq/slashing/theories/EquivocationDetector.v.
//
// Scenario: a justification points at a *nested* offender block whose
// own self-justification pointer is missing from the store. Pre-fix the
// detector would `unwrap` and panic; post-fix it treats the missing
// pointer as a non-contributing justification — the block classifies
// Valid, not Err.

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
