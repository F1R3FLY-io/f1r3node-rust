use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

use super::checkpoint_wal::SyntheticCheckpointSource;
use super::property_support::independent_inventory_digest;
use super::support::{TEST_SEMANTIC_PROFILE_ID, TracingStore, node as dag_node, nonzero};
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, GENERATION_CHECKPOINT_DOMAIN, GenerationCheckpointDescriptorV1,
    GenerationCheckpointDescriptorViewV1, GenerationCheckpointErrorV1,
    OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::closure::RetainedUnionClosureReportV1;
use crate::rspace::persistence::object::{CanonicalObjectStreamValidatorV1, DAG_NODE_DOMAIN};
use crate::rspace::persistence::retention::RETENTION_MANIFEST_DOMAIN;
use crate::rspace::persistence::retention::v2_format::{
    EMPTY_RETENTION_RADIX_NODE, EMPTY_RETENTION_RADIX_NODE_LEN, GENERATION_CHECKPOINT_V2_DOMAIN,
    RETENTION_ANCHOR_CANONICAL_LEN, RETENTION_ANCHOR_DOMAIN, RETENTION_KEY_LEN,
    RETENTION_RADIX_DOMAIN, RetentionAnchorV2, RetentionFormat as ProductionRetentionFormat,
    RetentionKeyV2, RetentionReader, empty_retention_radix_ref,
};
use crate::rspace::persistence::retention::v2_reader::{
    AppointedRetention, RetentionRadixErrorV2, RetentionRadixRefusalV2, admit_retention_history_v2,
};
use crate::rspace::persistence::wal::validate_object_for;
use crate::rspace::persistence::{
    AcceleratorArtifact, AcceleratorArtifactKind, AcceleratorManifestV1, CanonicalBlob,
    CanonicalBlobKind, CanonicalDagNode, CanonicalDagNodeView, CanonicalEncode,
    CanonicalFormatError, CanonicalObjectKind, CanonicalObjectView, DagChild, EndpointValue,
    LogPackFormatError, ObjectDigest, ObjectRefV1, RetainedStateV1, RetentionManifestV1,
    RetentionManifestView, SemanticRoots, SemanticStateManifestV1, SemanticStateManifestView,
    StateVersionV1,
};

pub(super) const KEY_WIDTH: usize = 48;
const MODEL_ANCHOR_DOMAIN: &[u8; 16] = b"RSpaceDAG.RetA\0\0";
const MODEL_RADIX_DOMAIN: &[u8; 16] = b"RSpaceDAG.RetR\0\0";
const MODEL_CHECKPOINT_V2_DOMAIN: &[u8; 16] = b"RSpaceDAG.Chkp2\0";
const EXISTING_DOMAINS: [&[u8; 16]; 15] = [
    b"RSpaceDAG.Node\0\0",
    b"RSpaceDAG.Blob\0\0",
    b"RSpace.Blob.CSrc",
    b"RSpaceDAG.SMan\0\0",
    b"RSpaceDAG.AMan\0\0",
    b"RSpaceDAG.Retn\0\0",
    b"RSpaceDAG.Chkpt\0",
    b"RSpaceInv.RefV1\0",
    b"RSpaceWAL.Prep\0\0",
    b"RSpaceWAL.Comm\0\0",
    b"RSpaceDAG.Mut\0\0\0",
    b"RSpaceCatalogV1\0",
    b"RSpaceIndex.V1\0\0",
    b"RSpacePack.V1\0\0\0",
    b"RSpace.Rust.V1\0\0",
];

pub(super) type History = BTreeMap<[u8; KEY_WIDTH], u64>;
pub(super) type ObjectSet = BTreeMap<ObjectDigest, Vec<u8>>;

pub(super) fn runner(cases: u32) -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
}

pub(super) fn reference(digest: [u8; 32], encoded_len: u64) -> ObjectRefV1 {
    ObjectRefV1::new(ObjectDigest::from_array(digest), encoded_len)
        .expect("model references cover an object envelope")
}

pub(super) fn state(generation: u64, digest: [u8; 32], encoded_len: u64) -> StateVersionV1 {
    StateVersionV1::new(generation, reference(digest, encoded_len))
}

pub(super) fn retained(state: StateVersionV1, claim: u64) -> RetainedStateV1 {
    RetainedStateV1::new(state, claim).expect("model claims cover the manifest length")
}

pub(super) fn reference_of(bytes: &[u8]) -> ObjectRefV1 {
    ObjectRefV1::new(ObjectDigest::of_canonical_bytes(bytes), bytes.len() as u64)
        .expect("model objects carry an envelope")
}

pub(super) fn encoded<T: CanonicalEncode>(value: &T) -> Vec<u8> {
    value.try_canonical_bytes().expect("model values encode")
}

fn envelope(domain: &[u8; 16], body: &[u8]) -> Vec<u8> {
    [&domain[..], &1_u64.to_le_bytes()[..], &(body.len() as u64).to_le_bytes()[..], body].concat()
}

fn with_domain(bytes: &[u8], domain: &[u8; 16]) -> Vec<u8> {
    let mut retagged = bytes.to_vec();
    retagged[..16].copy_from_slice(domain);
    retagged
}

fn reference_bytes(reference: ObjectRefV1) -> Vec<u8> {
    [reference.digest().as_bytes(), &reference.encoded_len().to_le_bytes()[..]].concat()
}

pub(super) fn model_key(state: StateVersionV1) -> [u8; KEY_WIDTH] {
    [
        &state.generation().to_be_bytes()[..],
        state.manifest().digest().as_bytes(),
        &state.manifest().encoded_len().to_be_bytes()[..],
    ]
    .concat()
    .try_into()
    .expect("a retention key is 48 bytes")
}

pub(super) fn model_decode_key(key: &[u8]) -> Option<StateVersionV1> {
    let (generation, rest) = key.split_first_chunk::<8>()?;
    let (digest, rest) = rest.split_first_chunk::<32>()?;
    let encoded_len: &[u8; 8] = rest.try_into().ok()?;
    let manifest =
        ObjectRefV1::new(ObjectDigest::from_array(*digest), u64::from_be_bytes(*encoded_len))
            .ok()?;
    Some(StateVersionV1::new(u64::from_be_bytes(*generation), manifest))
}

fn little_endian_key(state: StateVersionV1) -> Vec<u8> {
    [
        &state.generation().to_le_bytes()[..],
        state.manifest().digest().as_bytes(),
        &state.manifest().encoded_len().to_le_bytes()[..],
    ]
    .concat()
}

pub(super) fn model_anchor_bytes(
    current: RetainedStateV1,
    count: u64,
    root: ObjectRefV1,
) -> Vec<u8> {
    let body = [
        &current.state().generation().to_le_bytes()[..],
        &reference_bytes(current.state().manifest()),
        &current.canonical_bytes().to_le_bytes()[..],
        &count.to_le_bytes()[..],
        &reference_bytes(root),
    ]
    .concat();
    envelope(MODEL_ANCHOR_DOMAIN, &body)
}

pub(super) fn radix_node(prefix: &[u8], endpoint: EndpointValue, children: &[DagChild]) -> Vec<u8> {
    let node = CanonicalDagNode::new(prefix, endpoint, children.to_vec())
        .expect("model radix shapes are canonical DAG shapes");
    with_domain(&encoded(&node), MODEL_RADIX_DOMAIN)
}

pub(super) fn insert_object(objects: &mut ObjectSet, bytes: Vec<u8>) -> ObjectRefV1 {
    let reference = reference_of(&bytes);
    objects.entry(reference.digest()).or_insert(bytes);
    reference
}

fn golden(bytes: &[u8], length: usize, digest: &str) {
    assert_eq!(bytes.len(), length);
    assert_eq!(hex::encode(ObjectDigest::of_canonical_bytes(bytes).as_bytes()), digest);
}

#[derive(Clone, Debug)]
pub(super) struct RadixImage {
    pub(super) root: ObjectRefV1,
    pub(super) objects: ObjectSet,
}

struct PlannedNode {
    start: usize,
    end: usize,
    depth: usize,
    prefix_end: usize,
    children: Vec<(u8, usize)>,
}

