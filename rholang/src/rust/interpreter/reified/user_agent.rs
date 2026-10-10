//! User agent-backed spaces (P8, plan decision 77, design v2 W5 and the codecs of TR-5).
//!
//! The FIPS lets any agent with the behavioral type of an RSpace serve as a space (`Reifying
//! RSpaces.md`, lines 109 to 151). The reducer translates each operation on a name of such a
//! space into a method call `A!(return, "method", args…)`, the call shape of the Agents FIPS. A
//! translated receive gives the agent one matcher capability for each bind and one body
//! capability. This module holds the parts of the translation:
//!
//! * [`MatcherState`]: the pattern, the guard and the binder coordinates of one bind. The id
//!   `E0‖X‖state` of a matcher capability holds the canonical encoding of its state (TR-5), so
//!   the capability is self-describing and every node decodes the same state.
//! * [`MatcherProcess`]: the internal op `MATCHER_OP` (MO-1 to MO-4). The forwarder of a matcher
//!   capability sends it `(m, [ret, proc])`. The op matches the elements of `proc` against the
//!   pattern of `m` with the injected matcher of the host, checks the guard of `m`, and replies
//!   `(true, env)` or `(false, Nil)` on `ret`. `env` maps the decimal global position of each
//!   binder of the bind to its value. The reply tuple is the form of an optional result in the
//!   Agents FIPS (`(true, value)` and `(false, …)`).
//! * [`sink`]: the internal op `SINK_OP` (SK-1). The produce and consume calls of a translation
//!   use it as their return channel, because no caller waits for their replies. It takes a
//!   message of any arity and drops it.
//! * [`produce_call`]: the call `A'!(sink, "produce", raw, [d1, …, dk])` of a send on a space
//!   that an agent implements (P8 step 2, TS-1). The reducer hooks build it for a send, and the
//!   R7 backend builds it for the reply of a system process to an agent-minted name (R7A-1).
//! * [`translate_receive`]: the translation T_recv of a receive on a space that an agent
//!   implements (P8 step 3, TR-3 to TR-7). It builds one matcher capability with its persistent
//!   receive for each bind, the body capability with its receive, and the call
//!   `A'!(sink, "consume", [(bundle+{m_i}, raw_i)…], arrow, bundle+{b})`. [`guard_indices`]
//!   and [`place_guard`] give the guard to the bind that owns its binders (TR-4).
//!
//! # Matcher states
//!
//! A state is the canonical `Par` tuple
//! `(version, receive_tag, i, off_i, total, n, (patterns, free_count), guard)`:
//!
//! | Element      | Value                                                                 |
//! |--------------|-----------------------------------------------------------------------|
//! | `version`    | `GInt(1)`                                                             |
//! | `receive_tag`| `GByteArray` of the 32 bytes that every matcher of one receive shares |
//! | `i`          | the index of the bind in the receive                                  |
//! | `off_i`      | the global position of the first binder of the bind (TR-3)            |
//! | `total`      | the number of binders of the receive                                  |
//! | `n`          | the number of binds of the receive                                    |
//! | `patterns`   | an `EList` of the patterns of the bind, with its remainder            |
//! | `free_count` | `GInt` of the free count of the bind                                  |
//! | `guard`      | `[]` without a guard, or `[g]` with the guard `g`                     |
//!
//! A list of zero or one element holds the guard, so a guard that is `Nil` stays distinct from
//! no guard. The bytes of a state are its prost encoding. [`MatcherState::decode`] accepts
//! exactly the image of [`MatcherState::encode`]: a prost decode, the checks of the tuple, and a
//! byte-equal re-encode. prost refuses to decode a message nested deeper than its recursion
//! limit of 100, so the translation rejects such a state before it mints the capability
//! (`AgentPatternTooDeep`, P8 step 3).
//!
//! # Metering
//!
//! Every decode, encode, match, guard check and reply copy reserves its host work on the
//! host-work budget of the deploy before it is made, the same in play and replay (design v2
//! section 11, decision 40). The decode of a matcher state reserves the bytes of the id first
//! (MO-2).

use std::collections::BTreeSet;
use std::mem::size_of;
use std::sync::{Arc, LazyLock};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::create_bit_vector;
use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, Bundle, Connective, ConnectiveBody, CostAuthority, CostSignedTerm, EAnd, EDiv,
    EEq, EGt, EGte, EList, ELt, ELte, EMap, EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult,
    ENeg, ENeq, ENot, EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus, ESet, ETuple, EVar,
    EZipper, Expr, If, KeyValuePair, ListParWithRandom, Match, MatchCase, New, Par, ParWithRandom,
    Receive, ReceiveBind, Send, TaggedContinuation, Var,
};
use models::rust::utils::{
    new_boundvar_par, new_bundle_par, new_freevar_par, new_gbool_par, new_gstring_par, union,
};
use prost::Message;
use rspace_plus_plus::rspace::errors::RSpaceError;
use shared::rust::clone_backing;

use super::costs::ScanMeter;
use super::errors::ReifiedReduceError;
use super::factory::{handle_pattern, ContractArgs};
use super::handle::{is_reply_name, map_par, sole_list, tuple_par};
use super::runtime::ReifiedRuntime;
use super::scope::call_channel;
use super::system_channels::{MATCHER_OP_PROCESS, SINK_OP_PROCESS};
use crate::rust::interpreter::contract_call::ContractCall;
use crate::rust::interpreter::deterministic_reduction;
use crate::rust::interpreter::errors::{illegal_argument_error, InterpreterError};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::matching::SpaceMatcher;
use crate::rust::interpreter::spaces::namespace::{
    self, Durability, RandomPurpose, ReifiedId, SID_LEN, SINK_OP,
};
use crate::rust::interpreter::spaces::record::{expr_par, sole_expr};

/// The version of the encoding of a matcher state.
pub const MATCHER_STATE_VERSION: i64 = 1;

/// The number of elements of the tuple of a matcher state.
const STATE_ELEMENTS: usize = 8;

/// The canonical state of one matcher capability (design v2 TR-5).
#[derive(Clone, Debug, PartialEq)]
pub struct MatcherState {
    /// The tag that every matcher of one translated receive shares: `base.split_byte(2).next()`.
    pub receive_tag: [u8; SID_LEN],
    /// The index `i` of the bind in the receive.
    pub bind: u32,
    /// The global position `off_i` of the first binder of the bind (TR-3).
    pub offset: u32,
    /// The number `total` of binders of the receive.
    pub total: u32,
    /// The number `n` of binds of the receive.
    pub binds: u32,
    /// The pattern of the bind: its patterns, its remainder and its free count.
    pub pattern: BindPattern,
    /// The guard of the receive when this bind owns it (TR-4), or `None`.
    pub guard: Option<Par>,
}

impl MatcherState {
    /// The free count of the bind, or `None` when it is negative.
    pub fn free_count(&self) -> Option<u32> { u32::try_from(self.pattern.free_count).ok() }

    /// True when the coordinates are consistent: the bind is one of the `n` binds, and its
    /// binders lie inside the `total` binders of the receive.
    pub fn is_consistent(&self) -> bool {
        match self.free_count() {
            Some(free) => {
                self.bind < self.binds
                    && self
                        .offset
                        .checked_add(free)
                        .is_some_and(|end| end <= self.total)
            }
            None => false,
        }
    }

    /// The canonical `Par` of the state. The patterns and the guard are copied, and each copy
    /// is reserved first.
    fn to_par(&self, meter: &ScanMeter) -> Result<Par, SpaceError> {
        let patterns = meter.copy_slice(&self.pattern.patterns)?;
        let guard = match &self.guard {
            Some(guard) => vec![meter.copy(guard)?],
            None => Vec::new(),
        };
        meter.reserve_buffer::<Par>(STATE_ELEMENTS)?;
        Ok(tuple_par(vec![
            int_par(MATCHER_STATE_VERSION),
            expr_par(ExprInstance::GByteArray(self.receive_tag.to_vec())),
            int_par(i64::from(self.bind)),
            int_par(i64::from(self.offset)),
            int_par(i64::from(self.total)),
            int_par(i64::from(self.binds)),
            tuple_par(vec![
                list_par(patterns, self.pattern.remainder.clone()),
                int_par(i64::from(self.pattern.free_count)),
            ]),
            list_par(guard, None),
        ]))
    }

