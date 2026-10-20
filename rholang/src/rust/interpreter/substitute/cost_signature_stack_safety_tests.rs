//! DR-131 P1: stack-safety tests of the cost-signature substitution.
//!
//! Each deep test substitutes a compound chain that is nested 100,000 levels
//! deep on a thread with a small stack. The expected value comes from a
//! shallow input, so the tests need no deep recursion of their own. The deep
//! inputs are built without recursion, and the substitution consumes them. The
//! tests live in their own file, as the G1-1 tests do
//! (`accounting/delta_sigma/stack_safety_tests.rs`).

use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::CostSignatureCompound;
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
use models::rust::utils::{new_boundvar_par, new_freevar_par, new_gint_par};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::metering::MeteredMachine;
use crate::rust::interpreter::substitute_oracle::signature_recursive;

/// The nesting depth of the deep tests. It is far beyond the depth at which a
/// recursive substitution exhausts a small thread stack.
const DEEP_NESTING: usize = 100_000;

/// The stack of the thread that substitutes a deep chain.
const SMALL_STACK_BYTES: usize = 256 * 1024;

fn substitute_instance() -> Substitute {
    Substitute {
        metering: MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max())),
    }
}

fn ground(tag: u8) -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::Ground(vec![tag])),
    }
}

fn unit() -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::Unit(true)),
    }
}

fn compound(elements: Vec<CostSignature>) -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::Compound(CostSignatureCompound {
            elements,
        })),
    }
}

/// `depth` compounds, each holding only the next one. The innermost holds
/// `leaf`.
fn single_element_chain(depth: usize, leaf: CostSignature) -> CostSignature {
    let mut chain = leaf;
    for _ in 0..depth {
        chain = compound(vec![chain]);
    }
    chain
}

/// `depth` compounds that each hold a unit and then the next compound. The
/// innermost holds only `leaf`.
fn unit_per_level_chain(depth: usize, leaf: CostSignature) -> CostSignature {
    let mut chain = compound(vec![leaf]);
    for _ in 1..depth {
        chain = compound(vec![unit(), chain]);
    }
    chain
}

/// Substitutes `signature` at depth 0 in an empty environment, on a thread
/// whose stack is `SMALL_STACK_BYTES`.
fn substitute_on_small_stack(signature: CostSignature) -> CostSignature {
    std::thread::Builder::new()
        .name("cost-signature-substitution-small-stack".to_string())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(move || {
            substitute_instance()
                .substitute_cost_signature(signature, 0, &Env::new())
                .expect("the substitution of a ground chain succeeds")
        })
        .expect("spawn the small-stack thread")
        .join()
        .expect("the substitution stays within a small stack")
}

#[test]
fn a_single_element_chain_one_hundred_thousand_deep_substitutes_on_a_small_stack() {
    let substituted = substitute_on_small_stack(single_element_chain(DEEP_NESTING, ground(7)));
    assert_eq!(substituted, sort_signature(&ground(7)).term);
}

#[test]
fn a_unit_per_level_chain_keeps_its_only_ground_on_a_small_stack() {
    let substituted = substitute_on_small_stack(unit_per_level_chain(DEEP_NESTING, ground(9)));
    assert_eq!(substituted, sort_signature(&ground(9)).term);
}

fn environment() -> Env<Par> {
    Env::new()
        .put(new_gint_par(40, Vec::new(), false))
        .put(new_gint_par(41, Vec::new(), false))
}

#[test]
fn the_first_missing_value_stops_the_substitution_as_the_oracle_does() {
    let signature = compound(vec![
        ground(1),
        compound(vec![CostSignature { value: None }, ground(2)]),
        CostSignature { value: None },
    ]);
    let substitute = substitute_instance();
    let env = environment();
    let machine = substitute.substitute_cost_signature(signature.clone(), 0, &env);
    let oracle = signature_recursive(signature, SubCtx::root(0), EnvView::new(&env));
    assert!(matches!(
        machine,
        Err(InterpreterError::UndefinedRequiredProtobufFieldError(_))
    ));
    assert_eq!(machine, oracle);
}

fn leaf_signature() -> impl Strategy<Value = CostSignature> {
    prop_oneof![
        prop::collection::vec(any::<u8>(), 0..3).prop_map(|bytes| CostSignature {
            value: Some(CostSignatureValue::Ground(bytes)),
        }),
        any::<bool>().prop_map(|value| CostSignature {
            value: Some(CostSignatureValue::Unit(value)),
        }),
        (0..4_i32).prop_map(|level| CostSignature {
            value: Some(CostSignatureValue::BoundLevel(level)),
        }),
        Just(CostSignature { value: None }),
        (0..3_i32).prop_map(|level| CostSignature {
            value: Some(CostSignatureValue::Quote(new_boundvar_par(
                level,
                Vec::new(),
                false
            ))),
        }),
        (0..3_i64).prop_map(|value| CostSignature {
            value: Some(CostSignatureValue::Name(new_gint_par(
                value,
                Vec::new(),
                false
            ))),
        }),
    ]
}

fn nested_signature() -> impl Strategy<Value = CostSignature> {
    leaf_signature().prop_recursive(6, 64, 4, |inner| {
        prop::collection::vec(inner, 0..4).prop_map(compound)
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn the_work_stack_substitution_equals_the_recursive_substitution(
        signature in nested_signature(),
        depth in 0..2_i32,
    ) {
        let substitute = substitute_instance();
        let env = environment();
        let machine = substitute.substitute_cost_signature(signature.clone(), depth, &env);
        let oracle = signature_recursive(signature, SubCtx::root(depth), EnvView::new(&env));
        prop_assert_eq!(machine, oracle);
    }
}

#[test]
fn measure_wrappers_return_the_charged_prost_length() {
    use prost::Message;
    let env = environment();
    for term in [
        new_boundvar_par(0, Vec::new(), false),
        new_gint_par(7, Vec::new(), false),
        new_freevar_par(0, Vec::new()),
    ] {
        for sorted in [false, true] {
            let measuring = substitute_instance();
            let charging = substitute_instance();
            let measured = if sorted {
                measuring.substitute_and_measure(term.clone(), 0, &env)
            } else {
                measuring.substitute_no_sort_and_measure(term.clone(), 0, &env)
            };
            let charged = if sorted {
                charging.substitute_and_charge(term.clone(), 0, &env)
            } else {
                charging.substitute_no_sort_and_charge(term.clone(), 0, &env)
            };
            match (measured, charged) {
                (Ok((measured, bytes)), Ok(charged)) => {
                    assert_eq!(measured, charged);
                    assert_eq!(bytes, u64::try_from(measured.encoded_len()).unwrap());
                }
                (Err(measured), Err(charged)) => assert_eq!(measured, charged),
                other => panic!("the wrappers disagree: {other:?}"),
            }
            assert_eq!(
                measuring.metering.budget().remaining(),
                charging.metering.budget().remaining()
            );
        }
    }
}
