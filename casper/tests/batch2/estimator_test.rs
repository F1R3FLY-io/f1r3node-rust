// See casper/src/test/scala/coop/rchain/casper/batch2/EstimatorTest.scala

use std::collections::HashMap;

use casper::rust::estimator::Estimator;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::Bond;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{create_block, create_genesis_block};
use crate::helper::block_util::generate_validator;

// Macro to create justifications HashMap without excessive cloning
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

// Helper function to reduce cloning
fn create_test_block(
    block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
    block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
    parents: &[models::rust::block_hash::BlockHash],
    genesis: &models::rust::casper::protocol::casper_message::BlockMessage,
    creator: &models::rust::validator::Validator,
    bonds: &[Bond],
    justifications: HashMap<
        models::rust::validator::Validator,
        models::rust::block_hash::BlockHash,
    >,
) -> models::rust::casper::protocol::casper_message::BlockMessage {
    create_block(
        block_store,
        block_dag_storage,
        parents.to_vec(),
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

#[tokio::test]
async fn estimator_on_empty_latest_messages_should_return_the_genesis_regardless_of_dag() {
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

        let b2 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&genesis.block_hash),
            &genesis,
            &v2,
            &bonds,
            justifications!(v1 => genesis.block_hash, v2 => genesis.block_hash),
        );

        let b3 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&genesis.block_hash),
            &genesis,
            &v1,
            &bonds,
            justifications!(v1 => genesis.block_hash, v2 => genesis.block_hash),
        );

        let b4 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b2.block_hash),
            &genesis,
            &v2,
            &bonds,
            justifications!(v1 => genesis.block_hash, v2 => b2.block_hash),
        );

        let b5 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b2.block_hash),
            &genesis,
            &v1,
            &bonds,
            justifications!(v1 => b3.block_hash, v2 => b2.block_hash),
        );

        let _b6 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b4.block_hash),
            &genesis,
            &v2,
            &bonds,
            justifications!(v1 => b5.block_hash, v2 => b4.block_hash),
        );

        let b7 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b4.block_hash),
            &genesis,
            &v1,
            &bonds,
            justifications!(v1 => b5.block_hash, v2 => b4.block_hash),
        );

        let _b8 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b7.block_hash),
            &genesis,
            &v1,
            &bonds,
            justifications!(v1 => b7.block_hash, v2 => b4.block_hash),
        );

        let mut dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let estimator = Estimator::apply();
        let forkchoice = estimator
            .tips_with_latest_messages(
                &mut dag,
                &BlockMetadata::from_block(&genesis, false, None, None),
                HashMap::new(),
                i32::MAX,
                None,
            )
            .await
            .unwrap();

        assert_eq!(forkchoice.tips[0], genesis.block_hash);
    })
    .await
}

// See https://docs.google.com/presentation/d/1znz01SF1ljriPzbMoFV0J127ryPglUYLFyhvsb-ftQk/edit?usp=sharing slide 29 for diagram
#[tokio::test]
async fn estimator_on_simple_dag_should_return_the_appropriate_score_map_and_forkchoice() {
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

        let b2 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&genesis.block_hash),
            &genesis,
            &v2,
            &bonds,
            justifications!(v1 => genesis.block_hash, v2 => genesis.block_hash),
        );

        let b3 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&genesis.block_hash),
            &genesis,
            &v1,
            &bonds,
            justifications!(v1 => genesis.block_hash, v2 => genesis.block_hash),
        );

        let b4 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b2.block_hash),
            &genesis,
            &v2,
            &bonds,
            justifications!(v1 => genesis.block_hash, v2 => b2.block_hash),
        );

        let b5 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b2.block_hash),
            &genesis,
            &v1,
            &bonds,
            justifications!(v1 => b3.block_hash, v2 => b2.block_hash),
        );

        let b6 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b4.block_hash),
            &genesis,
            &v2,
            &bonds,
            justifications!(v1 => b5.block_hash, v2 => b4.block_hash),
        );

        let b7 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b4.block_hash),
            &genesis,
            &v1,
            &bonds,
            justifications!(v1 => b5.block_hash, v2 => b4.block_hash),
        );

        let b8 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b7.block_hash),
            &genesis,
            &v1,
            &bonds,
            justifications!(v1 => b7.block_hash, v2 => b4.block_hash),
        );

        let mut dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let latest_blocks = HashMap::from([
            (v1.clone(), b8.block_hash.clone()),
            (v2.clone(), b6.block_hash.clone()),
        ]);

        let estimator = Estimator::apply();
        let forkchoice = estimator
            .tips_with_latest_messages(
                &mut dag,
                &BlockMetadata::from_block(&genesis, false, None, None),
                latest_blocks,
                i32::MAX,
                None,
            )
            .await
            .unwrap();

        assert_eq!(forkchoice.tips[0], b6.block_hash);
        assert_eq!(forkchoice.tips[1], b8.block_hash);
    })
    .await
}

