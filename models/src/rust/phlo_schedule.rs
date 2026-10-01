use std::collections::BTreeSet;

use crypto::rust::hash::blake2b256::Blake2b256;
use thiserror::Error;

use super::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

pub const PHLO_SCHEDULE_V1_DOMAIN: &[u8] = b"f1r3node:phlo-schedule:v1";

pub const PHLO_GENESIS_POLICY_V1_DOMAIN: &[u8] = b"f1r3node:genesis-resource-policy:v1";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PhloGenesisPolicy {
    schedule_bytes: Vec<u8>,
}

impl PhloGenesisPolicy {
    pub const LIMITS: PhloScheduleLimits = PhloScheduleLimits {
        wire: PhloWireLimits {
            total_bytes: 1_048_576,
            field_bytes: 524_288,
        },
        classes: 4096,
    };

    pub fn from_schedule(schedule: &PhloScheduleV1<'_>) -> Result<Self, PhloScheduleError> {
        schedule.validate(Self::LIMITS)?;
        let mut policy = schedule.clone();
        policy.actual_price = 0;
        let record = Self {
            schedule_bytes: policy.encode(Self::LIMITS)?,
        };
        record.encode()?;
        Ok(record)
    }

    pub fn schedule(&self) -> Result<PhloScheduleV1<'_>, PhloScheduleError> {
        PhloScheduleV1::decode(&self.schedule_bytes, Self::LIMITS)
    }

    pub fn encode(&self) -> Result<Vec<u8>, PhloScheduleError> {
        let mut wire = PhloWireEncoder::new(Self::LIMITS.wire);
        wire.bytes(PHLO_GENESIS_POLICY_V1_DOMAIN)?;
        wire.bytes(&self.schedule_bytes)?;
        Ok(wire.into_bytes())
    }

    pub fn decode(input: &[u8]) -> Result<Self, PhloScheduleError> {
        let mut wire = PhloWireDecoder::new(input, Self::LIMITS.wire)?;
        if wire.bytes()? != PHLO_GENESIS_POLICY_V1_DOMAIN {
            return Err(PhloScheduleError::FormatDomain);
        }
        let bytes = wire.bytes()?;
        wire.finish()?;
        let schedule = PhloScheduleV1::decode(bytes, Self::LIMITS)?;
        if schedule.actual_price != 0 {
            return Err(PhloScheduleError::GenesisPolicyPrice);
        }
        let mut schedule_bytes = Vec::new();
        schedule_bytes
            .try_reserve_exact(bytes.len())
            .map_err(|_| PhloWireError::AllocationFailed)?;
        schedule_bytes.extend_from_slice(bytes);
        Ok(Self { schedule_bytes })
    }

    pub fn validate_context(
        &self,
        protocol: i64,
        shard: &str,
        decimal_scale: u32,
    ) -> Result<(), PhloScheduleError> {
        let policy = self.schedule()?;
        if u64::try_from(protocol).ok() != Some(policy.protocol_version)
            || policy.shard != shard.as_bytes()
            || u32::from(policy.decimal_scale) != decimal_scale
        {
            return Err(PhloScheduleError::GenesisPolicyContext);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloResourceClassV1<'a> {
    pub identity: &'a [u8],
    pub measurement_unit: &'a [u8],
    pub measurement_rule: [u8; 32],
    pub valuation_rule: [u8; 32],
    pub weight: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloScheduleV1<'a> {
    pub protocol_version: u64,
    pub network: &'a [u8],
    pub shard: &'a [u8],
    pub settlement_asset: &'a [u8],
    pub settlement_unit: &'a [u8],
    pub decimal_scale: u8,
    pub classes: Vec<PhloResourceClassV1<'a>>,
    pub actual_price: u64,
    pub compatibility_rule: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloScheduleLimits {
    pub wire: PhloWireLimits,
    pub classes: usize,
}

impl PhloScheduleLimits {
    fn nested(self) -> PhloWireLimits {
        PhloWireLimits {
            total_bytes: self.wire.field_bytes,
            field_bytes: self.wire.field_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloScheduleError {
    #[error("genesis resource policy must not contain an offered price")]
    GenesisPolicyPrice,
    #[error("genesis resource policy differs from its chain context")]
    GenesisPolicyContext,
    #[error("unsupported phlo schedule format domain")]
    FormatDomain,
    #[error("phlo schedule requires nonempty identities")]
    EmptyIdentity,
    #[error("phlo schedule requires resource classes")]
    EmptyClasses,
    #[error("phlo schedule exceeds the resource-class limit")]
    ClassLimit,
    #[error("phlo schedule repeats a resource-class identity")]
    DuplicateClass,
    #[error("phlo schedule field has a noncanonical width")]
    FieldWidth,
    #[error("phlo schedule fee differs from the fixed one-unit policy")]
    FeePolicy,
    #[error(transparent)]
    Wire(#[from] PhloWireError),
}

impl<'a> PhloScheduleV1<'a> {
    pub fn encode(&self, limits: PhloScheduleLimits) -> Result<Vec<u8>, PhloScheduleError> {
        self.validate(limits)?;
        let count = u32::try_from(self.classes.len()).map_err(|_| PhloScheduleError::ClassLimit)?;
        let mut classes = PhloWireEncoder::new(limits.nested());
        classes.bytes(&count.to_be_bytes())?;
        for class in &self.classes {
            let mut record = PhloWireEncoder::new(limits.nested());
            record.bytes(class.identity)?;
            record.bytes(class.measurement_unit)?;
            record.bytes(&class.measurement_rule)?;
            record.bytes(&class.valuation_rule)?;
            record.bytes(&class.weight.to_be_bytes())?;
            classes.bytes(record.as_bytes())?;
        }
        let mut output = PhloWireEncoder::new(limits.wire);
        for field in [
            PHLO_SCHEDULE_V1_DOMAIN,
            &self.protocol_version.to_be_bytes(),
            self.network,
            self.shard,
            self.settlement_asset,
            self.settlement_unit,
            &[self.decimal_scale],
            classes.as_bytes(),
            &self.actual_price.to_be_bytes(),
            &1u64.to_be_bytes(),
            &self.compatibility_rule,
        ] {
            output.bytes(field)?;
        }
        Ok(output.into_bytes())
    }

    pub fn digest(&self, limits: PhloScheduleLimits) -> Result<[u8; 32], PhloScheduleError> {
        Blake2b256::hash(self.encode(limits)?)
            .try_into()
            .map_err(|_| PhloScheduleError::FieldWidth)
    }

    pub fn decode(input: &'a [u8], limits: PhloScheduleLimits) -> Result<Self, PhloScheduleError> {
        let mut fields = PhloWireDecoder::new(input, limits.wire)?;
        if fields.bytes()? != PHLO_SCHEDULE_V1_DOMAIN {
            return Err(PhloScheduleError::FormatDomain);
        }
        let protocol_version = u64::from_be_bytes(fixed_field(&mut fields)?);
        let network = fields.bytes()?;
        let shard = fields.bytes()?;
        let settlement_asset = fields.bytes()?;
        let settlement_unit = fields.bytes()?;
        let decimal_scale = u8::from_be_bytes(fixed_field(&mut fields)?);
        let classes = Self::decode_classes(fields.bytes()?, limits)?;
        let actual_price = u64::from_be_bytes(fixed_field(&mut fields)?);
        if u64::from_be_bytes(fixed_field(&mut fields)?) != 1 {
            return Err(PhloScheduleError::FeePolicy);
        }
        let compatibility_rule = fixed_field(&mut fields)?;
        fields.finish()?;
        let schedule = Self {
            protocol_version,
            network,
            shard,
            settlement_asset,
            settlement_unit,
            decimal_scale,
            classes,
            actual_price,
            compatibility_rule,
        };
        schedule.validate(limits)?;
        Ok(schedule)
    }

    fn validate(&self, limits: PhloScheduleLimits) -> Result<(), PhloScheduleError> {
        if self.classes.is_empty() {
            return Err(PhloScheduleError::EmptyClasses);
        }
        if self.classes.len() > limits.classes {
            return Err(PhloScheduleError::ClassLimit);
        }
        if [
            self.network,
            self.shard,
            self.settlement_asset,
            self.settlement_unit,
        ]
        .iter()
        .any(|identity| identity.is_empty())
        {
            return Err(PhloScheduleError::EmptyIdentity);
        }
        let mut identities = BTreeSet::new();
        for class in &self.classes {
            if class.identity.is_empty() || class.measurement_unit.is_empty() {
                return Err(PhloScheduleError::EmptyIdentity);
            }
            if !identities.insert(class.identity) {
                return Err(PhloScheduleError::DuplicateClass);
            }
        }
        Ok(())
    }

    fn decode_classes(
        input: &'a [u8],
        limits: PhloScheduleLimits,
    ) -> Result<Vec<PhloResourceClassV1<'a>>, PhloScheduleError> {
        let mut fields = PhloWireDecoder::new(input, limits.nested())?;
        let count = u32::from_be_bytes(fixed_field(&mut fields)?);
        if count == 0 {
            return Err(PhloScheduleError::EmptyClasses);
        }
        let count = usize::try_from(count).map_err(|_| PhloScheduleError::ClassLimit)?;
        if count > limits.classes {
            return Err(PhloScheduleError::ClassLimit);
        }
        let mut classes = Vec::new();
        let mut identities = BTreeSet::new();
        for _ in 0..count {
            let mut class = PhloWireDecoder::new(fields.bytes()?, limits.nested())?;
            let identity = class.bytes()?;
            let measurement_unit = class.bytes()?;
            let measurement_rule = fixed_field(&mut class)?;
            let valuation_rule = fixed_field(&mut class)?;
            let weight = u64::from_be_bytes(fixed_field(&mut class)?);
            class.finish()?;
            if identity.is_empty() || measurement_unit.is_empty() {
                return Err(PhloScheduleError::EmptyIdentity);
            }
            if !identities.insert(identity) {
                return Err(PhloScheduleError::DuplicateClass);
            }
            classes
                .try_reserve(1)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            classes.push(PhloResourceClassV1 {
                identity,
                measurement_unit,
                measurement_rule,
                valuation_rule,
                weight,
            });
        }
        fields.finish()?;
        Ok(classes)
    }
}

fn fixed_field<const N: usize>(
    decoder: &mut PhloWireDecoder<'_>,
) -> Result<[u8; N], PhloScheduleError> {
    decoder
        .bytes()?
        .try_into()
        .map_err(|_| PhloScheduleError::FieldWidth)
}

#[cfg(test)]
mod tests;
