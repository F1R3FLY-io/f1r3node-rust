//! Lifecycle contract for `CursorSlot` — the shared resumable floor cursor's
//! wrapper that decides, on each snapshot, whether to resume, reset (and count
//! an invalidation), or freeze. The 50-validator resumption test in
//! `floor_of_view_test` pins the cursor's convergence to the reference
//! derivation; these tests pin the invalidation-cycle protection
//! (`MAX_CONSECUTIVE_INVALIDATIONS`) and the ready-clears-slot invariant.

use std::collections::HashMap;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::finality::floor::Floor;
use casper::rust::finality::floor_runner::{
    Advance, Budget, CursorSlot, Preparation, MAX_CONSECUTIVE_INVALIDATIONS,
};
use casper::rust::safety::clique_oracle::FtThreshold;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::validator::Validator;
use shared::rust::dag::observation_work::NoopWork;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{create_block, create_genesis_block};
use crate::helper::block_util::generate_validator;

fn creator_for<'a>(
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
    move |store, dag_store, parents, justifications| {
        let parent_hashes: Vec<BlockHash> = parents.iter().map(|p| p.block_hash.clone()).collect();
        let justifications: HashMap<Validator, BlockHash> = justifications
            .iter()
            .map(|(v, b)| ((*v).clone(), b.block_hash.clone()))
            .collect();
        create_block(
            store,
            dag_store,
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

#[tokio::test]
async fn prepare_fresh_initializes_a_cursor_on_an_empty_slot() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("V1")),
            generate_validator(Some("V2")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|v| Bond {
                validator: v.clone(),
                stake: 1,
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

        let dag = dag_store.get_representation().expect("dag representation");
        let thr = FtThreshold::from_f32_lossy(0.1);
        let mut slot = CursorSlot::default();

        let preparation = slot
            .prepare(dag, at(&genesis), thr)
            .expect("prepare on an empty slot");
        assert_eq!(preparation, Preparation::Fresh);
        assert!(
            slot.cursor().is_some(),
            "fresh preparation must install a cursor"
        );
        assert_eq!(slot.invalidations(), 0);

        let _ = store;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("fresh preparation test should complete");
}

#[tokio::test]
async fn prepare_resumed_keeps_the_same_cursor_when_the_snapshot_is_unchanged() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("V1")),
            generate_validator(Some("V2")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|v| Bond {
                validator: v.clone(),
                stake: 1,
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
        // Build a two-block chain so the cursor has real work to do under
        // Budget::Steps(1); a lone-genesis dag converges in one step and would
        // clear the slot before we can observe resumption.
        let creator1 = creator_for(&bonds, &genesis, &validators[0]);
        let justifications: HashMap<&Validator, &BlockMessage> =
            HashMap::from([(&validators[0], &genesis), (&validators[1], &genesis)]);
        let _b1 = creator1(&mut store, &mut dag_store, vec![&genesis], &justifications);

        let dag = dag_store.get_representation().expect("dag representation");
        let thr = FtThreshold::from_f32_lossy(0.1);
        let mut slot = CursorSlot::default();
        slot.prepare(dag.clone(), at(&genesis), thr)
            .expect("fresh preparation");
        // Advance one step so the cursor has observable state (steps_taken > 0)
        // and is not yet Done.
        slot.advance(&NoopWork, &store, Budget::Steps(1)).await;
        let steps_before = slot
            .cursor()
            .expect("cursor present mid-derivation")
            .steps_taken();
        assert!(
            steps_before > 0,
            "a Steps(1) advance must take at least one step"
        );

        let preparation = slot
            .prepare(dag, at(&genesis), thr)
            .expect("re-prepare with unchanged snapshot");
        assert_eq!(preparation, Preparation::Resumed);
        assert_eq!(
            slot.cursor().expect("cursor still present").steps_taken(),
            steps_before,
            "resumed preparation must not replace the cursor",
        );
        assert_eq!(
            slot.invalidations(),
            0,
            "resume never counts an invalidation"
        );

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("resumed preparation test should complete");
}

#[tokio::test]
async fn prepare_reset_counts_an_invalidation_when_the_snapshot_changes() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("V1")),
            generate_validator(Some("V2")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|v| Bond {
                validator: v.clone(),
                stake: 1,
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
        let creator1 = creator_for(&bonds, &genesis, &validators[0]);
        let justifications: HashMap<&Validator, &BlockMessage> =
            HashMap::from([(&validators[0], &genesis), (&validators[1], &genesis)]);

        let dag_before = dag_store.get_representation().expect("initial dag");
        let thr = FtThreshold::from_f32_lossy(0.1);
        let mut slot = CursorSlot::default();
        slot.prepare(dag_before, at(&genesis), thr)
            .expect("fresh preparation");
        assert_eq!(slot.invalidations(), 0);

        // Mutate the dag so the snapshot id changes.
        let _b1 = creator1(&mut store, &mut dag_store, vec![&genesis], &justifications);
        let dag_after = dag_store.get_representation().expect("dag after block 1");

        let preparation = slot
            .prepare(dag_after, at(&genesis), thr)
            .expect("re-prepare after dag mutation");
        assert_eq!(preparation, Preparation::Reset);
        assert_eq!(
            slot.invalidations(),
            1,
            "a reset must count one invalidation",
        );
        assert!(slot.cursor().is_some(), "reset installs a fresh cursor",);

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("reset preparation test should complete");
}

#[tokio::test]
async fn prepare_freezes_after_the_max_consecutive_invalidations() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("V1")),
            generate_validator(Some("V2")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|v| Bond {
                validator: v.clone(),
                stake: 1,
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
        let creator1 = creator_for(&bonds, &genesis, &validators[0]);
        let justifications: HashMap<&Validator, &BlockMessage> =
            HashMap::from([(&validators[0], &genesis), (&validators[1], &genesis)]);

        let thr = FtThreshold::from_f32_lossy(0.1);
        let mut slot = CursorSlot::default();

        // Install the initial cursor.
        let dag_initial = dag_store.get_representation().expect("initial dag");
        slot.prepare(dag_initial, at(&genesis), thr)
            .expect("fresh preparation");
        assert_eq!(slot.invalidations(), 0);

        // Issue MAX_CONSECUTIVE_INVALIDATIONS resets by mutating the dag between
        // each prepare call. Each prepare returns Reset until the cap is reached.
        for round in 0..MAX_CONSECUTIVE_INVALIDATIONS {
            let _ = creator1(&mut store, &mut dag_store, vec![&genesis], &justifications);
            let dag = dag_store.get_representation().expect("dag mutation");
            let preparation = slot
                .prepare(dag, at(&genesis), thr)
                .expect("reset preparation");
            assert_eq!(
                preparation,
                Preparation::Reset,
                "round {round} must still reset below the cap",
            );
            assert_eq!(slot.invalidations(), round + 1);
        }

        // One more snapshot change — the slot must now freeze instead of
        // resetting again.
        let _ = creator1(&mut store, &mut dag_store, vec![&genesis], &justifications);
        let dag_frozen_input = dag_store.get_representation().expect("post-cap dag");
        let frozen = slot
            .prepare(dag_frozen_input, at(&genesis), thr)
            .expect("frozen preparation");
        assert_eq!(frozen, Preparation::Frozen);
        assert_eq!(
            slot.invalidations(),
            MAX_CONSECUTIVE_INVALIDATIONS,
            "freezing must not advance the invalidation counter past the cap",
        );

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("freeze preparation test should complete");
}

#[tokio::test]
async fn advance_to_ready_clears_the_slot_and_resets_the_invalidation_counter() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("V1")),
            generate_validator(Some("V2")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|v| Bond {
                validator: v.clone(),
                stake: 1,
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
        let creator1 = creator_for(&bonds, &genesis, &validators[0]);
        let justifications: HashMap<&Validator, &BlockMessage> =
            HashMap::from([(&validators[0], &genesis), (&validators[1], &genesis)]);

        let thr = FtThreshold::from_f32_lossy(0.1);
        let mut slot = CursorSlot::default();

        // Reach a non-zero invalidation count the same way the previous test
        // does, so we can assert the counter is cleared on Ready.
        let dag_initial = dag_store.get_representation().expect("initial dag");
        slot.prepare(dag_initial, at(&genesis), thr)
            .expect("fresh preparation");
        let _ = creator1(&mut store, &mut dag_store, vec![&genesis], &justifications);
        let dag_mutated = dag_store.get_representation().expect("mutated dag");
        slot.prepare(dag_mutated, at(&genesis), thr)
            .expect("reset preparation");
        assert_eq!(slot.invalidations(), 1);

        // Drive advance until Ready. Using Budget::Unlimited collapses the loop.
        let advance = slot
            .advance(&NoopWork, &store, Budget::Unlimited)
            .await
            .expect("slot has a cursor to advance");
        assert!(
            matches!(advance.0, Advance::Ready(_)),
            "Unlimited budget must drive the cursor to Ready",
        );
        assert!(slot.cursor().is_none(), "ready must clear the cursor");
        assert_eq!(slot.invalidations(), 0, "ready must reset invalidations");

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("ready-clears-slot test should complete");
}
