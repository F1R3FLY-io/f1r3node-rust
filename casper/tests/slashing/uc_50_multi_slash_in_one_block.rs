// UC-50 — Multiple slashes applied in succession (one block can
// dispatch slashes to multiple validators).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-50.
// Theorems: T-Idem, T-11.
// Reference: design/06-proposing-and-effect.md §6.4.
//
// Scenario: validators v0 and v1 both equivocate independently.
// A single proposer round dispatches slashes to both. The Coop
// vault collects both bonds; the active set drops by exactly 2.

use super::harness::SlashingTestHarness;

#[test]
fn uc_50_multi_slash_in_succession() {
    let mut harness = SlashingTestHarness::new(4, 50);

    // v0 equivocates.
    let _v0a = harness.sign_block("v0", 5);
    let v0b = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(v0b);
    assert!(harness.has_record("v0", 4));

    // v1 equivocates.
    let _v1a = harness.sign_block("v1", 5);
    let v1b = harness.sign_block_distinct("v1", 5);
    let _ = harness.dispatch(v1b);
    assert!(harness.has_record("v1", 4));

    // Apply both slashes.
    let _ = harness.execute_slash("v0");
    let _ = harness.execute_slash("v1");

    assert_eq!(harness.bond("v0"), 0);
    assert_eq!(harness.bond("v1"), 0);
    assert!(!harness.is_active("v0"));
    assert!(!harness.is_active("v1"));
    assert_eq!(harness.coop_vault(), 100, "both bonds in the vault (50 + 50)");

    // Active set drops by 2: v2 and v3 remain.
    let active = harness.fork_choice();
    assert_eq!(active.len(), 2);
    assert!(active.contains(&"v2".to_string()));
    assert!(active.contains(&"v3".to_string()));
}
