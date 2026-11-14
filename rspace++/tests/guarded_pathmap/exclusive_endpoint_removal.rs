use std::cell::Cell;
use std::collections::BTreeMap;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::Duration;

use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::RngSeed;
use shared::rust::test_process::{
    ALLOW_EXPECTED_PROBE_CORE, ExpectedProbeCorePolicy, ProbeOutcome,
    prepare_expected_probe_core_dump, run_child_probe,
};

use super::{DisposeValue, ExclusiveRemoval, GuardedPathMap, TeardownBarrier};

const BARRIER: u64 = u64::MAX;
const MAX_HANDLES: usize = 6;
const MODEL_KEYS: [&[u8]; 6] = [&[0], &[0, 1], &[1], &[1, 2, 0], &[2], &[2, 2]];
const PROBE_KEYS: [&[u8]; 4] = [&[7], &[7, 1], &[7, 2, 9], &[8]];
const WRITERS: u8 = 8;
const DEEP_KEY_BYTES: usize = 65_536;
const DEEP_ANCESTOR_BYTES: usize = 32_768;
const DEEP_SHAPE: &str = "GUARDED_PATHMAP_EXCLUSIVE_DEEP_SHAPE";
const DEEP_SHAPES: [&str; 4] =
    ["root_fork_sibling", "valued_ancestor", "lone_chain", "dense_teeth_removed"];
const DENSE_LEVELS: usize = 16_384;
const DEEP_CHILD: &str = concat!(
    "rspace::guarded_pathmap::exclusive_endpoint_removal::",
    "exclusive_deep_key_small_stack_child"
);

thread_local! {
    static BARRIERS_MADE: Cell<usize> = const { Cell::new(0) };
    static BARRIERS_DISCARDED: Cell<usize> = const { Cell::new(0) };
}

fn barrier_counts() -> (usize, usize) { (BARRIERS_MADE.get(), BARRIERS_DISCARDED.get()) }

struct CountedBarrier;

impl TeardownBarrier<u64> for CountedBarrier {
    fn make_barrier() -> u64 {
        BARRIERS_MADE.set(BARRIERS_MADE.get() + 1);
        BARRIER
    }

    fn discard_barrier(_barrier: u64) { BARRIERS_DISCARDED.set(BARRIERS_DISCARDED.get() + 1); }
}

impl TeardownBarrier<CountedClone> for CountedBarrier {
    fn make_barrier() -> CountedClone {
        BARRIERS_MADE.set(BARRIERS_MADE.get() + 1);
        CountedClone::new(BARRIER, &Arc::new(AtomicUsize::new(0)))
    }

    fn discard_barrier(_barrier: CountedClone) {
        BARRIERS_DISCARDED.set(BARRIERS_DISCARDED.get() + 1);
    }
}

struct CountedClone {
    id: u64,
    clones: Arc<AtomicUsize>,
}

impl CountedClone {
    fn new(id: u64, clones: &Arc<AtomicUsize>) -> Self {
        Self {
            id,
            clones: Arc::clone(clones),
        }
    }
}

impl Clone for CountedClone {
    fn clone(&self) -> Self {
        self.clones.fetch_add(1, Ordering::SeqCst);
        Self::new(self.id, &self.clones)
    }
}

#[derive(Clone, Default)]
struct RecordingDisposer(Arc<Mutex<Vec<u64>>>);

impl RecordingDisposer {
    fn record(&self, id: u64) {
        self.0
            .lock()
            .expect("the disposal record is not poisoned")
            .push(id);
    }

    fn take(&self) -> Vec<u64> {
        std::mem::take(&mut *self.0.lock().expect("the disposal record is not poisoned"))
    }
}

impl DisposeValue<u64> for RecordingDisposer {
    fn dispose(&self, value: u64) { self.record(value); }
}

impl DisposeValue<CountedClone> for RecordingDisposer {
    fn dispose(&self, value: CountedClone) { self.record(value.id); }
}

type Subject = GuardedPathMap<u64, RecordingDisposer, CountedBarrier>;
type ProbeSubject = GuardedPathMap<CountedClone, RecordingDisposer, CountedBarrier>;
type DeepFixture = (Subject, Vec<u8>, Option<(Vec<u8>, u64)>, usize);

fn subject_with(entries: &BTreeMap<Vec<u8>, u64>, dispose: &RecordingDisposer) -> Subject {
    let mut subject = Subject::new_with_barrier(dispose.clone());
    for (key, value) in entries {
        assert!(!subject.set_val_at(key, *value), "every fixture key is written once");
    }
    subject
}

