use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use proptest::prelude::*;
use tokio::sync::RwLock;

use super::durable_publication::FORMAL_PUBLICATION_PHASE_ORDER;
use super::durable_recovery::limits;
use super::durable_rspace_authority::{Profile, Store, poll_checkpoint_with, publication_limits};
use super::owner_cross_base_checkpoint::{
    Step, Trial, apply_all, generated_steps, panic_text, view_limits,
};
use super::support::{PersistenceTrialDir, trial_scratch};
use crate::rspace::hashing::stable_hash_provider::{StableHashSerialize, hash, hash_from_slice};
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::hot_store::{DurableHotStoreBootstrapLimits, HotStoreInstances};
use crate::rspace::persistence::LogPackDecodeLimits;
use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableIoError, DurablePublishedCoreV1, DurableRSpaceAuthorityV1,
    DurableRSpaceTransactionError, DurableStateRootV1, DurableViewReaderV1,
    GenerationCheckpointPublicationHookPointV1, PREIMAGE_FILE_NAME_V1, PreimageIndexError,
    PreimageLookupErrorV1, PreimageWriteCountsV1, arm_generation_checkpoint_publication_hook,
    recover_read_authority,
};

type Owner = DurableRSpaceAuthorityV1<Profile>;
type PublishedCore = DurablePublishedCoreV1<Profile>;
type StoreFiles = BTreeMap<String, Vec<u8>>;

const CHANNEL_NAMES: [&str; 3] = ["a", "b", "c"];
const ORDERED_GROUPS: [&[&str]; 4] = [&["a"], &["a", "b"], &["b", "a"], &["c", "a", "b"]];
const GENERATED_SCENARIOS: u32 = 16;
const MAX_BATCHES: usize = 3;
const MAX_CHECKPOINTS: usize = 3;
const MAX_STEPS: usize = 6;
const BASE_SELECTORS: usize = 8;
const OWNER_CRASH_POINTS: usize = 2;
const PUBLICATION_CRASH_POINTS: usize = 13;
const SUCCESSOR_CRASH_POINTS: usize = 4;
const INTERRUPTION: &str = "the owner task stops at a test hook point";

fn group_of(names: &[&str]) -> Vec<String> { names.iter().map(|name| (*name).to_owned()).collect() }

fn channel_key(name: &str) -> [u8; 32] {
    hash(&name.to_owned())
        .0
        .try_into()
        .expect("a channel hash has 32 bytes")
}

fn bucket_key(group: &[String]) -> [u8; 32] {
    hash_from_slice(group)
        .0
        .try_into()
        .expect("a bucket key has 32 bytes")
}

fn blake2b256(bytes: &[u8]) -> [u8; 32] { Blake2b::<U32>::digest(bytes).into() }

#[derive(Debug, Default)]
struct PresentPreimages {
    channels: BTreeSet<String>,
    groups: BTreeSet<Vec<String>>,
}

fn present_preimages(core: &PublishedCore, root: DurableStateRootV1) -> PresentPreimages {
    let (resolved, handle) = core
        .read_handle(Some(root))
        .expect("a retained state takes a read handle");
    let reader = DurableViewReaderV1::new(handle, resolved.roots());
    let mut present = PresentPreimages::default();
    for names in ORDERED_GROUPS {
        let group = group_of(names);
        if !reader.get_continuations_canonical(&group).is_empty() {
            present.channels.extend(group.iter().cloned());
            present.groups.insert(group);
        }
    }
    for name in CHANNEL_NAMES {
        let channel = name.to_owned();
        let joins = reader.get_joins_canonical(&channel);
        assert!(
            joins.iter().all(|joined| ORDERED_GROUPS
                .iter()
                .any(|names| group_of(names) == *joined)),
            "every group that a join of the state names must belong to the test's universe: \
             {joins:?}"
        );
        if !joins.is_empty() || !reader.get_data_canonical(&channel).is_empty() {
            present.channels.insert(channel);
        }
    }
    present
}

