use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

use super::canonical_rebuild_oracle::{Entries, rebuild};
use super::support::{TestSemanticProfile, encode, manifest, nonzero};
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::{
    CanonicalBlob, CanonicalBlobKind, CanonicalEncode, CanonicalObjectStore, CanonicalObjectView,
    DagTransactionLimits, EndpointValue, ImmutableObjectStore, InheritedBoundaryKind,
    MutableObjectStore, MutationBatchViewV1, ObjectAdmission, ObjectRefV1, OpenDagTransaction,
    RSpaceMutationBatchV1, SemanticNamespace, SemanticRoots, SemanticValidationLimits,
    StateVersionV1, VerifiedDagState,
};
use crate::rspace::rho_path::RhoPathV1;
use crate::rspace::serializers::serializers::{
    encode_continuation_identity, encode_continuation_source_into, encode_join_record,
};

const DATA_CHANNELS: [&str; 3] = ["chan-a", "chan-b", "chan-c"];
const DATUM_TEXTS: [&str; 3] = ["datum-0", "datum-pair-a", "datum-pair-b"];
const DATA_HOLDER: &str = "holder";
pub(super) const DATA_SLOTS: usize = DATA_CHANNELS.len() * DATUM_TEXTS.len();
pub(super) const CONTINUATION_GROUPS: [&[&str]; 4] =
    [&["alpha"], &["beta"], &["alpha", "beta"], &["gamma", "alpha"]];
