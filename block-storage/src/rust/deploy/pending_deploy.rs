use std::hash::{Hash, Hasher};

use crypto::rust::signatures::signed::{Cosigned, Signed};
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_id::{DeployIdV6, DeployLookupId, LegacyDeploySignature};
use prost::bytes::Bytes;
use prost::Message;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PendingDeploy {
    deploy_id: DeployLookupId,
    #[serde(with = "shared::rust::serde_bytes")]
    deploy_id_bytes: Bytes,
    envelope: Cosigned<DeployData>,
}

impl PendingDeploy {
    pub fn from_envelope_v6(envelope: Cosigned<DeployData>) -> Result<Self, String> {
        if !envelope.is_envelope_bound() {
            return Err(
                "protocol-v6 pending deploy requires an envelope-bound signature".to_string(),
            );
        }
        let commitment = envelope
            .envelope_commitment()
            .map_err(|error| error.to_string())?;
        let deploy_id = DeployLookupId::V6(
            DeployIdV6::try_from(commitment.as_ref()).map_err(|error| error.to_string())?,
        );
        Ok(Self {
            deploy_id,
            deploy_id_bytes: commitment,
            envelope,
        })
    }

    pub fn from_legacy(deploy: Signed<DeployData>) -> Result<Self, String> {
        let deploy_id_bytes = deploy.sig.clone();
        let deploy_id =
            DeployLookupId::Legacy(LegacyDeploySignature::new(deploy_id_bytes.to_vec()));
        let envelope = Cosigned::from_single_signer(deploy).map_err(|error| error.to_string())?;
        Ok(Self {
            deploy_id,
            deploy_id_bytes,
            envelope,
        })
    }

    pub fn validate_for_protocol(&self, protocol_version: i64) -> Result<(), String> {
        if self.deploy_id.as_bytes() != self.deploy_id_bytes.as_ref() {
            return Err(
                "pending deploy typed identity does not match its byte encoding".to_string(),
            );
        }
        if protocol_version >= 6 {
            if !self.envelope.is_envelope_bound() {
                return Err(
                    "protocol-v6 pending deploy contains a legacy payload signature".to_string(),
                );
            }
            let expected = self
                .envelope
                .envelope_commitment()
                .map_err(|error| error.to_string())?;
            if expected.as_ref() != self.deploy_id.as_bytes() {
                return Err(
                    "pending deploy identity does not match its envelope commitment".to_string(),
                );
            }
        } else if self.envelope.is_envelope_bound() {
            return Err("pre-v6 pending deploy contains a protocol-v6 envelope".to_string());
        } else if self.envelope.primary().sig.as_ref() != self.deploy_id.as_bytes() {
            return Err(
                "legacy pending deploy identity does not match its primary signature".to_string(),
            );
        }
        Ok(())
    }

    pub fn deploy_id(&self) -> &Bytes { &self.deploy_id_bytes }

    pub fn typed_deploy_id(&self) -> &DeployLookupId { &self.deploy_id }

    pub fn data(&self) -> &DeployData { self.envelope.data() }

    pub fn envelope(&self) -> &Cosigned<DeployData> { &self.envelope }

    pub fn into_envelope(self) -> Cosigned<DeployData> { self.envelope }

    pub fn as_legacy_signed_ref(&self) -> Signed<DeployData> {
        self.envelope.as_legacy_signed_ref()
    }

    pub fn encoded_len(&self) -> usize {
        if self.envelope.is_envelope_bound() {
            DeployData::to_proto_cosigned(&self.envelope).encoded_len()
        } else {
            DeployData::to_proto_ref(&self.envelope.as_legacy_signed_ref()).encoded_len()
        }
    }
}

impl PartialEq for PendingDeploy {
    fn eq(&self, other: &Self) -> bool { self.deploy_id == other.deploy_id }
}

impl Eq for PendingDeploy {}

impl Hash for PendingDeploy {
    fn hash<H: Hasher>(&self, state: &mut H) { self.deploy_id.hash(state); }
}
