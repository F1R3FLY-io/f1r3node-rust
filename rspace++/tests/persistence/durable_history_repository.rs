use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic::resume_unwind;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, SystemTime};

use proptest::prelude::*;
use tokio::sync::RwLock;

use super::checkpoint_wal::fixture as checkpoint_source_fixture;
use super::durable_publication::{install_checkpoint_image, publish_data_successor};
use super::durable_recovery::{checkpoint_image_from_source, limits};
use super::durable_rspace_authority::{
    Profile, Store, fixture, poll_checkpoint_with, publication_limits, store_files,
};
use super::support::{PersistenceTrialDir, TestSemanticProfile, trial_scratch};
use crate::rspace::errors::{AuthorityBackendMismatch, HistoryError, RootError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::CurrentRoot;
use crate::rspace::history::durable_history_repository::DurableHistoryRepositoryV1;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::history::history_repository::{HistoryRepository, HistoryRepositoryInstances};
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::durable::journal_owner_hooks::{
    self, JournalOwnerHookGuard, JournalOwnerHookPoint,
};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableDirectoryV1, DurableIoFailpointV1, DurablePublishedCoreV1,
    DurableRSpaceAuthorityV1, DurableStateRootV1, SemanticallyAdmittedDurableWriterV1,
    arm_durable_io_failpoint, armed_durable_io_failpoint, index_file_name, pack_file_name,
};
use crate::rspace::persistence::{
    CanonicalDagNode, DurableStateReadHandleErrorV1, GenerationLeaseError, GenerationLeaseKeyV1,
    IndexGenerationV1, LogPackDecodeLimits, ObjectDigest, PackId,
};
use crate::rspace::shared::trie_exporter::StateExportError;
use crate::rspace::state::import_session::{
    StateImportError, StateImportPublicationMode, StateImportSpec,
};
use crate::rspace::state::rspace_state_authority::RSpaceStateAuthority;

type Repository = DurableHistoryRepositoryV1<Profile>;
type SharedOwner = Arc<Mutex<DurableRSpaceAuthorityV1<Profile>>>;
type Continuation = WaitingContinuation<String, String>;
type ResetOutcome = (
    Result<(Blake2b256Hash, Contents), HistoryError>,
    Result<Blake2b256Hash, HistoryError>,
    Result<bool, HistoryError>,
);

const CHANNELS: [&str; 3] = ["a", "b", "c"];
const VALUES: [&str; 2] = ["v0", "v1"];
const BODIES: [&str; 2] = ["k0", "k1"];
const GROUPS: [&[&str]; 4] = [&["a"], &["a", "b"], &["b", "a"], &["c", "a", "b"]];
const UNSTORED_GROUPS: [&[&str]; 3] = [&["b"], &["a", "a"], &["a", "b", "c"]];
const GENERATED_SCRIPTS: u32 = 24;
const MAX_BATCHES: usize = 3;
const MAX_BATCH_STEPS: usize = 6;
const ABSENT_DIGEST: [u8; 32] = [0xA5; 32];
const RELEASE_TIMEOUT: Duration = Duration::from_secs(30);
const DIRECTORY_FAILPOINTS: [DurableIoFailpointV1; 16] = [
    DurableIoFailpointV1::PhysicalWrite,
    DurableIoFailpointV1::PhysicalSync,
    DurableIoFailpointV1::CandidateWrite,
    DurableIoFailpointV1::CandidateSetLength,
    DurableIoFailpointV1::CandidateSync,
    DurableIoFailpointV1::BeforeReplace,
    DurableIoFailpointV1::AfterReplaceBeforeDirectorySync,
    DurableIoFailpointV1::DirectorySync,
    DurableIoFailpointV1::CatalogSlotWrite,
    DurableIoFailpointV1::CatalogSync,
    DurableIoFailpointV1::CatalogDirectorySync,
    DurableIoFailpointV1::ReclamationIntentCreate,
    DurableIoFailpointV1::ReclamationIntentSetLength,
    DurableIoFailpointV1::ReclamationIntentSync,
    DurableIoFailpointV1::ReclamationIntentBeforeRename,
    DurableIoFailpointV1::ReclamationIntentDirectorySync,
];

const fn directory_failpoint_index(failpoint: DurableIoFailpointV1) -> usize {
    match failpoint {
        DurableIoFailpointV1::PhysicalWrite => 0,
        DurableIoFailpointV1::PhysicalSync => 1,
        DurableIoFailpointV1::CandidateWrite => 2,
        DurableIoFailpointV1::CandidateSetLength => 3,
        DurableIoFailpointV1::CandidateSync => 4,
        DurableIoFailpointV1::BeforeReplace => 5,
        DurableIoFailpointV1::AfterReplaceBeforeDirectorySync => 6,
        DurableIoFailpointV1::DirectorySync => 7,
        DurableIoFailpointV1::CatalogSlotWrite => 8,
        DurableIoFailpointV1::CatalogSync => 9,
        DurableIoFailpointV1::CatalogDirectorySync => 10,
        DurableIoFailpointV1::ReclamationIntentCreate => 11,
        DurableIoFailpointV1::ReclamationIntentSetLength => 12,
        DurableIoFailpointV1::ReclamationIntentSync => 13,
        DurableIoFailpointV1::ReclamationIntentBeforeRename => 14,
        DurableIoFailpointV1::ReclamationIntentDirectorySync => 15,
    }
}

fn channel(name: &str) -> String { name.to_owned() }

fn group(names: &[&str]) -> Vec<String> { names.iter().copied().map(channel).collect() }

fn datum(channel_name: &str, value: &str) -> Datum<String> {
    Datum::create(&channel(channel_name), value.to_owned(), false)
}

