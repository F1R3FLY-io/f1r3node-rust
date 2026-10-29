use std::fmt::Display;
use std::str::FromStr;

use serde::{de, Deserialize, Deserializer, Serializer};

#[derive(Deserialize)]
#[serde(untagged)]
enum Repr {
    String(String),
    Int(i64),
    Uint(u64),
}

fn is_decimal(s: &str) -> bool {
    if s == "0" {
        return true;
    }
    let digits = s.strip_prefix('-').unwrap_or(s);
    matches!(digits.as_bytes().first(), Some(b'1'..=b'9'))
        && digits.bytes().all(|b| b.is_ascii_digit())
}

impl Repr {
    fn parse<T: FromStr, E: de::Error>(self) -> Result<T, E>
    where T::Err: Display {
        match self {
            Repr::String(s) if is_decimal(&s) => s.parse(),
            Repr::String(s) => return Err(E::custom(format!("invalid decimal integer: {s:?}"))),
            Repr::Int(v) => v.to_string().parse(),
            Repr::Uint(v) => v.to_string().parse(),
        }
        .map_err(E::custom)
    }
}

pub fn serialize<T: Display, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
}

pub fn deserialize<'de, T: FromStr, D: Deserializer<'de>>(deserializer: D) -> Result<T, D::Error>
where T::Err: Display {
    Repr::deserialize(deserializer)?.parse()
}

pub mod option {
    use super::*;

    pub fn serialize<T: Display, S: Serializer>(
        value: &Option<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => serializer.collect_str(value),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, T: FromStr, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<T>, D::Error>
    where T::Err: Display {
        Option::<Repr>::deserialize(deserializer)?
            .map(Repr::parse)
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Signed(#[serde(with = "super")] i64);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Unsigned(#[serde(with = "super")] u64);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Optional(#[serde(with = "super::option")] Option<i64>);

    #[test]
    fn signed_boundaries_accept_legacy_numbers_and_emit_strings() {
        for value in [i64::MIN, -1, 0, 1, (1 << 53) + 1, i64::MAX] {
            let decimal = value.to_string();
            let encoded = serde_json::to_string(&Signed(value)).unwrap();
            assert_eq!(encoded, format!("\"{decimal}\""));
            assert_eq!(
                serde_json::from_str::<Signed>(&encoded).unwrap(),
                Signed(value)
            );
            assert_eq!(
                serde_json::from_str::<Signed>(&decimal).unwrap(),
                Signed(value)
            );
        }
    }

    #[test]
    fn unsigned_boundaries_accept_legacy_numbers_and_emit_strings() {
        for value in [0, 1, (1 << 53) + 1, i64::MAX as u64 + 1, u64::MAX] {
            let decimal = value.to_string();
            let encoded = serde_json::to_string(&Unsigned(value)).unwrap();
            assert_eq!(encoded, format!("\"{decimal}\""));
            assert_eq!(
                serde_json::from_str::<Unsigned>(&encoded).unwrap(),
                Unsigned(value)
            );
            assert_eq!(
                serde_json::from_str::<Unsigned>(&decimal).unwrap(),
                Unsigned(value)
            );
        }
    }

    #[test]
    fn noncanonical_integers_and_wrong_json_types_are_rejected() {
        for input in [
            "", "-", "+1", "00", "01", "-0", "-01", "1.0", "1e3", " 1", "1 ", "１２",
        ] {
            let encoded = serde_json::to_string(input).unwrap();
            assert!(serde_json::from_str::<Signed>(&encoded).is_err(), "{input}");
            assert!(
                serde_json::from_str::<Unsigned>(&encoded).is_err(),
                "{input}"
            );
        }
        for input in ["1.0", "1e3", "null", "true", "[]", "{}"] {
            assert!(serde_json::from_str::<Signed>(input).is_err(), "{input}");
            assert!(serde_json::from_str::<Unsigned>(input).is_err(), "{input}");
        }
        for input in ["9223372036854775808", "-9223372036854775809"] {
            assert!(serde_json::from_str::<Signed>(input).is_err());
            assert!(serde_json::from_str::<Signed>(&format!("\"{input}\"")).is_err());
        }
        for input in ["-1", "18446744073709551616"] {
            assert!(serde_json::from_str::<Unsigned>(input).is_err());
            assert!(serde_json::from_str::<Unsigned>(&format!("\"{input}\"")).is_err());
        }
    }

    #[test]
    fn optional_integer_preserves_null_and_legacy_values() {
        assert_eq!(serde_json::to_string(&Optional(None)).unwrap(), "null");
        assert_eq!(
            serde_json::from_str::<Optional>("null").unwrap(),
            Optional(None)
        );
        for value in [i64::MIN, 0, i64::MAX] {
            let encoded = serde_json::to_string(&Optional(Some(value))).unwrap();
            assert_eq!(encoded, format!("\"{value}\""));
            assert_eq!(
                serde_json::from_str::<Optional>(&value.to_string()).unwrap(),
                Optional(Some(value))
            );
            assert_eq!(
                serde_json::from_str::<Optional>(&encoded).unwrap(),
                Optional(Some(value))
            );
        }
        assert!(serde_json::from_str::<Optional>("\"-0\"").is_err());
    }
}
