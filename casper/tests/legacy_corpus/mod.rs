use std::path::PathBuf;

use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::casper::BlockMessageProto;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use prost::Message;
use rholang::rust::interpreter::system_processes::BlockData;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

const VAULT_BALANCE: &str = r#"
new deployerId(`rho:system:deployerId`),
    rl(`rho:registry:lookup`),
    vaultAddressOps(`rho:vault:address`),
    vaultAddressCh,
    vaultCh in {
  rl!(`rho:vault:system`, *vaultCh) |
  vaultAddressOps!("fromDeployerId", *deployerId, *vaultAddressCh) |
  for (@userVaultAddress <- vaultAddressCh & @(_, systemVault) <- vaultCh) {
    new userVaultCh in {
      @systemVault!("findOrCreate", userVaultAddress, *userVaultCh) |
      for (@(true, userVault) <- userVaultCh) {
        @userVault!("balance", "IGNORE")
      }
    }
  }
}
"#;

const COMM_CHAIN: &str = r#"
new a, b, c in {
  a!(1) |
  for (@x <- a) { b!(x + 1) } |
  for (@y <- b) { c!(y * 3) } |
  for (@z <- c) { @"corpus-out"!([z, "done"]) }
}
"#;

const METHOD_ERROR: &str = "@0!(0) | for(@x <- @0){ x.undefined() }";

const OPERATOR_ERROR: &str =
    r#"for(@x <- @"x" & @y <- @"y"){ @"xy"!(x + y) } | @"x"!(1) | @"y"!("hi")"#;

const BLOCKS: [&str; 5] = ["genesis", "b0", "b1", "b2", "b3"];

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/test/resources/legacy_corpus")
        .join(format!("{name}.pb"))
}

fn deploy(
    source: &str,
    timestamp: i64,
    key: Option<PrivateKey>,
    shard: &str,
) -> Signed<DeployData> {
    construct_deploy::source_deploy(
        source.to_string(),
        timestamp,
        Some(500_000),
        Some(1),
        key,
        None,
        Some(shard.to_string()),
    )
    .expect("corpus deploy")
}

fn fixed_validators() -> Vec<(PrivateKey, PublicKey)> {
    (1..=2u8)
        .map(|seed| {
            let sk = PrivateKey::from_bytes(&[seed; 32]);
            let pk = Secp256k1.to_public(&sk);
            (sk, pk)
        })
        .collect()
}

#[tokio::test]
#[ignore = "records the corpus on the reference tree"]
async fn record_legacy_corpus() {
    let validators = fixed_validators();
    let mut parameters = GenesisBuilder::build_genesis_parameters(
        validators.clone(),
        &GenesisBuilder::create_bonds(validators.iter().map(|(_, pk)| pk.clone()).collect()),
    );
    parameters
        .2
        .proof_of_stake
        .validators
        .sort_by(|a, b| a.pk.bytes.cmp(&b.pk.bytes));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let shard = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("network");

    let sec2 = Some(construct_deploy::DEFAULT_SEC2.clone());
    let b0 = nodes[0]
        .add_block_from_deploys(&[
            deploy("@10!(1)", 1_000, None, &shard),
            deploy(VAULT_BALANCE, 1_001, None, &shard),
        ])
        .await
        .expect("b0");
    let b1 = nodes[1]
        .add_block_from_deploys(&[
            deploy("@20!(2)", 1_002, sec2.clone(), &shard),
            deploy(COMM_CHAIN, 1_003, sec2, &shard),
        ])
        .await
        .expect("b1");
    let b2 = nodes[0]
        .add_block_from_deploys(&[
            deploy("@30!(3)", 1_004, None, &shard),
            deploy(COMM_CHAIN, 1_005, None, &shard),
        ])
        .await
        .expect("b2");
    assert_eq!(b2.header.parents_hash_list, vec![b0.block_hash.clone()]);
    let b3 = nodes[0]
        .add_block_from_deploys(&[
            deploy(METHOD_ERROR, 1_006, None, &shard),
            deploy(OPERATOR_ERROR, 1_007, None, &shard),
            deploy("@40!(4)", 1_008, None, &shard),
        ])
        .await
        .expect("b3");
    let failed: Vec<bool> = casper::rust::util::proto_util::deploys(&b3)
        .iter()
        .map(|d| d.is_failed)
        .collect();
    assert_eq!(failed, vec![true, true, false]);

    std::fs::create_dir_all(fixture("x").parent().unwrap()).expect("fixture dir");
    for (name, block) in BLOCKS
        .iter()
        .zip([&genesis.genesis_block, &b0, &b1, &b2, &b3])
    {
        std::fs::write(fixture(name), block.to_proto().encode_to_vec()).expect("write fixture");
    }
}

#[tokio::test]
async fn legacy_blocks_replay_to_their_recorded_roots() {
    let runtime_manager =
        mk_runtime_manager("legacy-corpus", Some(Genesis::default_mergeable_tags_arc())).await;
    for name in BLOCKS {
        let bytes = std::fs::read(fixture(name)).expect("read fixture");
        let block = BlockMessage::from_proto(
            BlockMessageProto::decode(bytes.as_slice()).expect("decode fixture"),
        )
        .expect("convert fixture");
        let replayed = runtime_manager
            .replay_compute_state(
                &block.body.state.pre_state_hash,
                casper::rust::util::proto_util::deploys(&block),
                block.body.system_deploys.clone(),
                &BlockData::from_block(&block),
                None,
                block.header.parents_hash_list.is_empty(),
            )
            .await
            .unwrap_or_else(|error| panic!("{name} does not replay: {error:?}"));
        assert_eq!(
            hex::encode(&replayed),
            hex::encode(&block.body.state.post_state_hash),
            "{name}"
        );
    }
}
