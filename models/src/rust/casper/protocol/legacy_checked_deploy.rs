//! Checked historical Casper deploy authorization for processed records.
//!
//! This deliberately uses the phlo-bearing legacy `DeployData`, not the D3
//! cost-body type. The two payloads have different signed protobuf preimages.
//! Historical processed records authorize their flat signer list and ignore
//! `sig_algebra`; ingress has a separate algebra-selecting decoder.

use std::collections::{BTreeMap, BTreeSet};

use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signatures_alg::SignaturesAlgFactory;
use crypto::rust::signatures::signed::{Cosigned, Cosigner, Signed};

use super::casper_message::DeployData;
use crate::casper::{CompoundSigner, DeployDataProto};

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedLegacyDeploy {
    envelope: Cosigned<DeployData>,
    primary_index: usize,
    wire_order: Vec<usize>,
    wire_algorithms: Vec<String>,
    wire_threshold: i32,
}

impl CheckedLegacyDeploy {
    /// Re-verify a single signed deploy before accepting it as a processed
    /// record. This keeps even the direct runtime constructor checked.
    pub fn from_signed(signed: Signed<DeployData>) -> Result<Self, String> {
        let algorithm = signed.sig_algorithm.name();
        let signer = Cosigner {
            pk: signed.pk,
            sig: signed.sig,
            sig_algorithm: signed.sig_algorithm,
        };
        let envelope = Cosigned::from_signed_data(signed.data, vec![signer])
            .map_err(|error| error.to_string())?;
        Ok(Self {
            envelope,
            primary_index: 0,
            wire_order: vec![0],
            wire_algorithms: vec![algorithm],
            wire_threshold: 0,
        })
    }

