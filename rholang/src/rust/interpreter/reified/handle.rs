//! The handle process `HANDLE_OP` (P4 design C.8, body ref 248, A6, A22 and A23).
//!
//! The handle continuation of a space forwards each call `this!(ret, method, a…; k=v…)` as
//! `[0xE0, 0x30]!(this, [ret, method, a…, (PARAMS)?])`. The process takes these steps:
//!
//! 1. It accepts the call only when `this` is a bare handle id `E0‖H‖sid` or `E0‖h‖sid` of 34
//!    bytes (A6). It drops every other call.
//! 2. It reads `C(sid)` in a read-only job. A missing config record aborts with
//!    `DanglingHandle`.
//! 3. It drops the message unless the message is a list `[ret, method, …]` whose `ret` is a
//!    name and whose `method` is a string (A23). A name is one unforgeable, or one bundle with
//!    the write flag.
//! 4. It runs the method, or it replies a typed error `("rho:space:error", code, detail)` on
//!    `ret`. The detail map holds the method and the facts of the error.
//!
//! | Method      | Arguments                | Params       | Reply                          |
//! |-------------|--------------------------|--------------|--------------------------------|
//! | `gensym`    | none                     | none         | a fresh name `N(sid‖tail)`     |
//! | `qualifier` | none                     | none         | the qualifier of the config    |
//! | `config`    | none                     | none         | the config map                 |
//! | `produce`   | `ch`, `msg`              | `persistent` | `Nil`, after the operation     |
//! | `consume`   | `pairs`, `arrow`, `body` | none         | none: P8 runs it (abort)       |
//!
//! The checks of a method run in this order: positional params (abort, as the send hook does),
//! the method name (`UnknownMethod`), the argument count (`BadArgument`), the named params
//! (`UnknownParam`, keys in bytewise order), and then the arguments. `gensym`, `qualifier` and
//! `config` stage nothing and commit nothing. `produce` runs one collection operation with
//! the random state and the cost authority of the call (A16). Its fires, its re-issue and its
//! reply `Nil` are the branches of the outcome, and the reply is the last branch: a dispatch
//! ends the reduction participant that makes it, so the participant cannot send the reply after
//! a fire. `consume` accepts only matchers of user agents, so in P4 it ends with a typed error
//! or with the abort `AgentSpacesUnavailable` (A9).

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Bundle, EList, ETuple, Expr, ListParWithRandom, Par};
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::reified::encoding::{split_trailing_params, SendParams};
use models::rust::rholang::implicits::single_bundle;
use models::rust::utils::{new_gint_par, union};

use super::errors::ReifiedReduceError;
use super::factory::ContractArgs;
use super::runtime::ReifiedRuntime;
use super::system_channels::{handle_of, HANDLE_OP_PROCESS};
use crate::rust::interpreter::contract_call::ContractCall;
use crate::rust::interpreter::dispatch::{DispatchType, RhoDispatch};
use crate::rust::interpreter::errors::{illegal_argument_error, InterpreterError};
use crate::rust::interpreter::reduce::ReducerCore;
use crate::rust::interpreter::spaces::agent::{ProduceOp, SpaceAgent, SpaceOp};
use crate::rust::interpreter::spaces::errors::{SpaceError, SpaceReplyCode, SPACE_ERROR_REPLY_TAG};
use crate::rust::interpreter::spaces::factory::constructor::family_of;
use crate::rust::interpreter::spaces::factory::defaults::{send_schema, suggestion};
use crate::rust::interpreter::spaces::namespace::{self, CollectionKey, NativeSpace, ReifiedId};
use crate::rust::interpreter::spaces::types::config::SpaceConfig;

/// The methods of a handle, in bytewise order.
pub const HANDLE_METHODS: [&str; 5] = ["config", "consume", "gensym", "produce", "qualifier"];

/// The named param of the method `produce` that makes the datum persistent.
pub const PERSISTENT_PARAM: &str = "persistent";

/// The arrows of the method `consume`: `<-`, `<=` (persistent) and `<<-` (peek).
pub const CONSUME_ARROWS: [&str; 3] = ["<-", "<=", "<<-"];

/// A method of a handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandleMethod {
    Gensym,
    Qualifier,
    Config,
    Produce,
    Consume,
}

impl HandleMethod {
    /// The method with the name `name`.
    pub fn parse(name: &str) -> Option<HandleMethod> {
        match name {
            "gensym" => Some(HandleMethod::Gensym),
            "qualifier" => Some(HandleMethod::Qualifier),
            "config" => Some(HandleMethod::Config),
            "produce" => Some(HandleMethod::Produce),
            "consume" => Some(HandleMethod::Consume),
            _ => None,
        }
    }

    /// The number of arguments after `ret` and the method name.
    pub const fn arity(self) -> usize {
        match self {
            HandleMethod::Gensym | HandleMethod::Qualifier | HandleMethod::Config => 0,
            HandleMethod::Produce => 2,
            HandleMethod::Consume => 3,
        }
    }
}

/// A message that the handle process accepts: `[ret, method, a…, (PARAMS)?]`.
#[derive(Clone, Debug, PartialEq)]
pub struct HandleMessage {
    /// The reply name.
    pub ret: Par,
    /// The method name.
    pub method: String,
    /// The arguments after the method name, without the PARAMS tuple.
    pub args: Vec<Par>,
    /// The params of the call.
    pub params: SendParams,
}

/// The result of reading a message.
#[derive(Clone, Debug, PartialEq)]
pub enum ParsedMessage {
    /// The message has no reply name or no method name. The process drops it (A23).
    Dropped,
    /// The message has a reply name and a method name, but its PARAMS tuple is malformed.
    Malformed { ret: Par, method: String },
    /// A message that the process runs.
    Message(HandleMessage),
}

