//! The LFB decision over the live view (`floor::floor_of_view`) — the one
//! finality clock. Re-targets the retired Finalizer's contract pins: the
//! stagings are unchanged; the decision they pin is now the floor-of-view
//! derivation (exact `>= θ` witness + capture-gated advancement) instead of
//! the Finalizer's discontinuous highest-FT search.

use std::collections::HashMap;
use std::time::Instant;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::finality::floor::{floor_of_view, Floor};
use casper::rust::safety::clique_oracle::FtThreshold;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::validator::Validator;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{create_block, create_genesis_block};
use crate::helper::block_util::generate_validator;

fn create_block_creator<'a>(
    bonds: &'a [Bond],
    genesis: &'a BlockMessage,
    creator: &'a Validator,
) -> impl Fn(
    &mut KeyValueBlockStore,
    &mut IndexedBlockDagStorage,
    Vec<&BlockMessage>,
    &HashMap<&Validator, &BlockMessage>,
) -> BlockMessage
       + 'a {
    move |block_store, block_dag_storage, parents, justifications| {
        let parent_hashes: Vec<BlockHash> = parents
            .iter()
            .map(|parent| parent.block_hash.clone())
            .collect();

        let justifications: HashMap<Validator, BlockHash> = justifications
            .iter()
            .map(|(validator, block_message)| {
                ((*validator).clone(), block_message.block_hash.clone())
            })
            .collect();

        create_block(
            block_store,
            block_dag_storage,
            parent_hashes,
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
}

fn at(block: &BlockMessage) -> Floor {
    Floor {
        hash: block.block_hash.clone(),
        block_number: block.body.state.block_number,
    }
}

//   *  *            b8 b9
//   *               b7         <- must NOT become the LFB yet
//   *  *  *  *  *   b2 b3 b4 b5 b6
//   *               b1         <- becomes the LFB
//   v1 v2 v3 v4 v5
#[tokio::test]
async fn floor_of_view_holds_until_witnessed_then_advances() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("Validator 1")),
            generate_validator(Some("Validator 2")),
            generate_validator(Some("Validator 3")),
            generate_validator(Some("Validator 4")),
            generate_validator(Some("Validator 5")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|v| Bond {
                validator: v.clone(),
                stake: 3,
            })
            .collect();

        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let creator1 = create_block_creator(&bonds, &genesis, &validators[0]);
        let creator2 = create_block_creator(&bonds, &genesis, &validators[1]);
        let creator3 = create_block_creator(&bonds, &genesis, &validators[2]);
        let creator4 = create_block_creator(&bonds, &genesis, &validators[3]);
        let creator5 = create_block_creator(&bonds, &genesis, &validators[4]);

        let genesis_justification = HashMap::from([
            (&validators[0], &genesis),
            (&validators[1], &genesis),
            (&validators[2], &genesis),
            (&validators[3], &genesis),
            (&validators[4], &genesis),
        ]);

        let b1 = creator1(
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justification,
        );
        let b2 = creator1(
            &mut store,
            &mut dag_store,
            vec![&b1],
            &genesis_justification,
        );
        let b3 = creator2(
            &mut store,
            &mut dag_store,
            vec![&b1],
            &genesis_justification,
        );
        let b4 = creator3(
            &mut store,
            &mut dag_store,
            vec![&b1],
            &genesis_justification,
        );
        let b5 = creator4(
            &mut store,
            &mut dag_store,
            vec![&b1],
            &genesis_justification,
        );
        let b6 = creator5(
            &mut store,
            &mut dag_store,
            vec![&b1],
            &genesis_justification,
        );

        let thr = FtThreshold::from_f32_lossy(0.1);
        let dag = dag_store.get_representation().expect("dag representation");
        let advanced = floor_of_view(&dag, &store, &at(&genesis), thr)
            .await
            .expect("floor of view");
        assert_eq!(
            advanced.as_ref().map(|f| &f.hash),
            Some(&b1.block_hash),
            "b1 is witnessed by every latest message and becomes the LFB"
        );

        /* next layer — b7 on b2's chain (single-parent: capture walks derive
         * its base from the header; the retired staging's multi-parent b7
         * only "advanced" under the Finalizer's θ=-1, which bypassed the
         * clique's mutual-visibility requirement entirely) */
        let b7 = creator1(
            &mut store,
            &mut dag_store,
            vec![&b2],
            &HashMap::from([
                (&validators[0], &b2),
                (&validators[1], &b3),
                (&validators[2], &b4),
                (&validators[3], &b4),
                (&validators[4], &b5),
            ]),
        );

        // A first layer above b7 — descent alone is not witness (no mutual
        // visibility of the agreement yet): the LFB must hold at b1.
        let above_b7 = HashMap::from([
            (&validators[0], &b7),
            (&validators[1], &b3),
            (&validators[2], &b4),
            (&validators[3], &b5),
            (&validators[4], &b6),
        ]);
        let c1 = creator1(&mut store, &mut dag_store, vec![&b7], &above_b7);
        let c2 = creator2(&mut store, &mut dag_store, vec![&b7], &above_b7);
        let c3 = creator3(&mut store, &mut dag_store, vec![&b7], &above_b7);
        let c4 = creator4(&mut store, &mut dag_store, vec![&b7], &above_b7);
        let c5 = creator5(&mut store, &mut dag_store, vec![&b7], &above_b7);

        let dag = dag_store.get_representation().expect("dag representation");
        let held = floor_of_view(&dag, &store, &at(&b1), thr)
            .await
            .expect("floor of view");
        assert_eq!(
            held, None,
            "descent without mutual visibility does not witness b7 — the LFB \
             holds at b1"
        );

        // A second layer whose justifications cite the whole first layer:
        // every validator now SEES every other validator's agreement on b7
        // — the clique forms and the LFB advances onto b7's chain.
        let mutual = HashMap::from([
            (&validators[0], &c1),
            (&validators[1], &c2),
            (&validators[2], &c3),
            (&validators[3], &c4),
            (&validators[4], &c5),
        ]);
        creator1(&mut store, &mut dag_store, vec![&c1], &mutual);
        creator2(&mut store, &mut dag_store, vec![&c2], &mutual);
        creator3(&mut store, &mut dag_store, vec![&c3], &mutual);
        creator4(&mut store, &mut dag_store, vec![&c4], &mutual);
        creator5(&mut store, &mut dag_store, vec![&c5], &mutual);

        let dag = dag_store.get_representation().expect("dag representation");
        let advanced = floor_of_view(&dag, &store, &at(&b1), thr)
            .await
            .expect("floor of view")
            .expect("mutual visibility witnesses b7's chain — the LFB advances");
        assert!(
            advanced.block_number >= b7.body.state.block_number
                && dag
                    .is_dag_ancestor(&b7.block_hash, &advanced.hash)
                    .expect("ancestry"),
            "the advanced LFB must sit at-or-above b7 on its chain \
             (advanced {}#{})",
            hex::encode(&advanced.hash[..8]),
            advanced.block_number,
        );

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("Test should complete successfully");
}

