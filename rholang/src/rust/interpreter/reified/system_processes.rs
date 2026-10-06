//! The Reified system processes of P4 (P4 design A.5 and C.9).
//!
//! | URN                             | Process                                   |
//! |---------------------------------|-------------------------------------------|
//! | `rho:space:hashmap:queue`       | the factory of the family (`factory.rs`)  |
//! | `rho:space:getSpaceAgent`       | `getSpaceAgent!(ret, s)`                  |
//! | `rho:assert:assertEqual`        | `assertEqual!(ret, a, b)` (decision 33)   |
//! | `rho:reified:internal:handleOp` | the handle process (`handle.rs`, A6)      |
//!
//! `create_rho_env` calls [`register`] with the runtime of the host store, before
//! `setup_maps_and_refs` binds the URNs. `dispatch_table_creator` drains the vector, so a
//! vector that serves two runtimes never holds a definition twice. Every process has a fixed
//! arity (I-18).
//!
//! `getSpaceAgent!(ret, s)` classifies `s` with `classify_space_ref`. For a native space it
//! reads `C(sid)` in a read-only job and replies the capability of the factory of the family,
//! which equals the urn map value of the factory URN (plan decision 9.4). Every other result is
//! a typed error reply (A22): `DanglingHandle` for a missing config record, `NotANativeSpace`
//! for a user agent, and the code of the reference error otherwise. `assertEqual!(ret, a, b)`
//! replies `true` when `a` and `b` are equal after the sort of `ParSortMatcher`.

use std::sync::Arc;

