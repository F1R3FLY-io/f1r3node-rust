use std::sync::Arc;

use comm::rust::peer_node::PeerNode;
use dashmap::{DashMap, DashSet};
use models::rust::block_hash::BlockHash;
use tokio::time::Instant;

#[derive(Debug)]
struct Sets {
    started: Instant,
    held: DashSet<BlockHash>,
    solicited_held: DashSet<BlockHash>,
    waited_on: DashSet<BlockHash>,
    needed: DashSet<BlockHash>,
    solicited_needs: DashSet<BlockHash>,
    ignored_until: DashMap<PeerNode, u64>,
}

/// Read-only answers about block ingest for the tasks around its owner: the
/// network handler and intake. Each set has one writer, the record it mirrors:
/// the dependency tracker writes the held and waited-on sets, the fetch
/// scheduler the needed sets, peer reputation the ignore list. An answer can
/// lag the owner by the input it is processing, never more.
#[derive(Debug, Clone)]
pub struct IngestView(Arc<Sets>);

impl Default for IngestView {
    fn default() -> Self { Self::new() }
}

impl IngestView {
    pub fn new() -> Self {
        Self(Arc::new(Sets {
            started: Instant::now(),
            held: DashSet::new(),
            solicited_held: DashSet::new(),
            waited_on: DashSet::new(),
            needed: DashSet::new(),
            solicited_needs: DashSet::new(),
            ignored_until: DashMap::new(),
        }))
    }

    /// Block ingest's clock: milliseconds since this view was created.
    pub fn now_ms(&self) -> u64 { self.0.started.elapsed().as_millis() as u64 }

    /// Pending, ready or being validated.
    pub fn is_held(&self, hash: &BlockHash) -> bool { self.0.held.contains(hash) }

    /// The node needs the hash, whatever created the need, including a peer's
    /// announcement.
    pub fn is_awaited(&self, hash: &BlockHash) -> bool { self.0.needed.contains(hash) }

    /// Needed because a held block cites it, the proposer asked for it, or a
    /// validation named it, not merely announced; or held after arriving so.
    pub fn is_solicited(&self, hash: &BlockHash) -> bool {
        self.0.solicited_needs.contains(hash) || self.0.solicited_held.contains(hash)
    }

    /// Some held block lists the hash as a missing dependency.
    pub fn has_waiters(&self, hash: &BlockHash) -> bool { self.0.waited_on.contains(hash) }

    /// Block traffic from `peer` is ignored now.
    pub fn is_ignored(&self, peer: &PeerNode) -> bool { self.is_ignored_at(peer, self.now_ms()) }

    pub fn is_ignored_at(&self, peer: &PeerNode, now_ms: u64) -> bool {
        self.0
            .ignored_until
            .get(peer)
            .is_some_and(|until| now_ms < *until)
    }

    pub(super) fn set_held(&self, hash: &BlockHash, held: bool, solicited: bool) {
        set(&self.0.held, hash, held);
        set(&self.0.solicited_held, hash, held && solicited);
    }

    pub(super) fn set_waited_on(&self, hash: &BlockHash, waited_on: bool) {
        set(&self.0.waited_on, hash, waited_on);
    }

    pub(super) fn set_needed(&self, hash: &BlockHash, needed: bool, solicited: bool) {
        set(&self.0.needed, hash, needed);
        set(&self.0.solicited_needs, hash, needed && solicited);
    }

    pub(super) fn set_ignored_until(&self, peer: &PeerNode, until_ms: Option<u64>) {
        match until_ms {
            Some(until_ms) => {
                self.0.ignored_until.insert(peer.clone(), until_ms);
            }
            None => {
                self.0.ignored_until.remove(peer);
            }
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
