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
//! Each space operation runs as an `Intent::Space` job (readiness review S9) with one
//! transaction and at most one commit (S3). The job gets the host-work budget that the
//! scheduler captured before it submitted the job (S5).

use std::sync::{Arc, OnceLock};

use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::space_store_handle::{SpaceStoreHandle, SpaceTxnHost};

use super::costs::ScanMeter;
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::deterministic_reduction::reified_schedule::{
    submit_space, SpaceRequest,
};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::host_work::HostWorkBudget;
use crate::rust::interpreter::spaces::agent::{
    self, AllocOp, Allocation, ParSpaceStore, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOp,
    SpaceOutcome,
};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::namespace::{self, NativeSpace};

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

    // The store and the budget of a runtime that runs space jobs: a host runtime, in legacy
    // execution and in native play, and a bound native replay runtime (decision 47).
    fn host(&self) -> Result<(ParSpaceStore, RuntimeBudget), SpaceError> {
        match &self.state {
            // Disabled by decision 47: native play runs spaces, and its transactions read
            // through the metered reads of the substrate (`SpaceEnv::begin`). Native replay
            // runs them over the native replay session.
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

    /// The environment of a space job. `host_work` is the host-work budget of the deploy that
    /// the scheduler captured for the job.
    pub fn space_env(&self, host_work: Option<HostWorkBudget>) -> Result<SpaceEnv, SpaceError> {
        let (store, budget) = self.host()?;
        Ok(SpaceEnv {
            store,
            budget,
            meter: ScanMeter::new(host_work),
        })
    }

    /// Runs a collection operation with `agent` as an `Intent::Space` job: one transaction and
    /// one commit. The footprint holds the space key and `SpaceOp::footprint_authority`.
    pub async fn run_op(
        &self,
        agent: Arc<dyn SpaceAgent>,
        op: SpaceOp,
    ) -> Result<SpaceOutcome, InterpreterError> {
        let (store, budget) = self.host()?;
        let sid = op.space().sid;
        let authority = op.footprint_authority();
        submit_space(SpaceRequest {
            sid,
            authority,
            channels: Vec::new(),
            job: Box::new(move |host_work| {
                Box::pin(async move {
                    let env = SpaceEnv {
                        store,
                        budget,
                        meter: ScanMeter::new(host_work),
                    };
                    Ok(agent::run_op(agent.as_ref(), &env, op).await?)
                })
            }),
        })
        .await
    }

    /// Creates a space with `agent` as an `Intent::Space` job (P4 design C.7, steps 4 to 7).
    /// The footprint also holds `H(sid)`, and its authority is the caller authority.
    pub async fn create_space(
        &self,
        agent: Arc<dyn SpaceAgent>,
        creation: SpaceCreation,
    ) -> Result<(), InterpreterError> {
        let (store, budget) = self.host()?;
        let space = creation.space;
        let authority = match creation.authority.regions.is_empty() {
            true => None,
            false => Some(creation.authority.clone()),
        };
        submit_space(SpaceRequest {
            sid: space.sid,
            authority,
            channels: vec![namespace::gprivate(namespace::handle_channel(space))],
            job: Box::new(move |host_work| {
                Box::pin(async move {
                    let env = SpaceEnv {
                        store,
                        budget,
                        meter: ScanMeter::new(host_work),
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
        let sid = op.space.sid;
        let authority = match op.authority.regions.is_empty() {
            true => None,
            false => Some(op.authority.clone()),
        };
        submit_space(SpaceRequest {
            sid,
            authority,
            channels: Vec::new(),
            job: Box::new(move |host_work| {
                Box::pin(async move {
                    let env = SpaceEnv {
                        store,
                        budget,
                        meter: ScanMeter::new(host_work),
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
        submit_space(SpaceRequest {
            sid: space.sid,
            authority,
            channels: Vec::new(),
            job: Box::new(move |host_work| {
                Box::pin(async move {
                    let env = SpaceEnv {
                        store,
                        budget,
                        meter: ScanMeter::new(host_work),
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
    })
}

/// A `Detached` runtime.
pub fn unavailable() -> Arc<ReifiedRuntime> {
    Arc::new(ReifiedRuntime {
        state: RuntimeState::Detached,
    })
}

/// A `NativeReplay` runtime that is not bound yet (decision 47).
/// [`ReifiedRuntime::bind_native_replay`] binds it to the store of the native replay session.
pub fn native_replay() -> Arc<ReifiedRuntime> {
    Arc::new(ReifiedRuntime {
        state: RuntimeState::NativeReplay {
            bound: OnceLock::new(),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use async_trait::async_trait;
    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rust::host_work::{
        HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkUsage,
    };
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
        CollectionKey, Durability, NameTail, SID_LEN,
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
                None => Err(SpaceError::DanglingHandle),
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
                .run_op(Arc::clone(&agent), produce_op(Par::default()))
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
            runtime.space_env(None).err(),
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
            runtime.space_env(None).err(),
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
            .space_env(None)
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
        assert!(runtime.space_env(None).is_ok());
    }

    #[tokio::test]
    async fn host_runtime_begin_evaluation_scope_resets_the_committed_parents() {
        let runtime = for_host(rspace().await, budget());
        assert_eq!(runtime.mode(), ReifiedRuntimeMode::Host);
        let host_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(8)));
        let env = runtime
            .space_env(Some(host_work.clone()))
            .expect("a host runtime has a space environment");
        assert_eq!(env.store.mode(), SpaceMode::Play);
        env.meter.charge(1, 0, 0).expect("one operation fits");
        assert_eq!(
            host_work.usage(HostWorkDimension::VerificationOperations),
            HostWorkUsage::new(1)
        );
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
}
