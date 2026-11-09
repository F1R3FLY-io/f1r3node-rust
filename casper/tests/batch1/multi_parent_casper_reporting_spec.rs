// See casper/src/main/scala/coop/rchain/casper/batch1/MultiParentCasperReportingSpec.scala

// ReportingCasper IS ported to Rust (casper/src/rust/reporting_casper.rs): the `ReportingCasper`
// trait, the real `RhoReporterCasper` implementation, `ReportingRuntime`, the `trace` method, and
// the `rho_reporter` factory all exist and are wired into the node's block-report API
// (node/src/rust/runtime/setup.rs, casper/src/rust/api/block_report_api.rs). This test exercises the
// invariant the Scala spec checked: replaying a block through the reporting runtime reproduces
// exactly the same post-state as the block produced under normal multi-parent execution — that is
// what "behaves the same way as multi-parent casper" means.

use std::time::Duration;

// Changed by DR-134 (DR-70): an offered block report needs the Casper state.
// use casper::rust::reporting_casper;
use casper::rust::api::block_api::BlockAPI;
use casper::rust::reporting_casper::{self, ReportContext};
// Added by DR-134 (DR-66 item 5, DR-70): the genesis and historical-block
// report tests.
use casper::rust::util::rholang::costacc::offered_only::{
    BODY_ONLY_DEPLOYS_UNSUPPORTED, LEGACY_METERED_GENESIS_UNSUPPORTED,
};
use models::rhoapi::PCost;
use models::rust::casper::protocol::casper_message::{ProcessedDeploy, ProcessedUserDeploy};
use rholang::rust::interpreter::external_services::ExternalServices;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::reporting_rspace::ReportPhase;

// Changed by DR-134 (DR-66 item 1): offered deploys.
// use casper::rust::util::construct_deploy;
use crate::helper::offered_deploy as construct_deploy;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_test_rnode_store_manager_shared;

