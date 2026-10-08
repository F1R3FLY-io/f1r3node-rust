//! The operations of `rho:lang:vector` (P6 step 6, design v3 4.7; decisions 54, 57, 62 and 66).
//!
//! A call is `vec!(ret, op, x…)`. The reducer packs the arguments after `ret` into one list, so
//! the process receives `[ret, [op, x…]]` (I-18). [`evaluate`] computes the value of the list
//! `[op, x…]`, and [`reply`] wraps it into the one datum that the process sends on `ret`.
//!
//! | Operation | Operands | Value |
//! |---|---|---|
//! | `add` | two vectors of one length | the vector `a + b` |
//! | `scale` | a vector `v`, then a number `s` | the vector `s · v` |
//! | `dot` | two vectors of one length | the number `a · b` |
//! | `norm` | one vector `v` | the `f64` `RN_53(√(v · v))` |
//! | `distance` | two vectors of one length | the `f64` `RN_53(√‖a − b‖²)` |
//!
//! Every value is exact until it leaves. A number has a kind: an integer (`GInt`, `GBigInt`,
//! `GBool`), a rational (`GBigRat`, `GFixedPoint`) or a float (`GDouble`, `GFloat32`). The kinds
//! are ordered `integer < rational < float`, and the value of `add`, `scale` and `dot` has the
//! widest kind of the inputs (decision 66). Without an element and without a factor, the kind is
//! integer. The value leaves as:
//!
//! * an integer: `GInt` when it fits an `i64`, else `GBigInt` (big-endian two's complement);
//! * a rational: `GBigRat`, reduced, with a positive denominator, also for an integral value;
//! * a float: `GDouble` with the bits of `RN_53` of the exact value (`num::round_to_f64`).
//!
//! The reply is `[true, value]`, or `[false, code, detail]` for the failures of [`ReplyCode`], so
//! `!?` and `try … catch` apply (decision 57). A host-work rejection is not a reply: the deploy
//! fails (decision 40).
//!
//! ```text
//! EVALUATE([op, x₁, …, x_n]):
//!   operation ← the method named op, or reply UnknownMethod
//!   n = arity(operation), or reply BadArgument
//!   decode each operand: a vector (vector_of) or a number (number_of), with its kind
//!   the vectors of a pair have one length, or reply DimensionMismatch
//!   compute the value exactly, then encode it in the widest kind (norm, distance: RN_53 of √)
//! ```
//!
//! Every step reserves its work on the meter of the call before it makes it (decision 42).
//! Without a budget, as in legacy execution, the meter accepts every reservation.

use std::borrow::Cow;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, GBigRational, Par};
use num_traits::ToPrimitive;
use rspace_plus_plus::rspace::errors::RSpaceError;

use super::num::{self, Exact, ExactVector, NumError, Number};
use super::{costs, exact};
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::spaces::record::{expr_par, sole_expr};

/// An operation of `rho:lang:vector`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Add,
    Scale,
    Dot,
    Norm,
    Distance,
}

impl Operation {
    /// Every operation, in the order of the method lookup.
    pub const ALL: [Operation; 5] = [
        Operation::Add,
        Operation::Scale,
        Operation::Dot,
        Operation::Norm,
        Operation::Distance,
    ];

    /// The method name of the operation.
    pub const fn name(self) -> &'static str {
        match self {
            Operation::Add => "add",
            Operation::Scale => "scale",
            Operation::Dot => "dot",
            Operation::Norm => "norm",
            Operation::Distance => "distance",
        }
    }

    /// The number of operands of the operation.
    pub const fn operands(self) -> usize {
        match self {
            Operation::Norm => 1,
            Operation::Add | Operation::Scale | Operation::Dot | Operation::Distance => 2,
        }
    }
}

/// The kind of a number, ordered by width: the kind of a value is the widest kind of its inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Integer,
    Rational,
    Float,
}

