use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ClaimFamily, ClaimId, FreezeInterval,
};
use models::rust::validator::Validator;

use super::claims::{ClaimRecord, ClaimTerms};
use super::dishonest_vote::{Accusation, PendingAccusation};
use super::reward::{committee_of, compute_reward, Committee, ParentFact, RewardParams, PPM};
use crate::rust::rholang::runtime::FloorState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeakyBucketParams {
    /// Fraction of capacity a bucket loses per observed block.
    pub lambda_ppm: u64,
    /// Level below which a validator is a candidate for ejection, as a
    /// fraction of capacity.
    pub theta_eject_ppm: u64,
    /// How many observed blocks a level must stay below the threshold before
    /// the validator is ejectable.
    pub eject_observation_blocks: u64,
    /// Blocks an ejection claim stays open before the contract lets another be
    /// opened against the same target.
    pub claim_deadline_blocks: i64,
    /// How far below the finalized floor a block can still be judged. The
    /// view keeps the blocks it observed back to there; `None` keeps them all.
    pub retention_window_blocks: Option<i64>,
    /// Most votes one block may carry.
    pub vote_list_bound: NonZeroU32,
    pub reward: RewardParams,
}

#[cfg(any(test, feature = "test-utils"))]
impl LeakyBucketParams {
    /// The values the shipped genesis carries, for fixtures that hold no chain
    /// state to read them from.
    pub fn shipped() -> Self {
        crate::rust::chain_parameters::ChainParameters::shipped().leaky_bucket
    }
}

/// What one floor's state records about the buckets: each validator's
/// freezes, oldest first, and the overflow it has been paid in all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Knowledge {
    floor_number: i64,
    freezes: HashMap<Validator, Vec<FreezeInterval>>,
    claimed_total: HashMap<Validator, i64>,
}

impl Knowledge {
    /// What `floor`, the state of block `floor_number`, records.
    pub fn of(floor: &FloorState, floor_number: i64) -> Self {
        Self {
            floor_number,
            freezes: floor.freezes.iter().cloned().collect(),
            claimed_total: floor
                .claimed
                .iter()
                .map(|claimed| (claimed.validator.clone(), claimed.total))
                .collect(),
        }
    }

    /// A validator the state records no ejection for has never been frozen.
    pub fn freezes_of(&self, validator: &Validator) -> &[FreezeInterval] {
        self.freezes
            .get(validator)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// A validator the state records no payment for has been paid nothing.
    pub fn claimed_total(&self, validator: &Validator) -> i64 {
        self.claimed_total.get(validator).copied().unwrap_or(0)
    }

    fn is_ejected(&self, validator: &Validator) -> bool {
        self.freezes_of(validator)
            .iter()
            .any(|interval| interval.rejoin.is_none())
    }
}

/// The freezes in effect for `validator` under `knowledge`: those its floor
/// records, and every closed one in `remembered` that the floor no longer
/// records because the epoch sweep dropped it for age. The sweep drops a
/// freeze only once its rejoin plus the retention window lies below the
/// sweeping block, which is at or below the floor; a closed freeze the floor
/// lacks for any other reason opened above the floor, after its rejoin.
fn effective<'k>(
    knowledge: &'k Knowledge,
    remembered: &HashMap<Validator, Vec<FreezeInterval>>,
    validator: &Validator,
    window: Option<i64>,
) -> Cow<'k, [FreezeInterval]> {
    let recorded = knowledge.freezes_of(validator);
    let (Some(window), Some(known)) = (window, remembered.get(validator)) else {
        return Cow::Borrowed(recorded);
    };
    let aged: Vec<FreezeInterval> = known
        .iter()
        .filter(|interval| {
            interval
                .rejoin
                .is_some_and(|rejoin| rejoin.at.saturating_add(window) < knowledge.floor_number)
                && !recorded.iter().any(|kept| kept.from == interval.from)
        })
        .copied()
        .collect();
    if aged.is_empty() {
        return Cow::Borrowed(recorded);
    }
    let mut all = aged;
    all.extend_from_slice(recorded);
    all.sort_by_key(|interval| interval.from);
    Cow::Owned(all)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BucketEntry {
    /// In units of `1 / PPM` of a bond unit: the drain is a fraction of
    /// capacity and would round to nothing in whole bond units for any bond
    /// below `1 / lambda`, which is every bond the test genesis assigns.
    level: u128,
    /// Same units as the level: everything credited, before any payment is
    /// taken off.
    overflow: u128,
    capacity: NonZeroU64,
    /// Consecutive observations below the threshold, counted only while this
    /// validator is in the observed block's committee and not frozen, so a
    /// frozen bucket neither advances its window nor forgets it.
    blocks_below_threshold: u64,
    /// The block of the latest rejoin this entry has applied.
    refilled_through: Option<i64>,
}

impl BucketEntry {
    /// Starts full: the level answers "is this validator earning now?", and a
    /// validator that has just joined the committee has not yet had the
    /// chance to fail. Starting at zero would make every new member
    /// immediately ejectable.
    fn full(capacity: NonZeroU64) -> Self {
        Self {
            level: Self::scaled(capacity),
            overflow: 0,
            capacity,
            blocks_below_threshold: 0,
            refilled_through: None,
        }
    }

    /// Starts at the ejection threshold, for a validator first seen while
    /// frozen: the ejection is the record that more than two thirds of its
    /// electorate held it below this level.
    fn at_threshold(capacity: NonZeroU64, theta_eject_ppm: u64) -> Self {
        Self {
            level: capacity.get() as u128 * theta_eject_ppm as u128,
            ..Self::full(capacity)
        }
    }

    fn scaled(capacity: NonZeroU64) -> u128 { capacity.get() as u128 * PPM as u128 }

    fn threshold(&self, theta_eject_ppm: u64) -> u128 {
        self.capacity.get() as u128 * theta_eject_ppm as u128
    }

    fn decay(&mut self, lambda_ppm: u64) {
        self.level = self
            .level
            .saturating_sub(self.capacity.get() as u128 * lambda_ppm as u128);
    }

    fn fill(&mut self, reward: u128) {
        let headroom = Self::scaled(self.capacity).saturating_sub(self.level);
        let into_level = reward.min(headroom);
        self.level += into_level;
        self.overflow = self.overflow.saturating_add(reward - into_level);
    }

    /// Holds the level's share of capacity fixed, so a bond change moves a
    /// validator neither towards nor away from its own threshold. Clamping
    /// instead would let a draining validator refill by unbonding.
    fn reseat(&mut self, capacity: NonZeroU64) {
        if capacity != self.capacity {
            // Same floor as `level * new / old` without forming that product,
            // which exceeds `u128` at the shipped `bond-maximum` of `i64::MAX`.
            let (old, new) = (self.capacity.get() as u128, capacity.get() as u128);
            self.level = (self.level / old) * new + (self.level % old) * new / old;
            self.capacity = capacity;
        }
    }

    /// A rejoin fills the bucket to capacity, and whatever of `credit` that
    /// refill did not need becomes overflow.
    fn rejoin(&mut self, credit: u128) {
        let full = Self::scaled(self.capacity);
        let refill = full.saturating_sub(self.level);
        self.level = full;
        self.overflow = self.overflow.saturating_add(credit.saturating_sub(refill));
        self.blocks_below_threshold = 0;
    }

    /// Applies every rejoin paid below block `before` that this entry has not
    /// applied yet. One paid at or below `uncredited_through` predates the
    /// view, so it refills the level and credits nothing.
    fn refill(&mut self, freezes: &[FreezeInterval], before: i64, uncredited_through: i64) {
        for rejoin in freezes.iter().filter_map(|interval| interval.rejoin) {
            if rejoin.at < before && self.refilled_through.is_none_or(|at| rejoin.at > at) {
                let credit = if rejoin.at > uncredited_through {
                    rejoin.topup.max(0) as u128 * PPM as u128
                } else {
                    0
                };
                self.rejoin(credit);
                self.refilled_through = Some(rejoin.at);
            }
        }
    }

    /// Records the rejoins paid below block `before` as applied, for an entry
    /// that starts full there.
    fn mark_refilled(&mut self, freezes: &[FreezeInterval], before: i64) {
        self.refilled_through = freezes
            .iter()
            .filter_map(|interval| interval.rejoin)
            .map(|rejoin| rejoin.at)
            .filter(|at| *at < before)
            .max();
    }

    /// Below the ejection threshold right now, however briefly.
    fn is_drained(&self, theta_eject_ppm: u64) -> bool {
        self.level < self.threshold(theta_eject_ppm)
    }

    fn observe_threshold(&mut self, theta_eject_ppm: u64) {
        if self.is_drained(theta_eject_ppm) {
            self.blocks_below_threshold = self.blocks_below_threshold.saturating_add(1);
        } else {
            self.blocks_below_threshold = 0;
        }
    }
}

/// What this node's view decides for one proposal, under the parameters it
/// was decided with: the ejections it offers, the validators it votes to
/// eject, how far below capacity each bucket stands, which a rejoin's topup
/// must cover, and the overflow each validator is owed, which a claim may not
/// exceed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeakyBucketProposal {
    pub params: LeakyBucketParams,
    pub ejection_offers: Vec<Validator>,
    pub drained: HashSet<Validator>,
    /// Capacity less level, in units of `1 / PPM` of a bond unit.
    pub shortfall: HashMap<Validator, u128>,
    /// Overflow less what has been paid, in units of `1 / PPM` of a bond unit.
    pub overflow: HashMap<Validator, u128>,
    /// Nays on overflow claims this node has seen from validators its latest
    /// floor still shows holding stake, by accused and then in the order this
    /// node accepted them. Each is offered only if the block's floor proves it.
    pub accusations: Vec<PendingAccusation>,
}

impl LeakyBucketProposal {
    pub fn votes_yea(&self, record: &ClaimRecord) -> bool {
        match record.terms {
            ClaimTerms::Eject => self.drained.contains(&record.claim.target),
            ClaimTerms::Rejoin { topup, .. } => {
                // A validator with no bucket here is full, as a new entry
                // starts (`BucketEntry::full`).
                let shortfall = self
                    .shortfall
                    .get(&record.claim.target)
                    .copied()
                    .unwrap_or(0);
                (topup.max(0) as u128).saturating_mul(PPM as u128) >= shortfall
            }
            ClaimTerms::Overflow { amount } => {
                // A validator with no bucket here has earned nothing in this
                // view, as a new entry starts with no overflow.
                let overflow = self
                    .overflow
                    .get(&record.claim.target)
                    .copied()
                    .unwrap_or(0);
                overflow >= (amount.max(0) as u128).saturating_mul(PPM as u128)
            }
        }
    }
}

/// The view as it stands: every tracked bucket under the latest reconciled
/// floor, the committee at the tip, and the ejection claims still open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewSnapshot {
    pub reconciled_floor: i64,
    pub credited_from: i64,
    pub tip_height: i64,
    /// In identity order.
    pub tip_committee: Vec<Validator>,
    /// In identity order.
    pub buckets: Vec<BucketSnapshot>,
    /// Target and the block its claim was opened at, in identity order.
    pub open_ejection_claims: Vec<(Validator, i64)>,
    /// In identity order.
    pub slashed: Vec<Validator>,
}

/// One validator's bucket. Level, threshold and overflow are in units of
/// `1 / PPM` of a bond unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BucketSnapshot {
    pub validator: Validator,
    pub capacity: u64,
    pub level: u128,
    pub threshold: u128,
    pub drained: bool,
    /// Consecutive observations below the threshold.
    pub blocks_below_threshold: u64,
    /// Everything credited, before payments are taken off.
    pub overflow: u128,
    /// Bond units paid out of the overflow since the view began crediting.
    pub paid: i64,
    pub ejected: bool,
}

/// Decides a block's offers and votes from what its floor's state records.
pub type LeakyBucketDecision = Arc<dyn Fn(&Knowledge) -> LeakyBucketProposal + Send + Sync>;

#[cfg(any(test, feature = "test-utils"))]
impl LeakyBucketProposal {
    /// Offers nothing, finds nobody drained and every bucket full.
    pub fn abstaining(params: LeakyBucketParams) -> Self {
        Self {
            params,
            ejection_offers: Vec::new(),
            drained: HashSet::new(),
            shortfall: HashMap::new(),
            overflow: HashMap::new(),
            accusations: Vec::new(),
        }
    }

    /// This proposal, whatever the floor records.
    pub fn fixed(self) -> LeakyBucketDecision { Arc::new(move |_| self.clone()) }
}

