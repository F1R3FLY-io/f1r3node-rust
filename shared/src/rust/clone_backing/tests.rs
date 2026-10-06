use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use proptest::prelude::*;

use super::*;

thread_local! {
    static ALLOCATED: Cell<Option<usize>> = const { Cell::new(None) };
}

struct MeasuredAllocator;

fn measure(bytes: usize) {
    let _ = ALLOCATED.try_with(|total| {
        if let Some(current) = total.get() {
            total.set(Some(current.checked_add(bytes).expect("allocation total")));
        }
    });
}

unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            measure(layout.size());
        }
        result
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() {
            measure(layout.size());
        }
        result
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            measure(size);
        }
        result
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;

/// The bytes allocated on this thread while `action` runs.
fn measured<T>(action: impl FnOnce() -> T) -> (T, usize) {
    ALLOCATED.with(|total| total.set(Some(0)));
    let result = action();
    let allocated = ALLOCATED
        .with(|total| total.replace(None))
        .expect("measured total");
    (result, allocated)
}

/// The usage (operations, scanned bytes, backing bytes) that `charge` reserves.
fn usage(charge: impl FnOnce(&dyn BackingMeter) -> Result<(), BackingError>) -> [usize; 3] {
    let used = Cell::new([0usize; 3]);
    let meter = |operations: usize, scanned: usize, backing: usize| {
        let mut totals = used.get();
        for (total, amount) in totals.iter_mut().zip([operations, scanned, backing]) {
            *total = total.checked_add(amount).ok_or(BackingError::Overflow)?;
        }
        used.set(totals);
        Ok(())
    };
    charge(&meter).expect("charge");
    used.get()
}

/// A recursive test value with inline scalars, opaque byte blocks, visited
/// element blocks, boxed payloads and strings.
#[derive(Clone, Debug)]
enum Tree {
    Leaf(u64),
    Bytes(Vec<u8>),
    Node(Vec<Tree>),
    Boxed(Box<Tree>),
    Text(String),
}

impl CloneBacking for Tree {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        match self {
            Tree::Leaf(value) => walker.push(value),
            Tree::Bytes(bytes) => walker.push(bytes),
            Tree::Node(children) => walker.push(children),
            Tree::Boxed(child) => walker.push(child),
            Tree::Text(text) => walker.push(text),
        }
    }
}

fn tree_strategy() -> impl Strategy<Value = Tree> {
    let leaf = prop_oneof![
        any::<u64>().prop_map(Tree::Leaf),
        prop::collection::vec(any::<u8>(), 0..40).prop_map(Tree::Bytes),
        "[a-z]{0,24}".prop_map(Tree::Text),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Tree::Node),
            inner.prop_map(|child| Tree::Boxed(Box::new(child))),
        ]
    })
}

/// An independent statement of the block-accounting rules (D-O1, DR-92) for
/// `Tree`: the charge of the subtree below one `Tree` worklist entry.
fn oracle_below(tree: &Tree, copy: bool, totals: &mut [usize; 3]) {
    let entry = [3, BLOCK_ENTRY_SCANNED, BLOCK_ENTRY_BACKING];
    let field = [3, BLOCK_FIELD_SCANNED, 0];
    let add = |totals: &mut [usize; 3], charge: [usize; 3]| {
        for (total, amount) in totals.iter_mut().zip(charge) {
            *total += amount;
        }
    };
    // A heap block of `bytes`: read once, again if visited, written by a copy.
    let block = |totals: &mut [usize; 3], bytes: usize, visited: bool| {
        let reads = usize::from(visited) + 1 + usize::from(copy);
        add(totals, [0, bytes * reads, if copy { bytes } else { 0 }]);
    };
    match tree {
        Tree::Leaf(_) => add(totals, field),
        Tree::Bytes(bytes) => {
            add(totals, entry);
            add(totals, [2 * bytes.len(), 0, 0]);
            block(totals, bytes.len(), false);
        }
        Tree::Text(text) => {
            add(totals, entry);
            block(totals, text.len(), false);
        }
        Tree::Node(children) => {
            add(totals, entry);
            add(totals, [2 * children.len(), 0, 0]);
            block(totals, children.len() * size_of::<Tree>(), true);
            for child in children {
                add(totals, entry);
                oracle_below(child, copy, totals);
            }
        }
        Tree::Boxed(child) => {
            add(totals, entry);
            block(totals, size_of::<Tree>(), true);
            add(totals, entry);
            oracle_below(child, copy, totals);
        }
    }
}

