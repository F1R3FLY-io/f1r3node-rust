use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

use super::checkpoint_wal::fixture_with_state_and_history;
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{CheckpointPackImage, checkpoint_image_from_source, limits};
use super::path_exact_closure_properties::{ModelState, datum_key, install};
use super::retention_v2_editor_properties::{
    METADATA_STEP_WORK_BOUND, PATH_LOCAL_BYTE_BOUND, RADIX_NODE_BYTE_BOUND,
};
use super::retention_v2_format_properties::{
    History, ObjectSet, RadixImage, build_radix, load_radix_node, migrate_checkpoint_source_to_v2,
    model_anchor_bytes, model_key, reference_of, v1_manifest_entries,
};
use super::support::{TestSemanticProfile, encode, trial_scratch};
use super::trusted_history_production::{
    active_pack_name, failure, file_length, index_name, retained_history, walk,
};
use super::trusted_successor_production::{
    PublicationEffects, TrustedPolicyEvidence, observed_effects,
};
use crate::rspace::persistence::closure::ledger_trial::{LedgerTrialLimitsV1, run_ledger_trial};
use crate::rspace::persistence::closure::{ClosureDomainV1, StructuralClosureLedgerV1};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableAuthorityCoreV1, DurableDirectoryV1, DurableWriterLeaseV1,
    publish_trusted_incremental_v1,
};
use crate::rspace::persistence::retention::v2_format::{RetentionFormat, RetentionReader};
use crate::rspace::persistence::rewrite::DagRewriteReport;
use crate::rspace::persistence::{
    CanonicalObjectStore, CanonicalObjectView, DagTransactionLimits, DurableLimits, EndpointValue,
    INDEX_RECORD_LEN_V1, LogPackDecodeLimits, MutationBatchViewV1, OBJECT_RECORD_HEADER_LEN_V1,
    ObjectDigest, ObjectRefV1, OpenDagTransaction, RSpaceMutationBatchV1, RetainedStateV1,
    RetentionManifestV1, SemanticRoots, StateVersionV1, TRANSACTION_COMMIT_RECORD_LEN_V1,
    TRANSACTION_PREPARE_HEADER_LEN_V1,
};

pub(super) type Closure = BTreeMap<ObjectDigest, (ObjectRefV1, Arc<[u8]>)>;
type Counts = BTreeMap<ObjectDigest, [u64; 3]>;

const CASES: u32 = 8;
const HISTORY: usize = 16;
const CHECKSUM_LEN: u64 = 8;
const SEMANTIC_ROOT_DELTAS: u64 = 3;
const METADATA_ROOT_DELTAS: u64 = 2;
const DOMAIN_NAMES: [&str; 3] = ["current", "retained", "metadata"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Work {
    pub(super) references: u64,
    pub(super) expansions: u64,
    pub(super) quoted_bytes: u64,
}

pub(super) struct CommitRecord {
    pub(super) target: ObjectRefV1,
    pub(super) work: [Work; 3],
    pub(super) metadata_bytes: u64,
    pub(super) logical_writes: u64,
    pub(super) fresh_objects: u64,
    pub(super) fresh_bytes: u64,
    pub(super) pack_delta: u64,
    pub(super) index_delta: u64,
    pub(super) live_packs: u64,
    pub(super) old_path: u64,
    pub(super) new_path: u64,
    pub(super) deleted_objects: u64,
    pub(super) rewrite: DagRewriteReport,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Phase {
    Seal,
    Bind,
    Prepare,
    Publish,
}

pub(super) trait CommitProbe {
    fn phase<T>(&mut self, phase: Phase, operation: impl FnOnce() -> T) -> T;
}

struct Unprobed;

impl CommitProbe for Unprobed {
    fn phase<T>(&mut self, _: Phase, operation: impl FnOnce() -> T) -> T { operation() }
}

struct MetadataStep {
    work: Work,
    object: Vec<u8>,
    staged: ObjectSet,
    entered: BTreeSet<ObjectDigest>,
    superseded: BTreeSet<ObjectDigest>,
    touched: BTreeSet<ObjectDigest>,
    anchors: Option<(ObjectDigest, ObjectDigest)>,
}

pub(super) struct ScalingFixture {
    pub(super) writer: RetentionFormat,
    pub(super) oracle: CanonicalObjectStore,
    pub(super) image: CheckpointPackImage,
    pub(super) keys: Vec<Vec<u8>>,
    pub(super) model: ModelState,
    pub(super) state: StateVersionV1,
    pub(super) closure: Closure,
    pub(super) current: RetainedStateV1,
    pub(super) historical: Vec<RetainedStateV1>,
    pub(super) union: BTreeSet<ObjectDigest>,
    pub(super) stored: BTreeSet<ObjectDigest>,
}

pub(super) struct ScalingChain<'fixture> {
    pub(super) root: PathBuf,
    pub(super) lease: DurableWriterLeaseV1,
    pub(super) core: Arc<DurableAuthorityCoreV1<TestSemanticProfile>>,
    pub(super) fixture: &'fixture ScalingFixture,
    pub(super) model: ModelState,
    pub(super) state: StateVersionV1,
    pub(super) closure: Closure,
    pub(super) current: RetainedStateV1,
    pub(super) historical: Vec<RetainedStateV1>,
    pub(super) union: BTreeSet<ObjectDigest>,
    pub(super) stored: BTreeSet<ObjectDigest>,
}

pub(super) struct Planned {
    pub(super) model: ModelState,
    pub(super) batch: Vec<u8>,
    pub(super) target: ObjectRefV1,
    pub(super) closure: Closure,
}

fn scaling_runner(cases: u32) -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
}

