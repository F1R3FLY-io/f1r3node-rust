use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::support::{TestSemanticProfile, admit, encode};
use super::transaction_sealing::base_state;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::hot_store::{
    HotStore, HotStoreInstances, HotStoreState, mutation_batch_from_observation,
    stage_pending_continuation_sources,
};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::ordered_receive::{PreparedContinuationRecord, RestingReceiveVisit};
use crate::rspace::persistence::{
    AcceleratorArtifact, AcceleratorArtifactKind, AcceleratorManifestV1, AcceleratorManifestView,
    CanonicalObjectStore, DagTransactionLimits, ImmutableObjectStore, MutationBatchViewV1,
    ObjectDigest, ObjectRefV1, OpenDagTransaction, SealedObject, SemanticStateManifestView,
    SemanticValidationLimits, StateVersionV1, VerifiedDagState,
};
use crate::rspace::rho_path::{ReceiveChannelQuery, RhoPathEncode};

#[path = "../support/receive_accelerator_adoption.rs"]
mod adoption;

use self::adoption::{Adoption, persisted_accelerator_adoption};

type ChannelImage = Vec<u8>;
type GroupKey = Vec<ChannelImage>;
type Relation = BTreeMap<ChannelImage, BTreeSet<DerivedOccurrence>>;
type StringHotStore = Box<dyn HotStore<String, String, String, String>>;

const LEFT: &str = "manifest-binding-left";
const RIGHT: &str = "manifest-binding-right";
const BASE_GENERATION: u64 = 23;
const ACCELERATOR_EPOCH: u64 = 8010;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DerivedOccurrence {
    group: GroupKey,
    positions: Vec<u64>,
    identity: Vec<u8>,
    source: Vec<u8>,
}

struct PlannedReceive {
    channels: Vec<String>,
    continuation: WaitingContinuation<String, String>,
}

struct SealedState {
    hot: StringHotStore,
    version: StateVersionV1,
    objects: Vec<SealedObject>,
}

struct AdoptionCase<'a> {
    name: &'static str,
    accelerator: ObjectRefV1,
    persisted: &'a Relation,
    current: &'a SealedState,
    current_receives: &'a [PlannedReceive],
    expected: Adoption,
}

struct EmptyHistory;

impl HistoryReaderBase<String, String, String, String> for EmptyHistory {
    fn get_data_canonical(&self, _key: &String) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_canonical(
        &self,
        _key: &[String],
    ) -> Vec<WaitingContinuation<String, String>> {
        Vec::new()
    }

    fn get_joins_canonical(&self, _key: &String) -> Vec<Vec<String>> { Vec::new() }
}

fn planned_receive(
    channels: &[&str],
    body: &str,
    persist: bool,
    peeks: BTreeSet<i32>,
) -> PlannedReceive {
    let channels: Vec<String> = channels
        .iter()
        .map(|channel| (*channel).to_owned())
        .collect();
    let patterns: Vec<String> = (0..channels.len())
        .map(|position| format!("pattern-{position}"))
        .collect();
    let continuation =
        WaitingContinuation::create(&channels, &patterns, &body.to_owned(), persist, peeks);
    PlannedReceive {
        channels,
        continuation,
    }
}

fn first_state() -> Vec<PlannedReceive> {
    vec![
        planned_receive(&[LEFT, RIGHT, LEFT], "first-body", false, BTreeSet::new()),
        planned_receive(&[LEFT, RIGHT, LEFT], "second-body", true, BTreeSet::from([0])),
    ]
}

fn second_state() -> Vec<PlannedReceive> {
    vec![
        planned_receive(&[LEFT, RIGHT, LEFT], "first-body", false, BTreeSet::new()),
        planned_receive(&[RIGHT], "single-body", false, BTreeSet::new()),
    ]
}

fn group_key(channels: &[String]) -> GroupKey {
    channels
        .iter()
        .map(|channel| channel.rho_path_identity())
        .collect()
}

fn planned_groups(receives: &[PlannedReceive]) -> Vec<Vec<String>> {
    let distinct: BTreeSet<Vec<String>> = receives
        .iter()
        .map(|receive| receive.channels.clone())
        .collect();
    distinct.into_iter().collect()
}

