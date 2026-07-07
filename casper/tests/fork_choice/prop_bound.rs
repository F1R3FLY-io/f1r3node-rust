// BOUND seams (B2/B3/B4) — the real `Estimator::apply(max_parents, depth)
// .tips_with_latest_messages` truncation/overflow/empty edges, mirroring the
// `formal/rocq/fork_choice/theories/Bound.v` model and the `fork_choice_bound_correct`
// capstone the gate re-checks axiom-free. These edges previously had NO Rust modality.
//
//   * B2 (sentinel / usize-safe) — estimator.rs:120-129 treats BOTH "unlimited"
//     sentinels EXPLICITLY (`max_number_of_parents < 0` OR `== UNLIMITED_PARENTS`
//     (i32::MAX)) rather than relying on `-1 as usize == usize::MAX` wrapping. A
//     genuine positive cap truncates via `take(cap)`. We assert `-1`, `i32::MAX`,
//     and a positive cap all behave (no panic / no usize wrap) and that the positive
//     cap actually truncates.
//
//   * count-cap (take_never_drops_head) — `take(cap)` preserves order and never
//     drops the head, so the MAIN parent (`tips[0]`) survives ANY positive cap.
//     Proptest over random positive caps.
//
//   * B3 (score overflow) — estimator.rs:267-274 accumulates validator weight with
//     `checked_add`, surfacing a supply-cap-violating overflow as a typed
//     `Err(KvStoreError::InvalidArgument)` instead of wrapping. Reachable through the
//     PUBLIC API: two validators each bonded at `i64::MAX` whose latest messages share
//     a common supported block (the LCA, genesis) force `genesis.score = MAX + MAX`,
//     which `checked_add` rejects. (This is an already-invalid state — total bonded
//     stake is <= i64::MAX by the supply cap — so the check can only ever reject an
//     invalid input, never a valid one; the test confirms it rejects rather than wraps.)
//
//   * B4 (empty-tips typed Err) — estimator.rs:156-162: when `rank_forkchoices`
//     returns no tips AND a `Some(max_parent_depth)` is configured, `filter_deep_parents`
//     returns `Err(InvalidArgument)` (the P2-8 fix) rather than panicking on
//     `split_first().unwrap()`. `filter_deep_parents` is a PRIVATE `async fn` reading a
//     live LMDB-backed DAG, and — as the sibling `prop_filter_deep_parents.rs` documents
//     — `rank_forkchoices` structurally ALWAYS returns >= 1 tip (it seeds from the LCA
//     and `replace_block_hash_with_children` falls back to `[b]`), so the empty branch
//     is UNREACHABLE through the public `tips_*` path. We therefore (a) transcribe the
//     empty branch as a pure function and assert it yields the typed `Err` byte-for-byte,
//     and (b) exercise the REAL `Some(depth)` branch on a genesis-only DAG (a non-empty
//     `[genesis]` ranked list) to confirm it returns cleanly (Ok, no panic).
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
use shared::rust::store::key_value_store::KvStoreError;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{create_block, create_genesis_block};
use crate::helper::block_util::generate_validator;

lazy_static::lazy_static! {
    static ref RUNTIME: tokio::runtime::Runtime =
        tokio::runtime::Runtime::new().expect("tokio runtime");
}

