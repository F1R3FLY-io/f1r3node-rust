use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use super::deploy_envelope::DeployEnvelopeLimits;
use super::native_cost_evidence::{NativeFundingCaseLimits, NativePrepaidDeltaLimits};
use super::phlo_controls::PhloControlsLimits;
use super::phlo_intent::PhloFundingIntentLimits;
use super::phlo_wire::PhloWireLimits;
use super::signed_phlo_deploy::FundedDeployLimits;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProtocolCap {
    FundingSearchScans,
    AuthoritySearchNodes,
}

impl ProtocolCap {
    pub const ALL: [ProtocolCap; 2] = [
        ProtocolCap::FundingSearchScans,
        ProtocolCap::AuthoritySearchNodes,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ProtocolCap::FundingSearchScans => "funding-search-scans",
            ProtocolCap::AuthoritySearchNodes => "authority-search-nodes",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|cap| cap.name() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ProtocolCaps {
    pub funding_search_scans: usize,
    pub authority_search_nodes: usize,
}

impl Default for ProtocolCaps {
    fn default() -> Self { OFFERED_FUNDED_V6_CAPS }
}

impl ProtocolCaps {
    pub fn value(&self, cap: ProtocolCap) -> usize {
        match cap {
            ProtocolCap::FundingSearchScans => self.funding_search_scans,
            ProtocolCap::AuthoritySearchNodes => self.authority_search_nodes,
        }
    }

    pub fn check(&self) -> Result<(), ProtocolCap> {
        match ProtocolCap::ALL
            .into_iter()
            .find(|cap| self.value(*cap) == 0)
        {
            Some(cap) => Err(cap),
            None => Ok(()),
        }
    }
}

pub const OFFERED_FUNDED_V6_CAPS: ProtocolCaps = ProtocolCaps {
    funding_search_scans: 268_435_456,
    authority_search_nodes: 268_435_456,
};

static FUNDING_SEARCH_SCANS: AtomicUsize =
    AtomicUsize::new(OFFERED_FUNDED_V6_CAPS.funding_search_scans);
static AUTHORITY_SEARCH_NODES: AtomicUsize =
    AtomicUsize::new(OFFERED_FUNDED_V6_CAPS.authority_search_nodes);

pub fn set_protocol_caps(caps: ProtocolCaps) -> Result<(), ProtocolCap> {
    caps.check()?;
    FUNDING_SEARCH_SCANS.store(caps.funding_search_scans, Ordering::Relaxed);
    AUTHORITY_SEARCH_NODES.store(caps.authority_search_nodes, Ordering::Relaxed);
    Ok(())
}

pub fn protocol_caps() -> ProtocolCaps {
    ProtocolCaps {
        funding_search_scans: FUNDING_SEARCH_SCANS.load(Ordering::Relaxed),
        authority_search_nodes: AUTHORITY_SEARCH_NODES.load(Ordering::Relaxed),
    }
}

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
    fn caps_read_from_configuration_with_defaults_and_reject_zero() {
        let partial: ProtocolCaps =
            serde_json::from_str(r#"{"funding-search-scans": 40000000}"#).expect("a partial block");
        assert_eq!(partial.funding_search_scans, 40_000_000);
        assert_eq!(
            partial.authority_search_nodes,
            OFFERED_FUNDED_V6_CAPS.authority_search_nodes
        );
        assert_eq!(
            serde_json::from_str::<ProtocolCaps>("{}").expect("an empty block"),
            ProtocolCaps::default()
        );
        let zero = ProtocolCaps {
            authority_search_nodes: 0,
            ..partial
        };
        assert_eq!(zero.check(), Err(ProtocolCap::AuthoritySearchNodes));
        assert_eq!(partial.check(), Ok(()));
    }

    #[test]
    fn each_cap_has_one_name_and_a_positive_value() {
        for cap in ProtocolCap::ALL {
            assert_eq!(ProtocolCap::from_name(cap.name()), Some(cap));
            assert!(OFFERED_FUNDED_V6_CAPS.value(cap) > 0);
        }
        assert_eq!(ProtocolCap::from_name("host-work"), None);
    }
}
