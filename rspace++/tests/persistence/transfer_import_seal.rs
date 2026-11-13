use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::canonical_rebuild_oracle::{Entries, rebuild};
use super::checkpoint_wal::fixture as checkpoint_source_fixture;
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{checkpoint_image_for_profile, limits as store_limits};
use super::durable_rspace_authority::{
    Profile, Store, fixture, poll_checkpoint_with, publication_limits,
};
use super::support::{PersistenceTrialDir, admit, empty_roots, encode, trial_scratch};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, GenerationCheckpointDescriptorV1, OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::closure::RetainedUnionClosureReportV1;
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableImportSessionV1, DurableIoError, DurableIoFailpointV1,
    DurableStateRootV1, TransferBindingV1, TransferImportErrorV1, TransferObjectRefusalV1,
    TransferPageLimitsV1, TransferPageViewV1, TransferStagingOutcomeV1, TransferTokenV1,
    abort_import, arm_durable_io_failpoint, create_empty_store_v1, recover_read_authority,
};
use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalEncode, CanonicalFormatError, CanonicalObjectStore,
    CanonicalObjectView, CatalogSlotV1, ClosureError, DurableAuthorityViewV1, EndpointValue,
    ImmutableObjectStore, LogPackDecodeLimits, MutableObjectStore, ObjectAdmission, ObjectDigest,
    ObjectRefV1, RetainedStateV1, RetentionManifestV1, SemanticProfile, SemanticRoots,
    SemanticStateManifestV1, SemanticStateManifestView, SemanticValidationLimits, StateVersionV1,
    StructuralValidationLimits, verify_rspace_semantics, verify_structural_closure,
};

const STAGING: &str = "transfer-import.v1";
const OBJECTS: &str = "objects.v1";
const LENGTH_FIELD_LEN: usize = 8;
const MAX_PAGE_BYTES: u64 = 2048;
const MAX_PAGE_OBJECTS: u64 = 6;
const VALUES_PER_CHANNEL: usize = 8;
const CHANNELS: [&str; 3] = ["seal-alpha", "seal-beta", "seal-gamma"];
const GROUPS: [&[&str]; 3] =
    [&["seal-alpha"], &["seal-alpha", "seal-beta"], &["seal-alpha", "seal-beta", "seal-gamma"]];
const MEMBERSHIPS: usize = 6;

type Session = DurableImportSessionV1<Profile>;
type Sealing<S> = Result<S, Box<(Session, TransferImportErrorV1)>>;
type StoreImage = BTreeMap<PathBuf, Option<Vec<u8>>>;
type StagedFiles = BTreeMap<String, Vec<u8>>;

struct ExportedStream {
    manifest: ObjectRefV1,
    binding: TransferBindingV1,
    pages: Vec<Vec<u8>>,
}

struct DanglingState {
    manifest: ObjectRefV1,
    refusal: CanonicalFormatError,
}

#[derive(Debug, PartialEq, Eq)]
struct Staging {
    frontier: TransferTokenV1,
    outstanding: u64,
    files: StagedFiles,
}

#[derive(Debug, PartialEq, Eq)]
struct Trace {
    staging_left: bool,
    store: StoreImage,
    resumed: Result<TransferTokenV1, String>,
    fresh: Result<(TransferTokenV1, u64, usize), String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Incomplete {
        position: u64,
        outstanding: u64,
    },
    SessionFailed,
    ObjectsMissing {
        frontier: u64,
        staged: u64,
    },
    RecordLength {
        position: u64,
        len: u64,
        max_page_bytes: u64,
    },
    Duplicate {
        position: u64,
        digest: ObjectDigest,
    },
    Missing(ObjectDigest),
    Other(String),
}

fn refusal_of(error: &TransferImportErrorV1) -> Refusal {
    match error {
        TransferImportErrorV1::IncompleteClosure {
            position,
            outstanding,
        } => Refusal::Incomplete {
            position: *position,
            outstanding: *outstanding,
        },
        TransferImportErrorV1::SessionFailed => Refusal::SessionFailed,
        TransferImportErrorV1::StagedObjectsMissing { frontier, staged } => {
            Refusal::ObjectsMissing {
                frontier: *frontier,
                staged: *staged,
            }
        }
        TransferImportErrorV1::StagedRecordLength {
            position,
            len,
            max_page_bytes,
        } => Refusal::RecordLength {
            position: *position,
            len: *len,
            max_page_bytes: *max_page_bytes,
        },
        TransferImportErrorV1::StagedObject {
            position,
            refusal: TransferObjectRefusalV1::Duplicate { digest },
        } => Refusal::Duplicate {
            position: *position,
            digest: *digest,
        },
        TransferImportErrorV1::ColdAdmission(CanonicalFormatError::MissingObject(digest)) => {
            Refusal::Missing(*digest)
        }
        other => Refusal::Other(other.to_string()),
    }
}

