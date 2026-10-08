//! The exact numbers of the VectorDB family (P6, design v3 4.1; decision 54).
//!
//! Every consensus decision of a VectorDB space is exact. A number enters with the exact value
//! of its literal. An integer, a float and a Boolean enter as a dyadic number `m·2^e`, and so
//! does a big rational or a fixed-point decimal whose reduced denominator is a power of two. Any
//! other number enters as a reduced rational. NaN and the infinities fail with
//! `NonFiniteValue`, and `-0` becomes `0`. A Boolean is `0` or `1`.
//!
//! A value leaves as an `f64` only where a program asks for one (`scores=true`, `norm`,
//! `distance`). It is then `RN_53` of the exact value: the nearest `f64`, with ties to even. The
//! square root of a rational is rounded the same way, from an exact integer square root, so it
//! needs no loop: the rounding of an algebraic value is decided by integer comparisons.
//!
//! ```text
//! ROUND(x):                              ▷ x a rational, RN_53 of x
//!   e  ← ⌊log₂|x|⌋                        ▷ 2^e ≤ |x| < 2^(e+1)
//!   qe ← max(e − 52, −1074)               ▷ the spacing of the f64 values near |x|
//!   m  ← |x| / 2^qe rounded to an integer, ties to even
//!   return sign(x) · m · 2^qe             ▷ m = 2^53 carries into the next binade
//!
//! SQRT-ROUND(x):                         ▷ x ≥ 0 a rational, RN_53 of √x
//!   e  ← ⌊⌊log₂ x⌋ / 2⌋                   ▷ 2^e ≤ √x < 2^(e+1)
//!   qe ← max(e − 52, −1074)
//!   n/d ← x / 4^qe                        ▷ √(n/d) = √x / 2^qe
//!   s  ← ⌊√⌊n/d⌋⌋                          ▷ s ≤ √(n/d) < s + 1
//!   m  ← s + 1 when 4n > d(2s+1)², s when 4n < d(2s+1)², the even one when equal
//!   return m · 2^qe
//! ```
//!
//! The last line of SQRT-ROUND compares `√(n/d)` with the midpoint `s + 1/2` by squaring both
//! sides. Both sides are positive, so the comparison is exact.
//!
//! Every function that does work proportional to its inputs reserves that work on the meter of
//! the job before it makes it (decision 42). Without a budget the meter accepts every
//! reservation.

use std::borrow::Cow;
use std::cmp::Ordering;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Par};
use num_bigint::{BigInt, BigUint};
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};
use rspace_plus_plus::rspace::errors::RSpaceError;

use super::costs;
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::spaces::record::sole_expr;

/// The greatest scale of a fixed-point decimal that the numeric core accepts. A scale is a
/// `u32`, so a short literal could otherwise ask for a power of ten with billions of digits.
/// The bound limits the size of a number, not a scan (decision 40).
pub const MAX_FIXED_POINT_SCALE: u32 = 4096;

/// A failure of the numeric core.
#[derive(Clone, Debug, PartialEq)]
pub enum NumError {
    /// A value that is not a number of the numeric core.
    NotANumber,
    /// NaN or an infinity.
    NonFiniteValue,
    /// A big rational whose denominator is not positive.
    MalformedNumber,
    /// A value that is not a list of numbers.
    NotAVector,
    /// A vector whose length is not the dimension of its space.
    DimensionMismatch { expected: usize, found: usize },
    /// A value outside the range that the core represents: a delivered value beyond the finite
    /// range of `f64`, or a fixed-point scale above [`MAX_FIXED_POINT_SCALE`].
    ValueOutOfRange,
    /// Two scores of different metrics in one comparison. The scores of one query share its
    /// metric, so a caller that meets this error has mixed two queries.
    MixedScores,
    /// A rejection of a reservation by the host-work budget.
    HostWork(RSpaceError),
}

impl From<RSpaceError> for NumError {
    fn from(error: RSpaceError) -> Self { NumError::HostWork(error) }
}

/// An exact number of the core: a reduced rational with a positive denominator.
pub type Exact = BigRational;

/// The dyadic number `mantissa · 2^exponent`.
///
/// Its form is not unique (`1` is `1·2^0` and also `2^52·2^-52`), so it has no equality of its
/// own. Compare values through [`Dyadic::value`].
#[derive(Clone, Debug)]
pub struct Dyadic {
    /// The integer mantissa.
    pub mantissa: BigInt,
    /// The power of two.
    pub exponent: i64,
}

impl Dyadic {
    /// The exact value as a reduced rational. The work is reserved first.
    pub fn value(&self, meter: &ScanMeter) -> Result<Exact, NumError> {
        costs::reserve_dyadic_value(&self.mantissa, self.exponent, meter)?;
        Ok(scaled(self.mantissa.clone(), self.exponent))
    }
}

