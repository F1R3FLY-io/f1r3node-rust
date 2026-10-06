//! The interface between the native space agents and their callers (P4 design B.4, C.3, C.7
//! and C.8, as the readiness review corrects them). This interface is frozen for the P4
//! batches.
//!
//! * Batch 4B implements [`SpaceAgent`]. The P4 agent is `GenericSpace<HashMapOuter,
//!   QueueDiscipline>` (P4 design B.4), and a caller makes it with `Default::default()`.
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
//!
//! * A method opens at most one transaction. It locks every channel that it touches: a
//!   collection operation `[C(sid), S(sid), S(sid,k), D(sid,k)]`, a creation `[C(sid), S(sid),
//!   H(sid)]`, and a read-only access `[C(sid)]`.
//! * A read-only access stages nothing and drops its transaction. It never commits an empty
//!   transaction.
//! * A method that stages work returns a [`Prepared`]. The job commits it exactly once with
//!   [`Prepared::commit`]. Nothing awaits between `begin` and the commit, except `begin`.
//! * Matching uses only the metered matcher calls, with [`SpaceEnv::meter`]. The agent also
//!   reserves its own decodes, copies and tie-breaks on that meter before it makes them, and it
//!   prepays at staging the work of the outcome after the commit (decision 42).
//! * A collection operation fires in at most `costs::FIRE_ROUND_CAP` rounds. It re-issues the
//!   rest with the A10 rule of `costs::FireRounds`. A re-issued persistent operation keeps the
//!   `ord` of its first round (decision 43).

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{
    BindPattern, CostAuthority, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use rspace_plus_plus::rspace::rspace_interface::RSpaceResult;
use rspace_plus_plus::rspace::space_store_handle::{
    BoxedSpaceTxn, SpaceStoreHandle, SpaceTxnReceipt,
};

use super::errors::SpaceError;
use super::namespace::{CollectionKey, NativeSpace};
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::reified::costs::{ReissueCause, ScanMeter};

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
    /// The host-work meter of the job (readiness review S5, decision 42): the matcher and the
    /// agent reserve each read and copy on it before they make it.
    pub meter: ScanMeter,
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
    /// The introduction authority of the send (P4 design B.7).
    pub authority: CostAuthority,
    /// The `ord` of the first round of a re-issued persistent send, or `None` (decision 43).
    /// The send keeps this `ord` in every round, so it keeps its record, its identity and its
    /// place in the collection. A send of the program and a re-issued send that is not
    /// persistent take the `ord` of their bump.
    pub reissued_ord: Option<i64>,
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
    /// The cost authority of the continuation: the receive authority of `eval_receive`.
    pub authority: CostAuthority,
    /// The introduction authority of the receive (P4 design B.7).
    pub introduction_authority: CostAuthority,
    /// The `ord` of the first round of a re-issued persistent receive, or `None` (decision
    /// 43), as for [`ProduceOp::reissued_ord`].
    pub reissued_ord: Option<i64>,
}

/// One operation on a collection of a native space. It runs in one `Intent::Space` job.
#[derive(Clone, Debug, PartialEq)]
pub enum SpaceOp {
    Produce(ProduceOp),
    Consume(ConsumeOp),
}

impl SpaceOp {
    /// The space of the operation.
    pub fn space(&self) -> NativeSpace {
        match self {
            SpaceOp::Produce(op) => op.space,
            SpaceOp::Consume(op) => op.space,
        }
    }

    /// The authority of the scheduler footprint, as `prepare` takes it for a host intent: the
    /// cost authority of the datum, or the cost authority of the continuation.
    pub fn footprint_authority(&self) -> Option<CostAuthority> {
        match self {
            SpaceOp::Produce(op) => op.data.cost_authority.clone(),
            SpaceOp::Consume(op) => match op.authority.regions.is_empty() {
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
    pub async fn commit(self) -> Result<T, SpaceError> {
        let Prepared { mut txn, finish } = self;
        let receipt = txn.commit().await?;
        finish(receipt)
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
pub async fn run_op(
    agent: &dyn SpaceAgent,
    env: &SpaceEnv,
    op: SpaceOp,
) -> Result<SpaceOutcome, SpaceError> {
    let prepared = match op {
        SpaceOp::Produce(op) => agent.prepare_produce(env, op).await?,
        SpaceOp::Consume(op) => agent.prepare_consume(env, op).await?,
    };
    prepared.commit().await
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
            meter: ScanMeter::new(None),
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
            authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    fn region(byte: u8) -> CostAuthority {
        CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![byte; 32],
                signature: None,
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
            _space: NativeSpace,
        ) -> Result<Par, SpaceError> {
            Err(SpaceError::DanglingHandle)
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
}
