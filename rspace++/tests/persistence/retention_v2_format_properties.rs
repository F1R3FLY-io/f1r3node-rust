use std::collections::{BTreeMap, BTreeSet};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

use super::property_support::independent_inventory_digest;
use super::support::{TEST_SEMANTIC_PROFILE_ID, nonzero};
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, GENERATION_CHECKPOINT_DOMAIN, GenerationCheckpointErrorV1,
    OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::closure::RetainedUnionClosureReportV1;
use crate::rspace::persistence::object::{CanonicalObjectStreamValidatorV1, DAG_NODE_DOMAIN};
use crate::rspace::persistence::retention::RETENTION_MANIFEST_DOMAIN;
use crate::rspace::persistence::retention::v2_format::{
    EMPTY_RETENTION_RADIX_NODE, EMPTY_RETENTION_RADIX_NODE_LEN, GENERATION_CHECKPOINT_V2_DOMAIN,
    RETENTION_ANCHOR_CANONICAL_LEN, RETENTION_ANCHOR_DOMAIN, RETENTION_KEY_LEN,
    RETENTION_RADIX_DOMAIN, RetentionAnchorV2, RetentionKeyV2, empty_retention_radix_ref,
};
use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalDagNodeView, CanonicalEncode, CanonicalFormatError,
    CanonicalObjectKind, CanonicalObjectView, DagChild, EndpointValue, ObjectDigest, ObjectRefV1,
    RetainedStateV1, RetentionManifestV1, RetentionManifestView, SemanticRoots,
    SemanticStateManifestV1, SemanticStateManifestView, StateVersionV1,
};

const KEY_WIDTH: usize = 48;
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

type History = BTreeMap<[u8; KEY_WIDTH], u64>;
type ObjectSet = BTreeMap<ObjectDigest, Vec<u8>>;

fn runner(cases: u32) -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
}

fn reference(digest: [u8; 32], encoded_len: u64) -> ObjectRefV1 {
    ObjectRefV1::new(ObjectDigest::from_array(digest), encoded_len)
        .expect("model references cover an object envelope")
}

fn state(generation: u64, digest: [u8; 32], encoded_len: u64) -> StateVersionV1 {
    StateVersionV1::new(generation, reference(digest, encoded_len))
}

fn retained(state: StateVersionV1, claim: u64) -> RetainedStateV1 {
    RetainedStateV1::new(state, claim).expect("model claims cover the manifest length")
}

fn reference_of(bytes: &[u8]) -> ObjectRefV1 {
    ObjectRefV1::new(ObjectDigest::of_canonical_bytes(bytes), bytes.len() as u64)
        .expect("model objects carry an envelope")
}

fn encoded<T: CanonicalEncode>(value: &T) -> Vec<u8> {
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

fn model_key(state: StateVersionV1) -> [u8; KEY_WIDTH] {
    [
        &state.generation().to_be_bytes()[..],
        state.manifest().digest().as_bytes(),
        &state.manifest().encoded_len().to_be_bytes()[..],
    ]
    .concat()
    .try_into()
    .expect("a retention key is 48 bytes")
}

fn model_decode_key(key: &[u8]) -> Option<StateVersionV1> {
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

fn model_anchor_bytes(current: RetainedStateV1, count: u64, root: ObjectRefV1) -> Vec<u8> {
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

fn radix_node(prefix: &[u8], endpoint: EndpointValue, children: &[DagChild]) -> Vec<u8> {
    let node = CanonicalDagNode::new(prefix, endpoint, children.to_vec())
        .expect("model radix shapes are canonical DAG shapes");
    with_domain(&encoded(&node), MODEL_RADIX_DOMAIN)
}

fn insert_object(objects: &mut ObjectSet, bytes: Vec<u8>) -> ObjectRefV1 {
    let reference = reference_of(&bytes);
    objects.entry(reference.digest()).or_insert(bytes);
    reference
}

fn golden(bytes: &[u8], length: usize, digest: &str) {
    assert_eq!(bytes.len(), length);
    assert_eq!(hex::encode(ObjectDigest::of_canonical_bytes(bytes).as_bytes()), digest);
}

#[derive(Clone, Debug)]
struct RadixImage {
    root: ObjectRefV1,
    objects: ObjectSet,
}

struct PlannedNode {
    start: usize,
    end: usize,
    depth: usize,
    prefix_end: usize,
    children: Vec<(u8, usize)>,
}

fn build_radix(history: &History) -> RadixImage {
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
enum RadixRefusal {
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
struct RadixNodeImage {
    prefix: Vec<u8>,
    endpoint: EndpointValue,
    children: Vec<DagChild>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UnfoldedLeaf {
    key: [u8; KEY_WIDTH],
    claim: u64,
    object: ObjectDigest,
}

fn load_radix_node(
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

fn unfold_radix(
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

fn memoized_count(objects: &ObjectSet, root: ObjectRefV1) -> Result<u64, RadixRefusal> {
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

fn metadata_closure(
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

fn clustered_state() -> impl Strategy<Value = StateVersionV1> {
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

fn history_strategy(max_entries: usize) -> impl Strategy<Value = History> {
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

fn assert_v1_readers_refuse(bytes: &[u8]) {
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
