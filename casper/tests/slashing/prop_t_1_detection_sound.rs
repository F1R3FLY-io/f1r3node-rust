// Property-based test for T-1 (detection soundness): no honest
// validator is ever slashed.
//
// Theorem: T-1 (`t_1_detection_sound`,
// formal/rocq/slashing/theories/EquivocationDetector.v).
// Reference: docs/theory/slashing/slashing-specification.md §4
// (Theorem 4.2), design/04-detection-and-pipeline.md §4.4.
//
// Property: a validator that publishes a single block per sequence
// number (no equivocation, no self-regression) never receives an
// EquivocationRecord. The dispatcher's classifier returns
// Status::Valid for honest blocks.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::Status;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_1_honest_validator_never_recorded(
        validator_count in 2usize..8,
        depth in 1u64..10,
    ) {
        let mut harness = SlashingTestHarness::new(validator_count, 100);

        // Every validator publishes one block per seq number — strictly
        // honest behaviour, no equivocations.
        for seq in 0..depth {
            for i in 0..validator_count {
                let v = format!("v{}", i);
                let hash = harness.sign_block(&v, seq);
                let status = harness.dispatch(hash);
                prop_assert_eq!(status, Status::Valid,
                    "honest block must classify Valid");
            }
        }

        // No validator should have any EquivocationRecord.
        for i in 0..validator_count {
            let v = format!("v{}", i);
            for base_seq in 0..depth {
                prop_assert!(!harness.has_record(&v, base_seq),
                    "honest validator {} must not have a record at base_seq={}", v, base_seq);
            }
        }

        // No validator should be in the slashed set.
        for i in 0..validator_count {
            let v = format!("v{}", i);
            prop_assert!(harness.is_active(&v),
                "honest validator {} must remain active", v);
            prop_assert!(harness.bond(&v) > 0,
                "honest validator {} must retain a positive bond", v);
        }
    }
}
