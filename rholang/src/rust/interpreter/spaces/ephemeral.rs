//! The ephemeral spaces of one root evaluation (P7: I-4, decision 19 D1, design v3 VR-1 to VR-10
//! and DG-1).
//!
//! A `temp` or `seq` space lives for one deploy. Its ids use the lowercase KINDs (I-4), and its
//! space id comes from the message that created it (I-19), so no other deploy can make its ids.
//! The end of the root evaluation that created it sweeps its records, its waiting continuations
//! and its join entries out of the hot store (`SpaceTxnHost::sweep_volatile`), so its state
//! never reaches a checkpoint.
//!
//! [`VolatileRegistry`] is the book of one root evaluation (VR-1). It holds the ephemeral spaces
//! that the evaluation created, with their qualifiers, and the volatile channels that the
//! transactions of the evaluation locked. It is node bookkeeping, not consensus state:
//!
//! * The creation job records a space after its commit, as the last step of the job (VR-2,
//!   decision 83). The factory replies after the job, so every holder of the handle or of a
// Changed by decision 90: the scheduler retired, so no window bounds a read. The text before:
// //!   name of the space causally follows the record. The scheduler runs no reduction
// //!   participant during a job, so the book changes only between two scheduling windows, and
// //!   every read of one window sees one book.
//!   name of the space causally follows the record. A mutex guards the book, so each read
//!   sees one consistent state of the book.
//! * `SpaceEnv::begin` records each lock key with an ephemeral KIND. The lock keys of a
//!   transaction hold every channel that it stages (readiness review S2, FR-4), so the book
//!   holds every volatile channel that a commit can write.
//! * `ReifiedRuntime::begin_evaluation_scope` clears the book at the start of a root evaluation,
//!   and the sweep drains it at the end (VR-3, VR-4).
//!
//! An id with an ephemeral KIND whose space is not in the book belongs to a deploy that has
//! ended, so the entry points of DG-1 fail with `DanglingHandle` or `DanglingName` before any job.
//!
//! **Seq claims (design v3 SQ-3, SQ-4, decisions 73 and 91).** Each channel of a `seq` space has
//! its own claim in the book: the number of its last use. [`SeqClaims`] holds the claims of one
//! program job. A claim of a channel is admitted when the channel has no claim or its last use
//! is in the causal history of the job (`reified::seq::SeqHistory`). The job stores its claims
//! only after its commit succeeded, so a failed job or a dropped transaction changes no claim.
//! The stripes of the transaction cover each channel that the job claims, so two jobs on one
//! channel never check one claim before either stores (the atomic guard).
//!
//! Changed by decision 91 (DR-132): racing evaluation sets no operation order, so a claim is no
//! longer the participant path of the scheduler. The book also holds the causal history of each
//! item that a task with a history stored, by its random state, for the fired bodies.
//!
//! Changed by decision 90 (decision 91 SQ-4): the store runs in the finish of the job, and the
//! finish runs under the stripes of the commit (`Prepared::commit_holding`). Before, the commit
//! released the stripes first, so under racing evaluation a second job could check a claim
//! before the first job stored it (`SeqGuardToctouUnsafe` of `SpaceCoordination.tla`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use models::rhoapi::Par;
// Added by decision 124 (L14, plan v4 C7.8, rule CR-20): the book holds source hashes.
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
// Added by decision 124 (plan v4 C7.19, ruling RADD 1.8): the finish stores the claims of the
// actual unit of row d.
use rspace_plus_plus::rspace::space_store_handle::RowClaims;

// Changed by decision 91: the claims no longer read the operation order.
// use rspace_plus_plus::rspace::operation_context::{self, PathSegment};
use super::errors::SpaceError;
use super::namespace::{
    decode_reified_id, sole_gprivate_id, Durability, NativeSpace, ReifiedId, SpaceId,
};
use super::types::qualifier::SpaceQualifier;
use crate::rust::interpreter::reified::seq::{self as seq_history, SeqHistory};

/// The space of a space-scoped id with an ephemeral KIND: a config, counter, data, path-index,
/// allocator, handle or name id of a `temp` or `seq` space. Every other id gives `None`.
pub fn ephemeral_space(id: &[u8]) -> Option<NativeSpace> {
    let space = match decode_reified_id(id)? {
        ReifiedId::Config(space)
        | ReifiedId::SpaceCounter(space)
        | ReifiedId::CollectionCounter(space, _)
        | ReifiedId::Data(space, _)
        | ReifiedId::PathIndex(space, _)
        | ReifiedId::Allocator(space)
        | ReifiedId::Handle(space)
        | ReifiedId::Name(space, _) => space,
        ReifiedId::Op(_)
        | ReifiedId::Matcher(..)
        | ReifiedId::Body(..)
        | ReifiedId::Reserved(..) => return None,
    };
    match space.durability {
        Durability::Ephemeral => Some(space),
        Durability::Durable => None,
    }
}

/// The book of one root evaluation: its ephemeral spaces and its volatile channels.
#[derive(Debug, Default)]
pub struct VolatileRegistry {
    state: Mutex<VolatileState>,
}

/// Added by decision 124 (L14, plan v4 C7.8, rule CR-20): the pairs of the book of a root
/// evaluation.
pub type BookPairs = BTreeMap<Blake2b256Hash, BTreeSet<Blake2b256Hash>>;

#[derive(Debug, Default)]
struct VolatileState {
    /// The ephemeral spaces that the evaluation created, by space id.
    spaces: BTreeMap<SpaceId, SpaceQualifier>,
    /// The volatile channels that the transactions of the evaluation locked, by `GPrivate` id.
    channels: BTreeMap<Vec<u8>, Par>,
    // Changed by decision 91: a claim is the number of the last use.
    // /// The claims of the channels of the `seq` spaces (SQ-3): the participant path of the last
    // /// use of each claimed channel, by `GPrivate` id.
    // claims: BTreeMap<Vec<u8>, Vec<PathSegment>>,
    /// The claims of the channels of the `seq` spaces (QF-15): the number of the last use of
    /// each claimed channel, by `GPrivate` id. The first use of a channel has the number 1.
    claims: BTreeMap<Vec<u8>, u64>,
    /// Added by decision 91: the causal history of each item that a task with a history stored,
    /// by the random state of the item.
    items: BTreeMap<Vec<u8>, SeqHistory>,
    /// Added by decision 124 (L14, plan v4 C7.8, rule CR-20): the refused and item pairs of the
    /// evaluation, record source hash to continuation source hashes. A plan reads one snapshot.
    book: Arc<BookPairs>,
}