fn entries(subject: &Subject) -> BTreeMap<Vec<u8>, u64> {
    subject.iter().map(|(path, value)| (path, *value)).collect()
}

fn dangling_leaves(subject: &Subject) -> usize {
    let mut dangling = 0usize;
    subject.visit_topology(|path, value, is_leaf| {
        dangling += usize::from(matches!((path.is_empty(), value, is_leaf), (false, None, true)));
    });
    dangling
}

fn topology_key() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        4 => prop::collection::vec(0u8..3, 0..6),
        1 => (prop::collection::vec(0u8..3, 0..4), 0u8..3, 1usize..=160).prop_map(
            |(mut stem, byte, run)| {
                stem.resize(stem.len() + run, byte);
                stem
            }
        ),
    ]
}

#[derive(Clone, Debug)]
enum Step {
    Construct,
    CloneHandle(Index),
    DropHandle(Index),
    Write(Index, Index),
    Remove(Index, Index),
}

fn provenance_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        1 => Just(Step::Construct),
        2 => any::<Index>().prop_map(Step::CloneHandle),
        1 => any::<Index>().prop_map(Step::DropHandle),
        3 => (any::<Index>(), any::<Index>()).prop_map(|(handle, key)| Step::Write(handle, key)),
        3 => (any::<Index>(), any::<Index>()).prop_map(|(handle, key)| Step::Remove(handle, key)),
    ]
}

struct ModelAnchor {
    exclusive: bool,
    holders: usize,
}

struct ModelHandle {
    subject: Subject,
    anchor: usize,
    oracle: BTreeMap<Vec<u8>, u64>,
}

struct ProvenanceModel {
    anchors: Vec<ModelAnchor>,
    handles: Vec<ModelHandle>,
    next_value: u64,
    dispose: RecordingDisposer,
}

impl ProvenanceModel {
    fn new(steps: usize) -> Self {
        let mut model = Self {
            anchors: Vec::with_capacity(steps + 1),
            handles: Vec::with_capacity(MAX_HANDLES),
            next_value: 0,
            dispose: RecordingDisposer::default(),
        };
        model.construct();
        model
    }

    fn fresh_anchor(&mut self, exclusive: bool) -> usize {
        self.anchors.push(ModelAnchor {
            exclusive,
            holders: 1,
        });
        self.anchors.len() - 1
    }

    fn value(&mut self) -> u64 {
        self.next_value += 1;
        self.next_value
    }

    fn apply(&mut self, step: &Step) -> Result<(), TestCaseError> {
        match (step, self.handles.len()) {
            (Step::Construct, MAX_HANDLES) |
            (Step::CloneHandle(_), 0 | MAX_HANDLES) |
            (Step::DropHandle(_) | Step::Write(..) | Step::Remove(..), 0) => {}
            (Step::Construct, _) => self.construct(),
            (Step::CloneHandle(selector), handles) => self.clone_handle(selector.index(handles)),
            (Step::DropHandle(selector), handles) => self.drop_handle(selector.index(handles)),
            (Step::Write(selector, key), handles) => self.write(selector.index(handles), key)?,
            (Step::Remove(selector, key), handles) => self.remove(selector.index(handles), key)?,
        }
        self.check()
    }

    fn construct(&mut self) {
        let mut subject = Subject::new_with_barrier(self.dispose.clone());
        let mut oracle = BTreeMap::new();
        for key in &MODEL_KEYS[..3] {
            let value = self.value();
            assert!(!subject.set_val_at(key, value), "a fresh subject holds no prior value");
            oracle.insert(key.to_vec(), value);
        }
        let anchor = self.fresh_anchor(true);
        self.handles.push(ModelHandle {
            subject,
            anchor,
            oracle,
        });
    }

    fn clone_handle(&mut self, position: usize) {
        let source = &self.handles[position];
        let copy = ModelHandle {
            subject: source.subject.clone(),
            anchor: source.anchor,
            oracle: source.oracle.clone(),
        };
        self.anchors[copy.anchor].holders += 1;
        self.handles.push(copy);
    }

    fn drop_handle(&mut self, position: usize) {
        let dropped = self.handles.swap_remove(position);
        self.anchors[dropped.anchor].holders -= 1;
    }