/// A minimal 2-TIP DAG: genesis (bonds {v1: stake_v1, v2: stake_v2}) with two
/// sibling children — v1's `b_a` and v2's `b_b`, both on genesis. The LCA is
/// genesis and both children are scored leaves, so `tips` returns two parents.
/// Returns the genesis and the latest-message map {v1 -> b_a, v2 -> b_b}.
async fn build_two_tip_dag(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
    stake_v1: i64,
    stake_v2: i64,
) -> (BlockMessage, HashMap<Validator, BlockHash>) {
    let v1 = generate_validator(Some("Bound V1"));
    let v2 = generate_validator(Some("Bound V2"));
    let bonds = vec![
        Bond { validator: v1.clone(), stake: stake_v1 },
        Bond { validator: v2.clone(), stake: stake_v2 },
    ];

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

    let just: HashMap<Validator, BlockHash> = HashMap::from([
        (v1.clone(), genesis.block_hash.clone()),
        (v2.clone(), genesis.block_hash.clone()),
    ]);

    let b_a = create_block(
        block_store,
        block_dag_storage,
        vec![genesis.block_hash.clone()],
        &genesis,
        Some(v1.clone()),
        Some(bonds.clone()),
        Some(just.clone()),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let b_b = create_block(
        block_store,
        block_dag_storage,
        vec![genesis.block_hash.clone()],
        &genesis,
        Some(v2.clone()),
        Some(bonds.clone()),
        Some(just.clone()),
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let latest = HashMap::from([
        (v1, b_a.block_hash.clone()),
        (v2, b_b.block_hash.clone()),
    ]);
    (genesis, latest)
}

// B2 (example): both "unlimited" sentinels (`-1` and `i32::MAX`) take ALL tips
// with no `usize` wrap / panic, and a genuine positive cap truncates while keeping
// the head.
#[tokio::test]
async fn b2_sentinel_and_positive_cap_usize_safe() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let (genesis, latest) =
            build_two_tip_dag(&mut block_store, &mut block_dag_storage, 2, 3).await;
        let mut dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        // Unlimited via i32::MAX sentinel.
        let full = Estimator::apply(i32::MAX, None)
            .tips_with_latest_messages(&mut dag, &genesis, latest.clone())
            .await
            .expect("full tips")
            .tips;
        assert_eq!(full.len(), 2, "two sibling leaves must yield two tips");

        // Unlimited via the -1 config sentinel — must NOT wrap `-1 as usize`.
        let neg = Estimator::apply(-1, None)
            .tips_with_latest_messages(&mut dag, &genesis, latest.clone())
            .await
            .expect("neg-1 tips")
            .tips;
        assert_eq!(neg, full, "max_number_of_parents = -1 must mean unlimited (take all)");

        // Positive cap of 1 truncates to just the head.
        let cap1 = Estimator::apply(1, None)
            .tips_with_latest_messages(&mut dag, &genesis, latest.clone())
            .await
            .expect("cap-1 tips")
            .tips;
        assert_eq!(cap1.len(), 1, "positive cap of 1 must truncate to one tip");
        assert_eq!(cap1[0], full[0], "the truncated tip must be the head (main parent)");

        // Positive cap equal to the tip count keeps everything, in order.
        let cap2 = Estimator::apply(2, None)
            .tips_with_latest_messages(&mut dag, &genesis, latest)
            .await
            .expect("cap-2 tips")
            .tips;
        assert_eq!(cap2, full, "cap == tip count is a no-op truncation");
    })
    .await
}

// B3 (example): a supply-cap-violating weight map (two i64::MAX bonds meeting at
// the LCA) overflows the score `checked_add` and must yield a typed Err, not a wrap.
#[tokio::test]
async fn b3_score_overflow_is_typed_err() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let (genesis, latest) =
            build_two_tip_dag(&mut block_store, &mut block_dag_storage, i64::MAX, i64::MAX).await;
        let mut dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        let result = Estimator::apply(i32::MAX, None)
            .tips_with_latest_messages(&mut dag, &genesis, latest)
            .await;

        match result {
            Err(KvStoreError::InvalidArgument(msg)) => {
                assert!(
                    msg.contains("overflow"),
                    "expected a score-overflow InvalidArgument, got: {msg}"
                );
            }
            Err(other) => panic!("expected InvalidArgument overflow, got a different error: {other}"),
            Ok(fc) => panic!(
                "expected a typed overflow Err, but score wrapped and returned {} tips",
                fc.tips.len()
            ),
        }
    })
    .await
}

