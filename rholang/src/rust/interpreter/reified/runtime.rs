//! The Reified RSpaces runtime of a reducer (P4 design C.5, I-13).
//!
//! `create_rho_env` captures the concrete host space before the `DeterministicRSpace` wrap
//! and makes a `Host` runtime with one [`ParSpaceStore`] (A17). Host clones share all state, so
//! the handle sees the store that the reducer sees. A `Host` runtime runs spaces in legacy
//! execution and in native play (decision 47). `ReducerCore::new` and
//! `DebruijnInterpreter::new` make a `Detached` runtime, in which every space operation fails
//! closed. The native replay environment makes a `NativeReplay` runtime and binds it to the
//! store of its native replay session (decision 47). The runtime holds no cache, so nothing in
//! it survives a reset.
//!
//! The runtime also holds the mirror journal of the evaluation (P6 step 7b): the net writes of
//! the transactions of VectorDB spaces with a mirror. [`ReifiedRuntime::run_op`] appends them
//! inside the space job, so the scheduler orders the appends of one space as it orders its jobs.
//! [`ReifiedRuntime::settle_mirror_journal`] publishes the journal when the final result of the
//! deploy has no errors and drops it otherwise (M1, decision 66), and
//! [`ReifiedRuntime::begin_evaluation_scope`] drops a journal that no result settled.
//!
//! Each space operation runs as an `Intent::Space` job (readiness review S9) with one
//! transaction and at most one commit (S3).

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::space_store_handle::{SpaceStoreHandle, SpaceTxnHost};

use super::vectordb_services;
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::deterministic_reduction::reified_schedule::{
    submit_space, SpaceRequest,
};
use crate::rust::interpreter::deterministic_reduction::ReductionCoordinator;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::interpreter::EvaluateResult;
use crate::rust::interpreter::spaces::agent::{
    self, AllocOp, Allocation, OpOrigin, ParSpaceStore, SpaceAgent, SpaceCreation, SpaceEnv,
    SpaceOp, SpaceOutcome,
};
use crate::rust::interpreter::spaces::ephemeral::{ephemeral_space, VolatileRegistry};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::factory::constructor::Execution;
use crate::rust::interpreter::spaces::mirror::MirrorJournal;
use crate::rust::interpreter::spaces::namespace::{self, NativeSpace, ReifiedId};
use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

/// The mode of a [`ReifiedRuntime`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReifiedRuntimeMode {
    /// The host store of a play, replay or reporting runtime.
    Host,
    /// A reducer without a host store: every space operation fails with `RuntimeUnavailable`.
    Detached,
    /// The native replay environment (decision 47). The runtime runs spaces over the native
    /// replay session after the environment binds it, and fails with `RuntimeUnavailable`
    /// before.
    NativeReplay,
}

enum RuntimeState {
    Host {
        store: ParSpaceStore,
        budget: RuntimeBudget,
    },
    Detached,
    // The environment registers the runtime with its system processes before its native
    // replay session exists, so it binds the store of the session and its budget once,
    // right after it builds the session (decision 47, W2).
    NativeReplay {
        bound: OnceLock<(ParSpaceStore, RuntimeBudget)>,
    },
}

/// The Reified RSpaces runtime of a reducer. The reducer holds it in `ReducerCore::reified`.
pub struct ReifiedRuntime {
    state: RuntimeState,
    /// The mirror journal of the evaluation (P6 step 7b).
    journal: Arc<MirrorJournal>,
    /// The book of the ephemeral spaces of the root evaluation (P7, VR-1).
    volatile: Arc<VolatileRegistry>,
}

impl ReifiedRuntime {
    /// The mode of the runtime.
    pub fn mode(&self) -> ReifiedRuntimeMode {
        match self.state {
            RuntimeState::Host { .. } => ReifiedRuntimeMode::Host,
            RuntimeState::Detached => ReifiedRuntimeMode::Detached,
            RuntimeState::NativeReplay { .. } => ReifiedRuntimeMode::NativeReplay,
        }
    }

    /// Starts a root evaluation (FR-5', readiness review S4). In the `Host` mode and in a bound
    /// `NativeReplay` mode the handle forgets the parent orders that committed before, so a
    /// deploy that the runtime evaluates again commits under the same parent orders. A native
    /// replay environment keeps its store when it restores a checkpoint, so it resets the
    /// tracker here too (decision 47, W2). The call only resets an in-memory tracker. It makes
    /// no event, no charge and no random draw. The other modes do nothing.
    pub fn begin_evaluation_scope(&self) {
        // A journal that no final result settled belongs to an evaluation that ended without
        // one (P6 step 7b), so it is dropped.
        self.journal.clear();
        // The book of the ephemeral spaces starts empty (P7, VR-3). The sweep of the previous
        // root evaluation drained its channels.
        self.volatile.clear();
        match &self.state {
            RuntimeState::Host { store, .. } => store.begin_evaluation_scope(),
            RuntimeState::NativeReplay { bound } => {
                if let Some((store, _)) = bound.get() {
                    store.begin_evaluation_scope();
                }
            }
            RuntimeState::Detached => {}
        }
    }

