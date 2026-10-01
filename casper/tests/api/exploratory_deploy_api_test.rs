// See casper/src/test/scala/coop/rchain/casper/api/ExploratoryDeployAPITest.scala
//
// Tests for the exploratory deploy API, which allows read-only queries
// against the blockchain state.

use std::collections::HashMap;

use casper::rust::api::block_api::BlockAPI;
use casper::rust::util::construct_deploy;
use casper::rust::util::construct_deploy::{DEFAULT_PUB, DEFAULT_PUB2, DEFAULT_SEC};
use crypto::rust::public_key::PublicKey;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GDeployId, Par};

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// Creates genesis parameters with equal bond stakes (10 each) for 3 validators.
fn bonds_function(validators: Vec<PublicKey>) -> HashMap<PublicKey, i64> {
    validators
        .into_iter()
        .zip(vec![10i64, 10i64, 10i64])
        .collect()
}

/// Exploratory deploy should get data from the read-only node.
///
/// DAG structure for finalization:
/// With 3 validators at 10 stake each (total 30), finalization requires >15 stake.
///
///     n1: genesis -> b1 -> b2
///     n2: genesis ---------> b3 (main parent: b2)
///     n3: genesis ---------> b4 (main parent: b3)
///
/// After b3 and b4, b3 accumulates 20 stake (n2 + n3) and is finalized.
#[tokio::test]
async fn exploratory_deploy_should_get_data_from_read_only_node() {
    // Build genesis with 3 validators at 10 stake each
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(
        Some(bonds_function),
        None, // Use default validatorsNum = 4
    );
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("Failed to build genesis");

    // Create network with 3 validators + 1 read-only node
    let mut nodes = TestNode::create_network(
        genesis.clone(),
        3,       // network_size: 3 bonded validators
        None,    // synchrony_constraint_threshold
        None,    // max_number_of_parents
        None,    // max_parent_depth
        Some(1), // with_read_only_size: 1 read-only node
    )
    .await
    .expect("Failed to create network");
    // Heartbeat mode: under deploy-inclusion leadership only the leader packages
    // user deploys while unresolved user work is in the frontier; non-leaders
    // emit empty support blocks instead of erroring NoNewDeploys. The produce
    // deploys below are finalization fillers — the assertions read only the
    // @"store" datum (packaged by the leader) and the LFB.
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let shard_id = genesis.genesis_block.shard_id.clone();
    let stored_data = "data";

    // Create deploys
    // putDataDeploy stores data at @"store"
    let put_data_deploy = construct_deploy::source_deploy(
        format!(r#"@"store"!("{}")"#, stored_data),
        1,
        None,
        None,
        None,
        None,
        Some(shard_id.clone()),
    )
    .expect("Failed to create put data deploy");

    // produceDeploys for subsequent blocks
    let produce_deploy_0 = construct_deploy::source_deploy(
        "new x in { x!(0) }".to_string(),
        2,
        None,
        None,
        None,
        None,
        Some(shard_id.clone()),
    )
    .expect("Failed to create produce deploy 0");

    let produce_deploy_1 = construct_deploy::source_deploy(
        "new x in { x!(1) }".to_string(),
        3,
        None,
        None,
        None,
        None,
        Some(shard_id.clone()),
    )
    .expect("Failed to create produce deploy 1");

    let produce_deploy_2 = construct_deploy::source_deploy(
        "new x in { x!(2) }".to_string(),
        4,
        None,
        None,
        None,
        None,
        Some(shard_id.clone()),
    )
    .expect("Failed to create produce deploy 2");

    // b1: n1 creates block with putDataDeploy and propagates to all
    let _b1 = TestNode::propagate_block_at_index(&mut nodes, 0, &[put_data_deploy])
        .await
        .expect("n1 should create and propagate b1");

    // b2: n1 creates block with produceDeploy(0) and propagates to all
    let b2 = TestNode::propagate_block_at_index(&mut nodes, 0, &[produce_deploy_0])
        .await
        .expect("n1 should create and propagate b2");

    // b3: n2 creates block with produceDeploy(1) and propagates to all
    let b3 = TestNode::propagate_block_at_index(&mut nodes, 1, &[produce_deploy_1])
        .await
        .expect("n2 should create and propagate b3");

    // b4: n3 creates block with produceDeploy(2) and propagates to all
    // This finalizes b3 (n2 + n3 = 20 stake > 15 threshold)
    let _b4 = TestNode::propagate_block_at_index(&mut nodes, 2, &[produce_deploy_2])
        .await
        .expect("n3 should create and propagate b4");

    // Get the read-only node (index 3)
    let read_only_node = &nodes[3];

    // Use node's existing engine_cell instead of creating a new one
    // This ensures we use the same casper instance that processed the blocks
    let engine_cell = &read_only_node.engine_cell;

    // Run exploratory deploy to retrieve stored data
    let exploratory_term = r#"new return in { for (@data <- @"store") { return!(data) } }"#;

    let result = BlockAPI::exploratory_deploy(
        engine_cell,
        exploratory_term.to_string(),
        None,  // block_hash: None reads the last finalized block post-state
        false, // use_pre_state_hash
        false, // dev_mode
        None,  // deployer
    )
    .await;

    // Verify result
    match result {
        Ok((pars, last_finalized_block, _cost)) => {
            // Verify we got the stored data back
            assert!(!pars.is_empty(), "Exploratory deploy should return data");

            // The result should contain our stored data "data"
            let result_str = format!("{:?}", pars);
            assert!(
                result_str.contains(stored_data),
                "Result should contain stored data '{}', got: {:?}",
                stored_data,
                pars
            );

            // Verify last finalized block is in the expected finalized set.
            // Depending on parent tie-breaks, either b2 or b3 can be the current LFB here.
            let b2_hash_hex = hex::encode(&b2.block_hash);
            let b3_hash_hex = hex::encode(&b3.block_hash);
            let expected_lfb_hashes = [b2_hash_hex.clone(), b3_hash_hex.clone()];
            if !expected_lfb_hashes.contains(&last_finalized_block.block_hash) {
                let mut saw_expected_lfb = false;
                for _ in 0..20 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
                    let maybe_lfb = BlockAPI::last_finalized_block(engine_cell).await;
                    if let Ok(lfb) = maybe_lfb {
                        if let Some(block_info) = lfb.block_info {
                            if expected_lfb_hashes.contains(&block_info.block_hash) {
                                saw_expected_lfb = true;
                                break;
                            }
                        }
                    }
                }
                assert!(
                    saw_expected_lfb,
                    "Last finalized block should eventually be one of {:?}. observed={}",
                    expected_lfb_hashes, last_finalized_block.block_hash
                );
            }

            tracing::info!(
                "Exploratory deploy result: {:?}, LFB: {}",
                pars,
                last_finalized_block.block_hash
            );
        }
        Err(e) => {
            panic!("Exploratory deploy failed: {:?}", e);
        }
    }
}

