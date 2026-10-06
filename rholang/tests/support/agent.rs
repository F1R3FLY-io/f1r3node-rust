//! An in-memory agent environment over play and replay (P5 design v3, A.3.2).
//!
//! An agent test runs space operations with an agent directly, without a Rholang runtime: it
//! creates a space with `prepare_create`, runs each operation with `run_op`, and reads the
//! records, the waiters and the counters of a collection from the host store. The helpers are
//! the ones of the agent unit tests in `spaces/generic.rs`, over the public interface.

use std::collections::BTreeMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, Expr, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::space_factory_urn::SpaceFactoryUrn;
use models::rust::utils::{new_freevar_par, new_freevar_var, new_gint_par, new_gstring_par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::costs::ScanMeter;
use rholang::rust::interpreter::spaces::agent::{
    run_op, ConsumeOp, ProduceOp, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOp, SpaceOutcome,
};
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::namespace::{self, CollectionKey, NativeSpace};
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{SpaceStoreHandle, SpaceTxnHost};

/// The play space of an agent test.
pub type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// The replay space of an agent test.
pub type ParReplay = ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// The phlo budget of an agent test.
pub fn budget() -> RuntimeBudget { RuntimeBudget::new(Cost::create(1_000_000, "rrs p5 agent test")) }

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
pub fn receive_on(space: NativeSpace, channel: &Par, name: &str, pattern: BindPattern) -> ConsumeOp {
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
            let ord = record::continuation_ord(&waiter.continuation)
                .expect("a continuation record");
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
