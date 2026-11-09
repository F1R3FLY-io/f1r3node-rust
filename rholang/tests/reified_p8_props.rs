// P8 property tests of the spaces that user agents implement (plan decision 77, design v2).
//
// Each property test has the name that formal/reified-rspaces-invariant-map.json maps to a
// headline of phase P8. Step 1 maps headline A1, `agent_ref_requires_write_flag` of
// formal/rocq/reified_rspaces/theories/SpaceRefs.v: a call on a user agent is an ordinary send,
// so the reference of an agent follows the bundle rule of `eval_send` (AG-4). Step 2 maps
// headline A2, `r7_agent_reply_bundle_exact` of the same module: the R7 backend calls an agent
// for exactly the bundle with both flags around an agent-tagged tuple (R7A-1). Step 3 maps the
// headlines of formal/rocq/reified_rspaces/theories/AgentTranslation.v whose statements need no
// runtime: offsets_partition (A3), env_keys_disjoint_union (A4), the placement part of
// guard_fold_sound (A6) and matcher_state_roundtrip (A8). The native properties of
// body_equivalence (A5), guard_fold_sound (A6) and matcher_sound_complete (A7) are in
// rholang/tests/reified_user_agents.rs. Step 4 maps tag_untag_id (A9): an agent-minted name
// routes to its agent with its raw channel, and resume_binds_innermost (A10): the resume term
// of a translated `new`, run with the substitution of the reducer, binds every binder in its
// order. Step 6 maps invariants of formal/tlaplus/reified_rspaces/AgentSpaces.tla:
// RoutingIsolation (A11) and SystemChannelsDefault (A12), whose property routes every class of
// channel under every use-context, and HandleUnwrapExact (A14), whose property classifies
// references with zero, one or two bundles. The native properties of JoinSameSpace (A13),
// ReplayEquivalence (A15) and BodyAtMostOnceLinear (A16) are in
// rholang/tests/reified_user_agents.rs, and the property of NoMatchRemains (A17) is in
// rholang/tests/reified_agent_conformance.rs.

use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, Bundle, CostAuthority, EAnd, EEq, ETuple, EVar, Expr, Par, ParWithRandom, Send,
    Var,
};
use models::rust::utils::{
    new_boundvar_par, new_bundle_par, new_etuple_par, new_freevar_par, new_gint_par,
    new_gstring_par, union,
};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, RngSeed, TestCaseError, TestRunner};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::metering::MeteredMachine;
use rholang::rust::interpreter::reified::errors::ReifiedReduceError;
use rholang::rust::interpreter::reified::execution::r7_route;
use rholang::rust::interpreter::reified::route::{route, Route};
use rholang::rust::interpreter::reified::scope::{
    call_channel, classify_space_ref, SpaceCtx, SpaceRef, SpaceRefError, UseKind, UseTarget,
};
use rholang::rust::interpreter::reified::user_agent::{
    agent_minted_name, guard_indices, matcher_state_of, place_guard, translate_new,
    translate_receive, AgentBind, AgentBinder, MatcherState, ReceiveTranslation, REPLY_NAME_BRANCH,
};
use rholang::rust::interpreter::spaces::namespace::{
    gprivate, handle_channel, name_id, Durability, NameTail, NativeSpace, RandomPurpose, AGENT_TAG,
    REIFIED_TAG, SID_LEN,
};
use rholang::rust::interpreter::substitute::{Substitute, SubstituteTrait};

/// The seed of every property of this file, so a run is reproducible.
const PROPERTY_SEED: u64 = 0x0E0D_2026_1010_0008;

/// The cases of a property that runs no reducer.
const PURE_CASES: u32 = 64;