/// A number of the core.
#[derive(Clone, Debug)]
pub enum Number {
    /// A dyadic number: every integer, float and Boolean, and every rational whose reduced
    /// denominator is a power of two.
    Dyadic(Dyadic),
    /// Any other rational, reduced.
    Rational(Exact),
}

/// A vector of exact numbers.
#[derive(Clone, Debug)]
pub enum ExactVector {
    /// Every element is dyadic, so a kernel sums integers with one common exponent.
    Dyadic(Vec<Dyadic>),
    /// At least one element is not dyadic, so a kernel sums reduced rationals.
    Rational(Vec<Exact>),
}

impl ExactVector {
    /// The number of elements.
    pub fn len(&self) -> usize {
        match self {
            ExactVector::Dyadic(forms) => forms.len(),
            ExactVector::Rational(values) => values.len(),
        }
    }

    /// True for a vector without elements.
    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// The elements as reduced rationals: borrowed, or converted with the work reserved first.
    pub fn rationals(&self, meter: &ScanMeter) -> Result<Cow<'_, [Exact]>, NumError> {
        match self {
            ExactVector::Rational(values) => Ok(Cow::Borrowed(values)),
            ExactVector::Dyadic(forms) => {
                meter.reserve_buffer::<Exact>(forms.len())?;
                let mut values = Vec::with_capacity(forms.len());
                for form in forms {
                    values.push(form.value(meter)?);
                }
                Ok(Cow::Owned(values))
            }
        }
    }
}

/// `m · 2^k` as a reduced rational. For `k < 0` the reduction strips the common factors of two,
/// so no gcd is needed: the numerator is then odd, or the denominator is one.
pub fn scaled(mantissa: BigInt, power: i64) -> Exact {
    let shift = power.unsigned_abs();
    match (power >= 0, mantissa.trailing_zeros()) {
        (true, _) => Exact::from_integer(mantissa << shift),
        (false, None) => Exact::zero(),
        (false, Some(zeros)) => {
            let common = zeros.min(shift);
            Exact::new_raw(mantissa >> common, BigInt::one() << (shift - common))
        }
    }
}

/// The value of a two's complement big-endian integer, as the interpreter encodes `GBigInt`.
/// No bytes is zero.
fn integer_of(bytes: &[u8]) -> BigInt { BigInt::from_signed_bytes_be(bytes) }

/// A dyadic number of a sign and an unsigned mantissa.
fn signed_dyadic(negative: bool, magnitude: u64, exponent: i64) -> Dyadic {
    let magnitude = BigInt::from(magnitude);
    Dyadic {
        mantissa: match negative {
            true => -magnitude,
            false => magnitude,
        },
        exponent,
    }
}

/// The dyadic form of the `f64` with the bits `bits`, or `NonFiniteValue`. `-0` is `0`.
fn dyadic_of_f64(bits: u64) -> Result<Dyadic, NumError> {
    if !f64::from_bits(bits).is_finite() {
        return Err(NumError::NonFiniteValue);
    }
    let field = (bits >> 52) & 0x7ff;
    let fraction = bits & ((1u64 << 52) - 1);
    Ok(match field {
        0 => signed_dyadic(bits >> 63 == 1, fraction, -1074),
        _ => signed_dyadic(
            bits >> 63 == 1,
            fraction | (1u64 << 52),
            i64::try_from(field).map_err(|_| NumError::NonFiniteValue)? - 1075,
        ),
    })
}

/// The dyadic form of the `f32` with the bits `bits`, or `NonFiniteValue`. `-0` is `0`.
fn dyadic_of_f32(bits: u32) -> Result<Dyadic, NumError> {
    if !f32::from_bits(bits).is_finite() {
        return Err(NumError::NonFiniteValue);
    }
    let field = (bits >> 23) & 0xff;
    let fraction = bits & ((1u32 << 23) - 1);
    Ok(match field {
        0 => signed_dyadic(bits >> 31 == 1, u64::from(fraction), -149),
        _ => signed_dyadic(
            bits >> 31 == 1,
            u64::from(fraction | (1u32 << 23)),
            i64::from(field) - 150,
        ),
    })
}

/// True when `par` is a number of the core: exactly one expression of a numeric kind and no
/// connective. It reads no more than the kind of the expression.
pub fn is_number(par: &Par) -> bool {
    matches!(
        (sole_expr(par), par.connective_used),
        (
            Some(
                ExprInstance::GInt(_)
                    | ExprInstance::GBool(_)
                    | ExprInstance::GDouble(_)
                    | ExprInstance::GFloat32(_)
                    | ExprInstance::GBigInt(_)
                    | ExprInstance::GBigRat(_)
                    | ExprInstance::GFixedPoint(_)
            ),
            false
        )
    )
}

