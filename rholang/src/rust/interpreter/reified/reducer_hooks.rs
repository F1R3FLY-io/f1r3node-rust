//! The Reified RSpaces hooks of the reducer (plan decisions 22 and 32, P4 design C.3, readiness
//! review §2 and §4).
//!
//! This file is the child module `reified_hooks` of `reduce.rs`, so it uses the private items of
//! the reducer. Hook H1, the term filter, is `util::is_evaluated_process_expr`.
//!
//! * H2, `eval_marker`: `use s { P }` evaluates `P` under the use-context of the native space
//!   `s`. NEWSPACE moves the names of its annotated binders into their spaces.
//! * H3, `reified_produce`: the produce of `eval_send`. It routes the channel and applies the
//!   rules for params and for persistent sends.
//! * `reified_consume` and `reified_allocate`: the consume of `eval_receive` and the names of
//!   `eval_new`, under the use-context of the reducer.
//! * H4, `reject_query_source`: a QUERY source fails closed until P5.
//!
//! A channel that a program names without Reified syntax routes to the default space. Then each
//! hook calls the HEAD method with identical arguments, so ordinary execution does not change.
//! A space route runs one `Intent::Space` operation with `space_op`.
//!
//! HEAD evaluation and substitution keep the `connective_used` flag of a tuple, a list or a map
//! when they replace a variable in it. So before each strict decode, a hook recomputes the flags
//! that the canonical encoder derives from the children (I-20). The decoder still accepts only
//! the canonical image of the encoder.

use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    BindPattern, CostAuthority, EList, EMap, ETuple, Expr, ListParWithRandom, New, Par,
    ParWithRandom, TaggedContinuation,
};
use models::rust::reified::encoding::{
    decode_process_tuple, decode_query_source, marker_of, sole_marker_tuple, split_trailing_params,
    Marker, NewSpace, ProcessConstruct, ReifiedDecodeError, SendParams, UseBlock,
};
use models::rust::utils::{new_elist_par, union};

use super::ReducerCore;
use crate::rust::interpreter::accounting::costs::new_bindings_cost;
use crate::rust::interpreter::dispatch::DispatchType;
use crate::rust::interpreter::env::Env;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::reified::errors::ReifiedReduceError;
use crate::rust::interpreter::reified::route::{classify_channel, ChannelClass, Route};
use crate::rust::interpreter::reified::scope::{
    classify_space_ref, wrap_body, SpaceRef, UseTarget,
};
use crate::rust::interpreter::reified::system_channels::{
    check_persistent_send, handle_of, is_factory_op, op_of,
};
use crate::rust::interpreter::reified::system_processes::native_agent;
use crate::rust::interpreter::spaces::agent::{
    ConsumeOp, ProduceOp, SpaceFire, SpaceOp, SpaceOutcome, SpaceReissue,
};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::namespace::{self, CollectionKey, NameTail, NativeSpace};
use crate::rust::interpreter::{deterministic_reduction, unwrap_option_safe};

/// A boxed reducer future, as `produce`, `consume` and `dispatch` return it.
type DispatchFuture<'a> =
    Pin<Box<dyn Future<Output = Result<DispatchType, InterpreterError>> + std::marker::Send + 'a>>;

/// The dispatch data of a fired continuation: `(channel, matched datum, removed datum,
/// persistent)` for each bind.
type DispatchData = Vec<(Par, ListParWithRandom, ListParWithRandom, bool)>;

/// The target of a send after routing.
enum SendRoute {
    /// The HEAD produce on the default space.
    Host {
        chan: Par,
        data: ListParWithRandom,
        authority: CostAuthority,
    },
    /// A produce on a collection of a native space.
    Space(ProduceOp),
}

/// The target of a receive after routing.
enum ReceiveRoute {
    /// The HEAD consume on the default space.
    Host(Vec<(BindPattern, Par)>),
    /// A single-bind consume on a collection of a native space.
    Space {
        space: NativeSpace,
        key: CollectionKey,
        channel: Par,
        pattern: BindPattern,
    },
}

/// The spaces of the plain binders of a `new`.
enum BinderSpaces<'s> {
    /// The space of binder `i` is entry `i`. `None` keeps the name of HEAD.
    Each(&'s [Option<NativeSpace>]),
    /// Every plain binder goes to this space.
    All(NativeSpace),
}

impl ReducerCore {
    /// Hook H2: evaluates a USE or NEWSPACE tuple in process position (P4 design C.3). The term
    /// filter (H1) admits no other tuple. A tuple that is not canonical after the I-20 refresh
    /// fails with `Malformed`.
    pub(super) async fn eval_marker(
        &self,
        tuple: &ETuple,
        env: &Env<Par>,
        rand: Blake2b512Random,
        authority: &CostAuthority,
    ) -> Result<(), InterpreterError> {
        let construct =
            decode_process_tuple(&refreshed_tuple(tuple)).map_err(ReifiedReduceError::Malformed)?;
        match construct {
            ProcessConstruct::Use(block) => self.eval_use(block, env, rand, authority).await,
            ProcessConstruct::NewSpace(new_space) => {
                self.eval_new_space(new_space, env, rand, authority).await
            }
        }
    }

    /// `use s { P }`: evaluates `P` with the use-context of the native space `s`. `P` gets the
    /// random state of the tuple, so the tuple is random-transparent (P4 design C.2).
    async fn eval_use(
        &self,
        block: UseBlock,
        env: &Env<Par>,
        rand: Blake2b512Random,
        authority: &CostAuthority,
    ) -> Result<(), InterpreterError> {
        let UseBlock { space, body } = block;
        let (space, native) = self.native_space_ref(&space, env)?;
        let mut scoped = self.clone();
        scoped.space_ctx = Some(Arc::new(UseTarget { space, native }));
        scoped
            .eval_with_authority(body, env, rand, authority.clone())
            .await
    }

    /// `new x_1[: s_1], … in P` (P4 design C.3). The steps are those of `eval_new`, with one
    /// more step between the charge and the allocation:
    ///
    /// 1. Charge `new_bindings_cost` for every binder.
    /// 2. Evaluate each annotation in the outer environment and classify it. A binder without an
    ///    annotation takes the space of the use-context, or the default space without one.
    /// 3. Allocate the names with the draws of HEAD, and move each binder into its space.
    /// 4. Evaluate `P` with the remaining random state.
    async fn eval_new_space(
        &self,
        new_space: NewSpace,
        env: &Env<Par>,
        mut rand: Blake2b512Random,
        authority: &CostAuthority,
    ) -> Result<(), InterpreterError> {
        let NewSpace { annotations, new } = new_space;
        self.metering
            .reserve_reduction(new_bindings_cost(i64::from(new.bind_count)))?;
        let context = self.context_space();
        let mut spaces = Vec::with_capacity(annotations.len());
        for annotation in &annotations {
            spaces.push(match annotation {
                Some(reference) => Some(self.native_space_ref(reference, env)?.1),
                None => context,
            });
        }
        let bound = self.allocate_in_spaces(&new, env, &mut rand, BinderSpaces::Each(&spaces))?;
        let body = unwrap_option_safe(new.p)?;
        self.eval_with_authority(body, &bound, rand, authority.clone())
            .await
    }

    /// Evaluates a space reference as `unbundle_receive` evaluates a source, and classifies it
    /// (P4 design C.2). A space that a user agent implements fails closed until P8.
    fn native_space_ref(
        &self,
        reference: &Par,
        env: &Env<Par>,
    ) -> Result<(Par, NativeSpace), InterpreterError> {
        let evaluated = self.eval_expr(reference, env)?;
        let space = self.substitute.substitute_and_charge(&evaluated, 0, env)?;
        match classify_space_ref(&space) {
            Ok(SpaceRef::Native(native)) => Ok((space, native)),
            Ok(SpaceRef::Agent(_)) => Err(ReifiedReduceError::AgentSpacesUnavailable.into()),
            Err(error) => Err(ReifiedReduceError::from(error).into()),
        }
    }

    /// The native space of the use-context, or `None` outside every `use` block.
    fn context_space(&self) -> Option<NativeSpace> {
        self.space_ctx.as_ref().map(|target| target.native)
    }