fn oracle(tree: &Tree, copy: bool) -> [usize; 3] {
    // The root block is read through a reference: visited, not allocated.
    let reads = 2 + usize::from(copy);
    let mut totals = [
        3,
        size_of::<Tree>() * reads + BLOCK_ENTRY_SCANNED,
        BLOCK_ENTRY_BACKING,
    ];
    oracle_below(tree, copy, &mut totals);
    totals
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// D-O1 (DR-92): the walker's block charges equal an independent
    /// statement of the block rules, for inspections and copies.
    #[test]
    fn block_inspection_charge_matches_independent_block_oracle(tree in tree_strategy()) {
        prop_assert_eq!(usage(|meter| inspect_blocks(&tree, meter)), oracle(&tree, false));
        prop_assert_eq!(usage(|meter| reserve_blocks(&tree, meter)), oracle(&tree, true));
    }

    /// D-O1 (DR-92): a copy reserves at least the bytes its clone allocates.
    #[test]
    fn block_copy_backing_covers_counted_allocations(tree in tree_strategy()) {
        let reserved = usage(|meter| reserve_blocks(&tree, meter))[2];
        let (copy, allocated) = measured(|| tree.clone());
        drop(copy);
        prop_assert!(allocated <= reserved, "allocated {} reserved {}", allocated, reserved);
    }

    /// D-O1 (DR-92): an inspection fits every copy budget of the same value.
    #[test]
    fn block_inspection_fits_every_block_copy_budget(tree in tree_strategy()) {
        let inspect = usage(|meter| inspect_blocks(&tree, meter));
        let copy = usage(|meter| reserve_blocks(&tree, meter));
        for (inspected, copied) in inspect.iter().zip(copy) {
            prop_assert!(*inspected <= copied);
        }
    }

}

/// A large inline payload: 64 words that lie in the enclosing block.
#[derive(Clone)]
struct Big([u64; 64]);

impl CloneBacking for Big {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        walker.push(&self.0)
    }
}

/// One, two and three levels of inline nesting around `Big`.
type NestedValues = (
    Option<Big>,
    Option<Option<Big>>,
    Option<Option<Option<Big>>>,
);

fn nested_values() -> NestedValues {
    let big = Big([7; 64]);
    (
        Some(big.clone()),
        Some(Some(big.clone())),
        Some(Some(Some(big))),
    )
}

/// D-O1 (DR-92): each extra level of inline nesting adds the worklist
/// constants and the growth of the root's own inline size, not the size of
/// the nested value.
#[test]
fn block_charge_is_independent_of_inline_nesting_depth() {
    let (one, two, three) = nested_values();
    let blocks = [
        usage(|meter| inspect_blocks(&one, meter)),
        usage(|meter| inspect_blocks(&two, meter)),
        usage(|meter| inspect_blocks(&three, meter)),
    ];
    let sizes = [
        size_of::<Option<Big>>(),
        size_of::<Option<Option<Big>>>(),
        size_of::<Option<Option<Option<Big>>>>(),
    ];
    for level in 0..2 {
        let block_growth = blocks[level + 1][1] - blocks[level][1];
        let root_growth = 2 * (sizes[level + 1] - sizes[level]);
        assert_eq!(
            block_growth,
            BLOCK_ENTRY_SCANNED + root_growth,
            "level {level}"
        );
    }
}

/// D-O1 (DR-92), negative control: the legacy per-push charge grows by at
/// least three times the nested value's size per level of inline nesting.
#[test]
fn legacy_level_charge_grew_with_inline_nesting_depth() {
    let (one, two, three) = nested_values();
    let legacy = [
        usage(|meter| inspect(&one, meter)),
        usage(|meter| inspect(&two, meter)),
        usage(|meter| inspect(&three, meter)),
    ];
    for level in 0..2 {
        let legacy_growth = legacy[level + 1][1] - legacy[level][1];
        assert!(
            legacy_growth >= 3 * size_of::<Big>(),
            "level {level}: {legacy_growth}"
        );
    }
}

