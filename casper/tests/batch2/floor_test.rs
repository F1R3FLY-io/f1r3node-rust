// Tests for the justification-derived finalized floor (casper/src/rust/finality/floor.rs).

use std::collections::{BTreeMap, HashMap};

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::finality::floor::{finalized_floor, floor_of_block};
use casper::rust::safety::clique_oracle::CliqueOracle;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::validator::Validator;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::create_genesis_block;
use crate::helper::block_util::generate_validator;

const FT_THRESHOLD: f32 = 0.1;

fn create_block<'a>(
    bonds: &'a [Bond],
    genesis: &'a BlockMessage,
    creator: &'a Validator,
) -> impl Fn(
    &mut KeyValueBlockStore,
    &mut IndexedBlockDagStorage,
    &BlockMessage,
    &HashMap<&Validator, &BlockMessage>,
) -> BlockMessage
       + 'a {
    move |block_store, block_dag_storage, parent, justifications| {
        let justifications_map: HashMap<Validator, BlockHash> = justifications
            .iter()
            .map(|(validator, block_message)| {
                ((*validator).clone(), block_message.block_hash.clone())
            })
            .collect();

        crate::helper::block_generator::create_block(
            block_store,
            block_dag_storage,
            vec![parent.block_hash.clone()],
            genesis,
            Some(creator.clone()),
            Some(bonds.to_vec()),
            Some(justifications_map),
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }
}

fn snapshot(entries: &[(&Validator, &BlockMessage)]) -> BTreeMap<Validator, BlockHash> {
    entries
        .iter()
        .map(|(validator, block)| ((*validator).clone(), block.block_hash.clone()))
        .collect()
}

fn three_bonds(v1: &Validator, v2: &Validator, v3: &Validator) -> Vec<Bond> {
    [v1, v2, v3]
        .iter()
        .map(|v| Bond {
            validator: (*v).clone(),
            stake: 100,
        })
        .collect()
}

/// On a young DAG where no block has supermajority witness yet, the floor
/// degenerates to genesis (the walk's terminal case).
#[tokio::test]
async fn floor_is_genesis_on_young_dag() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("Floor Young V1"));
        let v2 = generate_validator(Some("Floor Young V2"));
        let v3 = generate_validator(Some("Floor Young V3"));
        let bonds = three_bonds(&v1, &v2, &v3);

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
        let creator1 = create_block(&bonds, &genesis, &v1);

        let gj = HashMap::from([(&v1, &genesis), (&v2, &genesis), (&v3, &genesis)]);
        let b1 = creator1(&mut block_store, &mut block_dag_storage, &genesis, &gj);

        let dag = block_dag_storage.get_representation();
        let latest_messages = snapshot(&[(&v1, &b1), (&v2, &genesis), (&v3, &genesis)]);

        let floor = finalized_floor(
            &dag,
            std::slice::from_ref(&b1.block_hash),
            &latest_messages,
            FT_THRESHOLD,
        )
        .await
        .expect("floor must exist: genesis terminates every walk");

        assert_eq!(
            floor.hash, genesis.block_hash,
            "young DAG floor must be genesis"
        );
    })
    .await;
}

