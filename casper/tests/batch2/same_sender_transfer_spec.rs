// Transfers from one sender. Each transfer sprouts a temporary purse whose
// balance is a mergeable-tagged number cell, named from the deploy's
// unforgeable-name seed: (deployer key, timestamp, valid-after block). Two
// distinct deploys signed by one key with the same seed inputs create the
// same purse: its cell is initialized twice and the next read trips the
// IntegerAdd single-value invariant. A capacity run wedged two of four
// validators this way.

use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::tools::Tools;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use num_bigint::BigInt;
use rholang::rust::interpreter::rho_type::RhoBigInt;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

const RECIPIENT: &str = "1111LAd2PWaHsw84gxarNx99YVK2aZhCThhrPsWTV7cs1BPcvHftP";

fn transfer(payer: &str, timestamp: i64, shard_id: &str) -> Signed<DeployData> {
    transfer_with_phlo(payer, timestamp, 1_000_000, shard_id)
}

fn transfer_with_phlo(
    payer: &str,
    timestamp: i64,
    phlo_limit: i64,
    shard_id: &str,
) -> Signed<DeployData> {
    let term = r#"
        new rl(`rho:registry:lookup`), systemCh, vaultCh, authCh, ret,
            deployerId(`rho:system:deployerId`), deployId(`rho:system:deployId`) in {
            rl!(`rho:vault:system`, *systemCh) |
            for (@(_, system) <- systemCh) {
                @system!("findOrCreate", "PAYER", *vaultCh) |
                @system!("deployerAuthKey", *deployerId, *authCh) |
                for (@(true, vault) <- vaultCh & key <- authCh) {
                    @vault!("transfer", "RECIPIENT", 1, *key, *ret) |
                    for (@result <- ret) { deployId!(result) }
                }
            }
        }
    "#
    .replace("PAYER", payer)
    .replace("RECIPIENT", RECIPIENT);
    construct_deploy::source_deploy(
        term,
        timestamp,
        Some(phlo_limit),
        None,
        None,
        None,
        Some(shard_id.to_string()),
    )
    .expect("transfer deploy")
}

async fn propose_and_replay(
    nodes: &mut [TestNode],
    deploys: &[Signed<DeployData>],
    label: &str,
) -> BlockMessage {
    let block = nodes[0]
        .add_block_from_deploys(deploys)
        .await
        .unwrap_or_else(|e| panic!("{label}: proposal failed: {e:?}"));
    assert_eq!(
        block.body.deploys.len(),
        deploys.len(),
        "{label}: every transfer must be included"
    );
    for processed in &block.body.deploys {
        assert!(
            !processed.is_failed && processed.system_deploy_error.is_none(),
            "{label}: transfer {} failed: {:?}",
            hex::encode(&processed.deploy.sig[..8]),
            processed.system_deploy_error
        );
    }
    let replay = nodes[1]
        .process_block(block.clone())
        .await
        .unwrap_or_else(|e| panic!("{label}: replay failed: {e:?}"));
    assert!(
        matches!(replay, Either::Right(_)),
        "{label}: replay {replay:?}"
    );
    block
}

async fn network() -> (Vec<TestNode>, String, String) {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, None),
        ))
        .await
        .expect("genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
    let payer = VaultAddress::from_public_key(&construct_deploy::DEFAULT_PUB)
        .expect("payer address")
        .to_base58();
    let nodes = TestNode::create_network(genesis, 2, None, None, None, None)
        .await
        .expect("network");
    (nodes, shard_id, payer)
}