fn position_list(group: &[ChannelImage], channel: &[u8]) -> Vec<u64> {
    let mut positions = Vec::with_capacity(group.len());
    for (index, image) in group.iter().enumerate() {
        if image.as_slice() == channel {
            positions.push(u64::try_from(index).expect("a receive position fits u64"));
        }
    }
    positions
}

fn expected_relation(receives: &[PlannedReceive]) -> Relation {
    let mut relation = Relation::new();
    for receive in receives {
        let group = group_key(&receive.channels);
        let prepared = PreparedContinuationRecord::encode_waiting(&receive.continuation);
        let record = prepared.record_view();
        let distinct: BTreeSet<ChannelImage> = group.iter().cloned().collect();
        for channel in distinct {
            let positions = position_list(&group, &channel);
            relation
                .entry(channel)
                .or_default()
                .insert(DerivedOccurrence {
                    group: group.clone(),
                    positions,
                    identity: record.identity().to_vec(),
                    source: record.source_suffix().to_vec(),
                });
        }
    }
    relation
}

fn relation_of(
    hot: &dyn HotStore<String, String, String, String>,
    groups: &[Vec<String>],
) -> Relation {
    let mut relation = Relation::new();
    for channels in groups {
        let group = group_key(channels);
        let distinct: BTreeSet<ChannelImage> = group.iter().cloned().collect();
        let visit = hot.try_visit_resting_receive_views(channels, &mut |_, admission| {
            let view = admission.expect("a resting continuation admits its ordered view");
            let record = view.record();
            for channel in &distinct {
                let positions: Vec<u64> = view
                    .positions(ReceiveChannelQuery::LegacyV1(channel))
                    .expect("a legacy continuation view answers a legacy channel query")
                    .map(|position| position.get())
                    .collect();
                relation
                    .entry(channel.clone())
                    .or_default()
                    .insert(DerivedOccurrence {
                        group: group.clone(),
                        positions,
                        identity: record.identity().to_vec(),
                        source: record.source_suffix().to_vec(),
                    });
            }
        });
        let RestingReceiveVisit::Visited(_) = visit else {
            panic!("every planned group is loaded in the hot store");
        };
    }
    relation
}

fn loaded_hot_store(receives: &[PlannedReceive]) -> StringHotStore {
    let hot =
        HotStoreInstances::create_from_hs_and_hr(HotStoreState::default(), Box::new(EmptyHistory));
    let mut joined: BTreeSet<(String, Vec<String>)> = BTreeSet::new();
    for receive in receives {
        assert_eq!(
            hot.put_continuation(&receive.channels, receive.continuation.clone()),
            Some(true),
            "every planned receive is a distinct record"
        );
        for channel in &receive.channels {
            if joined.insert((channel.clone(), receive.channels.clone())) {
                assert_eq!(
                    hot.put_join(channel, &receive.channels),
                    Some(()),
                    "the join membership of every distinct channel is recorded"
                );
            }
        }
    }
    hot
}

fn seal_state(
    store: &CanonicalObjectStore,
    base: StateVersionV1,
    receives: &[PlannedReceive],
) -> SealedState {
    let hot = loaded_hot_store(receives);
    let observation = hot
        .pending_mutation_observation()
        .expect("an in-memory hot store observes its journal");
    let batch = mutation_batch_from_observation(base, &observation)
        .expect("the live journal forms one mutation batch");
    let bytes = encode(
        &batch
            .plan()
            .expect("the mutation batch plans its canonical form"),
    );
    let view = MutationBatchViewV1::parse(&bytes).expect("the planned mutation batch parses");
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .expect("the base state is fully admitted");
    let mut transaction = OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED)
        .expect("the transaction opens on its verified base");
    stage_pending_continuation_sources(&mut transaction, &observation.pending)
        .expect("the journal stages every continuation source");
    let sealed = transaction
        .seal()
        .expect("a coherent continuation state seals");
    assert_eq!(sealed.receipt().base(), base, "the transition starts at the shared base");
    let version = sealed.receipt().target();
    let objects = sealed.objects().to_vec();
    SealedState {
        hot,
        version,
        objects,
    }
}

fn publish(store: &CanonicalObjectStore, sealed: &SealedState) {
    for object in &sealed.objects {
        let (reference, _) = store
            .admit(Arc::from(object.canonical_bytes()))
            .expect("a sealed object is canonical");
        assert_eq!(
            reference,
            object.reference(),
            "the store addresses a sealed object by its sealed reference"
        );
    }
    let _ = VerifiedDagState::<_, TestSemanticProfile>::admit(
        store,
        sealed.version,
        SemanticValidationLimits::unbounded(),
    )
    .expect("the published successor is a fully valid semantic state");
}