    /// The state of a canonical `Par`, or `None` when the `Par` is not the image of a
    /// consistent state. The parts move out of `par`, so nothing is copied.
    fn from_par(par: Par) -> Option<Self> {
        let elements = into_tuple(par)?;
        let [version, tag, bind, offset, total, binds, pattern, guard]: [Par; STATE_ELEMENTS] =
            elements.try_into().ok()?;
        if sole_int(&version)? != MATCHER_STATE_VERSION {
            return None;
        }
        let receive_tag = match sole_expr(&tag)? {
            ExprInstance::GByteArray(bytes) => <[u8; SID_LEN]>::try_from(bytes.as_slice()).ok()?,
            _ => return None,
        };
        let [patterns, free_count]: [Par; 2] = into_tuple(pattern)?.try_into().ok()?;
        let (patterns, remainder) = into_list(patterns)?;
        let free_count = i32::try_from(sole_int(&free_count)?).ok()?;
        let guard = match <[Par; 0]>::try_from(into_list_plain(guard)?) {
            Ok([]) => None,
            Err(elements) => {
                let [guard]: [Par; 1] = elements.try_into().ok()?;
                Some(guard)
            }
        };
        let state = MatcherState {
            receive_tag,
            bind: u32::try_from(sole_int(&bind)?).ok()?,
            offset: u32::try_from(sole_int(&offset)?).ok()?,
            total: u32::try_from(sole_int(&total)?).ok()?,
            binds: u32::try_from(sole_int(&binds)?).ok()?,
            pattern: BindPattern {
                patterns,
                remainder,
                free_count,
            },
            guard,
        };
        state.is_consistent().then_some(state)
    }

    /// The canonical bytes of the state: the prost encoding of its canonical `Par`. The build of
    /// the `Par`, the read of the length computation, the buffer and the encode are reserved
    /// first, as the escape arm of the path codec reserves a prost encode.
    pub fn encode(&self, meter: &ScanMeter) -> Result<Vec<u8>, SpaceError> {
        let par = self.to_par(meter)?;
        meter.inspect(&par)?;
        let length = par.encoded_len();
        let mut bytes = meter.buffer::<u8>(length)?;
        if meter.is_metered() {
            clone_backing::reserve_nested_encode(&par, length, meter)
                .map_err(|_| RSpaceError::HostWorkRejected)?;
        }
        par.encode(&mut bytes)
            .map_err(|_| SpaceError::Inconsistent("a matcher state does not encode"))?;
        Ok(bytes)
    }

    /// The state of the canonical bytes `bytes`, or `None` for every other input (MO-2). The
    /// bytes are reserved before the decode. Then the decode, the checks of the tuple, the
    /// re-encode and the byte comparison run, so a state decodes exactly when it is the image
    /// of [`MatcherState::encode`].
    pub fn decode(bytes: &[u8], meter: &ScanMeter) -> Result<Option<Self>, SpaceError> {
        meter.charge(1, bytes.len(), bytes.len())?;
        let Ok(par) = Par::decode(bytes) else {
            return Ok(None);
        };
        let Some(state) = Self::from_par(par) else {
            return Ok(None);
        };
        let encoded = state.encode(meter)?;
        meter.compare(encoded.len(), bytes.len())?;
        Ok((encoded == bytes).then_some(state))
    }

    /// The id `E0‖X‖state` of the matcher capability of the state.
    pub fn channel(&self, meter: &ScanMeter) -> Result<Vec<u8>, SpaceError> {
        let state = self.encode(meter)?;
        meter.reserve_buffer::<u8>(state.len() + 2)?;
        Ok(namespace::matcher_channel(&state))
    }
}

/// The state of a matcher capability `m`: a bare uppercase `X` id whose bytes decode exactly
/// (MO-2). Every other value is `None`: a bundle, a lowercase `x` id, another id, or bytes that
/// are not the image of the encoder.
pub fn matcher_state_of(m: &Par, meter: &ScanMeter) -> Result<Option<MatcherState>, SpaceError> {
    match namespace::reified_id_of(m) {
        Some(ReifiedId::Matcher(Durability::Durable, state)) => MatcherState::decode(state, meter),
        _ => Ok(None),
    }
}

fn int_par(value: i64) -> Par { expr_par(ExprInstance::GInt(value)) }

fn sole_int(par: &Par) -> Option<i64> {
    match sole_expr(par)? {
        ExprInstance::GInt(value) => Some(*value),
        _ => None,
    }
}

/// A list with the bit sets that the encoder of a state derives: the union of the
/// `locally_free` sets of its elements, and the disjunction of their `connective_used` flags
/// and of the presence of a remainder.
fn list_par(ps: Vec<Par>, remainder: Option<Var>) -> Par {
    let (locally_free, connective_used) = ps.iter().fold(
        (Vec::new(), remainder.is_some()),
        |(locally_free, connective_used), element| {
            (
                union(locally_free, element.locally_free.clone()),
                connective_used || element.connective_used,
            )
        },
    );
    Par {
        locally_free: locally_free.clone(),
        connective_used,
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps,
                locally_free,
                connective_used,
                remainder,
            })),
        }],
        ..Par::default()
    }
}

/// The elements of a `Par` that holds exactly one tuple.
fn into_tuple(par: Par) -> Option<Vec<Par>> {
    sole_expr(&par)?;
    match par.exprs.into_iter().next()?.expr_instance? {
        ExprInstance::ETupleBody(tuple) => Some(tuple.ps),
        _ => None,
    }
}

/// The elements and the remainder of a `Par` that holds exactly one list.
fn into_list(par: Par) -> Option<(Vec<Par>, Option<Var>)> {
    sole_expr(&par)?;
    match par.exprs.into_iter().next()?.expr_instance? {
        ExprInstance::EListBody(list) => Some((list.ps, list.remainder)),
        _ => None,
    }
}

/// The elements of a `Par` that holds exactly one list without a remainder.
fn into_list_plain(par: Par) -> Option<Vec<Par>> {
    match into_list(par)? {
        (elements, None) => Some(elements),
        (_, Some(_)) => None,
    }
}

/// The reply `(false, Nil)`: the matcher refuses the process.
fn refusal() -> Par {
    tuple_par(vec![
        new_gbool_par(false, Vec::new(), false),
        Par::default(),
    ])
}

/// The process `MATCHER_OP` (design v2 MO-1 to MO-4).
#[derive(Clone)]
pub struct MatcherProcess {
    runtime: Arc<ReifiedRuntime>,
}

impl MatcherProcess {
    /// The matcher process of the runtime `runtime`. It matches with the injected matcher of
    /// the host of the runtime.
    pub fn new(runtime: Arc<ReifiedRuntime>) -> Self { Self { runtime } }

    /// Runs the call `(m, [ret, proc])` (MO-2, MO-3):
    ///
    /// 1. `m` must be a bare uppercase `X` id whose state decodes exactly. The bytes of the id
    ///    are reserved before the decode. Every other call is dropped.
    /// 2. The message must be the list `[ret, proc]` with `ret` a reply name (A23). Every other
    ///    message is dropped.
    /// 3. A `proc` that is not a list gets `(false, Nil)`.
    /// 4. The elements of `proc` are matched against the pattern of the state with the
    ///    injected matcher of the host. A detached runtime fails with `RuntimeUnavailable`.
    /// 5. A state with a guard checks it over the values of all `n` binds in bind order: the
    ///    matched values hold the positions of bind `i`, and `Nil` placeholders hold the
    ///    positions of the other binds, so every binder keeps its position (TR-4).
    /// 6. A match that passes replies `(true, env)`, with `env` the map from the decimal global
    ///    position `off_i + j` of each binder `j` of the bind to its value. Otherwise the reply
    ///    is `(false, Nil)`.
    pub async fn call(
        &self,
        contract: &ContractCall,
        contract_args: ContractArgs,
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = contract.unapply(contract_args) else {
            return Err(illegal_argument_error(MATCHER_OP_PROCESS.urn));
        };
        let [m, message] = args.as_slice() else {
            return Err(illegal_argument_error(MATCHER_OP_PROCESS.urn));
        };
        let meter = ScanMeter::new(
            deterministic_reduction::current().and_then(|context| context.host_work_budget()),
        );
        let Some(state) = matcher_state_of(m, &meter)? else {
            return Ok(Vec::new());
        };
        let Some([ret, proc]) =
            sole_list(message).and_then(|elements| <&[Par; 2]>::try_from(elements).ok())
        else {
            return Ok(Vec::new());
        };
        if !is_reply_name(ret) {
            return Ok(Vec::new());
        }
        let reply = vec![self.answer(&state, proc, &meter)?];
        produce(&reply, ret).await?;
        Ok(reply)
    }

