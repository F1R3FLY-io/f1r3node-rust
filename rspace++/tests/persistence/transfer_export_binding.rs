use std::cell::Cell;
use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use proptest::prelude::*;
use shared::rust::store::key_value_store::KvStoreError;
use tokio::sync::RwLock;

use super::durable_recovery::limits;
use super::durable_rspace_authority::{
    Profile, Store, fixture, poll_checkpoint_with, publication_limits,
};
use super::support::PersistenceTrialDir;
use crate::rspace::errors::AuthorityBackendMismatch;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::durable_history_repository::{
    DAG_TRANSFER_PAGE_MAX_BYTES, DAG_TRANSFER_PAGE_MAX_OBJECTS, DAG_TRANSFER_SESSION_LIMIT,
    DurableHistoryRepositoryV1, DurableRSpaceExporterV1,
};
use crate::rspace::history::history_repository::HistoryRepository;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableExportSessionV1, DurableRSpaceAuthorityV1, DurableStateRootV1,
    TransferExportErrorV1, TransferPageLimitsV1, TransferPageViewV1, TransferTokenV1,
    recover_read_authority,
};
use crate::rspace::persistence::{DurableAuthorityViewV1, LogPackDecodeLimits};
use crate::rspace::shared::trie_exporter::{
    KeyHash, NodePath, StateExportError, dag_transfer_cursor, parse_dag_transfer_cursor,
};
use crate::rspace::state::instances::rspace_exporter_store::RSpaceExporterStore;
use crate::rspace::state::rspace_exporter::{
    RSpaceExporter, StateTransferFormat, StateTransferPage,
};
use crate::rspace::state::rspace_state_authority::RSpaceStateAuthority;

type Exporter = DurableRSpaceExporterV1<Profile>;
type Outcome = Result<StateTransferPage, StateExportError>;
type Installed = [[bool; BODIES.len()]; GROUPS.len()];

const CHANNELS: [&str; 3] = ["a", "b", "c"];
const VALUES: [&str; 3] = ["v0", "v1", "v2"];
const BODIES: [&str; 2] = ["k0", "k1"];
const GROUPS: [&[&str]; 3] = [&["a"], &["a", "b"], &["c", "a", "b"]];
const WIDE_FIRST_CHANNEL: u8 = b'!';
const WIDE_LAST_CHANNEL: u8 = b'~';
const WIDE_VALUE: &str = "wide";
const GENERATED_SCRIPTS: u32 = 12;
const MAX_BATCHES: usize = 3;
const MAX_BATCH_STEPS: usize = 5;
const GENEROUS_PAGE_BYTES: u64 = 1 << 20;
const TIGHT_PAGE_BYTES: u64 = 2048;
const SCRIPT_LIMITS: [(u64, u64); 4] = [
    (GENEROUS_PAGE_BYTES, 1),
    (GENEROUS_PAGE_BYTES, 3),
    (TIGHT_PAGE_BYTES, 16),
    (GENEROUS_PAGE_BYTES, 1_024),
];
const FOLLOWED_PAGE_BUDGET: usize = 4_096;
const BULK_CHANNELS: usize = 1_100;
const CONCURRENT_REQUESTERS: usize = 8;
const SEQUENCE: [(usize, u64); 10] =
    [(0, 0), (1, 0), (0, 2), (1, 2), (0, 4), (1, 1), (1, 4), (0, 1), (0, 2), (0, 5)];
const REBUILT: [(usize, u64); 2] = [(0, 4), (1, 2)];
const GROUPED: [(usize, u64); 4] = [(0, 4), (1, 5), (0, 1), (0, 6)];
const RESUMED_PAGE_DIFFERS: &str =
    "the page at a resumed position differs from the uninterrupted session";

