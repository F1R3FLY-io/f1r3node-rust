// GHOST scoring over a MERGED sibling race — the one DAG shape the rest of the
// fork-choice suite never builds. Every other fixture here is single-parent
// (`prop_estimator_determinism`, `prop_ghost_argmax`, `prop_bound` all pass a
// one-element parent list), and on a single-parent DAG "score every DAG
// ancestor" and "score every main-parent ancestor" are the same function. This
// file is the multi-parent case where they differ.

use std::collections::HashMap;

use casper::rust::estimator::Estimator;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::Bond;
use models::rust::validator::Validator;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{create_block, create_genesis_block};
use crate::helper::block_util::generate_validator;

/// The specimen shape, reduced to its scoring core:
///
/// ```text
///        genesis
///        /     \
///     S1(v1)  S2(v2)          same-height siblings
///       |\    /|
///       | \  / |
///       |  \/  |
///     M1(v1) M2(v2) M3(v3)    every validator MERGES both siblings
/// ```
///
/// `M1` and `M3` take `S1` as main parent, `M2` takes `S2`. Latest messages are
/// `{v1: M1, v2: M2, v3: M3}`, all three stakes equal.
///
/// `build_scores_map` credits a validator's weight to every block reached by
/// walking `meta.parents` — ALL parents (`estimator.rs:241`). Every latest
/// message DAG-descends from both siblings, so both accumulate the full 300 and
/// the fork choice between them is a permanent tie broken only by hash order.
/// That saturation is what made spine choice flippable between two sound
/// certificates while the state lineage followed the `merge_base` chain.
///
/// Scoring the MAIN-PARENT chain instead separates them — 200 for `S1`
/// (v1 and v3), 100 for `S2` (v2) — because a block has exactly one main
/// parent, so a validator's weight flows up exactly one path and same-height
/// siblings are mutually exclusive by construction.
///
/// Written red against all-parents scoring, where it failed at `300 == 300`.
#[tokio::test]
async fn merged_siblings_must_not_score_equal() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("Merged Sibling V1"));
        let v2 = generate_validator(Some("Merged Sibling V2"));
        let v3 = generate_validator(Some("Merged Sibling V3"));
        let bonds: Vec<Bond> = [&v1, &v2, &v3]
            .iter()
            .map(|v| Bond {
                validator: (*v).clone(),
                stake: 100,
            })
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

        let mk = |creator: &Validator,
                  parents: Vec<BlockHash>,
                  justs: HashMap<Validator, BlockHash>,
                  block_store: &mut _,
                  block_dag_storage: &mut _| {
            create_block(
                block_store,
                block_dag_storage,
                parents,
                &genesis,
                Some(creator.clone()),
                Some(bonds.clone()),
                Some(justs),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
        };

        let g = genesis.block_hash.clone();
        let gj: HashMap<Validator, BlockHash> = [&v1, &v2, &v3]
            .iter()
            .map(|v| ((*v).clone(), g.clone()))
            .collect();

        let s1 = mk(
            &v1,
            vec![g.clone()],
            gj.clone(),
            &mut block_store,
            &mut block_dag_storage,
        );
        let s2 = mk(
            &v2,
            vec![g.clone()],
            gj.clone(),
            &mut block_store,
            &mut block_dag_storage,
        );

        // Every validator merges BOTH siblings; the main parent (parents[0])
        // is S1 for v1 and v3, S2 for v2.
        let mj: HashMap<Validator, BlockHash> = HashMap::from([
            (v1.clone(), s1.block_hash.clone()),
            (v2.clone(), s2.block_hash.clone()),
            (v3.clone(), g.clone()),
        ]);
        let m1 = mk(
            &v1,
            vec![s1.block_hash.clone(), s2.block_hash.clone()],
            mj.clone(),
            &mut block_store,
            &mut block_dag_storage,
        );
        let m2 = mk(
            &v2,
            vec![s2.block_hash.clone(), s1.block_hash.clone()],
            mj.clone(),
            &mut block_store,
            &mut block_dag_storage,
        );
        let m3 = mk(
            &v3,
            vec![s1.block_hash.clone(), s2.block_hash.clone()],
            mj.clone(),
            &mut block_store,
            &mut block_dag_storage,
        );

        let mut dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let latest: HashMap<Validator, BlockHash> = HashMap::from([
            (v1.clone(), m1.block_hash.clone()),
            (v2.clone(), m2.block_hash.clone()),
            (v3.clone(), m3.block_hash.clone()),
        ]);

        let fork_choice = Estimator::apply(i32::MAX, None)
            .tips_with_latest_messages(&mut dag, &genesis, latest)
            .await
            .expect("tips");

        // Scoring the spine narrows what the GHOST descent can walk: it follows
        // MAIN-parent children, so a scored block is reachable only through the
        // chain that scored it. If the LCA lay on no validator's spine the
        // descent would stall and return the LCA itself as the head — a stale
        // block, and a liveness regression rather than a scoring one. Pin that
        // the head is an actual tip.
        let tips = &fork_choice.tips;
        let head = tips.first().expect("fork choice must return a head");
        assert!(
            [&m1.block_hash, &m2.block_hash, &m3.block_hash].contains(&head),
            "the GHOST head must be one of the merge-layer tips, not a block the \
             descent stalled on (head={head:?})"
        );

        let score = |hash: &BlockHash| fork_choice.scores.get(hash).copied().unwrap_or(0);
        let s1_score = score(&s1.block_hash);
        let s2_score = score(&s2.block_hash);

        // Staging integrity: both siblings must actually be scored, or the
        // assertion below would pass vacuously on two absent entries.
        assert!(
            s1_score > 0 && s2_score > 0,
            "staging: both siblings must be reachable from the latest messages \
             (S1={s1_score}, S2={s2_score})"
        );

        assert_ne!(
            s1_score, s2_score,
            "merged same-height siblings score EQUAL, so fork choice between them is \
             decided by a hash tie-break rather than by validator support. Two of the \
             three validators main-parent S1 and one main-parents S2, so a scoring rule \
             that reflects chain choice must separate them (200 vs 100). Scoring every \
             DAG ancestor saturates both to the full 300 and leaves the spine free to \
             flip between two sound certificates"
        );
        assert_eq!(
            s1_score, 200,
            "S1 is the main parent of v1's and v3's latest messages"
        );
        assert_eq!(
            s2_score, 100,
            "S2 is the main parent of v2's latest message only"
        );
    })
    .await
}