fn page_limits() -> TransferPageLimitsV1 {
    TransferPageLimitsV1::new(MAX_PAGE_BYTES, MAX_PAGE_OBJECTS)
        .expect("the test page limits admit at least one object")
}

fn owned(channels: &[&str]) -> Vec<String> {
    channels
        .iter()
        .map(|channel| (*channel).to_owned())
        .collect()
}

fn generate_state(hot: &Store) {
    for channel in owned(&CHANNELS) {
        for index in 0..VALUES_PER_CHANNEL {
            let value = format!("{channel}-value-{index:03}");
            hot.put_datum(&channel, Datum::create(&channel, value, false));
        }
    }
    for group in GROUPS {
        let channels = owned(group);
        let patterns = channels
            .iter()
            .map(|channel| format!("{channel}-pattern"))
            .collect::<Vec<_>>();
        let body = format!("body-of-{}", channels.join("+"));
        let continuation =
            WaitingContinuation::create(&channels, &patterns, &body, false, BTreeSet::new());
        assert_eq!(
            hot.put_continuation(&channels, continuation),
            Some(true),
            "the hot store stores a continuation it did not hold"
        );
        for member in &channels {
            hot.put_join(member, &channels)
                .expect("the hot store records the join");
        }
    }
}

fn recover(store: &Path) -> DurableAuthorityViewV1<Profile> {
    recover_read_authority::<Profile>(
        store,
        store_limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("restart-grade admission recovers the store")
}

fn export_stream(
    view: &DurableAuthorityViewV1<Profile>,
    manifest: ObjectRefV1,
    limits: TransferPageLimitsV1,
) -> ExportedStream {
    let mut session = view
        .open_export(DurableStateRootV1::of_manifest(manifest), limits)
        .expect("open an export of a retained state");
    let binding = session.binding();
    let mut token = session.first_token();
    let mut pages = Vec::new();
    loop {
        let page = session
            .page(token)
            .expect("the export session serves the next position")
            .to_vec();
        let parsed = TransferPageViewV1::parse(&page, limits).expect("an exported page parses");
        let (count, next) = (parsed.object_count(), parsed.next_token());
        pages.push(page);
        match count {
            0 => {
                return ExportedStream {
                    manifest,
                    binding,
                    pages,
                };
            }
            _ => token = next,
        }
    }
}

fn generated_stream(limits: TransferPageLimitsV1) -> ExportedStream {
    let (scratch, hot, gate, mut owner) = fixture();
    generate_state(&hot);
    let manifest = poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .expect("the owner publishes the generated state")
        .manifest();
    drop(owner);
    let view = recover(scratch.path());
    let stream = export_stream(&view, manifest, limits);
    drop(view);
    drop(scratch);
    stream
}

fn objects_of(pages: &[Vec<u8>], limits: TransferPageLimitsV1) -> Vec<Vec<u8>> {
    let terminal = pages.last().expect("a stream ends with its terminal page");
    let closure_len = TransferPageViewV1::parse(terminal, limits)
        .expect("an exported page parses")
        .position();
    let mut objects = Vec::with_capacity(closure_len as usize);
    for page in pages {
        objects.extend(
            TransferPageViewV1::parse(page, limits)
                .expect("an exported page parses")
                .objects()
                .map(<[u8]>::to_vec),
        );
    }
    objects
}

fn reference_of(object: &[u8]) -> ObjectRefV1 {
    ObjectRefV1::new(ObjectDigest::of_canonical_bytes(object), object.len() as u64)
        .expect("a canonical object has a valid reference")
}

fn encoded<T: CanonicalEncode>(value: &T) -> (ObjectRefV1, Vec<u8>) {
    let bytes = encode(value);
    (reference_of(&bytes), bytes)
}

fn memory_store(objects: &[Vec<u8>]) -> CanonicalObjectStore {
    let store = CanonicalObjectStore::new();
    for object in objects {
        assert_eq!(
            store
                .put_if_absent_or_equal(reference_of(object), Arc::from(object.as_slice()))
                .expect("the in-memory store admits an exported object"),
            ObjectAdmission::Inserted,
            "every object of an exported stream is distinct"
        );
    }
    store
}

fn read_object(store: &CanonicalObjectStore, reference: ObjectRefV1) -> Arc<[u8]> {
    store
        .get_bounded(&reference.digest(), reference.encoded_len())
        .expect("read the in-memory store")
        .expect("the in-memory store holds every object of a state closure")
}

fn references_of(object: &[u8]) -> Vec<ObjectRefV1> {
    match CanonicalObjectView::parse(object).expect("every object of a state closure is canonical")
    {
        CanonicalObjectView::SemanticManifest(manifest) => manifest
            .roots()
            .iter()
            .into_iter()
            .map(|(_, root)| root)
            .collect(),
        CanonicalObjectView::DagNode(node) => {
            let children = node.children();
            let mut references = Vec::with_capacity(children.len() + 1);
            if let EndpointValue::Content(content) = node.endpoint() {
                references.push(content);
            }
            references.extend(children.iter().map(|child| child.target));
            references
        }
        CanonicalObjectView::Blob(_) => Vec::new(),
        other => panic!("a state closure holds no {} object", other.kind().name()),
    }
}

fn closure_of(
    store: &CanonicalObjectStore,
    manifest: ObjectRefV1,
) -> BTreeMap<ObjectDigest, (ObjectRefV1, Vec<u8>)> {
    let mut closure = BTreeMap::new();
    let mut pending = vec![manifest];
    while let Some(reference) = pending.pop() {
        if closure.contains_key(&reference.digest()) {
            continue;
        }
        let object = read_object(store, reference);
        pending.extend(references_of(&object));
        closure.insert(reference.digest(), (reference, object.to_vec()));
    }
    closure
}

fn join_memberships(store: &CanonicalObjectStore, join_bag: ObjectRefV1) -> Entries {
    let mut memberships = Entries::new();
    let mut pending = vec![(join_bag, Vec::new())];
    while let Some((reference, mut key)) = pending.pop() {
        let object = read_object(store, reference);
        let CanonicalObjectView::DagNode(node) =
            CanonicalObjectView::parse(&object).expect("every object of a join bag is canonical")
        else {
            panic!("a join bag holds DAG nodes only");
        };
        key.extend_from_slice(node.prefix());
        for child in node.children().iter() {
            let mut child_key = Vec::with_capacity(key.len() + 1);
            child_key.extend_from_slice(&key);
            child_key.push(child.edge);
            pending.push((child.target, child_key));
        }
        match node.endpoint() {
            EndpointValue::Absent => {}
            endpoint => {
                memberships.insert(key, endpoint);
            }
        }
    }
    memberships
}

fn memory_refusal(
    store: &CanonicalObjectStore,
    manifest: ObjectRefV1,
    limits: SemanticValidationLimits,
) -> CanonicalFormatError {
    match verify_rspace_semantics::<Profile, _>(store, manifest, limits) {
        Ok(report) => panic!("cold admission in memory must refuse, not report {report:?}"),
        Err(ClosureError::Format(error)) => error,
        Err(ClosureError::Store(error)) => panic!("the in-memory store fails: {error}"),
    }
}

fn states_without_one_join_membership(
    store: &CanonicalObjectStore,
    manifest: ObjectRefV1,
) -> Vec<DanglingState> {
    let state = SemanticStateManifestView::parse(&read_object(store, manifest))
        .expect("the root of a state closure is its semantic manifest");
    let roots = state.roots();
    let memberships = join_memberships(store, roots.join_bag);
    assert_eq!(
        (memberships.len(), rebuild(store, &memberships)),
        (MEMBERSHIPS, roots.join_bag),
        "the generated join bag holds one entry for each membership, and the oracle rebuilds it \
         from them"
    );
    let mut states = Vec::with_capacity(memberships.len());
    for removed in memberships.keys() {
        let mut remaining = memberships.clone();
        remaining.remove(removed);
        let dangling = SemanticStateManifestV1::new(
            state.schema_epoch(),
            state.profile_id(),
            SemanticRoots {
                join_bag: rebuild(store, &remaining),
                ..roots
            },
            state.legacy_history_root(),
        )
        .expect("a manifest over the reduced join bag is valid");
        let manifest = admit(store, &dangling);
        let refusal = memory_refusal(store, manifest, SemanticValidationLimits::unbounded());
        assert!(
            matches!(refusal, CanonicalFormatError::SemanticCoherence { .. }),
            "a state without one join membership is refused for its dangling join, not for \
             {refusal}"
        );
        states.push(DanglingState { manifest, refusal });
    }
    states
}

fn history_store(store: &CanonicalObjectStore, states: &[DanglingState]) -> PersistenceTrialDir {
    let empty = encoded(&CanonicalDagNode::empty());
    let current = encoded(
        &SemanticStateManifestV1::new(1, Profile::ID, empty_roots(empty.0), None)
            .expect("a schema-one manifest of the empty state is valid"),
    );
    let current_state = StateVersionV1::new(states.len() as u64 + 1, current.0);
    let current_bytes = empty.0.encoded_len() + current.0.encoded_len();
    let mut union = BTreeMap::from([(empty.0.digest(), empty), (current.0.digest(), current)]);
    let mut historical = Vec::with_capacity(states.len());
    for (generation, state) in (1_u64..).zip(states) {
        let closure = closure_of(store, state.manifest);
        let closure_bytes = closure
            .values()
            .map(|(reference, _)| reference.encoded_len())
            .sum::<u64>();
        let structural = verify_structural_closure(store, state.manifest)
            .expect("a state without one join membership is structurally closed");
        assert_eq!(
            (structural.distinct_objects, structural.canonical_bytes),
            (closure.len() as u64, closure_bytes),
            "the structural closure of a retained state is the closure the source store holds"
        );
        historical.push(
            RetainedStateV1::new(StateVersionV1::new(generation, state.manifest), closure_bytes)
                .expect("a historical state claims its closure bytes"),
        );
        union.extend(closure);
    }
    let retention = encoded(
        &RetentionManifestV1::new(
            RetainedStateV1::new(current_state, current_bytes)
                .expect("the current state claims its closure bytes"),
            historical,
        )
        .expect("the retention manifest orders its historical states"),
    );
    let retained_union = RetainedUnionClosureReportV1 {
        distinct_objects: union.len() as u64,
        canonical_bytes: union
            .values()
            .map(|(reference, _)| reference.encoded_len())
            .sum(),
    };
    let retention_reference = retention.0;
    let mut objects = Vec::with_capacity(union.len() + 1);
    objects.extend(union.into_values());
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
        Profile::ID,
        current_state,
        retention_reference,
        2,
        current_bytes,
        inventory,
    )
    .expect("the descriptor satisfies the V1 inventory rule");
    source.objects = objects;
    source.sort_objects_by_digest();
    let scratch = trial_scratch("transfer-import-seal-history-");
    install_checkpoint_image(scratch.path(), &checkpoint_image_for_profile::<Profile>(source));
    scratch
}

