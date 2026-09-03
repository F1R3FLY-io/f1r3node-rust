// LCA (LUCA) properties — the real `DagOperations::lowest_universal_common_ancestor_many`
// driven on RANDOM well-formed DAGs (mirrors the GAP-2 random-DAG proptest pattern in
// `block-storage/tests/block_dag_storage_test.rs`, and the fixed-DAG example in
// `casper/tests/util/dag_operations_test.rs`, which is left untouched).
//
// NAMING (LUCA vs lcua): both spell the SAME concept — "Lowest Universal Common
// Ancestor". Prose here uses "LUCA", the acronym of the Rust function
// `lowest_universal_common_ancestor_many` (L-U-C-A). The test/identifier `lcua_many_is_max`
// is the EXACT Rocq theorem name in `Lca.v` (which titles the concept "LCUA"), mirrored
// verbatim so the gate's `Print Assumptions lcua_many_is_max` and the spec-to-test
// traceability line up. The two upstream sources disagree on the letter order (Rust=LUCA,
// Rocq=LCUA); neither spelling here is a typo.
//
// These are the Rust realizations of the proven fork-choice FV LCA results
// (`formal/rocq/fork_choice/theories/Lca.v`), which the gate re-checks axiom-free
// (`reduce_converges`, `lca_is_lowest`, `lcua_many_is_max`, `descends_from_root`,
// `common_ancestor_root`) but which previously had NO Rust modality:
//
//   * reduce_converges       — the frontier fold TERMINATES and returns cleanly on
//                              every random DAG (never hangs, never errors on a
//                              non-empty well-formed input).
//   * lca_is_common_ancestor — the returned LUCA `is_dag_ancestor` of EVERY input
//                              message (it is a genuine common ancestor).
//   * lcua_many_is_max       — the LUCA is the LOWEST such ancestor (no common
//     (a.k.a. lca_is_lowest)   ancestor has a strictly higher block number). Per
//                              `Lca.v` Section 7 (lca_is_lowest:934 / lcua_many_is_max:957)
//                              this maximality is proven ONLY under `single_parent_spine`
//                              (each block's parents are exactly its main parent — a
//                              TREE) + `NoDup ms`. On general MULTI-parent DAGs the
//                              fold provably OVER-DESCENDS past the true LCA when a
//                              block carries a "straddling" old parent (Lca.v:825-834,
//                              the documented RESIDUAL), so `numof c <= numof survivor`
//                              is FALSE there. We therefore assert maximality over random
//                              single-parent TREES (which satisfy `single_parent_spine`);
//                              `reduce_converges` + `lca_is_common_ancestor` above hold
//                              UNCONDITIONALLY and cover the general multi-parent DAGs.
//   * boundary               — single-genesis input => genesis; empty input => a
//                              typed `Err`; reflexive `lca(b,b) == b`; genesis is a
//                              universal ancestor (`descends_from_root`).
//
// Well-formedness: `IndexedBlockDagStorage::insert_indexed` (test/indexed_block_dag_storage.rs:76)
// assigns each inserted block a STRICTLY INCREASING `block_number` (`next_id = current_id + 1`),
// and this generator always creates parents BEFORE children, so every parent edge is
// number-decreasing (`block_number(child) > block_number(parent)`). That monotonicity is
// exactly the `wf_dag` precondition the LUCA height-reduction and `is_dag_ancestor`'s
// block-number prune rely on (the same precondition the GAP-2 test documents). The two
// ancestry primitives — `lowest_universal_common_ancestor_many` and `is_dag_ancestor` —
// are cross-checked against each other here (each catches the other's bugs).
//
// The DAG is driven through the real LMDB-backed casper test fixture (`with_storage`);
// the proptests are plain `#[test]`s that `block_on` a shared runtime (mirrors
// block_dag_storage_test.rs's `RUNTIME` + the casper lmdb_key_value_store_spec idiom),
// because `with_storage` and the LUCA API are async.
//
// LOCAL-ONLY verification (not consensus code). Run under `cargo test -p casper` and
// gated by scripts/check-fork-choice-ALL.sh via the `fork_choice::` filter.

