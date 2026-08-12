//! The per-block-operation derivation context: facts derived from a block's
//! frozen (parents, justifications) pair — the finalized floor, its block's
//! committed post-state, the canonical-disposition walk, and the
//! effect-in-floor-state probe — computed at most once per operation and
//! read by every consumer.
//!
//! One propose previously derived the floor separately for the merge base
//! and for bonds packaging, and one validate derived it separately for the
//! checkpoint and for the bonds cache; each redundant site was a standing
//! risk of input drift between two derivations of the same fact inside one
//! operation. The disposition walk and the effect probe are memoized here
//! so consumers asking the same question of the same frozen inputs share
//! one answer.
//!
//! The floor (and its post-state) is derived eagerly — every operation
//! needs it. The walk and the probes are lazy: single-parent operations and
//! empty heartbeat blocks never pay for them.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::floor::{self, Floor};
use crate::rust::errors::CasperError;
use crate::rust::safety::clique_oracle::FtThreshold;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// Per-sig canonical disposition facts over the operation's parents — the
/// latest disposition, the latest kept rejection record, and the first
/// carrier (see `interpreter_util::SigDisposition`).
pub(crate) type CanonicalDispositions =
    Arc<HashMap<Bytes, crate::rust::util::rholang::interpreter_util::SigDisposition>>;

pub struct FloorContext {
    pub floor: Floor,
    /// The floor block's committed post-state: the merge base and the
    /// bonds-committee source (`floor::floor_committee`) for both packaging
    /// and validation.
    pub floor_state: StateHash,
    parents: Vec<BlockHash>,
    /// Disposition walks memoized per scan bound. Bounds are data-dependent
    /// (each consumer derives its own from the deploys it holds), so equal
    /// bounds share one walk and distinct bounds pay their own — no walk's
    /// verdict is ever synthesized from a differently-bounded walk.
    dispositions: parking_lot::Mutex<HashMap<i64, CanonicalDispositions>>,
    /// Effect-in-floor-state probe results, shared across every consumer of
    /// this operation (the merge's settled-sig dedup and the buffer purge
    /// ask about the same sigs against the same state).
    effect_memo: parking_lot::Mutex<HashMap<Bytes, bool>>,
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
    ) -> Result<Self, CasperError> {
        let floor = floor::finalized_floor(dag, parents, latest_messages, ftt).await?;
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
            parents: parents.to_vec(),
            dispositions: parking_lot::Mutex::new(HashMap::new()),
            effect_memo: parking_lot::Mutex::new(HashMap::new()),
        })
    }

    /// The floor block's post-state as a history-repository hash (the merge
    /// base and the effect probe's target).
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

    /// Sigs whose latest canonical disposition over the operation's parents
    /// is a WIN — their effect is in the base the proposal builds on.
    pub fn won_sigs(
        &self,
        block_store: &KeyValueBlockStore,
        earliest_block_number: i64,
    ) -> Result<std::collections::HashSet<Bytes>, CasperError> {
        Ok(self
            .dispositions(block_store, earliest_block_number)?
            .iter()
            .filter(|(_, disposition)| disposition.won())
            .map(|(sig, _)| sig.clone())
            .collect())
    }

    /// Sigs whose latest canonical disposition over the operation's parents
    /// is a REJECTION — the retry contexts the gate adjudicates.
    pub fn rejected_sigs(
        &self,
        block_store: &KeyValueBlockStore,
        earliest_block_number: i64,
    ) -> Result<std::collections::HashSet<Bytes>, CasperError> {
        Ok(self
            .dispositions(block_store, earliest_block_number)?
            .iter()
            .filter(|(_, disposition)| !disposition.won())
            .map(|(sig, _)| sig.clone())
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
        sig: &Bytes,
    ) -> Result<bool, CasperError> {
        let dispositions = self.dispositions(block_store, earliest_block_number)?;
        let Some(disposition) = dispositions.get(sig) else {
            return Ok(false);
        };
        match &disposition.latest_kept_rejection {
            None => Ok(false),
            Some((_, record_block)) => Ok(*record_block == self.floor.hash
                || dag
                    .is_dag_ancestor(record_block, &self.floor.hash)
                    .map_err(CasperError::KvStoreError)?),
        }
    }

    /// True iff the sig's effect is present in the FLOOR block's committed
    /// post-state, memoized across every probe of this operation.
    pub fn effect_settled_in_floor(
        &self,
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        runtime_manager: &RuntimeManager,
        sig: &Bytes,
    ) -> Result<bool, CasperError> {
        if let Some(cached) = self.effect_memo.lock().get(sig) {
            return Ok(*cached);
        }
        let settled = crate::rust::util::rholang::interpreter_util::deploy_effect_in_state(
            dag,
            block_store,
            runtime_manager,
            &self.floor_state_hash(),
            sig,
        )?;
        self.effect_memo.lock().insert(sig.clone(), settled);
        Ok(settled)
    }
}