/// The number of a `Par`.
///
/// A number is a `Par` with exactly one expression of a numeric kind and no connective. The
/// work of a big integer, a big rational and a fixed-point decimal grows with its size, and it
/// is reserved first.
pub fn number_of(par: &Par, meter: &ScanMeter) -> Result<Number, NumError> {
    if par.connective_used {
        return Err(NumError::NotANumber);
    }
    match sole_expr(par) {
        Some(ExprInstance::GInt(value)) => Ok(Number::Dyadic(Dyadic {
            mantissa: BigInt::from(*value),
            exponent: 0,
        })),
        Some(ExprInstance::GBool(value)) => Ok(Number::Dyadic(Dyadic {
            mantissa: BigInt::from(u8::from(*value)),
            exponent: 0,
        })),
        Some(ExprInstance::GDouble(bits)) => {
            costs::reserve_float_ingress(meter)?;
            Ok(Number::Dyadic(dyadic_of_f64(*bits)?))
        }
        Some(ExprInstance::GFloat32(bits)) => {
            costs::reserve_float_ingress(meter)?;
            Ok(Number::Dyadic(dyadic_of_f32(*bits)?))
        }
        Some(ExprInstance::GBigInt(bytes)) => {
            costs::reserve_bytes_decode(bytes.len(), meter)?;
            Ok(Number::Dyadic(Dyadic {
                mantissa: integer_of(bytes),
                exponent: 0,
            }))
        }
        Some(ExprInstance::GBigRat(rational)) => {
            costs::reserve_bytes_decode(rational.numerator.len(), meter)?;
            costs::reserve_bytes_decode(rational.denominator.len(), meter)?;
            let denominator = integer_of(&rational.denominator);
            if !denominator.is_positive() {
                return Err(NumError::MalformedNumber);
            }
            rational_number(integer_of(&rational.numerator), denominator, meter)
        }
        Some(ExprInstance::GFixedPoint(fixed)) => {
            if fixed.scale > MAX_FIXED_POINT_SCALE {
                return Err(NumError::ValueOutOfRange);
            }
            costs::reserve_bytes_decode(fixed.unscaled.len(), meter)?;
            costs::reserve_power_of_ten(fixed.scale, meter)?;
            let scale = usize::try_from(fixed.scale).map_err(|_| NumError::ValueOutOfRange)?;
            let denominator = num_traits::pow::pow(BigInt::from(10u8), scale);
            rational_number(integer_of(&fixed.unscaled), denominator, meter)
        }
        _ => Err(NumError::NotANumber),
    }
}

/// The number `n / d` for `d > 0`: dyadic when its reduced denominator is a power of two.
fn rational_number(
    numerator: BigInt,
    denominator: BigInt,
    meter: &ScanMeter,
) -> Result<Number, NumError> {
    costs::reserve_gcd(&numerator, &denominator, meter)?;
    let value = Exact::new(numerator, denominator);
    match power_of_two(value.denom()) {
        Some(exponent) => {
            let (mantissa, _) = value.into_raw();
            Ok(Number::Dyadic(Dyadic {
                mantissa,
                exponent: -exponent,
            }))
        }
        None => Ok(Number::Rational(value)),
    }
}

/// `k` when the positive integer `d` is `2^k`.
fn power_of_two(denominator: &BigInt) -> Option<i64> {
    let zeros = denominator.trailing_zeros()?;
    match denominator.bits() == zeros + 1 {
        true => i64::try_from(zeros).ok(),
        false => None,
    }
}

/// The vector of a `Par` that is one list of numbers. With `dimensions`, its length must be
/// that dimension. The buffers are reserved before they are made.
pub fn vector_of(
    par: &Par,
    dimensions: Option<usize>,
    meter: &ScanMeter,
) -> Result<ExactVector, NumError> {
    let elements = match (sole_expr(par), par.connective_used) {
        (
            Some(ExprInstance::EListBody(EList {
                ps,
                remainder: None,
                ..
            })),
            false,
        ) => ps,
        _ => return Err(NumError::NotAVector),
    };
    if let Some(expected) = dimensions {
        if elements.len() != expected {
            return Err(NumError::DimensionMismatch {
                expected,
                found: elements.len(),
            });
        }
    }
    meter.reserve_buffer::<Dyadic>(elements.len())?;
    let mut forms = Vec::with_capacity(elements.len());
    for (index, element) in elements.iter().enumerate() {
        match number_of(element, meter)? {
            Number::Dyadic(form) => forms.push(form),
            Number::Rational(value) => {
                return rational_vector(forms, value, &elements[index + 1..], meter);
            }
        }
    }
    Ok(ExactVector::Dyadic(forms))
}

