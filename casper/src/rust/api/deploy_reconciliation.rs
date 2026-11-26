//! On-demand reconciliation of never-observed deploys (issue #1103).
//!
//! The status resolver ([`super::deploy_finalization_status`]) is a LOOKUP:
//! a sig with no terminal record and no open event row answers `Pending`
//! forever, even after the deploy's validity window has closed and
//! validators reject its resubmission as expired. A caller that still holds
//! the SAVED SIGNED DEPLOY can ask for a reconciliation verdict instead.
//!
//! The verdict rests on the lifecycle register's ingest-first invariant: a
//! sig absent from the register was never in any canonical body this node
//! holds, so it is not on the basis's state lineage and has no failed
//! execution in the basis's DAG closure — both preconditions
//! `beyond_contest_state` needs are covered by register absence, with no
//! lineage walk. Only the contestability bound remains, applied to a
//! record-less row: `Expired` once
//! `basis_height > valid_after + deploy_lifespan + citability_horizon`.
//!
//! Every unverifiable case fails closed to `Pending`:
//! - before the bound (delayed inclusion is still possible, a lagging
//!   observer's basis simply has not passed it yet);
//! - unlimited contestability (`max_parent_depth = i32::MAX` — nothing is
//!   ever provably uncitable);
//! - the validity window may lie at or below this node's restore anchor
//!   (LFS/GC truncation): bodies there were never ingested, so register
//!   absence proves nothing; a carrier can sit as low as
//!   `valid_after + 1`, hence the `valid_after < anchor` guard;
//! - the basis floor itself is not resolvable from the blocks this node
//!   holds.
//!
//! `expirationTimestamp` is verified as part of the signed deploy but never
//! decides the verdict: wall-clock expiry is admission policy, not
//! finalized chain evidence, and the block window closes on its own
//! schedule regardless.

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
    citability_horizon, contestability_bound, verdict_basis_floor,
};
use crate::rust::finality::floor::Floor;
use crate::rust::safety::clique_oracle::FtThreshold;

/// Convenience alias matching `BlockAPI`'s error type.
type ApiErr<T> = eyre::Result<T>;

/// A status answer plus the verdict basis it was judged against.
#[derive(Clone, Debug)]
pub struct ReconciledStatus {
    pub status: DeployFinalizationStatus,
    /// The floor of the adopted last finalized block the verdict was judged
    /// against. `None` on register lookups (nothing was judged) and when
    /// this node could not resolve a floor (a pending verdict carries no
    /// basis — the caller cannot use it as evidence).
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

/// The never-observed verdict against a resolved basis, mirroring
/// `beyond_contest_state` applied to a record-less row: no inclusion
/// events exist, so the contestability bound collapses to the validity
/// window's end plus the citability horizon. Saturating addition keeps a
/// pathological window fail-closed (an overflown bound is never passed).
pub(crate) fn never_observed_state(
    basis_height: i64,
    valid_after: i64,
    deploy_lifespan: i64,
    horizon: Option<i64>,
    effective_anchor: i64,
    coverage_from: Option<i64>,
) -> DeployFinalizationState {
    use DeployFinalizationState::{Expired, Pending};
    // Restore gap: carriers can sit at heights from `valid_after + 1`
    // upward, and bodies at or below the anchor were never ingested, so
    // register absence proves nothing about them.
    let first_carrier = valid_after.saturating_add(1).max(0);
    if (effective_anchor > 0 && valid_after < effective_anchor)
        || coverage_from.is_none_or(|start| first_carrier < start)
    {
        return Pending;
    }
    if contestability_bound(valid_after, deploy_lifespan, std::iter::empty(), horizon)
        .is_some_and(|bound| basis_height > bound)
    {
        Expired
    } else {
        Pending
    }
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

    let horizon = citability_horizon(shard_conf.max_parent_depth);
    let ftt = FtThreshold::from_ppm(shard_conf.fault_tolerance_threshold_ppm);
    let (state, basis) = match verdict_basis_floor(dag, block_store, ftt).await {
        Ok(floor) => {
            let state = never_observed_state(
                floor.block_number,
                deploy.data.valid_after_block_number,
                shard_conf.deploy_lifespan,
                horizon,
                effective_anchor,
                dag.carrier_index_watermark()?,
            );
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
        use DeployFinalizationState::{Expired, Pending};
        let horizon = Some(10);
        // bound = 5 + 30 + 10 = 45
        assert_eq!(
            never_observed_state(45, 5, 30, horizon, 0, Some(0)),
            Pending
        );
        assert_eq!(
            never_observed_state(46, 5, 30, horizon, 0, Some(6)),
            Expired
        );
        assert_eq!(
            never_observed_state(46, 5, 30, horizon, 0, Some(7)),
            Pending
        );
        assert_eq!(
            never_observed_state(46, -1, 30, horizon, 0, Some(0)),
            Expired
        );
    }

    #[test]
    fn unlimited_contestability_never_expires() {
        assert_eq!(
            never_observed_state(i64::MAX, 5, 30, None, 0, Some(0)),
            DeployFinalizationState::Pending
        );
    }

    #[test]
    fn a_window_below_the_restore_anchor_stays_pending() {
        use DeployFinalizationState::{Expired, Pending};
        // Past the bound, window start above the anchor: proven.
        assert_eq!(
            never_observed_state(100, 50, 30, Some(10), 20, Some(0)),
            Expired
        );
        assert_eq!(
            never_observed_state(100, 50, 30, Some(10), 60, Some(0)),
            Pending
        );
        // Boundary: valid_after == anchor means the lowest carrier sits
        // strictly above the anchor, so absence is again a proof.
        assert_eq!(
            never_observed_state(101, 60, 30, Some(10), 60, Some(0)),
            Expired
        );
    }

    #[test]
    fn a_pathological_window_saturates_to_pending() {
        assert_eq!(
            never_observed_state(i64::MAX, i64::MAX, i64::MAX, Some(i64::MAX), 0, Some(0)),
            DeployFinalizationState::Pending
        );
    }
}
