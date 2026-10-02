use std::sync::Arc;

use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Cosigned;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_grant_creation::{
    PhloGrantCreationLimits, PhloGrantCreationTerms, PhloGrantCreationV1, VerifiedGrantCreationV1,
};
use models::rust::phlo_intent::{
    PhloConversionCompositionV2, PhloFundingGrantUseV2, PhloFundingIntentV1, PhloFundingIntentV2,
    PhloFundingIntentV2Limits,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloResourceClassV1, PhloScheduleV1};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::phlo_wire::PhloWireLimits;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
use rholang::rust::interpreter::accounting::phlo_execution::{PhloExecutionLimits, PhloResource};
use rholang::rust::interpreter::accounting::SignatureChannel;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::*;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::costacc::offered_grants::OfferedGrantLimits;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[test]
fn private_purse_requires_only_its_own_resource_permissions_and_no_fee() {
    let name = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![7; 32] })),
    }]);
    let signature = CostSignature {
        value: Some(Value::Name(name)),
    };
    let payer = vault_payer(&signature).unwrap();
    let runtime = cost_signature_to_sig(&signature).unwrap();
    let location = SignatureChannel::from_sig(&runtime).par.encode_to_vec();
    let other_location = b"unrelated".to_vec();
    let key_limits = PhloExecutionLimits {
        resource_entries: 1,
        authority_nodes: 8,
        key_bytes: 4096,
    };
    let own = PhloResource {
        location: &location,
        class: 0,
        acquisition_terms: b"terms",
        authority: &runtime,
    }
    .wire_key(key_limits)
    .unwrap();
    let other = PhloResource {
        location: &other_location,
        class: 0,
        acquisition_terms: b"terms",
        authority: &runtime,
    }
    .wire_key(key_limits)
    .unwrap();
    let limits = PhloSourceLimits {
        wire: PhloWireLimits {
            total_bytes: 4096,
            field_bytes: 2048,
        },
        resource_permissions: 2,
        authority_nodes: 16,
    };
    let own_only = PhloSourcePolicyV1::new(
        &payer.custody_key,
        100,
        100,
        false,
        vec![own.clone()],
        limits,
    )
    .unwrap();
    assert_eq!(private_name_payer(&own_only).unwrap(), Some(payer.clone()));
    let fee_enabled = PhloSourcePolicyV1::new(
        &payer.custody_key,
        100,
        100,
        true,
        vec![own.clone()],
        limits,
    )
    .unwrap();
    assert!(matches!(
        private_name_payer(&fee_enabled),
        Err(DirectWalletFundingError::OnchainFee)
    ));
    let extra = PhloSourcePolicyV1::new(
        &payer.custody_key,
        100,
        100,
        false,
        vec![own, other],
        limits,
    )
    .unwrap();
    assert!(matches!(
        private_name_payer(&extra),
        Err(DirectWalletFundingError::AmbiguousOnchainAuthority)
    ));
}

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(10_000_000)))
}

fn public_key(byte: u8) -> [u8; 65] {
    Secp256k1
        .to_public(&PrivateKey::from_bytes(&[byte; 32]))
        .bytes
        .as_ref()
        .try_into()
        .unwrap()
}

fn schedule() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: vec![PhloResourceClassV1 {
            identity: b"compute",
            measurement_unit: b"unit",
            measurement_rule: [1; 32],
            valuation_rule: [2; 32],
            weight: 1,
        }],
        actual_price: 2,
        compatibility_rule: [3; 32],
    }
}

fn offered(custody: &[u8], time_stamp: i64) -> Cosigned<OfferedFundedDeploy> {
    let limits = offered_funded_v6_limits().envelope.payload;
    let selected = schedule();
    let commitment = selected.digest(PhloGenesisPolicy::LIMITS).unwrap();
    let source = PhloSourcePolicyV1::new(custody, 100, 100, true, vec![], PhloSourceLimits {
        wire: limits.funding.wire,
        resource_permissions: 0,
        authority_nodes: 0,
    })
    .unwrap();
    let intent = PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: 10,
                price_ceiling: 2,
                required_owner_ceilings: vec![2],
                permitted_schedules: vec![selected],
            },
            schedule_commitment: commitment,
            total_exposure: 100,
            sources: vec![source],
        },
        grant_uses: vec![PhloFundingGrantUseV2 {
            source_index: 0,
            grant_id: b"grant",
            authority_version: 7,
            operation_id: [5; 32],
            max_draw: 10,
            cumulative_ceiling: 100,
            valid_from: Some(10),
            valid_until: Some(20),
        }],
        conversion: PhloConversionCompositionV2::NoConversion,
    };
    let v2_limits = PhloFundingIntentV2Limits {
        wire: limits.funding.wire,
        base: limits.funding,
        grant_uses: limits.funding.wire.total_bytes / 8,
        grant_id_bytes: limits.funding.wire.field_bytes,
        quote_evidence_bytes: limits.funding.wire.field_bytes,
    };
    let body = DeployData {
        term: "Nil".to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let payload =
        OfferedFundedDeploy::new(body, intent.encode(v2_limits).unwrap(), 10, 2, limits).unwrap();
    Cosigned::create_single_envelope(
        payload,
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[2; 32]),
    )
    .unwrap()
}

