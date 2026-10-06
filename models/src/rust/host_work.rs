use std::fmt;

use serde::{Deserialize, Serialize};

pub const HOST_WORK_PHASE_COUNT: usize = 8;
pub const HOST_WORK_DIMENSION_COUNT: usize = 16;

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize
)]
pub enum HostWorkPhase {
    StructuralAdmission,
    PureReduction,
    PrimitiveEvaluation,
    Substitution,
    AuthorityDiscovery,
    PhysicalSearch,
    WitnessDecoding,
    WitnessVerification,
}

impl HostWorkPhase {
    pub const ALL: [Self; HOST_WORK_PHASE_COUNT] = [
        Self::StructuralAdmission,
        Self::PureReduction,
        Self::PrimitiveEvaluation,
        Self::Substitution,
        Self::AuthorityDiscovery,
        Self::PhysicalSearch,
        Self::WitnessDecoding,
        Self::WitnessVerification,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::StructuralAdmission => 0,
            Self::PureReduction => 1,
            Self::PrimitiveEvaluation => 2,
            Self::Substitution => 3,
            Self::AuthorityDiscovery => 4,
            Self::PhysicalSearch => 5,
            Self::WitnessDecoding => 6,
            Self::WitnessVerification => 7,
        }
    }
}

impl fmt::Display for HostWorkPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::StructuralAdmission => "structural admission",
            Self::PureReduction => "pure reduction",
            Self::PrimitiveEvaluation => "primitive evaluation",
            Self::Substitution => "substitution",
            Self::AuthorityDiscovery => "authority discovery",
            Self::PhysicalSearch => "physical search",
            Self::WitnessDecoding => "witness decoding",
            Self::WitnessVerification => "witness verification",
        })
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize
)]
pub enum HostWorkDimension {
    StructuralItems,
    StructuralBytes,
    ReductionSteps,
    ReductionTermBytes,
    PrimitiveCalls,
    PrimitiveInputBytes,
    SubstitutionBindings,
    SubstitutionBytes,
    AuthorityNodes,
    AuthorityDepth,
    SearchCandidates,
    SearchStateBytes,
    WitnessFields,
    WitnessBytes,
    VerificationOperations,
    VerificationBytes,
}

impl HostWorkDimension {
    pub const ALL: [Self; HOST_WORK_DIMENSION_COUNT] = [
        Self::StructuralItems,
        Self::StructuralBytes,
        Self::ReductionSteps,
        Self::ReductionTermBytes,
        Self::PrimitiveCalls,
        Self::PrimitiveInputBytes,
        Self::SubstitutionBindings,
        Self::SubstitutionBytes,
        Self::AuthorityNodes,
        Self::AuthorityDepth,
        Self::SearchCandidates,
        Self::SearchStateBytes,
        Self::WitnessFields,
        Self::WitnessBytes,
        Self::VerificationOperations,
        Self::VerificationBytes,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::StructuralItems => 0,
            Self::StructuralBytes => 1,
            Self::ReductionSteps => 2,
            Self::ReductionTermBytes => 3,
            Self::PrimitiveCalls => 4,
            Self::PrimitiveInputBytes => 5,
            Self::SubstitutionBindings => 6,
            Self::SubstitutionBytes => 7,
            Self::AuthorityNodes => 8,
            Self::AuthorityDepth => 9,
            Self::SearchCandidates => 10,
            Self::SearchStateBytes => 11,
            Self::WitnessFields => 12,
            Self::WitnessBytes => 13,
            Self::VerificationOperations => 14,
            Self::VerificationBytes => 15,
        }
    }

    pub const fn phase(self) -> HostWorkPhase {
        match self {
            Self::StructuralItems | Self::StructuralBytes => HostWorkPhase::StructuralAdmission,
            Self::ReductionSteps | Self::ReductionTermBytes => HostWorkPhase::PureReduction,
            Self::PrimitiveCalls | Self::PrimitiveInputBytes => HostWorkPhase::PrimitiveEvaluation,
            Self::SubstitutionBindings | Self::SubstitutionBytes => HostWorkPhase::Substitution,
            Self::AuthorityNodes | Self::AuthorityDepth => HostWorkPhase::AuthorityDiscovery,
            Self::SearchCandidates | Self::SearchStateBytes => HostWorkPhase::PhysicalSearch,
            Self::WitnessFields | Self::WitnessBytes => HostWorkPhase::WitnessDecoding,
            Self::VerificationOperations | Self::VerificationBytes => {
                HostWorkPhase::WitnessVerification
            }
        }
    }
}

