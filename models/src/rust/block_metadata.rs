// See models/src/main/scala/coop/rchain/models/BlockMetadata.scala

use std::cmp::Ordering;
use std::collections::BTreeMap;

use prost::bytes::Bytes;
use prost::Message;

use super::casper::protocol::casper_message::{BlockMessage, F1r3flyState, Justification};
use crate::casper::{BlockMetadataInternal, BondProto};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BlockMetadata {
    #[serde(with = "shared::rust::serde_bytes")]
    pub block_hash: Bytes,
    #[serde(with = "shared::rust::serde_vec_bytes")]
    pub parents: Vec<Bytes>,
    #[serde(with = "shared::rust::serde_bytes")]
    pub sender: Bytes,
    pub justifications: Vec<Justification>,
    #[serde(with = "shared::rust::serde_btreemap_bytes_i64")]
    pub weight_map: BTreeMap<Bytes, i64>,
    pub block_number: i64,
    pub sequence_number: i32,
    pub invalid: bool,
    pub directly_finalized: bool,
    pub finalized: bool,
    pub fault_tolerance_value: f32,
    /// Active (post-quarantine) validators at this block's post-state — a subset of the keys in
    /// `weight_map`. The finality safety oracle weights by these (see `active_weight_map`) so a
    /// just-bonded validator still in quarantine cannot dilute the finalization quorum. Derived
    /// data, like `fault_tolerance_value`: excluded from `PartialEq`/`Hash` (identity is the hash).
    #[serde(with = "shared::rust::serde_vec_bytes", default)]
    pub active_validators: Vec<Bytes>,
}

impl PartialEq for BlockMetadata {
    fn eq(&self, other: &Self) -> bool {
        self.block_hash == other.block_hash
            && self.parents == other.parents
            && self.sender == other.sender
            && self.justifications == other.justifications
            && self.weight_map == other.weight_map
            && self.block_number == other.block_number
            && self.sequence_number == other.sequence_number
            && self.invalid == other.invalid
            && self.directly_finalized == other.directly_finalized
            && self.finalized == other.finalized
    }
}

impl Eq for BlockMetadata {}

impl std::hash::Hash for BlockMetadata {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.block_hash.hash(state);
        self.parents.hash(state);
        self.sender.hash(state);
        self.justifications.hash(state);
        self.weight_map.iter().for_each(|(k, v)| {
            k.hash(state);
            v.hash(state);
        });
        self.block_number.hash(state);
        self.sequence_number.hash(state);
        self.invalid.hash(state);
        self.directly_finalized.hash(state);
        self.finalized.hash(state);
    }
}

impl BlockMetadata {
    pub fn from_proto(proto: BlockMetadataInternal) -> Self {
        BlockMetadata {
            block_hash: proto.block_hash,
            parents: proto.parents,
            sender: proto.sender,
            justifications: proto
                .justifications
                .into_iter()
                .map(|j| Justification::from_proto(j))
                .collect(),
            weight_map: proto
                .bonds
                .into_iter()
                .map(|b| (b.validator.into(), b.stake))
                .collect(),
            block_number: proto.block_num,
            sequence_number: proto.seq_num,
            invalid: proto.invalid,
            directly_finalized: proto.directly_finalized,
            finalized: proto.finalized,
            fault_tolerance_value: proto.fault_tolerance_value,
            active_validators: proto.active_validators,
        }
    }

