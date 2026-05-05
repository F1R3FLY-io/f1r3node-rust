// Property-based test for T-7 (slash zeros the offender's bond) +
// T-8 (the forfeited stake reaches the Coop vault).
//
// Theorems: T-7 (`t_7_slash_zeroes_bond`,
// formal/rocq/slashing/theories/PoSContract.v) and T-8
// (`t_8_forfeited_to_coop_vault`, same file).
// Reference: docs/theory/slashing/slashing-specification.md §5.2,
// design/06-proposing-and-effect.md §6.3.
//
// Properties:
//   ∀ ps v, slash(ps, v).0.bonds_map[v] = 0                          [T-7]
//   ∀ ps v, slash(ps, v).0.coop_vault =
//         ps.coop_vault + ps.bonds_map[v]                            [T-8]
//
// Combined: the slash transition transfers exactly the offender's
// stake from the bonds map into the Coop vault, and removes them
// from the active set.

use proptest::prelude::*;

use super::generators::gen_pos_state;
use super::harness::SlashingTestHarness;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_7_t_8_slash_transfers_stake_to_coop_vault(
        validator_count in 1usize..16,
        max_stake in 1i64..1_000_000,
        target_idx in 0usize..16,
        pos_state in gen_pos_state(16, 1_000_000),
    ) {
        let n = validator_count;
        let target = format!("v{}", target_idx % n);

        // Build a harness then overwrite its pos_state with the
        // generated one (truncated to `n` validators).
        let mut harness = SlashingTestHarness::new(n, max_stake);
        let mut bonds = std::collections::HashMap::new();
        let mut active = std::collections::HashSet::new();
        for i in 0..n {
            let v = format!("v{}", i);
            if let Some(&b) = pos_state.bonds.get(&v) {
                bonds.insert(v.clone(), b);
                active.insert(v);
            }
        }
        harness.pos_state.bonds = bonds.clone();
        harness.pos_state.active = active;
        harness.pos_state.coop_vault = pos_state.coop_vault;

        let initial_bond = harness.bond(&target);
        let initial_coop = harness.coop_vault();

        let result = harness.execute_slash(&target);
        prop_assert!(result.success);

        // T-7: bond zeroed.
        prop_assert_eq!(harness.bond(&target), 0);

        // T-8: coop vault gained exactly the offender's prior bond.
        prop_assert_eq!(harness.coop_vault(), initial_coop + initial_bond);

        // Active set: target removed.
        prop_assert!(!harness.is_active(&target));

        // Other validators are untouched.
        for i in 0..n {
            let v = format!("v{}", i);
            if v != target && bonds.contains_key(&v) {
                prop_assert_eq!(harness.bond(&v), bonds[&v]);
            }
        }
    }
}
