// See casper/src/test/scala/coop/rchain/casper/batch2/CliqueOracleTest.scala

use std::collections::HashMap;
use std::time::Instant;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::safety::clique_oracle::CliqueOracle;
use casper::rust::safety_oracle::{CliqueOracleImpl, SafetyOracle};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, ProcessedDeploy};
use models::rust::validator::Validator;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::create_genesis_block;
use crate::helper::block_util::generate_validator;

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
            None,
        )
    }
}

/// Verifies that cached fault tolerance in BlockMetadata is stable after
/// finalization, even when the DAG state changes.
///
/// DAG structure:
/// ```
///   Phase 1 — linear chain, all validators cooperating:
///     genesis ← b1(V1) ← b2(V2) ← b3(V3) ← b4(V1) ← b5(V2) ← b6(V3)
///
///   Phase 2 — V2 and V3 fork off genesis (simulating different DAG tip on another node):
///     genesis ← f1(V2) ← f2(V3)
/// ```
///
/// After Phase 1, b1 is finalized with FT=1.0 (all validators agree).
/// The FT is cached in BlockMetadata.fault_tolerance_value.
///
/// After Phase 2, V2 and V3's latest messages are on a fork — the clique
/// oracle would return FT=-1.0 for b1. But the cached value in metadata
/// remains 1.0 because finalization is permanent.
#[tokio::test]
async fn finalized_block_ft_should_not_change_with_dag_state() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("FT Stable V1"));
        let v2 = generate_validator(Some("FT Stable V2"));
        let v3 = generate_validator(Some("FT Stable V3"));
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

        let creator1 = create_block(&bonds, &genesis, &v1);
        let creator2 = create_block(&bonds, &genesis, &v2);
        let creator3 = create_block(&bonds, &genesis, &v3);

        // Phase 1: linear chain with full justification propagation
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

        let _b6 = creator3(
            &mut block_store,
            &mut block_dag_storage,
            &b5,
            &HashMap::from([(&v1, &b4), (&v2, &b5), (&v3, &b3)]),
        );

        // Compute FT via oracle before finalization
        let dag_phase1 = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let safety_oracle = CliqueOracleImpl;
        let ft_phase1 = safety_oracle
            .normalized_fault_tolerance(&dag_phase1, &b1.block_hash)
            .await
            .unwrap();

        assert!(
            ft_phase1 > 0.0,
            "b1 should have positive FT when all validators agree (got {ft_phase1})"
        );

        // Finalize b1 with the computed FT — this caches it in BlockMetadata
        block_dag_storage
            .record_directly_finalized(b1.block_hash.clone(), ft_phase1, |_| async { Ok(()) })
            .await
            .unwrap();

        // Verify FT is cached in metadata
        let dag_after_finalize = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let meta_after_finalize = dag_after_finalize.lookup(&b1.block_hash).unwrap().unwrap();
        assert!(
            meta_after_finalize.finalized,
            "b1 should be marked as finalized"
        );
        assert_eq!(
            meta_after_finalize.fault_tolerance_value, ft_phase1,
            "Cached FT should match the value at finalization time"
        );

        // Phase 2: V2 and V3 create blocks forking off genesis (not through b1).
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

        // Verify the oracle now returns a different (lower) FT for b1
        let dag_phase2 = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let ft_oracle_phase2 = safety_oracle
            .normalized_fault_tolerance(&dag_phase2, &b1.block_hash)
            .await
            .unwrap();
        assert!(
            ft_oracle_phase2 < ft_phase1,
            "Oracle FT should decrease after fork (was {ft_phase1}, now {ft_oracle_phase2})"
        );

        // But the cached metadata FT should be unchanged
        let meta_phase2 = dag_phase2.lookup(&b1.block_hash).unwrap().unwrap();
        assert_eq!(
            meta_phase2.fault_tolerance_value, ft_phase1,
            "Cached FT in metadata should be immutable after finalization. \
             Phase 1 (cached): {ft_phase1}, Phase 2 (cached): {}",
            meta_phase2.fault_tolerance_value
        );

        // The cached value must be above any reasonable threshold
        assert!(
            meta_phase2.fault_tolerance_value > 0.0,
            "Cached FT must be above threshold (got {})",
            meta_phase2.fault_tolerance_value
        );
    })
    .await
}

// See [[/docs/casper/images/cbc-casper_ping_pong_diagram.png]]
/**
 *       *     b8
 *       |
 *   *   *     b6 b7
 *   | /
 *   *   *     b4 b5
 *   | /
 *   *   *     b2 b3
 *    \ /
 *     *
 *   c2 c1
 */
#[tokio::test]
async fn clique_oracle_should_detect_finality_as_appropriate() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("Validator One"));
        let v2 = generate_validator(Some("Validator Two"));
        let v1_bond = Bond {
            validator: v1.clone(),
            stake: 2,
        };
        let v2_bond = Bond {
            validator: v2.clone(),
            stake: 3,
        };
        let bonds = vec![v1_bond, v2_bond];

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

        let genesis_justification = HashMap::from([(&v1, &genesis), (&v2, &genesis)]);

        let b2 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            &genesis_justification,
        );

        let b3 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            &genesis_justification,
        );

        let b4 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &b2,
            &HashMap::from([(&v1, &genesis), (&v2, &b2)]),
        );

        let b5 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &b2,
            &HashMap::from([(&v1, &b3), (&v2, &b2)]),
        );

        let _b6 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &b4,
            &HashMap::from([(&v1, &b5), (&v2, &b4)]),
        );

        let b7 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &b4,
            &HashMap::from([(&v1, &b5), (&v2, &b4)]),
        );

        let _b8 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &b7,
            &HashMap::from([(&v1, &b7), (&v2, &b4)]),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let safety_oracle = CliqueOracleImpl;

        let genesis_fault_tolerance = safety_oracle
            .normalized_fault_tolerance(&dag, &genesis.block_hash)
            .await
            .unwrap();
        assert!((genesis_fault_tolerance - 1.0).abs() < 0.01);

        let b2_fault_tolerance = safety_oracle
            .normalized_fault_tolerance(&dag, &b2.block_hash)
            .await
            .unwrap();
        assert!((b2_fault_tolerance - 1.0).abs() < 0.01);

        let b3_fault_tolerance = safety_oracle
            .normalized_fault_tolerance(&dag, &b3.block_hash)
            .await
            .unwrap();
        assert!((b3_fault_tolerance - (-1.0)).abs() < 0.01);

        let b4_fault_tolerance = safety_oracle
            .normalized_fault_tolerance(&dag, &b4.block_hash)
            .await
            .unwrap();
        assert!((b4_fault_tolerance - 0.2).abs() < 0.01);
    })
    .await
}

