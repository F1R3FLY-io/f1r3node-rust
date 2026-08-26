// Fork-choice DETERMINISM + SCORE-MONOID + FILTER (T-10) — the real `Estimator`
// driven on a fixed representative multi-validator DAG (mirrors
// `casper/tests/batch2/estimator_test.rs`'s builder).
//
// These are the Rust realizations of three proven fork-choice FV results that
// previously had NO Rust modality (only Rocq/TLA+/Z3/Sage):
//
//   * DETERMINISM  — `formal/rocq/fork_choice/theories/MainTheorem.v`'s
//     `fork_choice_determinism_correct` (and TLA+ `Inv_Deterministic`): the
//     fork-choice result is a pure function of the (validator -> latest-hash)
//     RELATION, independent of the iteration order of the `latest_messages`
//     map. Rust `std::collections::HashMap` seeds a fresh `RandomState` per
//     instance, so two maps built from the SAME pairs iterate in (generally)
//     different orders — certifying those maps and feeding the resulting context into
//     asserting identical `.tips` is the observable witness.
//
//   * SCORE MONOID — `formal/rocq/fork_choice/theories/Score.v`'s
//     `score_perm_invariant` (Sage `forkchoice_algebra.sage` score monoid):
//     `Estimator::build_scores_map` (estimator.rs:215) accumulates validator
//     weight by `+` over `latest_messages_hashes.iter()`, and `+` on `i64` is a
//     commutative monoid, so permuting the validators' order cannot change the
//     score map — hence cannot change the tips. `build_scores_map` is a PRIVATE
//     `async fn`, so (exactly as the task specifies) we assert the monoid
//     THROUGH the public `tips` path: permuting the latest-message order never
//     changes the outcome. That order-invariance is the monoid's only
//     observable consequence.
//
//   * FILTER (T-10) — `formal/rocq/slashing/theories/ForkChoice.v`'s
//     `fork_choice_exclusion` / GuardBridge's honest-parent filter: a slashed /
//     invalid latest message contributes ZERO to fork choice. The estimator
//     realizes this via `dag.invalid_latest_messages_from_hashes` +
//     `retain` (estimator.rs:86-91). We include an INVALID latest message
//     (`create_block(.., invalid = Some(true))`, the same DAG invalid-blocks
//     mechanism `block_dag_storage_test.rs`'s invalid-blocks test uses) and
//     assert it is excluded — the tips are identical with or without it — while
//     the valid validator is retained. (`uc_16`/`uc_17` assert the same T-10
//     exclusion against the abstract `SlashingTestHarness`; this file asserts it
//     against the REAL `Estimator` + `KeyValueDagRepresentation`.)
//
// LOCAL-ONLY verification (not consensus code). Run under `cargo test -p casper`
// and gated by scripts/check-fork-choice-ALL.sh via the `fork_choice::` filter.

use std::collections::{BTreeMap, HashMap};

use casper::rust::causal_equivocation::{CertifiedConsensusContext, VoteExclusion};
use casper::rust::estimator::Estimator;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::validator::Validator;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{
    certified_consensus_context, certified_fork_choice, create_block, create_genesis_block,
};
use crate::helper::block_util::generate_validator;

lazy_static::lazy_static! {
    // Shared Tokio runtime for the `#[test]` proptests (they cannot be
    // `#[tokio::test]`); mirrors casper/tests/batch2/lmdb_key_value_store_spec.rs.
    static ref RUNTIME: tokio::runtime::Runtime =
        tokio::runtime::Runtime::new().expect("tokio runtime");
}

// Macro to create justifications HashMap without excessive cloning
// (copied verbatim from estimator_test.rs so the builder is byte-identical).
macro_rules! justifications {
    ($($validator:expr => $block_hash:expr),* $(,)?) => {
        {
            let mut map = std::collections::HashMap::new();
            $(
                map.insert($validator.clone(), $block_hash.clone());
            )*
            map
        }
    };
}

