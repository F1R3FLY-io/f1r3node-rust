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
    /// The block's recorded state parent (`BlockProto.body.mergeBase`),
    /// copied verbatim at insert. Empty means the base is header-derivable:
    /// a single-parent block's base is its sole parent; genesis has none.
    /// Derivation from `parents` is the reader's job, never done here.
    #[serde(with = "shared::rust::serde_bytes", default)]
    pub merge_base: Bytes,
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
            && self.merge_base == other.merge_base
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
        self.merge_base.hash(state);
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
            merge_base: proto.merge_base,
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
            merge_base: self.merge_base.clone(),
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

    fn weight_map(state: &F1r3flyState) -> BTreeMap<Bytes, i64> {
        state
            .bonds
            .iter()
            .map(|b| (b.validator.clone(), b.stake))
            .collect()
    }

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
            merge_base: b.body.merge_base.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::hash::{DefaultHasher, Hash, Hasher};

    use super::*;
    use crate::rust::block_implicits::get_random_block_default;

    fn sample() -> BlockMetadata {
        BlockMetadata {
            block_hash: Bytes::from_static(b"hash-a"),
            parents: vec![
                Bytes::from_static(b"parent-1"),
                Bytes::from_static(b"parent-2"),
            ],
            sender: Bytes::from_static(b"sender"),
            justifications: vec![Justification {
                validator: Bytes::from_static(b"validator"),
                latest_block_hash: Bytes::from_static(b"latest"),
            }],
            weight_map: BTreeMap::from([
                (Bytes::from_static(b"v1"), 10),
                (Bytes::from_static(b"v2"), 20),
            ]),
            block_number: 7,
            sequence_number: 3,
            invalid: false,
            directly_finalized: true,
            finalized: true,
            fault_tolerance_value: 0.5,
            merge_base: Bytes::from_static(b"base"),
        }
    }

    fn hash_of(m: &BlockMetadata) -> u64 {
        let mut hasher = DefaultHasher::new();
        m.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn proto_round_trip_preserves_every_field() {
        let metadata = sample();
        let round_tripped = BlockMetadata::from_proto(metadata.to_proto());
        assert_eq!(round_tripped, metadata);
        assert_eq!(
            round_tripped.fault_tolerance_value,
            metadata.fault_tolerance_value
        );
    }

    #[test]
    fn bytes_round_trip_preserves_every_field() {
        let metadata = sample();
        let round_tripped = BlockMetadata::from_bytes(&metadata.to_bytes());
        assert_eq!(round_tripped, metadata);
        assert_eq!(
            round_tripped.fault_tolerance_value,
            metadata.fault_tolerance_value
        );
    }

    #[test]
    fn to_proto_maps_weight_map_to_bonds() {
        let proto = sample().to_proto();
        let validators: Vec<&[u8]> = proto.bonds.iter().map(|b| b.validator.as_ref()).collect();
        let stakes: Vec<i64> = proto.bonds.iter().map(|b| b.stake).collect();
        assert_eq!(validators, vec![b"v1".as_ref(), b"v2".as_ref()]);
        assert_eq!(stakes, vec![10, 20]);
    }

    #[test]
    fn ordering_by_num_orders_by_block_number_first() {
        let mut low = sample();
        low.block_number = 1;
        let mut high = sample();
        high.block_number = 2;
        assert_eq!(BlockMetadata::ordering_by_num(&low, &high), Ordering::Less);
        assert_eq!(
            BlockMetadata::ordering_by_num(&high, &low),
            Ordering::Greater
        );
    }

    #[test]
    fn ordering_by_num_breaks_ties_by_block_hash() {
        let mut a = sample();
        a.block_hash = Bytes::from_static(b"aaa");
        let mut b = sample();
        b.block_hash = Bytes::from_static(b"bbb");
        assert_eq!(BlockMetadata::ordering_by_num(&a, &b), Ordering::Less);
        assert_eq!(BlockMetadata::ordering_by_num(&a, &a), Ordering::Equal);
    }

    #[test]
    fn from_block_copies_block_fields_and_defaults_flags() {
        let block = get_random_block_default();
        let metadata = BlockMetadata::from_block(&block, true, None, None);

        assert_eq!(metadata.block_hash, block.block_hash);
        assert_eq!(metadata.parents, block.header.parents_hash_list);
        assert_eq!(metadata.sender, block.sender);
        assert_eq!(metadata.justifications, block.justifications);
        assert_eq!(metadata.block_number, block.body.state.block_number);
        assert_eq!(metadata.sequence_number, block.seq_num);
        assert_eq!(metadata.merge_base, block.body.merge_base);
        assert!(metadata.invalid);
        assert!(!metadata.directly_finalized);
        assert!(!metadata.finalized);
        assert_eq!(metadata.fault_tolerance_value, 0.0);

        for bond in &block.body.state.bonds {
            assert_eq!(metadata.weight_map.get(&bond.validator), Some(&bond.stake));
        }
        assert_eq!(metadata.weight_map.len(), block.body.state.bonds.len());
    }

    #[test]
    fn from_block_honors_explicit_finalization_flags() {
        let block = get_random_block_default();
        let metadata = BlockMetadata::from_block(&block, false, Some(true), Some(true));
        assert!(metadata.directly_finalized);
        assert!(metadata.finalized);
    }

    #[test]
    fn equality_and_hash_ignore_fault_tolerance_value() {
        let a = sample();
        let mut b = sample();
        b.fault_tolerance_value = -1.0;
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    #[test]
    fn equality_distinguishes_block_hash() {
        let a = sample();
        let mut b = sample();
        b.block_hash = Bytes::from_static(b"other");
        assert_ne!(a, b);
        assert_ne!(hash_of(&a), hash_of(&b));
    }
}