fn continuation(names: &[&str], body: &str) -> Continuation {
    let patterns: Vec<String> = names.iter().map(|name| format!("pattern-{name}")).collect();
    WaitingContinuation::create(&group(names), &patterns, &body.to_owned(), false, BTreeSet::new())
}

fn index_of(names: &[&str], value: &str) -> usize {
    names
        .iter()
        .position(|name| *name == value)
        .expect("an enumerated value belongs to the test universe")
}

fn stored_group<T: AsRef<str>>(channels: &[T]) -> usize {
    GROUPS
        .iter()
        .position(|names| {
            names.len() == channels.len() &&
                names
                    .iter()
                    .zip(channels)
                    .all(|(name, stored)| *name == stored.as_ref())
        })
        .expect("the group is one of the stored groups")
}

fn hash_of(digest: ObjectDigest) -> Blake2b256Hash {
    Blake2b256Hash::from_bytes(digest.as_bytes().to_vec())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Contents {
    data: [[u64; VALUES.len()]; CHANNELS.len()],
    continuations: [[bool; BODIES.len()]; GROUPS.len()],
    joins: BTreeSet<(usize, usize)>,
}

impl Contents {
    fn holds(&self, names: &[&str]) -> bool {
        self.continuations[stored_group(names)].contains(&true)
    }

    fn derive_joins(&mut self) {
        self.joins = GROUPS
            .iter()
            .enumerate()
            .filter(|&(group_index, _)| self.continuations[group_index].contains(&true))
            .flat_map(|(group_index, names)| {
                names
                    .iter()
                    .map(move |name| (index_of(&CHANNELS, name), group_index))
            })
            .collect();
    }
}

fn snapshot(reader: &dyn HistoryReaderBase<String, String, String, String>) -> Contents {
    let mut contents = Contents::default();
    for (channel_index, name) in CHANNELS.into_iter().enumerate() {
        for stored in reader.get_data_canonical(&channel(name)) {
            let value_index = index_of(&VALUES, &stored.a);
            assert_eq!(stored, datum(name, VALUES[value_index]), "a durable datum differs");
            contents.data[channel_index][value_index] += 1;
        }
        for joined in reader.get_joins_canonical(&channel(name)) {
            assert!(
                contents
                    .joins
                    .insert((channel_index, stored_group(&joined))),
                "a durable join was read twice: {name} {joined:?}"
            );
        }
    }
    for (group_index, names) in GROUPS.into_iter().enumerate() {
        for stored in reader.get_continuations_canonical(&group(names)) {
            let body_index = index_of(&BODIES, &stored.continuation);
            assert_eq!(stored, continuation(names, BODIES[body_index]), "a continuation differs");
            assert!(
                !contents.continuations[group_index][body_index],
                "a durable continuation was read twice: {stored:?}"
            );
            contents.continuations[group_index][body_index] = true;
        }
    }
    for names in UNSTORED_GROUPS {
        assert!(
            reader.get_continuations_canonical(&group(names)).is_empty(),
            "a continuation was read under the unstored group {names:?}"
        );
    }
    contents
}

fn put_value(hot: &Store, channel_index: usize, value_index: usize) {
    let name = CHANNELS[channel_index];
    hot.put_datum(&channel(name), datum(name, VALUES[value_index]));
}

fn take_value(hot: &Store, channel_index: usize, value_index: usize) {
    let key = channel(CHANNELS[channel_index]);
    let index = hot
        .get_data(&key)
        .iter()
        .position(|stored| stored.a.as_str() == VALUES[value_index])
        .expect("the datum to take is stored");
    hot.remove_datum(&key, i32::try_from(index).expect("a datum index fits i32"))
        .expect("take the stored datum");
}

fn install(hot: &Store, group_index: usize, body_index: usize) {
    let names = GROUPS[group_index];
    let channels = group(names);
    assert_eq!(
        hot.put_continuation(&channels, continuation(names, BODIES[body_index])),
        Some(true),
        "the hot store takes a continuation it does not hold"
    );
    for member in channels.iter().collect::<BTreeSet<_>>() {
        hot.put_join(member, &channels)
            .expect("the hot store takes the join");
    }
}

fn withdraw(hot: &Store, group_index: usize, body_index: usize) {
    let channels = group(GROUPS[group_index]);
    let index = hot
        .get_continuations(&channels)
        .iter()
        .position(|stored| stored.continuation.as_str() == BODIES[body_index])
        .expect("the continuation to withdraw is stored");
    hot.remove_continuation(
        &channels,
        i32::try_from(index).expect("a continuation index fits i32"),
    )
    .expect("withdraw the stored continuation");
    for member in channels.iter().collect::<BTreeSet<_>>() {
        hot.remove_join(member, &channels)
            .expect("drop the join once no continuation of its group remains");
    }
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Put {
        channel_index: usize,
        value_index: usize,
    },
    Take {
        channel_index: usize,
        value_index: usize,
    },
    Install {
        group_index: usize,
        body_index: usize,
    },
    Withdraw {
        group_index: usize,
        body_index: usize,
    },
}

impl Step {
    fn put(channel_name: &str, value: &str) -> Self {
        Self::Put {
            channel_index: index_of(&CHANNELS, channel_name),
            value_index: index_of(&VALUES, value),
        }
    }

    fn take(channel_name: &str, value: &str) -> Self {
        Self::Take {
            channel_index: index_of(&CHANNELS, channel_name),
            value_index: index_of(&VALUES, value),
        }
    }

    fn install(names: &[&str], body: &str) -> Self {
        Self::Install {
            group_index: stored_group(names),
            body_index: index_of(&BODIES, body),
        }
    }

    fn withdraw(names: &[&str], body: &str) -> Self {
        Self::Withdraw {
            group_index: stored_group(names),
            body_index: index_of(&BODIES, body),
        }
    }
}

fn apply(hot: &Store, contents: &mut Contents, step: Step) -> bool {
    let removed = match step {
        Step::Put {
            channel_index,
            value_index,
        } => {
            put_value(hot, channel_index, value_index);
            contents.data[channel_index][value_index] += 1;
            false
        }
        Step::Take {
            channel_index,
            value_index,
        } => match contents.data[channel_index][value_index] {
            0 => false,
            stored => {
                take_value(hot, channel_index, value_index);
                contents.data[channel_index][value_index] = stored - 1;
                true
            }
        },
        Step::Install {
            group_index,
            body_index,
        } => match contents.continuations[group_index][body_index] {
            true => false,
            false => {
                install(hot, group_index, body_index);
                contents.continuations[group_index][body_index] = true;
                false
            }
        },
        Step::Withdraw {
            group_index,
            body_index,
        } => match contents.continuations[group_index][body_index] {
            true => {
                withdraw(hot, group_index, body_index);
                contents.continuations[group_index][body_index] = false;
                true
            }
            false => false,
        },
    };
    contents.derive_joins();
    removed
}

fn lock(owner: &SharedOwner) -> MutexGuard<'_, DurableRSpaceAuthorityV1<Profile>> {
    owner
        .lock()
        .expect("the durable owner mutex is not poisoned")
}