// B4 (example): the real `Some(depth)` branch returns cleanly (Ok) on a genesis-only
// DAG — whose `rank_forkchoices` yields the non-empty `[genesis]` — confirming no panic.
#[tokio::test]
async fn b4_some_depth_on_genesis_only_is_ok() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let mut dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        // Some(0) drives the `filter_deep_parents` depth branch; empty latest =>
        // LCA = genesis => ranked = [genesis] (non-empty) => Ok([genesis]).
        let forkchoice = Estimator::apply(i32::MAX, Some(0))
            .tips_with_latest_messages(&mut dag, &genesis, HashMap::new())
            .await
            .expect("Some(depth) on genesis-only DAG must return cleanly");
        assert_eq!(
            forkchoice.tips,
            vec![genesis.block_hash.clone()],
            "genesis-only fork choice is [genesis]"
        );
    })
    .await
}

/// Pure transcription of `Estimator::filter_deep_parents`'s `Some(max_parent_depth)`
/// EMPTY-input case (estimator.rs:156-162), byte-for-byte. The private `async` method
/// reads a live LMDB DAG and its Err branch is unreachable through the public API
/// (`rank_forkchoices` structurally returns >= 1 tip); this pure function pins the
/// typed-error contract the way the sibling `prop_filter_deep_parents.rs` pins the
/// non-empty branch.
fn filter_deep_parents_empty_branch(
    _max_parent_depth: i64,
    ranked: &[BlockHash],
) -> Result<Vec<BlockHash>, KvStoreError> {
    let Some((main, _secondary)) = ranked.split_first() else {
        return Err(KvStoreError::InvalidArgument(
            "consensus invariant: rank_forkchoices returned no tips (genesis-only DAG?)"
                .to_string(),
        ));
    };
    Ok(vec![main.clone()])
}

// B4 (empty-tips typed Err): the transcribed empty branch yields a typed
// InvalidArgument (not a panic, not an empty Vec), and a non-empty input is Ok(head).
#[test]
fn b4_empty_ranked_tips_is_typed_err() {
    let empty: Vec<BlockHash> = Vec::new();
    match filter_deep_parents_empty_branch(0, &empty) {
        Err(KvStoreError::InvalidArgument(msg)) => {
            assert!(msg.contains("no tips"), "unexpected empty-branch message: {msg}");
        }
        other => panic!("empty ranked tips must be a typed InvalidArgument Err, got: {other:?}"),
    }

    let one = vec![BlockHash::from_static(b"main-parent")];
    let kept = filter_deep_parents_empty_branch(0, &one).expect("non-empty input is Ok");
    assert_eq!(kept, one, "non-empty input keeps the head");
}

prop_compose! {
    fn positive_cap()(cap in 1i32..=5) -> i32 { cap }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 16, max_shrink_iters: 8, ..ProptestConfig::default() })]

    // count-cap (take_never_drops_head): with ANY positive cap, `take(cap)` keeps the
    // head (main parent) and returns a prefix of the full ranked tips.
    #[test]
    fn take_never_drops_head(cap in positive_cap()) {
        RUNTIME.block_on(with_storage(|mut block_store, mut block_dag_storage| async move {
            let (genesis, latest) =
                build_two_tip_dag(&mut block_store, &mut block_dag_storage, 2, 3).await;
            let mut dag = block_dag_storage
                .get_representation()
                .expect("dag representation");

            let full = Estimator::apply(i32::MAX, None)
                .tips_with_latest_messages(&mut dag, &genesis, latest.clone())
                .await
                .expect("full tips")
                .tips;

            let capped = Estimator::apply(cap, None)
                .tips_with_latest_messages(&mut dag, &genesis, latest)
                .await
                .expect("capped tips")
                .tips;

            let expected_len = (cap as usize).min(full.len());
            prop_assert_eq!(capped.len(), expected_len, "cap must truncate to min(cap, |tips|)");
            prop_assert_eq!(&capped[0], &full[0], "the head (main parent) must always survive the cap");
            prop_assert_eq!(&capped[..], &full[..expected_len], "capped tips must be the ordered prefix");
            Ok::<(), TestCaseError>(())
        }))?;
    }
}
