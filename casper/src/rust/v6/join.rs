use std::collections::{HashMap, HashSet};

use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;

use super::block_record::BlockRecord;
use super::body::{StateHash, V6Records};
use super::record::ExecutionRecord;
use crate::rust::validate::Validate;

#[derive(Clone, Debug, Default)]
pub struct JoinProof {
    pub anchor_root: StateHash,
    pub citers: Vec<BlockMessage>,
    pub attesters: Vec<BlockMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinRefusal {
    BadBlock(BlockHash),
    NoAnchorRecord,
    NotBonded(Bytes),
    NoQuorum { attested: i128, bonded: i128 },
}

impl std::fmt::Display for JoinRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JoinRefusal::BadBlock(hash) => write!(f, "block {} has a bad hash or signature", hex::encode(hash)),
            JoinRefusal::NoAnchorRecord => write!(f, "no cited record fixes the LFB at the anchor root"),
            JoinRefusal::NotBonded(sender) => write!(f, "attester {} is not bonded", hex::encode(sender)),
            JoinRefusal::NoQuorum { attested, bonded } => {
                write!(f, "{attested} of {bonded} bonded stake attests the anchor chain")
            }
        }
    }
}

fn sound(block: &BlockMessage) -> bool {
    Validate::block_signature(block)
        && matches!(
            Validate::block_hash(block),
            rspace_plus_plus::rspace::history::Either::Right(_)
        )
}

fn records(citers: &[BlockMessage]) -> Vec<BlockRecord> {
    citers
        .iter()
        .filter_map(|block| block.body.v6.as_ref())
        .filter_map(|body| body.decode_records::<BlockRecord>().ok())
        .flatten()
        .collect()
}

fn chain(lfb: &BlockHash, anchor_root: &StateHash, records: &[BlockRecord]) -> Option<HashMap<StateHash, BlockHash>> {
    records
        .iter()
        .find(|record| record.target() == lfb && record.post_root() == anchor_root)?;
    let by_pre: HashMap<&StateHash, &BlockRecord> = records
        .iter()
        .map(|record| (record.pre_root(), record))
        .collect();
    let mut roots = HashMap::from([(anchor_root.clone(), lfb.clone())]);
    let mut root = anchor_root.clone();
    while let Some(next) = by_pre.get(&root) {
        if next.post_root() == &root || roots.contains_key(next.post_root()) {
            break;
        }
        root = next.post_root().clone();
        roots.insert(root.clone(), next.target().clone());
    }
    Some(roots)
}