fn pure_config() -> ProptestConfig {
    ProptestConfig {
        cases: PURE_CASES,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// Runs the property `test` over `strategy` with `config`. A failure reports the reason and the
/// minimal failing input, as the `proptest!` macro does.
fn check<S>(
    config: ProptestConfig,
    strategy: S,
    test: impl Fn(S::Value) -> Result<(), TestCaseError>,
) where
    S: Strategy,
    S::Value: std::fmt::Debug,
{
    TestRunner::new(config)
        .run(&strategy, test)
        .unwrap_or_else(|failure| panic!("{failure}"));
}

/// The value of a user agent: a string, an integer or an unforgeable name of 32 bytes. None is a
/// Reified id and none is empty, so `classify_space_ref` names an agent. A Reified id has 2 or
/// at least 34 bytes, so no name of 32 bytes is one (`Namespace.v`, `decode_32_none`).
fn arb_agent_value() -> BoxedStrategy<Par> {
    prop_oneof![
        "[a-z ]{1,12}".prop_map(|text| new_gstring_par(text, Vec::new(), false)),
        any::<i64>().prop_map(|value| new_gint_par(value, Vec::new(), false)),
        any::<[u8; 32]>().prop_map(|id| gprivate(id.to_vec())),
    ]
    .boxed()
}

/// The form of an agent reference: the value itself, or one bundle around it with any flags,
/// with or without its body.
#[derive(Clone, Debug)]
enum Form {
    Plain,
    Bundle { write: bool, read: bool, body: bool },
}

fn arb_form() -> BoxedStrategy<Form> {
    prop_oneof![
        Just(Form::Plain),
        (any::<bool>(), any::<bool>(), any::<bool>())
            .prop_map(|(write, read, body)| Form::Bundle { write, read, body }),
    ]
    .boxed()
}

/// The reference of the form `form` around `value`.
fn reference(value: &Par, form: &Form) -> Par {
    match form {
        Form::Plain => value.clone(),
        Form::Bundle {
            write,
            read,
            body: true,
        } => new_bundle_par(value.clone(), *write, *read),
        Form::Bundle {
            write,
            read,
            body: false,
        } => Par::default().with_bundles(vec![Bundle {
            body: None,
            write_flag: *write,
            read_flag: *read,
        }]),
    }
}

/// `agent_ref_requires_write_flag` (SpaceRefs.v, A1): every agent reference keeps its
/// classification of P4, with its bundle (AG-3). Its call channel is the value itself, or the
/// body of a bundle with the write flag. A bundle without the write flag, or without a body,
/// has no call channel, and the error is `HandleNotWritable`, the code that `use` and an
/// annotation of `new` raise for such a reference. The context of an agent has no native space.
#[test]
fn prop_agent_ref_requires_write_flag() {
    check(
        pure_config(),
        (arb_agent_value(), arb_form()),
        |(value, form)| {
            let agent = reference(&value, &form);
            prop_assert_eq!(
                classify_space_ref(&agent),
                Ok(SpaceRef::Agent(agent.clone()))
            );
            let expected = match form {
                Form::Plain => Ok(value.clone()),
                Form::Bundle {
                    write: true,
                    body: true,
                    ..
                } => Ok(value.clone()),
                Form::Bundle { .. } => Err(SpaceRefError::HandleNotWritable),
            };
            prop_assert_eq!(call_channel(&agent), expected.clone());
            if let Err(error) = expected {
                prop_assert_eq!(ReifiedReduceError::from(error).code(), "HandleNotWritable");
            }
            let target = UseTarget {
                space: agent,
                kind: UseKind::Agent,
            };
            prop_assert_eq!(target.native(), None);
            Ok(())
        },
    );
}

/// The body of a generated bundle (P8 step 2): none, an agent-tagged tuple, a tuple of three
/// elements whose head is another name, or a plain value. The variants box their values, so
/// the variants have one size.
#[derive(Clone, Debug)]
enum BundleBody {
    Empty,
    Tagged {
        agent: Box<Par>,
        raw: Box<Par>,
    },
    OtherHead {
        head: Box<Par>,
        agent: Box<Par>,
        raw: Box<Par>,
    },
    Value(Box<Par>),
}

/// A head of a tuple of three elements that is not the agent tag: another 2-byte Reified id,
/// a 32-byte name, or a string.
fn arb_other_head() -> BoxedStrategy<Par> {
    prop_oneof![
        (0u8..=0xFE).prop_map(|op| gprivate(vec![REIFIED_TAG, op])),
        any::<[u8; 32]>().prop_map(|id| gprivate(id.to_vec())),
        "[a-z]{1,8}".prop_map(|text| new_gstring_par(text, Vec::new(), false)),
    ]
    .boxed()
}

fn arb_bundle_body() -> BoxedStrategy<BundleBody> {
    prop_oneof![
        Just(BundleBody::Empty),
        (arb_agent_value(), arb_agent_value()).prop_map(|(agent, raw)| BundleBody::Tagged {
            agent: Box::new(agent),
            raw: Box::new(raw),
        }),
        (arb_other_head(), arb_agent_value(), arb_agent_value()).prop_map(|(head, agent, raw)| {
            BundleBody::OtherHead {
                head: Box::new(head),
                agent: Box::new(agent),
                raw: Box::new(raw),
            }
        }),
        arb_agent_value().prop_map(|value| BundleBody::Value(Box::new(value))),
    ]
    .boxed()
}

/// The tuple `(AGENT_TAG, agent, raw)` of an agent-minted name.
fn agent_tagged(agent: &Par, raw: &Par) -> Par {
    new_etuple_par(vec![
        gprivate(vec![REIFIED_TAG, AGENT_TAG]),
        agent.clone(),
        raw.clone(),
    ])
}

/// `r7_agent_reply_bundle_exact` (SpaceRefs.v, headline 14, R7A-1): for every single bundle,
/// the R7 backend routes to an agent exactly when the bundle has both flags and its body is an
/// agent-tagged tuple, and then to the agent and the raw channel of the tuple. Every other
/// bundle routes to the default space, as in HEAD. A bare agent-tagged tuple routes as the
/// reducer routes it under any context (R4, headline 7).
#[test]
fn prop_r7_agent_reply_bundle_exact() {
    check(
        pure_config(),
        (arb_bundle_body(), any::<bool>(), any::<bool>()),
        |(body, write, read)| {
            let (par, expected) = match &body {
                BundleBody::Empty => (None, Route::Default),
                BundleBody::Tagged { agent, raw } => {
                    (Some(agent_tagged(agent, raw)), match write && read {
                        true => Route::Agent {
                            agent: (**agent).clone(),
                            raw: (**raw).clone(),
                        },
                        false => Route::Default,
                    })
                }
                BundleBody::OtherHead { head, agent, raw } => (
                    Some(new_etuple_par(vec![
                        (**head).clone(),
                        (**agent).clone(),
                        (**raw).clone(),
                    ])),
                    Route::Default,
                ),
                BundleBody::Value(value) => (Some((**value).clone()), Route::Default),
            };
            let bundle = Par::default().with_bundles(vec![Bundle {
                body: par,
                write_flag: write,
                read_flag: read,
            }]);
            prop_assert_eq!(r7_route(&bundle), Ok(expected));
            if let BundleBody::Tagged { agent, raw } = &body {
                let tuple = agent_tagged(agent, raw);
                let agent_route = Route::Agent {
                    agent: (**agent).clone(),
                    raw: (**raw).clone(),
                };
                prop_assert_eq!(r7_route(&tuple), Ok(agent_route.clone()));
                prop_assert_eq!(route(&None, &tuple), Ok(agent_route));
            }
            Ok(())
        },
    );
}

// ─── P8 step 3: the translation of a receive (AgentTranslation.v) ────────────────────────────────

/// A bind pattern of `count` free variables `[x_0, …, x_(count-1)]`.
fn free_pattern(count: u32) -> BindPattern {
    BindPattern {
        patterns: (0..count)
            .map(|index| new_freevar_par(i32::try_from(index).expect("a small index"), Vec::new()))
            .collect(),
        remainder: None,
        free_count: i32::try_from(count).expect("a small count"),
    }
}

/// The free counts of 1 to 4 binds of a receive.
fn arb_counts() -> BoxedStrategy<Vec<u32>> { prop::collection::vec(0u32..4, 1..5).boxed() }

/// The translation of a receive on the agent `"agent"` whose binds have the free counts
/// `counts`, with the guard `guard`, and the states of its matchers in bind order.
fn translate(
    counts: &[u32],
    guard: Option<Par>,
) -> Result<(ReceiveTranslation, Vec<MatcherState>), TestCaseError> {
    let binds = counts
        .iter()
        .enumerate()
        .map(|(index, count)| AgentBind {
            pattern: free_pattern(*count),
            raw: new_gint_par(
                i64::try_from(index).expect("a small index"),
                Vec::new(),
                false,
            ),
        })
        .collect();
    let translation = translate_receive(
        &new_gstring_par("agent".to_string(), Vec::new(), false),
        binds,
        ParWithRandom {
            body: Some(Par::default()),
            random_state: Blake2b512Random::create_from_bytes(b"receive").to_bytes(),
        },
        false,
        false,
        guard,
        &CostAuthority::default(),
    )
    .map_err(|error| TestCaseError::fail(format!("the translation fails: {error:?}")))?;
    let states = translation
        .matchers
        .iter()
        .map(|matcher| {
            matcher_state_of(&matcher.channel)
                .map_err(|error| TestCaseError::fail(format!("{error:?}")))?
                .ok_or_else(|| TestCaseError::fail("a matcher id decodes to its state"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((translation, states))
}

/// `offsets_partition` (AgentTranslation.v, A3): the state of bind `i` holds the index `i`,
/// the bind count `n`, the total of the free counts, and the offset `off_i`, the sum of the free
/// counts before bind `i`. So the ranges `[off_i, off_i + f_i)` partition `[0, total)`.
#[test]
fn prop_offsets_partition() {
    check(pure_config(), arb_counts(), |counts| {
        let (_, states) = translate(&counts, None)?;
        let total: u32 = counts.iter().sum();
        let mut owners = vec![None; usize::try_from(total).expect("a small total")];
        let mut offset = 0u32;
        for (index, (state, count)) in states.iter().zip(&counts).enumerate() {
            prop_assert_eq!(usize::try_from(state.bind).ok(), Some(index));
            prop_assert_eq!(usize::try_from(state.binds).ok(), Some(counts.len()));
            prop_assert_eq!(state.total, total);
            prop_assert_eq!(state.offset, offset);
            prop_assert_eq!(state.free_count(), Some(*count));
            for position in offset..offset + count {
                let slot = &mut owners[usize::try_from(position).expect("a small position")];
                prop_assert_eq!(*slot, None, "position {} has two owners", position);
                *slot = Some(index);
            }
            offset += count;
        }
        prop_assert!(owners.iter().all(Option::is_some));
        Ok(())
    });
}

/// The keys and the free variables of the map pattern of a body receive.
fn body_keys(translation: &ReceiveTranslation) -> Result<Vec<(String, i32)>, TestCaseError> {
    let [pattern] = translation.body.pattern.patterns.as_slice() else {
        return Err(TestCaseError::fail("the body receive has one pattern"));
    };
    let map = match pattern.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EMapBody(map)),
        }] => map,
        other => return Err(TestCaseError::fail(format!("the pattern is {other:?}"))),
    };
    map.kvs
        .iter()
        .map(|pair| match (&pair.key, &pair.value) {
            (Some(key), Some(value)) => match (key.exprs.as_slice(), value.exprs.as_slice()) {
                (
                    [Expr {
                        expr_instance: Some(ExprInstance::GString(key)),
                    }],
                    [Expr {
                        expr_instance:
                            Some(ExprInstance::EVarBody(EVar {
                                v:
                                    Some(Var {
                                        var_instance: Some(VarInstance::FreeVar(index)),
                                    }),
                            })),
                    }],
                ) => Ok((key.clone(), *index)),
                other => Err(TestCaseError::fail(format!("an entry is {other:?}"))),
            },
            other => Err(TestCaseError::fail(format!("an entry is {other:?}"))),
        })
        .collect()
}

/// `env_keys_disjoint_union` (AgentTranslation.v, A4): the env keys of bind `i` are the decimal
/// strings of `off_i + j` for `j < f_i`. These key sets are disjoint and their union is the key
/// set of the map pattern of the body receive, `{"0", …, "total - 1"}`, whose key `"g"` binds
/// `FreeVar(g)`.
#[test]
fn prop_env_keys_disjoint_union() {
    check(pure_config(), arb_counts(), |counts| {
        let (translation, states) = translate(&counts, None)?;
        let mut env_keys = std::collections::BTreeSet::new();
        for state in &states {
            for position in state.offset..state.offset + state.free_count().unwrap_or(0) {
                prop_assert!(env_keys.insert(position.to_string()), "a key repeats");
            }
        }
        let mut body = body_keys(&translation)?;
        body.sort_by_key(|(_, index)| *index);
        let total = i32::try_from(counts.iter().sum::<u32>()).expect("a small total");
        prop_assert_eq!(
            body.iter().map(|(_, index)| *index).collect::<Vec<_>>(),
            (0..total).collect::<Vec<_>>()
        );
        for (key, index) in &body {
            prop_assert_eq!(key, &index.to_string());
        }
        prop_assert_eq!(
            body.into_iter()
                .map(|(key, _)| key)
                .collect::<std::collections::BTreeSet<_>>(),
            env_keys
        );
        prop_assert_eq!(translation.body.pattern.free_count, total);
        Ok(())
    });
}

/// The guard `BoundVar(k) == 1` for each `k` of `indices`, joined with `and`.
fn guard_over(indices: &[u32]) -> Par {
    let eq = |index: u32| {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EEqBody(EEq {
                p1: Some(new_boundvar_par(
                    i32::try_from(index).expect("a small index"),
                    Vec::new(),
                    false,
                )),
                p2: Some(new_gint_par(1, Vec::new(), false)),
            })),
        }])
    };
    indices
        .iter()
        .map(|index| eq(*index))
        .reduce(|left, right| {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EAndBody(EAnd {
                    p1: Some(left),
                    p2: Some(right),
                })),
            }])
        })
        .unwrap_or_else(|| new_gint_par(1, Vec::new(), false))
}