use casper::rust::util::dag_operations::DagOperations;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::BlockMessage;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{create_block, create_genesis_block};
use crate::helper::block_util::generate_validator;

lazy_static::lazy_static! {
    static ref RUNTIME: tokio::runtime::Runtime =
        tokio::runtime::Runtime::new().expect("tokio runtime");
}

/// Max non-genesis blocks a random DAG may contain. `PARENT_BITS_LEN` reserves one
/// selection bit per (child index in 1..=MAX_BLOCKS, candidate parent index in 0..child).
const MAX_BLOCKS: usize = 6;
const PARENT_BITS_LEN: usize = MAX_BLOCKS * MAX_BLOCKS;

/// Build a random WELL-FORMED DAG: genesis (index 0) plus `n` blocks. Block `i`
/// (1-based) picks its parents from the already-created nodes `0..i` according to
/// `parent_bits` (bit `(i-1)*MAX_BLOCKS + j` selects candidate `j`); an empty
/// selection defaults to `{genesis}` so every block is connected and has >= 1 parent.
/// Because parents are always created first, `insert_indexed` numbers every child
/// above all its parents (monotone => well-formed). Returns the genesis message and
/// the node hashes indexed `[genesis, block_1, .., block_n]`.
async fn build_random_dag(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
    n: usize,
    parent_bits: &[bool],
) -> (BlockMessage, Vec<BlockHash>) {
    let genesis = create_genesis_block(
        block_store,
        block_dag_storage,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let mut hashes: Vec<BlockHash> = Vec::with_capacity(n + 1);
    hashes.push(genesis.block_hash.clone());
    let creator = generate_validator(Some("LCA DAG"));

    for i in 1..=n {
        let mut parents: Vec<BlockHash> = (0..i)
            .filter(|&j| {
                parent_bits
                    .get((i - 1) * MAX_BLOCKS + j)
                    .copied()
                    .unwrap_or(false)
            })
            .map(|j| hashes[j].clone())
            .collect();
        if parents.is_empty() {
            parents.push(hashes[0].clone());
        }
        let block = create_block(
            block_store,
            block_dag_storage,
            parents,
            &genesis,
            Some(creator.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        hashes.push(block.block_hash.clone());
    }

    (genesis, hashes)
}

/// Build a random single-parent TREE (each non-genesis block has EXACTLY ONE parent,
/// picked from `0..i`), which satisfies the `single_parent_spine` precondition that
/// `Lca.v`'s `lca_is_lowest`/`lcua_many_is_max` maximality requires. `parent_choice[i-1]`
/// selects block `i`'s parent as `parent_choice[i-1] % i`. Still well-formed (monotone).
async fn build_random_tree(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
    n: usize,
    parent_choice: &[u64],
) -> (BlockMessage, Vec<BlockHash>) {
    let genesis = create_genesis_block(
        block_store,
        block_dag_storage,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let mut hashes: Vec<BlockHash> = Vec::with_capacity(n + 1);
    hashes.push(genesis.block_hash.clone());
    let creator = generate_validator(Some("LCA tree"));

    for i in 1..=n {
        let parent = (parent_choice.get(i - 1).copied().unwrap_or(0) as usize) % i;
        let block = create_block(
            block_store,
            block_dag_storage,
            vec![hashes[parent].clone()],
            &genesis,
            Some(creator.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        hashes.push(block.block_hash.clone());
    }

    (genesis, hashes)
}

prop_compose! {
    /// A random DAG shape + a random query sub-set over its non-genesis blocks.
    fn dag_and_query()(
        n in 2usize..=MAX_BLOCKS,
        parent_bits in prop::collection::vec(any::<bool>(), PARENT_BITS_LEN),
        query_mask in prop::collection::vec(any::<bool>(), MAX_BLOCKS),
    ) -> (usize, Vec<bool>, Vec<bool>) {
        (n, parent_bits, query_mask)
    }
}

prop_compose! {
    /// A random single-parent TREE shape + a random query sub-set (for the maximality
    /// property, which holds only on `single_parent_spine` DAGs).
    fn tree_and_query()(
        n in 2usize..=MAX_BLOCKS,
        parent_choice in prop::collection::vec(any::<u64>(), MAX_BLOCKS),
        query_mask in prop::collection::vec(any::<bool>(), MAX_BLOCKS),
    ) -> (usize, Vec<u64>, Vec<bool>) {
        (n, parent_choice, query_mask)
    }
}

/// Select the query node indices (into `hashes`) from `query_mask` over blocks
/// `1..=n`; default to ALL non-genesis blocks when the mask selects nothing (so the
/// query is always non-empty).
fn query_indices(n: usize, query_mask: &[bool]) -> Vec<usize> {
    let selected: Vec<usize> = (1..=n)
        .filter(|&i| query_mask.get(i - 1).copied().unwrap_or(false))
        .collect();
    if selected.is_empty() {
        (1..=n).collect()
    } else {
        selected
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 12, max_shrink_iters: 8, ..ProptestConfig::default() })]

    // reduce_converges: the frontier fold RETURNS (does not hang, does not error) on
    // every random well-formed DAG, and the answer is a real node of the DAG.
    #[test]
    fn reduce_converges((n, parent_bits, query_mask) in dag_and_query()) {
        RUNTIME.block_on(with_storage(|mut block_store, mut block_dag_storage| async move {
            let (_genesis, hashes) =
                build_random_dag(&mut block_store, &mut block_dag_storage, n, &parent_bits).await;
            let dag = block_dag_storage.get_representation().expect("dag representation");

            let query = query_indices(n, &query_mask);
            let query_metas: Vec<BlockMetadata> = query
                .iter()
                .map(|&i| dag.lookup_unsafe(&hashes[i]).expect("query metadata"))
                .collect();

            let genesis_meta = dag.lookup_unsafe(&hashes[0]).expect("genesis metadata");
            let luca_res =
                DagOperations::lowest_universal_common_ancestor_many(&query_metas, &dag, &genesis_meta).await;
            prop_assert!(
                luca_res.is_ok(),
                "LUCA fold must return cleanly on a well-formed non-empty query"
            );
            let luca = luca_res.expect("luca ok");
            prop_assert!(
                hashes.contains(&luca.block_hash),
                "LUCA must be a real node of the DAG"
            );
            Ok::<(), TestCaseError>(())
        }))?;
    }

    // lca_is_common_ancestor: the LUCA `is_dag_ancestor` of EVERY query member.
    #[test]
    fn lca_is_common_ancestor((n, parent_bits, query_mask) in dag_and_query()) {
        RUNTIME.block_on(with_storage(|mut block_store, mut block_dag_storage| async move {
            let (_genesis, hashes) =
                build_random_dag(&mut block_store, &mut block_dag_storage, n, &parent_bits).await;
            let dag = block_dag_storage.get_representation().expect("dag representation");

            let query = query_indices(n, &query_mask);
            let query_metas: Vec<BlockMetadata> = query
                .iter()
                .map(|&i| dag.lookup_unsafe(&hashes[i]).expect("query metadata"))
                .collect();

            let genesis_meta = dag.lookup_unsafe(&hashes[0]).expect("genesis metadata");
            let luca = DagOperations::lowest_universal_common_ancestor_many(&query_metas, &dag, &genesis_meta)
                .await
                .expect("luca");

            for &i in &query {
                let is_anc = dag
                    .is_dag_ancestor(&luca.block_hash, &hashes[i])
                    .expect("is_dag_ancestor");
                prop_assert!(
                    is_anc,
                    "LUCA (num {}) is NOT an ancestor of query node index {} — not a common ancestor",
                    luca.block_number, i
                );
            }
            Ok::<(), TestCaseError>(())
        }))?;
    }

    // lcua_many_is_max (== lca_is_lowest): on a random single-parent TREE (the proven
    // `single_parent_spine` precondition) with a distinct query (`NoDup ms`), NO common
    // ancestor of the query has a strictly higher block number than the LUCA — the
    // survivor IS the maximal common ancestor. `is_dag_ancestor` (the sibling ancestry
    // primitive, validated by GAP-2) is ground truth for "common ancestor". (On general
    // multi-parent DAGs this is provably FALSE — see the module header + Lca.v Section 7;
    // `reduce_converges` and `lca_is_common_ancestor` above cover those unconditionally.)
    #[test]
    fn lcua_many_is_max((n, parent_choice, query_mask) in tree_and_query()) {
        RUNTIME.block_on(with_storage(|mut block_store, mut block_dag_storage| async move {
            let (_genesis, hashes) =
                build_random_tree(&mut block_store, &mut block_dag_storage, n, &parent_choice).await;
            let dag = block_dag_storage.get_representation().expect("dag representation");

            // Distinct indices => distinct hashes, faithful to the `NoDup ms` premise.
            let query = query_indices(n, &query_mask);
            let query_metas: Vec<BlockMetadata> = query
                .iter()
                .map(|&i| dag.lookup_unsafe(&hashes[i]).expect("query metadata"))
                .collect();

            let genesis_meta = dag.lookup_unsafe(&hashes[0]).expect("genesis metadata");
            let luca = DagOperations::lowest_universal_common_ancestor_many(&query_metas, &dag, &genesis_meta)
                .await
                .expect("luca");

            // Every node that is a common ancestor of the whole query.
            for c in 0..hashes.len() {
                let is_common = query
                    .iter()
                    .all(|&q| dag.is_dag_ancestor(&hashes[c], &hashes[q]).expect("is_dag_ancestor"));
                if is_common {
                    let c_num = dag.lookup_unsafe(&hashes[c]).expect("meta").block_number;
                    prop_assert!(
                        c_num <= luca.block_number,
                        "common ancestor (num {}) is higher-numbered than the LUCA (num {}) — LUCA not maximal on a single-parent tree",
                        c_num, luca.block_number
                    );
                }
            }
            Ok::<(), TestCaseError>(())
        }))?;
    }
}

// boundary (example): single-genesis => genesis; empty => typed Err; reflexive
// pairwise `lca(b,b) == b`; genesis is a universal ancestor. Uses a tiny fixed DAG.
#[tokio::test]
async fn lca_single_and_genesis_boundary() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        // parent_bits all-false => each block defaults to genesis as its parent
        // (a two-deep chain rooted at genesis: genesis <- b1 <- ... but with the
        // all-genesis default b1 and b2 both hang off genesis).
        let (genesis, hashes) = build_random_dag(
            &mut block_store,
            &mut block_dag_storage,
            2,
            &[false; PARENT_BITS_LEN],
        )
        .await;
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        let genesis_meta = dag
            .lookup_unsafe(&genesis.block_hash)
            .expect("genesis meta");
        let b1_meta = dag.lookup_unsafe(&hashes[1]).expect("b1 meta");

        // single-genesis input => genesis (the len==1 fast path).
        let single = DagOperations::lowest_universal_common_ancestor_many(
            std::slice::from_ref(&genesis_meta),
            &dag,
            &genesis_meta,
        )
        .await
        .expect("single luca");
        assert_eq!(
            single, genesis_meta,
            "single-genesis input must return genesis"
        );

        // empty input => typed Err (the documented boundary of the fold).
        let empty =
            DagOperations::lowest_universal_common_ancestor_many(&[], &dag, &genesis_meta).await;
        assert!(
            empty.is_err(),
            "empty input must be a typed Err, not a panic"
        );

        // reflexive pairwise: lca(b, b) == b.
        let reflexive = DagOperations::lowest_universal_common_ancestor(
            &b1_meta,
            &b1_meta,
            &dag,
            &genesis_meta,
        )
        .await
        .expect("reflexive luca");
        assert_eq!(reflexive, b1_meta, "lca(b, b) must be b");

        // genesis is a universal ancestor (descends_from_root): ancestor of every node.
        for h in &hashes {
            assert!(
                dag.is_dag_ancestor(&genesis.block_hash, h)
                    .expect("is_dag_ancestor"),
                "genesis must be an ancestor of every node"
            );
        }
    })
    .await
}
