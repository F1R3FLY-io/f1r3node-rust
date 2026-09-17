use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;

use crypto::rust::signatures::signatures_alg::SignaturesAlgFactory;
use crypto::rust::signatures::signed::{Cosigned, Cosigner};
use prost::Message;

use crate::casper::authorization_policy_v61::Policy;
use crate::casper::{CompoundSigner, DeployDataProto};
use crate::rust::casper::protocol::casper_message::DeployData;
use crate::rust::deploy_id::{DeployIdV6, DeployLookupId, LegacyDeploySignature};
use crate::rust::signed_phlo_deploy::{
    FundedDeploy, FundedDeployLimits, OfferedFundedDeploy, FUNDED_DEPLOY_AUTHORIZATION_VERSION,
    OFFERED_FUNDED_DEPLOY_AUTHORIZATION_VERSION,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeployEnvelopeFormat {
    Legacy,
    BodyV61,
    Funded,
    OfferedFunded,
}

impl DeployEnvelopeFormat {
    pub fn authorization_version(self) -> Option<u32> {
        match self {
            Self::Legacy => None,
            Self::BodyV61 => Some(0x0006_0001),
            Self::Funded => Some(FUNDED_DEPLOY_AUTHORIZATION_VERSION),
            Self::OfferedFunded => Some(OFFERED_FUNDED_DEPLOY_AUTHORIZATION_VERSION),
        }
    }

    pub fn from_authorization_version(version: Option<u32>) -> Result<Self, String> {
        match version {
            None => Ok(Self::Legacy),
            Some(0x0006_0001) => Ok(Self::BodyV61),
            Some(FUNDED_DEPLOY_AUTHORIZATION_VERSION) => Ok(Self::Funded),
            Some(OFFERED_FUNDED_DEPLOY_AUTHORIZATION_VERSION) => Ok(Self::OfferedFunded),
            Some(version) => Err(format!(
                "unsupported deploy authorization version {version}"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DeployEnvelopeLimits {
    pub payload: FundedDeployLimits,
    pub members: NonZeroUsize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Envelope {
    Legacy {
        envelope: Cosigned<DeployData>,
        primary_index: usize,
        wire_order: Vec<usize>,
        wire_algorithms: Vec<String>,
        wire_threshold: i32,
    },
    BodyV61(Cosigned<DeployData>),
    Funded(Cosigned<FundedDeploy>),
    OfferedFunded(Cosigned<OfferedFundedDeploy>),
}

#[derive(Clone, Copy, Debug)]
pub enum DeployEnvelopeRef<'a> {
    Legacy(&'a Cosigned<DeployData>),
    BodyV61(&'a Cosigned<DeployData>),
    Funded(&'a Cosigned<FundedDeploy>),
    OfferedFunded(&'a Cosigned<OfferedFundedDeploy>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployEnvelope {
    envelope: Envelope,
    identity: DeployLookupId,
}

impl Hash for DeployEnvelope {
    fn hash<H: Hasher>(&self, state: &mut H) { self.identity.hash(state); }
}

impl DeployEnvelope {
    pub fn from_processed_body_proto(mut proto: DeployDataProto) -> Result<Self, String> {
        if !proto.deploy_id.is_empty() {
            return Self::from_body_envelope(DeployData::from_proto_cosigned(proto)?);
        }
        let additional = std::mem::take(&mut proto.cosigners);
        let wire_threshold = proto.cosigner_threshold;
        let primary = DeployData::from_proto(proto)?;
        let identity = DeployLookupId::Legacy(LegacyDeploySignature::new(primary.sig.to_vec()));
        let mut wire_algorithms = vec![primary.sig_algorithm.name()];
        let mut signers = vec![Cosigner {
            pk: primary.pk,
            sig: primary.sig,
            sig_algorithm: primary.sig_algorithm,
        }];
        for signer in additional {
            let algorithm =
                SignaturesAlgFactory::apply(&signer.sig_algorithm).ok_or_else(|| {
                    format!(
                        "unknown legacy signature algorithm: {}",
                        signer.sig_algorithm
                    )
                })?;
            wire_algorithms.push(signer.sig_algorithm);
            signers.push(Cosigner {
                pk: crypto::rust::public_key::PublicKey::from_bytes(&signer.pk),
                sig: signer.sig,
                sig_algorithm: algorithm,
            });
        }
        let envelope = if signers.len() > 1 && wire_threshold > 0 {
            Cosigned::from_signed_data_threshold(
                primary.data,
                signers.clone(),
                wire_threshold as u32,
            )
        } else {
            Cosigned::from_signed_data(primary.data, signers.clone())
        }
        .map_err(|error| error.to_string())?;
        let wire_order = Self::legacy_wire_order(
            envelope.signers(),
            signers.iter().map(|signer| {
                (
                    &signer.pk.bytes[..],
                    signer.sig.as_ref(),
                    signer.sig_algorithm.name(),
                )
            }),
        )?;
        Ok(Self {
            identity,
            envelope: Envelope::Legacy {
                primary_index: wire_order[0],
                envelope,
                wire_order,
                wire_algorithms,
                wire_threshold,
            },
        })
    }

    pub fn from_processed_proto_with_limits(
        proto: DeployDataProto,
        limits: DeployEnvelopeLimits,
    ) -> Result<Self, String> {
        if proto.encoded_len() > limits.payload.deploy_bytes {
            return Err("deploy envelope exceeds its byte limit".to_string());
        }
        if proto.deploy_id.is_empty() && proto.authorization_v61.is_none() {
            let members = proto
                .cosigners
                .len()
                .checked_add(1)
                .ok_or("deploy member count overflow")?;
            if members > limits.members.get() {
                return Err("deploy envelope exceeds its member limit".to_string());
            }
            Self::from_processed_body_proto(proto)
        } else {
            Self::from_proto(proto, limits)
        }
    }

    fn legacy_wire_order<'a>(
        canonical: &[Cosigner],
        original: impl IntoIterator<Item = (&'a [u8], &'a [u8], String)>,
    ) -> Result<Vec<usize>, String> {
        let positions: BTreeMap<_, _> = canonical
            .iter()
            .enumerate()
            .map(|(index, signer)| {
                (
                    (
                        &signer.pk.bytes[..],
                        signer.sig.as_ref(),
                        signer.sig_algorithm.name(),
                    ),
                    index,
                )
            })
            .collect();
        let order = original
            .into_iter()
            .map(|signer| {
                let algorithm = SignaturesAlgFactory::apply(&signer.2)
                    .ok_or_else(|| format!("unknown legacy signature algorithm: {}", signer.2))?
                    .name();
                positions
                    .get(&(signer.0, signer.1, algorithm))
                    .copied()
                    .ok_or_else(|| "legacy wire signer is not in the verified envelope".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if order.len() != canonical.len()
            || order.iter().copied().collect::<BTreeSet<_>>().len() != canonical.len()
        {
            return Err("legacy wire signer order is not a complete permutation".to_string());
        }
        Ok(order)
    }

    pub fn from_body_envelope(envelope: Cosigned<DeployData>) -> Result<Self, String> {
        if envelope.is_envelope_bound() {
            envelope
                .validate_envelope()
                .map_err(|error| error.to_string())?;
            let commitment = envelope
                .envelope_commitment()
                .map_err(|error| error.to_string())?;
            return Ok(Self {
                identity: DeployLookupId::V6(
                    DeployIdV6::try_from(commitment.as_ref()).map_err(|error| error.to_string())?,
                ),
                envelope: Envelope::BodyV61(envelope),
            });
        }
        let primary = envelope
            .signers()
            .first()
            .ok_or("legacy deploy has no primary signer")?
            .clone();
        if primary.sig.is_empty() {
            return Err("legacy deploy requires a selected primary signer".to_string());
        }
        let verified = if envelope.cosigner_threshold() == 0 {
            Cosigned::from_signed_data(envelope.data.clone(), envelope.signers().to_vec())
        } else {
            Cosigned::from_signed_data_threshold(
                envelope.data.clone(),
                envelope.signers().to_vec(),
                envelope.cosigner_threshold(),
            )
        }
        .map_err(|error| error.to_string())?;
        let wire_order = Self::legacy_wire_order(
            verified.signers(),
            envelope.signers().iter().map(|signer| {
                (
                    &signer.pk.bytes[..],
                    signer.sig.as_ref(),
                    signer.sig_algorithm.name(),
                )
            }),
        )?;
        let primary_index = wire_order[0];
        let wire_algorithms = envelope
            .signers()
            .iter()
            .map(|signer| signer.sig_algorithm.name())
            .collect();
        let wire_threshold = i32::try_from(envelope.cosigner_threshold())
            .map_err(|_| "legacy threshold exceeds i32")?;
        Ok(Self {
            identity: DeployLookupId::Legacy(LegacyDeploySignature::new(primary.sig.to_vec())),
            envelope: Envelope::Legacy {
                envelope: verified,
                primary_index,
                wire_order,
                wire_algorithms,
                wire_threshold,
            },
        })
    }

    pub fn body_envelope(&self) -> Result<&Cosigned<DeployData>, String> {
        match &self.envelope {
            Envelope::Legacy {
                envelope,
                primary_index: 0,
                wire_order,
                wire_algorithms,
                wire_threshold,
            } if wire_order.iter().copied().eq(0..envelope.signers().len())
                && wire_algorithms
                    .iter()
                    .zip(envelope.signers())
                    .all(|(name, signer)| *name == signer.sig_algorithm.name())
                && u32::try_from(*wire_threshold).ok() == Some(envelope.cosigner_threshold()) =>
            {
                Ok(envelope)
            }
            Envelope::BodyV61(envelope) => Ok(envelope),
            Envelope::Legacy { .. } => Err(
                "body-only adapter cannot preserve this legacy signer order and primary identity"
                    .to_string(),
            ),
            Envelope::Funded(_) | Envelope::OfferedFunded(_) => {
                Err("body-only adapter cannot execute a funded envelope".to_string())
            }
        }
    }

    pub fn into_body_envelope(self) -> Result<Cosigned<DeployData>, String> {
        self.body_envelope()?;
        match self.envelope {
            Envelope::Legacy { envelope, .. } | Envelope::BodyV61(envelope) => Ok(envelope),
            Envelope::Funded(_) | Envelope::OfferedFunded(_) => {
                unreachable!("body-only adapter checked the format")
            }
        }
    }

    pub fn decode(bytes: &[u8], limits: DeployEnvelopeLimits) -> Result<Self, String> {
        if bytes.len() > limits.payload.deploy_bytes {
            return Err("deploy envelope exceeds its byte limit".to_string());
        }
        Self::from_proto(
            DeployDataProto::decode(bytes).map_err(|error| error.to_string())?,
            limits,
        )
    }

    pub fn from_proto(
        proto: DeployDataProto,
        limits: DeployEnvelopeLimits,
    ) -> Result<Self, String> {
        if proto.encoded_len() > limits.payload.deploy_bytes {
            return Err("deploy envelope exceeds its byte limit".to_string());
        }
        let version = proto
            .authorization_v61
            .as_ref()
            .map(|auth| auth.format_version);
        let format = DeployEnvelopeFormat::from_authorization_version(version)?;
        let members = match proto
            .authorization_v61
            .as_ref()
            .and_then(|auth| auth.policy.as_ref())
            .and_then(|policy| policy.policy.as_ref())
        {
            Some(Policy::AllOf(policy)) => policy.members.len(),
            Some(Policy::Threshold(policy)) => policy.members.len(),
            None => proto
                .cosigners
                .len()
                .checked_add(1)
                .ok_or("deploy member count overflow")?,
        };
        if members > limits.members.get() {
            return Err("deploy envelope exceeds its member limit".to_string());
        }
        let identity = match format {
            DeployEnvelopeFormat::Legacy => {
                if proto.sig.is_empty() {
                    return Err("legacy deploy requires a selected primary signer".to_string());
                }
                DeployLookupId::Legacy(LegacyDeploySignature::new(proto.sig.to_vec()))
            }
            _ => DeployLookupId::V6(
                DeployIdV6::try_from(proto.deploy_id.as_ref())
                    .map_err(|error| error.to_string())?,
            ),
        };
        let envelope = match format {
            DeployEnvelopeFormat::Legacy => {
                let primary_pk = proto.deployer.clone();
                let primary_sig = proto.sig.clone();
                let primary_alg = proto.sig_algorithm.clone();
                let cosigners = proto.cosigners.clone();
                let has_algebra = proto.sig_algebra.is_some();
                let wire_threshold = proto.cosigner_threshold;
                let wire_algorithms: Vec<_> = std::iter::once(primary_alg.clone())
                    .chain(cosigners.iter().map(|signer| signer.sig_algorithm.clone()))
                    .collect();
                let envelope = DeployData::from_proto_cosigned_legacy(proto)?;
                let wire_order = if has_algebra {
                    let primary_index = envelope
                        .signers()
                        .iter()
                        .position(|signer| {
                            signer.pk.bytes[..] == primary_pk[..]
                                && signer.sig == primary_sig
                                && signer.sig_algorithm.name() == primary_alg
                        })
                        .ok_or("legacy primary signer is not in the verified envelope")?;
                    std::iter::once(primary_index)
                        .chain(
                            (0..envelope.signers().len()).filter(|index| *index != primary_index),
                        )
                        .collect()
                } else {
                    Self::legacy_wire_order(
                        envelope.signers(),
                        std::iter::once((primary_pk.as_ref(), primary_sig.as_ref(), primary_alg))
                            .chain(cosigners.iter().map(|signer| {
                                (
                                    signer.pk.as_ref(),
                                    signer.sig.as_ref(),
                                    signer.sig_algorithm.clone(),
                                )
                            })),
                    )?
                };
                let primary_index = wire_order[0];
                let (wire_algorithms, wire_threshold) = if has_algebra {
                    (
                        wire_order
                            .iter()
                            .map(|index| envelope.signers()[*index].sig_algorithm.name())
                            .collect(),
                        i32::try_from(envelope.cosigner_threshold())
                            .map_err(|_| "legacy threshold exceeds i32")?,
                    )
                } else {
                    (wire_algorithms, wire_threshold)
                };
                Envelope::Legacy {
                    envelope,
                    primary_index,
                    wire_order,
                    wire_algorithms,
                    wire_threshold,
                }
            }
            DeployEnvelopeFormat::BodyV61 => {
                Envelope::BodyV61(DeployData::from_proto_cosigned(proto)?)
            }
            DeployEnvelopeFormat::Funded => {
                Envelope::Funded(FundedDeploy::from_proto(proto, limits.payload)?)
            }
            DeployEnvelopeFormat::OfferedFunded => {
                Envelope::OfferedFunded(OfferedFundedDeploy::from_proto(proto, limits.payload)?)
            }
        };
        let result = Self { envelope, identity };
        if result.signers().len() > limits.members.get() {
            return Err("deploy envelope exceeds its member limit".to_string());
        }
        Ok(result)
    }

    pub fn view(&self) -> DeployEnvelopeRef<'_> {
        match &self.envelope {
            Envelope::Legacy { envelope, .. } => DeployEnvelopeRef::Legacy(envelope),
            Envelope::BodyV61(envelope) => DeployEnvelopeRef::BodyV61(envelope),
            Envelope::Funded(envelope) => DeployEnvelopeRef::Funded(envelope),
            Envelope::OfferedFunded(envelope) => DeployEnvelopeRef::OfferedFunded(envelope),
        }
    }

    pub fn format(&self) -> DeployEnvelopeFormat {
        match &self.envelope {
            Envelope::Legacy { .. } => DeployEnvelopeFormat::Legacy,
            Envelope::BodyV61(_) => DeployEnvelopeFormat::BodyV61,
            Envelope::Funded(_) => DeployEnvelopeFormat::Funded,
            Envelope::OfferedFunded(_) => DeployEnvelopeFormat::OfferedFunded,
        }
    }

    pub fn require_format(&self, permitted: &[DeployEnvelopeFormat]) -> Result<(), String> {
        if permitted.contains(&self.format()) {
            Ok(())
        } else {
            Err("deploy authorization format is not active for this execution policy".to_string())
        }
    }

    pub fn identity(&self) -> &DeployLookupId { &self.identity }

    pub fn body(&self) -> &DeployData {
        match &self.envelope {
            Envelope::Legacy { envelope, .. } | Envelope::BodyV61(envelope) => &envelope.data,
            Envelope::Funded(envelope) => envelope.data.body(),
            Envelope::OfferedFunded(envelope) => envelope.data.body(),
        }
    }

    pub fn signers(&self) -> &[Cosigner] {
        match &self.envelope {
            Envelope::Legacy { envelope, .. } | Envelope::BodyV61(envelope) => envelope.signers(),
            Envelope::Funded(envelope) => envelope.signers(),
            Envelope::OfferedFunded(envelope) => envelope.signers(),
        }
    }

    pub fn threshold(&self) -> u32 {
        match &self.envelope {
            Envelope::Legacy { envelope, .. } | Envelope::BodyV61(envelope) => {
                envelope.cosigner_threshold()
            }
            Envelope::Funded(envelope) => envelope.cosigner_threshold(),
            Envelope::OfferedFunded(envelope) => envelope.cosigner_threshold(),
        }
    }

    pub fn primary(&self) -> &Cosigner {
        match &self.envelope {
            Envelope::Legacy {
                envelope,
                primary_index,
                ..
            } => &envelope.signers()[*primary_index],
            _ => self
                .signers()
                .iter()
                .find(|signer| !signer.sig.is_empty())
                .expect("checked envelope has a selected signer"),
        }
    }

    pub fn to_proto(&self) -> Result<DeployDataProto, String> {
        match &self.envelope {
            Envelope::Legacy {
                envelope,
                primary_index,
                wire_order,
                wire_algorithms,
                wire_threshold,
            } => {
                let mut proto = DeployData::to_proto_cosigned(envelope);
                let primary = &envelope.signers()[*primary_index];
                proto.deployer = primary.pk.bytes.clone().into();
                proto.sig = primary.sig.clone();
                proto.sig_algorithm = wire_algorithms[0].clone();
                proto.cosigner_threshold = *wire_threshold;
                proto.cosigners = wire_order
                    .iter()
                    .skip(1)
                    .zip(wire_algorithms.iter().skip(1))
                    .map(|(index, algorithm)| (&envelope.signers()[*index], algorithm))
                    .map(|(signer, algorithm)| CompoundSigner {
                        pk: signer.pk.bytes.clone().into(),
                        sig: signer.sig.clone(),
                        sig_algorithm: algorithm.clone(),
                    })
                    .collect();
                Ok(proto)
            }
            Envelope::BodyV61(envelope) => Ok(DeployData::to_proto_cosigned(envelope)),
            Envelope::Funded(envelope) => FundedDeploy::to_proto(envelope),
            Envelope::OfferedFunded(envelope) => OfferedFundedDeploy::to_proto(envelope),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredDeployEnvelope {
    schema_version: u32,
    authorization_version: Option<u32>,
    wire: Vec<u8>,
}

impl StoredDeployEnvelope {
    pub fn new(envelope: &DeployEnvelope) -> Result<Self, String> {
        Ok(Self {
            schema_version: 1,
            authorization_version: envelope.format().authorization_version(),
            wire: envelope.to_proto()?.encode_to_vec(),
        })
    }

    pub fn decode(
        &self,
        expected: &DeployLookupId,
        limits: DeployEnvelopeLimits,
    ) -> Result<DeployEnvelope, String> {
        if self.schema_version != 1 {
            return Err("unsupported stored deploy envelope schema".to_string());
        }
        let format = DeployEnvelopeFormat::from_authorization_version(self.authorization_version)?;
        let envelope = DeployEnvelope::decode(&self.wire, limits)?;
        if envelope.format() != format || envelope.identity() != expected {
            return Err("stored deploy envelope format or identity mismatch".to_string());
        }
        if envelope.to_proto()?.encode_to_vec() != self.wire {
            return Err("stored deploy envelope wire encoding is not canonical".to_string());
        }
        Ok(envelope)
    }
}