pub(super) const CONTINUATION_BODIES: [&str; 3] = ["body-0", "body-one", "body-the-second"];
pub(super) const CONTINUATION_SLOTS: usize = CONTINUATION_GROUPS.len() * CONTINUATION_BODIES.len();

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ModelState {
    pub(super) data: BTreeMap<Vec<u8>, u64>,
    pub(super) continuations: BTreeMap<Vec<u8>, Vec<u8>>,
    pub(super) joins: BTreeMap<Vec<u8>, u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Slots {
    pub(super) data: [u8; DATA_SLOTS],
    pub(super) continuations: [u8; CONTINUATION_SLOTS],
}

#[derive(Clone, Copy, Debug)]
pub(super) enum SlotEdit {
    Datum { slot: usize, value: u8 },
    Continuation { slot: usize, value: u8 },
    CopyChannel { source: usize, target: usize },
    CopyGroup { source: usize, target: usize },
}

impl SlotEdit {
    pub(super) fn apply(self, slots: &mut Slots) {
        match self {
            Self::Datum { slot, value } => slots.data[slot] = value,
            Self::Continuation { slot, value } => slots.continuations[slot] = value,
            Self::CopyChannel { source, target } => {
                let width = DATUM_TEXTS.len();
                slots
                    .data
                    .copy_within(source * width..(source + 1) * width, target * width);
            }
            Self::CopyGroup { source, target } => {
                let width = CONTINUATION_BODIES.len();
                slots
                    .continuations
                    .copy_within(source * width..(source + 1) * width, target * width);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Occurrence {
    namespace: SemanticNamespace,
    kind: InheritedBoundaryKind,
    path: Vec<u8>,
    reference: ObjectRefV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WalkOrder {
    StagedFirst,
    CertificateFirst,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WalkOutcome {
    Accepted {
        visits: usize,
        reached: BTreeSet<ObjectRefV1>,
        coincident: usize,
    },
    MissingNode(ObjectRefV1),
    MissingContent(ObjectRefV1),
    WrongKind(ObjectRefV1),
    StagedOutsideClosure,
}

struct SealedTransition {
    coincident: usize,
    staged_first: WalkOutcome,
}

#[derive(Clone, Copy, Debug, Default)]
struct ClosureCoverage {
    transitions: usize,
    coincident: usize,
    staged_first_missing_node: usize,
    staged_first_missing_content: usize,
}

fn runner(cases: u32) -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
}

pub(super) const fn data_slot(channel: usize, text: usize) -> usize {
    channel * DATUM_TEXTS.len() + text
}

pub(super) const fn continuation_slot(group: usize, body: usize) -> usize {
    group * CONTINUATION_BODIES.len() + body
}

fn datum_key(channel: &str, text: &str) -> Vec<u8> {
    RSpaceDataIndex::datum_path(
        &channel.to_owned(),
        &Datum::create(&DATA_HOLDER.to_owned(), text.to_owned(), false),
    )
}

pub(super) fn continuation(group: &[&str], body: &str, independent: bool) -> (Vec<u8>, Vec<u8>) {
    let channels = group
        .iter()
        .map(|channel| (*channel).to_owned())
        .collect::<Vec<_>>();
    let patterns = group
        .iter()
        .map(|channel| format!("pattern-{channel}"))
        .collect::<Vec<_>>();
    let mut waiting = WaitingContinuation::create(
        &channels,
        &patterns,
        &body.to_owned(),
        false,
        BTreeSet::from([0]),
    );
    if independent {
        waiting.source = WaitingContinuation::create(
            &vec![format!("source-{body}")],
            &vec!["source-pattern".to_owned()],
            &"source-body".to_owned(),
            true,
            BTreeSet::new(),
        )
        .source;
    }
    let key = RhoPathV1::collection_record_path(
        &RhoPathV1::continuation_prefix(&channels),
        &encode_continuation_identity(&waiting),
    );
    let mut payload = Vec::new();
    encode_continuation_source_into(&waiting, &mut payload);
    (key, payload)
}

pub(super) fn join_key(channel: &str, group: &[&str]) -> Vec<u8> {
    let channels = group
        .iter()
        .map(|member| (*member).to_owned())
        .collect::<Vec<_>>();
    let mut key = RhoPathV1::join_prefix(&channel.to_owned());
    key.extend_from_slice(&encode_join_record(&channels));
    key
}

fn source_blob(payload: &[u8]) -> CanonicalBlob {
    CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, payload)
        .expect("a continuation source payload forms a canonical blob")
}

pub(super) fn model_state(slots: &Slots) -> ModelState {
    let mut state = ModelState::default();
    for (slot, value) in slots.data.iter().enumerate() {
        let multiplicity = match *value {
            0 => continue,
            multiplicity => u64::from(multiplicity),
        };
        let channel = DATA_CHANNELS[slot / DATUM_TEXTS.len()];
        let text = DATUM_TEXTS[slot % DATUM_TEXTS.len()];
        state.data.insert(datum_key(channel, text), multiplicity);
    }
    for (group_index, group) in CONTINUATION_GROUPS.iter().enumerate() {
        let mut waiting = 0_u64;
        for (body_index, body) in CONTINUATION_BODIES.iter().enumerate() {
            let independent =
                match slots.continuations[group_index * CONTINUATION_BODIES.len() + body_index] {
                    0 => continue,
                    1 => false,
                    _ => true,
                };
            let (key, payload) = continuation(group, body, independent);
            state.continuations.insert(key, payload);
            waiting = waiting
                .checked_add(1)
                .expect("a group holds at most three continuations");
        }
        if waiting == 0 {
            continue;
        }
        for channel in group.iter() {
            state.joins.insert(join_key(channel, group), waiting);
        }
    }
    state
}

fn admit_object<T: CanonicalEncode>(store: &CanonicalObjectStore, value: &T) -> ObjectRefV1 {
    let (reference, admission) = store
        .admit(encode(value).into())
        .expect("a model object is canonical");
    assert!(matches!(admission, ObjectAdmission::Inserted | ObjectAdmission::AlreadyPresent));
    reference
}

pub(super) fn install(store: &CanonicalObjectStore, state: &ModelState) -> ObjectRefV1 {
    let multiplicities = |entries: &BTreeMap<Vec<u8>, u64>| {
        entries
            .iter()
            .map(|(key, multiplicity)| {
                (key.clone(), EndpointValue::Multiplicity(nonzero(*multiplicity)))
            })
            .collect::<Entries>()
    };
    let continuations = state
        .continuations
        .iter()
        .map(|(key, payload)| {
            (key.clone(), EndpointValue::Content(admit_object(store, &source_blob(payload))))
        })
        .collect::<Entries>();
    admit_object(
        store,
        &manifest(SemanticRoots {
            data_bag: rebuild(store, &multiplicities(&state.data)),
            continuation_map: rebuild(store, &continuations),
            join_bag: rebuild(store, &multiplicities(&state.joins)),
        }),
    )
}

fn count_transitions<'state>(
    from: &'state BTreeMap<Vec<u8>, u64>,
    to: &'state BTreeMap<Vec<u8>, u64>,
) -> impl Iterator<Item = (&'state [u8], u64, u64)> {
    from.keys()
        .chain(to.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(move |key| {
            let before = from.get(key).copied().unwrap_or(0);
            let after = to.get(key).copied().unwrap_or(0);
            (before != after).then_some((key.as_slice(), before, after))
        })
}

pub(super) fn transition_batch(
    base: StateVersionV1,
    from: &ModelState,
    to: &ModelState,
) -> Option<Vec<u8>> {
    let mut batch =
        RSpaceMutationBatchV1::new(base).expect("the model base has a successor generation");
    for (key, before, after) in count_transitions(&from.data, &to.data) {
        batch
            .record_data(key, before, after)
            .expect("a model data transition is a valid mutation");
    }
    for key in from
        .continuations
        .keys()
        .chain(to.continuations.keys())
        .collect::<BTreeSet<_>>()
    {
        let before = from
            .continuations
            .get(key)
            .map(|payload| source_blob(payload).object_ref());
        let after = to
            .continuations
            .get(key)
            .map(|payload| source_blob(payload).object_ref());
        if before != after {
            batch
                .record_continuation(key, before, after)
                .expect("a model continuation transition is a valid mutation");
        }
    }
    for (key, before, after) in count_transitions(&from.joins, &to.joins) {
        batch
            .record_join(key, before, after)
            .expect("a model join transition is a valid mutation");
    }
    (!batch.is_empty()).then(|| encode(&batch.plan().expect("a nonempty model batch plans")))
}

pub(super) fn staged_sources(from: &ModelState, to: &ModelState) -> Vec<Arc<[u8]>> {
    let changed = to
        .continuations
        .iter()
        .filter(|(key, payload)| from.continuations.get(*key) != Some(*payload))
        .map(|(_, payload)| payload.as_slice())
        .collect::<BTreeSet<_>>();
    let mut staged = Vec::with_capacity(changed.len());
    staged.extend(
        changed
            .into_iter()
            .map(|payload| Arc::<[u8]>::from(encode(&source_blob(payload)))),
    );
    staged
}

fn read_object(store: &CanonicalObjectStore, reference: ObjectRefV1) -> Arc<[u8]> {
    store
        .get_bounded(&reference.digest(), reference.encoded_len())
        .expect("the base store is readable")
        .expect("every object of the admitted base is present")
}

fn base_occurrences(
    store: &CanonicalObjectStore,
    manifest_ref: ObjectRefV1,
) -> HashSet<Occurrence> {
    let bytes = read_object(store, manifest_ref);
    let CanonicalObjectView::SemanticManifest(view) =
        CanonicalObjectView::parse(&bytes).expect("the base manifest parses")
    else {
        panic!("the base root is a semantic manifest");
    };
    let roots = view.roots().iter();
    let mut work = Vec::with_capacity(roots.len());
    work.extend(
        roots
            .into_iter()
            .map(|(namespace, root)| (namespace, Vec::new(), root)),
    );
    let mut occurrences = HashSet::new();
    while let Some((namespace, path, reference)) = work.pop() {
        let bytes = read_object(store, reference);
        let CanonicalObjectView::DagNode(node) =
            CanonicalObjectView::parse(&bytes).expect("a base node parses")
        else {
            panic!("every base occurrence below a manifest root is a node");
        };
        let mut node_path = Vec::with_capacity(path.len() + node.prefix().len());
        node_path.extend_from_slice(&path);
        node_path.extend_from_slice(node.prefix());
        if let EndpointValue::Content(content) = node.endpoint() {
            occurrences.insert(Occurrence {
                namespace,
                kind: InheritedBoundaryKind::ContentAtEndpoint,
                path: node_path.clone(),
                reference: content,
            });
        }
        for child in node.children().iter() {
            let mut child_path = Vec::with_capacity(node_path.len() + 1);
            child_path.extend_from_slice(&node_path);
            child_path.push(child.edge);
            work.push((namespace, child_path, child.target));
        }
        occurrences.insert(Occurrence {
            namespace,
            kind: InheritedBoundaryKind::NodeEntry,
            path,
            reference,
        });
    }
    occurrences
}

fn closure_walk(
    order: WalkOrder,
    staged: &HashMap<ObjectRefV1, &[u8]>,
    base: &HashSet<Occurrence>,
    manifest_ref: ObjectRefV1,
) -> WalkOutcome {
    let Some(manifest_bytes) = staged.get(&manifest_ref) else {
        return WalkOutcome::MissingNode(manifest_ref);
    };
    let Ok(CanonicalObjectView::SemanticManifest(view)) =
        CanonicalObjectView::parse(manifest_bytes)
    else {
        return WalkOutcome::WrongKind(manifest_ref);
    };
    let mut reached = BTreeSet::from([manifest_ref]);
    let roots = view.roots().iter();
    let mut work = Vec::with_capacity(roots.len());
    work.extend(roots.into_iter().rev().map(|(namespace, root)| {
        (
            Occurrence {
                namespace,
                kind: InheritedBoundaryKind::NodeEntry,
                path: Vec::new(),
                reference: root,
            },
            false,
        )
    }));
    let mut visits = 1_usize;
    let mut coincident = 0_usize;
    while let Some((occurrence, parent_inherited)) = work.pop() {
        visits = visits
            .checked_add(1)
            .expect("a finite walk counts its visits");
        let inherited = base.contains(&occurrence);
        let certified = inherited && !parent_inherited;
        let staged_bytes = staged.get(&occurrence.reference).copied();
        if certified && staged_bytes.is_some() {
            coincident = coincident
                .checked_add(1)
                .expect("a finite walk counts its coincidences");
        }
        let bytes = match (order, certified, staged_bytes) {
            (WalkOrder::CertificateFirst, true, _) | (WalkOrder::StagedFirst, true, None) => {
                continue;
            }
            (_, _, Some(bytes)) => bytes,
            (_, false, None) => {
                return match occurrence.kind {
                    InheritedBoundaryKind::NodeEntry => {
                        WalkOutcome::MissingNode(occurrence.reference)
                    }
                    InheritedBoundaryKind::ContentAtEndpoint => {
                        WalkOutcome::MissingContent(occurrence.reference)
                    }
                };
            }
        };
        reached.insert(occurrence.reference);
        match (occurrence.kind, CanonicalObjectView::parse(bytes)) {
            (InheritedBoundaryKind::ContentAtEndpoint, Ok(CanonicalObjectView::Blob(_))) => {}
            (InheritedBoundaryKind::NodeEntry, Ok(CanonicalObjectView::DagNode(node))) => {
                let mut node_path = occurrence.path;
                node_path.extend_from_slice(node.prefix());
                if let EndpointValue::Content(content) = node.endpoint() {
                    work.push((
                        Occurrence {
                            namespace: occurrence.namespace,
                            kind: InheritedBoundaryKind::ContentAtEndpoint,
                            path: node_path.clone(),
                            reference: content,
                        },
                        inherited,
                    ));
                }
                for child in node.children().iter() {
                    let mut child_path = Vec::with_capacity(node_path.len() + 1);
                    child_path.extend_from_slice(&node_path);
                    child_path.push(child.edge);
                    work.push((
                        Occurrence {
                            namespace: occurrence.namespace,
                            kind: InheritedBoundaryKind::NodeEntry,
                            path: child_path,
                            reference: child.target,
                        },
                        inherited,
                    ));
                }
            }
            _ => return WalkOutcome::WrongKind(occurrence.reference),
        }
    }
    match staged.keys().all(|reference| reached.contains(reference)) {
        true => WalkOutcome::Accepted {
            visits,
            reached,
            coincident,
        },
        false => WalkOutcome::StagedOutsideClosure,
    }
}

fn check_transition(
    from: &ModelState,
    to: &ModelState,
) -> Result<Option<SealedTransition>, TestCaseError> {
    let store = CanonicalObjectStore::new();
    let base = StateVersionV1::new(1, install(&store, from));
    let Some(mutation) = transition_batch(base, from, to) else {
        return Ok(None);
    };
    let view = MutationBatchViewV1::parse(&mutation).expect("a model batch parses");
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .expect("the model base is admitted");
    let mut transaction = OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED)
        .expect("the batch opens over its exact base");
    for source in staged_sources(from, to) {
        transaction
            .stage_continuation_source(source)
            .expect("a changed continuation source stages");
    }
    let sealed = transaction.seal().map_err(|error| {
        TestCaseError::fail(format!("a coherent model transition did not seal: {error:?}"))
    })?;
    let target = sealed.receipt().target();
    prop_assert_eq!(target.manifest(), install(&CanonicalObjectStore::new(), to));

    let mut staged = HashMap::with_capacity(sealed.objects().len());
    staged.extend(
        sealed
            .objects()
            .iter()
            .map(|object| (object.reference(), object.canonical_bytes())),
    );
    let occurrences = base_occurrences(&store, base.manifest());
    let certificate_first =
        closure_walk(WalkOrder::CertificateFirst, &staged, &occurrences, target.manifest());
    let WalkOutcome::Accepted {
        reached,
        coincident,
        ..
    } = &certificate_first
    else {
        return Err(TestCaseError::fail(format!(
            "the certificate-first model refused a sealed delta: {certificate_first:?}"
        )));
    };
    prop_assert_eq!(reached, &staged.keys().copied().collect::<BTreeSet<_>>());
    let coincident = *coincident;
    let staged_first =
        closure_walk(WalkOrder::StagedFirst, &staged, &occurrences, target.manifest());
    prop_assert!(
        matches!(
            staged_first,
            WalkOutcome::Accepted { .. } |
                WalkOutcome::MissingNode(_) |
                WalkOutcome::MissingContent(_)
        ),
        "the staged-first order neither accepted nor missed an object: {:?}",
        staged_first
    );
    if coincident == 0 {
        prop_assert_eq!(&staged_first, &certificate_first);
    }

    for object in sealed.objects() {
        let admission = store
            .put_if_absent_or_equal(object.reference(), object.canonical_bytes_arc())
            .expect("a sealed object is admitted");
        prop_assert!(
            matches!(admission, ObjectAdmission::Inserted | ObjectAdmission::AlreadyPresent),
            "a sealed object was not admitted: {:?}",
            admission
        );
    }
    VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        target,
        SemanticValidationLimits::unbounded(),
    )
    .map_err(|error| {
        TestCaseError::fail(format!("the published target was not admitted: {error:?}"))
    })?;
    Ok(Some(SealedTransition {
        coincident,
        staged_first,
    }))
}

fn slots_strategy() -> impl Strategy<Value = Slots> {
    (
        prop::array::uniform::<_, DATA_SLOTS>(0_u8..3),
        prop::array::uniform::<_, CONTINUATION_SLOTS>(0_u8..3),
    )
        .prop_map(|(data, continuations)| Slots {
            data,
            continuations,
        })
}

fn edit_strategy() -> impl Strategy<Value = SlotEdit> {
    prop_oneof![
        (0..DATA_SLOTS, 0_u8..3).prop_map(|(slot, value)| SlotEdit::Datum { slot, value }),
        (0..CONTINUATION_SLOTS, 0_u8..3)
            .prop_map(|(slot, value)| SlotEdit::Continuation { slot, value }),
        (0..DATA_CHANNELS.len(), 0..DATA_CHANNELS.len())
            .prop_map(|(source, target)| SlotEdit::CopyChannel { source, target }),
        (0..CONTINUATION_GROUPS.len(), 0..CONTINUATION_GROUPS.len())
            .prop_map(|(source, target)| SlotEdit::CopyGroup { source, target }),
    ]
}

pub(super) fn transition_strategy() -> impl Strategy<Value = (Slots, Vec<SlotEdit>)> {
    let uniform = (slots_strategy(), prop::collection::vec(edit_strategy(), 1..5));
    let node_alias = (
        slots_strategy(),
        (1_u8..3, 1_u8..3, 1_u8..3, 1_u8..3),
        prop::collection::vec(edit_strategy(), 0..3),
    )
        .prop_map(|(mut slots, (rebuilt, other, shared, sibling), noise)| {
            for (slot, value) in [
                (data_slot(0, 0), rebuilt),
                (data_slot(0, 1), shared),
                (data_slot(0, 2), sibling),
                (data_slot(1, 0), other),
                (data_slot(1, 1), 3 - shared),
                (data_slot(1, 2), sibling),
            ] {
                slots.data[slot] = value;
            }
            let mut edits = Vec::with_capacity(noise.len() + 2);
            edits.push(SlotEdit::Datum {
                slot: data_slot(1, 1),
                value: shared,
            });
            edits.push(SlotEdit::Datum {
                slot: data_slot(0, 0),
                value: 3 - rebuilt,
            });
            edits.extend(noise);
            (slots, edits)
        });
    let content_alias = (
        slots_strategy(),
        any::<bool>(),
        1_u8..3,
        1_u8..3,
        prop::collection::vec(edit_strategy(), 0..3),
    )
        .prop_map(|(mut slots, insert_sibling, before, inserted, noise)| {
            slots.continuations[continuation_slot(2, 0)..CONTINUATION_SLOTS]
                .copy_from_slice(&[2, 0, before, 2, 0, 0]);
            let mut edits = Vec::with_capacity(noise.len() + 2);
            edits.push(match insert_sibling {
                true => SlotEdit::Continuation {
                    slot: continuation_slot(2, 1),
                    value: inserted,
                },
                false => SlotEdit::Continuation {
                    slot: continuation_slot(2, 2),
                    value: 3 - before,
                },
            });
            edits.push(SlotEdit::Continuation {
                slot: continuation_slot(3, 2),
                value: inserted,
            });
            edits.extend(noise);
            (slots, edits)
        });
    prop_oneof![uniform, node_alias, content_alias]
}

#[test]
fn shrunk_coincident_leaf_transition_seals_and_the_staged_first_control_refuses() {
    let from = Slots {
        data: [0; DATA_SLOTS],
        continuations: [0, 0, 0, 0, 0, 0, 2, 0, 1, 2, 0, 0],
    };
    let to = Slots {
        data: [0; DATA_SLOTS],
        continuations: [0, 0, 0, 0, 0, 0, 2, 0, 2, 2, 0, 1],
    };
    let (_, shared_payload) = continuation(CONTINUATION_GROUPS[2], CONTINUATION_BODIES[0], true);
    assert_eq!(
        shared_payload,
        continuation(CONTINUATION_GROUPS[3], CONTINUATION_BODIES[0], true).1,
        "an independent source does not depend on its group"
    );

    let sealed = check_transition(&model_state(&from), &model_state(&to))
        .expect("the shrunk transition seals to its independent rebuild")
        .expect("the shrunk transition is nonempty");
    assert!(sealed.coincident > 0, "the emitted leaf equals a certified untouched leaf");
    assert_eq!(
        sealed.staged_first,
        WalkOutcome::MissingContent(source_blob(&shared_payload).object_ref()),
        "the staged-first order reproduces the measured refusal"
    );
}

#[test]
fn generated_transitions_with_shared_subtrees_seal_to_the_independent_rebuild() {
    let coverage = Cell::new(ClosureCoverage::default());
    runner(256)
        .run(&transition_strategy(), |(slots, edits)| {
            let mut target = slots;
            for edit in edits {
                edit.apply(&mut target);
            }
            let Some(sealed) = check_transition(&model_state(&slots), &model_state(&target))?
            else {
                return Ok(());
            };
            let mut seen = coverage.get();
            seen.transitions += 1;
            if sealed.coincident > 0 {
                seen.coincident += 1;
            }
            match sealed.staged_first {
                WalkOutcome::MissingNode(_) => seen.staged_first_missing_node += 1,
                WalkOutcome::MissingContent(_) => seen.staged_first_missing_content += 1,
                _ => {}
            }
            coverage.set(seen);
            Ok(())
        })
        .expect("every coherent generated transition seals to its independent rebuild");
    let seen = coverage.get();
    assert!(
        seen.staged_first_missing_node > 0,
        "no generated transition hid a node behind a coincident certificate: {seen:?}"
    );
    assert!(
        seen.staged_first_missing_content > 0,
        "no generated transition hid content behind a coincident certificate: {seen:?}"
    );
}
