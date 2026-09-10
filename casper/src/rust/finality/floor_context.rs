//! The per-block-operation derivation context: facts derived from a block's
//! frozen (parents, justifications) pair — the finalized floor, its block's
//! committed post-state, the canonical-disposition walk, and the
//! effect-in-floor-state membership check — computed at most once per
//! operation and read by every consumer.
//!
//! One propose previously derived the floor separately for the merge base
//! and for bonds packaging, and one validate derived it separately for the
//! checkpoint and for the bonds cache; each redundant site was a standing
//! risk of input drift between two derivations of the same fact inside one
//! operation. The disposition walk and the membership check are memoized here
//! so consumers asking the same question of the same frozen inputs share
//! one answer.
//!
//! The floor (and its post-state) is derived eagerly — every operation
//! needs it. The walk and the membership checks are lazy: single-parent
//! operations and empty heartbeat blocks never pay for them.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::FinalizedFloorCommitment;
use models::rust::deploy_id::DeployLookupId;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::floor::{self, Floor};
use crate::rust::errors::CasperError;
use crate::rust::safety::clique_oracle::FtThreshold;

#[derive(Debug, thiserror::Error)]
pub enum CertifiedFloorContextError {
    #[error("invalid certified floor context: {0}")]
    Invalid(String),
    #[error("missing certified floor dependency {0:?}")]
    MissingDependency(BlockHash),
    #[error("certified floor context failed locally: {0}")]
    Local(#[from] CasperError),
}

#[derive(Clone, Copy)]
struct CertifiedFloorBinding<'a> {
    requested_hash: &'a [u8],
    committed_state: &'a [u8],
    stored_hash: &'a [u8],
    stored_block_state: &'a [u8],
    stored_metadata_state: &'a [u8],
    stored_block_number: i64,
    stored_metadata_number: i64,
    accepted: bool,
}

impl CertifiedFloorBinding<'_> {
    fn is_exact(&self) -> bool {
        self.stored_hash == self.requested_hash
            && self.stored_block_state == self.committed_state
            && self.stored_metadata_state == self.committed_state
            && self.stored_metadata_number == self.stored_block_number
            && self.accepted
    }
}

impl CertifiedFloorContextError {
    pub fn into_casper_error(self) -> CasperError {
        match self {
            Self::Invalid(message) => CasperError::RuntimeError(message),
            Self::MissingDependency(hash) => CasperError::BlockNotHeld(hash),
            Self::Local(error) => error,
        }
    }
}

/// Per-sig canonical disposition facts over the operation's parents — the
/// latest disposition, the latest kept rejection record, and the first
/// carrier (see `interpreter_util::SigDisposition`).
pub(crate) type CanonicalDispositions =
    Arc<HashMap<DeployLookupId, crate::rust::util::rholang::interpreter_util::SigDisposition>>;

type CanonicalDispositionSets = Arc<(HashSet<DeployLookupId>, HashSet<DeployLookupId>)>;

/// The retry gate's verdict with its basis. Closed splits into the three
/// distinguishable conditions: the sig has no canonical disposition in the
/// walk window, its latest disposition carries no kept rejection record, or
/// the record exists but its carrier is not settled inside the floor closure
/// (the carrier block is named for diagnostics).
pub enum RetryGateBasis {
    Open,
    NoDisposition,
    NoKeptRejection,
    RecordAboveFloor(BlockHash),
}

pub struct FloorContext {
    pub floor: Floor,
    /// The floor block's committed post-state: the merge base and the
    /// bonds-committee source (`floor::floor_committee`) for both packaging
    /// and validation.
    pub floor_state: StateHash,
    /// The settled candidate set: the chosen floor plus every inherited
    /// parent floor. The positions state monotonicity protects — the
    /// merge-time settled-rejection tripwire checks rejected chains
    /// against exactly this set.
    pub settled_floors: Vec<Floor>,
    parents: Vec<BlockHash>,
    protocol_version: i64,
    /// Disposition walks memoized per scan bound. Bounds are data-dependent
    /// (each consumer derives its own from the deploys it holds), so equal
    /// bounds share one walk and distinct bounds pay their own — no walk's
    /// verdict is ever synthesized from a differently-bounded walk.
    dispositions: parking_lot::Mutex<HashMap<i64, CanonicalDispositions>>,
    disposition_sets: parking_lot::Mutex<HashMap<i64, CanonicalDispositionSets>>,
    /// Effect-in-floor-state membership answers, shared across every
    /// consumer of this operation (the merge's settled-sig dedup and the
    /// buffer purge ask about the same sigs against the same state).
    effect_memo: parking_lot::Mutex<HashMap<DeployLookupId, bool>>,
}