// See [[/docs/casper/images/no_finalizable_block_mistake_with_no_disagreement_check.png]]
#[tokio::test]
async fn clique_oracle_should_detect_possible_disagreements_appropriately() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("Validator One"));
        let v2 = generate_validator(Some("Validator Two"));
        let v3 = generate_validator(Some("Validator Three"));
        let v1_bond = Bond {
            validator: v1.clone(),
            stake: 25,
        };
        let v2_bond = Bond {
            validator: v2.clone(),
            stake: 20,
        };
        let v3_bond = Bond {
            validator: v3.clone(),
            stake: 15,
        };
        let bonds = vec![v1_bond, v2_bond, v3_bond];

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

        let genesis_justification =
            HashMap::from([(&v1, &genesis), (&v2, &genesis), (&v3, &genesis)]);

        let b2 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            &genesis_justification,
        );

        let b3 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            &genesis_justification,
        );

        let b4 = creator3(
            &mut block_store,
            &mut block_dag_storage,
            &b2,
            &HashMap::from([(&v1, &genesis), (&v2, &b2), (&v3, &b2)]),
        );

        let b5 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &b3,
            &HashMap::from([(&v1, &b3), (&v2, &b2), (&v3, &genesis)]),
        );

        let b6 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &b4,
            &HashMap::from([(&v1, &b3), (&v2, &b2), (&v3, &b4)]),
        );

        let _b7 = creator3(
            &mut block_store,
            &mut block_dag_storage,
            &b5,
            &HashMap::from([(&v1, &b3), (&v2, &b5), (&v3, &b4)]),
        );

        let _b8 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &b6,
            &HashMap::from([(&v1, &b6), (&v2, &b5), (&v3, &b4)]),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let safety_oracle = CliqueOracleImpl;

        let genesis_fault_tolerance = safety_oracle
            .normalized_fault_tolerance(&dag, &genesis.block_hash)
            .await
            .unwrap();
        assert!((genesis_fault_tolerance - 1.0).abs() < 0.01);

        let b2_fault_tolerance = safety_oracle
            .normalized_fault_tolerance(&dag, &b2.block_hash)
            .await
            .unwrap();
        assert!((b2_fault_tolerance - (-1.0 / 6.0)).abs() < 0.01);

        let b3_fault_tolerance = safety_oracle
            .normalized_fault_tolerance(&dag, &b3.block_hash)
            .await
            .unwrap();
        assert!((b3_fault_tolerance - (-1.0)).abs() < 0.01);

        let b4_fault_tolerance = safety_oracle
            .normalized_fault_tolerance(&dag, &b4.block_hash)
            .await
            .unwrap();
        assert!((b4_fault_tolerance - (-1.0 / 6.0)).abs() < 0.01);
    })
    .await
}

