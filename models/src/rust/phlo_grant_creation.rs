use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::{Cosigned, CosignedError, ToMessage};
use thiserror::Error;

use super::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

pub const PHLO_GRANT_CREATION_V1_DOMAIN: &[u8] = b"f1r3node:phlo-grant-creation:v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloGrantCreationLimits {
    pub wire: PhloWireLimits,
    pub grant_id_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloGrantCreationTerms<'a> {
    pub grant_id: &'a [u8],
    pub issuer_public_key: &'a [u8],
    pub delegate_public_key: &'a [u8],
    pub custody: [u8; 32],
    pub network: &'a [u8],
    pub shard: &'a [u8],
    pub schedule_commitment: [u8; 32],
    pub authority_version: u64,
    pub max_draw: u128,
    pub cumulative_ceiling: u128,
    pub valid_from: Option<u64>,
    pub valid_until: Option<u64>,
    pub operation_id: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PhloGrantCreationV1 {
    grant_id: Vec<u8>,
    issuer_public_key: Vec<u8>,
    delegate_public_key: Vec<u8>,
    custody: [u8; 32],
    network: Vec<u8>,
    shard: Vec<u8>,
    schedule_commitment: [u8; 32],
    authority_version: u64,
    max_draw: u128,
    cumulative_ceiling: u128,
    valid_from: Option<u64>,
    valid_until: Option<u64>,
    operation_id: [u8; 32],
    #[serde(skip)]
    encoded: Vec<u8>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct PhloGrantCreationWireProto {
    #[prost(bytes = "vec", tag = "1")]
    pub canonical_creation: Vec<u8>,
}

#[derive(Debug)]
pub struct VerifiedGrantCreationV1 {
    envelope: Cosigned<PhloGrantCreationV1>,
}

#[derive(Debug, Error)]
pub enum PhloGrantCreationError {
    #[error("unsupported grant-creation format")]
    Format,
    #[error("grant creation has an empty or oversized identity")]
    Identity,
    #[error("grant creation has an invalid owner or delegate public key")]
    PublicKey,
    #[error("grant creation has invalid quantitative or validity bounds")]
    Bounds,
    #[error("grant creation has an invalid signed owner")]
    Owner,
    #[error("grant creation does not match the adopted network and shard")]
    Context,
    #[error("grant creation has a noncanonical field width")]
    FieldWidth,
    #[error(transparent)]
    Wire(#[from] PhloWireError),
    #[error(transparent)]
    Signature(#[from] CosignedError),
}

fn endpoint(value: Option<u64>) -> [u8; 9] {
    let mut bytes = [0; 9];
    if let Some(value) = value {
        bytes[0] = 1;
        bytes[1..].copy_from_slice(&value.to_be_bytes());
    }
    bytes
}

fn decode_endpoint(bytes: &[u8]) -> Result<Option<u64>, PhloGrantCreationError> {
    match bytes {
        [0, 0, 0, 0, 0, 0, 0, 0, 0] => Ok(None),
        [1, rest @ ..] => Ok(Some(u64::from_be_bytes(
            rest.try_into()
                .map_err(|_| PhloGrantCreationError::FieldWidth)?,
        ))),
        _ => Err(PhloGrantCreationError::FieldWidth),
    }
}

fn fixed<const N: usize>(
    decoder: &mut PhloWireDecoder<'_>,
) -> Result<[u8; N], PhloGrantCreationError> {
    decoder
        .bytes()?
        .try_into()
        .map_err(|_| PhloGrantCreationError::FieldWidth)
}

impl PhloGrantCreationV1 {
    pub fn new(
        terms: PhloGrantCreationTerms<'_>,
        limits: PhloGrantCreationLimits,
    ) -> Result<Self, PhloGrantCreationError> {
        if terms.grant_id.is_empty()
            || terms.grant_id.len() > limits.grant_id_bytes
            || terms.network.is_empty()
            || terms.shard.is_empty()
        {
            return Err(PhloGrantCreationError::Identity);
        }
        if PublicKey::validate_secp256k1_bytes(terms.issuer_public_key).is_err()
            || PublicKey::validate_secp256k1_bytes(terms.delegate_public_key).is_err()
        {
            return Err(PhloGrantCreationError::PublicKey);
        }
        if terms.max_draw == 0
            || terms.max_draw > terms.cumulative_ceiling
            || terms
                .valid_from
                .zip(terms.valid_until)
                .is_some_and(|(from, until)| from > until)
        {
            return Err(PhloGrantCreationError::Bounds);
        }
        let mut encoder = PhloWireEncoder::new(limits.wire);
        for field in [
            PHLO_GRANT_CREATION_V1_DOMAIN,
            terms.grant_id,
            terms.issuer_public_key,
            terms.delegate_public_key,
            &terms.custody,
            terms.network,
            terms.shard,
            &terms.schedule_commitment,
            &terms.authority_version.to_be_bytes(),
            &terms.max_draw.to_be_bytes(),
            &terms.cumulative_ceiling.to_be_bytes(),
            &endpoint(terms.valid_from),
            &endpoint(terms.valid_until),
            &terms.operation_id,
        ] {
            encoder.bytes(field)?;
        }
        Ok(Self {
            grant_id: terms.grant_id.to_vec(),
            issuer_public_key: terms.issuer_public_key.to_vec(),
            delegate_public_key: terms.delegate_public_key.to_vec(),
            custody: terms.custody,
            network: terms.network.to_vec(),
            shard: terms.shard.to_vec(),
            schedule_commitment: terms.schedule_commitment,
            authority_version: terms.authority_version,
            max_draw: terms.max_draw,
            cumulative_ceiling: terms.cumulative_ceiling,
            valid_from: terms.valid_from,
            valid_until: terms.valid_until,
            operation_id: terms.operation_id,
            encoded: encoder.into_bytes(),
        })
    }

    pub fn decode(
        bytes: &[u8],
        limits: PhloGrantCreationLimits,
    ) -> Result<Self, PhloGrantCreationError> {
        let mut decoder = PhloWireDecoder::new(bytes, limits.wire)?;
        if decoder.bytes()? != PHLO_GRANT_CREATION_V1_DOMAIN {
            return Err(PhloGrantCreationError::Format);
        }
        let terms = PhloGrantCreationTerms {
            grant_id: decoder.bytes()?,
            issuer_public_key: decoder.bytes()?,
            delegate_public_key: decoder.bytes()?,
            custody: fixed(&mut decoder)?,
            network: decoder.bytes()?,
            shard: decoder.bytes()?,
            schedule_commitment: fixed(&mut decoder)?,
            authority_version: u64::from_be_bytes(fixed(&mut decoder)?),
            max_draw: u128::from_be_bytes(fixed(&mut decoder)?),
            cumulative_ceiling: u128::from_be_bytes(fixed(&mut decoder)?),
            valid_from: decode_endpoint(&fixed::<9>(&mut decoder)?)?,
            valid_until: decode_endpoint(&fixed::<9>(&mut decoder)?)?,
            operation_id: fixed(&mut decoder)?,
        };
        decoder.finish()?;
        let creation = Self::new(terms, limits)?;
        if creation.encoded != bytes {
            return Err(PhloGrantCreationError::Format);
        }
        Ok(creation)
    }

    pub fn encoded(&self) -> &[u8] { &self.encoded }
    pub fn grant_id(&self) -> &[u8] { &self.grant_id }
    pub fn issuer_public_key(&self) -> &[u8] { &self.issuer_public_key }
    pub fn delegate_public_key(&self) -> &[u8] { &self.delegate_public_key }
    pub fn custody(&self) -> [u8; 32] { self.custody }
    pub fn network(&self) -> &[u8] { &self.network }
    pub fn shard(&self) -> &[u8] { &self.shard }
    pub fn schedule_commitment(&self) -> [u8; 32] { self.schedule_commitment }
    pub fn authority_version(&self) -> u64 { self.authority_version }
    pub fn max_draw(&self) -> u128 { self.max_draw }
    pub fn cumulative_ceiling(&self) -> u128 { self.cumulative_ceiling }
    pub fn valid_from(&self) -> Option<u64> { self.valid_from }
    pub fn valid_until(&self) -> Option<u64> { self.valid_until }
    pub fn operation_id(&self) -> [u8; 32] { self.operation_id }
}

impl ToMessage for PhloGrantCreationV1 {
    type Type = PhloGrantCreationWireProto;

    fn to_message(&self) -> Self::Type {
        PhloGrantCreationWireProto {
            canonical_creation: self.encoded.clone(),
        }
    }

    fn envelope_intent_v61(&self) -> Result<Vec<u8>, String> { Ok(self.encoded.clone()) }
}

impl VerifiedGrantCreationV1 {
    pub fn verify(
        envelope: Cosigned<PhloGrantCreationV1>,
        expected_network: &[u8],
        expected_shard: &[u8],
    ) -> Result<Self, PhloGrantCreationError> {
        envelope.validate_envelope()?;
        if envelope.data.network() != expected_network || envelope.data.shard() != expected_shard {
            return Err(PhloGrantCreationError::Context);
        }
        let selected = envelope.selected_signers_v61()?;
        if selected.len() != 1 || selected[0].pk.bytes.as_ref() != envelope.data.issuer_public_key()
        {
            return Err(PhloGrantCreationError::Owner);
        }
        Ok(Self { envelope })
    }

    pub fn creation(&self) -> &PhloGrantCreationV1 { &self.envelope.data }
    pub fn envelope(&self) -> &Cosigned<PhloGrantCreationV1> { &self.envelope }
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;

    use super::*;

    fn limits() -> PhloGrantCreationLimits {
        PhloGrantCreationLimits {
            wire: PhloWireLimits {
                total_bytes: 1024,
                field_bytes: 256,
            },
            grant_id_bytes: 64,
        }
    }

    fn key(byte: u8) -> Vec<u8> {
        Secp256k1
            .to_public(&PrivateKey::from_bytes(&[byte; 32]))
            .bytes
            .to_vec()
    }

    fn terms<'a>(issuer: &'a [u8], delegate: &'a [u8]) -> PhloGrantCreationTerms<'a> {
        PhloGrantCreationTerms {
            grant_id: b"grant",
            issuer_public_key: issuer,
            delegate_public_key: delegate,
            custody: [3; 32],
            network: b"network",
            shard: b"shard",
            schedule_commitment: [4; 32],
            authority_version: 7,
            max_draw: 10,
            cumulative_ceiling: 100,
            valid_from: Some(10),
            valid_until: Some(20),
            operation_id: [5; 32],
        }
    }

    #[test]
    fn signed_creation_binds_owner_delegate_terms_and_context() {
        let issuer = key(1);
        let delegate = key(2);
        let creation = PhloGrantCreationV1::new(terms(&issuer, &delegate), limits()).unwrap();
        assert_eq!(
            PhloGrantCreationV1::decode(creation.encoded(), limits()).unwrap(),
            creation
        );
        let envelope = Cosigned::create_single_envelope(
            creation.clone(),
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap();
        assert!(VerifiedGrantCreationV1::verify(envelope.clone(), b"network", b"shard").is_ok());
        assert!(
            VerifiedGrantCreationV1::verify(envelope.clone(), b"other-network", b"shard").is_err()
        );
        let mut changed = terms(&issuer, &delegate);
        changed.max_draw = 11;
        let mut tampered = envelope;
        tampered.data = PhloGrantCreationV1::new(changed, limits()).unwrap();
        assert!(VerifiedGrantCreationV1::verify(tampered, b"network", b"shard").is_err());
        let wrong_owner = Cosigned::create_single_envelope(
            creation,
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[2; 32]),
        )
        .unwrap();
        assert!(VerifiedGrantCreationV1::verify(wrong_owner, b"network", b"shard").is_err());
    }

    #[test]
    fn creation_rejects_invalid_bounds_and_noncanonical_wire() {
        let issuer = key(1);
        let delegate = key(2);
        let mut invalid = terms(&issuer, &delegate);
        invalid.max_draw = 101;
        assert!(PhloGrantCreationV1::new(invalid, limits()).is_err());
        invalid = terms(&issuer, &delegate);
        invalid.valid_from = Some(21);
        assert!(PhloGrantCreationV1::new(invalid, limits()).is_err());
        let valid = PhloGrantCreationV1::new(terms(&issuer, &delegate), limits()).unwrap();
        let mut trailing = valid.encoded().to_vec();
        trailing.push(0);
        assert!(PhloGrantCreationV1::decode(&trailing, limits()).is_err());
        let truncated = &valid.encoded()[..valid.encoded().len() - 1];
        assert!(PhloGrantCreationV1::decode(truncated, limits()).is_err());
    }
}
