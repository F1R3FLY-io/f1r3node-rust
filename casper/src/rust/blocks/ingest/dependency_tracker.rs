use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, HashSet};

use comm::rust::peer_node::PeerNode;
use models::rust::block_hash::BlockHash;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::ingest_view::IngestView;

pub const MAX_DISK_ENTRIES: usize = 65_536;

pub const MAX_VALIDATION_FAILURES: u32 = 32;

pub const VALIDATION_FAILURE_QUARANTINE_MS: u64 = 120_000;

/// First wait before a failed validation is retried. Each further failure
/// doubles it, up to `VALIDATION_FAILURE_QUARANTINE_MS`.
pub const VALIDATION_RETRY_BASE_MS: u64 = 500;

/// The wait before retry number `failures + 1` of a block whose validation
/// failed `failures` times.
pub fn validation_retry_delay_ms(failures: u32) -> u64 {
    let doublings = failures.saturating_sub(1).min(16);
    VALIDATION_RETRY_BASE_MS
        .saturating_mul(1 << doublings)
        .min(VALIDATION_FAILURE_QUARANTINE_MS)
}

/// Where a held block's body is: the block store, or the pending area for a
/// block whose sender's authorization is not yet known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyAt {
    Disk,
    Pending,
}

/// What the owner keeps about a held block. Intake fills it from the block
/// and the checks it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldMeta {
    pub hash: BlockHash,
    pub sender: Validator,
    /// Self-declared until the block is validated.
    pub height: i64,
    pub deps: Vec<BlockHash>,
    /// The parents as listed, main parent first.
    pub parents: Vec<BlockHash>,
    /// The validators the justifications name, as listed.
    pub justification_validators: Vec<Validator>,
    /// The block cites only its main parent until the main parent is in the
    /// DAG and the block's shape can be checked against its bonds.
    pub deferred: bool,
    pub body: BodyAt,
    pub size: usize,
    /// Signature checked and the sender bonded in the last finalized block.
    pub trusted: bool,
    /// The sender is bonded at the approved block.
    pub bonded_at_anchor: bool,
    pub solicited: bool,
    /// The peer that pushed the block unsolicited.
    pub pushed_by: Option<PeerNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Work {
    Validate,
    AdmitSettled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Waiting,
    Ready,
    Working,
}

#[derive(Debug)]
struct Held {
    meta: HeldMeta,
    work: Work,
    stage: Stage,
    missing: HashSet<BlockHash>,
    awaiting_root: Option<Blake2b256Hash>,
    retry_at_ms: Option<u64>,
    seq: u64,
}

impl Held {
    fn ready_key(&self) -> (i64, u64, BlockHash) {
        (self.meta.height, self.seq, self.meta.hash.clone())
    }
}

/// What leaving the tracker took with it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Departure {
    /// The entries that left.
    pub removed: Vec<HeldMeta>,
    /// Waiters that the departure made ready.
    pub released: Vec<BlockHash>,
    /// Missing dependencies that no remaining entry waits on.
    pub unwaited: Vec<BlockHash>,
    /// State roots that no remaining entry awaits.
    pub unawaited_roots: Vec<Blake2b256Hash>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureVerdict {
    Retry,
    GiveUp,
}

/// Which held blocks an eviction pass may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VictimClass {
    Unsolicited,
    Solicited,
}

/// Held blocks, what each waits on, and the blocks ready to dispatch, lowest
/// height first. A held block waits on missing blocks and at most one state
/// root; it becomes ready when the last of them arrives. Publishes which
/// blocks are held and which are waited on to the view.
#[derive(Debug)]
pub struct DependencyTracker {
    view: IngestView,
    held: HashMap<BlockHash, Held>,
    waiters: HashMap<BlockHash, HashSet<BlockHash>>,
    root_waiters: HashMap<Blake2b256Hash, HashSet<BlockHash>>,
    ready: BTreeSet<(i64, u64, BlockHash)>,
    cooling: BTreeSet<(u64, BlockHash)>,
    failures: HashMap<BlockHash, u32>,
    quarantined_until: HashMap<BlockHash, u64>,
    disk_entries: usize,
    disk_by_sender: HashMap<Validator, usize>,
    next_seq: u64,
}

