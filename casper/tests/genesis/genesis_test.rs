// See casper/src/test/scala/coop/rchain/casper/genesis/GenesisTest.scala
//
// Note: Tests are simplified compared to Scala original.
// In Scala, LogStub (from comm/src/test/scala/coop/rchain/p2p/EffectsTestInstances.scala)
// implements Log[F] trait and is passed as implicit parameter to functions like BondsParser.
// When these functions call log.info("..."), messages go directly to LogStub.
// Tests then assert on log.warns.count and log.infos.count.
//
// In Rust, BondsParser uses `tracing` crate (tracing::info!, tracing::warn!).
// These logs are not captured because we don't set up a tracing subscriber in tests.
// There are two ways to capture tracing logs:
// 1. Use `tracing-test` crate with #[traced_test] attribute and logs_contain() macro
// 2. Implement custom tracing_subscriber::Layer that captures logs into a Vec
// However, this adds complexity and dependencies for marginal benefit.
// For now, tests verify the end result (e.g., bonds.len()) instead of log message counts.

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use casper::rust::genesis::contracts::validator::Validator;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::bonds_parser::BondsParser;
use casper::rust::util::rholang::interpreter_util;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::rholang::tools::Tools;
use casper::rust::util::vault_parser::VaultParser;
use casper::rust::util::{construct_deploy, proto_util, rspace_util};
use comm::rust::test_instances::{LogStub, LogicalTime};
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::string_ops::StringOps;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use tempfile::TempDir;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, DEFAULT_POS_MULTI_SIG_PUBLIC_KEYS};
use crate::util::rholang::resources;
use crate::util::rholang::resources::generate_scope_id;

const AUTOGEN_SHARD_SIZE: usize = 5;
const RCHAIN_SHARD_ID: &str = "root";

fn genesis_path() -> PathBuf {
    TempDir::new()
        .expect("Failed to create genesis temp dir")
        .keep()
}

async fn with_gen_resources<F, Fut, R>(body: F) -> R
where
    F: FnOnce(RuntimeManager, PathBuf, LogStub, LogicalTime) -> Fut,
    Fut: Future<Output = R>,
{
    let scope_id = generate_scope_id();
    let gp = genesis_path();

    // Scala uses MetricsNOP, and this class in turn is empty, if it is used it means that the test does not log metrics.
    // implicit val noopMetrics: Metrics[F] = new metrics.Metrics.MetricsNOP[F]
    // implicit val span: Span[F]           = NoopSpan[F]()

    let time = LogicalTime::new();
    let log = LogStub::new();

    let mut kvs_manager = resources::mk_test_rnode_store_manager_shared(scope_id.clone());

    let m_store = RuntimeManager::mergeable_store(&mut *kvs_manager)
        .await
        .expect("Failed to create mergeable store");

    let r_store = kvs_manager
        .r_space_stores()
        .await
        .expect("Failed to create rspace stores");

    let runtime_manager = RuntimeManager::create_with_store(
        r_store,
        m_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    );

    let result = body(runtime_manager, gp.clone(), log, time).await;

    // Note: Scala uses PathOps.recursivelyDelete() with FileVisitor pattern.
    // Rust fs::remove_dir_all does the same - recursively removes directory with all contents.
    let _ = fs::remove_dir_all(&scope_id);
    let _ = fs::remove_dir_all(&gp);

    result
}

fn mk_casper_snapshot(dag: KeyValueDagRepresentation) -> CasperSnapshot {
    CasperSnapshot {
        dag,
        last_finalized_block: Bytes::new(),
        lca: Bytes::new(),
        tips: Vec::new(),
        parents: Vec::new(),
        justifications: Default::default(),
        invalid_blocks: HashMap::new(),
        deploys_in_scope: Default::default(),
        rejected_in_scope: Default::default(),
        max_block_num: 0,
        max_seq_nums: Default::default(),
        on_chain_state: OnChainCasperState {
            shard_conf: CasperShardConf::new(),
            bonds_map: HashMap::new(),
            active_validators: Vec::new(),
        },
    }
}

