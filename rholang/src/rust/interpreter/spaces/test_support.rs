//! Added by decision 102 (plan v4 C5): the support that the unit tests of the native spaces
//! share. [`ScriptedReplay`] answers as native replay from a script over a transaction of play.
//! So a unit test stages the picks of replay (rules CR-4 and CR-7) and the record end (rules
//! CR-6 and CR-32) in a play store, and the commit of play applies the staging. The tests of
//! the generic space (plan v4 C4) and of each discipline (plan v4 C5) use it.

use std::collections::{BTreeMap, BTreeSet};
// Added by decision 124 (plan v4 C7.11, ruling RADD2v2 7.4): the capture sends each staged
// row search to its test.
use std::sync::mpsc::Sender;
use std::sync::Arc;

use async_trait::async_trait;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::r#match::Match;
// Added by decision 124 (rules CR-4 and R13): the identity that the lists of a refusal name.
use rspace_plus_plus::rspace::replay_rspace::native_epoch::NativeCandidateIdentity;
use rspace_plus_plus::rspace::rspace_interface::{RSpaceAccountingObserver, RSpaceOperationSource};
use rspace_plus_plus::rspace::space_store_handle::{
    ActualFire, FinishRow, RowClaims, RowDeviation, RowItemRule, RowReissue, RowRun, RowSearch,
    SpaceConsume, SpaceMode, SpaceProduce, SpaceTxn, SpaceTxnError, StagedOp, TailRead, TxnHold,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};

use super::agent::{ParSpaceReceipt, ParSpaceTxn, SpaceEnv};
use super::generic::{ParCandidate, ParDatum, ParRowLists, ParRowRequest, ParRowSearch, ParWaiter};
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::owner_funding::{OwnerFundingBinding, OwnerTable};
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::reified::system_processes::harness::native_config;

/// Added by decision 102 (plan v4 C5.3): the operation of a per-source row of a scripted
/// replay.
pub(crate) enum ScriptedSource {
    Produce(Produce),
    Consume(Consume),
}

impl ScriptedSource {
    /// True when `source` is the operation of the row.
    fn is(&self, source: RSpaceOperationSource<'_>) -> bool {
        match (self, source) {
            (ScriptedSource::Produce(row), RSpaceOperationSource::Produce(produce)) => {
                row == produce
            }
            (ScriptedSource::Consume(row), RSpaceOperationSource::Consume(consume)) => {
                row == consume
            }
            (ScriptedSource::Produce(_), RSpaceOperationSource::Consume(_)) => false,
            (ScriptedSource::Consume(_), RSpaceOperationSource::Produce(_)) => false,
        }
    }
}

/// Added by decision 102 (plan v4 C5.3): a per-source row of a scripted replay: the recorded
/// COMMs `comms` of the operation `source`.
pub(crate) struct ScriptedRow {
    pub(crate) source: ScriptedSource,
    pub(crate) comms: Option<Vec<COMM>>,
}

/// A transaction of play that answers as native replay from a script. Its mode is native
/// replay. `recorded_comms` gives the `comms` of the first row of `rows` whose source is the
/// source of the call, and `comms` for every other source. Added by decision 124 (F2):
/// `recorded_row_index` gives the position of that row in `rows`, so `rows` lists the rows in
/// journal order. After `stages` stage calls, each stage call fails with `RecordEnded` (CR-6),
/// and `end_record` answers `follows` (CR-32). Every other call goes to the play transaction
/// `inner`, so the staged view, the admission of a record and the commit are those of play.
pub(crate) struct ScriptedReplay {
    pub(crate) inner: ParSpaceTxn,
    pub(crate) comms: Option<Vec<COMM>>,
    pub(crate) stages: Option<usize>,
    pub(crate) follows: bool,
    /// Added by decision 102 (plan v4 C5.3): the per-source rows.
    pub(crate) rows: Vec<ScriptedRow>,
}

impl ScriptedReplay {
    /// The script of one stage call: `RecordEnded` after the record ended.
    fn stage_call(&mut self) -> Result<(), SpaceTxnError> {
        match self.stages.as_mut() {
            Some(0) => Err(SpaceTxnError::RecordEnded),
            Some(left) => {
                *left -= 1;
                Ok(())
            }
            None => Ok(()),
        }
    }
}

#[async_trait]
impl SpaceTxn<Par, BindPattern, ListParWithRandom, TaggedContinuation> for ScriptedReplay {
    fn mode(&self) -> SpaceMode { SpaceMode::NativeReplay }

