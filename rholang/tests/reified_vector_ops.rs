//! The suite of `rho:lang:vector` (P6 step 6, design v3 4.7; decisions 54, 57, 62 and 66).
//!
//! A call `vec!?(op, x…)` replies `[true, value]` or `[false, code, detail]` on its reply
//! channel. The value of `add`, `scale` and `dot` is exact and has the widest kind of the inputs
//! (integer, then rational, then float). `norm` and `distance` give the correctly rounded `f64`
//! of the exact root.
//!
//! | Test | What it checks |
//! |---|---|
//! | `vector_ops_source_programs_compute_each_operation` | Each of the five operations replies its value to a Rholang program. |
//! | `vector_ops_keep_the_widest_kind_in_source` | Integers give an integer (`GBigInt` beyond `i64`), a rational or fixed-point input gives a `GBigRat`, and a float input gives an `f64`. |
//! | `vector_ops_are_exact_before_the_rounding` | `[1e16, 1.0, -1e16] · [1, 1, 1]` is `1.0`, where `f64` accumulation gives `0.0`. |
//! | `vector_ops_failures_reply_their_codes` | Each failure that a program can make replies its code, and the deploy goes on. |
//! | `vector_ops_try_catch_reads_the_reply` | The `try … catch` sugar reads the value and the code of a reply. |
//! | `vector_ops_params_and_persistent_sends_abort` | A call with params aborts with `ParamsUnsupported`, and a persistent call with `PersistentOpCall`. |
//! | `vector_ops_native_replay_equal` | The five operations play natively at the v6 limits, and native replay reproduces the play. |
//!
//! The properties call the operations directly and compare them with independent oracles:
//! `BigRational` arithmetic for the exact kinds, `f64` arithmetic on inputs whose results are
//! exact in `f64`, the dot laws (S6 of `Similarity.v`), the distance laws (S7), and the
//! correct rounding of each root.

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, GBigRational, GFixedPoint, Par};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::tensor::ops::{evaluate, OpFailure, ReplyCode};
use support::native::{assert_reproduces, certify, native_play, AMPLE_LIMIT, OUT as NATIVE_OUT};
use support::props::{check, pure_config};
use support::session::{Injections, Session};

// ─── Values ──────────────────────────────────────────────────────────────────────────────────

fn expr(instance: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Par::default()
    }
}

fn int(value: i64) -> Par { expr(ExprInstance::GInt(value)) }

fn flag(value: bool) -> Par { expr(ExprInstance::GBool(value)) }

fn string(value: &str) -> Par { expr(ExprInstance::GString(value.to_string())) }

fn float(value: f64) -> Par { expr(ExprInstance::GDouble(value.to_bits())) }

fn big(value: &BigInt) -> Par { expr(ExprInstance::GBigInt(value.to_signed_bytes_be())) }

fn rational(value: &BigRational) -> Par {
    expr(ExprInstance::GBigRat(GBigRational {
        numerator: value.numer().to_signed_bytes_be(),
        denominator: value.denom().to_signed_bytes_be(),
    }))
}

fn ratio(numerator: i64, denominator: i64) -> Par {
    rational(&BigRational::new(
        BigInt::from(numerator),
        BigInt::from(denominator),
    ))
}