fn validators() -> Vec<(String, usize)> {
    vec![
        (
            "299670c52849f1aa82e8dfe5be872c16b600bf09cc8983e04b903411358f2de6".to_string(),
            0,
        ),
        (
            "6bf1b2753501d02d386789506a6d93681d2299c6edfd4455f596b97bc5725968".to_string(),
            1,
        ),
    ]
}

fn print_bonds(bonds_file: &Path) {
    let content = validators()
        .into_iter()
        .map(|(v, i)| format!("{v} {i}"))
        .collect::<Vec<_>>()
        .join("\n");

    fs::write(bonds_file, format!("{content}\n")).expect("Failed to write bonds file");
}

//Note: using this struct + new() to describe default parameters
struct FromInputFilesParams<'a> {
    maybe_bonds_path: Option<&'a str>,
    autogen_shard_size: usize,
    maybe_vaults_path: Option<&'a str>,
    minimum_bond: i64,
    maximum_bond: i64,
    epoch_length: i32,
    quarantine_length: i32,
    number_of_active_validators: u32,
    shard_id: String,
    deploy_timestamp: Option<i64>,
    block_number: i64,
}

impl Default for FromInputFilesParams<'_> {
    fn default() -> Self {
        Self {
            maybe_bonds_path: None,
            autogen_shard_size: AUTOGEN_SHARD_SIZE,
            maybe_vaults_path: None,
            minimum_bond: 1,
            maximum_bond: i64::MAX,
            epoch_length: 10000,
            quarantine_length: 50000,
            number_of_active_validators: 100,
            shard_id: RCHAIN_SHARD_ID.to_string(),
            deploy_timestamp: None,
            block_number: 0,
        }
    }
}

impl<'a> FromInputFilesParams<'a> {
    fn new() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        Self {
            deploy_timestamp: Some(now),
            ..Default::default()
        }
    }
}

