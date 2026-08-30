use std::collections::{HashMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::deploy_lifecycle_types::{LifecycleEvents, TerminalState};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::deploy_id::DeployLookupId;
use prost::bytes::Bytes;

/// Convenience alias matching `BlockAPI`'s error type.
type ApiErr<T> = eyre::Result<T>;

/// Sentinel error for the known-block inconsistency case (a caller
/// claims a sig lives in a block whose body does not list it). Typed so
/// `BlockAPI::deploy_finalization_status` can downcast at the HTTP/gRPC
/// boundary and convert to `pending_unknown` — callers see a tractable
/// response instead of a 500 — while genuine I/O failures keep
/// propagating as errors.
#[derive(Debug)]
pub struct DeployFinalizationCorruption {
    pub sig: Bytes,
    pub block_hash: BlockHash,
}

impl std::fmt::Display for DeployFinalizationCorruption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "deploy_finalization_status: sig {} indexed at block {} \
             but missing from that block's body.deploys",
            hex::encode(&self.sig),
            PrettyPrinter::build_string_bytes(&self.block_hash),
        )
    }
}

impl std::error::Error for DeployFinalizationCorruption {}

/// Terminal or transitional state of a deploy as observed from the local DAG.
///
/// Clients poll `deploy_finalization_status` by deploy signature to learn
/// whether a deploy has canonically landed. Block-hash polling is insufficient
/// because a block can finalize while the effects of some of its deploys
/// were dropped during merge — `Finalized` here means the effects are in
/// canonical state, not merely that some block containing the sig finalized.
///
/// The resolver is a LOOKUP over the deploy-lifecycle register
/// (`finality::deploy_lifecycle`): terminal verdicts are determined once,
/// at the threshold crossing that makes them true, and persisted
/// write-once in the DAG storage's lifecycle tables. Pending is DEFINED as
/// "no terminal record exists" — this module computes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeployFinalizationState {
    /// The sig's standing clean win is inside the max frozen
    /// latest-message floor's closure and beyond the adjudication
    /// horizon. The effects are in every future merge base and no
    /// still-formable block can adjudicate the carrier. Terminal.
    Finalized,
    /// The validity window closed on the floor clock without a winning
    /// inclusion, and a floor-covered inclusion executed with
    /// `is_failed=true` (e.g., insufficient phlo, contract error).
    /// Effects will never apply. Terminal.
    Failed,
    /// Not yet settled: no terminal record has been determined. Covers
    /// unincluded deploys, wins still climbing toward floor coverage or
    /// the horizon, and contested histories still inside their validity
    /// window. Client should keep polling.
    Pending,
    /// The validity window closed without a winning inclusion or a failed
    /// execution. The deploy can never land. Terminal.
    Expired,
}

/// Full response payload for a deploy-finalization-status query.
#[derive(Clone, Debug)]
pub struct DeployFinalizationStatus {
    pub state: DeployFinalizationState,
    /// Number of canonical blocks in which the sig appears in
    /// `body.rejected_deploys` (duplicate-flagged records included — the
    /// count is observability, not the causal ordering). Gives operators
    /// visibility into deploys that are contending.
    pub rejection_count: u32,
    /// Hash of the highest-block-number surviving block that contains
    /// the sig in `body.deploys`.
    /// `None` when the sig has not yet been included in any block.
    pub latest_block_hash: Option<BlockHash>,
}

impl DeployFinalizationStatus {
    pub fn pending_unknown() -> Self {
        Self {
            state: DeployFinalizationState::Pending,
            rejection_count: 0,
            latest_block_hash: None,
        }
    }
}

fn terminal_state(state: TerminalState) -> DeployFinalizationState {
    match state {
        TerminalState::Finalized => DeployFinalizationState::Finalized,
        TerminalState::Expired => DeployFinalizationState::Expired,
        TerminalState::Failed => DeployFinalizationState::Failed,
    }
}