    fn data(&mut self, channel: &Par) -> Result<&[ParDatum], SpaceTxnError> {
        self.inner.data(channel)
    }

    fn continuations(&mut self, channels: &[Par]) -> Result<&[ParWaiter], SpaceTxnError> {
        self.inner.continuations(channels)
    }

    fn joins(&mut self, channel: &Par) -> Result<&[Vec<Par>], SpaceTxnError> {
        self.inner.joins(channel)
    }

    fn produce_counters(
        &mut self,
        produces: &[Produce],
    ) -> Result<BTreeMap<Produce, i32>, SpaceTxnError> {
        self.inner.produce_counters(produces)
    }

    fn recorded_comms(&mut self, source: RSpaceOperationSource<'_>) -> Option<Vec<COMM>> {
        match self.rows.iter().find(|row| row.source.is(source)) {
            Some(row) => row.comms.clone(),
            None => self.comms.clone(),
        }
    }

    // Added by decision 124 (F2, base one row source for each produce, replay_rspace.rs:1058-1082):
    // the position of the first row of `rows` whose source is the source of the call. `rows`
    // lists the rows in journal order. A source without a row has no index.
    fn recorded_row_index(&mut self, source: RSpaceOperationSource<'_>) -> Option<usize> {
        self.rows.iter().position(|row| row.source.is(source))
    }

    fn admits(
        &mut self,
        comm: &COMM,
        datum: &ParDatum,
        incoming: bool,
    ) -> Result<bool, SpaceTxnError> {
        self.inner.admits(comm, datum, incoming)
    }

