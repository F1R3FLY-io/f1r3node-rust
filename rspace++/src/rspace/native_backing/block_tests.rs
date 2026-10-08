//! D-E1 (DR-108): tests of the block-mode wrappers of rspace++. An
//! independent oracle states the block rules (DR-92, DR-94, DR-108) for the
//! records that the walker sites of rspace++ copy, inspect and release. The
//! per-level wrappers, which the sites used before D-E1, are the reference
//! charges of the comparison with the per-level walker.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;
use std::sync::Arc;

use proptest::prelude::*;
use shared::rust::clone_backing::{
    BLOCK_ENTRY_BACKING, BLOCK_ENTRY_SCANNED, BLOCK_FIELD_SCANNED, BLOCK_SHARED_HEADER_SCANNED,
    BackingMeter,
};
use shared::rust::collection_backing::tree_backing;

use super::*;
use crate::rspace::history::native_reader::measure_allocations;

/// The reservations of `action` on an unlimited meter, in call order.
fn calls(action: impl FnOnce(&dyn SourceMeter) -> Result<(), RSpaceError>) -> Vec<[usize; 3]> {
    let calls = RefCell::new(Vec::new());
    let meter = |operations: usize, scanned: usize, backing: usize| {
        calls.borrow_mut().push([operations, scanned, backing]);
        Ok::<(), RSpaceError>(())
    };
    action(&meter).expect("an unlimited meter");
    calls.into_inner()
}

/// The reservations of a walk of the shared walker on an unlimited meter.
fn walk_calls(
    action: impl FnOnce(&dyn BackingMeter) -> Result<(), BackingError>,
) -> Vec<[usize; 3]> {
    let calls = RefCell::new(Vec::new());
    let meter = |operations: usize, scanned: usize, backing: usize| {
        calls.borrow_mut().push([operations, scanned, backing]);
        Ok::<(), BackingError>(())
    };
    action(&meter).expect("an unlimited meter");
    calls.into_inner()
}

fn total(calls: &[[usize; 3]]) -> [usize; 3] {
    calls
        .iter()
        .fold([0; 3], |[o, s, b], [operations, scanned, backing]| {
            [o + operations, s + scanned, b + backing]
        })
}

fn usage(action: impl FnOnce(&dyn SourceMeter) -> Result<(), RSpaceError>) -> [usize; 3] {
    total(&calls(action))
}

fn sum(first: [usize; 3], second: [usize; 3]) -> [usize; 3] {
    [first[0] + second[0], first[1] + second[1], first[2] + second[2]]
}

/// A value as the block rules see it (DR-92, DR-94).
#[derive(Clone, Debug)]
enum Shape {
    /// An inline field, or a container that the walk reports empty. Its
    /// bytes lie in the enclosing block.
    Field,
    /// A worklist entry. When the drain pops it, its children reserve
    /// `operations`, read the heap blocks `blocks` (bytes, visited), and push
    /// `pushes` in order.
    Entry {
        operations: usize,
        blocks: Vec<(usize, bool)>,
        pushes: Vec<Shape>,
    },
}

/// A vector of bytes or a string: its elements are inline, so the walk does
/// not visit its block.
fn opaque(length: usize, operations: usize) -> Shape {
    if length == 0 {
        return Shape::Field;
    }
    Shape::Entry {
        operations,
        blocks: vec![(length, false)],
        pushes: Vec::new(),
    }
}

fn bytes(value: &[u8]) -> Shape { opaque(value.len(), 2 * value.len()) }

fn text(value: &str) -> Shape { opaque(value.len(), 0) }

/// A hash is a struct around its byte vector, so the walk never reports it
/// empty.
fn hash(value: &Blake2b256Hash) -> Shape {
    Shape::Entry {
        operations: 0,
        blocks: Vec::new(),
        pushes: vec![bytes(&value.0)],
    }
}

/// A vector whose elements the walk visits: two operations per element, its
/// block, and one push per element.
fn vector<T>(items: &[T], shape: impl Fn(&T) -> Shape) -> Shape {
    if items.is_empty() {
        return Shape::Field;
    }
    Shape::Entry {
        operations: 2 * items.len(),
        blocks: vec![(std::mem::size_of_val(items), true)],
        pushes: items.iter().map(shape).collect(),
    }
}