fn count(length: usize) -> u64 { u64::try_from(length).expect("a test collection length fits u64") }

pub(super) fn scaling_key(index: usize) -> Vec<u8> {
    datum_key(&format!("chan-{index:06x}"), &format!("datum-{index:06x}"))
}

pub(super) fn mutation_batch(
    state: StateVersionV1,
    transitions: &mut [(&[u8], u64, u64)],
) -> Vec<u8> {
    transitions.sort_unstable_by_key(|(key, _, _)| *key);
    let mut batch =
        RSpaceMutationBatchV1::new(state).expect("the scaling base has a successor generation");
    for &(key, before, after) in transitions.iter() {
        batch
            .record_data(key, before, after)
            .expect("a scheduled datum transition is a valid mutation");
    }
    encode(&batch.plan().expect("a nonempty scaling batch plans"))
}

fn scheduled_mutation(
    model: &mut ModelState,
    state: StateVersionV1,
    keys: &[Vec<u8>],
    edits: &[(usize, u64)],
) -> Vec<u8> {
    let mut transitions = Vec::with_capacity(edits.len());
    for &(index, after) in edits {
        let key = &keys[index];
        let before = match after {
            0 => model.data.remove(key),
            after => model.data.insert(key.clone(), after),
        }
        .expect("a scheduled key holds a datum");
        transitions.push((key.as_slice(), before, after));
    }
    mutation_batch(state, &mut transitions)
}

pub(super) fn admit_scaling_directory(
    root: &Path,
    writer: RetentionFormat,
    limits: DurableLimits,
) -> (DurableWriterLeaseV1, Arc<DurableAuthorityCoreV1<TestSemanticProfile>>) {
    let (lease, source) = DurableDirectoryV1::open(root)
        .expect("open the scaling trial directory")
        .with_retention_reader_for_test(RetentionReader::Dual)
        .with_retention_writer_for_test(writer)
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits,
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("cold admission of the scaling fixture")
        .into_parts();
    (lease, source.into_core())
}

fn closure_bytes(closure: &Closure) -> u64 {
    closure
        .values()
        .map(|(reference, _)| reference.encoded_len())
        .sum()
}

fn manifest_roots(closure: &Closure, manifest: ObjectRefV1) -> SemanticRoots {
    let (_, bytes) = closure
        .get(&manifest.digest())
        .expect("a state closure holds its manifest");
    match CanonicalObjectView::parse(bytes).expect("a state manifest parses") {
        CanonicalObjectView::SemanticManifest(view) => view.roots(),
        _ => panic!("a state root is a semantic manifest"),
    }
}

fn semantic_out_refs(bytes: &[u8]) -> Vec<ObjectRefV1> {
    match CanonicalObjectView::parse(bytes).expect("an oracle object parses") {
        CanonicalObjectView::SemanticManifest(view) => {
            Vec::from(view.roots().iter().map(|(_, root)| root))
        }
        CanonicalObjectView::DagNode(node) => {
            let mut targets = Vec::with_capacity(node.children().len() + 1);
            targets.extend(node.children().iter().map(|child| child.target));
            if let EndpointValue::Content(content) = node.endpoint() {
                targets.push(content);
            }
            targets
        }
        CanonicalObjectView::Blob(_) => Vec::new(),
        _ => panic!("a state closure holds only manifests, nodes and blobs"),
    }
}

fn semantic_work(roots: u64, frontier: &Closure) -> Work {
    frontier.values().fold(
        Work {
            references: roots,
            ..Work::default()
        },
        |work, (reference, bytes)| Work {
            references: work.references + count(semantic_out_refs(bytes).len()),
            expansions: work.expansions + 1,
            quoted_bytes: work.quoted_bytes + reference.encoded_len(),
        },
    )
}

fn symmetric_difference(left: &Closure, right: &Closure) -> Closure {
    left.iter()
        .filter(|(digest, _)| !right.contains_key(*digest))
        .chain(
            right
                .iter()
                .filter(|(digest, _)| !left.contains_key(*digest)),
        )
        .map(|(digest, entry)| (*digest, entry.clone()))
        .collect()
}

fn only(left: &ObjectSet, right: &ObjectSet) -> ObjectSet {
    left.iter()
        .filter(|(digest, _)| !right.contains_key(*digest))
        .map(|(digest, bytes)| (*digest, bytes.clone()))
        .collect()
}

fn radix_image(entries: &[RetainedStateV1]) -> RadixImage {
    build_radix(
        &entries
            .iter()
            .map(|entry| (model_key(entry.state()), entry.canonical_bytes()))
            .collect::<History>(),
    )
}