/// `guard_fold_sound` (AgentTranslation.v, A6), the placement part: the free indices of a
/// guard are the indices of its `BoundVar`s, and the guard goes to the one bind that owns the
/// binder of each index below the total (`BoundVar(k)` reads position `total - 1 - k`), to bind
/// 0 when no index is below the total, and aborts with `AgentGuardSpansJoin` for two owners.
/// The translation puts the guard into the state of that bind only.
#[test]
fn prop_guard_placement() {
    check(
        pure_config(),
        (arb_counts(), prop::collection::vec(0u32..12, 0..4)),
        |(counts, indices)| {
            let guard = guard_over(&indices);
            let found =
                guard_indices(&guard).map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
            prop_assert_eq!(
                found.iter().copied().collect::<Vec<_>>(),
                indices
                    .iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            );
            let total: u32 = counts.iter().sum();
            let mut owners = std::collections::BTreeSet::new();
            for index in &found {
                if *index < total {
                    let position = total - 1 - index;
                    let mut offset = 0u32;
                    for (bind, count) in counts.iter().enumerate() {
                        if offset <= position && position < offset + count {
                            owners.insert(bind);
                        }
                        offset += count;
                    }
                }
            }
            let expected = match owners.len() {
                0 => Ok(0),
                1 => Ok(*owners.iter().next().expect("one owner")),
                _ => Err("AgentGuardSpansJoin".to_string()),
            };
            let placed = place_guard(&found, &counts).map_err(|error| match error {
                // Changed by Reified RSpaces (user decision of 2026-10-25, as DR-128): the code
                // `AgentGuardSpansJoin` is a user failure, so it is a `SpaceUserError`.
                // InterpreterError::ReduceError(message) => message
                InterpreterError::ReduceError(message)
                | InterpreterError::SpaceUserError(message) => message
                    .strip_prefix("reified:")
                    .and_then(|rest| rest.split(':').next())
                    .unwrap_or_default()
                    .to_string(),
                other => format!("{other:?}"),
            });
            prop_assert_eq!(&placed, &expected);
            if let Ok(owner) = expected {
                let (_, states) = translate(&counts, Some(guard.clone()))?;
                for (bind, state) in states.iter().enumerate() {
                    let expected_guard = (bind == owner).then(|| guard.clone());
                    prop_assert_eq!(&state.guard, &expected_guard, "bind {}", bind);
                }
            }
            Ok(())
        },
    );
}

