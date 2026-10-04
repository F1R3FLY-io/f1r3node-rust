use std::sync::Arc;

use casper::rust::genesis::contracts::{
    proof_of_stake::ProofOfStake, validator::Validator as GenesisValidator, vault::Vault,
};
use casper::rust::genesis::genesis::Genesis;
use casper::rust::storage::rnode_key_value_store_manager::new_key_value_store_manager;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use cordial_consensus::{Chain, ChainSpec, DurableBlocklace, StoreConfig, Validator};
use cordial_rholang::{
    CommittedRholang, DeployBatch, DeployOutcome, ExecutionSummary, encode_batch,
};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::{
    secp256k1::Secp256k1, signatures_alg::SignaturesAlg, signed::Signed,
};
use k256::ecdsa::SigningKey;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::Message;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

struct StandaloneNetwork;
#[async_trait::async_trait]
impl cordial_consensus::PeerNetwork for StandaloneNetwork {
    fn peers(&self) -> Vec<consensus_api::Peer> {
        vec![]
    }
    async fn send(
        &self,
        _: &consensus_api::Peer,
        _: &str,
        _: Vec<u8>,
    ) -> Result<(), consensus_api::ConsensusError> {
        Err(consensus_api::ConsensusError::Stopped)
    }
}

async fn runtime(path: &std::path::Path) -> (impl KeyValueStoreManager + use<>, RuntimeManager) {
    let mut manager = new_key_value_store_manager(path.to_owned(), None);
    let stores = manager.r_space_stores().await.unwrap();
    let mergeable = RuntimeManager::mergeable_store(&mut manager).await.unwrap();
    let runtime = RuntimeManager::create_with_store(
        stores,
        mergeable,
        Arc::new(Genesis::default_mergeable_tags()),
        ExternalServices::noop(),
    );
    (manager, runtime)
}