/// The rational vector of the dyadic elements `forms`, then `value`, then the numbers of `rest`.
fn rational_vector(
    forms: Vec<Dyadic>,
    value: Exact,
    rest: &[Par],
    meter: &ScanMeter,
) -> Result<ExactVector, NumError> {
    let length = forms.len() + 1 + rest.len();
    meter.reserve_buffer::<Exact>(length)?;
    let mut values = Vec::with_capacity(length);
    for form in &forms {
        values.push(form.value(meter)?);
    }
    values.push(value);
    for element in rest {
        values.push(match number_of(element, meter)? {
            Number::Dyadic(form) => form.value(meter)?,
            Number::Rational(value) => value,
        });
    }
    Ok(ExactVector::Rational(values))
}

/// `⌊log₂(n/d)⌋` for positive integers `n` and `d`: the `e` with `2^e ≤ n/d < 2^(e+1)`. A
/// bit length beyond `i64` is `ValueOutOfRange`.
fn floor_log2(numerator: &BigUint, denominator: &BigUint) -> Result<i64, NumError> {
    let estimate = bits_i64(numerator)?
        .checked_sub(bits_i64(denominator)?)
        .ok_or(NumError::ValueOutOfRange)?;
    let shift = estimate.unsigned_abs();
    let at_least = match estimate >= 0 {
        true => *numerator >= (denominator << shift),
        false => (numerator << shift) >= *denominator,
    };
    match at_least {
        true => Ok(estimate),
        false => estimate.checked_sub(1).ok_or(NumError::ValueOutOfRange),
    }
}

fn bits_i64(value: &BigUint) -> Result<i64, NumError> {
    i64::try_from(value.bits()).map_err(|_| NumError::ValueOutOfRange)
}

/// `n / d` rounded to an integer with ties to even, for `d > 0`.
fn round_half_even(numerator: &BigUint, denominator: &BigUint) -> BigUint {
    let (quotient, remainder) = numerator.div_rem(denominator);
    match (remainder << 1u8).cmp(denominator) {
        Ordering::Less => quotient,
        Ordering::Greater => quotient + 1u8,
        Ordering::Equal => match quotient.is_even() {
            true => quotient,
            false => quotient + 1u8,
        },
    }
}

/// `n · 2^k` and `d`, with only the operand that the power scales copied.
fn scale_by_power_of_two<'a>(
    numerator: &'a BigUint,
    denominator: &'a BigUint,
    power: i64,
) -> (Cow<'a, BigUint>, Cow<'a, BigUint>) {
    let shift = power.unsigned_abs();
    match power >= 0 {
        true => (Cow::Owned(numerator << shift), Cow::Borrowed(denominator)),
        false => (Cow::Borrowed(numerator), Cow::Owned(denominator << shift)),
    }
}

/// The `f64` `±m · 2^qe` for `0 ≤ m ≤ 2^53`, or `ValueOutOfRange` beyond the finite range. A
/// value that rounds to zero is `+0`, as the ingress maps `-0` to `0`.
fn compose(negative: bool, mantissa: BigUint, quantum: i64) -> Result<f64, NumError> {
    let mut mantissa = mantissa.to_u64().ok_or(NumError::ValueOutOfRange)?;
    let mut exponent = quantum;
    if mantissa == 0 {
        return Ok(0.0);
    }
    if mantissa == 1u64 << 53 {
        mantissa = 1u64 << 52;
        exponent += 1;
    }
    let bits = match mantissa >= 1u64 << 52 {
        true => {
            let biased = exponent + 52 + 1023;
            if biased >= 2047 {
                return Err(NumError::ValueOutOfRange);
            }
            let biased = u64::try_from(biased).map_err(|_| NumError::ValueOutOfRange)?;
            (biased << 52) | (mantissa - (1u64 << 52))
        }
        // A subnormal: the quantum is `2^-1074` and the mantissa is below `2^52`.
        false => mantissa,
    };
    let sign = match negative {
        true => 1u64 << 63,
        false => 0,
    };
    Ok(f64::from_bits(sign | bits))
}

/// `RN_53(x)`: the `f64` nearest to the rational `x`, with ties to even, or `ValueOutOfRange`
/// beyond the finite range of `f64`. The work is reserved first.
pub fn round_to_f64(x: &Exact, meter: &ScanMeter) -> Result<f64, NumError> {
    if x.is_zero() {
        return Ok(0.0);
    }
    let (numerator, denominator) = (x.numer().magnitude(), x.denom().magnitude());
    costs::reserve_log2(x, meter)?;
    let exponent = floor_log2(numerator, denominator)?;
    if exponent > 1023 {
        return Err(NumError::ValueOutOfRange);
    }
    let quantum = (exponent - 52).max(-1074);
    costs::reserve_rounding(x, quantum, meter)?;
    let (numerator, denominator) = scale_by_power_of_two(numerator, denominator, -quantum);
    let mantissa = round_half_even(&numerator, &denominator);
    compose(x.is_negative(), mantissa, quantum)
}