impl VolatileRegistry {
    fn state(&self) -> std::sync::MutexGuard<'_, VolatileState> {
        self.state.lock().expect("volatile registry lock")
    }

    /// Records each key of `keys` that is a space-scoped id with an ephemeral KIND.
    pub fn record_locked(&self, keys: &[Par]) {
        let mut volatile = keys.iter().filter_map(|key| {
            sole_gprivate_id(key)
                .filter(|id| ephemeral_space(id).is_some())
                .map(|id| (id.to_vec(), key.clone()))
        });
        let Some(first) = volatile.next() else {
            return;
        };
        let mut state = self.state();
        state.channels.insert(first.0, first.1);
        state.channels.extend(volatile);
    }

    /// Records the ephemeral space `space` with its qualifier, after its creation committed.
    pub fn record_space(&self, space: NativeSpace, qualifier: SpaceQualifier) {
        self.state().spaces.insert(space.sid, qualifier);
    }

    /// Records the space `space` that a creation job committed, when it is ephemeral (VR-2,
    /// decision 83). A durable space is not recorded. The creation job calls it as the last step
    /// of its commit, so the book changes only inside a job.
    pub fn record_created(&self, space: NativeSpace, qualifier: SpaceQualifier) {
        match space.durability {
            Durability::Ephemeral => self.record_space(space, qualifier),
            Durability::Durable => {}
        }
    }

    /// The qualifier of the ephemeral space `sid` when the current evaluation created it.
    pub fn qualifier(&self, sid: &SpaceId) -> Option<SpaceQualifier> {
        self.state().spaces.get(sid).copied()
    }

    /// True when the current evaluation created the ephemeral space `sid`.
    pub fn is_live(&self, sid: &SpaceId) -> bool { self.state().spaces.contains_key(sid) }

    /// True when a transaction of the current evaluation locked a volatile channel.
    pub fn has_channels(&self) -> bool { !self.state().channels.is_empty() }

    /// Takes the volatile channels of the evaluation, in the order of their ids, and leaves the
    /// set of channels empty. The spaces stay in the book until the next clear.
    pub fn take_channels(&self) -> Vec<Par> {
        std::mem::take(&mut self.state().channels)
            .into_values()
            .collect()
    }

    /// The `seq` spaces that the current evaluation created (SQ-1).
    pub fn seq_spaces(&self) -> BTreeSet<SpaceId> {
        self.state()
            .spaces
            .iter()
            .filter(|(_, qualifier)| **qualifier == SpaceQualifier::Seq)
            .map(|(sid, _)| *sid)
            .collect()
    }

    /// True when the current evaluation created a `seq` space.
    pub fn has_seq_space(&self) -> bool {
        self.state()
            .spaces
            .values()
            .any(|qualifier| *qualifier == SpaceQualifier::Seq)
    }

    // Changed by decision 91: a claim is the number of the last use.
    // /// The claim of the channel with the id `id`.
    // fn claim_of(&self, id: &[u8]) -> Option<Vec<PathSegment>> {
    //     self.state().claims.get(id).cloned()
    // }
    //
    // /// Sets the claim of each channel of `ids` to `path`.
    // fn store_claims(&self, ids: BTreeSet<Vec<u8>>, path: &[PathSegment]) {
    //     let mut state = self.state();
    //     for id in ids {
    //         state.claims.insert(id, path.to_vec());
    //     }
    // }

    /// The number of the last use of the channel with the id `id`, when it has a claim.
    fn claim_of(&self, id: &[u8]) -> Option<u64> { self.state().claims.get(id).copied() }

    /// Gives each channel of `ids` its next use, and returns each channel with the number of
    /// that use (`CommitUse` of `SpaceCoordination.tla`).
    fn store_claims(&self, ids: BTreeSet<Vec<u8>>) -> Vec<(Vec<u8>, u64)> {
        let mut state = self.state();
        let mut uses = Vec::with_capacity(ids.len());
        for id in ids {
            let number = state.claims.get(&id).copied().unwrap_or(0) + 1;
            state.claims.insert(id.clone(), number);
            uses.push((id, number));
        }
        uses
    }

    /// Added by decision 91: records `history` as the causal history of the item with the
    /// random state `random_state`. An empty history records nothing.
    pub fn record_item(&self, random_state: &[u8], history: SeqHistory) {
        if history.is_empty() {
            return;
        }
        self.state().items.insert(random_state.to_vec(), history);
    }

    /// Added by decision 91: the causal history of the item with the random state
    /// `random_state`, or the empty history.
    pub fn item_history(&self, random_state: &[u8]) -> SeqHistory {
        self.state()
            .items
            .get(random_state)
            .cloned()
            .unwrap_or_default()
    }

    /// Added by decision 91: the union of the causal histories of the items with the random
    /// states `random_states`: the history of a fired body. A book with no item history gives
    /// the empty history without a lookup.
    pub fn union_of_items<'a>(
        &self,
        random_states: impl IntoIterator<Item = &'a [u8]>,
    ) -> SeqHistory {
        let state = self.state();
        if state.items.is_empty() {
            return SeqHistory::default();
        }
        random_states
            .into_iter()
            .filter_map(|random_state| state.items.get(random_state))
            .fold(SeqHistory::default(), |history, item| history.join(item))
    }

    /// Added by decision 91: true when the book holds an item history, so a fired body may
    /// need one.
    pub fn has_item_histories(&self) -> bool { !self.state().items.is_empty() }

    /// Added by decision 124 (L14, plan v4 C7.8): a snapshot of the book.
    pub fn book(&self) -> Arc<BookPairs> { Arc::clone(&self.state().book) }

    /// Added by decision 124 (L14, L15, plan v4 C7.8): books `pairs`. An empty list takes no
    /// lock.
    pub fn book_pairs(&self, pairs: impl IntoIterator<Item = (Blake2b256Hash, Blake2b256Hash)>) {
        let mut pairs = pairs.into_iter().peekable();
        let Some(_) = pairs.peek() else {
            return;
        };
        let mut state = self.state();
        let book = Arc::make_mut(&mut state.book);
        for (record, continuation) in pairs {
            book.entry(record).or_default().insert(continuation);
        }
    }

    /// Empties the book.
    pub fn clear(&self) {
        let mut state = self.state();
        state.spaces.clear();
        state.channels.clear();
        state.claims.clear();
        // Added by decision 91.
        state.items.clear();
        // Added by decision 124 (L14, plan v4 C7.8): the book starts empty with the evaluation.
        state.book = Arc::default();
    }
}