fn repository(owner: &SharedOwner) -> Repository {
    let published = lock(owner).published_core();
    Repository::new(published).expect("a repository view at the owner's current state")
}

fn publish(owner: &SharedOwner, gate: &Arc<RwLock<()>>, hot: &Store) -> Blake2b256Hash {
    let state = poll_checkpoint_with(&mut lock(owner), gate, hot, publication_limits())
        .expect("the owner publishes the batch");
    DurableStateRootV1::of_manifest(state.manifest()).into()
}

#[derive(Debug, PartialEq, Eq)]
struct DirectoryImage {
    directory_modified: SystemTime,
    entries: BTreeMap<String, (u64, SystemTime)>,
    store_files: BTreeMap<String, Vec<u8>>,
}

fn directory_image(root: &Path) -> DirectoryImage {
    let entries = fs::read_dir(root)
        .expect("list the store directory")
        .map(|entry| {
            let entry = entry.expect("read a store directory entry");
            let metadata = entry.metadata().expect("read a store entry's metadata");
            (
                entry
                    .file_name()
                    .into_string()
                    .expect("a store entry name is UTF-8"),
                (
                    metadata.len(),
                    metadata
                        .modified()
                        .expect("read a store entry's modification time"),
                ),
            )
        })
        .collect();
    DirectoryImage {
        directory_modified: fs::metadata(root)
            .expect("read the store directory's metadata")
            .modified()
            .expect("read the store directory's modification time"),
        entries,
        store_files: store_files(root),
    }
}

fn seed_scripts() -> [Vec<Vec<Step>>; 3] {
    [
        vec![
            vec![
                Step::put("a", "v0"),
                Step::put("a", "v0"),
                Step::put("b", "v1"),
                Step::install(&["a", "b"], "k0"),
                Step::install(&["b", "a"], "k0"),
                Step::install(&["a"], "k1"),
            ],
            vec![
                Step::take("a", "v0"),
                Step::withdraw(&["b", "a"], "k0"),
                Step::install(&["c", "a", "b"], "k1"),
                Step::put("c", "v1"),
            ],
            vec![
                Step::take("a", "v0"),
                Step::take("b", "v1"),
                Step::take("c", "v1"),
                Step::withdraw(&["a", "b"], "k0"),
                Step::withdraw(&["a"], "k1"),
                Step::withdraw(&["c", "a", "b"], "k1"),
            ],
        ],
        vec![
            vec![
                Step::install(&["a", "b"], "k0"),
                Step::install(&["a", "b"], "k1"),
                Step::put("c", "v0"),
            ],
            vec![Step::withdraw(&["a", "b"], "k0")],
            Vec::new(),
        ],
        vec![Vec::new()],
    ]
}

fn generated_steps() -> impl Strategy<Value = Step> {
    let datum_steps = (0..CHANNELS.len(), 0..VALUES.len());
    let continuation_steps = (0..GROUPS.len(), 0..BODIES.len());
    prop_oneof![
        datum_steps
            .clone()
            .prop_map(|(channel_index, value_index)| Step::Put {
                channel_index,
                value_index,
            }),
        datum_steps.prop_map(|(channel_index, value_index)| Step::Take {
            channel_index,
            value_index,
        }),
        continuation_steps
            .clone()
            .prop_map(|(group_index, body_index)| Step::Install {
                group_index,
                body_index,
            }),
        continuation_steps.prop_map(|(group_index, body_index)| Step::Withdraw {
            group_index,
            body_index,
        }),
    ]
}

fn generated_scripts() -> impl Strategy<Value = Vec<Vec<Step>>> {
    prop::collection::vec(
        prop::collection::vec(generated_steps(), 0..=MAX_BATCH_STEPS),
        1..=MAX_BATCHES,
    )
}

#[derive(Clone, Copy, Debug, Default)]
struct ResetCoverage {
    scripts: u32,
    states: u32,
    states_after_a_removal: u32,
    states_with_repeated_data: u32,
    states_with_both_orders_of_a_pair: u32,
    states_with_two_continuations_on_one_group: u32,
    returns_to_the_genesis_root: u32,
    resets: u32,
    resets_to_retained_predecessors: u32,
    resets_to_other_contents: u32,
    resets_from_superseded_views: u32,
}

