//! Canonical base-10 integers: ASCII digits without a leading zero, and a
//! leading `-` only on a non-zero signed value.

use std::fmt::Display;

use num_bigint::BigInt;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

pub fn parse_unsigned(value: &str) -> Result<BigInt, String> {
    if !is_canonical_magnitude(value) {
        return Err(format!(
            "expected a canonical unsigned decimal integer, got {value:?}"
        ));
    }
    value.parse::<BigInt>().map_err(|error| error.to_string())
}

pub fn parse_signed(value: &str) -> Result<BigInt, String> {
    let magnitude = match value.strip_prefix('-') {
        Some("0") => None,
        Some(magnitude) => Some(magnitude),
        None => Some(value),
    };
    if !magnitude.is_some_and(is_canonical_magnitude) {
        return Err(format!(
            "expected a canonical decimal integer, got {value:?}"
        ));
    }
    value.parse::<BigInt>().map_err(|error| error.to_string())
}

fn is_canonical_magnitude(digits: &str) -> bool {
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
}

/// Non-negative integers of any size.
pub mod unsigned {
    use super::*;

    pub fn serialize<S: Serializer>(value: &BigInt, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BigInt, D::Error> {
        parse_unsigned(&String::deserialize(deserializer)?).map_err(de::Error::custom)
    }

    pub fn serialize_vec<S: Serializer>(
        values: &[BigInt],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(
            value: &Option<BigInt>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            value
                .as_ref()
                .map(ToString::to_string)
                .serialize(serializer)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<BigInt>, D::Error> {
            Option::<String>::deserialize(deserializer)?
                .map(|value| parse_unsigned(&value).map_err(de::Error::custom))
                .transpose()
        }
    }
}

/// Signed integers, held in any type that a `BigInt` converts into with a
/// range check (`i64`, `u64`, `BigInt`).
pub mod signed {
    use super::*;

    pub fn serialize<T: Display, S: Serializer>(
        value: &T,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: TryFrom<BigInt>,
        T::Error: Display,
        D: Deserializer<'de>,
    {
        let value = parse_signed(&String::deserialize(deserializer)?).map_err(de::Error::custom)?;
        T::try_from(value).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Signed64(#[serde(with = "signed")] i64);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Unsigned64(#[serde(with = "signed")] u64);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Wide(#[serde(with = "signed")] BigInt);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Total(#[serde(with = "unsigned")] BigInt);

    #[test]
    fn integer_boundaries_round_trip_as_strings() {
        for value in [i64::MIN, -1, 0, 1, (1 << 53) + 1, i64::MAX] {
            let json = serde_json::to_string(&Signed64(value)).unwrap();
            assert_eq!(json, format!("\"{value}\""));
            assert_eq!(serde_json::from_str::<Signed64>(&json).unwrap().0, value);
        }
        for value in [0, (1 << 53) + 1, i64::MAX as u64 + 1, u64::MAX] {
            let json = serde_json::to_string(&Unsigned64(value)).unwrap();
            assert_eq!(serde_json::from_str::<Unsigned64>(&json).unwrap().0, value);
        }
        for value in [BigInt::from(-1) << 1024usize, BigInt::from(1) << 1024usize] {
            let json = serde_json::to_string(&Wide(value.clone())).unwrap();
            assert_eq!(serde_json::from_str::<Wide>(&json).unwrap().0, value);
        }
    }

    #[test]
    fn non_canonical_text_and_json_numbers_are_rejected() {
        for text in [
            "", "-", "+1", "00", "01", "-0", "-01", "1.0", "1e3", " 1", "1 ", "１２", "0x1",
        ] {
            assert!(parse_signed(text).is_err(), "{text:?}");
            assert!(parse_unsigned(text).is_err(), "{text:?}");
        }
        assert!(parse_unsigned("-1").is_err());
        for json in ["1", "-1", "1.5", "null", "true"] {
            assert!(serde_json::from_str::<Signed64>(json).is_err(), "{json}");
            assert!(serde_json::from_str::<Total>(json).is_err(), "{json}");
        }
    }

    #[test]
    fn values_outside_the_target_type_are_rejected() {
        for text in ["9223372036854775808", "-9223372036854775809"] {
            assert!(serde_json::from_str::<Signed64>(&format!("\"{text}\"")).is_err());
        }
        for text in ["-1", "18446744073709551616"] {
            assert!(serde_json::from_str::<Unsigned64>(&format!("\"{text}\"")).is_err());
        }
    }
}