    /// The reply to `proc` under `state` (steps 3 to 6 of [`MatcherProcess::call`]).
    fn answer(
        &self,
        state: &MatcherState,
        proc: &Par,
        meter: &ScanMeter,
    ) -> Result<Par, InterpreterError> {
        let Some(elements) = sole_list(proc) else {
            return Ok(refusal());
        };
        let env = self.runtime.space_env(meter.host_work().cloned())?;
        let matcher = SpaceMatcher::metered(&env)?;
        let data = ListParWithRandom {
            pars: meter.copy_slice(elements)?,
            random_state: Vec::new(),
            cost_authority: None,
            cost_stack: None,
        };
        let Some(matched) = matcher.bind(&state.pattern, &data)? else {
            return Ok(refusal());
        };
        if let Some(guard) = &state.guard {
            if !guard_passes(&matcher, state, guard, &matched, meter)? {
                return Ok(refusal());
            }
        }
        Ok(env_reply(state, matched.pars, meter)?)
    }
}

/// The guard of `state` over the values of all binds in bind order (MO-3, TR-4): `Nil`
/// placeholders for the `off_i` binders before bind `i`, the matched values of bind `i`, and
/// `Nil` placeholders for the binders after it. The guard check pushes the values of every bind
/// into one environment in bind order, so `BoundVar(k)` reads position `total − 1 − k`, as in a
/// native receive. The placeholders are reserved before they are built.
fn guard_passes(
    matcher: &SpaceMatcher<'_>,
    state: &MatcherState,
    guard: &Par,
    matched: &ListParWithRandom,
    meter: &ScanMeter,
) -> Result<bool, SpaceError> {
    let free = state.free_count().ok_or(SpaceError::Inconsistent(
        "a decoded matcher state has a negative free count",
    ))?;
    let before = usize::try_from(state.offset)
        .map_err(|_| SpaceError::Inconsistent("a binder position does not fit in usize"))?;
    let after = usize::try_from(state.total - state.offset - free)
        .map_err(|_| SpaceError::Inconsistent("a binder position does not fit in usize"))?;
    let placeholders = |count: usize| -> Result<ListParWithRandom, SpaceError> {
        let mut pars = meter.buffer::<Par>(count)?;
        pars.resize(count, Par::default());
        Ok(ListParWithRandom {
            pars,
            random_state: Vec::new(),
            cost_authority: None,
            cost_stack: None,
        })
    };
    let continuation = TaggedContinuation {
        tagged_cont: None,
        guard: Some(meter.copy(guard)?),
        cost_authority: None,
    };
    let bindings = [
        placeholders(before)?,
        meter.copy(matched)?,
        placeholders(after)?,
    ];
    matcher.admits_all(&continuation, &bindings)
}

/// The reply `(true, env)` with `env` the map from the decimal global position `off_i + j` of
/// each binder `j` of the bind to its value `values[j]`. The keys, the pair buffer, the sort of
/// the map and its nodes are reserved before they are built.
fn env_reply(state: &MatcherState, values: Vec<Par>, meter: &ScanMeter) -> Result<Par, SpaceError> {
    let mut pairs = meter.buffer::<(Par, Par)>(values.len())?;
    for (index, value) in values.into_iter().enumerate() {
        let position = u64::from(state.offset)
            .checked_add(u64::try_from(index).map_err(|_| RSpaceError::HostWorkRejected)?)
            .ok_or(SpaceError::Inconsistent("a binder position overflows"))?;
        let key = position.to_string();
        meter.charge(1, key.len(), key.len())?;
        meter.inspect(&value)?;
        meter.ordered_lookup(pairs.len(), key.len())?;
        pairs.push((expr_par(ExprInstance::GString(key)), value));
    }
    meter.tree_growth::<Par, Par>(0, pairs.len())?;
    meter.reserve_buffer::<Par>(2)?;
    Ok(tuple_par(vec![
        new_gbool_par(true, Vec::new(), false),
        map_par(pairs),
    ]))
}

/// The process `SINK_OP` (design v2 SK-1): it takes a message of any arity and drops it. The
/// install pattern of the sink has no binder and a wildcard remainder, so the dispatch gives it
/// no value.
pub async fn sink(contract_args: ContractArgs) -> Result<Vec<Par>, InterpreterError> {
    match contract_args.0.as_slice() {
        [_] => Ok(Vec::new()),
        _ => Err(illegal_argument_error(SINK_OP_PROCESS.urn)),
    }
}

/// The branch of the derivation tree under `base = Xlate.derive(R)` that the call of a send
/// takes (design v2 TS-1).
pub const SEND_CALL_BRANCH: u8 = 1;

/// The return channel of the calls of a translation (design v2 TS-2): the write-only bundle
/// `bundle+{[0xE0, SINK_OP]}`, the value of the URN of `SINK_OP`.
static SINK: LazyLock<Par> = LazyLock::new(|| {
    new_bundle_par(
        namespace::gprivate(namespace::op_channel(SINK_OP)),
        true,
        false,
    )
});

/// The method name of the call of a send.
static PRODUCE_METHOD: LazyLock<Par> =
    LazyLock::new(|| new_gstring_par("produce".to_string(), Vec::new(), false));

/// The return channel of the calls of a translation (design v2 TS-2).
pub fn sink_channel() -> Par { SINK.clone() }

/// The call of a send on a space that the user agent `agent` implements (design v2 TS-1, AG-4
/// and R7A-1): `A'!(sink, "produce", raw, [d1, …, dk])`.
///
/// * `A'` is the call channel of `agent` under the bundle rule of `eval_send`. A reference
///   without the write flag gives `HandleNotWritable`.
/// * `[d1, …, dk]` holds the data of the send in order, with the bit sets that the reducer
///   derives for a packed argument list.
/// * The call datum takes the random state `base.split_byte(1)` with `base = Xlate.derive(R)`,
///   where `R` is the random state of the send datum. It keeps the authority of the send datum.
///
/// The copy of the agent, the derived random state and the two constants are reserved before
/// they are made (design v2 A6).
pub fn produce_call(
    agent: &Par,
    raw: Par,
    data: ListParWithRandom,
    meter: &ScanMeter,
) -> Result<(Par, ListParWithRandom), InterpreterError> {
    meter.reserve_copy(agent)?;
    let channel = call_channel(agent).map_err(ReifiedReduceError::from)?;
    let ListParWithRandom {
        pars,
        random_state,
        cost_authority,
        cost_stack,
    } = data;
    let call = RandomPurpose::Xlate
        .derive(&Blake2b512Random::from_bytes(&random_state))
        .split_byte(SEND_CALL_BRANCH);
    meter.random_bytes(&call)?;
    let sink = meter.copy(&*SINK)?;
    let method = meter.copy(&*PRODUCE_METHOD)?;
    Ok((channel, ListParWithRandom {
        pars: vec![sink, method, raw, list_par(pars, None)],
        random_state: call.to_bytes(),
        cost_authority,
        cost_stack,
    }))
}

/// The branch of the receive tag of a translated receive (design v2 TR-5):
/// `base.split_byte(2).next()`.
pub const RECEIVE_TAG_BRANCH: u8 = 2;

/// The branch of the body id of a translated receive (TR-6): `base.split_byte(3).next()`.
pub const BODY_ID_BRANCH: u8 = 3;

/// The branch of the call of a translated receive (TR-7): `base.split_byte(4)`.
pub const RECEIVE_CALL_BRANCH: u8 = 4;

/// The branch of the matcher continuations of a translated receive (TR-5):
/// `base.split_byte(6).split_short(i)`.
pub const MATCHER_CONTINUATION_BRANCH: u8 = 6;

/// The method name of the call of a receive.
static CONSUME_METHOD: LazyLock<Par> =
    LazyLock::new(|| new_gstring_par("consume".to_string(), Vec::new(), false));

/// The 32 bytes of the next output of `random`.
fn output_bytes(mut random: Blake2b512Random) -> [u8; SID_LEN] {
    let mut bytes = [0u8; SID_LEN];
    for (byte, value) in bytes.iter_mut().zip(random.next()) {
        *byte = value as u8;
    }
    bytes
}

