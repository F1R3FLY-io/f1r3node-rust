//! The interface between the native space agents and their callers (P4 design B.4, C.3, C.7
//! and C.8, as the readiness review corrects them). This interface is frozen for the P4
//! batches.
//!
//! * Batch 4B implements [`SpaceAgent`]. The agent of the native spaces is the family agent
//!   `families::FamilyAgent` (P5 design v3, A.3.4). It reads the config of a space once and runs
//!   `GenericSpace<S, D>` of the family that the config names (P4 design B.4). A caller makes it
//!   with `Default::default()`.
//! * Batch 4C makes a [`SpaceOp`] in the reducer hooks and in the R7 backend, runs it with
//!   `ReifiedRuntime::run_op`, and dispatches the [`SpaceOutcome`].
//! * Batch 4D creates a space with `ReifiedRuntime::create_space`, reads its config with
//!   `ReifiedRuntime::read_config` for the handle methods and for `getSpaceAgent`, runs the
//!   handle methods `produce` and `consume` with `ReifiedRuntime::run_op`, and calls
//!   [`SpaceAgent::gensym`] for the handle method `gensym`.
//!
//! Each `ReifiedRuntime` method runs one `Intent::Space` job, and the job calls the agent.
//!
//! Every agent method follows the substrate rules (readiness review S2, S3, S5 and S10):
// Changed by decisions 92 and 94: a collection operation takes its locks in two phases, and
// every transaction of a space locks `S(sid)`.
// //! * A method opens at most one transaction, with [`SpaceEnv::begin`]. It locks every channel
// //!   that it touches: a collection operation `[C(sid), S(sid), S(sid,k), D(sid,k)]`, a creation
// //!   `[C(sid), S(sid), H(sid)]`, and a read-only access `[C(sid)]`.
//! * A method opens at most one transaction, with [`SpaceEnv::begin`] or with
//!   [`SpaceEnv::begin_phase_a`]. It locks every channel that it touches: a collection operation
//!   `[C(sid), S(sid), S(sid,k), D(sid,k)]` and the keys of its join scope, a creation
//!   `[C(sid), S(sid), H(sid)]`, an allocation `[C(sid), S(sid), A(sid)]`, and a read-only
//!   access `[C(sid), S(sid)]`.
//! * A collection operation takes phase A over its own keys, classifies its route under phase A,
//!   and then takes phase B over its own keys and the keys of its scope
//!   ([`SpaceEnv::lock_phase_b`], decision 94). It stages nothing before phase B.
//! * Every row of a space names `S(sid)` in its footprint, and the first row of each committed
//!   transaction is unique to its job (decision 92, `spaces::anchor`).
//! * A read-only access stages nothing and drops its transaction. It never commits an empty
//!   transaction.
//! * A method that stages work returns a [`Prepared`]. The job commits it exactly once with
//!   [`Prepared::commit`]. Nothing awaits between `begin` and the commit, except `begin`.
//! * Matching uses the injected matcher of the host ([`super::matching::SpaceMatcher`]). A space
//!   operation meters only its COMMs and the bytes that it stores, through the observer calls
//!   of the host (DR-131).
//! * A collection operation fires in at most `costs::FIRE_ROUND_CAP` rounds. It re-issues the
//!   rest with the A10 rule of `costs::FireRounds`. A re-issued persistent operation keeps the
//!   `ord` of its first round (decision 43).
//!
//! The spine S1 of P5 (P5 design v3, A.3.1) freezes this interface for the batches of P5: the
//! named params of a send ([`ProduceOp::params`]), the query of a receive
//! ([`ConsumeOp::query`]), the settle of a collection after a fire ([`SettleOp`],
//! [`SpaceAgent::prepare_settle`]), and the allocation of index names ([`AllocOp`],
//! [`SpaceAgent::allocate`]). Joins add [`JoinOp`], [`SpaceAgent::prepare_join`] and the
//! cascade state of a settle ([`SettleOp::cascade`]) in P5c.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{
    BindPattern, CostAuthority, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::space_factory_urn::SpaceFactoryUrn;
use rspace_plus_plus::rspace::rspace_interface::RSpaceResult;
// Changed by decision 90 (item 3.10): the hold of a commit.
// use rspace_plus_plus::rspace::space_store_handle::{
//     BoxedSpaceTxn, SpaceStoreHandle, SpaceTxnError, SpaceTxnReceipt,
// };
use rspace_plus_plus::rspace::space_store_handle::{
    BoxedSpaceTxn, SpaceStoreHandle, SpaceTxnError, SpaceTxnReceipt, TxnHold,
};

use super::ephemeral::VolatileRegistry;
use super::errors::SpaceError;
use super::mirror::MirrorWrites;
// Changed by decision 92: the footprint key of a space transaction.
// use super::namespace::{CollectionKey, NativeSpace};
use super::namespace::{self, CollectionKey, NativeSpace, ReifiedId};
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::reified::costs::ReissueCause;