/// The immutable context every block in a test DAG shares: the genesis it
/// descends from and the bond set that weights fork choice. Both are fixed for
/// the whole DAG and are always passed together, so they travel as one value.
struct DagContext<'a> {
    genesis: &'a BlockMessage,
    bonds: &'a [Bond],
}

fn create_test_block(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
    parents: &[BlockHash],
    ctx: &DagContext<'_>,
    creator: &Validator,
    justifications: HashMap<Validator, BlockHash>,
    invalid: Option<bool>,
) -> BlockMessage {
    create_block(
        block_store,
        block_dag_storage,
        parents.to_vec(),
        ctx.genesis,
        Some(creator.clone()),
        Some(ctx.bonds.to_vec()),
        Some(justifications),
        None,
        None,
        None,
        None,
        None,
        invalid,
    )
}

/// The fixed "flipping fork-choice" DAG from estimator_test.rs (3 validators,
/// stakes 25/20/15). Returns the genesis, the canonical (validator, latest-hash)
/// list, and the KNOWN-GOOD tips `[b8, b7]`. Both the example and the proptests
/// rebuild this exact DAG so the ONLY thing that varies is the latest-message
/// order/subset.
struct FlippingScenario {
    genesis: BlockMessage,
    /// (validator, latest-block-hash), in a canonical order.
    canonical_latest: Vec<(Validator, BlockHash)>,
    /// The heaviest-subtree tips the estimator must return for the FULL set.
    expected_tips: Vec<BlockHash>,
}

async fn build_flipping_dag(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
) -> FlippingScenario {
    let v1 = generate_validator(Some("Validator One"));
    let v2 = generate_validator(Some("Validator Two"));
    let v3 = generate_validator(Some("Validator Three"));
    let bonds = vec![
        Bond {
            validator: v1.clone(),
            stake: 25,
        },
        Bond {
            validator: v2.clone(),
            stake: 20,
        },
        Bond {
            validator: v3.clone(),
            stake: 15,
        },
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

    let ctx = DagContext {
        genesis: &genesis,
        bonds: &bonds,
    };

    let b2 = create_test_block(
        block_store,
        block_dag_storage,
        std::slice::from_ref(&genesis.block_hash),
        &ctx,
        &v2,
        justifications!(v1 => genesis.block_hash, v2 => genesis.block_hash, v3 => genesis.block_hash),
        None,
    );
    let b3 = create_test_block(
        block_store,
        block_dag_storage,
        std::slice::from_ref(&genesis.block_hash),
        &ctx,
        &v1,
        justifications!(v1 => genesis.block_hash, v2 => genesis.block_hash, v3 => genesis.block_hash),
        None,
    );
    let b4 = create_test_block(
        block_store,
        block_dag_storage,
        std::slice::from_ref(&b2.block_hash),
        &ctx,
        &v3,
        justifications!(v1 => genesis.block_hash, v2 => b2.block_hash, v3 => b2.block_hash),
        None,
    );
    let b5 = create_test_block(
        block_store,
        block_dag_storage,
        std::slice::from_ref(&b3.block_hash),
        &ctx,
        &v2,
        justifications!(v1 => b3.block_hash, v2 => b2.block_hash, v3 => genesis.block_hash),
        None,
    );
    let b6 = create_test_block(
        block_store,
        block_dag_storage,
        std::slice::from_ref(&b4.block_hash),
        &ctx,
        &v1,
        justifications!(v1 => b3.block_hash, v2 => b2.block_hash, v3 => b4.block_hash),
        None,
    );
    let b7 = create_test_block(
        block_store,
        block_dag_storage,
        std::slice::from_ref(&b5.block_hash),
        &ctx,
        &v3,
        justifications!(v1 => b3.block_hash, v2 => b5.block_hash, v3 => b4.block_hash),
        None,
    );
    let b8 = create_test_block(
        block_store,
        block_dag_storage,
        std::slice::from_ref(&b6.block_hash),
        &ctx,
        &v2,
        justifications!(v1 => b6.block_hash, v2 => b5.block_hash, v3 => b4.block_hash),
        None,
    );

    FlippingScenario {
        canonical_latest: vec![
            (v1, b6.block_hash.clone()),
            (v2, b8.block_hash.clone()),
            (v3, b7.block_hash.clone()),
        ],
        expected_tips: vec![b8.block_hash, b7.block_hash],
        genesis,
    }
}

struct FrozenAuthorityScenario {
    genesis: BlockMessage,
    common: BlockMessage,
    left: BlockMessage,
    right: BlockMessage,
    v1: Validator,
    v2: Validator,
}

async fn build_frozen_authority_scenario(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
) -> FrozenAuthorityScenario {
    let v1 = generate_validator(Some("Frozen Authority One"));
    let v2 = generate_validator(Some("Frozen Authority Two"));
    let floor_bonds = vec![
        Bond {
            validator: v1.clone(),
            stake: 10,
        },
        Bond {
            validator: v2.clone(),
            stake: 5,
        },
    ];
    let unfinalized_bonds = vec![
        Bond {
            validator: v1.clone(),
            stake: 1,
        },
        Bond {
            validator: v2.clone(),
            stake: 100,
        },
    ];
    let genesis = create_genesis_block(
        block_store,
        block_dag_storage,
        None,
        Some(floor_bonds),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let common = create_block(
        block_store,
        block_dag_storage,
        vec![genesis.block_hash.clone()],
        &genesis,
        Some(v1.clone()),
        Some(unfinalized_bonds.clone()),
        Some(justifications!(v1 => genesis.block_hash, v2 => genesis.block_hash)),
        None,
        None,
        None,
        None,
        Some(1),
        None,
    );
    let left = create_block(
        block_store,
        block_dag_storage,
        vec![common.block_hash.clone()],
        &genesis,
        Some(v1.clone()),
        Some(unfinalized_bonds.clone()),
        Some(justifications!(v1 => common.block_hash, v2 => genesis.block_hash)),
        None,
        None,
        None,
        None,
        Some(2),
        None,
    );
    let right = create_block(
        block_store,
        block_dag_storage,
        vec![common.block_hash.clone()],
        &genesis,
        Some(v2.clone()),
        Some(unfinalized_bonds),
        Some(justifications!(v1 => common.block_hash, v2 => genesis.block_hash)),
        None,
        None,
        None,
        None,
        Some(2),
        None,
    );
    FrozenAuthorityScenario {
        genesis,
        common,
        left,
        right,
        v1,
        v2,
    }
}

/// Every distinct ordering of a 3-element list (the 3! insertion orders of the
/// latest-message pairs). Used to drive the example determinism test.
const ORDERS_3: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
    2, 1, 0,
]];