fn destination_store() -> (PersistenceTrialDir, CatalogSlotV1) {
    let destination = trial_scratch("transfer-import-seal-destination-");
    let slot = create_empty_store_v1::<Profile>(destination.path(), store_limits())
        .expect("an empty directory receives the destination store");
    (destination, slot)
}

fn begin(
    store: &Path,
    binding: TransferBindingV1,
    limits: TransferPageLimitsV1,
) -> Result<Session, TransferImportErrorV1> {
    Session::begin_import(store, binding, limits, StructuralValidationLimits::unbounded())
}

fn resume(
    store: &Path,
    binding: TransferBindingV1,
    limits: TransferPageLimitsV1,
) -> Result<Session, TransferImportErrorV1> {
    Session::resume_import(store, binding, limits, StructuralValidationLimits::unbounded())
}

fn stage(session: &mut Session, page: &[u8]) -> TransferTokenV1 {
    match session.stage_page(page) {
        Ok(TransferStagingOutcomeV1::Staged(token)) => token,
        other => panic!("an object page of an honest stream is staged, not {other:?}"),
    }
}

fn stage_stream(session: &mut Session, pages: &[Vec<u8>]) -> TransferTokenV1 {
    for page in pages {
        match session.stage_page(page) {
            Ok(TransferStagingOutcomeV1::Staged(_)) => {}
            Ok(TransferStagingOutcomeV1::Complete(token)) => return token,
            Err(error) => panic!("the importer stages every page of an honest stream: {error}"),
        }
    }
    panic!("an honest stream ends with its terminal page")
}

