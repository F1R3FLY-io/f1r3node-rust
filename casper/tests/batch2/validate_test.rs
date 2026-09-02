// See casper/src/test/scala/coop/rchain/casper/batch2/ValidateTest.scala

use std::collections::HashMap;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::casper::CasperSnapshot;
use casper::rust::finality::floor_context::FloorContext;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::rholang::interpreter_util;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::{construct_deploy, proto_util};
use casper::rust::validate::Validate;
use casper::rust::validator_identity::ValidatorIdentity;
use casper_message::Justification;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, DeployData, ProcessedDeploy, RejectedDeploy, RejectedDeployReason,
    StateEffectId,
};
use models::rust::deploy_id::DeployLookupId;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{
    build_block, create_block, create_block_with_merge_facts, create_genesis_block,
    create_validator_block, MergeFacts,
};
use crate::helper::block_util::generate_validator;
use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_test_rnode_store_manager_from_genesis;

const SHARD_ID: &str = "root-shard";

fn mk_casper_snapshot(dag: KeyValueDagRepresentation) -> CasperSnapshot { CasperSnapshot::new(dag) }

fn create_chain(
    block_store: &mut KeyValueBlockStore,
    block_dag_storage: &mut IndexedBlockDagStorage,
    length: usize,
    bonds: Vec<Bond>,
) -> BlockMessage {
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

    let _final_block = (1..length).fold(genesis.clone(), |block, _| {
        create_block(
            block_store,
            block_dag_storage,
            vec![block.block_hash.clone()],
            &genesis,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    });

    genesis
}

fn create_chain_with_round_robin_validators(
    block_store: &mut KeyValueBlockStore,
    block_dag_storage: &mut IndexedBlockDagStorage,
    length: usize,
    validator_length: usize,
) -> BlockMessage {
    let validator_round_robin_cycle = std::iter::repeat(0..validator_length).flatten();

    let validators: Vec<Bytes> = std::iter::repeat_with(|| generate_validator(None))
        .take(validator_length)
        .collect();

    let genesis = create_genesis_block(
        block_store,
        block_dag_storage,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let fold_result = (0..length).zip(validator_round_robin_cycle).fold(
        (genesis.clone(), genesis.clone(), HashMap::new()),
        |acc, (_, validator_num)| {
            let (genesis, block, latest_messages) = acc;
            let creator = validators[validator_num].clone();
            let bnext = create_block(
                block_store,
                block_dag_storage,
                vec![block.block_hash.clone()],
                &genesis,
                Some(creator.clone()),
                None,
                Some(latest_messages.clone()),
                None,
                None,
                None,
                None,
                None,
                None,
            );

            let latest_messages_next = {
                let mut updated = latest_messages.clone();
                updated.insert(bnext.sender.clone(), bnext.block_hash.clone());
                updated
            };

            (genesis, bnext, latest_messages_next)
        },
    );

    fold_result.0 // .map(_._1) in Scala
}

fn signed_block(
    i: usize,
    private_key: &PrivateKey,
    block_dag_storage: &mut IndexedBlockDagStorage,
) -> BlockMessage {
    let secp256k1 = Secp256k1;
    let pk = secp256k1.to_public(private_key);
    let mut block = block_dag_storage.lookup_by_id_unsafe(i as i64);
    let dag = block_dag_storage
        .get_representation()
        .expect("dag representation");
    let sender = Bytes::copy_from_slice(&pk.bytes);
    let latest_message_opt = dag.latest_message(&sender).unwrap_or(None);
    let seq_num = latest_message_opt.map_or(0, |block_metadata| block_metadata.sequence_number) + 1;

    block.seq_num = seq_num;
    ValidatorIdentity::new(private_key).sign_block(&block)
}

fn with_block_number(block: &BlockMessage, n: i64) -> BlockMessage {
    let mut new_state = block.body.state.clone();
    new_state.block_number = n;
    let mut new_block = block.clone();
    new_block.body.state = new_state;
    new_block
}

//helper functions
fn with_sig_algorithm(block: &BlockMessage, algorithm: &str) -> BlockMessage {
    let mut new_block = block.clone();
    new_block.sig_algorithm = algorithm.to_string();
    new_block
}

fn with_sender(block: &BlockMessage, sender: &Bytes) -> BlockMessage {
    let mut new_block = block.clone();
    new_block.sender = sender.clone();
    new_block
}

fn with_sig(block: &BlockMessage, sig: &Bytes) -> BlockMessage {
    let mut new_block = block.clone();
    new_block.sig = sig.clone();
    new_block
}

fn with_seq_num(block: &BlockMessage, seq_num: i32) -> BlockMessage {
    let mut new_block = block.clone();
    new_block.seq_num = seq_num;
    new_block
}

fn with_shard_id(block: &BlockMessage, shard_id: &str) -> BlockMessage {
    let mut new_block = block.clone();
    new_block.shard_id = shard_id.to_string();
    new_block
}

fn with_post_state_hash(block: &BlockMessage, post_state_hash: &Bytes) -> BlockMessage {
    let mut new_block = block.clone();
    new_block.body.state.post_state_hash = post_state_hash.clone();
    new_block
}

fn with_block_hash(block: &BlockMessage, block_hash: &Bytes) -> BlockMessage {
    let mut new_block = block.clone();
    new_block.block_hash = block_hash.clone();
    new_block
}

fn modified_timestamp_header(block: &BlockMessage, timestamp: i64) -> BlockMessage {
    let mut modified_timestamp_header = block.header.clone();
    modified_timestamp_header.timestamp = timestamp;

    let mut block_with_modified_header = block.clone();
    block_with_modified_header.header = modified_timestamp_header;
    block_with_modified_header
}

fn create_signed_deploy_with_data(
    updated_deploy_data: DeployData,
) -> Result<Signed<DeployData>, String> {
    let secp = Secp256k1;
    Signed::create(
        updated_deploy_data,
        Box::new(secp),
        construct_deploy::DEFAULT_SEC.clone(),
    )
}

fn legacy_validation_block(deploy: Signed<DeployData>) -> BlockMessage {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let mut block = build_block(
        Vec::new(),
        None,
        now,
        None,
        None,
        Some(vec![ProcessedDeploy::empty(deploy)]),
        None,
        None,
        None,
        None,
    );
    block.header.version = casper::rust::casper::CERTIFIED_VALIDATOR_INCARNATION_PROTOCOL_VERSION;
    block.header.finalized_floor = None;
    block.finalized_floor_certificate = None;
    block
}

fn create_justifications(pairs: Vec<(Bytes, Bytes)>) -> HashMap<Bytes, Bytes> {
    pairs.into_iter().collect()
}

// Many tests use checks that must be added later
// TODO: Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_signature_validation_should_return_false_on_unknown_algorithms() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, 2, vec![]);

        let unknown_algorithm = "unknownAlgorithm";
        let rsa = "RSA";

        let block0 =
            with_sig_algorithm(&block_dag_storage.lookup_by_id_unsafe(0), unknown_algorithm);
        let block1 = with_sig_algorithm(&block_dag_storage.lookup_by_id_unsafe(1), rsa);

        let result0 = Validate::block_signature(&block0);
        assert!(!result0);

        let result1 = Validate::block_signature(&block1);
        assert!(!result1);

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns.last.contains(s"signature algorithm $unknownAlgorithm is unsupported") should be(true)
        // log.warns.last.contains(s"signature algorithm $rsa is unsupported") should be(true)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_signature_validation_should_return_false_on_invalid_secp256k1_signatures() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let secp256k1 = Secp256k1;
        let (private_key, public_key) = secp256k1.new_key_pair();

        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, 6, vec![]);
        let (_wrong_sk, wrong_pk) = secp256k1.new_key_pair();

        assert_ne!(
            public_key.bytes, wrong_pk.bytes,
            "Public keys should be different"
        );
        let empty = Bytes::new();
        let invalid_key = hex::decode("abcdef1234567890").unwrap().into();

        let block0 = with_sender(
            &signed_block(0, &private_key, &mut block_dag_storage),
            &empty,
        );

        let block1 = with_sender(
            &signed_block(1, &private_key, &mut block_dag_storage),
            &invalid_key,
        );

        let block2 = with_sender(
            &signed_block(2, &private_key, &mut block_dag_storage),
            &Bytes::copy_from_slice(&wrong_pk.bytes),
        );

        let block3 = with_sig(
            &signed_block(3, &private_key, &mut block_dag_storage),
            &empty,
        );

        let block4 = with_sig(
            &signed_block(4, &private_key, &mut block_dag_storage),
            &invalid_key,
        );

        let block5 = with_sig(
            &signed_block(5, &private_key, &mut block_dag_storage),
            &block0.sig,
        ); //wrong sig

        let blocks = [block0, block1, block2, block3, block4, block5];

        for (i, block) in blocks.iter().enumerate() {
            let result = Validate::block_signature(block);
            assert!(!result, "Block {} should have invalid signature", i);
        }

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns.size should be(blocks.length)
        // log.warns.forall(_.contains("signature is invalid")) should be(true)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_signature_validation_should_return_true_on_valid_secp256k1_signatures() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let n = 6;
        let secp256k1 = Secp256k1;
        let (private_key, _public_key) = secp256k1.new_key_pair();

        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, n, vec![]);

        let condition = (0..n).all(|i| {
            let block = signed_block(i, &private_key, &mut block_dag_storage);
            Validate::block_signature(&block)
        });

        assert!(condition);

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns should be(Nil)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn timestamp_validation_should_not_accept_blocks_with_future_time() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, 1, vec![]);
        let block = block_dag_storage.lookup_by_id_unsafe(0);

        // modifiedTimestampHeader = block.header.copy(timestamp = 99999999)
        // Note: In Scala tests LogicalTime starts from 0, but in Rust we use real Unix timestamps
        // So we need a timestamp that's actually in the future relative to current time
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let future_timestamp = current_time + 20000; // 20 seconds in future (> DRIFT of 15 seconds)

        let _dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        let result_invalid = Validate::timestamp(
            &modified_timestamp_header(&block, future_timestamp),
            &block_store,
        );
        assert_eq!(
            result_invalid,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidTimestamp))
        );

        let result_valid = Validate::timestamp(&block, &block_store);
        assert_eq!(result_valid, Either::Right(ValidBlock::Valid));

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // _ = log.warns.size should be(1)
        // result = log.warns.head.contains("block timestamp") should be(true)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn timestamp_validation_should_not_accept_blocks_that_were_published_before_parent_time() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, 2, vec![]);
        let block = block_dag_storage.lookup_by_id_unsafe(1);
        let modified_timestamp_header = modified_timestamp_header(&block, -1);

        let _dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        let result_invalid = Validate::timestamp(&modified_timestamp_header, &block_store);
        assert_eq!(
            result_invalid,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidTimestamp))
        );

        let result_valid = Validate::timestamp(&block, &block_store);
        assert_eq!(result_valid, Either::Right(ValidBlock::Valid));

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns.size should be(1)
        // log.warns.head.contains("block timestamp") should be(true)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_number_validation_should_only_accept_0_as_the_number_for_a_block_with_no_parents() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, 1, vec![]);
        let block = block_dag_storage.lookup_by_id_unsafe(0);
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result_invalid =
            Validate::block_number(&with_block_number(&block, 1), &mut casper_snapshot);
        assert_eq!(
            result_invalid,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockNumber))
        );

        let result_valid = Validate::block_number(&block, &mut casper_snapshot);
        assert_eq!(result_valid, Either::Right(ValidBlock::Valid));

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns.size should be(1)
        // log.warns.head.contains("not zero, but block has no parents") should be(true)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_number_validation_should_return_false_for_non_sequential_numbering() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, 2, vec![]);
        let block = block_dag_storage.lookup_by_id_unsafe(1);
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result_invalid =
            Validate::block_number(&with_block_number(&block, 17), &mut casper_snapshot);
        assert_eq!(
            result_invalid,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockNumber))
        );

        let result_valid = Validate::block_number(&block, &mut casper_snapshot);
        assert_eq!(result_valid, Either::Right(ValidBlock::Valid));

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns.size should be(1)
        // log.warns.head.contains("is not one more than maximum parent number") should be(true)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_number_validation_should_return_true_for_sequential_numbering() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let n = 6;
        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, n, vec![]);
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        // Test each block in the chain for sequential numbering
        let condition = (0..n).all(|i| {
            let block = block_dag_storage.lookup_by_id_unsafe(i as i64);
            let result = Validate::block_number(&block, &mut casper_snapshot);
            result == Either::Right(ValidBlock::Valid)
        });

        assert!(condition);

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns should be(Nil)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_number_validation_should_correctly_validate_a_multi_parent_block_where_the_parents_have_different_block_numbers(
) {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let create_block_with_number =
            |block_store: &mut KeyValueBlockStore,
             block_dag_storage: &mut IndexedBlockDagStorage,
             n: i64,
             _genesis: &BlockMessage,
             parent_hashes: Vec<Bytes>| {
                let current_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;

                let block = models::rust::block_implicits::get_random_block(
                    Some(n),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(current_time),
                    Some(parent_hashes),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(Box::new(|block| proto_util::hash_block(&block))),
                );

                block_store
                    .put(block.block_hash.clone(), &block)
                    .expect("Failed to put block");
                block_dag_storage
                    .insert(
                        &block,
                        block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
                    )
                    .expect("Failed to insert block");

                block
            };

        // Note we need to create a useless chain to satisfy the assert in TopoSort
        let genesis = create_chain(&mut block_store, &mut block_dag_storage, 8, vec![]);

        let b1 =
            create_block_with_number(&mut block_store, &mut block_dag_storage, 3, &genesis, vec![
                genesis.block_hash.clone(),
            ]);

        let b2 =
            create_block_with_number(&mut block_store, &mut block_dag_storage, 7, &genesis, vec![
                genesis.block_hash.clone(),
            ]);

        let b3 =
            create_block_with_number(&mut block_store, &mut block_dag_storage, 8, &genesis, vec![
                b1.block_hash.clone(),
                b2.block_hash.clone(),
            ]);

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let s1 = Validate::block_number(&b3, &mut casper_snapshot);
        assert_eq!(s1, Either::Right(ValidBlock::Valid));

        let s2 = Validate::block_number(&with_block_number(&b3, 4), &mut casper_snapshot);
        assert_eq!(
            s2,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockNumber))
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn future_deploy_validation_should_work() {
    with_storage(|_block_store, _block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();

        let updated_processed_deploy = {
            let mut updated_deploy_data = deploy.deploy.data.clone();
            updated_deploy_data.valid_after_block_number = -1;

            create_signed_deploy_with_data(updated_deploy_data)
                .expect("Failed to create signed deploy")
        };

        let block = legacy_validation_block(updated_processed_deploy);

        let status = Validate::future_transaction(&block);

        assert_eq!(status, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn future_deploy_validation_should_not_accept_blocks_with_a_deploy_for_a_future_block_number()
{
    with_storage(|_block_store, _block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();

        let updated_processed_deploy = {
            let mut updated_deploy_data = deploy.deploy.data.clone();
            updated_deploy_data.valid_after_block_number = i64::MAX;

            create_signed_deploy_with_data(updated_deploy_data)
                .expect("Failed to create signed deploy")
        };

        let block_with_future_deploy = legacy_validation_block(updated_processed_deploy);

        let status = Validate::future_transaction(&block_with_future_deploy);

        assert_eq!(
            status,
            Either::Left(BlockError::Invalid(InvalidBlock::ContainsFutureDeploy))
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deploy_expiration_validation_should_work() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let block = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![deploy]),
            None,
            None,
            None,
            None,
        );
        let status = Validate::transaction_expiration(&block, 10);
        assert_eq!(status, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deploy_expiration_validation_should_not_accept_blocks_with_a_deploy_that_is_expired() {
    with_storage(|_block_store, _block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();

        let updated_processed_deploy = {
            let mut updated_deploy_data = deploy.deploy.data.clone();
            updated_deploy_data.valid_after_block_number = i64::MIN;

            create_signed_deploy_with_data(updated_deploy_data)
                .expect("Failed to create signed deploy")
        };

        let block_with_expired_deploy = legacy_validation_block(updated_processed_deploy);

        let status = Validate::transaction_expiration(&block_with_expired_deploy, 10);
        assert_eq!(
            status,
            Either::Left(BlockError::Invalid(InvalidBlock::ContainsExpiredDeploy))
        );
    })
    .await
}

// C10 / Test-1: cover `Validate::deploys_shard_identifier` — the
// 14-step block_summary validator chain entry that rejects blocks
// whose deploys carry a foreign shard_id. Prior to this commit the
// validator had zero direct test callers, so a regression renaming
// the field or short-circuiting the per-deploy check would not be
// caught by any add_block / slashing integration test.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deploys_shard_identifier_should_accept_blocks_with_all_matching_shard_ids() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy =
            construct_deploy::basic_processed_deploy(0, Some(SHARD_ID.to_string())).unwrap();
        let block = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![deploy]),
            None,
            Some(SHARD_ID.to_string()),
            None,
            None,
        );

        let status = Validate::deploys_shard_identifier(&block, SHARD_ID);
        assert_eq!(status, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deploys_shard_identifier_should_reject_blocks_with_a_foreign_deploy_shard_id() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let foreign_deploy =
            construct_deploy::basic_processed_deploy(0, Some("rogue-shard".to_string())).unwrap();
        let block_with_foreign_deploy = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![foreign_deploy]),
            None,
            Some(SHARD_ID.to_string()),
            None,
            None,
        );

        let status = Validate::deploys_shard_identifier(&block_with_foreign_deploy, SHARD_ID);
        assert_eq!(
            status,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidShardId))
        );
    })
    .await
}