/// The kind of the number `par`, or `None` when `par` is not a number of the numeric core.
/// It reads only the kind of the expression.
pub fn kind_of(par: &Par) -> Option<Kind> {
    match (sole_expr(par), par.connective_used) {
        (
            Some(ExprInstance::GInt(_) | ExprInstance::GBigInt(_) | ExprInstance::GBool(_)),
            false,
        ) => Some(Kind::Integer),
        (Some(ExprInstance::GBigRat(_) | ExprInstance::GFixedPoint(_)), false) => {
            Some(Kind::Rational)
        }
        (Some(ExprInstance::GDouble(_) | ExprInstance::GFloat32(_)), false) => Some(Kind::Float),
        _ => None,
    }
}

/// A failure that the process replies as `[false, code, detail]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyCode {
    /// The first element of the call is not the name of an operation.
    UnknownMethod,
    /// The operation has another number of operands.
    BadArgument,
    /// A vector operand is not one list of numbers.
    NotAVector,
    /// An element or a factor is not a number.
    NotANumber,
    /// NaN or an infinity.
    NonFiniteValue,
    /// A big rational whose denominator is not positive.
    MalformedNumber,
    /// The two vectors of a pair have different lengths.
    DimensionMismatch,
    /// A float value beyond the finite range of `f64`, or a fixed-point scale above
    /// `num::MAX_FIXED_POINT_SCALE`.
    ValueOutOfRange,
}

impl ReplyCode {
    /// Every reply code.
    pub const ALL: [ReplyCode; 8] = [
        ReplyCode::UnknownMethod,
        ReplyCode::BadArgument,
        ReplyCode::NotAVector,
        ReplyCode::NotANumber,
        ReplyCode::NonFiniteValue,
        ReplyCode::MalformedNumber,
        ReplyCode::DimensionMismatch,
        ReplyCode::ValueOutOfRange,
    ];

    /// The code string of the reply.
    pub const fn code(self) -> &'static str {
        match self {
            ReplyCode::UnknownMethod => "UnknownMethod",
            ReplyCode::BadArgument => "BadArgument",
            ReplyCode::NotAVector => "NotAVector",
            ReplyCode::NotANumber => "NotANumber",
            ReplyCode::NonFiniteValue => "NonFiniteValue",
            ReplyCode::MalformedNumber => "MalformedNumber",
            ReplyCode::DimensionMismatch => "DimensionMismatch",
            ReplyCode::ValueOutOfRange => "ValueOutOfRange",
        }
    }

    /// The detail of the reply: a fixed text, so a reply never echoes an operand.
    pub const fn detail(self) -> &'static str {
        match self {
            ReplyCode::UnknownMethod => "the method is add, scale, dot, norm or distance",
            ReplyCode::BadArgument => "norm takes one operand, and the other methods take two",
            ReplyCode::NotAVector => "a vector operand is one list of numbers",
            ReplyCode::NotANumber => "every element and every factor is a number",
            ReplyCode::NonFiniteValue => "every number is finite",
            ReplyCode::MalformedNumber => "a big rational has a positive denominator",
            ReplyCode::DimensionMismatch => "the two vectors have different lengths",
            ReplyCode::ValueOutOfRange => "a value is beyond the range of the numeric core",
        }
    }
}