/// A candidate already marked directly-finalized ahead of the LFB pointer
/// is still the view's derived floor — the pointer catches up to it.
#[tokio::test]
async fn floor_of_view_advances_onto_the_already_finalized_candidate() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("Finalized Candidate Validator 1")),
            generate_validator(Some("Finalized Candidate Validator 2")),
            generate_validator(Some("Finalized Candidate Validator 3")),
            generate_validator(Some("Finalized Candidate Validator 4")),
            generate_validator(Some("Finalized Candidate Validator 5")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|validator| Bond {
                validator: validator.clone(),
                stake: 3,
            })
            .collect();
        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let creators = [
            create_block_creator(&bonds, &genesis, &validators[0]),
            create_block_creator(&bonds, &genesis, &validators[1]),
            create_block_creator(&bonds, &genesis, &validators[2]),
            create_block_creator(&bonds, &genesis, &validators[3]),
            create_block_creator(&bonds, &genesis, &validators[4]),
        ];
        let genesis_justifications = HashMap::from([
            (&validators[0], &genesis),
            (&validators[1], &genesis),
            (&validators[2], &genesis),
            (&validators[3], &genesis),
            (&validators[4], &genesis),
        ]);
        let candidate = creators[0](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        for creator in &creators {
            creator(
                &mut store,
                &mut dag_store,
                vec![&candidate],
                &genesis_justifications,
            );
        }
        dag_store
            .record_directly_finalized(candidate.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .expect("record candidate finalized");
        let dag = dag_store.get_representation().expect("dag representation");
        let advanced = floor_of_view(
            &dag,
            &store,
            &at(&genesis),
            FtThreshold::from_f32_lossy(0.1),
        )
        .await
        .expect("floor of view");
        assert_eq!(
            advanced.as_ref().map(|f| &f.hash),
            Some(&candidate.block_hash)
        );

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("validation fixture");
}

/// Manual diagnostic (run with --ignored): floor-of-view cost at growing
/// heights over a STALE-justification chain — each validator cites only its
/// own latest message, the historical growth-feedback geometry for the
/// retired Finalizer's agreement aggregation. The floor path's persisted
/// caches must keep the cost flat; a super-linear trend here is a
/// regression finding to report.
#[tokio::test]
#[ignore = "diagnostic: run manually for floor-of-view growth feedback"]
async fn floor_of_view_growth_feedback_loop_stale_justification_chain() {
    shared::rust::tracing_init::init_for_tests();
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("Growth Validator 1")),
            generate_validator(Some("Growth Validator 2")),
            generate_validator(Some("Growth Validator 3")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|v| Bond {
                validator: v.clone(),
                stake: 10,
            })
            .collect();

        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let creator1 = create_block_creator(&bonds, &genesis, &validators[0]);
        let creator2 = create_block_creator(&bonds, &genesis, &validators[1]);
        let creator3 = create_block_creator(&bonds, &genesis, &validators[2]);

        let checkpoints = [24usize, 48usize, 96usize];
        let mut timing_samples: Vec<(usize, u128)> = Vec::with_capacity(checkpoints.len());
        let mut latest_by_validator = [genesis.clone(), genesis.clone(), genesis.clone()];

        for height in 1..=checkpoints[checkpoints.len() - 1] {
            let creator_index = (height - 1) % validators.len();

            let mut justifications: HashMap<&Validator, &BlockMessage> = HashMap::new();
            for (idx, validator) in validators.iter().enumerate() {
                let justification = if idx == creator_index {
                    &latest_by_validator[idx]
                } else {
                    &genesis
                };
                justifications.insert(validator, justification);
            }

            let parent = &latest_by_validator[creator_index];
            let next_block = match creator_index {
                0 => creator1(&mut store, &mut dag_store, vec![parent], &justifications),
                1 => creator2(&mut store, &mut dag_store, vec![parent], &justifications),
                2 => creator3(&mut store, &mut dag_store, vec![parent], &justifications),
                _ => unreachable!("creator_index should be in [0, 2]"),
            };
            latest_by_validator[creator_index] = next_block;

            if checkpoints.contains(&height) {
                let dag = dag_store.get_representation().expect("dag representation");
                let started = Instant::now();
                let _ = floor_of_view(
                    &dag,
                    &store,
                    &at(&genesis),
                    FtThreshold::from_f32_lossy(0.1),
                )
                .await
                .expect("floor of view");
                timing_samples.push((height, started.elapsed().as_millis()));
            }
        }

        assert_eq!(timing_samples.len(), checkpoints.len());
        for (height, elapsed_ms) in timing_samples {
            tracing::info!(
                target: "f1r3fly.finalizer",
                height,
                floor_of_view_ms = elapsed_ms,
                "floor-of-view growth feedback sample (stale-justification chain)"
            );
        }

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("growth feedback test should complete successfully");
}