impl fmt::Display for HostWorkDimension {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::StructuralItems => "structural items",
            Self::StructuralBytes => "structural bytes",
            Self::ReductionSteps => "reduction steps",
            Self::ReductionTermBytes => "reduction term bytes",
            Self::PrimitiveCalls => "primitive calls",
            Self::PrimitiveInputBytes => "primitive input bytes",
            Self::SubstitutionBindings => "substitution bindings",
            Self::SubstitutionBytes => "substitution bytes",
            Self::AuthorityNodes => "authority nodes",
            Self::AuthorityDepth => "authority depth",
            Self::SearchCandidates => "search candidates",
            Self::SearchStateBytes => "search state bytes",
            Self::WitnessFields => "witness fields",
            Self::WitnessBytes => "witness bytes",
            Self::VerificationOperations => "verification operations",
            Self::VerificationBytes => "verification bytes",
        })
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize
)]
pub struct HostWorkUnits(u64);

impl HostWorkUnits {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self { Self(value) }

    pub const fn get(self) -> u64 { self.0 }
}

impl From<u64> for HostWorkUnits {
    fn from(value: u64) -> Self { Self::new(value) }
}

impl From<HostWorkUnits> for u64 {
    fn from(value: HostWorkUnits) -> Self { value.get() }
}

impl fmt::Display for HostWorkUnits {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(formatter) }
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize
)]
pub struct HostWorkLimit(u64);

impl HostWorkLimit {
    pub const fn new(value: u64) -> Self { Self(value) }

    pub const fn get(self) -> u64 { self.0 }
}

impl From<u64> for HostWorkLimit {
    fn from(value: u64) -> Self { Self::new(value) }
}

impl From<HostWorkLimit> for u64 {
    fn from(value: HostWorkLimit) -> Self { value.get() }
}

impl fmt::Display for HostWorkLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(formatter) }
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize
)]
pub struct HostWorkUsage(u64);

impl HostWorkUsage {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self { Self(value) }

    pub const fn get(self) -> u64 { self.0 }

    pub fn checked_reserve(
        self,
        dimension: HostWorkDimension,
        limit: HostWorkLimit,
        requested: HostWorkUnits,
    ) -> Result<HostWorkReservation, HostWorkReservationError> {
        let Some(candidate) = self.0.checked_add(requested.0) else {
            return Err(HostWorkReservationError::UsageOverflow {
                dimension,
                limit,
                usage: self,
                requested,
            });
        };
        if candidate > limit.get() {
            return Err(HostWorkReservationError::LimitExceeded {
                dimension,
                limit,
                usage: self,
                requested,
            });
        }
        Ok(HostWorkReservation {
            dimension,
            usage_before: self,
            requested,
            usage_after: Self(candidate),
        })
    }
}

impl From<HostWorkUsage> for u64 {
    fn from(value: HostWorkUsage) -> Self { value.get() }
}

impl fmt::Display for HostWorkUsage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(formatter) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostWorkLimits([HostWorkLimit; HOST_WORK_DIMENSION_COUNT]);

impl HostWorkLimits {
    pub const fn new(limits: [HostWorkLimit; HOST_WORK_DIMENSION_COUNT]) -> Self { Self(limits) }

    pub const fn uniform(limit: HostWorkLimit) -> Self { Self([limit; HOST_WORK_DIMENSION_COUNT]) }