#[tokio::test]
async fn reporting_casper_should_behave_the_same_way_as_multi_parent_casper() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut node = TestNode::standalone(genesis.clone())
        .await
        .expect("Failed to create standalone node");

    let correct_rholang = r#" for(@a <- @"1"){ Nil } | @"1"!("x") "#;

    let deploy = construct_deploy::source_deploy_now(
        correct_rholang.to_string(),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("Failed to construct deploy");

    // Add the block via the normal (multi-parent) path. `add_block_from_deploys` runs the block
    // through the full validation pipeline (which itself replays and checks the post-state), so the
    // returned block's recorded post-state is, by construction, reproducible by replay.
    let signed_block = node
        .add_block_from_deploys(&[deploy])
        .await
        .expect("Failed to add block");

    // The node shares its RSpace history under `genesis.rspace_scope_id`. Open a store manager on the
    // same scope to hand the reporting runtime the committed pre-state (the genesis post-state) that
    // it resets to before replaying the block's deploys.
    let mut rspace_kvm = mk_test_rnode_store_manager_shared(genesis.rspace_scope_id.clone());
    let rspace_store = rspace_kvm
        .r_space_stores()
        .await
        .expect("Failed to open shared RSpace stores");

    let reporter = reporting_casper::rho_reporter(
        &rspace_store,
        &node.block_dag_storage,
        node.runtime_manager.replay_lock(),
        ExternalServices::noop(),
    );

    // Changed by DR-134 (DR-70): an offered block needs the Casper state.
    // let replay = reporter
    //     .trace(&signed_block)
    //     .await
    //     .expect("ReportingCasper::trace failed");
    let replay = reporter
        .trace_in(&signed_block, ReportContext {
            runtime_manager: &node.runtime_manager,
            adopted: node.casper.adopted_resource_policy.as_ref(),
        })
        .await
        .expect("ReportingCasper::trace_in failed");

    // "Behaves the same way as multi-parent casper": the reporting replay reproduces the exact
    // post-state hash the block recorded under normal execution.
    assert_eq!(
        replay.post_state_hash,
        signed_block.body.state.post_state_hash.to_vec(),
        "reporting replay post-state must equal the block's recorded post-state"
    );

    // The single user deploy in the block is traced (one DeployReportResult).
    assert_eq!(
        replay.deploy_report_result.len(),
        1,
        "reporting should trace the one user deploy in the block"
    );

    // Added by DR-134 (DR-70): the offered deploy reports its committed native
    // evidence and the receipt of the receipt route, and no event batches.
    let offered = signed_block.body.deploys[0]
        .as_offered()
        .expect("the block deploy is offered");
    let deploy_report = &replay.deploy_report_result[0];
    let report = deploy_report
        .offered
        .as_ref()
        .expect("an offered deploy has an offered report");
    assert_eq!(
        report.native_cost_evidence,
        offered.native_cost_evidence_bytes()
    );
    // Changed by DR-134 (DR-70): the offered deploy reports the events of its
    // user run in one USER batch, as dev reports a user deploy, and the COMM of
    // the term on @"1" is among them.
    // assert!(
    //     deploy_report.events.is_empty(),
    //     "an offered deploy reports no event batches"
    // );
    let user_batches: Vec<_> = deploy_report
        .events
        .iter()
        .filter(|batch| batch.phase == ReportPhase::User)
        .collect();
    assert_eq!(
        user_batches.len(),
        1,
        "an offered deploy reports one USER batch"
    );
    let channel_one = models::rust::utils::new_gstring_par("1".to_string(), Vec::new(), false);
    assert!(
        user_batches[0].events.iter().any(|event| matches!(
            event,
            rspace_plus_plus::rspace::reporting_rspace::ReportingEvent::ReportingComm(comm)
                if comm.consume.channels == vec![channel_one.clone()]
        )),
        "the USER batch holds the COMM of the user run on @\"1\""
    );
    let receipt_route = BlockAPI::find_offered_settlement_receipt(
        &node.engine_cell,
        &offered.identity_bytes().to_vec(),
    )
    .await
    .expect("the receipt route answers");
    assert_eq!(report.receipt, receipt_route);
    assert!(
        report
            .receipt
            .as_ref()
            .is_some_and(|receipt| !receipt.purses.is_empty()),
        "the receipt names the payer purse"
    );
}