// See [[/docs/casper/images/no_majority_fork_safe_after_union.png]]
#[tokio::test]
async fn clique_oracle_should_identify_no_majority_fork_safe_after_union() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator Zero"));
        let v1 = generate_validator(Some("Validator One"));
        let v2 = generate_validator(Some("Validator Two"));
        let v3 = generate_validator(Some("Validator Three"));
        let v4 = generate_validator(Some("Validator Four"));
        let bonds = vec![
            Bond {
                validator: v0.clone(),
                stake: 500,
            },
            Bond {
                validator: v1.clone(),
                stake: 450,
            },
            Bond {
                validator: v2.clone(),
                stake: 600,
            },
            Bond {
                validator: v3.clone(),
                stake: 400,
            },
            Bond {
                validator: v4.clone(),
                stake: 525,
            },
        ];

        /*
        # create right hand side of fork and check for no safety
        'M-2-A SJ-1-A M-1-L0 SJ-0-L0 M-0-L1 SJ-1-L1 M-1-L2 SJ-0-L2 '
        'M-0-L3 SJ-1-L3 M-1-L4 SJ-0-L4 '
        # now, left hand side as well. should still have no safety
        'SJ-3-A M-3-R0 SJ-4-R0 M-4-R1 SJ-3-R1 M-3-R2 SJ-4-R2 M-4-R3 '
        'SJ-3-R3 M-3-R4 SJ-4-R4'
        */

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

        let creator0 = create_block(&bonds, &genesis, &v0);
        let creator1 = create_block(&bonds, &genesis, &v1);
        let creator2 = create_block(&bonds, &genesis, &v2);
        let creator3 = create_block(&bonds, &genesis, &v3);
        let creator4 = create_block(&bonds, &genesis, &v4);

        let mut gj_l = HashMap::from([
            (&v0, &genesis),
            (&v1, &genesis),
            (&v2, &genesis),
            (&v3, &genesis),
            (&v4, &genesis),
        ]);

        /*
         create left hand side of fork and check for no safety
        'M-2-A SJ-1-A M-1-L0 SJ-0-L0 M-0-L1 SJ-1-L1 M-1-L2 SJ-0-L2 '
        'M-0-L3 SJ-1-L3 M-1-L4 SJ-0-L4 '
         */
        let a = creator2(&mut block_store, &mut block_dag_storage, &genesis, &gj_l);

        gj_l.insert(&v2, &a);
        let l0 = creator1(&mut block_store, &mut block_dag_storage, &a, &gj_l);

        gj_l.insert(&v1, &l0);
        let l1 = creator0(&mut block_store, &mut block_dag_storage, &l0, &gj_l);

        gj_l.insert(&v0, &l1);
        let l2 = creator1(&mut block_store, &mut block_dag_storage, &l1, &gj_l);

        gj_l.insert(&v1, &l2);
        let l3 = creator0(&mut block_store, &mut block_dag_storage, &l2, &gj_l);

        gj_l.insert(&v0, &l3);
        let l4 = creator1(&mut block_store, &mut block_dag_storage, &l3, &gj_l);

        let mut gj_r = HashMap::from([
            (&v0, &genesis),
            (&v1, &genesis),
            (&v2, &genesis),
            (&v3, &genesis),
            (&v4, &genesis),
        ]);

        /*
         now, right hand side as well. should still have no safety
        'SJ-3-A M-3-R0 SJ-4-R0 M-4-R1 SJ-3-R1 M-3-R2 SJ-4-R2 M-4-R3 '
        'SJ-3-R3 M-3-R4 SJ-4-R4'
         */
        gj_r.insert(&v2, &a);
        let r0 = creator3(&mut block_store, &mut block_dag_storage, &a, &gj_r);

        gj_r.insert(&v3, &r0);
        let r1 = creator4(&mut block_store, &mut block_dag_storage, &r0, &gj_r);

        gj_r.insert(&v4, &r1);
        let r2 = creator3(&mut block_store, &mut block_dag_storage, &r1, &gj_r);

        gj_r.insert(&v3, &r2);
        let r3 = creator4(&mut block_store, &mut block_dag_storage, &r2, &gj_r);

        gj_r.insert(&v4, &r3);
        let r4 = creator3(&mut block_store, &mut block_dag_storage, &r3, &gj_r);

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let safety_oracle = CliqueOracleImpl;

        let l0_fault_tolerance = safety_oracle
            .normalized_fault_tolerance(&dag, &l0.block_hash)
            .await
            .unwrap();
        assert!((l0_fault_tolerance - (-1.0)).abs() < 0.01);

        let r0_fault_tolerance = safety_oracle
            .normalized_fault_tolerance(&dag, &r0.block_hash)
            .await
            .unwrap();
        assert!((r0_fault_tolerance - (-1.0)).abs() < 0.01);

        /*
         show all validators all messages
        'SJ-0-R4 SJ-1-R4 SJ-2-R4 SJ-2-L4 SJ-3-L4 SJ-4-L4 '
         */
        let mut aj = HashMap::from([(&v0, &l3), (&v1, &l4), (&v2, &a), (&v3, &r4), (&v4, &r3)]);

        /*
         two rounds of round robin, check have safety on the correct fork
        'M-0-J0 SJ-1-J0 M-1-J1 SJ-2-J1 M-2-J2 SJ-3-J2 M-3-J3 SJ-4-J3 M-4-J4 SJ-0-J4 '
        'M-0-J01 SJ-1-J01 M-1-J11 SJ-2-J11 M-2-J21 SJ-3-J21 M-3-J31 SJ-4-J31 M-4-J41 SJ-0-J41'
         */

        let j0 = creator0(&mut block_store, &mut block_dag_storage, &l4, &aj);

        aj.insert(&v0, &j0);
        let j1 = creator1(&mut block_store, &mut block_dag_storage, &j0, &aj);

        aj.insert(&v1, &j1);
        let j2 = creator2(&mut block_store, &mut block_dag_storage, &j1, &aj);

        aj.insert(&v2, &j2);
        let j3 = creator3(&mut block_store, &mut block_dag_storage, &j2, &aj);

        aj.insert(&v3, &j3);
        let j4 = creator4(&mut block_store, &mut block_dag_storage, &j3, &aj);

        aj.insert(&v4, &j4);
        let j01 = creator0(&mut block_store, &mut block_dag_storage, &j4, &aj);

        aj.insert(&v0, &j01);
        let j11 = creator1(&mut block_store, &mut block_dag_storage, &j01, &aj);

        aj.insert(&v1, &j11);
        let j21 = creator2(&mut block_store, &mut block_dag_storage, &j11, &aj);

        aj.insert(&v2, &j21);
        let j31 = creator3(&mut block_store, &mut block_dag_storage, &j21, &aj);

        aj.insert(&v3, &j31);
        let _j41 = creator4(&mut block_store, &mut block_dag_storage, &j31, &aj);

        let dag2 = block_dag_storage
            .get_representation()
            .expect("dag representation");

        let fault_tolerance = safety_oracle
            .normalized_fault_tolerance(&dag2, &l0.block_hash)
            .await
            .unwrap();
        assert!((fault_tolerance - 1.0).abs() < 0.01);
    })
    .await
}

#[tokio::test]
#[ignore = "diagnostic: run manually for fast clique-oracle growth feedback"]
async fn clique_oracle_growth_feedback_loop_stale_justification_chain() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let validators = [generate_validator(Some("Growth Validator One")),
            generate_validator(Some("Growth Validator Two")),
            generate_validator(Some("Growth Validator Three"))];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|validator| Bond {
                validator: validator.clone(),
                stake: 10,
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

        let creator1 = create_block(&bonds, &genesis, &validators[0]);
        let creator2 = create_block(&bonds, &genesis, &validators[1]);
        let creator3 = create_block(&bonds, &genesis, &validators[2]);

        let checkpoints = [24usize, 48usize, 96usize];
        let mut latest_by_validator = [genesis.clone(), genesis.clone(), genesis.clone()];
        let mut timing_samples: Vec<(usize, u128, f32)> = Vec::with_capacity(checkpoints.len());

        for height in 1..=checkpoints[checkpoints.len() - 1] {
            let creator_index = (height - 1) % validators.len();
            let parent = &latest_by_validator[creator_index];

            let mut justifications: HashMap<&Validator, &BlockMessage> = HashMap::new();
            for (idx, validator) in validators.iter().enumerate() {
                let justification = if idx == creator_index {
                    &latest_by_validator[idx]
                } else {
                    &genesis
                };
                justifications.insert(validator, justification);
            }

            let next_block = match creator_index {
                0 => creator1(
                    &mut block_store,
                    &mut block_dag_storage,
                    parent,
                    &justifications,
                ),
                1 => creator2(
                    &mut block_store,
                    &mut block_dag_storage,
                    parent,
                    &justifications,
                ),
                2 => creator3(
                    &mut block_store,
                    &mut block_dag_storage,
                    parent,
                    &justifications,
                ),
                _ => unreachable!("creator_index should be in [0, 2]"),
            };
            latest_by_validator[creator_index] = next_block;

            if checkpoints.contains(&height) {
                let dag = block_dag_storage.get_representation().expect("dag representation");
                let target_hash = genesis.block_hash.clone();
                let started = Instant::now();
                let message_weight_map = CliqueOracle::get_corresponding_weight_map(&target_hash, &dag)
                    .await
                    .expect("weight map should be available for target");
                let mut agreeing_weight_map = HashMap::new();
                for (validator, weight) in &message_weight_map {
                    if let Some(latest_hash) = dag.latest_message_hash(validator) {
                        let in_main_chain = dag
                            .is_in_main_chain(&target_hash, &latest_hash)
                            .expect("main chain lookup should succeed");
                        if in_main_chain {
                            agreeing_weight_map.insert(validator.clone(), *weight);
                        }
                    }
                }
                let latest_messages: std::collections::BTreeMap<_, _> =
                    dag.latest_message_hashes().into_iter().collect();
                let fault_tolerance = CliqueOracle::compute_output(
                    &target_hash,
                    &message_weight_map,
                    &agreeing_weight_map,
                    &dag,
                    &latest_messages,
                )
                .await
                .expect("Clique oracle should compute fault tolerance");
                timing_samples.push((height, started.elapsed().as_millis(), fault_tolerance));
            }
        }

        assert_eq!(timing_samples.len(), checkpoints.len());
        eprintln!("clique-oracle growth feedback (stale-justification chain):");
        for (height, elapsed_ms, fault_tolerance) in timing_samples {
            eprintln!(
                "  height={height:>3} clique_oracle_ms={elapsed_ms} fault_tolerance={fault_tolerance:.4}"
            );
        }
    })
    .await
}

