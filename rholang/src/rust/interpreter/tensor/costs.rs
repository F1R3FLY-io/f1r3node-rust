//! The host-work reservations of the numeric core (P6, design v3 4.1; decisions 40 and 42).
//!
//! Each step of the core reserves its work on the meter of the job before it makes it. The
//! amounts are functions of the sizes of the operands, measured in 64-bit limbs, so play and
//! replay reserve the same amounts, and no amount depends on a cache. The three amounts charge
//! `VerificationOperations`, `VerificationBytes` and `SearchStateBytes` (`ScanMeter::charge`).
//!
//! | Step | Operations | Scanned bytes | Backing bytes |
//! |---|---|---|---|
//! | decode `n` bytes | `1 + ⌈n/8⌉` | `n` | `n` |
//! | ingress of a float | `FLOAT_INGRESS_OPERATIONS` | `FLOAT_INGRESS_BYTES` | `FLOAT_INGRESS_BYTES` |
//! | product `a × b` | `ℓ(a)·ℓ(b)` | `8(ℓ(a)+ℓ(b))` | `8(ℓ(a)+ℓ(b))` |
//! | sum `a + b` | `max(ℓ(a), ℓ(b))` | `8(ℓ(a)+ℓ(b))` | `8(max+1)` |
//! | shift `a << k` | `ℓ(a) + ⌈k/64⌉` | `8ℓ(a)` | `8(ℓ(a) + ⌈k/64⌉)` |
//! | gcd and division | `L²` for `L = ℓ(a)+ℓ(b)` | `8L` | `8L` |
//! | `10^s` | `L²·(1 + ⌊log₂ s⌋)` for `L = ℓ(10^s)` | 0 | `8L` |
//! | value of `m·2^e` | `ℓ(m) + ⌈∣e∣/64⌉ + 1` | `8ℓ(m)` | `8(ℓ(m) + ⌈∣e∣/64⌉ + 1)` |
//! | operation on two rationals | `2L²` for `L` their limbs | `8L` | `8L` |
//! | comparison of two rationals | `L²` | `8L` | `8L` |
//! | `⌊log₂ x⌋` of a rational | `L` | `8L` | `8L` |
//! | rounding to `f64` | `L²` for the scaled operands | `8L` | `16L` |
//! | square root to `f64` | `L²·(⌈log₂ 64L⌉ + 2)` | `8L` | `24L` |
//!
//! Here `ℓ(x)` is the number of limbs of `x`, at least 1, and the limbs of a rational are those
//! of its numerator and its denominator.
//!
//! Every amount is computed with checked arithmetic (FR-6). An amount that does not fit in a
//! `usize` can never be reserved, so its reservation fails with `HostWorkRejected`. Without a
//! budget the meter accepts every reservation, and no amount is computed.

use num_bigint::BigInt;
use num_rational::BigRational;
use rspace_plus_plus::rspace::errors::RSpaceError;

use crate::rust::interpreter::reified::costs::ScanMeter;

/// The operations of the ingress of one float: the decode of its bits and the construction of
/// its one-limb mantissa. A float enters as a dyadic number, so its ingress needs no reduction.
pub const FLOAT_INGRESS_OPERATIONS: usize = 2;

/// The bytes of the ingress of one float: the bits that it reads, and the limb that it writes.
pub const FLOAT_INGRESS_BYTES: usize = 8;

/// The amounts of one step: operations, scanned bytes and backing bytes.
type Amounts = (usize, usize, usize);

/// Reserves the amounts that `amounts` computes, or rejects them when they overflow. Without a
/// budget nothing is computed.
fn reserve(
    meter: &ScanMeter,
    amounts: impl FnOnce() -> Option<Amounts>,
) -> Result<(), RSpaceError> {
    if !meter.is_metered() {
        return Ok(());
    }
    let (operations, scanned, backing) = amounts().ok_or(RSpaceError::HostWorkRejected)?;
    meter.charge(operations, scanned, backing)
}

/// The limbs of `x`, at least 1.
pub fn limbs(x: &BigInt) -> Option<usize> {
    usize::try_from(x.bits().div_ceil(64))
        .ok()
        .map(|count| count.max(1))
}

/// The limbs of a rational: its numerator and its denominator.
pub fn rational_limbs(x: &BigRational) -> Option<usize> {
    limbs(x.numer())?.checked_add(limbs(x.denom())?)
}

/// The limbs of a shift by `shift` bits.
fn shift_limbs(shift: u64) -> Option<usize> { usize::try_from(shift.div_ceil(64)).ok() }

fn bytes(count: usize) -> Option<usize> { count.checked_mul(8) }

fn square(count: usize) -> Option<usize> { count.checked_mul(count) }

/// The decode of `n` bytes of a two's complement integer.
pub fn reserve_bytes_decode(length: usize, meter: &ScanMeter) -> Result<(), RSpaceError> {
    reserve(meter, || {
        Some((length.div_ceil(8).checked_add(1)?, length, length))
    })
}

/// The ingress of one float.
pub fn reserve_float_ingress(meter: &ScanMeter) -> Result<(), RSpaceError> {
    reserve(meter, || {
        Some((
            FLOAT_INGRESS_OPERATIONS,
            FLOAT_INGRESS_BYTES,
            FLOAT_INGRESS_BYTES,
        ))
    })
}

/// The gcd that reduces `a / b`.
pub fn reserve_gcd(a: &BigInt, b: &BigInt, meter: &ScanMeter) -> Result<(), RSpaceError> {
    reserve(meter, || {
        let length = limbs(a)?.checked_add(limbs(b)?)?;
        Some((square(length)?, bytes(length)?, bytes(length)?))
    })
}