    pub fn to_proto(&self) -> BlockMetadataInternal {
        BlockMetadataInternal {
            block_hash: self.block_hash.clone(),
            parents: self.parents.clone(),
            sender: self.sender.clone(),
            justifications: self.justifications.iter().map(|j| j.to_proto()).collect(),
            bonds: self
                .weight_map
                .iter()
                .map(|(v, s)| BondProto {
                    validator: v.clone(),
                    stake: *s,
                })
                .collect(),
            block_num: self.block_number,
            seq_num: self.sequence_number,
            invalid: self.invalid,
            directly_finalized: self.directly_finalized,
            finalized: self.finalized,
            fault_tolerance_value: self.fault_tolerance_value,
            active_validators: self.active_validators.clone(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> { self.to_proto().encode_to_vec() }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let proto =
            BlockMetadataInternal::decode(bytes).expect("Failed to decode BlockMetadataInternal");
        Self::from_proto(proto)
    }

    fn bytes_ordering(left: &Bytes, right: &Bytes) -> Ordering { left.iter().cmp(right.iter()) }

    pub fn ordering_by_num(left: &BlockMetadata, right: &BlockMetadata) -> Ordering {
        match left.block_number.cmp(&right.block_number) {
            Ordering::Equal => Self::bytes_ordering(&left.block_hash, &right.block_hash),
            other => other,
        }
    }

    /// All bonded validators from a block's state. This is the active set at genesis (no
    /// quarantine has elapsed) and the conservative active set for test/admin inserts that do not
    /// have the rholang runtime to query the precise post-quarantine set.
    pub fn bonded_validators(b: &BlockMessage) -> Vec<Bytes> {
        b.body
            .state
            .bonds
            .iter()
            .map(|bond| bond.validator.clone())
            .collect()
    }

    fn weight_map(state: &F1r3flyState) -> BTreeMap<Bytes, i64> {
        state
            .bonds
            .iter()
            .map(|b| (b.validator.clone(), b.stake))
            .collect()
    }

    /// Builds metadata with `active_validators` defaulted to ALL bonded validators (the active set
    /// at genesis and the conservative default elsewhere). The casper layer, which owns the rholang
    /// runtime, overrides this with the precise post-quarantine active set when inserting a live
    /// block (see `BlockDagKeyValueStorage::insert_with_active`).
    pub fn from_block(
        b: &BlockMessage,
        invalid: bool,
        directly_finalized: Option<bool>,
        finalized: Option<bool>,
    ) -> Self {
        let directly_finalized = directly_finalized.unwrap_or(false);
        let finalized = finalized.unwrap_or(false);
        Self {
            block_hash: b.block_hash.clone(),
            parents: b.header.parents_hash_list.clone(),
            sender: b.sender.clone(),
            justifications: b.justifications.clone(),
            weight_map: Self::weight_map(&b.body.state),
            block_number: b.body.state.block_number,
            sequence_number: b.seq_num,
            invalid,
            // this value is not used anywhere down the call pipeline, so its safe to set it to false
            directly_finalized,
            finalized,
            fault_tolerance_value: 0.0,
            active_validators: Self::bonded_validators(b),
        }
    }

    /// The weight map restricted to active (post-quarantine) validators — the correct denominator
    /// for the finality safety oracle. Bonded-but-quarantined validators are excluded so they
    /// cannot dilute the finalization quorum (a validator's stake must not be in the denominator
    /// before it can vote). `active_validators` is always populated for blocks added through the
    /// casper layer; the empty-set guard returns the full bonds map only to preserve liveness for
    /// any pre-population path rather than zeroing the quorum.
    pub fn active_weight_map(&self) -> BTreeMap<Bytes, i64> {
        if self.active_validators.is_empty() {
            return self.weight_map.clone();
        }
        let active: std::collections::HashSet<&Bytes> = self.active_validators.iter().collect();
        self.weight_map
            .iter()
            .filter(|(validator, _)| active.contains(validator))
            .map(|(validator, stake)| (validator.clone(), *stake))
            .collect()
    }

    /// Stake of `validator` if it is active (post-quarantine), else 0 — the efficient
    /// single-validator form of `active_weight_map` for hot finality lookups. The bonded stake of
    /// an unlisted validator is intentionally 0 here (not a silent default): exclusion from the
    /// recorded active set IS the datum. Mirrors the empty-set liveness guard above.
    pub fn active_stake_of(&self, validator: &Bytes) -> i64 {
        if !self.active_validators.is_empty() && !self.active_validators.contains(validator) {
            return 0;
        }
        self.weight_map.get(validator).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod active_weight_tests {
    use super::*;

    fn v(n: u8) -> Bytes { Bytes::from(vec![n]) }

    fn meta_with(weight_map: Vec<(Bytes, i64)>, active: Vec<Bytes>) -> BlockMetadata {
        BlockMetadata {
            block_hash: Bytes::from(vec![0]),
            parents: vec![],
            sender: Bytes::new(),
            justifications: vec![],
            weight_map: weight_map.into_iter().collect(),
            block_number: 1,
            sequence_number: 0,
            invalid: false,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
            active_validators: active,
        }
    }

    // Regression guard for the finality quorum deadlock: a heavy just-bonded validator that is
    // still in quarantine must NOT count toward the finality denominator. Before the active-set
    // weighting fix the oracle used the full bonds map (total 600); the active set with v5
    // quarantined keeps the denominator at the active 300, so the 3 active validators can still
    // reach the fault-tolerance threshold and finalization advances.
    #[test]
    fn active_weight_map_excludes_quarantined_validator() {
        let m = meta_with(
            vec![(v(1), 100), (v(2), 100), (v(3), 100), (v(5), 300)],
            vec![v(1), v(2), v(3)],
        );
        let awm = m.active_weight_map();
        assert_eq!(
            awm.values().sum::<i64>(),
            300,
            "quarantined v5 must not dilute the quorum"
        );
        assert!(!awm.contains_key(&v(5)));
        assert_eq!(awm.len(), 3);
        assert_eq!(m.active_stake_of(&v(1)), 100);
        assert_eq!(
            m.active_stake_of(&v(5)),
            0,
            "quarantined validator has 0 finality weight"
        );
    }

    // Liveness guard: with no recorded active set (pre-population edge), fall back to full bonds
    // rather than producing a zero quorum.
    #[test]
    fn empty_active_set_falls_back_to_full_bonds() {
        let m = meta_with(vec![(v(1), 100), (v(2), 100)], vec![]);
        assert_eq!(m.active_weight_map().values().sum::<i64>(), 200);
        assert_eq!(m.active_stake_of(&v(1)), 100);
    }
}
