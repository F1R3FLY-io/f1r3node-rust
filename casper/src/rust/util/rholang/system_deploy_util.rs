// See casper/src/main/scala/coop/rchain/casper/util/rholang/SystemDeployUtil.scala

use byteorder::{LittleEndian, WriteBytesExt};
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::{Cosigned, Signed};
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
// Slashing can fire multiple times per block (one per equivocator detected
// in the proposer's invalid_latest_messages, plus any merge-rejected slash
// re-issued via the recovery path). The slash seed therefore takes the
// equivocator's invalid_block_hash as well so each slash in the same block
// gets a distinct rng — the slash contract allocates `new rl, poSCh, ...`
// from this rng, and shared seeds would alias those unforgeable channel
// names across slashes in the same block.

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
    invalid_block_hash: &BlockHash,
) -> Blake2b512Random {
    let seed: Vec<u8> = serialize_int32_fixed(SYSTEM_DEPLOY_PREFIX)
        .into_iter()
        .chain(validator)
        .chain(serialize_int32_fixed(seq_num))
        .chain(invalid_block_hash.iter().copied())
        .collect();
    Tools::rng(&seed).split_byte(1)
}

pub fn generate_pre_charge_deploy_random_seed(deploy: &Signed<DeployData>) -> Blake2b512Random {
    Tools::rng(&deploy.sig).split_byte(0)
}

pub fn generate_refund_deploy_random_seed(deploy: &Signed<DeployData>) -> Blake2b512Random {
    Tools::rng(&deploy.sig).split_byte(1)
}

/// Per-cosigner pre-charge seed derivation for multi-signature deploys.
///
/// Multi-sig deploys cannot reuse `generate_pre_charge_deploy_random_seed`
/// because that produces the same seed for every cosigner (it only hashes
/// `deploy.sig`, ignoring signer identity). The PoS `chargeDeploy` system
/// contract allocates `new rl, poSCh, depositCh, ...` channels using the
/// rng, and identical seeds across cosigners would alias those unforgeable
/// channel names, corrupting tuplespace state.
///
/// Derivation: `Tools::rng(b"pcs:" || 0u8 || primary_sig || signer_index_le).split_byte(0)`,
/// where `0u8` is the per-cosigner domain tag for pre-charge (refund uses
/// `1u8` — see `generate_refund_deploy_random_seed_for_signer`).
///
/// Replay determinism: the seed is a pure function of
/// `(cosigned.primary().sig, signer_index)`. Both inputs are stable under
/// replay: the primary signature is fixed at deploy submission; the
/// canonical signer order (sorted by `pk.bytes` ascending) makes
/// `signer_index` stable too.
///
/// **Legacy single-sig deploys MUST continue to use
/// `generate_pre_charge_deploy_random_seed` (NOT this function).** The
/// legacy seed scheme is preserved bit-for-bit for back-compat with
/// existing on-chain deploys; only multi-sig deploys use this new scheme.
pub fn generate_pre_charge_deploy_random_seed_for_signer(
    cosigned: &Cosigned<DeployData>,
    signer_index: usize,
) -> Blake2b512Random {
    let primary_sig = &cosigned.primary().sig;
    let mut seed = Vec::with_capacity(b"pcs:".len() + 1 + primary_sig.len() + 4);
    seed.extend_from_slice(b"pcs:");
    seed.push(0u8); // domain tag: 0 = pre-charge
    seed.extend_from_slice(primary_sig);
    seed.extend_from_slice(&(signer_index as u32).to_le_bytes());
    Tools::rng(&seed).split_byte(0)
}