// C10 / Test-2: cover `Validate::time_based_expiration` — the
// validator that rejects blocks containing time-expired deploys
// (i.e. deploys whose `expiration_timestamp` is set and is less than
// the block timestamp). Prior to this commit it had zero direct
// test callers; a regression would silently accept time-expired
// deploys.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn time_based_expiration_should_accept_blocks_with_unexpired_deploys() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        // No expiration_timestamp (None) ⇒ deploy never time-expires.
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let block = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![deploy]),
            None,
            None,
            None,
            None,
        );

        let status = Validate::time_based_expiration(&block);
        assert_eq!(status, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn time_based_expiration_should_reject_blocks_with_a_time_expired_deploy() {
    with_storage(|_block_store, _block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();

        // Force `expiration_timestamp = 1` — strictly less than the
        // block's `header.timestamp` (set to current wall time by
        // `create_genesis_block`). The deploy is therefore time-expired
        // for any block created after the unix epoch.
        let expired_processed_deploy = {
            let mut data = deploy.deploy.data.clone();
            data.expiration_timestamp = Some(1);
            create_signed_deploy_with_data(data).expect("failed to sign expired deploy")
        };

        let block_with_time_expired_deploy = legacy_validation_block(expired_processed_deploy);

        let status = Validate::time_based_expiration(&block_with_time_expired_deploy);
        assert_eq!(
            status,
            Either::Left(BlockError::Invalid(InvalidBlock::ContainsTimeExpiredDeploy))
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sequence_number_validation_should_only_accept_0_as_the_number_for_a_block_with_no_parents()
{
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, 1, vec![]);
        let block = block_dag_storage.lookup_by_id_unsafe(0);
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let block_with_seq_1 = with_seq_num(&block, 1);
        let result_invalid = Validate::sequence_number(&block_with_seq_1, &mut casper_snapshot);
        assert_eq!(
            result_invalid,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidSequenceNumber))
        );

        let result_valid = Validate::sequence_number(&block, &mut casper_snapshot);
        assert_eq!(result_valid, Either::Right(ValidBlock::Valid));

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns.size should be(1)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sequence_number_validation_should_return_false_for_non_sequential_numbering() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, 2, vec![]);
        let block = block_dag_storage.lookup_by_id_unsafe(1);
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let block_with_seq_1 = with_seq_num(&block, 1);
        let result = Validate::sequence_number(&block_with_seq_1, &mut casper_snapshot);
        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidSequenceNumber))
        );

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns.size should be(1)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sequence_number_validation_should_return_true_for_sequential_numbering() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let n = 20;
        let validator_count = 3;
        let _genesis = create_chain_with_round_robin_validators(
            &mut block_store,
            &mut block_dag_storage,
            n,
            validator_count,
        );
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let condition = (0..n).all(|i| {
            let block = block_dag_storage.lookup_by_id_unsafe(i as i64);
            let result = Validate::sequence_number(&block, &mut casper_snapshot);
            result == Either::Right(ValidBlock::Valid)
        });

        assert!(condition);

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns should be(Nil)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_validation_should_return_valid_for_empty_blocks() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, 2, vec![]);
        let block = block_dag_storage.lookup_by_id_unsafe(0);
        let block2 = block_dag_storage.lookup_by_id_unsafe(1);
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result1 = Validate::repeat_deploy(&block, &mut casper_snapshot, &block_store, 50);
        assert_eq!(result1, Either::Right(ValidBlock::Valid));

        let result2 = Validate::repeat_deploy(&block2, &mut casper_snapshot, &block_store, 50);
        assert_eq!(result2, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_validation_rejects_duplicate_signatures_within_one_block() {
    with_storage(|block_store, block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let block = build_block(
            Vec::new(),
            None,
            now,
            None,
            None,
            Some(vec![deploy.clone(), deploy]),
            None,
            None,
            None,
            None,
        );
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        assert_eq!(
            Validate::repeat_deploy(&block, &mut casper_snapshot, &block_store, 50),
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy))
        );
    })
    .await
}

