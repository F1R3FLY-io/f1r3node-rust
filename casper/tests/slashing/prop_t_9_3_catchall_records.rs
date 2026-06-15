// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property-based test for T-9.3 (catch-all dispatcher records every
// slashable variant).
//
// Theorem: T-9.3 (`t_9_3_catchall_mints_record`,
// formal/rocq/slashing/theories/BugFixDispatcher.v).
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.3.
//
// Property: regardless of which slashable status the upstream
// validator assigns, the post-fix dispatcher mints exactly one
// EquivocationRecord at `(sender, seq-1)` and adds the block to
// the invalid index.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::{base_seq_from_seq, Status};

/// Strategy producing the four slashable Status variants the harness
/// exposes (the production catch-all covers 14 InvalidBlock variants
/// — exhaustively testing each is out of scope for this proptest;
/// `uc_28_36_tier_b_variants.rs` covers the per-variant cases).
fn gen_slashable_status() -> impl Strategy<Value = Status> {
    prop_oneof![
        Just(Status::AdmissibleEquivocation),
        Just(Status::IgnorableEquivocation),
        Just(Status::NeglectedEquivocation),
        Just(Status::JustificationRegression),
        Just(Status::SlashableOther),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_9_3_catchall_mints_record_for_every_slashable(
        validator_count in 1usize..6,
        seq in 1u64..20,
        status in gen_slashable_status(),
    ) {
        let n = validator_count;
        let mut harness = SlashingTestHarness::new(n, 100);
        let hash = harness.sign_block("v0", seq);

        let returned = harness.dispatch_with_status(hash, status.clone());
        prop_assert_eq!(returned, status,
            "dispatch_with_status returns the forced classification");

        let base = base_seq_from_seq(seq).expect("generated seq is positive");
        prop_assert!(harness.has_record("v0", base),
            "post-fix #3: every slashable status mints a record");
        prop_assert!(harness.dag.invalid.contains(&hash),
            "block is added to the invalid index");
    }

    #[test]
    fn t_9_3_valid_status_does_not_mint(
        seq in 1u64..20,
    ) {
        let mut harness = SlashingTestHarness::new(2, 100);
        let hash = harness.sign_block("v0", seq);

        let _ = harness.dispatch_with_status(hash, Status::Valid);

        let base = base_seq_from_seq(seq).expect("generated seq is positive");
        prop_assert!(!harness.has_record("v0", base),
            "Valid status produces no record");
        prop_assert!(!harness.dag.invalid.contains(&hash),
            "Valid status leaves block out of the invalid index");
    }
}