#[tokio::test]
async fn idle_executor_checks_chain_and_rspace_state_before_reporting_no_work() {
    let directory = tempfile::tempdir().unwrap();
    let key = SigningKey::from_slice(&[2; 32]).unwrap();
    let chain = Chain::new(ChainSpec {
        network: "idle-vm-state".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
    })
    .unwrap();
    let mut state = DurableBlocklace::open(
        &directory.path().join("consensus"),
        chain.clone(),
        StoreConfig::default(),
    )
    .unwrap();
    let (mut manager, vm) = runtime(&directory.path().join("vm")).await;
    let mut other_spec = chain.spec().clone();
    other_spec.network = "different-chain".into();
    let other = CommittedRholang::new(vm.clone(), Chain::new(other_spec).unwrap()).unwrap();
    assert!(
        other
            .execute_next(&mut state)
            .await
            .unwrap_err()
            .to_string()
            .contains("chain mismatch")
    );
    let executor = CommittedRholang::new(vm, chain).unwrap();
    assert!(
        executor
            .execute_next(&mut state)
            .await
            .unwrap_err()
            .to_string()
            .contains("pre-state is unavailable")
    );
    assert!(state.execution_receipt(0).unwrap().is_none());
    drop(executor);
    drop(other);
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn unavailable_execution_state_fails_readiness_without_acknowledging_output() {
    let directory = tempfile::tempdir().unwrap();
    let key = SigningKey::from_slice(&[2; 32]).unwrap();
    let chain = Chain::new(ChainSpec {
        network: "missing-vm-state".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
    })
    .unwrap();
    let path = directory.path().join("consensus");
    {
        let mut state =
            DurableBlocklace::open(&path, chain.clone(), StoreConfig::default()).unwrap();
        let mut predecessors = vec![];
        for _ in 0..3 {
            let block = chain.build_block(&key, predecessors, vec![]).unwrap();
            state.admit(&chain.encode_block(&block).unwrap()).unwrap();
            predecessors = vec![block.identity];
        }
    }
    let (mut manager, vm) = runtime(&directory.path().join("vm")).await;
    let mut running = cordial_consensus::CordialIngressAdapter::prepare_with_executor(
        path.clone(),
        chain.clone(),
        StoreConfig::default(),
        consensus_runtime::RuntimeConfig::default(),
        Box::new(CommittedRholang::new(vm, chain.clone()).unwrap()),
    )
    .unwrap()
    .start();
    let handle = running.handle();
    let failure = handle.wait_ready().await.unwrap_err();
    assert!(failure.to_string().contains("pre-state is unavailable"));
    assert!(running.wait().await.is_err());
    assert_eq!(handle.status().phase, consensus_api::Phase::Failed);
    let state = DurableBlocklace::open(&path, chain, StoreConfig::default()).unwrap();
    assert_eq!(state.next_execution().unwrap().unwrap().index, 0);
    assert!(state.execution_receipt(0).unwrap().is_none());
    manager.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn committed_deploy_executes_once_and_replays_after_rspace_reopen() {
    tokio::time::timeout(std::time::Duration::from_secs(180), async {
        let directory = tempfile::tempdir().unwrap();
        let vm_path = directory.path().join("vm");
        let (mut manager, vm) = runtime(&vm_path).await;
        let key = SigningKey::from_slice(&[1; 32]).unwrap();
        let private = PrivateKey::from_bytes(&key.to_bytes());
        let public = Secp256k1.to_public(&private);
        let genesis = Genesis {
            shard_id: "root".into(),
            timestamp: 1_700_000_000_000,
            block_number: 0,
            version: 1,
            supply: 1_000_000_000,
            native_token_name: "Test Token".into(),
            native_token_symbol: "TEST".into(),
            native_token_decimals: 8,
            proof_of_stake: ProofOfStake {
                minimum_bond: 1,
                maximum_bond: i64::MAX,
                validators: vec![GenesisValidator {
                    pk: public.clone(),
                    stake: 1,
                }],
                epoch_length: 1000,
                quarantine_length: 50000,
                number_of_active_validators: 1,
                fault_tolerance_threshold_ppm: 0,
                max_parent_depth: 15,
                deploy_lifespan: 50,
                min_phlo_price: 1,
                pos_multi_sig_public_keys: vec![hex::encode(&public.bytes)],
                pos_multi_sig_quorum: 1,
            },
            vaults: vec![Vault {
                vault_address: VaultAddress::from_public_key(&public).unwrap(),
                initial_balance: 1_000_000_000,
            }],
        };
        let genesis_block = Genesis::create_genesis_block(&vm, &genesis).await.unwrap();
        let genesis_root: [u8; 32] = genesis_block
            .body
            .state
            .post_state_hash
            .as_ref()
            .try_into()
            .unwrap();
        let chain = Chain::new(ChainSpec {
            network: "committed-rholang".into(),
            shard: "root".into(),
            execution_genesis: genesis_root,
            wavelength: 3,
            validators: vec![Validator {
                public_key: key.verifying_key().to_sec1_bytes().to_vec(),
                weight: 1,
            }],
        })
        .unwrap();
        let deploy = Signed::create(
            DeployData {
                term: "@\"cordial-committed\"!(42)".into(),
                time_stamp: 1_700_000_000_001,
                phlo_price: 1,
                phlo_limit: 100_000,
                valid_after_block_number: 0,
                shard_id: "root".into(),
                expiration_timestamp: None,
            },
            Box::new(Secp256k1),
            private.clone(),
        )
        .unwrap();
        let payload = encode_batch(1_700_000_000_002, vec![deploy.clone()]).unwrap();
        let sign_batch = |data| {
            encode_batch(
                1_700_000_000_002,
                vec![Signed::create(data, Box::new(Secp256k1), private.clone()).unwrap()],
            )
            .unwrap()
        };
        let mut invalid_signature = DeployBatch::decode(payload.as_slice()).unwrap();
        invalid_signature.deploys[0].term.push_str(" | Nil");
        let mut wrong_shard = deploy.data.clone();
        wrong_shard.shard_id = "other-shard".into();
        let mut expired = deploy.data.clone();
        expired.expiration_timestamp = Some(1_700_000_000_001);
        let mut overflow = deploy.data.clone();
        overflow.phlo_price = i64::MAX;
        let mut future_height = deploy.data.clone();
        future_height.valid_after_block_number = 1000;
        let mut failed_term = deploy.data.clone();
        failed_term.term = "new x in { x!(1 / 0) }".into();
        let invalid_payloads = vec![
            invalid_signature.encode_to_vec(),
            sign_batch(wrong_shard),
            sign_batch(expired),
            sign_batch(overflow),
            sign_batch(future_height),
            sign_batch(failed_term),
        ];
        let first_path = directory.path().join("first");
        let second_path = directory.path().join("second");
        let mut first =
            DurableBlocklace::open(&first_path, chain.clone(), StoreConfig::default()).unwrap();
        let mut second =
            DurableBlocklace::open(&second_path, chain.clone(), StoreConfig::default()).unwrap();
        let mut predecessors = vec![];
        let mut packets = Vec::new();
        for round in 0..15 {
            let data = match round {
                1 | 2 => payload.clone(),
                3 => vec![0xff],
                4..=9 => invalid_payloads[round - 4].clone(),
                _ => vec![],
            };
            let block = chain.build_block(&key, predecessors, data).unwrap();
            let packet = chain.encode_block(&block).unwrap();
            first.admit(&packet).unwrap();
            second.admit(&packet).unwrap();
            packets.push(packet);
            predecessors = vec![block.identity];
        }
        let query = vm.clone();
        let executor = CommittedRholang::new(vm, chain.clone()).unwrap();
        assert!(executor.execute_next(&mut first).await.unwrap().is_none());
        first.advance_output().unwrap();
        second.advance_output().unwrap();
        let mut receipts = Vec::new();
        while let Some(receipt) = executor.execute_next(&mut first).await.unwrap() {
            receipts.push(receipt);
        }
        assert!(receipts.len() >= 10);
        assert_eq!(receipts[0].post_state, genesis_root);
        let applied: ExecutionSummary = serde_json::from_slice(&receipts[1].result).unwrap();
        assert_eq!(applied.deploys[0].outcome, DeployOutcome::Succeeded);
        assert!(applied.deploys[0].cost > 0);
        assert_ne!(receipts[1].post_state, genesis_root);
        let duplicate: ExecutionSummary = serde_json::from_slice(&receipts[2].result).unwrap();
        assert_eq!(duplicate.deploys[0].outcome, DeployOutcome::Duplicate);
        assert_eq!(receipts[2].post_state, receipts[1].post_state);
        let invalid: ExecutionSummary = serde_json::from_slice(&receipts[3].result).unwrap();
        assert_eq!(invalid.rejection.as_deref(), Some("invalid-batch"));
        assert_eq!(receipts[3].post_state, receipts[2].post_state);
        for (index, reason) in [
            (4, "invalid-signature"),
            (5, "shard-mismatch"),
            (6, "invalid-deploy-time"),
            (7, "invalid-phlo"),
            (8, "invalid-deploy-height"),
        ] {
            let summary: ExecutionSummary =
                serde_json::from_slice(&receipts[index].result).unwrap();
            assert_eq!(summary.deploys[0].outcome, DeployOutcome::Rejected);
            assert_eq!(summary.deploys[0].rejection.as_deref(), Some(reason));
            assert_eq!(receipts[index].post_state, receipts[index].pre_state);
            assert!(receipts[index].deploy_ids.is_empty());
        }
        let failed: ExecutionSummary = serde_json::from_slice(&receipts[9].result).unwrap();
        assert_eq!(failed.deploys[0].outcome, DeployOutcome::Failed);
        assert!(failed.deploys[0].cost > 0);
        let channel = rholang::rust::interpreter::compiler::compiler::Compiler::source_to_adt(
            "\"cordial-committed\"",
        )
        .unwrap();
        let expected =
            rholang::rust::interpreter::compiler::compiler::Compiler::source_to_adt("42").unwrap();
        assert_eq!(
            query
                .get_data(
                    prost::bytes::Bytes::copy_from_slice(&receipts[1].post_state),
                    &channel
                )
                .await
                .unwrap(),
            vec![expected.clone()]
        );
        drop(query);
        drop(executor);
        manager.shutdown().await.unwrap();
        drop(manager);
        drop(first);
        let (mut manager, vm) = runtime(&vm_path).await;
        let executor = CommittedRholang::new(vm, chain.clone()).unwrap();
        for receipt in &receipts {
            assert_eq!(
                executor.execute_next(&mut second).await.unwrap().as_ref(),
                Some(receipt)
            );
        }
        assert!(executor.execute_next(&mut second).await.unwrap().is_none());
        let mut recovered =
            DurableBlocklace::open(&first_path, chain.clone(), StoreConfig::default()).unwrap();
        assert!(
            executor
                .execute_next(&mut recovered)
                .await
                .unwrap()
                .is_none()
        );
        for receipt in &receipts {
            assert_eq!(
                recovered.execution_receipt(receipt.index).unwrap().as_ref(),
                Some(receipt)
            );
        }
        drop(executor);
        manager.shutdown().await.unwrap();
        drop(manager);
        let third_path = directory.path().join("runtime");
        let (mut manager, vm) = runtime(&vm_path).await;
        let running = cordial_consensus::CordialIngressAdapter::prepare_with_executor(
            third_path.clone(),
            chain.clone(),
            StoreConfig::default(),
            consensus_runtime::RuntimeConfig::default(),
            Box::new(CommittedRholang::new(vm, chain.clone()).unwrap()),
        )
        .unwrap()
        .start();
        let handle = running.handle();
        handle.wait_ready().await.unwrap();
        for payload in packets.iter().rev() {
            handle
                .handle_packet(consensus_api::NetworkPacket {
                    peer: consensus_api::Peer {
                        id: vec![1],
                        host: "localhost".into(),
                        tcp_port: 40400,
                        udp_port: 40404,
                    },
                    kind: cordial_consensus::BLOCK_PACKET_KIND.into(),
                    payload: payload.clone(),
                })
                .await
                .unwrap();
        }
        running.shutdown().await.unwrap();
        let state =
            DurableBlocklace::open(&third_path, chain.clone(), StoreConfig::default()).unwrap();
        assert!(state.next_execution().unwrap().is_none());
        for receipt in &receipts {
            assert_eq!(
                state.execution_receipt(receipt.index).unwrap().as_ref(),
                Some(receipt)
            );
        }
        drop(state);
        manager.shutdown().await.unwrap();
        drop(manager);
        let recovery_path = directory.path().join("runtime-recovery");
        {
            let mut state =
                DurableBlocklace::open(&recovery_path, chain.clone(), StoreConfig::default())
                    .unwrap();
            for payload in &packets {
                state.admit(payload).unwrap();
            }
        }
        let (mut manager, vm) = runtime(&vm_path).await;
        let running = cordial_consensus::CordialIngressAdapter::prepare_with_executor(
            recovery_path.clone(),
            chain.clone(),
            StoreConfig::default(),
            consensus_runtime::RuntimeConfig::default(),
            Box::new(CommittedRholang::new(vm, chain.clone()).unwrap()),
        )
        .unwrap()
        .start();
        running.handle().wait_ready().await.unwrap();
        running.shutdown().await.unwrap();
        let state =
            DurableBlocklace::open(&recovery_path, chain.clone(), StoreConfig::default()).unwrap();
        assert!(state.next_execution().unwrap().is_none());
        for receipt in &receipts {
            assert_eq!(
                state.execution_receipt(receipt.index).unwrap().as_ref(),
                Some(receipt)
            );
        }
        manager.shutdown().await.unwrap();
        drop(manager);
        let (mut manager, vm) = runtime(&vm_path).await;
        let (prepared, queries) = cordial_consensus::CordialNode::prepare(
            directory.path().join("proposing-runtime"),
            chain.clone(),
            StoreConfig::default(),
            consensus_runtime::RuntimeConfig::default(),
            cordial_consensus::NodeOptions {
                key: Some(key),
                tick: std::time::Duration::from_millis(50),
                ..Default::default()
            },
            Arc::new(StandaloneNetwork),
            Some(Box::new(CommittedRholang::new(vm.clone(), chain).unwrap())),
        )
        .unwrap();
        let running = prepared.start();
        let handle = running.handle();
        handle.wait_ready().await.unwrap();
        let id = handle
            .submit(DeployData::to_proto(deploy).encode_to_vec())
            .await
            .unwrap();
        assert_eq!(id.len(), 64);
        let mut found = false;
        while !found {
            let progress = queries.progress().await.unwrap();
            for index in 0..progress.executed {
                let receipt = queries.receipt(index).await.unwrap().unwrap();
                let summary: ExecutionSummary = serde_json::from_slice(&receipt.result).unwrap();
                if summary
                    .deploys
                    .iter()
                    .any(|deploy| deploy.outcome == DeployOutcome::Succeeded)
                {
                    found = true;
                    assert_eq!(
                        vm.get_data(
                            prost::bytes::Bytes::copy_from_slice(&receipt.post_state),
                            &channel
                        )
                        .await
                        .unwrap(),
                        vec![expected.clone()]
                    );
                }
            }
            if !found {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
        running.shutdown().await.unwrap();
        manager.shutdown().await.unwrap();
    })
    .await
    .expect("committed Rholang execution timed out");
}