    /// Records the ephemeral space `space` in the book of the current root evaluation. A
    /// durable space is not recorded. Since decision 83 the creation job records a space itself
    /// (`GenericSpace::prepare_create`, VR-2), so only the unit tests that build the book
    /// directly call this.
    pub fn record_ephemeral_space(&self, space: NativeSpace, qualifier: SpaceQualifier) {
        // Changed by P9 step 3 (decision 83): the registry holds the durability filter, which
        // the creation job shares.
        // if matches!(space.durability, namespace::Durability::Ephemeral) {
        //     self.volatile.record_space(space, qualifier);
        // }
        self.volatile.record_created(space, qualifier);
    }

    /// DG-1 for a space reference: an ephemeral space that the current root evaluation did not
    /// create belongs to an ended deploy and fails with `DanglingHandle`. A durable space
    /// passes; its first operation reads its config (DG-2).
    pub fn check_live_space(&self, space: NativeSpace) -> Result<(), SpaceError> {
        match space.durability {
            namespace::Durability::Durable => Ok(()),
            namespace::Durability::Ephemeral if self.volatile.is_live(&space.sid) => Ok(()),
            namespace::Durability::Ephemeral => {
                Err(SpaceError::DanglingHandle { space: space.sid })
            }
        }
    }

    /// DG-1 for a routed channel: a name of an ephemeral space that the current root
    /// evaluation did not create fails with `DanglingName`, and every other id of such a space
    /// (its handle, or a storage id) fails with `DanglingHandle`. The check reads only the book
    /// of the evaluation, so it costs nothing beyond the evaluation that reaches it (DG-4).
    pub fn check_live(&self, channel: &Par) -> Result<(), SpaceError> {
        let Some(id) = namespace::sole_gprivate_id(channel) else {
            return Ok(());
        };
        match ephemeral_space(id) {
            Some(space) if !self.volatile.is_live(&space.sid) => {
                Err(match namespace::decode_reified_id(id) {
                    Some(ReifiedId::Name(..)) => SpaceError::DanglingName { space: space.sid },
                    _ => SpaceError::DanglingHandle { space: space.sid },
                })
            }
            Some(_) | None => Ok(()),
        }
    }

    /// The qualifier of the ephemeral space `sid` when the current root evaluation created it.
    pub fn ephemeral_qualifier(&self, sid: &namespace::SpaceId) -> Option<SpaceQualifier> {
        self.volatile.qualifier(sid)
    }

    /// Sweeps the volatile channels of the root evaluation that ends (P7, VR-4 to VR-8). It
    /// runs after every detached task of the evaluation is done. With no volatile channel it
    /// takes no guard and calls nothing, so ordinary execution is unchanged (VR-5). Otherwise it
    /// takes the boundary guard of `coordinator`, as every store removal of the deterministic
    /// space does, so no checkpoint interleaves with it (VR-6). A failure of the sweep breaks an
    /// invariant of P7: it is a `BugFoundError`, which the evaluation classes `Platform` (VR-8).
    pub async fn sweep_volatile(
        &self,
        coordinator: &ReductionCoordinator,
    ) -> Result<(), InterpreterError> {
        if !self.volatile.has_channels() {
            return Ok(());
        }
        let channels = self.volatile.take_channels();
        let failed = |error: String| {
            InterpreterError::BugFoundError(format!("reified: the volatile sweep failed: {error}"))
        };
        let (store, _) = self.host().map_err(|error| failed(error.to_string()))?;
        let _boundary = coordinator.enter_boundary().await;
        store
            .sweep_volatile(&channels)
            .await
            .map(drop)
            .map_err(|error| failed(error.to_string()))
    }

    // The store and the budget of a runtime that runs space jobs: a host runtime, in legacy
    // execution and in native play, and a bound native replay runtime (decision 47).
    fn host(&self) -> Result<(ParSpaceStore, RuntimeBudget), SpaceError> {
        match &self.state {
            // Disabled by decision 47: native play runs spaces, and native replay runs them over
            // the native replay session.
            //
            // ```
            // RuntimeState::Host { budget, .. } if budget.native_execution_active() => {
            //     Err(SpaceError::SpacesUnsupportedInNativeExecution)
            // }
            // ```
            RuntimeState::Host { store, budget } => Ok((store.clone(), budget.clone())),
            RuntimeState::Detached => Err(SpaceError::RuntimeUnavailable),
            RuntimeState::NativeReplay { bound } => {
                bound.get().cloned().ok_or(SpaceError::RuntimeUnavailable)
            }
        }
    }