impl DependencyTracker {
    pub fn new(view: IngestView) -> Self {
        Self {
            view,
            held: HashMap::new(),
            waiters: HashMap::new(),
            root_waiters: HashMap::new(),
            ready: BTreeSet::new(),
            cooling: BTreeSet::new(),
            failures: HashMap::new(),
            quarantined_until: HashMap::new(),
            disk_entries: 0,
            disk_by_sender: HashMap::new(),
            next_seq: 0,
        }
    }

    /// Hold a block that waits on `missing`.
    pub fn hold(&mut self, meta: HeldMeta, work: Work, missing: Vec<BlockHash>) {
        let hash = meta.hash.clone();
        assert!(
            !self.held.contains_key(&hash),
            "a block is held at most once; the caller drops duplicates"
        );
        if meta.body == BodyAt::Disk {
            self.disk_entries += 1;
            *self.disk_by_sender.entry(meta.sender.clone()).or_insert(0) += 1;
        }
        self.view.set_held(&hash, true, meta.solicited);
        let seq = self.next_seq;
        self.next_seq += 1;
        let missing: HashSet<BlockHash> = missing.into_iter().collect();
        for dependency in &missing {
            self.add_waiter(dependency, &hash);
        }
        let held = Held {
            meta,
            work,
            stage: if missing.is_empty() {
                Stage::Ready
            } else {
                Stage::Waiting
            },
            missing,
            awaiting_root: None,
            retry_at_ms: None,
            seq,
        };
        if held.stage == Stage::Ready {
            self.ready.insert(held.ready_key());
        }
        self.held.insert(hash, held);
    }

    pub fn contains(&self, hash: &BlockHash) -> bool { self.held.contains_key(hash) }

    pub fn is_working(&self, hash: &BlockHash) -> bool {
        self.held
            .get(hash)
            .is_some_and(|held| held.stage == Stage::Working)
    }

    pub fn meta(&self, hash: &BlockHash) -> Option<&HeldMeta> {
        self.held.get(hash).map(|held| &held.meta)
    }

    pub fn is_waited_on(&self, hash: &BlockHash) -> bool { self.waiters.contains_key(hash) }

    #[cfg(test)]
    pub fn is_root_awaited(&self, root: &Blake2b256Hash) -> bool {
        self.root_waiters.contains_key(root)
    }

