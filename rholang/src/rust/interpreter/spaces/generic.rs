//! The generic native space (P4 design B.4, P5 design v3 A.3): an outer store and a discipline
//! over the substrate.
//!
//! [`GenericSpace<S, D>`](GenericSpace) runs the collection operations of the family whose outer
//! store is `S` and whose inner collection is `D`. The
//! [`FamilyAgent`](super::families::FamilyAgent) reads the config of a space once and sends each
//! operation to the `GenericSpace` of the family that the config names (P5 design v3, A.3.4).
//!
//! A collection operation runs in one transaction:
// Changed by decision 94: the operation takes its locks in two phases.
// //! 1. `begin` with the lock keys `[C(sid), S(sid), S(sid,k), D(sid,k)]`: every channel that the
// //!    operation can touch (readiness review S2).
//! 1. `begin_phase_a` with the lock keys `[C(sid), S(sid), S(sid,k), D(sid,k)]`: every channel
//!    of its collection that the operation can touch (readiness review S2).
//! 2. Read the config record on `C(sid)`. Without it the operation fails with `DanglingName` for
//!    a space name and with `DanglingHandle` for every other channel.
// Changed by decision 94: the classification runs under phase A, and phase B covers the scope.
// //! 3. Check the family of the config (`FamilyUnavailable`). Then classify the operation
// //!    ([`FamilySpace::classify`]): a collection whose data channel holds no join group other
// //!    than `[D(sid,k)]` runs the steps below. Every other operation, a join and a settle with a
// //!    cascade run in the join engine of P5c (`spaces::join`), which escalates the locks to the
// //!    join closure first ([`prepare_collection_op`]).
//! 3. Check the family of the config (`FamilyUnavailable`). Then classify the operation under
//!    phase A ([`FamilySpace::classify`]): a collection whose data channel holds no join group
//!    other than `[D(sid,k)]` takes phase B over its own keys and runs the steps below. Every
//!    other operation, a join and a settle with a cascade take phase B over the own keys and the
//!    keys of the join closure, and then run in the join engine of P5c (`spaces::join`,
//!    [`prepare_collection_op`]).
//! 4. Before any staging, check the routed key with the outer store (`OuterStore::check_key`),
//!    and the payload of a send or the pattern of a receive (`OuterStore::check_payload`,
//!    `OuterStore::check_patterns`: the unit rules of a HashSet store, P7 HS-2 and HS-3).
//!    Then check the named params and the query with the schema of `D`: the keys
//!    (`UnknownParam`), a query that `D` does not take (`QuerySpecUnsupported`), and the values
//!    (`Discipline::check_values`).
//! 5. Bump the collection counter when `D` uses one (`counter::bump`). The value before the bump
//!    is the `ord` of the operation. A re-issued persistent operation keeps the `ord` of its
//!    first round instead (`operation_ord`, decision 43). Without a counter, every `ord` is 0.
//! 6. Plan and stage the operation: `D::offer` for a produce, `D::receive` for a consume, and
//!    `D::settle_job` for a settle. The default plans are the head driver of this module
//!    ([`offer_head`], [`receive_head`]), which the queue uses (decision 11).
//! 7. The job commits the transaction exactly once (`Prepared::commit`). A produce and a consume
//!    stage at least a store, a park or a fire, so their commits are never empty (readiness
//!    review S3). Decision 92: the first staged operation is the job's own, or the transaction
//!    stages an anchor first (`spaces::anchor`). A transaction that stages nothing commits an
//!    anchor only. A failure at a position of the state (`SpaceError::is_positional`) removes
//!    the staged operations, commits an anchor only, and then fails.
//! 8. The receipts become the `SpaceOutcome`: the fired continuations without their envelopes,
//!    in staging order, and the re-issue. A counter fire, a retirement and a lift are never
//!    dispatched.
//!
//! The produce plan of the head driver is the `OFFER` of the substrate design (decision 30):
//!
//! ```text
//! if the datum does not reach the head of the records: stage Store, stop    ▷ its FIFO place
//! loop:
//!     W ← the head waiter, or stage Store and stop
//!     if W does not match the payload, or its guard refuses: stage Store, stop   ▷ W blocks
//!     stage Fire(datum, W)
//!     after_fire(datum spent, W stays):
//!         Done: stop
//!         Continue: offer the datum to the next head waiter (the re-offer)
//!         Reissue(cause): re-issue the produce as a new Intent::Space, stop
//! ```
//!
//! The datum is spent when it is not persistent and `W` does not peek, and `W` stays when it is
//! persistent. The consume plan is symmetric: the continuation waits behind earlier waiters, then
//! meets the head records one by one. It is spent when it is not persistent, and the head stays
//! when it is persistent or the consume peeks. A peek fire removes a non-persistent head record,
//! as the host COMM does. The discipline then stores the same record again right after the fire
//! (`Discipline::restore`), as the host reducer's `produce_peeks` does (decision 45, Rule E). The
//! re-store registers the record's own authority and bumps no counter. A divergent pair
//! re-issues after one round (A10), and a chain re-issues after `FIRE_ROUND_CAP` rounds (I-21).
//! Each round makes at most two matcher calls, and the bump makes one more. A store or a
//! park on a side that holds `D::CAPACITY` entries fails with `CellOccupied` before it stages
//! anything (F15).
//!
//! A re-issued persistent operation keeps the `ord` of its first round (decision 43), so its
//! record, its identity and its place stay the same in every round. Each round still bumps the
//! counter. Other operations can run between two rounds, so the element re-enters at its place
//! ([`Cx::at_head`]): at the head when its side is empty or its `ord` precedes the head of its
//! side (`Discipline::precedes`), and otherwise stored or parked behind the head, with no fire.
//! A fresh operation and a re-issued datum that is not persistent take the `ord` of their bump,
//! so they reach the head only of an empty side. While a persistent element is in flight, the
//! heads can match only when the element precedes the head of its side, and every completed
//! deploy ends with no element in flight (`HeadOnlyDisciplines.v`, `HeadOnlyDiscipline.tla`).
//!
//! The plans read the staged view, so play and replay stage the same operations: the heads come
//! from the `ord` of the envelopes, never from the list order of the store. A plan reaches the
//! transaction only through [`Cx`]. Each staging primitive of `Cx` stages its operation and
//! registers an introduction (P4 design B.7). Changed by decision 90 (decision 69): the legacy
//! chain of decision 45 (Option P) is gone. A re-offer and a re-fire repeat an identity with
//! its first authority (readiness review S6).
//!
//! An operation copies only the head that it meets, and a fire that ends the loop moves its
//! values instead of copying them. The outcome after the commit only moves values out of its
//! receipt.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum, WaitingContinuation};
// Added by decision 124 (plan v4 C7.9): a row search matches in the store of the commit.
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
// Changed by decision 102 (plan v4 C4.1): the pick helpers read the mode and shuffle with the
// play order of the base.
// use rspace_plus_plus::rspace::space_store_handle::{
//     canonical_continuation_order, canonical_data_order, incoming_candidate, ConsumeOutcome,
//     ProduceOutcome, SpaceConsume, SpaceOpReceipt, SpaceProduce,
// };
// Changed by decision 124 (plan v4 C7.9): the list gains the row search types.
use rspace_plus_plus::rspace::space_store_handle::{
    canonical_continuation_order, canonical_data_order, canonical_data_order_with,
    incoming_candidate, play_order, ActualFire, ConsumeOutcome, FinishRow, ProduceOutcome,
    RowItemRule, RowItemTest, RowLists, RowReissue, RowRun, RowSearch, RowTupleRequest,
    SpaceConsume, SpaceMode, SpaceOpReceipt, SpaceProduce, SpaceTxnError, TailRead,
};
// Changed by decision 102 (plan v4 C4.1): the replay order reads the recorded COMM.
// use rspace_plus_plus::rspace::trace::event::Produce;
// Changed by decision 102 (plan v4 C5.1, C5.6): a receive names the consume that it stages.
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};
use serde::Serialize;

use super::agent::{
    AllocOp, Allocation, ConsumeOp, JoinOp, OpOrigin, ParSpaceReceipt, ParSpaceTxn, Prepared,
    ProduceOp, QueryParams, SettleOp, SpaceAgent, SpaceCreation, SpaceEnv, SpaceFire, SpaceOp,
    SpaceOutcome, SpaceReissue,
};
use super::anchor::{self, SpaceJob};
use super::counter::{self, BumpContext, CounterChannels, CounterOp};
// Added by decision 128 (kind routing): the types of the windows whose steps a discipline gives.
use super::disciplines::lanes::{Lane, Level};
use super::disciplines::similarity::{
    Eligible, EligibleOrder, Flight, ListFire, SimilarityConfig, SimilarityQuery,
};
// Changed by decision 102 (plan v4 C4.3): R-A rewinds the claims of a job.
// use super::ephemeral::{ClaimRole, SeqClaims};
// Changed by decision 124 (L14, plan v4 C7.8): the plan holds a snapshot of the book.
use super::ephemeral::{BookPairs, ClaimRole, SeqClaims, SeqReads, SeqSnapshot};
use super::errors::SpaceError;
use super::factory::defaults::suggestion;
use super::join::{self, JoinRoute, JoinScope};
use super::matching::SpaceMatcher;
use super::mirror::MirrorCapture;
use super::namespace::{self, CollectionKey, NativeSpace, RandomPurpose, ReifiedId};
use super::record::{self, Introduction, OperationAuthority};
// Changed by decision 124 (plan v2 C7.10, plan v4 C7.11, rulings RADD 1.3, RADD2 1.3 and
// RADD2v2 6.1): gen reads `machinery_shape` too, and `settle_operation`, `restored_send` and
// `reissued_without_order` for the re-issue of Tail. The text before:
// use super::settle::reissued_operation;
use super::settle::{
    machinery_shape, reissued_operation, reissued_without_order, restored_send, settle_operation,
};
use super::theory::{self, SpaceTheory, TheoryCommitLease};
use super::types::config::SpaceConfig;
use crate::rust::interpreter::reified::costs::{
    FireRounds, FireStep, ReissueCause, FIRE_ROUND_CAP,
};
// Added by decision 128 (kind routing).
use crate::rust::interpreter::tensor::exact::Score;
use crate::rust::interpreter::tensor::num::ExactVector;
// Disabled by decision 90 (decision 69): Option P is gone.
// use crate::rust::interpreter::reified::legacy_charges::LegacyChain;

/// A record of a collection in the staged view.
pub type ParDatum = Datum<ListParWithRandom>;

/// A waiting continuation of a collection in the staged view.
pub type ParWaiter = WaitingContinuation<BindPattern, TaggedContinuation>;

/// A candidate of a staged fire.
pub type ParCandidate = ConsumeCandidate<Par, ListParWithRandom>;

/// Added by decision 124 (plan v4 C7.9, rules T3 and L3): a row search over the reads of the
/// commit.
pub type ParRowSearch = Box<dyn RowSearch<Par, BindPattern, ListParWithRandom, TaggedContinuation>>;
pub(crate) type ParRowLists<'r> =
    RowLists<'r, Par, BindPattern, ListParWithRandom, TaggedContinuation>;
pub(crate) type ParRowRequest<'r> =
    RowTupleRequest<'r, Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): a finish row of a cut
/// row over the reads of the commit (P1).
pub type ParFinishRow = FinishRow<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the produce that stores
/// `record` on `channel` with the introduction `introduction`, as `Cx::store_on` stages it and as
/// a finish row of a cut row commits it (P1). `own` is true when the record is the job's own.
pub(crate) fn store_request(
    channel: &Par,
    record: ListParWithRandom,
    persist: bool,
    introduction: &Arc<Introduction>,
    own: bool,
) -> SpaceProduce<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    SpaceProduce {
        channel: channel.clone(),
        data: record,
        persist,
        outcome: ProduceOutcome::Store,
        before_observe: Some(record::register_introduction(introduction)),
        own,
        search: None,
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.6): the end of a row that
/// its discipline does not end, a machinery row. `StagedRowSearch` answers the tail
/// methods of `RowSearch` with it when it holds no tail of its discipline. `()` gives the base
/// answers: `Ends`, no re-issue and no finish rows. A join kind gives the end of its cascade
/// lift (`JoinSource::lift_search`).
pub trait RowEnd: Send {
    /// The tail read of the row at the commit, as `RowSearch::tail_read`.
    fn tail_read(
        &mut self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<TailRead, SpaceTxnError>;

    /// The re-issue of the last tail read, taken out, as `RowSearch::tail_reissue`.
    fn tail_reissue(&mut self) -> Option<RowReissue>;

    /// The finish rows of the last tail read, taken out, as `RowSearch::finish_rows`.
    fn finish_rows(&mut self) -> Vec<ParFinishRow>;
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.6): the end of a row that
/// owes nothing after its fire: a retire row, a head row and the base.
impl RowEnd for () {
    fn tail_read(
        &mut self,
        _lists: &ParRowLists<'_>,
        _matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<TailRead, SpaceTxnError> {
        Ok(TailRead::Ends)
    }

    fn tail_reissue(&mut self) -> Option<RowReissue> { None }

    fn finish_rows(&mut self) -> Vec<ParFinishRow> { Vec::new() }
}

/// Added by decision 124 (plan v4 C7.9 and C7.11, section 2.4, rules T3 and L3, ruling RADD 1.4):
/// the row search of a head row and of a machinery row (V4-5). It lists the staged tuple only,
/// so the commit finds no alternative, and a refused machinery row parks inert (CR-16). It keeps
/// the discipline `D` of the collection, whose tail read follows a deviating head row.
pub struct StagedRowSearch<D: Discipline, E = ()> {
    channels: Vec<Par>,
    tuple: Vec<ParCandidate>,
    /// The checker of the theory of the space, or `None` without a theory or for a machinery row.
    theory: Option<Arc<dyn SpaceTheory>>,
    stored: Option<Consume>,
    pairs: Option<(TaggedContinuation, bool, Blake2b256Hash)>,
    /// The discipline of the collection of the row (RADD 1.4).
    discipline: D,
    /// The fire count before the row and the snapshot of the book that the staging read, for the
    /// tail read of a head row (errata V4-13). `None` for a machinery row, which reads `end`.
    tail: Option<(usize, Arc<BookPairs>)>,
    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.6): the end of the row
    /// when the discipline gives no tail (`tail` is `None`).
    end: E,
}

impl<D: Discipline> StagedRowSearch<D> {
    /// The search of a head offer: the datum in flight of `candidate` on `channel` fires the
    /// stored waiter `waiter`, in the staged view of `cx`, after `fires_before` fires of the
    /// transaction.
    pub fn of_offer(
        cx: &Cx<'_, '_>,
        waiter: &ParWaiter,
        channel: &Par,
        candidate: ParCandidate,
        fires_before: usize,
    ) -> Self {
        let mut channels = Vec::<Par>::with_capacity(1);
        channels.push(channel.clone());
        let mut tuple = Vec::<ParCandidate>::with_capacity(1);
        tuple.push(candidate);
        Self {
            channels,
            tuple,
            theory: cx.theory(),
            stored: Some(waiter.source.clone()),
            pairs: Some((
                waiter.continuation.clone(),
                waiter.persist,
                waiter.source.hash.clone(),
            )),
            discipline: D::default(),
            tail: Some((fires_before, Arc::clone(&cx.plan.book))),
            end: (),
        }
    }

    /// The search of a head receive: the consume `source` of `continuation` binds the stored
    /// record of `candidate` on `channel`, in the staged view of `cx`, after `fires_before` fires
    /// of the transaction.
    pub fn of_receive(
        cx: &Cx<'_, '_>,
        source: &Consume,
        continuation: &TaggedContinuation,
        persistent: bool,
        channel: &Par,
        candidate: ParCandidate,
        fires_before: usize,
    ) -> Self {
        let mut channels = Vec::<Par>::with_capacity(1);
        channels.push(channel.clone());
        let mut tuple = Vec::<ParCandidate>::with_capacity(1);
        tuple.push(candidate);
        Self {
            channels,
            tuple,
            theory: cx.theory(),
            stored: None,
            pairs: Some((continuation.clone(), persistent, source.hash.clone())),
            discipline: D::default(),
            tail: Some((fires_before, Arc::clone(&cx.plan.book))),
            end: (),
        }
    }

    /// The search of a machinery row: the staged tuple `candidates` over `channels`. It books
    /// no item pairs, and its tail read answers `Ends`.
    pub fn of_machinery(channels: &[Par], candidates: &[ParCandidate]) -> Self {
        Self {
            channels: channels.to_vec(),
            tuple: candidates.to_vec(),
            theory: None,
            stored: None,
            pairs: None,
            discipline: D::default(),
            tail: None,
            end: (),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.6): this search with
    /// the end `end` in place of `()`. The search tests the row as before.
    pub(crate) fn with_end<E: RowEnd>(self, end: E) -> StagedRowSearch<D, E> {
        let Self {
            channels,
            tuple,
            theory,
            stored,
            pairs,
            discipline,
            tail,
            end: (),
        } = self;
        StagedRowSearch {
            channels,
            tuple,
            theory,
            stored,
            pairs,
            discipline,
            tail,
            end,
        }
    }
}

impl<D: Discipline, E: RowEnd> RowSearch<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    for StagedRowSearch<D, E>
{
    fn runs(&self) -> Vec<RowRun<Par>> {
        vec![RowRun {
            channels: self.channels.clone(),
            candidates: 0..1,
        }]
    }

    fn continuations(
        &mut self,
        _run: usize,
        lists: &ParRowLists<'_>,
    ) -> Result<Vec<(Arc<ParWaiter>, i32)>, SpaceTxnError> {
        let Some(consume) = &self.stored else {
            return Ok(Vec::new());
        };
        let mut waiters = lists.continuations(&self.channels)?;
        let Some(index) = waiters.iter().position(|waiter| &waiter.source == consume) else {
            return Ok(Vec::new());
        };
        let position = i32::try_from(index)
            .map_err(|_| SpaceTxnError::Inconsistent("a waiter index overflows i32"))?;
        Ok(vec![(waiters.swap_remove(index), position)])
    }

    fn tuple(
        &mut self,
        _run: usize,
        _lists: &ParRowLists<'_>,
        request: ParRowRequest<'_>,
    ) -> Result<Option<Vec<ParCandidate>>, SpaceTxnError> {
        if let Some(item) = &request.item {
            for candidate in &self.tuple {
                match item.admits(&candidate.datum.a, candidate.datum.persist)? {
                    true => {}
                    false => return Ok(None),
                }
            }
        }
        Ok(Some(self.tuple.clone()))
    }

    fn restore(&mut self) {}

    fn commit_check(
        &mut self,
        _request: &ParRowRequest<'_>,
        _tuple: &[ParCandidate],
    ) -> Result<bool, SpaceTxnError> {
        Ok(true)
    }

    fn may_veto(
        &self,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        continuation: &TaggedContinuation,
    ) -> bool {
        super::matching::may_veto(matcher, continuation, self.theory.is_some())
    }

    fn staged_retires(&self) -> usize { 0 }

    fn actual_fire(
        &mut self,
        _request: &ParRowRequest<'_>,
        _tuple: &[ParCandidate],
    ) -> ActualFire<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
        ActualFire {
            retires: Vec::new(),
            dispatched: None,
        }
    }

    fn item_failures(
        &self,
        _lists: &ParRowLists<'_>,
        _matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
    ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceTxnError> {
        let Some((continuation, persistent, consume)) = &self.pairs else {
            return Ok(Vec::new());
        };
        let mut failed = Vec::new();
        for candidate in &self.tuple {
            let datum = &candidate.datum;
            match rule.admit_item(
                continuation,
                *persistent,
                self.channels.len(),
                &datum.a,
                datum.persist,
            )? {
                true => {}
                false => failed.push((datum.source.hash.clone(), consume.clone())),
            }
        }
        Ok(failed)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): the tail read of a
    /// head row, through the `Discipline` method of its collection over the store of the commit,
    /// with the fire count before the row. A deviating head row stores or parks (CR-2), so its
    /// unit fires nothing. A machinery row answers with its end (`RowEnd`), `Ends` for `()`.
    fn tail_read(
        &mut self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<TailRead, SpaceTxnError> {
        let Some((fires_before, book)) = &self.tail else {
            return self.end.tail_read(lists, matcher);
        };
        let Some(collection) = self.channels.first() else {
            return Err(SpaceTxnError::Inconsistent(
                "a head row search has no channel",
            ));
        };
        let mut view = CommitView::of_tail(
            lists,
            matcher,
            self.theory.as_deref(),
            Some(collection),
            book,
            1,
        );
        self.discipline
            .tail_read(&mut view, *fires_before)
            .map_err(join::join_commit_error)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.6): the re-issue of the
    /// end (`RowEnd`).
    fn tail_reissue(&mut self) -> Option<RowReissue> { self.end.tail_reissue() }

    /// Added by decision 124 (plan v4 C7.11, rules CR-31 and CR-33, ruling RADD2v2 6.6): the
    /// finish rows of the end (`RowEnd`).
    fn finish_rows(&mut self) -> Vec<ParFinishRow> { self.end.finish_rows() }
}

/// Added by decision 124 (plan v4 C7.9, ruling OP-J of RDOJ 1): the reads of a search over the
/// store of one space, in the form of `PrefixView`. The staging reads its
/// staged view through `Cx`, and a row search reads the store of the commit through
/// [`CommitView`]. It is the trait form of the reader closures of the base search (base
/// `ops_produce.rs`:370-379), so one search serves both (decision 123).
pub trait StoreView {
    /// The records of `channel`.
    fn data_on(&mut self, channel: &Par) -> Result<&[ParDatum], SpaceError>;

    /// The waiters of the group of `channel` alone.
    fn waiters_on(&mut self, channel: &Par) -> Result<&[ParWaiter], SpaceError>;

    /// The join groups that name `channel`.
    fn joins_on(&mut self, channel: &Par) -> Result<&[Vec<Par>], SpaceError>;

    /// The waiters of the join group `group`.
    fn group_waiters(&mut self, group: &[Par]) -> Result<&[ParWaiter], SpaceError>;

    /// The indices of the records of `channel` in the canonical order of DR-75.
    fn canonical_data_order(&mut self, channel: &Par) -> Result<Vec<usize>, SpaceError>;

    /// The indices of the records of `channel` in the pick order of the role, for the operation
    /// `source`.
    fn pick_data_order(
        &mut self,
        channel: &Par,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Vec<usize>, SpaceError>;

    /// As [`StoreView::pick_data_order`], with the record `extra` at the index after the records.
    fn pick_data_order_with(
        &mut self,
        channel: &Par,
        extra: &ParDatum,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Vec<usize>, SpaceError>;

    /// The spatial match of `pattern` against `data`, or `None`.
    fn bind(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom>;

    /// The spatial match of `pattern` against the datum `data`, then the item test of the view
    /// on the bound value, with the persistence `persist` of the datum (rule R5). A refused datum
    /// is a miss of the pattern, so the search goes on to the next datum.
    fn bind_item(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
        persist: bool,
    ) -> Result<Option<ListParWithRandom>, SpaceError>;

    /// True when the space has a theory.
    fn has_theory(&self) -> bool;

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, C4v2 B.4): true when the binds of
    /// this view run an item test (`bind_item`), so a class binds only when each of its
    /// members passes it. The staging tests no item.
    fn tests_items(&self) -> bool;

    /// The guard of `continuation` over `matched`, then the theory on each datum of `stored`.
    fn admits_all(
        &mut self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
        stored: &[StoredMatch<'_>],
    ) -> Result<bool, SpaceError>;

    /// The guard of `continuation` over the values `matched` of a list, then the theory on each
    /// stored member of `members` (P7, TH-6). `stored` gives the record of a stored member, or
    /// `None` for the element in flight.
    fn admits_records<T>(
        &mut self,
        continuation: &TaggedContinuation,
        matched: &ListParWithRandom,
        members: &[T],
        stored: impl Fn(&T) -> Option<&ListParWithRandom>,
        payload_of: fn(&ListParWithRandom) -> Result<ListParWithRandom, SpaceError>,
    ) -> Result<bool, SpaceError>;

    /// The theory of the space on each datum of `stored`. It holds without a theory.
    fn theory_admits(&self, stored: &[StoredMatch<'_>]) -> Result<bool, SpaceError>;

    /// Added by decision 124 (plan v4 C7.11, rule CR-20, ruling RADD 1.4): true when the book that
    /// the view reads holds the pair of `record` and `waiter`. A view without a book holds none.
    fn booked(&self, record: &Produce, waiter: &Consume) -> bool;

    /// Added by decision 124 (plan v4 C7.8, rule CR-20, rulings RAMD 1.5 and RADD3 1.2): true when
    /// the book that the view reads holds the pair of `waiter` and a member of a list: the element
    /// in flight `in_flight`, when the list holds it, or a stored member of `stored`. One booked
    /// member skips the whole list for the waiter (C4v2 B.4), so a re-issue does not fire the list
    /// again (C4v2 B.7). The staging and the tail read share this one rule.
    fn list_booked<'m>(
        &self,
        in_flight: Option<&'m Produce>,
        stored: impl IntoIterator<Item = &'m ParDatum>,
        waiter: &Consume,
    ) -> bool {
        in_flight
            .into_iter()
            .chain(stored.into_iter().map(|record| &record.source))
            .any(|record| self.booked(record, waiter))
    }

    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the data channel of the collection
    /// of a head row. A view of a join scope has none, so it fails with `Inconsistent`.
    fn data_channel(&self) -> Result<&Par, SpaceError>;

    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the records of the collection of a
    /// head row, as [`StoreView::data_on`] of its data channel reads them.
    fn data(&mut self) -> Result<&[ParDatum], SpaceError>;

    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the waiters of the group of the
    /// collection of a head row alone, as [`StoreView::waiters_on`] of its data channel reads them.
    fn waiters(&mut self) -> Result<&[ParWaiter], SpaceError>;
}

/// Added by decision 124 (plan v4 C7.9, ruling OP-J of RDOJ 1): the staging reads its staged
/// view. Each read is the `Cx` read of the same name.
impl StoreView for Cx<'_, '_> {
    fn data_on(&mut self, channel: &Par) -> Result<&[ParDatum], SpaceError> {
        Cx::data_on(self, channel)
    }

    fn waiters_on(&mut self, channel: &Par) -> Result<&[ParWaiter], SpaceError> {
        Cx::waiters_on(self, channel)
    }

    fn joins_on(&mut self, channel: &Par) -> Result<&[Vec<Par>], SpaceError> {
        Cx::joins_on(self, channel)
    }

    fn group_waiters(&mut self, group: &[Par]) -> Result<&[ParWaiter], SpaceError> {
        Cx::group_waiters(self, group)
    }

    fn canonical_data_order(&mut self, channel: &Par) -> Result<Vec<usize>, SpaceError> {
        Cx::canonical_data_order(self, channel)
    }

    fn pick_data_order(
        &mut self,
        channel: &Par,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Vec<usize>, SpaceError> {
        Cx::pick_data_order(self, channel, source)
    }

    fn pick_data_order_with(
        &mut self,
        channel: &Par,
        extra: &ParDatum,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Vec<usize>, SpaceError> {
        Cx::pick_data_order_with(self, channel, extra, source)
    }

    fn bind(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        Cx::bind(self, pattern, data)
    }

    // The staging runs no item test. The commit runs it under owner funding (RC2 D-10).
    fn bind_item(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
        _persist: bool,
    ) -> Result<Option<ListParWithRandom>, SpaceError> {
        Ok(Cx::bind(self, pattern, data))
    }

    fn has_theory(&self) -> bool { Cx::has_theory(self) }

    fn tests_items(&self) -> bool { false }

    fn admits_all(
        &mut self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
        stored: &[StoredMatch<'_>],
    ) -> Result<bool, SpaceError> {
        Cx::admits_all(self, continuation, matched, stored)
    }

    fn admits_records<T>(
        &mut self,
        continuation: &TaggedContinuation,
        matched: &ListParWithRandom,
        members: &[T],
        stored: impl Fn(&T) -> Option<&ListParWithRandom>,
        payload_of: fn(&ListParWithRandom) -> Result<ListParWithRandom, SpaceError>,
    ) -> Result<bool, SpaceError> {
        Cx::admits_records(self, continuation, matched, members, stored, payload_of)
    }

    fn theory_admits(&self, stored: &[StoredMatch<'_>]) -> Result<bool, SpaceError> {
        Cx::theory_admits(self, stored)
    }

    // Added by decision 124 (plan v4 C7.11, rule CR-20, ruling RADD 1.4): the book of the plan,
    // as the inherent `Cx::booked` reads it.
    fn booked(&self, record: &Produce, waiter: &Consume) -> bool {
        Cx::booked(self, record, waiter)
    }

    // Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the reads of the settle views.
    fn data_channel(&self) -> Result<&Par, SpaceError> { Ok(Cx::data_channel(self)) }

    fn data(&mut self) -> Result<&[ParDatum], SpaceError> { Cx::data(self) }

    fn waiters(&mut self) -> Result<&[ParWaiter], SpaceError> { Cx::waiters(self) }
}

/// The outer store of a family: how a channel names a collection, and where the records and the
/// counter of a collection live.
pub trait OuterStore: Default + Send + Sync + 'static {
    /// The `outer` name of the family in the config record.
    const NAME: &'static str;

    // Disabled by decision 94: every operation takes phase B over its own lock keys and the keys
    // of its scope in one call, so no store runs a join operation in a new transaction.
    // /// True when a `Join` operation of the store always runs in a new transaction over the lock
    // /// keys of its scope. The first transaction of an operation locks the channels of its routed
    // /// key. A store whose scope never holds these channels (the PathMap store) escalates every
    // /// join operation (joins plan section 9).
    // const ALWAYS_ESCALATES: bool = false;

    /// CLASSIFY of a collection operation of the store (joins plan section 4.1). The keyed stores
    /// use the keyed classification. A store with a join source of its own routes its operations
    /// to the join engine over its own scope (joins plan section 9).
    fn classify<D: Discipline>(
        agent: &GenericSpace<Self, D>,
        opened: &mut CollectionTxn,
        op: &SpaceOp,
    ) -> Result<JoinRoute, SpaceError> {
        join::classify(agent, opened, op)
    }

    /// RUN-JOIN of a collection operation of the store (joins plan section 4.2). The keyed stores
    /// run the engine with the keyed source. A store with a join source of its own runs the
    /// engine with that source (joins plan section 9).
    fn run_join<D: Discipline>(
        agent: &GenericSpace<Self, D>,
        env: &SpaceEnv,
        opened: CollectionTxn,
        scope: JoinScope,
        op: SpaceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        join::run_join(agent, env, opened, scope, op)
    }

    /// True when the store allocates index names (P5d, F11). A store without an allocator
    /// answers every allocation with [`Allocation::Baseline`] and does no work for it.
    const ALLOCATOR: bool = false;

    /// Added by decision 127 (kind registry): the qualifiers that a space of this store can
    /// have. A store that takes only some of the qualifiers overrides this default.
    const QUALIFIERS: &'static [models::rust::space_factory_urn::SpaceQualifier] =
        &models::rust::space_factory_urn::SpaceQualifier::ALL;

    /// Added by decision 127 (kind registry): the value rule of the constructor param `key` that
    /// this store brings, or `None`. A key without a rule of its family takes the rule of the
    /// built-in table (`defaults::ParamRule`).
    fn param_rule(_key: &str) -> Option<super::factory::defaults::ParamCheck> { None }

    /// The key of the collection that `channel` names. Routing computes the same key.
    // Changed by decision 128 (kind routing): the routed key of A5 is the default. The text
    // before:
    // fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError>;
    fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError> {
        CollectionKey::of_channel(channel)
    }

    /// Checks the routed key `key` of the collection that `channel` names, under the config
    /// `config` of the space, before any staging. An Array or a Vector space accepts only its own
    /// index names below its size (`NotAnIndexName`, F8). The default accepts every key and does
    /// no work.
    fn check_key(
        &self,
        _config: &SpaceConfig,
        _channel: &Par,
        _key: &CollectionKey,
    ) -> Result<(), SpaceError> {
        Ok(())
    }

    /// Checks the payload `data` of a send to a collection of the store, before any staging. A
    /// HashSet collection holds units, so the HashSet store accepts only the empty payload
    /// (`PayloadNotEmpty`, P7 HS-2). The default accepts every payload and does no work.
    fn check_payload(&self, _data: &ListParWithRandom) -> Result<(), SpaceError> { Ok(()) }

    /// Checks the pattern `pattern` of a receive bind on a collection of the store, before any
    /// staging. A HashSet receive takes a unit, so the HashSet store accepts only the empty
    /// pattern list, with no binder and no remainder (`PatternNotEmpty`, P7 HS-3). The default
    /// accepts every pattern and does no work.
    fn check_patterns(&self, _pattern: &BindPattern) -> Result<(), SpaceError> { Ok(()) }

    /// The channel of the records and waiting continuations of the collection `key`.
    // Changed by decision 128 (kind routing): `D(sid,key)` of A5 is the default. The text
    // before:
    // fn data_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par;
    fn data_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
        namespace::gprivate(namespace::data_channel(space, key))
    }

    /// The counter channel of the collection `key`.
    // Changed by decision 128 (kind routing): `S(sid,key)` of A5 is the default. The text
    // before:
    // fn counter_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par;
    fn counter_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
        namespace::gprivate(namespace::collection_counter(space, key))
    }

    /// The non-persistent records that the creation of `space` stages after the space counter,
    /// with their channels: for example the allocator `A(sid) = 0` of an Array or a Vector space
    /// (P5d). `random` is the random state of the factory call, and `authority` is the cost
    /// authority of the caller. The channels join the lock keys of the creation. The default
    /// stages none.
    fn creation_records(
        &self,
        _space: NativeSpace,
        _random: &Blake2b512Random,
        _authority: &CostAuthority,
    ) -> Result<Vec<(Par, ListParWithRandom)>, SpaceError> {
        Ok(Vec::new())
    }

    /// Stages the allocation of `op.count` index names in `cx` and returns them in order (P5d,
    /// F11). Only a store with [`OuterStore::ALLOCATOR`] is asked, so the default fails with
    /// `Inconsistent`.
    fn allocate(
        &self,
        _cx: &mut AllocCx<'_>,
        _config: &SpaceConfig,
        _op: &AllocOp,
    ) -> Result<Vec<u64>, SpaceError> {
        Err(SpaceError::Inconsistent("the outer store has no allocator"))
    }

    /// A fresh name of `space` for the handle method `gensym`. `random` is the random state of
    /// the call.
    // Changed by decision 128 (kind routing): the random-tail name of P4 is the default:
    // `N(sid ‖ tail)` with the 32-byte tail of `RandomPurpose::Gensym` (P4 design C.8). It
    // touches no storage. The text before:
    // fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError>;
    fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError> {
        let mut tail = [0u8; 32];
        for (byte, value) in tail
            .iter_mut()
            .zip(RandomPurpose::Gensym.derive(random).next())
        {
            *byte = value as u8;
        }
        Ok(namespace::gprivate(namespace::name_id(
            space,
            namespace::NameTail::Random(tail),
        )))
    }
}

/// One side of a collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// The records.
    Data,
    /// The waiting continuations.
    Waiters,
}

/// An entry of one side of a collection with the `ord` of its envelope.
#[derive(Clone, Copy, Debug)]
pub struct Ordered<'a, T> {
    /// The order of the entry.
    pub ord: i64,
    /// The entry.
    pub entry: &'a T,
}

/// The entries of a collection that a bind of a join can bind (P5c, joins plan section 3.2). A
// Changed by decision 118: a persistent entry can bind several positions. The text before:
// /// join with `m` binds on one collection binds `m` distinct entries of its window (decision 28).
/// join with `m` binds on one collection binds `m` entries of its window. A non-persistent entry
/// binds one position, and a persistent entry can bind several (decision 118).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JoinWindow {
    /// The first `m` entries of the head order of the records: the queue, the stack and the cell
    /// (decision 11).
    Head,
    /// For each level, the first `m` records of the level that the lane of the bind sees: the
    /// priority queue (decision 38).
    Lanes,
    // Changed by decision 102: the window of a discipline that picks at random follows the pick
    // order of the operation. The text before:
    // /// Every stored record, in the canonical order of DR-75: the bag (decision 41).
    /// Every stored record, in the pick order of the operation when the group names the collection
    /// once and the discipline picks at random, and in the canonical order of DR-75 otherwise.
    Scan,
    // Changed by decision 102: the class window takes every record, in the order of `Scan`. The
    // text before:
    // ```
    // /// One representative for each class, the first member of the class in the canonical order:
    // /// the set (F5). A bind claims its whole class.
    // ```
    /// Every stored record, in the order of `Scan`. A bind claims the whole class of the record
    /// that it binds.
    Classes,
    /// For each bind, the records that its query makes eligible, in its selection order: the
    // Changed by decision 102: a bind without a query reads the order of `Scan`. The text before:
    // ```
    // /// VectorDB (P6, design v3 4.6). A bind without a query sees every record in the canonical
    // /// order of DR-75, as a bag bind does.
    // ```
    /// similarity window (P6, design v3 4.6). A bind without a query sees every record in the
    /// order of `Scan`.
    Similarity,
}

/// The bound of one side of a collection (F15, P6): the most entries of the side, and the error
/// of a store or a park beyond it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideBound {
    /// The bound of a cell (F15): a store or a park beyond it fails with `CellOccupied`.
    Cell(usize),
    /// The record bound of a VectorDB space (P6): a store beyond it fails with
    /// `CapacityExceeded`.
    Records(usize),
}

impl SideBound {
    /// The most entries of the side.
    pub fn limit(&self) -> usize {
        match self {
            SideBound::Cell(limit) | SideBound::Records(limit) => *limit,
        }
    }

    /// The error of a store or a park on a full side.
    pub fn full(&self) -> SpaceError {
        match self {
            SideBound::Cell(_) => SpaceError::CellOccupied,
            SideBound::Records(_) => SpaceError::CapacityExceeded,
        }
    }
}

/// Added by decision 128 (kind routing): the error of a lanes step of a discipline without the
/// lanes window.
const NO_LANES: &str = "the discipline has no lanes window";

/// Added by decision 128 (kind routing): the error of a similarity step of a discipline without
/// the similarity window.
const NO_SIMILARITY: &str = "the discipline has no similarity window";

/// The inner collection of a family: the order in which the entries of a collection interact,
/// and the plans of its operations (P5 design v3, A.3.3).
///
/// The plans have defaults: the head driver of this module for `offer` and `receive`, a plain
/// re-store for `restore`, no settle, no capacity and no query params. A discipline overrides
/// the defaults that its rules change. A plan stages only through [`Cx`].
pub trait Discipline: Default + Send + Sync + 'static {
    /// The `inner` name of the family in the config record.
    const NAME: &'static str;

    /// True when every operation bumps the collection counter (M-1, decision 31).
    const USES_COUNTER: bool;

    // Disabled by decision 29: same-space joins into every collection run in the join engine of
    // P5c (`spaces::join`), so no family is gated on this flag. `Discipline::JOIN_WINDOW` gives
    // the entries that a join bind sees instead.
    // /// True when a receive may join this collection with other channels (P5).
    // const JOINS: bool;

    /// The named send params of the discipline.
    const SEND_PARAMS: &'static [&'static str];

    /// The named query params of a receive, for example `priority` on a priority queue (P5b).
    /// A discipline without query params rejects every query with `QuerySpecUnsupported`.
    const QUERY_PARAMS: &'static [&'static str] = &[];

    /// Added by decision 127 (kind registry): the value rule of the constructor param `key` that
    /// this discipline brings, or `None`. It comes before the rule of the outer store.
    fn param_rule(_key: &str) -> Option<super::factory::defaults::ParamCheck> { None }

    /// The most entries of each side of a collection, or `None` for no bound. A cell holds one
    /// record and one waiter (F15).
    const CAPACITY: Option<usize> = None;

    /// The bound of `side` of a collection of a space with the config `config`, or `None` for
    /// no bound (P6, design v3 4.0). The default is [`Discipline::CAPACITY`] on both sides,
    /// whose excess is `CellOccupied` (F15), and it reads nothing. A VectorDB space bounds the
    /// records of each collection by its `capacity`.
    fn bound(&self, _config: &SpaceConfig, _side: Side) -> Result<Option<SideBound>, SpaceError> {
        Ok(Self::CAPACITY.map(SideBound::Cell))
    }

    /// The window of a join bind on a collection of the discipline (P5c). The default is the
    /// head window of the head-only disciplines.
    const JOIN_WINDOW: JoinWindow = JoinWindow::Head;

    /// Added by decision 102 (plan v4 C4.3, rules CR-1 and CR-14): true when a plan of the
    /// discipline can pick at random in play: the bag, the set and the VectorDB. Only such a
    /// plan can need the restage of R-A, so the transaction keeps a spare copy of its operation
    /// only for it. The head-order disciplines keep the default, false (CR-2).
    const RANDOM_PICKS: bool = false;

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.4): true when the
    /// discipline settles after its plan (P5b). Row 6 of C4v2 B.6 re-issues a settle job only
    /// for such a discipline. Every other discipline keeps the default, false, because its
    /// `settle_job` fails with `Inconsistent`.
    const SETTLES: bool = false;

    /// The head of one side: the entry that interacts next, or `None` for an empty side. The
    /// result depends on the entry set only, not on the order of `entries`.
    fn head<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError>;

    /// True when a fresh element of `side` becomes the head of its side, which holds `queued`
    /// entries. A re-issued persistent element uses [`Discipline::precedes`] instead.
    fn reaches_head(&self, side: Side, queued: usize) -> bool;

    /// True when an element of `side` with the order `ord` interacts before the head of the side,
    /// whose order is `head` (decision 43). The two orders differ. A re-issued persistent element
    /// re-enters at the head exactly when its side is empty or this holds.
    fn precedes(&self, side: Side, ord: i64, head: i64) -> Result<bool, SpaceError>;

    /// Checks the values of the named params `params` of a send, and of the query `query` of a
    /// receive, under the config `config`, before the bump. The keys are already checked against
    /// the schema of the discipline. The default accepts every value.
    fn check_values(
        &self,
        _config: &SpaceConfig,
        _params: &BTreeMap<String, Par>,
        _query: Option<&QueryParams>,
    ) -> Result<(), SpaceError> {
        Ok(())
    }

    /// The produce plan of the datum of `op` with the order `ord`. The default is the head
    /// driver [`offer_head`].
    fn offer(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ProduceOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        offer_head(self, cx, op, ord)
    }

    /// The consume plan of the continuation of `op` with the order `ord`. The default is the
    /// head driver [`receive_head`].
    fn receive(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ConsumeOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        receive_head(self, cx, op, ord)
    }

    /// Stores again the non-persistent record `record` that a consume-side peek fire removed
    /// (decision 45, Rule E). The record bytes, the channel and `persist = false` fix the
    /// produce identity, so the record alone determines it, as for the host's second send. The
    /// default is a plain store with the record's own authority and no counter bump. It is
    /// exact for a head-only discipline, because a peek receive fires only when no waiter is
    /// queued.
    fn restore(
        &self,
        cx: &mut Cx<'_, '_>,
        record: ListParWithRandom,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        cx.restore(record)?;
        Ok(None)
    }

    /// The fires that the discipline owes before the plan of the operation `op`. The default
    /// owes none.
    fn settle_before(
        &self,
        _cx: &mut Cx<'_, '_>,
        _op: OpView<'_>,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        Ok(None)
    }

    /// The fires that the discipline owes after the plan of the operation `op` (P5b, F2). The
    /// default owes none.
    fn settle_after(
        &self,
        _cx: &mut Cx<'_, '_>,
        _op: OpView<'_>,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        Ok(None)
    }

    /// The plan of a re-issued settle (P5b). A discipline without a settle rule never issues
    /// one, so the default fails with `Inconsistent`.
    fn settle_job(
        &self,
        _cx: &mut Cx<'_, '_>,
        _op: SettleOp,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        Err(SpaceError::Inconsistent("the discipline has no settle"))
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): the tail read of a
    /// head row of the discipline, the first step that its continuation runs after the unit of
    /// the row, over the store view `view` with the fire count `fired` (ADD A2.8). It stages
    /// nothing. The default answers that no step reads the store, because a discipline without a
    /// settle runs no step after its plan (`settle_after`).
    fn tail_read<V: StoreView>(
        &self,
        _view: &mut V,
        _fired: usize,
    ) -> Result<TailRead, SpaceError> {
        Ok(TailRead::Ends)
    }

    /// The class of the payload `payload` in a join window (P5c). `None` makes each record a
    /// class of its own, as every discipline but the set does. The set gives its F5 class key.
    /// The default does no work.
    fn join_class_key(&self, _payload: &[Par]) -> Result<Option<Vec<u8>>, SpaceError> { Ok(None) }

    // Added by decision 128 (kind routing): the steps of the lanes window (decision 38) and of
    // the similarity window (P6, design v3 4.4). The join engine calls a step only for a
    // discipline with its window (`JoinSource::Discipline`), so the defaults fail closed.

    /// The level of a send with the named params `params`.
    fn send_level(_params: &BTreeMap<String, Par>) -> Result<Level, SpaceError> {
        Err(SpaceError::Inconsistent(NO_LANES))
    }

    /// The lane of a receive with the query `query`.
    fn receive_lane(_query: &Option<QueryParams>) -> Result<Lane, SpaceError> {
        Err(SpaceError::Inconsistent(NO_LANES))
    }

    /// The params of a space with the config `config`.
    fn similarity_config(_config: &SpaceConfig) -> Result<SimilarityConfig, SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }

    /// The record of a send with the params `params` and the data `data`, and its vector.
    fn similarity_record(
        _cfg: &SimilarityConfig,
        _params: &BTreeMap<String, Par>,
        _data: &ListParWithRandom,
    ) -> Result<(ListParWithRandom, Option<ExactVector>), SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }

    /// The vector of the stored record `record`.
    fn record_vector(
        _record: &ListParWithRandom,
        _cfg: &SimilarityConfig,
    ) -> Result<Option<ExactVector>, SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }

    /// The payload of the stored record `record`.
    fn similarity_payload(_record: &ListParWithRandom) -> Result<ListParWithRandom, SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }

    /// True for a list-mode query.
    fn is_list_query(_query: &QueryParams) -> bool { false }

    /// The waiter policy of a receive with the query `query`.
    fn policy_of_query(_query: Option<&QueryParams>) -> record::WaiterPolicy {
        record::WaiterPolicy::None
    }

    /// The checked query of the waiter policy `policy`, or `None` for a policy without a query.
    fn policy_query(
        _cfg: &SimilarityConfig,
        _policy: &record::WaiterPolicy,
    ) -> Result<Option<SimilarityQuery>, SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }

    /// The eligible records of `query` on `channel` in selection order, over the store view
    /// `view`.
    fn eligible<V: StoreView>(
        _view: &mut V,
        _cfg: &SimilarityConfig,
        _query: &SimilarityQuery,
        _channel: &Par,
        _flight: Option<&Flight<'_>>,
        _order: EligibleOrder<'_>,
    ) -> Result<Vec<Eligible>, SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }

    /// The eligibility of the element in flight `flight`, with its score.
    fn flight_eligibility(
        _query: &SimilarityQuery,
        _flight: &Flight<'_>,
    ) -> Result<Option<Option<Score>>, SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }

    /// The data that a single-mode bind binds.
    fn single_data(
        _query: &SimilarityQuery,
        _payload: &ListParWithRandom,
        _score: Option<&Score>,
    ) -> Result<ListParWithRandom, SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }

    /// The list that a list-mode bind binds, or `None`, over the store view `view`.
    fn list_selection<V: StoreView>(
        _view: &mut V,
        _cfg: &SimilarityConfig,
        _query: &SimilarityQuery,
        _k: Option<usize>,
        _pattern: &BindPattern,
        _channel: &Par,
        _flight: Option<&Flight<'_>>,
    ) -> Result<Option<ListFire>, SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }

    /// The list that the element in flight `flight` completes for a list-mode waiter, or `None`,
    /// over the store view `view`.
    fn list_offer<V: StoreView>(
        _view: &mut V,
        _cfg: &SimilarityConfig,
        _query: &SimilarityQuery,
        _k: Option<usize>,
        _pattern: &BindPattern,
        _channel: &Par,
        _flight: &Flight<'_>,
    ) -> Result<Option<ListFire>, SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }

    /// The record and the payload of each member of a fired list in list order, with the element
    /// `element` at its place, for the item test of the members.
    fn list_pairs(
        _members: &[Option<(ParDatum, usize)>],
        _element: Option<(&ParDatum, &ListParWithRandom)>,
    ) -> Result<Vec<(ParDatum, ListParWithRandom)>, SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }

    /// The item test of each member of a list: the first member with the values `matched` of
    /// the bind, each other member with its payload.
    fn members_admit(
        _request: &ParRowRequest<'_>,
        _matched: &ListParWithRandom,
        _members: &[(ParDatum, ListParWithRandom)],
    ) -> Result<bool, SpaceTxnError> {
        Err(SpaceTxnError::Inconsistent(NO_SIMILARITY))
    }

    /// The pairs of the members of a list that the waiter `waiter` may not take, by the item test
    /// of [`Discipline::members_admit`], appended to `failed`.
    fn member_failures(
        _rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
        _waiter: (&Consume, &TaggedContinuation, bool),
        _matched: &ListParWithRandom,
        _members: &[(ParDatum, ListParWithRandom)],
        _failed: &mut Vec<(Blake2b256Hash, Blake2b256Hash)>,
    ) -> Result<(), SpaceTxnError> {
        Err(SpaceTxnError::Inconsistent(NO_SIMILARITY))
    }

    /// Retires the stored members `members` of a fired list from `channel`.
    fn retire_list_members(
        _cx: &mut Cx<'_, '_>,
        _channel: &Par,
        _members: Vec<ParDatum>,
        _random: &Blake2b512Random,
        _authority: Option<&CostAuthority>,
    ) -> Result<bool, SpaceError> {
        Err(SpaceError::Inconsistent(NO_SIMILARITY))
    }
}

/// The operation around which a settle hook runs (P5b): a borrowed view. A hook copies only
/// the fields that it uses, for example to build the `SettleOp` of a re-issue.
#[derive(Clone, Copy, Debug)]
pub enum OpView<'a> {
    /// A produce.
    Produce(&'a ProduceOp),
    /// A consume.
    Consume(&'a ConsumeOp),
}

/// The steps of a family after the dispatch (P5 design v3, A.3.4). The family agent opens the
/// transaction of an operation and reads the config once. The agent of the family that the
/// config names then runs the operation in that transaction.
pub trait FamilySpace: SpaceAgent {
    /// Classifies the operation `op` in the open transaction `opened` (P5c, joins plan section
    /// 4.1): `Single` when the data channel of its collection holds no join group other than
    /// its own, and otherwise `Join` with the closure of its collections and the lock keys of
    /// that closure. A join and a settle with a cascade are always `Join`. The reads are not
    /// lock-checked, and the classification stages nothing.
    fn classify(&self, opened: &mut CollectionTxn, op: &SpaceOp) -> Result<JoinRoute, SpaceError>;

    /// Steps 3 to 8 of the collection operation `op` in the open transaction `opened`, whose
    /// config names the family of this agent (`RUN-WITH`).
    fn run_with(
        &self,
        env: &SpaceEnv,
        opened: CollectionTxn,
        op: SpaceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError>;

    /// The join engine (P5c) for the operation `op` in the open transaction `opened`, whose
    /// lock keys cover the closure `scope`.
    fn run_join(
        &self,
        env: &SpaceEnv,
        opened: CollectionTxn,
        scope: JoinScope,
        op: SpaceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError>;

    /// The allocation of `op` in the open transaction `opened`, whose config names the family of
    /// this agent (P5d, F11).
    fn run_allocate(
        &self,
        env: &SpaceEnv,
        opened: AllocationTxn,
        op: &AllocOp,
    ) -> Result<Allocation, SpaceError>;

    // Disabled by decision 94: phase B covers the scope of every operation, so no family runs a
    // join operation in a new transaction.
    // /// True when a `Join` operation of the family always runs in a new transaction over the
    // /// lock keys of its scope (`OuterStore::ALWAYS_ESCALATES`).
    // fn always_escalates(&self) -> bool;
}

/// The native space agent of the family `S:D`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GenericSpace<S, D> {
    outer: S,
    discipline: D,
}

impl<S, D> GenericSpace<S, D> {
    /// The agent of the outer store `outer` and the discipline `discipline`.
    pub const fn new(outer: S, discipline: D) -> Self { Self { outer, discipline } }

    /// The outer store of the family.
    pub(crate) fn outer(&self) -> &S { &self.outer }

    /// The discipline of the family.
    pub(crate) fn discipline(&self) -> &D { &self.discipline }
}

/// The channels of one collection operation, in the order of its lock keys: the config channel
/// `C(sid)`, the space counter `S(sid)`, the collection counter `S(sid,k)` and the data channel
/// `D(sid,k)`.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectionChannels {
    keys: [Par; 4],
}

impl CollectionChannels {
    /// The channels of the collection `key` of `space` in the outer store `outer`.
    pub fn new<S: OuterStore>(outer: &S, space: NativeSpace, key: &CollectionKey) -> Self {
        Self {
            keys: [
                namespace::gprivate(namespace::config_channel(space)),
                namespace::gprivate(namespace::space_counter(space)),
                outer.counter_channel(space, key),
                outer.data_channel(space, key),
            ],
        }
    }

    /// The config channel `C(sid)`.
    pub fn config(&self) -> &Par {
        let [config, _, _, _] = &self.keys;
        config
    }

    /// The space counter `S(sid)`.
    pub fn space_counter(&self) -> &Par {
        let [_, space_counter, _, _] = &self.keys;
        space_counter
    }

    /// The collection counter `S(sid,k)`.
    pub fn counter(&self) -> &Par {
        let [_, _, counter, _] = &self.keys;
        counter
    }

    /// The data channel `D(sid,k)`.
    pub fn data(&self) -> &Par {
        let [_, _, _, data] = &self.keys;
        data
    }

    /// The lock keys of a collection operation: every channel that it can touch (readiness
    /// review S2). The space counter is a lock key, because the first operation on a collection
    /// consumes it.
    pub fn lock_keys(&self) -> &[Par; 4] { &self.keys }

    fn counters(&self) -> CounterChannels<'_> {
        CounterChannels {
            space: self.space_counter(),
            collection: self.counter(),
        }
    }
}

/// The kind of a staged operation, to check its receipt and to build the outcome. The staged
/// list holds one byte for each operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Staged {
    /// A take or a put of a counter.
    Counter(CounterOp),
    /// A produce that stores its record.
    Store,
    /// A consume that parks its continuation.
    Park,
    /// A fire of a continuation of the program. The outcome dispatches every result of its
    /// receipt.
    Fire,
    /// A fire of a continuation of the program whose outcome dispatches only the first results
    /// of its receipt. Their count is the next entry of the prefix list of the plan: a consume
    /// over `[D; n]` that retires the other members of a set class binds only the first (F3).
    FirePrefix,
    /// A consume fire that retires a stored member of a set class (P5a, F3). It is never
    /// dispatched.
    Retire,
    /// A consume fire that lifts a stored record out of its collection during a settle (P5b,
    /// F2). It is never dispatched.
    Lift,
    /// A fire of a waiter with two or more binds (P5c). Its continuation record has the slots
    /// of version 2, and the outcome dispatches every result of its receipt in the bind order of
    /// the receive.
    JoinFire,
    /// Added by decision 92: the anchor of the transaction, a consume fire on `C(sid)` that
    /// leaves the config record in place (`spaces::anchor`). It is never dispatched.
    Anchor,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.3, RADD2 1.3 and
/// RADD2v2 6.1): the row of the table of C4v2 B.6 that a staged row belongs to. The finish
/// reads the class of row d for Tail(actual). The class list of the plan holds one byte for
/// each staged row, beside the staged list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TailRow {
    /// A take or a put of a counter, or the anchor (row 7). Such a row is never refused or
    /// skipped (CR-15 B1).
    Counter,
    /// A lift, a fire or a land of the settle before the plan (row 1). The element follows it.
    Before,
    /// A store, a park or a fire of the element of the operation (rows 2 to 6).
    Element,
    /// A lift after the plan or in a settle job (row 8). The re-offer follows it.
    Lift,
    /// A fire or a land of a settle after the plan or in a settle job (row 9).
    Settle,
    /// A retire of the group of a fire (row 8b). The cut of a group names its unit row, so a
    /// retire is never row d (CR-33).
    Retire,
    /// A fire that binds one member of its class (rows 3 and 5). Its class stays when the
    /// receive peeks or a member of the class is persistent (CR-9), and the receipt lists the
    /// bound member alone. `persistent` is true when a member of the class other than the bound
    /// one is persistent.
    ClassFire { persistent: bool },
    /// A store or a fire of the second send of Rule E: the record that a peek fire removed,
    /// sent again (decision 45). Its re-issue is the send of that record, not the operation.
    Restore,
    /// A lift of a cascade after the plan whose element settled (row 8). The re-offer follows
    /// it. Its settle job carries a cascade state, so it runs in the join engine
    /// (`join::classify`), as the stop of such a plan does.
    CascadeLift,
    /// A lift of a cascade after the plan whose element met a divergent pair (row 8, A10). Its
    /// settle job also carries the re-issue of the operation, which runs after the cascade.
    DivergentCascadeLift,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.3 and RADD2 1.1): how
/// Tail(actual) of row d ends the operation of a deviating commit (C2 E4.5). The search of a row
/// builds it at the commit when the end needs the state of that search (`RowDeviation::reissue`).
/// The finish builds it from the receipt for every other row (`tail_end`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TailEnd {
    /// Return(v): the continuation stages nothing more and ends with `v`, with its own cause.
    Return(Option<SpaceReissue>),
    /// Stop: the continuation would make one more staging step, so the finish re-issues the
    /// rest of the operation for an owner refusal (CR-19 case 2).
    Stop(SpaceOp),
}

/// Added by decision 102 (plan v4 C4.2, rules CR-1 and CR-14): the order of the pick helpers
/// of [`Cx`] in play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickMode {
    /// A pick shuffles the DR-75 order of its candidates once (CR-1).
    Random,
    /// A pick keeps the DR-75 order: the restage of R-A (CR-14).
    Canonical,
}

/// Added by decision 102 (plan v4 C4.2, rule CR-20): one pair of the book of a space, the
/// source hash of a record and the source hash of a continuation.
type BookPair = (Blake2b256Hash, Blake2b256Hash);

/// What the plans of one operation share.
struct Plan<'a> {
    data: &'a Par,
    /// The config of the space, read once at the begin of the operation. A VectorDB plan
    /// reads its metric and its `capacity` from it (P6, design v3 4.0).
    config: &'a SpaceConfig,
    matcher: &'a SpaceMatcher,
    /// The introduction of the operation, which every staged operation registers by default.
    introduction: &'a Arc<Introduction>,
    staged: Vec<Staged>,
    /// The dispatched counts of the `FirePrefix` fires, in staging order. A head-only plan
    /// stages none, so the list never allocates for it.
    prefixes: Vec<usize>,
    /// The value of the collection counter before the bump of this round, or `None` for a
    /// discipline without a counter.
    bumped: Option<i64>,
    // Disabled by decision 90 (decision 69): Option P is gone. The field held the legacy
    // charges of the staged operations (decision 45, Option P), and every staged operation of
    // the plan went through it.
    // legacy: LegacyChain,
    /// The fires of this transaction, its settles included (I-21). Every fire loop of the
    /// transaction counts on it, so one transaction makes at most `FIRE_ROUND_CAP` fires.
    rounds: FireRounds,
    /// The mirror capture of the transaction (P6 step 7b): active for a VectorDB space with a
    /// mirror. It records the staged stores, and after the commit it gives the net writes of
    /// the transaction, with no error path.
    mirror: MirrorCapture,
    /// The claims of a program job on a `seq` space (P7, SQ-3), or `None`. Every staging
    /// primitive of [`Cx`] claims the channels that it uses, and the job stores the claims
    /// after its commit succeeded (SQ-4).
    seq: Option<SeqClaims>,
    /// The checker of the theory of the space, or `None` (P7, TH-6). The config read resolved
    /// it, so the plan reads nothing more for it.
    theory: Option<Arc<dyn SpaceTheory>>,
    /// Changed by decision 124 (L6): the leases of the transaction, in order (P7, TH-9): those of
    /// the admission, then one for each proven stored datum of each fire, at the fire. The text
    /// before: "The leases of the `Proven` verdicts of the transaction, in order (P7, TH-9)." Two
    /// or more abort the transaction with `NestedCommitGuard` before its commit.
    leases: Vec<Box<dyn TheoryCommitLease>>,
    /// Added by decision 92: the job of the transaction. Each staging primitive of [`Cx`]
    /// compares the random state of its operation with the job, and it stages the anchor first
    /// when the first operation is not the job's own (`spaces::anchor`).
    job: &'a SpaceJob,
    /// Added by decision 102 (plan v4 C4.2, rule CR-14): true when a pick of the staging
    /// shuffled two or more candidates. Only then can a failure of the staging depend on the
    /// pick, so only then does R-A restage.
    picked: bool,
    /// Added by decision 102 (plan v4 C4.2): the order of the pick helpers in play. It is
    /// `Random` for the first staging and `Canonical` for the restage of R-A.
    order: PickMode,
    /// Added by decision 102 (plan v4 C4.2, rule CR-20): the lifts of the staging, each with
    /// its index in the staged list and the pair that its refusal books. A lift gets its entry
    /// only after its stage call succeeded (CR-6).
    lifts: Vec<(usize, BookPair)>,
    /// Added by decision 124 (L14, plan v4 C7.8, rule CR-20): the snapshot of the book that the
    /// staging reads (F13). `restart` keeps it, so the restage of R-A reads the same pairs.
    book: Arc<BookPairs>,
    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): the B.6 class of
    /// each staged row, with the index of `staged` (`Plan::push_row`). The finish reads the
    /// class of row d for Tail(actual).
    tail_rows: Vec<TailRow>,
    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.3): the one copy of the operation
    /// of an owner-funded run, taken before its staging, for the re-issue of Tail(actual). The
    /// row searches of the join engine share it (`EngineInputs`). `None` without owner
    /// funding (RC2 D-10).
    operation: Option<Arc<SpaceOp>>,
}

impl Plan<'_> {
    /// Added by decision 102 (plan v4 C4.3, rule CR-14): true when R-A restages after the
    /// staging failed with `error` in the mode `mode`: in play, after a pick that shuffled two
    /// or more candidates, with a positional error or `SeqConcurrent` (U5). The restage itself
    /// keeps the DR-75 order and sets no pick flag, so it never restages again.
    fn restages(&self, mode: SpaceMode, error: &SpaceError) -> bool {
        match (mode, self.picked) {
            (SpaceMode::Play, true) => {
                error.is_positional() || matches!(error, SpaceError::SeqConcurrent { .. })
            }
            (SpaceMode::Play, false) | (SpaceMode::NativeReplay, _) => false,
        }
    }

    /// Added by decision 102 (plan v4 C4.3, rule CR-14): resets the plan for the restage of
    /// R-A after the transaction discarded its staged operations (steps 2, 3, 4 and 6). The
    /// staged list with its classes, the prefixes, the bump, the fire count, the mirror
    /// capture, the lifts and the pick flag start again. The leases and the seq claims go back
    /// to `restart`. The pick helpers then keep the DR-75 order. The copy of the operation
    /// stays, because the restage runs the same operation (decision 124, ruling RADD 1.3).
    fn restart(&mut self, restart: &Restart) {
        self.staged.clear();
        self.tail_rows.clear();
        self.prefixes.clear();
        self.bumped = None;
        self.rounds = FireRounds::new();
        self.mirror = MirrorCapture::of(self.config);
        self.leases.truncate(restart.leases);
        self.lifts.clear();
        self.picked = false;
        self.order = PickMode::Canonical;
        if let (Some(claims), Some(snapshot)) = (self.seq.as_mut(), restart.seq.as_ref()) {
            claims.rewind(snapshot);
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): appends the staged
    /// row `kind` to the staged list and its B.6 class to the class list, so the two lists keep
    /// one index. Every push of the staged list comes here (F-PUSH). A counter row and the
    /// anchor are of class `Counter`, a lift of class `Lift`, a retire of class `Retire`, and
    /// every other row of class `Element`. The settle entry points of SETTLE and the discipline
    /// drivers then relabel the rows that they know more about (`Cx::settle_rows`,
    /// `Cx::class_fire`, `Cx::restore_rows`).
    fn push_row(&mut self, kind: Staged) {
        let class = match kind {
            Staged::Counter(_) | Staged::Anchor => TailRow::Counter,
            Staged::Lift => TailRow::Lift,
            Staged::Retire => TailRow::Retire,
            Staged::Store | Staged::Park | Staged::Fire | Staged::FirePrefix | Staged::JoinFire => {
                TailRow::Element
            }
        };
        self.staged.push(kind);
        self.tail_rows.push(class);
    }

    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.3, RC2 D-10): keeps one copy of the
    /// operation `op` of an owner-funded run, taken before its staging, for the re-issue of
    /// Tail(actual). A run without owner funding never deviates, so it copies nothing. A later
    /// call keeps the first copy, so the restage of R-A copies nothing more.
    fn keep_operation(&mut self, op: &SpaceOp) {
        match self.introduction.owner_funded() {
            false => {}
            true => {
                self.operation.get_or_insert_with(|| Arc::new(op.clone()));
            }
        }
    }
}

/// Added by decision 102 (plan v4 C4.3, rule CR-14, F6): where the restage of R-A starts: the
/// count of the leases that the admission of the operation gave, and the seq claims of the job
/// after the claim of its own collection.
struct Restart {
    leases: usize,
    seq: Option<SeqSnapshot>,
}

impl Restart {
    /// The start of the restage of an operation with the admission `admission` and the seq
    /// claims `seq`, taken before the first staging.
    fn of(admission: &Admission, seq: Option<&SeqClaims>) -> Self {
        Self {
            leases: admission.leases.len(),
            seq: seq.map(SeqClaims::snapshot),
        }
    }
}

/// A stored datum that a match binds (P7, TH-6): its payload as its send carried it, and the
/// values that the pattern of its bind captures. The theory of the space checks each stored
/// datum of a match after the guard admitted the match.
#[derive(Clone, Copy, Debug)]
pub struct StoredMatch<'a> {
    /// The payload of the stored datum, without its envelope.
    pub payload: &'a ListParWithRandom,
    /// The values that the pattern of its bind captures.
    pub captures: &'a ListParWithRandom,
}

/// Where the datum of a match is (P7, TH-6): a record of the store, or a datum in flight (the
/// datum of a send, or a record that a cascade, a re-offer or a second send carries). The theory
/// checks only stored data at a match: a datum in flight was admitted when it was sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    /// A record of the store.
    Stored,
    /// A datum in flight.
    InFlight,
}

/// Added by decision 102 (plan v4 C7.22, rule CR-19 case 3, F22): the datum that a produce-side
/// fire offers. A cascade re-offer of a stored persistent record keeps the source of that record.
/// Every re-offer of the record has that source, also a re-offer by a job of another chain, so
/// the row of such a re-offer is not unique to its job (`SpaceJob::owns_first_reoffer`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offered {
    /// A datum in flight: the element of an operation, a second send, a restored record, or a
    /// record that a lift removed.
    InFlight,
    /// A stored persistent record that a cascade offers again. The record stays stored.
    StoredPersistent,
}

/// A produce-side fire: the head waiter that the datum fires, and the values that it binds.
pub struct OfferFire {
    /// True for a persistent send.
    pub persistent: bool,
    /// The waiter that the datum fires.
    pub waiter: ParWaiter,
    /// The values that the waiter binds.
    pub matched: ListParWithRandom,
    /// Added by decision 102 (plan v4 C7.22): the datum that the fire offers.
    pub offered: Offered,
    /// Added by decision 124 (plan v4 C7.9, ruling D-1): the row search of the fire, which the
    /// kind code that stages it builds, or None without owner funding (ruling D-10).
    pub search: Option<ParRowSearch>,
}

/// A consume-side fire of one record: the record (the plan's own copy), the values that it
/// binds, and its index in the staged view of its channel.
pub struct ReceiveFire {
    /// The fired record.
    pub head: ParDatum,
    /// The values that the record binds.
    pub matched: ListParWithRandom,
    /// The index of the record in the staged view of its channel.
    pub view_index: i32,
}

/// The view and the staging primitives of one collection operation (P5 design v3, A.3.3). A
/// plan reads the staged view and stages only through these methods, so every staged operation
/// registers an introduction (P4 design B.7). `Cx` exposes no raw transaction. Changed by
/// decision 90 (decision 69): the legacy chain of decision 45 (Option P) is gone.
pub struct Cx<'c, 'p> {
    txn: &'c mut ParSpaceTxn,
    plan: &'c mut Plan<'p>,
}

impl<'p> Cx<'_, 'p> {
    /// The data channel `D(sid,k)` of the collection of the operation.
    pub fn data_channel(&self) -> &'p Par { self.plan.data }

    /// The config of the space of the operation (P6, design v3 4.0). It is the config that the
    /// begin of the operation read once, so a plan only borrows it.
    pub fn config(&self) -> &'p SpaceConfig { self.plan.config }

    /// The introduction of the operation.
    pub fn introduction(&self) -> &'p Arc<Introduction> { self.plan.introduction }

    /// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the job of the
    /// operation, for the finish rows that a search builds at the commit (P1).
    pub(crate) fn job(&self) -> &'p SpaceJob { self.plan.job }

    /// The value of the collection counter before the bump of this round, or `None` for a
    /// discipline without a counter. Each round bumps the counter, so the value differs in every
    /// round, also for a re-issued persistent operation that keeps its first-round order
    /// (decision 43). The body of a `K_lift` continuation needs it (P5b).
    pub fn bumped(&self) -> Option<i64> { self.plan.bumped }

    /// The fires of the transaction so far (I-21). The settle before the plan, the plan and the
    /// settle after it count on this one `FireRounds`, so the transaction makes at most
    /// `FIRE_ROUND_CAP` fires.
    pub fn rounds(&mut self) -> &mut FireRounds { &mut self.plan.rounds }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): the count of the
    /// staged rows. A settle and a second send read it before they stage, so they can give
    /// their rows their class.
    pub fn staged_rows(&self) -> usize { self.plan.staged.len() }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): gives the rows that
    /// a settle staged from the row `start` on their B.6 class. Before the plan (`before`) a
    /// lift, a fire and a land are of class `Before`, because the element follows them (row 1).
    /// After the plan and in a settle job a fire and a land are of class `Settle` (row 9), and
    /// a lift keeps its class (row 8). A counter row and the anchor keep theirs (row 7).
    pub fn settle_rows(&mut self, start: usize, before: bool) {
        let (element, lift) = match before {
            false => (TailRow::Settle, TailRow::Lift),
            true => (TailRow::Before, TailRow::Before),
        };
        for class in self.plan.tail_rows.iter_mut().skip(start) {
            *class = match *class {
                TailRow::Element => element,
                TailRow::Lift => lift,
                TailRow::Counter
                | TailRow::Before
                | TailRow::Settle
                | TailRow::Retire
                | TailRow::ClassFire { .. }
                | TailRow::Restore
                | TailRow::CascadeLift
                | TailRow::DivergentCascadeLift => *class,
            };
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): gives the last
    /// staged row, a fire that binds one member of its class, the class of a class fire. Its
    /// class stays when the receive peeks or a member of the class is persistent (CR-9).
    /// `persistent` is true when a member other than the bound one is persistent, because the
    /// receipt lists the bound member alone.
    pub fn class_fire(&mut self, persistent: bool) -> Result<(), SpaceError> {
        let Some(class) = self.plan.tail_rows.last_mut() else {
            return Err(SpaceError::Inconsistent(
                "a class fire follows no staged row",
            ));
        };
        *class = TailRow::ClassFire { persistent };
        Ok(())
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): gives the rows of
    /// the element class that the second send of Rule E staged from the row `start` on the
    /// class `Restore` (decision 45). A retire of its group keeps its class (row 8b).
    pub fn restore_rows(&mut self, start: usize) {
        for class in self.plan.tail_rows.iter_mut().skip(start) {
            *class = match *class {
                TailRow::Element => TailRow::Restore,
                TailRow::Counter
                | TailRow::Before
                | TailRow::Lift
                | TailRow::Settle
                | TailRow::Retire
                | TailRow::ClassFire { .. }
                | TailRow::Restore
                | TailRow::CascadeLift
                | TailRow::DivergentCascadeLift => *class,
            };
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.1): gives the last
    /// staged row, the lift of a cascade after the plan, the class of a cascade lift, with the
    /// end `own` of the element of the operation: `None` when the element settled, or a
    /// divergent pair (A10), whose operation the settle job re-issues after the cascade. A tail
    /// runs after the plan only below the round cap and no plan ends its element for an owner
    /// refusal, so another end fails closed, as a last row that is not a lift does.
    pub fn cascade_lift(&mut self, own: Option<ReissueCause>) -> Result<(), SpaceError> {
        let class = match own {
            None => TailRow::CascadeLift,
            Some(ReissueCause::DivergentPair) => TailRow::DivergentCascadeLift,
            Some(ReissueCause::RoundCap | ReissueCause::OwnerRefusal) => {
                return Err(SpaceError::Inconsistent(
                    "a cascade after the plan follows an element that ended at the cap",
                ))
            }
        };
        let Some(last) = self.plan.tail_rows.last_mut() else {
            return Err(SpaceError::Inconsistent(
                "the lift of a cascade follows no staged row",
            ));
        };
        match *last {
            TailRow::Lift => {
                *last = class;
                Ok(())
            }
            TailRow::Counter
            | TailRow::Before
            | TailRow::Element
            | TailRow::Settle
            | TailRow::Retire
            | TailRow::ClassFire { .. }
            | TailRow::Restore
            | TailRow::CascadeLift
            | TailRow::DivergentCascadeLift => Err(SpaceError::Inconsistent(
                "the lift of a cascade is not the last staged row",
            )),
        }
    }

    /// Added by decision 124 (plan v4 C7.9 and C7.11, rulings OP-J and RADD 1.3): keeps the one
    /// copy of the operation `op` of an owner-funded run (`Plan::keep_operation`) and returns
    /// it, so the row searches of the join engine share it (`EngineInputs`). `None`
    /// without owner funding (RC2 D-10).
    pub fn keep_operation(&mut self, op: &SpaceOp) -> Option<Arc<SpaceOp>> {
        self.plan.keep_operation(op);
        self.plan.operation.clone()
    }

    /// Added by decision 124 (plan v4 C7.11, ruling RADD2 1.1): the copy of the operation of
    /// an owner-funded run, which `run_with` keeps before the staging, for a row search that
    /// builds the end of Tail(actual). `None` without owner funding. The copy is one reference
    /// count.
    pub fn operation(&self) -> Option<Arc<SpaceOp>> { self.plan.operation.clone() }

    /// The records of the collection in the staged view.
    pub fn data(&mut self) -> Result<&[ParDatum], SpaceError> {
        let channel = self.plan.data;
        self.data_on(channel)
    }

    /// The waiting continuations of the collection in the staged view.
    pub fn waiters(&mut self) -> Result<&[ParWaiter], SpaceError> {
        let channel = self.plan.data;
        self.waiters_on(channel)
    }

    /// The records of `channel` in the staged view. The lock keys of the operation must hold
    /// `channel`.
    pub fn data_on(&mut self, channel: &Par) -> Result<&[ParDatum], SpaceError> {
        Ok(self.txn.data(channel)?)
    }

    /// The waiting continuations of `channel` in the staged view. The lock keys of the operation
    /// must hold `channel`.
    pub fn waiters_on(&mut self, channel: &Par) -> Result<&[ParWaiter], SpaceError> {
        Ok(self.txn.continuations(std::slice::from_ref(channel))?)
    }

    /// The indices of the records of `channel` in the canonical order of DR-75, the base of the
    /// replay order (decision 102). Changed by decision 102 (plan v4 C4.1): the text before was
    /// "(decision 41)".
    /// Added by decision 102 (plan v4 C5.7): kept DR-75, because it defines the order.
    pub fn canonical_data_order(&mut self, channel: &Par) -> Result<Vec<usize>, SpaceError> {
        Ok(canonical_data_order(self.txn.data(channel)?))
    }

    /// The indices of the waiting continuations of `channel` in the canonical order of DR-75,
    /// the base of the replay order (decision 102).
    /// Added by decision 102 (plan v4 C5.7): kept DR-75, because it defines the order.
    pub fn canonical_waiter_order(&mut self, channel: &Par) -> Result<Vec<usize>, SpaceError> {
        Ok(canonical_continuation_order(
            self.txn.continuations(std::slice::from_ref(channel))?,
        ))
    }

    /// Added by decision 102 (plan v4 C4.1, rules CR-1 and CR-4): the indices of the records of
    /// `channel` in the order of a pick of the operation `source`. Play shuffles the DR-75 order
    /// once, or keeps it in the restage of R-A. Replay puts first the records that the recorded
    /// row names, in DR-75 order, and then the other records in DR-75 order.
    pub fn pick_data_order(
        &mut self,
        channel: &Par,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Vec<usize>, SpaceError> {
        let order = self.canonical_data_order(channel)?;
        self.pick_order(order, source, |txn, comm, index| {
            let datum = match txn.data(channel)?.get(index) {
                Some(datum) if comm.produces.contains(&datum.source) => datum.clone(),
                Some(_) => return Ok(false),
                None => return Err(SpaceError::Inconsistent(PICK_OUTSIDE_VIEW)),
            };
            names_record(txn, comm, &datum, source)
        })
    }

    /// Added by decision 102 (plan v4 C4.1): [`Cx::pick_data_order`] of the records of
    /// `channel` and the record in flight `extra`, which has the index of the count of the
    /// records (`canonical_data_order_with`). A VectorDB offer ranks its datum among the stored
    /// records with it.
    pub fn pick_data_order_with(
        &mut self,
        channel: &Par,
        extra: &ParDatum,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Vec<usize>, SpaceError> {
        let order = canonical_data_order_with(self.txn.data(channel)?, extra);
        self.pick_order(order, source, |txn, comm, index| {
            let datum = match txn.data(channel)?.get(index) {
                Some(datum) if comm.produces.contains(&datum.source) => datum.clone(),
                Some(_) => return Ok(false),
                None if comm.produces.contains(&extra.source) => {
                    return names_record(txn, comm, extra, source)
                }
                None => return Ok(false),
            };
            names_record(txn, comm, &datum, source)
        })
    }

    /// Added by decision 102 (plan v4 C4.1, rules CR-1 and CR-4): the indices of the waiting
    /// continuations of `channel` in the order of a pick of the operation `source`. Replay puts
    /// first the waiter whose source is the consume of the recorded COMM.
    pub fn pick_waiter_order(
        &mut self,
        channel: &Par,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Vec<usize>, SpaceError> {
        let order = self.canonical_waiter_order(channel)?;
        self.pick_order(order, source, |txn, comm, index| {
            names_waiter(txn, comm, std::slice::from_ref(channel), index)
        })
    }

    /// Added by decision 102 (plan v4 C4.1, rule CR-7): true when the recorded row of the
    /// operation `source` is a Stored row. In replay such a row names no candidate, so the
    /// pick site runs in DR-75 order, and a receive whose commit may veto parks without a
    /// search (E3-a). It is false in play and when no row gives an answer.
    pub fn recorded_stored(&mut self, source: RSpaceOperationSource<'_>) -> bool {
        matches!(self.txn.recorded_comms(source), Some(comms) if comms.is_empty())
    }

    /// Added by decision 124 (F2, base one row source for each produce,
    /// replay_rspace.rs:1058-1082): true when no recorded row of a produce of `others` comes before
    /// the recorded row of `first`. The caller checks first that the row of `first` is Stored
    /// ([`Cx::recorded_stored`]). Native replay compares the journal indices of the rows at the
    /// cursor (`SpaceTxn::recorded_row_index`). A source without a row, or whose lookup fails,
    /// comes after every row. Play reads no row, so it answers false.
    pub fn recorded_first(&mut self, first: RSpaceOperationSource<'_>, others: &[Produce]) -> bool {
        match self.txn.mode() {
            SpaceMode::Play => false,
            SpaceMode::NativeReplay => {
                let stored = self.txn.recorded_row_index(first);
                others.iter().all(|other| {
                    let row = self
                        .txn
                        .recorded_row_index(RSpaceOperationSource::Produce(other));
                    match (stored, row) {
                        (_, None) => true,
                        (None, Some(_)) => false,
                        (Some(stored_at), Some(row_at)) => row_at > stored_at,
                    }
                })
            }
        }
    }

    /// Added by decision 102 (plan v4 C4.1, rules CR-7 and R15): true when the commit of
    /// `continuation` may refuse a match in this space: its guard can veto, or the space has a
    /// theory ([`SpaceMatcher::may_veto`]).
    pub fn may_veto(&self, continuation: &TaggedContinuation) -> bool {
        self.plan
            .matcher
            .may_veto(continuation, self.plan.theory.is_some())
    }

    /// Added by decision 124 (L14, plan v4 C7.8, rule CR-20): true when the book holds the
    /// pair of `record` and `waiter`, so the pair does not fire again until a new arrival.
    pub fn booked(&self, record: &Produce, waiter: &Consume) -> bool {
        book_holds(&self.plan.book, record, waiter)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-20, errata V4-13, ruling RADD 1.2): the
    /// snapshot of the book that the staging reads (C2v3 4.4 (b)), for the tail read of a join
    /// row. The copy is one reference count.
    pub fn book(&self) -> Arc<BookPairs> { Arc::clone(&self.plan.book) }

    /// Added by decision 102 (plan v4 C4.1): the pick order of the DR-75 order `order` of the
    /// operation `source`, by the mode of the transaction. In play the `Random` order shuffles
    /// `order` once with the base play order, and it sets the pick flag when two or more
    /// candidates take part (CR-1, CR-14). The `Canonical` order keeps `order`. In replay a
    /// candidate that `named` names for a recorded COMM of `source` comes first, and the
    /// partition keeps the DR-75 order on both sides (CR-4). A Stored row, or no row, names no
    /// candidate.
    fn pick_order(
        &mut self,
        order: Vec<usize>,
        source: RSpaceOperationSource<'_>,
        named: impl Fn(&mut ParSpaceTxn, &COMM, usize) -> Result<bool, SpaceError>,
    ) -> Result<Vec<usize>, SpaceError> {
        match (self.txn.mode(), self.plan.order) {
            (SpaceMode::Play, PickMode::Random) => {
                if order.len() >= 2 {
                    self.plan.picked = true;
                }
                let mut shuffled = Vec::<usize>::with_capacity(order.len());
                shuffled.extend(play_order(order).into_iter().map(|(index, _)| index));
                Ok(shuffled)
            }
            (SpaceMode::Play, PickMode::Canonical) => Ok(order),
            (SpaceMode::NativeReplay, _) => {
                let comms = match self.txn.recorded_comms(source) {
                    Some(comms) if !comms.is_empty() => comms,
                    Some(_) | None => return Ok(order),
                };
                let mut ordered = Vec::<usize>::with_capacity(order.len());
                let mut rest = Vec::<usize>::with_capacity(order.len());
                for index in order {
                    let mut hit = false;
                    for comm in &comms {
                        if named(self.txn, comm, index)? {
                            hit = true;
                            break;
                        }
                    }
                    match hit {
                        true => ordered.push(index),
                        false => rest.push(index),
                    }
                }
                ordered.append(&mut rest);
                Ok(ordered)
            }
        }
    }

    /// The spatial match of `pattern` against `data`.
    pub fn bind(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
    ) -> Option<ListParWithRandom> {
        self.plan.matcher.bind(pattern, data)
    }

    // Changed by P7 step 4 (design v3 TH-6): every user match names its stored data, and the
    // theory of the space checks them after the guard.
    // /// The `where` guard of `continuation` over `matched`, metered (decision 37).
    // pub fn admits(
    //     &self,
    //     continuation: &TaggedContinuation,
    //     matched: &ListParWithRandom,
    // ) -> Result<bool, SpaceError> {
    //     self.plan.matcher.admits(continuation, matched)
    // }
    //
    // /// The `where` guard of `continuation` over the values `matched` of every bind, in bind
    // /// order, metered (decision 37: once, after every bind matched).
    // pub fn admits_all(
    //     &self,
    //     continuation: &TaggedContinuation,
    //     matched: &[ListParWithRandom],
    // ) -> Result<bool, SpaceError> {
    //     self.plan.matcher.admits_all(continuation, matched)
    // }
    //
    // /// The spatial match and then the guard: `None` when either refuses.
    // pub fn matches(
    //     &self,
    //     pattern: &BindPattern,
    //     data: &ListParWithRandom,
    //     continuation: &TaggedContinuation,
    // ) -> Result<Option<ListParWithRandom>, SpaceError> {
    //     self.plan.matcher.matches(pattern, data, continuation)
    // }

    /// True when the space has a theory (P7, TH-6). A site that must read a stored payload for
    /// the theory reads it only then, so a space without a theory does no work for it.
    pub fn has_theory(&self) -> bool { self.plan.theory.is_some() }

    /// Added by decision 124 (plan v4 C7.9, ruling B-1): the checker of the theory of the space,
    /// which a row search runs at the commit for an alternative (F12), or `None`.
    pub fn theory(&self) -> Option<Arc<dyn SpaceTheory>> { self.plan.theory.clone() }

    /// Added by decision 124 (plan v4 C7.9, ruling B-1, C4v2 G8): true when the theory of the
    /// space gives a lease (TH-9). A lease does not depend on its datum (L6 P1), so one query
    /// answers for every fire. A row search of such a space gives no alternative.
    pub fn leases(&self) -> bool {
        match &self.plan.theory {
            Some(theory) => matches!(theory.commit_lease(), Some(_)),
            None => false,
        }
    }

    /// Added by decision 124 (plan v4 C7.9, ruling B-1): true when a staged operation with the
    /// random state `random_state` is the job's own ([`SpaceJob::owns`]), as `fire_internal`
    /// decides it for a `K_retire`.
    pub fn owns_random_state(&self, random_state: &[u8]) -> bool {
        self.plan.job.owns(random_state)
    }

    /// Added by decision 124 (plan v4 C7.19, CR-24 bullet 3, ruling RADD 1.8): the read handle
    /// of the claims of the job, for a row search that checks an alternative at the commit.
    /// `None` without owner funding or without claims (RC2 D-10).
    pub fn seq_reads(&self) -> Option<SeqReads> {
        match self.plan.introduction.owner_funded() {
            false => None,
            true => self.plan.seq.as_ref().map(SeqClaims::reads),
        }
    }

    /// The `where` guard of `continuation` over `matched` (decision 37), and then the check of
    /// the theory of the space on each datum of `stored` (P7, TH-6). A refusal of
    /// either is a guard refusal.
    pub fn admits(
        &mut self,
        continuation: &TaggedContinuation,
        matched: &ListParWithRandom,
        stored: &[StoredMatch<'_>],
    ) -> Result<bool, SpaceError> {
        match self.plan.matcher.admits(continuation, matched) {
            true => self.theory_admits(stored),
            false => Ok(false),
        }
    }

    /// The `where` guard of `continuation` over the values `matched` of every bind, in bind
    /// order (decision 37: once, after every bind matched), and then the check of the theory of
    /// the space on each datum of `stored` (P7, TH-6).
    pub fn admits_all(
        &mut self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
        stored: &[StoredMatch<'_>],
    ) -> Result<bool, SpaceError> {
        match self.plan.matcher.admits_all(continuation, matched) {
            true => self.theory_admits(stored),
            false => Ok(false),
        }
    }

    /// The `where` guard of `continuation` over `matched` (decision 37), and then the check of
    /// the theory of the space on each stored record of `members`, with the values
    /// `matched` as the captures of each (P7, TH-6: each stored member of a list). `stored` gives
    /// the record of a stored member, or `None` for the element in flight, which its send
    /// admitted. The decode of each payload (`payload_of`) and its buffer happen only for a space
    /// with a theory and only after the guard admitted the match, so a
    /// space without a theory does no work for them.
    pub fn admits_records<T>(
        &mut self,
        continuation: &TaggedContinuation,
        matched: &ListParWithRandom,
        members: &[T],
        stored: impl Fn(&T) -> Option<&ListParWithRandom>,
        payload_of: fn(&ListParWithRandom) -> Result<ListParWithRandom, SpaceError>,
    ) -> Result<bool, SpaceError> {
        // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the theory part is shared with the
        // commit view (`members_theory_admits`). The text before (HEAD):
        // ```
        // if !self.plan.matcher.admits(continuation, matched) {
        //     return Ok(false);
        // }
        // if self.plan.theory.is_none() {
        //     return Ok(true);
        // }
        // let count = members
        //     .iter()
        //     .filter(|member| stored(member).is_some())
        //     .count();
        // let mut payloads = Vec::<ListParWithRandom>::with_capacity(count);
        // for record in members.iter().filter_map(&stored) {
        //     payloads.push(payload_of(record)?);
        // }
        // let mut checks = Vec::<StoredMatch<'_>>::with_capacity(payloads.len());
        // checks.extend(payloads.iter().map(|payload| StoredMatch {
        //     payload,
        //     captures: matched,
        // }));
        // self.theory_admits(&checks)
        // ```
        match self.plan.matcher.admits(continuation, matched) {
            false => Ok(false),
            true => {
                let theory = self.plan.theory.as_deref();
                members_theory_admits(theory, matched, members, stored, payload_of)
            }
        }
    }

    /// The spatial match of `pattern` against `data`, then the guard, then the check of the
    /// theory on `data` when it is `held` in the store (P7, TH-6): `None` when one refuses.
    pub fn matches(
        &mut self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
        continuation: &TaggedContinuation,
        held: Held,
    ) -> Result<Option<ListParWithRandom>, SpaceError> {
        let Some(matched) = self.plan.matcher.bind(pattern, data) else {
            return Ok(None);
        };
        let admitted = match held {
            Held::Stored => self.admits(continuation, &matched, &[StoredMatch {
                payload: data,
                captures: &matched,
            }])?,
            Held::InFlight => self.admits(continuation, &matched, &[])?,
        };
        Ok(admitted.then_some(matched))
    }

    /// TH-6: the check of the theory of the space on each datum of `stored`, in order. A verdict
    /// that is not `Proven` refuses the match. Changed by decision 124 (L6): the check reserves
    /// nothing (`Cx::lease_stored`). The text before: "and a `Proven` verdict adds its lease to
    /// the transaction (TH-9)". A space without a theory admits every match and does no work.
    // Changed by decision 124 (L6, base B:operation_context.rs:238-263): the check reads the
    // plan and reserves nothing, as `MatchAdmission` takes `&self`. The text before:
    // pub(crate) fn theory_admits(&mut self, stored: &[StoredMatch<'_>])
    // -> Result<bool, SpaceError> {
    pub(crate) fn theory_admits(&self, stored: &[StoredMatch<'_>]) -> Result<bool, SpaceError> {
        // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the check is shared with the
        // commit view (`theory_admits_with`). The text before (HEAD):
        // ```
        // let Some(theory) = self.plan.theory.clone() else {
        //     return Ok(true);
        // };
        // for datum in stored {
        //     if !theory
        //         .check_match(datum.payload, datum.captures)?
        //         .is_proven()
        //     {
        //         return Ok(false);
        //     }
        //     self.plan.leases.extend(theory.commit_lease());
        // }
        // Ok(true)
        // ```
        theory_admits_with(self.plan.theory.as_deref(), stored)
    }

    /// Added by decision 124 (L6, CR30 D-b, base B:ops_consume.rs:185-195): the fire that commits
    /// takes one lease for each of the `count` stored data that its check proved (TH-9). Call it
    /// once, after the last place that can drop the fire. `commit_lease` may give `None`, so the
    /// vector reserves nothing.
    pub fn lease_stored(&mut self, count: usize) {
        let Some(theory) = &self.plan.theory else {
            return;
        };
        for _ in 0..count {
            self.plan.leases.extend(theory.commit_lease());
        }
    }

    /// The head of the entries of one side: its index in `entries` and its order. The scan
    /// decodes each envelope (`ord_of`), and it copies nothing.
    pub(crate) fn scan_head<D: Discipline, T: Serialize>(
        discipline: &D,
        side: Side,
        entries: &[T],
        ord_of: fn(&T) -> Result<i64, SpaceError>,
    ) -> Result<Option<(usize, i64)>, SpaceError> {
        let mut ordered = Vec::<Ordered<'_, T>>::with_capacity(entries.len());
        for entry in entries {
            ordered.push(Ordered {
                ord: ord_of(entry)?,
                entry,
            });
        }
        let head = discipline.head(side, &ordered)?;
        Ok(head.and_then(|index| ordered.get(index).map(|entry| (index, entry.ord))))
    }

    /// The head waiting continuation of the collection in the staged view. The scan decodes each
    /// envelope, and only the head is copied.
    pub fn head_waiter<D: Discipline>(
        &mut self,
        discipline: &D,
    ) -> Result<Option<ParWaiter>, SpaceError> {
        let waiters = self
            .txn
            .continuations(std::slice::from_ref(self.plan.data))?;
        // Changed by decision 124 (plan v2 C7.10, decision 100, plan v4 C7.11, ruling RADD 1.4):
        // the head skips each inert machinery waiter, in `head_waiter_of`, which the settle views
        // share. The text before:
        // ```
        // let head = Self::scan_head(discipline, Side::Waiters, waiters, waiter_ord)?;
        // match head.and_then(|(index, _)| waiters.get(index)) {
        //     Some(waiter) => Ok(Some(waiter.clone())),
        //     None => Ok(None),
        // }
        // ```
        head_waiter_of(discipline, waiters)
    }

    /// The head record of the collection in the staged view, with its index in the view. The
    /// scan decodes each envelope, and only the head is copied.
    pub fn head_datum<D: Discipline>(
        &mut self,
        discipline: &D,
    ) -> Result<Option<(usize, ParDatum)>, SpaceError> {
        let records = self.txn.data(self.plan.data)?;
        let head = Self::scan_head(discipline, Side::Data, records, datum_ord)?;
        match head.and_then(|(index, _)| records.get(index).map(|datum| (index, datum))) {
            Some((index, datum)) => Ok(Some((index, datum.clone()))),
            None => Ok(None),
        }
    }

    /// The order of the head of `side` of the collection in the staged view, or `None` for an
    /// empty side. The scan copies nothing.
    pub fn head_ord<D: Discipline>(
        &mut self,
        discipline: &D,
        side: Side,
    ) -> Result<Option<i64>, SpaceError> {
        let data = self.plan.data;
        let head = match side {
            Side::Data => Self::scan_head(discipline, side, self.txn.data(data)?, datum_ord)?,
            Side::Waiters => Self::scan_head(
                discipline,
                side,
                self.txn.continuations(std::slice::from_ref(data))?,
                waiter_ord,
            )?,
        };
        Ok(head.map(|(_, ord)| ord))
    }

    /// The number of entries of `side` of the collection in the staged view.
    pub fn side_len(&mut self, side: Side) -> Result<usize, SpaceError> {
        let data = self.plan.data;
        Ok(match side {
            Side::Data => self.txn.data(data)?.len(),
            Side::Waiters => self.txn.continuations(std::slice::from_ref(data))?.len(),
        })
    }

    /// True when the element of an operation reaches the head of `side` of the collection. A
    /// fresh element (`reissued` is `None`) joins the tail, so it reaches the head only of an
    /// empty side. A re-issued persistent element with the first-round order `reissued`
    /// re-enters at its place (decision 43): at the head when its side is empty or its order
    /// precedes the order of the head. An element in flight is stored nowhere, so a stored entry
    /// with its order is `Inconsistent`.
    pub fn at_head<D: Discipline>(
        &mut self,
        discipline: &D,
        side: Side,
        reissued: Option<i64>,
    ) -> Result<bool, SpaceError> {
        let Some(ord) = reissued else {
            let queued = self.side_len(side)?;
            return Ok(discipline.reaches_head(side, queued));
        };
        let Some(head) = self.head_ord(discipline, side)? else {
            return Ok(true);
        };
        match ord == head {
            true => Err(SpaceError::Inconsistent(
                "a re-issued element shares its order with a stored entry",
            )),
            false => discipline.precedes(side, ord, head),
        }
    }

    /// Fails when `side` of the collection already holds the most entries of its bound: a cell
    /// holds one record and one waiter (F15, `CellOccupied`), and a VectorDB space bounds the
    /// records of each collection by its `capacity` (P6, `CapacityExceeded`). The check reads
    /// the size of the side and stages nothing, so the dropped transaction of a failed store or
    /// park applies nothing (decision 19, D6). A side without a bound does no work.
    pub fn check_capacity<D: Discipline>(&mut self, side: Side) -> Result<(), SpaceError> {
        // Replaced by P6 (design v3 4.0): the bound of a side may depend on the config of the
        // space, so the discipline gives it. The default bound is `D::CAPACITY` on both sides
        // with `CellOccupied`, so the check of every family of P5 does the same work.
        // let Some(capacity) = D::CAPACITY else {
        //     return Ok(());
        // };
        let Some(bound) = D::default().bound(self.plan.config, side)? else {
            return Ok(());
        };
        let held = self.side_len(side)?;
        match held >= bound.limit() {
            true => Err(bound.full()),
            false => Ok(()),
        }
    }

    /// The one-channel group of `channel`.
    fn channel_list(&self, channel: &Par) -> Vec<Par> {
        let mut channels = Vec::<Par>::with_capacity(1);
        channels.push(channel.clone());
        channels
    }

    /// The one-pattern list of `pattern`.
    fn pattern_list(&self, pattern: &BindPattern) -> Vec<BindPattern> {
        let mut patterns = Vec::<BindPattern>::with_capacity(1);
        patterns.push(pattern.clone());
        patterns
    }

    /// Appends `kind` to the staged list.
    // Changed by decision 124 (plan v4 C7.19 and C7.11, rules CR-24 and CR-31, ruling RADD 1.3):
    // a staged row that is not an anchor takes the claims since the last row, and each staged
    // row takes its B.6 class. The text before:
    // fn push_staged(&mut self, kind: Staged) { self.plan.staged.push(kind); }
    fn push_staged(&mut self, kind: Staged) {
        let anchor = matches!(kind, Staged::Anchor);
        self.plan.push_row(kind);
        match (anchor, self.plan.seq.as_mut()) {
            (true, _) | (false, None) => {}
            (false, Some(seq)) => seq.close_row(self.plan.staged.len() - 1),
        }
    }

    /// Added by decision 92: stages the anchor of the transaction (`spaces::anchor`) before the
    /// operation that stages next, when that operation is the first of the transaction and is
    /// not the job's own (`own`). So the first operation of every transaction is the job's own.
    fn anchor_before(&mut self, own: bool) -> Result<(), SpaceError> {
        match (own, self.plan.staged.is_empty()) {
            (false, true) => {
                anchor::stage(
                    self.txn,
                    self.plan.job,
                    self.plan.matcher,
                    self.plan.introduction,
                )?;
                self.push_staged(Staged::Anchor);
                Ok(())
            }
            (true, _) | (false, false) => Ok(()),
        }
    }

    /// Claims `channel` in the role `role` for a program job on a `seq` space (P7, SQ-3). Every
    /// other job claims nothing. A refused claim aborts the deploy with `SeqConcurrent` before
    /// the primitive stages anything.
    fn claim(&mut self, channel: &Par, role: ClaimRole) -> Result<(), SpaceError> {
        match self.plan.seq.as_mut() {
            Some(seq) => seq.claim(channel, role),
            None => Ok(()),
        }
    }

    /// Stages the store of `record` on the data channel of the collection, with the
    /// introduction of the operation.
    pub fn store(&mut self, record: ListParWithRandom, persist: bool) -> Result<(), SpaceError> {
        let channel = self.plan.data;
        let introduction = self.plan.introduction;
        self.store_on(channel, record, persist, introduction)
    }

    /// Stages the store of `record` on `channel` with the introduction `introduction`: a record
    /// of the collection, a PathMap marker (P5e), or a re-store with its record's own authority
    /// (decision 45, Rule E).
    pub fn store_on(
        &mut self,
        channel: &Par,
        record: ListParWithRandom,
        persist: bool,
        introduction: &Arc<Introduction>,
    ) -> Result<(), SpaceError> {
        self.store_searched(channel, record, persist, introduction, None)
    }

    /// Added by decision 124 (errata V4-17, rule CR-7): [`Cx::store_on`] with the row search
    /// `search` of an E3-a store, or `None` for every other store.
    pub fn store_searched(
        &mut self,
        channel: &Par,
        record: ListParWithRandom,
        persist: bool,
        introduction: &Arc<Introduction>,
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        self.claim(channel, ClaimRole::Data)?;
        // Added by decision 92: the anchor goes first, so the mirror index below counts it.
        let own = self.plan.job.owns(&record.random_state);
        self.anchor_before(own)?;
        if self.plan.mirror.is_active() {
            let index = self.plan.staged.len();
            self.plan.mirror.store(index, channel, &record, persist);
        }
        // Changed by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the request
        // comes from `store_request`, which a finish row shares (P1). The text before:
        // ```
        // let op = SpaceProduce {
        //     channel: channel.clone(),
        //     data: record,
        //     persist,
        //     outcome: ProduceOutcome::Store,
        //     before_observe: Some(record::register_introduction(introduction)),
        // };
        // ```
        let op = SpaceProduce {
            search,
            ..store_request(channel, record, persist, introduction, own)
        };
        // Changed by decision 90 (decision 69): Option P is gone, so the operation stages directly.
        // self.plan.legacy.produce(self.txn, op)?;
        self.txn.produce(op)?;
        self.push_staged(Staged::Store);
        Ok(())
    }

    /// Stages the park of the continuation of the receive `op` on the data channel of the
    /// collection.
    pub fn park(
        &mut self,
        op: &ConsumeOp,
        continuation: TaggedContinuation,
        peeks: BTreeSet<i32>,
    ) -> Result<(), SpaceError> {
        let channels = self.channel_list(self.plan.data);
        let patterns = self.pattern_list(&op.pattern);
        // Changed by decision 124 (plan v4 C7.9, ruling B-1): a plain park carries no search.
        // The text before:
        // self.park_on(channels, patterns, continuation, op.persistent, peeks)
        self.park_on(channels, patterns, continuation, op.persistent, peeks, None)
    }

    /// Added by decision 102 (plan v4 C5.1, C5.6): the identity of the consume that the receive
    /// `op` with the continuation `continuation` stages on the data channel of the collection.
    /// Its park ([`Cx::park`]) and its one-bind fire have this source. A pick and the E3-a check
    /// of the receive read the recorded row of this source (rules CR-4 and CR-7).
    pub fn receive_source(&self, op: &ConsumeOp, continuation: &TaggedContinuation) -> Consume {
        Consume::create(
            &self.channel_list(self.plan.data),
            &self.pattern_list(&op.pattern),
            continuation,
            op.persistent,
        )
    }

    /// Added by decision 124 (plan v4 C7.9 and C7.11, rulings D-1 and RADD 1.4): the row search of
    /// a head offer on `channel` of a collection of the discipline `D`, in which the datum `record`
    /// of the produce `source` fires the waiter of `fire` after `fires_before` fires of the
    /// transaction. It lists the staged tuple only (CR-2). None without owner funding (ruling
    /// D-10).
    pub fn head_offer_search<D: Discipline>(
        &self,
        channel: &Par,
        fire: &OfferFire,
        source: &Produce,
        record: &ListParWithRandom,
        fires_before: usize,
    ) -> Option<ParRowSearch> {
        match self.plan.introduction.owner_funded() {
            true => {
                let candidate = incoming_candidate(
                    channel.clone(),
                    source.clone(),
                    record.clone(),
                    fire.persistent,
                    fire.matched.clone(),
                );
                let search = StagedRowSearch::<D>::of_offer(
                    self,
                    &fire.waiter,
                    channel,
                    candidate,
                    fires_before,
                );
                Some(Box::new(search))
            }
            false => None,
        }
    }

    /// Added by decision 124 (plan v4 C7.9 and C7.11, rulings D-1 and RADD 1.4): the row search of
    /// a head receive of a collection of the discipline `D`, in which the receive `op` with
    /// `continuation` binds the record of `fire` after `fires_before` fires of the transaction.
    /// It lists the staged tuple only (CR-2). None without owner funding (ruling D-10).
    pub fn head_receive_search<D: Discipline>(
        &self,
        op: &ConsumeOp,
        continuation: &TaggedContinuation,
        fire: &ReceiveFire,
        fires_before: usize,
    ) -> Option<ParRowSearch> {
        match self.plan.introduction.owner_funded() {
            true => {
                let source = self.receive_source(op, continuation);
                let candidate = receive_candidate(self.plan.data.clone(), ReceiveFire {
                    head: fire.head.clone(),
                    matched: fire.matched.clone(),
                    view_index: fire.view_index,
                });
                Some(Box::new(StagedRowSearch::<D>::of_receive(
                    self,
                    &source,
                    continuation,
                    op.persistent,
                    self.plan.data,
                    candidate,
                    fires_before,
                )))
            }
            false => None,
        }
    }

    /// Stages the park of `continuation` on the channel group `channels` with `patterns`, with
    /// the introduction of the operation: a waiter on a prefix (P5e) or on a join (P5c). Added
    /// by decision 124 (plan v4 C7.9, errata V4-17, ruling B-1): `search` is the row search of
    /// an E3-a park, which the kind code builds, or None.
    // Changed by decision 124 (plan v4 C7.9, ruling B-1): the park takes its search. The text
    // before:
    // ```
    // pub fn park_on(
    //     &mut self,
    //     channels: Vec<Par>,
    //     patterns: Vec<BindPattern>,
    //     continuation: TaggedContinuation,
    //     persist: bool,
    //     peeks: BTreeSet<i32>,
    // ) -> Result<(), SpaceError> {
    // ```
    pub fn park_on(
        &mut self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persist: bool,
        peeks: BTreeSet<i32>,
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        for channel in &channels {
            self.claim(channel, ClaimRole::Waiter)?;
        }
        // Added by decision 92.
        let own = self.plan.job.owns_continuation(&continuation);
        self.anchor_before(own)?;
        let staged = SpaceConsume {
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            outcome: ConsumeOutcome::Park,
            before_observe: Some(record::register_introduction(self.plan.introduction)),
            own,
            // Added by decision 124 (plan v4 C7.9, errata V4-17): the search of an E3-a park.
            search,
        };
        // Changed by decision 90 (decision 69): Option P is gone, so the operation stages directly.
        // self.plan.legacy.consume(self.txn, staged)?;
        self.txn.consume(staged)?;
        self.push_staged(Staged::Park);
        Ok(())
    }

    /// Stages the re-store of a record that a consume-side peek removed (decision 45, Rule E) on
    /// the data channel of the collection: the same record bytes, so the same produce identity,
    /// stored again as a non-persistent record, as the host reducer's `produce_peeks` sends a
    /// peeked datum again. The staged produce registers the record's own cost authority, and
    /// the operation bumps no counter for it.
    pub fn restore(&mut self, record: ListParWithRandom) -> Result<(), SpaceError> {
        let channel = self.plan.data;
        self.restore_on(channel, record)
    }

    /// [`Cx::restore`] on `channel`: a re-stored PathMap marker (P5e) or a record of a join
    /// (P5c).
    pub fn restore_on(
        &mut self,
        channel: &Par,
        record: ListParWithRandom,
    ) -> Result<(), SpaceError> {
        let introduction = self.plan.introduction.for_record(&record)?;
        self.store_on(channel, record, false, &introduction)
    }

    /// Stages a produce-side fire on the data channel of the collection, with the introduction
    /// of the operation: `data` is the record of the staged produce, `removed` the record of the
    /// incoming candidate and `source` the identity of the produce.
    pub fn fire_offer(
        &mut self,
        fire: OfferFire,
        data: ListParWithRandom,
        removed: ListParWithRandom,
        source: Produce,
    ) -> Result<(), SpaceError> {
        let channel = self.plan.data;
        let introduction = self.plan.introduction;
        self.fire_offer_on(channel, fire, data, removed, source, introduction)
    }

    /// [`Cx::fire_offer`] on `channel` with the introduction `introduction`: a re-offer of a
    /// lifted or restored record keeps the identity and the authority of that record (P5b, F2).
    /// Added by decision 102 (plan v4 C7.22): a re-offer of a stored persistent record
    /// ([`Offered::StoredPersistent`]) is not the job's own, so as the first operation it stages
    /// the anchor first (rule CR-19 case 3).
    pub fn fire_offer_on(
        &mut self,
        channel: &Par,
        fire: OfferFire,
        data: ListParWithRandom,
        removed: ListParWithRandom,
        source: Produce,
        introduction: &Arc<Introduction>,
    ) -> Result<(), SpaceError> {
        self.claim(channel, ClaimRole::Waiter)?;
        // Added by decision 92: a re-offer of a record keeps the random state of that record.
        // Changed by decision 102 (plan v4 C7.22, F22): a re-offer of a stored persistent record
        // keeps the source of the record, which every re-offer of the record shares, so it is
        // not the job's own.
        let own = self
            .plan
            .job
            .owns_first_reoffer(&data.random_state, fire.offered);
        self.anchor_before(own)?;
        // Added by decision 124 (plan v2 C7.13, F7): a fire whose row deviates can store its
        // datum, so the mirror captures the datum at the staged index of the fire. A capture
        // of a row that commits as a fire writes nothing (`MirrorCapture::writes`).
        match self.plan.mirror.is_active() {
            false => {}
            true => {
                let index = self.plan.staged.len();
                self.plan
                    .mirror
                    .store(index, channel, &data, fire.persistent);
            }
        }
        let channels = self.channel_list(channel);
        let mut candidates = Vec::<ParCandidate>::with_capacity(1);
        candidates.push(incoming_candidate(
            channel.clone(),
            source,
            removed,
            fire.persistent,
            fire.matched,
        ));
        let op = SpaceProduce {
            channel: channel.clone(),
            data,
            persist: fire.persistent,
            outcome: ProduceOutcome::Fire {
                channels,
                continuation: fire.waiter,
                data: candidates,
            },
            before_observe: Some(record::register_introduction(introduction)),
            own,
            // Added by decision 124 (plan v4 C7.9, ruling D-1): the fire carries the row search
            // that its kind code built.
            search: fire.search,
        };
        // Changed by decision 90 (decision 69): Option P is gone, so the operation stages directly.
        // self.plan.legacy.produce(self.txn, op)?;
        self.txn.produce(op)?;
        self.push_staged(Staged::Fire);
        Ok(())
    }

    /// Stages a consume-side fire of the receive `op` over one record of the collection, with
    /// the parked continuation `continuation` and the peek set `peeks`. The record is the plan's
    /// own copy, so it moves into the candidate. Added by decision 124 (plan v4 C7.9, ruling
    /// D-1): the fire carries the row search `search` that its kind code built.
    pub fn fire_receive(
        &mut self,
        op: &ConsumeOp,
        fire: ReceiveFire,
        continuation: TaggedContinuation,
        peeks: BTreeSet<i32>,
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        let channels = self.channel_list(self.plan.data);
        let patterns = self.pattern_list(&op.pattern);
        let mut candidates = Vec::<ParCandidate>::with_capacity(1);
        candidates.push(receive_candidate(self.plan.data.clone(), fire));
        // Changed by decision 124 (plan v4 C7.9, ruling D-1): the group step takes the staged
        // consume with its search. The text before:
        // ```
        // self.fire_receive_group(
        //     channels,
        //     patterns,
        //     continuation,
        //     op.persistent,
        //     peeks,
        //     candidates,
        //     1,
        // )
        // ```
        let own = self.plan.job.owns_continuation(&continuation);
        let staged = SpaceConsume {
            channels,
            patterns,
            continuation,
            persist: op.persistent,
            peeks,
            outcome: ConsumeOutcome::Fire { data: candidates },
            before_observe: Some(record::register_introduction(self.plan.introduction)),
            own,
            search,
        };
        self.stage_receive_group(staged, 1)
    }

    /// Stages a consume-side fire of `continuation` over the channel group `channels` with the
    /// candidates `candidates`, one for each channel, in group order. The outcome dispatches the
    /// first `binds` results, and a row with fewer binds than candidates stages as `FirePrefix`.
    /// Changed by decision 101 (rule CR-13): a set receive and a VectorDB list receive bind one
    /// member over `[D]`, and `K_retire` rows retire the other members, so no plan of a native
    /// space stages a prefix now. The text before: "The outcome dispatches the first `binds`
    /// results: a set consume over `[D; n]` binds one member and retires the other members of
    /// its class with the same COMM (F3, F20)."
    #[allow(clippy::too_many_arguments)]
    pub fn fire_receive_group(
        &mut self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persist: bool,
        peeks: BTreeSet<i32>,
        candidates: Vec<ParCandidate>,
        binds: usize,
        // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search of the fire,
        // which the kind code built, or None without owner funding (RC2 D-10).
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        // Changed by decision 124 (plan v4 C7.9, rulings D-1 and RADD 1.6): the checks, the
        // claims, the anchor and the stage moved to `stage_receive_group`. The fire carries the
        // row search `search` of its kind code. The text before:
        // ```
        // let width = candidates.len();
        // if channels.len() != width || patterns.len() != width || binds == 0 || binds > width {
        //     return Err(SpaceError::Inconsistent(
        //         "a fire group does not line up its channels, patterns, candidates and binds",
        //     ));
        // }
        // for channel in &channels {
        //     self.claim(channel, ClaimRole::Waiter)?;
        // }
        // for candidate in &candidates {
        //     self.claim(&candidate.channel, ClaimRole::Data)?;
        // }
        // let staged = SpaceConsume {
        //     channels,
        //     patterns,
        //     continuation,
        //     persist,
        //     peeks,
        //     outcome: ConsumeOutcome::Fire { data: candidates },
        //     before_observe: Some(record::register_introduction(self.plan.introduction)),
        // };
        // self.plan.legacy.consume(self.txn, staged)?;
        // match binds == width {
        //     true => {
        //         self.push_staged(Staged::Fire);
        //         Ok(())
        //     }
        //     false => {
        //         self.plan.prefixes.push(binds);
        //         self.push_staged(Staged::FirePrefix);
        //         Ok(())
        //     }
        // }
        // ```
        let own = self.plan.job.owns_continuation(&continuation);
        let staged = SpaceConsume {
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            outcome: ConsumeOutcome::Fire { data: candidates },
            before_observe: Some(record::register_introduction(self.plan.introduction)),
            own,
            search,
        };
        self.stage_receive_group(staged, binds)
    }

    /// Added by decision 124 (plan v4 C7.9, ruling D-1): the group step of a consume-side fire,
    /// which [`Cx::fire_receive_group`] and [`Cx::fire_receive`] share. It checks the group,
    /// claims its channels, stages the anchor first when the fire is not the job's own, and
    /// stages the fire.
    fn stage_receive_group(
        &mut self,
        staged: SpaceConsume<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        binds: usize,
    ) -> Result<(), SpaceError> {
        let ConsumeOutcome::Fire { data: candidates } = &staged.outcome else {
            return Err(SpaceError::Inconsistent("a fire group stages no fire"));
        };
        let width = candidates.len();
        if staged.channels.len() != width
            || staged.patterns.len() != width
            || binds == 0
            || binds > width
        {
            return Err(SpaceError::Inconsistent(
                "a fire group does not line up its channels, patterns, candidates and binds",
            ));
        }
        for channel in &staged.channels {
            self.claim(channel, ClaimRole::Waiter)?;
        }
        for candidate in candidates {
            self.claim(&candidate.channel, ClaimRole::Data)?;
        }
        self.anchor_before(staged.own)?;
        self.txn.consume(staged)?;
        match binds == width {
            true => {
                self.push_staged(Staged::Fire);
                Ok(())
            }
            false => {
                self.plan.prefixes.push(binds);
                self.push_staged(Staged::FirePrefix);
                Ok(())
            }
        }
    }

    /// Stages the retirement of stored members of a set class (P5a, F3): a consume fire of
    /// `continuation` over the channel group `channels` with the stored candidates `candidates`,
    /// one for each channel. The SET discipline builds the `K_retire` shape. The fire is never
    /// dispatched and is not a fire round.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the retire names the discipline
    // `D` of its collection, which its machinery search keeps. The text before:
    // pub fn retire(
    pub fn retire<D: Discipline>(
        &mut self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        candidates: Vec<ParCandidate>,
    ) -> Result<(), SpaceError> {
        // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the fire names `D`. The
        // text before:
        // self.fire_internal(Staged::Retire, channels, patterns, continuation, candidates)
        self.fire_internal::<D>(Staged::Retire, channels, patterns, continuation, candidates)
    }

    /// Stages the lift of a stored record out of its collection during a settle (P5b, F2): a
    /// consume fire of `continuation` over `channels` with the stored candidates `candidates`.
    /// The STACK discipline builds the `K_lift` shape. The fire is never dispatched.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the lift names the discipline
    // `D` of its collection, which its machinery search keeps. The text before:
    // pub fn lift(
    pub fn lift<D: Discipline>(
        &mut self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        candidates: Vec<ParCandidate>,
    ) -> Result<(), SpaceError> {
        // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the fire names `D`. The
        // text before:
        // self.fire_internal(Staged::Lift, channels, patterns, continuation, candidates)
        self.fire_internal::<D>(Staged::Lift, channels, patterns, continuation, candidates)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-16, ruling RADD2v2 6.4): the lift of
    /// [`Cx::lift`] with the row search `search` that the caller built. The join engine gives
    /// its cascade lift the lift search of its kind (`JoinSource::lift_search`), which ends
    /// Tail(actual) when the commit refuses the lift. `None` without owner funding (ruling D-10).
    pub fn lift_with(
        &mut self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        candidates: Vec<ParCandidate>,
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        self.fire_internal_with(
            Staged::Lift,
            channels,
            patterns,
            continuation,
            candidates,
            search,
        )
    }

    /// Added by decision 124 (plan v2 C7.5, F5): records the pair `pair` of the lift that
    /// was staged last, for the book of a refused lift (CR-20).
    pub fn record_lift(&mut self, pair: BookPair) {
        let at = self.plan.staged.len().saturating_sub(1);
        self.plan.lifts.push((at, pair));
    }

    /// Stages a non-persistent consume fire that removes stored records and is never
    /// dispatched, with the introduction of the operation.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the fire names the discipline
    // `D` of its collection, which its machinery search keeps. The text before:
    // fn fire_internal(
    fn fire_internal<D: Discipline>(
        &mut self,
        kind: Staged,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        candidates: Vec<ParCandidate>,
    ) -> Result<(), SpaceError> {
        // Changed by decision 124 (plan v4 C7.11, ruling RADD2v2 6.4): the body moved to
        // `fire_internal_with`, which the lift of the join engine calls with the search of its
        // kind (`Cx::lift_with`). This fire gives it the machinery search. The text before:
        // ```
        // let width = candidates.len();
        // if width == 0 || channels.len() != width || patterns.len() != width {
        //     return Err(SpaceError::Inconsistent(
        //         "an internal fire does not line up its channels, patterns and candidates",
        //     ));
        // }
        // // SQ-3: a retirement or a lift removes stored records, so it claims their data
        // // channels. A removed PathMap marker claims nothing.
        // for candidate in &candidates {
        //     self.claim(&candidate.channel, ClaimRole::Data)?;
        // }
        // let staged = SpaceConsume {
        //     channels,
        //     patterns,
        //     continuation,
        //     persist: false,
        //     peeks: BTreeSet::new(),
        //     outcome: ConsumeOutcome::Fire { data: candidates },
        //     before_observe: Some(record::register_introduction(self.plan.introduction)),
        // };
        // self.plan.legacy.consume(self.txn, staged)?;
        // self.push_staged(kind);
        // Ok(())
        // ```
        // Added by decision 124 (plan v4 C7.9 and C7.11, V4-5, ruling RADD2 1.6): an
        // owner-funded machinery row carries the machinery search of the discipline `D`, so the
        // commit can park a refused row inert (CR-16). It is built only under owner funding
        // (ruling D-10).
        let search: Option<ParRowSearch> = match self.plan.introduction.owner_funded() {
            true => Some(Box::new(StagedRowSearch::<D>::of_machinery(
                &channels,
                &candidates,
            ))),
            false => None,
        };
        self.fire_internal_with(kind, channels, patterns, continuation, candidates, search)
    }

    /// Added by decision 124 (plan v4 C7.11, ruling RADD2v2 6.4): stages a non-persistent consume
    /// fire that removes stored records and is never dispatched, with the introduction of the
    /// operation and the row search `search` of the row: the machinery search of
    /// `fire_internal`, or the lift search of the join engine (`Cx::lift_with`). `None` without
    /// owner funding (ruling D-10). It holds the body of `fire_internal`, whose signature stays
    /// (RADD section 2).
    fn fire_internal_with(
        &mut self,
        kind: Staged,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        candidates: Vec<ParCandidate>,
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        let width = candidates.len();
        match width == 0 || channels.len() != width || patterns.len() != width {
            false => {}
            true => {
                return Err(SpaceError::Inconsistent(
                    "an internal fire does not line up its channels, patterns and candidates",
                ))
            }
        }
        // SQ-3: a retirement or a lift removes stored records, so it claims their data
        // channels. A removed PathMap marker claims nothing.
        for candidate in &candidates {
            self.claim(&candidate.channel, ClaimRole::Data)?;
        }
        // Added by decision 92: a lift of a settle or a plan of the job derives its random state
        // from the job. A retirement derives it from the element that retires the class.
        let own = match kind {
            Staged::Lift => self.plan.job.owns_lift(&continuation),
            _ => self.plan.job.owns_continuation(&continuation),
        };
        // Added by decision 124 (plan v2 C7.5, rule CR-21): a lift peeks its one channel.
        let peeks = match kind {
            Staged::Lift => BTreeSet::from([0]),
            _ => BTreeSet::new(),
        };
        self.anchor_before(own)?;
        let staged = SpaceConsume {
            channels,
            patterns,
            continuation,
            persist: false,
            // Changed by decision 124 (plan v2 C7.5, rule CR-21): the peeks of the row.
            // peeks: BTreeSet::new(),
            peeks,
            outcome: ConsumeOutcome::Fire { data: candidates },
            before_observe: Some(record::register_introduction(self.plan.introduction)),
            own,
            // Added by decision 124 (plan v4 C7.9 and C7.11, V4-5, ruling RADD2v2 6.4): the row
            // search of the row.
            search,
        };
        // Changed by decision 90 (decision 69): Option P is gone, so the operation stages directly.
        // self.plan.legacy.consume(self.txn, staged)?;
        self.txn.consume(staged)?;
        self.push_staged(kind);
        Ok(())
    }

    /// The re-issue of `op` with the cause `cause`: a plan that ends at the round cap (I-21) or
    /// at a divergent pair (A10) runs the rest as a new `Intent::Space`.
    pub fn reissue(&self, op: SpaceOp, cause: ReissueCause) -> SpaceReissue {
        SpaceReissue { op, cause }
    }

    // ── The primitives of the join engine (P5c) ───────────────────────────────────────────

    /// The join groups of `channel` in the staged view.
    pub fn joins_on(&mut self, channel: &Par) -> Result<&[Vec<Par>], SpaceError> {
        Ok(self.txn.joins(channel)?)
    }

    /// The waiting continuations of the join group `group` in the staged view. The lock keys
    /// of the operation must hold every channel of the group.
    pub fn group_waiters(&mut self, group: &[Par]) -> Result<&[ParWaiter], SpaceError> {
        Ok(self.txn.continuations(group)?)
    }

    /// The indices of the waiting continuations of the join group `group` in the canonical
    /// order of DR-75, the base of the replay order (decision 102).
    /// Added by decision 102 (plan v4 C5.7): kept DR-75, because it defines the order.
    pub fn canonical_group_order(&mut self, group: &[Par]) -> Result<Vec<usize>, SpaceError> {
        Ok(canonical_continuation_order(self.txn.continuations(group)?))
    }

    /// Added by decision 102 (plan v4 C4.1, rules CR-1 and CR-4): the indices of the waiting
    /// continuations of the join group `group` in the order of a pick of the operation
    /// `source`, as [`Cx::pick_waiter_order`] gives them for one channel.
    pub fn pick_group_order(
        &mut self,
        group: &[Par],
        source: RSpaceOperationSource<'_>,
    ) -> Result<Vec<usize>, SpaceError> {
        let order = self.canonical_group_order(group)?;
        self.pick_order(order, source, |txn, comm, index| {
            names_waiter(txn, comm, group, index)
        })
    }

    /// Added by decision 124 (LP-1, L5; J1 ruling 3): the positions in `union` in the order of a
    /// pick of the operation `source`. `union` lists the waiters of the join groups `groups` of
    /// one channel as (group position, waiter index) pairs, in the canonical order of DR-75 of
    /// their union. `groups` is in the order of `joins_on`. In play each group gives its waiters
    /// in the order of [`Cx::pick_group_order`], and the groups keep their order, as the base
    /// meets each join group in turn and shuffles its waiters (ops_produce.rs:380-417). A waiter
    /// of a group that `union` does not list is not a candidate. In replay the waiter that a
    /// recorded COMM of `source` names comes first over `union`, and the others keep the order
    /// of `union` (CR-4).
    pub fn pick_union_order(
        &mut self,
        groups: &[Vec<Par>],
        union: &[(usize, usize)],
        source: RSpaceOperationSource<'_>,
    ) -> Result<Vec<usize>, SpaceError> {
        match self.txn.mode() {
            SpaceMode::Play => {
                let mut slots = Vec::<Vec<Option<usize>>>::with_capacity(groups.len());
                for group in groups {
                    let count = self.txn.continuations(group)?.len();
                    slots.push(vec![None; count]);
                }
                for (position, &(group, index)) in union.iter().enumerate() {
                    match slots.get_mut(group).and_then(|slots| slots.get_mut(index)) {
                        Some(slot) => *slot = Some(position),
                        None => return Err(SpaceError::Inconsistent(PICK_OUTSIDE_VIEW)),
                    }
                }
                let mut order = Vec::<usize>::with_capacity(union.len());
                for (group, slots) in groups.iter().zip(&slots) {
                    for index in self.pick_group_order(group, source)? {
                        match slots.get(index) {
                            Some(Some(position)) => order.push(*position),
                            Some(None) => continue,
                            None => return Err(SpaceError::Inconsistent(PICK_OUTSIDE_VIEW)),
                        }
                    }
                }
                Ok(order)
            }
            SpaceMode::NativeReplay => {
                let mut order = Vec::<usize>::with_capacity(union.len());
                order.extend(0..union.len());
                self.pick_order(order, source, |txn, comm, position| {
                    match union
                        .get(position)
                        .and_then(|&(group, index)| Some((groups.get(group)?, index)))
                    {
                        Some((group, index)) => names_waiter(txn, comm, group, index),
                        None => Err(SpaceError::Inconsistent(PICK_OUTSIDE_VIEW)),
                    }
                })
            }
        }
    }

    /// Bumps the counters `counters` of a collection that the transaction touches (decision
    /// 31): the staged operations of the bump of the collection of the operation. It returns
    /// the value of the collection counter before the bump.
    pub fn bump_on(
        &mut self,
        counters: CounterChannels<'_>,
        random: &Blake2b512Random,
        authority: OperationAuthority<'_>,
    ) -> Result<i64, SpaceError> {
        stage_bump(self.txn, self.plan, counters, random, authority)
    }

    /// Stages a produce-side fire of the group `channels`: the datum `data` on `channel` fires
    /// `waiter` with `candidates`, one for each channel of the group in group order, of which
    /// exactly one is the incoming candidate (`incoming_candidate`). A group of two or more
    /// channels stages a `JoinFire`, and a group of one channel a `Fire`. Added by decision 102
    /// (plan v4 C7.22): `offered` says whether `data` is in flight or a stored persistent record
    /// that a cascade re-offers ([`Cx::fire_offer_on`]).
    #[allow(clippy::too_many_arguments)]
    pub fn fire_offer_group(
        &mut self,
        channel: &Par,
        data: ListParWithRandom,
        persist: bool,
        waiter: ParWaiter,
        channels: Vec<Par>,
        candidates: Vec<ParCandidate>,
        introduction: &Arc<Introduction>,
        offered: Offered,
        // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search of the fire,
        // which the join engine built, or None without owner funding (RC2 D-10).
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        let width = candidates.len();
        if width == 0 || channels.len() != width {
            return Err(SpaceError::Inconsistent(
                "a group fire does not line up its channels and candidates",
            ));
        }
        let kind = match width {
            1 => Staged::Fire,
            _ => Staged::JoinFire,
        };
        // Added by decision 91 (QF-15): a group fire uses the channels of the waiter and the
        // channels of its candidates, so it claims them as `fire_offer_on` and
        // `fire_receive_group` do. Without the claims, a send that fires the waiter of a join or
        // of a PathMap prefix under racing evaluation missed a concurrent use of that channel
        // (8 percent of the schedules of `reducer_seq_atomic_guard` for `pathmap:queue`).
        for waiter_channel in &channels {
            self.claim(waiter_channel, ClaimRole::Waiter)?;
        }
        for candidate in &candidates {
            self.claim(&candidate.channel, ClaimRole::Data)?;
        }
        // Added by decision 92.
        // Changed by decision 102 (plan v4 C7.22, F22): as in `fire_offer_on`.
        let own = self
            .plan
            .job
            .owns_first_reoffer(&data.random_state, offered);
        self.anchor_before(own)?;
        // Added by decision 124 (plan v2 C7.13, F7): a join fire whose row deviates can store its
        // datum, so the mirror captures the datum at the staged index of the fire. A capture of a
        // row that commits as a fire writes nothing (`MirrorCapture::writes`).
        match self.plan.mirror.is_active() {
            false => {}
            true => {
                let index = self.plan.staged.len();
                self.plan.mirror.store(index, channel, &data, persist);
            }
        }
        let op = SpaceProduce {
            channel: channel.clone(),
            data,
            persist,
            outcome: ProduceOutcome::Fire {
                channels,
                continuation: waiter,
                data: candidates,
            },
            before_observe: Some(record::register_introduction(introduction)),
            own,
            // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search of the fire.
            search,
        };
        // Changed by decision 90 (decision 69): Option P is gone, so the operation stages directly.
        // self.plan.legacy.produce(self.txn, op)?;
        self.txn.produce(op)?;
        self.push_staged(kind);
        Ok(())
    }

    /// Stages a consume-side fire of a join waiter: `continuation`, a record of version 2, over
    /// the group `channels` with `patterns` and the stored candidates `candidates`, one for each
    /// channel in group order, with the introduction of the operation. It stages a `JoinFire`.
    // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search is one more input,
    // in the form of its two siblings and of `run_plan`.
    #[allow(clippy::too_many_arguments)]
    pub fn fire_join(
        &mut self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persist: bool,
        peeks: BTreeSet<i32>,
        candidates: Vec<ParCandidate>,
        // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search of the fire,
        // which the join engine built, or None without owner funding (RC2 D-10).
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        let width = candidates.len();
        if width < 2 || channels.len() != width || patterns.len() != width {
            return Err(SpaceError::Inconsistent(
                "a join fire does not line up its channels, patterns and candidates",
            ));
        }
        // Added by decision 91 (QF-15): a join fire uses the channels of its group and of its
        // candidates, so it claims them as `fire_receive_group` does.
        for channel in &channels {
            self.claim(channel, ClaimRole::Waiter)?;
        }
        for candidate in &candidates {
            self.claim(&candidate.channel, ClaimRole::Data)?;
        }
        // Added by decision 92.
        let own = self.plan.job.owns_continuation(&continuation);
        self.anchor_before(own)?;
        let staged = SpaceConsume {
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            outcome: ConsumeOutcome::Fire { data: candidates },
            before_observe: Some(record::register_introduction(self.plan.introduction)),
            own,
            // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search of the fire.
            search,
        };
        // Changed by decision 90 (decision 69): Option P is gone, so the operation stages directly.
        // self.plan.legacy.consume(self.txn, staged)?;
        self.txn.consume(staged)?;
        self.push_staged(Staged::JoinFire);
        Ok(())
    }
}

/// Added by decision 124 (plan v4 C7.9, ruling OP-J of RDOJ 1): the check of the theory
/// `theory` on each datum of `stored` (P7, TH-6), which the staging and the commit view share. A
/// space without a theory admits every match and does no work. The check reserves nothing.
fn theory_admits_with(
    theory: Option<&dyn SpaceTheory>,
    stored: &[StoredMatch<'_>],
) -> Result<bool, SpaceError> {
    let Some(theory) = theory else {
        return Ok(true);
    };
    for datum in stored {
        match theory
            .check_match(datum.payload, datum.captures)?
            .is_proven()
        {
            true => {}
            false => return Ok(false),
        }
    }
    Ok(true)
}

/// Added by decision 124 (plan v4 C7.9, ruling OP-J of RDOJ 1): the check of the theory `theory`
/// on each stored member of a list (P7, TH-6), with the values `matched` of the list as the
/// captures of each. `stored` gives the record of a stored member, or `None` for the element in
/// flight. The staging and the commit view share it. A space without a theory decodes nothing.
fn members_theory_admits<T>(
    theory: Option<&dyn SpaceTheory>,
    matched: &ListParWithRandom,
    members: &[T],
    stored: impl Fn(&T) -> Option<&ListParWithRandom>,
    payload_of: fn(&ListParWithRandom) -> Result<ListParWithRandom, SpaceError>,
) -> Result<bool, SpaceError> {
    let Some(_) = theory else {
        return Ok(true);
    };
    let count = members
        .iter()
        .filter(|member| stored(member).is_some())
        .count();
    let mut payloads = Vec::<ListParWithRandom>::with_capacity(count);
    for record in members.iter().filter_map(&stored) {
        payloads.push(payload_of(record)?);
    }
    let mut checks = Vec::<StoredMatch<'_>>::with_capacity(payloads.len());
    checks.extend(payloads.iter().map(|payload| StoredMatch {
        payload,
        captures: matched,
    }));
    theory_admits_with(theory, &checks)
}

/// Added by decision 124 (plan v4 C7.9, ruling OP-J of RDOJ 1): the store of the commit as a row
/// search reads it. Each read goes through the lists of the row, which check the lock coverage
/// first (CR-36), and stays for the rest of the search. The order of a pick is the order of the
/// role (`RowLists::pick_order`). A bind of one datum runs the item test of the request (R5).
pub(crate) struct CommitView<'v, 'r> {
    lists: &'v ParRowLists<'r>,
    matcher: &'v dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    theory: Option<&'v dyn SpaceTheory>,
    item: Option<RowItemTest<'v, TaggedContinuation, ListParWithRandom>>,
    data: Vec<(Par, Vec<ParDatum>)>,
    groups: Vec<(Vec<Par>, Vec<ParWaiter>)>,
    joins: Vec<(Par, Vec<Vec<Par>>)>,
    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the data channel of the collection
    /// of a head row, or `None` for a view of a join scope.
    collection: Option<&'v Par>,
    /// Added by decision 124 (plan v4 C7.11, rule CR-20, errata V4-13, ruling RADD 1.4): the book
    /// that the staging read, for a tail read. A commit search reads no book.
    book: Option<&'v BookPairs>,
}

impl<'v, 'r> CommitView<'v, 'r> {
    /// The view over `lists`, with the matcher `matcher` and the theory `theory` of the space and
    /// the item test `item` of the request (None for a check that binds nothing), for a search
    /// over the `channels` channels of one scope.
    pub(crate) fn new(
        lists: &'v ParRowLists<'r>,
        matcher: &'v dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        theory: Option<&'v dyn SpaceTheory>,
        item: Option<RowItemTest<'v, TaggedContinuation, ListParWithRandom>>,
        channels: usize,
    ) -> Self {
        Self {
            lists,
            matcher,
            theory,
            item,
            data: Vec::with_capacity(channels),
            groups: Vec::with_capacity(channels),
            joins: Vec::with_capacity(channels),
            collection: None,
            book: None,
        }
    }

    /// Added by decision 124 (rules CR-4 and R13): whether the recorded row names every datum
    /// of `tuple` (`RowLists::names_tuple`). It holds in play.
    pub(crate) fn names_tuple(&self, tuple: &[ParCandidate]) -> Result<bool, SpaceError> {
        Ok(self.lists.names_tuple(tuple)?)
    }

    // The position of the records of `channel` in `data`, after one read through the lists.
    fn records_at(&mut self, channel: &Par) -> Result<usize, SpaceError> {
        let Some(at) = self.data.iter().position(|(key, _)| key == channel) else {
            let records = self.lists.data(channel)?;
            self.data.push((channel.clone(), records));
            return Ok(self.data.len() - 1);
        };
        Ok(at)
    }

    // The records at the position `at` of `data`.
    fn records(&self, at: usize) -> Result<&[ParDatum], SpaceError> {
        let (_, records) = self.data.get(at).ok_or(SpaceError::Inconsistent(
            "a read of the commit view is missing",
        ))?;
        Ok(records.as_slice())
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): the view of a tail
    /// read over `lists`, with the matcher `matcher` and the theory `theory` of the space, the
    /// collection `collection` of a head row (`None` for a join scope) and the book `book` that
    /// the staging read (errata V4-13), for a read of the `channels` channels of the row.
    pub(crate) fn of_tail(
        lists: &'v ParRowLists<'r>,
        matcher: &'v dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        theory: Option<&'v dyn SpaceTheory>,
        collection: Option<&'v Par>,
        book: &'v BookPairs,
        channels: usize,
    ) -> Self {
        Self {
            collection,
            book: Some(book),
            ..Self::new(lists, matcher, theory, None, channels)
        }
    }

    // The collection of a head row. The reference lives as long as the view, so a read of the
    // collection holds no borrow of the view.
    fn collection_channel(&self) -> Result<&'v Par, SpaceError> {
        self.collection.ok_or(SpaceError::Inconsistent(
            "a view of a join scope has no collection of its own",
        ))
    }
}

impl StoreView for CommitView<'_, '_> {
    fn data_on(&mut self, channel: &Par) -> Result<&[ParDatum], SpaceError> {
        let at = self.records_at(channel)?;
        self.records(at)
    }

    fn waiters_on(&mut self, channel: &Par) -> Result<&[ParWaiter], SpaceError> {
        self.group_waiters(std::slice::from_ref(channel))
    }

    fn joins_on(&mut self, channel: &Par) -> Result<&[Vec<Par>], SpaceError> {
        let at = match self.joins.iter().position(|(key, _)| key == channel) {
            Some(at) => at,
            None => {
                let groups = self.lists.joins(channel)?;
                self.joins.push((channel.clone(), groups));
                self.joins.len() - 1
            }
        };
        let (_, groups) = self.joins.get(at).ok_or(SpaceError::Inconsistent(
            "a read of the commit view is missing",
        ))?;
        Ok(groups.as_slice())
    }

    fn group_waiters(&mut self, group: &[Par]) -> Result<&[ParWaiter], SpaceError> {
        let at = match self
            .groups
            .iter()
            .position(|(key, _)| key.as_slice() == group)
        {
            Some(at) => at,
            None => {
                let shared = self.lists.continuations(group)?;
                let mut waiters = Vec::<ParWaiter>::with_capacity(shared.len());
                waiters.extend(shared.iter().map(|waiter| (**waiter).clone()));
                self.groups.push((group.to_vec(), waiters));
                self.groups.len() - 1
            }
        };
        let (_, waiters) = self.groups.get(at).ok_or(SpaceError::Inconsistent(
            "a read of the commit view is missing",
        ))?;
        Ok(waiters.as_slice())
    }

    fn canonical_data_order(&mut self, channel: &Par) -> Result<Vec<usize>, SpaceError> {
        Ok(canonical_data_order(self.data_on(channel)?))
    }

    fn pick_data_order(
        &mut self,
        channel: &Par,
        _source: RSpaceOperationSource<'_>,
    ) -> Result<Vec<usize>, SpaceError> {
        let at = self.records_at(channel)?;
        let records = self.records(at)?;
        let order = canonical_data_order(records);
        Ok(self.lists.pick_order(records, None, order)?)
    }

    fn pick_data_order_with(
        &mut self,
        channel: &Par,
        extra: &ParDatum,
        _source: RSpaceOperationSource<'_>,
    ) -> Result<Vec<usize>, SpaceError> {
        let at = self.records_at(channel)?;
        let records = self.records(at)?;
        let order = canonical_data_order_with(records, extra);
        Ok(self.lists.pick_order(records, Some(extra), order)?)
    }

    fn bind(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        self.matcher.get(pattern, data)
    }

    fn bind_item(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
        persist: bool,
    ) -> Result<Option<ListParWithRandom>, SpaceError> {
        let Some(matched) = self.matcher.get(pattern, data) else {
            return Ok(None);
        };
        let Some(item) = &self.item else {
            return Ok(Some(matched));
        };
        match item.admits(&matched, persist)? {
            true => Ok(Some(matched)),
            false => Ok(None),
        }
    }

    fn has_theory(&self) -> bool { self.theory.is_some() }

    fn tests_items(&self) -> bool { self.item.is_some() }

    fn admits_all(
        &mut self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
        stored: &[StoredMatch<'_>],
    ) -> Result<bool, SpaceError> {
        match self.matcher.check_commit(continuation, matched) {
            true => self.theory_admits(stored),
            false => Ok(false),
        }
    }

    fn admits_records<T>(
        &mut self,
        continuation: &TaggedContinuation,
        matched: &ListParWithRandom,
        members: &[T],
        stored: impl Fn(&T) -> Option<&ListParWithRandom>,
        payload_of: fn(&ListParWithRandom) -> Result<ListParWithRandom, SpaceError>,
    ) -> Result<bool, SpaceError> {
        match self
            .matcher
            .check_commit(continuation, std::slice::from_ref(matched))
        {
            false => Ok(false),
            true => members_theory_admits(self.theory, matched, members, stored, payload_of),
        }
    }

    fn theory_admits(&self, stored: &[StoredMatch<'_>]) -> Result<bool, SpaceError> {
        theory_admits_with(self.theory, stored)
    }

    // Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the reads of the settle views.
    fn data_channel(&self) -> Result<&Par, SpaceError> { self.collection_channel() }

    fn data(&mut self) -> Result<&[ParDatum], SpaceError> {
        let channel = self.collection_channel()?;
        self.data_on(channel)
    }

    fn waiters(&mut self) -> Result<&[ParWaiter], SpaceError> {
        let channel = self.collection_channel()?;
        self.waiters_on(channel)
    }

    // A commit search reads no book. A tail read reads the book that the staging read
    // (errata V4-13).
    fn booked(&self, record: &Produce, waiter: &Consume) -> bool {
        let Some(book) = self.book else {
            return false;
        };
        book_holds(book, record, waiter)
    }
}

/// Stages the bump of the counters `counters`, and lists its staged operations in `plan`. It
/// returns the value of the collection counter before the
/// bump. The bump of the collection of an operation and the bump of each collection that a join
/// transaction touches stage the same operations.
///
/// Changed by plan v4 errata (V4-19, rule CR-6): the bump lists each operation in `plan` right
/// after its stage call succeeds, as `AllocCx::take` and `AllocCx::put` count theirs. So when a
/// record end of native replay stops the bump, `plan.staged` lists the followed prefix, and
/// `anchor_if_empty` stages nothing.
fn stage_bump(
    txn: &mut ParSpaceTxn,
    plan: &mut Plan<'_>,
    counters: CounterChannels<'_>,
    random: &Blake2b512Random,
    authority: OperationAuthority<'_>,
) -> Result<i64, SpaceError> {
    let bump = counter::bump(
        txn,
        counters,
        &BumpContext {
            matcher: plan.matcher,
            random,
            authority,
        },
        // Disabled by decision 90 (decision 69): Option P is gone.
        // &mut plan.legacy,
        // Added by plan v4 errata (V4-19): the staged record of the plan is the sink of the bump.
        // Each operation takes the class of a counter row (decision 124, ruling RADD 1.3).
        &mut |kind| plan.push_row(Staged::Counter(kind)),
    )?;
    // Changed by plan v4 errata (V4-19): the sink lists each operation of the bump right after
    // its stage call.
    // for kind in bump.staged.iter().copied() {
    //     plan.staged.push(Staged::Counter(kind));
    // }
    Ok(bump.ord)
}

/// The candidate of a consume-side fire of one record on `channel`: the record moves into the
/// candidate, with the values that it binds.
pub(crate) fn receive_candidate(channel: Par, fire: ReceiveFire) -> ParCandidate {
    let ReceiveFire {
        head: Datum { a, persist, source },
        matched,
        view_index,
    } = fire;
    ConsumeCandidate {
        channel,
        datum: Datum {
            a: matched,
            persist,
            source,
        },
        removed_datum: a,
        datum_index: view_index,
    }
}

/// The peek set of a receive: `{0}` for a peek, or the empty set.
pub fn peek_set(peek: bool) -> BTreeSet<i32> {
    match peek {
        true => BTreeSet::from([0]),
        false => BTreeSet::new(),
    }
}

/// The peek set of a join receive that peeks (P5c): `{0, …, n-1}`, because one arrow covers
/// every bind of a receive.
pub fn peek_set_all(n: usize) -> Result<BTreeSet<i32>, SpaceError> {
    let last = i32::try_from(n)
        .map_err(|_| SpaceError::Inconsistent("a join has more binds than a peek set holds"))?;
    Ok((0..last).collect())
}

/// The error of a missing config record: `DanglingName` for a space name and `DanglingHandle`
/// for every other channel.
fn missing_config(channel: &Par, space: NativeSpace) -> SpaceError {
    match namespace::reified_id_of(channel) {
        Some(ReifiedId::Name(..)) => SpaceError::DanglingName { space: space.sid },
        _ => SpaceError::DanglingHandle { space: space.sid },
    }
}

/// The start of a plan of `op` (I-21): runs the settle that `D` owes before the plan, and then
/// checks the round cap of the transaction. One space transaction makes at most `FIRE_ROUND_CAP`
/// fires, its settles included. When the settle leaves the transaction at the cap, the plan
/// stages nothing, and the operation re-issues as the program issued it, with the order rule of
/// decision 43 (`settle::reissued_operation`).
pub fn open_plan<D: Discipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    op: OpView<'_>,
) -> Result<Option<SpaceReissue>, SpaceError> {
    if let Some(reissue) = discipline.settle_before(cx, op)? {
        return Ok(Some(reissue));
    }
    if cx.rounds().fired() < FIRE_ROUND_CAP {
        return Ok(None);
    }
    let reissued = reissued_operation(op, cx.bumped());
    Ok(Some(cx.reissue(reissued, ReissueCause::RoundCap)))
}

/// The head driver of the produce plan (decision 11, decision 30): stages the store or the fires
/// of the datum of `op` with the order `ord`, and returns the re-issue. The datum fires only
/// from the head of the records ([`Cx::at_head`]), and otherwise it is stored at its place. A
/// re-issue of a persistent datum carries `ord` (decision 43). A fire that ends the loop moves
/// the record and its source into the staged produce, and a re-issue moves `op`, so only a
/// re-offer copies them. The settle hooks of `D` run before and after the plan.
pub fn offer_head<D: Discipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    op: ProduceOp,
    ord: i64,
) -> Result<Option<SpaceReissue>, SpaceError> {
    if let Some(reissue) = open_plan(discipline, cx, OpView::Produce(&op))? {
        return Ok(Some(reissue));
    }
    let record = record::wrap_datum(ord, &op.data);
    if !cx.at_head(discipline, Side::Data, op.reissued_ord)? {
        cx.check_capacity::<D>(Side::Data)?;
        cx.store(record, op.persistent)?;
        return discipline.settle_after(cx, OpView::Produce(&op));
    }
    let source = Produce::create(cx.data_channel(), &record, op.persistent);
    // Disabled by I-21 (one fire count for each transaction, `Cx::rounds`):
    // let mut rounds = FireRounds::new();
    loop {
        let Some(waiter) = cx.head_waiter(discipline)? else {
            cx.check_capacity::<D>(Side::Data)?;
            cx.store(record, op.persistent)?;
            return discipline.settle_after(cx, OpView::Produce(&op));
        };
        // Added by decision 124 (plan v4 C7.8, rule CR-20): the head waiter does not fire again
        // with a datum whose pair the book holds, so the datum is stored, as when the head
        // waiter does not match it.
        match cx.booked(&source, &waiter.source) {
            false => {}
            true => {
                cx.check_capacity::<D>(Side::Data)?;
                cx.store(record, op.persistent)?;
                return discipline.settle_after(cx, OpView::Produce(&op));
            }
        }
        let [pattern] = waiter.patterns.as_slice() else {
            return Err(SpaceError::Inconsistent(
                "a waiting continuation of a collection has several binds",
            ));
        };
        let Some(matched) = cx.matches(pattern, &op.data, &waiter.continuation, Held::InFlight)?
        else {
            cx.check_capacity::<D>(Side::Data)?;
            cx.store(record, op.persistent)?;
            return discipline.settle_after(cx, OpView::Produce(&op));
        };
        let spent = !op.persistent && waiter.peeks.is_empty();
        let stays = waiter.persist;
        // Changed by decision 124 (plan v4 C7.9 and C7.11, rulings D-1 and RADD 1.4): the fire
        // carries the head search of the discipline `D`, which the lines after the literal set
        // with the fire count before this fire. The text before:
        // let fire = OfferFire {
        let mut fire = OfferFire {
            persistent: op.persistent,
            waiter,
            matched,
            // Added by decision 102 (plan v4 C7.22): the datum of the produce is in flight.
            offered: Offered::InFlight,
            search: None,
        };
        let fires_before = cx.rounds().fired();
        fire.search =
            cx.head_offer_search::<D>(cx.data_channel(), &fire, &source, &record, fires_before);
        let step = cx.rounds().after_fire(spent, stays);
        match step {
            FireStep::Continue => {
                let (data, removed) = (record.clone(), record.clone());
                cx.fire_offer(fire, data, removed, source.clone())?;
            }
            FireStep::Done => {
                let removed = record.clone();
                cx.fire_offer(fire, record, removed, source)?;
                return discipline.settle_after(cx, OpView::Produce(&op));
            }
            FireStep::Reissue(cause) => {
                let removed = record.clone();
                cx.fire_offer(fire, record, removed, source)?;
                return Ok(Some(cx.reissue(
                    SpaceOp::Produce(ProduceOp {
                        reissued_ord: op.persistent.then_some(ord),
                        ..op
                    }),
                    cause,
                )));
            }
        }
    }
}

/// The head driver of the consume plan: stages the park or the fires of the continuation of
/// `op` with the order `ord`, and returns the re-issue. The continuation fires only from the head
/// of the waiters ([`Cx::at_head`]), and otherwise it is parked at its place. A re-issue of a
/// persistent continuation carries `ord` (decision 43). A fire that ends the loop moves the
/// parked continuation and the peek set into the staged consume, and a re-issue moves `op`, so
/// only a re-fire copies them. After a peek fire of a non-persistent record, `D::restore` stores
/// the record again (decision 45, Rule E). The settle hooks of `D` run before and after the plan.
pub fn receive_head<D: Discipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    op: ConsumeOp,
    ord: i64,
) -> Result<Option<SpaceReissue>, SpaceError> {
    if let Some(reissue) = open_plan(discipline, cx, OpView::Consume(&op))? {
        return Ok(Some(reissue));
    }
    let continuation = record::wrap_continuation_from_parts(
        ord,
        &op.body,
        &op.guard,
        record::stamped(&op.authority),
    )?;
    let peeks = peek_set(op.peek);
    if !cx.at_head(discipline, Side::Waiters, op.reissued_ord)? {
        cx.check_capacity::<D>(Side::Waiters)?;
        cx.park(&op, continuation, peeks)?;
        return discipline.settle_after(cx, OpView::Consume(&op));
    }
    // Added by decision 124 (plan v4 C7.8, rule CR-20): the identity of the consume, for the
    // pairs of the book.
    let source = cx.receive_source(&op, &continuation);
    // Disabled by I-21 (one fire count for each transaction, `Cx::rounds`):
    // let mut rounds = FireRounds::new();
    loop {
        let Some((index, head)) = cx.head_datum(discipline)? else {
            cx.check_capacity::<D>(Side::Waiters)?;
            cx.park(&op, continuation, peeks)?;
            return discipline.settle_after(cx, OpView::Consume(&op));
        };
        // Added by decision 124 (plan v4 C7.8, rule CR-20): the continuation does not fire
        // again with a head record whose pair the book holds, so it parks, as when the head
        // record does not match it.
        match cx.booked(&head.source, &source) {
            false => {}
            true => {
                cx.check_capacity::<D>(Side::Waiters)?;
                cx.park(&op, continuation, peeks)?;
                return discipline.settle_after(cx, OpView::Consume(&op));
            }
        }
        let payload = record::unwrap_datum(&head.a)?.datum;
        let Some(matched) = cx.matches(&op.pattern, &payload, &continuation, Held::Stored)? else {
            cx.check_capacity::<D>(Side::Waiters)?;
            cx.park(&op, continuation, peeks)?;
            return discipline.settle_after(cx, OpView::Consume(&op));
        };
        // Added by decision 124 (L6, CR30 D-b): the fire takes the lease of the stored datum
        // that the check proved.
        cx.lease_stored(1);
        let spent = !op.persistent;
        let stays = head.persist || op.peek;
        let view_index = i32::try_from(index)
            .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
        // Rule E (decision 45): the peek fire removes a non-persistent record, as the host
        // COMM does, and the record is stored again right after the fire.
        let restored = match op.peek && !head.persist {
            true => Some(head.a.clone()),
            false => None,
        };
        let fire = ReceiveFire {
            head,
            matched,
            view_index,
        };
        // Added by decision 124 (plan v4 C7.9 and C7.11, rulings D-1 and RADD 1.4): the head
        // search of the fire, of the discipline `D`, with the fire count before this fire. Each
        // arm below passes it to `fire_receive`.
        let fires_before = cx.rounds().fired();
        let search = cx.head_receive_search::<D>(&op, &continuation, &fire, fires_before);
        let step = cx.rounds().after_fire(spent, stays);
        match step {
            FireStep::Continue => {
                let fired = continuation.clone();
                // Changed by decision 124 (plan v4 C7.9, ruling D-1): the fire takes its search.
                // The text before:
                // cx.fire_receive(&op, fire, fired, peeks.clone())?;
                cx.fire_receive(&op, fire, fired, peeks.clone(), search)?;
                if let Some(reissue) = restore_peeked(discipline, cx, restored)? {
                    return Ok(Some(reissue));
                }
            }
            FireStep::Done => {
                // Changed by decision 124 (plan v4 C7.9, ruling D-1): the fire takes its search.
                // The text before:
                // cx.fire_receive(&op, fire, continuation, peeks)?;
                cx.fire_receive(&op, fire, continuation, peeks, search)?;
                if let Some(reissue) = restore_peeked(discipline, cx, restored)? {
                    return Ok(Some(reissue));
                }
                return discipline.settle_after(cx, OpView::Consume(&op));
            }
            FireStep::Reissue(cause) => {
                // Changed by decision 124 (plan v4 C7.9, ruling D-1): the fire takes its search.
                // The text before:
                // cx.fire_receive(&op, fire, continuation, peeks)?;
                cx.fire_receive(&op, fire, continuation, peeks, search)?;
                if let Some(reissue) = restore_peeked(discipline, cx, restored)? {
                    return Ok(Some(reissue));
                }
                return Ok(Some(cx.reissue(
                    SpaceOp::Consume(ConsumeOp {
                        reissued_ord: op.persistent.then_some(ord),
                        ..op
                    }),
                    cause,
                )));
            }
        }
    }
}

/// Stores again `restored`, the record that a peek fire removed, when there is one.
fn restore_peeked<D: Discipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    restored: Option<ListParWithRandom>,
) -> Result<Option<SpaceReissue>, SpaceError> {
    match restored {
        Some(record) => discipline.restore(cx, record),
        None => Ok(None),
    }
}

/// The order of a record in the staged view, from the decode of its envelope.
pub(crate) fn datum_ord(datum: &ParDatum) -> Result<i64, SpaceError> {
    record::datum_ord(&datum.a).map_err(SpaceError::from)
}

/// The order of a waiting continuation in the staged view, from the decode of its envelope.
pub(crate) fn waiter_ord(waiter: &ParWaiter) -> Result<i64, SpaceError> {
    record::continuation_ord(&waiter.continuation).map_err(SpaceError::from)
}

/// Added by decision 124 (plan v2 C7.10): [`waiter_ord`] of a live waiter of a head scan.
fn live_waiter_ord(waiter: &&ParWaiter) -> Result<i64, SpaceError> { waiter_ord(waiter) }

/// Added by decision 124 (plan v2 C7.10, decision 100, plan v4 C7.11, ruling RADD 1.4): the head
/// waiter of `waiters` by `discipline`, without the inert machinery waiters. The scan decodes
/// each live envelope, and only the head is copied. The staged view (`Cx::head_waiter`) and the
/// settle views over a store view read the head through it.
pub(crate) fn head_waiter_of<D: Discipline>(
    discipline: &D,
    waiters: &[ParWaiter],
) -> Result<Option<ParWaiter>, SpaceError> {
    let mut live = Vec::<&ParWaiter>::with_capacity(waiters.len());
    live.extend(waiters.iter().filter(|waiter| !machinery_shape(waiter)));
    let head = Cx::scan_head(discipline, Side::Waiters, &live, live_waiter_ord)?;
    match head.and_then(|(index, _)| live.get(index)) {
        Some(waiter) => Ok(Some((*waiter).clone())),
        None => Ok(None),
    }
}

/// Added by decision 124 (L14, plan v4 C7.8 and C7.11, rule CR-20, ruling RADD 1.4): true when
/// `book` holds the pair of `record` and `waiter`. The staged view and the view of a tail read
/// share this one rule.
fn book_holds(book: &BookPairs, record: &Produce, waiter: &Consume) -> bool {
    book.get(&record.hash)
        .is_some_and(|waiters| waiters.contains(&waiter.hash))
}

/// The order of an operation whose bump returned `bumped` (decision 43). With a counter, it is
/// the first-round order `reissued` of a re-issued persistent operation, and `bumped` for every
/// other operation. A bump value is never taken twice, so a first-round order lies in
/// `0..bumped`, and any other value is `Inconsistent`. Without a counter every order is 0, so a
/// re-issued order other than 0 is `Inconsistent`.
pub(crate) fn operation_ord(reissued: Option<i64>, bumped: Option<i64>) -> Result<i64, SpaceError> {
    match (reissued, bumped) {
        (None, Some(bumped)) => Ok(bumped),
        (None, None) => Ok(0),
        (Some(ord), Some(bumped)) => match (0..bumped).contains(&ord) {
            true => Ok(ord),
            false => Err(SpaceError::Inconsistent(
                "a re-issued order is not an order of the counter",
            )),
        },
        (Some(ord), None) => match ord == 0 {
            true => Ok(0),
            false => Err(SpaceError::Inconsistent(
                "a re-issued order of a collection without a counter is not 0",
            )),
        },
    }
}

/// Reads and decodes the config record of `space` on `channel`, and returns the config and the
/// map in the staged view. A missing record fails with `missing()`. The decode reads the
/// envelope, decodes the map strictly (`SpaceConfig::from_par_with_theory`) and compares the
/// space ids.
fn read_space_config<'t>(
    txn: &'t mut ParSpaceTxn,
    channel: &Par,
    space: NativeSpace,
    missing: impl FnOnce() -> SpaceError,
) -> Result<(SpaceConfig, &'t Par), SpaceError> {
    let map = match txn.data(channel)? {
        [] => return Err(missing()),
        [datum] => record::config_map(&datum.a)?,
        _ => return Err(SpaceError::Inconsistent("a space has two config records")),
    };
    let config = SpaceConfig::from_par_with_theory(map)?;
    match config.space_id == space.sid {
        true => Ok((config, map)),
        false => Err(SpaceError::Inconsistent(
            "the config record names another space",
        )),
    }
}

/// The fires that `staged` lists: the fires that the outcome dispatches.
fn fire_count(staged: &[Staged]) -> usize {
    staged
        .iter()
        .filter(|kind| matches!(kind, Staged::Fire | Staged::FirePrefix | Staged::JoinFire))
        .count()
}

/// The outcome of a committed collection operation. It moves each fired continuation out of its
/// receipt and copies nothing. A `FirePrefix` fire keeps the first results of its receipt, as
/// many as the next entry of `prefixes`. A counter operation, a retirement, a lift and an anchor
/// dispatch nothing.
fn outcome(
    receipt: ParSpaceReceipt,
    staged: Vec<Staged>,
    prefixes: Vec<usize>,
    reissue: Option<SpaceReissue>,
) -> Result<SpaceOutcome, SpaceError> {
    // Changed by decision 124 (decision 122, plan v4 V3-10): a receipt with a deviation holds its
    // rows up to row d and the group after d, so only a receipt without a deviation lists every
    // staged operation. The text before:
    // ```
    // if receipt.ops.len() != staged.len() {
    //     return Err(SpaceError::Inconsistent(
    //         "the receipt does not list the staged operations",
    //     ));
    // }
    // ```
    let cut = receipt
        .deviation
        .map(|deviation| (deviation.index, deviation.dispatched));
    let rows = match cut {
        None if receipt.ops.len() == staged.len() => staged.len(),
        Some((index, _)) if index < receipt.ops.len() && index < staged.len() => index,
        Some(_) | None => {
            return Err(SpaceError::Inconsistent(
                "the receipt does not list the staged operations",
            ))
        }
    };
    let mut fires = Vec::with_capacity(fire_count(&staged));
    let mut prefixes = prefixes.into_iter();
    let mut ops = receipt.ops.into_iter();
    let mut staged = staged.into_iter();
    // Changed by decision 124 (L12, plan v4 C7.11): the rows before row d decode as staged.
    // The text before:
    // for (receipt, kind) in receipt.ops.into_iter().zip(staged) {
    for (receipt, kind) in ops.by_ref().zip(staged.by_ref()).take(rows) {
        match (kind, receipt) {
            // Changed by decision 92: an anchor dispatches nothing.
            // (
            //     Staged::Counter(CounterOp::Take) | Staged::Retire | Staged::Lift,
            //     SpaceOpReceipt::Fired { .. },
            // )
            (
                Staged::Counter(CounterOp::Take) | Staged::Retire | Staged::Lift | Staged::Anchor,
                SpaceOpReceipt::Fired { .. },
            )
            | (Staged::Counter(CounterOp::Put) | Staged::Store, SpaceOpReceipt::Stored { .. })
            | (Staged::Park, SpaceOpReceipt::Parked { .. }) => {}
            (
                Staged::Fire,
                SpaceOpReceipt::Fired {
                    continuation,
                    results,
                    ..
                },
            ) => fires.push(SpaceFire {
                continuation: record::into_continuation(continuation.continuation)?.continuation,
                results,
            }),
            (
                Staged::FirePrefix,
                SpaceOpReceipt::Fired {
                    continuation,
                    mut results,
                    ..
                },
            ) => {
                let binds = prefixes.next().ok_or(SpaceError::Inconsistent(
                    "a prefix fire has no dispatched count",
                ))?;
                if binds > results.len() {
                    return Err(SpaceError::Inconsistent(
                        "a fire dispatches more results than its receipt holds",
                    ));
                }
                results.truncate(binds);
                fires.push(SpaceFire {
                    continuation: record::into_continuation(continuation.continuation)?
                        .continuation,
                    results,
                })
            }
            (
                Staged::JoinFire,
                SpaceOpReceipt::Fired {
                    continuation,
                    results,
                    ..
                },
            ) => {
                let record::JoinContinuationRecord {
                    slots,
                    continuation,
                } = record::into_join_continuation(continuation.continuation)?;
                fires.push(SpaceFire {
                    continuation,
                    results: record::restore_bind_order(&slots, results)?,
                })
            }
            _ => {
                return Err(SpaceError::Inconsistent(
                    "a receipt differs from its staged operation",
                ))
            }
        }
    }
    // Changed by decision 124 (L12, plan v4 C7.11). The text before:
    // ```
    // match prefixes.next() {
    //     None => Ok(SpaceOutcome {
    //         fires,
    //         reissue,
    //         mirror: None,
    //     }),
    //     Some(_) => Err(SpaceError::Inconsistent(
    //         "a dispatched count has no prefix fire",
    //     )),
    // }
    // ```
    match (cut, prefixes.next()) {
        (None, None) => Ok(SpaceOutcome {
            fires,
            reissue,
            mirror: None,
        }),
        (None, Some(_)) => Err(SpaceError::Inconsistent(
            "a dispatched count has no prefix fire",
        )),
        (Some((_, dispatched)), _) => {
            let (Some(receipt), Some(kind)) = (ops.next(), staged.next()) else {
                return Err(SpaceError::Inconsistent("a deviation names no row"));
            };
            fires.extend(deviating_fire(kind, receipt, dispatched)?);
            Ok(SpaceOutcome {
                fires,
                reissue,
                mirror: None,
            })
        }
    }
}

/// Added by decision 124 (L12, plan v4 C7.11): the fire of the deviating row d, or none. Row d
/// decodes by the version of its continuation record: a join record (version 2) restores its
/// bind order, and every other record decodes as one continuation. A `FirePrefix` row
/// dispatches `dispatched` results. A counter, retire, lift or anchor row dispatches nothing,
/// and so does a row that did not fire.
fn deviating_fire(
    kind: Staged,
    receipt: SpaceOpReceipt<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    dispatched: Option<usize>,
) -> Result<Option<SpaceFire>, SpaceError> {
    let SpaceOpReceipt::Fired {
        continuation,
        mut results,
        ..
    } = receipt
    else {
        return Ok(None);
    };
    match kind {
        Staged::Counter(_) | Staged::Retire | Staged::Lift | Staged::Anchor => return Ok(None),
        Staged::FirePrefix => {
            let binds = dispatched.ok_or(SpaceError::Inconsistent(
                "a deviating prefix fire has no dispatched count",
            ))?;
            if binds > results.len() {
                return Err(SpaceError::Inconsistent(
                    "a fire dispatches more results than its receipt holds",
                ));
            }
            results.truncate(binds);
        }
        Staged::Store | Staged::Park | Staged::Fire | Staged::JoinFire => {}
    }
    match record::join_continuation_slots(&continuation.continuation) {
        Ok(_) => {
            let record::JoinContinuationRecord {
                slots,
                continuation,
            } = record::into_join_continuation(continuation.continuation)?;
            Ok(Some(SpaceFire {
                continuation,
                results: record::restore_bind_order(&slots, results)?,
            }))
        }
        Err(_) => Ok(Some(SpaceFire {
            continuation: record::into_continuation(continuation.continuation)?.continuation,
            results,
        })),
    }
}

/// The most operations that one head-only operation stages: the bump, `FIRE_ROUND_CAP` fires,
/// and one store, park or re-store. The staged list starts with this room, so a plan of one
/// head-only operation does not grow it.
const MAX_STAGED: usize = OPEN_BUMP_OPERATIONS + FIRE_ROUND_CAP + 1;

/// The operations of the longest bump (`counter::bump` on a new collection).
const OPEN_BUMP_OPERATIONS: usize = 3;

/// The open transaction and the config of one collection operation after steps 1 and 2.
pub struct CollectionTxn {
    /// The transaction over the lock keys of the collection.
    pub txn: ParSpaceTxn,
    /// The channels of the collection.
    pub channels: CollectionChannels,
    /// The config of the space.
    pub config: SpaceConfig,
}

/// Steps 1 and 2 of a collection operation on `channel`, the collection `key` of `space`, with
/// the channels of the outer store `outer`: the transaction over the lock keys, and the config,
/// read once.
///
/// Changed by decision 94: the transaction holds phase A only, and [`prepare_collection_op`]
/// takes phase B after the classification.
pub async fn begin_collection<S: OuterStore>(
    outer: &S,
    env: &SpaceEnv,
    space: NativeSpace,
    key: &CollectionKey,
    channel: &Par,
) -> Result<CollectionTxn, SpaceError> {
    let channels = CollectionChannels::new(outer, space, key);
    // let mut txn = env.begin(channels.lock_keys()).await?;
    let mut txn = env.begin_phase_a(channels.lock_keys()).await?;
    let (config, _) = read_space_config(&mut txn, channels.config(), space, || {
        missing_config(channel, space)
    })?;
    Ok(CollectionTxn {
        txn,
        channels,
        config,
    })
}

// Changed by decision 94: the scheduler footprint `[2]‖sid` is gone, so the state can change
// between two transactions. The operation keeps its transaction, classifies under phase A, and
// takes phase B over its own keys and the keys of its scope, as the host takes its locks.
// /// Runs the collection operation `op` with the family agent `agent` in the open transaction
// /// `opened` ("scope, then lock, then plan", joins plan section 4.1). The agent classifies the
// /// operation. A `Single` operation runs `run_with` in `opened`. A `Join` operation runs the join
// /// engine. When the lock keys of `opened` do not cover the closure of the join, the agent drops
// /// `opened`, which stages nothing, and begins one new transaction over the lock keys of the
// /// closure: the scheduler footprint `[2]‖sid` serializes every job of the space, so the state
// /// does not change between the two transactions, and the closure is complete, so one escalation
// /// is enough. The config of `opened` is read once.
// pub async fn prepare_collection_op(
//     agent: &dyn FamilySpace,
//     env: &SpaceEnv,
//     mut opened: CollectionTxn,
//     op: SpaceOp,
// ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
//     match agent.classify(&mut opened, &op)? {
//         JoinRoute::Single => agent.run_with(env, opened, op),
//         JoinRoute::Join(scope) => match agent.always_escalates() {
//             false if scope.covers(&scope.own) => agent.run_join(env, opened, scope, op),
//             _ => {
//                 let CollectionTxn {
//                     txn,
//                     channels,
//                     config,
//                 } = opened;
//                 drop(txn);
//                 let txn = env.begin(scope.lock_keys()).await?;
//                 agent.run_join(
//                     env,
//                     CollectionTxn {
//                         txn,
//                         channels,
//                         config,
//                     },
//                     scope,
//                     op,
//                 )
//             }
//         },
//     }
// }

/// Runs the collection operation `op` with the family agent `agent` in the open transaction
/// `opened` ("scope, then lock, then plan", joins plan section 4.1). The transaction holds
/// phase A over the own lock keys, and the agent classifies the operation under phase A. A
/// `Single` operation takes phase B over its own keys and runs `run_with`. A `Join` operation
/// takes phase B over its own keys and the lock keys of its closure in one call, and runs the
/// join engine. The other jobs of the space wait at phase A, because every job locks `C(sid)`,
/// so the scope stays current until the commit (decision 94, `JoinScopeLock.tla`). The config
/// of `opened` is read once.
pub async fn prepare_collection_op(
    agent: &dyn FamilySpace,
    env: &SpaceEnv,
    mut opened: CollectionTxn,
    op: SpaceOp,
) -> Result<Prepared<SpaceOutcome>, SpaceError> {
    match agent.classify(&mut opened, &op)? {
        JoinRoute::Single => {
            env.lock_phase_b(&mut opened.txn, &[]).await?;
            agent.run_with(env, opened, op)
        }
        JoinRoute::Join(scope) => {
            env.lock_phase_b(&mut opened.txn, scope.lock_keys()).await?;
            agent.run_join(env, opened, scope, op)
        }
    }
}

/// The theory of a transaction and the leases of its admissions (P7, TH-5, TH-9): the checker
/// that the config read resolved, and the leases that the admission of the values of a send gave.
pub(crate) struct Admission {
    pub(crate) theory: Option<Arc<dyn SpaceTheory>>,
    pub(crate) leases: Vec<Box<dyn TheoryCommitLease>>,
}

impl Admission {
    /// TH-5: the theory of the space of `config`, and the admission of the values of `op` when
    /// `op` is a send. A send of the program and a re-issued send are admitted value by value,
    /// in order, before any staging or bump. A settle job carries only data that was admitted,
    /// so it admits nothing. The first value that is not `Proven` aborts the operation with
    /// `TheoryRefuted` or `TheoryUndetermined`.
    pub(crate) fn of(config: &SpaceConfig, op: &SpaceOp) -> Result<Self, SpaceError> {
        let theory = match &config.theory {
            Some(identity) => Some(theory::resolve(identity)?),
            None => None,
        };
        let leases = match (&theory, op) {
            (Some(theory), SpaceOp::Produce(produce)) => {
                theory::admit_values(theory.as_ref(), &produce.data.pars)?
            }
            (Some(_), SpaceOp::Consume(_) | SpaceOp::Join(_) | SpaceOp::Settle(_)) | (None, _) => {
                Vec::new()
            }
        };
        Ok(Self { theory, leases })
    }
}

/// Added by decision 102 (plan v4 C4.1): the error of a pick order whose index is outside the
/// staged view.
const PICK_OUTSIDE_VIEW: &str = "a pick order names an entry outside the staged view";

/// Added by decision 102 (plan v4 C4.1, rules CR-4 and R13): true when the recorded COMM `comm`
/// names the record `datum` at a pick of the operation `source`. The test is the base
/// `candidate_matches_with` through `SpaceTxn::admits`. The record is incoming when `source` is
/// its own produce, as in the base candidate test (`native_candidate.rs`).
fn names_record(
    txn: &mut ParSpaceTxn,
    comm: &COMM,
    datum: &ParDatum,
    source: RSpaceOperationSource<'_>,
) -> Result<bool, SpaceError> {
    let incoming =
        matches!(source, RSpaceOperationSource::Produce(produce) if *produce == datum.source);
    Ok(txn.admits(comm, datum, incoming)?)
}

/// Added by decision 102 (plan v4 C4.1, rule CR-4): true when the recorded COMM `comm` names the
/// waiter `index` of the group `group` in the staged view: its source is the consume of `comm`.
fn names_waiter(
    txn: &mut ParSpaceTxn,
    comm: &COMM,
    group: &[Par],
    index: usize,
) -> Result<bool, SpaceError> {
    match txn.continuations(group)?.get(index) {
        Some(waiter) => Ok(waiter.source == comm.consume),
        None => Err(SpaceError::Inconsistent(PICK_OUTSIDE_VIEW)),
    }
}

/// TH-9: a transaction with two or more theory leases aborts with `NestedCommitGuard` before its
/// commit, because their composition belongs to the guard composition of P12.
fn check_leases(leases: &[Box<dyn TheoryCommitLease>]) -> Result<(), SpaceError> {
    match leases {
        [] | [_] => Ok(()),
        [_, _, ..] => Err(SpaceError::NestedCommitGuard),
    }
}

/// Added by decision 92: stages the anchor of a transaction that staged nothing, so the
/// transaction commits one row that is unique to its job (`spaces::anchor`). The anchor takes
/// the class of a counter row (decision 124, ruling RADD 1.3).
fn anchor_if_empty(txn: &mut ParSpaceTxn, plan: &mut Plan<'_>) -> Result<(), SpaceError> {
    match plan.staged.is_empty() {
        true => {
            anchor::stage(txn, plan.job, plan.matcher, plan.introduction)?;
            plan.push_row(Staged::Anchor);
            Ok(())
        }
        false => Ok(()),
    }
}

/// Added by decision 124 (plan v2 C7.8, rule CR-16, ruling OP-LIFTS): the settle pair (r, W)
/// of the lift that the commit refused, from `lifts` (`Plan::lifts`). A lift row deviates
/// only when the commit refuses it (CR-16), and the commit parks a refused row. So the pair
/// is the entry of `lifts` at the row of the deviation, when that row parked. A lift that
/// fired, and every other row, books no settle pair.
fn refused_lift(lifts: Vec<(usize, BookPair)>, receipt: &ParSpaceReceipt) -> Option<BookPair> {
    let index = receipt.deviation.as_ref()?.index;
    match receipt.ops.get(index)? {
        SpaceOpReceipt::Parked { .. } => lifts
            .into_iter()
            .find(|(at, _)| *at == index)
            .map(|(_, pair)| pair),
        SpaceOpReceipt::Stored { .. }
        | SpaceOpReceipt::Fired { .. }
        | SpaceOpReceipt::Skipped { .. } => None,
    }
}

/// Added by decision 124 (L14, L15, plan v4 C7.8, rules CR-16 and CR-20, errata V4-17 and
/// V4-18, rulings OP-LIFTS and RAMD 1.7): the pairs that the finish books for `receipt`. They
/// are the refused pairs of every row, the item pairs of a deviation whose commit cannot veto
/// or whose Tail(actual) is Stop (`stop`), and the settle pair of a refused lift from `lifts`
/// (`Plan::lifts`). It moves the pairs out of the receipt. It reads no store and no registry.
fn finish_pairs(
    receipt: &mut ParSpaceReceipt,
    lifts: Vec<(usize, BookPair)>,
    stop: bool,
) -> impl Iterator<Item = BookPair> {
    let item_failed = match receipt.deviation.as_mut() {
        Some(deviation) => match (deviation.vetoing, stop) {
            (true, false) => Vec::new(),
            (false, _) | (true, true) => std::mem::take(&mut deviation.item_failed),
        },
        None => Vec::new(),
    };
    let lifted = refused_lift(lifts, receipt);
    let refused = std::mem::take(&mut receipt.refused);
    refused.into_iter().chain(item_failed).chain(lifted)
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.3 and RADD2 1.1 to 1.4, C2
/// E4.5): Tail(actual) of row d of the receipt `receipt`, or `None` when the commit did not
/// deviate, so the plan's own return holds. The search of row d gives the end when it built
/// one at the commit (`searched_end`). Otherwise it is a pure function of the receipt, of the
/// B.6 class of row d in `tail_rows`, of the fires before row d in `staged`
/// (`FireRounds::at`), of the restore rule (Rule E, decision 45), of the operation `operation`,
/// whose round bumped the counter from `bumped`, and of whether the discipline settles
/// (`settles`), by the table of C4v2 B.6. The search of row d gives the part that reads the
/// store (`RowDeviation::tail_read`).
fn tail_end(
    receipt: &ParSpaceReceipt,
    staged: &[Staged],
    tail_rows: &[TailRow],
    operation: Option<&SpaceOp>,
    bumped: Option<i64>,
    settles: bool,
) -> Result<Option<TailEnd>, SpaceError> {
    let Some(deviation) = receipt.deviation.as_ref() else {
        return Ok(None);
    };
    match searched_end(receipt)? {
        None => {}
        Some(end) => return Ok(Some(end)),
    }
    let (Some(row), Some(class), Some(before)) = (
        receipt.ops.get(deviation.index),
        tail_rows.get(deviation.index),
        staged.get(..deviation.index),
    ) else {
        return Err(SpaceError::Inconsistent(
            "the deviation names a row that the plan did not stage",
        ));
    };
    let fired = fire_count(before);
    let read = deviation.tail_read;
    let end = match (class, row) {
        // Row 7, and a retire of a group: the cut of a commit never names such a row.
        (TailRow::Counter | TailRow::Retire, _) => {
            return Err(SpaceError::Inconsistent(
                "the cut names a counter row, an anchor or a retire",
            ))
        }
        // Row 1: the element of the operation follows the settle before the plan.
        (TailRow::Before, _) => TailEnd::Stop(operation_reissue(operation, bumped)?),
        // Row 8: the re-offer follows the lift, with the pair in the book (OP-LIFTS).
        (TailRow::Lift, _) => TailEnd::Stop(settle_reissue(operation)?),
        // Row 8 of a cascade after the plan (ruling RADD2v2 6.1): the re-offer follows the lift,
        // and the settle job carries a cascade state, as the stop of such a plan builds it. It
        // cascades in the join engine, then re-issues the operation whose element met a
        // divergent pair (A10).
        (TailRow::CascadeLift, _) => TailEnd::Stop(cascade_reissue(operation, None)?),
        (TailRow::DivergentCascadeLift, _) => TailEnd::Stop(cascade_reissue(
            operation,
            Some(ReissueCause::DivergentPair),
        )?),
        // Row 6: R8 skipped the row. The receipt shows the skip, so both roles take the receipt
        // rule: a settle job only if the discipline settles and earlier rows fired.
        (
            TailRow::Element | TailRow::Settle | TailRow::ClassFire { .. } | TailRow::Restore,
            SpaceOpReceipt::Skipped { .. },
        ) => match (settles, fired) {
            (false, _) | (true, 0) => TailEnd::Return(None),
            (true, _) => TailEnd::Stop(settle_reissue(operation)?),
        },
        // Rows 2, 4 and 9: the element or the re-offer is in place. The search of row d read
        // the first step of the settle after the unit of row d.
        (
            TailRow::Element | TailRow::Settle | TailRow::ClassFire { .. },
            SpaceOpReceipt::Stored { .. } | SpaceOpReceipt::Parked { .. },
        ) => read_end(read, operation)?,
        // Rows 3 and 5, and the unit row of row 8b: the step of the fire at the fires before it.
        // A fire that a produce triggered offers its record, which is spent when it is not
        // persistent and the waiter does not peek, and the waiter stays when it is persistent.
        // A fire that a consume triggered spends its continuation unless it is persistent, and
        // its records stay when it peeks or one of them is persistent. The class
        // of a class fire stays by a member that the receipt does not list (CR-9).
        (
            TailRow::Element | TailRow::ClassFire { .. },
            SpaceOpReceipt::Fired {
                continuation,
                results,
                produce,
                ..
            },
        ) => {
            let persists = results.iter().any(|result| result.persistent);
            let class_stays = matches!(class, TailRow::ClassFire { persistent: true });
            let (spent, stays) = match produce {
                Some(_) => (!persists && !continuation.peek, continuation.persistent),
                None => (
                    !continuation.persistent,
                    continuation.peek || persists || class_stays,
                ),
            };
            // Rule E: a peek of a receive removes the record that it binds when the record is
            // not persistent, and the plan sends it again right after the fire. A receive of
            // one collection binds one record (`Cx::fire_receive`).
            let removed = match (produce, continuation.peek, results.as_slice()) {
                (Some(_), _, _) | (None, false, _) => None,
                (None, true, [result]) => match result.persistent {
                    true => None,
                    false => Some(&result.removed_datum),
                },
                (None, true, _) => {
                    return Err(SpaceError::Inconsistent(
                        "a receive that peeks fired other than one record",
                    ))
                }
            };
            match (removed, FireRounds::at(fired).after_fire(spent, stays)) {
                // The second send of the removed record follows the fire, so the re-issue
                // carries it. A receive that peeks is spent here.
                (Some(removed), FireStep::Done) => {
                    let origin = restore_origin(operation, bumped)?;
                    TailEnd::Stop(restore_reissue(origin, removed)?)
                }
                (Some(_), FireStep::Continue | FireStep::Reissue(_)) => {
                    return Err(SpaceError::Inconsistent(
                        "a receive that peeks and is not spent owes a second send and a re-issue",
                    ))
                }
                (None, FireStep::Done) => read_end(read, operation)?,
                (None, FireStep::Continue) => TailEnd::Stop(operation_reissue(operation, bumped)?),
                (None, FireStep::Reissue(cause)) => TailEnd::Return(Some(SpaceReissue {
                    op: operation_reissue(operation, bumped)?,
                    cause,
                })),
            }
        }
        // A fire of a settle offers its record. A lifted record that the fire
        // does not spend stays in flight, and the settle lands it before it re-issues.
        (
            TailRow::Settle,
            SpaceOpReceipt::Fired {
                continuation,
                results,
                ..
            },
        ) => {
            let persists = results.iter().any(|result| result.persistent);
            let spent = !persists && !continuation.peek;
            let step = FireRounds::at(fired).after_fire(spent, continuation.persistent);
            match (step, !persists && continuation.peek) {
                (FireStep::Done | FireStep::Continue, _) => read_end(read, operation)?,
                (FireStep::Reissue(cause), false) => TailEnd::Return(Some(SpaceReissue {
                    op: settle_reissue(operation)?,
                    cause,
                })),
                (FireStep::Reissue(_), true) => TailEnd::Stop(settle_reissue(operation)?),
            }
        }
        // The second send stored its record: the restore rule settles, and the spent receive ends.
        (TailRow::Restore, SpaceOpReceipt::Stored { .. }) => {
            restore_origin(operation, bumped)?;
            read_end(read, operation)?
        }
        // The second send fired a waiter: the step of its offer at the fires before it. It
        // continues, or re-issues at the cap or a divergent pair, as the send of its record
        // (`restore_scan`).
        (
            TailRow::Restore,
            SpaceOpReceipt::Fired {
                continuation,
                results,
                produce: Some(_),
                ..
            },
        ) => {
            let origin = restore_origin(operation, bumped)?;
            let persists = results.iter().any(|result| result.persistent);
            let spent = !persists && !continuation.peek;
            let removed = restored_record(receipt, tail_rows, deviation.index)?;
            match FireRounds::at(fired).after_fire(spent, continuation.persistent) {
                FireStep::Done => read_end(read, operation)?,
                FireStep::Continue => TailEnd::Stop(restore_reissue(origin, removed)?),
                FireStep::Reissue(cause) => TailEnd::Return(Some(SpaceReissue {
                    op: restore_reissue(origin, removed)?,
                    cause,
                })),
            }
        }
        // A second send is a send: it never parks and never fires as a receive.
        (
            TailRow::Restore,
            SpaceOpReceipt::Parked { .. } | SpaceOpReceipt::Fired { produce: None, .. },
        ) => {
            return Err(SpaceError::Inconsistent(
                "a second send of Rule E parked or fired as a receive",
            ))
        }
    };
    Ok(Some(end))
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the end of
/// Tail(actual) that the search of row d of `receipt` built at the commit with its tail read,
/// or `None` when the receipt does not deviate or the search built none. A search hands out a
/// `TailEnd` only, so a value of another type fails closed.
fn searched_end(receipt: &ParSpaceReceipt) -> Result<Option<TailEnd>, SpaceError> {
    let Some(deviation) = receipt.deviation.as_ref() else {
        return Ok(None);
    };
    let Some(reissue) = deviation.reissue.as_ref() else {
        return Ok(None);
    };
    match reissue.get::<TailEnd>() {
        Some(end) => Ok(Some(end.clone())),
        None => Err(SpaceError::Inconsistent(
            "the search of row d handed out a value that is not an end of Tail",
        )),
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.3 and RADD2 1.1, C2 E4.5):
/// the re-issue of a finish and the `stop` of `finish_pairs` from Tail(actual) `tail` and the
/// plan's own return `reissue`. Return(v) replaces the own return, and Stop re-issues the rest
/// of the operation for an owner refusal, which stages its anchor first (CR-19 case 2).
fn finish_reissue(
    tail: Option<TailEnd>,
    reissue: Option<SpaceReissue>,
) -> (Option<SpaceReissue>, bool) {
    match tail {
        None => (reissue, false),
        Some(TailEnd::Return(value)) => (value, false),
        Some(TailEnd::Stop(op)) => (
            Some(SpaceReissue {
                op,
                cause: ReissueCause::OwnerRefusal,
            }),
            true,
        ),
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): the end of Tail after a
/// row whose element or re-offer is in place, by the answer `read` of the search of row d after
/// its unit (rows 2, 4 and 9, and rows 3 and 5 after `Done`). The settle job continues after
/// the operation `operation`. At the round cap it re-issues with nothing staged (ADD A2.1).
fn read_end(read: TailRead, operation: Option<&SpaceOp>) -> Result<TailEnd, SpaceError> {
    match read {
        TailRead::Ends => Ok(TailEnd::Return(None)),
        TailRead::Stages => Ok(TailEnd::Stop(settle_reissue(operation)?)),
        TailRead::Reissues => Ok(TailEnd::Return(Some(SpaceReissue {
            op: settle_reissue(operation)?,
            cause: ReissueCause::RoundCap,
        }))),
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): the operation
/// `operation` again, as B.6 rows 1, 3 and 5 re-issue it: a produce or a consume as the program
/// issued it, with the order of decision 43 from `bumped` (`settle::reissued_operation`, as
/// `open_plan` re-issues it), and a settle job as it is. A run without owner funding keeps no
/// operation, and it never deviates (RC2 D-10). A join runs in the join engine.
fn operation_reissue(
    operation: Option<&SpaceOp>,
    bumped: Option<i64>,
) -> Result<SpaceOp, SpaceError> {
    match operation {
        Some(SpaceOp::Produce(op)) => Ok(reissued_operation(OpView::Produce(op), bumped)),
        Some(SpaceOp::Consume(op)) => Ok(reissued_operation(OpView::Consume(op), bumped)),
        Some(SpaceOp::Settle(op)) => Ok(SpaceOp::Settle(op.clone())),
        Some(SpaceOp::Join(_)) | None => Err(SpaceError::Inconsistent(
            "a deviating commit keeps no operation of its own to re-issue",
        )),
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): the settle job that
/// continues after the operation `operation`, as B.6 rows 2 to 6, 8 and 9 re-issue it
/// (`settle::settle_operation`). A settle job continues as itself.
fn settle_reissue(operation: Option<&SpaceOp>) -> Result<SpaceOp, SpaceError> {
    match operation {
        Some(SpaceOp::Produce(op)) => Ok(SpaceOp::Settle(settle_operation(OpView::Produce(op)))),
        Some(SpaceOp::Consume(op)) => Ok(SpaceOp::Settle(settle_operation(OpView::Consume(op)))),
        Some(SpaceOp::Settle(op)) => Ok(SpaceOp::Settle(op.clone())),
        Some(SpaceOp::Join(_)) | None => Err(SpaceError::Inconsistent(
            "a deviating commit keeps no operation of its own to re-issue",
        )),
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.1): the settle job that
/// continues after a refused lift of a cascade after the plan of the operation `operation`, as
/// the stop of such a plan builds it (arbitration A7): the fields of `settle::settle_operation`
/// and a cascade state with no seed and no owed send, which holds the re-issue of the operation
/// with the cause `resume` when its element did not settle (`settle::reissued_without_order`).
/// A settle with a cascade state runs in the join engine, which cascades over the whole scope
/// first (`join::classify`).
fn cascade_reissue(
    operation: Option<&SpaceOp>,
    resume: Option<ReissueCause>,
) -> Result<SpaceOp, SpaceError> {
    let (op, settle) = match operation {
        Some(op) => match op {
            SpaceOp::Produce(produce) => (op, settle_operation(OpView::Produce(produce))),
            SpaceOp::Consume(consume) => (op, settle_operation(OpView::Consume(consume))),
            SpaceOp::Settle(_) | SpaceOp::Join(_) => {
                return Err(SpaceError::Inconsistent(
                    "a lift of a cascade follows no produce or consume of its own",
                ))
            }
        },
        None => {
            return Err(SpaceError::Inconsistent(
                "a deviating commit keeps no operation of its own to re-issue",
            ))
        }
    };
    let resume = resume.map(|cause| {
        Box::new(SpaceReissue {
            op: reissued_without_order(op.clone()),
            cause,
        })
    });
    Ok(SpaceOp::Settle(SettleOp {
        cascade: Some(Box::new(super::agent::CascadeState {
            seeds: Vec::new(),
            pending: Vec::new(),
            resume,
        })),
        ..settle
    }))
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): the receive whose peek
/// fire owes the second send of Rule E, from the operation `operation` of a discipline whose
/// round bumped no counter (`bumped` is `None`). The send keeps the identity of its record only
/// with the order 0 of a discipline without a counter (`restored_send`). A receive that peeks
/// and is persistent would owe the send and its own re-issue, and Rholang gives no receive that
/// is persistent and peeks, so it fails closed.
fn restore_origin(
    operation: Option<&SpaceOp>,
    bumped: Option<i64>,
) -> Result<&ConsumeOp, SpaceError> {
    let (Some(SpaceOp::Consume(op)), None) = (operation, bumped) else {
        return Err(SpaceError::Inconsistent(
            "a second send of Rule E follows no receive of a discipline without a counter",
        ));
    };
    match op.persistent {
        false => Ok(op),
        true => Err(SpaceError::Inconsistent(
            "a persistent receive that peeks owes a second send and a re-issue",
        )),
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): the send that re-issues
/// the record `removed`, which a peek fire of the receive `origin` removed, in the form of
/// the restore rule (`settle::restored_send`).
fn restore_reissue(origin: &ConsumeOp, removed: &ListParWithRandom) -> Result<SpaceOp, SpaceError> {
    let payload = record::unwrap_datum(removed)?.datum;
    Ok(SpaceOp::Produce(restored_send(origin, payload)?))
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): the record of the second
/// send of Rule E that row `index` of `receipt` belongs to. The rows of the send hold the class
/// `Restore` in `tail_rows` from the row after the peek fire on, and that fire removed the
/// record that it binds (its one result).
fn restored_record<'r>(
    receipt: &'r ParSpaceReceipt,
    tail_rows: &[TailRow],
    index: usize,
) -> Result<&'r ListParWithRandom, SpaceError> {
    let fire = tail_rows.get(..index).and_then(|before| {
        before
            .iter()
            .rposition(|class| !matches!(class, TailRow::Restore))
    });
    let Some(SpaceOpReceipt::Fired {
        results,
        produce: None,
        ..
    }) = fire.and_then(|fire| receipt.ops.get(fire))
    else {
        return Err(SpaceError::Inconsistent(
            "a second send of Rule E follows no fire of a receive",
        ));
    };
    match results.as_slice() {
        [result] => Ok(&result.removed_datum),
        _ => Err(SpaceError::Inconsistent(
            "a receive that peeks fired other than one record",
        )),
    }
}

/// Added by decision 92: the end of a transaction whose staging failed with `error`. A failure
/// at a position of the state ([`SpaceError::is_positional`]) removes the staged operations,
/// stages the anchor only, and fails with `error` after the commit. So replay meets the failure
/// at the same place in the chain of the space. Every other failure drops the transaction,
/// which applies nothing.
fn prepare_failure(
    mut txn: ParSpaceTxn,
    plan: &Plan<'_>,
    error: SpaceError,
) -> Result<Prepared<SpaceOutcome>, SpaceError> {
    match error.is_positional() {
        false => Err(error),
        true => {
            txn.discard_staged()?;
            anchor::stage(&mut txn, plan.job, plan.matcher, plan.introduction)?;
            Ok(Prepared::new(txn, move |receipt| {
                match receipt.ops.as_slice() {
                    [SpaceOpReceipt::Fired { .. }] => Err(error),
                    _ => Err(SpaceError::Inconsistent(
                        "the receipt of a failed transaction does not list its anchor",
                    )),
                }
            }))
        }
    }
}

/// Added by decision 102 (plan v4 C4.3, rule CR-14): R-A. The first staging of the plan gave
/// `staging`, after its lease check. When it failed after a random pick ([`Plan::restages`]),
/// the transaction discards its staged operations, the plan starts again from `restart`, and
/// `restage` stages the spare operation once more, in DR-75 order. The lease check then runs on
/// the restage. A failure of the restage is the result, so the caller ends in
/// [`prepare_failure`] with the second error. Without `restart` the plan never restages.
fn restage_after_pick<T>(
    txn: &mut ParSpaceTxn,
    plan: &mut Plan<'_>,
    staging: Result<Option<SpaceReissue>, SpaceError>,
    restart: Option<(Restart, T)>,
    restage: impl FnOnce(&mut ParSpaceTxn, &mut Plan<'_>, T) -> Result<Option<SpaceReissue>, SpaceError>,
) -> Result<Option<SpaceReissue>, SpaceError> {
    match (staging, restart) {
        (Err(error), Some((restart, spare))) if plan.restages(txn.mode(), &error) => {
            txn.discard_staged()?;
            plan.restart(&restart);
            restage(txn, plan, spare)
                .and_then(|reissue| check_leases(&plan.leases).map(|()| reissue))
        }
        (staging, _) => staging,
    }
}

/// Added by decision 102 (plan v4 C4.4, rules CR-6 and CR-32): the staging result `staging`
/// after the record end of native replay. The staging ends, and the transaction keeps its
/// staged prefix, when a stage call answered `RecordEnded`, or when the staging failed after
/// the cursor followed a row (`SpaceTxn::end_record`). The lease check then runs on the
/// prefix, and the re-issue is `None`, because the receipt decides the finish (C7.11). Every
/// other failure stays, and the caller ends in [`prepare_failure`] (the error rule of CR-5).
fn record_end(
    txn: &mut ParSpaceTxn,
    plan: &Plan<'_>,
    staging: Result<Option<SpaceReissue>, SpaceError>,
) -> Result<Option<SpaceReissue>, SpaceError> {
    match staging {
        Ok(reissue) => Ok(reissue),
        Err(error) => match ends_staging(txn, &error) {
            true => check_leases(&plan.leases).map(|()| None),
            false => Err(error),
        },
    }
}

/// Added by decision 102 (plan v4 C4.4): true when the staging error `error` ends the staging
/// of `txn`: `RecordEnded` always does (CR-6), and any other error does when the transaction
/// answers so (CR-32).
fn ends_staging(txn: &mut ParSpaceTxn, error: &SpaceError) -> bool {
    match error {
        SpaceError::Substrate(SpaceTxnError::RecordEnded) => true,
        _ => txn.end_record(),
    }
}

/// Added by decision 124 (plan v4 C7.14, rule CR-19 case 2, errata V4-12): one staging of
/// `stage`, after the anchor when `anchor_first` holds. The anchor stages through
/// `anchor::stage`, so the first staging and the restage of R-A both start with it.
fn stage_anchored<T>(
    cx: &mut Cx<'_, '_>,
    anchor_first: bool,
    stage: impl FnOnce(&mut Cx<'_, '_>) -> Result<T, SpaceError>,
) -> Result<T, SpaceError> {
    match anchor_first {
        false => {}
        true => cx.anchor_before(false)?,
    }
    stage(cx)
}

/// The plan of one transaction of the join engine (P5c) over the open transaction `txn`, whose
/// own data channel is `data`: the staged list, the matcher, the introduction of
/// `introduction_authority` and the fire count of the transaction (decision 49), as `run_with`
/// makes them. `stage` stages the operations through
/// [`Cx`] and returns the re-issue. The outcome after the commit is the outcome of `run_with`.
///
/// Changed by decision 92: the plan takes the job `job` of the transaction. A transaction that
/// stages nothing commits an anchor only, and a failure at a position of the state commits an
/// anchor only and then fails ([`prepare_failure`]).
///
/// Changed by decision 102 (plan v4 C4.3 and C4.4): `stage` can run twice. When `may_restage`
/// holds, R-A calls it once more after a failure that followed a random pick
/// ([`restage_after_pick`]), and the caller gives it the operation again. A staging that the
/// record of native replay ends keeps its prefix ([`record_end`]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_plan(
    env: &SpaceEnv,
    mut txn: ParSpaceTxn,
    data: &Par,
    config: &SpaceConfig,
    introduction_authority: &CostAuthority,
    seq: Option<SeqClaims>,
    admission: Admission,
    job: &SpaceJob,
    // Added by decision 102 (plan v4 C4.3).
    may_restage: bool,
    // Changed by decision 102 (plan v4 C4.3): R-A calls `stage` again.
    // stage: impl FnOnce(&mut Cx<'_, '_>) -> Result<Option<SpaceReissue>, SpaceError>,
    mut stage: impl FnMut(&mut Cx<'_, '_>) -> Result<Option<SpaceReissue>, SpaceError>,
) -> Result<Prepared<SpaceOutcome>, SpaceError> {
    // Added by decision 124 (plan v4 C7.19, rule CR-24): the claims before the staging, the own
    // collection's among them, belong to row 0, so every commit stores them.
    let mut seq = seq;
    if let Some(claims) = seq.as_mut() {
        claims.close_row(0);
    }
    // Added by decision 102 (plan v4 C4.3): the start of R-A, before the plan takes the
    // admission and the claims.
    let restart = may_restage.then(|| (Restart::of(&admission, seq.as_ref()), ()));
    let staged = Vec::<Staged>::with_capacity(MAX_STAGED);
    let matcher = SpaceMatcher::new(env);
    let introduction = Introduction::new(&env.budget, introduction_authority);
    let mut plan = Plan {
        data,
        config,
        matcher: &matcher,
        introduction: &introduction,
        staged,
        prefixes: Vec::new(),
        bumped: None,
        // Disabled by decision 90 (decision 69): Option P is gone.
        // legacy: LegacyChain::new(&env.budget),
        rounds: FireRounds::new(),
        mirror: MirrorCapture::of(config),
        seq,
        theory: admission.theory,
        leases: admission.leases,
        job,
        // Added by decision 102 (plan v4 C4.2).
        picked: false,
        order: PickMode::Random,
        lifts: Vec::new(),
        // Added by decision 124 (L14, plan v4 C7.8): the plan reads one snapshot of the book.
        book: env.volatile.book(),
        // Added by decision 124 (plan v4 C7.11, ruling RADD 1.3): room for the class of each
        // staged row, as `staged` has, and no copy of the operation before the staging.
        tail_rows: Vec::<TailRow>::with_capacity(MAX_STAGED),
        operation: None,
    };
    // Changed by decision 92: a failure of the staging or of the lease check ends in
    // `prepare_failure`, and an empty transaction commits an anchor.
    // let reissue = stage(&mut Cx {
    //     txn: &mut txn,
    //     plan: &mut plan,
    // })?;
    // let Plan {
    //     staged,
    //     prefixes,
    //     legacy,
    //     mirror,
    //     seq,
    //     leases,
    //     ..
    // } = plan;
    // check_leases(&leases)?;
    // Changed by decision 124 (plan v4 C7.14, rule CR-19 case 2): the staging starts with the
    // anchor when `anchor_first` holds.
    let anchor_first = matches!(env.origin, OpOrigin::Internal { anchor_first: true });
    let staging = stage_anchored(
        &mut Cx {
            txn: &mut txn,
            plan: &mut plan,
        },
        anchor_first,
        &mut stage,
    );
    // Changed by decision 102 (plan v4 C4.3 and C4.4): R-A, and then the record end, before
    // the failure path.
    let staging = staging.and_then(|reissue| check_leases(&plan.leases).map(|()| reissue));
    // Changed by decision 124 (plan v4 C7.14, rule CR-19 case 2): the restage of R-A starts
    // with the anchor too.
    let staging = restage_after_pick(&mut txn, &mut plan, staging, restart, |txn, plan, ()| {
        stage_anchored(&mut Cx { txn, plan }, anchor_first, &mut stage)
    });
    let reissue = match record_end(&mut txn, &plan, staging) {
        Ok(reissue) => reissue,
        Err(error) => return prepare_failure(txn, &plan, error),
    };
    anchor_if_empty(&mut txn, &mut plan)?;
    let Plan {
        staged,
        prefixes,
        mirror,
        seq,
        // Added by decision 124 (plan v2 C7.8, rule CR-16, ruling OP-LIFTS): the finish books
        // the settle pair of a refused lift.
        lifts,
        ..
    } = plan;
    // Added by decision 124 (L14, plan v4 C7.8, rule CR-20): the finish books the pairs of the
    // receipt.
    let volatile = Arc::clone(&env.volatile);
    // Changed by decision 124 (L14, plan v4 C7.8, rule CR-20): the finish takes the pairs out
    // of the receipt. The text before:
    // Ok(Prepared::new(txn, move |receipt| {
    Ok(Prepared::new(txn, move |mut receipt| {
        // Disabled by decision 90 (decision 69): Option P is gone.
        // legacy.finish()?;
        let writes = mirror.writes(&receipt);
        // Added by decision 124 (plan v4 C7.19, rule CR-24, ruling RADD 1.8): the count of the
        // committed staged rows, and the claims of the actual unit of row d. A deviating receipt
        // holds rows 0 to d, then the retire group of row d (L12) and the finish rows of its
        // search (P1). With the actual claims, the staged claims of row d give way to them.
        let (rows, actual) = match receipt.deviation.as_mut() {
            None => (receipt.ops.len(), None),
            Some(deviation) => match deviation.claims.take() {
                None => (deviation.index + 1, None),
                Some(claims) => (deviation.index, Some(claims)),
            },
        };
        // Added by decision 124 (L14, L15, plan v4 C7.8 and C7.11, rules CR-20 and CR-31, errata
        // V4-13, V4-17, V4-18, rulings OP-LIFTS, RAMD 1.7 and RADD2 1.1, C2 E4.5): Tail(actual)
        // of row d when the search of row d built it at the commit, and then the pairs of
        // `finish_pairs`, booked after the outcome under the stripes of the commit. Without a
        // searched end the plan's own return holds. A Stop books the item pairs of a vetoing
        // row d too (CR-20 item 2).
        let (reissue, stop) = finish_reissue(searched_end(&receipt)?, reissue);
        let pairs = finish_pairs(&mut receipt, lifts, stop);
        let outcome = outcome(receipt, staged, prefixes, reissue)?.with_mirror(writes);
        volatile.book_pairs(pairs);
        // SQ-4: the claims of the job hold from its successful commit on.
        if let Some(seq) = seq {
            // Changed by decision 124 (plan v4 C7.19, rule CR-24): the claims of the committed
            // rows. The text before:
            // seq.store();
            seq.store(rows, actual);
        }
        Ok(outcome)
    }))
}

/// Steps 1 and 2 of the join `op` (P5c): the transaction over the lock keys `[C(sid), S(sid)]`
/// and `[S(sid,k), D(sid,k)]` of each distinct collection `k` of its binds, in ascending key
/// order, and the config, read once. The channels of the result are the channels of the
/// collection of the first bind. A join without a bind is `Inconsistent`. Changed by decision
/// 94: the transaction holds phase A only, and [`prepare_collection_op`] takes phase B.
pub async fn begin_join<S: OuterStore>(
    outer: &S,
    env: &SpaceEnv,
    op: &JoinOp,
) -> Result<CollectionTxn, SpaceError> {
    let Some(first) = op.binds.first() else {
        return Err(SpaceError::Inconsistent("a join has no bind"));
    };
    let channels = CollectionChannels::new(outer, op.space, &first.key);
    let keys = join::distinct_bind_keys(op);
    let length = keys.len() * 2 + 2;
    let mut lock_keys = Vec::<Par>::with_capacity(length);
    lock_keys.push(channels.config().clone());
    lock_keys.push(channels.space_counter().clone());
    for key in keys {
        let collection = join::scoped_collection(outer, op.space, key);
        lock_keys.push(collection.counter);
        lock_keys.push(collection.data);
    }
    // Changed by decision 94: phase A only, as `begin_collection`.
    // let mut txn = env.begin(&lock_keys).await?;
    let mut txn = env.begin_phase_a(&lock_keys).await?;
    let (config, _) = read_space_config(&mut txn, channels.config(), op.space, || {
        missing_config(&first.channel, op.space)
    })?;
    Ok(CollectionTxn {
        txn,
        channels,
        config,
    })
}

/// Compares the family of `config` with the family `S:D`. The comparison reads the names of the
/// config and of the family.
pub fn family_matches<S: OuterStore, D: Discipline>(config: &SpaceConfig) -> bool {
    config.is_family(S::NAME, D::NAME)
}

/// The `FamilyUnavailable` error of `config`, with the names of the config and no copy.
pub fn family_unavailable(config: SpaceConfig) -> SpaceError {
    SpaceError::FamilyUnavailable {
        outer: config.outer,
        inner: config.inner,
    }
}

/// The open transaction of an allocation (P5d): the transaction over the config channel, the
/// space counter and the allocator channel `A(sid)` of a space, the allocator channel, and the
/// config.
pub struct AllocationTxn {
    // Changed by decision 92 (D1): the transaction also locks `S(sid)`.
    // /// The transaction over `[C(sid), A(sid)]`.
    /// The transaction over `[C(sid), S(sid), A(sid)]`.
    pub txn: ParSpaceTxn,
    /// The allocator channel `A(sid)`.
    pub allocator: Par,
    /// The config of the space.
    pub config: SpaceConfig,
}

/// Opens the transaction of an allocation in `space` and reads the config once. Every
/// allocation locks `A(sid)`, because the family is known only after the config read. A missing
/// config fails with `DanglingHandle`. Changed by decision 92 (D1): the allocation also locks
/// `S(sid)`, the footprint key of every transaction of the space ([`SpaceEnv::begin_phase_a`]).
pub async fn begin_allocation(
    env: &SpaceEnv,
    space: NativeSpace,
) -> Result<AllocationTxn, SpaceError> {
    // Disabled by decision 92: the loop has an empty body, so it does no work.
    // for _ in 0..2 {}
    // let keys = [
    //     namespace::gprivate(namespace::config_channel(space)),
    //     namespace::gprivate(namespace::allocator_channel(space)),
    // ];
    // let mut txn = env.begin(&keys).await?;
    // let [config_channel, allocator] = keys;
    let keys = [
        namespace::gprivate(namespace::config_channel(space)),
        namespace::gprivate(namespace::space_counter(space)),
        namespace::gprivate(namespace::allocator_channel(space)),
    ];
    let mut txn = env.begin(&keys).await?;
    let [config_channel, _, allocator] = keys;
    let (config, _) = read_space_config(&mut txn, &config_channel, space, || {
        SpaceError::DanglingHandle { space: space.sid }
    })?;
    Ok(AllocationTxn {
        txn,
        allocator,
        config,
    })
}

/// Reads the config map of `space` in a transaction that commits nothing. A missing config
/// record fails with `DanglingHandle`. Changed by decision 92 (D1): the read also locks
/// `S(sid)`, the footprint key of every transaction of the space ([`SpaceEnv::begin_phase_a`]).
pub async fn read_config_map(env: &SpaceEnv, space: NativeSpace) -> Result<Par, SpaceError> {
    // let channel = namespace::gprivate(namespace::config_channel(space));
    // let mut txn = env.begin(std::slice::from_ref(&channel)).await?;
    let keys = [
        namespace::gprivate(namespace::config_channel(space)),
        namespace::gprivate(namespace::space_counter(space)),
    ];
    let mut txn = env.begin(&keys).await?;
    let [channel, _] = &keys;
    let (_, map) = read_space_config(&mut txn, channel, space, || SpaceError::DanglingHandle {
        space: space.sid,
    })?;
    Ok(map.clone())
}

/// Checks each key of `keys` against `schema`. Each comparison reads the key and the schema
/// entry. An unknown key fails with `UnknownParam` and the closest schema key.
fn check_keys<'k>(
    keys: impl Iterator<Item = &'k String>,
    schema: &[&str],
) -> Result<(), SpaceError> {
    for key in keys {
        let mut known = false;
        for entry in schema {
            if key == entry {
                known = true;
                break;
            }
        }
        if !known {
            return Err(SpaceError::UnknownParam {
                key: key.clone(),
                suggestion: suggestion(key, schema).map(str::to_owned),
            });
        }
    }
    Ok(())
}

impl<S: OuterStore, D: Discipline> GenericSpace<S, D> {
    /// Fails with `FamilyUnavailable` unless the config names the family of this agent, and
    /// otherwise returns the config. The error takes the names of the config without a copy.
    fn check_family(&self, config: SpaceConfig) -> Result<SpaceConfig, SpaceError> {
        // Disabled by decision 29: joins into ordered collections arrive in P5c, so the agent no
        // longer gates a family on `(USES_COUNTER, ¬JOINS)`. The family match stays.
        // match (config.is_family(S::NAME, D::NAME), D::USES_COUNTER, D::JOINS) {
        //     (true, true, false) => Ok(()),
        //     _ => Err(SpaceError::FamilyUnavailable {
        //         outer: config.outer,
        //         inner: config.inner,
        //     }),
        // }
        match family_matches::<S, D>(&config) {
            true => Ok(config),
            false => Err(family_unavailable(config)),
        }
    }

    // Disabled by decision 29 (P5c): `FamilySpace::classify` makes the same reads and
    // reservations, and it sends an operation on a collection with a join group to the join
    // engine (`spaces::join`) instead of failing with `Inconsistent`.
    // /// Step 3: the data channel holds no join other than its own group. The check reads the
    // /// stored group and the data channel, so it reserves both reads and one comparison, as the
    // /// branch's channel position check does.
    // fn check_joins(
    //     &self,
    //     txn: &mut ParSpaceTxn,
    //     channels: &CollectionChannels,
    //
    // ) -> Result<(), SpaceError> {
    //     let data = channels.data();
    //     match txn.joins(data)? {
    //         [] => Ok(()),
    //         [group] => {
    //
    //
    //
    //             match group.as_slice() == std::slice::from_ref(data) {
    //                 true => Ok(()),
    //                 false => Err(SpaceError::Inconsistent(
    //                     "a collection channel is part of a join",
    //                 )),
    //             }
    //         }
    //         _ => Err(SpaceError::Inconsistent(
    //             "a collection channel is part of a join",
    //         )),
    //     }
    // }

    /// Step 4: the named params `params` of a send and the query `query` of a receive against
    /// the schema of `D`, before any staging. Each key is compared with the schema, and an
    /// unknown key fails with `UnknownParam` and the closest schema key. A query on a discipline
    /// without query params fails with `QuerySpecUnsupported`. Then `D::check_values` checks the
    /// values. An operation without params and without a query does no work.
    pub(crate) fn check_params(
        &self,
        config: &SpaceConfig,
        params: &BTreeMap<String, Par>,
        query: Option<&QueryParams>,
    ) -> Result<(), SpaceError> {
        check_keys(params.keys(), D::SEND_PARAMS)?;
        if let Some(query) = query {
            if D::QUERY_PARAMS.is_empty() {
                return Err(SpaceError::QuerySpecUnsupported);
            }
            check_keys(query.named.keys(), D::QUERY_PARAMS)?;
        }
        match params.is_empty() && query.is_none() {
            true => Ok(()),
            false => self.discipline.check_values(config, params, query),
        }
    }

    /// Step 5: bumps the collection counter when `D` uses one, and returns the value before the
    /// bump.
    fn bump(
        &self,
        txn: &mut ParSpaceTxn,
        plan: &mut Plan<'_>,
        channels: &CollectionChannels,
        random_state: &[u8],
        authority: OperationAuthority<'_>,
    ) -> Result<Option<i64>, SpaceError> {
        if !D::USES_COUNTER {
            return Ok(None);
        }
        let random = Blake2b512Random::from_bytes(random_state);
        stage_bump(txn, plan, channels.counters(), &random, authority).map(Some)
    }

    /// Steps 5 and 6: the bump and the plan of the operation `op` in `txn`, with the
    /// introduction `introduction`. It returns the re-issue of the plan. Added by decision 92:
    /// the steps moved here unchanged from `run_with`, so that `run_with` sees a failure of
    /// either step ([`prepare_failure`]).
    fn stage_op(
        &self,
        txn: &mut ParSpaceTxn,
        plan: &mut Plan<'_>,
        channels: &CollectionChannels,
        introduction: &Arc<Introduction>,
        op: SpaceOp,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        match op {
            SpaceOp::Produce(op) => {
                let bumped = self.bump(
                    txn,
                    plan,
                    channels,
                    &op.data.random_state,
                    OperationAuthority::of_produce(&op, introduction),
                )?;
                plan.bumped = bumped;
                let ord = operation_ord(op.reissued_ord, bumped)?;
                let mut cx = Cx { txn, plan };
                self.discipline.offer(&mut cx, op, ord)
            }
            SpaceOp::Consume(op) => {
                let bumped = self.bump(
                    txn,
                    plan,
                    channels,
                    &op.body.random_state,
                    OperationAuthority::of_consume(&op, introduction),
                )?;
                plan.bumped = bumped;
                let ord = operation_ord(op.reissued_ord, bumped)?;
                let mut cx = Cx { txn, plan };
                self.discipline.receive(&mut cx, op, ord)
            }
            SpaceOp::Settle(op) => {
                plan.bumped =
                    self.bump(txn, plan, channels, &op.random_state, OperationAuthority {
                        continuation: record::stamped(&op.authority),
                        introduction,
                    })?;
                let mut cx = Cx { txn, plan };
                self.discipline.settle_job(&mut cx, op)
            }
            SpaceOp::Join(_) => Err(SpaceError::Inconsistent("a join runs in the join engine")),
        }
    }
}

impl<S: OuterStore, D: Discipline> FamilySpace for GenericSpace<S, D> {
    // Disabled by decision 94: phase B covers the scope of every operation.
    // fn always_escalates(&self) -> bool { S::ALWAYS_ESCALATES }

    fn classify(&self, opened: &mut CollectionTxn, op: &SpaceOp) -> Result<JoinRoute, SpaceError> {
        S::classify(self, opened, op)
    }

    /// Every check happens before the bump, so a rejected operation leaves the counter
    /// unchanged. A settle bumps the counter, as every round of an operation does (decision 31,
    /// decision 43).
    fn run_with(
        &self,
        env: &SpaceEnv,
        opened: CollectionTxn,
        op: SpaceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let CollectionTxn {
            mut txn,
            channels,
            config,
        } = opened;
        // Disabled by decision 29 (P5c): `classify` reads the join groups before this step.
        // self.check_joins(&mut txn, &channels)?;
        let no_params = BTreeMap::new();
        let (channel, key, params, query, introduction_authority) = match &op {
            SpaceOp::Produce(op) => (&op.channel, &op.key, &op.params, None, &op.authority),
            SpaceOp::Consume(op) => (
                &op.channel,
                &op.key,
                &no_params,
                op.query.as_ref(),
                &op.introduction_authority,
            ),
            SpaceOp::Settle(op) => match op.cascade {
                None => (
                    &op.channel,
                    &op.key,
                    &no_params,
                    None,
                    &op.introduction_authority,
                ),
                Some(_) => {
                    return Err(SpaceError::Inconsistent(
                        "a settle with a cascade runs in the join engine",
                    ))
                }
            },
            SpaceOp::Join(_) => {
                return Err(SpaceError::Inconsistent("a join runs in the join engine"))
            }
        };
        self.outer.check_key(&config, channel, key)?;
        match &op {
            SpaceOp::Produce(op) => self.outer.check_payload(&op.data)?,
            SpaceOp::Consume(op) => self.outer.check_patterns(&op.pattern)?,
            SpaceOp::Settle(_) | SpaceOp::Join(_) => {}
        }
        self.check_params(&config, params, query)?;
        // TH-5: the theory admits the values of a send before any staging or bump.
        let admission = Admission::of(&config, &op)?;
        // SQ-3: a program operation on a `seq` space claims its own collection first, as the
        // home of its datum or as the channel of its waiter. A settle job is internal.
        // Changed by decision 91: the claims also take the random state of the own item of the
        // operation, whose history learns the uses at the commit.
        let item: &[u8] = match &op {
            SpaceOp::Produce(op) => &op.data.random_state,
            SpaceOp::Consume(op) => &op.body.random_state,
            SpaceOp::Settle(op) => &op.random_state,
            SpaceOp::Join(_) => &[],
        };
        let mut seq = SeqClaims::open(
            &env.volatile,
            op.space(),
            config.qualifier,
            // Changed by decision 124 (plan v4 C7.14, ruling OP2-25 option B): a re-issue is
            // internal with or without its anchor first. The text before:
            // matches!(op, SpaceOp::Settle(_)) || env.origin == OpOrigin::Internal,
            matches!(op, SpaceOp::Settle(_)) || matches!(env.origin, OpOrigin::Internal { .. }),
            item,
        )?;
        if let Some(claims) = seq.as_mut() {
            let role = match &op {
                SpaceOp::Produce(_) => ClaimRole::Data,
                SpaceOp::Consume(_) | SpaceOp::Settle(_) | SpaceOp::Join(_) => ClaimRole::Waiter,
            };
            claims.claim(channels.data(), role)?;
            // Added by decision 124 (plan v4 C7.19, rule CR-24): the own claim belongs to row 0,
            // so every commit stores it.
            claims.close_row(0);
        }
        // Added by decision 102 (plan v4 C4.3, rule CR-14, U5): R-A can follow only a staging
        // of a discipline that picks at random, on a space where a staging can then fail with
        // `CapacityExceeded` (a bound), `NestedCommitGuard` (a theory) or `SeqConcurrent` (seq
        // claims). Only then does the transaction keep the start of R-A, taken after the own
        // claim, and a spare copy of the operation.
        let may_restage = D::RANDOM_PICKS
            && (self.discipline.bound(&config, Side::Data)?.is_some()
                || admission.theory.is_some()
                || seq.is_some());
        let restart = may_restage.then(|| (Restart::of(&admission, seq.as_ref()), op.clone()));
        // Added by decision 92: the job of the transaction, before the plan takes the operation.
        let job = SpaceJob::of(&op);
        let staged = Vec::<Staged>::with_capacity(MAX_STAGED);
        let matcher = SpaceMatcher::new(env);
        let introduction = Introduction::new(&env.budget, introduction_authority);
        let mut plan = Plan {
            data: channels.data(),
            config: &config,
            matcher: &matcher,
            introduction: &introduction,
            staged,
            prefixes: Vec::new(),
            bumped: None,
            // Disabled by decision 90 (decision 69): Option P is gone.
            // legacy: LegacyChain::new(&env.budget),
            rounds: FireRounds::new(),
            mirror: MirrorCapture::of(&config),
            seq,
            theory: admission.theory,
            leases: admission.leases,
            job: &job,
            // Added by decision 102 (plan v4 C4.2).
            picked: false,
            order: PickMode::Random,
            lifts: Vec::new(),
            // Added by decision 124 (L14, plan v4 C7.8): the plan reads one snapshot of the book.
            book: env.volatile.book(),
            // Added by decision 124 (plan v4 C7.11, ruling RADD 1.3): room for the class of each
            // staged row, as `staged` has, and no copy of the operation before the staging.
            tail_rows: Vec::<TailRow>::with_capacity(MAX_STAGED),
            operation: None,
        };
        // Added by decision 124 (plan v4 C7.11, ruling RADD 1.3, RC2 D-10): the plan keeps one
        // copy of the operation of an owner-funded run before the staging, as R-A keeps its
        // spare, for the re-issue of Tail(actual).
        plan.keep_operation(&op);
        // Changed by decision 92: the bump and the plan moved unchanged to `stage_op`. A failure
        // of the staging or of the lease check ends in `prepare_failure`, and an empty
        // transaction commits an anchor.
        // let reissue = match op {
        //     SpaceOp::Produce(op) => {
        //         let bumped = self.bump(
        //             &mut txn,
        //             &mut plan,
        //             &channels,
        //             &op.data.random_state,
        //             OperationAuthority::of_produce(&op, &introduction),
        //         )?;
        //         plan.bumped = bumped;
        //         let ord = operation_ord(op.reissued_ord, bumped)?;
        //         let mut cx = Cx {
        //             txn: &mut txn,
        //             plan: &mut plan,
        //         };
        //         self.discipline.offer(&mut cx, op, ord)?
        //     }
        //     SpaceOp::Consume(op) => {
        //         let bumped = self.bump(
        //             &mut txn,
        //             &mut plan,
        //             &channels,
        //             &op.body.random_state,
        //             OperationAuthority::of_consume(&op, &introduction),
        //         )?;
        //         plan.bumped = bumped;
        //         let ord = operation_ord(op.reissued_ord, bumped)?;
        //         let mut cx = Cx {
        //             txn: &mut txn,
        //             plan: &mut plan,
        //         };
        //         self.discipline.receive(&mut cx, op, ord)?
        //     }
        //     SpaceOp::Settle(op) => {
        //         plan.bumped = self.bump(
        //             &mut txn,
        //             &mut plan,
        //             &channels,
        //             &op.random_state,
        //             OperationAuthority {
        //                 continuation: record::stamped(&op.authority),
        //                 introduction: &introduction,
        //             },
        //         )?;
        //         let mut cx = Cx {
        //             txn: &mut txn,
        //             plan: &mut plan,
        //         };
        //         self.discipline.settle_job(&mut cx, op)?
        //     }
        //     SpaceOp::Join(_) => {
        //         return Err(SpaceError::Inconsistent("a join runs in the join engine"))
        //     }
        // };
        // let Plan {
        //     staged,
        //     prefixes,
        //     legacy,
        //     mirror,
        //     seq,
        //     leases,
        //     ..
        // } = plan;
        // check_leases(&leases)?;
        // Changed by decision 124 (plan v4 C7.14, rule CR-19 case 2): the staging starts with the
        // anchor when `anchor_first` holds.
        let anchor_first = matches!(env.origin, OpOrigin::Internal { anchor_first: true });
        let staging = stage_anchored(
            &mut Cx {
                txn: &mut txn,
                plan: &mut plan,
            },
            anchor_first,
            |cx| self.stage_op(cx.txn, cx.plan, &channels, &introduction, op),
        );
        // Changed by decision 102 (plan v4 C4.3 and C4.4): R-A restages the spare operation,
        // and then the record end runs, before the failure path.
        let staging = staging.and_then(|reissue| check_leases(&plan.leases).map(|()| reissue));
        // Changed by decision 124 (plan v4 C7.14, rule CR-19 case 2): the restage of R-A starts
        // with the anchor too.
        let staging = restage_after_pick(&mut txn, &mut plan, staging, restart, |txn, plan, op| {
            stage_anchored(&mut Cx { txn, plan }, anchor_first, |cx| {
                self.stage_op(cx.txn, cx.plan, &channels, &introduction, op)
            })
        });
        let reissue = match record_end(&mut txn, &plan, staging) {
            Ok(reissue) => reissue,
            Err(error) => return prepare_failure(txn, &plan, error),
        };
        anchor_if_empty(&mut txn, &mut plan)?;
        let Plan {
            staged,
            prefixes,
            mirror,
            seq,
            // Added by decision 124 (plan v2 C7.8, rule CR-16, ruling OP-LIFTS): the finish
            // books the settle pair of a refused lift.
            lifts,
            // Added by decision 124 (plan v4 C7.11, ruling RADD 1.3): the finish reads the class
            // of row d, the copy of the operation and its first-round order for Tail(actual).
            tail_rows,
            operation,
            bumped,
            ..
        } = plan;
        // Added by decision 124 (L14, plan v4 C7.8, rule CR-20): the finish books the pairs of
        // the receipt.
        let volatile = Arc::clone(&env.volatile);
        // Changed by decision 124 (L14, plan v4 C7.8, rule CR-20): the finish takes the pairs
        // out of the receipt. The text before:
        // Ok(Prepared::new(txn, move |receipt| {
        Ok(Prepared::new(txn, move |mut receipt| {
            // Disabled by decision 90 (decision 69): Option P is gone.
            // legacy.finish()?;
            let writes = mirror.writes(&receipt);
            // Added by decision 124 (plan v4 C7.19, rule CR-24, ruling RADD 1.8): the count of the
            // committed staged rows, and the claims of the actual unit of row d. A deviating
            // receipt holds rows 0 to d, then the retire group of row d (L12) and the finish rows
            // of its search (P1). With the actual claims, the staged claims of row d give way to
            // them.
            let (rows, actual) = match receipt.deviation.as_mut() {
                None => (receipt.ops.len(), None),
                Some(deviation) => match deviation.claims.take() {
                    None => (deviation.index + 1, None),
                    Some(claims) => (deviation.index, Some(claims)),
                },
            };
            // Added by decision 124 (L14, L15, plan v4 C7.8 and C7.11, rules CR-20 and CR-31,
            // errata V4-13, V4-17, V4-18, rulings OP-LIFTS, RAMD 1.7, RADD 1.3 and RADD2 1.1 to
            // 1.4, C2 E4.5): Tail(actual) of row d, and then the pairs of `finish_pairs`, booked
            // after the outcome under the stripes of the commit. Return(v) replaces the plan's own
            // return, and Stop re-issues the rest of the operation for an owner refusal. A Stop
            // books the item pairs of a vetoing row d too (CR-20 item 2).
            let tail = tail_end(
                &receipt,
                &staged,
                &tail_rows,
                operation.as_deref(),
                bumped,
                D::SETTLES,
            )?;
            let (reissue, stop) = finish_reissue(tail, reissue);
            let pairs = finish_pairs(&mut receipt, lifts, stop);
            let outcome = outcome(receipt, staged, prefixes, reissue)?.with_mirror(writes);
            volatile.book_pairs(pairs);
            // SQ-4: the claims of the job hold from its successful commit on.
            if let Some(seq) = seq {
                // Changed by decision 124 (plan v4 C7.19, rule CR-24): the claims of the
                // committed rows. The text before:
                // seq.store();
                seq.store(rows, actual);
            }
            Ok(outcome)
        }))
    }

    fn run_join(
        &self,
        env: &SpaceEnv,
        opened: CollectionTxn,
        scope: JoinScope,
        op: SpaceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        S::run_join(self, env, opened, scope, op)
    }

    /// A store without an allocator answers `Baseline` with no further work, and the dropped
    /// transaction commits nothing.
    ///
    /// Added by plan v4 errata (V4-19, C7.3 and rules CR-6 and CR-32) and decision 124 (L11): a
    /// record end of native replay ends the staging of the allocation (`ends_staging`). The
    /// transaction then commits the followed prefix. The commit of the followed prefix needs
    /// CanStop of its last row (CR-18). Otherwise it fails with the session mismatch. After the
    /// commit, the finish returns the staging error after its length check, so the allocation
    /// returns no index whose put did not commit. Every other error returns before the commit,
    /// as before.
    fn run_allocate(
        &self,
        env: &SpaceEnv,
        opened: AllocationTxn,
        op: &AllocOp,
    ) -> Result<Allocation, SpaceError> {
        if !S::ALLOCATOR {
            return Ok(Allocation::Baseline);
        }
        let AllocationTxn {
            mut txn,
            allocator,
            config,
        } = opened;
        let matcher = SpaceMatcher::new(env);
        let introduction = Introduction::new(&env.budget, &op.authority);
        let random = Blake2b512Random::from_bytes(&op.random_state);
        // Disabled by decision 90 (decision 69): Option P is gone.
        // let mut legacy = LegacyChain::new(&env.budget);
        // Changed by plan v4 errata (V4-19): the allocation keeps the result of the outer store
        // with the count of its staged operations.
        // let (indices, staged) = {
        let (allocation, staged) = {
            let mut cx = AllocCx {
                txn: &mut txn,
                context: BumpContext {
                    matcher: &matcher,
                    random: &random,
                    authority: OperationAuthority {
                        continuation: record::stamped(&op.authority),
                        introduction: &introduction,
                    },
                },
                allocator: &allocator,
                // Disabled by decision 90 (decision 69): Option P is gone.
                // legacy: &mut legacy,
                staged: 0,
            };
            // Changed by plan v4 errata (V4-19): an error of the outer store no longer returns
            // before the count of `cx` is read.
            // let indices = self.outer.allocate(&mut cx, &config, op)?;
            // (indices, cx.staged)
            let allocation = self.outer.allocate(&mut cx, &config, op);
            (allocation, cx.staged)
        };
        // Added by plan v4 errata (V4-19, rules CR-6 and CR-32) and decision 124 (L11): a record
        // end of native replay ends the staging, the transaction commits the followed prefix, and
        // the finish carries the staging error. The commit of the followed prefix needs CanStop of
        // its last row (CR-18). Otherwise it fails with the session mismatch. Every other error
        // returns before the commit, as before.
        let allocation = match allocation {
            Ok(indices) => Ok(indices),
            Err(error) => match ends_staging(&mut txn, &error) {
                true => Err(error),
                false => return Err(error),
            },
        };
        Ok(Allocation::Indices(Prepared::new(txn, move |receipt| {
            // Disabled by decision 90 (decision 69): Option P is gone.
            // legacy.finish()?;
            match receipt.ops.len() == staged {
                // Changed by plan v4 errata (V4-19): the finish returns the carried result of
                // the allocation.
                // true => Ok(indices),
                true => allocation,
                false => Err(SpaceError::Inconsistent(
                    "the receipt of an allocation does not list its staged operations",
                )),
            }
        })))
    }
}

/// The view and the staging primitives of an allocation (P5d, F11): the allocator record, and
/// the take and the put of the allocator, with the random states of `RandomPurpose::Allocator`
/// (`counter::take`, `counter::put`).
pub struct AllocCx<'c> {
    txn: &'c mut ParSpaceTxn,
    context: BumpContext<'c>,
    allocator: &'c Par,
    // Disabled by decision 90 (decision 69): Option P is gone.
    // legacy: &'c mut LegacyChain,
    staged: usize,
}

impl AllocCx<'_> {
    /// The allocator channel `A(sid)` of the space.
    pub fn allocator_channel(&self) -> &Par { self.allocator }

    /// The only record of the allocator in the staged view, copied, or `None`. Two records are
    /// `Inconsistent`.
    pub fn allocator_record(&mut self) -> Result<Option<ParDatum>, SpaceError> {
        match self.txn.data(self.allocator)? {
            [] => Ok(None),
            [stored] => Ok(Some(stored.clone())),
            _ => Err(SpaceError::Inconsistent(
                "a space has two allocator records",
            )),
        }
    }

    /// Consumes the allocator record `stored`, whose value is `value`.
    pub fn take(&mut self, stored: ParDatum, value: i64) -> Result<(), SpaceError> {
        counter::take(
            self.txn,
            self.allocator,
            stored,
            value,
            RandomPurpose::Allocator,
            &self.context,
            // Disabled by decision 90 (decision 69): Option P is gone.
            // self.legacy,
        )?;
        self.staged += 1;
        Ok(())
    }

    /// Stores the allocator record with the value `value`.
    pub fn put(&mut self, value: i64) -> Result<(), SpaceError> {
        counter::put(
            self.txn,
            self.allocator,
            value,
            RandomPurpose::Allocator,
            &self.context,
            // Disabled by decision 90 (decision 69): Option P is gone.
            // self.legacy,
        )?;
        self.staged += 1;
        Ok(())
    }
}

#[async_trait]
impl<S: OuterStore, D: Discipline> SpaceAgent for GenericSpace<S, D> {
    async fn prepare_produce(
        &self,
        env: &SpaceEnv,
        op: ProduceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_collection(&self.outer, env, op.space, &op.key, &op.channel).await?;
        let config = self.check_family(opened.config)?;
        prepare_collection_op(
            self,
            env,
            CollectionTxn { config, ..opened },
            SpaceOp::Produce(op),
        )
        .await
    }

    async fn prepare_consume(
        &self,
        env: &SpaceEnv,
        op: ConsumeOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_collection(&self.outer, env, op.space, &op.key, &op.channel).await?;
        let config = self.check_family(opened.config)?;
        prepare_collection_op(
            self,
            env,
            CollectionTxn { config, ..opened },
            SpaceOp::Consume(op),
        )
        .await
    }

    async fn prepare_settle(
        &self,
        env: &SpaceEnv,
        op: SettleOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_collection(&self.outer, env, op.space, &op.key, &op.channel).await?;
        let config = self.check_family(opened.config)?;
        prepare_collection_op(
            self,
            env,
            CollectionTxn { config, ..opened },
            SpaceOp::Settle(op),
        )
        .await
    }

    async fn prepare_join(
        &self,
        env: &SpaceEnv,
        op: JoinOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_join(&self.outer, env, &op).await?;
        let config = self.check_family(opened.config)?;
        prepare_collection_op(
            self,
            env,
            CollectionTxn { config, ..opened },
            SpaceOp::Join(op),
        )
        .await
    }

    async fn allocate(&self, env: &SpaceEnv, op: AllocOp) -> Result<Allocation, SpaceError> {
        let opened = begin_allocation(env, op.space).await?;
        let config = self.check_family(opened.config)?;
        self.run_allocate(env, AllocationTxn { config, ..opened }, &op)
    }

    /// Stages the records of a new space (P4 design C.7, steps 4 to 7): the check that `C(sid)`
    /// holds no config (`SpaceAlreadyExists`), the strict decode of the new config, the config
    /// datum, the space counter `S(sid) = 0`, the records of the outer store, and the handle
    /// continuation. The channels of the records of the outer store join the lock keys.
    ///
    /// Added by plan v4 errata (V4-19, C7.3 phase B and rules CR-6 and CR-32) and decision 124
    /// (L11): the config produce is the first operation and the job's own, so it never answers
    /// `RecordEnded`. The stage calls after it give one staging result. A record end of native
    /// replay ends that staging (`ends_staging`), and the transaction commits the followed prefix.
    /// The commit of the followed prefix needs CanStop of its last row (CR-18). Otherwise it fails
    /// with the session mismatch. The finish rejects a prefix receipt, so the space is not
    /// recorded as created. Every other error returns before the commit, as before.
    async fn prepare_create(
        &self,
        env: &SpaceEnv,
        creation: SpaceCreation,
    ) -> Result<Prepared<()>, SpaceError> {
        let space = creation.space;
        let keys = [
            namespace::gprivate(namespace::config_channel(space)),
            namespace::gprivate(namespace::space_counter(space)),
            namespace::gprivate(namespace::handle_channel(space)),
        ];
        // Added by decision 124 (plan v2 C7.3, decision 99): row 0 keeps the introduction of
        // the creation. Each record after it takes the machinery seal, and each row after it
        // the payer introduction, so R8 can skip row 0 only (CR-15).
        let introduction = Introduction::new(&env.budget, &creation.authority);
        let sealed = record::cost_authority(&creation.authority);
        let seal = introduction.seal_of(sealed.as_ref())?.unwrap_or_default();
        let payer = introduction.payer();
        // Changed by decision 124 (plan v2 C7.3): the records take the seal. The text before:
        // ```
        // let records = self
        //     .outer
        //     .creation_records(space, &creation.random, &creation.authority)?;
        // ```
        let records = self
            .outer
            .creation_records(space, &creation.random, &seal)?;
        let mut txn = match records.is_empty() {
            true => env.begin(&keys).await?,
            false => {
                let mut locked = Vec::<Par>::with_capacity(keys.len() + records.len());
                for channel in keys
                    .iter()
                    .chain(records.iter().map(|(channel, _)| channel))
                {
                    locked.push(channel.clone());
                }
                env.begin(&locked).await?
            }
        };
        let [config_channel, space_counter, handle] = keys;
        if !txn.data(&config_channel)?.is_empty() {
            return Err(SpaceError::SpaceAlreadyExists);
        }
        let config = SpaceConfig::from_par_with_theory(&creation.config)?;
        if config.space_id != space.sid {
            return Err(SpaceError::Inconsistent(
                "the config of a new space names another space",
            ));
        }
        let qualifier = config.qualifier;
        let volatile = Arc::clone(&env.volatile);
        self.check_family(config)?;
        // Changed by decision 124 (plan v2 C7.3): built before the records. The text before:
        // let introduction = Introduction::new(&env.budget, &creation.authority);
        // Disabled by decision 90 (decision 69): Option P is gone.
        // let mut legacy = LegacyChain::new(&env.budget);
        // Added by decision 92: every operation of a creation is the job's own, because its random
        // state derives from the random state of the creation (`Config`, `SpaceCounter`,
        // `Allocator` and `Handle`). The first operation is the config, so no anchor is needed.
        let config = SpaceProduce {
            channel: config_channel,
            data: record::config_record(
                creation.config,
                &RandomPurpose::Config.derive(&creation.random),
            ),
            persist: true,
            outcome: ProduceOutcome::Store,
            before_observe: Some(record::register_introduction(&introduction)),
            own: true,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        };
        // Changed by decision 90 (decision 69): Option P is gone, so the operation stages directly.
        // legacy.produce(&mut txn, config)?;
        txn.produce(config)?;
        // Changed by decision 124 (plan v2 C7.3): the space counter takes the seal. The text
        // before:
        // let cost_authority = record::cost_authority(&creation.authority);
        let cost_authority = record::cost_authority(&seal);
        let counter = SpaceProduce {
            channel: space_counter,
            data: record::counter_record(
                0,
                &RandomPurpose::SpaceCounter
                    .derive(&creation.random)
                    .split_byte(1),
                cost_authority,
            ),
            persist: false,
            outcome: ProduceOutcome::Store,
            // Changed by decision 124 (plan v2 C7.3): the payer introduction. The text before:
            // before_observe: Some(record::register_introduction(&introduction)),
            before_observe: Some(record::register_introduction(&payer)),
            own: true,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        };
        // Changed by decision 90 (decision 69): Option P is gone, so the operation stages directly.
        // legacy.produce(&mut txn, counter)?;
        // Changed by plan v4 errata (V4-19): the stage calls after the config give one staging
        // result, so a record end of native replay ends the staging.
        // txn.produce(counter)?;
        let mut staging = txn.produce(counter).map(drop);
        let extra = records.len();
        for (channel, data) in records {
            let staged = SpaceProduce {
                channel,
                data,
                persist: false,
                outcome: ProduceOutcome::Store,
                // Changed by decision 124 (plan v2 C7.3): the payer introduction. The text
                // before:
                // before_observe: Some(record::register_introduction(&introduction)),
                before_observe: Some(record::register_introduction(&payer)),
                own: true,
                // Added by decision 123 (plan v4, V3-9): no row search.
                search: None,
            };
            // Changed by decision 90 (decision 69): Option P is gone, so the operation stages
            // directly.
            // legacy.produce(&mut txn, staged)?;
            // Changed by plan v4 errata (V4-19): the record stages only while the staging holds.
            // txn.produce(staged)?;
            staging = staging.and_then(|()| txn.produce(staged).map(drop));
        }
        let mut channels = Vec::<Par>::with_capacity(1);
        channels.push(handle);
        let mut patterns = Vec::<BindPattern>::with_capacity(1);
        patterns.push(creation.handle_pattern);
        let handle = SpaceConsume {
            channels,
            patterns,
            continuation: creation.handle_continuation,
            persist: true,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Park,
            // Changed by decision 124 (plan v2 C7.3): the payer introduction. The text before:
            // before_observe: Some(record::register_introduction(&introduction)),
            before_observe: Some(record::register_introduction(&payer)),
            own: true,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        };
        // Changed by decision 90 (decision 69): Option P is gone, so the operation stages directly.
        // legacy.consume(&mut txn, handle)?;
        // Changed by plan v4 errata (V4-19): the handle stages only while the staging holds.
        // txn.consume(handle)?;
        let staging = staging.and_then(|()| txn.consume(handle).map(drop));
        // Added by plan v4 errata (V4-19, rules CR-6 and CR-32) and decision 124 (L11): a record
        // end of native replay ends the staging, and the transaction commits the followed prefix.
        // The commit of the followed prefix needs CanStop of its last row (CR-18). Otherwise it
        // fails with the session mismatch. Every other error returns before the commit, as before.
        match staging.map_err(SpaceError::from) {
            Ok(()) => {}
            Err(error) => match ends_staging(&mut txn, &error) {
                true => {}
                false => return Err(error),
            },
        }
        Ok(Prepared::new(txn, move |receipt| {
            // Disabled by decision 90 (decision 69): Option P is gone.
            // legacy.finish()?;
            // Added by decision 124 (plan v4 C7.3, errata V4-19): a creation whose first row the
            // owner skipped (R8) commits that row alone and creates no space.
            if let [SpaceOpReceipt::Skipped { .. }] = receipt.ops.as_slice() {
                return Ok(());
            }
            let committed = match receipt.ops.as_slice() {
                [SpaceOpReceipt::Stored { .. }, SpaceOpReceipt::Stored { .. }, stored @ .., SpaceOpReceipt::Parked { .. }] => {
                    stored.len() == extra
                        && stored
                            .iter()
                            .all(|op| matches!(op, SpaceOpReceipt::Stored { .. }))
                }
                _ => false,
            };
            match committed {
                // Decision 83 (VR-2): the job records an ephemeral space as its last step.
                true => {
                    volatile.record_created(space, qualifier);
                    Ok(())
                }
                false => Err(SpaceError::Inconsistent(
                    "a space creation committed other operations",
                )),
            }
        }))
    }

    async fn read_config(&self, env: &SpaceEnv, space: NativeSpace) -> Result<Par, SpaceError> {
        read_config_map(env, space).await
    }

    fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError> {
        self.outer.gensym(space, random)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, LazyLock};

    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{CostAuthority, CostRegion, CostSignature, Expr, ParWithRandom};
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{
        new_freevar_par, new_freevar_var, new_gbool_par, new_gint_par, new_gstring_par,
    };
    // Retired by decision 90 (C6, step D): the replay tests of the queue were its only users, and
    // they retire (decision 125). The text before:
    // use rspace_plus_plus::rspace::checkpoint::Checkpoint;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::hashing::stable_hash_provider;
    use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
    use rspace_plus_plus::rspace::merger::merging_logic::{self, NumberChannelsDiff};
    use rspace_plus_plus::rspace::operation_context::{self, CausalPath, OperationOrder};
    // Retired by decision 90 (C6, step D): the replay tests of the queue ran through the rigged
    // replay modes, which retire (decision 125). Native replay of re-entries and of the queue:
    // `space_native_persistent_retry_rows`, `space_native_replay_equal_hashmap`. The text before:
    // ```
    // use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
    // ```
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::{ContResult, ISpace, RSpaceResult};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    // Added by decision 124 (plan v4 C7.19, rule CR-24, ruling RADD2v4 7.6): the claims of the
    // test of the stored claims after a deviation.
    use rspace_plus_plus::rspace::space_store_handle::RowClaims;
    use rspace_plus_plus::rspace::space_store_handle::{
        SpaceStoreHandle, SpaceTxnError, SpaceTxnHost,
    };
    use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, COMM};

    use super::*;
    use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
    use crate::rust::interpreter::accounting::byte_accounting::{
        consume_introduction_identity, produce_introduction_identity,
    };
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::reified::costs::{ReissueCause, FIRE_ROUND_CAP};
    use crate::rust::interpreter::spaces::agent::run_op;
    use crate::rust::interpreter::spaces::counter::{counter_continuation, counter_pattern};
    use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
    use crate::rust::interpreter::spaces::families::FamilyAgent;
    use crate::rust::interpreter::spaces::namespace::{Durability, SID_LEN};
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
    use crate::rust::interpreter::spaces::test_support::{
        named_first, recorded, DeviatingCommit, Funded, ListsProbe, ScriptedReplay,
    };
    use crate::rust::interpreter::spaces::theory::identity::TheoryIdentity;
    use crate::rust::interpreter::spaces::theory::TheoryUsage;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    type Queue = GenericSpace<HashMapOuter, QueueDiscipline>;
    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
    // Retired by decision 90 (C6, step D): the replay tests of the queue ran through the rigged
    // replay modes, which retire (decision 125). Native replay of re-entries and of the queue:
    // `space_native_persistent_retry_rows`, `space_native_replay_equal_hashmap`. The text before:
    // ```
    // type ParReplay = ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
    // ```

    const SPACE: NativeSpace = NativeSpace {
        sid: [7; SID_LEN],
        durability: Durability::Durable,
    };

    fn budget() -> RuntimeBudget { RuntimeBudget::new(Cost::create(1_000_000, "queue agent test")) }

    fn env_over<H>(host: H, budget: &RuntimeBudget) -> SpaceEnv
    where H: SpaceTxnHost<Par, BindPattern, ListParWithRandom, TaggedContinuation> + 'static {
        SpaceEnv {
            store: SpaceStoreHandle::new(host),
            budget: budget.clone(),
            volatile: Default::default(),
            origin: Default::default(),
        }
    }

    async fn play() -> ParRSpace {
        let mut stores = InMemoryStoreManager::new();
        ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space")
    }

    // Retired by decision 90 (C6, step D): the replay tests of the queue ran through the rigged
    // replay modes, which retire (decision 125). Native replay of re-entries and of the queue:
    // `space_native_persistent_retry_rows`, `space_native_replay_equal_hashmap`. The text before:
    // ```
    // async fn play_and_replay() -> (ParRSpace, ParReplay) {
    //     let mut stores = InMemoryStoreManager::new();
    //     RSpace::create_with_replay(
    //         stores.r_space_stores().await.expect("in-memory stores"),
    //         Arc::new(Box::new(Matcher)),
    //     )
    //     .expect("an in-memory play and replay space")
    // }
    // ```

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn region(byte: u8) -> CostAuthority {
        CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![byte; 32],
                signature: Some(CostSignature {
                    value: Some(CostSignatureValue::Ground(vec![byte])),
                }),
                // Added by decision 90 (DR-135): an unpriced region, as outside native
                // execution.
                price: 0,
                caller: None,
            }],
        }
    }

    fn config() -> Par {
        SpaceConfig::new(
            SPACE.sid,
            HashMapOuter::NAME,
            QueueDiscipline::NAME,
            SpaceQualifier::Default,
        )
        .to_par()
    }

    fn handle() -> Par { namespace::gprivate(namespace::handle_channel(SPACE)) }

    fn named(name: &str, guard: Option<Par>) -> TaggedContinuation {
        TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(string(name)),
                random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
            })),
            guard,
            cost_authority: None,
        }
    }

    fn creation(config: Par) -> SpaceCreation {
        SpaceCreation {
            space: SPACE,
            config,
            handle_pattern: BindPattern {
                patterns: Vec::new(),
                remainder: Some(new_freevar_var(0)),
                free_count: 1,
            },
            handle_continuation: named("handle", None),
            random: Blake2b512Random::create_from_bytes(b"factory call"),
            authority: CostAuthority::default(),
            family: SpaceFactoryUrn::HASHMAP_QUEUE,
        }
    }

    async fn create(env: &SpaceEnv, creation: SpaceCreation) {
        Queue::default()
            .prepare_create(env, creation)
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
    }

    struct Fixture {
        space: ParRSpace,
        env: SpaceEnv,
    }

    async fn fixture() -> Fixture {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        create(&env, creation(config())).await;
        space.take_event_log().await;
        Fixture { space, env }
    }

    fn channel() -> Par { string("queue") }

    fn key_of(channel: &Par) -> CollectionKey { CollectionKey::of_channel(channel).expect("a key") }

    fn channels_of(channel: &Par) -> CollectionChannels {
        CollectionChannels::new(&HashMapOuter, SPACE, &key_of(channel))
    }

    fn channels() -> CollectionChannels { channels_of(&channel()) }

    fn send_on(channel: &Par, value: i64, persistent: bool) -> ProduceOp {
        ProduceOp {
            space: SPACE,
            key: key_of(channel),
            channel: channel.clone(),
            data: ListParWithRandom {
                pars: vec![int(value)],
                random_state: Blake2b512Random::create_from_bytes(
                    format!("send {value} {persistent}").as_bytes(),
                )
                .to_bytes(),
                cost_authority: None,
                cost_stack: None,
            },
            persistent,
            params: BTreeMap::new(),
            authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    fn send(value: i64) -> ProduceOp { send_on(&channel(), value, false) }

    fn send_forever(value: i64) -> ProduceOp { send_on(&channel(), value, true) }

    fn any() -> BindPattern {
        BindPattern {
            patterns: vec![new_freevar_par(0, Vec::new())],
            remainder: None,
            free_count: 1,
        }
    }

    fn exactly(value: i64) -> BindPattern {
        BindPattern {
            patterns: vec![int(value)],
            remainder: None,
            free_count: 0,
        }
    }

    fn receive_on(channel: &Par, name: &str, pattern: BindPattern) -> ConsumeOp {
        let continuation = named(name, None);
        let body = match continuation.tagged_cont {
            Some(TaggedCont::ParBody(body)) => body,
            Some(TaggedCont::ScalaBodyRef(_)) | None => ParWithRandom::default(),
        };
        ConsumeOp {
            space: SPACE,
            key: key_of(channel),
            channel: channel.clone(),
            pattern,
            body,
            persistent: false,
            peek: false,
            guard: None,
            query: None,
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    fn receive(name: &str, pattern: BindPattern) -> ConsumeOp {
        receive_on(&channel(), name, pattern)
    }

    fn peek(name: &str) -> ConsumeOp {
        ConsumeOp {
            peek: true,
            ..receive(name, any())
        }
    }

    fn receive_forever(name: &str) -> ConsumeOp {
        ConsumeOp {
            persistent: true,
            ..receive(name, any())
        }
    }

    async fn run(env: &SpaceEnv, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run_op(&Queue::default(), env, op).await
    }

    async fn produce(env: &SpaceEnv, op: ProduceOp) -> SpaceOutcome {
        run(env, SpaceOp::Produce(op))
            .await
            .expect("the produce commits")
    }

    async fn consume(env: &SpaceEnv, op: ConsumeOp) -> SpaceOutcome {
        run(env, SpaceOp::Consume(op))
            .await
            .expect("the consume commits")
    }

    fn body_name(continuation: &TaggedContinuation) -> String {
        match &continuation.tagged_cont {
            Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(body), ..
            })) => match body.exprs.as_slice() {
                [Expr {
                    expr_instance: Some(ExprInstance::GString(name)),
                }] => name.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }

    fn delivery(name: &str, value: i64) -> (String, Vec<Par>) {
        (name.to_string(), vec![int(value)])
    }

    /// The fire of the waiter `name` whose pattern is a literal, so it binds no value.
    fn literal_delivery(name: &str) -> (String, Vec<Par>) { (name.to_string(), Vec::new()) }

    /// The body name and the bound values of each fire.
    fn fired(outcome: &SpaceOutcome) -> Vec<(String, Vec<Par>)> {
        outcome
            .fires
            .iter()
            .map(|fire| {
                (
                    body_name(&fire.continuation),
                    fire.results
                        .iter()
                        .flat_map(|result| result.matched_datum.pars.clone())
                        .collect(),
                )
            })
            .collect()
    }

    /// The records of a collection as `(ord, payload, persistent)`, by `ord`.
    async fn records_on(space: &ParRSpace, channel: &Par) -> Vec<(i64, Vec<Par>, bool)> {
        let mut records: Vec<_> = space
            .get_data(channels_of(channel).data())
            .await
            .into_iter()
            .map(|datum| {
                let record = record::unwrap_datum(&datum.a).expect("a datum record");
                (record.ord, record.datum.pars, datum.persist)
            })
            .collect();
        records.sort_by_key(|(ord, _, _)| *ord);
        records
    }

    async fn records(space: &ParRSpace) -> Vec<(i64, Vec<Par>, bool)> {
        records_on(space, &channel()).await
    }

    /// The waiting continuations of a collection as `(ord, body, persistent, peek)`, by `ord`.
    async fn waiters_on(space: &ParRSpace, channel: &Par) -> Vec<(i64, String, bool, bool)> {
        let mut waiters: Vec<_> = space
            .get_waiting_continuations(vec![channels_of(channel).data().clone()])
            .await
            .into_iter()
            .map(|waiter| {
                let parked = record::unwrap_continuation(&waiter.continuation)
                    .expect("a continuation record");
                (
                    parked.ord,
                    body_name(&parked.continuation),
                    waiter.persist,
                    !waiter.peeks.is_empty(),
                )
            })
            .collect();
        waiters.sort_by_key(|(ord, ..)| *ord);
        waiters
    }

    async fn waiters(space: &ParRSpace) -> Vec<(i64, String, bool, bool)> {
        waiters_on(space, &channel()).await
    }

    fn waiter(ord: i64, name: &str, persistent: bool, peek: bool) -> (i64, String, bool, bool) {
        (ord, name.to_string(), persistent, peek)
    }

    async fn counter(space: &ParRSpace, channel: &Par) -> Vec<i64> {
        space
            .get_data(channel)
            .await
            .iter()
            .map(|datum| record::counter_value(&datum.a).expect("a counter record"))
            .collect()
    }

    fn name_of(hash: &Blake2b256Hash) -> &'static str {
        let ch = channels();
        let named = [
            ("C", ch.config()),
            ("S", ch.space_counter()),
            ("K", ch.counter()),
            ("D", ch.data()),
            ("H", &handle()),
        ];
        named
            .iter()
            .find(|(_, channel)| stable_hash_provider::hash(channel) == *hash)
            .map(|(name, _)| *name)
            .unwrap_or("?")
    }

    /// The events of a log as `kind channel` labels. A COMM with peeks is marked `peek`.
    fn labels(log: &[Event]) -> Vec<String> {
        log.iter()
            .map(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) => {
                    format!("produce {}", name_of(&produce.channel_hash))
                }
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    let names: Vec<&str> = consume.channel_hashes.iter().map(name_of).collect();
                    format!("consume {}", names.join(","))
                }
                Event::Comm(comm) => {
                    let names: Vec<&str> =
                        comm.consume.channel_hashes.iter().map(name_of).collect();
                    match comm.peeks.is_empty() {
                        true => format!("comm {}", names.join(",")),
                        false => format!("comm {} peek", names.join(",")),
                    }
                }
            })
            .collect()
    }

    const BUMP: [&str; 3] = ["consume K", "comm K", "produce K"];

    /// Added by decision 92: the events of an anchor, a consume of the config record that fires.
    const ANCHOR: [&str; 2] = ["consume C", "comm C"];

    /// Added by decision 92: the job of a test plan or of a bare anchor.
    fn job_of(label: &str, authority: Option<CostAuthority>) -> SpaceJob {
        SpaceJob {
            space: SPACE,
            random_state: Blake2b512Random::create_from_bytes(label.as_bytes()).to_bytes(),
            authority,
        }
    }

    /// Added by decision 92: the job of `staged_plan`.
    fn plan_job() -> SpaceJob { job_of("staged plan job", None) }

    fn bumped(rest: &[&str]) -> Vec<String> {
        BUMP.iter()
            .chain(rest)
            .map(|label| label.to_string())
            .collect()
    }

    #[tokio::test]
    async fn space_queue_creation_stages_the_config_counter_and_handle() {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        create(&env, creation(config())).await;
        assert_eq!(labels(&space.take_event_log().await), vec![
            "produce C",
            "produce S",
            "consume H"
        ]);
        let ch = channels();
        let config_records = space.get_data(ch.config()).await;
        assert_eq!(config_records.len(), 1);
        assert!(config_records[0].persist);
        assert_eq!(record::config_map(&config_records[0].a), Ok(&config()));
        assert_eq!(
            config_records[0].a.random_state,
            RandomPurpose::Config
                .derive(&creation(config()).random)
                .to_bytes()
        );
        let counters = space.get_data(ch.space_counter()).await;
        assert_eq!(counter(&space, ch.space_counter()).await, vec![0]);
        assert!(!counters[0].persist);
        assert_eq!(
            counters[0].a.random_state,
            RandomPurpose::SpaceCounter
                .derive(&creation(config()).random)
                .split_byte(1)
                .to_bytes()
        );
        let handles = space.get_waiting_continuations(vec![handle()]).await;
        assert_eq!(handles.len(), 1);
        assert!(handles[0].persist);
        assert!(handles[0].peeks.is_empty());
        assert_eq!(handles[0].continuation, named("handle", None));
        assert_eq!(handles[0].patterns, vec![creation(config()).handle_pattern]);
        assert_eq!(
            Queue::default().read_config(&env, SPACE).await,
            Ok(config())
        );

        let again = Queue::default()
            .prepare_create(&env, creation(config()))
            .await;
        assert_eq!(again.err(), Some(SpaceError::SpaceAlreadyExists));
        assert!(space.take_event_log().await.is_empty());
        assert_eq!(space.get_data(ch.config()).await.len(), 1);
    }

    #[tokio::test]
    async fn space_queue_creation_rejects_a_foreign_or_malformed_config() {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        let bag = SpaceConfig::new(SPACE.sid, "hashmap", "bag", SpaceQualifier::Default).to_par();
        let elsewhere =
            SpaceConfig::new([9; SID_LEN], "hashmap", "queue", SpaceQualifier::Default).to_par();
        for (config, expected) in [
            (bag, SpaceError::FamilyUnavailable {
                outer: "hashmap".to_string(),
                inner: "bag".to_string(),
            }),
            (
                elsewhere,
                SpaceError::Inconsistent("the config of a new space names another space"),
            ),
            (
                int(1),
                SpaceError::Inconsistent("a malformed config record"),
            ),
        ] {
            let prepared = Queue::default()
                .prepare_create(&env, creation(config))
                .await;
            assert_eq!(prepared.err(), Some(expected));
        }
        assert!(space.take_event_log().await.is_empty());
        assert!(space.get_data(channels().config()).await.is_empty());
        assert_eq!(
            Queue::default().read_config(&env, SPACE).await,
            Err(SpaceError::DanglingHandle { space: SPACE.sid })
        );
    }

    /// Decision 83 (P7 design v3, VR-2): the creation job records an ephemeral space in the book
    /// of the evaluation after its commit, inside the job. A durable space records nothing, and
    /// a creation that fails records nothing: a second creation of one space fails with
    /// `SpaceAlreadyExists`, and a config that names another space fails before the commit.
    #[tokio::test]
    async fn prepare_create_records_ephemeral_spaces_after_commit() {
        for (qualifier, durability) in [
            (SpaceQualifier::Seq, Durability::Ephemeral),
            (SpaceQualifier::Temp, Durability::Ephemeral),
            (SpaceQualifier::Default, Durability::Durable),
        ] {
            let space = NativeSpace {
                sid: SPACE.sid,
                durability,
            };
            let env = env_over(play().await, &budget());
            let config = SpaceConfig::new(
                space.sid,
                HashMapOuter::NAME,
                QueueDiscipline::NAME,
                qualifier,
            )
            .to_par();
            let prepared = Queue::default()
                .prepare_create(&env, SpaceCreation {
                    space,
                    ..creation(config.clone())
                })
                .await
                .expect("the creation stages");
            assert_eq!(
                env.volatile.qualifier(&space.sid),
                None,
                "{qualifier:?}: the book holds no space before the commit"
            );
            prepared.commit().await.expect("the creation commits");
            let recorded = match durability {
                Durability::Ephemeral => Some(qualifier),
                Durability::Durable => None,
            };
            assert_eq!(
                env.volatile.qualifier(&space.sid),
                recorded,
                "{qualifier:?}: the book after the commit"
            );
            let seq = match qualifier {
                SpaceQualifier::Seq => BTreeSet::from([space.sid]),
                SpaceQualifier::Temp | SpaceQualifier::Default => BTreeSet::new(),
            };
            assert_eq!(
                env.volatile.seq_spaces(),
                seq,
                "{qualifier:?}: the seq spaces of the book"
            );
            env.volatile.clear();
            let again = Queue::default()
                .prepare_create(&env, SpaceCreation {
                    space,
                    ..creation(config)
                })
                .await;
            assert_eq!(again.err(), Some(SpaceError::SpaceAlreadyExists));
            assert_eq!(
                env.volatile.qualifier(&space.sid),
                None,
                "{qualifier:?}: a second creation records nothing"
            );
        }
        let space = NativeSpace {
            sid: SPACE.sid,
            durability: Durability::Ephemeral,
        };
        let env = env_over(play().await, &budget());
        let elsewhere = SpaceConfig::new(
            [9; SID_LEN],
            HashMapOuter::NAME,
            QueueDiscipline::NAME,
            SpaceQualifier::Seq,
        )
        .to_par();
        let failed = Queue::default()
            .prepare_create(&env, SpaceCreation {
                space,
                ..creation(elsewhere)
            })
            .await;
        assert_eq!(
            failed.err(),
            Some(SpaceError::Inconsistent(
                "the config of a new space names another space"
            ))
        );
        assert_eq!(env.volatile.qualifier(&space.sid), None);
        assert!(
            env.volatile.seq_spaces().is_empty(),
            "a creation that fails before its commit records nothing"
        );
    }

    #[tokio::test]
    async fn space_queue_first_operation_opens_the_collection_counter() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        assert_eq!(produce(&env, send(1)).await, SpaceOutcome::default());
        assert_eq!(labels(&space.take_event_log().await), vec![
            "consume S",
            "comm S",
            "produce S",
            "produce K",
            "produce D"
        ]);
        assert_eq!(counter(&space, ch.space_counter()).await, vec![1]);
        assert_eq!(counter(&space, ch.counter()).await, vec![1]);
        assert_eq!(records(&space).await, vec![(0, vec![int(1)], false)]);

        produce(&env, send(2)).await;
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D"])
        );
        assert_eq!(counter(&space, ch.counter()).await, vec![2]);
        assert_eq!(counter(&space, ch.space_counter()).await, vec![1]);
        assert_eq!(records(&space).await, vec![
            (0, vec![int(1)], false),
            (1, vec![int(2)], false)
        ]);

        let other = string("other");
        produce(&env, send_on(&other, 3, false)).await;
        assert_eq!(counter(&space, ch.space_counter()).await, vec![2]);
        assert_eq!(counter(&space, channels_of(&other).counter()).await, vec![
            1
        ]);
        assert_eq!(counter(&space, ch.counter()).await, vec![2]);
        assert_eq!(records_on(&space, &other).await, vec![(
            0,
            vec![int(3)],
            false
        )]);
    }

    #[tokio::test]
    async fn space_queue_delivers_records_and_waiters_in_fifo_order() {
        let Fixture { space, env } = fixture().await;
        for value in [1, 2, 3] {
            produce(&env, send(value)).await;
        }
        let mut delivered = Vec::new();
        for name in ["a", "b", "c"] {
            delivered.extend(fired(&consume(&env, receive(name, any())).await));
        }
        assert_eq!(delivered, vec![
            delivery("a", 1),
            delivery("b", 2),
            delivery("c", 3)
        ]);
        assert!(records(&space).await.is_empty());

        for name in ["x", "y", "z"] {
            consume(&env, receive(name, any())).await;
        }
        assert_eq!(waiters(&space).await, vec![
            waiter(6, "x", false, false),
            waiter(7, "y", false, false),
            waiter(8, "z", false, false)
        ]);
        let mut delivered = Vec::new();
        for value in [4, 5, 6] {
            delivered.extend(fired(&produce(&env, send(value)).await));
        }
        assert_eq!(delivered, vec![
            delivery("x", 4),
            delivery("y", 5),
            delivery("z", 6)
        ]);
        assert!(waiters(&space).await.is_empty());
        assert!(records(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_data_head_blocks_later_records() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        produce(&env, send(2)).await;
        space.take_event_log().await;
        let second = ListParWithRandom {
            pars: vec![int(2)],
            ..ListParWithRandom::default()
        };
        assert!(
            SpaceMatcher::new(&env).bind(&exactly(2), &second).is_some(),
            "negative control: a scan of the records would fire the second record"
        );
        let blocked = consume(&env, receive("w", exactly(2))).await;
        assert_eq!(blocked, SpaceOutcome::default());
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["consume D"])
        );
        assert_eq!(records(&space).await.len(), 2);
        assert_eq!(waiters(&space).await, vec![waiter(2, "w", false, false)]);

        let queued = consume(&env, receive("v", exactly(1))).await;
        assert!(
            queued.fires.is_empty(),
            "a receive waits behind the head waiter, although the head record matches it"
        );
        assert_eq!(waiters(&space).await, vec![
            waiter(2, "w", false, false),
            waiter(3, "v", false, false)
        ]);
    }

    #[tokio::test]
    async fn space_queue_waiter_head_blocks_later_waiters() {
        let Fixture { space, env } = fixture().await;
        consume(&env, receive("w1", exactly(1))).await;
        consume(&env, receive("w2", any())).await;
        space.take_event_log().await;
        let sent = send(2);
        assert!(
            SpaceMatcher::new(&env).bind(&any(), &sent.data).is_some(),
            "negative control: a scan of the waiters would fire w2"
        );
        assert_eq!(produce(&env, sent).await, SpaceOutcome::default());
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D"])
        );
        assert_eq!(records(&space).await, vec![(2, vec![int(2)], false)]);
        assert_eq!(waiters(&space).await.len(), 2);

        assert_eq!(produce(&env, send(1)).await, SpaceOutcome::default());
        assert_eq!(
            records(&space).await,
            vec![(2, vec![int(2)], false), (3, vec![int(1)], false)],
            "a send joins the tail of the records, although the head waiter matches it"
        );
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20): `offer_head` does not fire the head
    /// waiter with a datum whose pair the book holds. A probe fixture with an empty book runs
    /// the same operations, and its COMM gives the pair. The fresh fixture books the pair before
    /// the send. So the send bumps the counter and stores the datum with no COMM on the data
    /// channel, and the waiter stays parked.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn offer_head_stores_a_datum_whose_head_pair_is_booked() {
        let probe = fixture().await;
        consume(&probe.env, receive("w", any())).await;
        probe.space.take_event_log().await;
        produce(&probe.env, send(1)).await;
        let log = probe.space.take_event_log().await;
        let [.., Event::Comm(comm)] = log.as_slice() else {
            panic!("the probe fires the waiter: {log:?}");
        };
        let pair = (comm.produces[0].hash.clone(), comm.consume.hash.clone());
        let Fixture { space, env } = fixture().await;
        consume(&env, receive("w", any())).await;
        space.take_event_log().await;
        env.volatile.book_pairs([pair]);
        produce(&env, send(1)).await;
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D"]),
            "the booked pair does not fire"
        );
        assert_eq!(records(&space).await, vec![(1, vec![int(1)], false)]);
        assert_eq!(waiters(&space).await, vec![waiter(0, "w", false, false)]);
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20): `receive_head` does not fire the
    /// continuation with a head record whose pair the book holds. A probe fixture with an empty
    /// book runs the same operations, and its COMM gives the pair. The fresh fixture books the
    /// pair before the receive. So the receive bumps the counter and parks the continuation with
    /// no COMM on the data channel, and the record stays stored.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn receive_head_parks_on_a_booked_head_pair() {
        let probe = fixture().await;
        produce(&probe.env, send(1)).await;
        probe.space.take_event_log().await;
        consume(&probe.env, receive("w", any())).await;
        let log = probe.space.take_event_log().await;
        let [.., Event::Comm(comm)] = log.as_slice() else {
            panic!("the probe fires the record: {log:?}");
        };
        let pair = (comm.produces[0].hash.clone(), comm.consume.hash.clone());
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        space.take_event_log().await;
        env.volatile.book_pairs([pair]);
        consume(&env, receive("w", any())).await;
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["consume D"]),
            "the booked pair does not fire"
        );
        assert_eq!(waiters(&space).await, vec![waiter(1, "w", false, false)]);
        assert_eq!(records(&space).await, vec![(0, vec![int(1)], false)]);
    }

    #[tokio::test]
    async fn space_queue_guard_of_the_head_waiter_blocks() {
        let Fixture { space, env } = fixture().await;
        let refusing = ConsumeOp {
            guard: Some(new_gbool_par(false, Vec::new(), false)),
            ..receive("w", any())
        };
        consume(&env, refusing).await;
        consume(&env, receive("v", any())).await;
        assert_eq!(produce(&env, send(1)).await, SpaceOutcome::default());
        assert_eq!(records(&space).await, vec![(2, vec![int(1)], false)]);
        assert_eq!(waiters(&space).await.len(), 2);

        let Fixture { space, env } = fixture().await;
        let admitting = ConsumeOp {
            guard: Some(new_gbool_par(true, Vec::new(), false)),
            ..receive("w", any())
        };
        consume(&env, admitting).await;
        assert_eq!(fired(&produce(&env, send(1)).await), vec![delivery("w", 1)]);
        assert!(records(&space).await.is_empty());
        assert!(waiters(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_peek_restores_the_head_record() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(5)).await;
        let stored = data_produces(&space.take_event_log().await);
        assert_eq!(stored.len(), 1);
        let peeked = consume(&env, peek("p")).await;
        assert_eq!(fired(&peeked), vec![delivery("p", 5)]);
        assert!(
            !peeked.fires[0].results[0].persistent,
            "the result keeps the flag of the host"
        );
        assert_eq!(peeked.reissue, None);
        let log = space.take_event_log().await;
        assert_eq!(
            labels(&log),
            bumped(&["consume D", "comm D peek", "produce D"]),
            "the peek fire removes the record, and the re-store produces it again (Rule E)"
        );
        assert_eq!(
            data_produces(&log),
            stored,
            "the re-store has the identity of the first produce"
        );
        assert_eq!(records(&space).await, vec![(0, vec![int(5)], false)]);
        assert!(waiters(&space).await.is_empty());

        assert_eq!(fired(&consume(&env, receive("t", any())).await), vec![
            delivery("t", 5)
        ]);
        assert!(records(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_peek_restore_registers_the_record_authority() {
        let space = play().await;
        let budget = budget();
        let _scope = budget.enter_comm_accounting_scope();
        let env = env_over(space.clone(), &budget);
        create(&env, creation(config())).await;
        let mut sending = send(5);
        sending.authority = region(2);
        sending.data.cost_authority = Some(region(2));
        produce(&env, sending).await;
        space.take_event_log().await;
        let peeking = ConsumeOp {
            introduction_authority: region(4),
            ..peek("p")
        };
        consume(&env, peeking).await;
        let log = space.take_event_log().await;
        let restore = log
            .iter()
            .rev()
            .find(|event| matches!(event, Event::IoEvent(IOEvent::Produce(_))))
            .expect("the re-store produces the record again");
        assert_eq!(
            registered(&budget, restore),
            Some(region(2)),
            "the re-store registers the authority of the record, not of the peek"
        );
        let peek_consume = log
            .iter()
            .find(|event| match event {
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    consume.channel_hashes == vec![stable_hash_provider::hash(channels().data())]
                }
                _ => false,
            })
            .expect("the peek consume");
        assert_eq!(registered(&budget, peek_consume), Some(region(4)));
    }

    #[tokio::test]
    async fn space_queue_peek_of_a_persistent_record_restores_nothing() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send_forever(5)).await;
        space.take_event_log().await;
        let peeked = consume(&env, peek("p")).await;
        assert_eq!(fired(&peeked), vec![delivery("p", 5)]);
        assert!(peeked.fires[0].results[0].persistent);
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["consume D", "comm D peek"]),
            "the fire keeps a persistent record, so nothing is stored again"
        );
        assert_eq!(records(&space).await, vec![(0, vec![int(5)], true)]);
    }

    #[tokio::test]
    async fn space_queue_peek_waiter_fire_reoffers_the_datum() {
        let Fixture { space, env } = fixture().await;
        consume(&env, peek("p")).await;
        consume(&env, receive("t", any())).await;
        space.take_event_log().await;
        let outcome = produce(&env, send(3)).await;
        assert_eq!(fired(&outcome), vec![delivery("p", 3), delivery("t", 3)]);
        assert!(
            !outcome.fires[0].results[0].persistent,
            "the incoming datum of a peek fire is not retained"
        );
        let log = space.take_event_log().await;
        assert_eq!(
            labels(&log),
            bumped(&["produce D", "comm D peek", "produce D", "comm D"])
        );
        assert_eq!(log[3], log[5], "the re-offer has the identity of the send");
        assert!(records(&space).await.is_empty());
        assert!(waiters(&space).await.is_empty());

        consume(&env, peek("q")).await;
        space.take_event_log().await;
        let outcome = produce(&env, send(4)).await;
        assert_eq!(fired(&outcome), vec![delivery("q", 4)]);
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D", "comm D peek", "produce D"])
        );
        assert_eq!(records(&space).await, vec![(4, vec![int(4)], false)]);
    }

    #[tokio::test]
    async fn space_queue_persistent_send_fires_each_waiter_then_stores() {
        let Fixture { space, env } = fixture().await;
        consume(&env, receive("w1", any())).await;
        consume(&env, receive("w2", any())).await;
        space.take_event_log().await;
        let outcome = produce(&env, send_forever(4)).await;
        assert_eq!(fired(&outcome), vec![delivery("w1", 4), delivery("w2", 4)]);
        assert_eq!(outcome.reissue, None);
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D", "comm D", "produce D", "comm D", "produce D"])
        );
        assert_eq!(records(&space).await, vec![(2, vec![int(4)], true)]);
        assert!(waiters(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_persistent_receive_drains_then_parks() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        produce(&env, send(2)).await;
        space.take_event_log().await;
        let outcome = consume(&env, receive_forever("r")).await;
        assert_eq!(fired(&outcome), vec![delivery("r", 1), delivery("r", 2)]);
        assert_eq!(outcome.reissue, None);
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["consume D", "comm D", "consume D", "comm D", "consume D"])
        );
        assert!(records(&space).await.is_empty());
        assert_eq!(waiters(&space).await, vec![waiter(2, "r", true, false)]);

        assert_eq!(fired(&produce(&env, send(3)).await), vec![delivery("r", 3)]);
        assert_eq!(waiters(&space).await, vec![waiter(2, "r", true, false)]);
        assert!(records(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_divergent_pairs_reissue_after_one_round() {
        let Fixture { space, env } = fixture().await;
        consume(&env, receive_forever("r")).await;
        space.take_event_log().await;
        let outcome = produce(&env, send_forever(9)).await;
        assert_eq!(fired(&outcome), vec![delivery("r", 9)]);
        assert_eq!(
            outcome.reissue,
            Some(SpaceReissue {
                op: reissued_send(send_forever(9), 1),
                cause: ReissueCause::DivergentPair,
            })
        );
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D", "comm D"])
        );
        assert!(records(&space).await.is_empty());
        assert_eq!(waiters(&space).await, vec![waiter(0, "r", true, false)]);
        let reissue = outcome.reissue.expect("a divergent pair re-issues");
        let again = run(&env, reissue.op).await.expect("the re-issue commits");
        assert_eq!(fired(&again), vec![delivery("r", 9)]);
        assert_eq!(
            again.reissue,
            Some(SpaceReissue {
                op: reissued_send(send_forever(9), 1),
                cause: ReissueCause::DivergentPair,
            }),
            "every round keeps the order of the first round"
        );
        assert_eq!(counter(&space, channels().counter()).await, vec![3]);

        let Fixture { space, env } = fixture().await;
        assert_eq!(
            produce(&env, send_forever(8)).await,
            SpaceOutcome::default()
        );
        let outcome = consume(&env, receive_forever("s")).await;
        assert_eq!(fired(&outcome), vec![delivery("s", 8)]);
        assert_eq!(
            outcome.reissue,
            Some(SpaceReissue {
                op: SpaceOp::Consume(ConsumeOp {
                    reissued_ord: Some(1),
                    ..receive_forever("s")
                }),
                cause: ReissueCause::DivergentPair,
            })
        );
        assert_eq!(records(&space).await, vec![(0, vec![int(8)], true)]);
        assert!(waiters(&space).await.is_empty());
        let peeked = consume(&env, peek("p")).await;
        assert_eq!(fired(&peeked), vec![delivery("p", 8)]);
        assert_eq!(peeked.reissue, None, "a non-persistent receive is spent");
    }

    #[tokio::test]
    async fn space_queue_round_cap_reissues_the_rest() {
        let Fixture { space, env } = fixture().await;
        let waiting = FIRE_ROUND_CAP + 1;
        for index in 0..waiting {
            consume(&env, receive(&format!("w{index}"), any())).await;
        }
        space.take_event_log().await;
        let outcome = produce(&env, send_forever(1)).await;
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        let expected: Vec<_> = (0..FIRE_ROUND_CAP)
            .map(|index| delivery(&format!("w{index}"), 1))
            .collect();
        assert_eq!(fired(&outcome), expected);
        let last = i64::try_from(FIRE_ROUND_CAP).expect("a small cap");
        assert_eq!(
            outcome.reissue,
            Some(SpaceReissue {
                op: reissued_send(send_forever(1), last + 1),
                cause: ReissueCause::RoundCap,
            })
        );
        assert_eq!(space.take_event_log().await.len(), 3 + 2 * FIRE_ROUND_CAP);
        assert!(records(&space).await.is_empty());
        assert_eq!(waiters(&space).await, vec![waiter(
            last,
            &format!("w{FIRE_ROUND_CAP}"),
            false,
            false
        )]);
        let rest = run(
            &env,
            outcome.reissue.expect("the chain re-issues the rest").op,
        )
        .await
        .expect("the re-issue commits");
        assert_eq!(fired(&rest), vec![delivery(
            &format!("w{FIRE_ROUND_CAP}"),
            1
        )]);
        assert_eq!(rest.reissue, None);
        assert_eq!(
            records(&space).await,
            vec![(last + 1, vec![int(1)], true)],
            "the persistent record keeps the order of its first round"
        );

        let Fixture { space, env } = fixture().await;
        for value in 0..=last {
            produce(&env, send(value)).await;
        }
        let outcome = consume(&env, receive_forever("r")).await;
        let expected: Vec<_> = (0..last).map(|value| delivery("r", value)).collect();
        assert_eq!(fired(&outcome), expected);
        assert_eq!(
            outcome.reissue.as_ref().map(|reissue| reissue.cause),
            Some(ReissueCause::RoundCap)
        );
        assert_eq!(records(&space).await, vec![(last, vec![int(last)], false)]);
        assert!(waiters(&space).await.is_empty());
        let rest = run(&env, outcome.reissue.expect("a re-issue").op)
            .await
            .expect("the re-issue commits");
        assert_eq!(fired(&rest), vec![delivery("r", last)]);
        assert!(records(&space).await.is_empty());
        assert_eq!(waiters(&space).await, vec![waiter(
            last + 1,
            "r",
            true,
            false
        )]);
    }

    /// Parks `count` plain waiters `w{first}` .. on the collection. Each takes every value.
    async fn park_plain(env: &SpaceEnv, first: usize, count: usize) {
        for index in first..first + count {
            consume(env, receive(&format!("w{index}"), any())).await;
        }
    }

    /// Parks `count` peek waiters `p0` .. on the collection. Each takes every value.
    async fn park_peeks(env: &SpaceEnv, count: usize) {
        for index in 0..count {
            consume(env, ConsumeOp {
                peek: true,
                ..receive(&format!("p{index}"), any())
            })
            .await;
        }
    }

    /// `FIRE_ROUND_CAP` as an order.
    fn cap() -> i64 { i64::try_from(FIRE_ROUND_CAP).expect("a small cap") }

    /// The re-issue of the persistent send `op` with the first-round order `ord` (decision 43).
    fn reissued_send(op: ProduceOp, ord: i64) -> SpaceOp {
        SpaceOp::Produce(ProduceOp {
            reissued_ord: Some(ord),
            ..op
        })
    }

    /// The produce events of a log on the data channel of the collection.
    fn data_produces(log: &[Event]) -> Vec<Produce> {
        let data = stable_hash_provider::hash(channels().data());
        log.iter()
            .filter_map(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == data => {
                    Some(produce.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn space_queue_reissue_keeps_the_first_round_ord_and_identity() {
        let Fixture { space, env } = fixture().await;
        park_plain(&env, 0, FIRE_ROUND_CAP + 1).await;
        let first = cap() + 1;
        space.take_event_log().await;
        let outcome = produce(&env, send_forever(1)).await;
        assert_eq!(
            outcome.reissue,
            Some(SpaceReissue {
                op: reissued_send(send_forever(1), first),
                cause: ReissueCause::RoundCap,
            })
        );
        let round_one = data_produces(&space.take_event_log().await);
        assert_eq!(round_one.len(), FIRE_ROUND_CAP);
        let rest = run(&env, outcome.reissue.expect("a re-issue").op)
            .await
            .expect("the re-issue commits");
        assert_eq!(fired(&rest), vec![delivery(
            &format!("w{FIRE_ROUND_CAP}"),
            1
        )]);
        assert_eq!(rest.reissue, None);
        let round_two = data_produces(&space.take_event_log().await);
        assert_eq!(round_two.len(), 2, "the fire and the store");
        assert!(
            round_one
                .iter()
                .chain(&round_two)
                .all(|produce| *produce == round_one[0]),
            "every round stages the same record, so the same identity"
        );
        assert_eq!(records(&space).await, vec![(first, vec![int(1)], true)]);
        assert_eq!(
            counter(&space, channels().counter()).await,
            vec![first + 2],
            "each round bumps the counter"
        );
    }

    #[tokio::test]
    async fn space_queue_reissue_reenters_ahead_of_younger_records() {
        let Fixture { space, env } = fixture().await;
        park_plain(&env, 0, FIRE_ROUND_CAP).await;
        consume(&env, receive("one", exactly(1))).await;
        let first = cap() + 1;
        let outcome = produce(&env, send_forever(1)).await;
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        let reissue = outcome.reissue.expect("the cap re-issues the send");
        // A younger datum arrives between the rounds. The head waiter takes only 1, so the datum
        // is stored while the send is in flight.
        assert_eq!(produce(&env, send(7)).await, SpaceOutcome::default());
        assert_eq!(records(&space).await, vec![(
            first + 1,
            vec![int(7)],
            false
        )]);
        let rest = run(&env, reissue.op).await.expect("the re-issue commits");
        assert_eq!(
            fired(&rest),
            vec![literal_delivery("one")],
            "the send re-enters at its place, ahead of 7"
        );
        assert_eq!(rest.reissue, None);
        assert_eq!(records(&space).await, vec![
            (first, vec![int(1)], true),
            (first + 1, vec![int(7)], false)
        ]);
        assert!(waiters(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_reissue_stores_behind_an_older_reentry() {
        let Fixture { space, env } = fixture().await;
        park_plain(&env, 0, 2 * FIRE_ROUND_CAP).await;
        consume(&env, receive("two", exactly(2))).await;
        let (older_ord, younger_ord) = (2 * cap() + 1, 2 * cap() + 2);
        let older = produce(&env, send_forever(1))
            .await
            .reissue
            .expect("the first send re-issues");
        let younger = produce(&env, send_forever(2))
            .await
            .reissue
            .expect("the second send re-issues");
        assert_eq!(older.op, reissued_send(send_forever(1), older_ord));
        assert_eq!(younger.op, reissued_send(send_forever(2), younger_ord));
        // The older send re-enters first. The head waiter takes only 2, so the older send is
        // stored at the head. The younger send then finds an older record at the head, so it
        // is stored at its place behind that record and fires nothing, although the head
        // waiter takes 2.
        let stored = run(&env, older.op)
            .await
            .expect("the older re-entry commits");
        assert_eq!(stored, SpaceOutcome::default());
        let behind = run(&env, younger.op)
            .await
            .expect("the younger re-entry commits");
        assert_eq!(behind, SpaceOutcome::default());
        assert_eq!(records(&space).await, vec![
            (older_ord, vec![int(1)], true),
            (younger_ord, vec![int(2)], true)
        ]);
        assert_eq!(waiters(&space).await, vec![waiter(
            2 * cap(),
            "two",
            false,
            false
        )]);
    }

    #[tokio::test]
    async fn space_queue_shadowed_pair_resolves_at_reentry() {
        let Fixture { space, env } = fixture().await;
        park_peeks(&env, FIRE_ROUND_CAP).await;
        consume(&env, receive("one", exactly(1))).await;
        let first = cap() + 1;
        let outcome = produce(&env, send_forever(1)).await;
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        let reissue = outcome.reissue.expect("the peek waiters reach the cap");
        produce(&env, send(2)).await;
        for index in 0..FIRE_ROUND_CAP {
            consume(&env, receive(&format!("z{index}"), any())).await;
        }
        // Round 2 re-enters ahead of 2. It fires the head waiter and 63 waiters behind it, and
        // it reaches the cap again.
        let outcome = run(&env, reissue.op).await.expect("round 2 commits");
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        assert_eq!(fired(&outcome)[0], literal_delivery("one"));
        let reissue = outcome.reissue.expect("round 2 reaches the cap");
        assert_eq!(reissue.op, reissued_send(send_forever(1), first));
        // The heads 2 and z63 match. The send in flight precedes the head record, so it shadows
        // the pair.
        let last = format!("z{}", FIRE_ROUND_CAP - 1);
        assert_eq!(records(&space).await, vec![(
            first + 1,
            vec![int(2)],
            false
        )]);
        assert_eq!(waiters(&space).await, vec![waiter(
            first + 1 + cap(),
            &last,
            false,
            false
        )]);
        // Round 3 re-enters ahead of 2 again, fires z63, and stores the send at its place.
        let outcome = run(&env, reissue.op).await.expect("round 3 commits");
        assert_eq!(fired(&outcome), vec![delivery(&last, 1)]);
        assert_eq!(outcome.reissue, None);
        assert_eq!(records(&space).await, vec![
            (first, vec![int(1)], true),
            (first + 1, vec![int(2)], false)
        ]);
        assert!(waiters(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_nonpersistent_reissue_takes_a_fresh_ord() {
        let Fixture { space, env } = fixture().await;
        park_peeks(&env, FIRE_ROUND_CAP).await;
        consume(&env, receive("one", exactly(1))).await;
        let outcome = produce(&env, send(1)).await;
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        let reissue = outcome.reissue.expect("the peek waiters reach the cap");
        assert_eq!(
            reissue.op,
            SpaceOp::Produce(send(1)),
            "a datum that is not persistent keeps no order"
        );
        produce(&env, send(2)).await;
        consume(&env, receive("z", any())).await;
        // The re-issued datum takes the order of its new bump, so it joins the tail behind 2. A
        // datum that kept its first order would fire the head waiter and leave the matching
        // heads 2 and z with nothing in flight.
        let rest = run(&env, reissue.op).await.expect("the re-issue commits");
        assert_eq!(rest, SpaceOutcome::default());
        assert_eq!(records(&space).await, vec![
            (cap() + 2, vec![int(2)], false),
            (cap() + 4, vec![int(1)], false)
        ]);
        assert_eq!(waiters(&space).await, vec![
            waiter(cap(), "one", false, false),
            waiter(cap() + 3, "z", false, false)
        ]);
    }

    // Disabled by decision 132: the user withdrew plan v2 C7.12, so a re-issued element that
    // is not persistent takes a fresh ord again (decision 43). The test
    // `space_queue_nonpersistent_reissue_takes_a_fresh_ord` pins that rule. This test pinned
    // the rule of C7.12.

    #[tokio::test]
    async fn space_queue_reissued_receive_keeps_its_first_round_ord() {
        let Fixture { space, env } = fixture().await;
        for value in 0..cap() {
            produce(&env, send(value)).await;
        }
        produce(&env, send(100)).await;
        let first = cap() + 1;
        let outcome = consume(&env, receive_forever("r")).await;
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        let reissue = outcome.reissue.expect("the drain reaches the cap");
        assert_eq!(
            reissue.op,
            SpaceOp::Consume(ConsumeOp {
                reissued_ord: Some(first),
                ..receive_forever("r")
            })
        );
        // A younger waiter parks behind the record 100 while the receive is in flight.
        consume(&env, receive("young", exactly(7))).await;
        let rest = run(&env, reissue.op).await.expect("the re-issue commits");
        assert_eq!(
            fired(&rest),
            vec![delivery("r", 100)],
            "the receive re-enters ahead of the younger waiter"
        );
        assert_eq!(rest.reissue, None);
        assert!(records(&space).await.is_empty());
        assert_eq!(waiters(&space).await, vec![
            waiter(first, "r", true, false),
            waiter(first + 1, "young", false, false)
        ]);
    }

    #[tokio::test]
    async fn space_queue_reentry_rejects_an_inconsistent_order() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(5)).await;
        for ord in [-1, 2, i64::MAX] {
            assert_eq!(
                run(&env, reissued_send(send_forever(1), ord)).await,
                Err(SpaceError::Inconsistent(
                    "a re-issued order is not an order of the counter"
                )),
                "the order {ord}"
            );
        }
        assert_eq!(
            run(&env, reissued_send(send_forever(1), 0)).await,
            Err(SpaceError::Inconsistent(
                "a re-issued element shares its order with a stored entry"
            ))
        );
        assert_eq!(
            records(&space).await,
            vec![(0, vec![int(5)], false)],
            "a failed operation changes nothing"
        );
        assert_eq!(counter(&space, channels().counter()).await, vec![1]);
    }

    // Retired by decision 90 (C6, step D): the replay tests of the queue ran through the rigged
    // replay modes, which retire (decision 125). Native replay of re-entries and of the queue:
    // `space_native_persistent_retry_rows`, `space_native_replay_equal_hashmap`. The text before:
    // ```
    // /// Runs `op`, then runs its re-issues while `rounds` allows, and returns every outcome.
    // async fn run_rounds(
    //     env: &SpaceEnv,
    //     op: SpaceOp,
    //     rounds: usize,
    // ) -> Result<Vec<SpaceOutcome>, SpaceError> {
    //     let mut outcomes = Vec::with_capacity(rounds);
    //     let mut next = Some(op);
    //     for _ in 0..rounds {
    //         let Some(op) = next.take() else {
    //             break;
    //         };
    //         let outcome = run(env, op).await?;
    //         next = outcome.reissue.as_ref().map(|reissue| reissue.op.clone());
    //         outcomes.push(outcome);
    //     }
    //     Ok(outcomes)
    // }
    //
    // /// A script of re-entries: a divergent pair for three rounds, and a capped send that
    // /// re-enters ahead of a younger record.
    // async fn run_reentry_script(env: &SpaceEnv) -> Result<Vec<SpaceOutcome>, SpaceError> {
    //     let divergent = string("divergent");
    //     let capped = string("capped");
    //     let mut outcomes = Vec::new();
    //     outcomes.push(
    //         run(
    //             env,
    //             SpaceOp::Consume(ConsumeOp {
    //                 persistent: true,
    //                 ..receive_on(&divergent, "s", any())
    //             }),
    //         )
    //         .await?,
    //     );
    //     outcomes.extend(run_rounds(env, SpaceOp::Produce(send_on(&divergent, 8, true)), 3).await?);
    //     for index in 0..FIRE_ROUND_CAP {
    //         outcomes.push(
    //             run(
    //                 env,
    //                 SpaceOp::Consume(receive_on(&capped, &format!("w{index}"), any())),
    //             )
    //             .await?,
    //         );
    //     }
    //     outcomes.push(
    //         run(
    //             env,
    //             SpaceOp::Consume(receive_on(&capped, "one", exactly(1))),
    //         )
    //         .await?,
    //     );
    //     let first = run(env, SpaceOp::Produce(send_on(&capped, 1, true))).await?;
    //     let reissue = first.reissue.clone().ok_or(SpaceError::Inconsistent(
    //         "the capped send does not re-issue",
    //     ))?;
    //     outcomes.push(first);
    //     outcomes.push(run(env, SpaceOp::Produce(send_on(&capped, 7, false))).await?);
    //     outcomes.push(run(env, reissue.op).await?);
    //     Ok(outcomes)
    // }
    //
    // #[tokio::test]
    // async fn space_queue_replay_reproduces_the_reentries() {
    //     let (play, replay) = play_and_replay().await;
    //     let play_env = env_over(play.clone(), &budget());
    //     create(&play_env, creation(config())).await;
    //     let start = play
    //         .create_checkpoint()
    //         .await
    //         .expect("a checkpoint after the creation")
    //         .root;
    //     let played = run_reentry_script(&play_env)
    //         .await
    //         .expect("the script plays");
    //     let divergent: Vec<_> = played[1..4]
    //         .iter()
    //         .map(|outcome| outcome.reissue.as_ref().map(|reissue| reissue.cause))
    //         .collect();
    //     assert_eq!(divergent, vec![Some(ReissueCause::DivergentPair); 3]);
    //     assert_eq!(fired(played.last().expect("the re-entry")), vec![
    //         literal_delivery("one")
    //     ]);
    //     let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
    //     let divergent_comms: Vec<&COMM> = log
    //         .iter()
    //         .filter_map(|event| match event {
    //             Event::Comm(comm)
    //                 if comm.consume.channel_hashes
    //                     == vec![stable_hash_provider::hash(
    //                         channels_of(&string("divergent")).data(),
    //                     )] =>
    //             {
    //                 Some(comm)
    //             }
    //             _ => None,
    //         })
    //         .collect();
    //     assert_eq!(divergent_comms.len(), 3);
    //     assert!(
    //         divergent_comms
    //             .iter()
    //             .all(|comm| *comm == divergent_comms[0]),
    //         "the rounds of a divergent pair repeat one COMM"
    //     );
    //
    //     replay
    //         .rig_and_reset(start, log)
    //         .await
    //         .expect("the replay rigs the play log");
    //     let replay_env = env_over(replay.clone(), &budget());
    //     let replayed = run_reentry_script(&replay_env)
    //         .await
    //         .expect("the script replays");
    //     assert_eq!(replayed, played);
    //     replay
    //         .check_replay_data()
    //         .await
    //         .expect("every recorded COMM is paired, the repeated ones included");
    //     assert_eq!(
    //         replay.create_checkpoint().await.expect("a checkpoint").root,
    //         root
    //     );
    // }
    // ```

    #[tokio::test]
    async fn space_queue_every_operation_bumps_the_counter() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        produce(&env, send(1)).await;
        assert_eq!(counter(&space, ch.counter()).await, vec![1]);
        space.take_event_log().await;
        let operations = [
            SpaceOp::Consume(peek("p")),
            SpaceOp::Consume(receive("t", any())),
            SpaceOp::Consume(receive("w", exactly(4))),
            SpaceOp::Produce(send(2)),
        ];
        for (index, op) in operations.into_iter().enumerate() {
            run(&env, op).await.expect("the operation commits");
            let log = labels(&space.take_event_log().await);
            assert_eq!(&log[..3], &BUMP.map(str::to_string), "operation {index}");
            let expected = i64::try_from(index).expect("a small index") + 2;
            assert_eq!(counter(&space, ch.counter()).await, vec![expected]);
        }
        assert_eq!(counter(&space, ch.space_counter()).await, vec![1]);
    }

    #[tokio::test]
    async fn space_queue_dangling_space_fails_closed() {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        let name = HashMapOuter
            .gensym(SPACE, &Blake2b512Random::create_from_bytes(b"a name"))
            .expect("a name");
        for (op, expected) in [
            (
                SpaceOp::Produce(send_on(&name, 1, false)),
                SpaceError::DanglingName { space: SPACE.sid },
            ),
            (SpaceOp::Produce(send(1)), SpaceError::DanglingHandle {
                space: SPACE.sid,
            }),
            (
                SpaceOp::Consume(receive_on(&name, "w", any())),
                SpaceError::DanglingName { space: SPACE.sid },
            ),
            (
                SpaceOp::Consume(receive("w", any())),
                SpaceError::DanglingHandle { space: SPACE.sid },
            ),
        ] {
            assert_eq!(run(&env, op).await, Err(expected));
        }
        assert_eq!(
            Queue::default().read_config(&env, SPACE).await,
            Err(SpaceError::DanglingHandle { space: SPACE.sid })
        );
        assert!(space.take_event_log().await.is_empty());
        assert!(space.get_data(channels().space_counter()).await.is_empty());
    }

    // Stages `config_records` on `C(sid)` and the space counter 0, outside the agent.
    async fn install(env: &SpaceEnv, config_records: Vec<ListParWithRandom>) {
        let ch = channels();
        let random = Blake2b512Random::create_from_bytes(b"install");
        let mut txn = env
            .store
            .begin(&[ch.config().clone(), ch.space_counter().clone()])
            .await
            .expect("a transaction");
        for data in config_records {
            txn.produce(SpaceProduce {
                channel: ch.config().clone(),
                data,
                persist: true,
                outcome: ProduceOutcome::Store,
                before_observe: None,
                own: false,
                // Added by decision 123 (plan v4, V3-9): no row search.
                search: None,
            })
            .expect("the config stages");
        }
        txn.produce(SpaceProduce {
            channel: ch.space_counter().clone(),
            data: record::counter_record(0, &random, None),
            persist: false,
            outcome: ProduceOutcome::Store,
            before_observe: None,
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        })
        .expect("the counter stages");
        txn.commit().await.expect("the records commit");
    }

    #[tokio::test]
    async fn space_queue_rejects_a_foreign_family_or_a_corrupt_config() {
        let random = Blake2b512Random::create_from_bytes(b"config");
        let bag = SpaceConfig::new(SPACE.sid, "hashmap", "bag", SpaceQualifier::Default).to_par();
        let elsewhere =
            SpaceConfig::new([1; SID_LEN], "hashmap", "queue", SpaceQualifier::Default).to_par();
        let cases = [
            (
                vec![record::config_record(bag, &random)],
                SpaceError::FamilyUnavailable {
                    outer: "hashmap".to_string(),
                    inner: "bag".to_string(),
                },
            ),
            (
                vec![record::config_record(elsewhere, &random)],
                SpaceError::Inconsistent("the config record names another space"),
            ),
            (
                vec![
                    record::config_record(config(), &random),
                    record::config_record(config(), &random.split_byte(1)),
                ],
                SpaceError::Inconsistent("a space has two config records"),
            ),
            (
                vec![record::counter_record(0, &random, None)],
                SpaceError::Inconsistent("a malformed config record"),
            ),
        ];
        for (config_records, expected) in cases {
            let space = play().await;
            let env = env_over(space.clone(), &budget());
            install(&env, config_records).await;
            space.take_event_log().await;
            assert_eq!(
                run(&env, SpaceOp::Produce(send(1))).await,
                Err(expected.clone())
            );
            assert_eq!(
                run(&env, SpaceOp::Consume(receive("w", any()))).await,
                Err(expected)
            );
            assert!(space.take_event_log().await.is_empty());
        }
    }

    #[tokio::test]
    /// A join group that holds a channel outside the namespace of the space is foreign: the
    /// classification of P5c finds it while it computes the closure, and every operation on the
    /// collection fails with `Inconsistent` before it stages anything (decision 29). The P4
    /// version of this test expected the rejection of every join group.
    async fn space_queue_foreign_join_is_inconsistent() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        let ch = channels();
        let mut group = vec![ch.data().clone(), int(5)];
        group.sort_by_key(stable_hash_provider::hash);
        let mut txn = env.store.begin(&group).await.expect("a transaction");
        txn.consume(SpaceConsume {
            channels: group.clone(),
            patterns: vec![any(), any()],
            continuation: named("join", None),
            persist: false,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Park,
            before_observe: None,
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        })
        .expect("the join parks");
        txn.commit().await.expect("the join commits");
        space.take_event_log().await;
        let joined = SpaceError::Inconsistent("a collection channel is part of a foreign join");
        assert_eq!(
            run(&env, SpaceOp::Produce(send(2))).await,
            Err(joined.clone())
        );
        assert_eq!(
            run(&env, SpaceOp::Consume(receive("w", any()))).await,
            Err(joined)
        );
        assert!(space.take_event_log().await.is_empty());
        assert_eq!(records(&space).await, vec![(0, vec![int(1)], false)]);
    }

    #[tokio::test]
    async fn space_queue_reads_stage_nothing_and_never_commit() {
        let Fixture { space, env } = fixture().await;
        let order = OperationOrder {
            session: [3; 32],
            path: CausalPath::from(vec![(0, 0)]),
        };
        let read =
            operation_context::scope(order.clone(), Queue::default().read_config(&env, SPACE))
                .await;
        assert_eq!(read, Ok(config()));
        assert!(space.take_event_log().await.is_empty());
        let first =
            operation_context::scope(order.clone(), run(&env, SpaceOp::Produce(send(1)))).await;
        assert_eq!(
            first,
            Ok(SpaceOutcome::default()),
            "the read claimed no parent order"
        );
        let second = operation_context::scope(order, run(&env, SpaceOp::Produce(send(2)))).await;
        assert_eq!(
            second,
            Err(SpaceError::Substrate(SpaceTxnError::ParentOrderReused)),
            "negative control: a second commit under one parent order is refused"
        );
    }

    #[tokio::test]
    async fn space_queue_names_route_to_their_space() {
        let Fixture { space, env } = fixture().await;
        let random = Blake2b512Random::create_from_bytes(b"gensym call");
        let name = Queue::default()
            .gensym(SPACE, &random)
            .expect("a name of the space");
        assert_eq!(HashMapOuter.gensym(SPACE, &random), Ok(name.clone()));
        produce(&env, send_on(&name, 5, false)).await;
        assert_eq!(records_on(&space, &name).await, vec![(
            0,
            vec![int(5)],
            false
        )]);
        assert_eq!(
            fired(&consume(&env, receive_on(&name, "n", any())).await),
            vec![delivery("n", 5)]
        );
        assert!(records_on(&space, &name).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_fires_dispatch_the_original_continuation_and_datum() {
        let Fixture { env, .. } = fixture().await;
        let waiting = ConsumeOp {
            guard: Some(new_gbool_par(true, Vec::new(), false)),
            authority: region(5),
            ..receive("w", any())
        };
        consume(&env, waiting.clone()).await;
        let sent = send(7);
        let outcome = produce(&env, sent.clone()).await;
        let [fire] = outcome.fires.as_slice() else {
            panic!("one fire: {outcome:?}");
        };
        assert_eq!(fire.continuation, TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(waiting.body.clone())),
            guard: waiting.guard.clone(),
            cost_authority: Some(region(5)),
        });
        let [result] = fire.results.as_slice() else {
            panic!("one result: {fire:?}");
        };
        assert_eq!(result.matched_datum, sent.data);
        assert_eq!(result.removed_datum, record::wrap_datum(1, &sent.data));
        assert_eq!(result.channel, *channels().data());
        assert!(!result.persistent);

        let stored = send(8);
        produce(&env, stored.clone()).await;
        let outcome = consume(&env, receive("t", any())).await;
        let [fire] = outcome.fires.as_slice() else {
            panic!("one fire: {outcome:?}");
        };
        assert_eq!(fire.continuation, named("t", None));
        assert_eq!(fire.results[0].matched_datum, stored.data);
        assert_eq!(
            fire.results[0].removed_datum,
            record::wrap_datum(2, &stored.data)
        );
    }

    fn registered(budget: &RuntimeBudget, event: &Event) -> Option<CostAuthority> {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => budget
                .introduction_authority(
                    produce_introduction_identity(produce),
                    AuthorityByteEventKind::ProduceIntroduction,
                )
                .ok(),
            Event::IoEvent(IOEvent::Consume(consume)) => budget
                .introduction_authority(
                    consume_introduction_identity(consume),
                    AuthorityByteEventKind::ConsumeIntroduction,
                )
                .ok(),
            Event::Comm(_) => None,
        }
    }

    // Changed by decision 124 (plan v2 C7.2 and C7.3, decision 99): the re-pins of 2.13 check
    // each registration by its channel with `assert_registered_on`, so this helper has no
    // caller. The text before:
    // ```
    // fn assert_registered(budget: &RuntimeBudget, log: &[Event], authority: &CostAuthority) {
    //     let mut checked = 0;
    //     for event in log {
    //         if let Event::IoEvent(_) = event {
    //             assert_eq!(
    //                 registered(budget, event).as_ref(),
    //                 Some(authority),
    //                 "{event:?}"
    //             );
    //             checked += 1;
    //         }
    //     }
    //     assert!(checked > 0);
    // }
    // ```

    /// Added by decision 124 (plan v2 C7.1, decision 99): the machinery seal of the authority
    /// `authority` of a payer on `budget`, built as `Introduction::seal_of` builds it: the
    /// system residue of each region for the deployment of `budget` (2.13).
    fn machinery_seal(budget: &RuntimeBudget, authority: &CostAuthority) -> CostAuthority {
        use crate::rust::interpreter::accounting::authority::system_residue_authority_with_caller;
        system_residue_authority_with_caller(authority, &budget.deploy_id(), None)
            .expect("the machinery seal of a region builds")
    }

    /// Added by decision 124 (plan v2 C7.2, C7.3, decision 99, rule CR-15): asserts that each
    /// produce and each consume of `log` on `channel` registers `authority`, and that each
    /// other one, a machinery row, registers the payer introduction. The payer carries no
    /// authority, so its row registers the fallback authority of its identity, which a budget
    /// without registrations also gives.
    fn assert_registered_on(
        budget: &RuntimeBudget,
        log: &[Event],
        channel: &Par,
        authority: &CostAuthority,
    ) {
        let hash = stable_hash_provider::hash(channel);
        let unregistered = self::budget();
        let mut checked = 0;
        for event in log {
            let on_channel = match event {
                Event::IoEvent(IOEvent::Produce(produce)) => produce.channel_hash == hash,
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    match consume.channel_hashes.as_slice() {
                        [only] => *only == hash,
                        _ => false,
                    }
                }
                Event::Comm(_) => continue,
            };
            let expected = match on_channel {
                true => Some(authority.clone()),
                false => registered(&unregistered, event),
            };
            assert_eq!(registered(budget, event), expected, "{event:?}");
            checked += 1;
        }
        assert!(checked > 0);
    }

    #[tokio::test]
    async fn space_queue_records_carry_and_register_the_b7_authorities() {
        let space = play().await;
        let budget = budget();
        let _scope = budget.enter_comm_accounting_scope();
        let env = env_over(space.clone(), &budget);
        let ch = channels();
        create(&env, SpaceCreation {
            authority: region(1),
            ..creation(config())
        })
        .await;
        // Changed by decision 124 (plan v2 C7.3, decision 99): row 0 keeps the introduction of
        // the creation, and each row after it registers the payer introduction. The text
        // before:
        // assert_registered(&budget, &space.take_event_log().await, &region(1));
        assert_registered_on(
            &budget,
            &space.take_event_log().await,
            ch.config(),
            &region(1),
        );
        assert_eq!(space.get_data(ch.config()).await[0].a.cost_authority, None);
        assert_eq!(
            space.get_data(ch.space_counter()).await[0].a.cost_authority,
            // Changed by decision 124 (plan v2 C7.3, decision 99): the machinery seal. The text
            // before:
            // Some(region(1))
            Some(machinery_seal(&budget, &region(1)))
        );
        let handles = space.get_waiting_continuations(vec![handle()]).await;
        assert_eq!(handles[0].continuation.cost_authority, None);

        let mut sending = send(1);
        sending.authority = region(2);
        sending.data.cost_authority = Some(region(2));
        produce(&env, sending.clone()).await;
        let log = space.take_event_log().await;
        // Changed by decision 124 (plan v2 C7.2, decision 99): the counter take and the counter
        // puts register the payer introduction. The text before:
        // assert_registered(&budget, &log, &region(2));
        assert_registered_on(&budget, &log, ch.data(), &region(2));
        let records = space.get_data(ch.data()).await;
        assert_eq!(records[0].a.cost_authority, Some(region(2)));
        assert_eq!(
            space.get_data(ch.counter()).await[0].a.cost_authority,
            // Changed by decision 124 (plan v2 C7.2, decision 99): the machinery seal. The text
            // before:
            // Some(region(2))
            Some(machinery_seal(&budget, &region(2)))
        );
        let opening = Consume::create(
            &vec![ch.space_counter().clone()],
            &vec![counter_pattern()],
            &counter_continuation(
                0,
                &RandomPurpose::SpaceCounter
                    .derive(&Blake2b512Random::from_bytes(&sending.data.random_state))
                    .split_byte(0),
                // Changed by decision 124 (plan v2 C7.2, decision 99): the machinery seal. The
                // text before:
                // Some(region(2)),
                Some(machinery_seal(&budget, &region(2))),
            ),
            false,
        );
        assert_eq!(log[0], Event::IoEvent(IOEvent::Consume(opening)));

        let parking = ConsumeOp {
            authority: region(3),
            introduction_authority: region(4),
            ..receive("w", exactly(99))
        };
        consume(&env, parking.clone()).await;
        let log = space.take_event_log().await;
        // Changed by decision 124 (plan v2 C7.2, decision 99): the counter take and the counter
        // put register the payer introduction. The text before:
        // assert_registered(&budget, &log, &region(4));
        assert_registered_on(&budget, &log, ch.data(), &region(4));
        let parked = space
            .get_waiting_continuations(vec![ch.data().clone()])
            .await;
        assert_eq!(parked[0].continuation.cost_authority, Some(region(3)));
        assert_eq!(
            space.get_data(ch.counter()).await[0].a.cost_authority,
            // Changed by decision 124 (plan v2 C7.2, decision 99): the machinery seal. The text
            // before:
            // Some(region(4))
            Some(machinery_seal(&budget, &region(4)))
        );
        let bump = Consume::create(
            &vec![ch.counter().clone()],
            &vec![counter_pattern()],
            &counter_continuation(
                1,
                &RandomPurpose::Counter
                    .derive(&Blake2b512Random::from_bytes(&parking.body.random_state))
                    .split_byte(0),
                // Changed by decision 124 (plan v2 C7.2, decision 99): the machinery seal. The
                // text before:
                // Some(region(3)),
                Some(machinery_seal(&budget, &region(3))),
            ),
            false,
        );
        assert_eq!(log[0], Event::IoEvent(IOEvent::Consume(bump)));
    }

    /// Added by decision 124 (plan v2 C7.3, decision 99, rule CR-15): row 0 of a creation, the
    /// config produce, keeps the introduction of the creation. The space counter, the counter
    /// row of the allocator and the handle park register the payer introduction, and the two
    /// counter records carry the machinery seal of the creation authority.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn creation_rows_after_row_0_take_the_payer_introduction() {
        let space = play().await;
        let budget = budget();
        let _scope = budget.enter_comm_accounting_scope();
        let env = env_over(space.clone(), &budget);
        let ch = channels();
        GenericSpace::<Counted, QueueDiscipline>::default()
            .prepare_create(&env, SpaceCreation {
                authority: region(1),
                ..creation(config_of(Counted::NAME, QueueDiscipline::NAME))
            })
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        assert_registered_on(
            &budget,
            &space.take_event_log().await,
            ch.config(),
            &region(1),
        );
        let seal = machinery_seal(&budget, &region(1));
        assert_eq!(
            space.get_data(ch.space_counter()).await[0].a.cost_authority,
            Some(seal.clone())
        );
        let allocator = namespace::gprivate(namespace::allocator_channel(SPACE));
        assert_eq!(
            space.get_data(&allocator).await[0].a.cost_authority,
            Some(seal)
        );
    }

    fn script() -> Vec<SpaceOp> {
        let fifo = string("fifo");
        let reoffer = string("reoffer");
        let fan = string("fan");
        let drain = string("drain");
        let divergent = string("divergent");
        let peeking = ConsumeOp {
            peek: true,
            ..receive_on(&fifo, "p", any())
        };
        let peek_waiter = ConsumeOp {
            peek: true,
            ..receive_on(&reoffer, "q", any())
        };
        let draining = ConsumeOp {
            persistent: true,
            ..receive_on(&drain, "r", any())
        };
        let forever = ConsumeOp {
            persistent: true,
            ..receive_on(&divergent, "s", any())
        };
        vec![
            SpaceOp::Produce(send_on(&fifo, 1, false)),
            SpaceOp::Produce(send_on(&fifo, 2, false)),
            SpaceOp::Consume(peeking),
            SpaceOp::Consume(receive_on(&fifo, "a", any())),
            SpaceOp::Consume(receive_on(&fifo, "b", exactly(9))),
            SpaceOp::Consume(peek_waiter),
            SpaceOp::Consume(receive_on(&reoffer, "t", any())),
            SpaceOp::Produce(send_on(&reoffer, 3, false)),
            SpaceOp::Consume(receive_on(&fan, "w1", any())),
            SpaceOp::Consume(receive_on(&fan, "w2", any())),
            SpaceOp::Produce(send_on(&fan, 4, true)),
            SpaceOp::Produce(send_on(&drain, 5, false)),
            SpaceOp::Produce(send_on(&drain, 6, false)),
            SpaceOp::Consume(draining),
            SpaceOp::Produce(send_on(&drain, 7, false)),
            SpaceOp::Consume(forever),
            SpaceOp::Produce(send_on(&divergent, 8, true)),
        ]
    }

    // Retired by decision 90 (C6, step D): the replay tests of the queue ran through the rigged
    // replay modes, which retire (decision 125). Native replay of re-entries and of the queue:
    // `space_native_persistent_retry_rows`, `space_native_replay_equal_hashmap`. The text before:
    // ```
    // async fn run_script(env: &SpaceEnv) -> Result<Vec<SpaceOutcome>, SpaceError> {
    //     let mut outcomes = Vec::new();
    //     for op in script() {
    //         outcomes.push(run(env, op).await?);
    //     }
    //     Ok(outcomes)
    // }
    //
    // #[tokio::test]
    // async fn space_queue_replay_stages_the_play_transactions() {
    //     let (play, replay) = play_and_replay().await;
    //     let play_env = env_over(play.clone(), &budget());
    //     create(&play_env, creation(config())).await;
    //     let start = play
    //         .create_checkpoint()
    //         .await
    //         .expect("a checkpoint after the creation")
    //         .root;
    //     let played = run_script(&play_env).await.expect("the script plays");
    //     assert_eq!(
    //         played
    //             .iter()
    //             .map(|outcome| outcome.fires.len())
    //             .sum::<usize>(),
    //         10
    //     );
    //     assert!(played[16].reissue.is_some());
    //     let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
    //
    //     replay
    //         .rig_and_reset(start.clone(), log)
    //         .await
    //         .expect("the replay rigs the play log");
    //     let replay_env = env_over(replay.clone(), &budget());
    //     let replayed = run_script(&replay_env).await.expect("the script replays");
    //     assert_eq!(replayed, played);
    //     replay
    //         .check_replay_data()
    //         .await
    //         .expect("every recorded COMM is paired");
    //     assert_eq!(
    //         replay.create_checkpoint().await.expect("a checkpoint").root,
    //         root
    //     );
    //
    //     replay
    //         .rig_and_reset(start, Vec::new())
    //         .await
    //         .expect("the replay rigs an empty log");
    //     match run(&replay_env, script().remove(0)).await {
    //         Err(error @ SpaceError::ReplayDivergence(_)) => {
    //             assert_eq!(error.code(), "ReplayDivergence")
    //         }
    //         other => panic!("negative control: a log without the COMMs diverges: {other:?}"),
    //     }
    // }
    // ```

    /// Added by decision 93 (decision 111): the operation `op` of [`script`] on the space
    /// `space`, with the authorities of the B7 test, so the budget registers each introduction.
    fn on_space(space: NativeSpace, op: SpaceOp) -> SpaceOp {
        match op {
            SpaceOp::Produce(produce) => {
                let mut produce = ProduceOp {
                    space,
                    authority: region(2),
                    ..produce
                };
                produce.data.cost_authority = Some(region(2));
                SpaceOp::Produce(produce)
            }
            SpaceOp::Consume(consume) => SpaceOp::Consume(ConsumeOp {
                space,
                authority: region(3),
                introduction_authority: region(4),
                ..consume
            }),
            other @ (SpaceOp::Settle(_) | SpaceOp::Join(_)) => {
                panic!("the script holds only sends and receives: {other:?}")
            }
        }
    }

    /// Added by decision 93 (decision 111): the name of `hash` in a shape, which is the number
    /// of distinct hashes before its first use.
    fn shape_name(names: &mut BTreeMap<Blake2b256Hash, usize>, hash: &Blake2b256Hash) -> usize {
        let next = names.len();
        *names.entry(hash.clone()).or_insert(next)
    }

    /// Added by decision 93 (decision 111): the shape of `produce`, with its hashes named by
    /// `names`.
    fn produce_shape(names: &mut BTreeMap<Blake2b256Hash, usize>, produce: &Produce) -> String {
        let channel = shape_name(names, &produce.channel_hash);
        let hash = shape_name(names, &produce.hash);
        format!(
            "{channel}:{hash} persistent {} deterministic {} failed {} outputs {:?}",
            produce.persistent, produce.is_deterministic, produce.failed, produce.output_value
        )
    }

    /// Added by decision 93 (decision 111): the shape of `consume`, with its hashes named by
    /// `names`.
    fn consume_shape(names: &mut BTreeMap<Blake2b256Hash, usize>, consume: &Consume) -> String {
        let channels: Vec<usize> = consume
            .channel_hashes
            .iter()
            .map(|hash| shape_name(names, hash))
            .collect();
        let hash = shape_name(names, &consume.hash);
        format!("{channels:?}:{hash} persistent {}", consume.persistent)
    }

    /// Added by decision 124 (rule CR-15): the registration of `event` in a shape. A machinery
    /// row registers the payer introduction, which carries no authority, so the row registers
    /// the fallback authority of its identity. A budget without registrations (`unregistered`)
    /// gives the same authority. That authority hashes the identity of the event, so the shape
    /// names it, as the shape names the hashes (QF-6). Every other authority stays as it is.
    fn registration_shape(
        budget: &RuntimeBudget,
        unregistered: &RuntimeBudget,
        event: &Event,
    ) -> String {
        let authority = registered(budget, event);
        match authority == registered(unregistered, event) {
            true => String::from("the fallback authority of the identity"),
            false => format!("{authority:?}"),
        }
    }

    /// Added by decision 93 (decision 111): the events of `log`, with each hash named by the
    /// order of its first use and with the introduction authority that `budget` registers for
    /// each produce and each consume. Two logs have one shape when they differ only in their ids
    /// (QF-6), the ids of the fallback authorities included (`registration_shape`).
    fn shape(log: &[Event], budget: &RuntimeBudget) -> Vec<String> {
        let unregistered = self::budget();
        let mut names = BTreeMap::new();
        log.iter()
            .map(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) => format!(
                    "produce {} registers {}",
                    produce_shape(&mut names, produce),
                    registration_shape(budget, &unregistered, event)
                ),
                Event::IoEvent(IOEvent::Consume(consume)) => format!(
                    "consume {} registers {}",
                    consume_shape(&mut names, consume),
                    registration_shape(budget, &unregistered, event)
                ),
                Event::Comm(comm) => {
                    let consume = consume_shape(&mut names, &comm.consume);
                    let produces: Vec<String> = comm
                        .produces
                        .iter()
                        .map(|produce| produce_shape(&mut names, produce))
                        .collect();
                    let repeated: Vec<(usize, i32)> = comm
                        .times_repeated
                        .iter()
                        .map(|(produce, times)| (shape_name(&mut names, &produce.hash), *times))
                        .collect();
                    format!(
                        "comm {consume} produces {produces:?} peeks {:?} repeated {repeated:?}",
                        comm.peeks
                    )
                }
            })
            .collect()
    }

    /// Added by decision 93 (decision 111): the log of the creation of a queue with the
    /// qualifier `qualifier` and of the operations of [`script`] on it, each awaited before the
    /// next, and the shape of that log.
    async fn qualifier_run(
        qualifier: SpaceQualifier,
        durability: Durability,
    ) -> (Vec<Event>, Vec<String>) {
        let space = NativeSpace {
            sid: SPACE.sid,
            durability,
        };
        let store = play().await;
        let budget = budget();
        let _scope = budget.enter_comm_accounting_scope();
        let env = env_over(store.clone(), &budget);
        let config = SpaceConfig::new(
            space.sid,
            HashMapOuter::NAME,
            QueueDiscipline::NAME,
            qualifier,
        )
        .to_par();
        create(&env, SpaceCreation {
            space,
            authority: region(1),
            ..creation(config)
        })
        .await;
        for op in script() {
            run(&env, on_space(space, op))
                .await
                .expect("the operation commits");
        }
        let log = store.take_event_log().await;
        let shaped = shape(&log, &budget);
        (log, shaped)
    }

    /// Added by decision 93 (decision 111): QF-7 without racing. The creation of a queue and the
    /// operations of [`script`] run one at a time on a `default` queue and on a `temp` queue with
    /// the same space id. The two logs differ in their ids (QF-6), and they have one shape: the
    /// same events and the same registered introduction authorities. So a temp operation emits the
    /// events and the observer calls of a default operation (QF-7).
    #[tokio::test]
    async fn space_temp_operations_emit_the_events_of_default_operations() {
        let (default_log, default) =
            qualifier_run(SpaceQualifier::Default, Durability::Durable).await;
        let (temp_log, temp) = qualifier_run(SpaceQualifier::Temp, Durability::Ephemeral).await;
        assert!(
            default.iter().any(|event| event.starts_with("comm")),
            "the script fires: {default:?}"
        );
        assert_ne!(
            temp_log, default_log,
            "a temp space and a default space have other ids"
        );
        assert_eq!(
            temp, default,
            "a temp operation emits the events of a default operation"
        );
    }

    #[test]
    fn space_queue_collection_lock_keys_cover_every_touched_channel() {
        let ch = channels();
        let key = key_of(&channel());
        assert_eq!(*ch.lock_keys(), [
            ch.config().clone(),
            ch.space_counter().clone(),
            ch.counter().clone(),
            ch.data().clone()
        ]);
        assert_eq!(
            namespace::reified_id_of(ch.config()),
            Some(ReifiedId::Config(SPACE))
        );
        assert_eq!(
            namespace::reified_id_of(ch.space_counter()),
            Some(ReifiedId::SpaceCounter(SPACE))
        );
        assert_eq!(
            namespace::reified_id_of(ch.counter()),
            Some(ReifiedId::CollectionCounter(SPACE, key.as_bytes()))
        );
        assert_eq!(
            namespace::reified_id_of(ch.data()),
            Some(ReifiedId::Data(SPACE, key.as_bytes()))
        );
    }

    #[test]
    fn space_queue_outcome_matches_receipts_to_staged_operations() {
        let stored = SpaceOpReceipt::Stored {
            produce: rspace_plus_plus::rspace::trace::event::Produce::create(
                &int(1),
                &int(2),
                false,
            ),
        };
        let consume_source = Consume::create(&vec![int(1)], &vec![any()], &named("k", None), false);
        let parked = SpaceOpReceipt::Parked {
            consume: consume_source.clone(),
        };
        let fired_with = |continuation: TaggedContinuation| SpaceOpReceipt::Fired {
            comm: COMM {
                consume: consume_source.clone(),
                produces: Vec::new(),
                peeks: BTreeSet::new(),
                times_repeated: BTreeMap::new(),
            },
            continuation: ContResult {
                continuation,
                persistent: false,
                channels: vec![int(1)],
                patterns: vec![any()],
                peek: false,
            },
            results: Vec::new(),
            produce: None,
        };
        let wrapped = record::wrap_continuation(3, &named("k", None)).expect("a record");
        // Changed by decision 123 (plan v4 C3d, V3-9, errata V4-17): the receipt has no deviation
        // and no refused pair. The text before:
        // let receipt = |ops| ParSpaceReceipt { ops };
        let receipt = |ops| ParSpaceReceipt {
            ops,
            deviation: None,
            refused: Vec::new(),
        };
        assert_eq!(
            outcome(
                receipt(vec![stored.clone()]),
                vec![Staged::Store],
                Vec::new(),
                None
            ),
            Ok(SpaceOutcome::default())
        );
        assert_eq!(
            outcome(
                receipt(vec![
                    fired_with(named("counter", None)),
                    stored.clone(),
                    fired_with(wrapped)
                ]),
                vec![
                    Staged::Counter(CounterOp::Take),
                    Staged::Counter(CounterOp::Put),
                    Staged::Fire
                ],
                Vec::new(),
                None
            ),
            Ok(SpaceOutcome {
                fires: vec![SpaceFire {
                    continuation: named("k", None),
                    results: Vec::new(),
                }],
                reissue: None,
                mirror: None,
            })
        );
        let differs = Err(SpaceError::Inconsistent(
            "a receipt differs from its staged operation",
        ));
        assert_eq!(
            outcome(
                receipt(vec![stored.clone()]),
                vec![Staged::Park],
                Vec::new(),
                None
            ),
            differs
        );
        assert_eq!(
            outcome(
                receipt(vec![parked]),
                vec![Staged::Counter(CounterOp::Put)],
                Vec::new(),
                None
            ),
            differs
        );
        assert_eq!(
            outcome(receipt(vec![stored]), vec![Staged::Fire], Vec::new(), None),
            differs
        );
        assert_eq!(
            outcome(receipt(Vec::new()), vec![Staged::Store], Vec::new(), None),
            Err(SpaceError::Inconsistent(
                "the receipt does not list the staged operations"
            ))
        );
        assert_eq!(
            outcome(
                receipt(vec![fired_with(named("bare", None))]),
                vec![Staged::Fire],
                Vec::new(),
                None
            ),
            Err(SpaceError::Inconsistent("a malformed continuation record"))
        );
    }

    /// Added by decision 124 (plan v2 C7.5, F5): a settle that lifts a stored record records
    /// the pair of the record and the waiter of its fire in `Plan.lifts`, at the row of the
    /// lift (CR-20). The head waiter of the queue does not match the send, so the send is
    /// stored. The settle of the stack view then fires the top waiter with the top record.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn settle_lift_records_the_pair_of_its_record_and_waiter() {
        use crate::rust::interpreter::spaces::disciplines::stack::{StackDiscipline, StackView};
        use crate::rust::interpreter::spaces::settle::{self, InFlight, SettleOrigin};
        let Fixture { space, env } = fixture().await;
        consume(&env, receive("w1", exactly(1))).await;
        consume(&env, receive("w2", any())).await;
        produce(&env, send(2)).await;
        let ch = channels();
        let record_hash = space
            .get_data(ch.data())
            .await
            .into_iter()
            .next()
            .expect("the send is stored")
            .source
            .hash;
        let waiter_hash = space
            .get_waiting_continuations(vec![ch.data().clone()])
            .await
            .into_iter()
            .find(|waiter| {
                let parked = record::unwrap_continuation(&waiter.continuation)
                    .expect("a continuation record");
                body_name(&parked.continuation) == "w2"
            })
            .expect("the top waiter is parked")
            .source
            .hash;
        let random = Blake2b512Random::create_from_bytes(b"settle lift");
        let mut seen = None;
        staged_plan(&env, |cx| {
            let introduction = Arc::clone(cx.introduction());
            let authority = OperationAuthority {
                continuation: None,
                introduction: &introduction,
            };
            // A lift takes the collection counter of the round, so the plan bumps it first. The
            // round keeps the value before the bump as its counter, as `stage_op` keeps it.
            let counter = cx.bump_on(ch.counters(), &random, authority)?;
            cx.plan.bumped = Some(counter);
            let sending = send(2);
            let origin = SettleOrigin::of_operation(OpView::Produce(&sending));
            settle::settle(
                &StackView::new(&StackDiscipline),
                cx,
                &origin,
                InFlight::none(),
            )?;
            let at = cx
                .plan
                .staged
                .iter()
                .position(|kind| matches!(kind, Staged::Lift));
            seen = Some((at, cx.plan.lifts.clone()));
            Ok(())
        })
        .await
        .expect("the settle commits");
        let (at, lifts) = seen.expect("the settle ran");
        let at = at.expect("the settle stages a lift");
        assert_eq!(lifts, vec![(at, (record_hash, waiter_hash))]);
    }

    /// Added by decision 124 (plan v2 C7.8, rule CR-16, ruling OP-LIFTS): `refused_lift` gives
    /// the settle pair of the lift row only when that row parked at the deviation. A lift row
    /// that fired, a deviation before the lift row and a receipt without a deviation give no
    /// settle pair.
    #[test]
    fn refused_lift_books_its_settle_pair() {
        use rspace_plus_plus::rspace::space_store_handle::RowDeviation;
        let stored = SpaceOpReceipt::Stored {
            produce: rspace_plus_plus::rspace::trace::event::Produce::create(
                &int(1),
                &int(2),
                false,
            ),
        };
        let consume_source = Consume::create(&vec![int(1)], &vec![any()], &named("k", None), false);
        let parked = SpaceOpReceipt::Parked {
            consume: consume_source.clone(),
        };
        let fired = SpaceOpReceipt::Fired {
            comm: COMM {
                consume: consume_source,
                produces: Vec::new(),
                peeks: BTreeSet::new(),
                times_repeated: BTreeMap::new(),
            },
            continuation: ContResult {
                continuation: named("lift", None),
                persistent: false,
                channels: vec![int(1)],
                patterns: vec![any()],
                peek: false,
            },
            results: Vec::new(),
            produce: None,
        };
        let at = |index| {
            Some(RowDeviation {
                index,
                item_failed: Vec::new(),
                dispatched: None,
                vetoing: false,
                tail_read: TailRead::Ends,
                claims: None,
                reissue: None,
            })
        };
        let receipt = |row, deviation| ParSpaceReceipt {
            ops: vec![stored.clone(), row],
            deviation,
            refused: Vec::new(),
        };
        let pair = (
            Blake2b256Hash::new(b"lifted record"),
            Blake2b256Hash::new(b"settle waiter"),
        );
        let lifts = || vec![(1, pair.clone())];
        assert_eq!(
            refused_lift(lifts(), &receipt(parked.clone(), at(1))),
            Some(pair.clone()),
            "the lift row parked at the deviation"
        );
        assert_eq!(
            refused_lift(lifts(), &receipt(fired, at(1))),
            None,
            "a record end names the lift row, which fired"
        );
        assert_eq!(
            refused_lift(lifts(), &receipt(parked.clone(), at(0))),
            None,
            "the commit stopped before the lift row"
        );
        assert_eq!(
            refused_lift(lifts(), &receipt(parked, None)),
            None,
            "a receipt without a deviation"
        );
    }

    /// Added by decision 124 (L14, plan v4 C7.8, rule CR-20, errata V4-17, ruling RAMD 1.7): a
    /// receipt without a deviation books the refused pairs of its rows and no other pair, and
    /// the finish takes them out of the receipt. A recorded lift and a Stop add no pair without
    /// a deviation.
    #[test]
    fn finish_writes_refused_pairs_without_a_deviation() {
        let first = (Blake2b256Hash::new(b"r1"), Blake2b256Hash::new(b"w1"));
        let second = (Blake2b256Hash::new(b"r2"), Blake2b256Hash::new(b"w2"));
        let lifted = (Blake2b256Hash::new(b"l1"), Blake2b256Hash::new(b"s1"));
        let refused = vec![first.clone(), second];
        let mut receipt = ParSpaceReceipt {
            ops: Vec::new(),
            deviation: None,
            refused: refused.clone(),
        };
        let lifts = vec![(0, lifted.clone())];
        let pairs: Vec<BookPair> = finish_pairs(&mut receipt, lifts, false).collect();
        assert_eq!(pairs, refused, "the refused pairs in order");
        assert!(receipt.refused.is_empty(), "the pairs leave the receipt");
        let mut stopped = ParSpaceReceipt {
            ops: Vec::new(),
            deviation: None,
            refused: vec![first.clone()],
        };
        let lifts = vec![(0, lifted)];
        let pairs: Vec<BookPair> = finish_pairs(&mut stopped, lifts, true).collect();
        assert_eq!(pairs, vec![first], "a Stop adds no item pair");
    }

    /// A head-only test discipline with one slot on each side (F15), and a named send param and
    /// a query param whose values must be non-negative integers. The engine tests of S1 use it.
    /// The CELL and PQ batches own the real disciplines.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Slot;

    impl Discipline for Slot {
        const NAME: &'static str = "slot";
        const USES_COUNTER: bool = true;
        const SEND_PARAMS: &'static [&'static str] = &["priority"];
        const QUERY_PARAMS: &'static [&'static str] = &["priority"];
        const CAPACITY: Option<usize> = Some(1);

        fn head<T: Serialize>(
            &self,
            side: Side,
            entries: &[Ordered<'_, T>],
        ) -> Result<Option<usize>, SpaceError> {
            QueueDiscipline.head(side, entries)
        }

        fn reaches_head(&self, side: Side, queued: usize) -> bool {
            QueueDiscipline.reaches_head(side, queued)
        }

        fn precedes(&self, side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
            QueueDiscipline.precedes(side, ord, head)
        }

        fn check_values(
            &self,
            _config: &SpaceConfig,
            params: &BTreeMap<String, Par>,
            query: Option<&QueryParams>,
        ) -> Result<(), SpaceError> {
            let named = params
                .iter()
                .chain(query.into_iter().flat_map(|query| query.named.iter()));
            for (key, value) in named {
                match value.exprs.as_slice() {
                    [Expr {
                        expr_instance: Some(ExprInstance::GInt(level)),
                    }] if *level >= 0 => {}
                    _ => return Err(SpaceError::InvalidParam { key: key.clone() }),
                }
            }
            Ok(())
        }
    }

    type SlotSpace = GenericSpace<HashMapOuter, Slot>;

    /// What the plans of the probe discipline saw: the counter value of each round, and the
    /// kind of the operation of each settle hook.
    static PROBED: LazyLock<std::sync::Mutex<Vec<(Option<i64>, &'static str)>>> =
        LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

    /// A queue that records what its plans see, for the engine test of `Cx::bumped` and of the
    /// views of the settle hooks.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Probe;

    impl Probe {
        fn record(cx: &Cx<'_, '_>, kind: &'static str) {
            PROBED
                .lock()
                .expect("the probe log is not poisoned")
                .push((cx.bumped(), kind));
        }
    }

    impl Discipline for Probe {
        const NAME: &'static str = "probe";
        const USES_COUNTER: bool = true;
        const SEND_PARAMS: &'static [&'static str] = &[];

        fn head<T: Serialize>(
            &self,
            side: Side,
            entries: &[Ordered<'_, T>],
        ) -> Result<Option<usize>, SpaceError> {
            QueueDiscipline.head(side, entries)
        }

        fn reaches_head(&self, side: Side, queued: usize) -> bool {
            QueueDiscipline.reaches_head(side, queued)
        }

        fn precedes(&self, side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
            QueueDiscipline.precedes(side, ord, head)
        }

        fn settle_before(
            &self,
            cx: &mut Cx<'_, '_>,
            op: OpView<'_>,
        ) -> Result<Option<SpaceReissue>, SpaceError> {
            Probe::record(cx, match op {
                OpView::Produce(_) => "before produce",
                OpView::Consume(_) => "before consume",
            });
            Ok(None)
        }

        fn settle_after(
            &self,
            cx: &mut Cx<'_, '_>,
            op: OpView<'_>,
        ) -> Result<Option<SpaceReissue>, SpaceError> {
            Probe::record(cx, match op {
                OpView::Produce(_) => "after produce",
                OpView::Consume(_) => "after consume",
            });
            Ok(None)
        }
    }

    /// A HashMap store with an allocator: the creation stages `A(sid) = 0`, and an allocation of
    /// `k` names takes the value `n` and puts `n + k`. The engine tests of P5d use it. The ARRAY
    /// batch owns the Array and Vector stores.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Counted;

    impl OuterStore for Counted {
        const NAME: &'static str = "counted";
        const ALLOCATOR: bool = true;

        fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError> {
            HashMapOuter.key(channel)
        }

        fn data_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
            HashMapOuter.data_channel(space, key)
        }

        fn counter_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
            HashMapOuter.counter_channel(space, key)
        }

        fn creation_records(
            &self,
            space: NativeSpace,
            random: &Blake2b512Random,
            authority: &CostAuthority,
        ) -> Result<Vec<(Par, ListParWithRandom)>, SpaceError> {
            let record = record::counter_record(
                0,
                &RandomPurpose::Allocator.derive(random).split_byte(1),
                record::cost_authority(authority),
            );
            Ok(vec![(
                namespace::gprivate(namespace::allocator_channel(space)),
                record,
            )])
        }

        fn allocate(
            &self,
            cx: &mut AllocCx<'_>,
            _config: &SpaceConfig,
            op: &AllocOp,
        ) -> Result<Vec<u64>, SpaceError> {
            let stored = cx.allocator_record()?.ok_or(SpaceError::Inconsistent(
                "a space without an allocator record",
            ))?;
            let value = record::counter_value(&stored.a)?;
            let count = i64::try_from(op.count).map_err(|_| SpaceError::OutOfNames)?;
            let next = value.checked_add(count).ok_or(SpaceError::OutOfNames)?;
            cx.take(stored, value)?;
            cx.put(next)?;
            Ok((value..next)
                .map(|index| u64::try_from(index).expect("a non-negative index"))
                .collect())
        }

        fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError> {
            HashMapOuter.gensym(space, random)
        }
    }

    fn config_of(outer: &str, inner: &str) -> Par {
        SpaceConfig::new(SPACE.sid, outer, inner, SpaceQualifier::Default).to_par()
    }

    async fn slot_fixture() -> Fixture {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        SlotSpace::default()
            .prepare_create(&env, creation(config_of(HashMapOuter::NAME, Slot::NAME)))
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        space.take_event_log().await;
        Fixture { space, env }
    }

    async fn run_slot(env: &SpaceEnv, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run_op(&SlotSpace::default(), env, op).await
    }

    fn params(entries: &[(&str, Par)]) -> BTreeMap<String, Par> {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    fn query(entries: &[(&str, Par)]) -> Option<QueryParams> {
        Some(QueryParams {
            payload: Vec::new(),
            named: params(entries),
        })
    }

    fn alloc_op(count: u64) -> AllocOp {
        AllocOp {
            space: SPACE,
            count,
            random_state: Blake2b512Random::create_from_bytes(format!("alloc {count}").as_bytes())
                .to_bytes(),
            authority: CostAuthority::default(),
        }
    }

    fn wrapped(name: &str) -> TaggedContinuation {
        record::wrap_continuation(0, &named(name, None)).expect("a continuation record")
    }

    /// Stages `stage` in a plan of its own over the collection on `channel()`, with the
    /// introduction of the empty authority, and commits it. It returns the outcome.
    async fn staged_plan(
        env: &SpaceEnv,
        stage: impl FnOnce(&mut Cx<'_, '_>) -> Result<(), SpaceError>,
    ) -> Result<SpaceOutcome, SpaceError> {
        staged_plan_with(env, None, stage).await
    }

    /// `staged_plan` in a space whose theory is `theory` (P7, TH-6).
    async fn staged_plan_with(
        env: &SpaceEnv,
        theory: Option<Arc<dyn SpaceTheory>>,
        stage: impl FnOnce(&mut Cx<'_, '_>) -> Result<(), SpaceError>,
    ) -> Result<SpaceOutcome, SpaceError> {
        let ch = channels();
        let mut txn = env.store.begin(ch.lock_keys()).await?;
        let matcher = SpaceMatcher::new(env);
        let introduction = Introduction::new(&env.budget, &CostAuthority::default());
        let config = SpaceConfig::new(SPACE.sid, "hashmap", "queue", SpaceQualifier::Default);
        // Added by decision 92: the job of the plan. No staged operation of a test plan has its
        // random state, so a plan that does not start with a bump stages an anchor first.
        let job = plan_job();
        let mut plan = Plan {
            data: ch.data(),
            config: &config,
            matcher: &matcher,
            introduction: &introduction,
            staged: Vec::with_capacity(MAX_STAGED),
            prefixes: Vec::new(),
            bumped: None,
            // Disabled by decision 90 (decision 69): Option P is gone.
            // legacy: LegacyChain::new(&env.budget),
            rounds: FireRounds::new(),
            mirror: MirrorCapture::inactive(),
            seq: None,
            theory,
            leases: Vec::new(),
            job: &job,
            // Added by decision 102 (plan v4 C4.2).
            picked: false,
            order: PickMode::Random,
            lifts: Vec::new(),
            // Added by decision 124 (L14, plan v4 C7.8): the plan reads one snapshot of the book.
            book: env.volatile.book(),
            // Added by decision 124 (plan v4 C7.11, ruling RADD 1.3): room for the class of each
            // staged row, as `staged` has, and no copy of the operation before the staging.
            tail_rows: Vec::<TailRow>::with_capacity(MAX_STAGED),
            operation: None,
        };
        stage(&mut Cx {
            txn: &mut txn,
            plan: &mut plan,
        })?;
        let Plan {
            staged,
            prefixes,
            // Disabled by decision 90 (decision 69): Option P is gone.
            // legacy,
            ..
        } = plan;
        Prepared::new(txn, move |receipt| {
            // Disabled by decision 90 (decision 69): Option P is gone.
            // legacy.finish()?;
            outcome(receipt, staged, prefixes, None)
        })
        .commit()
        .await
    }

    /// The candidates of a consume fire over the records of the collection in `cx`, bound by
    /// `any()`, in the order of their `ord`.
    fn candidates_by_ord(cx: &mut Cx<'_, '_>) -> Result<Vec<ParCandidate>, SpaceError> {
        let data = cx.data_channel();
        let mut stored: Vec<(i64, usize, ParDatum)> = cx
            .data()?
            .iter()
            .enumerate()
            .map(|(index, datum)| {
                let ord = record::unwrap_datum(&datum.a).expect("a datum record").ord;
                (ord, index, datum.clone())
            })
            .collect();
        stored.sort_by_key(|(ord, ..)| *ord);
        let mut candidates = Vec::with_capacity(stored.len());
        for (_, index, head) in stored {
            let payload = record::unwrap_datum(&head.a).expect("a datum record").datum;
            let matched = cx
                .bind(&any(), &payload)
                .expect("any() binds every payload");
            let view_index = i32::try_from(index).expect("a small index");
            candidates.push(receive_candidate(data.clone(), ReceiveFire {
                head,
                matched,
                view_index,
            }));
        }
        Ok(candidates)
    }

    #[tokio::test]
    async fn space_engine_checks_params_and_queries_before_the_bump() {
        let ch = channels();
        // The queue takes no params and no query.
        let Fixture { space, env } = fixture().await;
        let unknown = ProduceOp {
            params: params(&[("prority", int(1))]),
            ..send(1)
        };
        assert_eq!(
            run(&env, SpaceOp::Produce(unknown)).await,
            Err(SpaceError::UnknownParam {
                key: "prority".to_string(),
                suggestion: None,
            })
        );
        let queried = ConsumeOp {
            query: query(&[]),
            ..receive("w", any())
        };
        assert_eq!(
            run(&env, SpaceOp::Consume(queried)).await,
            Err(SpaceError::QuerySpecUnsupported)
        );
        assert!(space.take_event_log().await.is_empty());
        assert!(counter(&space, ch.counter()).await.is_empty());
        assert_eq!(counter(&space, ch.space_counter()).await, vec![0]);
        produce(&env, send(1)).await;
        assert_eq!(counter(&space, ch.counter()).await, vec![1]);
        // A discipline with params checks the keys and then the values, before the bump.
        let Fixture { space, env } = slot_fixture().await;
        let cases = [
            (
                SpaceOp::Produce(ProduceOp {
                    params: params(&[("prority", int(1))]),
                    ..send(1)
                }),
                SpaceError::UnknownParam {
                    key: "prority".to_string(),
                    suggestion: Some("priority".to_string()),
                },
            ),
            (
                SpaceOp::Produce(ProduceOp {
                    params: params(&[("priority", string("high"))]),
                    ..send(1)
                }),
                SpaceError::InvalidParam {
                    key: "priority".to_string(),
                },
            ),
            (
                SpaceOp::Consume(ConsumeOp {
                    query: query(&[("level", int(1))]),
                    ..receive("w", any())
                }),
                SpaceError::UnknownParam {
                    key: "level".to_string(),
                    suggestion: None,
                },
            ),
            (
                SpaceOp::Consume(ConsumeOp {
                    query: query(&[("priority", int(-1))]),
                    ..receive("w", any())
                }),
                SpaceError::InvalidParam {
                    key: "priority".to_string(),
                },
            ),
        ];
        for (op, expected) in cases {
            assert_eq!(run_slot(&env, op).await, Err(expected));
        }
        assert!(space.take_event_log().await.is_empty());
        assert!(counter(&space, ch.counter()).await.is_empty());
        let accepted = ProduceOp {
            params: params(&[("priority", int(2))]),
            ..send(1)
        };
        run_slot(&env, SpaceOp::Produce(accepted))
            .await
            .expect("an accepted param commits");
        assert_eq!(counter(&space, ch.counter()).await, vec![1]);
    }

    #[tokio::test]
    async fn space_engine_plans_see_the_round_counter_and_the_operation() {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        let probe = GenericSpace::<HashMapOuter, Probe>::default();
        probe
            .prepare_create(&env, creation(config_of(HashMapOuter::NAME, Probe::NAME)))
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        PROBED
            .lock()
            .expect("the probe log is not poisoned")
            .clear();
        for op in [
            SpaceOp::Produce(send(1)),
            SpaceOp::Consume(receive("w", any())),
            SpaceOp::Consume(receive("x", any())),
        ] {
            run_op(&probe, &env, op)
                .await
                .expect("the operation commits");
        }
        // Each round bumps the counter once, and each settle hook sees its operation.
        assert_eq!(
            PROBED
                .lock()
                .expect("the probe log is not poisoned")
                .clone(),
            vec![
                (Some(0), "before produce"),
                (Some(0), "after produce"),
                (Some(1), "before consume"),
                (Some(1), "after consume"),
                (Some(2), "before consume"),
                (Some(2), "after consume"),
            ]
        );
        // A plan of its own over a staged view has no bump.
        let Fixture { env, .. } = fixture().await;
        staged_plan(&env, |cx| {
            assert_eq!(cx.bumped(), None);
            Ok(())
        })
        .await
        .expect("an empty plan commits nothing");
    }

    // Disabled by decision 92: a failure at a position of the state commits the anchor of its
    // job, so the transaction is no longer empty.
    // `space_engine_positional_failure_commits_the_anchor_only` checks the new rule.
    // #[tokio::test]
    // async fn space_engine_capacity_abort_stages_nothing() {
    //     let Fixture { space, env } = slot_fixture().await;
    //     let ch = channels();
    //     run_slot(&env, SpaceOp::Produce(send(1)))
    //         .await
    //         .expect("the first record stores");
    //     space.take_event_log().await;
    //     assert_eq!(
    //         run_slot(&env, SpaceOp::Produce(send(2))).await,
    //         Err(SpaceError::CellOccupied)
    //     );
    //     assert!(space.take_event_log().await.is_empty());
    //     assert_eq!(records(&space).await, vec![(0, vec![int(1)], false)]);
    //     assert_eq!(counter(&space, ch.counter()).await, vec![1]);
    //     // The waiter side of another collection holds one waiter too.
    //     let other = string("other");
    //     let waiting = |name: &str| receive_on(&other, name, exactly(9));
    //     run_slot(&env, SpaceOp::Consume(waiting("w1")))
    //         .await
    //         .expect("the first waiter parks");
    //     space.take_event_log().await;
    //     assert_eq!(
    //         run_slot(&env, SpaceOp::Consume(waiting("w2"))).await,
    //         Err(SpaceError::CellOccupied)
    //     );
    //     assert!(space.take_event_log().await.is_empty());
    //     assert_eq!(waiters_on(&space, &other).await, vec![waiter(
    //         0, "w1", false, false
    //     )]);
    //     assert_eq!(counter(&space, channels_of(&other).counter()).await, vec![
    //         1
    //     ]);
    // }

    /// Added by decision 92: a store or a park on a full side fails with `CellOccupied`, a
    /// failure at a position of the state. The transaction discards its staged operations, the
    /// bump included, commits the anchor of its job only, and then fails. So the records, the
    /// waiters, the counters and the config record stay unchanged.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_positional_failure_commits_the_anchor_only() {
        let Fixture { space, env } = slot_fixture().await;
        let ch = channels();
        let config = space.get_data(ch.config()).await;
        run_slot(&env, SpaceOp::Produce(send(1)))
            .await
            .expect("the first record stores");
        space.take_event_log().await;
        assert_eq!(
            run_slot(&env, SpaceOp::Produce(send(2))).await,
            Err(SpaceError::CellOccupied)
        );
        assert_eq!(labels(&space.take_event_log().await), owned(&ANCHOR));
        assert_eq!(records(&space).await, vec![(0, vec![int(1)], false)]);
        assert_eq!(counter(&space, ch.counter()).await, vec![1]);
        // The waiter side of another collection holds one waiter too.
        let other = string("other");
        let waiting = |name: &str| receive_on(&other, name, exactly(9));
        run_slot(&env, SpaceOp::Consume(waiting("w1")))
            .await
            .expect("the first waiter parks");
        space.take_event_log().await;
        assert_eq!(
            run_slot(&env, SpaceOp::Consume(waiting("w2"))).await,
            Err(SpaceError::CellOccupied)
        );
        assert_eq!(labels(&space.take_event_log().await), owned(&ANCHOR));
        assert_eq!(waiters_on(&space, &other).await, vec![waiter(
            0, "w1", false, false
        )]);
        assert_eq!(counter(&space, channels_of(&other).counter()).await, vec![
            1
        ]);
        assert_eq!(space.get_data(ch.config()).await, config);
    }

    #[tokio::test]
    async fn space_engine_group_fire_lines_up_and_dispatches_its_prefix() {
        let Fixture { space, env } = fixture().await;
        for value in 1..=3 {
            produce(&env, send(value)).await;
        }
        space.take_event_log().await;
        // A group over `[D; 3]` that binds one result: the outcome dispatches the result of the
        // first candidate, and the fire removes all three records.
        let outcome = staged_plan(&env, |cx| {
            let data = cx.data_channel();
            let candidates = candidates_by_ord(cx)?;
            let width = candidates.len();
            cx.fire_receive_group(
                vec![data.clone(); width],
                vec![any(); width],
                wrapped("k"),
                false,
                BTreeSet::new(),
                candidates,
                1,
                // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): no row search.
                None,
            )
        })
        .await
        .expect("the group fire commits");
        assert_eq!(fired(&outcome), vec![delivery("k", 1)]);
        assert!(records(&space).await.is_empty());
        // Changed by decision 92: another job can also stage this fire, so the transaction
        // records its anchor first.
        // assert_eq!(labels(&space.take_event_log().await), vec![
        //     "consume D,D,D",
        //     "comm D,D,D"
        // ]);
        assert_eq!(
            labels(&space.take_event_log().await),
            [&ANCHOR[..], &["consume D,D,D", "comm D,D,D"]].concat()
        );
        // A group that does not line up, or binds no result or too many, stages nothing.
        produce(&env, send(4)).await;
        space.take_event_log().await;
        let misaligned = |binds: usize, extra_channel: bool| {
            let env = &env;
            async move {
                staged_plan(env, |cx| {
                    let data = cx.data_channel();
                    let candidates = candidates_by_ord(cx)?;
                    let width = candidates.len() + usize::from(extra_channel);
                    cx.fire_receive_group(
                        vec![data.clone(); width],
                        vec![any(); candidates.len()],
                        wrapped("k"),
                        false,
                        BTreeSet::new(),
                        candidates,
                        binds,
                        // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): no row search.
                        None,
                    )
                })
                .await
            }
        };
        let rejected = Err(SpaceError::Inconsistent(
            "a fire group does not line up its channels, patterns, candidates and binds",
        ));
        assert_eq!(misaligned(1, true).await, rejected);
        assert_eq!(misaligned(0, false).await, rejected);
        assert_eq!(misaligned(2, false).await, rejected);
        assert!(space.take_event_log().await.is_empty());
        assert_eq!(records(&space).await, vec![(3, vec![int(4)], false)]);
    }

    /// Added by decision 124 (plan v4 C7.9, ruling D-1): `run_plan` of a test plan of `job` on
    /// the collection `ch` of the seq space `space`, with the claims `seq`. The prepared
    /// transaction commits.
    async fn seq_plan(
        env: &SpaceEnv,
        space: NativeSpace,
        ch: &CollectionChannels,
        job: &SpaceJob,
        seq: Option<SeqClaims>,
        stage: impl FnMut(&mut Cx<'_, '_>) -> Result<Option<SpaceReissue>, SpaceError>,
    ) -> Result<SpaceOutcome, SpaceError> {
        let config = SpaceConfig::new(space.sid, "hashmap", "queue", SpaceQualifier::Seq);
        let admission = Admission {
            theory: None,
            leases: Vec::new(),
        };
        let txn = env.store.begin(ch.lock_keys()).await?;
        run_plan(
            env,
            txn,
            ch.data(),
            &config,
            &CostAuthority::default(),
            seq,
            admission,
            job,
            false,
            stage,
        )?
        .commit()
        .await
    }

    /// Added by decision 124 (plan v4 C7.9, ruling D-1): `fire_receive` and a one-channel
    /// `fire_receive_group` with one bind stage the same rows, the same claims and the same
    /// anchor, because both stage through one group step. The collection is on a `seq` space, so
    /// the fire claims its channel, and a later job meets the stored claim.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fire_receive_stages_as_the_group_step() {
        let space = NativeSpace {
            sid: SPACE.sid,
            durability: Durability::Ephemeral,
        };
        let ch = CollectionChannels::new(&HashMapOuter, space, &key_of(&channel()));
        let job = SpaceJob {
            space,
            ..plan_job()
        };
        let op = receive("k", any());
        let mut runs = Vec::with_capacity(2);
        for group in [false, true] {
            let store = play().await;
            let env = env_over(store.clone(), &budget());
            let config = SpaceConfig::new(
                space.sid,
                HashMapOuter::NAME,
                QueueDiscipline::NAME,
                SpaceQualifier::Seq,
            )
            .to_par();
            create(&env, SpaceCreation {
                space,
                ..creation(config)
            })
            .await;
            seq_plan(&env, space, &ch, &job, None, |cx| {
                cx.store(record_of(&job, 1), false)?;
                Ok(None)
            })
            .await
            .expect("the record is stored");
            store.take_event_log().await;
            let claims = SeqClaims::open(&env.volatile, space, SpaceQualifier::Seq, false, b"item")
                .expect("a seq job opens its claims")
                .expect("a program job on a seq space claims");
            let mut staged = Vec::<Staged>::new();
            let outcome = seq_plan(&env, space, &ch, &job, Some(claims), |cx| {
                let head = cx.data()?.first().cloned().expect("the stored record");
                let payload = record::unwrap_datum(&head.a).expect("a datum record").datum;
                let matched = cx.bind(&any(), &payload).expect("any() binds");
                let fire = ReceiveFire {
                    head,
                    matched,
                    view_index: 0,
                };
                match group {
                    false => cx.fire_receive(&op, fire, wrapped("k"), BTreeSet::new(), None)?,
                    true => {
                        let data = cx.data_channel();
                        let channels = cx.channel_list(data);
                        let patterns = cx.pattern_list(&op.pattern);
                        let mut candidates = Vec::<ParCandidate>::with_capacity(1);
                        candidates.push(receive_candidate(data.clone(), fire));
                        cx.fire_receive_group(
                            channels,
                            patterns,
                            wrapped("k"),
                            op.persistent,
                            BTreeSet::new(),
                            candidates,
                            1,
                            None,
                        )?
                    }
                }
                staged = cx.plan.staged.clone();
                Ok(None)
            })
            .await
            .expect("the fire commits");
            let mut later =
                SeqClaims::open(&env.volatile, space, SpaceQualifier::Seq, false, b"later")
                    .expect("a seq job opens its claims")
                    .expect("a program job on a seq space claims");
            let claimed = later.claim(ch.data(), ClaimRole::Data);
            let log = labels(&store.take_event_log().await);
            runs.push((staged, fired(&outcome), log, claimed));
        }
        assert_eq!(runs[0], runs[1], "the rows, the claims and the anchor");
    }

    #[tokio::test]
    async fn space_engine_retire_and_lift_fire_without_dispatch() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        produce(&env, send(2)).await;
        space.take_event_log().await;
        for (round, value) in [(0, 1), (1, 2)] {
            let outcome = staged_plan(&env, |cx| {
                let data = cx.data_channel();
                let candidates = candidates_by_ord(cx)?;
                let first = candidates.into_iter().take(1).collect();
                match round {
                    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the internal
                    // fires name the discipline of the space. The text before:
                    // 0 => cx.retire(vec![data.clone()], vec![any()], wrapped("retire"), first),
                    // _ => cx.lift(vec![data.clone()], vec![any()], wrapped("lift"), first),
                    0 => cx.retire::<QueueDiscipline>(
                        vec![data.clone()],
                        vec![any()],
                        wrapped("retire"),
                        first,
                    ),
                    _ => cx.lift::<QueueDiscipline>(
                        vec![data.clone()],
                        vec![any()],
                        wrapped("lift"),
                        first,
                    ),
                }
            })
            .await
            .expect("the internal fire commits");
            assert!(outcome.fires.is_empty(), "{value}");
            // Added by decision 124 (plan v2 C7.5, rule CR-21): the COMM of a `K_lift` fire peeks
            // its channel, so the lift round gives "comm D peek".
            let comm = match round {
                0 => "comm D",
                _ => "comm D peek",
            };
            // Changed by decision 92: another job can also stage this internal fire, so the
            // transaction records its anchor first.
            // assert_eq!(labels(&space.take_event_log().await), vec![
            //     "consume D",
            //     "comm D"
            // ]);
            assert_eq!(
                labels(&space.take_event_log().await),
                [&ANCHOR[..], &["consume D", comm]].concat()
            );
        }
        assert!(records(&space).await.is_empty());
        // An internal fire with no candidate, or one that does not line up, stages nothing.
        let rejected = Err(SpaceError::Inconsistent(
            "an internal fire does not line up its channels, patterns and candidates",
        ));
        assert_eq!(
            // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the retire names the
            // discipline of the space. The text before:
            // staged_plan(&env, |cx| cx.retire(
            staged_plan(&env, |cx| cx.retire::<QueueDiscipline>(
                Vec::new(),
                Vec::new(),
                wrapped("r"),
                Vec::new()
            ))
            .await,
            rejected
        );
        produce(&env, send(3)).await;
        space.take_event_log().await;
        assert_eq!(
            staged_plan(&env, |cx| {
                let candidates = candidates_by_ord(cx)?;
                // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the lift names the
                // discipline of the space. The text before:
                // cx.lift(Vec::new(), vec![any()], wrapped("l"), candidates)
                cx.lift::<QueueDiscipline>(Vec::new(), vec![any()], wrapped("l"), candidates)
            })
            .await,
            rejected
        );
        assert!(space.take_event_log().await.is_empty());
    }

    /// Added by decision 124 (plan v2 C7.5, rule CR-21): the COMM of a `K_lift` fire peeks its
    /// one channel, channel 0.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lift_comm_peeks_its_channel() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        space.take_event_log().await;
        staged_plan(&env, |cx| {
            let data = cx.data_channel();
            let candidates = candidates_by_ord(cx)?;
            cx.lift::<QueueDiscipline>(vec![data.clone()], vec![any()], wrapped("lift"), candidates)
        })
        .await
        .expect("the lift commits");
        let log = space.take_event_log().await;
        let [.., Event::Comm(comm)] = log.as_slice() else {
            panic!("the lift fires last: {log:?}");
        };
        assert_eq!(comm.peeks, BTreeSet::from([0]));
    }

    /// Added by decision 124 (plan v2 C7.10, decision 100): a `K_retire` and a `K_lift` that
    /// sit in the store before the operation are inert machinery waiters, so `head_waiter`
    /// skips both and returns the user waiter.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn readers_skip_an_inert_machinery_waiter() {
        use crate::rust::interpreter::spaces::disciplines::scan::{
            retire_continuation, retire_pattern,
        };
        use crate::rust::interpreter::spaces::settle::{lift_continuation, lift_pattern};
        let Fixture { space, env } = fixture().await;
        consume(&env, receive("w", any())).await;
        let random = Blake2b512Random::create_from_bytes(b"inert machinery waiters");
        let retired = stable_hash_provider::hash(&int(1));
        let inert = [
            (
                retire_pattern(),
                retire_continuation(&retired, &random, None),
            ),
            (lift_pattern(), lift_continuation(0, 0, &random, None)),
        ];
        for (pattern, continuation) in inert {
            space
                .consume(
                    vec![channels().data().clone()],
                    vec![pattern],
                    continuation,
                    false,
                    BTreeSet::new(),
                )
                .await
                .expect("the inert waiter parks");
        }
        let mut head = None;
        staged_plan(&env, |cx| {
            head = cx.head_waiter(&QueueDiscipline)?;
            Ok(())
        })
        .await
        .expect("the plan commits");
        let head = head.expect("the user waiter is the head");
        let parked =
            record::unwrap_continuation(&head.continuation).expect("a continuation record");
        assert_eq!(body_name(&parked.continuation), "w");
    }

    #[test]
    fn space_engine_outcome_dispatches_the_prefix_of_a_group_fire() {
        let result = |value: i64| RSpaceResult {
            channel: int(1),
            matched_datum: ListParWithRandom {
                pars: vec![int(value)],
                ..ListParWithRandom::default()
            },
            removed_datum: ListParWithRandom::default(),
            persistent: false,
        };
        let fired_with =
            |results: Vec<RSpaceResult<Par, ListParWithRandom>>| SpaceOpReceipt::Fired {
                comm: COMM {
                    consume: Consume::create(&vec![int(1)], &vec![any()], &named("k", None), false),
                    produces: Vec::new(),
                    peeks: BTreeSet::new(),
                    times_repeated: BTreeMap::new(),
                },
                continuation: ContResult {
                    continuation: wrapped("k"),
                    persistent: false,
                    channels: vec![int(1); results.len()],
                    patterns: vec![any(); results.len()],
                    peek: false,
                },
                results,
                produce: None,
            };
        let three = || vec![result(1), result(2), result(3)];
        // Changed by decision 123 (plan v4 C3d, V3-9, errata V4-17): the receipt has no deviation
        // and no refused pair. The text before:
        // let receipt = |ops| ParSpaceReceipt { ops };
        let receipt = |ops| ParSpaceReceipt {
            ops,
            deviation: None,
            refused: Vec::new(),
        };
        let dispatched = outcome(
            receipt(vec![fired_with(three())]),
            vec![Staged::FirePrefix],
            vec![1],
            None,
        )
        .expect("a prefix fire");
        assert_eq!(fired(&dispatched), vec![delivery("k", 1)]);
        let all = outcome(
            receipt(vec![fired_with(three())]),
            vec![Staged::Fire],
            Vec::new(),
            None,
        )
        .expect("a fire");
        assert_eq!(fired(&all), vec![("k".to_string(), vec![
            int(1),
            int(2),
            int(3)
        ])]);
        for kind in [Staged::Retire, Staged::Lift] {
            let internal = outcome(
                receipt(vec![fired_with(three())]),
                vec![kind],
                Vec::new(),
                None,
            )
            .expect("an internal fire");
            assert!(internal.fires.is_empty());
        }
        let cases = [
            (
                vec![Staged::FirePrefix],
                Vec::new(),
                "a prefix fire has no dispatched count",
            ),
            (
                vec![Staged::FirePrefix],
                vec![4],
                "a fire dispatches more results than its receipt holds",
            ),
            (
                vec![Staged::Fire],
                vec![1],
                "a dispatched count has no prefix fire",
            ),
        ];
        for (staged, prefixes, message) in cases {
            assert_eq!(
                outcome(receipt(vec![fired_with(three())]), staged, prefixes, None),
                Err(SpaceError::Inconsistent(message))
            );
        }
    }

    // ── The primitives of the join engine (P5c) ─────────────────────────────────────────────

    fn join_slot(bind: usize) -> record::JoinSlot {
        record::JoinSlot {
            ord: 0,
            bind,
            policy: record::WaiterPolicy::None,
        }
    }

    #[test]
    fn space_engine_join_fire_outcome_restores_bind_order() {
        let result = |value: i64| RSpaceResult {
            channel: int(value),
            matched_datum: ListParWithRandom {
                pars: vec![int(value)],
                ..ListParWithRandom::default()
            },
            removed_datum: ListParWithRandom::default(),
            persistent: false,
        };
        let fired_with =
            |continuation: TaggedContinuation,
             results: Vec<RSpaceResult<Par, ListParWithRandom>>| {
                SpaceOpReceipt::Fired {
                    comm: COMM {
                        consume: Consume::create(
                            &vec![int(1); results.len()],
                            &vec![any(); results.len()],
                            &named("k", None),
                            false,
                        ),
                        produces: Vec::new(),
                        peeks: BTreeSet::new(),
                        times_repeated: BTreeMap::new(),
                    },
                    continuation: ContResult {
                        continuation,
                        persistent: false,
                        channels: vec![int(1); results.len()],
                        patterns: vec![any(); results.len()],
                        peek: false,
                    },
                    results,
                    produce: None,
                }
            };
        // Changed by decision 123 (plan v4 C3d, V3-9, errata V4-17): the receipt has no deviation
        // and no refused pair. The text before:
        // let receipt = |ops| ParSpaceReceipt { ops };
        let receipt = |ops| ParSpaceReceipt {
            ops,
            deviation: None,
            refused: Vec::new(),
        };
        // The receipt lists the results in group order, and slot i names the bind of result i:
        // result 10 belongs to bind 2, result 20 to bind 0 and result 30 to bind 1.
        let join = record::wrap_join_continuation(
            &[join_slot(2), join_slot(0), join_slot(1)],
            &named("k", None),
        )
        .expect("a join record");
        let three = || vec![result(10), result(20), result(30)];
        let dispatched = outcome(
            receipt(vec![fired_with(join.clone(), three())]),
            vec![Staged::JoinFire],
            Vec::new(),
            None,
        )
        .expect("a join fire");
        assert_eq!(dispatched.fires.len(), 1);
        assert_eq!(dispatched.fires[0].continuation, named("k", None));
        assert_eq!(fired(&dispatched), vec![("k".to_string(), vec![
            int(20),
            int(30),
            int(10)
        ])]);
        // A join fire decodes a record of version 2 only, and a fire a record of version 1 only.
        let malformed = Err(SpaceError::Inconsistent("a malformed continuation record"));
        assert_eq!(
            outcome(
                receipt(vec![fired_with(wrapped("k"), three())]),
                vec![Staged::JoinFire],
                Vec::new(),
                None,
            ),
            malformed
        );
        assert_eq!(
            outcome(
                receipt(vec![fired_with(join.clone(), three())]),
                vec![Staged::Fire],
                Vec::new(),
                None,
            ),
            malformed
        );
        // A receipt with another number of results than the record has slots is Inconsistent.
        assert_eq!(
            outcome(
                receipt(vec![fired_with(join, vec![result(1), result(2)])]),
                vec![Staged::JoinFire],
                Vec::new(),
                None,
            ),
            Err(SpaceError::Inconsistent(
                "a join fire has not one result for each slot"
            ))
        );
    }

    /// Added by decision 124 (L12, plan v4 C7.11): `outcome` of a receipt whose deviation names
    /// row d decodes row d by the version of its continuation record, whatever row d staged,
    /// and cuts the receipt after row d. A record of version 1 decodes as one continuation, a
    /// join record of version 2 restores its bind order, and a `FirePrefix` row d dispatches
    /// the count of the deviation.
    #[test]
    fn outcome_decodes_row_d_by_its_record_version() {
        use rspace_plus_plus::rspace::space_store_handle::RowDeviation;
        let result = |value: i64| RSpaceResult {
            channel: int(value),
            matched_datum: ListParWithRandom {
                pars: vec![int(value)],
                ..ListParWithRandom::default()
            },
            removed_datum: ListParWithRandom::default(),
            persistent: false,
        };
        let fired_with =
            |continuation: TaggedContinuation,
             results: Vec<RSpaceResult<Par, ListParWithRandom>>| {
                SpaceOpReceipt::Fired {
                    comm: COMM {
                        consume: Consume::create(
                            &vec![int(1); results.len()],
                            &vec![any(); results.len()],
                            &named("k", None),
                            false,
                        ),
                        produces: Vec::new(),
                        peeks: BTreeSet::new(),
                        times_repeated: BTreeMap::new(),
                    },
                    continuation: ContResult {
                        continuation,
                        persistent: false,
                        channels: vec![int(1); results.len()],
                        patterns: vec![any(); results.len()],
                        peek: false,
                    },
                    results,
                    produce: None,
                }
            };
        // Row 0 takes the counter, and row d is row 1. The row after row d stands for the
        // retire group of row d, which the outcome does not read.
        let deviating = |row_d, dispatched| ParSpaceReceipt {
            ops: vec![
                fired_with(named("counter", None), Vec::new()),
                row_d,
                fired_with(named("retire", None), vec![result(9)]),
            ],
            deviation: Some(RowDeviation {
                index: 1,
                item_failed: Vec::new(),
                dispatched,
                vetoing: false,
                tail_read: TailRead::Ends,
                claims: None,
                reissue: None,
            }),
            refused: Vec::new(),
        };
        let staged = |kind| vec![Staged::Counter(CounterOp::Take), kind];
        let three = || vec![result(10), result(20), result(30)];
        let single = record::wrap_continuation(3, &named("k", None)).expect("a record");
        let join = record::wrap_join_continuation(
            &[join_slot(2), join_slot(0), join_slot(1)],
            &named("k", None),
        )
        .expect("a join record");
        // A record of version 1: row d staged a store, and it fired.
        let decoded = outcome(
            deviating(fired_with(single.clone(), three()), None),
            staged(Staged::Store),
            Vec::new(),
            None,
        )
        .expect("row d decodes its record of version 1");
        assert_eq!(fired(&decoded), vec![("k".to_string(), vec![
            int(10),
            int(20),
            int(30)
        ])]);
        // A join record of version 2: the results take the bind order of its slots.
        let decoded = outcome(
            deviating(fired_with(join, three()), None),
            staged(Staged::Fire),
            Vec::new(),
            None,
        )
        .expect("row d decodes its join record");
        assert_eq!(fired(&decoded), vec![("k".to_string(), vec![
            int(20),
            int(30),
            int(10)
        ])]);
        // A prefix row d dispatches the count of the deviation.
        let decoded = outcome(
            deviating(fired_with(single, three()), Some(1)),
            staged(Staged::FirePrefix),
            Vec::new(),
            None,
        )
        .expect("row d decodes its prefix fire");
        assert_eq!(fired(&decoded), vec![delivery("k", 10)]);
    }

    /// Added by decision 124 (plan v4 C7.9 and C7.11, rule L3, ruling RADD 1.4): the E3-a answer
    /// of a staged row search is the one shared rule (`matching::may_veto`). A continuation
    /// record of version 1 or 2 may veto its commit when it has a guard or the space has a
    /// theory, and not otherwise. The search reads the theory of the space from the plan.
    #[tokio::test]
    async fn staged_row_search_may_veto_answers_guard_or_theory() {
        let channel = channels().data().clone();
        let group = vec![channel.clone()];
        let patterns = vec![any()];
        let datum = record::wrap_datum(0, &send(1).data);
        let source = Produce::create(&channel, &datum, false);
        let records = |guard: Option<Par>| {
            let continuation = named("k", guard);
            [
                record::wrap_continuation(0, &continuation).expect("a record of version 1"),
                record::wrap_join_continuation(&[join_slot(0), join_slot(1)], &continuation)
                    .expect("a record of version 2"),
            ]
        };
        let nat = theory::resolve(&TheoryIdentity::builtin("Nat")).expect("Nat resolves");
        for (theory, guarded, may_veto) in [
            (false, false, false),
            (false, true, true),
            (true, false, true),
            (true, true, true),
        ] {
            let guard = match guarded {
                true => Some(new_gbool_par(true, Vec::new(), false)),
                false => None,
            };
            let checker = match theory {
                true => Some(Arc::clone(&nat)),
                false => None,
            };
            let Fixture { env, .. } = fixture().await;
            staged_plan_with(&env, checker, |cx| {
                for continuation in records(guard) {
                    let consume = Consume::create(&group, &patterns, &continuation, false);
                    let candidate = incoming_candidate(
                        channel.clone(),
                        source.clone(),
                        datum.clone(),
                        false,
                        datum.clone(),
                    );
                    let search = StagedRowSearch::<QueueDiscipline>::of_receive(
                        cx,
                        &consume,
                        &continuation,
                        false,
                        &channel,
                        candidate,
                        0,
                    );
                    assert_eq!(
                        search.may_veto(&Matcher, &continuation),
                        may_veto,
                        "theory {theory}, guard {guarded}"
                    );
                }
                Ok(())
            })
            .await
            .expect("the plan commits");
        }
    }

    /// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (a) and (f), U1):
    /// a scripted end of a row. Its tail read answers `Reissues`, and it hands out its re-issue and
    /// its finish rows once.
    struct ScriptedEnd {
        reissue: Option<RowReissue>,
        rows: Vec<ParFinishRow>,
    }

    impl RowEnd for ScriptedEnd {
        fn tail_read(
            &mut self,
            _lists: &ParRowLists<'_>,
            _matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        ) -> Result<TailRead, SpaceTxnError> {
            Ok(TailRead::Reissues)
        }

        fn tail_reissue(&mut self) -> Option<RowReissue> { self.reissue.take() }

        fn finish_rows(&mut self) -> Vec<ParFinishRow> { std::mem::take(&mut self.rows) }
    }

    /// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (a) and (f), U1):
    /// a machinery search answers the re-issue and the finish rows of its end (items G4 and G5).
    /// The base end `()` gives no re-issue and no rows. A scripted end gives its re-issue once and
    /// its one finish row once.
    #[test]
    fn staged_row_search_hands_out_the_parts_of_its_end() {
        let channels = vec![channels().data().clone()];
        let mut base = StagedRowSearch::<QueueDiscipline>::of_machinery(&channels, &[]);
        assert_eq!(base.tail_reissue(), None);
        assert!(base.finish_rows().is_empty());
        let reissue = RowReissue::new(1_u8);
        let row = ParFinishRow::Store(SpaceProduce {
            channel: channels[0].clone(),
            data: ListParWithRandom::default(),
            persist: false,
            outcome: ProduceOutcome::Store,
            before_observe: None,
            own: false,
            search: None,
        });
        let end = ScriptedEnd {
            reissue: Some(reissue.clone()),
            rows: vec![row],
        };
        let mut search =
            StagedRowSearch::<QueueDiscipline>::of_machinery(&channels, &[]).with_end(end);
        assert_eq!(search.tail_reissue(), Some(reissue), "its re-issue");
        assert_eq!(search.tail_reissue(), None, "a second hand-out");
        assert_eq!(search.finish_rows().len(), 1, "its finish row");
        assert!(search.finish_rows().is_empty(), "a second hand-out");
    }

    /// The data channels of the collections `a` and `b` in canonical group order (FR-1'), and the
    /// bind index of each group position, for a receive that binds `a` first and `b` second.
    fn join_group() -> (Vec<Par>, Vec<usize>) {
        let a = channels_of(&string("a")).data().clone();
        let b = channels_of(&string("b")).data().clone();
        match stable_hash_provider::hash(&a) <= stable_hash_provider::hash(&b) {
            true => (vec![a, b], vec![0, 1]),
            false => (vec![b, a], vec![1, 0]),
        }
    }

    /// The join waiter `name` of the receive `for (x <- a & y <- b)`, with a record of version 2.
    fn join_waiter(name: &str, binds: &[usize]) -> TaggedContinuation {
        let slots: Vec<record::JoinSlot> = binds.iter().map(|&bind| join_slot(bind)).collect();
        record::wrap_join_continuation(&slots, &named(name, None)).expect("a join record")
    }

    /// Stages `stage` in a plan of its own with the lock keys `keys`, whose own collection is
    /// the collection of the first data channel of `keys`, and commits it. Changed by decision
    /// 92: the plan also locks `C(sid)` and `S(sid)` when `keys` does not hold them, as
    /// `begin_join` does, because the anchor of the plan consumes `C(sid)`.
    async fn staged_group_plan(
        env: &SpaceEnv,
        keys: &[Par],
        stage: impl FnOnce(&mut Cx<'_, '_>) -> Result<(), SpaceError>,
    ) -> Result<SpaceOutcome, SpaceError> {
        // let mut txn = env.store.begin(keys).await?;
        let ch = channels();
        let mut lock_keys = Vec::with_capacity(keys.len() + 2);
        for key in [ch.config(), ch.space_counter()] {
            match keys.contains(key) {
                true => {}
                false => lock_keys.push(key.clone()),
            }
        }
        lock_keys.extend_from_slice(keys);
        let mut txn = env.store.begin(&lock_keys).await?;
        let matcher = SpaceMatcher::new(env);
        let introduction = Introduction::new(&env.budget, &CostAuthority::default());
        let config = SpaceConfig::new(SPACE.sid, "hashmap", "queue", SpaceQualifier::Default);
        // Added by decision 92: the job of the plan, as in `staged_plan_with`.
        let job = plan_job();
        let mut plan = Plan {
            data: &keys[0],
            config: &config,
            matcher: &matcher,
            introduction: &introduction,
            staged: Vec::with_capacity(MAX_STAGED),
            prefixes: Vec::new(),
            bumped: None,
            // Disabled by decision 90 (decision 69): Option P is gone.
            // legacy: LegacyChain::new(&env.budget),
            rounds: FireRounds::new(),
            mirror: MirrorCapture::inactive(),
            seq: None,
            theory: None,
            leases: Vec::new(),
            job: &job,
            // Added by decision 102 (plan v4 C4.2).
            picked: false,
            order: PickMode::Random,
            lifts: Vec::new(),
            // Added by decision 124 (L14, plan v4 C7.8): the plan reads one snapshot of the book.
            book: env.volatile.book(),
            // Added by decision 124 (plan v4 C7.11, ruling RADD 1.3): room for the class of each
            // staged row, as `staged` has, and no copy of the operation before the staging.
            tail_rows: Vec::<TailRow>::with_capacity(MAX_STAGED),
            operation: None,
        };
        stage(&mut Cx {
            txn: &mut txn,
            plan: &mut plan,
        })?;
        let Plan {
            staged,
            prefixes,
            // Disabled by decision 90 (decision 69): Option P is gone.
            // legacy,
            ..
        } = plan;
        Prepared::new(txn, move |receipt| {
            // Disabled by decision 90 (decision 69): Option P is gone.
            // legacy.finish()?;
            outcome(receipt, staged, prefixes, None)
        })
        .commit()
        .await
    }

    /// The datum record of the payload `value` with the order 0, and its stored copy on `channel`.
    fn stored_on(channel: &Par, value: i64) -> (ListParWithRandom, ParDatum) {
        let record = record::wrap_datum(0, &send_on(channel, value, false).data);
        let datum = Datum {
            a: record.clone(),
            persist: false,
            source: Produce::create(channel, &record, false),
        };
        (record, datum)
    }

    #[tokio::test]
    async fn space_engine_join_group_fires_line_up_and_dispatch_in_bind_order() {
        let Fixture { space, env } = fixture().await;
        let (group, binds) = join_group();
        let a = channels_of(&string("a")).data().clone();
        let b = channels_of(&string("b")).data().clone();
        // Park the join waiter `k` on the group, and store 2 on b.
        staged_group_plan(&env, &group, |cx| {
            cx.park_on(
                group.clone(),
                vec![any(), any()],
                join_waiter("k", &binds),
                false,
                BTreeSet::new(),
                // Added by decision 124 (plan v4 C7.9, ruling B-1): no E3-a search.
                None,
            )?;
            let (record, _) = stored_on(&b, 2);
            let introduction = Arc::clone(cx.introduction());
            cx.store_on(&b, record, false, &introduction)
        })
        .await
        .expect("the park and the store commit");
        // The produce of 1 on a fires the waiter with the incoming datum and the stored record.
        let outcome = staged_group_plan(&env, &group, |cx| {
            let waiter = cx.group_waiters(&group)?[0].clone();
            let stored = cx.data_on(&b)?[0].clone();
            let (incoming, _) = stored_on(&a, 1);
            let source = Produce::create(&a, &incoming, false);
            let payload = |record: &ListParWithRandom| {
                record::unwrap_datum(record).expect("a datum record").datum
            };
            let incoming_match = cx.bind(&any(), &payload(&incoming)).expect("any() binds");
            let stored_match = cx.bind(&any(), &payload(&stored.a)).expect("any() binds");
            let candidates = group
                .iter()
                .map(|channel| match *channel == a {
                    true => incoming_candidate(
                        a.clone(),
                        source.clone(),
                        incoming.clone(),
                        false,
                        incoming_match.clone(),
                    ),
                    false => receive_candidate(b.clone(), ReceiveFire {
                        head: stored.clone(),
                        matched: stored_match.clone(),
                        view_index: 0,
                    }),
                })
                .collect();
            let introduction = Arc::clone(cx.introduction());
            cx.fire_offer_group(
                &a,
                incoming,
                false,
                waiter,
                group.clone(),
                candidates,
                &introduction,
                Offered::InFlight,
                // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): no row search.
                None,
            )
        })
        .await
        .expect("the group fire commits");
        // The body sees the value of a first and the value of b second, whatever the group order.
        assert_eq!(fired(&outcome), vec![("k".to_string(), vec![
            int(1),
            int(2)
        ])]);
        assert!(space.get_data(&b).await.is_empty());
        assert!(space
            .get_waiting_continuations(group.clone())
            .await
            .is_empty());
        assert!(space.get_joins(a.clone()).await.is_empty());
        // The consume side: store 3 on a and 4 on b, then fire a new join waiter over both.
        staged_group_plan(&env, &group, |cx| {
            let introduction = Arc::clone(cx.introduction());
            for (channel, value) in [(&a, 3), (&b, 4)] {
                let (record, _) = stored_on(channel, value);
                cx.store_on(channel, record, false, &introduction)?;
            }
            Ok(())
        })
        .await
        .expect("the stores commit");
        let outcome = staged_group_plan(&env, &group, |cx| {
            let mut candidates = Vec::new();
            for channel in &group {
                let stored = cx.data_on(channel)?[0].clone();
                let payload = record::unwrap_datum(&stored.a)
                    .expect("a datum record")
                    .datum;
                let matched = cx.bind(&any(), &payload).expect("any() binds");
                candidates.push(receive_candidate(channel.clone(), ReceiveFire {
                    head: stored,
                    matched,
                    view_index: 0,
                }));
            }
            cx.fire_join(
                group.clone(),
                vec![any(), any()],
                join_waiter("j", &binds),
                false,
                BTreeSet::new(),
                candidates,
                // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): no row search.
                None,
            )
        })
        .await
        .expect("the join fire commits");
        assert_eq!(fired(&outcome), vec![("j".to_string(), vec![
            int(3),
            int(4)
        ])]);
        assert!(space.get_data(&a).await.is_empty());
        // A group fire that does not line up stages nothing.
        space.take_event_log().await;
        let misaligned = staged_group_plan(&env, &group, |cx| {
            cx.fire_join(
                vec![a.clone()],
                vec![any()],
                join_waiter("x", &binds),
                false,
                BTreeSet::new(),
                Vec::new(),
                // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): no row search.
                None,
            )
        })
        .await;
        assert_eq!(
            misaligned,
            Err(SpaceError::Inconsistent(
                "a join fire does not line up its channels, patterns and candidates"
            ))
        );
        let misaligned = staged_group_plan(&env, &group, |cx| {
            let waiter = ParWaiter {
                patterns: vec![any(), any()],
                continuation: join_waiter("x", &binds),
                persist: false,
                peeks: BTreeSet::new(),
                source: Consume::create(&group, &vec![any(), any()], &named("x", None), false),
            };
            let (incoming, _) = stored_on(&a, 5);
            let introduction = Arc::clone(cx.introduction());
            cx.fire_offer_group(
                &a,
                incoming,
                false,
                waiter,
                group.clone(),
                Vec::new(),
                &introduction,
                Offered::InFlight,
                // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): no row search.
                None,
            )
        })
        .await;
        assert_eq!(
            misaligned,
            Err(SpaceError::Inconsistent(
                "a group fire does not line up its channels and candidates"
            ))
        );
        assert!(space.take_event_log().await.is_empty());
    }

    #[tokio::test]
    async fn space_engine_bump_on_stages_the_bump_of_the_collection() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        let random = Blake2b512Random::create_from_bytes(b"touch");
        // Two bumps of the queue collection in one plan: the first opens the counter, the second
        // steps it, as the bumps of two operations do.
        staged_group_plan(&env, ch.lock_keys(), |cx| {
            let introduction = Arc::clone(cx.introduction());
            let authority = OperationAuthority {
                continuation: None,
                introduction: &introduction,
            };
            assert_eq!(cx.bump_on(ch.counters(), &random, authority)?, 0);
            assert_eq!(
                cx.bump_on(ch.counters(), &random.split_byte(1), authority)?,
                1
            );
            Ok(())
        })
        .await
        .expect("the bumps commit");
        assert_eq!(labels(&space.take_event_log().await), vec![
            "consume S",
            "comm S",
            "produce S",
            "produce K",
            "consume K",
            "comm K",
            "produce K"
        ]);
        assert_eq!(counter(&space, ch.counter()).await, vec![2]);
        assert_eq!(counter(&space, ch.space_counter()).await, vec![1]);
        // A produce on the collection now takes the next order.
        produce(&env, send(7)).await;
        assert_eq!(records(&space).await, vec![(2, vec![int(7)], false)]);
    }

    #[test]
    fn space_engine_join_peek_set_covers_every_bind() {
        assert_eq!(peek_set_all(3), Ok(BTreeSet::from([0, 1, 2])));
        assert_eq!(peek_set_all(0), Ok(BTreeSet::new()));
        assert_eq!(peek_set_all(1), Ok(peek_set(true)));
    }

    #[test]
    fn space_engine_operation_ord_without_a_counter_is_zero() {
        assert_eq!(operation_ord(None, None), Ok(0));
        assert_eq!(operation_ord(Some(0), None), Ok(0));
        assert_eq!(
            operation_ord(Some(3), None),
            Err(SpaceError::Inconsistent(
                "a re-issued order of a collection without a counter is not 0"
            ))
        );
        assert_eq!(operation_ord(None, Some(5)), Ok(5));
        assert_eq!(operation_ord(Some(2), Some(5)), Ok(2));
        assert_eq!(
            operation_ord(Some(5), Some(5)),
            Err(SpaceError::Inconsistent(
                "a re-issued order is not an order of the counter"
            ))
        );
    }

    #[tokio::test]
    async fn space_engine_allocation_without_an_allocator_is_baseline() {
        let Fixture { space, env } = fixture().await;
        let queue = Queue::default();
        for agent in [&queue as &dyn SpaceAgent, &FamilyAgent] {
            match agent.allocate(&env, alloc_op(2)).await {
                Ok(Allocation::Baseline) => {}
                Ok(Allocation::Indices(_)) => panic!("a HashMap space has no allocator"),
                Err(error) => panic!("the allocation fails: {error}"),
            }
            assert!(space.take_event_log().await.is_empty());
        }
        let allocator = namespace::gprivate(namespace::allocator_channel(SPACE));
        assert!(space.get_data(&allocator).await.is_empty());
        // A space without a config is a dangling handle.
        let empty = play().await;
        let env = env_over(empty.clone(), &budget());
        for agent in [&queue as &dyn SpaceAgent, &FamilyAgent] {
            assert!(matches!(
                agent.allocate(&env, alloc_op(1)).await,
                Err(SpaceError::DanglingHandle { space }) if space == SPACE.sid
            ));
        }
        assert!(empty.take_event_log().await.is_empty());
    }

    #[tokio::test]
    async fn space_engine_allocator_store_creates_and_allocates_its_names() {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        let agent = GenericSpace::<Counted, QueueDiscipline>::default();
        agent
            .prepare_create(
                &env,
                creation(config_of(Counted::NAME, QueueDiscipline::NAME)),
            )
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        let allocator = namespace::gprivate(namespace::allocator_channel(SPACE));
        let created = space.take_event_log().await;
        assert_eq!(
            created.len(),
            4,
            "the config, the counter, the allocator and the handle"
        );
        assert_eq!(counter(&space, &allocator).await, vec![0]);
        for (count, expected) in [(3, vec![0, 1, 2]), (2, vec![3, 4])] {
            let indices = match agent
                .allocate(&env, alloc_op(count))
                .await
                .expect("the allocation stages")
            {
                Allocation::Indices(prepared) => {
                    prepared.commit().await.expect("the allocation commits")
                }
                Allocation::Baseline => panic!("the store has an allocator"),
            };
            assert_eq!(indices, expected);
            assert_eq!(
                space.take_event_log().await.len(),
                3,
                "the take of the allocator and the put of its next value"
            );
        }
        assert_eq!(counter(&space, &allocator).await, vec![5]);
    }

    #[tokio::test]
    async fn space_engine_family_agent_runs_a_queue_as_its_own_agent_does() {
        let script = || {
            vec![
                SpaceOp::Produce(send(1)),
                SpaceOp::Consume(peek("p")),
                SpaceOp::Consume(receive("w", any())),
                SpaceOp::Consume(receive("x", exactly(2))),
                SpaceOp::Produce(send(2)),
                SpaceOp::Produce(send_forever(3)),
            ]
        };
        let queue = Queue::default();
        let mut runs = Vec::new();
        for agent in [&queue as &dyn SpaceAgent, &FamilyAgent] {
            let space = play().await;
            let env = env_over(space.clone(), &budget());
            agent
                .prepare_create(&env, creation(config()))
                .await
                .expect("the creation stages")
                .commit()
                .await
                .expect("the creation commits");
            let mut fires = Vec::new();
            for op in script() {
                let outcome = run_op(agent, &env, op)
                    .await
                    .expect("the operation commits");
                fires.push(fired(&outcome));
            }
            runs.push((
                fires,
                labels(&space.take_event_log().await),
                records(&space).await,
                waiters(&space).await,
            ));
        }
        assert_eq!(runs[0], runs[1]);
    }

    /// HS-2 to HS-5 through the family agent (P7 step 3). A send with a payload fails with
    /// `PayloadNotEmpty`, and a receive with a pattern fails with `PatternNotEmpty`. Both fail
    /// before any staging: the store logs no event and holds no record and no waiter. Units then
    /// follow the discipline of the family: the queue takes its oldest unit and the stack its
    /// newest, by the `ord` of the envelopes, the bag counts its units, the set retires every
    /// equal unit at once, and the cell holds one. The factory gives every HashSet space the
    /// qualifier `seq`. The store rules do not depend on the qualifier, so this test uses a
    /// durable config and needs no operation order.
    #[tokio::test]
    async fn space_engine_hashset_families_take_units_only() {
        fn unit(n: i64) -> ProduceOp {
            let sent = send(n);
            ProduceOp {
                data: ListParWithRandom {
                    pars: Vec::new(),
                    ..sent.data.clone()
                },
                ..sent
            }
        }
        fn take(name: &str) -> ConsumeOp {
            receive(name, BindPattern {
                patterns: Vec::new(),
                remainder: None,
                free_count: 0,
            })
        }
        async fn hashset(inner: SpaceInner) -> Fixture {
            let space = play().await;
            let env = env_over(space.clone(), &budget());
            let family = SpaceFactoryUrn {
                outer: SpaceOuter::HashSet,
                inner,
            };
            FamilyAgent
                .prepare_create(&env, SpaceCreation {
                    family,
                    ..creation(config_of("hashset", inner.as_str()))
                })
                .await
                .expect("the creation stages")
                .commit()
                .await
                .expect("the creation commits");
            space.take_event_log().await;
            Fixture { space, env }
        }
        let ords = |records: Vec<(i64, Vec<Par>, bool)>| -> Vec<i64> {
            assert!(
                records.iter().all(|(_, payload, _)| payload.is_empty()),
                "{records:?}"
            );
            records.into_iter().map(|(ord, _, _)| ord).collect()
        };
        for inner in [
            SpaceInner::Bag,
            SpaceInner::Queue,
            SpaceInner::Stack,
            SpaceInner::Set,
            SpaceInner::Cell,
            SpaceInner::PriorityQueue,
        ] {
            let Fixture { space, env } = hashset(inner).await;
            assert_eq!(
                run_op(&FamilyAgent, &env, SpaceOp::Produce(send(1))).await,
                Err(SpaceError::PayloadNotEmpty),
                "{inner:?}"
            );
            for pattern in [any(), exactly(1)] {
                assert_eq!(
                    run_op(&FamilyAgent, &env, SpaceOp::Consume(receive("w", pattern))).await,
                    Err(SpaceError::PatternNotEmpty),
                    "{inner:?}"
                );
            }
            assert!(space.take_event_log().await.is_empty(), "{inner:?}");
            assert!(records(&space).await.is_empty(), "{inner:?}");
            assert!(waiters_on(&space, &channel()).await.is_empty(), "{inner:?}");
            // A unit receive on an empty collection waits, and a unit send fires it.
            let parked = run_op(&FamilyAgent, &env, SpaceOp::Consume(take("t")))
                .await
                .expect("a unit receive parks");
            assert!(parked.fires.is_empty(), "{inner:?}");
            assert_eq!(waiters_on(&space, &channel()).await.len(), 1, "{inner:?}");
            let fired = run_op(&FamilyAgent, &env, SpaceOp::Produce(unit(1)))
                .await
                .expect("a unit send fires the waiter");
            assert_eq!(fired.fires.len(), 1, "{inner:?}");
            assert!(records(&space).await.is_empty(), "{inner:?}");
            assert!(waiters_on(&space, &channel()).await.is_empty(), "{inner:?}");
        }
        // The queue takes its oldest unit, and the stack its newest.
        for (inner, taken) in [(SpaceInner::Queue, 0), (SpaceInner::Stack, 2)] {
            let Fixture { space, env } = hashset(inner).await;
            for n in 0..3 {
                run_op(&FamilyAgent, &env, SpaceOp::Produce(unit(n)))
                    .await
                    .expect("a unit send stores");
            }
            let stored = ords(records(&space).await);
            assert_eq!(stored.len(), 3, "{inner:?}");
            let fired = run_op(&FamilyAgent, &env, SpaceOp::Consume(take("t")))
                .await
                .expect("a unit receive fires");
            assert_eq!(fired.fires.len(), 1, "{inner:?}");
            let mut rest = stored.clone();
            rest.remove(taken);
            assert_eq!(ords(records(&space).await), rest, "{inner:?}");
        }
        // The bag counts its units, and the set keeps one unit class: one receive retires both
        // equal units.
        for (inner, left) in [(SpaceInner::Bag, 1), (SpaceInner::Set, 0)] {
            let Fixture { space, env } = hashset(inner).await;
            for n in 0..2 {
                run_op(&FamilyAgent, &env, SpaceOp::Produce(unit(n)))
                    .await
                    .expect("a unit send stores");
            }
            run_op(&FamilyAgent, &env, SpaceOp::Consume(take("t")))
                .await
                .expect("a unit receive fires");
            assert_eq!(records(&space).await.len(), left, "{inner:?}");
        }
        // The cell holds one unit.
        let Fixture { space, env } = hashset(SpaceInner::Cell).await;
        run_op(&FamilyAgent, &env, SpaceOp::Produce(unit(0)))
            .await
            .expect("the first unit stores");
        space.take_event_log().await;
        assert_eq!(
            run_op(&FamilyAgent, &env, SpaceOp::Produce(unit(1))).await,
            Err(SpaceError::CellOccupied)
        );
        assert_eq!(records(&space).await.len(), 1);
    }

    #[tokio::test]
    async fn space_engine_family_agent_rejects_a_family_that_is_not_wired() {
        let random = Blake2b512Random::create_from_bytes(b"config");
        // The config names no family, so no arm is ever wired for it.
        let unknown = config_of("hashmap", "deque");
        let unavailable = SpaceError::FamilyUnavailable {
            outer: "hashmap".to_string(),
            inner: "deque".to_string(),
        };
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        install(&env, vec![record::config_record(unknown.clone(), &random)]).await;
        space.take_event_log().await;
        for op in [
            SpaceOp::Produce(send(1)),
            SpaceOp::Consume(receive("w", any())),
        ] {
            assert_eq!(
                run_op(&FamilyAgent, &env, op).await,
                Err(unavailable.clone())
            );
        }
        assert!(matches!(
            FamilyAgent.allocate(&env, alloc_op(1)).await,
            Err(error) if error == unavailable
        ));
        assert!(space.take_event_log().await.is_empty());
        // A creation goes to the agent of the family of its factory, which checks the config.
        let fresh = play().await;
        let env = env_over(fresh.clone(), &budget());
        // Changed by P7 step 3 (design v3 HS-7): HashSet is wired, so every family has an
        // agent. A creation whose family pair is no family, which no factory can send, has none.
        // // HashSet is wired in P7, so its factory has no agent yet.
        // let unwired_factory = SpaceCreation {
        //     family: SpaceFactoryUrn {
        //         outer: SpaceOuter::HashSet,
        //         inner: SpaceInner::Set,
        //     },
        //     ..creation(config())
        // };
        // assert_eq!(
        //     FamilyAgent
        //         .prepare_create(&env, unwired_factory)
        //         .await
        //         .err(),
        //     Some(SpaceError::FamilyUnavailable {
        //         outer: "hashset".to_string(),
        //         inner: "set".to_string(),
        //     })
        // );
        let unwired_factory = SpaceCreation {
            family: SpaceFactoryUrn {
                outer: SpaceOuter::HashSet,
                inner: SpaceInner::VectorDb,
            },
            ..creation(config())
        };
        assert_eq!(
            FamilyAgent
                .prepare_create(&env, unwired_factory)
                .await
                .err(),
            Some(SpaceError::FamilyUnavailable {
                outer: "hashset".to_string(),
                inner: "vectordb".to_string(),
            })
        );
        let mislabeled = SpaceCreation {
            family: SpaceFactoryUrn::HASHMAP_QUEUE,
            ..creation(unknown)
        };
        assert_eq!(
            FamilyAgent.prepare_create(&env, mislabeled).await.err(),
            Some(unavailable)
        );
        assert!(fresh.take_event_log().await.is_empty());
    }

    // ── The bound of a side (P6, design v3 4.0) ──────────────────────────────────────────────

    #[test]
    fn space_side_bound_defaults_to_the_capacity_of_the_discipline() {
        use crate::rust::interpreter::spaces::disciplines::bag::BagDiscipline;
        use crate::rust::interpreter::spaces::disciplines::cell::CellDiscipline;

        let config = SpaceConfig::new(SPACE.sid, "hashmap", "cell", SpaceQualifier::Default);
        // A cell holds one record and one waiter (F15), and its excess is `CellOccupied`.
        for side in [Side::Data, Side::Waiters] {
            assert_eq!(
                CellDiscipline.bound(&config, side),
                Ok(Some(SideBound::Cell(1)))
            );
            assert_eq!(BagDiscipline.bound(&config, side), Ok(None));
            assert_eq!(QueueDiscipline.bound(&config, side), Ok(None));
        }
        assert_eq!(SideBound::Cell(1).limit(), 1);
        assert_eq!(SideBound::Cell(1).full(), SpaceError::CellOccupied);
        // The record bound of a VectorDB space fails with its own code (P6).
        assert_eq!(SideBound::Records(7).limit(), 7);
        assert_eq!(SideBound::Records(7).full(), SpaceError::CapacityExceeded);
    }

    // ── The theory seam (P7, TH-5, TH-6, TH-8) ───────────────────────────────────────────────

    fn theory_config(identity: Option<TheoryIdentity>) -> SpaceConfig {
        SpaceConfig {
            theory: identity,
            ..SpaceConfig::new(
                SPACE.sid,
                HashMapOuter::NAME,
                QueueDiscipline::NAME,
                SpaceQualifier::Default,
            )
        }
    }

    fn send_values(values: Vec<Par>) -> SpaceOp {
        SpaceOp::Produce(ProduceOp {
            data: ListParWithRandom {
                pars: values,
                ..send(0).data
            },
            ..send(0)
        })
    }

    /// TH-5: the admission of a send checks its values in order. The first value that is not
    /// `Proven` stops it with its index. A theory whose work limit is zero cannot decide a value.
    /// A receive and a space without a theory admit nothing.
    #[test]
    fn theory_admission_checks_each_value_in_order() {
        let nat = theory_config(Some(TheoryIdentity::builtin("Nat")));
        let admission = Admission::of(&nat, &send_values(vec![int(0), int(1), int(2)]))
            .expect("Nat admits three naturals");
        assert!(admission.theory.is_some());
        assert!(admission.leases.is_empty(), "a builtin gives no lease");

        assert_eq!(
            Admission::of(&nat, &send_values(vec![int(0), int(-1), int(2)])).err(),
            Some(SpaceError::TheoryRefuted {
                theory: "Nat".to_string(),
                index: 1,
            })
        );

        let starved = theory_config(Some(TheoryIdentity {
            limit_profile: TheoryUsage::new(0, 0),
            ..TheoryIdentity::builtin("Nat")
        }));
        assert_eq!(
            Admission::of(&starved, &send_values(vec![int(0)])).err(),
            Some(SpaceError::TheoryUndetermined {
                theory: "Nat".to_string(),
                index: 0,
            })
        );

        for (config, op) in [
            (nat.clone(), SpaceOp::Consume(receive("w", any()))),
            (theory_config(None), send_values(vec![int(-1)])),
        ] {
            let admission = Admission::of(&config, &op).expect("nothing to admit");
            assert!(admission.leases.is_empty());
        }
        let unavailable = theory_config(Some(TheoryIdentity::builtin("Real")));
        assert_eq!(
            Admission::of(&unavailable, &send_values(vec![int(0)])).err(),
            Some(SpaceError::TheoryUnavailable("Real".to_string()))
        );
    }

    /// TH-6: after the guard admitted a match, the theory checks each stored datum of the match,
    /// and a datum that it does not prove refuses the match. A refusing guard refuses the match,
    /// and a space without a theory admits every stored datum.
    #[tokio::test]
    async fn theory_match_checks_each_stored_datum() {
        let admitting = named("w", None);
        let refusing = named("w", Some(new_gbool_par(false, Vec::new(), false)));
        let natural = ListParWithRandom {
            pars: vec![int(1)],
            ..ListParWithRandom::default()
        };
        let negative = ListParWithRandom {
            pars: vec![int(-1)],
            ..ListParWithRandom::default()
        };
        fn stored(payload: &ListParWithRandom) -> StoredMatch<'_> {
            StoredMatch {
                payload,
                captures: payload,
            }
        }
        let proven = [stored(&natural), stored(&natural)];
        let refuted = [stored(&natural), stored(&negative)];
        let nat = theory::resolve(&TheoryIdentity::builtin("Nat")).expect("Nat resolves");
        for (theory, checks, expected) in [
            (Some(nat.clone()), &proven[..], true),
            (Some(nat.clone()), &refuted[..], false),
            (Some(nat.clone()), &[][..], true),
            (None, &refuted[..], true),
        ] {
            let Fixture { env, .. } = fixture().await;
            staged_plan_with(&env, theory, |cx| {
                assert_eq!(cx.admits(&admitting, &natural, checks), Ok(expected));
                assert_eq!(cx.admits(&refusing, &natural, checks), Ok(false));
                Ok(())
            })
            .await
            .expect("the plan commits");
        }
    }

    /// Added by decision 124 (L6, base B:operation_context.rs:238-263): a theory check reserves
    /// nothing, and the fire takes one lease for each proven stored datum (TH-9, CR30 D-b).
    #[tokio::test]
    async fn theory_check_takes_no_lease_and_the_fire_takes_one_for_each_stored_datum() {
        use crate::rust::interpreter::spaces::theory::test_theories::{self, TestBehavior};
        fn stored(payload: &ListParWithRandom) -> StoredMatch<'_> {
            StoredMatch {
                payload,
                captures: payload,
            }
        }
        let admitting = named("w", None);
        let natural = ListParWithRandom {
            pars: vec![int(1)],
            ..ListParWithRandom::default()
        };
        let checks = [stored(&natural), stored(&natural)];
        let leasing = Some(test_theories::identity(TestBehavior::Leasing));
        let nat = Some(TheoryIdentity::builtin("Nat"));
        for (identity, leases) in [(leasing, 2), (nat, 0), (None, 0)] {
            let theory = identity.map(|identity| theory::resolve(&identity).expect("it resolves"));
            let Fixture { env, .. } = fixture().await;
            staged_plan_with(&env, theory, |cx| {
                assert_eq!(cx.admits(&admitting, &natural, &checks), Ok(true));
                assert!(cx.plan.leases.is_empty(), "a check reserves nothing");
                cx.lease_stored(checks.len());
                assert_eq!(cx.plan.leases.len(), leases, "the fire takes its leases");
                Ok(())
            })
            .await
            .expect("the plan commits");
        }
    }

    /// TH-5: the admission of a send runs after the config read and the gates, and before any
    /// staging or bump. A send that fails at the params gate fails there, whatever its values.
    /// A send that the theory refutes stages nothing: no record and no event. An admitted send
    /// stores its record.
    #[tokio::test]
    async fn theory_admission_precedes_any_staging() {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        create(
            &env,
            creation(theory_config(Some(TheoryIdentity::builtin("Nat"))).to_par()),
        )
        .await;
        space.take_event_log().await;
        let unknown = SpaceOp::Produce(ProduceOp {
            params: BTreeMap::from([("zz".to_string(), int(1))]),
            ..send(-1)
        });
        assert!(matches!(
            run_op(&FamilyAgent, &env, unknown).await,
            Err(SpaceError::UnknownParam { .. })
        ));
        assert_eq!(
            run_op(&FamilyAgent, &env, send_values(vec![int(-1)]))
                .await
                .map(|_| ()),
            Err(SpaceError::TheoryRefuted {
                theory: "Nat".to_string(),
                index: 0,
            })
        );
        assert!(
            space.take_event_log().await.is_empty(),
            "a refuted send stages nothing"
        );
        assert!(records(&space).await.is_empty());
        run_op(&FamilyAgent, &env, send_values(vec![int(1)]))
            .await
            .expect("Nat admits a natural");
        assert_eq!(records(&space).await.len(), 1);
    }

    /// TH-6 (`theory_refusal_is_guard_refusal` of Match.v): after the guard, the theory of the
    /// space checks each stored datum of a match, and a verdict that is not `Proven` refuses the
    /// match as a guard refusal does. A datum in flight is never checked. For generated guards,
    /// theories, values and numbers of stored data, `Cx::admits` gives the guard result and the
    /// verdicts of the stored data, conjoined, and `Cx::matches` checks the datum only when it
    /// is held in the store.
    #[test]
    fn prop_theory_refusal_is_guard_refusal() {
        use proptest::prelude::{any as arbitrary, prop_assert_eq, prop_oneof, Just};
        use proptest::test_runner::{Config, RngSeed, TestCaseError, TestRunner};

        use crate::rust::interpreter::spaces::theory::test_theories::{self, TestBehavior};

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Checker {
            Without,
            Proves,
            Refutes,
            Starved,
        }
        let checkers = prop_oneof![
            Just(Checker::Without),
            Just(Checker::Proves),
            Just(Checker::Refutes),
            Just(Checker::Starved),
        ];
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let mut runner = TestRunner::new(Config {
            cases: 64,
            rng_seed: RngSeed::Fixed(0x0E0D_2026_1009_0046),
            failure_persistence: None,
            ..Config::default()
        });
        runner
            .run(
                &(checkers, arbitrary::<bool>(), 0usize..3, -2i64..3),
                |(checker, guard, stored, value)| {
                    let theory = match checker {
                        Checker::Without => None,
                        Checker::Proves => Some(test_theories::identity(TestBehavior::RefuteValue)),
                        Checker::Refutes => {
                            Some(test_theories::identity(TestBehavior::RefuteMatch))
                        }
                        Checker::Starved => Some(TheoryIdentity {
                            limit_profile: TheoryUsage::new(0, 0),
                            ..TheoryIdentity::builtin("Nat")
                        }),
                    }
                    .map(|identity| theory::resolve(&identity).expect("the theory resolves"));
                    let proves = matches!(checker, Checker::Without | Checker::Proves);
                    let continuation = named("w", Some(new_gbool_par(guard, Vec::new(), false)));
                    let data = ListParWithRandom {
                        pars: vec![int(value)],
                        ..ListParWithRandom::default()
                    };
                    let records = vec![data.clone(); stored];
                    let mut seen = [false; 3];
                    runtime
                        .block_on(async {
                            let Fixture { env, .. } = fixture().await;
                            staged_plan_with(&env, theory, |cx| {
                                let checks: Vec<StoredMatch<'_>> = records
                                    .iter()
                                    .map(|payload| StoredMatch {
                                        payload,
                                        captures: &data,
                                    })
                                    .collect();
                                seen[0] = cx.admits(&continuation, &data, &checks)?;
                                seen[1] = cx
                                    .matches(&any(), &data, &continuation, Held::Stored)?
                                    .is_some();
                                seen[2] = cx
                                    .matches(&any(), &data, &continuation, Held::InFlight)?
                                    .is_some();
                                Ok(())
                            })
                            .await
                        })
                        .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
                    prop_assert_eq!(seen[0], guard && (stored == 0 || proves));
                    prop_assert_eq!(seen[1], guard && proves);
                    prop_assert_eq!(seen[2], guard);
                    Ok(())
                },
            )
            .expect("TH-6 holds");
    }

    // ── Anchors and the two-phase lock (decisions 92 and 94) ──────────────────────────────────

    /// The record of a send of `value` whose random state is the random state of `job`.
    fn record_of(job: &SpaceJob, value: i64) -> ListParWithRandom {
        record::wrap_datum(0, &ListParWithRandom {
            random_state: job.random_state.clone(),
            ..send(value).data
        })
    }

    /// Stages the anchor of `job` in a transaction of `env` over `[C(sid), S(sid)]`, with the
    /// introduction of `authority`, and commits it.
    async fn commit_anchor(
        env: &SpaceEnv,
        job: &SpaceJob,
        authority: &CostAuthority,
    ) -> ParSpaceReceipt {
        let ch = channels();
        let mut txn = env
            .begin(&[ch.config().clone(), ch.space_counter().clone()])
            .await
            .expect("begin over C(sid) and S(sid)");
        let matcher = SpaceMatcher::new(env);
        let introduction = Introduction::new(&env.budget, authority);
        anchor::stage(&mut txn, job, &matcher, &introduction).expect("the anchor stages");
        txn.commit()
            .await
            .expect("the anchor is the job's own, so its transaction commits")
    }

    /// The labels `labels` as owned strings.
    fn owned(labels: &[&str]) -> Vec<String> {
        labels.iter().map(|label| label.to_string()).collect()
    }

    /// Decision 92: the anchor of a job is one consume of the persistent config record on
    /// `C(sid)`, with the counter pattern and `K_anchor`. The body of `K_anchor` is `GInt(0)`,
    /// and its random state is `RandomPurpose::Anchor.derive(r).split_byte(0)` of the random
    /// state `r` of the job (D3). It carries the machinery seal of the cost authority of the
    /// job. The anchor fires without peeks and keeps the config record in place. Its consume
    /// registers the payer introduction, which carries no authority (D2). The anchor is its
    /// job's own, so a transaction with the footprint key commits it.
    /// Changed by decision 124 (plan v2 C7.4, decision 99): the anchor takes the machinery seal
    /// and the payer introduction.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_anchor_consumes_the_config_record_and_keeps_it() {
        let space = play().await;
        let budget = budget();
        let _scope = budget.enter_comm_accounting_scope();
        let env = env_over(space.clone(), &budget);
        create(&env, creation(config())).await;
        space.take_event_log().await;
        let ch = channels();
        let config_records = space.get_data(ch.config()).await;
        let job = job_of("anchored job", Some(region(3)));
        let receipt = commit_anchor(&env, &job, &region(4)).await;
        assert!(matches!(receipt.ops.as_slice(), [
            SpaceOpReceipt::Fired { .. }
        ]));
        let log = space.take_event_log().await;
        assert_eq!(labels(&log), owned(&ANCHOR));
        let consume = Consume::create(
            &vec![ch.config().clone()],
            &vec![counter_pattern()],
            &counter_continuation(
                0,
                &RandomPurpose::Anchor
                    .derive(&Blake2b512Random::from_bytes(&job.random_state))
                    .split_byte(0),
                // Changed by decision 124 (plan v2 C7.4, decision 99): the machinery seal. The
                // text before:
                // Some(region(3)),
                Some(machinery_seal(&budget, &region(3))),
            ),
            false,
        );
        assert_eq!(log[0], Event::IoEvent(IOEvent::Consume(consume.clone())));
        let [_, Event::Comm(comm)] = log.as_slice() else {
            panic!("the anchor fires: {log:?}");
        };
        assert_eq!(comm.consume, consume);
        assert!(comm.peeks.is_empty());
        assert_eq!(comm.produces, vec![config_records[0].source.clone()]);
        assert!(comm.produces[0].persistent);
        // Changed by decision 124 (plan v2 C7.4, decision 99): the anchor registers the payer
        // introduction. It carries no authority, so the anchor registers the fallback authority
        // of its identity, which a budget without registrations also gives.
        assert_eq!(
            registered(&budget, &log[0]),
            registered(&self::budget(), &log[0])
        );
        assert_eq!(space.get_data(ch.config()).await, config_records);
    }

    /// Decision 92: the anchors of two branches over one pre-state consume the same persistent
    /// config produce. Each COMM puts the config produce into `produces_consumed`. Merge check
    /// #1 skips persistent produces, so the branches merge. Negative control: the same logs
    /// with a linear config produce race on `C(sid)`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_anchors_of_two_branches_merge() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        let config_produce = space.get_data(ch.config()).await[0].source.clone();
        let mut logs = Vec::with_capacity(2);
        for branch in ["left branch", "right branch"] {
            commit_anchor(&env, &job_of(branch, None), &CostAuthority::default()).await;
            logs.push(space.take_event_log().await);
        }
        let index = |log: Vec<Event>| {
            EventLogIndex::new(
                log,
                |produce: &Produce| *produce == config_produce,
                |_: &Produce| false,
                NumberChannelsDiff::new(),
            )
        };
        let (left, right) = (index(logs[0].clone()), index(logs[1].clone()));
        for branch in [&left, &right] {
            assert!(branch.produces_consumed.0.contains(&config_produce));
        }
        assert!(merging_logic::conflicts(&left, &right).0.is_empty());
        let linear = |log: &[Event]| -> Vec<Event> {
            log.iter()
                .map(|event| match event {
                    Event::Comm(comm) => Event::Comm(COMM {
                        produces: comm
                            .produces
                            .iter()
                            .map(|produce| Produce {
                                persistent: false,
                                ..produce.clone()
                            })
                            .collect(),
                        ..comm.clone()
                    }),
                    other => other.clone(),
                })
                .collect()
        };
        let raced = merging_logic::conflicts(&index(linear(&logs[0])), &index(linear(&logs[1])));
        assert_eq!(raced.0.into_iter().collect::<Vec<_>>(), vec![
            stable_hash_provider::hash(ch.config())
        ]);
    }

    /// Decision 92: the first operation of a transaction is its job's own (`Cx::anchor_before`).
    /// The anchor goes before a first store whose record does not carry the random state of the
    /// job, and a later store of the same kind stages no second anchor. A plan whose first store
    /// is the job's own stages no anchor.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_anchor_goes_before_a_shared_first_operation() {
        let Fixture { space, env } = fixture().await;
        staged_plan(&env, |cx| {
            cx.store(record::wrap_datum(0, &send(1).data), false)?;
            cx.store(record::wrap_datum(0, &send(2).data), false)
        })
        .await
        .expect("the anchored plan commits");
        assert_eq!(
            labels(&space.take_event_log().await),
            owned(&["consume C", "comm C", "produce D", "produce D"])
        );
        staged_plan(&env, |cx| {
            cx.store(record_of(&plan_job(), 3), false)?;
            cx.store(record::wrap_datum(0, &send(4).data), false)
        })
        .await
        .expect("the plan commits");
        assert_eq!(
            labels(&space.take_event_log().await),
            owned(&["produce D", "produce D"])
        );
        assert_eq!(records(&space).await.len(), 4);
    }

    /// Added by decision 102 (plan v4 C7.22, rule CR-19 case 3, F22): a produce-side fire that
    /// re-offers a stored persistent record is not the job's own, also when the record carries
    /// the random state of the job. Every re-offer of the record has its source. So as the first
    /// operation of a plan, the re-offer stages the anchor first. The same fire of a datum in
    /// flight with the random state of the job is the job's own, and it stages no anchor. The
    /// re-offers leave the stored persistent record in place.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_anchor_goes_before_an_own_reoffer_of_a_stored_persistent_record() {
        let Fixture { space, env } = fixture().await;
        staged_plan(&env, |cx| {
            cx.store(record_of(&plan_job(), 7), true)?;
            let data = cx.data_channel().clone();
            for name in ["k", "j"] {
                cx.park_on(
                    vec![data.clone()],
                    vec![any()],
                    wrapped(name),
                    false,
                    BTreeSet::new(),
                    None,
                )?;
            }
            Ok(())
        })
        .await
        .expect("the store and the parks commit");
        space.take_event_log().await;
        for (offered, events) in [
            (
                Offered::StoredPersistent,
                owned(&["consume C", "comm C", "produce D", "comm D"]),
            ),
            (Offered::InFlight, owned(&["produce D", "comm D"])),
        ] {
            let outcome = staged_plan(&env, |cx| {
                let waiter = cx.waiters()?[0].clone();
                let stored = cx.data()?[0].clone();
                let payload = record::unwrap_datum(&stored.a)
                    .expect("a datum record")
                    .datum;
                let matched = cx.bind(&any(), &payload).expect("any() binds");
                let fire = OfferFire {
                    persistent: true,
                    waiter,
                    matched,
                    offered,
                    // Added by decision 124 (plan v4 C7.9, ruling D-1): this fire carries no row
                    // search.
                    search: None,
                };
                let data = stored.a.clone();
                cx.fire_offer(fire, data, stored.a, stored.source)
            })
            .await
            .expect("the re-offer commits");
            let values: Vec<Vec<Par>> = fired(&outcome)
                .into_iter()
                .map(|(_, values)| values)
                .collect();
            assert_eq!(values, vec![vec![int(7)]]);
            assert_eq!(labels(&space.take_event_log().await), events);
        }
        assert_eq!(records(&space).await, vec![(0, vec![int(7)], true)]);
        assert!(waiters(&space).await.is_empty());
    }

    /// Decision 92: a plan of the join engine (`run_plan`) that stages nothing commits the anchor
    /// of its job only, so the transaction has one row that is unique to its job. Its outcome
    /// has no fire and no re-issue.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_empty_plan_commits_the_anchor_only() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        let config = SpaceConfig::new(SPACE.sid, "hashmap", "queue", SpaceQualifier::Default);
        let txn = env.begin(ch.lock_keys()).await.expect("begin");
        let outcome = run_plan(
            &env,
            txn,
            ch.data(),
            &config,
            &CostAuthority::default(),
            None,
            Admission {
                theory: None,
                leases: Vec::new(),
            },
            &plan_job(),
            // Added by decision 102 (plan v4 C4.3): no restage.
            false,
            |_| Ok(None),
        )
        .expect("the empty plan prepares")
        .commit()
        .await
        .expect("the anchor commits");
        assert!(outcome.fires.is_empty());
        assert!(outcome.reissue.is_none());
        assert_eq!(labels(&space.take_event_log().await), owned(&ANCHOR));
    }

    /// Decision 92: a failure at a position of the state in a plan of the join engine
    /// (`run_plan`) discards the staged operations, commits the anchor only, and then fails with
    /// its error. Each of `CellOccupied`, `CapacityExceeded` and `NestedCommitGuard` is such a
    /// failure. Every other failure drops the transaction, so it commits nothing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_plan_failure_commits_the_anchor_only_at_a_position() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        let config = SpaceConfig::new(SPACE.sid, "hashmap", "queue", SpaceQualifier::Default);
        for (error, events) in [
            (SpaceError::CellOccupied, owned(&ANCHOR)),
            (SpaceError::CapacityExceeded, owned(&ANCHOR)),
            (SpaceError::NestedCommitGuard, owned(&ANCHOR)),
            (
                SpaceError::Inconsistent("a failure of the test plan"),
                Vec::new(),
            ),
        ] {
            let txn = env.begin(ch.lock_keys()).await.expect("begin");
            let prepared = run_plan(
                &env,
                txn,
                ch.data(),
                &config,
                &CostAuthority::default(),
                None,
                Admission {
                    theory: None,
                    leases: Vec::new(),
                },
                &plan_job(),
                // Added by decision 102 (plan v4 C4.3): no restage.
                false,
                |cx| {
                    cx.store(record_of(&plan_job(), 1), false)?;
                    Err(error.clone())
                },
            );
            let result = match prepared {
                Ok(prepared) => prepared.commit().await.map(|_| ()),
                Err(error) => Err(error),
            };
            assert_eq!(result, Err(error.clone()));
            assert_eq!(labels(&space.take_event_log().await), events, "{error:?}");
            assert!(records(&space).await.is_empty(), "{error:?}");
        }
    }

    // ─── Decision 102 (plan v4 C4): the pick helpers, R-A and the record end ─────────────────

    /// Added by decision 102 (plan v4 C4.1): the picks of one play test. Three candidates keep
    /// the order of the first pick in every later pick with probability (1/6)^63, about 10^-49.
    const PICK_RUNS: usize = 64;

    /// Added by decision 102 (plan v4 C4.3): the runs of the R-A test. Each run restages with
    /// probability 1/2, so no run restages with probability 2^-64.
    const RESTAGE_RUNS: usize = 64;

    /// Added by decision 102 (plan v4 C4): the operation source of a test pick. Play reads no
    /// source, and the scripted replay answers every source alike.
    fn pick_source() -> Produce { Produce::create(&channel(), &int(0), false) }

    /// Added by decision 102 (plan v4 C4): `run_plan` of a test plan of the job `plan_job()` on
    /// the queue collection, in the transaction `txn`, with the seq claims `seq` and the flag
    /// `may_restage`. The prepared transaction commits.
    async fn run_test_plan(
        env: &SpaceEnv,
        txn: ParSpaceTxn,
        seq: Option<SeqClaims>,
        may_restage: bool,
        stage: impl FnMut(&mut Cx<'_, '_>) -> Result<Option<SpaceReissue>, SpaceError>,
    ) -> Result<SpaceOutcome, SpaceError> {
        let ch = channels();
        let config = SpaceConfig::new(SPACE.sid, "hashmap", "queue", SpaceQualifier::Default);
        let admission = Admission {
            theory: None,
            leases: Vec::new(),
        };
        run_plan(
            env,
            txn,
            ch.data(),
            &config,
            &CostAuthority::default(),
            seq,
            admission,
            &plan_job(),
            may_restage,
            stage,
        )?
        .commit()
        .await
    }

    // Moved by decision 102 (plan v4 C5): `ScriptedReplay` is in `spaces::test_support`, which
    // the tests of the disciplines share.

    /// Added by decision 102 (plan v4 C4): a scripted replay over a new transaction of `env` on
    /// the lock keys of the queue collection.
    async fn scripted(
        env: &SpaceEnv,
        comms: Option<Vec<COMM>>,
        stages: Option<usize>,
        follows: bool,
    ) -> ParSpaceTxn {
        let inner = env.begin(channels().lock_keys()).await.expect("begin");
        Box::new(ScriptedReplay {
            inner,
            comms,
            stages,
            follows,
            // Added by decision 102 (plan v4 C5.3): no per-source rows.
            rows: Vec::new(),
        })
    }

    // Moved by decision 102 (plan v4 C5): `recorded` and `named_first` are in
    // `spaces::test_support`, which the tests of the disciplines share.

    /// Added by decision 102 (plan v4 C4.1, rules CR-1 and CR-14): in play a pick shuffles the
    /// DR-75 order of its candidates, and a pick of two or more candidates sets the pick flag. A
    /// pick of one candidate sets no flag. The restage order of R-A keeps the DR-75 order and
    /// sets no flag.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_pick_helpers_shuffle_in_play_and_keep_dr75_in_the_restage() {
        let Fixture { env, .. } = fixture().await;
        let ch = channels();
        let produce_source = pick_source();
        let source = RSpaceOperationSource::Produce(&produce_source);
        produce(&env, send(1)).await;
        let txn = env.begin(ch.lock_keys()).await.expect("begin");
        run_test_plan(&env, txn, None, false, |cx| {
            assert_eq!(cx.pick_data_order(ch.data(), source)?, vec![0]);
            assert!(!cx.plan.picked, "a pick of one candidate sets no flag");
            Ok(None)
        })
        .await
        .expect("the anchor commits");
        produce(&env, send(2)).await;
        produce(&env, send(3)).await;
        let mut orders = BTreeSet::<Vec<usize>>::new();
        let txn = env.begin(ch.lock_keys()).await.expect("begin");
        run_test_plan(&env, txn, None, false, |cx| {
            let canonical = cx.canonical_data_order(ch.data())?;
            for _ in 0..PICK_RUNS {
                let order = cx.pick_data_order(ch.data(), source)?;
                let mut sorted = order.clone();
                sorted.sort_unstable();
                assert_eq!(
                    sorted,
                    vec![0, 1, 2],
                    "a pick order holds each candidate once"
                );
                orders.insert(order);
            }
            assert!(cx.plan.picked, "a pick of three candidates sets the flag");
            cx.plan.picked = false;
            cx.plan.order = PickMode::Canonical;
            assert_eq!(cx.pick_data_order(ch.data(), source)?, canonical);
            assert!(!cx.plan.picked, "the restage order sets no flag");
            Ok(None)
        })
        .await
        .expect("the anchor commits");
        assert!(orders.len() >= 2, "the play order is random: {orders:?}");
    }

    /// Added by decision 102 (plan v4 C4.1, rules CR-4 and CR-7): in replay a pick puts first
    /// the candidates that the recorded COMM names, and then the other candidates in DR-75
    /// order. The produces of the COMM name a record, the record in flight included, and its
    /// consume names a waiter. A Stored row and a missing row name nothing, so the pick keeps
    /// the DR-75 order. Only the Stored row is `recorded_stored`. Replay sets no pick flag.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_pick_helpers_put_the_named_candidates_first_in_replay() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        let produce_source = pick_source();
        let source = RSpaceOperationSource::Produce(&produce_source);
        for value in 1..=3 {
            produce(&env, send(value)).await;
        }
        let named_record = space
            .get_data(ch.data())
            .await
            .into_iter()
            .find(|datum| {
                record::unwrap_datum(&datum.a)
                    .expect("a datum record")
                    .datum
                    .pars
                    == vec![int(2)]
            })
            .expect("the record of 2")
            .source;
        let flight = record_of(&plan_job(), 7);
        let extra = ParDatum {
            source: Produce::create(ch.data(), &flight, false),
            a: flight,
            persist: false,
        };
        let recorded_consume = Consume::create(
            &vec![ch.data().clone()],
            &vec![any()],
            &named("x", None),
            false,
        );
        let txn = scripted(
            &env,
            Some(vec![recorded(recorded_consume.clone(), vec![
                named_record.clone()
            ])]),
            None,
            false,
        )
        .await;
        run_test_plan(&env, txn, None, false, |cx| {
            let canonical = cx.canonical_data_order(ch.data())?;
            let index = cx
                .data_on(ch.data())?
                .iter()
                .position(|datum| datum.source == named_record)
                .expect("the named record");
            assert_eq!(
                cx.pick_data_order(ch.data(), source)?,
                named_first(index, &canonical)
            );
            let with_extra = canonical_data_order_with(cx.data_on(ch.data())?, &extra);
            assert_eq!(
                cx.pick_data_order_with(ch.data(), &extra, source)?,
                named_first(index, &with_extra),
                "the record in flight keeps its DR-75 place"
            );
            assert!(!cx.recorded_stored(source));
            assert!(!cx.plan.picked, "replay sets no pick flag");
            Ok(None)
        })
        .await
        .expect("the anchor commits");
        let txn = scripted(
            &env,
            Some(vec![recorded(recorded_consume, vec![extra.source.clone()])]),
            None,
            false,
        )
        .await;
        run_test_plan(&env, txn, None, false, |cx| {
            let stored = cx.data_on(ch.data())?.len();
            let with_extra = canonical_data_order_with(cx.data_on(ch.data())?, &extra);
            assert_eq!(
                cx.pick_data_order_with(ch.data(), &extra, source)?,
                named_first(stored, &with_extra),
                "the named record in flight comes first"
            );
            Ok(None)
        })
        .await
        .expect("the anchor commits");
        for (comms, stored) in [(Some(Vec::new()), true), (None, false)] {
            let txn = scripted(&env, comms, None, false).await;
            run_test_plan(&env, txn, None, false, |cx| {
                let canonical = cx.canonical_data_order(ch.data())?;
                assert_eq!(cx.pick_data_order(ch.data(), source)?, canonical);
                assert_eq!(cx.recorded_stored(source), stored);
                Ok(None)
            })
            .await
            .expect("the anchor commits");
        }
        let Fixture {
            space: waiting,
            env: waiting_env,
        } = fixture().await;
        for name in ["a", "b", "c"] {
            consume(&waiting_env, receive(name, any())).await;
        }
        let named_waiter = waiting
            .get_waiting_continuations(vec![ch.data().clone()])
            .await
            .into_iter()
            .find(|waiter| {
                let parked = record::unwrap_continuation(&waiter.continuation)
                    .expect("a continuation record");
                body_name(&parked.continuation) == "b"
            })
            .expect("the waiter b")
            .source;
        let txn = scripted(
            &waiting_env,
            Some(vec![recorded(named_waiter.clone(), Vec::new())]),
            None,
            false,
        )
        .await;
        run_test_plan(&waiting_env, txn, None, false, |cx| {
            let canonical = cx.canonical_waiter_order(ch.data())?;
            let index = cx
                .waiters_on(ch.data())?
                .iter()
                .position(|waiter| waiter.source == named_waiter)
                .expect("the named waiter");
            assert_eq!(
                cx.pick_waiter_order(ch.data(), source)?,
                named_first(index, &canonical)
            );
            assert_eq!(
                cx.pick_group_order(std::slice::from_ref(ch.data()), source)?,
                named_first(index, &canonical)
            );
            Ok(None)
        })
        .await
        .expect("the anchor commits");
    }

    /// Added by decision 124 (LP-1, L5; J1 ruling 3): the join groups of `channel` in the order
    /// of `joins_on`, and the union of their waiters as (group position, waiter index) pairs, by
    /// the source hash of each consume, as `union_order` merges the groups (decision 41).
    fn union_of(
        cx: &mut Cx<'_, '_>,
        channel: &Par,
    ) -> Result<(Vec<Vec<Par>>, Vec<(usize, usize)>), SpaceError> {
        let groups: Vec<Vec<Par>> = cx.joins_on(channel)?.to_vec();
        let mut count = 0usize;
        for group in &groups {
            count += cx.group_waiters(group)?.len();
        }
        let mut hashed = Vec::<(Blake2b256Hash, usize, usize)>::with_capacity(count);
        for (position, group) in groups.iter().enumerate() {
            for (index, waiter) in cx.group_waiters(group)?.iter().enumerate() {
                hashed.push((waiter.source.hash.clone(), position, index));
            }
        }
        hashed.sort();
        let mut union = Vec::<(usize, usize)>::with_capacity(count);
        union.extend(
            hashed
                .into_iter()
                .map(|(_, position, index)| (position, index)),
        );
        Ok((groups, union))
    }

    /// Added by decision 124 (LP-1, L5; J1 ruling 3): the entry of `union` at each position of
    /// `order`.
    fn entries_of(union: &[(usize, usize)], order: &[usize]) -> Vec<(usize, usize)> {
        let mut entries = Vec::<(usize, usize)>::with_capacity(order.len());
        for position in order {
            entries.push(*union.get(*position).expect("a position in the union"));
        }
        entries
    }

    /// Added by decision 124 (LP-1, L5; J1 ruling 3): `Cx::pick_union_order` over the two join
    /// groups of the data channel of `a`: `[a]` and the join group of `a` and `b`. In play the
    /// groups keep their `joins_on` order, and each group gives a permutation of its waiters.
    /// The pick flag is set only when a group has two or more waiters, and over `PICK_RUNS`
    /// picks each order of a two-waiter group occurs. Each group of two waiters keeps one order
    /// over `PICK_RUNS` picks with probability 2^-63, so the coverage check misses with
    /// probability at most 2^-62. The restage order of R-A gives each group in DR-75 order, and
    /// the groups in their order. In replay the waiter of the second group that a recorded COMM
    /// names comes first, and the others keep the order of `union`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pick_union_order_names_the_recorded_consume_first() {
        let Fixture { env, .. } = fixture().await;
        let ch = channels();
        let produce_source = pick_source();
        let source = RSpaceOperationSource::Produce(&produce_source);
        let (group, binds) = join_group();
        let a = channels_of(&string("a")).data().clone();
        let single = vec![a.clone()];
        let mut keys = Vec::<Par>::with_capacity(ch.lock_keys().len() + group.len());
        keys.extend(ch.lock_keys().iter().cloned());
        keys.extend(group.iter().cloned());
        // One waiter on each group: no group has two waiters, so play picks nothing.
        for (channels, patterns, continuation) in [
            (single.clone(), vec![any()], wrapped("s1")),
            (group.clone(), vec![any(), any()], join_waiter("j1", &binds)),
        ] {
            staged_group_plan(&env, &channels, |cx| {
                cx.park_on(
                    channels.clone(),
                    patterns,
                    continuation,
                    false,
                    BTreeSet::new(),
                    None,
                )
            })
            .await
            .expect("the park commits");
        }
        let txn = env.begin(&keys).await.expect("begin");
        run_test_plan(&env, txn, None, false, |cx| {
            let (groups, union) = union_of(cx, &a)?;
            assert_eq!(groups.len(), 2, "a has two join groups");
            let order = cx.pick_union_order(&groups, &union, source)?;
            let entries = entries_of(&union, &order);
            assert_eq!(entries, vec![(0, 0), (1, 0)], "the groups keep their order");
            assert!(
                !cx.plan.picked,
                "no group has two waiters, so play picks nothing"
            );
            Ok(None)
        })
        .await
        .expect("the anchor commits");
        // A second waiter on each group: each group has two waiters.
        for (channels, patterns, continuation) in [
            (single.clone(), vec![any()], wrapped("s2")),
            (group.clone(), vec![any(), any()], join_waiter("j2", &binds)),
        ] {
            staged_group_plan(&env, &channels, |cx| {
                cx.park_on(
                    channels.clone(),
                    patterns,
                    continuation,
                    false,
                    BTreeSet::new(),
                    None,
                )
            })
            .await
            .expect("the park commits");
        }
        let mut orders = [BTreeSet::<Vec<usize>>::new(), BTreeSet::<Vec<usize>>::new()];
        let txn = env.begin(&keys).await.expect("begin");
        run_test_plan(&env, txn, None, false, |cx| {
            let (groups, union) = union_of(cx, &a)?;
            let mut all = Vec::<usize>::with_capacity(union.len());
            all.extend(0..union.len());
            for _ in 0..PICK_RUNS {
                let order = cx.pick_union_order(&groups, &union, source)?;
                let mut sorted = order.clone();
                sorted.sort_unstable();
                assert_eq!(sorted, all, "a pick holds each waiter once");
                let entries = entries_of(&union, &order);
                let mut positions = Vec::<usize>::with_capacity(entries.len());
                positions.extend(entries.iter().map(|&(position, _)| position));
                assert_eq!(positions, vec![0, 0, 1, 1], "the groups keep their order");
                for (position, seen) in orders.iter_mut().enumerate() {
                    seen.insert(
                        entries
                            .iter()
                            .filter(|&&(group, _)| group == position)
                            .map(|&(_, index)| index)
                            .collect(),
                    );
                }
            }
            assert!(cx.plan.picked, "a group of two waiters sets the flag");
            cx.plan.picked = false;
            cx.plan.order = PickMode::Canonical;
            let mut restage = Vec::<usize>::with_capacity(union.len());
            for (position, group) in groups.iter().enumerate() {
                for index in cx.canonical_group_order(group)? {
                    restage.push(
                        union
                            .iter()
                            .position(|&entry| entry == (position, index))
                            .expect("a waiter of the union"),
                    );
                }
            }
            assert_eq!(cx.pick_union_order(&groups, &union, source)?, restage);
            assert!(!cx.plan.picked, "the restage order sets no flag");
            Ok(None)
        })
        .await
        .expect("the anchor commits");
        for (position, seen) in orders.iter().enumerate() {
            assert_eq!(
                seen.len(),
                2,
                "group {position} plays each order of its two waiters: {seen:?}"
            );
        }
        // Replay: a recorded COMM names the last waiter of the second group.
        let mut named_consume: Option<Consume> = None;
        let txn = env.begin(&keys).await.expect("begin");
        run_test_plan(&env, txn, None, false, |cx| {
            let (groups, _) = union_of(cx, &a)?;
            let second = groups.get(1).expect("a second group");
            named_consume = cx
                .group_waiters(second)?
                .last()
                .map(|waiter| waiter.source.clone());
            Ok(None)
        })
        .await
        .expect("the anchor commits");
        let named_consume = named_consume.expect("a waiter of the second group");
        let inner = env.begin(&keys).await.expect("begin");
        let txn: ParSpaceTxn = Box::new(ScriptedReplay {
            inner,
            comms: Some(vec![recorded(named_consume.clone(), Vec::new())]),
            stages: None,
            follows: false,
            rows: Vec::new(),
        });
        run_test_plan(&env, txn, None, false, |cx| {
            let (groups, union) = union_of(cx, &a)?;
            let second = groups.get(1).expect("a second group");
            let index = cx
                .group_waiters(second)?
                .iter()
                .position(|waiter| waiter.source == named_consume)
                .expect("the named waiter");
            let named = union
                .iter()
                .position(|&entry| entry == (1, index))
                .expect("the named waiter in the union");
            let mut base = Vec::<usize>::with_capacity(union.len());
            base.extend(0..union.len());
            assert_eq!(
                cx.pick_union_order(&groups, &union, source)?,
                named_first(named, &base)
            );
            assert!(!cx.plan.picked, "replay sets no pick flag");
            Ok(None)
        })
        .await
        .expect("the anchor commits");
    }

    /// Added by decision 102 (plan v4 C4.3, rule CR-14): R-A. The first staging fails with a
    /// positional error when its pick of two records does not put the DR-75 first record
    /// first. R-A discards that staging and stages again in DR-75 order, which succeeds, so the
    /// transaction commits the DR-75 staging once. No run restages with probability 2^-64.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_r_a_commits_the_dr75_staging_after_a_failed_pick() {
        let ch = channels();
        let produce_source = pick_source();
        let source = RSpaceOperationSource::Produce(&produce_source);
        let mut restaged = 0usize;
        for _ in 0..RESTAGE_RUNS {
            let Fixture { space, env } = fixture().await;
            produce(&env, send(1)).await;
            produce(&env, send(2)).await;
            space.take_event_log().await;
            let mut calls = 0usize;
            let txn = env.begin(ch.lock_keys()).await.expect("begin");
            run_test_plan(&env, txn, None, true, |cx| {
                calls += 1;
                let canonical = cx.canonical_data_order(ch.data())?;
                let order = cx.pick_data_order(ch.data(), source)?;
                match order.first() == canonical.first() {
                    true => {
                        cx.store(record_of(&plan_job(), 3), false)?;
                        Ok(None)
                    }
                    false => Err(SpaceError::CapacityExceeded),
                }
            })
            .await
            .expect("the DR-75 staging commits");
            assert!(calls <= 2, "R-A restages once");
            restaged += usize::from(calls == 2);
            assert_eq!(labels(&space.take_event_log().await), owned(&["produce D"]));
            assert_eq!(records(&space).await.len(), 3, "the record is stored once");
        }
        assert!(restaged >= 1, "R-A restaged in no run");
    }

    /// Added by decision 102 (plan v4 C4.3, rule CR-14): a restage that fails again ends in
    /// `prepare_failure` with the second error. The transaction commits the anchor only, and
    /// neither staging stores its record.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_r_a_second_failure_commits_the_anchor_with_the_second_error() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        let produce_source = pick_source();
        let source = RSpaceOperationSource::Produce(&produce_source);
        produce(&env, send(1)).await;
        produce(&env, send(2)).await;
        space.take_event_log().await;
        let mut calls = 0usize;
        let txn = env.begin(ch.lock_keys()).await.expect("begin");
        let result = run_test_plan(&env, txn, None, true, |cx| {
            calls += 1;
            cx.pick_data_order(ch.data(), source)?;
            cx.store(record_of(&plan_job(), 3), false)?;
            match calls {
                1 => Err(SpaceError::CapacityExceeded),
                _ => Err(SpaceError::CellOccupied),
            }
        })
        .await;
        assert_eq!(result.map(|_| ()), Err(SpaceError::CellOccupied));
        assert_eq!(calls, 2, "R-A restages once");
        assert_eq!(labels(&space.take_event_log().await), owned(&ANCHOR));
        assert_eq!(records(&space).await.len(), 2);
    }

    /// Added by decision 102 (plan v4 C4.3, rule CR-14 step 7, rule CR-19 case 4): a staging
    /// that began with the anchor, because its first operation was not the job's own, begins
    /// with the anchor again when R-A restages it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_r_a_restages_the_anchor_first() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        let produce_source = pick_source();
        let source = RSpaceOperationSource::Produce(&produce_source);
        produce(&env, send(1)).await;
        produce(&env, send(2)).await;
        space.take_event_log().await;
        let foreign = record::wrap_datum(0, &send(5).data);
        let mut calls = 0usize;
        let txn = env.begin(ch.lock_keys()).await.expect("begin");
        run_test_plan(&env, txn, None, true, |cx| {
            calls += 1;
            cx.pick_data_order(ch.data(), source)?;
            cx.store(foreign.clone(), false)?;
            match calls {
                1 => Err(SpaceError::CapacityExceeded),
                _ => Ok(None),
            }
        })
        .await
        .expect("the restage commits");
        assert_eq!(calls, 2, "R-A restages once");
        assert_eq!(
            labels(&space.take_event_log().await),
            owned(&["consume C", "comm C", "produce D"])
        );
    }

    /// Added by decision 102 (plan v4 C4.3, rule CR-14): R-A restages only in play, when the
    /// operation may restage, after a pick that shuffled two or more candidates, with a
    /// positional error or `SeqConcurrent`. Each case misses one condition, so its staging runs
    /// once and ends in `prepare_failure` as before: the flag is clear, the pick has no
    /// candidate, the error is not positional, or the transaction replays.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_r_a_needs_play_the_flag_a_pick_and_a_positional_error() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        let produce_source = pick_source();
        let source = RSpaceOperationSource::Produce(&produce_source);
        produce(&env, send(1)).await;
        produce(&env, send(2)).await;
        space.take_event_log().await;
        for (replay, may_restage, shuffles, error, events) in [
            (
                false,
                false,
                true,
                SpaceError::CapacityExceeded,
                owned(&ANCHOR),
            ),
            (
                false,
                true,
                false,
                SpaceError::CapacityExceeded,
                owned(&ANCHOR),
            ),
            (
                false,
                true,
                true,
                SpaceError::Inconsistent("a failure of the test plan"),
                Vec::new(),
            ),
            (
                true,
                true,
                true,
                SpaceError::CapacityExceeded,
                owned(&ANCHOR),
            ),
        ] {
            let mut calls = 0usize;
            let txn = match replay {
                true => scripted(&env, None, None, false).await,
                false => env.begin(ch.lock_keys()).await.expect("begin"),
            };
            let result = run_test_plan(&env, txn, None, may_restage, |cx| {
                calls += 1;
                match shuffles {
                    true => cx.pick_data_order(ch.data(), source)?,
                    false => cx.pick_waiter_order(ch.data(), source)?,
                };
                cx.store(record_of(&plan_job(), 3), false)?;
                Err(error.clone())
            })
            .await;
            assert_eq!(result.map(|_| ()), Err(error.clone()), "{error:?}");
            assert_eq!(calls, 1, "{replay} {may_restage} {shuffles} {error:?}");
            assert_eq!(labels(&space.take_event_log().await), events, "{error:?}");
        }
        assert_eq!(records(&space).await.len(), 2);
    }

    /// Added by decision 102 (plan v4 C4.3, rule CR-14, U5): `SeqConcurrent` after a pick of
    /// two candidates restages too. R-A rewinds the seq claims to the claims after the own
    /// collection, so the claim of the failed staging is never stored, and the own claim and
    /// the claim of the committed restage are.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_r_a_restages_after_seq_concurrent_and_rewinds_the_claims() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        let produce_source = pick_source();
        let source = RSpaceOperationSource::Produce(&produce_source);
        produce(&env, send(1)).await;
        produce(&env, send(2)).await;
        space.take_event_log().await;
        let seq = NativeSpace {
            sid: [8; SID_LEN],
            durability: Durability::Ephemeral,
        };
        let claimed = |byte: u8| {
            let key = CollectionKey::from_bytes(vec![byte]).expect("a key");
            namespace::gprivate(namespace::data_channel(seq, &key))
        };
        let (own, picked, restaged) = (claimed(1), claimed(2), claimed(3));
        let mut claims = SeqClaims::open(&env.volatile, seq, SpaceQualifier::Seq, false, b"item")
            .expect("a seq job opens its claims")
            .expect("a program job on a seq space claims");
        claims.claim(&own, ClaimRole::Data).expect("the own claim");
        let mut calls = 0usize;
        let txn = env.begin(ch.lock_keys()).await.expect("begin");
        run_test_plan(&env, txn, Some(claims), true, |cx| {
            calls += 1;
            cx.pick_data_order(ch.data(), source)?;
            match calls {
                1 => {
                    cx.claim(&picked, ClaimRole::Data)?;
                    Err(SpaceError::SeqConcurrent { space: seq.sid })
                }
                _ => {
                    cx.claim(&restaged, ClaimRole::Data)?;
                    // Added by decision 124 (plan v4 C7.19, rule CR-24, ruling M-4): the restage
                    // stages one row, which takes the claim.
                    cx.store(record_of(&plan_job(), 3), false)?;
                    Ok(None)
                }
            }
        })
        .await
        .expect("the restage commits");
        assert_eq!(calls, 2, "R-A restages once");
        let mut later = SeqClaims::open(&env.volatile, seq, SpaceQualifier::Seq, false, b"later")
            .expect("a seq job opens its claims")
            .expect("a program job on a seq space claims");
        for (channel, stored) in [(&own, true), (&picked, false), (&restaged, true)] {
            let expected = match stored {
                true => Err(SpaceError::SeqConcurrent { space: seq.sid }),
                false => Ok(()),
            };
            assert_eq!(later.claim(channel, ClaimRole::Data), expected, "{stored}");
        }
    }

    /// Added by decision 102 (U5, F18): `SeqConcurrent` after a pick
    /// restages in DR-75 order. The first staging fails when its pick does not put the DR-75
    /// first record first. That chance is 1/2 in each run, so no run restages with a chance of
    /// 2^-64.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_seq_concurrent_after_a_pick_restages_in_dr75_order() {
        let ch = channels();
        let produce_source = pick_source();
        let source = RSpaceOperationSource::Produce(&produce_source);
        let seq = NativeSpace {
            sid: [8; SID_LEN],
            durability: Durability::Ephemeral,
        };
        let own = namespace::gprivate(namespace::data_channel(
            seq,
            &CollectionKey::from_bytes(vec![1]).expect("a key"),
        ));
        let mut restaged = 0usize;
        for _ in 0..RESTAGE_RUNS {
            let Fixture { space, env } = fixture().await;
            produce(&env, send(1)).await;
            produce(&env, send(2)).await;
            space.take_event_log().await;
            let mut claims =
                SeqClaims::open(&env.volatile, seq, SpaceQualifier::Seq, false, b"item")
                    .expect("a seq job opens its claims")
                    .expect("a program job on a seq space claims");
            claims.claim(&own, ClaimRole::Data).expect("the own claim");
            let mut calls = 0usize;
            let mut restage = None::<(Vec<usize>, Vec<usize>)>;
            let txn = env.begin(ch.lock_keys()).await.expect("begin");
            run_test_plan(&env, txn, Some(claims), true, |cx| {
                calls += 1;
                let canonical = cx.canonical_data_order(ch.data())?;
                let order = cx.pick_data_order(ch.data(), source)?;
                match (calls, order.first() == canonical.first()) {
                    (1, false) => Err(SpaceError::SeqConcurrent { space: seq.sid }),
                    (1, true) => {
                        cx.store(record_of(&plan_job(), 3), false)?;
                        Ok(None)
                    }
                    (_, _) => {
                        restage = Some((order, canonical));
                        cx.store(record_of(&plan_job(), 3), false)?;
                        Ok(None)
                    }
                }
            })
            .await
            .expect("the DR-75 staging commits");
            match (calls, restage) {
                (1, None) => {}
                (2, Some((order, canonical))) => {
                    assert_eq!(order, canonical, "the restage reads the DR-75 order");
                    restaged += 1;
                }
                (calls, restage) => panic!("R-A restages once: {calls} calls, {restage:?}"),
            }
            assert_eq!(records(&space).await.len(), 3, "the record is stored once");
        }
        assert!(restaged >= 1, "R-A restaged in no run");
    }

    /// Added by decision 102 (plan v4 C4.4, rules CR-6, CR-32 and CR-5): in native replay a
    /// staging that the record ends keeps its staged prefix, and the re-issue is `None`. The
    /// prefix commits when a stage call answers `RecordEnded`, and when a staging error comes
    /// after the cursor followed a row (`end_record` is true). Before any followed row
    /// (`end_record` is false), a positional error keeps `prepare_failure` (the error rule).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_record_end_commits_the_followed_prefix() {
        for (stages, follows, error, committed) in [
            (Some(1), false, None, true),
            (None, true, Some(SpaceError::CapacityExceeded), true),
            (None, false, Some(SpaceError::CapacityExceeded), false),
        ] {
            let Fixture { space, env } = fixture().await;
            let txn = scripted(&env, None, stages, follows).await;
            let result = run_test_plan(&env, txn, None, false, |cx| {
                cx.store(record_of(&plan_job(), 1), false)?;
                match &error {
                    None => {
                        cx.store(record_of(&plan_job(), 2), false)?;
                        Err(SpaceError::Inconsistent(
                            "the record ends before this store",
                        ))
                    }
                    Some(error) => Err(error.clone()),
                }
            })
            .await;
            let case = format!("{stages:?} {follows} {error:?}");
            match committed {
                true => {
                    let outcome = result.expect("the followed prefix commits");
                    assert!(outcome.reissue.is_none(), "{case}");
                    assert_eq!(
                        labels(&space.take_event_log().await),
                        owned(&["produce D"]),
                        "{case}"
                    );
                    assert_eq!(
                        records(&space).await,
                        vec![(0, vec![int(1)], false)],
                        "{case}"
                    );
                }
                false => {
                    assert_eq!(
                        result.map(|_| ()),
                        error.clone().map_or(Ok(()), Err),
                        "{case}"
                    );
                    assert_eq!(
                        labels(&space.take_event_log().await),
                        owned(&ANCHOR),
                        "{case}"
                    );
                    assert!(records(&space).await.is_empty(), "{case}");
                }
            }
        }
    }

    /// Added by decision 102 (plan v4 C4.4): `run_with` ends its staging at the record end as
    /// `run_plan` does. The bump of a send follows its rows, the store answers `RecordEnded`,
    /// and the transaction commits the bump alone, with no fire and no re-issue.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_collection_record_end_commits_the_followed_prefix() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        space.take_event_log().await;
        let mut opened =
            begin_collection(&HashMapOuter, &env, SPACE, &key_of(&channel()), &channel())
                .await
                .expect("phase A");
        opened.txn = Box::new(ScriptedReplay {
            inner: opened.txn,
            comms: None,
            stages: Some(2),
            follows: false,
            // Added by decision 102 (plan v4 C5.3): no per-source rows.
            rows: Vec::new(),
        });
        let outcome =
            prepare_collection_op(&Queue::default(), &env, opened, SpaceOp::Produce(send(2)))
                .await
                .expect("the operation prepares")
                .commit()
                .await
                .expect("the bump commits");
        assert!(outcome.fires.is_empty());
        assert!(outcome.reissue.is_none());
        assert_eq!(labels(&space.take_event_log().await), owned(&BUMP));
        assert_eq!(records(&space).await, vec![(0, vec![int(1)], false)]);
    }

    /// Added by plan v4 errata (V4-19, rule CR-6): UT-1. A record end inside the STEP bump of a
    /// produce or a consume keeps the followed take in the staged record, so `anchor_if_empty`
    /// stages nothing. The transaction commits the take alone, with no fire and no re-issue.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_collection_record_end_inside_the_step_bump_commits_the_take() {
        let ch = channels();
        for (case, op) in [
            ("produce", SpaceOp::Produce(send(2))),
            ("consume", SpaceOp::Consume(receive("k", any()))),
        ] {
            let Fixture { space, env } = fixture().await;
            produce(&env, send(1)).await;
            space.take_event_log().await;
            let mut opened =
                begin_collection(&HashMapOuter, &env, SPACE, &key_of(&channel()), &channel())
                    .await
                    .expect("phase A");
            opened.txn = Box::new(ScriptedReplay {
                inner: opened.txn,
                comms: None,
                stages: Some(1),
                follows: false,
                // Added by decision 102 (plan v4 C5.3): no per-source rows.
                rows: Vec::new(),
            });
            let outcome = prepare_collection_op(&Queue::default(), &env, opened, op)
                .await
                .expect("the operation prepares")
                .commit()
                .await
                .expect("the take commits");
            assert!(outcome.fires.is_empty(), "{case}");
            assert!(outcome.reissue.is_none(), "{case}");
            assert_eq!(
                labels(&space.take_event_log().await),
                owned(&["consume K", "comm K"]),
                "{case}"
            );
            assert!(counter(&space, ch.counter()).await.is_empty(), "{case}");
            assert_eq!(counter(&space, ch.space_counter()).await, vec![1], "{case}");
            assert_eq!(
                records(&space).await,
                vec![(0, vec![int(1)], false)],
                "{case}"
            );
            assert!(waiters(&space).await.is_empty(), "{case}");
        }
    }

    /// Added by plan v4 errata (V4-19, rule CR-6): UT-2. A record end inside the OPEN bump of the
    /// first operation on a collection keeps the followed prefix of the bump in the staged
    /// record: the take of the space counter, or the take and its put. The transaction commits
    /// that prefix, with no fire and no re-issue.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_collection_record_end_inside_the_open_bump_commits_its_prefix() {
        let ch = channels();
        for (stages, events, space_values) in [
            (1, &["consume S", "comm S"][..], Vec::<i64>::new()),
            (2, &["consume S", "comm S", "produce S"][..], vec![1]),
        ] {
            let Fixture { space, env } = fixture().await;
            let mut opened =
                begin_collection(&HashMapOuter, &env, SPACE, &key_of(&channel()), &channel())
                    .await
                    .expect("phase A");
            opened.txn = Box::new(ScriptedReplay {
                inner: opened.txn,
                comms: None,
                stages: Some(stages),
                follows: false,
                // Added by decision 102 (plan v4 C5.3): no per-source rows.
                rows: Vec::new(),
            });
            let outcome =
                prepare_collection_op(&Queue::default(), &env, opened, SpaceOp::Produce(send(1)))
                    .await
                    .expect("the operation prepares")
                    .commit()
                    .await
                    .expect("the prefix commits");
            assert!(outcome.fires.is_empty(), "{stages}");
            assert!(outcome.reissue.is_none(), "{stages}");
            assert_eq!(
                labels(&space.take_event_log().await),
                owned(events),
                "{stages}"
            );
            assert_eq!(
                counter(&space, ch.space_counter()).await,
                space_values,
                "{stages}"
            );
            assert!(counter(&space, ch.counter()).await.is_empty(), "{stages}");
            assert!(records(&space).await.is_empty(), "{stages}");
        }
    }

    /// Added by plan v4 errata (V4-19, rule CR-6): UT-3. A record end inside `Cx::bump_on`, the
    /// bump of a join touch and of the PathMap token, keeps the followed prefix of the bump in
    /// the staged record, so `anchor_if_empty` stages nothing, and the plan commits the prefix:
    /// the take of a STEP bump, or the take, or the take and its put, of an OPEN bump. The sends
    /// `sends` run before the plan.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_record_end_inside_bump_on_commits_its_prefix() {
        let ch = channels();
        let random = Blake2b512Random::create_from_bytes(b"touch");
        for (sends, events, space_values, stages) in [
            // STEP: a send opened the collection.
            (&[1][..], &["consume K", "comm K"][..], vec![1], 1),
            // OPEN: the first bump of the collection.
            (&[][..], &["consume S", "comm S"][..], Vec::<i64>::new(), 1),
            (
                &[][..],
                &["consume S", "comm S", "produce S"][..],
                vec![1],
                2,
            ),
        ] {
            let Fixture { space, env } = fixture().await;
            for value in sends {
                produce(&env, send(*value)).await;
            }
            space.take_event_log().await;
            let txn = scripted(&env, None, Some(stages), false).await;
            let outcome = run_test_plan(&env, txn, None, false, |cx| {
                let introduction = Arc::clone(cx.introduction());
                let authority = OperationAuthority {
                    continuation: None,
                    introduction: &introduction,
                };
                cx.bump_on(ch.counters(), &random, authority)?;
                Ok(None)
            })
            .await
            .expect("the followed prefix commits");
            let case = format!("{sends:?} {stages}");
            assert!(outcome.fires.is_empty(), "{case}");
            assert!(outcome.reissue.is_none(), "{case}");
            assert_eq!(
                labels(&space.take_event_log().await),
                owned(events),
                "{case}"
            );
            assert_eq!(
                counter(&space, ch.space_counter()).await,
                space_values,
                "{case}"
            );
            assert!(counter(&space, ch.counter()).await.is_empty(), "{case}");
        }
    }

    /// Added by plan v4 errata (V4-19, rule CR-6): UT-4, the latent case. A record end inside a
    /// bump that stages after other operations keeps the followed take of the bump in the staged
    /// record. So the receipt of the prefix has the length of the staged record, and `outcome`
    /// accepts it. The bump stages after a store, or after the bump of another touch.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_engine_record_end_inside_a_later_bump_lists_its_take() {
        let ch = channels();
        let random = Blake2b512Random::create_from_bytes(b"touch");
        for (store_first, stages, events, stored, space_values) in [
            (
                true,
                2,
                &["produce D", "consume S", "comm S"][..],
                vec![(0, vec![int(1)], false)],
                Vec::<i64>::new(),
            ),
            (
                false,
                4,
                &[
                    "consume S",
                    "comm S",
                    "produce S",
                    "produce K",
                    "consume K",
                    "comm K",
                ][..],
                Vec::new(),
                vec![1],
            ),
        ] {
            let Fixture { space, env } = fixture().await;
            let txn = scripted(&env, None, Some(stages), false).await;
            let outcome = run_test_plan(&env, txn, None, false, |cx| {
                let introduction = Arc::clone(cx.introduction());
                let authority = OperationAuthority {
                    continuation: None,
                    introduction: &introduction,
                };
                match store_first {
                    true => {
                        cx.store(record_of(&plan_job(), 1), false)?;
                    }
                    false => {
                        cx.bump_on(ch.counters(), &random, authority)?;
                    }
                }
                cx.bump_on(ch.counters(), &random.split_byte(1), authority)?;
                Ok(None)
            })
            .await
            .expect("the followed prefix commits");
            let case = format!("{store_first} {stages}");
            assert!(outcome.fires.is_empty(), "{case}");
            assert!(outcome.reissue.is_none(), "{case}");
            assert_eq!(
                labels(&space.take_event_log().await),
                owned(events),
                "{case}"
            );
            assert_eq!(records(&space).await, stored, "{case}");
            assert_eq!(
                counter(&space, ch.space_counter()).await,
                space_values,
                "{case}"
            );
            assert!(counter(&space, ch.counter()).await.is_empty(), "{case}");
        }
    }

    /// Decision 94: a collection operation holds phase A over its own keys, `C(sid)` included,
    /// from its classification to its commit. So another job of the space waits at phase A, and
    /// the join scope that the first job reads stays current until its commit
    /// (`JoinScopeLock.tla`). The waiting job runs in a task of its own, because a second
    /// transaction in one task fails with `NestedTransaction`. It runs after the commit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_collection_phase_a_holds_the_other_jobs_of_the_space() {
        let Fixture { space, env } = fixture().await;
        let opened = begin_collection(&HashMapOuter, &env, SPACE, &key_of(&channel()), &channel())
            .await
            .expect("phase A");
        let other = string("other");
        let waiting_env = env.clone();
        let waiting_op = SpaceOp::Produce(send_on(&other, 7, false));
        let mut waiting = tokio::spawn(async move { run(&waiting_env, waiting_op).await });
        tokio::task::yield_now().await;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut waiting)
                .await
                .is_err(),
            "the other job waits at phase A"
        );
        prepare_collection_op(&Queue::default(), &env, opened, SpaceOp::Produce(send(1)))
            .await
            .expect("the operation prepares")
            .commit()
            .await
            .expect("the operation commits");
        waiting
            .await
            .expect("join the other job")
            .expect("the other job runs after the commit");
        assert_eq!(records(&space).await, vec![(0, vec![int(1)], false)]);
        assert_eq!(records_on(&space, &other).await, vec![(
            0,
            vec![int(7)],
            false
        )]);
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the finish keeps the
    /// plan's own return without an end, takes the value of Return, and re-issues the rest of a
    /// Stop for an owner refusal with the stop of `finish_pairs`.
    #[test]
    fn finish_reissue_maps_the_three_ends() {
        let own = Some(SpaceReissue {
            op: SpaceOp::Produce(send(1)),
            cause: ReissueCause::RoundCap,
        });
        assert_eq!(finish_reissue(None, own.clone()), (own.clone(), false));
        let value = Some(SpaceReissue {
            op: SpaceOp::Produce(send(2)),
            cause: ReissueCause::DivergentPair,
        });
        let returned = finish_reissue(Some(TailEnd::Return(value.clone())), own.clone());
        assert_eq!(returned, (value, false));
        let ended = finish_reissue(Some(TailEnd::Return(None)), own.clone());
        assert_eq!(ended, (None, false));
        let rest = SpaceOp::Produce(send(3));
        let stopped = finish_reissue(Some(TailEnd::Stop(rest.clone())), own);
        let reissue = SpaceReissue {
            op: rest,
            cause: ReissueCause::OwnerRefusal,
        };
        assert_eq!(stopped, (Some(reissue), true));
    }

    /// Added by decision 124 (plan of the batch B tests, B7, errata V4-13 and V4-18, rule CR-31,
    /// ruling RADD2 1.1): the finish books the item pairs of a deviation whose commit may veto
    /// only when Tail(actual) is Stop, and the item pairs of any other deviation on both branches.
    /// The searched end of row d decides the branch (`tail_end`, `finish_reissue`), and the
    /// refused pairs of the rows are booked in every case (`finish_pairs`).
    #[test]
    fn finish_writes_vetoing_item_pairs_only_on_stop() {
        use rspace_plus_plus::rspace::space_store_handle::{RowDeviation, RowReissue};
        let refused = (Blake2b256Hash::new(b"r1"), Blake2b256Hash::new(b"w1"));
        let item = (Blake2b256Hash::new(b"r2"), Blake2b256Hash::new(b"w2"));
        let stop = TailEnd::Stop(SpaceOp::Produce(send(1)));
        let cases = [
            (false, TailEnd::Return(None), true),
            (false, stop.clone(), true),
            (true, TailEnd::Return(None), false),
            (true, stop, true),
        ];
        for (vetoing, end, booked) in cases {
            let mut receipt = ParSpaceReceipt {
                ops: vec![SpaceOpReceipt::Stored {
                    produce: rspace_plus_plus::rspace::trace::event::Produce::create(
                        &int(1),
                        &int(2),
                        false,
                    ),
                }],
                deviation: Some(RowDeviation {
                    index: 0,
                    item_failed: vec![item.clone()],
                    dispatched: None,
                    vetoing,
                    tail_read: TailRead::Ends,
                    claims: None,
                    reissue: Some(RowReissue::new(end.clone())),
                }),
                refused: vec![refused.clone()],
            };
            let tail =
                tail_end(&receipt, &[], &[], None, None, false).expect("the searched end of row d");
            assert_eq!(
                tail.as_ref(),
                Some(&end),
                "vetoing {vetoing}: the searched end"
            );
            let (_, stop) = finish_reissue(tail, None);
            let pairs: Vec<BookPair> = finish_pairs(&mut receipt, Vec::new(), stop).collect();
            let expected = match booked {
                false => vec![refused.clone()],
                true => vec![refused.clone(), item.clone()],
            };
            assert_eq!(pairs, expected, "vetoing {vetoing}, end {end:?}");
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.1): the lift of a cascade
    /// after the plan takes the class of the end of its element, and `cascade_lift` fails closed
    /// when no row is staged, for an element that ended at the cap or for an owner refusal, and
    /// on a last row that is not a lift.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cascade_lift_classes_the_last_lift() {
        for (own, class) in [
            (None, TailRow::CascadeLift),
            (
                Some(ReissueCause::DivergentPair),
                TailRow::DivergentCascadeLift,
            ),
        ] {
            let Fixture { env, .. } = fixture().await;
            produce(&env, send(1)).await;
            let mut seen = None;
            staged_plan(&env, |cx| {
                let empty = cx.cascade_lift(own).is_err();
                let data = cx.data_channel();
                let candidates = candidates_by_ord(cx)?;
                cx.lift::<QueueDiscipline>(
                    vec![data.clone()],
                    vec![any()],
                    wrapped("lift"),
                    candidates,
                )?;
                let capped = cx.cascade_lift(Some(ReissueCause::RoundCap)).is_err();
                let refused = cx.cascade_lift(Some(ReissueCause::OwnerRefusal)).is_err();
                let kept = cx.plan.tail_rows.last().copied();
                cx.cascade_lift(own)?;
                let marked = cx.plan.tail_rows.last().copied();
                let again = cx.cascade_lift(own).is_err();
                seen = Some((empty, capped, refused, kept, marked, again));
                Ok(())
            })
            .await
            .expect("the lift commits");
            let seen = seen.expect("the plan ran");
            assert_eq!(
                seen,
                (true, true, true, Some(TailRow::Lift), Some(class), true),
                "the class of the lift with the end {own:?}"
            );
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.1): a refused lift of a
    /// cascade after the plan (B.6 row 8) stops with the settle job of its cascade, as the plan
    /// stops at that lift: the fields of `settle_operation`, no seed, no owed send, and the
    /// operation again without its order when the element met a divergent pair. The value does
    /// not depend on `settles`. Without the operation the end fails closed.
    #[test]
    fn tail_end_gives_a_cascade_lift_the_settle_of_its_cascade() {
        use rspace_plus_plus::rspace::space_store_handle::RowDeviation;

        use crate::rust::interpreter::spaces::agent::CascadeState;
        let consume_source = Consume::create(&vec![int(1)], &vec![any()], &named("k", None), false);
        let receipt = ParSpaceReceipt {
            ops: vec![SpaceOpReceipt::Parked {
                consume: consume_source,
            }],
            deviation: Some(RowDeviation {
                index: 0,
                item_failed: Vec::new(),
                dispatched: None,
                vetoing: false,
                tail_read: TailRead::Ends,
                claims: None,
                reissue: None,
            }),
            refused: Vec::new(),
        };
        let staged = [Staged::Lift];
        let produce = ProduceOp {
            reissued_ord: Some(4),
            ..send_forever(1)
        };
        let consume = ConsumeOp {
            reissued_ord: Some(4),
            ..receive("w", any())
        };
        let cases = [
            (
                SpaceOp::Produce(produce.clone()),
                settle_operation(OpView::Produce(&produce)),
                SpaceOp::Produce(ProduceOp {
                    reissued_ord: None,
                    ..produce.clone()
                }),
            ),
            (
                SpaceOp::Consume(consume.clone()),
                settle_operation(OpView::Consume(&consume)),
                SpaceOp::Consume(ConsumeOp {
                    reissued_ord: None,
                    ..consume.clone()
                }),
            ),
        ];
        for (operation, settle, unordered) in cases {
            let ends = [
                (TailRow::CascadeLift, None),
                (
                    TailRow::DivergentCascadeLift,
                    Some(Box::new(SpaceReissue {
                        op: unordered,
                        cause: ReissueCause::DivergentPair,
                    })),
                ),
            ];
            for (class, resume) in ends {
                let expected = SpaceOp::Settle(SettleOp {
                    cascade: Some(Box::new(CascadeState {
                        seeds: Vec::new(),
                        pending: Vec::new(),
                        resume,
                    })),
                    ..settle.clone()
                });
                for settles in [false, true] {
                    let end =
                        tail_end(&receipt, &staged, &[class], Some(&operation), None, settles)
                            .expect("a refused cascade lift ends");
                    assert_eq!(end, Some(TailEnd::Stop(expected.clone())), "{class:?}");
                }
                assert!(
                    tail_end(&receipt, &staged, &[class], None, None, false).is_err(),
                    "{class:?} without the operation"
                );
            }
        }
    }

    /// Added by decision 124 (plan v4 C7.9 and C7.15, ruling OP-LISTS): commits one fire of the
    /// stored record with the least `ord` of the collection on `channel()` under the observer
    /// `Funded`, and runs `probe` over the lists of that commit. It returns the answer of `probe`.
    /// A record must be stored.
    async fn commit_lists<T, F>(space: &ParRSpace, env: &SpaceEnv, probe: F) -> T
    where
        T: Send + 'static,
        F: FnOnce(&ParRowLists<'_>) -> T + Send + 'static,
    {
        space.set_accounting_observer(Some(Arc::new(Funded::admitting())));
        let (sender, receiver) = std::sync::mpsc::channel();
        staged_plan(env, |cx| {
            let data = cx.data_channel();
            let first: Vec<ParCandidate> = candidates_by_ord(cx)?.into_iter().take(1).collect();
            let continuation = wrapped("carrier");
            let (group, patterns) = (vec![data.clone()], vec![any()]);
            let source = Consume::create(&group, &patterns, &continuation, false);
            let staged = first.first().cloned().expect("a stored record");
            let inner = StagedRowSearch::<QueueDiscipline>::of_receive(
                cx,
                &source,
                &continuation,
                false,
                data,
                staged,
                0,
            );
            let search = ListsProbe::new(inner, move |lists| {
                let _ = sender.send(probe(lists));
            });
            cx.fire_receive_group(
                group,
                patterns,
                continuation,
                false,
                BTreeSet::new(),
                first,
                1,
                Some(Box::new(search)),
            )
        })
        .await
        .expect("the carrier fire commits");
        receiver.try_recv().expect("the commit ran the probe")
    }

    /// Added by decision 124 (plan v4 C7.9, ruling OP-LISTS): a waiter of the group of the
    /// collection on `channel()`, with the order `ord`, that binds any value.
    fn waiter_of(name: &str, ord: i64) -> ParWaiter {
        let continuation =
            record::wrap_continuation(ord, &named(name, None)).expect("a continuation record");
        let group = vec![channels().data().clone()];
        let patterns = vec![any()];
        let source = Consume::create(&group, &patterns, &continuation, false);
        WaitingContinuation {
            patterns,
            continuation,
            persist: false,
            peeks: BTreeSet::new(),
            source,
        }
    }

    /// Added by decision 124 (plan v4 C7.9, rule R5): an item rule that refuses the datum whose
    /// first value is `self.0`.
    struct RefusesValue(Par);

    impl RowItemRule<TaggedContinuation, ListParWithRandom> for RefusesValue {
        fn admit_item(
            &self,
            _continuation: &TaggedContinuation,
            _continuation_persistent: bool,
            _channel_count: usize,
            datum: &ListParWithRandom,
            _datum_persistent: bool,
        ) -> Result<bool, SpaceTxnError> {
            Ok(datum.pars.first() != Some(&self.0))
        }
    }

    /// Added by decision 124 (plan v4 C7.9, rule CR-36, ruling OP-J): a view of the commit
    /// returns the records, the waiters and the joins of the lists of the commit, and reads each
    /// channel once: a change of the store after the first read reaches the lists and not the
    /// view.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commit_view_reads_through_the_lists() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        let data = channels().data().clone();
        let group = vec![data.clone()];
        let store = space.get_store();
        store
            .put_continuation(&group, waiter_of("w", 0))
            .expect("the waiter is stored");
        store.put_join(&data, &group).expect("the join is stored");
        let (_, later) = stored_on(&data, 2);
        let later_group = vec![string("later")];
        commit_lists(&space, &env, move |lists| {
            let matcher = Matcher;
            let mut view = CommitView::new(lists, &matcher, None, None, 1);
            let records = lists.data(&data).expect("the records of the lists");
            let waiters: Vec<ParWaiter> = lists
                .continuations(&group)
                .expect("the waiters of the lists")
                .iter()
                .map(|waiter| (**waiter).clone())
                .collect();
            let joins = lists.joins(&data).expect("the joins of the lists");
            assert_eq!(records.len(), 1, "the record of the fixture");
            assert_eq!(waiters.len(), 1, "the waiter of the fixture");
            assert_eq!(joins, vec![group.clone()], "the join of the fixture");
            assert_eq!(
                view.data_on(&data).expect("the records of the view"),
                records.as_slice()
            );
            assert_eq!(
                view.waiters_on(&data).expect("the waiters of the view"),
                waiters.as_slice()
            );
            assert_eq!(
                view.joins_on(&data).expect("the joins of the view"),
                joins.as_slice()
            );
            // The later writes reach the lists, and the view keeps its first read of the channel.
            store.put_datum(&data, later);
            store
                .put_continuation(&group, waiter_of("later", 1))
                .expect("the later waiter is stored");
            store
                .put_join(&data, &later_group)
                .expect("the later join is stored");
            assert_eq!(lists.data(&data).expect("the records").len(), 2);
            assert_eq!(lists.continuations(&group).expect("the waiters").len(), 2);
            assert_eq!(lists.joins(&data).expect("the joins").len(), 2);
            assert_eq!(view.data_on(&data).expect("the view"), records.as_slice());
            assert_eq!(
                view.waiters_on(&data).expect("the view"),
                waiters.as_slice()
            );
            assert_eq!(view.joins_on(&data).expect("the view"), joins.as_slice());
            // Undo the writes, so the commit finds the store that it staged against.
            store.remove_datum(&data, 0).expect("the later record goes");
            store
                .remove_continuation(&group, 0)
                .expect("the later waiter goes");
            store
                .remove_join(&data, &later_group)
                .expect("the later join goes");
        })
        .await;
    }

    /// Added by decision 124 (plan v4 C7.9, rule R5): with an item rule that refuses one value,
    /// the bind of the view of the commit misses that datum and binds the others. The bind of the
    /// staging runs no item test, so it binds both.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commit_view_bind_item_misses_a_refused_datum() {
        fn values_of(bound: Option<ListParWithRandom>) -> Option<Vec<Par>> {
            bound.map(|matched| matched.pars)
        }

        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        let (refused, other) = (send(1).data, send(2).data);
        let mut staged = None;
        staged_plan(&env, |cx| {
            staged = Some((
                cx.bind_item(&any(), &refused, false)?,
                cx.bind_item(&any(), &other, false)?,
            ));
            Ok(())
        })
        .await
        .expect("the plan commits");
        let (first, second) = staged.expect("the plan ran");
        assert_eq!(values_of(first), Some(vec![int(1)]), "the staging binds it");
        assert_eq!(
            values_of(second),
            Some(vec![int(2)]),
            "the staging binds it"
        );
        let (missed, bound) = commit_lists(&space, &env, move |lists| {
            let rule = RefusesValue(int(1));
            let continuation = wrapped("k");
            let item = RowItemTest {
                rule: &rule,
                continuation: &continuation,
                persistent: false,
                channel_count: 1,
            };
            let matcher = Matcher;
            let view = CommitView::new(lists, &matcher, None, Some(item), 1);
            let missed = view.bind_item(&any(), &refused, false);
            let bound = view.bind_item(&any(), &other, false);
            (
                missed.expect("the bind of the refused value"),
                bound.expect("the bind of the other value"),
            )
        })
        .await;
        assert_eq!(values_of(missed), None, "a refused datum is a miss");
        assert_eq!(
            values_of(bound),
            Some(vec![int(2)]),
            "the other value binds"
        );
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7, form (1)): the guard of a list and
    /// the theory on each stored member give one answer through the staged view and through the
    /// view of the commit: with no theory, with a theory that refutes a stored member, with a
    /// theory and no stored member, and with a theory that gives a lease.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admits_records_shares_the_theory_part() {
        use crate::rust::interpreter::spaces::theory::test_theories::{self, TestBehavior};

        fn payload_of(record: &ListParWithRandom) -> Result<ListParWithRandom, SpaceError> {
            Ok(record.clone())
        }

        let stored = Some(send(1).data);
        let cases = [
            (None, vec![stored.clone(), None], true),
            (
                Some(TestBehavior::RefuteMatch),
                vec![stored.clone(), None],
                false,
            ),
            (Some(TestBehavior::RefuteMatch), vec![None], true),
            (
                Some(TestBehavior::Leasing),
                vec![stored.clone(), None],
                true,
            ),
        ];
        for (behavior, members, admitted) in cases {
            let Fixture { space, env } = fixture().await;
            produce(&env, send(2)).await;
            let checker = behavior.map(|behavior| {
                theory::resolve(&test_theories::identity(behavior)).expect("it resolves")
            });
            let continuation = wrapped("k");
            let matched = send(3).data;
            let mut staged = None;
            staged_plan_with(&env, checker.clone(), |cx| {
                staged = Some(cx.admits_records(
                    &continuation,
                    &matched,
                    &members,
                    |member| member.as_ref(),
                    payload_of,
                )?);
                Ok(())
            })
            .await
            .expect("the plan commits");
            let answer = commit_lists(&space, &env, move |lists| {
                let matcher = Matcher;
                let mut view = CommitView::new(lists, &matcher, checker.as_deref(), None, 1);
                view.admits_records(
                    &continuation,
                    &matched,
                    &members,
                    |member| member.as_ref(),
                    payload_of,
                )
                .expect("the commit check of the list")
            })
            .await;
            assert_eq!(staged, Some(admitted), "the staging with {behavior:?}");
            assert_eq!(answer, admitted, "the commit view with {behavior:?}");
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): every push of a staged
    /// row takes one class, so `staged` and `tail_rows` keep one index for each row of a plan with
    /// an anchor, a fire and counter rows.
    #[tokio::test]
    async fn push_row_keeps_one_index_for_every_push() {
        let Fixture { env, .. } = fixture().await;
        produce(&env, send(1)).await;
        let ch = channels();
        let random = Blake2b512Random::create_from_bytes(b"push row test");
        let mut seen = None;
        staged_plan(&env, |cx| {
            let data = cx.data_channel();
            let candidates = candidates_by_ord(cx)?;
            cx.fire_receive_group(
                vec![data.clone()],
                vec![any()],
                wrapped("k"),
                false,
                BTreeSet::new(),
                candidates,
                1,
                None,
            )?;
            let introduction = Arc::clone(cx.introduction());
            let authority = OperationAuthority {
                continuation: None,
                introduction: &introduction,
            };
            cx.bump_on(ch.counters(), &random, authority)?;
            seen = Some((cx.plan.staged.clone(), cx.plan.tail_rows.clone()));
            Ok(())
        })
        .await
        .expect("the plan commits");
        let (staged, tail_rows) = seen.expect("the plan ran");
        assert_eq!(staged, vec![
            Staged::Anchor,
            Staged::Fire,
            Staged::Counter(CounterOp::Take),
            Staged::Counter(CounterOp::Put),
        ]);
        assert_eq!(tail_rows, vec![
            TailRow::Counter,
            TailRow::Element,
            TailRow::Counter,
            TailRow::Counter,
        ]);
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): the rows of a settle
    /// before the plan, of the plan and of a settle after it take the class of their phase. A
    /// settle before the plan gives its lifts, fires and lands the class `Before`. The rows of the
    /// plan keep their classes. A settle after the plan keeps its lifts, gives its fires and lands
    /// the class `Settle` and leaves a counter row as it is.
    #[tokio::test]
    async fn settle_rows_class_the_rows_of_each_settle() {
        let Fixture { env, .. } = fixture().await;
        staged_plan(&env, |cx| {
            for kind in [Staged::Lift, Staged::Fire, Staged::Store] {
                cx.plan.push_row(kind);
            }
            cx.settle_rows(0, true);
            for kind in [Staged::Store, Staged::Lift] {
                cx.plan.push_row(kind);
            }
            let plan_end = cx.plan.tail_rows.len();
            let after = [
                Staged::Lift,
                Staged::Fire,
                Staged::Store,
                Staged::Counter(CounterOp::Take),
            ];
            for kind in after {
                cx.plan.push_row(kind);
            }
            cx.settle_rows(plan_end, false);
            assert_eq!(cx.plan.tail_rows, vec![
                TailRow::Before,
                TailRow::Before,
                TailRow::Before,
                TailRow::Element,
                TailRow::Lift,
                TailRow::Lift,
                TailRow::Settle,
                TailRow::Settle,
                TailRow::Counter,
            ]);
            // Nothing was staged in the transaction, so the plan commits empty.
            cx.plan.staged.clear();
            cx.plan.tail_rows.clear();
            Ok(())
        })
        .await
        .expect("the plan commits");
    }

    /// Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.3 and D-10): an owner-funded
    /// run keeps one copy of its operation, which the row searches of the join engine share (the
    /// plan hands out the same reference each time), and a later operation of the same run does
    /// not replace it. A run without owner funding keeps none.
    #[tokio::test]
    async fn keep_operation_copies_an_owner_funded_operation_once() {
        use crate::rust::interpreter::accounting::owner_funding::{
            OwnerFundingBinding, OwnerPurseSet, OwnerTable,
        };
        use crate::rust::interpreter::accounting::Sig;
        use crate::rust::interpreter::reified::system_processes::harness::native_config;

        fn no_purse(_: &CostSignature) -> Option<OwnerPurseSet> { None }

        let first = SpaceOp::Produce(send(1));
        let second = SpaceOp::Produce(send(2));
        let Fixture { env, .. } = fixture().await;
        env.budget
            .set_deploy_id_funded([7; 32], Sig::Ground(vec![1; 33]))
            .expect("a funded deployment");
        env.budget
            .reset_for_native_execution(native_config().with_owner_funding(Arc::new(
                OwnerFundingBinding::new(BTreeSet::new(), no_purse, OwnerTable::empty(0)),
            )))
            .expect("the budget enters a native execution with owner funding");
        staged_plan(&env, |cx| {
            let kept = cx.keep_operation(&first).expect("an owner-funded copy");
            let again = cx.keep_operation(&second).expect("the one copy");
            assert!(Arc::ptr_eq(&kept, &again), "a later call keeps the copy");
            assert_eq!(*kept, first, "the copy is the first operation");
            let held = cx.operation().expect("the plan holds the copy");
            assert!(Arc::ptr_eq(&kept, &held), "one copy for every reader");
            Ok(())
        })
        .await
        .expect("the plan commits");
        let Fixture { env, .. } = fixture().await;
        staged_plan(&env, |cx| {
            assert!(cx.keep_operation(&first).is_none(), "no owner funding");
            assert!(cx.operation().is_none(), "no copy");
            Ok(())
        })
        .await
        .expect("the plan commits");
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31): one row of a receipt.
    type ParRowReceipt = SpaceOpReceipt<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    /// Added by decision 124 (plan v4 C7.11, rule CR-31): a row that stored its record.
    fn stored_row() -> ParRowReceipt {
        SpaceOpReceipt::Stored {
            produce: Produce::create(&int(1), &int(2), false),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31): a row that parked its continuation.
    fn parked_row() -> ParRowReceipt {
        SpaceOpReceipt::Parked {
            consume: Consume::create(&vec![int(1)], &vec![any()], &named("k", None), false),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31): a row that R8 skipped.
    fn skipped_row() -> ParRowReceipt {
        use rspace_plus_plus::rspace::space_store_handle::OpSource;
        SpaceOpReceipt::Skipped {
            source: OpSource::Produce(Produce::create(&int(1), &int(2), false)),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31): a row of a fire of one record, which
    /// the commit removed as `removed`. A send triggered it when `by_produce` holds. The waiter
    /// is persistent when `waiter_persistent` holds and peeks when `peek` holds, and the record
    /// is persistent when `datum_persistent` holds.
    fn fired_row(
        by_produce: bool,
        waiter_persistent: bool,
        peek: bool,
        datum_persistent: bool,
        removed: ListParWithRandom,
    ) -> ParRowReceipt {
        let group = vec![int(1)];
        let patterns = vec![any()];
        let waiter = named("k", None);
        let consume = Consume::create(&group, &patterns, &waiter, waiter_persistent);
        let produce = Produce::create(&group[0], &removed, datum_persistent);
        SpaceOpReceipt::Fired {
            comm: COMM {
                consume,
                produces: vec![produce.clone()],
                peeks: BTreeSet::new(),
                times_repeated: BTreeMap::new(),
            },
            continuation: ContResult {
                continuation: waiter,
                persistent: waiter_persistent,
                channels: group.clone(),
                patterns,
                peek,
            },
            results: vec![RSpaceResult {
                channel: group[0].clone(),
                matched_datum: removed.clone(),
                removed_datum: removed,
                persistent: datum_persistent,
            }],
            produce: by_produce.then_some(produce),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31): the receipt of a commit that deviates
    /// at the row `index`, with the tail read `read` of its search and no searched end.
    fn deviated(ops: Vec<ParRowReceipt>, index: usize, read: TailRead) -> ParSpaceReceipt {
        use rspace_plus_plus::rspace::space_store_handle::RowDeviation;
        ParSpaceReceipt {
            ops,
            deviation: Some(RowDeviation {
                index,
                item_failed: Vec::new(),
                dispatched: None,
                vetoing: false,
                tail_read: read,
                claims: None,
                reissue: None,
            }),
            refused: Vec::new(),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31): `tail_end` of the row `row` of the
    /// class `class` after `fires` fires of the plan, for the operation `op`, whose round bumped
    /// the counter from `bumped`, in a discipline that settles when `settles` holds.
    fn tail_of(
        row: ParRowReceipt,
        fires: usize,
        class: TailRow,
        read: TailRead,
        op: &SpaceOp,
        bumped: Option<i64>,
        settles: bool,
    ) -> Result<Option<TailEnd>, SpaceError> {
        let mut ops = Vec::with_capacity(fires + 1);
        ops.extend((0..fires).map(|_| stored_row()));
        ops.push(row);
        let staged = vec![Staged::Fire; fires];
        let mut tail_rows = vec![TailRow::Element; fires];
        tail_rows.push(class);
        let receipt = deviated(ops, fires, read);
        tail_end(&receipt, &staged, &tail_rows, Some(op), bumped, settles)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.3 and RADD2 1.1):
    /// `tail_end` gives the Return or the Stop of each row of the table of C4v2 B.6, rows 8 and 8b
    /// included, with no searched end.
    #[test]
    fn tail_end_follows_the_b6_table() {
        let op = send(1);
        let operation = SpaceOp::Produce(op.clone());
        let job = SpaceOp::Settle(settle_operation(OpView::Produce(&op)));
        let again = SpaceOp::Produce(ProduceOp {
            reissued_ord: None,
            ..op.clone()
        });
        let ended = Some(TailEnd::Return(None));
        let removed = record::wrap_datum(0, &send(1).data);
        let fire = |by_produce, waiter_persistent, peek, datum_persistent| {
            fired_row(
                by_produce,
                waiter_persistent,
                peek,
                datum_persistent,
                removed.clone(),
            )
        };
        let end = |row, fires, class, read, on: &SpaceOp, bumped| {
            tail_of(row, fires, class, read, on, bumped, false).expect("a row with an end")
        };
        let stop = |rest: &SpaceOp| Some(TailEnd::Stop(rest.clone()));
        let returned = |rest: &SpaceOp, cause| {
            Some(TailEnd::Return(Some(SpaceReissue {
                op: rest.clone(),
                cause,
            })))
        };
        // Without a deviation the plan's own return holds.
        let plain = ParSpaceReceipt {
            ops: vec![stored_row()],
            deviation: None,
            refused: Vec::new(),
        };
        assert_eq!(
            tail_end(&plain, &[], &[], Some(&operation), None, false).expect("no deviation"),
            None
        );
        // Row 1: the element of the operation follows the settle before the plan.
        assert_eq!(
            end(
                stored_row(),
                0,
                TailRow::Before,
                TailRead::Ends,
                &operation,
                None
            ),
            stop(&again)
        );
        // Rows 2 and 4: the element is in place, and the read of the search ends the row.
        for row in [stored_row(), parked_row()] {
            assert_eq!(
                end(
                    row.clone(),
                    0,
                    TailRow::Element,
                    TailRead::Ends,
                    &operation,
                    None
                ),
                ended
            );
            assert_eq!(
                end(
                    row.clone(),
                    0,
                    TailRow::Element,
                    TailRead::Stages,
                    &operation,
                    None
                ),
                stop(&job)
            );
            assert_eq!(
                end(
                    row,
                    0,
                    TailRow::Element,
                    TailRead::Reissues,
                    &operation,
                    None
                ),
                returned(&job, ReissueCause::RoundCap)
            );
        }
        // Row 3 of a send: a fire that spends the record reads the search, a waiter that peeks
        // leaves the record in flight, and a waiter that stays with it re-issues the operation.
        assert_eq!(
            end(
                fire(true, false, false, false),
                0,
                TailRow::Element,
                TailRead::Ends,
                &operation,
                None
            ),
            ended
        );
        assert_eq!(
            end(
                fire(true, false, false, false),
                0,
                TailRow::Element,
                TailRead::Stages,
                &operation,
                None
            ),
            stop(&job)
        );
        assert_eq!(
            end(
                fire(true, false, true, false),
                0,
                TailRow::Element,
                TailRead::Ends,
                &operation,
                None
            ),
            stop(&again)
        );
        assert_eq!(
            end(
                fire(true, false, true, false),
                FIRE_ROUND_CAP - 1,
                TailRow::Element,
                TailRead::Ends,
                &operation,
                None
            ),
            returned(&again, ReissueCause::RoundCap)
        );
        assert_eq!(
            end(
                fire(true, true, true, false),
                0,
                TailRow::Element,
                TailRead::Ends,
                &operation,
                None
            ),
            returned(&again, ReissueCause::DivergentPair)
        );
        // Row 5: a receive that is not persistent is spent, and a persistent one goes on with
        // the order of its first round, or re-issues when its record stays too.
        let once = SpaceOp::Consume(receive("w", any()));
        assert_eq!(
            end(
                fire(false, false, false, false),
                0,
                TailRow::Element,
                TailRead::Ends,
                &once,
                None
            ),
            ended
        );
        let forever = receive_forever("w");
        let carried = SpaceOp::Consume(ConsumeOp {
            reissued_ord: Some(2),
            ..forever.clone()
        });
        let forever = SpaceOp::Consume(forever);
        assert_eq!(
            end(
                fire(false, true, false, false),
                0,
                TailRow::Element,
                TailRead::Ends,
                &forever,
                Some(2)
            ),
            stop(&carried)
        );
        assert_eq!(
            end(
                fire(false, true, false, true),
                0,
                TailRow::Element,
                TailRead::Ends,
                &forever,
                Some(2)
            ),
            returned(&carried, ReissueCause::DivergentPair)
        );
        // Row 6: a skipped row ends unless the discipline settles and earlier rows fired.
        for (settles, fires, expected) in [
            (false, 1, ended.clone()),
            (true, 0, ended.clone()),
            (true, 1, stop(&job)),
        ] {
            assert_eq!(
                tail_of(
                    skipped_row(),
                    fires,
                    TailRow::Element,
                    TailRead::Ends,
                    &operation,
                    None,
                    settles
                )
                .expect("a skipped row ends"),
                expected,
                "settles {settles}, fires {fires}"
            );
        }
        // Row 7, and a retire of a group: the cut of a commit never names such a row.
        for class in [TailRow::Counter, TailRow::Retire] {
            assert!(
                tail_of(
                    stored_row(),
                    0,
                    class,
                    TailRead::Ends,
                    &operation,
                    None,
                    false
                )
                .is_err(),
                "{class:?}"
            );
        }
        // Row 8: the re-offer follows the lift.
        assert_eq!(
            end(
                parked_row(),
                0,
                TailRow::Lift,
                TailRead::Ends,
                &operation,
                None
            ),
            stop(&job)
        );
        // Row 8b: the cut of a group names its unit row, and the retire row after it does not
        // change the end of the row.
        let group = deviated(
            vec![fire(true, false, false, false), stored_row()],
            0,
            TailRead::Stages,
        );
        let staged = [Staged::Fire, Staged::Retire];
        let classes = [TailRow::Element, TailRow::Retire];
        assert_eq!(
            tail_end(&group, &staged, &classes, Some(&operation), None, false).expect("row 8b"),
            stop(&job)
        );
        // Row 9: the settle reads the search after a store and after a fire that spends its
        // record. A waiter that peeks lands the record in flight first, and two persistent sides
        // re-issue the settle.
        assert_eq!(
            end(
                stored_row(),
                0,
                TailRow::Settle,
                TailRead::Ends,
                &operation,
                None
            ),
            ended
        );
        assert_eq!(
            end(
                stored_row(),
                0,
                TailRow::Settle,
                TailRead::Stages,
                &operation,
                None
            ),
            stop(&job)
        );
        assert_eq!(
            end(
                fire(true, false, false, false),
                0,
                TailRow::Settle,
                TailRead::Ends,
                &operation,
                None
            ),
            ended
        );
        assert_eq!(
            end(
                fire(true, true, true, false),
                0,
                TailRow::Settle,
                TailRead::Ends,
                &operation,
                None
            ),
            stop(&job)
        );
        assert_eq!(
            end(
                fire(true, true, false, true),
                0,
                TailRow::Settle,
                TailRead::Ends,
                &operation,
                None
            ),
            returned(&job, ReissueCause::DivergentPair)
        );
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): a receipt whose
    /// deviation carries the end of its search gives that end for each class of row d, a receipt
    /// without one falls back to the table, and a re-issue of another type fails closed.
    #[test]
    fn tail_end_takes_the_searched_end_first() {
        let send_one = send(1);
        let operation = SpaceOp::Produce(send_one.clone());
        let searched = TailEnd::Stop(SpaceOp::Produce(send(9)));
        let with_reissue = |reissue| {
            let mut receipt = deviated(vec![stored_row()], 0, TailRead::Stages);
            receipt.deviation.as_mut().expect("a deviation").reissue = reissue;
            receipt
        };
        let classes = [
            TailRow::Counter,
            TailRow::Before,
            TailRow::Element,
            TailRow::Lift,
            TailRow::Settle,
            TailRow::Retire,
            TailRow::ClassFire { persistent: false },
            TailRow::Restore,
            TailRow::CascadeLift,
            TailRow::DivergentCascadeLift,
        ];
        for class in classes {
            let receipt = with_reissue(Some(RowReissue::new(searched.clone())));
            assert_eq!(
                tail_end(
                    &receipt,
                    &[Staged::Store],
                    &[class],
                    Some(&operation),
                    None,
                    false
                ),
                Ok(Some(searched.clone())),
                "{class:?}"
            );
        }
        let table = with_reissue(None);
        assert_eq!(
            tail_end(
                &table,
                &[Staged::Store],
                &[TailRow::Element],
                Some(&operation),
                None,
                false
            ),
            Ok(Some(TailEnd::Stop(SpaceOp::Settle(settle_operation(
                OpView::Produce(&send_one)
            ))))),
            "the table of the row"
        );
        let wrong = with_reissue(Some(RowReissue::new(1_u8)));
        assert_eq!(
            tail_end(
                &wrong,
                &[Staged::Store],
                &[TailRow::Element],
                Some(&operation),
                None,
                false
            ),
            Err(SpaceError::Inconsistent(
                "the search of row d handed out a value that is not an end of Tail"
            ))
        );
    }

    /// Added by decision 124 (plan v4 C7.11, rules CR-9 and CR-31, ruling RADD2 1.3): a fire that
    /// binds one member of its class: the class stays when the receive peeks or another member
    /// is persistent, and the end of the row follows the class rule.
    #[test]
    fn tail_end_reads_the_class_rule_of_a_scan_fire() {
        let forever = receive_forever("w");
        let operation = SpaceOp::Consume(forever.clone());
        let reissue = reissued_operation(OpView::Consume(&forever), None);
        let stays = Some(TailEnd::Return(Some(SpaceReissue {
            op: reissue.clone(),
            cause: ReissueCause::DivergentPair,
        })));
        let removed = record::wrap_datum(0, &send(1).data);
        let fire = |peek, member_persistent| {
            fired_row(false, true, peek, member_persistent, removed.clone())
        };
        let end = |row, class| {
            tail_of(row, 0, class, TailRead::Ends, &operation, None, false).expect("a class fire")
        };
        let plain = TailRow::ClassFire { persistent: false };
        let kept = TailRow::ClassFire { persistent: true };
        assert_eq!(
            end(fire(false, false), plain),
            Some(TailEnd::Stop(reissue.clone())),
            "the class is spent"
        );
        assert_eq!(
            end(fire(false, false), kept),
            stays,
            "another member is persistent"
        );
        assert_eq!(
            end(fire(false, true), plain),
            stays,
            "the bound member is persistent"
        );
        assert_eq!(end(fire(true, true), plain), stays, "the receive peeks");
        // The fire of a receive that is not persistent is spent, so the read of the search ends
        // the row.
        let once = receive("w", any());
        let spent = SpaceOp::Consume(once.clone());
        let row = fired_row(false, false, false, false, removed.clone());
        let spent_end = |read| tail_of(row.clone(), 0, plain, read, &spent, None, false);
        assert_eq!(
            spent_end(TailRead::Ends).expect("a spent fire"),
            Some(TailEnd::Return(None))
        );
        assert_eq!(
            spent_end(TailRead::Stages).expect("a spent fire"),
            Some(TailEnd::Stop(SpaceOp::Settle(settle_operation(
                OpView::Consume(&once)
            ))))
        );
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3, Rule E): a peek fire
    /// of a receive that removed a record, and a second send of that record, stop with the send
    /// of the removed record in the form of `restored_send`. A second send that stored its
    /// record reads the search. A removed record that the receipt does not hold fails closed.
    #[test]
    fn tail_end_reissues_the_second_send_of_a_scan() {
        let op = peek("w");
        let operation = SpaceOp::Consume(op.clone());
        let removed = record::wrap_datum(0, &send(1).data);
        let send_again = Some(TailEnd::Stop(SpaceOp::Produce(
            restored_send(&op, send(1).data).expect("the second send"),
        )));
        let peek_fire = || fired_row(false, false, true, false, removed.clone());
        let send_fire = || fired_row(true, false, true, false, removed.clone());
        let two_rows = |first, second| {
            let receipt = deviated(vec![first, second], 1, TailRead::Ends);
            tail_end(
                &receipt,
                &[Staged::Fire, Staged::Fire],
                &[TailRow::Element, TailRow::Restore],
                Some(&operation),
                None,
                false,
            )
        };
        // The peek fire of row d owes the second send.
        assert_eq!(
            tail_of(
                peek_fire(),
                0,
                TailRow::Element,
                TailRead::Ends,
                &operation,
                None,
                false
            ),
            Ok(send_again.clone())
        );
        // The second send fired a waiter that peeks: the record stays in flight.
        assert_eq!(two_rows(peek_fire(), send_fire()), Ok(send_again));
        // The second send stored its record: the spent receive ends with the read of its search.
        assert_eq!(
            two_rows(peek_fire(), stored_row()),
            Ok(Some(TailEnd::Return(None)))
        );
        // The row before the second send is not a fire of the receive.
        assert_eq!(
            two_rows(stored_row(), send_fire()),
            Err(SpaceError::Inconsistent(
                "a second send of Rule E follows no fire of a receive"
            ))
        );
        // The fire of the receive holds no removed record.
        let mut empty = peek_fire();
        match &mut empty {
            SpaceOpReceipt::Fired { results, .. } => results.clear(),
            SpaceOpReceipt::Stored { .. }
            | SpaceOpReceipt::Parked { .. }
            | SpaceOpReceipt::Skipped { .. } => {}
        }
        assert_eq!(
            two_rows(empty, send_fire()),
            Err(SpaceError::Inconsistent(
                "a receive that peeks fired other than one record"
            ))
        );
        // A bumped counter names no discipline of Rule E.
        assert_eq!(
            tail_of(
                peek_fire(),
                0,
                TailRow::Element,
                TailRead::Ends,
                &operation,
                Some(1),
                false
            ),
            Err(SpaceError::Inconsistent(
                "a second send of Rule E follows no receive of a discipline without a counter"
            ))
        );
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.4): row 6 of the table of
    /// C4v2 B.6 gives the settle job when the discipline settles and `Return(None)` when it does
    /// not, and a stack and a priority queue settle while the default does not.
    #[test]
    fn row_six_settles_only_where_the_discipline_settles() {
        use crate::rust::interpreter::spaces::disciplines::priority_queue::PriorityQueueDiscipline;
        use crate::rust::interpreter::spaces::disciplines::stack::StackDiscipline;

        fn settles_of<D: Discipline>() -> bool { D::SETTLES }

        let op = send(1);
        let operation = SpaceOp::Produce(op.clone());
        let job = SpaceOp::Settle(settle_operation(OpView::Produce(&op)));
        let skipped = |settles| {
            tail_of(
                skipped_row(),
                1,
                TailRow::Element,
                TailRead::Ends,
                &operation,
                None,
                settles,
            )
            .expect("a skipped row ends")
        };
        assert_eq!(skipped(true), Some(TailEnd::Stop(job)));
        assert_eq!(skipped(false), Some(TailEnd::Return(None)));
        assert!(settles_of::<StackDiscipline>());
        assert!(settles_of::<PriorityQueueDiscipline>());
        assert!(!settles_of::<QueueDiscipline>());
    }

    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the data channel, the records and
    /// the waiters of the collection are the same through the staged view and through the view
    /// of a tail read over the lists of the same store.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn store_view_reads_the_collection_in_both_views() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        let data = channels().data().clone();
        space
            .get_store()
            .put_continuation(std::slice::from_ref(&data), waiter_of("w", 0))
            .expect("the waiter is stored");
        let mut staged = None;
        staged_plan(&env, |cx| {
            let channel = StoreView::data_channel(&*cx)?.clone();
            let records = StoreView::data(&mut *cx)?.to_vec();
            let waiters = StoreView::waiters(&mut *cx)?.to_vec();
            staged = Some((channel, records, waiters));
            Ok(())
        })
        .await
        .expect("the plan commits");
        let staged = staged.expect("the plan ran");
        assert_eq!(staged.0, data, "the data channel of the collection");
        assert_eq!(staged.1.len(), 1, "the record of the fixture");
        assert_eq!(staged.2.len(), 1, "the waiter of the fixture");
        let seen = commit_lists(&space, &env, move |lists| {
            let matcher = Matcher;
            let book = BookPairs::new();
            let mut view = CommitView::of_tail(lists, &matcher, None, Some(&data), &book, 1);
            let channel = StoreView::data_channel(&view).expect("the channel").clone();
            let records = StoreView::data(&mut view).expect("the records").to_vec();
            let waiters = StoreView::waiters(&mut view).expect("the waiters").to_vec();
            (channel, records, waiters)
        })
        .await;
        assert_eq!(seen, staged, "the two views read one collection");
    }

    /// Added by decision 124 (plan v2 C7.10, plan v4 C7.11, ruling RADD 1.4): `head_waiter_of`
    /// over the view of a tail read gives the waiter of `Cx::head_waiter` over the staged view.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn head_waiter_of_matches_the_staged_head_waiter() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        let data = channels().data().clone();
        let store = space.get_store();
        for (name, ord) in [("first", 0), ("second", 1)] {
            store
                .put_continuation(std::slice::from_ref(&data), waiter_of(name, ord))
                .expect("the waiter is stored");
        }
        let mut staged = None;
        staged_plan(&env, |cx| {
            staged = Some(cx.head_waiter(&QueueDiscipline)?);
            Ok(())
        })
        .await
        .expect("the plan commits");
        let staged = staged.expect("the plan ran").expect("a staged head waiter");
        assert_eq!(staged, waiter_of("first", 0), "the least order");
        let viewed = commit_lists(&space, &env, move |lists| {
            let matcher = Matcher;
            let book = BookPairs::new();
            let mut view = CommitView::of_tail(lists, &matcher, None, Some(&data), &book, 1);
            let waiters = view.waiters().expect("the waiters of the view");
            head_waiter_of(&QueueDiscipline, waiters).expect("the head waiter")
        })
        .await;
        assert_eq!(viewed, Some(staged));
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4, R-MACH form (1)): the
    /// tail read of a head row answers through the discipline of its collection: a stack owes
    /// the next fire of its stored pair, and a queue answers `Ends`. A machinery row answers
    /// `Ends`, even for a stack.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn staged_row_search_tail_read_uses_its_discipline() {
        use crate::rust::interpreter::spaces::disciplines::stack::StackDiscipline;

        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        let data = channels().data().clone();
        let waiter = waiter_of("w", 0);
        space
            .get_store()
            .put_continuation(std::slice::from_ref(&data), waiter.clone())
            .expect("the waiter is stored");
        let (record, datum) = stored_on(&data, 1);
        let candidate = incoming_candidate(
            data.clone(),
            datum.source.clone(),
            record.clone(),
            false,
            record,
        );
        let mut built = None;
        staged_plan(&env, |cx| {
            let stack = StagedRowSearch::<StackDiscipline>::of_offer(
                cx,
                &waiter,
                &data,
                candidate.clone(),
                0,
            );
            let queue = StagedRowSearch::<QueueDiscipline>::of_offer(
                cx,
                &waiter,
                &data,
                candidate.clone(),
                0,
            );
            built = Some((stack, queue));
            Ok(())
        })
        .await
        .expect("the plan commits");
        let (mut stack, mut queue) = built.expect("the plan ran");
        let mut machinery = StagedRowSearch::<StackDiscipline>::of_machinery(
            std::slice::from_ref(&data),
            std::slice::from_ref(&candidate),
        );
        let reads = commit_lists(&space, &env, move |lists| {
            let matcher = Matcher;
            (
                stack
                    .tail_read(lists, &matcher)
                    .expect("the read of the stack"),
                queue
                    .tail_read(lists, &matcher)
                    .expect("the read of the queue"),
                machinery
                    .tail_read(lists, &matcher)
                    .expect("the read of the machinery"),
            )
        })
        .await;
        assert_eq!(reads, (TailRead::Stages, TailRead::Ends, TailRead::Ends));
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-20, errata V4-13, F-TAIL): the tail read
    /// skips a pair that the book of the staging holds, as the staging skips it, and answers
    /// without that fire. The search reads the book that its staging read, so a search built
    /// before the pair was booked still owes the fire.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn staged_row_search_tail_read_reads_the_staging_book() {
        use crate::rust::interpreter::spaces::disciplines::stack::StackDiscipline;

        fn stack_search(
            cx: &Cx<'_, '_>,
            waiter: &ParWaiter,
            data: &Par,
            candidate: &ParCandidate,
        ) -> StagedRowSearch<StackDiscipline> {
            StagedRowSearch::<StackDiscipline>::of_offer(cx, waiter, data, candidate.clone(), 0)
        }

        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        let data = channels().data().clone();
        let waiter = waiter_of("w", 0);
        let store = space.get_store();
        store
            .put_continuation(std::slice::from_ref(&data), waiter.clone())
            .expect("the waiter is stored");
        let stored = store
            .get_data(&data)
            .first()
            .cloned()
            .expect("the stored record");
        let (record, datum) = stored_on(&data, 1);
        let candidate = incoming_candidate(
            data.clone(),
            datum.source.clone(),
            record.clone(),
            false,
            record,
        );
        let mut unbooked = None;
        staged_plan(&env, |cx| {
            unbooked = Some(stack_search(cx, &waiter, &data, &candidate));
            Ok(())
        })
        .await
        .expect("the plan commits");
        let mut unbooked = unbooked.expect("the plan ran");
        env.volatile
            .book_pairs([(stored.source.hash.clone(), waiter.source.hash.clone())]);
        let mut booked = None;
        staged_plan(&env, |cx| {
            booked = Some(stack_search(cx, &waiter, &data, &candidate));
            Ok(())
        })
        .await
        .expect("the plan commits");
        let mut booked = booked.expect("the plan ran");
        let reads = commit_lists(&space, &env, move |lists| {
            let matcher = Matcher;
            (
                unbooked
                    .tail_read(lists, &matcher)
                    .expect("the read without the pair"),
                booked
                    .tail_read(lists, &matcher)
                    .expect("the read with the pair"),
            )
        })
        .await;
        assert_eq!(reads, (TailRead::Stages, TailRead::Ends));
    }

    /// Added by decision 124 (plan v4 C7.9, rule T3, ruling OP-LISTS): the search
    /// of a head offer lists c0 alone: one run over the channel of the row, the stored waiter by
    /// its consume with its index in the lists, the staged tuple, and a check of c0 that passes
    /// without a new run. It retires nothing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn staged_row_search_lists_c0_with_its_tuple() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        let data = channels().data().clone();
        let group = vec![data.clone()];
        let waiter = waiter_of("w", 0);
        let store = space.get_store();
        store
            .put_continuation(&group, waiter.clone())
            .expect("the waiter is stored");
        store
            .put_continuation(&group, waiter_of("other", 1))
            .expect("the other waiter is stored");
        let (record, datum) = stored_on(&data, 1);
        let candidate = incoming_candidate(
            data.clone(),
            datum.source.clone(),
            record.clone(),
            false,
            record.clone(),
        );
        let mut built = None;
        staged_plan(&env, |cx| {
            built = Some(StagedRowSearch::<QueueDiscipline>::of_offer(
                cx,
                &waiter,
                &data,
                candidate.clone(),
                0,
            ));
            Ok(())
        })
        .await
        .expect("the plan commits");
        let mut search = built.expect("the plan ran");
        let patterns = vec![any()];
        let probe_waiter = waiter.clone();
        let seen = commit_lists(&space, &env, move |lists| {
            let matcher = Matcher;
            let request = ParRowRequest {
                matcher: &matcher,
                group: std::slice::from_ref(&data),
                patterns: &patterns,
                continuation: &probe_waiter.continuation,
                persistent: false,
                source: &probe_waiter.source,
                own: None,
                item: None,
            };
            let runs = search.runs();
            let waiters = search.continuations(0, lists).expect("the waiter of c0");
            let tuple = search
                .tuple(0, lists, request)
                .expect("the tuple of c0")
                .expect("c0 binds its staged tuple");
            let checked = search
                .commit_check(&request, &tuple)
                .expect("the check of c0");
            let retires = search.staged_retires();
            let actual = search.actual_fire(&request, &tuple);
            (
                runs,
                waiters,
                tuple,
                checked,
                retires,
                actual.retires.len(),
                actual.dispatched,
            )
        })
        .await;
        let (runs, waiters, tuple, checked, retires, retired, dispatched) = seen;
        assert_eq!(runs.len(), 1, "one run");
        assert_eq!(runs[0].channels, group, "the channel of the row");
        assert_eq!(runs[0].candidates, 0..1, "the tuple of c0");
        assert_eq!(waiters.len(), 1, "the waiter of the row alone");
        assert_eq!(*waiters[0].0, waiter, "the stored waiter by its consume");
        assert_eq!(waiters[0].1, 1, "its index in the lists");
        assert_eq!(tuple.len(), 1, "the staged tuple");
        assert_eq!(tuple[0].channel, candidate.channel);
        assert_eq!(tuple[0].datum, candidate.datum);
        assert_eq!(tuple[0].removed_datum, candidate.removed_datum);
        assert_eq!(tuple[0].datum_index, candidate.datum_index);
        assert!(checked, "the check of c0 passes");
        assert_eq!(
            (retires, retired, dispatched),
            (0, 0, None),
            "no retire and no prefix"
        );
    }

    /// Added by decision 124 (plan v4 C7.9, rule R5): a staged search answers with its tuple
    /// only when each datum of the tuple passes the item test of the request. A datum that
    /// fails it is a miss of its pattern, so the search answers `None`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn staged_row_search_misses_a_tuple_that_fails_its_item_test() {
        fn request<'a>(
            matcher: &'a Matcher,
            group: &'a [Par],
            patterns: &'a [BindPattern],
            waiter: &'a ParWaiter,
            rule: &'a RefusesValue,
        ) -> ParRowRequest<'a> {
            ParRowRequest {
                matcher,
                group,
                patterns,
                continuation: &waiter.continuation,
                persistent: false,
                source: &waiter.source,
                own: None,
                item: Some(RowItemTest {
                    rule,
                    continuation: &waiter.continuation,
                    persistent: false,
                    channel_count: 1,
                }),
            }
        }

        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        let data = channels().data().clone();
        let waiter = waiter_of("w", 0);
        let (record, datum) = stored_on(&data, 1);
        let candidate = incoming_candidate(
            data.clone(),
            datum.source.clone(),
            record,
            false,
            send(1).data,
        );
        let mut built = None;
        staged_plan(&env, |cx| {
            built = Some(StagedRowSearch::<QueueDiscipline>::of_offer(
                cx,
                &waiter,
                &data,
                candidate.clone(),
                0,
            ));
            Ok(())
        })
        .await
        .expect("the plan commits");
        let mut search = built.expect("the plan ran");
        let (refused, passed) = commit_lists(&space, &env, move |lists| {
            let matcher = Matcher;
            let group = vec![data.clone()];
            let patterns = vec![any()];
            let (refuses, admits) = (RefusesValue(int(1)), RefusesValue(int(2)));
            let refused = search
                .tuple(
                    0,
                    lists,
                    request(&matcher, &group, &patterns, &waiter, &refuses),
                )
                .expect("the search with the refusing rule");
            let passed = search
                .tuple(
                    0,
                    lists,
                    request(&matcher, &group, &patterns, &waiter, &admits),
                )
                .expect("the search with the admitting rule");
            (refused.is_none(), passed.map(|tuple| tuple.len()))
        })
        .await;
        assert!(refused, "the refused datum is a miss");
        assert_eq!(passed, Some(1), "the staged tuple passes");
    }

    /// Added by decision 124 (plan v4 C7.11, V4-5, rule CR-20): a machinery search books no item
    /// pair, even under a rule that refuses its tuple, while the search of a receive books the
    /// pair of its refused record and its consume.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn machinery_search_books_no_item_pairs() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        let data = channels().data().clone();
        let group = vec![data.clone()];
        let patterns = vec![any()];
        let continuation = wrapped("k");
        let source = Consume::create(&group, &patterns, &continuation, false);
        let (record, datum) = stored_on(&data, 1);
        let candidate = incoming_candidate(
            data.clone(),
            datum.source.clone(),
            record,
            false,
            send(1).data,
        );
        let machinery = StagedRowSearch::<QueueDiscipline>::of_machinery(
            std::slice::from_ref(&data),
            std::slice::from_ref(&candidate),
        );
        let mut built = None;
        staged_plan(&env, |cx| {
            built = Some(StagedRowSearch::<QueueDiscipline>::of_receive(
                cx,
                &source,
                &continuation,
                false,
                &data,
                candidate.clone(),
                0,
            ));
            Ok(())
        })
        .await
        .expect("the plan commits");
        let receive = built.expect("the plan ran");
        let (machinery_pairs, receive_pairs) = commit_lists(&space, &env, move |lists| {
            let matcher = Matcher;
            let rule = RefusesValue(int(1));
            (
                machinery
                    .item_failures(lists, &matcher, &rule)
                    .expect("the pairs of the machinery search"),
                receive
                    .item_failures(lists, &matcher, &rule)
                    .expect("the pairs of the receive search"),
            )
        })
        .await;
        assert!(
            machinery_pairs.is_empty(),
            "a machinery row books no item pair"
        );
        assert_eq!(receive_pairs, vec![(
            datum.source.hash.clone(),
            source.hash.clone()
        )]);
    }

    /// Added by decision 124 (plan v4 C7.9, T-C4-30, N3, rule CR-17): the check of the staged
    /// tuple c0 at the commit admits it without a new run. The guard of the matcher and the
    /// theory of the space do not run again, so the check extends no lease after `check_leases`
    /// ran: a refuting theory and a leasing theory give the same answer and no new lease.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commit_does_not_rerun_c0_checks_or_extend_leases() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        use crate::rust::interpreter::spaces::theory::test_theories::{self, TestBehavior};

        /// A matcher that counts the checks that the commit runs and refuses each.
        struct CountingMatcher(AtomicUsize);

        impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for CountingMatcher {
            fn get(
                &self,
                _pattern: &BindPattern,
                _data: &ListParWithRandom,
            ) -> Option<ListParWithRandom> {
                self.0.fetch_add(1, Ordering::SeqCst);
                None
            }

            fn check_commit(
                &self,
                _continuation: &TaggedContinuation,
                _matched: &[ListParWithRandom],
            ) -> bool {
                self.0.fetch_add(1, Ordering::SeqCst);
                false
            }
        }

        let data = channels().data().clone();
        let group = vec![data.clone()];
        let patterns = vec![any()];
        let continuation = wrapped("k");
        let source = Consume::create(&group, &patterns, &continuation, false);
        let (record, datum) = stored_on(&data, 1);
        let candidate = incoming_candidate(
            data.clone(),
            datum.source.clone(),
            record.clone(),
            false,
            record,
        );
        for behavior in [TestBehavior::RefuteMatch, TestBehavior::Leasing] {
            let Fixture { env, .. } = fixture().await;
            let checker = theory::resolve(&test_theories::identity(behavior)).expect("it resolves");
            let matcher = CountingMatcher(AtomicUsize::new(0));
            let mut seen = None;
            staged_plan_with(&env, Some(checker), |cx| {
                let mut search = StagedRowSearch::<QueueDiscipline>::of_receive(
                    cx,
                    &source,
                    &continuation,
                    false,
                    &data,
                    candidate.clone(),
                    0,
                );
                let request = ParRowRequest {
                    matcher: &matcher,
                    group: &group,
                    patterns: &patterns,
                    continuation: &continuation,
                    persistent: false,
                    source: &source,
                    own: None,
                    item: None,
                };
                let before = cx.plan.leases.len();
                let admitted = search
                    .commit_check(&request, std::slice::from_ref(&candidate))
                    .expect("the check of c0");
                seen = Some((admitted, before, cx.plan.leases.len()));
                Ok(())
            })
            .await
            .expect("the plan commits");
            let (admitted, before, after) = seen.expect("the plan ran");
            assert!(admitted, "{behavior:?}: c0 admits without a new run");
            assert_eq!(
                matcher.0.load(Ordering::SeqCst),
                0,
                "{behavior:?}: no guard runs"
            );
            assert_eq!(after, before, "{behavior:?}: no new lease");
        }
    }

    /// Added by decision 124 (plan v4 C7.19, T-C4-24, rule CR-24 bullets 2 and 3, ruling RADD2v4
    /// 7.6): after a deviation at row d, the finish of `run_plan` stores the claims of rows 0 to
    /// d, and no claim of a staged row after d. When the deviation carries the claims of its
    /// actual unit, the staged claims of row d give way to them. The read handle of a later job,
    /// which does not know the last uses, observes the stored claims.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn seq_claims_follow_the_committed_rows_after_a_deviation() {
        let space = NativeSpace {
            sid: SPACE.sid,
            durability: Durability::Ephemeral,
        };
        let ch = CollectionChannels::new(&HashMapOuter, space, &key_of(&channel()));
        let job = SpaceJob {
            space,
            ..plan_job()
        };
        let claimed = |byte: u8| {
            let key = CollectionKey::from_bytes(vec![byte]).expect("a key");
            namespace::gprivate(namespace::data_channel(space, &key))
        };
        // The staged channels of rows 0, 1 and 2, and the channel of the actual unit of row d.
        let rows = [claimed(1), claimed(2), claimed(3)];
        let actual = claimed(4);
        let unit = RowClaims {
            data: vec![actual.clone()],
            waiters: Vec::new(),
        };
        // Row 1 is row d. Each case gives the claims of the deviation, and whether a claim is
        // stored for rows 0, 1 and 2, for the channel of the actual unit, and for the data channel
        // of the collection, which the store of row 0 claims.
        for (claims, expected) in [
            (None, [true, true, false, false, true]),
            (Some(unit), [true, false, false, true, true]),
        ] {
            let env = env_over(play().await, &budget());
            let config = SpaceConfig::new(
                space.sid,
                HashMapOuter::NAME,
                QueueDiscipline::NAME,
                SpaceQualifier::Seq,
            );
            create(&env, SpaceCreation {
                space,
                ..creation(config.to_par())
            })
            .await;
            let seq = SeqClaims::open(&env.volatile, space, SpaceQualifier::Seq, false, b"item")
                .expect("a seq job opens its claims")
                .expect("a program job on a seq space claims");
            let inner = env.begin(ch.lock_keys()).await.expect("begin");
            let txn: ParSpaceTxn = Box::new(DeviatingCommit {
                inner,
                index: 1,
                claims,
            });
            let admission = Admission {
                theory: None,
                leases: Vec::new(),
            };
            run_plan(
                &env,
                txn,
                ch.data(),
                &config,
                &CostAuthority::default(),
                Some(seq),
                admission,
                &job,
                false,
                |cx| {
                    // Each staged row takes the claims since the row before it.
                    for (value, row) in (1..).zip(rows.iter()) {
                        cx.claim(row, ClaimRole::Data)?;
                        cx.store(record_of(&job, value), false)?;
                    }
                    Ok(None)
                },
            )
            .expect("the plan stages its rows")
            .commit()
            .await
            .expect("the finish takes the deviating receipt");
            let later = SeqClaims::open(&env.volatile, space, SpaceQualifier::Seq, false, b"later")
                .expect("a seq job opens its claims")
                .expect("a program job on a seq space claims");
            let reads = later.reads();
            for (channel, stored) in rows.iter().chain([&actual, ch.data()]).zip(expected) {
                assert_eq!(
                    reads.admits(channel, ClaimRole::Data),
                    !stored,
                    "{channel:?}"
                );
            }
        }
    }
}
