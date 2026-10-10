// P8 property tests of the spaces that user agents implement (plan decision 77, design v2).
//
// Each property test has the name that formal/reified-rspaces-invariant-map.json maps to a
// headline of phase P8. Step 1 maps headline A1, `agent_ref_requires_write_flag` of
// formal/rocq/reified_rspaces/theories/SpaceRefs.v: a call on a user agent is an ordinary send,
// so the reference of an agent follows the bundle rule of `eval_send` (AG-4).

use models::rhoapi::{Bundle, Par};
use models::rust::utils::{new_bundle_par, new_gint_par, new_gstring_par};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, RngSeed, TestCaseError, TestRunner};
use rholang::rust::interpreter::reified::errors::ReifiedReduceError;
use rholang::rust::interpreter::reified::scope::{
    call_channel, classify_space_ref, SpaceRef, SpaceRefError, UseKind, UseTarget,
};
use rholang::rust::interpreter::spaces::namespace::gprivate;

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