/// The substrate handle over the host store of a Rholang runtime.
pub type ParSpaceStore = SpaceStoreHandle<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// An open transaction of a [`ParSpaceStore`].
pub type ParSpaceTxn = BoxedSpaceTxn<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// The receipt of a committed [`ParSpaceTxn`].
pub type ParSpaceReceipt = SpaceTxnReceipt<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// What a space job runs against. `ReifiedRuntime::space_env` makes it in the job.
#[derive(Clone)]
pub struct SpaceEnv {
    /// The handle over the host store.
    pub store: ParSpaceStore,
    /// The phlo budget of the deploy. The `before_observe` closures register the introduction
    /// authority of each staged operation with it (P4 design B.7).
    pub budget: RuntimeBudget,
    /// The book of the ephemeral spaces of the root evaluation (P7, VR-1). `begin` records the
    /// volatile lock keys of each transaction in it (VR-2).
    pub volatile: Arc<VolatileRegistry>,
    /// Who started the job: the program, or the runtime that re-issues an operation (P7, SQ-3).
    /// The claims of the Seq rules apply only to program operations.
    pub origin: OpOrigin,
}

/// The origin of a space job (P7, design v3 SQ-3, A3). A routed send, receive or join, an R7
/// reply and the handle method `produce` are program operations. A re-issue at a divergent pair
/// or at `FIRE_ROUND_CAP` is internal, and a settle job is internal by its variant
/// (`SpaceOp::Settle`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OpOrigin {
    /// An operation that the program started.
    #[default]
    Program,
    // Changed by decision 124 (plan v4 C7.14, ruling OP2-25 option B): a re-issue says whether
    // it stages its anchor first. The text before:
    // ```
    // /// An operation that the runtime re-issues.
    // Internal,
    // ```
    /// An operation that the runtime re-issues. `anchor_first` holds when the finish built the
    /// re-issue from a Stop for an owner refusal (CR-19 case 2). Only the Stop branch of CR-31
    /// and a Skipped row d build the cause `OwnerRefusal` (ruling D-4), so the cause marks that
    /// case. `anchor_first` never depends on the cause alone (CR-19, ruling D-6).
    Internal { anchor_first: bool },
}

impl SpaceEnv {
    /// Opens the transaction of a method over `lock_keys`. The transaction reads the store as
    /// the host does.
    ///
    /// The lock keys with an ephemeral KIND go into the book of the evaluation first, so the
    /// sweep at the end of the evaluation removes every volatile channel that the transaction
    /// can write (VR-2).
    ///
    /// Changed by decision 94: the transaction takes phase A ([`SpaceEnv::begin_phase_a`]) and
    /// then phase B over the same keys ([`SpaceEnv::lock_phase_b`]).
    // pub async fn begin(&self, lock_keys: &[Par]) -> Result<ParSpaceTxn, SpaceTxnError> {
    //     self.volatile.record_locked(lock_keys);
    //     self.store.begin(lock_keys).await
    // }
    pub async fn begin(&self, lock_keys: &[Par]) -> Result<ParSpaceTxn, SpaceTxnError> {
        let mut txn = self.begin_phase_a(lock_keys).await?;
        self.lock_phase_b(&mut txn, &[]).await?;
        Ok(txn)
    }

    /// Added by decision 94: opens the transaction of a method over `lock_keys` with phase A
    /// only. The transaction can read the store, and it stages nothing before
    /// [`SpaceEnv::lock_phase_b`]. A collection operation classifies its route under phase A.
    ///
    /// The lock keys with an ephemeral KIND go into the book of the evaluation first (VR-2).
    /// Decision 92: when the lock keys hold the config channel `C(sid)` of a space, the
    /// transaction names the space counter `S(sid)` in the footprint of each row, so every row
    /// of the space is in one chain (`SpaceReplayOrder.tla`). The lock keys of such a
    /// transaction must hold `S(sid)`.
    pub async fn begin_phase_a(&self, lock_keys: &[Par]) -> Result<ParSpaceTxn, SpaceTxnError> {
        self.volatile.record_locked(lock_keys);
        let footprint_key = footprint_key(lock_keys)?;
        let mut txn = self.store.begin_phase_a(lock_keys).await?;
        if let Some(key) = footprint_key {
            txn.set_footprint_key(key)?;
        }
        Ok(txn)
    }

    /// Added by decision 94: takes phase B of `txn` over its own lock keys and the keys
    /// `scope_keys` of a join scope, in one sorted call (`JoinScopeLock.tla`). The keys with an
    /// ephemeral KIND go into the book of the evaluation first (VR-2).
    pub async fn lock_phase_b(
        &self,
        txn: &mut ParSpaceTxn,
        scope_keys: &[Par],
    ) -> Result<(), SpaceTxnError> {
        self.volatile.record_locked(scope_keys);
        txn.lock_phase_b(scope_keys).await
    }
}

