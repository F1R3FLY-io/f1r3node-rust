// Fork-choice heaviest-subtree DESCENT at depth 2 — the geometry the rest of
// this directory never builds. Every other fork fixture here is depth-1, where
// a tip's own score IS its subtree weight, so ranking tips by score and
// descending the heaviest subtree coincide. At depth 2 they separate: a tip's
// score is only its owner's weight, while its BRANCH carries every supporter
// below it. GHOST chooses branches, so the head must come from the branch with
// the majority of latest-message weight — regardless of how the tip hashes
// happen to sort.
//
// Pinned to CI instance ucc-i6 (run 32404488936, shard 58868952): at the h293
// fork the 725f branch held 200 of 300 latest-message weight and the rival
// 6c4f/45de branch 100, yet the spine head went to the rival because all three
// tips scored 100 and 45de hash-sorted first. A 200-weight finality
// certificate on the abandoned branch was reverted with zero equivocations,
// and two nodes wedged permanently on the erased content.

use std::collections::HashMap;

use casper::rust::estimator::Estimator;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::validator::Validator;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{certified_fork_choice, create_block, create_genesis_block};
use crate::helper::block_util::generate_validator;

lazy_static::lazy_static! {
    // Shared Tokio runtime for the `#[test]` proptests (they cannot be
    // `#[tokio::test]`); mirrors prop_ghost_argmax.rs.
    static ref RUNTIME: tokio::runtime::Runtime =
        tokio::runtime::Runtime::new().expect("tokio runtime");
}