/// `RN_53(√x)` for a rational `x ≥ 0`: the `f64` nearest to the square root, with ties to even.
/// A negative `x` is `NotANumber`. The work is reserved first.
pub fn sqrt_to_f64(x: &Exact, meter: &ScanMeter) -> Result<f64, NumError> {
    if x.is_negative() {
        return Err(NumError::NotANumber);
    }
    if x.is_zero() {
        return Ok(0.0);
    }
    let (numerator, denominator) = (x.numer().magnitude(), x.denom().magnitude());
    costs::reserve_log2(x, meter)?;
    let exponent = floor_log2(numerator, denominator)?.div_euclid(2);
    if exponent > 1023 {
        return Err(NumError::ValueOutOfRange);
    }
    let quantum = (exponent - 52).max(-1074);
    costs::reserve_square_root(x, quantum, meter)?;
    let (numerator, denominator) = scale_by_power_of_two(numerator, denominator, -2 * quantum);
    let root = (numerator.as_ref() / denominator.as_ref()).sqrt();
    let odd = (&root << 1u8) + 1u8;
    let mantissa = match (numerator.as_ref() << 2u8).cmp(&(denominator.as_ref() * &odd * &odd)) {
        Ordering::Less => root,
        Ordering::Greater => root + 1u8,
        Ordering::Equal => match root.is_even() {
            true => root,
            false => root + 1u8,
        },
    };
    compose(false, mantissa, quantum)
}

#[cfg(test)]
mod tests {
    use models::rhoapi::{Expr, GBigRational, GFixedPoint};
    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
    use proptest::prelude::*;

    use super::*;
    use crate::rust::interpreter::host_work::HostWorkBudget;

    fn expr(instance: ExprInstance) -> Par {
        Par {
            exprs: vec![Expr {
                expr_instance: Some(instance),
            }],
            ..Par::default()
        }
    }

    fn int(value: i64) -> Par { expr(ExprInstance::GInt(value)) }

    fn double(value: f64) -> Par { expr(ExprInstance::GDouble(value.to_bits())) }

    fn float32(value: f32) -> Par { expr(ExprInstance::GFloat32(value.to_bits())) }

    fn big_int(value: &BigInt) -> Par { expr(ExprInstance::GBigInt(value.to_signed_bytes_be())) }

    fn big_rat(numerator: i64, denominator: i64) -> Par {
        expr(ExprInstance::GBigRat(GBigRational {
            numerator: BigInt::from(numerator).to_signed_bytes_be(),
            denominator: BigInt::from(denominator).to_signed_bytes_be(),
        }))
    }

    fn fixed(unscaled: i64, scale: u32) -> Par {
        expr(ExprInstance::GFixedPoint(GFixedPoint {
            unscaled: BigInt::from(unscaled).to_signed_bytes_be(),
            scale,
        }))
    }

    fn list(ps: Vec<Par>) -> Par {
        expr(ExprInstance::EListBody(EList {
            ps,
            ..EList::default()
        }))
    }

    fn unmetered() -> ScanMeter { ScanMeter::new(None) }

    fn ratio(numerator: i64, denominator: i64) -> Exact {
        Exact::new(BigInt::from(numerator), BigInt::from(denominator))
    }

    fn power(exponent: i64) -> Exact { scaled(BigInt::one(), exponent) }

    fn value_of(par: &Par) -> Result<Exact, NumError> {
        match number_of(par, &unmetered())? {
            Number::Dyadic(form) => form.value(&unmetered()),
            Number::Rational(value) => Ok(value),
        }
    }

    fn exact_f64(value: f64) -> Exact {
        dyadic_of_f64(value.to_bits())
            .expect("a finite value")
            .value(&unmetered())
            .expect("an unmetered value")
    }

    fn is_dyadic(par: &Par) -> bool {
        matches!(number_of(par, &unmetered()), Ok(Number::Dyadic(_)))
    }

