//! Authorization of `Slash` system deploys.
//!
//! A slash carries its own evidence: two blocks one sender signed at one
//! sequence number. The proposer and every receiver apply the same rule to
//! it, and the rule reads only the evidence and the stake a slash can take
//! at the block's floor, its bonds and any stake in quarantine, so honest
//! nodes reach the same verdict whatever order they saw the equivocation in
//! and whichever of its blocks they hold.

use std::collections::{BTreeMap, HashMap, HashSet};

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, EquivocationEvidence, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::{self, Validator};
use num_bigint::BigInt;

use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::util::proto_util;
use crate::rust::validate::Validate;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SlashAuthError {
    #[error("slash deploy issuer does not match block sender (block={block_hash}, issuer={issuer}, sender={sender})")]
    IssuerMismatch {
        block_hash: String,
        issuer: String,
        sender: String,
    },
    #[error("slash evidence is not an equivocation: {reason}")]
    NotAnEquivocation { reason: &'static str },
    #[error("slash deploy target {validator} holds no stake a slash can take")]
    NothingToSlash { validator: String },
    #[error("duplicate slash deploy target in block (validator {validator})")]
    DuplicateTarget { validator: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct AuthorizedSlashCandidate {
    pub offender: Validator,
    pub evidence: EquivocationEvidence,
}

/// Predecessor of a sequence number used as the *exclusive* lower bound for
/// self-justification walks. The boundary is `seq_num <= 0`, not `<= 1`:
/// sequence 1 is a valid genesis-child and must round-trip to `Some(0)`.
pub fn checked_base_seq(seq_num: i32) -> Option<i32> {
    if seq_num <= 0 {
        None
    } else {
        Some(seq_num - 1)
    }
}

/// Successor of a `u64` sequence width, narrowed to the wire-format `i32`.
/// The double check (`u64::checked_add` then `i32::try_from`) saturates to
/// `None` on either u64 overflow or i32 truncation — silently wrapping would
/// let an attacker craft a sequence-number rollover.
pub fn checked_next_seq(max_seq: u64) -> Option<i32> {
    max_seq
        .checked_add(1)
        .and_then(|seq| i32::try_from(seq).ok())
}

/// The validator `evidence` proves equivocated on the chain `chain_id` of
/// shard `shard_id`: both commitments hash to blocks their common sender
/// signed for that chain, at one sequence number, and the two hashes differ
/// and are in order.
pub fn equivocator_proven_by(
    evidence: &EquivocationEvidence,
    shard_id: &str,
    chain_id: &[u8],
) -> Result<Validator, SlashAuthError> {
    let reject = |reason| Err(SlashAuthError::NotAnEquivocation { reason });
    let (first, second) = (&evidence.first, &evidence.second);
    if [first, second]
        .iter()
        .any(|block| block.shard_id != shard_id || block.header.chain_id != chain_id)
    {
        return reject("a block belongs to another shard or chain");
    }
    if first.sender != second.sender {
        return reject("the blocks have different senders");
    }
    if first.sender.len() != validator::LENGTH {
        return reject("the sender is not a validator key");
    }
    if first.seq_num != second.seq_num {
        return reject("the blocks have different sequence numbers");
    }
    let first_hash = proto_util::commitment_hash(first);
    let second_hash = proto_util::commitment_hash(second);
    if first_hash == second_hash {
        return reject("the two blocks are one block");
    }
    if first_hash > second_hash {
        return reject("the blocks are not ordered by hash");
    }
    if !Validate::commitment_signature(first, &first_hash)
        || !Validate::commitment_signature(second, &second_hash)
    {
        return reject("a block is not signed by its sender");
    }
    Ok(first.sender.clone())
}

/// The evidence for the equivocation `invalid` is stored with: the block and
/// its sibling, ordered by hash.
fn evidence_for(
    invalid: &BlockMetadata,
    block_store: &KeyValueBlockStore,
) -> Result<EquivocationEvidence, CasperError> {
    let sibling = invalid.equivocates.as_ref().ok_or_else(|| {
        CasperError::RuntimeError(format!(
            "invalid block {} is stored without its pair",
            hex::encode(&invalid.block_hash)
        ))
    })?;
    let commitment = |hash: &BlockHash| {
        block_store
            .get(hash)?
            .map(|block| proto_util::block_commitment(&block))
            .ok_or_else(|| {
                CasperError::RuntimeError(format!(
                    "block {} of a recorded equivocation is not in the block store",
                    hex::encode(hash)
                ))
            })
    };
    let (low, high) = if invalid.block_hash < *sibling {
        (&invalid.block_hash, sibling)
    } else {
        (sibling, &invalid.block_hash)
    };
    Ok(EquivocationEvidence {
        first: commitment(low)?,
        second: commitment(high)?,
    })
}

/// The slashes the block being built issues: one per offender this node
/// holds an equivocation of and `slashable` holds stake for. Where an
/// offender has several, the one recorded on the lowest block hash is used,
/// so every node holding the same equivocations issues the same slashes.
pub fn authorized_slash_candidates(
    snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
    slashable: &HashMap<Validator, BigInt>,
) -> Result<Vec<AuthorizedSlashCandidate>, CasperError> {
    let mut by_offender: BTreeMap<Validator, BlockMetadata> = BTreeMap::new();
    for offender in slashable.keys() {
        let lowest = snapshot
            .dag
            .equivocations_of(offender)?
            .into_iter()
            .min_by(|left, right| left.block_hash.cmp(&right.block_hash));
        if let Some(invalid) = lowest {
            by_offender.insert(offender.clone(), invalid);
        }
    }
    by_offender
        .into_iter()
        .map(|(offender, invalid)| {
            Ok(AuthorizedSlashCandidate {
                offender,
                evidence: evidence_for(&invalid, block_store)?,
            })
        })
        .collect()
}

/// Every succeeded `Slash` system deploy in `block` must be issued by the
/// block's sender, carry evidence that proves an equivocation on the chain
/// `chain_id` of shard `shard_id`, target a validator `slashable` holds stake
/// for, and be the block's only slash of that validator. Any violation
/// rejects the block as `InvalidBlock::UnauthorizedSlashDeploy`.
pub fn validate_received_slash_deploys(
    block: &BlockMessage,
    slashable: &HashMap<Validator, BigInt>,
    shard_id: &str,
    chain_id: &[u8],
) -> Result<(), CasperError> {
    let mut slashed = HashSet::new();
    for system_deploy in &block.body.system_deploys {
        let ProcessedSystemDeploy::Succeeded {
            system_deploy:
                SystemDeployData::Slash {
                    evidence,
                    issuer_public_key,
                },
            ..
        } = system_deploy
        else {
            continue;
        };

        if issuer_public_key.bytes.is_empty() || issuer_public_key.bytes != block.sender {
            return Err(SlashAuthError::IssuerMismatch {
                block_hash: hex::encode(&block.block_hash),
                issuer: hex::encode(&issuer_public_key.bytes),
                sender: hex::encode(&block.sender),
            }
            .into());
        }
        let offender = equivocator_proven_by(evidence, shard_id, chain_id)?;
        if !slashable
            .get(&offender)
            .is_some_and(|stake| *stake > BigInt::from(0))
        {
            return Err(SlashAuthError::NothingToSlash {
                validator: hex::encode(&offender),
            }
            .into());
        }
        if !slashed.insert(offender.clone()) {
            return Err(SlashAuthError::DuplicateTarget {
                validator: hex::encode(&offender),
            }
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rust::block_implicits::get_random_block_default;
    use models::rust::casper::protocol::casper_message::BlockCommitment;

    use super::*;

    const SHARD: &str = "root";
    const CHAIN: [u8; 32] = [0xC1; 32];

    /// A block of `shard` and `chain` at `seq_num` signed by the key
    /// `secret`, distinguished by `nonce`.
    fn signed_for(
        secret: &PrivateKey,
        seq_num: i32,
        nonce: u8,
        shard: &str,
        chain: &[u8],
    ) -> BlockCommitment {
        let secp = Secp256k1;
        let mut block = get_random_block_default();
        block.sender = secp.to_public(secret).bytes.into();
        block.sig_algorithm = Secp256k1::name();
        block.seq_num = seq_num;
        block.extra_bytes = vec![nonce].into();
        block.shard_id = shard.to_string();
        block.header.chain_id = chain.to_vec().into();
        let hash = proto_util::hash_block(&block);
        block.sig = secp.sign(&hash, &secret.bytes).into();
        proto_util::block_commitment(&block)
    }

    fn signed_commitment(secret: &PrivateKey, seq_num: i32, nonce: u8) -> BlockCommitment {
        signed_for(secret, seq_num, nonce, SHARD, &CHAIN)
    }

    fn ordered(a: BlockCommitment, b: BlockCommitment) -> EquivocationEvidence {
        if proto_util::commitment_hash(&a) < proto_util::commitment_hash(&b) {
            EquivocationEvidence {
                first: a,
                second: b,
            }
        } else {
            EquivocationEvidence {
                first: b,
                second: a,
            }
        }
    }

    fn key(byte: u8) -> PrivateKey { PrivateKey::from_bytes(&[byte; 32]) }

    #[test]
    fn two_signed_blocks_at_one_sequence_number_prove_their_sender() {
        let secret = key(0x11);
        let evidence = ordered(
            signed_commitment(&secret, 4, 1),
            signed_commitment(&secret, 4, 2),
        );
        assert_eq!(
            equivocator_proven_by(&evidence, SHARD, &CHAIN),
            Ok(evidence.first.sender.clone())
        );
    }

    #[test]
    fn evidence_that_is_not_an_equivocation_is_rejected() {
        let secret = key(0x11);
        let a = signed_commitment(&secret, 4, 1);
        let b = signed_commitment(&secret, 4, 2);
        let pair = ordered(a.clone(), b.clone());

        let same_block = EquivocationEvidence {
            first: a.clone(),
            second: a.clone(),
        };
        let different_seq = ordered(a.clone(), signed_commitment(&secret, 5, 2));
        let different_sender = ordered(a.clone(), signed_commitment(&key(0x22), 4, 2));
        let unordered = EquivocationEvidence {
            first: pair.second.clone(),
            second: pair.first.clone(),
        };
        let mut forged = pair.clone();
        forged.second.sig = signed_commitment(&key(0x22), 4, 2).sig;
        let mut altered_block = b.clone();
        altered_block.post_state_hash = vec![0xAB; 32].into();
        let altered = ordered(a.clone(), altered_block);
        let short_sender = {
            let (mut a, mut b) = (a.clone(), b.clone());
            a.sender = vec![0x04; 33].into();
            b.sender = a.sender.clone();
            ordered(a, b)
        };
        let other_shard = ordered(a.clone(), signed_for(&secret, 4, 2, "other", &CHAIN));
        let other_chain = ordered(a.clone(), signed_for(&secret, 4, 2, SHARD, &[0xC2; 32]));

        for (name, evidence, reason) in [
            (
                "a sender that is not a validator key",
                short_sender,
                "the sender is not a validator key",
            ),
            (
                "a block of another shard",
                other_shard,
                "a block belongs to another shard or chain",
            ),
            (
                "a block of another chain",
                other_chain,
                "a block belongs to another shard or chain",
            ),
            ("same block", same_block, "the two blocks are one block"),
            (
                "different sequence numbers",
                different_seq,
                "the blocks have different sequence numbers",
            ),
            (
                "different senders",
                different_sender,
                "the blocks have different senders",
            ),
            ("unordered", unordered, "the blocks are not ordered by hash"),
            (
                "forged signature",
                forged,
                "a block is not signed by its sender",
            ),
            (
                "content altered after signing",
                altered,
                "a block is not signed by its sender",
            ),
        ] {
            assert_eq!(
                equivocator_proven_by(&evidence, SHARD, &CHAIN),
                Err(SlashAuthError::NotAnEquivocation { reason }),
                "{name}"
            );
        }
    }
}

// Kani proofs — symbolic-model-check the sequence-number helpers.
// CI does not currently run kani; run locally with `cargo kani -p casper`.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    #[kani::proof]
    fn checked_base_seq_rejects_nonpositive() {
        let seq: i32 = kani::any();
        kani::assume(seq <= 0);
        assert_eq!(checked_base_seq(seq), None);
    }

    #[kani::proof]
    fn checked_base_seq_matches_positive_i32_predecessor() {
        let seq: i32 = kani::any();
        kani::assume(seq > 0);
        assert_eq!(checked_base_seq(seq), Some(seq - 1));
    }

    #[kani::proof]
    fn checked_next_seq_matches_i32_successor() {
        let seq: u64 = kani::any();
        let expected = seq.checked_add(1).and_then(|next| i32::try_from(next).ok());
        assert_eq!(checked_next_seq(seq), expected);
    }
}