fn assert_stored(store: &CanonicalObjectStore, reference: ObjectRefV1, object: &str) {
    let stored = store
        .get_bounded(&reference.digest(), reference.encoded_len())
        .expect("the in-memory object store reads");
    assert!(stored.is_some(), "the {object} is a stored object");
}

fn stored_manifest(
    store: &CanonicalObjectStore,
    manifest: ObjectRefV1,
) -> SemanticStateManifestView {
    let bytes = store
        .get_bounded(&manifest.digest(), manifest.encoded_len())
        .expect("the in-memory object store reads")
        .expect("the sealed semantic manifest is stored");
    let view = SemanticStateManifestView::parse(&bytes).expect("a stored semantic manifest parses");
    assert_eq!(
        view.object_ref(),
        manifest,
        "the stored semantic manifest carries its sealed reference"
    );
    view
}

fn bind_accelerator(
    store: &CanonicalObjectStore,
    semantic_manifest: ObjectRefV1,
    artifact: ObjectRefV1,
) -> ObjectRefV1 {
    let manifest =
        AcceleratorManifestV1::new(semantic_manifest, ACCELERATOR_EPOCH, [AcceleratorArtifact {
            kind: AcceleratorArtifactKind::ChannelReceives,
            object: artifact,
        }])
        .expect("one channel-receive artifact is strictly ordered");
    let encoded = encode(&manifest);
    let parsed =
        AcceleratorManifestView::parse(&encoded).expect("an encoded accelerator manifest parses");
    assert_eq!(
        parsed.object_ref(),
        manifest.object_ref(),
        "parsing preserves the accelerator manifest reference"
    );
    assert_eq!(
        parsed.semantic_manifest(),
        semantic_manifest,
        "parsing preserves the bound semantic manifest"
    );
    assert_eq!(parsed.artifact_count(), 1, "the accelerator manifest carries its one artifact");
    admit(store, &manifest)
}

fn bound_semantic_manifest(store: &CanonicalObjectStore, accelerator: ObjectRefV1) -> ObjectRefV1 {
    let bytes = store
        .get_bounded(&accelerator.digest(), accelerator.encoded_len())
        .expect("the in-memory object store reads")
        .expect("the bound accelerator manifest is stored");
    let view =
        AcceleratorManifestView::parse(&bytes).expect("a stored accelerator manifest parses");
    assert_eq!(
        view.object_ref(),
        accelerator,
        "the stored accelerator manifest carries its admitted reference"
    );
    view.semantic_manifest()
}

fn resolved_relation(adoption: Adoption, case: &AdoptionCase<'_>) -> Relation {
    match adoption {
        Adoption::Adopted => case.persisted.clone(),
        Adoption::Foreign => {
            relation_of(&*case.current.hot, &planned_groups(case.current_receives))
        }
    }
}

