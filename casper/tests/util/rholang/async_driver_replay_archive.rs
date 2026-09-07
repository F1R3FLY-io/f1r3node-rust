use std::collections::{BTreeMap, HashMap};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use casper::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use casper::rust::genesis::contracts::validator::Validator as GenesisValidator;
use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::rholang::replay_cache::InMemoryReplayCache;
use casper::rust::util::rholang::state_hash_cache::StateHashCache;
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use models::casper::{ProcessedDeployProto, ProcessedSystemDeployProto};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::ProcessedSystemDeploy;
use models::rust::validator::Validator;
use proptest::prelude::*;
use rholang::rust::interpreter::system_processes::BlockData;
use serde_json::{json, Value};

use super::*;
use crate::util::genesis_builder::GenesisBuilder;

const ARCHIVE_EPOCH: &str = "async-driver-replay-v1-current-2026-09-07";
const CAPTURE_TEST: &str =
    "util::rholang::async_driver_differential::replay_archive::capture_current_replay_archive";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtobufPayload {
    byte_length: u64,
    blake2b256: String,
    bytes_hex: String,
}

impl ProtobufPayload {
    fn new(bytes: &[u8]) -> Self {
        Self {
            byte_length: bytes.len() as u64,
            blake2b256: digest(bytes),
            bytes_hex: hex::encode(bytes),
        }
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        let bytes = decode_hex(&self.bytes_hex)?;
        require(bytes.len() as u64 == self.byte_length, "payload length")?;
        require(digest(&bytes) == self.blake2b256, "payload digest")?;
        Ok(bytes)
    }

    fn deploy(&self) -> Result<ProcessedDeploy, String> {
        let bytes = self.bytes()?;
        let proto = ProcessedDeployProto::decode(bytes.as_slice()).map_err(|e| e.to_string())?;
        let deploy = ProcessedDeploy::from_proto(proto)?;
        require(
            deploy.clone().to_proto().encode_to_vec() == bytes,
            "deploy fixed point",
        )?;
        Ok(deploy)
    }