use models::rhoapi::{CostAuthority, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::new_gbool_par;

use super::factory::{ContractArgs, FactoryProcess};
use super::handle::{typed_error, HandleProcess};
use super::runtime::{ReifiedRuntime, ReifiedRuntimeMode};
use super::scope::{classify_space_ref, SpaceRef};
use super::system_channels::{
    SystemChannel, ASSERT_EQUAL_PROCESS, GET_SPACE_AGENT_PROCESS, HANDLE_OP_PROCESS,
    P4_SYSTEM_CHANNELS,
};
use crate::rust::interpreter::contract_call::ContractCall;
use crate::rust::interpreter::deterministic_reduction::reified_schedule::{
    submit_space, SpaceRequest,
};
use crate::rust::interpreter::errors::{illegal_argument_error, InterpreterError};
use crate::rust::interpreter::spaces::agent::SpaceAgent;
use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
use crate::rust::interpreter::spaces::errors::{SpaceError, SpaceReplyCode};
use crate::rust::interpreter::spaces::factory::constructor::family_of;
use crate::rust::interpreter::spaces::factory::urn::{factory_capability, REGISTERED_FAMILIES};
use crate::rust::interpreter::spaces::generic::GenericSpace;
use crate::rust::interpreter::spaces::namespace::NativeSpace;
use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use crate::rust::interpreter::spaces::types::config::SpaceConfig;
use crate::rust::interpreter::system_processes::{Definition, ProcessContext};

/// The native agent of the families of this phase: `GenericSpace<HashMapOuter,
/// QueueDiscipline>`. Every config record has one format, so the agent also reads the config
/// of any space.
pub fn native_agent() -> Arc<dyn SpaceAgent> {
    Arc::new(GenericSpace::<HashMapOuter, QueueDiscipline>::default())
}

/// The definitions of the Reified system processes of `runtime`: the factory of each registered
/// family, `getSpaceAgent`, `assertEqual` and the handle process.
pub fn definitions(runtime: &Arc<ReifiedRuntime>) -> Vec<Definition> {
    let agent = native_agent();
    let mut definitions = Vec::with_capacity(P4_SYSTEM_CHANNELS.len());
    for (family, channel) in REGISTERED_FAMILIES {
        definitions.push(definition(
            channel,
            ReifiedProcess::Factory(FactoryProcess::new(
                family,
                Arc::clone(runtime),
                Arc::clone(&agent),
            )),
        ));
    }
    definitions.push(definition(
        GET_SPACE_AGENT_PROCESS,
        ReifiedProcess::GetSpaceAgent {
            runtime: Arc::clone(runtime),
            agent: Arc::clone(&agent),
        },
    ));
    definitions.push(definition(
        ASSERT_EQUAL_PROCESS,
        ReifiedProcess::AssertEqual,
    ));
    definitions.push(definition(
        HANDLE_OP_PROCESS,
        ReifiedProcess::Handle(HandleProcess::new(Arc::clone(runtime), agent)),
    ));
    definitions
}

/// Adds the Reified system processes of `runtime` to `processes`.
pub fn register(processes: &mut Vec<Definition>, runtime: &Arc<ReifiedRuntime>) {
    processes.extend(definitions(runtime));
}

/// A Reified system process.
enum ReifiedProcess {
    Factory(FactoryProcess),
    GetSpaceAgent {
        runtime: Arc<ReifiedRuntime>,
        agent: Arc<dyn SpaceAgent>,
    },
    AssertEqual,
    Handle(HandleProcess),
}

impl ReifiedProcess {
    async fn call(
        &self,
        contract: &ContractCall,
        contract_args: ContractArgs,
    ) -> Result<Vec<Par>, InterpreterError> {
        match self {
            ReifiedProcess::Factory(factory) => factory.call(contract, contract_args).await,
            ReifiedProcess::GetSpaceAgent { runtime, agent } => {
                get_space_agent(runtime, agent, contract, contract_args).await
            }
            ReifiedProcess::AssertEqual => assert_equal(contract, contract_args).await,
            ReifiedProcess::Handle(handle) => handle.call(contract, contract_args).await,
        }
    }
}

fn definition(channel: SystemChannel, process: ReifiedProcess) -> Definition {
    let process = Arc::new(process);
    Definition::new(
        channel.urn.to_string(),
        channel.channel(),
        channel.arity,
        channel.body_ref(),
        Box::new(move |context: ProcessContext| {
            let process = Arc::clone(&process);
            let space = context.space;
            let dispatcher = context.dispatcher;
            Box::new(move |contract_args| {
                let process = Arc::clone(&process);
                let contract = ContractCall {
                    space: space.clone(),
                    dispatcher: dispatcher.clone(),
                };
                Box::pin(async move { process.call(&contract, contract_args).await })
            })
        }),
        None,
    )
}

/// `getSpaceAgent!(ret, s)` (P4 design C.9, A22).
async fn get_space_agent(
    runtime: &Arc<ReifiedRuntime>,
    agent: &Arc<dyn SpaceAgent>,
    contract: &ContractCall,
    contract_args: ContractArgs,
) -> Result<Vec<Par>, InterpreterError> {
    let authority = match contract_args.0.as_slice() {
        [call] => call.cost_authority.clone(),
        _ => return Err(illegal_argument_error(GET_SPACE_AGENT_PROCESS.urn)),
    };
    let Some((produce, _, _, args)) = contract.unapply(contract_args) else {
        return Err(illegal_argument_error(GET_SPACE_AGENT_PROCESS.urn));
    };
    let [ret, reference] = args.as_slice() else {
        return Err(illegal_argument_error(GET_SPACE_AGENT_PROCESS.urn));
    };
    let reply = match classify_space_ref(reference) {
        Err(error) => typed_error(error.reply_code(), Vec::new()),
        Ok(SpaceRef::Agent(_)) => typed_error(SpaceReplyCode::NotANativeSpace, Vec::new()),
        Ok(SpaceRef::Native(space)) => match read_config(runtime, agent, space, authority).await? {
            Ok(config) => {
                let config = SpaceConfig::from_par(&config).map_err(SpaceError::from)?;
                let family = family_of(&config)?;
                factory_capability(family).ok_or(SpaceError::Inconsistent(
                    "the config record names a family without a factory",
                ))?
            }
            Err(SpaceError::DanglingHandle) => {
                typed_error(SpaceReplyCode::DanglingHandle, Vec::new())
            }
            Err(error) => return Err(error.into()),
        },
    };
    let reply = vec![reply];
    produce(&reply, ret).await?;
    Ok(reply)
}

/// Reads the config map of `space` as `ReifiedRuntime::read_config` does: one read-only
/// `Intent::Space` job that commits nothing. The outer result is the result of the job, and
/// the inner result keeps the typed space error, so `getSpaceAgent` can reply
/// `DanglingHandle`.
async fn read_config(
    runtime: &Arc<ReifiedRuntime>,
    agent: &Arc<dyn SpaceAgent>,
    space: NativeSpace,
    authority: Option<CostAuthority>,
) -> Result<Result<Par, SpaceError>, InterpreterError> {
    match runtime.mode() {
        ReifiedRuntimeMode::Host => {}
        ReifiedRuntimeMode::Detached => return Err(SpaceError::RuntimeUnavailable.into()),
        ReifiedRuntimeMode::NativeReplay => {
            return Err(SpaceError::SpacesUnsupportedInNativeReplay.into())
        }
    }
    let runtime = Arc::clone(runtime);
    let agent = Arc::clone(agent);
    submit_space(SpaceRequest {
        sid: space.sid,
        authority,
        channels: Vec::new(),
        job: Box::new(move |host_work| {
            Box::pin(async move {
                let env = runtime.space_env(host_work)?;
                Ok(agent.read_config(&env, space).await)
            })
        }),
    })
    .await
}

/// `assertEqual!(ret, a, b)` (plan decision 33).
async fn assert_equal(
    contract: &ContractCall,
    contract_args: ContractArgs,
) -> Result<Vec<Par>, InterpreterError> {
    let Some((produce, _, _, args)) = contract.unapply(contract_args) else {
        return Err(illegal_argument_error(ASSERT_EQUAL_PROCESS.urn));
    };
    let [ret, left, right] = args.as_slice() else {
        return Err(illegal_argument_error(ASSERT_EQUAL_PROCESS.urn));
    };
    let equal = ParSortMatcher::sort_match(left).term == ParSortMatcher::sort_match(right).term;
    let reply = vec![new_gbool_par(equal, Vec::new(), false)];
    produce(&reply, ret).await?;
    Ok(reply)
}

#[cfg(test)]
pub(crate) mod harness {
    //! The fixtures of the unit tests of the factory, the handle and the system processes.

    use std::collections::HashMap;
    use std::future::Future;
    use std::sync::{Arc, OnceLock};

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
    use models::rust::space_factory_urn::SpaceFactoryUrn;
    use models::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::native_agent;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::contract_call::ContractCall;
    use crate::rust::interpreter::deterministic_reduction::root;
    use crate::rust::interpreter::dispatch::RholangAndScalaDispatcher;
    use crate::rust::interpreter::errors::InterpreterError;
    use crate::rust::interpreter::external_services::ExternalServices;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::reduce::DebruijnInterpreter;
    use crate::rust::interpreter::reified::factory::{ContractArgs, FactoryProcess};
    use crate::rust::interpreter::reified::runtime::{for_host, ReifiedRuntime};
    use crate::rust::interpreter::rho_runtime::{create_rho_env, RhoISpace};
    use crate::rust::interpreter::spaces::agent::SpaceAgent;
    use crate::rust::interpreter::spaces::namespace::{self, Durability, NativeSpace, SID_LEN};
    use crate::rust::interpreter::spaces::types::id::derive_space_id;

    pub(crate) type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    pub(crate) async fn rspace() -> ParRSpace {
        let mut stores = InMemoryStoreManager::new();
        RSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space")
    }

    pub(crate) fn budget() -> RuntimeBudget {
        RuntimeBudget::new(Cost::create(i64::MAX, "reified system process test"))
    }

    /// The arguments of a call with the data `pars` and the random state of `seed`.
    pub(crate) fn call(pars: Vec<Par>, seed: &[u8]) -> ContractArgs {
        (
            vec![ListParWithRandom {
                pars,
                random_state: Blake2b512Random::create_from_bytes(seed).to_bytes(),
                cost_authority: None,
                cost_stack: None,
            }],
            false,
            Vec::new(),
        )
    }

    /// The durable space that a factory call with the random state of `seed` creates.
    pub(crate) fn space_of(seed: &[u8]) -> NativeSpace {
        NativeSpace {
            sid: derive_space_id(&Blake2b512Random::create_from_bytes(seed)),
            durability: Durability::Durable,
        }
    }

    /// A reply name: an unforgeable name of 32 bytes.
    pub(crate) fn reply_name(seed: u8) -> Par { namespace::gprivate(vec![seed; SID_LEN]) }

    pub(crate) fn string(value: &str) -> Par {
        new_gstring_par(value.to_string(), Vec::new(), false)
    }

    pub(crate) fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    pub(crate) fn list(ps: Vec<Par>) -> Par {
        new_elist_par(ps, Vec::new(), false, None, Vec::new(), false)
    }

    /// A runtime over a fresh in-memory host, with no reducer. Replies go to the host.
    pub(crate) struct Fixture {
        pub(crate) rspace: ParRSpace,
        pub(crate) runtime: Arc<ReifiedRuntime>,
        pub(crate) agent: Arc<dyn SpaceAgent>,
        pub(crate) contract: ContractCall,
    }

    impl Fixture {
        /// A `Host` runtime over the host.
        pub(crate) async fn new() -> Self {
            let rspace = rspace().await;
            let runtime = for_host(rspace.clone(), budget());
            Self::over(rspace, runtime)
        }

        /// `runtime` with a host for the replies only.
        pub(crate) async fn with_runtime(runtime: Arc<ReifiedRuntime>) -> Self {
            Self::over(rspace().await, runtime)
        }

        fn over(rspace: ParRSpace, runtime: Arc<ReifiedRuntime>) -> Self {
            let session: RhoISpace = Arc::new(Box::new(rspace.clone()));
            let dispatcher = Arc::new(RholangAndScalaDispatcher {
                _dispatch_table: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                reducer: Arc::new(OnceLock::new()),
            });
            Fixture {
                rspace,
                runtime,
                agent: native_agent(),
                contract: ContractCall {
                    space: session.into(),
                    dispatcher,
                },
            }
        }

        /// The factory of `hashmap:queue` of the runtime.
        pub(crate) fn factory(&self) -> FactoryProcess {
            FactoryProcess::new(
                SpaceFactoryUrn::HASHMAP_QUEUE,
                Arc::clone(&self.runtime),
                Arc::clone(&self.agent),
            )
        }

        /// Creates the space of `seed` with an empty constructor.
        pub(crate) async fn create(&self, seed: &[u8]) -> Result<NativeSpace, InterpreterError> {
            self.factory()
                .call(
                    &self.contract,
                    call(vec![reply_name(0xFF), list(Vec::new())], seed),
                )
                .await?;
            Ok(space_of(seed))
        }

        /// The payloads of the data on `channel`.
        pub(crate) async fn data(&self, channel: &Par) -> Vec<Vec<Par>> {
            self.rspace
                .get_data(channel)
                .await
                .into_iter()
                .map(|datum| datum.a.pars)
                .collect()
        }
    }

    /// A reducer of `create_rho_env` over a fresh in-memory host, with the Reified system
    /// processes of [`Reduction::runtime`] installed. The runtime and the reducer share the
    /// host and the budget.
    pub(crate) struct Reduction {
        pub(crate) rspace: ParRSpace,
        pub(crate) runtime: Arc<ReifiedRuntime>,
        pub(crate) reducer: Arc<DebruijnInterpreter>,
        pub(crate) budget: RuntimeBudget,
    }

    impl Reduction {
        pub(crate) async fn new() -> Self {
            let rspace = rspace().await;
            let budget = budget();
            let runtime = for_host(rspace.clone(), budget.clone());
            let mut processes = super::definitions(&runtime);
            let (reducer, _, _, _) = create_rho_env(
                rspace.clone(),
                Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                Arc::new(HashMap::new()),
                &mut processes,
                budget.clone(),
                ExternalServices::noop(),
            )
            .await
            .expect("create the rho environment");
            assert!(
                processes.is_empty(),
                "the dispatch table drains the definitions"
            );
            Reduction {
                rspace,
                runtime,
                reducer,
                budget,
            }
        }

        /// A contract call over the space and the dispatcher of the reducer.
        pub(crate) fn contract(&self) -> ContractCall {
            ContractCall {
                space: self.reducer.core.space.clone(),
                dispatcher: self.reducer.core.dispatcher.clone(),
            }
        }

        /// Runs `future` as one root evaluation of the reducer.
        pub(crate) async fn reduce<T>(&self, future: impl Future<Output = T>) -> T {
            self.runtime.begin_evaluation_scope();
            self.reducer.core.reified.begin_evaluation_scope();
            root(
                self.reducer.core.space.clone(),
                self.budget.clone(),
                self.reducer.core.reduction_coordinator.clone(),
                future,
            )
            .await
        }

        /// Evaluates `term` as a deploy body.
        pub(crate) async fn evaluate(
            &self,
            term: Par,
            seed: &[u8],
        ) -> Result<(), InterpreterError> {
            self.runtime.begin_evaluation_scope();
            self.reducer
                .inj(term, Blake2b512Random::create_from_bytes(seed))
                .await
        }

        /// The payloads of the data on `channel`.
        pub(crate) async fn data(&self, channel: &Par) -> Vec<Vec<Par>> {
            self.rspace
                .get_data(channel)
                .await
                .into_iter()
                .map(|datum| datum.a.pars)
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::{Par, Send as SendProcess};
    use models::rust::space_factory_urn::SpaceFactoryUrn;
    use models::rust::utils::{new_bundle_par, new_gbool_par};

    use super::harness::{call, int, list, reply_name, space_of, string, Fixture, Reduction};
    use super::*;
    use crate::rust::interpreter::reified::factory::handle_capability;
    use crate::rust::interpreter::reified::runtime::{native_replay, unavailable};
    use crate::rust::interpreter::spaces::namespace::{self, NameTail, GET_SPACE_AGENT};

    fn typed(code: SpaceReplyCode) -> Par { typed_error(code, Vec::new()) }

    fn send(channel: Par, data: Vec<Par>) -> Par {
        Par::default().with_sends(vec![SendProcess {
            chan: Some(channel),
            data,
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        }])
    }

    #[test]
    fn space_system_ops_arity_and_body_refs_stable() {
        let runtime = unavailable();
        let defined = definitions(&runtime);
        assert_eq!(defined.len(), P4_SYSTEM_CHANNELS.len());
        for (definition, channel) in defined.iter().zip(P4_SYSTEM_CHANNELS) {
            assert_eq!(definition.urn, channel.urn);
            assert_eq!(definition.fixed_channel, channel.channel());
            assert_eq!(definition.arity, channel.arity);
            assert_eq!(definition.body_ref, channel.body_ref());
            assert_eq!(definition.remainder, None);
            assert_eq!(
                definition.to_urn_map(),
                (
                    channel.urn.to_string(),
                    new_bundle_par(channel.channel(), true, false)
                )
            );
        }
        assert_eq!(
            Some(defined[0].to_urn_map().1),
            factory_capability(SpaceFactoryUrn::HASHMAP_QUEUE)
        );
        let mut processes = definitions(&runtime);
        register(&mut processes, &runtime);
        assert_eq!(processes.len(), 2 * P4_SYSTEM_CHANNELS.len());
        assert!(processes
            .iter()
            .zip(P4_SYSTEM_CHANNELS.iter().chain(P4_SYSTEM_CHANNELS.iter()))
            .all(|(definition, channel)| definition.body_ref == channel.body_ref()));
    }

    #[tokio::test]
    async fn space_system_ops_assert_equal() {
        let fixture = Fixture::new().await;
        let pair = |first: i64, second: i64| {
            let mut par = int(first);
            par.exprs.extend(int(second).exprs);
            par
        };
        let mut flagged = pair(1, 2);
        flagged.locally_free = vec![1];
        let cases = [
            (pair(1, 2), pair(2, 1), true),
            (pair(1, 2), flagged, true),
            (int(1), int(2), false),
            (string("a"), string("a"), true),
            (Par::default(), Par::default(), true),
            (
                list(vec![int(1), int(2)]),
                list(vec![int(2), int(1)]),
                false,
            ),
        ];
        for (index, (left, right, expected)) in cases.into_iter().enumerate() {
            let ret = reply_name(u8::try_from(index).expect("a small index"));
            let reply = assert_equal(
                &fixture.contract,
                call(vec![ret.clone(), left, right], b"assert"),
            )
            .await
            .expect("assertEqual replies");
            assert_eq!(reply, vec![new_gbool_par(expected, Vec::new(), false)]);
            assert_eq!(fixture.data(&ret).await, vec![reply]);
        }
        assert!(matches!(
            assert_equal(
                &fixture.contract,
                call(vec![reply_name(9), int(1)], b"assert")
            )
            .await,
            Err(InterpreterError::IllegalArgumentError(_))
        ));
    }

    #[tokio::test]
    async fn space_system_ops_get_space_agent_capability() {
        let fixture = Fixture::new().await;
        let space = fixture
            .create(b"agent space")
            .await
            .expect("the factory creates the space");
        let expected = definitions(&fixture.runtime)[0].to_urn_map().1;
        let this = namespace::gprivate(namespace::handle_channel(space));
        for (index, reference) in [
            handle_capability(space),
            this.clone(),
            new_bundle_par(this, true, true),
        ]
        .into_iter()
        .enumerate()
        {
            let ret = reply_name(u8::try_from(index).expect("a small index"));
            let reply = get_space_agent(
                &fixture.runtime,
                &fixture.agent,
                &fixture.contract,
                call(vec![ret.clone(), reference], b"get agent"),
            )
            .await
            .expect("getSpaceAgent replies");
            assert_eq!(reply, vec![expected.clone()]);
            assert_eq!(fixture.data(&ret).await, vec![reply]);
        }
    }

    #[tokio::test]
    async fn space_system_ops_get_space_agent_dangling_handle() {
        let fixture = Fixture::new().await;
        let ret = reply_name(1);
        let reply = get_space_agent(
            &fixture.runtime,
            &fixture.agent,
            &fixture.contract,
            call(
                vec![ret.clone(), handle_capability(space_of(b"never created"))],
                b"dangling",
            ),
        )
        .await
        .expect("a dangling handle gets a typed reply");
        assert_eq!(reply, vec![typed(SpaceReplyCode::DanglingHandle)]);
        assert_eq!(fixture.data(&ret).await, vec![reply]);
    }

    #[tokio::test]
    async fn space_system_ops_get_space_agent_not_native() {
        let fixture = Fixture::new().await;
        let space = space_of(b"references");
        let this = namespace::gprivate(namespace::handle_channel(space));
        let cases = [
            (string("an agent"), SpaceReplyCode::NotANativeSpace),
            (Par::default(), SpaceReplyCode::EmptySpaceRef),
            (
                new_bundle_par(this, false, true),
                SpaceReplyCode::HandleNotWritable,
            ),
            (
                namespace::gprivate(namespace::config_channel(space)),
                SpaceReplyCode::NotASpaceRef,
            ),
            (
                namespace::gprivate(namespace::name_id(space, NameTail::Random([1; 32]))),
                SpaceReplyCode::NotASpaceRef,
            ),
            (
                namespace::gprivate(namespace::op_channel(GET_SPACE_AGENT)),
                SpaceReplyCode::NotASpaceRef,
            ),
        ];
        for (index, (reference, code)) in cases.into_iter().enumerate() {
            let ret = reply_name(u8::try_from(index).expect("a small index"));
            let reply = get_space_agent(
                &fixture.runtime,
                &fixture.agent,
                &fixture.contract,
                call(vec![ret.clone(), reference], b"references"),
            )
            .await
            .expect("a typed reply");
            assert_eq!(reply, vec![typed(code)], "{code:?}");
            assert_eq!(fixture.data(&ret).await, vec![reply]);
        }
    }

    #[tokio::test]
    async fn space_system_ops_fail_closed_without_a_host_runtime() {
        for (runtime, code) in [
            (unavailable(), "RuntimeUnavailable"),
            (native_replay(), "SpacesUnsupportedInNativeReplay"),
        ] {
            let fixture = Fixture::with_runtime(runtime).await;
            let ret = reply_name(1);
            match get_space_agent(
                &fixture.runtime,
                &fixture.agent,
                &fixture.contract,
                call(
                    vec![ret.clone(), handle_capability(space_of(b"no host"))],
                    b"no host",
                ),
            )
            .await
            {
                Err(InterpreterError::ReduceError(message)) => {
                    assert!(
                        message.starts_with(&format!("reified:{code}: ")),
                        "{message}"
                    );
                }
                other => panic!("getSpaceAgent without a host returned {other:?}"),
            }
            assert!(fixture.data(&ret).await.is_empty());
            let reply = get_space_agent(
                &fixture.runtime,
                &fixture.agent,
                &fixture.contract,
                call(vec![ret.clone(), string("an agent")], b"no host"),
            )
            .await
            .expect("a user agent needs no host");
            assert_eq!(reply, vec![typed(SpaceReplyCode::NotANativeSpace)]);
            let equal = assert_equal(
                &fixture.contract,
                call(vec![reply_name(2), int(3), int(3)], b"no host"),
            )
            .await
            .expect("assertEqual needs no host");
            assert_eq!(equal, vec![new_gbool_par(true, Vec::new(), false)]);
        }
    }

    #[tokio::test]
    async fn space_system_ops_dispatch_through_the_runtime() {
        let reduction = Reduction::new().await;
        let assert_capability = new_bundle_par(ASSERT_EQUAL_PROCESS.channel(), true, false);
        let equal = reply_name(1);
        let unequal = reply_name(2);
        let term = {
            let mut term = send(assert_capability.clone(), vec![
                equal.clone(),
                int(4),
                int(4),
            ]);
            term.sends
                .extend(send(assert_capability, vec![unequal.clone(), int(4), int(5)]).sends);
            term
        };
        reduction
            .evaluate(term, b"dispatch")
            .await
            .expect("the deploy evaluates");
        assert_eq!(reduction.data(&equal).await, vec![vec![new_gbool_par(
            true,
            Vec::new(),
            false
        )]]);
        assert_eq!(reduction.data(&unequal).await, vec![vec![new_gbool_par(
            false,
            Vec::new(),
            false
        )]]);
        let agent_ret = reply_name(3);
        let term = send(
            new_bundle_par(GET_SPACE_AGENT_PROCESS.channel(), true, false),
            vec![agent_ret.clone(), string("an agent")],
        );
        reduction
            .evaluate(term, b"get agent")
            .await
            .expect("the deploy evaluates");
        assert_eq!(reduction.data(&agent_ret).await, vec![vec![typed(
            SpaceReplyCode::NotANativeSpace
        )]]);
    }
}