    /// Hook H3: the produce of `eval_send` (P4 design C.3, readiness review §4).
    ///
    /// 1. A trailing PARAMS tuple is decoded after the I-20 refresh. A malformed tuple fails with
    ///    `Malformed`.
    /// 2. The channel routes with the use-context of the reducer.
    /// 3. On the default space, a persistent send to an op channel or to a handle aborts (A21).
    ///    A factory call gets its constructor as one list. A handle call keeps its params. Every
    ///    other channel rejects params.
    /// 4. On a native space, the produce runs with `space_op`. A queue takes no params.
    /// 5. A space that a user agent implements fails closed until P8.
    pub(super) fn reified_produce<'a>(
        &'a self,
        chan: Par,
        data: ListParWithRandom,
        persistent: bool,
        introduction_authority: CostAuthority,
    ) -> Pin<
        Box<dyn Future<Output = Result<DispatchType, InterpreterError>> + std::marker::Send + 'a>,
    > {
        match self.route_send(chan, data, persistent, introduction_authority) {
            Ok(SendRoute::Host {
                chan,
                data,
                authority,
            }) => self.produce(chan, data, persistent, authority),
            Ok(SendRoute::Space(op)) => Box::pin(self.space_op(SpaceOp::Produce(op), None)),
            Err(error) => Box::pin(std::future::ready(Err(error))),
        }
    }

    fn route_send(
        &self,
        chan: Par,
        mut data: ListParWithRandom,
        persistent: bool,
        authority: CostAuthority,
    ) -> Result<SendRoute, InterpreterError> {
        let params = trailing_params(&mut data.pars)?;
        match self.route_channel(&chan)? {
            Route::Default => {
                check_persistent_send(&chan, persistent)?;
                let data = match (op_of(&chan), handle_of(&chan)) {
                    (Some(op), _) if is_factory_op(op) => {
                        reject_positional(params.as_ref())?;
                        pack_constructor(data)
                    }
                    (_, Some(_)) => {
                        reject_positional(params.as_ref())?;
                        data
                    }
                    _ if params.is_some() => {
                        return Err(ReifiedReduceError::ParamsUnsupported.into())
                    }
                    _ => data,
                };
                Ok(SendRoute::Host {
                    chan,
                    data,
                    authority,
                })
            }
            Route::Space { space, key } => match params {
                None => Ok(SendRoute::Space(ProduceOp {
                    space,
                    key,
                    channel: chan,
                    data,
                    persistent,
                    authority,
                    reissued_ord: None,
                })),
                Some(params) => Err(space_params_error(params)),
            },
            Route::Agent => Err(ReifiedReduceError::AgentSpacesUnavailable.into()),
        }
    }

    /// The consume of `eval_receive` (P4 design C.3). It takes the parameters of `consume`.
    ///
    /// Outside a use-context, a receive whose channels all route to the default space calls
    /// `consume` with identical arguments. Otherwise the body is wrapped in the USE tuple of the
    /// use-context first, and the routes decide:
    ///
    /// * Every channel on the default space: `consume` with the wrapped body.
    /// * One channel on a native space: a consume with `space_op`.
    /// * Two or more channels on one native space: `OrderedJoinUnsupported` (A3).
    /// * A channel of a user agent: `AgentSpacesUnavailable`.
    /// * Every other join: `CrossSpaceJoin`.
    pub(in crate::rust::interpreter) fn reified_consume<'a>(
        &'a self,
        binds: Vec<(BindPattern, Par)>,
        body: ParWithRandom,
        persistent: bool,
        peek: bool,
        guard: Option<Par>,
        authority: CostAuthority,
        introduction_authority: CostAuthority,
    ) -> Pin<
        Box<dyn Future<Output = Result<DispatchType, InterpreterError>> + std::marker::Send + 'a>,
    > {
        if self.space_ctx.is_none()
            && binds
                .iter()
                .all(|(_, channel)| routes_to_default_without_context(channel))
        {
            return self.consume(
                binds,
                body,
                persistent,
                peek,
                guard,
                authority,
                introduction_authority,
            );
        }
        let ParWithRandom { body, random_state } = body;
        let body = ParWithRandom {
            body: body.map(|body| wrap_body(&self.space_ctx, body)),
            random_state,
        };
        match self.route_receive(binds) {
            Ok(ReceiveRoute::Host(binds)) => self.consume(
                binds,
                body,
                persistent,
                peek,
                guard,
                authority,
                introduction_authority,
            ),
            Ok(ReceiveRoute::Space {
                space,
                key,
                channel,
                pattern,
            }) => Box::pin(self.space_op(
                SpaceOp::Consume(ConsumeOp {
                    space,
                    key,
                    channel,
                    pattern,
                    body,
                    persistent,
                    peek,
                    guard,
                    authority,
                    introduction_authority,
                    reissued_ord: None,
                }),
                None,
            )),
            Err(error) => Box::pin(std::future::ready(Err(error))),
        }
    }

    fn route_receive(
        &self,
        binds: Vec<(BindPattern, Par)>,
    ) -> Result<ReceiveRoute, InterpreterError> {
        let mut routed = Vec::with_capacity(binds.len());
        for (pattern, channel) in binds {
            let route = self.route_channel(&channel)?;
            routed.push((pattern, channel, route));
        }
        if routed
            .iter()
            .all(|(_, _, route)| matches!(route, Route::Default))
        {
            return Ok(ReceiveRoute::Host(
                routed
                    .into_iter()
                    .map(|(pattern, channel, _)| (pattern, channel))
                    .collect(),
            ));
        }
        match <[_; 1]>::try_from(routed) {
            Ok([(pattern, channel, Route::Space { space, key })]) => Ok(ReceiveRoute::Space {
                space,
                key,
                channel,
                pattern,
            }),
            Ok([(pattern, channel, Route::Default)]) => {
                Ok(ReceiveRoute::Host(vec![(pattern, channel)]))
            }
            Ok([(_, _, Route::Agent)]) => Err(ReifiedReduceError::AgentSpacesUnavailable.into()),
            Err(routed) => {
                Err(join_error(routed.iter().map(|(_, _, route)| route).collect()).into())
            }
        }
    }

    /// The names of `eval_new` (P4 design C.3). It takes the parameters of
    /// `allocate_new_bindings` without the URN map of the reducer. Outside a use-context the
    /// result is the result of `allocate_new_bindings`. Inside a use-context, each plain binder
    /// becomes a name of the space of the context, with the same random draws. The URI binders
    /// keep their values.
    pub(super) fn reified_allocate(
        &self,
        new: &New,
        env: &Env<Par>,
        rand: &mut Blake2b512Random,
    ) -> Result<Env<Par>, InterpreterError> {
        match self.context_space() {
            None => super::allocate_new_bindings(new, env, rand, &self.urn_map),
            Some(space) => self.allocate_in_spaces(new, env, rand, BinderSpaces::All(space)),
        }
    }

    /// Allocates the names of `new` with `allocate_new_bindings`, then moves each plain binder
    /// that has a space into it: `GPrivate(r)` becomes `GPrivate(E0‖N‖sid‖r)`.
    fn allocate_in_spaces(
        &self,
        new: &New,
        env: &Env<Par>,
        rand: &mut Blake2b512Random,
        spaces: BinderSpaces<'_>,
    ) -> Result<Env<Par>, InterpreterError> {
        let mut bound = super::allocate_new_bindings(new, env, rand, &self.urn_map)?;
        for offset in 0..plain_binders(new)? {
            let space = match spaces {
                BinderSpaces::Each(spaces) => spaces.get(offset).copied().flatten(),
                BinderSpaces::All(space) => Some(space),
            };
            if let Some(space) = space {
                stamp_name(&mut bound, binder_level(env, offset)?, space)?;
            }
        }
        Ok(bound)
    }

    /// Runs one operation on a collection of a native space as an `Intent::Space` job, then
    /// dispatches its fires and its re-issue (P4 design C.3). The routed sends and receives and
    /// the handle methods reach a space through this method. `then` is the work of the caller
    /// after the operation, for example the reply of a handle method. It runs as the last branch
    /// of the outcome (see `dispatch_space_outcome`).
    pub(crate) async fn space_op(
        &self,
        op: SpaceOp,
        then: Option<DispatchFuture<'static>>,
    ) -> Result<DispatchType, InterpreterError> {
        let outcome = self.reified.run_op(native_agent(), op).await?;
        self.dispatch_space_outcome(outcome, then).await
    }

    /// Dispatches the fires of a committed space operation, runs its re-issue and then the
    /// work `then` of the caller (P4 design C.3, readiness review H6).
    ///
    /// Each fire dispatches its continuation with the matched data, as a HEAD COMM does. The
    /// substrate keeps a peeked record, so no fire produces a datum again.
    ///
    /// In a reduction, a dispatch ends the participant that makes it: `eval_continuation`
    /// moves the participant to the detached task of the body. So the participant can make no
    /// operation after a dispatch. For this reason, `then` is a branch of the outcome, as the
    /// re-issue is. The branches are the fires in staging order, then the re-issue (a new
    /// `space_op`), then `then`. Each fire and the re-issue has its own metering child.
    ///
    /// A single fire without a re-issue and without `then` dispatches directly, and `then`
    /// without fires and without a re-issue runs directly. Otherwise the branches run as
    /// parallel participants through `run_parallel_dispatches`, which needs a reduction
    /// context. Without one, the branches run in index order.
    pub(crate) fn dispatch_space_outcome<'a>(
        &'a self,
        outcome: SpaceOutcome,
        then: Option<DispatchFuture<'static>>,
    ) -> DispatchFuture<'a> {
        Box::pin(async move {
            let SpaceOutcome { fires, reissue } = outcome;
            if fires.is_empty() && reissue.is_none() {
                return match then {
                    Some(then) => then.await,
                    None => Ok(DispatchType::Skip),
                };
            }
            let is_replay = self.space.is_replay().await;
            let fires = match (
                <[SpaceFire; 1]>::try_from(fires),
                reissue.is_some() || then.is_some(),
            ) {
                (Ok([fire]), false) => {
                    let (continuation, data) = dispatch_parts(fire);
                    return self
                        .dispatch(continuation, data, is_replay, Vec::new())
                        .await;
                }
                (Ok(fire), true) => Vec::from(fire),
                (Err(fires), _) => fires,
            };
            let mut branches: Vec<DispatchFuture<'static>> = Vec::with_capacity(
                fires.len() + usize::from(reissue.is_some()) + usize::from(then.is_some()),
            );
            for (index, fire) in fires.into_iter().enumerate() {
                let branch = self.with_metering_child(index);
                let (continuation, data) = dispatch_parts(fire);
                branches.push(Box::pin(async move {
                    branch
                        .dispatch(continuation, data, is_replay, Vec::new())
                        .await
                }) as DispatchFuture<'static>);
            }
            if let Some(SpaceReissue { op, .. }) = reissue {
                let branch = self.with_metering_child(branches.len());
                branches.push(Box::pin(async move { branch.space_op(op, None).await })
                    as DispatchFuture<'static>);
            }
            branches.extend(then);
            match deterministic_reduction::current() {
                Some(_) => self.run_parallel_dispatches(branches).await,
                None => self.run_sequential_dispatches(branches).await,
            }
        })
    }

    /// Runs dispatch branches one after another in index order, and aggregates their errors as
    /// `run_parallel_dispatches` does.
    async fn run_sequential_dispatches(
        &self,
        branches: Vec<DispatchFuture<'static>>,
    ) -> Result<DispatchType, InterpreterError> {
        let mut errors = Vec::new();
        for branch in branches {
            if let Err(error) = branch.await {
                errors.push(error);
            }
        }
        self.aggregate_evaluator_errors(errors)
    }
}

/// Hook H4: checks the evaluated source of a receive bind before `unbundle_receive` checks its
/// bundle. A QUERY tuple fails closed until P5 (readiness review H3). The I-20 refresh runs
/// before the decode.
pub(super) fn reject_query_source(source: &Par) -> Result<(), InterpreterError> {
    match decode_query_source(&refreshed_marker_par(source, Marker::Query)) {
        Ok(None) => Ok(()),
        Ok(Some(_)) => Err(ReifiedReduceError::QuerySpecUnsupported.into()),
        Err(error) => Err(ReifiedReduceError::Malformed(error).into()),
    }
}

/// True when `channel` routes to the default space outside every `use` block: every class except
/// a space name (R3) and an agent-tagged name (R4). The check allocates nothing.
fn routes_to_default_without_context(channel: &Par) -> bool {
    !matches!(
        classify_channel(channel),
        ChannelClass::SpaceName(_) | ChannelClass::AgentTagged
    )
}

/// The error of a join with two or more binds whose channels do not all route to the default
/// space.
fn join_error(routes: Vec<&Route>) -> ReifiedReduceError {
    let first = match routes.first() {
        Some(Route::Space { space, .. }) => Some(*space),
        _ => None,
    };
    let one_space = first.is_some_and(|first| {
        routes
            .iter()
            .all(|route| matches!(route, Route::Space { space, .. } if *space == first))
    });
    match (
        one_space,
        routes.iter().any(|route| matches!(route, Route::Agent)),
    ) {
        (true, _) => ReifiedReduceError::OrderedJoinUnsupported,
        (false, true) => ReifiedReduceError::AgentSpacesUnavailable,
        (false, false) => ReifiedReduceError::CrossSpaceJoin,
    }
}

/// The params of a send on a factory or a handle can be named only.
fn reject_positional(params: Option<&SendParams>) -> Result<(), InterpreterError> {
    match params {
        Some(SendParams { positional, .. }) if !positional.is_empty() => {
            Err(SpaceError::PositionalParamsUnsupported.into())
        }
        Some(_) | None => Ok(()),
    }
}

/// The error of params on a collection of a queue space. The queue discipline has no send
/// params (P4 design B.4), so every named param is unknown and has no suggestion.
fn space_params_error(params: SendParams) -> InterpreterError {
    let SendParams { positional, named } = params;
    match (positional.is_empty(), named.into_keys().next()) {
        (false, _) => SpaceError::PositionalParamsUnsupported.into(),
        (true, Some(key)) => SpaceError::UnknownParam {
            key,
            suggestion: None,
        }
        .into(),
        (true, None) => ReifiedReduceError::Malformed(ReifiedDecodeError::EmptyCanonicalForm {
            marker: Marker::Params,
        })
        .into(),
    }
}