impl ParsedMessage {
    /// Reads the message list of a call.
    pub fn parse(message: &Par) -> ParsedMessage {
        let Some(elements) = sole_list(message) else {
            return ParsedMessage::Dropped;
        };
        let [ret, method, rest @ ..] = elements else {
            return ParsedMessage::Dropped;
        };
        let Some(method) = sole_string(method) else {
            return ParsedMessage::Dropped;
        };
        if !is_reply_name(ret) {
            return ParsedMessage::Dropped;
        }
        let (args, params) = match split_trailing_params(rest) {
            Ok(None) => (rest, SendParams::default()),
            Ok(Some(params)) => (rest.split_last().map_or(rest, |(_, init)| init), params),
            Err(_) => {
                return ParsedMessage::Malformed {
                    ret: ret.clone(),
                    method: method.to_string(),
                }
            }
        };
        ParsedMessage::Message(HandleMessage {
            ret: ret.clone(),
            method: method.to_string(),
            args: args.to_vec(),
            params,
        })
    }
}

/// What a method leaves to do after its checks.
#[derive(Clone, Debug, PartialEq)]
enum MethodEffect {
    /// A reply that the process sends on `ret` at once.
    Reply(Par),
    /// A collection operation. The process replies `Nil` as the last branch of its outcome.
    Operation(SpaceOp),
}

/// True when a typed reply can use `ret`: one unforgeable, or one bundle with the write flag,
/// and nothing else (A23).
pub fn is_reply_name(ret: &Par) -> bool {
    namespace::sole_unforgeable(ret).is_some()
        || matches!(
            sole_bundle(ret),
            Some(Bundle {
                write_flag: true,
                ..
            })
        )
}

/// The typed error reply `("rho:space:error", code, detail)` (A23).
pub fn typed_error(code: SpaceReplyCode, detail: Vec<(&str, Par)>) -> Par {
    let entries = detail
        .into_iter()
        .map(|(key, value)| (string_par(key), value))
        .collect();
    tuple_par(vec![
        string_par(SPACE_ERROR_REPLY_TAG),
        string_par(code.code()),
        map_par(entries),
    ])
}

/// The process `HANDLE_OP`.
#[derive(Clone)]
pub struct HandleProcess {
    runtime: Arc<ReifiedRuntime>,
    agent: Arc<dyn SpaceAgent>,
}

impl HandleProcess {
    /// The handle process of the native spaces of `runtime`. `agent` reads the config records
    /// and makes the names of `gensym`.
    pub fn new(runtime: Arc<ReifiedRuntime>, agent: Arc<dyn SpaceAgent>) -> Self {
        Self { runtime, agent }
    }

    /// Runs the call `(this, message)` and replies on the `ret` of the message.
    pub async fn call(
        &self,
        contract: &ContractCall,
        contract_args: ContractArgs,
    ) -> Result<Vec<Par>, InterpreterError> {
        let call = match contract_args.0.as_slice() {
            [call] => call.clone(),
            _ => return Err(illegal_argument_error(HANDLE_OP_PROCESS.urn)),
        };
        let Some((produce, _, _, args)) = contract.unapply(contract_args) else {
            return Err(illegal_argument_error(HANDLE_OP_PROCESS.urn));
        };
        let [this, message] = args.as_slice() else {
            return Err(illegal_argument_error(HANDLE_OP_PROCESS.urn));
        };
        let Some(space) = handle_of(this) else {
            return Ok(Vec::new());
        };
        let config_map = self
            .runtime
            .read_config(Arc::clone(&self.agent), space, call.cost_authority.clone())
            .await?;
        let config = SpaceConfig::from_par(&config_map).map_err(SpaceError::from)?;
        let (ret, effect) = match ParsedMessage::parse(message) {
            ParsedMessage::Dropped => return Ok(Vec::new()),
            ParsedMessage::Malformed { ret, method } => {
                let reply = method_error(SpaceReplyCode::MalformedMessage, &method, Vec::new());
                (ret, MethodEffect::Reply(reply))
            }
            ParsedMessage::Message(message) => {
                let ret = message.ret.clone();
                let effect = self
                    .run(space, (&config_map, &config), &call, message)
                    .await?;
                (ret, effect)
            }
        };
        match effect {
            MethodEffect::Reply(reply) => {
                let reply = vec![reply];
                produce(&reply, &ret).await?;
                Ok(reply)
            }
            MethodEffect::Operation(op) => {
                let reply = vec![Par::default()];
                let replied = produce(&reply, &ret);
                let then: Pin<
                    Box<dyn Future<Output = Result<DispatchType, InterpreterError>> + Send>,
                > = Box::pin(async move { replied.await.map(|_| DispatchType::Skip) });
                reducer(&contract.dispatcher)?
                    .space_op(op, Some(then))
                    .await?;
                Ok(reply)
            }
        }
    }

