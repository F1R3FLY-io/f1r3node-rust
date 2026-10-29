use std::collections::HashMap;

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

/// Unsolicited blocks are admitted to the pending area only below this share
/// of each of its limits; the rest is reserved for requested blocks.
pub const PENDING_LOW_WATER_PERCENT: usize = 90;

pub const LOCAL_LOOKUPS_PER_TICK: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AreaLimits {
    pub max_blocks: usize,
    pub max_bytes: usize,
    pub max_per_sender: usize,
}

impl AreaLimits {
    fn low_water(limit: usize) -> usize { limit * PENDING_LOW_WATER_PERCENT / 100 }
}

/// The pending area's totals, read by the owner when it decides evictions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AreaUsage {
    pub blocks: usize,
    pub bytes: usize,
    pub by_sender: HashMap<Validator, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestConfig {
    /// Height of the approved block: blocks at or below it may be admitted
    /// as settled history.
    pub anchor_height: i64,
    pub area: AreaLimits,
    pub max_disk_entries: usize,
}

impl IngestConfig {
    pub fn new(anchor_height: i64, area: AreaLimits) -> Self {
        Self {
            anchor_height,
            area,
            max_disk_entries: MAX_DISK_ENTRIES,
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
    Malformed,
    Pending(PendingRefusal),
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
    /// Intake stored a block on disk or in the pending area. `area` is the
    /// pending area's usage after a pending store.
    Received {
        meta: HeldMeta,
        area: Option<AreaUsage>,
    },
    Announced {
        hash: BlockHash,
        peer: PeerNode,
    },
    Need {
        hash: BlockHash,
    },
    IntakeRefused {
        hash: BlockHash,
        from: Option<PeerNode>,
        unsolicited: bool,
        size: usize,
        sender: Validator,
        reason: Refusal,
        area: Option<AreaUsage>,
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
        area: AreaUsage,
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

/// The DAG's answers the owner needs while classifying a block.
pub trait Dependencies {
    fn in_dag(&self, hash: &BlockHash) -> bool;

    /// In the DAG, or recorded as an equivocation or an invalid block.
    fn is_satisfied(&self, hash: &BlockHash) -> bool;
}

/// The block ingest owner's state: what the node holds, what it needs, and
/// which peers it ignores. Every input is handled synchronously and returns
/// the effects for the owner task to carry out.
#[derive(Debug)]
pub struct IngestState {
    config: IngestConfig,
    tracker: DependencyTracker,
    scheduler: FetchScheduler,
    reputation: PeerReputation,
    view: IngestView,
}

impl IngestState {
    pub fn new(config: IngestConfig, view: IngestView) -> Self {
        Self {
            config,
            tracker: DependencyTracker::new(),
            scheduler: FetchScheduler::new(),
            reputation: PeerReputation::new(),
            view,
        }
    }

    pub fn view(&self) -> &IngestView { &self.view }

    pub fn held(&self) -> usize { self.tracker.held() }

    pub fn disk_entries(&self) -> usize { self.tracker.disk_entries() }

    pub fn block_needs(&self) -> usize { self.scheduler.block_needs() }

    pub fn uncited_needs(&self) -> usize { self.scheduler.uncited_needs() }

    pub fn root_needs(&self) -> usize { self.scheduler.root_needs() }

    pub fn is_held(&self, hash: &BlockHash) -> bool { self.tracker.contains(hash) }

    pub fn is_needed(&self, hash: &BlockHash) -> bool { self.scheduler.is_needed(hash) }

    pub fn held_meta(&self, hash: &BlockHash) -> Option<&HeldMeta> { self.tracker.meta(hash) }

    pub fn is_waited_on(&self, hash: &BlockHash) -> bool { self.tracker.is_waited_on(hash) }

    /// The next block for a free worker: the lowest ready block.
    pub fn next_dispatch(&mut self) -> Option<(BlockHash, Work)> { self.tracker.next_ready() }

    pub fn handle(
        &mut self,
        input: IngestInput,
        now_ms: u64,
        dag: &dyn Dependencies,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        match input {
            IngestInput::Received { meta, area } => {
                self.received(meta, area, now_ms, dag, &mut effects)
            }
            IngestInput::Announced { hash, peer } => {
                if !dag.in_dag(&hash) && !self.tracker.contains(&hash) {
                    let dropped = self.scheduler.announce(&hash, &peer, now_ms);
                    self.view.set_needed(&hash, true);
                    self.forget_needs(dropped);
                }
            }
            IngestInput::Need { hash } => {
                if !dag.in_dag(&hash) && !self.tracker.contains(&hash) {
                    let dropped = self.scheduler.need_block(
                        &hash,
                        NeedOrigin::Requested,
                        NeedFacts::default(),
                        now_ms,
                    );
                    self.view.set_needed(&hash, true);
                    self.view.set_solicited(&hash, true);
                    self.forget_needs(dropped);
                }
            }
            IngestInput::IntakeRefused {
                hash,
                from,
                unsolicited,
                size,
                sender,
                reason,
                area,
            } => self.refused(
                &hash,
                from,
                unsolicited,
                size,
                &sender,
                reason,
                area,
                now_ms,
                &mut effects,
            ),
            IngestInput::LocalMiss { hash } => self.scheduler.local_miss(&hash, now_ms),
            IngestInput::Inserted { hash } => self.inserted(&hash),
            IngestInput::Outcome { hash, outcome } => {
                self.outcome(&hash, outcome, now_ms, dag, &mut effects)
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
            IngestInput::Tick { connected, area } => {
                self.tracker.forget_expired_quarantines(now_ms);
                self.proactive_pass(&area, now_ms, &mut effects);
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
        area: Option<AreaUsage>,
        now_ms: u64,
        dag: &dyn Dependencies,
        effects: &mut Vec<Effect>,
    ) {
        let hash = meta.hash.clone();
        if self.tracker.contains(&hash) {
            return;
        }
        if dag.in_dag(&hash) || self.tracker.is_quarantined(&hash, now_ms) {
            if meta.body == BodyAt::Pending {
                effects.push(Effect::DiscardPending(hash));
            }
            return;
        }
        let need = self.scheduler.remove_block(&hash);
        let settled = need.as_ref().is_some_and(|need| {
            need.facts.cited_by_bonded
                && self.config.anchor_height > 0
                && meta.height <= self.config.anchor_height
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
            false => (Work::Validate, self.missing_dependencies(&meta.deps, dag)),
        };
        let facts = NeedFacts {
            height_hint: Some(meta.height - 1),
            trusted: meta.trusted,
            cited_by_bonded: meta.bonded_at_anchor,
        };
        let body = meta.body;
        self.view.set_held(&hash, true);
        self.view.set_needed(&hash, false);
        self.view.set_solicited(&hash, meta.solicited);
        self.tracker.hold(meta, work, missing.clone());
        self.cite(&missing, &facts, now_ms);
        match (body, area) {
            (BodyAt::Disk, _) => self.enforce_disk_bound(now_ms, effects),
            (BodyAt::Pending, Some(area)) => self.proactive_pass(&area, now_ms, effects),
            (BodyAt::Pending, None) => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn refused(
        &mut self,
        hash: &BlockHash,
        from: Option<PeerNode>,
        unsolicited: bool,
        size: usize,
        sender: &Validator,
        reason: Refusal,
        area: Option<AreaUsage>,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) {
        if unsolicited && matches!(reason, Refusal::Unauthorized | Refusal::Malformed) {
            if let Some(peer) = &from {
                let strike = match reason {
                    Refusal::Unauthorized => Strike::UnauthorizedSender,
                    _ => Strike::Malformed,
                };
                self.strike(peer, strike, now_ms, effects);
            }
        }
        match reason {
            Refusal::NotOfInterest | Refusal::Unauthorized | Refusal::Malformed => {
                if let Some(peer) = &from {
                    self.scheduler.peer_failed(hash, peer, now_ms);
                }
            }
            Refusal::Pending(_) if unsolicited => {}
            Refusal::Pending(PendingRefusal::AreaFull) => {
                let area = area.expect("an area refusal reports the area's usage");
                self.reactive_pass(&area, size, now_ms, effects);
            }
            Refusal::Pending(PendingRefusal::SenderLimitReached) => {
                let area = area.expect("an area refusal reports the area's usage");
                // The area lists only senders with blocks in it.
                let count = area.by_sender.get(sender).copied().unwrap_or(0);
                let excess = (count + 1).saturating_sub(self.config.area.max_per_sender);
                let victims =
                    self.tracker
                        .victims(BodyAt::Pending, VictimClass::Unsolicited, Some(sender));
                for (victim, _) in victims.into_iter().take(excess) {
                    self.evict(&victim, now_ms, effects);
                }
            }
            Refusal::Pending(PendingRefusal::TooLarge) => {
                if self.scheduler.remove_block(hash).is_some() {
                    self.view.set_needed(hash, false);
                    self.view.set_solicited(hash, false);
                }
                let departure = self.tracker.drop_with_dependents(hash);
                self.departed(departure, effects);
                self.view.set_waited_on(hash, false);
            }
        }
    }

    fn inserted(&mut self, hash: &BlockHash) {
        let departure = self.tracker.inserted(hash);
        self.view.set_waited_on(hash, false);
        if self.scheduler.remove_block(hash).is_some() {
            self.view.set_needed(hash, false);
        }
        self.view.set_solicited(hash, false);
        self.view.set_held(hash, false);
        for meta in &departure.removed {
            self.view.set_held(&meta.hash, false);
        }
        self.unwait(departure.unwaited);
        for root in departure.unawaited_roots {
            self.scheduler.remove_root(&root);
        }
    }

    fn outcome(
        &mut self,
        hash: &BlockHash,
        outcome: WorkOutcome,
        now_ms: u64,
        dag: &dyn Dependencies,
        effects: &mut Vec<Effect>,
    ) {
        if !self.tracker.contains(hash) {
            return;
        }
        match outcome {
            WorkOutcome::Settled => {}
            WorkOutcome::Rejected { invalid_sender } => {
                let meta = self.tracker.meta(hash).expect("checked held above").clone();
                if invalid_sender {
                    if let Some(peer) = &meta.pushed_by {
                        self.strike(peer, Strike::InvalidSender, now_ms, effects);
                    }
                }
                let departure = self.tracker.drop_with_dependents(hash);
                self.departed(departure, effects);
            }
            WorkOutcome::AwaitingBlock(missing) => {
                let first = self.tracker.wait_on_block(hash, &missing);
                if dag.is_satisfied(&missing) {
                    self.inserted(&missing);
                } else if first {
                    let meta = self.tracker.meta(hash).expect("checked held above").clone();
                    let facts = NeedFacts {
                        height_hint: Some(meta.height - 1),
                        trusted: meta.trusted,
                        cited_by_bonded: meta.bonded_at_anchor,
                    };
                    self.cite(std::slice::from_ref(&missing), &facts, now_ms);
                }
            }
            WorkOutcome::AwaitingState(root) => {
                if self.tracker.wait_on_root(hash, &root) {
                    self.scheduler.need_root(&root, now_ms);
                }
            }
            WorkOutcome::NotSettledHistory => {
                let meta = self.tracker.meta(hash).expect("checked held above").clone();
                let missing = self.missing_dependencies(&meta.deps, dag);
                self.tracker.reclassify(hash, missing.clone());
                let facts = NeedFacts {
                    height_hint: Some(meta.height - 1),
                    trusted: meta.trusted,
                    cited_by_bonded: meta.bonded_at_anchor,
                };
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

    fn missing_dependencies(&self, deps: &[BlockHash], dag: &dyn Dependencies) -> Vec<BlockHash> {
        let mut missing: Vec<BlockHash> = deps
            .iter()
            .filter(|dependency| !dag.is_satisfied(dependency))
            .cloned()
            .collect();
        missing.sort_unstable();
        missing.dedup();
        missing
    }

    /// Held blocks wait on `missing`: each one not itself held is needed.
    fn cite(&mut self, missing: &[BlockHash], facts: &NeedFacts, now_ms: u64) {
        for dependency in missing {
            self.view.set_waited_on(dependency, true);
            if self.tracker.contains(dependency) {
                continue;
            }
            let dropped =
                self.scheduler
                    .need_block(dependency, NeedOrigin::Cited, facts.clone(), now_ms);
            self.view.set_needed(dependency, true);
            self.view.set_solicited(dependency, true);
            self.forget_needs(dropped);
        }
    }

    /// No held block waits on these any more.
    fn unwait(&mut self, unwaited: Vec<BlockHash>) {
        for dependency in unwaited {
            self.view.set_waited_on(&dependency, false);
            match self.scheduler.uncite(&dependency) {
                Some(_) => {
                    self.view.set_needed(&dependency, false);
                    self.view.set_solicited(&dependency, false);
                }
                None => {
                    if self.scheduler.origin(&dependency) == Some(NeedOrigin::Announced) {
                        self.view.set_solicited(&dependency, false);
                    }
                }
            }
        }
    }

    fn forget_needs(&mut self, dropped: Vec<BlockHash>) {
        for hash in dropped {
            self.view.set_needed(&hash, false);
            self.view.set_solicited(&hash, false);
        }
    }

    fn departed(&mut self, departure: Departure, effects: &mut Vec<Effect>) {
        for meta in &departure.removed {
            self.view.set_held(&meta.hash, false);
            self.view.set_solicited(&meta.hash, false);
            self.view
                .set_waited_on(&meta.hash, self.tracker.is_waited_on(&meta.hash));
            if meta.body == BodyAt::Pending {
                effects.push(Effect::DiscardPending(meta.hash.clone()));
            }
        }
        self.unwait(departure.unwaited);
        for root in departure.unawaited_roots {
            self.scheduler.remove_root(&root);
        }
    }

    /// Evict a held block. If others wait on it, it becomes a need again and
    /// keeps the peer that pushed it.
    fn evict(&mut self, hash: &BlockHash, now_ms: u64, effects: &mut Vec<Effect>) {
        let mut departure = self.tracker.evict(hash);
        let meta = departure
            .removed
            .pop()
            .expect("an eviction removes exactly its own entry");
        self.view.set_held(hash, false);
        if meta.body == BodyAt::Pending {
            effects.push(Effect::DiscardPending(hash.clone()));
        }
        if self.tracker.is_waited_on(hash) {
            let facts = NeedFacts {
                height_hint: Some(meta.height),
                trusted: meta.trusted,
                cited_by_bonded: false,
            };
            let dropped = self
                .scheduler
                .need_block(hash, NeedOrigin::Cited, facts, now_ms);
            if let Some(pusher) = meta.pushed_by {
                self.scheduler.carry_pusher(hash, pusher);
            }
            self.view.set_needed(hash, true);
            self.view.set_solicited(hash, true);
            self.forget_needs(dropped);
        } else {
            self.view.set_solicited(hash, false);
        }
        self.unwait(departure.unwaited);
        for root in departure.unawaited_roots {
            self.scheduler.remove_root(&root);
        }
    }

    fn enforce_disk_bound(&mut self, now_ms: u64, effects: &mut Vec<Effect>) {
        if self.tracker.disk_entries() <= self.config.max_disk_entries {
            return;
        }
        let excess = self.tracker.disk_entries() - self.config.max_disk_entries;
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
    fn proactive_pass(&mut self, area: &AreaUsage, now_ms: u64, effects: &mut Vec<Effect>) {
        let limits = self.config.area;
        let mut blocks = area.blocks;
        let mut bytes = area.bytes;
        let block_target = AreaLimits::low_water(limits.max_blocks);
        let byte_target = AreaLimits::low_water(limits.max_bytes);
        if blocks > block_target || bytes > byte_target {
            for (victim, size) in
                self.tracker
                    .victims(BodyAt::Pending, VictimClass::Unsolicited, None)
            {
                if blocks <= block_target && bytes <= byte_target {
                    break;
                }
                self.evict(&victim, now_ms, effects);
                blocks = blocks.saturating_sub(1);
                bytes = bytes.saturating_sub(size);
            }
        }
        let sender_target = AreaLimits::low_water(limits.max_per_sender);
        let mut over: Vec<(&Validator, usize)> = area
            .by_sender
            .iter()
            .filter(|(_, count)| **count > sender_target)
            .map(|(sender, count)| (sender, *count))
            .collect();
        over.sort_unstable();
        for (sender, count) in over {
            let excess = count - sender_target;
            let victims =
                self.tracker
                    .victims(BodyAt::Pending, VictimClass::Unsolicited, Some(sender));
            for (victim, _) in victims.into_iter().take(excess) {
                if self.tracker.contains(&victim) {
                    self.evict(&victim, now_ms, effects);
                }
            }
        }
    }

    /// A requested block was refused for room: evict unsolicited blocks, then
    /// requested ones only once no unsolicited block remains, until it fits.
    fn reactive_pass(
        &mut self,
        area: &AreaUsage,
        size: usize,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) {
        let limits = self.config.area;
        let mut blocks = area.blocks;
        let mut bytes = area.bytes;
        let fits = |blocks: usize, bytes: usize| {
            blocks < limits.max_blocks && bytes.saturating_add(size) <= limits.max_bytes
        };
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
            if fits(blocks, bytes) {
                break;
            }
            self.evict(&victim, now_ms, effects);
            blocks = blocks.saturating_sub(1);
            bytes = bytes.saturating_sub(victim_size);
        }
    }

    fn strike(&mut self, peer: &PeerNode, strike: Strike, now_ms: u64, effects: &mut Vec<Effect>) {
        let outcome = self.reputation.strike(peer, now_ms);
        if matches!(outcome, StrikeOutcome::Ignored { .. }) {
            self.view.set_ignored(self.reputation.ignored());
        }
        effects.push(Effect::Struck {
            peer: peer.clone(),
            strike,
            outcome,
        });
    }
}