fn page_limits(max_page_bytes: u64, max_page_objects: u64) -> TransferPageLimitsV1 {
    TransferPageLimitsV1::new(max_page_bytes, max_page_objects)
        .expect("the test page limits admit at least one object")
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

fn owned(names: &[&str]) -> Vec<String> { names.iter().map(|name| (*name).to_owned()).collect() }

fn put_datum(hot: &Store, channel: &str, value: &str) {
    let channel = channel.to_owned();
    hot.put_datum(&channel, Datum::create(&channel, value.to_owned(), false));
}

fn install_continuation(hot: &Store, group: &[&str], body: &str) {
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
    for member in channels.iter().collect::<BTreeSet<_>>() {
        hot.put_join(member, &channels)
            .expect("the hot store records the join");
    }
}

fn publish(
    owner: &mut DurableRSpaceAuthorityV1<Profile>,
    gate: &Arc<RwLock<()>>,
    hot: &Store,
) -> DurableStateRootV1 {
    let state = poll_checkpoint_with(owner, gate, hot, publication_limits())
        .expect("the owner publishes the state");
    DurableStateRootV1::of_manifest(state.manifest())
}

fn cursor_at(root: &KeyHash, position: u64) -> NodePath {
    let mut cursor = Vec::with_capacity(2);
    cursor.push((root.clone(), None));
    match position {
        0 => {}
        later => {
            let mut component = [0_u8; 32];
            component[24..].copy_from_slice(&later.to_be_bytes());
            cursor.push((Blake2b256Hash::from_bytes(component.to_vec()), None));
        }
    }
    cursor
}

fn component(prefix: [u8; 24], count: u64) -> KeyHash {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(&prefix);
    bytes.extend_from_slice(&count.to_be_bytes());
    Blake2b256Hash::from_bytes(bytes)
}

fn sized(len: usize, last: u8) -> KeyHash {
    let mut bytes = vec![0_u8; len];
    if let Some(byte) = bytes.last_mut() {
        *byte = last;
    }
    Blake2b256Hash::from_bytes(bytes)
}

fn follow(exporter: &dyn RSpaceExporter, root: &KeyHash) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    let mut cursor = cursor_at(root, 0);
    while outcomes.len() < FOLLOWED_PAGE_BUDGET {
        let outcome = exporter.get_transfer_page(&cursor);
        let next = match &outcome {
            Ok(page) if !page.terminal && page.next_cursor != cursor => {
                Some(page.next_cursor.clone())
            }
            _ => None,
        };
        outcomes.push(outcome);
        match next {
            Some(next) => cursor = next,
            None => break,
        }
    }
    outcomes
}

struct ReferencePage {
    start: u64,
    next: u64,
    bytes: Vec<u8>,
}

struct ReferenceStream {
    pages: Vec<ReferencePage>,
    refusal: Option<TransferExportErrorV1>,
}

fn reference_stream(
    view: &DurableAuthorityViewV1<Profile>,
    root: DurableStateRootV1,
    limits: TransferPageLimitsV1,
) -> ReferenceStream {
    let mut session = view
        .open_export(root, limits)
        .expect("open an export of a retained state");
    let mut token = session.first_token();
    let mut pages = Vec::new();
    loop {
        match session.page(token) {
            Ok(page) => {
                let next = TransferPageViewV1::parse(page, limits)
                    .expect("an exported page parses")
                    .next_token();
                pages.push(ReferencePage {
                    start: token.position(),
                    next: next.position(),
                    bytes: page.to_vec(),
                });
                match next == token {
                    true => {
                        return ReferenceStream {
                            pages,
                            refusal: None,
                        };
                    }
                    false => token = next,
                }
            }
            Err(refusal) => {
                return ReferenceStream {
                    pages,
                    refusal: Some(refusal),
                };
            }
        }
    }
}

fn expected_outcomes(root: &KeyHash, reference: &ReferenceStream) -> Vec<Outcome> {
    let mut expected = Vec::with_capacity(reference.pages.len() + 1);
    for page in &reference.pages {
        expected.push(Ok(StateTransferPage {
            bytes: page.bytes.clone(),
            next_cursor: cursor_at(root, page.next),
            terminal: page.next == page.start,
        }));
    }
    match &reference.refusal {
        Some(oversized @ TransferExportErrorV1::ObjectExceedsPage { .. }) => {
            expected.push(Err(StateExportError::Store(KvStoreError::SerializationError(
                oversized.to_string(),
            ))));
        }
        Some(other) => {
            panic!("a reference session refused something but an oversized object: {other}")
        }
        None => {}
    }
    expected
}

fn describe(outcome: &Outcome) -> String {
    match outcome {
        Ok(page) => format!(
            "a page of {} bytes ending in {:02x?}, next cursor {:?}, terminal {}",
            page.bytes.len(),
            &page.bytes[page.bytes.len().saturating_sub(8)..],
            page.next_cursor,
            page.terminal
        ),
        Err(refusal) => format!("the refusal {refusal:?}"),
    }
}