    #[test]
    fn tensor_ingress_keeps_the_exact_value_of_each_number() {
        assert_eq!(value_of(&int(-7)), Ok(ratio(-7, 1)));
        assert_eq!(value_of(&expr(ExprInstance::GBool(true))), Ok(ratio(1, 1)));
        assert_eq!(value_of(&expr(ExprInstance::GBool(false))), Ok(ratio(0, 1)));
        // 0.1 is the dyadic 3602879701896397 / 2^55, and 0.1f32 is 13421773 / 2^27.
        assert_eq!(
            value_of(&double(0.1)),
            Ok(Exact::new(
                BigInt::from(3_602_879_701_896_397u64),
                BigInt::one() << 55u8
            ))
        );
        assert_eq!(
            value_of(&float32(0.1)),
            Ok(Exact::new(
                BigInt::from(13_421_773u32),
                BigInt::one() << 27u8
            ))
        );
        let large = BigInt::one() << 100u8;
        assert_eq!(value_of(&big_int(&large)), Ok(Exact::from_integer(large)));
        assert_eq!(value_of(&big_int(&BigInt::from(-5))), Ok(ratio(-5, 1)));
        assert_eq!(
            value_of(&expr(ExprInstance::GBigInt(Vec::new()))),
            Ok(ratio(0, 1))
        );
        assert_eq!(value_of(&big_rat(1, 3)), Ok(ratio(1, 3)));
        assert_eq!(value_of(&big_rat(6, 4)), Ok(ratio(3, 2)));
        assert_eq!(value_of(&fixed(25, 1)), Ok(ratio(5, 2)));
        assert_eq!(value_of(&fixed(1, 1)), Ok(ratio(1, 10)));
        assert_eq!(value_of(&fixed(-125, 3)), Ok(ratio(-1, 8)));
        // The smallest subnormals of both widths.
        assert_eq!(value_of(&double(f64::from_bits(1))), Ok(power(-1074)));
        assert_eq!(value_of(&float32(f32::from_bits(1))), Ok(power(-149)));
        assert_eq!(value_of(&double(f64::MAX)), Ok(exact_f64(f64::MAX)));
    }

    #[test]
    fn tensor_ingress_keeps_dyadic_numbers_dyadic() {
        for par in [
            int(3),
            double(0.75),
            float32(-2.5),
            big_rat(6, 4),
            fixed(25, 1),
            expr(ExprInstance::GBool(true)),
        ] {
            assert!(is_dyadic(&par), "{par:?} is dyadic");
        }
        for par in [big_rat(1, 3), fixed(1, 1), big_rat(-2, 6)] {
            assert!(!is_dyadic(&par), "{par:?} is not dyadic");
        }
    }