fn peeks(value: &BTreeSet<i32>) -> Shape {
    if value.is_empty() {
        return Shape::Field;
    }
    let (operations, bytes) = tree_backing::<i32, ()>(value.len()).expect("a small set");
    Shape::Entry {
        operations,
        blocks: vec![(bytes, true)],
        pushes: vec![Shape::Field; value.len()],
    }
}

fn counters(value: &BTreeMap<Produce, i32>) -> Shape {
    if value.is_empty() {
        return Shape::Field;
    }
    let (operations, bytes) = tree_backing::<Produce, i32>(value.len()).expect("a small map");
    let mut pushes = Vec::with_capacity(2 * value.len());
    for key in value.keys() {
        pushes.push(produce(key));
        pushes.push(Shape::Field);
    }
    Shape::Entry {
        operations,
        blocks: vec![(bytes, true)],
        pushes,
    }
}

fn record(pushes: Vec<Shape>) -> Shape {
    Shape::Entry {
        operations: 0,
        blocks: Vec::new(),
        pushes,
    }
}

fn produce(value: &Produce) -> Shape {
    record(vec![
        hash(&value.channel_hash),
        hash(&value.hash),
        Shape::Field,
        Shape::Field,
        vector(&value.output_value, |output| bytes(output)),
        Shape::Field,
    ])
}

fn consume(value: &Consume) -> Shape {
    record(vec![vector(&value.channel_hashes, hash), hash(&value.hash), Shape::Field])
}

fn comm(value: &COMM) -> Shape {
    record(vec![
        consume(&value.consume),
        vector(&value.produces, produce),
        peeks(&value.peeks),
        counters(&value.times_repeated),
    ])
}

fn io_event(value: &IOEvent) -> Shape {
    record(vec![match value {
        IOEvent::Produce(source) => produce(source),
        IOEvent::Consume(source) => consume(source),
    }])
}

fn event(value: &Event) -> Shape {
    record(vec![match value {
        Event::Comm(source) => comm(source),
        Event::IoEvent(source) => io_event(source),
    }])
}

fn datum(value: &Datum<String>) -> Shape {
    record(vec![text(&value.a), Shape::Field, produce(&value.source)])
}

fn waiting(value: &WaitingContinuation<String, String>) -> Shape {
    record(vec![
        vector(&value.patterns, |pattern| text(pattern)),
        text(&value.continuation),
        Shape::Field,
        peeks(&value.peeks),
        consume(&value.source),
    ])
}

fn joins(value: &[Vec<String>]) -> Shape {
    vector(value, |join| vector(join, |channel| text(channel)))
}

/// A heap block of `bytes`: an inspection reads it once for the consumer
/// and once more if the walk visits its elements. A copy also writes it,
/// and it allocates it unless the block is a root read through a reference.
fn block(totals: &mut [usize; 3], bytes: usize, visited: bool, allocated: bool, copy: bool) {
    let reads = usize::from(visited) + 1 + usize::from(copy);
    *totals = sum(*totals, [0, bytes * reads, if copy && allocated { bytes } else { 0 }]);
}

/// The charge of one push of `shape` and of the subtree below it. A pushed
/// entry reserves worklist backing unless it reuses the slot of the entry
/// that the drain popped last (DR-108): the first entry that a popped
/// entry's children push reuses it. Fields do not take the slot.
fn charge(shape: &Shape, copy: bool, reused: bool, totals: &mut [usize; 3]) {
    match shape {
        Shape::Field => *totals = sum(*totals, [3, BLOCK_FIELD_SCANNED, 0]),
        Shape::Entry {
            operations,
            blocks,
            pushes,
        } => {
            let backing = if reused { 0 } else { BLOCK_ENTRY_BACKING };
            *totals = sum(*totals, [3 + operations, BLOCK_ENTRY_SCANNED, backing]);
            for (bytes, visited) in blocks {
                block(totals, *bytes, *visited, true, copy);
            }
            let mut taken = false;
            for push in pushes {
                let entry = matches!(push, Shape::Entry { .. });
                charge(push, copy, entry && !taken, totals);
                taken |= entry;
            }
        }
    }
}

