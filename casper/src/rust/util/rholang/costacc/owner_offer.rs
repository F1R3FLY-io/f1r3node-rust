// Added by DR-134 (DR-66 item 2): the owner-direct offer of a client.
//
// On protocol 6 a node accepts only offered-funded deploys. A client such as
// `node deploy` signs an owner-direct offer: the signer's own vault funds every
// class of the signer's own principal lane, under the genesis schedule of the
// shard at the signed price. The REV ceiling of the offer is the phlo limit
// times the price plus the fixed fee. The source caps and the total exposure
// equal that ceiling, so the owner consents to the same maximum cost as a dev
// deploy with the same phlo limit and price.

use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Cosigned;
use models::casper::DeployDataProto;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_intent::{
    PhloConversionCompositionV2, PhloFundingIntentV1, PhloFundingIntentV2,
    PhloFundingIntentV2Limits,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleError, PhloScheduleV1};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension,
};
use rholang::rust::interpreter::accounting::phlo_execution::{PhloExecutionLimits, PhloResource};
use rholang::rust::interpreter::accounting::{principal_ground_v61, SignatureChannel};

use crate::rust::errors::CasperError;
use crate::rust::util::rholang::acceptance::FIXED_FEE_REV;
use crate::rust::util::rholang::costacc::vault_payer::vault_payer;

/// The network of the default resource schedule.
pub const DEFAULT_RESOURCE_NETWORK: &[u8] = b"test";

