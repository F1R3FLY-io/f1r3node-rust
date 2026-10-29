use std::cell::Cell;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use super::retention_v2_format_properties::{
    History, KEY_WIDTH, ObjectSet, RadixImage, RadixNodeImage, RadixRefusal,
    assert_v1_readers_refuse, build_radix, clustered_state, encoded, history_strategy,
    insert_object, load_radix_node, memoized_count, metadata_closure, model_anchor_bytes,
    model_count_by_depth, model_decode_key, model_key, radix_node, radix_occurrences,
    raw_radix_node, retained, rewrite_radix_path, runner, state, tracing_store, unfold_radix,
};
use super::support::{TracingStore, nonzero};
use crate::rspace::persistence::closure::ledger_trial::{LedgerTrialLimitsV1, run_ledger_trial};
use crate::rspace::persistence::closure::{
    ClosureDomainV1, ClosureInventoryV1, StructuralClosureLedgerV1,
};
use crate::rspace::persistence::retention::v2_format::{
    RetentionAnchorV2, RetentionReader, empty_retention_radix_ref,
};
use crate::rspace::persistence::retention::v2_reader::{
    AdmittedRetentionHistoryV2, RetentionRadixErrorV2, RetentionRadixRefusalV2,
    admit_retention_history_v2,
};
use crate::rspace::persistence::retention::v2_writer::{
    RetentionStepV2, commit_retention_v2, evict_retention_v2, migrate_retention_v1,
};
use crate::rspace::persistence::rewrite::{
    CompareAndSwapFailure, DagRewriteReport, RadixDomainV1, rewrite_retention_history,
};
use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalFormatError, DagChild, DagRewriteLimits, EndpointValue,
    ObjectDigest, ObjectRefV1, RetainedStateV1, RetentionManifestV1, RetentionManifestView,
    RewriteError, StagingObjectStore, StagingStoreError, StateVersionV1,
};

/// The largest node of a 48-byte key image: an inner node with a Node256
/// child table and a prefix shorter than the key.
pub(super) const RADIX_NODE_BYTE_BOUND: u64 = 32 + 8 + KEY_WIDTH as u64 + 1 + 9 + 32 + 40 * 256;
/// The bytes one insertion or eviction may write: a search path of at most
/// `KEY_WIDTH + 1` nodes and the one node split off beside it.
pub(super) const PATH_LOCAL_BYTE_BOUND: u64 = (KEY_WIDTH as u64 + 2) * RADIX_NODE_BYTE_BOUND;
/// Two root events plus the expansions of one metadata step: at most 51
/// objects enter the closure and 50 leave it.
pub(super) const METADATA_STEP_WORK_BOUND: u64 = 2 * KEY_WIDTH as u64 + 7;
const GROWTH_EXPONENTS: std::ops::RangeInclusive<u32> = 4..=14;

type RadixEdit = ([u8; KEY_WIDTH], EndpointValue, EndpointValue);
type StagedError = StagingStoreError<CanonicalFormatError>;
type DualStaging<'a> = StagingObjectStore<'a, TracingStore>;

fn fail(message: String) -> TestCaseError { TestCaseError::fail(message) }

fn claim(value: u64) -> EndpointValue { EndpointValue::Multiplicity(nonzero(value)) }

fn dual_staging(base: &TracingStore) -> DualStaging<'_> {
    StagingObjectStore::new(base).with_retention_reader(RetentionReader::Dual)
}

fn staged_objects(staging: &DualStaging<'_>) -> ObjectSet {
    staging
        .staged_snapshot()
        .expect("the staging overlay is readable")
        .iter()
        .map(|(reference, bytes)| (reference.digest(), bytes.to_vec()))
        .collect()
}

fn overlaid(base: &ObjectSet, staged: &ObjectSet) -> ObjectSet {
    let mut objects = base.clone();
    for (digest, bytes) in staged {
        let previous = objects.insert(*digest, bytes.clone());
        assert!(previous.is_none_or(|previous| previous == *bytes), "staging never rewrites bytes");
    }
    objects
}

fn reachable(objects: &ObjectSet, root: ObjectRefV1) -> ObjectSet {
    let mut reached = ObjectSet::new();
    let mut pending = vec![root];
    while let Some(reference) = pending.pop() {
        let Entry::Vacant(slot) = reached.entry(reference.digest()) else {
            continue;
        };
        let node = load_radix_node(objects, reference).expect("reachable objects are radix nodes");
        let bytes = objects
            .get(&reference.digest())
            .expect("a loaded node is stored");
        slot.insert(bytes.clone());
        pending.extend(node.children.iter().map(|child| child.target));
    }
    reached
}

fn entries_of(history: &History) -> Vec<RetainedStateV1> {
    history
        .iter()
        .map(|(key, claim)| retained(model_decode_key(key).expect("model keys decode"), *claim))
        .collect()
}

fn history_of(entries: &[RetainedStateV1]) -> History {
    entries
        .iter()
        .map(|entry| (model_key(entry.state()), entry.canonical_bytes()))
        .collect()
}

fn with_entry(history: &History, entry: RetainedStateV1) -> History {
    let mut stepped = history.clone();
    let previous = stepped.insert(model_key(entry.state()), entry.canonical_bytes());
    assert!(previous.is_none(), "a committed entry is new to its history");
    stepped
}

fn without_entry(history: &History, entry: RetainedStateV1) -> History {
    let mut stepped = history.clone();
    let removed = stepped.remove(&model_key(entry.state()));
    assert_eq!(removed, Some(entry.canonical_bytes()), "an evicted entry is in its history");
    stepped
}

/// The independent rebuild of a history and the model anchor over it.
struct AnchoredImage {
    image: RadixImage,
    anchor: RetentionAnchorV2,
}

impl AnchoredImage {
    fn new(current: RetainedStateV1, history: &History) -> Self {
        let image = build_radix(history);
        let anchor = RetentionAnchorV2::parse(&model_anchor_bytes(
            current,
            history.len() as u64,
            image.root,
        ))
        .expect("the model anchor parses");
        Self { image, anchor }
    }

    fn objects(&self) -> ObjectSet {
        let mut objects = self.image.objects.clone();
        insert_object(&mut objects, encoded(&self.anchor));
        objects
    }
}

fn retention_state() -> impl Strategy<Value = StateVersionV1> {
    (
        prop_oneof![0_u64..6, 250_u64..262, Just(65_535_u64), Just(65_536_u64)],
        0_u8..3,
        0_u8..3,
        prop_oneof![Just(50_u64), Just(51_u64)],
    )
        .prop_map(|(generation, high, low, encoded_len)| {
            let mut digest = [high; 32];
            digest[31] = low;
            state(generation, digest, encoded_len)
        })
}

/// A V1 retention manifest as a history and its current state, whose
/// generation follows every historical one, as each commit leaves it.
fn manifest_strategy(max_entries: usize) -> impl Strategy<Value = (RetainedStateV1, History)> {
    (prop::collection::vec((retention_state(), 0_u64..2), 0..=max_entries), 0_u8..3, 0_u64..2)
        .prop_map(|(entries, seed, extra)| {
            let history = entries
                .into_iter()
                .map(|(state, extra)| (model_key(state), state.manifest().encoded_len() + extra))
                .collect::<History>();
            let generation = history.keys().next_back().map_or(0, |key| {
                model_decode_key(key)
                    .expect("model keys decode")
                    .generation() +
                    1
            });
            let mut digest = [seed; 32];
            digest[31] = seed;
            (retained(state(generation, digest, 50), 50 + extra), history)
        })
}

