use prost::bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::rust::block::state_hash::StateHashSerde;
use crate::rust::block_hash::BlockHashSerde;

/// One seal-rejection verdict: the chain hosting `sig` in block `host` was
/// rejected when its cone was sealed. Keyed per INCLUSION, not per sig — a
/// deploy re-included by recovery gets a fresh verdict for the new host; the
/// old entry keeps damning only the dead copy.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize
)]
pub struct SealedRejection {
    #[serde(with = "shared::rust::serde_bytes")]
    pub sig: Bytes,
    pub host: BlockHashSerde,
    /// Block number of `host`, for retention pruning of the cumulative ledger.
    pub host_number: i64,
}

/// One seal-acceptance record: the chain hosting `sig` in block `host` was
/// KEPT when its cone was sealed, so the deploy's effect is in `FS`. Keyed
/// per INCLUSION (like [`SealedRejection`]) so retention windowing can prune
/// by host height. The exactly-once authority is "any accepted inclusion
/// wins": once a sig has any accepted record it is finalized-applied and must
/// never be re-executed, even if a LATER re-inclusion was rejected.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize
)]
pub struct SealedAcceptance {
    #[serde(with = "shared::rust::serde_bytes")]
    pub sig: Bytes,
    pub host: BlockHashSerde,
    /// Block number of `host`, for retention pruning of the cumulative ledger.
    pub host_number: i64,
}

/// The sealed finalized state at one floor cut, keyed (in storage) by the
/// floor block hash.
///
/// A floor is a justification-derived finalized cut: `floor(B)` is the highest
/// ancestor certified finalized by the clique oracle over block B's frozen
/// justification snapshot. The sealed state for a floor F is produced by ONE
/// canonical recursion — merging `closure(F) \ closure(floor(F))` onto the
/// sealed state of `floor(F)` — so it is a pure function of F, identical on
/// every node, whether sealed at finalization time or recomputed on a read
/// miss. Descendant blocks base their pre-state merge on this state rather
/// than on an LCA, which is what makes a finalized effect impossible to merge
/// away.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FloorData {
    /// The sealed finalized state hash at this cut — the merge base for every
    /// block whose floor is this cut.
    pub state_hash: StateHashSerde,
    /// Per-inclusion rejection verdicts accumulated by the seals up to this
    /// cut: the enforceRejected input. A rejected (sig, host) pair cannot be
    /// resurrected above the cut by a later merge; a RE-inclusion of the same
    /// sig in a new host is a fresh chain judged on its own merits.
    pub rejected_deploys: Vec<SealedRejection>,
    /// Per-inclusion ACCEPTANCE verdicts accumulated by the seals up to this
    /// cut: the deploys whose effect is in `FS`. A sig with any entry here is
    /// finalized-applied — recovery must never re-propose it and `repeat_deploy`
    /// must reject any re-inclusion, regardless of a more-recent sealed
    /// rejection of a different inclusion. This is the fix for the
    /// recover-an-already-finalized-deploy content-twin: judging by the single
    /// most-recent inclusion classified an accepted deploy as `SealedRejected`.
    pub accepted_deploys: Vec<SealedAcceptance>,
    /// Block number of the floor block, used for retention windowing.
    pub block_number: i64,
}