/// What one accepted block contributes to the buckets. `committee` is the
/// block's own `bonds` field, which `Validate::bonds_cache_from_floor` has
/// already checked against the committee on the block's floor.
#[derive(Clone, Debug, PartialEq)]
pub struct Observation {
    pub block_hash: BlockHash,
    pub sender: Validator,
    pub block_number: i64,
    pub committee: Committee,
    pub parents: Vec<ParentFact>,
    /// The other members whose new blocks this block acknowledges since its
    /// sender's previous one.
    pub fresh: Vec<Validator>,
    /// The ejection claims this block votes on. Every block after a claim's
    /// opening owes a vote on it, so the votes show which claims are open,
    /// which stops this node asking for a second one the contract would
    /// refuse.
    pub ejection_claims: Vec<ClaimId>,
    /// The overflow claims this block's sender voted nay on.
    pub overflow_nays: Vec<ClaimId>,
}

impl Observation {
    /// What `block`, whose parents are `parents` and whose newly acknowledged
    /// members are `fresh`, contributes.
    pub fn of(block: &BlockMessage, parents: &[BlockMetadata], fresh: Vec<Validator>) -> Self {
        Self {
            block_hash: block.block_hash.clone(),
            sender: block.sender.clone(),
            block_number: block.body.state.block_number,
            committee: committee_of(&block.body.state.bonds),
            parents: parents
                .iter()
                .map(|parent| ParentFact {
                    sender: parent.sender.clone(),
                    block_number: parent.block_number,
                })
                .collect(),
            fresh,
            ejection_claims: block
                .body
                .leaky_bucket_votes
                .iter()
                .filter(|vote| vote.claim.family == ClaimFamily::Eject)
                .map(|vote| vote.claim.clone())
                .collect(),
            overflow_nays: block
                .body
                .leaky_bucket_votes
                .iter()
                .filter(|vote| vote.testimony.is_some())
                .map(|vote| vote.claim.clone())
                .collect(),
        }
    }
}

/// One observed block as the buckets need it.
struct Logged {
    number: i64,
    sender: Validator,
    committee: Arc<BTreeMap<Validator, NonZeroU64>>,
    reward: u128,
}

impl LeakyBucketParams {
    /// Folds one block into `validator`'s entry. A block whose committee
    /// leaves the validator out does not touch it: it stops draining and
    /// keeps its overflow, which a validator on its way out of the bond can
    /// still claim. A block numbered inside one of its freezes only seats it.
    fn step(
        &self,
        entry: &mut Option<BucketEntry>,
        validator: &Validator,
        block: &Logged,
        freezes: &[FreezeInterval],
        uncredited_through: i64,
    ) {
        let Some(capacity) = block.committee.get(validator).copied() else {
            return;
        };
        let frozen = freezes.iter().any(|interval| interval.covers(block.number));
        let entry = match entry {
            Some(entry) => {
                entry.refill(freezes, block.number, uncredited_through);
                entry.reseat(capacity);
                if frozen {
                    return;
                }
                entry.decay(self.lambda_ppm);
                entry
            }
            None => {
                let mut fresh = if frozen {
                    BucketEntry::at_threshold(capacity, self.theta_eject_ppm)
                } else {
                    BucketEntry::full(capacity)
                };
                fresh.mark_refilled(freezes, block.number);
                let entry = entry.insert(fresh);
                if frozen {
                    return;
                }
                entry
            }
        };
        if block.sender == *validator {
            entry.fill(block.reward);
        }
        entry.observe_threshold(self.theta_eject_ppm);
    }
}

/// This node's view of every committee member's earning capacity, one bucket
/// per validator. Node-local, not consensus state: levels depend on which
/// blocks this node accepted and in what order, so two nodes legitimately
/// disagree about the same validator. What a floor records — the freezes and
/// the payments — is objective, and the view answers under any floor's record.
pub struct BucketView {
    state: parking_lot::Mutex<ViewState>,
    params: LeakyBucketParams,
}

#[derive(Default)]
struct ViewState {
    /// Each tracked validator's entry after `base` and the whole log, under
    /// `known`. A rejoin no block above it has followed yet is applied on read.
    live: HashMap<Validator, BucketEntry>,
    /// Each entry after the blocks pruned from the front of the log.
    base: HashMap<Validator, BucketEntry>,
    /// The observed blocks not yet pruned, in the order this node accepted
    /// them, which is not their number order.
    log: VecDeque<Logged>,
    /// What the latest reconciled floor records.
    known: Knowledge,
    /// The closed freezes the reconciled floors have recorded, kept while the
    /// validator holds stake, so a freeze the epoch sweep drops still covers
    /// its blocks, including one admitted late.
    remembered: HashMap<Validator, Vec<FreezeInterval>>,
    baseline: Baseline,
    /// Target to the block number at which the latest ejection claim against
    /// it was opened.
    claims: HashMap<Validator, i64>,
    /// Each validator's nays on overflow claims, with the number of the block
    /// that carries each. They are kept until the validator's stake leaves
    /// the system, since the block carrying evidence against it may never
    /// become canonical.
    accusations: BTreeMap<Validator, Vec<(i64, PendingAccusation)>>,
    /// Validators a floor has shown slashed: their key holds no stake, and
    /// it stays in the bonds so it can never bond again. A late block that
    /// still lists one is not let back into the view.
    slashed: HashSet<Validator>,
    tip: Tip,
}

/// Where the view's credit begins: the first block number it observes, and
/// what each validator was paid before the view began crediting it. A rejoin
/// paid below that block refilled a level the view never held, and a payment
/// made before it drew on overflow the view never credited.
#[derive(Default)]
struct Baseline {
    credited_from: i64,
    paid_before: HashMap<Validator, i64>,
}

/// The highest-numbered block observed: its committee is who can currently be
/// ejected.
#[derive(Default)]
struct Tip {
    height: i64,
    committee: HashSet<Validator>,
}

impl ViewState {
    fn uncredited_through(&self) -> i64 { self.baseline.credited_from.saturating_sub(1) }

    /// Whether the latest ejection claim known against `target` still holds
    /// its slot: it does until a floor reaches its deadline.
    fn ejection_claim_open(&self, target: &Validator, deadline: i64) -> bool {
        self.claims
            .get(target)
            .is_some_and(|opened| opened.saturating_add(deadline) > self.known.floor_number)
    }

    /// What `knowledge` shows `validator` paid since the view's credit began.
    fn paid_since_baseline(&self, validator: &Validator, knowledge: &Knowledge) -> i64 {
        // Paid nothing before the view began when the starting floor recorded
        // no payment to it.
        let before = self
            .baseline
            .paid_before
            .get(validator)
            .copied()
            .unwrap_or(0);
        knowledge
            .claimed_total(validator)
            .saturating_sub(before)
            .max(0)
    }

    fn replay(
        &self,
        params: &LeakyBucketParams,
        validator: &Validator,
        freezes: &[FreezeInterval],
    ) -> Option<BucketEntry> {
        let uncredited_through = self.uncredited_through();
        let mut entry = self.base.get(validator).copied();
        for block in &self.log {
            params.step(&mut entry, validator, block, freezes, uncredited_through);
        }
        entry
    }
}

impl BucketView {
    pub fn new(params: LeakyBucketParams) -> Self {
        Self {
            state: parking_lot::Mutex::new(ViewState::default()),
            params,
        }
    }

    pub fn params(&self) -> &LeakyBucketParams { &self.params }

    /// Starts the view at `floor`, the state of block `floor_number`, for a
    /// node that will observe the blocks it holds from `credited_from` up. A
    /// view that is never begun credits from block 0 with nothing paid
    /// before it, as a node starting from genesis does.
    ///
    /// The floor records every payment made at or above `credited_from`, since
    /// the epoch sweep drops one only once it lies a retention window below
    /// the sweeping block, and `credited_from` is no further below the floor.
    pub fn begin(&self, floor: &FloorState, floor_number: i64, credited_from: i64) {
        let paid_before = floor
            .claimed
            .iter()
            .map(|claimed| {
                let since: i64 = claimed
                    .payments
                    .iter()
                    .filter(|payment| payment.at >= credited_from)
                    .map(|payment| payment.amount)
                    .sum();
                (
                    claimed.validator.clone(),
                    claimed.total.saturating_sub(since),
                )
            })
            .collect();
        self.state.lock().baseline = Baseline {
            credited_from,
            paid_before,
        };
        self.reconcile(floor, floor_number);
    }

    /// Whether the latest ejection claim this view knows against `target` is
    /// still open.
    pub fn ejection_claim_open(&self, target: &Validator) -> bool {
        self.state
            .lock()
            .ejection_claim_open(target, self.params.claim_deadline_blocks)
    }

    /// Validators ejected as of the latest reconciled floor.
    pub fn ejected(&self) -> HashSet<Validator> {
        let state = self.state.lock();
        state
            .known
            .freezes
            .keys()
            .filter(|validator| state.known.is_ejected(validator))
            .cloned()
            .collect()
    }

    /// The view as it stands, with every rejoin the floor records applied.
    pub fn snapshot(&self) -> ViewSnapshot {
        let state = self.state.lock();
        let uncredited_through = state.uncredited_through();
        let mut buckets: Vec<BucketSnapshot> = state
            .live
            .iter()
            .map(|(validator, live)| {
                let mut entry = *live;
                entry.refill(
                    &effective(
                        &state.known,
                        &state.remembered,
                        validator,
                        self.params.retention_window_blocks,
                    ),
                    i64::MAX,
                    uncredited_through,
                );
                BucketSnapshot {
                    validator: validator.clone(),
                    capacity: entry.capacity.get(),
                    level: entry.level,
                    threshold: entry.threshold(self.params.theta_eject_ppm),
                    drained: entry.is_drained(self.params.theta_eject_ppm),
                    blocks_below_threshold: entry.blocks_below_threshold,
                    overflow: entry.overflow,
                    paid: state.paid_since_baseline(validator, &state.known),
                    ejected: state.known.is_ejected(validator),
                }
            })
            .collect();
        buckets.sort_by(|a, b| a.validator.cmp(&b.validator));
        let mut tip_committee: Vec<Validator> = state.tip.committee.iter().cloned().collect();
        tip_committee.sort();
        let mut open_ejection_claims: Vec<(Validator, i64)> = state
            .claims
            .iter()
            .filter(|(target, _)| {
                state.ejection_claim_open(target, self.params.claim_deadline_blocks)
            })
            .map(|(target, opened)| (target.clone(), *opened))
            .collect();
        open_ejection_claims.sort();
        let mut slashed: Vec<Validator> = state.slashed.iter().cloned().collect();
        slashed.sort();
        ViewSnapshot {
            reconciled_floor: state.known.floor_number,
            credited_from: state.baseline.credited_from,
            tip_height: state.tip.height,
            tip_committee,
            buckets,
            open_ejection_claims,
            slashed,
        }
    }

    /// The decision this view makes for `own`'s proposals.
    pub fn decision(self: &Arc<Self>, own: Validator) -> LeakyBucketDecision {
        let view = self.clone();
        Arc::new(move |knowledge| view.proposal_at(&own, knowledge))
    }

    /// Folds one accepted block in: every member's bucket drains by one
    /// block, and the sender's refills by the block's reward.
    pub fn observe(&self, observation: &Observation) {
        let committee: BTreeMap<Validator, NonZeroU64> = observation
            .committee
            .iter()
            .filter_map(|(validator, stake)| {
                NonZeroU64::new((*stake).max(0) as u64).map(|stake| (validator.clone(), stake))
            })
            .collect();
        if committee.is_empty() {
            return;
        }
        let reward = compute_reward(
            &observation.committee,
            &observation.sender,
            &observation.fresh,
            &observation.parents,
            observation.block_number,
            &self.params.reward,
        );

        let mut state = self.state.lock();
        if observation.block_number >= state.tip.height {
            state.tip.height = observation.block_number;
            state.tip.committee = committee.keys().cloned().collect();
        }
        for claim in &observation.ejection_claims {
            // Admission order is not block order, so a claim learned late can
            // be older than the one already recorded.
            let opened = state
                .claims
                .entry(claim.target.clone())
                .or_insert(claim.opened_at);
            *opened = (*opened).max(claim.opened_at);
        }
        let sender_slashed = state.slashed.contains(&observation.sender);
        for claim in observation.overflow_nays.iter().filter(|_| !sender_slashed) {
            state
                .accusations
                .entry(observation.sender.clone())
                .or_default()
                .push((
                    observation.block_number,
                    PendingAccusation::new(Accusation {
                        accused: observation.sender.clone(),
                        vote_block: observation.block_hash.clone(),
                        claim: claim.clone(),
                    }),
                ));
        }

        let committee = match state.log.back() {
            Some(last) if *last.committee == committee => last.committee.clone(),
            _ => Arc::new(committee),
        };
        let block = Logged {
            number: observation.block_number,
            sender: observation.sender.clone(),
            committee,
            reward,
        };
        let uncredited_through = state.uncredited_through();
        let window = self.params.retention_window_blocks;
        let ViewState {
            live,
            known,
            remembered,
            slashed,
            ..
        } = &mut *state;
        for validator in block.committee.keys().filter(|v| !slashed.contains(*v)) {
            let mut entry = live.get(validator).copied();
            self.params.step(
                &mut entry,
                validator,
                &block,
                &effective(known, remembered, validator, window),
                uncredited_through,
            );
            if let Some(entry) = entry {
                live.insert(validator.clone(), entry);
            }
        }
        state.log.push_back(block);
    }

