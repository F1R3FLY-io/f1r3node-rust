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
//! | encode an integer `x` | `ℓ(x) + 1` | `8ℓ(x)` | `8ℓ(x) + 1` |
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

/// The encode of the integer `x` into its big-endian two's complement bytes: one pass over its
/// limbs, which writes at most `8ℓ(x) + 1` bytes (`rho:lang:vector`, P6 step 6).
pub fn reserve_integer_encode(x: &BigInt, meter: &ScanMeter) -> Result<(), RSpaceError> {
    reserve(meter, || {
        let length = limbs(x)?;
        Some((
            length.checked_add(1)?,
            bytes(length)?,
            bytes(length)?.checked_add(1)?,
        ))
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

#[cfg(test)]
mod tests {
    //! The reservations of the numeric core against the table of this module (P6, tracker leaf
    //! `rrs-reducer-query-costs`; decisions 40 and 42). The leaf asks for boundary and overflow
    //! tests that keep the charges deterministic: each step reserves the amounts of the table,
    //! a limit equal to an amount accepts it, a limit one unit lower rejects it, and an amount
    //! that does not fit in a `usize` rejects without a reservation.

    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
    use num_bigint::BigInt;
    use num_rational::BigRational;
    use rspace_plus_plus::rspace::errors::RSpaceError;

    use super::*;
    use crate::rust::interpreter::host_work::HostWorkBudget;

    /// The dimensions of `ScanMeter::charge`, in the order of the amounts of the table.
    const DIMENSIONS: [HostWorkDimension; 3] = [
        HostWorkDimension::VerificationOperations,
        HostWorkDimension::VerificationBytes,
        HostWorkDimension::SearchStateBytes,
    ];

    /// A step of the core on `meter`.
    type Step = Box<dyn Fn(&ScanMeter) -> Result<(), RSpaceError>>;

    /// A budget with the limits `limits` on the three dimensions of the table, and no limit on
    /// the others.
    fn budget(limits: [u64; 3]) -> HostWorkBudget {
        let mut all = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        for (dimension, limit) in DIMENSIONS.into_iter().zip(limits) {
            all.set(dimension, HostWorkLimit::new(limit));
        }
        HostWorkBudget::new(all)
    }

    /// The amounts that `step` reserves on a budget without limits.
    fn reserved(step: &Step) -> [u64; 3] {
        let budget = budget([u64::MAX; 3]);
        step(&ScanMeter::new(Some(budget.clone()))).expect("a budget without limits accepts");
        DIMENSIONS.map(|dimension| budget.usage(dimension).get())
    }

    fn int(value: i64) -> BigInt { BigInt::from(value) }

    /// `2^64 + 1`, an integer of two limbs.
    fn two_limbs() -> BigInt { (BigInt::from(1) << 64) + 1 }

    fn ratio(numerator: i64, denominator: i64) -> BigRational {
        BigRational::new(int(numerator), int(denominator))
    }

    /// Each step of the core with operands of one and two limbs, and its amounts from the table
    /// of this module: operations, scanned bytes and backing bytes.
    fn table() -> Vec<(&'static str, Step, [u64; 3])> {
        vec![
            (
                "decode 17 bytes: 1 + ⌈17/8⌉, 17, 17",
                Box::new(|meter: &ScanMeter| reserve_bytes_decode(17, meter)),
                [4, 17, 17],
            ),
            (
                "encode an integer of 2 limbs: 2 + 1, 16, 16 + 1",
                Box::new(|meter: &ScanMeter| reserve_integer_encode(&two_limbs(), meter)),
                [3, 16, 17],
            ),
            ("ingress of a float", Box::new(reserve_float_ingress), [
                2, 8, 8,
            ]),
            (
                "gcd of 1 and 2 limbs: L = 3, L², 8L, 8L",
                Box::new(|meter: &ScanMeter| reserve_gcd(&int(3), &two_limbs(), meter)),
                [9, 24, 24],
            ),
            (
                "10^5: L = 1, 3 squarings, 0, 8L",
                Box::new(|meter: &ScanMeter| reserve_power_of_ten(5, meter)),
                [3, 0, 8],
            ),
            (
                "product of 1 and 2 limbs: 1·2, 8·3, 8·3",
                Box::new(|meter: &ScanMeter| reserve_product(&int(3), &two_limbs(), meter)),
                [2, 24, 24],
            ),
            (
                "sum of 1 and 2 limbs: 2, 8·3, 8·(2 + 1)",
                Box::new(|meter: &ScanMeter| reserve_sum(&int(3), &two_limbs(), meter)),
                [2, 24, 24],
            ),
            (
                "shift of 1 limb by 130 bits: 1 + 3, 8, 8·4",
                Box::new(|meter: &ScanMeter| reserve_shift(&int(3), 130, meter)),
                [4, 8, 32],
            ),
            (
                "value of 3·2^-70: 1 + 2 + 1, 8, 8·4",
                Box::new(|meter: &ScanMeter| reserve_dyadic_value(&int(3), -70, meter)),
                [4, 8, 32],
            ),
            (
                "operation on 1/3 and 5/7: L = 4, 2L², 8L, 8L",
                Box::new(|meter: &ScanMeter| reserve_rational(&ratio(1, 3), &ratio(5, 7), meter)),
                [32, 32, 32],
            ),
            (
                "comparison of 1/3 and 5/7: L = 4, L², 8L, 8L",
                Box::new(|meter: &ScanMeter| {
                    reserve_rational_compare(&ratio(1, 3), &ratio(5, 7), meter)
                }),
                [16, 32, 32],
            ),
            (
                "⌊log₂ 1/3⌋: L = 2, L, 8L, 8L",
                Box::new(|meter: &ScanMeter| reserve_log2(&ratio(1, 3), meter)),
                [2, 16, 16],
            ),
            (
                "rounding of 1/3 with the quantum 2^-60: L = 2 + 1 + 1, L², 8L, 16L",
                Box::new(|meter: &ScanMeter| reserve_rounding(&ratio(1, 3), -60, meter)),
                [16, 32, 64],
            ),
            (
                "square root of 2 with the quantum 2^-52: L = 2 + 2 + 1, L²·(9 + 2), 8L, 24L",
                Box::new(|meter: &ScanMeter| reserve_square_root(&ratio(2, 1), -52, meter)),
                [275, 40, 120],
            ),
        ]
    }

    /// Each step reserves exactly the amounts of the table, twice the same, so a charge is a
    /// function of the sizes of the operands only.
    #[test]
    fn reducer_query_costs_follow_the_table_of_the_module() {
        for (label, step, expected) in table() {
            assert_eq!(reserved(&step), expected, "{label}");
            assert_eq!(reserved(&step), expected, "{label}: a second run");
        }
        let small = reserved(
            &(Box::new(|meter: &ScanMeter| reserve_product(&int(3), &int(5), meter)) as Step),
        );
        let large = reserved(
            &(Box::new(|meter: &ScanMeter| reserve_product(&two_limbs(), &two_limbs(), meter))
                as Step),
        );
        assert_eq!(small, [1, 16, 16]);
        assert_eq!(
            large,
            [4, 32, 32],
            "the amounts grow with the limbs of the operands"
        );
    }

    /// A limit equal to each amount accepts the step, and a limit one unit lower on any one
    /// dimension rejects it with `HostWorkRejected`.
    #[test]
    fn reducer_query_costs_accept_at_the_limit_and_reject_below_it() {
        for (label, step, expected) in table() {
            let exact = budget(expected);
            assert_eq!(step(&ScanMeter::new(Some(exact))), Ok(()), "{label}");
            for index in 0..3 {
                if expected[index] == 0 {
                    continue;
                }
                let mut limits = [u64::MAX; 3];
                limits[index] = expected[index] - 1;
                assert_eq!(
                    step(&ScanMeter::new(Some(budget(limits)))),
                    Err(RSpaceError::HostWorkRejected),
                    "{label}: one unit below the amount of {:?}",
                    DIMENSIONS[index]
                );
            }
        }
    }

    /// An amount that does not fit in a `usize` rejects with `HostWorkRejected` before any
    /// reservation, also on a budget without limits. Without a budget, no amount is computed and
    /// the step is accepted.
    #[test]
    fn reducer_query_costs_reject_amounts_that_overflow() {
        let overflowing: Vec<(&str, Step)> = vec![
            (
                "rounding with the quantum 2^i64::MIN: L² overflows",
                Box::new(|meter: &ScanMeter| reserve_rounding(&ratio(1, 3), i64::MIN, meter)),
            ),
            (
                "square root with the quantum 2^i64::MIN: 2|quantum| overflows",
                Box::new(|meter: &ScanMeter| reserve_square_root(&ratio(2, 1), i64::MIN, meter)),
            ),
        ];
        for (label, step) in overflowing {
            let unlimited = budget([u64::MAX; 3]);
            assert_eq!(
                step(&ScanMeter::new(Some(unlimited.clone()))),
                Err(RSpaceError::HostWorkRejected),
                "{label}"
            );
            assert_eq!(
                DIMENSIONS.map(|dimension| unlimited.usage(dimension).get()),
                [0, 0, 0],
                "{label}: nothing is reserved"
            );
            assert_eq!(step(&ScanMeter::new(None)), Ok(()), "{label}: no budget");
        }
    }
}
