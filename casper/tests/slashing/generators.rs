// Proptest strategies for the SlashingTestHarness.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.2.2.
//
// Each generator is shrinking-aware: failure cases shrink to minimal
// counter-examples (smallest validator count, smallest seq numbers,
// fewest blocks) so test failures are easy to interpret.

#![allow(dead_code)]

use std::collections::HashMap;

use proptest::collection::{hash_map, vec};
use proptest::prelude::*;

use super::types::{BlockHash, BlockMeta, DagState, PoSState, SeqNum, ValidatorId};

/// Validator identities are short labels "v0", "v1", ..., "v{n-1}".
pub fn gen_validator_id(max_idx: usize) -> impl Strategy<Value = ValidatorId> {
    (0..max_idx).prop_map(|i| format!("v{}", i))
}

/// Block hashes are 64-bit ints in `[1, max]`. `1` is reserved as the
/// first allocated hash, so generators that interleave with the
/// harness's auto-allocation should pass `max = u64::MAX`.
pub fn gen_block_hash() -> impl Strategy<Value = BlockHash> {
    1u64..=u64::MAX
}

/// Sequence numbers in `[0, max]`.
pub fn gen_seq_num(max: SeqNum) -> impl Strategy<Value = SeqNum> {
    0u64..=max
}

/// Bonds map: every validator gets a positive stake in
/// `[1, max_stake]`. Mirrors the post-fix #5 invariant
/// `active_implies_bonded`.
pub fn gen_bonds_map(
    validator_count: usize,
    max_stake: i64,
) -> impl Strategy<Value = HashMap<ValidatorId, i64>> {
    let v = validator_count;
    let s = max_stake;
    Just(()).prop_flat_map(move |_| {
        let validators = (0..v).map(|i| format!("v{}", i)).collect::<Vec<_>>();
        vec(1i64..=s, v).prop_map(move |stakes| {
            validators
                .iter()
                .cloned()
                .zip(stakes.into_iter())
                .collect::<HashMap<_, _>>()
        })
    })
}

/// PoS state with random bonds and matching active set (every bonded
/// validator is active). Coop vault starts at 0, slashed set empty.
pub fn gen_pos_state(
    validator_count: usize,
    max_stake: i64,
) -> impl Strategy<Value = PoSState> {
    gen_bonds_map(validator_count, max_stake).prop_map(|bonds| {
        let active = bonds.keys().cloned().collect();
        PoSState {
            bonds,
            active,
            slashed: Default::default(),
            coop_vault: 0,
        }
    })
}

/// A simple DAG of `validator_count` validators, each producing one
/// block per sequence number from `0..depth`. Each block's only
/// justification is its own creator-justification (the validator's
/// previous block).
pub fn gen_linear_dag(
    validator_count: usize,
    depth: SeqNum,
) -> impl Strategy<Value = DagState> {
    Just((validator_count, depth)).prop_map(|(v, d)| {
        let mut dag = DagState::default();
        let mut prev: HashMap<ValidatorId, BlockHash> = HashMap::new();
        let mut next_hash: BlockHash = 1;
        for seq in 0..d {
            for i in 0..v {
                let sender = format!("v{}", i);
                let hash = next_hash;
                next_hash += 1;
                let justifications = match prev.get(&sender) {
                    Some(&prev_hash) => vec![(sender.clone(), prev_hash)],
                    None => vec![],
                };
                dag.blocks.insert(
                    hash,
                    BlockMeta {
                        hash,
                        sender: sender.clone(),
                        seq,
                        justifications,
                        slash_targets: vec![],
                    },
                );
                prev.insert(sender, hash);
            }
        }
        dag
    })
}

/// Equivocation injection: pick a random `(validator, seq)` and
/// produce a *second* block by that validator at that seq with a
/// distinct hash. Returns the (validator, original_hash,
/// equivocating_hash) triple.
pub fn gen_equivocation(
    dag: DagState,
) -> impl Strategy<Value = (ValidatorId, BlockHash, BlockHash)> {
    let candidates: Vec<(ValidatorId, BlockHash)> = dag
        .blocks
        .values()
        .map(|b| (b.sender.clone(), b.hash))
        .collect();
    proptest::sample::select(candidates).prop_map(move |(sender, original)| {
        let max_existing = dag.blocks.keys().copied().max().unwrap_or(0);
        let equivocating = max_existing.wrapping_add(1).wrapping_add(original);
        (sender, original, equivocating)
    })
}

/// Composite generator producing a 4-tuple
/// `(validator_count, stake, dag, pos_state)` matching at the
/// validator-set level. Useful for whole-state property tests.
pub fn gen_5_component_state(
    max_validator_count: usize,
    max_stake: i64,
    max_depth: SeqNum,
) -> impl Strategy<Value = (usize, i64, DagState, PoSState)> {
    (1usize..=max_validator_count, 1i64..=max_stake, 1u64..=max_depth).prop_flat_map(
        move |(v, s, d)| {
            (gen_linear_dag(v, d), gen_pos_state(v, s))
                .prop_map(move |(dag, pos)| (v, s, dag, pos))
        },
    )
}

#[cfg(test)]
mod generators_smoke {
    use super::*;
    use proptest::strategy::ValueTree;

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 32,
            .. ProptestConfig::default()
        })]

        #[test]
        fn bonds_map_has_n_validators(
            n in 1usize..16,
            stake in 1i64..1000,
        ) {
            let strategy = gen_bonds_map(n, stake);
            let mut runner = proptest::test_runner::TestRunner::default();
            let bonds = strategy.new_tree(&mut runner).unwrap().current();
            prop_assert_eq!(bonds.len(), n);
            for (_, &b) in &bonds {
                prop_assert!(b >= 1 && b <= stake);
            }
        }

        #[test]
        fn linear_dag_has_n_times_d_blocks(
            n in 1usize..8,
            d in 1u64..8,
        ) {
            let strategy = gen_linear_dag(n, d);
            let mut runner = proptest::test_runner::TestRunner::default();
            let dag = strategy.new_tree(&mut runner).unwrap().current();
            prop_assert_eq!(dag.blocks.len(), (n as u64 * d) as usize);
        }
    }
}