fn admitted<S>(sealing: Sealing<S>, case: &str) -> S {
    match sealing {
        Ok(sealed) => sealed,
        Err(refused) => panic!("{case} must be sealed, not refused: {}", refused.1),
    }
}

fn refused<S>(sealing: Sealing<S>, admitted_case: &str) -> (Session, TransferImportErrorV1) {
    match sealing {
        Ok(_) => panic!("{admitted_case}"),
        Err(refused) => *refused,
    }
}

fn outside_refusal<T>(result: Result<T, TransferImportErrorV1>, case: &str) -> String {
    match result {
        Ok(_) => panic!("{case} must be refused"),
        Err(error) => error.to_string(),
    }
}

fn staging_path(store: &Path) -> PathBuf { store.join(STAGING) }

fn staged_files(store: &Path) -> StagedFiles {
    fs::read_dir(staging_path(store))
        .expect("list the staging directory")
        .map(|entry| {
            let entry = entry.expect("read a staging entry");
            (
                entry
                    .file_name()
                    .into_string()
                    .expect("staging names are UTF-8"),
                fs::read(entry.path()).expect("read a staged file"),
            )
        })
        .collect()
}

fn staging(session: &Session, store: &Path) -> Staging {
    Staging {
        frontier: session.frontier(),
        outstanding: session.outstanding(),
        files: staged_files(store),
    }
}