/// `tree` with the children of every node rotated by `rotation`.
fn rotated(tree: &Tree, rotation: usize) -> Tree {
    match tree {
        Tree::Node(children) => {
            let mut children: Vec<Tree> = children
                .iter()
                .map(|child| rotated(child, rotation))
                .collect();
            if !children.is_empty() {
                let shift = rotation % children.len();
                children.rotate_left(shift);
            }
            Tree::Node(children)
        }
        Tree::Boxed(child) => Tree::Boxed(Box::new(rotated(child, rotation))),
        other => other.clone(),
    }
}

/// D-B3 (DR-92): the bytes the chunked worklist allocates for a peak of
/// `peak` entries: chunks 0..=k with `FIRST_CHUNK_SLOTS << j` slots, and the
/// vector of the headers of chunks 1..=k, which grows by doubling from 4.
fn chunk_model_bytes(peak: usize) -> usize {
    let mut chunks = 0;
    let mut slots = 0;
    while slots < peak.max(1) {
        slots += FIRST_CHUNK_SLOTS << chunks;
        chunks += 1;
    }
    let extra_chunks = chunks - 1;
    let mut header_slots = 0;
    if extra_chunks > 0 {
        let mut capacity = 4;
        header_slots += capacity;
        while capacity < extra_chunks {
            capacity *= 2;
            header_slots += capacity;
        }
    }
    slots * size_of::<&dyn CloneBacking>() + header_slots * size_of::<Vec<&dyn CloneBacking>>()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// D-B3 (DR-92): the charge does not depend on the order of siblings,
    /// although the worklist peak does.
    #[test]
    fn worklist_charge_independent_of_child_order(tree in tree_strategy(), rotation in 0_usize..8) {
        let reordered = rotated(&tree, rotation);
        prop_assert_eq!(usage(|meter| inspect_blocks(&tree, meter)), usage(|meter| inspect_blocks(&reordered, meter)));
        prop_assert_eq!(usage(|meter| reserve_blocks(&tree, meter)), usage(|meter| reserve_blocks(&reordered, meter)));
    }

    /// D-B3 (DR-92): the worklist allocates at most the backing that an
    /// inspection charges (an inspection's backing is the entry backing).
    #[test]
    fn worklist_allocations_within_charge(tree in tree_strategy()) {
        let (charged, allocated) = measured(|| usage(|meter| inspect_blocks(&tree, meter)));
        prop_assert!(allocated <= charged[2], "allocated {} charged {}", allocated, charged[2]);
    }
}

/// D-B3 (DR-92): a flat node with `n` children reaches a peak of `n`
/// entries; the worklist allocates exactly the chunks and chunk headers of
/// the chunk model, within the entry backing.
#[test]
fn worklist_chunks_follow_the_chunk_model() {
    for count in 0..=600 {
        let tree = Tree::Node(vec![Tree::Node(Vec::new()); count]);
        let (charged, allocated) = measured(|| usage(|meter| inspect_blocks(&tree, meter)));
        assert_eq!(allocated, chunk_model_bytes(count), "{count} children");
        let entries = 2 * count + 2;
        assert_eq!(
            charged[2],
            entries * BLOCK_ENTRY_BACKING,
            "{count} children"
        );
        assert!(allocated <= charged[2], "{count} children");
    }
}

/// D-B3 (DR-92): the chunked worklist pops in LIFO order, keeps its chunks
/// when the stack shrinks, and reuses them when it grows again.
#[test]
fn chunked_worklist_reuses_chunks_and_pops_in_order() {
    let values: Vec<u64> = (0..64).collect();
    let mut worklist: ChunkedWorklist<&dyn CloneBacking> = ChunkedWorklist::new();
    let ((), grown) = measured(|| {
        for value in &values[..29] {
            worklist.push(value).expect("push");
        }
    });
    assert_eq!(grown, chunk_model_bytes(29));
    for _ in 0..20 {
        let ((), regrown) = measured(|| {
            for _ in 0..17 {
                worklist.pop().expect("entry");
            }
            for value in &values[12..29] {
                worklist.push(value).expect("push");
            }
        });
        assert_eq!(regrown, 0);
    }
    let mut popped = Vec::new();
    while let Some(value) = worklist.pop() {
        popped.push(value as *const dyn CloneBacking as *const ());
    }
    let expected: Vec<*const ()> = values[..29]
        .iter()
        .rev()
        .map(|value| value as *const u64 as *const ())
        .collect();
    assert_eq!(popped, expected);
}

