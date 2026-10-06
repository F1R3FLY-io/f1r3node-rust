use std::num::NonZeroUsize;

use super::deploy_envelope::DeployEnvelopeLimits;
use super::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
use super::native_cost_evidence::{NativeFundingCaseLimits, NativePrepaidDeltaLimits};
use super::phlo_controls::PhloControlsLimits;
use super::phlo_intent::PhloFundingIntentLimits;
use super::phlo_wire::PhloWireLimits;
use super::signed_phlo_deploy::FundedDeployLimits;

#[derive(Clone, Copy, Debug)]
pub struct OfferedFundedProtocolLimits {
    pub envelope: DeployEnvelopeLimits,
    pub evidence: PhloWireLimits,
    pub funding_case: NativeFundingCaseLimits,
    pub prepaid_delta: NativePrepaidDeltaLimits,
    pub deploy_log_events: usize,
    pub deploy_log_items: usize,
    pub deploy_log_bytes: usize,
}

pub fn offered_funded_v6_limits() -> OfferedFundedProtocolLimits {
    let controls = PhloControlsLimits {
        wire: PhloWireLimits {
            total_bytes: 65_536,
            field_bytes: 32_768,
        },
        owners: 64,
        schedules: 4,
        total_classes: 32,
    };
    let funding = PhloFundingIntentLimits {
        wire: PhloWireLimits {
            total_bytes: 262_144,
            field_bytes: 131_072,
        },
        controls,
        sources: 64,
        resource_permissions: 1_024,
        authority_nodes: 4_096,
    };
    OfferedFundedProtocolLimits {
        envelope: DeployEnvelopeLimits {
            payload: FundedDeployLimits {
                deploy_bytes: 1_048_576,
                signing: PhloWireLimits {
                    total_bytes: 524_288,
                    field_bytes: 262_144,
                },
                funding,
            },
            members: NonZeroUsize::new(64).expect("positive protocol member limit"),
        },
        evidence: PhloWireLimits {
            total_bytes: 4_194_304,
            field_bytes: 1_048_576,
        },
        funding_case: NativeFundingCaseLimits {
            wire: PhloWireLimits {
                total_bytes: 1_048_576,
                field_bytes: 262_144,
            },
            sources: 64,
            obligations: 4_096,
            cells: 65_536,
            custody_bytes: 131_072,
            key_bytes: 262_144,
        },
        prepaid_delta: NativePrepaidDeltaLimits {
            wire: PhloWireLimits {
                total_bytes: 1_048_576,
                field_bytes: 262_144,
            },
            draws: 4_096,
            positions: 4_096,
            births: 4_096,
            replacements: 4_096,
        },
        deploy_log_events: 65_536,
        deploy_log_items: 262_144,
        deploy_log_bytes: 8_388_608,
    }
}

pub fn offered_funded_v6_host_work_limits() -> HostWorkLimits {
    let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(268_435_456));
    limits.set(
        HostWorkDimension::SearchStateBytes,
        HostWorkLimit::new(134_217_728),
    );
    limits.set(
        HostWorkDimension::AuthorityDepth,
        HostWorkLimit::new(1_048_576),
    );
    limits.set(
        HostWorkDimension::VerificationOperations,
        HostWorkLimit::new(1_073_741_824),
    );
    limits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offered_limits_bound_each_nested_wire_below_its_parent() {
        let limits = offered_funded_v6_limits();
        let funding = limits.envelope.payload.funding;
        assert!(limits.envelope.payload.deploy_bytes >= funding.wire.total_bytes);
        assert!(limits.envelope.payload.signing.total_bytes >= funding.wire.total_bytes);
        assert!(funding.wire.field_bytes >= funding.controls.wire.total_bytes);
        assert!(funding.controls.wire.field_bytes <= funding.controls.wire.total_bytes);
        assert_eq!(limits.envelope.members.get(), funding.sources);
        assert!(limits.evidence.total_bytes >= limits.evidence.field_bytes);
        assert!(limits.evidence.field_bytes >= limits.funding_case.wire.total_bytes);
        assert!(limits.evidence.field_bytes >= limits.prepaid_delta.wire.total_bytes);
        assert!(limits.deploy_log_events <= limits.deploy_log_items);
        assert!(limits.deploy_log_bytes >= limits.evidence.total_bytes);
        assert!(
            limits.funding_case.cells
                <= limits.funding_case.sources * limits.funding_case.obligations
        );
    }

    #[test]
    fn offered_host_work_profile_is_finite_and_stricter_for_state_and_depth() {
        let limits = offered_funded_v6_host_work_limits();
        assert!(limits
            .into_array()
            .iter()
            .all(|limit| limit.get() > 0 && limit.get() < u64::MAX));
        assert!(
            limits.get(HostWorkDimension::SearchStateBytes).get()
                < limits.get(HostWorkDimension::VerificationOperations).get()
        );
        assert!(
            limits.get(HostWorkDimension::AuthorityDepth).get()
                > offered_funded_v6_limits()
                    .envelope
                    .payload
                    .funding
                    .authority_nodes as u64
        );
    }
}