    /// Brings the view into line with the finalized floor's state, block
    /// `floor_number`.
    ///
    /// Drops every entry whose validator no longer holds a bond in the system,
    /// keeping those that do: seated, unseated-but-bonded, and unbonding in
    /// quarantine, the last of which may still have overflow to claim.
    /// Membership is read as positive stake rather than key presence, because
    /// slashing leaves a zero-stake key in `allBonds` as a tombstone against
    /// re-bonding and the epoch re-pick puts that key back into
    /// `activeValidators`.
    ///
    /// A validator whose freezes the floor records differently from before is
    /// rebuilt from the log, so a freeze covers the same blocks whenever this
    /// node learns of it. The floor's ejection records replace whatever the
    /// view learned from blocks at or below it: a claim a merge dropped from
    /// state is still in the body of the block that opened it.
    pub fn reconcile(&self, floor: &FloorState, floor_number: i64) {
        let in_play: HashSet<&Validator> = floor
            .bonds
            .iter()
            .chain(floor.withdrawers.iter())
            .filter(|bond| bond.stake > 0)
            .map(|bond| &bond.validator)
            .collect();
        let knowledge = Knowledge::of(floor, floor_number);
        let window = self.params.retention_window_blocks;

        let mut state = self.state.lock();
        state.slashed.extend(
            floor
                .bonds
                .iter()
                .chain(floor.withdrawers.iter())
                .filter(|bond| bond.stake == 0)
                .map(|bond| bond.validator.clone()),
        );
        // A validator that bonds again starts a new entry, which must not be
        // debited what it was paid before it left.
        let departed: Vec<Validator> = state
            .live
            .keys()
            .chain(state.base.keys())
            .filter(|validator| !in_play.contains(validator))
            .cloned()
            .collect();
        for validator in departed {
            let paid = knowledge.claimed_total(&validator);
            state.baseline.paid_before.insert(validator, paid);
        }
        state
            .live
            .retain(|validator, _| in_play.contains(validator));
        state
            .base
            .retain(|validator, _| in_play.contains(validator));
        state
            .remembered
            .retain(|validator, _| in_play.contains(validator));

        let rebuilt: Vec<(Validator, Option<BucketEntry>)> = state
            .live
            .keys()
            .filter_map(|validator| {
                let freezes = effective(&knowledge, &state.remembered, validator, window);
                (freezes != effective(&state.known, &state.remembered, validator, window)).then(
                    || {
                        (
                            validator.clone(),
                            state.replay(&self.params, validator, &freezes),
                        )
                    },
                )
            })
            .collect();
        for (validator, entry) in rebuilt {
            match entry {
                Some(entry) => state.live.insert(validator, entry),
                None => state.live.remove(&validator),
            };
        }
        for (validator, intervals) in &knowledge.freezes {
            let kept = state.remembered.entry(validator.clone()).or_default();
            for interval in intervals
                .iter()
                .filter(|interval| interval.rejoin.is_some())
            {
                match kept.iter_mut().find(|known| known.from == interval.from) {
                    Some(known) => *known = *interval,
                    None => kept.push(*interval),
                }
            }
        }
        state.known = knowledge;

        if let Some(window) = window {
            let keep_from = floor_number.saturating_sub(window);
            let uncredited_through = state.uncredited_through();
            while let Some(block) = state.log.pop_front_if(|block| block.number < keep_from) {
                let ViewState {
                    base,
                    known,
                    remembered,
                    slashed,
                    ..
                } = &mut *state;
                for validator in block.committee.keys().filter(|v| !slashed.contains(*v)) {
                    let mut entry = base.get(validator).copied();
                    self.params.step(
                        &mut entry,
                        validator,
                        &block,
                        &effective(known, remembered, validator, Some(window)),
                        uncredited_through,
                    );
                    if let Some(entry) = entry {
                        base.insert(validator.clone(), entry);
                    }
                }
            }
        }
        state.accusations.retain(|accused, nays| {
            nays.retain(|(number, _)| {
                window.is_none_or(|window| *number >= floor_number.saturating_sub(window))
            });
            in_play.contains(accused) && !nays.is_empty()
        });
        state
            .claims
            .retain(|validator, opened| in_play.contains(validator) && *opened > floor_number);
        for record in floor
            .claims
            .iter()
            .filter(|record| record.terms == ClaimTerms::Eject)
        {
            let target = &record.claim.target;
            if in_play.contains(target) {
                let opened = state
                    .claims
                    .entry(target.clone())
                    .or_insert(record.claim.opened_at);
                *opened = (*opened).max(record.claim.opened_at);
            }
        }
    }

    /// What this view decides under the latest floor it reconciled.
    pub fn proposal(&self, own: &Validator) -> LeakyBucketProposal {
        let known = self.state.lock().known.clone();
        self.proposal_at(own, &known)
    }

