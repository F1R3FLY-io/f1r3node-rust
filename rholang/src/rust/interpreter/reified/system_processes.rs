//! The Reified system processes of P4 and P5 (P4 design A.5 and C.9, P5 design v3 A.7).
//!
//! | URN                             | Process                                   |
//! |---------------------------------|-------------------------------------------|
//! | a family URN or a FIPS alias    | the factory of the family (`factory.rs`)  |
//! | `rho:space:getSpaceAgent`       | `getSpaceAgent!(ret, s)`                  |
//! | `rho:assert:assertEqual`        | `assertEqual!(ret, a, b)` (decision 33)   |
//! | `rho:reified:internal:handleOp` | the handle process (`handle.rs`, A6)      |
//! | `rho:lang:vector`               | `vec!(ret, op, x…)` (P6 step 6)           |
//! | `rho:vectordb:embed`            | `embed!(ret, text)` (P6 step 7, recorded) |
//! | `rho:vectordb:ann`              | `ann!(ret, space, channel, query, options)` (P6 step 7, recorded) |
//! | `rho:reified:internal:matcherOp` | the matcher process of user agents (`user_agent.rs`, P8) |
//! | `rho:reified:internal:sinkOp`   | the sink of the calls of a translation (`user_agent.rs`, P8) |
//!
//! `create_rho_env` calls [`register`] with the runtime of the host store, before
//! `setup_maps_and_refs` binds the URNs. `dispatch_table_creator` drains the vector, so a
//! vector that serves two runtimes never holds a definition twice. Every process has a fixed
//! arity (I-18).
//!
//! `getSpaceAgent!(ret, s)` classifies `s` with `classify_space_ref`. For a native space it
//! reads `C(sid)` in a read-only job and replies the capability of the factory of the family.
//! That capability is the urn map value of the family URN and of its alias (decision 9.4).
//! Every other result is a typed error reply (A22): `DanglingHandle` for a missing config
//! record, `NotANativeSpace` for a user agent, and the code of the reference error otherwise.
//! `assertEqual!(ret, a, b)` replies `true` for equal `a` and `b` after `ParSortMatcher`.
//!
//! `vec!(ret, op, x…)` reaches the process as `[ret, [op, x…]]`, because the reducer packs the
//! arguments after `ret` (I-18). `tensor::ops` computes the value, and the process replies
//! `[true, value]` or `[false, code, detail]` on `ret` (decision 57).
//!
//! `embed` and `ann` are recorded processes (decision 57). In play they ask the services of the
//! node (`vectordb_services`), and the dispatcher records the reply of the call on its produce
//! event. Replay delivers the recorded reply.
//!
//! Changed by decisions 104 to 106 (DR-114 oracle mode): both processes follow the recorded
//! process contract of `feature/cost-accounted-rho`. They are in its recorded set
//! (`is_native_recorded_op`). A failure replies `[false, code, message]` with a base code, and the
//! dispatcher records it too. A success of play takes its bytes from the record room of the deploy
//! (`native_played_reply`). Replay makes no check and delivers the record
//! (`native_replayed_reply`). A persistent send to either process fails at its fire, by base rule
//! P2. Before, replay made the checks of `ann` too, the failures had Reified codes, and rule A21
//! aborted a persistent send.
//!
//! Changed by decision 116: the two processes keep their own handlers, for two reasons. The reply
//! channel `ret` comes first, as the File I/O sugar requires, but the base driver takes its ack
//! last. `ann` also reads the Reified runtime, which the base driver does not hold.
//!
//! Changed by decision 113: only a build with the feature `chromadb` registers `embed` and `ann`.
//! The VectorDB space type exists in every build.

