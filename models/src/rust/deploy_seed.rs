use crypto::rust::signatures::signed::Signed;
use prost::Message;

use crate::casper::DeployDataProto;
use crate::rust::casper::protocol::casper_message::DeployData;

/// The bytes every unforgeable name a deploy creates is derived from. Two
/// deploys with equal seeds create the same names, so the seed is also the
/// identity under which at most one deploy may take effect. Proto3 omits a
/// zero field, so a deploy with no valid-after block seeds as
/// (deployer, timestamp) alone.
pub fn name_seed(deployer: &[u8], timestamp: i64, valid_after_block_number: i64) -> Vec<u8> {
    DeployDataProto {
        deployer: deployer.to_vec().into(),
        timestamp,
        valid_after_block_number,
        ..Default::default()
    }
    .encode_to_vec()
}

pub fn deploy_name_seed(deploy: &Signed<DeployData>) -> Vec<u8> {
    name_seed(
        &deploy.pk.bytes,
        deploy.data.time_stamp,
        deploy.data.valid_after_block_number,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_covers_deployer_timestamp_and_valid_after_block() {
        let base = name_seed(&[4, 1], 7, 315);
        assert_eq!(base, name_seed(&[4, 1], 7, 315));
        assert_ne!(base, name_seed(&[4, 2], 7, 315));
        assert_ne!(base, name_seed(&[4, 1], 8, 315));
        assert_ne!(base, name_seed(&[4, 1], 7, 316));
    }

    #[test]
    fn a_zero_valid_after_block_seeds_as_deployer_and_timestamp() {
        let without = DeployDataProto {
            deployer: vec![4, 1].into(),
            timestamp: 7,
            ..Default::default()
        }
        .encode_to_vec();
        assert_eq!(name_seed(&[4, 1], 7, 0), without);
    }
}
