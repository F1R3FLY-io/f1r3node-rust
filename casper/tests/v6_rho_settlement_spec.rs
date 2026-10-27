use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::casper::{CasperShardConf, PROTOCOL_V6_VERSION};
use casper::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use casper::rust::genesis::contracts::validator::Validator;
use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::rholang::costacc::genesis_resource_policy::{
    resolve_policy_genesis, shard_resource_schedule, AdoptedResourcePolicy,
};
use casper::rust::util::rholang::costacc::owner_offer::owner_direct_offer;
use casper::rust::util::rholang::costacc::supply_reader::{
    RuntimeManagerSupplyReader, SupplyReader,
};
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::gate::GateVerdict;
use casper::rust::v6::rho_settlement::RhoSettlement;
use casper::rust::v6::settlement::{EnvelopeOutcome, EnvelopeRun, Settlement};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use models::rust::casper::protocol::casper_message::{BlockMessage, ProcessedUserDeploy};
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use models::rust::record_encoding::{set_record_encoding, RecordEncoding};
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::principal_ground_v61;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const SHARD: &str = "root";
const LIMIT: u64 = 100_000_000;
const BALANCE: u128 = 1_000_000_000_000;

struct Shard {
    runtime_manager: Arc<RuntimeManager>,
    settlement: RhoSettlement,
    genesis: BlockMessage,
    validator: PublicKey,
    first: PrivateKey,
    second: PrivateKey,
}

fn vault(key: &PrivateKey) -> Vault {
    Vault {
        vault_address: VaultAddress::from_public_key(&Secp256k1.to_public(key))
            .expect("a vault address"),
        initial_balance: BALANCE,
    }
}

fn genesis(validator: PublicKey, first: &PrivateKey, second: &PrivateKey) -> Genesis {
    Genesis {
        shard_id: SHARD.to_string(),
        timestamp: 0,
        block_number: 0,
        proof_of_stake: ProofOfStake {
            minimum_bond: 1,
            maximum_bond: i64::MAX,
            validators: vec![Validator {
                pk: validator,
                stake: 100,
            }],
            epoch_length: 1000,
            quarantine_length: 50000,
            number_of_active_validators: 1,
            fault_tolerance_threshold_ppm: 0,
            max_parent_depth: 15,
            deploy_lifespan: 50,
            min_phlo_price: 1,
            pos_multi_sig_public_keys: vec![
                "04db91a53a2b72fcdcb201031772da86edad1e4979eb6742928d27731b1771e0bc40c9e9c9fa6554bdec041a87cee423d6f2e09e9dfb408b78e85a4aa611aad20c".to_string(),
                "042a736b30fffcc7d5a58bb9416f7e46180818c82b15542d0a7819d1a437aa7f4b6940c50db73a67bfc5f5ec5b5fa555d24ef8339b03edaa09c096de4ded6eae14".to_string(),
                "047f0f0f5bbe1d6d1a8dac4d88a3957851940f39a57cd89d55fe25b536ab67e6d76fd3f365c83e5bfe11fe7117e549b1ae3dd39bfc867d1c725a4177692c4e7754".to_string(),
            ],
            pos_multi_sig_quorum: 2,
        },
        vaults: vec![vault(first), vault(second)],
        supply: i64::MAX,
        version: PROTOCOL_V6_VERSION,
        native_token_name: "F1R3CAP".to_string(),
        native_token_symbol: "F1R3".to_string(),
        native_token_decimals: 8,
        fs_bundle: Vec::new(),
        consensus_fs_snapshot_cadence: None,
        protocol_caps: models::rust::cost_protocol_limits::OFFERED_FUNDED_V6_CAPS,
    }
}