use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
// Changed by decision 90 (DR-132): no parameter names `CostAuthority` now.
// use models::rhoapi::{CostAuthority, EList, Par};
use models::rhoapi::{EList, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::space_factory_urn::SpaceInner;
use models::rust::utils::{new_gbool_par, new_wildcard_var};
use prost::Message;

use super::factory::{ContractArgs, FactoryProcess};
use super::handle::{typed_error, HandleProcess};
use super::runtime::ReifiedRuntime;
use super::scope::{classify_space_ref, SpaceRef};
use super::system_channels::{
    SystemChannel, ANN_PROCESS, ASSERT_EQUAL_PROCESS, EMBED_PROCESS, GET_SPACE_AGENT_PROCESS,
    HANDLE_OP_PROCESS, MATCHER_OP_PROCESS, P4_SYSTEM_CHANNELS, P6_PROCESSES, P8_PROCESSES,
    SINK_OP_PROCESS, VECTOR_PROCESS,
};
use super::user_agent::{self, MatcherProcess};
// Changed by decision 105: the base codes replace the Reified codes, and the base helpers build
// the replies.
// use super::vectordb_services::{
//     self, channel_digest, collection_name, f32_list, failed_reply, is_ok_reply, list_par, ok_reply,
//     EmbedFailure, Neighbour, ServiceReplyCode, VectorDbServices,
// };
use super::vectordb_services::{
    self, channel_digest, collection_name, f32_list, list_par, ok_reply, EmbedFailure, Neighbour,
    VectorDbServices,
};
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::contract_call::{ContractCall, Producer};
// Changed by decision 90 (DR-132): the scheduler and its space jobs are gone.
// use crate::rust::interpreter::deterministic_reduction::reified_schedule::{
//     submit_space, SpaceRequest,
// };
use crate::rust::interpreter::errors::{illegal_argument_error, InterpreterError};
use crate::rust::interpreter::spaces::agent::SpaceAgent;
use crate::rust::interpreter::spaces::disciplines::vectordb::{vdb_config, ID_LEN};
use crate::rust::interpreter::spaces::errors::{SpaceError, SpaceReplyCode};
use crate::rust::interpreter::spaces::factory::constructor::family_of;
use crate::rust::interpreter::spaces::factory::defaults::MIRROR_PARAM;
use crate::rust::interpreter::spaces::factory::urn::{self, factory_capability};
use crate::rust::interpreter::spaces::families::FamilyAgent;
use crate::rust::interpreter::spaces::namespace::{CollectionKey, NativeSpace, SpaceId};
use crate::rust::interpreter::spaces::record::{expr_par, sole_expr};
use crate::rust::interpreter::spaces::types::config::{map_entries, SpaceConfig};
// Changed by decision 105: the base codes replace the Reified codes, and the base helpers build
// the replies.
// use crate::rust::interpreter::system_processes::{Definition, ProcessContext, Remainder};
// use crate::rust::interpreter::tensor::num::{self, NumError};
use crate::rust::interpreter::system_processes::{
    native_failure_reply, native_played_reply, native_replayed_reply, Definition, ProcessContext,
    Remainder, EXT_BAD_ARG, EXT_FAILED,
};
use crate::rust::interpreter::tensor::num;
use crate::rust::interpreter::tensor::ops::{self, OpFailure};

/// The native agent of every wired family: the family agent (P5 design v3, A.3.4). It reads the
/// config of a space once and runs the agent of the family that the config names. Every config
/// record has one format, so the agent also reads the config of any space.
pub fn native_agent() -> Arc<dyn SpaceAgent> { Arc::new(FamilyAgent) }

/// The definitions of the Reified system processes of `runtime`. The four processes of P4 come
/// first, in their P4 order: the factory of `hashmap:queue`, `getSpaceAgent`, `assertEqual` and
/// the handle process. Then come the factory of each registered family of P5, in family-index
/// order, the factory of each registered alias (`urn::registered_p5_families`,
/// `urn::registered_aliases`), and the factory of each registered family of P6
/// (`urn::registered_p6_families`), then the processes of P6 (`P6_PROCESSES`), and last the
/// factory of each registered family of P7 (`urn::registered_p7_families`), so every earlier
/// definition keeps its place. An alias has the channel, the arity, the body ref and the
/// handler of its family, so the second install of its channel changes nothing: the install
/// map and the dispatch table are keyed by the channel and by the body ref. P8 appends the
/// matcher process and the sink of user agents (`P8_PROCESSES`) after the factories of P7.
/// Added by decision 127 (kind registry): the factories of the registered kinds come last, then
/// their system processes, each in op order.
pub fn definitions(runtime: &Arc<ReifiedRuntime>) -> Vec<Definition> {
    let agent = native_agent();
    let families: Vec<_> = urn::registered_p5_families().collect();
    let aliases: Vec<_> = urn::registered_aliases().collect();
    let p6_families: Vec<_> = urn::registered_p6_families().collect();
    let p7_families: Vec<_> = urn::registered_p7_families().collect();
    // Added by decision 127 (kind registry): the factories and the processes of the registered
    // kinds, in op order.
    let kind_families: Vec<_> = urn::registered_kind_families().collect();
    let mut kind_processes: Vec<&SpaceProcess> =
        crate::rust::interpreter::spaces::families::kind_processes().collect();
    kind_processes.sort_by_key(|process| process.channel.op);
    let mut definitions = Vec::with_capacity(
        P4_SYSTEM_CHANNELS.len()
            + families.len()
            + aliases.len()
            + p6_families.len()
            + P6_PROCESSES.len()
            + p7_families.len()
            + P8_PROCESSES.len(),
    );
    // Added by decision 127 (kind registry): room for the definitions of the registered kinds.
    definitions.reserve(kind_families.len() + kind_processes.len());
    let factory = |family, channel| {
        definition(
            channel,
            ReifiedProcess::Factory(FactoryProcess::new(
                family,
                Arc::clone(runtime),
                Arc::clone(&agent),
            )),
        )
    };
    for (family, channel) in urn::REGISTERED_FAMILIES {
        definitions.push(factory(family, channel));
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
        ReifiedProcess::Handle(HandleProcess::new(Arc::clone(runtime), Arc::clone(&agent))),
    ));
    for (family, channel) in families {
        definitions.push(factory(family, channel));
    }
    for (channel, family) in aliases {
        definitions.push(factory(family, channel));
    }
    for (family, channel) in p6_families {
        definitions.push(factory(family, channel));
    }
    definitions.push(definition(VECTOR_PROCESS, ReifiedProcess::Vector));
    // Changed by decision 113: only a build with the feature `chromadb` registers `embed` and
    // `ann`, as the base does for its Chroma processes (`rho_runtime.rs`).
    #[cfg(feature = "chromadb")]
    definitions.push(definition(EMBED_PROCESS, ReifiedProcess::Embed));
    #[cfg(feature = "chromadb")]
    definitions.push(definition(ANN_PROCESS, ReifiedProcess::Ann {
        runtime: Arc::clone(runtime),
        agent: Arc::clone(&agent),
    }));
    for (family, channel) in p7_families {
        definitions.push(factory(family, channel));
    }
    definitions.push(definition(
        MATCHER_OP_PROCESS,
        ReifiedProcess::Matcher(MatcherProcess::new(Arc::clone(runtime))),
    ));
    definitions.push(definition_with_remainder(
        SINK_OP_PROCESS,
        ReifiedProcess::Sink,
        Some(new_wildcard_var()),
    ));
    // Added by decision 127 (kind registry): the factories of the registered kinds, then their
    // system processes.
    for (family, channel) in kind_families {
        definitions.push(factory(family, channel));
    }
    for process in kind_processes {
        definitions.push((process.definition)(runtime));
    }
    definitions
}

/// Adds the Reified system processes of `runtime` to `processes`.
pub fn register(processes: &mut Vec<Definition>, runtime: &Arc<ReifiedRuntime>) {
    processes.extend(definitions(runtime));
}

/// Added by decision 127 (kind registry): a system process of a registered kind: its channel,
/// whether its handler records and replays its replies (DR-114), and its definition.
pub struct SpaceProcess {
    /// The URN, the op and the arity of the process.
    pub channel: SystemChannel,
    /// True for a process whose handler records its reply in play and replays it, as the
    /// recorded built-in processes do. Base rule P2 then fails a persistent send to it at its
    /// fire.
    pub recorded: bool,
    /// The definition of the process for the runtime `runtime`.
    pub definition: fn(&Arc<ReifiedRuntime>) -> Definition,
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
    Vector,
    // Changed by decision 113: only a chromadb build constructs these two.
    #[cfg_attr(not(feature = "chromadb"), allow(dead_code))]
    Embed,
    #[cfg_attr(not(feature = "chromadb"), allow(dead_code))]
    Ann {
        runtime: Arc<ReifiedRuntime>,
        agent: Arc<dyn SpaceAgent>,
    },
    /// The matcher process of user agents (P8, design v2 MO-1).
    Matcher(MatcherProcess),
    /// The sink of the calls of a translation (P8, design v2 SK-1).
    Sink,
}

impl ReifiedProcess {
    /// Runs the process on the call `contract_args`. `cost` is the budget of the deploy: the
    /// recorded processes `embed` and `ann` take the record room of their replies from it
    /// (decision 68).
    async fn call(
        &self,
        contract: &ContractCall,
        contract_args: ContractArgs,
        cost: &RuntimeBudget,
    ) -> Result<Vec<Par>, InterpreterError> {
        match self {
            ReifiedProcess::Factory(factory) => factory.call(contract, contract_args).await,
            ReifiedProcess::GetSpaceAgent { runtime, agent } => {
                get_space_agent(runtime, agent, contract, contract_args).await
            }
            ReifiedProcess::AssertEqual => assert_equal(contract, contract_args).await,
            ReifiedProcess::Handle(handle) => handle.call(contract, contract_args).await,
            ReifiedProcess::Vector => vector(contract, contract_args).await,
            ReifiedProcess::Embed => embed(contract, contract_args, cost).await,
            ReifiedProcess::Ann { runtime, agent } => {
                ann(runtime, agent, contract, contract_args, cost).await
            }
            ReifiedProcess::Matcher(matcher) => matcher.call(contract, contract_args).await,
            ReifiedProcess::Sink => user_agent::sink(contract_args).await,
        }
    }
}

fn definition(channel: SystemChannel, process: ReifiedProcess) -> Definition {
    definition_with_remainder(channel, process, None)
}