/// Packs the arguments of a factory call into `[ret, [ctor…]]`, so every factory has the fixed
/// arity 2 (I-18). The list holds the arguments after `ret`, the PARAMS tuple included. A send
/// without arguments is unchanged.
fn pack_constructor(data: ListParWithRandom) -> ListParWithRandom {
    let ListParWithRandom {
        pars,
        random_state,
        cost_authority,
        cost_stack,
    } = data;
    let mut arguments = pars.into_iter();
    let pars = match arguments.next() {
        None => Vec::new(),
        Some(ret) => {
            let constructor: Vec<Par> = arguments.collect();
            let (locally_free, connective_used) = constructor.iter().fold(
                (Vec::new(), false),
                |(locally_free, connective_used), par| {
                    (
                        union(locally_free, par.locally_free.clone()),
                        connective_used || par.connective_used,
                    )
                },
            );
            vec![
                ret,
                new_elist_par(
                    constructor,
                    locally_free.clone(),
                    connective_used,
                    None,
                    locally_free,
                    connective_used,
                ),
            ]
        }
    };
    ListParWithRandom {
        pars,
        random_state,
        cost_authority,
        cost_stack,
    }
}

/// The continuation and the dispatch data of a fire.
fn dispatch_parts(fire: SpaceFire) -> (TaggedContinuation, DispatchData) {
    let SpaceFire {
        continuation,
        results,
    } = fire;
    let data = results
        .into_iter()
        .map(|result| {
            (
                result.channel,
                result.matched_datum,
                result.removed_datum,
                result.persistent,
            )
        })
        .collect();
    (continuation, data)
}

/// The number of plain binders of a `new` that `allocate_new_bindings` accepted.
fn plain_binders(new: &New) -> Result<usize, InterpreterError> {
    usize::try_from(new.bind_count)
        .ok()
        .and_then(|binders| binders.checked_sub(new.uri.len()))
        .ok_or_else(|| {
            InterpreterError::ReduceError(
                "new URI binding count exceeds the total binding count".to_string(),
            )
        })
}

/// The environment level of plain binder `offset`. `allocate_new_bindings` puts the plain
/// binders first, from the level of `env` upward.
fn binder_level(env: &Env<Par>, offset: usize) -> Result<i32, InterpreterError> {
    i32::try_from(offset)
        .ok()
        .and_then(|offset| env.level.checked_add(offset))
        .ok_or_else(|| {
            InterpreterError::BugFoundError("a new binder level does not fit in i32".to_string())
        })
}

/// Moves the name at `level` into `space`: the 32-byte id `r` of a new name becomes the tail of
/// the name id `E0‖N‖sid‖r`.
fn stamp_name(
    bound: &mut Env<Par>,
    level: i32,
    space: NativeSpace,
) -> Result<(), InterpreterError> {
    let tail = bound
        .env_map
        .get(&level)
        .and_then(namespace::sole_gprivate_id)
        .and_then(|id| <[u8; 32]>::try_from(id).ok())
        .ok_or_else(|| {
            InterpreterError::BugFoundError(format!(
                "the new name at level {level} is not a 32-byte private name"
            ))
        })?;
    bound.env_map.insert(
        level,
        namespace::gprivate(namespace::name_id(space, NameTail::Random(tail))),
    );
    Ok(())
}

/// A field of a marker tuple after its head (I-20). The canonical encoder derives the
/// `connective_used` flag of each container field from the flags of its children. A value field
/// keeps the flags that the program gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    /// A value of the program.
    Value,
    /// A list: the list and its `Par` take the disjunction over the elements.
    List,
    /// A map: the map and its `Par` take the disjunction over the keys and the values.
    Map,
    /// The annotations of NEWSPACE: a list of the lists `[]` and `[s]`.
    Annotations,
    /// `Par{news: [New]}`: the `Par` takes the flag of the body of the `New`.
    NewBody,
}

/// The fields of the tuple of `marker`, in tuple order after the head.
const fn marker_fields(marker: Marker) -> &'static [Field] {
    match marker {
        Marker::Use => &[Field::Value, Field::Value],
        Marker::NewSpace => &[Field::Annotations, Field::NewBody],
        Marker::Params => &[Field::List, Field::Map],
        Marker::Query => &[Field::Value, Field::List, Field::Map],
        Marker::Free => &[Field::Value, Field::List],
    }
}

/// The field at tuple index `index`. The head and the elements after the last field are values.
/// The decoder rejects a tuple with extra elements whatever their flags are.
fn field_at(fields: &[Field], index: usize) -> Field {
    index
        .checked_sub(1)
        .and_then(|field| fields.get(field))
        .copied()
        .unwrap_or(Field::Value)
}

/// True when `par` holds nothing but its expressions and its news. The bit sets do not count.
fn only_exprs_and_news(par: &Par) -> bool {
    let Par {
        sends,
        receives,
        news: _,
        exprs: _,
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
    sends.is_empty()
        && receives.is_empty()
        && matches.is_empty()
        && unforgeables.is_empty()
        && bundles.is_empty()
        && connectives.is_empty()
        && conditionals.is_empty()
        && cost_signed_terms.is_empty()
        && cost_stacks.is_empty()
}

/// The expression of a `Par` that holds exactly one expression and nothing else.
fn sole_expr(par: &Par) -> Option<&ExprInstance> {
    match (
        only_exprs_and_news(par),
        par.exprs.as_slice(),
        par.news.is_empty(),
    ) {
        (
            true,
            [Expr {
                expr_instance: Some(instance),
            }],
            true,
        ) => Some(instance),
        _ => None,
    }
}

/// The mutable expression of a `Par` that holds exactly one expression and nothing else.
fn sole_expr_mut(par: &mut Par) -> Option<&mut ExprInstance> {
    match (only_exprs_and_news(par), par.news.is_empty()) {
        (true, true) => match par.exprs.as_mut_slice() {
            [Expr {
                expr_instance: Some(instance),
            }] => Some(instance),
            _ => None,
        },
        _ => None,
    }
}

/// The `New` of a `Par` that holds exactly one `New` and nothing else.
fn sole_new(par: &Par) -> Option<&New> {
    match (
        only_exprs_and_news(par),
        par.exprs.is_empty(),
        par.news.as_slice(),
    ) {
        (true, true, [new]) => Some(new),
        _ => None,
    }
}

/// The flag that the encoder derives for `par` as `field`. A `Par` without the shape of its field
/// keeps its own flag, because the decoder rejects the shape first.
fn derived_flag(par: &Par, field: Field) -> bool {
    match (field, sole_expr(par)) {
        (
            Field::List,
            Some(ExprInstance::EListBody(EList {
                ps,
                remainder: None,
                ..
            })),
        ) => ps.iter().any(|element| element.connective_used),
        (
            Field::Annotations,
            Some(ExprInstance::EListBody(EList {
                ps,
                remainder: None,
                ..
            })),
        ) => ps
            .iter()
            .any(|annotation| derived_flag(annotation, Field::List)),
        (
            Field::Map,
            Some(ExprInstance::EMapBody(EMap {
                kvs,
                remainder: None,
                ..
            })),
        ) => kvs.iter().any(|pair| {
            pair.key.as_ref().is_some_and(|key| key.connective_used)
                || pair
                    .value
                    .as_ref()
                    .is_some_and(|value| value.connective_used)
        }),
        (Field::NewBody, _) => match sole_new(par) {
            Some(new) => derived_flag_of_new(new),
            None => par.connective_used,
        },
        (Field::Value | Field::List | Field::Map | Field::Annotations, _) => par.connective_used,
    }
}

/// True when every flag that the encoder derives inside `par` as `field` has its derived value.
fn field_is_current(par: &Par, field: Field) -> bool {
    let derived = derived_flag(par, field);
    match (field, sole_expr(par)) {
        (Field::List, Some(ExprInstance::EListBody(list))) if list.remainder.is_none() => {
            par.connective_used == derived && list.connective_used == derived
        }
        (Field::Annotations, Some(ExprInstance::EListBody(list))) if list.remainder.is_none() => {
            par.connective_used == derived
                && list.connective_used == derived
                && list
                    .ps
                    .iter()
                    .all(|annotation| field_is_current(annotation, Field::List))
        }
        (Field::Map, Some(ExprInstance::EMapBody(map))) if map.remainder.is_none() => {
            par.connective_used == derived && map.connective_used == derived
        }
        (Field::NewBody, _) => par.connective_used == derived,
        _ => true,
    }
}

/// Sets every flag that the encoder derives inside `par` as `field` to its derived value.
fn refresh_field(par: &mut Par, field: Field) {
    if field == Field::NewBody {
        if let Some(flag) = sole_new(par).map(derived_flag_of_new) {
            par.connective_used = flag;
        }
        return;
    }
    let flag = match (field, sole_expr_mut(par)) {
        (Field::List, Some(ExprInstance::EListBody(list))) if list.remainder.is_none() => {
            list.connective_used = list.ps.iter().any(|element| element.connective_used);
            list.connective_used
        }
        (Field::Annotations, Some(ExprInstance::EListBody(list))) if list.remainder.is_none() => {
            for annotation in &mut list.ps {
                refresh_field(annotation, Field::List);
            }
            list.connective_used = list.ps.iter().any(|annotation| annotation.connective_used);
            list.connective_used
        }
        (Field::Map, Some(ExprInstance::EMapBody(map))) if map.remainder.is_none() => {
            map.connective_used = map.kvs.iter().any(|pair| {
                pair.key.as_ref().is_some_and(|key| key.connective_used)
                    || pair
                        .value
                        .as_ref()
                        .is_some_and(|value| value.connective_used)
            });
            map.connective_used
        }
        _ => return,
    };
    par.connective_used = flag;
}

/// The flag that `Par{news: [new]}` takes in the encoder: the flag of the body of `new`.
fn derived_flag_of_new(new: &New) -> bool {
    new.p.as_ref().is_some_and(|body| body.connective_used)
}

/// The fields of a tuple with a marker head, or `None` for every other tuple.
fn tuple_fields(tuple: &ETuple) -> Option<&'static [Field]> {
    tuple.ps.first().and_then(marker_of).map(marker_fields)
}

/// The flag that the encoder derives for a marker tuple: the disjunction over its elements.
fn derived_tuple_flag(tuple: &ETuple, fields: &[Field]) -> bool {
    tuple
        .ps
        .iter()
        .enumerate()
        .any(|(index, element)| derived_flag(element, field_at(fields, index)))
}

/// True when every flag that the encoder derives for a marker tuple has its derived value. A
/// tuple without a marker head is always current.
fn tuple_is_current(tuple: &ETuple) -> bool {
    match tuple_fields(tuple) {
        Some(fields) => {
            tuple
                .ps
                .iter()
                .enumerate()
                .all(|(index, element)| field_is_current(element, field_at(fields, index)))
                && tuple.connective_used == derived_tuple_flag(tuple, fields)
        }
        None => true,
    }
}

/// Sets every flag that the encoder derives for a marker tuple to its derived value (I-20). The
/// elements keep their values.
fn refresh_tuple(tuple: &mut ETuple) {
    let Some(fields) = tuple_fields(tuple) else {
        return;
    };
    for (index, element) in tuple.ps.iter_mut().enumerate() {
        refresh_field(element, field_at(fields, index));
    }
    tuple.connective_used = tuple.ps.iter().any(|element| element.connective_used);
}

