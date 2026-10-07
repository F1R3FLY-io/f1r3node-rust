//! An in-memory agent environment over play and replay (P5 design v3, A.3.2).
//!
//! An agent test runs space operations with an agent directly, without a Rholang runtime: it
//! creates a space with `prepare_create`, runs each operation with `run_op`, and reads the
//! records, the waiters and the counters of a collection from the host store. The helpers are
//! the ones of the agent unit tests in `spaces/generic.rs`, over the public interface.
//!
//! The join suites of P5c (`reified_joins.rs`, `reified_join_props.rs`) add the helpers of the
//! section "Joins". These are the builders of a join, the canonical group of its binds, and the
//! readers of the join waiters and of the join groups. A [`RecordingHost`] records the
//! transactions of an agent, and a [`CountingMatcher`] counts the guard checks of the matcher.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, Expr, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits, HostWorkUsages};
use models::rust::space_factory_urn::SpaceFactoryUrn;
use models::rust::utils::{new_freevar_par, new_freevar_var, new_gint_par, new_gstring_par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::costs::ScanMeter;
use rholang::rust::interpreter::reified::legacy_charges::{ParSpaceConsume, ParSpaceProduce};
use rholang::rust::interpreter::spaces::agent::{
    run_op, ConsumeOp, JoinBind, JoinOp, ProduceOp, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOp,
    SpaceOutcome,
};
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::namespace::{self, CollectionKey, NativeSpace};
use rholang::rust::interpreter::spaces::record::{self, JoinSlot};
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{ISpace, RSpaceOperationSource};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{
    BoxedSpaceTxn, ProduceOutcome, SpaceConsume, SpaceMode, SpaceProduce, SpaceStoreHandle,
    SpaceTxn, SpaceTxnError, SpaceTxnHost, SpaceTxnReceipt, StagedOp, SweepReport, TxnRegistration,
};
use rspace_plus_plus::rspace::trace::event::{Produce, COMM};

/// The play space of an agent test.
pub type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// The replay space of an agent test.
pub type ParReplay = ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// The phlo budget of an agent test.
pub fn budget() -> RuntimeBudget {
    RuntimeBudget::new(Cost::create(1_000_000, "rrs p5 agent test"))
}

/// A host-work budget that accepts every charge and records the usage.
pub fn unbounded_host_work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)))
}

/// A play space over a fresh in-memory store.
pub async fn play() -> ParRSpace {
    let mut stores = InMemoryStoreManager::new();
    ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("an in-memory space")
}

/// A play space and its replay space over one fresh in-memory store.
pub async fn play_and_replay() -> (ParRSpace, ParReplay) {
    let mut stores = InMemoryStoreManager::new();
    RSpace::create_with_replay(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("an in-memory play and replay space")
}

/// The environment of a job over `host` with the phlo budget `budget` and no host-work budget.
pub fn env_over<H>(host: H, budget: &RuntimeBudget) -> SpaceEnv
where H: SpaceTxnHost<Par, BindPattern, ListParWithRandom, TaggedContinuation> + 'static {
    SpaceEnv {
        store: SpaceStoreHandle::new(host),
        budget: budget.clone(),
        meter: ScanMeter::new(None),
    }
}

/// The environment of a job over `host` whose meter charges `host_work`.
pub fn metered_env<H>(host: H, budget: &RuntimeBudget, host_work: &HostWorkBudget) -> SpaceEnv
where H: SpaceTxnHost<Par, BindPattern, ListParWithRandom, TaggedContinuation> + 'static {
    SpaceEnv {
        store: SpaceStoreHandle::new(host),
        budget: budget.clone(),
        meter: ScanMeter::new(Some(host_work.clone())),
    }
}

/// The config map of `space` for the family `outer:inner` with the qualifier `qualifier`.
pub fn config_of(space: NativeSpace, outer: &str, inner: &str, qualifier: SpaceQualifier) -> Par {
    SpaceConfig::new(space.sid, outer, inner, qualifier).to_par()
}

/// A continuation whose body is the string `name`, with a random state of its own.
pub fn named(name: &str, guard: Option<Par>) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(string(name)),
            random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
        })),
        guard,
        cost_authority: None,
    }
}

