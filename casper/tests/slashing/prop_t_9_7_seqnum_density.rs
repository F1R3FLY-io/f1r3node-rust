// Property-based test for T-9.7 (BFS seq-number density).
//
// Theorem: T-9.7 (`t_9_7_finds_descendant_with_gap`,
// formal/rocq/slashing/theories/BugFixSeqNumDensity.v:84).
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.8.
//
// Property: an equivocation is detected even when the validator
// has skipped sequence numbers (under partition recovery). The
// post-fix detector walks the creator-justification chain looking
// for any block with seq > base, not exact-match base+1.
//
// The harness's `detect` operates on (sender, seq) pairs directly,
// so it does not exercise the BFS code path — but it does verify
// that detection holds for any seq pair, including those with
// gaps. The full BFS-density proof is in Rocq.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::Status;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_9_7_equivocation_detected_with_seq_gap(
        early_seq in 0u64..5,
        gap in 2u64..20,
    ) {
        let later_seq = early_seq + gap;
        let mut harness = SlashingTestHarness::new(2, 100);

        // Validator publishes at early_seq, then skips to later_seq
        // (gap >= 2 means seq numbers are NOT dense).
        let _b_early = harness.sign_block("v0", early_seq);
        let _b_later = harness.sign_block("v0", later_seq);

        // Equivocate at later_seq — detection must hold despite
        // the gap (post-fix #7).
        let bad = harness.sign_block_distinct("v0", later_seq);
        let s = harness.dispatch(bad);
        prop_assert_eq!(s, Status::IgnorableEquivocation,
            "T-9.7: equivocation at gapped seq still detected");
        prop_assert!(harness.has_record("v0", later_seq.saturating_sub(1)),
            "T-9.7: dispatcher records the equivocation at base = later_seq - 1");
    }
}
