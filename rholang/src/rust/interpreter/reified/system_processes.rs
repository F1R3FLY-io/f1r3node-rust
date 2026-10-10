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
//! arguments after `ret` (I-18). `tensor::ops` computes the value with the host-work budget of
//! the deploy, and the process replies `[true, value]` or `[false, code, detail]` on `ret`
//! (decision 57). A host-work rejection fails the deploy.
//!
//! `embed` and `ann` are recorded processes (decision 57): the dispatcher records the reply of a
//! call on its produce event (`non_deterministic_ops`), and replay produces the recorded reply.
//! In play they ask the services of the node (`vectordb_services`). Every failure is a reply
//! `[false, code, detail]`, so it is recorded too. The checks of an `ann` call read only
//! consensus state, so play and replay both make them and reserve the same host work. Both
//! processes run in legacy and in native execution, and native replay produces the recorded
//! replies as legacy replay does.

use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{CostAuthority, EList, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::space_factory_urn::SpaceInner;
use models::rust::utils::new_gbool_par;
use prost::Message;

use super::costs::ScanMeter;
use super::factory::{ContractArgs, FactoryProcess};
use super::handle::{typed_error, HandleProcess};
use super::runtime::ReifiedRuntime;
use super::scope::{classify_space_ref, SpaceRef};
use super::system_channels::{
    SystemChannel, ANN_PROCESS, ASSERT_EQUAL_PROCESS, EMBED_PROCESS, GET_SPACE_AGENT_PROCESS,
    HANDLE_OP_PROCESS, P4_SYSTEM_CHANNELS, P6_PROCESSES, VECTOR_PROCESS,
};
use super::vectordb_services::{
    self, channel_digest, collection_name, f32_list, failed_reply, list_par, ok_reply,
    EmbedFailure, Neighbour, ServiceReplyCode, VectorDbServices,
};
use crate::rust::interpreter::contract_call::{ContractCall, Producer};
use crate::rust::interpreter::deterministic_reduction;
use crate::rust::interpreter::deterministic_reduction::reified_schedule::{
    submit_space, SpaceRequest,
};
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
use crate::rust::interpreter::system_processes::{Definition, ProcessContext};
use crate::rust::interpreter::tensor::num::{self, NumError};
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
/// map and the dispatch table are keyed by the channel and by the body ref.
pub fn definitions(runtime: &Arc<ReifiedRuntime>) -> Vec<Definition> {
    let agent = native_agent();
    let families: Vec<_> = urn::registered_p5_families().collect();
    let aliases: Vec<_> = urn::registered_aliases().collect();
    let p6_families: Vec<_> = urn::registered_p6_families().collect();
    let p7_families: Vec<_> = urn::registered_p7_families().collect();
    let mut definitions = Vec::with_capacity(
        P4_SYSTEM_CHANNELS.len()
            + families.len()
            + aliases.len()
            + p6_families.len()
            + P6_PROCESSES.len()
            + p7_families.len(),
    );
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
    definitions.push(definition(EMBED_PROCESS, ReifiedProcess::Embed));
    definitions.push(definition(ANN_PROCESS, ReifiedProcess::Ann {
        runtime: Arc::clone(runtime),
        agent: Arc::clone(&agent),
    }));
    for (family, channel) in p7_families {
        definitions.push(factory(family, channel));
    }
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
    Vector,
    Embed,
    Ann {
        runtime: Arc<ReifiedRuntime>,
        agent: Arc<dyn SpaceAgent>,
    },
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
            ReifiedProcess::Vector => vector(contract, contract_args).await,
            ReifiedProcess::Embed => embed(contract, contract_args).await,
            ReifiedProcess::Ann { runtime, agent } => {
                ann(runtime, agent, contract, contract_args).await
            }
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
        // DG-1: an ephemeral space that an ended deploy created gets the reply before any job.
        Ok(SpaceRef::Native(space)) if runtime.check_live_space(space).is_err() => {
            dangling_reply(space.sid)
        }
        Ok(SpaceRef::Native(space)) => match read_config(runtime, agent, space, authority).await? {
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

/// `vec!(ret, op, x…)`, packed as `[ret, [op, x…]]` (P6 step 6, design v3 4.7). The work is
/// reserved on the host-work budget of the deploy (decision 42), which the reduction context
/// holds, as `submit_space` reads it. Without a budget the meter accepts every reservation.
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
    let meter = ScanMeter::new(
        deterministic_reduction::current().and_then(|context| context.host_work_budget()),
    );
    let reply = match ops::reply(ops::evaluate(call, &meter), &meter) {
        Ok(reply) => reply,
        Err(OpFailure::HostWork(error)) => return Err(error.into()),
        Err(OpFailure::Inconsistent(reason)) => return Err(SpaceError::Inconsistent(reason).into()),
        Err(OpFailure::Reply(_)) => {
            return Err(SpaceError::Inconsistent("a reply failure became a reply").into())
        }
    };
    let reply = vec![reply];
    produce(&reply, ret).await?;
    Ok(reply)
}

/// `embed!(ret, text)` or `embed!(ret, [text…])` (P6 step 7, decision 57). Play embeds the
/// texts with the embedder of the node and replies `[true, vector, model]` for one text or
/// `[true, [vector…], model]` for a list, with each vector a list of `GFloat32`, or
/// `[false, code, detail]`. Replay produces the recorded reply.
async fn embed(
    contract: &ContractCall,
    contract_args: ContractArgs,
) -> Result<Vec<Par>, InterpreterError> {
    let Some((produce, is_replay, previous_output, args)) = contract.unapply(contract_args) else {
        return Err(illegal_argument_error(EMBED_PROCESS.urn));
    };
    let [ret, input] = args.as_slice() else {
        return Err(illegal_argument_error(EMBED_PROCESS.urn));
    };
    if is_replay {
        produce(&previous_output, ret).await?;
        return Ok(previous_output);
    }
    let output = vec![embed_reply(input).await];
    produce_recorded(produce, output, ret).await
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
async fn embed_reply(input: &Par) -> Par {
    let Some((texts, single)) = texts_of(input) else {
        return failed_reply(ServiceReplyCode::BadArgument);
    };
    let bytes: usize = texts.iter().map(String::len).sum();
    if texts.len() > vectordb_services::MAX_TEXTS || bytes > vectordb_services::MAX_TEXT_BYTES {
        return failed_reply(ServiceReplyCode::EmbeddingRateLimited);
    }
    let services = vectordb_services::current();
    match embed_texts(&services, &texts).await {
        Ok(vectors) => {
            let value = match (single, vectors.as_slice()) {
                (true, [vector]) => f32_list(vector),
                _ => list_par(vectors.iter().map(|vector| f32_list(vector)).collect()),
            };
            let model = expr_par(ExprInstance::GString(services.embedder.model().to_string()));
            ok_reply(vec![value, model])
        }
        Err(code) => failed_reply(code),
    }
}

/// The vectors of `texts` from the embedder of the node: one finite vector for each text.
async fn embed_texts(
    services: &VectorDbServices,
    texts: &[String],
) -> Result<Vec<Vec<f32>>, ServiceReplyCode> {
    match services.embedder.embed(texts).await {
        Ok(vectors) if vectors.len() != texts.len() => Err(ServiceReplyCode::EmbeddingUnavailable),
        Ok(vectors) if vectors.iter().flatten().any(|value| !value.is_finite()) => {
            Err(ServiceReplyCode::NonFiniteEmbedding)
        }
        Ok(vectors) => Ok(vectors),
        Err(EmbedFailure::Unavailable) => Err(ServiceReplyCode::EmbeddingUnavailable),
        Err(EmbedFailure::NonFinite) => Err(ServiceReplyCode::NonFiniteEmbedding),
    }
}

/// `ann!(ret, space, channel, query, options)` (P6 step 7, decision 57). Play asks the mirror of
/// the VectorDB space `space` for the `k` records of `channel` that are nearest to `query`, and
/// replies `[true, ids, distances, watermark]` or `[false, code, detail]`. The ids are hints: only
/// the exact query param `ids=` turns them into a decision (decision 58). Replay produces the
/// recorded reply.
///
/// The checks of the call read consensus state only, so play and replay both make them: the
/// config read and the decode of a numeric query reserve host work on the budget of the deploy,
/// and both modes must reserve the same amounts. Only the answers of the services are recorded.
async fn ann(
    runtime: &Arc<ReifiedRuntime>,
    agent: &Arc<dyn SpaceAgent>,
    contract: &ContractCall,
    contract_args: ContractArgs,
) -> Result<Vec<Par>, InterpreterError> {
    let authority = match contract_args.0.as_slice() {
        [call] => call.cost_authority.clone(),
        _ => return Err(illegal_argument_error(ANN_PROCESS.urn)),
    };
    let Some((produce, is_replay, previous_output, args)) = contract.unapply(contract_args) else {
        return Err(illegal_argument_error(ANN_PROCESS.urn));
    };
    let [ret, space, channel, query, options] = args.as_slice() else {
        return Err(illegal_argument_error(ANN_PROCESS.urn));
    };
    let request = ann_request(runtime, agent, authority, space, channel, query, options).await?;
    if is_replay {
        produce(&previous_output, ret).await?;
        return Ok(previous_output);
    }
    let reply = match request {
        Ok(request) => ann_answer(&vectordb_services::current(), request).await,
        Err(code) => failed_reply(code),
    };
    produce_recorded(produce, vec![reply], ret).await
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

/// The checks of an `ann` call, in this order: the space is a native VectorDB space
/// (`NotAVectorDbSpace`) with a mirror (`MirrorDisabled`), the query is a text or a numeric
/// vector of the dimensions of the space (`DimensionMismatch`, `BadArgument`), and the options
/// are a map of `k` (`InvalidOption`). A failure of the config job fails the deploy, as for
/// `getSpaceAgent`, and so does a host-work rejection.
async fn ann_request(
    runtime: &Arc<ReifiedRuntime>,
    agent: &Arc<dyn SpaceAgent>,
    authority: Option<CostAuthority>,
    space: &Par,
    channel: &Par,
    query: &Par,
    options: &Par,
) -> Result<Result<AnnRequest, ServiceReplyCode>, InterpreterError> {
    let native = match classify_space_ref(space) {
        Ok(SpaceRef::Native(native)) => native,
        Ok(SpaceRef::Agent(_)) | Err(_) => return Ok(Err(ServiceReplyCode::NotAVectorDbSpace)),
    };
    let config_map = match read_config(runtime, agent, native, authority).await? {
        Ok(config_map) => config_map,
        Err(SpaceError::DanglingHandle { .. }) => {
            return Ok(Err(ServiceReplyCode::NotAVectorDbSpace))
        }
        Err(error) => return Err(error.into()),
    };
    let config = SpaceConfig::from_par(&config_map).map_err(SpaceError::from)?;
    if family_of(&config)?.inner != SpaceInner::VectorDb {
        return Ok(Err(ServiceReplyCode::NotAVectorDbSpace));
    }
    if !config.params.contains_key(MIRROR_PARAM) {
        return Ok(Err(ServiceReplyCode::MirrorDisabled));
    }
    let meter = ScanMeter::new(
        deterministic_reduction::current().and_then(|context| context.host_work_budget()),
    );
    let cfg = vdb_config(&config, &meter)?;
    let query = match query_of(query, cfg.dimensions, &meter)? {
        Ok(query) => query,
        Err(code) => return Ok(Err(code)),
    };
    let Some(k) = option_k(options, cfg.capacity) else {
        return Ok(Err(ServiceReplyCode::InvalidOption));
    };
    Ok(Ok(AnnRequest {
        collection: collection_name(&native.sid),
        channel: channel_digest(&CollectionKey::of_channel(channel)?),
        query,
        dimensions: cfg.dimensions,
        k,
    }))
}

/// The query of an `ann` call: a text, or a numeric vector of `dimensions` elements whose
/// values are finite as `f32` values. A host-work rejection fails the deploy.
fn query_of(
    query: &Par,
    dimensions: usize,
    meter: &ScanMeter,
) -> Result<Result<AnnQuery, ServiceReplyCode>, InterpreterError> {
    if let (Some(ExprInstance::GString(text)), false) = (sole_expr(query), query.connective_used) {
        return Ok(Ok(AnnQuery::Text(text.clone())));
    }
    let vector = match num::vector_of(query, Some(dimensions), meter) {
        Ok(vector) => vector,
        Err(NumError::HostWork(error)) => return Err(error.into()),
        Err(NumError::DimensionMismatch { .. }) => {
            return Ok(Err(ServiceReplyCode::DimensionMismatch))
        }
        Err(_) => return Ok(Err(ServiceReplyCode::BadArgument)),
    };
    let values = match vector.rationals(meter) {
        Ok(values) => values,
        Err(NumError::HostWork(error)) => return Err(error.into()),
        Err(_) => return Ok(Err(ServiceReplyCode::BadArgument)),
    };
    let mut elements = Vec::with_capacity(values.len());
    for value in values.iter() {
        match num::round_to_f64(value, meter).map(|value| value as f32) {
            Ok(element) if element.is_finite() => elements.push(element),
            Ok(_) => return Ok(Err(ServiceReplyCode::BadArgument)),
            Err(NumError::HostWork(error)) => return Err(error.into()),
            Err(_) => return Ok(Err(ServiceReplyCode::BadArgument)),
        }
    }
    Ok(Ok(AnnQuery::Vector(elements)))
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
/// mirror answers the `k` neighbours and the watermark of the collection. A failure replies its
/// code, and the reply drops an id that is not 32 bytes long, because it cannot name a record.
async fn ann_answer(services: &VectorDbServices, request: AnnRequest) -> Par {
    let vector = match request.query {
        AnnQuery::Vector(vector) => vector,
        AnnQuery::Text(text) => match embed_texts(services, std::slice::from_ref(&text)).await {
            Ok(vectors) => match <[Vec<f32>; 1]>::try_from(vectors) {
                Ok([vector]) if vector.len() == request.dimensions => vector,
                Ok(_) => return failed_reply(ServiceReplyCode::DimensionMismatch),
                Err(_) => return failed_reply(ServiceReplyCode::EmbeddingUnavailable),
            },
            Err(code) => return failed_reply(code),
        },
    };
    let neighbours = match services
        .mirror
        .query(&request.collection, &request.channel, &vector, request.k)
        .await
    {
        Ok(neighbours) => neighbours,
        Err(_) => return failed_reply(ServiceReplyCode::MirrorUnavailable),
    };
    let watermark = match services.mirror.watermark(&request.collection).await {
        Ok(watermark) => watermark,
        Err(_) => return failed_reply(ServiceReplyCode::MirrorUnavailable),
    };
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
    let watermark = expr_par(ExprInstance::GInt(
        i64::try_from(watermark).unwrap_or(i64::MAX),
    ));
    ok_reply(vec![ids, distances, watermark])
}

/// Produces the recorded or computed `output` of a recorded process on `ret`. A failure of the
/// produce keeps the output, so the dispatcher records it on the failed produce event, as for the
/// other recorded processes.
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
    use crate::rust::interpreter::reified::system_channels::{
        P5_SYSTEM_CHANNELS, P6_SYSTEM_CHANNELS, P7_SYSTEM_CHANNELS,
    };
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
        // The four processes of P4 first, then the registered families of P5 in family-index
        // order, then the registered aliases, then the registered families of P6, then the
        // processes of P6, then the registered families of P7.
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
            .copied()
            .collect();
        assert_eq!(defined.len(), expected.len());
        for (definition, channel) in defined.iter().zip(&expected) {
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

    /// `rho:lang:vector` reserves its work on the host-work budget of the deploy, which the
    /// reduction context holds (P6 step 6). One unit less of host work rejects the call with
    /// `HostWorkRejected`, and the call sends no reply.
    #[tokio::test]
    async fn space_system_ops_vector_ops_reserve_on_the_deploy_budget() {
        use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};

        use crate::rust::interpreter::deterministic_reduction::{
            root_with_host_work, ReductionCoordinator,
        };
        use crate::rust::interpreter::host_work::HostWorkBudget;
        use crate::rust::interpreter::rho_runtime::RhoISpace;

        let fixture = Fixture::new().await;
        let session: RhoISpace = Arc::new(Box::new(fixture.rspace.clone()));
        let dot = list(vec![
            string("dot"),
            list(vec![int(1), int(2)]),
            list(vec![int(3), int(4)]),
        ]);
        let host_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        let ret = reply_name(1);
        let reply = root_with_host_work(
            session.clone(),
            super::harness::budget(),
            ReductionCoordinator::default(),
            Some(host_work.clone()),
            vector(
                &fixture.contract,
                call(vec![ret.clone(), dot.clone()], b"vector"),
            ),
        )
        .await
        .expect("the vector process replies");
        assert_eq!(reply, vec![list(vec![
            new_gbool_par(true, Vec::new(), false),
            int(11)
        ])]);
        assert_eq!(fixture.data(&ret).await, vec![reply]);
        let used = host_work
            .usage(HostWorkDimension::VerificationOperations)
            .get();
        assert!(used > 0, "the process reserves on the budget of the deploy");
        let tight = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(used - 1)));
        let rejected = root_with_host_work(
            session,
            super::harness::budget(),
            ReductionCoordinator::default(),
            Some(tight),
            vector(&fixture.contract, call(vec![reply_name(2), dot], b"vector")),
        )
        .await;
        assert!(
            matches!(rejected, Err(InterpreterError::HostWorkRejected)),
            "{rejected:?}"
        );
        assert!(
            fixture.data(&reply_name(2)).await.is_empty(),
            "a rejected call sends no reply"
        );
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