fn retention_bytes(
    writer: RetentionFormat,
    current: RetainedStateV1,
    historical: &[RetainedStateV1],
) -> Vec<u8> {
    match writer {
        RetentionFormat::V1 => encode(
            &RetentionManifestV1::new(current, historical.to_vec())
                .expect("the model retention manifest is valid"),
        ),
        RetentionFormat::V2 => {
            model_anchor_bytes(current, count(historical.len()), radix_image(historical).root)
        }
    }
}

fn ledger_counts(ledger: &StructuralClosureLedgerV1) -> Counts {
    ledger
        .counted_objects()
        .into_iter()
        .map(|(reference, counts)| (reference.digest(), counts))
        .collect()
}

fn ledger_roots(ledger: &StructuralClosureLedgerV1) -> Counts {
    ledger.root_multiplicities().into_iter().collect()
}

fn zero_entries(counts: &Counts) -> BTreeSet<ObjectDigest> {
    counts
        .iter()
        .filter(|(_, counts)| **counts == [0; 3])
        .map(|(digest, _)| *digest)
        .collect()
}

fn search_path(closure: &Closure, root: ObjectRefV1, key: &[u8]) -> Vec<ObjectDigest> {
    let mut path = Vec::with_capacity(key.len() + 1);
    let mut node = root;
    let mut depth = 0;
    loop {
        path.push(node.digest());
        let (_, bytes) = closure
            .get(&node.digest())
            .expect("a search path stays inside its closure");
        let CanonicalObjectView::DagNode(view) =
            CanonicalObjectView::parse(bytes).expect("a trie node parses")
        else {
            panic!("a search path visits only DAG nodes");
        };
        let end = depth + view.prefix().len();
        assert_eq!(
            key.get(depth..end),
            Some(view.prefix()),
            "a stored key matches each prefix on its search path"
        );
        match key.get(end) {
            None => {
                assert!(
                    matches!(view.endpoint(), EndpointValue::Multiplicity(_)),
                    "a stored datum ends at its endpoint"
                );
                return path;
            }
            Some(edge) => {
                node = view
                    .children()
                    .get(*edge)
                    .expect("a stored key's search path continues");
                depth = end + 1;
            }
        }
    }
}

fn deleted_closure<'key>(
    closure: &Closure,
    manifest: ObjectRefV1,
    kept: impl Iterator<Item = &'key [u8]>,
) -> BTreeSet<ObjectDigest> {
    let roots = manifest_roots(closure, manifest);
    let mut live = BTreeSet::from([manifest.digest()]);
    live.extend(roots.iter().map(|(_, root)| root.digest()));
    for key in kept {
        live.extend(search_path(closure, roots.data_bag, key));
    }
    closure
        .keys()
        .filter(|digest| !live.contains(*digest))
        .copied()
        .collect()
}

impl MetadataStep {
    fn model(
        writer: RetentionFormat,
        (current, historical): (RetainedStateV1, &[RetainedStateV1]),
        (successor, successor_history): (RetainedStateV1, &[RetainedStateV1]),
    ) -> Self {
        match writer {
            RetentionFormat::V1 => Self {
                work: Work::default(),
                object: retention_bytes(writer, successor, successor_history),
                staged: ObjectSet::new(),
                entered: BTreeSet::new(),
                superseded: BTreeSet::new(),
                touched: BTreeSet::new(),
                anchors: None,
            },
            RetentionFormat::V2 => {
                let old = radix_image(historical);
                let new = radix_image(successor_history);
                let old_anchor =
                    reference_of(&model_anchor_bytes(current, count(historical.len()), old.root));
                let object =
                    model_anchor_bytes(successor, count(successor_history.len()), new.root);
                let anchor = reference_of(&object);
                let departed = only(&old.objects, &new.objects);
                let staged = only(&new.objects, &old.objects);
                let mut work = Work {
                    references: 4,
                    expansions: 2,
                    quoted_bytes: old_anchor.encoded_len() + anchor.encoded_len(),
                };
                let mut touched = BTreeSet::from([
                    old_anchor.digest(),
                    anchor.digest(),
                    old.root.digest(),
                    new.root.digest(),
                ]);
                for objects in [&departed, &staged] {
                    for bytes in objects.values() {
                        let node = load_radix_node(objects, reference_of(bytes))
                            .expect("a model radix node loads");
                        work.references += count(node.children.len());
                        work.expansions += 1;
                        work.quoted_bytes += count(bytes.len());
                        touched.extend(node.children.iter().map(|child| child.target.digest()));
                    }
                }
                let mut superseded = departed.into_keys().collect::<BTreeSet<_>>();
                superseded.insert(old_anchor.digest());
                let mut entered = staged.keys().copied().collect::<BTreeSet<_>>();
                entered.insert(anchor.digest());
                Self {
                    work,
                    object,
                    staged,
                    entered,
                    superseded,
                    touched,
                    anchors: Some((old_anchor.digest(), anchor.digest())),
                }
            }
        }
    }
}