impl ResetCoverage {
    fn merge(&mut self, other: Self) {
        let Self {
            scripts,
            states,
            states_after_a_removal,
            states_with_repeated_data,
            states_with_both_orders_of_a_pair,
            states_with_two_continuations_on_one_group,
            returns_to_the_genesis_root,
            resets,
            resets_to_retained_predecessors,
            resets_to_other_contents,
            resets_from_superseded_views,
        } = other;
        self.scripts += scripts;
        self.states += states;
        self.states_after_a_removal += states_after_a_removal;
        self.states_with_repeated_data += states_with_repeated_data;
        self.states_with_both_orders_of_a_pair += states_with_both_orders_of_a_pair;
        self.states_with_two_continuations_on_one_group +=
            states_with_two_continuations_on_one_group;
        self.returns_to_the_genesis_root += returns_to_the_genesis_root;
        self.resets += resets;
        self.resets_to_retained_predecessors += resets_to_retained_predecessors;
        self.resets_to_other_contents += resets_to_other_contents;
        self.resets_from_superseded_views += resets_from_superseded_views;
    }

    fn assert_complete(&self) {
        let Self {
            scripts,
            states,
            states_after_a_removal,
            states_with_repeated_data,
            states_with_both_orders_of_a_pair,
            states_with_two_continuations_on_one_group,
            returns_to_the_genesis_root,
            resets,
            resets_to_retained_predecessors,
            resets_to_other_contents,
            resets_from_superseded_views,
        } = *self;
        assert!(
            [
                scripts,
                states,
                states_after_a_removal,
                states_with_repeated_data,
                states_with_both_orders_of_a_pair,
                states_with_two_continuations_on_one_group,
                returns_to_the_genesis_root,
                resets,
                resets_to_retained_predecessors,
                resets_to_other_contents,
                resets_from_superseded_views,
            ]
            .into_iter()
            .all(|count| count > 0),
            "a class of reset never occurred: {self:?}"
        );
    }
}

fn check_resets(script: &[Vec<Step>]) -> Result<ResetCoverage, TestCaseError> {
    let (_scratch, hot, gate, owner) = fixture();
    let owner = Arc::new(Mutex::new(owner));
    let genesis = repository(&owner);
    let genesis_root = genesis.root();
    let mut coverage = ResetCoverage {
        scripts: 1,
        ..ResetCoverage::default()
    };
    let mut contents = Contents::default();
    let mut states = Vec::with_capacity(script.len() + 1);
    let mut views = Vec::with_capacity(script.len() + 1);
    states.push((genesis_root.clone(), contents.clone()));
    views.push(genesis);
    for batch in script {
        let mut removed = false;
        for &step in batch {
            removed |= apply(&hot, &mut contents, step);
        }
        let root = publish(&owner, &gate, &hot);
        prop_assert_eq!(&root, &Blake2b256Hash::from(lock(&owner).current_root()));
        let view = views
            .last()
            .expect("the genesis view precedes every publication")
            .reset_view(&root)
            .map_err(|error| {
                TestCaseError::fail(format!("a view refused the root just published: {error}"))
            })?;
        coverage.states += 1;
        coverage.states_after_a_removal += u32::from(removed);
        coverage.states_with_repeated_data +=
            u32::from(contents.data.iter().flatten().any(|&count| count >= 2));
        coverage.states_with_both_orders_of_a_pair +=
            u32::from(contents.holds(&["a", "b"]) && contents.holds(&["b", "a"]));
        coverage.states_with_two_continuations_on_one_group += u32::from(
            contents
                .continuations
                .iter()
                .any(|bodies| bodies.iter().all(|&present| present)),
        );
        coverage.returns_to_the_genesis_root += u32::from(
            root == genesis_root && states.iter().any(|(earlier, _)| *earlier != genesis_root),
        );
        states.push((root, contents.clone()));
        views.push(view);
    }
    for (left_root, left_contents) in &states {
        for (right_root, right_contents) in &states {
            prop_assert_eq!(
                left_root == right_root,
                left_contents == right_contents,
                "a durable state root must name exactly one modelled content"
            );
        }
    }
    let current = &states.last().expect("the genesis state is recorded").0;
    for (view, (view_root, view_contents)) in views.iter().zip(&states) {
        prop_assert_eq!(&view.root(), view_root);
        prop_assert_eq!(&snapshot(view.reader()), view_contents);
        for (root, expected) in &states {
            let reset = view.reset_view(root).map_err(|error| {
                TestCaseError::fail(format!(
                    "the view at {view_root} refused the retained root {root}: {error}"
                ))
            })?;
            prop_assert_eq!(&reset.root(), root);
            prop_assert_eq!(
                &snapshot(reset.reader()),
                expected,
                "a reset view must read the contents of the state it was reset to"
            );
            let boxed = view.reset(root).map_err(|error| {
                TestCaseError::fail(format!(
                    "the boxed reset of the view at {view_root} refused {root}: {error}"
                ))
            })?;
            prop_assert_eq!(&boxed.root(), root);
            prop_assert_eq!(view.contains_root(root), Ok(true));
            coverage.resets += 1;
            coverage.resets_to_retained_predecessors += u32::from(root != current);
            coverage.resets_to_other_contents += u32::from(expected != view_contents);
            coverage.resets_from_superseded_views += u32::from(view_root != current);
        }
    }
    Ok(coverage)
}

