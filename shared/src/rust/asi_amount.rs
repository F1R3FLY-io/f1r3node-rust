use num_bigint::{BigInt, Sign};
use serde::{de, Deserialize, Deserializer, Serializer};

pub const MAX_DECIMAL: &str =
    "115792089237316195423570985008687907853269984665640564039457584007913129639935";
pub const WIRE_BYTES: usize = 32;

pub fn maximum() -> BigInt { (BigInt::from(1) << 256usize) - 1 }

pub fn validate(amount: &BigInt) -> Result<(), String> {
    if amount.sign() == Sign::Minus || amount.bits() > 256 {
        Err("ASI amount must be between 0 and 2^256-1".to_string())
    } else {
        Ok(())
    }
}

pub fn parse_decimal(value: &str) -> Result<BigInt, String> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err("ASI amount must be a canonical unsigned decimal integer".to_string());
    }
    value.parse::<BigInt>().map_err(|error| error.to_string())
}

pub fn parse(value: &str) -> Result<BigInt, String> {
    if value.len() > MAX_DECIMAL.len() {
        return Err("ASI amount must be between 0 and 2^256-1".to_string());
    }
    let amount = parse_decimal(value)?;
    validate(&amount)?;
    Ok(amount)
}

pub fn encode(amount: &BigInt) -> Result<[u8; WIRE_BYTES], String> {
    validate(amount)?;
    let (_, bytes) = amount.to_bytes_be();
    let mut encoded = [0; WIRE_BYTES];
    encoded[WIRE_BYTES - bytes.len()..].copy_from_slice(&bytes);
    Ok(encoded)
}

pub fn decode(bytes: &[u8]) -> Result<BigInt, String> {
    if bytes.len() != WIRE_BYTES {
        return Err("ASI amount encoding must contain exactly 32 bytes".to_string());
    }
    Ok(BigInt::from_bytes_be(Sign::Plus, bytes))
}

pub fn serialize<S: Serializer>(amount: &BigInt, serializer: S) -> Result<S::Ok, S::Error> {
    validate(amount).map_err(serde::ser::Error::custom)?;
    serializer.serialize_str(&amount.to_string())
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BigInt, D::Error> {
    parse(&String::deserialize(deserializer)?).map_err(de::Error::custom)
}

pub mod optional {
    use super::*;

    pub fn serialize<S: Serializer>(
        amount: &Option<BigInt>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match amount {
            Some(amount) => {
                validate(amount).map_err(serde::ser::Error::custom)?;
                serializer.serialize_some(&amount.to_string())
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<BigInt>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|value| parse(&value).map_err(de::Error::custom))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde::Serialize;

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Amount(#[serde(with = "super")] BigInt);

    #[test]
    fn boundaries_round_trip_exactly() {
        for amount in [
            BigInt::from(0),
            BigInt::from(1),
            BigInt::from(i64::MAX),
            BigInt::from(1) << 63usize,
            BigInt::from(1) << 128usize,
            maximum(),
        ] {
            assert_eq!(decode(&encode(&amount).unwrap()).unwrap(), amount);
            let json = serde_json::to_string(&Amount(amount.clone())).unwrap();
            assert_eq!(json, format!("\"{}\"", amount));
            assert_eq!(serde_json::from_str::<Amount>(&json).unwrap().0, amount);
        }
        assert_eq!(parse(MAX_DECIMAL).unwrap(), maximum());
    }

    #[test]
    fn invalid_amounts_and_encodings_are_rejected() {
        for amount in [BigInt::from(-1), maximum() + 1] {
            assert!(encode(&amount).is_err());
            assert!(parse(&amount.to_string()).is_err());
            assert!(serde_json::to_string(&Amount(amount)).is_err());
        }
        for value in ["", "00", "01", "+1", "-0", "1.0", "1e2", " 1", "1 "] {
            assert!(parse(value).is_err(), "{value}");
        }
        assert!(serde_json::from_str::<Amount>("1").is_err());
        for length in [0, 1, 31, 33, 64] {
            assert!(decode(&vec![0; length]).is_err());
        }
    }

    proptest! {
        #[test]
        fn every_256_bit_encoding_is_canonical(bytes in any::<[u8; 32]>()) {
            let amount = decode(&bytes).unwrap();
            prop_assert_eq!(encode(&amount).unwrap(), bytes);
            prop_assert_eq!(parse(&amount.to_string()).unwrap(), amount);
        }
    }
}