fn write_staged_log(store: &Path, bytes: &[u8]) {
    fs::write(staging_path(store).join(OBJECTS), bytes).expect("rewrite the staging log");
}

fn store_image(store: &Path) -> StoreImage {
    let staging = staging_path(store);
    let mut image = StoreImage::new();
    let mut pending = vec![store.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("list a directory of the store") {
            let path = entry.expect("read an entry of the store").path();
            if path == staging {
                continue;
            }
            let name = path
                .strip_prefix(store)
                .expect("an entry of the store lies below it")
                .to_path_buf();
            match path.is_dir() {
                true => {
                    image.insert(name, None);
                    pending.push(path);
                }
                false => {
                    image.insert(name, Some(fs::read(&path).expect("read a file of the store")));
                }
            }
        }
    }
    assert!(image.contains_key(Path::new("catalog.v1")), "the image of a store holds its catalog");
    image
}

fn trace_after_abort(
    store: &Path,
    binding: TransferBindingV1,
    limits: TransferPageLimitsV1,
) -> Trace {
    let staging_left = staging_path(store).exists();
    let image = store_image(store);
    let resumed = resume(store, binding, limits)
        .map(|session| session.frontier())
        .map_err(|error| error.to_string());
    let fresh = begin(store, binding, limits)
        .and_then(|session| {
            let state =
                (session.frontier(), session.outstanding(), staged_files(store)[OBJECTS].len());
            session.abort().map(|()| state)
        })
        .map_err(|error| error.to_string());
    Trace {
        staging_left,
        store: image,
        resumed,
        fresh,
    }
}

fn no_trace(store: &Path, binding: TransferBindingV1, store_before: &StoreImage) -> Trace {
    Trace {
        staging_left: false,
        store: store_before.clone(),
        resumed: Err(TransferImportErrorV1::NoStaging {
            path: staging_path(store),
        }
        .to_string()),
        fresh: Ok((TransferTokenV1::new(binding, 0), 0, 0)),
    }
}

fn read_u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + LENGTH_FIELD_LEN]
            .try_into()
            .expect("an eight-byte field"),
    )
}

fn record_offsets(log: &[u8], records: u64) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(records as usize);
    let mut offset = 0;
    while offset < log.len() {
        offsets.push(offset);
        offset += LENGTH_FIELD_LEN + read_u64_at(log, offset) as usize;
    }
    offsets
}