fn assert_covered(core: &PublishedCore, present: &PresentPreimages, state: &str, context: &str) {
    for channel in &present.channels {
        let key = channel_key(channel);
        let Some(identity) = core
            .channel_preimage(&key)
            .expect("a lookup through the published core of a live owner is answered")
        else {
            panic!("a channel of {state} has no preimage entry: {channel:?} ({context})");
        };
        assert_eq!(
            (blake2b256(&identity), identity),
            (key, channel.stable_hash_bytes()),
            "the preimage of a channel of {state} must be its identity bytes and hash to its key: \
             {channel:?} ({context})"
        );
    }
    for group in &present.groups {
        let bucket = bucket_key(group);
        let listed = core
            .ordered_group_preimages(&bucket)
            .expect("a lookup through the published core of a live owner is answered");
        let expected: Vec<[u8; 32]> = group
            .iter()
            .map(|channel| channel_key(channel.as_str()))
            .collect();
        assert!(
            listed.contains(&expected),
            "an ordered group of {state} has no preimage entry: {group:?} ({context})"
        );
        for members in &listed {
            let mut sorted = members.clone();
            sorted.sort_unstable();
            assert_eq!(
                blake2b256(&sorted.concat()),
                bucket,
                "every ordered group listed under the bucket key of {group:?} must hash to it \
                 ({context})"
            );
        }
    }
}

#[derive(Clone, Debug)]
struct Scenario {
    batches: Vec<Vec<Step>>,
    checkpoints: Vec<(usize, Vec<Step>)>,
}

fn seed_scenarios() -> [Scenario; 2] {
    [
        Scenario {
            batches: vec![
                vec![
                    Step::put("a", "v0"),
                    Step::install(&["a", "b"], "k0"),
                    Step::install(&["b", "a"], "k1"),
                ],
                vec![
                    Step::put("b", "v1"),
                    Step::install(&["c", "a", "b"], "k0"),
                    Step::withdraw(&["a", "b"], "k0"),
                ],
            ],
            checkpoints: vec![
                (0, vec![Step::put("c", "v0"), Step::install(&["a"], "k1")]),
                (2, vec![Step::put("a", "v1"), Step::install(&["a", "b"], "k1")]),
                (1, vec![Step::take("a", "v0"), Step::install(&["c", "a", "b"], "k1")]),
            ],
        },
        Scenario {
            batches: vec![vec![Step::put("a", "v0")]],
            checkpoints: vec![
                (0, vec![Step::install(&["b", "a"], "k0"), Step::install(&["a", "b"], "k1")]),
                (1, vec![Step::put("c", "v1"), Step::install(&["c", "a", "b"], "k1")]),
                (3, vec![Step::put("b", "v0")]),
            ],
        },
    ]
}

fn generated_scenarios() -> impl Strategy<Value = Scenario> {
    let journals = || prop::collection::vec(generated_steps(), 0..=MAX_STEPS);
    (
        prop::collection::vec(journals(), 1..=MAX_BATCHES),
        prop::collection::vec((0..BASE_SELECTORS, journals()), 1..=MAX_CHECKPOINTS),
    )
        .prop_map(|(batches, checkpoints)| Scenario {
            batches,
            checkpoints,
        })
}

#[derive(Clone, Copy, Debug, Default)]
struct Coverage {
    scenarios: u32,
    bound_publications: u32,
    same_base_view_publications: u32,
    cross_base_view_publications: u32,
    retained_states_checked: u32,
    states_holding_both_orders: u32,
    states_holding_three_channel_groups: u32,
}

impl Coverage {
    fn merge(&mut self, other: Self) {
        let Self {
            scenarios,
            bound_publications,
            same_base_view_publications,
            cross_base_view_publications,
            retained_states_checked,
            states_holding_both_orders,
            states_holding_three_channel_groups,
        } = other;
        self.scenarios += scenarios;
        self.bound_publications += bound_publications;
        self.same_base_view_publications += same_base_view_publications;
        self.cross_base_view_publications += cross_base_view_publications;
        self.retained_states_checked += retained_states_checked;
        self.states_holding_both_orders += states_holding_both_orders;
        self.states_holding_three_channel_groups += states_holding_three_channel_groups;
    }

    fn assert_complete(&self) {
        let Self {
            scenarios,
            bound_publications,
            same_base_view_publications,
            cross_base_view_publications,
            retained_states_checked,
            states_holding_both_orders,
            states_holding_three_channel_groups,
        } = *self;
        assert!(
            [
                scenarios,
                bound_publications,
                same_base_view_publications,
                cross_base_view_publications,
                retained_states_checked,
                states_holding_both_orders,
                states_holding_three_channel_groups,
            ]
            .into_iter()
            .all(|count| count > 0),
            "PI-06 must check retained states after bound, same-base and cross-base publications, \
             among them states that hold a group in both orders and a group of three channels: \
             {self:?}"
        );
    }
}

