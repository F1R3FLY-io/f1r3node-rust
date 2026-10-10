// P8 property tests of the spaces that user agents implement (plan decision 77, design v2).
//
// Each property test has the name that formal/reified-rspaces-invariant-map.json maps to a
// headline of phase P8. Step 1 maps headline A1, `agent_ref_requires_write_flag` of
// formal/rocq/reified_rspaces/theories/SpaceRefs.v: a call on a user agent is an ordinary send,
// so the reference of an agent follows the bundle rule of `eval_send` (AG-4). Step 2 maps
// headline A2, `r7_agent_reply_bundle_exact` of the same module: the R7 backend calls an agent
// for exactly the bundle with both flags around an agent-tagged tuple (R7A-1).

use models::rhoapi::{Bundle, Par};
use models::rust::utils::{new_bundle_par, new_etuple_par, new_gint_par, new_gstring_par};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, RngSeed, TestCaseError, TestRunner};
use rholang::rust::interpreter::reified::errors::ReifiedReduceError;
use rholang::rust::interpreter::reified::execution::r7_route;
use rholang::rust::interpreter::reified::route::{route, Route};
use rholang::rust::interpreter::reified::scope::{
    call_channel, classify_space_ref, SpaceRef, SpaceRefError, UseKind, UseTarget,
};
use rholang::rust::interpreter::spaces::namespace::{gprivate, AGENT_TAG, REIFIED_TAG};

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