#[tokio::test]
async fn an_incomplete_closure_cannot_be_sealed() {
    let limits = page_limits();
    let unbounded = SemanticValidationLimits::unbounded();
    let stream = generated_stream(limits);
    let (binding, pages) = (stream.binding, &stream.pages);
    let object_pages = pages.len() - 1;
    assert!(object_pages >= 4, "the stream must span several object pages, not {object_pages}");
    let (destination, _) = destination_store();
    let store = destination.path();
    let store_before = store_image(store);

    let mut failing = begin(store, binding, limits).expect("begin an import");
    stage(&mut failing, &pages[0]);
    arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalWrite);
    let injected = matches!(
        failing.stage_page(&pages[1]),
        Err(TransferImportErrorV1::Io(DurableIoError::Io { .. }))
    );
    let failed_before = staging(&failing, store);
    let (mut failing, failed_refusal) =
        refused(failing.seal(unbounded), "a session that failed while staging a page was sealed");
    let failed_after = staging(&failing, store);
    let still_failed =
        matches!(failing.stage_page(&pages[1]), Err(TransferImportErrorV1::SessionFailed));
    failing.abort().expect("abort the failed import");
    assert!(
        injected &&
            matches!(failed_refusal, TransferImportErrorV1::SessionFailed) &&
            failed_after == failed_before &&
            still_failed,
        "a session that failed while staging a page is refused as failed and handed back as it \
         was, not {failed_refusal}"
    );

    let mut session = begin(store, binding, limits).expect("begin an import");
    let mut observed = Vec::with_capacity(object_pages);
    let mut expected = Vec::with_capacity(object_pages);
    for page in &pages[..object_pages] {
        let before = staging(&session, store);
        let (returned, refusal) = refused(
            session.seal(unbounded),
            "a staging that has not staged every announced object was sealed",
        );
        session = returned;
        observed.push((refusal_of(&refusal), staging(&session, store)));
        expected.push((
            Refusal::Incomplete {
                position: before.frontier.position(),
                outstanding: before.outstanding,
            },
            before,
        ));
        stage(&mut session, page);
    }
    assert!(
        matches!(&expected[0].0, Refusal::Incomplete {
            position: 0,
            outstanding: 0
        }) && expected[1..]
            .iter()
            .all(|(refusal, _)| matches!(refusal, Refusal::Incomplete {
                position: 1..,
                outstanding: 1..
            })),
        "the stream is incomplete at frontier 0 and after every object page but the last"
    );
    assert_eq!(
        observed, expected,
        "a staging that has not staged every announced object is refused as an incomplete closure \
         at its own position, and the refusal leaves the session and its staging unchanged"
    );

    let complete = staging(&session, store);
    let frontier = complete.frontier.position();
    let log = complete.files[OBJECTS].clone();
    let offsets = record_offsets(&log, frontier);
    let last_record = offsets[offsets.len() - 1];
    let last_object = ObjectDigest::of_canonical_bytes(&log[last_record + LENGTH_FIELD_LEN..]);
    let with_length = |offset: usize, len: u64| {
        let mut bytes = log.clone();
        bytes[offset..offset + LENGTH_FIELD_LEN].copy_from_slice(&len.to_le_bytes());
        bytes
    };
    let beyond_log = (log.len() - last_record - LENGTH_FIELD_LEN) as u64 + 1;
    let mut repeated_manifest = Vec::with_capacity(log.len() + offsets[1]);
    repeated_manifest.extend_from_slice(&log[..offsets[1]]);
    repeated_manifest.extend_from_slice(&log[..offsets[1]]);
    repeated_manifest.extend_from_slice(&log[offsets[2]..]);
    let mut flipped_last_object = log.clone();
    flipped_last_object[log.len() - 1] ^= 0x01;
    let damaged = [
        (log[..log.len() - 1].to_vec(), Refusal::ObjectsMissing {
            frontier,
            staged: frontier - 1,
        }),
        (log[..last_record + LENGTH_FIELD_LEN / 2].to_vec(), Refusal::ObjectsMissing {
            frontier,
            staged: frontier - 1,
        }),
        (with_length(last_record, beyond_log), Refusal::ObjectsMissing {
            frontier,
            staged: frontier - 1,
        }),
        (with_length(0, 31), Refusal::RecordLength {
            position: 0,
            len: 31,
            max_page_bytes: MAX_PAGE_BYTES,
        }),
        (with_length(0, MAX_PAGE_BYTES + 1), Refusal::RecordLength {
            position: 0,
            len: MAX_PAGE_BYTES + 1,
            max_page_bytes: MAX_PAGE_BYTES,
        }),
        (repeated_manifest, Refusal::Duplicate {
            position: 1,
            digest: binding.root_digest(),
        }),
        (flipped_last_object, Refusal::Missing(last_object)),
    ];
    let mut damaged_observed = Vec::with_capacity(damaged.len());
    for (bytes, _) in &damaged {
        write_staged_log(store, bytes);
        let (returned, refusal) = refused(
            session.seal(unbounded),
            "a staging log that does not hold the acknowledged closure was sealed",
        );
        session = returned;
        damaged_observed.push((
            refusal_of(&refusal),
            staged_files(store)[OBJECTS] == *bytes,
            session.frontier(),
            session.outstanding(),
        ));
    }
    write_staged_log(store, &log);
    assert_eq!(
        damaged_observed,
        damaged
            .into_iter()
            .map(|(_, refusal)| (refusal, true, complete.frontier, 0))
            .collect::<Vec<_>>(),
        "at the complete frontier a staging log that is short, holds a length outside 32 bytes to \
         the page limit, repeats an object or lacks an object of the closure is refused, and the \
         refusal leaves the log and the session unchanged"
    );

    let live = admitted(session.seal(unbounded), "a complete closure before its terminal page");
    let live_manifest = live.manifest();
    drop(live);
    let resumed = resume(store, binding, limits).expect("resume the complete staging");
    let resumed = admitted(resumed.seal(unbounded), "a complete closure of a resumed session");
    let resumed_manifest = resumed.manifest();
    drop(resumed);
    let mut terminal = resume(store, binding, limits).expect("resume the complete staging again");
    let completed = terminal.stage_page(&pages[object_pages]);
    let terminal = admitted(terminal.seal(unbounded), "a complete closure after its terminal page");
    assert!(
        matches!(
            completed,
            Ok(TransferStagingOutcomeV1::Complete(token)) if token == complete.frontier
        ),
        "the terminal page completes the stream, not {completed:?}"
    );
    assert_eq!(
        (live_manifest, resumed_manifest, terminal.manifest(), store_image(store)),
        (stream.manifest, stream.manifest, stream.manifest, store_before),
        "at the complete frontier the closure of the session binding is sealed, by the session \
         that staged it, by a resumed session and after the terminal page, and the store is \
         unchanged"
    );
}

