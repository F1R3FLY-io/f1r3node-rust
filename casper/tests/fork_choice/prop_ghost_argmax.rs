// Fork-choice GHOST heaviest-subtree ARGMAX (T-GHOST) + MAIN-PARENT selection
// (T-MP) — the real `Estimator` driven on GENERATED weighted multi-validator fork
// DAGs, with an INDEPENDENT heaviest-subtree oracle.
//
// These close two fork-choice FV rows that previously had only a pinned example or
// a Rocq/Wolfram modality (`prop_estimator_determinism.rs`'s
// `fork_choice_determinism_correct` asserts the ONE fixed tip set `[b8, b7]`):
//
//   * T-GHOST — `formal/rocq/fork_choice/theories/Rank.v`'s `rank_head_is_argmax`
//     ("the chosen child has the MAXIMUM score") and `rank_selects_heaviest` (the
//     `(score DESC, hash ASC)` argmax over the scored children = GHOST's heaviest
//     subtree). We build a RANDOM two-level weighted fork: every validator's latest
//     message is authored by that validator beneath its assigned branch. We
//     independently sum support at the branch level, descend through the winning
//     subtree, and assert that the real certified-context estimator returns the
//     greedy GHOST head first while retaining all other scored terminal leaves in
//     `(score DESC, hash ASC)` order.
//
//   * T-MP (STAGE 1) — `formal/rocq/fork_choice/theories/GuardBridge.v`'s
//     `ghost_sort_first_deterministic` (snapshot.rs:317-331): the GHOST head is
//     `tips.into_iter().next()` (the head of the ranked tips, :317-323), and the
//     proposer sorts the parent list so the ghost head comes first, ties by hash
//     (:325-331). `main_parent_is_ghost_head_deterministic` pins that on a fixed
//     distinct-stake fork: the head is the heaviest branch, stable across every input
//     ordering of the latest-message map, and the snapshot.rs parent-ordering
//     comparator places it first.
//
//     SCOPE CAVEAT — this covers STAGE 1 ONLY. The GHOST head is NOT necessarily the
//     block's MAIN PARENT: snapshot.rs:332 then runs `prefer_deploy_support_main_parent`
//     (:124-185), which can PROMOTE a deploy-carrying branch to index 0 and override it
//     (GuardBridge.v `pipeline_head_may_differ_from_ghost` refutes the old
//     "main parent = ghost head" bridge by computation). Stage 2 is a private fn, so its
//     proptests live in-module in snapshot.rs's `mod tests` (`deploy_support_*`). The
//     ESTIMATOR results asserted here are unaffected — see
//     docs/theory/fork-choice/fork-choice-verification.md §6.2.
//
// LOCAL-ONLY verification (not consensus code). Run under `cargo test -p casper` and
// gated by scripts/check-fork-choice-ALL.sh via the `fork_choice::` filter.

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
    // `#[tokio::test]`); mirrors casper/tests/fork_choice/prop_estimator_determinism.rs.
    static ref RUNTIME: tokio::runtime::Runtime =
        tokio::runtime::Runtime::new().expect("tokio runtime");
}

#[allow(clippy::too_many_arguments)]
fn make_block(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
    genesis: &BlockMessage,
    parent: &BlockHash,
    creator: &Validator,
    bonds: &[Bond],
    justifications: HashMap<Validator, BlockHash>,
) -> BlockMessage {
    make_block_with_parents(
        block_store,
        block_dag_storage,
        genesis,
        vec![parent.clone()],
        creator,
        bonds,
        justifications,
    )
}