/// A failure of a call.
#[derive(Clone, Debug, PartialEq)]
pub enum OpFailure {
    /// A failure that the process replies.
    Reply(ReplyCode),
    /// A rejection of a reservation by the host-work budget: the deploy fails.
    HostWork(RSpaceError),
    /// A broken invariant of the module: the deploy fails.
    Inconsistent(&'static str),
}

impl From<RSpaceError> for OpFailure {
    fn from(error: RSpaceError) -> Self { OpFailure::HostWork(error) }
}

impl From<NumError> for OpFailure {
    fn from(error: NumError) -> Self {
        match error {
            NumError::NotANumber => OpFailure::Reply(ReplyCode::NotANumber),
            NumError::NonFiniteValue => OpFailure::Reply(ReplyCode::NonFiniteValue),
            NumError::MalformedNumber => OpFailure::Reply(ReplyCode::MalformedNumber),
            NumError::NotAVector => OpFailure::Reply(ReplyCode::NotAVector),
            NumError::DimensionMismatch { .. } => OpFailure::Reply(ReplyCode::DimensionMismatch),
            NumError::ValueOutOfRange => OpFailure::Reply(ReplyCode::ValueOutOfRange),
            NumError::MixedScores => OpFailure::Inconsistent("a vector operation mixes scores"),
            NumError::HostWork(error) => OpFailure::HostWork(error),
        }
    }
}

/// A decoded vector operand and the widest kind of its elements.
struct VectorOperand {
    vector: ExactVector,
    kind: Kind,
}

/// A decoded number operand and its kind.
struct NumberOperand {
    number: Number,
    kind: Kind,
}

impl NumberOperand {
    /// The exact value: borrowed, or converted with the work reserved first.
    fn value(&self, meter: &ScanMeter) -> Result<Cow<'_, Exact>, OpFailure> {
        match &self.number {
            Number::Dyadic(form) => Ok(Cow::Owned(form.value(meter)?)),
            Number::Rational(value) => Ok(Cow::Borrowed(value)),
        }
    }
}

/// The value of the call `[op, x…]`: the list that the reducer packed after `ret`.
pub fn evaluate(call: &Par, meter: &ScanMeter) -> Result<Par, OpFailure> {
    let elements = list_elements(call).ok_or(OpFailure::Reply(ReplyCode::BadArgument))?;
    let (name, operands) = elements
        .split_first()
        .ok_or(OpFailure::Reply(ReplyCode::UnknownMethod))?;
    let operation = operation_of(name, meter)?;
    if operands.len() != operation.operands() {
        return Err(OpFailure::Reply(ReplyCode::BadArgument));
    }
    match (operation, operands) {
        (Operation::Add, [left, right]) => {
            let (left, right) = vector_pair(left, right, meter)?;
            add(&left, &right, meter)
        }
        (Operation::Scale, [vector, factor]) => {
            let vector = vector_operand(vector, meter)?;
            let factor = number_operand(factor, meter)?;
            scale(&vector, &factor, meter)
        }
        (Operation::Dot, [left, right]) => {
            let (left, right) = vector_pair(left, right, meter)?;
            let value = exact::dot(&left.vector, &right.vector, meter)?;
            number_par(&value, left.kind.max(right.kind), meter)
        }
        (Operation::Norm, [vector]) => {
            let vector = vector_operand(vector, meter)?;
            let square = exact::dot(&vector.vector, &vector.vector, meter)?;
            float_par(num::sqrt_to_f64(&square, meter)?, meter)
        }
        (Operation::Distance, [left, right]) => {
            let (left, right) = vector_pair(left, right, meter)?;
            let square = exact::distance2(&left.vector, &right.vector, meter)?;
            float_par(num::sqrt_to_f64(&square, meter)?, meter)
        }
        _ => Err(OpFailure::Inconsistent(
            "the operand count was checked against the operation",
        )),
    }
}

/// The reply of a call: `[true, value]`, or `[false, code, detail]` for a failure that the
/// process replies. A host-work rejection and an inconsistency pass through.
pub fn reply(result: Result<Par, OpFailure>, meter: &ScanMeter) -> Result<Par, OpFailure> {
    let parts = match result {
        Ok(value) => {
            meter.reserve_buffer::<Par>(2)?;
            meter.reserve_buffer::<Expr>(1)?;
            vec![expr_par(ExprInstance::GBool(true)), value]
        }
        Err(OpFailure::Reply(code)) => {
            meter.reserve_buffer::<Par>(3)?;
            meter.reserve_buffer::<Expr>(3)?;
            meter.reserve_buffer::<u8>(code.code().len())?;
            meter.reserve_buffer::<u8>(code.detail().len())?;
            vec![
                expr_par(ExprInstance::GBool(false)),
                expr_par(ExprInstance::GString(code.code().to_string())),
                expr_par(ExprInstance::GString(code.detail().to_string())),
            ]
        }
        Err(failure) => return Err(failure),
    };
    list_par(parts, meter)
}

