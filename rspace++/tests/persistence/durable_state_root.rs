use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::sync::Arc;

use tokio::sync::RwLock;

use super::canonical_rebuild_oracle::{Entries, rebuild};
use super::checkpoint_wal::{
    SyntheticCheckpointSource, fixture as checkpoint_source_fixture, fixture_for_profile,
};
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{
    CheckpointPackImage, checkpoint_image_for_profile, checkpoint_image_from_source, limits,
};
use super::durable_rspace_authority::{
    Profile, Store, fixture, poll_checkpoint_with, publication_limits,
};
use super::support::{
    PersistenceTrialDir, TEST_SEMANTIC_PROFILE_ID, TestSemanticProfile, TracingStore, empty_roots,
    encode, manifest, nonzero, trial_scratch,
};
use super::transaction_sealing::data_batch;
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::errors::RootError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hot_store::{DurableHotStoreBootstrapLimits, HotStoreInstances};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::canonical::OBJECT_DIGEST_LEN;
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, GenerationCheckpointDescriptorV1, OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::closure::RetainedUnionClosureReportV1;
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableStateReadHandleErrorV1, DurableStateRootV1,
    PreparedIndexAppendV1, ResolvedDurableStateV1, recover_read_authority, resolve_retained_state,
};
use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalEncode, CanonicalObjectStore, CatalogPackStateV1, CatalogSlotV1,
    CommitPublicationV1, CommittedObjectComparison, CommittedObjectLookup, DagTransactionLimits,
    DurableAuthorityViewV1, EndpointValue, GenerationLeaseKeyV1, IndexGenerationV1,
    LegacyHistoryRoot, LogPackAddress, LogPackDecodeLimits, LogPackObjectPlanV1,
    MutableObjectStore, MutationBatchViewV1, ObjectAdmission, ObjectDigest, ObjectIndexHeaderV1,
    ObjectRefV1, OpenDagTransaction, PackId, PreparedLogPackTransactionV1, RetainedStateV1,
    RetainedUnionClosureLimitsV1, RetentionManifestV1, SemanticNamespace, SemanticProfile,
    SemanticProfileId, SemanticRoots, SemanticStateManifestV1, SemanticStateManifestView,
    SemanticValidationLimits, StateVersionV1, StructuralClosureLedgerV1,
    StructuralValidationLimits, TRANSACTION_COMMIT_RECORD_LEN_V1, TransactionCommitRecordV1,
    VerifiedDagState, verify_structural_closure, verify_structural_closure_in_ledger_with_limits,
};

type Endpoints = BTreeMap<(SemanticNamespace, Vec<u8>), EndpointValue>;

const PAIR: &[&str] = &["left", "right"];
const LEFT: &[&str] = &["left"];
const RIGHT: &[&str] = &["right"];

const CONTENTS: [Insertion; 7] = [
    Insertion::Datum {
        channel: "left",
        value: "alpha",
    },
    Insertion::Datum {
        channel: "left",
        value: "gamma",
    },
    Insertion::Datum {
        channel: "right",
        value: "beta",
    },
    Insertion::Continuation {
        group: PAIR,
        body: "first-body",
    },
    Insertion::Continuation {
        group: PAIR,
        body: "second-body",
    },
    Insertion::Join {
        channel: "left",
        group: PAIR,
    },
    Insertion::Join {
        channel: "right",
        group: PAIR,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Insertion {
    Datum {
        channel: &'static str,
        value: &'static str,
    },
    Continuation {
        group: &'static [&'static str],
        body: &'static str,
    },
    Join {
        channel: &'static str,
        group: &'static [&'static str],
    },
}

#[derive(Debug, PartialEq)]
enum Resolution {
    Resolved(ObjectRefV1, SemanticRoots),
    UnknownRoot(String),
    Failed(RootError),
}

#[derive(Clone, Copy)]
struct ManifestShape {
    generation: u64,
    profile: SemanticProfileId,
    legacy_history_root: Option<LegacyHistoryRoot>,
}

struct UnretainingImage {
    image: CheckpointPackImage,
    checkpoint_manifest: ObjectRefV1,
    checkpoint_retention: ObjectRefV1,
    successor_retention: ObjectRefV1,
}

struct CheckpointObjects(BTreeMap<ObjectDigest, Arc<[u8]>>);

impl CommittedObjectLookup for CheckpointObjects {
    type Error = Infallible;

    fn compare_committed_object(
        &self,
        reference: ObjectRefV1,
        sealed_canonical_bytes: &[u8],
    ) -> Result<CommittedObjectComparison, Self::Error> {
        Ok(match self.0.get(&reference.digest()) {
            None => CommittedObjectComparison::Missing,
            Some(committed) if committed.as_ref() == sealed_canonical_bytes => {
                CommittedObjectComparison::Equal
            }
            Some(_) => CommittedObjectComparison::Collision,
        })
    }
}