    /// What this view decides under what `knowledge`'s floor records: every
    /// bucket as that floor's freezes leave it, and overflow less what that
    /// floor shows paid since this view began.
    ///
    /// Ejection offers go lowest identity first so map order cannot decide
    /// them. Only a current committee member with no open claim against it
    /// qualifies, and never `own`: a validator knows its own production
    /// exactly, so its view of itself is no evidence. The observation window
    /// counts this node's own observations rather than a span of block
    /// numbers, because admission order is not monotone in block number across
    /// sibling branches.
    pub fn proposal_at(&self, own: &Validator, knowledge: &Knowledge) -> LeakyBucketProposal {
        let state = self.state.lock();
        let uncredited_through = state.uncredited_through();
        let retention = self.params.retention_window_blocks;
        let entries: Vec<(&Validator, BucketEntry)> = state
            .live
            .iter()
            .map(|(validator, live)| {
                let freezes = effective(knowledge, &state.remembered, validator, retention);
                let mut entry = if freezes
                    == effective(&state.known, &state.remembered, validator, retention)
                {
                    *live
                } else {
                    state.replay(&self.params, validator, &freezes).expect(
                        "a tracked validator is rebuilt from the base and log that built it",
                    )
                };
                entry.refill(&freezes, i64::MAX, uncredited_through);
                (validator, entry)
            })
            .collect();

        let theta = self.params.theta_eject_ppm;
        let window = self.params.eject_observation_blocks;
        let deadline = self.params.claim_deadline_blocks;
        let mut ejection_offers: Vec<Validator> = entries
            .iter()
            .filter(|(validator, entry)| {
                *validator != own
                    && !knowledge.is_ejected(validator)
                    && entry.blocks_below_threshold >= window
                    && state.tip.committee.contains(*validator)
                    && !state.ejection_claim_open(validator, deadline)
            })
            .map(|(validator, _)| (*validator).clone())
            .collect();
        ejection_offers.sort();

        LeakyBucketProposal {
            params: self.params,
            ejection_offers,
            drained: entries
                .iter()
                .filter(|(_, entry)| entry.is_drained(theta))
                .map(|(validator, _)| (*validator).clone())
                .collect(),
            shortfall: entries
                .iter()
                .map(|(validator, entry)| {
                    (
                        (*validator).clone(),
                        BucketEntry::scaled(entry.capacity).saturating_sub(entry.level),
                    )
                })
                .collect(),
            overflow: entries
                .iter()
                .map(|(validator, entry)| {
                    let paid =
                        state.paid_since_baseline(validator, knowledge) as u128 * PPM as u128;
                    ((*validator).clone(), entry.overflow.saturating_sub(paid))
                })
                .collect(),
            accusations: state
                .accusations
                .values()
                .flat_map(|nays| nays.iter().map(|(_, accusation)| accusation.clone()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::{Bond, Rejoin};
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::rholang::runtime::Claimed;

    fn validator(byte: u8) -> Validator { Bytes::from(vec![byte; models::rust::validator::LENGTH]) }

    /// The shipped parameters with a drain of 1% a block and a fifty-block
    /// window, so a member crosses the threshold in eleven blocks and becomes
    /// ejectable in sixty-one. The mechanics are the same at any drain; the
    /// shipped one takes some seven hundred blocks to show them.
    fn fast() -> LeakyBucketParams {
        LeakyBucketParams {
            lambda_ppm: 10_000,
            eject_observation_blocks: 50,
            ..LeakyBucketParams::shipped()
        }
    }

    /// One validator's bucket as the view holds it under its latest floor,
    /// before payments are taken off. The level and overflow are in units of
    /// `1 / PPM` of a bond unit.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct BucketStatus {
        level: u128,
        overflow: u128,
        capacity: u64,
        blocks_below_threshold: u64,
    }

    impl BucketView {
        fn status_of(&self, validator: &Validator) -> Option<BucketStatus> {
            self.snapshot()
                .buckets
                .into_iter()
                .find(|bucket| bucket.validator == *validator)
                .map(|bucket| BucketStatus {
                    level: bucket.level,
                    overflow: bucket.overflow,
                    capacity: bucket.capacity,
                    blocks_below_threshold: bucket.blocks_below_threshold,
                })
        }

        fn tracked(&self) -> usize { self.state.lock().live.len() }

        fn logged(&self) -> usize { self.state.lock().log.len() }
    }

    /// Whether the view votes to eject `target`.
    fn drained(view: &BucketView, target: &Validator) -> bool {
        view.proposal(&validator(OBSERVER)).drained.contains(target)
    }

    /// Whether the view offers `target` for ejection.
    fn ejectable(view: &BucketView, target: &Validator) -> bool { targets(view).contains(target) }

    /// The overflow the view would vote with, payments taken off.
    fn owed(view: &BucketView, target: &Validator) -> u128 {
        view.proposal(&validator(OBSERVER))
            .overflow
            .get(target)
            .copied()
            .expect("tracked")
    }

    fn floor(
        bonds: Vec<Bond>,
        withdrawers: Vec<Bond>,
        claims: Vec<crate::rust::economics::claims::ClaimRecord>,
    ) -> FloorState {
        FloorState {
            bonds,
            active_validators: Vec::new(),
            withdrawers,
            claims,
            ejected: Vec::new(),
            freezes: Vec::new(),
            claimed: Vec::new(),
        }
    }

    /// `committee`'s floor, holding `target` frozen by `intervals`.
    fn frozen_floor(
        committee: &[(u8, i64)],
        target: u8,
        intervals: &[FreezeInterval],
    ) -> FloorState {
        FloorState {
            ejected: if intervals.iter().any(|interval| interval.rejoin.is_none()) {
                vec![validator(target)]
            } else {
                Vec::new()
            },
            freezes: vec![(validator(target), intervals.to_vec())],
            ..floor(bonds(committee), vec![], vec![])
        }
    }

    fn open_from(from: i64) -> FreezeInterval { FreezeInterval { from, rejoin: None } }

    fn closed(from: i64, at: i64, topup: i64) -> FreezeInterval {
        FreezeInterval {
            from,
            rejoin: Some(Rejoin { at, topup }),
        }
    }

    fn entry(capacity: u64) -> BucketEntry {
        BucketEntry::full(NonZeroU64::new(capacity).expect("a bucket needs a positive bond"))
    }

    fn bonds(stakes: &[(u8, i64)]) -> Vec<Bond> {
        stakes
            .iter()
            .map(|(byte, stake)| Bond {
                validator: validator(*byte),
                stake: *stake,
            })
            .collect()
    }

    /// One block's contribution: sent by `sender` at `number`, citing a parent
    /// from each of `parent_senders` one height back and acknowledging new
    /// work from each of them, under `committee`.
    fn observation(
        number: i64,
        sender: u8,
        parent_senders: &[u8],
        committee: &[(u8, i64)],
    ) -> Observation {
        Observation {
            block_hash: Bytes::from([number.to_le_bytes().as_slice(), &[sender]].concat()),
            sender: validator(sender),
            block_number: number,
            committee: committee_of(&bonds(committee)),
            parents: parent_senders
                .iter()
                .map(|byte| ParentFact {
                    sender: validator(*byte),
                    block_number: number - 1,
                })
                .collect(),
            fresh: parent_senders
                .iter()
                .filter(|byte| **byte != sender)
                .map(|byte| validator(*byte))
                .collect(),
            ejection_claims: Vec::new(),
            overflow_nays: Vec::new(),
        }
    }

    /// The same block, additionally showing a claim against `target` opened
    /// at block `number`.
    fn observation_opening(
        number: i64,
        sender: u8,
        parent_senders: &[u8],
        committee: &[(u8, i64)],
        target: u8,
    ) -> Observation {
        Observation {
            ejection_claims: vec![ClaimId {
                family: ClaimFamily::Eject,
                target: validator(target),
                opened_at: number,
                proposer: validator(sender),
            }],
            ..observation(number, sender, parent_senders, committee)
        }
    }

    /// Isolates the bucket arithmetic from the reward: nothing refills, so
    /// every level is the starting level less the drains applied to it.
    fn rewardless() -> BucketView {
        BucketView::new(LeakyBucketParams {
            reward: paying(0),
            ..fast()
        })
    }

    /// A slashed validator keeps a zero-stake key in `allBonds` as a tombstone
    /// against re-bonding, and the epoch re-pick puts that key back into
    /// `activeValidators` — so it is still named by both reads and only its
    /// stake tells it apart from a live member.
    #[test]
    fn reconcile_drops_a_slashed_validator_still_named_by_bonds() {
        let view = BucketView::new(fast());
        view.observe(&observation(1, 1, &[1], &[(1, 100), (2, 100)]));
        assert_eq!(view.tracked(), 2);

        view.reconcile(&floor(bonds(&[(1, 100), (2, 0)]), vec![], vec![]), 0);

        assert!(view.status_of(&validator(1)).is_some(), "still bonded");
        assert!(
            view.status_of(&validator(2)).is_none(),
            "a zero-stake bond has left the system"
        );
    }

    /// An unbonding validator is retained through quarantine: its bond has
    /// left `allBonds` for the withdrawers map, and it may still have overflow
    /// to claim before the payout completes.
    #[test]
    fn reconcile_retains_a_withdrawer_and_drops_it_after_payout() {
        let view = BucketView::new(fast());
        view.observe(&observation(1, 1, &[1], &[(1, 100), (2, 100)]));

        view.reconcile(&floor(bonds(&[(1, 100)]), bonds(&[(2, 100)]), vec![]), 0);
        assert!(
            view.status_of(&validator(2)).is_some(),
            "a quarantined withdrawer keeps its entry"
        );

        view.reconcile(&floor(bonds(&[(1, 100)]), vec![], vec![]), 0);
        assert!(
            view.status_of(&validator(2)).is_none(),
            "the payout completed, so the bond has left the system"
        );
    }

    /// A bonded validator that has stopped appearing in committees is frozen,
    /// not dropped: it may still be owed overflow.
    #[test]
    fn reconcile_retains_a_bonded_validator_absent_from_the_committee() {
        let view = BucketView::new(fast());
        view.observe(&observation(1, 1, &[1], &[(1, 100), (2, 100)]));
        let before = view.status_of(&validator(2)).expect("tracked");

        view.observe(&observation(2, 1, &[1], &[(1, 100)]));
        view.reconcile(&floor(bonds(&[(1, 100), (2, 100)]), vec![], vec![]), 0);

        let after = view.status_of(&validator(2)).expect("still tracked");
        assert_eq!(
            before.level, after.level,
            "an absent member's level is frozen, not drained"
        );
    }

    #[test]
    fn decay_removes_lambda_of_capacity_per_block_and_floors_at_zero() {
        let mut bucket = entry(1_000);
        bucket.decay(10_000);
        assert_eq!(bucket.level, 990_000_000, "1% of capacity per block");
        for _ in 0..200 {
            bucket.decay(10_000);
        }
        assert_eq!(bucket.level, 0, "the level floors rather than underflowing");
    }

    /// `(1 − θ)/λ` decays land the level EXACTLY on the threshold, and the
    /// ejection test is a strict `<`, so the crossing is one block later.
    /// The count is independent of capacity, which is the arithmetic the
    /// ejection window rests on — `θ·C` and `λ·C` both scale with `C`. It
    /// holds down to a bond of 1 only because the level is denominated below
    /// the bond unit.
    #[test]
    fn the_threshold_crossing_is_independent_of_capacity() {
        for capacity in [1u64, 3, 5, 7, 100, 1_000, 1_000_000] {
            let mut bucket = entry(capacity);
            let threshold = bucket.threshold(900_000);
            let mut blocks = 0;
            while bucket.level >= threshold {
                bucket.decay(10_000);
                blocks += 1;
            }
            assert_eq!(
                blocks, 11,
                "capacity {capacity} should cross on the same block regardless of size"
            );
        }
    }

    #[test]
    fn fill_tops_up_the_level_then_spills_to_overflow() {
        let mut bucket = entry(1_000);
        bucket.decay(10_000);
        assert_eq!(bucket.level, 990_000_000);

        bucket.fill(5_000_000);
        assert_eq!((bucket.level, bucket.overflow), (995_000_000, 0));

        bucket.fill(100_000_000);
        assert_eq!(
            (bucket.level, bucket.overflow),
            (1_000_000_000, 95_000_000),
            "the level caps at capacity and the remainder becomes overflow"
        );
    }

    #[test]
    fn overflow_never_refills_the_level() {
        let mut bucket = entry(1_000);
        bucket.fill(500_000_000);
        assert_eq!(
            (bucket.level, bucket.overflow),
            (1_000_000_000, 500_000_000)
        );
        for _ in 0..50 {
            bucket.decay(10_000);
        }
        assert_eq!(
            (bucket.level, bucket.overflow),
            (500_000_000, 500_000_000),
            "a drained level does not draw on overflow, even below the threshold"
        );
    }

    /// Changing the bond holds the level's share of capacity fixed, so it
    /// moves the validator neither towards nor away from its own threshold.
    /// Overflow is untouched: a bond change is not earnings.
    #[test]
    fn reseating_a_bond_preserves_the_level_as_a_share_of_capacity() {
        let drained = |capacity| {
            let mut bucket = entry(capacity);
            bucket.fill(50_000_000);
            for _ in 0..11 {
                bucket.decay(10_000);
            }
            bucket
        };

        let mut shrunk = drained(1_000);
        assert!(shrunk.level < shrunk.threshold(900_000));
        shrunk.reseat(NonZeroU64::new(400).unwrap());
        assert_eq!(
            (shrunk.level, shrunk.capacity.get(), shrunk.overflow),
            (356_000_000, 400, 50_000_000)
        );
        assert!(
            shrunk.level < shrunk.threshold(900_000),
            "unbonding must not rescue a draining validator"
        );

        let mut grown = drained(1_000);
        grown.reseat(NonZeroU64::new(3_000).unwrap());
        assert_eq!(grown.level, 2_670_000_000);
        assert!(
            grown.level < grown.threshold(900_000),
            "the share of capacity is unchanged, so the position is unchanged"
        );
    }

    /// `bond-maximum` ships as `i64::MAX`, so the reseat is reachable with two
    /// bonds whose product overflows `u128`.
    #[test]
    fn reseating_between_the_largest_permitted_bonds_holds_the_share() {
        let largest = NonZeroU64::new(i64::MAX as u64).expect("positive");
        let mut bucket = BucketEntry::full(largest);
        bucket.decay(10_000);

        bucket.reseat(NonZeroU64::new(largest.get() - 1).expect("positive"));

        let capacity = bucket.capacity.get() as u128;
        assert!(
            bucket.level >= capacity * 989_999 && bucket.level < capacity * 990_001,
            "the level must still be 99% of the new capacity, got {}",
            bucket.level
        );
    }

    #[test]
    fn the_observation_window_counts_consecutive_blocks_below_the_threshold() {
        let mut bucket = entry(1_000);
        for _ in 0..11 {
            bucket.decay(10_000);
        }
        bucket.observe_threshold(900_000);
        assert_eq!(bucket.blocks_below_threshold, 1);

        bucket.observe_threshold(900_000);
        assert_eq!(
            bucket.blocks_below_threshold, 2,
            "each observation below the threshold adds one"
        );

        bucket.fill(1_000_000_000);
        bucket.observe_threshold(900_000);
        assert_eq!(
            bucket.blocks_below_threshold, 0,
            "producing a block clears the window"
        );
    }

    /// The window is a count of this node's observations, so it cannot be
    /// satisfied by the spread between block numbers. Admission follows a
    /// topological order, under which a low-numbered sibling branch can
    /// arrive after a much higher-numbered one.
    #[test]
    fn the_ejection_window_counts_observations_not_block_numbers() {
        let view = rewardless();
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        for number in 1_000..=1_010i64 {
            view.observe(&observation(number, 1, &[1], &committee));
        }
        let poised = view.status_of(&validator(2)).expect("tracked");
        assert_eq!(
            (poised.level, poised.blocks_below_threshold),
            (900_000_000, 0)
        );

        // A sibling 1005 blocks lower opens the window, and one more
        // observation follows it.
        view.observe(&observation(5, 1, &[1], &committee));
        view.observe(&observation(1_011, 1, &[1], &committee));
        assert_eq!(
            view.status_of(&validator(2))
                .expect("tracked")
                .blocks_below_threshold,
            2
        );
        assert!(
            !ejectable(&view, &validator(2)),
            "two observations cannot satisfy a fifty-observation window"
        );

        for number in 1_012..=1_059i64 {
            view.observe(&observation(number, 1, &[1], &committee));
        }
        assert!(ejectable(&view, &validator(2)), "fifty observations do");
    }

    /// The smallest bonds the test genesis assigns are `i * 2 + 1`, every one
    /// of them below `1 / lambda`. The drain has to bite at that size.
    #[test]
    fn the_drain_bites_at_the_smallest_bonds_the_genesis_assigns() {
        for bond in [1u64, 3, 5, 7] {
            let mut bucket = entry(bond);
            bucket.decay(10_000);
            assert_eq!(
                bucket.level,
                bond as u128 * 990_000,
                "bond {bond} must lose 1% of itself"
            );
        }

        // Validators 1 and 2 alternate, each acknowledging the other. The
        // poorer share is 2 acknowledging 1: breadth 1/16, so 1 − (15/16)² =
        // 0.12, at recency 1/2. Over its two blocks it drains 2% of its stake
        // and earns 0.06 · rate · 4 of it, so 0.2 holds it with margin.
        let view = with_rate(200_000);
        let committee = [(1u8, 1i64), (2, 3), (3, 5), (4, 7)];
        for number in 0..=70i64 {
            let (sender, other) = if number % 2 == 0 { (1, 2) } else { (2, 1) };
            view.observe(&observation(number, sender, &[other], &committee));
        }
        for byte in [3u8, 4] {
            assert!(
                drained(&view, &validator(byte)),
                "validator {byte} produced nothing across 71 blocks"
            );
            assert!(ejectable(&view, &validator(byte)));
        }
        for byte in [1u8, 2] {
            assert!(
                !drained(&view, &validator(byte)),
                "producer {byte} stays above the threshold"
            );
        }
    }

    /// The whole path: a three-member committee where one member never
    /// produces. Members are seated full on first sight, drain on every block,
    /// the producer refills, and only the silent member reaches the ejection
    /// window.
    #[test]
    fn a_silent_member_becomes_ejectable_while_producers_do_not() {
        let committee = [(1u8, 1_000i64), (2, 1_000), (3, 1_000)];
        // Each producer acknowledges the other: breadth 1/3, so 1 − (2/3)² =
        // 0.56, at recency 1/2. Over its two blocks it drains 2% of its stake
        // and earns 0.28 · rate · 3 of it, so 0.1 holds it with margin.
        let view = with_rate(100_000);

        view.observe(&observation(0, 1, &[], &committee));
        // Validators 1 and 2 alternate; 3 never proposes.
        for number in 1..=70i64 {
            let (sender, other) = if number % 2 == 0 { (1, 2) } else { (2, 1) };
            view.observe(&observation(number, sender, &[other], &committee));
        }

        assert_eq!(view.tracked(), 3, "every committee member is seated");

        let silent = view
            .status_of(&validator(3))
            .expect("silent member tracked");
        assert_eq!(silent.capacity, 1_000);
        assert!(
            drained(&view, &validator(3)),
            "a member that never produced has drained below the threshold, level {}",
            silent.level
        );
        assert_eq!(
            silent.overflow, 0,
            "a member that never produced has earned nothing"
        );
        assert!(
            ejectable(&view, &validator(3)),
            "the silent member has been below the threshold for the whole window"
        );

        for producer in [validator(1), validator(2)] {
            assert!(
                !drained(&view, &producer),
                "a producing member stays above the threshold"
            );
            assert!(
                !ejectable(&view, &producer),
                "a producing member must never reach the ejection window"
            );
        }
    }

    /// A snapshot shows each member's bucket against its threshold, the
    /// committee at the tip, and the claims still open.
    #[test]
    fn a_snapshot_shows_a_silent_member_draining_and_the_producers_holding() {
        let committee = [(1u8, 1_000i64), (2, 1_000), (3, 1_000)];
        let view = with_rate(100_000);
        view.observe(&observation(0, 1, &[], &committee));
        for number in 1..=70i64 {
            let (sender, other) = if number % 2 == 0 { (1, 2) } else { (2, 1) };
            view.observe(&observation(number, sender, &[other], &committee));
        }
        view.observe(&observation_opening(71, 1, &[2], &committee, 3));

        let snapshot = view.snapshot();
        assert_eq!(snapshot.tip_height, 71);
        assert_eq!(snapshot.tip_committee, vec![
            validator(1),
            validator(2),
            validator(3)
        ]);
        assert_eq!(snapshot.open_ejection_claims, vec![(validator(3), 71)]);
        let buckets: Vec<&Validator> = snapshot.buckets.iter().map(|b| &b.validator).collect();
        assert_eq!(buckets, vec![&validator(1), &validator(2), &validator(3)]);
        for bucket in &snapshot.buckets {
            assert_eq!(bucket.capacity, 1_000);
            assert_eq!(
                bucket.threshold,
                1_000 * view.params().theta_eject_ppm as u128
            );
            assert_eq!(bucket.drained, bucket.level < bucket.threshold);
            assert!(!bucket.ejected);
        }
        let silent = &snapshot.buckets[2];
        assert!(silent.drained);
        assert!(silent.blocks_below_threshold >= view.params().eject_observation_blocks);
        assert_eq!(silent.overflow, 0);
        assert!(snapshot.buckets[..2]
            .iter()
            .all(|producer| !producer.drained));
    }

    /// The liveness test profile: a silent member of a four-member committee
    /// drains below the threshold within about twenty blocks and becomes
    /// ejectable ten blocks later, while members producing in turn never
    /// drain.
    #[test]
    fn the_test_profile_ejects_a_silent_member_within_thirty_blocks_and_spares_the_producers() {
        let view = BucketView::new(LeakyBucketParams {
            lambda_ppm: 5_000,
            eject_observation_blocks: 10,
            reward: RewardParams {
                reward_rate_ppm: 50_000,
                ..LeakyBucketParams::shipped().reward
            },
            ..LeakyBucketParams::shipped()
        });
        let committee = [(1u8, 1_000i64), (2, 1_000), (3, 1_000), (4, 1_000)];
        let producers = [1u8, 2, 3];
        view.observe(&observation(0, 1, &[], &committee));
        let (mut drained_at, mut ejectable_at) = (None, None);
        for number in 1..=60i64 {
            let sender = producers[number as usize % producers.len()];
            let others: Vec<u8> = producers
                .iter()
                .copied()
                .filter(|producer| *producer != sender)
                .collect();
            view.observe(&observation(number, sender, &others, &committee));
            if drained_at.is_none() && drained(&view, &validator(4)) {
                drained_at = Some(number);
            }
            if ejectable_at.is_none() && ejectable(&view, &validator(4)) {
                ejectable_at = Some(number);
            }
            for producer in producers {
                assert!(
                    !drained(&view, &validator(producer)),
                    "producer {producer} drained at block {number}"
                );
            }
        }
        let (drained_at, ejectable_at) = (
            drained_at.expect("the silent member drains"),
            ejectable_at.expect("the silent member becomes ejectable"),
        );
        assert!(
            (19..=21).contains(&drained_at),
            "drained at block {drained_at}"
        );
        assert!(
            ejectable_at <= drained_at + 10,
            "ejectable at block {ejectable_at}, drained at {drained_at}"
        );
    }

    /// An observer outside every committee in these tests.
    const OBSERVER: u8 = 0xEE;

    fn targets(view: &BucketView) -> Vec<Validator> {
        view.proposal(&validator(OBSERVER)).ejection_offers
    }

    /// Ordered by identity rather than by map order, so the same observations
    /// always produce the same offers.
    #[test]
    fn candidates_are_ordered_by_identity() {
        let committee = [(9u8, 1_000i64), (3, 1_000), (7, 1_000), (1, 1_000)];
        let view = rewardless();
        for number in 0..=70i64 {
            view.observe(&observation(number, 9, &[9], &committee));
        }

        assert_eq!(targets(&view), vec![
            validator(1),
            validator(3),
            validator(7),
            validator(9)
        ]);
    }

    #[test]
    fn a_node_never_offers_itself() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let view = rewardless();
        for number in 0..=70i64 {
            view.observe(&observation(number, 1, &[1], &committee));
        }
        assert!(ejectable(&view, &validator(1)));
        assert_eq!(view.proposal(&validator(1)).ejection_offers, vec![
            validator(2)
        ]);
    }

    /// An open claim is learned from the block that carried it, so this node
    /// stops proposing a second one the contract would refuse — and resumes
    /// once a floor reaches the claim's deadline, which the block that expires
    /// it follows.
    #[test]
    fn an_open_claim_read_from_a_block_withholds_the_candidate_until_its_deadline() {
        let committee = TRIO;
        // The producers earn, so only validator 2 drains and the candidate is
        // unambiguous.
        let view = draining_second_member();
        assert_eq!(targets(&view), vec![validator(2)]);

        // Block 71 opens proceedings against validator 2.
        view.observe(&observation_opening(71, 1, &[3], &committee, 2));
        assert!(
            targets(&view).is_empty(),
            "the claim is open, so there is nothing to propose"
        );

        // A block past the deadline does not end the claim while its floor
        // is inside it (71 + 40 > 110 is false at 111).
        view.observe(&observation(130, 1, &[3], &committee));
        assert!(targets(&view).is_empty());
        let record = crate::rust::economics::claims::ClaimRecord {
            claim: ClaimId {
                family: ClaimFamily::Eject,
                target: validator(2),
                opened_at: 71,
                proposer: validator(1),
            },
            electorate: bonds(&committee),
            terms: ClaimTerms::Eject,
        };
        view.reconcile(&floor(bonds(&committee), vec![], vec![record.clone()]), 110);
        assert!(targets(&view).is_empty(), "a floor inside the deadline");

        view.reconcile(&floor(bonds(&committee), vec![], vec![record]), 111);
        assert_eq!(
            targets(&view),
            vec![validator(2)],
            "once a floor reaches the deadline the claim has lapsed undecided"
        );
    }

    #[test]
    fn there_is_no_candidate_while_every_member_is_earning() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        // Each acknowledges the other: breadth 1/2 gives 0.75, at recency
        // 1/2, so 0.375 · rate · 2 of the stake a block against 2% drained
        // between blocks; 0.1 holds with margin.
        let view = with_rate(100_000);
        for number in 0..=70i64 {
            let (sender, other) = if number % 2 == 0 { (1, 2) } else { (2, 1) };
            view.observe(&observation(number, sender, &[other], &committee));
        }
        assert!(targets(&view).is_empty());
    }

    /// A validator that leaves the committee keeps its entry and its
    /// overflow — it can still be owed earnings — and stops draining.
    #[test]
    fn leaving_the_committee_freezes_the_bucket_and_keeps_overflow() {
        let full = [(1u8, 1_000i64), (2, 1_000)];
        let reduced = [(1u8, 1_000i64)];

        // Validator 2 acknowledges 1's block: 0.75 at recency 1/2, so 0.375 ·
        // rate · 2 of its stake, far past its capacity at a rate of 2.
        let view = with_rate(2_000_000);

        view.observe(&observation(0, 1, &[], &full));
        view.observe(&observation(1, 2, &[1], &full));
        let before = view.status_of(&validator(2)).expect("departing tracked");
        assert!(before.overflow > 0, "the departing member earned overflow");

        view.observe(&observation(2, 1, &[2], &reduced));
        let after_status = view.status_of(&validator(2)).expect("entry retained");
        assert_eq!(
            after_status, before,
            "an absent member's bucket is frozen whole: same level, same overflow"
        );
    }

    /// Validators 1 and 3 produce, validator 2 is silent.
    const TRIO: [(u8, i64); 3] = [(1, 1_000), (2, 1_000), (3, 1_000)];

    /// Block `number` of the trio: 1 and 3 alternate, each acknowledging the
    /// other's last block.
    fn producing(number: i64, committee: &[(u8, i64)]) -> Observation {
        let (sender, other) = if number % 2 == 0 { (1, 3) } else { (3, 1) };
        observation(number, sender, &[other], committee)
    }

    /// Breadth 1/3 gives 1 − (2/3)² = 0.56, at recency 1/2: a producer
    /// earns 0.28 · rate · 3 of its stake a block against 2% drained between
    /// its blocks, so 0.1 holds it with margin.
    const TRIO_RATE: u64 = 100_000;

    /// Validator 2 drains while 1 and 3 produce; a claim lasts 40 blocks.
    fn draining_second_member() -> BucketView {
        let view = BucketView::new(LeakyBucketParams {
            claim_deadline_blocks: 40,
            reward: paying(TRIO_RATE),
            ..fast()
        });
        for number in 0..=70i64 {
            view.observe(&producing(number, &TRIO));
        }
        view
    }

    /// The contract refuses a target outside the opening block's committee,
    /// and a frozen bucket stays past the window for as long as it is kept.
    #[test]
    fn a_member_that_left_the_committee_is_not_a_candidate() {
        let view = draining_second_member();
        assert_eq!(targets(&view), vec![validator(2)]);

        view.observe(&observation(71, 1, &[3], &[(1, 1_000), (3, 1_000)]));
        assert!(
            view.status_of(&validator(2))
                .expect("tracked")
                .blocks_below_threshold
                >= fast().eject_observation_blocks,
            "its bucket is frozen, not reset"
        );
        assert!(targets(&view).is_empty());
    }

    /// Admission follows a topological order, so a claim opened on a lower
    /// branch can be admitted after a later one against the same target.
    #[test]
    fn a_late_admitted_older_claim_does_not_shorten_a_newer_one() {
        let committee = TRIO;
        let view = draining_second_member();
        view.observe(&observation_opening(100, 1, &[3], &committee, 2));
        view.observe(&observation_opening(60, 1, &[3], &committee, 2));
        assert!(
            targets(&view).is_empty(),
            "the claim opened at 100 is open until 140"
        );
    }

    /// A merge can drop a claim from state while the block that opened it
    /// stays accepted, so every node that saw the block would otherwise hold
    /// the target back for the whole deadline.
    #[test]
    fn the_floor_drops_a_claim_its_state_does_not_hold() {
        let committee = TRIO;
        let view = draining_second_member();
        view.observe(&observation_opening(71, 1, &[3], &committee, 2));
        assert!(targets(&view).is_empty());

        view.reconcile(&floor(bonds(&committee), vec![], vec![]), 71);
        assert_eq!(targets(&view), vec![validator(2)]);
    }

    /// A restarted node rebuilds its view from new blocks only, so a claim
    /// opened before it started is known only from the floor's state.
    #[test]
    fn the_floor_restores_a_claim_this_node_never_saw() {
        let committee = TRIO;
        let view = draining_second_member();
        let claim = crate::rust::economics::claims::ClaimRecord {
            claim: models::rust::casper::protocol::casper_message::ClaimId {
                family: models::rust::casper::protocol::casper_message::ClaimFamily::Eject,
                target: validator(2),
                opened_at: 65,
                proposer: validator(1),
            },
            electorate: bonds(&committee),
            terms: crate::rust::economics::claims::ClaimTerms::Eject,
        };
        view.reconcile(&floor(bonds(&committee), vec![], vec![claim]), 68);
        assert!(targets(&view).is_empty(), "opened at 65, open until 105");
    }

    #[test]
    fn a_claim_above_the_floor_survives_reconciliation() {
        let committee = TRIO;
        let view = draining_second_member();
        view.observe(&observation_opening(71, 1, &[3], &committee, 2));
        view.reconcile(&floor(bonds(&committee), vec![], vec![]), 68);
        assert!(
            targets(&view).is_empty(),
            "the floor's state cannot yet show a claim opened above it"
        );
    }

    /// An ejected validator's bucket is frozen: it neither drains nor counts
    /// towards a new ejection.
    #[test]
    fn an_ejected_validators_bucket_is_frozen() {
        let committee = TRIO;
        let view = draining_second_member();
        let before = view.status_of(&validator(2)).expect("tracked");
        view.reconcile(&frozen_floor(&committee, 2, &[open_from(71)]), 71);
        for number in 72..=200i64 {
            view.observe(&producing(number, &committee));
        }
        assert_eq!(view.status_of(&validator(2)), Some(before));
        assert!(targets(&view).is_empty());
    }

    fn rejoin_claim(target: u8, topup: i64) -> crate::rust::economics::claims::ClaimRecord {
        crate::rust::economics::claims::ClaimRecord {
            claim: models::rust::casper::protocol::casper_message::ClaimId {
                family: models::rust::casper::protocol::casper_message::ClaimFamily::Rejoin,
                target: validator(target),
                opened_at: 80,
                proposer: validator(target),
            },
            electorate: Vec::new(),
            terms: crate::rust::economics::claims::ClaimTerms::Rejoin {
                topup,
                grace_end: 1_000,
            },
        }
    }

    /// A voter backs a rejoin iff its level for the validator plus the topup
    /// reaches capacity.
    #[test]
    fn a_rejoin_is_backed_iff_the_topup_covers_the_shortfall() {
        let view = draining_second_member();
        let status = view.status_of(&validator(2)).expect("tracked");
        let shortfall = status.capacity as u128 * PPM as u128 - status.level;
        let exact = shortfall.div_ceil(PPM as u128) as i64;
        let proposal = view.proposal(&validator(OBSERVER));
        assert!(proposal.votes_yea(&rejoin_claim(2, exact)));
        assert!(!proposal.votes_yea(&rejoin_claim(2, exact - 1)));
    }

    /// A paid rejoin fills the bucket and credits what the refill did not
    /// need to overflow, once.
    #[test]
    fn a_paid_rejoin_refills_once_and_credits_the_surplus() {
        let committee = TRIO;
        let view = draining_second_member();
        view.reconcile(&floor(bonds(&committee), vec![], vec![]), 70);
        let before = view.status_of(&validator(2)).expect("tracked");
        let refill = before.capacity as u128 * PPM as u128 - before.level;

        // More than the whole capacity, so some of it must be surplus.
        let rejoined = frozen_floor(&committee, 2, &[closed(75, 90, 2_000)]);
        view.reconcile(&rejoined, 90);
        let after = view.status_of(&validator(2)).expect("tracked");
        assert_eq!(after.level, after.capacity as u128 * PPM as u128);
        assert_eq!(
            after.overflow,
            before.overflow + 2_000 * PPM as u128 - refill
        );
        assert_eq!(after.blocks_below_threshold, 0);

        view.reconcile(&rejoined, 91);
        assert_eq!(
            view.status_of(&validator(2)),
            Some(after),
            "the same rejoin applies once"
        );
        view.observe(&producing(92, &committee));
        let next = view.status_of(&validator(2)).expect("tracked");
        assert_eq!(
            next.overflow, after.overflow,
            "a block after the rejoin does not apply it again"
        );
    }

    /// A rejoin paid below the block a view credits from predates its view:
    /// the level refills, and the view credits none of the topup it may
    /// already have been paid back.
    #[test]
    fn a_rejoin_paid_before_the_view_began_refills_without_crediting() {
        let committee = TRIO;
        let view = draining_second_member();
        let before = view.status_of(&validator(2)).expect("tracked");
        view.begin(&frozen_floor(&committee, 2, &[closed(71, 72, 500)]), 72, 73);
        let after = view.status_of(&validator(2)).expect("tracked");
        assert_eq!(after.level, after.capacity as u128 * PPM as u128);
        assert_eq!(after.overflow, before.overflow);
    }

    /// A rejoin applies before the first block numbered above it, wherever
    /// that block arrives in admission order.
    #[test]
    fn a_rejoin_applies_before_the_first_block_numbered_above_it() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let view = rewardless();
        view.reconcile(&floor(bonds(&committee), vec![], vec![]), 0);
        for number in 1..=20i64 {
            view.observe(&observation(number, 1, &[1], &committee));
        }
        view.reconcile(&frozen_floor(&committee, 2, &[closed(5, 10, 0)]), 20);
        let status = view.status_of(&validator(2)).expect("tracked");
        let full = 1_000u128 * PPM as u128;
        let drain = 1_000u128 * fast().lambda_ppm as u128;
        assert_eq!(
            status.level,
            full - 10 * drain,
            "full after the rejoin at 10, then the ten blocks above it drain"
        );
    }

