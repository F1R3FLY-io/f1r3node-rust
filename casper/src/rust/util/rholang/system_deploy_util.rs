// See casper/src/main/scala/coop/rchain/casper/util/rholang/SystemDeployUtil.scala

use byteorder::{LittleEndian, WriteBytesExt};
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rust::block_hash::BlockHash;
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

/// Per-redemption random seed for the Cost-Accounted Rho Stage-C validator
/// redemption system deploy (DR-7/DR-12). The redeem deploy's `source()`
/// allocates its own private channels (`new ...`) from this rng, so the seed
/// must be distinct per (validator, seq_num, outcome) to avoid aliasing those
/// unforgeable names within a block, and identical across play/replay for the
/// same inputs.
///
/// Derivation: a domain-tagged preimage
/// `b"redeem:v1" || SYSTEM_DEPLOY_PREFIX || validator || seq_num || outcome_tag`,
/// hashed via `Tools::rng` and `split_byte(3)`. The domain tag `b"redeem:v1"`
/// and the distinct `split_byte(3)` keep this seed family disjoint from the
/// close-block (`split_byte(0)`), slash (`split_byte(1)`), and epoch-mint
/// (`split_byte(2)`) families. Like the slash/epoch-mint seeds it never derives
/// from a signature (which can be empty on the threshold path) — the validator
/// pk, sequence number, and outcome tag are always present and replay-stable.
pub fn generate_redeem_deploy_random_seed(
    validator: Validator,
    seq_num: i32,
    outcome_tag: &str,
) -> Blake2b512Random {
    const REDEEM_DOMAIN: &[u8] = b"redeem:v1";
    let prefix = serialize_int32_fixed(SYSTEM_DEPLOY_PREFIX);
    let seq = serialize_int32_fixed(seq_num);
    let outcome = outcome_tag.as_bytes();
    let mut seed: Vec<u8> = Vec::with_capacity(
        REDEEM_DOMAIN.len() + prefix.len() + validator.len() + seq.len() + outcome.len(),
    );
    seed.extend_from_slice(REDEEM_DOMAIN);
    seed.extend(prefix);
    seed.extend(validator);
    seed.extend(seq);
    seed.extend_from_slice(outcome);
    Tools::rng(&seed).split_byte(3)
}

/// Per-validator, per-sequence-number random seed for the epoch/bond
/// phlogiston mint system deploy (Cost-Accounted Rho, spec Appendix B / §4.7;
/// DR-13). The epoch mint runs once per epoch boundary per validator; the
/// mint deploy's `source()` allocates its own private channels (`new ...`)
/// from this rng, so the seed must be distinct per (validator, seq_num) to
/// avoid aliasing those unforgeable names across validators within a block,
/// and identical across play/replay for the same inputs.
///
/// Derivation: a domain-tagged preimage
/// `b"epoch-mint:v1" || SYSTEM_DEPLOY_PREFIX || validator || seq_num`, hashed
/// via `Tools::rng` and `split_byte(2)`. The domain tag `b"epoch-mint:v1"`
/// and the distinct `split_byte(2)` keep this seed family disjoint from the
/// close-block (`split_byte(0)`), slash (`split_byte(1)`), and per-cosigner
/// pre-charge/refund seed families, so a mint can never alias the channels
/// allocated by any other system deploy in the same block. The disjointness
/// of the wallet/quarantine/funding-slot seed DOMAINS is proved in
/// `formal/rocq/cost_accounted_rho/theories/WalletNaming.v`
/// (`wallet_quarantine_domain_disjoint`, `wallet_funding_slot_domain_disjoint`,
/// `quarantine_funding_slot_domain_disjoint`).
///
/// We deliberately do NOT derive any name from `cosigned.primary().sig`: the
/// primary signature can be EMPTY on the threshold/placeholder construction
/// path (`signed.rs` `from_signed_data_threshold`), which would collide
/// distinct validators' seeds and fork consensus. The validator public key
/// and the sequence number are always present and replay-stable, so they are
/// a safe basis (mirrors `deploy_group_id`'s pk-set rationale and the
/// slash-seed `invalid_block_hash` rationale above).
pub fn generate_epoch_mint_deploy_random_seed(
    validator: Validator,
    seq_num: i32,
) -> Blake2b512Random {
    const EPOCH_MINT_DOMAIN: &[u8] = b"epoch-mint:v1";
    let prefix = serialize_int32_fixed(SYSTEM_DEPLOY_PREFIX);
    let seq = serialize_int32_fixed(seq_num);
    let mut seed: Vec<u8> =
        Vec::with_capacity(EPOCH_MINT_DOMAIN.len() + prefix.len() + validator.len() + seq.len());
    seed.extend_from_slice(EPOCH_MINT_DOMAIN);
    seed.extend(prefix);
    seed.extend(validator);
    seed.extend(seq);
    Tools::rng(&seed).split_byte(2)
}