/// Tests whether a finalized block that becomes unreachable from future LFBs
/// gets its cached FT updated by the propagation pass.
///
/// DAG structure:
/// ```
///   genesis
///    ├── b1_v1 (V1, height 1) ← finalized as first LFB with FT=0.33
///    ├── b1_v2 (V2, height 1)
///    └── b1_v3 (V3, height 1)
///              └── b2 (V1, height 2, parent=b1_v3) ← later LFB
///
///   b2's ancestor chain: b2 → b1_v3 → genesis
///   b1_v1 is NOT in b2's ancestor chain (it's a sibling at height 1)
/// ```
///
/// Question: does propagate_ft_to_ancestors update b1_v1 when b2 is finalized?
/// If not, b1_v1 stays at FT=0.33 forever — the node issue.
#[tokio::test]
async fn orphaned_finalized_block_should_still_get_ft_updated() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v1 = generate_validator(Some("Orphan V1"));
        let v2 = generate_validator(Some("Orphan V2"));
        let v3 = generate_validator(Some("Orphan V3"));
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

        let creator1 = create_block(&bonds, &genesis, &v1);
        let creator2 = create_block(&bonds, &genesis, &v2);
        let creator3 = create_block(&bonds, &genesis, &v3);

        let gj = HashMap::from([(&v1, &genesis), (&v2, &genesis), (&v3, &genesis)]);

        // Three blocks at height 1, one per validator, all parented on genesis
        let b1_v1 = creator1(&mut block_store, &mut block_dag_storage, &genesis, &gj);
        let _b1_v2 = creator2(&mut block_store, &mut block_dag_storage, &genesis, &gj);
        let b1_v3 = creator3(&mut block_store, &mut block_dag_storage, &genesis, &gj);

        // Finalize b1_v1 as the first LFB with a low FT
        block_dag_storage
            .record_directly_finalized(b1_v1.block_hash.clone(), 0.33, |_| async { Ok(()) })
            .await
            .unwrap();

        let dag_after_first = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let meta_v1 = dag_after_first.lookup(&b1_v1.block_hash).unwrap().unwrap();
        assert!(
            (meta_v1.fault_tolerance_value - 0.33).abs() < 0.01,
            "b1_v1 should have FT=0.33 after first finalization (got {})",
            meta_v1.fault_tolerance_value
        );

        // Build b2 on top of b1_v3 (NOT b1_v1) — the DAG diverges
        let b2 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &b1_v3,
            &HashMap::from([(&v1, &b1_v1), (&v2, &genesis), (&v3, &b1_v3)]),
        );

        // Finalize b2 as the new LFB with FT=1.0
        // b2's ancestor chain: b2 → b1_v3 → genesis
        // b1_v1 is NOT in this chain
        block_dag_storage
            .record_directly_finalized(b2.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .unwrap();

        // Check: did b1_v1 get updated?
        let dag_after_second = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let meta_v1_after = dag_after_second.lookup(&b1_v1.block_hash).unwrap().unwrap();

        eprintln!(
            "b1_v1 FT after second finalization: {} (expected 1.0)",
            meta_v1_after.fault_tolerance_value
        );

        // This assertion will FAIL if propagation doesn't reach orphaned
        // finalized blocks — confirming the node issue.
        assert!(
            (meta_v1_after.fault_tolerance_value - 1.0).abs() < 0.01,
            "b1_v1 should have FT updated to 1.0 after second finalization, \
             but got {}. The block is finalized but not in the new LFB's \
             ancestor chain — propagation didn't reach it.",
            meta_v1_after.fault_tolerance_value
        );
    })
    .await
}