/// The power `10^scale` by squaring: `⌊log₂ scale⌋ + 1` products of at most `ℓ(10^scale)` limbs.
pub fn reserve_power_of_ten(scale: u32, meter: &ScanMeter) -> Result<(), RSpaceError> {
    reserve(meter, || {
        // log₂ 10 < 3.3220, so 10^scale has at most ⌈3.322·scale⌉ + 1 bits.
        let bits = u64::from(scale)
            .checked_mul(3322)?
            .div_ceil(1000)
            .checked_add(1)?;
        let length = shift_limbs(bits)?.max(1);
        let steps = usize::try_from(u32::BITS - scale.leading_zeros())
            .ok()?
            .max(1);
        Some((square(length)?.checked_mul(steps)?, 0, bytes(length)?))
    })
}

/// The product `a × b` of two integers.
pub fn reserve_product(a: &BigInt, b: &BigInt, meter: &ScanMeter) -> Result<(), RSpaceError> {
    reserve(meter, || {
        let (left, right) = (limbs(a)?, limbs(b)?);
        let both = bytes(left.checked_add(right)?)?;
        Some((left.checked_mul(right)?, both, both))
    })
}

/// The sum `a + b` of two integers.
pub fn reserve_sum(a: &BigInt, b: &BigInt, meter: &ScanMeter) -> Result<(), RSpaceError> {
    reserve(meter, || {
        let (left, right) = (limbs(a)?, limbs(b)?);
        let wider = left.max(right);
        Some((
            wider,
            bytes(left.checked_add(right)?)?,
            bytes(wider.checked_add(1)?)?,
        ))
    })
}

/// The shift `a << shift` of an integer.
pub fn reserve_shift(a: &BigInt, shift: u64, meter: &ScanMeter) -> Result<(), RSpaceError> {
    reserve(meter, || {
        let length = limbs(a)?.checked_add(shift_limbs(shift)?)?;
        Some((length, bytes(limbs(a)?)?, bytes(length)?))
    })
}

/// The value `mantissa · 2^exponent` as a reduced rational: a shift of the mantissa and the
/// power of two of the denominator, with no gcd (the reduction strips trailing zeros).
pub fn reserve_dyadic_value(
    mantissa: &BigInt,
    exponent: i64,
    meter: &ScanMeter,
) -> Result<(), RSpaceError> {
    reserve(meter, || {
        let length = limbs(mantissa)?
            .checked_add(shift_limbs(exponent.unsigned_abs())?)?
            .checked_add(1)?;
        Some((length, bytes(limbs(mantissa)?)?, bytes(length)?))
    })
}

/// An operation on two rationals: the cross products, the reduction and the result.
pub fn reserve_rational(
    a: &BigRational,
    b: &BigRational,
    meter: &ScanMeter,
) -> Result<(), RSpaceError> {
    reserve(meter, || {
        let length = rational_limbs(a)?.checked_add(rational_limbs(b)?)?;
        Some((
            square(length)?.checked_mul(2)?,
            bytes(length)?,
            bytes(length)?,
        ))
    })
}

/// The comparison of two rationals by cross-multiplication.
pub fn reserve_rational_compare(
    a: &BigRational,
    b: &BigRational,
    meter: &ScanMeter,
) -> Result<(), RSpaceError> {
    reserve(meter, || {
        let length = rational_limbs(a)?.checked_add(rational_limbs(b)?)?;
        Some((square(length)?, bytes(length)?, bytes(length)?))
    })
}

/// `⌊log₂ x⌋` of a positive rational: the bit lengths, one shift and one comparison.
pub fn reserve_log2(x: &BigRational, meter: &ScanMeter) -> Result<(), RSpaceError> {
    reserve(meter, || {
        let length = rational_limbs(x)?;
        Some((length, bytes(length)?, bytes(length)?))
    })
}

/// The rounding of the rational `x` to an `f64` with the quantum `2^quantum`: the shift of one
/// operand by `|quantum|` bits, the division and the rounding.
pub fn reserve_rounding(
    x: &BigRational,
    quantum: i64,
    meter: &ScanMeter,
) -> Result<(), RSpaceError> {
    reserve(meter, || {
        let length = rational_limbs(x)?
            .checked_add(shift_limbs(quantum.unsigned_abs())?)?
            .checked_add(1)?;
        Some((
            square(length)?,
            bytes(length)?,
            bytes(length)?.checked_mul(2)?,
        ))
    })
}

/// The rounding of the square root of the rational `x ≥ 0` to an `f64` with the quantum
/// `2^quantum`: the shift by `2|quantum|` bits, the division, the integer square root (Newton
/// steps of products of `L` limbs) and the comparison of the rounding.
pub fn reserve_square_root(
    x: &BigRational,
    quantum: i64,
    meter: &ScanMeter,
) -> Result<(), RSpaceError> {
    reserve(meter, || {
        let length = rational_limbs(x)?
            .checked_add(shift_limbs(quantum.unsigned_abs().checked_mul(2)?)?)?
            .checked_add(1)?;
        let bits = length.checked_mul(64)?;
        let steps = usize::try_from(usize::BITS - bits.leading_zeros())
            .ok()?
            .checked_add(2)?;
        Some((
            square(length)?.checked_mul(steps)?,
            bytes(length)?,
            bytes(length)?.checked_mul(3)?,
        ))
    })
}
