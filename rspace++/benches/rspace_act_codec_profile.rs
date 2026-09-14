#[cfg(feature = "act-codec-instrumentation")]
use std::alloc::{GlobalAlloc, Layout, System};
use std::env;
use std::hint::black_box;
#[cfg(feature = "act-codec-instrumentation")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use pathmap::PathMap;
use pathmap::arena_compact::ArenaCompactTree;
use pathmap::zipper::{Zipper, ZipperMoving, ZipperValues};
#[cfg(feature = "act-codec-instrumentation")]
use rspace_plus_plus::rspace::pathmap_codec::CanonicalCompactTrieWorkCounters;
use rspace_plus_plus::rspace::pathmap_codec::{
    CanonicalCompactTrieTopologyCursor, CanonicalCompactTrieTopologyEvent, CompactTrie,
    CompactTrieTopologyEvent,
};
use rspace_plus_plus::rspace::pathmap_mutation::{DisposeValue, NativePathCursor, TeardownBarrier};

#[derive(Clone, Copy)]
struct FixturePolicy;

impl DisposeValue<u64> for FixturePolicy {
    fn dispose(&self, _: u64) {}
}

impl TeardownBarrier<u64> for FixturePolicy {
    fn make_barrier() -> u64 { 0 }
}

type FixtureCursor<'map> = NativePathCursor<'map, 'static, u64, FixturePolicy, FixturePolicy>;
type Plan<'a> = CanonicalCompactTrieTopologyCursor<&'a [u8]>;

#[derive(Clone, Copy, Debug, Default)]
struct Allocations {
    calls: u64,
    bytes: u64,
    frees: u64,
    freed_bytes: u64,
}

#[cfg(feature = "act-codec-instrumentation")]
static ALLOCATION_ACTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "act-codec-instrumentation")]
static ALLOCATION_TOTALS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

#[cfg(feature = "act-codec-instrumentation")]
struct CountingAllocator;

#[cfg(feature = "act-codec-instrumentation")]
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[cfg(feature = "act-codec-instrumentation")]
fn allocation_record(calls: u64, bytes: usize, frees: u64, freed_bytes: usize) {
    if ALLOCATION_ACTIVE.load(Ordering::Relaxed) {
        for (total, increment) in
            ALLOCATION_TOTALS
                .iter()
                .zip([calls, bytes as u64, frees, freed_bytes as u64])
        {
            total.fetch_add(increment, Ordering::Relaxed);
        }
    }
}

#[cfg(feature = "act-codec-instrumentation")]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocation_record(1, layout.size(), 0, 0);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocation_record(1, layout.size(), 0, 0);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        allocation_record(0, 0, 1, layout.size());
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, size: usize) -> *mut u8 {
        allocation_record(1, size, 1, old.size());
        unsafe { System.realloc(pointer, old, size) }
    }
}

