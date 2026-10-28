//! Refusal cases for `validate_commit` — the pure predicate the finalizer's
//! atomic LFB commit runs under the write lock. The existing integration test
//! `a_refused_commit_changes_nothing` only exercises a `StateChanged` refusal
//! fabricated by the test closure; these tests pin the predicate's own
//! refusals: `NotHigher` (backwards candidate) and `ALinViolation` (off the
//! main-parent chain under `ALinPolicy::Halt`).

use std::collections::HashMap;

use block_storage::rust::dag::finalization_commit::CommitRefusal;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::engine::multi_parent_casper::finalization_runner::{validate_commit, ALinPolicy};
use casper::rust::finality::floor::Floor;
use casper::rust::finality::floor_runner::snapshot_id;
use casper::rust::safety::clique_oracle::FtThreshold;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::validator::Validator;

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
async fn validate_commit_refuses_a_candidate_not_higher_than_the_current_lfb() {
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
        let sigma = snapshot_id(&dag, thr).expect("snapshot id");
        let genesis_floor = at(&genesis);

        // Candidate at the same height as the current LFB (genesis). The
        // predicate refuses with NotHigher reporting the current height.
        let refusal = validate_commit(
            &dag,
            &store,
            &genesis_floor,
            sigma,
            &genesis_floor,
            thr,
            ALinPolicy::Halt,
        )
        .expect_err("a candidate at the lfb height must be refused");
        match refusal {
            CommitRefusal::NotHigher { lfb_height } => {
                assert_eq!(
                    lfb_height, genesis_floor.block_number,
                    "the refusal must report the current lfb height",
                );
            }
            other => panic!("expected NotHigher, got {other:?}"),
        }

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("not-higher refusal test should complete");
}

#[tokio::test]
async fn validate_commit_refuses_an_off_main_chain_candidate_under_halt_policy() {
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
        let creator2 = creator_for(&bonds, &genesis, &validators[1]);
        let justifications: HashMap<&Validator, &BlockMessage> =
            HashMap::from([(&validators[0], &genesis), (&validators[1], &genesis)]);

        // Fork off genesis: b1 on the main chain the lfb walk follows, b1_side
        // as the fork. b2_fork extends the fork; its main parent is b1_side,
        // not b1.
        let b1 = creator1(&mut store, &mut dag_store, vec![&genesis], &justifications);
        let b1_side = creator2(&mut store, &mut dag_store, vec![&genesis], &justifications);
        let b2_fork = creator2(&mut store, &mut dag_store, vec![&b1_side], &justifications);

        // Advance the lfb to b1 so b1_side's chain is strictly off the main
        // chain of the current lfb. Without this move the lfb sits at genesis
        // and every direct child is on the main chain by definition — the
        // predicate's main-chain refusal cannot fire from the root.
        dag_store
            .commit_lfb(b1.block_hash.clone(), 0.5, |_| Ok(()))
            .expect("advance lfb to b1");

        let dag = dag_store.get_representation().expect("dag representation");
        assert_eq!(dag.last_finalized_block(), b1.block_hash, "lfb now at b1");
        let thr = FtThreshold::from_f32_lossy(0.1);
        let sigma = snapshot_id(&dag, thr).expect("snapshot id");
        let lfb_floor = at(&b1);

        // b2_fork is strictly higher than the lfb (passing the NotHigher
        // guard) but its main-parent walk at the lfb's height lands on
        // b1_side, not b1. Under ALinPolicy::Halt the predicate must refuse
        // with ALinViolation.
        //
        // Setting `derived_against` to the current lfb skips the state-
        // containment branch, isolating the chain-walk refusal.
        let refusal = validate_commit(
            &dag,
            &store,
            &lfb_floor,
            sigma,
            &at(&b2_fork),
            thr,
            ALinPolicy::Halt,
        )
        .expect_err("the off-main-chain extension must be refused");
        assert!(
            matches!(refusal, CommitRefusal::ALinViolation(_)),
            "expected ALinViolation, got {refusal:?}",
        );

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("a-lin refusal test should complete");
}