#[test]
fn every_view_resets_to_every_retained_root_and_reads_that_states_contents() {
    let mut seeded = ResetCoverage::default();
    for script in seed_scripts() {
        seeded.merge(check_resets(&script).expect("a seeded script satisfies HR-01"));
    }
    println!("durable history repository seeds: {seeded:?}");
    seeded.assert_complete();
    let generated = Cell::new(ResetCoverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: GENERATED_SCRIPTS,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&generated_scripts(), |script| {
        let mut total = generated.get();
        total.merge(check_resets(&script)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated scripts satisfy HR-01");
    assert!(generated.get().scripts >= GENERATED_SCRIPTS);
    let mut merged = seeded;
    merged.merge(generated.get());
    println!("durable history repository: {merged:?}");
    merged.assert_complete();
}

#[test]
fn an_unknown_root_is_refused_and_changes_no_file() {
    let (scratch, hot, gate, owner) = fixture();
    let owner = Arc::new(Mutex::new(owner));
    let mut contents = Contents::default();
    let genesis = repository(&owner);
    for step in [Step::put("a", "v0"), Step::install(&["a", "b"], "k0")] {
        apply(&hot, &mut contents, step);
    }
    let retained_root = publish(&owner, &gate, &hot);
    let retained = genesis
        .reset_view(&retained_root)
        .expect("the view resets to the state just published");
    for step in [Step::put("b", "v1"), Step::withdraw(&["a", "b"], "k0")] {
        apply(&hot, &mut contents, step);
    }
    let current_root = publish(&owner, &gate, &hot);
    let current = retained
        .reset_view(&current_root)
        .expect("the view resets to the state just published");
    let roots = [genesis.root(), retained_root.clone(), current_root.clone()];
    let views = [genesis, retained, current];
    let foreign_root = {
        let (_foreign_scratch, foreign_hot, foreign_gate, foreign_owner) = fixture();
        let foreign_owner = Arc::new(Mutex::new(foreign_owner));
        let mut foreign_contents = Contents::default();
        for _ in 0..3 {
            apply(&foreign_hot, &mut foreign_contents, Step::put("c", "v1"));
        }
        publish(&foreign_owner, &foreign_gate, &foreign_hot)
    };
    let published = lock(&owner).published_core();
    let retained_data_bag = published
        .read_handle(Some(
            DurableStateRootV1::try_from(&retained_root)
                .expect("a published durable state root is 32 bytes"),
        ))
        .expect("the retained state resolves")
        .0
        .roots()
        .data_bag
        .digest();
    let candidates = [
        ("a digest absent from the store", Blake2b256Hash::from_bytes(ABSENT_DIGEST.to_vec())),
        ("the LMDB empty radix root", RadixHistory::empty_root_node_hash()),
        ("another store's published state root", foreign_root),
        ("the shared empty trie node", hash_of(CanonicalDagNode::empty().object_ref().digest())),
        ("a retained state's data-bag node", hash_of(retained_data_bag)),
    ];
    let malformed_hashes =
        [Vec::new(), vec![0x5A; 31], vec![0x5A; 33]].map(Blake2b256Hash::from_bytes);
    let files = directory_image(scratch.path());
    let reads = views.each_ref().map(|view| snapshot(view.reader()));

    let mut unknown = Vec::with_capacity(candidates.len() * views.len());
    for (label, hash) in &candidates {
        for view in &views {
            unknown.push((
                *label,
                view.reset_view(hash).err(),
                view.reset(hash).err(),
                view.contains_root(hash),
                view.contains_verified_root(hash),
            ));
        }
    }
    let mut malformed = Vec::with_capacity(malformed_hashes.len() * views.len());
    for hash in &malformed_hashes {
        for view in &views {
            malformed.push((
                hash.0.len(),
                view.reset_view(hash).err(),
                view.reset(hash).err(),
                view.contains_root(hash),
            ));
        }
    }

    assert!(
        unknown
            .iter()
            .all(|(_, reset, boxed, contained, verified)| matches!(
                (reset, boxed, contained, verified),
                (
                    Some(HistoryError::RootError(RootError::UnknownRootError(_))),
                    Some(HistoryError::RootError(RootError::UnknownRootError(_))),
                    Ok(false),
                    Ok(false),
                )
            )),
        "every unknown root must be refused as unknown and reported absent: {unknown:#?}"
    );
    assert!(
        malformed
            .iter()
            .all(|(_, reset, boxed, contained)| matches!(
                (reset, boxed, contained),
                (
                    Some(HistoryError::RootError(RootError::InvalidRootHash(_))),
                    Some(HistoryError::RootError(RootError::InvalidRootHash(_))),
                    Err(HistoryError::RootError(RootError::InvalidRootHash(_))),
                )
            )),
        "a hash of another length must be refused as an invalid root: {malformed:#?}"
    );
    assert_eq!(directory_image(scratch.path()), files, "a refused root changed the store");
    assert_eq!(
        views.each_ref().map(|view| snapshot(view.reader())),
        reads,
        "a refused root changed what a view reads"
    );
    assert_eq!(views.each_ref().map(|view| view.root()), roots, "a refused root moved a view");
    assert_eq!(Blake2b256Hash::from(lock(&owner).current_root()), current_root);
}

fn reset_outcome(source: &Repository, target: &Blake2b256Hash) -> ResetOutcome {
    (
        source
            .reset_view(target)
            .map(|view| (view.root(), snapshot(view.reader()))),
        source.reset(target).map(|view| view.root()),
        source.contains_root(target),
    )
}

fn reset_outcomes(sources: &[&Repository], targets: &[Blake2b256Hash]) -> Vec<ResetOutcome> {
    let mut outcomes = Vec::with_capacity(sources.len() * targets.len());
    for source in sources {
        for target in targets {
            outcomes.push(reset_outcome(source, target));
        }
    }
    outcomes
}

#[test]
fn reset_performs_no_write_and_no_sync_under_every_directory_failpoint() {
    assert!(
        DIRECTORY_FAILPOINTS
            .iter()
            .enumerate()
            .all(|(index, failpoint)| directory_failpoint_index(*failpoint) == index),
        "the failpoint list must hold every directory failpoint once"
    );
    let (scratch, hot, gate, owner) = fixture();
    let owner = Arc::new(Mutex::new(owner));
    let mut contents = Contents::default();
    let genesis = repository(&owner);
    for step in [Step::put("a", "v0"), Step::install(&["b", "a"], "k1")] {
        apply(&hot, &mut contents, step);
    }
    let retained_root = publish(&owner, &gate, &hot);
    for step in [Step::put("c", "v1"), Step::take("a", "v0")] {
        apply(&hot, &mut contents, step);
    }
    let current_root = publish(&owner, &gate, &hot);
    let current = genesis
        .reset_view(&current_root)
        .expect("the view resets to the current state");
    let sources = [&genesis, &current];
    let targets = [
        current_root.clone(),
        retained_root,
        genesis.empty_root(),
        Blake2b256Hash::from_bytes(ABSENT_DIGEST.to_vec()),
    ];
    let files = directory_image(scratch.path());
    let baseline = reset_outcomes(&sources, &targets);
    assert!(
        baseline.iter().any(|(reset, _, _)| reset.is_ok()) &&
            baseline.iter().any(|(reset, _, _)| matches!(
                reset,
                Err(HistoryError::RootError(RootError::UnknownRootError(_)))
            )),
        "the resets must include an accepted and a refused root: {baseline:?}"
    );

    let observations = std::thread::scope(|scope| {
        let mut observations = Vec::with_capacity(DIRECTORY_FAILPOINTS.len());
        for failpoint in DIRECTORY_FAILPOINTS {
            let (sources, targets) = (&sources, &targets);
            observations.push(
                scope
                    .spawn(move || {
                        arm_durable_io_failpoint(failpoint);
                        let outcomes = reset_outcomes(sources, targets);
                        (failpoint, outcomes, armed_durable_io_failpoint())
                    })
                    .join()
                    .expect("the failpoint thread completes"),
            );
        }
        observations
    });

    for (failpoint, outcomes, still_armed) in &observations {
        assert_eq!(outcomes, &baseline, "a reset changed its outcome with {failpoint:?} armed");
        assert_eq!(
            *still_armed,
            Some(*failpoint),
            "a reset reached the {failpoint:?} failpoint and cleared it"
        );
    }
    assert_eq!(directory_image(scratch.path()), files, "a reset changed the store directory");
    assert_eq!(Blake2b256Hash::from(lock(&owner).current_root()), current_root);

    apply(&hot, &mut contents, Step::put("b", "v0"));
    let control = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalWrite);
                let refusal =
                    poll_checkpoint_with(&mut lock(&owner), &gate, &hot, publication_limits())
                        .err()
                        .map(|error| format!("{error:?}"));
                let cleared = armed_durable_io_failpoint().is_none();
                (refusal, cleared)
            })
            .join()
            .expect("the liveness control thread completes")
    });
    assert!(
        matches!(
            &control,
            (Some(refusal), true) if refusal.contains("injected durable I/O failure at PhysicalWrite")
        ),
        "an owner publication in a failpoint thread must trip and clear PhysicalWrite: {control:?}"
    );
}

