// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property-based test for T-9.7 (canonical seq-number density).
//
// Theorem: T-9.7 (`t_9_7_canonical_finds_visible_descendant_with_gap`,
// formal/rocq/slashing/theories/BugFixSeqNumDensity.v).
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.8.
//
// Property: an equivocation is detected even when the validator
// has skipped sequence numbers (under partition recovery). The
// post-fix detector walks the visible self-justification chain and
// returns the canonical branch root with seq > base, not an exact
// base+1 match and not an arbitrary later block on the same branch.
//
// The harness's `detect` operates on (sender, seq) pairs directly,
// so it does not exercise the production self-chain path — but it does verify
// that detection holds for any seq pair, including those with
// gaps. The full canonical-chain proof is in Rocq.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::{base_seq_from_seq, Status};

fn canonical_visible_child(chain_latest_to_oldest: &[u64], base_seq: u64) -> Option<u64> {
    let mut candidate = None;
    for seq in chain_latest_to_oldest {
        if *seq > base_seq {
            candidate = Some(*seq);
        } else {
            break;
        }
    }
    candidate
}

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
        let base = base_seq_from_seq(later_seq).expect("later_seq is positive");
        prop_assert!(harness.has_record("v0", base),
            "T-9.7: dispatcher records the equivocation at base = later_seq - 1");
    }

    #[test]
    fn t_9_7_canonical_child_is_oldest_visible_above_base(
        base_seq in 0u64..20,
        root_gap in 1u64..20,
        later_gap in 0u64..20,
    ) {
        let root = base_seq + root_gap;
        let latest = root + later_gap;
        let chain = if latest == root {
            vec![root, base_seq]
        } else {
            vec![latest, root, base_seq]
        };

        prop_assert_eq!(canonical_visible_child(&chain, base_seq), Some(root));
    }

    #[test]
    fn t_9_7_same_branch_latest_messages_do_not_overcount(
        base_seq in 0u64..20,
        root_gap in 1u64..20,
        later_gap in 1u64..20,
    ) {
        let root = base_seq + root_gap;
        let latest = root + later_gap;
        let root_view = vec![root, base_seq];
        let latest_view = vec![latest, root, base_seq];

        prop_assert_eq!(
            canonical_visible_child(&latest_view, base_seq),
            canonical_visible_child(&root_view, base_seq)
        );
        prop_assert_eq!(canonical_visible_child(&latest_view, base_seq), Some(root));
    }
}