#[allow(clippy::too_many_arguments)]
fn make_block(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
    parents: Vec<BlockHash>,
    genesis: &BlockMessage,
    creator: &Validator,
    bonds: &[Bond],
    justifications: HashMap<Validator, BlockHash>,
) -> BlockMessage {
    create_block(
        block_store,
        block_dag_storage,
        parents,
        genesis,
        Some(creator.clone()),
        Some(bonds.to_vec()),
        Some(justifications),
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

/// The ucc-i6 sub-DAG, reduced to its fork-choice core:
///
/// ```text
///          genesis
///             |
///             p                 common parent (01778b1dd4)
///           /   \
///     a(v1)      b(v2)          same-height siblings (725f / 6c4f)
///      /  \        \
///  a1(v1) a2(v0)   b1(v2)       b1 merges a as a secondary parent (45de)
/// ```
///
/// Latest messages `{v0: a2, v1: a1, v2: b1}`, equal stakes. Branch weight at
/// the p-fork: a = 200, b = 100. Each TIP scores only its owner's 100.
struct Depth2Fork {
    heavy_tips: Vec<BlockHash>,
    light_tip: BlockHash,
    genesis: BlockMessage,
    latest: HashMap<Validator, BlockHash>,
}

async fn build_ucc_i6_fork(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
) -> Depth2Fork {
    let validators: Vec<Validator> = (0..3)
        .map(|i| generate_validator(Some(&format!("Descent Validator {i}"))))
        .collect();
    let bonds: Vec<Bond> = validators
        .iter()
        .map(|v| Bond {
            validator: v.clone(),
            stake: 100,
        })
        .collect();

    let genesis = create_genesis_block(
        block_store,
        block_dag_storage,
        None,
        Some(bonds.clone()),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let justifications: HashMap<Validator, BlockHash> = validators
        .iter()
        .map(|v| (v.clone(), genesis.block_hash.clone()))
        .collect();

    let p = make_block(
        block_store,
        block_dag_storage,
        vec![genesis.block_hash.clone()],
        &genesis,
        &validators[0],
        &bonds,
        justifications.clone(),
    );
    let a = make_block(
        block_store,
        block_dag_storage,
        vec![p.block_hash.clone()],
        &genesis,
        &validators[1],
        &bonds,
        justifications.clone(),
    );
    let b = make_block(
        block_store,
        block_dag_storage,
        vec![p.block_hash.clone()],
        &genesis,
        &validators[2],
        &bonds,
        justifications.clone(),
    );
    let a1 = make_block(
        block_store,
        block_dag_storage,
        vec![a.block_hash.clone()],
        &genesis,
        &validators[1],
        &bonds,
        justifications.clone(),
    );
    let a2 = make_block(
        block_store,
        block_dag_storage,
        vec![a.block_hash.clone()],
        &genesis,
        &validators[0],
        &bonds,
        justifications.clone(),
    );
    let b1 = make_block(
        block_store,
        block_dag_storage,
        vec![b.block_hash.clone(), a.block_hash.clone()],
        &genesis,
        &validators[2],
        &bonds,
        justifications.clone(),
    );

    let latest: HashMap<Validator, BlockHash> = vec![
        (validators[0].clone(), a2.block_hash.clone()),
        (validators[1].clone(), a1.block_hash.clone()),
        (validators[2].clone(), b1.block_hash.clone()),
    ]
    .into_iter()
    .collect();

    Depth2Fork {
        heavy_tips: vec![a1.block_hash, a2.block_hash],
        light_tip: b1.block_hash,
        genesis,
        latest,
    }
}

/// RED against the flat tip re-sort, GREEN under heaviest-subtree descent.
///
/// Block hashes are not directly controllable, and the current defect only
/// shows when the light-branch tip hash-sorts before both heavy-branch tips
/// (p = 1/3 per build), so the fixture is rebuilt in fresh storage until the
/// adversarial ordering holds — a bounded loop; (2/3)^64 is negligible. The
/// assertion itself is ordering-free: the head must come from the 200-weight
/// branch no matter how the hashes sorted.
#[tokio::test]
async fn the_head_must_not_leave_a_majority_branch_for_a_hash_earlier_rival() {
    for _ in 0..64 {
        let outcome = with_storage(|mut block_store, mut block_dag_storage| async move {
            let fork = build_ucc_i6_fork(&mut block_store, &mut block_dag_storage).await;
            let adversarial_ordering = fork.heavy_tips.iter().all(|h| fork.light_tip < *h);
            if !adversarial_ordering {
                return None;
            }
            let dag = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let estimator = Estimator::apply(i32::MAX, None);
            let head = certified_fork_choice(&estimator, &dag, &fork.genesis, fork.latest.clone())
                .await
                .expect("tips")
                .tips
                .into_iter()
                .next()
                .expect("non-empty tips");
            Some((head, fork.heavy_tips.clone(), fork.light_tip.clone()))
        })
        .await;

        if let Some((head, heavy_tips, light_tip)) = outcome {
            assert!(
                heavy_tips.contains(&head),
                "the GHOST head left the 200-of-300 branch for the hash-earlier \
                 100-weight rival: head={}, light_tip={}, heavy_tips={:?}",
                hex::encode(&head),
                hex::encode(&light_tip),
                heavy_tips.iter().map(hex::encode).collect::<Vec<_>>(),
            );
            return;
        }
    }
    panic!("fixture construction never produced the adversarial hash ordering in 64 attempts");
}

/// A generated depth-2 weighted fork: `n` validators with random stakes split
/// across two same-height siblings, every validator holding its OWN depth-2
/// tip above its sibling. `assign[i]` is validator `i`'s branch; both branches
/// carry at least one validator, and the branch weights are forced unequal so
/// the heaviest branch is unambiguous.
#[derive(Debug, Clone)]
struct Depth2Shape {
    stakes: Vec<i64>,
    assign: Vec<usize>,
}

prop_compose! {
    fn depth2_shape()(
        stakes in prop::collection::vec(1i64..=50, 2..=4),
        raw_assign in prop::collection::vec(0usize..2, 4),
    ) -> Depth2Shape {
        let n = stakes.len();
        let mut assign = vec![0usize; n];
        for (i, slot) in assign.iter_mut().enumerate() {
            *slot = if i < 2 { i } else { raw_assign[i] };
        }
        Depth2Shape { stakes, assign }
    }
}

fn branch_weights(shape: &Depth2Shape) -> [i64; 2] {
    let mut weights = [0i64; 2];
    for (i, &branch) in shape.assign.iter().enumerate() {
        weights[branch] += shape.stakes[i];
    }
    weights
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, max_shrink_iters: 8, ..ProptestConfig::default() })]

    // The depth-2 generalization of prop_ghost_argmax's T-GHOST: the head must
    // land in the branch holding the strictly greater latest-message weight.
    // Tips score only their owners here, so any implementation that ranks tips
    // by score instead of descending subtrees degenerates to hash order and
    // fails whenever a light-branch tip hash-sorts first.
    #[test]
    fn ghost_head_lands_in_the_heaviest_subtree_branch(shape in depth2_shape()) {
        let weights = branch_weights(&shape);
        prop_assume!(weights[0] != weights[1]);
        let heaviest = if weights[0] > weights[1] { 0 } else { 1 };

        RUNTIME.block_on(with_storage(|mut block_store, mut block_dag_storage| async move {
            let n = shape.stakes.len();
            let validators: Vec<Validator> = (0..n)
                .map(|i| generate_validator(Some(&format!("Depth2 Validator {i}"))))
                .collect();
            let bonds: Vec<Bond> = validators
                .iter()
                .zip(shape.stakes.iter())
                .map(|(v, stake)| Bond { validator: v.clone(), stake: *stake })
                .collect();
            let genesis = create_genesis_block(
                &mut block_store,
                &mut block_dag_storage,
                None,
                Some(bonds.clone()),
                None,
                None,
                None,
                None,
                None,
                None,
            );
            let justifications: HashMap<Validator, BlockHash> = validators
                .iter()
                .map(|v| (v.clone(), genesis.block_hash.clone()))
                .collect();

            let siblings: Vec<BlockMessage> = (0..2)
                .map(|j| {
                    let creator_idx = shape
                        .assign
                        .iter()
                        .position(|&a| a == j)
                        .expect("both branches are seeded by construction");
                    make_block(
                        &mut block_store,
                        &mut block_dag_storage,
                        vec![genesis.block_hash.clone()],
                        &genesis,
                        &validators[creator_idx],
                        &bonds,
                        justifications.clone(),
                    )
                })
                .collect();

            let mut latest: HashMap<Validator, BlockHash> = HashMap::new();
            let mut tip_branch: HashMap<BlockHash, usize> = HashMap::new();
            for (i, validator) in validators.iter().enumerate() {
                let branch = shape.assign[i];
                let tip = make_block(
                    &mut block_store,
                    &mut block_dag_storage,
                    vec![siblings[branch].block_hash.clone()],
                    &genesis,
                    validator,
                    &bonds,
                    justifications.clone(),
                );
                tip_branch.insert(tip.block_hash.clone(), branch);
                latest.insert(validator.clone(), tip.block_hash);
            }

            let dag = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let estimator = Estimator::apply(i32::MAX, None);
            let head = certified_fork_choice(&estimator, &dag, &genesis, latest)
                .await
                .expect("tips")
                .tips
                .into_iter()
                .next()
                .expect("non-empty tips");

            let head_branch = tip_branch
                .get(&head)
                .copied()
                .expect("the head must be one of the latest-message tips");
            prop_assert_eq!(
                head_branch, heaviest,
                "head landed in branch {} (weight {}) while branch {} holds {}",
                head_branch, weights[head_branch], heaviest, weights[heaviest]
            );
            Ok::<(), TestCaseError>(())
        }))?;
    }
}