/// A node of the work list of the guard walk, with the number of binders of the guard that the
/// node lies under.
enum GuardNode<'a> {
    Par(&'a Par, u32),
    Expr(&'a Expr, u32),
}

/// The work list of one guard walk. Each push reserves the `SearchStateBytes` of its entry.
struct GuardWork<'a, 'm> {
    nodes: Vec<GuardNode<'a>>,
    meter: &'m ScanMeter,
}

impl<'a> GuardWork<'a, '_> {
    fn push(&mut self, node: GuardNode<'a>) -> Result<(), SpaceError> {
        self.meter.charge(0, 0, size_of::<GuardNode<'_>>())?;
        self.nodes
            .try_reserve(1)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        self.nodes.push(node);
        Ok(())
    }

    fn par(&mut self, par: &'a Par, depth: u32) -> Result<(), SpaceError> {
        self.push(GuardNode::Par(par, depth))
    }

    fn optional(&mut self, par: &'a Option<Par>, depth: u32) -> Result<(), SpaceError> {
        match par {
            Some(par) => self.par(par, depth),
            None => Ok(()),
        }
    }

    fn pars(&mut self, pars: &'a [Par], depth: u32) -> Result<(), SpaceError> {
        pars.iter().try_for_each(|par| self.par(par, depth))
    }
}

/// The depth under `count` more binders.
fn deeper(depth: u32, count: i32) -> Result<u32, SpaceError> {
    u32::try_from(count)
        .ok()
        .and_then(|count| depth.checked_add(count))
        .ok_or(SpaceError::Inconsistent(
            "a binder count of a guard does not fit",
        ))
}

/// The free `BoundVar` indices of `guard` (design v2 TR-4). A `BoundVar(k)` under `d` binders
/// of the guard is free when `k >= d`, and then it names the index `k - d` of the scope of the
/// guard: the binders of the receive, then the outer binders. The walk tracks the binders of
/// nested receives, news and match cases, reads no `locally_free` set (A4), and reserves each
/// visit, as the Seq scan does.
pub fn guard_indices(guard: &Par, meter: &ScanMeter) -> Result<BTreeSet<u32>, SpaceError> {
    let mut indices = BTreeSet::new();
    let mut work = GuardWork {
        nodes: Vec::new(),
        meter,
    };
    work.par(guard, 0)?;
    while let Some(node) = work.nodes.pop() {
        meter.charge(1, 0, 0)?;
        let free = match node {
            GuardNode::Par(par, depth) => {
                guard_par(par, depth, &mut work)?;
                None
            }
            GuardNode::Expr(expr, depth) => guard_expr(expr, depth, &mut work)?,
        };
        if let Some(index) = free {
            meter.ordered_lookup(indices.len(), size_of::<u32>())?;
            meter.tree_growth::<u32, ()>(indices.len(), 1)?;
            indices.insert(index);
        }
    }
    Ok(indices)
}

/// Adds every child of `par`, under `depth` binders of the guard, to the work list. A receive
/// binds `bind_count` names for its body and its condition, a `new` binds `bind_count` names
/// for its process, and a match case binds `free_count` names for its body and its guard.
/// Patterns, sources and every other child stay at `depth`.
fn guard_par<'a>(par: &'a Par, depth: u32, work: &mut GuardWork<'a, '_>) -> Result<(), SpaceError> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables: _,
        bundles,
        connectives,
        conditionals,
        locally_free: _,
        connective_used: _,
        cost_signed_terms,
        cost_stacks: _,
    } = par;
    for send in sends {
        let Send {
            chan,
            data,
            persistent: _,
            locally_free: _,
            connective_used: _,
        } = send;
        work.optional(chan, depth)?;
        work.pars(data, depth)?;
    }
    for receive in receives {
        let Receive {
            binds,
            body,
            persistent: _,
            peek: _,
            bind_count,
            locally_free: _,
            connective_used: _,
            condition,
        } = receive;
        for bind in binds {
            let ReceiveBind {
                patterns,
                source,
                remainder: _,
                free_count: _,
                cost_signature: _,
            } = bind;
            work.pars(patterns, depth)?;
            work.optional(source, depth)?;
        }
        let inner = deeper(depth, *bind_count)?;
        work.optional(body, inner)?;
        work.optional(condition, inner)?;
    }
    for new in news {
        let New {
            bind_count,
            p,
            uri: _,
            injections,
            locally_free: _,
        } = new;
        work.optional(p, deeper(depth, *bind_count)?)?;
        for injected in injections.values() {
            work.par(injected, depth)?;
        }
    }
    for expr in exprs {
        work.push(GuardNode::Expr(expr, depth))?;
    }
    for matched in matches {
        let Match {
            target,
            cases,
            locally_free: _,
            connective_used: _,
        } = matched;
        work.optional(target, depth)?;
        for case in cases {
            let MatchCase {
                pattern,
                source,
                free_count,
                guard,
            } = case;
            work.optional(pattern, depth)?;
            let inner = deeper(depth, *free_count)?;
            work.optional(source, inner)?;
            work.optional(guard, inner)?;
        }
    }
    for bundle in bundles {
        let Bundle {
            body,
            write_flag: _,
            read_flag: _,
        } = bundle;
        work.optional(body, depth)?;
    }
    for connective in connectives {
        let Connective {
            connective_instance,
        } = connective;
        match connective_instance {
            Some(ConnectiveInstance::ConnAndBody(ConnectiveBody { ps }))
            | Some(ConnectiveInstance::ConnOrBody(ConnectiveBody { ps })) => {
                work.pars(ps, depth)?
            }
            Some(ConnectiveInstance::ConnNotBody(negated)) => work.par(negated, depth)?,
            Some(
                ConnectiveInstance::VarRefBody(_)
                | ConnectiveInstance::ConnBool(_)
                | ConnectiveInstance::ConnInt(_)
                | ConnectiveInstance::ConnString(_)
                | ConnectiveInstance::ConnUri(_)
                | ConnectiveInstance::ConnByteArray(_),
            )
            | None => {}
        }
    }
    for conditional in conditionals {
        let If {
            condition,
            if_true,
            if_false,
            locally_free: _,
            connective_used: _,
        } = conditional;
        work.optional(condition, depth)?;
        work.optional(if_true, depth)?;
        work.optional(if_false, depth)?;
    }
    for term in cost_signed_terms {
        let CostSignedTerm { body, signature: _ } = term;
        work.optional(body, depth)?;
    }
    Ok(())
}

/// Adds every child of `expr` to the work list, and returns the index that `expr` names when
/// it is a free `BoundVar`.
fn guard_expr<'a>(
    expr: &'a Expr,
    depth: u32,
    work: &mut GuardWork<'a, '_>,
) -> Result<Option<u32>, SpaceError> {
    let Expr { expr_instance } = expr;
    match expr_instance {
        Some(ExprInstance::EVarBody(EVar {
            v:
                Some(Var {
                    var_instance: Some(VarInstance::BoundVar(index)),
                }),
        })) => Ok(u32::try_from(*index)
            .ok()
            .and_then(|index| index.checked_sub(depth))),
        Some(ExprInstance::EVarBody(EVar { v: _ })) => Ok(None),
        Some(
            ExprInstance::GBool(_)
            | ExprInstance::GInt(_)
            | ExprInstance::GString(_)
            | ExprInstance::GUri(_)
            | ExprInstance::GByteArray(_)
            | ExprInstance::GDouble(_)
            | ExprInstance::GBigInt(_)
            | ExprInstance::GBigRat(_)
            | ExprInstance::GFixedPoint(_)
            | ExprInstance::GFloat32(_),
        )
        | None => Ok(None),
        Some(ExprInstance::ENotBody(ENot { p }) | ExprInstance::ENegBody(ENeg { p })) => {
            work.optional(p, depth)?;
            Ok(None)
        }
        Some(
            ExprInstance::EMultBody(EMult { p1, p2 })
            | ExprInstance::EDivBody(EDiv { p1, p2 })
            | ExprInstance::EPlusBody(EPlus { p1, p2 })
            | ExprInstance::EMinusBody(EMinus { p1, p2 })
            | ExprInstance::ELtBody(ELt { p1, p2 })
            | ExprInstance::ELteBody(ELte { p1, p2 })
            | ExprInstance::EGtBody(EGt { p1, p2 })
            | ExprInstance::EGteBody(EGte { p1, p2 })
            | ExprInstance::EEqBody(EEq { p1, p2 })
            | ExprInstance::ENeqBody(ENeq { p1, p2 })
            | ExprInstance::EAndBody(EAnd { p1, p2 })
            | ExprInstance::EOrBody(EOr { p1, p2 })
            | ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 })
            | ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 })
            | ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 })
            | ExprInstance::EModBody(EMod { p1, p2 }),
        ) => {
            work.optional(p1, depth)?;
            work.optional(p2, depth)?;
            Ok(None)
        }
        Some(ExprInstance::EListBody(EList {
            ps,
            locally_free: _,
            connective_used: _,
            remainder: _,
        }))
        | Some(ExprInstance::ETupleBody(ETuple {
            ps,
            locally_free: _,
            connective_used: _,
        }))
        | Some(ExprInstance::ESetBody(ESet {
            ps,
            locally_free: _,
            connective_used: _,
            remainder: _,
        }))
        | Some(ExprInstance::EPathmapBody(EPathMap {
            ps,
            locally_free: _,
            connective_used: _,
            remainder: _,
        })) => {
            work.pars(ps, depth)?;
            Ok(None)
        }
        Some(ExprInstance::EMapBody(EMap {
            kvs,
            locally_free: _,
            connective_used: _,
            remainder: _,
        })) => {
            for pair in kvs {
                let KeyValuePair { key, value } = pair;
                work.optional(key, depth)?;
                work.optional(value, depth)?;
            }
            Ok(None)
        }
        Some(ExprInstance::EMethodBody(EMethod {
            method_name: _,
            target,
            arguments,
            locally_free: _,
            connective_used: _,
        })) => {
            work.optional(target, depth)?;
            work.pars(arguments, depth)?;
            Ok(None)
        }
        Some(ExprInstance::EZipperBody(EZipper {
            pathmap,
            current_path: _,
            is_write_zipper: _,
            locally_free: _,
            connective_used: _,
        })) => {
            if let Some(EPathMap {
                ps,
                locally_free: _,
                connective_used: _,
                remainder: _,
            }) = pathmap
            {
                work.pars(ps, depth)?;
            }
            Ok(None)
        }
        Some(ExprInstance::EMatchesBody(EMatches { target, pattern })) => {
            work.optional(target, depth)?;
            work.optional(pattern, depth)?;
            Ok(None)
        }
    }
}