#[allow(clippy::too_many_arguments)]
fn make_block_with_parents(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
    genesis: &BlockMessage,
    parents: Vec<BlockHash>,
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

/// A generated two-level weighted fork: `n` validators with random stakes are
/// partitioned onto `n_branches` distinct children of genesis and then author their
/// own latest messages below those branches.
#[derive(Debug, Clone)]
struct ForkShape {
    stakes: Vec<i64>,
    n_branches: usize,
    assign: Vec<usize>,
}

prop_compose! {
    fn fork_shape()(
        stakes in prop::collection::vec(1i64..=50, 2..=4),
        raw_assign in prop::collection::vec(0usize..4, 4),
        branch_seed in 0usize..3,
    ) -> ForkShape {
        let n = stakes.len();               // 2..=4
        let n_branches = 2 + branch_seed % (n - 1);   // 2..=n
        let mut assign = vec![0usize; n];
        for (i, slot) in assign.iter_mut().enumerate() {
            // First `n_branches` validators seed distinct branches (surjectivity ⇒
            // every branch has ≥ 1 supporter, so every branch is scored); the rest
            // fall to an arbitrary existing branch.
            *slot = if i < n_branches { i } else { raw_assign[i] % n_branches };
        }
        ForkShape { stakes, n_branches, assign }
    }
}

/// Build the fork in the DAG and return genesis, branch roots, terminal supporter
/// blocks, and the author-bound latest-message map.
async fn build_fork(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
    shape: &ForkShape,
) -> (
    BlockMessage,
    Vec<BlockMessage>,
    Vec<BlockMessage>,
    HashMap<Validator, BlockHash>,
) {
    let n = shape.stakes.len();
    let validators: Vec<Validator> = (0..n)
        .map(|i| {
            let name = format!("Ghost Validator {i}");
            generate_validator(Some(&name))
        })
        .collect();
    let bonds: Vec<Bond> = validators
        .iter()
        .zip(shape.stakes.iter())
        .map(|(v, stake)| Bond {
            validator: v.clone(),
            stake: *stake,
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

    // Every branch block justifies all validators at genesis (a well-formed initial
    // fork); the estimator scores via the explicit latest-message map, not these.
    let justifications: HashMap<Validator, BlockHash> = validators
        .iter()
        .map(|v| (v.clone(), genesis.block_hash.clone()))
        .collect();

    let mut branch_blocks: Vec<BlockMessage> = Vec::with_capacity(shape.n_branches);
    for j in 0..shape.n_branches {
        let creator_idx = shape
            .assign
            .iter()
            .position(|&a| a == j)
            .expect("every branch has ≥ 1 supporter by construction");
        let block = make_block(
            block_store,
            block_dag_storage,
            &genesis,
            &genesis.block_hash,
            &validators[creator_idx],
            &bonds,
            justifications.clone(),
        );
        branch_blocks.push(block);
    }

    let mut supporter_blocks = Vec::with_capacity(validators.len());
    for (index, validator) in validators.iter().enumerate() {
        let parent = &branch_blocks[shape.assign[index]].block_hash;
        supporter_blocks.push(make_block(
            block_store,
            block_dag_storage,
            &genesis,
            parent,
            validator,
            &bonds,
            justifications.clone(),
        ));
    }
    let latest = validators
        .into_iter()
        .zip(
            supporter_blocks
                .iter()
                .map(|block| block.block_hash.clone()),
        )
        .collect();

    (genesis, branch_blocks, supporter_blocks, latest)
}

/// The independent two-lane oracle: select the GHOST head by greedy subtree descent,
/// then order every other terminal leaf by `(score DESC, hash ASC)`.
fn expected_ranked_tips(
    shape: &ForkShape,
    branch_blocks: &[BlockMessage],
    supporter_blocks: &[BlockMessage],
) -> Vec<BlockHash> {
    let mut scores = vec![0i64; shape.n_branches];
    for (i, &branch) in shape.assign.iter().enumerate() {
        scores[branch] += shape.stakes[i];
    }
    let mut ranked_branches: Vec<(i64, BlockHash, usize)> = (0..shape.n_branches)
        .map(|j| (scores[j], branch_blocks[j].block_hash.clone()))
        .enumerate()
        .map(|(index, (score, hash))| (score, hash, index))
        .collect();
    ranked_branches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let winning_branch = ranked_branches[0].2;
    let mut winning_children = shape
        .assign
        .iter()
        .enumerate()
        .filter(|(_, branch)| **branch == winning_branch)
        .map(|(index, _)| {
            (
                shape.stakes[index],
                supporter_blocks[index].block_hash.clone(),
                index,
            )
        })
        .collect::<Vec<_>>();
    winning_children.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let head_index = winning_children[0].2;
    let mut tail = supporter_blocks
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != head_index)
        .map(|(index, block)| (shape.stakes[index], block.block_hash.clone()))
        .collect::<Vec<_>>();
    tail.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    std::iter::once(supporter_blocks[head_index].block_hash.clone())
        .chain(tail.into_iter().map(|(_, hash)| hash))
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 16, max_shrink_iters: 8, ..ProptestConfig::default() })]

    // T-GHOST: the head follows greedy heaviest-subtree descent, while the tail is the
    // exact terminal frontier ordered by score and hash.
    #[test]
    fn ghost_ranked_tips_are_heaviest_subtree_argmax(shape in fork_shape()) {
        RUNTIME.block_on(with_storage(|mut block_store, mut block_dag_storage| async move {
            let (genesis, branch_blocks, supporter_blocks, latest) =
                build_fork(&mut block_store, &mut block_dag_storage, &shape).await;
            let dag = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let estimator = Estimator::apply(i32::MAX, None);

            let tips = certified_fork_choice(&estimator, &dag, &genesis, latest)
                .await
                .expect("tips")
                .tips;

            let expected = expected_ranked_tips(&shape, &branch_blocks, &supporter_blocks);

            prop_assert_eq!(
                &tips, &expected,
                "estimator tips must equal GHOST head followed by the ranked terminal frontier"
            );
            prop_assert_eq!(
                tips.first(), expected.first(),
                "estimator head must be the result of greedy heaviest-subtree descent"
            );
            Ok::<(), TestCaseError>(())
        }))?;
    }
}