/// Added by decision 92: the footprint key of a transaction over `lock_keys`. It is the space
/// counter `S(sid)` when the lock keys hold the config channel `C(sid)` of the space, and
/// otherwise `None`. Lock keys with two config channels are `Inconsistent`.
fn footprint_key(lock_keys: &[Par]) -> Result<Option<Par>, SpaceTxnError> {
    let mut space = None;
    for key in lock_keys {
        match (namespace::reified_id_of(key), space) {
            (Some(ReifiedId::Config(found)), None) => space = Some(found),
            (Some(ReifiedId::Config(_)), Some(_)) => {
                return Err(SpaceTxnError::Inconsistent(
                    "the lock keys of a space transaction hold two config channels",
                ))
            }
            _ => {}
        }
    }
    Ok(space.map(|space| namespace::gprivate(namespace::space_counter(space))))
}

/// A produce on a collection of a native space: a routed send (P4 design C.3), an R7 reply
/// (C.6) or the handle method `produce` (C.8).
#[derive(Clone, Debug, PartialEq)]
pub struct ProduceOp {
    /// The space.
    pub space: NativeSpace,
    /// The collection: the canonical bytes of the inner channel.
    pub key: CollectionKey,
    /// The inner channel as the program names it, after the unbundling of `eval_send`. When
    /// the space has no config record, the agent fails with `DanglingName` for a space name
    /// (route R3) and with `DanglingHandle` for every other channel (P4 design B.4).
    pub channel: Par,
    /// The datum as the program sends it: the payload, its random state and its cost
    /// authority. A trailing PARAMS tuple is already removed.
    pub data: ListParWithRandom,
    /// True for `!!`.
    pub persistent: bool,
    /// The named params of the send, by key (decision 21): for example `priority` on a priority
    /// queue (P5b). A send without params has none. The agent checks the keys against the
    /// schema of its discipline before the bump, and then their values.
    pub params: BTreeMap<String, Par>,
    /// The introduction authority of the send (P4 design B.7).
    pub authority: CostAuthority,
    /// The `ord` of the first round of a re-issued persistent send, or `None` (decision 43).
    /// The send keeps this `ord` in every round, so it keeps its record, its identity and its
    /// place in the collection. A send of the program and a re-issued send that is not
    /// persistent take the `ord` of their bump.
    pub reissued_ord: Option<i64>,
}

/// The query of a receive (decision 21): `ch ~ (q…; k=v…)` or the bare `ch ~ e`. The
/// payload holds the positional parts and `named` the named params, by key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueryParams {
    /// The positional parts of the query.
    pub payload: Vec<Par>,
    /// The named params of the query, by key: for example `priority` on a priority queue.
    pub named: BTreeMap<String, Par>,
}

/// A single-bind receive on a collection of a native space (P4 design C.3).
#[derive(Clone, Debug, PartialEq)]
pub struct ConsumeOp {
    /// The space.
    pub space: NativeSpace,
    /// The collection: the canonical bytes of the inner channel.
    pub key: CollectionKey,
    /// The inner channel as the program names it, after the unbundling of `unbundle_receive`.
    /// A missing config record fails as for [`ProduceOp::channel`].
    pub channel: Par,
    /// The pattern of the bind.
    pub pattern: BindPattern,
    /// The body and its random state. Under a use-context the body is already wrapped
    /// (`reified::scope::wrap_body`).
    pub body: ParWithRandom,
    /// True for `<=`.
    pub persistent: bool,
    /// True for `<<-`.
    pub peek: bool,
    /// The substituted `where` guard.
    pub guard: Option<Par>,
    /// The query of the receive, or `None`. A discipline that takes no query params rejects a
    /// query with `QuerySpecUnsupported` before the bump.
    pub query: Option<QueryParams>,
    /// The cost authority of the continuation: the receive authority of `eval_receive`.
    pub authority: CostAuthority,
    /// The introduction authority of the receive (P4 design B.7).
    pub introduction_authority: CostAuthority,
    /// The `ord` of the first round of a re-issued persistent receive, or `None` (decision
    /// 43), as for [`ProduceOp::reissued_ord`].
    pub reissued_ord: Option<i64>,
}

/// One bind of a join (P5c, decision 29): a collection of the space, the channel as the program
/// names it, the pattern of the bind and its query. A bind on a priority queue may carry the
/// query param `priority`.
#[derive(Clone, Debug, PartialEq)]
pub struct JoinBind {
    /// The collection of the bind: the canonical bytes of its inner channel.
    pub key: CollectionKey,
    /// The inner channel as the program names it, after the unbundling of `unbundle_receive`.
    pub channel: Par,
    /// The pattern of the bind.
    pub pattern: BindPattern,
    /// The query of the bind, or `None`.
    pub query: Option<QueryParams>,
}

