//! The R7 backend (plan rule R7, P4 design C.6, A18).
//!
//! A system process sends its replies through the execution space of its process context, not
//! through the reducer hooks. `wrap` gives the system processes a [`RoutingExecution`] over the
//! execution space of the reducer. The backend routes the channel of each produce with the rules
//! R1 to R4 and no use-context, because a system process has no lexical context.
//!
//! * A space name (R3) runs the produce on its native space and dispatches the fires.
//! * An agent-tagged name (R4) fails closed until P8.
//! * Every other channel goes to the wrapped space unchanged.
//!
//! The backend does not unbundle a channel and does not pack a factory call. Consumes, joins,
//! the replay flag and produce updates go to the wrapped space unchanged.

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock, Weak};

use async_trait::async_trait;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::trace::event::Produce;

use super::errors::ReifiedReduceError;
use super::route::{route, Route};
use super::runtime::ReifiedRuntime;
use super::system_processes::native_agent;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::execution_space::{
    ConsumeResult, ExecutionBackend, ExecutionSpace, ProduceResult,
};
use crate::rust::interpreter::reduce::ReducerCore;
use crate::rust::interpreter::spaces::agent::{ProduceOp, SpaceOp};
use crate::rust::interpreter::spaces::namespace::{CollectionKey, NativeSpace};

/// The execution space of the system processes (R7). [`wrap`] makes it.
pub struct RoutingExecution {
    inner: ExecutionSpace,
    runtime: Arc<ReifiedRuntime>,
    reducer: Arc<OnceLock<Weak<ReducerCore>>>,
}

/// Wraps the execution space of the reducer for the dispatch table. `runtime` runs the space
/// operations, and `reducer` is the cell of the reducer, which dispatches their fires.
pub fn wrap(
    inner: ExecutionSpace,
    runtime: &Arc<ReifiedRuntime>,
    reducer: &Arc<OnceLock<Weak<ReducerCore>>>,
) -> ExecutionSpace {
    ExecutionSpace::new(RoutingExecution {
        inner,
        runtime: Arc::clone(runtime),
        reducer: Arc::clone(reducer),
    })
}

impl RoutingExecution {
    /// A produce on a space name (P4 design C.6): get the reducer, run the produce as one
    /// `Intent::Space` operation, and dispatch its fires. The result is `None`, so the producer
    /// dispatches nothing more. The introduction authority is the cost authority of the datum.
    async fn produce_in_space(
        &self,
        space: NativeSpace,
        key: CollectionKey,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
    ) -> Result<ProduceResult, InterpreterError> {
        let reducer = self.reducer.get().and_then(Weak::upgrade).ok_or_else(|| {
            InterpreterError::BugFoundError("Reducer not initialized".to_string())
        })?;
        let authority = data.cost_authority.clone().unwrap_or_default();
        let outcome = self
            .runtime
            .run_op(
                native_agent(),
                SpaceOp::Produce(ProduceOp {
                    space,
                    key,
                    channel,
                    data,
                    persistent,
                    authority,
                    reissued_ord: None,
                }),
            )
            .await?;
        reducer.dispatch_space_outcome(outcome, None).await?;
        Ok(None)
    }
}

/// The error of a routed produce as the backend returns it. A phlo failure and a host-work
/// failure keep their class, and every other failure keeps its `reified:<Code>:` text.
fn backend_error(error: InterpreterError) -> RSpaceError { RSpaceError::from(error) }

