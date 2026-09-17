use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::time::Instant;

use models::rust::canonical_path::CanonicalTopLevelSegmentCursor;
use models::rust::rholang::match_path::{
    epathmap_match_digest, observed_par_bag_match_summary_experiment,
};

use super::*;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Counts {
    allocations: u64,
    allocated_bytes: u64,
    deallocations: u64,
    deallocated_bytes: u64,
}

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<Counts> = const { Cell::new(Counts {
        allocations: 0, allocated_bytes: 0, deallocations: 0, deallocated_bytes: 0,
    }) };
}

fn count(allocate: Option<usize>, deallocate: Option<usize>) {
    if ACTIVE.try_with(Cell::get).unwrap_or(false) {
        let _ = COUNTS.try_with(|counter| {
            let mut counts = counter.get();
            if let Some(bytes) = allocate {
                counts.allocations += 1;
                counts.allocated_bytes += bytes as u64;
            }
            if let Some(bytes) = deallocate {
                counts.deallocations += 1;
                counts.deallocated_bytes += bytes as u64;
            }
            counter.set(counts);
        });
    }
}

struct CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            count(Some(layout.size()), None);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            count(Some(layout.size()), None);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count(None, Some(layout.size()));
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, old, new_size) };
        if !replacement.is_null() {
            count(Some(new_size), Some(old.size()));
        }
        replacement
    }
}

struct Window;

impl Drop for Window {
    fn drop(&mut self) { ACTIVE.with(|active| active.set(false)); }
}

fn measure<R>(operation: impl FnOnce() -> R) -> (R, Counts, u128) {
    ACTIVE.with(|active| assert!(!active.replace(true), "nested measurement"));
    COUNTS.with(|counts| counts.set(Counts::default()));
    let guard = Window;
    let start = Instant::now();
    let output = black_box(operation());
    let elapsed = start.elapsed().as_nanos();
    drop(guard);
    (output, COUNTS.with(Cell::get), elapsed)
}

#[derive(Clone, Copy, Debug)]
enum Shape {
    Unary,
    Escaped,
    Map,
}

impl Shape {
    fn parse(name: &str) -> Self {
        match name {
            "unary" => Self::Unary,
            "escaped" => Self::Escaped,
            "map" => Self::Map,
            _ => panic!("unknown FIXTURE_SHAPE: {name}"),
        }
    }

    fn map(self, depth: usize) -> EPathMap {
        match self {
            Self::Escaped => EPathMap::new(vec![deep_escaped(depth)], Vec::new(), false, None),
            Self::Map => nested_map_value_chain_from(depth, 0),
            Self::Unary => panic!("the unary control has no EPathMap root"),
        }
    }

    fn par(self, depth: usize) -> Par {
        match self {
            Self::Unary => Par::wrap_unary_elist(depth, gint(0)).unwrap(),
            _ => epathmap_carrier(self.map(depth)),
        }
    }
}

struct CellConfig {
    shape: Shape,
    phase: String,
    depth: usize,
    batch: usize,
    sample: usize,
    role: &'static str,
    prepare_only: bool,
}

fn topology_image_bytes(map: &EPathMap) -> usize {
    let snapshot = map.trie_snapshot();
    let mut cursor = 6;
    let bytes = read_varint(snapshot, &mut cursor);
    assert!(cursor + bytes <= snapshot.len());
    bytes
}

impl CellConfig {
    fn witness(mut self) {
        self.run();
        self.prepare_only = true;
        self.run();
    }

    fn row<R>(
        &self,
        measured: (R, Counts, u128),
        input_bytes: Option<usize>,
        output_bytes: Option<usize>,
    ) -> R {
        let (output, counts, elapsed) = measured;
        println!(
            "FIXTURE_PHASE\t{:?}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.shape,
            self.phase,
            self.depth,
            self.role,
            self.sample,
            self.batch,
            input_bytes.map_or_else(|| "NA".to_owned(), |n| n.to_string()),
            output_bytes.map_or_else(|| "NA".to_owned(), |n| n.to_string()),
            elapsed,
            counts.allocations,
            counts.allocated_bytes,
            counts.deallocations,
            counts.deallocated_bytes,
            std::process::id(),
            self.prepare_only
        );
        output
    }

    fn prepared(&self, input_bytes: Option<usize>) -> bool {
        if self.prepare_only {
            self.row(((), Counts::default(), 0), input_bytes, None);
        }
        self.prepare_only
    }