/// Certification must be EXCLUSIVE per height: two same-height sibling blocks
/// can never both be witnessed-finalized over one snapshot with less than the
/// fault-tolerance weight equivocating. A validator's chain passes through
/// exactly one block per height, so agreement — the estimator-relevant
/// relation the clique locks — can back at most one sibling.
///
/// DAG (three equal-stake validators; S1/S2 are siblings over genesis, every
/// later block MERGES both, and the visibility layer L* gives every pair
/// mutual justification sight):
///
/// ```text
///           genesis
///           /     \
///        S1(v1)  S2(v2)
///          | \   / |
///          |  \ /  |
///        M1(v1) M2(v2) M3(v3)     parents [S1,S2] each; M2 spine=S2, M1/M3 spine=S1
///          |      |      |
///        L1(v1) L2(v2) L3(v3)     parents [M*..]; justs {v1:M1, v2:M2, v3:M3}
/// ```
///
/// Over J = {v1:L1, v2:L2, v3:L3} every validator has MERGED both siblings
/// (both are DAG-ancestors of every latest message — asserted below), so an
/// agreement relation that reads DAG ancestry counts full weight for BOTH
/// siblings and certifies both. That plural certificate is exactly what froze
/// the finalized floor in ucc session 00e6a2e3 (two inherited floors at one
/// height, neither containing the other's state). Chain-choice agreement is
/// exclusive: the spines back S1 (v1, v3) over S2 (v2 only), and at most one
/// sibling clears the oracle.
#[tokio::test]
async fn conflicting_same_height_siblings_cannot_both_certify() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        use casper::rust::safety::clique_oracle::FtThreshold;

        let v1 = generate_validator(Some("Sibling V1"));
        let v2 = generate_validator(Some("Sibling V2"));
        let v3 = generate_validator(Some("Sibling V3"));
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
                  block_store: &mut KeyValueBlockStore,
                  block_dag_storage: &mut IndexedBlockDagStorage| {
            crate::helper::block_generator::create_block(
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

        // The same-height siblings.
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

        // Merge layer: every validator merges BOTH siblings; v2's spine runs
        // through its own sibling, v1's and v3's through S1.
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

        // Visibility layer: every pair sees every other's merge-layer block.
        let lj: HashMap<Validator, BlockHash> = HashMap::from([
            (v1.clone(), m1.block_hash.clone()),
            (v2.clone(), m2.block_hash.clone()),
            (v3.clone(), m3.block_hash.clone()),
        ]);
        let l1 = mk(
            &v1,
            vec![
                m1.block_hash.clone(),
                m2.block_hash.clone(),
                m3.block_hash.clone(),
            ],
            lj.clone(),
            &mut block_store,
            &mut block_dag_storage,
        );
        let l2 = mk(
            &v2,
            vec![
                m2.block_hash.clone(),
                m1.block_hash.clone(),
                m3.block_hash.clone(),
            ],
            lj.clone(),
            &mut block_store,
            &mut block_dag_storage,
        );
        let l3 = mk(
            &v3,
            vec![
                m3.block_hash.clone(),
                m1.block_hash.clone(),
                m2.block_hash.clone(),
            ],
            lj.clone(),
            &mut block_store,
            &mut block_dag_storage,
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        // Precondition: every latest message has MERGED both siblings, so a
        // DAG-ancestry agreement relation counts full weight for both.
        let snapshot: std::collections::BTreeMap<Validator, BlockHash> = [
            (v1.clone(), l1.block_hash.clone()),
            (v2.clone(), l2.block_hash.clone()),
            (v3.clone(), l3.block_hash.clone()),
        ]
        .into_iter()
        .collect();
        for lm in snapshot.values() {
            for sib in [&s1.block_hash, &s2.block_hash] {
                assert!(
                    dag.is_dag_ancestor(sib, lm).unwrap(),
                    "staging: every latest message must have merged both siblings"
                );
            }
        }

        let thr = FtThreshold::from_f32_lossy(0.1);
        let s1_certified =
            CliqueOracle::ft_witnessed_exact(&s1.block_hash, &dag, &snapshot, thr, false)
                .await
                .expect("ft_witnessed_exact(S1)");
        let s2_certified =
            CliqueOracle::ft_witnessed_exact(&s2.block_hash, &dag, &snapshot, thr, false)
                .await
                .expect("ft_witnessed_exact(S2)");

        assert!(
            !(s1_certified && s2_certified),
            "two same-height siblings certified over ONE snapshot with zero \
             equivocation — certification is not exclusive, so two finalized \
             floors can freeze at one height (the ucc 00e6a2e3 consensus halt). \
             s1_certified={s1_certified} s2_certified={s2_certified}"
        );
    })
    .await
}

/// The ucc ca7197d8 finalized fork, reduced to its oracle-level essence:
/// per-instant-sound certificates form on BOTH sides of a sibling fork at
/// DIFFERENT times, with full mutual knowledge and zero equivocation,
/// because honest spine choice between score-tied branches can flip
/// between certificates. Staged from the live shard's message genealogy:
/// A never leaves its sibling's spine; B flips onto A's branch, then onto
/// C's, then back (the live 8566/50a90f4a/812baf sequence); C builds its
/// own branch. Snapshot 1 (B and C on C's branch, mutually seen)
/// certifies C's block; snapshot 2 (A and B on A's branch, mutually
/// seen — B's C-era agreement hidden below the justification stopper)
/// certifies A's sibling. Each certificate alone is exclusive at its
/// instant; the SEQUENCE certifies two blocks whose states are
/// incompatible — the read-surface fork the floor guards then correctly
/// freeze on. This pin documents the oracle-level fact; the cure is that
/// honest fork choice must never mint the flip messages once a
/// certificate is visible (spine follows certification at GHOST ties).
#[tokio::test]
async fn sound_certificates_form_on_both_fork_sides_across_time() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        use casper::rust::safety::clique_oracle::FtThreshold;

        let va = generate_validator(Some("Temporal A"));
        let vb = generate_validator(Some("Temporal B"));
        let vc = generate_validator(Some("Temporal C"));
        let bonds = vec![
            Bond {
                validator: va.clone(),
                stake: 100,
            },
            Bond {
                validator: vb.clone(),
                stake: 100,
            },
            Bond {
                validator: vc.clone(),
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

        let creator_a = create_block(&bonds, &genesis, &va);
        let creator_b = create_block(&bonds, &genesis, &vb);
        let creator_c = create_block(&bonds, &genesis, &vc);
        let gj = HashMap::from([(&va, &genesis), (&vb, &genesis), (&vc, &genesis)]);

        // Three same-height siblings over the common base (79b00b / f5ba /
        // d89a in the specimen).
        let s_a = creator_a(&mut block_store, &mut block_dag_storage, &genesis, &gj);
        let s_b = creator_b(&mut block_store, &mut block_dag_storage, &genesis, &gj);
        let s_c = creator_c(&mut block_store, &mut block_dag_storage, &genesis, &gj);

        // A extends its own sibling forever (18a9…281e).
        let a1 = creator_a(
            &mut block_store,
            &mut block_dag_storage,
            &s_a,
            &HashMap::from([(&va, &s_a), (&vb, &s_b), (&vc, &s_c)]),
        );
        // C builds its own branch (9acc → 3560a642).
        let c1 = creator_c(
            &mut block_store,
            &mut block_dag_storage,
            &s_c,
            &HashMap::from([(&va, &s_a), (&vb, &s_b), (&vc, &s_c)]),
        );
        // B's first flip: onto A's sibling, knowing A's agreement (8566).
        let b1 = creator_b(
            &mut block_store,
            &mut block_dag_storage,
            &s_a,
            &HashMap::from([(&va, &s_a), (&vb, &s_b), (&vc, &s_c)]),
        );
        // C's join-era block on its own branch (3560a642's analog).
        let c2 = creator_c(
            &mut block_store,
            &mut block_dag_storage,
            &c1,
            &HashMap::from([(&va, &s_a), (&vb, &s_b), (&vc, &c1)]),
        );
        // B's second flip: onto C's branch, knowing C's chain (50a90f4a).
        let b2 = creator_b(
            &mut block_store,
            &mut block_dag_storage,
            &c2,
            &HashMap::from([(&va, &a1), (&vb, &b1), (&vc, &c2)]),
        );
        // C sees B agreeing on its branch (de0997/9b2a): mutual knowledge.
        let c3 = creator_c(
            &mut block_store,
            &mut block_dag_storage,
            &c2,
            &HashMap::from([(&va, &a1), (&vb, &b2), (&vc, &c2)]),
        );

        let thr = FtThreshold::from_f32_lossy(0.1);
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        // Snapshot 1 — the :53.5–:55.0 window: B and C spine through C's
        // branch, mutually seen; A holds its own sibling.
        let snap1: std::collections::BTreeMap<Validator, BlockHash> = [
            (va.clone(), a1.block_hash.clone()),
            (vb.clone(), b2.block_hash.clone()),
            (vc.clone(), c3.block_hash.clone()),
        ]
        .into_iter()
        .collect();
        let c2_certified_snap1 =
            CliqueOracle::ft_witnessed_exact(&c2.block_hash, &dag, &snap1, thr, false)
                .await
                .expect("ft(c2) at snapshot 1");
        let sa_certified_snap1 =
            CliqueOracle::ft_witnessed_exact(&s_a.block_hash, &dag, &snap1, thr, false)
                .await
                .expect("ft(s_a) at snapshot 1");

        // B's third flip: back onto A's branch (812baf); A mints having
        // seen it (mutual knowledge on the other side).
        let b3 = creator_b(
            &mut block_store,
            &mut block_dag_storage,
            &a1,
            &HashMap::from([(&va, &a1), (&vb, &b2), (&vc, &c3)]),
        );
        let a2 = creator_a(
            &mut block_store,
            &mut block_dag_storage,
            &a1,
            &HashMap::from([(&va, &a1), (&vb, &b3), (&vc, &c3)]),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        // Snapshot 2 — the :55.3–:55.8 window: A and B spine through A's
        // sibling, mutually seen; C still on its own branch.
        let snap2: std::collections::BTreeMap<Validator, BlockHash> = [
            (va.clone(), a2.block_hash.clone()),
            (vb.clone(), b3.block_hash.clone()),
            (vc.clone(), c3.block_hash.clone()),
        ]
        .into_iter()
        .collect();
        let sa_certified_snap2 =
            CliqueOracle::ft_witnessed_exact(&s_a.block_hash, &dag, &snap2, thr, false)
                .await
                .expect("ft(s_a) at snapshot 2");
        let c2_certified_snap2 =
            CliqueOracle::ft_witnessed_exact(&c2.block_hash, &dag, &snap2, thr, false)
                .await
                .expect("ft(c2) at snapshot 2");

        // Per-snapshot exclusivity holds — the fork is TEMPORAL.
        assert!(
            !sa_certified_snap1 && !c2_certified_snap2,
            "certificates must stay exclusive within one snapshot \
             (sa@1={sa_certified_snap1}, c2@2={c2_certified_snap2})"
        );
        // The class: both sides certify across time, soundly, fault-free.
        assert!(
            c2_certified_snap1 && sa_certified_snap2,
            "the ca7197d8 genealogy must certify C's branch at snapshot 1 \
             and A's sibling at snapshot 2 (c2@1={c2_certified_snap1}, \
             sa@2={sa_certified_snap2}) — if either fails, the staged \
             genealogy no longer mirrors the specimen"
        );
    })
    .await
}