fn successor_of(current: RetainedStateV1, seed: u8, extra: u64) -> RetainedStateV1 {
    retained(state(current.state().generation() + 1, [seed; 32], 51), 51 + extra)
}

struct EditOutcome {
    root: ObjectRefV1,
    staged: ObjectSet,
    report: DagRewriteReport,
}

/// One editor batch over a stored image, or its refusal with the number of
/// objects it left staged.
fn edit_image(
    objects: &ObjectSet,
    root: ObjectRefV1,
    edits: &[RadixEdit],
) -> Result<EditOutcome, (RewriteError<StagedError>, usize)> {
    let base = tracing_store(objects);
    let staging = dual_staging(&base);
    match rewrite_retention_history(
        &staging,
        root,
        edits
            .iter()
            .map(|(key, before, after)| (&key[..], *before, *after)),
        DagRewriteLimits::UNBOUNDED,
    ) {
        Ok((root, report)) => Ok(EditOutcome {
            root,
            staged: staged_objects(&staging),
            report,
        }),
        Err(error) => Err((error, staging.len().expect("the staging overlay is readable"))),
    }
}

struct PathStep {
    reference: ObjectRefV1,
    node: RadixNodeImage,
    edge: Option<u8>,
}

/// The nodes a search for `key` visits: from the root to the node where the
/// key ends, leaves the node's prefix, or misses an edge.
fn search_path(objects: &ObjectSet, root: ObjectRefV1, key: &[u8; KEY_WIDTH]) -> Vec<PathStep> {
    let mut steps = Vec::with_capacity(KEY_WIDTH + 1);
    let (mut reference, mut edge, mut position) = (root, None, 0_usize);
    loop {
        let node =
            load_radix_node(objects, reference).expect("search paths run over canonical images");
        let next = match key[position..].strip_prefix(node.prefix.as_slice()) {
            Some([byte, ..]) => node
                .children
                .iter()
                .find(|child| child.edge == *byte)
                .map(|child| (child.target, *byte)),
            Some([]) | None => None,
        };
        position += node.prefix.len() + 1;
        steps.push(PathStep {
            reference,
            node,
            edge,
        });
        match next {
            Some((target, byte)) => {
                reference = target;
                edge = Some(byte);
            }
            None => return steps,
        }
    }
}

fn path_bytes(path: &[PathStep]) -> u64 {
    path.iter().map(|step| step.reference.encoded_len()).sum()
}

fn path_digests(path: &[PathStep]) -> BTreeSet<ObjectDigest> {
    path.iter().map(|step| step.reference.digest()).collect()
}

/// Every child of the old search path except the one followed at each step.
fn reused_references(old_path: &[PathStep]) -> u64 {
    let children = old_path
        .iter()
        .map(|step| step.node.children.len() as u64)
        .sum::<u64>();
    children - (old_path.len() as u64 - 1)
}

