use std::fmt;
use std::str::FromStr;

use num_bigint::{BigInt, Sign};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

pub const MAX_DECIMAL: &str =
    "115792089237316195423570985008687907853269984665640564039457584007913129639935";
pub const WIRE_BYTES: usize = 32;

const OUT_OF_RANGE: &str = "token amount must be between 0 and 2^256-1";

/// A token amount in `[0, 2^256 - 1]`. Construction checks the range, so
/// every held value has a 32-byte wire form and a canonical decimal form.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenAmount(BigInt);

impl TokenAmount {
    pub fn zero() -> Self { Self(BigInt::from(0)) }

    pub fn max() -> Self { Self((BigInt::from(1) << 256usize) - 1) }

    pub fn new(value: BigInt) -> Result<Self, String> {
        if value.sign() == Sign::Minus || value.bits() > 256 {
            Err(OUT_OF_RANGE.to_string())
        } else {
            Ok(Self(value))
        }
    }

    /// Parses a canonical unsigned decimal: digits only, no leading zero.
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.len() > MAX_DECIMAL.len() {
            return Err(OUT_OF_RANGE.to_string());
        }
        Self::new(decimal::parse(value)?)
    }

    pub fn from_wire(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != WIRE_BYTES {
            return Err("token amount encoding must contain exactly 32 bytes".to_string());
        }
        Ok(Self(BigInt::from_bytes_be(Sign::Plus, bytes)))
    }

    pub fn to_wire(&self) -> [u8; WIRE_BYTES] {
        let (_, bytes) = self.0.to_bytes_be();
        let mut encoded = [0; WIRE_BYTES];
        encoded[WIRE_BYTES - bytes.len()..].copy_from_slice(&bytes);
        encoded
    }

    pub fn as_bigint(&self) -> &BigInt { &self.0 }

    pub fn to_bigint(&self) -> BigInt { self.0.clone() }

    pub fn into_bigint(self) -> BigInt { self.0 }

    pub fn is_zero(&self) -> bool { self.0.sign() == Sign::NoSign }
}

impl From<u64> for TokenAmount {
    fn from(value: u64) -> Self { Self(BigInt::from(value)) }
}

impl From<TokenAmount> for BigInt {
    fn from(value: TokenAmount) -> Self { value.0 }
}

impl TryFrom<BigInt> for TokenAmount {
    type Error = String;

    fn try_from(value: BigInt) -> Result<Self, String> { Self::new(value) }
}

impl fmt::Display for TokenAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
}

impl FromStr for TokenAmount {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> { Self::parse(value) }
}

impl Serialize for TokenAmount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TokenAmount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Unbounded non-negative integers in the same canonical decimal form, for
/// totals that can exceed a single amount's range.
pub mod decimal {
    use num_bigint::BigInt;
    use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

    pub fn parse(value: &str) -> Result<BigInt, String> {
        if value.is_empty()
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err("amount must be a canonical unsigned decimal integer".to_string());
        }
        value.parse::<BigInt>().map_err(|error| error.to_string())
    }

    pub fn serialize<S: Serializer>(value: &BigInt, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BigInt, D::Error> {
        parse(&String::deserialize(deserializer)?).map_err(de::Error::custom)
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
                .map(|value| parse(&value).map_err(de::Error::custom))
                .transpose()
        }
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
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn boundaries_round_trip_exactly() {
        for amount in [
            BigInt::from(0),
            BigInt::from(1),
            BigInt::from(i64::MAX),
            BigInt::from(1) << 63usize,
            BigInt::from(1) << 128usize,
            TokenAmount::max().into_bigint(),
        ] {
            let token = TokenAmount::new(amount.clone()).unwrap();
            assert_eq!(TokenAmount::from_wire(&token.to_wire()).unwrap(), token);
            let json = serde_json::to_string(&token).unwrap();
            assert_eq!(json, format!("\"{}\"", amount));
            assert_eq!(serde_json::from_str::<TokenAmount>(&json).unwrap(), token);
        }
        assert_eq!(TokenAmount::parse(MAX_DECIMAL).unwrap(), TokenAmount::max());
    }

    #[test]
    fn out_of_range_values_and_encodings_are_rejected() {
        for amount in [BigInt::from(-1), TokenAmount::max().into_bigint() + 1] {
            assert!(TokenAmount::new(amount.clone()).is_err());
            assert!(TokenAmount::parse(&amount.to_string()).is_err());
        }
        for value in ["", "00", "01", "+1", "-0", "1.0", "1e2", " 1", "1 "] {
            assert!(TokenAmount::parse(value).is_err(), "{value}");
        }
        assert!(serde_json::from_str::<TokenAmount>("1").is_err());
        for length in [0, 1, 31, 33, 64] {
            assert!(TokenAmount::from_wire(&vec![0; length]).is_err());
        }
    }

    #[test]
    fn totals_beyond_one_amount_use_the_same_grammar() {
        #[derive(Serialize, Deserialize)]
        struct Total(#[serde(with = "decimal")] BigInt);
        let total = TokenAmount::max().into_bigint() * 2u8;
        let json = serde_json::to_string(&Total(total.clone())).unwrap();
        assert_eq!(serde_json::from_str::<Total>(&json).unwrap().0, total);
        for value in ["\"-1\"", "\"007\"", "\"+1\"", "0"] {
            assert!(serde_json::from_str::<Total>(value).is_err(), "{value}");
        }
    }

    proptest! {
        #[test]
        fn every_32_byte_encoding_is_canonical(bytes in any::<[u8; 32]>()) {
            let amount = TokenAmount::from_wire(&bytes).unwrap();
            prop_assert_eq!(amount.to_wire(), bytes);
            prop_assert_eq!(TokenAmount::parse(&amount.to_string()).unwrap(), amount);
        }
    }
}
