//! The space factory process (P4 design C.7, body ref `200 + i` for family `i`).
//!
//! A call `F!(ret, ctor)` on the factory channel of a family creates a new space:
//!
//! 1. The random state `r` and the cost authority `α` are those of the call datum.
//! 2. `FactoryConstructor::validate` checks the constructor list `ctor`.
//! 3. The space id is `sid = r.split_byte(0xE0).split_byte(0x01).next()` (I-19,
//!    `types::id::derive_space_id`).
//! 4. `ReifiedRuntime::create_space` runs one `Intent::Space` job whose footprint holds
//!    `[2]‖sid` and `H(sid)`. The agent locks `C(sid)`, `S(sid)` and `H(sid)`, fails with
//!    `SpaceAlreadyExists` when `C(sid)` holds a record, and stages the config record, the
//!    space counter `S(sid) = 0` and the handle continuation on `H(sid)` (steps 5 to 7).
//! 5. After the commit, the factory replies `bundle+{H(sid)}` on `ret` (step 8).
//!
//! Every failure aborts the deploy. The reducer aborts a persistent send to a factory before
//! the call (A21), so no call repeats.
//!
//! The handle continuation is a persistent single-channel receive on `this = H(sid)` with the
//! pattern `[...@args]`. Its body `[0xE0, 0x30]!(this, args)` forwards each call to
//! `HANDLE_OP`, which accepts only the bare `this` (A6). The forwarder has no guard and no
//! cost authority, so each call is paid by its caller (P4 design B.7).

use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::create_bit_vector;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, ListParWithRandom, Par, ParWithRandom, Send as SendProcess, TaggedContinuation,
};
use models::rust::space_factory_urn::SpaceFactoryUrn;
use models::rust::utils::{new_boundvar_par, new_bundle_par, new_freevar_var};

use super::runtime::ReifiedRuntime;
use super::system_channels::HANDLE_OP_PROCESS;
use crate::rust::interpreter::contract_call::ContractCall;
use crate::rust::interpreter::errors::{illegal_argument_error, InterpreterError};
use crate::rust::interpreter::spaces::agent::{SpaceAgent, SpaceCreation};
use crate::rust::interpreter::spaces::factory::constructor::FactoryConstructor;
use crate::rust::interpreter::spaces::namespace::{self, NativeSpace, RandomPurpose};
use crate::rust::interpreter::spaces::types::id::derive_space_id;

/// The arguments of a system process call: the call data, the replay flag and the output of
/// the call in the replayed log.
pub type ContractArgs = (Vec<ListParWithRandom>, bool, Vec<Par>);

/// The factory process of one space family.
#[derive(Clone)]
pub struct FactoryProcess {
    family: SpaceFactoryUrn,
    runtime: Arc<ReifiedRuntime>,
    agent: Arc<dyn SpaceAgent>,
}

impl FactoryProcess {
    /// The factory of `family`. `agent` stages the records of each new space.
    pub fn new(
        family: SpaceFactoryUrn,
        runtime: Arc<ReifiedRuntime>,
        agent: Arc<dyn SpaceAgent>,
    ) -> Self {
        Self {
            family,
            runtime,
            agent,
        }
    }

    /// The family of the factory.
    pub fn family(&self) -> SpaceFactoryUrn { self.family }

    /// Creates a space for the call `F!(ret, ctor)` and replies its handle capability on `ret`.
    pub async fn call(
        &self,
        contract: &ContractCall,
        contract_args: ContractArgs,
    ) -> Result<Vec<Par>, InterpreterError> {
        let illegal = || illegal_argument_error(&self.family.to_string());
        let (random, authority) = match contract_args.0.as_slice() {
            [call] => (
                Blake2b512Random::from_bytes(&call.random_state),
                call.cost_authority.clone().unwrap_or_default(),
            ),
            _ => return Err(illegal()),
        };
        let Some((produce, _, _, args)) = contract.unapply(contract_args) else {
            return Err(illegal());
        };
        let [ret, ctor] = args.as_slice() else {
            return Err(illegal());
        };
        let constructor = FactoryConstructor::validate(self.family, ctor)?;
        let space = NativeSpace {
            sid: derive_space_id(&random),
            durability: constructor.durability(),
        };
        let creation = SpaceCreation {
            space,
            config: constructor.config(space).to_par(),
            handle_pattern: handle_pattern(),
            handle_continuation: handle_continuation(space, &random),
            random,
            authority,
            family: self.family,
        };
        self.runtime
            .create_space(Arc::clone(&self.agent), creation)
            .await?;
        let reply = vec![handle_capability(space)];
        produce(&reply, ret).await?;
        Ok(reply)
    }
}