    async fn run(
        &self,
        space: NativeSpace,
        (config_map, config): (&Par, &SpaceConfig),
        call: &ListParWithRandom,
        message: HandleMessage,
    ) -> Result<MethodEffect, InterpreterError> {
        let HandleMessage {
            method,
            args,
            params: SendParams { positional, named },
            ..
        } = message;
        if !positional.is_empty() {
            return Err(SpaceError::PositionalParamsUnsupported.into());
        }
        let Some(handle_method) = HandleMethod::parse(&method) else {
            let suggested = suggestion(&method, &HANDLE_METHODS)
                .map(string_par)
                .unwrap_or_default();
            return Ok(MethodEffect::Reply(method_error(
                SpaceReplyCode::UnknownMethod,
                &method,
                vec![("suggestion", suggested)],
            )));
        };
        if args.len() != handle_method.arity() {
            return Ok(MethodEffect::Reply(bad_argument(&method, "arguments")));
        }
        let schema = param_schema(handle_method, config)?;
        if let Some(key) = named.keys().find(|key| !schema.contains(&key.as_str())) {
            let suggested = suggestion(key, &schema).map(string_par).unwrap_or_default();
            return Ok(MethodEffect::Reply(method_error(
                SpaceReplyCode::UnknownParam,
                &method,
                vec![("param", string_par(key)), ("suggestion", suggested)],
            )));
        }
        match handle_method {
            HandleMethod::Gensym => {
                let random = Blake2b512Random::from_bytes(&call.random_state);
                Ok(MethodEffect::Reply(self.agent.gensym(space, &random)?))
            }
            HandleMethod::Qualifier => {
                Ok(MethodEffect::Reply(string_par(config.qualifier.as_str())))
            }
            HandleMethod::Config => Ok(MethodEffect::Reply(config_map.clone())),
            HandleMethod::Produce => produce_method(space, call, &method, &args, &named),
            HandleMethod::Consume => consume_method(&method, &args).map(MethodEffect::Reply),
        }
    }
}

/// The named params of a method: `persistent` and the send schema of the family for
/// `produce`, and none for the other methods.
fn param_schema(
    method: HandleMethod,
    config: &SpaceConfig,
) -> Result<Vec<&'static str>, SpaceError> {
    match method {
        HandleMethod::Produce => {
            let family = family_of(config)?;
            let discipline = send_schema(family).ok_or_else(|| SpaceError::FamilyUnavailable {
                outer: family.outer.as_str().to_string(),
                inner: family.inner.as_str().to_string(),
            })?;
            Ok(std::iter::once(PERSISTENT_PARAM)
                .chain(discipline.iter().copied())
                .collect())
        }
        HandleMethod::Gensym
        | HandleMethod::Qualifier
        | HandleMethod::Config
        | HandleMethod::Consume => Ok(Vec::new()),
    }
}

/// `produce(ch, msg; persistent = b)`: one collection operation on the key of `ch`, then
/// `Nil`. A bad argument is a typed error reply.
fn produce_method(
    space: NativeSpace,
    call: &ListParWithRandom,
    method: &str,
    args: &[Par],
    named: &BTreeMap<String, Par>,
) -> Result<MethodEffect, InterpreterError> {
    let [ch, msg] = args else {
        return Ok(MethodEffect::Reply(bad_argument(method, "arguments")));
    };
    let Some(channel) = writable_channel(ch) else {
        return Ok(MethodEffect::Reply(bad_argument(method, "ch")));
    };
    let Some(payload) = sole_list(msg) else {
        return Ok(MethodEffect::Reply(bad_argument(method, "msg")));
    };
    let persistent = match named.get(PERSISTENT_PARAM) {
        None => false,
        Some(value) => match sole_bool(value) {
            Some(persistent) => persistent,
            None => return Ok(MethodEffect::Reply(bad_argument(method, PERSISTENT_PARAM))),
        },
    };
    let op = ProduceOp {
        space,
        key: CollectionKey::of_channel(&channel)?,
        channel,
        data: ListParWithRandom {
            pars: payload.to_vec(),
            random_state: call.random_state.clone(),
            cost_authority: call.cost_authority.clone(),
            cost_stack: None,
        },
        persistent,
        authority: call.cost_authority.clone().unwrap_or_default(),
        reissued_ord: None,
    };
    Ok(MethodEffect::Operation(SpaceOp::Produce(op)))
}

/// `consume(pairs, arrow, body)`: the shape checks of a consume whose matchers are user-agent
/// matchers (P8). A message that passes them aborts with `AgentSpacesUnavailable`.
fn consume_method(method: &str, args: &[Par]) -> Result<Par, InterpreterError> {
    let [pairs, arrow, body] = args else {
        return Ok(bad_argument(method, "arguments"));
    };
    if !sole_string(arrow).is_some_and(|arrow| CONSUME_ARROWS.contains(&arrow)) {
        return Ok(method_error(
            SpaceReplyCode::InvalidArrow,
            method,
            Vec::new(),
        ));
    }
    let matchers = match sole_list(pairs) {
        Some(pairs) if !pairs.is_empty() => pairs
            .iter()
            .map(|pair| match sole_tuple(pair) {
                Some([matcher, _channel]) => Some(matcher),
                Some(_) | None => None,
            })
            .collect::<Option<Vec<&Par>>>(),
        Some(_) | None => None,
    };
    let Some(matchers) = matchers else {
        return Ok(bad_argument(method, "pairs"));
    };
    if !matches!(capability_id(body), Some(ReifiedId::Body(..))) {
        return Ok(bad_argument(method, "body"));
    }
    if let Some(index) = matchers
        .iter()
        .position(|matcher| !matches!(capability_id(matcher), Some(ReifiedId::Matcher(..))))
    {
        let index = i64::try_from(index).map_err(|_| {
            InterpreterError::BugFoundError("a matcher index does not fit in i64".to_string())
        })?;
        return Ok(method_error(SpaceReplyCode::ForeignMatcher, method, vec![
            ("index", new_gint_par(index, Vec::new(), false)),
        ]));
    }
    Err(ReifiedReduceError::AgentSpacesUnavailable.into())
}

/// The reducer of the runtime, from the reducer cell of the dispatcher.
fn reducer(dispatcher: &RhoDispatch) -> Result<Arc<ReducerCore>, InterpreterError> {
    dispatcher
        .reducer
        .get()
        .and_then(Weak::upgrade)
        .ok_or_else(|| InterpreterError::BugFoundError("Reducer not initialized".to_string()))
}

fn method_error(code: SpaceReplyCode, method: &str, detail: Vec<(&str, Par)>) -> Par {
    let entries = std::iter::once(("method", string_par(method)))
        .chain(detail)
        .collect();
    typed_error(code, entries)
}