    fn write(&mut self, position: usize, key: &Index) -> Result<(), TestCaseError> {
        let value = self.value();
        let path = MODEL_KEYS[key.index(MODEL_KEYS.len())];
        let anchor = self.handles[position].anchor;
        match self.anchors[anchor].holders {
            1 => {}
            _ => {
                self.anchors[anchor].exclusive = false;
                self.anchors[anchor].holders -= 1;
                let copy = self.fresh_anchor(false);
                self.handles[position].anchor = copy;
            }
        }
        let handle = &mut self.handles[position];
        let replaced = handle.subject.set_val_at(path, value);
        let resident = handle.oracle.insert(path.to_vec(), value).is_some();
        prop_assert_eq!(replaced, resident, "a write replaces exactly a resident value");
        Ok(())
    }

    fn remove(&mut self, position: usize, key: &Index) -> Result<(), TestCaseError> {
        let oracle = &self.handles[position].oracle;
        let path = match oracle.len() {
            0 => return Ok(()),
            resident => oracle
                .keys()
                .nth(key.index(resident))
                .cloned()
                .expect("the selector names a resident key"),
        };
        let anchor = self.handles[position].anchor;
        let expected = match (self.anchors[anchor].holders, self.anchors[anchor].exclusive) {
            (1, true) => ExclusiveRemoval::Removed,
            _ => ExclusiveRemoval::Shared,
        };
        let observed = self.handles[position]
            .subject
            .remove_exclusive_endpoint(&path);
        prop_assert_eq!(observed, expected, "the gateway decision follows the provenance model");
        if let ExclusiveRemoval::Shared = observed {
            prop_assert!(
                self.handles[position]
                    .subject
                    .remove_val_at_and_prune(&path)
            );
            self.anchors[anchor].holders -= 1;
            let rebuilt = self.fresh_anchor(true);
            self.handles[position].anchor = rebuilt;
        }
        self.handles[position].oracle.remove(&path);
        Ok(())
    }

    fn check(&self) -> Result<(), TestCaseError> {
        for (position, handle) in self.handles.iter().enumerate() {
            prop_assert_eq!(&entries(&handle.subject), &handle.oracle);
            prop_assert_eq!(
                handle.subject.anchor.exclusive.load(Ordering::Acquire),
                self.anchors[handle.anchor].exclusive,
                "the anchor's exclusivity bit follows the provenance model"
            );
            for other in &self.handles[position + 1..] {
                prop_assert_eq!(
                    handle.subject.anchor_identity() == other.subject.anchor_identity(),
                    handle.anchor == other.anchor,
                    "two handles share an anchor exactly when the model says so"
                );
            }
        }
        Ok(())
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(14_495),
        ..ProptestConfig::default()
    })]

    #[test]
    fn exclusive_removal_matches_a_rebuilt_map_for_every_topology(
        keys in prop::collection::btree_set(topology_key(), 1..24),
        selector in any::<Index>(),
    ) {
        let mut oracle: BTreeMap<Vec<u8>, u64> = keys.into_iter().zip(0u64..).collect();
        let dispose = RecordingDisposer::default();
        let mut subject = subject_with(&oracle, &dispose);
        let (removed, removed_value) = oracle
            .iter()
            .nth(selector.index(oracle.len()))
            .map(|(key, value)| (key.clone(), *value))
            .expect("the selector names a resident key");
        let anchor = subject.anchor_identity();
        let (made, discarded) = barrier_counts();
        prop_assert_eq!(subject.remove_exclusive_endpoint(&removed), ExclusiveRemoval::Removed);
        let (made_after, discarded_after) = barrier_counts();
        oracle.remove(&removed);
        prop_assert_eq!(subject.anchor_identity(), anchor);
        prop_assert_eq!(
            made_after - made,
            discarded_after - discarded,
            "every installed barrier is retired"
        );
        prop_assert_eq!(
            dispose.take(),
            [removed_value],
            "the disposer sees exactly the removed value"
        );
        prop_assert_eq!(&entries(&subject), &oracle);
        prop_assert_eq!(subject.anchor.value_count, oracle.len());
        prop_assert_eq!(subject.iterative_value_count(), oracle.len());
        prop_assert_eq!(subject.with_read_cursor(|cursor| cursor.val_count()), oracle.len());
        prop_assert_eq!(dangling_leaves(&subject), 0);
        let rebuilt = subject_with(&oracle, &RecordingDisposer::default());
        prop_assert_eq!(
            subject.compact_u64_bytes(|value| *value),
            rebuilt.compact_u64_bytes(|value| *value),
            "the in-place removal leaves the image of a map rebuilt from the oracle"
        );
    }

    #[test]
    fn gateway_decisions_follow_the_provenance_model(
        steps in prop::collection::vec(provenance_step(), 1..48),
    ) {
        let mut model = ProvenanceModel::new(steps.len());
        model.check()?;
        for step in &steps {
            model.apply(step)?;
        }
    }
}