/// A block carrying a deploy that failed during execution must still produce a report.
///
/// Reporting propagates replay and report-collection failures instead of degrading them to empty
/// events, so any replay-check condition reachable for a failed deploy would make that block's
/// report permanently unavailable. `check_replay_data_with_fix`
/// (casper/src/rust/rholang/replay_runtime.rs) tolerates leftover replay data when the deploy
/// failed, and the rollback on failure restores the hot store, event log and produce counter but
/// NOT the replay-data multimap — so the tolerance path is load-bearing rather than decorative.
#[tokio::test]
async fn reporting_a_block_with_a_failed_deploy_still_produces_a_report() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut node = TestNode::standalone(genesis.clone())
        .await
        .expect("Failed to create standalone node");

    // A COMM completes (send + receive on `ch`) and the continuation THEN fails: negation of a
    // string is an OperatorNotDefined at reduce time (rholang/src/rust/interpreter/reduce.rs).
    // Failing after a COMM is what can leave replay data consumed while the rollback discards the
    // events. A term that fails while evaluating a send's argument never COMMs and would not
    // exercise that path.
    let failing_rholang = r#" new ch in { ch!(1) | for(@x <- ch){ @"out"!(-"boom") } } "#;

    let deploy = construct_deploy::source_deploy_now(
        failing_rholang.to_string(),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("Failed to construct deploy");

    let signed_block = node
        .add_block_from_deploys(&[deploy])
        .await
        .expect("Failed to add block");

    // Guard the premise: if the term stopped failing this test would pass vacuously.
    assert_eq!(
        signed_block.body.deploys.len(),
        1,
        "block should carry the one user deploy"
    );
    assert!(
        signed_block.body.deploys[0].is_failed(),
        "premise: the deploy must be recorded as failed for this test to mean anything"
    );

    let mut rspace_kvm = mk_test_rnode_store_manager_shared(genesis.rspace_scope_id.clone());
    let rspace_store = rspace_kvm
        .r_space_stores()
        .await
        .expect("Failed to open shared RSpace stores");

    let reporter = reporting_casper::rho_reporter(
        &rspace_store,
        &node.block_dag_storage,
        node.runtime_manager.replay_lock(),
        ExternalServices::noop(),
    );

    // Changed by DR-134 (DR-70): an offered block needs the Casper state.
    // let replay = reporter.trace(&signed_block).await;
    let replay = reporter
        .trace_in(&signed_block, ReportContext {
            runtime_manager: &node.runtime_manager,
            adopted: node.casper.adopted_resource_policy.as_ref(),
        })
        .await;

    assert!(
        replay.is_ok(),
        "a block with a failed deploy must still report: {:?}",
        replay.err()
    );

    let replay = replay.expect("trace succeeded");
    assert_eq!(
        replay.post_state_hash,
        signed_block.body.state.post_state_hash.to_vec(),
        "reporting replay post-state must equal the block's recorded post-state"
    );

    // Added by DR-134 (DR-70): a failed offered deploy keeps its charge, so its
    // report carries the evidence and a receipt with a nonzero charge.
    let report = replay.deploy_report_result[0]
        .offered
        .as_ref()
        .expect("a failed offered deploy has an offered report");
    assert!(
        report
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.phlo_used > 0 && receipt.rev_spent > 0),
        "the failed deploy is charged for its COMM"
    );
}

#[tokio::test]
async fn reporting_replay_runs_while_a_consensus_replay_runs() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let mut node = TestNode::standalone(genesis.clone())
        .await
        .expect("Failed to create standalone node");
    let deploy = construct_deploy::source_deploy_now(
        r#"for (@a <- @"1") { Nil } | @"1"!("x")"#.to_string(),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("Failed to construct deploy");
    let signed_block = node
        .add_block_from_deploys(&[deploy])
        .await
        .expect("Failed to add block");
    let mut rspace_kvm = mk_test_rnode_store_manager_shared(genesis.rspace_scope_id.clone());
    let rspace_store = rspace_kvm
        .r_space_stores()
        .await
        .expect("Failed to open shared RSpace stores");
    let replay_lock = node.runtime_manager.replay_lock();
    let consensus_permit = replay_lock
        .acquire_consensus()
        .await
        .expect("Replay semaphore closed");
    let reporter = reporting_casper::rho_reporter(
        &rspace_store,
        &node.block_dag_storage,
        replay_lock,
        ExternalServices::noop(),
    );
    let replay = tokio::time::timeout(
        Duration::from_secs(30),
        reporter.trace_in(&signed_block, ReportContext {
            runtime_manager: &node.runtime_manager,
            adopted: node.casper.adopted_resource_policy.as_ref(),
        }),
    )
    .await
    .expect("Reporting replay waited for the running consensus replay")
    .expect("Reporting replay failed");
    assert_eq!(
        replay.post_state_hash,
        signed_block.body.state.post_state_hash.to_vec()
    );
    drop(consensus_permit);
}