//Test 18: "Repeat deploy validation" should "not accept blocks with a repeated deploy"
// +
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_validation_should_not_accept_blocks_with_a_repeated_deploy() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![deploy.clone()]),
            None,
            None,
            None,
            None,
        );

        let block1 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            Some(vec![deploy]),
            None,
            None,
            None,
            None,
            None,
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::repeat_deploy(&block1, &mut casper_snapshot, &block_store, 50);
        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy))
        );
    })
    .await
}

/// Production order: a candidate is validated BEFORE insertion, so its own
/// deploys are not yet in any inserted block, and fresh deploys must clear
/// the repeat check — the parent-scope scan and the ancestor traversal have
/// nothing to find for them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_accepts_fresh_deploys_in_block_not_yet_inserted() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let genesis_deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![genesis_deploy]),
            None,
            None,
            None,
            None,
        );

        let fresh_deploy = construct_deploy::basic_processed_deploy(1, None).unwrap();
        let candidate = build_block(
            vec![genesis.block_hash.clone()],
            None,
            1786500000000,
            None,
            None,
            Some(vec![fresh_deploy]),
            None,
            None,
            None,
            Some(1),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::repeat_deploy(&candidate, &mut casper_snapshot, &block_store, 50);
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

/// Fast-path equivalence, repeat case: with the carrier index certified
/// complete, a row hit routes to the exact scan and the repeat is flagged
/// exactly as the uncertified path flags it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_certified_index_still_flags_a_repeated_deploy() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let carried = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![carried.clone()]),
            None,
            None,
            None,
            None,
        );
        let block1 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            Some(vec![carried.clone()]),
            None,
            None,
            None,
            None,
            None,
        );
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        dag.carrier_index
            .write()
            .set_watermark_if_absent(0)
            .expect("certify");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::repeat_deploy(&block1, &mut casper_snapshot, &block_store, 50);
        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy))
        );
    })
    .await
}

/// Fast-path equivalence, fresh case: with the carrier index certified
/// complete, a fresh sig's absence proof skips the ancestor scan and the
/// verdict stays Valid — identical to the uncertified path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_certified_index_accepts_fresh_deploys() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let genesis_deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![genesis_deploy]),
            None,
            None,
            None,
            None,
        );

        let fresh_deploy = construct_deploy::basic_processed_deploy(1, None).unwrap();
        let candidate = build_block(
            vec![genesis.block_hash.clone()],
            None,
            1786500000000,
            None,
            None,
            Some(vec![fresh_deploy]),
            None,
            None,
            None,
            Some(1),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        dag.carrier_index
            .write()
            .set_watermark_if_absent(0)
            .expect("certify");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::repeat_deploy(&candidate, &mut casper_snapshot, &block_store, 50);
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

/// Fast-path soundness on the invalid-carrier gap that made the removed
/// deploy_index fast path unportable: a sig carried ONLY by an INVALID
/// ancestor is still a repeat (the ancestor scan reads bodies without a
/// validity qualifier), and the certified index must reach the same
/// verdict — the `CarriedInvalid` row routes the sig to the exact scan
/// instead of proving absence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_certified_index_still_flags_a_repeat_via_an_invalid_ancestor() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let genesis_deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![genesis_deploy]),
            None,
            None,
            None,
            None,
        );

        let carried = construct_deploy::basic_processed_deploy(1, None).unwrap();
        let invalid_carrier = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            Some(vec![carried.clone()]),
            None,
            None,
            None,
            None,
            Some(true),
        );

        let candidate = build_block(
            vec![invalid_carrier.block_hash.clone()],
            None,
            1786500000000,
            None,
            None,
            Some(vec![carried]),
            None,
            None,
            None,
            Some(1),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut uncertified_snapshot = mk_casper_snapshot(dag);
        let scan_verdict =
            Validate::repeat_deploy(&candidate, &mut uncertified_snapshot, &block_store, 50);
        assert_eq!(
            scan_verdict,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy)),
            "the ancestor scan flags a repeat carried by an invalid ancestor"
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        dag.carrier_index
            .write()
            .set_watermark_if_absent(0)
            .expect("certify");
        let mut certified_snapshot = mk_casper_snapshot(dag);
        let index_verdict =
            Validate::repeat_deploy(&candidate, &mut certified_snapshot, &block_store, 50);
        assert_eq!(
            index_verdict, scan_verdict,
            "index-served and scan-served verdicts must be equal"
        );
    })
    .await
}

/// The duplicate scan walks the block's ancestry, and a storage failure during
/// that walk used to be swallowed: the expansion returned nothing, the walk ended
/// early, and the block passed. So a DAG that cannot be read all the way down —
/// a truncated one, or a damaged one — silently ADMITS the repeat deploy the scan
/// exists to reject. A validator that cannot read the set it must scan has to
/// refuse the verdict, not return the one that absence produces.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_validation_should_surface_a_storage_failure_not_admit_the_deploy() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();

        // Genesis carries the deploy: it is the duplicate the scan must find.
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![deploy.clone()]),
            None,
            None,
            None,
            None,
        );

        let missing_parent = Bytes::from(b"ancestor-absent-from-this-dag".to_vec());
        let mid = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut mid_metadata = dag.lookup_unsafe(&mid.block_hash).expect("mid metadata");
        mid_metadata.parents.push(missing_parent.clone());
        dag.block_metadata_index
            .write()
            .add(mid_metadata)
            .expect("corrupt metadata row");

        let head = build_block(
            vec![mid.block_hash.clone()],
            None,
            1786500000001,
            None,
            None,
            Some(vec![deploy]),
            None,
            None,
            None,
            Some(2),
        );
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::repeat_deploy(&head, &mut casper_snapshot, &block_store, 50);
        assert!(
            matches!(&result, Either::Left(BlockError::Undecidable(hash)) if hash == &missing_parent),
            "an ancestry this node cannot read must name the block it is missing, not be \
             swallowed (which admits the repeated deploy genesis carries) and not be \
             reported as a local storage fault. Validation \
             reports the gap; whether this node may act on it is decided by the block \
             processor, which alone knows if its own history is cut short. Got {:?}",
            result
        );
    })
    .await
}

/// Fast-path ENGAGEMENT pin: the three certified-index tests above assert
/// verdicts that are identical whether the scan ran or was skipped, so a
/// regression that silently disables the fast path would ship clean past
/// them. This test makes the skip itself observable: the candidate's
/// ancestry contains an unreadable parent, so the exact scan CANNOT
/// succeed (the uncertified control below proves it errors) — a Valid
/// verdict is therefore only reachable through the engaged absence proof.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_certified_index_engagement_skips_the_scan() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let genesis_deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![genesis_deploy]),
            None,
            None,
            None,
            None,
        );

        let missing_parent = Bytes::from(b"ancestor-absent-from-this-dag".to_vec());
        let mid = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );

        let fresh_deploy = construct_deploy::basic_processed_deploy(1, None).unwrap();
        let candidate = build_block(
            vec![mid.block_hash.clone()],
            None,
            1786500000000,
            None,
            None,
            Some(vec![fresh_deploy]),
            None,
            None,
            None,
            Some(1),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut mid_metadata = dag.lookup_unsafe(&mid.block_hash).expect("mid metadata");
        mid_metadata.parents.push(missing_parent);
        dag.block_metadata_index
            .write()
            .add(mid_metadata)
            .expect("corrupt metadata row");
        let mut uncertified_snapshot = mk_casper_snapshot(dag);
        let scan_verdict =
            Validate::repeat_deploy(&candidate, &mut uncertified_snapshot, &block_store, 50);
        assert!(
            matches!(scan_verdict, Either::Left(_)),
            "control: with the fast path off, the unreadable ancestry must fail the scan; \
             got {:?}",
            scan_verdict
        );

        uncertified_snapshot
            .dag
            .carrier_index
            .write()
            .set_watermark_if_absent(0)
            .expect("certify");
        let result =
            Validate::repeat_deploy(&candidate, &mut uncertified_snapshot, &block_store, 50);
        assert_eq!(
            result,
            Either::Right(ValidBlock::Valid),
            "a fresh sig's absence proof must skip the scan entirely — this Valid is \
             unreachable through the scan path"
        );
    })
    .await
}

/// The scan reads each ancestor's body to test it for the deploy signature, and
/// an ancestor the DAG knows about may not be in the block store — that is the
/// normal shape after an LFS restore, which fills the DAG from the sync window.
/// Killing the validator thread on that read turns a recoverable storage gap
/// into a crash; it belongs in the same typed failure as the walk itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_validation_should_surface_an_unreadable_ancestor_body() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let ghost = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );
        assert!(block_store
            .remove_block_for_tests(&ghost.block_hash)
            .expect("remove ghost body"));

        let head = build_block(
            vec![ghost.block_hash.clone()],
            None,
            1786500000000,
            None,
            None,
            Some(vec![deploy]),
            None,
            None,
            None,
            Some(2),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::repeat_deploy(&head, &mut casper_snapshot, &block_store, 50);
        assert!(
            matches!(result, Either::Left(BlockError::BlockException(_))),
            "an ancestor whose body is missing must be a typed failure, not a panic; got {:?}",
            result
        );
    })
    .await
}