fn other_of_two(node: &RadixNodeImage, edge: u8) -> Option<DagChild> {
    match node.children.as_slice() {
        [first, second] if first.edge == edge => Some(*second),
        [first, second] if second.edge == edge => Some(*first),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct EditorWork {
    loaded_nodes: u64,
    loaded_bytes: u64,
    emitted_nodes: u64,
    emitted_bytes: u64,
    reused_child_references: u64,
    staged: BTreeSet<ObjectDigest>,
}

impl EditorWork {
    fn of(report: DagRewriteReport, staged: BTreeSet<ObjectDigest>) -> Self {
        Self {
            loaded_nodes: report.loaded_nodes,
            loaded_bytes: report.loaded_bytes,
            emitted_nodes: report.emitted_nodes,
            emitted_bytes: report.emitted_bytes,
            reused_child_references: report.reused_child_references,
            staged,
        }
    }
}

/// An insertion loads the old search path and builds the new one. A key that
/// leaves a node's prefix splits it: the new leaf's parent then has exactly
/// two children, and the node's remainder is built beside the path. The empty
/// root is never read: the leaf is built below it and merged into the root.
fn insertion_work(before: &RadixImage, after: &RadixImage, key: &[u8; KEY_WIDTH]) -> EditorWork {
    let new_path = search_path(&after.objects, after.root, key);
    if before.root == empty_retention_radix_ref() {
        let root = &new_path[0];
        let transient = radix_node(&key[1..], root.node.endpoint, &[]);
        return EditorWork {
            loaded_nodes: 0,
            loaded_bytes: 0,
            emitted_nodes: 2,
            emitted_bytes: transient.len() as u64 + root.reference.encoded_len(),
            reused_child_references: 0,
            staged: path_digests(&new_path),
        };
    }
    let old_path = search_path(&before.objects, before.root, key);
    let leaf = new_path.last().expect("a search path has a root");
    let parent = &new_path[new_path.len() - 2];
    let split = other_of_two(&parent.node, leaf.edge.expect("an inserted leaf has a parent"));
    let mut staged = path_digests(&new_path);
    staged.extend(split.map(|child| child.target.digest()));
    EditorWork {
        loaded_nodes: old_path.len() as u64,
        loaded_bytes: path_bytes(&old_path),
        emitted_nodes: new_path.len() as u64 + u64::from(split.is_some()),
        emitted_bytes: path_bytes(&new_path) + split.map_or(0, |child| child.target.encoded_len()),
        reused_child_references: reused_references(&old_path),
        staged,
    }
}

/// An eviction loads the old search path to the leaf; a parent left with one
/// child merges with it, which loads that sibling. The new search path is
/// built, and it is the empty root alone when the history empties.
fn eviction_work(before: &RadixImage, after: &RadixImage, key: &[u8; KEY_WIDTH]) -> EditorWork {
    let old_path = search_path(&before.objects, before.root, key);
    let new_path = search_path(&after.objects, after.root, key);
    let leaf = old_path.last().expect("a search path has a root");
    let merged = old_path.len().checked_sub(2).and_then(|parent| {
        other_of_two(&old_path[parent].node, leaf.edge.expect("an evicted leaf has a parent"))
    });
    EditorWork {
        loaded_nodes: old_path.len() as u64 + u64::from(merged.is_some()),
        loaded_bytes: path_bytes(&old_path) + merged.map_or(0, |child| child.target.encoded_len()),
        emitted_nodes: new_path.len() as u64,
        emitted_bytes: path_bytes(&new_path),
        reused_child_references: reused_references(&old_path),
        staged: path_digests(&new_path),
    }
}

fn check_path_locality(
    before: &RadixImage,
    after: &RadixImage,
    key: &[u8; KEY_WIDTH],
    work: &EditorWork,
    beside_limit: usize,
) -> Result<(), TestCaseError> {
    let old_path = search_path(&before.objects, before.root, key);
    let new_path = search_path(&after.objects, after.root, key);
    prop_assert!(old_path.len() <= KEY_WIDTH + 1 && new_path.len() <= KEY_WIDTH + 1);
    let old_path = path_digests(&old_path);
    let appeared = after
        .objects
        .keys()
        .filter(|digest| !before.objects.contains_key(*digest))
        .copied()
        .collect::<BTreeSet<_>>();
    let disappeared = before
        .objects
        .keys()
        .filter(|digest| !after.objects.contains_key(*digest))
        .copied()
        .collect::<BTreeSet<_>>();
    prop_assert!(appeared.is_subset(&work.staged), "{:?} appeared off the staged path", appeared);
    let beside = disappeared.difference(&old_path).count();
    prop_assert!(beside <= beside_limit, "{} removed nodes lie beside the old search path", beside);
    prop_assert!(work.staged.len() <= KEY_WIDTH + 2);
    prop_assert!(work.loaded_nodes <= KEY_WIDTH as u64 + 2);
    prop_assert!(work.emitted_bytes <= PATH_LOCAL_BYTE_BOUND);
    prop_assert!(
        before
            .objects
            .values()
            .chain(after.objects.values())
            .all(|bytes| bytes.len() as u64 <= RADIX_NODE_BYTE_BOUND)
    );
    Ok(())
}

/// A production step's work, staged radix objects and anchor bytes against
/// those the independent rebuild of the stepped history determines.
fn check_step(
    step: RetentionStepV2,
    staging: &DualStaging<'_>,
    reads: u64,
    expected_anchor: &[u8],
    expected_work: &EditorWork,
) -> Result<(), TestCaseError> {
    let mut staged = staged_objects(staging);
    let anchor = staged
        .remove(&step.anchor().object_ref().digest())
        .ok_or_else(|| fail("the successor anchor is staged".to_owned()))?;
    prop_assert_eq!(anchor.as_slice(), expected_anchor);
    prop_assert_eq!(encoded(&step.anchor()), expected_anchor);
    let actual = EditorWork::of(step.report(), staged.keys().copied().collect());
    prop_assert_eq!(&actual, expected_work);
    prop_assert_eq!(step.report().mutations, 1);
    prop_assert_eq!(step.report().inserted_objects, staged.len() as u64);
    prop_assert_eq!(reads, step.report().loaded_nodes);
    Ok(())
}

fn check_commit(
    current: RetainedStateV1,
    history: &History,
    target: RetainedStateV1,
) -> Result<EditorWork, TestCaseError> {
    let before = AnchoredImage::new(current, history);
    let stepped = with_entry(history, current);
    let after = build_radix(&stepped);
    let base = tracing_store(&before.objects());
    let staging = dual_staging(&base);
    let step = commit_retention_v2(&staging, before.anchor, target, DagRewriteLimits::UNBOUNDED)
        .map_err(|error| fail(format!("{error:?}")))?;
    let key = model_key(current.state());
    let work = insertion_work(&before.image, &after, &key);
    let anchor = model_anchor_bytes(target, stepped.len() as u64, after.root);
    check_step(step, &staging, base.total_reads(), &anchor, &work)?;
    check_path_locality(&before.image, &after, &key, &work, 0)?;
    Ok(work)
}

fn check_eviction(
    current: RetainedStateV1,
    history: &History,
    victim: RetainedStateV1,
) -> Result<EditorWork, TestCaseError> {
    let before = AnchoredImage::new(current, history);
    let evicted = without_entry(history, victim);
    let after = build_radix(&evicted);
    let base = tracing_store(&before.objects());
    let staging = dual_staging(&base);
    let step = evict_retention_v2(&staging, before.anchor, victim, DagRewriteLimits::UNBOUNDED)
        .map_err(|error| fail(format!("{error:?}")))?;
    let key = model_key(victim.state());
    let work = eviction_work(&before.image, &after, &key);
    let anchor = model_anchor_bytes(current, evicted.len() as u64, after.root);
    check_step(step, &staging, base.total_reads(), &anchor, &work)?;
    check_path_locality(&before.image, &after, &key, &work, 1)?;
    Ok(work)
}

#[derive(Clone, Copy, Debug)]
enum GrowthShape {
    Dense,
    Gapped,
    Aliased,
}

const GROWTH_SHAPES: [GrowthShape; 3] =
    [GrowthShape::Dense, GrowthShape::Gapped, GrowthShape::Aliased];

/// The `index`th state of a growing history: consecutive generations of
/// distinct manifests, generations with gaps, or one manifest at every
/// generation, whose leaves are one shared object.
fn growth_state(shape: GrowthShape, index: u64) -> RetainedStateV1 {
    let generation = match shape {
        GrowthShape::Dense | GrowthShape::Aliased => index,
        GrowthShape::Gapped => index * 1_009 + index % 7,
    };
    let (digest, claim) = match shape {
        GrowthShape::Aliased => (ObjectDigest::from_array([0x5a; 32]), 64),
        GrowthShape::Dense | GrowthShape::Gapped => {
            (ObjectDigest::of_canonical_bytes(&index.to_be_bytes()), 64 + index % 3)
        }
    };
    let manifest = ObjectRefV1::new(digest, 64).expect("a growth manifest has an envelope");
    retained(StateVersionV1::new(generation, manifest), claim)
}

fn growth_history(shape: GrowthShape, entries: u64) -> History {
    (0..entries)
        .map(|index| {
            let entry = growth_state(shape, index);
            (model_key(entry.state()), entry.canonical_bytes())
        })
        .collect()
}

fn admitted(
    staging: &DualStaging<'_>,
    anchor: RetentionAnchorV2,
) -> Result<AdmittedRetentionHistoryV2, TestCaseError> {
    admit_retention_history_v2(staging, anchor)
        .map_err(|error| fail(format!("the cold reader refuses an editor image: {error:?}")))
}

fn closure_inventory(closure: &BTreeMap<ObjectDigest, ObjectRefV1>) -> ClosureInventoryV1 {
    ClosureInventoryV1 {
        distinct_objects: closure.len() as u64,
        canonical_bytes: closure
            .values()
            .map(|reference| reference.encoded_len())
            .sum(),
    }
}

fn oracle_closure(
    anchor: RetentionAnchorV2,
    image: &RadixImage,
) -> BTreeMap<ObjectDigest, ObjectRefV1> {
    metadata_closure(anchor, image).expect("canonical images have complete closures")
}

/// The metadata-domain count of every closure object: its root multiplicity
/// plus one for each edge of a distinct closure object that names it.
fn oracle_occurrences(
    anchor: RetentionAnchorV2,
    image: &RadixImage,
) -> BTreeMap<ObjectDigest, u64> {
    let closure = oracle_closure(anchor, image);
    let mut counts = closure
        .keys()
        .map(|digest| (*digest, 0_u64))
        .collect::<BTreeMap<_, _>>();
    let mut credit = |digest: ObjectDigest| {
        *counts
            .get_mut(&digest)
            .expect("references stay inside the closure") += 1;
    };
    credit(anchor.object_ref().digest());
    credit(anchor.historical_root().digest());
    for reference in closure
        .values()
        .filter(|reference| reference.digest() != anchor.object_ref().digest())
    {
        let node = load_radix_node(&image.objects, *reference).expect("closure radix nodes load");
        for child in node.children {
            credit(child.target.digest());
        }
    }
    counts
}

fn seeded_ledger(
    staging: &DualStaging<'_>,
    anchor: RetentionAnchorV2,
) -> StructuralClosureLedgerV1 {
    let history =
        admit_retention_history_v2(staging, anchor).expect("the seeded anchor is admitted");
    let mut ledger = StructuralClosureLedgerV1::new();
    ledger
        .seed_retention_metadata(anchor.object_ref(), history.metadata())
        .expect("an empty metadata domain is seeded once");
    ledger
}

fn out_degree(digest: ObjectDigest, anchors: &[RetentionAnchorV2], objects: &ObjectSet) -> u64 {
    match anchors
        .iter()
        .any(|anchor| anchor.object_ref().digest() == digest)
    {
        true => 1,
        false => {
            let bytes = objects
                .get(&digest)
                .expect("a changed radix node is stored");
            let reference =
                ObjectRefV1::new(digest, bytes.len() as u64).expect("radix nodes carry envelopes");
            load_radix_node(objects, reference)
                .expect("changed radix nodes load")
                .children
                .len() as u64
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum AnchorStep {
    Commit(RetainedStateV1),
    Evict(RetainedStateV1),
}

/// Expansions of the faithful metadata trial of one step, and of the mutant
/// that deactivates the predecessor before activating the successor.
struct MetadataTrialWork {
    faithful: u64,
    deactivate_first: u64,
}

fn metadata_trial_work(
    current: RetainedStateV1,
    history: &History,
    step: AnchorStep,
) -> Result<MetadataTrialWork, TestCaseError> {
    let before = AnchoredImage::new(current, history);
    let base = tracing_store(&before.objects());
    let predecessor = seeded_ledger(&dual_staging(&base), before.anchor);
    let staging = dual_staging(&base);
    let limits = DagRewriteLimits::UNBOUNDED;
    let (produced, after) = match step {
        AnchorStep::Commit(target) => (
            commit_retention_v2(&staging, before.anchor, target, limits),
            build_radix(&with_entry(history, current)),
        ),
        AnchorStep::Evict(victim) => (
            evict_retention_v2(&staging, before.anchor, victim, limits),
            build_radix(&without_entry(history, victim)),
        ),
    };
    let successor = produced
        .map_err(|error| fail(format!("{error:?}")))?
        .anchor();
    let old = oracle_closure(before.anchor, &before.image);
    let new = oracle_closure(successor, &after);
    let departed = old
        .keys()
        .filter(|digest| !new.contains_key(*digest))
        .copied()
        .collect::<BTreeSet<_>>();
    let arrived = new
        .keys()
        .filter(|digest| !old.contains_key(*digest))
        .copied()
        .collect::<BTreeSet<_>>();
    let objects = overlaid(&before.objects(), &staged_objects(&staging));
    let anchors = [before.anchor, successor];
    let references = departed
        .iter()
        .chain(&arrived)
        .map(|digest| out_degree(*digest, &anchors, &objects))
        .sum::<u64>();

    base.clear_reads();
    let trial = run_ledger_trial(
        &staging,
        &predecessor,
        ClosureDomainV1::Metadata,
        &[successor.object_ref()],
        &[before.anchor.object_ref()],
        LedgerTrialLimitsV1::UNBOUNDED,
    )
    .map_err(|refusal| fail(format!("{refusal:?}")))?;
    prop_assert_eq!(trial.usage.expansions, (departed.len() + arrived.len()) as u64);
    prop_assert_eq!(trial.usage.references, 2 + references);
    prop_assert!(2 + trial.usage.expansions <= METADATA_STEP_WORK_BOUND);
    prop_assert_eq!(base.total_reads(), departed.len() as u64);
    prop_assert_eq!(trial.successor.metadata_inventory(), closure_inventory(&new));
    let cold = seeded_ledger(&staging, successor);
    for digest in old.keys().chain(new.keys()) {
        prop_assert_eq!(
            trial.successor.count(ClosureDomainV1::Metadata, *digest),
            cold.count(ClosureDomainV1::Metadata, *digest)
        );
    }
    let roots = |ledger: &StructuralClosureLedgerV1| {
        (
            ledger.root_count(ClosureDomainV1::Metadata, successor.object_ref().digest()),
            ledger.root_count(ClosureDomainV1::Metadata, before.anchor.object_ref().digest()),
        )
    };
    prop_assert_eq!(roots(&trial.successor), (1, 0));

    let deactivated = run_ledger_trial(
        &staging,
        &predecessor,
        ClosureDomainV1::Metadata,
        &[],
        &[before.anchor.object_ref()],
        LedgerTrialLimitsV1::UNBOUNDED,
    )
    .map_err(|refusal| fail(format!("{refusal:?}")))?;
    let reactivated = run_ledger_trial(
        &staging,
        &deactivated.successor,
        ClosureDomainV1::Metadata,
        &[successor.object_ref()],
        &[],
        LedgerTrialLimitsV1::UNBOUNDED,
    )
    .map_err(|refusal| fail(format!("{refusal:?}")))?;
    let deactivate_first = deactivated.usage.expansions + reactivated.usage.expansions;
    prop_assert_eq!(deactivate_first, (old.len() + new.len()) as u64);
    prop_assert_eq!(reactivated.successor.metadata_inventory(), closure_inventory(&new));
    prop_assert_eq!(roots(&reactivated.successor), (1, 0));
    Ok(MetadataTrialWork {
        faithful: trial.usage.expansions,
        deactivate_first,
    })
}

fn check_cold_admission(
    objects: &ObjectSet,
    anchor: RetentionAnchorV2,
    current: RetainedStateV1,
    history: &History,
) -> Result<(), TestCaseError> {
    let rebuild = build_radix(history);
    prop_assert_eq!(anchor.historical_root(), rebuild.root);
    prop_assert_eq!(anchor.current(), current);
    let read_back = admit_retention_history_v2(&tracing_store(objects), anchor)
        .map_err(|error| fail(format!("the cold reader refuses an editor image: {error:?}")))?;
    prop_assert_eq!(read_back.entries().to_vec(), entries_of(history));
    let closure = oracle_closure(anchor, &rebuild);
    prop_assert_eq!(
        read_back.metadata().objects().to_vec(),
        closure.values().copied().collect::<Vec<_>>()
    );
    prop_assert_eq!(
        read_back.metadata().canonical_bytes(),
        closure_inventory(&closure).canonical_bytes
    );
    let count = history.len() as u64;
    let leaves = unfold_radix(objects, anchor.historical_root(), count)
        .map_err(|refusal| fail(format!("{refusal:?}")))?;
    prop_assert_eq!(
        leaves.iter().map(|leaf| leaf.key).collect::<Vec<_>>(),
        history.keys().copied().collect::<Vec<_>>()
    );
    prop_assert_eq!(memoized_count(objects, anchor.historical_root()), Ok(count));
    match count {
        0 => {}
        _ => prop_assert_eq!(model_count_by_depth(objects, anchor.historical_root()), Ok(count)),
    }
    Ok(())
}

#[test]
fn canonical_radix_images_are_determined_by_their_entries() {
    let unary_controls = Cell::new(0_u64);
    runner(128)
        .run(
            &(history_strategy(24), history_strategy(8), any::<u64>(), any::<usize>()),
            |(history, extra, seed, pick)| {
                let expected = build_radix(&history);
                let empty = build_radix(&History::new());
                let batch = history
                    .iter()
                    .map(|(key, value)| (*key, EndpointValue::Absent, claim(*value)))
                    .collect::<Vec<_>>();
                let outcome = edit_image(&empty.objects, empty.root, &batch)
                    .map_err(|(error, _)| fail(format!("{error:?}")))?;
                prop_assert_eq!(outcome.root, expected.root);
                prop_assert_eq!(
                    reachable(&overlaid(&empty.objects, &outcome.staged), outcome.root),
                    expected.objects.clone()
                );

                let detours = extra
                    .into_iter()
                    .filter(|(key, _)| !history.contains_key(key))
                    .collect::<Vec<_>>();
                let order = |key: &[u8; KEY_WIDTH]| {
                    ObjectDigest::of_canonical_bytes(&[&seed.to_le_bytes()[..], &key[..]].concat())
                };
                let mut insertions = history
                    .iter()
                    .map(|(key, value)| (*key, *value))
                    .chain(detours.iter().copied())
                    .collect::<Vec<_>>();
                insertions.sort_by_key(|(key, _)| order(key));
                let mut evictions = detours;
                evictions.sort_by_key(|(key, _)| order(key));
                let steps = insertions
                    .iter()
                    .map(|(key, value)| (*key, EndpointValue::Absent, claim(*value)))
                    .chain(
                        evictions
                            .iter()
                            .map(|(key, value)| (*key, claim(*value), EndpointValue::Absent)),
                    );
                let (mut objects, mut root) = (empty.objects.clone(), empty.root);
                for edit in steps {
                    let outcome = edit_image(&objects, root, &[edit])
                        .map_err(|(error, _)| fail(format!("{error:?}")))?;
                    objects = overlaid(&objects, &outcome.staged);
                    root = outcome.root;
                }
                prop_assert_eq!(root, expected.root);
                prop_assert_eq!(reachable(&objects, root), expected.objects.clone());

                let leaves = radix_occurrences(&expected.objects, expected.root)
                    .into_iter()
                    .filter(|occurrence| occurrence.is_leaf() && !occurrence.node.prefix.is_empty())
                    .collect::<Vec<_>>();
                let Some(leaf) = leaves.get(pick % leaves.len().max(1)) else {
                    return Ok(());
                };
                let split = pick % leaf.node.prefix.len();
                let mut mutant = expected.objects.clone();
                let below = insert_object(
                    &mut mutant,
                    radix_node(&leaf.node.prefix[split + 1..], leaf.node.endpoint, &[]),
                );
                let unary = DagChild {
                    edge: leaf.node.prefix[split],
                    target: below,
                };
                prop_assert_eq!(
                    CanonicalDagNode::new(
                        leaf.node.prefix[..split].to_vec(),
                        EndpointValue::Absent,
                        vec![unary]
                    )
                    .err(),
                    Some(CanonicalFormatError::CompressibleUnaryNode)
                );
                let replacement = insert_object(
                    &mut mutant,
                    raw_radix_node(&leaf.node.prefix[..split], &[0], &[unary]),
                );
                let mutant_root =
                    rewrite_radix_path(&mut mutant, expected.root, &leaf.path, replacement);
                prop_assert_ne!(mutant_root, expected.root);
                let current = retained(state(u64::MAX, [0xcd; 32], 64), 4096);
                let count = history.len() as u64;
                let refusal = admit_retention_history_v2(
                    &tracing_store(&mutant),
                    RetentionAnchorV2::new(current, count, mutant_root)
                        .expect("the mutant root is not the empty root"),
                );
                prop_assert!(
                    matches!(
                        refusal,
                        Err(RetentionRadixErrorV2::Refusal(RetentionRadixRefusalV2::Body(
                            CanonicalFormatError::CompressibleUnaryNode
                        )))
                    ),
                    "{:?}",
                    refusal
                );
                prop_assert_eq!(
                    unfold_radix(&mutant, mutant_root, count).err(),
                    Some(RadixRefusal::Body(CanonicalFormatError::CompressibleUnaryNode))
                );
                prop_assert!(
                    admit_retention_history_v2(
                        &tracing_store(&expected.objects),
                        RetentionAnchorV2::new(current, count, expected.root)
                            .expect("the rebuild root is not the empty root"),
                    )
                    .is_ok()
                );
                unary_controls.set(unary_controls.get() + 1);
                Ok(())
            },
        )
        .expect("every edit order reaches the one canonical image of its entries");
    assert!(unary_controls.get() > 0, "no generated image had a leaf to split");
}

type CasChoice = (Option<usize>, StateVersionV1, u8, u64);

/// One CAS edit per chosen key, present or fresh, consistent with the
/// history or stale, in key order.
fn cas_batch(history: &History, choices: &[CasChoice]) -> Vec<RadixEdit> {
    let keys = history.keys().copied().collect::<Vec<_>>();
    let mut batch = BTreeMap::new();
    for (pick, fresh, kind, extra) in choices {
        let key = match (pick, keys.as_slice()) {
            (Some(index), [_, ..]) => keys[index % keys.len()],
            (Some(_) | None, _) => model_key(*fresh),
        };
        let length = model_decode_key(&key)
            .expect("model keys decode")
            .manifest()
            .encoded_len();
        let edit = match (history.get(&key), kind) {
            (Some(present), 0..=2) => (claim(*present), EndpointValue::Absent),
            (Some(present), 3..=5) => (claim(*present), claim(length + extra)),
            (Some(_), 6) => (EndpointValue::Absent, claim(length)),
            (Some(present), _) => (claim(present + 1), EndpointValue::Absent),
            (None, 0..=5) => (EndpointValue::Absent, claim(length + extra)),
            (None, _) => (claim(length), EndpointValue::Absent),
        };
        batch.entry(key).or_insert(edit);
    }
    batch
        .into_iter()
        .map(|(key, (before, after))| (key, before, after))
        .collect()
}

#[test]
fn radix_edits_equal_the_independent_rebuild() {
    let (applied, refused) = (Cell::new(0_u64), Cell::new(0_u64));
    runner(256)
        .run(
            &(
                history_strategy(24),
                prop::collection::vec(
                    (prop::option::of(any::<usize>()), clustered_state(), 0_u8..8, 0_u64..3),
                    0..10,
                ),
            ),
            |(history, choices)| {
                let image = build_radix(&history);
                let batch = cas_batch(&history, &choices);
                let mut updated = history.clone();
                let mut stale = None;
                for (key, before, after) in &batch {
                    let actual = updated
                        .get(key)
                        .map_or(EndpointValue::Absent, |value| claim(*value));
                    if actual != *before {
                        stale = Some(CompareAndSwapFailure {
                            domain: RadixDomainV1::RetentionHistory,
                            key_digest: ObjectDigest::of_canonical_bytes(key),
                            key_len: KEY_WIDTH as u64,
                            expected: *before,
                            actual,
                        });
                        break;
                    }
                    match after {
                        EndpointValue::Multiplicity(value) => updated.insert(*key, value.get()),
                        _ => updated.remove(key),
                    };
                }
                match (edit_image(&image.objects, image.root, &batch), stale) {
                    (Err((RewriteError::CompareAndSwap(failure), staged)), Some(expected)) => {
                        prop_assert_eq!(*failure, expected);
                        prop_assert_eq!(staged, 0);
                        refused.set(refused.get() + 1);
                    }
                    (Ok(outcome), None) => {
                        let rebuild = build_radix(&updated);
                        prop_assert_eq!(outcome.root, rebuild.root);
                        prop_assert_eq!(
                            reachable(&overlaid(&image.objects, &outcome.staged), outcome.root),
                            rebuild.objects.clone()
                        );
                        for (digest, bytes) in &outcome.staged {
                            prop_assert_eq!(rebuild.objects.get(digest), Some(bytes));
                        }
                        prop_assert_eq!(outcome.report.mutations, batch.len() as u64);
                        match (updated.is_empty(), batch.is_empty()) {
                            (true, false) => {
                                prop_assert_eq!(outcome.root, empty_retention_radix_ref());
                                prop_assert!(outcome.staged.contains_key(&outcome.root.digest()));
                            }
                            (true, true) => {
                                prop_assert_eq!(outcome.root, empty_retention_radix_ref());
                            }
                            (false, _) => {}
                        }
                        applied.set(applied.get() + 1);
                    }
                    (Ok(_), Some(expected)) => {
                        return Err(fail(format!("a stale CAS {expected:?} was applied")));
                    }
                    (Err((error, _)), _) => return Err(fail(format!("{error:?}"))),
                }
                Ok(())
            },
        )
        .expect("CAS-consistent batches yield the rebuild and stale batches stage nothing");
    assert!(applied.get() > 0, "no generated batch was consistent");
    assert!(refused.get() > 0, "no generated batch was stale");

    let left = retained(state(0, [0x31; 32], 50), 50);
    let right = retained(state(1, [0x31; 32], 50), 51);
    let history = history_of(&[left, right]);
    let image = build_radix(&history);
    let right_key = model_key(right.state());
    let outcome =
        edit_image(&image.objects, image.root, &[(right_key, claim(51), EndpointValue::Absent)])
            .expect("evicting a present entry is consistent");
    let rebuild = build_radix(&without_entry(&history, right));
    assert_eq!(outcome.root, rebuild.root);
    assert_eq!(
        load_radix_node(&rebuild.objects, rebuild.root)
            .expect("the rebuild is one leaf")
            .prefix,
        model_key(left.state()).to_vec()
    );
    let root = load_radix_node(&image.objects, image.root).expect("the two-entry root loads");
    let depth = root.prefix.len();
    let mut unpruned = image.objects.clone();
    let emptied = DagChild {
        edge: right_key[depth],
        target: insert_object(&mut unpruned, raw_radix_node(&right_key[depth + 1..], &[0], &[])),
    };
    let children = root
        .children
        .iter()
        .map(|child| match *child {
            DagChild { edge, .. } if edge == emptied.edge => emptied,
            kept => kept,
        })
        .collect::<Vec<_>>();
    let unpruned_root =
        insert_object(&mut unpruned, radix_node(&root.prefix, EndpointValue::Absent, &children));
    assert_ne!(unpruned_root, rebuild.root);
    assert_eq!(
        CanonicalDagNode::new(
            right_key[depth + 1..].to_vec(),
            EndpointValue::Absent,
            Vec::<DagChild>::new()
        )
        .err(),
        Some(CanonicalFormatError::EmptyNode)
    );
    let refusal = admit_retention_history_v2(
        &tracing_store(&unpruned),
        RetentionAnchorV2::new(retained(state(2, [0x32; 32], 50), 50), 1, unpruned_root)
            .expect("the unpruned root is not the empty root"),
    );
    assert!(
        matches!(
            refusal,
            Err(RetentionRadixErrorV2::Refusal(RetentionRadixRefusalV2::Body(
                CanonicalFormatError::EmptyNode
            )))
        ),
        "{refusal:?}"
    );
    assert_eq!(
        unfold_radix(&unpruned, unpruned_root, 1).err(),
        Some(RadixRefusal::Body(CanonicalFormatError::EmptyNode))
    );
}

#[test]
fn radix_edit_work_is_the_changed_path_under_geometric_growth() {
    runner(128)
        .run(
            &(manifest_strategy(16), any::<usize>(), 0_u8..3, 0_u64..2),
            |((current, history), victim, seed, extra)| {
                check_commit(current, &history, successor_of(current, seed, extra))?;
                match entries_of(&history).as_slice() {
                    [] => Ok(()),
                    entries => {
                        check_eviction(current, &history, entries[victim % entries.len()]).map(drop)
                    }
                }
            },
        )
        .expect("one commit or eviction does exactly the work of its search paths");

    let entry = |generation, digest, claim| retained(state(generation, [digest; 32], 50), claim);
    let vacant = check_commit(entry(0, 0x71, 50), &History::new(), entry(1, 0x72, 50))
        .unwrap_or_else(|error| panic!("commit into the empty history: {error}"));
    assert_eq!((vacant.loaded_nodes, vacant.emitted_nodes, vacant.staged.len()), (0, 2, 1));
    let split =
        check_commit(entry(1, 0x73, 50), &history_of(&[entry(0, 0x74, 50)]), entry(2, 0x75, 50))
            .unwrap_or_else(|error| panic!("commit splitting a leaf root: {error}"));
    assert_eq!((split.loaded_nodes, split.emitted_nodes, split.staged.len()), (1, 3, 3));
    let aliased_split = check_commit(
        entry(0x0200, 0x76, 64),
        &history_of(&[entry(0x0100, 0x76, 64)]),
        entry(0x0201, 0x77, 50),
    )
    .unwrap_or_else(|error| panic!("commit splitting off an equal leaf: {error}"));
    assert_eq!(
        (aliased_split.emitted_nodes, aliased_split.staged.len()),
        (3, 2),
        "the split remainder and the new leaf are one object"
    );
    let merge = check_eviction(
        entry(2, 0x7a, 50),
        &history_of(&[entry(0, 0x78, 50), entry(1, 0x79, 51)]),
        entry(1, 0x79, 51),
    )
    .unwrap_or_else(|error| panic!("eviction merging the root: {error}"));
    assert_eq!((merge.loaded_nodes, merge.emitted_nodes, merge.staged.len()), (3, 1, 1));
    let emptied =
        check_eviction(entry(1, 0x7c, 50), &history_of(&[entry(0, 0x7b, 50)]), entry(0, 0x7b, 50))
            .unwrap_or_else(|error| panic!("eviction emptying the history: {error}"));
    assert_eq!(emptied.staged, BTreeSet::from([empty_retention_radix_ref().digest()]));

    for exponent in GROWTH_EXPONENTS {
        let entries = 1_u64 << exponent;
        for shape in GROWTH_SHAPES {
            let history = growth_history(shape, entries);
            let current = growth_state(shape, entries);
            let target = growth_state(shape, entries + 1);
            check_commit(current, &history, target)
                .unwrap_or_else(|error| panic!("commit at {entries} {shape:?}: {error}"));
            check_eviction(
                target,
                &with_entry(&history, current),
                growth_state(shape, entries / 2),
            )
            .unwrap_or_else(|error| panic!("eviction at {entries} {shape:?}: {error}"));
            let flat = encoded(
                &RetentionManifestV1::new(current, entries_of(&history))
                    .expect("growth histories are V1 manifests"),
            )
            .len() as u64;
            assert_eq!(flat, 96 + 56 * entries);
            assert_eq!(flat > PATH_LOCAL_BYTE_BOUND, entries >= 9_258, "at {entries} entries");
        }
    }
}

#[test]
fn anchor_steps_realize_retention_entry_steps() {
    runner(128)
        .run(
            &(manifest_strategy(16), any::<usize>(), 0_u8..3, 0_u64..2),
            |((current, history), victim, seed, extra)| {
                let before = AnchoredImage::new(current, &history);
                let base = tracing_store(&before.objects());
                let target = successor_of(current, seed, extra);
                let staging = dual_staging(&base);
                let step = commit_retention_v2(
                    &staging,
                    before.anchor,
                    target,
                    DagRewriteLimits::UNBOUNDED,
                )
                .map_err(|error| fail(format!("{error:?}")))?;
                let committed = with_entry(&history, current);
                let image = build_radix(&committed);
                prop_assert_eq!(
                    encoded(&step.anchor()),
                    model_anchor_bytes(target, committed.len() as u64, image.root)
                );
                prop_assert_eq!(step.anchor().historical_count(), history.len() as u64 + 1);
                let read_back = admitted(&staging, step.anchor())?;
                prop_assert_eq!(read_back.entries().to_vec(), entries_of(&committed));
                prop_assert!(
                    RetentionManifestV1::new(target, read_back.entries().to_vec()).is_ok()
                );

                let entries = entries_of(&history);
                let Some(victim) = entries.get(victim % entries.len().max(1)).copied() else {
                    return Ok(());
                };
                let staging = dual_staging(&base);
                let step = evict_retention_v2(
                    &staging,
                    before.anchor,
                    victim,
                    DagRewriteLimits::UNBOUNDED,
                )
                .map_err(|error| fail(format!("{error:?}")))?;
                let evicted = without_entry(&history, victim);
                let image = build_radix(&evicted);
                prop_assert_eq!(
                    encoded(&step.anchor()),
                    model_anchor_bytes(current, evicted.len() as u64, image.root)
                );
                let read_back = admitted(&staging, step.anchor())?;
                prop_assert_eq!(read_back.entries().to_vec(), entries_of(&evicted));
                Ok(())
            },
        )
        .expect("successor anchors are the anchors of the stepped manifests");

    let aliased = |generation| retained(state(generation, [0x44; 32], 50), 64);
    let history = history_of(&[aliased(1), aliased(2)]);
    let current = aliased(3);
    let before = AnchoredImage::new(current, &history);
    let base = tracing_store(&before.objects());
    let staging = dual_staging(&base);
    let target = successor_of(current, 0x45, 0);
    let step = commit_retention_v2(&staging, before.anchor, target, DagRewriteLimits::UNBOUNDED)
        .expect("the aliased commit is consistent");
    let after = build_radix(&with_entry(&history, current));
    let leaves = |image: &RadixImage| {
        radix_occurrences(&image.objects, image.root)
            .into_iter()
            .filter(|occurrence| occurrence.is_leaf())
            .map(|occurrence| occurrence.reference.digest())
            .collect::<BTreeSet<_>>()
    };
    let (old_leaves, new_leaves) = (leaves(&before.image), leaves(&after));
    assert_eq!(old_leaves.len(), 1, "the two historical entries share one leaf object");
    assert_eq!(new_leaves, old_leaves, "the committed entry's leaf is that shared leaf");
    let stored_leaf_count = history.len() as u64 + (new_leaves.len() - old_leaves.len()) as u64;
    assert_eq!(step.anchor().historical_count(), 3);
    assert_ne!(stored_leaf_count, step.anchor().historical_count());
    assert_eq!(
        admitted(&staging, step.anchor())
            .expect("the CAS count is admitted")
            .entries()
            .len(),
        3
    );
    let undercounted = RetentionAnchorV2::new(target, stored_leaf_count, after.root)
        .expect("the undercounted root is not the empty root");
    let refusal = admit_retention_history_v2(&staging, undercounted);
    assert!(
        matches!(
            refusal,
            Err(RetentionRadixErrorV2::Refusal(RetentionRadixRefusalV2::CountMismatch {
                declared: 2,
                counted: 3,
            }))
        ),
        "{refusal:?}"
    );
}

#[derive(Clone, Copy, Debug)]
enum HistoryStep {
    Commit { seed: u8, extra: u64 },
    Evict { pick: usize },
}

fn history_step() -> impl Strategy<Value = HistoryStep> {
    prop_oneof![
        (0_u8..3, 0_u64..2).prop_map(|(seed, extra)| HistoryStep::Commit { seed, extra }),
        any::<usize>().prop_map(|pick| HistoryStep::Evict { pick }),
    ]
}

#[test]
fn editor_images_are_admitted_by_the_cold_reader() {
    let emptied = Cell::new(0_u64);
    runner(96)
        .run(
            &(manifest_strategy(6), prop::collection::vec(history_step(), 1..10)),
            |((mut current, mut history), steps)| {
                let start = AnchoredImage::new(current, &history);
                let (mut objects, mut anchor) = (start.objects(), start.anchor);
                check_cold_admission(&objects, anchor, current, &history)?;
                for step in steps {
                    let base = tracing_store(&objects);
                    let staging = dual_staging(&base);
                    let limits = DagRewriteLimits::UNBOUNDED;
                    let produced = match (step, entries_of(&history).as_slice()) {
                        (HistoryStep::Commit { seed, extra }, _) => {
                            let target = successor_of(current, seed, extra);
                            let produced = commit_retention_v2(&staging, anchor, target, limits);
                            history = with_entry(&history, current);
                            current = target;
                            produced
                        }
                        (HistoryStep::Evict { .. }, []) => continue,
                        (HistoryStep::Evict { pick }, entries) => {
                            let victim = entries[pick % entries.len()];
                            history = without_entry(&history, victim);
                            evict_retention_v2(&staging, anchor, victim, limits)
                        }
                    }
                    .map_err(|error| fail(format!("{error:?}")))?;
                    let staged = staged_objects(&staging);
                    if history.is_empty() {
                        prop_assert_eq!(
                            produced.anchor().historical_root(),
                            empty_retention_radix_ref()
                        );
                        prop_assert!(staged.contains_key(&empty_retention_radix_ref().digest()));
                        emptied.set(emptied.get() + 1);
                    }
                    objects = overlaid(&objects, &staged);
                    anchor = produced.anchor();
                    check_cold_admission(&objects, anchor, current, &history)?;
                }
                Ok(())
            },
        )
        .expect("every editor image is admitted with its entries, count and closure");
    assert!(emptied.get() > 0, "no generated sequence emptied its history");

    let left = retained(state(0, [0x51; 32], 50), 50);
    let right = retained(state(1, [0x51; 32], 50), 51);
    let history = history_of(&[left, right]);
    let current = retained(state(2, [0x52; 32], 50), 50);
    let before = AnchoredImage::new(current, &history);
    let base = tracing_store(&before.objects());
    let staging = dual_staging(&base);
    let step = evict_retention_v2(&staging, before.anchor, right, DagRewriteLimits::UNBOUNDED)
        .expect("the eviction is consistent");
    let faithful = admitted(&staging, step.anchor()).expect("the merged root is admitted");
    assert_eq!(faithful.entries(), [left]);
    let root = load_radix_node(&before.image.objects, before.image.root).expect("the root loads");
    let right_edge = model_key(right.state())[root.prefix.len()];
    let kept = root
        .children
        .iter()
        .copied()
        .filter(|child| child.edge != right_edge)
        .collect::<Vec<_>>();
    assert_eq!(kept.len(), 1);
    let mut unmerged = before.image.objects.clone();
    let unmerged_root = insert_object(&mut unmerged, raw_radix_node(&root.prefix, &[0], &kept));
    let refusal = admit_retention_history_v2(
        &tracing_store(&unmerged),
        RetentionAnchorV2::new(current, 1, unmerged_root)
            .expect("the unmerged root is not the empty root"),
    );
    assert!(
        matches!(
            refusal,
            Err(RetentionRadixErrorV2::Refusal(RetentionRadixRefusalV2::Body(
                CanonicalFormatError::CompressibleUnaryNode
            )))
        ),
        "{refusal:?}"
    );
    let unary = Err(RadixRefusal::Body(CanonicalFormatError::CompressibleUnaryNode));
    assert_eq!(
        unfold_radix(&unmerged, unmerged_root, 1).map(|leaves| leaves.len() as u64),
        unary.clone()
    );
    assert_eq!(memoized_count(&unmerged, unmerged_root), unary.clone());
    assert_eq!(model_count_by_depth(&unmerged, unmerged_root), unary);
}

#[test]
fn metadata_trial_work_is_the_changed_objects_under_geometric_growth() {
    runner(64)
        .run(
            &(manifest_strategy(12), any::<usize>(), 0_u8..3, 0_u64..2),
            |((current, history), victim, seed, extra)| {
                metadata_trial_work(
                    current,
                    &history,
                    AnchorStep::Commit(successor_of(current, seed, extra)),
                )?;
                match entries_of(&history).as_slice() {
                    [] => Ok(()),
                    entries => metadata_trial_work(
                        current,
                        &history,
                        AnchorStep::Evict(entries[victim % entries.len()]),
                    )
                    .map(drop),
                }
            },
        )
        .expect("a metadata trial expands exactly the objects one step changes");

    for exponent in GROWTH_EXPONENTS {
        let entries = 1_u64 << exponent;
        for shape in GROWTH_SHAPES {
            let history = growth_history(shape, entries);
            let current = growth_state(shape, entries);
            let target = growth_state(shape, entries + 1);
            let committed = with_entry(&history, current);
            let steps = [
                (current, &history, AnchorStep::Commit(target)),
                (target, &committed, AnchorStep::Evict(growth_state(shape, entries / 2))),
            ];
            for (current, history, step) in steps {
                let work = metadata_trial_work(current, history, step)
                    .unwrap_or_else(|error| panic!("{step:?} at {entries} {shape:?}: {error}"));
                assert!(2 + work.faithful <= METADATA_STEP_WORK_BOUND);
                match shape {
                    GrowthShape::Dense | GrowthShape::Gapped => assert!(
                        work.deactivate_first > entries,
                        "deactivating first expands the whole history at {entries} {shape:?}"
                    ),
                    GrowthShape::Aliased => {}
                }
            }
        }
    }
}

#[test]
fn seeded_metadata_ledger_equals_the_cold_ledger() {
    runner(128)
        .run(&manifest_strategy(16), |(current, history)| {
            let anchored = AnchoredImage::new(current, &history);
            let base = tracing_store(&anchored.objects());
            let staging = dual_staging(&base);
            let read_back = admitted(&staging, anchored.anchor)?;
            let occurrences = oracle_occurrences(anchored.anchor, &anchored.image);
            prop_assert_eq!(
                read_back.metadata().occurrences().to_vec(),
                occurrences.values().copied().collect::<Vec<_>>()
            );
            let seeded = seeded_ledger(&staging, anchored.anchor);
            let cold = run_ledger_trial(
                &staging,
                &StructuralClosureLedgerV1::new(),
                ClosureDomainV1::Metadata,
                &[anchored.anchor.object_ref()],
                &[],
                LedgerTrialLimitsV1::UNBOUNDED,
            )
            .map_err(|refusal| fail(format!("{refusal:?}")))?
            .successor;
            prop_assert_eq!(seeded.counted_objects(), cold.counted_objects());
            prop_assert_eq!(seeded.root_multiplicities(), cold.root_multiplicities());
            prop_assert_eq!(seeded.metadata_inventory(), cold.metadata_inventory());
            for (digest, count) in &occurrences {
                prop_assert_eq!(seeded.count(ClosureDomainV1::Metadata, *digest), *count);
            }
            Ok(())
        })
        .expect("the seeded metadata ledger is the ledger activation from empty builds");

    let aliased = |generation| retained(state(generation, [0x61; 32], 50), 64);
    let history = history_of(&[aliased(1), aliased(2), retained(state(3, [0x62; 32], 50), 64)]);
    let current = retained(state(4, [0x63; 32], 50), 64);
    let before = AnchoredImage::new(current, &history);
    let base = tracing_store(&before.objects());
    let staging = dual_staging(&base);
    let step = evict_retention_v2(&staging, before.anchor, aliased(2), DagRewriteLimits::UNBOUNDED)
        .expect("evicting one alias is consistent");
    let after = build_radix(&without_entry(&history, aliased(2)));
    let successor = closure_inventory(&oracle_closure(step.anchor(), &after));
    let read_back = admitted(&staging, before.anchor).expect("the aliased history is admitted");
    let shared =
        search_path(&before.image.objects, before.image.root, &model_key(aliased(1).state()))
            .last()
            .expect("the shared leaf ends its search path")
            .reference;
    let position = read_back
        .metadata()
        .objects()
        .iter()
        .position(|reference| *reference == shared)
        .expect("the shared leaf is a metadata object");
    assert_eq!(read_back.metadata().occurrences()[position], 2);
    let trial_from = |ledger: &StructuralClosureLedgerV1| {
        run_ledger_trial(
            &staging,
            ledger,
            ClosureDomainV1::Metadata,
            &[step.anchor().object_ref()],
            &[before.anchor.object_ref()],
            LedgerTrialLimitsV1::UNBOUNDED,
        )
        .expect("both seeds admit the alias eviction")
        .successor
    };
    let faithful = trial_from(&seeded_ledger(&staging, before.anchor));
    assert_eq!(faithful.count(ClosureDomainV1::Metadata, shared.digest()), 1);
    assert_eq!(faithful.metadata_inventory(), successor);
    let mut set_seeded = StructuralClosureLedgerV1::new();
    set_seeded
        .seed_retention_metadata(
            before.anchor.object_ref(),
            &read_back.metadata().with_occurrences_for_test(
                vec![1_u64; read_back.metadata().objects().len()].into_boxed_slice(),
            ),
        )
        .expect("an empty metadata domain is seeded once");
    let dropped = trial_from(&set_seeded);
    assert_eq!(dropped.count(ClosureDomainV1::Metadata, shared.digest()), 0);
    assert_eq!(dropped.metadata_inventory(), ClosureInventoryV1 {
        distinct_objects: successor.distinct_objects - 1,
        canonical_bytes: successor.canonical_bytes - shared.encoded_len(),
    });
}

#[test]
fn v1_migration_is_the_canonical_rebuild_and_refused_by_old_readers() {
    runner(128)
        .run(&(manifest_strategy(16), 0_u8..3, 0_u64..2), |((current, history), seed, extra)| {
            let manifest = RetentionManifestV1::new(current, entries_of(&history))
                .map_err(|error| fail(format!("{error:?}")))?;
            let manifest_bytes = encoded(&manifest);
            let view = RetentionManifestView::parse(&manifest_bytes)
                .map_err(|error| fail(format!("{error:?}")))?;
            let target = successor_of(current, seed, extra);
            let base = TracingStore::default();
            let staging = dual_staging(&base);
            let step = migrate_retention_v1(&staging, view, target, DagRewriteLimits::UNBOUNDED)
                .map_err(|error| fail(format!("{error:?}")))?;
            prop_assert_eq!(base.total_reads(), 0);
            prop_assert_eq!(step.report().loaded_nodes, 0);
            let migrated = with_entry(&history, current);
            prop_assert_eq!(step.report().mutations, migrated.len() as u64);
            let image = build_radix(&migrated);
            let anchor = model_anchor_bytes(target, migrated.len() as u64, image.root);
            prop_assert_eq!(encoded(&step.anchor()), anchor.clone());
            let mut staged = staged_objects(&staging);
            prop_assert_eq!(staged.remove(&step.anchor().object_ref().digest()), Some(anchor));
            prop_assert_eq!(&staged, &image.objects);

            let v2_source = AnchoredImage::new(current, &history);
            let commit_base = tracing_store(&v2_source.objects());
            let committed = commit_retention_v2(
                &dual_staging(&commit_base),
                v2_source.anchor,
                target,
                DagRewriteLimits::UNBOUNDED,
            )
            .map_err(|error| fail(format!("{error:?}")))?;
            prop_assert_eq!(committed.anchor(), step.anchor());

            let read_back = admitted(&staging, step.anchor())?;
            prop_assert_eq!(read_back.entries().to_vec(), entries_of(&migrated));
            for bytes in staged_objects(&staging).values() {
                assert_v1_readers_refuse(bytes);
            }

            let duplicated = RetentionAnchorV2::new(current, migrated.len() as u64, image.root)
                .map_err(|error| fail(format!("{error:?}")))?;
            let refusal = admit_retention_history_v2(&staging, duplicated);
            prop_assert!(
                matches!(
                    refusal,
                    Err(RetentionRadixErrorV2::Canonical(
                        CanonicalFormatError::RetentionContainsCurrent
                    ))
                ),
                "{:?}",
                refusal
            );
            prop_assert_eq!(
                RetentionManifestV1::new(current, entries_of(&migrated)).err(),
                Some(CanonicalFormatError::RetentionContainsCurrent)
            );
            Ok(())
        })
        .expect("migrating a V1 manifest is the canonical rebuild of its history");
}