    fn overflow_claim(target: u8, amount: i64) -> crate::rust::economics::claims::ClaimRecord {
        crate::rust::economics::claims::ClaimRecord {
            claim: models::rust::casper::protocol::casper_message::ClaimId {
                family: models::rust::casper::protocol::casper_message::ClaimFamily::Overflow,
                target: validator(target),
                opened_at: 95,
                proposer: validator(target),
            },
            electorate: Vec::new(),
            terms: ClaimTerms::Overflow { amount },
        }
    }

    fn claimed_floor(committee: &[(u8, i64)], total: i64) -> FloorState {
        FloorState {
            claimed: vec![Claimed {
                validator: validator(2),
                total,
                payments: Vec::new(),
            }],
            ..floor(bonds(committee), vec![], vec![])
        }
    }

    /// A view holding overflow for the second member, credited by a rejoin
    /// that topped up past its shortfall.
    fn with_overflow(committee: &[(u8, i64)]) -> BucketView {
        let view = draining_second_member();
        view.reconcile(&floor(bonds(committee), vec![], vec![]), 70);
        view.reconcile(&frozen_floor(committee, 2, &[closed(75, 90, 2_000)]), 90);
        view
    }

    /// A voter backs a claim iff its view of the claimant's overflow covers
    /// the amount.
    #[test]
    fn an_overflow_claim_is_backed_iff_the_view_covers_it() {
        let committee = TRIO;
        let view = with_overflow(&committee);
        let whole = (owed(&view, &validator(2)) / PPM as u128) as i64;
        let proposal = view.proposal(&validator(OBSERVER));
        assert!(proposal.votes_yea(&overflow_claim(2, whole)));
        assert!(!proposal.votes_yea(&overflow_claim(2, whole + 1)));
    }

