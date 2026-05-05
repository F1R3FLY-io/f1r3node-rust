// Property-based test for T-9.6 (self-regression caught post-fix #6).
//
// Theorem: T-9.6 (`t_9_6_self_regression_detected`,
// formal/rocq/slashing/theories/BugFixSelfRegression.v).
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.7.
//
// Property: for every block whose creator-justification cites a
// previous block by the same sender at a *higher* sequence number,
// `dispatch` classifies the block as `JustificationRegression` and
// the post-fix #3 catch-all mints an EquivocationRecord.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::{BlockMeta, Status};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_9_6_self_regression_always_detected(
        early_seq in 0u64..10,
        gap in 1u64..20,
    ) {
        let later_seq = early_seq + gap;
        let mut harness = SlashingTestHarness::new(2, 100);

        // v0 publishes at later_seq first.
        let later = harness.sign_block("v0", later_seq);

        // v0 publishes a regressing block at early_seq citing the
        // later block as its creator-justification.
        let regressing = later.wrapping_add(100_000);
        harness.dag.blocks.insert(
            regressing,
            BlockMeta {
                hash: regressing,
                sender: "v0".into(),
                seq: early_seq,
                justifications: vec![("v0".into(), later)],
                slash_targets: vec![],
            },
        );

        let s = harness.dispatch(regressing);
        prop_assert_eq!(s, Status::JustificationRegression,
            "post-fix #6: any self-regressing block (early_seq={} citing later_seq={}) is detected",
            early_seq, later_seq);
        prop_assert!(harness.has_record("v0", early_seq.saturating_sub(1)),
            "post-fix #3: dispatcher mints record for JustificationRegression");
    }
}