/// The retry gate at the validity layer: a re-inclusion whose kept
/// rejection is LIVE (above the block's frozen floor) is
/// `PrematureDeployRetry` — never a legal recovery, never
/// `InvalidRepeatDeploy` (which would misread the retry as a plain
/// duplicate and slash-classify differently).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_rejects_premature_retry_of_a_live_rejection() {
    use std::sync::Arc;

    use dashmap::DashSet;

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let deploy_id = deploy
            .deploy_id_for_protocol(casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION)
            .expect("protocol-v6 deploy identity");
        let deploy_id_v6 = match &deploy_id {
            DeployLookupId::V6(deploy_id) => *deploy_id,
            DeployLookupId::Legacy(_) => unreachable!(),
        };

        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let block_x = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            Some(vec![deploy.clone()]),
            None,
            None,
            None,
            None,
            None,
        );
        let block_m = create_block_with_merge_facts(
            &mut block_store,
            &mut block_dag_storage,
            vec![block_x.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            MergeFacts {
                rejected_deploys: vec![RejectedDeploy::occurrence_v6(
                    deploy_id_v6,
                    block_x.block_hash.clone(),
                    RejectedDeployReason::MergeConflict,
                )],
                ..Default::default()
            },
        );
        let block_w = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![block_m.block_hash.clone()],
            &genesis,
            None,
            None,
            Some(HashMap::from([(
                block_m.sender.clone(),
                block_m.block_hash.clone(),
            )])),
            Some(vec![deploy]),
            None,
            None,
            None,
            None,
            None,
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut snapshot = mk_casper_snapshot(dag);
        let latest_messages = block_w
            .justifications
            .iter()
            .map(|justification| {
                (
                    justification.validator.clone(),
                    justification.latest_block_hash.clone(),
                )
            })
            .collect();
        let floor_context = FloorContext::derive(
            &snapshot.dag,
            &block_store,
            &block_w.header.parents_hash_list,
            &latest_messages,
            FtThreshold::from_ppm(1_000_000),
            block_w.header.version,
        )
        .await
        .expect("live-rejection floor");
        assert_eq!(floor_context.floor.hash, genesis.block_hash);

        let rejected: DashSet<DeployLookupId> = DashSet::new();
        rejected.insert(deploy_id);
        snapshot.rejected_in_scope = Arc::new(rejected);

        let result = Validate::repeat_deploy_at_floor(
            &block_w,
            &mut snapshot,
            &block_store,
            50,
            Some(&floor_context),
        );
        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::PrematureDeployRetry))
        );
    })
    .await
}

/// Regression test for `repeat_deploy`'s `rejected_in_scope` exemption.
///
/// The on-chain rejection record makes the exemption a pure function of the
/// block's parent scope. The settled rejection permits one recovery inclusion.
///
/// DAG: genesis (no deploys) → block_x (body.deploys=[deploy]) →
/// block_m (rejected_deploys=[deploy]) → block_w (body.deploys=[deploy],
/// the re-inclusion). Latest canonical disposition in block_w's parent
/// scope is the rejection at block_m, so re-inclusion is legal recovery.
///
/// Companion test:
/// `repeat_deploy_blocks_double_execution_when_finalized_and_in_rejected_in_scope`
/// covers the symmetric case where the sig's latest disposition in the
/// parent scope is a WIN (clean inclusion, never rejected) and the
/// recovery exemption must NOT apply.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_validation_allows_recovered_deploy_from_rejected_in_scope() {
    use std::sync::Arc;

    use dashmap::DashSet;

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let deploy_id = deploy
            .deploy_id_for_protocol(casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION)
            .expect("protocol-v6 deploy identity");
        let deploy_id_v6 = match &deploy_id {
            DeployLookupId::V6(deploy_id) => *deploy_id,
            DeployLookupId::Legacy(_) => unreachable!(),
        };

        // Genesis carries no user deploys: keeps the LFB clean of `deploy`
        // so the resolver cannot find a canonical clean inclusion.
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        // block_x carries the deploy in body.deploys but is NOT marked
        // approved/finalized (insert_indexed only marks genesis approved).
        // The resolver's LFB BFS walks main parents up from genesis and
        // never visits block_x, so the resolver returns `Pending`.
        let block_x = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            Some(vec![deploy.clone()]),
            None,
            None,
            None,
            None,
            None,
        );

        // block_m is the merge that rejected the deploy: its on-chain
        // rejected_deploys record is the disposition the deterministic
        // exemption reads (and the source `rejected_in_scope` is derived
        // from in the real pipeline).
        let block_m = create_block_with_merge_facts(
            &mut block_store,
            &mut block_dag_storage,
            vec![block_x.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            MergeFacts {
                rejected_deploys: vec![RejectedDeploy::occurrence_v6(
                    deploy_id_v6,
                    block_x.block_hash.clone(),
                    RejectedDeployReason::MergeConflict,
                )],
                ..Default::default()
            },
        );

        // block_w re-includes the deploy. repeat_deploy walks block_w's
        // ancestor chain and finds block_x with deploy in body.deploys —
        // an "ancestor occurrence" that without the exemption would be
        // flagged as repeat.
        let block_w = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![block_m.block_hash.clone()],
            &genesis,
            None,
            None,
            Some(HashMap::from([(
                block_m.sender.clone(),
                block_m.block_hash.clone(),
            )])),
            Some(vec![deploy]),
            None,
            None,
            None,
            None,
            None,
        );

        let dag = block_dag_storage.get_representation().expect("dag representation");
        let mut snapshot = mk_casper_snapshot(dag);
        let latest_messages = block_w
            .justifications
            .iter()
            .map(|justification| {
                (
                    justification.validator.clone(),
                    justification.latest_block_hash.clone(),
                )
            })
            .collect();
        let floor_context = FloorContext::derive(
            &snapshot.dag,
            &block_store,
            &block_w.header.parents_hash_list,
            &latest_messages,
            FtThreshold::from_ppm(0),
            block_w.header.version,
        )
        .await
        .expect("certified recovery floor");
        assert_eq!(floor_context.floor.hash, block_m.block_hash);

        // The snapshot flag mirrors what the recovery pipeline derives from
        // the on-chain record above; the validation exemption itself no
        // longer reads it (node-local), but keep it for realism.
        let rejected: DashSet<DeployLookupId> = DashSet::new();
        rejected.insert(deploy_id);
        snapshot.rejected_in_scope = Arc::new(rejected);

        let result = Validate::repeat_deploy_at_floor(
            &block_w,
            &mut snapshot,
            &block_store,
            50,
            Some(&floor_context),
        );
        assert_eq!(
            result,
            Either::Right(ValidBlock::Valid),
            "recovery re-inclusion of a rejected-in-scope sig with status=Pending must validate; got {:?}",
            result,
        );
    })
    .await
}

/// Companion to `repeat_deploy_validation_allows_recovered_deploy_from_rejected_in_scope`.
/// Tests the symmetric case the recovery exemption must NOT cover: a sig
/// that is in `rejected_in_scope` but ALSO has a clean canonical
/// inclusion (status `Finalized`). Re-including a Finalized sig is
/// double-execution, not recovery — the catchup gate
/// (`should_admit_to_rejected_buffer`) is the primary defense, but the
/// repeat-deploy validator must serve as a second line in case the gate
/// misses.
///
/// DAG: genesis (body.deploys=[deploy], LFB) → block_w
/// (body.deploys=[deploy], re-inclusion). Genesis IS the LFB so the
/// resolver finds a clean canonical inclusion of `deploy` in genesis
/// and returns `Finalized`. The recovery exemption must therefore NOT
/// apply, and the repeat check must catch the duplicate inclusion.
///
/// Pre-fix: the rejected_in_scope filter in `repeat_deploy` exempts
/// the sig unconditionally → returns `Valid` → double-execution slips
/// through. This test fails.
///
/// Post-fix: the filter is gated on `status != Finalized`. The sig is
/// Finalized, so it is NOT exempted; ancestor scan finds the clean
/// inclusion in genesis and returns `InvalidRepeatDeploy`. This test
/// passes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_blocks_double_execution_when_finalized_and_in_rejected_in_scope() {
    use std::sync::Arc;

    use dashmap::DashSet;

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let deploy_id = deploy
            .deploy_id_for_protocol(casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION)
            .expect("protocol-v6 deploy identity");

        // Genesis IS the LFB and contains `deploy` clean in body.deploys.
        // The resolver therefore reports `Finalized` for this sig.
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![deploy.clone()]),
            None,
            None,
            None,
            None,
        );

        // block_w re-includes the deploy. ancestor scan would find genesis's
        // clean inclusion if not exempted by the rejected_in_scope filter.
        let block_w = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            Some(vec![deploy]),
            None,
            None,
            None,
            None,
            None,
        );

        let dag = block_dag_storage.get_representation().expect("dag representation");
        let mut snapshot = mk_casper_snapshot(dag);

        // Same `rejected_in_scope` membership as the recovery test — the
        // gap is exactly that the repeat_deploy filter cannot distinguish
        // "rejected somewhere, recoverable" from "finalized somewhere,
        // non-recoverable" via this set alone.
        let rejected: DashSet<DeployLookupId> = DashSet::new();
        rejected.insert(deploy_id);
        snapshot.rejected_in_scope = Arc::new(rejected);

        let result = Validate::repeat_deploy(&block_w, &mut snapshot, &block_store, 50);
        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy)),
            "double-execution of a Finalized sig (rejected_in_scope membership notwithstanding) must be caught; got {:?}",
            result,
        );
    })
    .await
}

