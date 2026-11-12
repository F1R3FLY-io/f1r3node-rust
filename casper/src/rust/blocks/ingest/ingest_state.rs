use std::collections::{HashMap, HashSet};

use block_storage::rust::pending_blocks::{PendingBlockLimits, PendingUsage};
use comm::rust::peer_node::PeerNode;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::dependency_tracker::{
    BodyAt, Departure, DependencyTracker, FailureVerdict, HeldMeta, VictimClass, Work,
    MAX_DISK_ENTRIES, MAX_VALIDATION_FAILURES,
};
use super::fetch_scheduler::{FetchAction, FetchScheduler, NeedFacts, NeedOrigin};
use super::ingest_view::IngestView;
use super::peer_reputation::{PeerReputation, Strike, StrikeOutcome};
use super::LOG_TARGET;
// The retriever's source label is part of the exported name the block-transfer
// dashboard reads.
use crate::rust::metrics_constants::{
    BLOCK_DOWNLOAD_END_TO_END_TIME_METRIC, BLOCK_INGEST_DROPPED_METRIC,
    BLOCK_INGEST_EVICTIONS_METRIC, BLOCK_INGEST_METRICS_SOURCE, BLOCK_RETRIEVER_METRICS_SOURCE,
};

pub const LOCAL_LOOKUPS_PER_TICK: usize = 256;

/// Why a held block was evicted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvictReason {
    /// The pending area was above its low-water mark.
    PendingArea,
    /// Its sender held more than its share of the pending area.
    SenderShare,
    /// Room for a requested block the pending area refused.
    RoomForRequested,
    /// Bodies on disk exceeded their bound.
    DiskBound,
    /// Its validation kept failing.
    GiveUp,
}

impl EvictReason {
    fn label(self) -> &'static str {
        match self {
            EvictReason::PendingArea => "pending-area",
            EvictReason::SenderShare => "sender-share",
            EvictReason::RoomForRequested => "room-for-requested",
            EvictReason::DiskBound => "disk-bound",
            EvictReason::GiveUp => "give-up",
        }
    }
}

/// Why a block left together with every held block that waits on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropReason {
    Invalid,
    TooLarge,
    StateUnavailable,
}

impl DropReason {
    fn label(self) -> &'static str {
        match self {
            DropReason::Invalid => "invalid",
            DropReason::TooLarge => "too-large",
            DropReason::StateUnavailable => "state-unavailable",
        }
    }
}

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
    /// Self-declared.
    pub height: i64,
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
    /// Keep room in the pending area for a block that was refused for room,
    /// so no other block takes the room made for it.
    Reserve {
        hash: BlockHash,
        sender: Validator,
        size: usize,
    },
    ReleaseReservation(BlockHash),
    /// Delete the block's body from disk unless the block is in the DAG.
    DiscardStored(BlockHash),
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

    /// The hashes in `hashes` that are not satisfied.
    fn unsatisfied(&self, hashes: &[BlockHash]) -> Vec<BlockHash> {
        hashes
            .iter()
            .filter(|hash| !self.is_satisfied(hash))
            .cloned()
            .collect()
    }

    fn pending_usage(&self) -> PendingUsage;

    /// The pending area holds a body for `hash`.
    fn holds_pending(&self, hash: &BlockHash) -> bool;

    /// The validators with stake in `hash`, or `None` if the DAG does not
    /// hold it.
    fn bonded_in(&self, hash: &BlockHash) -> Option<HashSet<Validator>>;

    /// The validators with stake in the last finalized block, once there is
    /// one.
    fn bonded_in_last_finalized(&self) -> Option<HashSet<Validator>>;
}

/// What a held block's parents and justifications show before validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Fits the bonds it can be checked against: every dependency is cited.
    Fits,
    /// Cannot be checked yet: only the main parent is cited.
    Unchecked,
    /// Cannot be valid: only the main parent is cited, and validation judges
    /// the block.
    Invalid,
}