// DETERMINISM (example): the flipping DAG returns the SAME tips for every
// insertion order of the latest-message map, AND that stable answer is the
// known-good heaviest-subtree `[b8, b7]`. Because `HashMap` reseeds per
// instance, each of the six maps also iterates in its own order — so this
// pins BOTH order-independence and correctness.
#[tokio::test]
async fn fork_choice_determinism_correct() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let scenario = build_flipping_dag(&mut block_store, &mut block_dag_storage).await;
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let estimator = Estimator::apply(i32::MAX, None);

        let mut all_tips = Vec::new();
        for order in ORDERS_3 {
            // Fresh HashMap per order => fresh RandomState => distinct iteration order.
            let latest: HashMap<Validator, BlockHash> = order
                .iter()
                .map(|&i| scenario.canonical_latest[i].clone())
                .collect();
            let tips = certified_fork_choice(&estimator, &dag, &scenario.genesis, latest)
                .await
                .expect("tips")
                .tips;
            all_tips.push(tips);
        }

        for (i, tips) in all_tips.iter().enumerate() {
            assert_eq!(
                tips, &all_tips[0],
                "order #{i} produced different tips than order #0 — fork choice leaked map order"
            );
        }
        assert_eq!(
            all_tips[0], scenario.expected_tips,
            "deterministic tips must equal the known-good heaviest subtree [b8, b7]"
        );
    })
    .await
}

