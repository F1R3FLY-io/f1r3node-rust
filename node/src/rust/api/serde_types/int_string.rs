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
    let digits = s.strip_prefix('-').unwrap_or(s);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
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