/// On a converged linear chain the floor must equal the brute-force frontier:
/// the highest main-chain ancestor of the parent with ft_witnessed >= threshold.
#[tokio::test]
async fn floor_matches_bruteforce_frontier_on_converged_chain() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("Floor Chain V1"));
        let v2 = generate_validator(Some("Floor Chain V2"));
        let v3 = generate_validator(Some("Floor Chain V3"));
        let bonds = three_bonds(&v1, &v2, &v3);

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
        let creator1 = create_block(&bonds, &genesis, &v1);
        let creator2 = create_block(&bonds, &genesis, &v2);
        let creator3 = create_block(&bonds, &genesis, &v3);

        let gj = HashMap::from([(&v1, &genesis), (&v2, &genesis), (&v3, &genesis)]);
        let b1 = creator1(&mut block_store, &mut block_dag_storage, &genesis, &gj);
        let b2 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &b1,
            &HashMap::from([(&v1, &b1), (&v2, &genesis), (&v3, &genesis)]),
        );
        let b3 = creator3(
            &mut block_store,
            &mut block_dag_storage,
            &b2,
            &HashMap::from([(&v1, &b1), (&v2, &b2), (&v3, &genesis)]),
        );
        let b4 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &b3,
            &HashMap::from([(&v1, &b1), (&v2, &b2), (&v3, &b3)]),
        );
        let b5 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &b4,
            &HashMap::from([(&v1, &b4), (&v2, &b2), (&v3, &b3)]),
        );
        let b6 = creator3(
            &mut block_store,
            &mut block_dag_storage,
            &b5,
            &HashMap::from([(&v1, &b4), (&v2, &b5), (&v3, &b3)]),
        );

        let dag = block_dag_storage.get_representation();
        let latest_messages = snapshot(&[(&v1, &b4), (&v2, &b5), (&v3, &b6)]);

        let floor = finalized_floor(
            &dag,
            std::slice::from_ref(&b6.block_hash),
            &latest_messages,
            FT_THRESHOLD,
        )
        .await
        .expect("floor must exist on a converged chain");

        // Brute force the ADVANCEMENT half: first main-chain ancestor of b6 with
        // ft_witnessed >= threshold, falling back to genesis.
        let mut frontier = b6.block_hash.clone();
        loop {
            let ft = CliqueOracle::ft_witnessed(&frontier, &dag, &latest_messages)
                .await
                .expect("ft_witnessed must succeed for chain blocks");
            if ft >= FT_THRESHOLD {
                break;
            }
            match dag.main_parent(&frontier) {
                Some(parent) => frontier = parent,
                None => break,
            }
        }
        let frontier_number = dag
            .block_number_unsafe(&frontier)
            .expect("frontier number must resolve");

        // The INHERITANCE half: the parent's own floor.
        let inherited = floor_of_block(&dag, &b6.block_hash, FT_THRESHOLD)
            .await
            .expect("parent floor must resolve");

        // The floor is the max of the two candidate sources.
        let expected = if inherited.block_number > frontier_number
            || (inherited.block_number == frontier_number && inherited.hash > frontier)
        {
            inherited.hash.clone()
        } else {
            frontier
        };
        assert_eq!(
            floor.hash, expected,
            "floor must equal max(inherited parent floor, brute-force frontier)"
        );
        assert!(
            floor.block_number >= 0,
            "floor block number must be sane, got {}",
            floor.block_number
        );
    })
    .await;
}