// FILTER (T-10) (example): an INVALID latest message contributes zero to fork
// choice. v0's latest block is marked invalid; the estimator must (a) flag it
// via `invalid_latest_messages_from_hashes`, (b) return tips IDENTICAL to the
// run that omits v0 entirely (excluded => contributes zero), and (c) never emit
// the invalid block as a tip, while (d) the valid v1 is retained and drives the
// result.
#[tokio::test]
async fn filter_t10_invalid_latest_message_excluded() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("Valid One"));
        let v0 = generate_validator(Some("Slashed Zero"));
        let bonds = vec![
            Bond {
                validator: v1.clone(),
                stake: 5,
            },
            Bond {
                validator: v0.clone(),
                stake: 7,
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

        let ctx = DagContext {
            genesis: &genesis,
            bonds: &bonds,
        };

        // v1's valid block on genesis.
        let b_valid = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&genesis.block_hash),
            &ctx,
            &v1,
            justifications!(v1 => genesis.block_hash, v0 => genesis.block_hash),
            None,
        );
        // v0's INVALID block on genesis (goes into the DAG's invalid-blocks set).
        let b_invalid = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&genesis.block_hash),
            &ctx,
            &v0,
            justifications!(v1 => genesis.block_hash, v0 => genesis.block_hash),
            Some(true),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        let latest_all: HashMap<Validator, BlockHash> = HashMap::from([
            (v1.clone(), b_valid.block_hash.clone()),
            (v0.clone(), b_invalid.block_hash.clone()),
        ]);

        // (a) the invalid latest message is FLAGGED for exactly v0, not v1.
        let flagged = dag
            .invalid_latest_messages_from_hashes(&latest_all)
            .expect("invalid latest messages");
        assert!(
            flagged.contains_key(&v0),
            "v0's invalid latest message must be flagged"
        );
        assert!(
            !flagged.contains_key(&v1),
            "v1's valid latest message must NOT be flagged"
        );

        let estimator = Estimator::apply(i32::MAX, None);

        let tips_all = certified_fork_choice(&estimator, &dag, &genesis, latest_all)
            .await
            .expect("tips (all)")
            .tips;

        // (b) tips with v0 present == tips with v0 omitted (v0 contributes zero).
        let latest_v1_only: HashMap<Validator, BlockHash> =
            HashMap::from([(v1.clone(), b_valid.block_hash.clone())]);
        let tips_v1_only = certified_fork_choice(&estimator, &dag, &genesis, latest_v1_only)
            .await
            .expect("tips (v1 only)")
            .tips;
        assert_eq!(
            tips_all, tips_v1_only,
            "invalid v0 must be excluded — tips must match the v1-only fork choice"
        );

        // (c) the invalid block is never a tip; (d) the valid block is the tip.
        assert!(
            !tips_all.contains(&b_invalid.block_hash),
            "invalid block must not appear in the fork-choice tips"
        );
        assert_eq!(
            tips_all,
            vec![b_valid.block_hash.clone()],
            "the retained valid validator drives the fork choice"
        );
    })
    .await
}

#[tokio::test]
async fn unfinalized_bond_cache_cannot_reweight_frozen_floor_fork_choice() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let scenario =
            build_frozen_authority_scenario(&mut block_store, &mut block_dag_storage).await;
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let latest = HashMap::from([
            (scenario.v1.clone(), scenario.left.block_hash.clone()),
            (scenario.v2.clone(), scenario.right.block_hash.clone()),
        ]);
        let context = certified_consensus_context(&dag, &scenario.genesis, latest)
            .expect("certified consensus context");
        let choice = Estimator::apply(i32::MAX, None)
            .tips_with_context(&dag, &scenario.genesis, &context)
            .await
            .expect("fork choice");

        assert_eq!(choice.lca, scenario.common.block_hash);
        assert_eq!(choice.tips[0], scenario.left.block_hash);
        assert_eq!(choice.tips[1], scenario.right.block_hash);
    })
    .await
}