fn pending_from_row(row: &LifecycleEvents) -> DeployFinalizationStatus {
    let (rejection_count, _, latest_block_hash) =
        crate::rust::finality::deploy_lifecycle::lifecycle_display(row);
    let latest_block_hash = (!latest_block_hash.is_empty()).then(|| Bytes::from(latest_block_hash));
    DeployFinalizationStatus {
        state: DeployFinalizationState::Pending,
        rejection_count,
        latest_block_hash,
    }
}

/// Consistency check for the fallback paths: the sig must actually appear
/// in the claimed block's body. An indexed-but-missing sig is returned as
/// the typed `DeployFinalizationCorruption` sentinel.
fn checked_block_membership(
    block_store: &KeyValueBlockStore,
    deploy_id: &DeployLookupId,
    block_hash: &BlockHash,
) -> ApiErr<Option<DeployFinalizationStatus>> {
    let sig_bytes: Bytes = Bytes::copy_from_slice(deploy_id.as_bytes());
    let block = match block_store.get(block_hash) {
        Ok(Some(b)) => b,
        Ok(None) => {
            tracing::warn!(
                target: "f1r3fly.casper.deploy_finalization.validation",
                "sig {} indexed at block {} but block body absent from store",
                hex::encode(&sig_bytes),
                PrettyPrinter::build_string_bytes(block_hash)
            );
            return Ok(None);
        }
        Err(e) => {
            return Err(eyre::eyre!(
                "block_store.get failed for first-seen block {}: {}",
                PrettyPrinter::build_string_bytes(block_hash),
                e
            ));
        }
    };
    let in_body = block
        .body
        .deploys
        .iter()
        .any(|pd| pd.deploy_id_for_protocol(block.header.version).as_ref() == Ok(deploy_id))
        || block
            .body
            .rejected_deploys
            .iter()
            .any(|rd| rd.typed_deploy_id() == deploy_id);
    if !in_body {
        tracing::warn!(
            target: "f1r3fly.casper.deploy_finalization.validation",
            "sig {} claimed at block {} but missing from that block's body",
            hex::encode(&sig_bytes),
            PrettyPrinter::build_string_bytes(block_hash),
        );
        return Err(eyre::Report::new(DeployFinalizationCorruption {
            sig: sig_bytes,
            block_hash: block_hash.clone(),
        }));
    }
    Ok(Some(DeployFinalizationStatus {
        state: DeployFinalizationState::Pending,
        rejection_count: 0,
        latest_block_hash: Some(block_hash.clone()),
    }))
}

/// Resolve a deploy's finalization status: a LOOKUP over the lifecycle
/// register, in precedence order —
///
/// 1. A terminal record answers directly with its frozen fields
///    (Finalized / Expired / Failed can never flip — write-once).
/// 2. An open event row answers Pending with its display fields.
/// 3. Neither: the sig was never in any canonical body this node holds
///    (the register ingests every insert path from genesis). A
///    caller-provided block hash supplies a checked Pending answer, with
///    the indexed-but-missing corruption sentinel preserved; otherwise
///    `pending_unknown`.
///
/// The resolver is an API/observability surface (deploy status reporting);
/// consensus validation (`repeat_deploy`) deliberately does NOT read it.
pub fn resolve(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_id: &DeployLookupId,
    known_block_hash: Option<&BlockHash>,
) -> ApiErr<DeployFinalizationStatus> {
    let status = resolve_lookup(dag, block_store, deploy_id, known_block_hash)?;
    tracing::info!(
        target: "f1r3fly.casper.deploy_lifecycle",
        event = "status_resolved",
        deploy_sig = %hex::encode(deploy_id.as_bytes()),
        resolved_state = ?status.state,
        rejection_count = status.rejection_count,
        latest_block = ?status.latest_block_hash.as_ref().map(hex::encode),
        "deploy lifecycle"
    );
    Ok(status)
}

