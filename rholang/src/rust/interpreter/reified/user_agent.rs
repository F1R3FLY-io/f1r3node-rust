//! User agent-backed spaces (P8, plan decision 77, design v2 W5 and the codecs of TR-5).
//!
//! The FIPS lets any agent with the behavioral type of an RSpace serve as a space (`Reifying
//! RSpaces.md`, lines 109 to 151). The reducer translates each operation on a name of such a
//! space into a method call `A!(return, "method", args…)`, the call shape of the Agents FIPS. A
//! translated receive gives the agent one matcher capability for each bind and one body
//! capability. This module holds the parts of P8 step 1 that the later steps use:
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

use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, EList, Expr, ListParWithRandom, Par, TaggedContinuation, Var};
use models::rust::utils::{new_gbool_par, union};
use prost::Message;
use rspace_plus_plus::rspace::errors::RSpaceError;
use shared::rust::clone_backing;

use super::costs::ScanMeter;
use super::factory::ContractArgs;
use super::handle::{is_reply_name, map_par, sole_list, tuple_par};
use super::runtime::ReifiedRuntime;
use super::system_channels::{MATCHER_OP_PROCESS, SINK_OP_PROCESS};
use crate::rust::interpreter::contract_call::ContractCall;
use crate::rust::interpreter::deterministic_reduction;
use crate::rust::interpreter::errors::{illegal_argument_error, InterpreterError};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::matching::SpaceMatcher;
use crate::rust::interpreter::spaces::namespace::{self, Durability, ReifiedId, SID_LEN};
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

#[cfg(test)]
mod tests {
    use models::rhoapi::var::VarInstance;
    use models::rust::utils::{
        new_boundvar_par, new_bundle_par, new_freevar_par, new_freevar_var, new_gint_par,
        new_wildcard_var,
    };

    use super::*;
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
}