    /// Fails as every space entry point fails when this runtime cannot run a space job: in a
    /// detached runtime, and in a native replay runtime that is not bound yet.
    pub fn ensure_available(&self) -> Result<(), SpaceError> { self.host().map(drop) }

    /// The execution model of the deploy that the runtime evaluates (P7, design v3 QT-5):
    /// native when the budget of the runtime runs native execution, in native play and in
    /// native replay, and legacy otherwise.
    pub fn execution(&self) -> Result<Execution, SpaceError> {
        let (_, budget) = self.host()?;
        Ok(match budget.native_execution_active() {
            true => Execution::Native,
            false => Execution::Legacy,
        })
    }

    /// The `seq` spaces of the current root evaluation (P7, SQ-1), for the payload scan of
    /// SQ-2. With no `seq` space the set is empty and nothing is scanned.
    pub fn seq_spaces(&self) -> BTreeSet<namespace::SpaceId> {
        match self.volatile.has_seq_space() {
            true => self.volatile.seq_spaces(),
            false => BTreeSet::new(),
        }
    }

    /// Binds a native replay runtime to the space store of its native replay session and to the
    /// budget of its environment (decision 47, W2). The environment registers the runtime with
    /// its system processes before the session exists, so it binds the runtime once, right
    /// after it builds the session. A second bind, or the bind of another mode, is a bug.
    pub fn bind_native_replay(
        &self,
        store: ParSpaceStore,
        budget: RuntimeBudget,
    ) -> Result<(), InterpreterError> {
        match &self.state {
            RuntimeState::NativeReplay { bound } => bound.set((store, budget)).map_err(|_| {
                InterpreterError::BugFoundError(
                    "the native replay runtime of Reified RSpaces is already bound".to_owned(),
                )
            }),
            RuntimeState::Host { .. } | RuntimeState::Detached => {
                Err(InterpreterError::BugFoundError(
                    "only a native replay runtime binds a native replay session".to_owned(),
                ))
            }
        }
    }

    /// The environment of a space job.
    pub fn space_env(&self) -> Result<SpaceEnv, SpaceError> {
        let (store, budget) = self.host()?;
        Ok(SpaceEnv {
            store,
            budget,
            volatile: Arc::clone(&self.volatile),
            origin: OpOrigin::Program,
        })
    }

    /// Runs a collection operation with `agent` as an `Intent::Space` job: one transaction and
    /// one commit. The footprint holds the space key and `SpaceOp::footprint_authority`.
    /// `origin` tells the Seq rules whether the program started the operation or the runtime
    /// re-issues it (P7, SQ-3).
    pub async fn run_op(
        &self,
        agent: Arc<dyn SpaceAgent>,
        op: SpaceOp,
        origin: OpOrigin,
    ) -> Result<SpaceOutcome, InterpreterError> {
        let (store, budget) = self.host()?;
        let volatile = Arc::clone(&self.volatile);
        let sid = op.space().sid;
        let authority = op.footprint_authority();
        let journal = Arc::clone(&self.journal);
        submit_space(SpaceRequest {
            sid,
            authority,
            channels: Vec::new(),
            job: Box::new(move || {
                Box::pin(async move {
                    let env = SpaceEnv {
                        store,
                        budget,
                        volatile,
                        origin,
                    };
                    let mut outcome = agent::run_op(agent.as_ref(), &env, op).await?;
                    if let Some(writes) = outcome.mirror.take() {
                        journal.record(writes);
                    }
                    Ok(outcome)
                })
            }),
        })
        .await
    }

    /// Settles the mirror journal of the evaluation with its final result (M1, decision 66): a
    /// result with no errors publishes the journal to the mirror writer of the node, and any
    /// other result drops it. Publication never waits.
    pub fn settle_mirror_journal(&self, result: &Result<EvaluateResult, InterpreterError>) {
        let batches = self.journal.take();
        if matches!(result, Ok(evaluation) if evaluation.errors.is_empty()) && !batches.is_empty() {
            vectordb_services::current().publish(batches);
        }
    }

    /// True when the mirror journal of the evaluation holds no write.
    pub fn mirror_journal_is_empty(&self) -> bool { self.journal.is_empty() }

    /// Creates a space with `agent` as an `Intent::Space` job (P4 design C.7, steps 4 to 7).
    /// The footprint also holds `H(sid)`, and its authority is the caller authority.
    pub async fn create_space(
        &self,
        agent: Arc<dyn SpaceAgent>,
        creation: SpaceCreation,
    ) -> Result<(), InterpreterError> {
        let (store, budget) = self.host()?;
        let volatile = Arc::clone(&self.volatile);
        let space = creation.space;
        let authority = match creation.authority.regions.is_empty() {
            true => None,
            false => Some(creation.authority.clone()),
        };
        submit_space(SpaceRequest {
            sid: space.sid,
            authority,
            channels: vec![namespace::gprivate(namespace::handle_channel(space))],
            job: Box::new(move || {
                Box::pin(async move {
                    let env = SpaceEnv {
                        store,
                        budget,
                        volatile,
                        origin: OpOrigin::Program,
                    };
                    let prepared = agent.prepare_create(&env, creation).await?;
                    Ok(prepared.commit().await?)
                })
            }),
        })
        .await
    }

