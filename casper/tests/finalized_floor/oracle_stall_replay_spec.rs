// Exact oracle replays of two CI finality-stall instances, from committed
// sub-DAG fixtures distilled out of the shards' own logs (structure from
// `Computed parents post state` lines, justification maps and verdict tuples
// from `ft_witnessed_exact verdict` lines):
//
//   i1 — bonding_validators, run 32284324989: the floor pinned at h23 for
//        239 s while agreement stayed unanimous; every certification failure
//        in the logged windows is a same-height rival-contest veto.
//   i5 — validator_lifecycle, run 32397055615: the 851 s stall. The committee
//        shrinks across the fixture (V5's 200 then a 300-stake member depart);
//        snapshots still track the departed validators while their stake has
//        left the bonds map.
//
// FIDELITY pins (green): rebuilding each instance's sub-DAG block-for-block
// (senders, heights, main parents, justification maps, per-era bonds) and
// evaluating the real oracle at the logged snapshots must reproduce the
// logged `(agreeing, max_clique_weight, total_stake, decision)` tuples
// exactly. Any change to agreement, the disagreement walk, or the clique
// solver that shifts these tuples fails here first.
//
// REFINED-WALK red (i5): 37 of h146's logged false verdicts fail ONLY on
// below-target ancestor-prefix vetoes — blocks on the target's own main
// chain, visited in a departed validator's stale window. Ignorance of a
// target's own ancestry is not disagreement with the target, so these
// snapshots must certify; today the walk conflates the two and they do not.
// (i1 carries NO such sample — all 264 of its false verdicts are rival
// vetoes, which must KEEP vetoing — so the refinement's scope is pinned by
// both fixtures together.)
//
// Fixtures: casper/tests/resources/stall_fixtures/{i1,i5}.json, emitted by
// the system-integration classifier's distiller, which validates that the
// distilled structure replays identically to the full reconstructed shard
// before emitting.

use std::collections::{BTreeMap, HashMap};

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::safety::clique_oracle::{CliqueOracle, FtThreshold};
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, Justification};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use serde::Deserialize;

use crate::helper::block_util::generate_validator;

#[derive(Deserialize)]
struct Fixture {
    instance: String,
    ftt_ppm: i64,
    max_height: i64,
    eras: Vec<BTreeMap<String, i64>>,
    blocks: Vec<FxBlock>,
    samples: Vec<FxSample>,
}

