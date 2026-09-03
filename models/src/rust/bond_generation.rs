use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::rust::validator::Validator;

#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize
)]
pub struct BondGeneration(i64);

impl<'de> Deserialize<'de> for BondGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let value = i64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl BondGeneration {
    pub const GENESIS: Self = Self(0);

    pub fn new(value: i64) -> Result<Self, BondGenerationError> {
        if value < 0 {
            Err(BondGenerationError(value))
        } else {
            Ok(Self(value))
        }
    }

    pub const fn get(self) -> i64 { self.0 }

    pub fn next(self) -> Result<Self, BondGenerationOverflow> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(BondGenerationOverflow)
    }
}

impl TryFrom<i64> for BondGeneration {
    type Error = BondGenerationError;

    fn try_from(value: i64) -> Result<Self, Self::Error> { Self::new(value) }
}

impl From<BondGeneration> for i64 {
    fn from(generation: BondGeneration) -> Self { generation.get() }
}

impl fmt::Display for BondGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(formatter) }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("bond generation must be nonnegative, got {0}")]
pub struct BondGenerationError(i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("bond generation overflow")]
pub struct BondGenerationOverflow;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValidatorIncarnation {
    pub validator: Validator,
    pub generation: BondGeneration,
}

impl ValidatorIncarnation {
    pub fn new(validator: Validator, generation: BondGeneration) -> Self {
        Self {
            validator,
            generation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_negative_generation() {
        assert_eq!(
            BondGeneration::new(-1).unwrap_err().to_string(),
            "bond generation must be nonnegative, got -1"
        );
    }

    #[test]
    fn increments_without_wrapping() {
        assert_eq!(BondGeneration::GENESIS.next().unwrap().get(), 1);
        assert_eq!(
            BondGeneration::new(i64::MAX).unwrap().next(),
            Err(BondGenerationOverflow)
        );
    }
}
