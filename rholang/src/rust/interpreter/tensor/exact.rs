//! The exact kernels and the exact order of the built-in metrics (P6, design v3 4.1 and 4.4).
//!
//! A VectorDB space decides every selection with these functions, so every node makes the same
//! decision (decision 54, ledger D-5). No decision takes a square root or rounds: cosine compares
//! squared forms, euclidean compares squared distances, and jaccard cross-multiplies.
//!
//! For a query vector `q` and a record vector `e`:
//!
//! | Metric | Kind | Exact score | Better | Bound |
//! |---|---|---|---|---|
//! | `cosine` | similarity | `q·e` and `‖e‖²` | larger `q·e / ‖e‖` | `q·e ≥ 0` and `(q·e)² ≥ t²‖q‖²‖e‖²` |
//! | `dot` | similarity | `q·e` | larger | `q·e ≥ t` |
//! | `euclidean` | distance | `δ² = ‖q − e‖²` | smaller | `δ² ≤ ρ²` |
//! | `manhattan` | distance | `‖q − e‖₁` | smaller | `‖q − e‖₁ ≤ ρ` |
//! | `hamming` | distance | the differing positions | fewer | count `≤ ρ` |
//! | `jaccard` | similarity | `|q ∧ e|` and `|q ∨ e|` | larger ratio | `|q ∧ e| ≥ t·|q ∨ e|` |
//!
//! The bound of cosine in the table holds for a threshold `t ≥ 0`, the range of the syntax
//! contract. [`within`] decides every rational `t` exactly.
//!
//! Two cosine scores `(u, n)` and `(v, n')` of one query, with `u = q·e`, `n = ‖e‖²`, `v = q·e'`
//! and `n' = ‖e'‖²`, compare without a root:
//!
//! ```text
//! COSINE-BETTER(u, n, v, n'):            ▷ cos = u/(‖q‖√n), cos' = v/(‖q‖√n'), n, n' > 0
//!   u ≥ 0 > v:   the first is better
//!   v ≥ 0 > u:   the second is better
//!   u, v ≥ 0:    compare u²·n' with v²·n; the larger is better
//!   u, v < 0:    compare u²·n' with v²·n; the smaller is better
//! ```
//!
//! The map `x ↦ sign(x)·x²` is strictly increasing, so it keeps the order of the cosines, and
//! `sign(cos)·cos² = sign(u)·u² / (‖q‖²·n)`. The common factor `‖q‖²` cancels.
//!
//! A score is undefined, and the record is not eligible, in three cases:
//!
//! - the record has a zero vector under cosine;
//! - the query or the record has an element other than 0 and 1 under hamming or jaccard;
//! - no position is set in either vector under jaccard.
//!
//! An undefined score is never an error. A zero query vector under cosine has no direction, so
//! [`Query::new`] refuses it, and the caller fails with `ZeroQueryVector`.
//!
//! Dot products of dyadic vectors sum integers: each product `m·m'·2^(e+e')` joins one exact
//! integer accumulator whose unit is the least exponent seen so far (DYADIC-SUM). Any other pair
//! of vectors sums reduced rationals.
//!
//! ```text
//! DYADIC-SUM-ADD(S·2^E, t·2^F):          ▷ the sum stays exact, with no rounding
//!   F ≥ E:   S ← S + t·2^(F−E)
//!   F < E:   S ← S·2^(E−F) + t;  E ← F
//! ```

use std::cmp::Ordering;

use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use rspace_plus_plus::rspace::errors::RSpaceError;

use super::costs;
use super::num::{round_to_f64, scaled, sqrt_to_f64, Dyadic, Exact, ExactVector, NumError};
use crate::rust::interpreter::reified::costs::ScanMeter;

/// A built-in metric of a VectorDB space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Metric {
    Cosine,
    Dot,
    Euclidean,
    Manhattan,
    Hamming,
    Jaccard,
}

impl Metric {
    /// Every metric, in the order of the syntax contract.
    pub const ALL: [Metric; 6] = [
        Metric::Cosine,
        Metric::Dot,
        Metric::Euclidean,
        Metric::Manhattan,
        Metric::Hamming,
        Metric::Jaccard,
    ];

    /// The metric of its name, or `None`.
    pub fn parse(name: &str) -> Option<Metric> {
        Metric::ALL.into_iter().find(|metric| metric.name() == name)
    }

    /// The name of the metric in the syntax contract.
    pub fn name(&self) -> &'static str {
        match self {
            Metric::Cosine => "cosine",
            Metric::Dot => "dot",
            Metric::Euclidean => "euclidean",
            Metric::Manhattan => "manhattan",
            Metric::Hamming => "hamming",
            Metric::Jaccard => "jaccard",
        }
    }

    /// True for a similarity (larger is better), false for a distance (smaller is better).
    pub fn is_similarity(&self) -> bool {
        matches!(self, Metric::Cosine | Metric::Dot | Metric::Jaccard)
    }
}

/// The exact score of one record for one query. It holds what every comparison needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Score {
    /// `q·e` and `‖e‖² > 0`. The norm of the query is common to every record of one query.
    Cosine { dot: Exact, norm2: Exact },
    /// `q·e`.
    Dot(Exact),
    /// The squared euclidean distance `‖q − e‖²`.
    Euclidean(Exact),
    /// The manhattan distance `‖q − e‖₁`.
    Manhattan(Exact),
    /// The number of positions where the binary vectors differ.
    Hamming(u64),
    /// The positions set in both binary vectors and in either, with `union > 0`.
    Jaccard { inter: u64, union: u64 },
}

/// The bound of a query: a threshold `t` on a similarity, or a radius `ρ` on a distance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bound {
    Threshold(Exact),
    Radius(Exact),
}

/// The exact query of a receive: its vector and its metric, with the parts that every score of
/// the query shares.
#[derive(Clone, Debug)]
pub struct Query {
    /// The query vector.
    pub vector: ExactVector,
    /// The metric of the query.
    pub metric: Metric,
    /// `‖q‖² > 0` under cosine, and 0 under every other metric.
    pub norm2: Exact,
    /// The bits of the query under hamming and jaccard, or `None`: under another metric, or
    /// when the query is not binary (then no record is eligible).
    pub bits: Option<Vec<bool>>,
}

impl Query {
    /// The query of `vector` under `metric`. A zero vector under cosine has no direction, so it
    /// is `None`, and the caller fails with `ZeroQueryVector`. The work is reserved first.
    pub fn new(
        vector: ExactVector,
        metric: Metric,
        meter: &ScanMeter,
    ) -> Result<Option<Query>, NumError> {
        let (norm2, bits) = match metric {
            Metric::Cosine => match dot(&vector, &vector, meter)? {
                norm2 if norm2.is_zero() => return Ok(None),
                norm2 => (norm2, None),
            },
            Metric::Hamming | Metric::Jaccard => (Exact::zero(), bits(&vector, meter)?),
            Metric::Dot | Metric::Euclidean | Metric::Manhattan => (Exact::zero(), None),
        };
        Ok(Some(Query {
            vector,
            metric,
            norm2,
            bits,
        }))
    }
}

/// An exact sum of dyadic terms: the integer `sum` and the exponent `unit` of its last place
/// (DYADIC-SUM). A term with a lower exponent shifts the sum, so the sum never rounds.
struct DyadicSum {
    sum: BigInt,
    unit: Option<i64>,
}

impl DyadicSum {
    fn new() -> Self {
        Self {
            sum: BigInt::zero(),
            unit: None,
        }
    }

    /// Adds `term · 2^exponent`. Each shift and each sum is reserved before it is made.
    fn add(&mut self, term: BigInt, exponent: i64, meter: &ScanMeter) -> Result<(), NumError> {
        if term.is_zero() {
            return Ok(());
        }
        let Some(unit) = self.unit else {
            self.sum = term;
            self.unit = Some(exponent);
            return Ok(());
        };
        match exponent.cmp(&unit) {
            Ordering::Less => {
                let shift = exponent_gap(unit, exponent)?;
                costs::reserve_shift(&self.sum, shift, meter)?;
                let aligned = std::mem::take(&mut self.sum) << shift;
                costs::reserve_sum(&aligned, &term, meter)?;
                self.sum = aligned + term;
                self.unit = Some(exponent);
            }
            Ordering::Equal | Ordering::Greater => {
                let shift = exponent_gap(exponent, unit)?;
                costs::reserve_shift(&term, shift, meter)?;
                let aligned = term << shift;
                costs::reserve_sum(&self.sum, &aligned, meter)?;
                self.sum += aligned;
            }
        }
        Ok(())
    }

    /// The exact value of the sum, reserved first. A sum of no term is zero.
    fn value(self, meter: &ScanMeter) -> Result<Exact, NumError> {
        match self.unit {
            None => Ok(Exact::zero()),
            Some(unit) => {
                costs::reserve_dyadic_value(&self.sum, unit, meter)?;
                Ok(scaled(self.sum, unit))
            }
        }
    }
}