impl ScalingFixture {
    pub(super) fn build(writer: RetentionFormat, key_count: usize, history: usize) -> Self {
        let keys = (0..key_count).map(scaling_key).collect::<Vec<_>>();
        let mut model = ModelState::default();
        model.data.extend(keys.iter().map(|key| (key.clone(), 1)));
        let oracle = CanonicalObjectStore::new();
        let manifest = install(&oracle, &model);
        let closure = walk(&oracle, &[manifest]).objects;
        let mut state_objects = Vec::with_capacity(closure.len());
        state_objects.extend(
            closure
                .values()
                .filter(|(reference, _)| *reference != manifest)
                .map(|(reference, bytes)| (*reference, bytes.to_vec())),
        );
        let generation = count(history)
            .checked_add(1)
            .expect("the fixture generation has a successor");
        let v1 = fixture_with_state_and_history(
            state_objects,
            manifest_roots(&closure, manifest),
            generation,
            history,
        );
        let (current, historical) = v1_manifest_entries(&v1);
        let state = StateVersionV1::new(generation, manifest);
        assert_eq!(current.state(), state, "the fixture appoints the installed manifest");
        assert_eq!(current.canonical_bytes(), closure_bytes(&closure));
        let source_objects = v1
            .objects
            .iter()
            .map(|(reference, bytes)| (reference.digest(), bytes.as_slice()))
            .collect::<BTreeMap<_, _>>();
        let mut retained_roots = Vec::with_capacity(history + 1);
        for entry in &historical {
            let reference = entry.state().manifest();
            let (admitted, _) = oracle
                .admit(Arc::from(source_objects[&reference.digest()]))
                .expect("a historical manifest is canonical");
            assert_eq!(admitted, reference);
            retained_roots.push(reference);
        }
        retained_roots.push(manifest);
        let union = walk(&oracle, &retained_roots)
            .objects
            .into_keys()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            union.len(),
            closure.len() + history,
            "each historical state adds only its manifest to the retained union"
        );
        let source = match writer {
            RetentionFormat::V1 => v1,
            RetentionFormat::V2 => migrate_checkpoint_source_to_v2(&v1),
        };
        let appointed = source.descriptor.retention_manifest();
        let (_, appointed_bytes) = source
            .objects
            .iter()
            .find(|(reference, _)| *reference == appointed)
            .expect("the source holds its retention object");
        assert_eq!(
            appointed_bytes,
            &retention_bytes(writer, current, &historical),
            "the model encodes the appointed retention object"
        );
        let stored = source
            .objects
            .iter()
            .map(|(reference, _)| reference.digest())
            .collect();
        Self {
            writer,
            oracle,
            image: checkpoint_image_from_source(source),
            keys,
            model,
            state,
            closure,
            current,
            historical,
            union,
            stored,
        }
    }
}

impl<'fixture> ScalingChain<'fixture> {
    pub(super) fn new(
        fixture: &'fixture ScalingFixture,
        root: PathBuf,
        lease: DurableWriterLeaseV1,
        core: Arc<DurableAuthorityCoreV1<TestSemanticProfile>>,
    ) -> Self {
        assert_eq!(core.authority().slot().state(), fixture.state);
        Self {
            root,
            lease,
            core,
            fixture,
            model: fixture.model.clone(),
            state: fixture.state,
            closure: fixture.closure.clone(),
            current: fixture.current,
            historical: fixture.historical.clone(),
            union: fixture.union.clone(),
            stored: fixture.stored.clone(),
        }
    }

    fn open(fixture: &'fixture ScalingFixture, root: PathBuf, limits: DurableLimits) -> Self {
        install_checkpoint_image(&root, &fixture.image);
        let (lease, core) = admit_scaling_directory(&root, fixture.writer, limits);
        Self::new(fixture, root, lease, core)
    }

    pub(super) fn plan(&self, edits: &[(usize, u64)]) -> Planned {
        let mut model = self.model.clone();
        let batch = scheduled_mutation(&mut model, self.state, &self.fixture.keys, edits);
        let target = install(&self.fixture.oracle, &model);
        let closure = walk(&self.fixture.oracle, &[target]).objects;
        Planned {
            model,
            batch,
            target,
            closure,
        }
    }

    fn deactivate_first_expansions(&self, target: ObjectRefV1) -> u64 {
        let removed = run_ledger_trial(
            &self.fixture.oracle,
            self.core.retained_closure(),
            ClosureDomainV1::Current,
            &[],
            &[self.state.manifest()],
            LedgerTrialLimitsV1::UNBOUNDED,
        )
        .expect("the removal-only control trial succeeds");
        let added = run_ledger_trial(
            &self.fixture.oracle,
            &removed.successor,
            ClosureDomainV1::Current,
            &[target],
            &[],
            LedgerTrialLimitsV1::UNBOUNDED,
        )
        .expect("the addition-only control trial succeeds");
        removed.usage.expansions + added.usage.expansions
    }