    /// Allocates the index names of `op` with `agent` as an `Intent::Space` job (P5d, F11): the
    /// names of a `new` inside a `use` block of an Array or Vector space, or of its handle
    /// method `gensym`. An outer store without an allocator answers [`Allocation::Baseline`]:
    /// the job commits nothing, and the result is `None`, so the reducer draws the names as it
    /// does for every `new`. Native play and native replay run the job as every space entry
    /// runs it (decision 47).
    pub async fn allocate(
        &self,
        agent: Arc<dyn SpaceAgent>,
        op: AllocOp,
    ) -> Result<Option<Vec<u64>>, InterpreterError> {
        let (store, budget) = self.host()?;
        let volatile = Arc::clone(&self.volatile);
        let sid = op.space.sid;
        let authority = match op.authority.regions.is_empty() {
            true => None,
            false => Some(op.authority.clone()),
        };
        submit_space(SpaceRequest {
            sid,
            authority,
            channels: Vec::new(),
            job: Box::new(move || {
                Box::pin(async move {
                    let env = SpaceEnv {
                        store,
                        budget,
                        volatile,
                        origin: OpOrigin::Program,
                    };
                    match agent.allocate(&env, op).await? {
                        Allocation::Baseline => Ok(None),
                        Allocation::Indices(prepared) => Ok(Some(prepared.commit().await?)),
                    }
                })
            }),
        })
        .await
    }

    /// Reads the config map of `space` with `agent` as an `Intent::Space` job. The job
    /// commits nothing (readiness review S3). `authority` is the authority of the footprint:
    /// the cost authority of the call.
    pub async fn read_config(
        &self,
        agent: Arc<dyn SpaceAgent>,
        space: NativeSpace,
        authority: Option<CostAuthority>,
    ) -> Result<Par, InterpreterError> {
        let (store, budget) = self.host()?;
        let volatile = Arc::clone(&self.volatile);
        submit_space(SpaceRequest {
            sid: space.sid,
            authority,
            channels: Vec::new(),
            job: Box::new(move || {
                Box::pin(async move {
                    let env = SpaceEnv {
                        store,
                        budget,
                        volatile,
                        origin: OpOrigin::Program,
                    };
                    Ok(agent.read_config(&env, space).await?)
                })
            }),
        })
        .await
    }
}

/// A `Host` runtime over the concrete host space `host` (`RSpace`, `ReplayRSpace` or
/// `ReportingRspace`) and the phlo budget of the runtime.
pub fn for_host<T>(host: T, budget: RuntimeBudget) -> Arc<ReifiedRuntime>
where T: SpaceTxnHost<Par, BindPattern, ListParWithRandom, TaggedContinuation> + 'static {
    Arc::new(ReifiedRuntime {
        state: RuntimeState::Host {
            store: SpaceStoreHandle::new(host),
            budget,
        },
        journal: Arc::new(MirrorJournal::new()),
        volatile: Arc::new(VolatileRegistry::default()),
    })
}

/// A `Detached` runtime.
pub fn unavailable() -> Arc<ReifiedRuntime> {
    Arc::new(ReifiedRuntime {
        state: RuntimeState::Detached,
        journal: Arc::new(MirrorJournal::new()),
        volatile: Arc::new(VolatileRegistry::default()),
    })
}