/// The elements of `par` when it is one list without a remainder or a connective.
fn list_elements(par: &Par) -> Option<&[Par]> {
    match (sole_expr(par), par.connective_used) {
        (
            Some(ExprInstance::EListBody(EList {
                ps,
                remainder: None,
                ..
            })),
            false,
        ) => Some(ps),
        _ => None,
    }
}

/// The operation that `name` names. Each comparison of a method name is reserved first.
fn operation_of(name: &Par, meter: &ScanMeter) -> Result<Operation, OpFailure> {
    let text = match (sole_expr(name), name.connective_used) {
        (Some(ExprInstance::GString(text)), false) => text,
        _ => return Err(OpFailure::Reply(ReplyCode::UnknownMethod)),
    };
    for operation in Operation::ALL {
        meter.compare(text.len(), operation.name().len())?;
        if text == operation.name() {
            return Ok(operation);
        }
    }
    Err(OpFailure::Reply(ReplyCode::UnknownMethod))
}

/// The vector operand `par` and the widest kind of its elements. The kind test of each element
/// reads the kind of its expression, one operation each, as the driver tests a numeric list.
fn vector_operand(par: &Par, meter: &ScanMeter) -> Result<VectorOperand, OpFailure> {
    let vector = num::vector_of(par, None, meter)?;
    let elements = list_elements(par).ok_or(OpFailure::Inconsistent(
        "a decoded vector operand is one list",
    ))?;
    meter.charge(elements.len(), 0, 0)?;
    let kind = elements
        .iter()
        .filter_map(kind_of)
        .max()
        .unwrap_or(Kind::Integer);
    Ok(VectorOperand { vector, kind })
}

/// The number operand `par` and its kind.
fn number_operand(par: &Par, meter: &ScanMeter) -> Result<NumberOperand, OpFailure> {
    let number = num::number_of(par, meter)?;
    let kind = kind_of(par).ok_or(OpFailure::Inconsistent(
        "a decoded number operand has a kind",
    ))?;
    Ok(NumberOperand { number, kind })
}

/// The two vector operands of a pair, which must have one length.
fn vector_pair(
    left: &Par,
    right: &Par,
    meter: &ScanMeter,
) -> Result<(VectorOperand, VectorOperand), OpFailure> {
    let (left, right) = (vector_operand(left, meter)?, vector_operand(right, meter)?);
    match left.vector.len() == right.vector.len() {
        true => Ok((left, right)),
        false => Err(OpFailure::Reply(ReplyCode::DimensionMismatch)),
    }
}

/// `a + b`, exact, in the widest kind of both vectors.
fn add(left: &VectorOperand, right: &VectorOperand, meter: &ScanMeter) -> Result<Par, OpFailure> {
    let kind = left.kind.max(right.kind);
    let (a, b) = (
        left.vector.rationals(meter)?,
        right.vector.rationals(meter)?,
    );
    let mut values = meter.buffer::<Par>(a.len())?;
    for (x, y) in a.iter().zip(b.iter()) {
        costs::reserve_rational(x, y, meter)?;
        values.push(number_par(&(x + y), kind, meter)?);
    }
    list_par(values, meter)
}

/// `s · v`, exact, in the widest kind of the vector and the factor.
fn scale(
    vector: &VectorOperand,
    factor: &NumberOperand,
    meter: &ScanMeter,
) -> Result<Par, OpFailure> {
    let kind = vector.kind.max(factor.kind);
    let s = factor.value(meter)?;
    let elements = vector.vector.rationals(meter)?;
    let mut values = meter.buffer::<Par>(elements.len())?;
    for x in elements.iter() {
        costs::reserve_rational(&s, x, meter)?;
        values.push(number_par(&(s.as_ref() * x), kind, meter)?);
    }
    list_par(values, meter)
}

