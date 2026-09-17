use crypto::rust::signatures::signed::{Cosigned, ToMessage};
use prost::Message;

use super::{check_canonical_funding, encode_signing_payload, FundedDeployLimits};
use crate::casper::DeployDataProto;
use crate::rust::casper::protocol::casper_message::DeployData;

pub const OFFERED_FUNDED_DEPLOY_AUTHORIZATION_VERSION: u32 = 0x0006_0003;
pub const OFFERED_FUNDED_DEPLOY_INTENT_DOMAIN: &[u8] = b"f1r3node:offered-funded-deploy-intent:v1";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct OfferedFundedDeploy {
    body: DeployData,
    funding_intent: Vec<u8>,
    #[serde(rename = "phloLimit")]
    phlo_limit: i64,
    #[serde(rename = "phloPrice")]
    phlo_price: i64,
    #[serde(skip)]
    signing_payload: Vec<u8>,
}

impl OfferedFundedDeploy {
    pub fn new(
        body: DeployData,
        funding_intent: Vec<u8>,
        phlo_limit: i64,
        phlo_price: i64,
        limits: FundedDeployLimits,
    ) -> Result<Self, String> {
        let limit = u64::try_from(phlo_limit)
            .map_err(|_| "offered phloLimit must be nonnegative".to_string())?;
        let price = u64::try_from(phlo_price)
            .map_err(|_| "offered phloPrice must be nonnegative".to_string())?;
        check_canonical_funding(&funding_intent, limits.funding)?;
        let mut proto = body.to_message();
        proto.language.clone_from(&body.language);
        proto.phlo_limit = phlo_limit;
        proto.phlo_price = phlo_price;
        if proto
            .encoded_len()
            .checked_add(prost::encoding::bytes::encoded_len(21, &funding_intent))
            .is_none_or(|size| size > limits.deploy_bytes)
        {
            return Err("funded deploy exceeds its byte limit".to_string());
        }
        let body_intent = body.envelope_intent_v61()?;
        let signing_payload = encode_signing_payload(
            [0, 3],
            &[
                OFFERED_FUNDED_DEPLOY_INTENT_DOMAIN,
                &OFFERED_FUNDED_DEPLOY_AUTHORIZATION_VERSION.to_be_bytes(),
                &body_intent,
                &funding_intent,
                &limit.to_be_bytes(),
                &price.to_be_bytes(),
            ],
            limits.signing,
        )?;
        Ok(Self {
            body,
            funding_intent,
            phlo_limit,
            phlo_price,
            signing_payload,
        })
    }

    pub fn body(&self) -> &DeployData { &self.body }

    pub fn funding_intent(&self) -> &[u8] { &self.funding_intent }

    pub fn phlo_limit(&self) -> i64 { self.phlo_limit }

    pub fn phlo_price(&self) -> i64 { self.phlo_price }

    pub fn from_proto(
        proto: DeployDataProto,
        limits: FundedDeployLimits,
    ) -> Result<Cosigned<Self>, String> {
        if proto.encoded_len() > limits.deploy_bytes {
            return Err("funded deploy exceeds its byte limit".to_string());
        }
        DeployData::from_proto_envelope(
            proto,
            OFFERED_FUNDED_DEPLOY_AUTHORIZATION_VERSION,
            |mut proto| {
                let funding = proto
                    .funding_intent
                    .take()
                    .ok_or_else(|| "funded deploy requires explicit funding consent".to_string())?;
                let limit = proto.phlo_limit;
                let price = proto.phlo_price;
                Self::new(
                    DeployData::_from_proto(proto),
                    funding.to_vec(),
                    limit,
                    price,
                    limits,
                )
            },
        )
    }

    pub fn to_proto(envelope: &Cosigned<Self>) -> Result<DeployDataProto, String> {
        DeployData::to_proto_envelope(
            envelope,
            envelope.data.to_message(),
            OFFERED_FUNDED_DEPLOY_AUTHORIZATION_VERSION,
        )
    }
}

impl ToMessage for OfferedFundedDeploy {
    type Type = DeployDataProto;

    fn to_message(&self) -> Self::Type {
        let mut proto = self.body.to_message();
        proto.language.clone_from(&self.body.language);
        proto.phlo_limit = self.phlo_limit;
        proto.phlo_price = self.phlo_price;
        proto.funding_intent = Some(self.funding_intent.clone().into());
        proto
    }

    fn envelope_intent_v61(&self) -> Result<Vec<u8>, String> { Ok(self.signing_payload.clone()) }
}