/// T-MP STAGE 1 (GuardBridge.v `ghost_sort_first_deterministic`, snapshot.rs:317-331):
/// the GHOST head is the head of the ranked tips (`tips.into_iter().next()`, :317-323),
/// it is the heaviest branch, it is STABLE across every input ordering of the
/// latest-message map (S1 determinism at the main-parent granularity), and the
/// snapshot.rs parent-ordering comparator (`is_main DESC, then hash ASC`, :325-331)
/// sorts it first. A fixed distinct-stake fork (30 / 20 / 10) makes the heaviest branch
/// unambiguous.
///
/// STAGE 2 (`prefer_deploy_support_main_parent`, :332 -> :124-185) can still PROMOTE a
/// deploy-carrying branch over this ghost head, so the value asserted here is the GHOST
/// HEAD, not necessarily the block's final main parent. Stage 2 is covered by the
/// `deploy_support_*` proptests in snapshot.rs's in-module `mod tests`.
#[tokio::test]
async fn main_parent_is_ghost_head_deterministic() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let shape = ForkShape {
            stakes: vec![30, 20, 10],
            n_branches: 3,
            assign: vec![0, 1, 2],
        };
        let (genesis, _branch_blocks, supporter_blocks, latest) =
            build_fork(&mut block_store, &mut block_dag_storage, &shape).await;
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let estimator = Estimator::apply(i32::MAX, None);

        // The heaviest branch is validator 0's (stake 30) — the expected main parent.
        let expected_main = supporter_blocks[0].block_hash.clone();

        // Deterministic across every ordering of the latest-message map: the ghost main
        // parent (tips[0]) is invariant to HashMap iteration order.
        let entries: Vec<(Validator, BlockHash)> = latest.into_iter().collect();
        let orders: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
            2, 1, 0,
        ]];
        for order in orders {
            let permuted: HashMap<Validator, BlockHash> =
                order.iter().map(|&i| entries[i].clone()).collect();
            let ghost_main_parent = certified_fork_choice(&estimator, &dag, &genesis, permuted)
                .await
                .expect("tips")
                .tips
                .into_iter()
                .next();
            assert_eq!(
                ghost_main_parent.as_ref(),
                Some(&expected_main),
                "ghost main parent (tips[0]) must be the heaviest branch, stable across map order"
            );
        }

        // snapshot.rs:325-331 STAGE-1 parent ordering: place the ghost head first, then
        // by hash — asserted on a deliberately shuffled parent list. (Stage 2, :332 ->
        // :124-185, can still promote a deploy-carrying branch over this head; see the
        // `deploy_support_*` proptests in snapshot.rs's in-module `mod tests`.)
        let ghost_main_parent = Some(expected_main.clone());
        let mut parents: Vec<BlockMessage> = vec![
            supporter_blocks[2].clone(),
            supporter_blocks[0].clone(),
            supporter_blocks[1].clone(),
        ];
        parents.sort_by(|a, b| {
            let a_main = ghost_main_parent.as_ref() == Some(&a.block_hash);
            let b_main = ghost_main_parent.as_ref() == Some(&b.block_hash);
            b_main
                .cmp(&a_main)
                .then_with(|| a.block_hash.cmp(&b.block_hash))
        });
        assert_eq!(
            parents.first().map(|b| b.block_hash.clone()),
            Some(expected_main),
            "the snapshot.rs parent ordering must sort the ghost main parent first"
        );
    })
    .await
}