fn list(values: Vec<Par>) -> Par {
    expr(ExprInstance::EListBody(EList {
        ps: values,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

/// The reply `[true, value]`.
fn ok(value: Par) -> Par { list(vec![flag(true), value]) }

/// The reply `[false, code, detail]` of `code`.
fn failed(code: ReplyCode) -> Par {
    list(vec![
        flag(false),
        string(code.code()),
        string(code.detail()),
    ])
}

// ─── Programs ────────────────────────────────────────────────────────────────────────────────

/// The output channel of the programs.
const OUT: &str = "rrs-vec-out";

/// A deploy that makes each call of `calls`, a label and the Rholang arguments of `vec!?`, and
/// sends `[label, reply]` on [`OUT`].
fn program(calls: &[(&str, &str)]) -> String {
    let body = calls
        .iter()
        .map(|(label, arguments)| {
            format!(r#"for (@r <- vec!?({arguments})) {{ @"{OUT}"!(["{label}", r]) }}"#)
        })
        .collect::<Vec<_>>()
        .join(" |\n  ");
    format!("new vec(`rho:lang:vector`) in {{\n  {body}\n}}")
}

/// The pairs `[label, value]` that [`OUT`] holds, by label.
async fn outputs(session: &Session) -> BTreeMap<String, Par> {
    let mut found = BTreeMap::new();
    for datum in session.values(OUT).await {
        let [pair] = datum.as_slice() else {
            panic!("an output datum holds {} values", datum.len());
        };
        let parts = elements(pair);
        let [label, value] = parts.as_slice() else {
            panic!("an output is not a pair: {pair:?}");
        };
        let Some(ExprInstance::GString(label)) = label
            .exprs
            .first()
            .and_then(|expr| expr.expr_instance.as_ref())
        else {
            panic!("an output label is not a string: {label:?}");
        };
        found.insert(label.clone(), value.clone());
    }
    found
}

/// The elements of the list `par`.
fn elements(par: &Par) -> Vec<Par> {
    match par
        .exprs
        .first()
        .and_then(|expr| expr.expr_instance.as_ref())
    {
        Some(ExprInstance::EListBody(EList { ps, .. })) => ps.clone(),
        other => panic!("not a list: {other:?}"),
    }
}

/// Runs the calls `calls` in one deploy and checks each reply against `expected`.
async fn check_calls(label: &str, calls: &[(&str, &str, Par)]) {
    let mut session = Session::new().await;
    let pairs: Vec<(&str, &str)> = calls
        .iter()
        .map(|(name, arguments, _)| (*name, *arguments))
        .collect();
    session.run(&program(&pairs), label).await;
    let found = outputs(&session).await;
    let expected: BTreeMap<String, Par> = calls
        .iter()
        .map(|(name, _, reply)| ((*name).to_string(), reply.clone()))
        .collect();
    assert_eq!(found, expected, "{label}");
}

// ─── Examples ────────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vector_ops_source_programs_compute_each_operation() {
    check_calls("each operation", &[
        (
            "add",
            r#""add", [1, 2], [3, 4]"#,
            ok(list(vec![int(4), int(6)])),
        ),
        (
            "scale",
            r#""scale", [1, -2], 3"#,
            ok(list(vec![int(3), int(-6)])),
        ),
        ("dot", r#""dot", [1, 2, 3], [4, 5, 6]"#, ok(int(32))),
        ("norm", r#""norm", [3, 4]"#, ok(float(5.0))),
        ("distance", r#""distance", [1, 2], [4, 6]"#, ok(float(5.0))),
        (
            "root",
            r#""norm", [1.0, 1.0]"#,
            ok(float(std::f64::consts::SQRT_2)),
        ),
    ])
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vector_ops_keep_the_widest_kind_in_source() {
    let beyond = BigInt::from(i64::MAX) + 1;
    check_calls("the widest kind", &[
        ("integers", r#""dot", [2n, true], [3, 4]"#, ok(int(10))),
        (
            "beyond i64",
            r#""add", [9223372036854775807], [1]"#,
            ok(list(vec![big(&beyond)])),
        ),
        (
            "integral rational",
            r#""add", [1r], [1]"#,
            ok(list(vec![ratio(2, 1)])),
        ),
        (
            "thirds",
            r#""scale", [1, 2], 1r / 3r"#,
            ok(list(vec![ratio(1, 3), ratio(2, 3)])),
        ),
        (
            "fixed point",
            r#""add", [1.5p1], [1]"#,
            ok(list(vec![ratio(5, 2)])),
        ),
        ("float", r#""add", [0.5], [1r]"#, ok(list(vec![float(1.5)]))),
        (
            "f32",
            r#""scale", [1, 2], 0.5f32"#,
            ok(list(vec![float(0.5), float(1.0)])),
        ),
    ])
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vector_ops_are_exact_before_the_rounding() {
    check_calls("exact accumulation", &[
        (
            "cancel",
            r#""dot", [1e16, 1.0, -1e16], [1, 1, 1]"#,
            ok(float(1.0)),
        ),
        (
            "thirds",
            r#""dot", [1r / 3r, 2r / 3r], [3, 3]"#,
            ok(ratio(3, 1)),
        ),
    ])
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vector_ops_failures_reply_their_codes() {
    check_calls("the reply codes", &[
        (
            "unknown",
            r#""cross", [1], [2]"#,
            failed(ReplyCode::UnknownMethod),
        ),
        (
            "not a name",
            r#"42, [1], [2]"#,
            failed(ReplyCode::UnknownMethod),
        ),
        ("empty", "", failed(ReplyCode::UnknownMethod)),
        (
            "arity",
            r#""norm", [1], [2]"#,
            failed(ReplyCode::BadArgument),
        ),
        ("vector", r#""add", 1, [2]"#, failed(ReplyCode::NotAVector)),
        (
            "element",
            r#""add", ["a"], [1]"#,
            failed(ReplyCode::NotANumber),
        ),
        (
            "factor",
            r#""scale", [1], [2]"#,
            failed(ReplyCode::NotANumber),
        ),
        (
            "lengths",
            r#""dot", [1, 2], [3]"#,
            failed(ReplyCode::DimensionMismatch),
        ),
        (
            "range",
            r#""add", [1.7976931348623157e308], [1.7976931348623157e308]"#,
            failed(ReplyCode::ValueOutOfRange),
        ),
    ])
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vector_ops_try_catch_reads_the_reply() {
    let mut session = Session::new().await;
    session
        .run(
            &format!(
                r#"new vec(`rho:lang:vector`) in {{
  try @[v] <- vec!?("dot", [1, 2], [3, 4]) {{ @"{OUT}"!(["value", v]) }}
  catch @[code, detail] {{ @"{OUT}"!(["value", code]) }} |
  try @[v] <- vec!?("dot", [1, 2], [3]) {{ @"{OUT}"!(["code", v]) }}
  catch @[code, detail] {{ @"{OUT}"!(["code", code]) }}
}}"#
            ),
            "try catch",
        )
        .await;
    assert_eq!(
        outputs(&session).await,
        BTreeMap::from([
            ("value".to_string(), int(11)),
            ("code".to_string(), string("DimensionMismatch")),
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vector_ops_params_and_persistent_sends_abort() {
    let mut session = Session::new().await;
    for (label, source, code) in [
        (
            "params",
            r#"new vec(`rho:lang:vector`), r in { vec!(*r, "dot", [1], [2]; k=1) }"#,
            "ParamsUnsupported",
        ),
        (
            "persistent",
            r#"new vec(`rho:lang:vector`), r in { vec!!(*r, "dot", [1], [2]) }"#,
            "PersistentOpCall",
        ),
    ] {
        session
            .fail_with(source, label, &Injections::new(), &[code])
            .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vector_ops_native_replay_equal() {
    let calls = [
        ("add", r#""add", [1, 2.5], [1r / 3r, 4]"#),
        ("scale", r#""scale", [1, 2], 1.5p1"#),
        ("dot", r#""dot", [1e16, 1.0, -1e16], [1, 1, 1]"#),
        ("norm", r#""norm", [1, 1]"#),
        ("distance", r#""distance", [1, 2], [4, 6]"#),
        ("code", r#""dot", [1, 2], [3]"#),
    ];
    let body = calls
        .iter()
        .map(|(label, arguments)| {
            format!(r#"for (@r <- vec!?({arguments})) {{ out!(["{label}", r]) }}"#)
        })
        .collect::<Vec<_>>()
        .join(" |\n  ");
    let term = format!(
        "new vec(`rho:lang:vector`), out in {{\n  {body} |\n  for (@value <= out) {{ @\"{NATIVE_OUT}\"!(value) }}\n}}"
    );
    let played = native_play(&term, AMPLE_LIMIT).await;
    assert!(
        played.result.errors.is_empty(),
        "native play fails: {:?}",
        played.result.errors
    );
    assert_eq!(
        played.outputs.len(),
        calls.len(),
        "one reply for each call: {:?}",
        played.outputs
    );
    let certification = certify(&played, &term, AMPLE_LIMIT).await;
    assert_reproduces("the vector operations", &played, &certification);
}

// ─── Oracles ─────────────────────────────────────────────────────────────────────────────────

/// The kinds of the inputs, ordered by width (decision 66).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Integer,
    Rational,
    Float,
}

/// A generated number: its `Par`, its exact value and its kind.
#[derive(Clone, Debug)]
struct Number {
    par: Par,
    value: BigRational,
    kind: Kind,
}

/// An integer of either integer kind: a `GInt`, a `GBigInt` or a `GBool`.
fn arb_integer() -> BoxedStrategy<Number> {
    prop_oneof![
        4 => (-50i64..50).prop_map(|value| Number {
            par: int(value),
            value: BigRational::from_integer(BigInt::from(value)),
            kind: Kind::Integer,
        }),
        1 => (-50i64..50, 0u32..3).prop_map(|(value, power)| {
            let value = BigInt::from(value) << (64 * power);
            Number {
                par: big(&value),
                value: BigRational::from_integer(value),
                kind: Kind::Integer,
            }
        }),
        1 => any::<bool>().prop_map(|value| Number {
            par: flag(value),
            value: BigRational::from_integer(BigInt::from(u8::from(value))),
            kind: Kind::Integer,
        }),
    ]
    .boxed()
}

/// A rational of either rational kind: a `GBigRat` or a `GFixedPoint`.
fn arb_rational() -> BoxedStrategy<Number> {
    prop_oneof![
        (-30i64..30, 1i64..12).prop_map(|(numerator, denominator)| {
            let value = BigRational::new(BigInt::from(numerator), BigInt::from(denominator));
            Number {
                par: rational(&value),
                value,
                kind: Kind::Rational,
            }
        }),
        (-3000i64..3000, 0u32..4).prop_map(|(unscaled, scale)| Number {
            par: expr(ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: BigInt::from(unscaled).to_signed_bytes_be(),
                scale,
            })),
            value: BigRational::new(
                BigInt::from(unscaled),
                num_traits::pow::pow(BigInt::from(10u8), scale as usize),
            ),
            kind: Kind::Rational,
        }),
    ]
    .boxed()
}

/// A float `k / 2^j` with `|k| < 2^10` and `j ≤ 5`, as a `GDouble`. With integers below 50, a
/// sum of at most five products of such numbers has a numerator below `2^34` over `2^10`, so
/// every value of `add`, `scale` and `dot` is exact in `f64`.
fn arb_dyadic_float() -> BoxedStrategy<Number> {
    (-(1i64 << 10)..(1i64 << 10), 0u32..=5)
        .prop_map(|(numerator, power)| {
            let value = numerator as f64 / f64::from(1u32 << power);
            Number {
                par: float(value),
                value: BigRational::new(BigInt::from(numerator), BigInt::from(1u32 << power)),
                kind: Kind::Float,
            }
        })
        .boxed()
}

/// A number of an exact kind.
fn arb_exact() -> BoxedStrategy<Number> {
    prop_oneof![2 => arb_integer(), 1 => arb_rational()].boxed()
}

/// The exact `value` encoded in `kind`: the oracle of the encoding of decision 66.
fn encoded(value: &BigRational, kind: Kind) -> Par {
    match kind {
        Kind::Integer => {
            assert!(value.is_integer(), "an integer result");
            match value.numer().to_i64() {
                Some(small) => int(small),
                None => big(value.numer()),
            }
        }
        Kind::Rational => rational(value),
        Kind::Float => float(exact_f64(value)),
    }
}

/// The `f64` of a value that is exact in `f64`: the quotient of its numerator and its
/// denominator, both below `2^53`. IEEE division rounds correctly, so an exact quotient stays
/// exact.
fn exact_f64(value: &BigRational) -> f64 {
    let bound = BigInt::from(1u64 << 53);
    assert!(
        value.numer().abs() < bound && value.denom() < &bound,
        "the generators keep each value exact in f64"
    );
    let numerator = value.numer().to_f64().expect("a numerator below 2^53");
    let denominator = value.denom().to_f64().expect("a denominator below 2^53");
    numerator / denominator
}

/// The call `[op, operands…]`.
fn call(name: &str, operands: Vec<Par>) -> Par {
    let mut values = vec![string(name)];
    values.extend(operands);
    list(values)
}

/// The value of the call.
fn value_of(name: &str, operands: Vec<Par>) -> Result<Par, OpFailure> {
    evaluate(&call(name, operands))
}

fn pars(numbers: &[Number]) -> Par {
    list(numbers.iter().map(|number| number.par.clone()).collect())
}

fn widest(numbers: &[Number]) -> Kind {
    numbers
        .iter()
        .map(|number| number.kind)
        .max()
        .unwrap_or(Kind::Integer)
}

/// Two vectors of one length from `element`.
fn arb_pair(element: BoxedStrategy<Number>) -> BoxedStrategy<(Vec<Number>, Vec<Number>)> {
    (0usize..6)
        .prop_flat_map(move |length| (vec(element.clone(), length), vec(element.clone(), length)))
        .boxed()
}

/// Checks `add`, `scale` and `dot` of `left`, `right` and `factor` against the oracle.
fn check_against_the_oracle(
    left: &[Number],
    right: &[Number],
    factor: &Number,
) -> Result<(), TestCaseError> {
    let pair_kind = widest(left).max(widest(right));
    let sums: Vec<Par> = left
        .iter()
        .zip(right)
        .map(|(x, y)| encoded(&(&x.value + &y.value), pair_kind))
        .collect();
    prop_assert_eq!(
        value_of("add", vec![pars(left), pars(right)]),
        Ok(list(sums))
    );
    let scale_kind = widest(left).max(factor.kind);
    let products: Vec<Par> = left
        .iter()
        .map(|x| encoded(&(&factor.value * &x.value), scale_kind))
        .collect();
    prop_assert_eq!(
        value_of("scale", vec![pars(left), factor.par.clone()]),
        Ok(list(products))
    );
    let dot = left
        .iter()
        .zip(right)
        .fold(BigRational::zero(), |sum, (x, y)| sum + &x.value * &y.value);
    prop_assert_eq!(
        value_of("dot", vec![pars(left), pars(right)]),
        Ok(encoded(&dot, pair_kind))
    );
    Ok(())
}

// ─── Properties ──────────────────────────────────────────────────────────────────────────────

/// `add`, `scale` and `dot` on integers, rationals and fixed-point decimals equal the exact
/// `BigRational` values, in the widest kind of the inputs, encoded as decision 66 states.
#[test]
fn prop_vector_ops_agree_with_an_exact_oracle() {
    check(
        pure_config(),
        (arb_pair(arb_exact()), arb_exact()),
        |((left, right), factor)| check_against_the_oracle(&left, &right, &factor),
    );
}

/// With a float among the inputs, the values are floats. On dyadic inputs whose results are
/// exact in `f64`, they equal the exact values.
#[test]
fn prop_vector_ops_float_inputs_give_floats() {
    let element = prop_oneof![
        2 => arb_dyadic_float(),
        1 => (-50i64..50).prop_map(|value| Number {
            par: int(value),
            value: BigRational::from_integer(BigInt::from(value)),
            kind: Kind::Integer,
        }),
    ]
    .boxed();
    check(
        pure_config(),
        (arb_pair(element), arb_dyadic_float()),
        |((left, right), factor)| check_against_the_oracle(&left, &right, &factor),
    );
}

/// The dot laws (S6 of `Similarity.v`): `a · b = b · a`, and one permutation of the coordinates
/// of both vectors keeps the value.
#[test]
fn prop_vector_ops_dot_laws() {
    let element = prop_oneof![arb_exact(), arb_dyadic_float()].boxed();
    check(
        pure_config(),
        (arb_pair(element), any::<u64>()),
        |((left, right), seed)| {
            let forward = value_of("dot", vec![pars(&left), pars(&right)]);
            prop_assert!(forward.is_ok(), "{forward:?}");
            prop_assert_eq!(&value_of("dot", vec![pars(&right), pars(&left)]), &forward);
            let mut order: Vec<usize> = (0..left.len()).collect();
            let mut state = seed;
            for index in (1..order.len()).rev() {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                order.swap(index, (state >> 33) as usize % (index + 1));
            }
            let permuted = |numbers: &[Number]| -> Vec<Number> {
                order.iter().map(|index| numbers[*index].clone()).collect()
            };
            prop_assert_eq!(
                &value_of("dot", vec![pars(&permuted(&left)), pars(&permuted(&right))]),
                &forward
            );
            Ok(())
        },
    );
}

/// The distance laws (S7 of `Similarity.v`): the distance is not negative, it is symmetric, and
/// it is zero from a vector to itself. The norm of `v` is the distance from `v` to zero.
#[test]
fn prop_vector_ops_distance_laws() {
    let element = prop_oneof![arb_exact(), arb_dyadic_float()].boxed();
    check(pure_config(), arb_pair(element), |(left, right)| {
        let distance = value_of("distance", vec![pars(&left), pars(&right)]);
        let Ok(value) = &distance else {
            return Err(TestCaseError::fail(format!("{distance:?}")));
        };
        let bits = match value
            .exprs
            .first()
            .and_then(|expr| expr.expr_instance.as_ref())
        {
            Some(ExprInstance::GDouble(bits)) => *bits,
            other => return Err(TestCaseError::fail(format!("not a float: {other:?}"))),
        };
        prop_assert!(f64::from_bits(bits) >= 0.0);
        prop_assert_eq!(
            &value_of("distance", vec![pars(&right), pars(&left)]),
            &distance
        );
        prop_assert_eq!(
            value_of("distance", vec![pars(&left), pars(&left)]),
            Ok(float(0.0))
        );
        let zeros = list(vec![int(0); left.len()]);
        prop_assert_eq!(
            value_of("norm", vec![pars(&left)]),
            value_of("distance", vec![pars(&left), zeros])
        );
        Ok(())
    });
}

/// The root of `norm` is correctly rounded: with `x = v · v` exact and `r` the reply, no `f64`
/// is nearer to `√x` than `r`. The check compares the squares of the midpoints between `r`
/// and its neighbours with `x`, exactly.
#[test]
fn prop_vector_ops_roots_are_correctly_rounded() {
    let element = prop_oneof![arb_exact(), arb_dyadic_float()].boxed();
    check(pure_config(), vec(element, 0..6), |vector| {
        let square = vector
            .iter()
            .fold(BigRational::zero(), |sum, x| sum + &x.value * &x.value);
        let reply = value_of("norm", vec![pars(&vector)]);
        let root = match reply
            .as_ref()
            .ok()
            .and_then(|par| par.exprs.first())
            .and_then(|expr| expr.expr_instance.as_ref())
        {
            Some(ExprInstance::GDouble(bits)) => f64::from_bits(*bits),
            other => return Err(TestCaseError::fail(format!("not a float: {other:?}"))),
        };
        prop_assert!(root >= 0.0 && root.is_finite());
        let exact =
            |value: f64| BigRational::from_float(value).expect("a finite f64 is a rational");
        let two = BigRational::from_integer(BigInt::from(2));
        let below = match root > 0.0 {
            true => (exact(root.next_down()) + exact(root)) / &two,
            false => BigRational::zero(),
        };
        let above = (exact(root) + exact(root.next_up())) / &two;
        prop_assert!(
            !below.is_positive() || &below * &below <= square,
            "the midpoint below {root} is above the root of {square}"
        );
        prop_assert!(
            square <= &above * &above,
            "the midpoint above {root} is below the root of {square}"
        );
        Ok(())
    });
}