fn first_difference(served: &[Outcome], expected: &[Outcome]) -> Option<String> {
    let differing = served
        .iter()
        .zip(expected)
        .position(|(served, expected)| served != expected);
    match (differing, served.len() == expected.len()) {
        (Some(index), _) => Some(format!(
            "outcome {index}: the trait returned {}; one session yields {}",
            describe(&served[index]),
            describe(&expected[index])
        )),
        (None, false) => Some(format!(
            "the trait returned {} outcomes; one session yields {}",
            served.len(),
            expected.len()
        )),
        (None, true) => None,
    }
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Put {
        channel_index: usize,
        value_index: usize,
    },
    Install {
        group_index: usize,
        body_index: usize,
    },
    Widen,
}

fn apply(hot: &Store, installed: &mut Installed, step: Step) {
    match step {
        Step::Put {
            channel_index,
            value_index,
        } => put_datum(hot, CHANNELS[channel_index], VALUES[value_index]),
        Step::Install {
            group_index,
            body_index,
        } => match std::mem::replace(&mut installed[group_index][body_index], true) {
            true => {}
            false => install_continuation(hot, GROUPS[group_index], BODIES[body_index]),
        },
        Step::Widen => {
            for byte in WIDE_FIRST_CHANNEL..=WIDE_LAST_CHANNEL {
                put_datum(hot, &char::from(byte).to_string(), WIDE_VALUE);
            }
        }
    }
}

fn seed_scripts() -> [Vec<Vec<Step>>; 3] {
    [
        vec![
            vec![
                Step::Put {
                    channel_index: 0,
                    value_index: 0,
                },
                Step::Put {
                    channel_index: 0,
                    value_index: 1,
                },
                Step::Put {
                    channel_index: 1,
                    value_index: 0,
                },
                Step::Install {
                    group_index: 1,
                    body_index: 0,
                },
            ],
            vec![
                Step::Widen,
                Step::Install {
                    group_index: 2,
                    body_index: 1,
                },
                Step::Put {
                    channel_index: 0,
                    value_index: 0,
                },
            ],
            vec![
                Step::Put {
                    channel_index: 2,
                    value_index: 2,
                },
                Step::Install {
                    group_index: 0,
                    body_index: 0,
                },
            ],
        ],
        vec![vec![Step::Put {
            channel_index: 1,
            value_index: 1,
        }]],
        vec![Vec::new()],
    ]
}

fn generated_steps() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (0..CHANNELS.len(), 0..VALUES.len()).prop_map(|(channel_index, value_index)| {
            Step::Put {
                channel_index,
                value_index,
            }
        }),
        3 => (0..GROUPS.len(), 0..BODIES.len()).prop_map(|(group_index, body_index)| {
            Step::Install {
                group_index,
                body_index,
            }
        }),
        1 => Just(Step::Widen),
    ]
}

fn generated_scripts() -> impl Strategy<Value = Vec<Vec<Step>>> {
    prop::collection::vec(
        prop::collection::vec(generated_steps(), 0..=MAX_BATCH_STEPS),
        1..=MAX_BATCHES,
    )
}

#[derive(Clone, Copy, Debug, Default)]
struct BindingCoverage {
    scripts: u32,
    scripts_of_several_states: u32,
    streams: u32,
    streams_of_several_pages: u32,
    streams_with_a_page_of_several_objects: u32,
    streams_completed_within_the_tight_byte_limit: u32,
    streams_refused_at_an_oversized_object_after_a_page: u32,
}

impl BindingCoverage {
    fn merge(&mut self, other: Self) {
        let Self {
            scripts,
            scripts_of_several_states,
            streams,
            streams_of_several_pages,
            streams_with_a_page_of_several_objects,
            streams_completed_within_the_tight_byte_limit,
            streams_refused_at_an_oversized_object_after_a_page,
        } = other;
        self.scripts += scripts;
        self.scripts_of_several_states += scripts_of_several_states;
        self.streams += streams;
        self.streams_of_several_pages += streams_of_several_pages;
        self.streams_with_a_page_of_several_objects += streams_with_a_page_of_several_objects;
        self.streams_completed_within_the_tight_byte_limit +=
            streams_completed_within_the_tight_byte_limit;
        self.streams_refused_at_an_oversized_object_after_a_page +=
            streams_refused_at_an_oversized_object_after_a_page;
    }

    fn assert_complete(&self) {
        let Self {
            scripts,
            scripts_of_several_states,
            streams,
            streams_of_several_pages,
            streams_with_a_page_of_several_objects,
            streams_completed_within_the_tight_byte_limit,
            streams_refused_at_an_oversized_object_after_a_page,
        } = *self;
        assert!(
            [
                scripts,
                scripts_of_several_states,
                streams,
                streams_of_several_pages,
                streams_with_a_page_of_several_objects,
                streams_completed_within_the_tight_byte_limit,
                streams_refused_at_an_oversized_object_after_a_page,
            ]
            .into_iter()
            .all(|count| count > 0),
            "a class of transfer stream never occurred: {self:?}"
        );
    }
}