#[tokio::test]
async fn a_complete_closure_is_admitted_and_one_with_a_dangling_join_is_refused() {
    let limits = page_limits();
    let unbounded = SemanticValidationLimits::unbounded();
    let complete = generated_stream(limits);
    let objects = objects_of(&complete.pages, limits);
    let canonical = memory_store(&objects);
    let admission = verify_rspace_semantics::<Profile, _>(&canonical, complete.manifest, unbounded)
        .expect("cold admission admits the generated state in memory");
    assert!(
        complete.pages.len() > 4 &&
            admission.structural.distinct_objects == objects.len() as u64 &&
            admission.join_entries == MEMBERSHIPS as u64 &&
            admission.continuation_entries == GROUPS.len() as u64,
        "the generated state spans several pages and holds the generated continuations and join \
         memberships: {admission:?}"
    );
    let dangling = states_without_one_join_membership(&canonical, complete.manifest);
    let history = history_store(&canonical, &dangling);
    let history_view = recover(history.path());
    let dangling_streams = dangling
        .iter()
        .map(|state| export_stream(&history_view, state.manifest, limits))
        .collect::<Vec<_>>();
    drop(history_view);
    drop(history);
    let (destination, genesis) = destination_store();
    let store = destination.path();
    let store_before = store_image(store);

    for (state, stream) in dangling.iter().zip(&dangling_streams) {
        let mut session =
            begin(store, stream.binding, limits).expect("begin the import of a dangling state");
        let completed = stage_stream(&mut session, &stream.pages);
        let staged = staged_files(store);
        let (mut session, refusal) =
            refused(session.seal(unbounded), "a staged state with a dangling join was sealed");
        assert!(
            matches!(
                &refusal,
                TransferImportErrorV1::ColdAdmission(error) if *error == state.refusal
            ),
            "the seal of a state with a dangling join is refused by cold admission as that state \
             is refused in memory, not for {refusal}"
        );
        let terminal = &stream.pages[stream.pages.len() - 1];
        assert_eq!(
            (
                store_image(store),
                staged_files(store),
                session.frontier(),
                session.outstanding(),
                session
                    .stage_page(terminal)
                    .map_err(|error| error.to_string())
            ),
            (
                store_before.clone(),
                staged,
                completed,
                0,
                Ok(TransferStagingOutcomeV1::Complete(completed))
            ),
            "the refused seal changes neither the store nor the staging and hands the session \
             back complete"
        );
        session.abort().expect("abort the refused import");
    }

    let mut session =
        begin(store, complete.binding, limits).expect("begin the import of the generated state");
    stage_stream(&mut session, &complete.pages);
    let staged = staged_files(store);
    let scarce = SemanticValidationLimits {
        max_endpoints: admission.endpoints - 1,
        ..unbounded
    };
    let scarce_refusal = memory_refusal(&canonical, complete.manifest, scarce);
    let (session, bounded_refusal) = refused(
        session.seal(scarce),
        "a closure beyond the admission limits passed to the seal was sealed",
    );
    assert!(
        matches!(scarce_refusal, CanonicalFormatError::ResourceLimit { .. }) &&
            matches!(
                &bounded_refusal,
                TransferImportErrorV1::ColdAdmission(error) if *error == scarce_refusal
            ),
        "cold admission runs within the limits passed to the seal, and beyond them refuses with \
         {scarce_refusal}, not {bounded_refusal}"
    );
    let sealed =
        admitted(session.seal(unbounded), "the completely staged export of the generated state");
    assert_eq!(
        (sealed.manifest(), sealed.admission(), store_image(store), staged_files(store)),
        (complete.manifest, admission, store_before, staged),
        "the sealed import holds the manifest of the binding and the report of the full cold \
         admission of its closure, and sealing changes neither the store nor the staging"
    );
    let staged_objects = sealed.store();
    let read_back = objects
        .iter()
        .map(|object| {
            let reference = reference_of(object);
            staged_objects
                .get_bounded(&reference.digest(), reference.encoded_len())
                .expect("read a staged object")
                .is_some_and(|read| *read == **object)
        })
        .collect::<Vec<_>>();
    let unknown = ObjectDigest::of_canonical_bytes(b"no object of the staged closure");
    let absent = staged_objects
        .get(&unknown)
        .expect("look up an unknown digest");
    let above_bound = staged_objects
        .get_bounded(&complete.manifest.digest(), complete.manifest.encoded_len() - 1)
        .map(|_| ());
    assert_eq!(
        (read_back, absent),
        (vec![true; objects.len()], None),
        "the staged-object store reads every object of the closure by its digest and no other"
    );
    assert!(
        matches!(
            above_bound,
            Err(TransferImportErrorV1::StagedObjectBound { digest, staged, limit })
                if digest == complete.manifest.digest()
                    && staged == complete.manifest.encoded_len()
                    && limit == staged - 1
        ),
        "the staged-object store refuses an object above the read bound before reading it"
    );
    let recovered = recover(store);
    assert_eq!(
        recovered.catalog(),
        genesis,
        "beside a sealed import the store recovers at its genesis slot: nothing is published"
    );
}