/// An unpinned exploratory read serves the last finalized block post-state, so
/// state that exists only in an unfinalized block is not visible through it.
///
/// The contract is stated in the OpenAPI description, `api-reference.md` and
/// `exploratory-deploy.md`; this asserts it. With 3 validators at 10 stake each
/// (total 30, finalization needs > 15), a single block carries only its
/// proposer's 10 and cannot finalize, so the LFB stays genesis.
///
/// The pinned read is the control: it proves the effect is genuinely in b1 and
/// therefore that the unpinned read's empty result is the read basis at work and
/// not a failed deploy.
#[tokio::test]
async fn unpinned_exploratory_read_does_not_see_unfinalized_state() {
    let parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(Some(bonds_function), None);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, Some(1))
        .await
        .expect("Failed to create network");

    let shard_id = genesis.genesis_block.shard_id.clone();
    let stored_data = "unfinalized";

    let put_data_deploy = construct_deploy::source_deploy(
        format!(r#"@"store"!("{}")"#, stored_data),
        1,
        None,
        None,
        None,
        None,
        Some(shard_id),
    )
    .expect("Failed to create put data deploy");

    // One block from n1: 10 of 30 stake, below the > 15 finalization threshold.
    let b1 = TestNode::propagate_block_at_index(&mut nodes, 0, &[put_data_deploy])
        .await
        .expect("n1 should create and propagate b1");
    assert!(
        !b1.body.deploys[0].is_failed,
        "the deploy must succeed for this test to be about visibility"
    );

    let engine_cell = &nodes[3].engine_cell;
    let exploratory_term = r#"new return in { for (@data <- @"store") { return!(data) } }"#;

    let (unpinned_pars, unpinned_block, _cost) = BlockAPI::exploratory_deploy(
        engine_cell,
        exploratory_term.to_string(),
        None,
        false,
        false,
        None,
    )
    .await
    .expect("unpinned exploratory deploy should succeed");

    assert_eq!(
        unpinned_block.block_hash,
        hex::encode(&genesis.genesis_block.block_hash),
        "an unpinned read must report the finalized block it read, which is still genesis"
    );
    assert!(
        unpinned_pars.is_empty(),
        "unfinalized state must not be visible through an unpinned read, got: {:?}",
        unpinned_pars
    );

    let (pinned_pars, pinned_block, _cost) = BlockAPI::exploratory_deploy(
        engine_cell,
        exploratory_term.to_string(),
        Some(hex::encode(&b1.block_hash)),
        false,
        false,
        None,
    )
    .await
    .expect("pinned exploratory deploy should succeed");

    assert_eq!(pinned_block.block_hash, hex::encode(&b1.block_hash));
    assert!(
        format!("{:?}", pinned_pars).contains(stored_data),
        "a read pinned to b1 must see b1's effect, got: {:?}",
        pinned_pars
    );
}