    /// Payments come off the overflow as the floor's running total grows,
    /// once each, and a view that credited less than was paid stops at zero.
    #[test]
    fn a_paid_claim_debits_overflow_once_and_stops_at_zero() {
        let committee = TRIO;
        let view = with_overflow(&committee);
        let rejoined = (2u8, [closed(75, 90, 2_000)]);
        let at = |total: i64| FloorState {
            freezes: vec![(validator(rejoined.0), rejoined.1.to_vec())],
            ..claimed_floor(&committee, total)
        };
        let before = owed(&view, &validator(2));

        view.reconcile(&at(10), 100);
        let after = owed(&view, &validator(2));
        assert_eq!(after, before - 10 * PPM as u128);
        view.reconcile(&at(10), 101);
        assert_eq!(
            owed(&view, &validator(2)),
            after,
            "the same total debits once"
        );

        view.reconcile(&at(i64::MAX / 2), 102);
        assert_eq!(owed(&view, &validator(2)), 0);
    }

    /// A validator that leaves the bond and bonds again is not debited, in its
    /// new entry, what it was paid before it left.
    #[test]
    fn a_returning_validator_is_not_debited_what_it_was_paid_before_leaving() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let view = with_rate(WHOLE_UNITS_FOR_TWO);
        view.reconcile(&floor(bonds(&committee), vec![], vec![]), 0);
        view.observe(&observation(1, 2, &[1, 2], &committee));
        let paid = (view.status_of(&validator(2)).expect("tracked").overflow / PPM as u128) as i64;
        view.reconcile(&claimed_floor(&committee, paid), 1);
        assert_eq!(
            owed(&view, &validator(2)),
            0,
            "the first stake's overflow is paid"
        );

        let left = FloorState {
            claimed: claimed_floor(&committee, paid).claimed,
            ..floor(bonds(&[(1, 1_000)]), vec![], vec![])
        };
        view.reconcile(&left, 2);
        assert!(
            view.status_of(&validator(2)).is_none(),
            "its stake has left"
        );