#[tokio::test]
async fn certified_fork_choice_is_independent_of_receiver_local_indices() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let scenario =
            build_frozen_authority_scenario(&mut block_store, &mut block_dag_storage).await;
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let latest = HashMap::from([
            (scenario.v1.clone(), scenario.left.block_hash.clone()),
            (scenario.v2.clone(), scenario.right.block_hash.clone()),
        ]);
        let context = certified_consensus_context(&dag, &scenario.genesis, latest)
            .expect("certified consensus context");
        let estimator = Estimator::apply(i32::MAX, None);
        let baseline = estimator
            .tips_with_context(&dag, &scenario.genesis, &context)
            .await
            .expect("baseline fork choice");

        let mut receiver_variant = dag.clone();
        receiver_variant.invalid_blocks_set.insert(
            receiver_variant
                .lookup_unsafe(&scenario.left.block_hash)
                .expect("left metadata"),
        );
        receiver_variant.latest_messages_map = imbl::HashMap::new();
        receiver_variant
            .finalized_blocks_set
            .insert(scenario.right.block_hash.clone());
        let unrelated_hash = BlockHash::from(vec![0x7f; models::rust::block_hash::LENGTH]);
        let mut unrelated_height = imbl::HashSet::new();
        unrelated_height.insert(unrelated_hash);
        receiver_variant.height_map.insert(10_000, unrelated_height);

        let variant = estimator
            .tips_with_context(&receiver_variant, &scenario.genesis, &context)
            .await
            .expect("receiver-variant fork choice");
        assert_eq!(variant, baseline);
        assert_eq!(variant.lca, scenario.common.block_hash);
    })
    .await
}

#[tokio::test]
async fn fork_choice_rejects_an_incomplete_frozen_authority_view() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let scenario =
            build_frozen_authority_scenario(&mut block_store, &mut block_dag_storage).await;
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let partial = BTreeMap::from([(scenario.v1.clone(), scenario.left.block_hash.clone())]);
        let context = CertifiedConsensusContext::for_frozen_floor(
            &dag,
            scenario.genesis.block_hash.clone(),
            &partial,
        )
        .expect("partial context is representable for diagnostics");
        assert!(!context.has_complete_latest_message_slots());

        let error = Estimator::apply(i32::MAX, None)
            .tips_with_context(&dag, &scenario.genesis, &context)
            .await
            .expect_err("incomplete authority view must fail closed");
        assert!(matches!(
            error,
            shared::rust::store::key_value_store::KvStoreError::InvalidArgument(_)
        ));
    })
    .await
}

#[tokio::test]
async fn latest_message_outside_the_certified_floor_is_ineligible() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let scenario =
            build_frozen_authority_scenario(&mut block_store, &mut block_dag_storage).await;
        let outside = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![scenario.genesis.block_hash.clone()],
            &scenario.genesis,
            Some(scenario.v2.clone()),
            Some(scenario.genesis.body.state.bonds.clone()),
            Some(justifications!(
                scenario.v1 => scenario.genesis.block_hash,
                scenario.v2 => scenario.genesis.block_hash
            )),
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let exact = BTreeMap::from([
            (scenario.v1.clone(), scenario.left.block_hash.clone()),
            (scenario.v2.clone(), outside.block_hash.clone()),
        ]);
        let context = CertifiedConsensusContext::for_frozen_floor(
            &dag,
            scenario.common.block_hash.clone(),
            &exact,
        )
        .expect("certified floor context");

        assert_eq!(
            context.vote_projection().exclusions().get(&scenario.v2),
            Some(&VoteExclusion::DoesNotDescendFromFloor)
        );
        assert_eq!(
            context
                .vote_projection()
                .eligible_latest_messages()
                .get(&scenario.v1),
            Some(&scenario.left.block_hash)
        );
        assert!(!context
            .vote_projection()
            .eligible_latest_messages()
            .contains_key(&scenario.v2));
        assert_eq!(
            context
                .causal_parent_projection()
                .eligible_latest_messages()
                .get(&scenario.v2),
            Some(&outside.block_hash)
        );
        assert!(context
            .causal_parent_projection()
            .exclusions()
            .get(&scenario.v2)
            .is_none());
    })
    .await
}

