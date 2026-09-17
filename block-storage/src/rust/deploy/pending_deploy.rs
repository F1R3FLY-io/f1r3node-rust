use std::hash::{Hash, Hasher};

use crypto::rust::signatures::signed::{Cosigned, Signed};
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeFormat};
use models::rust::deploy_id::DeployLookupId;
use prost::bytes::Bytes;
use prost::Message;

#[derive(Clone, Debug)]
pub struct PendingDeploy {
    envelope: DeployEnvelope,
    deploy_id_bytes: Bytes,
    encoded_len: usize,
}

#[derive(serde::Deserialize)]
struct LegacyPendingDeployRecord {
    deploy_id: DeployLookupId,
    #[serde(with = "shared::rust::serde_bytes")]
    deploy_id_bytes: Bytes,
    envelope: Cosigned<DeployData>,
}

#[derive(serde::Serialize)]
struct LegacyPendingDeployRecordRef<'a> {
    deploy_id: &'a DeployLookupId,
    #[serde(with = "shared::rust::serde_bytes")]
    deploy_id_bytes: &'a Bytes,
    envelope: &'a Cosigned<DeployData>,
}

impl PendingDeploy {
    pub fn from_envelope(envelope: DeployEnvelope) -> Result<Self, String> {
        let encoded_len = envelope.to_proto()?.encoded_len();
        Ok(Self {
            deploy_id_bytes: Bytes::copy_from_slice(envelope.identity().as_bytes()),
            envelope,
            encoded_len,
        })
    }

    pub fn from_envelope_v6(envelope: Cosigned<DeployData>) -> Result<Self, String> {
        if !envelope.is_envelope_bound() {
            return Err(
                "protocol-v6 pending deploy requires an envelope-bound signature".to_string(),
            );
        }
        Self::from_envelope(DeployEnvelope::from_body_envelope(envelope)?)
    }

    pub fn from_legacy(deploy: Signed<DeployData>) -> Result<Self, String> {
        let envelope = Cosigned::from_single_signer(deploy).map_err(|error| error.to_string())?;
        Self::from_envelope(DeployEnvelope::from_body_envelope(envelope)?)
    }

    pub fn validate_for_protocol(&self, protocol_version: i64) -> Result<(), String> {
        let permitted = if protocol_version >= 6 {
            DeployEnvelopeFormat::BodyV61
        } else {
            DeployEnvelopeFormat::Legacy
        };
        self.envelope.require_format(&[permitted])
    }

    pub fn deploy_id(&self) -> &Bytes { &self.deploy_id_bytes }

    pub fn typed_deploy_id(&self) -> &DeployLookupId { self.envelope.identity() }

    pub fn data(&self) -> &DeployData { self.envelope.body() }

    pub fn envelope(&self) -> &DeployEnvelope { &self.envelope }

    pub fn into_envelope(self) -> DeployEnvelope { self.envelope }

    pub fn into_body_envelope(self) -> Result<Cosigned<DeployData>, String> {
        self.envelope.into_body_envelope()
    }

    pub fn encoded_len(&self) -> usize { self.encoded_len }
}

impl TryFrom<LegacyPendingDeployRecord> for PendingDeploy {
    type Error = String;

    fn try_from(record: LegacyPendingDeployRecord) -> Result<Self, Self::Error> {
        if record.deploy_id.as_bytes() != record.deploy_id_bytes.as_ref() {
            return Err(
                "pending deploy typed identity does not match its byte encoding".to_string(),
            );
        }
        let envelope = DeployEnvelope::from_body_envelope(record.envelope)?;
        if envelope.identity() != &record.deploy_id {
            return Err(
                "pending deploy identity does not match its authenticated envelope".to_string(),
            );
        }
        Self::from_envelope(envelope)
    }
}

impl serde::Serialize for PendingDeploy {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let envelope = self
            .envelope
            .body_envelope()
            .map_err(serde::ser::Error::custom)?;
        LegacyPendingDeployRecordRef {
            deploy_id: self.typed_deploy_id(),
            deploy_id_bytes: &self.deploy_id_bytes,
            envelope,
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for PendingDeploy {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let record = LegacyPendingDeployRecord::deserialize(deserializer)?;
        Self::try_from(record).map_err(serde::de::Error::custom)
    }
}

impl PartialEq for PendingDeploy {
    fn eq(&self, other: &Self) -> bool { self.typed_deploy_id() == other.typed_deploy_id() }
}

impl Eq for PendingDeploy {}

impl Hash for PendingDeploy {
    fn hash<H: Hasher>(&self, state: &mut H) { self.typed_deploy_id().hash(state); }
}

#[cfg(test)]
mod tests;
