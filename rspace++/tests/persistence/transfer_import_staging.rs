use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::durable_recovery::limits as store_limits;
use super::durable_rspace_authority::{
    Profile, Store, fixture, poll_checkpoint_with, publication_limits, store_files,
};
use super::support::{PersistenceTrialDir, TEST_SEMANTIC_PROFILE_ID, trial_scratch};
use crate::rspace::internal::Datum;
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableImportSessionV1, DurableIoError, DurableIoFailpointV1,
    DurableStateRootV1, TransferBindingV1, TransferFrontierFormatErrorV1, TransferImportBoundV1,
    TransferImportErrorV1, TransferObjectRefusalV1, TransferPageFormatErrorV1,
    TransferPageLimitsV1, TransferPageViewV1, TransferStagingOutcomeV1, TransferTokenV1,
    abort_import, arm_durable_io_failpoint, create_empty_store_v1, recover_read_authority,
};
use crate::rspace::persistence::{
    CanonicalObjectView, CatalogSlotV1, DurableAuthorityViewV1, EndpointValue, LogPackDecodeLimits,
    ObjectDigest, ObjectRefV1, SemanticProfile, StructuralValidationLimits,
};

const TEST_CRC32C: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);
const STAGING: &str = "transfer-import.v1";
const OBJECTS: &str = "objects.v1";
const FRONTIER: &str = "frontier.v1";
const FRONTIER_TEMPORARY: &str = "frontier.v1.tmp";
const FRONTIER_LEN: usize = 80;
const FRONTIER_VERSION_OFFSET: usize = 8;
const FRONTIER_ROOT_OFFSET: usize = 16;
const FRONTIER_PROFILE_OFFSET: usize = 48;
const FRONTIER_POSITION_OFFSET: usize = 64;
const FRONTIER_CHECKSUM_OFFSET: usize = 72;
const PAGE_VERSION_OFFSET: usize = 0;
const PAGE_ROOT_OFFSET: usize = 8;
const PAGE_PROFILE_OFFSET: usize = 40;
const PAGE_POSITION_OFFSET: usize = 56;
const PAGE_COUNT_OFFSET: usize = 64;
const PAGE_HEADER_LEN: usize = 72;
const LENGTH_FIELD_LEN: usize = 8;
const CHECKSUM_LEN: usize = 8;
const CHANNELS: [&str; 3] = ["import-alpha", "import-beta", "import-gamma"];

struct ExportedStream {
    root: DurableStateRootV1,
    binding: TransferBindingV1,
    pages: Vec<Vec<u8>>,
}

#[derive(Debug, PartialEq, Eq)]
struct StagingSnapshot {
    frontier: TransferTokenV1,
    outstanding: u64,
    record: Vec<u8>,
    log: Vec<u8>,
    entries: BTreeSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Page(TransferPageFormatErrorV1),
    PageBinding(TransferBindingV1),
    Position {
        expected: u64,
        found: u64,
    },
    Object {
        position: u64,
        refusal: TransferObjectRefusalV1,
    },
    Other(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrashOutcome {
    VisibleEffectsSurvive,
    UnsyncedEffectsLost,
}

fn page_limits(max_page_bytes: u64, max_page_objects: u64) -> TransferPageLimitsV1 {
    TransferPageLimitsV1::new(max_page_bytes, max_page_objects)
        .expect("the test page limits admit at least one object")
}

fn put_datum(hot: &Store, channel: &str, value: &str) {
    let channel = channel.to_owned();
    hot.put_datum(&channel, Datum::create(&channel, value.to_owned(), false));
}

fn export_stream(
    view: &DurableAuthorityViewV1<Profile>,
    manifest: ObjectRefV1,
    limits: TransferPageLimitsV1,
) -> ExportedStream {
    let root = DurableStateRootV1::of_manifest(manifest);
    let mut session = view
        .open_export(root, limits)
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
                    root,
                    binding,
                    pages,
                };
            }
            _ => token = next,
        }
    }
}

fn exported_streams(rounds: &[usize], limits: TransferPageLimitsV1) -> Vec<ExportedStream> {
    let (scratch, hot, gate, mut owner) = fixture();
    let mut manifests = Vec::with_capacity(rounds.len());
    for (round, values) in rounds.iter().enumerate() {
        for channel in CHANNELS {
            for index in 0..*values {
                put_datum(&hot, channel, &format!("round-{round}-{channel}-{index:03}"));
            }
        }
        manifests.push(
            poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
                .expect("the owner publishes a source state")
                .manifest(),
        );
    }
    drop(owner);
    let view = recover_read_authority::<Profile>(
        scratch.path(),
        store_limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("restart-grade admission recovers the source store");
    let streams = manifests
        .iter()
        .map(|manifest| export_stream(&view, *manifest, limits))
        .collect();
    drop(view);
    drop(scratch);
    streams
}

fn destination_store() -> (PersistenceTrialDir, CatalogSlotV1) {
    let destination = trial_scratch("transfer-import-destination-");
    let slot = create_empty_store_v1::<Profile>(destination.path(), store_limits())
        .expect("an empty directory receives the destination store");
    (destination, slot)
}

fn begin(
    store: &Path,
    binding: TransferBindingV1,
    limits: TransferPageLimitsV1,
) -> Result<DurableImportSessionV1<Profile>, TransferImportErrorV1> {
    DurableImportSessionV1::<Profile>::begin_import(
        store,
        binding,
        limits,
        StructuralValidationLimits::unbounded(),
    )
}

fn resume(
    store: &Path,
    binding: TransferBindingV1,
    limits: TransferPageLimitsV1,
) -> Result<DurableImportSessionV1<Profile>, TransferImportErrorV1> {
    DurableImportSessionV1::<Profile>::resume_import(
        store,
        binding,
        limits,
        StructuralValidationLimits::unbounded(),
    )
}

fn refusal<T>(result: Result<T, TransferImportErrorV1>, case: &str) -> TransferImportErrorV1 {
    match result {
        Ok(_) => panic!("{case} must be refused"),
        Err(error) => error,
    }
}

fn refusal_of(error: TransferImportErrorV1) -> Refusal {
    match error {
        TransferImportErrorV1::Page(error) => Refusal::Page(error),
        TransferImportErrorV1::PageBinding { page, .. } => Refusal::PageBinding(page),
        TransferImportErrorV1::PositionMismatch { expected, found } => {
            Refusal::Position { expected, found }
        }
        TransferImportErrorV1::Object { position, refusal } => {
            Refusal::Object { position, refusal }
        }
        other => Refusal::Other(other.to_string()),
    }
}

fn stage(session: &mut DurableImportSessionV1<Profile>, page: &[u8]) -> TransferTokenV1 {
    match session.stage_page(page) {
        Ok(TransferStagingOutcomeV1::Staged(token)) => token,
        other => panic!("an object page of an honest stream is staged, not {other:?}"),
    }
}

fn stage_stream(
    session: &mut DurableImportSessionV1<Profile>,
    pages: &[Vec<u8>],
) -> TransferTokenV1 {
    for page in pages {
        match session.stage_page(page) {
            Ok(TransferStagingOutcomeV1::Staged(_)) => {}
            Ok(TransferStagingOutcomeV1::Complete(token)) => return token,
            Err(error) => panic!("the importer stages every page of an honest stream: {error}"),
        }
    }
    panic!("an honest stream ends with its terminal page")
}

fn staging_path(store: &Path) -> PathBuf { store.join(STAGING) }

fn read_staged(store: &Path, name: &str) -> Vec<u8> {
    fs::read(staging_path(store).join(name))
        .unwrap_or_else(|error| panic!("read the staged file {name}: {error}"))
}

fn write_staged(store: &Path, name: &str, bytes: &[u8]) {
    fs::write(staging_path(store).join(name), bytes)
        .unwrap_or_else(|error| panic!("rewrite the staged file {name}: {error}"));
}

fn truncate_staged_log(store: &Path, len: u64) {
    OpenOptions::new()
        .write(true)
        .open(staging_path(store).join(OBJECTS))
        .and_then(|log| log.set_len(len))
        .expect("cut the staging log back to its synchronized length");
}

fn staged_len(store: &Path) -> u64 {
    fs::metadata(staging_path(store).join(OBJECTS))
        .expect("read the staging log's metadata")
        .len()
}

fn staging_entries(store: &Path) -> BTreeSet<String> {
    fs::read_dir(staging_path(store))
        .expect("list the staging directory")
        .map(|entry| {
            entry
                .expect("read a staging entry")
                .file_name()
                .into_string()
                .expect("staging names are UTF-8")
        })
        .collect()
}

fn settled_entries() -> BTreeSet<String> {
    [FRONTIER, OBJECTS].into_iter().map(str::to_owned).collect()
}

fn snapshot(session: &DurableImportSessionV1<Profile>, store: &Path) -> StagingSnapshot {
    StagingSnapshot {
        frontier: session.frontier(),
        outstanding: session.outstanding(),
        record: read_staged(store, FRONTIER),
        log: read_staged(store, OBJECTS),
        entries: staging_entries(store),
    }
}

fn read_u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("an eight-byte field"),
    )
}

