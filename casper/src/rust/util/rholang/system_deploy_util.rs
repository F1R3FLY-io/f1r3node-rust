// See casper/src/main/scala/coop/rchain/casper/util/rholang/SystemDeployUtil.scala

use byteorder::{LittleEndian, WriteBytesExt};
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::{Cosigned, Signed, ToMessage};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::validator::Validator;
use prost::Message;

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

/// Per-deploy-group identifier for the PoS charge-tracking state channel
/// (M1 of the multi-parent-merge fix).
///
/// The PoS `chargeDeploy`/`refundDeploy` contracts track in-flight charges
/// on a content-addressed channel `@(*posDeployStateTag, deployGroupId)`.
/// In multi-parent merge, the content-addressed merge engine identifies a
/// `Produce` by `hash(channel ++ datum ++ persist)`. A single
/// genesis-scoped channel seeded once with `{}` makes every branch's first
/// charge consume the SAME base produce, so independent deploys are flagged
/// as conflicting/dependent. Scoping the channel by `deployGroupId` gives
/// distinct deploys distinct channel hashes (hence distinct `Produce`
/// identities) even when the tracked value is an identical `{}`.
///
/// Derivation:
/// `blake2b256( concat(sorted(signer.pk.bytes)) ++ deploy_data_serialized )`
/// where `deploy_data_serialized = cosigned.data().to_message().encode_to_vec()`
/// (the SAME serialization used for signing — see
/// `Signed::signature_hash` and `mixed_algorithm_cosigned_test::sign_with`).
/// The pk byte-vectors are sorted lexicographically before concatenation.
///
/// Properties (all required for consensus correctness):
/// - **Non-empty:** Blake2b256 always yields 32 bytes.
/// - **Identical across a deploy's cosigners:** the input depends only on
///   the cosigner pk SET and the deploy payload, both shared by every
///   cosigner of a deploy. (Cosigner order does not matter because the
///   pks are sorted here; the `Cosigned` constructor already canonicalizes
///   signer order, so sorting is belt-and-braces.)
/// - **Distinct across deploys:** distinct payloads (term/timestamp/…) or
///   distinct signer sets change the hash preimage.
/// - **Play == replay:** computed from `(signer pk set, payload)` only,
///   both of which round-trip identically through `ProcessedDeploy`/
///   `to_cosigned()`.
///
/// We deliberately do NOT use `cosigned.primary().sig`: on the
/// threshold/placeholder construction path the primary may be a placeholder
/// whose `sig` is EMPTY (`signed.rs` `from_signed_data_threshold`), which
/// would collide distinct deploys. Public keys are always present (even for
/// placeholder signers), so the pk set is a safe, replay-stable basis.
pub fn deploy_group_id(cosigned: &Cosigned<DeployData>) -> Vec<u8> {
    let mut pk_bytes: Vec<&[u8]> = cosigned
        .signers()
        .iter()
        .map(|s| s.pk.bytes.as_ref())
        .collect();
    pk_bytes.sort_unstable();

    let deploy_data_serialized = cosigned.data().to_message().encode_to_vec();

    // Length-prefixed, count-delimited preimage so that distinct
    // (signer-set, payload) inputs can NEVER share a preimage. Public keys
    // vary in length across signature algorithms (ed25519 = 32 bytes,
    // secp256k1 = 33/65), so a bare concatenation could be ambiguous at the
    // pk/pk and pk/payload boundaries; the u32 length prefixes make the
    // encoding injective, so the group id is provably unique per deploy — a
    // consensus channel key must not collide across distinct deploys.
    let total_pk_len: usize = pk_bytes.iter().map(|b| b.len()).sum();
    let mut preimage = Vec::with_capacity(
        4 + pk_bytes.len() * 4 + total_pk_len + 4 + deploy_data_serialized.len(),
    );
    preimage.extend_from_slice(&(pk_bytes.len() as u32).to_be_bytes());
    for pk in &pk_bytes {
        preimage.extend_from_slice(&(pk.len() as u32).to_be_bytes());
        preimage.extend_from_slice(pk);
    }
    preimage.extend_from_slice(&(deploy_data_serialized.len() as u32).to_be_bytes());
    preimage.extend_from_slice(&deploy_data_serialized);

    Blake2b256::hash(preimage)
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

    /// `deploy_group_id` must be non-empty (Blake2b256 → 32 bytes) and
    /// deterministic for the same envelope. Deploy-group scoping of the PoS
    /// charge channel depends on a stable, non-trivial id.
    #[test]
    fn deploy_group_id_non_empty_and_deterministic() {
        let cosigned = build_test_cosigned(2);
        let a = deploy_group_id(&cosigned);
        let b = deploy_group_id(&cosigned);
        assert_eq!(a.len(), 32, "blake2b256 digest is 32 bytes");
        assert_eq!(a, b, "same envelope must yield same group id (replay determinism)");
    }

    /// Every cosigner of a SINGLE deploy must derive the SAME group id, so
    /// all of a deploy's charges/refunds share one channel (dedup/cap/refund
    /// rely on this). The id is a pure function of the envelope, so this is
    /// trivially true — assert it explicitly as a regression guard against a
    /// future per-signer derivation creeping in.
    #[test]
    fn deploy_group_id_identical_across_cosigners_of_one_deploy() {
        let cosigned = build_test_cosigned(3);
        // The runtime calls deploy_group_id(&cosigned) ONCE and reuses it for
        // every signer; emulate by recomputing and comparing.
        let id_for_signer_0 = deploy_group_id(&cosigned);
        let id_for_signer_2 = deploy_group_id(&cosigned);
        assert_eq!(id_for_signer_0, id_for_signer_2);
    }

    /// Two DISTINCT deploys (different payload → different signer keypairs +
    /// different phlo_limit) must derive DISTINCT group ids; otherwise the
    /// merge engine would still see colliding `Produce` identities.
    #[test]
    fn deploy_group_id_distinct_across_deploys() {
        let cosigned_a = build_test_cosigned(2);
        let cosigned_b = build_test_cosigned(2);
        // build_test_cosigned generates fresh keypairs each call, so the
        // signer sets differ; the ids must differ.
        assert_ne!(deploy_group_id(&cosigned_a), deploy_group_id(&cosigned_b));
    }

    /// The id must be independent of the INPUT signer order (the constructor
    /// canonicalizes, and we also sort here). Two envelopes with the same
    /// signer set submitted in different order must share an id.
    #[test]
    fn deploy_group_id_invariant_under_signer_permutation() {
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        use crypto::rust::signatures::signed::{Cosigner, ToMessage};
        use prost::bytes::Bytes;
        use prost::Message;

        let secp = Secp256k1;
        let data = DeployData {
            term: "Nil".to_string(),
            time_stamp: 42,
            phlo_price: 1,
            phlo_limit: 200,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
        };
        let serialized = data.to_message().encode_to_vec();
        let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
        let mut signers = Vec::with_capacity(2);
        for _ in 0..2 {
            let (sk, pk) = secp.new_key_pair();
            let sig = secp.sign(&hash, &sk.bytes);
            signers.push(Cosigner {
                pk,
                sig: Bytes::from(sig),
                sig_algorithm: Box::new(secp.clone()),
                phlo_share: 100,
            });
        }
        let forward = signers.clone();
        let mut reversed = signers;
        reversed.reverse();

        let cosigned_a = Cosigned::from_signed_data(data.clone(), forward, 200).expect("valid");
        let cosigned_b = Cosigned::from_signed_data(data, reversed, 200).expect("valid");
        assert_eq!(deploy_group_id(&cosigned_a), deploy_group_id(&cosigned_b));
    }
}