    pub(super) fn commit(
        &mut self,
        planned: Planned,
        probe: &mut impl CommitProbe,
    ) -> Result<CommitRecord, TestCaseError> {
        let writer = self.fixture.writer;
        let base = self.state.manifest();
        let target = planned.target;
        let successor = StateVersionV1::new(
            self.state
                .generation()
                .checked_add(1)
                .expect("the scaling generation has a successor"),
            target,
        );
        prop_assert_eq!(retained_history(&self.core), (self.historical.clone(), self.current));
        let retention_before = retention_bytes(writer, self.current, &self.historical);
        prop_assert_eq!(self.core.retention_manifest_bytes(), retention_before.as_slice());

        let current_frontier = symmetric_difference(&self.closure, &planned.closure);
        let retained_frontier = planned
            .closure
            .iter()
            .filter(|(digest, _)| !self.union.contains(*digest))
            .map(|(digest, entry)| (*digest, entry.clone()))
            .collect::<Closure>();
        let new_path = count(
            planned
                .closure
                .keys()
                .filter(|digest| !self.closure.contains_key(*digest))
                .count(),
        );
        let departed = count(current_frontier.len())
            .checked_sub(new_path)
            .ok_or_else(|| TestCaseError::fail("the new path exceeds the changed frontier"))?;
        let deleted_objects = if planned.model.data.len() == self.model.data.len() {
            0
        } else {
            let deleted =
                deleted_closure(&self.closure, base, planned.model.data.keys().map(Vec::as_slice));
            prop_assert!(
                deleted
                    .iter()
                    .all(|digest| !planned.closure.contains_key(digest)),
                "a deleted object stays in the target closure"
            );
            count(deleted.len())
        };
        let old_path = departed.checked_sub(deleted_objects).ok_or_else(|| {
            TestCaseError::fail("the deleted closure exceeds the departed objects")
        })?;
        let current = RetainedStateV1::new(successor, closure_bytes(&planned.closure))
            .expect("the successor claims its closure bytes");
        let mut historical = Vec::with_capacity(self.historical.len() + 1);
        historical.extend_from_slice(&self.historical);
        historical.push(self.current);
        let metadata =
            MetadataStep::model(writer, (self.current, &self.historical), (current, &historical));
        let expected = [
            semantic_work(2, &current_frontier),
            semantic_work(1, &retained_frontier),
            metadata.work,
        ];

        let seal = || {
            OpenDagTransaction::new(
                self.core.verified_state(),
                MutationBatchViewV1::parse(&planned.batch).expect("parse the scaling mutation"),
                DagTransactionLimits::UNBOUNDED,
            )
            .and_then(OpenDagTransaction::seal)
            .expect("seal the scaling mutation on the trusted core")
        };
        let evidence = TrustedPolicyEvidence::measure(&self.lease, &self.core, seal());
        prop_assert_eq!(evidence.target(), successor);
        let policy = evidence.exact();
        for ((name, work), expected) in DOMAIN_NAMES.into_iter().zip(policy.work).zip(expected) {
            let observed = Work {
                references: work.references,
                expansions: work.expansions,
                quoted_bytes: work.quoted_bytes,
            };
            prop_assert_eq!(
                observed,
                expected,
                "the {} trial work is not the changed frontier",
                name
            );
        }

        let before = self.core.authority().slot();
        let counts_before = ledger_counts(self.core.retained_closure());
        let roots_before = ledger_roots(self.core.retained_closure());
        let transaction = probe.phase(Phase::Seal, seal);
        let rewrite = transaction.diagnostics().rewrite;
        let bound = probe
            .phase(Phase::Bind, || {
                self.core
                    .bind_trusted_transaction(&self.lease, transaction, policy)
            })
            .map_err(|error| failure("the exact-policy bind refused", error))?;
        prop_assert_eq!(bound.receipt().base(), self.state);
        prop_assert_eq!(bound.receipt().target(), successor);

        let mut fresh = retained_frontier
            .iter()
            .map(|(digest, (_, bytes))| (*digest, bytes.to_vec()))
            .collect::<ObjectSet>();
        fresh.extend(
            metadata
                .staged
                .iter()
                .map(|(digest, bytes)| (*digest, bytes.clone())),
        );
        let retention = reference_of(&metadata.object);
        fresh.insert(retention.digest(), metadata.object.clone());
        let sealed = bound
            .objects()
            .iter()
            .map(|object| (object.reference().digest(), object.canonical_bytes().to_vec()))
            .collect::<ObjectSet>();
        prop_assert_eq!(sealed.len(), bound.objects().len(), "the sealed objects repeat a digest");
        let missing = fresh
            .keys()
            .filter(|digest| !sealed.contains_key(*digest))
            .collect::<Vec<_>>();
        let extra = sealed
            .keys()
            .filter(|digest| !fresh.contains_key(*digest))
            .collect::<Vec<_>>();
        prop_assert!(
            missing.is_empty() && extra.is_empty(),
            "the sealed objects are not the fresh frontier: missing {:?}, extra {:?}",
            missing,
            extra
        );
        prop_assert!(sealed == fresh, "a sealed object differs from its model bytes");
        prop_assert!(
            fresh.keys().all(|digest| !self.stored.contains(digest)),
            "a sealed object is already stored"
        );
        prop_assert_eq!(bound.retention_manifest(), Some(retention));
        let staged_bytes = metadata
            .staged
            .values()
            .map(|bytes| count(bytes.len()))
            .sum::<u64>();
        match metadata.anchors {
            None => {
                prop_assert_eq!(count(metadata.object.len()), 96 + 56 * count(historical.len()));
            }
            Some(_) => {
                prop_assert!(2 + metadata.work.expansions <= METADATA_STEP_WORK_BOUND);
                prop_assert!(staged_bytes <= PATH_LOCAL_BYTE_BOUND);
                prop_assert!(
                    metadata
                        .staged
                        .values()
                        .all(|bytes| count(bytes.len()) <= RADIX_NODE_BYTE_BOUND)
                );
            }
        }

        let sequence = before
            .commit_sequence()
            .successor()
            .expect("the published commit sequence has a successor");
        let prepared = probe
            .phase(Phase::Prepare, || {
                self.core.prepare_transaction(
                    &self.lease,
                    &bound,
                    sequence,
                    LogPackDecodeLimits::UNBOUNDED,
                )
            })
            .expect("prepare the trusted-bound scaling commit");
        let (published, effects) = observed_effects(|| {
            probe.phase(Phase::Publish, || {
                publish_trusted_incremental_v1(
                    &mut self.lease,
                    &self.core,
                    &prepared,
                    policy,
                    |_| {},
                )
            })
        });
        let core = published.map_err(|error| failure("the trusted publication failed", error))?;
        prop_assert_eq!(effects, PublicationEffects::trusted());

        let after = core.authority().slot();
        prop_assert_eq!(after.state(), successor);
        prop_assert_eq!(after.active_pack(), before.active_pack());
        prop_assert_eq!(after.pack().index_generation(), before.pack().index_generation());
        let fresh_objects = count(fresh.len());
        let fresh_bytes = fresh.values().map(|bytes| count(bytes.len())).sum::<u64>();
        let pack_delta = after
            .durable_pack_tail()
            .checked_sub(before.durable_pack_tail())
            .ok_or_else(|| TestCaseError::fail("the durable pack tail moved backwards"))?;
        let index_delta = after
            .durable_index_tail()
            .checked_sub(before.durable_index_tail())
            .ok_or_else(|| TestCaseError::fail("the durable index tail moved backwards"))?;
        prop_assert_eq!(
            pack_delta,
            TRANSACTION_PREPARE_HEADER_LEN_V1 +
                count(planned.batch.len()) +
                fresh_objects * OBJECT_RECORD_HEADER_LEN_V1 +
                fresh_bytes +
                CHECKSUM_LEN +
                TRANSACTION_COMMIT_RECORD_LEN_V1
        );
        prop_assert_eq!(index_delta, fresh_objects * INDEX_RECORD_LEN_V1);
        prop_assert_eq!(
            file_length(&self.root.join(active_pack_name(after))),
            after.durable_pack_tail()
        );
        prop_assert_eq!(
            file_length(&self.root.join(index_name(after))),
            after.durable_index_tail()
        );
        let live_packs = after
            .pack()
            .active_pack()
            .checked_sub(after.pack().first_live_pack())
            .and_then(|span| span.checked_add(1))
            .ok_or_else(|| TestCaseError::fail("the first live pack follows the active pack"))?;

        let counts_after = ledger_counts(core.retained_closure());
        let roots_after = ledger_roots(core.retained_closure());
        let mut entered = retained_frontier.keys().copied().collect::<BTreeSet<_>>();
        entered.extend(metadata.entered.iter().copied());
        let added = counts_after
            .keys()
            .filter(|digest| !counts_before.contains_key(*digest))
            .copied()
            .collect::<BTreeSet<_>>();
        prop_assert_eq!(&added, &entered, "the new ledger entries are not the fresh digests");
        prop_assert!(
            counts_before
                .keys()
                .all(|digest| counts_after.contains_key(digest)),
            "a ledger entry disappeared"
        );
        let mut zeros = zero_entries(&counts_before);
        zeros.extend(metadata.superseded.iter().copied());
        prop_assert_eq!(
            zero_entries(&counts_after),
            zeros,
            "the zero entries did not grow by exactly the superseded metadata"
        );
        let mut touched = metadata.touched.clone();
        touched.insert(target.digest());
        touched.insert(base.digest());
        for (_, bytes) in current_frontier.values().chain(retained_frontier.values()) {
            touched.extend(
                semantic_out_refs(bytes)
                    .into_iter()
                    .map(|reference| reference.digest()),
            );
        }
        let changed = counts_after
            .iter()
            .filter(|(digest, counts)| counts_before.get(*digest) != Some(*counts))
            .map(|(digest, _)| *digest)
            .collect::<BTreeSet<_>>();
        let references = expected.iter().map(|work| work.references).sum::<u64>();
        prop_assert!(
            changed.is_subset(&touched),
            "a ledger entry changed outside the touched frontier"
        );
        prop_assert!(
            count(changed.len()) <= references,
            "{} ledger entries changed under {} references",
            changed.len(),
            references
        );
        let mut roots = roots_before;
        let previous = roots.insert(target.digest(), [1, 1, 0]);
        prop_assert_eq!(previous, None, "the target was already a root");
        let base_counts = roots
            .get_mut(&base.digest())
            .ok_or_else(|| TestCaseError::fail("the base is not a root"))?;
        base_counts[0] = base_counts[0]
            .checked_sub(1)
            .ok_or_else(|| TestCaseError::fail("the base holds no current root"))?;
        let root_deltas = match metadata.anchors {
            None => SEMANTIC_ROOT_DELTAS,
            Some((old_anchor, anchor)) => {
                let previous = roots.insert(anchor, [0, 0, 1]);
                prop_assert_eq!(previous, None, "the new anchor was already a root");
                let old_counts = roots
                    .get_mut(&old_anchor)
                    .ok_or_else(|| TestCaseError::fail("the old anchor is not a root"))?;
                old_counts[2] = old_counts[2]
                    .checked_sub(1)
                    .ok_or_else(|| TestCaseError::fail("the old anchor holds no metadata root"))?;
                SEMANTIC_ROOT_DELTAS + METADATA_ROOT_DELTAS
            }
        };
        prop_assert_eq!(
            roots_after,
            roots,
            "the root multiplicities changed beyond the commit's root deltas"
        );

        self.core = Arc::new(core);
        self.stored.extend(fresh.into_keys());
        self.union.extend(planned.closure.keys().copied());
        self.historical = historical;
        self.current = current;
        self.closure = planned.closure;
        self.state = successor;
        self.model = planned.model;
        Ok(CommitRecord {
            target,
            work: expected,
            metadata_bytes: count(metadata.object.len()) + staged_bytes,
            logical_writes: references + root_deltas,
            fresh_objects,
            fresh_bytes,
            pack_delta,
            index_delta,
            live_packs,
            old_path,
            new_path,
            deleted_objects,
            rewrite,
        })
    }
}

