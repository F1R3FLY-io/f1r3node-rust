//! Stack-safe structural paths for RSpace matching.
//!
//! This format is a derived in-memory search key, not a protobuf, bincode, or
//! consensus identity encoding. It follows the generated matching cursor and
//! therefore covers the complete rhoapi descriptor without recursive calls.
//! `EPathMap` stays native: its token is a domain-separated BLAKE2b-256 digest
//! streamed from a logical zipper walk. It never materializes entries or
//! forces the EPM1 snapshot cache.

use crypto::rust::hash::blake2b256::Blake2b256;
use pathmap::utils::ByteMaskIter;
use pathmap::zipper::{Zipper, ZipperMoving, ZipperReadOnlyValues};
use pathmap::PathMap;
pub use rspace_plus_plus::rspace::rho_path::RHO_MATCH_PATH_VERSION;
pub(super) use rspace_plus_plus::rspace::rho_path::{
    RHO_MATCH_ABSENT as ABSENT, RHO_MATCH_BOOL as BOOL, RHO_MATCH_BYTES as BYTES,
    RHO_MATCH_BYTES_SEQUENCE as BYTES_SEQUENCE, RHO_MATCH_FIELD_END as FIELD_END,
    RHO_MATCH_FIELD_START as FIELD_START, RHO_MATCH_I32 as I32, RHO_MATCH_I64 as I64,
    RHO_MATCH_MAP_END as MAP_END, RHO_MATCH_MAP_KEY as MAP_KEY, RHO_MATCH_MAP_START as MAP_START,
    RHO_MATCH_NODE_END as NODE_END, RHO_MATCH_NODE_START as NODE_START,
    RHO_MATCH_ONEOF_END as ONEOF_END, RHO_MATCH_ONEOF_START as ONEOF_START,
    RHO_MATCH_PAR_BAG as PAR_BAG, RHO_MATCH_PATHMAP as PATHMAP,
    RHO_MATCH_SEQUENCE_END as SEQUENCE_END, RHO_MATCH_SEQUENCE_START as SEQUENCE_START,
    RHO_MATCH_STRING as STRING, RHO_MATCH_STRING_SEQUENCE as STRING_SEQUENCE, RHO_MATCH_U32 as U32,
    RHO_MATCH_U64 as U64,
};

use super::match_schema::{RhoPathCursor, RhoPathEvent, RhoScalarRef};
use super::match_schema_tables::ParCursor;
use crate::rhoapi::{
    Bundle, Connective, Expr, GUnforgeable, If, Match, New, Par, Receive, Send as RhoSend,
};
use crate::rust::epathmap_trie_codec::EPathMapRepr;
use crate::rust::par_bag::{Multiplicity, ParBag};
use crate::rust::rhoapi_ext::EPathMap;

/// Encode one `Par` as a complete-schema structural RSpace search path.
pub fn encode_par_match_path(value: &Par) -> Vec<u8> { encode_match_path(ParCursor::from(value)) }

/// Decode the sole expression of a `Par` whose authoritative component Bag
/// contains exactly that one occurrence and no other component family.
///
/// This projection is intentionally limited to the candidate-index boundary:
/// it exposes ordered structure beneath the compact ParBag token without
/// restoring a retained `Vec<Par>` representation.
pub fn is_single_expr_match_carrier(value: &Par) -> bool {
    value.family_cardinality::<RhoSend>() == 0
        && value.family_cardinality::<Receive>() == 0
        && value.family_cardinality::<New>() == 0
        && value.family_cardinality::<Expr>() == 1
        && value.family_cardinality::<Match>() == 0
        && value.family_cardinality::<GUnforgeable>() == 0
        && value.family_cardinality::<Bundle>() == 0
        && value.family_cardinality::<Connective>() == 0
        && value.family_cardinality::<If>() == 0
}