    fn produce(
        &mut self,
        op: SpaceProduce<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<StagedOp, SpaceTxnError> {
        self.stage_call()?;
        self.inner.produce(op)
    }

    fn consume(
        &mut self,
        op: SpaceConsume<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<StagedOp, SpaceTxnError> {
        self.stage_call()?;
        self.inner.consume(op)
    }

    async fn lock_phase_b(&mut self, scope_keys: &[Par]) -> Result<(), SpaceTxnError> {
        self.inner.lock_phase_b(scope_keys).await
    }

    fn set_footprint_key(&mut self, key: Par) -> Result<(), SpaceTxnError> {
        self.inner.set_footprint_key(key)
    }

    fn discard_staged(&mut self) -> Result<(), SpaceTxnError> { self.inner.discard_staged() }

    fn mark_leading_anchor(&mut self) { self.inner.mark_leading_anchor() }

    fn end_record(&mut self) -> bool { self.follows }

    async fn commit(&mut self) -> Result<ParSpaceReceipt, SpaceTxnError> {
        self.inner.commit().await
    }

    async fn commit_holding(&mut self) -> Result<(ParSpaceReceipt, TxnHold), SpaceTxnError> {
        self.inner.commit_holding().await
    }
}

/// A scripted replay over the play transaction `inner` that names `comms` for every source,
/// with no record end.
pub(crate) fn replaying(inner: ParSpaceTxn, comms: Option<Vec<COMM>>) -> ParSpaceTxn {
    Box::new(ScriptedReplay {
        inner,
        comms,
        stages: None,
        follows: false,
        // Added by decision 102 (plan v4 C5.3): no per-source rows.
        rows: Vec::new(),
    })
}

/// Added by decision 102 (plan v4 C5.3) and decision 124 (L1): a scripted replay over the play
/// transaction `inner` that answers from the per-source rows `rows`, names nothing for every
/// other source, and has no record end.
pub(crate) fn replaying_rows(inner: ParSpaceTxn, rows: Vec<ScriptedRow>) -> ParSpaceTxn {
    Box::new(ScriptedReplay {
        inner,
        comms: None,
        stages: None,
        follows: false,
        rows,
    })
}

/// A recorded COMM of the consume `consume` that binds the produces `produces`.
pub(crate) fn recorded(consume: Consume, produces: Vec<Produce>) -> COMM {
    COMM {
        consume,
        produces,
        peeks: BTreeSet::new(),
        times_repeated: BTreeMap::new(),
    }
}

/// `canonical` with `first` moved to its head.
pub(crate) fn named_first(first: usize, canonical: &[usize]) -> Vec<usize> {
    let mut order = Vec::<usize>::with_capacity(canonical.len());
    order.push(first);
    order.extend(canonical.iter().copied().filter(|index| *index != first));
    order
}

/// Added by decision 124 (ruling RADD2v4 6.1): the fields of one staged operation, without its
/// closure and its search, so a test compares what an operation stages.
#[derive(Debug, PartialEq)]
pub(crate) enum CapturedOp {
    Store {
        channel: Par,
        data: ListParWithRandom,
        persist: bool,
        outcome: String,
        own: bool,
    },
    Take {
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persist: bool,
        peeks: BTreeSet<i32>,
        outcome: String,
        own: bool,
    },
}

impl CapturedOp {
    /// The fields of a produce.
    pub(crate) fn of_store(
        op: &SpaceProduce<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Self {
        CapturedOp::Store {
            channel: op.channel.clone(),
            data: op.data.clone(),
            persist: op.persist,
            outcome: format!("{:?}", op.outcome),
            own: op.own,
        }
    }

    /// The fields of a consume.
    pub(crate) fn of_take(
        op: &SpaceConsume<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Self {
        CapturedOp::Take {
            channels: op.channels.clone(),
            patterns: op.patterns.clone(),
            continuation: op.continuation.clone(),
            persist: op.persist,
            peeks: op.peeks.clone(),
            outcome: format!("{:?}", op.outcome),
            own: op.own,
        }
    }
}

/// Added by decision 124 (plan v4 C7.11, ruling RADD2v2 7.4): a transaction of play that takes
/// the row search out of each operation that a stage call carries, sends it on `searches`, and
/// stages the operation without it. Every call goes to the play transaction `inner`, so the
/// staged view and the staging are those of play. A test drops the prepared work, so no taken
/// search runs at a commit, and the test drives each search by hand.
pub(crate) struct SearchCapture {
    pub(crate) inner: ParSpaceTxn,
    pub(crate) searches: Sender<ParRowSearch>,
    pub(crate) ops: Option<Sender<CapturedOp>>,
}

impl SearchCapture {
    /// Sends the search of one stage call, if it carries one.
    fn keep(&self, search: Option<ParRowSearch>) {
        let Some(search) = search else {
            return;
        };
        self.searches
            .send(search)
            .expect("the test keeps the receiver of the searches");
    }

    /// Sends the fields of one stage call, when the test asks for them.
    fn record(&self, fields: impl FnOnce() -> CapturedOp) {
        let Some(ops) = self.ops.as_ref() else {
            return;
        };
        ops.send(fields())
            .expect("the test keeps the receiver of the operations");
    }
}

#[async_trait]
impl SpaceTxn<Par, BindPattern, ListParWithRandom, TaggedContinuation> for SearchCapture {
    fn mode(&self) -> SpaceMode { self.inner.mode() }

    fn data(&mut self, channel: &Par) -> Result<&[ParDatum], SpaceTxnError> {
        self.inner.data(channel)
    }

    fn continuations(&mut self, channels: &[Par]) -> Result<&[ParWaiter], SpaceTxnError> {
        self.inner.continuations(channels)
    }

    fn joins(&mut self, channel: &Par) -> Result<&[Vec<Par>], SpaceTxnError> {
        self.inner.joins(channel)
    }

    fn produce_counters(
        &mut self,
        produces: &[Produce],
    ) -> Result<BTreeMap<Produce, i32>, SpaceTxnError> {
        self.inner.produce_counters(produces)
    }

    fn recorded_comms(&mut self, source: RSpaceOperationSource<'_>) -> Option<Vec<COMM>> {
        self.inner.recorded_comms(source)
    }

    fn recorded_row_index(&mut self, source: RSpaceOperationSource<'_>) -> Option<usize> {
        self.inner.recorded_row_index(source)
    }

    fn admits(
        &mut self,
        comm: &COMM,
        datum: &ParDatum,
        incoming: bool,
    ) -> Result<bool, SpaceTxnError> {
        self.inner.admits(comm, datum, incoming)
    }

    fn produce(
        &mut self,
        mut op: SpaceProduce<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<StagedOp, SpaceTxnError> {
        self.record(|| CapturedOp::of_store(&op));
        self.keep(op.search.take());
        self.inner.produce(op)
    }

    fn consume(
        &mut self,
        mut op: SpaceConsume<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<StagedOp, SpaceTxnError> {
        self.record(|| CapturedOp::of_take(&op));
        self.keep(op.search.take());
        self.inner.consume(op)
    }

    async fn lock_phase_b(&mut self, scope_keys: &[Par]) -> Result<(), SpaceTxnError> {
        self.inner.lock_phase_b(scope_keys).await
    }

    fn set_footprint_key(&mut self, key: Par) -> Result<(), SpaceTxnError> {
        self.inner.set_footprint_key(key)
    }

    fn discard_staged(&mut self) -> Result<(), SpaceTxnError> { self.inner.discard_staged() }

    fn mark_leading_anchor(&mut self) { self.inner.mark_leading_anchor() }

    fn end_record(&mut self) -> bool { self.inner.end_record() }

    async fn commit(&mut self) -> Result<ParSpaceReceipt, SpaceTxnError> {
        self.inner.commit().await
    }

    async fn commit_holding(&mut self) -> Result<(ParSpaceReceipt, TxnHold), SpaceTxnError> {
        self.inner.commit_holding().await
    }
}

/// Added by decision 124 (plan v4 C7.11, ruling RADD2v2 7.4): a [`SearchCapture`] over the play
/// transaction `inner` that sends the searches on `searches`.
pub(crate) fn capturing(inner: ParSpaceTxn, searches: Sender<ParRowSearch>) -> ParSpaceTxn {
    Box::new(SearchCapture {
        inner,
        searches,
        ops: None,
    })
}

/// Added by decision 124 (ruling RADD2v4 6.1): a [`SearchCapture`] over the play transaction
/// `inner` that also sends the fields of each staged operation on `ops`.
pub(crate) fn capturing_ops(
    inner: ParSpaceTxn,
    searches: Sender<ParRowSearch>,
    ops: Sender<CapturedOp>,
) -> ParSpaceTxn {
    Box::new(SearchCapture {
        inner,
        searches,
        ops: Some(ops),
    })
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3, and plan v4 C7.19, rule
/// CR-24, ruling RADD2v4 7.9): a transaction of play whose commit deviates at row `index`. The
/// store commits every row. The receipt then holds rows 0 to `index`, names row `index` as row
/// d with no search and no item pair, and carries `claims`, the claims of the actual unit of
/// row d. So the finish reads Tail(actual) from the class of row d (C2 E4.5), and the finish
/// of `run_plan` chooses the rows and the claims that it stores. Every other call goes to the
/// play transaction `inner`.
pub(crate) struct DeviatingCommit {
    pub(crate) inner: ParSpaceTxn,
    pub(crate) index: usize,
    pub(crate) claims: Option<RowClaims<Par>>,
}

impl DeviatingCommit {
    /// Cuts `receipt` at row `index`.
    fn deviate(&self, receipt: &mut ParSpaceReceipt) {
        receipt.ops.truncate(self.index + 1);
        receipt.deviation = Some(RowDeviation {
            index: self.index,
            item_failed: Vec::new(),
            dispatched: None,
            vetoing: false,
            tail_read: TailRead::Ends,
            claims: self.claims.clone(),
            reissue: None,
        });
    }
}

#[async_trait]
impl SpaceTxn<Par, BindPattern, ListParWithRandom, TaggedContinuation> for DeviatingCommit {
    fn mode(&self) -> SpaceMode { self.inner.mode() }

    fn data(&mut self, channel: &Par) -> Result<&[ParDatum], SpaceTxnError> {
        self.inner.data(channel)
    }

    fn continuations(&mut self, channels: &[Par]) -> Result<&[ParWaiter], SpaceTxnError> {
        self.inner.continuations(channels)
    }

    fn joins(&mut self, channel: &Par) -> Result<&[Vec<Par>], SpaceTxnError> {
        self.inner.joins(channel)
    }

    fn produce_counters(
        &mut self,
        produces: &[Produce],
    ) -> Result<BTreeMap<Produce, i32>, SpaceTxnError> {
        self.inner.produce_counters(produces)
    }

    fn recorded_comms(&mut self, source: RSpaceOperationSource<'_>) -> Option<Vec<COMM>> {
        self.inner.recorded_comms(source)
    }

    fn recorded_row_index(&mut self, source: RSpaceOperationSource<'_>) -> Option<usize> {
        self.inner.recorded_row_index(source)
    }

    fn admits(
        &mut self,
        comm: &COMM,
        datum: &ParDatum,
        incoming: bool,
    ) -> Result<bool, SpaceTxnError> {
        self.inner.admits(comm, datum, incoming)
    }

    fn produce(
        &mut self,
        op: SpaceProduce<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<StagedOp, SpaceTxnError> {
        self.inner.produce(op)
    }

    fn consume(
        &mut self,
        op: SpaceConsume<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<StagedOp, SpaceTxnError> {
        self.inner.consume(op)
    }

    async fn lock_phase_b(&mut self, scope_keys: &[Par]) -> Result<(), SpaceTxnError> {
        self.inner.lock_phase_b(scope_keys).await
    }

    fn set_footprint_key(&mut self, key: Par) -> Result<(), SpaceTxnError> {
        self.inner.set_footprint_key(key)
    }

    fn discard_staged(&mut self) -> Result<(), SpaceTxnError> { self.inner.discard_staged() }

    fn mark_leading_anchor(&mut self) { self.inner.mark_leading_anchor() }

    fn end_record(&mut self) -> bool { self.inner.end_record() }

    async fn commit(&mut self) -> Result<ParSpaceReceipt, SpaceTxnError> {
        let mut receipt = self.inner.commit().await?;
        self.deviate(&mut receipt);
        Ok(receipt)
    }

    async fn commit_holding(&mut self) -> Result<(ParSpaceReceipt, TxnHold), SpaceTxnError> {
        let (mut receipt, hold) = self.inner.commit_holding().await?;
        self.deviate(&mut receipt);
        Ok((receipt, hold))
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 7.4, and ruling RADD2v4
/// 7.10): `env` with a budget in a native run with owner funding (R11), so each plan over it
/// is owner-funded (`Introduction::owner_funded`) and carries its row search. The binding
/// lists no client lane and no owner.
pub(crate) fn owner_funded(env: &SpaceEnv) -> SpaceEnv {
    let binding = OwnerFundingBinding::new(BTreeSet::new(), |_| None, OwnerTable::empty(0));
    let budget = RuntimeBudget::new(Cost::create(10_000_000, "owner-funded test"));
    budget
        .reset_for_native_execution(native_config().with_owner_funding(Arc::new(binding)))
        .expect("an idle budget starts a native run with owner funding");
    SpaceEnv {
        budget,
        ..env.clone()
    }
}

/// Added by decision 124 (plan v4 C7.9 and C7.15, rule L16, ruling OP-LISTS, and ruling
/// RADD2v4 7.10): the observer of the commit tests of a row search. It charges nothing and
/// turns on the owner checks of a commit, so a staged row with a search runs the search of
/// play over the real lists of the commit. The constructor of those lists stays in the base
/// crate. Its item test answers with `admit`.
pub(crate) struct Funded {
    admit: Box<dyn Fn(&TaggedContinuation, &ListParWithRandom) -> bool + Send + Sync>,
}

impl Funded {
    /// An observer that admits every item.
    pub(crate) fn admitting() -> Self { Self::with_item_test(|_, _| true) }

    /// An observer whose item test is `admit`: true when the waiter `continuation` may take
    /// the value `datum`.
    pub(crate) fn with_item_test(
        admit: impl Fn(&TaggedContinuation, &ListParWithRandom) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            admit: Box::new(admit),
        }
    }
}

impl RSpaceAccountingObserver<Par, BindPattern, ListParWithRandom, TaggedContinuation> for Funded {
    fn observe_produce(
        &self,
        _source: &Produce,
        _channel: &Par,
        _data: &ListParWithRandom,
        _persistent: bool,
    ) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn observe_consume(
        &self,
        _source: &Consume,
        _channels: &[Par],
        _patterns: &[BindPattern],
        _continuation: &TaggedContinuation,
        _persistent: bool,
        _peeks: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn observe_comm(
        &self,
        _comm: &COMM,
        _continuation: &TaggedContinuation,
        _continuation_persistent: bool,
        _data: &[(&ListParWithRandom, bool)],
    ) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn owner_checks_active(&self) -> bool { true }

    fn admit_item(
        &self,
        continuation: &TaggedContinuation,
        _continuation_persistent: bool,
        _channel_count: usize,
        datum: &ListParWithRandom,
        _datum_persistent: bool,
    ) -> Result<bool, RSpaceError> {
        Ok((self.admit)(continuation, datum))
    }
}

/// Added by decision 124 (rules CR-4 and R13): a recorded refusal that names the records
/// `named`, each with repetition 0, as the search of the refusal reads it through its lists.
pub(crate) struct NamedRecords {
    pub(crate) named: Vec<Produce>,
}

impl NativeCandidateIdentity for NamedRecords {
    fn matches_consume(&self, _source: &Consume) -> bool { true }

    fn matches_produce(&self, source: &Produce) -> bool { self.named.contains(source) }

    fn repetition(&self, _source: &Produce) -> Option<i32> { Some(0) }

    fn matches_comm(&self, _source: &COMM) -> bool { true }
}

/// Added by decision 124 (rules CR-4 and R13): the answer of `run` over lists that read
/// `records` on every channel, name the records of `identity`, and count 0 for each source.
pub(crate) fn with_named_lists<T>(
    records: &[ParDatum],
    identity: &dyn NativeCandidateIdentity,
    run: impl FnOnce(&ParRowLists<'_>) -> T,
) -> T {
    let data = |_channel: &Par| -> Result<Vec<ParDatum>, SpaceTxnError> { Ok(records.to_vec()) };
    let continuations =
        |_channels: &[Par]| -> Result<Vec<Arc<ParWaiter>>, SpaceTxnError> { Ok(Vec::new()) };
    let joins = |_channel: &Par| -> Result<Vec<Vec<Par>>, SpaceTxnError> { Ok(Vec::new()) };
    let counter = |_produce: &Produce| 0;
    let lists = ParRowLists::new(&data, &continuations, &joins, Some(identity), &counter);
    run(&lists)
}

/// Added by decision 124 (plan v4 C7.9 and C7.15, ruling OP-LISTS, and ruling RADD2v4 7.10): a
/// row search that hands the lists of the commit to `probe` once, at its group step, and
/// otherwise answers as `inner`.
pub(crate) struct ListsProbe<S> {
    inner: S,
    probe: Option<Box<dyn FnOnce(&ParRowLists<'_>) + Send>>,
}

impl<S> ListsProbe<S> {
    pub(crate) fn new(inner: S, probe: impl FnOnce(&ParRowLists<'_>) + Send + 'static) -> Self {
        Self {
            inner,
            probe: Some(Box::new(probe)),
        }
    }
}

impl<S> RowSearch<Par, BindPattern, ListParWithRandom, TaggedContinuation> for ListsProbe<S>
where S: RowSearch<Par, BindPattern, ListParWithRandom, TaggedContinuation>
{
    fn runs(&self) -> Vec<RowRun<Par>> { self.inner.runs() }

    fn continuations(
        &mut self,
        run: usize,
        lists: &ParRowLists<'_>,
    ) -> Result<Vec<(Arc<ParWaiter>, i32)>, SpaceTxnError> {
        match self.probe.take() {
            None => {}
            Some(probe) => probe(lists),
        }
        self.inner.continuations(run, lists)
    }

    fn tuple(
        &mut self,
        run: usize,
        lists: &ParRowLists<'_>,
        request: ParRowRequest<'_>,
    ) -> Result<Option<Vec<ParCandidate>>, SpaceTxnError> {
        self.inner.tuple(run, lists, request)
    }

    fn restore(&mut self) { self.inner.restore() }

    fn commit_check(
        &mut self,
        request: &ParRowRequest<'_>,
        tuple: &[ParCandidate],
    ) -> Result<bool, SpaceTxnError> {
        self.inner.commit_check(request, tuple)
    }

    fn may_veto(
        &self,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        continuation: &TaggedContinuation,
    ) -> bool {
        self.inner.may_veto(matcher, continuation)
    }

    fn staged_retires(&self) -> usize { self.inner.staged_retires() }

    fn actual_fire(
        &mut self,
        request: &ParRowRequest<'_>,
        tuple: &[ParCandidate],
    ) -> ActualFire<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
        self.inner.actual_fire(request, tuple)
    }

    fn item_failures(
        &self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
    ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceTxnError> {
        self.inner.item_failures(lists, matcher, rule)
    }

    fn tail_read(
        &mut self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<TailRead, SpaceTxnError> {
        self.inner.tail_read(lists, matcher)
    }

    fn actual_claims(&self) -> Option<RowClaims<Par>> { self.inner.actual_claims() }

    fn tail_reissue(&mut self) -> Option<RowReissue> { self.inner.tail_reissue() }

    fn finish_rows(
        &mut self,
    ) -> Vec<FinishRow<Par, BindPattern, ListParWithRandom, TaggedContinuation>> {
        self.inner.finish_rows()
    }

    fn keeps_staged_record(&self) -> bool { self.inner.keeps_staged_record() }

    fn store_unit(
        &mut self,
    ) -> Vec<FinishRow<Par, BindPattern, ListParWithRandom, TaggedContinuation>> {
        self.inner.store_unit()
    }
}