// See [[/docs/casper/images/no_finalizable_block_mistake_with_no_disagreement_check.png]]
#[tokio::test]
async fn estimator_on_flipping_forkchoice_dag_should_return_the_appropriate_score_map_and_forkchoice(
) {
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

        let b2 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&genesis.block_hash),
            &genesis,
            &v2,
            &bonds,
            justifications!(v1 => genesis.block_hash, v2 => genesis.block_hash, v3 => genesis.block_hash),
        );

        let b3 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&genesis.block_hash),
            &genesis,
            &v1,
            &bonds,
            justifications!(v1 => genesis.block_hash, v2 => genesis.block_hash, v3 => genesis.block_hash),
        );

        let b4 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b2.block_hash),
            &genesis,
            &v3,
            &bonds,
            justifications!(v1 => genesis.block_hash, v2 => b2.block_hash, v3 => b2.block_hash),
        );

        let b5 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b3.block_hash),
            &genesis,
            &v2,
            &bonds,
            justifications!(v1 => b3.block_hash, v2 => b2.block_hash, v3 => genesis.block_hash),
        );

        let b6 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b4.block_hash),
            &genesis,
            &v1,
            &bonds,
            justifications!(v1 => b3.block_hash, v2 => b2.block_hash, v3 => b4.block_hash),
        );

        let b7 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b5.block_hash),
            &genesis,
            &v3,
            &bonds,
            justifications!(v1 => b3.block_hash, v2 => b5.block_hash, v3 => b4.block_hash),
        );

        let b8 = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            std::slice::from_ref(&b6.block_hash),
            &genesis,
            &v2,
            &bonds,
            justifications!(v1 => b6.block_hash, v2 => b5.block_hash, v3 => b4.block_hash),
        );

        let mut dag = block_dag_storage.get_representation().expect("dag representation");
        let latest_blocks = HashMap::from([
            (v1.clone(), b6.block_hash.clone()),
            (v2.clone(), b8.block_hash.clone()),
            (v3.clone(), b7.block_hash.clone()),
        ]);

        let estimator = Estimator::apply();
        let forkchoice = estimator
            .tips_with_latest_messages(&mut dag, &BlockMetadata::from_block(&genesis, false, None, None), latest_blocks, i32::MAX, None)
            .await
            .unwrap();

        assert_eq!(forkchoice.tips[0], b8.block_hash);
        assert_eq!(forkchoice.tips[1], b7.block_hash);
    })
    .await
}

/// The dense regime: every height carries one block per validator, each citing
/// all three of the height below, so the LCA walk's candidate set never
/// collapses and descends to its bound. Bounded at genesis the scored band
/// grows with chain length — the cost the colleague's shard measured climbing
/// 192 -> 1107 ms over 2.5 days. Bounded at the fork-choice floor it is flat in
/// chain length, and the head is the same either way.
#[tokio::test]
async fn estimator_scored_from_a_floor_is_flat_in_chain_length_on_a_dense_dag() {
    let short = dense_regime_scores(10).await;
    let long = dense_regime_scores(30).await;

    assert_eq!(short.head_from_floor, short.head_from_genesis);
    assert_eq!(long.head_from_floor, long.head_from_genesis);
    assert!(
        long.scored_from_genesis > short.scored_from_genesis,
        "genesis-bounded scoring grows with chain length: {} at N=10 vs {} at N=30",
        short.scored_from_genesis,
        long.scored_from_genesis
    );
    assert_eq!(
        short.scored_from_floor, long.scored_from_floor,
        "floor-bounded scoring is flat in chain length"
    );
}

