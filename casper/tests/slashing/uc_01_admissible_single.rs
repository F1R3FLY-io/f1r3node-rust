// UC-01 — Single admissible equivocation by one validator is detected,
// recorded, and slashed.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-01.
// This is the canonical happy-path scenario that exercises the
// entire pipeline: detection → record-mint → slash. Pre-fix this
// already worked for AdmissibleEquivocation (the variant the original
// dispatcher handled); the post-fix preserves it while extending the
// same treatment to Ignorable, NeglectedEquivocation, and the 14
// other slashable variants (bug fixes #1 + #3).

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_01_admissible_single_full_pipeline() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates: two distinct blocks at seq=5.
    let b1 = harness.sign_block("v0", 5);
    let b1_prime = harness.sign_block_distinct("v0", 5);

    // Pre-existing record makes the second block admissible (the
    // first observation became Ignorable).
    harness.record_equivocation("v0", 4, b1);
    let status = harness.dispatch(b1_prime);
    assert_eq!(status, Status::AdmissibleEquivocation);

    // Tracker has the record with both witnesses.
    assert!(harness.has_record("v0", 4));
    let witnesses = harness.record_witnesses("v0", 4);
    assert!(witnesses.contains(&b1));
    assert!(witnesses.contains(&b1_prime));

    // Apply the slash transition.
    let initial_coop = harness.coop_vault();
    let result = harness.execute_slash("v0");
    assert!(result.success);

    // Post-state: v0 is slashed, removed from active, bond is zero,
    // coop vault gained the original 100 stake.
    assert_eq!(harness.bond("v0"), 0);
    assert!(!harness.is_active("v0"));
    assert_eq!(harness.coop_vault(), initial_coop + 100);

    // Other validators are untouched.
    assert_eq!(harness.bond("v1"), 100);
    assert!(harness.is_active("v1"));
    assert_eq!(harness.bond("v2"), 100);
    assert!(harness.is_active("v2"));

    // Fork-choice excludes the slashed validator.
    let fc = harness.fork_choice();
    assert!(!fc.contains(&"v0".to_string()));
    assert!(fc.contains(&"v1".to_string()));
    assert!(fc.contains(&"v2".to_string()));
}