async fn launch() -> Shard {
    set_record_encoding(RecordEncoding::Cost);
    let first = Secp256k1.new_key_pair().0;
    let second = Secp256k1.new_key_pair().0;
    let (_, validator) = Secp256k1.new_key_pair();
    let mut kvm = InMemoryStoreManager::new();
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let dag = BlockDagKeyValueStorage::new(&mut kvm).await.expect("dag");
    let rspace_store = kvm.r_space_stores().await.expect("rspace stores");
    let mergeable_store = RuntimeManager::mergeable_store(&mut kvm)
        .await
        .expect("mergeable store");
    let (runtime_manager, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        Genesis::default_mergeable_tags_arc(),
        ExternalServices::noop(),
    );
    let block = Genesis::create_genesis_block(
        &runtime_manager,
        &genesis(validator.clone(), &first, &second),
    )
    .await
    .expect("the genesis block is created");
    block_store
        .put_block_message(&block)
        .expect("genesis stored");
    dag.insert(&block, InsertMode::Approved)
        .expect("genesis in dag");
    let policy_genesis = resolve_policy_genesis(&block, &dag, &block_store).unwrap();
    let adopted =
        AdoptedResourcePolicy::load(&runtime_manager, &policy_genesis, &CasperShardConf {
            casper_version: PROTOCOL_V6_VERSION,
            shard_name: SHARD.to_string(),
            min_phlo_price: 1,
            ..CasperShardConf::new()
        })
        .await
        .expect("the shard adopts its policy");
    let runtime_manager = Arc::new(runtime_manager);
    Shard {
        settlement: RhoSettlement::new(runtime_manager.clone(), Arc::new(adopted)),
        runtime_manager,
        genesis: block,
        validator,
        first,
        second,
    }
}