/// The creation of `space` with the config map `config` by the factory of `family`.
pub fn creation(space: NativeSpace, config: Par, family: SpaceFactoryUrn) -> SpaceCreation {
    SpaceCreation {
        space,
        config,
        handle_pattern: BindPattern {
            patterns: Vec::new(),
            remainder: Some(new_freevar_var(0)),
            free_count: 1,
        },
        handle_continuation: named("handle", None),
        random: Blake2b512Random::create_from_bytes(b"rrs p5 factory call"),
        authority: CostAuthority::default(),
        family,
    }
}

/// Creates a space with `agent` and commits it.
pub async fn create(agent: &dyn SpaceAgent, env: &SpaceEnv, creation: SpaceCreation) {
    agent
        .prepare_create(env, creation)
        .await
        .expect("the creation stages")
        .commit()
        .await
        .expect("the creation commits");
}

/// The integer `value` as a `Par`.
pub fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

/// The string `value` as a `Par`.
pub fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

/// The collection key of `channel`.
pub fn key_of(channel: &Par) -> CollectionKey {
    CollectionKey::of_channel(channel).expect("a channel has a collection key")
}

/// The data channel `D(sid,k)` of the collection of `channel` in a HashMap-keyed space.
pub fn data_channel(space: NativeSpace, channel: &Par) -> Par {
    namespace::gprivate(namespace::data_channel(space, &key_of(channel)))
}

/// The counter channel `S(sid,k)` of the collection of `channel` in a HashMap-keyed space.
pub fn counter_channel(space: NativeSpace, channel: &Par) -> Par {
    namespace::gprivate(namespace::collection_counter(space, &key_of(channel)))
}

/// The space counter `S(sid)`.
pub fn space_counter_channel(space: NativeSpace) -> Par {
    namespace::gprivate(namespace::space_counter(space))
}

