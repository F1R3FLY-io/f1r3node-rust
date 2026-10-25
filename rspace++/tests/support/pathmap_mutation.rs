use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pathmap::zipper::{Zipper, ZipperMoving, ZipperValues};
use proptest::prelude::*;

use super::*;

fn assert_mutation_unwind_child(case: &str, expected_diagnostic: &str) {
    let scratch = shared::rust::test_scratch::acquire("rspace-pathmap-mutation-unwind-");
    let marker = scratch.path().join("verified");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "rspace::pathmap_mutation::tests::pathmap_mutation_unwind_child",
            "--nocapture",
        ])
        .env("RSPACE_PATHMAP_MUTATION_UNWIND_CASE", case)
        .env("RSPACE_PATHMAP_MUTATION_UNWIND_MARKER", &marker)
        .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
    let outcome = shared::rust::test_process::run_child_probe(
        &mut command,
        std::time::Duration::from_secs(30),
    )
    .expect("run the PathMap mutation unwind child");
    match outcome {
        shared::rust::test_process::ProbeOutcome::Failed(failure) => {
            assert_eq!(failure.kind, shared::rust::test_process::ProbeFailureKind::Panicked);
            assert!(failure.stderr.contains(expected_diagnostic), "{}", failure.stderr);
        }
        shared::rust::test_process::ProbeOutcome::Passed => {
            panic!("the PathMap mutation unwind child did not fail: {case}")
        }
    }
    assert_eq!(std::fs::read(marker).unwrap(), case.as_bytes());
}

fn mutation_unwind_marker() -> std::path::PathBuf {
    std::env::var_os("RSPACE_PATHMAP_MUTATION_UNWIND_MARKER")
        .expect("PathMap mutation unwind marker")
        .into()
}

#[test]
#[ignore = "isolated PathMap mutation unwind child"]
fn pathmap_mutation_unwind_child() {
    let Some(case) = std::env::var_os("RSPACE_PATHMAP_MUTATION_UNWIND_CASE") else {
        return;
    };
    let case = case
        .to_str()
        .expect("PathMap mutation unwind case is UTF-8");
    let mut parts = case.split(':');
    match parts.next().unwrap() {
        "virtual" => {
            let index = parts.next().unwrap().parse::<usize>().unwrap();
            assert!(parts.next().is_none());
            run_virtual_navigation_unwind_child(index, case);
        }
        "phase" => {
            let phase = match parts.next().unwrap() {
                "Parked" => ExtensionPhase::Parked,
                "Grafted" => ExtensionPhase::Grafted,
                "Restored" => ExtensionPhase::Restored,
                _ => panic!("unknown extension phase"),
            };
            let insert = parts.next().unwrap().parse::<bool>().unwrap();
            assert!(parts.next().is_none());
            run_phase_interruption_child(phase, insert, case);
        }
        "path" => {
            let shared = parts.next().unwrap().parse::<bool>().unwrap();
            assert!(parts.next().is_none());
            run_panicking_path_child(shared, case);
        }
        // Disabled with the move to PathMap 0.3.4: this case reproduced the
        // 0.2.2 empty-sentinel panic (PMU-006), which 0.3.4 fixes. The
        // histories now run in process in
        // raw_four_operation_create_history_creates_the_final_path.
        //
        // "upstream" => {
        //     let index = parts.next().unwrap().parse::<usize>().unwrap();
        //     assert!(parts.next().is_none());
        //     run_upstream_create_history_child(index, case);
        // }
        _ => panic!("unknown PathMap mutation unwind case: {case}"),
    }
}

struct StageObservation {
    interrupt: Option<ExtensionPhase>,
    events: Vec<(ExtensionPhase, Option<usize>)>,
}

thread_local! {
    static EXTENSION_OBSERVER: RefCell<Option<StageObservation>> = const { RefCell::new(None) };
}

pub(super) fn observe_extension_stage(phase: ExtensionPhase, count: Option<usize>) {
    let interrupt = EXTENSION_OBSERVER.with(|slot| {
        let mut observer = slot.borrow_mut();
        let Some(observer) = observer.as_mut() else {
            return false;
        };
        observer.events.push((phase, count));
        observer.interrupt == Some(phase)
    });
    if interrupt {
        panic!("bounded native extension interruption at {phase:?}");
    }
}

struct StageScope;

impl StageScope {
    fn new(interrupt: Option<ExtensionPhase>) -> Self {
        EXTENSION_OBSERVER.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(StageObservation {
                interrupt,
                events: Vec::new(),
            });
        });
        Self
    }

    fn finish(self) -> Vec<(ExtensionPhase, Option<usize>)> {
        EXTENSION_OBSERVER.with(|slot| slot.borrow_mut().take().unwrap().events)
    }
}

impl Drop for StageScope {
    fn drop(&mut self) { EXTENSION_OBSERVER.with(|slot| *slot.borrow_mut() = None); }
}

#[derive(Default)]
struct PayloadCounts {
    clones: AtomicUsize,
    drops: AtomicUsize,
    disposals: AtomicUsize,
}

struct TrackedPayload {
    id: u64,
    identity: Box<u64>,
    counts: Option<Arc<PayloadCounts>>,
}

impl TrackedPayload {
    fn new(id: u64, counts: &Arc<PayloadCounts>) -> Self {
        Self {
            id,
            identity: Box::new(id),
            counts: Some(Arc::clone(counts)),
        }
    }

    fn identity(&self) -> usize { self.identity.as_ref() as *const u64 as usize }
}

impl Clone for TrackedPayload {
    fn clone(&self) -> Self {
        if let Some(counts) = &self.counts {
            counts.clones.fetch_add(1, Ordering::SeqCst);
        }
        Self {
            id: self.id,
            identity: Box::new(self.id),
            counts: self.counts.clone(),
        }
    }
}

