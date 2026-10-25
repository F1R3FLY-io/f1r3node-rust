use std::sync::Arc;

use casper::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use casper::rust::util::rholang::system_deploy_util;
use casper::rust::v6::gate::GateVerdict;
use casper::rust::v6::settlement::{
    CloseOnlySettlement, EnvelopeOutcome, EnvelopeRun, ExecutedEnvelope, SettledEnvelope,
    Settlement,
};
use crypto::rust::public_key::PublicKey;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::{
    mk_runtime_manager_with_history_at, mk_test_rnode_store_manager_from_genesis,
};

fn child_of(genesis: &BlockMessage, sender: Bytes) -> BlockMessage {
    let mut block = genesis.clone();
    block.block_hash = Bytes::from_static(b"settlement-target");
    block.header.parents_hash_list = vec![genesis.block_hash.clone()];
    block.header.timestamp += 1;
    block.body.state.block_number = 1;
    block.body.deploys = Vec::new();
    block.sender = sender;
    block.seq_num = 1;
    block
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_only_settlement_matches_the_tranche_1_close_path_and_replays() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("genesis");
    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis);
    let (runtime_manager, _history) = mk_runtime_manager_with_history_at(&mut *kvm).await;
    let runtime_manager = Arc::new(runtime_manager);
    let sender = genesis.validator_pks()[0].bytes.clone();
    let block = child_of(&genesis.genesis_block, sender.clone());
    let mid_root = genesis.genesis_block.body.state.post_state_hash.clone();

    let settlement = CloseOnlySettlement::new(runtime_manager.clone());
    let outcome = settlement
        .settle(&block, &mid_root, &mid_root, &[])
        .await
        .expect("close-only settlement");
    assert!(outcome.settled.is_empty());

    let (direct_root, _, _) = runtime_manager
        .compute_state(
            &mid_root,
            vec![],
            vec![SystemDeployEnum::Close(CloseBlockDeploy {
                initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    PublicKey::from_bytes(&sender),
                    block.seq_num,
                ),
            })],
            BlockData {
                time_stamp: block.header.timestamp,
                block_number: block.body.state.block_number,
                sender: PublicKey::from_bytes(&sender),
                seq_num: block.seq_num,
            },
            None,
        )
        .await
        .expect("the tranche-1 close path");
    assert_eq!(outcome.post_root, direct_root);

    let replayed = settlement
        .replay(&block, &mid_root, &mid_root, &[], &outcome)
        .await
        .expect("replay of the close-only settlement");
    assert_eq!(replayed, outcome.post_root);

    let mut wrong = outcome.clone();
    wrong.post_root = Bytes::from_static(&[7; 32]);
    assert!(settlement
        .replay(&block, &mid_root, &mid_root, &[], &wrong)
        .await
        .is_err());

    let stray = ExecutedEnvelope {
        envelope_id: Bytes::from_static(b"not in the block"),
        verdict: GateVerdict::Admitted { holds: vec![] },
        outcome: EnvelopeOutcome::Completed,
        phlo_used: 1,
        budget_recording: Bytes::new(),
        operation_journal: Bytes::new(),
        user_log: Vec::new(),
    };
    assert!(settlement
        .settle(&block, &mid_root, &mid_root, &[EnvelopeRun::recorded(
            stray.clone()
        )],)
        .await
        .is_err());
    let mut charged = outcome.clone();
    charged.settled = vec![SettledEnvelope::unsettled(stray.envelope_id.clone())];
    assert!(settlement
        .replay(&block, &mid_root, &mid_root, &[], &charged)
        .await
        .is_err());
}