#[tokio::test]
async fn block_replay_proceeds_while_another_consensus_replay_runs() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("Failed to create network");
    let deploy = construct_deploy::source_deploy_now(
        r#"for (@a <- @"1") { Nil } | @"1"!("x")"#.to_string(),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("Failed to construct deploy");
    let block = nodes[0]
        .add_block_from_deploys(&[deploy])
        .await
        .expect("Failed to add block");

    let in_flight_replay = nodes[1]
        .runtime_manager
        .replay_lock()
        .acquire_consensus()
        .await
        .expect("Replay semaphore closed");
    let status = tokio::time::timeout(Duration::from_secs(60), nodes[1].process_block(block))
        .await
        .expect("Block replay waited for the in-flight consensus replay")
        .expect("Block processing failed");
    assert!(
        matches!(status, Either::Right(_)),
        "Block was not valid: {:?}",
        status
    );
    drop(in_flight_replay);
}

// Added by DR-134 (DR-66 items 4 and 5, DR-70): the genesis replays in the
// trusted install scope, so its report reproduces the genesis post-state,
// records cost 0 for every deploy and carries only USER or UNSPECIFIED
// segments. Ported from the genesis half of the disabled
// precharge_report_shape_spec.
#[tokio::test]
async fn genesis_reports_in_the_trusted_install_scope() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let node = TestNode::standalone(genesis.clone())
        .await
        .expect("Failed to create standalone node");
    let mut rspace_kvm = mk_test_rnode_store_manager_shared(genesis.rspace_scope_id.clone());
    let rspace_store = rspace_kvm
        .r_space_stores()
        .await
        .expect("Failed to open shared RSpace stores");
    let reporter = reporting_casper::rho_reporter(
        &rspace_store,
        &node.block_dag_storage,
        node.runtime_manager.replay_lock(),
        ExternalServices::noop(),
    );

    let replay = reporter
        .trace_in(&genesis.genesis_block, ReportContext {
            runtime_manager: &node.runtime_manager,
            adopted: node.casper.adopted_resource_policy.as_ref(),
        })
        .await
        .expect("reporting replay of the genesis block failed");

    assert_eq!(
        replay.post_state_hash,
        genesis.genesis_block.body.state.post_state_hash.to_vec(),
        "the genesis report reproduces the genesis post-state"
    );
    assert!(
        !replay.deploy_report_result.is_empty(),
        "genesis block must replay at least one deploy"
    );
    for (index, deploy_result) in replay.deploy_report_result.iter().enumerate() {
        assert_eq!(
            deploy_result.processed_deploy.cost().cost,
            0,
            "genesis deploy {index} records cost 0 in the trusted install scope"
        );
        assert!(deploy_result.offered.is_none());
        for batch in &deploy_result.events {
            assert!(
                matches!(batch.phase, ReportPhase::User | ReportPhase::Unspecified),
                "genesis deploy {index} carries only USER or UNSPECIFIED segments, got {:?}",
                batch.phase
            );
        }
    }

    // The node is held until here to keep the DAG storage alive for the
    // reporting replay above.
    drop(node);
}