/// Exploratory deploy should return error on bonded validator.
///
/// The exploratory deploy API should only work on read-only nodes.
/// When called on a bonded validator, it should return an error.
#[tokio::test]
async fn exploratory_deploy_should_return_error_on_bonded_validator() {
    // Build genesis with default parameters
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    // Create network with 1 bonded validator (no read-only nodes)
    let mut nodes = TestNode::create_network(
        genesis.clone(),
        1,    // network_size: 1 bonded validator
        None, // synchrony_constraint_threshold
        None, // max_number_of_parents
        None, // max_parent_depth
        None, // with_read_only_size: None (no read-only nodes)
    )
    .await
    .expect("Failed to create network");

    let shard_id = genesis.genesis_block.shard_id.clone();

    // Create a deploy and propagate a block
    let produce_deploy = construct_deploy::source_deploy(
        "new x in { x!(0) }".to_string(),
        1,
        None,
        None,
        None,
        None,
        Some(shard_id),
    )
    .expect("Failed to create produce deploy");

    let _b1 = TestNode::propagate_block_at_index(&mut nodes, 0, &[produce_deploy])
        .await
        .expect("n1 should create and propagate b1");

    // Use node's existing engine_cell for the bonded validator (node 0)
    let engine_cell = &nodes[0].engine_cell;

    // Try to run exploratory deploy on bonded validator
    let result = BlockAPI::exploratory_deploy(
        engine_cell,
        "new return in { return!(1) }".to_string(),
        None,  // block_hash
        false, // use_pre_state_hash
        false, // dev_mode: false means read-only check is enforced
        None,  // deployer
    )
    .await;

    // Verify it returns an error
    match result {
        Err(e) => {
            let error_message = format!("{:?}", e);
            assert!(
                error_message.contains("Exploratory deploy can only be executed on read-only node"),
                "Expected read-only error message, got: {}",
                error_message
            );
            tracing::info!("Got expected error: {}", error_message);
        }
        Ok(_) => {
            panic!("Exploratory deploy should fail on bonded validator");
        }
    }
}