pub fn verify_join(
    lfb: &BlockHash,
    proof: &JoinProof,
    bonds: &[(Bytes, i64)],
) -> Result<StateHash, JoinRefusal> {
    for block in proof.citers.iter().chain(&proof.attesters) {
        if !sound(block) {
            return Err(JoinRefusal::BadBlock(block.block_hash.clone()));
        }
    }
    let roots = chain(lfb, &proof.anchor_root, &records(&proof.citers))
        .ok_or(JoinRefusal::NoAnchorRecord)?;
    let stake: HashMap<&Bytes, i128> = bonds
        .iter()
        .filter(|(_, stake)| *stake > 0)
        .map(|(validator, stake)| (validator, *stake as i128))
        .collect();
    let bonded: i128 = stake.values().sum();
    let mut counted = HashSet::new();
    let mut attested = 0i128;
    for block in &proof.attesters {
        let Some(attestation) = block.body.v6.as_ref().and_then(|body| body.attestation.as_ref()) else {
            continue;
        };
        if roots.get(&attestation.root) != Some(&attestation.block_hash) {
            continue;
        }
        let Some(weight) = stake.get(&block.sender) else {
            return Err(JoinRefusal::NotBonded(block.sender.clone()));
        };
        if counted.insert(block.sender.clone()) {
            attested += weight;
        }
    }
    if 3 * attested > 2 * bonded {
        Ok(proof.anchor_root.clone())
    } else {
        Err(JoinRefusal::NoQuorum { attested, bonded })
    }
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rust::casper::protocol::casper_message::{
        Attestation, Body, F1r3flyState, Header, V6Body,
    };

    use super::*;
    use crate::rust::util::proto_util;
    use crate::rust::validator_identity::ValidatorIdentity;

    fn h(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    fn rec(target: &str, pre: &str, post: &str) -> BlockRecord {
        BlockRecord::empty(h(target), h(pre), h(post))
    }

    fn signed(key: &PrivateKey, seq: i32, attestation: (&str, &str), records: &[BlockRecord]) -> BlockMessage {
        let body = Body {
            state: F1r3flyState {
                pre_state_hash: h(attestation.1),
                post_state_hash: h(attestation.1),
                bonds: vec![],
                block_number: 10 + seq as i64,
            },
            deploys: vec![],
            rejected_deploys: vec![],
            system_deploys: vec![],
            extra_bytes: Bytes::new(),
            applied_from_scope: vec![],
            merge_base: Bytes::new(),
            v6: Some(V6Body::with_records(
                Attestation {
                    block_hash: h(attestation.0),
                    root: h(attestation.1),
                },
                records,
            )),
        };
        let header = Header {
            parents_hash_list: vec![],
            timestamp: seq as i64,
            version: 6,
            extra_bytes: Bytes::new(),
        };
        let unsigned = proto_util::unsigned_block_proto(body, header, vec![], "root".into(), Some(seq));
        ValidatorIdentity::new(key).sign_block(&unsigned)
    }

    struct Shard {
        keys: Vec<PrivateKey>,
        bonds: Vec<(Bytes, i64)>,
    }

    fn shard() -> Shard {
        let pairs: Vec<_> = (0..3).map(|_| Secp256k1.new_key_pair()).collect();
        Shard {
            bonds: pairs.iter().map(|(_, pk)| (pk.bytes.clone(), 10)).collect(),
            keys: pairs.into_iter().map(|(sk, _)| sk).collect(),
        }
    }

    fn honest(s: &Shard) -> JoinProof {
        let citer = signed(&s.keys[0], 1, ("b0", "r0"), &[rec("L", "r0", "rL"), rec("b2", "rL", "r2")]);
        JoinProof {
            anchor_root: h("rL"),
            citers: vec![citer],
            attesters: vec![
                signed(&s.keys[0], 2, ("L", "rL"), &[]),
                signed(&s.keys[1], 2, ("b2", "r2"), &[]),
                signed(&s.keys[2], 2, ("L", "rL"), &[]),
            ],
        }
    }

    #[test]
    fn an_honest_proof_anchors_the_joiner_at_the_lfb_root() {
        let s = shard();
        assert_eq!(verify_join(&h("L"), &honest(&s), &s.bonds), Ok(h("rL")));
    }

    #[test]
    fn a_forged_quorum_is_rejected() {
        let s = shard();
        let mut short = honest(&s);
        short.attesters.pop();
        assert!(matches!(verify_join(&h("L"), &short, &s.bonds), Err(JoinRefusal::NoQuorum { .. })));

        let mut wrong_root = honest(&s);
        wrong_root.attesters[2] = signed(&s.keys[2], 2, ("L", "forged"), &[]);
        assert!(matches!(verify_join(&h("L"), &wrong_root, &s.bonds), Err(JoinRefusal::NoQuorum { .. })));

        let mut twice = honest(&s);
        twice.attesters[2] = signed(&s.keys[0], 3, ("b2", "r2"), &[]);
        assert!(matches!(verify_join(&h("L"), &twice, &s.bonds), Err(JoinRefusal::NoQuorum { .. })));

        let mut tampered = honest(&s);
        tampered.attesters[1].sender = tampered.attesters[0].sender.clone();
        assert!(matches!(verify_join(&h("L"), &tampered, &s.bonds), Err(JoinRefusal::BadBlock(_))));

        let stranger = Secp256k1.new_key_pair().0;
        let mut unbonded = honest(&s);
        unbonded.attesters[1] = signed(&stranger, 2, ("b2", "r2"), &[]);
        assert!(matches!(verify_join(&h("L"), &unbonded, &s.bonds), Err(JoinRefusal::NotBonded(_))));
    }

    #[test]
    fn a_forged_anchor_root_is_rejected() {
        let s = shard();
        let mut forged = honest(&s);
        forged.anchor_root = h("rX");
        forged.citers.push(signed(&s.keys[1], 5, ("b0", "r0"), &[rec("L", "r0", "rX")]));
        assert!(matches!(verify_join(&h("L"), &forged, &s.bonds), Err(JoinRefusal::NoQuorum { .. })));
        let mut missing = honest(&s);
        missing.citers.clear();
        assert_eq!(verify_join(&h("L"), &missing, &s.bonds), Err(JoinRefusal::NoAnchorRecord));
    }
}
