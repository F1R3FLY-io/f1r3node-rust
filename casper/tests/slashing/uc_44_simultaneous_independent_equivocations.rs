// UC-44 — Two validators equivocate simultaneously (independently).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-44.
// Theorems: T-1 (soundness — only the two equivocators get records),
// T-9.2 (atomic-RMW), T-Idem.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_44_simultaneous_independent_equivocations() {
    let mut harness = SlashingTestHarness::new(4, 100);

    // v0 and v2 equivocate at the same seq, independently.
    let _v0a = harness.sign_block("v0", 5);
    let v0b = harness.sign_block_distinct("v0", 5);
    let _v2a = harness.sign_block("v2", 5);
    let v2b = harness.sign_block_distinct("v2", 5);

    let s_v0 = harness.dispatch(v0b);
    let s_v2 = harness.dispatch(v2b);
    assert_eq!(s_v0, Status::IgnorableEquivocation);
    assert_eq!(s_v2, Status::IgnorableEquivocation);

    // T-1 soundness: only v0 and v2 have records.
    assert!(harness.has_record("v0", 4));
    assert!(harness.has_record("v2", 4));
    assert!(!harness.has_record("v1", 4));
    assert!(!harness.has_record("v3", 4));

    // Slash both.
    let _ = harness.execute_slash("v0");
    let _ = harness.execute_slash("v2");

    let fc = harness.fork_choice();
    assert_eq!(fc.len(), 2);
    assert!(fc.contains(&"v1".to_string()));
    assert!(fc.contains(&"v3".to_string()));
    assert_eq!(harness.coop_vault(), 200);
}