impl FloorContext {
    /// Derive the context for one block operation. `parents` and
    /// `latest_messages` must be the operation's frozen pair (the
    /// snapshot's at propose, the block's own at validate) — never the live
    /// DAG view.
    pub async fn derive(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        parents: &[BlockHash],
        latest_messages: &BTreeMap<Validator, BlockHash>,
        ftt: FtThreshold,
        protocol_version: i64,
    ) -> Result<Self, CasperError> {
        let (floor, settled_floors) =
            floor::finalized_floor_with_candidates(dag, block_store, parents, latest_messages, ftt)
                .await?;
        let floor_block = block_store.get(&floor.hash)?.ok_or_else(|| {
            CasperError::RuntimeError(format!(
                "finalized-floor block {} not in block store",
                PrettyPrinter::build_string_bytes(&floor.hash)
            ))
        })?;
        let floor_state = floor_block.body.state.post_state_hash.clone();
        Ok(Self {
            floor,
            floor_state,
            settled_floors,
            parents: parents.to_vec(),
            protocol_version,
            dispositions: parking_lot::Mutex::new(HashMap::new()),
            disposition_sets: parking_lot::Mutex::new(HashMap::new()),
            effect_memo: parking_lot::Mutex::new(HashMap::new()),
        })
    }

    pub fn from_certified_floor(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        parents: &[BlockHash],
        commitment: &FinalizedFloorCommitment,
        protocol_version: i64,
    ) -> Result<Self, CertifiedFloorContextError> {
        commitment
            .validate_shape()
            .map_err(CertifiedFloorContextError::Invalid)?;
        if parents.is_empty() {
            return Err(CertifiedFloorContextError::Invalid(
                "certified non-genesis floor context has no parent".to_string(),
            ));
        }

        let floor_hash = commitment.floor_hash.clone();
        let floor_block = block_store
            .get(&floor_hash)
            .map_err(|error| CertifiedFloorContextError::Local(error.into()))?
            .ok_or_else(|| CertifiedFloorContextError::MissingDependency(floor_hash.clone()))?;
        let floor_metadata = dag
            .lookup(&floor_hash)
            .map_err(|error| CertifiedFloorContextError::Local(error.into()))?
            .ok_or_else(|| CertifiedFloorContextError::MissingDependency(floor_hash.clone()))?;
        if !(CertifiedFloorBinding {
            requested_hash: &floor_hash,
            committed_state: &commitment.floor_post_state_hash,
            stored_hash: &floor_block.block_hash,
            stored_block_state: &floor_block.body.state.post_state_hash,
            stored_metadata_state: &floor_metadata.post_state_hash,
            stored_block_number: floor_block.body.state.block_number,
            stored_metadata_number: floor_metadata.block_number,
            accepted: floor_metadata.is_accepted(),
        })
        .is_exact()
        {
            return Err(CertifiedFloorContextError::Invalid(
                "certified floor does not bind one accepted stored block state".to_string(),
            ));
        }

        let mut floor_is_causal_input = false;
        for parent in parents {
            if dag
                .lookup(parent)
                .map_err(|error| CertifiedFloorContextError::Local(error.into()))?
                .is_none()
            {
                return Err(CertifiedFloorContextError::MissingDependency(
                    parent.clone(),
                ));
            }
            if dag
                .is_dag_ancestor(&floor_hash, parent)
                .map_err(|error| CertifiedFloorContextError::Local(error.into()))?
            {
                floor_is_causal_input = true;
            }
        }
        if !floor_is_causal_input {
            return Err(CertifiedFloorContextError::Invalid(
                "certified floor is absent from the causal parent frontier".to_string(),
            ));
        }

        let floor = Floor {
            hash: floor_hash,
            block_number: floor_metadata.block_number,
        };
        Ok(Self {
            floor: floor.clone(),
            floor_state: commitment.floor_post_state_hash.clone(),
            settled_floors: vec![floor],
            parents: parents.to_vec(),
            protocol_version,
            dispositions: parking_lot::Mutex::new(HashMap::new()),
            disposition_sets: parking_lot::Mutex::new(HashMap::new()),
            effect_memo: parking_lot::Mutex::new(HashMap::new()),
        })
    }

    /// The floor block's post-state as a history-repository hash (the
    /// merge base state).
    pub fn floor_state_hash(&self) -> Blake2b256Hash {
        Blake2b256Hash::from_bytes_prost(&self.floor_state)
    }