/// A receive with two or more binds whose channels all route to one native space (P5c,
/// decision 29). The join engine runs it (`spaces::join`).
#[derive(Clone, Debug, PartialEq)]
pub struct JoinOp {
    /// The space.
    pub space: NativeSpace,
    /// The binds in the bind order of the receive. A join has at least two binds.
    pub binds: Vec<JoinBind>,
    /// The body and its random state. Under a use-context the body is already wrapped
    /// (`reified::scope::wrap_body`).
    pub body: ParWithRandom,
    /// True for `<=`.
    pub persistent: bool,
    /// True for `<<-`. One arrow covers every bind of a receive, so every bind peeks.
    pub peek: bool,
    /// The substituted `where` guard. It is checked once, over the values of every bind in
    /// bind order (decision 37).
    pub guard: Option<Par>,
    /// The cost authority of the continuation: the receive authority of `eval_receive`.
    pub authority: CostAuthority,
    /// The introduction authority of the receive (P4 design B.7).
    pub introduction_authority: CostAuthority,
    /// The order of the first round of a re-issued persistent join in the collection of each
    /// bind, in bind order, or `None` (decision 43). Repeated binds on one collection have equal
    /// orders. A join of the program and a re-issued join that is not persistent have none.
    pub reissued_ords: Option<Vec<i64>>,
}

/// A second send (decision 45, Rule E) that a cascade of the join engine still owes when its
/// transaction stops at the round cap (decision 49). The settle job that continues the cascade
/// offers it first.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingProduce {
    /// The collection of the record.
    pub key: CollectionKey,
    /// The record as the store held it: its envelope and its payload. The bytes fix the produce
    /// identity, so the second send keeps the identity of the record.
    pub record: ListParWithRandom,
    /// True for a persistent record.
    pub persist: bool,
    /// The depth of the marker that a peek COMM removed, for a second send of a PathMap datum
    /// (joins plan section 9): the settle job restores that marker when the second send fires
    /// nothing. A keyed store always carries `None`.
    pub restore: Option<u8>,
}

/// The rest of a cascade of the join engine (P5c, joins plan section 4.10).
#[derive(Clone, Debug, PartialEq)]
pub struct CascadeState {
    /// The other collections that may owe fires, in ascending key order, without the key of the
    /// settle.
    pub seeds: Vec<CollectionKey>,
    /// The second sends that the cascade owes, in the order in which they run.
    pub pending: Vec<PendingProduce>,
    /// The operation to re-issue when the cascade completes (decision 43 order): the operation
    /// whose transaction stopped before its element settled, or `None`.
    pub resume: Option<Box<SpaceReissue>>,
}

/// The settle of a collection: the fires that a discipline with a settle rule still owes after
/// an operation (P5b, F2). A stack or a priority queue issues it as the re-issue of a settle
/// that reached `FIRE_ROUND_CAP`. The join engine issues it with a cascade state (P5c). A
/// program never issues it.
#[derive(Clone, Debug, PartialEq)]
pub struct SettleOp {
    /// The space.
    pub space: NativeSpace,
    /// The collection.
    pub key: CollectionKey,
    /// The inner channel of the operation that started the settle.
    pub channel: Par,
    /// The random state of the operation that started the settle.
    pub random_state: Vec<u8>,
    /// The cost authority of the operation that started the settle.
    pub authority: CostAuthority,
    /// The introduction authority of the operation that started the settle.
    pub introduction_authority: CostAuthority,
    /// The rest of a cascade of the join engine, or `None` for the settle of one collection
    /// (P5b).
    pub cascade: Option<Box<CascadeState>>,
}

/// The allocation of `count` index names of an Array or Vector space (P5d, F11): the names of
/// a `new` inside a `use` block of the space, or of the handle method `gensym`.
#[derive(Clone, Debug, PartialEq)]
pub struct AllocOp {
    /// The space.
    pub space: NativeSpace,
    /// The number of names.
    pub count: u64,
    /// The random state of the operation.
    pub random_state: Vec<u8>,
    /// The cost authority of the operation.
    pub authority: CostAuthority,
}

/// The result of [`SpaceAgent::allocate`].
pub enum Allocation {
    /// The outer store has no allocator: the reducer draws the names as it does for every
    /// `new`. Nothing is staged, so nothing is committed.
    Baseline,
    /// The staged allocation of the index names, in order. The job commits it once.
    Indices(Prepared<Vec<u64>>),
}

/// One operation on a collection of a native space. It runs in one `Intent::Space` job.
#[derive(Clone, Debug, PartialEq)]
pub enum SpaceOp {
    Produce(ProduceOp),
    Consume(ConsumeOp),
    Settle(SettleOp),
    Join(JoinOp),
}

impl SpaceOp {
    /// The space of the operation.
    pub fn space(&self) -> NativeSpace {
        match self {
            SpaceOp::Produce(op) => op.space,
            SpaceOp::Consume(op) => op.space,
            SpaceOp::Settle(op) => op.space,
            SpaceOp::Join(op) => op.space,
        }
    }

