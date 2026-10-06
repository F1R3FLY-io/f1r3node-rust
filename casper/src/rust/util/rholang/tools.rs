// See casper/src/main/scala/coop/rchain/casper/util/rholang/Tools.scala

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeFormat};

pub struct Tools;

impl Tools {
    pub fn unforgeable_name_rng(deployer: &PublicKey, timestamp: i64) -> Blake2b512Random {
        rholang::rust::interpreter::merging::mergeable_tags::unforgeable_name_rng(
            deployer, timestamp,
        )
    }

    pub fn rng(signature: &[u8]) -> Blake2b512Random {
        Blake2b512Random::create_from_bytes(signature)
    }

    pub fn user_envelope_rng(deploy: &DeployEnvelope) -> Blake2b512Random {
        if deploy.format() == DeployEnvelopeFormat::Legacy {
            return Self::unforgeable_name_rng(&deploy.primary().pk, deploy.body().time_stamp);
        }
        let mut seed = b"f1r3node:user-deploy-unforgeable:v6".to_vec();
        seed.extend_from_slice(deploy.identity().as_bytes());
        Self::rng(&seed)
    }
}
