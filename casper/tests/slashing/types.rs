// Local type definitions for the SlashingTestHarness state machine.
//
// These mirror the abstract LTS state from
// docs/theory/slashing/slashing-specification.md §2.2 / §3.1
// (S = (D, I, E, B, A, Sl, C)) and the Rocq formalization at
// formal/rocq/slashing/theories/{Block,EquivocationRecord,DAGState,PoSContract}.v.
//
// The harness operates on these in-memory projections rather than the
// production BlockDagKeyValueStorage so the 54 use-case tests run in
// milliseconds without LMDB setup. The cross-implementation bisim test
// (UC-39) verifies these projections match the production types over
// every harness operation.

use std::collections::{BTreeSet, HashMap, HashSet};

/// Block hash; opaque 32-byte identity in production, indexed by `u64`
/// in the harness for readable assertions.
pub type BlockHash = u64;

/// Validator identity; opaque ByteString in production, mapped to a short
/// string label (e.g. "A", "B", "v0") in the harness.
pub type ValidatorId = String;

/// Sequence number of a block within its sender's chain.
pub type SeqNum = u64;

/// Local projection of a single block's identity-relevant fields. Mirrors
/// `formal/rocq/slashing/theories/Block.v` `Block` record.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockMeta {
    pub hash: BlockHash,
    pub sender: ValidatorId,
    pub seq: SeqNum,
    /// (validator, latest-block-hash) pairs cited by this block.
    pub justifications: Vec<(ValidatorId, BlockHash)>,
}

/// Local projection of the DAG store (`D`) and the invalid-block index
/// (`I`). The DAG is keyed by hash; `invalid` is the subset of hashes
/// marked invalid by validation.
#[derive(Debug, Clone, Default)]
pub struct DagState {
    pub blocks: HashMap<BlockHash, BlockMeta>,
    pub invalid: HashSet<BlockHash>,
}

/// One equivocation record (`EqRec`); mirrors
/// `formal/rocq/slashing/theories/EquivocationRecord.v` `EqRec` record.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EqRecord {
    pub equivocator: ValidatorId,
    pub base_seq: SeqNum,
    pub witnesses: BTreeSet<BlockHash>,
}

/// Local projection of the equivocation tracker (`E`); a set of records
/// keyed conceptually by `(validator, base_seq)`.
#[derive(Debug, Clone, Default)]
pub struct EqRecordSet {
    pub records: HashMap<(ValidatorId, SeqNum), EqRecord>,
}

impl EqRecordSet {
    pub fn contains(&self, validator: &str, base_seq: SeqNum) -> bool {
        self.records.contains_key(&(validator.to_string(), base_seq))
    }

    pub fn witnesses(&self, validator: &str, base_seq: SeqNum) -> BTreeSet<BlockHash> {
        self.records
            .get(&(validator.to_string(), base_seq))
            .map(|r| r.witnesses.clone())
            .unwrap_or_default()
    }

    pub fn insert_or_update(&mut self, record: EqRecord) {
        let key = (record.equivocator.clone(), record.base_seq);
        match self.records.get_mut(&key) {
            Some(existing) => {
                existing.witnesses.extend(record.witnesses);
            }
            None => {
                self.records.insert(key, record);
            }
        }
    }
}

/// Local projection of on-chain PoS state: bonds map (`B`), active set
/// (`A`), slashed set (`Sl`), and Coop-vault balance (`C`). Mirrors
/// `formal/rocq/slashing/theories/PoSContract.v` `PoSState` record.
#[derive(Debug, Clone, Default)]
pub struct PoSState {
    pub bonds: HashMap<ValidatorId, i64>,
    pub active: HashSet<ValidatorId>,
    pub slashed: HashSet<ValidatorId>,
    pub coop_vault: i64,
}

impl PoSState {
    pub fn bond(&self, validator: &str) -> i64 {
        self.bonds.get(validator).copied().unwrap_or(0)
    }

    pub fn is_active(&self, validator: &str) -> bool {
        self.active.contains(validator)
    }
}

/// Detection-status enum exposed by `harness.detect(...)`. Mirrors
/// `casper::rust::block_status::InvalidBlock` projected to the
/// equivocation-relevant variants the test plan uses (other slashable
/// variants are tested via the dispatcher catch-all in UC-28..UC-36).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Valid,
    AdmissibleEquivocation,
    IgnorableEquivocation,
    NeglectedEquivocation,
    JustificationRegression,
    /// Catch-all for the 14 other slashable `InvalidBlock` variants
    /// (UC-28..UC-36 + a few audit-tier cases).
    SlashableOther,
}

/// Outcome of `harness.execute_slash(...)`. Mirrors the
/// `(PoSState, bool)` pair returned by Rocq `PoSContract.slash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashResult {
    pub success: bool,
    pub error: Option<String>,
}