#[test]
fn trusted_commit_work_is_the_changed_frontier_under_geometric_current_growth() {
    const KEY_COUNTS: [usize; 4] = [16, 64, 256, 1024];
    const WIDTHS: [usize; 3] = [1, 4, 1];
    let mut minima = Vec::with_capacity(KEY_COUNTS.len());
    for key_count in KEY_COUNTS {
        let fixture = ScalingFixture::build(RetentionFormat::V2, key_count, HISTORY);
        let minimum = Cell::new(u64::MAX);
        let peak = Cell::new([0_u64; 4]);
        scaling_runner(CASES)
            .run(&(0..key_count, 0..key_count / 2), |(offset, half)| {
                let stride = 2 * half + 1;
                let directory = trial_scratch("trusted-scaling-frontier-");
                let mut chain =
                    ScalingChain::open(&fixture, directory.path().to_path_buf(), limits());
                let mut position = 0;
                for (commit, width) in WIDTHS.into_iter().enumerate() {
                    let multiplicity = 2 + count(commit);
                    let edits = (position..position + width)
                        .map(|step| ((offset + step * stride) % key_count, multiplicity))
                        .collect::<Vec<_>>();
                    position += width;
                    let planned = chain.plan(&edits);
                    let control = chain.deactivate_first_expansions(planned.target);
                    prop_assert_eq!(control, count(chain.closure.len() + planned.closure.len()));
                    let shared = count(
                        planned
                            .closure
                            .keys()
                            .filter(|digest| chain.closure.contains_key(*digest))
                            .count(),
                    );
                    let record = chain.commit(planned, &mut Unprobed)?;
                    let excess =
                        control
                            .checked_sub(record.work[0].expansions)
                            .ok_or_else(|| {
                                TestCaseError::fail(
                                    "the deactivate-first control expanded less than the trusted \
                                     commit",
                                )
                            })?;
                    prop_assert_eq!(excess, 2 * shared);
                    minimum.set(minimum.get().min(excess));
                    let [current, retained, metadata, writes] = peak.get();
                    peak.set([
                        current.max(record.work[0].expansions),
                        retained.max(record.work[1].expansions),
                        metadata.max(record.work[2].expansions),
                        writes.max(record.logical_writes),
                    ]);
                }
                Ok(())
            })
            .expect("trusted commit work equals the changed frontier at every current size");
        let [current, retained, metadata, writes] = peak.get();
        println!(
            "SW-01 keys={key_count} history={HISTORY} cases={CASES} \
             min_deactivate_first_excess={} peak_expansions={current}/{retained}/{metadata} \
             peak_logical_writes={writes}",
            minimum.get()
        );
        minima.push(minimum.get());
    }
    assert!(
        minima.windows(2).all(|pair| pair[0] < pair[1]),
        "the deactivate-first excess does not grow with the current state: {minima:?}"
    );
}

