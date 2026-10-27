use serde::Serializer;

/// Always serialize as an empty byte vec, regardless of actual content.
///
/// Used for `locally_free` fields which are transient analysis data
/// (free-variable bitvectors) that must NOT affect Blake2b256 channel
/// hashes in RSpace. This matches Scala's `AlwaysEqual` semantics
/// where `locally_free` is ignored for equality, hashing, and
/// effectively for serialization-based hashing.
pub fn serialize_as_empty_bytes<S: Serializer>(
    _value: &Vec<u8>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_bytes(&[])
}

pub mod decimal_bigint {
    use std::fmt;

    use num_bigint::BigInt;
    use serde::{de, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &BigInt, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BigInt, D::Error> {
        struct DecimalVisitor;
        impl<'de> de::Visitor<'de> for DecimalVisitor {
            type Value = BigInt;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an integer or decimal integer string")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<BigInt, E> {
                let digits = value.strip_prefix('-').unwrap_or(value);
                if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(E::custom("expected a decimal integer string"));
                }
                value.parse().map_err(E::custom)
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<BigInt, E> { Ok(value.into()) }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<BigInt, E> { Ok(value.into()) }
        }
        deserializer.deserialize_any(DecimalVisitor)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct DecimalBigInt(#[serde(with = "decimal_bigint")] num_bigint::BigInt);

pub mod optional_decimal_bigint {
    use num_bigint::BigInt;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::DecimalBigInt;

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
        Ok(Option::<DecimalBigInt>::deserialize(deserializer)?.map(|value| value.0))
    }
}

pub fn serialize_decimal_bigint_vec<S: Serializer>(
    values: &[num_bigint::BigInt],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .serialize(serializer)
}

#[cfg(test)]
mod bigint_json_tests {
    use super::DecimalBigInt;

    #[test]
    fn decimal_input_preserves_precision_and_rejects_non_decimal_syntax() {
        for value in ["0", "100000000000000000001", "-1"] {
            let encoded = serde_json::to_string(value).unwrap();
            let decoded: DecimalBigInt = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded.0.to_string(), value);
            assert_eq!(serde_json::to_string(&decoded).unwrap(), encoded);
        }
        for value in ["+1", "1_000", "1.5", "1e18", "", " 1", "--1"] {
            assert!(serde_json::from_value::<DecimalBigInt>(serde_json::json!(value)).is_err());
        }
        assert!(serde_json::from_str::<DecimalBigInt>("1.5").is_err());
        assert_eq!(
            serde_json::from_str::<DecimalBigInt>("9223372036854775807")
                .unwrap()
                .0
                .to_string(),
            "9223372036854775807"
        );
    }
}