/// A clique certificate must be MUTUALLY KNOWN agreement, not an
/// instantaneous coincidence (the ucc ca7197d8 finalized fork): here v1's
/// latest message T and v2's latest message m2 both spine through T, but
/// v1's knowledge of v2 (its justification, ov2) predates T entirely — v1
/// has never seen v2 agree. Before the agreement propagates, honest fork
/// choice is still free to move off T without any fault, so certifying at
/// this instant certifies nothing binding. Once v1 mints on m2 (its
/// justification for v2 now spines through T, and v2's for v1 already
/// does), the agreement is mutually known and the certificate is earned.
#[tokio::test]
async fn a_coincidence_never_mutually_seen_must_not_certify() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        use casper::rust::safety::clique_oracle::FtThreshold;

        let v1 = generate_validator(Some("Mutual V1"));
        let v2 = generate_validator(Some("Mutual V2"));
        let v3 = generate_validator(Some("Mutual V3"));
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

        let creator1 = create_block(&bonds, &genesis, &v1);
        let creator2 = create_block(&bonds, &genesis, &v2);

        let gj = HashMap::from([(&v1, &genesis), (&v2, &genesis), (&v3, &genesis)]);

        // v2's pre-target message: everything v1 will ever have seen of v2.
        let ov2 = creator2(&mut block_store, &mut block_dag_storage, &genesis, &gj);

        // The target T: v1 has seen ov2, which cannot spine through T.
        let t = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            &HashMap::from([(&v1, &genesis), (&v2, &ov2), (&v3, &genesis)]),
        );

        // v2 agrees on T and has seen v1's T — but v1 has never seen this.
        let m2 = creator2(
            &mut block_store,
            &mut block_dag_storage,
            &t,
            &HashMap::from([(&v1, &t), (&v2, &ov2), (&v3, &genesis)]),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let thr = FtThreshold::from_f32_lossy(0.1);

        // The coincidence instant: both latest messages spine through T,
        // mutual knowledge absent (v1's view of v2 is still ov2).
        let coincidence: std::collections::BTreeMap<Validator, BlockHash> = [
            (v1.clone(), t.block_hash.clone()),
            (v2.clone(), m2.block_hash.clone()),
            (v3.clone(), genesis.block_hash.clone()),
        ]
        .into_iter()
        .collect();
        let certified_at_coincidence =
            CliqueOracle::ft_witnessed_exact(&t.block_hash, &dag, &coincidence, thr, false)
                .await
                .expect("ft_witnessed_exact(T) at the coincidence");
        assert!(
            !certified_at_coincidence,
            "T certified while v1 has never seen v2 agree on it — a \
             transient spine coincidence is not a commitment, and \
             certifying it lets two sides of a sibling race finalize \
             incompatibly (the ucc ca7197d8 fork)"
        );

        // v1 mints on m2: its justification for v2 now spines through T —
        // the agreement is mutually known and must certify.
        let t2 = creator1(
            &mut block_store,
            &mut block_dag_storage,
            &m2,
            &HashMap::from([(&v1, &t), (&v2, &m2), (&v3, &genesis)]),
        );
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mutual: std::collections::BTreeMap<Validator, BlockHash> = [
            (v1.clone(), t2.block_hash.clone()),
            (v2.clone(), m2.block_hash.clone()),
            (v3.clone(), genesis.block_hash.clone()),
        ]
        .into_iter()
        .collect();
        let certified_at_mutual =
            CliqueOracle::ft_witnessed_exact(&t.block_hash, &dag, &mutual, thr, false)
                .await
                .expect("ft_witnessed_exact(T) at mutual knowledge");
        assert!(
            certified_at_mutual,
            "mutually-known agreement must certify — the mutual-knowledge \
             requirement must not over-restrict a settled clique"
        );
    })
    .await
}