/// The offsets `off_i` of the binds with the free counts `counts`, and their total (design v2
/// TR-3): `off_0 = 0`, `off_(i+1) = off_i + f_i`, `total = off_n`.
fn offsets_of(counts: &[u32]) -> Result<(Vec<u32>, u32), SpaceError> {
    let mut offsets = Vec::with_capacity(counts.len());
    let mut next = 0u32;
    for count in counts {
        offsets.push(next);
        next = next.checked_add(*count).ok_or(SpaceError::Inconsistent(
            "the binders of a receive overflow",
        ))?;
    }
    Ok((offsets, next))
}

/// The bind that owns the global binder position `position`: the bind `i` with
/// `off_i <= position < off_i + f_i`.
fn owner_of(position: u32, offsets: &[u32], counts: &[u32]) -> Option<usize> {
    offsets
        .iter()
        .zip(counts)
        .position(|(offset, count)| *offset <= position && position - *offset < *count)
}

/// The bind that takes the guard of a receive (design v2 TR-4). Each free index `k` below
/// `total` reads the binder at the position `total - 1 - k`, which one bind owns. The one owner
/// of every such index takes the guard, bind 0 takes a guard with no such index, and two or
/// more owners abort with `AgentGuardSpansJoin`. An index from `total` on is outer and unbound,
/// as in a native receive (F7).
pub fn place_guard(
    indices: &BTreeSet<u32>,
    counts: &[u32],
    meter: &ScanMeter,
) -> Result<usize, InterpreterError> {
    let (offsets, total) = offsets_of(counts)?;
    let mut owner = None;
    for index in indices {
        meter.charge(1, 0, 0)?;
        let Some(position) = index
            .checked_add(1)
            .and_then(|next| total.checked_sub(next))
        else {
            continue;
        };
        let bind = owner_of(position, &offsets, counts).ok_or(SpaceError::Inconsistent(
            "a binder position of a receive has no bind",
        ))?;
        match owner {
            None => owner = Some(bind),
            Some(first) if first == bind => {}
            Some(_) => return Err(ReifiedReduceError::AgentGuardSpansJoin.into()),
        }
    }
    Ok(owner.unwrap_or(0))
}

/// The pattern of the body receive of a translated receive with `total` binders (design v2
/// TR-6): one map `{"g": FreeVar(g) | g < total}`. The key of a binder is the minimal decimal
/// string of its global position, as the matcher op writes it, and `FreeVar(g)` sits at the key
/// `"g"`, so the body binds its values in binder order. Each key, entry and insertion is
/// reserved before it is made, as `env_reply` reserves its map.
fn body_pattern(total: u32, meter: &ScanMeter) -> Result<BindPattern, SpaceError> {
    let count = usize::try_from(total).map_err(|_| RSpaceError::HostWorkRejected)?;
    let mut pairs = meter.buffer::<(Par, Par)>(count)?;
    for position in 0..total {
        let key = position.to_string();
        meter.charge(1, key.len(), key.len())?;
        meter.reserve_buffer::<Par>(2)?;
        meter.ordered_lookup(pairs.len(), key.len())?;
        let index = i32::try_from(position)
            .map_err(|_| SpaceError::Inconsistent("a binder position does not fit"))?;
        pairs.push((
            expr_par(ExprInstance::GString(key)),
            new_freevar_par(index, Vec::new()),
        ));
    }
    meter.tree_growth::<Par, Par>(0, pairs.len())?;
    Ok(BindPattern {
        patterns: vec![map_par(pairs)],
        remainder: None,
        free_count: i32::try_from(total)
            .map_err(|_| SpaceError::Inconsistent("the binder count does not fit"))?,
    })
}

/// The forwarder of the matcher capability `matcher` (design v2 TR-5): the body
/// `[0xE0, MATCHER_OP]!(m, args)`, where `args` is the list that the remainder of the matcher
/// receive binds. It has the shape of the forwarder of a handle.
fn matcher_forwarder(matcher: &Par) -> Par {
    let args_bound = create_bit_vector(&[0]);
    Par::default()
        .with_sends(vec![Send {
            chan: Some(MATCHER_OP_PROCESS.channel()),
            data: vec![
                matcher.clone(),
                new_boundvar_par(0, args_bound.clone(), false),
            ],
            persistent: false,
            locally_free: args_bound.clone(),
            connective_used: false,
        }])
        .with_locally_free(args_bound)
}

/// A bind of a receive on a space that a user agent implements: its pattern and the raw channel
/// of the agent.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentBind {
    pub pattern: BindPattern,
    pub raw: Par,
}

/// One receive that a translation makes on the default space.
#[derive(Clone, Debug, PartialEq)]
pub struct TranslatedReceive {
    pub pattern: BindPattern,
    pub channel: Par,
    pub continuation: ParWithRandom,
    pub persistent: bool,
}

/// The operations of a translated receive, in the order of TR-7: one persistent matcher receive
/// for each bind, the body receive, and the call on the agent, which comes last because only it
/// can fire.
#[derive(Clone, Debug, PartialEq)]
pub struct ReceiveTranslation {
    pub matchers: Vec<TranslatedReceive>,
    pub body: TranslatedReceive,
    pub call_channel: Par,
    pub call: ListParWithRandom,
}