/// `high − low ≥ 0` for two exponents, as a shift. An exponent gap beyond `i64` is
/// `ValueOutOfRange`.
fn exponent_gap(high: i64, low: i64) -> Result<u64, NumError> {
    high.checked_sub(low)
        .map(i64::unsigned_abs)
        .ok_or(NumError::ValueOutOfRange)
}

/// The exponent `a + b` of a product. An exponent beyond `i64` is `ValueOutOfRange`.
fn exponent_sum(a: i64, b: i64) -> Result<i64, NumError> {
    a.checked_add(b).ok_or(NumError::ValueOutOfRange)
}

/// `count · width` bytes, or `HostWorkRejected` when the amount overflows.
fn amount(count: usize, width: usize) -> Result<usize, NumError> {
    count
        .checked_mul(width)
        .ok_or(NumError::HostWork(RSpaceError::HostWorkRejected))
}

/// `x − y` of two dyadic numbers, as an integer and the exponent of its unit.
fn dyadic_difference(x: &Dyadic, y: &Dyadic, meter: &ScanMeter) -> Result<(BigInt, i64), NumError> {
    let unit = x.exponent.min(y.exponent);
    let (left_shift, right_shift) = (
        exponent_gap(x.exponent, unit)?,
        exponent_gap(y.exponent, unit)?,
    );
    costs::reserve_shift(&x.mantissa, left_shift, meter)?;
    costs::reserve_shift(&y.mantissa, right_shift, meter)?;
    let (left, right) = (&x.mantissa << left_shift, &y.mantissa << right_shift);
    costs::reserve_sum(&left, &right, meter)?;
    Ok((left - right, unit))
}

/// `a·b`, exact. The caller has checked that both vectors have one length.
pub fn dot(a: &ExactVector, b: &ExactVector, meter: &ScanMeter) -> Result<Exact, NumError> {
    match (a, b) {
        (ExactVector::Dyadic(left), ExactVector::Dyadic(right)) => {
            let mut sum = DyadicSum::new();
            for (x, y) in left.iter().zip(right) {
                costs::reserve_product(&x.mantissa, &y.mantissa, meter)?;
                sum.add(
                    &x.mantissa * &y.mantissa,
                    exponent_sum(x.exponent, y.exponent)?,
                    meter,
                )?;
            }
            sum.value(meter)
        }
        _ => {
            let (left, right) = (a.rationals(meter)?, b.rationals(meter)?);
            let mut sum = Exact::zero();
            for (x, y) in left.iter().zip(right.iter()) {
                costs::reserve_rational(x, y, meter)?;
                let product = x * y;
                costs::reserve_rational(&sum, &product, meter)?;
                sum += product;
            }
            Ok(sum)
        }
    }
}

/// The squared euclidean distance `‖a − b‖²`, exact.
pub fn distance2(a: &ExactVector, b: &ExactVector, meter: &ScanMeter) -> Result<Exact, NumError> {
    match (a, b) {
        (ExactVector::Dyadic(left), ExactVector::Dyadic(right)) => {
            let mut sum = DyadicSum::new();
            for (x, y) in left.iter().zip(right) {
                let (delta, unit) = dyadic_difference(x, y, meter)?;
                costs::reserve_product(&delta, &delta, meter)?;
                sum.add(&delta * &delta, exponent_sum(unit, unit)?, meter)?;
            }
            sum.value(meter)
        }
        _ => {
            let (left, right) = (a.rationals(meter)?, b.rationals(meter)?);
            let mut sum = Exact::zero();
            for (x, y) in left.iter().zip(right.iter()) {
                costs::reserve_rational(x, y, meter)?;
                let delta = x - y;
                costs::reserve_rational(&delta, &delta, meter)?;
                let square = &delta * &delta;
                costs::reserve_rational(&sum, &square, meter)?;
                sum += square;
            }
            Ok(sum)
        }
    }
}

/// The manhattan distance `‖a − b‖₁`, exact.
pub fn manhattan(a: &ExactVector, b: &ExactVector, meter: &ScanMeter) -> Result<Exact, NumError> {
    match (a, b) {
        (ExactVector::Dyadic(left), ExactVector::Dyadic(right)) => {
            let mut sum = DyadicSum::new();
            for (x, y) in left.iter().zip(right) {
                let (delta, unit) = dyadic_difference(x, y, meter)?;
                let magnitude = match delta.is_negative() {
                    true => -delta,
                    false => delta,
                };
                sum.add(magnitude, unit, meter)?;
            }
            sum.value(meter)
        }
        _ => {
            let (left, right) = (a.rationals(meter)?, b.rationals(meter)?);
            let mut sum = Exact::zero();
            for (x, y) in left.iter().zip(right.iter()) {
                costs::reserve_rational(x, y, meter)?;
                let delta = x - y;
                costs::reserve_rational(&sum, &delta, meter)?;
                sum += delta.abs();
            }
            Ok(sum)
        }
    }
}

/// The bit of a dyadic number that is 0 or 1, or `None`. `m·2^e` is 1 exactly when `m` is the
/// power of two `2^(−e)`.
fn bit_of_dyadic(form: &Dyadic) -> Option<bool> {
    if form.mantissa.is_zero() {
        return Some(false);
    }
    let position = u64::try_from(form.exponent.checked_neg()?).ok()?;
    let one = form.mantissa.is_positive()
        && form.mantissa.trailing_zeros() == Some(position)
        && form.mantissa.bits() == position + 1;
    one.then_some(true)
}

/// The bit of a rational that is 0 or 1, or `None`.
fn bit_of_rational(value: &Exact) -> Option<bool> {
    match (
        value.is_integer(),
        value.numer().is_zero(),
        value.numer() == &BigInt::from(1),
    ) {
        (true, true, _) => Some(false),
        (true, false, true) => Some(true),
        (true, false, false) | (false, _, _) => None,
    }
}

/// The bits of a binary vector, or `None` when an element is not 0 or 1. The buffer and the
/// test of each element are reserved first.
pub fn bits(vector: &ExactVector, meter: &ScanMeter) -> Result<Option<Vec<bool>>, NumError> {
    meter.reserve_buffer::<bool>(vector.len())?;
    meter.charge(vector.len(), amount(vector.len(), size_of::<Exact>())?, 0)?;
    let mut bits = Vec::with_capacity(vector.len());
    let mut keep = |bit: Option<bool>| match bit {
        Some(bit) => {
            bits.push(bit);
            true
        }
        None => false,
    };
    let complete = match vector {
        ExactVector::Dyadic(forms) => forms.iter().map(bit_of_dyadic).all(&mut keep),
        ExactVector::Rational(values) => values.iter().map(bit_of_rational).all(&mut keep),
    };
    Ok(complete.then_some(bits))
}

/// A count of positions as a `u64`.
fn count_of(count: usize) -> Result<u64, NumError> {
    u64::try_from(count).map_err(|_| NumError::ValueOutOfRange)
}

/// The hamming distance of two bit vectors of one length.
pub fn hamming(left: &[bool], right: &[bool], meter: &ScanMeter) -> Result<u64, NumError> {
    meter.charge(left.len(), amount(left.len(), 2)?, 0)?;
    count_of(left.iter().zip(right).filter(|(x, y)| x != y).count())
}

/// The intersection and the union of two bit vectors of one length, or `None` when the union
/// is empty.
pub fn jaccard(
    left: &[bool],
    right: &[bool],
    meter: &ScanMeter,
) -> Result<Option<(u64, u64)>, NumError> {
    meter.charge(left.len(), amount(left.len(), 2)?, 0)?;
    let (inter, union) = left
        .iter()
        .zip(right)
        .fold((0usize, 0usize), |(inter, union), (x, y)| {
            (inter + usize::from(*x && *y), union + usize::from(*x || *y))
        });
    match union {
        0 => Ok(None),
        _ => Ok(Some((count_of(inter)?, count_of(union)?))),
    }
}

/// The score of the record vector `record` for `query`, or `None` when it is undefined. The
/// caller has checked that the record has the dimension of the space.
pub fn score(
    query: &Query,
    record: &ExactVector,
    meter: &ScanMeter,
) -> Result<Option<Score>, NumError> {
    let q = &query.vector;
    Ok(match query.metric {
        Metric::Cosine => match dot(record, record, meter)? {
            norm2 if norm2.is_zero() => None,
            norm2 => Some(Score::Cosine {
                dot: dot(q, record, meter)?,
                norm2,
            }),
        },
        Metric::Dot => Some(Score::Dot(dot(q, record, meter)?)),
        Metric::Euclidean => Some(Score::Euclidean(distance2(q, record, meter)?)),
        Metric::Manhattan => Some(Score::Manhattan(manhattan(q, record, meter)?)),
        Metric::Hamming => match &query.bits {
            Some(left) => match bits(record, meter)? {
                Some(right) => Some(Score::Hamming(hamming(left, &right, meter)?)),
                None => None,
            },
            None => None,
        },
        Metric::Jaccard => match &query.bits {
            Some(left) => match bits(record, meter)? {
                Some(right) => jaccard(left, &right, meter)?
                    .map(|(inter, union)| Score::Jaccard { inter, union }),
                None => None,
            },
            None => None,
        },
    })
}

