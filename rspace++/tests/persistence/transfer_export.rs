use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use tokio::sync::RwLock;

use super::durable_recovery::limits;
use super::durable_rspace_authority::{
    Profile, Store, fixture, poll_checkpoint_with, publication_limits,
};
use super::support::{PersistenceTrialDir, TEST_SEMANTIC_PROFILE_ID};
use crate::rspace::errors::RootError;
use crate::rspace::hot_store::{DurableHotStoreBootstrapLimits, HotStoreInstances};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableExportSessionV1, DurableStateReadHandleErrorV1,
    DurableStateRootV1, TransferBindingV1, TransferExportErrorV1, TransferPageFormatErrorV1,
    TransferPageLimitsV1, TransferPageViewV1, TransferTokenV1, recover_read_authority,
};
use crate::rspace::persistence::{
    BranchKind, CanonicalDagNodeView, CanonicalObjectKind, CanonicalObjectStore,
    CanonicalObjectView, DurableAuthorityViewV1, EndpointValue, GenerationLeaseKeyV1,
    LogPackDecodeLimits, MutableObjectStore, ObjectAdmission, ObjectDigest, ObjectRefV1,
    SemanticProfile, SemanticStateManifestView, StateVersionV1, verify_structural_closure,
};

const TEST_CRC32C: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);
const VERSION_OFFSET: usize = 0;
const ROOT_OFFSET: usize = 8;
const PROFILE_OFFSET: usize = 40;
const POSITION_OFFSET: usize = 56;
const COUNT_OFFSET: usize = 64;
const HEADER_LEN: usize = 72;
const LENGTH_FIELD_LEN: usize = 8;
const CHECKSUM_LEN: usize = 8;
const DEEP_VALUES: usize = 200;
const MEDIUM_VALUES: u8 = 40;
const SHARED_CHANNELS: &[&str] = &["x-shared", "y-shared"];
const SHARED_GROUPS: [&[&str]; 4] = [
    &["x-shared", "y-shared"],
    &["x-shared", "y-shared", "zeta"],
    &["x-shared", "y-shared", "omega-1"],
    &["x-shared", "y-shared", "omega-2"],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImportRefusal {
    Page(TransferPageFormatErrorV1),
    Binding {
        expected: TransferBindingV1,
        found: TransferBindingV1,
    },
    Position {
        expected: u64,
        found: u64,
    },
    NotParentFirst {
        position: u64,
    },
    Duplicate {
        position: u64,
    },
    LengthMismatch {
        position: u64,
    },
    NotCanonical {
        position: u64,
    },
    ForeignProfile {
        position: u64,
    },
    UnexpectedKind {
        position: u64,
    },
    IncompleteAtEnd {
        missing: u64,
    },
}

struct ImportedObject {
    depth: u64,
    bytes: Vec<u8>,
    references: Vec<ObjectRefV1>,
    kind: CanonicalObjectKind,
    branch: Option<BranchKind>,
}

struct ReferenceImporter {
    binding: TransferBindingV1,
    limits: TransferPageLimitsV1,
    next_position: u64,
    announced: BTreeMap<ObjectDigest, (u64, u64)>,
    received: BTreeMap<ObjectDigest, ImportedObject>,
    order: Vec<ObjectDigest>,
    incoming: BTreeMap<ObjectDigest, u64>,
    complete: bool,
}

impl ReferenceImporter {
    fn new(binding: TransferBindingV1, limits: TransferPageLimitsV1) -> Self {
        Self {
            binding,
            limits,
            next_position: 0,
            announced: BTreeMap::new(),
            received: BTreeMap::new(),
            order: Vec::new(),
            incoming: BTreeMap::new(),
            complete: false,
        }
    }

    fn token(&self) -> TransferTokenV1 { TransferTokenV1::new(self.binding, self.next_position) }

    fn accept(&mut self, page: &[u8]) -> Result<TransferTokenV1, ImportRefusal> {
        let view = TransferPageViewV1::parse(page, self.limits).map_err(ImportRefusal::Page)?;
        if view.binding() != self.binding {
            return Err(ImportRefusal::Binding {
                expected: self.binding,
                found: view.binding(),
            });
        }
        if view.position() != self.next_position {
            return Err(ImportRefusal::Position {
                expected: self.next_position,
                found: view.position(),
            });
        }
        match (view.object_count(), self.next_position, self.announced.is_empty()) {
            (0, 1.., true) => self.complete = true,
            (0, _, _) => {
                return Err(ImportRefusal::IncompleteAtEnd {
                    missing: self.announced.len() as u64,
                });
            }
            _ => {
                for object in view.objects() {
                    self.accept_object(object)?;
                }
            }
        }
        Ok(view.next_token())
    }

    fn accept_object(&mut self, object: &[u8]) -> Result<(), ImportRefusal> {
        let position = self.next_position;
        let digest = ObjectDigest::of_canonical_bytes(object);
        let encoded_len = object.len() as u64;
        if self.received.contains_key(&digest) {
            return Err(ImportRefusal::Duplicate { position });
        }
        let depth = match (position, self.announced.remove(&digest)) {
            (0, None) if digest == self.binding.root_digest() => 0,
            (1.., Some((announced_len, depth))) if announced_len == encoded_len => depth,
            (1.., Some(_)) => return Err(ImportRefusal::LengthMismatch { position }),
            _ => return Err(ImportRefusal::NotParentFirst { position }),
        };
        let parsed = CanonicalObjectView::parse(object)
            .map_err(|_| ImportRefusal::NotCanonical { position })?;
        let (references, branch) = match (position, parsed) {
            (0, CanonicalObjectView::SemanticManifest(manifest))
                if manifest.profile_id() == self.binding.profile() =>
            {
                (manifest_references(manifest), None)
            }
            (0, CanonicalObjectView::SemanticManifest(_)) => {
                return Err(ImportRefusal::ForeignProfile { position });
            }
            (1.., CanonicalObjectView::DagNode(node)) => {
                (node_references(node), Some(node.children().kind()))
            }
            (1.., CanonicalObjectView::Blob(_)) => (Vec::new(), None),
            _ => return Err(ImportRefusal::UnexpectedKind { position }),
        };
        for reference in &references {
            let target = reference.digest();
            *self.incoming.entry(target).or_insert(0) += 1;
            let known_len = match (self.received.get(&target), self.announced.get(&target)) {
                (Some(received), _) => Some(received.bytes.len() as u64),
                (None, Some((announced_len, _))) => Some(*announced_len),
                (None, None) => None,
            };
            match known_len {
                Some(len) if len != reference.encoded_len() => {
                    return Err(ImportRefusal::LengthMismatch { position });
                }
                Some(_) => {}
                None => {
                    self.announced
                        .insert(target, (reference.encoded_len(), depth + 1));
                }
            }
        }
        self.received.insert(digest, ImportedObject {
            depth,
            bytes: object.to_vec(),
            references,
            kind: parsed.kind(),
            branch,
        });
        self.order.push(digest);
        self.next_position += 1;
        Ok(())
    }
}

struct OracleClosure {
    order: Vec<ObjectDigest>,
    objects: BTreeMap<ObjectDigest, Vec<ObjectRefV1>>,
    canonical_bytes: u64,
    max_frontier: usize,
    max_node_depth: u64,
}

fn recover(scratch: &PersistenceTrialDir) -> DurableAuthorityViewV1<Profile> {
    recover_read_authority::<Profile>(
        scratch.path(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("restart-grade admission recovers the store")
}

fn page_limits(max_page_bytes: u64, max_page_objects: u64) -> TransferPageLimitsV1 {
    TransferPageLimitsV1::new(max_page_bytes, max_page_objects)
        .expect("the test page limits admit at least one object")
}

fn root_of(state: StateVersionV1) -> DurableStateRootV1 {
    DurableStateRootV1::of_manifest(state.manifest())
}

fn owned(channels: &[&str]) -> Vec<String> {
    channels
        .iter()
        .map(|channel| (*channel).to_owned())
        .collect()
}

fn put_datum(hot: &Store, channel: &str, value: &str) {
    let channel = channel.to_owned();
    hot.put_datum(&channel, Datum::create(&channel, value.to_owned(), false));
}

fn put_continuation(hot: &Store, group: &[&str], body: &str) {
    let channels = owned(group);
    let patterns = channels
        .iter()
        .map(|channel| format!("{channel}-pattern"))
        .collect::<Vec<_>>();
    let continuation =
        WaitingContinuation::create(&channels, &patterns, &body.to_owned(), false, BTreeSet::new());
    assert_eq!(
        hot.put_continuation(&channels, continuation),
        Some(true),
        "the hot store stores a continuation it did not hold"
    );
}

fn publish_values(
    channel: &str,
    values: impl IntoIterator<Item = String>,
) -> (PersistenceTrialDir, StateVersionV1) {
    let (scratch, hot, gate, mut owner) = fixture();
    for value in values {
        put_datum(&hot, channel, &value);
    }
    let state = poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .expect("the owner publishes the state");
    drop(owner);
    (scratch, state)
}

fn distinct_lead_byte_values() -> Vec<String> {
    let mut values = BTreeMap::new();
    let mut buffer = [0_u8; 4];
    for character in (1..=u32::from(char::MAX)).filter_map(char::from_u32) {
        let lead = character.encode_utf8(&mut buffer).as_bytes()[0];
        values.entry(lead).or_insert_with(|| character.to_string());
    }
    values.into_values().collect()
}

fn manifest_references(manifest: SemanticStateManifestView) -> Vec<ObjectRefV1> {
    manifest
        .roots()
        .iter()
        .into_iter()
        .map(|(_, root)| root)
        .collect()
}

fn node_references(node: CanonicalDagNodeView<'_>) -> Vec<ObjectRefV1> {
    let children = node.children();
    let mut references = Vec::with_capacity(children.len() + 1);
    if let EndpointValue::Content(content) = node.endpoint() {
        references.push(content);
    }
    references.extend(children.iter().map(|child| child.target));
    references
}

fn closure_of(view: &DurableAuthorityViewV1<Profile>, manifest: ObjectRefV1) -> OracleClosure {
    let mut objects = BTreeMap::new();
    let mut order = Vec::new();
    let mut canonical_bytes = 0_u64;
    let mut pending = vec![(manifest, 0_u64)];
    let mut max_frontier = pending.len();
    let mut max_node_depth = 0_u64;
    while let Some((reference, depth)) = pending.pop() {
        if objects.contains_key(&reference.digest()) {
            continue;
        }
        let bytes = view
            .get_object_bounded(&reference.digest(), reference.encoded_len())
            .expect("read the object index")
            .expect("every object of a retained closure is indexed");
        assert_eq!(
            (ObjectDigest::of_canonical_bytes(&bytes), bytes.len() as u64),
            (reference.digest(), reference.encoded_len()),
            "the object index returns exactly the referenced object"
        );
        let references = match CanonicalObjectView::parse(&bytes)
            .expect("every object of a retained closure is canonical")
        {
            CanonicalObjectView::SemanticManifest(state) => manifest_references(state),
            CanonicalObjectView::DagNode(node) => {
                max_node_depth = max_node_depth.max(depth);
                node_references(node)
            }
            CanonicalObjectView::Blob(_) => Vec::new(),
            other => panic!("a state closure holds no {} object", other.kind().name()),
        };
        pending.extend(references.iter().rev().map(|target| (*target, depth + 1)));
        max_frontier = max_frontier.max(pending.len());
        canonical_bytes += reference.encoded_len();
        order.push(reference.digest());
        objects.insert(reference.digest(), references);
    }
    OracleClosure {
        order,
        objects,
        canonical_bytes,
        max_frontier,
        max_node_depth,
    }
}

fn union_reference_counts(
    closures: &[OracleClosure],
    manifests: &[ObjectRefV1],
) -> BTreeMap<ObjectDigest, u64> {
    let mut union = BTreeMap::new();
    for closure in closures {
        for (digest, references) in &closure.objects {
            union.entry(*digest).or_insert(references);
        }
    }
    let mut counts = BTreeMap::new();
    for manifest in manifests {
        *counts.entry(manifest.digest()).or_insert(0_u64) += 1;
    }
    for references in union.values() {
        for reference in references.iter() {
            *counts.entry(reference.digest()).or_insert(0_u64) += 1;
        }
    }
    counts
}

fn import_stream(
    session: &mut DurableExportSessionV1<Profile>,
    importer: &mut ReferenceImporter,
) -> Result<Vec<Vec<u8>>, ImportRefusal> {
    let mut pages = Vec::new();
    while !importer.complete {
        let page = session
            .page(importer.token())
            .expect("the export session serves every position the importer requests")
            .to_vec();
        importer.accept(&page)?;
        pages.push(page);
    }
    Ok(pages)
}

fn import_pages(
    binding: TransferBindingV1,
    limits: TransferPageLimitsV1,
    pages: &[Vec<u8>],
) -> Result<u64, ImportRefusal> {
    let mut importer = ReferenceImporter::new(binding, limits);
    for page in pages {
        importer.accept(page)?;
    }
    match importer.complete {
        true => Ok(importer.next_position),
        false => Err(ImportRefusal::IncompleteAtEnd {
            missing: importer.announced.len() as u64,
        }),
    }
}

fn stream_objects(importer: &ReferenceImporter) -> Vec<Vec<u8>> {
    importer
        .order
        .iter()
        .map(|digest| importer.received[digest].bytes.clone())
        .collect()
}

fn imported_store(importer: &ReferenceImporter) -> CanonicalObjectStore {
    let store = CanonicalObjectStore::new();
    for (digest, object) in &importer.received {
        let reference = ObjectRefV1::new(*digest, object.bytes.len() as u64)
            .expect("an imported object has a valid reference");
        assert_eq!(
            store
                .put_if_absent_or_equal(reference, Arc::from(object.bytes.as_slice()))
                .expect("the in-memory store admits an imported object"),
            ObjectAdmission::Inserted,
            "every imported object is distinct"
        );
    }
    store
}

fn encode_page_at(binding: TransferBindingV1, position: u64, objects: &[Vec<u8>]) -> Vec<u8> {
    let records = objects
        .iter()
        .map(|object| LENGTH_FIELD_LEN + object.len())
        .sum::<usize>();
    let mut page = Vec::with_capacity(HEADER_LEN + records + CHECKSUM_LEN);
    page.extend_from_slice(&1_u64.to_le_bytes());
    page.extend_from_slice(binding.root_digest().as_bytes());
    page.extend_from_slice(binding.profile().as_bytes());
    page.extend_from_slice(&position.to_le_bytes());
    page.extend_from_slice(&(objects.len() as u64).to_le_bytes());
    for object in objects {
        page.extend_from_slice(&(object.len() as u64).to_le_bytes());
        page.extend_from_slice(object);
    }
    let checksum = TEST_CRC32C.checksum(&page);
    page.extend_from_slice(&u64::from(checksum).to_le_bytes());
    page
}

fn greedy_pages(
    binding: TransferBindingV1,
    objects: &[Vec<u8>],
    start: usize,
    limits: TransferPageLimitsV1,
) -> Vec<Vec<u8>> {
    let max_page_bytes =
        usize::try_from(limits.max_page_bytes()).expect("the test page limit fits usize");
    let max_page_objects =
        usize::try_from(limits.max_page_objects()).expect("the test object limit fits usize");
    let mut pages = Vec::new();
    let mut position = start;
    loop {
        let mut page_len = HEADER_LEN + CHECKSUM_LEN;
        let mut end = position;
        while let Some(object) = objects.get(end) {
            if end - position == max_page_objects ||
                page_len + LENGTH_FIELD_LEN + object.len() > max_page_bytes
            {
                break;
            }
            page_len += LENGTH_FIELD_LEN + object.len();
            end += 1;
        }
        pages.push(encode_page_at(binding, position as u64, &objects[position..end]));
        match (end == position, end == objects.len()) {
            (true, true) => return pages,
            (true, false) => panic!("object {position} does not fit an empty test page"),
            (false, _) => position = end,
        }
    }
}

fn reseal(page: &mut [u8]) {
    let body_len = page.len() - CHECKSUM_LEN;
    let checksum = TEST_CRC32C.checksum(&page[..body_len]);
    page[body_len..].copy_from_slice(&u64::from(checksum).to_le_bytes());
}

fn patched(page: &[u8], offset: usize, value: u64) -> Vec<u8> {
    let mut patched = page.to_vec();
    patched[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    reseal(&mut patched);
    patched
}

#[tokio::test]
async fn pages_carry_exactly_the_closure_once_parent_first_for_deep_wide_and_shared_states() {
    let (scratch, hot, gate, mut owner) = fixture();
    let checkpoint = owner.state(&hot).expect("read the checkpoint state");
    for length in 1..=DEEP_VALUES {
        put_datum(&hot, "deep-channel", &"a".repeat(length));
    }
    let deep = poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .expect("the owner publishes the deep state");
    let wide_values = distinct_lead_byte_values();
    for value in &wide_values {
        put_datum(&hot, "wide-channel", value);
    }
    for offset in 0..MEDIUM_VALUES {
        put_datum(&hot, "medium-channel", &char::from(b'A' + offset).to_string());
    }
    let wide = poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .expect("the owner publishes the wide state");
    for channel in SHARED_CHANNELS {
        for group in SHARED_GROUPS {
            hot.put_join(&(*channel).to_owned(), &owned(group))
                .expect("the hot store records the join");
        }
    }
    for body in ["first-shared-body", "second-shared-body"] {
        put_continuation(&hot, SHARED_CHANNELS, body);
    }
    let shared = poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .expect("the owner publishes the shared state");
    drop(owner);

    let view = recover(&scratch);
    let manifests = [checkpoint, deep, wide, shared].map(|state| state.manifest());
    let closures = manifests.map(|manifest| closure_of(&view, manifest));
    let counts = union_reference_counts(&closures, &manifests);
    let limits = page_limits(8192, 16);
    let mut observed = Vec::with_capacity(manifests.len());
    let mut expected = Vec::with_capacity(manifests.len());
    let mut footprints = Vec::with_capacity(manifests.len());
    let mut page_counts = Vec::with_capacity(manifests.len());
    let mut imports = Vec::with_capacity(manifests.len());
    for (manifest, closure) in manifests.iter().zip(&closures) {
        let mut session = view
            .open_export(DurableStateRootV1::of_manifest(*manifest), limits)
            .expect("open an export of a retained state");
        let mut importer = ReferenceImporter::new(session.binding(), limits);
        let pages = import_stream(&mut session, &mut importer)
            .expect("the reference importer accepts every page of a retained state");
        let (page_capacity, frontier_capacity, recorded_shared) = session.footprint_for_test();
        let structural = verify_structural_closure(&imported_store(&importer), *manifest)
            .expect("the imported objects form a structurally closed state");
        let counts_per_page = pages
            .iter()
            .map(|page| {
                TransferPageViewV1::parse(page, limits)
                    .expect("an exported page parses")
                    .object_count()
            })
            .collect::<Vec<_>>();
        observed.push((
            importer.order.clone(),
            structural.distinct_objects,
            structural.canonical_bytes,
            recorded_shared,
        ));
        expected.push((
            closure.order.clone(),
            closure.order.len() as u64,
            closure.canonical_bytes,
            closure
                .objects
                .keys()
                .filter(|digest| counts[*digest] != 1)
                .count(),
        ));
        footprints.push((
            pages.iter().map(Vec::len).max().unwrap_or(0),
            counts_per_page.iter().copied().max().unwrap_or(0),
            page_capacity,
            closure.max_frontier,
            closure.max_node_depth,
            frontier_capacity,
        ));
        page_counts.push(counts_per_page);
        imports.push(importer);
    }

    let deepest = imports[1]
        .received
        .values()
        .map(|object| object.depth)
        .max()
        .unwrap_or(0);
    let branch_classes = [BranchKind::Node256, BranchKind::Node48].map(|kind| {
        imports[2]
            .received
            .values()
            .any(|object| object.branch == Some(kind))
    });
    let shared_with_descendants = imports[3]
        .received
        .iter()
        .filter(|(digest, object)| {
            matches!(imports[3].incoming.get(*digest).copied(), Some(2..)) &&
                object.kind == CanonicalObjectKind::DagNode &&
                !object.references.is_empty()
        })
        .count();
    let shared_across_states = closures[1]
        .objects
        .iter()
        .filter(|(digest, references)| {
            closures[2].objects.contains_key(*digest) && !references.is_empty()
        })
        .count();
    let content_blobs = imports[3]
        .received
        .values()
        .filter(|object| object.kind == CanonicalObjectKind::Blob)
        .count();
    let count_bound_pages = page_counts
        .iter()
        .flatten()
        .filter(|count| **count == 16)
        .count();
    let byte_bound_pages = page_counts
        .iter()
        .map(|counts| {
            counts[..counts.len().saturating_sub(2)]
                .iter()
                .filter(|count| **count < 16)
                .count()
        })
        .sum::<usize>();

    assert_eq!(
        (view.report().retained_states, wide_values.len(), closures[0].order.len()),
        (4, 178, 2),
        "the store retains the checkpoint and the three published states, the wide channel holds \
         one datum per UTF-8 lead byte, and the checkpoint's three namespace roots are one object"
    );
    assert_eq!(
        observed, expected,
        "each state's pages carry exactly its closure, each object once, in the parent-first \
         export order, and the shared record holds exactly the objects whose retained reference \
         count is not one"
    );
    for (state, footprint) in footprints.iter().enumerate() {
        let (max_page_len, max_page_count, page_capacity, max_frontier, depth, frontier_capacity) =
            *footprint;
        assert!(
            max_page_len <= 8192 &&
                max_page_count <= 16 &&
                page_capacity <= 8192 &&
                max_frontier as u64 <= 255 * depth + 4 &&
                frontier_capacity <= (2 * (max_frontier + 2)).max(4),
            "state {state}: pages of at most {max_page_len} bytes and {max_page_count} objects, a \
             page buffer of {page_capacity} bytes, a frontier of at most {max_frontier} \
             references at node depth {depth} and a frontier capacity of {frontier_capacity} must \
             stay within the page limits and the frontier bound"
        );
    }
    assert_eq!(
        (
            deepest >= 200,
            branch_classes,
            shared_with_descendants > 0,
            shared_across_states > 0,
            content_blobs,
            count_bound_pages > 0,
            byte_bound_pages > 0
        ),
        (true, [true, true], true, true, 2, true, true),
        "the states must reach node depth 200 (reached {deepest}), hold a Node256 and a Node48 \
         branch, an object with two incoming references and children ({shared_with_descendants}), \
         nodes shared across states ({shared_across_states}), two content blobs \
         ({content_blobs}), and pages bounded by count ({count_bound_pages}) and by bytes \
         ({byte_bound_pages})"
    );
}

#[tokio::test]
async fn the_session_binding_is_the_root_digest_and_profile_and_other_bindings_are_refused() {
    let (scratch, hot, gate, mut owner) = fixture();
    let mut states = Vec::with_capacity(2);
    for value in ["first-bound-value", "second-bound-value"] {
        put_datum(&hot, "binding-channel", value);
        states.push(
            poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
                .expect("the owner publishes a state"),
        );
    }
    drop(owner);
    let limits = page_limits(2048, 8);
    let view = recover(&scratch);
    let view_key = GenerationLeaseKeyV1::from_catalog(view.catalog());
    let roots = [root_of(states[0]), root_of(states[1])];
    let mut sessions = roots.map(|root| {
        view.open_export(root, limits)
            .expect("open an export of a retained state")
    });
    let bindings = sessions.each_ref().map(|session| session.binding());
    let headers = sessions.each_mut().map(|session| {
        let token = session.first_token();
        let page = session.page(token).expect("serve the first page");
        let parsed = TransferPageViewV1::parse(page, limits).expect("an exported page parses");
        (
            parsed.binding(),
            parsed.position(),
            page[ROOT_OFFSET..PROFILE_OFFSET].to_vec(),
            page[PROFILE_OFFSET..POSITION_OFFSET].to_vec(),
        )
    });
    let unknown_root = DurableStateRootV1::of_manifest(
        ObjectRefV1::new(ObjectDigest::from_array([0xA5; 32]), 64)
            .expect("a 64-byte reference is valid"),
    );
    let foreign_bindings = [
        bindings[1],
        TransferBindingV1::new(roots[0], TEST_SEMANTIC_PROFILE_ID),
        TransferBindingV1::new(unknown_root, Profile::ID),
    ];
    let refusals =
        foreign_bindings.map(|binding| match sessions[0].page(TransferTokenV1::new(binding, 0)) {
            Err(TransferExportErrorV1::BindingMismatch { session, requested }) => {
                Some((session, requested))
            }
            _ => None,
        });
    let unknown_refused = matches!(
        view.open_export(unknown_root, limits),
        Err(TransferExportErrorV1::Open(DurableStateReadHandleErrorV1::Root(
            RootError::UnknownRootError(_)
        )))
    );
    let mut first_importer = ReferenceImporter::new(bindings[0], limits);
    let first_stream = import_stream(&mut sessions[0], &mut first_importer)
        .expect("the importer accepts the first state's stream");

    let owner_hot: Store = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .expect("bootstrap the hot store from the view"),
    );
    let owner_gate = Arc::new(RwLock::new(()));
    let mut owner = view
        .into_rspace_authority(&owner_hot)
        .unwrap_or_else(|failure| panic!("the view becomes the owner: {:?}", failure.1));
    put_datum(&owner_hot, "binding-channel", "third-bound-value");
    let third = poll_checkpoint_with(&mut owner, &owner_gate, &owner_hot, publication_limits())
        .expect("the owner publishes a third state");
    drop(owner);
    let reopened = recover(&scratch);
    let reopened_key = GenerationLeaseKeyV1::from_catalog(reopened.catalog());
    let mut later_session = reopened
        .open_export(roots[0], limits)
        .expect("the reopened store exports the first state at its later generation");
    let mut later_importer = ReferenceImporter::new(bindings[0], limits);
    let later_stream = import_stream(&mut later_session, &mut later_importer)
        .expect("the importer accepts the first state's stream at the later generation");

    assert_eq!(
        bindings,
        [0_usize, 1].map(|index| TransferBindingV1::new(roots[index], Profile::ID)),
        "a session binds the state root it was opened at and the view's profile"
    );
    assert_eq!(
        headers,
        [0_usize, 1].map(|index| (
            bindings[index],
            0,
            states[index].manifest().digest().as_bytes().to_vec(),
            Profile::ID.as_bytes().to_vec()
        )),
        "every page carries the manifest digest and the profile id as its binding"
    );
    assert_eq!(
        refusals,
        foreign_bindings.map(|requested| Some((bindings[0], requested))),
        "a token bound to another root, to the same root under another profile, or to a root the \
         store does not retain is refused"
    );
    assert!(unknown_refused, "an export of a root the store does not retain is refused");
    assert_eq!(
        (
            later_session.binding(),
            later_stream == first_stream,
            view_key != reopened_key,
            third.generation() > states[1].generation()
        ),
        (bindings[0], true, true, true),
        "the binding is exactly the root digest and the profile: at a later catalog generation \
         the same state has the same binding and the same pages"
    );
}

#[tokio::test]
async fn a_pinned_state_stays_servable_while_successors_are_published() {
    let (scratch, pinned) =
        publish_values("pinned-channel", (0..64).map(|index| format!("pinned-{index:02}")));
    let limits = page_limits(2048, 8);
    let view = recover(&scratch);
    let oracle = closure_of(&view, pinned.manifest());
    let registry = view.generation_leases();
    let key = GenerationLeaseKeyV1::from_catalog(view.catalog());
    let leases_before = registry.active_lease_count_for_test();
    let mut session = view
        .open_export(root_of(pinned), limits)
        .expect("open an export of the current state");
    let binding = session.binding();
    let session_leases = registry.active_lease_count_for_test() - leases_before;
    let mut importer = ReferenceImporter::new(binding, limits);
    let first_page = session
        .page(importer.token())
        .expect("serve the first page")
        .to_vec();
    importer
        .accept(&first_page)
        .expect("the importer accepts the first page");

    let hot: Store = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .expect("bootstrap the hot store from the view"),
    );
    let gate = Arc::new(RwLock::new(()));
    let mut owner = view
        .into_rspace_authority(&hot)
        .unwrap_or_else(|failure| panic!("the view becomes the owner: {:?}", failure.1));
    let successors = ["first-successor", "second-successor"].map(|value| {
        put_datum(&hot, "pinned-channel", value);
        poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
            .expect("the owner publishes a successor")
    });
    let pinned_while_published = registry
        .has_active(key)
        .expect("query the pinned generation's lease");
    let rest = import_stream(&mut session, &mut importer)
        .expect("the importer accepts the pages served after the successors");
    drop(owner);
    let reopened = recover(&scratch);
    let reopened_state = reopened.state();
    drop(reopened);
    let reclaimable = registry
        .retire_pack_range_if_unleased(key)
        .expect("decide whether the pinned generation is reclaimable");
    let leases_after_teardown = registry.active_lease_count_for_test();
    let mut replay_importer = ReferenceImporter::new(binding, limits);
    let replay = import_stream(&mut session, &mut replay_importer)
        .expect("the session serves its whole stream again");
    session.release_pin();
    let after_release = session
        .page(TransferTokenV1::new(binding, 0))
        .map(<[u8]>::to_vec);
    let leases_after_release = registry.active_lease_count_for_test();
    let pinned_after_release = registry
        .has_active(key)
        .expect("query the pinned generation's lease");
    let mut stream = Vec::with_capacity(rest.len() + 1);
    stream.push(first_page);
    stream.extend(rest);

    assert_eq!(
        (
            session_leases,
            pinned_while_published,
            successors.map(|state| state.generation()),
            reopened_state
        ),
        (1, true, [pinned.generation() + 1, pinned.generation() + 2], successors[1]),
        "the session holds one lease on its generation while the owner publishes two successors"
    );
    assert_eq!(
        importer.order, oracle.order,
        "the pages served while successors are published carry the pinned state's closure"
    );
    assert_eq!(
        (reclaimable, leases_after_teardown, replay == stream),
        (false, 1, true),
        "after owner teardown and a reopen, the session's lease alone keeps the pinned generation \
         from reclamation and the session serves the same pages again"
    );
    assert!(
        matches!(
            after_release,
            Err(TransferExportErrorV1::PinReleased { binding: released }) if released == binding
        ),
        "a session refuses to serve once its pin is released: {after_release:?}"
    );
    assert_eq!(
        (leases_after_release, pinned_after_release),
        (0, false),
        "releasing the pin returns the session's lease"
    );
}

#[tokio::test]
async fn serving_is_refused_after_the_pin_is_released_and_resume_tokens_do_not_survive_a_reopen() {
    let (scratch, state) =
        publish_values("resume-channel", (0..32).map(|index| format!("resume-{index:02}")));
    let limits = page_limits(2048, 4);
    let root = root_of(state);
    let view = recover(&scratch);
    let mut session = view
        .open_export(root, limits)
        .expect("open an export of the current state");
    let mut importer = ReferenceImporter::new(session.binding(), limits);
    let mut served = Vec::with_capacity(2);
    for _ in 0..2 {
        let page = session
            .page(importer.token())
            .expect("serve a page")
            .to_vec();
        importer
            .accept(&page)
            .expect("the importer accepts the page");
        served.push(page);
    }
    let resume = importer.token();
    drop(view);
    let reopened = recover(&scratch);
    let mut fresh = reopened
        .open_export(root, limits)
        .expect("the reopened store exports the same state");
    let refused_resume = fresh.page(resume).map(<[u8]>::to_vec);
    let fresh_first = fresh
        .page(fresh.first_token())
        .expect("the fresh session serves position 0")
        .to_vec();
    let pinned_resume = session
        .page(resume)
        .expect("the session opened before the reopen still holds its pin")
        .to_vec();
    let resumed = importer
        .accept(&pinned_resume)
        .map(|token| token.position() > resume.position());
    let first_token = session.first_token();
    session.release_pin();
    let released = [resume, first_token].map(|token| session.page(token).map(<[u8]>::to_vec));

    assert!(
        matches!(
            refused_resume,
            Err(TransferExportErrorV1::PositionNotServed { requested, served_end: 0 })
                if requested == resume.position() && requested > 0
        ),
        "a session opened after a reopen refuses a resume token of an earlier session: \
         {refused_resume:?}"
    );
    assert_eq!(
        (fresh_first == served[0], resumed),
        (true, Ok(true)),
        "the fresh session serves the same first page, and the earlier session, which still holds \
         its own pin, serves its resume position"
    );
    assert!(
        released.iter().all(|outcome| matches!(
            outcome,
            Err(TransferExportErrorV1::PinReleased { binding }) if *binding == fresh.binding()
        )),
        "a session whose pin is released refuses every position: {released:?}"
    );
}

#[tokio::test]
async fn resume_positions_serve_the_same_stream() {
    let (scratch, state) =
        publish_values("rewind-channel", (0..48).map(|index| format!("rewind-{index:02}")));
    let limits = page_limits(2048, 4);
    let view = recover(&scratch);
    let mut session = view
        .open_export(root_of(state), limits)
        .expect("open an export of the current state");
    let binding = session.binding();
    let mut importer = ReferenceImporter::new(binding, limits);
    let pages =
        import_stream(&mut session, &mut importer).expect("the importer accepts the whole stream");
    let objects = stream_objects(&importer);
    let positions = pages
        .iter()
        .map(|page| {
            TransferPageViewV1::parse(page, limits)
                .expect("an exported page parses")
                .position()
        })
        .collect::<Vec<_>>();
    let end = objects.len() as u64;
    let requests = [positions[2], positions[1] + 1, positions[positions.len() - 2], end, 0, 0];
    let served = requests.map(|position| {
        session
            .page(TransferTokenV1::new(binding, position))
            .expect("the session serves a position it has served")
            .to_vec()
    });
    let expected = requests.map(|position| {
        greedy_pages(
            binding,
            &objects,
            usize::try_from(position).expect("a stream position fits usize"),
            limits,
        )
        .swap_remove(0)
    });
    let beyond = session
        .page(TransferTokenV1::new(binding, end + 1))
        .map(<[u8]>::to_vec);

    assert!(
        positions.len() > 4 && positions[2] - positions[1] >= 2,
        "the stream spans several pages of several objects: {positions:?}"
    );
    assert_eq!(
        greedy_pages(binding, &objects, 0, limits),
        pages,
        "the test encoder, written from the layout table, reproduces every exported page byte for \
         byte"
    );
    assert_eq!(
        served, expected,
        "an earlier page start, a position inside a page, the last page, the end of the stream \
         and the start, twice, serve the pages the layout and the export order define"
    );
    assert!(
        matches!(
            beyond,
            Err(TransferExportErrorV1::PositionNotServed { requested, served_end })
                if requested == end + 1 && served_end == end
        ),
        "a position beyond the end of the stream is refused: {beyond:?}"
    );
}

#[tokio::test]
async fn a_child_first_order_is_rejected_by_the_reference_importer() {
    let (scratch, state) =
        publish_values("order-channel", (0..24).map(|index| format!("order-{index:02}")));
    let limits = page_limits(4096, 8);
    let view = recover(&scratch);
    let mut session = view
        .open_export(root_of(state), limits)
        .expect("open an export of the current state");
    let binding = session.binding();
    let mut importer = ReferenceImporter::new(binding, limits);
    let pages = import_stream(&mut session, &mut importer)
        .expect("the importer accepts the exported order");
    let objects = stream_objects(&importer);
    let second_is_child_of_first = importer.received[&importer.order[1]]
        .references
        .iter()
        .any(|reference| reference.digest() == importer.order[2]);
    let mut child_first = objects.clone();
    child_first.reverse();
    let mut swapped = objects.clone();
    swapped.swap(1, 2);
    let mut repeated = objects.clone();
    repeated.insert(2, objects[1].clone());
    let mut truncated = objects.clone();
    truncated.pop();
    let outcomes = [objects.clone(), child_first, swapped, repeated, truncated]
        .map(|order| import_pages(binding, limits, &greedy_pages(binding, &order, 0, limits)));

    assert_eq!(
        greedy_pages(binding, &objects, 0, limits),
        pages,
        "the test encoder reproduces every exported page byte for byte"
    );
    assert!(second_is_child_of_first, "the third object of the stream is a child of the second");
    assert_eq!(
        outcomes,
        [
            Ok(objects.len() as u64),
            Err(ImportRefusal::NotParentFirst { position: 0 }),
            Err(ImportRefusal::NotParentFirst { position: 1 }),
            Err(ImportRefusal::Duplicate { position: 2 }),
            Err(ImportRefusal::IncompleteAtEnd { missing: 1 }),
        ],
        "the importer accepts the exported order and refuses the child-first order, a child \
         before its parent, an object twice and a missing object"
    );
}

#[tokio::test]
async fn every_page_length_is_untrusted_bounded_and_checked() {
    let (scratch, state) =
        publish_values("decoder-channel", (0..12).map(|index| format!("decoder-{index:02}")));
    let serve_limits = page_limits(4096, 4);
    let parse_limits = page_limits(4096, 8);
    let view = recover(&scratch);
    let mut session = view
        .open_export(root_of(state), serve_limits)
        .expect("open an export of the current state");
    let binding = session.binding();
    let page = session
        .page(session.first_token())
        .expect("serve the first page")
        .to_vec();
    let parsed = TransferPageViewV1::parse(&page, parse_limits).expect("an exported page parses");
    let count = parsed.object_count();
    let last_record = (LENGTH_FIELD_LEN +
        parsed
            .objects()
            .last()
            .expect("the first page holds objects")
            .len()) as u64;
    let page_len = page.len() as u64;
    let checksum_field = u64::from_le_bytes(
        page[page.len() - CHECKSUM_LEN..]
            .try_into()
            .expect("the page ends with an eight-byte checksum field"),
    );
    let mut wide_checksum = page.clone();
    let high_half = page.len() - 4;
    wide_checksum[high_half] = 1;
    let mut flipped = page.clone();
    flipped[HEADER_LEN + LENGTH_FIELD_LEN] ^= 1;
    let mut foreign_root = page.clone();
    foreign_root[ROOT_OFFSET] ^= 1;
    reseal(&mut foreign_root);
    let foreign_binding = TransferPageViewV1::parse(&foreign_root, parse_limits)
        .expect("a resealed page with another root parses")
        .binding();
    let cases = [
        (page[..79].to_vec(), parse_limits),
        (page.clone(), page_limits(page_len - 1, 8)),
        (wide_checksum, parse_limits),
        (flipped.clone(), parse_limits),
        (patched(&page, VERSION_OFFSET, 2), parse_limits),
        (patched(&page, COUNT_OFFSET, 9), parse_limits),
        (patched(&page, COUNT_OFFSET, count + 1), parse_limits),
        (patched(&page, HEADER_LEN, 31), parse_limits),
        (patched(&page, HEADER_LEN, u64::MAX), parse_limits),
        (patched(&page, HEADER_LEN, page_len - 87), parse_limits),
        (patched(&page, COUNT_OFFSET, count - 1), parse_limits),
        (patched(&page, POSITION_OFFSET, u64::MAX), parse_limits),
        (page.clone(), parse_limits),
    ];
    let outcomes = cases.each_ref().map(|(bytes, limits)| {
        TransferPageViewV1::parse(bytes, *limits).map(TransferPageViewV1::object_count)
    });
    let importer_outcomes =
        [foreign_root, patched(&page, POSITION_OFFSET, 5), page.clone()].map(|candidate| {
            ReferenceImporter::new(binding, parse_limits)
                .accept(&candidate)
                .map(|token| token.position())
        });

    assert_eq!(count, 4, "the first page holds four objects");
    assert_eq!(
        outcomes,
        [
            Err(TransferPageFormatErrorV1::TooShort { len: 79 }),
            Err(TransferPageFormatErrorV1::TooLong {
                len: page_len,
                max_page_bytes: page_len - 1,
            }),
            Err(TransferPageFormatErrorV1::ChecksumOutOfRange(checksum_field | (1_u64 << 32))),
            Err(TransferPageFormatErrorV1::Checksum {
                expected: checksum_field as u32,
                actual: TEST_CRC32C.checksum(&flipped[..flipped.len() - CHECKSUM_LEN]),
            }),
            Err(TransferPageFormatErrorV1::UnsupportedVersion(2)),
            Err(TransferPageFormatErrorV1::TooManyObjects {
                count: 9,
                max_page_objects: 8,
            }),
            Err(TransferPageFormatErrorV1::TruncatedObjectLength { index: count }),
            Err(TransferPageFormatErrorV1::ObjectTooShort { index: 0, len: 31 }),
            Err(TransferPageFormatErrorV1::ObjectOverrunsPage {
                index: 0,
                len: u64::MAX,
                remaining: page_len - 88,
            }),
            Err(TransferPageFormatErrorV1::ObjectOverrunsPage {
                index: 0,
                len: page_len - 87,
                remaining: page_len - 88,
            }),
            Err(TransferPageFormatErrorV1::TrailingBytes {
                remaining: last_record,
            }),
            Err(TransferPageFormatErrorV1::PositionOverflow {
                position: u64::MAX,
                count,
            }),
            Ok(count),
        ],
        "every length and count of a page is checked against the page before it is used"
    );
    assert_eq!(
        importer_outcomes,
        [
            Err(ImportRefusal::Binding {
                expected: binding,
                found: foreign_binding,
            }),
            Err(ImportRefusal::Position {
                expected: 0,
                found: 5,
            }),
            Ok(count),
        ],
        "the importer refuses a page of another binding or another position"
    );
}

#[tokio::test]
async fn page_limits_admit_one_object_at_least_and_an_oversized_object_is_refused() {
    let (scratch, state) =
        publish_values("limits-channel", (0..6).map(|index| format!("limits-{index}")));
    let manifest_len = state.manifest().encoded_len();
    let exact = 88 + manifest_len;
    let view = recover(&scratch);
    let refused_limits = [(119, 1), (120, 0), (0, 0)].map(|(bytes, objects)| {
        matches!(
            TransferPageLimitsV1::new(bytes, objects),
            Err(TransferExportErrorV1::PageLimits { max_page_bytes, max_page_objects })
                if max_page_bytes == bytes && max_page_objects == objects
        )
    });
    let smallest = TransferPageLimitsV1::new(120, 1)
        .map(|limits| (limits.max_page_bytes(), limits.max_page_objects()))
        .ok();
    let mut too_small = view
        .open_export(root_of(state), page_limits(exact - 1, 16))
        .expect("open an export whose pages cannot hold the manifest");
    let first = too_small.first_token();
    let oversized = [0, 1].map(|_| match too_small.page(first) {
        Err(TransferExportErrorV1::ObjectExceedsPage {
            digest,
            encoded_len,
            max_page_bytes,
        }) => Some((digest, encoded_len, max_page_bytes)),
        _ => None,
    });
    let exact_limits = page_limits(exact, 16);
    let mut exact_session = view
        .open_export(root_of(state), exact_limits)
        .expect("open an export whose pages hold the manifest exactly");
    let exact_page = exact_session
        .page(exact_session.first_token())
        .expect("a page of exactly the manifest's size holds the manifest")
        .to_vec();
    let exact_count = TransferPageViewV1::parse(&exact_page, exact_limits)
        .expect("an exported page parses")
        .object_count();
    let single = page_limits(1 << 20, 1);
    let mut single_session = view
        .open_export(root_of(state), single)
        .expect("open an export of one object per page");
    let mut importer = ReferenceImporter::new(single_session.binding(), single);
    let pages = import_stream(&mut single_session, &mut importer)
        .expect("the importer accepts one object per page");
    let single_counts = pages
        .iter()
        .map(|page| {
            TransferPageViewV1::parse(page, single)
                .expect("an exported page parses")
                .object_count()
        })
        .collect::<Vec<_>>();
    let closure = closure_of(&view, state.manifest());
    let mut expected_counts = vec![1; closure.order.len()];
    expected_counts.push(0);

    assert_eq!(
        (refused_limits, smallest),
        ([true, true, true], Some((120, 1))),
        "limits below one object of the smallest canonical size are refused"
    );
    assert_eq!(
        oversized,
        [Some((state.manifest().digest(), manifest_len, exact - 1)); 2],
        "an object larger than a page is refused, again on a retry"
    );
    assert_eq!(
        (exact_page.len() as u64, exact_count),
        (exact, 1),
        "a page limit of exactly one object's record holds that object"
    );
    assert_eq!(
        single_counts, expected_counts,
        "a limit of one object serves every object of the closure on its own page"
    );
}