#[tokio::test]
async fn signed_v2_delegated_source_requires_its_root_and_envelope() {
    let limits = offered_funded_v6_limits();
    let grant_limits = OfferedGrantLimits {
        uses: 4,
        grant_id_bytes: 64,
        batch_bytes: 8192,
    };
    let stores = RSpaceStore {
        history: Arc::new(InMemoryKeyValueStore::new()),
        roots: Arc::new(InMemoryKeyValueStore::new()),
        cold: Arc::new(InMemoryKeyValueStore::new()),
    };
    let (manager, _) = RuntimeManager::create_with_history(
        stores,
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        Arc::new(Default::default()),
        ExternalServices::noop(),
    );
    let mut runtime = RuntimeOps::new(manager.spawn_runtime().await);
    let original: [u8; 32] = runtime
        .runtime
        .create_checkpoint()
        .await
        .root
        .bytes()
        .try_into()
        .unwrap();
    let issuer = public_key(1);
    let principal = principal_ground_v61(&issuer);
    let payer = vault_payer(&CostSignature {
        value: Some(Value::Ground(principal)),
    })
    .unwrap();
    let selected = schedule();
    let commitment = selected.digest(PhloGenesisPolicy::LIMITS).unwrap();
    let creation = PhloGrantCreationV1::new(
        PhloGrantCreationTerms {
            grant_id: b"grant",
            issuer_public_key: &issuer,
            delegate_public_key: &public_key(2),
            custody: payer.custody_key,
            network: b"test",
            shard: b"root",
            schedule_commitment: commitment,
            authority_version: 7,
            max_draw: 10,
            cumulative_ceiling: 100,
            valid_from: Some(10),
            valid_until: Some(20),
            operation_id: [6; 32],
        },
        PhloGrantCreationLimits {
            wire: PhloWireLimits {
                total_bytes: 1024,
                field_bytes: 256,
            },
            grant_id_bytes: 64,
        },
    )
    .unwrap();
    let verified_creation = VerifiedGrantCreationV1::verify(
        Cosigned::create_single_envelope(
            creation,
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap(),
        b"test",
        b"root",
    )
    .unwrap();
    let prepared = manager
        .capture_offered_grant_issuance(original, &verified_creation, grant_limits, &budget())
        .unwrap()
        .prepare_issuance(original, &verified_creation, grant_limits, &budget())
        .unwrap();
    runtime
        .apply_offered_grant_transitions(&prepared, original, grant_limits)
        .await
        .unwrap();
    let root: [u8; 32] = runtime
        .runtime
        .create_checkpoint()
        .await
        .root
        .bytes()
        .try_into()
        .unwrap();
    let offer = offered(&payer.custody_key, 1);
    let snapshot = manager
        .capture_offered_grants_from_signed(root, &offer, grant_limits, &budget())
        .unwrap();
    assert!(snapshot
        .verified_source_payers(original, &offer, 15, grant_limits, &budget())
        .is_err());
    let verified = snapshot
        .verified_source_payers(root, &offer, 15, grant_limits, &budget())
        .unwrap();
    let funding_limits = DirectWalletFundingLimits {
        members: limits.envelope.members,
        funding: limits.envelope.payload.funding,
    };
    assert!(authorize_offered_direct_wallet_funding(&offer, funding_limits).is_err());
    let authorized =
        authorize_offered_direct_wallet_funding_with_grants(&offer, funding_limits, &verified)
            .unwrap();
    assert_eq!(authorized.grant_root(), Some(root));
    assert_eq!(authorized.payers().get(&payer.custody_key), Some(&payer));
    let other_offer = offered(&payer.custody_key, 2);
    assert!(matches!(
        authorize_offered_direct_wallet_funding_with_grants(
            &other_offer,
            funding_limits,
            &verified,
        ),
        Err(DirectWalletFundingError::GrantEnvelopeMismatch)
    ));
}