/// The definition of `process` on `channel` with the remainder `remainder` of its install
/// pattern. Every process but the sink of P8 has no remainder (I-18). The sink has the arity 0
/// and a wildcard remainder, so it takes a message of any arity (design v2 TS-2).
fn definition_with_remainder(
    channel: SystemChannel,
    process: ReifiedProcess,
    remainder: Remainder,
) -> Definition {
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
            let cost = context.cost;
            Box::new(move |contract_args| {
                let process = Arc::clone(&process);
                let contract = ContractCall {
                    space: space.clone(),
                    dispatcher: dispatcher.clone(),
                };
                let cost = cost.clone();
                Box::pin(async move { process.call(&contract, contract_args, &cost).await })
            })
        }),
        remainder,
    )
}

/// `getSpaceAgent!(ret, s)` (P4 design C.9, A22).
async fn get_space_agent(
    runtime: &Arc<ReifiedRuntime>,
    agent: &Arc<dyn SpaceAgent>,
    contract: &ContractCall,
    contract_args: ContractArgs,
) -> Result<Vec<Par>, InterpreterError> {
    // Changed by decision 90 (DR-132): the config read takes no cost authority, so only the
    // arity of the call is checked here.
    // let authority = match contract_args.0.as_slice() {
    //     [call] => call.cost_authority.clone(),
    //     _ => return Err(illegal_argument_error(GET_SPACE_AGENT_PROCESS.urn)),
    // };
    let [_] = contract_args.0.as_slice() else {
        return Err(illegal_argument_error(GET_SPACE_AGENT_PROCESS.urn));
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
        // DG-1: an ephemeral space that an ended deploy created gets the reply before any job.
        Ok(SpaceRef::Native(space)) if runtime.check_live_space(space).is_err() => {
            dangling_reply(space.sid)
        }
        Ok(SpaceRef::Native(space)) => match read_config(runtime, agent, space).await? {
            Ok(config) => {
                let config = SpaceConfig::from_par(&config).map_err(SpaceError::from)?;
                let family = family_of(&config)?;
                factory_capability(family).ok_or(SpaceError::Inconsistent(
                    "the config record names a family without a factory",
                ))?
            }
            Err(SpaceError::DanglingHandle { space }) => dangling_reply(space),
            Err(error) => return Err(error.into()),
        },
    };
    let reply = vec![reply];
    produce(&reply, ret).await?;
    Ok(reply)
}

/// The typed reply `DanglingHandle` of `getSpaceAgent` for the missing space `space` (A22). The
/// detail map names the space id under the key of the config map (DG-3).
fn dangling_reply(space: SpaceId) -> Par {
    typed_error(SpaceReplyCode::DanglingHandle, vec![(
        "spaceId",
        expr_par(ExprInstance::GByteArray(space.to_vec())),
    )])
}