    /// The historical processed decoder verifies the *flat* list even if an
    /// ingress-only signature algebra is present. The algebra is intentionally
    /// discarded on re-encode; neither path sniffs cost payloads or epochs.
    pub fn from_processed_proto(mut proto: DeployDataProto) -> Result<Self, String> {
        let additional = std::mem::take(&mut proto.cosigners);
        let wire_threshold = std::mem::take(&mut proto.cosigner_threshold);
        let primary_wire_algorithm = proto.sig_algorithm.clone();
        if wire_threshold < 0 {
            return Err("negative historical cosigner threshold".to_string());
        }
        proto.sig_algebra = None;
        let primary = DeployData::from_proto(proto)?;
        let mut wire_algorithms = vec![primary_wire_algorithm];
        let mut wire_signers = vec![Cosigner {
            pk: primary.pk,
            sig: primary.sig,
            sig_algorithm: primary.sig_algorithm,
        }];
        for signer in additional {
            let algorithm =
                SignaturesAlgFactory::apply(&signer.sig_algorithm).ok_or_else(|| {
                    format!(
                        "Unknown historical cosigner signature algorithm: {}",
                        signer.sig_algorithm
                    )
                })?;
            wire_algorithms.push(signer.sig_algorithm);
            wire_signers.push(Cosigner {
                pk: PublicKey::from_bytes(&signer.pk),
                sig: signer.sig,
                sig_algorithm: algorithm,
            });
        }

        // A historical single-signer record may carry a nonzero flat
        // threshold: it was ignored as authority but retained as wire data.
        let envelope = if wire_signers.len() > 1 && wire_threshold > 0 {
            Cosigned::from_signed_data_threshold(
                primary.data,
                wire_signers.clone(),
                wire_threshold as u32,
            )
        } else {
            Cosigned::from_signed_data(primary.data, wire_signers.clone())
        }
        .map_err(|error| error.to_string())?;

        let positions: BTreeMap<_, _> = envelope
            .signers()
            .iter()
            .enumerate()
            .map(|(index, signer)| {
                (
                    (
                        signer.pk.bytes.to_vec(),
                        signer.sig.to_vec(),
                        signer.sig_algorithm.name(),
                    ),
                    index,
                )
            })
            .collect();
        let wire_order = wire_signers
            .iter()
            .map(|signer| {
                positions
                    .get(&(
                        signer.pk.bytes.to_vec(),
                        signer.sig.to_vec(),
                        signer.sig_algorithm.name(),
                    ))
                    .copied()
                    .ok_or_else(|| "historical signer absent from checked envelope".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if wire_order.len() != envelope.signers().len()
            || wire_order.iter().copied().collect::<BTreeSet<_>>().len() != wire_order.len()
        {
            return Err("historical signer order is not a permutation".to_string());
        }

        Ok(Self {
            envelope,
            primary_index: wire_order[0],
            wire_order,
            wire_algorithms,
            wire_threshold,
        })
    }

    pub fn body(&self) -> &DeployData { &self.envelope.data }

    pub fn primary(&self) -> &Cosigner { &self.envelope.signers()[self.primary_index] }

    pub fn signers(&self) -> &[Cosigner] { self.envelope.signers() }

    pub fn threshold(&self) -> u32 { self.envelope.cosigner_threshold() }

    pub fn wire_threshold(&self) -> i32 { self.wire_threshold }

    pub fn identity_bytes(&self) -> &[u8] { &self.primary().sig }

    /// Historical runtime body context after full flat-envelope verification.
    /// This projection is not an authorization object: callers needing payer
    /// authority must inspect `signers` and `threshold` on this envelope.
    pub fn execution_projection(&self) -> Signed<DeployData> {
        let primary = self.primary();
        Signed {
            data: self.envelope.data.clone(),
            pk: primary.pk.clone(),
            sig: primary.sig.clone(),
            sig_algorithm: primary.sig_algorithm.clone(),
        }
    }

    pub fn to_proto(&self) -> DeployDataProto {
        let mut proto = DeployData::to_proto_ref(&self.execution_projection());
        proto.sig_algorithm = self.wire_algorithms[0].clone();
        proto.cosigner_threshold = self.wire_threshold;
        proto.cosigners = self
            .wire_order
            .iter()
            .skip(1)
            .zip(self.wire_algorithms.iter().skip(1))
            .map(|(index, algorithm)| {
                let signer = &self.envelope.signers()[*index];
                CompoundSigner {
                    pk: signer.pk.bytes.clone().into(),
                    sig: signer.sig.clone(),
                    sig_algorithm: algorithm.clone(),
                }
            })
            .collect();
        proto
    }
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::secp256k1_eth::Secp256k1Eth;
    use prost::Message;

    use super::*;

    fn payload() -> DeployData {
        DeployData {
            term: "Nil".to_string(),
            time_stamp: 1,
            phlo_price: 7,
            phlo_limit: 11,
            valid_after_block_number: 2,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
        }
    }

    fn flat_two_signer_wire() -> DeployDataProto {
        let first = Signed::create(
            payload(),
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap();
        let second = Signed::create(
            payload(),
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[2; 32]),
        )
        .unwrap();
        // Put the larger public key first so the wire order differs from the
        // canonical order retained inside Cosigned.
        let (primary, additional) = if first.pk.bytes > second.pk.bytes {
            (first, second)
        } else {
            (second, first)
        };
        let mut wire = DeployData::to_proto(primary);
        wire.cosigners.push(CompoundSigner {
            pk: additional.pk.bytes.into(),
            sig: additional.sig,
            sig_algorithm: additional.sig_algorithm.name(),
        });
        wire.cosigner_threshold = 2;
        wire
    }

    #[test]
    fn processed_flat_signers_keep_target_signed_phlo_bytes_and_wire_order() {
        let wire = flat_two_signer_wire();
        let encoded = wire.encode_to_vec();
        let checked = CheckedLegacyDeploy::from_processed_proto(wire.clone()).unwrap();
        assert_eq!(checked.body().phlo_price, 7);
        assert_eq!(checked.body().phlo_limit, 11);
        assert_eq!(checked.signers().len(), 2);
        assert_eq!(checked.threshold(), 2);
        assert_eq!(checked.primary().pk.bytes.as_ref(), wire.deployer.as_ref());
        assert_eq!(checked.to_proto().encode_to_vec(), encoded);
    }

    #[test]
    fn historical_processed_decode_ignores_ingress_algebra_but_checks_flat_signers() {
        let mut wire = flat_two_signer_wire();
        wire.sig_algebra = Some(crate::casper::SigCompound::default());
        let checked = CheckedLegacyDeploy::from_processed_proto(wire.clone()).unwrap();
        assert_eq!(checked.threshold(), 2);
        assert!(checked.to_proto().sig_algebra.is_none());

        let mut forged = wire;
        forged.cosigners[0].sig = vec![0; 64].into();
        assert!(CheckedLegacyDeploy::from_processed_proto(forged).is_err());
    }

    #[test]
    fn historical_primary_algorithm_alias_roundtrips_exact_wire_bytes() {
        let signed = Signed::create(
            payload(),
            Box::new(Secp256k1Eth),
            PrivateKey::from_bytes(&[3; 32]),
        )
        .unwrap();
        let mut wire = DeployData::to_proto(signed);
        wire.sig_algorithm = Secp256k1Eth::LEGACY_NAME.to_string();
        let old_bytes = wire.encode_to_vec();
        let checked = CheckedLegacyDeploy::from_processed_proto(wire).unwrap();
        assert_eq!(checked.to_proto().encode_to_vec(), old_bytes);
    }

    #[test]
    fn checked_execution_receipt_restores_all_cosigners() {
        let wire = flat_two_signer_wire();
        let checked = CheckedLegacyDeploy::from_processed_proto(wire.clone()).unwrap();
        let receipt =
            super::super::casper_message::ProcessedDeploy::empty(checked.execution_projection())
                .unwrap()
                .with_checked_envelope(checked.clone())
                .unwrap();
        assert_eq!(receipt.signers().len(), 2);
        assert_eq!(receipt.to_proto().deploy.unwrap(), wire);

        let unrelated = Signed::create(
            payload(),
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[3; 32]),
        )
        .unwrap();
        let unrelated_receipt =
            super::super::casper_message::ProcessedDeploy::empty(unrelated).unwrap();
        assert!(unrelated_receipt.with_checked_envelope(checked).is_err());
    }

    #[test]
    fn historical_processed_primary_cannot_be_a_threshold_placeholder() {
        let mut wire = flat_two_signer_wire();
        wire.cosigner_threshold = 1;
        wire.sig.clear();
        // The target's processed-record decoder historically verified the
        // primary Signed deploy before considering its flat cosigners. The
        // new offered-funded ingress may use threshold placeholders, but it
        // cannot silently change a historical processed record's identity.
        assert!(CheckedLegacyDeploy::from_processed_proto(wire).is_err());
    }
}