fn owned(channels: &[&str]) -> Vec<String> {
    channels
        .iter()
        .map(|channel| (*channel).to_owned())
        .collect()
}

fn insert(hot: &Store, insertion: Insertion) {
    match insertion {
        Insertion::Datum { channel, value } => {
            let channel = channel.to_owned();
            hot.put_datum(&channel, Datum::create(&channel, value.to_owned(), false));
        }
        Insertion::Continuation { group, body } => {
            let channels = owned(group);
            let patterns = channels
                .iter()
                .map(|channel| format!("{channel}-pattern"))
                .collect::<Vec<_>>();
            let continuation = WaitingContinuation::create(
                &channels,
                &patterns,
                &body.to_owned(),
                false,
                BTreeSet::new(),
            );
            assert_eq!(
                hot.put_continuation(&channels, continuation),
                Some(true),
                "the hot store stores a continuation it did not hold"
            );
        }
        Insertion::Join { channel, group } => {
            hot.put_join(&channel.to_owned(), &owned(group))
                .expect("the hot store records the join");
        }
    }
}

fn recover<P: SemanticProfile>(scratch: &PersistenceTrialDir) -> DurableAuthorityViewV1<P> {
    recover_read_authority::<P>(
        scratch.path(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("restart-grade admission recovers the store")
}

fn checkpoint_schedule(schedule: &[Vec<Insertion>]) -> (PersistenceTrialDir, Vec<StateVersionV1>) {
    let (scratch, hot, gate, mut owner) = fixture();
    let mut states = Vec::with_capacity(schedule.len());
    for unit in schedule {
        for insertion in unit {
            insert(&hot, *insertion);
        }
        states.push(
            poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
                .expect("the owner publishes the unit"),
        );
    }
    (scratch, states)
}

fn publish_datum(view: DurableAuthorityViewV1<Profile>, value: &str) -> StateVersionV1 {
    let hot: Store = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .expect("bootstrap the hot store from the view"),
    );
    let gate = Arc::new(RwLock::new(()));
    let mut owner = view
        .into_rspace_authority(&hot)
        .unwrap_or_else(|failure| panic!("the view becomes the owner: {:?}", failure.1));
    let channel = "retained-channel".to_owned();
    hot.put_datum(&channel, Datum::create(&channel, value.to_owned(), false));
    poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .expect("the owner publishes a successor")
}

fn endpoints<P: SemanticProfile>(
    view: &DurableAuthorityViewV1<P>,
    roots: SemanticRoots,
) -> Endpoints {
    let mut endpoints = Endpoints::new();
    for (namespace, root) in roots.iter() {
        for (path, value) in view
            .project_prefix(root, &[], u64::MAX, u64::MAX)
            .expect("project a retained namespace")
        {
            endpoints.insert((namespace, path), value);
        }
    }
    endpoints
}

fn endpoint_counts(endpoints: &Endpoints) -> [usize; 3] {
    [SemanticNamespace::DataBag, SemanticNamespace::ContinuationMap, SemanticNamespace::JoinBag]
        .map(|namespace| {
            endpoints
                .keys()
                .filter(|(candidate, _)| *candidate == namespace)
                .count()
        })
}

fn resolution(outcome: Result<ResolvedDurableStateV1, RootError>) -> Resolution {
    match outcome {
        Ok(resolved) => Resolution::Resolved(resolved.manifest(), resolved.roots()),
        Err(RootError::UnknownRootError(reason)) => Resolution::UnknownRoot(reason),
        Err(error) => Resolution::Failed(error),
    }
}

fn root_from_hash_bytes(digest: ObjectDigest) -> DurableStateRootV1 {
    DurableStateRootV1::try_from(&Blake2b256Hash::from_bytes(digest.as_bytes().to_vec()))
        .expect("a 32-byte digest converts to a durable state root")
}

fn encoded<T: CanonicalEncode>(value: &T) -> (ObjectRefV1, Vec<u8>) {
    let bytes = encode(value);
    let reference = ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&bytes), bytes.len() as u64)
        .expect("a canonical encoding has a valid object reference");
    (reference, bytes)
}

fn refusal(manifest: ObjectRefV1, reason: &str) -> Resolution {
    Resolution::UnknownRoot(format!(
        "durable state root {} {reason}",
        DurableStateRootV1::of_manifest(manifest)
    ))
}