    fn drop_root<T: Clone>(&self, root: T) {
        assert_eq!(self.batch, 1);
        let retained = (self.phase == "drop_shared").then(|| root.clone());
        if !self.prepared(None) {
            self.row(measure(|| drop(root)), None, None);
        } else {
            drop(root);
        }
        drop(retained);
    }

    fn run(&self) {
        let Self {
            shape,
            depth,
            batch,
            ..
        } = *self;
        match self.phase.as_str() {
            "construct" | "construct_legacy" => {
                assert_eq!(batch, 1);
                let legacy = self.phase == "construct_legacy";
                if legacy {
                    assert!(
                        depth <= 128,
                        "frozen control domain, not a production limit"
                    );
                }
                if self.prepared(None) {
                    return;
                }
                if matches!(shape, Shape::Map) {
                    let measured = measure(|| {
                        if legacy {
                            frozen_resident_nested_map_value_chain_from(depth, 0)
                        } else {
                            nested_map_value_chain_from(depth, 0)
                        }
                    });
                    let bytes = measured.0.trie_snapshot().len();
                    drop(self.row(measured, None, Some(bytes)));
                    return;
                }
                let measured = measure(|| match (shape, legacy) {
                    (Shape::Unary, false) => Par::wrap_unary_elist(depth, gint(0)).unwrap(),
                    (Shape::Unary, true) => {
                        let mut value = gint(0);
                        for _ in 0..depth {
                            value = Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::EListBody(EList {
                                    ps: vec![value],
                                    ..EList::default()
                                })),
                            }]);
                        }
                        value
                    }
                    (Shape::Escaped, false) => deep_escaped(depth),
                    (Shape::Escaped, true) => frozen_resident_deep_escaped(depth),
                    (Shape::Map, _) => unreachable!(),
                });
                let bytes = measured.0.protobuf_bytes().unwrap().len();
                drop(self.row(measured, None, Some(bytes)));
            }
            "outer_map_construct" => {
                assert!(matches!(shape, Shape::Escaped));
                assert_eq!(batch, 1);
                let inner = deep_escaped(depth);
                let bytes = inner.protobuf_bytes().unwrap().len();
                if self.prepared(Some(bytes)) {
                    return;
                }
                let measured = measure(|| EPathMap::new(vec![inner], Vec::new(), false, None));
                let output_bytes = measured.0.trie_snapshot().len();
                drop(self.row(measured, Some(bytes), Some(output_bytes)));
            }
            "snapshot_cold" | "snapshot_warm" => {
                let map = shape.map(depth);
                if self.phase == "snapshot_warm" {
                    black_box(map.trie_snapshot());
                } else {
                    assert_eq!(batch, 1);
                }
                if self.prepared(None) {
                    return;
                }
                let measured = measure(|| {
                    let mut size = 0;
                    for _ in 0..batch {
                        size = black_box(map.trie_snapshot()).len();
                    }
                    size
                });
                let size = measured.0;
                black_box(self.row(measured, Some(size), Some(size)));
            }
            "par_digest_cold" | "par_digest_warm" => {
                let par = shape.par(depth);
                let bytes = par.protobuf_bytes().unwrap().len();
                let warm = self.phase == "par_digest_warm";
                if warm {
                    assert!(observed_par_bag_match_summary_experiment(par.components()).1);
                } else {
                    assert_eq!(batch, 1);
                }
                if self.prepared(Some(bytes)) {
                    return;
                }
                let measured = measure(|| {
                    let mut initialized = 0;
                    let mut digest = [0; 32];
                    for _ in 0..batch {
                        let (summary, new) =
                            observed_par_bag_match_summary_experiment(par.components());
                        initialized += usize::from(new);
                        digest = black_box(summary.digest());
                    }
                    (initialized, digest)
                });
                assert_eq!(measured.0 .0, usize::from(!warm));
                black_box(self.row(measured, Some(bytes), Some(32)));
            }
            "epm_key_digest" => {
                let map = shape.map(depth);
                let bytes = topology_image_bytes(&map);
                if self.prepared(Some(bytes)) {
                    return;
                }
                let measured = measure(|| {
                    let mut digest = [0; 32];
                    for _ in 0..batch {
                        digest = black_box(epathmap_match_digest(&map));
                    }
                    digest
                });
                black_box(self.row(measured, Some(bytes), Some(32)));
            }
            "key_cursor" | "key_decode" => {
                let par = match shape {
                    Shape::Escaped => deep_escaped(depth),
                    _ => shape.par(depth),
                };
                let key = encode_trie_path(&par);
                if self.prepared(Some(key.len())) {
                    return;
                }
                if self.phase == "key_cursor" {
                    let measured = measure(|| {
                        let mut count = 0;
                        for _ in 0..batch {
                            let mut cursor =
                                CanonicalTopLevelSegmentCursor::from_validated_path(&key).unwrap();
                            while let Some(segment) = cursor.next_segment(&key) {
                                black_box(segment);
                                count += 1;
                            }
                        }
                        count
                    });
                    assert!(measured.0 > 0);
                    black_box(self.row(measured, Some(key.len()), None));
                } else {
                    assert_eq!(batch, 1);
                    let measured = measure(|| decode_trie_path(&key).unwrap());
                    assert_eq!(
                        measured.0.protobuf_bytes().unwrap(),
                        par.protobuf_bytes().unwrap()
                    );
                    drop(self.row(
                        measured,
                        Some(key.len()),
                        Some(par.protobuf_bytes().unwrap().len()),
                    ));
                }
            }
            "epm_decode" | "protobuf_decode" | "bincode_decode" | "legacy_decode" => {
                assert_eq!(batch, 1);
                let map = shape.map(depth);
                let snapshot = map.trie_snapshot();
                let bytes = match self.phase.as_str() {
                    "epm_decode" => snapshot.to_vec(),
                    "protobuf_decode" => protobuf_encoder::encode_to_vec(&map),
                    "bincode_decode" => bincode_encoder::encode(&map),
                    "legacy_decode" => {
                        assert!(matches!(shape, Shape::Escaped));
                        legacy_set_field(&deep_escaped(depth))
                    }
                    _ => unreachable!(),
                };
                if self.prepared(Some(bytes.len())) {
                    return;
                }
                if self.phase == "epm_decode" {
                    let measured =
                        measure(|| models::rust::epathmap_trie_codec::decode(&bytes).unwrap());
                    assert_eq!(
                        models::rust::epathmap_trie_codec::encode(&measured.0),
                        snapshot
                    );
                    drop(self.row(measured, Some(bytes.len()), Some(snapshot.len())));
                } else {
                    let measured = measure(|| match self.phase.as_str() {
                        "bincode_decode" => bincode::deserialize::<EPathMap>(&bytes).unwrap(),
                        _ => protobuf_decoder::decode_epath_map(bytes.as_slice()).unwrap(),
                    });
                    assert_eq!(measured.0.trie_snapshot(), snapshot);
                    drop(self.row(measured, Some(bytes.len()), Some(snapshot.len())));
                }
            }
            "lookup_decode_descent" => {
                assert!(matches!(shape, Shape::Map));
                assert_eq!(batch, 1);
                let map = shape.map(depth);
                let key = gstring("key");
                if self.prepared(None) {
                    return;
                }
                let measured = measure(|| {
                    let mut current = map.clone();
                    for level in 1..depth {
                        let child = current.get_map_value(&key).unwrap().unwrap();
                        let (expression, _) = child.first::<Expr>().unwrap().unwrap();
                        let Some(ExprInstance::EPathmapBody(next)) = expression.expr_instance
                        else {
                            panic!("level {level}");
                        };
                        current = next;
                    }
                    current
                });
                assert_eq!(measured.0.get_map_value(&key).unwrap().unwrap(), &gint(0));
                drop(self.row(measured, None, None));
            }
            "drop_unique" | "drop_shared" => match shape {
                Shape::Unary => self.drop_root(shape.par(depth)),
                _ => self.drop_root(shape.map(depth)),
            },
            other => panic!("unknown FIXTURE_PHASE: {other}"),
        }
    }
}