/// The translation of a receive on a space that the user agent `agent` implements (design v2
/// W3, TR-3 to TR-7). `binds` are the binds of the receive in bind order, `body` is the body
/// after the wrapper of its use-context, with the random state `R` of the receive, and `base` is
/// `Xlate.derive(R)`.
///
/// * TR-3: the offsets of the binds and their total.
/// * TR-4: the guard goes to the bind that owns its binders (`place_guard`).
/// * TR-5: the matcher capability `m_i = E0‖X‖state_i` of each bind, with its persistent
///   receive `[...@args]` and its forwarder, with the state `base.split_byte(6).split_short(i)`.
///   A state that prost cannot decode aborts with `AgentPatternTooDeep`.
/// * TR-6: the body capability `b = E0‖B‖base.split_byte(3).next()`, whose receive binds the
///   env map and runs the body. It is persistent only for `<=`.
/// * TR-7: the call `A'!(sink, "consume", [(bundle+{m_i}, raw_i)…], arrow, bundle+{b})` with
///   the state `base.split_byte(4)` and the introduction authority of the receive.
///
/// The caller scans every `raw_i` for seq names first (A5).
#[allow(clippy::too_many_arguments)]
pub fn translate_receive(
    agent: &Par,
    binds: Vec<AgentBind>,
    body: ParWithRandom,
    persistent: bool,
    peek: bool,
    guard: Option<Par>,
    introduction_authority: &CostAuthority,
    meter: &ScanMeter,
) -> Result<ReceiveTranslation, InterpreterError> {
    let arrow = match (persistent, peek) {
        (false, false) => "<-",
        (true, false) => "<=",
        (false, true) => "<<-",
        (true, true) => {
            return Err(InterpreterError::BugFoundError(
                "a receive is persistent and a peek".to_string(),
            ))
        }
    };
    let counts = binds
        .iter()
        .map(|bind| u32::try_from(bind.pattern.free_count))
        .collect::<Result<Vec<u32>, _>>()
        .map_err(|_| SpaceError::Inconsistent("a bind has a negative free count"))?;
    let (offsets, total) = offsets_of(&counts)?;
    let bind_count = u32::try_from(binds.len())
        .map_err(|_| SpaceError::Inconsistent("a receive has too many binds"))?;
    let base = RandomPurpose::Xlate.derive(&Blake2b512Random::from_bytes(&body.random_state));
    let tag_random = base.split_byte(RECEIVE_TAG_BRANCH);
    meter.random_bytes(&tag_random)?;
    let receive_tag = output_bytes(tag_random);
    let owner = match &guard {
        Some(condition) => place_guard(&guard_indices(condition, meter)?, &counts, meter)?,
        None => 0,
    };
    let mut guard = guard;
    let mut matchers = Vec::with_capacity(binds.len());
    let mut pairs = Vec::with_capacity(binds.len());
    meter.reserve_buffer::<TranslatedReceive>(binds.len())?;
    meter.reserve_buffer::<Par>(binds.len())?;
    let continuations = base.split_byte(MATCHER_CONTINUATION_BRANCH);
    for (index, (bind, offset)) in binds.into_iter().zip(offsets).enumerate() {
        let AgentBind { pattern, raw } = bind;
        let state = MatcherState {
            receive_tag,
            bind: u32::try_from(index)
                .map_err(|_| SpaceError::Inconsistent("a bind index does not fit"))?,
            offset,
            total,
            binds: bind_count,
            pattern,
            guard: match index == owner {
                true => guard.take(),
                false => None,
            },
        };
        let bytes = state.encode(meter)?;
        if MatcherState::decode(&bytes, meter)?.is_none() {
            return Err(ReifiedReduceError::AgentPatternTooDeep.into());
        }
        meter.reserve_buffer::<u8>(bytes.len() + 2)?;
        let matcher = namespace::gprivate(namespace::matcher_channel(&bytes));
        let short = i16::try_from(index)
            .map_err(|_| SpaceError::Inconsistent("a bind index does not fit a short split"))?;
        let continuation_random = continuations.split_short(short);
        meter.random_bytes(&continuation_random)?;
        meter.reserve_copy(&matcher)?;
        let forwarder = matcher_forwarder(&matcher);
        meter.reserve_copy(&matcher)?;
        let capability = new_bundle_par(matcher.clone(), true, false);
        matchers.push(TranslatedReceive {
            pattern: handle_pattern(),
            channel: matcher,
            continuation: ParWithRandom {
                body: Some(forwarder),
                random_state: continuation_random.to_bytes(),
            },
            persistent: true,
        });
        pairs.push(tuple_par(vec![capability, raw]));
    }
    let body_random = base.split_byte(BODY_ID_BRANCH);
    meter.random_bytes(&body_random)?;
    let body_capability = namespace::gprivate(namespace::body_channel(&output_bytes(body_random)));
    let body_receive = TranslatedReceive {
        pattern: body_pattern(total, meter)?,
        channel: body_capability.clone(),
        continuation: body,
        persistent,
    };
    meter.reserve_copy(agent)?;
    let call_channel = call_channel(agent).map_err(ReifiedReduceError::from)?;
    let call_random = base.split_byte(RECEIVE_CALL_BRANCH);
    meter.random_bytes(&call_random)?;
    let call = ListParWithRandom {
        pars: vec![
            meter.copy(&*SINK)?,
            meter.copy(&*CONSUME_METHOD)?,
            list_par(pairs, None),
            new_gstring_par(arrow.to_string(), Vec::new(), false),
            new_bundle_par(body_capability, true, false),
        ],
        random_state: call_random.to_bytes(),
        cost_authority: (!introduction_authority.regions.is_empty())
            .then(|| introduction_authority.clone()),
        cost_stack: None,
    };
    Ok(ReceiveTranslation {
        matchers,
        body: body_receive,
        call_channel,
        call,
    })
}

#[cfg(test)]
mod tests {
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::CostAuthority;
    use models::rust::cost_protocol_limits::offered_funded_v6_host_work_limits;
    use models::rust::host_work::{HostWorkDimension, HostWorkUsage};
    use models::rust::utils::{
        new_boundvar_par, new_bundle_par, new_freevar_par, new_freevar_var, new_gint_par,
        new_wildcard_var,
    };

    use super::*;
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::reified::runtime::unavailable;
    use crate::rust::interpreter::reified::system_processes::harness::{
        call, int, list, reply_name, string, Fixture,
    };
    use crate::rust::interpreter::spaces::namespace::REIFIED_TAG;

    fn unmetered() -> ScanMeter { ScanMeter::new(None) }

    /// The state of bind `bind` of a receive with the free counts `sizes`, with the patterns
    /// `patterns` and the guard `guard`.
    fn state(sizes: &[u32], bind: u32, patterns: Vec<Par>, guard: Option<Par>) -> MatcherState {
        let offset = sizes[..bind as usize].iter().sum();
        MatcherState {
            receive_tag: [7; SID_LEN],
            bind,
            offset,
            total: sizes.iter().sum(),
            binds: u32::try_from(sizes.len()).expect("a small receive"),
            pattern: BindPattern {
                patterns,
                remainder: None,
                free_count: i32::try_from(sizes[bind as usize]).expect("a small bind"),
            },
            guard,
        }
    }

    fn free(index: i32) -> Par { new_freevar_par(index, Vec::new()) }

    fn matcher_of(state: &MatcherState) -> Par {
        namespace::gprivate(state.channel(&unmetered()).expect("a matcher id"))
    }

    fn env(pairs: Vec<(&str, Par)>) -> Par {
        tuple_par(vec![
            new_gbool_par(true, Vec::new(), false),
            map_par(
                pairs
                    .into_iter()
                    .map(|(key, value)| (string(key), value))
                    .collect(),
            ),
        ])
    }