/// The validator must survive deploys whose seeds collide. Offered together
/// with an unrelated deploy, then followed by further proposals, no
/// proposal fails, the unrelated deploy lands, every block replays as
/// valid, and no two deploys sharing a seed (nor one sharing a seed with a
/// deploy already in state) are ever included.
async fn must_survive(
    nodes: &mut [TestNode],
    colliding: &[Signed<DeployData>],
    already_in_state: Option<&Signed<DeployData>>,
    shard_id: &str,
    label: &str,
) {
    const ROUNDS: i32 = 3;
    let marker = |nonce: i32| {
        construct_deploy::basic_deploy_data(nonce, None, Some(shard_id.to_string()))
            .expect("unrelated deploy")
    };
    let unrelated = marker(0);
    let mut offered = colliding.to_vec();
    offered.push(unrelated.clone());

    let mut included = Vec::new();
    for round in 1..=ROUNDS {
        let block = nodes[0]
            .add_block_from_deploys(&offered)
            .await
            .unwrap_or_else(|e| {
                let message = format!("{e:?}");
                assert!(
                    !message.contains("single-value invariant"),
                    "{label}: SEED COLLISION — round {round} failed on a cell two \
                     deploys created under one (key, timestamp) seed: {message}"
                );
                panic!("{label}: round {round} proposal failed: {message}")
            });
        let replay = nodes[1]
            .process_block(block.clone())
            .await
            .unwrap_or_else(|e| panic!("{label}: round {round} replay failed: {e:?}"));
        assert!(
            matches!(replay, Either::Right(_)),
            "{label}: round {round} replay {replay:?}"
        );
        included.extend(block.body.deploys.iter().map(|pd| pd.deploy.sig.clone()));
        offered = vec![marker(round)];
    }

    assert!(
        included.contains(&unrelated.sig),
        "{label}: the unrelated deploy never landed within {ROUNDS} proposals"
    );
    let colliding_included = colliding
        .iter()
        .filter(|d| included.contains(&d.sig))
        .count();
    let allowed = if already_in_state.is_some() { 0 } else { 1 };
    assert!(
        colliding_included <= allowed,
        "{label}: {colliding_included} deploys sharing a seed with each other or \
         with state were included; at most {allowed} may be"
    );
}

/// Two distinct transfers signed by one key at one timestamp (they differ
/// only in phlo limit, so their signatures differ), in consecutive blocks —
/// the shape that wedged validator 4.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_transfer_sharing_key_and_timestamp_with_one_in_state_must_not_wedge() {
    let (mut nodes, shard_id, payer) = network().await;
    let first = transfer_with_phlo(&payer, 7, 1_000_000, &shard_id);
    let second = transfer_with_phlo(&payer, 7, 1_000_001, &shard_id);
    assert_ne!(first.sig, second.sig, "the two deploys must be distinct");

    propose_and_replay(
        &mut nodes,
        std::slice::from_ref(&first),
        "the first transfer",
    )
    .await;
    must_survive(
        &mut nodes,
        std::slice::from_ref(&second),
        Some(&first),
        &shard_id,
        "a second transfer with the first's key and timestamp",
    )
    .await;
}

/// The same pair in one block — the two-deploy attempt that failed first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn transfers_sharing_key_and_timestamp_in_one_block_must_not_wedge() {
    let (mut nodes, shard_id, payer) = network().await;
    let first = transfer_with_phlo(&payer, 7, 1_000_000, &shard_id);
    let second = transfer_with_phlo(&payer, 7, 1_000_001, &shard_id);
    assert_ne!(first.sig, second.sig, "the two deploys must be distinct");

    must_survive(
        &mut nodes,
        &[first, second],
        None,
        &shard_id,
        "two transfers sharing key and timestamp in one block",
    )
    .await;
}

async fn recipient_balance(node: &TestNode, block: &BlockMessage) -> BigInt {
    let source = r#"
        new return, rl(`rho:registry:lookup`), systemCh, vaultCh in {
            rl!(`rho:vault:system`, *systemCh) |
            for (@(_, system) <- systemCh) {
                @system!("findOrCreate", "RECIPIENT", *vaultCh) |
                for (@(true, vault) <- vaultCh) { @vault!("balance", *return) }
            }
        }
    "#
    .replace("RECIPIENT", RECIPIENT);
    let (values, _) = node
        .runtime_manager
        .play_exploratory_deploy(source, &block.body.state.post_state_hash, None, Vec::new())
        .await
        .expect("balance read");
    assert_eq!(values.len(), 1, "one balance reply");
    RhoBigInt::unapply(&values[0]).expect("balance is a number")
}