pub(super) fn build_radix(history: &History) -> RadixImage {
    let entries = history
        .iter()
        .map(|(key, claim)| (*key, *claim))
        .collect::<Vec<_>>();
    let mut objects = ObjectSet::new();
    if entries.is_empty() {
        let root = insert_object(&mut objects, radix_node(&[], EndpointValue::Absent, &[]));
        return RadixImage { root, objects };
    }
    let mut planned = Vec::with_capacity(2 * entries.len());
    planned.push(PlannedNode {
        start: 0,
        end: entries.len(),
        depth: 0,
        prefix_end: 0,
        children: Vec::new(),
    });
    let mut pending = vec![0_usize];
    while let Some(index) = pending.pop() {
        let (start, end, depth) = (planned[index].start, planned[index].end, planned[index].depth);
        if end - start == 1 {
            planned[index].prefix_end = KEY_WIDTH;
            continue;
        }
        let shared = entries[start].0[depth..]
            .iter()
            .zip(&entries[end - 1].0[depth..])
            .take_while(|(left, right)| left == right)
            .count();
        let prefix_end = depth + shared;
        planned[index].prefix_end = prefix_end;
        let mut group_start = start;
        while group_start < end {
            let edge = entries[group_start].0[prefix_end];
            let group_end = group_start +
                entries[group_start..end]
                    .iter()
                    .take_while(|(key, _)| key[prefix_end] == edge)
                    .count();
            let child = planned.len();
            planned.push(PlannedNode {
                start: group_start,
                end: group_end,
                depth: prefix_end + 1,
                prefix_end: 0,
                children: Vec::new(),
            });
            planned[index].children.push((edge, child));
            pending.push(child);
            group_start = group_end;
        }
    }
    let mut references = vec![None; planned.len()];
    for index in (0..planned.len()).rev() {
        let node = &planned[index];
        let (key, claim) = entries[node.start];
        let prefix = &key[node.depth..node.prefix_end];
        let bytes = if node.children.is_empty() {
            radix_node(prefix, EndpointValue::Multiplicity(nonzero(claim)), &[])
        } else {
            let children = node
                .children
                .iter()
                .map(|&(edge, child)| DagChild {
                    edge,
                    target: references[child].expect("children are encoded before parents"),
                })
                .collect::<Vec<_>>();
            radix_node(prefix, EndpointValue::Absent, &children)
        };
        references[index] = Some(insert_object(&mut objects, bytes));
    }
    RadixImage {
        root: references[0].expect("the root is encoded"),
        objects,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RadixRefusal {
    MissingObject,
    ReferenceMismatch,
    NotRadixDomain,
    Body(CanonicalFormatError),
    EndpointKind(&'static str),
    EndpointDepth { remaining: usize, prefix: usize },
    EmptyBelowRoot,
    LeafKey,
    LeafClaim(CanonicalFormatError),
    CountMismatch { declared: u64, unfolded: u64 },
    CountOverflow,
}

#[derive(Clone, Debug)]
pub(super) struct RadixNodeImage {
    pub(super) prefix: Vec<u8>,
    pub(super) endpoint: EndpointValue,
    pub(super) children: Vec<DagChild>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct UnfoldedLeaf {
    pub(super) key: [u8; KEY_WIDTH],
    pub(super) claim: u64,
    pub(super) object: ObjectDigest,
}

pub(super) fn load_radix_node(
    objects: &ObjectSet,
    reference: ObjectRefV1,
) -> Result<RadixNodeImage, RadixRefusal> {
    let Some(bytes) = objects.get(&reference.digest()) else {
        return Err(RadixRefusal::MissingObject);
    };
    if bytes.len() as u64 != reference.encoded_len() ||
        ObjectDigest::of_canonical_bytes(bytes) != reference.digest()
    {
        return Err(RadixRefusal::ReferenceMismatch);
    }
    match bytes.get(..16) {
        Some(domain) if domain == MODEL_RADIX_DOMAIN => {}
        _ => return Err(RadixRefusal::NotRadixDomain),
    }
    let body = with_domain(bytes, DAG_NODE_DOMAIN);
    let view = CanonicalDagNodeView::parse(&body).map_err(RadixRefusal::Body)?;
    Ok(RadixNodeImage {
        prefix: view.prefix().to_vec(),
        endpoint: view.endpoint(),
        children: view.children().iter().collect(),
    })
}

pub(super) fn unfold_radix(
    objects: &ObjectSet,
    root: ObjectRefV1,
    declared: u64,
) -> Result<Vec<UnfoldedLeaf>, RadixRefusal> {
    let mut leaves = Vec::new();
    let mut pending = vec![(root, Vec::with_capacity(KEY_WIDTH))];
    while let Some((reference, path)) = pending.pop() {
        let node = load_radix_node(objects, reference)?;
        let remaining = KEY_WIDTH - path.len();
        let prefix = node.prefix.len();
        match (node.endpoint, node.children.is_empty(), path.is_empty()) {
            (EndpointValue::Absent, true, true) => {
                return match declared {
                    0 => Ok(leaves),
                    _ => Err(RadixRefusal::CountMismatch {
                        declared,
                        unfolded: 0,
                    }),
                };
            }
            (EndpointValue::Absent, true, false) => return Err(RadixRefusal::EmptyBelowRoot),
            (EndpointValue::Multiplicity(claim), true, _) if prefix == remaining => {
                let mut key = [0_u8; KEY_WIDTH];
                key[..path.len()].copy_from_slice(&path);
                key[path.len()..].copy_from_slice(&node.prefix);
                let Some(state) = model_decode_key(&key) else {
                    return Err(RadixRefusal::LeafKey);
                };
                RetainedStateV1::new(state, claim.get()).map_err(RadixRefusal::LeafClaim)?;
                leaves.push(UnfoldedLeaf {
                    key,
                    claim: claim.get(),
                    object: reference.digest(),
                });
            }
            (EndpointValue::Absent, false, _) if prefix < remaining => {
                let mut below = path;
                below.extend_from_slice(&node.prefix);
                for child in node.children.iter().rev() {
                    let mut child_path = Vec::with_capacity(KEY_WIDTH);
                    child_path.extend_from_slice(&below);
                    child_path.push(child.edge);
                    pending.push((child.target, child_path));
                }
            }
            (EndpointValue::Absent | EndpointValue::Multiplicity(_), _, _) => {
                return Err(RadixRefusal::EndpointDepth { remaining, prefix });
            }
            (endpoint, _, _) => return Err(RadixRefusal::EndpointKind(endpoint.name())),
        }
    }
    let unfolded = leaves.len() as u64;
    if unfolded != declared {
        return Err(RadixRefusal::CountMismatch { declared, unfolded });
    }
    Ok(leaves)
}

pub(super) fn memoized_count(objects: &ObjectSet, root: ObjectRefV1) -> Result<u64, RadixRefusal> {
    let mut memo = BTreeMap::<(ObjectDigest, usize), u64>::new();
    let mut pending = vec![(root, KEY_WIDTH, false)];
    while let Some((reference, remaining, expanded)) = pending.pop() {
        let memo_key = (reference.digest(), remaining);
        if memo.contains_key(&memo_key) {
            continue;
        }
        let node = load_radix_node(objects, reference)?;
        let prefix = node.prefix.len();
        match (node.endpoint, node.children.is_empty(), expanded) {
            (EndpointValue::Absent, true, _) => {
                memo.insert(memo_key, 0);
            }
            (EndpointValue::Multiplicity(_), true, _) if prefix == remaining => {
                memo.insert(memo_key, 1);
            }
            (EndpointValue::Absent, false, false) if prefix < remaining => {
                pending.push((reference, remaining, true));
                for child in &node.children {
                    pending.push((child.target, remaining - prefix - 1, false));
                }
            }
            (EndpointValue::Absent, false, true) => {
                let below = remaining - prefix - 1;
                let mut total = 0_u64;
                for child in &node.children {
                    let count = memo
                        .get(&(child.target.digest(), below))
                        .copied()
                        .expect("children are counted before their parent");
                    total = total
                        .checked_add(count)
                        .ok_or(RadixRefusal::CountOverflow)?;
                }
                memo.insert(memo_key, total);
            }
            _ => return Err(RadixRefusal::EndpointDepth { remaining, prefix }),
        }
    }
    Ok(memo
        .get(&(root.digest(), KEY_WIDTH))
        .copied()
        .expect("the root is counted"))
}

pub(super) fn metadata_closure(
    anchor: RetentionAnchorV2,
    image: &RadixImage,
) -> Result<BTreeMap<ObjectDigest, ObjectRefV1>, RadixRefusal> {
    let mut members = BTreeMap::new();
    members.insert(anchor.object_ref().digest(), anchor.object_ref());
    let mut pending = vec![anchor.historical_root()];
    while let Some(reference) = pending.pop() {
        if members.insert(reference.digest(), reference).is_some() {
            continue;
        }
        let node = load_radix_node(&image.objects, reference)?;
        pending.extend(node.children.iter().map(|child| child.target));
    }
    Ok(members)
}

fn generation_strategy() -> impl Strategy<Value = u64> {
    prop_oneof![
        Just(0_u64),
        Just(1_u64),
        Just(255_u64),
        Just(256_u64),
        Just(u64::MAX),
        any::<u64>()
    ]
}

fn digest_strategy() -> impl Strategy<Value = [u8; 32]> {
    prop_oneof![Just([0_u8; 32]), Just([0xff_u8; 32]), any::<[u8; 32]>()]
}

fn length_strategy() -> impl Strategy<Value = u64> {
    prop_oneof![Just(32_u64), Just(33_u64), Just(50_u64), Just(u64::MAX), 32_u64..]
}

fn state_strategy() -> impl Strategy<Value = StateVersionV1> {
    (generation_strategy(), digest_strategy(), length_strategy())
        .prop_map(|(generation, digest, encoded_len)| state(generation, digest, encoded_len))
}

pub(super) fn clustered_state() -> impl Strategy<Value = StateVersionV1> {
    (
        prop_oneof![0_u64..4, Just(255_u64), Just(256_u64), Just(u64::MAX)],
        0_u8..2,
        0_u8..3,
        prop_oneof![Just(32_u64), Just(50_u64), Just(51_u64)],
    )
        .prop_map(|(generation, high, low, encoded_len)| {
            let mut digest = [high; 32];
            digest[31] = low;
            state(generation, digest, encoded_len)
        })
}

pub(super) fn history_strategy(max_entries: usize) -> impl Strategy<Value = History> {
    prop::collection::vec((clustered_state(), 0_u64..3), 0..=max_entries).prop_map(|entries| {
        entries
            .into_iter()
            .map(|(state, extra)| (model_key(state), state.manifest().encoded_len() + extra))
            .collect()
    })
}

fn retained_union() -> impl Strategy<Value = Vec<ObjectRefV1>> {
    prop::collection::btree_map(any::<[u8; 32]>(), 32_u64..4096, 1..12).prop_map(|entries| {
        entries
            .into_iter()
            .map(|(digest, encoded_len)| reference(digest, encoded_len))
            .collect()
    })
}

fn product_history() -> impl Strategy<Value = (History, usize, usize)> {
    (
        prop::collection::btree_set(0_u64..8, 2..=4),
        prop::collection::btree_set(any::<u8>(), 1..=4),
        any::<u64>(),
    )
        .prop_map(|(generations, digests, base)| {
            let base = base & !0xff;
            let history = generations
                .iter()
                .flat_map(|generation| {
                    digests.iter().map(move |digest| {
                        (
                            model_key(state(base + generation, [*digest; 32], 50)),
                            50 + u64::from(*digest),
                        )
                    })
                })
                .collect::<History>();
            (history, generations.len(), digests.len())
        })
}

fn history_anchor(history: &History, image: &RadixImage) -> RetentionAnchorV2 {
    RetentionAnchorV2::new(
        retained(state(u64::MAX, [0xcc; 32], 64), 4096),
        history.len() as u64,
        image.root,
    )
    .expect("the model history root matches its count")
}

pub(super) fn assert_v1_readers_refuse(bytes: &[u8]) {
    assert!(matches!(
        CanonicalObjectKind::from_domain_prefix(bytes),
        Err(CanonicalFormatError::UnknownObjectDomain)
    ));
    assert!(matches!(
        CanonicalObjectView::parse(bytes),
        Err(CanonicalFormatError::UnknownObjectDomain)
    ));
    let mut validator = CanonicalObjectStreamValidatorV1::new(bytes.len());
    assert!(matches!(validator.feed(bytes), Err(CanonicalFormatError::UnknownObjectDomain)));
    assert_eq!(
        RetentionManifestView::parse(bytes).map(|_| ()),
        Err(CanonicalFormatError::InvalidDomain {
            expected: RETENTION_MANIFEST_DOMAIN
        })
    );
    assert_eq!(
        CanonicalDagNodeView::parse(bytes).map(|_| ()),
        Err(CanonicalFormatError::InvalidDomain {
            expected: DAG_NODE_DOMAIN
        })
    );
}

fn semantic_manifest(seed: u8) -> (SemanticStateManifestV1, Vec<u8>) {
    let roots = SemanticRoots {
        data_bag: reference([seed; 32], 50),
        continuation_map: reference([seed ^ 0x40; 32], 51),
        join_bag: reference([seed ^ 0x80; 32], 52),
    };
    let manifest = SemanticStateManifestV1::new(1, TEST_SEMANTIC_PROFILE_ID, roots, None)
        .expect("schema-one manifests are valid");
    let bytes = encoded(&manifest);
    (manifest, bytes)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetentionFormat {
    V1,
    V2,
}

fn session_binding(
    state: StateVersionV1,
    format: RetentionFormat,
    retention: ObjectRefV1,
) -> [u8; 32] {
    let (retention_domain, descriptor_domain) = match format {
        RetentionFormat::V1 => (RETENTION_MANIFEST_DOMAIN, GENERATION_CHECKPOINT_DOMAIN),
        RetentionFormat::V2 => (MODEL_ANCHOR_DOMAIN, MODEL_CHECKPOINT_V2_DOMAIN),
    };
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(state.generation().to_le_bytes());
    hasher.update(reference_bytes(state.manifest()));
    hasher.update(reference_bytes(retention));
    hasher.update(retention_domain);
    hasher.update(descriptor_domain);
    let mut binding = [0_u8; 32];
    binding.copy_from_slice(&hasher.finalize());
    binding
}

fn state_version_only_binding(
    state: StateVersionV1,
    _format: RetentionFormat,
    _retention: ObjectRefV1,
) -> StateVersionV1 {
    state
}

#[test]
fn retention_keys_are_exact_48_byte_bijections() {
    for width in (0..=KEY_WIDTH + 1).filter(|width| *width != KEY_WIDTH) {
        assert_eq!(
            RetentionKeyV2::parse(&vec![0x33; width]),
            Err(CanonicalFormatError::LengthOutOfRange {
                field: "retention key",
                value: width as u64,
            })
        );
    }
    let mut short = model_key(state(4, [0x10; 32], 32));
    short[KEY_WIDTH - 1] = 31;
    assert_eq!(RetentionKeyV2::parse(&short), Err(CanonicalFormatError::ObjectLengthTooSmall(31)));

    runner(64)
        .run(
            &(state_strategy(), prop::array::uniform::<_, KEY_WIDTH>(any::<u8>())),
            |(state, raw)| {
                let key = RetentionKeyV2::new(state);
                prop_assert_eq!(key.as_bytes(), &model_key(state));
                prop_assert_eq!(RetentionKeyV2::parse(key.as_bytes()), Ok(key));
                prop_assert_eq!(key.state(), state);
                prop_assert_eq!(model_decode_key(key.as_bytes()), Some(state));
                match (RetentionKeyV2::parse(&raw), model_decode_key(&raw)) {
                    (Ok(parsed), Some(decoded)) => {
                        prop_assert_eq!(parsed.as_bytes(), &raw);
                        prop_assert_eq!(parsed.state(), decoded);
                        prop_assert_eq!(RetentionKeyV2::new(decoded), parsed);
                    }
                    (Err(CanonicalFormatError::ObjectLengthTooSmall(length)), None) => {
                        prop_assert!(length < 32);
                    }
                    (parsed, decoded) => {
                        return Err(TestCaseError::fail(format!(
                            "production {parsed:?} disagrees with the model {decoded:?}"
                        )));
                    }
                }
                Ok(())
            },
        )
        .expect("retention keys are exact 48-byte bijections");
}

#[test]
fn retention_key_bytes_sort_like_v1_identities_but_little_endian_keys_do_not() {
    let reference = [0x5a; 32];
    let (one, two_fifty_six) = (state(1, reference, 50), state(256, reference, 50));
    assert!(RetentionKeyV2::new(one) < RetentionKeyV2::new(two_fifty_six));
    assert!(little_endian_key(two_fifty_six) < little_endian_key(one));

    let current = retained(state(u64::MAX, [0xcc; 32], 32), 32);
    runner(256)
        .run(
            &(state_strategy(), state_strategy(), prop::collection::vec(clustered_state(), 0..12)),
            |(left, right, states)| {
                let identity_order = (left.generation(), left.manifest())
                    .cmp(&(right.generation(), right.manifest()));
                prop_assert_eq!(
                    RetentionKeyV2::new(left).cmp(&RetentionKeyV2::new(right)),
                    identity_order
                );
                prop_assert_eq!(
                    RetentionKeyV2::new(left)
                        .as_bytes()
                        .cmp(RetentionKeyV2::new(right).as_bytes()),
                    identity_order
                );
                prop_assert_eq!(model_key(left).cmp(&model_key(right)), identity_order);

                let by_key = states
                    .iter()
                    .map(|state| (model_key(*state), *state))
                    .collect::<BTreeMap<_, _>>();
                let historical = by_key
                    .values()
                    .map(|state| retained(*state, state.manifest().encoded_len()))
                    .collect::<Vec<_>>();
                prop_assert!(RetentionManifestV1::new(current, historical.clone()).is_ok());
                if historical.len() >= 2 {
                    let mut reversed = historical;
                    reversed.reverse();
                    prop_assert_eq!(
                        RetentionManifestV1::new(current, reversed).err(),
                        Some(CanonicalFormatError::RetainedStatesNotStrict)
                    );
                }
                Ok(())
            },
        )
        .expect("retention key order is V1 identity order");
}

#[test]
fn generation_and_reference_aliases_keep_distinct_keys() {
    let base = state(5, [0x21; 32], 50);
    let aliases = [state(6, [0x21; 32], 50), state(5, [0x22; 32], 50), state(5, [0x21; 32], 51)];
    let generation_only = |state: StateVersionV1| state.generation().to_be_bytes().to_vec();
    let reference_only = |state: StateVersionV1| model_key(state)[8..].to_vec();
    let length_free = |state: StateVersionV1| model_key(state)[..40].to_vec();
    let current = retained(state(9, [0xcc; 32], 50), 50);
    for (alias, collapses) in aliases.into_iter().zip([
        reference_only(base) == reference_only(aliases[0]),
        generation_only(base) == generation_only(aliases[1]),
        length_free(base) == length_free(aliases[2]),
    ]) {
        assert!(collapses, "the lossy control keeps {alias:?} distinct from {base:?}");
        assert_ne!(RetentionKeyV2::new(alias), RetentionKeyV2::new(base));
        let mut pair = [retained(base, 51), retained(alias, 51)];
        pair.sort_unstable_by_key(|entry| model_key(entry.state()));
        assert!(RetentionManifestV1::new(current, pair.to_vec()).is_ok());
    }

    runner(256)
        .run(
            &(state_strategy(), generation_strategy(), digest_strategy(), length_strategy()),
            |(base, generation, digest, encoded_len)| {
                let base_digest = *base.manifest().digest().as_array();
                let base_len = base.manifest().encoded_len();
                for alias in [
                    state(generation, base_digest, base_len),
                    state(base.generation(), digest, base_len),
                    state(base.generation(), base_digest, encoded_len),
                ] {
                    prop_assert_eq!(
                        alias == base,
                        RetentionKeyV2::new(alias) == RetentionKeyV2::new(base)
                    );
                    prop_assert_eq!(alias == base, model_key(alias) == model_key(base));
                }
                Ok(())
            },
        )
        .expect("aliases keep distinct keys");
}

#[test]
fn retention_v2_encodings_are_frozen() {
    assert_eq!(RETENTION_ANCHOR_DOMAIN, MODEL_ANCHOR_DOMAIN);
    assert_eq!(RETENTION_RADIX_DOMAIN, MODEL_RADIX_DOMAIN);
    assert_eq!(GENERATION_CHECKPOINT_V2_DOMAIN, MODEL_CHECKPOINT_V2_DOMAIN);
    assert_eq!(RETENTION_KEY_LEN, KEY_WIDTH);
    assert_eq!(RETENTION_ANCHOR_CANONICAL_LEN, 136);
    assert_eq!(EMPTY_RETENTION_RADIX_NODE_LEN, 50);

    let empty_radix = radix_node(&[], EndpointValue::Absent, &[]);
    let empty_semantic = encoded(&CanonicalDagNode::empty());
    assert_eq!(empty_radix, EMPTY_RETENTION_RADIX_NODE.to_vec());
    assert_eq!(empty_radix, envelope(MODEL_RADIX_DOMAIN, &[0_u8; 18]));
    assert_eq!(with_domain(&empty_semantic, MODEL_RADIX_DOMAIN), empty_radix);
    assert_eq!(empty_retention_radix_ref(), reference_of(&empty_radix));
    golden(&empty_radix, 50, "50e61ba04a451666697cf575964a1c6ada6a5d6a3a43f321c31f898d79f5cda8");
    golden(&empty_semantic, 50, "21dd2a72f6a9d98af1a2fd6cf2a871f2076bec013ce1cd6c7cca6d46ff4c0dc2");

    let empty_current = retained(state(1, [0xaa; 32], 50), 50);
    let empty_history = RetentionAnchorV2::new(empty_current, 0, empty_retention_radix_ref())
        .expect("an empty history appoints the empty radix node");
    let empty_history_bytes = encoded(&empty_history);
    assert_eq!(
        empty_history_bytes,
        model_anchor_bytes(empty_current, 0, reference_of(&empty_radix))
    );
    golden(
        &empty_history_bytes,
        136,
        "aafa9f0acc53e37a3a686374191493dbaae8041691ed96f1e8af8b3e6730511a",
    );
    assert_eq!(RetentionAnchorV2::parse(&empty_history_bytes), Ok(empty_history));
    assert_eq!(empty_history.object_ref(), reference_of(&empty_history_bytes));

    let leaf_state = state(1, [0xbb; 32], 50);
    let leaf = build_radix(&History::from([(model_key(leaf_state), 50)]));
    assert_eq!(leaf.objects.len(), 1);
    let leaf_bytes = &leaf.objects[&leaf.root.digest()];
    assert_eq!(
        leaf_bytes,
        &radix_node(&model_key(leaf_state), EndpointValue::Multiplicity(nonzero(50)), &[])
    );
    golden(leaf_bytes, 106, "ed69c9c67bfd19d28914472c29d1082d30017a86e4b31186db5d2e292d3abfd2");

    let one_current = retained(state(2, [0xaa; 32], 50), 50);
    let one_entry = RetentionAnchorV2::new(one_current, 1, leaf.root)
        .expect("a one-entry history appoints its leaf");
    let one_entry_bytes = encoded(&one_entry);
    assert_eq!(one_entry_bytes, model_anchor_bytes(one_current, 1, leaf.root));
    golden(
        &one_entry_bytes,
        136,
        "c618aca6611095642bc65d31754660b79ecbaf6da0d5c30f9134b18263770571",
    );
    let v1_bytes = encoded(
        &RetentionManifestV1::new(one_current, Vec::new()).expect("a V1 manifest is valid"),
    );
    assert_eq!(one_entry_bytes[32..88], v1_bytes[32..88]);
}

#[test]
fn radix_nodes_reuse_the_dag_body_under_the_metadata_domain() {
    let (left, right) = (state(1, [0x11; 32], 50), state(2, [0x11; 32], 50));
    let history = History::from([(model_key(left), 50), (model_key(right), 60)]);
    let image = build_radix(&history);
    let leaves = unfold_radix(&image.objects, image.root, 2).expect("the history unfolds");
    assert_eq!(
        leaves
            .iter()
            .map(|leaf| (leaf.key, leaf.claim))
            .collect::<History>(),
        history
    );
    let root = load_radix_node(&image.objects, image.root).expect("the root is a radix node");
    assert_eq!(root.prefix, vec![0; 7]);
    assert_eq!(root.endpoint, EndpointValue::Absent);
    assert_eq!(
        root.children
            .iter()
            .map(|child| child.edge)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );

    let suffix = model_key(left)[8..].to_vec();
    let sibling = root.children[1];
    let refuse = |replacement: Vec<u8>| {
        let mut objects = image.objects.clone();
        let target = insert_object(&mut objects, replacement);
        let replaced = radix_node(&root.prefix, EndpointValue::Absent, &[
            DagChild { edge: 1, target },
            sibling,
        ]);
        let replaced = insert_object(&mut objects, replaced);
        unfold_radix(&objects, replaced, 2).expect_err("the malformed history is refused")
    };

    for endpoint in [
        EndpointValue::ValueFree,
        EndpointValue::Unit,
        EndpointValue::Content(reference([0x22; 32], 64)),
    ] {
        assert_eq!(
            refuse(radix_node(&suffix, endpoint, &[])),
            RadixRefusal::EndpointKind(endpoint.name())
        );
    }
    assert_eq!(
        refuse(radix_node(&suffix[1..], EndpointValue::Multiplicity(nonzero(50)), &[])),
        RadixRefusal::EndpointDepth {
            remaining: 40,
            prefix: 39
        }
    );
    assert_eq!(
        refuse(radix_node(&suffix[..10], EndpointValue::Multiplicity(nonzero(50)), &[
            DagChild {
                edge: 0,
                target: sibling.target
            },
            DagChild {
                edge: 1,
                target: sibling.target
            },
        ],)),
        RadixRefusal::EndpointDepth {
            remaining: 40,
            prefix: 10
        }
    );
    let mut objects = image.objects.clone();
    let full_width = insert_object(
        &mut objects,
        radix_node(&[0; KEY_WIDTH], EndpointValue::Absent, &root.children),
    );
    assert_eq!(
        unfold_radix(&objects, full_width, 2),
        Err(RadixRefusal::EndpointDepth {
            remaining: KEY_WIDTH,
            prefix: KEY_WIDTH
        })
    );

    assert_eq!(
        refuse(with_domain(
            &radix_node(&suffix, EndpointValue::Multiplicity(nonzero(50)), &[]),
            DAG_NODE_DOMAIN
        )),
        RadixRefusal::NotRadixDomain
    );
    let current = retained(state(9, [0xcc; 32], 50), 50);
    let mut objects = image.objects.clone();
    let anchor = insert_object(&mut objects, model_anchor_bytes(current, 2, image.root));
    assert_eq!(unfold_radix(&objects, anchor, 2), Err(RadixRefusal::NotRadixDomain));

    assert_eq!(refuse(EMPTY_RETENTION_RADIX_NODE.to_vec()), RadixRefusal::EmptyBelowRoot);
    let mismatch = |count| Err(CanonicalFormatError::RetentionHistoryRootMismatch { count });
    assert_eq!(RetentionAnchorV2::new(current, 0, image.root), mismatch(0));
    assert_eq!(RetentionAnchorV2::new(current, 2, empty_retention_radix_ref()), mismatch(2));
    assert_eq!(RetentionAnchorV2::parse(&model_anchor_bytes(current, 0, image.root)), mismatch(0));
    assert_eq!(
        RetentionAnchorV2::parse(&model_anchor_bytes(current, 1, empty_retention_radix_ref())),
        mismatch(1)
    );
    assert_eq!(
        RetentionAnchorV2::parse(&model_anchor_bytes(current, 2, image.root)),
        RetentionAnchorV2::new(current, 2, image.root)
    );

    assert_eq!(
        refuse(radix_node(&suffix, EndpointValue::Multiplicity(nonzero(49)), &[])),
        RadixRefusal::LeafClaim(CanonicalFormatError::RetentionStateBytes {
            generation: 1,
            minimum: 50,
            actual: 49,
        })
    );
    let mut short_suffix = suffix.clone();
    short_suffix[39] = 31;
    assert_eq!(
        refuse(radix_node(&short_suffix, EndpointValue::Multiplicity(nonzero(50)), &[])),
        RadixRefusal::LeafKey
    );

    assert_eq!(
        unfold_radix(&image.objects, image.root, 3),
        Err(RadixRefusal::CountMismatch {
            declared: 3,
            unfolded: 2
        })
    );
    assert_eq!(
        unfold_radix(&image.objects, image.root, 0),
        Err(RadixRefusal::CountMismatch {
            declared: 0,
            unfolded: 2
        })
    );

    let child = reference_bytes(sibling.target);
    let unary = envelope(
        MODEL_RADIX_DOMAIN,
        &[&0_u64.to_le_bytes()[..], &[0_u8, 1][..], &1_u64.to_le_bytes()[..], &[1_u8][..], &child]
            .concat(),
    );
    assert_eq!(refuse(unary), RadixRefusal::Body(CanonicalFormatError::CompressibleUnaryNode));
    let unordered = envelope(
        MODEL_RADIX_DOMAIN,
        &[
            &0_u64.to_le_bytes()[..],
            &[0_u8, 1][..],
            &2_u64.to_le_bytes()[..],
            &[2_u8][..],
            &child,
            &[1_u8][..],
            &child,
        ]
        .concat(),
    );
    assert_eq!(
        refuse(unordered),
        RadixRefusal::Body(CanonicalFormatError::ChildEdgesNotStrict {
            previous: 2,
            current: 1
        })
    );

    let mut missing = image.objects.clone();
    missing.remove(&sibling.target.digest());
    assert_eq!(unfold_radix(&missing, image.root, 2), Err(RadixRefusal::MissingObject));
    let substituted = ObjectRefV1::new(image.root.digest(), image.root.encoded_len() + 1)
        .expect("the substituted length stays above the envelope");
    assert_eq!(unfold_radix(&image.objects, substituted, 2), Err(RadixRefusal::ReferenceMismatch));

    runner(256)
        .run(&history_strategy(24), |history| {
            let image = build_radix(&history);
            let leaves = unfold_radix(&image.objects, image.root, history.len() as u64)
                .map_err(|refusal| TestCaseError::fail(format!("{refusal:?}")))?;
            prop_assert_eq!(
                leaves
                    .iter()
                    .map(|leaf| (leaf.key, leaf.claim))
                    .collect::<History>(),
                history.clone()
            );
            prop_assert!(leaves.windows(2).all(|pair| pair[0].key < pair[1].key));
            prop_assert_eq!(memoized_count(&image.objects, image.root), Ok(history.len() as u64));
            Ok(())
        })
        .expect("model radix histories round-trip");
}

#[test]
fn radix_count_is_entry_occurrences_under_leaf_sharing() {
    let zero = [0_u8; 32];
    let history =
        History::from([(model_key(state(0, zero, 50)), 50), (model_key(state(1, zero, 50)), 50)]);
    let image = build_radix(&history);
    assert_eq!(image.objects.len(), 2);
    let leaves = unfold_radix(&image.objects, image.root, 2).expect("the shared history unfolds");
    assert_eq!(leaves.len(), 2);
    assert_eq!(memoized_count(&image.objects, image.root), Ok(2));
    let distinct = leaves
        .iter()
        .map(|leaf| leaf.object)
        .collect::<BTreeSet<_>>();
    assert_eq!(distinct.len(), 1);
    assert_eq!(
        leaves
            .iter()
            .map(|leaf| model_decode_key(&leaf.key))
            .collect::<Vec<_>>(),
        vec![Some(state(0, zero, 50)), Some(state(1, zero, 50))]
    );

    runner(256)
        .run(&product_history(), |(history, generations, digests)| {
            let image = build_radix(&history);
            let count = history.len() as u64;
            prop_assert_eq!(count, (generations * digests) as u64);
            let leaves = unfold_radix(&image.objects, image.root, count)
                .map_err(|refusal| TestCaseError::fail(format!("{refusal:?}")))?;
            prop_assert_eq!(leaves.len() as u64, count);
            prop_assert_eq!(memoized_count(&image.objects, image.root), Ok(count));
            let distinct = leaves
                .iter()
                .map(|leaf| leaf.object)
                .collect::<BTreeSet<_>>();
            prop_assert_eq!(distinct.len(), digests);
            prop_assert!(distinct.len() < leaves.len());
            prop_assert_eq!(
                unfold_radix(&image.objects, image.root, distinct.len() as u64).err(),
                Some(RadixRefusal::CountMismatch {
                    declared: distinct.len() as u64,
                    unfolded: count
                })
            );
            prop_assert!(
                RetentionAnchorV2::new(
                    retained(state(u64::MAX, [0xcc; 32], 64), 4096),
                    distinct.len() as u64,
                    image.root,
                )
                .is_ok()
            );
            Ok(())
        })
        .expect("radix counts are entry occurrences");
}

struct ClosureSummary {
    entries: usize,
    members: usize,
}

fn check_metadata_closure(history: &History) -> ClosureSummary {
    let image = build_radix(history);
    let anchor = history_anchor(history, &image);
    let anchor_bytes = encoded(&anchor);
    let anchor_digest = anchor.object_ref().digest();
    let members = metadata_closure(anchor, &image).expect("model radix images are readable");
    let expected = image
        .objects
        .keys()
        .copied()
        .chain([anchor_digest])
        .collect::<BTreeSet<_>>();
    assert_eq!(members.keys().copied().collect::<BTreeSet<_>>(), expected);
    assert!(members.contains_key(&anchor.historical_root().digest()));

    let mut metadata_bytes = 0_u64;
    for (digest, member) in &members {
        metadata_bytes += member.encoded_len();
        if *digest == anchor_digest {
            assert_eq!(reference_of(&anchor_bytes), *member);
            assert_eq!(&anchor_bytes[..16], MODEL_ANCHOR_DOMAIN);
            continue;
        }
        let bytes = &image.objects[digest];
        assert_eq!(reference_of(bytes), *member);
        assert_eq!(&bytes[..16], MODEL_RADIX_DOMAIN);
        assert_ne!(ObjectDigest::of_canonical_bytes(&with_domain(bytes, DAG_NODE_DOMAIN)), *digest);
        let node = load_radix_node(&image.objects, *member).expect("members are radix nodes");
        for child in node.children {
            assert!(members.contains_key(&child.target.digest()));
        }
    }
    assert!(members.len() >= 2);
    assert!(metadata_bytes >= 186);
    for semantic in
        [anchor.current().state().manifest(), reference_of(&encoded(&CanonicalDagNode::empty()))]
    {
        assert!(!members.contains_key(&semantic.digest()));
    }
    let leaves = unfold_radix(&image.objects, image.root, history.len() as u64)
        .expect("the history unfolds");
    assert_eq!(leaves.len(), history.len());
    assert_eq!(memoized_count(&image.objects, image.root), Ok(history.len() as u64));
    ClosureSummary {
        entries: leaves.len(),
        members: members.len(),
    }
}

#[test]
fn metadata_closure_is_complete_closed_and_domain_separated() {
    runner(256)
        .run(&history_strategy(24), |history| {
            check_metadata_closure(&history);
            Ok(())
        })
        .expect("metadata closures are complete, closed and domain separated");

    let large = (0..4096_u64)
        .map(|generation| {
            let mut digest = [0x5a; 32];
            digest[24..].copy_from_slice(&generation.to_be_bytes());
            (model_key(state(generation, digest, 50)), 50 + generation % 7)
        })
        .collect::<History>();
    let summary = std::thread::Builder::new()
        .name("retention-v2-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(move || check_metadata_closure(&large))
        .expect("the small-stack thread starts")
        .join()
        .expect("the 4096-entry closure fits a 256 KiB stack");
    assert_eq!(summary.entries, 4096);
    assert_eq!(summary.members, 1 + 1 + 16 + 4096);
}

#[test]
fn v2_checkpoint_data_is_the_disjoint_union_of_retained_and_metadata() {
    runner(256)
        .run(&(retained_union(), history_strategy(16)), |(retained, history)| {
            let image = build_radix(&history);
            let anchor = history_anchor(&history, &image);
            let members = metadata_closure(anchor, &image)
                .map_err(|refusal| TestCaseError::fail(format!("{refusal:?}")))?;
            let metadata = members.values().copied().collect::<Vec<_>>();
            let retained_digests = retained
                .iter()
                .map(|reference| reference.digest())
                .collect::<BTreeSet<_>>();
            prop_assert!(
                metadata
                    .iter()
                    .all(|reference| !retained_digests.contains(&reference.digest()))
            );

            let retained_bytes = retained
                .iter()
                .map(|reference| reference.encoded_len())
                .sum::<u64>();
            let metadata_bytes = metadata
                .iter()
                .map(|reference| reference.encoded_len())
                .sum::<u64>();
            let mut merged = retained
                .iter()
                .chain(&metadata)
                .copied()
                .collect::<Vec<_>>();
            merged.sort_unstable_by_key(|reference| reference.digest());
            let objects = (retained.len() + metadata.len()) as u64;
            let bytes = retained_bytes + metadata_bytes;
            let mut builder = OrderedObjectInventoryBuilderV1::new(objects, bytes);
            for reference in &merged {
                builder
                    .push(*reference)
                    .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
            }
            let ordered = builder
                .finish()
                .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
            prop_assert_eq!(ordered.objects(), objects);
            prop_assert_eq!(ordered.canonical_bytes(), bytes);
            prop_assert_eq!(ordered.digest(), independent_inventory_digest(&merged));

            let report = RetainedUnionClosureReportV1 {
                distinct_objects: retained.len() as u64,
                canonical_bytes: retained_bytes,
            };
            prop_assert_eq!(
                CheckpointObjectInventoryV1::from_test_ordered(
                    report,
                    anchor.object_ref(),
                    ordered
                )
                .err(),
                Some(GenerationCheckpointErrorV1::InventoryIncoherent {
                    expected_objects: retained.len() as u64 + 1,
                    actual_objects: objects,
                    expected_bytes: retained_bytes + RETENTION_ANCHOR_CANONICAL_LEN,
                    actual_bytes: bytes,
                })
            );
            let anchor_only = retained_digests
                .iter()
                .copied()
                .chain([anchor.object_ref().digest()])
                .collect::<BTreeSet<_>>();
            prop_assert!(!anchor_only.contains(&anchor.historical_root().digest()));
            Ok(())
        })
        .expect("V2 checkpoint data is the disjoint union of retained and metadata objects");
}

#[test]
fn v1_object_readers_refuse_every_frozen_v2_object() {
    let leaf = build_radix(&History::from([(model_key(state(1, [0xbb; 32], 50)), 50)]));
    let current = retained(state(1, [0xaa; 32], 50), 50);
    let empty_history = encoded(
        &RetentionAnchorV2::new(current, 0, empty_retention_radix_ref())
            .expect("an empty history appoints the empty radix node"),
    );
    for object in [
        EMPTY_RETENTION_RADIX_NODE.to_vec(),
        leaf.objects[&leaf.root.digest()].clone(),
        empty_history,
    ] {
        assert_v1_readers_refuse(&object);
    }
    let v1_manifest =
        encoded(&RetentionManifestV1::new(current, Vec::new()).expect("a V1 manifest is valid"));
    assert_eq!(
        RetentionAnchorV2::parse(&v1_manifest),
        Err(CanonicalFormatError::InvalidDomain {
            expected: RETENTION_ANCHOR_DOMAIN
        })
    );

    runner(64)
        .run(
            &(history_strategy(12), state_strategy(), 0_u64..64),
            |(history, current_state, extra)| {
                let image = build_radix(&history);
                let claim = current_state.manifest().encoded_len().saturating_add(extra);
                let anchor = RetentionAnchorV2::new(
                    retained(current_state, claim),
                    history.len() as u64,
                    image.root,
                )
                .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
                assert_v1_readers_refuse(&encoded(&anchor));
                for bytes in image.objects.values() {
                    assert_v1_readers_refuse(bytes);
                }
                Ok(())
            },
        )
        .expect("V1 readers refuse every V2 object");
}

#[test]
fn metadata_and_semantic_universes_are_disjoint_and_collisions_fail_closed() {
    let new_domains =
        [RETENTION_ANCHOR_DOMAIN, RETENTION_RADIX_DOMAIN, GENERATION_CHECKPOINT_V2_DOMAIN];
    let domains = EXISTING_DOMAINS
        .iter()
        .copied()
        .chain(new_domains)
        .collect::<BTreeSet<_>>();
    assert_eq!(domains.len(), EXISTING_DOMAINS.len() + new_domains.len());
    for domain in [DAG_NODE_DOMAIN, RETENTION_MANIFEST_DOMAIN, GENERATION_CHECKPOINT_DOMAIN] {
        assert!(EXISTING_DOMAINS.contains(&domain));
    }
    for domain in new_domains {
        let object = [&domain[..], &[0_u8; 34][..]].concat();
        assert!(matches!(
            CanonicalObjectKind::from_domain_prefix(&object),
            Err(CanonicalFormatError::UnknownObjectDomain)
        ));
    }

    let empty_semantic = encoded(&CanonicalDagNode::empty());
    let empty_radix = EMPTY_RETENTION_RADIX_NODE.to_vec();
    assert_eq!(empty_semantic[16..], empty_radix[16..]);
    assert_ne!(reference_of(&empty_semantic), reference_of(&empty_radix));
    let shared_domain_mutant = with_domain(&empty_radix, DAG_NODE_DOMAIN);
    assert_eq!(shared_domain_mutant, empty_semantic);
    let mut builder = OrderedObjectInventoryBuilderV1::new(2, 100);
    builder
        .push(reference_of(&empty_semantic))
        .expect("the semantic root enters the inventory");
    assert!(matches!(
        builder.push(reference_of(&shared_domain_mutant)),
        Err(GenerationCheckpointErrorV1::DuplicateInventoryDigest { .. })
    ));

    runner(64)
        .run(
            &(retained_union(), history_strategy(8), any::<prop::sample::Index>()),
            |(retained, history, pick)| {
                let image = build_radix(&history);
                let anchor = history_anchor(&history, &image);
                let members = metadata_closure(anchor, &image)
                    .map_err(|refusal| TestCaseError::fail(format!("{refusal:?}")))?;
                let metadata = members.values().copied().collect::<Vec<_>>();
                let colliding = metadata[pick.index(metadata.len())];
                let impostor = ObjectRefV1::new(colliding.digest(), colliding.encoded_len() + 1)
                    .expect("the impostor length stays above the envelope");
                let mut merged = retained
                    .iter()
                    .chain(&metadata)
                    .copied()
                    .chain([impostor])
                    .collect::<Vec<_>>();
                merged.sort_by_key(|reference| reference.digest());
                let objects = merged.len() as u64;
                let bytes = merged
                    .iter()
                    .map(|reference| reference.encoded_len())
                    .sum::<u64>();
                let mut builder = OrderedObjectInventoryBuilderV1::new(objects, bytes);
                let refused = merged
                    .iter()
                    .map(|reference| builder.push(*reference))
                    .find_map(Result::err);
                prop_assert_eq!(
                    refused,
                    Some(GenerationCheckpointErrorV1::DuplicateInventoryDigest {
                        digest: colliding.digest(),
                        first_len: colliding.encoded_len(),
                        second_len: impostor.encoded_len(),
                    })
                );
                Ok(())
            },
        )
        .expect("cross-universe digest collisions fail closed");
}

#[test]
fn v1_migration_preserves_entries_order_count_aliases_and_roots() {
    let aliases = [
        state(5, [0x21; 32], 50),
        state(5, [0x21; 32], 51),
        state(5, [0x22; 32], 50),
        state(6, [0x21; 32], 50),
    ]
    .into_iter()
    .map(|state| (model_key(state), 64))
    .collect::<History>();
    let check = |history: &History, seed: u8| -> Result<(), TestCaseError> {
        let (manifest, manifest_bytes) = semantic_manifest(seed);
        let current_state = StateVersionV1::new(u64::MAX, reference_of(&manifest_bytes));
        let current = retained(current_state, current_state.manifest().encoded_len() + 1);
        let historical = history
            .iter()
            .map(|(key, claim)| retained(model_decode_key(key).expect("model keys decode"), *claim))
            .collect::<Vec<_>>();
        let v1 = RetentionManifestV1::new(current, historical)
            .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
        let v1_bytes = encoded(&v1);

        let migrated = v1
            .historical()
            .iter()
            .map(|entry| (model_key(entry.state()), entry.canonical_bytes()))
            .collect::<History>();
        prop_assert_eq!(&migrated, history);
        let image = build_radix(&migrated);
        let anchor = RetentionAnchorV2::new(v1.current(), v1.historical().len() as u64, image.root)
            .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
        let anchor_bytes = encoded(&anchor);

        let read = RetentionAnchorV2::parse(&anchor_bytes)
            .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
        prop_assert_eq!(read, anchor);
        prop_assert_eq!(read.current(), v1.current());
        prop_assert_eq!(read.historical_count(), v1.historical().len() as u64);
        let leaves = unfold_radix(&image.objects, read.historical_root(), read.historical_count())
            .map_err(|refusal| TestCaseError::fail(format!("{refusal:?}")))?;
        let restored = leaves
            .iter()
            .map(|leaf| {
                retained(model_decode_key(&leaf.key).expect("leaf keys decode"), leaf.claim)
            })
            .collect::<Vec<_>>();
        prop_assert_eq!(&restored[..], v1.historical());
        prop_assert_eq!(
            encoded(
                &RetentionManifestV1::new(read.current(), restored)
                    .map_err(|error| TestCaseError::fail(format!("{error:?}")))?
            ),
            v1_bytes.clone()
        );
        prop_assert_eq!(&anchor_bytes[32..88], &v1_bytes[32..88]);

        prop_assert_eq!(read.current().state().manifest(), reference_of(&manifest_bytes));
        let roots = SemanticStateManifestView::parse(&manifest_bytes)
            .map_err(|error| TestCaseError::fail(format!("{error:?}")))?
            .roots();
        prop_assert_eq!(roots, manifest.roots());
        Ok(())
    };
    check(&aliases, 0).expect("generation, digest and length aliases survive migration");

    runner(256)
        .run(&(history_strategy(16), any::<u8>()), |(history, seed)| check(&history, seed))
        .expect("V1 migration preserves entries, order, count, aliases and roots");
}

#[test]
fn resume_binding_separates_v1_and_v2_representations_of_one_state() {
    runner(256)
        .run(
            &(
                state_strategy(),
                0_u64..1024,
                digest_strategy(),
                digest_strategy(),
                length_strategy(),
            ),
            |(state, extra, v1_digest, v2_digest, encoded_len)| {
                let current = retained(state, state.manifest().encoded_len().saturating_add(extra));
                let v1 = RetentionManifestV1::new(current, Vec::new())
                    .map_err(|error| TestCaseError::fail(format!("{error:?}")))?
                    .object_ref();
                let v2 = RetentionAnchorV2::new(current, 0, empty_retention_radix_ref())
                    .map_err(|error| TestCaseError::fail(format!("{error:?}")))?
                    .object_ref();
                for (v1_reference, v2_reference) in [
                    (v1, v2),
                    (reference(v1_digest, encoded_len), reference(v2_digest, encoded_len)),
                    (v1, v1),
                ] {
                    prop_assert_ne!(
                        session_binding(state, RetentionFormat::V1, v1_reference),
                        session_binding(state, RetentionFormat::V2, v2_reference)
                    );
                    prop_assert_eq!(
                        state_version_only_binding(state, RetentionFormat::V1, v1_reference),
                        state_version_only_binding(state, RetentionFormat::V2, v2_reference)
                    );
                }
                prop_assert_eq!(
                    session_binding(state, RetentionFormat::V2, v2),
                    session_binding(state, RetentionFormat::V2, v2)
                );
                Ok(())
            },
        )
        .expect("resume bindings separate V1 and V2 representations");
}

#[test]
fn arbitrary_retention_v2_bytes_are_total() {
    runner(64)
        .run(
            &(
                prop::collection::vec(any::<u8>(), 0..=160),
                history_strategy(6),
                any::<prop::sample::Index>(),
                1_u8..=255,
                any::<prop::sample::Index>(),
            ),
            |(arbitrary, history, position, flip, cut)| {
                let image = build_radix(&history);
                let anchor = encoded(&history_anchor(&history, &image));
                let key = model_key(state(9, [0xcc; 32], 50)).to_vec();
                let mut candidates = vec![arbitrary];
                for valid in [anchor, key]
                    .into_iter()
                    .chain(image.objects.values().cloned())
                {
                    let mut flipped = valid.clone();
                    flipped[position.index(valid.len())] ^= flip;
                    candidates.push(flipped);
                    candidates.push(valid[..cut.index(valid.len())].to_vec());
                    let mut extended = valid;
                    extended.push(flip);
                    candidates.push(extended);
                }
                for bytes in &candidates {
                    if let Ok(key) = RetentionKeyV2::parse(bytes) {
                        prop_assert_eq!(&key.as_bytes()[..], &bytes[..]);
                        prop_assert_eq!(RetentionKeyV2::new(key.state()), key);
                    }
                    if let Ok(anchor) = RetentionAnchorV2::parse(bytes) {
                        prop_assert_eq!(&encoded(&anchor), bytes);
                    }
                    if let Ok(reference) = ObjectRefV1::new(
                        ObjectDigest::of_canonical_bytes(bytes),
                        bytes.len() as u64,
                    ) {
                        let mut objects = image.objects.clone();
                        objects.insert(reference.digest(), bytes.clone());
                        let _ = unfold_radix(&objects, reference, history.len() as u64);
                        let _ = memoized_count(&objects, reference);
                    }
                }
                Ok(())
            },
        )
        .expect("retention V2 parsers are total");
}

const RADIX_NODE_NAME: &str = "RSpace retention radix node";
const DAG_NODE_NAME: &str = "RSpace DAG node";
const V1_CHECKPOINT_NAME: &str = "RSpace generation checkpoint";
const UNKNOWN_METADATA_DOMAIN: &[u8; 16] = b"RSpaceDAG.RetX\0\0";
const READER_DOMAINS: [&[u8; 16]; 11] = [
    b"RSpaceDAG.Node\0\0",
    b"RSpaceDAG.Blob\0\0",
    b"RSpaceDAG.SMan\0\0",
    b"RSpaceDAG.AMan\0\0",
    b"RSpaceDAG.Retn\0\0",
    b"RSpaceDAG.Chkpt\0",
    b"RSpace.Blob.CSrc",
    MODEL_ANCHOR_DOMAIN,
    MODEL_RADIX_DOMAIN,
    MODEL_CHECKPOINT_V2_DOMAIN,
    UNKNOWN_METADATA_DOMAIN,
];
const STREAM_WIDTHS: [usize; 12] = [2, 7, 15, 16, 31, 32, 39, 40, 41, 255, 256, 257];
const ENUMERABLE_ENTRIES: u64 = 4096;

type ObjectIdentity = Result<(CanonicalObjectKind, ObjectRefV1), CanonicalFormatError>;
type AppointedSummary =
    Result<(ProductionRetentionFormat, RetainedStateV1, u64), CanonicalFormatError>;
type AdmittedImage = (Vec<RetainedStateV1>, Vec<ObjectRefV1>, u64);

fn fail(message: String) -> TestCaseError { TestCaseError::fail(message) }

fn model_current() -> RetainedStateV1 { retained(state(u64::MAX, [0xcc; 32], 64), 4096) }

fn renamed_version(error: CanonicalFormatError, name: &'static str) -> CanonicalFormatError {
    match error {
        CanonicalFormatError::UnsupportedVersion { version, .. } => {
            CanonicalFormatError::UnsupportedVersion {
                domain: name,
                version,
            }
        }
        error => error,
    }
}

fn with_version(bytes: &[u8], version: u64) -> Vec<u8> {
    let mut versioned = bytes.to_vec();
    versioned[16..24].copy_from_slice(&version.to_le_bytes());
    versioned
}

/// The V1 object domains, written out independently of the production
/// classifier.
fn v1_domain_kind(bytes: &[u8]) -> Result<CanonicalObjectKind, CanonicalFormatError> {
    let Some(domain) = bytes.get(..16) else {
        return Err(CanonicalFormatError::Truncated {
            offset: 0,
            needed: 16,
            available: bytes.len(),
        });
    };
    match domain {
        b"RSpaceDAG.Node\0\0" => Ok(CanonicalObjectKind::DagNode),
        b"RSpaceDAG.Blob\0\0" => Ok(CanonicalObjectKind::Blob),
        b"RSpaceDAG.SMan\0\0" => Ok(CanonicalObjectKind::SemanticManifest),
        b"RSpaceDAG.AMan\0\0" => Ok(CanonicalObjectKind::AcceleratorManifest),
        b"RSpaceDAG.Retn\0\0" => Ok(CanonicalObjectKind::RetentionManifest),
        _ => Err(CanonicalFormatError::UnknownObjectDomain),
    }
}

fn dual_domain_kind(bytes: &[u8]) -> Result<CanonicalObjectKind, CanonicalFormatError> {
    match (v1_domain_kind(bytes), bytes.get(..16)) {
        (Err(CanonicalFormatError::UnknownObjectDomain), Some(b"RSpaceDAG.RetA\0\0")) => {
            Ok(CanonicalObjectKind::RetentionAnchor)
        }
        (Err(CanonicalFormatError::UnknownObjectDomain), Some(b"RSpaceDAG.RetR\0\0")) => {
            Ok(CanonicalObjectKind::RetentionRadixNode)
        }
        (classified, _) => classified,
    }
}

fn strict_identity(bytes: &[u8]) -> ObjectIdentity {
    let object = CanonicalObjectView::parse(bytes)?;
    Ok((object.kind(), reference_of(bytes)))
}

/// The dual reader's contiguous oracle: the anchor parser on the anchor
/// domain, the unchanged DAG parser after a retag on the radix domain, and
/// the strict reader everywhere else.
fn dual_identity(bytes: &[u8]) -> ObjectIdentity {
    match bytes.get(..16) {
        Some(domain) if domain == MODEL_ANCHOR_DOMAIN => {
            RetentionAnchorV2::parse(bytes)?;
            Ok((CanonicalObjectKind::RetentionAnchor, reference_of(bytes)))
        }
        Some(domain) if domain == MODEL_RADIX_DOMAIN => {
            CanonicalDagNodeView::parse(&with_domain(bytes, DAG_NODE_DOMAIN))
                .map_err(|error| renamed_version(error, RADIX_NODE_NAME))?;
            Ok((CanonicalObjectKind::RetentionRadixNode, reference_of(bytes)))
        }
        _ => strict_identity(bytes),
    }
}

fn stream_identity(bytes: &[u8], reader: RetentionReader, widths: &[usize]) -> ObjectIdentity {
    let mut validator = CanonicalObjectStreamValidatorV1::new_for(bytes.len(), reader);
    let mut offset = 0_usize;
    for width in widths {
        let end = offset.saturating_add(*width).min(bytes.len());
        validator.feed(&bytes[offset..end])?;
        offset = end;
    }
    if offset < bytes.len() {
        validator.feed(&bytes[offset..])?;
    }
    let identity = validator.finish()?;
    Ok((identity.kind(), identity.reference()))
}

/// The whole input, one-byte steps, every split for inputs of at most 160
/// bytes (a stride above that), and the fixed chunk widths.
fn stream_partitions(len: usize) -> Vec<Vec<usize>> {
    let stride = match len {
        0..=160 => 1,
        _ => len / 64 + 1,
    };
    let mut partitions = Vec::with_capacity(2 + len / stride + 1 + STREAM_WIDTHS.len());
    partitions.push(vec![usize::MAX]);
    partitions.push(vec![1; len]);
    partitions.extend(
        (0..=len)
            .step_by(stride)
            .map(|split| vec![split, len - split]),
    );
    partitions.extend(
        STREAM_WIDTHS
            .iter()
            .map(|width| vec![*width; len / width + 2]),
    );
    partitions
}

fn declared_reference(bytes: &[u8]) -> ObjectRefV1 {
    ObjectRefV1::new(ObjectDigest::of_canonical_bytes(bytes), (bytes.len() as u64).max(32))
        .expect("the declared length covers an envelope")
}

fn expected_validation(
    identity: &ObjectIdentity,
    declared: ObjectRefV1,
) -> Result<CanonicalObjectKind, LogPackFormatError> {
    match identity {
        Ok((kind, actual)) if *actual == declared => Ok(*kind),
        Ok((_, actual)) => Err(LogPackFormatError::ObjectReference {
            declared,
            actual: *actual,
        }),
        Err(error) => Err(LogPackFormatError::Canonical(error.clone())),
    }
}

fn check_object_readers(bytes: &[u8]) -> Result<(), TestCaseError> {
    let strict = strict_identity(bytes);
    let dual = dual_identity(bytes);
    prop_assert_eq!(CanonicalObjectKind::from_domain_prefix(bytes), v1_domain_kind(bytes));
    prop_assert_eq!(
        CanonicalObjectKind::from_domain_prefix_for(bytes, RetentionReader::V1Only),
        v1_domain_kind(bytes)
    );
    prop_assert_eq!(
        CanonicalObjectKind::from_domain_prefix_for(bytes, RetentionReader::Dual),
        dual_domain_kind(bytes)
    );
    if let Ok((kind, _)) = &strict {
        prop_assert_eq!(Ok(*kind), v1_domain_kind(bytes));
    }
    match bytes.get(..16) {
        Some(domain) if domain == MODEL_ANCHOR_DOMAIN || domain == MODEL_RADIX_DOMAIN => {
            prop_assert_eq!(strict.clone().err(), Some(CanonicalFormatError::UnknownObjectDomain));
        }
        _ => prop_assert_eq!(&dual, &strict),
    }
    let declared = declared_reference(bytes);
    let foreign = ObjectRefV1::new(ObjectDigest::from_array([0x5c; 32]), declared.encoded_len())
        .expect("the foreign declaration covers an envelope");
    for (reader, expected) in [(RetentionReader::V1Only, &strict), (RetentionReader::Dual, &dual)] {
        for widths in stream_partitions(bytes.len()) {
            prop_assert_eq!(
                &stream_identity(bytes, reader, &widths),
                expected,
                "reader {:?} widths {:?}",
                reader,
                widths
            );
        }
        for declaration in [declared, foreign] {
            prop_assert_eq!(
                validate_object_for(declaration, bytes, reader),
                expected_validation(expected, declaration)
            );
        }
    }
    Ok(())
}

fn check_descriptor_readers(bytes: &[u8]) -> Result<(), TestCaseError> {
    let strict = GenerationCheckpointDescriptorViewV1::parse(bytes);
    prop_assert_eq!(
        GenerationCheckpointDescriptorViewV1::parse_for(bytes, RetentionReader::V1Only),
        strict.clone()
    );
    if let Ok(view) = strict {
        prop_assert_eq!(view.format(), ProductionRetentionFormat::V1);
        prop_assert_eq!(&encoded(&GenerationCheckpointDescriptorV1::from_view(view)), bytes);
    }
    let dual = GenerationCheckpointDescriptorViewV1::parse_for(bytes, RetentionReader::Dual);
    match bytes.get(..16) {
        Some(domain) if domain == MODEL_CHECKPOINT_V2_DOMAIN => {
            prop_assert_eq!(
                strict.err(),
                Some(GenerationCheckpointErrorV1::Canonical(CanonicalFormatError::InvalidDomain {
                    expected: GENERATION_CHECKPOINT_DOMAIN,
                }))
            );
            let retagged = GenerationCheckpointDescriptorViewV1::parse(&with_domain(
                bytes,
                GENERATION_CHECKPOINT_DOMAIN,
            ));
            match (&dual, &retagged) {
                (Err(GenerationCheckpointErrorV1::Canonical(error)), _) => {
                    prop_assert_eq!(
                        retagged.clone().err(),
                        Some(GenerationCheckpointErrorV1::Canonical(renamed_version(
                            error.clone(),
                            V1_CHECKPOINT_NAME,
                        )))
                    );
                }
                (_, Err(GenerationCheckpointErrorV1::Canonical(error))) => {
                    return Err(fail(format!(
                        "the retagged strict parse refuses {error:?} but the dual parse gives \
                         {dual:?}"
                    )));
                }
                (Ok(view), _) => {
                    prop_assert_eq!(view.format(), ProductionRetentionFormat::V2);
                    prop_assert_eq!(
                        &encoded(&GenerationCheckpointDescriptorV1::from_view(*view)),
                        bytes
                    );
                    prop_assert_eq!(
                        view.retention_manifest().encoded_len(),
                        RETENTION_ANCHOR_CANONICAL_LEN
                    );
                    prop_assert!(view.data_objects() >= view.retained_union_objects() + 2);
                    prop_assert!(
                        view.data_canonical_bytes().get() >=
                            view.retained_union_canonical_bytes().get() + 186
                    );
                }
                (Err(_), _) => {}
            }
        }
        _ => prop_assert_eq!(dual, strict),
    }
    Ok(())
}

fn appointed_summary(bytes: &[u8], reader: RetentionReader) -> AppointedSummary {
    let appointed = AppointedRetention::parse(bytes, reader)?;
    Ok((appointed.format(), appointed.current(), appointed.historical_count()?))
}

fn check_appointed_readers(bytes: &[u8]) -> Result<(), TestCaseError> {
    let v1 = RetentionManifestView::parse(bytes).map(|manifest| {
        (ProductionRetentionFormat::V1, manifest.current(), manifest.historical().len() as u64)
    });
    let dual = match bytes.get(..16) {
        Some(domain) if domain == MODEL_ANCHOR_DOMAIN => {
            RetentionAnchorV2::parse(bytes).map(|anchor| {
                (ProductionRetentionFormat::V2, anchor.current(), anchor.historical_count())
            })
        }
        _ => v1.clone(),
    };
    prop_assert_eq!(appointed_summary(bytes, RetentionReader::V1Only), v1);
    prop_assert_eq!(appointed_summary(bytes, RetentionReader::Dual), dual);
    Ok(())
}

fn check_all_readers(bytes: &[u8]) -> Result<(), TestCaseError> {
    check_object_readers(bytes)?;
    check_descriptor_readers(bytes)?;
    check_appointed_readers(bytes)
}

fn reader_descriptor(
    format: ProductionRetentionFormat,
    retained_union: &[ObjectRefV1],
    retention: ObjectRefV1,
    metadata: &[ObjectRefV1],
) -> Vec<u8> {
    let retained_bytes = retained_union
        .iter()
        .map(|reference| reference.encoded_len())
        .sum::<u64>();
    let mut data = Vec::with_capacity(retained_union.len() + metadata.len() + 1);
    data.extend_from_slice(retained_union);
    match format {
        ProductionRetentionFormat::V1 => data.push(retention),
        ProductionRetentionFormat::V2 => data.extend_from_slice(metadata),
    }
    data.sort_unstable_by_key(|reference| reference.digest());
    let data_bytes = data
        .iter()
        .map(|reference| reference.encoded_len())
        .sum::<u64>();
    let mut builder = OrderedObjectInventoryBuilderV1::new(data.len() as u64, data_bytes);
    for reference in &data {
        builder
            .push(*reference)
            .expect("reader DATA is strictly ordered");
    }
    let ordered = builder.finish().expect("reader DATA is complete");
    let report = RetainedUnionClosureReportV1 {
        distinct_objects: retained_union.len() as u64,
        canonical_bytes: retained_bytes,
    };
    let inventory = match format {
        ProductionRetentionFormat::V1 => {
            CheckpointObjectInventoryV1::from_test_ordered(report, retention, ordered)
        }
        ProductionRetentionFormat::V2 => CheckpointObjectInventoryV1::from_test_ordered_v2(
            report,
            metadata.len() as u64,
            data_bytes - retained_bytes,
            ordered,
        ),
    }
    .expect("reader inventories are coherent");
    let current = retained_union[0];
    encoded(
        &GenerationCheckpointDescriptorV1::from_test_inventory(
            TEST_SEMANTIC_PROFILE_ID,
            StateVersionV1::new(7, current),
            retention,
            1,
            current.encoded_len(),
            inventory,
        )
        .expect("reader descriptors are valid"),
    )
}

/// Valid objects of every V1 kind, the V2 anchor and radix nodes of one
/// history, and a V1 and a V2 checkpoint descriptor over one retained union.
fn reader_objects(history: &History, retained_union: &[ObjectRefV1], seed: u8) -> Vec<Vec<u8>> {
    let image = build_radix(history);
    let anchor = history_anchor(history, &image);
    let metadata = metadata_closure(anchor, &image)
        .expect("model radix images are readable")
        .into_values()
        .collect::<Vec<_>>();
    let historical = history
        .iter()
        .map(|(key, claim)| retained(model_decode_key(key).expect("model keys decode"), *claim))
        .collect::<Vec<_>>();
    let v1_manifest = encoded(
        &RetentionManifestV1::new(anchor.current(), historical)
            .expect("the model history is a V1 history"),
    );
    let (semantic, semantic_bytes) = semantic_manifest(seed);
    let accelerator = AcceleratorManifestV1::new(semantic.object_ref(), 7, [
        AcceleratorArtifact {
            kind: AcceleratorArtifactKind::ChannelReceives,
            object: reference([seed; 32], 64),
        },
        AcceleratorArtifact {
            kind: AcceleratorArtifactKind::ActImage,
            object: reference([seed ^ 0x10; 32], 65),
        },
    ])
    .expect("the accelerator manifest is valid");
    let blob = CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, vec![seed; 17])
        .expect("the blob is valid");
    let wide =
        encoded(&dag_node(&[seed, 2, 3], EndpointValue::Unit, 17).expect("the wide node is valid"));
    let mut objects = Vec::with_capacity(image.objects.len() + 11);
    objects.push(encoded(&anchor));
    objects.extend(image.objects.values().cloned());
    objects.push(with_domain(&wide, MODEL_RADIX_DOMAIN));
    objects.push(v1_manifest.clone());
    objects.push(semantic_bytes);
    objects.push(encoded(&accelerator));
    objects.push(encoded(&blob));
    objects.push(wide);
    objects.push(encoded(&CanonicalDagNode::empty()));
    objects.push(reader_descriptor(
        ProductionRetentionFormat::V1,
        retained_union,
        reference_of(&v1_manifest),
        &[],
    ));
    objects.push(reader_descriptor(
        ProductionRetentionFormat::V2,
        retained_union,
        anchor.object_ref(),
        &metadata,
    ));
    objects
}

#[test]
fn dual_object_readers_extend_v1_readers_only_on_metadata_domains() {
    for (reader, format, admitted) in [
        (RetentionReader::V1Only, ProductionRetentionFormat::V1, true),
        (RetentionReader::V1Only, ProductionRetentionFormat::V2, false),
        (RetentionReader::Dual, ProductionRetentionFormat::V1, true),
        (RetentionReader::Dual, ProductionRetentionFormat::V2, true),
    ] {
        assert_eq!(reader.admits(format), admitted, "{reader:?} admits {format:?}");
    }

    let aliases = History::from([
        (model_key(state(1, [0x11; 32], 50)), 50),
        (model_key(state(2, [0x11; 32], 50)), 60),
        (model_key(state(2, [0x11; 32], 51)), 51),
    ]);
    let fixed =
        reader_objects(&aliases, &[reference([0x31; 32], 64), reference([0x32; 32], 96)], 7)
            .into_iter()
            .chain(reader_objects(&History::new(), &[reference([0x41; 32], 50)], 9));
    for valid in fixed {
        let candidates = READER_DOMAINS
            .iter()
            .map(|domain| with_domain(&valid, domain))
            .chain([with_version(&valid, 2), valid.clone()]);
        for bytes in candidates {
            check_all_readers(&bytes).expect("fixed reader candidates agree with their oracles");
        }
    }

    runner(64)
        .run(
            &(
                history_strategy(6),
                retained_union(),
                any::<u8>(),
                any::<prop::sample::Index>(),
                any::<prop::sample::Index>(),
                1_u8..=255,
                any::<prop::sample::Index>(),
                any::<prop::sample::Index>(),
            ),
            |(history, union, seed, pick, position, flip, cut, domain)| {
                let objects = reader_objects(&history, &union, seed);
                let valid = &objects[pick.index(objects.len())];
                let mut flipped = valid.clone();
                flipped[position.index(valid.len())] ^= flip;
                let mut extended = valid.clone();
                extended.push(flip);
                for bytes in [
                    valid.clone(),
                    flipped,
                    valid[..cut.index(valid.len())].to_vec(),
                    extended,
                    with_version(valid, 2),
                    with_domain(valid, READER_DOMAINS[domain.index(READER_DOMAINS.len())]),
                ] {
                    check_all_readers(&bytes)?;
                }
                Ok(())
            },
        )
        .expect("dual readers extend V1 readers only on the metadata domains");
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AdmissionClass {
    Radix(RadixRefusal),
    History(CanonicalFormatError),
    Unmapped(String),
}

/// The correspondence from production refusals to reference refusals:
/// every store-level length or digest disagreement is a reference mismatch,
/// body errors agree modulo the envelope name, and every other refusal maps
/// one to one.
fn production_class(error: RetentionRadixErrorV2<CanonicalFormatError>) -> AdmissionClass {
    match error {
        RetentionRadixErrorV2::Store(CanonicalFormatError::ResourceLimit {
            resource: "materialized object bytes",
            ..
        }) |
        RetentionRadixErrorV2::Canonical(
            CanonicalFormatError::ObjectLengthMismatch { .. } |
            CanonicalFormatError::DigestMismatch { .. },
        ) => AdmissionClass::Radix(RadixRefusal::ReferenceMismatch),
        RetentionRadixErrorV2::Canonical(CanonicalFormatError::MissingObject(_)) => {
            AdmissionClass::Radix(RadixRefusal::MissingObject)
        }
        RetentionRadixErrorV2::Canonical(error) => AdmissionClass::History(error),
        RetentionRadixErrorV2::Store(error) => AdmissionClass::Unmapped(format!("store {error:?}")),
        RetentionRadixErrorV2::Refusal(refusal) => match refusal {
            RetentionRadixRefusalV2::NotRadixDomain { .. } => {
                AdmissionClass::Radix(RadixRefusal::NotRadixDomain)
            }
            RetentionRadixRefusalV2::Body(error) => {
                AdmissionClass::Radix(RadixRefusal::Body(renamed_version(error, DAG_NODE_NAME)))
            }
            RetentionRadixRefusalV2::EndpointKind { endpoint } => {
                AdmissionClass::Radix(RadixRefusal::EndpointKind(endpoint))
            }
            RetentionRadixRefusalV2::EndpointDepth { remaining, prefix } => {
                AdmissionClass::Radix(RadixRefusal::EndpointDepth { remaining, prefix })
            }
            RetentionRadixRefusalV2::EmptyBelowRoot => {
                AdmissionClass::Radix(RadixRefusal::EmptyBelowRoot)
            }
            RetentionRadixRefusalV2::LeafKey(_) => AdmissionClass::Radix(RadixRefusal::LeafKey),
            RetentionRadixRefusalV2::LeafClaim(error) => {
                AdmissionClass::Radix(RadixRefusal::LeafClaim(error))
            }
            RetentionRadixRefusalV2::CountMismatch { declared, counted } => {
                AdmissionClass::Radix(RadixRefusal::CountMismatch {
                    declared,
                    unfolded: counted,
                })
            }
            RetentionRadixRefusalV2::CountOverflow => {
                AdmissionClass::Radix(RadixRefusal::CountOverflow)
            }
            refusal @ RetentionRadixRefusalV2::EntryAllocation { .. } => {
                AdmissionClass::Unmapped(format!("{refusal:?}"))
            }
        },
    }
}

pub(super) fn tracing_store(objects: &ObjectSet) -> TracingStore {
    let store = TracingStore::default();
    for (digest, bytes) in objects {
        store
            .inner
            .put_prehashed_for_test(*digest, Arc::from(bytes.as_slice()))
            .expect("each model digest enters the store once");
    }
    store
}

fn production_admission(
    objects: &ObjectSet,
    anchor: RetentionAnchorV2,
) -> Result<AdmittedImage, AdmissionClass> {
    let store = tracing_store(objects);
    let admitted = admit_retention_history_v2(&store, anchor).map_err(production_class)?;
    Ok((
        admitted.entries().to_vec(),
        admitted.metadata().objects().to_vec(),
        admitted.metadata().canonical_bytes(),
    ))
}

/// The reference pipeline: unfold the radix image, then apply the V1 history
/// rules to the decoded entries.
fn reference_admission(
    objects: &ObjectSet,
    anchor: RetentionAnchorV2,
) -> Result<Vec<RetainedStateV1>, AdmissionClass> {
    let leaves = unfold_radix(objects, anchor.historical_root(), anchor.historical_count())
        .map_err(AdmissionClass::Radix)?;
    let entries = leaves
        .iter()
        .map(|leaf| {
            retained(model_decode_key(&leaf.key).expect("unfolded keys decode"), leaf.claim)
        })
        .collect::<Vec<_>>();
    RetentionManifestV1::new(anchor.current(), entries.clone()).map_err(AdmissionClass::History)?;
    Ok(entries)
}

/// Phase one of the reader as a model: every node is checked at the key width
/// still unread where it is reached, and counts are memoized by (digest,
/// remaining width). A digest reached through another reference is refused
/// before the memo is consulted, as the reader refuses a cached digest under
/// another length. With `keyed_by_depth` false the memo is keyed by digest
/// alone, which is the mutant that accepts a shared leaf at the wrong depth.
fn shape_strict_count(
    objects: &ObjectSet,
    root: ObjectRefV1,
    keyed_by_depth: bool,
) -> Result<u64, RadixRefusal> {
    let memo_key = |digest: ObjectDigest, remaining: usize| match keyed_by_depth {
        true => (digest, remaining),
        false => (digest, 0),
    };
    let mut memo = BTreeMap::<(ObjectDigest, usize), u64>::new();
    let mut references = BTreeMap::<ObjectDigest, ObjectRefV1>::new();
    let mut pending = vec![(root, KEY_WIDTH, false)];
    while let Some((reference, remaining, expanded)) = pending.pop() {
        if *references.entry(reference.digest()).or_insert(reference) != reference {
            return Err(RadixRefusal::ReferenceMismatch);
        }
        let key = memo_key(reference.digest(), remaining);
        if !expanded && memo.contains_key(&key) {
            continue;
        }
        let node = load_radix_node(objects, reference)?;
        let prefix = node.prefix.len();
        match (node.endpoint, node.children.is_empty(), prefix.cmp(&remaining), expanded) {
            (EndpointValue::Multiplicity(_), true, Ordering::Equal, _) => {
                memo.insert(key, 1);
            }
            (EndpointValue::Absent, true, _, _) => return Err(RadixRefusal::EmptyBelowRoot),
            (EndpointValue::Absent, false, Ordering::Less, false) => {
                pending.push((reference, remaining, true));
                let below = remaining - prefix - 1;
                pending.extend(
                    node.children
                        .iter()
                        .rev()
                        .map(|child| (child.target, below, false)),
                );
            }
            (EndpointValue::Absent, false, Ordering::Less, true) => {
                let below = remaining - prefix - 1;
                let mut total = 0_u64;
                for child in &node.children {
                    let count = memo
                        .get(&memo_key(child.target.digest(), below))
                        .copied()
                        .expect("children are counted before their parent");
                    total = total
                        .checked_add(count)
                        .ok_or(RadixRefusal::CountOverflow)?;
                }
                memo.insert(key, total);
            }
            (EndpointValue::Absent | EndpointValue::Multiplicity(_), _, _, _) => {
                return Err(RadixRefusal::EndpointDepth { remaining, prefix });
            }
            (endpoint, _, _, _) => return Err(RadixRefusal::EndpointKind(endpoint.name())),
        }
    }
    Ok(memo
        .get(&memo_key(root.digest(), KEY_WIDTH))
        .copied()
        .expect("the root is counted"))
}

pub(super) fn model_count_by_depth(
    objects: &ObjectSet,
    root: ObjectRefV1,
) -> Result<u64, RadixRefusal> {
    shape_strict_count(objects, root, true)
}

fn digest_only_memoized_count(objects: &ObjectSet, root: ObjectRefV1) -> Result<u64, RadixRefusal> {
    shape_strict_count(objects, root, false)
}

/// The enumeration-order mutant: children visited in reverse edge order.
fn reverse_edge_enumeration(objects: &ObjectSet, root: ObjectRefV1) -> Vec<RetainedStateV1> {
    let mut entries = Vec::new();
    let mut pending = vec![(root, Vec::with_capacity(KEY_WIDTH))];
    while let Some((reference, mut key)) = pending.pop() {
        let node = load_radix_node(objects, reference).expect("valid images load");
        key.extend_from_slice(&node.prefix);
        match node.endpoint {
            EndpointValue::Multiplicity(claim) => entries
                .push(retained(model_decode_key(&key).expect("valid keys decode"), claim.get())),
            _ => {
                for child in &node.children {
                    let mut child_key = Vec::with_capacity(KEY_WIDTH);
                    child_key.extend_from_slice(&key);
                    child_key.push(child.edge);
                    pending.push((child.target, child_key));
                }
            }
        }
    }
    entries
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum RadixDefect {
    Drop,
    LengthShift { longer: bool },
    ForgeDigest,
    Retag { anchor_domain: bool },
    Endpoint { kind: usize },
    ShortLeaf,
    MultiplicityWithChildren,
    EmptyChild,
    ClaimBelowLength,
    KeyLengthBelow32,
    WrongCount { more: bool },
    CurrentInHistory,
    EnvelopeVersion2,
    UnaryNode,
    UnorderedEdges,
}

const RADIX_DEFECTS: [RadixDefect; 20] = [
    RadixDefect::Drop,
    RadixDefect::LengthShift { longer: false },
    RadixDefect::LengthShift { longer: true },
    RadixDefect::ForgeDigest,
    RadixDefect::Retag {
        anchor_domain: false,
    },
    RadixDefect::Retag {
        anchor_domain: true,
    },
    RadixDefect::Endpoint { kind: 0 },
    RadixDefect::Endpoint { kind: 1 },
    RadixDefect::Endpoint { kind: 2 },
    RadixDefect::ShortLeaf,
    RadixDefect::MultiplicityWithChildren,
    RadixDefect::EmptyChild,
    RadixDefect::ClaimBelowLength,
    RadixDefect::KeyLengthBelow32,
    RadixDefect::WrongCount { more: false },
    RadixDefect::WrongCount { more: true },
    RadixDefect::CurrentInHistory,
    RadixDefect::EnvelopeVersion2,
    RadixDefect::UnaryNode,
    RadixDefect::UnorderedEdges,
];

fn radix_defect_strategy() -> impl Strategy<Value = RadixDefect> {
    prop::sample::select(RADIX_DEFECTS.to_vec())
}

/// One occurrence of a radix node in the unfolded tree: the child indices
/// from the root, the node, and the key bytes read before it.
#[derive(Clone, Debug)]
pub(super) struct RadixOccurrence {
    pub(super) path: Vec<usize>,
    pub(super) reference: ObjectRefV1,
    pub(super) node: RadixNodeImage,
    pub(super) key_prefix: Vec<u8>,
}

impl RadixOccurrence {
    pub(super) fn is_leaf(&self) -> bool {
        matches!(self.node.endpoint, EndpointValue::Multiplicity(_)) &&
            self.node.children.is_empty()
    }

    pub(super) fn is_inner(&self) -> bool {
        matches!(self.node.endpoint, EndpointValue::Absent) && !self.node.children.is_empty()
    }

    pub(super) fn leaf_key(&self) -> Vec<u8> {
        [&self.key_prefix[..], &self.node.prefix[..]].concat()
    }
}

pub(super) fn radix_occurrences(objects: &ObjectSet, root: ObjectRefV1) -> Vec<RadixOccurrence> {
    let mut occurrences = Vec::new();
    let mut pending = vec![(root, Vec::new(), Vec::with_capacity(KEY_WIDTH))];
    while let Some((reference, path, key_prefix)) = pending.pop() {
        let node = load_radix_node(objects, reference).expect("occurrences come from valid images");
        for (index, child) in node.children.iter().enumerate().rev() {
            let mut child_path = Vec::with_capacity(path.len() + 1);
            child_path.extend_from_slice(&path);
            child_path.push(index);
            let mut child_key = Vec::with_capacity(KEY_WIDTH);
            child_key.extend_from_slice(&key_prefix);
            child_key.extend_from_slice(&node.prefix);
            child_key.push(child.edge);
            pending.push((child.target, child_path, child_key));
        }
        occurrences.push(RadixOccurrence {
            path,
            reference,
            node,
            key_prefix,
        });
    }
    occurrences
}

/// Replace the reference reached along `path` and re-encode every ancestor
/// on that path; other occurrences of shared nodes keep the originals.
pub(super) fn rewrite_radix_path(
    objects: &mut ObjectSet,
    root: ObjectRefV1,
    path: &[usize],
    replacement: ObjectRefV1,
) -> ObjectRefV1 {
    let mut ancestors = Vec::with_capacity(path.len());
    let mut reference = root;
    for index in path {
        let node = load_radix_node(objects, reference).expect("ancestors are valid radix nodes");
        reference = node.children[*index].target;
        ancestors.push((node, *index));
    }
    let mut target = replacement;
    while let Some((mut node, index)) = ancestors.pop() {
        node.children[index].target = target;
        target = insert_object(objects, radix_node(&node.prefix, node.endpoint, &node.children));
    }
    target
}

pub(super) fn raw_radix_node(prefix: &[u8], endpoint: &[u8], children: &[DagChild]) -> Vec<u8> {
    let branch = match children.len() {
        0 => 0_u8,
        1..=4 => 1,
        _ => 2,
    };
    let mut body = Vec::with_capacity(8 + prefix.len() + endpoint.len() + 9 + 41 * children.len());
    body.extend_from_slice(&(prefix.len() as u64).to_le_bytes());
    body.extend_from_slice(prefix);
    body.extend_from_slice(endpoint);
    body.push(branch);
    body.extend_from_slice(&(children.len() as u64).to_le_bytes());
    for child in children {
        body.push(child.edge);
        body.extend_from_slice(&reference_bytes(child.target));
    }
    envelope(MODEL_RADIX_DOMAIN, &body)
}

struct DefectImage {
    objects: ObjectSet,
    anchor: RetentionAnchorV2,
}

/// Apply one defect at one occurrence, or return `None` when the defect does
/// not apply there.
fn apply_radix_defect(
    objects: &ObjectSet,
    anchor: RetentionAnchorV2,
    defect: RadixDefect,
    occurrence: &RadixOccurrence,
    position: usize,
) -> Option<DefectImage> {
    let mut objects = objects.clone();
    let count = anchor.historical_count();
    let root = anchor.historical_root();
    let node = &occurrence.node;
    let is_root = occurrence.path.is_empty();
    let digest = occurrence.reference.digest();
    let stored = objects
        .get(&digest)
        .cloned()
        .expect("occurrences are stored");
    let leaf_length = match occurrence.is_leaf() {
        true => {
            let key = occurrence.leaf_key();
            Some(u64::from_be_bytes(key[40..48].try_into().expect("leaf keys are 48 bytes")))
        }
        false => None,
    };
    let replace = |objects: &mut ObjectSet, bytes: Vec<u8>| {
        let replacement = insert_object(objects, bytes);
        rewrite_radix_path(objects, root, &occurrence.path, replacement)
    };
    let rerooted = match (defect, leaf_length) {
        (RadixDefect::Drop, _) => {
            objects.remove(&digest);
            root
        }
        (RadixDefect::LengthShift { longer }, _) if is_root => {
            let mut bytes = stored;
            match longer {
                true => bytes.push(0),
                false => {
                    bytes.pop();
                }
            }
            objects.insert(digest, bytes);
            root
        }
        (RadixDefect::LengthShift { longer }, _) => {
            let length = occurrence.reference.encoded_len();
            let shifted = match longer {
                true => length + 1,
                false => length - 1,
            };
            let shifted = ObjectRefV1::new(digest, shifted).expect("radix nodes exceed 33 bytes");
            rewrite_radix_path(&mut objects, root, &occurrence.path, shifted)
        }
        (RadixDefect::ForgeDigest, _) => {
            let mut bytes = stored;
            let index = position % bytes.len();
            bytes[index] ^= 1;
            objects.insert(digest, bytes);
            root
        }
        (RadixDefect::Retag { anchor_domain }, _) if count > 0 => {
            let domain = match anchor_domain {
                true => MODEL_ANCHOR_DOMAIN,
                false => DAG_NODE_DOMAIN,
            };
            replace(&mut objects, with_domain(&stored, domain))
        }
        (RadixDefect::Endpoint { kind }, _) if count > 0 => {
            let endpoint = [
                EndpointValue::ValueFree,
                EndpointValue::Unit,
                EndpointValue::Content(reference([0x22; 32], 64)),
            ][kind];
            replace(&mut objects, radix_node(&node.prefix, endpoint, &node.children))
        }
        (RadixDefect::ShortLeaf, Some(_)) => {
            let prefix = match node.prefix.split_first() {
                Some((_, rest)) => rest.to_vec(),
                None => vec![0],
            };
            replace(&mut objects, radix_node(&prefix, node.endpoint, &[]))
        }
        (RadixDefect::MultiplicityWithChildren, _) if occurrence.is_inner() => replace(
            &mut objects,
            radix_node(&node.prefix, EndpointValue::Multiplicity(nonzero(50)), &node.children),
        ),
        (RadixDefect::EmptyChild, _) if !is_root => {
            objects
                .insert(empty_retention_radix_ref().digest(), EMPTY_RETENTION_RADIX_NODE.to_vec());
            rewrite_radix_path(&mut objects, root, &occurrence.path, empty_retention_radix_ref())
        }
        (RadixDefect::ClaimBelowLength, Some(length)) => replace(
            &mut objects,
            radix_node(&node.prefix, EndpointValue::Multiplicity(nonzero(length - 1)), &[]),
        ),
        (RadixDefect::KeyLengthBelow32, Some(length))
            if length < 256 && !node.prefix.is_empty() =>
        {
            let mut prefix = node.prefix.clone();
            let last = prefix.len() - 1;
            prefix[last] = 31;
            replace(&mut objects, radix_node(&prefix, node.endpoint, &[]))
        }
        (RadixDefect::WrongCount { more }, _) if is_root && count > 0 && (more || count > 1) => {
            let declared = match more {
                true => count + 1,
                false => count - 1,
            };
            let anchor = RetentionAnchorV2::new(anchor.current(), declared, root)
                .expect("a nonzero count keeps the radix root");
            return Some(DefectImage { objects, anchor });
        }
        (RadixDefect::CurrentInHistory, Some(_)) => {
            let EndpointValue::Multiplicity(claim) = node.endpoint else {
                unreachable!("a leaf carries a multiplicity");
            };
            let current = model_decode_key(&occurrence.leaf_key()).expect("valid leaf keys decode");
            let anchor = RetentionAnchorV2::new(retained(current, claim.get()), count, root)
                .expect("the history is unchanged");
            return Some(DefectImage { objects, anchor });
        }
        (RadixDefect::EnvelopeVersion2, _) if count > 0 => {
            replace(&mut objects, with_version(&stored, 2))
        }
        (RadixDefect::UnaryNode, _) if occurrence.is_inner() => {
            replace(&mut objects, raw_radix_node(&node.prefix, &[0], &node.children[..1]))
        }
        (RadixDefect::UnorderedEdges, _)
            if occurrence.is_inner() && (2..=16).contains(&node.children.len()) =>
        {
            let mut children = node.children.clone();
            children.swap(0, 1);
            replace(&mut objects, raw_radix_node(&node.prefix, &[0], &children))
        }
        _ => return None,
    };
    let anchor = RetentionAnchorV2::new(anchor.current(), count, rerooted)
        .expect("a defect keeps the declared count's root rule");
    Some(DefectImage { objects, anchor })
}

/// One single-defect case: the reference must refuse it (non-vacuity), and
/// production must refuse it with the corresponding class.
fn check_radix_defect(
    objects: &ObjectSet,
    anchor: RetentionAnchorV2,
    defect: RadixDefect,
    occurrence: &RadixOccurrence,
    position: usize,
) -> Result<bool, TestCaseError> {
    let Some(image) = apply_radix_defect(objects, anchor, defect, occurrence, position) else {
        return Ok(false);
    };
    let expected = reference_admission(&image.objects, image.anchor);
    prop_assert!(expected.is_err(), "the reference refuses {:?} at {:?}", defect, occurrence.path);
    prop_assert_eq!(
        production_admission(&image.objects, image.anchor).err(),
        expected.err(),
        "defect {:?} at {:?}",
        defect,
        occurrence.path
    );
    let root = image.anchor.historical_root();
    if let Ok(count) = model_count_by_depth(&image.objects, root) {
        match unfold_radix(&image.objects, root, count) {
            Ok(leaves) => prop_assert_eq!(leaves.len() as u64, count),
            Err(RadixRefusal::LeafKey | RadixRefusal::LeafClaim(_)) => {}
            Err(refusal) => {
                return Err(fail(format!(
                    "a shape-strict count of {count} unfolds to {refusal:?} after {defect:?}"
                )));
            }
        }
    }
    Ok(true)
}

fn check_valid_admission(history: &History) -> Result<(), TestCaseError> {
    let image = build_radix(history);
    let anchor = history_anchor(history, &image);
    let expected = reference_admission(&image.objects, anchor)
        .map_err(|class| fail(format!("the reference refuses a valid history: {class:?}")))?;
    let (entries, metadata, metadata_bytes) = production_admission(&image.objects, anchor)
        .map_err(|class| fail(format!("production refuses a valid history: {class:?}")))?;
    prop_assert_eq!(&entries, &expected);
    let closure = metadata_closure(anchor, &image)
        .map_err(|refusal| fail(format!("{refusal:?}")))?
        .into_values()
        .collect::<Vec<_>>();
    prop_assert_eq!(&metadata, &closure);
    prop_assert_eq!(
        metadata_bytes,
        closure
            .iter()
            .map(|reference| reference.encoded_len())
            .sum::<u64>()
    );
    if !history.is_empty() {
        prop_assert_eq!(model_count_by_depth(&image.objects, image.root), Ok(history.len() as u64));
    }
    Ok(())
}

fn defect_histories() -> Vec<History> {
    let aliases = [
        state(1, [0x11; 32], 50),
        state(2, [0x11; 32], 50),
        state(2, [0x11; 32], 51),
        state(3, [0x12; 32], 32),
    ];
    let product = (0..3_u64)
        .flat_map(|generation| [0x21_u8, 0x22, 0x23].map(|seed| state(generation, [seed; 32], 50)));
    let wide = (0..6_u64).map(|generation| state(generation, [0x31; 32], 50));
    vec![
        History::new(),
        History::from([(model_key(state(4, [0x41; 32], 50)), 50)]),
        aliases
            .into_iter()
            .map(|state| (model_key(state), state.manifest().encoded_len() + 1))
            .collect(),
        product.map(|state| (model_key(state), 50)).collect(),
        wide.map(|state| (model_key(state), 64)).collect(),
    ]
}

#[test]
fn production_radix_admission_refines_the_reference_decoder() {
    let current = model_current();
    let leaf47 = [&[0_u8; 7][..], &[0x47; 32][..], &50_u64.to_be_bytes()[..]].concat();
    let mut objects = ObjectSet::new();
    let empty = insert_object(&mut objects, EMPTY_RETENTION_RADIX_NODE.to_vec());
    let leaf = insert_object(
        &mut objects,
        radix_node(&leaf47, EndpointValue::Multiplicity(nonzero(50)), &[]),
    );
    let root = insert_object(
        &mut objects,
        radix_node(&[], EndpointValue::Absent, &[
            DagChild {
                edge: 0,
                target: empty,
            },
            DagChild {
                edge: 1,
                target: leaf,
            },
        ]),
    );
    let anchor = RetentionAnchorV2::new(current, 1, root).expect("one entry is declared");
    let empty_below_root = AdmissionClass::Radix(RadixRefusal::EmptyBelowRoot);
    assert_eq!(production_admission(&objects, anchor).err(), Some(empty_below_root.clone()));
    assert_eq!(reference_admission(&objects, anchor).err(), Some(empty_below_root));
    assert_eq!(model_count_by_depth(&objects, root), Err(RadixRefusal::EmptyBelowRoot));
    assert_eq!(memoized_count(&objects, root), Ok(1));

    let history = defect_histories().swap_remove(3);
    let image = build_radix(&history);
    let mutant = reverse_edge_enumeration(&image.objects, image.root);
    assert_eq!(mutant.len(), history.len());
    assert_eq!(
        RetentionManifestV1::new(current, mutant).err(),
        Some(CanonicalFormatError::RetainedStatesNotStrict)
    );
    let (entries, _, _) = production_admission(&image.objects, history_anchor(&history, &image))
        .expect("the product history is admitted");
    assert!(RetentionManifestV1::new(current, entries).is_ok());

    let contained = state(4, [0x41; 32], 50);
    let history = History::from([(model_key(contained), 50)]);
    let image = build_radix(&history);
    let anchor = RetentionAnchorV2::new(retained(contained, 50), 1, image.root)
        .expect("the anchor appoints its leaf");
    let contains_current = AdmissionClass::History(CanonicalFormatError::RetentionContainsCurrent);
    assert_eq!(production_admission(&image.objects, anchor).err(), Some(contains_current.clone()));
    assert_eq!(reference_admission(&image.objects, anchor).err(), Some(contains_current));

    let mut applied = BTreeSet::new();
    for history in defect_histories() {
        check_valid_admission(&history).expect("fixed histories are admitted");
        let image = build_radix(&history);
        let anchor = history_anchor(&history, &image);
        for occurrence in radix_occurrences(&image.objects, image.root) {
            for (position, defect) in RADIX_DEFECTS.into_iter().enumerate() {
                if check_radix_defect(&image.objects, anchor, defect, &occurrence, position * 7)
                    .expect("single defects refuse with corresponding classes")
                {
                    applied.insert(defect);
                }
            }
        }
    }
    assert_eq!(applied, RADIX_DEFECTS.into_iter().collect::<BTreeSet<_>>());

    let random_applied = RefCell::new(BTreeSet::new());
    runner(256)
        .run(
            &(
                prop_oneof![
                    history_strategy(24),
                    product_history().prop_map(|(history, _, _)| history)
                ],
                radix_defect_strategy(),
                any::<prop::sample::Index>(),
                any::<usize>(),
            ),
            |(history, defect, pick, position)| {
                check_valid_admission(&history)?;
                let image = build_radix(&history);
                let anchor = history_anchor(&history, &image);
                let occurrences = radix_occurrences(&image.objects, image.root);
                let occurrence = &occurrences[pick.index(occurrences.len())];
                if check_radix_defect(&image.objects, anchor, defect, occurrence, position)? {
                    random_applied.borrow_mut().insert(defect);
                }
                Ok(())
            },
        )
        .expect("production radix admission refines the reference decoder");
    assert!(random_applied.borrow().len() >= RADIX_DEFECTS.len() / 2);
}

/// A shared DAG of `levels` inner nodes, each pointing every edge in
/// `0..arity` at the level below, over one leaf with the given prefix.
fn shared_radix_dag(arity: usize, levels: usize, leaf_prefix: &[u8]) -> (ObjectSet, ObjectRefV1) {
    let mut objects = ObjectSet::new();
    let mut target = insert_object(
        &mut objects,
        radix_node(leaf_prefix, EndpointValue::Multiplicity(nonzero(u64::MAX)), &[]),
    );
    for _ in 0..levels {
        let children = (0..arity)
            .map(|edge| DagChild {
                edge: u8::try_from(edge).expect("radix edges are bytes"),
                target,
            })
            .collect::<Vec<_>>();
        target = insert_object(&mut objects, radix_node(&[], EndpointValue::Absent, &children));
    }
    (objects, target)
}

/// A comb of depth 49: inner node `d` (empty prefix) branches to leaf `d` on
/// edge 0 and to inner node `d + 1` on edge 1; the last inner node branches to
/// two leaves. Leaves above depth 40 end in a 50-byte manifest length.
fn radix_comb() -> (ObjectSet, ObjectRefV1) {
    let mut objects = ObjectSet::new();
    let mut spine = insert_object(
        &mut objects,
        radix_node(&[], EndpointValue::Multiplicity(nonzero(u64::MAX - 1)), &[]),
    );
    for depth in (0..KEY_WIDTH).rev() {
        let mut prefix = vec![0xff_u8; KEY_WIDTH - 1 - depth];
        if depth < 40 {
            let end = prefix.len();
            prefix[end - 8..].copy_from_slice(&50_u64.to_be_bytes());
        }
        let leaf = insert_object(
            &mut objects,
            radix_node(&prefix, EndpointValue::Multiplicity(nonzero(u64::MAX)), &[]),
        );
        spine = insert_object(
            &mut objects,
            radix_node(&[], EndpointValue::Absent, &[
                DagChild {
                    edge: 0,
                    target: leaf,
                },
                DagChild {
                    edge: 1,
                    target: spine,
                },
            ]),
        );
    }
    (objects, spine)
}

fn assert_read_once(store: &TracingStore, objects: &ObjectSet, reads: u64) {
    assert_eq!(store.total_reads(), reads);
    for digest in objects.keys() {
        assert_eq!(store.read_count(*digest), 1);
    }
}

#[test]
fn radix_memo_is_keyed_by_depth_and_shared_graphs_cannot_force_enumeration() {
    let current = model_current();
    let suffix = |zeros: usize| {
        [&vec![0_u8; zeros][..], &[0x19; 32][..], &50_u64.to_be_bytes()[..]].concat()
    };
    let mut objects = ObjectSet::new();
    let leaf = |objects: &mut ObjectSet, prefix: &[u8]| {
        insert_object(objects, radix_node(prefix, EndpointValue::Multiplicity(nonzero(50)), &[]))
    };
    let node2 = leaf(&mut objects, &suffix(7));
    let node4 = leaf(&mut objects, &suffix(6));
    let node3 = insert_object(
        &mut objects,
        radix_node(&[], EndpointValue::Absent, &[
            DagChild {
                edge: 0,
                target: node2,
            },
            DagChild {
                edge: 1,
                target: node4,
            },
        ]),
    );
    let root = insert_object(
        &mut objects,
        radix_node(&[], EndpointValue::Absent, &[
            DagChild {
                edge: 0,
                target: node2,
            },
            DagChild {
                edge: 1,
                target: node3,
            },
        ]),
    );
    let misplaced = RadixRefusal::EndpointDepth {
        remaining: 46,
        prefix: 47,
    };
    let store = tracing_store(&objects);
    let anchor = RetentionAnchorV2::new(current, 3, root).expect("three entries are declared");
    assert_eq!(
        admit_retention_history_v2(&store, anchor)
            .map_err(production_class)
            .err(),
        Some(AdmissionClass::Radix(misplaced.clone()))
    );
    assert_eq!([root, node2, node3, node4].map(|node| store.read_count(node.digest())), [
        1, 1, 1, 0
    ]);
    assert_eq!(unfold_radix(&objects, root, 3).err(), Some(misplaced.clone()));
    assert_eq!(model_count_by_depth(&objects, root), Err(misplaced));
    assert_eq!(digest_only_memoized_count(&objects, root), Ok(3));

    let (objects, root) = shared_radix_dag(2, 48, &[]);
    assert_eq!(objects.len(), 49);
    let declared = (1_u64 << 48) - 1;
    let store = tracing_store(&objects);
    let anchor = RetentionAnchorV2::new(current, declared, root).expect("the count is nonzero");
    assert_eq!(
        admit_retention_history_v2(&store, anchor)
            .map_err(production_class)
            .err(),
        Some(AdmissionClass::Radix(RadixRefusal::CountMismatch {
            declared,
            unfolded: 1 << 48,
        }))
    );
    assert_read_once(&store, &objects, 49);
    assert_eq!(unfold_radix(&objects, root, declared).err(), Some(RadixRefusal::LeafKey));
    assert_eq!(model_count_by_depth(&objects, root), Ok(1 << 48));

    let (objects, root) = shared_radix_dag(4, 32, &[0x44; 16]);
    let store = tracing_store(&objects);
    let anchor = RetentionAnchorV2::new(current, u64::MAX, root).expect("the count is nonzero");
    assert_eq!(
        admit_retention_history_v2(&store, anchor)
            .map_err(production_class)
            .err(),
        Some(AdmissionClass::Radix(RadixRefusal::CountOverflow))
    );
    assert_read_once(&store, &objects, 33);
    assert_eq!(model_count_by_depth(&objects, root), Err(RadixRefusal::CountOverflow));

    let (objects, root) = shared_radix_dag(256, 7, &[0x56; 41]);
    let store = tracing_store(&objects);
    let anchor = RetentionAnchorV2::new(current, 1 << 56, root).expect("the count is nonzero");
    assert!(matches!(
        admit_retention_history_v2(&store, anchor),
        Err(RetentionRadixErrorV2::Refusal(RetentionRadixRefusalV2::EntryAllocation { entries }))
            if entries == 1 << 56
    ));
    assert_read_once(&store, &objects, 8);
    assert_eq!(model_count_by_depth(&objects, root), Ok(1 << 56));

    let (objects, root) = radix_comb();
    assert_eq!(objects.len(), 97);
    let anchor = RetentionAnchorV2::new(current, 49, root).expect("the comb holds 49 entries");
    let expected = reference_admission(&objects, anchor).expect("the comb unfolds");
    let (entries, metadata, _) = std::thread::Builder::new()
        .name("retention-v2-comb".into())
        .stack_size(256 * 1024)
        .spawn(move || production_admission(&objects, anchor))
        .expect("the small-stack thread starts")
        .join()
        .expect("the depth-49 comb fits a 256 KiB stack")
        .expect("the depth-49 comb is admitted");
    assert_eq!(entries.len(), 49);
    assert_eq!(entries, expected);
    assert_eq!(metadata.len(), 98);

    runner(256)
        .run(
            &prop_oneof![
                history_strategy(24),
                product_history().prop_map(|(history, _, _)| history)
            ],
            |history| {
                let image = build_radix(&history);
                let anchor = history_anchor(&history, &image);
                let store = tracing_store(&image.objects);
                let admitted = admit_retention_history_v2(&store, anchor)
                    .map_err(|error| fail(format!("{:?}", production_class(error))))?;
                prop_assert_eq!(admitted.metadata().object_count(), image.objects.len() as u64 + 1);
                prop_assert_eq!(store.total_reads(), admitted.metadata().object_count() - 1);
                for digest in image.objects.keys() {
                    prop_assert_eq!(store.read_count(*digest), 1);
                }
                if !history.is_empty() {
                    prop_assert_eq!(
                        digest_only_memoized_count(&image.objects, image.root),
                        model_count_by_depth(&image.objects, image.root)
                    );
                }
                Ok(())
            },
        )
        .expect("valid radix histories are read once");
}

type GraphNodeSpec = (usize, u8, u8, u64, Vec<(u8, usize)>);

fn arbitrary_graph_strategy() -> impl Strategy<Value = Vec<GraphNodeSpec>> {
    prop::collection::vec(
        (
            prop_oneof![
                Just(0_usize),
                Just(6_usize),
                Just(7_usize),
                Just(40_usize),
                Just(46_usize),
                Just(47_usize),
                Just(48_usize),
                0_usize..=49
            ],
            any::<u8>(),
            0_u8..6,
            prop_oneof![Just(31_u64), Just(50_u64), Just(u64::MAX), any::<u64>()],
            prop::collection::vec((any::<u8>(), any::<usize>()), 0..=5),
        ),
        1..=10,
    )
}

fn endpoint_bytes(choice: u8, claim: u64, seed: u8) -> Vec<u8> {
    match choice {
        0 | 1 => vec![0],
        2 => [&[3_u8][..], &claim.to_le_bytes()[..]].concat(),
        3 => vec![1],
        4 => vec![2],
        _ => [&[4_u8][..], &reference_bytes(reference([seed; 32], 64))[..]].concat(),
    }
}

/// An arbitrary graph of radix-domain objects: honest nodes may only point at
/// earlier nodes or at a forged object stored under a digest that its own
/// children reference.
fn arbitrary_graph(specs: &[GraphNodeSpec], forged: [u8; 32]) -> (ObjectSet, Vec<ObjectRefV1>) {
    let forged_len = (32 + 8 + 1 + 1 + 8 + 2 * 41) as u64;
    let forged_reference = reference(forged, forged_len);
    let forged_bytes = raw_radix_node(&[], &[0], &[
        DagChild {
            edge: 0,
            target: forged_reference,
        },
        DagChild {
            edge: 1,
            target: forged_reference,
        },
    ]);
    assert_eq!(forged_bytes.len() as u64, forged_len);
    let mut objects = ObjectSet::new();
    let mut references = Vec::with_capacity(specs.len() + 1);
    references.push(forged_reference);
    for (prefix_len, prefix_byte, endpoint, claim, children) in specs {
        let children = children
            .iter()
            .map(|(edge, pick)| DagChild {
                edge: *edge,
                target: references[pick % references.len()],
            })
            .collect::<Vec<_>>();
        let bytes = raw_radix_node(
            &vec![*prefix_byte; *prefix_len],
            &endpoint_bytes(*endpoint, *claim, *prefix_byte),
            &children,
        );
        references.push(insert_object(&mut objects, bytes));
    }
    objects
        .entry(forged_reference.digest())
        .or_insert(forged_bytes);
    (objects, references)
}

fn check_arbitrary_graph(
    objects: &ObjectSet,
    root: ObjectRefV1,
    extra_count: u64,
) -> Result<(), TestCaseError> {
    let model_count = model_count_by_depth(objects, root);
    let _ = memoized_count(objects, root);
    let mut declared_counts = vec![0, 1, 2, extra_count];
    if let Ok(count) = model_count {
        if count <= ENUMERABLE_ENTRIES {
            declared_counts.push(count);
        }
    }
    for declared in declared_counts {
        let Ok(anchor) = RetentionAnchorV2::new(model_current(), declared, root) else {
            continue;
        };
        let store = tracing_store(objects);
        let result = admit_retention_history_v2(&store, anchor);
        for digest in objects.keys() {
            prop_assert!(store.read_count(*digest) <= 1);
        }
        prop_assert!(store.total_reads() <= objects.len() as u64);
        match result {
            Ok(admitted) => {
                prop_assert!(declared <= ENUMERABLE_ENTRIES);
                let expected = reference_admission(objects, anchor)
                    .map_err(|class| fail(format!("{class:?}")))?;
                prop_assert_eq!(admitted.entries(), &expected[..]);
            }
            Err(RetentionRadixErrorV2::Refusal(RetentionRadixRefusalV2::CountMismatch {
                counted,
                ..
            })) => prop_assert_eq!(model_count.clone(), Ok(counted)),
            Err(_) => {}
        }
    }
    Ok(())
}

#[test]
fn dual_retention_readers_are_total_on_arbitrary_bytes_and_graphs() {
    runner(64)
        .run(
            &(
                prop::collection::vec(any::<u8>(), 0..=256),
                (history_strategy(6), retained_union(), any::<u8>()),
                (any::<prop::sample::Index>(), 1_u8..=255, any::<prop::sample::Index>()),
                prop::collection::vec(0_usize..64, 0..32),
                (arbitrary_graph_strategy(), any::<[u8; 32]>(), any::<prop::sample::Index>()),
                0_u64..=64,
            ),
            |(arbitrary, (history, union, seed), (position, flip, cut), widths, graph, extra)| {
                let objects = reader_objects(&history, &union, seed);
                let mut candidates = Vec::with_capacity(3 * objects.len() + 1);
                candidates.push(arbitrary);
                for valid in &objects {
                    let mut flipped = valid.clone();
                    flipped[position.index(valid.len())] ^= flip;
                    candidates.push(flipped);
                    candidates.push(valid[..cut.index(valid.len())].to_vec());
                    let mut extended = valid.clone();
                    extended.push(flip);
                    candidates.push(extended);
                }
                for bytes in &candidates {
                    let dual = dual_identity(bytes);
                    prop_assert_eq!(
                        CanonicalObjectKind::from_domain_prefix_for(bytes, RetentionReader::Dual),
                        dual_domain_kind(bytes)
                    );
                    prop_assert_eq!(
                        stream_identity(bytes, RetentionReader::Dual, &widths),
                        dual.clone()
                    );
                    let declared = declared_reference(bytes);
                    prop_assert_eq!(
                        validate_object_for(declared, bytes, RetentionReader::Dual),
                        expected_validation(&dual, declared)
                    );
                    if let Ok(view) = GenerationCheckpointDescriptorViewV1::parse_for(
                        bytes,
                        RetentionReader::Dual,
                    ) {
                        prop_assert_eq!(
                            &encoded(&GenerationCheckpointDescriptorV1::from_view(view)),
                            bytes
                        );
                    }
                    if let Ok(anchor) = RetentionAnchorV2::parse(bytes) {
                        prop_assert_eq!(&encoded(&anchor), bytes);
                    }
                    if let Ok(AppointedRetention::V2(anchor)) =
                        AppointedRetention::parse(bytes, RetentionReader::Dual)
                    {
                        prop_assert_eq!(&encoded(&anchor), bytes);
                    }
                    let _ = CanonicalDagNodeView::parse_in_domain(
                        bytes,
                        RETENTION_RADIX_DOMAIN,
                        RADIX_NODE_NAME,
                    );
                }

                let (specs, forged, root) = graph;
                let (objects, references) = arbitrary_graph(&specs, forged);
                check_arbitrary_graph(&objects, references[root.index(references.len())], extra)?;
                check_arbitrary_graph(&objects, references[0], extra)
            },
        )
        .expect("dual retention readers are total");
}

/// Replace a V1 checkpoint source's retention manifest by the given anchor
/// and radix objects under their stated references: DATA becomes the
/// retained union plus the metadata, and the descriptor is re-sealed under
/// the V2 inventory rule. A stated reference need not authenticate its bytes,
/// so a fixture can carry a forged object into the durable pack.
pub(super) fn v2_source_from_objects(
    v1_source: &SyntheticCheckpointSource,
    anchor: (ObjectRefV1, Vec<u8>),
    radix_objects: impl IntoIterator<Item = (ObjectRefV1, Vec<u8>)>,
) -> SyntheticCheckpointSource {
    let descriptor = v1_source.descriptor;
    let inventory = descriptor.inventory();
    let manifest = descriptor.retention_manifest();
    let anchor_reference = anchor.0;
    let mut source = v1_source.clone();
    source
        .objects
        .retain(|(reference, _)| *reference != manifest);
    let mut present = source
        .objects
        .iter()
        .map(|(reference, _)| reference.digest())
        .collect::<BTreeSet<_>>();
    let (mut metadata_objects, mut metadata_bytes) = (0_u64, 0_u64);
    for (reference, bytes) in std::iter::once(anchor).chain(radix_objects) {
        if present.insert(reference.digest()) {
            metadata_objects += 1;
            metadata_bytes += reference.encoded_len();
            source.push_object((reference, bytes));
        }
    }
    source.sort_objects_by_digest();
    let data_bytes = source
        .objects
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum::<u64>();
    let mut builder = OrderedObjectInventoryBuilderV1::new(source.objects.len() as u64, data_bytes);
    for (reference, _) in &source.objects {
        builder
            .push(*reference)
            .expect("V2 DATA is strictly ordered");
    }
    let report = RetainedUnionClosureReportV1 {
        distinct_objects: inventory.retained_union_objects(),
        canonical_bytes: inventory.retained_union_canonical_bytes().get(),
    };
    let inventory = CheckpointObjectInventoryV1::from_test_ordered_v2(
        report,
        metadata_objects,
        metadata_bytes,
        builder.finish().expect("V2 DATA is complete"),
    )
    .expect("V2 DATA is the retained union plus the metadata");
    source.descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        descriptor.profile_id(),
        descriptor.state(),
        anchor_reference,
        descriptor.current_state_objects(),
        descriptor.current_state_canonical_bytes().get(),
        inventory,
    )
    .expect("the V2 descriptor satisfies the V2 inventory rule");
    source
}

/// [`v2_source_from_objects`] with every reference computed from its bytes.
pub(super) fn v2_source_from_parts(
    v1_source: &SyntheticCheckpointSource,
    anchor_bytes: Vec<u8>,
    radix_objects: Vec<Vec<u8>>,
) -> SyntheticCheckpointSource {
    v2_source_from_objects(
        v1_source,
        (reference_of(&anchor_bytes), anchor_bytes),
        radix_objects
            .into_iter()
            .map(|bytes| (reference_of(&bytes), bytes)),
    )
}

fn history_of(historical: &[RetainedStateV1]) -> History {
    let history = historical
        .iter()
        .map(|entry| (model_key(entry.state()), entry.canonical_bytes()))
        .collect::<History>();
    assert_eq!(history.len(), historical.len(), "a V2 history has distinct keys");
    history
}

/// A V2 source over the given current state and history. Every historical
/// manifest must already be in the V1 source's retained union.
pub(super) fn v2_source_with_history(
    v1_source: &SyntheticCheckpointSource,
    current: RetainedStateV1,
    historical: &[RetainedStateV1],
) -> SyntheticCheckpointSource {
    let history = history_of(historical);
    let image = build_radix(&history);
    let anchor = RetentionAnchorV2::new(current, history.len() as u64, image.root)
        .expect("the anchor appoints its radix root");
    v2_source_from_parts(v1_source, encoded(&anchor), image.objects.into_values().collect())
}

/// [`v2_source_with_history`] with the radix child reached along `path`
/// replaced by `replacement(original)` and every ancestor re-encoded; the
/// anchor keeps the declared count. Returns the source and the original
/// child reference.
pub(super) fn v2_source_with_radix_child(
    v1_source: &SyntheticCheckpointSource,
    current: RetainedStateV1,
    historical: &[RetainedStateV1],
    path: &[usize],
    replacement: impl FnOnce(ObjectRefV1) -> ObjectRefV1,
) -> (SyntheticCheckpointSource, ObjectRefV1) {
    let history = history_of(historical);
    let mut image = build_radix(&history);
    let original = radix_occurrences(&image.objects, image.root)
        .into_iter()
        .find(|occurrence| occurrence.path == path)
        .expect("the path reaches a radix node")
        .reference;
    let root = rewrite_radix_path(&mut image.objects, image.root, path, replacement(original));
    let anchor = RetentionAnchorV2::new(current, history.len() as u64, root)
        .expect("the anchor appoints its radix root");
    let source =
        v2_source_from_parts(v1_source, encoded(&anchor), image.objects.into_values().collect());
    (source, original)
}

pub(super) fn v1_manifest_entries(
    v1_source: &SyntheticCheckpointSource,
) -> (RetainedStateV1, Vec<RetainedStateV1>) {
    let manifest_reference = v1_source.descriptor.retention_manifest();
    let (_, manifest_bytes) = v1_source
        .objects
        .iter()
        .find(|(reference, _)| *reference == manifest_reference)
        .expect("a V1 source carries its retention manifest");
    let manifest =
        RetentionManifestView::parse(manifest_bytes).expect("the V1 retention manifest parses");
    let historical = manifest
        .historical()
        .collect::<Result<Vec<_>, _>>()
        .expect("a parsed V1 manifest's entries decode");
    (manifest.current(), historical)
}

/// The V2 twin of a V1 checkpoint source: the same retained union and
/// history, with the manifest replaced by an anchor over a radix image.
pub(super) fn migrate_checkpoint_source_to_v2(
    v1_source: &SyntheticCheckpointSource,
) -> SyntheticCheckpointSource {
    let (current, historical) = v1_manifest_entries(v1_source);
    v2_source_with_history(v1_source, current, &historical)
}

/// Replace a V1 checkpoint source's retention manifest by the given bytes,
/// which need not parse: DATA becomes the retained union plus the new
/// manifest, and the descriptor is re-sealed under the V1 inventory rule.
pub(super) fn v1_source_with_manifest(
    v1_source: &SyntheticCheckpointSource,
    manifest_bytes: Vec<u8>,
) -> SyntheticCheckpointSource {
    let descriptor = v1_source.descriptor;
    let inventory = descriptor.inventory();
    let old_manifest = descriptor.retention_manifest();
    let manifest = reference_of(&manifest_bytes);
    let mut source = v1_source.clone();
    source
        .objects
        .retain(|(reference, _)| *reference != old_manifest);
    source.push_object((manifest, manifest_bytes));
    source.sort_objects_by_digest();
    let data_bytes = source
        .objects
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum::<u64>();
    let mut builder = OrderedObjectInventoryBuilderV1::new(source.objects.len() as u64, data_bytes);
    for (reference, _) in &source.objects {
        builder
            .push(*reference)
            .expect("V1 DATA is strictly ordered");
    }
    let report = RetainedUnionClosureReportV1 {
        distinct_objects: inventory.retained_union_objects(),
        canonical_bytes: inventory.retained_union_canonical_bytes().get(),
    };
    let inventory = CheckpointObjectInventoryV1::from_test_ordered(
        report,
        manifest,
        builder.finish().expect("V1 DATA is complete"),
    )
    .expect("V1 DATA is the retained union plus the manifest");
    source.descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        descriptor.profile_id(),
        descriptor.state(),
        manifest,
        descriptor.current_state_objects(),
        descriptor.current_state_canonical_bytes().get(),
        inventory,
    )
    .expect("the V1 descriptor satisfies the V1 inventory rule");
    source
}

/// Every radix defect applied at its first applicable occurrence in the
/// migrated image of a V1 source, as a durable V2 source together with the
/// unit reader's refusal of the defective image. Stored bytes keep their
/// digest-keyed references, so a forged object reaches the durable pack.
pub(super) fn radix_defect_sources(
    v1_source: &SyntheticCheckpointSource,
) -> Vec<(RadixDefect, SyntheticCheckpointSource, RetentionRadixErrorV2<CanonicalFormatError>)> {
    let (current, historical) = v1_manifest_entries(v1_source);
    let history = history_of(&historical);
    let image = build_radix(&history);
    let anchor = RetentionAnchorV2::new(current, history.len() as u64, image.root)
        .expect("the anchor appoints its radix root");
    let occurrences = radix_occurrences(&image.objects, image.root);
    let mut sources = Vec::with_capacity(RADIX_DEFECTS.len());
    for defect in RADIX_DEFECTS {
        let defective = occurrences
            .iter()
            .find_map(|occurrence| {
                apply_radix_defect(&image.objects, anchor, defect, occurrence, 0)
            })
            .unwrap_or_else(|| panic!("{defect:?} applies to the fixture's radix image"));
        let refusal = match admit_retention_history_v2(
            &tracing_store(&defective.objects),
            defective.anchor,
        ) {
            Err(error) => error,
            Ok(_) => panic!("the unit reader refuses {defect:?}"),
        };
        let anchor_bytes = encoded(&defective.anchor);
        let objects = defective.objects.into_iter().map(|(digest, bytes)| {
            let reference = ObjectRefV1::new(digest, bytes.len() as u64)
                .expect("defective radix objects keep an envelope");
            (reference, bytes)
        });
        let source =
            v2_source_from_objects(v1_source, (reference_of(&anchor_bytes), anchor_bytes), objects);
        sources.push((defect, source, refusal));
    }
    sources
}