/// A consistent matcher state: a tag, the coordinates of one bind among `binds`, its pattern,
/// and an optional guard.
fn arb_state() -> BoxedStrategy<MatcherState> {
    (
        any::<[u8; 32]>(),
        arb_counts(),
        any::<prop::sample::Index>(),
        prop::option::of(any::<i64>()),
    )
        .prop_map(|(tag, counts, which, guard)| {
            let bind = which.index(counts.len());
            let offset: u32 = counts[..bind].iter().sum();
            MatcherState {
                receive_tag: tag,
                bind: u32::try_from(bind).expect("a small bind"),
                offset,
                total: counts.iter().sum(),
                binds: u32::try_from(counts.len()).expect("a small count"),
                pattern: free_pattern(counts[bind]),
                guard: guard.map(|value| new_gint_par(value, Vec::new(), false)),
            }
        })
        .boxed()
}

/// `matcher_state_roundtrip` (AgentTranslation.v, A8): a consistent state decodes from its
/// encoding and from its matcher id; the decoder is strict, so bytes that decode re-encode to
/// themselves (a flipped byte decodes to nothing or to the state of those bytes); and two
/// different states have different encodings.
#[test]
fn prop_matcher_state_roundtrip() {
    check(
        pure_config(),
        (
            arb_state(),
            arb_state(),
            any::<prop::sample::Index>(),
            1u8..=255,
        ),
        |(state, other, at, flip)| {
            let bytes = state.encode().expect("a state encodes");
            prop_assert_eq!(
                MatcherState::decode(&bytes).expect("the decode runs"),
                Some(state.clone())
            );
            let id =
                gprivate(rholang::rust::interpreter::spaces::namespace::matcher_channel(&bytes));
            prop_assert_eq!(
                matcher_state_of(&id).expect("the decode runs"),
                Some(state.clone())
            );
            let mut mutated = bytes.clone();
            let position = at.index(mutated.len());
            mutated[position] ^= flip;
            if let Some(decoded) = MatcherState::decode(&mutated).expect("the decode runs") {
                prop_assert_eq!(decoded.encode().expect("a state encodes"), mutated);
            }
            let other_bytes = other.encode().expect("a state encodes");
            prop_assert_eq!(state == other, bytes == other_bytes);
            Ok(())
        },
    );
}

