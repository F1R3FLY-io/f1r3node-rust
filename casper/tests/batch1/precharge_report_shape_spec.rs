// Pins the report-shape invariant that
// `node/src/rust/web/block_info_enricher.rs::extract_transfers_from_report`
// relies on. The batch order (precharge -> user -> refund) is a contract of
// `casper/src/rust/rholang/replay_runtime.rs::process_deploy_with_cost_accounting`,
// not a coincidence, so it is asserted here in the crate that produces it
// rather than worked around in the web layer.

use casper::rust::genesis::contracts::standard_deploys::{
    to_public, SYSTEM_VAULT_PK, SYSTEM_VAULT_TIMESTAMP,
};
use casper::rust::reporting_casper;
use casper::rust::reporting_proto_transformer::{to_proto_phase, ReportingProtoTransformer};
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::tools::Tools;
use models::casper::{ReportPhase, ReportProto, SingleReport};
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable};
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::reporting_rspace::ReportBatch;
use rspace_plus_plus::rspace::reporting_transformer::ReportingTransformer;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_test_rnode_store_manager_shared;

/// Reconstruct the transfer unforgeable channel exactly as
/// `node/src/rust/web/transaction::transfer_unforgeable` does, so this test
/// does not depend on the `node` crate.
fn transfer_unforgeable_channel() -> models::rhoapi::Par {
    let system_vault_pub_key = to_public(SYSTEM_VAULT_PK);
    let mut rng = Tools::unforgeable_name_rng(&system_vault_pub_key, SYSTEM_VAULT_TIMESTAMP);
    for _ in 0..10 {
        rng.next();
    }
    let unforgeable_bytes = rng.next();
    models::rhoapi::Par {
        unforgeables: vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: unforgeable_bytes.into_iter().map(|b| b as u8).collect(),
            })),
        }],
        ..Default::default()
    }
}

/// Build the per-batch `SingleReport` list (mirroring
/// `BlockReportAPI::create_deploy_report`) from the reporting replay
/// result, so the same batch notion the web layer consumes is inspected
/// here. Each `ReportBatch` carries its phase marker. The marker is
/// projected onto the proto `SingleReport.phase` field exactly as the
/// production API does.
fn report_batches(
    events: &[ReportBatch<
        models::rhoapi::Par,
        models::rhoapi::BindPattern,
        models::rhoapi::ListParWithRandom,
        models::rhoapi::TaggedContinuation,
    >],
) -> Vec<SingleReport> {
    let transformer = ReportingProtoTransformer::new();
    events
        .iter()
        .map(|batch| {
            let proto_events: Vec<ReportProto> = batch
                .events
                .iter()
                .map(|ev| transformer.transform_event(ev))
                .collect();
            SingleReport {
                events: proto_events,
                phase: to_proto_phase(batch.phase),
            }
        })
        .collect()
}