/// Where the merge `B` records its STATE parent: the floor (the specimen —
/// `B` re-based past its own main parent and rejected that parent's
/// content) or its main parent (the ordinary merge that keeps it).
#[derive(Clone, Copy)]
enum StateParentOfMerge {
    Floor,
    MainParent,
}

struct SpineStateDivergence {
    dag: block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
    snapshot: std::collections::BTreeMap<Validator, BlockHash>,
    a: BlockHash,
    c: BlockHash,
}

/// Stages the live ucc specimen's geometry (session `bc35a3ad`, blocks
/// #534/#536/#537, five nodes wedged at #544):
///
/// ```text
///   genesis <- F                    the floor
///              |\
///              A S                  A(v1) carries x, S(v3) carries z
///              |/
///              B                    parents [A, S] — MAIN PARENT A
///              |
///              C                    C(v1)
///              |
///              D                    D(v3)
/// ```
///
/// `B` is a merge, so its state parent is its recorded `merge_base`, not
/// `parents[0]`. With [`StateParentOfMerge::Floor`] it re-bases onto `F`
/// and rejects `A`'s deploy `x`, applying only `S`'s `z` from scope —
/// exactly what the live #537 records against #536. Every block above `B`
/// inherits that state, so `x` is in no live state while `A` sits on every
/// latest message's main-parent spine.
///
/// The latest-message snapshot is `{v1: C, v2: B, v3: D}`, mirroring the
/// specimen's certifying snapshot (one validator still at the merge, two on
/// its descendants). `v3`'s self-justification chain leaves `A`'s spine at
/// `S`, so the `(v1,v3)` and `(v2,v3)` clique edges are refused and the
/// maximum clique is 200 of 300 — the FT 0.33333334 the live #536 carries.
fn stage_spine_state_divergence(
    block_store: &mut KeyValueBlockStore,
    block_dag_storage: &mut IndexedBlockDagStorage,
    state_parent_of_b: StateParentOfMerge,
) -> SpineStateDivergence {
    use casper::rust::util::construct_deploy::basic_processed_deploy;
    use models::rust::casper::protocol::casper_message::RejectedDeploy;

    use crate::helper::block_generator::MergeFacts;

    let v1 = generate_validator(Some("State Finality V1"));
    let v2 = generate_validator(Some("State Finality V2"));
    let v3 = generate_validator(Some("State Finality V3"));
    let bonds: Vec<Bond> = [&v1, &v2, &v3]
        .iter()
        .map(|v| Bond {
            validator: (*v).clone(),
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

    let mk = |creator: &Validator,
              parents: Vec<BlockHash>,
              justs: HashMap<Validator, BlockHash>,
              deploys: Option<Vec<ProcessedDeploy>>,
              merge_facts: Option<MergeFacts>,
              block_store: &mut KeyValueBlockStore,
              block_dag_storage: &mut IndexedBlockDagStorage| {
        crate::helper::block_generator::create_block(
            block_store,
            block_dag_storage,
            parents,
            &genesis,
            Some(creator.clone()),
            Some(bonds.clone()),
            Some(justs),
            deploys,
            None,
            None,
            None,
            None,
            None,
            merge_facts,
        )
    };

    let g = genesis.block_hash.clone();
    let gj: HashMap<Validator, BlockHash> = [&v1, &v2, &v3]
        .iter()
        .map(|v| ((*v).clone(), g.clone()))
        .collect();

    let f = mk(
        &v1,
        vec![g.clone()],
        gj.clone(),
        None,
        None,
        block_store,
        block_dag_storage,
    );

    let fj: HashMap<Validator, BlockHash> = [&v1, &v2, &v3]
        .iter()
        .map(|v| ((*v).clone(), f.block_hash.clone()))
        .collect();

    // The two branches off the floor: A carries the content that goes
    // missing, S carries the content B's merge keeps.
    let x = basic_processed_deploy(1, Some("root".to_string())).expect("deploy x");
    let z = basic_processed_deploy(2, Some("root".to_string())).expect("deploy z");
    let x_sig = x.deploy.sig.clone();
    let z_sig = z.deploy.sig.clone();

    let a = mk(
        &v1,
        vec![f.block_hash.clone()],
        fj.clone(),
        Some(vec![x]),
        None,
        block_store,
        block_dag_storage,
    );
    let s = mk(
        &v3,
        vec![f.block_hash.clone()],
        fj.clone(),
        Some(vec![z]),
        None,
        block_store,
        block_dag_storage,
    );

    let b_merge_base = match state_parent_of_b {
        StateParentOfMerge::Floor => f.block_hash.clone(),
        StateParentOfMerge::MainParent => a.block_hash.clone(),
    };
    // Re-based onto the floor, A's chain is rejected and only S's z is
    // applied; re-based onto A, nothing is rejected and A's x survives in
    // the state B inherits.
    let b_rejected = match state_parent_of_b {
        StateParentOfMerge::Floor => vec![RejectedDeploy {
            sig: x_sig.clone(),
            duplicate: false,
            carrier: a.block_hash.clone(),
        }],
        StateParentOfMerge::MainParent => Vec::new(),
    };
    let b = mk(
        &v2,
        vec![a.block_hash.clone(), s.block_hash.clone()],
        HashMap::from([
            (v1.clone(), a.block_hash.clone()),
            (v2.clone(), f.block_hash.clone()),
            (v3.clone(), s.block_hash.clone()),
        ]),
        None,
        Some(MergeFacts {
            merge_base: Some(b_merge_base.clone()),
            applied_from_scope: vec![z_sig.clone()],
            rejected_deploys: b_rejected,
        }),
        block_store,
        block_dag_storage,
    );

    let c = mk(
        &v1,
        vec![b.block_hash.clone()],
        HashMap::from([
            (v1.clone(), a.block_hash.clone()),
            (v2.clone(), b.block_hash.clone()),
            (v3.clone(), s.block_hash.clone()),
        ]),
        None,
        None,
        block_store,
        block_dag_storage,
    );
    let d = mk(
        &v3,
        vec![c.block_hash.clone()],
        HashMap::from([
            (v1.clone(), c.block_hash.clone()),
            (v2.clone(), b.block_hash.clone()),
            (v3.clone(), s.block_hash.clone()),
        ]),
        None,
        None,
        block_store,
        block_dag_storage,
    );

    // Staging integrity: the whole point of the fixture is what the BODIES
    // record, and a silently-empty merge base would make the geometry
    // ordinary without failing any assertion below.
    let stored_a = block_store
        .get(&a.block_hash)
        .expect("read A")
        .expect("A is stored");
    assert!(
        stored_a
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == x_sig && !pd.is_failed),
        "staging: A must carry x as a non-failed deploy — it is the settled \
         content whose absence downstream is the whole specimen"
    );
    let stored_b = block_store
        .get(&b.block_hash)
        .expect("read B")
        .expect("B is stored");
    assert_eq!(
        stored_b.body.merge_base, b_merge_base,
        "staging: B must record the intended state parent"
    );
    assert_eq!(
        stored_b.header.parents_hash_list.first(),
        Some(&a.block_hash),
        "staging: A must be B's MAIN parent — spine agreement is what makes \
         A certify today"
    );
    assert!(
        stored_b.body.applied_from_scope.contains(&z_sig),
        "staging: B must apply S's z from scope"
    );

    let dag = block_dag_storage
        .get_representation()
        .expect("dag representation");
    let snapshot: std::collections::BTreeMap<Validator, BlockHash> = [
        (v1, c.block_hash.clone()),
        (v2, b.block_hash.clone()),
        (v3, d.block_hash.clone()),
    ]
    .into_iter()
    .collect();

    SpineStateDivergence {
        dag,
        snapshot,
        a: a.block_hash,
        c: c.block_hash,
    }
}

/// WHY the oracle may read agreement off the MAIN-PARENT SPINE alone.
///
/// `agree` asks only `dag.is_in_main_chain(target, latest_message)`. That is
/// sound exactly while every merge keeps its main parent's content, because
/// then spine descent implies state containment. It is NOT sound on its own:
/// this test stages the one geometry where the two part company — `A` carrying
/// a deploy, `B` with `parents[0] = A` but `merge_base` = the floor below `A`
/// and `A`'s deploy rejected — and shows the oracle certifying `A` at
/// FT 0.33333334 with all 300 stake agreeing, while no state on the DAG holds
/// `A`'s content.
///
/// 0.33333334 is not a chosen number: it is the fault tolerance the live #536
/// carries on shard `bc35a3ad`, reproduced here from 300 agreeing and a 200
/// clique. On that shard five nodes finalized three mutually non-contained
/// floors (#536 on two validators, #537 on two, #539 on one) and every propose
/// was refused thereafter.
///
/// The geometry is staged DIRECTLY here because a merge can no longer build
/// it: `conflict_resolution_never_rejects_main_parent_content`
/// (dag_merger.rs) pins the main parent's chains against rejection, and a
/// block whose rejection set disagrees with the validator's recomputation is
/// `invalid_rejected_deploy`. So this is a standing pin on the ORACLE's
/// precondition, not a live defect — if the merge rule is ever weakened, the
/// oracle silently goes back to certifying blocks nothing holds, and the
/// failure resurfaces here rather than on a wedged shard.
#[tokio::test]
async fn spine_agreement_is_sound_only_because_merges_keep_main_parent_content() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        use casper::rust::safety::clique_oracle::FtThreshold;

        let staged = stage_spine_state_divergence(
            &mut block_store,
            &mut block_dag_storage,
            StateParentOfMerge::Floor,
        );
        let thr = FtThreshold::from_f32_lossy(0.1);

        assert!(
            staged
                .dag
                .is_in_main_chain(&staged.a, &staged.c)
                .expect("main-chain membership"),
            "staging: A must be on the spine of the latest messages"
        );

        let ft = CliqueOracle::ft_witnessed(&staged.a, &staged.dag, &staged.snapshot)
            .await
            .expect("ft_witnessed(A)");
        assert_eq!(
            ft, 0.33333334,
            "the oracle reads full agreement off the spine — 300 agreeing, a 200 \
             clique — with no regard for whether any state holds A's content. This \
             is the live #536's own recorded fault tolerance"
        );

        let decision =
            CliqueOracle::ft_witnessed_exact(&staged.a, &staged.dag, &staged.snapshot, thr, false)
                .await
                .expect("ft_witnessed_exact(A)");
        assert!(
            decision,
            "A certifies on spine agreement alone. Nothing in the oracle prevents \
             this — only the merge rule that keeps A's content in every descendant's \
             state makes spine agreement equal state agreement"
        );
    })
    .await
}