/// The exact `value` encoded in `kind`. The encode of each integer and the rounding of a float
/// are reserved first.
fn number_par(value: &Exact, kind: Kind, meter: &ScanMeter) -> Result<Par, OpFailure> {
    let instance = match kind {
        Kind::Integer => {
            if !value.is_integer() {
                return Err(OpFailure::Inconsistent(
                    "an operation on integers gives an integer",
                ));
            }
            let integer = value.numer();
            costs::reserve_integer_encode(integer, meter)?;
            match integer.to_i64() {
                Some(small) => ExprInstance::GInt(small),
                None => ExprInstance::GBigInt(integer.to_signed_bytes_be()),
            }
        }
        Kind::Rational => {
            costs::reserve_integer_encode(value.numer(), meter)?;
            costs::reserve_integer_encode(value.denom(), meter)?;
            ExprInstance::GBigRat(GBigRational {
                numerator: value.numer().to_signed_bytes_be(),
                denominator: value.denom().to_signed_bytes_be(),
            })
        }
        Kind::Float => ExprInstance::GDouble(num::round_to_f64(value, meter)?.to_bits()),
    };
    meter.reserve_buffer::<Expr>(1)?;
    Ok(expr_par(instance))
}

/// The `f64` `value` as a `GDouble`.
fn float_par(value: f64, meter: &ScanMeter) -> Result<Par, OpFailure> {
    meter.reserve_buffer::<Expr>(1)?;
    Ok(expr_par(ExprInstance::GDouble(value.to_bits())))
}

/// The list of the ground values `values`. A number and a list of numbers have no free variable
/// and no connective, so the list has none either.
fn list_par(values: Vec<Par>, meter: &ScanMeter) -> Result<Par, OpFailure> {
    meter.reserve_buffer::<Expr>(1)?;
    Ok(expr_par(ExprInstance::EListBody(EList {
        ps: values,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    })))
}

#[cfg(test)]
mod tests {
    use models::rhoapi::GFixedPoint;
    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
    use num_bigint::BigInt;

    use super::*;
    use crate::rust::interpreter::host_work::HostWorkBudget;

    fn unmetered() -> ScanMeter { ScanMeter::new(None) }

    fn int(value: i64) -> Par { expr_par(ExprInstance::GInt(value)) }

    fn float(value: f64) -> Par { expr_par(ExprInstance::GDouble(value.to_bits())) }

    fn float32(value: f32) -> Par { expr_par(ExprInstance::GFloat32(value.to_bits())) }

    fn flag(value: bool) -> Par { expr_par(ExprInstance::GBool(value)) }

    fn string(value: &str) -> Par { expr_par(ExprInstance::GString(value.to_string())) }

    fn big(value: BigInt) -> Par { expr_par(ExprInstance::GBigInt(value.to_signed_bytes_be())) }

    fn rational(numerator: i64, denominator: i64) -> Par {
        expr_par(ExprInstance::GBigRat(GBigRational {
            numerator: BigInt::from(numerator).to_signed_bytes_be(),
            denominator: BigInt::from(denominator).to_signed_bytes_be(),
        }))
    }

    fn fixed(unscaled: i64, scale: u32) -> Par {
        expr_par(ExprInstance::GFixedPoint(GFixedPoint {
            unscaled: BigInt::from(unscaled).to_signed_bytes_be(),
            scale,
        }))
    }

    fn list(values: Vec<Par>) -> Par {
        expr_par(ExprInstance::EListBody(EList {
            ps: values,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        }))
    }

    fn call(name: &str, operands: Vec<Par>) -> Par {
        let mut elements = vec![string(name)];
        elements.extend(operands);
        list(elements)
    }

    fn value_of(name: &str, operands: Vec<Par>) -> Result<Par, OpFailure> {
        evaluate(&call(name, operands), &unmetered())
    }