/// Candidate-index projection for a `Par` atom.
///
/// A sole expression is encoded from its generated `Expr` cursor so ordered
/// holes nested below EList/ETuple can retain a useful trie prefix. Every other
/// Par remains the compact, byte-native ParBag token. The authoritative matcher
/// and full bincode identity still operate on the complete Par.
pub fn encode_par_match_atom(value: &Par) -> Vec<u8> {
    if !is_single_expr_match_carrier(value) {
        return encode_par_match_path(value);
    }

    let mut encoded = None;
    value.visit_encoded::<Expr>(|entry| {
        debug_assert_eq!(entry.multiplicity(), Multiplicity::ONE);
        debug_assert!(encoded.is_none());
        encoded = Some(
            super::match_region_path::encode_admitted_expr_match_path(entry)
                .expect("Par invariant: an admitted Expr has canonical protobuf regions"),
        );
    });
    encoded.expect("Par invariant: sole-Expr cardinality has one resident endpoint")
}

/// Encode any generated rhoapi cursor as a complete-schema structural path.
pub fn encode_match_path(root: ParCursor<'_>) -> Vec<u8> {
    let mut output = Vec::with_capacity(256);
    output.extend_from_slice(&RHO_MATCH_PATH_VERSION);
    for event in RhoPathCursor::new(root) {
        append_match_path_event(&mut output, event);
    }
    output
}

/// Prove that two generated-schema values have the same canonical bincode
/// image without constructing that image.
///
/// The lockstep walk compares every scalar, field/sequence/map boundary, and
/// oneof discriminator. Opaque PathMap-backed fields are accepted only when
/// they retain the same immutable CoW root. A `true` result is therefore a
/// sound byte-identity witness; `false` merely means that callers must use the
/// ordinary encoder/key lookup. The walk is iterative and constant-stack.
pub fn shares_cow_bincode_identity(left: ParCursor<'_>, right: ParCursor<'_>) -> bool {
    let mut left = RhoPathCursor::new(left);
    let mut right = RhoPathCursor::new(right);

    loop {
        let equal = match (left.next(), right.next()) {
            (None, None) => return true,
            (
                Some(RhoPathEvent::EnterNode { program: left, .. }),
                Some(RhoPathEvent::EnterNode { program: right, .. }),
            ) => left.type_id == right.type_id,
            (Some(RhoPathEvent::LeaveNode), Some(RhoPathEvent::LeaveNode)) => true,
            (Some(RhoPathEvent::EnterField(left)), Some(RhoPathEvent::EnterField(right))) => {
                left.tag == right.tag
            }
            (Some(RhoPathEvent::LeaveField), Some(RhoPathEvent::LeaveField)) => true,
            (Some(RhoPathEvent::Scalar(left)), Some(RhoPathEvent::Scalar(right))) => left == right,
            (Some(RhoPathEvent::Absent), Some(RhoPathEvent::Absent)) => true,
            (Some(RhoPathEvent::EnterSequence(left)), Some(RhoPathEvent::EnterSequence(right))) => {
                left == right
            }
            (Some(RhoPathEvent::LeaveSequence), Some(RhoPathEvent::LeaveSequence)) => true,
            (Some(RhoPathEvent::EnterMap(left)), Some(RhoPathEvent::EnterMap(right))) => {
                left == right
            }
            (Some(RhoPathEvent::MapKey(left)), Some(RhoPathEvent::MapKey(right))) => left == right,
            (Some(RhoPathEvent::LeaveMap), Some(RhoPathEvent::LeaveMap)) => true,
            (Some(RhoPathEvent::EnterOneof(left)), Some(RhoPathEvent::EnterOneof(right))) => {
                left.protobuf_tag == right.protobuf_tag && left.serde_index == right.serde_index
            }
            (Some(RhoPathEvent::LeaveOneof), Some(RhoPathEvent::LeaveOneof)) => true,
            (Some(RhoPathEvent::ParBag(left)), Some(RhoPathEvent::ParBag(right))) => {
                left.shares_storage_with(right)
            }
            (Some(RhoPathEvent::PathMap(left)), Some(RhoPathEvent::PathMap(right))) => {
                left.shares_storage_with(right)
            }
            _ => false,
        };
        if !equal {
            return false;
        }
    }
}

