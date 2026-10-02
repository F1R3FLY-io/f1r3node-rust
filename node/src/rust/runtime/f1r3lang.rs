//! Upper-application composition; node core remains independent of MeTTaIL.

use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use mettail_rholang_runtime::guard_discharge::LoweringOptions;
use mettail_rholang_runtime::guard_par_substrate::SubstrateGuardMatcher;
use mettail_rholang_runtime::language_install::{
    language_runtime_definitions, EmptyRegistrySnapshot, LanguageInstallPolicy,
    LanguageInstallService, RegistrySnapshot, RholangLanguageRuntime,
    LANGUAGE_CAPABILITY_ABI_CURRENT,
};
use mettail_rholang_runtime::rholang_ast::{RholangPreparationPolicy, RholangProgramFrontend};
use mettail_rholang_runtime::{EmptyFltResolver, LanguageRight, LanguageRights, RuntimePolicy};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::frontend::{PreparedProgram, ProgramFrontend};
use rholang::rust::interpreter::rho_runtime::{
    bootstrap_registry, validate_extra_system_processes, RhoRuntime, RhoRuntimeImpl,
};
use rholang::rust::interpreter::system_processes::{Definition, FixedChannels};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EvalRegistryState {
    Fresh,
    Bootstrapped,
    Installed,
}

fn classify_eval_registry(
    shapes: &[Vec<(bool, usize, usize)>; 5],
) -> Result<EvalRegistryState, String> {
    const FORWARDER: &[(bool, usize, usize)] = &[(false, 1, 1)];
    const INTERNAL_API: &[(bool, usize, usize)] =
        &[(true, 1, 4), (true, 1, 5), (true, 1, 5), (true, 1, 6)];
    const PUBLIC_API: &[(bool, usize, usize)] = &[(true, 1, 2)];

    if shapes.iter().all(Vec::is_empty) {
        return Ok(EvalRegistryState::Fresh);
    }
    if shapes.iter().all(|shape| shape.as_slice() == FORWARDER) {
        return Ok(EvalRegistryState::Bootstrapped);
    }
    if shapes[..3]
        .iter()
        .all(|shape| shape.as_slice() == FORWARDER)
        && shapes[3].as_slice() == INTERNAL_API
        && shapes[4].as_slice() == PUBLIC_API
    {
        return Ok(EvalRegistryState::Installed);
    }
    Err("eval registry contains a partial or unexpected fixed-channel installation".into())
}

async fn observe_eval_registry(runtime: &RhoRuntimeImpl) -> [Vec<(bool, usize, usize)>; 5] {
    let channels = [
        FixedChannels::reg_lookup(),
        FixedChannels::reg_insert_random(),
        FixedChannels::reg_insert_signed(),
        FixedChannels::reg_v1_internal(),
        FixedChannels::reg_v1(),
    ];
    let mut shapes: [Vec<(bool, usize, usize)>; 5] = std::array::from_fn(|_| Vec::new());
    for (shape, channel) in shapes.iter_mut().zip(channels) {
        *shape = runtime
            .get_continuations(vec![channel])
            .await
            .into_iter()
            .map(|continuation| {
                (
                    continuation.persist,
                    continuation.patterns.len(),
                    continuation
                        .patterns
                        .first()
                        .map_or(0, |binding| binding.patterns.len()),
                )
            })
            .collect();
        shape.sort_unstable();
    }
    shapes
}

pub(crate) async fn ensure_eval_registry(runtime: &mut RhoRuntimeImpl) -> Result<(), String> {
    let initial = classify_eval_registry(&observe_eval_registry(runtime).await)?;
    if initial == EvalRegistryState::Installed {
        return Ok(());
    }

    let compiled = rholang::rust::build::compile_rholang_source::CompiledRholangSource::new(
        casper::rust::genesis::contracts::embedded_rho::VERSIONED_REGISTRY.to_owned(),
        std::collections::HashMap::new(),
        "VersionedRegistry.rho".to_owned(),
    )
    .map_err(|error| format!("trusted versioned registry compilation failed: {error}"))?;

    let checkpoint = runtime.create_soft_checkpoint().await;
    if initial == EvalRegistryState::Fresh {
        bootstrap_registry(runtime).await;
        if classify_eval_registry(&observe_eval_registry(runtime).await)
            != Ok(EvalRegistryState::Bootstrapped)
        {
            runtime.revert_to_soft_checkpoint(checkpoint).await;
            return Err("trusted eval registry bootstrap did not install all forwarders".into());
        }
    }

    let evaluated = runtime
        .evaluate_prepared(
            PreparedProgram::from_normalized(compiled.term),
            Cost::unsafe_max(),
            Blake2b512Random::create_from_length(128),
        )
        .await;
    let result = match evaluated {
        Ok(result) => result,
        Err(error) => {
            runtime.revert_to_soft_checkpoint(checkpoint).await;
            return Err(format!(
                "trusted versioned registry execution failed: {error}"
            ));
        }
    };
    let installed_shapes = observe_eval_registry(runtime).await;
    if !result.errors.is_empty()
        || classify_eval_registry(&installed_shapes) != Ok(EvalRegistryState::Installed)
    {
        runtime.revert_to_soft_checkpoint(checkpoint).await;
        return Err(format!(
            "trusted versioned registry installation failed: errors={:?}, channel_shapes={installed_shapes:?}",
            result.errors,
        ));
    }
    runtime.create_checkpoint().await;
    Ok(())
}

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