/// `floor_of_block` derives the floor from the block's own metadata, persists
/// it through the floor cache, and resolves identically on repeat calls.
#[tokio::test]
async fn floor_of_block_is_cached_and_stable() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("Floor Cache V1"));
        let v2 = generate_validator(Some("Floor Cache V2"));
        let v3 = generate_validator(Some("Floor Cache V3"));
        let bonds = three_bonds(&v1, &v2, &v3);

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
        let creator1 = create_block(&bonds, &genesis, &v1);
        let creator2 = create_block(&bonds, &genesis, &v2);
        let creator3 = create_block(&bonds, &genesis, &v3);

        let gj = HashMap::from([(&v1, &genesis), (&v2, &genesis), (&v3, &genesis)]);
        let b1 = creator1(&mut block_store, &mut block_dag_storage, &genesis, &gj);
        let b2 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &b1,
            &HashMap::from([(&v1, &b1), (&v2, &genesis), (&v3, &genesis)]),
        );
        let b3 = creator3(
            &mut block_store,
            &mut block_dag_storage,
            &b2,
            &HashMap::from([(&v1, &b1), (&v2, &b2), (&v3, &genesis)]),
        );
        let b4 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &b3,
            &HashMap::from([(&v1, &b1), (&v2, &b2), (&v3, &b3)]),
        );

        let dag = block_dag_storage.get_representation();

        // First resolution computes from the block's own metadata.
        let first = floor_of_block(&dag, &b4.block_hash, FT_THRESHOLD)
            .await
            .expect("floor_of_block must resolve for an inserted block");

        // It must match a direct derivation from the same metadata.
        let metadata = dag
            .lookup_unsafe(&b4.block_hash)
            .expect("metadata must exist for an inserted block");
        let latest_messages: BTreeMap<_, _> = metadata
            .justifications
            .iter()
            .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
            .collect();
        let direct = finalized_floor(&dag, &metadata.parents, &latest_messages, FT_THRESHOLD)
            .await
            .expect("direct derivation must succeed");
        assert_eq!(
            first, direct,
            "cached resolution must match direct derivation"
        );

        // The cache must now hold it, and a repeat call must return the same floor.
        let cached = dag
            .get_cached_floor(&b4.block_hash)
            .expect("floor cache read must succeed")
            .expect("floor must be cached after first resolution");
        assert_eq!(cached, first.hash);
        let second = floor_of_block(&dag, &b4.block_hash, FT_THRESHOLD)
            .await
            .expect("repeat resolution must succeed");
        assert_eq!(first, second, "floor_of_block must be stable across calls");

        // Genesis is its own floor — the recursion's terminal cut.
        let genesis_floor = floor_of_block(&dag, &genesis.block_hash, FT_THRESHOLD)
            .await
            .expect("genesis floor must resolve");
        assert_eq!(genesis_floor.hash, genesis.block_hash);
    })
    .await;
}

/// The floor is MONOTONE along ancestry: a child whose own justifications lag
/// behind a cut its parent already carries must INHERIT the parent's floor,
/// never derive a lower one. Without inheritance, such a child's merge would
/// re-litigate races a sealed cut already decided (the V4/V5 finalized-bonds
/// flip observed in integration run 3).
#[tokio::test]
async fn floor_is_monotone_along_ancestry() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("Floor Mono V1"));
        let v2 = generate_validator(Some("Floor Mono V2"));
        let v3 = generate_validator(Some("Floor Mono V3"));
        let bonds = three_bonds(&v1, &v2, &v3);

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
        let creator1 = create_block(&bonds, &genesis, &v1);
        let creator2 = create_block(&bonds, &genesis, &v2);
        let creator3 = create_block(&bonds, &genesis, &v3);

        let gj = HashMap::from([(&v1, &genesis), (&v2, &genesis), (&v3, &genesis)]);
        let b1 = creator1(&mut block_store, &mut block_dag_storage, &genesis, &gj);
        let b2 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &b1,
            &HashMap::from([(&v1, &b1), (&v2, &genesis), (&v3, &genesis)]),
        );
        let b3 = creator3(
            &mut block_store,
            &mut block_dag_storage,
            &b2,
            &HashMap::from([(&v1, &b1), (&v2, &b2), (&v3, &genesis)]),
        );
        let b4 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &b3,
            &HashMap::from([(&v1, &b1), (&v2, &b2), (&v3, &b3)]),
        );
        let b5 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &b4,
            &HashMap::from([(&v1, &b4), (&v2, &b2), (&v3, &b3)]),
        );
        let b6 = creator3(
            &mut block_store,
            &mut block_dag_storage,
            &b5,
            &HashMap::from([(&v1, &b4), (&v2, &b5), (&v3, &b3)]),
        );

        // b7 extends b6 but carries STALE justifications (all genesis) — the
        // run-3 shape: a proposer that has the blocks but whose justification
        // view predates the finalization its lineage already witnessed.
        let b7 = creator1(&mut block_store, &mut block_dag_storage, &b6, &gj);

        let dag = block_dag_storage.get_representation();
        let parent_floor = floor_of_block(&dag, &b6.block_hash, FT_THRESHOLD)
            .await
            .expect("parent floor must resolve");
        let child_floor = floor_of_block(&dag, &b7.block_hash, FT_THRESHOLD)
            .await
            .expect("child floor must resolve");

        assert!(
            child_floor.block_number >= parent_floor.block_number,
            "child floor #{} must not regress below parent floor #{} (stale justifications must inherit)",
            child_floor.block_number,
            parent_floor.block_number,
        );
        assert!(
            parent_floor.block_number > 0,
            "test needs the parent floor past genesis to be meaningful, got #{}",
            parent_floor.block_number,
        );
    })
    .await;
}