/// Append one generated cursor event to an `RMP2` byte stream.
///
/// Pattern compilers use the same primitive as the exact encoder so literal
/// instruction runs cannot drift from datum encoding.
pub fn append_match_path_event(output: &mut Vec<u8>, event: RhoPathEvent<'_>) {
    match event {
        RhoPathEvent::EnterNode { program, .. } => {
            output.push(NODE_START);
            output.extend_from_slice(&program.type_id.to_be_bytes());
        }
        RhoPathEvent::LeaveNode => output.push(NODE_END),
        RhoPathEvent::EnterField(field) => {
            output.push(FIELD_START);
            output.extend_from_slice(&field.tag.to_be_bytes());
        }
        RhoPathEvent::LeaveField => output.push(FIELD_END),
        RhoPathEvent::Scalar(value) => append_scalar(output, value),
        RhoPathEvent::Absent => output.push(ABSENT),
        RhoPathEvent::EnterSequence(len) => {
            output.push(SEQUENCE_START);
            append_len(output, len);
        }
        RhoPathEvent::LeaveSequence => output.push(SEQUENCE_END),
        RhoPathEvent::EnterMap(len) => {
            output.push(MAP_START);
            append_len(output, len);
        }
        RhoPathEvent::MapKey(key) => {
            output.push(MAP_KEY);
            append_bytes(output, key.as_bytes());
        }
        RhoPathEvent::LeaveMap => output.push(MAP_END),
        RhoPathEvent::EnterOneof(oneof) => {
            output.push(ONEOF_START);
            output.extend_from_slice(&oneof.protobuf_tag.to_be_bytes());
        }
        RhoPathEvent::LeaveOneof => output.push(ONEOF_END),
        RhoPathEvent::ParBag(bag) => append_par_bag(output, bag),
        RhoPathEvent::PathMap(map) => append_pathmap(output, map),
    }
}