/// A marker tuple with the flags that the encoder derives. A current tuple is not copied.
fn refreshed_tuple(tuple: &ETuple) -> Cow<'_, ETuple> {
    match tuple_is_current(tuple) {
        true => Cow::Borrowed(tuple),
        false => {
            let mut refreshed = tuple.clone();
            refresh_tuple(&mut refreshed);
            Cow::Owned(refreshed)
        }
    }
}

/// The tuple of a `Par` that holds exactly one tuple and nothing else.
fn held_tuple_mut(par: &mut Par) -> Option<&mut ETuple> {
    match sole_expr_mut(par) {
        Some(ExprInstance::ETupleBody(tuple)) => Some(tuple),
        _ => None,
    }
}

/// Sets the derived flags of the marker tuple that `par` holds, and the flag of `par`, which the
/// encoder takes from the tuple.
fn refresh_tuple_par(par: &mut Par) {
    let flag = match held_tuple_mut(par) {
        Some(tuple) => {
            refresh_tuple(tuple);
            tuple.connective_used
        }
        None => return,
    };
    par.connective_used = flag;
}

/// True when `par` holds a marker tuple with its derived flags and has the flag of that tuple.
fn tuple_par_is_current(par: &Par) -> bool {
    match sole_expr(par) {
        Some(ExprInstance::ETupleBody(tuple)) => match tuple_fields(tuple) {
            Some(fields) => {
                tuple_is_current(tuple) && par.connective_used == derived_tuple_flag(tuple, fields)
            }
            None => true,
        },
        _ => true,
    }
}

/// `par` with the derived flags of its tuple when it holds exactly a tuple of `marker`, and `par`
/// unchanged otherwise. The tuple of any other marker is inert data here and keeps its bytes.
fn refreshed_marker_par(par: &Par, marker: Marker) -> Cow<'_, Par> {
    match sole_marker_tuple(par) {
        Some((head, _)) if head == marker && !tuple_par_is_current(par) => {
            let mut refreshed = par.clone();
            refresh_tuple_par(&mut refreshed);
            Cow::Owned(refreshed)
        }
        _ => Cow::Borrowed(par),
    }
}