fn check_script(script: &[Vec<Step>]) -> Result<BindingCoverage, TestCaseError> {
    let (scratch, hot, gate, mut owner) = fixture();
    let mut installed = Installed::default();
    let mut roots = Vec::with_capacity(script.len() + 1);
    roots.push(owner.current_root());
    for batch in script {
        for &step in batch {
            apply(&hot, &mut installed, step);
        }
        roots.push(publish(&mut owner, &gate, &hot));
    }
    let published = owner.published_core();
    let mut served = Vec::with_capacity(SCRIPT_LIMITS.len() * roots.len());
    for (max_page_bytes, max_page_objects) in SCRIPT_LIMITS {
        let exporter: Arc<dyn RSpaceExporter> = Arc::new(Exporter::with_limits(
            published.clone(),
            page_limits(max_page_bytes, max_page_objects),
        ));
        prop_assert_eq!(exporter.transfer_format(), StateTransferFormat::DagStreamV1);
        for &root in &roots {
            served.push(follow(&*exporter, &KeyHash::from(root)));
        }
    }
    drop(published);
    drop(owner);

    let view = recover(&scratch);
    let mut coverage = BindingCoverage {
        scripts: 1,
        scripts_of_several_states: u32::from(roots.len() > 2),
        ..BindingCoverage::default()
    };
    let mut streams = served.iter();
    for (max_page_bytes, max_page_objects) in SCRIPT_LIMITS {
        let limits = page_limits(max_page_bytes, max_page_objects);
        for &root in &roots {
            let reference = reference_stream(&view, root, limits);
            let outcomes = streams
                .next()
                .expect("one stream was served for every page limit and root");
            prop_assert_eq!(
                first_difference(outcomes, &expected_outcomes(&KeyHash::from(root), &reference)),
                None,
                "the pages of root {} under {:?} differ from the pages of one session",
                root,
                limits
            );
            coverage.streams += 1;
            coverage.streams_of_several_pages += u32::from(reference.pages.len() >= 3);
            coverage.streams_with_a_page_of_several_objects += u32::from(
                reference
                    .pages
                    .iter()
                    .any(|page| page.next - page.start >= 2),
            );
            coverage.streams_completed_within_the_tight_byte_limit +=
                u32::from(max_page_bytes == TIGHT_PAGE_BYTES && reference.refusal.is_none());
            coverage.streams_refused_at_an_oversized_object_after_a_page +=
                u32::from(reference.refusal.is_some() && !reference.pages.is_empty());
        }
    }
    Ok(coverage)
}