/// The block ingest owner's state: what the node holds, what it needs, and
/// which peers it ignores. Every input is handled synchronously and returns
/// the effects for the owner task to carry out.
#[derive(Debug)]
pub struct IngestState {
    /// Height of the approved block, once casper runs: cited blocks at or
    /// below it may be admitted as settled history.
    anchor_height: Option<i64>,
    /// The most parents a valid block lists; `None` when unlimited.
    max_parents: Option<usize>,
    /// Blocks inserted while a worker still validated them; their waiters
    /// are released when the outcome arrives.
    inserted_while_working: HashSet<BlockHash>,
    /// Needed blocks the pending area keeps room for.
    reserved: HashSet<BlockHash>,
    tracker: DependencyTracker,
    scheduler: FetchScheduler,
    reputation: PeerReputation,
}

impl IngestState {
    pub fn new(view: IngestView) -> Self {
        Self {
            anchor_height: None,
            max_parents: None,
            inserted_while_working: HashSet::new(),
            reserved: HashSet::new(),
            tracker: DependencyTracker::new(view.clone()),
            scheduler: FetchScheduler::new(view.clone()),
            reputation: PeerReputation::new(view),
        }
    }

    /// Blocks listing more than `max_parents` parents cite only their main
    /// parent; `None` leaves the parent count unchecked.
    pub fn with_max_parents(mut self, max_parents: Option<usize>) -> Self {
        self.max_parents = max_parents;
        self
    }

    /// The local lookup for `hash` could not be issued; issue it again.
    pub fn defer_local_lookup(&mut self, hash: &BlockHash) {
        self.scheduler.defer_local_lookup(hash);
    }

    pub fn held(&self) -> usize { self.tracker.held() }

    /// Held blocks ready for a validation slot.
    pub fn ready(&self) -> usize { self.tracker.ready() }