/// A send of `value` on `channel` of `space`.
pub fn send_on(space: NativeSpace, channel: &Par, value: i64, persistent: bool) -> ProduceOp {
    ProduceOp {
        space,
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

/// The pattern that binds any one value.
pub fn any() -> BindPattern {
    BindPattern {
        patterns: vec![new_freevar_par(0, Vec::new())],
        remainder: None,
        free_count: 1,
    }
}

/// The pattern that matches exactly the integer `value`.
pub fn exactly(value: i64) -> BindPattern {
    BindPattern {
        patterns: vec![int(value)],
        remainder: None,
        free_count: 0,
    }
}

/// A receive on `channel` of `space` whose body is the string `name`.
pub fn receive_on(
    space: NativeSpace,
    channel: &Par,
    name: &str,
    pattern: BindPattern,
) -> ConsumeOp {
    let body = match named(name, None).tagged_cont {
        Some(TaggedCont::ParBody(body)) => body,
        Some(TaggedCont::ScalaBodyRef(_)) | None => ParWithRandom::default(),
    };
    ConsumeOp {
        space,
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

/// Runs `op` with `agent` in one job and commits it.
pub async fn run(
    agent: &dyn SpaceAgent,
    env: &SpaceEnv,
    op: SpaceOp,
) -> Result<SpaceOutcome, SpaceError> {
    run_op(agent, env, op).await
}

/// The body name of a continuation that [`named`] made, or the empty string.
pub fn body_name(continuation: &TaggedContinuation) -> String {
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

/// The body name and the bound values of each fire of `outcome`.
pub fn fired(outcome: &SpaceOutcome) -> Vec<(String, Vec<Par>)> {
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

/// The records on the data channel `data` as `(ord, payload, persistent)`, by `ord`.
pub async fn records_on(space: &ParRSpace, data: &Par) -> Vec<(i64, Vec<Par>, bool)> {
    let mut records: Vec<_> = space
        .get_data(data)
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

/// The waiting continuations on the data channel `data` as `(ord, persistent, peek)`, by `ord`.
pub async fn waiters_on(space: &ParRSpace, data: &Par) -> Vec<(i64, bool, bool)> {
    let mut waiters: Vec<_> = space
        .get_waiting_continuations(vec![data.clone()])
        .await
        .into_iter()
        .map(|waiter| {
            let ord =
                record::continuation_ord(&waiter.continuation).expect("a continuation record");
            (ord, waiter.persist, !waiter.peeks.is_empty())
        })
        .collect();
    waiters.sort_by_key(|(ord, ..)| *ord);
    waiters
}

/// The values of the counter records on `channel`.
pub async fn counter(space: &ParRSpace, channel: &Par) -> Vec<i64> {
    space
        .get_data(channel)
        .await
        .iter()
        .map(|datum| record::counter_value(&datum.a).expect("a counter record"))
        .collect()
}

// ─── Joins (P5c) ─────────────────────────────────────────────────────────────────────────────

/// The body of the continuation that [`named`] makes for `name`.
pub fn body_of(name: &str) -> ParWithRandom {
    match named(name, None).tagged_cont {
        Some(TaggedCont::ParBody(body)) => body,
        Some(TaggedCont::ScalaBodyRef(_)) | None => ParWithRandom::default(),
    }
}

/// A bind of a join on `channel` with the pattern `pattern` and no query.
pub fn join_bind(channel: &Par, pattern: BindPattern) -> JoinBind {
    JoinBind {
        key: key_of(channel),
        channel: channel.clone(),
        pattern,
        query: None,
    }
}

/// A join on `space` with the binds `binds`, in bind order, whose body is the string `name`. Every
/// bind has the arrow `<-`, and the join has no guard.
pub fn join_on(space: NativeSpace, binds: &[(&Par, BindPattern)], name: &str) -> JoinOp {
    JoinOp {
        space,
        binds: binds
            .iter()
            .map(|(channel, pattern)| join_bind(channel, pattern.clone()))
            .collect(),
        body: body_of(name),
        persistent: false,
        peek: false,
        guard: None,
        authority: CostAuthority::default(),
        introduction_authority: CostAuthority::default(),
        reissued_ords: None,
    }
}

/// The bind positions of a join on `space` whose binds name the inner channels `channels`, in
/// canonical group order (FR-1'). The order ascends by the stable hash of the data channel of
/// each bind, then by the bind index.
pub fn group_positions(space: NativeSpace, channels: &[&Par]) -> Vec<usize> {
    let mut keyed: Vec<(Blake2b256Hash, usize)> = channels
        .iter()
        .enumerate()
        .map(|(index, channel)| {
            (
                stable_hash_provider::hash(&data_channel(space, channel)),
                index,
            )
        })
        .collect();
    keyed.sort();
    keyed.into_iter().map(|(_, index)| index).collect()
}

/// The canonical group of a join on `space` whose binds name the inner channels `channels`: the
/// data channel of each bind, in group order.
pub fn join_group(space: NativeSpace, channels: &[&Par]) -> Vec<Par> {
    group_positions(space, channels)
        .into_iter()
        .map(|index| data_channel(space, channels[index]))
        .collect()
}

/// A record of any discipline as a test reads it: `(ord, level, payload, persistent)`. The level
/// is `None` outside a priority queue and for the unprioritized level of a priority queue.
pub type AnyRecord = (i64, Option<i64>, Vec<Par>, bool);

/// The records on the data channel `data`, of any discipline, by `ord`, then by payload.
pub async fn any_records_on(space: &ParRSpace, data: &Par) -> Vec<AnyRecord> {
    let mut records: Vec<AnyRecord> = space
        .get_data(data)
        .await
        .into_iter()
        .map(|datum| {
            let decoded = record::unwrap_pq_datum(&datum.a).expect("a datum record");
            (
                decoded.ord,
                decoded.priority,
                decoded.datum.pars,
                datum.persist,
            )
        })
        .collect();
    records.sort_by(|left, right| {
        (left.0, format!("{:?}", left.2)).cmp(&(right.0, format!("{:?}", right.2)))
    });
    records
}

/// The values of the records on `data`, of any discipline, when each payload is one number,
/// sorted.
pub async fn values_on(space: &ParRSpace, data: &Par) -> Vec<i64> {
    let mut values: Vec<i64> = any_records_on(space, data)
        .await
        .into_iter()
        .map(|(_, _, payload, _)| match payload.as_slice() {
            [Par { exprs, .. }] => match exprs.as_slice() {
                [Expr {
                    expr_instance: Some(ExprInstance::GInt(value)),
                }] => *value,
                other => panic!("a payload of one number: {other:?}"),
            },
            other => panic!("a payload of one number: {other:?}"),
        })
        .collect();
    values.sort();
    values
}

/// A single-bind waiter of any discipline as a test reads it: `(ord, lane, body name,
/// persistent, peek)`. The lane is `None` outside a priority queue and for the unrestricted lane.
pub type AnyWaiter = (i64, Option<i64>, String, bool, bool);

/// The single-bind waiters on the data channel `data`, of any discipline, by `ord`.
pub async fn any_waiters_on(space: &ParRSpace, data: &Par) -> Vec<AnyWaiter> {
    let mut waiters: Vec<AnyWaiter> = space
        .get_waiting_continuations(vec![data.clone()])
        .await
        .into_iter()
        .map(|waiter| {
            let parked = record::unwrap_pq_continuation(&waiter.continuation)
                .expect("a continuation record");
            (
                parked.ord,
                parked.priority,
                body_name(&parked.continuation),
                waiter.persist,
                !waiter.peeks.is_empty(),
            )
        })
        .collect();
    waiters.sort_by(|left, right| (left.0, &left.2).cmp(&(right.0, &right.2)));
    waiters
}

/// A join waiter as a test reads it. It has the slots of its group positions in group order, the
/// body name of its continuation, and its persist and peek flags.
#[derive(Clone, Debug, PartialEq)]
pub struct JoinWaiterRow {
    /// The slots of the group positions, in group order.
    pub slots: Vec<JoinSlot>,
    /// The body name of the continuation.
    pub name: String,
    /// True for a persistent waiter.
    pub persist: bool,
    /// True for a waiter that peeks.
    pub peek: bool,
}

/// The join waiters parked on the group `group`, each decoded from its record of version 2, in
/// the list order of the store.
pub async fn join_waiters_on(space: &ParRSpace, group: &[Par]) -> Vec<JoinWaiterRow> {
    space
        .get_waiting_continuations(group.to_vec())
        .await
        .into_iter()
        .map(|waiter| {
            let decoded = record::unwrap_join_continuation(&waiter.continuation)
                .expect("a join continuation record");
            JoinWaiterRow {
                slots: decoded.slots,
                name: body_name(&decoded.continuation),
                persist: waiter.persist,
                peek: !waiter.peeks.is_empty(),
            }
        })
        .collect()
}

/// The join groups of the data channel `data`, in the list order of the store.
pub async fn joins_on(space: &ParRSpace, data: &Par) -> Vec<Vec<Par>> {
    space.get_joins(data.clone()).await
}

// ─── A recording host ────────────────────────────────────────────────────────────────────────

/// A staged operation that a [`RecordingHost`] saw: a copy of the staged produce or consume,
/// without its callback.
pub enum StagedCopy {
    /// A staged produce.
    Produce(Box<ParSpaceProduce>),
    /// A staged consume.
    Consume(Box<ParSpaceConsume>),
}

impl StagedCopy {
    /// The channels of the staged operation. A produce gives its channel, then the channels of the
    /// group of its fire. A consume gives its channels.
    pub fn channels(&self) -> Vec<Par> {
        match self {
            StagedCopy::Produce(op) => {
                let mut channels = vec![op.channel.clone()];
                if let ProduceOutcome::Fire {
                    channels: group, ..
                } = &op.outcome
                {
                    channels.extend(group.iter().cloned());
                }
                channels
            }
            StagedCopy::Consume(op) => op.channels.clone(),
        }
    }
}

/// A point of the timeline of a metered [`RecordingHost`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordPoint {
    /// The begin of the transaction with this index.
    Begin(usize),
    /// The staged operation with this index in [`HostRecord::staged`], before the host stages it.
    Staged(usize),
    /// The commit of the transaction with this index, before the host commits it.
    Commit(usize),
}

/// What a [`RecordingHost`] saw, in order.
#[derive(Default)]
pub struct HostRecord {
    /// The lock keys of each transaction, in the order of the begins.
    pub begins: Vec<Vec<Par>>,
    /// Each staged operation with the index of its transaction, in staging order.
    pub staged: Vec<(usize, StagedCopy)>,
    /// The index of each transaction that committed, in commit order.
    pub commits: Vec<usize>,
    /// The host-work usage of the agent at each begin, staged operation and commit, in order.
    /// Only a metered host ([`RecordingHost::metered`]) fills it.
    pub usages: Vec<(RecordPoint, HostWorkUsages)>,
}

/// The shared record of a [`RecordingHost`].
pub type SharedRecord = Arc<Mutex<HostRecord>>;

/// A host that records the transactions of an agent: the lock keys of each `begin`, a copy of
/// each staged operation, and each commit. It forwards every call to the host `inner`, so the
/// agent behaves as over `inner`.
pub struct RecordingHost<H> {
    inner: H,
    record: SharedRecord,
    host_work: Option<HostWorkBudget>,
}

impl<H> RecordingHost<H> {
    /// A recording host over `inner`, and its record.
    pub fn new(inner: H) -> (Self, SharedRecord) {
        let record: SharedRecord = Arc::new(Mutex::new(HostRecord::default()));
        (
            Self {
                inner,
                record: Arc::clone(&record),
                host_work: None,
            },
            record,
        )
    }

    /// A recording host over `inner` that also records the usage of `host_work`, the host-work
    /// budget of the agent, at each begin, staged operation and commit.
    pub fn metered(inner: H, host_work: &HostWorkBudget) -> (Self, SharedRecord) {
        let (host, record) = Self::new(inner);
        (
            Self {
                host_work: Some(host_work.clone()),
                ..host
            },
            record,
        )
    }
}

/// Records the usage of `host_work` at `point`, when the host is metered.
fn note_usage(record: &mut HostRecord, host_work: Option<&HostWorkBudget>, point: RecordPoint) {
    if let Some(host_work) = host_work {
        record.usages.push((point, host_work.usages()));
    }
}

/// A copy of the staged produce `op` without its callback.
fn copy_produce(op: &ParSpaceProduce) -> ParSpaceProduce {
    SpaceProduce {
        channel: op.channel.clone(),
        data: op.data.clone(),
        persist: op.persist,
        outcome: op.outcome.clone(),
        before_observe: None,
    }
}

/// A copy of the staged consume `op` without its callback.
fn copy_consume(op: &ParSpaceConsume) -> ParSpaceConsume {
    SpaceConsume {
        channels: op.channels.clone(),
        patterns: op.patterns.clone(),
        continuation: op.continuation.clone(),
        persist: op.persist,
        peeks: op.peeks.clone(),
        outcome: op.outcome.clone(),
        before_observe: None,
    }
}

#[async_trait]
impl<H> SpaceTxnHost<Par, BindPattern, ListParWithRandom, TaggedContinuation> for RecordingHost<H>
where H: SpaceTxnHost<Par, BindPattern, ListParWithRandom, TaggedContinuation>
{
    fn mode(&self) -> SpaceMode { self.inner.mode() }

    fn matcher(&self) -> Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>> {
        self.inner.matcher()
    }

    async fn begin(
        &self,
        lock_keys: &[Par],
        registration: TxnRegistration,
    ) -> Result<BoxedSpaceTxn<Par, BindPattern, ListParWithRandom, TaggedContinuation>, SpaceTxnError>
    {
        let inner = self.inner.begin(lock_keys, registration).await?;
        let index = {
            let mut record = self.record.lock().expect("the record lock");
            record.begins.push(lock_keys.to_vec());
            let index = record.begins.len() - 1;
            note_usage(
                &mut record,
                self.host_work.as_ref(),
                RecordPoint::Begin(index),
            );
            index
        };
        Ok(Box::new(RecordingTxn {
            inner,
            record: Arc::clone(&self.record),
            index,
            host_work: self.host_work.clone(),
        }))
    }

    fn root_leaf_record_count(&self, channel: &Par) -> Result<u64, SpaceTxnError> {
        self.inner.root_leaf_record_count(channel)
    }

    async fn sweep_volatile(
        &self,
        channels: &[Par],
        registration: &TxnRegistration,
    ) -> Result<SweepReport, SpaceTxnError> {
        self.inner.sweep_volatile(channels, registration).await
    }
}

/// A transaction of a [`RecordingHost`]: it forwards every call and records each staged
/// operation and the commit.
struct RecordingTxn {
    inner: BoxedSpaceTxn<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    record: SharedRecord,
    index: usize,
    host_work: Option<HostWorkBudget>,
}

impl RecordingTxn {
    /// The usage of the agent before the host stages an operation, when the host is metered.
    fn usage(&self) -> Option<HostWorkUsages> {
        self.host_work.as_ref().map(HostWorkBudget::usages)
    }

    /// Records the copy `copy` of a staged operation, with the usage `usage` that the agent had
    /// before the host staged it.
    fn note_staged(&self, copy: StagedCopy, usage: Option<HostWorkUsages>) {
        let mut record = self.record.lock().expect("the record lock");
        let position = record.staged.len();
        if let Some(usage) = usage {
            record.usages.push((RecordPoint::Staged(position), usage));
        }
        record.staged.push((self.index, copy));
    }
}

#[async_trait]
impl SpaceTxn<Par, BindPattern, ListParWithRandom, TaggedContinuation> for RecordingTxn {
    fn mode(&self) -> SpaceMode { self.inner.mode() }

    fn data(&mut self, channel: &Par) -> Result<&[Datum<ListParWithRandom>], SpaceTxnError> {
        self.inner.data(channel)
    }

    fn continuations(
        &mut self,
        channels: &[Par],
    ) -> Result<&[WaitingContinuation<BindPattern, TaggedContinuation>], SpaceTxnError> {
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

    fn admits(
        &mut self,
        comm: &COMM,
        datum: &Datum<ListParWithRandom>,
        incoming: bool,
    ) -> Result<bool, SpaceTxnError> {
        self.inner.admits(comm, datum, incoming)
    }

    fn produce(
        &mut self,
        op: SpaceProduce<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<StagedOp, SpaceTxnError> {
        let copy = copy_produce(&op);
        let usage = self.usage();
        let staged = self.inner.produce(op)?;
        self.note_staged(StagedCopy::Produce(Box::new(copy)), usage);
        Ok(staged)
    }

    fn consume(
        &mut self,
        op: SpaceConsume<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<StagedOp, SpaceTxnError> {
        let copy = copy_consume(&op);
        let usage = self.usage();
        let staged = self.inner.consume(op)?;
        self.note_staged(StagedCopy::Consume(Box::new(copy)), usage);
        Ok(staged)
    }

    async fn commit(
        &mut self,
    ) -> Result<
        SpaceTxnReceipt<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        SpaceTxnError,
    > {
        {
            let mut record = self.record.lock().expect("the record lock");
            note_usage(
                &mut record,
                self.host_work.as_ref(),
                RecordPoint::Commit(self.index),
            );
        }
        let receipt = self.inner.commit().await?;
        self.record
            .lock()
            .expect("the record lock")
            .commits
            .push(self.index);
        Ok(receipt)
    }
}

// ─── A counting matcher ──────────────────────────────────────────────────────────────────────

/// The matcher of a test that counts the guard checks. It is the Rholang matcher. Each call of
/// `check_commit` or `check_commit_metered` on a continuation with a guard adds one count.
#[derive(Clone, Default)]
pub struct CountingMatcher {
    /// The guard checks so far.
    pub guards: Arc<AtomicUsize>,
}

impl CountingMatcher {
    /// The guard checks so far.
    pub fn count(&self) -> usize { self.guards.load(Ordering::SeqCst) }

    /// Counts one call of a guard check on `continuation`, when it has a guard.
    fn note(&self, continuation: &TaggedContinuation) {
        if continuation.guard.is_some() {
            self.guards.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for CountingMatcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        Matcher.get(pattern, data)
    }

    fn get_metered(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<Option<ListParWithRandom>, RSpaceError> {
        Matcher.get_metered(pattern, data, meter)
    }

    fn check_commit(
        &self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
    ) -> bool {
        self.note(continuation);
        Matcher.check_commit(continuation, matched)
    }

    fn check_commit_metered(
        &self,
        continuation: &TaggedContinuation,
        matched: &[&ListParWithRandom],
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<bool, RSpaceError> {
        self.note(continuation);
        Matcher.check_commit_metered(continuation, matched, meter)
    }
}

/// A play space over a fresh in-memory store whose injected matcher is `matcher`.
pub async fn play_with(
    matcher: impl Match<BindPattern, ListParWithRandom, TaggedContinuation> + 'static,
) -> ParRSpace {
    let mut stores = InMemoryStoreManager::new();
    ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(matcher)),
    )
    .expect("an in-memory space")
}
