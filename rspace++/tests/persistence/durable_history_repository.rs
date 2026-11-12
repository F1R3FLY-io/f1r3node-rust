use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic::catch_unwind;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use proptest::prelude::*;
use tokio::sync::RwLock;

use super::durable_rspace_authority::{
    Profile, Store, fixture, poll_checkpoint_with, publication_limits, store_files,
};
use crate::rspace::errors::{AuthorityBackendMismatch, HistoryError, RootError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::CurrentRoot;
use crate::rspace::history::durable_history_repository::DurableHistoryRepositoryV1;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::history::history_repository::{HistoryRepository, HistoryRepositoryInstances};
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::durable::{
    DurableIoFailpointV1, DurableRSpaceAuthorityV1, DurableStateRootV1, arm_durable_io_failpoint,
};
use crate::rspace::persistence::{CanonicalDagNode, ObjectDigest};
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
const ARMED_FAILPOINT_REFUSAL: &str = "durable I/O failpoint already armed";
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

fn panic_text(payload: &(dyn std::any::Any + Send)) -> &str {
    match (payload.downcast_ref::<&'static str>(), payload.downcast_ref::<String>()) {
        (Some(text), _) => text,
        (None, Some(text)) => text.as_str(),
        (None, None) => "a panic without a text payload",
    }
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
    Repository::new(Arc::clone(owner)).expect("a repository view at the owner's current state")
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
    let retained_data_bag = lock(&owner)
        .resolve(
            DurableStateRootV1::try_from(&retained_root)
                .expect("a published durable state root is 32 bytes"),
        )
        .expect("the retained state resolves")
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
                        let rearm = catch_unwind(move || arm_durable_io_failpoint(failpoint));
                        let refusal = rearm
                            .err()
                            .map(|payload| panic_text(payload.as_ref()).to_owned());
                        (failpoint, outcomes, refusal)
                    })
                    .join()
                    .expect("the failpoint thread completes"),
            );
        }
        observations
    });

    for (failpoint, outcomes, refusal) in &observations {
        assert_eq!(outcomes, &baseline, "a reset changed its outcome with {failpoint:?} armed");
        assert_eq!(
            refusal.as_deref(),
            Some(ARMED_FAILPOINT_REFUSAL),
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
                let cleared =
                    catch_unwind(|| arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalWrite))
                        .is_ok();
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
