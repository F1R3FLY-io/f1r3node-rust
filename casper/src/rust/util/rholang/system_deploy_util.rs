// See casper/src/main/scala/coop/rchain/casper/util/rholang/SystemDeployUtil.scala

use byteorder::{LittleEndian, WriteBytesExt};
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::validator::Validator;

use super::tools::Tools;

// Currently we have 4 system deploys -> refund, preCharge, closeBlock, Slashing
// In every user deploy, the rnode would do the preCharge first, then execute the
// user deploy and do the refund at last.
//
// The refund and preCharge system deploy
// would use user deploy signature to generate the system deploy. The random seed of
// the refund and preCharge has to be exactly the same to make sure replay the user
// deploy would come out the exact same result.
//
// closeBlock fires exactly once per block, so PREFIX ++ PublicKey ++ seqNum
// is collision-free for it.
//
// Slashing can fire multiple times per block (one per offender). The slash
// seed therefore takes the block the slash cites as well so each slash in the
// same block gets a distinct rng — the slash contract allocates
// `new rl, poSCh, ...` from this rng, and shared seeds would alias those
// unforgeable channel names across slashes in the same block.

const SYSTEM_DEPLOY_PREFIX: i32 = 1;

fn serialize_int32_fixed(value: i32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4);
    buf.write_i32::<LittleEndian>(value)
        .expect("Failed to write bytes");
    buf
}

pub fn generate_system_deploy_random_seed(sender: Validator, seq_num: i32) -> Blake2b512Random {
    let seed: Vec<u8> = serialize_int32_fixed(SYSTEM_DEPLOY_PREFIX)
        .into_iter()
        .chain(sender)
        .chain(serialize_int32_fixed(seq_num))
        .collect();

    Tools::rng(&seed)
}

pub fn generate_close_deploy_random_seed_from_validator(
    validator: Validator,
    seq_num: i32,
) -> Blake2b512Random {
    generate_system_deploy_random_seed(validator, seq_num).split_byte(0)
}

pub fn generate_close_deploy_random_seed_from_pk(pk: PublicKey, seq_num: i32) -> Blake2b512Random {
    let sender = pk.bytes;
    generate_close_deploy_random_seed_from_validator(sender, seq_num)
}

pub fn generate_slash_deploy_random_seed(
    validator: Validator,
    seq_num: i32,
    cited_block_hash: &BlockHash,
) -> Blake2b512Random {
    let seed: Vec<u8> = serialize_int32_fixed(SYSTEM_DEPLOY_PREFIX)
        .into_iter()
        .chain(validator)
        .chain(serialize_int32_fixed(seq_num))
        .chain(cited_block_hash.iter().copied())
        .collect();
    Tools::rng(&seed).split_byte(1)
}

/// The split byte keeps it disjoint from the close-block and slash seeds
/// drawn from the same proposer and sequence number.
pub fn generate_resolve_claims_deploy_random_seed(
    validator: Validator,
    seq_num: i32,
) -> Blake2b512Random {
    let seed: Vec<u8> = serialize_int32_fixed(SYSTEM_DEPLOY_PREFIX)
        .into_iter()
        .chain(validator)
        .chain(serialize_int32_fixed(seq_num))
        .collect();
    Tools::rng(&seed).split_byte(3)
}

/// Seeded from the accused, so the evidence against each accused in one
/// block draws its own randomness.
pub fn generate_dishonest_vote_deploy_random_seed(
    validator: Validator,
    seq_num: i32,
    accused: &Validator,
) -> Blake2b512Random {
    let seed: Vec<u8> = serialize_int32_fixed(SYSTEM_DEPLOY_PREFIX)
        .into_iter()
        .chain(validator)
        .chain(serialize_int32_fixed(seq_num))
        .chain(accused.iter().copied())
        .collect();
    Tools::rng(&seed).split_byte(4)
}

pub fn generate_pre_charge_deploy_random_seed(deploy: &Signed<DeployData>) -> Blake2b512Random {
    Tools::rng(&deploy.sig).split_byte(0)
}

pub fn generate_refund_deploy_random_seed(deploy: &Signed<DeployData>) -> Blake2b512Random {
    Tools::rng(&deploy.sig).split_byte(1)
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;

    /// Two slashes in the same block, same proposer, same seq_num, different
    /// equivocators must produce distinct rng seeds. Without the cited block
    /// in the seed input, the slash contract's
    /// `new rl, poSCh, ...` allocations would alias unforgeable channel
    /// names across slashes in the same block, corrupting tuplespace state
    /// and the per-slash return-channel routing.
    #[test]
    fn slash_seed_differs_per_cited_block_hash() {
        let validator: Validator = Bytes::from(vec![0xAA; 32]);
        let seq_num = 7;
        let cited_block_a: BlockHash = Bytes::from(vec![0x11; 32]);
        let cited_block_b: BlockHash = Bytes::from(vec![0x22; 32]);

        let seed_a = generate_slash_deploy_random_seed(validator.clone(), seq_num, &cited_block_a);
        let seed_b = generate_slash_deploy_random_seed(validator.clone(), seq_num, &cited_block_b);

        assert_ne!(
            seed_a.to_bytes(),
            seed_b.to_bytes(),
            "two distinct cited blocks must produce distinct slash seeds; \
             without this, multi-slash blocks emit SlashDeploys with identical rng \
             which alias the unforgeable channel names allocated by the slash contract"
        );
    }

    /// Same proposer, same seq_num, same cited block must produce the
    /// SAME seed across calls. Replay determinism depends on this — a validator
    /// re-running a historical slash must reconstruct the exact rng state used
    /// at original execution.
    #[test]
    fn slash_seed_is_deterministic_for_same_inputs() {
        let validator: Validator = Bytes::from(vec![0xBB; 32]);
        let seq_num = 13;
        let cited_block: BlockHash = Bytes::from(vec![0x33; 32]);

        let seed_first =
            generate_slash_deploy_random_seed(validator.clone(), seq_num, &cited_block);
        let seed_second =
            generate_slash_deploy_random_seed(validator.clone(), seq_num, &cited_block);

        assert_eq!(
            seed_first.to_bytes(),
            seed_second.to_bytes(),
            "same inputs must produce same seed for replay determinism"
        );
    }
}