fn check_retained(trial: &Trial, coverage: &mut Coverage) {
    let core = trial.owner.published_core();
    let both_orders = [group_of(&["a", "b"]), group_of(&["b", "a"])];
    for (position, (root, _)) in trial.states.iter().enumerate() {
        let present = present_preimages(&core, *root);
        assert_covered(&core, &present, "a retained state", &format!("retained state {position}"));
        coverage.retained_states_checked += 1;
        coverage.states_holding_both_orders += u32::from(
            both_orders
                .iter()
                .all(|group| present.groups.contains(group)),
        );
        coverage.states_holding_three_channel_groups +=
            u32::from(present.groups.iter().any(|group| group.len() == 3));
    }
}

fn check_scenario(scenario: &Scenario) -> Result<Coverage, TestCaseError> {
    let mut trial = Trial::open();
    let mut coverage = Coverage {
        scenarios: 1,
        ..Coverage::default()
    };
    for batch in &scenario.batches {
        let before = trial.owner.current_root();
        coverage.bound_publications += u32::from(trial.publish(batch) != before);
        check_retained(&trial, &mut coverage);
    }
    for (selector, journal) in &scenario.checkpoints {
        let (base, contents) = trial.states[selector % trial.states.len()].clone();
        let same_base = base == trial.owner.current_root();
        let mut view = trial.view(base, contents);
        apply_all(&view.hot, &mut view.contents, journal);
        let reached = trial
            .checkpoint_with(view.base, &view.hot, view_limits())
            .map_err(|refusal| {
                TestCaseError::fail(format!(
                    "the owner refused a view checkpoint from a retained base: {refusal:?}"
                ))
            })?
            .root();
        if trial.states.iter().all(|(known, _)| *known != reached) {
            match same_base {
                true => coverage.same_base_view_publications += 1,
                false => coverage.cross_base_view_publications += 1,
            }
            trial.states.push((reached, view.contents));
        }
        check_retained(&trial, &mut coverage);
    }
    Ok(coverage)
}