struct DenseRegimeScores {
    scored_from_genesis: usize,
    scored_from_floor: usize,
    head_from_genesis: prost::bytes::Bytes,
    head_from_floor: prost::bytes::Bytes,
}

/// `heights` heights of a fully merged 3-wide DAG; the floor is two heights
/// below the top, where a live shard's finalized floor trails the frontier.
async fn dense_regime_scores(heights: usize) -> DenseRegimeScores {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let validators = [
            generate_validator(Some("Validator One")),
            generate_validator(Some("Validator Two")),
            generate_validator(Some("Validator Three")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|v| Bond {
                validator: v.clone(),
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

        let mut level: Vec<prost::bytes::Bytes> = vec![genesis.block_hash.clone()];
        let mut latest: HashMap<_, _> = validators
            .iter()
            .map(|v| (v.clone(), genesis.block_hash.clone()))
            .collect();
        let mut floor = BlockMetadata::from_block(&genesis, false, None, None);

        for height in 0..heights {
            let parents = level.clone();
            let mut next = Vec::with_capacity(validators.len());
            for (index, validator) in validators.iter().enumerate() {
                let block = create_test_block(
                    &mut block_store,
                    &mut block_dag_storage,
                    &parents,
                    &genesis,
                    validator,
                    &bonds,
                    latest.clone(),
                );
                next.push(block.block_hash.clone());
                // The main parent is parents[0], so the spine runs through the
                // first validator's blocks; a floor off that spine is not a
                // floor any node would derive.
                if index == 0 && height + 2 == heights {
                    floor = BlockMetadata::from_block(&block, false, None, None);
                }
            }
            for (validator, hash) in validators.iter().zip(next.iter()) {
                latest.insert(validator.clone(), hash.clone());
            }
            level = next;
        }

        // One block on top of the widest level. Three equal-stake validators on
        // a fully merged level tie, and a tie is broken by hash, so without a
        // single top block "same head" would assert on the tiebreak rather than
        // on the bound under test.
        let head = create_test_block(
            &mut block_store,
            &mut block_dag_storage,
            &level,
            &genesis,
            &validators[0],
            &bonds,
            latest.clone(),
        );
        latest.insert(validators[0].clone(), head.block_hash.clone());

        let mut dag = block_dag_storage.get_representation().expect("dag");
        let estimator = Estimator::apply();
        let from_genesis = estimator
            .tips_with_latest_messages(
                &mut dag,
                &BlockMetadata::from_block(&genesis, false, None, None),
                latest.clone(),
                i32::MAX,
                None,
            )
            .await
            .expect("fork choice from genesis");
        let from_floor = estimator
            .tips_with_latest_messages(&mut dag, &floor, latest, i32::MAX, None)
            .await
            .expect("fork choice from the floor");

        let floor_parent = floor.parents.first().expect("floor has a main parent");
        for scored in from_floor.scores.keys() {
            assert!(
                scored == floor_parent
                    || dag
                        .lookup_unsafe(scored)
                        .expect("scored block")
                        .block_number
                        >= floor.block_number,
                "nothing below the floor is scored except the floor's own main parent"
            );
        }

        DenseRegimeScores {
            scored_from_genesis: from_genesis.scores.len(),
            scored_from_floor: from_floor.scores.len(),
            head_from_genesis: from_genesis.tips.first().expect("a head").clone(),
            head_from_floor: from_floor.tips.first().expect("a head").clone(),
        }
    })
    .await
}
