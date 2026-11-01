use block_storage::rust::pending_blocks::{PendingBlockLimits, PendingUsage};
use comm::rust::peer_node::PeerNode;
use models::rust::block_hash::BlockHash;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::dependency_tracker::{
    BodyAt, Departure, DependencyTracker, FailureVerdict, HeldMeta, VictimClass, Work,
    MAX_DISK_ENTRIES,
};
use super::fetch_scheduler::{FetchAction, FetchScheduler, NeedFacts, NeedOrigin};
use super::ingest_view::IngestView;
use super::peer_reputation::{PeerReputation, Strike, StrikeOutcome};
// The retriever's source label is part of the exported name the block-transfer
// dashboard reads.
use crate::rust::metrics_constants::{
    BLOCK_DOWNLOAD_END_TO_END_TIME_METRIC, BLOCK_RETRIEVER_METRICS_SOURCE,
};

pub const LOCAL_LOOKUPS_PER_TICK: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingRefusal {
    AreaFull,
    SenderLimitReached,
    TooLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NotOfInterest,
    Unauthorized,
    Pending(PendingRefusal),
}

/// A block intake did not hold, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub hash: BlockHash,
    pub from: Option<PeerNode>,
    pub unsolicited: bool,
    pub size: usize,
    pub sender: Validator,
    pub reason: Refusal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkOutcome {
    /// Inserted into the DAG, valid or invalid; `Inserted` arrives as well.
    Settled,
    /// Invalid and not inserted.
    Rejected {
        invalid_sender: bool,
    },
    AwaitingBlock(BlockHash),
    AwaitingState(Blake2b256Hash),
    /// A settled-history admission declined the block.
    NotSettledHistory,
    /// No verdict: the attempt failed.
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFetchDrop {
    QueueFull,
    BudgetExhausted,
    PeerLacksRoot,
    Unverified,
    Deadline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFetchResult {
    Imported,
    Dropped(StateFetchDrop),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestInput {
    /// Casper runs from an approved block at this height.
    Attached {
        anchor_height: i64,
    },
    /// Intake stored a block on disk or in the pending area.
    Received(HeldMeta),
    Announced {
        hash: BlockHash,
        peer: PeerNode,
    },
    Need {
        hash: BlockHash,
    },
    IntakeRefused(Refused),
    /// A peer sent a block message that failed the integrity check.
    Misbehaved {
        peer: PeerNode,
        strike: Strike,
    },
    LocalMiss {
        hash: BlockHash,
    },
    Inserted {
        hash: BlockHash,
    },
    Outcome {
        hash: BlockHash,
        outcome: WorkOutcome,
    },
    StateFetch {
        root: Blake2b256Hash,
        result: StateFetchResult,
    },
    Tick {
        connected: Vec<PeerNode>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Fetch(FetchAction),
    DiscardPending(BlockHash),
    Struck {
        peer: PeerNode,
        strike: Strike,
        outcome: StrikeOutcome,
    },
}

/// What the owner reads from the node's stores while handling an input.
pub trait StoreReads {
    fn in_dag(&self, hash: &BlockHash) -> bool;

    /// In the DAG, or recorded as an equivocation or an invalid block.
    fn is_satisfied(&self, hash: &BlockHash) -> bool;

    fn pending_usage(&self) -> PendingUsage;
}

/// The block ingest owner's state: what the node holds, what it needs, and
/// which peers it ignores. Every input is handled synchronously and returns
/// the effects for the owner task to carry out.
#[derive(Debug)]
pub struct IngestState {
    /// Height of the approved block, once casper runs: cited blocks at or
    /// below it may be admitted as settled history.
    anchor_height: Option<i64>,
    tracker: DependencyTracker,
    scheduler: FetchScheduler,
    reputation: PeerReputation,
}

impl IngestState {
    pub fn new(view: IngestView) -> Self {
        Self {
            anchor_height: None,
            tracker: DependencyTracker::new(view.clone()),
            scheduler: FetchScheduler::new(view.clone()),
            reputation: PeerReputation::new(view),
        }
    }

    /// The local lookup for `hash` could not be issued; issue it again.
    pub fn defer_local_lookup(&mut self, hash: &BlockHash) {
        self.scheduler.defer_local_lookup(hash);
    }

    pub fn held(&self) -> usize { self.tracker.held() }

    pub fn disk_entries(&self) -> usize { self.tracker.disk_entries() }

    pub fn block_needs(&self) -> usize { self.scheduler.block_needs() }

    pub fn uncited_needs(&self) -> usize { self.scheduler.uncited_needs() }

    pub fn root_needs(&self) -> usize { self.scheduler.root_needs() }

    pub fn ignored_peers(&self, now_ms: u64) -> usize { self.reputation.ignored_peers(now_ms) }

    pub fn is_held(&self, hash: &BlockHash) -> bool { self.tracker.contains(hash) }

    pub fn is_needed(&self, hash: &BlockHash) -> bool { self.scheduler.is_needed(hash) }

    #[cfg(test)]
    pub fn held_meta(&self, hash: &BlockHash) -> Option<&HeldMeta> { self.tracker.meta(hash) }

    /// The next block for a free worker: the lowest ready block.
    pub fn next_dispatch(&mut self) -> Option<(BlockHash, Work)> { self.tracker.next_ready() }

    pub fn handle(
        &mut self,
        input: IngestInput,
        now_ms: u64,
        stores: &dyn StoreReads,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        match input {
            IngestInput::Attached { anchor_height } => self.anchor_height = Some(anchor_height),
            IngestInput::Received(meta) => self.received(meta, now_ms, stores, &mut effects),
            IngestInput::Announced { hash, peer } => {
                if !stores.in_dag(&hash) && !self.tracker.contains(&hash) {
                    self.scheduler.announce(&hash, &peer, now_ms);
                }
            }
            IngestInput::Need { hash } => {
                if !stores.in_dag(&hash) && !self.tracker.contains(&hash) {
                    self.scheduler.need_block(
                        &hash,
                        NeedOrigin::Requested,
                        NeedFacts::default(),
                        now_ms,
                    );
                }
            }
            IngestInput::IntakeRefused(refused) => {
                self.refused(refused, now_ms, stores, &mut effects)
            }
            IngestInput::Misbehaved { peer, strike } => {
                self.strike(&peer, strike, now_ms, &mut effects)
            }
            IngestInput::LocalMiss { hash } => self.scheduler.local_miss(&hash, now_ms),
            IngestInput::Inserted { hash } => self.inserted(&hash),
            IngestInput::Outcome { hash, outcome } => {
                self.outcome(&hash, outcome, now_ms, stores, &mut effects)
            }
            IngestInput::StateFetch { root, result } => match result {
                StateFetchResult::Imported => {
                    self.tracker.root_imported(&root);
                    self.scheduler.remove_root(&root);
                }
                StateFetchResult::Dropped(StateFetchDrop::BudgetExhausted) => {
                    self.scheduler.remove_root(&root);
                    for waiter in self.tracker.root_waiters(&root) {
                        let departure = self.tracker.drop_with_dependents(&waiter);
                        self.departed(departure, &mut effects);
                    }
                }
                StateFetchResult::Dropped(_) => self.scheduler.root_dropped(&root),
            },
            IngestInput::Tick { connected } => {
                self.tracker.forget_expired_quarantines(now_ms);
                self.proactive_pass(&stores.pending_usage(), now_ms, &mut effects);
                effects.extend(
                    self.scheduler
                        .due(now_ms, &connected, LOCAL_LOOKUPS_PER_TICK)
                        .into_iter()
                        .map(Effect::Fetch),
                );
            }
        }
        effects
    }

    fn received(
        &mut self,
        mut meta: HeldMeta,
        now_ms: u64,
        stores: &dyn StoreReads,
        effects: &mut Vec<Effect>,
    ) {
        let hash = meta.hash.clone();
        if self.tracker.contains(&hash) {
            return;
        }
        if stores.in_dag(&hash) || self.tracker.is_quarantined(&hash, now_ms) {
            if meta.body == BodyAt::Pending {
                effects.push(Effect::DiscardPending(hash));
            }
            return;
        }
        let need = self.scheduler.remove_block(&hash);
        if let Some(first_requested_ms) = need.as_ref().and_then(|need| need.first_requested_ms) {
            metrics::histogram!(
                BLOCK_DOWNLOAD_END_TO_END_TIME_METRIC,
                "source" => BLOCK_RETRIEVER_METRICS_SOURCE
            )
            .record(now_ms.saturating_sub(first_requested_ms) as f64 / 1_000.0);
        }
        let settled = need.as_ref().is_some_and(|need| {
            need.facts.cited_by_bonded
                && self
                    .anchor_height
                    .is_some_and(|anchor| anchor > 0 && meta.height <= anchor)
        });
        if meta.solicited {
            meta.pushed_by = None;
        }
        if let Some(need) = need {
            meta.solicited |= need.origin != NeedOrigin::Announced;
            if need.pushed_by.is_some() {
                meta.pushed_by = need.pushed_by;
            }
        }
        let (work, missing) = match settled {
            true => (Work::AdmitSettled, Vec::new()),
            false => (Work::Validate, missing_dependencies(&meta.deps, stores)),
        };
        let facts = citation(&meta);
        let body = meta.body;
        self.tracker.hold(meta, work, missing.clone());
        self.cite(&missing, &facts, now_ms);
        match body {
            BodyAt::Disk => self.enforce_disk_bound(now_ms, effects),
            BodyAt::Pending => self.proactive_pass(&stores.pending_usage(), now_ms, effects),
        }
    }

    fn refused(
        &mut self,
        refused: Refused,
        now_ms: u64,
        stores: &dyn StoreReads,
        effects: &mut Vec<Effect>,
    ) {
        let Refused {
            hash,
            from,
            unsolicited,
            size,
            sender,
            reason,
        } = refused;
        if unsolicited {
            let strike = match reason {
                Refusal::Unauthorized => Some(Strike::UnauthorizedSender),
                Refusal::NotOfInterest | Refusal::Pending(_) => None,
            };
            if let (Some(strike), Some(peer)) = (strike, &from) {
                self.strike(peer, strike, now_ms, effects);
            }
        }
        match reason {
            Refusal::NotOfInterest | Refusal::Unauthorized => {
                if let Some(peer) = &from {
                    self.scheduler.peer_failed(&hash, peer, now_ms);
                }
            }
            Refusal::Pending(_) if unsolicited => {}
            Refusal::Pending(PendingRefusal::AreaFull) => {
                self.reactive_pass(&stores.pending_usage(), size, now_ms, effects)
            }
            Refusal::Pending(PendingRefusal::SenderLimitReached) => {
                let area = stores.pending_usage();
                // The area lists only senders with blocks in it.
                let count = area.by_sender.get(&sender).copied().unwrap_or(0);
                let excess = (count + 1).saturating_sub(area.limits.max_per_sender);
                let victims =
                    self.tracker
                        .victims(BodyAt::Pending, VictimClass::Unsolicited, Some(&sender));
                for (victim, _) in victims.into_iter().take(excess) {
                    self.evict(&victim, now_ms, effects);
                }
            }
            Refusal::Pending(PendingRefusal::TooLarge) => {
                self.scheduler.remove_block(&hash);
                let departure = self.tracker.drop_with_dependents(&hash);
                self.departed(departure, effects);
            }
        }
    }

    fn inserted(&mut self, hash: &BlockHash) {
        let departure = self.tracker.inserted(hash);
        self.scheduler.remove_block(hash);
        self.released(departure);
    }

    fn outcome(
        &mut self,
        hash: &BlockHash,
        outcome: WorkOutcome,
        now_ms: u64,
        stores: &dyn StoreReads,
        effects: &mut Vec<Effect>,
    ) {
        let Some(meta) = self.tracker.meta(hash) else {
            return;
        };
        let facts = citation(meta);
        match outcome {
            WorkOutcome::Settled => {}
            WorkOutcome::Rejected { invalid_sender } => {
                let pusher = meta.pushed_by.clone();
                if let (true, Some(peer)) = (invalid_sender, &pusher) {
                    self.strike(peer, Strike::InvalidSender, now_ms, effects);
                }
                let departure = self.tracker.drop_with_dependents(hash);
                self.departed(departure, effects);
            }
            WorkOutcome::AwaitingBlock(missing) => {
                self.tracker.wait_on_block(hash, &missing);
                if stores.is_satisfied(&missing) {
                    self.inserted(&missing);
                } else {
                    self.cite(std::slice::from_ref(&missing), &facts, now_ms);
                }
            }
            WorkOutcome::AwaitingState(root) => {
                self.tracker.wait_on_root(hash, &root);
                self.scheduler.need_root(&root, now_ms);
            }
            WorkOutcome::NotSettledHistory => {
                let missing = missing_dependencies(&meta.deps, stores);
                self.tracker.reclassify(hash, missing.clone());
                self.cite(&missing, &facts, now_ms);
            }
            WorkOutcome::Failed => {
                if self.tracker.failed(hash, now_ms) == FailureVerdict::GiveUp {
                    let departure = self.tracker.drop_with_dependents(hash);
                    self.departed(departure, effects);
                }
            }
        }
    }

    /// Held blocks wait on `missing`: each one not itself held is needed.
    fn cite(&mut self, missing: &[BlockHash], facts: &NeedFacts, now_ms: u64) {
        for dependency in missing {
            if !self.tracker.contains(dependency) {
                self.scheduler
                    .need_block(dependency, NeedOrigin::Cited, facts.clone(), now_ms);
            }
        }
    }

    /// Needs and state roots no held block waits on any more are dropped.
    fn released(&mut self, departure: Departure) {
        for dependency in departure.unwaited {
            self.scheduler.uncite(&dependency);
        }
        for root in departure.unawaited_roots {
            self.scheduler.remove_root(&root);
        }
    }

    /// Entries left for good: their pending bodies are discarded.
    fn departed(&mut self, departure: Departure, effects: &mut Vec<Effect>) {
        effects.extend(
            departure
                .removed
                .iter()
                .filter(|meta| meta.body == BodyAt::Pending)
                .map(|meta| Effect::DiscardPending(meta.hash.clone())),
        );
        self.released(departure);
    }

    /// Evict a held block. If others wait on it, it becomes a need again and
    /// keeps the peer that pushed it.
    fn evict(&mut self, hash: &BlockHash, now_ms: u64, effects: &mut Vec<Effect>) {
        let departure = self.tracker.evict(hash);
        let meta = departure
            .removed
            .first()
            .cloned()
            .expect("an eviction removes exactly its own entry");
        if self.tracker.is_waited_on(hash) {
            let facts = NeedFacts {
                height_hint: Some(meta.height),
                trusted: meta.trusted,
                cited_by_bonded: false,
            };
            self.scheduler
                .need_block(hash, NeedOrigin::Cited, facts, now_ms);
            if let Some(pusher) = meta.pushed_by {
                self.scheduler.carry_pusher(hash, pusher);
            }
        }
        self.departed(departure, effects);
    }

    fn enforce_disk_bound(&mut self, now_ms: u64, effects: &mut Vec<Effect>) {
        let Some(excess) = self
            .tracker
            .disk_entries()
            .checked_sub(MAX_DISK_ENTRIES)
            .filter(|excess| *excess > 0)
        else {
            return;
        };
        let victims: Vec<BlockHash> = self
            .tracker
            .victims(BodyAt::Disk, VictimClass::Unsolicited, None)
            .into_iter()
            .chain(
                self.tracker
                    .victims(BodyAt::Disk, VictimClass::Solicited, None),
            )
            .map(|(hash, _)| hash)
            .take(excess)
            .collect();
        for victim in victims {
            self.evict(&victim, now_ms, effects);
        }
    }

    /// Bring the pending area back to its low-water mark, globally and per
    /// sender, by evicting unsolicited blocks only.
    fn proactive_pass(&mut self, area: &PendingUsage, now_ms: u64, effects: &mut Vec<Effect>) {
        let limits = area.limits;
        let mut blocks = area.blocks;
        let mut bytes = area.bytes;
        // Unknown senders' blocks down to their own mark, which keeps room for
        // known senders; then any unsolicited block down to the low-water mark.
        let passes = [
            (
                false,
                PendingBlockLimits::unknown_sender_mark(limits.max_blocks),
                PendingBlockLimits::unknown_sender_mark(limits.max_bytes),
            ),
            (
                true,
                PendingBlockLimits::low_water(limits.max_blocks),
                PendingBlockLimits::low_water(limits.max_bytes),
            ),
        ];
        for (trusted_too, block_target, byte_target) in passes {
            if blocks <= block_target && bytes <= byte_target {
                continue;
            }
            for (victim, size) in
                self.tracker
                    .victims(BodyAt::Pending, VictimClass::Unsolicited, None)
            {
                if (blocks <= block_target && bytes <= byte_target)
                    || (!trusted_too && self.tracker.is_trusted(&victim))
                {
                    break;
                }
                self.evict(&victim, now_ms, effects);
                blocks = blocks.saturating_sub(1);
                bytes = bytes.saturating_sub(size);
            }
        }
        let sender_target = PendingBlockLimits::low_water(limits.max_per_sender);
        let mut over: Vec<(&Validator, usize)> = area
            .by_sender
            .iter()
            .filter(|(_, count)| **count > sender_target)
            .map(|(sender, count)| (sender, *count))
            .collect();
        over.sort_unstable();
        for (sender, count) in over {
            let victims =
                self.tracker
                    .victims(BodyAt::Pending, VictimClass::Unsolicited, Some(sender));
            for (victim, _) in victims.into_iter().take(count - sender_target) {
                self.evict(&victim, now_ms, effects);
            }
        }
    }

    /// A requested block was refused for room: evict unsolicited blocks, then
    /// requested ones only once no unsolicited block remains, until it fits.
    fn reactive_pass(
        &mut self,
        area: &PendingUsage,
        size: usize,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) {
        let limits = area.limits;
        let mut blocks = area.blocks;
        let mut bytes = area.bytes;
        let victims: Vec<(BlockHash, usize)> = self
            .tracker
            .victims(BodyAt::Pending, VictimClass::Unsolicited, None)
            .into_iter()
            .chain(
                self.tracker
                    .victims(BodyAt::Pending, VictimClass::Solicited, None),
            )
            .collect();
        for (victim, victim_size) in victims {
            if blocks < limits.max_blocks && bytes.saturating_add(size) <= limits.max_bytes {
                break;
            }
            self.evict(&victim, now_ms, effects);
            blocks = blocks.saturating_sub(1);
            bytes = bytes.saturating_sub(victim_size);
        }
    }

    fn strike(&mut self, peer: &PeerNode, strike: Strike, now_ms: u64, effects: &mut Vec<Effect>) {
        let outcome = self.reputation.strike(peer, now_ms);
        effects.push(Effect::Struck {
            peer: peer.clone(),
            strike,
            outcome,
        });
    }
}

/// What a held block's citation tells about the blocks it waits on.
fn citation(meta: &HeldMeta) -> NeedFacts {
    NeedFacts {
        height_hint: Some(meta.height - 1),
        trusted: meta.trusted,
        cited_by_bonded: meta.bonded_at_anchor,
    }
}

fn missing_dependencies(deps: &[BlockHash], stores: &dyn StoreReads) -> Vec<BlockHash> {
    let mut missing: Vec<BlockHash> = deps
        .iter()
        .filter(|dependency| !stores.is_satisfied(dependency))
        .cloned()
        .collect();
    missing.sort_unstable();
    missing.dedup();
    missing
}