// Added by DR-134 (DR-66 item 5, DR-70): a body-only user deploy after genesis
// and a genesis that the legacy meter produced return the compatibility error
// before any permit, runtime or reset. The crafted pre-state root does not
// exist, so a reset would fail with a different error.
#[tokio::test]
async fn historical_blocks_return_the_compatibility_error_before_any_reset() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let node = TestNode::standalone(genesis.clone())
        .await
        .expect("Failed to create standalone node");
    let mut rspace_kvm = mk_test_rnode_store_manager_shared(genesis.rspace_scope_id.clone());
    let rspace_store = rspace_kvm
        .r_space_stores()
        .await
        .expect("Failed to open shared RSpace stores");
    let reporter = reporting_casper::rho_reporter(
        &rspace_store,
        &node.block_dag_storage,
        node.runtime_manager.replay_lock(),
        ExternalServices::noop(),
    );
    let context = ReportContext {
        runtime_manager: &node.runtime_manager,
        adopted: node.casper.adopted_resource_policy.as_ref(),
    };
    let missing_root = prost::bytes::Bytes::from(vec![0xab; 32]);

    let body_only = casper::rust::util::construct_deploy::source_deploy_now(
        "@0!(0)".to_string(),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("a body-only deploy");
    let mut body_only_block = genesis.genesis_block.clone();
    body_only_block.header.parents_hash_list = vec![genesis.genesis_block.block_hash.clone()];
    body_only_block.body.state.pre_state_hash = missing_root.clone();
    body_only_block.body.deploys = vec![ProcessedUserDeploy::Legacy(ProcessedDeploy::empty(
        body_only,
    ))];
    let result = reporter.trace_in(&body_only_block, context).await;
    assert!(
        matches!(&result, Err(message) if message == BODY_ONLY_DEPLOYS_UNSUPPORTED),
        "unexpected body-only result: {:?}",
        result.as_ref().err()
    );

    let mut legacy_genesis = genesis.genesis_block.clone();
    legacy_genesis.body.state.pre_state_hash = missing_root;
    let mut metered = legacy_genesis.body.deploys[0]
        .as_legacy()
        .cloned()
        .expect("a genesis deploy is legacy-format");
    metered.cost = PCost { cost: 200_245 };
    legacy_genesis.body.deploys[0] = ProcessedUserDeploy::Legacy(metered);
    let result = reporter.trace_in(&legacy_genesis, context).await;
    assert!(
        matches!(&result, Err(message) if message == LEGACY_METERED_GENESIS_UNSUPPORTED),
        "unexpected legacy-metered genesis result: {:?}",
        result.as_ref().err()
    );

    drop(node);
}

// Added by DR-134 (DR-70): the helpers below are dev's, copied from the
// disabled precharge_report_shape_spec.rs, which pins the report shape that
// node/src/rust/web/block_info_enricher.rs reads.

/// Reconstruct the transfer unforgeable channel exactly as
/// `node/src/rust/web/transaction::transfer_unforgeable` does, so this test
/// does not depend on the `node` crate.
fn transfer_unforgeable_channel() -> models::rhoapi::Par {
    use casper::rust::genesis::contracts::standard_deploys::{
        to_public, SYSTEM_VAULT_PK, SYSTEM_VAULT_TIMESTAMP,
    };
    let system_vault_pub_key = to_public(SYSTEM_VAULT_PK);
    let mut rng = casper::rust::util::rholang::tools::Tools::unforgeable_name_rng(
        &system_vault_pub_key,
        SYSTEM_VAULT_TIMESTAMP,
    );
    for _ in 0..10 {
        rng.next();
    }
    let unforgeable_bytes = rng.next();
    models::rhoapi::Par {
        unforgeables: vec![models::rhoapi::GUnforgeable {
            unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GPrivateBody(
                models::rhoapi::GPrivate {
                    id: unforgeable_bytes.into_iter().map(|b| b as u8).collect(),
                },
            )),
        }],
        ..Default::default()
    }
}

/// Build the per-batch `SingleReport` list as `BlockReportAPI` does.
fn report_batches(
    events: &[rspace_plus_plus::rspace::reporting_rspace::ReportBatch<
        models::rhoapi::Par,
        models::rhoapi::BindPattern,
        models::rhoapi::ListParWithRandom,
        models::rhoapi::TaggedContinuation,
    >],
) -> Vec<models::casper::SingleReport> {
    use rspace_plus_plus::rspace::reporting_transformer::ReportingTransformer;
    let transformer = casper::rust::reporting_proto_transformer::ReportingProtoTransformer::new();
    events
        .iter()
        .map(|batch| models::casper::SingleReport {
            events: batch
                .events
                .iter()
                .map(|event| transformer.transform_event(event))
                .collect(),
            phase: casper::rust::reporting_proto_transformer::to_proto_phase(batch.phase),
        })
        .collect()
}