/// Hook H3, step 1: refreshes a trailing PARAMS tuple in place and decodes it. The data of a send
/// without a PARAMS tuple keeps its bytes.
fn trailing_params(data: &mut [Par]) -> Result<Option<SendParams>, InterpreterError> {
    if let Some(last) = data.last_mut() {
        if matches!(sole_marker_tuple(last), Some((Marker::Params, _))) {
            refresh_tuple_par(last);
        }
    }
    split_trailing_params(data).map_err(|error| ReifiedReduceError::Malformed(error).into())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use models::rhoapi::connective::ConnectiveInstance;
    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{Connective, Receive, ReceiveBind, Send as SendProcess};
    use models::rust::reified::builders::{
        new_space, push_process_expr, query_spec, send_params, use_block,
    };
    use models::rust::reified::params::SendParam;
    use models::rust::utils::{
        new_boundvar_par, new_bundle_par, new_emap_par, new_etuple_par, new_freevar_par,
        new_gint_par, new_gstring_par,
    };
    use prost::Message;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::{ISpace, RSpaceResult};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::interpreter::accounting::cost_accounting::CostAccounting;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::deterministic_reduction::root_with_observation;
    use crate::rust::interpreter::external_services::ExternalServices;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::reduce::DebruijnInterpreter;
    use crate::rust::interpreter::reified::costs::ReissueCause;
    use crate::rust::interpreter::reified::runtime::native_replay;
    use crate::rust::interpreter::rho_runtime::{create_rho_runtime, RhoISpace};
    use crate::rust::interpreter::spaces::agent::SpaceCreation;
    use crate::rust::interpreter::spaces::namespace::{
        Durability, AGENT_TAG, GET_SPACE_AGENT, HANDLE_OP, SID_LEN,
    };
    use crate::rust::interpreter::spaces::record::unwrap_datum;
    use crate::rust::interpreter::spaces::types::config::SpaceConfig;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
    use crate::rust::interpreter::util::{allocate_new_bindings, is_evaluated_process_expr};

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    /// A reducer over a fresh in-memory host, and the host.
    struct Fixture {
        host: ParRSpace,
        reducer: Arc<DebruijnInterpreter>,
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
        cost.set(Cost::create(i64::MAX, "reified hooks test".to_string()));
        Fixture { host, reducer }
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
            .set(Cost::create(i64::MAX, "reified hooks test".to_string()));
        Fixture {
            host,
            reducer: runtime.reducer,
        }
    }

    fn rand(seed: &[u8]) -> Blake2b512Random { Blake2b512Random::create_from_bytes(seed) }

    fn authority() -> CostAuthority { CostAuthority::default() }

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn private(seed: u8) -> Par { namespace::gprivate(vec![seed; SID_LEN]) }

    fn bound(index: i32) -> Par { new_boundvar_par(index, Vec::new(), false) }

    fn list(ps: Vec<Par>) -> Par {
        let connective_used = ps.iter().any(|par| par.connective_used);
        new_elist_par(
            ps,
            Vec::new(),
            connective_used,
            None,
            Vec::new(),
            connective_used,
        )
    }

    /// The process `Int`: a connective, which sets `connective_used`.
    fn int_connective() -> Par {
        Par {
            connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnInt(true)),
            }],
            connective_used: true,
            ..Par::default()
        }
    }

    /// `chan!(data…)` with the flags of the normalizer.
    fn send(chan: Par, data: Vec<Par>) -> Par {
        let connective_used = chan.connective_used || data.iter().any(|par| par.connective_used);
        Par {
            sends: vec![SendProcess {
                chan: Some(chan),
                data,
                persistent: false,
                locally_free: Vec::new(),
                connective_used,
            }],
            connective_used,
            ..Par::default()
        }
    }

    fn native(seed: u8) -> NativeSpace {
        NativeSpace {
            sid: [seed; SID_LEN],
            durability: Durability::Durable,
        }
    }

    fn handle(space: NativeSpace) -> Par { namespace::gprivate(namespace::handle_channel(space)) }

    fn space_name(space: NativeSpace, tail: [u8; 32]) -> Par {
        namespace::gprivate(namespace::name_id(space, NameTail::Random(tail)))
    }

    fn datum(pars: Vec<Par>) -> ListParWithRandom {
        ListParWithRandom {
            pars,
            random_state: rand(b"datum").to_bytes(),
            cost_authority: None,
            cost_stack: None,
        }
    }

    fn bind(channel: Par) -> (BindPattern, Par) {
        (
            BindPattern {
                patterns: vec![new_freevar_par(0, Vec::new())],
                remainder: None,
                free_count: 1,
            },
            channel,
        )
    }

    fn body(par: Par) -> ParWithRandom {
        ParWithRandom {
            body: Some(par),
            random_state: rand(b"body").to_bytes(),
        }
    }

    /// The reducer with the use-context of `space`.
    fn scoped(reducer: &ReducerCore, space: NativeSpace) -> ReducerCore {
        let mut core = reducer.clone();
        core.space_ctx = Some(Arc::new(UseTarget {
            space: handle(space),
            native: space,
        }));
        core
    }

    fn sole_tuple(par: &Par) -> &ETuple {
        match par.exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::ETupleBody(tuple)),
            }] => tuple,
            other => panic!("the Par holds {other:?} instead of one tuple"),
        }
    }

    fn env_of(value: Par) -> Env<Par> { Env::new().put(value) }

    /// `par` after the substitution that `eval_receive` applies to a stored body.
    fn substituted(reducer: &ReducerCore, par: &Par, env: &Env<Par>) -> Par {
        reducer
            .substitute
            .substitute_no_sort_and_charge(par, 0, env)
            .expect("the substitution succeeds")
    }

    /// The `reified:<Code>` of a failed hook.
    fn code<T>(result: Result<T, InterpreterError>) -> Option<String> {
        match result {
            Err(InterpreterError::ReduceError(message)) => message
                .strip_prefix("reified:")
                .and_then(|rest| rest.split(':').next())
                .map(str::to_string),
            Ok(_) | Err(_) => None,
        }
    }

    fn expected(code: &str) -> Option<String> { Some(code.to_string()) }

    async fn stored(host: &ParRSpace, channel: &Par) -> Vec<Vec<Par>> {
        let mut values: Vec<Vec<Par>> = host
            .get_data(channel)
            .await
            .into_iter()
            .map(|datum| datum.a.pars)
            .collect();
        values.sort_by_key(|pars| pars.iter().map(Message::encode_to_vec).collect::<Vec<_>>());
        values
    }

    fn next_tail(random: &mut Blake2b512Random) -> [u8; 32] {
        let mut tail = [0; 32];
        for (byte, value) in tail.iter_mut().zip(random.next()) {
            *byte = value as u8;
        }
        tail
    }

    /// Creates `space` with the config of `hashmap:queue` in the runtime of `reducer`.
    async fn create(reducer: &ReducerCore, space: NativeSpace) -> Result<(), InterpreterError> {
        let creation = SpaceCreation {
            space,
            config: SpaceConfig::new(space.sid, "hashmap", "queue", SpaceQualifier::Default)
                .to_par(),
            handle_pattern: bind(Par::default()).0,
            handle_continuation: TaggedContinuation {
                tagged_cont: Some(TaggedCont::ParBody(body(Par::default()))),
                guard: None,
                cost_authority: None,
            },
            random: rand(&space.sid),
            authority: authority(),
        };
        reducer.reified.create_space(native_agent(), creation).await
    }

    // ── I-20: the derived connective_used flags ────────────────────────────────────────────────

    /// A substitution that makes the body of a USE tuple the process `x!(Int)` leaves the flag
    /// of the tuple stale. The hook recognizes the tuple, recomputes the flag and decodes the
    /// canonical image.
    #[tokio::test]
    async fn reified_marker_flags_use_body_with_connective_send_is_canonical() {
        let fixture = detached().await;
        let space = handle(native(1));
        let x = private(7);
        let program = push_process_expr(Par::default(), use_block(space.clone(), bound(0)));
        let process = substituted(
            &fixture.reducer,
            &program,
            &env_of(send(x.clone(), vec![int_connective()])),
        );
        let tuple = sole_tuple(&process);
        assert!(!tuple.connective_used);
        assert!(tuple.ps[2].connective_used);
        assert!(is_evaluated_process_expr(&process.exprs[0]));
        assert_eq!(
            decode_process_tuple(tuple),
            Err(ReifiedDecodeError::NonCanonical {
                marker: Marker::Use
            })
        );
        assert!(!tuple_is_current(tuple));
        let refreshed = refreshed_tuple(tuple);
        assert!(matches!(refreshed, Cow::Owned(_)));
        assert_eq!(
            *refreshed,
            use_block(tuple.ps[1].clone(), tuple.ps[2].clone())
        );
        assert_eq!(
            decode_process_tuple(&refreshed),
            Ok(ProcessConstruct::Use(UseBlock {
                space,
                body: tuple.ps[2].clone(),
            }))
        );
        fixture
            .reducer
            .eval_marker(tuple, &Env::new(), rand(b"use"), &authority())
            .await
            .expect("the hook evaluates the recognized tuple");
        assert_eq!(stored(&fixture.host, &x).await, vec![
            vec![int_connective()]
        ]);
    }

    /// A variable child bound to the connective value `Int` leaves the derived flags stale in
    /// each hook position. Each hook recognizes the canonical construct: the USE and NEWSPACE
    /// tuples decode, and PARAMS and QUERY fail with their own codes, not with `Malformed`.
    #[tokio::test]
    async fn reified_marker_flags_variable_bound_to_connective_is_canonical() {
        let fixture = detached().await;
        let value = int_connective();
        let space = handle(native(2));

        let use_process = substituted(
            &fixture.reducer,
            &push_process_expr(Par::default(), use_block(space.clone(), bound(0))),
            &env_of(value.clone()),
        );
        let tuple = sole_tuple(&use_process);
        assert_eq!(
            decode_process_tuple(tuple),
            Err(ReifiedDecodeError::NonCanonical {
                marker: Marker::Use
            })
        );
        assert_eq!(
            decode_process_tuple(&refreshed_tuple(tuple)),
            Ok(ProcessConstruct::Use(UseBlock {
                space: space.clone(),
                body: value.clone(),
            }))
        );
        fixture
            .reducer
            .eval_marker(tuple, &Env::new(), rand(b"use"), &authority())
            .await
            .expect("an inert connective body evaluates");

        let new = New {
            bind_count: 1,
            p: Some(bound(1)),
            uri: Vec::new(),
            injections: BTreeMap::new(),
            locally_free: Vec::new(),
        };
        let new_process = substituted(
            &fixture.reducer,
            &push_process_expr(
                Par::default(),
                new_space(new, vec![Some(space.clone())]).expect("a NEWSPACE tuple builds"),
            ),
            &env_of(value.clone()),
        );
        let tuple = sole_tuple(&new_process);
        assert!(!tuple.ps[2].connective_used);
        assert_eq!(
            decode_process_tuple(tuple),
            Err(ReifiedDecodeError::NonCanonical {
                marker: Marker::NewSpace
            })
        );
        match decode_process_tuple(&refreshed_tuple(tuple)) {
            Ok(ProcessConstruct::NewSpace(decoded)) => {
                assert_eq!(decoded.annotations, vec![Some(space.clone())]);
                assert_eq!(decoded.new.p, Some(value.clone()));
            }
            other => panic!("the refreshed NEWSPACE tuple decodes, got {other:?}"),
        }
        fixture
            .reducer
            .eval_marker(tuple, &Env::new(), rand(b"new"), &authority())
            .await
            .expect("the NEWSPACE tuple evaluates");

        let annotation_process = substituted(
            &fixture.reducer,
            &push_process_expr(
                Par::default(),
                new_space(
                    New {
                        bind_count: 1,
                        p: Some(Par::default()),
                        uri: Vec::new(),
                        injections: BTreeMap::new(),
                        locally_free: Vec::new(),
                    },
                    vec![Some(bound(0))],
                )
                .expect("a NEWSPACE tuple builds"),
            ),
            &env_of(value.clone()),
        );
        let tuple = sole_tuple(&annotation_process);
        assert!(decode_process_tuple(tuple).is_err());
        assert!(decode_process_tuple(&refreshed_tuple(tuple)).is_ok());
        assert_eq!(
            code(
                fixture
                    .reducer
                    .eval_marker(tuple, &Env::new(), rand(b"new"), &authority())
                    .await
            ),
            expected("AgentSpacesUnavailable")
        );

        let params = send_params(vec![SendParam::Named("k".to_string(), bound(0))])
            .expect("named params build")
            .expect("params are not empty");
        let env = env_of(value.clone());
        let evaluated = fixture
            .reducer
            .eval_expr(&params, &env)
            .expect("the params evaluate");
        let mut data = vec![fixture
            .reducer
            .substitute
            .substitute_and_charge(&evaluated, 0, &env)
            .expect("the params substitute")];
        assert_eq!(
            split_trailing_params(&data),
            Err(ReifiedDecodeError::NonCanonical {
                marker: Marker::Params
            })
        );
        assert_eq!(
            trailing_params(&mut data).expect("the refreshed params decode"),
            Some(SendParams {
                positional: Vec::new(),
                named: BTreeMap::from([("k".to_string(), value.clone())]),
            })
        );
        let send_term = SendProcess {
            chan: Some(string("rrs-4c-params")),
            data: vec![int(1), params],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        };
        assert_eq!(
            code(
                fixture
                    .reducer
                    .eval_send(&send_term, &env, rand(b"send"), &authority())
                    .await
            ),
            expected("ParamsUnsupported")
        );

        let source =
            query_spec(string("rrs-4c-query"), vec![bound(0)], Vec::new()).expect("a query builds");
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: vec![new_freevar_par(0, Vec::new())],
                source: Some(source),
                remainder: None,
                free_count: 1,
                cost_signature: None,
            }],
            body: Some(Par::default()),
            persistent: false,
            peek: false,
            bind_count: 1,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
        };
        assert_eq!(
            code(
                fixture
                    .reducer
                    .eval_receive(&receive, &env, rand(b"receive"), &authority())
                    .await
            ),
            expected("QuerySpecUnsupported")
        );
    }

    /// The refresh changes only the flags that the encoder derives. A current tuple is not
    /// copied, the values keep their flags, and a tuple of another marker, or without a marker
    /// head, keeps its bytes.
    #[tokio::test]
    async fn reified_marker_flags_change_derived_flags_only() {
        let fixture = detached().await;
        let space = handle(native(3));
        let canonical = [
            use_block(space.clone(), send(private(1), vec![int_connective()])),
            new_space(
                New {
                    bind_count: 2,
                    p: Some(send(private(2), vec![bound(0)])),
                    uri: Vec::new(),
                    injections: BTreeMap::new(),
                    locally_free: Vec::new(),
                },
                vec![Some(space.clone()), None],
            )
            .expect("a NEWSPACE tuple builds"),
            sole_tuple(
                &send_params(vec![
                    SendParam::Positional(int_connective()),
                    SendParam::Named("k".to_string(), int(1)),
                ])
                .expect("params build")
                .expect("params are not empty"),
            )
            .clone(),
            sole_tuple(
                &query_spec(string("rrs-4c"), vec![int_connective()], vec![
                    SendParam::Named("k".to_string(), int(2)),
                ])
                .expect("a query builds"),
            )
            .clone(),
        ];
        let env = env_of(int_connective());
        let mut samples = Vec::with_capacity(2 * canonical.len() + 2);
        for tuple in &canonical {
            assert!(tuple_is_current(tuple));
            assert!(matches!(refreshed_tuple(tuple), Cow::Borrowed(_)));
            samples.push(tuple.clone());
            let mut stale = tuple.clone();
            stale.connective_used = !stale.connective_used;
            samples.push(stale);
        }
        let stale_use = substituted(
            &fixture.reducer,
            &push_process_expr(Par::default(), use_block(space.clone(), bound(0))),
            &env,
        );
        samples.push(sole_tuple(&stale_use).clone());
        let plain = sole_tuple(&new_etuple_par(vec![int(1), int_connective()])).clone();
        samples.push(plain.clone());
        for tuple in samples {
            let mut refreshed = tuple.clone();
            refresh_tuple(&mut refreshed);
            assert_eq!(tuple_is_current(&tuple), refreshed == tuple, "{tuple:?}");
            assert_eq!(refreshed.ps.len(), tuple.ps.len());
            if let Some(fields) = tuple_fields(&tuple) {
                for (index, (before, after)) in tuple.ps.iter().zip(&refreshed.ps).enumerate() {
                    if field_at(fields, index) == Field::Value {
                        assert_eq!(before, after);
                    }
                }
            }
        }
        assert!(tuple_is_current(&plain));
        let mut forged_plain = plain.clone();
        forged_plain.connective_used = false;
        assert!(tuple_is_current(&forged_plain));

        let inert = stale_use.clone();
        let mut data = vec![int(1), inert.clone()];
        assert_eq!(trailing_params(&mut data), Ok(None));
        assert_eq!(data[1].encode_to_vec(), inert.encode_to_vec());
        assert!(matches!(
            refreshed_marker_par(&inert, Marker::Query),
            Cow::Borrowed(_)
        ));
        let mut flagged_head = sole_tuple(&inert).clone();
        flagged_head.ps[0].connective_used = true;
        assert!(tuple_is_current(&flagged_head));
    }

    // ── H3: sends ──────────────────────────────────────────────────────────────────────────────

    /// On terms without Reified syntax, the hooks equal the HEAD methods. They make the same
    /// data, the same continuations and the same names with the same random draws.
    #[tokio::test]
    async fn reducer_space_routing_hooks_match_head_on_ordinary_terms() {
        let hooked = detached().await;
        let head = detached().await;
        for (channel, persistent) in [(private(9), false), (string("rrs-4c-forged"), true)] {
            hooked
                .reducer
                .reified_produce(
                    channel.clone(),
                    datum(vec![int(1)]),
                    persistent,
                    authority(),
                )
                .await
                .expect("an ordinary send runs");
            head.reducer
                .produce(
                    channel.clone(),
                    datum(vec![int(1)]),
                    persistent,
                    authority(),
                )
                .await
                .expect("the HEAD send runs");
            assert_eq!(
                hooked.host.get_data(&channel).await,
                head.host.get_data(&channel).await
            );
        }
        let channels = vec![private(10), string("rrs-4c-join")];
        let binds: Vec<(BindPattern, Par)> = channels.iter().cloned().map(bind).collect();
        let stored_body = body(send(private(11), vec![bound(0)]));
        hooked
            .reducer
            .reified_consume(
                binds.clone(),
                stored_body.clone(),
                false,
                true,
                None,
                authority(),
                authority(),
            )
            .await
            .expect("an ordinary receive runs");
        head.reducer
            .consume(
                binds,
                stored_body,
                false,
                true,
                None,
                authority(),
                authority(),
            )
            .await
            .expect("the HEAD receive runs");
        assert_eq!(
            hooked
                .host
                .get_waiting_continuations(channels.clone())
                .await,
            head.host.get_waiting_continuations(channels).await
        );
        let new = New {
            bind_count: 3,
            p: Some(Par::default()),
            uri: vec!["rho:test:reified".to_string()],
            injections: BTreeMap::from([("rho:test:reified".to_string(), int(5))]),
            locally_free: Vec::new(),
        };
        let env = env_of(int(0));
        let mut hooked_rand = rand(b"new");
        let mut head_rand = rand(b"new");
        let hooked_env = hooked
            .reducer
            .reified_allocate(&new, &env, &mut hooked_rand)
            .expect("the names are allocated");
        let head_env = allocate_new_bindings(&new, &env, &mut head_rand, &head.reducer.urn_map)
            .expect("the HEAD names are allocated");
        assert_eq!(hooked_env.env_map, head_env.env_map);
        assert_eq!(hooked_env.level, head_env.level);
        assert_eq!(hooked_rand.next(), head_rand.next());
    }

    /// A persistent send to an op channel that takes calls, or to a handle, aborts (A21). The
    /// same send without persistence runs, and so does a persistent send to another name.
    #[tokio::test]
    async fn reducer_space_routing_persistent_calls_abort() {
        let fixture = detached().await;
        let space = native(4);
        let ephemeral = NativeSpace {
            durability: Durability::Ephemeral,
            ..space
        };
        for (channel, abort) in [
            (
                namespace::gprivate(namespace::op_channel(0x01)),
                "PersistentFactoryCall",
            ),
            (
                namespace::gprivate(namespace::op_channel(GET_SPACE_AGENT)),
                "PersistentOpCall",
            ),
            (
                namespace::gprivate(namespace::op_channel(HANDLE_OP)),
                "PersistentOpCall",
            ),
            (handle(space), "PersistentHandleCall"),
            (handle(ephemeral), "PersistentHandleCall"),
        ] {
            let call = datum(vec![private(1)]);
            assert_eq!(
                code(
                    fixture
                        .reducer
                        .reified_produce(channel.clone(), call.clone(), true, authority())
                        .await
                ),
                expected(abort)
            );
            assert!(fixture.host.get_data(&channel).await.is_empty());
            fixture
                .reducer
                .reified_produce(channel.clone(), call, false, authority())
                .await
                .expect("a call without persistence runs");
            assert_eq!(fixture.host.get_data(&channel).await.len(), 1);
        }
        for channel in [Marker::Use.par(), private(2), string("rrs-4c-persistent")] {
            fixture
                .reducer
                .reified_produce(channel.clone(), datum(vec![int(1)]), true, authority())
                .await
                .expect("a persistent send to another name runs");
            assert_eq!(fixture.host.get_data(&channel).await.len(), 1);
        }
    }

    /// A factory call gets its constructor as one list, and a handle call keeps its named params.
    /// Every other default channel rejects params, and a queue collection takes none.
    #[tokio::test]
    async fn reducer_space_routing_send_params_follow_the_route() {
        let fixture = detached().await;
        let named = send_params(vec![SendParam::Named("k".to_string(), int(1))])
            .expect("named params build")
            .expect("params are not empty");
        let positional = send_params(vec![SendParam::Positional(int(2))])
            .expect("positional params build")
            .expect("params are not empty");
        let ret = private(3);
        let send_to = |channel: Par, pars: Vec<Par>| {
            fixture
                .reducer
                .reified_produce(channel, datum(pars), false, authority())
        };

        let factory = namespace::gprivate(namespace::op_channel(0x01));
        send_to(factory.clone(), vec![
            ret.clone(),
            string("default"),
            named.clone(),
        ])
        .await
        .expect("a factory call runs");
        assert_eq!(stored(&fixture.host, &factory).await, vec![vec![
            ret.clone(),
            list(vec![string("default"), named.clone()]),
        ]]);
        let bare = namespace::gprivate(namespace::op_channel(0x02));
        send_to(bare.clone(), vec![ret.clone()])
            .await
            .expect("a factory call without a constructor runs");
        assert_eq!(stored(&fixture.host, &bare).await, vec![vec![
            ret.clone(),
            list(Vec::new()),
        ]]);
        assert_eq!(
            code(send_to(factory.clone(), vec![ret.clone(), positional.clone()]).await),
            expected("PositionalParamsUnsupported")
        );
        assert_eq!(fixture.host.get_data(&factory).await.len(), 1);

        let this = handle(native(5));
        let call = vec![ret.clone(), string("produce"), int(4), named.clone()];
        send_to(this.clone(), call.clone())
            .await
            .expect("a handle call runs");
        assert_eq!(stored(&fixture.host, &this).await, vec![call]);
        assert_eq!(
            code(send_to(this.clone(), vec![ret.clone(), positional.clone()]).await),
            expected("PositionalParamsUnsupported")
        );

        for channel in [
            private(4),
            string("rrs-4c-params"),
            namespace::gprivate(namespace::op_channel(GET_SPACE_AGENT)),
            Marker::Use.par(),
        ] {
            assert_eq!(
                code(send_to(channel.clone(), vec![int(1), named.clone()]).await),
                expected("ParamsUnsupported")
            );
            assert!(fixture.host.get_data(&channel).await.is_empty());
        }

        let name = space_name(native(6), [1; 32]);
        assert_eq!(
            code(send_to(name.clone(), vec![int(1), positional.clone()]).await),
            expected("PositionalParamsUnsupported")
        );
        match send_to(name, vec![int(1), named.clone()]).await {
            Err(error) => assert_eq!(
                error,
                InterpreterError::from(SpaceError::UnknownParam {
                    key: "k".to_string(),
                    suggestion: None,
                })
            ),
            Ok(_) => panic!("a queue collection took a named param"),
        }

        let erased = new_etuple_par(vec![
            Marker::Params.par(),
            list(Vec::new()),
            new_emap_par(Vec::new(), Vec::new(), false, None, Vec::new(), false),
        ]);
        assert_eq!(
            code(send_to(private(5), vec![erased]).await),
            expected("Malformed")
        );
    }

    /// A space name routes to its space in every context, and a forged channel routes to the
    /// space of the use-context. The operation fails closed without a host runtime, and with a
    /// host runtime it reaches the agent, which reports the missing space.
    #[tokio::test]
    async fn reducer_space_routing_space_routes_reach_the_runtime() {
        let fixture = detached().await;
        let space = native(7);
        let name = space_name(space, [1; 32]);
        let forged = string("rrs-4c-forged");
        let core = scoped(&fixture.reducer, space);
        assert_eq!(
            code(
                fixture
                    .reducer
                    .reified_produce(name.clone(), datum(vec![int(1)]), false, authority())
                    .await
            ),
            expected("RuntimeUnavailable")
        );
        assert_eq!(
            code(
                fixture
                    .reducer
                    .reified_consume(
                        vec![bind(name.clone())],
                        body(Par::default()),
                        true,
                        false,
                        None,
                        authority(),
                        authority(),
                    )
                    .await
            ),
            expected("RuntimeUnavailable")
        );
        assert_eq!(
            code(
                core.reified_produce(forged.clone(), datum(vec![int(1)]), true, authority())
                    .await
            ),
            expected("RuntimeUnavailable")
        );
        assert_eq!(
            code(
                core.reified_consume(
                    vec![bind(forged.clone())],
                    body(Par::default()),
                    false,
                    true,
                    None,
                    authority(),
                    authority(),
                )
                .await
            ),
            expected("RuntimeUnavailable")
        );
        for channel in [&name, &forged] {
            assert!(fixture.host.get_data(channel).await.is_empty());
            assert!(fixture
                .host
                .get_waiting_continuations(vec![channel.clone()])
                .await
                .is_empty());
        }
        let mut replay = ReducerCore::clone(&fixture.reducer);
        replay.reified = native_replay();
        assert_eq!(
            code(
                replay
                    .reified_produce(name.clone(), datum(vec![int(1)]), false, authority())
                    .await
            ),
            expected("SpacesUnsupportedInNativeExecution")
        );

        let hosted = hosted().await;
        assert_eq!(
            code(
                hosted
                    .reducer
                    .reified_produce(name.clone(), datum(vec![int(1)]), false, authority())
                    .await
            ),
            expected("DanglingName")
        );
        assert_eq!(
            code(
                hosted
                    .reducer
                    .reified_consume(
                        vec![bind(name.clone())],
                        body(Par::default()),
                        false,
                        false,
                        None,
                        authority(),
                        authority(),
                    )
                    .await
            ),
            expected("DanglingName")
        );
        assert_eq!(
            code(
                scoped(&hosted.reducer, space)
                    .reified_produce(forged.clone(), datum(vec![int(1)]), false, authority())
                    .await
            ),
            expected("DanglingHandle")
        );
        assert!(hosted.host.get_data(&name).await.is_empty());
    }

    /// An agent-tagged name fails closed in a send, a receive and a join until P8.
    #[tokio::test]
    async fn reducer_space_routing_agent_names_fail_closed() {
        let fixture = detached().await;
        let tagged = new_etuple_par(vec![
            namespace::gprivate(namespace::op_channel(AGENT_TAG)),
            string("agent"),
            int(1),
        ]);
        assert_eq!(
            code(
                fixture
                    .reducer
                    .reified_produce(tagged.clone(), datum(vec![int(1)]), false, authority())
                    .await
            ),
            expected("AgentSpacesUnavailable")
        );
        for channels in [
            vec![tagged.clone()],
            vec![tagged.clone(), private(1)],
            vec![space_name(native(8), [2; 32]), tagged.clone()],
        ] {
            assert_eq!(
                code(
                    fixture
                        .reducer
                        .reified_consume(
                            channels.into_iter().map(bind).collect(),
                            body(Par::default()),
                            false,
                            false,
                            None,
                            authority(),
                            authority(),
                        )
                        .await
                ),
                expected("AgentSpacesUnavailable")
            );
        }
        assert!(fixture.host.get_data(&tagged).await.is_empty());
    }

    // ── Receives ───────────────────────────────────────────────────────────────────────────────

    /// A receive with one bind for each channel and a `Nil` body.
    async fn receive(
        core: &ReducerCore,
        channels: Vec<Par>,
    ) -> Result<DispatchType, InterpreterError> {
        core.reified_consume(
            channels.into_iter().map(bind).collect(),
            body(Par::default()),
            false,
            false,
            None,
            authority(),
            authority(),
        )
        .await
    }

    /// A join with a channel on a native space fails closed: one space gives
    /// `OrderedJoinUnsupported` (A3), and every other mix gives `CrossSpaceJoin`.
    #[tokio::test]
    async fn reducer_space_routing_receive_joins_route_by_space() {
        let fixture = detached().await;
        let first = native(9);
        let second = native(10);
        let name = |space, seed| space_name(space, [seed; 32]);
        let reducer: &ReducerCore = &fixture.reducer;
        for (channels, failure) in [
            (vec![name(first, 1), private(1)], "CrossSpaceJoin"),
            (vec![private(1), name(first, 1)], "CrossSpaceJoin"),
            (
                vec![name(first, 1), name(first, 2)],
                "OrderedJoinUnsupported",
            ),
            (
                vec![name(first, 1), name(first, 1)],
                "OrderedJoinUnsupported",
            ),
            (vec![name(first, 1), name(second, 1)], "CrossSpaceJoin"),
        ] {
            assert_eq!(code(receive(reducer, channels).await), expected(failure));
        }
        let core = scoped(reducer, first);
        for (channels, failure) in [
            (
                vec![string("rrs-4c-a"), string("rrs-4c-b")],
                "OrderedJoinUnsupported",
            ),
            (
                vec![string("rrs-4c-a"), name(first, 3)],
                "OrderedJoinUnsupported",
            ),
            (vec![string("rrs-4c-a"), private(2)], "CrossSpaceJoin"),
            (vec![string("rrs-4c-a"), name(second, 3)], "CrossSpaceJoin"),
        ] {
            assert_eq!(code(receive(&core, channels).await), expected(failure));
        }
        for channel in [private(1), private(2), string("rrs-4c-a")] {
            assert!(fixture
                .host
                .get_waiting_continuations(vec![channel])
                .await
                .is_empty());
        }
    }

    /// Under a use-context, a receive on the default space stores its body inside the USE
    /// tuple of the context. The stored body keeps its random state.
    #[tokio::test]
    async fn reducer_space_routing_use_context_wraps_stored_bodies() {
        let fixture = detached().await;
        let core = scoped(&fixture.reducer, native(11));
        let channel = private(12);
        let original = send(private(13), vec![bound(0)]);
        core.reified_consume(
            vec![bind(channel.clone())],
            body(original.clone()),
            true,
            false,
            None,
            authority(),
            authority(),
        )
        .await
        .expect("the receive parks");
        let waiting = fixture.host.get_waiting_continuations(vec![channel]).await;
        let [waiter] = waiting.as_slice() else {
            panic!("the channel holds {} continuations", waiting.len());
        };
        assert!(waiter.persist);
        match &waiter.continuation.tagged_cont {
            Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(stored),
                random_state,
            })) => {
                assert_eq!(*stored, wrap_body(&core.space_ctx, original.clone()));
                assert_ne!(*stored, original);
                assert_eq!(*random_state, body(Par::default()).random_state);
            }
            other => panic!("the continuation is {other:?}"),
        }
    }

    // ── Names ──────────────────────────────────────────────────────────────────────────────────

    /// Under a use-context each plain binder of `new` becomes a name of the context space with
    /// the random draws of HEAD. URI binders keep their values.
    #[tokio::test]
    async fn reducer_space_routing_allocate_stamps_plain_binders_in_context() {
        let fixture = detached().await;
        let space = native(14);
        let new = New {
            bind_count: 3,
            p: Some(Par::default()),
            uri: vec!["rho:test:reified".to_string()],
            injections: BTreeMap::from([("rho:test:reified".to_string(), int(5))]),
            locally_free: Vec::new(),
        };
        let env = env_of(int(0));
        let mut head_rand = rand(b"stamp");
        let head = allocate_new_bindings(&new, &env, &mut head_rand, &fixture.reducer.urn_map)
            .expect("the HEAD names are allocated");
        let mut stamped_rand = rand(b"stamp");
        let stamped = scoped(&fixture.reducer, space)
            .reified_allocate(&new, &env, &mut stamped_rand)
            .expect("the names are allocated");
        let mut draws = rand(b"stamp");
        for offset in 0..2 {
            let level = env.level + offset;
            let tail = next_tail(&mut draws);
            assert_eq!(
                head.env_map.get(&level),
                Some(&namespace::gprivate(tail.to_vec()))
            );
            assert_eq!(stamped.env_map.get(&level), Some(&space_name(space, tail)));
        }
        assert_eq!(
            stamped.env_map.get(&(env.level + 2)),
            head.env_map.get(&(env.level + 2))
        );
        assert_eq!(stamped.level, head.level);
        assert_eq!(stamped_rand.next(), head_rand.next());
        let mut negative = new.clone();
        negative.bind_count = 0;
        assert_eq!(
            scoped(&fixture.reducer, space)
                .reified_allocate(&negative, &env, &mut rand(b"stamp"))
                .err(),
            allocate_new_bindings(
                &negative,
                &env,
                &mut rand(b"stamp"),
                &fixture.reducer.urn_map
            )
            .err()
        );
    }

    // ── H2: USE and NEWSPACE ───────────────────────────────────────────────────────────────────

    /// `use s` takes a bare or writable handle, and evaluates its body in the context of the
    /// space. Every other reference fails closed with the code of `classify_space_ref`, and a
    /// value that is not a Reified id is a user agent (P8).
    #[tokio::test]
    async fn reducer_handle_unwrap_use_classifies_its_space() {
        let fixture = detached().await;
        let space = native(15);
        let forged = string("rrs-4c-use");
        let use_body = send(forged.clone(), vec![int(1)]);
        for (reference, result) in [
            (Par::default(), "EmptySpaceRef"),
            (
                new_bundle_par(handle(space), false, true),
                "HandleNotWritable",
            ),
            (
                namespace::gprivate(namespace::config_channel(space)),
                "NotASpaceRef",
            ),
            (
                namespace::gprivate(namespace::op_channel(0x01)),
                "NotASpaceRef",
            ),
            (string("agent"), "AgentSpacesUnavailable"),
            (handle(space), "RuntimeUnavailable"),
            (
                new_bundle_par(handle(space), true, false),
                "RuntimeUnavailable",
            ),
        ] {
            let tuple = use_block(reference, use_body.clone());
            assert_eq!(
                code(
                    fixture
                        .reducer
                        .eval_marker(&tuple, &Env::new(), rand(b"use"), &authority())
                        .await
                ),
                expected(result)
            );
        }
        assert!(fixture.host.get_data(&forged).await.is_empty());
        let x = private(16);
        fixture
            .reducer
            .eval_marker(
                &use_block(handle(space), send(x.clone(), vec![int(2)])),
                &Env::new(),
                rand(b"use"),
                &authority(),
            )
            .await
            .expect("a default channel in a use block reaches the host");
        assert_eq!(stored(&fixture.host, &x).await, vec![vec![int(2)]]);
        let short = sole_tuple(&new_etuple_par(vec![Marker::Use.par(), int(1)])).clone();
        assert_eq!(
            code(
                fixture
                    .reducer
                    .eval_marker(&short, &Env::new(), rand(b"use"), &authority())
                    .await
            ),
            expected("Malformed")
        );
    }

    /// NEWSPACE moves each annotated binder into its space, and each other binder into the
    /// space of the use-context, or leaves it in the default space. The names use the random
    /// draws of `new`.
    #[tokio::test]
    async fn reducer_space_routing_newspace_moves_annotated_binders() {
        let fixture = detached().await;
        let annotated = native(17);
        let context = native(18);
        let tuple_to = |out: &Par, annotation: Par| {
            new_space(
                New {
                    bind_count: 2,
                    p: Some(send(out.clone(), vec![bound(1), bound(0)])),
                    uri: Vec::new(),
                    injections: BTreeMap::new(),
                    locally_free: Vec::new(),
                },
                vec![Some(annotation), None],
            )
            .expect("a NEWSPACE tuple builds")
        };
        let mut draws = rand(b"newspace");
        let first = next_tail(&mut draws);
        let second = next_tail(&mut draws);

        let out = private(19);
        fixture
            .reducer
            .eval_marker(
                &tuple_to(&out, handle(annotated)),
                &Env::new(),
                rand(b"newspace"),
                &authority(),
            )
            .await
            .expect("the NEWSPACE tuple evaluates");
        assert_eq!(stored(&fixture.host, &out).await, vec![vec![
            space_name(annotated, first),
            namespace::gprivate(second.to_vec()),
        ]]);

        let scoped_out = private(20);
        scoped(&fixture.reducer, context)
            .eval_marker(
                &tuple_to(&scoped_out, new_bundle_par(handle(annotated), true, false)),
                &Env::new(),
                rand(b"newspace"),
                &authority(),
            )
            .await
            .expect("the NEWSPACE tuple evaluates in a context");
        assert_eq!(stored(&fixture.host, &scoped_out).await, vec![vec![
            space_name(annotated, first),
            space_name(context, second),
        ]]);

        let rejected = private(21);
        for (annotation, failure) in [
            (string("agent"), "AgentSpacesUnavailable"),
            (Par::default(), "EmptySpaceRef"),
            (
                new_bundle_par(handle(annotated), false, true),
                "HandleNotWritable",
            ),
        ] {
            assert_eq!(
                code(
                    fixture
                        .reducer
                        .eval_marker(
                            &tuple_to(&rejected, annotation),
                            &Env::new(),
                            rand(b"newspace"),
                            &authority(),
                        )
                        .await
                ),
                expected(failure)
            );
        }
        assert!(fixture.host.get_data(&rejected).await.is_empty());
    }

    // ── H4: query sources ──────────────────────────────────────────────────────────────────────

    /// A QUERY source fails closed with `QuerySpecUnsupported`, a malformed QUERY tuple with
    /// `Malformed`, and every other source passes.
    #[test]
    fn reducer_space_routing_query_sources_fail_closed() {
        let query =
            query_spec(string("rrs-4c-query"), vec![int(1)], Vec::new()).expect("a query builds");
        assert_eq!(
            code(reject_query_source(&query)),
            expected("QuerySpecUnsupported")
        );
        let mut stale = query.clone();
        stale.connective_used = true;
        assert_eq!(
            code(reject_query_source(&stale)),
            expected("QuerySpecUnsupported")
        );
        let malformed = new_etuple_par(vec![Marker::Query.par(), int(1), list(vec![int(2)])]);
        assert_eq!(code(reject_query_source(&malformed)), expected("Malformed"));
        for source in [
            private(1),
            string("rrs-4c"),
            new_etuple_par(vec![int(1), int(2)]),
            push_process_expr(Par::default(), use_block(handle(native(1)), Par::default())),
        ] {
            assert_eq!(reject_query_source(&source), Ok(()));
        }
    }

    // ── Dispatch of space outcomes ─────────────────────────────────────────────────────────────

    fn fire_with(body_par: Par, value: i64) -> SpaceFire {
        SpaceFire {
            continuation: TaggedContinuation {
                tagged_cont: Some(TaggedCont::ParBody(body(body_par))),
                guard: None,
                cost_authority: None,
            },
            results: vec![RSpaceResult {
                channel: string("rrs-4c-collection"),
                matched_datum: datum(vec![int(value)]),
                removed_datum: datum(vec![int(value)]),
                persistent: false,
            }],
        }
    }

    /// A fire whose body sends the matched value on `out`.
    fn fire_to(out: &Par, value: i64) -> SpaceFire {
        fire_with(send(out.clone(), vec![bound(0)]), value)
    }

    /// Two fires whose bodies fail with different errors: a send on a read-only bundle, then an
    /// unbound variable.
    fn failing_fires() -> Vec<SpaceFire> {
        vec![
            fire_with(
                send(
                    new_bundle_par(string("rrs-4c-read-only"), false, true),
                    vec![int(1)],
                ),
                1,
            ),
            fire_with(send(string("rrs-4c-unbound"), vec![bound(5)]), 2),
        ]
    }

    fn assert_failures_in_index_order(result: Result<DispatchType, InterpreterError>) {
        match result {
            Err(InterpreterError::AggregateError { interpreter_errors }) => {
                match interpreter_errors.as_slice() {
                    [InterpreterError::ReduceError(first), InterpreterError::ReduceError(second)] =>
                    {
                        assert!(first.contains("non-writeable"), "{first}");
                        assert!(second.starts_with("Unbound variable"), "{second}");
                    }
                    other => panic!("the failures are {other:?}"),
                }
            }
            Err(other) => panic!("the dispatch failed with {other:?}"),
            Ok(_) => panic!("failing fires dispatched without an error"),
        }
    }

    /// Runs `step` as one root evaluation and returns its result. A dispatch ends the
    /// participant that makes it, so a test that runs several operations runs each one in its
    /// own root, as each deploy runs in its own root. As `inj` does, the step begins a new
    /// evaluation scope of the spaces. The detached reductions must not fail.
    async fn in_root<T>(reducer: &ReducerCore, step: impl Future<Output = T>) -> T {
        reducer.reified.begin_evaluation_scope();
        let (result, _, errors) = root_with_observation(
            reducer.space.clone(),
            reducer.metering.budget(),
            reducer.reduction_coordinator.clone(),
            None,
            step,
        )
        .await;
        assert!(
            errors.is_empty(),
            "the detached reductions failed: {errors:?}"
        );
        result
    }

    /// A branch `then` that records the number of data on `channel` when it runs.
    fn observing_then(
        host: &ParRSpace,
        channel: &Par,
        seen: &Arc<std::sync::Mutex<Option<usize>>>,
    ) -> DispatchFuture<'static> {
        let host = host.clone();
        let channel = channel.clone();
        let seen = Arc::clone(seen);
        Box::pin(async move {
            let count = host.get_data(&channel).await.len();
            *seen.lock().expect("the observation lock") = Some(count);
            Ok(DispatchType::Skip)
        })
    }

    /// A branch `then` that sends `value` on `channel` through the reducer: an operation after
    /// the dispatch of the fires.
    fn sending_then(
        reducer: &Arc<ReducerCore>,
        channel: &Par,
        value: i64,
    ) -> DispatchFuture<'static> {
        let reducer = Arc::clone(reducer);
        let channel = channel.clone();
        Box::pin(async move {
            reducer
                .reified_produce(channel, datum(vec![int(value)]), false, authority())
                .await
        })
    }

    /// Readiness review H6: without a reduction context the branches run one after another in
    /// index order, and their errors aggregate in index order.
    #[tokio::test]
    async fn reducer_space_routing_dispatch_without_a_context_runs_in_index_order() {
        let fixture = detached().await;
        assert!(deterministic_reduction::current().is_none());
        let out = private(30);
        fixture
            .reducer
            .dispatch_space_outcome(
                SpaceOutcome {
                    fires: vec![fire_to(&out, 1), fire_to(&out, 2)],
                    reissue: None,
                },
                None,
            )
            .await
            .expect("both fires dispatch");
        assert_eq!(stored(&fixture.host, &out).await, vec![vec![int(1)], vec![
            int(2)
        ]]);
        assert_failures_in_index_order(
            fixture
                .reducer
                .dispatch_space_outcome(
                    SpaceOutcome {
                        fires: failing_fires(),
                        reissue: None,
                    },
                    None,
                )
                .await,
        );
        let single = private(31);
        fixture
            .reducer
            .dispatch_space_outcome(
                SpaceOutcome {
                    fires: vec![fire_to(&single, 3)],
                    reissue: None,
                },
                None,
            )
            .await
            .expect("one fire dispatches directly");
        assert_eq!(stored(&fixture.host, &single).await, vec![vec![int(3)]]);
        assert!(matches!(
            fixture
                .reducer
                .dispatch_space_outcome(SpaceOutcome::default(), None)
                .await,
            Ok(DispatchType::Skip)
        ));
        let after = private(35);
        let seen = Arc::new(std::sync::Mutex::new(None));
        fixture
            .reducer
            .dispatch_space_outcome(
                SpaceOutcome {
                    fires: vec![fire_to(&after, 10), fire_to(&after, 11)],
                    reissue: None,
                },
                Some(observing_then(&fixture.host, &after, &seen)),
            )
            .await
            .expect("the fires and then run");
        assert_eq!(*seen.lock().expect("the observation lock"), Some(2));
    }

    /// Negative control of H6: `run_parallel_dispatches` without a reduction context panics, so
    /// `dispatch_space_outcome` must not call it there.
    #[tokio::test]
    #[should_panic(expected = "parallel dispatch requires a reduction context")]
    async fn reducer_space_routing_parallel_dispatch_needs_a_reduction_context() {
        let fixture = detached().await;
        let branch: DispatchFuture<'static> = Box::pin(async { Ok(DispatchType::Skip) });
        let _ = fixture.reducer.run_parallel_dispatches(vec![branch]).await;
    }

    /// In a reduction the branches run as parallel participants. The dispatch returns when it
    /// has started them, and the errors of the branches reach the root in index order. Each
    /// dispatch runs in its own root, because a dispatch ends the participant that makes it.
    #[tokio::test]
    async fn reducer_space_routing_dispatch_in_a_reduction_runs_parallel_branches() {
        let fixture = detached().await;
        let reducer: &ReducerCore = &fixture.reducer;
        let out = private(32);
        let dispatched = in_root(reducer, async {
            assert!(deterministic_reduction::current().is_some());
            reducer
                .dispatch_space_outcome(
                    SpaceOutcome {
                        fires: vec![fire_to(&out, 4), fire_to(&out, 5)],
                        reissue: None,
                    },
                    None,
                )
                .await
        })
        .await;
        assert!(matches!(dispatched, Ok(DispatchType::Skip)));
        assert_eq!(stored(&fixture.host, &out).await, vec![vec![int(4)], vec![
            int(5)
        ]]);
        let (failed, _, errors) = root_with_observation(
            reducer.space.clone(),
            reducer.metering.budget(),
            reducer.reduction_coordinator.clone(),
            None,
            reducer.dispatch_space_outcome(
                SpaceOutcome {
                    fires: failing_fires(),
                    reissue: None,
                },
                None,
            ),
        )
        .await;
        assert!(matches!(failed, Ok(DispatchType::Skip)));
        assert_failures_in_index_order(
            reducer
                .finish_detached_errors(Ok(()), errors)
                .map(|()| DispatchType::Skip),
        );
    }

    /// Regression of the merge with the stack-safe reducer: an operation after a dispatch in one
    /// participant fails, because the dispatch moved the participant to the body. So `then` runs
    /// as the last branch, in its own participant. Its operation succeeds, and without fires and
    /// re-issue `then` runs directly.
    #[tokio::test]
    async fn reducer_space_routing_then_runs_after_the_fires_in_its_own_branch() {
        let fixture = detached().await;
        let reducer: &ReducerCore = &fixture.reducer;
        let core = Arc::clone(&fixture.reducer.core);
        let out = private(36);
        for fires in [vec![fire_to(&out, 12)], vec![
            fire_to(&out, 13),
            fire_to(&out, 14),
        ]] {
            let count = fires.len();
            let dispatched = in_root(
                reducer,
                reducer.dispatch_space_outcome(
                    SpaceOutcome {
                        fires,
                        reissue: None,
                    },
                    Some(sending_then(&core, &out, 15 + count as i64)),
                ),
            )
            .await;
            assert!(matches!(dispatched, Ok(DispatchType::Skip)));
        }
        assert_eq!(stored(&fixture.host, &out).await, vec![
            vec![int(12)],
            vec![int(13)],
            vec![int(14)],
            vec![int(16)],
            vec![int(17)]
        ]);
        let alone = private(37);
        let direct = in_root(
            reducer,
            reducer.dispatch_space_outcome(
                SpaceOutcome::default(),
                Some(sending_then(&core, &alone, 18)),
            ),
        )
        .await;
        assert!(matches!(direct, Ok(DispatchType::Skip)));
        assert_eq!(stored(&fixture.host, &alone).await, vec![vec![int(18)]]);
    }

    /// The re-issue of an outcome is the last branch, a new space operation. In a runtime
    /// without spaces it fails closed after the fire has dispatched.
    #[tokio::test]
    async fn reducer_space_routing_dispatch_runs_the_reissue_last() {
        let fixture = detached().await;
        let out = private(33);
        let space = native(34);
        let name = space_name(space, [3; 32]);
        let reissue = SpaceReissue {
            op: SpaceOp::Produce(ProduceOp {
                space,
                key: CollectionKey::of_channel(&name).expect("a key"),
                channel: name,
                data: datum(vec![int(6)]),
                persistent: true,
                authority: authority(),
                reissued_ord: Some(0),
            }),
            cause: ReissueCause::DivergentPair,
        };
        for reissue_only in [false, true] {
            let fires = match reissue_only {
                true => Vec::new(),
                false => vec![fire_to(&out, 6)],
            };
            assert_eq!(
                code(
                    fixture
                        .reducer
                        .dispatch_space_outcome(
                            SpaceOutcome {
                                fires,
                                reissue: Some(reissue.clone()),
                            },
                            None,
                        )
                        .await
                ),
                expected("RuntimeUnavailable")
            );
        }
        assert_eq!(stored(&fixture.host, &out).await, vec![vec![int(6)]]);
    }

    // ── A native space end to end ──────────────────────────────────────────────────────────────

    /// FR-5': every root entry of the reducer begins an evaluation scope of the spaces, and
    /// `inj_with_host_work` too. Each root evaluation starts at the order of the root
    /// participant, and the session is the deploy of the budget. So the creation and both runs
    /// commit their first transaction under one parent order. Without a new scope, the first run
    /// already fails with `ParentOrderReused`.
    #[tokio::test]
    async fn reducer_space_routing_inj_with_host_work_begins_an_evaluation_scope() {
        use models::rust::host_work::{HostWorkLimit, HostWorkLimits};

        use crate::rust::interpreter::host_work::HostWorkBudget;

        let fixture = hosted().await;
        let reducer: &ReducerCore = &fixture.reducer;
        let space = native(42);
        let name = space_name(space, [5; 32]);
        in_root(reducer, create(reducer, space))
            .await
            .expect("create the space");
        let term = send(name.clone(), vec![int(1)]);
        for run in 0..2 {
            let host_work =
                HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1 << 40)));
            reducer
                .inj_with_host_work(term.clone(), rand(b"host-work"), host_work)
                .await
                .unwrap_or_else(|error| panic!("run {run} of the send failed: {error:?}"));
        }
        let collection = namespace::gprivate(namespace::data_channel(
            space,
            &CollectionKey::of_channel(&name).expect("a key"),
        ));
        assert_eq!(fixture.host.get_data(&collection).await.len(), 2);
    }

    /// With a host runtime, the hooks run the operations of a space name on its collection. A
    /// forged channel in a use block also reaches the space. The fired bodies dispatch.
    #[tokio::test]
    async fn reducer_space_routing_space_operations_fire_through_the_space() {
        let fixture = hosted().await;
        let reducer: &ReducerCore = &fixture.reducer;
        let space = native(40);
        let name = space_name(space, [4; 32]);
        let out = private(41);
        let forged = string("rrs-4c-in-space");
        let waiter = || body(send(out.clone(), vec![bound(0)]));
        in_root(reducer, create(reducer, space))
            .await
            .expect("create the space");
        in_root(
            reducer,
            reducer.reified_consume(
                vec![bind(name.clone())],
                waiter(),
                false,
                false,
                None,
                authority(),
                authority(),
            ),
        )
        .await
        .expect("park the waiter");
        in_root(
            reducer,
            reducer.reified_produce(name.clone(), datum(vec![int(7)]), false, authority()),
        )
        .await
        .expect("fire the waiter");
        in_root(
            reducer,
            reducer.reified_produce(name.clone(), datum(vec![int(8)]), false, authority()),
        )
        .await
        .expect("store the datum");
        in_root(
            reducer,
            reducer.reified_consume(
                vec![bind(name.clone())],
                waiter(),
                false,
                false,
                None,
                authority(),
                authority(),
            ),
        )
        .await
        .expect("take the datum");
        in_root(
            reducer,
            reducer.eval_marker(
                &use_block(handle(space), send(forged.clone(), vec![int(9)])),
                &Env::new(),
                rand(b"use"),
                &authority(),
            ),
        )
        .await
        .expect("send in the use block");
        assert_eq!(stored(&fixture.host, &out).await, vec![vec![int(7)], vec![
            int(8)
        ]]);
        let collection = |channel: &Par| {
            namespace::gprivate(namespace::data_channel(
                space,
                &CollectionKey::of_channel(channel).expect("a key"),
            ))
        };
        let names = collection(&name);
        assert!(fixture.host.get_data(&names).await.is_empty());
        assert!(fixture
            .host
            .get_waiting_continuations(vec![names])
            .await
            .is_empty());
        let records = fixture.host.get_data(&collection(&forged)).await;
        let [record] = records.as_slice() else {
            panic!("the forged collection holds {} records", records.len());
        };
        assert_eq!(
            unwrap_datum(&record.a).expect("a datum record").datum.pars,
            vec![int(9)]
        );
        for channel in [&name, &forged] {
            assert!(fixture.host.get_data(channel).await.is_empty());
        }
    }
}