pub fn resolve_batch(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_ids: &HashSet<DeployLookupId>,
) -> ApiErr<HashMap<DeployLookupId, DeployFinalizationStatus>> {
    deploy_ids
        .iter()
        .map(|deploy_id| {
            resolve(dag, block_store, deploy_id, None).map(|status| (deploy_id.clone(), status))
        })
        .collect()
}

fn resolve_lookup(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_id: &DeployLookupId,
    known_block_hash: Option<&BlockHash>,
) -> ApiErr<DeployFinalizationStatus> {
    if let Some(record) = dag
        .deploy_terminal(deploy_id)
        .map_err(|e| eyre::eyre!("deploy lifecycle terminal lookup failed: {}", e))?
    {
        let latest_block_hash = if record.latest_block_hash.is_empty() {
            None
        } else {
            Some(Bytes::from(record.latest_block_hash))
        };
        return Ok(DeployFinalizationStatus {
            state: terminal_state(record.state),
            rejection_count: record.rejection_count,
            latest_block_hash,
        });
    }

    if let Some(row) = dag
        .deploy_lifecycle_events(deploy_id)
        .map_err(|e| eyre::eyre!("deploy lifecycle events lookup failed: {}", e))?
    {
        return Ok(pending_from_row(&row));
    }

    // The register ingests every insert path from genesis, so a sig with
    // neither a terminal record nor an open row was never in any canonical
    // body this node holds. A caller-provided block hash still gets a
    // checked Pending answer (with the indexed-but-missing corruption
    // sentinel preserved for the consistency case).
    match known_block_hash {
        Some(block_hash) => match checked_block_membership(block_store, deploy_id, block_hash)? {
            Some(status) => Ok(status),
            None => Ok(DeployFinalizationStatus::pending_unknown()),
        },
        None => Ok(DeployFinalizationStatus::pending_unknown()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_unknown_has_empty_fields() {
        let s = DeployFinalizationStatus::pending_unknown();
        assert_eq!(s.state, DeployFinalizationState::Pending);
        assert_eq!(s.rejection_count, 0);
        assert!(s.latest_block_hash.is_none());
    }

    #[test]
    fn states_are_distinct() {
        let all = [
            DeployFinalizationState::Finalized,
            DeployFinalizationState::Failed,
            DeployFinalizationState::Pending,
            DeployFinalizationState::Expired,
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                assert_eq!(
                    a == b,
                    i == j,
                    "state equality mismatch: {:?} vs {:?}",
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn pending_display_uses_surviving_inclusion_and_distinct_recording_blocks() {
        use block_storage::rust::dag::deploy_lifecycle_types::{
            LifecycleEvent, LifecycleEventKind,
        };

        let surviving = vec![0x10u8; 32];
        let rejected = vec![0xf0u8; 32];
        let recording_block = vec![0x20u8; 32];
        let row = LifecycleEvents {
            valid_after: Some(1),
            events: vec![
                LifecycleEvent {
                    height: 10,
                    block_hash: surviving.clone(),
                    kind: LifecycleEventKind::Included { is_failed: false },
                },
                LifecycleEvent {
                    height: 10,
                    block_hash: rejected.clone(),
                    kind: LifecycleEventKind::Included { is_failed: false },
                },
                LifecycleEvent {
                    height: 11,
                    block_hash: recording_block.clone(),
                    kind: LifecycleEventKind::Rejected {
                        duplicate: false,
                        carrier: rejected,
                    },
                },
                LifecycleEvent {
                    height: 11,
                    block_hash: recording_block,
                    kind: LifecycleEventKind::Rejected {
                        duplicate: true,
                        carrier: vec![0xe0u8; 32],
                    },
                },
            ],
        };

        let status = pending_from_row(&row);
        assert_eq!(status.rejection_count, 1);
        assert_eq!(status.latest_block_hash, Some(Bytes::from(surviving)));
    }
}
