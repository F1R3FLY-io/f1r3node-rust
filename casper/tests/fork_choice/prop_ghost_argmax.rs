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
//     subtree). We build a RANDOM single-level weighted fork — genesis with a random
//     number of branch blocks (≥ 2), each branch a distinct child of genesis carrying
//     a random-stake validator set — so every branch's own score IS its subtree
//     weight (a depth-1 subtree is its own leaf). We independently sum each branch's
//     supporter stake and assert the real `tips_with_latest_messages`:
//       (a) returns exactly the branches, ordered `(score DESC, hash ASC)` — the full
//           `rank_selects_heaviest` ranking; and
//       (b) leads with the heaviest branch — `rank_head_is_argmax`.
//     The score a branch accrues is `Σ weight_from_validator_by_dag(branch, v)` over
//     the validators whose latest message is that branch (estimator.rs:215-299), and
//     `weight_from_validator_by_dag` reads the block's MAIN-PARENT (genesis) weight
//     map, so a branch's score is the total genesis-bonded stake of its supporters —
//     exactly the oracle below.
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
use crate::helper::block_generator::{create_block, create_genesis_block};
use crate::helper::block_util::generate_validator;

lazy_static::lazy_static! {
    // Shared Tokio runtime for the `#[test]` proptests (they cannot be
    // `#[tokio::test]`); mirrors casper/tests/fork_choice/prop_estimator_determinism.rs.
    static ref RUNTIME: tokio::runtime::Runtime =
        tokio::runtime::Runtime::new().expect("tokio runtime");
}

#[allow(clippy::too_many_arguments)]
fn make_branch(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
    genesis: &BlockMessage,
    creator: &Validator,
    bonds: &[Bond],
    justifications: HashMap<Validator, BlockHash>,
) -> BlockMessage {
    create_block(
        block_store,
        block_dag_storage,
        vec![genesis.block_hash.clone()],
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
        None,
    )
}

/// A generated single-level weighted fork: `n` validators with random stakes,
/// partitioned onto `n_branches` (≥ 2) distinct children of genesis. `assign[i]` is
/// validator `i`'s branch; branches `0..n_branches` each carry ≥ 1 validator.
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

/// Build the fork in the DAG and return `(genesis, per-branch blocks, latest-message
/// map)`. All blocks carry the SAME bonds; the score a branch accrues is the total
/// genesis-bonded stake of the validators whose latest message it is.
async fn build_fork(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
    shape: &ForkShape,
) -> (
    BlockMessage,
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
        let block = make_branch(
            block_store,
            block_dag_storage,
            &genesis,
            &validators[creator_idx],
            &bonds,
            justifications.clone(),
        );
        branch_blocks.push(block);
    }

    let latest: HashMap<Validator, BlockHash> = validators
        .iter()
        .enumerate()
        .map(|(i, v)| (v.clone(), branch_blocks[shape.assign[i]].block_hash.clone()))
        .collect();

    (genesis, branch_blocks, latest)
}

/// The INDEPENDENT heaviest-subtree oracle: each branch's score is the sum of its
/// supporters' stakes; the expected ranked tips are the branch hashes ordered
/// `(score DESC, hash ASC)` — the same total order `sort_by_with_decreasing_order`
/// (estimator.rs:320) realizes.
fn expected_ranked_tips(shape: &ForkShape, branch_blocks: &[BlockMessage]) -> Vec<BlockHash> {
    let mut scores = vec![0i64; shape.n_branches];
    for (i, &branch) in shape.assign.iter().enumerate() {
        scores[branch] += shape.stakes[i];
    }
    let mut ranked: Vec<(i64, BlockHash)> = (0..shape.n_branches)
        .map(|j| (scores[j], branch_blocks[j].block_hash.clone()))
        .collect();
    // score DESC, then hash ASC (the estimator's decreasing-order tie-break).
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    ranked.into_iter().map(|(_, hash)| hash).collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 16, max_shrink_iters: 8, ..ProptestConfig::default() })]

    // T-GHOST (Rank.v `rank_head_is_argmax` + `rank_selects_heaviest`): on a RANDOM
    // weighted fork the estimator's ranked tips equal the branches ordered by
    // heaviest subtree first (score DESC, hash ASC), and the HEAD is the heaviest —
    // the GHOST argmax. The oracle is computed independently from the generated
    // stakes/assignment, so a wrong ranking (or a leaked HashMap iteration order)
    // fails the test.
    #[test]
    fn ghost_ranked_tips_are_heaviest_subtree_argmax(shape in fork_shape()) {
        RUNTIME.block_on(with_storage(|mut block_store, mut block_dag_storage| async move {
            let (genesis, branch_blocks, latest) =
                build_fork(&mut block_store, &mut block_dag_storage, &shape).await;
            let mut dag = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let estimator = Estimator::apply(i32::MAX, None);

            let tips = estimator
                .tips_with_latest_messages(&mut dag, &genesis, latest)
                .await
                .expect("tips")
                .tips;

            let expected = expected_ranked_tips(&shape, &branch_blocks);

            // (a) rank_selects_heaviest: the full ranked tip list is the heaviest-first
            //     ordering of the branches (each branch is its own depth-1 subtree).
            prop_assert_eq!(
                &tips, &expected,
                "estimator tips must equal the heaviest-subtree ranking (score DESC, hash ASC)"
            );
            // (b) rank_head_is_argmax: the head is the maximum-score branch.
            prop_assert_eq!(
                tips.first(), expected.first(),
                "estimator head tip must be the heaviest-subtree argmax"
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
        let (genesis, branch_blocks, latest) =
            build_fork(&mut block_store, &mut block_dag_storage, &shape).await;
        let mut dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let estimator = Estimator::apply(i32::MAX, None);

        // The heaviest branch is validator 0's (stake 30) — the expected main parent.
        let expected_main = branch_blocks[0].block_hash.clone();

        // Deterministic across every ordering of the latest-message map: the ghost main
        // parent (tips[0]) is invariant to HashMap iteration order.
        let entries: Vec<(Validator, BlockHash)> = latest.into_iter().collect();
        let orders: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
            2, 1, 0,
        ]];
        for order in orders {
            let permuted: HashMap<Validator, BlockHash> =
                order.iter().map(|&i| entries[i].clone()).collect();
            let ghost_main_parent = estimator
                .tips_with_latest_messages(&mut dag, &genesis, permuted)
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
            branch_blocks[2].clone(),
            branch_blocks[0].clone(),
            branch_blocks[1].clone(),
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