/// The use-contexts of a route: none, the context of a native space, and the context of a user
/// agent.
fn contexts(agent: &Par) -> Vec<SpaceCtx> {
    let space = NativeSpace {
        sid: [7; SID_LEN],
        durability: Durability::Durable,
    };
    vec![
        None,
        Some(Arc::new(UseTarget {
            space: gprivate(handle_channel(space)),
            kind: UseKind::Native(space),
        })),
        Some(Arc::new(UseTarget {
            space: agent.clone(),
            kind: UseKind::Agent,
        })),
    ]
}

/// `tag_untag_id` (AgentTranslation.v, A9, P8 step 4): the agent-minted name of a raw channel
/// is a bundle with both flags around the agent-tagged tuple of the agent and the raw channel
/// (design v2 TN-3). The write rule of `eval_send` and the read rule of `unbundle_receive` both
/// give the tuple, and R4 routes the tuple to its agent with its raw channel under every
/// use-context, also inside the context of another agent.
#[test]
fn prop_tag_untag_id() {
    check(
        pure_config(),
        (
            arb_agent_value(),
            arb_form(),
            arb_agent_value(),
            arb_agent_value(),
        ),
        |(value, form, raw, other)| {
            let agent = reference(&value, &form);
            let name = agent_minted_name(agent.clone(), raw.clone());
            let [bundle] = name.bundles.as_slice() else {
                return Err(TestCaseError::fail("an agent-minted name is one bundle"));
            };
            prop_assert!(bundle.write_flag && bundle.read_flag);
            let tuple = bundle.body.clone().expect("a bundle body");
            for context in contexts(&other) {
                match route(&context, &tuple) {
                    Ok(Route::Agent {
                        agent: routed,
                        raw: routed_raw,
                    }) => {
                        prop_assert_eq!(&routed, &agent);
                        prop_assert_eq!(&routed_raw, &raw);
                    }
                    other => {
                        return Err(TestCaseError::fail(format!(
                            "the tuple of an agent-minted name routes to {other:?}"
                        )))
                    }
                }
            }
            Ok(())
        },
    );
}

/// The substitution of the reducer, with an unlimited budget.
fn substitution() -> Substitute {
    Substitute {
        metering: MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max())),
    }
}

/// `par` after the substitution of `env`, sorted, as `substitute_and_charge` gives it.
fn substituted(par: &Par, env: &Env<Par>) -> Par {
    substitution()
        .substitute(par.clone(), 0, env)
        .expect("the substitution succeeds")
}