/// `a·b` for two exact numbers, reserved first.
fn product(a: &Exact, b: &Exact, meter: &ScanMeter) -> Result<Exact, NumError> {
    costs::reserve_rational(a, b, meter)?;
    Ok(a * b)
}

/// The exact comparison of two rationals, reserved first.
fn compare(a: &Exact, b: &Exact, meter: &ScanMeter) -> Result<Ordering, NumError> {
    costs::reserve_rational_compare(a, b, meter)?;
    Ok(a.cmp(b))
}

/// The order of two scores of one query: `Greater` when `a` is better than `b`, `Equal` when
/// they tie. Scores of two metrics never meet, and such a pair is `None` for the caller.
pub fn better(a: &Score, b: &Score, meter: &ScanMeter) -> Result<Option<Ordering>, NumError> {
    Ok(Some(match (a, b) {
        (
            Score::Cosine { dot: u, norm2: n },
            Score::Cosine {
                dot: v,
                norm2: n_prime,
            },
        ) => match (u.is_negative(), v.is_negative()) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (negative, _) => {
                let left = product(&product(u, u, meter)?, n_prime, meter)?;
                let right = product(&product(v, v, meter)?, n, meter)?;
                let order = compare(&left, &right, meter)?;
                match negative {
                    false => order,
                    true => order.reverse(),
                }
            }
        },
        (Score::Dot(u), Score::Dot(v)) => compare(u, v, meter)?,
        (Score::Euclidean(u), Score::Euclidean(v)) | (Score::Manhattan(u), Score::Manhattan(v)) => {
            compare(u, v, meter)?.reverse()
        }
        (Score::Hamming(u), Score::Hamming(v)) => {
            meter.compare(size_of::<u64>(), size_of::<u64>())?;
            u.cmp(v).reverse()
        }
        (
            Score::Jaccard { inter, union },
            Score::Jaccard {
                inter: inter_prime,
                union: union_prime,
            },
        ) => {
            // inter/union against inter'/union': two products and one comparison.
            meter.charge(3, 4 * size_of::<u64>(), 0)?;
            (u128::from(*inter) * u128::from(*union_prime))
                .cmp(&(u128::from(*inter_prime) * u128::from(*union)))
        }
        _ => return Ok(None),
    }))
}

/// True when `score` passes `bound` for `query`. A threshold applies to a similarity and a
/// radius to a distance. Any other pair is `None`, and the caller rejects it before it scores.
pub fn within(
    score: &Score,
    bound: &Bound,
    query: &Query,
    meter: &ScanMeter,
) -> Result<Option<bool>, NumError> {
    Ok(Some(match (score, bound) {
        (Score::Cosine { dot, norm2 }, Bound::Threshold(t)) => {
            // cos = q·e / (‖q‖·‖e‖) ≥ t: by the signs, and otherwise by the squares.
            match (dot.is_negative(), t.is_negative()) {
                (false, true) => true,
                (true, false) => false,
                (negative, _) => {
                    let left = product(dot, dot, meter)?;
                    let scale = product(&query.norm2, norm2, meter)?;
                    let right = product(&product(t, t, meter)?, &scale, meter)?;
                    let order = compare(&left, &right, meter)?;
                    match negative {
                        // Both non-negative: cos ≥ t when cos² ≥ t².
                        false => order != Ordering::Less,
                        // Both negative: cos ≥ t when cos² ≤ t².
                        true => order != Ordering::Greater,
                    }
                }
            }
        }
        (Score::Dot(value), Bound::Threshold(t)) => compare(value, t, meter)? != Ordering::Less,
        (Score::Jaccard { inter, union }, Bound::Threshold(t)) => {
            let left = Exact::from_integer(BigInt::from(*inter));
            let right = product(t, &Exact::from_integer(BigInt::from(*union)), meter)?;
            compare(&left, &right, meter)? != Ordering::Less
        }
        (Score::Euclidean(distance2), Bound::Radius(radius)) => match radius.is_negative() {
            // A distance is never negative, and ρ² would lose the sign of ρ.
            true => false,
            false => {
                let limit = product(radius, radius, meter)?;
                compare(distance2, &limit, meter)? != Ordering::Greater
            }
        },
        (Score::Manhattan(distance), Bound::Radius(radius)) => {
            compare(distance, radius, meter)? != Ordering::Greater
        }
        (Score::Hamming(count), Bound::Radius(radius)) => {
            let count = Exact::from_integer(BigInt::from(*count));
            compare(&count, radius, meter)? != Ordering::Greater
        }
        _ => return Ok(None),
    }))
}

/// The delivered value of `score` for `query`: `RN_53` of the exact value of the metric, or
/// `ValueOutOfRange` beyond the finite range of `f64`.
pub fn delivered(score: &Score, query: &Query, meter: &ScanMeter) -> Result<f64, NumError> {
    match score {
        Score::Cosine { dot, norm2 } => {
            // |cos| = √((q·e)² / (‖q‖²‖e‖²)), and RN_53 commutes with the sign.
            let numerator = product(dot, dot, meter)?;
            let denominator = product(&query.norm2, norm2, meter)?;
            costs::reserve_rational(&numerator, &denominator, meter)?;
            let magnitude = sqrt_to_f64(&(numerator / denominator), meter)?;
            Ok(match dot.is_negative() {
                true => -magnitude,
                false => magnitude,
            })
        }
        Score::Dot(value) | Score::Manhattan(value) => round_to_f64(value, meter),
        Score::Euclidean(distance2) => sqrt_to_f64(distance2, meter),
        Score::Hamming(count) => round_to_f64(&Exact::from_integer(BigInt::from(*count)), meter),
        Score::Jaccard { inter, union } => round_to_f64(
            &Exact::new(BigInt::from(*inter), BigInt::from(*union)),
            meter,
        ),
    }
}

/// A candidate of a selection: its score and its rank in the canonical order of DR-75
/// (decision 41: the hash of the source produce, then the full digest, then the index in the
/// view), which `Cx::canonical_data_order` gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// The exact score of the record for the query.
    pub score: Score,
    /// The rank of the record in the canonical order of its collection.
    pub rank: usize,
}

/// The selection order of design v3, section 4.4 (decisions 41 and 60): `Less` when `a` comes
/// before `b`. The better score comes first, and the lower rank breaks a tie, so the order is
/// total on candidates of distinct ranks (Similarity.v, S13). Candidates of two metrics never
/// meet (`MixedScores`).
pub fn selection_order(
    a: &Candidate,
    b: &Candidate,
    meter: &ScanMeter,
) -> Result<Ordering, NumError> {
    match better(&a.score, &b.score, meter)?.ok_or(NumError::MixedScores)? {
        Ordering::Greater => Ok(Ordering::Less),
        Ordering::Less => Ok(Ordering::Greater),
        Ordering::Equal => {
            meter.compare(size_of::<usize>(), size_of::<usize>())?;
            Ok(a.rank.cmp(&b.rank))
        }
    }
}

/// Sorts `candidates` in the selection order with the fallible heap sort of the substrate's
/// space order (`SpaceOrder::sort` of space_store_handle.rs). The moves of the sort and each
/// comparison are reserved before they are made. On distinct ranks the result is the one
/// sorted arrangement of the candidates, whatever their input order (Similarity.v, S14).
pub fn rank(candidates: &mut [Candidate], meter: &ScanMeter) -> Result<(), NumError> {
    let moves = amount(size_of::<Candidate>(), 2)?;
    meter.charge(candidates.len(), amount(candidates.len(), moves)?, 0)?;
    shared::rust::fallible_sort::sort(candidates, |a, b| {
        meter.charge(1, moves, 0)?;
        selection_order(a, b, meter)
    })
}