#[test]
fn pages_served_through_the_trait_equal_the_pages_of_one_session() {
    let mut seeded = BindingCoverage::default();
    for script in seed_scripts() {
        seeded.merge(check_script(&script).expect("a seeded script satisfies TX-07"));
    }
    println!("transfer export binding seeds: {seeded:?}");
    seeded.assert_complete();
    let generated = Cell::new(BindingCoverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: GENERATED_SCRIPTS,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&generated_scripts(), |script| {
        let mut total = generated.get();
        total.merge(check_script(&script)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated scripts satisfy TX-07");
    println!("transfer export binding generated: {:?}", generated.get());
    assert!(generated.get().scripts >= GENERATED_SCRIPTS);

    let (scratch, hot, gate, mut owner) = fixture();
    for index in 0..BULK_CHANNELS {
        put_datum(&hot, &format!("bulk-{index:04}"), "bulk");
    }
    let root = publish(&mut owner, &gate, &hot);
    let root_hash = KeyHash::from(root);
    let repository = DurableHistoryRepositoryV1::<Profile>::new(owner.published_core())
        .expect("a repository view at the owner's current state");
    let exporter = repository.exporter();
    let format = exporter.transfer_format();
    let served = follow(&*exporter, &root_hash);
    drop(exporter);
    drop(repository);
    drop(owner);

    let view = recover(&scratch);
    let reference = reference_stream(
        &view,
        root,
        page_limits(DAG_TRANSFER_PAGE_MAX_BYTES, DAG_TRANSFER_PAGE_MAX_OBJECTS),
    );
    let counts = reference
        .pages
        .iter()
        .map(|page| page.next - page.start)
        .collect::<Vec<_>>();

    assert_eq!(
        (
            DAG_TRANSFER_PAGE_MAX_BYTES,
            DAG_TRANSFER_PAGE_MAX_OBJECTS,
            DAG_TRANSFER_SESSION_LIMIT,
            format
        ),
        (64 * 1024 * 1024, 1_024, 4, StateTransferFormat::DagStreamV1),
        "the durable exporter serves the DAG stream in pages of at most 64 MiB and 1,024 objects \
         from at most 4 sessions"
    );
    assert_eq!(
        first_difference(&served, &expected_outcomes(&root_hash, &reference)),
        None,
        "the pages the repository's exporter serves differ from the pages of one session under \
         the production limits"
    );
    assert!(
        counts.len() >= 3 && counts[0] == DAG_TRANSFER_PAGE_MAX_OBJECTS && counts.ends_with(&[0]),
        "a state of more than 1,024 objects fills its first page to the object limit and ends \
         with the empty page: {counts:?}"
    );
}

fn object_count(exporter: &Exporter, root: &KeyHash) -> u64 {
    let mut position = 0;
    loop {
        match exporter.get_transfer_page(&cursor_at(root, position)) {
            Ok(page) if page.terminal => return position,
            Ok(_) => position += 1,
            Err(refusal) => panic!("the probe exporter refused position {position}: {refusal:?}"),
        }
    }
}

fn served_session(
    view: &DurableAuthorityViewV1<Profile>,
    root: DurableStateRootV1,
    limits: TransferPageLimitsV1,
) -> (DurableExportSessionV1<Profile>, u64) {
    let mut session = view
        .open_export(root, limits)
        .expect("open an export of a retained state");
    let mut token = session.first_token();
    loop {
        let page = session
            .page(token)
            .expect("the session serves its next page");
        let next = TransferPageViewV1::parse(page, limits)
            .expect("an exported page parses")
            .next_token();
        match next == token {
            true => return (session, token.position()),
            false => token = next,
        }
    }
}

fn assert_resumed_page(
    outcome: &Outcome,
    session: &mut DurableExportSessionV1<Profile>,
    position: u64,
) {
    let token = TransferTokenV1::new(session.binding(), position);
    let expected = session
        .page(token)
        .expect("a session that served the whole stream serves each of its positions");
    assert_eq!(
        outcome.as_ref().map(|page| page.bytes.as_slice()),
        Ok(expected),
        "{RESUMED_PAGE_DIFFERS}"
    );
}

#[test]
fn a_position_no_session_has_served_is_reached_and_sessions_stay_bounded() {
    let (scratch, hot, gate, mut owner) = fixture();
    for value in ["v0", "v1", "v2", "v3", "v4", "v5"] {
        put_datum(&hot, "resumed", value);
    }
    install_continuation(&hot, &["resumed", "other"], "body");
    let first = publish(&mut owner, &gate, &hot);
    for value in ["v6", "v7"] {
        put_datum(&hot, "other", value);
    }
    let second = publish(&mut owner, &gate, &hot);
    let roots = [first, second];
    let hashes = roots.map(KeyHash::from);
    let limits = page_limits(GENEROUS_PAGE_BYTES, 1);
    let grouped_limits = page_limits(GENEROUS_PAGE_BYTES, 3);
    let published = owner.published_core();
    let registry = owner.generation_leases();

    let probe = Exporter::with_limits(published.clone(), limits);
    let lengths = [object_count(&probe, &hashes[0]), object_count(&probe, &hashes[1])];
    drop(probe);
    let idle_leases = registry.active_lease_count_for_test();

    let exporter = Exporter::with_limits(published.clone(), limits);
    let mut sequence = Vec::with_capacity(SEQUENCE.len());
    for (root_index, position) in SEQUENCE {
        let outcome = exporter.get_transfer_page(&cursor_at(&hashes[root_index], position));
        let live = exporter.live_sessions_for_test();
        sequence.push((outcome, live, registry.active_lease_count_for_test() - idle_leases));
    }
    let end = exporter.get_transfer_page(&cursor_at(&hashes[0], lengths[0]));
    let live_at_end = exporter.live_sessions_for_test();
    let beyond = [lengths[1] + 1, u64::MAX].map(|position| {
        (
            exporter.get_transfer_page(&cursor_at(&hashes[1], position)),
            exporter.live_sessions_for_test(),
            registry.active_lease_count_for_test() - idle_leases,
        )
    });
    drop(exporter);
    let leases_after_drop = registry.active_lease_count_for_test();

    let rebuilt = Exporter::with_limits(published.clone(), limits);
    let rebuilt_outcomes = REBUILT.map(|(root_index, position)| {
        rebuilt.get_transfer_page(&cursor_at(&hashes[root_index], position))
    });
    let rebuilt_live = rebuilt.live_sessions_for_test();
    drop(rebuilt);

    let grouped = Exporter::with_limits(published.clone(), grouped_limits);
    let grouped_outcomes = GROUPED.map(|(root_index, position)| {
        grouped.get_transfer_page(&cursor_at(&hashes[root_index], position))
    });
    let grouped_live = grouped.live_sessions_for_test();
    drop(grouped);

    let concurrent = Exporter::with_limits(published.clone(), limits);
    let walks = std::thread::scope(|scope| {
        let walkers = (0..CONCURRENT_REQUESTERS)
            .map(|walker| {
                let (exporter, root, end) = (&concurrent, &hashes[walker % 2], lengths[walker % 2]);
                scope.spawn(move || {
                    (0..=end)
                        .map(|position| exporter.get_transfer_page(&cursor_at(root, position)))
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        walkers
            .into_iter()
            .map(|walker| {
                walker
                    .join()
                    .expect("a concurrent requester does not panic")
            })
            .collect::<Vec<_>>()
    });
    let concurrent_live = concurrent.live_sessions_for_test();
    let concurrent_leases = registry.active_lease_count_for_test() - idle_leases;
    drop(concurrent);

    let unwound = Exporter::with_limits(published.clone(), limits);
    let before_the_panic = unwound.get_transfer_page(&cursor_at(&hashes[0], 0));
    let hook = journal_owner_hooks::arm(JournalOwnerHookPoint::AfterPublishedCoreLoad, || {
        panic!("the published core load panics inside a transfer request")
    });
    let panicked =
        catch_unwind(AssertUnwindSafe(|| unwound.get_transfer_page(&cursor_at(&hashes[1], 2))))
            .is_err();
    drop(hook);
    let live_after_the_panic = unwound.live_sessions_for_test();
    let leases_after_the_panic = registry.active_lease_count_for_test() - idle_leases;
    let after_the_panic = unwound.get_transfer_page(&cursor_at(&hashes[1], 2));
    let live_after_the_retry = unwound.live_sessions_for_test();
    drop(unwound);
    drop(published);
    drop(owner);

    let view = recover(&scratch);
    let mut references = roots.map(|root| served_session(&view, root, limits));
    let mut grouped_references = roots.map(|root| served_session(&view, root, grouped_limits));
    for ((root_index, position), (outcome, ..)) in SEQUENCE.into_iter().zip(&sequence) {
        assert_resumed_page(outcome, &mut references[root_index].0, position);
    }
    assert_resumed_page(&end, &mut references[0].0, lengths[0]);
    for ((root_index, position), outcome) in REBUILT.into_iter().zip(&rebuilt_outcomes) {
        assert_resumed_page(outcome, &mut references[root_index].0, position);
    }
    for ((root_index, position), outcome) in GROUPED.into_iter().zip(&grouped_outcomes) {
        assert_resumed_page(outcome, &mut grouped_references[root_index].0, position);
    }
    for (walker, walk) in walks.iter().enumerate() {
        for (position, outcome) in (0..).zip(walk) {
            assert_resumed_page(outcome, &mut references[walker % 2].0, position);
        }
    }
    assert_resumed_page(&before_the_panic, &mut references[0].0, 0);
    assert_resumed_page(&after_the_panic, &mut references[1].0, 2);

    assert_eq!(
        (references[0].1, references[1].1, lengths.map(|length| length >= 6)),
        (lengths[0], lengths[1], [true, true]),
        "the probe exporter counted the objects of both states, and each holds the six objects \
         the request sequence addresses"
    );
    assert_eq!(
        sequence
            .iter()
            .map(|(_, live, _)| live.clone())
            .collect::<Vec<_>>(),
        [
            vec![Some((first, 1))],
            vec![Some((first, 1)), Some((second, 1))],
            vec![Some((first, 1)), Some((second, 1)), Some((first, 3))],
            vec![Some((first, 1)), Some((second, 1)), Some((first, 3)), Some((second, 3))],
            vec![Some((first, 5)), Some((second, 1)), Some((first, 3)), Some((second, 3))],
            vec![Some((first, 5)), Some((second, 2)), Some((first, 3)), Some((second, 3))],
            vec![Some((first, 5)), Some((second, 2)), Some((second, 5)), Some((second, 3))],
            vec![Some((first, 5)), Some((second, 2)), Some((second, 5)), Some((second, 3))],
            vec![Some((first, 5)), Some((second, 2)), Some((second, 5)), Some((second, 3))],
            vec![Some((first, 6)), Some((second, 2)), Some((second, 5)), Some((second, 3))],
        ],
        "a vacant slot is filled first, then the least recently used session is released; a \
         position that no session has served is reached by a new session advanced from position \
         0, and a served position is answered by the session that served it"
    );
    assert!(
        sequence.iter().all(|(outcome, live, leases)| {
            matches!(outcome, Ok(page) if !page.terminal) &&
                live.len() <= DAG_TRANSFER_SESSION_LIMIT &&
                *leases == live.len()
        }) && live_at_end.len() <= DAG_TRANSFER_SESSION_LIMIT &&
            matches!(&end, Ok(page) if page.terminal),
        "every live session holds one generation lease, never more than \
         {DAG_TRANSFER_SESSION_LIMIT} are live, and the page at the end of the stream is the \
         terminal page"
    );
    let [
        (beyond_the_end, live_beyond_the_end, leases_beyond_the_end),
        (far_beyond, live_far_beyond, leases_far_beyond),
    ] = beyond;
    assert_eq!(
        (beyond_the_end, far_beyond),
        (
            Err(StateExportError::InvalidPath(format!(
                "DAG transfer position {} lies beyond the {} objects of the state",
                lengths[1] + 1,
                lengths[1]
            ))),
            Err(StateExportError::InvalidPath(format!(
                "DAG transfer position {} lies beyond the {} objects of the state",
                u64::MAX,
                lengths[1]
            ))),
        ),
        "a position beyond the end of the stream is refused"
    );
    assert!(
        live_beyond_the_end.contains(&Some((second, lengths[1]))) &&
            live_far_beyond == live_beyond_the_end &&
            leases_beyond_the_end == live_beyond_the_end.len() &&
            leases_far_beyond == leases_beyond_the_end &&
            live_beyond_the_end.len() <= DAG_TRANSFER_SESSION_LIMIT,
        "the first request beyond the end advances one session to the end of the stream and the \
         second is refused by that session: {live_beyond_the_end:?} then {live_far_beyond:?}"
    );
    assert_eq!(
        leases_after_drop, idle_leases,
        "dropping the exporter releases every session's lease"
    );
    assert_eq!(
        (rebuilt_outcomes, rebuilt_live),
        (
            REBUILT.map(|request| {
                let earlier = SEQUENCE
                    .iter()
                    .position(|asked| *asked == request)
                    .expect("the rebuilt exporter repeats requests of the sequence");
                sequence[earlier].0.clone()
            }),
            vec![Some((first, 5)), Some((second, 3))],
        ),
        "an exporter that was dropped and rebuilt returns the pages that the first exporter \
         returned, from new sessions that it advanced from position 0"
    );
    assert_eq!(
        grouped_live,
        [Some((first, lengths[0].min(9))), Some((second, lengths[1].min(8)))],
        "a new exporter reaches a position inside a page by advancing a new session from position \
         0 and then answers earlier and later served positions from that session"
    );
    assert!(
        concurrent_live.len() <= DAG_TRANSFER_SESSION_LIMIT &&
            concurrent_live.iter().all(Option::is_some) &&
            concurrent_leases == concurrent_live.len(),
        "after {CONCURRENT_REQUESTERS} concurrent requesters no session is checked out and no \
         more than {DAG_TRANSFER_SESSION_LIMIT} are live: {concurrent_live:?} with \
         {concurrent_leases} leases"
    );
    assert_eq!(
        (panicked, live_after_the_panic, leases_after_the_panic, live_after_the_retry),
        (true, vec![Some((first, 1))], 1, vec![Some((first, 1)), Some((second, 3))]),
        "a request that panics while it opens a session leaves no slot checked out, and the same \
         request then opens its session"
    );
}

#[test]
fn a_cursor_of_another_shape_is_refused() {
    let (_scratch, hot, gate, mut owner) = fixture();
    put_datum(&hot, "cursor", "value");
    let retained = KeyHash::from(publish(&mut owner, &gate, &hot));
    let unretained = Blake2b256Hash::from_bytes(vec![0xA5; 32]);
    let registry = owner.generation_leases();
    let repository = DurableHistoryRepositoryV1::<Profile>::new(owner.published_core())
        .expect("a repository view at the owner's current state");
    let exporter = repository.exporter();
    let idle_leases = registry.active_lease_count_for_test();

    let positions = [0, 1, 255, 256, u64::MAX];
    let built = positions.map(|position| dag_transfer_cursor(&retained, position));
    let parsed = positions.map(|position| {
        parse_dag_transfer_cursor(&cursor_at(&retained, position))
            .map(|(root, position)| (root.clone(), position))
    });
    let position_one = component([0; 24], 1);
    let mut leading = [0_u8; 24];
    leading[0] = 1;
    let mut trailing = [0_u8; 24];
    trailing[23] = 1;
    let malformed: [(&str, NodePath); 14] = [
        ("an empty cursor", Vec::new()),
        ("three components", vec![
            (retained.clone(), None),
            (position_one.clone(), None),
            (component([0; 24], 2), None),
        ]),
        (
            "a legacy path of six components",
            (0..6)
                .map(|index| (retained.clone(), Some(index)))
                .collect(),
        ),
        ("a child index on the root alone", vec![(retained.clone(), Some(0))]),
        ("a child index on the root before a position", vec![
            (retained.clone(), Some(0)),
            (position_one.clone(), None),
        ]),
        ("a child index on the position", vec![
            (retained.clone(), None),
            (position_one.clone(), Some(0)),
        ]),
        ("a position whose first prefix byte is not zero", vec![
            (retained.clone(), None),
            (component(leading, 1), None),
        ]),
        ("a position whose last prefix byte is not zero", vec![
            (retained.clone(), None),
            (component(trailing, 1), None),
        ]),
        ("a position of zero", vec![(retained.clone(), None), (component([0; 24], 0), None)]),
        ("a position of 31 bytes", vec![(retained.clone(), None), (sized(31, 1), None)]),
        ("a position of 33 bytes", vec![(retained.clone(), None), (sized(33, 1), None)]),
        ("a root of 31 bytes", vec![(sized(31, 1), None)]),
        ("a root of 33 bytes before a position", vec![
            (sized(33, 1), None),
            (position_one.clone(), None),
        ]),
        ("an empty root", vec![(sized(0, 0), None)]),
    ];
    let accepted = malformed
        .iter()
        .filter(|(_, cursor)| {
            !matches!(parse_dag_transfer_cursor(cursor), Err(StateExportError::InvalidPath(_))) ||
                !matches!(
                    exporter.get_transfer_page(cursor),
                    Err(StateExportError::InvalidPath(_))
                )
        })
        .map(|(shape, _)| *shape)
        .collect::<Vec<_>>();
    let unretained_outcomes =
        [0, 3].map(|position| exporter.get_transfer_page(&cursor_at(&unretained, position)));
    let leases_after_the_refusals = registry.active_lease_count_for_test();
    let served = exporter.get_transfer_page(&cursor_at(&retained, 0));
    let leases_after_a_page = registry.active_lease_count_for_test();
    let legacy = RSpaceExporterStore::create(RSpaceStateAuthority::in_memory());
    let legacy_outcomes =
        (legacy.transfer_format(), legacy.get_transfer_page(&cursor_at(&retained, 0)));

    assert_eq!(
        built,
        positions.map(|position| cursor_at(&retained, position)),
        "the cursor of position 0 is the root alone, and a later position follows the root as 24 \
         zero bytes and the object count in 8 big-endian bytes"
    );
    assert_eq!(
        parsed,
        positions.map(|position| Ok((retained.clone(), position))),
        "the parser reads back the root and the position of each of the two shapes"
    );
    assert_eq!(
        accepted,
        Vec::<&str>::new(),
        "the parser and the exporter refuse every other shape with InvalidPath"
    );
    assert_eq!(
        unretained_outcomes,
        [
            Err(StateExportError::MissingHistoryObject(unretained.clone())),
            Err(StateExportError::MissingHistoryObject(unretained)),
        ],
        "a well-formed cursor of a root the store does not retain is refused as a missing object"
    );
    assert_eq!(
        (leases_after_the_refusals, leases_after_a_page),
        (idle_leases, idle_leases + 1),
        "a refused cursor opens no session, and the first page of a retained root opens one"
    );
    assert!(
        matches!(&served, Ok(page) if !page.terminal),
        "the exporter that refused the other shapes serves the retained root"
    );
    assert_eq!(
        (exporter.transfer_format(), legacy_outcomes),
        (
            StateTransferFormat::DagStreamV1,
            (
                StateTransferFormat::LegacyRadix,
                Err(StateExportError::BackendMismatch(AuthorityBackendMismatch::History)),
            ),
        ),
        "the durable exporter reports the DAG stream, and an exporter that does not override the \
         two methods reports the legacy format and refuses a transfer page"
    );
}