        view.reconcile(&claimed_floor(&committee, paid), 3);
        view.observe(&observation(4, 2, &[1, 2], &committee));
        let credited = view
            .status_of(&validator(2))
            .expect("bonded again")
            .overflow;
        assert!(credited > 0);
        assert_eq!(owed(&view, &validator(2)), credited);
    }

    /// Payments made below the block a view credits from drew on overflow it
    /// never credited, so they are not taken off.
    #[test]
    fn payments_before_the_view_began_are_not_taken_off() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let view = with_rate(WHOLE_UNITS_FOR_TWO);
        view.begin(&claimed_floor(&committee, 7), 0, 1);
        view.observe(&observation(1, 2, &[1, 2], &committee));
        let credited = view.status_of(&validator(2)).expect("tracked").overflow;
        assert!(credited > 0);
        assert_eq!(owed(&view, &validator(2)), credited);
    }

    /// A view rebuilt over blocks a claim was already paid from takes that
    /// payment off, though its starting floor recorded it.
    #[test]
    fn a_payment_inside_the_credited_span_is_taken_off_a_begun_view() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let view = with_rate(WHOLE_UNITS_FOR_TWO);
        let paid = FloorState {
            claimed: vec![Claimed {
                validator: validator(2),
                total: 7,
                payments: vec![
                    crate::rust::rholang::runtime::Payment { at: 1, amount: 3 },
                    crate::rust::rholang::runtime::Payment { at: 4, amount: 4 },
                ],
            }],
            ..floor(bonds(&committee), vec![], vec![])
        };
        view.begin(&paid, 5, 2);
        for number in 2..=6i64 {
            view.observe(&observation(number, 2, &[1, 2], &committee));
        }
        let credited = view.status_of(&validator(2)).expect("tracked").overflow;
        assert!(credited > 4 * PPM as u128);
        assert_eq!(
            owed(&view, &validator(2)),
            credited - 4 * PPM as u128,
            "the payment at 4 drew on blocks this view credits; the one at 1 did not"
        );
    }

    /// The shipped factors at a reward rate of `rate_ppm` of the proposer's
    /// stake per member per block. In a committee of four equal stakes, a
    /// block acknowledging the other three with recency saturated scores
    /// `1 − (1/4)² = 0.9375`, so it earns `0.9375 · rate · 4 · stake`.
    fn paying(rate_ppm: u64) -> RewardParams {
        RewardParams {
            reward_rate_ppm: rate_ppm,
            ..fast().reward
        }
    }

    /// In a committee of two equal stakes of 1 000, a block citing both
    /// members scores 0.75 at full recency and so earns `0.75 · rate · 2 ·
    /// 1 000`: 9 999 bond units at this rate, a whole number, so a payment in
    /// whole units can clear it.
    const WHOLE_UNITS_FOR_TWO: u64 = 6_666_000;

    /// The same for a committee of four equal stakes of 1 000, whose block
    /// citing all four earns `0.9375 · rate · 4 · 1 000`.
    fn rate_paying_four(bond_units: u64) -> u64 { (bond_units * PPM).div_ceil(3_750) }

    fn with_rate(rate_ppm: u64) -> BucketView {
        BucketView::new(LeakyBucketParams {
            reward: paying(rate_ppm),
            ..fast()
        })
    }

    fn reward_of(view: &BucketView, observation: &Observation) -> u128 {
        compute_reward(
            &observation.committee,
            &observation.sender,
            &observation.fresh,
            &observation.parents,
            observation.block_number,
            &view.params.reward,
        )
    }

    /// The published bound `Cum_W(V) − C(W) − ClaimedOverflow_W` is not a
    /// lower bound on what an honest view holds: the bucket refills as often
    /// as it drains, so over time it absorbs far more than `C(W)`. Here a
    /// member earns less each turn than it drains between turns, so its
    /// overflow stays nothing while that bound grows.
    #[test]
    fn the_cumulative_bound_exceeds_an_honest_view_once_drain_absorbs_the_reward() {
        let committee = [(1u8, 1_000i64), (2, 1_000), (3, 1_000), (4, 1_000)];
        // 20 a turn against 4 drains of 10 between turns. Every member enters
        // full, so the member proposes only once it has drained below that.
        let view = with_rate(rate_paying_four(20));
        let mut cum = 0u128;
        for number in 1..=400i64 {
            let sender = ((number + 1) % 4 + 1) as u8;
            let block = observation(number, sender, &[1, 2, 3, 4], &committee);
            if sender == 2 {
                cum += reward_of(&view, &block);
            }
            view.observe(&block);
        }
        let capacity = 1_000u128 * PPM as u128;
        let bound = cum - capacity;
        let overflow = view.status_of(&validator(2)).expect("tracked").overflow;
        assert_eq!(overflow, 0, "the honest view holds no overflow");
        assert!(bound > 0, "yet the bound claims {bound} of it");
    }

    /// A reward above `(1 − θ) · C(W)` need not lift an empty bucket past the
    /// threshold: from empty, crossing `θ · C` takes `θ · C`.
    #[test]
    fn a_reward_above_the_complement_need_not_lift_an_empty_bucket_past_the_threshold() {
        let committee = [(1u8, 1_000i64), (2, 1_000), (3, 1_000), (4, 1_000)];
        let view = with_rate(rate_paying_four(500));
        for number in 1..=150i64 {
            let sender = [1u8, 3, 4][number as usize % 3];
            view.observe(&observation(number, sender, &[1, 3, 4], &committee));
        }
        assert_eq!(
            view.status_of(&validator(2)).expect("tracked").level,
            0,
            "the silent member has drained to nothing"
        );
        let block = observation(151, 2, &[1, 3, 4], &committee);
        let reward = reward_of(&view, &block);
        view.observe(&block);

        let capacity = 1_000u128 * PPM as u128;
        let theta = view.params.theta_eject_ppm as u128;
        assert!(
            reward * PPM as u128 > (PPM as u128 - theta) * capacity,
            "the reward clears (1 − θ) · C"
        );
        assert!(
            view.proposal(&validator(OBSERVER))
                .drained
                .contains(&validator(2)),
            "yet the honest view still finds it drained, so its yea is honest"
        );
    }

    /// A frozen view credits nothing from W's blocks, so a bound summing every
    /// W-block regardless claims overflow the view never held.
    #[test]
    fn a_bound_over_every_w_block_counts_blocks_a_frozen_view_never_credits() {
        let committee = [(1u8, 1_000i64), (2, 1_000), (3, 1_000), (4, 1_000)];
        let view = with_rate(rate_paying_four(500));
        view.reconcile(&frozen_floor(&committee, 2, &[open_from(0)]), 0);
        let mut cum = 0u128;
        for number in 1..=40i64 {
            let sender = (number % 4 + 1) as u8;
            let block = observation(number, sender, &[1, 2, 3, 4], &committee);
            if sender == 2 {
                cum += reward_of(&view, &block);
            }
            view.observe(&block);
        }
        let capacity = 1_000u128 * PPM as u128;
        assert_eq!(view.status_of(&validator(2)).expect("tracked").overflow, 0);
        assert!(
            cum > capacity,
            "the bound claims {} for a view that credited nothing",
            cum - capacity
        );
    }

    /// A fresh W-block's reward cannot prove a yea on ejecting W dishonest,
    /// whatever its size: the view keeps draining with every block it accepts
    /// before voting, and nothing bounds how many it accepts. Here a reward
    /// that alone fills the bucket is followed by twenty blocks from the rest,
    /// and the honest view finds W drained.
    #[test]
    fn any_fresh_reward_drains_away_before_the_vote() {
        let committee = [(1u8, 1_000i64), (2, 1_000), (3, 1_000), (4, 1_000)];
        let view = with_rate(rate_paying_four(5_000));
        let block = observation(1, 2, &[1, 2, 3, 4], &committee);
        let reward = reward_of(&view, &block);
        view.observe(&block);
        let capacity = 1_000u128 * PPM as u128;
        assert!(reward >= capacity, "the reward alone fills the bucket");
        for number in 2..=21i64 {
            let sender = [1u8, 3, 4][number as usize % 3];
            view.observe(&observation(number, sender, &[1, 2, 3, 4], &committee));
        }
        assert!(
            view.proposal(&validator(OBSERVER))
                .drained
                .contains(&validator(2)),
            "an honest view votes yea on ejecting W after W's fresh block"
        );
    }

    /// A freeze covers the same blocks however late a view learns of it: two
    /// views of the same blocks, one told of the ejection before them and one
    /// after, credit the same.
    #[test]
    fn a_freeze_covers_the_same_blocks_whenever_a_view_learns_of_it() {
        let committee = [(1u8, 1_000i64), (2, 1_000), (3, 1_000), (4, 1_000)];
        let ejected = frozen_floor(&committee, 2, &[open_from(3)]);
        let early = with_rate(rate_paying_four(500));
        let late = with_rate(rate_paying_four(500));
        early.reconcile(&floor(bonds(&committee), vec![], vec![]), 0);
        late.reconcile(&floor(bonds(&committee), vec![], vec![]), 0);
        early.reconcile(&ejected, 0);
        for number in 1..=8i64 {
            let sender = (number % 4 + 1) as u8;
            let block = observation(number, sender, &[1, 2, 3, 4], &committee);
            early.observe(&block);
            late.observe(&block);
        }
        late.reconcile(&ejected, 8);
        assert_eq!(
            early.status_of(&validator(2)),
            late.status_of(&validator(2)),
            "the same blocks under the same freeze credit the same"
        );
    }

    /// A view debits a claim when its floor first shows it paid, which can be
    /// long after the block that paid it. Subtracting only the claims paid
    /// inside a window therefore misses a debit applied inside it: here W's
    /// overflow is spent by a claim paid before the window, and the windowed
    /// bound still claims the window's spill.
    #[test]
    fn a_claim_paid_before_the_window_can_be_debited_inside_it() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let view = with_rate(WHOLE_UNITS_FOR_TWO / 2);
        view.reconcile(&floor(bonds(&committee), vec![], vec![]), 0);
        let mut window_spill = 0u128;
        for number in 1..=20i64 {
            let sender = (number % 2 + 1) as u8;
            let block = observation(number, sender, &[1, 2], &committee);
            // No entry before the first block: nothing credited yet.
            let before = view
                .status_of(&validator(2))
                .map(|status| status.overflow)
                .unwrap_or(0);
            view.observe(&block);
            let after = view.status_of(&validator(2)).expect("tracked").overflow;
            if number > 10 {
                window_spill += after - before;
            }
        }
        let total = view.status_of(&validator(2)).expect("tracked").overflow;
        let paid = (total / PPM as u128) as i64;
        view.reconcile(&claimed_floor(&committee, paid), 25);

        let overflow = owed(&view, &validator(2));
        let claimed_in_window = 0u128;
        assert!(window_spill > claimed_in_window);
        assert!(
            overflow < window_spill - claimed_in_window,
            "the windowed bound claims {} against an honest overflow of {overflow}",
            window_spill - claimed_in_window
        );
        assert!(
            overflow + paid as u128 * PPM as u128 >= window_spill,
            "subtracting everything ever paid keeps the bound under it"
        );
    }

    /// A view whose floor has not yet shown a rejoin holds W frozen past it,
    /// so a bound that counts W-blocks above the rejoin exceeds what that
    /// view holds under its own floor — and not what it holds under the floor
    /// that shows the rejoin.
    #[test]
    fn a_rejoin_the_view_has_not_reconciled_is_credited_under_the_floor_that_shows_it() {
        let committee = [(1u8, 1_000i64), (2, 1_000), (3, 1_000), (4, 1_000)];
        let view = with_rate(rate_paying_four(5_000));
        view.reconcile(&floor(bonds(&committee), vec![], vec![]), 0);
        view.reconcile(&frozen_floor(&committee, 2, &[open_from(1)]), 0);
        let capacity = 1_000u128 * PPM as u128;
        let rejoined_at = 10i64;
        let mut bound = 0u128;
        for number in 1..=40i64 {
            let sender = (number % 4 + 1) as u8;
            let block = observation(number, sender, &[1, 2, 3, 4], &committee);
            if sender == 2 && number > rejoined_at {
                bound += reward_of(&view, &block).saturating_sub(capacity);
            }
            view.observe(&block);
        }
        assert!(
            bound > owed(&view, &validator(2)),
            "under its own floor the view holds less than the bound"
        );
        let showing = Knowledge::of(
            &frozen_floor(&committee, 2, &[closed(1, rejoined_at, 0)]),
            0,
        );
        let under_showing = view
            .proposal_at(&validator(OBSERVER), &showing)
            .overflow
            .get(&validator(2))
            .copied()
            .expect("tracked");
        assert!(
            bound <= under_showing,
            "bound {bound} against {under_showing} under the floor that shows the rejoin"
        );
    }

    /// The log keeps only the blocks the retention window still covers below
    /// the floor, and what it prunes stays in the entries.
    #[test]
    fn the_log_is_pruned_below_the_retention_window_and_its_blocks_stay_counted() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let params = LeakyBucketParams {
            retention_window_blocks: Some(10),
            reward: paying(WHOLE_UNITS_FOR_TWO / 2),
            ..fast()
        };
        let pruned = BucketView::new(params);
        let kept = BucketView::new(LeakyBucketParams {
            retention_window_blocks: None,
            ..params
        });
        for view in [&pruned, &kept] {
            view.reconcile(&floor(bonds(&committee), vec![], vec![]), 0);
            for number in 1..=50i64 {
                let sender = (number % 2 + 1) as u8;
                view.observe(&observation(number, sender, &[1, 2], &committee));
            }
            view.reconcile(&floor(bonds(&committee), vec![], vec![]), 50);
        }
        assert_eq!(pruned.logged(), 11, "blocks 40 through 50");
        assert_eq!(kept.logged(), 50);
        assert_eq!(
            pruned.status_of(&validator(2)),
            kept.status_of(&validator(2))
        );

        let ejected = frozen_floor(&committee, 2, &[open_from(45)]);
        for view in [&pruned, &kept] {
            view.reconcile(&ejected, 50);
        }
        assert_eq!(
            pruned.status_of(&validator(2)),
            kept.status_of(&validator(2)),
            "a freeze inside the window rebuilds from what the log kept"
        );
    }

    /// A closed freeze the epoch sweep drops for age still covers its blocks,
    /// both when a later floor no longer records it and when a proposal is
    /// decided under such a floor.
    #[test]
    fn a_freeze_the_epoch_sweep_drops_still_covers_its_blocks() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let params = LeakyBucketParams {
            retention_window_blocks: Some(10),
            reward: paying(WHOLE_UNITS_FOR_TWO / 2),
            ..fast()
        };
        let swept = BucketView::new(params);
        let kept = BucketView::new(LeakyBucketParams {
            retention_window_blocks: None,
            ..params
        });
        let freeze = [closed(5, 8, 0)];
        for view in [&swept, &kept] {
            view.reconcile(&floor(bonds(&committee), vec![], vec![]), 0);
            for number in 1..=30i64 {
                let sender = (number % 2 + 1) as u8;
                view.observe(&observation(number, sender, &[1, 2], &committee));
            }
            view.reconcile(&frozen_floor(&committee, 2, &freeze), 10);
        }
        let without_it = floor(bonds(&committee), vec![], vec![]);
        let at_19 = Knowledge::of(&without_it, 19);
        assert_eq!(
            swept.proposal_at(&validator(OBSERVER), &at_19),
            swept.proposal(&validator(OBSERVER)),
            "a floor past the freeze's window decides as one that records it"
        );

        swept.reconcile(&without_it, 19);
        kept.reconcile(&frozen_floor(&committee, 2, &freeze), 19);
        assert_eq!(
            swept.status_of(&validator(2)),
            kept.status_of(&validator(2)),
            "the floor that swept the freeze leaves the bucket as one that kept it"
        );
    }

    /// The same blocks, in the same order, under the same floor, leave the
    /// same proposal whether the view learned that floor's freezes before the
    /// blocks or after them.
    mod replay {
        use proptest::prelude::*;

        use super::*;

        const W: u8 = 2;

        fn interval_set() -> impl Strategy<Value = Vec<FreezeInterval>> {
            prop::collection::vec((1i64..60, 0i64..30, 0i64..3_000, prop::bool::ANY), 0..3)
                .prop_map(|raw| {
                    let mut from = 0;
                    let mut intervals = Vec::new();
                    for (gap, length, topup, closed) in raw {
                        from += gap;
                        if closed {
                            let at = from + length;
                            intervals.push(FreezeInterval {
                                from,
                                rejoin: Some(Rejoin { at, topup }),
                            });
                            from = at + 1;
                        } else {
                            intervals.push(FreezeInterval { from, rejoin: None });
                            break;
                        }
                    }
                    intervals
                })
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 1024, .. ProptestConfig::default() })]

            #[test]
            fn replay_under_a_floor_equals_a_view_that_knew_it_from_the_start(
                rate in prop_oneof![0u64..30_000, 0u64..6_000_000],
                order in Just((1i64..=150).collect::<Vec<_>>()).prop_shuffle(),
                senders in prop::collection::vec(1u8..=4, 150),
                learn_at in 0usize..150,
                intervals in interval_set(),
            ) {
                let committee = [(1u8, 1_000i64), (W, 1_000), (3, 700), (4, 1_300)];
                let early = with_rate(rate);
                let late = with_rate(rate);
                let never = with_rate(rate);
                let empty = floor(bonds(&committee), vec![], vec![]);
                let frozen = frozen_floor(&committee, W, &intervals);
                for view in [&early, &late, &never] {
                    view.reconcile(&empty, 0);
                }
                early.reconcile(&frozen, 0);
                for (index, number) in order.iter().enumerate() {
                    if index == learn_at {
                        late.reconcile(&frozen, 0);
                    }
                    let sender = senders[*number as usize - 1];
                    let block = observation(*number, sender, &[1, 2, 3, 4], &committee);
                    for view in [&early, &late, &never] {
                        view.observe(&block);
                    }
                }
                let knowledge = Knowledge::of(&frozen, 0);
                let expected = early.proposal_at(&validator(OBSERVER), &knowledge);
                prop_assert_eq!(&late.proposal_at(&validator(OBSERVER), &knowledge), &expected);
                prop_assert_eq!(&never.proposal_at(&validator(OBSERVER), &knowledge), &expected);
                prop_assert_eq!(late.status_of(&validator(W)), early.status_of(&validator(W)));

                // A floor far enough above every closed freeze that the epoch
                // sweep has dropped them all.
                let open: Vec<FreezeInterval> = intervals
                    .iter()
                    .filter(|interval| interval.rejoin.is_none())
                    .copied()
                    .collect();
                let swept = Knowledge::of(&frozen_floor(&committee, W, &open), 20_000);
                for view in [&early, &late] {
                    prop_assert_eq!(&view.proposal_at(&validator(OBSERVER), &swept), &expected);
                }
            }
        }
    }

    mod strict_bound {
        use proptest::prelude::*;

        use super::*;

        const W: u8 = 2;

        #[derive(Clone, Debug)]
        struct Block {
            sender: u8,
            parents: Vec<u8>,
            w_stake: i64,
        }

        /// What the floor records at one stage: the first `starts` freezes are
        /// known, the first `ends` of them with their rejoins, and `claimed`
        /// has been paid.
        #[derive(Clone, Copy, Debug)]
        struct Stage {
            starts: usize,
            ends: usize,
            claimed: i64,
        }

        fn knowledge(intervals: &[FreezeInterval], stage: Stage) -> Vec<FreezeInterval> {
            intervals
                .iter()
                .take(stage.starts)
                .enumerate()
                .map(|(index, interval)| FreezeInterval {
                    from: interval.from,
                    rejoin: if index < stage.ends {
                        interval.rejoin
                    } else {
                        None
                    },
                })
                .collect()
        }

        fn stage_floor(intervals: &[FreezeInterval], stage: Stage) -> FloorState {
            let base = [(1u8, 1_000i64), (W, 1_000), (3, 700), (4, 1_300)];
            FloorState {
                claimed: vec![Claimed {
                    validator: validator(W),
                    total: stage.claimed,
                    payments: Vec::new(),
                }],
                ..frozen_floor(&base, W, &knowledge(intervals, stage))
            }
        }

        fn intervals() -> impl Strategy<Value = Vec<FreezeInterval>> {
            prop::collection::vec((1i64..80, 0i64..40, 0i64..3_000), 0..4).prop_map(|raw| {
                let mut from = 0;
                raw.into_iter()
                    .map(|(gap, length, topup)| {
                        from += gap;
                        let at = from + length;
                        let interval = FreezeInterval {
                            from,
                            rejoin: Some(Rejoin { at, topup }),
                        };
                        from = at + 1;
                        interval
                    })
                    .collect()
            })
        }

        fn blocks(count: usize) -> impl Strategy<Value = Vec<Block>> {
            prop::collection::vec(
                (1u8..=4, prop::collection::vec(1u8..=4, 0..4), 1i64..2_000).prop_map(
                    |(sender, parents, w_stake)| Block {
                        sender,
                        parents,
                        w_stake,
                    },
                ),
                count,
            )
        }

        /// What happens between admissions: nothing, the floor advancing a
        /// stage, or a payment.
        #[derive(Clone, Copy, Debug)]
        enum Between {
            Nothing,
            Advance,
            Pay(i64),
        }

        fn between() -> impl Strategy<Value = Between> {
            prop_oneof![
                6 => Just(Between::Nothing),
                2 => Just(Between::Advance),
                1 => (0i64..3_000).prop_map(Between::Pay),
            ]
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 4096, .. ProptestConfig::default() })]

            /// The bound the predicate slashes on — `Σ max(0, R(P) − C_P(W))`
            /// over the W-blocks a floor's freezes leave uncovered, less all W
            /// was ever paid by that floor — never exceeds the overflow an
            /// honest view holds under that same floor, whatever it drained,
            /// in whatever order it accepted the blocks, and however far its
            /// own floor lags or leads the one asked about.
            #[test]
            fn the_strict_bound_never_exceeds_an_honest_view_under_the_same_floor(
                rate in prop_oneof![0u64..30_000, 0u64..6_000_000],
                intervals in intervals(),
                block_list in blocks(200),
                order in Just((1i64..=200).collect::<Vec<_>>()).prop_shuffle(),
                gaps in prop::collection::vec(between(), 200),
                asked in prop::collection::vec(0usize..64, 200),
            ) {
                let view = with_rate(rate);
                let mut stages = vec![Stage { starts: 0, ends: 0, claimed: 0 }];
                view.reconcile(&stage_floor(&intervals, stages[0]), 0);
                let mut observed: Vec<(i64, Observation)> = Vec::new();
                for (step, number) in order.iter().enumerate() {
                    let current = *stages.last().expect("a stage");
                    match gaps[step] {
                        Between::Nothing => {}
                        Between::Advance => {
                            let next = if current.ends < current.starts {
                                Stage { ends: current.ends + 1, ..current }
                            } else if current.starts < intervals.len() {
                                Stage { starts: current.starts + 1, ..current }
                            } else {
                                current
                            };
                            stages.push(next);
                            view.reconcile(&stage_floor(&intervals, next), 0);
                        }
                        Between::Pay(amount) => {
                            let next = Stage { claimed: current.claimed + amount, ..current };
                            stages.push(next);
                            view.reconcile(&stage_floor(&intervals, next), 0);
                        }
                    }
                    let block = &block_list[*number as usize - 1];
                    let base = [(1u8, 1_000i64), (3, 700), (4, 1_300)];
                    let mut committee = base.to_vec();
                    committee.push((W, block.w_stake));
                    let observation = observation(*number, block.sender, &block.parents, &committee);
                    view.observe(&observation);
                    observed.push((block.w_stake, observation));

                    let stage = stages[asked[step] % stages.len()];
                    let frozen = knowledge(&intervals, stage);
                    let spill: u128 = observed
                        .iter()
                        .filter(|(_, obs)| obs.sender == validator(W))
                        .filter(|(_, obs)| !frozen.iter().any(|interval| interval.covers(obs.block_number)))
                        .map(|(w_stake, obs)| {
                            reward_of(&view, obs).saturating_sub(*w_stake as u128 * PPM as u128)
                        })
                        .sum();
                    let bound = spill.saturating_sub(stage.claimed as u128 * PPM as u128);
                    let holds = view
                        .proposal_at(&validator(OBSERVER), &Knowledge::of(&stage_floor(&intervals, stage), 0))
                        .overflow
                        .get(&validator(W))
                        .copied()
                        .unwrap_or(0);
                    prop_assert!(bound <= holds, "bound {} over overflow {}", bound, holds);
                }
            }
        }
    }

    /// A nay stays offered until the floor shows its voter without stake.
    #[test]
    fn a_nay_is_offered_until_the_floor_shows_its_voter_slashed() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let view = BucketView::new(fast());
        view.reconcile(&floor(bonds(&committee), vec![], vec![]), 0);
        let claim = overflow_claim(1, 10).claim;
        view.observe(&Observation {
            overflow_nays: vec![claim.clone()],
            ..observation(1, 2, &[1, 2], &committee)
        });
        let offered = |view: &BucketView| {
            view.proposal(&validator(OBSERVER))
                .accusations
                .iter()
                .map(|pending| pending.accusation.accused.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(offered(&view), vec![validator(2)]);

        view.reconcile(&floor(bonds(&committee), vec![], vec![]), 1);
        assert_eq!(offered(&view), vec![validator(2)], "still holding stake");

        view.reconcile(&floor(bonds(&[(1, 1_000), (2, 0)]), vec![], vec![]), 2);
        assert!(offered(&view).is_empty(), "slashed");
    }

    /// A freeze stops the drain, not the bond: a stake that changes while
    /// frozen moves the level with it, so the shortfall a rejoin must cover
    /// and the surplus it credits are measured against the stake held.
    #[test]
    fn a_bond_change_during_a_freeze_reseats_the_frozen_bucket() {
        let full = [(1u8, 1_000i64), (2, 1_000)];
        let halved = [(1u8, 1_000i64), (2, 500)];
        let view = rewardless();
        view.reconcile(&floor(bonds(&full), vec![], vec![]), 0);
        for number in 1..=20i64 {
            view.observe(&observation(number, 1, &[1], &full));
        }
        view.reconcile(&frozen_floor(&halved, 2, &[open_from(21)]), 20);
        for number in 21..=30i64 {
            view.observe(&observation(number, 1, &[1], &halved));
        }
        let shortfall = view.proposal(&validator(OBSERVER)).shortfall[&validator(2)];
        assert_eq!(
            shortfall,
            95 * PPM as u128,
            "19 drains leave 81% of the bond, so 19% of the 500 held is short"
        );

        view.reconcile(&frozen_floor(&halved, 2, &[closed(21, 30, 150)]), 30);
        view.observe(&observation(31, 1, &[1], &halved));
        assert_eq!(
            view.status_of(&validator(2)).expect("tracked").overflow,
            55 * PPM as u128,
            "a topup of 150 refills the 95 short and credits the rest"
        );
    }

    /// A late block numbered below a slash still lists the slashed validator
    /// and may carry its nays, but the view lets it back in neither as a
    /// bucket nor as an accusation.
    #[test]
    fn a_late_block_does_not_bring_back_a_slashed_validator() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let view = BucketView::new(fast());
        view.reconcile(&floor(bonds(&committee), vec![], vec![]), 0);
        for number in 1..=10i64 {
            view.observe(&observation(number, 1, &[1, 2], &committee));
        }
        view.reconcile(&floor(bonds(&[(1, 1_000), (2, 0)]), vec![], vec![]), 10);
        view.observe(&Observation {
            overflow_nays: vec![overflow_claim(1, 10).claim],
            ..observation(5, 2, &[1, 2], &committee)
        });
        assert!(view.status_of(&validator(2)).is_none(), "no bucket");
        assert!(
            view.proposal(&validator(OBSERVER))
                .accusations
                .iter()
                .all(|pending| pending.accusation.accused != validator(2)),
            "no accusation"
        );
    }

    /// A member this node first sees while it is ejected is kept, frozen at
    /// the threshold its ejection found it below.
    #[test]
    fn a_member_first_seen_ejected_starts_at_the_threshold() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let view = BucketView::new(fast());
        view.reconcile(&frozen_floor(&committee, 2, &[open_from(0)]), 0);
        for number in 1..=100i64 {
            view.observe(&observation(number, 1, &[1], &committee));
        }
        let entry = view.status_of(&validator(2)).expect("an entry is kept");
        assert_eq!(
            entry.level,
            entry.capacity as u128 * fast().theta_eject_ppm as u128
        );
    }

    /// A view rebuilt after an ejection that lies below its window cannot see
    /// the drain, and backs a rejoin only for the shortfall the ejection
    /// proved, not any topup at all.
    #[test]
    fn a_view_rebuilt_during_a_grace_backs_only_the_proven_shortfall() {
        let committee = [(1u8, 1_000i64), (2, 1_000)];
        let rebuilt = rewardless();
        rebuilt.begin(&frozen_floor(&committee, 2, &[open_from(30)]), 100, 50);
        for number in 50..=100i64 {
            rebuilt.observe(&observation(number, 1, &[1], &committee));
        }
        let proposal = rebuilt.proposal(&validator(OBSERVER));
        assert!(!proposal.votes_yea(&rejoin_claim(2, 1)));
        assert!(!proposal.votes_yea(&rejoin_claim(2, 99)));
        assert!(
            proposal.votes_yea(&rejoin_claim(2, 100)),
            "a tenth of the bond lifts it from the threshold to full"
        );
    }
}