#[test]
fn trusted_commit_work_is_unchanged_by_geometric_history_growth() {
    const HISTORIES: [usize; 4] = [16, 64, 256, 1024];
    const KEY_COUNT: usize = 16;
    const COMMITS: usize = 3;
    let writers = [RetentionFormat::V2, RetentionFormat::V1];
    let mut semantic = Vec::with_capacity(writers.len() * HISTORIES.len());
    let mut written = [Vec::with_capacity(HISTORIES.len()), Vec::with_capacity(HISTORIES.len())];
    for (writer, bytes) in writers.into_iter().zip(&mut written) {
        for history in HISTORIES {
            let fixture = ScalingFixture::build(writer, KEY_COUNT, history);
            let directory = trial_scratch("trusted-scaling-history-");
            let mut chain = ScalingChain::open(&fixture, directory.path().to_path_buf(), limits());
            let records = (0..COMMITS)
                .map(|commit| {
                    let planned = chain.plan(&[(commit, 2 + count(commit))]);
                    chain
                        .commit(planned, &mut Unprobed)
                        .unwrap_or_else(|failure| {
                            panic!("{writer:?} history {history} commit {commit}: {failure}")
                        })
                })
                .collect::<Vec<_>>();
            semantic.push(
                records
                    .iter()
                    .map(|record| [record.work[0], record.work[1]])
                    .collect::<Vec<_>>(),
            );
            bytes.push(
                records
                    .iter()
                    .map(|record| record.metadata_bytes)
                    .collect::<Vec<_>>(),
            );
        }
    }
    assert!(
        semantic.windows(2).all(|pair| pair[0] == pair[1]),
        "the current or retained trial work changed with the history: {semantic:?}"
    );
    for ((history, radix), flat) in HISTORIES.into_iter().zip(&written[0]).zip(&written[1]) {
        println!("SW-02 history={history} v1_metadata_bytes={flat:?} v2_metadata_bytes={radix:?}");
        if history >= 64 {
            assert!(
                flat.iter().zip(radix).all(|(flat, radix)| flat > radix),
                "the flat manifest did not outgrow the radix step at history {history}: {flat:?} \
                 versus {radix:?}"
            );
        }
    }
}

