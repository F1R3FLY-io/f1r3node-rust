//! Validators of the Reified RSpaces syntax.
//!
//! The syntax layer applies them to literals, and the runtime layer applies them to evaluated
//! values. An accepted param key or theory name must be a name that the frontend can produce
//! (plan decision 27): a token of the grammar rule `var` that is not in the word list of its
//! position. Both lists were measured on parser `6548aec` over the keywords of grammar
//! `02cef80`.

use std::fmt;

use super::encoding::{is_identifier, sole_expr, ReifiedEncodeError};
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{GBigRational, GFixedPoint, Par};

/// The words that the parser does not accept as a param key: `Bool ByteArray Int Nil String
/// Uri true false` and the 18 other keywords of grammar `02cef80` that are not
/// `catch constructor default in method private`. Sorted bytewise.
pub const REJECTED_PARAM_KEYS: [&str; 26] = [
    "Bool",
    "ByteArray",
    "Int",
    "Nil",
    "Set",
    "String",
    "Uri",
    "agent",
    "and",
    "bundle",
    "bundle0",
    "contract",
    "else",
    "false",
    "for",
    "if",
    "let",
    "match",
    "matches",
    "new",
    "not",
    "or",
    "select",
    "true",
    "try",
    "where",
];

/// The words that the parser does not accept as a theory name: every keyword of grammar
/// `02cef80` except `Bool ByteArray Int Nil String Uri agent bundle0 catch constructor default
/// method private try`. Sorted bytewise.
pub const REJECTED_THEORY_NAMES: [&str; 18] = [
    "Set", "and", "bundle", "contract", "else", "false", "for", "if", "in", "let", "match",
    "matches", "new", "not", "or", "select", "true", "where",
];

/// The key of the priority of a send or a receive: an Int >= 0.
pub const PRIORITY_KEY: &str = "priority";
/// The key of the result count of a query: an Int >= 1.
pub const K_KEY: &str = "k";
/// The key of the result mode of a query: the string `"all"`.
pub const RESULTS_KEY: &str = "results";
/// The only value of `results`.
pub const RESULTS_ALL: &str = "all";
/// The key of the similarity threshold: a Float, a BigRat or a FixedPoint in [0, 1].
pub const THRESHOLD_KEY: &str = "threshold";
/// The key of the distance bound of a VectorDB query (P6). A query takes a `threshold` or a
/// `radius`, not both (decision 56).
pub const RADIUS_KEY: &str = "radius";
/// The key of the similarity metric: a word of `METRIC_VOCABULARY`.
pub const METRIC_KEY: &str = "metric";
/// The similarity metrics of the VectorDB spaces.
pub const METRIC_VOCABULARY: [&str; 6] = [
    "cosine",
    "dot",
    "euclidean",
    "manhattan",
    "hamming",
    "jaccard",
];

/// The position of a name in the Reified syntax.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IdentifierPosition {
    /// The key of a named param.
    ParamKey,
    /// The theory name of `free Th(a…)`.
    TheoryName,
}

impl fmt::Display for IdentifierPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            IdentifierPosition::ParamKey => "param key",
            IdentifierPosition::TheoryName => "theory name",
        })
    }
}

/// True when the parser accepts `name` as the key of a named param.
pub fn is_param_key(name: &str) -> bool {
    is_identifier(name) && !REJECTED_PARAM_KEYS.contains(&name)
}

/// True when the parser accepts `name` as the theory name of `free Th(a…)`.
pub fn is_theory_name(name: &str) -> bool {
    is_identifier(name) && !REJECTED_THEORY_NAMES.contains(&name)
}

/// Reified syntax that a builder or a validator rejects.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReifiedSyntaxError {
    #[error("{name:?} is not a valid {position}")]
    InvalidIdentifier {
        name: String,
        position: IdentifierPosition,
    },
    #[error("the key {key:?} occurs more than once")]
    DuplicateKey { key: String },
    #[error("a positional param follows a named param")]
    PositionalAfterNamed,
    #[error("a query takes named params only")]
    PositionalQueryParam,
    #[error("a query takes `k` or `results`, not both")]
    KAndResults,
    #[error("a query takes `threshold` or `radius`, not both")]
    ConflictingParams,
    #[error("PARAMS needs a positional or a named param")]
    EmptyParams,
    #[error("a query needs a payload element or a named param")]
    EmptyQuery,
    #[error("NEWSPACE needs an annotated binder")]
    NoAnnotation,
    #[error(
        "{annotations} binder annotations for a New with {bind_count} binders and {uris} URIs"
    )]
    AnnotationCount {
        annotations: usize,
        bind_count: i32,
        uris: usize,
    },
    #[error("a URI binder cannot take a space annotation")]
    SpaceTypeOnUriBinding,
    #[error("the value of {key:?} is not {expected}")]
    OutOfRange { key: String, expected: &'static str },
}

impl From<ReifiedEncodeError> for ReifiedSyntaxError {
    fn from(error: ReifiedEncodeError) -> Self {
        match error {
            ReifiedEncodeError::EmptyParams => ReifiedSyntaxError::EmptyParams,
            ReifiedEncodeError::EmptyQuery => ReifiedSyntaxError::EmptyQuery,
            ReifiedEncodeError::NoAnnotation => ReifiedSyntaxError::NoAnnotation,
            ReifiedEncodeError::AnnotationCount {
                annotations,
                bind_count,
                uris,
            } => ReifiedSyntaxError::AnnotationCount {
                annotations,
                bind_count,
                uris,
            },
            ReifiedEncodeError::InvalidIdentifier { name, position } => {
                ReifiedSyntaxError::InvalidIdentifier { name, position }
            }
        }
    }
}