#[tokio::test]
async fn aggregate_subtree_weight_beats_larger_terminal_leaf() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let shape = ForkShape {
            stakes: vec![30, 30, 40],
            n_branches: 2,
            assign: vec![0, 0, 1],
        };
        let (genesis, branch_blocks, supporter_blocks, latest) =
            build_fork(&mut block_store, &mut block_dag_storage, &shape).await;
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let tips = certified_fork_choice(&Estimator::apply(i32::MAX, None), &dag, &genesis, latest)
            .await
            .expect("tips")
            .tips;
        let expected = expected_ranked_tips(&shape, &branch_blocks, &supporter_blocks);

        assert_eq!(tips, expected);
        assert_ne!(tips[0], supporter_blocks[2].block_hash);
        assert!(
            tips[0] == supporter_blocks[0].block_hash || tips[0] == supporter_blocks[1].block_hash
        );
    })
    .await
}

#[tokio::test]
async fn multi_parent_diamond_has_one_shared_terminal_leaf() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let validators = [
            generate_validator(Some("Diamond Validator 0")),
            generate_validator(Some("Diamond Validator 1")),
        ];
        let bonds = vec![
            Bond {
                validator: validators[0].clone(),
                stake: 30,
            },
            Bond {
                validator: validators[1].clone(),
                stake: 20,
            },
        ];
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
        let justifications = validators
            .iter()
            .map(|validator| (validator.clone(), genesis.block_hash.clone()))
            .collect::<HashMap<_, _>>();
        let left = make_block(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            &genesis.block_hash,
            &validators[0],
            &bonds,
            justifications.clone(),
        );
        let right = make_block(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            &genesis.block_hash,
            &validators[1],
            &bonds,
            justifications.clone(),
        );
        let shared = make_block_with_parents(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            vec![left.block_hash.clone(), right.block_hash.clone()],
            &validators[0],
            &bonds,
            justifications.clone(),
        );
        let independent = make_block(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            &genesis.block_hash,
            &validators[1],
            &bonds,
            justifications,
        );
        let latest = HashMap::from([
            (validators[0].clone(), shared.block_hash.clone()),
            (validators[1].clone(), independent.block_hash.clone()),
        ]);
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let tips = certified_fork_choice(&Estimator::apply(i32::MAX, None), &dag, &genesis, latest)
            .await
            .expect("tips")
            .tips;

        assert_eq!(tips.len(), 2);
        assert_eq!(tips[0], shared.block_hash);
        assert_eq!(tips[1], independent.block_hash);
    })
    .await
}