/// The role in which an operation uses a channel of a `seq` space (SQ-3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimRole {
    /// The channel holds the records that the operation stores, restores, delivers or removes.
    /// Only a data channel `d(sid, k)` is claimed in this role: a marker on a path-index channel
    /// is bookkeeping and claims nothing.
    Data,
    /// The channel is in the group of a waiting receive: a data channel `d(sid, k)` of a keyed
    /// store or a join, or a path-index channel `i(sid, q)` of a PathMap receive on the prefix
    /// `q`.
    Waiter,
}

/// Added by decision 124 (plan v4 C7.19, CR-24, ruling RADD 1.8): the id of `channel` when a
/// job of the `seq` space `space` claims it in the role `role` (QF-15), or `None`. Only a data
/// channel is claimed as `Data`, a data or path-index channel as `Waiter`, and only a channel of
/// the ephemeral space itself. The staging (`SeqClaims::claim`), the commit (`SeqReads::admits`)
/// and the finish (`SeqClaims::store`) read this one rule.
fn claimable_id<'p>(space: &SpaceId, channel: &'p Par, role: ClaimRole) -> Option<&'p [u8]> {
    let id = sole_gprivate_id(channel)?;
    match (decode_reified_id(id), role) {
        (Some(ReifiedId::Data(owner, _)), ClaimRole::Data | ClaimRole::Waiter)
        | (Some(ReifiedId::PathIndex(owner, _)), ClaimRole::Waiter) => {
            match owner.sid == *space && owner.durability == Durability::Ephemeral {
                false => None,
                true => Some(id),
            }
        }
        (Some(_), _) | (None, _) => None,
    }
}

/// Added by decision 124 (plan v4 C7.19, CR-24, ruling RADD 1.8): true when a job with the
/// causal history `history` may use the channel `id`: the channel has no claim, or its last use
/// is in the history (`Admissible` of `SpaceCoordination.tla`). It locks the registry for one
/// read (`claim_of`).
fn admitted(registry: &VolatileRegistry, history: &SeqHistory, id: &[u8]) -> bool {
    let Some(last) = registry.claim_of(id) else {
        return true;
    };
    history.knows(id, last)
}

/// The claims of one program job on the channels of one `seq` space (SQ-3, SQ-4).
pub struct SeqClaims {
    registry: Arc<VolatileRegistry>,
    space: SpaceId,
    // Changed by decision 91: the causal history of the job replaces the participant path.
    // /// The participant path of the job: the path of its operation order without the last
    // /// segment `(step, 0)` (F12).
    // path: Vec<PathSegment>,
    /// The causal history of the job: the history of its task when the job opens its claims.
    history: SeqHistory,
    /// The random state of the own item of the job: its datum or its continuation.
    item: Vec<u8>,
    // Changed by decision 124 (plan v4 C7.19, rule CR-24): each claim names its staged row. The
    // text before:
    // ```
    // /// The ids of the channels that the job claimed, which `store` makes its claims.
    // pending: BTreeSet<Vec<u8>>,
    // ```
    /// The ids of the channels that the job claimed, each with the staged row that took the
    /// claim. `store` makes the claims of the committed rows its claims.
    pending: BTreeMap<Vec<u8>, usize>,
    /// The ids that the job claimed after its last staged row. The next row that is not an
    /// anchor takes them, so a claim before a failing stage call takes no row (CR-6).
    open: Vec<Vec<u8>>,
}

/// Added by decision 102 (plan v4 C4.3, rule CR-14): the pending claims of a job at one point of
/// its staging ([`SeqClaims::snapshot`]). Changed by decision 124 (plan v4 C7.19): the open
/// claims too, and the row of each pending claim.
pub struct SeqSnapshot {
    pending: BTreeMap<Vec<u8>, usize>,
    open: Vec<Vec<u8>>,
}

/// Added by decision 124 (plan v4 C7.19, CR-24 bullet 3, ruling RADD 1.8): an owned read handle
/// of the claims of one job, which a row search holds to check the claims of an alternative at
/// the commit. It reads the rule of `SeqClaims::claim` without the insert. Only an owner-funded
/// run of a program job on a `seq` space builds one (RC2 D-10).
#[derive(Clone)]
pub struct SeqReads {
    registry: Arc<VolatileRegistry>,
    space: SpaceId,
    history: SeqHistory,
}

impl SeqReads {
    /// True when the job may claim `channel` in the role `role`: the channel is not claimable,
    /// or its claim is admissible. One read of the registry for a claimable channel.
    pub fn admits(&self, channel: &Par, role: ClaimRole) -> bool {
        let Some(id) = claimable_id(&self.space, channel, role) else {
            return true;
        };
        admitted(&self.registry, &self.history, id)
    }
}

impl SeqClaims {
    /// The claims of a job on `space`, or `None` when no claim applies: the space is not a `seq`
    /// space, or the job is internal (a re-issue or a settle job, SQ-3). `item` is the random
    /// state of the own item of the job: its datum or its continuation.
    ///
    /// Changed by decision 91 (DR-132): the claims take the causal history of the task of the
    /// job. Before, they took the participant path of the operation order, and a program job
    /// without one failed with `Inconsistent`, which racing evaluation made every job do
    /// (`ClaimsNeedContextUnsafe` of `SpaceCoordination.tla`).
    pub fn open(
        registry: &Arc<VolatileRegistry>,
        space: NativeSpace,
        qualifier: SpaceQualifier,
        internal: bool,
        item: &[u8],
    ) -> Result<Option<Self>, SpaceError> {
        if qualifier != SpaceQualifier::Seq || internal {
            return Ok(None);
        }
        // let order = operation_context::current().ok_or(SpaceError::Inconsistent(
        //     "a program operation on a seq space runs without an operation order",
        // ))?;
        // let mut path = order.path.to_vec();
        // path.pop().ok_or(SpaceError::Inconsistent(
        //     "the operation order of a job has no step",
        // ))?;
        Ok(Some(Self {
            registry: Arc::clone(registry),
            space: space.sid,
            history: seq_history::current_history(),
            item: item.to_vec(),
            // Changed by decision 124 (plan v4 C7.19): the claims start empty and open. The text
            // before:
            // pending: BTreeSet::new(),
            pending: BTreeMap::new(),
            open: Vec::new(),
        }))
    }

