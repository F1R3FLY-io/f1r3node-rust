//! Upper-application composition; node core remains independent of MeTTaIL.

use std::sync::Arc;

use mettail_rholang_runtime::guard_discharge::LoweringOptions;
use mettail_rholang_runtime::guard_par_substrate::SubstrateGuardMatcher;
use mettail_rholang_runtime::language_install::{
    language_runtime_definitions, EmptyRegistrySnapshot, LanguageInstallPolicy,
    LanguageInstallService, RegistrySnapshot, RholangLanguageRuntime,
    LANGUAGE_CAPABILITY_ABI_CURRENT,
};
use mettail_rholang_runtime::rholang_ast::{RholangPreparationPolicy, RholangProgramFrontend};
use mettail_rholang_runtime::{EmptyFltResolver, LanguageRights, RuntimePolicy};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::frontend::ProgramFrontend;
use rholang::rust::interpreter::rho_runtime::validate_extra_system_processes;
use rholang::rust::interpreter::system_processes::Definition;

pub(crate) fn require_standalone(standalone: bool) -> Result<(), String> {
    if standalone {
        Ok(())
    } else {
        Err("F1R3Lang evaluation currently requires standalone mode; consensus source routes are not activated".into())
    }
}

/// Do not feed peer-provided blocks into unactivated Casper source execution.
pub(crate) struct UnactivatedCasperPackets;

#[async_trait::async_trait]
impl comm::rust::p2p::packet_handler::PacketHandler for UnactivatedCasperPackets {
    async fn handle_packet(
        &self,
        _peer: &comm::rust::peer_node::PeerNode,
        _packet: &models::routing::Packet,
    ) -> Result<(), comm::rust::errors::CommError> {
        Err(comm::rust::errors::unknown_protocol(
            "Casper peer execution is not activated for F1R3Lang evaluation".into(),
        ))
    }
}

/// One funded public route and the matcher belonging to its installed services.
#[derive(Clone)]
pub struct F1r3langEvaluation {
    pub(crate) frontend: Arc<dyn ProgramFrontend>,
    pub(crate) max_source_bytes: usize,
    pub(crate) funding: Cost,
    pub(crate) matcher: SubstrateGuardMatcher,
}

pub struct F1r3langComposition {
    pub definitions: Vec<Definition>,
    pub evaluation: F1r3langEvaluation,
}

impl F1r3langComposition {
    /// Explicit startup policy. Missing or invalid limits disable startup, not meters.
    /// This initial route supports inline modules; registry reads remain unavailable.
    pub fn from_environment() -> Result<Self, String> {
        let read = |name: &str| std::env::var(name).ok();
        let (preparation, funding) = host_policy(&read)?;
        Self::new(
            Arc::new(EmptyRegistrySnapshot),
            LanguageInstallPolicy::new(
                LanguageRights::native_flt_default(),
                RuntimePolicy::default(),
                LANGUAGE_CAPABILITY_ABI_CURRENT,
            ),
            preparation,
            funding,
        )
    }

    /// The snapshot and installer policy are supplied by the host, not source text.
    pub fn new(
        snapshot: Arc<dyn RegistrySnapshot>,
        policy: LanguageInstallPolicy,
        preparation: RholangPreparationPolicy,
        funding: i64,
    ) -> Result<Self, String> {
        if funding <= 0 || funding == i64::MAX {
            return Err("F1R3Lang requires a positive finite host-funded evaluation budget".into());
        }
        let service = Arc::new(LanguageInstallService::new(snapshot, policy));
        let runtime = Arc::new(RholangLanguageRuntime::new(service));
        let definitions = language_runtime_definitions(runtime.clone());
        validate_extra_system_processes(&definitions)?;
        let matcher = SubstrateGuardMatcher::with_language_runtime(runtime);
        Ok(Self {
            definitions,
            evaluation: F1r3langEvaluation {
                max_source_bytes: preparation.max_source_bytes,
                frontend: Arc::new(RholangProgramFrontend::new(
                    preparation,
                    Arc::new(EmptyFltResolver),
                )),
                funding: Cost::create(funding, "F1R3Lang host evaluation policy"),
                matcher,
            },
        })
    }
}