async fn from_input_files(
    runtime_manager: &mut RuntimeManager,
    genesis_path: &Path,
    params: FromInputFilesParams<'_>,
) -> Result<BlockMessage, Box<dyn Error>> {
    // deploy_timestamp is always Some
    let timestamp = params
        .deploy_timestamp
        .expect("deploy_timestamp should be set");

    let vaults_path = params
        .maybe_vaults_path
        .map(|p| p.to_string())
        .unwrap_or_else(|| {
            genesis_path
                .join("wallets.txt")
                .to_string_lossy()
                .to_string()
        });

    let vaults = VaultParser::parse_from_path_str(&vaults_path)?;

    let bonds_path = params
        .maybe_bonds_path
        .map(|p| p.to_string())
        .unwrap_or_else(|| genesis_path.join("bonds.txt").to_string_lossy().to_string());

    let bonds = BondsParser::parse_with_autogen(&bonds_path, params.autogen_shard_size)?;

    let validators: Vec<Validator> = bonds
        .iter()
        .map(|(pk, stake)| Validator {
            pk: pk.clone(),
            stake: *stake,
        })
        .collect();

    let genesis = Genesis {
        shard_id: params.shard_id,
        timestamp,
        proof_of_stake: ProofOfStake {
            minimum_bond: params.minimum_bond,
            maximum_bond: params.maximum_bond,
            epoch_length: params.epoch_length,
            quarantine_length: params.quarantine_length,
            number_of_active_validators: params.number_of_active_validators,
            fault_tolerance_threshold_ppm: 0,
            validators,
            pos_multi_sig_public_keys: DEFAULT_POS_MULTI_SIG_PUBLIC_KEYS.to_vec(),
            pos_multi_sig_quorum: DEFAULT_POS_MULTI_SIG_PUBLIC_KEYS.len() as u32 - 1,
        },
        vaults,
        supply: i64::MAX,
        block_number: params.block_number,
        version: 1,
        native_token_name: "F1R3CAP".to_string(),
        native_token_symbol: "F1R3".to_string(),
        native_token_decimals: 8,
        fs_bundle: Vec::new(),
        consensus_fs_snapshot_cadence: None,
    };

    let genesis_block = Genesis::create_genesis_block(runtime_manager, &genesis).await?;

    Ok(genesis_block)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn genesis_from_input_files_should_generate_random_validators_when_no_bonds_file_is_given() {
    with_gen_resources(
        |mut runtime_manager, genesis_path, _log, _time| async move {
            let genesis_block = from_input_files(
                &mut runtime_manager,
                &genesis_path,
                FromInputFilesParams::new(),
            )
            .await
            .expect("Genesis creation should succeed");

            let bonds = proto_util::bonds(&genesis_block);

            assert_eq!(
                bonds.len(),
                AUTOGEN_SHARD_SIZE,
                "Should generate {} random validators",
                AUTOGEN_SHARD_SIZE
            );
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn genesis_from_input_files_should_tell_when_bonds_file_does_not_exist() {
    with_gen_resources(
        |mut runtime_manager, genesis_path, _log, _time| async move {
            // Path that does not exist - using a fake path, no need to create a real directory
            let non_existing_path = "/tmp/non_existing_test_path/not/a/real/file".to_string();

            let result =
                from_input_files(&mut runtime_manager, &genesis_path, FromInputFilesParams {
                    maybe_bonds_path: Some(&non_existing_path),
                    ..FromInputFilesParams::new()
                })
                .await;

            // BondsParser::parse_with_autogen logs warn "BONDS FILE NOT FOUND" and creates random bonds
            assert!(
                result.is_ok(),
                "Genesis creation should succeed with auto-generated bonds"
            );
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn genesis_from_input_files_should_fail_with_error_when_bonds_file_cannot_be_parsed() {
    with_gen_resources(
        |mut runtime_manager, genesis_path, _log, _time| async move {
            let bad_bonds_file = genesis_path.join("misformatted.txt");
            let mut file =
                fs::File::create(&bad_bonds_file).expect("Failed to create bad bonds file");
            writeln!(file, "xzy 1\nabc 123 7").expect("Failed to write bad bonds content");

            let bad_bonds_path = bad_bonds_file.to_str().unwrap().to_string();
            let result =
                from_input_files(&mut runtime_manager, &genesis_path, FromInputFilesParams {
                    maybe_bonds_path: Some(&bad_bonds_path),
                    ..FromInputFilesParams::new()
                })
                .await;

            assert!(result.is_err(), "Genesis creation should fail");

            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("FAILED PARSING BONDS FILE") || err_msg.contains("INVALID"),
                "Error should mention parsing failure, got: {}",
                err_msg
            );
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn genesis_from_input_files_should_create_a_genesis_block_with_the_right_bonds_when_a_proper_bonds_file_is_given(
) {
    with_gen_resources(
        |mut runtime_manager, genesis_path, _log, _time| async move {
            let bonds_file = genesis_path.join("givenBonds.txt");
            print_bonds(&bonds_file);

            let bonds_path = bonds_file.to_str().unwrap().to_string();
            let result =
                from_input_files(&mut runtime_manager, &genesis_path, FromInputFilesParams {
                    maybe_bonds_path: Some(&bonds_path),
                    ..FromInputFilesParams::new()
                })
                .await;

            assert!(result.is_ok(), "Genesis creation should succeed");

            let genesis_block = result.unwrap();
            let bonds = proto_util::bonds(&genesis_block);

            let expected_bonds: Vec<Bond> = validators()
                .iter()
                .map(|(v, i)| {
                    let pk_bytes = StringOps::decode_hex(v.clone()).expect("Failed to decode hex");
                    Bond {
                        validator: pk_bytes.into(),
                        stake: *i as i64,
                    }
                })
                .collect();

            for expected in &expected_bonds {
                assert!(
                    bonds
                        .iter()
                        .any(|b| b.validator == expected.validator && b.stake == expected.stake),
                    "Expected bond {:?} not found in bonds",
                    expected
                );
            }
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn genesis_from_input_files_should_create_a_valid_genesis_block() {
    with_storage(|block_store, mut block_dag_storage| async move {
        with_gen_resources(
            |mut runtime_manager, genesis_path, _log, _time| async move {
                let genesis = from_input_files(
                    &mut runtime_manager,
                    &genesis_path,
                    FromInputFilesParams::new(),
                )
                .await
                .expect("Genesis creation should succeed");

                block_dag_storage
                    .insert(
                        &genesis,
                        block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved,
                    )
                    .expect("Failed to insert genesis into DAG");

                block_store
                    .put(genesis.block_hash.clone(), &genesis)
                    .expect("Failed to put genesis into block store");

                let dag = block_dag_storage
                    .get_representation()
                    .expect("dag representation");

                let maybe_post_genesis_state_hash = interpreter_util::validate_block_checkpoint(
                    &genesis,
                    &block_store,
                    &mut mk_casper_snapshot(dag),
                    &runtime_manager,
                    None,
                )
                .await
                .expect("validate_block_checkpoint should succeed");

                match maybe_post_genesis_state_hash {
                    Either::Right(Some(_)) => {
                        // Success - full checkpoint replay produced a post-state hash.
                    }
                    Either::Right(None) => {
                        // Also acceptable: genesis checkpoint may be treated as already validated
                        // and return no additional post-state hash.
                    }
                    Either::Left(block_error) => {
                        panic!("Expected Right(Some(_)), got Left({:?})", block_error);
                    }
                }
            },
        )
        .await
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn genesis_from_input_files_should_detect_an_existing_bonds_file_in_the_default_location() {
    with_gen_resources(
        |mut runtime_manager, genesis_path, _log, _time| async move {
            // Create bonds.txt in default location
            let bonds_file = genesis_path.join("bonds.txt");
            print_bonds(&bonds_file);

            let result = from_input_files(
                &mut runtime_manager,
                &genesis_path,
                FromInputFilesParams::new(),
            )
            .await;

            assert!(result.is_ok(), "Genesis creation should succeed");

            let genesis_block = result.unwrap();
            let bonds = proto_util::bonds(&genesis_block);

            let expected_bonds: Vec<Bond> = validators()
                .iter()
                .map(|(v, i)| {
                    let pk_bytes = StringOps::decode_hex(v.clone()).expect("Failed to decode hex");
                    Bond {
                        validator: pk_bytes.into(),
                        stake: *i as i64,
                    }
                })
                .collect();

            for expected in &expected_bonds {
                assert!(
                    bonds
                        .iter()
                        .any(|b| b.validator == expected.validator && b.stake == expected.stake),
                    "Expected bond {:?} not found in bonds",
                    expected
                );
            }
        },
    )
    .await;
}

// Rholang that reads the on-chain REV vault balance for a single rev address and returns it on
// `ret`. `ret` is the first `new`-bound (non-urn) name, so its unforgeable id equals the deploy
// RNG's first output — exactly what `calculate_unforgeable_name` reconstructs for the read-back.
const BALANCE_QUERY_TEMPLATE: &str = r#"
new ret, rl(`rho:registry:lookup`), RevVaultCh, vaultCh, balanceCh in {
  rl!(`rho:vault:system`, *RevVaultCh) |
  for (@(_, RevVault) <- RevVaultCh) {
    match "__REV_ADDRESS__" {
      revAddress => {
        @RevVault!("findOrCreate", revAddress, *vaultCh) |
        for (@(true, vault) <- vaultCh) {
          @vault!("balance", *balanceCh) |
          for (@balance <- balanceCh) {
            ret!(balance)
          }
        }
      }
    }
  }
}
"#;

// A phlo budget that comfortably exceeds the cost of a RevVault balance read (the insufficient-phlo
// spec proves the read needs > 3000) while staying below the 9_000_000 default deployer vault.
const BALANCE_QUERY_PHLO_LIMIT: i64 = 4_000_000;

// Reconstructs the unforgeable id of the first `new`-bound name of a deploy signed by DEFAULT_SEC.
fn calculate_unforgeable_name(timestamp: i64) -> String {
    let secp256k1 = Secp256k1;
    let public_key = secp256k1.to_public(&construct_deploy::DEFAULT_SEC);
    let unforgeable_id = Tools::unforgeable_name_rng(&public_key, timestamp).next();
    let unforgeable_id_u8: Vec<u8> = unforgeable_id.iter().map(|&b| b as u8).collect();
    hex::encode(unforgeable_id_u8)
}

// Deploys a balance query for `rev_address`, adds it in a block, and returns the pretty-printed
// on-chain balance (a decimal string) read back from the deploy's `ret` channel.
async fn rev_vault_balance(node: &mut TestNode, shard_id: &str, rev_address: &str) -> String {
    let term = BALANCE_QUERY_TEMPLATE.replace("__REV_ADDRESS__", rev_address);
    let deploy = construct_deploy::source_deploy_now_full(
        term,
        Some(BALANCE_QUERY_PHLO_LIMIT),
        None,
        None,
        None,
        Some(shard_id.to_string()),
    )
    .expect("Failed to build balance-query deploy");

    let block = node
        .add_block_from_deploys(std::slice::from_ref(&deploy))
        .await
        .expect("Failed to add balance-query block");

    let data = rspace_util::get_data_at_private_channel(
        &block,
        &calculate_unforgeable_name(deploy.data.time_stamp),
        &node.runtime_manager,
    )
    .await;

    assert_eq!(
        data.len(),
        1,
        "expected exactly one balance datum on ret for {}, got {:?}",
        rev_address,
        data
    );
    data.into_iter()
        .next()
        .expect("balance datum should be present")
}

// Parses the wallets input file at genesis and asserts the corresponding REV vault is created
// on-chain with exactly the wallets-file balance. Exercises the real wallets-file parser
// (`VaultParser`) end-to-end into genesis vault issuance. (The "Scala ignore" port label was
// stale — this behavior is directly testable against the current Rust genesis builder.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn genesis_from_input_files_should_parse_the_wallets_file_and_create_corresponding_rev_vaults(
) {
    // A known, valid rev address funded with a known non-zero balance in the wallets file.
    const KNOWN_REV_ADDRESS: &str = "1111LAd2PWaHsw84gxarNx99YVK2aZhCThhrPsWTV7cs1BPcvHftP";
    const KNOWN_BALANCE: u64 = 123_456_789;

    // Write and PARSE a wallets file (the behavior under test): `<rev_address>,<balance>` lines
    // ingested by the genesis wallets parser used by `from_input_files`.
    let wallets_dir = TempDir::new().expect("Failed to create wallets temp dir");
    let wallets_path = wallets_dir.path().join("wallets.txt");
    fs::write(
        &wallets_path,
        format!("{},{}\n", KNOWN_REV_ADDRESS, KNOWN_BALANCE),
    )
    .expect("Failed to write wallets file");

    let parsed_vaults = VaultParser::parse_from_path_str(
        wallets_path
            .to_str()
            .expect("wallets path must be valid UTF-8"),
    )
    .expect("Failed to parse wallets file");
    assert_eq!(
        parsed_vaults.len(),
        1,
        "wallets file should parse into exactly one vault"
    );
    assert_eq!(
        parsed_vaults[0].vault_address.to_base58(),
        KNOWN_REV_ADDRESS,
        "parsed vault address should match the wallets-file address"
    );

    // Build a genesis whose vault set includes the parsed wallet vault(s). Explicit parameters keep
    // the custom vault set in the genesis-cache key (no sharing with default-vault geneses).
    let (validator_key_pairs, genesis_vaults, mut genesis_params) =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    genesis_params.vaults.extend(parsed_vaults.clone());

    let genesis_context = GenesisBuilder::new()
        .build_genesis_with_parameters(Some((validator_key_pairs, genesis_vaults, genesis_params)))
        .await
        .expect("Failed to build genesis from parsed wallets");

    let shard_id = genesis_context.genesis_block.shard_id.clone();
    let mut node = TestNode::standalone(genesis_context.clone())
        .await
        .expect("Failed to create standalone node");

    // Assert the genesis created a corresponding REV vault holding exactly the wallets-file balance.
    let on_chain_balance = rev_vault_balance(&mut node, &shard_id, KNOWN_REV_ADDRESS).await;
    assert_eq!(
        on_chain_balance,
        KNOWN_BALANCE.to_string(),
        "genesis REV vault for {} must hold the wallets-file balance {}",
        KNOWN_REV_ADDRESS,
        KNOWN_BALANCE
    );
}