    fn code_of(name: &str, operands: Vec<Par>) -> Option<ReplyCode> {
        match value_of(name, operands) {
            Err(OpFailure::Reply(code)) => Some(code),
            _ => None,
        }
    }

    #[test]
    fn vector_ops_keep_the_widest_kind() {
        assert_eq!(
            value_of("add", vec![
                list(vec![int(1), int(2)]),
                list(vec![int(3), flag(true)])
            ]),
            Ok(list(vec![int(4), int(3)]))
        );
        assert_eq!(
            value_of("add", vec![list(vec![int(1)]), list(vec![rational(1, 2)])]),
            Ok(list(vec![rational(3, 2)]))
        );
        assert_eq!(
            value_of("add", vec![list(vec![fixed(25, 1)]), list(vec![int(1)])]),
            Ok(list(vec![rational(7, 2)])),
            "a fixed-point input gives a rational"
        );
        assert_eq!(
            value_of("add", vec![list(vec![int(1)]), list(vec![rational(1, 1)])]),
            Ok(list(vec![rational(2, 1)])),
            "an integral rational stays a rational"
        );
        assert_eq!(
            value_of("add", vec![
                list(vec![float(0.5)]),
                list(vec![rational(1, 3)])
            ]),
            Ok(list(vec![float(5.0 / 6.0)])),
            "RN_53(5/6): the IEEE quotient is correctly rounded, and 0.5 + fl(1/3) is not"
        );
        assert_eq!(
            value_of("scale", vec![list(vec![int(2), int(-3)]), float32(0.5)]),
            Ok(list(vec![float(1.0), float(-1.5)])),
            "an f32 factor gives an f64"
        );
        assert_eq!(
            value_of("dot", vec![list(vec![]), list(vec![])]),
            Ok(int(0)),
            "without an element the kind is integer"
        );
    }

    #[test]
    fn vector_ops_integers_widen_to_big_integers() {
        let max = BigInt::from(i64::MAX);
        assert_eq!(
            value_of("add", vec![list(vec![int(i64::MAX)]), list(vec![int(1)])]),
            Ok(list(vec![big(max + 1)]))
        );
        assert_eq!(
            value_of("scale", vec![
                list(vec![big(BigInt::from(1) << 70u8)]),
                int(0)
            ]),
            Ok(list(vec![int(0)])),
            "a value that fits an i64 is a GInt"
        );
    }

    #[test]
    fn vector_ops_are_exact_before_the_rounding() {
        // In f64, (1e16 + 1) - 1e16 accumulates to 0. The exact sum is 1.
        assert_eq!(
            value_of("dot", vec![
                list(vec![float(1e16), float(1.0), float(-1e16)]),
                list(vec![int(1), int(1), int(1)])
            ]),
            Ok(float(1.0))
        );
        assert_eq!(
            value_of("dot", vec![list(vec![rational(1, 3)]), list(vec![int(3)])]),
            Ok(rational(1, 1))
        );
    }

    #[test]
    fn vector_ops_norm_and_distance_are_correctly_rounded() {
        assert_eq!(
            value_of("norm", vec![list(vec![int(3), int(4)])]),
            Ok(float(5.0))
        );
        assert_eq!(
            value_of("norm", vec![list(vec![int(1), int(1)])]),
            Ok(float(std::f64::consts::SQRT_2))
        );
        assert_eq!(value_of("norm", vec![list(vec![])]), Ok(float(0.0)));
        assert_eq!(
            value_of("distance", vec![
                list(vec![int(1), int(2)]),
                list(vec![int(4), int(6)])
            ]),
            Ok(float(5.0))
        );
        assert_eq!(
            value_of("distance", vec![
                list(vec![rational(1, 3)]),
                list(vec![rational(1, 3)])
            ]),
            Ok(float(0.0))
        );
    }