    pub fn root_waiters(&self, root: &Blake2b256Hash) -> Vec<BlockHash> {
        // The index drops a root with its last waiter, so absence means none.
        self.root_waiters
            .get(root)
            .map(|waiters| waiters.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn held(&self) -> usize { self.held.len() }

    pub fn disk_entries(&self) -> usize { self.disk_entries }

    pub fn is_quarantined(&self, hash: &BlockHash, now_ms: u64) -> bool {
        self.quarantined_until
            .get(hash)
            .is_some_and(|until| now_ms < *until)
    }

    /// The lowest ready block, now being worked on.
    pub fn next_ready(&mut self) -> Option<(BlockHash, Work)> {
        let key = self.ready.pop_first()?;
        let held = self
            .held
            .get_mut(&key.2)
            .expect("the ready set holds only held entries");
        held.stage = Stage::Working;
        Some((key.2, held.work))
    }

    /// `hash` is in the DAG: its own entry leaves and its waiters stop waiting
    /// on it. Returns the departure of its entry, if it was held.
    pub fn inserted(&mut self, hash: &BlockHash) -> Departure {
        self.failures.remove(hash);
        let mut departure = match self.held.contains_key(hash) {
            true => self.take(hash),
            false => Departure::default(),
        };
        if let Some(waiters) = self.waiters.remove(hash) {
            self.view.set_waited_on(hash, false);
            for waiter in waiters {
                let held = self
                    .held
                    .get_mut(&waiter)
                    .expect("a waiter is a held entry");
                held.missing.remove(hash);
                if self.promote_if_ready(&waiter) {
                    departure.released.push(waiter);
                }
            }
        }
        departure
    }

    /// A ready block whose shape now checks out waits on the rest of its
    /// dependencies.
    pub fn wait_on_dependencies(&mut self, hash: &BlockHash, missing: Vec<BlockHash>) {
        let held = self.held.get_mut(hash).expect("a released block is held");
        assert_eq!(held.stage, Stage::Ready, "only a ready block is held back");
        let key = held.ready_key();
        held.stage = Stage::Waiting;
        held.missing.extend(missing.iter().cloned());
        self.ready.remove(&key);
        for dependency in &missing {
            self.add_waiter(dependency, hash);
        }
        self.promote_if_ready(hash);
    }

    /// Whether `hash` still has its shape to check, clearing the mark.
    pub fn take_deferred(&mut self, hash: &BlockHash) -> bool {
        self.held
            .get_mut(hash)
            .is_some_and(|held| std::mem::take(&mut held.meta.deferred))
    }

    pub fn root_imported(&mut self, root: &Blake2b256Hash) {
        // A root no held block awaits releases nothing.
        for waiter in self.root_waiters.remove(root).unwrap_or_default() {
            let held = self
                .held
                .get_mut(&waiter)
                .expect("a root waiter is a held entry");
            held.awaiting_root = None;
            self.promote_if_ready(&waiter);
        }
    }

    /// Validation of `hash` named a block it still needs.
    pub fn wait_on_block(&mut self, hash: &BlockHash, missing: &BlockHash) {
        let held = self.working(hash);
        held.stage = Stage::Waiting;
        held.missing.insert(missing.clone());
        self.add_waiter(missing, hash);
    }

    /// Validation of `hash` needs a state root the node lacks.
    pub fn wait_on_root(&mut self, hash: &BlockHash, root: &Blake2b256Hash) {
        let held = self.working(hash);
        held.stage = Stage::Waiting;
        held.awaiting_root = Some(root.clone());
        self.root_waiters
            .entry(root.clone())
            .or_default()
            .insert(hash.clone());
    }

    /// A settled-history admission declined `hash`; it waits on `missing` and
    /// is validated like any other block.
    pub fn reclassify(&mut self, hash: &BlockHash, missing: Vec<BlockHash>) {
        let held = self.working(hash);
        held.work = Work::Validate;
        held.stage = Stage::Waiting;
        held.missing.extend(missing.iter().cloned());
        for dependency in &missing {
            self.add_waiter(dependency, hash);
        }
        self.promote_if_ready(hash);
    }

    /// A validation attempt of `hash` failed without a verdict. A retry waits
    /// `validation_retry_delay_ms`; the last failure quarantines the block.
    pub fn failed(&mut self, hash: &BlockHash, now_ms: u64) -> FailureVerdict {
        self.record_failure(hash, now_ms, true)
    }

    /// Validation of `hash` named a dependency the stores already satisfy, so
    /// its snapshot predates that dependency. The block is retried at once,
    /// but the attempt counts as a failure, which bounds a repeat.
    pub fn stale_snapshot(&mut self, hash: &BlockHash, now_ms: u64) -> FailureVerdict {
        self.record_failure(hash, now_ms, false)
    }

    fn record_failure(&mut self, hash: &BlockHash, now_ms: u64, paced: bool) -> FailureVerdict {
        let failures = {
            let failures = self.failures.entry(hash.clone()).or_insert(0);
            *failures += 1;
            *failures
        };
        self.working(hash).stage = Stage::Waiting;
        if failures >= MAX_VALIDATION_FAILURES {
            self.failures.remove(hash);
            self.quarantined_until
                .insert(hash.clone(), now_ms + VALIDATION_FAILURE_QUARANTINE_MS);
            return FailureVerdict::GiveUp;
        }
        if paced {
            let retry_at_ms = now_ms + validation_retry_delay_ms(failures);
            self.held
                .get_mut(hash)
                .expect("a failing block is held")
                .retry_at_ms = Some(retry_at_ms);
            self.cooling.insert((retry_at_ms, hash.clone()));
        } else {
            self.promote_if_ready(hash);
        }
        FailureVerdict::Retry
    }

    /// Failed blocks whose retry delay has passed become ready again.
    pub fn release_cooled(&mut self, now_ms: u64) {
        while let Some((due_ms, hash)) = self.cooling.first().cloned() {
            if due_ms > now_ms {
                return;
            }
            self.cooling.remove(&(due_ms, hash.clone()));
            self.held
                .get_mut(&hash)
                .expect("a cooling entry is held")
                .retry_at_ms = None;
            self.promote_if_ready(&hash);
        }
    }

    pub fn quarantined_until(&self, hash: &BlockHash) -> Option<u64> {
        self.quarantined_until.get(hash).copied()
    }

    pub fn forget_expired_quarantines(&mut self, now_ms: u64) {
        self.quarantined_until.retain(|_, until| now_ms < *until);
    }

    /// `hash` and every held block that waits on it, directly or through
    /// another, leave: none of them can become ready.
    pub fn drop_with_dependents(&mut self, hash: &BlockHash) -> Departure {
        let (seen, order) = self.waiting_closure(hash);
        let mut departure = Departure::default();
        for doomed in &order {
            let mut taken = self.take(doomed);
            departure.removed.append(&mut taken.removed);
            departure.unwaited.append(&mut taken.unwaited);
            departure.unawaited_roots.append(&mut taken.unawaited_roots);
        }
        if self.waiters.remove(hash).is_some() {
            self.view.set_waited_on(hash, false);
        }
        departure
            .unwaited
            .retain(|h| !seen.contains(h) && !self.waiters.contains_key(h));
        departure
    }

    /// The held blocks that wait on `hash`, directly or through another held
    /// block.
    pub fn dependents(&self, hash: &BlockHash) -> HashSet<BlockHash> {
        let (_, held) = self.waiting_closure(hash);
        held.into_iter().filter(|h| h != hash).collect()
    }

    /// Every hash reached from `hash` through the waiter index, and the held
    /// ones among them in the order reached.
    fn waiting_closure(&self, hash: &BlockHash) -> (HashSet<BlockHash>, Vec<BlockHash>) {
        let mut pending = vec![hash.clone()];
        let mut seen: HashSet<BlockHash> = HashSet::new();
        let mut held = Vec::new();
        while let Some(next) = pending.pop() {
            if !seen.insert(next.clone()) {
                continue;
            }
            if let Some(waiters) = self.waiters.get(&next) {
                pending.extend(waiters.iter().cloned());
            }
            if self.held.contains_key(&next) {
                held.push(next);
            }
        }
        (seen, held)
    }

    /// Evict `hash`. Blocks waiting on it keep waiting, so it becomes a need
    /// again if anything waits on it.
    pub fn evict(&mut self, hash: &BlockHash) -> Departure {
        assert!(
            self.held
                .get(hash)
                .is_some_and(|held| held.stage != Stage::Working),
            "only an entry no worker holds is evicted"
        );
        self.take(hash)
    }

    /// Held blocks an eviction pass may take, in eviction order: untrusted
    /// before trusted, then highest height first. Blocks being worked on are
    /// never candidates. A block that another held block waits on counts as
    /// solicited, however it arrived.
    pub fn victims(
        &self,
        body: BodyAt,
        class: VictimClass,
        sender: Option<&Validator>,
    ) -> Vec<(BlockHash, usize)> {
        let mut candidates: Vec<&Held> = self
            .held
            .values()
            .filter(|held| held.stage != Stage::Working)
            .filter(|held| held.meta.body == body)
            .filter(|held| {
                let solicited = held.meta.solicited || self.waiters.contains_key(&held.meta.hash);
                solicited == (class == VictimClass::Solicited)
            })
            .filter(|held| sender.is_none_or(|s| held.meta.sender == *s))
            .collect();
        candidates.sort_unstable_by_key(|held| {
            (
                held.meta.trusted,
                Reverse(held.meta.height),
                Reverse(held.seq),
            )
        });
        candidates
            .into_iter()
            .map(|held| (held.meta.hash.clone(), held.meta.size))
            .collect()
    }

    /// The block to evict when bodies on disk exceed their bound: one of the
    /// sender holding the most of them, so no sender can push out the others'
    /// blocks. Within that sender, eviction order applies.
    pub fn disk_victim(&self) -> Option<BlockHash> {
        let mut senders: Vec<(&Validator, usize)> = self
            .disk_by_sender
            .iter()
            .map(|(sender, count)| (sender, *count))
            .collect();
        senders.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        senders.into_iter().find_map(|(sender, _)| {
            self.held
                .values()
                .filter(|held| {
                    held.stage != Stage::Working
                        && held.meta.body == BodyAt::Disk
                        && held.meta.sender == *sender
                })
                .min_by_key(|held| {
                    (
                        held.meta.solicited || self.waiters.contains_key(&held.meta.hash),
                        held.meta.trusted,
                        Reverse(held.meta.height),
                        Reverse(held.seq),
                    )
                })
                .map(|held| held.meta.hash.clone())
        })
    }

    pub fn is_trusted(&self, hash: &BlockHash) -> bool {
        self.held.get(hash).is_some_and(|held| held.meta.trusted)
    }

    fn working(&mut self, hash: &BlockHash) -> &mut Held {
        let held = self
            .held
            .get_mut(hash)
            .expect("an outcome arrives only for a dispatched, held block");
        assert_eq!(
            held.stage,
            Stage::Working,
            "an outcome arrives only for a dispatched block"
        );
        held
    }

    fn promote_if_ready(&mut self, hash: &BlockHash) -> bool {
        let held = self.held.get_mut(hash).expect("promoted entries are held");
        let ready = held.stage == Stage::Waiting
            && held.missing.is_empty()
            && held.awaiting_root.is_none()
            && held.retry_at_ms.is_none();
        if ready {
            held.stage = Stage::Ready;
            self.ready.insert(held.ready_key());
        }
        ready
    }

    fn add_waiter(&mut self, dependency: &BlockHash, waiter: &BlockHash) {
        let waiters = self.waiters.entry(dependency.clone()).or_default();
        if waiters.is_empty() {
            self.view.set_waited_on(dependency, true);
        }
        waiters.insert(waiter.clone());
    }

    fn take(&mut self, hash: &BlockHash) -> Departure {
        let held = self.held.remove(hash).expect("only held entries are taken");
        self.view.set_held(hash, false, false);
        self.failures.remove(hash);
        if held.meta.body == BodyAt::Disk {
            self.disk_entries -= 1;
            let count = self
                .disk_by_sender
                .get_mut(&held.meta.sender)
                .expect("every disk entry is counted under its sender");
            *count -= 1;
            if *count == 0 {
                self.disk_by_sender.remove(&held.meta.sender);
            }
        }
        if held.stage == Stage::Ready {
            self.ready.remove(&held.ready_key());
        }
        if let Some(retry_at_ms) = held.retry_at_ms {
            self.cooling.remove(&(retry_at_ms, hash.clone()));
        }
        let mut unwaited = Vec::new();
        for dependency in &held.missing {
            let waiters = self
                .waiters
                .get_mut(dependency)
                .expect("a missing dependency is indexed");
            waiters.remove(hash);
            if waiters.is_empty() {
                self.waiters.remove(dependency);
                self.view.set_waited_on(dependency, false);
                unwaited.push(dependency.clone());
            }
        }
        let mut unawaited_roots = Vec::new();
        if let Some(root) = &held.awaiting_root {
            let waiters = self
                .root_waiters
                .get_mut(root)
                .expect("an awaited root is indexed");
            waiters.remove(hash);
            if waiters.is_empty() {
                self.root_waiters.remove(root);
                unawaited_roots.push(root.clone());
            }
        }
        Departure {
            removed: vec![held.meta],
            released: Vec::new(),
            unwaited,
            unawaited_roots,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use prost::bytes::Bytes;

    use super::*;

    pub(crate) fn hash(i: u32) -> BlockHash { Bytes::copy_from_slice(&i.to_be_bytes()) }

    pub(crate) fn meta(i: u32, height: i64, deps: &[u32]) -> HeldMeta {
        HeldMeta {
            hash: hash(i),
            sender: Bytes::from_static(b"sender"),
            height,
            deps: deps.iter().copied().map(hash).collect(),
            parents: deps.iter().copied().map(hash).collect(),
            justification_validators: Vec::new(),
            deferred: false,
            body: BodyAt::Disk,
            size: 100,
            trusted: true,
            bonded_at_anchor: true,
            solicited: false,
            pushed_by: None,
        }
    }

    fn tracker() -> DependencyTracker { DependencyTracker::new(IngestView::new()) }

    fn hold(t: &mut DependencyTracker, i: u32, height: i64, missing: &[u32]) {
        t.hold(
            meta(i, height, missing),
            Work::Validate,
            missing.iter().copied().map(hash).collect(),
        )
    }

    #[test]
    fn ready_blocks_are_dispatched_lowest_height_first() {
        let mut t = tracker();
        hold(&mut t, 1, 9, &[]);
        hold(&mut t, 2, 3, &[]);
        hold(&mut t, 3, 5, &[]);
        let order: Vec<BlockHash> = std::iter::from_fn(|| t.next_ready().map(|(h, _)| h)).collect();
        assert_eq!(order, vec![hash(2), hash(3), hash(1)]);
    }

    fn as_set(hashes: Vec<BlockHash>) -> HashSet<BlockHash> { hashes.into_iter().collect() }

    #[test]
    fn a_block_is_ready_once_its_last_dependency_is_inserted() {
        let mut t = tracker();
        hold(&mut t, 10, 5, &[1, 2]);
        hold(&mut t, 11, 5, &[1]);
        assert!(t.is_waited_on(&hash(1)) && t.is_waited_on(&hash(2)));
        assert!(t.next_ready().is_none());
        t.inserted(&hash(1));
        assert_eq!(t.next_ready(), Some((hash(11), Work::Validate)));
        t.inserted(&hash(2));
        assert_eq!(t.next_ready(), Some((hash(10), Work::Validate)));
        assert!(!t.is_waited_on(&hash(1)) && !t.is_waited_on(&hash(2)));
    }

    #[test]
    fn the_view_mirrors_held_and_waited_on_blocks() {
        let view = IngestView::new();
        let mut t = DependencyTracker::new(view.clone());
        let mut solicited = meta(1, 1, &[9]);
        solicited.solicited = true;
        t.hold(solicited, Work::Validate, vec![hash(9)]);
        hold(&mut t, 2, 2, &[1]);
        assert!(view.is_held(&hash(1)) && view.is_solicited(&hash(1)));
        assert!(view.is_held(&hash(2)) && !view.is_solicited(&hash(2)));
        assert!(view.has_waiters(&hash(9)) && view.has_waiters(&hash(1)));
        t.evict(&hash(1));
        assert!(!view.is_held(&hash(1)) && !view.is_solicited(&hash(1)));
        assert!(!view.has_waiters(&hash(9)) && view.has_waiters(&hash(1)));
        t.drop_with_dependents(&hash(1));
        assert!(!view.is_held(&hash(2)) && !view.has_waiters(&hash(1)));
    }

    #[test]
    fn an_inserted_held_block_leaves_and_releases_its_waiters() {
        let mut t = tracker();
        hold(&mut t, 1, 1, &[]);
        hold(&mut t, 2, 2, &[1]);
        assert_eq!(t.next_ready(), Some((hash(1), Work::Validate)));
        let departure = t.inserted(&hash(1));
        assert_eq!(departure.removed, vec![meta(1, 1, &[])]);
        assert!(!t.contains(&hash(1)));
        assert_eq!(t.next_ready(), Some((hash(2), Work::Validate)));
    }

    #[test]
    fn a_block_waiting_on_a_state_root_is_ready_when_the_root_is_imported() {
        let mut t = tracker();
        let root = Blake2b256Hash::new(b"root");
        hold(&mut t, 1, 1, &[]);
        t.next_ready();
        t.wait_on_root(&hash(1), &root);
        assert!(t.is_root_awaited(&root));
        assert!(t.next_ready().is_none());
        t.root_imported(&root);
        assert_eq!(t.next_ready(), Some((hash(1), Work::Validate)));
        assert!(!t.is_root_awaited(&root));
    }

    #[test]
    fn dropping_a_block_drops_everything_that_waits_on_it() {
        let mut t = tracker();
        hold(&mut t, 1, 1, &[]);
        hold(&mut t, 2, 2, &[1, 7]);
        hold(&mut t, 3, 3, &[2]);
        hold(&mut t, 4, 2, &[7]);
        t.next_ready();
        let departure = t.drop_with_dependents(&hash(1));
        assert_eq!(
            as_set(departure.removed.iter().map(|m| m.hash.clone()).collect()),
            as_set(vec![hash(1), hash(2), hash(3)])
        );
        assert!(departure.unwaited.is_empty(), "block 4 still waits on 7");
        assert!(t.contains(&hash(4)) && t.is_waited_on(&hash(7)));
        assert!(!t.is_waited_on(&hash(1)) && !t.is_waited_on(&hash(2)));
        assert_eq!(t.held(), 1);
    }

    #[test]
    fn an_evicted_block_stays_waited_on_by_its_waiters() {
        let mut t = tracker();
        hold(&mut t, 1, 1, &[9]);
        hold(&mut t, 2, 2, &[1]);
        let departure = t.evict(&hash(1));
        assert_eq!(departure.unwaited, vec![hash(9)]);
        assert!(t.is_waited_on(&hash(1)));
        t.inserted(&hash(1));
        assert_eq!(t.next_ready(), Some((hash(2), Work::Validate)));
    }

    #[test]
    fn victims_are_untrusted_first_then_highest_first_and_never_in_work() {
        let mut t = tracker();
        let mut entry = |i: u32, height: i64, trusted: bool| {
            let mut m = meta(i, height, &[]);
            m.trusted = trusted;
            m.body = BodyAt::Pending;
            t.hold(m, Work::Validate, vec![hash(1_000 + i)]);
        };
        entry(1, 5, true);
        entry(2, 9, true);
        entry(3, 1, false);
        entry(4, 7, false);
        let order: Vec<BlockHash> = t
            .victims(BodyAt::Pending, VictimClass::Unsolicited, None)
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        assert_eq!(order, vec![hash(4), hash(3), hash(2), hash(1)]);
        assert!(t
            .victims(BodyAt::Pending, VictimClass::Solicited, None)
            .is_empty());
        assert!(t
            .victims(BodyAt::Disk, VictimClass::Unsolicited, None)
            .is_empty());
    }

    #[test]
    fn a_held_block_another_held_block_waits_on_counts_as_solicited() {
        let mut t = tracker();
        hold(&mut t, 1, 1, &[]);
        hold(&mut t, 2, 2, &[1]);
        let hashes = |class| -> Vec<BlockHash> {
            t.victims(BodyAt::Disk, class, None)
                .into_iter()
                .map(|(h, _)| h)
                .collect()
        };
        assert_eq!(hashes(VictimClass::Unsolicited), vec![hash(2)]);
        assert_eq!(hashes(VictimClass::Solicited), vec![hash(1)]);
    }

    #[test]
    fn a_failing_block_is_retried_then_quarantined() {
        let mut t = tracker();
        hold(&mut t, 1, 1, &[]);
        for _ in 1..MAX_VALIDATION_FAILURES {
            assert!(t.next_ready().is_some());
            assert_eq!(t.failed(&hash(1), 0), FailureVerdict::Retry);
            assert!(t.next_ready().is_none(), "a retry waits out its delay");
            t.release_cooled(VALIDATION_FAILURE_QUARANTINE_MS);
        }
        t.next_ready();
        assert_eq!(t.failed(&hash(1), 0), FailureVerdict::GiveUp);
        assert!(t.is_quarantined(&hash(1), VALIDATION_FAILURE_QUARANTINE_MS - 1));
        assert!(!t.is_quarantined(&hash(1), VALIDATION_FAILURE_QUARANTINE_MS));
    }

    #[test]
    fn an_insert_clears_the_failure_count() {
        let mut t = tracker();
        hold(&mut t, 1, 1, &[]);
        for _ in 1..MAX_VALIDATION_FAILURES {
            t.next_ready();
            assert_eq!(t.failed(&hash(1), 0), FailureVerdict::Retry);
            t.release_cooled(VALIDATION_FAILURE_QUARANTINE_MS);
        }
        t.inserted(&hash(1));
        hold(&mut t, 1, 1, &[]);
        for _ in 1..MAX_VALIDATION_FAILURES {
            t.next_ready();
            assert_eq!(t.failed(&hash(1), 0), FailureVerdict::Retry);
            t.release_cooled(VALIDATION_FAILURE_QUARANTINE_MS);
        }
    }

    #[test]
    fn a_declined_settled_admission_waits_and_is_validated() {
        let mut t = tracker();
        t.hold(meta(1, 1, &[2]), Work::AdmitSettled, vec![]);
        assert_eq!(t.next_ready(), Some((hash(1), Work::AdmitSettled)));
        t.reclassify(&hash(1), vec![hash(2)]);
        assert!(t.is_waited_on(&hash(2)));
        assert!(t.next_ready().is_none());
        t.inserted(&hash(2));
        assert_eq!(t.next_ready(), Some((hash(1), Work::Validate)));
    }

    #[test]
    fn disk_entries_are_counted_as_they_come_and_go() {
        let mut t = tracker();
        hold(&mut t, 1, 1, &[5]);
        let mut pending = meta(2, 1, &[5]);
        pending.body = BodyAt::Pending;
        t.hold(pending, Work::Validate, vec![hash(5)]);
        assert_eq!(t.disk_entries(), 1);
        t.evict(&hash(1));
        assert_eq!(t.disk_entries(), 0);
    }
}