/// Estimate-cost with deployer must match real deploy cost (issue #53).
///
/// Deploys an identity-dependent RevVault transfer term signed by DEFAULT_SEC
/// (which holds a genesis REV vault), then verifies that:
/// 1. exploratory_deploy with deployer=Some(DEFAULT_PUB) returns the same cost
/// 2. exploratory_deploy with deployer=None returns a different cost
///
/// This pins the acceptance criterion of issue #53: for identity-dependent
/// terms such as REV vault transfers, passing the deployer public key is
/// essential — without it the term executes under an ephemeral identity and
/// the returned cost can be significantly lower than the real deploy cost.
#[tokio::test]
async fn estimate_cost_with_deployer_matches_real_deploy_cost() {
    // Build genesis with 3 validators at 10 stake each
    let parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(Some(bonds_function), None);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("Failed to build genesis");

    // Create network with 3 validators + 1 read-only node
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, Some(1))
        .await
        .expect("Failed to create network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let shard_id = genesis.genesis_block.shard_id.clone();

    // Identity-dependent term: RevVault transfer from the deployer's own vault.
    // The RevAddress is derived from the deployer's identity via deployerId,
    // so the execution path and cost depend on who the deployer is.
    // DEFAULT_SEC has a genesis vault with 9M balance.
    let vault_transfer_term = r#"
        new
            rl(`rho:registry:lookup`), SystemVaultCh,
            deployerId(`rho:system:deployerId`),
            vaultAddressOps(`rho:vault:address`),
            vaultAddrCh, vaultCh, targetVaultCh, authKeyCh, ret
        in {
            rl!(`rho:vault:system`, *SystemVaultCh) |
            for (@(_, SystemVault) <- SystemVaultCh) {
                vaultAddressOps!("fromDeployerId", *deployerId, *vaultAddrCh) |
                for (@vaultAddr <- vaultAddrCh) {
                    @SystemVault!("findOrCreate", vaultAddr, *vaultCh) |
                    @SystemVault!("findOrCreate", "1111111111111111111111111111111111111111111111111111", *targetVaultCh) |
                    @SystemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
                    for (@(true, vault) <- vaultCh & @(true, _) <- targetVaultCh & key <- authKeyCh) {
                        @vault!("transfer", "1111111111111111111111111111111111111111111111111111", 100, *key, *ret)
                    }
                }
            }
        }
    "#;

    // Step 1: Deploy as a real deploy signed with DEFAULT_SEC
    let real_deploy = construct_deploy::source_deploy(
        vault_transfer_term.to_string(),
        1,
        Some(5_000_000),
        None,
        Some(DEFAULT_SEC.clone()),
        None,
        Some(shard_id.clone()),
    )
    .expect("Failed to create deploy");

    // Propagate a block with the real deploy
    let transfer_block = TestNode::propagate_block_at_index(&mut nodes, 0, &[real_deploy])
        .await
        .expect("n1 should create and propagate block");

    // Get the real cost from the processed deploy
    assert!(
        !transfer_block.body.deploys.is_empty(),
        "Block should contain the deploy"
    );
    let actual_cost = transfer_block.body.deploys[0].cost.cost;
    tracing::info!("Real deploy cost: {}", actual_cost);

    // Step 2: Get the parent block's post-state hash (state before the transfer)
    // We need to estimate against the state the real deploy executed against.
    // The parent is the block just before the transfer block.
    let parent_hash_hex = transfer_block
        .header
        .parents_hash_list
        .first()
        .map(hex::encode)
        .expect("Transfer block should have at least one parent");

    // Step 3: Call exploratory_deploy with deployer = Some(DEFAULT_PUB)
    // against the parent state (same state the real deploy started from)
    let result_with_deployer = BlockAPI::exploratory_deploy(
        &nodes[3].engine_cell,
        vault_transfer_term.to_string(),
        Some(parent_hash_hex.clone()),
        false,
        false,
        Some(DEFAULT_PUB.clone()),
    )
    .await
    .expect("exploratory deploy with deployer should succeed");
    let cost_with_deployer = result_with_deployer.2;

    // Step 4: Assert costs match exactly
    assert_eq!(
        actual_cost, cost_with_deployer,
        "Cost with deployer ({}) must match real deploy cost ({})",
        cost_with_deployer, actual_cost
    );

    // Step 5: Call exploratory_deploy with deployer = None (ephemeral identity)
    // against the same parent state
    let result_without_deployer = BlockAPI::exploratory_deploy(
        &nodes[3].engine_cell,
        vault_transfer_term.to_string(),
        Some(parent_hash_hex),
        false,
        false,
        None,
    )
    .await
    .expect("exploratory deploy without deployer should succeed");
    let cost_without_deployer = result_without_deployer.2;

    // Step 6: Assert costs differ — this pins the original bug (#53)
    // Under an ephemeral identity the vault path diverges (no vault exists
    // for the ephemeral deployer), so the estimate is wrong.
    assert_ne!(
        actual_cost, cost_without_deployer,
        "Cost without deployer ({}) must differ from real deploy cost ({}) — identity-dependent terms require deployer key",
        cost_without_deployer, actual_cost
    );

    tracing::info!(
        "actual_cost={}, cost_with_deployer={}, cost_without_deployer={}",
        actual_cost,
        cost_with_deployer,
        cost_without_deployer
    );
}

fn extract_deploy_id_sig(par: &Par) -> Option<Vec<u8>> {
    par.unforgeables.iter().find_map(|u| match &u.unf_instance {
        Some(UnfInstance::GDeployIdBody(GDeployId { sig })) => Some(sig.clone()),
        _ => None,
    })
}