    #[test]
    fn vector_ops_failures_have_their_codes() {
        let pair = || vec![list(vec![int(1)]), list(vec![int(2)])];
        assert_eq!(code_of("cross", pair()), Some(ReplyCode::UnknownMethod));
        assert_eq!(
            evaluate(&list(vec![int(1)]), &unmetered()),
            Err(OpFailure::Reply(ReplyCode::UnknownMethod)),
            "the method is a string"
        );
        assert_eq!(
            evaluate(&list(vec![]), &unmetered()),
            Err(OpFailure::Reply(ReplyCode::UnknownMethod))
        );
        assert_eq!(
            evaluate(&int(1), &unmetered()),
            Err(OpFailure::Reply(ReplyCode::BadArgument))
        );
        assert_eq!(
            code_of("norm", pair()),
            Some(ReplyCode::BadArgument),
            "norm takes one operand"
        );
        assert_eq!(
            code_of("add", vec![list(vec![int(1)])]),
            Some(ReplyCode::BadArgument)
        );
        assert_eq!(
            code_of("add", vec![int(1), list(vec![int(2)])]),
            Some(ReplyCode::NotAVector)
        );
        assert_eq!(
            code_of("add", vec![list(vec![string("1")]), list(vec![int(2)])]),
            Some(ReplyCode::NotANumber)
        );
        assert_eq!(
            code_of("scale", vec![list(vec![int(1)]), list(vec![int(2)])]),
            Some(ReplyCode::NotANumber),
            "the factor of scale is a number"
        );
        assert_eq!(
            code_of("norm", vec![list(vec![float(f64::NAN)])]),
            Some(ReplyCode::NonFiniteValue)
        );
        assert_eq!(
            code_of("norm", vec![list(vec![rational(1, 0)])]),
            Some(ReplyCode::MalformedNumber)
        );
        assert_eq!(
            code_of("dot", vec![list(vec![int(1), int(2)]), list(vec![int(3)])]),
            Some(ReplyCode::DimensionMismatch)
        );
        assert_eq!(
            code_of("add", vec![
                list(vec![float(f64::MAX)]),
                list(vec![float(f64::MAX)])
            ]),
            Some(ReplyCode::ValueOutOfRange)
        );
        assert_eq!(
            code_of("norm", vec![list(vec![fixed(
                1,
                num::MAX_FIXED_POINT_SCALE + 1
            )])]),
            Some(ReplyCode::ValueOutOfRange)
        );
    }

    #[test]
    fn vector_ops_replies_have_the_ok_shape() {
        let meter = unmetered();
        assert_eq!(
            reply(Ok(int(7)), &meter),
            Ok(list(vec![flag(true), int(7)]))
        );
        for code in ReplyCode::ALL {
            assert_eq!(
                reply(Err(OpFailure::Reply(code)), &meter),
                Ok(list(vec![
                    flag(false),
                    string(code.code()),
                    string(code.detail())
                ])),
                "{code:?}"
            );
        }
        assert_eq!(
            reply(
                Err(OpFailure::HostWork(RSpaceError::HostWorkRejected)),
                &meter
            ),
            Err(OpFailure::HostWork(RSpaceError::HostWorkRejected)),
            "a host-work rejection is not a reply"
        );
    }

    #[test]
    fn vector_ops_reserve_their_work_and_reject_on_the_budget() {
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        let meter = ScanMeter::new(Some(budget.clone()));
        let program = call("dot", vec![
            list(vec![rational(1, 3), int(2)]),
            list(vec![int(3), int(4)]),
        ]);
        assert_eq!(evaluate(&program, &meter), Ok(rational(9, 1)));
        let used = budget
            .usage(HostWorkDimension::VerificationOperations)
            .get();
        assert!(used > 0, "the call reserves its work");
        let tight = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(used - 1)));
        assert_eq!(
            evaluate(&program, &ScanMeter::new(Some(tight))),
            Err(OpFailure::HostWork(RSpaceError::HostWorkRejected)),
            "one unit less of host work rejects the call"
        );
    }
}