fn bad_argument(method: &str, argument: &str) -> Par {
    method_error(SpaceReplyCode::BadArgument, method, vec![(
        "argument",
        string_par(argument),
    )])
}

/// The channel of `ch` under the write rule of `eval_send`: the body of a bundle with the write
/// flag, or `ch` itself when it is no bundle.
fn writable_channel(ch: &Par) -> Option<Par> {
    match single_bundle(ch) {
        Some(Bundle {
            body: Some(body),
            write_flag: true,
            ..
        }) => Some(body),
        Some(Bundle { .. }) => None,
        None => Some(ch.clone()),
    }
}

/// The Reified id of a capability: a bare id, or an id under one bundle.
fn capability_id(capability: &Par) -> Option<ReifiedId<'_>> {
    match sole_bundle(capability) {
        Some(Bundle {
            body: Some(body), ..
        }) => namespace::reified_id_of(body),
        Some(Bundle { body: None, .. }) => None,
        None => namespace::reified_id_of(capability),
    }
}

/// A `Par` that holds exactly one string.
fn string_par(value: &str) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(value.to_string())),
    }])
}

/// A `Par` that holds exactly the map of `pairs`, sorted, with the bit sets that
/// `ParMap::create_from_vec` computes.
fn map_par(pairs: Vec<(Par, Par)>) -> Par {
    let map = ParMapTypeMapper::par_map_to_emap(ParMap::create_from_vec(pairs));
    Par {
        locally_free: map.locally_free.clone(),
        connective_used: map.connective_used,
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EMapBody(map)),
        }],
        ..Par::default()
    }
}

/// A `Par` that holds exactly the tuple of `ps`, with the union and the disjunction of the bit
/// sets of its elements.
fn tuple_par(ps: Vec<Par>) -> Par {
    let (locally_free, connective_used) = ps.iter().fold(
        (Vec::new(), false),
        |(locally_free, connective_used), par| {
            (
                union(locally_free, par.locally_free.clone()),
                connective_used || par.connective_used,
            )
        },
    );
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps,
                locally_free: locally_free.clone(),
                connective_used,
            })),
        }],
        locally_free,
        connective_used,
        ..Par::default()
    }
}

/// The single component of a `Par` that holds nothing else. The bit sets do not count.
enum SoleComponent<'a> {
    Expr(&'a ExprInstance),
    Bundle(&'a Bundle),
    Other,
}

fn sole_component(par: &Par) -> SoleComponent<'_> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free: _,
        connective_used: _,
        cost_signed_terms,
        cost_stacks,
    } = par;
    let structural = sends.is_empty()
        && receives.is_empty()
        && news.is_empty()
        && matches.is_empty()
        && unforgeables.is_empty()
        && connectives.is_empty()
        && conditionals.is_empty()
        && cost_signed_terms.is_empty()
        && cost_stacks.is_empty();
    match (structural, exprs.as_slice(), bundles.as_slice()) {
        (
            true,
            [Expr {
                expr_instance: Some(instance),
            }],
            [],
        ) => SoleComponent::Expr(instance),
        (true, [], [bundle]) => SoleComponent::Bundle(bundle),
        (true, _, _) | (false, _, _) => SoleComponent::Other,
    }
}

fn sole_expr(par: &Par) -> Option<&ExprInstance> {
    match sole_component(par) {
        SoleComponent::Expr(instance) => Some(instance),
        SoleComponent::Bundle(_) | SoleComponent::Other => None,
    }
}

fn sole_bundle(par: &Par) -> Option<&Bundle> {
    match sole_component(par) {
        SoleComponent::Bundle(bundle) => Some(bundle),
        SoleComponent::Expr(_) | SoleComponent::Other => None,
    }
}

fn sole_list(par: &Par) -> Option<&[Par]> {
    match sole_expr(par) {
        Some(ExprInstance::EListBody(EList {
            ps,
            remainder: None,
            ..
        })) => Some(ps),
        _ => None,
    }
}

fn sole_tuple(par: &Par) -> Option<&[Par]> {
    match sole_expr(par) {
        Some(ExprInstance::ETupleBody(ETuple { ps, .. })) => Some(ps),
        _ => None,
    }
}

fn sole_string(par: &Par) -> Option<&str> {
    match sole_expr(par) {
        Some(ExprInstance::GString(value)) => Some(value),
        _ => None,
    }
}