/// The pattern of the handle continuation: `[...@args]`. The remainder binds the whole call
/// as one list.
pub fn handle_pattern() -> BindPattern {
    BindPattern {
        patterns: Vec::new(),
        remainder: Some(new_freevar_var(0)),
        free_count: 1,
    }
}

/// The handle continuation of `space`: the body `[0xE0, 0x30]!(this, args)` with the random
/// state `random.split_byte(0xE0).split_byte(0x06)`, no guard and no cost authority.
pub fn handle_continuation(space: NativeSpace, random: &Blake2b512Random) -> TaggedContinuation {
    let args_bound = create_bit_vector(&[0]);
    let forward = SendProcess {
        chan: Some(HANDLE_OP_PROCESS.channel()),
        data: vec![
            namespace::gprivate(namespace::handle_channel(space)),
            new_boundvar_par(0, args_bound.clone(), false),
        ],
        persistent: false,
        locally_free: args_bound.clone(),
        connective_used: false,
    };
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(
                Par::default()
                    .with_sends(vec![forward])
                    .with_locally_free(args_bound),
            ),
            random_state: RandomPurpose::Handle.derive(random).to_bytes(),
        })),
        guard: None,
        cost_authority: None,
    }
}

/// The handle capability of `space` that the factory replies: `bundle+{H(sid)}`.
pub fn handle_capability(space: NativeSpace) -> Par {
    new_bundle_par(
        namespace::gprivate(namespace::handle_channel(space)),
        true,
        false,
    )
}

#[cfg(test)]
mod tests {
    use rspace_plus_plus::rspace::rspace_interface::ISpace;

    use super::*;
    use crate::rust::interpreter::reified::runtime::{native_replay, unavailable};
    use crate::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
    use crate::rust::interpreter::reified::system_processes::harness::{
        call, int, list, reply_name, space_of, string, Fixture, Reduction,
    };
    use crate::rust::interpreter::reified::system_processes::native_agent;
    use crate::rust::interpreter::spaces::factory::constructor::family_of;
    use crate::rust::interpreter::spaces::namespace::{Durability, HANDLE_OP, REIFIED_TAG};
    use crate::rust::interpreter::spaces::types::config::SpaceConfig;

    async fn record_counts(fixture: &Fixture, space: NativeSpace) -> (usize, usize, usize) {
        let config = namespace::gprivate(namespace::config_channel(space));
        let counter = namespace::gprivate(namespace::space_counter(space));
        let handle = namespace::gprivate(namespace::handle_channel(space));
        (
            fixture.rspace.get_data(&config).await.len(),
            fixture.rspace.get_data(&counter).await.len(),
            fixture
                .rspace
                .get_waiting_continuations(vec![handle])
                .await
                .len(),
        )
    }

    #[tokio::test]
    async fn space_already_exists_aborts() {
        let fixture = Fixture::new().await;
        let factory = fixture.factory();
        let first = reply_name(1);
        let reply = factory
            .call(
                &fixture.contract,
                call(vec![first.clone(), list(Vec::new())], b"twice"),
            )
            .await
            .expect("the first call creates the space");
        let space = space_of(b"twice");
        assert_eq!(reply, vec![handle_capability(space)]);
        assert_eq!(fixture.data(&first).await, vec![reply]);
        assert_eq!(record_counts(&fixture, space).await, (1, 1, 1));
        let second = reply_name(2);
        match factory
            .call(
                &fixture.contract,
                call(vec![second.clone(), list(Vec::new())], b"twice"),
            )
            .await
        {
            Err(InterpreterError::ReduceError(message)) => {
                assert!(
                    message.starts_with("reified:SpaceAlreadyExists: "),
                    "{message}"
                );
            }
            other => panic!("a second creation of one space returned {other:?}"),
        }
        assert!(fixture.data(&second).await.is_empty());
        assert_eq!(record_counts(&fixture, space).await, (1, 1, 1));
        let third = reply_name(3);
        let other = factory
            .call(
                &fixture.contract,
                call(vec![third, list(Vec::new())], b"another"),
            )
            .await
            .expect("another call creates another space");
        assert_ne!(space_of(b"another"), space);
        assert_eq!(other, vec![handle_capability(space_of(b"another"))]);
    }