#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EList, Expr, GBigRational, Par};
    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
    use proptest::prelude::*;

    use super::super::num::vector_of;
    use super::*;
    use crate::rust::interpreter::host_work::HostWorkBudget;

    fn unmetered() -> ScanMeter { ScanMeter::new(None) }

    fn expr(instance: ExprInstance) -> Par {
        Par {
            exprs: vec![Expr {
                expr_instance: Some(instance),
            }],
            ..Par::default()
        }
    }

    fn list(ps: Vec<Par>) -> Par {
        expr(ExprInstance::EListBody(EList {
            ps,
            ..EList::default()
        }))
    }

    fn ints(values: &[i64]) -> ExactVector {
        let ps = values
            .iter()
            .map(|value| expr(ExprInstance::GInt(*value)))
            .collect();
        vector_of(&list(ps), None, &unmetered()).expect("an integer vector")
    }

    fn doubles(values: &[f64]) -> ExactVector {
        let ps = values
            .iter()
            .map(|value| expr(ExprInstance::GDouble(value.to_bits())))
            .collect();
        vector_of(&list(ps), None, &unmetered()).expect("a float vector")
    }

    /// The same values as rationals, so that a kernel takes its rational path.
    fn rational(vector: &ExactVector) -> ExactVector {
        ExactVector::Rational(vector.rationals(&unmetered()).expect("values").into_owned())
    }

    fn ratio(numerator: i64, denominator: i64) -> Exact {
        Exact::new(BigInt::from(numerator), BigInt::from(denominator))
    }

    fn query(vector: ExactVector, metric: Metric) -> Query {
        Query::new(vector, metric, &unmetered())
            .expect("an unmetered query")
            .expect("a query with a direction")
    }

    fn score_of(query: &Query, record: &ExactVector) -> Option<Score> {
        score(query, record, &unmetered()).expect("an unmetered score")
    }

    fn sole(score: Option<Score>) -> Score { score.expect("a defined score") }

    fn better_of(a: &Score, b: &Score) -> Ordering {
        better(a, b, &unmetered())
            .expect("an unmetered comparison")
            .expect("scores of one metric")
    }

    fn within_of(score: &Score, bound: Bound, query: &Query) -> bool {
        within(score, &bound, query, &unmetered())
            .expect("an unmetered bound")
            .expect("a bound of the metric")
    }

    #[test]
    fn tensor_metric_names_round_trip() {
        for metric in Metric::ALL {
            assert_eq!(Metric::parse(metric.name()), Some(metric));
        }
        assert_eq!(Metric::parse("Cosine"), None);
        assert_eq!(Metric::parse("l2"), None);
        let similarities: Vec<Metric> = Metric::ALL
            .into_iter()
            .filter(Metric::is_similarity)
            .collect();
        assert_eq!(similarities, vec![
            Metric::Cosine,
            Metric::Dot,
            Metric::Jaccard
        ]);
    }

    #[test]
    fn tensor_exact_dot_differs_from_f64_accumulation() {
        // The f64-accumulation mutant: 1e16 + 1 rounds to 1e16 in f64, so the float sum is 0.
        let q = doubles(&[1e16, 1.0, -1e16]);
        let e = ints(&[1, 1, 1]);
        let exact = dot(&q, &e, &unmetered()).expect("a dot product");
        assert_eq!(exact, ratio(1, 1));
        let float: f64 = [1e16f64, 1.0, -1e16].iter().sum();
        assert_eq!(float, 0.0);
        let query = query(q, Metric::Dot);
        let score = sole(score_of(&query, &e));
        assert!(within_of(&score, Bound::Threshold(ratio(1, 2)), &query));
    }

    #[test]
    fn tensor_cosine_order_follows_the_three_case_rule() {
        let q = query(ints(&[1, 0]), Metric::Cosine);
        let same = sole(score_of(&q, &ints(&[5, 0])));
        let diagonal = sole(score_of(&q, &ints(&[1, 1])));
        let orthogonal = sole(score_of(&q, &ints(&[0, 3])));
        let back_diagonal = sole(score_of(&q, &ints(&[-1, 1])));
        let opposite = sole(score_of(&q, &ints(&[-2, 0])));
        let ranked = [&same, &diagonal, &orthogonal, &back_diagonal, &opposite];
        for (index, left) in ranked.iter().enumerate() {
            for (other, right) in ranked.iter().enumerate() {
                assert_eq!(
                    better_of(left, right),
                    other.cmp(&index),
                    "{index} against {other}"
                );
            }
        }
        // The length of a record does not change its cosine.
        assert_eq!(
            better_of(&diagonal, &sole(score_of(&q, &ints(&[7, 7])))),
            Ordering::Equal
        );
    }

    #[test]
    fn tensor_distances_prefer_the_nearer_record() {
        for metric in [Metric::Euclidean, Metric::Manhattan] {
            let q = query(ints(&[0, 0]), metric);
            let near = sole(score_of(&q, &ints(&[1, 1])));
            let far = sole(score_of(&q, &ints(&[3, 0])));
            assert_eq!(better_of(&near, &far), Ordering::Greater, "{metric:?}");
            assert_eq!(better_of(&far, &near), Ordering::Less, "{metric:?}");
        }
        // Euclidean ranks [1, 1] (δ² = 2) before [2, 0] (δ² = 4); manhattan ties them (2 = 2).
        let euclidean = query(ints(&[0, 0]), Metric::Euclidean);
        let manhattan = query(ints(&[0, 0]), Metric::Manhattan);
        let (a, b) = (ints(&[1, 1]), ints(&[2, 0]));
        assert_eq!(
            better_of(
                &sole(score_of(&euclidean, &a)),
                &sole(score_of(&euclidean, &b))
            ),
            Ordering::Greater
        );
        assert_eq!(
            better_of(
                &sole(score_of(&manhattan, &a)),
                &sole(score_of(&manhattan, &b))
            ),
            Ordering::Equal
        );
    }

    #[test]
    fn tensor_bounds_are_decided_exactly() {
        // cos([3, 4], [4, 3]) = 24/25 exactly.
        let q = query(ints(&[3, 4]), Metric::Cosine);
        let close = sole(score_of(&q, &ints(&[4, 3])));
        assert!(within_of(&close, Bound::Threshold(ratio(24, 25)), &q));
        assert!(!within_of(
            &close,
            Bound::Threshold(ratio(24, 25) + ratio(1, 10_000)),
            &q
        ));
        assert!(within_of(&close, Bound::Threshold(Exact::zero()), &q));
        let negative = sole(score_of(&q, &ints(&[-4, -3])));
        assert!(!within_of(&negative, Bound::Threshold(Exact::zero()), &q));
        assert!(within_of(&negative, Bound::Threshold(ratio(-24, 25)), &q));
        assert!(!within_of(&negative, Bound::Threshold(ratio(-23, 25)), &q));
        assert!(within_of(&close, Bound::Threshold(ratio(-1, 2)), &q));
        // δ([0, 0], [3, 4]) = 5 exactly.
        let origin = query(ints(&[0, 0]), Metric::Euclidean);
        let five = sole(score_of(&origin, &ints(&[3, 4])));
        assert!(within_of(&five, Bound::Radius(ratio(5, 1)), &origin));
        assert!(!within_of(
            &five,
            Bound::Radius(ratio(4_999, 1_000)),
            &origin
        ));
        assert!(!within_of(&five, Bound::Radius(ratio(-5, 1)), &origin));
        let l1 = query(ints(&[0, 0]), Metric::Manhattan);
        let seven = sole(score_of(&l1, &ints(&[3, -4])));
        assert!(within_of(&seven, Bound::Radius(ratio(7, 1)), &l1));
        assert!(!within_of(&seven, Bound::Radius(ratio(69, 10)), &l1));
        let dot_query = query(ints(&[1, 2]), Metric::Dot);
        let eleven = sole(score_of(&dot_query, &ints(&[3, 4])));
        assert!(within_of(
            &eleven,
            Bound::Threshold(ratio(11, 1)),
            &dot_query
        ));
        assert!(!within_of(
            &eleven,
            Bound::Threshold(ratio(111, 10)),
            &dot_query
        ));
        let bits_query = query(ints(&[1, 1, 0, 0]), Metric::Jaccard);
        let third = sole(score_of(&bits_query, &ints(&[1, 0, 1, 0])));
        assert_eq!(third, Score::Jaccard { inter: 1, union: 3 });
        assert!(within_of(
            &third,
            Bound::Threshold(ratio(1, 3)),
            &bits_query
        ));
        assert!(!within_of(
            &third,
            Bound::Threshold(ratio(1, 2)),
            &bits_query
        ));
        let hamming_query = query(ints(&[1, 1, 0, 0]), Metric::Hamming);
        let two = sole(score_of(&hamming_query, &ints(&[1, 0, 1, 0])));
        assert_eq!(two, Score::Hamming(2));
        assert!(within_of(&two, Bound::Radius(ratio(2, 1)), &hamming_query));
        assert!(!within_of(&two, Bound::Radius(ratio(3, 2)), &hamming_query));
        // A threshold never bounds a distance, and a radius never bounds a similarity.
        assert_eq!(
            within(&five, &Bound::Threshold(ratio(1, 2)), &origin, &unmetered()),
            Ok(None)
        );
        assert_eq!(
            within(&close, &Bound::Radius(ratio(1, 2)), &q, &unmetered()),
            Ok(None)
        );
    }

    #[test]
    fn tensor_undefined_scores_make_records_ineligible() {
        assert!(Query::new(ints(&[0, 0]), Metric::Cosine, &unmetered())
            .expect("an unmetered query")
            .is_none());
        let q = query(ints(&[1, 2]), Metric::Cosine);
        assert_eq!(score_of(&q, &ints(&[0, 0])), None);
        let hamming_query = query(ints(&[1, 0]), Metric::Hamming);
        assert_eq!(score_of(&hamming_query, &ints(&[2, 0])), None);
        assert_eq!(score_of(&hamming_query, &doubles(&[0.5, 0.0])), None);
        // A query that is not binary makes every record ineligible.
        let not_binary = query(ints(&[1, 3]), Metric::Hamming);
        assert_eq!(not_binary.bits, None);
        assert_eq!(score_of(&not_binary, &ints(&[1, 0])), None);
        let jaccard_query = query(ints(&[0, 0]), Metric::Jaccard);
        assert_eq!(score_of(&jaccard_query, &ints(&[0, 0])), None);
        assert_eq!(
            score_of(&jaccard_query, &ints(&[0, 1])),
            Some(Score::Jaccard { inter: 0, union: 1 })
        );
        // Floats and Booleans that are 0 and 1 are binary.
        let booleans = vector_of(
            &list(vec![
                expr(ExprInstance::GBool(true)),
                expr(ExprInstance::GBool(false)),
            ]),
            None,
            &unmetered(),
        )
        .expect("a Boolean vector");
        assert_eq!(score_of(&hamming_query, &booleans), Some(Score::Hamming(0)));
        assert_eq!(
            score_of(&hamming_query, &doubles(&[1.0, 1.0])),
            Some(Score::Hamming(1))
        );
        // A rational vector with a third is not binary; one with only 0 and 1 is.
        let third = ExactVector::Rational(vec![ratio(1, 3), ratio(0, 1)]);
        assert_eq!(score_of(&hamming_query, &third), None);
        let unit = ExactVector::Rational(vec![ratio(1, 1), ratio(0, 1)]);
        assert_eq!(score_of(&hamming_query, &unit), Some(Score::Hamming(0)));
    }

    #[test]
    fn tensor_scores_of_two_metrics_never_compare() {
        let dot_score = Score::Dot(ratio(1, 1));
        let distance = Score::Euclidean(ratio(1, 1));
        assert_eq!(better(&dot_score, &distance, &unmetered()), Ok(None));
    }

    #[test]
    fn tensor_delivered_values_are_correctly_rounded() {
        let q = query(ints(&[3, 4]), Metric::Cosine);
        let close = sole(score_of(&q, &ints(&[4, 3])));
        assert_eq!(delivered(&close, &q, &unmetered()), Ok(0.96));
        let opposite = sole(score_of(&q, &ints(&[-4, -3])));
        assert_eq!(delivered(&opposite, &q, &unmetered()), Ok(-0.96));
        let axis = query(ints(&[1, 0]), Metric::Cosine);
        let tilted = sole(score_of(&axis, &ints(&[1, 1])));
        assert_eq!(
            delivered(&tilted, &axis, &unmetered()),
            Ok(std::f64::consts::FRAC_1_SQRT_2)
        );
        let origin = query(ints(&[0, 0]), Metric::Euclidean);
        let root_two = sole(score_of(&origin, &ints(&[1, 1])));
        assert_eq!(
            delivered(&root_two, &origin, &unmetered()),
            Ok(std::f64::consts::SQRT_2)
        );
        let jaccard_query = query(ints(&[1, 1, 0]), Metric::Jaccard);
        let third = sole(score_of(&jaccard_query, &ints(&[1, 0, 1])));
        assert_eq!(
            delivered(&third, &jaccard_query, &unmetered()),
            Ok(1.0 / 3.0)
        );
        let hamming_query = query(ints(&[1, 1, 0]), Metric::Hamming);
        let two = sole(score_of(&hamming_query, &ints(&[1, 0, 1])));
        assert_eq!(delivered(&two, &hamming_query, &unmetered()), Ok(2.0));
        let dot_query = query(doubles(&[1e308]), Metric::Dot);
        let huge = sole(score_of(&dot_query, &ints(&[10])));
        assert_eq!(
            delivered(&huge, &dot_query, &unmetered()),
            Err(NumError::ValueOutOfRange)
        );
        let fraction = ExactVector::Rational(vec![ratio(1, 3)]);
        let l1 = query(ints(&[0]), Metric::Manhattan);
        let distance = sole(score_of(&l1, &fraction));
        assert_eq!(delivered(&distance, &l1, &unmetered()), Ok(1.0 / 3.0));
    }

    #[test]
    fn tensor_kernels_reserve_their_work_first() {
        let unbounded =
            || HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        let operations = HostWorkDimension::VerificationOperations;
        let two_sevenths = expr(ExprInstance::GBigRat(GBigRational {
            numerator: BigInt::from(2).to_signed_bytes_be(),
            denominator: BigInt::from(7).to_signed_bytes_be(),
        }));
        let record = list(vec![
            two_sevenths,
            expr(ExprInstance::GDouble(0.25f64.to_bits())),
        ]);
        let run = |meter: &ScanMeter| -> Result<Vec<f64>, NumError> {
            let mut values = Vec::new();
            for metric in Metric::ALL {
                let q = Query::new(doubles(&[1.5, -2.0]), metric, meter)?
                    .expect("a query with a direction");
                let record = vector_of(&record, Some(2), meter)?;
                let dyadic = doubles(&[0.5, 1.0]);
                for candidate in [&record, &dyadic] {
                    if let Some(score) = score(&q, candidate, meter)? {
                        values.push(delivered(&score, &q, meter)?);
                    }
                }
            }
            Ok(values)
        };
        let budget = unbounded();
        let expected = run(&ScanMeter::new(Some(budget.clone()))).expect("an unbounded run");
        assert_eq!(run(&unmetered()), Ok(expected));
        let usage = budget.usage(operations).get();
        assert!(usage > 0, "the kernels reserve their work");
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        limits.set(operations, HostWorkLimit::new(usage - 1));
        let cut = ScanMeter::new(Some(HostWorkBudget::new(limits)));
        assert!(matches!(run(&cut), Err(NumError::HostWork(_))));
    }

    /// The bits of a delivered value, as the reference vectors write them.
    fn delivered_text(value: Result<f64, NumError>) -> String {
        match value {
            Ok(value) => format!("0x{:016x}", value.to_bits()),
            Err(NumError::ValueOutOfRange) => "ValueOutOfRange".to_string(),
            Err(error) => format!("{error:?}"),
        }
    }

    /// The `Par` of a reference element: an integer, a rational `p/q`, `f64:<bits>` or
    /// `bool:<b>`.
    fn reference_element(text: &str) -> Par {
        if let Some(bits) = text.strip_prefix("f64:") {
            let bits = u64::from_str_radix(bits, 16).expect("hex bits of an f64");
            return expr(ExprInstance::GDouble(bits));
        }
        if let Some(value) = text.strip_prefix("bool:") {
            return expr(ExprInstance::GBool(value == "true"));
        }
        let value: Exact = text.parse().expect("a rational");
        match (value.is_integer(), i64::try_from(value.numer())) {
            (true, Ok(integer)) => expr(ExprInstance::GInt(integer)),
            _ => expr(ExprInstance::GBigRat(GBigRational {
                numerator: value.numer().to_signed_bytes_be(),
                denominator: value.denom().to_signed_bytes_be(),
            })),
        }
    }

    fn reference_vector(elements: &serde_json::Value) -> Par {
        let elements = elements.as_array().expect("a list of elements");
        list(
            elements
                .iter()
                .map(|element| reference_element(element.as_str().expect("an element")))
                .collect(),
        )
    }

    /// N8: the Rust core agrees with every case of the Sage reference vectors
    /// (test-vectors/numeric-reference-v1.json), which the pure-Python receipt also
    /// checks: RN_53 of rationals and of square roots, and the outcome of each metric.
    #[test]
    fn tensor_numeric_reference_vectors_agree() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-vectors/numeric-reference-v1.json"
        );
        let text = std::fs::read_to_string(path).expect("the reference vectors");
        let document: serde_json::Value = serde_json::from_str(&text).expect("JSON");
        assert_eq!(
            document["schema"],
            serde_json::json!("reified-rspaces-numeric-reference/1")
        );
        let meter = unmetered();
        let mut checked = 0usize;
        let mut failures = Vec::new();
        for case in document["rounding_cases"]
            .as_array()
            .expect("rounding cases")
        {
            let x: Exact = case["x"].as_str().expect("x").parse().expect("a rational");
            let got = delivered_text(round_to_f64(&x, &meter));
            if got != case["rn53"].as_str().expect("rn53") {
                failures.push(format!("rounding {}: {got}", case["x"]));
            }
            checked += 1;
        }
        for case in document["sqrt_cases"]
            .as_array()
            .expect("square root cases")
        {
            let x: Exact = case["x"].as_str().expect("x").parse().expect("a rational");
            let got = delivered_text(sqrt_to_f64(&x, &meter));
            if got != case["rn53"].as_str().expect("rn53") {
                failures.push(format!("square root {}: {got}", case["x"]));
            }
            checked += 1;
        }
        for case in document["metric_cases"].as_array().expect("metric cases") {
            let name = case["name"].as_str().expect("a name");
            let metric = Metric::parse(case["metric"].as_str().expect("a metric"))
                .expect("a metric of the contract");
            let q = reference_vector(&case["query"]);
            let e = reference_vector(&case["record"]);
            let dimension = case["query"].as_array().expect("a query").len();
            let q = vector_of(&q, Some(dimension), &meter).expect("a query vector");
            let e = vector_of(&e, Some(dimension), &meter).expect("a record vector");
            let bound = match case["bound"].as_object() {
                Some(bound) => match (bound.get("threshold"), bound.get("radius")) {
                    (Some(t), None) => Some(Bound::Threshold(
                        t.as_str().expect("t").parse().expect("a rational"),
                    )),
                    (None, Some(r)) => Some(Bound::Radius(
                        r.as_str().expect("r").parse().expect("a rational"),
                    )),
                    _ => panic!("{name}: a bound has one key"),
                },
                None => None,
            };
            let outcome = match Query::new(q, metric, &meter).expect("an unmetered query") {
                None => serde_json::json!({
                    "query_refused": true, "score_defined": false,
                    "delivered": null, "within": null,
                }),
                Some(query) => match score(&query, &e, &meter).expect("an unmetered score") {
                    None => serde_json::json!({
                        "query_refused": false, "score_defined": false,
                        "delivered": null, "within": null,
                    }),
                    Some(score) => {
                        let within = bound.as_ref().map(|bound| {
                            within(&score, bound, &query, &meter)
                                .expect("an unmetered bound")
                                .expect("a bound of the metric")
                        });
                        serde_json::json!({
                            "query_refused": false, "score_defined": true,
                            "delivered": delivered_text(delivered(&score, &query, &meter)),
                            "within": within,
                        })
                    }
                },
            };
            for key in ["query_refused", "score_defined", "delivered", "within"] {
                if outcome[key] != case[key] {
                    failures.push(format!(
                        "{name} {key}: {} against {}",
                        outcome[key], case[key]
                    ));
                }
            }
            checked += 1;
        }
        assert!(
            failures.is_empty(),
            "{} disagreements: {failures:#?}",
            failures.len()
        );
        assert_eq!(checked, 24 + 15 + 143, "every case of the file is checked");
    }

    /// The candidates of one query over integer records, ranked by their index; undefined
    /// scores are left out.
    fn candidates_of(query: &Query, records: &[Vec<i64>]) -> Vec<Candidate> {
        records
            .iter()
            .enumerate()
            .filter_map(|(rank, record)| {
                score_of(query, &ints(record)).map(|score| Candidate { score, rank })
            })
            .collect()
    }

    fn ranked(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
        rank(&mut candidates, &unmetered()).expect("an unmetered sort");
        candidates
    }

    /// S16, the negative control of the kappa tie-break: two records with one score. By
    /// score alone, the heap sort of the space order keeps the first record of one input
    /// order and the other record of the reverse order, so two nodes that read the view in
    /// different orders could select different records. The rank decides.
    #[test]
    fn tensor_selection_without_ranks_is_not_unique() {
        let q = query(ints(&[1, 0]), Metric::Dot);
        let first = Candidate {
            score: sole(score_of(&q, &ints(&[2, 5]))),
            rank: 0,
        };
        let second = Candidate {
            score: sole(score_of(&q, &ints(&[2, -7]))),
            rank: 1,
        };
        assert_eq!(better_of(&first.score, &second.score), Ordering::Equal);
        let by_score = |mut list: Vec<Candidate>| {
            shared::rust::fallible_sort::sort(&mut list, |a, b| {
                better(&b.score, &a.score, &unmetered()).map(|order| order.expect("one metric"))
            })
            .expect("an unmetered sort");
            list
        };
        let forward = vec![first.clone(), second.clone()];
        let backward = vec![second.clone(), first.clone()];
        assert_ne!(by_score(forward.clone())[0], by_score(backward.clone())[0]);
        assert_eq!(ranked(forward)[0], first);
        assert_eq!(ranked(backward)[0], first);
        assert_eq!(
            selection_order(
                &first,
                &Candidate {
                    score: Score::Euclidean(ratio(1, 1)),
                    rank: 2
                },
                &unmetered()
            ),
            Err(NumError::MixedScores)
        );
    }

    fn property_config() -> ProptestConfig {
        ProptestConfig {
            cases: 64,
            rng_seed: proptest::test_runner::RngSeed::Fixed(0x0F6E_5EED_2026_1008),
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    /// A finite `f64` of moderate size, so that every delivered value stays in range.
    fn moderate_f64() -> impl Strategy<Value = f64> {
        (-1_000_000i64..=1_000_000, -40i32..=40)
            .prop_map(|(mantissa, exponent)| mantissa as f64 * 2f64.powi(exponent))
    }

    /// A rational with a small numerator and a positive denominator, negative values and
    /// values above 1 included.
    fn small_ratio() -> impl Strategy<Value = Exact> {
        (-30i64..=30, 1i64..=12).prop_map(|(numerator, denominator)| ratio(numerator, denominator))
    }

    /// Integer vectors of one dimension.
    fn int_vectors(count: usize, low: i64, high: i64) -> impl Strategy<Value = Vec<Vec<i64>>> {
        (1usize..5).prop_flat_map(move |dimension| {
            proptest::collection::vec(proptest::collection::vec(low..=high, dimension), count)
        })
    }

    /// `sign(x)·x²` of the cosine of a score, as an exact rational: it keeps the order of the
    /// cosines (`ssq` and `cosine_le_ssq` of Similarity.v).
    fn signed_square_cosine(score: &Score, query: &Query) -> Exact {
        let Score::Cosine { dot, norm2 } = score else {
            panic!("a cosine score");
        };
        let square = dot * dot / (&query.norm2 * norm2);
        match dot.is_negative() {
            true => -square,
            false => square,
        }
    }

    /// `sign(t)·t²`.
    fn signed_square(t: &Exact) -> Exact {
        match t.is_negative() {
            true => -(t * t),
            false => t * t,
        }
    }

    fn at_least(order: Ordering) -> bool { order != Ordering::Less }

    proptest! {
        #![proptest_config(property_config())]

        /// S1 (cosine_squared_form_iff) and S18: `within` decides cos ≥ t exactly, as the
        /// order of the signed squares `sign(cos)·cos² ≥ sign(t)·t²`.
        #[test]
        fn prop_cosine_threshold_matches_the_squared_form(
            vectors in int_vectors(2, -9, 9),
            t in small_ratio(),
        ) {
            let Some(query) = Query::new(ints(&vectors[0]), Metric::Cosine, &unmetered())
                .expect("an unmetered query") else {
                return Ok(());
            };
            let Some(score) = score_of(&query, &ints(&vectors[1])) else {
                return Ok(());
            };
            let expected = signed_square_cosine(&score, &query) >= signed_square(&t);
            prop_assert_eq!(within_of(&score, Bound::Threshold(t), &query), expected);
        }

        /// S2 (cosine_order_squared_form) and S18: `better` orders two cosine scores as their
        /// exact cosines, and it is antisymmetric.
        #[test]
        fn prop_cosine_order_matches_the_squared_form(vectors in int_vectors(3, -20, 20)) {
            let Some(query) = Query::new(ints(&vectors[0]), Metric::Cosine, &unmetered())
                .expect("an unmetered query") else {
                return Ok(());
            };
            let (Some(a), Some(b)) =
                (score_of(&query, &ints(&vectors[1])), score_of(&query, &ints(&vectors[2])))
            else {
                return Ok(());
            };
            let expected =
                signed_square_cosine(&a, &query).cmp(&signed_square_cosine(&b, &query));
            prop_assert_eq!(better_of(&a, &b), expected);
            prop_assert_eq!(better_of(&b, &a), expected.reverse());
        }

        /// S3 (euclidean_radius_squared_form) and S18: a distance d ≥ 0 is within the radius r
        /// exactly when d ≤ r, although `within` compares d² with r².
        #[test]
        fn prop_euclidean_radius_matches_the_squared_form(
            numerator in 0i64..=30,
            denominator in 1i64..=12,
            r in small_ratio(),
            vectors in int_vectors(2, -9, 9),
        ) {
            let d = ratio(numerator, denominator);
            let query = query(ints(&vectors[0]), Metric::Euclidean);
            let score = Score::Euclidean(&d * &d);
            prop_assert_eq!(within_of(&score, Bound::Radius(r.clone()), &query), d <= r);
            let kernel = sole(score_of(&query, &ints(&vectors[1])));
            let Score::Euclidean(distance2) = &kernel else {
                panic!("a euclidean score");
            };
            let expected = !r.is_negative() && *distance2 <= &r * &r;
            prop_assert_eq!(within_of(&kernel, Bound::Radius(r), &query), expected);
        }

        /// S4 (threshold_monotone): a lower threshold admits every record that a higher one
        /// admits, under cosine, dot and jaccard.
        #[test]
        fn prop_thresholds_are_monotone(
            vectors in int_vectors(2, -9, 9),
            bits in int_vectors(2, 0, 1),
            first in small_ratio(),
            second in small_ratio(),
        ) {
            let (t1, t2) = match first <= second {
                true => (first, second),
                false => (second, first),
            };
            for (metric, pair) in [
                (Metric::Cosine, &vectors),
                (Metric::Dot, &vectors),
                (Metric::Jaccard, &bits),
            ] {
                let Some(query) = Query::new(ints(&pair[0]), metric, &unmetered())
                    .expect("an unmetered query") else {
                    continue;
                };
                let Some(score) = score_of(&query, &ints(&pair[1])) else {
                    continue;
                };
                if within_of(&score, Bound::Threshold(t2.clone()), &query) {
                    prop_assert!(within_of(&score, Bound::Threshold(t1.clone()), &query));
                }
            }
        }

        /// S5 (radius_monotone): a larger radius admits every record that a smaller one admits,
        /// under euclidean, manhattan and hamming.
        #[test]
        fn prop_radii_are_monotone(
            vectors in int_vectors(2, -9, 9),
            bits in int_vectors(2, 0, 1),
            first in small_ratio(),
            second in small_ratio(),
        ) {
            let (r1, r2) = match first <= second {
                true => (first, second),
                false => (second, first),
            };
            for (metric, pair) in [
                (Metric::Euclidean, &vectors),
                (Metric::Manhattan, &vectors),
                (Metric::Hamming, &bits),
            ] {
                let query = query(ints(&pair[0]), metric);
                let score = sole(score_of(&query, &ints(&pair[1])));
                if within_of(&score, Bound::Radius(r1.clone()), &query) {
                    prop_assert!(within_of(&score, Bound::Radius(r2.clone()), &query));
                }
            }
        }

        /// S6 (dot_laws): the dot product is symmetric, and an exact sum does not depend on
        /// the order of its terms, unlike an f64 sum.
        #[test]
        fn prop_exact_kernels_are_permutation_invariant(
            (pairs, order) in proptest::collection::vec((moderate_f64(), moderate_f64()), 1..9)
                .prop_flat_map(|pairs| {
                    let indices: Vec<usize> = (0..pairs.len()).collect();
                    (Just(pairs), Just(indices).prop_shuffle())
                }),
        ) {
            let (left, right): (Vec<f64>, Vec<f64>) = pairs.iter().copied().unzip();
            let (left_permuted, right_permuted): (Vec<f64>, Vec<f64>) =
                order.iter().map(|index| pairs[*index]).unzip();
            let (a, b) = (doubles(&left), doubles(&right));
            let (pa, pb) = (doubles(&left_permuted), doubles(&right_permuted));
            let meter = unmetered();
            prop_assert_eq!(dot(&a, &b, &meter), dot(&b, &a, &meter));
            prop_assert_eq!(dot(&a, &b, &meter), dot(&pa, &pb, &meter));
            prop_assert_eq!(distance2(&a, &b, &meter), distance2(&pa, &pb, &meter));
            prop_assert_eq!(manhattan(&a, &b, &meter), manhattan(&pa, &pb, &meter));
            let (ra, rb) = (rational(&pa), rational(&pb));
            prop_assert_eq!(dot(&a, &b, &meter), dot(&ra, &rb, &meter));
        }

        /// S7 (distance_laws): dist2 is non-negative, symmetric and zero exactly on equal
        /// vectors; manhattan is non-negative, symmetric and satisfies the triangle inequality.
        #[test]
        fn prop_distance_laws_hold(vectors in int_vectors(3, -9, 9), scale in 0i32..=3) {
            let factor = 2f64.powi(-scale);
            let a = doubles(&vectors[0].iter().map(|x| *x as f64 * factor).collect::<Vec<_>>());
            let (b, c) = (ints(&vectors[1]), ints(&vectors[2]));
            let meter = unmetered();
            let ab = distance2(&a, &b, &meter).expect("a distance");
            prop_assert!(!ab.is_negative());
            prop_assert_eq!(Ok(ab.clone()), distance2(&b, &a, &meter));
            prop_assert_eq!(distance2(&a, &a, &meter), Ok(Exact::zero()));
            let values = |v: &ExactVector| v.rationals(&meter).expect("values").into_owned();
            prop_assert_eq!(ab.is_zero(), values(&a) == values(&b));
            let (l_ab, l_bc, l_ac) = (
                manhattan(&a, &b, &meter).expect("a distance"),
                manhattan(&b, &c, &meter).expect("a distance"),
                manhattan(&a, &c, &meter).expect("a distance"),
            );
            prop_assert!(!l_ab.is_negative());
            prop_assert_eq!(Ok(l_ab.clone()), manhattan(&b, &a, &meter));
            prop_assert!(l_ac <= &l_ab + &l_bc);
        }

        /// S8 (cauchy_schwarz) and S18: (a·b)² ≤ ‖a‖²·‖b‖², exactly.
        #[test]
        fn prop_cauchy_schwarz_holds(vectors in int_vectors(2, -50, 50), scale in 0i32..=4) {
            let factor = 2f64.powi(-scale);
            let a = doubles(&vectors[0].iter().map(|x| *x as f64 * factor).collect::<Vec<_>>());
            let b = ints(&vectors[1]);
            let meter = unmetered();
            let ab = dot(&a, &b, &meter).expect("a dot product");
            let aa = dot(&a, &a, &meter).expect("a dot product");
            let bb = dot(&b, &b, &meter).expect("a dot product");
            prop_assert!(&ab * &ab <= aa * bb);
        }

        /// S9 (cosine_bounds) and S18: every record passes the threshold −1, none passes a
        /// threshold above 1, and a delivered cosine lies in [−1, 1].
        #[test]
        fn prop_cosine_bounds_hold(vectors in int_vectors(2, -50, 50), excess in 1i64..=100) {
            let Some(query) = Query::new(ints(&vectors[0]), Metric::Cosine, &unmetered())
                .expect("an unmetered query") else {
                return Ok(());
            };
            let Some(score) = score_of(&query, &ints(&vectors[1])) else {
                return Ok(());
            };
            prop_assert!(within_of(&score, Bound::Threshold(ratio(-1, 1)), &query));
            let above = ratio(1, 1) + ratio(1, excess);
            prop_assert!(!within_of(&score, Bound::Threshold(above), &query));
            let value = delivered(&score, &query, &unmetered()).expect("a value in range");
            prop_assert!((-1.0..=1.0).contains(&value));
        }

        /// S10 (score_orders_total): for every metric, `better` is a total preorder on the
        /// defined scores of one query: reflexive, antisymmetric in its result, and transitive.
        #[test]
        fn prop_score_orders_are_total_preorders(
            metric in proptest::sample::select(Metric::ALL.to_vec()),
            vectors in int_vectors(4, -6, 6),
            bits in int_vectors(4, 0, 1),
        ) {
            let pool = match metric {
                Metric::Hamming | Metric::Jaccard => &bits,
                Metric::Cosine | Metric::Dot | Metric::Euclidean | Metric::Manhattan => &vectors,
            };
            let Some(query) = Query::new(ints(&pool[0]), metric, &unmetered())
                .expect("an unmetered query") else {
                return Ok(());
            };
            let scores: Vec<Score> =
                pool[1..].iter().filter_map(|record| score_of(&query, &ints(record))).collect();
            for a in &scores {
                prop_assert_eq!(better_of(a, a), Ordering::Equal);
                for b in &scores {
                    prop_assert_eq!(better_of(a, b), better_of(b, a).reverse());
                    for c in &scores {
                        if at_least(better_of(a, b)) && at_least(better_of(b, c)) {
                            prop_assert!(at_least(better_of(a, c)));
                        }
                    }
                }
            }
        }

        /// S11 (squared_forms_embed) and S19: the decisions do not depend on the number kind
        /// that carries a value. Floats and integers take the dyadic path, and the same values
        /// as rationals take the rational path, with equal exact results.
        #[test]
        fn prop_dyadic_and_rational_kernels_agree(
            pairs in proptest::collection::vec((moderate_f64(), moderate_f64()), 1..9),
            vectors in int_vectors(2, -9, 9),
            metric in proptest::sample::select(Metric::ALL.to_vec()),
        ) {
            let (left, right): (Vec<f64>, Vec<f64>) = pairs.into_iter().unzip();
            let (a, b) = (doubles(&left), doubles(&right));
            let (ra, rb) = (rational(&a), rational(&b));
            let meter = unmetered();
            prop_assert_eq!(dot(&a, &b, &meter), dot(&ra, &rb, &meter));
            prop_assert_eq!(dot(&a, &b, &meter), dot(&a, &rb, &meter));
            prop_assert_eq!(distance2(&a, &b, &meter), distance2(&ra, &rb, &meter));
            prop_assert_eq!(manhattan(&a, &b, &meter), manhattan(&ra, &rb, &meter));
            let (q, e) = (ints(&vectors[0]), ints(&vectors[1]));
            let dyadic = Query::new(q.clone(), metric, &meter).expect("an unmetered query");
            let exact = Query::new(rational(&q), metric, &meter).expect("an unmetered query");
            if let (Some(dyadic), Some(exact)) = (dyadic, exact) {
                prop_assert_eq!(score_of(&dyadic, &e), score_of(&exact, &rational(&e)));
            }
        }

        /// S12 (binary_laws): hamming is symmetric, satisfies the triangle inequality and is
        /// zero only on equal vectors; the jaccard counts are symmetric with inter ≤ union, and
        /// the union is empty only when neither vector sets a position.
        #[test]
        fn prop_binary_metric_laws_hold(vectors in int_vectors(3, 0, 1)) {
            let meter = unmetered();
            let bit_vectors: Vec<Vec<bool>> = vectors
                .iter()
                .map(|vector| {
                    bits(&ints(vector), &meter)
                        .expect("an unmetered test")
                        .expect("a binary vector")
                })
                .collect();
            let (a, b, c) = (&bit_vectors[0], &bit_vectors[1], &bit_vectors[2]);
            let distance = |x: &[bool], y: &[bool]| hamming(x, y, &meter).expect("a distance");
            prop_assert_eq!(distance(a, b), distance(b, a));
            prop_assert!(distance(a, c) <= distance(a, b) + distance(b, c));
            prop_assert_eq!(distance(a, b) == 0, a == b);
            let counts = jaccard(a, b, &meter).expect("an unmetered test");
            prop_assert_eq!(counts, jaccard(b, a, &meter).expect("an unmetered test"));
            match counts {
                Some((inter, union)) => prop_assert!(inter <= union && union > 0),
                None => prop_assert!(a.iter().chain(b).all(|bit| !bit)),
            }
        }

        /// S17 (Qc_ordered_ring): the exact rationals of the Rust code satisfy the hypotheses
        /// that Similarity.v discharges for Qc.
        #[test]
        fn prop_rationals_form_an_ordered_ring(
            x in small_ratio(),
            y in small_ratio(),
            z in small_ratio(),
        ) {
            let zero = Exact::zero();
            let below = [x < y, x == y, y < x];
            prop_assert_eq!(below.iter().filter(|holds| **holds).count(), 1);
            prop_assert!(!(x < x));
            if x < y && y < z {
                prop_assert!(x < z);
            }
            if x < y {
                prop_assert!(&x + &z < &y + &z);
            }
            if zero < x && zero < y {
                prop_assert!(zero < &x * &y);
            }
            prop_assert!(zero < ratio(1, 1));
        }

        /// S13 (selection_total_order) and S20: the selection order is irreflexive,
        /// antisymmetric, transitive, and total on candidates of distinct ranks.
        #[test]
        fn prop_selection_order_is_a_strict_total_order(
            metric in proptest::sample::select(Metric::ALL.to_vec()),
            vectors in int_vectors(5, -4, 4),
            bits in int_vectors(5, 0, 1),
        ) {
            let pool = match metric {
                Metric::Hamming | Metric::Jaccard => &bits,
                Metric::Cosine | Metric::Dot | Metric::Euclidean | Metric::Manhattan => &vectors,
            };
            let Some(query) = Query::new(ints(&pool[0]), metric, &unmetered())
                .expect("an unmetered query") else {
                return Ok(());
            };
            let candidates = candidates_of(&query, &pool[1..]);
            let order = |a: &Candidate, b: &Candidate| {
                selection_order(a, b, &unmetered()).expect("one metric")
            };
            for a in &candidates {
                prop_assert_eq!(order(a, a), Ordering::Equal);
                for b in &candidates {
                    prop_assert_eq!(order(a, b), order(b, a).reverse());
                    if a.rank != b.rank {
                        prop_assert_ne!(order(a, b), Ordering::Equal);
                    }
                    for c in &candidates {
                        if order(a, b) == Ordering::Less && order(b, c) == Ordering::Less {
                            prop_assert_eq!(order(a, c), Ordering::Less);
                        }
                    }
                }
            }
        }

        /// S14 (topk_deterministic), S16 and S20: every input order of one set of candidates
        /// ranks to one sorted arrangement, so every top-k is the same.
        #[test]
        fn prop_topk_is_deterministic(
            metric in proptest::sample::select(Metric::ALL.to_vec()),
            vectors in int_vectors(6, -3, 3),
            bits in int_vectors(6, 0, 1),
            order in Just((0..5usize).collect::<Vec<_>>()).prop_shuffle(),
            k in 0usize..6,
        ) {
            let pool = match metric {
                Metric::Hamming | Metric::Jaccard => &bits,
                Metric::Cosine | Metric::Dot | Metric::Euclidean | Metric::Manhattan => &vectors,
            };
            let Some(query) = Query::new(ints(&pool[0]), metric, &unmetered())
                .expect("an unmetered query") else {
                return Ok(());
            };
            let candidates = candidates_of(&query, &pool[1..]);
            let permuted: Vec<Candidate> =
                order.iter().filter_map(|index| candidates.get(*index).cloned()).collect();
            let sorted = ranked(candidates);
            prop_assert_eq!(&sorted, &ranked(permuted));
            for pair in sorted.windows(2) {
                prop_assert_eq!(
                    selection_order(&pair[0], &pair[1], &unmetered()),
                    Ok(Ordering::Less)
                );
            }
            let top: Vec<&Candidate> = sorted.iter().take(k).collect();
            prop_assert_eq!(top.len(), k.min(sorted.len()));
        }

        /// S15 (list_selection_deterministic): the first k candidates that pass the bound are
        /// one list for every input order of the records.
        #[test]
        fn prop_list_selection_is_deterministic(
            metric in proptest::sample::select(Metric::ALL.to_vec()),
            vectors in int_vectors(6, -3, 3),
            bits in int_vectors(6, 0, 1),
            order in Just((0..5usize).collect::<Vec<_>>()).prop_shuffle(),
            bound in small_ratio(),
            k in 1usize..6,
        ) {
            let pool = match metric {
                Metric::Hamming | Metric::Jaccard => &bits,
                Metric::Cosine | Metric::Dot | Metric::Euclidean | Metric::Manhattan => &vectors,
            };
            let Some(query) = Query::new(ints(&pool[0]), metric, &unmetered())
                .expect("an unmetered query") else {
                return Ok(());
            };
            let bound = match metric.is_similarity() {
                true => Bound::Threshold(bound),
                false => Bound::Radius(bound),
            };
            let select = |candidates: Vec<Candidate>| {
                let eligible: Vec<Candidate> = candidates
                    .into_iter()
                    .filter(|candidate| within_of(&candidate.score, bound.clone(), &query))
                    .collect();
                ranked(eligible).into_iter().take(k).collect::<Vec<_>>()
            };
            let candidates = candidates_of(&query, &pool[1..]);
            let permuted: Vec<Candidate> =
                order.iter().filter_map(|index| candidates.get(*index).cloned()).collect();
            prop_assert_eq!(select(candidates), select(permuted));
        }

        /// The delivered cosine is RN_53 of the exact cosine. The f64 formula has a few
        /// rounding steps, so it agrees within a few units in the last place.
        #[test]
        fn delivered_cosine_is_near_the_f64_formula(vectors in int_vectors(2, -50, 50)) {
            let Some(query) = Query::new(ints(&vectors[0]), Metric::Cosine, &unmetered())
                .expect("an unmetered query") else {
                return Ok(());
            };
            let Some(score) = score_of(&query, &ints(&vectors[1])) else {
                return Ok(());
            };
            let value = delivered(&score, &query, &unmetered()).expect("a value in range");
            let (q, record) = (&vectors[0], &vectors[1]);
            let float_dot: f64 = q.iter().zip(record).map(|(x, y)| (*x * *y) as f64).sum();
            let float_q: f64 = q.iter().map(|x| (*x * *x) as f64).sum::<f64>().sqrt();
            let float_e: f64 = record.iter().map(|x| (*x * *x) as f64).sum::<f64>().sqrt();
            let approximate = float_dot / (float_q * float_e);
            prop_assert!((value - approximate).abs() <= 4.0 * f64::EPSILON);
        }
    }
}
