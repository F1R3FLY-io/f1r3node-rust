//! Space operations under the deterministic scheduler (P4 design C.4, readiness review S5
//! and S9).
//!
//! This file is the child module `reified_schedule` of `deterministic_reduction.rs`, so it uses
//! the private items of the scheduler. `reified/mod.rs` does not declare it.
//!
//! Every space job runs as an `Intent::Space`. Its footprint is `[2]‖sid`, the prost key of
//! each extra channel, and the authority keys, so the scheduler orders the jobs of one space
//! and runs the jobs of other spaces in parallel. The driver polls the conflict components as
//! futures of one task, and the substrate admits one open transaction for each task (FR-9).
//! So each job runs in its own task, inside `operation_context::scope` of its order. A job
//! that a single participant claims directly runs inline, as a host operation does.

use std::any::Any;
use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use futures::future::BoxFuture;
use models::rhoapi::{CostAuthority, Par};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::operation_context::{self, OperationOrder};
use tokio::sync::oneshot;

use super::{
    current, insert_authority, insert_channel, internal_scope, poll_driver_once, Completion,
    DirectExecutionGuard, Intent, ReductionContext, ReductionSession,
};
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::spaces::namespace::{SpaceId, SID_LEN};

/// The footprint tag of a space key. Host channel keys use 0 and authority keys use 1.
pub(crate) const SPACE_FOOTPRINT_TAG: u8 = 2;

/// The work of a space job.
pub(crate) type SpaceJob<T> =
    Box<dyn FnOnce() -> BoxFuture<'static, Result<T, InterpreterError>> + Send>;

/// A space job to schedule.
pub(crate) struct SpaceRequest<T> {
    /// The space of the job. The footprint holds `[2]‖sid`.
    pub(crate) sid: SpaceId,
    /// The authority of the footprint, as `prepare` takes it for a host intent.
    pub(crate) authority: Option<CostAuthority>,
    /// Default-space channels that the job also touches. The factory passes `H(sid)`.
    pub(crate) channels: Vec<Par>,
    /// The work. It runs in its own task, so it must not wait for another job.
    pub(crate) job: SpaceJob<T>,
}

type ErasedOutput = Box<dyn Any + Send>;

type SpaceJobResult = Result<ErasedOutput, InterpreterError>;

type ErasedJob = Box<dyn FnOnce() -> BoxFuture<'static, SpaceJobResult> + Send>;

/// `Intent::Space`: a submitted space job and the channel of its reply.
pub(crate) struct SpaceIntent {
    sid: SpaceId,
    authority: Option<CostAuthority>,
    channels: Vec<Par>,
    job: ErasedJob,
    reply: oneshot::Sender<SpaceJobResult>,
}

/// `Completion::Space`: the result of a space job and the channel of its reply.
pub(crate) struct SpaceDelivery {
    reply: oneshot::Sender<SpaceJobResult>,
    result: SpaceJobResult,
}

impl SpaceDelivery {
    /// Sends the result. A submitter that stopped waiting drops it.
    pub(super) fn deliver(self) { let _ = self.reply.send(self.result); }
}

impl SpaceIntent {
    /// The footprint of the job: `[2]‖sid`, `[0]‖prost(c)` for each extra channel, and the
    /// authority keys, as `prepare` adds them for a host intent.
    pub(super) fn extend_footprint(
        &self,
        footprint: &mut BTreeSet<Vec<u8>>,
        budget: &RuntimeBudget,
    ) {
        let mut key = Vec::with_capacity(1 + SID_LEN);
        key.push(SPACE_FOOTPRINT_TAG);
        key.extend_from_slice(&self.sid);
        footprint.insert(key);
        for channel in &self.channels {
            insert_channel(footprint, channel);
        }
        insert_authority(footprint, self.authority.as_ref(), budget);
    }

    /// The delivery of a job that the scheduler rejects before it runs.
    pub(super) fn reject(self, error: RSpaceError) -> SpaceDelivery {
        SpaceDelivery {
            reply: self.reply,
            result: Err(InterpreterError::from(error)),
        }
    }