    #[tokio::test]
    async fn reified_factory_creates_config_counter_handle() {
        let fixture = Fixture::new().await;
        let random = Blake2b512Random::create_from_bytes(b"records");
        let ret = reply_name(1);
        let reply = fixture
            .factory()
            .call(
                &fixture.contract,
                call(vec![ret.clone(), list(vec![string("default")])], b"records"),
            )
            .await
            .expect("the factory creates the space");
        let space = NativeSpace {
            sid: derive_space_id(&random),
            durability: Durability::Durable,
        };
        let mut expected_sid = random.split_byte(REIFIED_TAG).split_byte(0x01);
        assert_eq!(
            space.sid.to_vec(),
            expected_sid
                .next()
                .into_iter()
                .map(|byte| byte as u8)
                .collect::<Vec<u8>>()
        );
        assert_eq!(reply, vec![handle_capability(space)]);
        assert_eq!(classify_space_ref(&reply[0]), Ok(SpaceRef::Native(space)));
        let config = fixture
            .runtime
            .read_config(Arc::clone(&fixture.agent), space, None)
            .await
            .expect("the config record exists");
        let constructor =
            FactoryConstructor::validate(SpaceFactoryUrn::HASHMAP_QUEUE, &list(Vec::new()))
                .expect("the default constructor is valid");
        assert_eq!(config, constructor.config(space).to_par());
        assert_eq!(record_counts(&fixture, space).await, (1, 1, 1));
        let waiting = fixture
            .rspace
            .get_waiting_continuations(vec![namespace::gprivate(namespace::handle_channel(space))])
            .await;
        let [handle] = waiting.as_slice() else {
            panic!("the handle channel holds {} continuations", waiting.len());
        };
        assert_eq!(handle.patterns, vec![handle_pattern()]);
        assert_eq!(handle.continuation, handle_continuation(space, &random));
        assert!(handle.persist);
        assert!(handle.peeks.is_empty());
    }

    #[tokio::test]
    async fn reified_factory_rejects_bad_constructor_before_any_effect() {
        let fixture = Fixture::new().await;
        let cases = [
            (list(vec![string("temp")]), "QualifierUnavailable"),
            (list(vec![string("seq")]), "QualifierUnavailable"),
            (string("default"), "BadConstructorArgument"),
            (list(vec![int(1)]), "BadConstructorArgument"),
            (
                list(vec![string("default"), string("default")]),
                "BadConstructorArgument",
            ),
        ];
        for (index, (ctor, code)) in cases.into_iter().enumerate() {
            let seed = [u8::try_from(index).expect("a small index")];
            let ret = reply_name(seed[0]);
            match fixture
                .factory()
                .call(&fixture.contract, call(vec![ret.clone(), ctor], &seed))
                .await
            {
                Err(InterpreterError::ReduceError(message)) => {
                    assert!(
                        message.starts_with(&format!("reified:{code}: ")),
                        "{message}"
                    );
                }
                other => panic!("a bad constructor returned {other:?}"),
            }
            assert!(fixture.data(&ret).await.is_empty());
            assert_eq!(record_counts(&fixture, space_of(&seed)).await, (0, 0, 0));
        }
        assert!(matches!(
            fixture
                .factory()
                .call(&fixture.contract, call(vec![reply_name(9)], b"arity"))
                .await,
            Err(InterpreterError::IllegalArgumentError(_))
        ));
    }