    /// `x > 5` with `x` the binder at position `position` of a receive with `total` binders.
    fn greater_than_five(position: i32, total: i32) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EGtBody(models::rhoapi::EGt {
                p1: Some(new_boundvar_par(total - 1 - position, Vec::new(), false)),
                p2: Some(new_gint_par(5, Vec::new(), false)),
            })),
        }])
    }

    #[test]
    fn matcher_states_round_trip_and_decode_exactly() {
        let samples = [
            state(&[1], 0, vec![free(0)], None),
            state(&[2, 1, 3], 1, vec![free(0)], Some(greater_than_five(2, 6))),
            state(&[0, 2], 1, vec![free(0), free(1)], Some(Par::default())),
            MatcherState {
                pattern: BindPattern {
                    patterns: vec![free(0)],
                    remainder: Some(new_freevar_var(1)),
                    free_count: 2,
                },
                ..state(&[2], 0, Vec::new(), None)
            },
            MatcherState {
                pattern: BindPattern {
                    patterns: Vec::new(),
                    remainder: Some(new_wildcard_var()),
                    free_count: 0,
                },
                ..state(&[0], 0, Vec::new(), None)
            },
        ];
        for sample in samples {
            assert!(sample.is_consistent(), "{sample:?}");
            let bytes = sample.encode(&unmetered()).expect("an encoding");
            assert!(bytes.len() > SID_LEN);
            assert_eq!(
                MatcherState::decode(&bytes, &unmetered()).expect("a decode"),
                Some(sample.clone())
            );
            // A matcher id is the uppercase X form, and it decodes to the same state.
            let id = sample.channel(&unmetered()).expect("an id");
            assert_eq!(&id[..2], &[REIFIED_TAG, b'X']);
            assert_eq!(
                matcher_state_of(&namespace::gprivate(id.clone()), &unmetered()).expect("a decode"),
                Some(sample.clone())
            );
            // A bundle, the lowercase form and any change of a byte name no state.
            assert_eq!(
                matcher_state_of(
                    &new_bundle_par(namespace::gprivate(id.clone()), true, true),
                    &unmetered()
                )
                .expect("a decode"),
                None
            );
            let mut lowercase = id.clone();
            lowercase[1] = b'x';
            assert_eq!(
                matcher_state_of(&namespace::gprivate(lowercase), &unmetered()).expect("a decode"),
                None
            );
            let mut longer = bytes.clone();
            longer.push(0);
            assert_eq!(
                MatcherState::decode(&longer, &unmetered()).expect("a decode"),
                None
            );
            assert_eq!(
                MatcherState::decode(&bytes[..bytes.len() - 1], &unmetered()).expect("a decode"),
                None
            );
        }
        // The guard `Nil` is distinct from no guard.
        let nil_guard = state(&[1], 0, vec![free(0)], Some(Par::default()));
        let no_guard = state(&[1], 0, vec![free(0)], None);
        assert_ne!(
            nil_guard.encode(&unmetered()).expect("an encoding"),
            no_guard.encode(&unmetered()).expect("an encoding")
        );
    }

    #[test]
    fn inconsistent_states_and_other_tuples_do_not_decode() {
        let consistent = state(&[1, 2], 1, vec![free(0), free(1)], None);
        for broken in [
            MatcherState {
                bind: 2,
                ..consistent.clone()
            },
            MatcherState {
                offset: 2,
                ..consistent.clone()
            },
            MatcherState {
                total: 2,
                ..consistent.clone()
            },
            MatcherState {
                pattern: BindPattern {
                    free_count: -1,
                    ..consistent.pattern.clone()
                },
                ..consistent.clone()
            },
        ] {
            assert!(!broken.is_consistent(), "{broken:?}");
            let bytes = broken.encode(&unmetered()).expect("an encoding");
            assert_eq!(
                MatcherState::decode(&bytes, &unmetered()).expect("a decode"),
                None
            );
        }
        let other_version = {
            let par = consistent.to_par(&unmetered()).expect("a par");
            let mut elements = into_tuple(par).expect("a tuple");
            elements[0] = int_par(MATCHER_STATE_VERSION + 1);
            tuple_par(elements).encode_to_vec()
        };
        assert_eq!(
            MatcherState::decode(&other_version, &unmetered()).expect("a decode"),
            None
        );
        for other in [int(1), string("state"), list(Vec::new()), Par::default()] {
            assert_eq!(
                MatcherState::decode(&other.encode_to_vec(), &unmetered()).expect("a decode"),
                None
            );
        }
        assert_eq!(
            MatcherState::decode(&[0xFF; 40], &unmetered()).expect("a decode"),
            None
        );
    }

    #[test]
    fn body_and_matcher_ids_are_capabilities_of_the_default_space() {
        let body = namespace::body_channel(&[3; SID_LEN]);
        assert_eq!(body.len(), 2 + SID_LEN);
        assert!(matches!(
            namespace::decode_reified_id(&body),
            Some(ReifiedId::Body(Durability::Durable, tag)) if tag == [3; SID_LEN]
        ));
        let matcher = state(&[1], 0, vec![free(0)], None)
            .channel(&unmetered())
            .expect("an id");
        assert!(matches!(
            namespace::decode_reified_id(&matcher),
            Some(ReifiedId::Matcher(Durability::Durable, _))
        ));
    }

    async fn ask(fixture: &Fixture, m: Par, message: Par) -> Result<Vec<Par>, InterpreterError> {
        MatcherProcess::new(Arc::clone(&fixture.runtime))
            .call(&fixture.contract, call(vec![m, message], b"matcher"))
            .await
    }

    #[tokio::test]
    async fn matcher_op_replies_the_env_of_a_match_and_refuses_the_rest() {
        let fixture = Fixture::new().await;
        let ret = reply_name(1);
        // Bind 1 of a receive with the free counts [2, 1, 3]: its binder is position 2.
        let single = matcher_of(&state(&[2, 1, 3], 1, vec![free(0)], None));
        let reply = ask(
            &fixture,
            single.clone(),
            list(vec![ret.clone(), list(vec![int(9)])]),
        )
        .await
        .expect("a reply");
        assert_eq!(reply, vec![env(vec![("2", int(9))])]);
        assert_eq!(fixture.data(&ret).await, vec![reply]);
        // Two values for one pattern, a value of another shape, and a process that is not a
        // list are refused.
        for proc in [
            list(vec![int(1), int(2)]),
            list(Vec::new()),
            int(9),
            string("not a list"),
        ] {
            let reply = ask(&fixture, single.clone(), list(vec![ret.clone(), proc]))
                .await
                .expect("a reply");
            assert_eq!(reply, vec![refusal()]);
        }
        // A structured pattern binds its variables at the positions of its bind.
        // A pattern with variables carries `connective_used`, as the normalizer sets it.
        let pair = matcher_of(&state(
            &[1, 2],
            1,
            vec![list_par(vec![free(0), free(1)], None)],
            None,
        ));
        let reply = ask(
            &fixture,
            pair,
            list(vec![
                ret.clone(),
                list(vec![list(vec![int(4), string("b")])]),
            ]),
        )
        .await
        .expect("a reply");
        assert_eq!(reply, vec![env(vec![("1", int(4)), ("2", string("b"))])]);
    }

    #[tokio::test]
    async fn matcher_op_checks_the_guard_at_the_global_positions() {
        let fixture = Fixture::new().await;
        let ret = reply_name(2);
        // Bind 1 of [2, 1, 3] owns the guard `x > 5` on its binder, position 2 of 6 binders.
        let guarded = matcher_of(&state(
            &[2, 1, 3],
            1,
            vec![free(0)],
            Some(greater_than_five(2, 6)),
        ));
        let pass = ask(
            &fixture,
            guarded.clone(),
            list(vec![ret.clone(), list(vec![int(9)])]),
        )
        .await
        .expect("a reply");
        assert_eq!(pass, vec![env(vec![("2", int(9))])]);
        let refuse = ask(
            &fixture,
            guarded.clone(),
            list(vec![ret.clone(), list(vec![int(3)])]),
        )
        .await
        .expect("a reply");
        assert_eq!(refuse, vec![refusal()]);
        // A guard on a position of another bind reads a placeholder, so it never passes, as a
        // native receive whose guard reads `Nil` never fires.
        let misplaced = matcher_of(&state(
            &[2, 1, 3],
            1,
            vec![free(0)],
            Some(greater_than_five(0, 6)),
        ));
        let reply = ask(&fixture, misplaced, list(vec![ret, list(vec![int(9)])]))
            .await
            .expect("a reply");
        assert_eq!(reply, vec![refusal()]);
    }

    /// The usage of every dimension of `host_work`.
    fn usage_of(host_work: &HostWorkBudget) -> Vec<HostWorkUsage> {
        HostWorkDimension::ALL
            .into_iter()
            .map(|dimension| host_work.usage(dimension))
            .collect()
    }

    /// How the reference charges the match and the guard of one bind.
    #[derive(Clone, Copy, PartialEq)]
    enum MatchCalls {
        /// The metered methods that a native receive calls for one bind
        /// (`rspace++/src/rspace/replay_rspace/native_candidate/metered.rs`).
        Metered,
        /// The unmetered methods `get` and `check_commit`: the negative control.
        Unmetered,
    }

    /// The reply of the matcher op to the values `elements` under `state`, and its host work,
    /// rebuilt from the calls of a native receive and the reservations of the op itself (MO-4):
    /// the copy of the shared matcher handle, the copy of the values, the guard environment of
    /// TR-4, and the reply map.
    fn reference(
        fixture: &Fixture,
        state: &MatcherState,
        elements: &[Par],
        calls: MatchCalls,
    ) -> (Par, Vec<HostWorkUsage>) {
        let host_work = HostWorkBudget::new(offered_funded_v6_host_work_limits());
        let meter = ScanMeter::new(Some(host_work.clone()));
        let env = fixture
            .runtime
            .space_env(Some(host_work.clone()))
            .expect("the fixture has a host");
        let matcher = env.store.matcher();
        meter.shared(1).expect("the handle copy fits");
        let data = ListParWithRandom {
            pars: meter.copy_slice(elements).expect("the copy fits"),
            random_state: Vec::new(),
            cost_authority: None,
            cost_stack: None,
        };
        let matched = match calls {
            MatchCalls::Metered => matcher
                .get_metered(&state.pattern, &data, &meter)
                .expect("the match fits"),
            MatchCalls::Unmetered => matcher.get(&state.pattern, &data),
        };
        let Some(matched) = matched else {
            return (refusal(), usage_of(&host_work));
        };
        let admitted = match &state.guard {
            None => true,
            Some(guard) => {
                let free = state.free_count().expect("a consistent state");
                let placeholders = |count: u32| {
                    let count = usize::try_from(count).expect("a small receive");
                    let mut pars = meter.buffer::<Par>(count).expect("the buffer fits");
                    pars.resize(count, Par::default());
                    ListParWithRandom {
                        pars,
                        random_state: Vec::new(),
                        cost_authority: None,
                        cost_stack: None,
                    }
                };
                let continuation = TaggedContinuation {
                    tagged_cont: None,
                    guard: Some(meter.copy(guard).expect("the copy fits")),
                    cost_authority: None,
                };
                let bindings = [
                    placeholders(state.offset),
                    meter.copy(&matched).expect("the copy fits"),
                    placeholders(state.total - state.offset - free),
                ];
                let mut references = meter
                    .buffer::<&ListParWithRandom>(bindings.len())
                    .expect("the buffer fits");
                references.extend(bindings.iter());
                match calls {
                    MatchCalls::Metered => matcher
                        .check_commit_metered(&continuation, &references, &meter)
                        .expect("the guard fits"),
                    MatchCalls::Unmetered => matcher.check_commit(&continuation, &bindings),
                }
            }
        };
        let reply = match admitted {
            true => env_reply(state, matched.pars, &meter).expect("the reply fits"),
            false => refusal(),
        };
        (reply, usage_of(&host_work))
    }

    /// I-15 and MO-4: the matcher op meters its match and its guard with the metered methods
    /// of the injected matcher, the methods that a native receive calls for one bind. So the
    /// host work of the op is the host work of those calls plus the reservations of the op
    /// itself, in every dimension: the op matches nothing without its meter and meters no match
    /// twice. The cases cover a match, a structured match, a mismatch after a partial match, a
    /// wrong count of values, and a guard that passes and one that refuses. A reference with the
    /// unmetered methods charges less wherever the match does work, so the comparison can fail.
    #[tokio::test]
    async fn agent_matcher_metering_equals_host() {
        let fixture = Fixture::new().await;
        let op = MatcherProcess::new(Arc::clone(&fixture.runtime));
        let pair = list_par(vec![free(0), free(1)], None);
        let cases = [
            (state(&[1], 0, vec![free(0)], None), vec![int(9)], true),
            (
                state(&[1, 2], 1, vec![pair.clone()], None),
                vec![list(vec![int(4), string("b")])],
                true,
            ),
            (
                state(&[1], 0, vec![list_par(vec![free(0), int(1)], None)], None),
                vec![list(vec![int(4), int(2)])],
                false,
            ),
            (
                state(&[1], 0, vec![free(0)], None),
                vec![int(1), int(2)],
                false,
            ),
            (
                state(&[2, 1, 3], 1, vec![free(0)], Some(greater_than_five(2, 6))),
                vec![int(9)],
                true,
            ),
            (
                state(&[2, 1, 3], 1, vec![free(0)], Some(greater_than_five(2, 6))),
                vec![int(3)],
                false,
            ),
        ];
        for (index, (state, elements, matches)) in cases.into_iter().enumerate() {
            let host_work = HostWorkBudget::new(offered_funded_v6_host_work_limits());
            let meter = ScanMeter::new(Some(host_work.clone()));
            let reply = op
                .answer(&state, &list(elements.clone()), &meter)
                .expect("the op fits the budget");
            let used = usage_of(&host_work);
            let (expected, host) = reference(&fixture, &state, &elements, MatchCalls::Metered);
            assert_eq!(reply, expected, "case {index}");
            assert_eq!(reply != refusal(), matches, "case {index}");
            assert_eq!(used, host, "case {index}: the op and the host calls differ");
            assert!(
                host_work.usage(HostWorkDimension::VerificationBytes) > HostWorkUsage::ZERO,
                "case {index}"
            );
            let (_, unmetered) = reference(&fixture, &state, &elements, MatchCalls::Unmetered);
            if index != 3 {
                assert_ne!(
                    used, unmetered,
                    "case {index}: the control charges the match"
                );
            }
        }
    }

    #[tokio::test]
    async fn matcher_op_drops_calls_without_a_state_or_a_reply_name() {
        let fixture = Fixture::new().await;
        let ret = reply_name(3);
        let m = matcher_of(&state(&[1], 0, vec![free(0)], None));
        let mut lowercase = namespace::sole_gprivate_id(&m).expect("an id").to_vec();
        lowercase[1] = b'x';
        let mut unknown = vec![REIFIED_TAG, b'X'];
        unknown.extend([1; 40]);
        for bad in [
            new_bundle_par(m.clone(), true, true),
            namespace::gprivate(lowercase),
            namespace::gprivate(unknown),
            namespace::gprivate(namespace::body_channel(&[4; SID_LEN])),
            reply_name(9),
            string("m"),
        ] {
            let reply = ask(&fixture, bad, list(vec![ret.clone(), list(vec![int(1)])]))
                .await
                .expect("a dropped call");
            assert!(reply.is_empty());
        }
        for message in [
            list(vec![ret.clone()]),
            list(vec![ret.clone(), list(vec![int(1)]), int(2)]),
            list(vec![string("not a name"), list(vec![int(1)])]),
            int(1),
        ] {
            let reply = ask(&fixture, m.clone(), message)
                .await
                .expect("a dropped call");
            assert!(reply.is_empty());
        }
        assert!(fixture.data(&ret).await.is_empty());
    }

    #[tokio::test]
    async fn matcher_op_needs_the_host_of_the_runtime() {
        let fixture = Fixture::with_runtime(unavailable()).await;
        let m = matcher_of(&state(&[1], 0, vec![free(0)], None));
        let error = ask(
            &fixture,
            m.clone(),
            list(vec![reply_name(4), list(vec![int(1)])]),
        )
        .await
        .expect_err("a detached runtime");
        assert!(
            format!("{error:?}").contains("RuntimeUnavailable"),
            "{error:?}"
        );
        // The checks before the match need no host: a refusal of the shape replies at once.
        let reply = ask(&fixture, m, list(vec![reply_name(4), int(1)]))
            .await
            .expect("a reply");
        assert_eq!(reply, vec![refusal()]);
    }

    #[tokio::test]
    async fn sink_drops_every_message() {
        for pars in [Vec::new(), vec![int(1)], vec![
            int(1),
            string("two"),
            list(Vec::new()),
        ]] {
            assert_eq!(
                sink(call(pars, b"sink")).await.expect("a sink"),
                Vec::<Par>::new()
            );
        }
    }

    #[test]
    fn remainders_keep_their_variables() {
        let with_remainder = list_par(vec![free(0)], Some(new_freevar_var(1)));
        let (_, remainder) = into_list(with_remainder).expect("a list");
        assert!(matches!(
            remainder,
            Some(Var {
                var_instance: Some(VarInstance::FreeVar(1))
            })
        ));
    }

    /// A datum of the data `pars` with the random state of `seed` and the authority
    /// `authority`.
    fn send_datum(
        pars: Vec<Par>,
        seed: &[u8],
        authority: Option<CostAuthority>,
    ) -> ListParWithRandom {
        ListParWithRandom {
            pars,
            random_state: Blake2b512Random::create_from_bytes(seed).to_bytes(),
            cost_authority: authority,
            cost_stack: None,
        }
    }

    /// The message of a failed call.
    fn call_code(error: InterpreterError) -> String {
        match error {
            InterpreterError::ReduceError(message) => message,
            other => format!("{other:?}"),
        }
    }

    /// TS-1 (P8 step 2): the call of a send holds the sink, the method, `raw` and one list of
    /// the data in order. Its random state is `Xlate.derive(R).split_byte(1)`, it keeps the
    /// authority of the send, and the list takes the bit sets of its elements.
    #[test]
    fn produce_call_builds_the_call_of_a_send() {
        let bound = new_boundvar_par(0, vec![1], true);
        let data = send_datum(
            vec![int(7), bound.clone()],
            b"send",
            Some(CostAuthority::default()),
        );
        let (channel, call) =
            produce_call(&string("agent"), int(1), data, &unmetered()).expect("a call");
        assert_eq!(channel, string("agent"));
        let expected_list = list_par(vec![int(7), bound], None);
        assert_eq!(expected_list.locally_free, vec![1]);
        assert!(expected_list.connective_used);
        assert_eq!(call.pars, vec![
            sink_channel(),
            string("produce"),
            int(1),
            expected_list
        ]);
        assert_eq!(
            call.random_state,
            RandomPurpose::Xlate
                .derive(&Blake2b512Random::create_from_bytes(b"send"))
                .split_byte(SEND_CALL_BRANCH)
                .to_bytes()
        );
        assert_eq!(call.cost_authority, Some(CostAuthority::default()));
        assert_eq!(call.cost_stack, None);
    }

    /// AG-4 (P8 step 2): the call channel follows the bundle rule of `eval_send`. A bundle with
    /// the write flag gives its body, and a bundle without it, or without a body, aborts with
    /// `HandleNotWritable` before any work.
    #[test]
    fn produce_call_follows_the_bundle_rule() {
        let agent = string("agent");
        for (reference, expected) in [
            (
                new_bundle_par(agent.clone(), true, false),
                Some(agent.clone()),
            ),
            (
                new_bundle_par(agent.clone(), true, true),
                Some(agent.clone()),
            ),
            (new_bundle_par(agent.clone(), false, true), None),
            (new_bundle_par(agent.clone(), false, false), None),
        ] {
            let result = produce_call(
                &reference,
                int(1),
                send_datum(vec![int(2)], b"send", None),
                &unmetered(),
            );
            match (result, expected) {
                (Ok((channel, _)), Some(expected)) => assert_eq!(channel, expected),
                (Err(error), None) => assert!(
                    call_code(error).starts_with("reified:HandleNotWritable: "),
                    "{reference:?}"
                ),
                (other, expected) => panic!("{reference:?} gave {other:?}, not {expected:?}"),
            }
        }
    }

    /// The sink is the write-only bundle of the op channel `[0xE0, SINK_OP]` (TS-2).
    #[test]
    fn sink_channel_is_the_write_only_op_channel() {
        assert_eq!(
            sink_channel(),
            new_bundle_par(namespace::gprivate(vec![REIFIED_TAG, SINK_OP]), true, false)
        );
    }
}