#[tokio::test]
async fn abort_before_and_after_sealing_leaves_no_trace() {
    let limits = page_limits();
    let unbounded = SemanticValidationLimits::unbounded();
    let stream = generated_stream(limits);
    let (binding, pages) = (stream.binding, &stream.pages);
    let object_pages = pages.len() - 1;
    assert!(object_pages >= 4, "the stream must span several object pages, not {object_pages}");
    let (destination, _) = destination_store();
    let store = destination.path();
    let store_before = store_image(store);

    let mut partial = begin(store, binding, limits).expect("begin an import");
    for page in &pages[..object_pages / 2] {
        stage(&mut partial, page);
    }
    let (partial, _) = refused(partial.seal(unbounded), "an incomplete staging was sealed");
    partial.abort().expect("abort the incomplete import");
    assert_eq!(
        trace_after_abort(store, binding, limits),
        no_trace(store, binding, &store_before),
        "aborting a session that a refused seal handed back removes its staging and leaves no \
         trace"
    );

    let mut unsealed = begin(store, binding, limits).expect("begin an import");
    stage_stream(&mut unsealed, pages);
    drop(unsealed);
    abort_import(store).expect("abort the complete staging from outside");
    assert_eq!(
        trace_after_abort(store, binding, limits),
        no_trace(store, binding, &store_before),
        "aborting a complete staging that was never sealed removes it and leaves no trace"
    );

    let mut session = begin(store, binding, limits).expect("begin an import");
    stage_stream(&mut session, pages);
    let staged = staged_files(store);
    let sealed = admitted(session.seal(unbounded), "a completely staged stream");
    let blocked = [
        outside_refusal(abort_import(store), "an abort of a sealed import from outside it"),
        outside_refusal(resume(store, binding, limits), "a resume beside a sealed import"),
        outside_refusal(begin(store, binding, limits), "a second import beside a sealed import"),
    ];
    let in_use = TransferImportErrorV1::StagingInUse {
        path: staging_path(store),
    }
    .to_string();
    let exists = TransferImportErrorV1::StagingExists {
        path: staging_path(store),
    }
    .to_string();
    assert_eq!(
        (blocked, staged_files(store), store_image(store)),
        ([in_use.clone(), in_use, exists], staged, store_before.clone()),
        "while a sealed import lives, nothing outside it aborts, resumes or replaces its staging, \
         and the staging and the store stay unchanged"
    );
    sealed.abort().expect("abort the sealed import");
    assert_eq!(
        trace_after_abort(store, binding, limits),
        no_trace(store, binding, &store_before),
        "aborting a sealed import removes its staging and leaves no trace"
    );

    let mut session = begin(store, binding, limits).expect("begin an import");
    let completed = stage_stream(&mut session, pages);
    let staged_again = staged_files(store);
    let sealed = admitted(session.seal(unbounded), "a completely staged stream");
    drop(sealed);
    let dropped = (staged_files(store), store_image(store));
    let resumed = resume(store, binding, limits).expect("resume after a sealed import was dropped");
    let resumed_at = (resumed.frontier(), resumed.outstanding());
    let resealed = admitted(resumed.seal(unbounded), "the staging a dropped sealed import left");
    assert_eq!(
        (dropped, resumed_at, resealed.manifest()),
        ((staged_again, store_before.clone()), (completed, 0), stream.manifest),
        "dropping a sealed import publishes nothing and leaves its staging byte for byte, which \
         is resumed complete and sealed again"
    );
    resealed.abort().expect("abort the import sealed again");
    assert_eq!(
        trace_after_abort(store, binding, limits),
        no_trace(store, binding, &store_before),
        "aborting an import that was sealed again after a drop removes its staging and leaves no \
         trace"
    );
}