fn offer(key: &PrivateKey, term: &str, time_stamp: i64) -> ProcessedUserDeploy {
    let body = DeployData {
        term: term.to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: SHARD.to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let proto = owner_direct_offer(key, body, LIMIT, 1, &shard_resource_schedule(SHARD, 8))
        .expect("an owner-direct offer");
    let envelope = DeployEnvelope::from_proto(proto, offered_funded_v6_limits().envelope)
        .expect("an offered envelope");
    ProcessedUserDeploy::ordered(envelope).expect("an ordered envelope")
}

fn block(shard: &Shard, deploys: Vec<ProcessedUserDeploy>) -> BlockMessage {
    let mut block = shard.genesis.clone();
    block.block_hash = Bytes::from_static(b"settled-block");
    block.header.parents_hash_list = vec![shard.genesis.block_hash.clone()];
    block.header.timestamp = 10;
    block.body.state.block_number = 1;
    block.body.deploys = deploys;
    block.sender = shard.validator.bytes.clone();
    block.seq_num = 1;
    block
}

async fn balance(shard: &Shard, key: &PrivateKey, root: &Bytes) -> i64 {
    let signature = CostSignature {
        value: Some(Value::Ground(principal_ground_v61(
            &Secp256k1.to_public(key).bytes,
        ))),
    };
    RuntimeManagerSupplyReader {
        runtime_manager: &shard.runtime_manager,
        pre_state_hash: root.clone(),
    }
    .read_purse(&signature)
    .await
    .expect("a purse read")
    .balance
    .expect("a vault balance")
}

#[tokio::test]
async fn one_offer_is_charged_its_evidence_and_replays() {
    let shard = launch().await;
    let pre_root = shard.genesis.body.state.post_state_hash.clone();
    let block = block(&shard, vec![offer(&shard.first, "@\"a\"!(1)", 1)]);
    let (runs, mid_root) = shard
        .settlement
        .run_sequentially(&block, &pre_root)
        .await
        .expect("the offer runs");
    assert_eq!(runs[0].executed.outcome, EnvelopeOutcome::Completed);
    let outcome = shard
        .settlement
        .settle(&block, &pre_root, &mid_root, &runs)
        .await
        .expect("the offer settles");
    let settled = &outcome.settled[0];
    assert!(settled.rev_spent > 0);
    assert!(settled.fee_rev > 0);
    assert!(settled.check().is_ok());
    assert!(!settled.cost_evidence.is_empty());
    let GateVerdict::Admitted { holds } = &runs[0].executed.verdict else {
        panic!("the offer is admitted");
    };
    assert!(settled.rev_spent <= holds.iter().map(|(_, hold)| hold).sum());
    assert_eq!(
        balance(&shard, &shard.first, &mid_root).await,
        BALANCE as i64
    );
    assert_eq!(
        balance(&shard, &shard.first, &outcome.post_root).await,
        (BALANCE - settled.rev_spent) as i64
    );

    let again = shard
        .settlement
        .settle(&block, &pre_root, &mid_root, &runs)
        .await
        .expect("the offer settles again");
    assert_eq!(again, outcome);
    assert_eq!(
        shard
            .settlement
            .replay(&block, &pre_root, &mid_root, &runs, &outcome)
            .await
            .expect("the settlement replays"),
        outcome.post_root
    );
    let mut forged = outcome.clone();
    forged.settled[0].rev_spent += 1;
    assert!(shard
        .settlement
        .replay(&block, &pre_root, &mid_root, &runs, &forged)
        .await
        .is_err());
}

#[tokio::test]
async fn offers_settle_in_block_order_and_a_rejected_envelope_is_not_charged() {
    let shard = launch().await;
    let pre_root = shard.genesis.body.state.post_state_hash.clone();
    let unfunded = Secp256k1.new_key_pair().0;
    let block = block(&shard, vec![
        offer(&shard.first, "@\"b\"!(2)", 2),
        offer(&unfunded, "@\"c\"!(3)", 3),
        offer(&shard.second, "@\"d\"!(4)", 4),
    ]);
    let (runs, mid_root) = shard
        .settlement
        .run_sequentially(&block, &pre_root)
        .await
        .expect("the offers run");
    assert!(!runs[1].executed.verdict.is_admitted());
    assert_eq!(runs[1].executed.outcome, EnvelopeOutcome::NotRun);
    let outcome = shard
        .settlement
        .settle(&block, &pre_root, &mid_root, &runs)
        .await
        .expect("the offers settle");
    assert!(outcome.settled[0].rev_spent > 0);
    assert_eq!(outcome.settled[1].rev_spent, 0);
    assert!(outcome.settled[1].debits.is_empty());
    assert!(outcome.settled[2].rev_spent > 0);
    assert_eq!(
        balance(&shard, &shard.first, &outcome.post_root).await,
        (BALANCE - outcome.settled[0].rev_spent) as i64
    );
    assert_eq!(
        balance(&shard, &shard.second, &outcome.post_root).await,
        (BALANCE - outcome.settled[2].rev_spent) as i64
    );
    assert_ne!(
        outcome.settled[0].debits[0].purse,
        outcome.settled[2].debits[0].purse
    );
}

#[tokio::test]
async fn a_rolled_back_offer_is_charged_from_the_restored_balance() {
    let shard = launch().await;
    let pre_root = shard.genesis.body.state.post_state_hash.clone();
    let block = block(&shard, vec![offer(&shard.first, "@\"e\"!(5)", 5)]);
    let (mut runs, _) = shard
        .settlement
        .run_sequentially(&block, &pre_root)
        .await
        .expect("the offer runs");
    runs[0].executed.outcome = EnvelopeOutcome::RolledBack;
    let outcome = shard
        .settlement
        .settle(&block, &pre_root, &pre_root, &runs)
        .await
        .expect("the rolled-back offer settles");
    assert!(outcome.settled[0].rev_spent > 0);
    assert_eq!(
        balance(&shard, &shard.first, &outcome.post_root).await,
        (BALANCE - outcome.settled[0].rev_spent) as i64
    );
}

async fn refused(
    shard: &Shard,
    block: &BlockMessage,
    pre_root: &Bytes,
    mid_root: &Bytes,
    runs: &[EnvelopeRun],
) -> String {
    shard
        .settlement
        .settle(block, pre_root, mid_root, runs)
        .await
        .expect_err("the settlement is refused")
        .to_string()
}

#[tokio::test]
async fn settlement_refuses_inputs_that_differ_from_the_gate_or_lack_a_run() {
    let shard = launch().await;
    let pre_root = shard.genesis.body.state.post_state_hash.clone();
    let block = block(&shard, vec![offer(&shard.first, "@\"f\"!(6)", 6)]);
    let (runs, mid_root) = shard
        .settlement
        .run_sequentially(&block, &pre_root)
        .await
        .expect("the offer runs");

    let mut forged = runs.clone();
    forged[0].executed.verdict = GateVerdict::Admitted { holds: vec![] };
    assert!(refused(&shard, &block, &pre_root, &mid_root, &forged)
        .await
        .contains("differs from the gate"));

    let recorded: Vec<EnvelopeRun> = runs
        .iter()
        .map(|run| EnvelopeRun::recorded(run.executed.clone()))
        .collect();
    assert!(refused(&shard, &block, &pre_root, &mid_root, &recorded)
        .await
        .contains("no local run"));

    let mut failed = runs.clone();
    failed[0].executed.outcome = EnvelopeOutcome::UserFailure;
    assert!(refused(&shard, &block, &pre_root, &mid_root, &failed)
        .await
        .contains("failure class"));

    let mut used = runs;
    used[0].executed.phlo_used += 1;
    assert!(refused(&shard, &block, &pre_root, &mid_root, &used)
        .await
        .contains("phlo usage"));
}

fn recorded(runs: &[EnvelopeRun]) -> Vec<EnvelopeRun> {
    runs.iter()
        .map(|run| EnvelopeRun::recorded(run.executed.clone()))
        .collect()
}

#[tokio::test]
async fn a_cited_record_replays_without_local_runs() {
    let shard = launch().await;
    let pre_root = shard.genesis.body.state.post_state_hash.clone();
    let unfunded = Secp256k1.new_key_pair().0;
    let block = block(&shard, vec![
        offer(&unfunded, "@\"g\"!(7)", 7),
        offer(
            &shard.first,
            "new out in { out!(8) | for (@x <- out) { @\"h\"!(x) } }",
            8,
        ),
    ]);
    let (runs, mid_root) = shard
        .settlement
        .run_sequentially(&block, &pre_root)
        .await
        .expect("the offers run");
    assert!(!runs[1].executed.user_log.is_empty());
    let outcome = shard
        .settlement
        .settle(&block, &pre_root, &mid_root, &runs)
        .await
        .expect("the offers settle");

    let cited = recorded(&runs);
    let rebuilt = shard
        .settlement
        .replay_runs(&block, &pre_root, &cited)
        .await
        .expect("the runs replay");
    assert!(rebuilt[0].run.is_none());
    let original = runs[1].run.as_ref().expect("a local run");
    let replayed = rebuilt[1].run.as_ref().expect("a replayed run");
    assert_eq!(replayed.post_root(), original.post_root());
    assert_eq!(replayed.phlo_used(), original.phlo_used());
    assert_eq!(replayed.user_log(), original.user_log());
    assert_eq!(
        shard
            .settlement
            .replay(&block, &pre_root, &mid_root, &cited, &outcome)
            .await
            .expect("the cited record replays"),
        outcome.post_root
    );
}

#[tokio::test]
async fn a_user_failure_replays_and_is_charged() {
    let shard = launch().await;
    let pre_root = shard.genesis.body.state.post_state_hash.clone();
    let block = block(&shard, vec![offer(&shard.first, "@\"i\"!(1 / 0)", 9)]);
    let (runs, mid_root) = shard
        .settlement
        .run_sequentially(&block, &pre_root)
        .await
        .expect("the offer runs");
    assert_eq!(runs[0].executed.outcome, EnvelopeOutcome::UserFailure);
    assert_eq!(mid_root, pre_root);
    let outcome = shard
        .settlement
        .settle(&block, &pre_root, &mid_root, &runs)
        .await
        .expect("the failed offer settles");
    assert!(outcome.settled[0].rev_spent > 0);
    assert_eq!(
        shard
            .settlement
            .replay(&block, &pre_root, &mid_root, &recorded(&runs), &outcome)
            .await
            .expect("the failed offer replays"),
        outcome.post_root
    );
}

#[tokio::test]
async fn a_forged_record_is_refused_on_replay() {
    let shard = launch().await;
    let pre_root = shard.genesis.body.state.post_state_hash.clone();
    let block = block(&shard, vec![offer(
        &shard.first,
        "new out in { out!(10) | for (@x <- out) { @\"j\"!(x) } }",
        10,
    )]);
    let (runs, mid_root) = shard
        .settlement
        .run_sequentially(&block, &pre_root)
        .await
        .expect("the offer runs");
    let outcome = shard
        .settlement
        .settle(&block, &pre_root, &mid_root, &runs)
        .await
        .expect("the offer settles");
    let cited = recorded(&runs);

    let mut short_log = cited.clone();
    short_log[0].executed.user_log.pop();
    let mut long_log = cited.clone();
    let extra = long_log[0].executed.user_log[0].clone();
    long_log[0].executed.user_log.push(extra);
    let mut recording = cited.clone();
    let mut bytes = recording[0].executed.budget_recording.to_vec();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    recording[0].executed.budget_recording = Bytes::from(bytes);
    let mut usage = cited.clone();
    usage[0].executed.phlo_used += 1;
    let mut failure = cited.clone();
    failure[0].executed.outcome = EnvelopeOutcome::UserFailure;
    for (forged, cause) in [
        (short_log, "omits native user events"),
        (long_log, "grant log differs"),
        (recording, "native recording wire"),
        (usage, "recorded budget differs"),
        (failure, "differs from the executed usage or failure"),
    ] {
        let error = shard
            .settlement
            .replay(&block, &pre_root, &mid_root, &forged, &outcome)
            .await
            .expect_err("a forged record is refused")
            .to_string();
        assert!(error.contains(cause), "{error}");
    }
}
