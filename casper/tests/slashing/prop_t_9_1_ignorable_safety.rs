// Property-based test for T-9.1 (Ignorable equivocation safety).
//
// Theorem: T-9.1 (`t_9_1_ignorable_recorded`,
// formal/rocq/slashing/theories/BugFixIgnorable.v).
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.1.
//
// Property: every equivocation classified as Ignorable produces
// an EquivocationRecord post-fix #1. This is the closure of the
// pre-fix DOS vector — pre-fix, Ignorable was non-slashable and
// the dispatcher silently dropped the block; post-fix, every
// Ignorable equivocation lands in the tracker.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::Status;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_9_1_ignorable_always_recorded(
        validator_count in 1usize..6,
        equivocator_idx in 0usize..6,
        seq in 1u64..10,
    ) {
        let n = validator_count;
        let v = format!("v{}", equivocator_idx % n);
        let mut harness = SlashingTestHarness::new(n, 100);

        let _b = harness.sign_block(&v, seq);
        let bad = harness.sign_block_distinct(&v, seq);
        let s = harness.dispatch(bad);

        // T-9.1: Ignorable equivocations are recorded post-fix.
        // (They may also be Admissible if a record already exists,
        // but the `not unrecorded` invariant is the same in both.)
        prop_assert!(matches!(s, Status::IgnorableEquivocation | Status::AdmissibleEquivocation),
            "first observation classifies as an equivocation variant");
        prop_assert!(harness.has_record(&v, seq.saturating_sub(1)),
            "T-9.1: dispatcher mints record for every equivocation, regardless of variant");
    }
}