#[tokio::test]
async fn estimate_cost_deploy_id_is_64_bytes_on_deployer_path() {
    let parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(Some(bonds_function), None);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, Some(1))
        .await
        .expect("Failed to create network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let term = r#"new return, deployId(`rho:rchain:deployId`) in { return!(*deployId) }"#;

    // Path 1: `deployer` public key is passed. Sig is a real secp256k1 DER signature over a
    // preimage that folds the `deployer` public key in (via create_unbound).
    let result_with_deployer = BlockAPI::exploratory_deploy(
        &nodes[3].engine_cell,
        term.to_string(),
        None,
        false,
        false,
        Some(DEFAULT_PUB.clone()),
    )
    .await
    .expect("exploratory deploy with deployer should succeed");
    let (pars_with, _block_with, cost_with) = result_with_deployer;

    let sig_with = extract_deploy_id_sig(&pars_with[0])
        .expect("Some(deployer) path should return a GDeployId unforgeable on rho:rchain:deployId");
    assert!(
        !sig_with.is_empty(),
        "deployId sig must be non-empty on Some(deployer) path (real secp256k1 DER signature), got {} bytes",
        sig_with.len()
    );
    assert!(
        !sig_with.iter().all(|&b| b == 0),
        "deployId sig must be non-trivial (not all-zero) on Some(deployer) path"
    );

    // Path 2: no `deployer` public key is passed.
    let result_without_deployer = BlockAPI::exploratory_deploy(
        &nodes[3].engine_cell,
        term.to_string(),
        None,
        false,
        false,
        None,
    )
    .await
    .expect("exploratory deploy without deployer should succeed");
    let (pars_without, _block_without, cost_without) = result_without_deployer;

    let sig_without = extract_deploy_id_sig(&pars_without[0])
        .expect("None path should return a GDeployId unforgeable on rho:rchain:deployId");
    assert!(
        !sig_without.is_empty(),
        "deployId sig must be non-empty on None path (real secp256k1 DER signature)"
    );

    let diff = cost_with.abs_diff(cost_without);
    let tolerance = cost_with.max(cost_without) / 20; // 5%
    assert!(
        diff <= tolerance,
        "deployId-reading term cost must match within 5% between Some(deployer) ({}) and None ({}) paths (diff={}, tolerance={})",
        cost_with,
        cost_without,
        diff,
        tolerance
    );

    tracing::info!(
        "deployId sig lengths: with_deployer={}, without_deployer={}; costs: with={}, without={}",
        sig_with.len(),
        sig_without.len(),
        cost_with,
        cost_without
    );
}

/// Two different deployer public keys must observe different `rho:rchain:deployId`
/// values for the same term, and repeating the same (term, deployer) within one
/// process must observe the same value (process-wide ephemeral key is stable).
#[tokio::test]
async fn exploratory_deploy_deploy_id_varies_by_deployer_and_is_stable() {
    let parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(Some(bonds_function), None);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, Some(1))
        .await
        .expect("Failed to create network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let term = r#"new return, deployId(`rho:rchain:deployId`) in { return!(*deployId) }"#;

    // Two different deployers must produce different deployId values.
    let result_a = BlockAPI::exploratory_deploy(
        &nodes[3].engine_cell,
        term.to_string(),
        None,
        false,
        false,
        Some(DEFAULT_PUB.clone()),
    )
    .await
    .expect("exploratory deploy with deployer A should succeed");
    let sig_a =
        extract_deploy_id_sig(&result_a.0[0]).expect("should return GDeployId for deployer A");

    let result_b = BlockAPI::exploratory_deploy(
        &nodes[3].engine_cell,
        term.to_string(),
        None,
        false,
        false,
        Some(DEFAULT_PUB2.clone()),
    )
    .await
    .expect("exploratory deploy with deployer B should succeed");
    let sig_b =
        extract_deploy_id_sig(&result_b.0[0]).expect("should return GDeployId for deployer B");

    assert_ne!(
        sig_a, sig_b,
        "deployId must differ between different deployer public keys"
    );

    // Repeating the same (term, deployer) within one process must yield the same value
    // because the ephemeral signing key is process-wide stable.
    let result_a2 = BlockAPI::exploratory_deploy(
        &nodes[3].engine_cell,
        term.to_string(),
        None,
        false,
        false,
        Some(DEFAULT_PUB.clone()),
    )
    .await
    .expect("exploratory deploy with deployer A (repeat) should succeed");
    let sig_a2 = extract_deploy_id_sig(&result_a2.0[0])
        .expect("should return GDeployId for deployer A (repeat)");

    assert_eq!(
        sig_a, sig_a2,
        "deployId must be stable for the same (term, deployer) within one process"
    );

    tracing::info!(
        "deployId sig lengths: deployer_A={}, deployer_B={}; stable repeat={}",
        sig_a.len(),
        sig_b.len(),
        sig_a == sig_a2
    );
}
