// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property-based test for T-12 (BFT quorum preservation under bound).
//
// Theorem: T-12 (`bft_quorum_preservation`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: docs/theory/slashing/slashing-specification.md §8.4,
// design/08-two-level-and-collusion.md, citation [LSP82].
//
// Property: under the BFT precondition `|closure| ≤ F = ⌊(n-1)/3⌋`,
// the slash closure preserves quorum — the post-slash active set
// has at least `n - F = ⌈2n/3⌉` validators.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_12_quorum_preserved_when_closure_within_bound(
        n in 4usize..16,
    ) {
        let f = (n.saturating_sub(1)) / 3; // F = ⌊(n-1)/3⌋
        // Slash exactly F validators (the maximum allowed under BFT).
        let mut harness = SlashingTestHarness::new(n, 100);
        for i in 0..f {
            let v = format!("v{}", i);
            let r = harness.execute_slash(&v);
            prop_assert!(r.success);
        }

        let active_count = (0..n)
            .filter(|i| harness.is_active(&format!("v{}", i)))
            .count();
        // T-12: active size ≥ n - F.
        prop_assert!(
            active_count >= n.saturating_sub(f),
            "n={}, F={}, expected active ≥ {}, got {}",
            n, f, n.saturating_sub(f), active_count
        );
        // For n ≥ 4, F ≤ ⌊(n-1)/3⌋ so n - F ≥ ⌈2n/3⌉.
        prop_assert!(active_count >= (2 * n).div_ceil(3));
    }
}