/// The send of `data` on `chan`, with the bit sets of its data.
fn send_par(chan: Par, data: Vec<Par>) -> Par {
    let locally_free = data.iter().fold(Vec::new(), |bits, par| {
        union(bits, par.locally_free.clone())
    });
    Par::default()
        .with_sends(vec![Send {
            chan: Some(chan),
            data,
            persistent: false,
            locally_free: locally_free.clone(),
            connective_used: false,
            params: None,
        }])
        .with_locally_free(locally_free)
}

/// The reply name `r_a` of the agent binder `a` of a translated `new` under `base` (design v2
/// TN-2): `GPrivate(base.split_byte(5).split_short(a).next())`.
fn reply_name(base: &Blake2b512Random, binder: i16) -> Par {
    gprivate(
        base.split_byte(REPLY_NAME_BRANCH)
            .split_short(binder)
            .next()
            .into_iter()
            .map(|byte| byte as u8)
            .collect(),
    )
}

/// One generated `new`: its plain binders and URI binders, the agent binders among the plain
/// binders, and the reference of their agent.
#[derive(Clone, Debug)]
struct NewCase {
    plain: usize,
    uris: usize,
    agents: Vec<bool>,
    agent: Par,
}

fn arb_new_case() -> BoxedStrategy<NewCase> {
    (
        1usize..=3,
        0usize..=1,
        prop::collection::vec(any::<bool>(), 3),
        (arb_agent_value(), any::<bool>()),
    )
        .prop_map(|(plain, uris, mut agents, (value, bundled))| {
            agents.truncate(plain);
            if !agents.iter().any(|agent| *agent) {
                agents[plain - 1] = true;
            }
            let agent = match bundled {
                true => new_bundle_par(value, true, false),
                false => value,
            };
            NewCase {
                plain,
                uris,
                agents,
                agent,
            }
        })
        .boxed()
}

/// `resume_binds_innermost` (AgentTranslation.v, A10, P8 step 4, design v2 TN-3): the resume
/// term of a translated `new` binds every binder of the `new`, in its order. The property builds
/// the translation of a generated `new` whose body sends its binders, and runs the resume with
/// the substitution of the reducer: the nested receives bind the replies in the order of the
/// agent binders, and the match binds the binders. The body then sends, in binder order, the
/// agent-minted name of each reply for an agent binder and the bound name for every other
/// binder. The calls name the reply names of TN-2 on the call channel of the agent (TN-4).
#[test]
fn prop_resume_binds_innermost() {
    let out = gprivate(vec![30; SID_LEN]);
    check(pure_config(), arb_new_case(), |case| {
        let bind_count = case.plain + case.uris;
        let outer = Env::new()
            .put(new_gint_par(100, Vec::new(), false))
            .put(new_gint_par(101, Vec::new(), false));
        let mut bound_env = outer.clone();
        let mut values = Vec::with_capacity(bind_count);
        for binder in 0..bind_count {
            let value = gprivate(vec![
                40 + u8::try_from(binder).expect("a small new");
                SID_LEN
            ]);
            bound_env = bound_env.put(value.clone());
            values.push(value);
        }
        let binders: Vec<AgentBinder> = case
            .agents
            .iter()
            .enumerate()
            .filter(|(_, agent)| **agent)
            .map(|(index, _)| AgentBinder {
                index,
                agent: case.agent.clone(),
            })
            .collect();
        let body = send_par(
            out.clone(),
            (0..bind_count)
                .rev()
                .map(|level| {
                    new_boundvar_par(
                        i32::try_from(level).expect("a small new"),
                        Vec::new(),
                        false,
                    )
                })
                .collect(),
        );
        let base =
            RandomPurpose::Xlate.derive(&Blake2b512Random::create_from_bytes(b"rrs-p8-resume"));
        let translation = translate_new(&bound_env, outer.level, bind_count, &binders, body, &base)
            .expect("the translation builds");
        let replies: Vec<Par> = binders
            .iter()
            .map(|binder| reply_name(&base, i16::try_from(binder.index).expect("a small new")))
            .collect();
        let raws: Vec<Par> = (0..binders.len())
            .map(|rank| {
                new_gint_par(
                    500 + i64::try_from(rank).expect("a small new"),
                    Vec::new(),
                    false,
                )
            })
            .collect();
        // TN-4: one call for each agent binder, on the call channel of the agent.
        let channel = call_channel(&case.agent).expect("a writable agent reference");
        prop_assert_eq!(translation.par.sends.len(), binders.len());
        for (call, reply) in translation.par.sends.iter().zip(&replies) {
            prop_assert_eq!(call.chan.as_ref(), Some(&channel));
            prop_assert_eq!(&call.data, &vec![
                reply.clone(),
                new_gstring_par("gensym".to_string(), Vec::new(), false)
            ]);
        }
        // TN-3: the receives nest in the order of the agent binders.
        let [resume] = translation.par.receives.as_slice() else {
            return Err(TestCaseError::fail("one resume receive"));
        };
        let mut receive = resume.clone();
        let mut replied = Env::new();
        for (rank, reply) in replies.iter().enumerate() {
            prop_assert_eq!(receive.binds[0].source.as_ref(), Some(reply));
            replied = replied.put(raws[rank].clone());
            let inner = receive.body.clone().expect("a resume body");
            if rank + 1 < replies.len() {
                let [next] = inner.receives.as_slice() else {
                    return Err(TestCaseError::fail("a resume receive nests the next"));
                };
                receive = next.clone();
                continue;
            }
            let [matched] = inner.matches.as_slice() else {
                return Err(TestCaseError::fail("the innermost body is one match"));
            };
            let target = substituted(matched.target.as_ref().expect("a target"), &replied);
            let bindings: Vec<Par> = match target.exprs.as_slice() {
                [Expr {
                    expr_instance: Some(ExprInstance::ETupleBody(ETuple { ps, .. })),
                }] => ps.clone(),
                other => return Err(TestCaseError::fail(format!("the target is {other:?}"))),
            };
            let mut expected = Vec::with_capacity(bind_count);
            let mut next_rank = 0;
            for (binder, value) in values.iter().enumerate() {
                match case.agents.get(binder) {
                    Some(true) => {
                        expected.push(substituted(
                            &agent_minted_name(case.agent.clone(), raws[next_rank].clone()),
                            &Env::new(),
                        ));
                        next_rank += 1;
                    }
                    Some(false) | None => expected.push(substituted(value, &Env::new())),
                }
            }
            prop_assert_eq!(&bindings, &expected);
            let source = matched.cases[0].source.as_ref().expect("a case body");
            let mut matched_env = replied.clone();
            for binding in &bindings {
                matched_env = matched_env.put(binding.clone());
            }
            prop_assert_eq!(
                substituted(source, &matched_env),
                substituted(&send_par(out.clone(), expected), &Env::new())
            );
            return Ok(());
        }
        Err(TestCaseError::fail("the resume has no match"))
    });
}