// ============================================================================
// Parent validation tests - Testing validator progress check (InvalidParents)
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_allow_first_block_from_new_validator() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let bonds = vec![Bond {
            validator: v0.clone(),
            stake: 10,
        }];

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

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // First block from v0 - build without inserting (we're validating it)
        let b1 = build_block(
            vec![genesis.block_hash.clone()],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::parents(&b1, &genesis, &mut casper_snapshot, -1, i32::MAX, 0, false);
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_allow_empty_block_when_new_parents_exist() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let v1 = generate_validator(Some("Validator1"));
        let bonds = vec![
            Bond {
                validator: v0.clone(),
                stake: 10,
            },
            Bond {
                validator: v1.clone(),
                stake: 10,
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

        // v0 creates first block (inserted into DAG - this is v0's "previous" block)
        let b1 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(v0.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );

        // v1 creates a block (inserted into DAG - represents a block v0 receives)
        let b2 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(v1.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // v0 creates empty block with parents [b1, b2] - build without inserting
        // b2 is new (not an ancestor of b1), so this should be valid
        let b3 = build_block(
            vec![b1.block_hash.clone(), b2.block_hash.clone()],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            Some(vec![Justification {
                validator: v0.clone(),
                latest_block_hash: b1.block_hash.clone(),
            }]),
            None,
            None,
            None,
            None,
            Some(2),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::parents(&b3, &genesis, &mut casper_snapshot, -1, i32::MAX, 0, false);
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_reject_empty_block_when_no_new_parents_exist() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let bonds = vec![Bond {
            validator: v0.clone(),
            stake: 10,
        }];

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

        // v0 creates first block (inserted into DAG - this is v0's "previous" block)
        let b1 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(v0.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // v0 creates another empty block with parent [b1] - build without inserting
        // No new parents (b1 is an ancestor of itself), so this should fail
        let b2 = build_block(
            vec![b1.block_hash.clone()],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            Some(vec![Justification {
                validator: v0.clone(),
                latest_block_hash: b1.block_hash.clone(),
            }]),
            None,
            None,
            None,
            None,
            Some(2),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::parents(&b2, &genesis, &mut casper_snapshot, -1, i32::MAX, 0, false);
        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents))
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_progress_is_independent_of_receiver_finalization_cache() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let validator = generate_validator(Some("Validator0"));
        let bonds = vec![Bond {
            validator: validator.clone(),
            stake: 10,
        }];
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
        let first = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(validator.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );
        let second = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![first.block_hash.clone()],
            &genesis,
            Some(validator.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(2),
            None,
        );
        let candidate = build_block(
            vec![genesis.block_hash.clone()],
            Some(validator.clone()),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            Some(bonds),
            Some(vec![Justification {
                validator,
                latest_block_hash: second.block_hash.clone(),
            }]),
            None,
            None,
            None,
            None,
            Some(3),
        );
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut without_local_finalization = mk_casper_snapshot(dag.clone());
        let mut with_local_finalization_dag = dag;
        with_local_finalization_dag
            .finalized_blocks_set
            .insert(first.block_hash);
        let mut with_local_finalization = mk_casper_snapshot(with_local_finalization_dag);

        let expected = Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents));
        assert_eq!(
            Validate::parents(
                &candidate,
                &genesis,
                &mut without_local_finalization,
                -1,
                i32::MAX,
                0,
                false,
            ),
            expected
        );
        assert_eq!(
            Validate::parents(
                &candidate,
                &genesis,
                &mut with_local_finalization,
                -1,
                i32::MAX,
                0,
                false,
            ),
            expected
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_allow_block_with_user_deploys_regardless_of_parents() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let bonds = vec![Bond {
            validator: v0.clone(),
            stake: 10,
        }];

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

        // v0 creates first block (inserted into DAG - this is v0's "previous" block)
        let b1 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(v0.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // Create a user deploy (uses construct_deploy helper)
        let user_deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();

        // v0 creates block with user deploys and parent [b1] - build without inserting
        // No new parents but has deploys, so this should still be valid
        let b2 = build_block(
            vec![b1.block_hash.clone()],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            None,
            Some(vec![user_deploy]),
            None,
            None,
            None,
            Some(2),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::parents(&b2, &genesis, &mut casper_snapshot, -1, i32::MAX, 0, false);
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_allow_proposal_when_previous_block_is_genesis() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let bonds = vec![Bond {
            validator: v0.clone(),
            stake: 10,
        }];

        // Create genesis with v0 as sender (so v0's "previous block" is genesis)
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            Some(v0.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // v0 creates empty block with parent [genesis] - build without inserting
        // Since v0's previous block is genesis (which has no parents), this should be valid
        let b1 = build_block(
            vec![genesis.block_hash.clone()],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::parents(&b1, &genesis, &mut casper_snapshot, -1, i32::MAX, 0, false);
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_enforce_max_number_of_parents_constraint() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let v1 = generate_validator(Some("Validator1"));
        let v2 = generate_validator(Some("Validator2"));
        let bonds = vec![
            Bond {
                validator: v0.clone(),
                stake: 10,
            },
            Bond {
                validator: v1.clone(),
                stake: 10,
            },
            Bond {
                validator: v2.clone(),
                stake: 10,
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

        let b1 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(v0.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );

        let b2 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(v1.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );

        let b3 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(v2.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
        );

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // Create block with 3 parents but maxNumberOfParents = 2 - build without inserting
        let b4 = build_block(
            vec![
                b1.block_hash.clone(),
                b2.block_hash.clone(),
                b3.block_hash.clone(),
            ],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(2),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        // maxNumberOfParents = 2, but block has 3 parents
        let result = Validate::parents(&b4, &genesis, &mut casper_snapshot, 2, i32::MAX, 0, false);
        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents))
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_summary_validation_should_short_circuit_after_first_invalidity() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let _genesis = create_chain(&mut block_store, &mut block_dag_storage, 2, vec![]);
        let block = block_dag_storage.lookup_by_id_unsafe(1);
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        let secp256k1 = Secp256k1;
        let (sk, pk) = secp256k1.new_key_pair();
        let sender = Bytes::copy_from_slice(&pk.bytes);
        let latest_message_opt = dag.latest_message(&sender).unwrap_or(None);
        let _seq_num =
            latest_message_opt.map_or(0, |block_metadata| block_metadata.sequence_number) + 1;

        let signed_block = ValidatorIdentity::new(&sk)
            .sign_block(&with_seq_num(&with_block_number(&block, 17), 1));

        let mut casper_snapshot = mk_casper_snapshot(dag);

        // Use unlimited parents (-1) like in Scala tests: Estimator.UnlimitedParents
        let max_number_of_parents = -1;

        let result = Validate::block_summary(
            &signed_block,
            &get_random_block(
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(Box::new(|block| proto_util::hash_block(&block))),
            ),
            &mut casper_snapshot,
            "root",
            i32::MAX,
            max_number_of_parents,
            i32::MAX, // max_parent_depth: disable depth check for this test
            0,        // depth_buffer: irrelevant when depth check disabled
            &block_store,
            false,
        )
        .await;

        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockNumber))
        );

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns.size should be(1)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn justification_shape_validation_does_not_use_post_state_bonds_as_authority() {
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

        let b2 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(v2.clone()),
            Some(bonds.clone()),
            Some(create_justifications(vec![
                (v1.clone(), genesis.block_hash.clone()),
                (v2.clone(), genesis.block_hash.clone()),
            ])),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let b3 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(v1.clone()),
            Some(bonds.clone()),
            Some(create_justifications(vec![
                (v1.clone(), genesis.block_hash.clone()),
                (v2.clone(), genesis.block_hash.clone()),
            ])),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let b4 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b2.block_hash.clone()],
            &genesis,
            Some(v2.clone()),
            Some(bonds.clone()),
            Some(create_justifications(vec![
                (v1.clone(), genesis.block_hash.clone()),
                (v2.clone(), b2.block_hash.clone()),
            ])),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let b5 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b2.block_hash.clone()],
            &genesis,
            Some(v1.clone()),
            Some(bonds.clone()),
            Some(create_justifications(vec![
                (v1.clone(), b3.block_hash.clone()),
                (v2.clone(), b2.block_hash.clone()),
            ])),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let _b6 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b4.block_hash.clone()],
            &genesis,
            Some(v2.clone()),
            Some(bonds.clone()),
            Some(create_justifications(vec![
                (v1.clone(), b5.block_hash.clone()),
                (v2.clone(), b4.block_hash.clone()),
            ])),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let b7 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b4.block_hash.clone()],
            &genesis,
            Some(v1.clone()),
            Some(vec![]),
            Some(create_justifications(vec![
                (v1.clone(), b5.block_hash.clone()),
                (v2.clone(), b4.block_hash.clone()),
            ])),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let b8 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b7.block_hash.clone()],
            &genesis,
            Some(v1.clone()),
            Some(bonds.clone()),
            Some(create_justifications(vec![
                (v1.clone(), b7.block_hash.clone()),
                (v2.clone(), b4.block_hash.clone()),
            ])),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let condition = (1..=7).all(|i| {
            let block = block_dag_storage.lookup_by_id_unsafe(i as i64);
            let _dag = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let result = Validate::justifications_well_formed(&block);
            result == Either::Right(ValidBlock::Valid)
        });
        assert!(condition);

        let provenance_condition = (1..=7).all(|i| {
            let block = block_dag_storage.lookup_by_id_unsafe(i as i64);
            Validate::justification_provenance(&block, &genesis, &block_store)
                == Either::Right(ValidBlock::Valid)
        });
        assert!(provenance_condition);

        let mut forged = b8;
        let v1_justification = forged
            .justifications
            .iter_mut()
            .find(|justification| justification.validator == v1)
            .expect("v1 justification");
        v1_justification.latest_block_hash = b4.block_hash;
        assert_eq!(
            Validate::justification_provenance(&forged, &genesis, &block_store),
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidFollows))
        );
    })
    .await
}