/// The resource schedule of the shipped configurations for `shard`: the native
/// classes compute, introduction, transfer and trace at weight 1, settled in
/// atomic REV at 8 decimals under the native compatibility rule. The schedule
/// has price 0, as a genesis policy stores it.
pub fn default_resource_schedule(shard: &str) -> PhloScheduleV1<'_> {
    let identities: [&[u8]; 4] = [b"compute", b"introduction", b"transfer", b"trace"];
    PhloScheduleV1 {
        protocol_version: 6,
        network: DEFAULT_RESOURCE_NETWORK,
        shard: shard.as_bytes(),
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(identities)
            .map(|(dimension, identity)| dimension.resource_class(identity, 1))
            .collect(),
        actual_price: 0,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

/// The active protocol-6 genesis policy of the default resource schedule. The
/// shipped configurations carry its hexadecimal encoding as `resource-policy`.
pub fn default_resource_policy(shard: &str) -> Result<PhloGenesisPolicy, PhloScheduleError> {
    Ok(
        PhloGenesisPolicy::from_schedule(&default_resource_schedule(shard))?
            .with_offered_funded_v6(models::rust::cost_protocol_limits::OFFERED_FUNDED_V6_CAPS),
    )
}

fn invalid_offer(message: impl std::fmt::Display) -> CasperError {
    CasperError::RuntimeError(format!("owner-direct offer: {message}"))
}

/// The wire form of an owner-direct offer of `body`, signed by `owner`. The
/// offer signs `genesis_schedule` at `phlo_price`, and the owner's vault funds
/// every class of the schedule on the owner's own principal lane.
pub fn owner_direct_offer(
    owner: &PrivateKey,
    body: DeployData,
    phlo_limit: u64,
    phlo_price: u64,
    genesis_schedule: &PhloScheduleV1<'_>,
) -> Result<DeployDataProto, CasperError> {
    let limits = offered_funded_v6_limits().envelope.payload;
    let owner_public = Secp256k1.to_public(owner);
    let signature = CostSignature {
        value: Some(Value::Ground(principal_ground_v61(&owner_public.bytes))),
    };
    let payer = vault_payer(&signature).map_err(invalid_offer)?;
    let schedule = PhloScheduleV1 {
        actual_price: phlo_price,
        ..genesis_schedule.clone()
    };
    let acquisition_terms = schedule
        .encode(PhloGenesisPolicy::LIMITS)
        .map_err(invalid_offer)?;
    let authority = cost_signature_to_sig(&signature).map_err(invalid_offer)?;
    let location = SignatureChannel::from_sig(&authority).par.encode_to_vec();
    let ceiling = u128::from(phlo_limit)
        .checked_mul(u128::from(phlo_price))
        .and_then(|amount| amount.checked_add(FIXED_FEE_REV))
        .ok_or_else(|| invalid_offer("the REV ceiling overflows"))?;
    let source_cap = u64::try_from(ceiling)
        .map_err(|_| invalid_offer("the REV ceiling exceeds a source cap"))?;
    let mut permissions = Vec::with_capacity(schedule.classes.len());
    for class in 0..schedule.classes.len() {
        permissions.push(
            PhloResource {
                location: &location,
                class,
                acquisition_terms: &acquisition_terms,
                authority: &authority,
            }
            .wire_key(PhloExecutionLimits {
                resource_entries: 1,
                authority_nodes: limits.funding.authority_nodes,
                key_bytes: limits.funding.wire.field_bytes,
            })
            .map_err(invalid_offer)?,
        );
    }
    let source = PhloSourcePolicyV1::new(
        &payer.custody_key,
        source_cap,
        source_cap,
        true,
        permissions,
        PhloSourceLimits {
            wire: limits.funding.wire,
            resource_permissions: schedule.classes.len(),
            authority_nodes: limits.funding.authority_nodes,
        },
    )
    .map_err(invalid_offer)?;
    let schedule_commitment = schedule
        .digest(PhloGenesisPolicy::LIMITS)
        .map_err(invalid_offer)?;
    let funding = PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: phlo_limit,
                price_ceiling: phlo_price,
                required_owner_ceilings: vec![phlo_price],
                permitted_schedules: vec![schedule.clone()],
            },
            schedule_commitment,
            total_exposure: ceiling,
            sources: vec![source],
        },
        grant_uses: Vec::new(),
        conversion: PhloConversionCompositionV2::NoConversion,
    }
    .encode(PhloFundingIntentV2Limits {
        wire: limits.funding.wire,
        base: limits.funding,
        grant_uses: limits.funding.wire.total_bytes / 8,
        grant_id_bytes: limits.funding.wire.field_bytes,
        quote_evidence_bytes: limits.funding.wire.field_bytes,
    })
    .map_err(invalid_offer)?;
    let payload = OfferedFundedDeploy::new(
        body,
        funding,
        i64::try_from(phlo_limit).map_err(|_| invalid_offer("the phlo limit exceeds an i64"))?,
        i64::try_from(phlo_price).map_err(|_| invalid_offer("the phlo price exceeds an i64"))?,
        limits,
    )
    .map_err(invalid_offer)?;
    let signed = Cosigned::create_single_envelope(payload, Box::new(Secp256k1), owner.clone())
        .map_err(invalid_offer)?;
    OfferedFundedDeploy::to_proto(&signed).map_err(invalid_offer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> DeployData {
        DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        }
    }

    #[test]
    fn the_default_resource_policy_binds_protocol_6_and_the_shard() {
        let policy = default_resource_policy("root").expect("the default policy forms");
        assert!(policy.offered_funded_v6_active());
        policy
            .validate_context(6, "root", 8)
            .expect("the default policy binds protocol 6, the shard and 8 decimals");
        assert!(policy.validate_context(6, "other", 8).is_err());
        let encoded = policy.encode().expect("the default policy encodes");
        assert_eq!(
            PhloGenesisPolicy::decode(&encoded).expect("the default policy decodes"),
            policy
        );
    }

    // The REV ceiling of two u64 factors fits a u128, because
    // (2^64 - 1)^2 + 1 < 2^128, so the reachable refusal is the source cap.
    #[test]
    fn an_owner_direct_offer_refuses_a_ceiling_above_a_source_cap() {
        let (owner, _) = Secp256k1.new_key_pair();
        let schedule = default_resource_schedule("root");
        for (phlo_limit, phlo_price) in [(u64::MAX, u64::MAX), (u64::MAX, 2)] {
            let error = owner_direct_offer(&owner, body(), phlo_limit, phlo_price, &schedule)
                .expect_err("a ceiling above a source cap is refused");
            assert!(
                error.to_string().contains("exceeds a source cap"),
                "{error}"
            );
        }
    }

    #[test]
    fn an_owner_direct_offer_signs_its_rev_ceiling_as_every_cap() {
        use models::rust::phlo_intent::PhloFundingIntentVersioned;

        let (owner, owner_public) = Secp256k1.new_key_pair();
        let schedule = default_resource_schedule("root");
        let limits = offered_funded_v6_limits().envelope;
        let proto = owner_direct_offer(&owner, body(), 1_000, 2, &schedule)
            .expect("an owner-direct offer signs");
        let signed = OfferedFundedDeploy::from_proto(proto, limits.payload)
            .expect("the owner-direct offer decodes");
        assert_eq!(signed.data.phlo_limit(), 1_000);
        assert_eq!(signed.data.phlo_price(), 2);
        assert_eq!(signed.primary().pk.bytes, owner_public.bytes);
        let syntax = limits.payload.funding;
        let intent = PhloFundingIntentVersioned::decode(
            signed.data.funding_intent(),
            PhloFundingIntentV2Limits {
                wire: syntax.wire,
                base: syntax,
                grant_uses: syntax.wire.total_bytes / 8,
                grant_id_bytes: syntax.wire.field_bytes,
                quote_evidence_bytes: syntax.wire.field_bytes,
            },
        )
        .expect("the funding intent decodes");
        let PhloFundingIntentVersioned::V2(intent) = intent else {
            panic!("an owner-direct offer signs a V2 funding intent");
        };
        let ceiling = 1_000 * 2 + FIXED_FEE_REV;
        assert_eq!(intent.base.total_exposure, ceiling);
        assert_eq!(intent.base.controls.limit, 1_000);
        assert_eq!(intent.base.controls.price_ceiling, 2);
        assert_eq!(intent.base.controls.required_owner_ceilings, vec![2]);
        assert_eq!(intent.base.sources.len(), 1);
        let source = &intent.base.sources[0];
        assert_eq!(u128::from(source.hold_cap()), ceiling);
        assert_eq!(u128::from(source.debit_cap()), ceiling);
        assert!(matches!(
            intent.conversion,
            PhloConversionCompositionV2::NoConversion
        ));
    }
}