fn positive(name: &str, read: &impl Fn(&str) -> Option<String>) -> Result<u64, String> {
    let value = read(name)
        .ok_or_else(|| format!("Required host policy {name} is not configured"))?
        .parse::<u64>()
        .map_err(|_| format!("Host policy {name} must be a positive integer"))?;
    if value == 0 {
        return Err(format!("Host policy {name} must be positive"));
    }
    Ok(value)
}

fn host_policy(
    read: &impl Fn(&str) -> Option<String>,
) -> Result<(RholangPreparationPolicy, i64), String> {
    let size = |name| {
        usize::try_from(positive(name, read)?)
            .map_err(|_| format!("Host policy {name} exceeds this platform's size limit"))
    };
    let funding = i64::try_from(positive("F1R3LANG_EVAL_PHLO", read)?)
        .map_err(|_| "F1R3LANG_EVAL_PHLO exceeds the signed budget domain".to_owned())?;
    if funding == i64::MAX {
        return Err("F1R3LANG_EVAL_PHLO may not select the unlimited sentinel".into());
    }
    Ok((
        RholangPreparationPolicy {
            max_source_bytes: size("F1R3LANG_MAX_SOURCE_BYTES")?,
            max_import_entries: size("F1R3LANG_MAX_IMPORT_ENTRIES")?,
            max_import_nodes: size("F1R3LANG_MAX_IMPORT_NODES")?,
            max_import_payload_bytes: size("F1R3LANG_MAX_IMPORT_BYTES")?,
            max_preparation_work: positive("F1R3LANG_PREPARATION_WORK", read)?,
            max_preparation_units: size("F1R3LANG_PREPARATION_UNITS")?,
            lowering: LoweringOptions::NO_DISCHARGE,
        },
        funding,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_mode_is_rejected_before_node_setup() {
        assert!(require_standalone(false).is_err());
        assert!(require_standalone(true).is_ok());
    }

    #[tokio::test]
    async fn peer_packets_refuse_without_decoding_or_legacy_execution() {
        use comm::rust::p2p::packet_handler::PacketHandler;
        use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
        let peer = PeerNode {
            id: NodeIdentifier::new("00".into()),
            endpoint: Endpoint::new("127.0.0.1".into(), 1, 2),
        };
        assert!(UnactivatedCasperPackets
            .handle_packet(&peer, &models::routing::Packet::default())
            .await
            .is_err());
    }

    #[test]
    fn missing_negative_zero_overflow_and_unlimited_funding_refuse() {
        assert!(host_policy(&|_| None).is_err());
        for invalid in ["-1", "0", "9223372036854775807", "18446744073709551615"] {
            assert!(host_policy(&|name| Some(if name == "F1R3LANG_EVAL_PHLO" {
                invalid.into()
            } else {
                "100".into()
            }))
            .is_err());
        }
    }

    #[test]
    fn host_funding_and_preparation_limits_are_retained_exactly() {
        let (policy, funding) = host_policy(&|_| Some("1234".into())).unwrap();
        assert_eq!(funding, 1234);
        assert_eq!(policy.max_source_bytes, 1234);
        assert_eq!(policy.max_preparation_work, 1234);
        assert_eq!(policy.max_preparation_units, 1234);
        assert_eq!(policy.lowering, LoweringOptions::NO_DISCHARGE);
    }

    #[test]
    fn provider_definitions_pass_real_node_registration_checks() {
        let (policy, funding) = host_policy(&|_| Some("1234".into())).unwrap();
        let composition = F1r3langComposition::new(
            Arc::new(EmptyRegistrySnapshot),
            LanguageInstallPolicy::new(
                LanguageRights::native_flt_default(),
                RuntimePolicy::default(),
                LANGUAGE_CAPABILITY_ABI_CURRENT,
            ),
            policy,
            funding,
        )
        .unwrap();
        assert!(!composition.definitions.is_empty());
        validate_extra_system_processes(&composition.definitions).unwrap();
        assert_eq!(composition.evaluation.funding.value, funding);
    }
}