    /// Claims `channel` in the role `role` (QF-15). A channel that is not claimable in that role,
    /// or that belongs to another space, claims nothing. The claim is admitted when the channel
    /// has no claim or its last use is in the causal history of the job (`Admissible` of
    /// `SpaceCoordination.tla`), and it is then pending until [`SeqClaims::store`]. Otherwise
    /// the deploy aborts with `SeqConcurrent`.
    pub fn claim(&mut self, channel: &Par, role: ClaimRole) -> Result<(), SpaceError> {
        // Changed by decision 124 (plan v4 C7.19, ruling RADD 1.8): the commit reads the same
        // claim rule (`claimable_id`). The text before:
        // ```
        // let Some(id) = sole_gprivate_id(channel) else {
        //     return Ok(());
        // };
        // let claimable = match (decode_reified_id(id), role) {
        //     (Some(ReifiedId::Data(space, _)), ClaimRole::Data | ClaimRole::Waiter)
        //     | (Some(ReifiedId::PathIndex(space, _)), ClaimRole::Waiter) => {
        //         space.sid == self.space && space.durability == Durability::Ephemeral
        //     }
        //     (Some(_), _) | (None, _) => false,
        // };
        // if !claimable {
        //     return Ok(());
        // }
        // ```
        let Some(id) = claimable_id(&self.space, channel, role) else {
            return Ok(());
        };
        // Changed by decision 124 (plan v4 C7.19): a claim of the job is pending or open. The
        // text before:
        // ```
        // if self.pending.contains(id) {
        //     return Ok(());
        // }
        // ```
        let claimed = self.pending.contains_key(id) || self.open.iter().any(|open| open == id);
        match claimed {
            false => {}
            true => return Ok(()),
        }
        // Changed by decision 91: the last use must be in the history of the job.
        // if let Some(claim) = self.registry.claim_of(id) {
        //     if !self.path.starts_with(&claim) {
        //         return Err(SpaceError::SeqConcurrent { space: self.space });
        //     }
        // }
        // Changed by decision 124 (plan v4 C7.19, ruling RADD 1.8): the commit reads the same
        // admission (`admitted`).
        match admitted(&self.registry, &self.history, id) {
            false => return Err(SpaceError::SeqConcurrent { space: self.space }),
            true => {}
        }
        // Changed by decision 124 (plan v4 C7.19): the claim is open until its row stages. The
        // text before:
        // self.pending.insert(id.to_vec());
        self.open.push(id.to_vec());
        Ok(())
    }

    /// Added by decision 102 (plan v4 C4.3, rule CR-14): the pending claims so far. R-A takes
    /// the snapshot after the claim of the own collection of the job.
    pub fn snapshot(&self) -> SeqSnapshot {
        SeqSnapshot {
            pending: self.pending.clone(),
            // Added by decision 124 (plan v4 C7.19): the open claims.
            open: self.open.clone(),
        }
    }

    /// Added by decision 124 (plan v4 C7.19, ruling RADD 1.8): the read handle of these claims.
    pub fn reads(&self) -> SeqReads {
        SeqReads {
            registry: Arc::clone(&self.registry),
            space: self.space,
            history: self.history.clone(),
        }
    }

    /// Added by decision 102 (plan v4 C4.3, rule CR-14): drops the pending claims that the job
    /// made after `snapshot`, before the restage of R-A. A pending claim reaches the registry
    /// only through [`SeqClaims::store`] after the commit, so the rewind changes only the job.
    // Changed by decision 124 (plan v4 C7.19): the rewind restores the open claims too.
    pub fn rewind(&mut self, snapshot: &SeqSnapshot) {
        self.pending.clone_from(&snapshot.pending);
        self.open.clone_from(&snapshot.open);
    }

    /// Added by decision 124 (plan v4 C7.19, rule CR-24): the open claims belong to the staged
    /// row `row`. The plan calls it for each staged row that is not an anchor, and once with
    /// row 0 before the staging, for the claims of the own collection.
    pub fn close_row(&mut self, row: usize) {
        for id in self.open.drain(..) {
            self.pending.entry(id).or_insert(row);
        }
    }