/// Checks the value of a named param with a fixed range. `priority` is an Int >= 0, `k` is an
/// Int >= 1, `results` is `"all"`, `threshold` is a Float, a BigRat or a FixedPoint in [0, 1]
/// (decision 56), and `metric` is a word of `METRIC_VOCABULARY`. Every other key has no range
/// at this layer.
pub fn validate_param_value(key: &str, value: &Par) -> Result<(), ReifiedSyntaxError> {
    let (in_range, expected) = match key {
        PRIORITY_KEY => (sole_int(value).is_some_and(|int| int >= 0), "an Int >= 0"),
        K_KEY => (sole_int(value).is_some_and(|int| int >= 1), "an Int >= 1"),
        RESULTS_KEY => (sole_str(value) == Some(RESULTS_ALL), "the string \"all\""),
        THRESHOLD_KEY => (
            in_unit_interval(value),
            "a Float, a BigRat or a FixedPoint in [0, 1]",
        ),
        METRIC_KEY => (
            sole_str(value).is_some_and(|metric| METRIC_VOCABULARY.contains(&metric)),
            "one of cosine, dot, euclidean, manhattan, hamming, jaccard",
        ),
        _ => (true, ""),
    };
    match in_range {
        true => Ok(()),
        false => Err(ReifiedSyntaxError::OutOfRange {
            key: key.to_owned(),
            expected,
        }),
    }
}

fn sole_int(value: &Par) -> Option<i64> {
    match sole_expr(value) {
        Some(ExprInstance::GInt(int)) => Some(*int),
        _ => None,
    }
}

fn sole_str(value: &Par) -> Option<&str> {
    match sole_expr(value) {
        Some(ExprInstance::GString(string)) => Some(string),
        _ => None,
    }
}

fn in_unit_interval(value: &Par) -> bool {
    match sole_expr(value) {
        Some(ExprInstance::GDouble(bits)) => (0.0..=1.0).contains(&f64::from_bits(*bits)),
        Some(ExprInstance::GBigRat(GBigRational {
            numerator,
            denominator,
        })) => rational_in_unit_interval(numerator, denominator),
        Some(ExprInstance::GFixedPoint(GFixedPoint { unscaled, scale })) => {
            fixed_point_in_unit_interval(unscaled, *scale)
        }
        _ => false,
    }
}

/// True when `unscaled / 10^scale` lies in [0, 1], that is `0 <= unscaled <= 10^scale`. The
/// unscaled value is a big-endian two's complement integer of `n` magnitude bytes, so it is
/// below `256^n`. Since `log10 256 < 2.409`, a scale with `1000 scale >= 2409 n` has
/// `10^scale >= 256^n`, and the value is in range without a power. A smaller scale has a power
/// of at most `n + 1` bytes, which the check computes exactly.
fn fixed_point_in_unit_interval(unscaled: &[u8], scale: u32) -> bool {
    let Some(magnitude) = non_negative_magnitude(unscaled) else {
        return false;
    };
    // A length of at most 2^64 times 2409 stays far below 2^128.
    match u128::try_from(magnitude.len()) {
        Ok(length) if u128::from(scale) * 1000 >= length * 2409 => true,
        Ok(_) | Err(_) => {
            let power = power_of_ten_magnitude(scale);
            (magnitude.len(), magnitude) <= (power.len(), power.as_slice())
        }
    }
}

/// The big-endian magnitude of `10^scale`, without leading zero bytes.
fn power_of_ten_magnitude(scale: u32) -> Vec<u8> {
    let mut little_endian = vec![1u8];
    for _ in 0..scale {
        let mut carry = 0u16;
        for byte in little_endian.iter_mut() {
            let product = u16::from(*byte) * 10 + carry;
            *byte = product.to_le_bytes()[0];
            carry = product >> 8;
        }
        if carry > 0 {
            little_endian.push(carry.to_le_bytes()[0]);
        }
    }
    little_endian.reverse();
    little_endian
}

/// True when `numerator / denominator` lies in [0, 1]. Both are big-endian two's complement
/// integers, and a canonical denominator is positive.
fn rational_in_unit_interval(numerator: &[u8], denominator: &[u8]) -> bool {
    match (
        non_negative_magnitude(numerator),
        non_negative_magnitude(denominator),
    ) {
        (Some(numerator), Some(denominator)) => {
            !denominator.is_empty()
                && (numerator.len(), numerator) <= (denominator.len(), denominator)
        }
        (Some(_), None) | (None, Some(_) | None) => false,
    }
}

/// The big-endian magnitude, without leading zero bytes, of a non-negative two's complement
/// integer. A negative integer has none.
fn non_negative_magnitude(bytes: &[u8]) -> Option<&[u8]> {
    match bytes.first() {
        Some(first) if first & 0x80 != 0 => None,
        Some(_) | None => {
            let mut magnitude = bytes;
            while let [0, rest @ ..] = magnitude {
                magnitude = rest;
            }
            Some(magnitude)
        }
    }
}
