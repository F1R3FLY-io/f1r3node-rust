// See casper/src/main/scala/coop/rchain/casper/util/rholang/Tools.scala

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Cosigned;
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::Message;

pub struct Tools;

impl Tools {
    pub fn unforgeable_name_rng(deployer: &PublicKey, timestamp: i64) -> Blake2b512Random {
        let seed = DeployDataProto {
            deployer: deployer.bytes.clone(),
            timestamp,
            ..Default::default()
        };

        Blake2b512Random::create_from_bytes(&seed.encode_to_vec())
    }

    pub fn user_deploy_rng(deploy: &Cosigned<DeployData>) -> Blake2b512Random {
        if !deploy.is_envelope_bound() {
            return Self::unforgeable_name_rng(&deploy.primary().pk, deploy.data().time_stamp);
        }
        let mut seed = Vec::new();
        seed.extend_from_slice(b"f1r3node:user-deploy-unforgeable:v6");
        seed.extend_from_slice(
            &deploy
                .envelope_commitment()
                .expect("validated protocol-v6 deploy RNG identity"),
        );
        Self::rng(&seed)
    }

    pub fn rng(signature: &[u8]) -> Blake2b512Random {
        Blake2b512Random::create_from_bytes(signature)
    }
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signed::{Cosigned, Signed};
    use models::rust::casper::protocol::casper_message::DeployData;
    use prost::bytes::Bytes;

    use super::Tools;

    fn data(term: &str) -> DeployData {
        DeployData {
            term: term.to_string(),
            language: "rholang".to_string(),
            time_stamp: 17,
            valid_after_block_number: 3,
            shard_id: "root".to_string(),
            expiration_timestamp: Some(101),
            authority_presentations: Vec::new(),
        }
    }

    #[test]
    fn legacy_user_deploy_rng_is_byte_identical() {
        let signed = Signed {
            data: data("Nil"),
            pk: crypto::rust::public_key::PublicKey::from_bytes(&[2; 33]),
            sig: Bytes::from_static(b"legacy-signature"),
            sig_algorithm: Box::new(Secp256k1),
        };
        let expected = Tools::unforgeable_name_rng(&signed.pk, signed.data.time_stamp);
        let deploy = Cosigned::from_single_signer(signed).unwrap();
        assert_eq!(Tools::user_deploy_rng(&deploy), expected);
    }

    #[test]
    fn protocol_v6_user_deploy_rng_is_bound_to_deploy_id() {
        let key = PrivateKey::from_bytes(&[9; 32]);
        let first = Cosigned::create_single_envelope(data("Nil"), Box::new(Secp256k1), key.clone())
            .unwrap();
        let second = Cosigned::create_single_envelope(data("0"), Box::new(Secp256k1), key).unwrap();
        assert_ne!(
            first.envelope_commitment().unwrap(),
            second.envelope_commitment().unwrap()
        );
        assert_ne!(
            Tools::user_deploy_rng(&first),
            Tools::user_deploy_rng(&second)
        );

        let mut seed = b"f1r3node:user-deploy-unforgeable:v6".to_vec();
        seed.extend_from_slice(&first.envelope_commitment().unwrap());
        assert_eq!(Tools::user_deploy_rng(&first), Tools::rng(&seed));
    }
}
