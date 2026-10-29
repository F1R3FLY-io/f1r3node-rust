use std::sync::Arc;

use comm::rust::peer_node::PeerNode;
use dashmap::{DashMap, DashSet};
use models::rust::block_hash::BlockHash;

#[derive(Debug, Default)]
struct Sets {
    held: DashSet<BlockHash>,
    needed: DashSet<BlockHash>,
    solicited: DashSet<BlockHash>,
    waited_on: DashSet<BlockHash>,
    ignored_until: DashMap<PeerNode, u64>,
}

/// Read-only answers about block ingest for the tasks around its owner: the
/// network handler and intake. Only the owner's `IngestState` writes it, so an
/// answer can lag the owner by the input it is processing, never more.
#[derive(Debug, Clone, Default)]
pub struct IngestView(Arc<Sets>);

impl IngestView {
    pub fn new() -> Self { Self::default() }

    /// Pending, ready or being validated.
    pub fn is_held(&self, hash: &BlockHash) -> bool { self.0.held.contains(hash) }

    /// A request is outstanding for the hash, whatever created the need,
    /// including a peer's announcement.
    pub fn is_awaited(&self, hash: &BlockHash) -> bool { self.0.needed.contains(hash) }

    /// Needed because a held block cites it, the proposer asked for it, or a
    /// validation named it; not merely announced. Stays set for a held block
    /// that arrived solicited until its final outcome.
    pub fn is_solicited(&self, hash: &BlockHash) -> bool { self.0.solicited.contains(hash) }

    /// Some held block lists the hash as a missing dependency.
    pub fn has_waiters(&self, hash: &BlockHash) -> bool { self.0.waited_on.contains(hash) }

    pub fn is_ignored(&self, peer: &PeerNode, now_ms: u64) -> bool {
        self.0
            .ignored_until
            .get(peer)
            .is_some_and(|until| now_ms < *until)
    }

    pub(super) fn set_held(&self, hash: &BlockHash, held: bool) { set(&self.0.held, hash, held); }

    pub(super) fn set_needed(&self, hash: &BlockHash, needed: bool) {
        set(&self.0.needed, hash, needed);
    }

    pub(super) fn set_solicited(&self, hash: &BlockHash, solicited: bool) {
        set(&self.0.solicited, hash, solicited);
    }

    pub(super) fn set_waited_on(&self, hash: &BlockHash, waited_on: bool) {
        set(&self.0.waited_on, hash, waited_on);
    }

    pub(super) fn set_ignored<'a>(&self, ignored: impl Iterator<Item = (&'a PeerNode, u64)>) {
        let ignored: Vec<(PeerNode, u64)> =
            ignored.map(|(peer, until)| (peer.clone(), until)).collect();
        self.0
            .ignored_until
            .retain(|peer, _| ignored.iter().any(|(kept, _)| kept == peer));
        for (peer, until) in ignored {
            self.0.ignored_until.insert(peer, until);
        }
    }
}

fn set(members: &DashSet<BlockHash>, hash: &BlockHash, present: bool) {
    if present {
        members.insert(hash.clone());
    } else {
        members.remove(hash);
    }
}