fn frontier_position(record: &[u8]) -> u64 { read_u64_at(record, FRONTIER_POSITION_OFFSET) }

fn reseal_frontier(record: &mut [u8]) {
    let checksum = TEST_CRC32C.checksum(&record[..FRONTIER_CHECKSUM_OFFSET]);
    record[FRONTIER_CHECKSUM_OFFSET..].copy_from_slice(&u64::from(checksum).to_le_bytes());
}

fn position_of(page: &[u8]) -> u64 { read_u64_at(page, PAGE_POSITION_OFFSET) }

fn records_of(page: &[u8]) -> &[u8] { &page[PAGE_HEADER_LEN..page.len() - CHECKSUM_LEN] }

fn log_of(pages: &[Vec<u8>]) -> Vec<u8> {
    pages
        .iter()
        .flat_map(|page| records_of(page).iter().copied())
        .collect()
}

fn reseal_page(page: &mut [u8]) {
    let body_len = page.len() - CHECKSUM_LEN;
    let checksum = TEST_CRC32C.checksum(&page[..body_len]);
    page[body_len..].copy_from_slice(&u64::from(checksum).to_le_bytes());
}

fn patched_page(page: &[u8], offset: usize, value: &[u8]) -> Vec<u8> {
    let mut patched = page.to_vec();
    patched[offset..offset + value.len()].copy_from_slice(value);
    reseal_page(&mut patched);
    patched
}

