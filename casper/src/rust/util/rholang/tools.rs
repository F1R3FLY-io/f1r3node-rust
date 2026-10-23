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
        Self::user_identity_rng(deploy.identity().as_bytes())
    }

    /// Added by DR-134 (DR-71): the names of a protocol-6 deployment come from
    /// its identity. The exploratory dry run seeds its names the same way.
    pub fn user_identity_rng(identity: &[u8]) -> Blake2b512Random {
        let mut seed = b"f1r3node:user-deploy-unforgeable:v6".to_vec();
        seed.extend_from_slice(identity);
        Self::rng(&seed)
    }
}