#[test]
fn justification_shape_validation_rejects_parentless_nonapproved_block() {
    let mut block = models::rust::block_implicits::get_random_block_default();
    block.header.parents_hash_list.clear();

    assert_eq!(
        Validate::justifications_well_formed(&block),
        Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn justification_regression_validation_should_return_valid_for_proper_justifications_and_justification_regression_detected_otherwise(
) {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator 1"));
        let v1 = generate_validator(Some("Validator 2"));

        // bonds = List(v0, v1).zipWithIndex.map { case (v, i) => Bond(v, 2L * i.toLong + 1L) }
        let bonds = vec![
            Bond {
                validator: v0.clone(),
                stake: 1, // 2 * 0 (index) + 1 = 1
            },
            Bond {
                validator: v1.clone(),
                stake: 3, // 2 * 1(index) + 1 = 3
            },
        ];

        let b0 = create_genesis_block(
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

        let b1 = create_validator_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b0.clone()],
            &b0,
            vec![b0.clone(), b0.clone()],
            v0.clone(),
            bonds.clone(),
            None,
            None,
            SHARD_ID.to_string(),
        );

        let b2 = create_validator_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b1.clone()],
            &b0,
            vec![b1.clone(), b0.clone()],
            v0.clone(),
            bonds.clone(),
            None,
            None,
            SHARD_ID.to_string(),
        );

        let b3 = create_validator_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b0.clone()],
            &b0,
            vec![b2.clone(), b0.clone()],
            v1.clone(),
            bonds.clone(),
            None,
            None,
            SHARD_ID.to_string(),
        );

        let b4 = create_validator_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b3.clone()],
            &b0,
            vec![b2.clone(), b3.clone()],
            v1.clone(),
            bonds.clone(),
            None,
            None,
            SHARD_ID.to_string(),
        );

        let condition = (0..=4).all(|i| {
            let block = block_dag_storage.lookup_by_id_unsafe(i as i64);
            let dag = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let mut casper_snapshot = mk_casper_snapshot(dag);
            let result = Validate::justification_regressions(&block, &mut casper_snapshot);
            result == Either::Right(ValidBlock::Valid)
        });
        assert!(condition);

        // The justification block for validator 0 should point to b2 or above.
        let justifications_with_regression = vec![
            Justification {
                validator: v0.clone(),
                latest_block_hash: b1.block_hash.clone(),
            },
            Justification {
                validator: v1.clone(),
                latest_block_hash: b4.block_hash.clone(),
            },
        ];

        let block_with_justification_regression = get_random_block(
            None,
            None,
            None,
            None,
            Some(v1.clone()),
            None,
            None,
            None,
            Some(justifications_with_regression),
            None,
            None,
            None,
            None,
            Some(Box::new(|block| proto_util::hash_block(&block))),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::justification_regressions(
            &block_with_justification_regression,
            &mut casper_snapshot,
        );
        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::JustificationRegression))
        );
        casper_snapshot
            .dag
            .latest_messages_map
            .insert(v1, b0.block_hash.clone());
        assert_eq!(
            Validate::justification_regressions(
                &block_with_justification_regression,
                &mut casper_snapshot,
            ),
            result,
            "receiver-local latest-message state cannot change regression validity"
        );

        // Add log validation mechanism when LogStub mechanism from Scala will be implemented on Rust.
        // log.warns.size shouldBe 1
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn justification_regression_validation_should_return_valid_for_regressive_invalid_blocks() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator 1"));
        let v1 = generate_validator(Some("Validator 2"));

        let bonds = vec![
            Bond {
                validator: v0.clone(),
                stake: 1, // 2 * 0 (index) + 1 = 1
            },
            Bond {
                validator: v1.clone(),
                stake: 3, // 2 * 1 (index) + 1 = 3
            },
        ];

        let b0 = create_genesis_block(
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

        let b1 = create_validator_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b0.clone()],
            &b0,
            vec![b0.clone(), b0.clone()],
            v0.clone(),
            bonds.clone(),
            Some(1),
            None,
            SHARD_ID.to_string(),
        );

        let b2 = create_validator_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b0.clone()],
            &b0,
            vec![b1.clone(), b0.clone()],
            v1.clone(),
            bonds.clone(),
            Some(1),
            None,
            SHARD_ID.to_string(),
        );

        let b3 = create_validator_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b0.clone()],
            &b0,
            vec![b1.clone(), b2.clone()],
            v0.clone(),
            bonds.clone(),
            Some(2),
            None,
            SHARD_ID.to_string(),
        );

        let b4 = create_validator_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b0.clone()],
            &b0,
            vec![b3.clone(), b2.clone()],
            v1.clone(),
            bonds.clone(),
            Some(2),
            None,
            SHARD_ID.to_string(),
        );

        let b5 = create_validator_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b0.clone()],
            &b0,
            vec![b3.clone(), b4.clone()],
            v0.clone(),
            bonds.clone(),
            Some(1),
            Some(true),
            SHARD_ID.to_string(),
        );

        let justifications_with_invalid_block = vec![
            Justification {
                validator: v0.clone(),
                latest_block_hash: b5.block_hash.clone(),
            },
            Justification {
                validator: v1.clone(),
                latest_block_hash: b4.block_hash.clone(),
            },
        ];

        let block_with_invalid_justification = get_random_block(
            None,
            None,
            None,
            None,
            Some(v1.clone()),
            None,
            None,
            None,
            Some(justifications_with_invalid_block),
            None,
            None,
            None,
            None,
            None,
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::justification_regressions(
            &block_with_invalid_justification,
            &mut casper_snapshot,
        );
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bonds_cache_validation_should_succeed_on_a_valid_block_and_fail_on_modified_bonds() {
    with_storage(|block_store, mut block_dag_storage| async move {
        let context = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let genesis = context.genesis_block.clone();

        block_dag_storage
            .insert(
                &genesis,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
            )
            .unwrap();

        let mut kvm = mk_test_rnode_store_manager_from_genesis(&context);

        let m_store = crate::util::rholang::resources::mergeable_store_from_dyn(&mut *kvm)
            .await
            .unwrap();

        let runtime_manager = RuntimeManager::create_with_store(
            (*kvm).r_space_stores().await.unwrap(),
            m_store,
            std::sync::Arc::new(Genesis::default_mergeable_tags()),
            rholang::rust::interpreter::external_services::ExternalServices::noop(),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        interpreter_util::validate_block_checkpoint(
            &genesis,
            &block_store,
            &mut casper_snapshot,
            &runtime_manager,
            None,
        )
        .await
        .unwrap();

        let result_valid = Validate::bonds_cache(&genesis, &runtime_manager).await;
        assert_eq!(result_valid, Either::Right(ValidBlock::Valid));

        let modified_bonds = vec![];

        let mut modified_post_state = genesis.body.state.clone();
        modified_post_state.bonds = modified_bonds;

        let mut modified_body = genesis.body.clone();
        modified_body.state = modified_post_state;

        let mut modified_genesis = genesis.clone();
        modified_genesis.body = modified_body;

        let result_invalid = Validate::bonds_cache(&modified_genesis, &runtime_manager).await;
        assert_eq!(
            result_invalid,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBondsCache))
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn field_format_validation_should_succeed_on_a_valid_block_and_fail_on_empty_fields() {
    with_storage(|_block_store, mut block_dag_storage| async move {
        let context = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let (sk, pk) = &context.validator_key_pairs[0];

        block_dag_storage
            .insert(
                &context.genesis_block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
            )
            .unwrap();
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let sender = Bytes::copy_from_slice(&pk.bytes);
        let latest_message_opt = dag.latest_message(&sender).unwrap_or(None);
        let seq_num =
            latest_message_opt.map_or(0, |block_metadata| block_metadata.sequence_number) + 1;

        let genesis =
            ValidatorIdentity::new(sk).sign_block(&with_seq_num(&context.genesis_block, seq_num));

        let result = Validate::format_of_fields(&genesis);
        assert!(result);

        let invalid_block = with_sig(&genesis, &Bytes::new());
        let result = Validate::format_of_fields(&invalid_block);
        assert!(!result);

        let invalid_block = with_sig_algorithm(&genesis, "");
        let result = Validate::format_of_fields(&invalid_block);
        assert!(!result);

        let invalid_block = with_shard_id(&genesis, "");
        let result = Validate::format_of_fields(&invalid_block);
        assert!(!result);

        let invalid_block = with_post_state_hash(&genesis, &Bytes::new());
        let result = Validate::format_of_fields(&invalid_block);
        assert!(!result);
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_hash_format_validation_should_fail_on_invalid_hash() {
    with_storage(|_block_store, mut block_dag_storage| async move {
        let context = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let (sk, pk) = &context.validator_key_pairs[0];
        let sender = Bytes::copy_from_slice(&pk.bytes);

        block_dag_storage
            .insert(
                &context.genesis_block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
            )
            .unwrap();
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        let latest_message_opt = dag.latest_message(&sender).unwrap_or(None);
        let seq_num =
            latest_message_opt.map_or(0, |block_metadata| block_metadata.sequence_number) + 1;

        let genesis =
            ValidatorIdentity::new(sk).sign_block(&with_seq_num(&context.genesis_block, seq_num));

        let result = Validate::block_hash(&genesis);
        assert_eq!(result, Either::Right(ValidBlock::Valid));

        let invalid_block = with_block_hash(&genesis, &Bytes::copy_from_slice("123".as_bytes()));
        let result = Validate::block_hash(&invalid_block);
        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockHash))
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_version_validation_should_work() {
    with_storage(|_block_store, mut block_dag_storage| async move {
        let context = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let (sk, pk) = &context.validator_key_pairs[0];
        let sender = Bytes::copy_from_slice(&pk.bytes);

        block_dag_storage
            .insert(
                &context.genesis_block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
            )
            .unwrap();
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        let latest_message_opt = dag.latest_message(&sender).unwrap_or(None);
        let seq_num =
            latest_message_opt.map_or(0, |block_metadata| block_metadata.sequence_number) + 1;

        let genesis =
            ValidatorIdentity::new(sk).sign_block(&with_seq_num(&context.genesis_block, seq_num));

        let result = Validate::version(&genesis, -1);
        assert!(!result);

        let result = Validate::version(
            &genesis,
            casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
        );
        assert!(result);
    })
    .await
}

// ── Parent-depth enforcement (symmetric to proposer-side filterDeepParents) ──
//
// `validate::parents` rejects blocks whose parents fall outside
// `max_parent_depth + depth_buffer` from the highest tip. Joiners that LFS-sync
// to the LFB hold rspace history only for blocks within this horizon; rejecting
// out-of-horizon blocks here prevents `UnknownRootError` cascades during
// validation. Symmetric to the proposer-side `Estimator::filterDeepParents`
// in `engine::multi_parent_casper::create_block`.

fn build_linear_chain(
    block_store: &mut KeyValueBlockStore,
    block_dag_storage: &mut IndexedBlockDagStorage,
    length: usize,
    bonds: Vec<Bond>,
    validator: Bytes,
) -> Vec<BlockMessage> {
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
    let mut chain = vec![genesis.clone()];
    for i in 1..length {
        let parent = chain.last().unwrap().clone();
        let block = create_block(
            block_store,
            block_dag_storage,
            vec![parent.block_hash.clone()],
            &genesis,
            Some(validator.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(i as i32),
            None,
        );
        chain.push(block);
    }
    chain
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_pass_when_parent_within_horizon() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let bonds = vec![Bond {
            validator: v0.clone(),
            stake: 10,
        }];

        // Chain of 5 blocks. Tip at chain[4] (block_number=4).
        let chain = build_linear_chain(
            &mut block_store,
            &mut block_dag_storage,
            5,
            bonds.clone(),
            v0.clone(),
        );
        let genesis = chain[0].clone();
        let tip = chain[4].clone();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // Test block parents the tip directly (depth 0). max_parent_depth=2, buffer=0.
        let test_block = build_block(
            vec![tip.block_hash.clone()],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(5),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::parents(
            &test_block,
            &genesis,
            &mut casper_snapshot,
            -1,   // max_number_of_parents (unlimited)
            2,    // max_parent_depth
            0,    // depth_buffer
            true, // disable_validator_progress_check (isolate depth check)
        );
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_pass_at_horizon_boundary() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let bonds = vec![Bond {
            validator: v0.clone(),
            stake: 10,
        }];

        // Chain of 6 blocks. Max block_number = 5, latest_block_number() returns 6.
        let chain = build_linear_chain(
            &mut block_store,
            &mut block_dag_storage,
            6,
            bonds.clone(),
            v0.clone(),
        );
        let genesis = chain[0].clone();
        let tip = chain[5].clone();
        let parent_at_depth_4 = chain[1].clone();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let test_block = build_block(
            vec![tip.block_hash.clone(), parent_at_depth_4.block_hash.clone()],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(6),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        // depth=4, max_parent_depth=4, buffer=0 → 4 <= 4, passes (boundary)
        let result = Validate::parents(
            &test_block,
            &genesis,
            &mut casper_snapshot,
            -1,
            4,
            0,
            true, // disable_validator_progress_check (isolate depth check)
        );
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_pass_at_horizon_plus_buffer_boundary() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let bonds = vec![Bond {
            validator: v0.clone(),
            stake: 10,
        }];

        // Chain of 7 blocks. Max block_number = 6, latest_block_number() returns 7.
        let chain = build_linear_chain(
            &mut block_store,
            &mut block_dag_storage,
            7,
            bonds.clone(),
            v0.clone(),
        );
        let genesis = chain[0].clone();
        let tip = chain[6].clone();
        let parent_at_depth_5 = chain[1].clone();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let test_block = build_block(
            vec![tip.block_hash.clone(), parent_at_depth_5.block_hash.clone()],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(7),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        // depth=5, max_parent_depth=4, buffer=1 → 5 <= 4+1, passes (boundary)
        let result = Validate::parents(
            &test_block,
            &genesis,
            &mut casper_snapshot,
            -1,
            4,
            1,
            true, // disable_validator_progress_check (isolate depth check)
        );
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_reject_when_parent_beyond_horizon() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let bonds = vec![Bond {
            validator: v0.clone(),
            stake: 10,
        }];

        // Chain of 7 blocks. Max block_number = 6, latest_block_number() returns 7.
        let chain = build_linear_chain(
            &mut block_store,
            &mut block_dag_storage,
            7,
            bonds.clone(),
            v0.clone(),
        );
        let genesis = chain[0].clone();
        let tip = chain[6].clone();
        let parent_beyond_horizon = chain[1].clone();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let test_block = build_block(
            vec![
                tip.block_hash.clone(),
                parent_beyond_horizon.block_hash.clone(),
            ],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(7),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        // depth=5, max_parent_depth=4, buffer=0 → 5 > 4, REJECT
        let result = Validate::parents(
            &test_block,
            &genesis,
            &mut casper_snapshot,
            -1,
            4,
            0,
            true, // disable_validator_progress_check (isolate depth check)
        );
        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents))
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_exempt_genesis_from_depth_check() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let bonds = vec![Bond {
            validator: v0.clone(),
            stake: 10,
        }];

        // Chain of 12 blocks. Tip at chain[11] (block_number=11). Genesis depth=11.
        let chain = build_linear_chain(
            &mut block_store,
            &mut block_dag_storage,
            12,
            bonds.clone(),
            v0.clone(),
        );
        let genesis = chain[0].clone();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // Test block parents genesis directly. depth=11, max_parent_depth=4, buffer=0 →
        // would normally be REJECTED (11 > 4) — but genesis (block_number=0) is exempt.
        let test_block = build_block(
            vec![chain[11].block_hash.clone(), genesis.block_hash.clone()],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(12),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        let result = Validate::parents(
            &test_block,
            &genesis,
            &mut casper_snapshot,
            -1,
            4,
            0,
            true, // disable_validator_progress_check (isolate depth check)
        );
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_should_skip_depth_check_when_max_parent_depth_is_unlimited() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let bonds = vec![Bond {
            validator: v0.clone(),
            stake: 10,
        }];

        // Chain of 7 blocks. Max block_number = 6, latest_block_number() returns 7.
        let chain = build_linear_chain(
            &mut block_store,
            &mut block_dag_storage,
            7,
            bonds.clone(),
            v0.clone(),
        );
        let genesis = chain[0].clone();
        let tip = chain[6].clone();
        let old_parent = chain[1].clone();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let test_block = build_block(
            vec![tip.block_hash.clone(), old_parent.block_hash.clone()],
            Some(v0.clone()),
            now,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(7),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = mk_casper_snapshot(dag);

        // depth=5 but max_parent_depth=i32::MAX → check skipped, passes regardless
        let result = Validate::parents(
            &test_block,
            &genesis,
            &mut casper_snapshot,
            -1,
            i32::MAX,
            0,
            true, // disable_validator_progress_check (isolate depth check)
        );
        assert_eq!(result, Either::Right(ValidBlock::Valid));
    })
    .await
}

/// C12 (GuardBridge `honest_forkchoice_parents_validate` / `capped_parents_validate`):
/// `Validate::parents` is the receive-side mirror of the proposer's `filter_deep_parents`.
/// A parent WITHIN the depth horizon (`highest_parent − parent_number ≤ max_parent_depth +
/// depth_buffer`) is ACCEPTED; one BEYOND it is `InvalidParents`; and `depth_buffer`
/// extends the horizon. Both existing `Validate::parents` tests pass `i32::MAX` (depth
/// check OFF), so the finite-horizon accept / reject / buffer paths were entirely untested —
/// this closes the receive-side half of the C12 abstract bridge. The test block is sent by a
/// FRESH validator (no prior message), so it is `Valid` as soon as the depth check passes
/// (validate.rs:1020), isolating the depth filter from the validator-progress check.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parent_validation_enforces_max_parent_depth_horizon() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let v0 = generate_validator(Some("Validator0"));
        let v_fresh = generate_validator(Some("FreshValidator"));
        let bonds = vec![
            Bond { validator: v0.clone(), stake: 10 },
            Bond { validator: v_fresh.clone(), stake: 10 },
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

        // Linear chain genesis <- b1 <- b2 <- b3, all by v0 (consecutive block numbers),
        // so b3 is the highest tip. Depth is relative: b2 is at depth 1 from b3, b1 at depth 2.
        let b1 = create_block(
            &mut block_store, &mut block_dag_storage, vec![genesis.block_hash.clone()],
            &genesis, Some(v0.clone()), Some(bonds.clone()),
            None, None, None, None, None, Some(1), None,
        );
        let b2 = create_block(
            &mut block_store, &mut block_dag_storage, vec![b1.block_hash.clone()],
            &genesis, Some(v0.clone()), Some(bonds.clone()),
            None, None, None, None, None, Some(2), None,
        );
        let b3 = create_block(
            &mut block_store, &mut block_dag_storage, vec![b2.block_hash.clone()],
            &genesis, Some(v0.clone()), Some(bonds.clone()),
            None, None, None, None, None, Some(3), None,
        );
        let _ = &b3;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // Both candidates bind their depth boundary to b3, so unrelated receiver-local tips
        // cannot change the result.
        let b_ok = build_block(
            vec![b3.block_hash.clone(), b2.block_hash.clone()], Some(v_fresh.clone()), now, Some(bonds.clone()),
            None, None, None, None, None, Some(1),
        );
        let b_deep = build_block(
            vec![b3.block_hash.clone(), b1.block_hash.clone()], Some(v_fresh.clone()), now, Some(bonds.clone()),
            None, None, None, None, None, Some(1),
        );
        let b_taller_secondary = build_block(
            vec![b1.block_hash.clone(), b3.block_hash.clone(), b2.block_hash.clone()],
            Some(v_fresh.clone()),
            now,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
        );
        let missing_parent = Bytes::from(vec![77; models::rust::block_hash::LENGTH]);
        let b_missing_metadata = build_block(
            vec![b3.block_hash.clone(), missing_parent],
            Some(v_fresh.clone()),
            now,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(1),
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut snap = mk_casper_snapshot(dag);

        let b3_num = snap.dag.lookup_unsafe(&b3.block_hash).expect("b3 metadata").block_number;
        let b1_num = snap.dag.lookup_unsafe(&b1.block_hash).expect("b1 metadata").block_number;
        let b2_num = snap.dag.lookup_unsafe(&b2.block_hash).expect("b2 metadata").block_number;
        let depth_b1 = b3_num - b1_num;
        let depth_b2 = b3_num - b2_num;
        assert!(
            depth_b1 > depth_b2 && depth_b2 >= 1,
            "b1 must be strictly deeper than b2 and b2 not the tip (depths b1={depth_b1}, b2={depth_b2})"
        );
        // Horizon = depth_b2: b2 sits exactly at the horizon (accept), b1 is beyond it (reject).
        let horizon = depth_b2 as i32;

        // Accept: honest parent b2 within the horizon (depth_b2 ≤ horizon + buffer 0).
        let ok = Validate::parents(&b_ok, &genesis, &mut snap, -1, horizon, 0, false);
        assert_eq!(
            ok,
            Either::Right(ValidBlock::Valid),
            "an honest parent within the depth horizon must validate"
        );
        assert_eq!(
            Validate::parents(
                &b_taller_secondary,
                &genesis,
                &mut snap,
                -1,
                horizon,
                0,
                false,
            ),
            Either::Right(ValidBlock::Valid),
            "the selected first parent is preserved while secondary depth is measured from the freshest declared parent"
        );
        assert!(matches!(
            Validate::parents(
                &b_missing_metadata,
                &genesis,
                &mut snap,
                -1,
                horizon,
                0,
                false,
            ),
            Either::Left(BlockError::BlockException(_))
        ));
        let mut ahead_dag = snap.dag.clone();
        ahead_dag.height_map.insert(
            b3_num + 100,
            imbl::HashSet::from_iter([Bytes::from(vec![
                99;
                models::rust::block_hash::LENGTH
            ])]),
        );
        let mut ahead_snapshot = mk_casper_snapshot(ahead_dag);
        assert_eq!(
            Validate::parents(
                &b_ok,
                &genesis,
                &mut ahead_snapshot,
                -1,
                horizon,
                0,
                false,
            ),
            ok,
            "receiver-local blocks outside the candidate closure cannot change parent validity"
        );

        // Reject: parent b1 beyond the horizon (depth_b1 > horizon + buffer 0).
        let deep = Validate::parents(&b_deep, &genesis, &mut snap, -1, horizon, 0, false);
        assert_eq!(
            deep,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents)),
            "a parent beyond max_parent_depth must be InvalidParents"
        );

        // depth_buffer extends the horizon: the SAME too-deep parent b1 now validates when
        // depth_buffer lifts max_allowed_depth to cover depth_b1 exactly.
        let buffer_needed = (depth_b1 - depth_b2) as i32;
        let buffered = Validate::parents(&b_deep, &genesis, &mut snap, -1, horizon, buffer_needed, false);
        assert_eq!(
            buffered,
            Either::Right(ValidBlock::Valid),
            "depth_buffer must extend the accepted parent horizon"
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bonds_cache_uses_post_state_for_child_block_bonds() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let context = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let genesis = context.genesis_block.clone();

        block_store
            .put(genesis.block_hash.clone(), &genesis)
            .unwrap();
        block_dag_storage
            .insert(
                &genesis,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
            )
            .unwrap();

        let mut kvm = mk_test_rnode_store_manager_from_genesis(&context);
        let m_store = crate::util::rholang::resources::mergeable_store_from_dyn(&mut *kvm)
            .await
            .unwrap();
        let runtime_manager = RuntimeManager::create_with_store(
            (*kvm).r_space_stores().await.unwrap(),
            m_store,
            std::sync::Arc::new(Genesis::default_mergeable_tags()),
            rholang::rust::interpreter::external_services::ExternalServices::noop(),
        );

        let mut casper_snapshot = mk_casper_snapshot(
            block_dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        interpreter_util::validate_block_checkpoint(
            &genesis,
            &block_store,
            &mut casper_snapshot,
            &runtime_manager,
            None,
        )
        .await
        .unwrap();

        let floor_state = proto_util::post_state_hash(&genesis);
        let active = runtime_manager
            .get_active_validators(&floor_state)
            .await
            .unwrap();
        let floor_bonds: Vec<Bond> = runtime_manager
            .compute_bonds(&floor_state)
            .await
            .unwrap()
            .into_iter()
            .filter(|bond| active.contains(&bond.validator))
            .collect();
        let justifications: HashMap<_, _> = floor_bonds
            .iter()
            .map(|bond| (bond.validator.clone(), genesis.block_hash.clone()))
            .collect();

        let child = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(floor_bonds[0].validator.clone()),
            Some(floor_bonds.clone()),
            Some(justifications),
            None,
            Some(floor_state),
            None,
            None,
            Some(1),
            None,
        );
        let result_valid = Validate::bonds_cache(&child, &runtime_manager).await;
        assert_eq!(result_valid, Either::Right(ValidBlock::Valid));

        let mut modified_child = child.clone();
        modified_child.body.state.bonds = Vec::new();

        let result_invalid = Validate::bonds_cache(&modified_child, &runtime_manager).await;
        assert_eq!(
            result_invalid,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBondsCache))
        );
        let mut duplicate_bond = child.clone();
        duplicate_bond
            .body
            .state
            .bonds
            .push(duplicate_bond.body.state.bonds[0].clone());
        let duplicate = Validate::bonds_cache(&duplicate_bond, &runtime_manager).await;
        assert_eq!(
            duplicate,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBondsCache))
        );
    })
    .await
}

/// T-RECOMPUTE (`interpreter_util::validate_block_checkpoint` :259/:269 → `block_status.rs:153`).
///
/// This is the enforcement seam that makes ALL merge-determinism consequential: the
/// validator RECOMPUTES the parents' post-state (`compute_parents_post_state`) and, before
/// trusting the block, checks the recomputed pre-state and rejected-deploy set against what
/// the block RECORDED. A block that lies about either is rejected. Without this gate a
/// dishonest proposer could record any post-state and the merge proofs would be inert.
///
/// Three phases against ONE genesis+runtime (the recompute is deterministic and the two
/// reject paths return before any replay, so no state carries across phases):
///   • baseline  — untampered genesis recomputes a MATCHING pre-state and replays ⇒ `Right(Some)`.
///   • pre-state — flip one byte of the recorded `pre_state_hash`; the recompute disagrees ⇒
///                 `Right(None)` (reject, NO replay — the :259 gate).
///   • rejected  — append a bogus `rejected_deploys` sig the recompute never produces; pre-state
///                 still matches so control reaches the :269 gate ⇒ `Left(InvalidRejectedDeploy)`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validate_block_checkpoint_recompute_rejects_pre_state_and_rejected_deploy_tampering() {
    with_storage(|block_store, mut block_dag_storage| async move {
        let context = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let genesis = context.genesis_block.clone();

        block_store
            .put(genesis.block_hash.clone(), &genesis)
            .unwrap();
        block_dag_storage
            .insert(
                &genesis,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
            )
            .unwrap();

        let mut kvm = mk_test_rnode_store_manager_from_genesis(&context);
        let m_store = crate::util::rholang::resources::mergeable_store_from_dyn(&mut *kvm)
            .await
            .unwrap();
        let runtime_manager = RuntimeManager::create_with_store(
            (*kvm).r_space_stores().await.unwrap(),
            m_store,
            std::sync::Arc::new(Genesis::default_mergeable_tags()),
            rholang::rust::interpreter::external_services::ExternalServices::noop(),
        );

        // ── Phase 1 (baseline / GREEN): honest genesis recomputes + replays ──────────────
        let mut snap_ok = mk_casper_snapshot(
            block_dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        let valid = interpreter_util::validate_block_checkpoint(
            &genesis,
            &block_store,
            &mut snap_ok,
            &runtime_manager,
            None,
        )
        .await
        .expect("checkpoint (baseline)");
        assert!(
            matches!(valid, Either::Right(Some(_))),
            "untampered genesis must recompute a matching pre-state and replay to Some(state), got {valid:?}"
        );

        // ── Phase 2 (pre-state tamper / RED): recorded pre-state ≠ recompute ⇒ reject, NO replay ──
        let mut bad_pre = proto_util::pre_state_hash(&genesis).to_vec();
        bad_pre[0] ^= 0xFF;
        let mut tampered_pre = genesis.clone();
        tampered_pre.body.state.pre_state_hash = Bytes::from(bad_pre);

        let mut snap_pre = mk_casper_snapshot(
            block_dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        let rejected_pre = interpreter_util::validate_block_checkpoint(
            &tampered_pre,
            &block_store,
            &mut snap_pre,
            &runtime_manager,
            None,
        )
        .await
        .expect("checkpoint (pre-state tamper)");
        assert_eq!(
            rejected_pre,
            Either::Right(None),
            "a tampered pre-state hash must be rejected WITHOUT replay (the recompute-vs-recorded gate, :259)"
        );

        // ── Phase 3 (rejected-deploy tamper / RED): recorded rejected set ≠ recompute ⇒ InvalidRejectedDeploy ──
        let mut tampered_rej = genesis.clone();
        tampered_rej
            .body
            .rejected_deploys
            .push(RejectedDeploy::occurrence_v6(
                models::rust::deploy_id::DeployIdV6::try_from([0xAB; 32].as_slice())
                    .expect("fixed-width v6 deploy identity"),
                genesis.block_hash.clone(),
                RejectedDeployReason::MergeConflict,
            ));

        let mut snap_rej = mk_casper_snapshot(
            block_dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        let rejected_rej = interpreter_util::validate_block_checkpoint(
            &tampered_rej,
            &block_store,
            &mut snap_rej,
            &runtime_manager,
            None,
        )
        .await
        .expect("checkpoint (rejected-deploy tamper)");
        assert_eq!(
            rejected_rej,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRejectedDeploy)),
            "a block claiming a rejected-deploy the validator's recompute does not produce must be InvalidRejectedDeploy (:269)"
        );

        let mut tampered_effect = genesis.clone();
        tampered_effect
            .body
            .rejected_state_effects
            .push(StateEffectId {
                source_block_hash: Bytes::from_static(b"bogus-source"),
                execution_index: 0,
            });
        let mut snap_effect = mk_casper_snapshot(
            block_dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        let rejected_effect = interpreter_util::validate_block_checkpoint(
            &tampered_effect,
            &block_store,
            &mut snap_effect,
            &runtime_manager,
            None,
        )
        .await
        .expect("checkpoint (state-effect tamper)");
        assert_eq!(
            rejected_effect,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRejectedDeploy)),
        );

        let mut noncanonical_effects = genesis.clone();
        noncanonical_effects.body.rejected_state_effects = vec![
            StateEffectId {
                source_block_hash: Bytes::from_static(b"z-source"),
                execution_index: 0,
            },
            StateEffectId {
                source_block_hash: Bytes::from_static(b"a-source"),
                execution_index: 0,
            },
        ];
        let mut snap_noncanonical = mk_casper_snapshot(
            block_dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        let rejected_noncanonical = interpreter_util::validate_block_checkpoint(
            &noncanonical_effects,
            &block_store,
            &mut snap_noncanonical,
            &runtime_manager,
            None,
        )
        .await
        .expect("checkpoint (noncanonical state effects)");
        assert_eq!(
            rejected_noncanonical,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRejectedDeploy)),
        );

        let duplicate = StateEffectId {
            source_block_hash: Bytes::from_static(b"duplicate-source"),
            execution_index: 1,
        };
        let mut duplicate_effects = genesis.clone();
        duplicate_effects.body.rejected_state_effects = vec![duplicate.clone(), duplicate];
        let mut snap_duplicate = mk_casper_snapshot(
            block_dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        let rejected_duplicate = interpreter_util::validate_block_checkpoint(
            &duplicate_effects,
            &block_store,
            &mut snap_duplicate,
            &runtime_manager,
            None,
        )
        .await
        .expect("checkpoint (duplicate state effects)");
        assert_eq!(
            rejected_duplicate,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRejectedDeploy)),
        );
    })
    .await
}