fn encode_page_at(binding: TransferBindingV1, position: u64, objects: &[&[u8]]) -> Vec<u8> {
    let records = objects
        .iter()
        .map(|object| LENGTH_FIELD_LEN + object.len())
        .sum::<usize>();
    let mut page = Vec::with_capacity(PAGE_HEADER_LEN + records + CHECKSUM_LEN);
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

fn objects_of(page: &[u8], limits: TransferPageLimitsV1) -> Vec<Vec<u8>> {
    TransferPageViewV1::parse(page, limits)
        .expect("an exported page parses")
        .objects()
        .map(<[u8]>::to_vec)
        .collect()
}

fn references_of(object: &[u8]) -> Vec<ObjectDigest> {
    match CanonicalObjectView::parse(object).expect("every exported object is canonical") {
        CanonicalObjectView::SemanticManifest(manifest) => manifest
            .roots()
            .iter()
            .into_iter()
            .map(|(_, root)| root.digest())
            .collect(),
        CanonicalObjectView::DagNode(node) => {
            let children = node.children();
            let mut references = Vec::with_capacity(children.len() + 1);
            if let EndpointValue::Content(content) = node.endpoint() {
                references.push(content.digest());
            }
            references.extend(children.iter().map(|child| child.target.digest()));
            references
        }
        CanonicalObjectView::Blob(_) => Vec::new(),
        other => panic!("a state closure holds no {} object", other.kind().name()),
    }
}

fn parse_refusal(page: &[u8], limits: TransferPageLimitsV1, case: &str) -> Refusal {
    match TransferPageViewV1::parse(page, limits) {
        Ok(_) => panic!("{case} must not parse"),
        Err(error) => Refusal::Page(error),
    }
}

fn record_offsets(log: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut offset = 0;
    while offset < log.len() {
        offsets.push(offset);
        offset += LENGTH_FIELD_LEN + read_u64_at(log, offset) as usize;
    }
    offsets
}

#[tokio::test]
async fn staged_pages_hold_exactly_the_exported_stream_and_never_touch_the_store() {
    let limits = page_limits(2048, 6);
    let streams = exported_streams(&[24], limits);
    let stream = &streams[0];
    let (binding, pages) = (stream.binding, &stream.pages);
    let object_pages = pages.len() - 1;
    assert!(object_pages >= 6, "the stream must span several object pages, not {object_pages}");
    let closure_len = position_of(&pages[object_pages]);
    let (destination, genesis) = destination_store();
    let store = destination.path();
    let store_before = store_files(store);

    let mut session = begin(store, binding, limits).expect("begin an import");
    let fresh = (
        session.frontier(),
        frontier_position(&read_staged(store, FRONTIER)),
        staged_len(store),
        staging_entries(store),
        fs::metadata(staging_path(store))
            .expect("read the staging directory's metadata")
            .permissions()
            .mode() &
            0o777,
        [FRONTIER, OBJECTS].map(|name| {
            fs::metadata(staging_path(store).join(name))
                .expect("read a staged file's metadata")
                .permissions()
                .mode() &
                0o777
        }),
    );
    let half = object_pages / 2;
    let mut tokens = Vec::with_capacity(object_pages);
    for page in &pages[..half] {
        tokens.push(stage(&mut session, page));
    }
    drop(session);
    let mut resumed = resume(store, binding, limits).expect("resume the interrupted import");
    let resumed_at = resumed.frontier();
    for page in &pages[half..object_pages] {
        tokens.push(stage(&mut resumed, page));
    }
    let completed = resumed
        .stage_page(&pages[object_pages])
        .expect("the terminal page completes the stream");
    let completed_again = resumed
        .stage_page(&pages[object_pages])
        .expect("the terminal page completes the stream again");
    let outstanding = resumed.outstanding();
    let staged_log = read_staged(store, OBJECTS);
    let staged_record = read_staged(store, FRONTIER);
    drop(resumed);
    let mut reopened = resume(store, binding, limits).expect("resume the complete import");
    let reopened_completion = reopened
        .stage_page(&pages[object_pages])
        .expect("a resumed complete import accepts the terminal page");
    let store_during = store_files(store);
    let view = recover_read_authority::<Profile>(
        store,
        store_limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("the destination store recovers beside a complete staging");
    let recovered_catalog = view.catalog();
    drop(view);
    reopened.abort().expect("abort the import");
    let after_abort = (staging_path(store).exists(), store_files(store));

    assert_eq!(
        fresh,
        (TransferTokenV1::new(binding, 0), 0, 0, settled_entries(), 0o700, [0o600, 0o600]),
        "a new import has frontier 0 on disk and in memory, an empty staging log and a private \
         staging directory"
    );
    assert_eq!(
        tokens,
        pages[1..=object_pages]
            .iter()
            .map(|page| TransferTokenV1::new(binding, position_of(page)))
            .collect::<Vec<_>>(),
        "each staged page advances the frontier to the position of the next page"
    );
    assert_eq!(
        resumed_at,
        TransferTokenV1::new(binding, position_of(&pages[half])),
        "a resumed import continues at its persisted token"
    );
    assert_eq!(
        (completed, completed_again, reopened_completion, outstanding),
        (
            TransferStagingOutcomeV1::Complete(TransferTokenV1::new(binding, closure_len)),
            TransferStagingOutcomeV1::Complete(TransferTokenV1::new(binding, closure_len)),
            TransferStagingOutcomeV1::Complete(TransferTokenV1::new(binding, closure_len)),
            0
        ),
        "the terminal page completes the stream once every announced object is staged"
    );
    assert_eq!(
        (staged_log == log_of(pages), frontier_position(&staged_record)),
        (true, closure_len),
        "the staging log holds exactly the stream's records and the frontier covers the whole \
         closure"
    );
    assert_eq!(
        (store_during, recovered_catalog, after_abort),
        (store_before.clone(), genesis, (false, store_before)),
        "staging never changes the store's catalog, packs or index, the store recovers beside it, \
         and abort removes the staging directory and nothing else"
    );
}

#[tokio::test]
async fn an_ended_session_frees_its_staging_while_a_duplicate_of_its_log_descriptor_lives() {
    let limits = page_limits(2048, 6);
    let streams = exported_streams(&[8], limits);
    let stream = &streams[0];
    let (binding, pages) = (stream.binding, &stream.pages);
    let (destination, _) = destination_store();
    let store = destination.path();

    let mut session = begin(store, binding, limits).expect("begin an import");
    let staged = stage(&mut session, &pages[0]);
    let first_duplicate = session
        .duplicate_log_description()
        .expect("duplicate the open description of the live session's log");
    drop(session);
    let resumed = resume(store, binding, limits);
    let resumed_at = resumed
        .as_ref()
        .map(|session| session.frontier())
        .map_err(ToString::to_string);
    let second_duplicate = resumed
        .as_ref()
        .ok()
        .map(|session| session.duplicate_log_description());
    drop(resumed);
    let aborted = abort_import(store).map_err(|error| error.to_string());
    let staging_left = staging_path(store).exists();
    drop((first_duplicate, second_duplicate));

    assert_eq!(
        resumed_at,
        Ok(staged),
        "a session that ended holds no lock on its staging, whatever duplicates of its log's open \
         description remain, so a resume continues at its frontier"
    );
    assert_eq!(
        (aborted, staging_left),
        (Ok(()), false),
        "after the resumed session ended beside a second duplicate, an abort from outside removes \
         the staging"
    );
}

#[tokio::test]
async fn a_token_resumes_only_on_a_stream_of_its_transfer_version_binding_and_profile() {
    let limits = page_limits(2048, 6);
    let streams = exported_streams(&[12, 4], limits);
    let (first, second) = (&streams[0], &streams[1]);
    assert!(
        first.pages.len() > 3 && first.binding != second.binding,
        "the test needs two distinct streams of several pages"
    );
    let (destination, _) = destination_store();
    let store = destination.path();
    let mut session = begin(store, first.binding, limits).expect("begin an import");
    for page in &first.pages[..2] {
        stage(&mut session, page);
    }
    let acknowledged = session.frontier();
    drop(session);
    let record = read_staged(store, FRONTIER);
    let log = read_staged(store, OBJECTS);

    let foreign_profile = TransferBindingV1::new(first.root, TEST_SEMANTIC_PROFILE_ID);
    let refused_bindings = [second.binding, foreign_profile].map(|binding| {
        let refused = refusal(resume(store, binding, limits), "a resume under another binding");
        (
            refused.to_string(),
            read_staged(store, FRONTIER) == record,
            read_staged(store, OBJECTS) == log,
        )
    });
    let foreign_begin =
        refusal(begin(store, foreign_profile, limits), "a new import of a foreign profile");

    let mut other_version = record.clone();
    other_version[FRONTIER_VERSION_OFFSET..FRONTIER_ROOT_OFFSET]
        .copy_from_slice(&2_u64.to_le_bytes());
    reseal_frontier(&mut other_version);
    let mut other_profile = record.clone();
    other_profile[FRONTIER_PROFILE_OFFSET..FRONTIER_POSITION_OFFSET]
        .copy_from_slice(TEST_SEMANTIC_PROFILE_ID.as_bytes());
    reseal_frontier(&mut other_profile);
    let crafted = [other_version, other_profile].map(|token_bytes| {
        write_staged(store, FRONTIER, &token_bytes);
        let refused = refusal(resume(store, first.binding, limits), "a resume of a crafted token");
        let kept =
            read_staged(store, FRONTIER) == token_bytes && read_staged(store, OBJECTS) == log;
        (refused, kept)
    });
    write_staged(store, FRONTIER, &record);

    let mut resumed =
        resume(store, first.binding, limits).expect("resume under the token's identity");
    let resumed_at = resumed.frontier();
    let next = &first.pages[2];
    let foreign_pages = [
        patched_page(next, PAGE_ROOT_OFFSET, second.binding.root_digest().as_bytes()),
        patched_page(next, PAGE_PROFILE_OFFSET, TEST_SEMANTIC_PROFILE_ID.as_bytes()),
        patched_page(next, PAGE_VERSION_OFFSET, &2_u64.to_le_bytes()),
    ];
    let before = snapshot(&resumed, store);
    let foreign_refusals = foreign_pages
        .iter()
        .map(|page| refusal_of(refusal(resumed.stage_page(page), "a page of another stream")))
        .collect::<Vec<_>>();
    let after = snapshot(&resumed, store);
    let completed = stage_stream(&mut resumed, &first.pages[2..]);
    let final_log = read_staged(store, OBJECTS);
    resumed.abort().expect("abort the import");

    let binding_refusal = |requested: TransferBindingV1| {
        TransferImportErrorV1::BindingMismatch {
            staged_root: first.binding.root_digest(),
            staged_profile: first.binding.profile(),
            requested,
        }
        .to_string()
    };
    assert_eq!(
        refused_bindings,
        [
            (binding_refusal(second.binding), true, true),
            (
                TransferImportErrorV1::ForeignProfile {
                    store: Profile::ID,
                    requested: TEST_SEMANTIC_PROFILE_ID
                }
                .to_string(),
                true,
                true
            )
        ],
        "a token does not resume under another root or another profile, and the refusal keeps the \
         token and the staging byte for byte"
    );
    assert!(
        matches!(foreign_begin, TransferImportErrorV1::ForeignProfile { store, requested }
            if store == Profile::ID && requested == TEST_SEMANTIC_PROFILE_ID),
        "a transfer of a foreign profile is refused before any staging, not {foreign_begin}"
    );
    assert!(
        matches!(
            &crafted,
            [
                (TransferImportErrorV1::UnsupportedTransferVersion { version: 2 }, true),
                (TransferImportErrorV1::BindingMismatch { staged_root, staged_profile, requested }, true),
            ] if *staged_root == first.binding.root_digest()
                && *staged_profile == TEST_SEMANTIC_PROFILE_ID
                && *requested == first.binding
        ),
        "a token of transfer version 2 and a token of another profile do not resume, and both are \
         kept: {crafted:?}"
    );
    assert_eq!(
        (resumed_at, acknowledged),
        (TransferTokenV1::new(first.binding, position_of(next)), resumed_at),
        "a resume under the token's version, binding and profile continues at the token"
    );
    assert_eq!(
        (foreign_refusals, after),
        (
            vec![
                Refusal::PageBinding(TransferBindingV1::new(second.root, Profile::ID)),
                Refusal::PageBinding(foreign_profile),
                Refusal::Page(TransferPageFormatErrorV1::UnsupportedVersion(2)),
            ],
            before
        ),
        "pages of another root, another profile or another transfer version are refused and leave \
         the frontier unchanged"
    );
    assert_eq!(
        (completed, final_log == log_of(&first.pages)),
        (
            TransferTokenV1::new(first.binding, position_of(&first.pages[first.pages.len() - 1])),
            true
        ),
        "the resumed stream stages to completion"
    );
}

#[tokio::test]
async fn staging_and_frontier_never_cross_sessions_and_a_new_session_starts_empty_at_frontier_zero()
{
    let limits = page_limits(2048, 6);
    let streams = exported_streams(&[12, 4], limits);
    let (first, second) = (&streams[0], &streams[1]);
    let (destination, _) = destination_store();
    let store = destination.path();

    let mut live = begin(store, first.binding, limits).expect("begin the first import");
    stage(&mut live, &first.pages[0]);
    let live_before = snapshot(&live, store);
    let blocked = [
        refusal(begin(store, first.binding, limits), "a second import of the same binding"),
        refusal(begin(store, second.binding, limits), "a second import of another binding"),
        refusal(resume(store, first.binding, limits), "a resume of a live session"),
        refusal(abort_import(store), "an abort of a live session from outside it"),
    ]
    .map(|error| error.to_string());
    let live_after = snapshot(&live, store);
    live.abort().expect("abort the first import");
    let aborted = (
        staging_path(store).exists(),
        refusal(resume(store, first.binding, limits), "a resume after abort").to_string(),
    );

    let mut fresh = begin(store, second.binding, limits).expect("begin an import after abort");
    let fresh_state = (
        fresh.frontier(),
        fresh.outstanding(),
        frontier_position(&read_staged(store, FRONTIER)),
        staged_len(store),
        staging_entries(store),
    );
    let foreign_first_page = refusal_of(refusal(
        fresh.stage_page(&first.pages[0]),
        "a page of the aborted session's stream",
    ));
    let fresh_completed = stage_stream(&mut fresh, &second.pages);
    let fresh_log = read_staged(store, OBJECTS);
    drop(fresh);
    let refused_resume = refusal(
        resume(store, first.binding, limits),
        "a resume of the earlier binding over another session's staging",
    );
    let kept_after_refusal = read_staged(store, OBJECTS) == fresh_log;
    abort_import(store).expect("abort the refused session's staging");
    let mut after_refusal = begin(store, first.binding, limits).expect("begin after the refusal");
    let after_refusal_state = (after_refusal.frontier(), staged_len(store));
    let after_refusal_completed = stage_stream(&mut after_refusal, &first.pages);
    let after_refusal_log = read_staged(store, OBJECTS);
    after_refusal.abort().expect("abort the import");

    arm_durable_io_failpoint(DurableIoFailpointV1::CandidateWrite);
    let interrupted =
        refusal(begin(store, first.binding, limits), "a begin interrupted at its token");
    let interrupted_entries = staging_entries(store);
    let interrupted_effects = [
        refusal(resume(store, first.binding, limits), "a resume of an interrupted begin"),
        refusal(begin(store, first.binding, limits), "a begin over an interrupted begin"),
    ]
    .map(|error| error.to_string());
    abort_import(store).expect("abort the interrupted begin");
    let restarted = begin(store, first.binding, limits).expect("begin after the interrupted begin");
    let restarted_state = (restarted.frontier(), staged_len(store));
    restarted.abort().expect("abort the import");
    let idle_abort = abort_import(store).is_ok();

    let staging = staging_path(store);
    let exists = TransferImportErrorV1::StagingExists {
        path: staging.clone(),
    }
    .to_string();
    let in_use = TransferImportErrorV1::StagingInUse {
        path: staging.clone(),
    }
    .to_string();
    assert_eq!(
        (blocked, live_after),
        ([exists.clone(), exists.clone(), in_use.clone(), in_use], live_before),
        "while a session lives, no second session begins, resumes or aborts its staging, and its \
         frontier and staging stay unchanged"
    );
    assert_eq!(
        aborted,
        (
            false,
            TransferImportErrorV1::NoStaging {
                path: staging.clone()
            }
            .to_string()
        ),
        "abort deletes the staging directory, so its token resumes nothing"
    );
    assert_eq!(
        fresh_state,
        (TransferTokenV1::new(second.binding, 0), 0, 0, 0, settled_entries()),
        "a session opened after an abort starts at frontier 0 with empty private staging"
    );
    assert_eq!(
        (foreign_first_page, fresh_completed, fresh_log == log_of(&second.pages)),
        (
            Refusal::PageBinding(first.binding),
            TransferTokenV1::new(
                second.binding,
                position_of(&second.pages[second.pages.len() - 1])
            ),
            true
        ),
        "the new session refuses the aborted session's pages and stages only its own stream"
    );
    assert!(
        matches!(&refused_resume, TransferImportErrorV1::BindingMismatch { staged_root, .. }
            if *staged_root == second.binding.root_digest()) &&
            kept_after_refusal,
        "a resume under another binding is refused and keeps the staging, not {refused_resume}"
    );
    assert_eq!(
        (after_refusal_state, after_refusal_completed, after_refusal_log == log_of(&first.pages)),
        (
            (TransferTokenV1::new(first.binding, 0), 0),
            TransferTokenV1::new(first.binding, position_of(&first.pages[first.pages.len() - 1])),
            true
        ),
        "a session opened after a refusal and an abort starts at frontier 0 with empty staging \
         and holds only its own stream"
    );
    assert!(
        matches!(&interrupted, TransferImportErrorV1::Io(DurableIoError::Io { .. })),
        "the armed failpoint interrupts the begin, not {interrupted}"
    );
    assert_eq!(
        (interrupted_entries, interrupted_effects, restarted_state, idle_abort),
        (
            [FRONTIER_TEMPORARY, OBJECTS]
                .into_iter()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>(),
            [
                TransferImportErrorV1::IncompleteStaging {
                    path: staging.clone()
                }
                .to_string(),
                exists
            ],
            (TransferTokenV1::new(first.binding, 0), 0),
            true
        ),
        "a begin interrupted before its token resumes nothing and blocks a new begin until abort \
         removes it; the next session starts at frontier 0 with empty staging"
    );
}

#[tokio::test]
async fn a_crash_between_object_sync_and_token_write_leaves_at_most_one_page_beyond_the_frontier_and_replay_is_idempotent()
 {
    let limits = page_limits(2048, 6);
    let streams = exported_streams(&[16], limits);
    let stream = &streams[0];
    let (binding, pages) = (stream.binding, &stream.pages);
    let object_pages = pages.len() - 1;
    assert!(object_pages >= 4, "the stream must span several object pages, not {object_pages}");
    let positions = pages
        .iter()
        .map(|page| position_of(page))
        .collect::<Vec<_>>();
    let mut offsets = Vec::with_capacity(pages.len());
    let mut offset = 0_u64;
    for page in pages {
        offsets.push(offset);
        offset += records_of(page).len() as u64;
    }
    let (destination, _) = destination_store();
    let store = destination.path();

    let mut reference = begin(store, binding, limits).expect("begin the reference import");
    let complete = stage_stream(&mut reference, pages);
    let reference_record = read_staged(store, FRONTIER);
    let reference_log = read_staged(store, OBJECTS);
    reference.abort().expect("abort the reference import");

    let mut early = begin(store, binding, limits).expect("begin an import");
    stage(&mut early, &pages[0]);
    let before_beyond = snapshot(&early, store);
    let beyond =
        refusal_of(refusal(early.stage_page(&pages[2]), "a page beyond the next position"));
    let after_beyond = snapshot(&early, store);
    early.abort().expect("abort the import");

    let crashes = [
        (DurableIoFailpointV1::PhysicalWrite, CrashOutcome::VisibleEffectsSurvive),
        (DurableIoFailpointV1::PhysicalSync, CrashOutcome::VisibleEffectsSurvive),
        (DurableIoFailpointV1::PhysicalSync, CrashOutcome::UnsyncedEffectsLost),
        (DurableIoFailpointV1::CandidateWrite, CrashOutcome::VisibleEffectsSurvive),
        (DurableIoFailpointV1::CandidateSync, CrashOutcome::VisibleEffectsSurvive),
        (DurableIoFailpointV1::BeforeReplace, CrashOutcome::VisibleEffectsSurvive),
        (
            DurableIoFailpointV1::AfterReplaceBeforeDirectorySync,
            CrashOutcome::VisibleEffectsSurvive,
        ),
        (DurableIoFailpointV1::AfterReplaceBeforeDirectorySync, CrashOutcome::UnsyncedEffectsLost),
        (DurableIoFailpointV1::DirectorySync, CrashOutcome::VisibleEffectsSurvive),
        (DurableIoFailpointV1::DirectorySync, CrashOutcome::UnsyncedEffectsLost),
    ];
    let crashed_pages = [0, object_pages / 2, object_pages - 1];
    let mut observed = Vec::with_capacity(crashes.len() * crashed_pages.len());
    let mut predicted = Vec::with_capacity(crashes.len() * crashed_pages.len());
    for crashed in crashed_pages {
        for (failpoint, outcome) in crashes {
            let mut session = begin(store, binding, limits).expect("begin an import");
            for page in &pages[..crashed] {
                stage(&mut session, page);
            }
            let durable_record = read_staged(store, FRONTIER);
            arm_durable_io_failpoint(failpoint);
            let injected = matches!(
                session.stage_page(&pages[crashed]),
                Err(TransferImportErrorV1::Io(DurableIoError::Io { .. }))
            );
            let failed = matches!(
                session.stage_page(&pages[crashed]),
                Err(TransferImportErrorV1::SessionFailed)
            );
            drop(session);
            match (outcome, failpoint) {
                (CrashOutcome::VisibleEffectsSurvive, _) => {}
                (CrashOutcome::UnsyncedEffectsLost, DurableIoFailpointV1::PhysicalSync) => {
                    truncate_staged_log(store, offsets[crashed]);
                }
                (CrashOutcome::UnsyncedEffectsLost, _) => {
                    write_staged(store, FRONTIER, &durable_record);
                }
            }
            let token = frontier_position(&read_staged(store, FRONTIER));
            let log_len = staged_len(store);
            let acknowledged = positions
                .iter()
                .position(|position| *position == token)
                .expect("the frontier record names a page boundary");
            assert!(
                log_len >= offsets[acknowledged],
                "after a crash at {failpoint:?} with {outcome:?} while staging page {crashed}, \
                 the frontier record acknowledges {token} objects but the staging log holds only \
                 {log_len} of the {} bytes their records occupy: acknowledged objects were lost",
                offsets[acknowledged]
            );
            let mut resumed = resume(store, binding, limits).expect("resume after the crash");
            let resumed_at = (resumed.frontier().position(), staged_len(store));
            let completed = stage_stream(&mut resumed, &pages[acknowledged..]);
            let replayed = (
                read_staged(store, FRONTIER) == reference_record,
                read_staged(store, OBJECTS) == reference_log,
                staging_entries(store),
            );
            resumed.abort().expect("abort the import");
            observed.push((
                crashed, failpoint, outcome, injected, failed, token, log_len, resumed_at,
                completed, replayed,
            ));
            let survives = outcome == CrashOutcome::VisibleEffectsSurvive;
            let advanced = crashed +
                usize::from(
                    survives &&
                        matches!(
                            failpoint,
                            DurableIoFailpointV1::AfterReplaceBeforeDirectorySync |
                                DurableIoFailpointV1::DirectorySync
                        ),
                );
            let written = crashed +
                usize::from(match failpoint {
                    DurableIoFailpointV1::PhysicalWrite => false,
                    DurableIoFailpointV1::PhysicalSync => survives,
                    _ => true,
                });
            predicted.push((
                crashed,
                failpoint,
                outcome,
                true,
                true,
                positions[advanced],
                offsets[written],
                (positions[advanced], offsets[advanced]),
                complete,
                (true, true, settled_entries()),
            ));
        }
    }

    let mut torn = begin(store, binding, limits).expect("begin an import");
    for page in &pages[..2] {
        stage(&mut torn, page);
    }
    drop(torn);
    let next_records = records_of(&pages[2]);
    OpenOptions::new()
        .append(true)
        .open(staging_path(store).join(OBJECTS))
        .and_then(|mut log| log.write_all(&next_records[..next_records.len() / 2]))
        .expect("append a torn record tail to the staging log");
    let torn_len = staged_len(store);
    let mut recovered = resume(store, binding, limits).expect("resume over a torn tail");
    let recovered_at = (recovered.frontier().position(), staged_len(store));
    let recovered_complete = stage_stream(&mut recovered, &pages[2..]);
    let recovered_log = read_staged(store, OBJECTS) == reference_log;
    recovered.abort().expect("abort the import");

    assert_eq!(
        (beyond, after_beyond),
        (
            Refusal::Position {
                expected: positions[1],
                found: positions[2]
            },
            before_beyond
        ),
        "a page beyond the next position is rejected and leaves the frontier unchanged"
    );
    assert_eq!(
        observed, predicted,
        "a crash at each staging step leaves the predicted frontier record and staging log, at \
         most one page beyond the frontier, and resuming and replaying reproduces the uncrashed \
         staging byte for byte"
    );
    assert_eq!(
        (torn_len > offsets[2], recovered_at, recovered_complete, recovered_log),
        (true, (positions[2], offsets[2]), complete, true),
        "a torn record tail beyond the frontier is discarded on resume and the replay is \
         idempotent"
    );
}

#[tokio::test]
async fn duplicate_reordered_truncated_and_corrupted_pages_are_refused_and_leave_the_frontier_unchanged()
 {
    let export_limits = page_limits(2048, 6);
    let limits = page_limits(4096, 12);
    let streams = exported_streams(&[16], export_limits);
    let stream = &streams[0];
    let (binding, pages) = (stream.binding, &stream.pages);
    let acknowledged = 2;
    let next = &pages[acknowledged];
    let next_objects = objects_of(next, limits);
    assert!(
        pages.len() > acknowledged + 2 && next_objects.len() >= 2,
        "the stream must hold at least two pages beyond the frontier and a next page of two \
         objects"
    );
    let (destination, _) = destination_store();
    let store = destination.path();
    let mut session = begin(store, binding, limits).expect("begin an import");
    for page in &pages[..acknowledged] {
        stage(&mut session, page);
    }
    let frontier = position_of(next);
    let staged_objects = pages[..acknowledged]
        .iter()
        .flat_map(|page| objects_of(page, limits))
        .collect::<Vec<_>>();
    let staged_digests = staged_objects
        .iter()
        .map(|object| ObjectDigest::of_canonical_bytes(object))
        .collect::<BTreeSet<_>>();
    let announced = staged_objects
        .iter()
        .flat_map(|object| references_of(object))
        .filter(|digest| !staged_digests.contains(digest))
        .collect::<BTreeSet<_>>();
    let unannounced = pages[acknowledged..]
        .iter()
        .flat_map(|page| objects_of(page, limits))
        .find(|object| !announced.contains(&ObjectDigest::of_canonical_bytes(object)))
        .expect("a later object is announced only by an object that is not yet staged");
    let first_object_end = PAGE_HEADER_LEN + LENGTH_FIELD_LEN + next_objects[0].len();
    let mut flipped_unsealed = next.clone();
    flipped_unsealed[first_object_end - 1] ^= 0x01;
    let mut flipped_resealed = flipped_unsealed.clone();
    reseal_page(&mut flipped_resealed);
    let flipped_object =
        &flipped_resealed[first_object_end - next_objects[0].len()..first_object_end];
    let truncated_header = next[..PAGE_HEADER_LEN + CHECKSUM_LEN - 1].to_vec();
    let truncated_tail = next[..next.len() - 1].to_vec();
    let mut truncated_object = next[..first_object_end - 1].to_vec();
    truncated_object.extend_from_slice(&[0; CHECKSUM_LEN]);
    reseal_page(&mut truncated_object);
    let next_count = read_u64_at(next, PAGE_COUNT_OFFSET);
    let last_object_len = next_objects[next_objects.len() - 1].len();
    let cases = vec![
        ("a replay of the last staged page", pages[acknowledged - 1].clone(), Refusal::Position {
            expected: frontier,
            found: position_of(&pages[acknowledged - 1]),
        }),
        (
            "a page that repeats an already staged object",
            encode_page_at(binding, frontier, &[staged_objects[1].as_slice()]),
            Refusal::Object {
                position: frontier,
                refusal: TransferObjectRefusalV1::Duplicate {
                    digest: ObjectDigest::of_canonical_bytes(&staged_objects[1]),
                },
            },
        ),
        (
            "a page that holds its next object twice",
            encode_page_at(binding, frontier, &[
                next_objects[0].as_slice(),
                next_objects[0].as_slice(),
            ]),
            Refusal::Object {
                position: frontier + 1,
                refusal: TransferObjectRefusalV1::Duplicate {
                    digest: ObjectDigest::of_canonical_bytes(&next_objects[0]),
                },
            },
        ),
        (
            "the page after the next, beyond the frontier",
            pages[acknowledged + 1].clone(),
            Refusal::Position {
                expected: frontier,
                found: position_of(&pages[acknowledged + 1]),
            },
        ),
        (
            "a reordered page whose object no staged object references",
            encode_page_at(binding, frontier, &[unannounced.as_slice()]),
            Refusal::Object {
                position: frontier,
                refusal: TransferObjectRefusalV1::Unreferenced {
                    digest: ObjectDigest::of_canonical_bytes(&unannounced),
                },
            },
        ),
        (
            "the next page cut below its header and checksum",
            truncated_header,
            Refusal::Page(TransferPageFormatErrorV1::TooShort {
                len: (PAGE_HEADER_LEN + CHECKSUM_LEN - 1) as u64,
            }),
        ),
        (
            "the next page without its last byte",
            truncated_tail.clone(),
            parse_refusal(&truncated_tail, limits, "a page without its last byte"),
        ),
        (
            "the next page cut inside its first object and resealed",
            truncated_object.clone(),
            parse_refusal(&truncated_object, limits, "a page cut inside an object"),
        ),
        (
            "the next page whose count claims one object more",
            patched_page(next, PAGE_COUNT_OFFSET, &(next_count + 1).to_le_bytes()),
            Refusal::Page(TransferPageFormatErrorV1::TruncatedObjectLength { index: next_count }),
        ),
        (
            "the next page whose count claims one object fewer",
            patched_page(next, PAGE_COUNT_OFFSET, &(next_count - 1).to_le_bytes()),
            Refusal::Page(TransferPageFormatErrorV1::TrailingBytes {
                remaining: (LENGTH_FIELD_LEN + last_object_len) as u64,
            }),
        ),
        (
            "the next page with one bit flipped under its old checksum",
            flipped_unsealed.clone(),
            parse_refusal(&flipped_unsealed, limits, "a page with a flipped bit"),
        ),
        (
            "the next page with one bit flipped in its first object and resealed",
            flipped_resealed.clone(),
            Refusal::Object {
                position: frontier,
                refusal: TransferObjectRefusalV1::Unreferenced {
                    digest: ObjectDigest::of_canonical_bytes(flipped_object),
                },
            },
        ),
        (
            "the next page with a corrupted transfer version",
            patched_page(next, PAGE_VERSION_OFFSET, &2_u64.to_le_bytes()),
            Refusal::Page(TransferPageFormatErrorV1::UnsupportedVersion(2)),
        ),
        (
            "the next page with a corrupted position",
            patched_page(next, PAGE_POSITION_OFFSET, &(frontier + 1).to_le_bytes()),
            Refusal::Position {
                expected: frontier,
                found: frontier + 1,
            },
        ),
    ];
    let mut observed = Vec::with_capacity(cases.len());
    let mut expected = Vec::with_capacity(cases.len());
    for (case, page, refused) in cases {
        let before = snapshot(&session, store);
        let outcome = refusal_of(refusal(session.stage_page(&page), case));
        let after = snapshot(&session, store);
        assert_eq!(
            (&outcome, &after),
            (&refused, &before),
            "{case} must be refused by the failing check and leave the frontier, the frontier \
             record and the staging log unchanged"
        );
        observed.push((case, outcome, after == before));
        expected.push((case, refused, true));
    }
    let completed = stage_stream(&mut session, &pages[acknowledged..]);
    let final_log = read_staged(store, OBJECTS);
    session.abort().expect("abort the import");

    assert_eq!(observed, expected, "every malformed page is refused without effect");
    assert_eq!(
        (completed, final_log == log_of(pages)),
        (TransferTokenV1::new(binding, position_of(&pages[pages.len() - 1])), true),
        "after the refusals the honest stream stages to completion and the staging log holds \
         exactly its records"
    );
}

#[tokio::test]
async fn every_staged_length_and_the_frontier_record_are_untrusted_bounded_and_checked() {
    let limits = page_limits(2048, 6);
    let streams = exported_streams(&[8], limits);
    let stream = &streams[0];
    let (binding, pages) = (stream.binding, &stream.pages);
    let (destination, _) = destination_store();
    let store = destination.path();
    let mut session = begin(store, binding, limits).expect("begin an import");
    for page in &pages[..2] {
        stage(&mut session, page);
    }
    let frontier = session.frontier().position();
    drop(session);
    let record = read_staged(store, FRONTIER);
    let log = read_staged(store, OBJECTS);
    let offsets = record_offsets(&log);
    let last_record = offsets[offsets.len() - 1];
    let manifest_len = read_u64_at(&log, 0);

    let mut longer = record.clone();
    longer.push(0);
    let mut magic = record.clone();
    magic[0] ^= 0x01;
    let mut wide_checksum = record.clone();
    wide_checksum[FRONTIER_LEN - 1] = 0x01;
    let wide_value = read_u64_at(&wide_checksum, FRONTIER_CHECKSUM_OFFSET);
    let mut flipped_position = record.clone();
    flipped_position[FRONTIER_POSITION_OFFSET] ^= 0x01;
    let stored = read_u64_at(&record, FRONTIER_CHECKSUM_OFFSET) as u32;
    let flipped_checksum = TEST_CRC32C.checksum(&flipped_position[..FRONTIER_CHECKSUM_OFFSET]);
    let record_cases = [
        (record[..FRONTIER_LEN - 1].to_vec(), TransferFrontierFormatErrorV1::Length { len: 79 }),
        (longer, TransferFrontierFormatErrorV1::Length { len: 81 }),
        (magic, TransferFrontierFormatErrorV1::Magic),
        (wide_checksum, TransferFrontierFormatErrorV1::ChecksumOutOfRange(wide_value)),
        (flipped_position, TransferFrontierFormatErrorV1::Checksum {
            expected: stored,
            actual: flipped_checksum,
        }),
    ];
    let record_observed = record_cases
        .iter()
        .map(|(bytes, _)| {
            write_staged(store, FRONTIER, bytes);
            let refused = match refusal(resume(store, binding, limits), "a malformed frontier") {
                TransferImportErrorV1::Frontier { error, .. } => Some(error),
                _ => None,
            };
            (refused, read_staged(store, FRONTIER) == *bytes && read_staged(store, OBJECTS) == log)
        })
        .collect::<Vec<_>>();
    write_staged(store, FRONTIER, &record);

    let with_length = |offset: usize, len: u64| {
        let mut bytes = log.clone();
        bytes[offset..offset + LENGTH_FIELD_LEN].copy_from_slice(&len.to_le_bytes());
        bytes
    };
    let mut flipped_manifest = log.clone();
    flipped_manifest[LENGTH_FIELD_LEN + 40] ^= 0x01;
    let flipped_digest = ObjectDigest::of_canonical_bytes(
        &flipped_manifest[LENGTH_FIELD_LEN..LENGTH_FIELD_LEN + manifest_len as usize],
    );
    let beyond_log = (log.len() - last_record - LENGTH_FIELD_LEN) as u64 + 1;
    let log_cases = [
        (with_length(0, 31), "StagedRecordLength 0 31".to_owned()),
        (with_length(0, 2049), "StagedRecordLength 0 2049".to_owned()),
        (
            with_length(last_record, beyond_log),
            format!("StagedObjectsMissing {frontier} {}", frontier - 1),
        ),
        (
            log[..log.len() - 1].to_vec(),
            format!("StagedObjectsMissing {frontier} {}", frontier - 1),
        ),
        (flipped_manifest, format!("StagedObject 0 Unreferenced {flipped_digest}")),
    ];
    let log_observed = log_cases
        .iter()
        .map(|(bytes, _)| {
            write_staged(store, OBJECTS, bytes);
            let refused = match refusal(resume(store, binding, limits), "a malformed staging log") {
                TransferImportErrorV1::StagedRecordLength {
                    position,
                    len,
                    max_page_bytes: 2048,
                } => format!("StagedRecordLength {position} {len}"),
                TransferImportErrorV1::StagedObjectsMissing { frontier, staged } => {
                    format!("StagedObjectsMissing {frontier} {staged}")
                }
                TransferImportErrorV1::StagedObject {
                    position,
                    refusal: TransferObjectRefusalV1::Unreferenced { digest },
                } => format!("StagedObject {position} Unreferenced {digest}"),
                other => other.to_string(),
            };
            (
                refused,
                read_staged(store, OBJECTS) == *bytes && read_staged(store, FRONTIER) == record,
            )
        })
        .collect::<Vec<_>>();
    write_staged(store, OBJECTS, &log);

    let tight = |bound: TransferImportBoundV1, limit: u64| {
        let unbounded = StructuralValidationLimits::unbounded();
        match bound {
            TransferImportBoundV1::DistinctObjects => StructuralValidationLimits {
                max_distinct_objects: limit,
                ..unbounded
            },
            TransferImportBoundV1::CanonicalBytes => StructuralValidationLimits {
                max_canonical_bytes: limit,
                ..unbounded
            },
            TransferImportBoundV1::References => StructuralValidationLimits {
                max_references: limit,
                ..unbounded
            },
            TransferImportBoundV1::Outstanding => StructuralValidationLimits {
                max_worklist: limit,
                ..unbounded
            },
        }
    };
    let tight_resume = refusal(
        DurableImportSessionV1::<Profile>::resume_import(
            store,
            binding,
            limits,
            tight(TransferImportBoundV1::DistinctObjects, 1),
        ),
        "a resume under bounds below the staged prefix",
    );
    let resume_kept = read_staged(store, OBJECTS) == log && read_staged(store, FRONTIER) == record;
    let resumed = resume(store, binding, limits).expect("resume the intact staging");
    let resumed_at = resumed.frontier().position();
    resumed.abort().expect("abort the import");

    let bound_cases = [
        (TransferImportBoundV1::DistinctObjects, 1),
        (TransferImportBoundV1::CanonicalBytes, manifest_len - 1),
        (TransferImportBoundV1::References, 0),
        (TransferImportBoundV1::Outstanding, 0),
    ];
    let bound_observed = bound_cases
        .iter()
        .map(|(bound, limit)| {
            let mut bounded = DurableImportSessionV1::<Profile>::begin_import(
                store,
                binding,
                limits,
                tight(*bound, *limit),
            )
            .expect("begin a bounded import");
            let before = snapshot(&bounded, store);
            let refused =
                refusal_of(refusal(bounded.stage_page(&pages[0]), "a page beyond the bounds"));
            let unchanged = snapshot(&bounded, store) == before;
            bounded.abort().expect("abort the bounded import");
            (refused, unchanged)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        record_observed,
        record_cases
            .iter()
            .map(|(_, error)| (Some(*error), true))
            .collect::<Vec<_>>(),
        "a frontier record of the wrong length, magic or checksum is refused and kept"
    );
    assert_eq!(
        log_observed,
        log_cases
            .iter()
            .map(|(_, refused)| (refused.clone(), true))
            .collect::<Vec<_>>(),
        "a staged length outside 32 bytes to the page limit, a record beyond the log, a short log \
         and a corrupted staged object are refused and kept"
    );
    assert!(
        matches!(&tight_resume, TransferImportErrorV1::StagedObject {
            position: 0,
            refusal: TransferObjectRefusalV1::Bound {
                bound: TransferImportBoundV1::DistinctObjects,
                limit: 1
            }
        }) && resume_kept &&
            resumed_at == frontier,
        "a resume re-admits the staged prefix under its bounds and keeps the staging when it is \
         refused, not {tight_resume}"
    );
    assert_eq!(
        bound_observed,
        bound_cases
            .iter()
            .map(|(bound, limit)| (
                Refusal::Object {
                    position: 0,
                    refusal: TransferObjectRefusalV1::Bound {
                        bound: *bound,
                        limit: *limit
                    }
                },
                true
            ))
            .collect::<Vec<_>>(),
        "a page beyond each session bound is refused without effect"
    );
}