/// Find transfers on the transfer channel in a single batch and return
/// `(from_addr, amount)` for each. Replicates the parse order of
/// `block_info_enricher::find_transfers_in_report` without taking a
/// dependency on the `node` crate.
fn transfers_in_batch(batch: &SingleReport, channel: &models::rhoapi::Par) -> Vec<(String, i64)> {
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

/// A RevVault transfer from the deployer's own vault to a dummy target vault.
/// Signed by DEFAULT_SEC, which holds a genesis REV vault with 9M balance, so
/// the precharge (phlo_limit * phlo_price) and the user transfer both emit on
/// the transfer channel under real cost accounting.
const VAULT_TRANSFER_TERM: &str = r#"
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

/// phlo_limit * phlo_price must match the precharge amount emitted by
/// `PreChargeDeploy { charge_amount: total_phlo_charge(), .. }`. Keep these in
/// sync with the assertion below.
const PHLO_LIMIT: i64 = 5_000_000;
const PHLO_PRICE: i64 = 1;

/// Cost-accounted user deploy: the reporting replay yields >= 2 batches, and
/// `report[0]` is the precharge — exactly one transfer on the transfer
/// channel, from the deployer's vault address, for `total_phlo_charge()`.
#[tokio::test]
async fn cost_accounted_user_deploy_report_starts_with_precharge_batch() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut node = TestNode::standalone(genesis.clone())
        .await
        .expect("Failed to create standalone node");

    let shard_id = genesis.genesis_block.shard_id.clone();
    let deploy = construct_deploy::source_deploy(
        VAULT_TRANSFER_TERM.to_string(),
        1,
        Some(PHLO_LIMIT),
        Some(PHLO_PRICE),
        Some(construct_deploy::DEFAULT_SEC.clone()),
        None,
        Some(shard_id.clone()),
    )
    .expect("Failed to construct deploy");

    let signed_block = node
        .add_block_from_deploys(&[deploy])
        .await
        .expect("Failed to add block with vault transfer");

    // Open the shared RSpace stores on the genesis scope and replay the block
    // through the reporting runtime, exactly as the node's BlockReportAPI does.
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
        .trace(&signed_block)
        .await
        .expect("reporting replay of the user block failed");

    assert_eq!(
        replay.deploy_report_result.len(),
        1,
        "the block carries one user deploy"
    );

    let deploy_result = &replay.deploy_report_result[0];
    let deploy = &deploy_result.processed_deploy.deploy;
    let expected_precharge = deploy.data.total_phlo_charge();
    assert_eq!(
        expected_precharge,
        PHLO_LIMIT * PHLO_PRICE,
        "total_phlo_charge must match the configured phlo_limit * phlo_price"
    );

    let batches = report_batches(&deploy_result.events);
    assert!(
        batches.len() >= 2,
        "cost-accounted deploy must yield at least [precharge, user] batches, got {}",
        batches.len()
    );

    // Marker assertions: the reporting rspace tags each segment at the
    // phase boundary. The report must contain segments tagged
    // PRECHARGE, USER, and REFUND. The pre-deploy setup events (reset,
    // bootstrap) are flushed as an Unspecified segment before the first
    // phase boundary, so the precharge is not necessarily batches[0].
    let phases: Vec<i32> = batches.iter().map(|b| b.phase).collect();
    assert!(
        phases.contains(&(ReportPhase::Precharge as i32)),
        "the report must contain a PRECHARGE-tagged segment; got phases {:?}",
        phases
    );
    assert!(
        phases.contains(&(ReportPhase::User as i32)),
        "the report must contain a USER-tagged segment; got phases {:?}",
        phases
    );
    assert!(
        phases.contains(&(ReportPhase::Refund as i32)),
        "the report must contain a REFUND-tagged segment; got phases {:?}",
        phases
    );

    let transfer_channel = transfer_unforgeable_channel();
    let expected_deployer_vault = VaultAddress::from_public_key(&construct_deploy::DEFAULT_PUB)
        .expect("DEFAULT_PUB must derive a vault address")
        .to_base58();

    // Find the precharge batch by marker, not by position. The marker is
    // authoritative; the pre-deploy Unspecified segment may precede it.
    let precharge_batch = batches
        .iter()
        .find(|b| b.phase == ReportPhase::Precharge as i32)
        .expect("a PRECHARGE-tagged segment must exist");
    let precharge_transfers = transfers_in_batch(precharge_batch, &transfer_channel);

    // The precharge segment holds exactly one transfer on the transfer
    // channel, from the deployer's vault, for total_phlo_charge(). Both
    // the invariant and the marker are pinned so a regression in either
    // is caught.
    assert_eq!(
        precharge_transfers.len(),
        1,
        "the PRECHARGE segment must hold exactly one transfer on the transfer channel"
    );
    assert_eq!(
        precharge_transfers[0].0, expected_deployer_vault,
        "precharge sender must be the deployer's vault address"
    );
    assert_eq!(
        precharge_transfers[0].1, expected_precharge,
        "precharge amount must equal total_phlo_charge()"
    );
}

/// Genesis block: deploys are the blessed standard contracts built by
/// `standard_deploys::to_deploy` with `phlo_price = 0`, so
/// `total_phlo_charge()` = 0.
#[tokio::test]
async fn genesis_deploys_carry_zero_phlo_price_so_no_precharge() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let deploys = &genesis.genesis_block.body.deploys;
    assert!(
        !deploys.is_empty(),
        "genesis block must carry the blessed standard deploys"
    );

    for (i, processed) in deploys.iter().enumerate() {
        assert_eq!(
            processed.deploy.data.phlo_price, 0,
            "genesis deploy {} must carry phlo_price == 0 (standard_deploys::to_deploy)",
            i
        );
        assert_eq!(
            processed.deploy.data.total_phlo_charge(),
            0,
            "genesis deploy {} must have zero precharge amount",
            i
        );
    }
}

/// Genesis replays run with `with_cost_accounting == false` (no parents),
/// so `process_deploy_without_cost_accounting` runs and the only phase
/// marker set is `USER`. Every deploy's report must therefore contain no
/// `PRECHARGE` or `REFUND` segment. Segments may be `USER` or
/// `UNSPECIFIED`.
#[tokio::test]
async fn genesis_report_carries_only_user_markers() {
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
        .trace(&genesis.genesis_block)
        .await
        .expect("reporting replay of the genesis block failed");

    assert!(
        !replay.deploy_report_result.is_empty(),
        "genesis block must replay at least one deploy"
    );

    for (i, deploy_result) in replay.deploy_report_result.iter().enumerate() {
        let batches = report_batches(&deploy_result.events);
        for (j, batch) in batches.iter().enumerate() {
            assert!(
                batch.phase == ReportPhase::User as i32
                    || batch.phase == ReportPhase::Unspecified as i32,
                "genesis deploy {} batch {} must carry USER or UNSPECIFIED, got phase {}",
                i,
                j,
                batch.phase
            );
            assert_ne!(
                batch.phase,
                ReportPhase::Precharge as i32,
                "genesis must never produce a PRECHARGE segment"
            );
            assert_ne!(
                batch.phase,
                ReportPhase::Refund as i32,
                "genesis must never produce a REFUND segment"
            );
        }
    }

    // The node is held until here to keep the DAG storage alive for the
    // reporting replay above.
    drop(node);
}
