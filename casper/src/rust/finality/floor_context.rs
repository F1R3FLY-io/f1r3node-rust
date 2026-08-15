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

/// Per-sig latest canonical disposition over the operation's parents:
/// `sig -> (height, won)`.
pub type CanonicalDispositions = Arc<HashMap<Bytes, (i64, bool)>>;

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
    floor_dispositions: parking_lot::Mutex<HashMap<i64, CanonicalDispositions>>,
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
            CasperError::MissingBlock(PrettyPrinter::build_string_bytes(&floor.hash))
        })?;
        let floor_state = floor_block.body.state.post_state_hash.clone();
        Ok(Self {
            floor,
            floor_state,
            parents: parents.to_vec(),
            dispositions: parking_lot::Mutex::new(HashMap::new()),
            floor_dispositions: parking_lot::Mutex::new(HashMap::new()),
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
        let mut cache = self.dispositions.lock();
        if let Some(cached) = cache.get(&earliest_block_number) {
            return Ok(cached.clone());
        }
        let walked = Arc::new(
            crate::rust::util::rholang::interpreter_util::canonical_dispositions(
                block_store,
                &self.parents,
                earliest_block_number,
            )?,
        );
        cache.insert(earliest_block_number, walked.clone());
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
            .filter(|(_, (_, won))| *won)
            .map(|(sig, _)| sig.clone())
            .collect())
    }

    pub fn floor_won_sigs(
        &self,
        block_store: &KeyValueBlockStore,
        earliest_block_number: i64,
    ) -> Result<std::collections::HashSet<Bytes>, CasperError> {
        let mut cache = self.floor_dispositions.lock();
        let dispositions = if let Some(cached) = cache.get(&earliest_block_number) {
            cached.clone()
        } else {
            let walked = Arc::new(
                crate::rust::util::rholang::interpreter_util::canonical_dispositions(
                    block_store,
                    std::slice::from_ref(&self.floor.hash),
                    earliest_block_number,
                )?,
            );
            cache.insert(earliest_block_number, walked.clone());
            walked
        };
        Ok(dispositions
            .iter()
            .filter(|(_, (_, won))| *won)
            .map(|(sig, _)| sig.clone())
            .collect())
    }
}