/// The floor is a pure function of the justification snapshot: growing the DAG
/// (new fork blocks, changed live latest messages) must not change the floor
/// derived from the SAME frozen snapshot. This is the property node-local LFB
/// state never had.
#[tokio::test]
async fn floor_is_frozen_under_dag_growth() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("Floor Frozen V1"));
        let v2 = generate_validator(Some("Floor Frozen V2"));
        let v3 = generate_validator(Some("Floor Frozen V3"));
        let bonds = three_bonds(&v1, &v2, &v3);

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
        let creator1 = create_block(&bonds, &genesis, &v1);
        let creator2 = create_block(&bonds, &genesis, &v2);
        let creator3 = create_block(&bonds, &genesis, &v3);

        let gj = HashMap::from([(&v1, &genesis), (&v2, &genesis), (&v3, &genesis)]);
        let b1 = creator1(&mut block_store, &mut block_dag_storage, &genesis, &gj);
        let b2 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &b1,
            &HashMap::from([(&v1, &b1), (&v2, &genesis), (&v3, &genesis)]),
        );
        let b3 = creator3(
            &mut block_store,
            &mut block_dag_storage,
            &b2,
            &HashMap::from([(&v1, &b1), (&v2, &b2), (&v3, &genesis)]),
        );
        let b4 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &b3,
            &HashMap::from([(&v1, &b1), (&v2, &b2), (&v3, &b3)]),
        );

        let latest_messages = snapshot(&[(&v1, &b4), (&v2, &b2), (&v3, &b3)]);
        let parents = vec![b4.block_hash.clone()];

        let dag_before = block_dag_storage.get_representation();
        let floor_before = finalized_floor(&dag_before, &parents, &latest_messages, FT_THRESHOLD)
            .await
            .expect("floor must exist before DAG growth");

        // Grow the DAG: V2 and V3 fork off genesis, moving their LIVE latest
        // messages away from the chain.
        let f1 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            &HashMap::from([(&v1, &genesis), (&v2, &genesis), (&v3, &genesis)]),
        );
        let _f2 = creator3(
            &mut block_store,
            &mut block_dag_storage,
            &f1,
            &HashMap::from([(&v1, &genesis), (&v2, &f1), (&v3, &genesis)]),
        );

        let dag_after = block_dag_storage.get_representation();
        let floor_after = finalized_floor(&dag_after, &parents, &latest_messages, FT_THRESHOLD)
            .await
            .expect("floor must exist after DAG growth");

        assert_eq!(
            floor_before, floor_after,
            "floor over a frozen justification snapshot must not move when the DAG grows"
        );
    })
    .await;
}