    /// The authority of the scheduler footprint, as `prepare` takes it for a host intent: the
    /// cost authority of the datum, or the cost authority of the continuation or of the settle.
    pub fn footprint_authority(&self) -> Option<CostAuthority> {
        match self {
            SpaceOp::Produce(op) => op.data.cost_authority.clone(),
            SpaceOp::Consume(op) => match op.authority.regions.is_empty() {
                true => None,
                false => Some(op.authority.clone()),
            },
            SpaceOp::Settle(op) => match op.authority.regions.is_empty() {
                true => None,
                false => Some(op.authority.clone()),
            },
            SpaceOp::Join(op) => match op.authority.regions.is_empty() {
                true => None,
                false => Some(op.authority.clone()),
            },
        }
    }
}

/// A continuation that a space operation fired, ready for `ReducerCore::dispatch`.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceFire {
    /// The fired continuation without its space envelope: the original body, its random
    /// state, its guard and its cost authority.
    pub continuation: TaggedContinuation,
    /// One result for each bind, in bind order. `matched_datum` is the match of the payload
    /// without its envelope. The dispatch data list is
    /// `(channel, matched_datum, removed_datum, persistent)` of each result.
    pub results: Vec<RSpaceResult<Par, ListParWithRandom>>,
}

/// The rest of an operation that a new `Intent::Space` must run (A10, readiness review S5).
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceReissue {
    /// The operation to run again, as the program issued it. A persistent operation also
    /// carries the `ord` of its first round in its `reissued_ord` (decision 43).
    pub op: SpaceOp,
    /// Why the operation re-issues.
    pub cause: ReissueCause,
}

/// The result of a committed collection operation. The fires are in staging order. A counter
/// fire is never a [`SpaceFire`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpaceOutcome {
    pub fires: Vec<SpaceFire>,
    pub reissue: Option<SpaceReissue>,
    /// The net writes of the transaction to the mirror of a VectorDB space with a mirror (P6
    /// step 7b). Changed by decision 98: the runtime publishes them in play before the hold drops.
    pub mirror: Option<MirrorWrites>,
}

impl SpaceOutcome {
    /// The outcome with the mirror writes `mirror`.
    pub fn with_mirror(self, mirror: Option<MirrorWrites>) -> Self { Self { mirror, ..self } }
}

/// The records of a new space (P4 design C.7, steps 5 to 7). The factory makes this value and
/// the agent stages the records.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceCreation {
    /// The new space.
    pub space: NativeSpace,
    /// The config map (I-17): `spaceId`, `outer`, `inner`, `qualifier`, `params` and `theory`.
    pub config: Par,
    /// The pattern of the handle continuation (P4 design C.7, step 7).
    pub handle_pattern: BindPattern,
    /// The handle continuation that forwards each call to `HANDLE_OP` (P4 design C.7, step 7).
    /// The factory makes it with the random state `RandomPurpose::Handle` of the call, no guard
    /// and no cost authority. The agent parks it as a persistent continuation.
    pub handle_continuation: TaggedContinuation,
    /// The random state of the factory call. The config datum uses `RandomPurpose::Config`, and
    /// the space counter datum uses `RandomPurpose::SpaceCounter` and then `split_byte(1)`
    /// (P4 design C.7 and C.11).
    pub random: Blake2b512Random,
    /// The cost authority of the caller (P4 design B.7).
    pub authority: CostAuthority,
    /// The family of the factory (P5 design v3, A.3.4). The family agent sends the creation to
    /// the agent of this family, and that agent checks that the config names the same family.
    pub family: SpaceFactoryUrn,
}

type Finish<T> = Box<dyn FnOnce(ParSpaceReceipt) -> Result<T, SpaceError> + Send>;

/// Staged work and its open transaction. [`Prepared::commit`] is the only mutation. A dropped
/// value releases its locks and changes nothing.
pub struct Prepared<T> {
    txn: ParSpaceTxn,
    finish: Finish<T>,
}

impl<T> Prepared<T> {
    /// Wraps a transaction with staged work. `finish` turns the receipt into the result.
    pub fn new(
        txn: ParSpaceTxn,
        finish: impl FnOnce(ParSpaceReceipt) -> Result<T, SpaceError> + Send + 'static,
    ) -> Self {
        Self {
            txn,
            finish: Box::new(finish),
        }
    }

    /// Commits the transaction once and returns the result. A failure of operation `i` keeps
    /// operations `0..i` (prefix atomicity, decision 19 D6).
    ///
    /// Changed by decision 90 (decision 91 SQ-4): the finish runs under the stripes of the
    /// commit (`commit_holding`), so the store of the seq claims is atomic with their check
    /// (`SeqGuardToctouUnsafe` of `SpaceCoordination.tla`). Racing evaluation runs other jobs
    /// of the space between a commit and its finish, which the scheduler did not.
    // pub async fn commit(self) -> Result<T, SpaceError> {
    //     let Prepared { mut txn, finish } = self;
    //     let receipt = txn.commit().await?;
    //     finish(receipt)
    // }
    pub async fn commit(self) -> Result<T, SpaceError> {
        let (result, hold) = self.commit_holding().await?;
        drop(hold);
        Ok(result)
    }