    /// Runs the job in its own task, inside `operation_context::scope(order)` (readiness review
    /// S9). The task is registered with the session's tasks for the time it runs, so a canceled
    /// root aborts it as it aborts the detached reductions. A join error is a `BugFoundError`.
    pub(super) async fn execute(
        self,
        session: &ReductionSession,
        order: OperationOrder,
    ) -> Completion {
        let SpaceIntent { job, reply, .. } = self;
        let task = tokio::spawn(operation_context::scope(order.clone(), job()));
        let task_id = session.next_detached_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut tasks = session
                .detached_tasks
                .lock()
                .expect("detached reduction tasks lock");
            match tasks.canceled {
                true => task.abort(),
                false => {
                    tasks.handles.insert(task_id, task.abort_handle());
                }
            }
        }
        let joined = task.await;
        session
            .detached_tasks
            .lock()
            .expect("detached reduction tasks lock")
            .handles
            .remove(&task_id);
        let result = match joined {
            Ok(result) => result,
            Err(error) => Err(InterpreterError::BugFoundError(format!(
                "deterministic space operation failed: {error}"
            ))),
        };
        Completion::Space(order, SpaceDelivery { reply, result })
    }
}

impl ReductionSession {
    // Mirrors `submit_produce`: the next order of the participant, then a direct claim or an
    // intent of the frontier.
    async fn submit_space(
        self: &Arc<Self>,
        context: &ReductionContext,
        sid: SpaceId,
        authority: Option<CostAuthority>,
        channels: Vec<Par>,
        job: ErasedJob,
    ) -> SpaceJobResult {
        let order = context.next_operation();
        if self.claim_direct(&context.participant) {
            let _direct = DirectExecutionGuard::new(self.clone(), context.participant.clone());
            return internal_scope(operation_context::scope(order, job())).await;
        }
        let (reply, receive) = oneshot::channel();
        let drive = self.submit(
            &context.participant,
            order,
            Intent::Space(SpaceIntent {
                sid,
                authority,
                channels,
                job,
                reply,
            }),
        );
        if drive {
            let _ = poll_driver_once(internal_scope(self.clone().drive())).await;
        }
        receive.await.map_err(|_| {
            InterpreterError::BugFoundError(
                "deterministic space operation was cancelled".to_string(),
            )
        })?
    }
}