    #[test]
    fn tensor_ingress_rejects_non_numbers_and_non_finite_values() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(value_of(&double(value)), Err(NumError::NonFiniteValue));
        }
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(value_of(&float32(value)), Err(NumError::NonFiniteValue));
        }
        assert_eq!(
            value_of(&expr(ExprInstance::GString("1".to_string()))),
            Err(NumError::NotANumber)
        );
        assert_eq!(value_of(&Par::default()), Err(NumError::NotANumber));
        let mut connective = int(1);
        connective.connective_used = true;
        assert_eq!(value_of(&connective), Err(NumError::NotANumber));
        assert!(!is_number(&connective));
        let mut two = int(1);
        two.exprs.push(Expr {
            expr_instance: Some(ExprInstance::GInt(2)),
        });
        assert_eq!(value_of(&two), Err(NumError::NotANumber));
        assert_eq!(value_of(&big_rat(1, 0)), Err(NumError::MalformedNumber));
        assert_eq!(value_of(&big_rat(1, -2)), Err(NumError::MalformedNumber));
        assert_eq!(
            value_of(&fixed(1, MAX_FIXED_POINT_SCALE + 1)),
            Err(NumError::ValueOutOfRange)
        );
        assert!(value_of(&fixed(1, MAX_FIXED_POINT_SCALE)).is_ok());
    }

    #[test]
    fn tensor_negative_zero_enters_as_zero() {
        assert_eq!(value_of(&double(-0.0)), Ok(Exact::zero()));
        assert_eq!(value_of(&float32(-0.0)), Ok(Exact::zero()));
        assert_eq!(
            round_to_f64(&Exact::zero(), &unmetered()).map(f64::to_bits),
            Ok(0)
        );
    }

    #[test]
    fn tensor_is_number_names_the_numeric_kinds() {
        for par in [
            int(1),
            double(1.0),
            float32(1.0),
            big_int(&BigInt::from(1)),
            big_rat(1, 2),
            fixed(1, 1),
            expr(ExprInstance::GBool(false)),
        ] {
            assert!(is_number(&par), "{par:?} is a number");
        }
        for par in [
            expr(ExprInstance::GString("1".to_string())),
            list(vec![int(1)]),
            Par::default(),
        ] {
            assert!(!is_number(&par), "{par:?} is not a number");
        }
    }

    #[test]
    fn tensor_vector_of_checks_the_dimension_and_keeps_exact_values() {
        let dyadic = vector_of(&list(vec![int(1), double(0.5)]), Some(2), &unmetered())
            .expect("a dyadic vector");
        assert!(matches!(dyadic, ExactVector::Dyadic(_)));
        assert_eq!(
            dyadic.rationals(&unmetered()).expect("values").into_owned(),
            vec![ratio(1, 1), ratio(1, 2)]
        );
        let rational = vector_of(
            &list(vec![int(1), double(2.5), big_rat(1, 3), int(4)]),
            None,
            &unmetered(),
        )
        .expect("a rational vector");
        assert!(matches!(rational, ExactVector::Rational(_)));
        assert_eq!(
            rational
                .rationals(&unmetered())
                .expect("values")
                .into_owned(),
            vec![ratio(1, 1), ratio(5, 2), ratio(1, 3), ratio(4, 1)]
        );
        assert_eq!(
            vector_of(&list(vec![int(1)]), Some(2), &unmetered()).map(|vector| vector.len()),
            Err(NumError::DimensionMismatch {
                expected: 2,
                found: 1
            })
        );
        assert_eq!(
            vector_of(&int(1), None, &unmetered()).map(|vector| vector.len()),
            Err(NumError::NotAVector)
        );
        let tail = list(vec![int(1)]);
        let mut open = tail.clone();
        if let Some(ExprInstance::EListBody(body)) = open.exprs[0].expr_instance.as_mut() {
            body.remainder = Some(models::rhoapi::Var::default());
        }
        assert_eq!(
            vector_of(&open, None, &unmetered()).map(|vector| vector.len()),
            Err(NumError::NotAVector)
        );
        assert_eq!(
            vector_of(&list(vec![int(1), double(f64::NAN)]), None, &unmetered())
                .map(|vector| vector.len()),
            Err(NumError::NonFiniteValue)
        );
        assert_eq!(
            vector_of(
                &list(vec![int(1), expr(ExprInstance::GString("a".into()))]),
                None,
                &unmetered()
            )
            .map(|vector| vector.len()),
            Err(NumError::NotANumber)
        );
        assert!(vector_of(&list(Vec::new()), Some(0), &unmetered())
            .expect("an empty vector")
            .is_empty());
    }

    #[test]
    fn tensor_rounding_breaks_ties_to_even() {
        let two_53 = BigInt::one() << 53u8;
        let at = |offset: i64| Exact::from_integer(&two_53 + BigInt::from(offset));
        assert_eq!(
            round_to_f64(&at(1), &unmetered()),
            Ok(9_007_199_254_740_992.0)
        );
        assert_eq!(
            round_to_f64(&at(3), &unmetered()),
            Ok(9_007_199_254_740_996.0)
        );
        assert_eq!(
            round_to_f64(&at(-1), &unmetered()),
            Ok(9_007_199_254_740_991.0)
        );
        // Ties at the subnormal end: 2^-1075 rounds to 0, 3·2^-1075 to 2·2^-1074.
        assert_eq!(round_to_f64(&power(-1075), &unmetered()), Ok(0.0));
        assert_eq!(
            round_to_f64(&(power(-1075) * ratio(3, 1)), &unmetered()),
            Ok(f64::from_bits(2))
        );
        assert_eq!(
            round_to_f64(&-(power(-1075) * ratio(3, 1)), &unmetered()),
            Ok(-f64::from_bits(2))
        );
        // A value just below the least normal carries into it.
        let below = power(-1022) - power(-1080);
        assert_eq!(round_to_f64(&below, &unmetered()), Ok(f64::MIN_POSITIVE));
        assert_eq!(round_to_f64(&ratio(1, 3), &unmetered()), Ok(1.0 / 3.0));
        assert_eq!(round_to_f64(&ratio(-2, 3), &unmetered()), Ok(-2.0 / 3.0));
    }

    #[test]
    fn tensor_rounding_rejects_values_beyond_the_f64_range() {
        let max = exact_f64(f64::MAX);
        assert_eq!(round_to_f64(&max, &unmetered()), Ok(f64::MAX));
        assert_eq!(round_to_f64(&-max.clone(), &unmetered()), Ok(f64::MIN));
        // The midpoint between f64::MAX and 2^1024 rounds to the even 2^1024, beyond the range.
        let midpoint = &max + power(970);
        assert_eq!(
            round_to_f64(&midpoint, &unmetered()),
            Err(NumError::ValueOutOfRange)
        );
        assert_eq!(
            round_to_f64(&(&midpoint - power(900)), &unmetered()),
            Ok(f64::MAX)
        );
        assert_eq!(
            round_to_f64(&power(1024), &unmetered()),
            Err(NumError::ValueOutOfRange)
        );
        assert_eq!(
            round_to_f64(&-power(1100), &unmetered()),
            Err(NumError::ValueOutOfRange)
        );
    }

    #[test]
    fn tensor_square_roots_are_correctly_rounded() {
        assert_eq!(sqrt_to_f64(&ratio(9, 4), &unmetered()), Ok(1.5));
        assert_eq!(
            sqrt_to_f64(&ratio(2, 1), &unmetered()),
            Ok(std::f64::consts::SQRT_2)
        );
        assert_eq!(
            sqrt_to_f64(&ratio(1, 2), &unmetered()),
            Ok(std::f64::consts::FRAC_1_SQRT_2)
        );
        assert_eq!(sqrt_to_f64(&Exact::zero(), &unmetered()), Ok(0.0));
        assert_eq!(sqrt_to_f64(&power(2046), &unmetered()), Ok(2f64.powi(1023)));
        assert_eq!(
            sqrt_to_f64(&power(2048), &unmetered()),
            Err(NumError::ValueOutOfRange)
        );
        assert_eq!(
            sqrt_to_f64(&power(-2148), &unmetered()),
            Ok(f64::from_bits(1))
        );
        assert_eq!(sqrt_to_f64(&power(-2150), &unmetered()), Ok(0.0));
        assert_eq!(
            sqrt_to_f64(&ratio(-1, 4), &unmetered()),
            Err(NumError::NotANumber)
        );
        // √(25/36) is exactly 5/6, and RN_53(5/6) is the rounded quotient.
        assert_eq!(sqrt_to_f64(&ratio(25, 36), &unmetered()), Ok(5.0 / 6.0));
    }

    #[test]
    fn tensor_numeric_core_reserves_its_work_first() {
        let unbounded =
            || HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        let operations = HostWorkDimension::VerificationOperations;
        let vector = list(vec![
            big_rat(1, 3),
            double(0.1),
            fixed(7, 2),
            big_int(&BigInt::from(9)),
        ]);
        let run = |meter: &ScanMeter| -> Result<(f64, f64), NumError> {
            let values = vector_of(&vector, Some(4), meter)?;
            let values = values.rationals(meter)?;
            Ok((
                round_to_f64(&values[0], meter)?,
                sqrt_to_f64(&values[1], meter)?,
            ))
        };
        let budget = unbounded();
        let expected = run(&ScanMeter::new(Some(budget.clone()))).expect("an unbounded run");
        assert_eq!(run(&unmetered()), Ok(expected));
        let usage = budget.usage(operations).get();
        assert!(usage > 0, "the core reserves its work");
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        limits.set(operations, HostWorkLimit::new(usage - 1));
        let cut = ScanMeter::new(Some(HostWorkBudget::new(limits)));
        assert!(matches!(run(&cut), Err(NumError::HostWork(_))));
    }

    fn finite_f64() -> impl Strategy<Value = f64> {
        any::<u64>()
            .prop_map(f64::from_bits)
            .prop_filter("a finite value", |value| value.is_finite())
    }

    fn property_config() -> ProptestConfig {
        ProptestConfig {
            cases: 64,
            rng_seed: proptest::test_runner::RngSeed::Fixed(0x0F6E_5EED_2026_1008),
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    proptest! {
        #![proptest_config(property_config())]

        #[test]
        fn tensor_rounding_is_the_identity_on_f64_values(value in finite_f64()) {
            let rounded = round_to_f64(&exact_f64(value), &unmetered()).expect("a value in range");
            // -0 enters as 0, and every other value comes back with its bits.
            let expected = if value == 0.0 { 0.0 } else { value };
            prop_assert_eq!(rounded.to_bits(), expected.to_bits());
        }

        #[test]
        fn tensor_rounding_equals_the_ieee_quotient(
            numerator in -(1i64 << 53)..=(1i64 << 53),
            denominator in (-(1i64 << 53)..=(1i64 << 53)).prop_filter("not zero", |d| *d != 0),
        ) {
            // Both operands are exact f64 values, and IEEE division rounds correctly.
            let expected = numerator as f64 / denominator as f64;
            let rounded = round_to_f64(&ratio(numerator, denominator), &unmetered())
                .expect("a value in range");
            prop_assert_eq!(rounded, expected);
        }

        #[test]
        fn tensor_square_root_equals_the_ieee_square_root(value in finite_f64()) {
            let value = value.abs();
            let root = sqrt_to_f64(&exact_f64(value), &unmetered()).expect("a value in range");
            prop_assert_eq!(root.to_bits(), value.sqrt().to_bits());
        }

        #[test]
        fn tensor_float32_ingress_equals_the_widened_value(bits in any::<u32>()) {
            let value = f32::from_bits(bits);
            prop_assume!(value.is_finite());
            prop_assert_eq!(value_of(&float32(value)), Ok(exact_f64(f64::from(value))));
        }
    }
}
