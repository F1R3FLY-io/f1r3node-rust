// The clique edge's disagreement-walk contract, pinned in both directions.
//
// The walk over a peer's not-yet-justified messages must veto on genuine
// divergence and ONLY on genuine divergence. Exhaustive replay of the CI
// stall instances (tests/finalized_floor/oracle_stall_replay_spec.rs) fixed
// this file's scope: same-height rival contests and live fork-choice flaps
// are CORRECTLY refused (relaxing them is how a certificate becomes
// revertible — the ucc-i6 divergence), and the one real defect is the
// below-target conflation: a visited block BENEATH the target vetoed
// regardless of whether it was a rival prefix or the target's own settled
// ancestry. The deep-stale test here is that defect in miniature — the
// geometry that held CI instance i5's finality hostage for 851 s to the
// stalest window in a shrunken committee.

use std::collections::{BTreeMap, HashMap};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::safety::clique_oracle::{CliqueOracle, FtThreshold};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::validator::Validator;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::create_genesis_block;
use crate::helper::block_util::generate_validator;

const FTT: f32 = 0.33;

#[allow(clippy::too_many_arguments)]
fn propose(
    block_store: &mut KeyValueBlockStore,
    dag_storage: &mut IndexedBlockDagStorage,
    genesis: &BlockMessage,
    bonds: &[Bond],
    creator: &Validator,
    parents: Vec<BlockHash>,
    justifications: HashMap<Validator, BlockHash>,
) -> BlockMessage {
    crate::helper::block_generator::create_block(
        block_store,
        dag_storage,
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

/// Why a pair is or is not an edge, recomputed from the PUBLIC DAG API with
/// the same relations the oracle uses — including the two-sided height rule.
/// The tests assert the CAUSE of a missing edge, not only the symptom.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EdgeClass {
    Edge,
    MissingMutualJustification,
    /// The disagreement walk over b's messages that a has not yet justified
    /// visited this block, and it genuinely disagrees with the target: a
    /// rival estimate at/above the target's height, or a rival prefix below.
    DivergenceInWindow(BlockHash),
}

fn directed_veto(
    dag: &KeyValueDagRepresentation,
    target: &BlockHash,
    lm_b: &BlockHash,
    lm_a_j_b: &BlockHash,
) -> Option<BlockHash> {
    let target_height = dag
        .lookup_unsafe(target)
        .expect("lookup target")
        .block_number;
    let stopper = dag
        .self_justification(lm_a_j_b)
        .expect("self_justification")
        .unwrap_or_else(|| lm_a_j_b.clone());
    let mut current = dag.self_justification(lm_b).expect("self_justification");
    while let Some(hash) = current {
        if hash == stopper {
            break;
        }
        let visited_height = dag
            .lookup_unsafe(&hash)
            .expect("lookup visited")
            .block_number;
        let no_disagreement = if visited_height < target_height {
            dag.is_in_main_chain(&hash, target)
                .expect("is_in_main_chain")
        } else {
            dag.is_in_main_chain(target, &hash)
                .expect("is_in_main_chain")
        };
        if !no_disagreement {
            return Some(hash);
        }
        current = dag.self_justification(&hash).expect("self_justification");
    }
    None
}

fn edge_breakdown(
    dag: &KeyValueDagRepresentation,
    target: &BlockHash,
    snapshot: &BTreeMap<Validator, BlockHash>,
) -> Vec<(Validator, Validator, EdgeClass)> {
    let validators: Vec<&Validator> = snapshot.keys().collect();
    let justs: HashMap<&Validator, HashMap<Validator, BlockHash>> = validators
        .iter()
        .map(|v| {
            let meta = dag.lookup_unsafe(&snapshot[*v]).expect("lookup latest");
            let map = meta
                .justifications
                .iter()
                .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
                .collect();
            (*v, map)
        })
        .collect();

    let mut out = Vec::new();
    for i in 0..validators.len() {
        for j in (i + 1)..validators.len() {
            let (a, b) = (validators[i], validators[j]);
            let (Some(lm_a_j_b), Some(lm_b_j_a)) = (justs[a].get(b), justs[b].get(a)) else {
                out.push((a.clone(), b.clone(), EdgeClass::MissingMutualJustification));
                continue;
            };
            let veto = directed_veto(dag, target, &snapshot[b], lm_a_j_b)
                .or_else(|| directed_veto(dag, target, &snapshot[a], lm_b_j_a));
            out.push((
                a.clone(),
                b.clone(),
                veto.map_or(EdgeClass::Edge, EdgeClass::DivergenceInWindow),
            ));
        }
    }
    out
}

fn four_bonded() -> (Vec<Validator>, Vec<Bond>) {
    let validators: Vec<Validator> = (1..=4)
        .map(|i| generate_validator(Some(&format!("stall V{i}"))))
        .collect();
    let bonds = validators
        .iter()
        .map(|v| Bond {
            validator: v.clone(),
            stake: 100,
        })
        .collect();
    (validators, bonds)
}

async fn certified(
    dag: &KeyValueDagRepresentation,
    target: &BlockHash,
    snapshot: &BTreeMap<Validator, BlockHash>,
) -> bool {
    CliqueOracle::ft_witnessed_exact(target, dag, snapshot, FtThreshold::from_f32_lossy(FTT))
        .await
        .expect("ft_witnessed_exact")
}

/// The below-target conflation, in miniature (the CI-scale replay red is
/// `i5_withdrawn_era_target_certifies_despite_ancestor_prefix_windows`).
///
/// One single chain, no contest anywhere, ever. Every validator's final
/// message agrees on a mid-chain target, but their justifications of EACH
/// OTHER are deeply stale, so every pair's walk window reaches below the
/// target through the target's own settled ancestry. Ignorance of ancestry
/// is not disagreement: certification must not be hostage to the stalest
/// window in the committee.
#[tokio::test]
async fn a_deeply_stale_window_on_a_single_chain_must_not_veto() {
    with_storage(|mut block_store, mut dag_storage| async move {
        let (validators, bonds) = four_bonded();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut dag_storage,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        // Round 0: one block per validator on a single chain, everyone
        // justifying genesis.
        let gj: HashMap<Validator, BlockHash> = validators
            .iter()
            .map(|v| (v.clone(), genesis.block_hash.clone()))
            .collect();
        let mut tip = genesis.block_hash.clone();
        let mut round0: HashMap<Validator, BlockHash> = HashMap::new();
        let mut own_prev: HashMap<Validator, BlockHash> = HashMap::new();
        for v in &validators {
            let n = propose(
                &mut block_store,
                &mut dag_storage,
                &genesis,
                &bonds,
                v,
                vec![tip.clone()],
                gj.clone(),
            );
            tip = n.block_hash.clone();
            round0.insert(v.clone(), n.block_hash.clone());
            own_prev.insert(v.clone(), n.block_hash);
        }

        // Rounds 1..4 continue the single chain, but every proposal's
        // justifications name the OTHERS' ROUND-0 blocks (deeply stale) and
        // only the proposer's own previous block fresh — so each pair's walk
        // window spans the other's whole chain segment back to round 0.
        let mut target: Option<BlockHash> = None;
        let mut snapshot: BTreeMap<Validator, BlockHash> = BTreeMap::new();
        for round in 1..=4 {
            for v in &validators {
                let mut justs: HashMap<Validator, BlockHash> = round0.clone();
                justs.insert(v.clone(), own_prev[v].clone());
                let n = propose(
                    &mut block_store,
                    &mut dag_storage,
                    &genesis,
                    &bonds,
                    v,
                    vec![tip.clone()],
                    justs,
                );
                tip = n.block_hash.clone();
                own_prev.insert(v.clone(), n.block_hash.clone());
                if round == 1 && target.is_none() {
                    target = Some(n.block_hash.clone());
                }
                snapshot.insert(v.clone(), n.block_hash);
            }
        }
        let target = target.expect("target staged");

        let dag = dag_storage.get_representation().expect("dag");
        for lm in snapshot.values() {
            assert!(
                dag.is_in_main_chain(&target, lm).expect("main chain"),
                "staging: a single chain means unanimous agreement on the target"
            );
        }
        assert!(
            certified(&dag, &target, &snapshot).await,
            "a single chain with unanimous agreement failed to certify: the \
             walk vetoed on the target's own below-target ancestry; \
             breakdown: {:?}",
            edge_breakdown(&dag, &target, &snapshot)
        );
    })
    .await
}

/// GUARD: a live same-height contest is correctly refused — and must stay
/// refused by any walk change. One round of concurrent proposal plus one
/// convergence round leaves every validator agreeing on the winner while
/// each pair's window still holds the other's rival sibling AT the target's
/// height. Certifying here is what makes a certificate revertible by an
/// honest flip (the ucc-i6 divergence): the veto is load-bearing. The heal
/// path is BUILDING PAST, not relaxation: one witnessing round later the
/// stoppers sit above the contest and the winner certifies with a full
/// edge set.
#[tokio::test]
async fn a_live_sibling_contest_is_refused_then_heals_by_witnessing() {
    with_storage(|mut block_store, mut dag_storage| async move {
        let (validators, bonds) = four_bonded();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut dag_storage,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let gj: HashMap<Validator, BlockHash> = validators
            .iter()
            .map(|v| (v.clone(), genesis.block_hash.clone()))
            .collect();
        let b1 = propose(
            &mut block_store,
            &mut dag_storage,
            &genesis,
            &bonds,
            &validators[0],
            vec![genesis.block_hash.clone()],
            gj.clone(),
        );

        // Contested round: every validator proposes its own child of b1 —
        // four same-height siblings. Normal behavior under load.
        let bj: HashMap<Validator, BlockHash> = validators
            .iter()
            .map(|v| (v.clone(), b1.block_hash.clone()))
            .collect();
        let siblings: Vec<BlockMessage> = validators
            .iter()
            .map(|v| {
                propose(
                    &mut block_store,
                    &mut dag_storage,
                    &genesis,
                    &bonds,
                    v,
                    vec![b1.block_hash.clone()],
                    bj.clone(),
                )
            })
            .collect();
        let target = siblings[0].block_hash.clone(); // the winner

        // Convergence round: everyone builds on the winner, justifying
        // everyone's contested-round block (one round behind the tips).
        let cj: HashMap<Validator, BlockHash> = validators
            .iter()
            .zip(&siblings)
            .map(|(v, s)| (v.clone(), s.block_hash.clone()))
            .collect();
        let mut latest: HashMap<Validator, BlockHash> = HashMap::new();
        let converge: BTreeMap<Validator, BlockHash> = validators
            .iter()
            .map(|v| {
                let n = propose(
                    &mut block_store,
                    &mut dag_storage,
                    &genesis,
                    &bonds,
                    v,
                    vec![target.clone()],
                    cj.clone(),
                );
                latest.insert(v.clone(), n.block_hash.clone());
                (v.clone(), n.block_hash)
            })
            .collect();

        {
            let dag = dag_storage.get_representation().expect("dag");
            for (a, b, class) in edge_breakdown(&dag, &target, &converge) {
                assert!(
                    matches!(class, EdgeClass::DivergenceInWindow(_)),
                    "staging: every pair must be vetoed by a rival sibling in \
                     the window; ({a:?},{b:?}) classified {class:?}"
                );
            }
            assert!(
                !certified(&dag, &target, &converge).await,
                "a contest whose rival siblings are still inside the walk \
                 windows must NOT certify — this refusal is the \
                 transient-flip guard"
            );
        }

        // HEAL: one more witnessing round — everyone justifies everyone's
        // convergence block, so every stopper moves above the contest and
        // the windows hold only winner-chain blocks.
        let wj: HashMap<Validator, BlockHash> = latest.clone();
        let mut chain_tip = converge[&validators[0]].clone();
        let witnessed: BTreeMap<Validator, BlockHash> = validators
            .iter()
            .map(|v| {
                let n = propose(
                    &mut block_store,
                    &mut dag_storage,
                    &genesis,
                    &bonds,
                    v,
                    vec![chain_tip.clone()],
                    wj.clone(),
                );
                chain_tip = n.block_hash.clone();
                (v.clone(), n.block_hash)
            })
            .collect();
        let dag = dag_storage.get_representation().expect("dag");
        let breakdown = edge_breakdown(&dag, &target, &witnessed);
        assert!(
            breakdown.iter().all(|(_, _, c)| *c == EdgeClass::Edge),
            "staging: after a witnessing round every pair must be an edge; \
             breakdown: {breakdown:?}"
        );
        assert!(
            certified(&dag, &target, &witnessed).await,
            "once every window has moved past the contest, the winner must \
             certify"
        );
    })
    .await
}

/// GUARD: a live fork-choice flap is correctly refused — and must stay
/// refused by any walk change. Two validators keep extending a rival chain
/// before returning to the winner chain; every snapshot shows unanimous
/// agreement on the winner, but each window holds a recent rival-chain
/// block at or above the target's height. That is a genuinely revocable
/// estimate, exactly what a certificate must not contain.
#[tokio::test]
async fn a_live_flap_onto_a_rival_chain_is_correctly_refused() {
    with_storage(|mut block_store, mut dag_storage| async move {
        let (validators, bonds) = four_bonded();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut dag_storage,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let gj: HashMap<Validator, BlockHash> = validators
            .iter()
            .map(|v| (v.clone(), genesis.block_hash.clone()))
            .collect();
        let b1 = propose(
            &mut block_store,
            &mut dag_storage,
            &genesis,
            &bonds,
            &validators[0],
            vec![genesis.block_hash.clone()],
            gj.clone(),
        );

        let bj: HashMap<Validator, BlockHash> = validators
            .iter()
            .map(|v| (v.clone(), b1.block_hash.clone()))
            .collect();
        let winner = propose(
            &mut block_store,
            &mut dag_storage,
            &genesis,
            &bonds,
            &validators[0],
            vec![b1.block_hash.clone()],
            bj.clone(),
        );
        let rival = propose(
            &mut block_store,
            &mut dag_storage,
            &genesis,
            &bonds,
            &validators[1],
            vec![b1.block_hash.clone()],
            bj.clone(),
        );
        let target = winner.block_hash.clone();

        let mut latest: HashMap<Validator, BlockHash> = HashMap::from([
            (validators[0].clone(), winner.block_hash.clone()),
            (validators[1].clone(), rival.block_hash.clone()),
            (validators[2].clone(), b1.block_hash.clone()),
            (validators[3].clone(), b1.block_hash.clone()),
        ]);
        let mut tip_w = winner.block_hash.clone();
        let mut tip_r = rival.block_hash.clone();

        let mut snapshot: BTreeMap<Validator, BlockHash> = BTreeMap::new();
        for _round in 0..4 {
            let just: HashMap<Validator, BlockHash> = latest.clone();
            // Flap: v3, v4 extend the rival chain (spine through the rival,
            // NOT through the winner).
            for v in [&validators[2], &validators[3]] {
                let excursion = propose(
                    &mut block_store,
                    &mut dag_storage,
                    &genesis,
                    &bonds,
                    v,
                    vec![tip_r.clone()],
                    just.clone(),
                );
                tip_r = excursion.block_hash.clone();
                latest.insert(v.clone(), excursion.block_hash);
            }
            // Return: everyone builds on the winner chain.
            let just: HashMap<Validator, BlockHash> = latest.clone();
            snapshot = validators
                .iter()
                .map(|v| {
                    let n = propose(
                        &mut block_store,
                        &mut dag_storage,
                        &genesis,
                        &bonds,
                        v,
                        vec![tip_w.clone()],
                        just.clone(),
                    );
                    if v == &validators[0] {
                        tip_w = n.block_hash.clone();
                    }
                    latest.insert(v.clone(), n.block_hash.clone());
                    (v.clone(), n.block_hash)
                })
                .collect();
        }

        let dag = dag_storage.get_representation().expect("dag");
        for lm in snapshot.values() {
            assert!(
                dag.is_in_main_chain(&target, lm).expect("main chain"),
                "staging: the final snapshot must agree unanimously on the winner"
            );
        }
        assert!(
            !certified(&dag, &target, &snapshot).await,
            "a snapshot taken mid-flap must NOT certify: the windows hold \
             live rival-chain estimates, and a certificate here is \
             revertible by an honest return to the rival"
        );
    })
    .await
}

/// Control: staleness alone is harmless. Justifications run one round
/// behind, but with a single proposer per height there is never an
/// off-spine block in any window — the edge set is complete and the
/// candidate certifies.
#[tokio::test]
async fn stale_justifications_alone_do_not_break_the_edge_set() {
    with_storage(|mut block_store, mut dag_storage| async move {
        let (validators, bonds) = four_bonded();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut dag_storage,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let mut latest: HashMap<Validator, BlockHash> = validators
            .iter()
            .map(|v| (v.clone(), genesis.block_hash.clone()))
            .collect();
        let mut tip = genesis.block_hash.clone();
        let mut target: Option<BlockHash> = None;
        for round in 0..4 {
            let just = latest.clone();
            for v in &validators {
                let n = propose(
                    &mut block_store,
                    &mut dag_storage,
                    &genesis,
                    &bonds,
                    v,
                    vec![tip.clone()],
                    just.clone(),
                );
                tip = n.block_hash.clone();
                latest.insert(v.clone(), n.block_hash.clone());
                if round == 0 && target.is_none() {
                    target = Some(n.block_hash);
                }
            }
        }
        let target = target.expect("target staged");
        let snapshot: BTreeMap<Validator, BlockHash> =
            latest.iter().map(|(v, h)| (v.clone(), h.clone())).collect();

        let dag = dag_storage.get_representation().expect("dag");
        assert!(
            certified(&dag, &target, &snapshot).await,
            "control: a single-proposer chain with lagging justifications must certify"
        );
    })
    .await
}

/// Control: a rival sibling alone is harmless once every justification has
/// caught up past the contest — the sibling sits below every walk stopper,
/// no window contains it, and the candidate certifies.
#[tokio::test]
async fn a_rival_sibling_fully_built_past_does_not_break_the_edge_set() {
    with_storage(|mut block_store, mut dag_storage| async move {
        let (validators, bonds) = four_bonded();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut dag_storage,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let gj: HashMap<Validator, BlockHash> = validators
            .iter()
            .map(|v| (v.clone(), genesis.block_hash.clone()))
            .collect();
        let b1 = propose(
            &mut block_store,
            &mut dag_storage,
            &genesis,
            &bonds,
            &validators[0],
            vec![genesis.block_hash.clone()],
            gj.clone(),
        );
        let bj: HashMap<Validator, BlockHash> = validators
            .iter()
            .map(|v| (v.clone(), b1.block_hash.clone()))
            .collect();
        let winner = propose(
            &mut block_store,
            &mut dag_storage,
            &genesis,
            &bonds,
            &validators[0],
            vec![b1.block_hash.clone()],
            bj.clone(),
        );
        let _rival = propose(
            &mut block_store,
            &mut dag_storage,
            &genesis,
            &bonds,
            &validators[1],
            vec![b1.block_hash.clone()],
            bj.clone(),
        );
        let target = winner.block_hash.clone();

        // Two full rounds on the winner chain with FULLY CAUGHT-UP
        // justifications: after round 2, every validator's view of every
        // other is that validator's round-1 winner-chain block, so every
        // walk stopper sits above the contested height.
        let mut latest: HashMap<Validator, BlockHash> = HashMap::from([
            (validators[0].clone(), winner.block_hash.clone()),
            (validators[1].clone(), _rival.block_hash.clone()),
            (validators[2].clone(), b1.block_hash.clone()),
            (validators[3].clone(), b1.block_hash.clone()),
        ]);
        let mut tip = target.clone();
        let mut snapshot: BTreeMap<Validator, BlockHash> = BTreeMap::new();
        for _round in 0..2 {
            let just = latest.clone();
            snapshot = validators
                .iter()
                .map(|v| {
                    let n = propose(
                        &mut block_store,
                        &mut dag_storage,
                        &genesis,
                        &bonds,
                        v,
                        vec![tip.clone()],
                        just.clone(),
                    );
                    if v == &validators[0] {
                        tip = n.block_hash.clone();
                    }
                    latest.insert(v.clone(), n.block_hash.clone());
                    (v.clone(), n.block_hash)
                })
                .collect();
        }

        let dag = dag_storage.get_representation().expect("dag");
        let breakdown = edge_breakdown(&dag, &target, &snapshot);
        let vetoed = breakdown
            .iter()
            .filter(|(_, _, c)| matches!(c, EdgeClass::DivergenceInWindow(_)))
            .count();

        assert!(
            certified(&dag, &target, &snapshot).await,
            "control: a fully built-past rival sibling must not block \
             certification ({vetoed} pairs still vetoed)"
        );
    })
    .await
}