#[derive(Deserialize)]
struct FxBlock {
    id: String,
    sender: String,
    height: i64,
    main_parent: Option<String>,
    extra_filler_parent: bool,
    era: usize,
    justifications: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct FxSample {
    kind: String,
    target: String,
    snapshot: BTreeMap<String, String>,
    agreeing: i64,
    max_clique: i64,
    total: i64,
    decision: bool,
    era: usize,
}

/// The rebuilt instance: id -> real hash, v8 -> validator identity, per-era
/// bonds, and the live DAG representation.
struct Rebuilt {
    dag: block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
    hash_of: HashMap<String, BlockHash>,
    validator_of: HashMap<String, Validator>,
    era_weights: Vec<HashMap<Validator, i64>>,
}

async fn rebuild(fx: &Fixture) -> Rebuilt {
    let mut kvm = InMemoryStoreManager::new();
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
        .await
        .expect("dag storage");

    let mut validator_of: HashMap<String, Validator> = HashMap::new();
    let mut vids: Vec<String> = fx
        .eras
        .iter()
        .flat_map(|e| e.keys().cloned())
        .chain(fx.blocks.iter().map(|b| b.sender.clone()))
        .chain(fx.samples.iter().flat_map(|s| s.snapshot.keys().cloned()))
        .collect();
    vids.sort();
    vids.dedup();
    for v in &vids {
        validator_of.insert(
            v.clone(),
            generate_validator(Some(&format!("{} {}", fx.instance, v))),
        );
    }
    let filler = generate_validator(Some(&format!("{} filler", fx.instance)));

    let era_bonds: Vec<Vec<Bond>> = fx
        .eras
        .iter()
        .map(|e| {
            e.iter()
                .map(|(v, stake)| Bond {
                    validator: validator_of[v].clone(),
                    stake: *stake,
                })
                .collect()
        })
        .collect();
    let era_weights: Vec<HashMap<Validator, i64>> = fx
        .eras
        .iter()
        .map(|e| {
            e.iter()
                .map(|(v, stake)| (validator_of[v].clone(), *stake))
                .collect()
        })
        .collect();

    let mk = |number: i64,
              sender: Validator,
              parents: Vec<BlockHash>,
              justifications: Vec<Justification>,
              bonds: Vec<Bond>| {
        block_implicits::get_random_block(
            Some(number),
            Some(number as i32),
            None,
            None,
            Some(sender),
            None,
            Some(number),
            Some(parents),
            Some(justifications),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(bonds),
            Some("root".to_string()),
            None,
        )
    };
    let store = |block: &BlockMessage, mode: InsertMode| {
        block_store.put_block_message(block).expect("store block");
        dag_storage.insert(block, mode).expect("insert block");
    };

    let genesis = mk(
        0,
        filler.clone(),
        Vec::new(),
        Vec::new(),
        era_bonds[0].clone(),
    );
    store(&genesis, InsertMode::Approved);

    // Filler spine: one block per height up to the fixture's max, so every
    // fixture block sits at exactly its CI height and spine walks that leave
    // the distilled window descend a real chain to genesis.
    let mut filler_at: Vec<BlockHash> = vec![genesis.block_hash.clone()];
    for h in 1..=fx.max_height {
        let block = mk(
            h,
            filler.clone(),
            vec![filler_at[(h - 1) as usize].clone()],
            Vec::new(),
            era_bonds[0].clone(),
        );
        store(&block, InsertMode::Normal);
        filler_at.push(block.block_hash);
    }

    let mut hash_of: HashMap<String, BlockHash> = HashMap::new();
    for b in &fx.blocks {
        let mut parents: Vec<BlockHash> = Vec::with_capacity(2);
        match &b.main_parent {
            Some(mp) => parents.push(hash_of[mp].clone()),
            None => parents.push(filler_at[(b.height - 1) as usize].clone()),
        }
        if b.extra_filler_parent {
            parents.push(filler_at[(b.height - 1) as usize].clone());
        }
        let justifications: Vec<Justification> = b
            .justifications
            .iter()
            .map(|(v, id)| Justification {
                validator: validator_of[v].clone(),
                latest_block_hash: hash_of[id].clone(),
            })
            .collect();
        let block = mk(
            b.height,
            validator_of[&b.sender].clone(),
            parents,
            justifications,
            era_bonds[b.era].clone(),
        );
        store(&block, InsertMode::Normal);
        hash_of.insert(b.id.clone(), block.block_hash);
    }

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    Rebuilt {
        dag,
        hash_of,
        validator_of,
        era_weights,
    }
}

/// Replays one logged verdict and returns the reproduced tuple
/// `(agreeing, max_clique, total, decision)`.
async fn replay_sample(
    rebuilt: &Rebuilt,
    ftt_ppm: i64,
    sample: &FxSample,
) -> (i64, i64, i64, bool) {
    let target = rebuilt.hash_of[&sample.target].clone();
    let snapshot: BTreeMap<Validator, BlockHash> = sample
        .snapshot
        .iter()
        .map(|(v, id)| (rebuilt.validator_of[v].clone(), rebuilt.hash_of[id].clone()))
        .collect();

    let weight_map = CliqueOracle::get_corresponding_weight_map(&target, &rebuilt.dag)
        .await
        .expect("weight map");
    let expected_weights = &rebuilt.era_weights[sample.era];
    assert_eq!(
        &weight_map, expected_weights,
        "staging: the rebuilt target's committee must be the fixture era"
    );
    let total: i64 = weight_map.values().sum();

    // Agreement, by the oracle's own definition: the target is on the
    // main-parent spine of the validator's latest message.
    let mut agreeing_map: HashMap<Validator, i64> = HashMap::new();
    for (v, stake) in &weight_map {
        if let Some(lm) = snapshot.get(v) {
            if rebuilt
                .dag
                .is_in_main_chain(&target, lm)
                .expect("is_in_main_chain")
            {
                agreeing_map.insert(v.clone(), *stake);
            }
        }
    }
    let agreeing: i64 = agreeing_map.values().sum();

    // (2q − S)/S recovers the max clique weight exactly for these stakes.
    let ft =
        CliqueOracle::compute_output(&target, &weight_map, &agreeing_map, &rebuilt.dag, &snapshot)
            .await
            .expect("compute_output");
    let max_clique = (((ft as f64) * (total as f64) + total as f64) / 2.0).round() as i64;

    let decision = CliqueOracle::ft_witnessed_exact(
        &target,
        &rebuilt.dag,
        &snapshot,
        FtThreshold::from_ppm(ftt_ppm),
        false,
    )
    .await
    .expect("ft_witnessed_exact");

    (agreeing, max_clique, total, decision)
}

fn load(json: &str) -> Fixture { serde_json::from_str(json).expect("fixture parses") }

async fn assert_fidelity(fx: &Fixture) {
    let rebuilt = rebuild(fx).await;
    for sample in fx.samples.iter().filter(|s| s.kind == "fidelity") {
        let (agreeing, max_clique, total, decision) =
            replay_sample(&rebuilt, fx.ftt_ppm, sample).await;
        assert_eq!(
            (agreeing, max_clique, total, decision),
            (
                sample.agreeing,
                sample.max_clique,
                sample.total,
                sample.decision
            ),
            "{}: replay of target {} diverged from the CI-logged verdict",
            fx.instance,
            sample.target
        );
    }
}

#[tokio::test]
async fn i1_replay_reproduces_the_ci_verdicts_exactly() {
    crate::init_logger();
    assert_fidelity(&load(include_str!("../resources/stall_fixtures/i1.json"))).await;
}

#[tokio::test]
async fn i5_replay_reproduces_the_ci_verdicts_exactly() {
    crate::init_logger();
    assert_fidelity(&load(include_str!("../resources/stall_fixtures/i5.json"))).await;
}

/// RED until the disagreement walk separates ancestor-prefix from rival
/// prefix below the target: in these logged snapshots every missing edge is
/// vetoed by a block on the TARGET'S OWN main chain — settled ancestry a
/// departed-era window has not caught up past, not a divergent estimate. The
/// live committee's agreement meets the threshold the moment those vetoes
/// stop counting; the shard stalled 851 s on exactly this arithmetic.
#[tokio::test]
async fn i5_withdrawn_era_target_certifies_despite_ancestor_prefix_windows() {
    crate::init_logger();
    let fx = load(include_str!("../resources/stall_fixtures/i5.json"));
    let rebuilt = rebuild(&fx).await;
    let reds: Vec<&FxSample> = fx
        .samples
        .iter()
        .filter(|s| s.kind == "refined_red")
        .collect();
    assert!(
        !reds.is_empty(),
        "staging: the fixture must carry a refined_red sample"
    );
    for sample in reds {
        let (agreeing, max_clique, total, decision) =
            replay_sample(&rebuilt, fx.ftt_ppm, sample).await;
        assert!(
            decision,
            "target {} must certify: agreement {}/{} meets the threshold and \
             every veto in the logged window is a below-target block on the \
             target's own main chain (logged clique {}, replayed {})",
            sample.target, agreeing, total, sample.max_clique, max_clique
        );
    }
}
