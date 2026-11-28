//! On-demand reconciliation of never-observed deploys (issue #1103).
//!
//! The status resolver ([`super::deploy_finalization_status`]) is a LOOKUP:
//! a sig with no terminal record and no open event row answers `Pending`
//! forever, even after the deploy's validity window has closed and
//! validators reject its resubmission as expired. A caller that still holds
//! the SAVED SIGNED DEPLOY can ask for a reconciliation verdict instead.
//!
//! Reconciliation requires a resolved basis past the validity window, the
//! merge-floor distance allowance, and the citability horizon. It requires
//! carrier-index coverage from the first possible carrier height. A held
//! carrier keeps the verdict `Pending` at every height, including when it
//! awaits equivocation validation. Missing history also keeps it `Pending`.
//!
//! Admission and block validation use `expirationTimestamp`.
//! Reconciliation uses the finalized block heights.

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::DeployData;

use super::deploy_finalization_status::{
    lookup_register, DeployFinalizationState, DeployFinalizationStatus,
};
use crate::rust::casper::CasperShardConf;
use crate::rust::errors::CasperError;
use crate::rust::finality::deploy_lifecycle::{
    contestability_bound, reconciliation_horizon, verdict_basis_floor,
};
use crate::rust::finality::floor::Floor;
use crate::rust::safety::clique_oracle::FtThreshold;

/// Convenience alias matching `BlockAPI`'s error type.
type ApiErr<T> = eyre::Result<T>;

/// A status answer plus the verdict basis it was judged against.
#[derive(Clone, Debug)]
pub struct ReconciledStatus {
    pub status: DeployFinalizationStatus,
    /// The floor of the adopted last finalized block used for reconciliation.
    /// Register answers and queries with an unresolved floor have no basis.
    pub basis: Option<Floor>,
}

impl ReconciledStatus {
    /// The register-derived answer, carrying no basis.
    pub fn from_lookup(status: DeployFinalizationStatus) -> Self {
        Self {
            status,
            basis: None,
        }
    }
}

/// The first possible carrier height once the resolved basis passes the
/// reconciliation bound and the window lies above the restore anchor.
fn carrier_scan_start(
    basis_height: i64,
    valid_after: i64,
    deploy_lifespan: i64,
    horizon: Option<i64>,
    effective_anchor: i64,
) -> Option<i64> {
    if (effective_anchor > 0 && valid_after < effective_anchor) || deploy_lifespan < 0 {
        return None;
    }
    let bound = contestability_bound(valid_after, deploy_lifespan, std::iter::empty(), horizon)?;
    (basis_height > bound).then_some(valid_after.checked_add(1)?.max(0))
}

fn carrier_history_proves_absence(
    dag: &KeyValueDagRepresentation,
    sig: &[u8],
    first_carrier: i64,
) -> ApiErr<bool> {
    let covers = |cutoff: Option<i64>| cutoff.is_none_or(|cutoff| cutoff <= first_carrier);
    Ok(dag
        .carrier_index_watermark()?
        .is_some_and(|start| start <= first_carrier)
        && covers(dag.carrier_index_prune_cutoff()?)
        && dag.carrier_index_proves_absence(sig)?
        && covers(dag.carrier_index_prune_cutoff()?))
}

/// Resolve a sig's status, reconciling a never-observed sig from the saved
/// signed deploy. Lookup-first, in precedence order:
///
/// 1. A register record (terminal or open) answers as-is — a reconciliation
///    never overrides the register, covering delayed inclusion and
///    re-application the register already observed.
/// 2. Neither register table names the sig: the never-observed verdict
///    above, against the floor of the adopted last finalized block.
///
/// The deploy's signature is NOT verified here — the decode paths
/// (`DeployData::from_proto`, `DeployRequest::to_signed`) verify before
/// this is reachable.
pub async fn reconcile(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    shard_conf: &CasperShardConf,
    effective_anchor: i64,
    deploy: &Signed<DeployData>,
) -> ApiErr<ReconciledStatus> {
    let sig = deploy.sig.as_ref();
    if let Some(status) = lookup_register(dag, sig)? {
        return Ok(ReconciledStatus::from_lookup(status));
    }

    let horizon = reconciliation_horizon(shard_conf.max_parent_depth);
    let ftt = FtThreshold::from_ppm(shard_conf.fault_tolerance_threshold_ppm);
    let (state, basis) = match verdict_basis_floor(dag, block_store, ftt).await {
        Ok(floor) => {
            let scan_start = carrier_scan_start(
                floor.block_number,
                deploy.data.valid_after_block_number,
                shard_conf.deploy_lifespan,
                horizon,
                effective_anchor,
            );
            let state = match scan_start {
                Some(start) if carrier_history_proves_absence(dag, sig, start)? => {
                    if let Some(status) = lookup_register(dag, sig)? {
                        return Ok(ReconciledStatus::from_lookup(status));
                    }
                    DeployFinalizationState::Expired
                }
                _ => DeployFinalizationState::Pending,
            };
            (state, Some(floor))
        }
        Err(CasperError::BlockNotHeld(missing, _)) => {
            tracing::warn!(
                target: "f1r3fly.casper.deploy_reconciliation",
                missing = %PrettyPrinter::build_string_bytes(&missing),
                "reconciliation basis below the blocks this node holds; the verdict stays Pending"
            );
            (DeployFinalizationState::Pending, None)
        }
        Err(e) => return Err(eyre::Report::new(e)),
    };
    let status = DeployFinalizationStatus {
        state,
        rejection_count: 0,
        latest_block_hash: None,
    };
    tracing::info!(
        target: "f1r3fly.casper.deploy_reconciliation",
        event = "reconciliation_resolved",
        deploy_sig = %hex::encode(sig),
        resolved_state = ?state,
        basis_height = basis.as_ref().map(|f| f.block_number),
        "deploy reconciliation"
    );
    Ok(ReconciledStatus { status, basis })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bound_is_closed_at_basis_and_open_past_it() {
        let horizon = Some(10);
        assert_eq!(carrier_scan_start(45, 5, 30, horizon, 0), None);
        assert_eq!(carrier_scan_start(46, 5, 30, horizon, 0), Some(6));
        assert_eq!(carrier_scan_start(46, -1, 30, horizon, 0), Some(0));
    }

    #[test]
    fn unlimited_contestability_never_expires() {
        assert_eq!(carrier_scan_start(i64::MAX, 5, 30, None, 0), None);
    }

    #[test]
    fn a_window_below_the_restore_anchor_stays_pending() {
        assert_eq!(carrier_scan_start(100, 50, 30, Some(10), 20), Some(51));
        assert_eq!(carrier_scan_start(100, 50, 30, Some(10), 60), None);
        assert_eq!(carrier_scan_start(101, 60, 30, Some(10), 60), Some(61));
    }

    #[test]
    fn a_pathological_window_saturates_to_pending() {
        assert_eq!(
            carrier_scan_start(i64::MAX, i64::MAX - 1, 1, Some(10), 0),
            None
        );
    }
}