#[test]
fn persisted_accelerator_adoption_requires_the_bound_semantic_manifest() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, BASE_GENERATION, None);
    let first_receives = first_state();
    let mut reversed_receives = first_state();
    reversed_receives.reverse();
    let second_receives = second_state();
    let first = seal_state(&store, base, &first_receives);
    let reversed = seal_state(&store, base, &reversed_receives);
    let second = seal_state(&store, base, &second_receives);
    for sealed in [&first, &reversed, &second] {
        publish(&store, sealed);
    }

    let first_manifest = first.version.manifest();
    let reversed_manifest = reversed.version.manifest();
    let second_manifest = second.version.manifest();
    assert_eq!(
        reversed_manifest, first_manifest,
        "inserting the same receives in another order seals the same semantic manifest"
    );
    assert_ne!(
        second_manifest, first_manifest,
        "different continuation states seal different semantic manifests"
    );
    let first_roots = stored_manifest(&store, first_manifest).roots();
    let second_roots = stored_manifest(&store, second_manifest).roots();
    assert_ne!(
        first_roots.continuation_map, second_roots.continuation_map,
        "the two states differ in their continuation maps"
    );
    assert_stored(&store, first_roots.continuation_map, "first continuation map");
    assert_stored(&store, second_roots.continuation_map, "second continuation map");

    let first_relation = relation_of(&*first.hot, &planned_groups(&first_receives));
    let reversed_relation = relation_of(&*reversed.hot, &planned_groups(&reversed_receives));
    let second_relation = relation_of(&*second.hot, &planned_groups(&second_receives));
    assert_eq!(
        first_relation,
        expected_relation(&first_receives),
        "the first store derives the planned relation"
    );
    assert_eq!(
        reversed_relation, first_relation,
        "insertion order does not change the derived relation"
    );
    assert_eq!(
        second_relation,
        expected_relation(&second_receives),
        "the second store derives the planned relation"
    );
    assert_ne!(second_relation, first_relation, "the two states derive different relations");

    let first_accelerator = bind_accelerator(&store, first_manifest, first_roots.continuation_map);
    let second_accelerator =
        bind_accelerator(&store, second_manifest, second_roots.continuation_map);
    let forged_length = ObjectRefV1::new(
        first_manifest.digest(),
        first_manifest
            .encoded_len()
            .checked_add(1)
            .expect("a forged manifest length fits u64"),
    )
    .expect("a lengthened reference keeps its envelope");
    let mut flipped = *first_manifest.digest().as_array();
    let [leading, ..] = &mut flipped;
    *leading ^= 0x01;
    let flipped_digest =
        ObjectRefV1::new(ObjectDigest::from_array(flipped), first_manifest.encoded_len())
            .expect("a flipped digest keeps the manifest length");
    let forged_length_accelerator =
        bind_accelerator(&store, forged_length, first_roots.continuation_map);
    let flipped_digest_accelerator =
        bind_accelerator(&store, flipped_digest, first_roots.continuation_map);

    let cases = [
        AdoptionCase {
            name: "an accelerator bound to an identical state sealed in another order",
            accelerator: first_accelerator,
            persisted: &first_relation,
            current: &reversed,
            current_receives: &reversed_receives,
            expected: Adoption::Adopted,
        },
        AdoptionCase {
            name: "an accelerator bound to its own state",
            accelerator: second_accelerator,
            persisted: &second_relation,
            current: &second,
            current_receives: &second_receives,
            expected: Adoption::Adopted,
        },
        AdoptionCase {
            name: "the first state's accelerator over the second state",
            accelerator: first_accelerator,
            persisted: &first_relation,
            current: &second,
            current_receives: &second_receives,
            expected: Adoption::Foreign,
        },
        AdoptionCase {
            name: "the second state's accelerator over the first state",
            accelerator: second_accelerator,
            persisted: &second_relation,
            current: &first,
            current_receives: &first_receives,
            expected: Adoption::Foreign,
        },
        AdoptionCase {
            name: "an accelerator whose bound manifest length is forged",
            accelerator: forged_length_accelerator,
            persisted: &first_relation,
            current: &first,
            current_receives: &first_receives,
            expected: Adoption::Foreign,
        },
        AdoptionCase {
            name: "an accelerator whose bound manifest digest is flipped",
            accelerator: flipped_digest_accelerator,
            persisted: &first_relation,
            current: &first,
            current_receives: &first_receives,
            expected: Adoption::Foreign,
        },
    ];
    let mut foreign = 0usize;
    for case in &cases {
        let name = case.name;
        let bound = bound_semantic_manifest(&store, case.accelerator);
        let current_manifest = case.current.version.manifest();
        let adoption = persisted_accelerator_adoption(&bound, &current_manifest);
        assert_eq!(
            adoption, case.expected,
            "{name}: adoption requires the bound semantic manifest to equal the current one"
        );
        assert_eq!(
            adoption == Adoption::Adopted,
            bound == current_manifest,
            "{name}: adoption is exactly digest and length equality"
        );
        assert_eq!(
            resolved_relation(adoption, case),
            expected_relation(case.current_receives),
            "{name}: the adopted or rebuilt relation equals the current derivation"
        );
        match adoption {
            Adoption::Adopted => {}
            Adoption::Foreign => {
                foreign = foreign
                    .checked_add(1)
                    .expect("the foreign case count fits usize");
            }
        }
    }
    assert_eq!(foreign, 4, "four bindings are foreign to the current manifest");
    assert_ne!(
        first_relation, second_relation,
        "adopting a foreign accelerator would answer from the wrong relation"
    );
}