/// The pair sent to two validators: each includes one in a sibling block,
/// and a merge block joins them. The two are one deploy by seed, so the
/// joined state must carry one transfer, not two.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn transfers_sharing_key_and_timestamp_on_two_validators_must_merge_to_one() {
    let (mut nodes, shard_id, payer) = network().await;
    let first = transfer_with_phlo(&payer, 7, 1_000_000, &shard_id);
    let second = transfer_with_phlo(&payer, 7, 1_000_001, &shard_id);
    assert_ne!(first.sig, second.sig, "the two deploys must be distinct");

    let a = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&first))
        .await
        .expect("validator 0 includes the first");
    let b = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&second))
        .await
        .expect("validator 1 includes the second");
    assert_eq!(recipient_balance(&nodes[0], &a).await, BigInt::from(1));
    assert_eq!(recipient_balance(&nodes[1], &b).await, BigInt::from(1));
    nodes[0].process_block(b.clone()).await.expect("0 sees b");
    nodes[1].process_block(a.clone()).await.expect("1 sees a");

    let unrelated = construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone()))
        .expect("unrelated deploy");
    let merge = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&unrelated))
        .await
        .unwrap_or_else(|e| panic!("the merge block failed: {e:?}"));
    assert!(
        merge.header.parents_hash_list.contains(&a.block_hash)
            && merge.header.parents_hash_list.contains(&b.block_hash),
        "the merge block must join both siblings"
    );
    let replay = nodes[1]
        .process_block(merge.clone())
        .await
        .unwrap_or_else(|e| panic!("merge replay failed: {e:?}"));
    assert!(
        matches!(replay, Either::Right(_)),
        "merge replay {replay:?}"
    );

    assert_eq!(
        recipient_balance(&nodes[0], &merge).await,
        BigInt::from(1),
        "DUPLICATE BY SEED: the merge kept both transfers sharing key and \
         timestamp (rejected: {:?})",
        merge
            .body
            .rejected_deploys
            .iter()
            .map(|r| hex::encode(&r.sig[..8]))
            .collect::<Vec<_>>()
    );
}

/// Two deploys sharing a seed that touch nothing but their own first private
/// name, on two validators. Nothing makes them conflict, so a merge that
/// treats them as two deploys keeps both sends on the one shared name.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn private_sends_sharing_a_seed_on_two_validators_must_merge_to_one() {
    let (mut nodes, shard_id, _payer) = network().await;
    let private_send = |phlo_limit: i64| {
        construct_deploy::source_deploy(
            r#"new x in { x!("once") }"#.to_string(),
            7,
            Some(phlo_limit),
            None,
            None,
            None,
            Some(shard_id.clone()),
        )
        .expect("private send deploy")
    };
    let first = private_send(100_000);
    let second = private_send(100_001);
    assert_ne!(first.sig, second.sig, "the two deploys must be distinct");
    let x = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
            id: Tools::deploy_name_rng(&first)
                .next()
                .into_iter()
                .map(|b| b as u8)
                .collect(),
        })),
    }]);
    let second_x_id: Vec<u8> = Tools::deploy_name_rng(&second)
        .next()
        .into_iter()
        .map(|b| b as u8)
        .collect();
    assert_eq!(
        x.unforgeables[0].unf_instance,
        Some(UnfInstance::GPrivateBody(GPrivate { id: second_x_id })),
        "deploys sharing a seed share their private names"
    );

    let a = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&first))
        .await
        .expect("validator 0 includes the first");
    let b = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&second))
        .await
        .expect("validator 1 includes the second");
    nodes[0].process_block(b.clone()).await.expect("0 sees b");
    nodes[1].process_block(a.clone()).await.expect("1 sees a");

    let unrelated = construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone()))
        .expect("unrelated deploy");
    let merge = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&unrelated))
        .await
        .unwrap_or_else(|e| panic!("the merge block failed: {e:?}"));
    assert!(
        merge.header.parents_hash_list.contains(&a.block_hash)
            && merge.header.parents_hash_list.contains(&b.block_hash),
        "the merge block must join both siblings"
    );

    let datums = nodes[0]
        .runtime_manager
        .get_data(merge.body.state.post_state_hash.clone(), &x)
        .await
        .expect("read the shared private name");
    assert_eq!(
        datums.len(),
        1,
        "DUPLICATE BY SEED: the merge kept both deploys' sends on their shared \
         private name (rejected: {:?})",
        merge
            .body
            .rejected_deploys
            .iter()
            .map(|r| hex::encode(&r.sig[..8]))
            .collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn transfers_from_one_sender_in_one_block_and_on_top_of_each_other() {
    let (mut nodes, shard_id, payer) = network().await;

    propose_and_replay(
        &mut nodes,
        &[
            transfer(&payer, 1, &shard_id),
            transfer(&payer, 2, &shard_id),
        ],
        "two transfers from one sender in one block",
    )
    .await;
    propose_and_replay(
        &mut nodes,
        &[transfer(&payer, 3, &shard_id)],
        "a transfer on top of the sender's previous block",
    )
    .await;
    propose_and_replay(
        &mut nodes,
        &[
            transfer(&payer, 4, &shard_id),
            transfer(&payer, 5, &shard_id),
            transfer(&payer, 6, &shard_id),
        ],
        "three transfers from one sender in one block",
    )
    .await;
}