#[async_trait]
impl ExecutionBackend for RoutingExecution {
    async fn consume(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persistent: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<ConsumeResult, RSpaceError> {
        self.inner
            .consume(channels, patterns, continuation, persistent, peeks)
            .await
    }

    async fn produce(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
    ) -> Result<ProduceResult, RSpaceError> {
        match route(&None, &channel) {
            Ok(Route::Default) => self.inner.produce(channel, data, persistent).await,
            Ok(Route::Space { space, key }) => self
                .produce_in_space(space, key, channel, data, persistent)
                .await
                .map_err(backend_error),
            Ok(Route::Agent) => Err(backend_error(
                ReifiedReduceError::AgentSpacesUnavailable.into(),
            )),
            Err(error) => Err(backend_error(error.into())),
        }
    }

    async fn get_joins(&self, channel: Par) -> Result<Vec<Vec<Par>>, RSpaceError> {
        self.inner.get_joins(channel).await
    }

    async fn is_replay(&self) -> bool { self.inner.is_replay().await }

    async fn update_produce(
        &self,
        original: &Produce,
        updated: Produce,
    ) -> Result<(), RSpaceError> {
        self.inner.update_produce(original, updated).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{CostAuthority, ParWithRandom, Send as SendProcess};
    use models::rust::utils::{
        new_boundvar_par, new_bundle_par, new_etuple_par, new_freevar_par, new_gint_par,
        new_gstring_par,
    };
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::interpreter::accounting::cost_accounting::CostAccounting;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::deterministic_reduction::root;
    use crate::rust::interpreter::external_services::ExternalServices;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::reduce::DebruijnInterpreter;
    use crate::rust::interpreter::rho_runtime::{create_rho_runtime, RhoISpace};
    use crate::rust::interpreter::spaces::agent::SpaceCreation;
    use crate::rust::interpreter::spaces::namespace::{
        self, Durability, NameTail, AGENT_TAG, SID_LEN,
    };
    use crate::rust::interpreter::spaces::types::config::SpaceConfig;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    /// A reducer over a fresh in-memory host, the host, and the backend of the reducer.
    struct Fixture {
        host: ParRSpace,
        reducer: Arc<DebruijnInterpreter>,
        backend: ExecutionSpace,
    }

    impl Fixture {
        fn over(host: ParRSpace, reducer: Arc<DebruijnInterpreter>) -> Self {
            let cell = Arc::new(OnceLock::new());
            cell.set(Arc::downgrade(&reducer.core))
                .expect("a new cell is empty");
            let backend = wrap(reducer.core.space.clone(), &reducer.core.reified, &cell);
            Self {
                host,
                reducer,
                backend,
            }
        }
    }

    async fn host() -> ParRSpace {
        let mut stores = InMemoryStoreManager::new();
        RSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space")
    }

    /// A reducer with the `Detached` runtime of `DebruijnInterpreter::new`.
    async fn detached() -> Fixture {
        let host = host().await;
        let session: RhoISpace = Arc::new(Box::new(host.clone()));
        let cost = CostAccounting::empty_cost();
        let reducer = DebruijnInterpreter::new(
            session,
            Arc::new(HashMap::new()),
            Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            Arc::new(HashMap::new()),
            cost.clone(),
        );
        cost.set(Cost::create(i64::MAX, "routing execution test".to_string()));
        Fixture::over(host, reducer)
    }

    /// The reducer of a runtime with a `Host` runtime over the host.
    async fn hosted() -> Fixture {
        let host = host().await;
        let runtime = create_rho_runtime(
            host.clone(),
            Arc::new(HashMap::new()),
            false,
            &mut Vec::new(),
            ExternalServices::noop(),
        )
        .await
        .expect("create the rho runtime");
        runtime
            .cost
            .set(Cost::create(i64::MAX, "routing execution test".to_string()));
        Fixture::over(host, runtime.reducer)
    }

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn private(seed: u8) -> Par { namespace::gprivate(vec![seed; SID_LEN]) }

    fn native(seed: u8) -> NativeSpace {
        NativeSpace {
            sid: [seed; SID_LEN],
            durability: Durability::Durable,
        }
    }

    fn space_name(space: NativeSpace, seed: u8) -> Par {
        namespace::gprivate(namespace::name_id(space, NameTail::Random([seed; 32])))
    }

    fn datum(pars: Vec<Par>) -> ListParWithRandom {
        ListParWithRandom {
            pars,
            random_state: Blake2b512Random::create_from_bytes(b"reply").to_bytes(),
            cost_authority: None,
            cost_stack: None,
        }
    }

    fn pattern() -> BindPattern {
        BindPattern {
            patterns: vec![new_freevar_par(0, Vec::new())],
            remainder: None,
            free_count: 1,
        }
    }

    fn continuation(body: Par) -> TaggedContinuation {
        TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(body),
                random_state: Blake2b512Random::create_from_bytes(b"waiter").to_bytes(),
            })),
            guard: None,
            cost_authority: None,
        }
    }

    /// `out!(x)` for the bound variable `x` of a one-bind receive.
    fn forward_to(out: &Par) -> Par {
        Par::default().with_sends(vec![SendProcess {
            chan: Some(out.clone()),
            data: vec![new_boundvar_par(0, Vec::new(), false)],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        }])
    }

    /// The `reified:<Code>` of a failed produce of the backend.
    fn code<T>(result: Result<T, RSpaceError>) -> Option<String> {
        match result {
            Err(RSpaceError::InterpreterError(message)) => message
                .split("reified:")
                .nth(1)
                .and_then(|rest| rest.split(':').next())
                .map(str::to_string),
            Ok(_) | Err(_) => None,
        }
    }

    async fn stored(host: &ParRSpace, channel: &Par) -> Vec<Vec<Par>> {
        host.get_data(channel)
            .await
            .into_iter()
            .map(|datum| datum.a.pars)
            .collect()
    }

    /// Creates `space` with the config of `hashmap:queue` in the runtime of `reducer`.
    async fn create(reducer: &ReducerCore, space: NativeSpace) -> Result<(), InterpreterError> {
        let creation = SpaceCreation {
            space,
            config: SpaceConfig::new(space.sid, "hashmap", "queue", SpaceQualifier::Default)
                .to_par(),
            handle_pattern: pattern(),
            handle_continuation: continuation(Par::default()),
            random: Blake2b512Random::create_from_bytes(&space.sid),
            authority: CostAuthority::default(),
        };
        reducer.reified.create_space(native_agent(), creation).await
    }

    /// R1 and R2: a reply on a channel that is not a space name or an agent-tagged name goes to
    /// the wrapped space unchanged. Consumes, joins and the replay flag also go there. A bundled
    /// space name is not unbundled, so it is a forged channel of the default space.
    #[tokio::test]
    async fn reducer_space_routing_r7_delegates_default_channels() {
        let routed = detached().await;
        let direct = detached().await;
        let bundled_name = new_bundle_par(space_name(native(1), 1), true, false);
        for channel in [
            private(1),
            string("rrs-4c-reply"),
            namespace::gprivate(namespace::op_channel(0x01)),
            namespace::gprivate(namespace::handle_channel(native(2))),
            bundled_name,
        ] {
            let routed_result = routed
                .backend
                .produce(channel.clone(), datum(vec![int(1)]), false)
                .await
                .expect("a default reply is produced");
            let direct_result = direct
                .reducer
                .core
                .space
                .produce(channel.clone(), datum(vec![int(1)]), false)
                .await
                .expect("the direct produce runs");
            assert_eq!(routed_result.is_none(), direct_result.is_none());
            assert_eq!(
                routed.host.get_data(&channel).await,
                direct.host.get_data(&channel).await
            );
            assert_eq!(routed.host.get_data(&channel).await.len(), 1);
        }
        let channels = vec![private(3), string("rrs-4c-join")];
        for space in [&routed.backend, &direct.reducer.core.space] {
            space
                .consume(
                    channels.clone(),
                    vec![pattern(), pattern()],
                    continuation(Par::default()),
                    false,
                    BTreeSet::new(),
                )
                .await
                .expect("the consume runs");
        }
        let waiting = routed
            .host
            .get_waiting_continuations(channels.clone())
            .await;
        assert_eq!(waiting.len(), 1);
        assert_eq!(
            waiting,
            direct
                .host
                .get_waiting_continuations(channels.clone())
                .await
        );
        let joins = routed
            .backend
            .get_joins(private(3))
            .await
            .expect("the joins are read");
        assert!(!joins.is_empty());
        assert_eq!(
            joins,
            direct
                .reducer
                .core
                .space
                .get_joins(private(3))
                .await
                .expect("the direct joins are read")
        );
        assert_eq!(
            routed.backend.is_replay().await,
            direct.reducer.core.space.is_replay().await
        );
    }

    /// R3: a reply on a space name runs as a produce on the space, and the backend dispatches
    /// the continuation that it fires. Nothing goes to the default space on the name.
    #[tokio::test]
    async fn reducer_space_routing_r7_routes_space_name_replies_into_the_space() {
        let fixture = hosted().await;
        let core: &ReducerCore = &fixture.reducer.core;
        let space = native(3);
        let name = space_name(space, 2);
        let out = private(4);
        root(
            core.space.clone(),
            core.metering.budget(),
            core.reduction_coordinator.clone(),
            async {
                create(core, space).await?;
                core.reified_consume(
                    vec![(pattern(), name.clone())],
                    ParWithRandom {
                        body: Some(forward_to(&out)),
                        random_state: Blake2b512Random::create_from_bytes(b"receive").to_bytes(),
                    },
                    false,
                    false,
                    None,
                    CostAuthority::default(),
                    CostAuthority::default(),
                )
                .await?;
                let reply = fixture
                    .backend
                    .produce(name.clone(), datum(vec![int(7)]), false)
                    .await
                    .map_err(InterpreterError::from)?;
                assert!(reply.is_none());
                Ok::<_, InterpreterError>(())
            },
        )
        .await
        .expect("the reply reaches the space");
        assert_eq!(stored(&fixture.host, &out).await, vec![vec![int(7)]]);
        assert!(fixture.host.get_data(&name).await.is_empty());

        let missing = space_name(native(5), 3);
        assert_eq!(
            code(
                fixture
                    .backend
                    .produce(missing.clone(), datum(vec![int(1)]), false)
                    .await
            ),
            Some("DanglingName".to_string())
        );
        let detached = detached().await;
        assert_eq!(
            code(
                detached
                    .backend
                    .produce(missing.clone(), datum(vec![int(1)]), false)
                    .await
            ),
            Some("RuntimeUnavailable".to_string())
        );
        assert!(detached.host.get_data(&missing).await.is_empty());
    }

    /// R4: a reply on an agent-tagged name fails closed until P8.
    #[tokio::test]
    async fn reducer_space_routing_r7_rejects_agent_tagged_replies() {
        let fixture = detached().await;
        let tagged = new_etuple_par(vec![
            namespace::gprivate(namespace::op_channel(AGENT_TAG)),
            string("agent"),
            int(1),
        ]);
        assert_eq!(
            code(
                fixture
                    .backend
                    .produce(tagged.clone(), datum(vec![int(1)]), false)
                    .await
            ),
            Some("AgentSpacesUnavailable".to_string())
        );
        assert!(fixture.host.get_data(&tagged).await.is_empty());
    }

    /// Without a reducer in the cell, a reply on a space name fails before the operation runs,
    /// so the space stays unchanged.
    #[tokio::test]
    async fn reducer_space_routing_r7_without_a_reducer_fails_closed() {
        let fixture = hosted().await;
        let core: &ReducerCore = &fixture.reducer.core;
        let space = native(6);
        create(core, space).await.expect("the space is created");
        let backend = wrap(
            core.space.clone(),
            &core.reified,
            &Arc::new(OnceLock::new()),
        );
        let name = space_name(space, 4);
        match backend
            .produce(name.clone(), datum(vec![int(1)]), false)
            .await
        {
            Err(RSpaceError::InterpreterError(message)) => {
                assert!(message.contains("Reducer not initialized"), "{message}");
            }
            other => panic!("a reply without a reducer returned {other:?}"),
        }
        let collection = namespace::gprivate(namespace::data_channel(
            space,
            &CollectionKey::of_channel(&name).expect("a key"),
        ));
        assert!(fixture.host.get_data(&collection).await.is_empty());
    }

    /// A phlo or host-work failure keeps its class, and every other failure keeps its code.
    #[test]
    fn reducer_space_routing_r7_errors_keep_their_class() {
        assert_eq!(
            backend_error(InterpreterError::OutOfPhlogistonsError),
            RSpaceError::OutOfPhlogistons
        );
        assert_eq!(
            backend_error(InterpreterError::HostWorkRejected),
            RSpaceError::HostWorkRejected
        );
        assert_eq!(
            code::<()>(Err(backend_error(
                ReifiedReduceError::AgentSpacesUnavailable.into()
            ))),
            Some("AgentSpacesUnavailable".to_string())
        );
    }
}