fn append_par_bag(output: &mut Vec<u8>, bag: &ParBag) {
    let summary = par_bag_match_summary(bag);
    append_par_bag_summary(output, summary);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParBagMatchSummary {
    distinct: u64,
    cardinality: u64,
    digest: [u8; 32],
}

impl ParBagMatchSummary {
    pub const fn new(distinct: u64, cardinality: u64, digest: [u8; 32]) -> Self {
        Self {
            distinct,
            cardinality,
            digest,
        }
    }

    pub const fn distinct(self) -> u64 { self.distinct }

    pub const fn cardinality(self) -> u64 { self.cardinality }

    pub const fn digest(self) -> [u8; 32] { self.digest }
}

pub(crate) const PAR_BAG_MATCH_TRANSCRIPT_DOMAIN: &[u8] = b"RhoMatchPath/ParBag/components/v2";

pub(crate) fn feed_par_bag_match_record(
    feed: &mut dyn FnMut(&[u8]),
    tag: u32,
    multiplicity: u64,
    protobuf: &[u8],
) {
    feed(&tag.to_be_bytes());
    feed(&multiplicity.to_be_bytes());
    let len =
        u64::try_from(protobuf.len()).expect("an admitted protobuf component length fits in u64");
    feed(&len.to_be_bytes());
    feed(protobuf);
}

pub(crate) fn par_bag_match_summary(bag: &ParBag) -> ParBagMatchSummary {
    par_bag_match_summary_with(bag, || {})
}

fn par_bag_match_summary_with(
    bag: &ParBag,
    on_digest_initialize: impl FnOnce(),
) -> ParBagMatchSummary {
    let digest = bag.memoized_rmp2_component_digest(|bag| {
        on_digest_initialize();
        compute_par_bag_match_digest(bag)
    });
    ParBagMatchSummary::new(
        u64::try_from(bag.distinct_len()).expect("an allocated Bag length fits in u64"),
        bag.cardinality(),
        digest,
    )
}

fn compute_par_bag_match_digest(bag: &ParBag) -> [u8; 32] {
    Blake2b256::hash_stream(|feed| {
        feed(PAR_BAG_MATCH_TRANSCRIPT_DOMAIN);
        bag.visit_canonical_key_parts(|tag, _score, protobuf, multiplicity| {
            feed_par_bag_match_record(feed, u32::from(tag.byte()), multiplicity.get(), protobuf);
        });
    })
    .try_into()
    .expect("BLAKE2b-256 always returns exactly 32 bytes")
}

/// Measurement-only uncached control for the RMP2 ParBag digest experiment.
///
/// This calls the exact transcript implementation used by the production
/// memoized route. It exists only in unit-test builds or when the explicit
/// experiment feature is enabled, preventing a second algorithm from drifting
/// while keeping the uncached route unavailable to ordinary production code.
#[cfg(any(test, feature = "rmp2-digest-experiment"))]
pub fn uncached_par_bag_match_summary_experiment(bag: &ParBag) -> ParBagMatchSummary {
    ParBagMatchSummary::new(
        u64::try_from(bag.distinct_len()).expect("an allocated Bag length fits in u64"),
        bag.cardinality(),
        compute_par_bag_match_digest(bag),
    )
}

/// Measurement-only access to the production memoized RMP2 summary route.
///
/// The benchmark target is an external crate, so it cannot call the ordinary
/// crate-private helper directly. This wrapper is absent unless the explicit
/// experiment feature is selected; default production builds therefore gain
/// neither an API surface nor a branch at the call site.
#[cfg(feature = "rmp2-digest-experiment")]
#[inline]
pub fn cached_par_bag_match_summary_experiment(bag: &ParBag) -> ParBagMatchSummary {
    par_bag_match_summary(bag)
}

/// Measurement-only cache-initialization witness for experiment preflights.
///
/// `initialized` is true exactly when this call executed the digest initializer
/// that visits every canonical ParBag endpoint. A false warm result is thus an
/// executable witness that the summary route did not reopen the transcript.
/// Timed samples use [`cached_par_bag_match_summary_experiment`] so this witness
/// adds no instrumentation to the measured treatment.
#[cfg(feature = "rmp2-digest-experiment")]
pub fn observed_par_bag_match_summary_experiment(bag: &ParBag) -> (ParBagMatchSummary, bool) {
    let mut initialized = false;
    let summary = par_bag_match_summary_with(bag, || initialized = true);
    (summary, initialized)
}

pub(crate) fn append_par_bag_summary(output: &mut Vec<u8>, summary: ParBagMatchSummary) {
    output.push(PAR_BAG);
    output.extend_from_slice(&summary.distinct.to_be_bytes());
    output.extend_from_slice(&summary.cardinality.to_be_bytes());
    output.extend_from_slice(&summary.digest);
}

fn append_scalar(output: &mut Vec<u8>, value: RhoScalarRef<'_>) {
    match value {
        RhoScalarRef::Bool(value) => {
            output.extend_from_slice(&[BOOL, u8::from(value)]);
        }
        RhoScalarRef::I32(value) => {
            output.push(I32);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoScalarRef::U32(value) => {
            output.push(U32);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoScalarRef::I64(value) => {
            output.push(I64);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoScalarRef::U64(value) => {
            output.push(U64);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoScalarRef::Bytes(value) => {
            output.push(BYTES);
            append_bytes(output, value);
        }
        RhoScalarRef::String(value) => {
            output.push(STRING);
            append_bytes(output, value.as_bytes());
        }
        RhoScalarRef::StringSequence(values) => {
            output.push(STRING_SEQUENCE);
            append_len(output, values.len());
            for value in values {
                append_bytes(output, value.as_bytes());
            }
        }
        RhoScalarRef::BytesSequence(values) => {
            output.push(BYTES_SEQUENCE);
            append_len(output, values.len());
            for value in values {
                append_bytes(output, value);
            }
        }
    }
}

fn append_pathmap(output: &mut Vec<u8>, map: &EPathMap) {
    output.push(PATHMAP);
    output.push(map.mode() as u8);
    append_len(output, map.len());
    output.extend_from_slice(&epathmap_match_digest(map));
}

/// Deterministic EPathMap key-topology digest for the derived match index.
///
/// Values in map mode are deliberately absent: the native EPathMap matcher is
/// the acceptance authority, so equal key topology may admit a conservative
/// false positive but can never accept one. Mode and cardinality are encoded
/// beside this digest by [`encode_match_path`].
pub fn epathmap_match_digest(map: &EPathMap) -> [u8; 32] {
    let digest = match map.representation() {
        EPathMapRepr::Empty => logical_pathmap_digest::<()>(None),
        EPathMapRepr::Set(entries) => logical_pathmap_digest(Some(entries)),
        EPathMapRepr::Map(entries) => logical_pathmap_digest(Some(entries)),
    };
    digest
        .try_into()
        .expect("BLAKE2b-256 always returns exactly 32 bytes")
}

pub(super) const PATHMAP_MATCH_DOMAIN: &[u8] = b"RhoMatchPath/EPathMap/key-topology/v1";
pub(super) const PATHMAP_MATCH_EDGE: u8 = 1;
pub(super) const PATHMAP_MATCH_LEAVE: u8 = 2;

pub(crate) fn logical_pathmap_digest<V>(entries: Option<&PathMap<V>>) -> Vec<u8>
where V: Clone + Send + Sync + Unpin {
    Blake2b256::hash_stream(|feed| {
        feed(PATHMAP_MATCH_DOMAIN);
        let Some(entries) = entries else {
            feed(&[0, PATHMAP_MATCH_LEAVE]);
            return;
        };
        let mut zipper = entries.read_zipper();
        feed(&[u8::from(zipper.get_val().is_some())]);
        let mut levels: Vec<ByteMaskIter> = vec![zipper.child_mask().iter()];
        while let Some(children) = levels.last_mut() {
            match children.next() {
                Some(byte) => {
                    feed(&[PATHMAP_MATCH_EDGE, byte]);
                    zipper.descend_to_byte(byte);
                    feed(&[u8::from(zipper.get_val().is_some())]);
                    levels.push(zipper.child_mask().iter());
                }
                None => {
                    feed(&[PATHMAP_MATCH_LEAVE]);
                    levels.pop();
                    if !levels.is_empty() {
                        assert!(zipper.ascend_byte());
                    }
                }
            }
        }
    })
}

fn append_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    append_len(output, bytes.len());
    output.extend_from_slice(bytes);
}

fn append_len(output: &mut Vec<u8>, len: usize) {
    let len = u64::try_from(len).expect("an allocated rhoapi collection length fits in u64");
    output.extend_from_slice(&len.to_be_bytes());
}

#[cfg(test)]
mod rmp2_digest_cache_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rust::par_bag::{CanonicalParBagKey, ParBagComponent, ParComponentTag};

    fn expr(value: i64) -> Expr {
        Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }
    }

    fn bag(values: &[i64]) -> ParBag {
        let mut bag = ParBag::new();
        for value in values {
            bag.insert(&expr(*value))
                .expect("test multiplicity fits u64");
        }
        bag
    }

    fn counted_summary(bag: &ParBag, initializations: &AtomicUsize) -> ParBagMatchSummary {
        par_bag_match_summary_with(bag, || {
            initializations.fetch_add(1, Ordering::SeqCst);
        })
    }

    fn assert_mutation_invalidates(name: &str, mut bag: ParBag, mutate: impl FnOnce(&mut ParBag)) {
        let initializations = AtomicUsize::new(0);
        let before = counted_summary(&bag, &initializations);
        assert_eq!(initializations.load(Ordering::SeqCst), 1, "{name}: prime");

        mutate(&mut bag);
        assert_eq!(
            initializations.load(Ordering::SeqCst),
            1,
            "{name}: mutation itself must not eagerly hash"
        );
        let after = counted_summary(&bag, &initializations);
        assert_eq!(
            initializations.load(Ordering::SeqCst),
            2,
            "{name}: first post-mutation access must traverse once"
        );
        assert_ne!(
            before.digest(),
            after.digest(),
            "{name}: fixture did not mutate"
        );
        assert_eq!(counted_summary(&bag, &initializations), after, "{name}");
        assert_eq!(
            initializations.load(Ordering::SeqCst),
            2,
            "{name}: second post-mutation access reopened the trie"
        );
    }

    #[test]
    fn first_and_second_summary_encoding_traverse_once() {
        let bag = bag(&[1, 1, 2]);
        let initializations = AtomicUsize::new(0);
        let first = counted_summary(&bag, &initializations);
        let second = counted_summary(&bag, &initializations);

        assert_eq!(first, second);
        assert_eq!(initializations.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn untouched_cow_clone_shares_the_one_digest_traversal() {
        let original = bag(&[1, 2]);
        let clone = original.clone();
        assert!(original.shares_storage_with(&clone));
        let initializations = AtomicUsize::new(0);

        let from_clone = counted_summary(&clone, &initializations);
        let from_original = counted_summary(&original, &initializations);
        assert_eq!(from_clone, from_original);
        assert_eq!(initializations.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn mutating_a_cow_clone_invalidates_only_the_detached_clone() {
        let original = bag(&[1, 2]);
        let initializations = AtomicUsize::new(0);
        let original_summary = counted_summary(&original, &initializations);
        let mut changed = original.clone();

        changed
            .insert(&expr(3))
            .expect("test multiplicity fits u64");
        assert!(!original.shares_storage_with(&changed));
        assert_eq!(
            counted_summary(&original, &initializations),
            original_summary
        );
        assert_eq!(initializations.load(Ordering::SeqCst), 1);

        let changed_summary = counted_summary(&changed, &initializations);
        assert_ne!(changed_summary, original_summary);
        assert_eq!(initializations.load(Ordering::SeqCst), 2);
        assert_eq!(
            counted_summary(&original, &initializations),
            original_summary
        );
        assert_eq!(counted_summary(&changed, &initializations), changed_summary);
        assert_eq!(initializations.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn every_component_mutation_route_invalidates_the_digest() {
        let key_one = CanonicalParBagKey::compile(&expr(1));
        let key_three = CanonicalParBagKey::compile(&expr(3));

        assert_mutation_invalidates("insert", bag(&[1, 2]), |bag| {
            bag.insert(&expr(3)).unwrap();
        });
        assert_mutation_invalidates("insert_many", bag(&[1, 2]), |bag| {
            bag.insert_many(&expr(1), Multiplicity::new(2).unwrap())
                .unwrap();
        });
        assert_mutation_invalidates("insert_key", bag(&[1, 2]), |bag| {
            bag.insert_key(&key_three).unwrap();
        });
        assert_mutation_invalidates("insert_key_many", bag(&[1, 2]), |bag| {
            bag.insert_key_many(&key_one, Multiplicity::new(2).unwrap())
                .unwrap();
        });
        assert_mutation_invalidates("insert_planned_key", bag(&[1, 2]), |bag| {
            bag.insert_planned_key(
                ParComponentTag::Expr,
                key_three.as_bytes(),
                Multiplicity::ONE,
                <Expr as ParBagComponent>::canonical_path_stable(&expr(3)),
            )
            .unwrap();
        });
        assert_mutation_invalidates("remove", bag(&[1, 2]), |bag| {
            assert!(bag.remove(&expr(1)).is_none());
        });
        assert_mutation_invalidates("remove_many", bag(&[1, 1, 2]), |bag| {
            bag.remove_many(&expr(1), Multiplicity::new(2).unwrap());
        });
        assert_mutation_invalidates("remove_key", bag(&[1, 2]), |bag| {
            bag.remove_key(&key_one);
        });
        assert_mutation_invalidates("remove_key_many", bag(&[1, 1, 2]), |bag| {
            bag.remove_key_many(&key_one, Multiplicity::new(2).unwrap());
        });
        assert_mutation_invalidates("try_remove_many", bag(&[1, 2]), |bag| {
            bag.try_remove_many(&expr(1), Multiplicity::ONE).unwrap();
        });
        assert_mutation_invalidates("try_remove_key_many", bag(&[1, 2]), |bag| {
            bag.try_remove_key_many(&key_one, Multiplicity::ONE)
                .unwrap();
        });
        assert_mutation_invalidates("remove_family", bag(&[1, 2]), |bag| {
            assert_eq!(bag.remove_family::<Expr>(), (2, 2));
        });
    }

    #[test]
    fn metadata_only_changes_and_component_noops_retain_the_digest() {
        let original = Par::from_parts(bag(&[1, 2]), Vec::new(), false);
        let initializations = AtomicUsize::new(0);
        let summary = counted_summary(original.components(), &initializations);

        let metadata = original
            .with_locally_free(vec![0x80])
            .with_connective_used(true);
        assert!(original
            .components()
            .shares_storage_with(metadata.components()));
        assert_eq!(
            counted_summary(metadata.components(), &initializations),
            summary
        );

        let mut absent_remove = original.components().clone();
        assert!(absent_remove.remove(&expr(99)).is_none());
        assert!(original.components().shares_storage_with(&absent_remove));
        assert_eq!(counted_summary(&absent_remove, &initializations), summary);

        assert_eq!(absent_remove.remove_family::<RhoSend>(), (0, 0));
        assert!(original.components().shares_storage_with(&absent_remove));
        assert_eq!(counted_summary(&absent_remove, &initializations), summary);
        assert_eq!(initializations.load(Ordering::SeqCst), 1);

        let mut saturated = ParBag::new();
        saturated
            .insert_many(&expr(7), Multiplicity::new(u64::MAX).unwrap())
            .unwrap();
        let saturated_summary = counted_summary(&saturated, &initializations);
        assert!(saturated.insert(&expr(7)).is_err());
        assert_eq!(
            counted_summary(&saturated, &initializations),
            saturated_summary
        );

        let mut underflow = bag(&[7]);
        let underflow_summary = counted_summary(&underflow, &initializations);
        assert!(underflow
            .try_remove_many(&expr(7), Multiplicity::new(2).unwrap())
            .is_err());
        assert_eq!(
            counted_summary(&underflow, &initializations),
            underflow_summary
        );
    }

    #[test]
    fn concurrent_first_access_initializes_one_shared_digest() {
        const THREADS: usize = 8;
        let bag = bag(&[1, 1, 2, 3]);
        let barrier = Arc::new(Barrier::new(THREADS));
        let initializations = Arc::new(AtomicUsize::new(0));
        let handles = (0..THREADS)
            .map(|_| {
                let bag = bag.clone();
                let barrier = Arc::clone(&barrier);
                let initializations = Arc::clone(&initializations);
                std::thread::spawn(move || {
                    barrier.wait();
                    counted_summary(&bag, &initializations)
                })
            })
            .collect::<Vec<_>>();

        let summaries = handles
            .into_iter()
            .map(|handle| handle.join().expect("digest worker did not panic"))
            .collect::<Vec<_>>();
        assert!(summaries.windows(2).all(|pair| pair[0] == pair[1]));
        assert_eq!(initializations.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cached_digest_and_token_are_byte_identical_to_a_contiguous_oracle() {
        let bag = bag(&[1, 1, 2, 3]);
        let mut transcript = PAR_BAG_MATCH_TRANSCRIPT_DOMAIN.to_vec();
        bag.visit_canonical_key_parts(|tag, _score, protobuf, multiplicity| {
            transcript.extend_from_slice(&u32::from(tag.byte()).to_be_bytes());
            transcript.extend_from_slice(&multiplicity.get().to_be_bytes());
            transcript.extend_from_slice(&(protobuf.len() as u64).to_be_bytes());
            transcript.extend_from_slice(protobuf);
        });
        let oracle_digest: [u8; 32] = Blake2b256::hash(transcript)
            .try_into()
            .expect("BLAKE2b-256 has a fixed width");

        let summary = par_bag_match_summary(&bag);
        assert_eq!(summary.digest(), oracle_digest);
        let mut cached_token = Vec::new();
        append_par_bag_summary(&mut cached_token, summary);
        let mut oracle_token = vec![PAR_BAG];
        oracle_token.extend_from_slice(&(bag.distinct_len() as u64).to_be_bytes());
        oracle_token.extend_from_slice(&bag.cardinality().to_be_bytes());
        oracle_token.extend_from_slice(&oracle_digest);
        assert_eq!(cached_token, oracle_token);
    }
}
