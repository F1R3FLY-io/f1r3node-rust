use crypto::rust::signatures::signed::{Cosigned, ToMessage};
use prost::Message;

use crate::casper::DeployDataProto;
use crate::rust::casper::protocol::casper_message::DeployData;
use crate::rust::phlo_intent::{PhloFundingIntentLimits, PhloFundingIntentV1};
use crate::rust::phlo_wire::{PhloWireEncoder, PhloWireLimits};

mod offered;
pub use offered::{
    OfferedFundedDeploy, OFFERED_FUNDED_DEPLOY_AUTHORIZATION_VERSION,
    OFFERED_FUNDED_DEPLOY_INTENT_DOMAIN,
};

pub const FUNDED_DEPLOY_AUTHORIZATION_VERSION: u32 = 0x0006_0002;
pub const FUNDED_DEPLOY_INTENT_DOMAIN: &[u8] = b"f1r3node:funded-deploy-intent:v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FundedDeployLimits {
    pub deploy_bytes: usize,
    pub signing: PhloWireLimits,
    pub funding: PhloFundingIntentLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FundedDeploy {
    body: DeployData,
    funding_intent: Vec<u8>,
    #[serde(skip)]
    signing_payload: Vec<u8>,
}

impl FundedDeploy {
    pub fn new(
        body: DeployData,
        funding_intent: Vec<u8>,
        limits: FundedDeployLimits,
    ) -> Result<Self, String> {
        check_canonical_funding(&funding_intent, limits.funding)?;
        let mut proto = body.to_message();
        proto.language.clone_from(&body.language);
        let encoded_funding_len = prost::encoding::bytes::encoded_len(21, &funding_intent);
        if proto
            .encoded_len()
            .checked_add(encoded_funding_len)
            .is_none_or(|size| size > limits.deploy_bytes)
        {
            return Err("funded deploy exceeds its byte limit".to_string());
        }
        let body_intent = body.envelope_intent_v61()?;
        let signing_payload = encode_signing_payload(
            [0, 2],
            &[
                FUNDED_DEPLOY_INTENT_DOMAIN,
                &FUNDED_DEPLOY_AUTHORIZATION_VERSION.to_be_bytes(),
                body_intent.as_slice(),
                funding_intent.as_slice(),
            ],
            limits.signing,
        )?;
        Ok(Self {
            body,
            funding_intent,
            signing_payload,
        })
    }

    pub fn body(&self) -> &DeployData { &self.body }

    pub fn funding_intent(&self) -> &[u8] { &self.funding_intent }

    pub fn from_proto(
        proto: DeployDataProto,
        limits: FundedDeployLimits,
    ) -> Result<Cosigned<Self>, String> {
        DeployData::reject_phlo_offer(&proto)?;
        if proto.encoded_len() > limits.deploy_bytes {
            return Err("funded deploy exceeds its byte limit".to_string());
        }
        DeployData::from_proto_envelope(proto, FUNDED_DEPLOY_AUTHORIZATION_VERSION, |mut proto| {
            let funding = proto
                .funding_intent
                .take()
                .ok_or_else(|| "funded deploy requires explicit funding consent".to_string())?;
            Self::new(DeployData::_from_proto(proto), funding.to_vec(), limits)
        })
    }

    pub fn to_proto(envelope: &Cosigned<Self>) -> Result<DeployDataProto, String> {
        DeployData::to_proto_envelope(
            envelope,
            envelope.data.to_message(),
            FUNDED_DEPLOY_AUTHORIZATION_VERSION,
        )
    }
}

impl ToMessage for FundedDeploy {
    type Type = DeployDataProto;

    fn to_message(&self) -> Self::Type {
        let mut proto = self.body.to_message();
        proto.language.clone_from(&self.body.language);
        proto.funding_intent = Some(self.funding_intent.clone().into());
        proto
    }

    fn envelope_intent_v61(&self) -> Result<Vec<u8>, String> { Ok(self.signing_payload.clone()) }
}

fn check_canonical_funding(funding: &[u8], limits: PhloFundingIntentLimits) -> Result<(), String> {
    let intent = PhloFundingIntentV1::decode(funding, limits).map_err(|error| error.to_string())?;
    if intent.encode(limits).map_err(|error| error.to_string())? != funding {
        return Err("funding intent is not canonical".to_string());
    }
    Ok(())
}

fn encode_signing_payload(
    prefix: [u8; 2],
    values: &[&[u8]],
    limits: PhloWireLimits,
) -> Result<Vec<u8>, String> {
    let fields_limit = limits
        .total_bytes
        .checked_sub(prefix.len())
        .ok_or_else(|| "funded signing payload exceeds its byte limit".to_string())?;
    let mut fields = PhloWireEncoder::new(PhloWireLimits {
        total_bytes: fields_limit,
        field_bytes: limits.field_bytes,
    });
    for field in values {
        fields.bytes(field).map_err(|error| error.to_string())?;
    }
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(fields.as_bytes().len() + prefix.len())
        .map_err(|_| "funded signing payload allocation failed".to_string())?;
    payload.extend_from_slice(&prefix);
    payload.extend_from_slice(fields.as_bytes());
    Ok(payload)
}

#[cfg(test)]
mod tests;
