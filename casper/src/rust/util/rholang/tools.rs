// See casper/src/main/scala/coop/rchain/casper/util/rholang/Tools.scala

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;

pub struct Tools;

impl Tools {
    pub fn unforgeable_name_rng(
        deployer: &PublicKey,
        timestamp: i64,
        valid_after_block_number: i64,
    ) -> Blake2b512Random {
        rholang::rust::interpreter::merging::mergeable_tags::unforgeable_name_rng(
            deployer,
            timestamp,
            valid_after_block_number,
        )
    }

    pub fn deploy_name_rng(deploy: &Signed<DeployData>) -> Blake2b512Random {
        Self::unforgeable_name_rng(
            &deploy.pk,
            deploy.data.time_stamp,
            deploy.data.valid_after_block_number,
        )
    }

    pub fn rng(signature: &[u8]) -> Blake2b512Random {
        Blake2b512Random::create_from_bytes(signature)
    }
}