    #[tokio::test]
    async fn reified_factory_fails_closed_without_a_host_runtime() {
        for (runtime, code) in [
            (unavailable(), "RuntimeUnavailable"),
            (native_replay(), "SpacesUnsupportedInNativeExecution"),
        ] {
            let fixture = Fixture::with_runtime(runtime).await;
            let ret = reply_name(1);
            match fixture
                .factory()
                .call(
                    &fixture.contract,
                    call(vec![ret.clone(), list(Vec::new())], b"no host"),
                )
                .await
            {
                Err(InterpreterError::ReduceError(message)) => {
                    assert!(
                        message.starts_with(&format!("reified:{code}: ")),
                        "{message}"
                    );
                }
                other => panic!("a factory without a host returned {other:?}"),
            }
            assert!(fixture.data(&ret).await.is_empty());
        }
    }

    #[test]
    fn reified_factory_handle_continuation_forwards_to_handle_op() {
        let random = Blake2b512Random::create_from_bytes(b"shape");
        let space = space_of(b"shape");
        assert_eq!(handle_pattern(), BindPattern {
            patterns: Vec::new(),
            remainder: Some(new_freevar_var(0)),
            free_count: 1,
        });
        let continuation = handle_continuation(space, &random);
        assert_eq!(continuation.guard, None);
        assert_eq!(continuation.cost_authority, None);
        let Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(body),
            random_state,
        })) = &continuation.tagged_cont
        else {
            panic!("the handle continuation is not a body: {continuation:?}");
        };
        assert_eq!(
            random_state,
            &random.split_byte(REIFIED_TAG).split_byte(0x06).to_bytes()
        );
        assert_eq!(body.locally_free, create_bit_vector(&[0]));
        let [forward] = body.sends.as_slice() else {
            panic!("the handle body is not one send: {body:?}");
        };
        assert_eq!(
            forward.chan,
            Some(namespace::gprivate(namespace::op_channel(HANDLE_OP)))
        );
        assert!(!forward.persistent);
        assert_eq!(forward.data, vec![
            namespace::gprivate(namespace::handle_channel(space)),
            new_boundvar_par(0, create_bit_vector(&[0]), false),
        ]);
        assert_eq!(forward.locally_free, create_bit_vector(&[0]));
        assert_ne!(
            handle_continuation(space_of(b"other"), &random),
            continuation
        );
        assert_ne!(
            handle_continuation(space, &Blake2b512Random::create_from_bytes(b"other")),
            continuation
        );
        assert_eq!(space_of(b"shape").sid, derive_space_id(&random));
        assert_ne!(space_of(b"other").sid, derive_space_id(&random));
        assert_eq!(
            handle_capability(space),
            new_bundle_par(
                namespace::gprivate(namespace::handle_channel(space)),
                true,
                false
            )
        );
    }

    #[tokio::test]
    async fn reified_factory_runs_through_the_reducer() {
        let reduction = Reduction::new().await;
        let ret = reply_name(1);
        let index = SpaceFactoryUrn::HASHMAP_QUEUE
            .family_index()
            .expect("a family index");
        let factory = new_bundle_par(
            namespace::gprivate(namespace::op_channel(index)),
            true,
            false,
        );
        let term = Par::default().with_sends(vec![SendProcess {
            chan: Some(factory),
            data: vec![ret.clone()],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        }]);
        reduction
            .evaluate(term, b"through the reducer")
            .await
            .expect("the factory call evaluates");
        let replies = reduction.data(&ret).await;
        let [reply] = replies.as_slice() else {
            panic!("the factory replied {replies:?}");
        };
        let [capability] = reply.as_slice() else {
            panic!("the reply is not one capability: {reply:?}");
        };
        let Ok(SpaceRef::Native(space)) = classify_space_ref(capability) else {
            panic!("the reply is not a native handle: {capability:?}");
        };
        let config = reduction
            .runtime
            .read_config(native_agent(), space, None)
            .await
            .expect("the space exists");
        let config = SpaceConfig::from_par(&config).expect("a canonical config");
        assert_eq!(family_of(&config), Ok(SpaceFactoryUrn::HASHMAP_QUEUE));
    }
}