fn parameter(name: &str, default: usize) -> usize {
    std::env::var(name).map_or(default, |value| value.parse().expect(name))
}

#[test]
fn phase_meter_retains_output_and_counts_teardown_separately() {
    let (buffer, allocations, _) = measure(|| black_box(vec![7u8; black_box(513)]));
    assert_eq!(allocations.allocations, 1);
    assert_eq!(allocations.allocated_bytes, 513);
    assert_eq!(allocations.deallocations, 0);
    let (_, teardown, _) = measure(|| drop(buffer));
    assert_eq!(teardown.allocations, 0);
    assert_eq!(teardown.deallocations, 1);
    assert_eq!(teardown.deallocated_bytes, 513);
    let (_, null, _) = measure(|| black_box(7));
    assert_eq!(null, Counts::default());
}

#[test]
fn phase_meter_counts_are_thread_local() {
    let left = std::thread::spawn(|| measure(|| black_box(vec![1u8; black_box(137)])).1);
    let right = std::thread::spawn(|| measure(|| black_box(vec![2u8; black_box(251)])).1);
    assert_eq!(left.join().unwrap().allocated_bytes, 137);
    assert_eq!(right.join().unwrap().allocated_bytes, 251);
}

#[test]
fn topology_denominator_excludes_nested_map_values() {
    let small = Shape::Map.map(2);
    let large = Shape::Map.map(8);
    assert!(large.trie_snapshot().len() > small.trie_snapshot().len());
    assert_eq!(topology_image_bytes(&small), topology_image_bytes(&large));
    assert_eq!(epathmap_match_digest(&small), epathmap_match_digest(&large));
    let (_, unique, _) = measure(|| drop(small));
    assert!(unique.deallocations > 0);
    let retained = large.clone();
    let (_, shared, _) = measure(|| drop(large));
    assert_eq!(shared.deallocations, 0);
    drop(retained);
}

