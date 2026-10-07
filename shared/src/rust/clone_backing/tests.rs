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
    // Changed by D-E1 (DR-108): the first entry that a popped entry's
    // children push reuses the popped slot and reserves no backing.
    // let entry = [3, BLOCK_ENTRY_SCANNED, BLOCK_ENTRY_BACKING];
    let first = [3, BLOCK_ENTRY_SCANNED, 0];
    let further = [3, BLOCK_ENTRY_SCANNED, BLOCK_ENTRY_BACKING];
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
        // D-O1 (DR-94): an empty container is charged as an inline field.
        Tree::Bytes(bytes) if bytes.is_empty() => add(totals, field),
        Tree::Text(text) if text.is_empty() => add(totals, field),
        Tree::Node(children) if children.is_empty() => add(totals, field),
        // The popped `Tree` pushes one entry, which reuses its slot.
        Tree::Bytes(bytes) => {
            add(totals, first);
            add(totals, [2 * bytes.len(), 0, 0]);
            block(totals, bytes.len(), false);
        }
        Tree::Text(text) => {
            add(totals, first);
            block(totals, text.len(), false);
        }
        Tree::Node(children) => {
            add(totals, first);
            add(totals, [2 * children.len(), 0, 0]);
            block(totals, children.len() * size_of::<Tree>(), true);
            // The popped vector pushes its elements; the first reuses its slot.
            for (index, child) in children.iter().enumerate() {
                add(totals, if index == 0 { first } else { further });
                oracle_below(child, copy, totals);
            }
        }
        // The popped `Tree` pushes the box, and the popped box pushes the
        // boxed `Tree`: each reuses the slot of its parent.
        Tree::Boxed(child) => {
            add(totals, first);
            block(totals, size_of::<Tree>(), true);
            add(totals, first);
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
        // Changed by D-E1 (DR-108): the root and each child after the first
        // reserve backing; the child vector and the first child reuse the
        // slots of their popped parents. Each child's empty vector is a
        // field (DR-94).
        // let entries = if count == 0 { 1 } else { count + 2 };
        let entries = count.max(1);
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
        Tree::Bytes(bytes) if bytes.is_empty() => 1,
        Tree::Text(text) if text.is_empty() => 1,
        Tree::Bytes(_) | Tree::Text(_) => 2,
        Tree::Node(children) => children
            .iter()
            .map(|child| 2 + entry_depth(child))
            .max()
            .unwrap_or(1),
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
        // Changed by D-E1 (DR-108): only the root and the further entries
        // reserve backing, while every entry reserves its scanned bytes.
        // let entries = block[2] / BLOCK_ENTRY_BACKING;
        // prop_assert_eq!(block[2], entries * BLOCK_ENTRY_BACKING);
        // prop_assert_eq!(charged[2], entries * BLOCK_DEPTH_ENTRY_BACKING);
        // prop_assert_eq!(
        //     charged[1],
        //     block[1] + entries * (BLOCK_DEPTH_ENTRY_SCANNED - BLOCK_ENTRY_SCANNED)
        // );
        let (entries, backed) = entry_counts(&tree);
        prop_assert_eq!(block[2], backed * BLOCK_ENTRY_BACKING);
        prop_assert_eq!(charged[2], backed * BLOCK_DEPTH_ENTRY_BACKING);
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

/// D-O1 (DR-94): a block-mode copy of a slice of shared pointers charges the
/// pointer bytes as an opaque block and the strong-count update of every
/// pointer; its shared-pointer cleanup does not walk the payloads.
#[test]
fn block_slice_copy_of_shared_pointers_charges_each_strong_count() {
    let shared: Vec<Arc<Big>> = (0..5).map(|index| Arc::new(Big([index; 64]))).collect();
    let pointers = shared.len() * size_of::<Arc<Big>>();
    let copy = usage(|meter| reserve_blocks_slice(&shared, meter));
    assert_eq!(copy, [
        2 * shared.len(),
        2 * pointers + shared.len() * BLOCK_SHARED_HEADER_SCANNED,
        pointers,
    ]);
    let cleanup = usage(|meter| inspect_shared_pointer_slice_blocks(&shared, meter));
    assert!(cleanup[1] < size_of::<Big>(), "{cleanup:?}");
    let legacy = usage(|meter| reserve_slice(&shared, meter));
    assert_eq!(legacy[1], 2 * pointers);
}

/// D-O1 (DR-94): in block mode an empty container is charged as an inline
/// field (three operations and one word, no worklist entry and no backing);
/// the same container with content is a worklist entry.
#[test]
fn empty_containers_are_charged_as_fields() {
    fn inspected<T: CloneBacking>(value: &T) -> [usize; 3] {
        usage(|meter| inspect_blocks(value, meter))
    }
    let root = |size: usize| 2 * size;
    let field = BLOCK_FIELD_SCANNED;
    let entry = BLOCK_ENTRY_SCANNED;
    let backing = BLOCK_ENTRY_BACKING;
    let size = size_of::<Option<Vec<u64>>>();
    assert_eq!(inspected(&None::<Vec<u64>>), [3, root(size) + field, 0]);
    assert_eq!(inspected(&Some(Vec::<u64>::new())), [
        6,
        root(size) + entry + field,
        backing
    ]);
    let size = size_of::<Option<String>>();
    assert_eq!(inspected(&Some(String::new())), [
        6,
        root(size) + entry + field,
        backing
    ]);
    let size = size_of::<Option<BTreeMap<u64, u64>>>();
    assert_eq!(inspected(&Some(BTreeMap::<u64, u64>::new())), [
        6,
        root(size) + entry + field,
        backing
    ]);
    let size = size_of::<Option<BTreeSet<u64>>>();
    assert_eq!(inspected(&Some(BTreeSet::<u64>::new())), [
        6,
        root(size) + entry + field,
        backing
    ]);
    let full = inspected(&Some(vec![1_u64]));
    // Changed by D-E1 (DR-108): the vector reuses the slot of the popped
    // root, so only the root reserves backing.
    // assert_eq!(full[2], 2 * backing);
    assert_eq!(full[2], backing);
}

/// D-E1 (DR-108): the worklist entries of a block walk of `tree` from the
/// root, and the entries that reserve worklist backing: the root and, for
/// each popped entry, the entries after the first that its children push.
fn entry_counts(tree: &Tree) -> (usize, usize) {
    /// The entries pushed below a popped `Tree` entry, and those of them
    /// that reserve backing.
    fn below(tree: &Tree) -> (usize, usize) {
        match tree {
            Tree::Leaf(_) => (0, 0),
            Tree::Bytes(bytes) if bytes.is_empty() => (0, 0),
            Tree::Text(text) if text.is_empty() => (0, 0),
            Tree::Node(children) if children.is_empty() => (0, 0),
            Tree::Bytes(_) | Tree::Text(_) => (1, 0),
            Tree::Node(children) => children.iter().fold(
                (1 + children.len(), children.len() - 1),
                |(entries, backed), child| {
                    let (child_entries, child_backed) = below(child);
                    (entries + child_entries, backed + child_backed)
                },
            ),
            Tree::Boxed(child) => {
                let (entries, backed) = below(child);
                (2 + entries, backed)
            }
        }
    }
    let (entries, backed) = below(tree);
    (1 + entries, 1 + backed)
}

/// D-E1 (DR-108): a chain of `depth` boxes around a leaf.
fn box_chain(depth: usize) -> Tree {
    let mut tree = Tree::Leaf(7);
    for _ in 0..depth {
        tree = Tree::Boxed(Box::new(tree));
    }
    tree
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// D-E1 (DR-108; `WalkerBlockCharge.charged_pushes_le_entries`): the
    /// chained slot reserves backing for the root and the further entries
    /// only, which is never more than the backing of every entry (DR-92).
    #[test]
    fn chain_slot_backing_never_exceeds_per_entry_backing(tree in tree_strategy()) {
        let (entries, backed) = entry_counts(&tree);
        let charged = usage(|meter| inspect_blocks(&tree, meter));
        prop_assert_eq!(charged[2], backed * BLOCK_ENTRY_BACKING);
        prop_assert!(backed <= entries);
    }
}

/// D-E1 (DR-108; `WalkerBlockCharge.chain_slot_charge_covers_worklist`): the
/// worklist allocates at most the backing that the chained slot reserves,
/// for a wide node, a deep chain and a mix of both. A chain of boxes keeps
/// one entry on the worklist, so its walk reserves and allocates one chunk.
#[test]
fn chain_slot_backing_covers_worklist_allocations() {
    let wide = Tree::Node(vec![Tree::Leaf(1); 600]);
    let deep = box_chain(10_000);
    let mixed = Tree::Node((0..50).map(|_| box_chain(50)).collect());
    for (name, tree) in [("wide", &wide), ("deep", &deep), ("mixed", &mixed)] {
        let (charged, allocated) = measured(|| usage(|meter| inspect_blocks(tree, meter)));
        assert!(
            allocated <= charged[2],
            "{name}: allocated {allocated} charged {charged:?}"
        );
        let (_, backed) = entry_counts(tree);
        assert_eq!(charged[2], backed * BLOCK_ENTRY_BACKING, "{name}");
        let (depth_charged, depth_allocated) = measured(|| depth_walk(tree).0);
        assert!(depth_allocated <= depth_charged[2], "{name}: depth walk");
    }
    let (charged, allocated) = measured(|| usage(|meter| inspect_blocks(&deep, meter)));
    assert_eq!(charged[2], BLOCK_ENTRY_BACKING);
    assert_eq!(
        allocated,
        FIRST_CHUNK_SLOTS * size_of::<&dyn CloneBacking>()
    );
}

/// D-E1 (DR-108): the root reserves backing, and each popped entry's first
/// pushed entry reuses its slot. A field that the popped entry pushes before
/// that entry does not take the slot.
#[test]
fn chain_slot_charges_the_root_and_each_further_child() {
    let backing = |value: &dyn Fn(&dyn BackingMeter) -> Result<(), BackingError>| {
        usage(|meter| value(meter))[2]
    };
    // The root, then the tuple and its vector reuse popped slots; the
    // scalar before the vector is a field.
    let after_field = Some((7_u64, vec![1_u64]));
    assert_eq!(
        backing(&|meter| inspect_blocks(&after_field, meter)),
        BLOCK_ENTRY_BACKING
    );
    // The tuple pushes two vectors: the second reserves backing.
    let two = Some((vec![1_u64], vec![2_u64]));
    assert_eq!(
        backing(&|meter| inspect_blocks(&two, meter)),
        2 * BLOCK_ENTRY_BACKING
    );
    // A node of n leaves: the root and the n - 1 leaves after the first.
    for count in 1..=9 {
        let node = Tree::Node(vec![Tree::Leaf(1); count]);
        assert_eq!(
            backing(&|meter| inspect_blocks(&node, meter)),
            count * BLOCK_ENTRY_BACKING,
            "{count} leaves"
        );
    }
}

/// D-E1 (DR-108; `WalkerBlockCharge.shared_release_charge_covers_work`): the
/// block-mode cleanup of store-owned shared pointers charges each pointer as
/// a field with its strong-count header, and no worklist entry or payload.
#[test]
fn shared_release_walk_charges_pointer_and_strong_count() {
    let single = Arc::new(Big([3; 64]));
    assert_eq!(
        usage(|meter| inspect_shared_pointers_blocks(&single, meter)),
        [
            3,
            size_of::<Arc<Big>>() + BLOCK_FIELD_SCANNED + BLOCK_SHARED_HEADER_SCANNED,
            0
        ]
    );
    let count: usize = 5;
    let large: Vec<Arc<Big>> = (0..count)
        .map(|index| Arc::new(Big([u64::try_from(index).expect("small index"); 64])))
        .collect();
    let small: Vec<Arc<u64>> = (0..count)
        .map(|index| Arc::new(u64::try_from(index).expect("small index")))
        .collect();
    let pointers = count * size_of::<Arc<Big>>();
    let expected = [
        3 + 2 * count,
        2 * size_of::<Vec<Arc<Big>>>()
            + BLOCK_ENTRY_SCANNED
            + pointers
            + count * BLOCK_SHARED_HEADER_SCANNED,
        BLOCK_ENTRY_BACKING,
    ];
    assert_eq!(
        usage(|meter| inspect_shared_pointers_blocks(&large, meter)),
        expected
    );
    assert_eq!(
        usage(|meter| inspect_shared_pointers_blocks(&small, meter)),
        expected
    );
    let slice = usage(|meter| inspect_shared_pointer_slice_blocks(&large, meter));
    assert_eq!(slice, [
        2 * count,
        pointers + count * BLOCK_SHARED_HEADER_SCANNED,
        0
    ]);
}