fn checkpoint_source_of_empty_states(
    current: ManifestShape,
    historical: &[ManifestShape],
) -> (SyntheticCheckpointSource, SemanticRoots, Vec<ObjectRefV1>) {
    let empty = encoded(&CanonicalDagNode::empty());
    let roots = empty_roots(empty.0);
    let manifest_of = |shape: ManifestShape| {
        encoded(
            &SemanticStateManifestV1::new(1, shape.profile, roots, shape.legacy_history_root)
                .expect("a schema-one manifest is valid"),
        )
    };
    let closure_bytes = |manifest: ObjectRefV1| empty.0.encoded_len() + manifest.encoded_len();
    let mut manifests = Vec::with_capacity(historical.len() + 1);
    let mut retained = Vec::with_capacity(historical.len());
    for shape in historical {
        let manifest = manifest_of(*shape);
        retained.push(
            RetainedStateV1::new(
                StateVersionV1::new(shape.generation, manifest.0),
                closure_bytes(manifest.0),
            )
            .expect("a historical state claims its closure bytes"),
        );
        manifests.push(manifest);
    }
    let current_manifest = manifest_of(current);
    let current_state = StateVersionV1::new(current.generation, current_manifest.0);
    let current_bytes = closure_bytes(current_manifest.0);
    manifests.push(current_manifest);
    let retention = encoded(
        &RetentionManifestV1::new(
            RetainedStateV1::new(current_state, current_bytes)
                .expect("the current state claims its closure bytes"),
            retained,
        )
        .expect("the retention manifest is valid"),
    );
    let references = manifests
        .iter()
        .map(|(reference, _)| *reference)
        .collect::<Vec<_>>();
    let retained_union = RetainedUnionClosureReportV1 {
        distinct_objects: manifests.len() as u64 + 1,
        canonical_bytes: empty.0.encoded_len() +
            references
                .iter()
                .map(|reference| reference.encoded_len())
                .sum::<u64>(),
    };
    let retention_reference = retention.0;
    let mut objects = Vec::with_capacity(manifests.len() + 2);
    objects.push(empty);
    objects.extend(manifests);
    objects.push(retention);
    objects.sort_unstable_by_key(|(reference, _)| reference.digest());
    let data_bytes = objects
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum();
    let mut ordered = OrderedObjectInventoryBuilderV1::new(objects.len() as u64, data_bytes);
    for (reference, _) in &objects {
        ordered
            .push(*reference)
            .expect("the objects are distinct and digest ordered");
    }
    let inventory = CheckpointObjectInventoryV1::from_test_ordered(
        retained_union,
        retention_reference,
        ordered.finish().expect("the inventory is complete"),
    )
    .expect("the inventory is the retained union plus its retention manifest");
    let mut source = checkpoint_source_fixture();
    source.descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        current.profile,
        current_state,
        retention_reference,
        2,
        current_bytes,
        inventory,
    )
    .expect("the descriptor satisfies the V1 inventory rule");
    source.objects = objects;
    source.sort_objects_by_digest();
    (source, roots, references)
}

