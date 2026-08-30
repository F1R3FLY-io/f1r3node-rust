mod add_block;
mod api;
mod batch1;
mod batch2;
mod blocks;
mod compute_parents_post_state_regression_spec;
mod engine;
mod finalized_floor;
mod fork_choice;
mod genesis;
mod helper;
mod merging;
mod multi_node;
mod multi_sig_pipeline_spec;
mod multi_sig_runtime_fanout_spec;
mod multi_sig_runtime_integration_spec;
mod repeat_deploy;
mod slashing;
mod sync;
mod util;

pub fn legacy_deploy_id(bytes: &[u8]) -> models::rust::deploy_id::DeployLookupId {
    models::rust::deploy_id::DeployLookupId::Legacy(
        models::rust::deploy_id::LegacyDeploySignature::new(bytes.to_vec()),
    )
}

pub fn pending_legacy(
    deploy: crypto::rust::signatures::signed::Signed<
        models::rust::casper::protocol::casper_message::DeployData,
    >,
) -> block_storage::rust::deploy::pending_deploy::PendingDeploy {
    block_storage::rust::deploy::pending_deploy::PendingDeploy::from_legacy(deploy)
        .expect("legacy pending deploy")
}

pub fn legacy_rejected_occurrence(
    deploy_id: impl AsRef<[u8]>,
    source_block_hash: models::rust::block_hash::BlockHash,
    reason: models::rust::casper::protocol::casper_message::RejectedDeployReason,
) -> models::rust::casper::protocol::casper_message::RejectedDeploy {
    models::rust::casper::protocol::casper_message::RejectedDeploy::occurrence_legacy(
        models::rust::deploy_id::LegacyDeploySignature::new(deploy_id.as_ref().to_vec()),
        source_block_hash,
        reason,
    )
}

pub fn init_logger() { shared::rust::tracing_init::init_for_tests(); }