#[test]
fn every_channel_and_group_of_a_retained_state_resolves_through_the_index() {
    let mut seeded = Coverage::default();
    for scenario in seed_scenarios() {
        seeded.merge(check_scenario(&scenario).expect("a seeded scenario satisfies PI-06"));
    }
    seeded.assert_complete();
    let generated = Cell::new(Coverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: GENERATED_SCENARIOS,
        failure_persistence: None,
        ..ProptestConfig::default()
    })
    .run(&generated_scenarios(), |scenario| {
        let mut total = generated.get();
        total.merge(check_scenario(&scenario)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated scenarios satisfy PI-06");
    assert!(
        generated.get().scenarios >= GENERATED_SCENARIOS,
        "every generated scenario must run to its end"
    );
    let mut merged = seeded;
    merged.merge(generated.get());
    println!("preimage publication PI-06 coverage: {merged:?}");
}

#[derive(Clone, Copy, Debug)]
enum CrashPoint {
    Owner(JournalOwnerHookPoint),
    Publication(GenerationCheckpointPublicationHookPointV1),
}

impl CrashPoint {
    const fn selects_the_successor(self) -> bool {
        matches!(
            self,
            Self::Publication(
                GenerationCheckpointPublicationHookPointV1::AfterCatalogSlotWrite |
                    GenerationCheckpointPublicationHookPointV1::AfterCatalogFileSyncBeforeDirectorySync |
                    GenerationCheckpointPublicationHookPointV1::AfterCatalogDirectorySync |
                    GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery
            )
        )
    }
}

fn crash_points() -> Vec<CrashPoint> {
    let publication = &FORMAL_PUBLICATION_PHASE_ORDER[2..=14];
    let mut points = Vec::with_capacity(OWNER_CRASH_POINTS + publication.len());
    points.push(CrashPoint::Owner(JournalOwnerHookPoint::AfterCapture));
    points.push(CrashPoint::Owner(JournalOwnerHookPoint::AfterPreparation));
    points.extend(publication.iter().copied().map(CrashPoint::Publication));
    points
}

fn every_store_file(root: &Path) -> StoreFiles {
    fs::read_dir(root)
        .expect("list the store directory")
        .map(|entry| entry.expect("read a store directory entry"))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| {
            let name = entry
                .file_name()
                .into_string()
                .expect("a store file name is UTF-8");
            let bytes = fs::read(entry.path()).expect("read a store file");
            (name, bytes)
        })
        .collect()
}

fn copy_of(files: &StoreFiles) -> PersistenceTrialDir {
    let scratch = trial_scratch("preimage-publication-copy-");
    for (name, bytes) in files {
        let path = scratch.path().join(name);
        fs::write(&path, bytes).expect("write a copied store file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .expect("restrict a copied store file to its owner");
    }
    scratch
}

fn owner_over(root: &Path) -> (Store, Owner) {
    let view = recover_read_authority::<Profile>(
        root,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("the store recovers");
    let hot: Store = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .expect("the recovered state loads into a hot store"),
    );
    let owner = view
        .into_rspace_authority(&hot)
        .unwrap_or_else(|failure| panic!("an owner opens over the store: {:?}", failure.1));
    (hot, owner)
}

#[test]
fn a_crash_at_each_owner_hook_leaves_the_index_covering_the_selected_state() {
    let points = crash_points();
    let journal = [Step::put("c", "v0"), Step::install(&["c", "a", "b"], "k1")];
    let successor_group = group_of(&["c", "a", "b"]);
    let mut successors_selected = 0;
    for point in points.iter().copied() {
        let mut trial = Trial::open();
        trial.publish(&[Step::put("a", "v0"), Step::install(&["a", "b"], "k0")]);
        let predecessor = trial.publish(&[Step::put("b", "v1")]);
        let (base, contents) = trial.states[1].clone();
        let mut view = trial.view(base, contents);
        apply_all(&view.hot, &mut view.contents, &journal);
        let copied: Rc<RefCell<Option<StoreFiles>>> = Rc::new(RefCell::new(None));
        let copy_and_stop = {
            let copied = Rc::clone(&copied);
            let directory = trial.path().to_path_buf();
            move || {
                copied.replace(Some(every_store_file(&directory)));
                panic!("{INTERRUPTION}");
            }
        };
        let hook = match point {
            CrashPoint::Owner(owner_point) => {
                Some(journal_owner_hooks::arm(owner_point, copy_and_stop))
            }
            CrashPoint::Publication(publication_point) => {
                arm_generation_checkpoint_publication_hook(publication_point, copy_and_stop);
                None
            }
        };
        let interruption = match catch_unwind(AssertUnwindSafe(|| {
            trial.checkpoint_with(view.base, &view.hot, view_limits())
        })) {
            Ok(completed) => format!("completed with {completed:?}"),
            Err(payload) => panic_text(payload.as_ref()).to_owned(),
        };
        drop(hook);
        assert_eq!(interruption, INTERRUPTION, "the checkpoint must stop at {point:?}");
        let files = copied
            .borrow_mut()
            .take()
            .expect("the stop copied the store files");
        assert!(
            files.contains_key(PREIMAGE_FILE_NAME_V1),
            "the files copied at {point:?} must hold the preimage index"
        );
        let copy = copy_of(&files);
        let (_recovered_hot, recovered) = owner_over(copy.path());
        let selected = recovered.current_root();
        let core = recovered.published_core();
        let present = present_preimages(&core, selected);
        let successor = point.selects_the_successor();
        assert_eq!(
            (
                selected == predecessor,
                present.channels.contains("c"),
                present.groups.contains(&successor_group),
            ),
            (!successor, successor, successor),
            "recovery of the files copied at {point:?} must select the predecessor before the \
             catalog slot write and, after it, the successor, which adds the channel c and the \
             group (c,a,b)"
        );
        assert_covered(&core, &present, "the recovered state", &format!("a stop at {point:?}"));
        successors_selected += usize::from(successor);
    }
    assert_eq!(
        (points.len(), successors_selected),
        (OWNER_CRASH_POINTS + PUBLICATION_CRASH_POINTS, SUCCESSOR_CRASH_POINTS),
        "PI-07 must stop at the two owner hook points and the thirteen test points of a trusted \
         publication, four of which follow the catalog slot write"
    );
}

#[test]
fn a_publication_of_known_channels_appends_nothing_and_adds_no_sync() {
    let counted = |appended_batches, file_syncs| PreimageWriteCountsV1 {
        appended_batches,
        file_syncs,
    };
    let counts = |core: &PublishedCore| {
        core.preimage_write_counts_for_test()
            .expect("a live owner holds its preimage index")
    };
    let mut trial = Trial::open();
    let core = trial.owner.published_core();
    assert_eq!(
        counts(&core),
        counted(0, 0),
        "an owner opened over a store without preimage.v1 has appended nothing, and its sync at \
         open finds no file to sync"
    );
    trial.publish(&[Step::put("a", "v0"), Step::install(&["a", "b"], "k0")]);
    assert_eq!(
        counts(&core),
        counted(1, 1),
        "a publication of new channels and a new ordered group appends one batch and syncs once"
    );
    trial.publish(&[Step::put("b", "v1"), Step::install(&["a", "b"], "k1"), Step::take("a", "v0")]);
    assert_eq!(
        counts(&core),
        counted(1, 1),
        "a publication whose channels and ordered groups the index holds must append nothing and \
         add no sync"
    );
    trial.publish(&[Step::install(&["b", "a"], "k0")]);
    assert_eq!(
        counts(&core),
        counted(2, 2),
        "a new order of known channels is a new ordered group, appended in one batch and synced \
         once"
    );
    assert_eq!(
        core.ordered_group_preimages(&bucket_key(&group_of(&["a", "b"])))
            .expect("a lookup through the published core of a live owner is answered"),
        vec![vec![channel_key("a"), channel_key("b")], vec![channel_key("b"), channel_key("a")]],
        "(a,b) and (b,a) share one bucket key and are listed in the order of their publication"
    );

    let preimage_file = trial.path().join(PREIMAGE_FILE_NAME_V1);
    let (current, contents) = trial
        .states
        .last()
        .cloned()
        .expect("the trial records its current state");
    let mut view = trial.view(current, contents);
    apply_all(&view.hot, &mut view.contents, &[Step::put("c", "v0")]);
    fs::set_permissions(&preimage_file, fs::Permissions::from_mode(0o640))
        .expect("grant the group read access to preimage.v1");
    let refused = trial.checkpoint_with(view.base, &view.hot, view_limits());
    assert!(
        matches!(
            refused.as_ref().map_err(|refusal| &**refusal),
            Err(DurableRSpaceTransactionError::Preimage(PreimageIndexError::Io(
                DurableIoError::UnsafePermissions { .. }
            )))
        ),
        "a sync of preimage.v1 that the store refuses must refuse the publication: {refused:?}"
    );
    assert_eq!(
        (
            counts(&core),
            trial.owner.current_root(),
            core.channel_preimage(&channel_key("a"))
                .expect("a lookup through the published core of a live owner is answered")
                .is_some(),
        ),
        (counted(3, 2), current, true),
        "the refused publication appended the new channel and executed no sync, the current state \
         is unchanged, and readers keep the index until the next publication recovers it"
    );
    fs::set_permissions(&preimage_file, fs::Permissions::from_mode(0o600))
        .expect("restrict preimage.v1 to its owner again");
    let retried = trial
        .checkpoint_with(view.base, &view.hot, view_limits())
        .expect("the retry publishes once preimage.v1 syncs");
    assert_eq!(
        (counts(&core), trial.owner.current_root()),
        (counted(0, 1), retried.root()),
        "the retry must reopen the index under the held lease and pay the owed sync before it \
         appends, and must append nothing for the channel that the refused publication appended"
    );
    assert_covered(
        &core,
        &present_preimages(&core, retried.root()),
        "the published state",
        "the retry after a refused sync",
    );

    trial
        .owner
        .close()
        .expect("an owner without a pending publication closes");
    assert!(
        matches!(core.channel_preimage(&channel_key("a")), Err(PreimageLookupErrorV1::Unavailable)) &&
            core.preimage_write_counts_for_test().is_none(),
        "closing the owner must close its index, so that a lookup through its published core is \
         refused"
    );
    let (second_hot, mut second) = owner_over(trial.path());
    let second_core = second.published_core();
    assert_eq!(
        counts(&second_core),
        counted(0, 1),
        "an owner opened over a store that holds preimage.v1 syncs it once"
    );
    let mut second_contents = view.contents.clone();
    apply_all(&second_hot, &mut second_contents, &[Step::put("c", "v1")]);
    let gate = Arc::new(RwLock::new(()));
    poll_checkpoint_with(&mut second, &gate, &second_hot, publication_limits())
        .expect("the second owner publishes a datum of a known channel");
    assert_eq!(
        counts(&second_core),
        counted(0, 1),
        "a publication of a known channel through the second owner must append nothing and add no \
         sync"
    );
    assert_covered(
        &second_core,
        &present_preimages(&second_core, second.current_root()),
        "the published state",
        "the second owner's publication",
    );
}
