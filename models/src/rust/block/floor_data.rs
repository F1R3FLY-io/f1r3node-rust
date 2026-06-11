use prost::bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::rust::block::state_hash::StateHashSerde;

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
    /// Deploy signatures rejected by the seal at this cut: the enforceRejected
    /// input. A finalization-rejected deploy cannot be resurrected above the
    /// cut by a later merge.
    #[serde(with = "shared::rust::serde_vec_bytes")]
    pub rejected_deploys: Vec<Bytes>,
    /// Block number of the floor block, used for retention windowing.
    pub block_number: i64,
}