#[test]
fn last_reference_deletion_expands_exactly_the_deleted_closure() {
    const KEY_COUNT: usize = 256;
    const GROUPS: usize = 16;
    const GROUP_WIDTHS: [usize; 3] = [1, 4, 16];
    const COMMITS: usize = 3;
    for width in GROUP_WIDTHS {
        let fixture =
            ScalingFixture::build(RetentionFormat::V2, KEY_COUNT + GROUPS * width, HISTORY);
        scaling_runner(CASES)
            .run(&(0..GROUPS, 0..GROUPS / 2), |(offset, half)| {
                let stride = 2 * half + 1;
                let directory = trial_scratch("trusted-scaling-deletion-");
                let mut chain =
                    ScalingChain::open(&fixture, directory.path().to_path_buf(), limits());
                for commit in 0..COMMITS {
                    let group = (offset + commit * stride) % GROUPS;
                    let first = KEY_COUNT + group * width;
                    let edits = (first..first + width)
                        .map(|index| (index, 0))
                        .collect::<Vec<_>>();
                    let planned = chain.plan(&edits);
                    let base = chain.state.manifest();
                    let deleted = deleted_closure(
                        &chain.closure,
                        base,
                        planned.model.data.keys().map(Vec::as_slice),
                    );
                    prop_assert!(
                        deleted
                            .iter()
                            .all(|digest| !planned.closure.contains_key(digest)),
                        "a deleted object stays in the target closure"
                    );
                    let data_root = manifest_roots(&chain.closure, base).data_bag;
                    for key in &fixture.keys[first..first + width] {
                        let path = search_path(&chain.closure, data_root, key);
                        let leaf = path
                            .last()
                            .ok_or_else(|| TestCaseError::fail("a search path is empty"))?;
                        prop_assert!(
                            deleted.contains(leaf),
                            "a deleted key's leaf survives the deletion"
                        );
                    }
                    prop_assert!(
                        (count(width)..=count(2 * width - 1)).contains(&count(deleted.len())),
                        "{} objects lose their last reference for {} deleted keys",
                        deleted.len(),
                        width
                    );
                    let new_path = planned
                        .closure
                        .keys()
                        .filter(|digest| !chain.closure.contains_key(*digest))
                        .copied()
                        .collect::<BTreeSet<_>>();
                    let retained = planned
                        .closure
                        .keys()
                        .filter(|digest| !chain.union.contains(*digest))
                        .copied()
                        .collect::<BTreeSet<_>>();
                    prop_assert_eq!(
                        &retained,
                        &new_path,
                        "the retained frontier is not the new path"
                    );
                    let departed = chain
                        .closure
                        .keys()
                        .filter(|digest| !planned.closure.contains_key(*digest))
                        .copied()
                        .collect::<Vec<_>>();
                    chain.commit(planned, &mut Unprobed)?;
                    let ledger = chain.core.retained_closure();
                    for digest in departed {
                        prop_assert_eq!(ledger.count(ClosureDomainV1::Current, digest), 0);
                        prop_assert!(
                            ledger.count(ClosureDomainV1::Retained, digest) >= 1,
                            "a departed object lost its retained count"
                        );
                    }
                }
                Ok(())
            })
            .expect("a last-reference deletion expands exactly the deleted closure");
    }
}