    /// Stores the pending claims (SQ-4): each claimed channel gets its next use. The job calls
    /// it only after its commit succeeded. The uses join the history of the task of the job and
    /// the history of its own item, so the reply of the job and every body that the item fires
    /// know them (`CommitUse` of `SpaceCoordination.tla`).
    ///
    /// Changed by decision 91: before, it stored the participant path of the job.
    // pub fn store(self) { self.registry.store_claims(self.pending, &self.path); }
    // Changed by decision 124 (plan v4 C7.19, rule CR-24, ruling RADD 1.8): the claims of the
    // committed rows, the first `rows` staged rows, reach the registry, with the claims `actual`
    // of the actual unit of row d when it has some (CR-24, bullet 2).
    pub fn store(self, rows: usize, actual: Option<RowClaims<Par>>) {
        let mut claims: BTreeSet<Vec<u8>> = self
            .pending
            .into_iter()
            .filter(|(_, row)| *row < rows)
            .map(|(id, _)| id)
            .collect();
        match actual {
            None => {}
            Some(RowClaims { data, waiters }) => {
                let data = data.iter().map(|channel| (channel, ClaimRole::Data));
                let waiters = waiters.iter().map(|channel| (channel, ClaimRole::Waiter));
                for (channel, role) in data.chain(waiters) {
                    match claimable_id(&self.space, channel, role) {
                        None => {}
                        Some(id) => {
                            claims.insert(id.to_vec());
                        }
                    }
                }
            }
        }
        let uses = self.registry.store_claims(claims);
        match uses.is_empty() {
            false => {}
            true => return,
        }
        seq_history::learn(&uses);
        self.registry
            .record_item(&self.item, self.history.with_uses(&uses));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::spaces::namespace::{
        self, gprivate, CollectionKey, NameTail, SID_LEN,
    };

    fn space(seed: u8, durability: Durability) -> NativeSpace {
        NativeSpace {
            sid: [seed; SID_LEN],
            durability,
        }
    }

    fn key(bytes: &[u8]) -> CollectionKey {
        CollectionKey::from_bytes(bytes.to_vec()).expect("a collection key")
    }

    #[test]
    fn ephemeral_space_accepts_every_space_scoped_ephemeral_kind() {
        let temp = space(3, Durability::Ephemeral);
        let ids = [
            namespace::config_channel(temp),
            namespace::space_counter(temp),
            namespace::collection_counter(temp, &key(&[1])),
            namespace::data_channel(temp, &key(&[1])),
            namespace::path_index_channel(temp, &[2]),
            namespace::allocator_channel(temp),
            namespace::handle_channel(temp),
            namespace::name_id(temp, NameTail::Random([4; 32])),
            namespace::name_id(temp, NameTail::Index(7)),
        ];
        for id in ids {
            assert_eq!(ephemeral_space(&id), Some(temp), "{id:?}");
        }
    }

    #[test]
    fn ephemeral_space_rejects_durable_ids_ops_and_foreign_ids() {
        let durable = space(3, Durability::Durable);
        let ids = [
            namespace::config_channel(durable),
            namespace::data_channel(durable, &key(&[1])),
            namespace::handle_channel(durable),
            namespace::name_id(durable, NameTail::Random([4; 32])),
            namespace::op_channel(namespace::HANDLE_OP),
            vec![1; 32],
            Vec::new(),
        ];
        for id in ids {
            assert_eq!(ephemeral_space(&id), None, "{id:?}");
        }
    }

    #[test]
    fn volatile_registry_records_only_ephemeral_lock_keys_in_id_order() {
        let registry = VolatileRegistry::default();
        let temp = space(9, Durability::Ephemeral);
        let durable = space(9, Durability::Durable);
        let data = gprivate(namespace::data_channel(temp, &key(&[5])));
        let config = gprivate(namespace::config_channel(temp));
        registry.record_locked(&[
            gprivate(namespace::config_channel(durable)),
            data.clone(),
            config.clone(),
            data.clone(),
            Par::default(),
        ]);
        assert!(registry.has_channels());
        let mut expected = vec![config, data];
        expected.sort_by_key(|par| sole_gprivate_id(par).expect("an id").to_vec());
        assert_eq!(registry.take_channels(), expected);
        assert!(!registry.has_channels());
        assert!(registry.take_channels().is_empty());
    }

    // Disabled by decision 91 (DR-132): these tests check the participant-path claims of the
    // removed scheduler (decision 73). The tests below check the causal-history claims.
    // /// Runs `body` with the claims of a program job on `space`, inside the operation order
    // /// whose path is `path` followed by the step `(9, 0)`. So the participant path of the job is
    // /// `path`.
    // async fn job<T>(
    //     registry: &Arc<VolatileRegistry>,
    //     space: NativeSpace,
    //     path: &[PathSegment],
    //     body: impl FnOnce(SeqClaims) -> T,
    // ) -> T {
    //     let mut order = path.to_vec();
    //     order.push((9, 0));
    //     let order = operation_context::OperationOrder {
    //         session: [7; 32],
    //         path: order.into(),
    //     };
    //     operation_context::scope(order, async {
    //         let claims = SeqClaims::open(registry, space, SpaceQualifier::Seq, false)
    //             .expect("a seq job opens its claims")
    //             .expect("a program job on a seq space claims");
    //         body(claims)
    //     })
    //     .await
    // }
    //
    // fn data(space: NativeSpace, byte: u8) -> Par {
    //     gprivate(namespace::data_channel(space, &key(&[byte])))
    // }
    //
    // #[tokio::test]
    // async fn seq_claims_apply_only_to_program_jobs_on_seq_spaces() {
    //     let registry = Arc::new(VolatileRegistry::default());
    //     let seq = space(4, Durability::Ephemeral);
    //     for (qualifier, internal) in [
    //         (SpaceQualifier::Default, false),
    //         (SpaceQualifier::Temp, false),
    //         (SpaceQualifier::Seq, true),
    //     ] {
    //         let claims = SeqClaims::open(&registry, seq, qualifier, internal);
    //         assert!(matches!(claims, Ok(None)), "{qualifier:?} {internal}");
    //     }
    //     let unordered = SeqClaims::open(&registry, seq, SpaceQualifier::Seq, false);
    //     assert!(matches!(
    //         unordered,
    //         Err(SpaceError::Inconsistent(
    //             "a program operation on a seq space runs without an operation order"
    //         ))
    //     ));
    //     let opened = job(&registry, seq, &[(0, 1), (4, 2)], |claims| {
    //         claims.path.clone()
    //     })
    //     .await;
    //     assert_eq!(
    //         opened,
    //         vec![(0, 1), (4, 2)],
    //         "the participant path of the job"
    //     );
    // }
    //
    // #[tokio::test]
    // async fn seq_claims_admit_exactly_the_descendants_of_the_last_use() {
    //     let registry = Arc::new(VolatileRegistry::default());
    //     let seq = space(4, Durability::Ephemeral);
    //     let x = data(seq, 1);
    //     let id = sole_gprivate_id(&x).expect("an id").to_vec();
    //     let used = |mut claims: SeqClaims| claims.claim(&x, ClaimRole::Data).map(|_| claims);
    //     // A job that fails or drops its transaction stores no claim (SQ-4).
    //     job(&registry, seq, &[(0, 1)], |claims| {
    //         used(claims).expect("a first use")
    //     })
    //     .await;
    //     assert_eq!(
    //         registry.claim_of(&id),
    //         None,
    //         "a pending claim is not stored"
    //     );
    //     // A job stores its claims after its commit succeeded.
    //     job(&registry, seq, &[(0, 1)], |claims| {
    //         used(claims).expect("a first use").store()
    //     })
    //     .await;
    //     assert_eq!(registry.claim_of(&id), Some(vec![(0, 1)]));
    //     // The same participant and its descendants use x again.
    //     for path in [vec![(0, 1)], vec![(0, 1), (2, 3)]] {
    //         job(&registry, seq, &path, |claims| {
    //             used(claims).expect("a descendant")
    //         })
    //         .await;
    //     }
    //     job(&registry, seq, &[(0, 1), (2, 3)], |claims| {
    //         used(claims).expect("a child").store()
    //     })
    //     .await;
    //     assert_eq!(registry.claim_of(&id), Some(vec![(0, 1), (2, 3)]));
    //     // A sibling, the parent and an unrelated participant are concurrent.
    //     for path in [vec![(0, 1), (2, 4)], vec![(0, 1)], vec![(0, 2)], vec![]] {
    //         let refused = job(&registry, seq, &path, |claims| used(claims).err()).await;
    //         assert_eq!(
    //             refused,
    //             Some(SpaceError::SeqConcurrent { space: seq.sid }),
    //             "{path:?}"
    //         );
    //     }
    //     // Another channel of the space has its own claim (decision 73).
    //     job(&registry, seq, &[(0, 2)], |mut claims| {
    //         claims
    //             .claim(&data(seq, 2), ClaimRole::Data)
    //             .expect("another channel");
    //         claims.store();
    //     })
    //     .await;
    //     registry.clear();
    //     assert_eq!(registry.claim_of(&id), None, "a clear forgets the claims");
    // }

    /// Added by decision 91: runs `body` with the claims of a program job on `space`, in a task
    /// whose causal history is `history`, for the own item with the random state `item`.
    async fn job<T>(
        registry: &Arc<VolatileRegistry>,
        space: NativeSpace,
        history: SeqHistory,
        item: &[u8],
        body: impl FnOnce(SeqClaims) -> T,
    ) -> T {
        seq_history::with_history(history, async {
            let claims = SeqClaims::open(registry, space, SpaceQualifier::Seq, false, item)
                .expect("a seq job opens its claims")
                .expect("a program job on a seq space claims");
            body(claims)
        })
        .await
    }

    fn data(space: NativeSpace, byte: u8) -> Par {
        gprivate(namespace::data_channel(space, &key(&[byte])))
    }

    /// The history that knows the uses `uses`.
    fn knowing(uses: &[(&[u8], u64)]) -> SeqHistory {
        let uses: Vec<(Vec<u8>, u64)> = uses.iter().map(|(id, n)| (id.to_vec(), *n)).collect();
        SeqHistory::default().with_uses(&uses)
    }

    /// Added by decision 91: a non-seq space and an internal job claim nothing, and a program
    /// job on a seq space opens its claims with no operation order (the code before decision
    /// 91 failed it with `Inconsistent`, `ClaimsNeedContextUnsafe` of `SpaceCoordination.tla`).
    #[tokio::test]
    async fn seq_claims_open_for_program_jobs_on_seq_spaces_without_an_order() {
        let registry = Arc::new(VolatileRegistry::default());
        let seq = space(4, Durability::Ephemeral);
        for (qualifier, internal) in [
            (SpaceQualifier::Default, false),
            (SpaceQualifier::Temp, false),
            (SpaceQualifier::Seq, true),
        ] {
            let claims = SeqClaims::open(&registry, seq, qualifier, internal, b"item");
            assert!(matches!(claims, Ok(None)), "{qualifier:?} {internal}");
        }
        assert!(operation_context_is_unset());
        let opened = SeqClaims::open(&registry, seq, SpaceQualifier::Seq, false, b"item")
            .expect("a program job opens its claims without an operation order")
            .expect("a program job on a seq space claims");
        assert!(opened.history.is_empty(), "outside a history scope");
        let history = knowing(&[(b"x", 3)]);
        let inside = job(&registry, seq, history.clone(), b"item", |claims| {
            claims.history
        })
        .await;
        assert_eq!(inside, history, "the claims take the history of the task");
    }

    fn operation_context_is_unset() -> bool {
        rspace_plus_plus::rspace::operation_context::current().is_none()
    }

    /// Added by decision 91: `Admissible` and `CommitUse` of `SpaceCoordination.tla`. A use of a
    /// channel is admitted when the channel has no claim or its last use is in the history of
    /// the job, and a stored claim gives the channel its next use.
    #[tokio::test]
    async fn seq_claims_admit_exactly_the_jobs_that_know_the_last_use() {
        let registry = Arc::new(VolatileRegistry::default());
        let seq = space(4, Durability::Ephemeral);
        let x = data(seq, 1);
        let id = sole_gprivate_id(&x).expect("an id").to_vec();
        let used = |mut claims: SeqClaims| claims.claim(&x, ClaimRole::Data).map(|_| claims);
        // A job that fails or drops its transaction stores no claim (SQ-4).
        job(&registry, seq, SeqHistory::default(), b"a", |claims| {
            used(claims).expect("a first use")
        })
        .await;
        assert_eq!(
            registry.claim_of(&id),
            None,
            "a pending claim is not stored"
        );
        // A job stores its claims after its commit succeeded: use 1.
        job(&registry, seq, SeqHistory::default(), b"a", |claims| {
            // Changed by decision 124 (plan v4 C7.19, rule CR-24, ruling M-4): a claim belongs to
            // its row. The text before:
            // used(claims).expect("a first use").store()
            let mut claims = used(claims).expect("a first use");
            claims.close_row(0);
            claims.store(1, None)
        })
        .await;
        assert_eq!(registry.claim_of(&id), Some(1));
        // A job that knows use 1 uses x again: use 2.
        job(&registry, seq, knowing(&[(&id, 1)]), b"b", |claims| {
            // Added by decision 124 (plan v4 C7.19, rule CR-24, ruling M-4): a claim belongs to
            // its row.
            let mut claims = used(claims).expect("a use after use 1");
            claims.close_row(0);
            claims.store(1, None)
        })
        .await;
        assert_eq!(registry.claim_of(&id), Some(2));
        // A job that knows only use 1, a job with no history, and a job that knows another
        // channel are concurrent with use 2.
        for history in [
            knowing(&[(&id, 1)]),
            SeqHistory::default(),
            knowing(&[(b"y", 9)]),
        ] {
            let refused = job(&registry, seq, history.clone(), b"c", |claims| {
                used(claims).err()
            })
            .await;
            assert_eq!(
                refused,
                Some(SpaceError::SeqConcurrent { space: seq.sid }),
                "{history:?}"
            );
        }
        // A job that knows a later use of x knows use 2 too.
        job(&registry, seq, knowing(&[(&id, 5)]), b"d", |claims| {
            used(claims).expect("a later use knows use 2")
        })
        .await;
        // Another channel of the space has its own claim (decision 73).
        job(&registry, seq, SeqHistory::default(), b"e", |mut claims| {
            claims
                .claim(&data(seq, 2), ClaimRole::Data)
                .expect("another channel");
            // Changed by decision 124 (plan v4 C7.19, rule CR-24, ruling M-4): a claim belongs to
            // its row. The text before:
            // claims.store();
            claims.close_row(0);
            claims.store(1, None);
        })
        .await;
        registry.clear();
        assert_eq!(registry.claim_of(&id), None, "a clear forgets the claims");
    }

    /// Added by decision 102 (plan v4 C4.3, rule CR-14): R-A rewinds the pending claims of a job
    /// to the snapshot after the claim of its own collection. The claim after the snapshot is
    /// dropped and never stored. A claim after the rewind is stored with the claims before the
    /// snapshot.
    #[tokio::test]
    async fn seq_claims_rewind_drops_the_claims_after_the_snapshot() {
        let registry = Arc::new(VolatileRegistry::default());
        let seq = space(4, Durability::Ephemeral);
        let (own, picked, restaged) = (data(seq, 1), data(seq, 2), data(seq, 3));
        job(
            &registry,
            seq,
            SeqHistory::default(),
            b"item",
            |mut claims| {
                claims.claim(&own, ClaimRole::Data).expect("the own claim");
                // Added by decision 124 (plan v4 C7.19, rule CR-24, ruling M-4): the own claim
                // belongs to row 0.
                claims.close_row(0);
                let snapshot = claims.snapshot();
                claims
                    .claim(&picked, ClaimRole::Data)
                    .expect("the claim of the pick");
                claims.rewind(&snapshot);
                claims
                    .claim(&restaged, ClaimRole::Data)
                    .expect("the claim of the restage");
                // Added by decision 124 (plan v4 C7.19, rule CR-24, ruling M-4): the restage is
                // row 1, so the commit stores rows 0 and 1.
                claims.close_row(1);
                claims.store(2, None);
            },
        )
        .await;
        for (channel, stored) in [(&own, Some(1)), (&picked, None), (&restaged, Some(1))] {
            let id = sole_gprivate_id(channel).expect("an id");
            assert_eq!(registry.claim_of(id), stored, "{channel:?}");
        }
    }

    /// Added by decision 124 (plan v4 C7.19, rules CR-24 and CR-6): `store(rows)` stores the
    /// claims of the staged rows below `rows` only. A claim that is still open, after the last
    /// staged row, is not stored.
    #[tokio::test]
    async fn seq_claims_store_only_the_committed_rows() {
        let registry = Arc::new(VolatileRegistry::default());
        let seq = space(4, Durability::Ephemeral);
        let rows = [data(seq, 1), data(seq, 2), data(seq, 3)];
        let open = data(seq, 4);
        job(
            &registry,
            seq,
            SeqHistory::default(),
            b"item",
            |mut claims| {
                for (row, channel) in rows.iter().enumerate() {
                    claims
                        .claim(channel, ClaimRole::Data)
                        .expect("the claim of a row");
                    claims.close_row(row);
                }
                claims.claim(&open, ClaimRole::Data).expect("an open claim");
                claims.store(2, None);
            },
        )
        .await;
        for (channel, stored) in [
            (&rows[0], Some(1)),
            (&rows[1], Some(1)),
            (&rows[2], None),
            (&open, None),
        ] {
            let id = sole_gprivate_id(channel).expect("an id");
            assert_eq!(registry.claim_of(id), stored, "{channel:?}");
        }
    }

    /// Added by decision 124 (plan v4 C7.19, rule CR-24, ruling RADD 1.8): for a job and a
    /// channel that another job claimed, the read handle answers as the staging claim decides
    /// (decision 123). A job that knows the last use is admitted, and a job that does not know it
    /// is refused. A path-index channel in the role `Data` claims nothing, so no job is refused.
    #[tokio::test]
    async fn seq_reads_admits_as_the_staging_claims() {
        let registry = Arc::new(VolatileRegistry::default());
        let seq = space(4, Durability::Ephemeral);
        let claimed = data(seq, 1);
        let index = gprivate(namespace::path_index_channel(seq, &[3]));
        let claimed_id = sole_gprivate_id(&claimed).expect("an id").to_vec();
        let index_id = sole_gprivate_id(&index).expect("an id").to_vec();
        // Another job claims the data channel and the path-index channel: use 1 of each.
        job(
            &registry,
            seq,
            SeqHistory::default(),
            b"other",
            |mut claims| {
                claims
                    .claim(&claimed, ClaimRole::Data)
                    .expect("a first use");
                claims
                    .claim(&index, ClaimRole::Waiter)
                    .expect("a first use");
                claims.close_row(0);
                claims.store(1, None);
            },
        )
        .await;
        let knows = knowing(&[(&claimed_id, 1), (&index_id, 1)]);
        let unaware = SeqHistory::default();
        for (channel, role, history, admitted) in [
            // The job knows the last use of the channel, so its claim is admissible.
            (&claimed, ClaimRole::Data, &knows, true),
            (&claimed, ClaimRole::Waiter, &knows, true),
            (&index, ClaimRole::Waiter, &knows, true),
            // The job does not know the last use of the channel, so its claim is concurrent.
            (&claimed, ClaimRole::Data, &unaware, false),
            (&claimed, ClaimRole::Waiter, &unaware, false),
            (&index, ClaimRole::Waiter, &unaware, false),
            // A path-index channel in the role `Data` claims nothing, whatever the job knows.
            (&index, ClaimRole::Data, &unaware, true),
        ] {
            let (admits, decided) = job(&registry, seq, history.clone(), b"job", |mut claims| {
                let admits = claims.reads().admits(channel, role);
                (admits, claims.claim(channel, role))
            })
            .await;
            let expected = match admitted {
                false => Err(SpaceError::SeqConcurrent { space: seq.sid }),
                true => Ok(()),
            };
            assert_eq!(decided, expected, "{channel:?} {role:?}");
            assert_eq!(admits, admitted, "{channel:?} {role:?}");
        }
    }

    /// Added by decision 124 (plan v4 C7.19, rule CR-24, ruling RADD 1.8): with the claims of the
    /// actual unit of row d, `store` stores them in place of the staged claims of row d. The
    /// claims of the rows before d are stored, and no claim of a staged row after d is stored.
    /// Each actual channel goes through the one claim rule, so a path-index channel in the role
    /// `Data` claims nothing.
    #[tokio::test]
    async fn store_keeps_the_claims_of_the_actual_unit() {
        let registry = Arc::new(VolatileRegistry::default());
        let seq = space(4, Durability::Ephemeral);
        // Row 0, row d (row 1) and a staged row after d.
        let rows = [data(seq, 1), data(seq, 2), data(seq, 3)];
        let actual = data(seq, 4);
        let waiter = gprivate(namespace::path_index_channel(seq, &[5]));
        let unclaimable = gprivate(namespace::path_index_channel(seq, &[6]));
        job(
            &registry,
            seq,
            SeqHistory::default(),
            b"item",
            |mut claims| {
                for (row, channel) in rows.iter().enumerate() {
                    claims
                        .claim(channel, ClaimRole::Data)
                        .expect("the claim of a staged row");
                    claims.close_row(row);
                }
                // The finish passes the index of row d as the count of the rows before it, and
                // the claims of its actual unit.
                claims.store(
                    1,
                    Some(RowClaims {
                        data: vec![actual.clone(), unclaimable.clone()],
                        waiters: vec![waiter.clone()],
                    }),
                );
            },
        )
        .await;
        for (channel, stored) in [
            (&rows[0], Some(1)),
            (&rows[1], None),
            (&rows[2], None),
            (&actual, Some(1)),
            (&waiter, Some(1)),
            (&unclaimable, None),
        ] {
            let id = sole_gprivate_id(channel).expect("an id");
            assert_eq!(registry.claim_of(id), stored, "{channel:?}");
        }
    }

    /// Added by decision 91: a stored use joins the history of the task of the job and the
    /// history of its own item, so the reply and every body that the item fires know it
    /// (`CommitUse` and `HistIsCausalPast` of `SpaceCoordination.tla`).
    #[tokio::test]
    async fn seq_claims_store_teaches_the_task_and_the_item() {
        let registry = Arc::new(VolatileRegistry::default());
        let seq = space(4, Durability::Ephemeral);
        let x = data(seq, 1);
        let id = sole_gprivate_id(&x).expect("an id").to_vec();
        let before = knowing(&[(b"y", 2)]);
        let after = seq_history::with_history(before.clone(), async {
            let mut claims = SeqClaims::open(&registry, seq, SpaceQualifier::Seq, false, b"item")
                .expect("open")
                .expect("claims");
            claims.claim(&x, ClaimRole::Data).expect("a first use");
            // Changed by decision 124 (plan v4 C7.19, rule CR-24, ruling M-4): a claim belongs to
            // its row. The text before:
            // claims.store();
            claims.close_row(0);
            claims.store(1, None);
            seq_history::current_history()
        })
        .await;
        assert!(after.knows(&id, 1), "the task learns use 1");
        assert!(after.knows(b"y", 2), "the task keeps its history");
        assert_eq!(
            registry.item_history(b"item"),
            after,
            "the item learns use 1"
        );
        assert!(registry.has_item_histories());
        assert_eq!(registry.item_history(b"other"), SeqHistory::default());
    }

    /// Added by decision 91: the union of item histories (`Comm` of `SpaceCoordination.tla`)
    /// takes the larger use of each channel, and an empty book unions nothing.
    #[test]
    fn volatile_registry_unions_item_histories() {
        let registry = VolatileRegistry::default();
        assert_eq!(
            registry.union_of_items([b"a".as_slice()]),
            SeqHistory::default()
        );
        registry.record_item(b"a", knowing(&[(b"x", 1), (b"y", 4)]));
        registry.record_item(b"b", knowing(&[(b"x", 3)]));
        registry.record_item(b"c", SeqHistory::default());
        assert!(registry.has_item_histories());
        assert_eq!(
            registry.union_of_items([b"a".as_slice(), b"b".as_slice(), b"missing".as_slice()]),
            knowing(&[(b"x", 3), (b"y", 4)])
        );
        assert_eq!(
            registry.item_history(b"c"),
            SeqHistory::default(),
            "an empty history is not recorded"
        );
        registry.clear();
        assert!(
            !registry.has_item_histories(),
            "a clear forgets the item histories"
        );
    }

    #[tokio::test]
    async fn seq_claims_claim_only_user_channels_of_their_space() {
        let registry = Arc::new(VolatileRegistry::default());
        let seq = space(4, Durability::Ephemeral);
        let other = space(5, Durability::Ephemeral);
        let durable = space(4, Durability::Durable);
        let index = gprivate(namespace::path_index_channel(seq, &[3]));
        // Changed by decision 91: the job runs with a causal history instead of a path.
        // let pending = job(&registry, seq, &[(0, 1)], |mut claims| {
        let pending = job(
            &registry,
            seq,
            SeqHistory::default(),
            b"item",
            |mut claims| {
                for (channel, role) in [
                    (gprivate(namespace::config_channel(seq)), ClaimRole::Waiter),
                    (gprivate(namespace::space_counter(seq)), ClaimRole::Waiter),
                    (
                        gprivate(namespace::collection_counter(seq, &key(&[1]))),
                        ClaimRole::Data,
                    ),
                    (gprivate(namespace::allocator_channel(seq)), ClaimRole::Data),
                    (gprivate(namespace::handle_channel(seq)), ClaimRole::Waiter),
                    (
                        gprivate(namespace::name_id(seq, NameTail::Random([1; 32]))),
                        ClaimRole::Data,
                    ),
                    (index.clone(), ClaimRole::Data),
                    (data(other, 1), ClaimRole::Data),
                    (data(durable, 1), ClaimRole::Data),
                    (Par::default(), ClaimRole::Waiter),
                ] {
                    claims.claim(&channel, role).expect("nothing to claim");
                }
                // Added by decision 124 (plan v4 C7.19, rule CR-24, ruling M-4): the claims of
                // the job close at row 0, so `pending` holds them.
                claims.close_row(0);
                assert!(claims.pending.is_empty(), "{:?}", claims.pending);
                claims
                    .claim(&index, ClaimRole::Waiter)
                    .expect("a waiter on a prefix");
                claims
                    .claim(&data(seq, 1), ClaimRole::Data)
                    .expect("a data channel");
                // Added by decision 124 (plan v4 C7.19, rule CR-24, ruling M-4): the claims of
                // the job close at row 0, so `pending` holds them.
                claims.close_row(0);
                claims.pending.len()
            },
        )
        .await;
        assert_eq!(pending, 2);
    }

    #[test]
    fn volatile_registry_records_spaces_until_it_is_cleared() {
        let registry = VolatileRegistry::default();
        let temp = space(2, Durability::Ephemeral);
        assert!(!registry.is_live(&temp.sid));
        registry.record_space(temp, SpaceQualifier::Temp);
        registry.record_locked(&[gprivate(namespace::config_channel(temp))]);
        assert!(registry.is_live(&temp.sid));
        assert_eq!(registry.qualifier(&temp.sid), Some(SpaceQualifier::Temp));
        assert_eq!(registry.take_channels().len(), 1);
        assert!(
            registry.is_live(&temp.sid),
            "taking the channels keeps the spaces"
        );
        registry.record_locked(&[gprivate(namespace::config_channel(temp))]);
        registry.clear();
        assert!(!registry.is_live(&temp.sid));
        assert!(!registry.has_channels());
    }

    /// Added by decision 124 (L14, plan v4 C7.8): the book of a root evaluation clears with the
    /// evaluation scope, whose `clear` empties the registry. A booked pair does not outlive it.
    #[test]
    fn book_clears_with_the_evaluation_scope() {
        let registry = VolatileRegistry::default();
        let record = Blake2b256Hash::new(b"record");
        let continuation = Blake2b256Hash::new(b"continuation");
        registry.book_pairs([(record.clone(), continuation.clone())]);
        assert_eq!(
            registry.book().get(&record),
            Some(&BTreeSet::from([continuation]))
        );
        registry.clear();
        assert!(registry.book().is_empty(), "a clear empties the book");
    }
}