    pub const fn get(&self, dimension: HostWorkDimension) -> HostWorkLimit {
        self.0[dimension.index()]
    }

    pub fn set(&mut self, dimension: HostWorkDimension, limit: HostWorkLimit) {
        self.0[dimension.index()] = limit;
    }

    pub const fn into_array(self) -> [HostWorkLimit; HOST_WORK_DIMENSION_COUNT] { self.0 }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostWorkUsages([HostWorkUsage; HOST_WORK_DIMENSION_COUNT]);

impl HostWorkUsages {
    pub const ZERO: Self = Self([HostWorkUsage::ZERO; HOST_WORK_DIMENSION_COUNT]);

    pub const fn new(usages: [HostWorkUsage; HOST_WORK_DIMENSION_COUNT]) -> Self { Self(usages) }

    pub const fn get(&self, dimension: HostWorkDimension) -> HostWorkUsage {
        self.0[dimension.index()]
    }

    pub const fn into_array(self) -> [HostWorkUsage; HOST_WORK_DIMENSION_COUNT] { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostWorkReservation {
    pub dimension: HostWorkDimension,
    pub usage_before: HostWorkUsage,
    pub requested: HostWorkUnits,
    pub usage_after: HostWorkUsage,
}

impl HostWorkReservation {
    pub const fn phase(self) -> HostWorkPhase { self.dimension.phase() }
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error
)]
pub enum HostWorkReservationError {
    #[error(
        "host work usage overflow in {dimension}: usage {usage}, requested {requested}, limit {limit}"
    )]
    UsageOverflow {
        dimension: HostWorkDimension,
        limit: HostWorkLimit,
        usage: HostWorkUsage,
        requested: HostWorkUnits,
    },
    #[error(
        "host work limit exceeded in {dimension}: usage {usage}, requested {requested}, limit {limit}"
    )]
    LimitExceeded {
        dimension: HostWorkDimension,
        limit: HostWorkLimit,
        usage: HostWorkUsage,
        requested: HostWorkUnits,
    },
    #[error("host work budget is rejected for {dimension}: usage {usage}, requested {requested}")]
    BudgetRejected {
        dimension: HostWorkDimension,
        usage: HostWorkUsage,
        requested: HostWorkUnits,
    },
}

impl HostWorkReservationError {
    pub const fn dimension(self) -> HostWorkDimension {
        match self {
            Self::UsageOverflow { dimension, .. }
            | Self::LimitExceeded { dimension, .. }
            | Self::BudgetRejected { dimension, .. } => dimension,
        }
    }

    pub const fn phase(self) -> HostWorkPhase { self.dimension().phase() }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn phase_indices_cover_each_phase_once() {
        let mut found = [false; HOST_WORK_PHASE_COUNT];
        for phase in HostWorkPhase::ALL {
            assert!(!found[phase.index()]);
            found[phase.index()] = true;
        }
        assert!(found.into_iter().all(|present| present));
    }

    #[test]
    fn dimensions_cover_each_index_once_and_map_to_the_required_phase() {
        let expected = [
            HostWorkPhase::StructuralAdmission,
            HostWorkPhase::StructuralAdmission,
            HostWorkPhase::PureReduction,
            HostWorkPhase::PureReduction,
            HostWorkPhase::PrimitiveEvaluation,
            HostWorkPhase::PrimitiveEvaluation,
            HostWorkPhase::Substitution,
            HostWorkPhase::Substitution,
            HostWorkPhase::AuthorityDiscovery,
            HostWorkPhase::AuthorityDiscovery,
            HostWorkPhase::PhysicalSearch,
            HostWorkPhase::PhysicalSearch,
            HostWorkPhase::WitnessDecoding,
            HostWorkPhase::WitnessDecoding,
            HostWorkPhase::WitnessVerification,
            HostWorkPhase::WitnessVerification,
        ];
        let mut found = [false; HOST_WORK_DIMENSION_COUNT];

        for dimension in HostWorkDimension::ALL {
            assert!(!found[dimension.index()]);
            found[dimension.index()] = true;
            assert_eq!(dimension.phase(), expected[dimension.index()]);
        }
        assert!(found.into_iter().all(|present| present));
    }