#[test]
fn empty_root_is_the_store_genesis_state_and_stays_resettable() {
    let (_scratch, hot, gate, owner) = fixture();
    let owner = Arc::new(Mutex::new(owner));
    let mut contents = Contents::default();
    let genesis = repository(&owner);
    let legacy = HistoryRepositoryInstances::<String, String, String, String>::repository(
        RSpaceStateAuthority::in_memory(),
    )
    .expect("an LMDB-mode repository over an in-memory authority");
    for step in [Step::put("a", "v0"), Step::install(&["a", "b"], "k0")] {
        apply(&hot, &mut contents, step);
    }
    let published = publish(&owner, &gate, &hot);
    let current = genesis
        .reset_view(&published)
        .expect("the view resets to the published state");
    let empty = current
        .reset_view(&current.empty_root())
        .expect("the store-genesis state stays resettable after a publication");

    assert_eq!(genesis.root(), genesis.empty_root(), "the fixture starts at the store genesis");
    assert_eq!(current.empty_root(), genesis.empty_root(), "views of one store share a genesis");
    assert_ne!(published, genesis.empty_root(), "the publication leaves the store genesis");
    assert_ne!(
        genesis.empty_root(),
        RadixHistory::empty_root_node_hash(),
        "the durable store genesis is not the LMDB radix empty root"
    );
    assert_eq!(
        legacy.empty_root(),
        RadixHistory::empty_root_node_hash(),
        "the LMDB-mode repository keeps the radix empty root"
    );
    assert_eq!(
        (empty.root(), snapshot(empty.reader())),
        (genesis.empty_root(), Contents::default())
    );
    assert_eq!(snapshot(current.reader()), contents);
}

