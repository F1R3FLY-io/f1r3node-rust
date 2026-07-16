// See casper/src/test/scala/coop/rchain/casper/genesis/contracts/VaultIssuanceTest.scala
//
// Verifies that genesis correctly ISSUES REV vaults with the balances from the genesis vault set.
// A standalone genesis is built whose vault set contains two known (rev-address, balance) pairs;
// a node is brought up on that genesis and, for each pair, the on-chain REV vault balance is
// queried through the `rho:vault:system` RevVault contract (`findOrCreate` + `balance`). Each
// queried balance must equal the issued `initial_balance`.
//
// The original Scala test was left disabled (rchain PR #2424, reimplementation guidance) pending a
// genesis-vault reimplementation. The Rust genesis builder issues vaults directly from the
// `Vec<Vault>` genesis set (see `casper/src/rust/genesis/contracts/vaults_generator.rs`), so the
// behavior is now directly testable and the `#[ignore]` has been removed.

use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::util::rholang::tools::Tools;
use casper::rust::util::{construct_deploy, rspace_util};
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

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
// spec proves the read needs > 3000) while staying below the 9_000_000 default deployer vault so
// that two sequential query deploys succeed even under a worst-case (no-refund) charging model.
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
        .add_block_from_deploys(&[deploy.clone()])
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

#[tokio::test]
async fn vault_issuance_test() {
    // Two known, valid rev addresses issued at genesis with distinct non-zero balances. Distinct
    // values make each per-address assertion load-bearing (a result cannot pass by symmetry).
    const REV_ADDRESS_1: &str = "1111LAd2PWaHsw84gxarNx99YVK2aZhCThhrPsWTV7cs1BPcvHftP";
    const BALANCE_1: u64 = 777_000_000;
    const REV_ADDRESS_2: &str = "1111La6tHaCtGjRiv4wkffbTAAjGyMsVhzSUNzQxH1jjZH9jtEi3M";
    const BALANCE_2: u64 = 555_000_123;

    let issued_vaults = vec![
        Vault {
            vault_address: VaultAddress::parse(REV_ADDRESS_1).expect("REV_ADDRESS_1 must be valid"),
            initial_balance: BALANCE_1,
        },
        Vault {
            vault_address: VaultAddress::parse(REV_ADDRESS_2).expect("REV_ADDRESS_2 must be valid"),
            initial_balance: BALANCE_2,
        },
    ];

    // Build genesis whose vault set includes the two issued vaults alongside the default
    // validator/deployer vaults. Passing explicit parameters (rather than `with_vaults`) makes the
    // custom vault set part of the genesis-cache key, so this test never shares a cached genesis
    // with tests that build against the default vault set.
    let (validator_key_pairs, genesis_vaults, mut genesis_params) =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    genesis_params.vaults.extend(issued_vaults.clone());

    let genesis_context = GenesisBuilder::new()
        .build_genesis_with_parameters(Some((
            validator_key_pairs,
            genesis_vaults,
            genesis_params,
        )))
        .await
        .expect("Failed to build genesis with issued vaults");

    let shard_id = genesis_context.genesis_block.shard_id.clone();
    let mut node = TestNode::standalone(genesis_context.clone())
        .await
        .expect("Failed to create standalone node");

    for vault in &issued_vaults {
        let rev_address = vault.vault_address.to_base58();
        let on_chain_balance = rev_vault_balance(&mut node, &shard_id, &rev_address).await;
        assert_eq!(
            on_chain_balance,
            vault.initial_balance.to_string(),
            "genesis must issue REV vault {} with balance {}",
            rev_address,
            vault.initial_balance
        );
    }
}