// ─── Step 6: the properties of AgentSpaces.tla ─────────────────────────────────────────────────

/// A channel of one routing class (route.rs, AgentSpaces.tla). The variants box their values,
/// so the variants have one size.
#[derive(Clone, Debug)]
enum RoutedChannel {
    /// A system channel: a private name of one or two bytes.
    System(Vec<u8>),
    /// A default object: the handle of a native space.
    Handle(Box<NativeSpace>),
    /// An unforgeable name of 32 bytes. A Reified id has 2 or at least 34 bytes.
    Unforgeable(Box<[u8; SID_LEN]>),
    /// A name of a native space, with a random tail.
    SpaceName(Box<(NativeSpace, [u8; SID_LEN])>),
    /// The agent-tagged tuple of an agent-minted name.
    Minted { agent: Box<Par>, raw: Box<Par> },
    /// A forged channel: a string or an integer.
    Forged(Box<Par>),
}

fn arb_native_space() -> BoxedStrategy<NativeSpace> {
    (any::<[u8; SID_LEN]>(), prop_oneof![
        Just(Durability::Durable),
        Just(Durability::Ephemeral)
    ])
        .prop_map(|(sid, durability)| NativeSpace { sid, durability })
        .boxed()
}

fn arb_forged() -> BoxedStrategy<Par> {
    prop_oneof![
        "[a-z]{1,8}".prop_map(|text| new_gstring_par(text, Vec::new(), false)),
        any::<i64>().prop_map(|value| new_gint_par(value, Vec::new(), false)),
    ]
    .boxed()
}

fn arb_routed_channel() -> BoxedStrategy<RoutedChannel> {
    prop_oneof![
        prop::collection::vec(any::<u8>(), 1..3).prop_map(RoutedChannel::System),
        arb_native_space().prop_map(|space| RoutedChannel::Handle(Box::new(space))),
        any::<[u8; SID_LEN]>().prop_map(|id| RoutedChannel::Unforgeable(Box::new(id))),
        (arb_native_space(), any::<[u8; SID_LEN]>())
            .prop_map(|named| RoutedChannel::SpaceName(Box::new(named))),
        (arb_agent_value(), arb_form(), arb_agent_value()).prop_map(|(value, form, raw)| {
            RoutedChannel::Minted {
                agent: Box::new(reference(&value, &form)),
                raw: Box::new(raw),
            }
        }),
        arb_forged().prop_map(|channel| RoutedChannel::Forged(Box::new(channel))),
    ]
    .boxed()
}

/// The channel `Par` of a routed channel.
fn routed_channel_par(channel: &RoutedChannel) -> Par {
    match channel {
        RoutedChannel::System(id) => gprivate(id.clone()),
        RoutedChannel::Handle(space) => gprivate(handle_channel(**space)),
        RoutedChannel::Unforgeable(id) => gprivate(id.to_vec()),
        RoutedChannel::SpaceName(named) => gprivate(name_id(named.0, NameTail::Random(named.1))),
        RoutedChannel::Minted { agent, raw } => agent_tagged(agent, raw),
        RoutedChannel::Forged(channel) => (**channel).clone(),
    }
}

