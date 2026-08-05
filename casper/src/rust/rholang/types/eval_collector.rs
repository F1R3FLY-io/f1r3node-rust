// See casper/src/main/scala/coop/rchain/casper/rholang/types/EvalCollector.scala

use std::collections::HashMap;

use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::Event;
use rholang::rust::interpreter::io::wal::WalEntry;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;

pub struct EvalCollector {
    pub event_log: Vec<Event>,
    pub mergeable_channels: HashMap<Par, MergeType>,
    /// Slice 30: per-deploy consensus filesystem WAL contribution.
    /// Populated at the end of `play_deploy_with_cost_accounting` via
    /// `Wal::take_deploy_entries(mark)` where `mark` was captured at
    /// the top of the deploy.  Insertion order is preserved (the
    /// order the leader's Rholang execution appended them, which is
    /// deterministic per Rholang small-step semantics up to the
    /// slice-29 H-R3 Par-parallel caveat).
    ///
    /// The bytes represented by these entries are NOT yet committed
    /// on-chain — that requires a hard-fork proto schema change
    /// bundled with slice 30b.  For now, casper's per-block WAL
    /// commitment is computed as `Blake2b256(canonical_encoding)`
    /// via `rholang::interpreter::io::snapshot::compute_wal_root`
    /// and can be logged / attached out-of-band.
    pub fs_wal_entries: Vec<WalEntry>,
}

impl EvalCollector {
    pub fn new() -> Self {
        Self {
            event_log: Vec::new(),
            mergeable_channels: HashMap::new(),
            fs_wal_entries: Vec::new(),
        }
    }

    pub fn add_event_log(&mut self, event_log: Vec<Event>) { self.event_log.extend(event_log); }

    pub fn add_mergeable_channels(&mut self, mergeable_channels: HashMap<Par, MergeType>) {
        self.mergeable_channels.extend(mergeable_channels);
    }

    pub fn add(&mut self, event_log: Vec<Event>, mergeable_channels: HashMap<Par, MergeType>) {
        self.event_log.extend(event_log);
        self.mergeable_channels.extend(mergeable_channels);
    }

    /// Slice 30: append WAL contributions from a sub-step (system
    /// deploy pre-charge, user deploy, refund, etc.).  Callers pass
    /// the drained-since-mark slice produced by
    /// `Wal::take_deploy_entries`.
    pub fn add_fs_wal_entries(&mut self, entries: Vec<WalEntry>) {
        self.fs_wal_entries.extend(entries);
    }
}