prop_compose! {
    /// A random total order over the 3 validators, plus a random subset mask.
    /// `keys` yields a permutation via argsort; `mask` selects a sub-relation of
    /// the latest messages (defaulting to the full set when empty).
    fn perm_and_subset()(
        keys in prop::collection::vec(any::<u64>(), 3),
        mask in prop::collection::vec(any::<bool>(), 3),
    ) -> (Vec<u64>, Vec<bool>) {
        (keys, mask)
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, max_shrink_iters: 8, ..ProptestConfig::default() })]

    // SCORE MONOID (observable consequence): permuting the order of the FULL
    // latest-message set never changes the tips. Because the only order-sensitive
    // step in the pipeline is `build_scores_map`'s `+`-accumulation (a commutative
    // i64 monoid), order-invariant tips witness the monoid. Reference = canonical
    // order; comparison = argsort(keys) order.
    #[test]
    fn score_perm_invariant((keys, _mask) in perm_and_subset()) {
        RUNTIME.block_on(with_storage(|mut block_store, mut block_dag_storage| async move {
            let scenario = build_flipping_dag(&mut block_store, &mut block_dag_storage).await;
            let dag = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let estimator = Estimator::apply(i32::MAX, None);

            let reference: HashMap<Validator, BlockHash> =
                scenario.canonical_latest.iter().cloned().collect();
            let reference_tips =
                certified_fork_choice(&estimator, &dag, &scenario.genesis, reference)
                .await
                .expect("reference tips")
                .tips;

            // Stable argsort of [0,1,2] by `keys` -> a permutation of the pairs.
            let mut order: Vec<usize> = (0..scenario.canonical_latest.len()).collect();
            order.sort_by_key(|&i| keys[i]);
            let permuted: HashMap<Validator, BlockHash> = order
                .iter()
                .map(|&i| scenario.canonical_latest[i].clone())
                .collect();
            let permuted_tips =
                certified_fork_choice(&estimator, &dag, &scenario.genesis, permuted)
                .await
                .expect("permuted tips")
                .tips;

            prop_assert_eq!(
                permuted_tips, reference_tips,
                "permuting latest-message order changed the tips (score monoid violated)"
            );
            Ok::<(), TestCaseError>(())
        }))?;
    }

    // DETERMINISM over random SUBSETS: for any subset of the latest messages,
    // the tips are a pure function of the sub-relation — two independently-seeded
    // HashMaps built from the same (possibly permuted) subset yield identical
    // tips. This exercises the same order-independence over the (2^3 - 1) proper
    // sub-relations, not just the full set.
    #[test]
    fn fork_choice_determinism_over_subsets((keys, mask) in perm_and_subset()) {
        RUNTIME.block_on(with_storage(|mut block_store, mut block_dag_storage| async move {
            let scenario = build_flipping_dag(&mut block_store, &mut block_dag_storage).await;
            let dag = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let estimator = Estimator::apply(i32::MAX, None);

            // Selected sub-relation (default to the full set when the mask is empty).
            let mut selected: Vec<usize> = (0..scenario.canonical_latest.len())
                .filter(|&i| mask[i])
                .collect();
            if selected.is_empty() {
                selected = (0..scenario.canonical_latest.len()).collect();
            }

            // First build: canonical selection order.
            let first: HashMap<Validator, BlockHash> = selected
                .iter()
                .map(|&i| scenario.canonical_latest[i].clone())
                .collect();
            let first_tips =
                certified_fork_choice(&estimator, &dag, &scenario.genesis, first)
                .await
                .expect("first tips")
                .tips;

            // Second build: the SAME subset, permuted by argsort(keys), fresh map.
            let mut order = selected.clone();
            order.sort_by_key(|&i| keys[i]);
            let second: HashMap<Validator, BlockHash> = order
                .iter()
                .map(|&i| scenario.canonical_latest[i].clone())
                .collect();
            let second_tips =
                certified_fork_choice(&estimator, &dag, &scenario.genesis, second)
                .await
                .expect("second tips")
                .tips;

            prop_assert_eq!(
                first_tips, second_tips,
                "same latest-message subset produced different tips across rebuilds (nondeterminism)"
            );
            Ok::<(), TestCaseError>(())
        }))?;
    }
}