#[test]
fn exclusive_removal_clones_no_value_and_header_clones_stay_shared() {
    let clones = Arc::new(AtomicUsize::new(0));
    let dispose = RecordingDisposer::default();
    let mut subject = ProbeSubject::new_with_barrier(dispose.clone());
    for (id, key) in (0u64..).zip(PROBE_KEYS) {
        assert!(!subject.set_val_at(key, CountedClone::new(id, &clones)));
    }
    let anchor = subject.anchor_identity();
    let cloned_before_removal = clones.load(Ordering::SeqCst);
    for key in PROBE_KEYS {
        assert_eq!(
            subject.remove_exclusive_endpoint(key),
            ExclusiveRemoval::Removed,
            "a fresh probe subject removes in place"
        );
    }
    assert_eq!(
        clones.load(Ordering::SeqCst),
        cloned_before_removal,
        "an exclusive removal clones no value"
    );
    assert_eq!(subject.anchor_identity(), anchor);
    assert_eq!(dispose.take(), [0, 1, 2, 3]);
    assert!(subject.is_empty());

    for (id, key) in (10u64..).zip(PROBE_KEYS) {
        assert!(!subject.set_val_at(key, CountedClone::new(id, &clones)));
    }
    let mut successor = subject.clone();
    assert!(!successor.set_val_at([9], CountedClone::new(20, &clones)));
    assert_ne!(subject.anchor_identity(), successor.anchor_identity());
    let cloned_by_the_split = clones.load(Ordering::SeqCst);
    for version in [&mut subject, &mut successor] {
        for key in PROBE_KEYS {
            assert_eq!(
                version.remove_exclusive_endpoint(key),
                ExclusiveRemoval::Shared,
                "a header clone leaves both probe versions shared"
            );
        }
    }
    assert_eq!(
        clones.load(Ordering::SeqCst),
        cloned_by_the_split,
        "a refused removal clones no value"
    );
    assert!(dispose.take().is_empty());
}

#[test]
fn concurrent_header_clones_clear_exclusivity_on_every_version() {
    let mut source = Subject::new_with_barrier(RecordingDisposer::default());
    assert!(!source.set_val_at([1, 2], 1));
    assert!(!source.set_val_at([1, 3], 2));
    let start = Arc::new(Barrier::new(usize::from(WRITERS)));
    let mut writers = Vec::with_capacity(usize::from(WRITERS));
    for writer in 0..WRITERS {
        let mut version = source.clone();
        let start = Arc::clone(&start);
        writers.push(thread::spawn(move || {
            start.wait();
            assert!(!version.set_val_at([2, writer], u64::from(writer) + 10));
            version
        }));
    }
    let mut versions = Vec::with_capacity(usize::from(WRITERS));
    for writer in writers {
        versions.push(writer.join().expect("a concurrent writer completes"));
    }
    let source_anchor = source.anchor_identity();
    for version in &mut versions {
        assert_ne!(
            version.anchor_identity(),
            source_anchor,
            "every concurrent writer forks a header"
        );
        assert_eq!(
            version.remove_exclusive_endpoint(&[1, 2]),
            ExclusiveRemoval::Shared,
            "a concurrent header clone starts its copy shared"
        );
    }
    assert_eq!(
        source.remove_exclusive_endpoint(&[1, 2]),
        ExclusiveRemoval::Shared,
        "concurrent header clones clear the source"
    );
    drop(versions);
    assert_eq!(
        source.remove_exclusive_endpoint(&[1, 2]),
        ExclusiveRemoval::Shared,
        "a cleared source stays shared after every copy is dropped"
    );
    assert!(source.remove_val_at_and_prune([1, 2]));
    assert_eq!(
        source.remove_exclusive_endpoint(&[1, 3]),
        ExclusiveRemoval::Removed,
        "a rebuilt source is exclusive again"
    );
    assert!(source.is_empty());
}