impl Drop for TrackedPayload {
    fn drop(&mut self) {
        if let Some(counts) = &self.counts {
            counts.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[derive(Clone, Copy)]
struct TrackedDispose;

impl DisposeValue<TrackedPayload> for TrackedDispose {
    fn dispose(&self, value: TrackedPayload) {
        value
            .counts
            .as_ref()
            .expect("synthetic barriers never enter the real payload sink")
            .disposals
            .fetch_add(1, Ordering::SeqCst);
        drop(value);
    }
}

struct TrackedBarrier;

impl TeardownBarrier<TrackedPayload> for TrackedBarrier {
    fn make_barrier() -> TrackedPayload {
        TrackedPayload {
            id: 0,
            identity: Box::new(0),
            counts: None,
        }
    }
}

type TrackedCursor<'map, 'path> =
    NativePathCursor<'map, 'path, TrackedPayload, TrackedDispose, TrackedBarrier>;

fn observe_tracked(map: &PathMap<TrackedPayload>) -> Observation {
    let mut observation = Observation::default();
    let mut cursor = map.read_zipper();
    loop {
        observation.prefixes.insert(cursor.path().to_vec());
        if let Some(value) = cursor.val() {
            observation.values.insert(cursor.path().to_vec(), value.id);
        }
        if let Some(byte) = cursor.child_mask().iter().next() {
            cursor.descend_to_byte(byte);
            continue;
        }
        loop {
            if cursor.at_root() {
                return observation;
            }
            let previous = *cursor.path().last().unwrap();
            assert!(cursor.ascend_byte());
            if let Some(byte) = cursor.child_mask().iter().find(|byte| *byte > previous) {
                cursor.descend_to_byte(byte);
                break;
            }
        }
    }
}

fn retire_tracked(mut map: PathMap<TrackedPayload>, mut count: usize) {
    let paths = observe_tracked(&map).values.into_keys().collect::<Vec<_>>();
    {
        let mut cursor = TrackedCursor::new_counted(&mut map, &[], &TrackedDispose, &mut count);
        for path in paths {
            cursor.move_to_path(&path);
            TrackedDispose.dispose(cursor.remove_val().unwrap());
        }
    }
    assert_eq!(count, 0);
    assert!(observe_tracked(&map).values.is_empty());
}

#[derive(Clone, Copy)]
struct TestDispose;

impl DisposeValue<u64> for TestDispose {
    fn dispose(&self, _: u64) {}
}

struct TestBarrier;

impl TeardownBarrier<u64> for TestBarrier {
    fn make_barrier() -> u64 { 0 }
}

type Cursor<'map, 'path> = NativePathCursor<'map, 'path, u64, TestDispose, TestBarrier>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Observation {
    prefixes: BTreeSet<Vec<u8>>,
    values: BTreeMap<Vec<u8>, u64>,
}

impl Default for Observation {
    fn default() -> Self {
        Self {
            prefixes: BTreeSet::from([Vec::new()]),
            values: BTreeMap::new(),
        }
    }
}

impl Observation {
    fn create(&mut self, path: &[u8]) -> bool {
        let created = !self.prefixes.contains(path);
        for length in 0..=path.len() {
            self.prefixes.insert(path[..length].to_vec());
        }
        created
    }

    fn set(&mut self, path: &[u8], value: u64) -> Option<u64> {
        self.create(path);
        self.values.insert(path.to_vec(), value)
    }
}

fn observe(map: &PathMap<u64>) -> Observation {
    let mut observation = Observation::default();
    let mut cursor = map.read_zipper();
    loop {
        observation.prefixes.insert(cursor.path().to_vec());
        if let Some(value) = cursor.val() {
            observation.values.insert(cursor.path().to_vec(), *value);
        }
        if let Some(byte) = cursor.child_mask().iter().next() {
            cursor.descend_to_byte(byte);
            continue;
        }
        loop {
            if cursor.at_root() {
                return observation;
            }
            let previous = *cursor.path().last().unwrap();
            assert!(cursor.ascend_byte());
            if let Some(byte) = cursor.child_mask().iter().find(|byte| *byte > previous) {
                cursor.descend_to_byte(byte);
                break;
            }
        }
    }
}

fn observe_cursor(cursor: &mut Cursor<'_, '_>) -> Observation {
    let saved = cursor.path().to_vec();
    cursor.move_to_path(&[]);
    let mut observation = Observation::default();
    loop {
        observation.prefixes.insert(cursor.path().to_vec());
        if let Some(value) = cursor.val() {
            observation.values.insert(cursor.path().to_vec(), *value);
        }
        if cursor.descend_first_byte() {
            continue;
        }
        loop {
            if cursor.to_next_sibling_byte() {
                break;
            }
            if cursor.at_root() {
                cursor.move_to_path(&saved);
                return observation;
            }
            assert!(cursor.ascend_byte());
        }
    }
}

fn histories() -> [Vec<Vec<u8>>; 2] {
    [vec![vec![0], vec![178], vec![1], vec![178, 0]], vec![vec![1], vec![2], vec![3], vec![1, 0]]]
}

fn assert_read_selection(cursor: &Cursor<'_, '_>, model: &Observation, absolute: &[u8]) {
    let boundary_length = (0..=absolute.len())
        .take_while(|length| model.prefixes.contains(&absolute[..*length]))
        .last()
        .unwrap();
    assert_eq!(cursor.inner.path(), &absolute[..boundary_length]);
    assert!(cursor.inner.path_exists());
    assert_eq!(cursor.missing, boundary_length != absolute.len());
    if cursor.missing {
        assert_eq!(cursor.pending, absolute);
        assert!(!model.prefixes.contains(&absolute[..boundary_length + 1]));
    }
}

#[test]
fn native_leaf_extension_four_operation_create_and_set_histories() {
    for history in histories() {
        for valued in [false, true] {
            let mut map = PathMap::new();
            let mut model = Observation::default();
            let mut count = 0;
            for (index, path) in history.iter().enumerate() {
                let snapshot = map.clone();
                let before = model.clone();
                {
                    let mut cursor = Cursor::new_counted(&mut map, &[], &TestDispose, &mut count);
                    cursor.move_to_path(path);
                    assert_read_selection(&cursor, &model, path);
                    assert_eq!(cursor.path_exists(), model.prefixes.contains(path));
                    if valued {
                        assert_eq!(cursor.set_val(index as u64), model.set(path, index as u64));
                    } else {
                        assert_eq!(cursor.create_path(), model.create(path));
                    }
                    assert!(cursor.path_exists());
                    assert_eq!(cursor.path(), path);
                    assert!(!cursor.create_path());
                }
                assert_eq!(count, model.values.len());
                assert_eq!(observe(&map), model);
                assert_eq!(observe(&snapshot), before);
            }
        }
    }
}

#[test]
fn native_leaf_extension_four_operation_single_cursor_histories() {
    for history in histories() {
        for valued in [false, true] {
            let mut map = PathMap::new();
            let mut model = Observation::default();
            let mut count = 0;
            {
                let mut cursor = Cursor::new_counted(&mut map, &[], &TestDispose, &mut count);
                for (index, path) in history.iter().enumerate() {
                    cursor.move_to_path(path);
                    if valued {
                        assert_eq!(cursor.set_val(index as u64), model.set(path, index as u64));
                    } else {
                        assert_eq!(cursor.create_path(), model.create(path));
                    }
                    assert_eq!(observe_cursor(&mut cursor), model);
                    assert_eq!(cursor.path(), path);
                }
            }
            assert_eq!(count, model.values.len());
            assert_eq!(observe(&map), model);
        }
    }
}

#[test]
fn native_leaf_extension_prefix_construction_preserves_valued_and_dangling_roots() {
    for history in histories() {
        for valued in [false, true] {
            for authority in [vec![], vec![91], vec![91, 92, 93]] {
                let mut map = PathMap::new();
                let mut model = Observation::default();
                let mut count = 0;
                for (index, relative) in history.iter().enumerate() {
                    let target = [authority.as_slice(), relative.as_slice()].concat();
                    {
                        let mut cursor =
                            Cursor::new_counted(&mut map, &target, &TestDispose, &mut count);
                        assert!(cursor.at_root());
                        assert!(cursor.path().is_empty());
                        assert_eq!(cursor.path_exists(), model.prefixes.contains(&target));
                        assert_read_selection(&cursor, &model, &target);
                        assert!(!cursor.ascend_byte());
                        if valued {
                            assert_eq!(
                                cursor.set_val(index as u64),
                                model.set(&target, index as u64)
                            );
                        } else {
                            assert_eq!(cursor.create_path(), model.create(&target));
                        }
                    }
                    assert_eq!(count, model.values.len());
                    assert_eq!(observe(&map), model);
                }
            }
        }
    }
}

#[test]
fn native_leaf_extension_root_parent_values_compression_and_dense_siblings() {
    for valued in [false, true] {
        let mut map = PathMap::new();
        let mut model = Observation::default();
        let mut count = 0;
        let mut paths = vec![vec![], vec![17; 24], vec![17; 11]];
        paths.extend((0..=255).map(|byte| vec![17, byte]));
        paths.extend([vec![17; 25], vec![17; 12], vec![17, 178, 0]]);
        for (index, path) in paths.iter().enumerate() {
            {
                let mut cursor = Cursor::new_counted(&mut map, &[], &TestDispose, &mut count);
                cursor.move_to_path(path);
                if valued {
                    assert_eq!(cursor.set_val(index as u64), model.set(path, index as u64));
                } else {
                    assert_eq!(cursor.create_path(), model.create(path));
                }
            }
            assert_eq!(count, model.values.len());
            assert_eq!(observe(&map), model);
        }
    }
}

#[test]
fn native_virtual_navigation_exit_and_unwind_never_materialize_missing_paths() {
    for (index, authority) in [vec![], vec![3], vec![3, 9], vec![255, 254]]
        .into_iter()
        .enumerate()
    {
        let mut map = PathMap::new();
        {
            let mut cursor = Cursor::new(&mut map, &[], &TestDispose);
            cursor.move_to_path(&[3]);
            cursor.set_val(7);
        }
        let before = observe(&map);
        exercise_virtual_navigation(&mut map, &authority);
        assert_eq!(observe(&map), before);
        assert_mutation_unwind_child(&format!("virtual:{index}"), "bounded cursor-exit control");
    }
}

fn exercise_virtual_navigation(map: &mut PathMap<u64>, authority: &[u8]) {
    let mut cursor = Cursor::new(map, authority, &TestDispose);
    cursor.move_to_path(&[8, 7, 6]);
    assert!(!cursor.path_exists());
    assert!(!cursor.is_val());
    assert_eq!(cursor.val(), None);
    assert_eq!(cursor.remove_val(), None);
    assert!(!cursor.descend_first_byte());
    assert!(!cursor.descend_until());
    assert!(!cursor.to_next_sibling_byte());
    assert_eq!(cursor.path(), &[8, 7, 6]);
    assert!(cursor.ascend_byte());
    assert_eq!(cursor.path(), &[8, 7]);
}

fn run_virtual_navigation_unwind_child(index: usize, case: &str) {
    struct VerifyOnDrop {
        map: PathMap<u64>,
        before: Observation,
        case: String,
        marker: std::path::PathBuf,
    }
    impl Drop for VerifyOnDrop {
        fn drop(&mut self) {
            assert_eq!(observe(&self.map), self.before);
            std::fs::write(&self.marker, self.case.as_bytes()).unwrap();
        }
    }
    let authorities = [vec![], vec![3], vec![3, 9], vec![255, 254]];
    let authority = &authorities[index];
    let mut map = PathMap::new();
    {
        let mut cursor = Cursor::new(&mut map, &[], &TestDispose);
        cursor.move_to_path(&[3]);
        cursor.set_val(7);
    }
    let mut verify = VerifyOnDrop {
        before: observe(&map),
        map,
        case: case.to_owned(),
        marker: mutation_unwind_marker(),
    };
    exercise_virtual_navigation(&mut verify.map, authority);
    panic!("bounded cursor-exit control");
}

#[test]
fn native_virtual_authority_bounds_children_siblings_ascent_and_missing_removal() {
    let mut map = PathMap::new();
    {
        let mut cursor = Cursor::new(&mut map, &[], &TestDispose);
        for (path, value) in [
            (vec![4], 40),
            (vec![5], 50),
            (vec![4, 1, 2], 12),
            (vec![4, 3], 43),
            (vec![4, 3, 7], 37),
        ] {
            cursor.move_to_path(&path);
            cursor.set_val(value);
        }
    }
    let before = observe(&map);
    {
        let mut cursor = Cursor::new(&mut map, &[4], &TestDispose);
        assert!(cursor.at_root());
        assert_eq!(cursor.val(), Some(&40));
        assert!(!cursor.ascend_byte());
        assert!(!cursor.to_next_sibling_byte());
        assert!(!cursor.descend_until());
        assert!(cursor.descend_first_byte());
        assert_eq!(cursor.path(), &[1]);
        assert!(cursor.descend_until());
        assert_eq!(cursor.path(), &[1, 2]);
        assert_eq!(cursor.val(), Some(&12));
        assert!(cursor.ascend_byte());
        assert!(cursor.to_next_sibling_byte());
        assert_eq!(cursor.path(), &[3]);
        assert_eq!(cursor.val(), Some(&43));
        assert!(!cursor.to_next_sibling_byte());
        assert!(cursor.descend_until());
        assert_eq!(cursor.path(), &[3, 7]);
        cursor.move_to_path(&[2]);
        assert_eq!(cursor.remove_val(), None);
        assert!(!cursor.path_exists());
        assert!(cursor.to_next_sibling_byte());
        assert_eq!(cursor.path(), &[3]);
        cursor.move_to_path(&[99, 98]);
        assert!(cursor.ascend_byte());
        assert!(cursor.ascend_byte());
        assert!(!cursor.ascend_byte());
        assert!(cursor.at_root());
        assert_eq!(cursor.val(), Some(&40));
    }
    assert_eq!(observe(&map), before);
}

#[test]
fn native_leaf_extension_counted_create_replace_remove_and_reopen_trace() {
    let mut map = PathMap::new();
    let mut model = Observation::default();
    let mut count = 0;
    for (kind, path, value) in [
        (0, vec![], 7),
        (1, vec![1, 2], 0),
        (0, vec![1, 2], 12),
        (0, vec![1, 2, 3], 123),
        (0, vec![1, 2], 99),
        (2, vec![1, 2], 0),
        (2, vec![1, 2], 0),
        (2, vec![9, 9], 0),
        (2, vec![1, 2, 3], 0),
        (2, vec![], 0),
        (1, vec![], 0),
        (0, vec![], 8),
    ] {
        {
            let mut cursor = Cursor::new_counted(&mut map, &[], &TestDispose, &mut count);
            cursor.move_to_path(&path);
            match kind {
                0 => assert_eq!(cursor.set_val(value), model.set(&path, value)),
                1 => assert_eq!(cursor.create_path(), model.create(&path)),
                _ => assert_eq!(cursor.remove_val(), model.values.remove(&path)),
            }
        }
        assert_eq!(count, model.values.len());
        assert_eq!(observe(&map), model);
        assert_ne!(count + 1, model.values.len());
    }
}

#[test]
fn native_leaf_extension_observation_controls_detect_root_and_sibling_loss() {
    let mut map = PathMap::new();
    let mut model = Observation::default();
    {
        let mut cursor = Cursor::new(&mut map, &[], &TestDispose);
        for (path, value) in [(vec![], 7), (vec![1, 2], 12), (vec![1, 3], 13)] {
            cursor.move_to_path(&path);
            assert_eq!(cursor.set_val(value), model.set(&path, value));
        }
        cursor.move_to_path(&[1, 4]);
        assert_eq!(cursor.create_path(), model.create(&[1, 4]));
    }
    assert_eq!(observe(&map), model);
    let mut lost_root = model.clone();
    lost_root.values.remove(&Vec::new());
    assert_ne!(observe(&map), lost_root);
    let mut wrong_boundary = model.clone();
    wrong_boundary.prefixes.remove(&vec![1, 2]);
    wrong_boundary.values.remove(&vec![1, 2]);
    assert_ne!(observe(&map), wrong_boundary);
    let mut lost_dangling = model;
    lost_dangling.prefixes.remove(&vec![1, 4]);
    assert_ne!(observe(&map), lost_dangling);
}

#[test]
fn native_leaf_extension_fresh_donor_exact_suffix_and_borrowed_retirement() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for suffix in [vec![], vec![0], vec![178, 0], vec![37; 16_384]] {
                let donor =
                    GuardedPathMap::<u64, ValueFreeDisposer, TestBarrier>::value_free_suffix(
                        &suffix,
                    );
                donor.with_read_cursor(|mut cursor| {
                    assert!(cursor.path().is_empty());
                    assert_eq!(cursor.val(), None);
                    for (index, byte) in suffix.iter().enumerate() {
                        assert!(cursor.path_exists());
                        assert_eq!(cursor.path(), &suffix[..index]);
                        assert_eq!(cursor.val(), None);
                        assert_eq!(cursor.child_mask().iter().collect::<Vec<_>>(), vec![*byte]);
                        assert_eq!(cursor.descend_to_existing([*byte]), 1);
                    }
                    assert_eq!(cursor.path(), suffix);
                    assert!(cursor.path_exists());
                    assert_eq!(cursor.val(), None);
                    assert_eq!(cursor.child_mask().iter().count(), 0);
                });
                if suffix.len() < 32 {
                    let mut destination = PathMap::new();
                    {
                        let mut writer = destination.write_zipper();
                        donor.graft_value_free_suffix(&mut writer);
                    }
                    let mut expected = Observation::default();
                    expected.create(&suffix);
                    assert_eq!(observe(&destination), expected);
                    drop(donor);
                    assert_eq!(observe(&destination), expected);
                } else {
                    drop(donor);
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn native_leaf_extension_actual_naive_graft_and_stale_count_controls() {
    let mut map = PathMap::new();
    let mut count = 0;
    {
        let mut cursor = Cursor::new_counted(&mut map, &[], &TestDispose, &mut count);
        assert_eq!(cursor.set_val(7), None);
        cursor.move_to_path(&[1, 2]);
        assert_eq!(cursor.set_val(12), None);
        cursor.move_to_path(&[1, 3]);
        assert!(cursor.create_path());
    }
    let original = observe(&map);
    assert_eq!(count, original.values.len());
    let donor = GuardedPathMap::<u64, ValueFreeDisposer, TestBarrier>::value_free_suffix(&[4]);
    let mut root_mutant = map.clone();
    {
        let mut writer = root_mutant.write_zipper();
        donor.graft_value_free_suffix(&mut writer);
    }
    assert_eq!(root_mutant.get_val_at([]), None);
    assert_ne!(observe(&root_mutant), original);
    let mut branch_mutant = map.clone();
    {
        let mut writer = branch_mutant.write_zipper();
        writer.descend_to(&[1]);
        assert_eq!(writer.child_count(), 2);
        donor.graft_value_free_suffix(&mut writer);
    }
    assert!(!branch_mutant.path_exists_at([1, 2]));
    assert!(!branch_mutant.path_exists_at([1, 3]));
    assert_ne!(observe(&branch_mutant), original);
    let mut stale_count_mutant = map.clone();
    {
        let mut writer = stale_count_mutant.write_zipper();
        assert_eq!(writer.remove_val(false), Some(7));
    }
    assert_ne!(count, observe(&stale_count_mutant).values.len());
    assert_eq!(observe(&map), original);
}

struct TrackedFixture {
    map: PathMap<TrackedPayload>,
    count: usize,
    model: Observation,
    counts: Vec<Arc<PayloadCounts>>,
    identities: BTreeMap<Vec<u8>, usize>,
}

impl TrackedFixture {
    fn new() -> Self {
        let mut fixture = Self {
            map: PathMap::new(),
            count: 0,
            model: Observation::default(),
            counts: Vec::new(),
            identities: BTreeMap::new(),
        };
        {
            let mut cursor = TrackedCursor::new_counted(
                &mut fixture.map,
                &[],
                &TrackedDispose,
                &mut fixture.count,
            );
            for (index, byte) in [0, 1, 2, 178].into_iter().enumerate() {
                let counts = Arc::new(PayloadCounts::default());
                let payload = TrackedPayload::new(index as u64 + 1, &counts);
                fixture.identities.insert(vec![byte], payload.identity());
                fixture.model.set(&[byte], payload.id);
                fixture.counts.push(counts);
                cursor.move_to_path(&[byte]);
                assert!(cursor.set_val(payload).is_none());
            }
        }
        assert_eq!(observe_tracked(&fixture.map), fixture.model);
        assert_eq!(fixture.count, 4);
        fixture
    }
}

#[test]
fn native_leaf_extension_unique_payload_identity_and_no_clone_restore() {
    for insert in [false, true] {
        let mut fixture = TrackedFixture::new();
        let incoming_counts = Arc::new(PayloadCounts::default());
        let scope = StageScope::new(None);
        {
            let mut cursor = TrackedCursor::new_counted(
                &mut fixture.map,
                &[],
                &TrackedDispose,
                &mut fixture.count,
            );
            cursor.move_to_path(&[178, 0]);
            if insert {
                let payload = TrackedPayload::new(5, &incoming_counts);
                let identity = payload.identity();
                assert!(cursor.set_val(payload).is_none());
                assert_eq!(cursor.val().unwrap().identity(), identity);
                fixture.model.set(&[178, 0], 5);
            } else {
                assert!(cursor.create_path());
                fixture.model.create(&[178, 0]);
            }
        }
        assert_eq!(scope.finish(), vec![
            (ExtensionPhase::Parked, Some(3)),
            (ExtensionPhase::Grafted, Some(3)),
            (ExtensionPhase::Restored, Some(4)),
        ]);
        assert_eq!(observe_tracked(&fixture.map), fixture.model);
        assert_eq!(fixture.count, fixture.model.values.len());
        for (path, identity) in &fixture.identities {
            assert_eq!(fixture.map.get_val_at(path).unwrap().identity(), *identity);
        }
        for counts in fixture
            .counts
            .iter()
            .chain(std::iter::once(&incoming_counts))
        {
            assert_eq!(counts.clones.load(Ordering::SeqCst), 0);
            assert_eq!(counts.disposals.load(Ordering::SeqCst), 0);
            assert_eq!(counts.drops.load(Ordering::SeqCst), 0);
        }
        retire_tracked(fixture.map, fixture.count);
        for counts in &fixture.counts {
            assert_eq!(counts.clones.load(Ordering::SeqCst), 0);
            assert_eq!(counts.disposals.load(Ordering::SeqCst), 1);
            assert_eq!(counts.drops.load(Ordering::SeqCst), 1);
        }
        assert_eq!(incoming_counts.disposals.load(Ordering::SeqCst), usize::from(insert));
        assert_eq!(incoming_counts.drops.load(Ordering::SeqCst), usize::from(insert));
    }
}

#[test]
fn native_leaf_extension_phase_interruptions_preserve_exact_resident_authority() {
    for phase in [ExtensionPhase::Parked, ExtensionPhase::Grafted, ExtensionPhase::Restored] {
        for insert in [false, true] {
            assert_mutation_unwind_child(
                &format!("phase:{phase:?}:{insert}"),
                "bounded native extension interruption",
            );
        }
    }
}

fn run_phase_interruption_child(phase: ExtensionPhase, insert: bool, case: &str) {
    struct VerifyOnDrop {
        fixture: Option<TrackedFixture>,
        incoming_counts: Arc<PayloadCounts>,
        scope: Option<StageScope>,
        phase: ExtensionPhase,
        insert: bool,
        case: String,
        marker: std::path::PathBuf,
    }
    impl Drop for VerifyOnDrop {
        fn drop(&mut self) {
            let mut fixture = self.fixture.take().unwrap();
            let mut expected_events = vec![(ExtensionPhase::Parked, Some(3))];
            if self.phase != ExtensionPhase::Parked {
                expected_events.push((ExtensionPhase::Grafted, Some(3)));
                fixture.model.create(&[178, 0]);
            }
            if self.phase == ExtensionPhase::Restored {
                expected_events.push((ExtensionPhase::Restored, Some(4)));
            } else {
                fixture.model.values.remove(&vec![178]);
            }
            assert_eq!(self.scope.take().unwrap().finish(), expected_events);
            assert_eq!(observe_tracked(&fixture.map), fixture.model);
            assert_eq!(fixture.count, fixture.model.values.len());
            for (path, identity) in &fixture.identities {
                if fixture.model.values.contains_key(path) {
                    assert_eq!(fixture.map.get_val_at(path).unwrap().identity(), *identity);
                }
            }
            for (index, counts) in fixture.counts.iter().enumerate() {
                let retired = usize::from(index == 3 && self.phase != ExtensionPhase::Restored);
                assert_eq!(counts.clones.load(Ordering::SeqCst), 0);
                assert_eq!(counts.disposals.load(Ordering::SeqCst), retired);
                assert_eq!(counts.drops.load(Ordering::SeqCst), retired);
            }
            assert_eq!(self.incoming_counts.clones.load(Ordering::SeqCst), 0);
            assert_eq!(
                self.incoming_counts.disposals.load(Ordering::SeqCst),
                usize::from(self.insert)
            );
            assert_eq!(self.incoming_counts.drops.load(Ordering::SeqCst), usize::from(self.insert));
            retire_tracked(fixture.map, fixture.count);
            for counts in &fixture.counts {
                assert_eq!(counts.disposals.load(Ordering::SeqCst), 1);
                assert_eq!(counts.drops.load(Ordering::SeqCst), 1);
            }
            std::fs::write(&self.marker, self.case.as_bytes()).unwrap();
        }
    }
    let fixture = TrackedFixture::new();
    let incoming_counts = Arc::new(PayloadCounts::default());
    let incoming = insert.then(|| TrackedPayload::new(5, &incoming_counts));
    let scope = StageScope::new(Some(phase));
    let mut verify = VerifyOnDrop {
        fixture: Some(fixture),
        incoming_counts,
        scope: Some(scope),
        phase,
        insert,
        case: case.to_owned(),
        marker: mutation_unwind_marker(),
    };
    let fixture = verify.fixture.as_mut().unwrap();
    let mut cursor =
        TrackedCursor::new_counted(&mut fixture.map, &[], &TrackedDispose, &mut fixture.count);
    cursor.move_to_path(&[178, 0]);
    if let Some(value) = incoming {
        assert!(cursor.set_val(value).is_none());
    } else {
        cursor.create_path();
    }
}

struct PanickingPath;

impl AsRef<[u8]> for PanickingPath {
    fn as_ref(&self) -> &[u8] { panic!("bounded AsRef path conversion interruption") }
}

#[test]
fn native_leaf_extension_guarded_incoming_value_survives_panicking_path_conversion() {
    for shared in [false, true] {
        assert_mutation_unwind_child(
            &format!("path:{shared}"),
            "bounded AsRef path conversion interruption",
        );
    }
}

fn run_panicking_path_child(shared: bool, case: &str) {
    struct VerifyOnDrop {
        map: Option<GuardedPathMap<TrackedPayload, TrackedDispose, TrackedBarrier>>,
        snapshot: Option<GuardedPathMap<TrackedPayload, TrackedDispose, TrackedBarrier>>,
        before: Vec<u8>,
        resident_counts: Arc<PayloadCounts>,
        incoming_counts: Arc<PayloadCounts>,
        case: String,
        marker: std::path::PathBuf,
    }
    impl Drop for VerifyOnDrop {
        fn drop(&mut self) {
            assert_eq!(self.incoming_counts.clones.load(Ordering::SeqCst), 0);
            assert_eq!(self.incoming_counts.disposals.load(Ordering::SeqCst), 1);
            assert_eq!(self.incoming_counts.drops.load(Ordering::SeqCst), 1);
            let map = self.map.take().unwrap();
            assert_eq!(map.compact_u64_bytes(|value| value.id), self.before);
            if let Some(snapshot) = &self.snapshot {
                assert_eq!(snapshot.compact_u64_bytes(|value| value.id), self.before);
            }
            drop(map);
            drop(self.snapshot.take());
            let resident_instances = self.resident_counts.clones.load(Ordering::SeqCst) + 1;
            assert_eq!(self.resident_counts.disposals.load(Ordering::SeqCst), resident_instances);
            assert_eq!(self.resident_counts.drops.load(Ordering::SeqCst), resident_instances);
            std::fs::write(&self.marker, self.case.as_bytes()).unwrap();
        }
    }
    let resident_counts = Arc::new(PayloadCounts::default());
    let incoming_counts = Arc::new(PayloadCounts::default());
    let mut map =
        GuardedPathMap::<TrackedPayload, TrackedDispose, TrackedBarrier>::new_with_barrier(
            TrackedDispose,
        );
    assert!(!map.set_val_at([1], TrackedPayload::new(1, &resident_counts)));
    assert!(map.create_path([1, 2]));
    let before = map.compact_u64_bytes(|value| value.id);
    let snapshot = shared.then(|| map.clone());
    let mut verify = VerifyOnDrop {
        map: Some(map),
        snapshot,
        before,
        resident_counts,
        incoming_counts,
        case: case.to_owned(),
        marker: mutation_unwind_marker(),
    };
    verify
        .map
        .as_mut()
        .unwrap()
        .set_val_at(PanickingPath, TrackedPayload::new(2, &verify.incoming_counts));
}

/// PathMap 0.2.2 panicked on these histories at an empty sentinel node
/// (PMU-006). Upstream 06d10991a3, released in 0.3.4, makes `get_child_mut`
/// skip empty children.
#[test]
fn raw_four_operation_create_history_creates_the_final_path() {
    for mut history in histories() {
        let final_path = history.pop().expect("each history ends in its final path");
        let mut map = PathMap::<u64>::new();
        for path in history {
            map.create_path(path);
        }
        assert!(map.create_path(&final_path));
        assert!(map.path_exists_at(&final_path));
    }
}

// Disabled with the move to PathMap 0.3.4: this child reproduced the 0.2.2
// empty-sentinel panic (PMU-006), which 0.3.4 fixes.
// raw_four_operation_create_history_creates_the_final_path now checks the same
// histories in process.
//
// fn run_upstream_create_history_child(index: usize, case: &str) {
//     let mut map = PathMap::<u64>::new();
//     let mut history = histories().into_iter().nth(index).unwrap();
//     let final_path = history.pop().unwrap();
//     for path in history {
//         map.create_path(path);
//     }
//     std::fs::write(mutation_unwind_marker(), case.as_bytes()).unwrap();
//     map.create_path(final_path);
// }

#[derive(Clone, Debug)]
enum MinimalMutation {
    Set(Vec<u8>, u64),
    Create(Vec<u8>),
    Remove(Vec<u8>),
}

impl MinimalMutation {
    fn path(&self) -> &[u8] {
        match self {
            Self::Set(path, _) | Self::Create(path) | Self::Remove(path) => path,
        }
    }
}

fn raw_minimal_history_trace(operations: &[MinimalMutation], probes: &[Vec<u8>]) -> Vec<String> {
    let mut map = PathMap::<u64>::new();
    let mut trace = Vec::new();
    for (index, operation) in operations.iter().enumerate() {
        let before = observe(&map);
        let expected_boundary = (0..=operation.path().len())
            .take_while(|length| before.prefixes.contains(&operation.path()[..*length]))
            .last()
            .unwrap();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut direct = map.read_zipper();
            let direct_count = direct.descend_to_existing(operation.path());
            assert_eq!(direct_count, expected_boundary);
            assert_eq!(direct.path(), &operation.path()[..expected_boundary]);
            drop(direct);
            let mut writer = map.write_zipper();
            {
                let origin = writer.fork_read_zipper();
                let focus: TrieRefBorrowed<'_, u64> = origin.trie_ref_at_path([]);
                let mut borrowed = focus.fork_read_zipper();
                let borrowed_count = borrowed.descend_to_existing(operation.path());
                assert_eq!(borrowed_count, expected_boundary);
                assert_eq!(borrowed.path(), &operation.path()[..expected_boundary]);
            }
            writer.move_to_path(operation.path());
            match operation {
                MinimalMutation::Set(_, value) => {
                    assert_eq!(
                        writer.set_val(*value),
                        before.values.get(operation.path()).copied()
                    );
                }
                MinimalMutation::Create(_) => {
                    assert_eq!(writer.create_path(), !before.prefixes.contains(operation.path()));
                }
                MinimalMutation::Remove(_) => {
                    assert_eq!(
                        writer.remove_val(false),
                        before.values.get(operation.path()).copied()
                    );
                }
            }
        }));
        if let Err(failure) = outcome {
            let message = failure
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| failure.downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic");
            trace.push(format!("raw operation {index} {operation:?}: panic {message}"));
            break;
        }
        let points = probes
            .iter()
            .map(|path| (path, map.path_exists_at(path), map.get_val_at(path).copied()))
            .collect::<Vec<_>>();
        let mut leaf_descent = Vec::new();
        for path in probes.iter().filter(|path| map.path_exists_at(path)) {
            let mut read = map.read_zipper();
            read.descend_to(path);
            if read.child_mask().iter().next().is_none() {
                let descended = read.descend_first_byte();
                leaf_descent.push((path, descended, read.path().to_vec()));
            }
        }
        trace.push(format!(
            "raw operation {index} {operation:?}: mask walk {:?}; point reads {points:?}; \
             optimized leaf descent {leaf_descent:?}",
            observe(&map),
        ));
    }
    trace
}

fn assert_minimal_native_history(operations: &[MinimalMutation], probes: &[Vec<u8>]) {
    let raw_trace = raw_minimal_history_trace(operations, probes);
    eprintln!("pinned upstream counterpart: {raw_trace:#?}");
    let mut map = PathMap::new();
    let mut model = Observation::default();
    let mut count = 0;
    for (index, operation) in operations.iter().enumerate() {
        {
            let mut cursor = Cursor::new_counted(&mut map, &[], &TestDispose, &mut count);
            cursor.move_to_path(operation.path());
            assert_read_selection(&cursor, &model, operation.path());
            match operation {
                MinimalMutation::Set(path, value) => {
                    assert_eq!(cursor.set_val(*value), model.set(path, *value));
                }
                MinimalMutation::Create(path) => {
                    assert_eq!(cursor.create_path(), model.create(path));
                }
                MinimalMutation::Remove(path) => {
                    assert_eq!(cursor.remove_val(), model.values.remove(path));
                }
            }
        }
        for path in model.prefixes.iter().chain(probes) {
            assert_eq!(
                map.path_exists_at(path),
                model.prefixes.contains(path),
                "native operation {index} {operation:?}, point topology {path:?}; {raw_trace:#?}",
            );
            assert_eq!(
                map.get_val_at(path).copied(),
                model.values.get(path).copied(),
                "native operation {index} {operation:?}, point value {path:?}; {raw_trace:#?}",
            );
        }
        assert_eq!(count, model.values.len());
        assert_eq!(
            observe(&map),
            model,
            "native operation {index} {operation:?}, explicit-mask oracle; {raw_trace:#?}",
        );
    }
}

#[test]
fn native_leaf_extension_minimal_valued_leaf_and_missing_sibling_branch() {
    assert_minimal_native_history(
        &[MinimalMutation::Set(vec![0], 0), MinimalMutation::Create(vec![1, 0])],
        &[vec![], vec![0], vec![1], vec![1, 0], vec![0, 0]],
    );
}

#[test]
fn native_leaf_extension_minimal_parent_removal_retains_valued_descendant() {
    assert_minimal_native_history(
        &[
            MinimalMutation::Set(vec![], 7),
            MinimalMutation::Create(vec![1, 2]),
            MinimalMutation::Set(vec![1, 2], 12),
            MinimalMutation::Set(vec![1, 2, 3], 123),
            MinimalMutation::Set(vec![1, 2], 99),
            MinimalMutation::Remove(vec![1, 2]),
        ],
        &[vec![], vec![1], vec![1, 2], vec![1, 2, 3], vec![1, 3]],
    );
}

fn raw_grafted_parent_removal(prune: bool) -> (Observation, bool, Vec<u8>, usize) {
    let mut map = PathMap::<u64>::new();
    assert_eq!(map.set_val_at([1], 7), None);
    let mut donor = PathMap::<u64>::new();
    assert!(donor.create_path([2]));
    {
        let mut writer = map.write_zipper();
        writer.move_to_path(&[1]);
        assert_eq!(writer.child_count(), 0);
        let parked = writer.remove_val(false).unwrap();
        writer.graft(&donor.read_zipper());
        assert_eq!(writer.set_val(parked), None);
        assert_eq!(writer.child_count(), 1);
    }
    assert_eq!(map.get_val_at([1]), Some(&7));
    assert_eq!(map.get_val_at([1, 2]), None);
    assert!(map.path_exists_at([1, 2]));
    let mut expected_before = Observation::default();
    expected_before.set(&[1], 7);
    expected_before.create(&[1, 2]);
    assert_eq!(observe(&map), expected_before);
    {
        let mut writer = map.write_zipper();
        writer.move_to_path(&[1]);
        assert_eq!(writer.child_count(), 1);
        assert_eq!(writer.remove_val(prune), Some(7));
    }
    assert_eq!(map.get_val_at([1]), None);
    let observation = observe(&map);
    let point_exists = map.path_exists_at([1, 2]);
    let last_path = {
        let mut read = map.read_zipper();
        assert!(pathmap::zipper::ZipperIteration::descend_last_path(&mut read));
        read.path().to_vec()
    };
    let writer_children = {
        let mut writer = map.write_zipper();
        writer.move_to_path(&last_path);
        writer.child_count()
    };
    (observation, point_exists, last_path, writer_children)
}

/// PathMap 0.2.2 left a duplicate empty child at `[1]` here, so the last path
/// stopped at `[1]` with one child (PMU-008). Upstream 02afb73523, released in
/// 0.3.4, removes it.
#[test]
fn raw_nonpruning_parent_removal_keeps_the_same_key_child_consistent() {
    let actual = raw_grafted_parent_removal(false);
    let mut expected = Observation::default();
    expected.create(&[1, 2]);
    assert_eq!(actual, (expected, true, vec![1, 2], 0));
}

#[test]
fn raw_pruning_parent_removal_preserves_same_key_real_child() {
    let actual = raw_grafted_parent_removal(true);
    let mut expected = Observation::default();
    expected.create(&[1, 2]);
    assert_eq!(actual, (expected, true, vec![1, 2], 0));
}

#[test]
fn native_terminal_dangling_value_cycles_preserve_exact_topology() {
    for path in [vec![1], vec![17; 24], vec![178, 0]] {
        let mut map = PathMap::new();
        let mut count = 0;
        let mut expected = Observation::default();
        expected.create(&path);
        for value in 0..16 {
            {
                let mut cursor = Cursor::new_counted(&mut map, &[], &TestDispose, &mut count);
                cursor.move_to_path(&path);
                cursor.create_path();
                assert_eq!(cursor.set_val(value), None);
                assert_eq!(cursor.remove_val(), Some(value));
                assert!(cursor.path_exists());
                assert_eq!(cursor.inner.child_count(), 0);
            }
            assert_eq!(count, 0);
            assert_eq!(observe(&map), expected);
            let mut read = map.read_zipper();
            assert!(pathmap::zipper::ZipperIteration::descend_last_path(&mut read));
            assert_eq!(read.path(), path);
        }
    }
}

#[test]
fn native_unique_mutations_never_clone_the_real_value_disposer() {
    struct UniquePolicy;

    impl Clone for UniquePolicy {
        fn clone(&self) -> Self { panic!("unique mutation must not clone its disposal policy") }
    }

    impl DisposeValue<u64> for UniquePolicy {
        fn dispose(&self, _value: u64) {}
    }

    let mut map = GuardedPathMap::<u64, UniquePolicy, TestBarrier>::new_with_barrier(UniquePolicy);
    assert!(!map.set_val_at([1], 7));
    assert!(map.create_path([1, 2]));
    assert!(!map.set_val_at([1, 2, 3], 123));
    assert!(map.set_val_at([1], 8));
    assert_eq!(map.get([1]), Some(&8));
    assert_eq!(map.get([1, 2, 3]), Some(&123));
    assert!(map.create_path([2, 3]));
    drop(map);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn native_leaf_extension_original_order_exact_prefix_and_value_snapshot_property(
        authority in prop::collection::vec(any::<u8>(), 0..5),
        operations in prop::collection::vec(
            (0u8..5, prop::collection::vec(any::<u8>(), 0..17), any::<u64>()),
            0..65,
        ),
        reverse_drop in any::<bool>(),
    ) {
        let mut map = PathMap::new();
        let mut model = Observation::default();
        let mut count = 0;
        let mut snapshots = Vec::new();
        for (index, (kind, relative, value)) in operations.into_iter().enumerate() {
            if index % 4 == 0 {
                snapshots.push((map.clone(), model.clone()));
            }
            let target = [authority.as_slice(), relative.as_slice()].concat();
            {
                let mut cursor = Cursor::new_counted(
                    &mut map, &authority, &TestDispose, &mut count,
                );
                cursor.move_to_path(&relative);
                assert_read_selection(&cursor, &model, &target);
                prop_assert_eq!(cursor.path(), relative.as_slice());
                prop_assert_eq!(cursor.path_exists(), model.prefixes.contains(&target));
                prop_assert_eq!(cursor.val().copied(), model.values.get(&target).copied());
                match kind {
                    0 => prop_assert_eq!(cursor.create_path(), model.create(&target)),
                    1 | 2 => prop_assert_eq!(cursor.set_val(value), model.set(&target, value)),
                    3 => prop_assert_eq!(cursor.remove_val(), model.values.remove(&target)),
                    _ => {
                        prop_assert_eq!(cursor.is_val(), model.values.contains_key(&target));
                    }
                }
                prop_assert_eq!(cursor.path_exists(), model.prefixes.contains(&target));
                prop_assert_eq!(cursor.val().copied(), model.values.get(&target).copied());
            }
            prop_assert_eq!(count, model.values.len());
            prop_assert_eq!(observe(&map), model.clone());
            for (snapshot, expected) in &snapshots {
                prop_assert_eq!(observe(snapshot), expected.clone());
            }
            if snapshots.len() > 4 {
                let dropped = if reverse_drop { snapshots.pop().unwrap() } else { snapshots.remove(0) };
                drop(dropped);
                prop_assert_eq!(observe(&map), model.clone());
            }
        }
        drop(map);
        if reverse_drop {
            snapshots.reverse();
        }
        while let Some((snapshot, expected)) = snapshots.pop() {
            prop_assert_eq!(observe(&snapshot), expected);
            drop(snapshot);
            for (remaining, expected) in &snapshots {
                prop_assert_eq!(observe(remaining), expected.clone());
            }
        }
    }
}
