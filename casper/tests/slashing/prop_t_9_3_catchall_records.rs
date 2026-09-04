// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property-based test for T-9.3 certified-rejection persistence and exact
// economic-evidence eligibility.
//
// Theorem: T-9.3 (`t_9_3_catchall_mints_record`,
// formal/rocq/slashing/theories/BugFixDispatcher.v).
// Reference: docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.3.
//
// Property: every certified rejection enters the invalid index. Only the two
// objective-equivocation statuses mint one EquivocationRecord.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::{base_seq_from_seq, Status};

/// Strategy that covers each rejection class in the harness projection.
fn gen_rejected_status() -> impl Strategy<Value = Status> {
    prop_oneof![
        Just(Status::AdmissibleEquivocation),
        Just(Status::IgnorableEquivocation),
        Just(Status::NeglectedEquivocation),
        Just(Status::JustificationRegression),
        Just(Status::RejectedOther),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_9_3_certified_rejection_persists_and_only_equivocation_mints_evidence(
        validator_count in 1usize..6,
        seq in 1u64..20,
        status in gen_rejected_status(),
    ) {
        let n = validator_count;
        let mut harness = SlashingTestHarness::new(n, 100);
        let hash = harness.sign_block("v0", seq);

        let returned = harness.dispatch_with_status(hash, status);
        prop_assert_eq!(returned, status,
            "dispatch_with_status returns the forced classification");

        let base = base_seq_from_seq(seq).expect("generated seq is positive");
        prop_assert_eq!(
            harness.has_record("v0", base),
            status.is_slash_evidence_eligible()
        );
        prop_assert!(harness.dag.invalid.contains(&hash),
            "certified rejection is added to the invalid index");
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