/// Reads the config map of `space` as `ReifiedRuntime::read_config` does: one read-only job
/// that commits nothing. The outer result is the result of the job, and the inner result keeps
/// the typed space error, so `getSpaceAgent` can reply `DanglingHandle`.
///
/// Changed by decision 90 (DR-132): the job runs inline in the counted task of the caller.
/// Before, it was an `Intent::Space` of the scheduler, whose footprint held the parameter
/// `authority: Option<CostAuthority>`. Nothing else used the parameter, so it is gone.
// async fn read_config(
//     runtime: &Arc<ReifiedRuntime>,
//     agent: &Arc<dyn SpaceAgent>,
//     space: NativeSpace,
//     authority: Option<CostAuthority>,
// ) -> Result<Result<Par, SpaceError>, InterpreterError> {
async fn read_config(
    runtime: &Arc<ReifiedRuntime>,
    agent: &Arc<dyn SpaceAgent>,
    space: NativeSpace,
) -> Result<Result<Par, SpaceError>, InterpreterError> {
    // Replaced by decision 46: the mode alone does not decide availability, because a host
    // runtime is unavailable in native play too.
    // match runtime.mode() {
    //     ReifiedRuntimeMode::Host => {}
    //     ReifiedRuntimeMode::Detached => return Err(SpaceError::RuntimeUnavailable.into()),
    //     ReifiedRuntimeMode::NativeReplay => {
    //         return Err(SpaceError::SpacesUnsupportedInNativeReplay.into())
    //     }
    // }
    runtime.ensure_available()?;
    // let runtime = Arc::clone(runtime);
    // let agent = Arc::clone(agent);
    // submit_space(SpaceRequest {
    //     sid: space.sid,
    //     authority,
    //     channels: Vec::new(),
    //     job: Box::new(move || {
    //         Box::pin(async move {
    //             let env = runtime.space_env()?;
    //             Ok(agent.read_config(&env, space).await)
    //         })
    //     }),
    // })
    // .await
    let env = runtime.space_env()?;
    Ok(agent.read_config(&env, space).await)
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

/// `vec!(ret, op, x…)`, packed as `[ret, [op, x…]]` (P6 step 6, design v3 4.7).
async fn vector(
    contract: &ContractCall,
    contract_args: ContractArgs,
) -> Result<Vec<Par>, InterpreterError> {
    let Some((produce, _, _, args)) = contract.unapply(contract_args) else {
        return Err(illegal_argument_error(VECTOR_PROCESS.urn));
    };
    let [ret, call] = args.as_slice() else {
        return Err(illegal_argument_error(VECTOR_PROCESS.urn));
    };
    let reply = match ops::reply(ops::evaluate(call)) {
        Ok(reply) => reply,
        Err(OpFailure::Inconsistent(reason)) => return Err(SpaceError::Inconsistent(reason).into()),
        Err(OpFailure::Reply(_)) => {
            return Err(SpaceError::Inconsistent("a reply failure became a reply").into())
        }
    };
    let reply = vec![reply];
    produce(&reply, ret).await?;
    Ok(reply)
}

/// The process names of the base codes of `embed` and `ann` (decision 105, DR-114 item 4).
const EMBED_NAME: &str = "vectordb_embed";
const ANN_NAME: &str = "vectordb_ann";

/// `embed!(ret, text)` or `embed!(ret, [text…])` (P6 step 7, decision 57). Play embeds the
/// texts with the embedder of the node and replies `[true, vector, model]` for one text or
/// `[true, [vector…], model]` for a list, with each vector a list of `GFloat32`.
///
/// Changed by decisions 104 and 105 (DR-114): a failure replies `[false, code, message]` with a
/// base code. A bad argument gives `EXT_BAD_ARG`, and a failure of the embedder gives
/// `EXT_FAILED`. Play records a success through `native_played_reply`. Replay delivers the
/// record through `native_replayed_reply`. Before, a failure replied a Reified code, and
/// `within_record_room` and `record_present` made these steps.
async fn embed(
    contract: &ContractCall,
    contract_args: ContractArgs,
    cost: &RuntimeBudget,
) -> Result<Vec<Par>, InterpreterError> {
    let Some((produce, is_replay, previous_output, args)) = contract.unapply(contract_args) else {
        return Err(illegal_argument_error(EMBED_PROCESS.urn));
    };
    let [ret, input] = args.as_slice() else {
        return Err(illegal_argument_error(EMBED_PROCESS.urn));
    };
    // Changed by decision 104 (DR-114): replay delivers the record after the checks of the base
    // driver.
    // record_present(EMBED_PROCESS.urn, is_replay, &previous_output)?;
    // if is_replay {
    //     produce(&previous_output, ret).await?;
    //     return Ok(previous_output);
    // }
    // let output = vec![embed_reply(input).await];
    // Changed by decision 89 (DR-114, decision 68): a success of native play takes its bytes
    // from the record room.
    // produce_recorded(produce, output, ret).await
    // produce_recorded(produce, within_record_room(output, cost), ret).await
    let reply = match is_replay {
        true => native_replayed_reply(EMBED_NAME, previous_output)?,
        false => embed_reply(input, cost).await,
    };
    produce_recorded(produce, reply, ret).await
}

/// The texts of the argument of `embed`: one text, or a non-empty list of texts. The flag is
/// true for one text.
fn texts_of(input: &Par) -> Option<(Vec<String>, bool)> {
    match (sole_expr(input), input.connective_used) {
        (Some(ExprInstance::GString(text)), false) => Some((vec![text.clone()], true)),
        (
            Some(ExprInstance::EListBody(EList {
                ps,
                remainder: None,
                ..
            })),
            false,
        ) if !ps.is_empty() => ps
            .iter()
            .map(|par| match (sole_expr(par), par.connective_used) {
                (Some(ExprInstance::GString(text)), false) => Some(text.clone()),
                _ => None,
            })
            .collect::<Option<Vec<String>>>()
            .map(|texts| (texts, false)),
        _ => None,
    }
}

/// The reply of `embed` to `input`. The node-local quota bounds the work of the model.
///
/// Changed by decision 105: the reply is the recorded output of play. A bad argument or a text
/// over the quota gives `EXT_BAD_ARG`, and a failure of the embedder gives `EXT_FAILED`. A
/// success takes its bytes from the record room through `native_played_reply`. Before, the reply
/// was one value with a Reified code, and `embed` took the record room.
// async fn embed_reply(input: &Par) -> Par {
async fn embed_reply(input: &Par, cost: &RuntimeBudget) -> Vec<Par> {
    let Some((texts, single)) = texts_of(input) else {
        // return failed_reply(ServiceReplyCode::BadArgument);
        return vec![native_failure_reply(
            EMBED_NAME,
            EXT_BAD_ARG,
            "invalid argument",
        )];
    };
    let bytes: usize = texts.iter().map(String::len).sum();
    if texts.len() > vectordb_services::MAX_TEXTS || bytes > vectordb_services::MAX_TEXT_BYTES {
        // return failed_reply(ServiceReplyCode::EmbeddingRateLimited);
        return vec![native_failure_reply(
            EMBED_NAME,
            EXT_BAD_ARG,
            "invalid argument",
        )];
    }
    let services = vectordb_services::current();
    match embed_texts(&services, &texts).await {
        // Ok(vectors) => {
        Some(vectors) => {
            let value = match (single, vectors.as_slice()) {
                (true, [vector]) => f32_list(vector),
                _ => list_par(vectors.iter().map(|vector| f32_list(vector)).collect()),
            };
            let model = expr_par(ExprInstance::GString(services.embedder.model().to_string()));
            // ok_reply(vec![value, model])
            native_played_reply(EMBED_NAME, vec![ok_reply(vec![value, model])], cost)
        }
        // Err(code) => failed_reply(code),
        None => vec![native_failure_reply(
            EMBED_NAME,
            EXT_FAILED,
            "external service failed",
        )],
    }
}

/// The vectors of `texts` from the embedder of the node: one finite vector for each text.
///
/// Changed by decision 105: a failure gives `None`, because the base code `EXT_FAILED` names
/// every failure of a service. Before, each failure gave its own Reified code.
// async fn embed_texts(
//     services: &VectorDbServices,
//     texts: &[String],
// ) -> Result<Vec<Vec<f32>>, ServiceReplyCode> {
//     match services.embedder.embed(texts).await {
//         Ok(vectors) if vectors.len() != texts.len() => Err(ServiceReplyCode::EmbeddingUnavailable),
//         Ok(vectors) if vectors.iter().flatten().any(|value| !value.is_finite()) => {
//             Err(ServiceReplyCode::NonFiniteEmbedding)
//         }
//         Ok(vectors) => Ok(vectors),
//         Err(EmbedFailure::Unavailable) => Err(ServiceReplyCode::EmbeddingUnavailable),
//         Err(EmbedFailure::NonFinite) => Err(ServiceReplyCode::NonFiniteEmbedding),
//     }
// }
async fn embed_texts(services: &VectorDbServices, texts: &[String]) -> Option<Vec<Vec<f32>>> {
    match services.embedder.embed(texts).await {
        Ok(vectors) if vectors.len() != texts.len() => None,
        Ok(vectors) if vectors.iter().flatten().any(|value| !value.is_finite()) => None,
        Ok(vectors) => Some(vectors),
        Err(EmbedFailure::Unavailable | EmbedFailure::NonFinite) => None,
    }
}

/// `ann!(ret, space, channel, query, options)` (P6 step 7, decision 57). Play asks the mirror of
/// the VectorDB space `space` for the `k` records of `channel` that are nearest to `query`. The
/// ids are hints: only the exact query param `ids=` turns them into a decision (decision 58).
///
/// Changed by decisions 104, 105 and 114 (DR-114): play replies `[true, ids, distances]`, and a
/// success takes its bytes from the record room through `native_played_reply`. A failed check of
/// play gives `EXT_BAD_ARG`, and a failure of a service gives `EXT_FAILED`. Replay makes no check
/// and delivers the record through `native_replayed_reply`. Before, replay made the checks of
/// play too, and the reply ended with the watermark.
async fn ann(
    runtime: &Arc<ReifiedRuntime>,
    agent: &Arc<dyn SpaceAgent>,
    contract: &ContractCall,
    contract_args: ContractArgs,
    cost: &RuntimeBudget,
) -> Result<Vec<Par>, InterpreterError> {
    // Changed by decision 90 (DR-132): the config read takes no cost authority, so only the
    // arity of the call is checked here.
    // let authority = match contract_args.0.as_slice() {
    //     [call] => call.cost_authority.clone(),
    //     _ => return Err(illegal_argument_error(ANN_PROCESS.urn)),
    // };
    let [_] = contract_args.0.as_slice() else {
        return Err(illegal_argument_error(ANN_PROCESS.urn));
    };
    let Some((produce, is_replay, previous_output, args)) = contract.unapply(contract_args) else {
        return Err(illegal_argument_error(ANN_PROCESS.urn));
    };
    let [ret, space, channel, query, options] = args.as_slice() else {
        return Err(illegal_argument_error(ANN_PROCESS.urn));
    };
    // Changed by decision 104 (DR-114): replay delivers the record after the checks of the base
    // driver, and it makes no check of play.
    // record_present(ANN_PROCESS.urn, is_replay, &previous_output)?;
    // let request = ann_request(runtime, agent, authority, space, channel, query, options).await?;
    // if is_replay {
    //     produce(&previous_output, ret).await?;
    //     return Ok(previous_output);
    // }
    // let reply = match request {
    //     Ok(request) => ann_answer(&vectordb_services::current(), request).await,
    //     Err(code) => failed_reply(code),
    // };
    // Changed by decision 89 (DR-114, decision 68): a success of native play takes its bytes
    // from the record room.
    // produce_recorded(produce, vec![reply], ret).await
    // produce_recorded(produce, within_record_room(vec![reply], cost), ret).await
    let reply = match is_replay {
        true => native_replayed_reply(ANN_NAME, previous_output)?,
        false => match ann_request(runtime, agent, space, channel, query, options).await? {
            Some(request) => match ann_answer(&vectordb_services::current(), request).await {
                Some(answer) => native_played_reply(ANN_NAME, vec![answer], cost),
                None => vec![native_failure_reply(
                    ANN_NAME,
                    EXT_FAILED,
                    "external service failed",
                )],
            },
            None => vec![native_failure_reply(
                ANN_NAME,
                EXT_BAD_ARG,
                "invalid argument",
            )],
        },
    };
    produce_recorded(produce, reply, ret).await
}

/// A call of `ann` whose arguments pass every check of consensus state.
struct AnnRequest {
    /// The mirror collection of the space.
    collection: String,
    /// The channel digest of the collection key of `channel`.
    channel: String,
    /// The query.
    query: AnnQuery,
    /// The dimensions of the space.
    dimensions: usize,
    /// The number of neighbours, in `[1, capacity]`.
    k: usize,
}

/// The query of an `ann` call.
enum AnnQuery {
    /// A numeric vector of the dimensions of the space, each element rounded to `f64` and then
    /// to `f32`.
    Vector(Vec<f32>),
    /// A text that the embedder of the node embeds.
    Text(String),
}

/// The checks of an `ann` call in play, in this order. The space is a native VectorDB space with
/// a mirror. The query is a text, or a numeric vector of the dimensions of the space. The options
/// are a map of `k`. A failure of the config job fails the deploy, as for `getSpaceAgent`.
///
/// Changed by decisions 104 and 105: only play makes the checks. Before, replay made them too,
/// and each failed check had its own Reified code.
// Changed by decision 90 (DR-132): the config read takes no cost authority, so the parameter
// `authority: Option<CostAuthority>` after `agent` is gone.
// Changed by decision 105: a failed check gives `None`, and `ann` replies `EXT_BAD_ARG`.
// async fn ann_request(
//     runtime: &Arc<ReifiedRuntime>,
//     agent: &Arc<dyn SpaceAgent>,
//     space: &Par,
//     channel: &Par,
//     query: &Par,
//     options: &Par,
// ) -> Result<Result<AnnRequest, ServiceReplyCode>, InterpreterError> {
async fn ann_request(
    runtime: &Arc<ReifiedRuntime>,
    agent: &Arc<dyn SpaceAgent>,
    space: &Par,
    channel: &Par,
    query: &Par,
    options: &Par,
) -> Result<Option<AnnRequest>, InterpreterError> {
    let native = match classify_space_ref(space) {
        Ok(SpaceRef::Native(native)) => native,
        // Ok(SpaceRef::Agent(_)) | Err(_) => return Ok(Err(ServiceReplyCode::NotAVectorDbSpace)),
        Ok(SpaceRef::Agent(_)) | Err(_) => return Ok(None),
    };
    // let config_map = match read_config(runtime, agent, native, authority).await? {
    let config_map = match read_config(runtime, agent, native).await? {
        Ok(config_map) => config_map,
        // Err(SpaceError::DanglingHandle { .. }) => {
        //     return Ok(Err(ServiceReplyCode::NotAVectorDbSpace))
        // }
        Err(SpaceError::DanglingHandle { .. }) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let config = SpaceConfig::from_par(&config_map).map_err(SpaceError::from)?;
    if family_of(&config)?.inner != SpaceInner::VectorDb {
        // return Ok(Err(ServiceReplyCode::NotAVectorDbSpace));
        return Ok(None);
    }
    if !config.params.contains_key(MIRROR_PARAM) {
        // return Ok(Err(ServiceReplyCode::MirrorDisabled));
        return Ok(None);
    }
    let cfg = vdb_config(&config)?;
    // let query = match query_of(query, cfg.dimensions) {
    //     Ok(query) => query,
    //     Err(code) => return Ok(Err(code)),
    // };
    let Some(query) = query_of(query, cfg.dimensions) else {
        return Ok(None);
    };
    let Some(k) = option_k(options, cfg.capacity) else {
        // return Ok(Err(ServiceReplyCode::InvalidOption));
        return Ok(None);
    };
    // Ok(Ok(AnnRequest {
    Ok(Some(AnnRequest {
        collection: collection_name(&native.sid),
        channel: channel_digest(&CollectionKey::of_channel(channel)?),
        query,
        dimensions: cfg.dimensions,
        k,
    }))
}

/// The query of an `ann` call: a text, or a numeric vector of `dimensions` elements whose
/// values are finite as `f32` values.
///
/// Changed by decision 105: a failure gives `None`. Before, a failure gave `DimensionMismatch` or
/// `BadArgument`.
// fn query_of(query: &Par, dimensions: usize) -> Result<AnnQuery, ServiceReplyCode> {
fn query_of(query: &Par, dimensions: usize) -> Option<AnnQuery> {
    if let (Some(ExprInstance::GString(text)), false) = (sole_expr(query), query.connective_used) {
        // return Ok(AnnQuery::Text(text.clone()));
        return Some(AnnQuery::Text(text.clone()));
    }
    // let vector = match num::vector_of(query, Some(dimensions)) {
    //     Ok(vector) => vector,
    //     Err(NumError::DimensionMismatch { .. }) => return Err(ServiceReplyCode::DimensionMismatch),
    //     Err(_) => return Err(ServiceReplyCode::BadArgument),
    // };
    let Ok(vector) = num::vector_of(query, Some(dimensions)) else {
        return None;
    };
    let values = vector.rationals();
    let mut elements = Vec::with_capacity(values.len());
    for value in values.iter() {
        match num::round_to_f64(value).map(|value| value as f32) {
            Ok(element) if element.is_finite() => elements.push(element),
            // Ok(_) | Err(_) => return Err(ServiceReplyCode::BadArgument),
            Ok(_) | Err(_) => return None,
        }
    }
    // Ok(AnnQuery::Vector(elements))
    Some(AnnQuery::Vector(elements))
}

/// The `k` of the options of `ann`: a map with the one key `k`, an `Int` in `[1, capacity]`.
fn option_k(options: &Par, capacity: usize) -> Option<usize> {
    let entries = map_entries(options)?;
    let k = match (entries.len(), entries.get("k")) {
        (1, Some(k)) => *k,
        _ => return None,
    };
    match (sole_expr(k), k.connective_used) {
        (Some(ExprInstance::GInt(k)), false) => usize::try_from(*k)
            .ok()
            .filter(|k| (1..=capacity).contains(k)),
        _ => None,
    }
}

/// The answer of the services to a checked `ann` call: the embedder embeds a text query, and the
/// mirror answers the `k` neighbours. The reply drops an id that is not 32 bytes long, because it
/// cannot name a record.
///
/// Changed by decisions 105 and 114: a failure gives `None`, and the reply carries no watermark.
/// Before, a failure replied its Reified code, and the mirror also answered the watermark of the
/// collection.
// async fn ann_answer(services: &VectorDbServices, request: AnnRequest) -> Par {
async fn ann_answer(services: &VectorDbServices, request: AnnRequest) -> Option<Par> {
    let vector = match request.query {
        AnnQuery::Vector(vector) => vector,
        AnnQuery::Text(text) => match embed_texts(services, std::slice::from_ref(&text)).await {
            // Ok(vectors) => match <[Vec<f32>; 1]>::try_from(vectors) {
            //     Ok([vector]) if vector.len() == request.dimensions => vector,
            //     Ok(_) => return failed_reply(ServiceReplyCode::DimensionMismatch),
            //     Err(_) => return failed_reply(ServiceReplyCode::EmbeddingUnavailable),
            // },
            // Err(code) => return failed_reply(code),
            Some(vectors) => match <[Vec<f32>; 1]>::try_from(vectors) {
                Ok([vector]) if vector.len() == request.dimensions => vector,
                Ok(_) | Err(_) => return None,
            },
            None => return None,
        },
    };
    let neighbours = match services
        .mirror
        .query(&request.collection, &request.channel, &vector, request.k)
        .await
    {
        Ok(neighbours) => neighbours,
        // Err(_) => return failed_reply(ServiceReplyCode::MirrorUnavailable),
        Err(_) => return None,
    };
    // Retired by decision 114: the reply carries no watermark.
    // let watermark = match services.mirror.watermark(&request.collection).await {
    //     Ok(watermark) => watermark,
    //     Err(_) => return failed_reply(ServiceReplyCode::MirrorUnavailable),
    // };
    let neighbours: Vec<Neighbour> = neighbours
        .into_iter()
        .filter(|neighbour| neighbour.id.len() == ID_LEN)
        .take(request.k)
        .collect();
    let ids = list_par(
        neighbours
            .iter()
            .map(|neighbour| expr_par(ExprInstance::GByteArray(neighbour.id.clone())))
            .collect(),
    );
    let distances = list_par(
        neighbours
            .iter()
            .map(|neighbour| expr_par(ExprInstance::GFloat32(neighbour.distance.to_bits())))
            .collect(),
    );
    // Retired by decision 114: the reply carries no watermark.
    // let watermark = expr_par(ExprInstance::GInt(
    //     i64::try_from(watermark).unwrap_or(i64::MAX),
    // ));
    // ok_reply(vec![ids, distances, watermark])
    Some(ok_reply(vec![ids, distances]))
}

/// Produces the recorded or computed `output` of a recorded process on `ret`. A failure of the
/// produce keeps the output, so the dispatcher records it, as for the other recorded processes:
/// native funded execution keeps the record on the produce that triggered the call and raises the
/// cause (DR-114 of `feature/cost-accounted-rho`, gap G-A).
async fn produce_recorded(
    produce: Producer,
    output: Vec<Par>,
    ret: &Par,
) -> Result<Vec<Par>, InterpreterError> {
    match produce(&output, ret).await {
        Ok(_) => Ok(output),
        Err(error) => Err(InterpreterError::ProduceFailureWithOutput {
            cause: Box::new(error),
            output_not_produced: output.iter().map(|par| par.encode_to_vec()).collect(),
        }),
    }
}

// Retired by decision 105: `native_played_reply` and `native_replayed_reply` of the base take
// their place.
// /// The record room that the reply `output` of native play takes (DR-114, decision 5): the
// /// encoded length of each reply value and 64 bytes. `None` when the sum overflows `u64`.
// fn record_bytes(output: &[Par]) -> Option<u64> {
//     output.iter().try_fold(0_u64, |total, par| {
//         u64::try_from(par.encoded_len())
//             .ok()
//             .and_then(|bytes| bytes.checked_add(64))
//             .and_then(|bytes| total.checked_add(bytes))
//     })
// }
//
// /// The reply that play records for the reply `output` of a call (decision 68). In native funded
// /// execution a success takes its bytes from the record room of the deploy
// /// (`RuntimeBudget::take_native_record_room`), and a success that does not fit becomes the
// /// failure `OutputTooLarge`. A failure takes no room, because its size is small and fixed.
// /// Outside native funded execution the reply stays as it is.
// fn within_record_room(output: Vec<Par>, cost: &RuntimeBudget) -> Vec<Par> {
//     let success = matches!(output.as_slice(), [reply] if is_ok_reply(reply));
//     let fits = match (cost.native_execution_active(), success) {
//         (true, true) => {
//             record_bytes(&output).is_some_and(|bytes| cost.take_native_record_room(bytes))
//         }
//         (true, false) | (false, _) => true,
//     };
//     match fits {
//         true => output,
//         false => vec![failed_reply(ServiceReplyCode::OutputTooLarge)],
//     }
// }
//
// /// Replay of a recorded call needs the record of play, as the driver of DR-114 requires
// /// (`native_recorded_call`). Play records every reply of `embed` and `ann`, and a call whose reply
// /// play could not produce keeps its record too, so an empty record means a block without the
// /// record. Replay then fails with `BugFoundError` before it reads anything.
// fn record_present(
//     urn: &str,
//     is_replay: bool,
//     previous_output: &[Par],
// ) -> Result<(), InterpreterError> {
//     match is_replay && previous_output.is_empty() {
//         true => Err(InterpreterError::BugFoundError(format!(
//             "replay of {urn} has no recorded output"
//         ))),
//         false => Ok(()),
//     }
// }

#[cfg(test)]
pub(crate) mod harness {
    //! The fixtures of the unit tests of the factory, the handle and the system processes.

    use std::collections::HashMap;
    use std::future::Future;
    use std::sync::{Arc, OnceLock};

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
    use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
    use models::rust::space_factory_urn::SpaceFactoryUrn;
    use models::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::native_agent;
    use crate::rust::interpreter::accounting::costs::Cost;
    // Added by decision 90 (C6): the native configuration of the fixtures.
    use crate::rust::interpreter::accounting::native_phlo_rules::{
        native_resource_compatibility_rule, NativeBoundSource, NativeBudgetTraceLimits,
        NativePhloDimension, NativePhloExecutionContract, NativePhloRegionLimits,
    };
    use crate::rust::interpreter::accounting::phlo_controls::{
        check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
    };
    use crate::rust::interpreter::accounting::{NativeRuntimeConfig, RuntimeBudget};
    use crate::rust::interpreter::contract_call::ContractCall;
    // Changed by decision 90 (DR-132): the scheduler is gone.
    // use crate::rust::interpreter::deterministic_reduction::root;
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

    // Changed by decision 90 (C6, decision 69): a host runtime runs space jobs only in native
    // execution, so the fixtures run on a native budget, as the native fixtures of the base do
    // (`accounting/native_runtime/tests/racing_evaluation.rs`).
    // pub(crate) fn budget() -> RuntimeBudget {
    //     RuntimeBudget::new(Cost::create(i64::MAX, "reified system process test"))
    // }
    pub(crate) fn budget() -> RuntimeBudget {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget
            .reset_for_native_execution(native_config())
            .expect("the test budget enters native execution");
        budget
    }

    /// Added by decision 90 (C6): the native configuration `config(u64::MAX, [0, 1, 1, 0])` of
    /// the base fixtures (`accounting/native_runtime/tests.rs`). The base keeps that fixture
    /// private to its module, so this harness restates it.
    // Changed by decision 107: the fixture is `priced_native_config(u64::MAX, 0)`, as the base
    // fixture `config` calls `priced_config(limit, weights, 0)`.
    pub(crate) fn native_config() -> NativeRuntimeConfig { priced_native_config(u64::MAX, 0) }

    /// Added by decision 107: the native configuration `priced_config(limit, [0, 1, 1, 0],
    /// price)` of the base fixtures. The schedule has the actual price `price`, and the signed
    /// price ceiling and the owner ceiling equal it. The resource bound equals the limit. A
    /// positive price needs a limit whose charge `limit * price + 1` fits in `u64`.
    pub(crate) fn priced_native_config(limit: u64, price: u64) -> NativeRuntimeConfig {
        let descriptor = PhloScheduleV1 {
            protocol_version: 6,
            network: b"native-test",
            shard: b"root",
            settlement_asset: b"REV",
            settlement_unit: b"atomic-REV",
            decimal_scale: 8,
            classes: NativePhloDimension::ALL
                .into_iter()
                .zip([0, 1, 1, 0])
                .zip([b"c".as_slice(), b"i", b"t", b"r"])
                .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
                .collect(),
            actual_price: price,
            compatibility_rule: native_resource_compatibility_rule(),
        };
        let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS)
            .expect("the test schedule binds");
        let selected = binding.schedule();
        let schedules = [selected];
        let owner_ceilings = [price];
        let controls = check_phlo_controls(
            selected.environment,
            0,
            u64::MAX,
            SignedPhloControls {
                limit,
                price_ceiling: price,
                required_owner_ceilings: &owner_ceilings,
                permitted_schedules: &schedules,
            },
            selected,
            limit,
        )
        .expect("the test controls check");
        let contract =
            NativePhloExecutionContract::new(controls, &binding, NativeBoundSource::Certificate)
                .expect("the test contract builds");
        NativeRuntimeConfig::new(contract, NativeBudgetTraceLimits {
            attempts: 100_000,
            path_segments: 4096,
            regions: NativePhloRegionLimits {
                regions: 1000,
                encoded_authority_bytes: 10_000_000,
            },
        })
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
        // Disabled by decision 90 (DR-132): only the scheduler root of `reduce` read the budget.
        // pub(crate) budget: RuntimeBudget,
    }

    impl Reduction {
        pub(crate) async fn new() -> Self {
            let rspace = rspace().await;
            let budget = budget();
            let runtime = for_host(rspace.clone(), budget.clone());
            let mut processes = super::definitions(&runtime);
            let space_runtime = crate::rust::interpreter::reified::runtime::for_host(
                rspace.clone(),
                budget.clone(),
            );
            let (reducer, _, _, _) = create_rho_env(
                rspace.clone(),
                space_runtime,
                Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                Arc::new(HashMap::new()),
                &mut processes,
                budget.clone(),
                ExternalServices::noop(),
                // Added by decision 90: the file handles that the base passes to every
                // environment.
                crate::rust::interpreter::io::handle_table::FileHandleTable::new(),
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
                // budget,
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
        ///
        /// Changed by decision 90 (DR-132): the future runs in the per-call dispatch scope of
        /// the base, as `inj_with_observation` runs a deploy, and the scope drains before the
        /// result returns. Before, it ran under the scheduler root, which dropped the errors
        /// of the detached tasks as this form does.
        pub(crate) async fn reduce<T>(&self, future: impl Future<Output = T>) -> T {
            self.runtime.begin_evaluation_scope();
            self.reducer.core.reified.begin_evaluation_scope();
            // root(
            //     self.reducer.core.space.clone(),
            //     self.budget.clone(),
            //     self.reducer.core.reduction_coordinator.clone(),
            //     future,
            // )
            // .await
            crate::rust::interpreter::reduce::with_dispatch_scope(future)
                .await
                .0
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
    use crate::rust::interpreter::reified::system_channels::{
        P5_SYSTEM_CHANNELS, P6_SYSTEM_CHANNELS, P7_SYSTEM_CHANNELS,
    };
    use crate::rust::interpreter::spaces::namespace::{self, NameTail, GET_SPACE_AGENT, SINK_OP};

    fn typed(code: SpaceReplyCode) -> Par { typed_error(code, Vec::new()) }

    // Retired by decision 105: the base helpers `native_played_reply` and `native_replayed_reply`
    // replace `record_present`, `record_bytes` and `within_record_room`, and
    // `vectordb_replay_delivers_the_record_without_a_check` covers the replay checks.
    // /// Decision 68 (DR-114): replay of `embed` or `ann` without the record of play fails with
    // /// `BugFoundError`. Play, and replay with a record, pass the check. The success predicate of
    // /// the record room tells an `ok_reply` from a `failed_reply`.
    // #[test]
    // fn recorded_processes_need_their_record_in_replay() {
    //     let reply = failed_reply(ServiceReplyCode::BadArgument);
    //     for urn in [EMBED_PROCESS.urn, ANN_PROCESS.urn] {
    //         assert_eq!(
    //             record_present(urn, true, &[]),
    //             Err(InterpreterError::BugFoundError(format!(
    //                 "replay of {urn} has no recorded output"
    //             )))
    //         );
    //         assert_eq!(
    //             record_present(urn, true, std::slice::from_ref(&reply)),
    //             Ok(())
    //         );
    //         assert_eq!(record_present(urn, false, &[]), Ok(()));
    //     }
    //     assert!(is_ok_reply(&ok_reply(vec![int(1)])));
    //     assert!(!is_ok_reply(&reply));
    //     assert!(!is_ok_reply(&int(1)));
    //     assert_eq!(record_bytes(&[]), Some(0));
    //     assert_eq!(
    //         record_bytes(std::slice::from_ref(&reply)),
    //         Some(u64::try_from(reply.encoded_len()).expect("a small reply") + 64)
    //     );
    // }

    /// Added by decisions 104 and 116 (DR-114): replay of `embed` and `ann` delivers the record of
    /// play through `native_replayed_reply`, and it makes no check of play. Play of `ann` reads the
    /// config of the space before it reads the options, so it fails on a runtime without a host.
    /// Replay of the same call delivers its record. An empty record and a record that is not plain
    /// data fail with `BugFoundError`, and nothing lands on the reply channel.
    #[cfg(feature = "chromadb")]
    #[tokio::test]
    async fn vectordb_replay_delivers_the_record_without_a_check() {
        use super::harness::budget;
        use crate::rust::interpreter::accounting::owner_funding::plain_data_tests::review_forgeries;

        let fixture = Fixture::with_runtime(unavailable()).await;
        let cost = budget();
        let embed = ReifiedProcess::Embed;
        let ann = ReifiedProcess::Ann {
            runtime: Arc::clone(&fixture.runtime),
            agent: Arc::clone(&fixture.agent),
        };
        let handle = namespace::gprivate(namespace::handle_channel(space_of(b"no host")));
        let ann_data = |ret: Par| {
            vec![
                ret,
                handle.clone(),
                string("records"),
                string("text"),
                int(1),
            ]
        };
        let ret = reply_name(1);
        match ann
            .call(
                &fixture.contract,
                call(ann_data(ret.clone()), b"ann play"),
                &cost,
            )
            .await
        {
            Err(InterpreterError::ReduceError(message)) => {
                assert!(
                    message.starts_with("reified:RuntimeUnavailable: "),
                    "{message}"
                );
            }
            other => panic!("play of ann without a host returned {other:?}"),
        }
        assert!(fixture.data(&ret).await.is_empty());
        let record = vec![ok_reply(vec![int(1)])];
        let cases = [
            (&embed, EMBED_NAME, vec![reply_name(2), string("text")]),
            (&ann, ANN_NAME, ann_data(reply_name(3))),
        ];
        for (process, name, data) in cases {
            let ret = data[0].clone();
            let replay = |record: Vec<Par>| {
                let (pars, _, _) = call(data.clone(), b"replay");
                (pars, true, record)
            };
            let reply = process
                .call(&fixture.contract, replay(record.clone()), &cost)
                .await
                .expect("replay delivers the record");
            assert_eq!(reply, record, "{name}");
            assert_eq!(fixture.data(&ret).await, vec![record.clone()], "{name}");
            assert_eq!(
                process
                    .call(&fixture.contract, replay(Vec::new()), &cost)
                    .await,
                Err(InterpreterError::BugFoundError(format!(
                    "native replay of {name} has no recorded output"
                ))),
                "{name}"
            );
            for (label, forged) in review_forgeries() {
                assert_eq!(
                    process
                        .call(&fixture.contract, replay(vec![forged]), &cost)
                        .await,
                    Err(InterpreterError::BugFoundError(format!(
                        "native replay of {name} has a recorded output that is not plain data"
                    ))),
                    "{name}: {label}"
                );
            }
            assert_eq!(fixture.data(&ret).await, vec![record.clone()], "{name}");
        }
    }

    fn send(channel: Par, data: Vec<Par>) -> Par {
        Par::default().with_sends(vec![SendProcess {
            chan: Some(channel),
            data,
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
            params: None,
        }])
    }

    #[test]
    fn space_system_ops_arity_and_body_refs_stable() {
        let runtime = unavailable();
        let defined = definitions(&runtime);
        // The four processes of P4 first, then the registered families of P5 in family-index
        // order, then the registered aliases, then the registered families of P6, then the
        // processes of P6, then the registered families of P7, then the processes of P8.
        let p5: Vec<SystemChannel> = urn::registered_p5_families()
            .map(|(_, channel)| channel)
            .chain(urn::registered_aliases().map(|(channel, _)| channel))
            .collect();
        let p6: Vec<SystemChannel> = urn::registered_p6_families()
            .map(|(_, channel)| channel)
            .collect();
        assert_eq!(
            p6, P6_SYSTEM_CHANNELS,
            "both VectorDB families are registered"
        );
        let p7: Vec<SystemChannel> = urn::registered_p7_families()
            .map(|(_, channel)| channel)
            .collect();
        assert_eq!(
            p7, P7_SYSTEM_CHANNELS,
            "the six HashSet families are registered"
        );
        let expected: Vec<SystemChannel> = P4_SYSTEM_CHANNELS
            .iter()
            .chain(p5.iter())
            .chain(p6.iter())
            .chain(P6_PROCESSES.iter())
            .chain(p7.iter())
            .chain(P8_PROCESSES.iter())
            .copied()
            .collect();
        assert_eq!(defined.len(), expected.len());
        for (definition, channel) in defined.iter().zip(&expected) {
            assert_eq!(definition.urn, channel.urn);
            assert_eq!(definition.fixed_channel, channel.channel());
            assert_eq!(definition.arity, channel.arity);
            assert_eq!(definition.body_ref, channel.body_ref());
            // Changed by P8 step 1: the sink has a wildcard remainder (design v2 TS-2).
            // assert_eq!(definition.remainder, None);
            let expected_remainder = match channel.op {
                SINK_OP => Some(new_wildcard_var()),
                _ => None,
            };
            assert_eq!(definition.remainder, expected_remainder);
            assert_eq!(
                definition.to_urn_map(),
                (
                    channel.urn.to_string(),
                    new_bundle_par(channel.channel(), true, false)
                )
            );
            // P8 step 2 (design v2 TS-2): the return channel of a call is the URN value of
            // the sink.
            if channel.op == SINK_OP {
                assert_eq!(
                    definition.to_urn_map().1,
                    super::super::user_agent::sink_channel()
                );
            }
        }
        let rows: Vec<SystemChannel> = P5_SYSTEM_CHANNELS
            .iter()
            .filter(|channel| p5.contains(channel))
            .copied()
            .collect();
        assert_eq!(
            rows, p5,
            "the P5 definitions keep the order of the P5 table"
        );
        assert_eq!(
            Some(defined[0].to_urn_map().1),
            factory_capability(SpaceFactoryUrn::HASHMAP_QUEUE)
        );
        let mut processes = definitions(&runtime);
        register(&mut processes, &runtime);
        assert_eq!(processes.len(), 2 * expected.len());
        assert!(processes
            .iter()
            .zip(expected.iter().chain(expected.iter()))
            .all(|(definition, channel)| definition.body_ref == channel.body_ref()));
    }

    #[test]
    fn space_system_ops_alias_definition_is_the_definition_of_its_family() {
        let runtime = unavailable();
        let factory = |family: SpaceFactoryUrn, channel: SystemChannel| {
            definition(
                channel,
                ReifiedProcess::Factory(FactoryProcess::new(
                    family,
                    Arc::clone(&runtime),
                    native_agent(),
                )),
            )
        };
        for (alias, family) in urn::P5_ALIAS_ROWS {
            let (_, channel) = urn::P5_FAMILY_ROWS
                .into_iter()
                .find(|(candidate, _)| *candidate == family)
                .expect("the family of an alias has a row");
            let of_alias = factory(family, alias);
            let of_family = factory(family, channel);
            assert_eq!(of_alias.urn, alias.urn);
            assert_ne!(of_alias.urn, of_family.urn);
            assert_eq!(of_alias.fixed_channel, of_family.fixed_channel);
            assert_eq!(of_alias.arity, of_family.arity);
            assert_eq!(of_alias.body_ref, of_family.body_ref);
            assert_eq!(of_alias.remainder, None);
            assert_eq!(of_alias.to_urn_map().1, of_family.to_urn_map().1);
            assert_eq!(Some(of_alias.to_urn_map().1), factory_capability(family));
        }
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
        assert_eq!(reply, vec![dangling_reply(space_of(b"never created").sid)]);
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
            (native_replay(), "RuntimeUnavailable"),
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