/// Per-cosigner refund seed derivation. Symmetric counterpart to
/// `generate_pre_charge_deploy_random_seed_for_signer`. The domain tag
/// `1u8` distinguishes refund seeds from pre-charge seeds so a cosigner's
/// pre-charge and refund operations cannot alias each other's rng-derived
/// channel names. Replay-deterministic and legacy-back-compat-preserving
/// per the same reasoning as the pre-charge counterpart.
pub fn generate_refund_deploy_random_seed_for_signer(
    cosigned: &Cosigned<DeployData>,
    signer_index: usize,
) -> Blake2b512Random {
    let primary_sig = &cosigned.primary().sig;
    let mut seed = Vec::with_capacity(b"pcs:".len() + 1 + primary_sig.len() + 4);
    seed.extend_from_slice(b"pcs:");
    seed.push(1u8); // domain tag: 1 = refund
    seed.extend_from_slice(primary_sig);
    seed.extend_from_slice(&(signer_index as u32).to_le_bytes());
    Tools::rng(&seed).split_byte(1)
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;

    /// Two slashes in the same block, same proposer, same seq_num, different
    /// equivocators must produce distinct rng seeds. Without
    /// `invalid_block_hash` in the seed input, the slash contract's
    /// `new rl, poSCh, ...` allocations would alias unforgeable channel
    /// names across slashes in the same block, corrupting tuplespace state
    /// and the per-slash return-channel routing.
    #[test]
    fn slash_seed_differs_per_invalid_block_hash() {
        let validator: Validator = Bytes::from(vec![0xAA; 32]);
        let seq_num = 7;
        let invalid_block_a: BlockHash = Bytes::from(vec![0x11; 32]);
        let invalid_block_b: BlockHash = Bytes::from(vec![0x22; 32]);

        let seed_a =
            generate_slash_deploy_random_seed(validator.clone(), seq_num, &invalid_block_a);
        let seed_b =
            generate_slash_deploy_random_seed(validator.clone(), seq_num, &invalid_block_b);

        assert_ne!(
            seed_a.to_bytes(),
            seed_b.to_bytes(),
            "two distinct invalid_block_hashes must produce distinct slash seeds; \
             without this, multi-slash blocks emit SlashDeploys with identical rng \
             which alias the unforgeable channel names allocated by the slash contract"
        );
    }

    /// Same proposer, same seq_num, same invalid_block_hash must produce the
    /// SAME seed across calls. Replay determinism depends on this — a validator
    /// re-running a historical slash must reconstruct the exact rng state used
    /// at original execution.
    #[test]
    fn slash_seed_is_deterministic_for_same_inputs() {
        let validator: Validator = Bytes::from(vec![0xBB; 32]);
        let seq_num = 13;
        let invalid_block: BlockHash = Bytes::from(vec![0x33; 32]);

        let seed_first =
            generate_slash_deploy_random_seed(validator.clone(), seq_num, &invalid_block);
        let seed_second =
            generate_slash_deploy_random_seed(validator.clone(), seq_num, &invalid_block);

        assert_eq!(
            seed_first.to_bytes(),
            seed_second.to_bytes(),
            "same inputs must produce same seed for replay determinism"
        );
    }

    fn build_test_cosigned(n_signers: usize) -> Cosigned<DeployData> {
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        use crypto::rust::signatures::signed::{Cosigner, ToMessage};
        use prost::Message;

        let secp = Secp256k1;
        let data = DeployData {
            term: "Nil".to_string(),
            time_stamp: 1,
            phlo_price: 1,
            phlo_limit: (n_signers as i64) * 100,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
        };
        let serialized = data.to_message().encode_to_vec();
        let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
        let mut signers = Vec::with_capacity(n_signers);
        for _ in 0..n_signers {
            let (sk, pk) = secp.new_key_pair();
            let sig = secp.sign(&hash, &sk.bytes);
            signers.push(Cosigner {
                pk,
                sig: Bytes::from(sig),
                sig_algorithm: Box::new(secp.clone()),
                phlo_share: 100,
            });
        }
        Cosigned::from_signed_data(data, signers, (n_signers as i64) * 100)
            .expect("valid cosigned envelope")
    }

    /// Distinct cosigners (different `signer_index`) must produce distinct
    /// pre-charge seeds. Without this, the PoS contract's per-cosigner
    /// `chargeDeploy` calls would allocate aliasing unforgeable channel
    /// names via `new rl, poSCh, depositCh, ...` from identical rng states,
    /// corrupting tuplespace state.
    #[test]
    fn pre_charge_signer_seed_distinct_per_signer_index() {
        let cosigned = build_test_cosigned(3);
        let seed_0 = generate_pre_charge_deploy_random_seed_for_signer(&cosigned, 0);
        let seed_1 = generate_pre_charge_deploy_random_seed_for_signer(&cosigned, 1);
        let seed_2 = generate_pre_charge_deploy_random_seed_for_signer(&cosigned, 2);
        assert_ne!(seed_0.to_bytes(), seed_1.to_bytes());
        assert_ne!(seed_1.to_bytes(), seed_2.to_bytes());
        assert_ne!(seed_0.to_bytes(), seed_2.to_bytes());
    }

    /// Same cosigned envelope + same signer_index must yield the SAME seed
    /// across calls. Replay determinism depends on this.
    #[test]
    fn pre_charge_signer_seed_deterministic() {
        let cosigned = build_test_cosigned(2);
        let seed_a = generate_pre_charge_deploy_random_seed_for_signer(&cosigned, 0);
        let seed_b = generate_pre_charge_deploy_random_seed_for_signer(&cosigned, 0);
        assert_eq!(seed_a.to_bytes(), seed_b.to_bytes());
    }

    /// Pre-charge and refund seeds for the same cosigner must differ.
    /// Without this, the cosigner's pre-charge and refund would allocate
    /// aliasing channel names within the same deploy.
    #[test]
    fn pre_charge_and_refund_signer_seeds_distinct() {
        let cosigned = build_test_cosigned(2);
        let pre = generate_pre_charge_deploy_random_seed_for_signer(&cosigned, 0);
        let refund = generate_refund_deploy_random_seed_for_signer(&cosigned, 0);
        assert_ne!(pre.to_bytes(), refund.to_bytes());
    }
}