    /// Latest canonical dispositions over the operation's parents at
    /// `earliest_block_number`, one walk per distinct bound.
    fn dispositions(
        &self,
        block_store: &KeyValueBlockStore,
        earliest_block_number: i64,
    ) -> Result<CanonicalDispositions, CasperError> {
        if let Some(cached) = self.dispositions.lock().get(&earliest_block_number) {
            return Ok(cached.clone());
        }
        let walked = Arc::new(
            crate::rust::util::rholang::interpreter_util::canonical_dispositions(
                block_store,
                &self.parents,
                earliest_block_number,
            )?,
        );
        self.dispositions
            .lock()
            .insert(earliest_block_number, walked.clone());
        Ok(walked)
    }

    fn disposition_sets(
        &self,
        block_store: &KeyValueBlockStore,
        earliest_block_number: i64,
    ) -> Result<CanonicalDispositionSets, CasperError> {
        if let Some(cached) = self.disposition_sets.lock().get(&earliest_block_number) {
            return Ok(cached.clone());
        }
        let sets = Arc::new(
            crate::rust::util::rholang::interpreter_util::canonical_disposition_sets_at_floor(
                block_store,
                &self.floor.hash,
                &self.parents,
                earliest_block_number,
                self.protocol_version,
            )?,
        );
        self.disposition_sets
            .lock()
            .insert(earliest_block_number, sets.clone());
        Ok(sets)
    }

    /// Sigs whose latest canonical disposition over the operation's parents
    /// is a WIN — their effect is in the base the proposal builds on.
    pub fn won_sigs(
        &self,
        block_store: &KeyValueBlockStore,
        earliest_block_number: i64,
    ) -> Result<std::collections::HashSet<DeployLookupId>, CasperError> {
        Ok(self
            .disposition_sets(block_store, earliest_block_number)?
            .0
            .clone())
    }

    /// Sigs whose latest canonical disposition over the operation's parents
    /// is a REJECTION — the retry contexts the gate adjudicates.
    pub fn rejected_sigs(
        &self,
        block_store: &KeyValueBlockStore,
        earliest_block_number: i64,
    ) -> Result<std::collections::HashSet<DeployLookupId>, CasperError> {
        Ok(self
            .disposition_sets(block_store, earliest_block_number)?
            .1
            .clone())
    }