fn sole_bool(par: &Par) -> Option<bool> {
    match sole_expr(par) {
        Some(ExprInstance::GBool(value)) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::Send as SendProcess;
    use models::rust::reified::builders;
    use models::rust::reified::params::SendParam;
    use models::rust::space_factory_urn::SpaceFactoryUrn;
    use models::rust::utils::{new_bundle_par, new_elist_par, new_gbool_par};
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};

    use super::*;
    use crate::rust::interpreter::reified::factory::{handle_capability, FactoryProcess};
    use crate::rust::interpreter::reified::route::{classify_channel, ChannelClass};
    use crate::rust::interpreter::reified::system_processes::harness::{
        call, int, list, reply_name, space_of, string, Fixture, Reduction,
    };
    use crate::rust::interpreter::reified::system_processes::native_agent;
    use crate::rust::interpreter::spaces::factory::constructor::FactoryConstructor;
    use crate::rust::interpreter::spaces::namespace::{NameTail, HANDLE_OP, REIFIED_TAG};

    fn params(params: Vec<SendParam>) -> Par {
        builders::send_params(params)
            .expect("valid params")
            .expect("non-empty params")
    }

    fn named(key: &str, value: Par) -> Par {
        params(vec![SendParam::Named(key.to_string(), value)])
    }

    fn this(space: NativeSpace) -> Par { namespace::gprivate(namespace::handle_channel(space)) }

    fn capability(kind: u8, fill: u8) -> Par {
        let mut id = vec![REIFIED_TAG, kind];
        id.extend([fill; 32]);
        namespace::gprivate(id)
    }

    fn tuple(ps: Vec<Par>) -> Par { tuple_par(ps) }

    fn handle(fixture: &Fixture) -> HandleProcess {
        HandleProcess::new(Arc::clone(&fixture.runtime), Arc::clone(&fixture.agent))
    }

    fn expected_error(code: SpaceReplyCode, method: &str, detail: Vec<(&str, Par)>) -> Par {
        let mut entries = vec![("method", string(method))];
        entries.extend(detail);
        typed_error(code, entries)
    }

    async fn reply_of(
        fixture: &Fixture,
        space: NativeSpace,
        message: Vec<Par>,
        seed: &[u8],
    ) -> Result<Vec<Par>, InterpreterError> {
        handle(fixture)
            .call(
                &fixture.contract,
                call(vec![this(space), list(message)], seed),
            )
            .await
    }

    #[test]
    fn reified_malformed_message_dropped() {
        let ret = reply_name(1);
        let mut two_names = ret.clone();
        two_names.unforgeables.extend(reply_name(2).unforgeables);
        for message in [
            string("not a list"),
            list(Vec::new()),
            list(vec![ret.clone()]),
            list(vec![int(1), string("gensym")]),
            list(vec![string("ret"), string("gensym")]),
            list(vec![
                new_bundle_par(ret.clone(), false, true),
                string("gensym"),
            ]),
            list(vec![two_names, string("gensym")]),
            list(vec![ret.clone(), int(1)]),
            list(vec![ret.clone(), list(vec![string("gensym")])]),
            new_elist_par(
                vec![ret.clone(), string("gensym")],
                Vec::new(),
                false,
                Some(Default::default()),
                Vec::new(),
                false,
            ),
        ] {
            assert_eq!(
                ParsedMessage::parse(&message),
                ParsedMessage::Dropped,
                "{message:?}"
            );
        }
        let mut stale = named(PERSISTENT_PARAM, new_gbool_par(true, Vec::new(), false));
        stale.connective_used = true;
        assert_eq!(
            ParsedMessage::parse(&list(vec![ret.clone(), string("produce"), stale])),
            ParsedMessage::Malformed {
                ret: ret.clone(),
                method: "produce".to_string(),
            }
        );
        let persistent = named(PERSISTENT_PARAM, new_gbool_par(true, Vec::new(), false));
        let write_bundle = new_bundle_par(ret.clone(), true, false);
        assert_eq!(
            ParsedMessage::parse(&list(vec![
                write_bundle.clone(),
                string("produce"),
                string("ch"),
                list(Vec::new()),
                persistent,
            ])),
            ParsedMessage::Message(HandleMessage {
                ret: write_bundle,
                method: "produce".to_string(),
                args: vec![string("ch"), list(Vec::new())],
                params: SendParams {
                    positional: Vec::new(),
                    named: BTreeMap::from([(
                        PERSISTENT_PARAM.to_string(),
                        new_gbool_par(true, Vec::new(), false)
                    )]),
                },
            })
        );
        assert_eq!(
            ParsedMessage::parse(&list(vec![ret.clone(), string("gensym")])),
            ParsedMessage::Message(HandleMessage {
                ret,
                method: "gensym".to_string(),
                args: Vec::new(),
                params: SendParams::default(),
            })
        );
    }

    #[test]
    fn space_handle_methods_reply_names_and_typed_errors() {
        let name = reply_name(1);
        assert!(is_reply_name(&name));
        assert!(is_reply_name(&new_bundle_par(name.clone(), true, false)));
        assert!(is_reply_name(&new_bundle_par(name.clone(), true, true)));
        assert!(!is_reply_name(&new_bundle_par(name.clone(), false, true)));
        assert!(!is_reply_name(&new_bundle_par(name.clone(), false, false)));
        assert!(!is_reply_name(&string("ret")));
        assert!(!is_reply_name(&Par::default()));
        let mut with_expr = name;
        with_expr.exprs.extend(int(1).exprs);
        assert!(!is_reply_name(&with_expr));
        let reply = typed_error(SpaceReplyCode::UnknownMethod, vec![(
            "method",
            string("frobnicate"),
        )]);
        assert_eq!(
            sole_tuple(&reply).map(<[Par]>::to_vec),
            Some(vec![
                string(SPACE_ERROR_REPLY_TAG),
                string("UnknownMethod"),
                map_par(vec![(string("method"), string("frobnicate"))]),
            ])
        );
        assert!(!reply.connective_used);
        for method in HANDLE_METHODS {
            let parsed = HandleMethod::parse(method).expect("a handle method");
            assert_eq!(parsed.arity(), match parsed {
                HandleMethod::Produce => 2,
                HandleMethod::Consume => 3,
                HandleMethod::Gensym | HandleMethod::Qualifier | HandleMethod::Config => 0,
            });
        }
        let mut sorted = HANDLE_METHODS;
        sorted.sort_unstable();
        assert_eq!(sorted, HANDLE_METHODS);
        assert_eq!(HandleMethod::parse("Produce"), None);
    }

    #[tokio::test]
    async fn reified_internal_op_requires_handle_capability() {
        let fixture = Fixture::new().await;
        let space = fixture
            .create(b"capability")
            .await
            .expect("the factory creates the space");
        let ret = reply_name(1);
        let message = list(vec![ret.clone(), string("config")]);
        let mut with_expr = this(space);
        with_expr.exprs.extend(int(1).exprs);
        for forged in [
            handle_capability(space),
            new_bundle_par(this(space), true, true),
            namespace::gprivate(namespace::config_channel(space)),
            namespace::gprivate(namespace::name_id(space, NameTail::Random([1; 32]))),
            namespace::gprivate(namespace::op_channel(HANDLE_OP)),
            namespace::gprivate(namespace::handle_channel(space)[..33].to_vec()),
            string("this"),
            Par::default(),
            with_expr,
        ] {
            assert_eq!(
                handle(&fixture)
                    .call(
                        &fixture.contract,
                        call(vec![forged.clone(), message.clone()], b"call")
                    )
                    .await,
                Ok(Vec::new()),
                "{forged:?}"
            );
        }
        assert!(fixture.data(&ret).await.is_empty());
        let reply = handle(&fixture)
            .call(&fixture.contract, call(vec![this(space), message], b"call"))
            .await
            .expect("the bare handle is accepted");
        assert_eq!(fixture.data(&ret).await, vec![reply]);
    }

    #[tokio::test]
    async fn space_handle_methods_read_only_methods_stage_nothing() {
        let fixture = Fixture::new().await;
        let space = fixture
            .create(b"read only")
            .await
            .expect("the factory creates the space");
        fixture.rspace.take_event_log().await;
        for (index, method) in ["config", "qualifier", "gensym", "frobnicate"]
            .into_iter()
            .enumerate()
        {
            let ret = reply_name(u8::try_from(index).expect("a small index"));
            let seed = [u8::try_from(index).expect("a small index")];
            let reply = reply_of(&fixture, space, vec![ret.clone(), string(method)], &seed)
                .await
                .expect("the method replies");
            let reply_datum = ListParWithRandom {
                pars: reply,
                random_state: Blake2b512Random::create_from_bytes(&seed).to_bytes(),
                cost_authority: None,
                cost_stack: None,
            };
            let log = fixture.rspace.take_event_log().await;
            assert_eq!(
                log,
                vec![Event::IoEvent(IOEvent::Produce(Produce::create(
                    &ret,
                    &reply_datum,
                    false
                )))],
                "{method}"
            );
        }
        for this in [
            handle_capability(space),
            namespace::gprivate(namespace::config_channel(space)),
        ] {
            let dropped = handle(&fixture)
                .call(
                    &fixture.contract,
                    call(
                        vec![this, list(vec![reply_name(9), string("config")])],
                        b"d",
                    ),
                )
                .await;
            assert_eq!(dropped, Ok(Vec::new()));
        }
        let dropped = reply_of(&fixture, space, vec![int(1), string("config")], b"d").await;
        assert_eq!(dropped, Ok(Vec::new()));
        assert!(fixture.rspace.take_event_log().await.is_empty());
    }

    #[tokio::test]
    async fn reified_handle_dangling_aborts() {
        let fixture = Fixture::new().await;
        let missing = space_of(b"never created");
        let ret = reply_name(1);
        for message in [
            list(vec![ret.clone(), string("gensym")]),
            list(vec![ret.clone(), string("config")]),
            string("not a list"),
        ] {
            match handle(&fixture)
                .call(
                    &fixture.contract,
                    call(vec![this(missing), message], b"dangling"),
                )
                .await
            {
                Err(InterpreterError::ReduceError(error)) => {
                    assert!(error.starts_with("reified:DanglingHandle: "), "{error}");
                }
                other => panic!("a call on a dangling handle returned {other:?}"),
            }
        }
        assert!(fixture.data(&ret).await.is_empty());
    }

    #[tokio::test]
    async fn space_handle_methods_gensym() {
        let fixture = Fixture::new().await;
        let space = fixture
            .create(b"gensym")
            .await
            .expect("the factory creates the space");
        let first = reply_of(
            &fixture,
            space,
            vec![reply_name(1), string("gensym")],
            b"one",
        )
        .await
        .expect("gensym replies");
        let second = reply_of(
            &fixture,
            space,
            vec![reply_name(2), string("gensym")],
            b"two",
        )
        .await
        .expect("gensym replies");
        let expected = fixture
            .agent
            .gensym(space, &Blake2b512Random::create_from_bytes(b"one"))
            .expect("the agent makes a name");
        assert_eq!(first, vec![expected]);
        assert_ne!(first, second);
        for name in first.iter().chain(second.iter()) {
            assert_eq!(classify_channel(name), ChannelClass::SpaceName(space));
        }
        assert_eq!(fixture.data(&reply_name(1)).await, vec![first]);
    }

    #[tokio::test]
    async fn space_handle_methods_qualifier() {
        let fixture = Fixture::new().await;
        let space = fixture
            .create(b"qualifier")
            .await
            .expect("the factory creates the space");
        let reply = reply_of(
            &fixture,
            space,
            vec![reply_name(1), string("qualifier")],
            b"q",
        )
        .await
        .expect("qualifier replies");
        assert_eq!(reply, vec![string("default")]);
        assert_eq!(fixture.data(&reply_name(1)).await, vec![reply]);
    }

    #[tokio::test]
    async fn space_handle_methods_config() {
        let fixture = Fixture::new().await;
        let space = fixture
            .create(b"config")
            .await
            .expect("the factory creates the space");
        let reply = reply_of(&fixture, space, vec![reply_name(1), string("config")], b"c")
            .await
            .expect("config replies");
        let expected =
            FactoryConstructor::validate(SpaceFactoryUrn::HASHMAP_QUEUE, &list(Vec::new()))
                .expect("the default constructor is valid")
                .config(space)
                .to_par();
        assert_eq!(reply, vec![expected]);
        assert_eq!(fixture.data(&reply_name(1)).await, vec![reply]);
    }

    #[tokio::test]
    async fn reified_unknown_method_typed_reply() {
        let fixture = Fixture::new().await;
        let space = fixture
            .create(b"unknown method")
            .await
            .expect("the factory creates the space");
        for (index, (method, suggested)) in [
            ("prodcue", string("produce")),
            ("Gensym", string("gensym")),
            ("frobnicate", Par::default()),
            ("", Par::default()),
        ]
        .into_iter()
        .enumerate()
        {
            let ret = reply_name(u8::try_from(index).expect("a small index"));
            let reply = reply_of(&fixture, space, vec![ret.clone(), string(method)], b"m")
                .await
                .expect("a typed reply");
            assert_eq!(reply, vec![expected_error(
                SpaceReplyCode::UnknownMethod,
                method,
                vec![("suggestion", suggested)]
            )]);
            assert_eq!(fixture.data(&ret).await, vec![reply]);
        }
    }

    #[tokio::test]
    async fn space_handle_methods_bad_argument_typed() {
        let fixture = Fixture::new().await;
        let space = fixture
            .create(b"bad arguments")
            .await
            .expect("the factory creates the space");
        let ch = string("ch");
        let payload = list(vec![int(1)]);
        let mut stale = named("k", int(1));
        stale.connective_used = true;
        let argument = |method: &str, name: &str| {
            expected_error(SpaceReplyCode::BadArgument, method, vec![(
                "argument",
                string(name),
            )])
        };
        let unknown = |method: &str, key: &str, suggested: Par| {
            expected_error(SpaceReplyCode::UnknownParam, method, vec![
                ("param", string(key)),
                ("suggestion", suggested),
            ])
        };
        let cases = [
            (
                vec![string("gensym"), int(1)],
                argument("gensym", "arguments"),
            ),
            (
                vec![string("config"), named("k", int(1))],
                unknown("config", "k", Par::default()),
            ),
            (
                vec![string("qualifier"), stale],
                expected_error(SpaceReplyCode::MalformedMessage, "qualifier", Vec::new()),
            ),
            (
                vec![string("produce"), ch.clone()],
                argument("produce", "arguments"),
            ),
            (
                vec![string("produce"), ch.clone(), int(1)],
                argument("produce", "msg"),
            ),
            (
                vec![
                    string("produce"),
                    new_bundle_par(ch.clone(), false, true),
                    payload.clone(),
                ],
                argument("produce", "ch"),
            ),
            (
                vec![
                    string("produce"),
                    ch.clone(),
                    payload.clone(),
                    named(PERSISTENT_PARAM, int(1)),
                ],
                argument("produce", PERSISTENT_PARAM),
            ),
            (
                vec![
                    string("produce"),
                    ch.clone(),
                    payload.clone(),
                    named("priority", int(1)),
                ],
                unknown("produce", "priority", Par::default()),
            ),
            (
                vec![
                    string("produce"),
                    ch.clone(),
                    payload.clone(),
                    named("persistant", new_gbool_par(true, Vec::new(), false)),
                ],
                unknown("produce", "persistant", string(PERSISTENT_PARAM)),
            ),
        ];
        for (index, (mut message, expected)) in cases.into_iter().enumerate() {
            let ret = reply_name(u8::try_from(index).expect("a small index"));
            message.insert(0, ret.clone());
            let reply = reply_of(&fixture, space, message, b"bad")
                .await
                .expect("a typed reply");
            assert_eq!(reply, vec![expected], "case {index}");
            assert_eq!(fixture.data(&ret).await, vec![reply]);
        }
        let key = CollectionKey::of_channel(&ch).expect("a key");
        assert!(fixture
            .data(&namespace::gprivate(namespace::data_channel(space, &key)))
            .await
            .is_empty());
        let positional = params(vec![SendParam::Positional(int(1))]);
        match reply_of(
            &fixture,
            space,
            vec![reply_name(20), string("gensym"), positional],
            b"p",
        )
        .await
        {
            Err(InterpreterError::ReduceError(error)) => {
                assert!(
                    error.starts_with("reified:PositionalParamsUnsupported: "),
                    "{error}"
                );
            }
            other => panic!("positional params returned {other:?}"),
        }
        assert!(fixture.data(&reply_name(20)).await.is_empty());
    }

    #[tokio::test]
    async fn space_handle_methods_consume_rejects_foreign_matcher() {
        let fixture = Fixture::new().await;
        let space = fixture
            .create(b"consume")
            .await
            .expect("the factory creates the space");
        let matcher = new_bundle_par(capability(b'X', 7), true, false);
        let body = new_bundle_par(capability(b'B', 8), true, false);
        let channel = string("ch");
        let pair = |matcher: Par| tuple(vec![matcher, channel.clone()]);
        let arrow = string("<-");
        let argument = |name: &str| {
            expected_error(SpaceReplyCode::BadArgument, "consume", vec![(
                "argument",
                string(name),
            )])
        };
        let foreign = |index: i64| {
            expected_error(SpaceReplyCode::ForeignMatcher, "consume", vec![(
                "index",
                int(index),
            )])
        };
        let invalid_arrow = expected_error(SpaceReplyCode::InvalidArrow, "consume", Vec::new());
        let cases = [
            (
                vec![list(vec![pair(matcher.clone())]), arrow.clone()],
                argument("arguments"),
            ),
            (
                vec![
                    list(vec![pair(matcher.clone())]),
                    string("<~"),
                    body.clone(),
                ],
                invalid_arrow.clone(),
            ),
            (
                vec![list(vec![pair(matcher.clone())]), int(1), body.clone()],
                invalid_arrow,
            ),
            (vec![int(1), arrow.clone(), body.clone()], argument("pairs")),
            (
                vec![list(Vec::new()), arrow.clone(), body.clone()],
                argument("pairs"),
            ),
            (
                vec![
                    list(vec![tuple(vec![matcher.clone()])]),
                    arrow.clone(),
                    body.clone(),
                ],
                argument("pairs"),
            ),
            (
                vec![
                    list(vec![list(vec![matcher.clone(), channel.clone()])]),
                    arrow.clone(),
                    body.clone(),
                ],
                argument("pairs"),
            ),
            (
                vec![
                    list(vec![pair(matcher.clone())]),
                    arrow.clone(),
                    string("body"),
                ],
                argument("body"),
            ),
            (
                vec![
                    list(vec![pair(matcher.clone()), pair(string("m"))]),
                    arrow.clone(),
                    body.clone(),
                ],
                foreign(1),
            ),
            (
                vec![
                    list(vec![pair(handle_capability(space))]),
                    string("<<-"),
                    body.clone(),
                ],
                foreign(0),
            ),
        ];
        for (index, (arguments, expected)) in cases.into_iter().enumerate() {
            let ret = reply_name(u8::try_from(index).expect("a small index"));
            let mut message = vec![ret.clone(), string("consume")];
            message.extend(arguments);
            let reply = reply_of(&fixture, space, message, b"consume")
                .await
                .expect("a typed reply");
            assert_eq!(reply, vec![expected], "case {index}");
        }
        for arrow in CONSUME_ARROWS {
            let message = vec![
                reply_name(30),
                string("consume"),
                list(vec![pair(matcher.clone()), pair(capability(b'X', 9))]),
                string(arrow),
                body.clone(),
            ];
            match reply_of(&fixture, space, message, b"agents").await {
                Err(InterpreterError::ReduceError(error)) => {
                    assert!(
                        error.starts_with("reified:AgentSpacesUnavailable: "),
                        "{error}"
                    );
                }
                other => panic!("a consume with agent matchers returned {other:?}"),
            }
        }
        assert!(fixture.data(&reply_name(30)).await.is_empty());
    }

    #[tokio::test]
    async fn space_handle_methods_produce() {
        let reduction = Reduction::new().await;
        let contract = reduction.contract();
        let agent = native_agent();
        let factory = FactoryProcess::new(
            SpaceFactoryUrn::HASHMAP_QUEUE,
            Arc::clone(&reduction.runtime),
            Arc::clone(&agent),
        );
        let handle = HandleProcess::new(Arc::clone(&reduction.runtime), agent);
        let space = space_of(b"produce");
        let plain = reply_name(1);
        let persistent = reply_name(2);
        let jobs = string("jobs");
        let events = string("events");
        let (created, first, second) = reduction
            .reduce(async {
                let created = factory
                    .call(
                        &contract,
                        call(vec![reply_name(0), list(Vec::new())], b"produce"),
                    )
                    .await;
                let first = handle
                    .call(
                        &contract,
                        call(
                            vec![
                                this(space),
                                list(vec![
                                    plain.clone(),
                                    string("produce"),
                                    jobs.clone(),
                                    list(vec![int(1), int(2)]),
                                ]),
                            ],
                            b"first",
                        ),
                    )
                    .await;
                let second = handle
                    .call(
                        &contract,
                        call(
                            vec![
                                this(space),
                                list(vec![
                                    persistent.clone(),
                                    string("produce"),
                                    new_bundle_par(events.clone(), true, false),
                                    list(vec![string("e")]),
                                    named(PERSISTENT_PARAM, new_gbool_par(true, Vec::new(), false)),
                                ]),
                            ],
                            b"second",
                        ),
                    )
                    .await;
                (created, first, second)
            })
            .await;
        created.expect("the factory creates the space");
        assert_eq!(first, Ok(vec![Par::default()]));
        assert_eq!(second, Ok(vec![Par::default()]));
        assert_eq!(reduction.data(&plain).await, vec![vec![Par::default()]]);
        assert_eq!(reduction.data(&persistent).await, vec![
            vec![Par::default()]
        ]);
        for (channel, payload, persist, seed) in [
            (jobs, vec![int(1), int(2)], false, b"first".as_slice()),
            (events, vec![string("e")], true, b"second".as_slice()),
        ] {
            let key = CollectionKey::of_channel(&channel).expect("a key");
            let data = reduction
                .rspace
                .get_data(&namespace::gprivate(namespace::data_channel(space, &key)))
                .await;
            let [datum] = data.as_slice() else {
                panic!("the collection holds {} records", data.len());
            };
            assert_eq!(datum.a.pars.get(1..), Some(payload.as_slice()));
            assert_eq!(
                datum.a.random_state,
                Blake2b512Random::create_from_bytes(seed).to_bytes()
            );
            assert_eq!(datum.persist, persist);
        }
    }

    #[tokio::test]
    async fn reified_handle_forwards_calls_through_the_handle_continuation() {
        let reduction = Reduction::new().await;
        let agent = native_agent();
        let factory = FactoryProcess::new(
            SpaceFactoryUrn::HASHMAP_QUEUE,
            Arc::clone(&reduction.runtime),
            agent,
        );
        let contract = reduction.contract();
        let created = reduction
            .reduce(factory.call(
                &contract,
                call(vec![reply_name(0), list(Vec::new())], b"forward"),
            ))
            .await
            .expect("the factory creates the space");
        let space = space_of(b"forward");
        assert_eq!(created, vec![handle_capability(space)]);
        let ret = reply_name(1);
        let term = Par::default().with_sends(vec![SendProcess {
            chan: Some(handle_capability(space)),
            data: vec![ret.clone(), string("gensym")],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        }]);
        reduction
            .evaluate(term, b"user")
            .await
            .expect("the handle call evaluates");
        let replies = reduction.data(&ret).await;
        let [reply] = replies.as_slice() else {
            panic!("the handle replied {replies:?}");
        };
        let [name] = reply.as_slice() else {
            panic!("the reply is not one name: {reply:?}");
        };
        assert_eq!(classify_channel(name), ChannelClass::SpaceName(space));
        let waiting = reduction
            .rspace
            .get_waiting_continuations(vec![this(space)])
            .await;
        assert_eq!(waiting.len(), 1);
    }
}