/// The oracle charge of a block walk of a root with `size` inline bytes.
/// The root is read through a reference: its block is visited and not
/// allocated, and its push reserves the backing of the first slot.
fn oracle(shape: &Shape, size: usize, copy: bool) -> [usize; 3] {
    let mut totals = [0; 3];
    block(&mut totals, size, true, false, copy);
    charge(shape, copy, false, &mut totals);
    totals
}

/// The oracle charge of a block walk of a slice of `count` values of
/// `size` inline bytes. The slice is copied into a new vector, so its block
/// is visited and allocated. Every element is pushed before the first pop,
/// so no element reuses a slot.
fn slice_oracle(shape: &Shape, count: usize, size: usize, copy: bool) -> [usize; 3] {
    let mut totals = [2 * count, 0, 0];
    block(&mut totals, count * size, true, true, copy);
    for _ in 0..count {
        charge(shape, copy, false, &mut totals);
    }
    totals
}

/// One record that the walker sites of rspace++ copy, inspect or release.
#[derive(Clone, Debug)]
enum Record {
    Datum(Datum<String>),
    Waiting(WaitingContinuation<String, String>),
    Produce(Produce),
    Consume(Consume),
    Comm(COMM),
    Log(Vec<Event>),
    Counters(BTreeMap<Produce, i32>),
    Joins(Vec<Vec<String>>),
}

/// Runs `$body` with `$value` bound to the record's value and `$shape`
/// bound to its oracle shape.
macro_rules! with_record {
    ($record:expr, | $value:ident, $shape:ident | $body:block) => {
        match $record {
            Record::Datum($value) => {
                let $shape = datum($value);
                $body
            }
            Record::Waiting($value) => {
                let $shape = waiting($value);
                $body
            }
            Record::Produce($value) => {
                let $shape = produce($value);
                $body
            }
            Record::Consume($value) => {
                let $shape = consume($value);
                $body
            }
            Record::Comm($value) => {
                let $shape = comm($value);
                $body
            }
            Record::Log($value) => {
                let $shape = vector($value, event);
                $body
            }
            Record::Counters($value) => {
                let $shape = counters($value);
                $body
            }
            Record::Joins($value) => {
                let $shape = joins($value);
                $body
            }
        }
    };
}

/// The bytes of the B-tree nodes of a record, which a block copy and
/// cleanup reads once more than a per-level one.
fn tree_node_bytes(record: &Record) -> usize {
    fn set_nodes(value: &BTreeSet<i32>) -> usize {
        tree_backing::<i32, ()>(value.len()).expect("a small set").1
    }
    fn map_nodes(value: &BTreeMap<Produce, i32>) -> usize {
        tree_backing::<Produce, i32>(value.len())
            .expect("a small map")
            .1
    }
    fn comm_nodes(value: &COMM) -> usize {
        set_nodes(&value.peeks) + map_nodes(&value.times_repeated)
    }
    match record {
        Record::Waiting(value) => set_nodes(&value.peeks),
        Record::Comm(value) => comm_nodes(value),
        Record::Log(events) => events
            .iter()
            .map(|event| match event {
                Event::Comm(value) => comm_nodes(value),
                Event::IoEvent(_) => 0,
            })
            .sum(),
        Record::Counters(value) => map_nodes(value),
        Record::Datum(_) | Record::Produce(_) | Record::Consume(_) | Record::Joins(_) => 0,
    }
}

fn label() -> impl Strategy<Value = String> { "[a-z]{0,12}" }

fn produce_strategy() -> impl Strategy<Value = Produce> {
    (
        label(),
        label(),
        any::<bool>(),
        prop::collection::vec(prop::collection::vec(any::<u8>(), 0..24), 0..3),
        any::<bool>(),
    )
        .prop_map(|(channel, data, persistent, output_value, failed)| {
            let mut source = Produce::create(&channel, &data, persistent);
            source.output_value = output_value;
            source.failed = failed;
            source
        })
}

fn consume_strategy() -> impl Strategy<Value = Consume> {
    (
        prop::collection::vec(label(), 0..3),
        prop::collection::vec(label(), 0..3),
        label(),
        any::<bool>(),
    )
        .prop_map(|(channels, patterns, body, persistent)| {
            Consume::create(&channels, &patterns, &body, persistent)
        })
}