/// A `NativeReplay` runtime that is not bound yet (decision 47).
/// [`ReifiedRuntime::bind_native_replay`] binds it to the store of the native replay session.
pub fn native_replay() -> Arc<ReifiedRuntime> {
    Arc::new(ReifiedRuntime {
        state: RuntimeState::NativeReplay {
            bound: OnceLock::new(),
        },
        journal: Arc::new(MirrorJournal::new()),
        volatile: Arc::new(VolatileRegistry::default()),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use async_trait::async_trait;
    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rust::space_factory_urn::SpaceFactoryUrn;
    use models::rust::utils::{new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::operation_context::{self, CausalPath, OperationOrder};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::{
        ProduceOutcome, SpaceMode, SpaceProduce, SpaceTxnError,
    };

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::deterministic_reduction::{root, ReductionCoordinator};
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::rho_runtime::RhoISpace;
    use crate::rust::interpreter::spaces::agent::{
        ConsumeOp, JoinOp, Prepared, ProduceOp, SettleOp,
    };
    use crate::rust::interpreter::spaces::namespace::{
        CollectionKey, Durability, NameTail, SpaceId, SID_LEN,
    };

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    fn budget() -> RuntimeBudget { RuntimeBudget::new(Cost::create(1_000, "reified runtime test")) }

    fn space() -> NativeSpace {
        NativeSpace {
            sid: [1; SID_LEN],
            durability: Durability::Durable,
        }
    }

    async fn rspace() -> ParRSpace {
        let mut stores = InMemoryStoreManager::new();
        RSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space")
    }

    fn datum(par: Par) -> ListParWithRandom {
        ListParWithRandom {
            pars: vec![par],
            random_state: vec![2],
            cost_authority: None,
            cost_stack: None,
        }
    }

    fn produce_op(par: Par) -> SpaceOp {
        SpaceOp::Produce(ProduceOp {
            space: space(),
            key: CollectionKey::from_bytes(vec![1]).expect("a key"),
            channel: new_gint_par(1, Vec::new(), false),
            data: datum(par),
            persistent: false,
            params: BTreeMap::new(),
            authority: CostAuthority::default(),
            reissued_ord: None,
        })
    }

    /// An agent that keeps one datum on the config channel: a produce or a creation stores
    /// it, and `read_config` returns its first par.
    struct ConfigAgent;

    impl ConfigAgent {
        async fn store<T: Send + 'static>(
            env: &SpaceEnv,
            space: NativeSpace,
            data: ListParWithRandom,
            result: T,
        ) -> Result<Prepared<T>, SpaceError> {
            let channel = namespace::gprivate(namespace::config_channel(space));
            let mut txn = env.store.begin(std::slice::from_ref(&channel)).await?;
            txn.produce(SpaceProduce {
                channel,
                data,
                persist: true,
                outcome: ProduceOutcome::Store,
                before_observe: None,
            })?;
            Ok(Prepared::new(txn, move |_| Ok(result)))
        }
    }

    #[async_trait]
    impl SpaceAgent for ConfigAgent {
        async fn prepare_produce(
            &self,
            env: &SpaceEnv,
            op: ProduceOp,
        ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
            Self::store(env, op.space, op.data, SpaceOutcome::default()).await
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
            env: &SpaceEnv,
            creation: SpaceCreation,
        ) -> Result<Prepared<()>, SpaceError> {
            Self::store(env, creation.space, datum(creation.config), ()).await
        }

        async fn read_config(&self, env: &SpaceEnv, space: NativeSpace) -> Result<Par, SpaceError> {
            let channel = namespace::gprivate(namespace::config_channel(space));
            let mut txn = env.store.begin(std::slice::from_ref(&channel)).await?;
            match txn.data(&channel)?.first() {
                Some(datum) => datum
                    .a
                    .pars
                    .first()
                    .cloned()
                    .ok_or(SpaceError::Inconsistent("an empty config datum")),
                None => Err(SpaceError::DanglingHandle { space: space.sid }),
            }
        }

        fn gensym(
            &self,
            space: NativeSpace,
            _random: &Blake2b512Random,
        ) -> Result<Par, SpaceError> {
            Ok(namespace::gprivate(namespace::name_id(
                space,
                NameTail::Index(0),
            )))
        }
    }

    fn alloc_op() -> AllocOp {
        AllocOp {
            space: space(),
            count: 2,
            random_state: vec![1],
            authority: CostAuthority::default(),
        }
    }

    fn creation(config: Par, authority: CostAuthority) -> SpaceCreation {
        SpaceCreation {
            space: space(),
            config,
            handle_pattern: BindPattern::default(),
            handle_continuation: TaggedContinuation::default(),
            random: Blake2b512Random::create_from_bytes(b"factory call"),
            authority,
            family: SpaceFactoryUrn::HASHMAP_QUEUE,
        }
    }

    async fn fails_closed(runtime: Arc<ReifiedRuntime>, code: &str) {
        runtime.begin_evaluation_scope();
        let agent: Arc<dyn SpaceAgent> = Arc::new(ConfigAgent);
        let expected = |message: String| {
            assert!(
                message.starts_with(&format!("reified:{code}: ")),
                "{message}"
            );
        };
        for result in [
            runtime
                .run_op(
                    Arc::clone(&agent),
                    produce_op(Par::default()),
                    OpOrigin::Program,
                )
                .await
                .map(|_| ()),
            runtime
                .create_space(
                    Arc::clone(&agent),
                    creation(Par::default(), CostAuthority::default()),
                )
                .await,
            runtime
                .read_config(Arc::clone(&agent), space(), None)
                .await
                .map(|_| ()),
            runtime
                .allocate(Arc::clone(&agent), alloc_op())
                .await
                .map(|_| ()),
        ] {
            match result {
                Err(InterpreterError::ReduceError(message)) => expected(message),
                other => panic!("a space job ran in a runtime without spaces: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn detached_runtime_fails_closed() {
        let runtime = unavailable();
        assert_eq!(runtime.mode(), ReifiedRuntimeMode::Detached);
        assert_eq!(
            runtime.space_env().err(),
            Some(SpaceError::RuntimeUnavailable)
        );
        assert_eq!(
            runtime.ensure_available(),
            Err(SpaceError::RuntimeUnavailable)
        );
        fails_closed(runtime, "RuntimeUnavailable").await;
    }

    // Decision 47: a native replay runtime that is not bound yet has no store, so every space
    // entry fails closed as in a detached runtime.
    #[tokio::test]
    async fn native_replay_runtime_fails_closed() {
        let runtime = native_replay();
        assert_eq!(runtime.mode(), ReifiedRuntimeMode::NativeReplay);
        assert_eq!(
            runtime.space_env().err(),
            Some(SpaceError::RuntimeUnavailable)
        );
        assert_eq!(
            runtime.ensure_available(),
            Err(SpaceError::RuntimeUnavailable)
        );
        fails_closed(runtime, "RuntimeUnavailable").await;
    }

    // Decision 47, W2: the environment binds a native replay runtime once. The bound runtime
    // runs space jobs over its store, and `begin_evaluation_scope` resets the committed parents
    // of that store, because a restore of the environment keeps the store.
    #[tokio::test]
    async fn native_replay_runtime_binds_once_and_resets_its_scope() {
        let runtime = native_replay();
        let store = SpaceStoreHandle::new(rspace().await);
        runtime
            .bind_native_replay(store.clone(), budget())
            .expect("the first bind succeeds");
        assert!(matches!(
            runtime.bind_native_replay(store, budget()),
            Err(InterpreterError::BugFoundError(_))
        ));
        assert!(matches!(
            unavailable().bind_native_replay(SpaceStoreHandle::new(rspace().await), budget()),
            Err(InterpreterError::BugFoundError(_))
        ));
        assert_eq!(runtime.mode(), ReifiedRuntimeMode::NativeReplay);
        assert_eq!(runtime.ensure_available(), Ok(()));
        let env = runtime
            .space_env()
            .expect("a bound native replay runtime has a space environment");
        let lock = namespace::gprivate(namespace::config_channel(space()));
        let order = OperationOrder {
            session: [5; 32],
            path: CausalPath::from(vec![(0, 0)]),
        };
        let commit = || {
            let store = env.store.clone();
            let lock = lock.clone();
            operation_context::scope(order.clone(), async move {
                let mut txn = store.begin(std::slice::from_ref(&lock)).await?;
                txn.commit().await.map(|receipt| receipt.ops.len())
            })
        };
        assert_eq!(commit().await, Ok(0));
        assert_eq!(commit().await, Err(SpaceTxnError::ParentOrderReused));
        runtime.begin_evaluation_scope();
        assert_eq!(commit().await, Ok(0));
    }

    #[tokio::test]
    async fn host_runtime_is_available_outside_native_execution() {
        let runtime = for_host(rspace().await, budget());
        assert_eq!(runtime.ensure_available(), Ok(()));
        assert!(runtime.space_env().is_ok());
    }

    #[tokio::test]
    async fn host_runtime_begin_evaluation_scope_resets_the_committed_parents() {
        let runtime = for_host(rspace().await, budget());
        assert_eq!(runtime.mode(), ReifiedRuntimeMode::Host);
        let env = runtime
            .space_env()
            .expect("a host runtime has a space environment");
        assert_eq!(env.store.mode(), SpaceMode::Play);
        let lock = namespace::gprivate(namespace::config_channel(space()));
        let order = OperationOrder {
            session: [4; 32],
            path: CausalPath::from(vec![(0, 0)]),
        };
        let commit = || {
            let store = env.store.clone();
            let lock = lock.clone();
            operation_context::scope(order.clone(), async move {
                let mut txn = store.begin(std::slice::from_ref(&lock)).await?;
                txn.commit().await.map(|receipt| receipt.ops.len())
            })
        };
        assert_eq!(commit().await, Ok(0));
        assert_eq!(commit().await, Err(SpaceTxnError::ParentOrderReused));
        runtime.begin_evaluation_scope();
        assert_eq!(commit().await, Ok(0));
    }

    #[tokio::test]
    async fn host_runtime_runs_space_jobs_under_the_scheduler() {
        let host = rspace().await;
        let session: RhoISpace = Arc::new(Box::new(host.clone()));
        let runtime = for_host(host, budget());
        let agent: Arc<dyn SpaceAgent> = Arc::new(ConfigAgent);
        let config = new_gstring_par("config".to_string(), Vec::new(), false);
        let outcome = root(session, budget(), ReductionCoordinator::default(), async {
            runtime
                .create_space(
                    Arc::clone(&agent),
                    creation(config.clone(), CostAuthority::default()),
                )
                .await?;
            let read = runtime
                .read_config(Arc::clone(&agent), space(), None)
                .await?;
            let outcome = runtime
                .run_op(
                    Arc::clone(&agent),
                    produce_op(new_gint_par(5, Vec::new(), false)),
                    OpOrigin::Program,
                )
                .await?;
            // An agent without an allocator answers the baseline, and the job commits nothing.
            let allocated = runtime.allocate(Arc::clone(&agent), alloc_op()).await?;
            Ok::<_, InterpreterError>((read, outcome, allocated))
        })
        .await
        .expect("the space jobs run");
        assert_eq!(outcome, (config, SpaceOutcome::default(), None));
        let missing = NativeSpace {
            sid: [2; SID_LEN],
            durability: Durability::Durable,
        };
        match runtime.read_config(agent, missing, None).await {
            Err(InterpreterError::ReduceError(message)) => {
                assert!(message.starts_with("reified:DanglingHandle: "), "{message}");
            }
            other => panic!("a missing config read returned {other:?}"),
        }
    }

    /// `dead_ephemeral_dangling` (P7-2 of VolatileSweep.v, design v3 DG-1): an id of an
    /// ephemeral space that the evaluation did not create fails, a name with `DanglingName` and
    /// every other id with `DanglingHandle`, and the ids of a created space and of every durable
    /// space pass. A new root evaluation forgets the created spaces.
    #[test]
    fn prop_dead_ephemeral_dangling() {
        use std::collections::BTreeSet;

        use proptest::collection::btree_set;
        use proptest::prelude::{any, prop_assert, prop_assert_eq};
        use proptest::test_runner::{Config, RngSeed, TestRunner};

        let sids = btree_set(any::<[u8; SID_LEN]>(), 2..6);
        let mut runner = TestRunner::new(Config {
            cases: 64,
            rng_seed: RngSeed::Fixed(0x0E0D_2026_1009_0007),
            failure_persistence: None,
            ..Config::default()
        });
        runner
            .run(&(sids, 1usize..5), |(sids, created)| {
                let runtime = unavailable();
                let sids: Vec<SpaceId> = sids.into_iter().collect();
                let created = created.min(sids.len() - 1);
                let (live, dead) = sids.split_at(created);
                for sid in live {
                    let space = NativeSpace {
                        sid: *sid,
                        durability: Durability::Ephemeral,
                    };
                    runtime.record_ephemeral_space(space, SpaceQualifier::Temp);
                }
                let live: BTreeSet<SpaceId> = live.iter().copied().collect();
                for sid in &sids {
                    for durability in [Durability::Ephemeral, Durability::Durable] {
                        let space = NativeSpace {
                            sid: *sid,
                            durability,
                        };
                        let passes = durability == Durability::Durable || live.contains(sid);
                        let ids = [
                            (namespace::handle_channel(space), false),
                            (namespace::config_channel(space), false),
                            (
                                namespace::data_channel(
                                    space,
                                    &CollectionKey::from_bytes(vec![7]).expect("a key"),
                                ),
                                false,
                            ),
                            (namespace::name_id(space, NameTail::Random([3; 32])), true),
                            (namespace::name_id(space, NameTail::Index(1)), true),
                        ];
                        for (id, is_name) in ids {
                            let checked = runtime.check_live(&namespace::gprivate(id));
                            let expected = match (passes, is_name) {
                                (true, _) => Ok(()),
                                (false, true) => Err(SpaceError::DanglingName { space: *sid }),
                                (false, false) => Err(SpaceError::DanglingHandle { space: *sid }),
                            };
                            prop_assert_eq!(checked, expected);
                        }
                        let expected = match passes {
                            true => Ok(()),
                            false => Err(SpaceError::DanglingHandle { space: *sid }),
                        };
                        prop_assert_eq!(runtime.check_live_space(space), expected);
                    }
                }
                prop_assert!(dead.iter().all(|sid| !live.contains(sid)));
                runtime.begin_evaluation_scope();
                for sid in live {
                    let space = NativeSpace {
                        sid,
                        durability: Durability::Ephemeral,
                    };
                    prop_assert_eq!(
                        runtime.check_live_space(space),
                        Err(SpaceError::DanglingHandle { space: sid })
                    );
                }
                Ok(())
            })
            .expect("the live check of DG-1 holds");
    }

    /// The seq space of [`prop_seq_gate_constant_per_window`].
    const SEQ_SPACE: NativeSpace = NativeSpace {
        sid: [5; SID_LEN],
        durability: Durability::Ephemeral,
    };

    /// One evaluation of [`prop_seq_gate_constant_per_window`]: `count` participants, the
    /// participant `creator` creates [`SEQ_SPACE`] with the native agent, and each other
    /// participant submits one empty job, so all jobs share one frontier. Each participant
    /// sleeps for the milliseconds of its pair before its job and after it, and reads the seq
    /// spaces before its job and after it.
    async fn seq_views(
        count: usize,
        creator: usize,
        sleeps: Vec<(u64, u64)>,
    ) -> Vec<(BTreeSet<SpaceId>, BTreeSet<SpaceId>)> {
        use std::time::Duration;

        use crate::rust::interpreter::deterministic_reduction::{current, scope, ParticipantGuard};
        use crate::rust::interpreter::reified::factory::{handle_continuation, handle_pattern};
        use crate::rust::interpreter::reified::system_processes::native_agent;
        use crate::rust::interpreter::spaces::types::config::SpaceConfig;

        let budget = || RuntimeBudget::new(Cost::create(1_000_000, "seq gate test"));
        let host = rspace().await;
        let session: RhoISpace = Arc::new(Box::new(host.clone()));
        let runtime = for_host(host, budget());
        runtime.begin_evaluation_scope();
        root(session, budget(), ReductionCoordinator::default(), async {
            let parent = current().expect("root reduction context");
            let children = parent.split(count);
            let mut participants = Vec::with_capacity(count);
            for (index, (child, (before, after))) in children.into_iter().zip(sleeps).enumerate() {
                let runtime = Arc::clone(&runtime);
                participants.push(tokio::spawn(scope(child.clone(), async move {
                    let _guard = ParticipantGuard::for_context(&child);
                    tokio::time::sleep(Duration::from_millis(before)).await;
                    let seen_before = runtime.seq_spaces();
                    match index == creator {
                        true => {
                            let random = Blake2b512Random::create_from_bytes(b"seq gate");
                            let config = SpaceConfig::new(
                                SEQ_SPACE.sid,
                                "hashmap",
                                "queue",
                                SpaceQualifier::Seq,
                            )
                            .to_par();
                            runtime
                                .create_space(native_agent(), SpaceCreation {
                                    space: SEQ_SPACE,
                                    config,
                                    handle_pattern: handle_pattern(),
                                    handle_continuation: handle_continuation(SEQ_SPACE, &random),
                                    random,
                                    authority: CostAuthority::default(),
                                    family: SpaceFactoryUrn::HASHMAP_QUEUE,
                                })
                                .await
                                .expect("the seq space is created");
                        }
                        false => {
                            let byte = u8::try_from(index).expect("a small participant index");
                            submit_space(SpaceRequest {
                                sid: [byte; SID_LEN],
                                authority: None,
                                channels: Vec::new(),
                                job: Box::new(|| Box::pin(async { Ok(()) })),
                            })
                            .await
                            .expect("the empty job runs");
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(after)).await;
                    (seen_before, runtime.seq_spaces())
                })));
            }
            let mut seen = Vec::with_capacity(count);
            for participant in participants {
                seen.push(participant.await.expect("the participant task joins"));
            }
            parent.rejoin();
            seen
        })
        .await
    }

    /// `ScanViewPerWindow` (`SpaceCoordination.tla`, decision 83): the book of the seq spaces
    /// changes only inside a job, so every read of one scheduling window sees one set. Generated
    /// participants share one frontier with the creation job of a seq space, which reaches
    /// `GenericSpace::prepare_create` through the native agent, with generated sleeps before and
    /// after their jobs. Every read before that frontier sees no seq space, and every read after
    /// it sees the new space.
    #[test]
    fn prop_seq_gate_constant_per_window() {
        use std::time::Duration;

        use proptest::collection::vec;
        use proptest::prelude::{any, prop_assert, prop_assert_eq};
        use proptest::test_runner::{Config, RngSeed, TestRunner};

        let mut runner = TestRunner::new(Config {
            cases: 32,
            rng_seed: RngSeed::Fixed(0x0E0D_2026_1012_0083),
            failure_persistence: None,
            ..Config::default()
        });
        let tokio = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("a tokio runtime");
        runner
            .run(
                &(2usize..=4, any::<usize>(), vec((0u64..3, 0u64..3), 4)),
                |(count, creator, sleeps)| {
                    let creator = creator % count;
                    let sleeps: Vec<(u64, u64)> = sleeps.into_iter().take(count).collect();
                    let seen = tokio.block_on(async {
                        tokio::time::timeout(
                            Duration::from_secs(60),
                            seq_views(count, creator, sleeps),
                        )
                        .await
                        .expect("the participants finish")
                    });
                    prop_assert_eq!(seen.len(), count);
                    for (index, (before, after)) in seen.into_iter().enumerate() {
                        prop_assert!(
                            before.is_empty(),
                            "participant {} read {:?} before the frontier",
                            index,
                            before
                        );
                        prop_assert_eq!(
                            after,
                            BTreeSet::from([SEQ_SPACE.sid]),
                            "participant {} after the frontier",
                            index
                        );
                    }
                    Ok(())
                },
            )
            .expect("every read of one window sees one set of seq spaces");
    }
}