    pub fn outstanding_requests(&self, now_ms: u64) -> usize { self.scheduler.outstanding(now_ms) }

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
                if self.tracker.contains(&hash) {
                    self.vouch_for_held(&hash);
                } else if !stores.in_dag(&hash) {
                    let facts = NeedFacts {
                        vouched: true,
                        ..NeedFacts::default()
                    };
                    self.scheduler
                        .need_block(&hash, NeedOrigin::Requested, facts, now_ms);
                }
            }
            IngestInput::IntakeRefused(refused) => {
                self.refused(refused, now_ms, stores, &mut effects)
            }
            IngestInput::Misbehaved { peer, strike } => {
                self.strike(&peer, strike, now_ms, &mut effects)
            }
            IngestInput::LocalMiss { hash } => self.scheduler.local_miss(&hash, now_ms),
            IngestInput::Inserted { hash } => self.inserted(&hash, now_ms, stores),
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
                        self.drop_with_dependents(
                            &waiter,
                            DropReason::StateUnavailable,
                            &mut effects,
                        );
                    }
                }
                StateFetchResult::Dropped(_) => self.scheduler.root_dropped(&root),
            },
            IngestInput::Tick { connected } => {
                self.tracker.forget_expired_quarantines(now_ms);
                self.tracker.release_cooled(now_ms);
                self.proactive_pass(&stores.pending_usage(), now_ms, &mut effects);
                effects.extend(
                    self.scheduler
                        .due(now_ms, &connected, LOCAL_LOOKUPS_PER_TICK)
                        .into_iter()
                        .map(Effect::Fetch),
                );
            }
        }
        self.release_unneeded_reservations(&mut effects);
        effects
    }

    /// Room is kept only for a block the node still needs: one that arrived,
    /// or whose need ended, gives its room back.
    fn release_unneeded_reservations(&mut self, effects: &mut Vec<Effect>) {
        let scheduler = &self.scheduler;
        self.reserved.retain(|hash| {
            let needed = scheduler.is_needed(hash);
            if !needed {
                effects.push(Effect::ReleaseReservation(hash.clone()));
            }
            needed
        });
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
        if stores.in_dag(&hash) {
            tracing::debug!(
                target: LOG_TARGET,
                block = %short(&hash),
                "Block is already in the DAG; copy dropped"
            );
            if meta.body == BodyAt::Pending {
                effects.push(Effect::DiscardPending(hash.clone()));
            }
            self.scheduler.remove_block(&hash);
            return;
        }
        if let Some(until_ms) = self
            .tracker
            .quarantined_until(&hash)
            .filter(|until_ms| now_ms < *until_ms)
        {
            tracing::debug!(
                target: LOG_TARGET,
                block = %short(&hash),
                quarantine_left_ms = until_ms - now_ms,
                "Block is quarantined after repeated validation failures; copy dropped"
            );
            if meta.body == BodyAt::Pending {
                effects.push(Effect::DiscardPending(hash.clone()));
            }
            self.scheduler.defer_until(&hash, until_ms);
            return;
        }
        // A late report for a pending body the owner already discarded: the
        // block is not held, so its need stays and it is fetched again.
        if meta.body == BodyAt::Pending && !stores.holds_pending(&hash) {
            tracing::debug!(
                target: LOG_TARGET,
                block = %short(&hash),
                "Pending body was discarded before its report arrived; block fetched again"
            );
            self.scheduler.local_miss(&hash, now_ms);
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
        let settled = need
            .as_ref()
            .is_some_and(|need| need.facts.vouched && self.at_or_below_anchor(meta.height));
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
            false => match self.shape(&meta, stores) {
                Shape::Fits => (Work::Validate, missing_dependencies(&meta.deps, stores)),
                shape => {
                    meta.deferred = shape == Shape::Unchecked;
                    let main_parent = meta
                        .parents
                        .first()
                        .cloned()
                        .into_iter()
                        .collect::<Vec<_>>();
                    (Work::Validate, missing_dependencies(&main_parent, stores))
                }
            },
        };
        let facts = citation(&meta);
        let body = meta.body;
        tracing::debug!(
            target: LOG_TARGET,
            block = %short(&hash),
            height = meta.height,
            sender = %short(&meta.sender),
            solicited = meta.solicited,
            body = ?body,
            work = ?work,
            waits_on = %short_all(&missing),
            "Block held"
        );
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
            height,
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
                if stores.in_dag(&hash) || self.tracker.contains(&hash) {
                    self.scheduler.remove_block(&hash);
                } else {
                    match &from {
                        Some(peer) => self.scheduler.peer_failed(&hash, peer, now_ms),
                        None => self.scheduler.local_miss(&hash, now_ms),
                    }
                }
            }
            Refusal::Pending(_) if unsolicited => {}
            Refusal::Pending(PendingRefusal::AreaFull) => {
                let reserve_at = effects.len();
                if self.reactive_pass(&stores.pending_usage(), size, height, now_ms, effects) {
                    self.reserve(&hash, &sender, size, reserve_at, stores, effects);
                }
                self.keep_wanted(&hash, now_ms, stores);
            }
            Refusal::Pending(PendingRefusal::SenderLimitReached) => {
                let reserve_at = effects.len();
                let area = stores.pending_usage();
                // The area lists only senders with blocks in it.
                let count = area.by_sender.get(&sender).copied().unwrap_or(0);
                let mut excess = (count + 1).saturating_sub(area.limits.max_per_sender);
                let unsolicited =
                    self.tracker
                        .victims(BodyAt::Pending, VictimClass::Unsolicited, Some(&sender));
                for (victim, _) in unsolicited.into_iter().take(excess) {
                    self.evict(&victim, EvictReason::SenderShare, now_ms, effects);
                    excess -= 1;
                }
                // Requested blocks that wait on the refused one cannot validate
                // before it, so they give up their slots to it.
                let dependents = self.tracker.dependents(&hash);
                let waiting: Vec<BlockHash> = self
                    .tracker
                    .victims(BodyAt::Pending, VictimClass::Solicited, Some(&sender))
                    .into_iter()
                    .map(|(victim, _)| victim)
                    .filter(|victim| dependents.contains(victim))
                    .take(excess)
                    .collect();
                excess -= waiting.len();
                for victim in waiting {
                    self.displace(&victim, EvictReason::SenderShare, now_ms, effects);
                }
                if excess == 0 {
                    self.reserve(&hash, &sender, size, reserve_at, stores, effects);
                }
                self.keep_wanted(&hash, now_ms, stores);
            }
            Refusal::Pending(PendingRefusal::TooLarge) => {
                self.scheduler.remove_block(&hash);
                self.drop_with_dependents(&hash, DropReason::TooLarge, effects);
            }
        }
    }

    fn inserted(&mut self, hash: &BlockHash, now_ms: u64, stores: &dyn StoreReads) {
        // The worker validating `hash` still applies the insert's effects
        // (latest messages, finalization); its waiters start after its
        // outcome.
        if self.tracker.is_working(hash) {
            self.inserted_while_working.insert(hash.clone());
            return;
        }
        self.finish_insert(hash, now_ms, stores);
    }

    fn finish_insert(&mut self, hash: &BlockHash, now_ms: u64, stores: &dyn StoreReads) {
        let departure = self.tracker.inserted(hash);
        self.scheduler.remove_block(hash);
        let released = departure.released.clone();
        tracing::debug!(
            target: LOG_TARGET,
            block = %short(hash),
            released = %short_all(&released),
            "Block inserted into the DAG"
        );
        self.released(departure);
        for waiter in released {
            self.check_deferred_shape(&waiter, now_ms, stores);
        }
    }

    /// A block that cited only its main parent, released now that the main
    /// parent is in the DAG: if its shape fits that parent's bonds, it waits
    /// on the rest of its dependencies; otherwise validation judges it.
    fn check_deferred_shape(&mut self, hash: &BlockHash, now_ms: u64, stores: &dyn StoreReads) {
        if !self.tracker.take_deferred(hash) {
            return;
        }
        let meta = self
            .tracker
            .meta(hash)
            .expect("a released block is held")
            .clone();
        if self.shape(&meta, stores) != Shape::Fits {
            return;
        }
        let missing = missing_dependencies(&meta.deps, stores);
        if missing.is_empty() {
            return;
        }
        tracing::debug!(
            target: LOG_TARGET,
            block = %short(hash),
            waits_on = %short_all(&missing),
            "Block fits its main parent's bonds; it waits on its other dependencies"
        );
        self.tracker.wait_on_dependencies(hash, missing.clone());
        self.cite(&missing, &citation(&meta), now_ms);
    }

    /// Whether `meta`'s parents and justifications fit the rules a valid
    /// block meets: no validator named twice, no more parents than allowed,
    /// and the named validators equal to the main parent's bonds. Before the
    /// main parent is in the DAG, the named validators must be bonded in the
    /// last finalized block.
    fn shape(&self, meta: &HeldMeta, stores: &dyn StoreReads) -> Shape {
        let named: HashSet<&Validator> = meta.justification_validators.iter().collect();
        if named.len() != meta.justification_validators.len()
            || self
                .max_parents
                .is_some_and(|max_parents| meta.parents.len() > max_parents)
        {
            return Shape::Invalid;
        }
        let Some(main_parent) = meta.parents.first() else {
            return Shape::Invalid;
        };
        match stores.bonded_in(main_parent) {
            Some(bonds) => {
                match named.len() == bonds.len() && named.iter().all(|v| bonds.contains(*v)) {
                    true => Shape::Fits,
                    false => Shape::Invalid,
                }
            }
            None => match stores.bonded_in_last_finalized() {
                Some(bonds) if named.iter().all(|v| bonds.contains(*v)) => Shape::Fits,
                _ => Shape::Unchecked,
            },
        }
    }

    fn outcome(
        &mut self,
        hash: &BlockHash,
        outcome: WorkOutcome,
        now_ms: u64,
        stores: &dyn StoreReads,
        effects: &mut Vec<Effect>,
    ) {
        if self.inserted_while_working.remove(hash) {
            self.finish_insert(hash, now_ms, stores);
            return;
        }
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
                self.drop_with_dependents(hash, DropReason::Invalid, effects);
            }
            WorkOutcome::AwaitingBlock(missing) => {
                if stores.is_satisfied(&missing) {
                    tracing::debug!(
                        target: LOG_TARGET,
                        block = %short(hash),
                        dependency = %short(&missing),
                        "Validation read a snapshot older than a dependency now in the DAG; retrying"
                    );
                    if self.tracker.stale_snapshot(hash, now_ms) == FailureVerdict::GiveUp {
                        self.give_up(hash, now_ms, effects);
                    }
                } else {
                    tracing::debug!(
                        target: LOG_TARGET,
                        block = %short(hash),
                        waits_on = %short(&missing),
                        "Validation needs a block this node does not hold"
                    );
                    self.tracker.wait_on_block(hash, &missing);
                    self.cite(std::slice::from_ref(&missing), &facts, now_ms);
                }
            }
            WorkOutcome::AwaitingState(root) => {
                tracing::debug!(
                    target: LOG_TARGET,
                    block = %short(hash),
                    root = %root,
                    "Validation needs a state root this node does not hold"
                );
                self.tracker.wait_on_root(hash, &root);
                self.scheduler.need_root(&root, now_ms);
            }
            WorkOutcome::NotSettledHistory => {
                let missing = missing_dependencies(&meta.deps, stores);
                tracing::debug!(
                    target: LOG_TARGET,
                    block = %short(hash),
                    waits_on = %short_all(&missing),
                    "Settled-history admission declined the block; it is validated"
                );
                self.tracker.reclassify(hash, missing.clone());
                self.cite(&missing, &facts, now_ms);
            }
            WorkOutcome::Failed => match self.tracker.failed(hash, now_ms) {
                FailureVerdict::GiveUp => self.give_up(hash, now_ms, effects),
                FailureVerdict::Retry => tracing::debug!(
                    target: LOG_TARGET,
                    block = %short(hash),
                    failures = self.tracker.failures(hash),
                    retry_in_ms = self
                        .tracker
                        .retry_at_ms(hash)
                        .map(|retry_at_ms| retry_at_ms.saturating_sub(now_ms)),
                    "Validation attempt failed without a verdict; retrying"
                ),
            },
        }
    }

    /// Stop holding a block whose validation keeps failing. The blocks that
    /// wait on it keep waiting, and it is asked for again once its quarantine
    /// ends.
    fn give_up(&mut self, hash: &BlockHash, now_ms: u64, effects: &mut Vec<Effect>) {
        let quarantined_until = self.tracker.quarantined_until(hash);
        tracing::warn!(
            target: LOG_TARGET,
            block = %short(hash),
            failures = MAX_VALIDATION_FAILURES,
            quarantine_ms = quarantined_until.map(|until_ms| until_ms.saturating_sub(now_ms)),
            "Block validation keeps failing without a verdict; block evicted until its quarantine ends"
        );
        self.evict(hash, EvictReason::GiveUp, now_ms, effects);
        if let Some(until_ms) = quarantined_until {
            self.scheduler.defer_until(hash, until_ms);
        }
    }

    /// Held blocks wait on `missing`: each one not itself held is needed, and a
    /// vouched citation of one that is held may make it settled history.
    fn cite(&mut self, missing: &[BlockHash], facts: &NeedFacts, now_ms: u64) {
        for dependency in missing {
            if !self.tracker.contains(dependency) {
                self.scheduler
                    .need_block(dependency, NeedOrigin::Cited, facts.clone(), now_ms);
            } else if facts.vouched {
                self.vouch_for_held(dependency);
            }
        }
    }

    /// A held block vouched for after it was held is admitted as settled
    /// history instead of validated, if it is at or below the anchor and its
    /// validation has not started.
    fn vouch_for_held(&mut self, hash: &BlockHash) {
        let Some(height) = self.tracker.meta(hash).map(|meta| meta.height) else {
            return;
        };
        if !self.at_or_below_anchor(height) {
            return;
        }
        if let Some(unwaited) = self.tracker.admit_settled_instead(hash) {
            tracing::debug!(
                target: LOG_TARGET,
                block = %short(hash),
                height,
                "Block vouched for while held; it is admitted as settled history"
            );
            self.released(Departure {
                unwaited,
                ..Departure::default()
            });
        }
    }

    fn at_or_below_anchor(&self, height: i64) -> bool {
        self.anchor_height
            .is_some_and(|anchor| anchor > 0 && height <= anchor)
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
    /// A body on disk is deleted only when nothing needs the block any more;
    /// a needed block is read back from disk by a local lookup.
    fn departed(&mut self, departure: Departure, effects: &mut Vec<Effect>) {
        for meta in &departure.removed {
            match meta.body {
                BodyAt::Pending => effects.push(Effect::DiscardPending(meta.hash.clone())),
                BodyAt::Disk if !self.scheduler.is_needed(&meta.hash) => {
                    effects.push(Effect::DiscardStored(meta.hash.clone()))
                }
                BodyAt::Disk => {}
            }
        }
        self.released(departure);
    }

    /// `hash` and every held block that waits on it leave for good.
    fn drop_with_dependents(
        &mut self,
        hash: &BlockHash,
        reason: DropReason,
        effects: &mut Vec<Effect>,
    ) {
        let departure = self.tracker.drop_with_dependents(hash);
        let dropped = departure.removed.len();
        metrics::counter!(
            BLOCK_INGEST_DROPPED_METRIC,
            "source" => BLOCK_INGEST_METRICS_SOURCE,
            "reason" => reason.label()
        )
        .increment(dropped as u64);
        match reason {
            DropReason::Invalid => tracing::info!(
                target: LOG_TARGET,
                block = %short(hash),
                reason = reason.label(),
                dropped,
                "Block dropped with the held blocks that wait on it"
            ),
            DropReason::TooLarge | DropReason::StateUnavailable => tracing::warn!(
                target: LOG_TARGET,
                block = %short(hash),
                reason = reason.label(),
                dropped,
                "Block dropped with the held blocks that wait on it"
            ),
        }
        self.departed(departure, effects);
    }

    /// Evict a held block. If others wait on it, it becomes a need again and
    /// keeps the peer that pushed it.
    fn evict(
        &mut self,
        hash: &BlockHash,
        reason: EvictReason,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) {
        self.evict_keeping(hash, false, reason, now_ms, effects);
    }

    /// Evict a held block to make room for a refused requested block. A
    /// requested victim nothing waits on stays a need, so it is fetched again
    /// once the block it made room for has moved on.
    fn displace(
        &mut self,
        hash: &BlockHash,
        reason: EvictReason,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) {
        self.evict_keeping(hash, true, reason, now_ms, effects);
    }

    /// A refused requested block stays needed after the passes that made room
    /// for it, which may have removed its last waiter, and is asked for again
    /// without waiting out its re-request interval.
    fn keep_wanted(&mut self, hash: &BlockHash, now_ms: u64, stores: &dyn StoreReads) {
        if stores.in_dag(hash) || self.tracker.contains(hash) {
            return;
        }
        self.scheduler
            .need_block(hash, NeedOrigin::Requested, NeedFacts::default(), now_ms);
        self.scheduler.hasten(hash, now_ms);
    }

    fn evict_keeping(
        &mut self,
        hash: &BlockHash,
        keep_requested: bool,
        reason: EvictReason,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) {
        let departure = self.tracker.evict(hash);
        let meta = departure
            .removed
            .first()
            .cloned()
            .expect("an eviction removes exactly its own entry");
        let waited_on = self.tracker.is_waited_on(hash);
        metrics::counter!(
            BLOCK_INGEST_EVICTIONS_METRIC,
            "source" => BLOCK_INGEST_METRICS_SOURCE,
            "reason" => reason.label()
        )
        .increment(1);
        tracing::debug!(
            target: LOG_TARGET,
            block = %short(hash),
            height = meta.height,
            sender = %short(&meta.sender),
            reason = reason.label(),
            fetched_again = waited_on || (keep_requested && meta.solicited),
            "Block evicted"
        );
        if waited_on {
            let facts = NeedFacts {
                height_hint: Some(meta.height),
                trusted: meta.trusted,
                vouched: false,
            };
            self.scheduler
                .need_block(hash, NeedOrigin::Cited, facts, now_ms);
            if let Some(pusher) = meta.pushed_by {
                self.scheduler.carry_pusher(hash, pusher);
            }
        } else if keep_requested && meta.solicited {
            let facts = NeedFacts {
                height_hint: Some(meta.height),
                trusted: meta.trusted,
                vouched: false,
            };
            self.scheduler
                .need_block(hash, NeedOrigin::Requested, facts, now_ms);
        }
        self.departed(departure, effects);
    }

    fn enforce_disk_bound(&mut self, now_ms: u64, effects: &mut Vec<Effect>) {
        while self.tracker.disk_entries() > MAX_DISK_ENTRIES {
            let Some(victim) = self.tracker.disk_victim() else {
                return;
            };
            self.evict(&victim, EvictReason::DiskBound, now_ms, effects);
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
        let mut evicted_by_sender: HashMap<Validator, usize> = HashMap::new();
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
                let sender = self
                    .tracker
                    .meta(&victim)
                    .expect("a victim is held")
                    .sender
                    .clone();
                self.evict(&victim, EvictReason::PendingArea, now_ms, effects);
                *evicted_by_sender.entry(sender).or_insert(0) += 1;
                blocks = blocks.saturating_sub(1);
                bytes = bytes.saturating_sub(size);
            }
        }
        let sender_target = PendingBlockLimits::low_water(limits.max_per_sender);
        let mut over: Vec<(&Validator, usize)> = area
            .by_sender
            .iter()
            .map(|(sender, count)| {
                // A sender absent here lost no block in the passes above.
                let evicted = evicted_by_sender.get(sender).copied().unwrap_or(0);
                (sender, count.saturating_sub(evicted))
            })
            .filter(|(_, count)| *count > sender_target)
            .collect();
        over.sort_unstable();
        for (sender, count) in over {
            let victims =
                self.tracker
                    .victims(BodyAt::Pending, VictimClass::Unsolicited, Some(sender));
            for (victim, _) in victims.into_iter().take(count - sender_target) {
                self.evict(&victim, EvictReason::SenderShare, now_ms, effects);
            }
        }
    }

    /// Keep the room just made for `hash`, a needed block, until it arrives
    /// or its need ends. The reservation goes ahead of the evictions that make
    /// the room, so no other block can take it in between.
    fn reserve(
        &mut self,
        hash: &BlockHash,
        sender: &Validator,
        size: usize,
        at: usize,
        stores: &dyn StoreReads,
        effects: &mut Vec<Effect>,
    ) {
        if stores.in_dag(hash) || self.tracker.contains(hash) || !self.reserved.insert(hash.clone())
        {
            return;
        }
        effects.insert(at, Effect::Reserve {
            hash: hash.clone(),
            sender: sender.clone(),
            size,
        });
    }

    /// A requested block at `height` was refused for room: evict unsolicited
    /// blocks, then requested blocks above it, until it fits. A requested
    /// block never makes room by taking a lower one's, so the blocks nearest
    /// the DAG keep theirs. Returns whether the block fits.
    fn reactive_pass(
        &mut self,
        area: &PendingUsage,
        size: usize,
        height: i64,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let limits = area.limits;
        let mut blocks = area.blocks;
        let mut bytes = area.bytes;
        let fits = |blocks: usize, bytes: usize| {
            blocks < limits.max_blocks && bytes.saturating_add(size) <= limits.max_bytes
        };
        let higher: Vec<(BlockHash, usize)> = self
            .tracker
            .victims(BodyAt::Pending, VictimClass::Solicited, None)
            .into_iter()
            .filter(|(victim, _)| {
                self.tracker.meta(victim).expect("a victim is held").height > height
            })
            .collect();
        let victims: Vec<(BlockHash, usize)> = self
            .tracker
            .victims(BodyAt::Pending, VictimClass::Unsolicited, None)
            .into_iter()
            .chain(higher)
            .collect();
        for (victim, victim_size) in victims {
            if fits(blocks, bytes) {
                break;
            }
            self.displace(&victim, EvictReason::RoomForRequested, now_ms, effects);
            blocks = blocks.saturating_sub(1);
            bytes = bytes.saturating_sub(victim_size);
        }
        fits(blocks, bytes)
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
        vouched: meta.bonded_at_anchor,
    }
}

fn short(hash: &BlockHash) -> String { PrettyPrinter::build_string_bytes(hash) }

fn short_all(hashes: &[BlockHash]) -> String {
    hashes.iter().map(short).collect::<Vec<_>>().join(",")
}

fn missing_dependencies(deps: &[BlockHash], stores: &dyn StoreReads) -> Vec<BlockHash> {
    let mut missing = stores.unsatisfied(deps);
    missing.sort_unstable();
    missing.dedup();
    missing
}