fn image_whose_successor_stops_retaining_the_checkpoint() -> UnretainingImage {
    let source = fixture_for_profile(Profile::ID);
    let checkpoint = checkpoint_image_for_profile::<Profile>(fixture_for_profile(Profile::ID));
    assert_eq!(checkpoint.catalog.state(), source.descriptor.state());
    let store = CanonicalObjectStore::new();
    let mut committed = BTreeMap::new();
    for (reference, bytes) in &source.objects {
        let bytes: Arc<[u8]> = bytes.clone().into();
        assert!(matches!(
            store
                .put_if_absent_or_equal(*reference, Arc::clone(&bytes))
                .expect("the sealing store admits a checkpoint object"),
            ObjectAdmission::Inserted | ObjectAdmission::AlreadyPresent
        ));
        committed.insert(reference.digest(), bytes);
    }
    let mutation = data_batch(source.descriptor.state(), "retained-successor");
    let batch = MutationBatchViewV1::parse(&mutation).expect("the successor mutation parses");
    let verified = VerifiedDagState::<_, Profile>::admit(
        &store,
        batch.base(),
        SemanticValidationLimits::unbounded(),
    )
    .expect("the checkpoint state is admitted");
    let sealed = OpenDagTransaction::new(verified, batch, DagTransactionLimits::UNBOUNDED)
        .expect("the successor transaction opens")
        .seal()
        .expect("the successor transaction seals");
    let target = sealed.receipt().target();
    for object in sealed.objects() {
        store
            .put_if_absent_or_equal(object.reference(), object.canonical_bytes_arc())
            .expect("the sealing store admits a successor object");
    }
    let closure_bytes = verify_structural_closure(&store, target.manifest())
        .expect("the successor state is structurally closed")
        .canonical_bytes;
    let retention = RetentionManifestV1::new(
        RetainedStateV1::new(target, closure_bytes)
            .expect("the successor claims its closure bytes"),
        Vec::new(),
    )
    .expect("a retention manifest without history is valid");
    let successor_retention = retention.object_ref();
    let sealed = sealed
        .with_retention_manifest(retention)
        .expect("the successor carries its retention manifest");
    let plan = LogPackObjectPlanV1::classify(&sealed, &CheckpointObjects(committed))
        .expect("the successor objects are classified against the checkpoint");
    let store_id = checkpoint.catalog.store_id();
    let sequence = checkpoint
        .catalog
        .commit_sequence()
        .successor()
        .expect("the checkpoint sequence has a successor");
    let prepared = PreparedLogPackTransactionV1::<Profile>::new(
        &sealed,
        plan,
        store_id,
        sequence,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .expect("the successor transaction is prepared");
    let mut pack = checkpoint.pack.to_vec();
    let prepare_offset = pack.len() as u64;
    let index_append = PreparedIndexAppendV1::new(
        ObjectIndexHeaderV1::new(store_id, IndexGenerationV1::FIRST),
        checkpoint.catalog.durable_index_tail(),
        PackId::GENESIS,
        prepare_offset,
        &prepared,
    )
    .expect("the successor index append is prepared");
    let publication = CommitPublicationV1::new(
        PackId::GENESIS.get(),
        IndexGenerationV1::FIRST.get(),
        index_append.new_tail(),
        successor_retention,
        closure_bytes,
        closure_bytes,
    )
    .expect("the successor publication is valid");
    let prepare_identity = prepared
        .write_to(&mut pack)
        .expect("the successor prepare record is written");
    let prepare_address =
        LogPackAddress::from_pack(PackId::GENESIS, prepare_offset, prepare_identity.record_len())
            .expect("the successor prepare record has an address");
    let commit = TransactionCommitRecordV1::new(
        sealed.receipt(),
        store_id,
        checkpoint.catalog.commit_digest(),
        prepare_address,
        prepare_identity,
        publication,
    )
    .expect("the successor commit record is valid");
    let commit_address = LogPackAddress::from_pack(
        PackId::GENESIS,
        pack.len() as u64,
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    )
    .expect("the successor commit record has an address");
    pack.extend_from_slice(commit.canonical_bytes());
    let mut canonical_index = checkpoint.canonical_index;
    let mut records = checkpoint.records;
    for record in index_append.records() {
        let record = record.expect("the successor index record is valid");
        canonical_index.extend_from_slice(&record.encode());
        records.push(record);
    }
    assert_eq!(canonical_index.len() as u64, index_append.new_tail());
    let pack_state = CatalogPackStateV1::new(
        PackId::GENESIS.get(),
        PackId::GENESIS.get(),
        commit_address.end(),
        IndexGenerationV1::FIRST.get(),
        index_append.new_tail(),
        successor_retention,
        closure_bytes,
        closure_bytes,
    )
    .expect("the successor pack state is valid");
    let catalog = CatalogSlotV1::new(
        store_id,
        1,
        target,
        Profile::ID,
        commit.digest(),
        sequence,
        commit_address,
        pack_state,
    )
    .expect("the successor catalog slot is valid");
    UnretainingImage {
        image: CheckpointPackImage {
            pack: pack.into(),
            catalog,
            canonical_index,
            records,
            prepare_address,
            commit_address,
        },
        checkpoint_manifest: source.descriptor.state().manifest(),
        checkpoint_retention: source.descriptor.retention_manifest(),
        successor_retention,
    }
}

#[tokio::test]
async fn durable_state_root_is_a_function_of_contents_across_insertion_orders_and_reopen() {
    let mut schedules = Vec::with_capacity(CONTENTS.len() + 7);
    for shift in 0..CONTENTS.len() {
        let mut order = CONTENTS;
        order.rotate_left(shift);
        schedules.push(vec![order.to_vec()]);
    }
    let mut reversed = CONTENTS;
    reversed.reverse();
    schedules.push(vec![reversed.to_vec()]);
    let single_checkpoint_schedules = schedules.len();
    let units = [&CONTENTS[..2], &CONTENTS[2..3], &CONTENTS[3..]];
    for unit_order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        schedules.push(unit_order.map(|unit| units[unit].to_vec()).to_vec());
    }
    let distinct_schedules = schedules.iter().collect::<BTreeSet<_>>().len();
    let distinct_insertion_orders = schedules
        .iter()
        .map(|schedule| schedule.concat())
        .collect::<BTreeSet<_>>()
        .len();

    let runs = schedules
        .iter()
        .map(|schedule| checkpoint_schedule(schedule))
        .collect::<Vec<_>>();
    let final_roots = runs
        .iter()
        .map(|(_, states)| {
            DurableStateRootV1::of_manifest(
                states
                    .last()
                    .expect("every schedule publishes a state")
                    .manifest(),
            )
        })
        .collect::<Vec<_>>();
    let distinct_final_roots = final_roots.iter().collect::<BTreeSet<_>>().len();
    let distinct_first_unit_roots = runs[single_checkpoint_schedules..]
        .iter()
        .map(|(_, states)| DurableStateRootV1::of_manifest(states[0].manifest()))
        .collect::<BTreeSet<_>>()
        .len();
    let reopened = runs
        .iter()
        .map(|(scratch, _)| {
            let view = recover::<Profile>(scratch);
            let root = DurableStateRootV1::of_manifest(view.manifest());
            let resolved = view
                .resolve(root)
                .expect("the reopened current state resolves");
            (
                root,
                resolved.roots(),
                view.semantic_roots()
                    .expect("read the reopened semantic roots"),
                endpoint_counts(&endpoints(&view, resolved.roots())),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        (
            distinct_schedules,
            distinct_insertion_orders,
            distinct_final_roots,
            distinct_first_unit_roots
        ),
        (14, 11, 1, 3),
        "fourteen distinct schedules (eleven distinct insertion orders, three unit orders with \
         distinct first states) must publish one final root"
    );
    let expected_roots = reopened[0].1;
    assert_eq!(
        reopened,
        vec![(final_roots[0], expected_roots, expected_roots, [3, 2, 2]); runs.len()],
        "every reopened store must recover the published root and resolve it to the same roots"
    );
}

#[tokio::test]
async fn every_retained_state_resolves_to_its_own_roots_and_nothing_else_resolves() {
    let scratch = trial_scratch("durable-state-root-retained-");
    let built = image_whose_successor_stops_retaining_the_checkpoint();
    install_checkpoint_image(scratch.path(), &built.image);
    let mut retained = Vec::with_capacity(3);
    for value in ["second-state", "third-state"] {
        let view = recover::<Profile>(&scratch);
        retained.push((
            view.manifest(),
            view.semantic_roots()
                .expect("read the current semantic roots"),
        ));
        publish_datum(view, value);
    }
    let view = recover::<Profile>(&scratch);
    retained.push((
        view.manifest(),
        view.semantic_roots()
            .expect("read the current semantic roots"),
    ));
    let distinct_states = retained
        .iter()
        .map(|(manifest, roots)| (manifest.digest(), roots.data_bag.digest()))
        .collect::<BTreeSet<_>>()
        .len();

    let resolved = retained
        .iter()
        .map(|(manifest, _)| resolution(view.resolve(DurableStateRootV1::of_manifest(*manifest))))
        .collect::<Vec<_>>();
    let expected = retained
        .iter()
        .map(|(manifest, roots)| Resolution::Resolved(*manifest, *roots))
        .collect::<Vec<_>>();

    let candidates = [
        ("a digest absent from the store", ObjectDigest::from_array([0xA5; 32])),
        ("the shared empty trie node", encoded(&CanonicalDagNode::empty()).0.digest()),
        ("the first retained state's data-bag node", retained[0].1.data_bag.digest()),
        ("the checkpoint's retention manifest", built.checkpoint_retention.digest()),
        ("the first successor's retention manifest", built.successor_retention.digest()),
        ("the current retention anchor", view.catalog().pack().retention_manifest().digest()),
        ("the unretained checkpoint manifest", built.checkpoint_manifest.digest()),
    ];
    let presence = candidates.map(|(label, digest)| {
        (
            label,
            view.get_object_bounded(&digest, u64::MAX)
                .expect("read the object index")
                .is_some(),
        )
    });
    let checkpoint_manifest = SemanticStateManifestView::parse(
        &view
            .get_object_bounded(&built.checkpoint_manifest.digest(), u64::MAX)
            .expect("read the object index")
            .expect("the unretained checkpoint manifest stays indexed"),
    )
    .expect("the unretained checkpoint manifest is a semantic manifest");
    let refusals = candidates
        .map(|(label, digest)| (label, resolution(view.resolve(root_from_hash_bytes(digest)))));

    assert_eq!(
        (view.report().retained_states, distinct_states),
        (3, 3),
        "the store retains three distinct states"
    );
    assert_eq!(resolved, expected, "every retained state resolves to exactly its own roots");
    assert_eq!(
        presence.map(|(_, present)| present),
        [false, true, true, true, true, true, true],
        "every refused digest except the absent one names an object in the store: {presence:?}"
    );
    assert_eq!(
        (checkpoint_manifest.profile_id(), checkpoint_manifest.legacy_history_root()),
        (Profile::ID, None),
        "the unretained checkpoint manifest would resolve if only the object index were consulted"
    );
    assert!(
        refusals
            .iter()
            .all(|(_, outcome)| matches!(outcome, Resolution::UnknownRoot(_))),
        "every digest that is not a retained state must be refused as an unknown root: \
         {refusals:#?}"
    );
}

#[tokio::test]
async fn states_that_differ_in_one_endpoint_have_different_roots() {
    let base = [
        Insertion::Datum {
            channel: "left",
            value: "alpha",
        },
        Insertion::Continuation {
            group: LEFT,
            body: "first-body",
        },
        Insertion::Join {
            channel: "left",
            group: LEFT,
        },
    ];
    let beta = Insertion::Datum {
        channel: "left",
        value: "beta",
    };
    let steps = [
        (beta, SemanticNamespace::DataBag),
        (beta, SemanticNamespace::DataBag),
        (
            Insertion::Continuation {
                group: LEFT,
                body: "second-body",
            },
            SemanticNamespace::ContinuationMap,
        ),
        (
            Insertion::Join {
                channel: "right",
                group: RIGHT,
            },
            SemanticNamespace::JoinBag,
        ),
    ];
    let (scratch, hot, gate, mut owner) = fixture();
    let mut states = Vec::with_capacity(steps.len() + 1);
    for insertion in base {
        insert(&hot, insertion);
    }
    states.push(
        poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
            .expect("the owner publishes the base state"),
    );
    for (insertion, _) in steps {
        insert(&hot, insertion);
        states.push(
            poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
                .expect("the owner publishes the changed state"),
        );
    }
    drop(owner);

    let view = recover::<Profile>(&scratch);
    let resolved = states
        .iter()
        .map(|state| {
            view.resolve(DurableStateRootV1::of_manifest(state.manifest()))
                .expect("every published state is retained and resolves")
        })
        .collect::<Vec<_>>();
    let contents = resolved
        .iter()
        .map(|state| endpoints(&view, state.roots()))
        .collect::<Vec<_>>();
    let observed = resolved
        .windows(2)
        .zip(contents.windows(2))
        .map(|(states, endpoints)| {
            let changed_endpoints = endpoints[0]
                .keys()
                .chain(endpoints[1].keys())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter(|key| endpoints[0].get(*key) != endpoints[1].get(*key))
                .map(|(namespace, _)| *namespace)
                .collect::<Vec<_>>();
            let changed_roots = states[0]
                .roots()
                .iter()
                .into_iter()
                .zip(states[1].roots().iter())
                .filter_map(|((namespace, before), (_, after))| {
                    (before != after).then_some(namespace)
                })
                .collect::<Vec<_>>();
            (changed_endpoints, changed_roots, states[0].root() != states[1].root())
        })
        .collect::<Vec<_>>();
    let expected = steps
        .map(|(_, namespace)| (vec![namespace], vec![namespace], true))
        .to_vec();
    let channel = "left".to_owned();
    let beta_path =
        RSpaceDataIndex::datum_path(&channel, &Datum::create(&channel, "beta".to_owned(), false));
    let beta_endpoints = contents[..3]
        .iter()
        .map(|endpoints| {
            endpoints
                .get(&(SemanticNamespace::DataBag, beta_path.clone()))
                .copied()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        endpoint_counts(&contents[0]),
        [1, 1, 1],
        "the base state holds one endpoint in each namespace"
    );
    assert_eq!(
        observed, expected,
        "each step changes exactly one endpoint, only its namespace root, and the state root"
    );
    assert_eq!(
        beta_endpoints,
        [
            None,
            Some(EndpointValue::Multiplicity(nonzero(1))),
            Some(EndpointValue::Multiplicity(nonzero(2)))
        ],
        "the second step changes only the multiplicity of the datum the first step added"
    );
}

#[test]
fn a_manifest_with_a_legacy_history_root_does_not_resolve() {
    let shape = |generation, seed: Option<u8>| ManifestShape {
        generation,
        profile: TEST_SEMANTIC_PROFILE_ID,
        legacy_history_root: seed.map(|seed| LegacyHistoryRoot::from_array([seed; 32])),
    };
    let stores = [
        (shape(9, None), vec![shape(1, Some(0x11)), shape(2, Some(0x22))]),
        (shape(9, Some(0x33)), Vec::new()),
    ];
    let mut observed = Vec::with_capacity(stores.len());
    let mut expected = Vec::with_capacity(stores.len());
    for (current, historical) in stores {
        let (source, roots, manifests) = checkpoint_source_of_empty_states(current, &historical);
        let scratch = trial_scratch("durable-state-root-legacy-");
        install_checkpoint_image(scratch.path(), &checkpoint_image_from_source(source));
        let view = recover::<TestSemanticProfile>(&scratch);
        let legacy_history_roots = manifests
            .iter()
            .map(|manifest| {
                SemanticStateManifestView::parse(
                    &view
                        .get_object_bounded(&manifest.digest(), u64::MAX)
                        .expect("read the object index")
                        .expect("every retained manifest is indexed"),
                )
                .expect("every retained manifest is a semantic manifest")
                .legacy_history_root()
            })
            .collect::<Vec<_>>();
        let outcomes = manifests
            .iter()
            .map(|manifest| resolution(view.resolve(DurableStateRootV1::of_manifest(*manifest))))
            .collect::<Vec<_>>();
        observed.push((view.report().retained_states, legacy_history_roots, outcomes));

        let shapes = historical
            .iter()
            .copied()
            .chain([current])
            .collect::<Vec<_>>();
        expected.push((
            shapes.len() as u64,
            shapes
                .iter()
                .map(|shape| shape.legacy_history_root)
                .collect::<Vec<_>>(),
            shapes
                .iter()
                .zip(&manifests)
                .map(|(shape, manifest)| match shape.legacy_history_root {
                    Some(_) => {
                        refusal(*manifest, "names a manifest with a legacy history provenance root")
                    }
                    None => Resolution::Resolved(*manifest, roots),
                })
                .collect::<Vec<_>>(),
        ));
    }

    assert_eq!(
        observed, expected,
        "every retained manifest with a legacy history root is refused for that reason alone"
    );
}

#[test]
fn a_retained_manifest_of_another_semantic_profile_does_not_resolve() {
    let current = ManifestShape {
        generation: 9,
        profile: TEST_SEMANTIC_PROFILE_ID,
        legacy_history_root: None,
    };
    let foreign = ManifestShape {
        generation: 1,
        profile: Profile::ID,
        legacy_history_root: None,
    };
    let (source, roots, manifests) = checkpoint_source_of_empty_states(current, &[foreign]);
    let scratch = trial_scratch("durable-state-root-profile-");
    install_checkpoint_image(scratch.path(), &checkpoint_image_from_source(source));
    let view = recover::<TestSemanticProfile>(&scratch);
    let outcomes = manifests
        .iter()
        .map(|manifest| resolution(view.resolve(DurableStateRootV1::of_manifest(*manifest))))
        .collect::<Vec<_>>();

    assert_ne!(Profile::ID, TEST_SEMANTIC_PROFILE_ID);
    assert_eq!(
        (view.report().retained_states, outcomes),
        (2, vec![
            refusal(manifests[0], "names a manifest of another semantic profile"),
            Resolution::Resolved(manifests[1], roots),
        ]),
        "a retained manifest of another profile is refused while the current state resolves"
    );
}

#[test]
fn durable_state_root_converts_to_and_from_blake2b256_hash_without_rehashing() {
    let empty = encoded(&CanonicalDagNode::empty()).0;
    let state_manifest = manifest(empty_roots(empty));
    let root = DurableStateRootV1::of_manifest(state_manifest.object_ref());
    let hash = Blake2b256Hash::from(root);

    assert_eq!(hash.bytes(), root.digest().as_bytes().to_vec());
    assert_eq!(hash, Blake2b256Hash::new(&encode(&state_manifest)));
    assert_ne!(hash, Blake2b256Hash::new(root.digest().as_bytes()));
    assert_eq!(DurableStateRootV1::try_from(&hash), Ok(root));

    let lengths = 0..=2 * OBJECT_DIGEST_LEN;
    let conversions = lengths
        .clone()
        .map(|length| {
            let bytes = (0..length)
                .map(|index| (index as u8) ^ 0xA5)
                .collect::<Vec<_>>();
            DurableStateRootV1::try_from(&Blake2b256Hash::from_bytes(bytes))
                .map(|converted| Blake2b256Hash::from(converted).bytes())
        })
        .collect::<Vec<_>>();
    let expected = lengths
        .map(|length| match length {
            OBJECT_DIGEST_LEN => Ok((0..length)
                .map(|index| (index as u8) ^ 0xA5)
                .collect::<Vec<_>>()),
            _ => Err(RootError::InvalidRootHash(format!(
                "expected {OBJECT_DIGEST_LEN} bytes, found {length}"
            ))),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        conversions, expected,
        "only a 32-byte hash converts, and it converts back to the same bytes"
    );
}

#[test]
fn resolution_reads_only_the_resolved_manifest_whatever_the_state_and_history_size() {
    let store = TracingStore::default();
    let empty = rebuild(&store, &Entries::new());
    let mut ledger = StructuralClosureLedgerV1::new();
    let sizes = [1_usize, 16, 256];
    let mut retained = Vec::with_capacity(sizes.len());
    for size in sizes {
        let channel = format!("scan-channel-{size}");
        let entries = (0..size)
            .map(|index| {
                let datum = Datum::create(&channel, format!("datum-{index:04}"), false);
                (
                    RSpaceDataIndex::datum_path(&channel, &datum),
                    EndpointValue::Multiplicity(nonzero(1)),
                )
            })
            .collect::<Entries>();
        let roots = SemanticRoots {
            data_bag: rebuild(&store, &entries),
            ..empty_roots(empty)
        };
        let state_manifest = manifest(roots);
        assert_eq!(
            store
                .put_if_absent_or_equal(state_manifest.object_ref(), encode(&state_manifest).into())
                .expect("the store admits the manifest"),
            ObjectAdmission::Inserted
        );
        let closure = verify_structural_closure_in_ledger_with_limits(
            &store,
            state_manifest.object_ref(),
            StructuralValidationLimits::unbounded(),
            &mut ledger,
            RetainedUnionClosureLimitsV1::UNBOUNDED,
        )
        .expect("the state is credited to the ledger");
        retained.push((state_manifest.object_ref(), roots, closure.report().distinct_objects));
    }
    let unretained = manifest(empty_roots(empty));
    assert_eq!(
        store
            .put_if_absent_or_equal(unretained.object_ref(), encode(&unretained).into())
            .expect("the store admits the unretained manifest"),
        ObjectAdmission::Inserted
    );

    let measured = retained
        .iter()
        .map(|(state_manifest, _, _)| {
            store.clear_reads();
            let outcome = resolution(resolve_retained_state::<TestSemanticProfile, _>(
                &ledger,
                &store,
                DurableStateRootV1::of_manifest(*state_manifest),
            ));
            (outcome, store.total_reads(), store.read_count(state_manifest.digest()))
        })
        .collect::<Vec<_>>();
    store.clear_reads();
    let refused = resolution(resolve_retained_state::<TestSemanticProfile, _>(
        &ledger,
        &store,
        DurableStateRootV1::of_manifest(unretained.object_ref()),
    ));
    let refused_reads = store.total_reads();
    let largest_closure = retained
        .iter()
        .map(|(_, _, objects)| *objects)
        .max()
        .expect("the ledger holds retained states");

    assert!(
        largest_closure > 256,
        "a scan of the largest retained state would read {largest_closure} objects"
    );
    assert_eq!(
        measured,
        retained
            .iter()
            .map(|(state_manifest, roots, _)| (Resolution::Resolved(*state_manifest, *roots), 1, 1))
            .collect::<Vec<_>>(),
        "each resolution reads exactly one object, the resolved manifest"
    );
    assert_eq!(
        (refused, refused_reads),
        (refusal(unretained.object_ref(), "is not a current or retained state of this store"), 0),
        "a digest the ledger does not count is refused without reading the store"
    );
}

#[tokio::test]
async fn read_handle_at_leases_the_view_generation_and_reads_the_resolved_state() {
    let (scratch, hot, gate, mut owner) = fixture();
    let checkpoint = owner.state(&hot).expect("read the checkpoint state");
    insert(&hot, Insertion::Datum {
        channel: "left",
        value: "handle",
    });
    let successor = poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .expect("the owner publishes a successor");
    drop(owner);
    let (foreign_scratch, foreign_hot, foreign_gate, mut foreign_owner) = fixture();
    insert(&foreign_hot, Insertion::Datum {
        channel: "left",
        value: "foreign",
    });
    poll_checkpoint_with(&mut foreign_owner, &foreign_gate, &foreign_hot, publication_limits())
        .expect("the foreign owner publishes a successor");
    drop(foreign_owner);
    let foreign_view = recover::<Profile>(&foreign_scratch);
    let foreign = foreign_view
        .resolve(DurableStateRootV1::of_manifest(foreign_view.manifest()))
        .expect("the foreign store resolves its current state");

    let view = recover::<Profile>(&scratch);
    let registry = view.generation_leases();
    let key = GenerationLeaseKeyV1::from_catalog(view.catalog());
    let resolved = [checkpoint, successor].map(|state| {
        view.resolve(DurableStateRootV1::of_manifest(state.manifest()))
            .expect("a retained state resolves")
    });
    let without_handles = registry.active_lease_count_for_test();
    let handles = resolved.map(|state| {
        view.read_handle_at(&state)
            .expect("take a read handle at a retained state")
    });
    let handle_leases = registry.active_lease_count_for_test() - without_handles;
    let refused = view.read_handle_at(&foreign).map(|_| ());
    let leases_after_refusal = registry.active_lease_count_for_test() - without_handles;
    drop(view);
    let pinned_without_view = registry
        .has_active(key)
        .expect("query the view generation's lease");
    let read_roots = handles
        .each_ref()
        .map(|handle| handle.semantic_roots().expect("read through the handle"));
    drop(handles);

    assert_ne!(resolved[0].roots(), resolved[1].roots());
    assert!(
        matches!(refused, Err(DurableStateReadHandleErrorV1::Root(RootError::UnknownRootError(_)))),
        "a state this store does not retain is refused: {refused:?}"
    );
    assert_eq!(
        (
            handle_leases,
            leases_after_refusal,
            pinned_without_view,
            read_roots,
            registry.active_lease_count_for_test()
        ),
        (2, 2, true, resolved.map(ResolvedDurableStateV1::roots), 0),
        "each handle holds one lease on the view's generation and reads its own state; a refused \
         state takes no lease"
    );
}