fn peeks_strategy() -> impl Strategy<Value = BTreeSet<i32>> {
    prop::collection::btree_set(0..8_i32, 0..3)
}

fn comm_strategy() -> impl Strategy<Value = COMM> {
    (
        consume_strategy(),
        prop::collection::vec(produce_strategy(), 0..3),
        peeks_strategy(),
        prop::collection::btree_map(produce_strategy(), 1..4_i32, 0..3),
    )
        .prop_map(|(consume, produces, peeks, times_repeated)| COMM {
            consume,
            produces,
            peeks,
            times_repeated,
        })
}

fn event_strategy() -> impl Strategy<Value = Event> {
    prop_oneof![
        comm_strategy().prop_map(Event::Comm),
        produce_strategy().prop_map(|source| Event::IoEvent(IOEvent::Produce(source))),
        consume_strategy().prop_map(|source| Event::IoEvent(IOEvent::Consume(source))),
    ]
}

fn record_strategy() -> impl Strategy<Value = Record> {
    prop_oneof![
        (label(), any::<bool>(), produce_strategy())
            .prop_map(|(a, persist, source)| Record::Datum(Datum { a, persist, source })),
        (
            prop::collection::vec(label(), 0..4),
            label(),
            any::<bool>(),
            peeks_strategy(),
            consume_strategy(),
        )
            .prop_map(|(patterns, continuation, persist, peeks, source)| {
                Record::Waiting(WaitingContinuation {
                    patterns,
                    continuation,
                    persist,
                    peeks,
                    source,
                })
            }),
        produce_strategy().prop_map(Record::Produce),
        consume_strategy().prop_map(Record::Consume),
        comm_strategy().prop_map(Record::Comm),
        prop::collection::vec(event_strategy(), 0..4).prop_map(Record::Log),
        prop::collection::btree_map(produce_strategy(), 0..5_i32, 0..4).prop_map(Record::Counters),
        prop::collection::vec(prop::collection::vec(label(), 0..3), 0..4).prop_map(Record::Joins),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// D-E1 (DR-108): for every record that the walker sites of rspace++
    /// reach, the block walks of the shared walker equal an independent
    /// statement of the block rules, for one value and for a slice of
    /// values, for an inspection and for a copy.
    #[test]
    fn rspace_records_block_charges_match_independent_oracle(
        record in record_strategy(),
        count in 0_usize..3,
    ) {
        with_record!(&record, |value, shape| {
            let size = std::mem::size_of_val(value);
            prop_assert_eq!(
                total(&walk_calls(|meter| clone_backing::inspect_blocks(value, meter))),
                oracle(&shape, size, false)
            );
            prop_assert_eq!(
                total(&walk_calls(|meter| clone_backing::reserve_blocks(value, meter))),
                oracle(&shape, size, true)
            );
            let values = vec![value.clone(); count];
            prop_assert_eq!(
                total(&walk_calls(|meter| clone_backing::inspect_blocks_slice(&values, meter))),
                slice_oracle(&shape, count, size, false)
            );
            prop_assert_eq!(
                total(&walk_calls(|meter| clone_backing::reserve_blocks_slice(&values, meter))),
                slice_oracle(&shape, count, size, true)
            );
        });
    }

    /// D-E1 (DR-108): each block wrapper of rspace++ makes exactly the
    /// reservations of the shared block walks that it names, in the same
    /// order. A copy and cleanup is the block copy followed by the block
    /// inspection of the same value.
    #[test]
    fn block_wrappers_charge_the_shared_block_walks(
        record in record_strategy(),
        count in 0_usize..3,
    ) {
        with_record!(&record, |value, shape| {
            let _ = shape;
            let copy = walk_calls(|meter| clone_backing::reserve_blocks(value, meter));
            let inspection = walk_calls(|meter| clone_backing::inspect_blocks(value, meter));
            let copy_and_cleanup = [copy.clone(), inspection.clone()].concat();
            prop_assert_eq!(
                calls(|meter| reserve_blocks_copy_and_cleanup(value, meter)),
                copy_and_cleanup.clone()
            );
            prop_assert_eq!(
                walk_calls(|meter| clone_backing::reserve_blocks_copy_and_cleanup(value, meter)),
                copy_and_cleanup
            );
            prop_assert_eq!(calls(|meter| inspect_blocks(value, meter)), inspection.clone());
            prop_assert_eq!(calls(|meter| reserve_blocks_cleanup(value, meter)), inspection);
            let values = vec![value.clone(); count];
            let slice_copy = walk_calls(|meter| clone_backing::reserve_blocks_slice(&values, meter));
            let slice_inspection =
                walk_calls(|meter| clone_backing::inspect_blocks_slice(&values, meter));
            prop_assert_eq!(
                calls(|meter| reserve_blocks_slice_copy_and_cleanup(&values, meter)),
                [slice_copy, slice_inspection.clone()].concat()
            );
            prop_assert_eq!(
                calls(|meter| inspect_blocks_slice(&values, meter)),
                slice_inspection
            );
            let shared: Vec<Arc<_>> = (0..count).map(|_| Arc::new(value.clone())).collect();
            prop_assert_eq!(
                calls(|meter| reserve_shared_blocks_copy_and_cleanup(&shared, meter)),
                [
                    walk_calls(|meter| clone_backing::reserve_blocks(&shared, meter)),
                    walk_calls(|meter| {
                        clone_backing::inspect_shared_pointers_blocks(&shared, meter)
                    }),
                ]
                .concat()
            );
            let pointer = Arc::new(value.clone());
            prop_assert_eq!(
                calls(|meter| reserve_shared_blocks_cleanup(&pointer, meter)),
                walk_calls(|meter| clone_backing::inspect_shared_pointers_blocks(&pointer, meter))
            );
        });
    }

    /// D-E1 (DR-108): for the records of RSpace, a block inspection never
    /// charges more VerificationBytes than the per-level inspection that the
    /// site used before, for one value and for a slice of values. A block
    /// copy and cleanup reads each visited heap block five times, against
    /// four times in per-level mode, and its root pays two entry constants.
    /// So its charge exceeds the per-level charge by at most the bytes of
    /// the B-tree nodes, which the per-level walk also charged whole, and
    /// for one value by at most the root's two entry constants less one
    /// more read of the root. A vector's block holds only its elements, and
    /// each element saves more than its extra read.
    #[test]
    fn block_charge_le_per_level_for_rspace_records(
        record in record_strategy(),
        count in 0_usize..3,
    ) {
        let nodes = tree_node_bytes(&record);
        with_record!(&record, |value, shape| {
            let _ = shape;
            let scanned = |action: &dyn Fn(&dyn SourceMeter) -> Result<(), RSpaceError>| {
                usage(action)[1]
            };
            let root = (2 * BLOCK_ENTRY_SCANNED).saturating_sub(std::mem::size_of_val(value));
            prop_assert!(
                scanned(&|meter| inspect_blocks(value, meter))
                    <= scanned(&|meter| inspect(value, meter))
            );
            prop_assert!(
                scanned(&|meter| reserve_blocks_copy_and_cleanup(value, meter))
                    <= scanned(&|meter| reserve_copy_and_cleanup(value, meter)) + nodes + root
            );
            let values = vec![value.clone(); count];
            prop_assert!(
                scanned(&|meter| inspect_blocks_slice(&values, meter))
                    <= scanned(&|meter| inspect_slice(&values, meter))
            );
            prop_assert!(
                scanned(&|meter| reserve_blocks_slice_copy_and_cleanup(&values, meter))
                    <= scanned(&|meter| reserve_slice_copy_and_cleanup(&values, meter))
                        + count * nodes
            );
        });
    }

    /// D-E1 (DR-108): the backing that a block copy and cleanup reserves
    /// covers every allocation of the two walks, of the clone and of its
    /// release. The backing of an inspection covers the allocations of its
    /// walk.
    #[test]
    fn block_walks_cover_copy_and_worklist_allocations(record in record_strategy()) {
        with_record!(&record, |value, shape| {
            let _ = shape;
            let reserved = Cell::new(0_usize);
            let meter = |_: usize, _: usize, backing: usize| {
                reserved.set(reserved.get() + backing);
                Ok::<(), RSpaceError>(())
            };
            let ((), allocated) = measure_allocations(|| {
                reserve_blocks_copy_and_cleanup(value, &meter).expect("an unlimited meter");
                drop(value.clone());
            });
            prop_assert!(allocated <= reserved.get(), "{} > {}", allocated, reserved.get());
            reserved.set(0);
            let ((), allocated) = measure_allocations(|| {
                inspect_blocks(value, &meter).expect("an unlimited meter");
            });
            prop_assert!(allocated <= reserved.get(), "{} > {}", allocated, reserved.get());
        });
    }
}

/// Negative control for `block_charge_le_per_level_for_rspace_records`: the
/// comparison does not hold for every value. A bare scalar root costs one
/// read of its byte and a field word in block mode, against three reads of
/// its byte in per-level mode.
#[test]
fn block_charge_exceeds_per_level_for_bare_scalars() {
    let value = 7_u8;
    assert_eq!(usage(|meter| inspect(&value, meter)), [3, 3, 0]);
    assert_eq!(usage(|meter| inspect_blocks(&value, meter)), [3, 1 + BLOCK_FIELD_SCANNED, 0]);
    assert!(
        usage(|meter| inspect_blocks(&value, meter))[1] > usage(|meter| inspect(&value, meter))[1]
    );
}

/// The VerificationBytes of a block copy and cleanup of `value` less those
/// of the per-level copy and cleanup.
fn copy_and_cleanup_excess<T: CloneBacking>(value: &T) -> i64 {
    let block = usage(|meter| reserve_blocks_copy_and_cleanup(value, meter))[1];
    let per_level = usage(|meter| reserve_copy_and_cleanup(value, meter))[1];
    i64::try_from(block).expect("a small charge") -
        i64::try_from(per_level).expect("a small charge")
}

fn signed(bytes: usize) -> i64 { i64::try_from(bytes).expect("a small size") }

/// Negative control for `block_charge_le_per_level_for_rspace_records`: a
/// block copy and cleanup of a sparse B-tree exceeds the per-level one. The
/// node of a one-entry map has eleven slots, and the block walks read the
/// whole node five times against four. The rest of the map saves less than
/// that extra read. Each term is the block charge less the per-level charge
/// of one part, over the copy and the cleanup: a part of inline size `s`
/// costs two entry constants or two field words in block mode, and two
/// pushes of three reads of `s` in per-level mode.
#[test]
fn block_copy_exceeds_per_level_for_sparse_tree_nodes() {
    let key = Produce::create(&"channel".to_owned(), &"value".to_owned(), false);
    assert!(key.output_value.is_empty() && key.hash.0.len() == 32);
    let map = BTreeMap::from([(key, 1_i32)]);
    let node = signed(tree_backing::<Produce, i32>(1).expect("one node").1);
    let entry = |inline: usize| 2 * signed(BLOCK_ENTRY_SCANNED) - 6 * signed(inline);
    let field = |inline: usize| 2 * signed(BLOCK_FIELD_SCANNED) - 6 * signed(inline);
    // The root is read once more in per-level mode, so it costs the two
    // entry constants less one read of its bytes.
    let root = 2 * signed(BLOCK_ENTRY_SCANNED) - signed(size_of::<BTreeMap<Produce, i32>>());
    // A hash and its byte vector are entries. Their 32 bytes are read three
    // times in block mode, against four.
    let hash = entry(size_of::<Blake2b256Hash>()) + entry(size_of::<Vec<u8>>()) - 32;
    let expected = root +
        node +
        entry(size_of::<Produce>()) +
        field(size_of::<i32>()) +
        2 * hash +
        3 * field(size_of::<bool>()) +
        field(size_of::<Vec<Vec<u8>>>());
    assert_eq!(copy_and_cleanup_excess(&map), expected);
    assert!(0 < expected && expected <= node, "{expected} of a {node}-byte node");
}

/// Negative control for `block_charge_le_per_level_for_rspace_records`: a
/// block copy and cleanup of a small vector root exceeds the per-level one.
/// The root costs two entry constants less one read of its 24 bytes. Each
/// of the two vector elements saves only 8 bytes: two entry constants and
/// one more read of its block bytes, against two per-level pushes. The
/// string byte is read three times, against four.
#[test]
fn block_copy_exceeds_per_level_for_small_roots() {
    let joins = vec![vec!["a".to_owned()]];
    let root = 2 * signed(BLOCK_ENTRY_SCANNED) - signed(size_of::<Vec<Vec<String>>>());
    let element = |inline: usize| 2 * signed(BLOCK_ENTRY_SCANNED) - 5 * signed(inline);
    let expected = root + element(size_of::<Vec<String>>()) + element(size_of::<String>()) - 1;
    assert_eq!(copy_and_cleanup_excess(&joins), expected);
    assert!(0 < expected && expected <= root, "{expected} of {root}");
}

/// D-E1 (DR-108; `WalkerBlockCharge.shared_release_charge_covers_work`): the
/// block-mode copy and cleanup of a store-owned vector of shared pointers
/// charges each pointer and its strong count, and it does not depend on the
/// size of the payloads. The release of one pointer charges the pointer, a
/// field word and the strong count.
#[test]
fn shared_block_wrappers_charge_pointers_and_strong_counts() {
    let vector_bytes = size_of::<Vec<Arc<String>>>();
    let pointer_bytes = size_of::<Arc<String>>();
    for count in 1..5 {
        for payload in [1, 4_096] {
            let values: Vec<Arc<String>> =
                (0..count).map(|_| Arc::new("x".repeat(payload))).collect();
            let copy = [
                3 + 2 * count,
                3 * vector_bytes +
                    BLOCK_ENTRY_SCANNED +
                    count * (2 * pointer_bytes + BLOCK_SHARED_HEADER_SCANNED),
                BLOCK_ENTRY_BACKING + count * pointer_bytes,
            ];
            let cleanup = [
                3 + 2 * count,
                2 * vector_bytes +
                    BLOCK_ENTRY_SCANNED +
                    count * (pointer_bytes + BLOCK_SHARED_HEADER_SCANNED),
                BLOCK_ENTRY_BACKING,
            ];
            assert_eq!(
                usage(|meter| reserve_shared_blocks_copy_and_cleanup(&values, meter)),
                sum(copy, cleanup),
                "{count} pointers to {payload} bytes"
            );
            assert_eq!(
                usage(|meter| reserve_shared_blocks_cleanup(&values[0], meter)),
                [3, pointer_bytes + BLOCK_FIELD_SCANNED + BLOCK_SHARED_HEADER_SCANNED, 0],
                "{payload} bytes"
            );
        }
    }
}

/// D-E1 (DR-108): each block wrapper returns the meter's error at every cut
/// and makes no reservation after it.
#[test]
fn block_wrappers_stop_at_every_cut() {
    let value = Datum::create(&"channel".to_owned(), "value".to_owned(), false);
    let values = vec![value.clone(); 2];
    let shared: Vec<_> = (0..2).map(|_| Arc::new(value.clone())).collect();
    let wrappers: [(&str, &dyn Fn(&dyn SourceMeter) -> Result<(), RSpaceError>); 6] = [
        ("copy and cleanup", &|meter| reserve_blocks_copy_and_cleanup(&value, meter)),
        ("slice copy and cleanup", &|meter| reserve_blocks_slice_copy_and_cleanup(&values, meter)),
        ("slice inspection", &|meter| inspect_blocks_slice(&values, meter)),
        ("cleanup", &|meter| reserve_blocks_cleanup(&value, meter)),
        ("shared copy and cleanup", &|meter| {
            reserve_shared_blocks_copy_and_cleanup(&shared, meter)
        }),
        ("shared cleanup", &|meter| reserve_shared_blocks_cleanup(&shared[0], meter)),
    ];
    for (name, wrapper) in wrappers {
        let baseline = calls(wrapper).len();
        assert!(baseline > 0, "{name}");
        for cut in 0..baseline {
            let made = Cell::new(0_usize);
            let meter = |_: usize, _: usize, _: usize| {
                let call = made.get();
                made.set(call + 1);
                if call == cut {
                    Err(RSpaceError::HostWorkRejected)
                } else {
                    Ok(())
                }
            };
            assert_eq!(wrapper(&meter), Err(RSpaceError::HostWorkRejected), "{name}, cut {cut}");
            assert_eq!(made.get(), cut + 1, "{name}, cut {cut}");
        }
    }
}