#[test]
fn every_phase_has_a_small_exact_witness() {
    for shape in [Shape::Unary, Shape::Escaped, Shape::Map] {
        for phase in [
            "construct",
            "construct_legacy",
            "par_digest_cold",
            "par_digest_warm",
            "key_cursor",
            "key_decode",
            "drop_unique",
            "drop_shared",
        ] {
            CellConfig {
                shape,
                phase: phase.to_owned(),
                depth: 2,
                batch: 1,
                sample: 0,
                role: "witness",
                prepare_only: false,
            }
            .witness();
        }
    }
    for shape in [Shape::Escaped, Shape::Map] {
        for phase in [
            "snapshot_cold",
            "snapshot_warm",
            "epm_key_digest",
            "epm_decode",
            "protobuf_decode",
            "bincode_decode",
        ] {
            CellConfig {
                shape,
                phase: phase.to_owned(),
                depth: 2,
                batch: 1,
                sample: 0,
                role: "witness",
                prepare_only: false,
            }
            .witness();
        }
    }
    for (shape, phase) in [
        (Shape::Escaped, "outer_map_construct"),
        (Shape::Escaped, "legacy_decode"),
        (Shape::Map, "lookup_decode_descent"),
    ] {
        CellConfig {
            shape,
            phase: phase.to_owned(),
            depth: 2,
            batch: 1,
            sample: 0,
            role: "witness",
            prepare_only: false,
        }
        .witness();
    }
}

#[test]
#[ignore = "experiment 345: explicit shape, phase and resource-capped invocation required"]
fn fixture_phase_measurement() {
    let shape = Shape::parse(&std::env::var("FIXTURE_SHAPE").expect("FIXTURE_SHAPE"));
    let phase = std::env::var("FIXTURE_PHASE").expect("FIXTURE_PHASE");
    let depth = parameter("FIXTURE_DEPTH", 64);
    let batch = parameter("FIXTURE_BATCH", 1);
    let samples = parameter("FIXTURE_SAMPLES", 7);
    let warmups = parameter("FIXTURE_WARMUPS", 3);
    let prepare_only = parameter("FIXTURE_PREPARE_ONLY", 0) == 1;
    assert!(batch > 0 && samples > 0);
    println!("FIXTURE_PHASE_HEADER\tshape\tphase\tdepth\trole\tsample\tbatch\tinput_bytes\toutput_bytes\telapsed_ns\tallocations\tallocated_bytes\tdeallocations\tdeallocated_bytes\tpid\tprepare_only");
    for sample in 0..(1 + warmups + samples) {
        let role = if sample == 0 {
            "pilot"
        } else if sample <= warmups {
            "warmup"
        } else {
            "retained"
        };
        CellConfig {
            shape,
            phase: phase.clone(),
            depth,
            batch,
            sample,
            role,
            prepare_only,
        }
        .run();
    }
}