    #[test]
    fn limits_and_usages_are_dimension_specific() {
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(10));
        limits.set(HostWorkDimension::SearchStateBytes, HostWorkLimit::new(3));
        let mut usages = HostWorkUsages::ZERO.into_array();
        usages[HostWorkDimension::WitnessBytes.index()] = HostWorkUsage::new(2);
        let usages = HostWorkUsages::new(usages);

        assert_eq!(
            limits.get(HostWorkDimension::SearchStateBytes),
            HostWorkLimit::new(3)
        );
        assert_eq!(
            limits.get(HostWorkDimension::SearchCandidates),
            HostWorkLimit::new(10)
        );
        assert_eq!(
            usages.get(HostWorkDimension::WitnessBytes),
            HostWorkUsage::new(2)
        );
        assert_eq!(
            usages.get(HostWorkDimension::WitnessFields),
            HostWorkUsage::ZERO
        );
    }

    #[test]
    fn accepted_reservation_reports_dimension_and_phase() {
        let reservation = HostWorkUsage::new(4)
            .checked_reserve(
                HostWorkDimension::SubstitutionBindings,
                HostWorkLimit::new(9),
                HostWorkUnits::new(3),
            )
            .unwrap();

        assert_eq!(
            reservation.dimension,
            HostWorkDimension::SubstitutionBindings
        );
        assert_eq!(reservation.phase(), HostWorkPhase::Substitution);
        assert_eq!(reservation.usage_before, HostWorkUsage::new(4));
        assert_eq!(reservation.requested, HostWorkUnits::new(3));
        assert_eq!(reservation.usage_after, HostWorkUsage::new(7));
    }

    #[test]
    fn exceeding_limit_is_typed_without_fabricating_usage() {
        let error = HostWorkUsage::new(4)
            .checked_reserve(
                HostWorkDimension::AuthorityNodes,
                HostWorkLimit::new(5),
                HostWorkUnits::new(2),
            )
            .unwrap_err();

        assert!(matches!(error, HostWorkReservationError::LimitExceeded {
            usage: HostWorkUsage(4),
            ..
        }));
        assert_eq!(error.dimension(), HostWorkDimension::AuthorityNodes);
        assert_eq!(error.phase(), HostWorkPhase::AuthorityDiscovery);
    }

    #[test]
    fn arithmetic_overflow_is_distinct_from_limit_exhaustion() {
        let error = HostWorkUsage::new(u64::MAX - 1)
            .checked_reserve(
                HostWorkDimension::VerificationOperations,
                HostWorkLimit::new(u64::MAX),
                HostWorkUnits::new(2),
            )
            .unwrap_err();

        assert!(matches!(
            error,
            HostWorkReservationError::UsageOverflow {
                usage: HostWorkUsage(value),
                ..
            } if value == u64::MAX - 1
        ));
        assert_eq!(error.phase(), HostWorkPhase::WitnessVerification);
    }

    proptest! {
        #[test]
        fn accepted_usage_is_the_exact_checked_sum(
            limit in any::<u64>(),
            requests in proptest::collection::vec(0_u64..=1_000_000, 0..24),
        ) {
            let mut usage = HostWorkUsage::ZERO;
            let mut accepted = 0_u64;
            for requested in requests {
                let transition = usage.checked_reserve(
                    HostWorkDimension::ReductionSteps,
                    HostWorkLimit::new(limit),
                    HostWorkUnits::new(requested),
                );
                match transition {
                    Ok(reservation) => {
                        accepted = accepted.checked_add(requested).unwrap();
                        usage = reservation.usage_after;
                    }
                    Err(_) => break,
                }
            }
            prop_assert_eq!(usage.get(), accepted);
            prop_assert!(usage.get() <= limit);
        }
    }
}