#[test]
fn legacy_operations_refuse_with_their_typed_backend_mismatch() {
    let (scratch, _hot, _gate, owner) = fixture();
    let owner = Arc::new(Mutex::new(owner));
    let view = repository(&owner);
    let root = view.root();
    let files = directory_image(scratch.path());
    let history = HistoryError::BackendMismatch(AuthorityBackendMismatch::History);
    let roots = HistoryError::BackendMismatch(AuthorityBackendMismatch::Roots);
    let exporter = view.exporter();
    let importer = view.importer();
    let spec = StateImportSpec::new([root.clone()], StateImportPublicationMode::PreserveCurrent)
        .expect("a one-root import specification");

    assert_eq!(
        [
            view.checkpoint(Vec::new()).err(),
            view.checkpoint_at(&CurrentRoot::new(root.clone(), 0), Vec::new())
                .err(),
            view.current_authority_root().err(),
            view.do_checkpoint(Vec::new()).err(),
            view.get_history_reader(&root).err(),
            view.get_history_reader_struct(&root).err(),
        ],
        [
            Some(history.clone()),
            Some(roots.clone()),
            Some(roots),
            Some(history.clone()),
            Some(history.clone()),
            Some(history),
        ]
    );
    assert_eq!(
        (
            exporter.get_page(Vec::new(), 0, 1).err(),
            exporter.get_history_items(Vec::new()).err(),
            exporter.get_data_items(Vec::new()).err(),
        ),
        (
            Some(StateExportError::BackendMismatch(AuthorityBackendMismatch::History)),
            Some(StateExportError::BackendMismatch(AuthorityBackendMismatch::History)),
            Some(StateExportError::BackendMismatch(AuthorityBackendMismatch::Cold)),
        )
    );
    assert_eq!(
        (importer.begin_import(spec).err(), importer.get_history_item(root.clone()).err()),
        (
            Some(StateImportError::BackendMismatch(AuthorityBackendMismatch::History)),
            Some(StateImportError::BackendMismatch(AuthorityBackendMismatch::History)),
        )
    );
    assert_eq!(view.root(), root);
    assert_eq!(directory_image(scratch.path()), files, "a refused operation changed the store");
}

#[test]
fn the_exporter_root_follows_the_owners_current_state() {
    let (_scratch, hot, gate, owner) = fixture();
    let owner = Arc::new(Mutex::new(owner));
    let mut contents = Contents::default();
    let genesis = repository(&owner);
    let exporter = genesis.exporter();
    let before = exporter
        .get_root()
        .expect("the exporter reads the owner's current root");
    apply(&hot, &mut contents, Step::put("a", "v1"));
    let published = publish(&owner, &gate, &hot);
    let after = exporter
        .get_root()
        .expect("the exporter reads the owner's current root");

    assert_eq!(
        (before, after, genesis.root()),
        (genesis.empty_root(), published, genesis.empty_root()),
        "the exporter's root follows the owner while a view keeps the state it was made at"
    );
}

#[test]
fn a_reset_completes_while_a_publication_holds_the_owner_mutex() {
    let (_scratch, hot, gate, owner) = fixture();
    let owner = Arc::new(Mutex::new(owner));
    let mut contents = Contents::default();
    let genesis_root = repository(&owner).root();
    apply(&hot, &mut contents, Step::put("a", "v0"));
    let predecessor_root = publish(&owner, &gate, &hot);
    let predecessor_contents = contents.clone();
    apply(&hot, &mut contents, Step::install(&["a", "b"], "k0"));
    let published = lock(&owner).published_core();
    let (prepared_signal, prepared) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();

    let (during, view, successor_root) = std::thread::scope(|scope| {
        let (owner, gate, hot) = (&owner, &gate, &hot);
        let publication = scope.spawn(move || {
            let mut held = lock(owner);
            let _hook =
                journal_owner_hooks::arm(JournalOwnerHookPoint::AfterPreparation, move || {
                    prepared_signal
                        .send(())
                        .expect("the test thread awaits the prepared publication");
                    if let Err(RecvTimeoutError::Timeout) = released.recv_timeout(RELEASE_TIMEOUT) {
                        panic!("the test thread did not release the publication within 30 s");
                    }
                });
            poll_checkpoint_with(&mut held, gate, hot, publication_limits())
                .expect("the held publication completes once released")
        });
        prepared
            .recv_timeout(RELEASE_TIMEOUT)
            .expect("the publication reaches its preparation while it holds the owner mutex");
        let held_before_reads = matches!(owner.try_lock(), Err(TryLockError::WouldBlock));
        let view = Repository::new(published.clone())
            .expect("a repository view built from the published core");
        let current = view
            .reset_view(&predecessor_root)
            .expect("a reset to the current root");
        let retained = view
            .reset_view(&genesis_root)
            .expect("a reset to a retained predecessor");
        let during = (
            held_before_reads,
            view.root(),
            (current.root(), snapshot(current.reader())),
            (retained.root(), snapshot(retained.reader())),
            (view.contains_root(&predecessor_root), view.contains_root(&genesis_root)),
            view.exporter().get_root(),
            matches!(owner.try_lock(), Err(TryLockError::WouldBlock)),
        );
        drop(release);
        let successor = publication
            .join()
            .unwrap_or_else(|payload| resume_unwind(payload));
        (during, view, Blake2b256Hash::from(DurableStateRootV1::of_manifest(successor.manifest())))
    });

    assert_eq!(
        during,
        (
            true,
            predecessor_root.clone(),
            (predecessor_root.clone(), predecessor_contents),
            (genesis_root, Contents::default()),
            (Ok(true), Ok(true)),
            Ok(predecessor_root.clone()),
            true,
        ),
        "while a publication holds the owner mutex, a view built from the published core must \
         reset to the current root and to a retained predecessor and report the predecessor's \
         roots"
    );
    assert_ne!(successor_root, predecessor_root, "the held publication moves the current state");
    let successor = view
        .reset_view(&successor_root)
        .expect("a reset to the successor's root after the release");
    assert_eq!(
        (view.exporter().get_root(), successor.root(), snapshot(successor.reader())),
        (Ok(successor_root.clone()), successor_root.clone(), contents),
        "after the release the exporter must answer the successor's root and a reset to it must \
         read the successor's contents"
    );
    assert_eq!(Blake2b256Hash::from(lock(&owner).current_root()), successor_root);
}

fn installed_test_store(prefix: &str) -> PersistenceTrialDir {
    let directory = trial_scratch(prefix);
    install_checkpoint_image(
        directory.path(),
        &checkpoint_image_from_source(checkpoint_source_fixture()),
    );
    directory
}