/// Runs a space job under the scheduler and returns its output (P4 design C.4).
///
/// Inside a reduction the job gets the next operation order of the participant. A single
/// participant claims it directly and runs it inline. Otherwise the job is an
/// `Intent::Space`, and the driver runs it in its own task when its frontier is complete.
/// Outside a reduction the job runs at once without an order, as a host operation does.
pub(crate) async fn submit_space<T: Send + 'static>(
    request: SpaceRequest<T>,
) -> Result<T, InterpreterError> {
    let SpaceRequest {
        sid,
        authority,
        channels,
        job,
    } = request;
    let context = current();
    let erased: ErasedJob = Box::new(move || {
        Box::pin(async move {
            let output = job().await?;
            Ok(Box::new(output) as ErasedOutput)
        })
    });
    let output = match context {
        Some(context) => {
            context
                .session
                .submit_space(&context, sid, authority, channels, erased)
                .await?
        }
        None => erased().await?,
    };
    match output.downcast::<T>() {
        Ok(output) => Ok(*output),
        Err(_) => Err(InterpreterError::BugFoundError(
            "a space job returned an output of another type".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use async_trait::async_trait;
    use models::rhoapi::{BindPattern, CostRegion, ListParWithRandom, TaggedContinuation};
    use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
    use models::rust::utils::new_gint_par;
    use prost::Message;
    use rspace_plus_plus::rspace::r#match::Match;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::{
        BoxedSpaceTxn, ProduceOutcome, SpaceMode, SpaceProduce, SpaceStoreHandle, SpaceTxnError,
        SpaceTxnHost, SweepReport, TxnRegistration,
    };
    use tokio::sync::Notify;

    use super::super::{
        conflict_components, root, scope, ParticipantGuard, PreparedIntent, ReductionCoordinator,
        ScopedJoinHandle,
    };
    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::native_phlo_rules::{
        native_resource_compatibility_rule, NativeBoundSource, NativeBudgetTraceLimits,
        NativePhloDimension, NativePhloExecutionContract, NativePhloRegionLimits,
    };
    use crate::rust::interpreter::accounting::phlo_controls::{
        check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
    };
    use crate::rust::interpreter::accounting::NativeRuntimeConfig;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::rho_runtime::RhoISpace;
    use crate::rust::interpreter::spaces::namespace::{self, Durability, NativeSpace};

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    type ParHandle = SpaceStoreHandle<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    async fn rspace() -> ParRSpace {
        let mut stores = InMemoryStoreManager::new();
        RSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space")
    }

    fn session_space(space: &ParRSpace) -> RhoISpace { Arc::new(Box::new(space.clone())) }

    fn budget(label: &'static str) -> RuntimeBudget {
        RuntimeBudget::new(Cost::create(1_000, label))
    }

    fn sid(seed: u8) -> SpaceId { [seed; SID_LEN] }

    fn config_channel(seed: u8) -> Par {
        namespace::gprivate(namespace::config_channel(NativeSpace {
            sid: sid(seed),
            durability: Durability::Durable,
        }))
    }

    fn unit_job() -> ErasedJob {
        Box::new(|| Box::pin(std::future::ready(Ok(Box::new(()) as ErasedOutput))))
    }

    fn intent(seed: u8, authority: Option<CostAuthority>, channels: Vec<Par>) -> SpaceIntent {
        let (reply, _receive) = oneshot::channel();
        SpaceIntent {
            sid: sid(seed),
            authority,
            channels,
            job: unit_job(),
            reply,
        }
    }

    /// A host whose `begin` waits on one lock key until the test releases it, as a contended
    /// lock stripe makes `begin` wait.
    struct GatedHost {
        inner: ParRSpace,
        gated: Par,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[async_trait]
    impl SpaceTxnHost<Par, BindPattern, ListParWithRandom, TaggedContinuation> for GatedHost {
        fn mode(&self) -> SpaceMode { SpaceTxnHost::mode(&self.inner) }

        fn matcher(
            &self,
        ) -> Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>> {
            SpaceTxnHost::matcher(&self.inner)
        }

        async fn begin(
            &self,
            lock_keys: &[Par],
            registration: TxnRegistration,
        ) -> Result<
            BoxedSpaceTxn<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
            SpaceTxnError,
        > {
            if lock_keys.first() == Some(&self.gated) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            SpaceTxnHost::begin(&self.inner, lock_keys, registration).await
        }

        fn root_leaf_record_count(&self, channel: &Par) -> Result<u64, SpaceTxnError> {
            SpaceTxnHost::root_leaf_record_count(&self.inner, channel)
        }

        async fn sweep_volatile(
            &self,
            channels: &[Par],
            registration: &TxnRegistration,
        ) -> Result<SweepReport, SpaceTxnError> {
            SpaceTxnHost::sweep_volatile(&self.inner, channels, registration).await
        }
    }

    /// Stores one datum on `channel` and commits. It waits for `wait` before `begin` and
    /// signals `signal` after `begin`, whatever `begin` returns.
    fn store_job(
        handle: ParHandle,
        channel: Par,
        wait: Option<Arc<Notify>>,
        signal: Option<Arc<Notify>>,
    ) -> SpaceJob<Result<usize, SpaceTxnError>> {
        Box::new(move || {
            Box::pin(async move {
                if let Some(wait) = wait {
                    wait.notified().await;
                }
                let begun = handle.begin(std::slice::from_ref(&channel)).await;
                if let Some(signal) = signal {
                    signal.notify_one();
                }
                let mut txn = match begun {
                    Ok(txn) => txn,
                    Err(error) => return Ok(Err(error)),
                };
                let staged = txn.produce(SpaceProduce {
                    channel: channel.clone(),
                    data: ListParWithRandom {
                        pars: vec![new_gint_par(1, Vec::new(), false)],
                        random_state: vec![1],
                        cost_authority: None,
                        cost_stack: None,
                    },
                    persist: true,
                    outcome: ProduceOutcome::Store,
                    before_observe: None,
                });
                if let Err(error) = staged {
                    return Ok(Err(error));
                }
                Ok(txn.commit().await.map(|receipt| receipt.ops.len()))
            })
        })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn space_intents_for_two_sids_in_one_frontier_run_in_separate_tasks() {
        let space = rspace().await;
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let gated = config_channel(1);
        let handle = SpaceStoreHandle::new(GatedHost {
            inner: space.clone(),
            gated: gated.clone(),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        });
        let jobs = vec![
            (sid(1), store_job(handle.clone(), gated, None, None)),
            (
                sid(2),
                store_job(
                    handle.clone(),
                    config_channel(2),
                    Some(entered),
                    Some(release),
                ),
            ),
        ];
        let evaluation = root(
            session_space(&space),
            budget("two sids in one frontier"),
            ReductionCoordinator::default(),
            async move {
                let parent = current().expect("root reduction context");
                let children = parent.split(jobs.len());
                let mut participants = Vec::with_capacity(jobs.len());
                for (child, (sid, job)) in children.into_iter().zip(jobs) {
                    participants.push(ScopedJoinHandle::new(tokio::spawn(scope(
                        child.clone(),
                        async move {
                            let _guard = ParticipantGuard::for_context(&child);
                            submit_space(SpaceRequest {
                                sid,
                                authority: None,
                                channels: Vec::new(),
                                job,
                            })
                            .await
                        },
                    ))));
                }
                let mut results = Vec::with_capacity(participants.len());
                for participant in participants {
                    results.push(participant.await.expect("the participant task joins"));
                }
                parent.rejoin();
                results
            },
        );
        let results = tokio::time::timeout(Duration::from_secs(60), evaluation)
            .await
            .expect("both space jobs finish");
        assert_eq!(results.len(), 2);
        for result in results {
            let committed = result.expect("the scheduler delivers the job result");
            assert_ne!(committed, Err(SpaceTxnError::NestedTransaction));
            assert_eq!(committed, Ok(1));
        }
    }

    #[tokio::test]
    async fn submit_space_outside_a_reduction_runs_the_job_at_once() {
        let output = submit_space(SpaceRequest {
            sid: sid(4),
            authority: None,
            channels: Vec::new(),
            job: Box::new(|| Box::pin(async move { Ok(operation_context::current().is_none()) })),
        })
        .await;
        assert_eq!(output, Ok(true));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failed_space_job_task_is_a_bug_found_error() {
        let space = rspace().await;
        let evaluation = root(
            session_space(&space),
            budget("failed space job"),
            ReductionCoordinator::default(),
            async {
                let parent = current().expect("root reduction context");
                let children = parent.split(2);
                let mut participants = Vec::with_capacity(2);
                for (index, child) in children.into_iter().enumerate() {
                    participants.push(ScopedJoinHandle::new(tokio::spawn(scope(
                        child.clone(),
                        async move {
                            let _guard = ParticipantGuard::for_context(&child);
                            let job: SpaceJob<()> = Box::new(move || {
                                Box::pin(async move {
                                    if index == 0 {
                                        panic!("an injected failure of a space job");
                                    }
                                    Ok(())
                                })
                            });
                            submit_space(SpaceRequest {
                                sid: sid(5 + index as u8),
                                authority: None,
                                channels: Vec::new(),
                                job,
                            })
                            .await
                        },
                    ))));
                }
                let mut results = Vec::with_capacity(2);
                for participant in participants {
                    results.push(participant.await.expect("the participant task joins"));
                }
                parent.rejoin();
                results
            },
        );
        let results = tokio::time::timeout(Duration::from_secs(60), evaluation)
            .await
            .expect("both space jobs finish");
        match &results[0] {
            Err(InterpreterError::BugFoundError(message)) => {
                assert!(
                    message.starts_with("deterministic space operation failed"),
                    "{message}"
                );
            }
            other => panic!("the failed job returned {other:?}"),
        }
        assert_eq!(results[1], Ok(()));
    }

    #[test]
    fn space_footprints_isolate_spaces_and_hold_channels_and_authorities() {
        let scoped = budget("space footprint");
        let _scope = scoped.enter_comm_accounting_scope();
        let authority = CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![9; 32],
                signature: None,
            }],
        };
        let mut footprint = BTreeSet::new();
        intent(5, Some(authority), vec![config_channel(5)])
            .extend_footprint(&mut footprint, &scoped);
        let mut space_key = vec![SPACE_FOOTPRINT_TAG];
        space_key.extend_from_slice(&sid(5));
        let mut channel_key = vec![0];
        channel_key.extend(config_channel(5).encode_to_vec());
        let mut authority_key = vec![1];
        authority_key.extend([9; 32]);
        assert_eq!(
            footprint,
            BTreeSet::from([space_key, channel_key, authority_key])
        );

        let unscoped = budget("space components");
        let prepared = |order: u64, seed: u8| {
            let intent = intent(seed, None, Vec::new());
            let mut footprint = BTreeSet::new();
            intent.extend_footprint(&mut footprint, &unscoped);
            PreparedIntent {
                order: OperationOrder {
                    session: [7; 32],
                    path: vec![(order, 0)].into(),
                },
                footprint,
                intent: Intent::Space(intent),
            }
        };
        let components = conflict_components(vec![prepared(0, 1), prepared(1, 2), prepared(2, 1)]);
        let mut orders: Vec<Vec<u64>> = components
            .iter()
            .map(|component| {
                component
                    .iter()
                    .map(|intent| intent.order.path.to_vec()[0].0)
                    .collect()
            })
            .collect();
        orders.sort();
        assert_eq!(orders, vec![vec![0, 2], vec![1]]);
    }

    /// A runtime budget in native execution (DR-78), as the native budget of the scheduler tests
    /// in `deterministic_reduction.rs` builds it.
    fn native_budget() -> RuntimeBudget {
        let descriptor = PhloScheduleV1 {
            protocol_version: 6,
            network: b"native-test",
            shard: b"root",
            settlement_asset: b"REV",
            settlement_unit: b"atomic-REV",
            decimal_scale: 8,
            classes: NativePhloDimension::ALL
                .into_iter()
                .zip([0, 0, 1, 0])
                .zip([b"c".as_slice(), b"i", b"t", b"r"])
                .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
                .collect(),
            actual_price: 0,
            compatibility_rule: native_resource_compatibility_rule(),
        };
        let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS)
            .expect("the test schedule binds");
        let selected = binding.schedule();
        let permitted = [selected];
        let owners = [0];
        let controls = check_phlo_controls(
            selected.environment,
            0,
            u64::MAX,
            SignedPhloControls {
                limit: 1,
                price_ceiling: 0,
                required_owner_ceilings: &owners,
                permitted_schedules: &permitted,
            },
            selected,
            1,
        )
        .expect("the test controls check");
        let contract =
            NativePhloExecutionContract::new(controls, &binding, NativeBoundSource::Certificate)
                .expect("the test contract builds");
        let config = NativeRuntimeConfig::new(contract, NativeBudgetTraceLimits {
            attempts: 4,
            path_segments: 4,
            regions: NativePhloRegionLimits {
                regions: 4,
                encoded_authority_bytes: 4096,
            },
        });
        let budget = RuntimeBudget::new(Cost::create(1, "native space schedule"));
        budget
            .reset_for_native_execution(config)
            .expect("the budget enters native execution");
        budget
    }

    /// The space intent of `seed` with the order `order` of one frontier, with the footprint
    /// that `budget` gives it.
    fn prepared_space(order: u64, seed: u8, budget: &RuntimeBudget) -> PreparedIntent {
        let intent = intent(seed, None, Vec::new());
        let mut footprint = BTreeSet::new();
        intent.extend_footprint(&mut footprint, budget);
        PreparedIntent {
            order: OperationOrder {
                session: [7; 32],
                path: vec![(order, 0)].into(),
            },
            footprint,
            intent: Intent::Space(intent),
        }
    }

    /// The orders of the intents of each conflict group, sorted.
    fn component_orders(components: &[Vec<PreparedIntent>]) -> Vec<Vec<u64>> {
        let mut orders: Vec<Vec<u64>> = components
            .iter()
            .map(|component| {
                component
                    .iter()
                    .map(|intent| intent.order.path.to_vec()[0].0)
                    .collect()
            })
            .collect();
        orders.sort();
        orders
    }

    /// Decision 47 (design v2, section 4.5): native execution puts the key `[2, 0]` in every
    /// footprint (DR-78, DR-79), so the space intents of one frontier form one conflict group,
    /// whatever their spaces.
    #[test]
    fn space_native_single_conflict_group() {
        let native = native_budget();
        assert!(native.native_execution_active());
        let intents = vec![
            prepared_space(0, 1, &native),
            prepared_space(1, 2, &native),
            prepared_space(2, 3, &native),
        ];
        assert!(intents
            .iter()
            .all(|intent| intent.footprint.contains(&vec![2, 0])));
        let components = conflict_components(intents);
        assert_eq!(component_orders(&components), vec![vec![0, 1, 2]]);
    }

    /// Decision 47 (design v2, section 4.5, I-11): the key `[2]‖sid` alone keeps the intents of
    /// one space in one conflict group. Without `[2, 0]`, the native footprints group by space,
    /// as the legacy footprints do. So the no-deadlock argument does not rest on `[2, 0]`.
    #[test]
    fn space_native_space_key_groups() {
        let native = native_budget();
        let without_native_key = |order: u64, seed: u8| {
            let mut intent = prepared_space(order, seed, &native);
            assert!(
                intent.footprint.remove(&vec![2, 0]),
                "native execution adds [2, 0]"
            );
            intent
        };
        let components = conflict_components(vec![
            without_native_key(0, 1),
            without_native_key(1, 2),
            without_native_key(2, 1),
            without_native_key(3, 2),
        ]);
        assert_eq!(component_orders(&components), vec![vec![0, 2], vec![1, 3]]);

        let legacy = budget("space key groups");
        assert!(!legacy.native_execution_active());
        let components = conflict_components(vec![
            prepared_space(0, 1, &legacy),
            prepared_space(1, 2, &legacy),
            prepared_space(2, 1, &legacy),
            prepared_space(3, 2, &legacy),
        ]);
        assert_eq!(component_orders(&components), vec![vec![0, 2], vec![1, 3]]);
    }

    /// The sid of a prepared space intent.
    fn sid_of(intent: &PreparedIntent) -> SpaceId {
        match &intent.intent {
            Intent::Space(space) => space.sid,
            _ => panic!("a prepared space intent"),
        }
    }

    fn property_config() -> proptest::test_runner::Config {
        proptest::test_runner::Config {
            cases: 64,
            rng_seed: proptest::test_runner::RngSeed::Fixed(0x0D47_5EED_2026_1007),
            failure_persistence: None,
            ..proptest::test_runner::Config::default()
        }
    }

    proptest::proptest! {
        #![proptest_config(property_config())]

        /// NoBlockedReadiness of NativeReplay.tla (decision 47, I-11): for any frontier of
        /// space intents, the key [2]||sid puts all intents of one space in one conflict
        /// group, in operation order, and the intents of other spaces in other groups. The
        /// key [2, 0] of native execution puts the whole frontier in one group.
        #[test]
        fn prop_space_key_keeps_one_space_in_one_group(
            seeds in proptest::collection::vec(1u8..5, 1..9)
        ) {
            let legacy = budget("space key property");
            let components = conflict_components(
                seeds
                    .iter()
                    .enumerate()
                    .map(|(order, seed)| prepared_space(order as u64, *seed, &legacy))
                    .collect(),
            );
            let spaces: BTreeSet<u8> = seeds.iter().copied().collect();
            proptest::prop_assert_eq!(components.len(), spaces.len());
            for component in &components {
                let sids: BTreeSet<SpaceId> = component.iter().map(sid_of).collect();
                proptest::prop_assert_eq!(sids.len(), 1);
                let orders: Vec<u64> = component
                    .iter()
                    .map(|intent| intent.order.path.to_vec()[0].0)
                    .collect();
                let mut sorted = orders.clone();
                sorted.sort_unstable();
                proptest::prop_assert_eq!(&orders, &sorted);
                let expected: Vec<u64> = seeds
                    .iter()
                    .enumerate()
                    .filter(|(_, seed)| sid(**seed) == sid_of(&component[0]))
                    .map(|(order, _)| order as u64)
                    .collect();
                proptest::prop_assert_eq!(orders, expected);
            }

            let native = native_budget();
            let components = conflict_components(
                seeds
                    .iter()
                    .enumerate()
                    .map(|(order, seed)| prepared_space(order as u64, *seed, &native))
                    .collect(),
            );
            proptest::prop_assert_eq!(components.len(), 1);
        }
    }
}