    /// Added by decision 90 (item 3.10, and decision 91 SQ-4): commits the transaction once,
    /// runs the finish under its stripes, and returns the result with the hold of the stripes.
    /// The caller does what must follow the commit in commit order, such as the publication of
    /// the mirror writes of play (decision 98), before it drops the hold. A failed commit or
    /// finish releases the stripes at once.
    pub async fn commit_holding(self) -> Result<(T, TxnHold), SpaceError> {
        let Prepared { mut txn, finish } = self;
        let (receipt, hold) = txn.commit_holding().await?;
        Ok((finish(receipt)?, hold))
    }
}

/// A native space agent. Each method gets the [`SpaceEnv`] of its job.
#[async_trait]
pub trait SpaceAgent: Send + Sync {
    /// Stages a produce (P4 design B.4): lock, read the config, bump the collection counter,
    /// plan the fires, stage them.
    async fn prepare_produce(
        &self,
        env: &SpaceEnv,
        op: ProduceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError>;

    /// Stages a consume (P4 design B.4).
    async fn prepare_consume(
        &self,
        env: &SpaceEnv,
        op: ConsumeOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError>;

    /// Stages the settle of a collection (P5b). A discipline without a settle rule fails with
    /// `Inconsistent`, because no operation of it issues a settle. A settle with a cascade
    /// state runs in the join engine (P5c).
    async fn prepare_settle(
        &self,
        env: &SpaceEnv,
        op: SettleOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError>;

    /// Stages a join (P5c, decision 29): lock the channels of the collections of its binds,
    /// read the config, then escalate the locks to the join closure and run the join engine.
    async fn prepare_join(
        &self,
        env: &SpaceEnv,
        op: JoinOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError>;

    /// Allocates the index names of `op` (P5d). An outer store without an allocator returns
    /// [`Allocation::Baseline`] and stages nothing.
    async fn allocate(&self, env: &SpaceEnv, op: AllocOp) -> Result<Allocation, SpaceError>;

    /// Stages the records of a new space: the config datum on `C(sid)`, the space counter
    /// `S(sid) = 0`, and the handle continuation on `H(sid)`. A config record on `C(sid)`
    /// fails with `SpaceAlreadyExists`.
    async fn prepare_create(
        &self,
        env: &SpaceEnv,
        creation: SpaceCreation,
    ) -> Result<Prepared<()>, SpaceError>;

    /// Reads the config map of `space` in a read-only transaction. A missing config record
    /// fails with `DanglingHandle`.
    async fn read_config(&self, env: &SpaceEnv, space: NativeSpace) -> Result<Par, SpaceError>;

    /// A fresh name of `space` for the handle method `gensym` (P4 design C.8). `random` is the
    /// random state of the call.
    fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError>;
}

/// Runs a collection operation with `agent`: one transaction and exactly one commit.
///
/// Changed by decision 90 (item 3.10): `run_op_holding` without the hold.
// pub async fn run_op(
//     agent: &dyn SpaceAgent,
//     env: &SpaceEnv,
//     op: SpaceOp,
// ) -> Result<SpaceOutcome, SpaceError> {
//     let prepared = match op {
//         SpaceOp::Produce(op) => agent.prepare_produce(env, op).await?,
//         SpaceOp::Consume(op) => agent.prepare_consume(env, op).await?,
//         SpaceOp::Settle(op) => agent.prepare_settle(env, op).await?,
//         SpaceOp::Join(op) => agent.prepare_join(env, op).await?,
//     };
//     prepared.commit().await
// }
pub async fn run_op(
    agent: &dyn SpaceAgent,
    env: &SpaceEnv,
    op: SpaceOp,
) -> Result<SpaceOutcome, SpaceError> {
    let (outcome, hold) = run_op_holding(agent, env, op).await?;
    drop(hold);
    Ok(outcome)
}

/// Added by decision 90 (item 3.10): runs a collection operation with `agent`, one transaction
/// and exactly one commit, and returns the outcome with the hold of the stripes of the commit
/// ([`Prepared::commit_holding`]).
pub async fn run_op_holding(
    agent: &dyn SpaceAgent,
    env: &SpaceEnv,
    op: SpaceOp,
) -> Result<(SpaceOutcome, TxnHold), SpaceError> {
    let prepared = match op {
        SpaceOp::Produce(op) => agent.prepare_produce(env, op).await?,
        SpaceOp::Consume(op) => agent.prepare_consume(env, op).await?,
        SpaceOp::Settle(op) => agent.prepare_settle(env, op).await?,
        SpaceOp::Join(op) => agent.prepare_join(env, op).await?,
    };
    prepared.commit_holding().await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use models::rhoapi::CostRegion;
    use models::rust::utils::new_gint_par;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::{
        ProduceOutcome, SpaceOpReceipt, SpaceProduce,
    };

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::namespace::{self, Durability};

    fn space() -> NativeSpace {
        NativeSpace {
            sid: [6; namespace::SID_LEN],
            durability: Durability::Durable,
        }
    }

    async fn env() -> SpaceEnv {
        let mut stores = InMemoryStoreManager::new();
        let host = RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        SpaceEnv {
            store: SpaceStoreHandle::new(host),
            budget: RuntimeBudget::new(Cost::create(1_000, "space agent test")),
            volatile: Default::default(),
            origin: Default::default(),
        }
    }

    fn produce_op() -> ProduceOp {
        ProduceOp {
            space: space(),
            key: CollectionKey::from_bytes(vec![1, 2, 3]).expect("a key"),
            channel: new_gint_par(1, Vec::new(), false),
            data: ListParWithRandom {
                pars: vec![new_gint_par(2, Vec::new(), false)],
                random_state: vec![3],
                cost_authority: None,
                cost_stack: None,
            },
            persistent: false,
            params: BTreeMap::new(),
            authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    fn region(byte: u8) -> CostAuthority {
        CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![byte; 32],
                signature: None,
                // Added by decision 90 (DR-135): an unpriced region, as outside native
                // execution.
                price: 0,
                caller: None,
            }],
        }
    }

    /// An agent that stores the produced datum on the config channel and counts its commits.
    struct StoringAgent {
        prepared: AtomicUsize,
        receipts: Arc<Mutex<Vec<usize>>>,
    }

    #[async_trait]
    impl SpaceAgent for StoringAgent {
        async fn prepare_produce(
            &self,
            env: &SpaceEnv,
            op: ProduceOp,
        ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
            self.prepared.fetch_add(1, Ordering::SeqCst);
            let channel = namespace::gprivate(namespace::config_channel(op.space));
            let mut txn = env.store.begin(std::slice::from_ref(&channel)).await?;
            txn.produce(SpaceProduce {
                channel,
                data: op.data,
                persist: op.persistent,
                outcome: ProduceOutcome::Store,
                before_observe: None,
                own: false,
                // Added by decision 123 (plan v4, V3-9): no row search.
                search: None,
            })?;
            let receipts = Arc::clone(&self.receipts);
            Ok(Prepared::new(txn, move |receipt| {
                receipts
                    .lock()
                    .expect("receipts lock")
                    .push(receipt.ops.len());
                match receipt.ops.as_slice() {
                    [SpaceOpReceipt::Stored { .. }] => Ok(SpaceOutcome::default()),
                    _ => Err(SpaceError::Inconsistent("the test agent stores one datum")),
                }
            }))
        }

        async fn prepare_consume(
            &self,
            _env: &SpaceEnv,
            _op: ConsumeOp,
        ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
            Err(SpaceError::Inconsistent("the test agent does not consume"))
        }

        async fn prepare_settle(
            &self,
            _env: &SpaceEnv,
            _op: SettleOp,
        ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
            Err(SpaceError::Inconsistent("the test agent does not settle"))
        }

        async fn prepare_join(
            &self,
            _env: &SpaceEnv,
            _op: JoinOp,
        ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
            Err(SpaceError::Inconsistent("the test agent does not join"))
        }

        async fn allocate(&self, _env: &SpaceEnv, _op: AllocOp) -> Result<Allocation, SpaceError> {
            Ok(Allocation::Baseline)
        }

        async fn prepare_create(
            &self,
            _env: &SpaceEnv,
            _creation: SpaceCreation,
        ) -> Result<Prepared<()>, SpaceError> {
            Err(SpaceError::Inconsistent("the test agent does not create"))
        }

        async fn read_config(
            &self,
            _env: &SpaceEnv,
            space: NativeSpace,
        ) -> Result<Par, SpaceError> {
            Err(SpaceError::DanglingHandle { space: space.sid })
        }

        fn gensym(
            &self,
            space: NativeSpace,
            _random: &Blake2b512Random,
        ) -> Result<Par, SpaceError> {
            Ok(namespace::gprivate(namespace::name_id(
                space,
                namespace::NameTail::Index(0),
            )))
        }
    }

    #[tokio::test]
    async fn run_op_commits_the_prepared_work_once() {
        let env = env().await;
        let agent = StoringAgent {
            prepared: AtomicUsize::new(0),
            receipts: Arc::new(Mutex::new(Vec::new())),
        };
        let outcome = run_op(&agent, &env, SpaceOp::Produce(produce_op()))
            .await
            .expect("the produce commits");
        assert_eq!(outcome, SpaceOutcome::default());
        assert_eq!(agent.prepared.load(Ordering::SeqCst), 1);
        assert_eq!(*agent.receipts.lock().expect("receipts lock"), vec![1]);
        let channel = namespace::gprivate(namespace::config_channel(space()));
        let mut txn = env
            .store
            .begin(std::slice::from_ref(&channel))
            .await
            .expect("a read-only transaction");
        assert_eq!(txn.data(&channel).expect("the staged view").len(), 1);
    }

    #[tokio::test]
    async fn a_dropped_prepared_value_changes_nothing() {
        let env = env().await;
        let agent = StoringAgent {
            prepared: AtomicUsize::new(0),
            receipts: Arc::new(Mutex::new(Vec::new())),
        };
        let prepared = agent
            .prepare_produce(&env, produce_op())
            .await
            .expect("the produce stages");
        drop(prepared);
        let channel = namespace::gprivate(namespace::config_channel(space()));
        let mut txn = env
            .store
            .begin(std::slice::from_ref(&channel))
            .await
            .expect("the locks are released");
        assert!(txn.data(&channel).expect("the staged view").is_empty());
        assert!(agent.receipts.lock().expect("receipts lock").is_empty());
        let consume = run_op(
            &agent,
            &env,
            SpaceOp::Consume(ConsumeOp {
                space: space(),
                key: CollectionKey::from_bytes(vec![1]).expect("a key"),
                channel: Par::default(),
                pattern: BindPattern::default(),
                body: ParWithRandom::default(),
                persistent: false,
                peek: false,
                guard: None,
                query: None,
                authority: CostAuthority::default(),
                introduction_authority: CostAuthority::default(),
                reissued_ord: None,
            }),
        )
        .await;
        assert_eq!(
            consume,
            Err(SpaceError::Inconsistent("the test agent does not consume"))
        );
    }

    #[test]
    fn footprint_authority_mirrors_the_host_intents() {
        let mut produce = produce_op();
        assert_eq!(
            SpaceOp::Produce(produce.clone()).footprint_authority(),
            None
        );
        produce.data.cost_authority = Some(region(4));
        assert_eq!(
            SpaceOp::Produce(produce.clone()).footprint_authority(),
            Some(region(4))
        );
        assert_eq!(SpaceOp::Produce(produce).space(), space());
        let consume = ConsumeOp {
            space: space(),
            key: CollectionKey::from_bytes(vec![9]).expect("a key"),
            channel: Par::default(),
            pattern: BindPattern::default(),
            body: ParWithRandom::default(),
            persistent: true,
            peek: false,
            guard: None,
            query: None,
            authority: CostAuthority::default(),
            introduction_authority: region(5),
            reissued_ord: None,
        };
        assert_eq!(
            SpaceOp::Consume(consume.clone()).footprint_authority(),
            None
        );
        let signed = ConsumeOp {
            authority: region(6),
            ..consume
        };
        assert_eq!(
            SpaceOp::Consume(signed).footprint_authority(),
            Some(region(6))
        );
    }

    /// Added by decision 92: the footprint key of a transaction is the space counter `S(sid)`
    /// of the one space whose config channel `C(sid)` is a lock key. Lock keys without a config
    /// channel give no key. Lock keys with the config channels of two spaces are `Inconsistent`.
    #[test]
    fn footprint_key_is_the_space_counter_of_the_one_config_channel() {
        let other = NativeSpace {
            sid: [7; namespace::SID_LEN],
            ..space()
        };
        let config = |space| namespace::gprivate(namespace::config_channel(space));
        let counter = |space| namespace::gprivate(namespace::space_counter(space));
        assert_eq!(footprint_key(&[]), Ok(None));
        assert_eq!(
            footprint_key(&[counter(space()), new_gint_par(1, Vec::new(), false)]),
            Ok(None)
        );
        assert_eq!(
            footprint_key(&[counter(space()), config(space())]),
            Ok(Some(counter(space())))
        );
        assert_eq!(
            footprint_key(&[config(space()), config(other)]),
            Err(SpaceTxnError::Inconsistent(
                "the lock keys of a space transaction hold two config channels"
            ))
        );
    }

    /// Added by decision 92: `SpaceEnv::begin` sets the footprint key of a transaction whose
    /// lock keys hold `C(sid)`, so its commit requires that the first staged operation is the
    /// job's own. A transaction that opens on the store directly has no key.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn begin_sets_the_footprint_key_of_a_space_transaction() {
        let env = env().await;
        let config = namespace::gprivate(namespace::config_channel(space()));
        let keys = [
            config.clone(),
            namespace::gprivate(namespace::space_counter(space())),
        ];
        let store = |own| SpaceProduce {
            channel: config.clone(),
            data: produce_op().data,
            persist: true,
            outcome: ProduceOutcome::Store,
            before_observe: None,
            own,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        };
        let mut txn = env.begin(&keys).await.expect("begin");
        txn.produce(store(false)).expect("stage a shared store");
        assert_eq!(
            txn.commit().await.err(),
            Some(SpaceTxnError::InvalidOperation(
                "the first operation of a space transaction is not its job's own"
            ))
        );
        let mut txn = env.begin(&keys).await.expect("begin");
        txn.produce(store(true)).expect("stage an own store");
        txn.commit().await.expect("an own first operation commits");
        let mut txn = env.store.begin(&keys).await.expect("begin on the store");
        txn.produce(store(false)).expect("stage a shared store");
        txn.commit()
            .await
            .expect("a transaction without a footprint key commits");
    }
}