/// D-O1 (DR-92): a copy of a shared pointer charges the field constant and
/// the strong-count header; its inspection charges the payload block.
#[test]
fn shared_pointer_blocks_charge_header_and_payload() {
    let shared = Arc::new(Big([1; 64]));
    let copy = usage(|meter| reserve_blocks(&shared, meter));
    assert_eq!(copy, [
        3,
        2 * size_of::<Arc<Big>>() + BLOCK_FIELD_SCANNED + BLOCK_SHARED_HEADER_SCANNED,
        0
    ]);
    let inspect = usage(|meter| inspect_blocks(&shared, meter));
    assert!(inspect[1] >= 2 * size_of::<Big>());
    let pointers = usage(|meter| inspect_shared_pointers_blocks(&shared, meter));
    assert!(pointers[1] < size_of::<Big>());
}

/// D-O6 (DR-93): the nesting depth of the deepest entry when `tree` is an
/// entry at depth 1, by an independent recursion.
fn entry_depth(tree: &Tree) -> usize {
    match tree {
        Tree::Leaf(_) => 1,
        Tree::Bytes(_) | Tree::Text(_) => 2,
        Tree::Node(children) => children
            .iter()
            .map(|child| 2 + entry_depth(child))
            .max()
            .unwrap_or(2),
        Tree::Boxed(child) => 2 + entry_depth(child),
    }
}

/// The usage and the result of a depth walk.
fn depth_walk(tree: &Tree) -> ([usize; 3], BlockWalk) {
    let walked = Cell::new(None);
    let charged = usage(|meter| {
        walked.set(Some(inspect_blocks_depth(tree, meter)?));
        Ok(())
    });
    (charged, walked.get().expect("walk"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// D-O6 (DR-93): a depth walk charges the block charge with the depth
    /// entry constants, reports the scanned bytes it reserved, and finds the
    /// depth of an independent recursion.
    #[test]
    fn depth_walk_matches_block_walk_with_depth_entries(tree in tree_strategy()) {
        let block = usage(|meter| inspect_blocks(&tree, meter));
        let (charged, walk) = depth_walk(&tree);
        prop_assert_eq!(walk.depth, entry_depth(&tree));
        prop_assert_eq!(walk.scanned, charged[1]);
        prop_assert_eq!(charged[0], block[0]);
        let entries = block[2] / BLOCK_ENTRY_BACKING;
        prop_assert_eq!(block[2], entries * BLOCK_ENTRY_BACKING);
        prop_assert_eq!(charged[2], entries * BLOCK_DEPTH_ENTRY_BACKING);
        prop_assert_eq!(
            charged[1],
            block[1] + entries * (BLOCK_DEPTH_ENTRY_SCANNED - BLOCK_ENTRY_SCANNED)
        );
    }

    /// D-O6 (DR-93): the depth walk's worklist allocates at most the backing
    /// that it charges.
    #[test]
    fn depth_walk_allocations_within_charge(tree in tree_strategy()) {
        let (charged, allocated) = measured(|| depth_walk(&tree).0);
        prop_assert!(allocated <= charged[2], "allocated {} charged {}", allocated, charged[2]);
    }
}

/// D-O6 (DR-93): the nested-encode reservation is the depth walk, then
/// `depth` more traversals of the walk's scanned charge and the output bytes.
#[test]
fn nested_encode_reservation_adds_depth_traversals_and_output() {
    let tree = Tree::Boxed(Box::new(Tree::Node(vec![
        Tree::Leaf(3),
        Tree::Text("abc".to_owned()),
        Tree::Boxed(Box::new(Tree::Bytes(vec![1; 9]))),
    ])));
    let (walk_usage, walk) = depth_walk(&tree);
    assert_eq!(walk.depth, entry_depth(&tree));
    let encode = usage(|meter| reserve_nested_encode(&tree, 100, meter));
    assert_eq!(encode, [
        walk_usage[0],
        walk_usage[1] + walk.depth * walk.scanned + 100,
        walk_usage[2],
    ]);
}