    /// Latest kept rejection height per sig, resolved against ONE
    /// dispositions lookup so a retry batch costs a single map walk.
    pub fn latest_kept_rejection_heights<'a>(
        &self,
        block_store: &KeyValueBlockStore,
        earliest_block_number: i64,
        sigs: impl IntoIterator<Item = &'a DeployLookupId>,
    ) -> Result<std::collections::HashMap<DeployLookupId, Option<i64>>, CasperError> {
        let dispositions = self.dispositions(block_store, earliest_block_number)?;
        Ok(sigs
            .into_iter()
            .map(|sig| {
                let height = dispositions.get(sig).and_then(|disposition| {
                    disposition
                        .latest_kept_rejection
                        .as_ref()
                        .map(|(height, _)| *height)
                });
                (sig.clone(), height)
            })
            .collect())
    }

    /// The retry gate — a pure validity predicate over frozen block facts,
    /// so proposer and every validator compute the identical verdict:
    /// re-including a rejected sig is legal iff its LATEST kept rejection
    /// is settled inside this operation's frozen floor closure — the
    /// adjudication is a fact of the block's own base. There is
    /// deliberately NO unsettleable-rejection escape: a record visible in
    /// the parent cone entered it through a merge, every proposal merges
    /// its full frontier (parent selection never narrows), so the floor
    /// passes that merge point within rounds — in-cone records settle
    /// structurally. A stalled floor closes neither the gate's condition
    /// nor the validity window (both are floor-clock), so deferral under
    /// stall is custody, never loss. If a genuinely unsettleable in-cone
    /// rejection ever appears (the old starvation class: recovery re-picks
    /// and defers until the work is destroyed — watch the "deferred by the
    /// retry gate" proposer log), an escape must be derived from an
    /// ON-CHAIN citability bound, never from node-local config.
    ///
    /// This is what sequentializes recovery: a loser cannot be re-proposed
    /// against a live contest — ungated re-proposal regenerated same-sig
    /// sibling copies faster than merges could adjudicate them and
    /// livelocked the shard under sustained contention. A sig with no kept
    /// rejection in the cone is not in a retry context and the gate stays
    /// closed — first inclusions never consult it, and a standing win is
    /// governed by the repeat check. A rejection settled DEEPER than the
    /// walk window (possible while the floor lags the tip by more than the
    /// deploy lifespan) also reads as no-disposition: retries defer through
    /// deep floor lag — delay, never loss, since the floor-clock buffer
    /// retain keeps custody until the floor itself closes the window.
    pub fn retry_gate_open(
        &self,
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        earliest_block_number: i64,
        sig: &DeployLookupId,
    ) -> Result<bool, CasperError> {
        Ok(matches!(
            self.retry_gate_basis(dag, block_store, earliest_block_number, sig)?,
            RetryGateBasis::Open
        ))
    }

    /// The gate's decision with its basis, for per-sig diagnostics: which
    /// of the three closed conditions held, and for an unsettled record,
    /// which block carries it.
    pub fn retry_gate_basis(
        &self,
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        earliest_block_number: i64,
        sig: &DeployLookupId,
    ) -> Result<RetryGateBasis, CasperError> {
        let dispositions = self.dispositions(block_store, earliest_block_number)?;
        let Some(disposition) = dispositions.get(sig) else {
            return Ok(RetryGateBasis::NoDisposition);
        };
        match &disposition.latest_kept_rejection {
            None => Ok(RetryGateBasis::NoKeptRejection),
            Some((_, record_block)) => {
                let settled = *record_block == self.floor.hash
                    || dag
                        .is_dag_ancestor(record_block, &self.floor.hash)
                        .map_err(CasperError::from)?;
                if settled {
                    Ok(RetryGateBasis::Open)
                } else {
                    Ok(RetryGateBasis::RecordAboveFloor(record_block.clone()))
                }
            }
        }
    }

    /// True iff the sig's effect is present in the FLOOR block's committed
    /// post-state, memoized across every check of this operation.
    /// `min_height` bounds the membership walk below (callers with the
    /// deploy pass its `valid_after`; sig-only callers the validity-window
    /// bound — see `deploy_lifecycle::effect_in_state_of`). The memo stays
    /// sig-keyed even though bounds differ per caller: no execution
    /// precedes validity, so every correct lower bound yields one answer.
    pub fn effect_settled_in_floor(
        &self,
        block_store: &KeyValueBlockStore,
        min_height: i64,
        sig: &DeployLookupId,
    ) -> Result<bool, CasperError> {
        if let Some(cached) = self.effect_memo.lock().get(sig) {
            return Ok(*cached);
        }
        let settled = crate::rust::finality::deploy_lifecycle::effect_in_state_of(
            block_store,
            &self.floor.hash,
            sig,
            min_height,
        )?;
        self.effect_memo.lock().insert(sig.clone(), settled);
        Ok(settled)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::CertifiedFloorBinding;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn certified_floor_binding_accepts_only_one_exact_hash_state_and_height(
            hash in any::<[u8; 32]>(),
            state in any::<[u8; 32]>(),
            height in 0i64..i64::MAX,
            hash_index in 0usize..32,
            state_index in 0usize..32,
        ) {
            let exact = CertifiedFloorBinding {
                requested_hash: &hash,
                committed_state: &state,
                stored_hash: &hash,
                stored_block_state: &state,
                stored_metadata_state: &state,
                stored_block_number: height,
                stored_metadata_number: height,
                accepted: true,
            };
            prop_assert!(exact.is_exact());

            let mut different_hash = hash;
            different_hash[hash_index] ^= 1;
            let wrong_hash = CertifiedFloorBinding {
                stored_hash: &different_hash,
                ..exact
            };
            prop_assert!(!wrong_hash.is_exact());

            let mut different_state = state;
            different_state[state_index] ^= 1;
            let wrong_block_state = CertifiedFloorBinding {
                stored_block_state: &different_state,
                ..exact
            };
            prop_assert!(!wrong_block_state.is_exact());
            let wrong_metadata_state = CertifiedFloorBinding {
                stored_metadata_state: &different_state,
                ..exact
            };
            prop_assert!(!wrong_metadata_state.is_exact());

            let wrong_height = CertifiedFloorBinding {
                stored_metadata_number: height.saturating_add(1),
                ..exact
            };
            prop_assert!(!wrong_height.is_exact());
            let wrong_block_height = CertifiedFloorBinding {
                stored_block_number: height.saturating_add(1),
                ..exact
            };
            prop_assert!(!wrong_block_height.is_exact());
            let rejected = CertifiedFloorBinding {
                accepted: false,
                ..exact
            };
            prop_assert!(!rejected.is_exact());
        }
    }
}