/// `RoutingIsolation` and `SystemChannelsDefault` (AgentSpaces.tla, A11 and A12): under every
/// use-context, also the context of an agent, a channel routes to its owner. System channels,
/// handles and other unforgeable names route to the default space (R1). The name of a native
/// space routes to that space (R3), and an agent-tagged tuple routes to its agent with its raw
/// channel (R4), so no name of one space reaches another space. Only a forged channel takes
/// the space of the context (R5).
#[test]
fn prop_routes_follow_the_owner_of_the_channel() {
    check(
        pure_config(),
        (arb_routed_channel(), arb_agent_value()),
        |(channel, context_agent)| {
            let par = routed_channel_par(&channel);
            for context in contexts(&context_agent) {
                let routed = route(&context, &par);
                match &channel {
                    RoutedChannel::System(_)
                    | RoutedChannel::Handle(_)
                    | RoutedChannel::Unforgeable(_) => {
                        prop_assert_eq!(routed, Ok(Route::Default));
                    }
                    RoutedChannel::SpaceName(named) => match routed {
                        Ok(Route::Space { space, .. }) => prop_assert_eq!(space, named.0),
                        other => {
                            return Err(TestCaseError::fail(format!(
                                "a space name routes to {other:?}"
                            )))
                        }
                    },
                    RoutedChannel::Minted { agent, raw } => {
                        prop_assert_eq!(
                            routed,
                            Ok(Route::Agent {
                                agent: (**agent).clone(),
                                raw: (**raw).clone(),
                            })
                        );
                    }
                    RoutedChannel::Forged(_) => match context.as_deref() {
                        None => prop_assert_eq!(routed, Ok(Route::Default)),
                        Some(UseTarget {
                            kind: UseKind::Native(native),
                            ..
                        }) => match routed {
                            Ok(Route::Space { space, .. }) => prop_assert_eq!(space, *native),
                            other => {
                                return Err(TestCaseError::fail(format!(
                                    "a forged channel under a native context routes to {other:?}"
                                )))
                            }
                        },
                        Some(UseTarget {
                            space,
                            kind: UseKind::Agent,
                        }) => prop_assert_eq!(
                            routed,
                            Ok(Route::Agent {
                                agent: space.clone(),
                                raw: par.clone(),
                            })
                        ),
                    },
                }
            }
            Ok(())
        },
    );
}

/// The base of a generated space reference: the handle of a native space or an agent value.
#[derive(Clone, Debug)]
enum RefBase {
    Handle(Box<NativeSpace>),
    Agent(Box<Par>),
}

/// `HandleUnwrapExact` (AgentSpaces.tla, A14): a reference names a space exactly when it is
/// bare or under one bundle with the write flag, and the space is the unwrapped reference. A
/// handle, bare or under a bundle with the write flag, is its native space, and under a bundle
/// without the write flag it is `HandleNotWritable`. An agent reference keeps its bundle, and
/// its call channel is the reference without one bundle, which needs the write flag. A second
/// bundle is never removed: a handle under two bundles is an agent whose call channel is the
/// inner bundle.
#[test]
fn prop_space_ref_unwraps_one_bundle() {
    check(
        pure_config(),
        (
            prop_oneof![
                arb_native_space().prop_map(|space| RefBase::Handle(Box::new(space))),
                arb_agent_value().prop_map(|value| RefBase::Agent(Box::new(value))),
            ],
            prop::collection::vec((any::<bool>(), any::<bool>()), 0..3),
        ),
        |(base, layers)| {
            let base_par = match &base {
                RefBase::Handle(space) => gprivate(handle_channel(**space)),
                RefBase::Agent(value) => (**value).clone(),
            };
            // The layers are outermost first: wrap the innermost first.
            let reference = layers
                .iter()
                .rev()
                .fold(base_par.clone(), |inner, (write, read)| {
                    new_bundle_par(inner, *write, *read)
                });
            let classified = classify_space_ref(&reference);
            match (layers.as_slice(), &base) {
                ([], RefBase::Handle(space)) => {
                    prop_assert_eq!(classified, Ok(SpaceRef::Native(**space)))
                }
                ([(true, _)], RefBase::Handle(space)) => {
                    prop_assert_eq!(classified, Ok(SpaceRef::Native(**space)))
                }
                ([(false, _)], RefBase::Handle(_)) => {
                    prop_assert_eq!(classified, Err(SpaceRefError::HandleNotWritable))
                }
                _ => {
                    prop_assert_eq!(classified, Ok(SpaceRef::Agent(reference.clone())));
                    let expected = match layers.as_slice() {
                        [] => Ok(base_par.clone()),
                        [(true, _), inner @ ..] => Ok(inner
                            .iter()
                            .rev()
                            .fold(base_par.clone(), |body, (write, read)| {
                                new_bundle_par(body, *write, *read)
                            })),
                        [(false, _), ..] => Err(SpaceRefError::HandleNotWritable),
                    };
                    prop_assert_eq!(call_channel(&reference), expected);
                }
            }
            Ok(())
        },
    );
}