/// Find transfers on the transfer channel in a single batch and return
/// `(from_addr, amount)` for each, in the parse order of the node's enricher.
fn transfers_in_batch(
    batch: &models::casper::SingleReport,
    channel: &models::rhoapi::Par,
) -> Vec<(String, i64)> {
    use models::rust::par_ext::ParExt;

    let mut out = Vec::new();
    for event in &batch.events {
        if let Some(models::casper::report_proto::Report::Comm(comm)) = &event.report {
            let on_channel = comm
                .consume
                .as_ref()
                .and_then(|c| c.channels.first())
                .is_some_and(|ch| ch == channel);
            if !on_channel {
                continue;
            }
            if let Some(produce) = comm.produces.first() {
                if let Some(data) = &produce.data {
                    if data.pars.len() >= 6 {
                        let from = data.pars[0].get_g_string();
                        let amount = data.pars[3].get_g_int();
                        if let (Some(from), Some(amount)) = (from, amount) {
                            out.push((from, amount));
                        }
                    }
                }
            }
        }
    }
    out
}

// A vault transfer of 100 from the deployer's own vault to a dummy target.
// Disabled by DR-134 (DR-70): the dummy target "1111…1" is not a valid vault
// address (its checksum fails), so `findOrCreate` answers `(false, _)`, the
// join never fires and the transfer never runs. `vault_transfer_term` below
// transfers to a genesis vault instead.
// const VAULT_TRANSFER_TERM: &str = r#"
//     new
//         rl(`rho:registry:lookup`), SystemVaultCh,
//         deployerId(`rho:system:deployerId`),
//         vaultAddressOps(`rho:vault:address`),
//         vaultAddrCh, vaultCh, targetVaultCh, authKeyCh, ret
//     in {
//         rl!(`rho:vault:system`, *SystemVaultCh) |
//         for (@(_, SystemVault) <- SystemVaultCh) {
//             vaultAddressOps!("fromDeployerId", *deployerId, *vaultAddrCh) |
//             for (@vaultAddr <- vaultAddrCh) {
//                 @SystemVault!("findOrCreate", vaultAddr, *vaultCh) |
//                 @SystemVault!("findOrCreate", "1111111111111111111111111111111111111111111111111111", *targetVaultCh) |
//                 @SystemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
//                 for (@(true, vault) <- vaultCh & @(true, _) <- targetVaultCh & key <- authKeyCh) {
//                     @vault!("transfer", "1111111111111111111111111111111111111111111111111111", 100, *key, *ret)
//                 }
//             }
//         }
//     }
// "#;

/// Added by DR-134 (DR-70): a vault transfer of 100 from the deployer's own
/// vault to the vault at `target`, which exists.
fn vault_transfer_term(target: &str) -> String {
    format!(
        r#"
    new
        rl(`rho:registry:lookup`), SystemVaultCh,
        deployerId(`rho:system:deployerId`),
        vaultAddressOps(`rho:vault:address`),
        vaultAddrCh, vaultCh, targetVaultCh, authKeyCh, ret
    in {{
        rl!(`rho:vault:system`, *SystemVaultCh) |
        for (@(_, SystemVault) <- SystemVaultCh) {{
            vaultAddressOps!("fromDeployerId", *deployerId, *vaultAddrCh) |
            for (@vaultAddr <- vaultAddrCh) {{
                @SystemVault!("findOrCreate", vaultAddr, *vaultCh) |
                @SystemVault!("findOrCreate", "{target}", *targetVaultCh) |
                @SystemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
                for (@(true, vault) <- vaultCh & @(true, _) <- targetVaultCh & key <- authKeyCh) {{
                    @vault!("transfer", "{target}", 100, *key, *ret)
                }}
            }}
        }}
    }}
"#
    )
}