fn measure<T>(operation: impl FnOnce() -> T) -> (T, u128, Allocations) {
    #[cfg(feature = "act-codec-instrumentation")]
    {
        for total in &ALLOCATION_TOTALS {
            total.store(0, Ordering::Relaxed);
        }
        assert!(!ALLOCATION_ACTIVE.swap(true, Ordering::SeqCst));
    }
    let started = Instant::now();
    let result = operation();
    let elapsed = started.elapsed().as_nanos();
    #[cfg(feature = "act-codec-instrumentation")]
    let allocations = {
        assert!(ALLOCATION_ACTIVE.swap(false, Ordering::SeqCst));
        Allocations {
            calls: ALLOCATION_TOTALS[0].load(Ordering::Relaxed),
            bytes: ALLOCATION_TOTALS[1].load(Ordering::Relaxed),
            frees: ALLOCATION_TOTALS[2].load(Ordering::Relaxed),
            freed_bytes: ALLOCATION_TOTALS[3].load(Ordering::Relaxed),
        }
    };
    #[cfg(not(feature = "act-codec-instrumentation"))]
    let allocations = Allocations::default();
    (result, elapsed, allocations)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Audit {
    nodes: usize,
    edges: usize,
    leaves: usize,
    values: usize,
    digest: blake3::Hash,
    payload_digest: blake3::Hash,
}

impl Audit {
    fn event_count(&self) -> usize { self.nodes + self.edges + self.leaves }
}

struct AuditBuilder {
    nodes: usize,
    edges: usize,
    leaves: usize,
    values: usize,
    events: blake3::Hasher,
    payloads: blake3::Hasher,
}

impl AuditBuilder {
    fn new() -> Self {
        Self {
            nodes: 0,
            edges: 0,
            leaves: 0,
            values: 0,
            events: blake3::Hasher::new(),
            payloads: blake3::Hasher::new(),
        }
    }

    fn event(&mut self, event: CompactTrieTopologyEvent) {
        match event {
            CompactTrieTopologyEvent::Value(present) => {
                self.nodes += 1;
                self.values += usize::from(present);
                self.events.update(&[0, u8::from(present)]);
            }
            CompactTrieTopologyEvent::Edge(byte) => {
                self.edges += 1;
                self.events.update(&[1, byte]);
            }
            CompactTrieTopologyEvent::Leave => {
                self.leaves += 1;
                self.events.update(&[2]);
            }
        }
    }

    fn payload(&mut self, value: u64) { self.payloads.update(&value.to_le_bytes()); }

    fn finish(self) -> Audit {
        assert_eq!(self.nodes, self.edges + 1);
        assert_eq!(self.leaves, self.nodes);
        Audit {
            nodes: self.nodes,
            edges: self.edges,
            leaves: self.leaves,
            values: self.values,
            digest: self.events.finalize(),
            payload_digest: self.payloads.finalize(),
        }
    }
}

fn erase(event: CanonicalCompactTrieTopologyEvent) -> CompactTrieTopologyEvent {
    match event {
        CanonicalCompactTrieTopologyEvent::Value { value, .. } => {
            CompactTrieTopologyEvent::Value(value.is_some())
        }
        CanonicalCompactTrieTopologyEvent::Edge { byte, .. } => {
            CompactTrieTopologyEvent::Edge(byte)
        }
        CanonicalCompactTrieTopologyEvent::Leave { .. } => CompactTrieTopologyEvent::Leave,
    }
}

fn audit_native(map: &PathMap<u64>) -> Audit {
    let mut audit = AuditBuilder::new();
    let mut cursor = map.read_zipper();
    loop {
        audit.event(CompactTrieTopologyEvent::Value(cursor.is_val()));
        if let Some(value) = cursor.val() {
            audit.payload(*value);
        }
        if let Some(byte) = cursor.child_mask().iter().next() {
            audit.event(CompactTrieTopologyEvent::Edge(byte));
            cursor.descend_to_byte(byte);
            continue;
        }
        loop {
            audit.event(CompactTrieTopologyEvent::Leave);
            if cursor.at_root() {
                return audit.finish();
            }
            let previous = *cursor.path().last().unwrap();
            assert!(cursor.ascend_byte());
            if let Some(byte) = cursor.child_mask().iter().find(|byte| *byte > previous) {
                audit.event(CompactTrieTopologyEvent::Edge(byte));
                cursor.descend_to_byte(byte);
                break;
            }
        }
    }
}

fn fixture(shape: &str, size: usize, valued: bool) -> (Vec<u8>, Audit) {
    let mut map = PathMap::new();
    {
        let mut cursor = FixtureCursor::new(&mut map, &[], &FixturePolicy);
        match shape {
            "deep" => {
                let mut path = Vec::with_capacity(size + 1);
                for ordinal in 0..size {
                    path.push(0x61);
                    cursor.move_to_path(&path);
                    if valued {
                        cursor.set_val(ordinal as u64);
                    } else {
                        cursor.create_path();
                    }
                    path.push(0x62);
                    cursor.move_to_path(&path);
                    cursor.create_path();
                    path.pop();
                }
            }
            "wide" => {
                assert!(size <= 65_536, "wide fixture uses two-byte keys");
                for ordinal in 0..size {
                    cursor.move_to_path(&(ordinal as u16).to_be_bytes());
                    if valued {
                        cursor.set_val(ordinal as u64);
                    } else {
                        cursor.create_path();
                    }
                }
            }
            "compressed" | "reused" | "distinct" => {
                let branches = if shape == "compressed" { 1 } else { 8 };
                for branch in 0..branches {
                    let mut path = Vec::with_capacity(size + 1);
                    if branches != 1 {
                        path.push(branch);
                    }
                    let salt = if shape == "distinct" { branch } else { 0 };
                    path.extend(
                        (0..size).map(|index| (index as u8).wrapping_mul(31).wrapping_add(salt)),
                    );
                    cursor.move_to_path(&path);
                    if valued {
                        cursor.set_val(branch as u64);
                    } else {
                        cursor.create_path();
                    }
                }
            }
            _ => panic!("unknown ACT_PROFILE_SHAPE {shape:?}"),
        }
    }
    let expected = audit_native(&map);
    let bytes = ArenaCompactTree::from_zipper(map.read_zipper(), |value| *value)
        .get_data()
        .to_vec();
    loop {
        let mut cursor = FixtureCursor::new(&mut map, &[], &FixturePolicy);
        while cursor.descend_first_byte() {
            cursor.descend_until();
        }
        if cursor.at_root() {
            cursor.remove_val();
            break;
        }
        if !cursor.is_val() {
            cursor.set_val(0);
        }
        assert!(cursor.remove_val_and_prune());
    }
    (bytes, expected)
}

fn audit_legacy(trie: &CompactTrie<'_>) -> Audit {
    let mut audit = AuditBuilder::new();
    trie.visit_topology(|event| {
        audit.event(event);
        Ok::<_, ()>(())
    })
    .unwrap();
    trie.visit(|_, value| {
        if let Some(value) = value {
            audit.payload(value);
        }
        Ok::<_, ()>(())
    })
    .unwrap();
    audit.finish()
}

fn audit_plan(cursor: &mut Plan<'_>) -> Audit {
    cursor.rewind();
    let mut audit = AuditBuilder::new();
    while let Some(event) = cursor.next_event() {
        if let CanonicalCompactTrieTopologyEvent::Value {
            value: Some(value), ..
        } = event
        {
            audit.payload(value);
        }
        audit.event(erase(event));
    }
    audit.finish()
}

fn walk_legacy(trie: &CompactTrie<'_>) -> usize {
    let mut count = 0;
    trie.visit_topology(|event| {
        black_box(event);
        count += 1;
        Ok::<_, ()>(())
    })
    .unwrap();
    count
}

fn walk_plan(cursor: &mut Plan<'_>) -> usize {
    let mut count = 0;
    while let Some(event) = cursor.next_event() {
        black_box(erase(event));
        count += 1;
    }
    count
}

macro_rules! counter_fields {
    ($visitor:ident) => {
        $visitor!(
            semantic_node_parses,
            wrapper_node_parses,
            node_reparses,
            sparse_branch_bytes,
            dense_mask_bytes,
            line_hash_bytes,
            line_hash_collisions,
            line_equality_bytes,
            validation_source_byte_reads,
            validation_source_copy_bytes,
            validation_source_contiguous_bytes,
            validation_source_equality_bytes,
            event_metadata_source_reads,
            event_line_source_reads,
            plan_records,
            plan_bytes,
            plan_record_bytes,
            validated_topology_state_bytes,
            topology_cursor_bytes,
            validation_branch_entries_peak,
            validation_branch_bytes_peak,
            validation_branch_capacity_bytes_peak,
            validation_branch_allocations,
            validation_frame_bytes_peak,
            validation_frame_allocations,
            validation_record_capacity_bytes_peak,
            validation_record_allocations,
            validation_seen_bitmap_bytes,
            max_validation_depth,
            max_event_depth
        )
    };
}

macro_rules! names { ($($field:ident),*) => { &[$(stringify!($field)),*] }; }
const COUNTER_NAMES: &[&str] = counter_fields!(names);

#[cfg(feature = "act-codec-instrumentation")]
type Work = Option<CanonicalCompactTrieWorkCounters>;
#[cfg(not(feature = "act-codec-instrumentation"))]
type Work = Option<()>;

fn work(cursor: &Plan<'_>) -> Work {
    #[cfg(feature = "act-codec-instrumentation")]
    {
        Some(cursor.work_counters())
    }
    #[cfg(not(feature = "act-codec-instrumentation"))]
    {
        let _ = cursor;
        None
    }
}

fn counter_columns(snapshot: Work) -> Vec<String> {
    #[cfg(feature = "act-codec-instrumentation")]
    if let Some(counters) = snapshot {
        macro_rules! columns { ($($field:ident),*) => { vec![$(counters.$field.to_string()),*] }; }
        return counter_fields!(columns);
    }
    let _ = snapshot;
    vec!["NA".to_owned(); COUNTER_NAMES.len()]
}

fn event_read_delta(before: Work, after: Work) -> String {
    #[cfg(feature = "act-codec-instrumentation")]
    if let Some(after) = after {
        return after
            .event_line_source_reads
            .checked_sub(before.map_or(0, |before| before.event_line_source_reads))
            .expect("event counters are monotone")
            .to_string();
    }
    let _ = (before, after);
    "NA".to_owned()
}

struct Sample {
    duration: u128,
    allocations: Allocations,
    before: Work,
    after: Work,
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn main() {
    let shape = required("ACT_PROFILE_SHAPE");
    let size = required("ACT_PROFILE_SIZE")
        .parse::<usize>()
        .expect("positive fixture size");
    let arm = required("ACT_PROFILE_ARM");
    let phase = required("ACT_PROFILE_PHASE");
    let values = env::var("ACT_PROFILE_VALUES").unwrap_or_else(|_| "valued".to_owned());
    let iterations = env::var("ACT_PROFILE_ITERATIONS")
        .unwrap_or_else(|_| "30".to_owned())
        .parse::<usize>()
        .expect("positive iteration count");
    let warmups = env::var("ACT_PROFILE_WARMUPS")
        .unwrap_or_else(|_| "5".to_owned())
        .parse::<usize>()
        .expect("nonnegative warmup count");
    let total = warmups
        .checked_add(iterations)
        .expect("iteration count fits usize");
    assert!(size > 0 && iterations > 0);
    assert!(matches!(arm.as_str(), "legacy" | "plan"));
    assert!(matches!(phase.as_str(), "admission" | "cold_walk" | "warm_walk" | "rewind"));
    assert!(matches!(values.as_str(), "valued" | "valueless"));
    assert!(
        arm != "legacy" || phase != "rewind",
        "legacy exposes no equivalent rewind-only operation"
    );
    let (bytes, expected) = fixture(&shape, size, values == "valued");
    let mut samples = Vec::with_capacity(iterations);
    let mut warm_legacy = None;
    let mut warm_plan = None;
    if matches!(phase.as_str(), "warm_walk" | "rewind") {
        if arm == "legacy" {
            let trie = CompactTrie::parse_canonical(&bytes).unwrap();
            assert_eq!(audit_legacy(&trie), expected);
            warm_legacy = Some(trie);
        } else {
            let mut cursor = Plan::new(&bytes).unwrap();
            assert_eq!(audit_plan(&mut cursor), expected);
            warm_plan = Some(cursor);
        }
    }
    for iteration in 0..total {
        let sample = if arm == "legacy" {
            if phase == "warm_walk" {
                let trie = warm_legacy.as_ref().unwrap();
                let (count, duration, allocations) = measure(|| walk_legacy(black_box(trie)));
                assert_eq!(count, expected.event_count());
                assert_eq!(audit_legacy(trie), expected);
                Sample {
                    duration,
                    allocations,
                    before: None,
                    after: None,
                }
            } else {
                let ((trie, count), duration, allocations) = measure(|| {
                    let trie = CompactTrie::parse_canonical(black_box(&bytes)).unwrap();
                    let count = if phase == "cold_walk" {
                        walk_legacy(&trie)
                    } else {
                        0
                    };
                    (trie, count)
                });
                if phase == "cold_walk" {
                    assert_eq!(count, expected.event_count());
                }
                assert_eq!(audit_legacy(&trie), expected);
                Sample {
                    duration,
                    allocations,
                    before: None,
                    after: None,
                }
            }
        } else if let Some(cursor) = warm_plan.as_mut() {
            let before = work(cursor);
            let (count, duration, allocations) = measure(|| {
                cursor.rewind();
                if phase == "warm_walk" {
                    walk_plan(black_box(&mut *cursor))
                } else {
                    black_box(&mut *cursor);
                    0
                }
            });
            let after = work(cursor);
            if phase == "warm_walk" {
                assert_eq!(count, expected.event_count());
            }
            assert_eq!(audit_plan(cursor), expected);
            Sample {
                duration,
                allocations,
                before,
                after,
            }
        } else {
            let ((mut cursor, count), duration, allocations) = measure(|| {
                let mut cursor = Plan::new(black_box(&bytes)).unwrap();
                let count = if phase == "cold_walk" {
                    walk_plan(&mut cursor)
                } else {
                    0
                };
                (cursor, count)
            });
            let after = work(&cursor);
            if phase == "cold_walk" {
                assert_eq!(count, expected.event_count());
            }
            assert_eq!(audit_plan(&mut cursor), expected);
            Sample {
                duration,
                allocations,
                before: None,
                after,
            }
        };
        #[cfg(feature = "act-codec-instrumentation")]
        if let Some(counters) = sample.after {
            assert_eq!(counters.semantic_node_parses, counters.plan_records);
            assert_eq!(counters.node_reparses, 0);
            assert_eq!(counters.event_metadata_source_reads, 0);
        }
        if iteration >= warmups {
            samples.push(sample);
        }
    }
    let mut header = vec![
        "schema",
        "experiment_id",
        "replicate",
        "shape",
        "size",
        "values",
        "arm",
        "role",
        "phase",
        "warmups",
        "iterations",
        "instrumented",
        "input_bytes",
        "rss_scope",
        "nodes",
        "edges",
        "leaves",
        "value_count",
        "event_digest",
        "payload_digest",
        "duration_ns",
        "allocation_calls",
        "allocation_bytes",
        "deallocation_calls",
        "deallocation_bytes",
        "counter_scope",
        "event_line_reads_delta",
    ];
    header.extend_from_slice(COUNTER_NAMES);
    println!("{}", header.join("\t"));
    for (replicate, sample) in samples.into_iter().enumerate() {
        let mut row = vec![
            "act-codec-profile-v1".to_owned(),
            "343".to_owned(),
            replicate.to_string(),
            shape.clone(),
            size.to_string(),
            values.clone(),
            arm.clone(),
            if arm == "legacy" {
                "control"
            } else {
                "treatment"
            }
            .to_owned(),
            phase.clone(),
            warmups.to_string(),
            iterations.to_string(),
            cfg!(feature = "act-codec-instrumentation").to_string(),
            bytes.len().to_string(),
            "process_includes_fixture_audit".to_owned(),
            expected.nodes.to_string(),
            expected.edges.to_string(),
            expected.leaves.to_string(),
            expected.values.to_string(),
            expected.digest.to_string(),
            expected.payload_digest.to_string(),
            sample.duration.to_string(),
        ];
        if cfg!(feature = "act-codec-instrumentation") {
            row.extend([
                sample.allocations.calls.to_string(),
                sample.allocations.bytes.to_string(),
                sample.allocations.frees.to_string(),
                sample.allocations.freed_bytes.to_string(),
            ]);
        } else {
            row.extend(["NA".to_owned(), "NA".to_owned(), "NA".to_owned(), "NA".to_owned()]);
        }
        row.push("state_after_measurement".to_owned());
        row.push(event_read_delta(sample.before, sample.after));
        row.extend(counter_columns(sample.after));
        assert_eq!(row.len(), header.len());
        println!("{}", row.join("\t"));
    }
}