/// RED reproduction of the finalized-floor liveness wedge exposed once the
/// clique oracle finalizes via DAG-ancestry (integration run 8c2952a8:
/// `finalized-floor safety violation: cut (#11) is not an ancestor of floor
/// (#11)`; validator3 could not propose).
///
/// Three equal-weight validators fork at height 1 (a1,a2,a3), each merges all
/// three at height 2 (m1,m2,m3, own block as main parent), then a convergence
/// round at height 3 (n1,n2,n3) mutually justifies the merges. Every height-1
/// block is now merged into every latest message, so the clique oracle
/// finalizes a1, a2 AND a3 (DAG-ancestry agreement).
///
/// A block merging m1 and m2 has per-parent frontiers a1 (frontier of m1) and
/// a2 (frontier of m2) — two finalized SIBLINGS at height 1, neither on the
/// other's main chain. `derive_floor` picks the max and demands the other be a
/// general-ancestor of it, which siblings never are → it raises a "safety
/// violation" and blocks the proposer. But a1 and a2 are MERGEABLE (both merged
/// into m1 and m2), so this is a single-main-parent false positive, not a real
/// fork: a valid floor exists — either sibling is a general-ancestor of BOTH
/// parents, so it is a sound merge base.
///
/// FIX: the floor must be the highest finalized candidate that is a
/// general-ancestor of every PARENT (a valid merge base below all inputs),
/// instead of requiring every candidate to lie on the floor's own ancestry.
#[tokio::test]
async fn finalized_floor_admits_mergeable_cofinalized_siblings() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("Floor Sym V1"));
        let v2 = generate_validator(Some("Floor Sym V2"));
        let v3 = generate_validator(Some("Floor Sym V3"));
        let bonds = vec![
            Bond {
                validator: v1.clone(),
                stake: 100,
            },
            Bond {
                validator: v2.clone(),
                stake: 100,
            },
            Bond {
                validator: v3.clone(),
                stake: 100,
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

        let mk = |store: &mut KeyValueBlockStore,
                  dag: &mut IndexedBlockDagStorage,
                  parents: Vec<BlockHash>,
                  creator: &Validator,
                  just: HashMap<Validator, BlockHash>|
         -> BlockMessage {
            crate::helper::block_generator::create_block(
                store,
                dag,
                parents,
                &genesis,
                Some(creator.clone()),
                Some(bonds.clone()),
                Some(just),
                None,
                None,
                None,
                None,
                None,
                None,
            )
        };

        // Height 1: symmetric 3-way fork off genesis.
        let gj = HashMap::from([
            (v1.clone(), genesis.block_hash.clone()),
            (v2.clone(), genesis.block_hash.clone()),
            (v3.clone(), genesis.block_hash.clone()),
        ]);
        let a1 = mk(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &v1,
            gj.clone(),
        );
        let a2 = mk(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &v2,
            gj.clone(),
        );
        let a3 = mk(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &v3,
            gj.clone(),
        );

        // Height 2: each merges all three height-1 blocks (own as main parent).
        let saw_all = HashMap::from([
            (v1.clone(), a1.block_hash.clone()),
            (v2.clone(), a2.block_hash.clone()),
            (v3.clone(), a3.block_hash.clone()),
        ]);
        let m1 = mk(
            &mut block_store,
            &mut block_dag_storage,
            vec![
                a1.block_hash.clone(),
                a2.block_hash.clone(),
                a3.block_hash.clone(),
            ],
            &v1,
            saw_all.clone(),
        );
        let m2 = mk(
            &mut block_store,
            &mut block_dag_storage,
            vec![
                a2.block_hash.clone(),
                a1.block_hash.clone(),
                a3.block_hash.clone(),
            ],
            &v2,
            saw_all.clone(),
        );
        let m3 = mk(
            &mut block_store,
            &mut block_dag_storage,
            vec![
                a3.block_hash.clone(),
                a1.block_hash.clone(),
                a2.block_hash.clone(),
            ],
            &v3,
            saw_all.clone(),
        );

        // Height 3: convergence round — mutually justify the merge blocks, so the
        // oracle witnesses that a1,a2,a3 are merged everywhere and finalizes them.
        let saw_merges = HashMap::from([
            (v1.clone(), m1.block_hash.clone()),
            (v2.clone(), m2.block_hash.clone()),
            (v3.clone(), m3.block_hash.clone()),
        ]);
        let n1 = mk(
            &mut block_store,
            &mut block_dag_storage,
            vec![m1.block_hash.clone()],
            &v1,
            saw_merges.clone(),
        );
        let n2 = mk(
            &mut block_store,
            &mut block_dag_storage,
            vec![m2.block_hash.clone()],
            &v2,
            saw_merges.clone(),
        );
        let n3 = mk(
            &mut block_store,
            &mut block_dag_storage,
            vec![m3.block_hash.clone()],
            &v3,
            saw_merges.clone(),
        );

        let dag = block_dag_storage.get_representation();
        let latest_messages = snapshot(&[(&v1, &n1), (&v2, &n2), (&v3, &n3)]);

        // Sanity: a1 and a2 are finalized siblings (DAG-ancestry agreement).
        let ft_a1 = CliqueOracle::ft_witnessed(&a1.block_hash, &dag, &latest_messages)
            .await
            .unwrap();
        let ft_a2 = CliqueOracle::ft_witnessed(&a2.block_hash, &dag, &latest_messages)
            .await
            .unwrap();
        assert!(
            ft_a1 >= FT_THRESHOLD && ft_a2 >= FT_THRESHOLD,
            "precondition: both height-1 siblings must be finalized (ft_a1={ft_a1}, ft_a2={ft_a2})"
        );

        // A block merging m1 and m2: its parent frontiers are the finalized
        // siblings a1 and a2. The floor must be a valid merge base, not an error.
        let floor = finalized_floor(
            &dag,
            &[m1.block_hash.clone(), m2.block_hash.clone()],
            &latest_messages,
            FT_THRESHOLD,
        )
        .await
        .expect(
            "co-finalized MERGEABLE siblings (a1, a2 both merged into m1 and m2) must yield a \
             valid floor, not a finalized-floor safety violation — the violation is a \
             single-main-parent false positive that blocks the proposer (run 8c2952a8)",
        );

        // The floor must be a real finalized cut that is a general-ancestor of
        // BOTH parents (a sound merge base), and must not advance above the
        // finalized height-1 cut.
        let floor_id = if floor.hash == a1.block_hash {
            "a1"
        } else if floor.hash == a2.block_hash {
            "a2"
        } else if floor.hash == a3.block_hash {
            "a3"
        } else if floor.hash == m1.block_hash {
            "m1"
        } else if floor.hash == m2.block_hash {
            "m2"
        } else if floor.hash == m3.block_hash {
            "m3"
        } else if floor.hash == genesis.block_hash {
            "genesis"
        } else {
            "UNKNOWN"
        };
        // The floor must be a sound merge base: a finalized cut that is a general
        // DAG-ancestor of BOTH parents (so the block can merge each parent onto
        // it), and a common ANCESTOR below the parents — never one of the
        // parents/merge tips themselves.
        let cov_m1 = dag.is_dag_ancestor(&floor.hash, &m1.block_hash).unwrap();
        let cov_m2 = dag.is_dag_ancestor(&floor.hash, &m2.block_hash).unwrap();
        assert!(
            cov_m1 && cov_m2,
            "floor {floor_id} must be a general-ancestor of every parent \
             (cov_m1={cov_m1}, cov_m2={cov_m2})"
        );
        let floor_ft = CliqueOracle::ft_witnessed(&floor.hash, &dag, &latest_messages)
            .await
            .unwrap();
        assert!(
            floor_ft >= FT_THRESHOLD,
            "floor {floor_id} must itself be finalized; got ft={floor_ft}"
        );
        assert!(
            floor.hash != m1.block_hash
                && floor.hash != m2.block_hash
                && floor.hash != m3.block_hash,
            "floor must be a common ancestor below the parents, not a merge tip; got {floor_id}"
        );
    })
    .await;
}

// `fate_any_accepted_inclusion_overrides_a_later_rejection` (the regression lock
// for the recover-an-already-finalized-deploy content-twin) removed: it
// regression-locked `FloorFateResolver` / `DeployFateAtFloor`, which the
// buffer-drain change deletes. Recovery no longer judges a deploy by its sealed
// fate; the buffer invariant ("membership == not in the execution base", kept by
// the accept-time purge in handle_valid_block) prevents re-executing an applied
// deploy, with `Validate::repeat_deploy` as the consensus backstop.