// Added by DR-134 (DR-70): an offered deploy that transfers REV reports the
// transfer in its USER batch, where dev reports the transfer of a user deploy,
// so the node extracts it as on dev. The cost settlement moves REV outside the
// user run, so the report has no PRECHARGE or REFUND batch and no fee transfer.
#[tokio::test]
async fn an_offered_vault_transfer_is_reported_in_the_user_batch() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let mut node = TestNode::standalone(genesis.clone())
        .await
        .expect("Failed to create standalone node");
    // Changed by DR-134 (DR-70): the transfer goes to an existing genesis vault.
    let target_vault =
        rholang::rust::interpreter::util::vault_address::VaultAddress::from_public_key(
            &casper::rust::util::construct_deploy::DEFAULT_PUB2,
        )
        .expect("the second genesis key forms a vault address")
        .to_base58();
    let deploy = construct_deploy::source_deploy_now(
        // VAULT_TRANSFER_TERM.to_string(),
        vault_transfer_term(&target_vault),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("Failed to construct deploy");
    let signed_block = node
        .add_block_from_deploys(&[deploy])
        .await
        .expect("Failed to add block");
    assert!(
        !signed_block.body.deploys[0].is_failed(),
        "premise: the transfer deploy succeeds"
    );

    let mut rspace_kvm = mk_test_rnode_store_manager_shared(genesis.rspace_scope_id.clone());
    let rspace_store = rspace_kvm
        .r_space_stores()
        .await
        .expect("Failed to open shared RSpace stores");
    let reporter = reporting_casper::rho_reporter(
        &rspace_store,
        &node.block_dag_storage,
        node.runtime_manager.replay_lock(),
        ExternalServices::noop(),
    );
    let replay = reporter
        .trace_in(&signed_block, ReportContext {
            runtime_manager: &node.runtime_manager,
            adopted: node.casper.adopted_resource_policy.as_ref(),
        })
        .await
        .expect("ReportingCasper::trace_in failed");

    let batches = report_batches(&replay.deploy_report_result[0].events);
    assert!(
        batches.iter().all(
            |batch| batch.phase != models::casper::ReportPhase::Precharge as i32
                && batch.phase != models::casper::ReportPhase::Refund as i32
        ),
        "an offered deploy has no PRECHARGE or REFUND batch"
    );
    let user: Vec<_> = batches
        .iter()
        .filter(|batch| batch.phase == models::casper::ReportPhase::User as i32)
        .collect();
    assert_eq!(user.len(), 1, "an offered deploy reports one USER batch");
    let deployer_vault =
        rholang::rust::interpreter::util::vault_address::VaultAddress::from_public_key(
            &casper::rust::util::construct_deploy::DEFAULT_PUB,
        )
        .expect("the deployer key forms a vault address")
        .to_base58();
    // The diagnostic lists, for each COMM in the USER batch whose first produce
    // has six or more values, the consume channel and the string and integer
    // values of that produce, so a failure shows where the transfer ran.
    let comm_shapes: Vec<(String, Vec<String>)> = user[0]
        .events
        .iter()
        .filter_map(|event| match &event.report {
            Some(models::casper::report_proto::Report::Comm(comm)) => {
                let data = comm
                    .produces
                    .first()
                    .and_then(|produce| produce.data.as_ref())?;
                (data.pars.len() >= 6).then(|| {
                    use models::rust::par_ext::ParExt;
                    (
                        comm.consume
                            .as_ref()
                            .and_then(|consume| consume.channels.first())
                            .map(|channel| format!("{:?}", channel.unforgeables))
                            .unwrap_or_default(),
                        data.pars
                            .iter()
                            .map(|par| match (par.get_g_string(), par.get_g_int()) {
                                (Some(text), _) => text,
                                (None, Some(number)) => number.to_string(),
                                (None, None) => "_".to_string(),
                            })
                            .collect(),
                    )
                })
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        transfers_in_batch(user[0], &transfer_unforgeable_channel()),
        vec![(deployer_vault, 100)],
        "the USER batch holds exactly the user transfer; transfer channel {:?}; {} events; COMMs (consume channel, produce arity): {:?}",
        transfer_unforgeable_channel(),
        user[0].events.len(),
        comm_shapes
    );
}