fn small_stack<T: Send + 'static>(operation: impl FnOnce() -> T + Send + 'static) -> T {
    let handle = thread::Builder::new()
        .name("guarded-pathmap-256k".to_owned())
        .stack_size(256 * 1024)
        .spawn(operation)
        .expect("the fixed-stack test thread must spawn");
    match handle.join() {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn deep_key_child(shape: &str) -> ProbeOutcome {
    let executable = std::env::current_exe().expect("the test binary has a path");
    let mut command = Command::new(executable);
    command
        .args(["--ignored", "--exact", DEEP_CHILD, "--nocapture"])
        .env(DEEP_SHAPE, shape)
        .env_remove(ALLOW_EXPECTED_PROBE_CORE);
    run_child_probe(&mut command, Duration::from_secs(120)).expect("the deep key child runs")
}

fn deep_fixture(shape: &str, dispose: &RecordingDisposer) -> DeepFixture {
    let mut subject = Subject::new_with_barrier(dispose.clone());
    let survivor = match shape {
        "root_fork_sibling" => Some((vec![0x3C], 2)),
        "valued_ancestor" => Some((vec![0xC3; DEEP_ANCESTOR_BYTES], 2)),
        "lone_chain" => None,
        "dense_teeth_removed" => return dense_teeth_removed(subject),
        other => panic!("unknown {DEEP_SHAPE} {other:?}"),
    };
    if let Some((path, value)) = &survivor {
        assert!(!subject.set_val_at(path, *value));
    }
    let key = vec![0xC3; DEEP_KEY_BYTES];
    assert!(!subject.set_val_at(&key, 1));
    (subject, key, survivor, 0)
}

fn dense_teeth_removed(mut subject: Subject) -> DeepFixture {
    let key = vec![0u8; DENSE_LEVELS];
    subject.with_write_cursor(|mut cursor| {
        let mut path: Vec<u8> = Vec::with_capacity(DENSE_LEVELS + 1);
        for _ in 0..DENSE_LEVELS {
            for tooth in [1u8, 2] {
                path.push(tooth);
                cursor.move_to_path(&path);
                cursor.set_val(u64::from(tooth) + 1);
                path.pop();
            }
            path.push(0);
        }
        cursor.move_to_path(&path);
        cursor.set_val(1);
        path.clear();
        for _ in 0..DENSE_LEVELS {
            for tooth in [1u8, 2] {
                path.push(tooth);
                cursor.move_to_existing_value(&path);
                assert!(cursor.remove_val_and_prune(), "a dense tooth is removed in place");
                path.pop();
            }
            path.push(0);
        }
    });
    (subject, key, None, 2 * DENSE_LEVELS)
}

#[test]
fn exclusive_deep_key_removal_runs_on_a_256k_stack() {
    for shape in DEEP_SHAPES {
        assert_eq!(deep_key_child(shape), ProbeOutcome::Passed, "exclusive deep key {shape} child");
    }
}

#[test]
#[ignore = "isolated exclusive deep key small-stack child"]
fn exclusive_deep_key_small_stack_child() {
    let Ok(shape) = std::env::var(DEEP_SHAPE) else {
        return;
    };
    assert!(
        matches!(
            prepare_expected_probe_core_dump(),
            ExpectedProbeCorePolicy::SuppressionInstalled |
                ExpectedProbeCorePolicy::ExplicitlyAllowed
        ),
        "the deep key child cannot suppress its core dump"
    );
    let dispose = RecordingDisposer::default();
    let (mut subject, key, survivor, set_up) = deep_fixture(&shape, &dispose);
    assert_eq!(
        dispose.take().len(),
        set_up,
        "the {shape} fixture disposes only its own setup values"
    );
    let (subject, key, removal, (made, discarded)) = small_stack(move || {
        let removal = subject.remove_exclusive_endpoint(&key);
        (subject, key, removal, barrier_counts())
    });
    assert_eq!(removal, ExclusiveRemoval::Removed, "the deep {shape} subject is exclusive");
    assert_eq!(made, discarded, "every barrier installed on the small stack is retired");
    assert_eq!(dispose.take(), [1], "the disposer sees exactly the deep key's value");
    assert!(!subject.path_exists(key), "the deep {shape} key is gone");
    assert_eq!(dangling_leaves(&subject), 0, "the removal leaves no value-free leaf");
    match survivor {
        Some((path, value)) => {
            assert_eq!(subject.get(path), Some(&value), "the {shape} survivor keeps its value");
            assert_eq!(subject.anchor.value_count, 1);
        }
        None => {
            assert!(subject.is_empty(), "removing the deep key empties the map");
            assert_eq!(subject.anchor.value_count, 0);
        }
    }
}