// D3 (DR-9, OD-2): the pre-charge / refund random-seed derivations
// (`generate_pre_charge_deploy_random_seed[_for_signer]`,
// `generate_refund_deploy_random_seed[_for_signer]`) and the
// `deploy_group_id` PoS charge-tracking channel id are REMOVED with the
// per-deploy escrow model. A deploy's cost is the per-COMM token count,
// settled once against Σ⟦s⟧ at block close (no pre-charge/refund system
// deploys, hence no per-deploy/per-cosigner seeds and no charge-tracking
// channel). The close-block / slash / redeem / epoch-mint seed families
// below are retained (they name DISTINCT system deploys).

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

    /// Two distinct validators, same seq_num, must produce distinct epoch-mint
    /// seeds. The epoch mint deploy allocates `new ...` channels from this rng;
    /// without per-validator distinctness, two validators' mints in the same
    /// block would alias those unforgeable channel names and fork consensus.
    #[test]
    fn epoch_mint_seed_differs_per_validator() {
        let validator_a: Validator = Bytes::from(vec![0xA1; 32]);
        let validator_b: Validator = Bytes::from(vec![0xB2; 32]);
        let seq_num = 5;

        let seed_a = generate_epoch_mint_deploy_random_seed(validator_a, seq_num);
        let seed_b = generate_epoch_mint_deploy_random_seed(validator_b, seq_num);

        assert_ne!(
            seed_a.to_bytes(),
            seed_b.to_bytes(),
            "distinct validators must produce distinct epoch-mint seeds"
        );
    }

    /// Same validator, distinct seq_nums, must produce distinct epoch-mint
    /// seeds (one mint per epoch boundary; the seq_num advances per block).
    #[test]
    fn epoch_mint_seed_differs_per_seq_num() {
        let validator: Validator = Bytes::from(vec![0xC3; 32]);

        let seed_1 = generate_epoch_mint_deploy_random_seed(validator.clone(), 1);
        let seed_2 = generate_epoch_mint_deploy_random_seed(validator, 2);

        assert_ne!(
            seed_1.to_bytes(),
            seed_2.to_bytes(),
            "distinct seq_nums must produce distinct epoch-mint seeds"
        );
    }

    /// Same validator + same seq_num must produce the SAME epoch-mint seed
    /// across calls. Replay determinism depends on this — a validator
    /// re-running a historical epoch mint must reconstruct the exact rng state.
    #[test]
    fn epoch_mint_seed_is_deterministic_for_same_inputs() {
        let validator: Validator = Bytes::from(vec![0xD4; 32]);
        let seq_num = 11;

        let seed_first = generate_epoch_mint_deploy_random_seed(validator.clone(), seq_num);
        let seed_second = generate_epoch_mint_deploy_random_seed(validator, seq_num);

        assert_eq!(
            seed_first.to_bytes(),
            seed_second.to_bytes(),
            "same inputs must produce same epoch-mint seed for replay determinism"
        );
    }

    /// The epoch-mint seed family must be DISJOINT from the slash and
    /// close-block seed families for the SAME (validator, seq_num): the
    /// domain tag `b"epoch-mint:v1"` + `split_byte(2)` ensure a mint never
    /// aliases the channels allocated by a slash (`split_byte(1)`) or a
    /// close-block (`split_byte(0)`) in the same block.
    #[test]
    fn epoch_mint_seed_disjoint_from_slash_and_close_block() {
        let validator: Validator = Bytes::from(vec![0xE5; 32]);
        let seq_num = 3;
        let invalid_block: BlockHash = Bytes::from(vec![0x55; 32]);

        let mint = generate_epoch_mint_deploy_random_seed(validator.clone(), seq_num);
        let slash =
            generate_slash_deploy_random_seed(validator.clone(), seq_num, &invalid_block);
        let close = generate_close_deploy_random_seed_from_validator(validator, seq_num);

        assert_ne!(
            mint.to_bytes(),
            slash.to_bytes(),
            "epoch-mint and slash seed families must be disjoint"
        );
        assert_ne!(
            mint.to_bytes(),
            close.to_bytes(),
            "epoch-mint and close-block seed families must be disjoint"
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

    // D3 (DR-9, OD-2): the pre-charge / refund per-signer-seed tests and the
    // `deploy_group_id` tests (`build_test_cosigned`,
    // `pre_charge_signer_seed_*`, `pre_charge_and_refund_signer_seeds_distinct`,
    // `deploy_group_id_*`) are removed with the escrow pre-charge/refund system
    // deploys they exercised. The retained slash / epoch-mint seed tests above
    // cover the remaining system-deploy seed families.
}