/// The paired pin for
/// [`spine_agreement_is_sound_only_because_merges_keep_main_parent_content`]:
/// the same DAG with `B` built as an ordinary merge — `merge_base = A`,
/// nothing rejected — certifies `A` too.
///
/// The pair together is the actual point. Both geometries produce the SAME
/// oracle verdict, so the oracle cannot distinguish the state that holds `A`
/// from the state that dropped it. Whether a certified block's content
/// survives is settled entirely by the merge rule, never by finality — which
/// is why the rule is enforced where content is chosen (conflict resolution)
/// rather than where agreement is counted.
#[tokio::test]
async fn an_ordinary_merge_still_certifies_the_main_parent_it_kept() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        use casper::rust::safety::clique_oracle::FtThreshold;

        let staged = stage_spine_state_divergence(
            &mut block_store,
            &mut block_dag_storage,
            StateParentOfMerge::MainParent,
        );
        let thr = FtThreshold::from_f32_lossy(0.1);

        let decision =
            CliqueOracle::ft_witnessed_exact(&staged.a, &staged.dag, &staged.snapshot, thr, false)
                .await
                .expect("ft_witnessed_exact(A)");
        assert!(
            decision,
            "an ordinary merge keeps its main parent's content and A finalizes — \
             the same verdict the divergent geometry gets, which is why the merge \
             rule and not the oracle is what makes certification meaningful"
        );
    })
    .await
}