/// Public inline languages retain the ordinary guest-right ceiling. The
/// compiled Rholang Boolean host profile has a distinct, explicit Bridge
/// grant; source DDL cannot acquire that guest right through this choice.
pub(crate) fn inline_language_install_policy() -> LanguageInstallPolicy {
    LanguageInstallPolicy::new(
        LanguageRights::native_flt_default(),
        RuntimePolicy::default(),
        LANGUAGE_CAPABILITY_ABI_CURRENT,
    )
    .with_host_profile_grants(LanguageRights::from_rights([LanguageRight::Bridge]))
}

impl F1r3langComposition {
    /// Explicit startup policy. Missing or invalid limits disable startup, not meters.
    /// This initial route supports inline modules; registry reads remain unavailable.
    pub fn from_environment() -> Result<Self, String> {
        let read = |name: &str| std::env::var(name).ok();
        let (preparation, funding) = host_policy(&read)?;
        Self::new(
            Arc::new(EmptyRegistrySnapshot),
            inline_language_install_policy(),
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
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rholang::rust::interpreter::rho_runtime::create_runtime_from_kv_store;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;

    async fn test_eval_runtime(stores: &mut InMemoryStoreManager) -> RhoRuntimeImpl {
        create_runtime_from_kv_store(
            stores.eval_stores().await.unwrap(),
            Arc::new(std::collections::HashMap::new()),
            false,
            &mut Vec::new(),
            Arc::new(Box::new(SubstrateGuardMatcher::new())),
            ExternalServices::noop(),
        )
        .await
    }

    #[test]
    fn eval_registry_startup_classification_refuses_partial_states() {
        const FORWARDER: (bool, usize, usize) = (false, 1, 1);
        let fresh = std::array::from_fn(|_| Vec::new());
        assert_eq!(classify_eval_registry(&fresh), Ok(EvalRegistryState::Fresh));

        let bootstrapped = std::array::from_fn(|_| vec![FORWARDER]);
        assert_eq!(
            classify_eval_registry(&bootstrapped),
            Ok(EvalRegistryState::Bootstrapped)
        );

        let installed = [
            vec![FORWARDER],
            vec![FORWARDER],
            vec![FORWARDER],
            vec![(true, 1, 4), (true, 1, 5), (true, 1, 5), (true, 1, 6)],
            vec![(true, 1, 2)],
        ];
        assert_eq!(
            classify_eval_registry(&installed),
            Ok(EvalRegistryState::Installed)
        );

        let partial = [
            vec![FORWARDER],
            vec![FORWARDER],
            vec![FORWARDER],
            vec![FORWARDER],
            vec![],
        ];
        assert!(classify_eval_registry(&partial).is_err());
    }

    #[tokio::test]
    async fn eval_registry_installs_once_and_reuses_committed_state_after_restart() {
        let mut stores = InMemoryStoreManager::new();
        let mut runtime = test_eval_runtime(&mut stores).await;
        assert_eq!(
            classify_eval_registry(&observe_eval_registry(&runtime).await),
            Ok(EvalRegistryState::Fresh)
        );
        ensure_eval_registry(&mut runtime).await.unwrap();
        let root = runtime.get_root().await;
        assert_eq!(
            classify_eval_registry(&observe_eval_registry(&runtime).await),
            Ok(EvalRegistryState::Installed)
        );
        ensure_eval_registry(&mut runtime).await.unwrap();
        assert_eq!(runtime.get_root().await, root);
        drop(runtime);

        let mut restarted = test_eval_runtime(&mut stores).await;
        assert_eq!(
            classify_eval_registry(&observe_eval_registry(&restarted).await),
            Ok(EvalRegistryState::Installed)
        );
        ensure_eval_registry(&mut restarted).await.unwrap();
        assert_eq!(restarted.get_root().await, root);
    }

    #[tokio::test]
    async fn eval_registry_recovers_complete_bootstrap_without_duplicating_forwarders() {
        let mut stores = InMemoryStoreManager::new();
        let mut runtime = test_eval_runtime(&mut stores).await;
        bootstrap_registry(&runtime).await;
        runtime.create_checkpoint().await;
        assert_eq!(
            classify_eval_registry(&observe_eval_registry(&runtime).await),
            Ok(EvalRegistryState::Bootstrapped)
        );
        drop(runtime);

        let mut restarted = test_eval_runtime(&mut stores).await;
        ensure_eval_registry(&mut restarted).await.unwrap();
        assert_eq!(
            classify_eval_registry(&observe_eval_registry(&restarted).await),
            Ok(EvalRegistryState::Installed)
        );
    }

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