    fn system_deploy(&self) -> Result<ProcessedSystemDeploy, String> {
        let bytes = self.bytes()?;
        let proto =
            ProcessedSystemDeployProto::decode(bytes.as_slice()).map_err(|e| e.to_string())?;
        let deploy = ProcessedSystemDeploy::from_proto(proto)?;
        require(
            deploy.clone().to_proto().encode_to_vec() == bytes,
            "system deploy fixed point",
        )?;
        Ok(deploy)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayContext {
    time_stamp: i64,
    block_number: i64,
    sender_hex: String,
    seq_num: i32,
    system_deploys: Vec<ProtobufPayload>,
    #[serde(deserialize_with = "Option::deserialize")]
    invalid_blocks: Option<BTreeMap<String, String>>,
    is_genesis: bool,
    strict_funding_enforcement: bool,
    client_fuel_allocations: Vec<(String, i64)>,
}

impl ReplayContext {
    fn block_data(&self) -> Result<BlockData, String> {
        Ok(BlockData {
            time_stamp: self.time_stamp,
            block_number: self.block_number,
            sender: PublicKey::from_bytes(&decode_hex(&self.sender_hex)?),
            seq_num: self.seq_num,
        })
    }

    fn invalid_blocks(&self) -> Result<Option<HashMap<BlockHash, Validator>>, String> {
        self.invalid_blocks
            .as_ref()
            .map(|blocks| {
                blocks
                    .iter()
                    .map(|(hash, validator)| {
                        Ok((decode_hex(hash)?.into(), decode_hex(validator)?.into()))
                    })
                    .collect()
            })
            .transpose()
    }

    fn client_allocations(&self) -> Result<Vec<(Vec<u8>, i64)>, String> {
        self.client_fuel_allocations
            .iter()
            .map(|(key, count)| Ok((decode_hex(key)?, *count)))
            .collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayRecord {
    name: String,
    pre_root_hex: String,
    final_root_hex: String,
    context: ReplayContext,
    processed_deploy: ProtobufPayload,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    source_commit: String,
    capture_command: String,
    source_file_blake2b256: BTreeMap<String, String>,
    tracked_diff_blake2b256: String,
    local_dependency_commits: BTreeMap<String, String>,
    rustc_version: String,
    debug_assertions: bool,
    build_environment: BTreeMap<String, Option<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayArchive {
    epoch: String,
    provenance: Provenance,
    genesis: Value,
    records: Vec<ReplayRecord>,
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

fn digest(bytes: &[u8]) -> String { hex::encode(Blake2b256::hash_parts([bytes])) }

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    let bytes = hex::decode(text).map_err(|e| e.to_string())?;
    require(hex::encode(&bytes) == text, "noncanonical hex")?;
    Ok(bytes)
}

fn root(text: &str) -> Result<StateHash, String> {
    let bytes = decode_hex(text)?;
    require(
        bytes.len() == models::rust::block::state_hash::LENGTH,
        "root length",
    )?;
    Ok(bytes.into())
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/util/rholang/async_driver_replay_v1.json")
}

fn checksum_path(path: &Path) -> PathBuf { path.with_extension("blake2b256") }

fn read_archive(path: &Path) -> Result<ReplayArchive, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let seal = std::fs::read_to_string(checksum_path(path)).map_err(|e| e.to_string())?;
    decode_archive(&bytes, seal.trim())
}

fn decode_archive(bytes: &[u8], expected_digest: &str) -> Result<ReplayArchive, String> {
    require(digest(bytes) == expected_digest, "archive digest")?;
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

fn genesis_descriptor() -> Value {
    let (validator_keys, vault_keys, genesis) =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let Genesis {
        shard_id,
        timestamp,
        block_number,
        proof_of_stake,
        vaults,
        supply,
        version,
        native_token_name,
        native_token_symbol,
        native_token_decimals,
    } = genesis;
    let ProofOfStake {
        minimum_bond,
        maximum_bond,
        validators,
        epoch_length,
        quarantine_length,
        number_of_active_validators,
        pos_multi_sig_public_keys,
        pos_multi_sig_quorum,
        max_cosigners_per_deploy,
        initial_phlogiston,
        epoch_phlogiston,
    } = proof_of_stake;
    json!({
        "fixture": "GenesisBuilder::build_genesis_parameters_with_defaults(None,None)",
        "validator_public_keys": validator_keys.iter().map(|(_, key)| hex::encode(&key.bytes)).collect::<Vec<_>>(),
        "funded_vault_public_keys": vault_keys.iter().map(|(_, key)| hex::encode(&key.bytes)).collect::<Vec<_>>(),
        "shard_id": shard_id,
        "timestamp": timestamp,
        "block_number": block_number,
        "proof_of_stake": {
            "minimum_bond": minimum_bond, "maximum_bond": maximum_bond,
            "validators": validators.into_iter().map(|GenesisValidator { pk, stake }| {
                json!({ "pk": hex::encode(&pk.bytes), "stake": stake })
            }).collect::<Vec<_>>(),
            "epoch_length": epoch_length, "quarantine_length": quarantine_length,
            "number_of_active_validators": number_of_active_validators,
            "pos_multi_sig_public_keys": pos_multi_sig_public_keys,
            "pos_multi_sig_quorum": pos_multi_sig_quorum,
            "max_cosigners_per_deploy": max_cosigners_per_deploy,
            "initial_phlogiston": initial_phlogiston, "epoch_phlogiston": epoch_phlogiston,
        },
        "vaults": vaults.into_iter().map(|Vault { vault_address, initial_balance }| {
            json!({ "address": vault_address.to_base58(), "balance": initial_balance })
        }).collect::<Vec<_>>(),
        "supply": supply, "version": version,
        "native_token_name": native_token_name, "native_token_symbol": native_token_symbol,
        "native_token_decimals": native_token_decimals,
    })
}

fn validate_archive(archive: &ReplayArchive) -> Result<(), String> {
    require(archive.epoch == ARCHIVE_EPOCH, "archive epoch")?;
    require(archive.genesis == genesis_descriptor(), "genesis context")?;
    require(
        archive.provenance.source_commit.len() == 40,
        "source commit length",
    )?;
    decode_hex(&archive.provenance.source_commit)?;
    require(
        !archive.provenance.source_file_blake2b256.is_empty(),
        "source provenance",
    )?;
    require(
        archive.provenance.capture_command.contains(CAPTURE_TEST),
        "capture command",
    )?;
    let programs = corpus();
    require(
        programs.len() == 7 && archive.records.len() == programs.len(),
        "corpus floor",
    )?;
    for (record, program) in archive.records.iter().zip(programs) {
        require(record.name == program.name, "corpus order/name")?;
        root(&record.pre_root_hex)?;
        root(&record.final_root_hex)?;
        let deploy = record.processed_deploy.deploy()?;
        require(deploy.deploy.data.term == program.source, "corpus source")?;
        require(
            deploy.deploy.data.time_stamp == FIXED_TS,
            "corpus timestamp",
        )?;
        record.context.block_data()?;
        record.context.invalid_blocks()?;
        record.context.client_allocations()?;
        for deploy in &record.context.system_deploys {
            deploy.system_deploy()?;
        }
    }
    Ok(())
}

fn replay_boundary(
    runtime: &RuntimeManager,
    record: &ReplayRecord,
    actual_pre: &StateHash,
) -> Result<(), String> {
    require(runtime.replay_cache.is_none(), "replay cache enabled")?;
    require(
        runtime.state_hash_cache.is_none(),
        "state hash cache enabled",
    )?;
    require(
        root(&record.pre_root_hex)? == *actual_pre,
        "pre-root mismatch",
    )
}

async fn replay_record(
    runtime: &RuntimeManager,
    record: &ReplayRecord,
    actual_pre: &StateHash,
) -> Result<(), String> {
    replay_boundary(runtime, record, actual_pre).map_err(|error| {
        format!(
            "{error}; actual pre-root {}; archived pre-root {}",
            hex::encode(actual_pre),
            record.pre_root_hex
        )
    })?;
    let final_root = runtime
        .replay_compute_state(
            actual_pre,
            vec![record.processed_deploy.deploy()?],
            record
                .context
                .system_deploys
                .iter()
                .map(ProtobufPayload::system_deploy)
                .collect::<Result<Vec<_>, _>>()?,
            &record.context.block_data()?,
            record.context.invalid_blocks()?,
            record.context.is_genesis,
            record.context.strict_funding_enforcement,
            &record.context.client_allocations()?,
        )
        .await
        .map_err(|e| format!("runtime replay: {e:?}"))?;
    require(
        final_root == root(&record.final_root_hex)?,
        "final-root mismatch",
    )
}

fn git_output(args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(args)
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
        .output()
        .expect("capture git command failed");
    assert!(output.status.success(), "capture git command failed");
    output.stdout
}

fn provenance() -> Provenance {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let files = [
        "Cargo.toml",
        "Cargo.lock",
        ".cargo/config.toml",
        "rust-toolchain.toml",
        "casper/tests/util/rholang/async_driver_differential.rs",
        "casper/tests/util/rholang/async_driver_replay_archive.rs",
        "casper/tests/util/genesis_builder.rs",
        "casper/tests/util/rholang/resources.rs",
        "casper/src/rust/util/rholang/runtime_manager.rs",
        "casper/src/rust/rholang/replay_runtime.rs",
        "models/src/main/protobuf/CasperMessage.proto",
        "models/src/main/protobuf/RhoTypes.proto",
    ];
    let parser = "../rholang-rs-cost-accounting-transpiler";
    assert!(
        git_output(&[
            "-C",
            parser,
            "-c",
            "core.fsmonitor=false",
            "status",
            "--porcelain"
        ])
        .is_empty(),
        "capture requires the local parser dependency to be committed"
    );
    let parser_commit = String::from_utf8(git_output(&["-C", parser, "rev-parse", "HEAD"]))
        .expect("parser commit utf8")
        .trim()
        .to_owned();
    let rustc = Command::new("rustc")
        .arg("-Vv")
        .output()
        .expect("rustc version");
    assert!(rustc.status.success());
    Provenance {
        source_commit: String::from_utf8(git_output(&["rev-parse", "HEAD"]))
            .expect("commit utf8").trim().to_owned(),
        capture_command: format!("F1R3_CAPTURE_REPLAY_ARCHIVE=casper/tests/util/rholang/async_driver_replay_v1.json cargo test --locked --release -j4 -p casper --test mod {CAPTURE_TEST} -- --exact --ignored --nocapture"),
        source_file_blake2b256: files.into_iter().map(|file| {
            (file.to_owned(), digest(&std::fs::read(repo.join(file)).expect(file)))
        }).collect(),
        tracked_diff_blake2b256: digest(&git_output(&["diff", "--binary", "HEAD"])),
        local_dependency_commits: BTreeMap::from([(parser.to_owned(), parser_commit)]),
        rustc_version: String::from_utf8(rustc.stdout).expect("rustc version utf8"),
        debug_assertions: cfg!(debug_assertions),
        build_environment: ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_JOBS", "RUST_BACKTRACE"]
            .into_iter().map(|key| (key.to_owned(), std::env::var(key).ok())).collect(),
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit current-epoch capture; requires a new F1R3_CAPTURE_REPLAY_ARCHIVE path"]
async fn capture_current_replay_archive() {
    let destination = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(
            std::env::var_os("F1R3_CAPTURE_REPLAY_ARCHIVE")
                .expect("explicit archive destination required"),
        );
    assert!(
        destination.parent().is_some_and(Path::is_dir),
        "archive parent must already exist"
    );
    assert!(
        !destination.exists() && !checksum_path(&destination).exists(),
        "capture never replaces an archive"
    );
    let provenance = provenance();
    let genesis = genesis_descriptor();
    let records = with_runtime_manager(|mut runtime, context, genesis_block| async move {
        runtime.replay_cache = None;
        runtime.state_hash_cache = None;
        let pre = genesis_block.body.state.post_state_hash;
        let mut records = Vec::new();
        for program in corpus() {
            eprintln!("capture {}: play", program.name);
            let deploy = construct_deploy::source_deploy(
                program.source.to_owned(),
                FIXED_TS,
                None,
                None,
                None,
                None,
                None,
            )
            .expect("capture deploy");
            let (post, processed) = compute_state(&mut runtime, &context, deploy, &pre).await;
            let record = ReplayRecord {
                name: program.name.to_owned(),
                pre_root_hex: hex::encode(&pre),
                final_root_hex: hex::encode(&post),
                context: ReplayContext {
                    time_stamp: FIXED_TS,
                    block_number: 0,
                    sender_hex: hex::encode(&context.validator_pks()[0].bytes),
                    seq_num: 0,
                    system_deploys: Vec::new(),
                    invalid_blocks: None,
                    is_genesis: false,
                    strict_funding_enforcement: false,
                    client_fuel_allocations: Vec::new(),
                },
                processed_deploy: ProtobufPayload::new(&processed.to_proto().encode_to_vec()),
            };
            eprintln!("capture {}: uncached replay", program.name);
            replay_record(&runtime, &record, &pre)
                .await
                .expect("capture self-replay");
            records.push(record);
        }
        records
    })
    .await
    .expect("capture runtime");
    let archive = ReplayArchive {
        epoch: ARCHIVE_EPOCH.to_owned(),
        provenance,
        genesis,
        records,
    };
    validate_archive(&archive).expect("capture admission");
    let bytes = serde_json::to_vec_pretty(&archive).expect("archive serialization");
    write_new(&destination, &bytes).expect("new archive publication");
    write_new(
        &checksum_path(&destination),
        format!("{}\n", digest(&bytes)).as_bytes(),
    )
    .expect("new archive checksum publication");
    println!(
        "captured {} records, {} bytes, BLAKE2b-256 {}",
        archive.records.len(),
        bytes.len(),
        digest(&bytes)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn archived_deploys_replay_without_play_or_caches() {
    let archive = read_archive(&fixture_path()).expect("required sealed archive");
    validate_archive(&archive).expect("archive admission");
    with_runtime_manager(|mut runtime, _, genesis| async move {
        runtime.replay_cache = None;
        runtime.state_hash_cache = None;
        for record in &archive.records {
            eprintln!("verify {}: uncached replay", record.name);
            replay_record(&runtime, record, &genesis.body.state.post_state_hash)
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", record.name));
        }
    })
    .await
    .expect("archive verification runtime");
}

#[test]
fn replay_archive_requires_present_unchanged_files() {
    let directory = tempfile::tempdir().unwrap();
    let absent = directory.path().join("absent.json");
    assert!(read_archive(&absent).is_err());
    assert!(!absent.exists() && !checksum_path(&absent).exists());
    write_new(&absent, b"first").unwrap();
    assert!(write_new(&absent, b"second").is_err());
    assert_eq!(std::fs::read(&absent).unwrap(), b"first");
    assert!(read_archive(&absent).is_err());
    let bytes = std::fs::read(fixture_path()).unwrap();
    let seal = digest(&bytes);
    assert!(decode_archive(b"{", &digest(b"{")).is_err());
    let mut value: Value = serde_json::from_slice(&bytes).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("unexpected".to_owned(), json!(1));
    let changed = serde_json::to_vec(&value).unwrap();
    assert!(decode_archive(&changed, &digest(&changed)).is_err());
    assert!(decode_archive(&changed, &seal).is_err());
}

#[test]
fn replay_archive_context_and_corpus_are_complete() {
    let archive = read_archive(&fixture_path()).unwrap();
    validate_archive(&archive).unwrap();
    let mut changed = archive.clone();
    changed.epoch.push('x');
    assert_eq!(validate_archive(&changed).unwrap_err(), "archive epoch");
    let mut changed = archive.clone();
    changed.genesis["timestamp"] = json!(1);
    assert_eq!(validate_archive(&changed).unwrap_err(), "genesis context");
    let mut changed = archive.clone();
    changed.records.pop();
    assert_eq!(validate_archive(&changed).unwrap_err(), "corpus floor");
    let mut changed = archive;
    changed.records.swap(0, 1);
    assert_eq!(validate_archive(&changed).unwrap_err(), "corpus order/name");
    let bytes = std::fs::read(fixture_path()).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    for field in value["records"][0]["context"].as_object().unwrap().keys() {
        let mut changed = value.clone();
        changed["records"][0]["context"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            serde_json::from_value::<ReplayArchive>(changed).is_err(),
            "missing {field}"
        );
    }
}

#[test]
fn replay_archive_payloads_preserve_all_fields_and_order() {
    let archive = read_archive(&fixture_path()).unwrap();
    for record in archive.records {
        let payload = &record.processed_deploy;
        let bytes = payload.bytes().unwrap();
        let deploy = payload.deploy().unwrap();
        assert!(!deploy.deploy_log.is_empty());
        assert_eq!(deploy.to_proto().encode_to_vec(), bytes);
        let mut malformed = bytes.clone();
        malformed.push(0x80);
        assert!(ProtobufPayload::new(&malformed).deploy().is_err());
        let mut unknown = bytes.clone();
        unknown.extend_from_slice(&[0xf8, 0x7f, 1]);
        assert_eq!(
            ProtobufPayload::new(&unknown).deploy().unwrap_err(),
            "deploy fixed point"
        );
        let mut changed = payload.clone();
        changed.byte_length += 1;
        assert_eq!(changed.deploy().unwrap_err(), "payload length");
        let mut changed = payload.clone();
        changed.blake2b256 = digest(b"changed");
        assert_eq!(changed.deploy().unwrap_err(), "payload digest");
    }
}

#[test]
fn replay_archive_event_edits_cannot_reuse_original_seal() {
    let original = std::fs::read(fixture_path()).unwrap();
    let archive = read_archive(&fixture_path()).unwrap();
    let seal = digest(&original);
    for index in 0..archive.records.len() {
        let deploy = archive.records[index].processed_deploy.deploy().unwrap();
        let different = deploy
            .deploy_log
            .iter()
            .position(|event| *event != deploy.deploy_log[0])
            .unwrap();
        for mutation in 0..3 {
            let mut changed_deploy = deploy.clone();
            match mutation {
                0 => {
                    changed_deploy.deploy_log.remove(0);
                }
                1 => {
                    changed_deploy.deploy_log.push(deploy.deploy_log[0].clone());
                }
                _ => changed_deploy.deploy_log.swap(0, different),
            }
            let changed_payload = changed_deploy.to_proto().encode_to_vec();
            assert_ne!(
                changed_payload,
                archive.records[index].processed_deploy.bytes().unwrap()
            );
            let mut changed = archive.clone();
            changed.records[index].processed_deploy = ProtobufPayload::new(&changed_payload);
            let changed_bytes = serde_json::to_vec_pretty(&changed).unwrap();
            assert_eq!(
                decode_archive(&changed_bytes, &seal).unwrap_err(),
                "archive digest"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn replay_archive_byte_mutations_are_rejected(index in any::<usize>(), bit in 0u8..8) {
        let bytes = std::fs::read(fixture_path()).unwrap();
        let seal = digest(&bytes);
        let position = index % bytes.len();
        let mut changed = bytes.clone();
        changed[position] ^= 1 << bit;
        prop_assert!(decode_archive(&changed, &seal).is_err());
        let mut changed = bytes.clone();
        changed.remove(position);
        prop_assert!(decode_archive(&changed, &seal).is_err());
        let mut changed = bytes.clone();
        changed.insert(position, bytes[position]);
        prop_assert!(decode_archive(&changed, &seal).is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replay_archive_rejects_cache_shortcuts_and_wrong_roots() {
    let archive = read_archive(&fixture_path()).unwrap();
    with_runtime_manager(|mut runtime, _, genesis| async move {
        let pre = genesis.body.state.post_state_hash;
        let record = &archive.records[0];
        runtime.state_hash_cache = None;
        runtime.replay_cache = Some(Arc::new(InMemoryReplayCache::new(1)));
        assert_eq!(
            replay_boundary(&runtime, record, &pre).unwrap_err(),
            "replay cache enabled"
        );
        runtime.replay_cache = None;
        runtime.state_hash_cache = Some(Arc::new(StateHashCache::new(1)));
        assert_eq!(
            replay_boundary(&runtime, record, &pre).unwrap_err(),
            "state hash cache enabled"
        );
        runtime.state_hash_cache = None;
        let wrong: StateHash = vec![0; 32].into();
        assert_ne!(pre, wrong);
        assert_eq!(
            replay_boundary(&runtime, record, &wrong).unwrap_err(),
            "pre-root mismatch"
        );
        let mut changed = record.clone();
        changed.final_root_hex = hex::encode(&wrong);
        assert_ne!(changed.final_root_hex, record.final_root_hex);
        assert_eq!(
            replay_record(&runtime, &changed, &pre).await.unwrap_err(),
            "final-root mismatch"
        );
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replay_archive_runtime_rejects_cost_status_and_comm_mutations() {
    let archive = read_archive(&fixture_path()).unwrap();
    with_runtime_manager(|mut runtime, _, genesis| async move {
        runtime.replay_cache = None;
        runtime.state_hash_cache = None;
        let pre = genesis.body.state.post_state_hash;
        let record = archive
            .records
            .iter()
            .find(|record| record.name == "join_two")
            .unwrap();
        let original = record.processed_deploy.deploy().unwrap();
        replay_record(&runtime, record, &pre)
            .await
            .expect("untouched control replay");
        let mut mutations = Vec::new();
        let mut cost = original.clone();
        cost.cost.cost += 1;
        mutations.push(("cost", cost));
        let mut status = original.clone();
        status.is_failed = !status.is_failed;
        mutations.push(("status", status));
        let mut identity = original.clone();
        let event = identity
            .deploy_log
            .iter_mut()
            .find_map(|event| match event {
                Event::Comm(comm) => Some(comm),
                _ => None,
            })
            .unwrap();
        event.consume.hash = vec![0x5a; 32].into();
        mutations.push(("COMM identity", identity));
        let mut extra = original.clone();
        let event = extra
            .deploy_log
            .iter()
            .find(|event| matches!(event, Event::Comm(_)))
            .unwrap()
            .clone();
        extra.deploy_log.push(event);
        mutations.push(("COMM multiplicity", extra));
        for (label, deploy) in mutations {
            let mut changed = record.clone();
            changed.processed_deploy = ProtobufPayload::new(&deploy.to_proto().encode_to_vec());
            let error = replay_record(&runtime, &changed, &pre)
                .await
                .expect_err(label);
            let expected = match label {
                "cost" => error.starts_with("runtime replay: ReplayFailure(ReplayCostMismatch"),
                "status" => error.starts_with("runtime replay: ReplayFailure(ReplayStatusMismatch"),
                _ => {
                    error.starts_with("runtime replay: ReplayFailure(UnusedCOMMEvent")
                        || error.starts_with("runtime replay: ReplayFailure(ReplayCostMismatch")
                }
            };
            assert!(expected, "{label}: {error}");
        }
    })
    .await
    .unwrap();
}
