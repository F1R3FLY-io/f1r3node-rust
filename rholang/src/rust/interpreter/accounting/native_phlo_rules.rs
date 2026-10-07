use std::sync::LazyLock;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::phlo_schedule::{PhloResourceClassV1, PhloScheduleV1};
use thiserror::Error;

mod measurements;
mod acquisition;
mod execution;
pub use acquisition::{
    NativePhloAcquisitionDemand, NativePhloAcquisitionError, NativePhloAcquisitionLimits,
};
pub(super) use execution::{observation_comparison_bytes, CheckedNativeBudgetEvidence};
pub use execution::{
    CheckedNativeBudgetTrace, NativeAttemptStage, NativeBudgetAttempt, NativeBudgetOccurrence,
    NativeBudgetReplayDecision, NativeBudgetTraceError, NativeBudgetTraceLimits,
    NativePhloChargePreparer, NativePhloExecutionContract, NativePhloExecutionError,
    NativePhloReservation, PreparedNativePhloCharge,
};
pub use measurements::{
    NativePhloLocatedDemand, NativePhloLocatedDemands, NativePhloMeasurement,
    NativePhloMeasurementError, NativePhloMeasurements, NativePhloPurse, NativePhloPurseError,
    NativePhloPurseLimits, NativePhloRegionDemand, NativePhloRegionDemands, NativePhloRegionError,
    NativePhloRegionLimits,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativePhloDimension {
    Compute,
    Introduction,
    Transfer,
    Trace,
}

impl NativePhloDimension {
    pub const ALL: [Self; 4] = [
        Self::Compute,
        Self::Introduction,
        Self::Transfer,
        Self::Trace,
    ];

    fn index(self) -> usize {
        match self {
            Self::Compute => 0,
            Self::Introduction => 1,
            Self::Transfer => 2,
            Self::Trace => 3,
        }
    }

    pub fn measurement_rule(self) -> [u8; 32] { MEASUREMENT_RULES[self.index()] }

    pub fn measurement_unit(self) -> &'static [u8] {
        match self {
            Self::Compute => b"COMM",
            Self::Introduction | Self::Transfer | Self::Trace => b"byte",
        }
    }

    pub fn resource_class(self, identity: &[u8], weight: u64) -> PhloResourceClassV1<'_> {
        PhloResourceClassV1 {
            identity,
            measurement_unit: self.measurement_unit(),
            measurement_rule: self.measurement_rule(),
            valuation_rule: native_authority_valuation_rule(),
            weight,
        }
    }
}

fn rule_id(domain: &[u8]) -> [u8; 32] {
    Blake2b256::hash(domain.to_vec())
        .try_into()
        .expect("Blake2b-256 digest length")
}

static MEASUREMENT_RULES: LazyLock<[[u8; 32]; 4]> = LazyLock::new(|| {
    [
        b"f1r3node:phlo-measurement:comm-occurrence:v1".as_slice(),
        b"f1r3node:phlo-measurement:canonical-introduction-bytes:v1",
        b"f1r3node:phlo-measurement:canonical-delivery-bytes:v1",
        b"f1r3node:phlo-measurement:canonical-trace-footprint-bytes:v1",
    ]
    .map(rule_id)
});

pub fn native_authority_valuation_rule() -> [u8; 32] {
    static RULE: LazyLock<[u8; 32]> =
        LazyLock::new(|| rule_id(b"f1r3node:phlo-valuation:authority-leaf-occurrences:v1"));
    *RULE
}

pub fn native_resource_compatibility_rule() -> [u8; 32] {
    static RULE: LazyLock<[u8; 32]> = LazyLock::new(|| {
        rule_id(b"f1r3node:phlo-compatibility:exact-location-class-terms-authority:v1")
    });
    *RULE
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePhloRules {
    classes: [Option<usize>; 4],
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum NativePhloRuleError {
    #[error("native phlo policy requires between one and four resource classes")]
    ClassCount,
    #[error("unsupported native phlo measurement rule at class {0}")]
    Measurement(usize),
    #[error("unsupported native phlo measurement unit at class {0}")]
    Unit(usize),
    #[error("unsupported native phlo valuation rule at class {0}")]
    Valuation(usize),
    #[error("unsupported native phlo resource compatibility rule")]
    Compatibility,
    #[error("duplicate native phlo measurement dimension {0:?}")]
    Duplicate(NativePhloDimension),
    #[error("native phlo policy has no class for dimension {0:?}")]
    Missing(NativePhloDimension),
}

impl NativePhloRules {
    pub fn resolve(schedule: &PhloScheduleV1<'_>) -> Result<Self, NativePhloRuleError> {
        if schedule.classes.is_empty() || schedule.classes.len() > NativePhloDimension::ALL.len() {
            return Err(NativePhloRuleError::ClassCount);
        }
        if schedule.compatibility_rule != native_resource_compatibility_rule() {
            return Err(NativePhloRuleError::Compatibility);
        }
        let mut classes = [None; 4];
        for (index, class) in schedule.classes.iter().enumerate() {
            let dimension = NativePhloDimension::ALL
                .into_iter()
                .find(|dimension| dimension.measurement_rule() == class.measurement_rule)
                .ok_or(NativePhloRuleError::Measurement(index))?;
            if class.measurement_unit != dimension.measurement_unit() {
                return Err(NativePhloRuleError::Unit(index));
            }
            if class.valuation_rule != native_authority_valuation_rule() {
                return Err(NativePhloRuleError::Valuation(index));
            }
            if classes[dimension.index()].replace(index).is_some() {
                return Err(NativePhloRuleError::Duplicate(dimension));
            }
        }
        Ok(Self { classes })
    }

    pub fn class_for(&self, dimension: NativePhloDimension) -> Result<usize, NativePhloRuleError> {
        self.classes[dimension.index()].ok_or(NativePhloRuleError::Missing(dimension))
    }
}

#[cfg(test)]
mod tests;