fn admitted_test_writer(root: &Path) -> SemanticallyAdmittedDurableWriterV1<TestSemanticProfile> {
    DurableDirectoryV1::open(root)
        .expect("open the installed durable directory")
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("cold admission of the installed checkpoint")
}

fn generation_checkpoint(
    writer: SemanticallyAdmittedDurableWriterV1<TestSemanticProfile>,
) -> SemanticallyAdmittedDurableWriterV1<TestSemanticProfile> {
    writer
        .publish_generation_checkpoint_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("a generation checkpoint reclaims the unleased first generation")
}

fn generation_files(key: GenerationLeaseKeyV1) -> Vec<String> {
    (key.first_live_pack()..=key.active_pack())
        .map(|pack| pack_file_name(PackId::new(pack).expect("a catalog pack id")))
        .chain([index_file_name(
            IndexGenerationV1::new(key.index_generation()).expect("a catalog index generation"),
        )])
        .collect()
}

fn count_published_core_loads(loads: Rc<Cell<u32>>) {
    loads.set(loads.get() + 1);
    match loads.get() {
        3 => panic!("the published core was loaded a third time"),
        _ => journal_owner_hooks::rearm(JournalOwnerHookPoint::AfterPublishedCoreLoad, move || {
            count_published_core_loads(loads)
        }),
    }
}

fn arm_published_core_loads(
    first_load: impl FnOnce() + 'static,
) -> (Rc<Cell<u32>>, JournalOwnerHookGuard) {
    let loads = Rc::new(Cell::new(0));
    let counted = Rc::clone(&loads);
    let guard =
        journal_owner_hooks::arm(JournalOwnerHookPoint::AfterPublishedCoreLoad, move || {
            first_load();
            count_published_core_loads(counted);
        });
    (loads, guard)
}

#[test]
fn a_read_retries_on_the_published_successor_when_its_core_is_reclaimed_before_its_lease() {
    let scratch = installed_test_store("published-core-retry-");
    let writer = admitted_test_writer(scratch.path());
    let first_key = GenerationLeaseKeyV1::from_catalog(writer.catalog());
    let first_root = DurableStateRootV1::of_manifest(writer.catalog().state().manifest());
    let registry = writer.generation_leases();
    let published =
        DurablePublishedCoreV1::detached_for_test(writer.shared_core(), registry.clone());
    let successor = Rc::new(Cell::new(None));
    let (installer, recorded) = (published.clone(), Rc::clone(&successor));
    let (loads, hook) = arm_published_core_loads(move || {
        let advanced =
            publish_data_successor(generation_checkpoint(writer), "published-core-successor");
        recorded.set(Some((
            GenerationLeaseKeyV1::from_catalog(advanced.catalog()),
            DurableStateRootV1::of_manifest(advanced.catalog().state().manifest()),
        )));
        installer.install_for_test(advanced.shared_core());
    });

    let (resolved, handle) = published
        .read_handle(None)
        .expect("the read retries on the published successor");
    let observed_loads = loads.get();
    drop(hook);
    let (successor_key, successor_root) = successor
        .get()
        .expect("the first load installs a successor");

    assert_ne!(successor_root, first_root, "the successor's current state differs");
    assert!(
        successor_key.first_live_pack() > first_key.active_pack(),
        "the successor starts a fresh pack range: {first_key:?} {successor_key:?}"
    );
    assert_eq!(
        resolved.root(),
        successor_root,
        "the read must return the published successor's current state"
    );
    assert_eq!(observed_loads, 2, "the read loads the first core and then its successor");
    assert_eq!(
        (registry.active_lease_count_for_test(), registry.has_active(successor_key)),
        (1, Ok(true)),
        "the registry must hold exactly the read's lease, on the successor's key"
    );
    assert_eq!(
        generation_files(first_key)
            .into_iter()
            .filter(|name| scratch.path().join(name).exists())
            .collect::<Vec<_>>(),
        Vec::<String>::new(),
        "the first core's generation is reclaimed"
    );
    assert!(
        generation_files(successor_key)
            .iter()
            .all(|name| scratch.path().join(name).exists()),
        "the successor's generation is present"
    );
    assert_eq!(
        handle
            .semantic_roots()
            .expect("read the successor's roots through the handle"),
        resolved.roots()
    );
    drop(handle);
    let (superseded, _) = published
        .read_handle(Some(first_root))
        .expect("the successor retains the first core's state");
    assert_eq!(
        (superseded.root(), registry.active_lease_count_for_test()),
        (first_root, 0),
        "the first core's state stays retained and a dropped handle releases its lease"
    );
}

#[test]
fn a_read_refused_on_a_reclaimed_core_without_a_successor_reloads_once() {
    let scratch = installed_test_store("published-core-refusal-");
    let writer = admitted_test_writer(scratch.path());
    let first_key = GenerationLeaseKeyV1::from_catalog(writer.catalog());
    let first_root = DurableStateRootV1::of_manifest(writer.catalog().state().manifest());
    let registry = writer.generation_leases();
    let published =
        DurablePublishedCoreV1::detached_for_test(writer.shared_core(), registry.clone());
    let checkpointed = generation_checkpoint(writer);
    let (loads, _hook) = arm_published_core_loads(|| {});

    let refusal = published.read_handle(None).err();

    assert_ne!(GenerationLeaseKeyV1::from_catalog(checkpointed.catalog()), first_key);
    assert_eq!(
        (refusal, loads.get(), registry.active_lease_count_for_test()),
        (
            Some(DurableStateReadHandleErrorV1::GenerationLease(GenerationLeaseError::Retired(
                first_key
            ))),
            1,
            0,
        ),
        "a read on a retired core without a successor must return the refusal after one load and \
         keep no lease"
    );
    assert_eq!(
        published.current_root(),
        first_root,
        "the current root reads only the published catalog slot"
    );
}
