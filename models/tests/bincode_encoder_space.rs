//! # The SPACE gate — the encoder's heap, measured rather than intended
//!
//! Space is a co-equal acceptance criterion, not a footnote to throughput, and
//! every claim below is about a *mechanism* that no correctness test can see:
//!
//! | claim | why a correctness test cannot see it |
//! |---|---|
//! | the op stack is Θ(term **depth**), not Θ(term **size**) | a driver that pushed all `n` children eagerly emits byte-identical output |
//! | the op-stack entry is four words | an `Op` that silently grew multiplies the only per-call heap there is |
//! | the reused buffer converges and does not pin | a leak of capacity is invisible to every assertion about bytes |
//! | program addresses do **not** identify a type | the unsound downcast *worked* on every fixture until an `EList` met it |
//!
//! ⚠ The last row is not hypothetical. `EPATHMAP_PROGRAM` is
//! `[Seq, EmptyBytes, Bool, Opt]` — byte-for-byte identical to `ELIST_PROGRAM`,
//! `ESET_PROGRAM` and `EMAP_PROGRAM` — and the linker **merges** identical
//! read-only statics. A downcast keyed on the program's address therefore
//! reinterpreted an `EList` as an `EPathMap` and produced a `SIGSEGV` on the
//! differential's first run. [`program_addresses_do_not_identify_a_type`] keeps
//! that fact executable so the trick cannot return as an "optimization".

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par, Send};
use models::rust::rholang::bincode_encoder::{
    encode, encode_into, op_size, op_stack_high_water, program_address, with_encoded,
};
use models::rust::rholang::bincode_schema::BincodeNode;

mod par_corpus;
use par_corpus as corpus;

// ---------------------------------------------------------------------------
// term shapes, built ITERATIVELY so construction is never the constraint
// ---------------------------------------------------------------------------

/// A left-spine of `depth` nested `EList`s: DEEP and narrow.
fn deep(depth: usize) -> Par {
    let mut par = Par::default();
    for _ in 0..depth {
        par = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![par],
                    ..Default::default()
                })),
            }],
            ..Default::default()
        };
    }
    par
}

/// One node with `width` children: SHALLOW and wide. Same node COUNT as
/// `deep(width)`, so the two shapes isolate depth from size exactly.
fn wide(width: usize) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: (0..width).map(|i| corpus::gint(i as i64)).collect(),
                ..Default::default()
            })),
        }],
        ..Default::default()
    }
}

// ===========================================================================
// §1  ★ The op stack is Θ(DEPTH), not Θ(SIZE)
// ===========================================================================

/// **The verdict**, pure in its observations, so it can be shown a synthetic
/// Θ(size) ladder and required to reject it.
fn no_size_slope_verdict(
    label: &str,
    lo_width: usize,
    lo_high_water: usize,
    hi_width: usize,
    hi_high_water: usize,
) -> Result<(), String> {
    let growth = hi_high_water.saturating_sub(lo_high_water);
    // A width ladder spanning 1,000× may cost a small constant more (the count
    // prefix opens one `Seq` op), but nothing proportional.
    if growth <= 4 {
        return Ok(());
    }
    Err(format!(
        "Θ(SIZE) OP STACK for `{label}`: the high-water mark grew from {lo_high_water} at \
         width {lo_width} to {hi_high_water} at width {hi_width} — {:.4} entries per sibling. \
         `Op::Seq` must RE-PUSH ITSELF with `index + 1`, not push one op per child; a term \
         with a million siblings would otherwise cost a million-entry stack while emitting \
         byte-identical output.",
        growth as f64 / (hi_width - lo_width) as f64
    ))
}

#[test]
fn the_op_stack_is_flat_in_term_size() {
    let lo = op_stack_high_water(&wide(4));
    let hi = op_stack_high_water(&wide(65_536));
    if let Err(why) = no_size_slope_verdict("wide", 4, lo, 65_536, hi) {
        panic!("{why}");
    }
    println!("  op stack (width): {lo} entries at 4, {hi} at 65,536");

    // ★★ ANTI-VACUITY: the verdict must REJECT a Θ(size) ladder. Without this,
    // a checker that accepted everything would look identical to a pass.
    let synthetic = no_size_slope_verdict("synthetic-Θ(size)", 4, 4, 65_536, 65_536);
    let why = synthetic.expect_err("a Θ(size) ladder must be REJECTED");
    assert!(
        why.contains("entries per sibling"),
        "the rejection must name the per-sibling growth; got: {why}"
    );
}

#[test]
fn the_op_stack_grows_only_with_depth_and_only_by_a_bounded_amount() {
    // Depth IS allowed to cost entries — that is the whole point of an explicit
    // stack — but the cost per level must be a small constant, not a
    // multiplier. A `Par → Expr → EList → Par` level is four nodes.
    let lo_depth = 16usize;
    let hi_depth = 4_096usize;
    let lo = op_stack_high_water(&deep(lo_depth));
    let hi = op_stack_high_water(&deep(hi_depth));
    let per_level = (hi - lo) as f64 / (hi_depth - lo_depth) as f64;
    println!("  op stack (depth): {lo} at {lo_depth}, {hi} at {hi_depth} ({per_level:.3}/level)");
    assert!(
        per_level <= 2.0,
        "the op stack costs {per_level:.3} entries per nesting level, and the measured value \
         is 2.000. Each level is four schema nodes (Par → Expr → EList → Par), of which TWO \
         need a resume point: `Par` has trailing fields after `exprs`, and `EList` has \
         trailing fields after `ps`. The other two are tail calls — a spent program \
         (`Machine::suspend`) and the LAST element of a sequence (`Op::Seq`). A regression \
         here means one of the two tail calls stopped being taken; it doubles the encoder\'s \
         only per-call heap on the adversarial shape."
    );

    // And the heap that costs, stated in bytes, so the number is comparable to
    // the decoder's value stacks rather than to an entry count.
    let bytes = hi * op_size();
    assert!(
        bytes <= 320 * 1024,
        "a 4,096-deep term costs {bytes} B of op stack ({} entries × {} B); the encoder's \
         only per-call heap must stay small enough that it is never the reason a node runs \
         out of memory",
        hi,
        op_size()
    );
    println!(
        "  op stack (depth {hi_depth}): {bytes} B = {hi} × {} B",
        op_size()
    );
}

#[test]
fn the_op_stack_entry_stays_small() {
    // `&dyn` is two words; the widest arm is a `&dyn` plus a `usize` plus the
    // discriminant, which packs into four words on a 64-bit target.
    assert!(
        op_size() <= 4 * std::mem::size_of::<usize>(),
        "`Op` grew to {} B ({} words). Every entry is multiplied by the term's DEPTH, so an \
         arm that started carrying a large payload should move to a side stack — the \
         discipline `bincode_decoder`'s `ParFrame`/`ReceiveTail`/`NewFrame` already follow.",
        op_size(),
        op_size() / std::mem::size_of::<usize>()
    );
    println!("  size_of::<Op>() = {} B", op_size());
}

// ===========================================================================
// §2  ★ Zero per-encode allocation in the steady state
// ===========================================================================

/// A counting allocator, installed globally for this test binary.
///
/// ⚠ It is the only honest way to assert "allocates nothing": a massif profile
/// shows aggregate behaviour, whereas the acceptance criterion is a statement
/// about *one call*.
mod counting_alloc {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    // ⚠ PER-THREAD, not global. `cargo test` runs test functions on separate
    // threads by default, so an `AtomicUsize` would have every measurement
    // counting its neighbours' allocations — a number that changes with
    // `--test-threads` is not a measurement of this encoder at all. (Observed:
    // the same assertion passed at `--test-threads=1` and failed in the full
    // suite.)
    //
    // `const`-initialised `Cell<usize>` has no destructor, so it registers no
    // TLS teardown hook and cannot re-enter the allocator it lives inside.
    thread_local! {
        static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
        static BYTES: Cell<usize> = const { Cell::new(0) };
    }

    fn bump(bytes: usize) {
        // `try_with` because a thread in TLS teardown must not panic here.
        let _ = ALLOCATIONS.try_with(|c| c.set(c.get() + 1));
        let _ = BYTES.try_with(|c| c.set(c.get() + bytes));
    }

    fn read() -> (usize, usize) {
        (
            ALLOCATIONS.try_with(Cell::get).unwrap_or(0),
            BYTES.try_with(Cell::get).unwrap_or(0),
        )
    }

    pub struct Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            bump(layout.size());
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            bump(new_size);
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    /// Allocations and bytes attributable to `f`, **on this thread**.
    pub fn measure<R>(f: impl FnOnce() -> R) -> (R, usize, usize) {
        let (a0, b0) = read();
        let r = f();
        let (a1, b1) = read();
        (r, a1 - a0, b1 - b0)
    }
}

#[global_allocator]
static ALLOC: counting_alloc::Counting = counting_alloc::Counting;

mod act_inspection_collection_primitives {
    use std::collections::{HashMap, HashSet};
    use std::fmt::Debug;
    use std::hash::Hash;
    use std::mem::size_of;

    use proptest::prelude::*;
    use shared::rust::collection_backing::hash_backing;

    use super::counting_alloc;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Metadata {
        offsets: [usize; 4],
        value: Option<u64>,
        children: [u8; 32],
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct ShallowNode {
        metadata: Metadata,
        child_bytes: Vec<u8>,
    }

    fn metadata(ordinal: usize) -> Metadata {
        Metadata {
            offsets: [ordinal; 4],
            value: Some(ordinal as u64),
            children: [ordinal as u8; 32],
        }
    }

    fn requested_buckets(required: usize) -> usize {
        match required {
            0 => 0,
            1..=3 => 4,
            4..=7 => 8,
            8..=14 => 16,
            _ => (required.checked_mul(8).unwrap() / 7)
                .checked_next_power_of_two().unwrap(),
        }
    }

    fn bucket_capacity(buckets: usize) -> usize {
        match buckets {
            0 => 0,
            4 | 8 => buckets - 1,
            _ => buckets / 8 * 7,
        }
    }

    fn check_hash_reserve<K, V>(
        length: usize,
        capacity: usize,
        additional: usize,
        totals: &mut (usize, usize),
        reserve: impl FnOnce() -> usize,
    ) {
        let required = length.checked_add(additional).unwrap();
        let (expected_capacity, bound) = if required > capacity {
            let buckets = requested_buckets(required);
            (bucket_capacity(buckets), hash_backing::<K, V>(buckets - 1).unwrap().1)
        } else {
            (capacity, 0)
        };
        totals.0 += bound;
        let (actual_capacity, allocations, requested) = counting_alloc::measure(reserve);
        totals.1 += requested;
        assert_eq!(actual_capacity, expected_capacity);
        assert!(requested <= bound, "reserve requested {requested} bytes, bound {bound}");
        assert!(totals.1 <= totals.0);
        if bound == 0 {
            assert_eq!(allocations, 0);
        } else {
            assert!(allocations > 0);
        }
    }

    fn check_reserved_insert<R>(insert: impl FnOnce() -> R) -> R {
        let (result, allocations, requested) = counting_alloc::measure(insert);
        assert_eq!(allocations, 0);
        assert_eq!(requested, 0);
        result
    }

    fn check_map<K: Copy + Debug + Eq + Hash, V: Debug + PartialEq>(
        initial: usize,
        insertions: usize,
        key: impl Fn(usize) -> K,
        value: impl Fn(usize) -> V,
    ) {
        let mut map = HashMap::new();
        let mut totals = (0, 0);
        check_hash_reserve::<K, V>(0, 0, initial, &mut totals, || {
            map.try_reserve(initial).unwrap();
            map.capacity()
        });
        for ordinal in 0..insertions.max(1) {
            check_hash_reserve::<K, V>(map.len(), map.capacity(), 1, &mut totals, || {
                map.try_reserve(1).unwrap();
                map.capacity()
            });
            let capacity = map.capacity();
            let next_value = value(ordinal);
            assert!(check_reserved_insert(|| map.insert(key(ordinal), next_value)).is_none());
            assert_eq!(map.capacity(), capacity);
            assert_eq!(map.len(), ordinal + 1);
        }
        while map.len() < map.capacity() {
            let ordinal = map.len();
            check_hash_reserve::<K, V>(map.len(), map.capacity(), 1, &mut totals, || {
                map.try_reserve(1).unwrap();
                map.capacity()
            });
            let next_value = value(ordinal);
            assert!(check_reserved_insert(|| map.insert(key(ordinal), next_value)).is_none());
        }
        let full_capacity = map.capacity();
        assert_eq!(map.len(), full_capacity);
        check_hash_reserve::<K, V>(map.len(), map.capacity(), 1, &mut totals, || {
            map.try_reserve(1).unwrap();
            map.capacity()
        });
        assert!(map.capacity() > full_capacity);
        let replacement = value(full_capacity + 1);
        assert_eq!(check_reserved_insert(|| map.insert(key(0), replacement)), Some(value(0)));
        assert_eq!(map.len(), full_capacity);
        assert_eq!(map.get(&key(0)), Some(&value(full_capacity + 1)));
        for ordinal in 1..full_capacity {
            assert_eq!(map.get(&key(ordinal)), Some(&value(ordinal)));
        }
        assert!(totals.1 <= totals.0);
    }

    fn check_set<K: Copy + Debug + Eq + Hash>(
        initial: usize,
        insertions: usize,
        key: impl Fn(usize) -> K,
    ) {
        let mut set = HashSet::new();
        let mut totals = (0, 0);
        check_hash_reserve::<K, ()>(0, 0, initial, &mut totals, || {
            set.try_reserve(initial).unwrap();
            set.capacity()
        });
        for ordinal in 0..insertions.max(1) {
            check_hash_reserve::<K, ()>(set.len(), set.capacity(), 1, &mut totals, || {
                set.try_reserve(1).unwrap();
                set.capacity()
            });
            let capacity = set.capacity();
            assert!(check_reserved_insert(|| set.insert(key(ordinal))));
            assert_eq!(set.capacity(), capacity);
            assert_eq!(set.len(), ordinal + 1);
        }
        while set.len() < set.capacity() {
            let ordinal = set.len();
            check_hash_reserve::<K, ()>(set.len(), set.capacity(), 1, &mut totals, || {
                set.try_reserve(1).unwrap();
                set.capacity()
            });
            assert!(check_reserved_insert(|| set.insert(key(ordinal))));
        }
        let full_capacity = set.capacity();
        assert_eq!(set.len(), full_capacity);
        check_hash_reserve::<K, ()>(set.len(), set.capacity(), 1, &mut totals, || {
            set.try_reserve(1).unwrap();
            set.capacity()
        });
        assert!(set.capacity() > full_capacity);
        assert!(!check_reserved_insert(|| set.insert(key(0))));
        assert_eq!(set.len(), full_capacity);
        for ordinal in 0..full_capacity {
            assert!(set.contains(&key(ordinal)));
        }
        assert!(totals.1 <= totals.0);
    }

    fn check_hash_payloads(initial: usize, insertions: usize) {
        check_map(initial, insertions, |ordinal| ordinal, |ordinal| ordinal);
        check_map(initial, insertions, |ordinal| ordinal as u64, |ordinal| ordinal as u64);
        check_map(initial, insertions, |ordinal| ordinal, metadata);
        check_map(initial, insertions, |ordinal| ordinal as u64, |ordinal| [ordinal; 16]);
        check_set(initial, insertions, |ordinal| ordinal);
        check_set(initial, insertions, |ordinal| ordinal as u64);
    }

    fn check_vector<T: Copy + Debug + PartialEq>(
        initial: usize,
        length: usize,
        additional: usize,
        value: T,
    ) {
        let mut values = Vec::new();
        let initial_bound = initial.checked_mul(size_of::<T>()).unwrap();
        let (result, _, initial_requested) =
            counting_alloc::measure(|| values.try_reserve_exact(initial));
        result.unwrap();
        assert_eq!(values.capacity(), initial);
        assert_eq!(initial_requested, initial_bound);
        values.resize(length.min(initial), value);
        let previous_length = values.len();
        let required = previous_length.checked_add(additional).unwrap();
        let target = if required > values.capacity() {
            values.capacity().max(1).checked_mul(2).unwrap().max(required)
        } else {
            values.capacity()
        };
        let growth = required > values.capacity();
        let bound = if growth { target.checked_mul(size_of::<T>()).unwrap() } else { 0 };
        let cumulative_bound = initial_bound + bound;
        let (_, allocations, requested) = counting_alloc::measure(|| {
            if growth {
                values.try_reserve_exact(target - values.len()).unwrap();
            }
        });
        assert_eq!(values.capacity(), target);
        assert_eq!(requested, bound);
        assert_eq!(allocations, usize::from(growth));
        assert!(initial_requested + requested <= cumulative_bound);
        assert_eq!(values.len(), previous_length);
        assert!(values.iter().all(|item| *item == value));
        check_reserved_insert(|| values.resize(required, value));
        assert_eq!(values.len(), required);
        assert_eq!(values.capacity(), target);
        assert!(values.iter().all(|item| *item == value));
    }

    fn check_vector_payloads(initial: usize, length: usize, additional: usize) {
        check_vector(initial, length, additional, 17usize);
        check_vector(initial, length, additional, 23u64);
        check_vector(initial, length, additional, [29usize; 16]);
        check_vector(initial, length, additional, metadata(31));
    }

    fn check_shallow_clone(length: usize, spare: usize) {
        let mut child_bytes = Vec::with_capacity(length + spare);
        child_bytes.resize(length, 37);
        let node = ShallowNode { metadata: metadata(41), child_bytes };
        let bound = node.child_bytes.len();
        let (cloned, allocations, requested) = counting_alloc::measure(|| node.clone());
        assert_eq!(cloned, node);
        assert_eq!(requested, bound);
        assert_eq!(allocations, usize::from(length != 0));
        assert_eq!(cloned.child_bytes.capacity(), length);
        if length != 0 {
            assert_ne!(cloned.child_bytes.as_ptr(), node.child_bytes.as_ptr());
        }
    }

    #[test]
    fn hash_reserve_growth_and_full_replacement_fit_source_bounds() {
        for initial in [0, 1, 3, 4, 7, 8, 14, 15, 28, 29, 56, 57, 112, 113] {
            check_hash_payloads(initial, 128);
        }
    }

    #[test]
    fn exact_vector_reservation_prepays_complete_replacement() {
        for (initial, length, additional) in [
            (0, 0, 0), (0, 0, 1), (1, 0, 1), (1, 1, 1),
            (4, 2, 2), (4, 2, 4), (4, 4, 1), (4, 1, 31), (128, 127, 2),
        ] {
            check_vector_payloads(initial, length, additional);
        }
    }

    #[test]
    fn shallow_byte_vector_clone_allocates_only_live_children() {
        for length in [0, 1, 7, 8, 31, 32, 127, 128, 255, 256, 1024] {
            for spare in [0, 1, 64] {
                check_shallow_clone(length, spare);
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn generated_hash_collection_reservations_fit_source_bounds(
            initial in 0usize..64,
            insertions in 0usize..96,
        ) {
            check_hash_payloads(initial, insertions);
        }

        #[test]
        fn generated_exact_vector_growth_fits_source_bounds(
            initial in 0usize..64,
            length in 0usize..64,
            additional in 0usize..128,
        ) {
            check_vector_payloads(initial, length, additional);
        }

        #[test]
        fn generated_shallow_byte_vector_clones_fit_source_bounds(
            length in 0usize..1024,
            spare in 0usize..64,
        ) {
            check_shallow_clone(length, spare);
        }
    }
}

mod new_injection_tree_primitives {
    use std::collections::BTreeMap;

    use proptest::prelude::*;
    use shared::rust::collection_backing::{tree_height_bound, tree_insert_transient_backing};

    use super::{corpus, counting_alloc, Par};

    fn allocation_free<R>(action: impl FnOnce() -> R) -> R {
        let (result, allocations, requested) = counting_alloc::measure(action);
        assert_eq!((allocations, requested), (0, 0));
        result
    }

    fn value(ordinal: usize, simple: bool) -> Par {
        if simple {
            corpus::gint(ordinal as i64)
        } else {
            Par::default()
        }
    }

    fn check_insertions(
        keys: impl IntoIterator<Item = String>,
        simple: bool,
    ) -> (BTreeMap<String, Par>, usize) {
        let mut map = BTreeMap::new();
        let mut latest = BTreeMap::new();
        let mut maximum_allocations = 0;
        for (ordinal, key) in keys.into_iter().enumerate() {
            let before = map.len();
            let previous_ordinal = latest.insert(key.clone(), ordinal);
            let next = value(ordinal, simple);
            let (operations, backing) =
                tree_insert_transient_backing::<String, Par>(before).unwrap();
            let node_bound = tree_height_bound(before + 1);
            let (previous, allocations, requested) =
                counting_alloc::measure(|| map.insert(key, next));
            assert!(
                allocations <= node_bound,
                "insertion {ordinal} into {before} entries allocated {allocations} nodes, bound {node_bound}"
            );
            assert!(2 * (allocations + 1) <= operations);
            assert!(
                requested <= backing,
                "insertion {ordinal} into {before} entries requested {requested} bytes, bound {backing}"
            );
            assert_eq!(map.len(), before + usize::from(previous_ordinal.is_none()));
            assert_eq!(
                previous,
                previous_ordinal.map(|ordinal| value(ordinal, simple))
            );
            if previous_ordinal.is_some() {
                assert_eq!((allocations, requested), (0, 0));
            }
            allocation_free(|| drop(previous));
            maximum_allocations = maximum_allocations.max(allocations);
        }
        for (key, ordinal) in latest {
            assert_eq!(map.get(&key), Some(&value(ordinal, simple)));
        }
        (map, maximum_allocations)
    }

    fn check_teardown(map: BTreeMap<String, Par>) {
        let direct = map.clone();
        allocation_free(|| drop(direct));

        let partial = map.clone();
        let mut partial = allocation_free(|| partial.into_values());
        allocation_free(|| drop(partial.next()));
        allocation_free(|| drop(partial.next_back()));
        allocation_free(|| drop(partial));

        let expected = map.len();
        let mut values = allocation_free(|| map.into_values());
        let mut consumed = 0;
        while let Some(value) = allocation_free(|| {
            if consumed % 2 == 0 {
                values.next()
            } else {
                values.next_back()
            }
        }) {
            allocation_free(|| drop(value));
            consumed += 1;
            assert_eq!(values.len(), expected - consumed);
        }
        assert_eq!(consumed, expected);
        allocation_free(|| drop(values));
    }

    #[test]
    fn empty_maps_and_root_splits_fit_transient_backing() {
        for simple in [false, true] {
            let (empty, allocations) = check_insertions([], simple);
            assert_eq!(allocations, 0);
            check_teardown(empty);
            let (root, allocations) = check_insertions([String::new()], simple);
            assert_eq!(allocations, 1);
            check_teardown(root);
            let (split, allocations) =
                check_insertions((0..12).map(|index| format!("{index:04}")), simple);
            assert!(allocations >= 2);
            check_teardown(split);
        }
    }

    #[test]
    fn ascending_descending_and_shuffled_cascades_fit_transient_backing() {
        for simple in [false, true] {
            for order in 0..3 {
                let keys = (0..2048).map(|index| {
                    let index = match order {
                        0 => index,
                        1 => 2047 - index,
                        _ => index * 109 % 2048,
                    };
                    format!("{index:04}")
                });
                let (map, allocations) = check_insertions(keys, simple);
                assert!(allocations >= 3, "order {order} did not reach a cascade");
                assert_eq!(map.len(), 2048);
                check_teardown(map);
            }
        }
    }

    #[test]
    fn duplicate_empty_and_long_prefix_keys_fit_transient_backing() {
        let prefix = "p".repeat(4096);
        for simple in [false, true] {
            let keys = std::iter::once(String::new())
                .chain((0..128).map(|index| format!("{prefix}{index:04}")))
                .chain((0..128).rev().map(|index| format!("{prefix}{index:04}")))
                .chain([String::new(), String::new()]);
            let (map, _) = check_insertions(keys, simple);
            assert_eq!(map.len(), 129);
            check_teardown(map);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn generated_key_streams_fit_transient_backing_and_teardown(
            keys in prop::collection::vec("[a-d]{0,24}", 0..128),
            simple in any::<bool>(),
        ) {
            let (map, _) = check_insertions(keys, simple);
            check_teardown(map);
        }

        #[test]
        fn generated_common_prefix_streams_fit_transient_backing_and_teardown(
            suffixes in prop::collection::vec(0u16..256, 0..128),
            prefix_length in 0usize..1024,
            simple in any::<bool>(),
        ) {
            let prefix = "p".repeat(prefix_length);
            let keys = suffixes.into_iter().map(|suffix| format!("{prefix}{suffix:04}"));
            let (map, _) = check_insertions(keys, simple);
            check_teardown(map);
        }
    }
}

mod prost_scalar_merge_primitives {
    use std::fmt::Debug;
    use std::mem::size_of;

    use prost::bytes::{Buf, Bytes};
    use prost::encoding::{self, DecodeContext, WireType};
    use prost::DecodeError;
    use proptest::prelude::*;

    use super::counting_alloc;

    const ERROR_BACKING: usize =
        size_of::<([String; 2], u64, Vec<(&'static str, &'static str)>)>();

    struct ContiguousInput<B>(B);

    impl<B: Buf> Buf for ContiguousInput<B> {
        fn remaining(&self) -> usize { self.0.remaining() }
        fn chunk(&self) -> &[u8] { self.0.chunk() }
        fn advance(&mut self, count: usize) { self.0.advance(count); }
    }

    fn wire_type(index: usize) -> WireType {
        [WireType::Varint, WireType::SixtyFourBit, WireType::LengthDelimited,
            WireType::StartGroup, WireType::EndGroup, WireType::ThirtyTwoBit][index % 6]
    }

    fn framed(payload: &[u8], noncanonical: bool) -> Vec<u8> {
        let mut input = Vec::new();
        encoding::encode_varint(payload.len() as u64, &mut input);
        if noncanonical {
            *input.last_mut().unwrap() |= 0x80;
            input.push(0);
        }
        input.extend_from_slice(payload);
        input
    }

    fn payload<'a>(input: &'a [u8], wire: WireType) -> Option<&'a [u8]> {
        if wire != WireType::LengthDelimited { return None; }
        let mut probe = input;
        let length = encoding::decode_varint(&mut probe).ok()?;
        if length > probe.len() as u64 { return None; }
        Some(&probe[..length as usize])
    }

    fn byte_growth(capacity: usize, length: usize) -> usize {
        if length <= capacity { 0 } else { 8.max(2 * capacity).max(length) }
    }

    fn bare_error_backing() -> usize {
        let (_, allocations, requested) = counting_alloc::measure(|| {
            encoding::decode_varint(&mut &[][..]).unwrap_err()
        });
        assert_eq!(allocations, 1);
        assert!(requested <= ERROR_BACKING, "bare error requested {requested} bytes");
        requested
    }

    fn check_error(error: impl FnOnce() -> DecodeError) {
        let (error, allocations, requested) = counting_alloc::measure(error);
        assert_eq!(allocations, 1, "{error:?}");
        assert_eq!(requested, bare_error_backing(), "{error:?}");
    }

    fn check_probe(input: &[u8]) {
        let mut expected_input = input;
        let expected = encoding::decode_varint(&mut expected_input);
        let actual_input = ContiguousInput(input);
        let mut borrowed = actual_input.chunk();
        let (actual, allocations, requested) =
            counting_alloc::measure(|| encoding::decode_varint(&mut borrowed));
        assert_eq!(actual, expected);
        assert_eq!(borrowed, expected_input);
        assert_eq!(actual_input.remaining(), input.len());
        assert_eq!(actual_input.chunk().as_ptr(), input.as_ptr());
        assert_eq!(allocations, usize::from(actual.is_err()));
        assert_eq!(requested, bare_error_backing() * usize::from(actual.is_err()));
    }

    fn check_key(input: &[u8]) -> bool {
        let mut expected_input = input;
        let expected = encoding::decode_key(&mut expected_input);
        let mut actual_input = input;
        let (actual, allocations, requested) =
            counting_alloc::measure(|| encoding::decode_key(&mut actual_input));
        assert_eq!(actual, expected);
        assert_eq!(actual_input, expected_input);
        assert_eq!(allocations, usize::from(actual.is_err()));
        assert!(requested <= ERROR_BACKING);
        if actual.is_ok() {
            assert_eq!(requested, 0);
        }
        actual.is_ok()
    }

    fn check_nonrecursive_skip(input: &[u8], wire: WireType) {
        assert!(matches!(wire, WireType::ThirtyTwoBit | WireType::SixtyFourBit | WireType::EndGroup));
        let mut expected_input = input;
        let expected = encoding::skip_field(
            wire, encoding::MIN_TAG, &mut expected_input, DecodeContext::default(),
        );
        let mut actual_input = input;
        let (actual, allocations, requested) = counting_alloc::measure(|| {
            models::rust::rholang::protobuf_decoder::skip_unknown_field(
                wire, encoding::MIN_TAG, &mut actual_input,
            )
        });
        assert_eq!(actual, expected);
        assert_eq!(actual_input, expected_input);
        assert_eq!(allocations, usize::from(actual.is_err()));
        assert!(requested <= ERROR_BACKING);
        if actual.is_ok() {
            assert_eq!(requested, 0);
        }
    }

    fn check_bytes<B: Buf>(input: &[u8], source: B, wire: WireType, capacity: usize) {
        let mut actual_value = Vec::with_capacity(capacity);
        actual_value.resize(capacity.min(3), 17);
        let capacity = actual_value.capacity();
        let mut expected_value = actual_value.clone();
        let mut expected_input = input;
        let expected = encoding::bytes::merge(
            wire, &mut expected_value, &mut expected_input, DecodeContext::default(),
        );
        let body = payload(input, wire);
        let growth = body.map_or(0, |body| byte_growth(capacity, body.len()));
        let temporary = body.map_or(0, <[u8]>::len);
        let backing = growth + temporary + bare_error_backing() * usize::from(expected.is_err());
        let expected_allocations = usize::from(growth != 0)
            + usize::from(temporary != 0) + usize::from(expected.is_err());
        let mut actual_input = ContiguousInput(source);
        let (actual, allocations, requested) = counting_alloc::measure(|| {
            encoding::bytes::merge(
                wire, &mut actual_value, &mut actual_input, DecodeContext::default(),
            )
        });
        assert_eq!(actual, expected);
        assert_eq!(actual_value, expected_value);
        assert_eq!(actual_input.chunk(), expected_input);
        assert_eq!(actual_input.remaining(), expected_input.len());
        assert_eq!(actual_value.capacity(), capacity.max(growth));
        assert_eq!(allocations, expected_allocations);
        assert_eq!(requested, backing);
    }

    fn check_string<B: Buf>(input: &[u8], source: B, wire: WireType, capacity: usize) {
        let mut actual_value = String::with_capacity(capacity);
        actual_value.extend(std::iter::repeat_n('x', capacity.min(3)));
        let capacity = actual_value.capacity();
        let mut expected_value = actual_value.clone();
        let mut expected_input = input;
        let expected = encoding::string::merge(
            wire, &mut expected_value, &mut expected_input, DecodeContext::default(),
        );
        let growth = payload(input, wire).map_or(0, |body| byte_growth(capacity, body.len()));
        let backing = growth + bare_error_backing() * usize::from(expected.is_err());
        let expected_allocations = usize::from(growth != 0) + usize::from(expected.is_err());
        let mut actual_input = ContiguousInput(source);
        let (actual, allocations, requested) = counting_alloc::measure(|| {
            encoding::string::merge(
                wire, &mut actual_value, &mut actual_input, DecodeContext::default(),
            )
        });
        assert_eq!(actual, expected);
        assert_eq!(actual_value, expected_value);
        assert_eq!(actual_input.chunk(), expected_input);
        assert_eq!(actual_input.remaining(), expected_input.len());
        assert_eq!(actual_value.capacity(), capacity.max(growth));
        assert_eq!(allocations, expected_allocations);
        assert_eq!(requested, backing);
    }

    fn check_owned_and_borrowed(input: &[u8], wire: WireType, capacity: usize) {
        check_bytes(input, input, wire, capacity);
        check_bytes(input, Bytes::copy_from_slice(input), wire, capacity);
        check_string(input, input, wire, capacity);
        check_string(input, Bytes::copy_from_slice(input), wire, capacity);
    }

    fn check_repeated<'a, T: Clone + Default + Debug + PartialEq>(
        input: &'a [u8],
        wire: WireType,
        capacity: usize,
        length: usize,
        temporary_copy: bool,
        merge: impl Fn(WireType, &mut Vec<T>, &mut ContiguousInput<&'a [u8]>, DecodeContext)
            -> Result<(), DecodeError>,
    ) {
        let mut actual_values = Vec::with_capacity(capacity.max(length));
        actual_values.resize_with(length, T::default);
        let capacity = actual_values.capacity();
        let mut expected_values = actual_values.clone();
        let mut expected_input = ContiguousInput(input);
        let expected = merge(wire, &mut expected_values, &mut expected_input, DecodeContext::default());
        let body = payload(input, wire);
        let body_growth = body.map_or(0, |body| byte_growth(0, body.len()));
        let temporary = if temporary_copy { body.map_or(0, <[u8]>::len) } else { 0 };
        let outer_capacity = if expected.is_ok() && length == capacity {
            4.max(2 * capacity).max(length + 1)
        } else {
            capacity
        };
        let outer_growth = if outer_capacity > capacity { outer_capacity * size_of::<T>() } else { 0 };
        let backing = body_growth + temporary + outer_growth
            + bare_error_backing() * usize::from(expected.is_err());
        let expected_allocations = usize::from(body_growth != 0) + usize::from(temporary != 0)
            + usize::from(outer_growth != 0) + usize::from(expected.is_err());
        let mut actual_input = ContiguousInput(input);
        let (actual, allocations, requested) = counting_alloc::measure(|| {
            merge(wire, &mut actual_values, &mut actual_input, DecodeContext::default())
        });
        assert_eq!(actual, expected);
        assert_eq!(actual_values, expected_values);
        assert_eq!(actual_input.chunk(), expected_input.chunk());
        assert_eq!(actual_values.capacity(), outer_capacity);
        assert_eq!(allocations, expected_allocations);
        assert_eq!(requested, backing);
    }

    fn check_repeated_payloads(input: &[u8], wire: WireType, capacity: usize, length: usize) {
        check_repeated::<String>(input, wire, capacity, length, false, encoding::string::merge_repeated);
        check_repeated::<Vec<u8>>(input, wire, capacity, length, true, encoding::bytes::merge_repeated);
    }

    fn check_scalar_roster(input: &[u8], wire: WireType) {
        macro_rules! check {
            ($module:ident, $initial:expr) => {{
                let mut expected_value = $initial;
                let mut expected_input = input;
                let expected = encoding::$module::merge(
                    wire, &mut expected_value, &mut expected_input, DecodeContext::default(),
                );
                let mut actual_value = $initial;
                let mut actual_input = ContiguousInput(input);
                let (actual, allocations, requested) = counting_alloc::measure(|| {
                    encoding::$module::merge(
                        wire, &mut actual_value, &mut actual_input, DecodeContext::default(),
                    )
                });
                assert_eq!(actual, expected);
                assert_eq!(actual_value, expected_value);
                assert_eq!(actual_input.chunk(), expected_input);
                assert_eq!(allocations, usize::from(expected.is_err()));
                assert_eq!(requested, bare_error_backing() * usize::from(expected.is_err()));
            }};
        }
        check!(bool, false);
        check!(int32, -17i32);
        check!(uint32, 17u32);
        check!(int64, -17i64);
        check!(uint64, 17u64);
        check!(sint32, -17i32);
        check!(sint64, -17i64);
        check!(fixed32, 17u32);
        check!(fixed64, 17u64);
    }

    #[test]
    fn bare_prost_errors_and_borrowed_probes_fit_the_pinned_box_bound() {
        assert_eq!(size_of::<usize>(), 8);
        println!("bare Prost DecodeError requested {} bytes, bound {ERROR_BACKING}", bare_error_backing());
        check_error(|| encoding::decode_varint(&mut &[][..]).unwrap_err());
        check_error(|| encoding::decode_varint(&mut &[0xff; 10][..]).unwrap_err());
        check_error(|| encoding::check_wire_type(WireType::Varint, WireType::StartGroup).unwrap_err());
        check_error(|| encoding::fixed64::merge(
            WireType::SixtyFourBit, &mut 0u64, &mut &[0; 7][..], DecodeContext::default(),
        ).unwrap_err());
        check_error(|| DecodeError::new_unexpected_type_url(String::new(), String::new()));
        let mut value = String::with_capacity(8);
        check_error(|| encoding::string::merge(
            WireType::LengthDelimited, &mut value, &mut &[1, 0xff][..], DecodeContext::default(),
        ).unwrap_err());
        assert!(value.is_empty());
        for input in [&[][..], &[0], &[0x80], &[0x80, 0], &[0x81, 0], &[0xff; 10], &[0x80; 11],
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1],
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 2]] {
            check_probe(input);
        }
    }

    #[test]
    fn initial_protobuf_primitives_fit_the_error_backing_bound() {
        let fixtures: &[(&[u8], bool, bool)] = &[
            (&[], false, false),
            (&[0], true, false),
            (&[8], true, true),
            (&[0x80], false, false),
            (&[0x80, 0], true, false),
            (&[0x88, 0], true, true),
            (&[0x0e], true, false),
            (&[0x0f], true, false),
            (&[0xfd, 0xff], false, false),
            (&[0xfd, 0xff, 0xff, 0xff, 0x0f], true, true),
            (&[0x80, 0x80, 0x80, 0x80, 0x10], true, false),
            (&[0xff; 10], false, false),
            (&[0x80; 11], false, false),
            (&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1], true, false),
            (&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 2], false, false),
            (&[0x88, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0], true, true),
        ];
        for &(input, varint_valid, key_valid) in fixtures {
            check_probe(input);
            assert_eq!(encoding::decode_varint(&mut &input[..]).is_ok(), varint_valid);
            assert_eq!(check_key(input), key_valid);
        }
        for length in 0..=8 {
            let input = vec![0xa5; length];
            check_nonrecursive_skip(&input, WireType::ThirtyTwoBit);
            check_nonrecursive_skip(&input, WireType::SixtyFourBit);
            check_nonrecursive_skip(&input, WireType::EndGroup);
        }
    }

    #[test]
    fn bytes_default_copy_and_string_growth_match_the_pinned_allocations() {
        for length in [0, 1, 7, 8, 9, 15, 16, 17, 127, 128] {
            let body = vec![b'a'; length];
            for capacity in [0, 1, 7, 8, 16, 64, 128] {
                for noncanonical in [false, true] {
                    check_owned_and_borrowed(&framed(&body, noncanonical), WireType::LengthDelimited, capacity);
                }
            }
        }
        for body in [&b"\xc3\xa9\xf0\x9f\x8c\x8d"[..], &[0xff], &[0xc0, 0x80], &[0xed, 0xa0, 0x80],
            &[0xf4, 0x90, 0x80, 0x80], &[0xe2, 0x82]] {
            for capacity in [0, 1, 8, 32] {
                check_owned_and_borrowed(&framed(body, false), WireType::LengthDelimited, capacity);
            }
        }
    }

    #[test]
    fn repeated_payloads_pay_for_the_new_body_and_outer_growth_only() {
        for capacity in [0usize, 1, 3, 4, 7, 8, 16] {
            for length in [0, capacity.saturating_sub(1), capacity] {
                for body in [&[][..], &b"abc"[..], &[0xff], &b"\xc3\xa9"[..]] {
                    for wire in [WireType::LengthDelimited, WireType::Varint] {
                        check_repeated_payloads(&framed(body, true), wire, capacity, length);
                    }
                }
            }
        }
    }

    #[test]
    fn malformed_merge_fixtures_preserve_cursor_and_destination_behavior() {
        let fixtures: &[(&[u8], WireType, usize, Option<&[u8]>)] = &[
            (&[], WireType::LengthDelimited, 0, None),
            (&[0x80], WireType::LengthDelimited, 1, None),
            (&[0x80, 0x80], WireType::LengthDelimited, 2, None),
            (&[0xff; 10], WireType::LengthDelimited, 10, None),
            (&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1],
                WireType::LengthDelimited, 10, None),
            (&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 2],
                WireType::LengthDelimited, 0, None),
            (&[0x80; 11], WireType::LengthDelimited, 0, None),
            (&[3, b'a', b'b'], WireType::LengthDelimited, 1, None),
            (&[0x81, 0, 0xff], WireType::LengthDelimited, 3, Some(&[0xff])),
            (&[0x81, 0, b'a'], WireType::LengthDelimited, 3, Some(b"a")),
            (&[0x80, 0, 17], WireType::LengthDelimited, 2, Some(&[])),
            (&[0x80, 0x80], WireType::Varint, 0, None),
        ];
        for &(input, wire, consumed, expected_body) in fixtures {
            check_probe(input);
            check_owned_and_borrowed(input, wire, 8);
            check_repeated_payloads(input, wire, 4, 4);
            check_scalar_roster(input, wire);
            let mut bytes = b"old".to_vec();
            let mut bytes_input = ContiguousInput(input);
            let bytes_result = encoding::bytes::merge(wire, &mut bytes, &mut bytes_input, DecodeContext::default());
            assert_eq!(input.len() - bytes_input.remaining(), consumed);
            assert_eq!(bytes_result.is_ok(), expected_body.is_some());
            assert_eq!(bytes.as_slice(), expected_body.unwrap_or(b"old"));
            let mut string = "old".to_owned();
            let mut string_input = ContiguousInput(input);
            let string_result = encoding::string::merge(wire, &mut string, &mut string_input, DecodeContext::default());
            assert_eq!(input.len() - string_input.remaining(), consumed);
            let expected_string = expected_body.and_then(|body| std::str::from_utf8(body).ok());
            assert_eq!(string_result.is_ok(), expected_string.is_some());
            assert_eq!(string, expected_string.unwrap_or_default());
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn generated_initial_primitives_allocate_only_the_error_box(
            input in prop::collection::vec(any::<u8>(), 0..24),
            value in any::<u64>(),
            tag in 1u32..(1u32 << 29),
            wire in 0usize..6,
            fixed_input in prop::collection::vec(any::<u8>(), 0..9),
        ) {
            let mut encoded_varint = Vec::new();
            encoding::encode_varint(value, &mut encoded_varint);
            let mut encoded_key = Vec::new();
            encoding::encode_key(tag, wire_type(wire), &mut encoded_key);
            let truncated = &encoded_varint[..encoded_varint.len() - 1];
            check_probe(&input);
            check_key(&input);
            check_probe(&encoded_varint);
            check_probe(truncated);
            prop_assert!(check_key(&encoded_key));
            prop_assert!(!check_key(truncated));
            check_nonrecursive_skip(&fixed_input, WireType::ThirtyTwoBit);
            check_nonrecursive_skip(&fixed_input, WireType::SixtyFourBit);
            check_nonrecursive_skip(&fixed_input, WireType::EndGroup);
        }

        #[test]
        fn generated_scalar_errors_and_probes_preserve_exact_results(
            input in prop::collection::vec(any::<u8>(), 0..48),
            wire in 0usize..6,
        ) {
            check_probe(&input);
            check_scalar_roster(&input, wire_type(wire));
        }

        #[test]
        fn generated_bytes_and_strings_fit_field_local_backing_bounds(
            body in prop::collection::vec(any::<u8>(), 0..128),
            capacity in 0usize..96,
            noncanonical in any::<bool>(),
            wire in 0usize..6,
        ) {
            check_owned_and_borrowed(&framed(&body, noncanonical), WireType::LengthDelimited, capacity);
            check_owned_and_borrowed(&body, wire_type(wire), capacity);
        }

        #[test]
        fn generated_repeated_payloads_fit_body_and_outer_bounds(
            body in prop::collection::vec(any::<u8>(), 0..128),
            capacity in 0usize..16,
            length in 0usize..16,
            noncanonical in any::<bool>(),
            wire in 0usize..6,
        ) {
            check_repeated_payloads(&framed(&body, noncanonical), WireType::LengthDelimited, capacity, length);
            check_repeated_payloads(&body, wire_type(wire), capacity, length);
        }
    }
}

mod pending_snapshot_sharing {
    use std::mem::size_of;

    use models::rust::epathmap_trie_codec;
    use prost::bytes::{Buf, Bytes, BytesMut};
    use proptest::prelude::*;

    use super::counting_alloc;

    fn check_slice(snapshot: Bytes, start: usize, end: usize) {
        let expected = &snapshot[start..end];
        let expected_pointer = snapshot.as_ptr().wrapping_add(start);
        let backing_bound = if start == end { 0 } else { 3 * size_of::<usize>() };
        let (first, allocations, requested) = counting_alloc::measure(|| snapshot.slice(start..end));
        assert_eq!(first.as_ref(), expected);
        assert_eq!(first.as_ptr(), expected_pointer);
        assert!(allocations <= usize::from(start != end));
        assert!(requested <= backing_bound);
        let (second, allocations, requested) = counting_alloc::measure(|| snapshot.slice(start..end));
        assert_eq!(second, first);
        assert_eq!(second.as_ptr(), expected_pointer);
        assert_eq!(allocations, 0);
        assert_eq!(requested, 0);
    }

    fn check_owners(input: &[u8], start: usize, end: usize) {
        check_slice(Bytes::copy_from_slice(input), start, end);
        check_slice(Bytes::from(input.to_vec().into_boxed_slice()), start, end);
        check_slice(Bytes::from_owner(input.to_vec().into_boxed_slice()), start, end);
        let mut spare = Vec::with_capacity(input.len() + 7);
        spare.extend_from_slice(input);
        check_slice(Bytes::from(spare), start, end);
        let mut shared = BytesMut::with_capacity(input.len() + 7);
        shared.extend_from_slice(input);
        let region = shared.split_to(input.len()).freeze();
        check_slice(region, start, end);
    }

    #[test]
    fn bytes_slices_share_payload_with_at_most_one_header_allocation() {
        for length in [0, 1, 2, 7, 8, 31, 32, 255, 256, 1024] {
            let input: Vec<u8> = (0..length).map(|index| index as u8).collect();
            for (start, end) in [(0, 0), (0, length), (length / 2, length), (length, length)] {
                check_owners(&input, start, end);
            }
        }
        check_slice(Bytes::from_static(b"shared static snapshot"), 2, 9);
        let promoted = Bytes::from(vec![17; 1024].into_boxed_slice());
        let (view, allocations, requested) = counting_alloc::measure(|| promoted.slice(1..1023));
        assert_eq!(view.as_ptr(), promoted.as_ptr().wrapping_add(1));
        assert_eq!(allocations, 1);
        assert_eq!(requested, 3 * size_of::<usize>());
    }

    fn check_region(input: &[u8], length: usize) {
        let mut borrowed = input;
        let (copied, allocations, requested) =
            counting_alloc::measure(|| borrowed.copy_to_bytes(length));
        assert_eq!(copied.as_ref(), &input[..length]);
        assert_eq!(borrowed, &input[length..]);
        assert_eq!(allocations, usize::from(length != 0));
        assert_eq!(requested, length);
        if length != 0 {
            assert_ne!(copied.as_ptr(), input.as_ptr());
        }

        let mut spare = Vec::with_capacity(input.len() + 7);
        spare.extend_from_slice(input);
        let mut shared = BytesMut::with_capacity(input.len() + 7);
        shared.extend_from_slice(input);
        let owners = [
            Bytes::copy_from_slice(input),
            Bytes::from(input.to_vec().into_boxed_slice()),
            Bytes::from_owner(input.to_vec().into_boxed_slice()),
            Bytes::from(spare),
            shared.split_to(input.len()).freeze(),
        ];
        for mut owner in owners {
            let pointer = owner.as_ptr();
            let (region, allocations, requested) =
                counting_alloc::measure(|| owner.split_to(length));
            let promotion = usize::from(length != 0 && length < input.len());
            assert_eq!(region.as_ref(), &input[..length]);
            assert_eq!(owner.as_ref(), &input[length..]);
            assert_eq!(region.as_ptr(), pointer);
            assert_eq!(owner.as_ptr(), pointer.wrapping_add(length));
            assert!(allocations <= promotion);
            assert!(requested <= promotion * 3 * size_of::<usize>());
        }

        let mut owner = Bytes::copy_from_slice(input);
        let pointer = owner.as_ptr();
        let (region, allocations, requested) =
            counting_alloc::measure(|| owner.copy_to_bytes(length));
        assert_eq!(region.as_ref(), &input[..length]);
        assert_eq!(owner.as_ref(), &input[length..]);
        assert_eq!(region.as_ptr(), pointer);
        assert!(allocations <= usize::from(length != 0 && length < input.len()));
        assert!(requested <= usize::from(length != 0) * 3 * size_of::<usize>());
    }

    fn check_error_conversion(input: &[u8]) {
        let error = epathmap_trie_codec::decode(input).err().expect("invalid snapshot");
        let message_length = error.to_string().len();
        let expected = prost::DecodeError::from(error.clone());
        let (converted, allocations, requested) =
            counting_alloc::measure(|| prost::DecodeError::from(error));
        let error_backing = size_of::<([String; 2], u64, Vec<(&'static str, &'static str)>)>();
        let format_backing = if message_length <= 24 {
            48
        } else {
            48 + 96.max(24 + message_length)
        };
        assert_eq!(converted, expected);
        assert_eq!(allocations, if message_length <= 24 { 2 } else { 3 });
        assert!(requested <= format_backing + error_backing);
    }

    #[test]
    fn region_copies_and_splits_fit_their_distinct_backing_bounds() {
        for size in [0, 1, 2, 7, 8, 23, 24, 31, 32, 255, 1024] {
            let input: Vec<_> = (0..size).map(|index| index as u8).collect();
            for length in [0, size / 2, size] {
                check_region(&input, length);
            }
        }
        let mut owner = Bytes::from_static(b"static snapshot bytes");
        let (region, allocations, requested) = counting_alloc::measure(|| owner.split_to(7));
        assert_eq!(region.as_ref(), b"static ");
        assert_eq!(owner.as_ref(), b"snapshot bytes");
        assert_eq!((allocations, requested), (0, 0));
    }

    #[test]
    fn original_trie_error_conversion_fits_format_and_box_credit() {
        for input in [b"".as_slice(), b"NOPE", b"EPM1\xff", b"EPM1\x01\xff"] {
            check_error_conversion(input);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn generated_shared_ranges_fit_the_header_bound(
            input in prop::collection::vec(any::<u8>(), 0..1024),
            left in any::<usize>(),
            right in any::<usize>(),
        ) {
            let left = left % (input.len() + 1);
            let right = right % (input.len() + 1);
            check_owners(&input, left.min(right), left.max(right));
        }

        #[test]
        fn generated_region_copies_and_splits_fit_backing_credit(
            input in prop::collection::vec(any::<u8>(), 0..1024),
            length in any::<usize>(),
        ) {
            check_region(&input, length % (input.len() + 1));
        }

        #[test]
        fn generated_trie_error_conversion_fits_format_and_box_credit(count in 1u64..=u64::MAX) {
            let mut input = b"EPM1\x01\x02\x00".to_vec();
            prost::encoding::encode_varint(count, &mut input);
            check_error_conversion(&input);
        }
    }
}

mod non_pathmap_public_cleanup {
    use std::mem::size_of;

    use models::rhoapi::cost_signature::Value as CostValue;
    use models::rhoapi::{CostSignature, CostSignatureCompound, CostSignedTerm, CostStack};
    use models::rust::rholang::par_children::{
        dismantle, dismantle_cost_signature, dismantle_cost_stack,
    };
    use proptest::prelude::*;

    use super::{corpus, counting_alloc, wide, EList, Expr, ExprInstance, Par};

    #[derive(Clone, Copy, Debug, Default)]
    struct Counts {
        pars: usize,
        signatures: usize,
        stacks: usize,
    }

    impl Counts {
        fn add(&mut self, other: Self) {
            self.pars += other.pars;
            self.signatures += other.signatures;
            self.stacks += other.stacks;
        }

        fn backing(self, standalone_signatures: usize) -> usize {
            4 * size_of::<Par>() * (self.pars + standalone_signatures)
                + 4 * size_of::<CostSignature>() * self.signatures
                + 4 * size_of::<CostStack>() * (self.stacks + standalone_signatures)
                + standalone_signatures * (size_of::<CostSignature>() + size_of::<CostStack>())
        }
    }

    fn check<T>(
        value: T,
        counts: Counts,
        standalone_signatures: usize,
        dispose: impl FnOnce(T),
    ) -> (usize, usize) {
        assert!(standalone_signatures <= counts.signatures);
        let bound = counts.backing(standalone_signatures);
        let acquisition_bound = 4 * size_of::<Par>() * (counts.pars + counts.signatures)
            + 5 * size_of::<CostSignature>() * counts.signatures
            + size_of::<CostStack>() * (4 * counts.stacks + 5 * counts.signatures);
        assert!(bound <= acquisition_bound);
        let ((), allocations, requested) = counting_alloc::measure(|| dispose(value));
        let allocation_bound =
            counts.pars + counts.signatures + counts.stacks + 4 * standalone_signatures;
        assert!(
            allocations <= allocation_bound,
            "non-PathMap public cleanup used {allocations} allocations, bound {allocation_bound}, counts {counts:?}, standalone signatures {standalone_signatures}"
        );
        assert!(
            requested <= bound,
            "non-PathMap public cleanup requested {requested} bytes, bound {bound}, counts {counts:?}, standalone signatures {standalone_signatures}"
        );
        (allocations, requested)
    }

    fn unit() -> CostSignature {
        CostSignature {
            value: Some(CostValue::Unit(true)),
        }
    }

    fn par_fixture(depth: usize, width: usize) -> (Par, Counts) {
        let mut value = wide(width);
        for _ in 0..depth {
            value = models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::EListBody(EList {
                        ps: vec![value],
                        ..Default::default()
                    })),
                }],
                ..Default::default()
            };
        }
        (value, Counts {
            pars: depth + width + 1,
            ..Counts::default()
        })
    }

    fn signature_fixture(depth: usize, width: usize) -> (CostSignature, Counts) {
        let (par, mut counts) = par_fixture(depth, width);
        let mut value = CostSignature {
            value: Some(CostValue::Quote(par)),
        };
        counts.signatures += 1;
        if width != 0 {
            let mut elements = vec![value];
            for index in 0..width {
                elements.push(if index % 2 == 0 {
                    counts.pars += 1;
                    CostSignature {
                        value: Some(CostValue::Name(corpus::gint(index as i64))),
                    }
                } else {
                    unit()
                });
            }
            value = CostSignature {
                value: Some(CostValue::Compound(CostSignatureCompound { elements })),
            };
            counts.signatures += width + 1;
        }
        for _ in 0..depth {
            value = CostSignature {
                value: Some(CostValue::Compound(CostSignatureCompound {
                    elements: vec![value],
                })),
            };
            counts.signatures += 1;
        }
        (value, counts)
    }

    fn stack_fixture(depth: usize, width: usize) -> (CostStack, Counts) {
        let (signature, mut counts) = signature_fixture(depth, width);
        let mut value = CostStack {
            cells: vec![signature],
            parallel_tail: (0..width)
                .map(|_| CostStack {
                    cells: vec![unit()],
                    parallel_tail: Vec::new(),
                })
                .collect(),
        };
        counts.signatures += width;
        counts.stacks += width + 1;
        for _ in 0..depth {
            value = CostStack {
                cells: vec![unit()],
                parallel_tail: vec![value],
            };
            counts.signatures += 1;
            counts.stacks += 1;
        }
        (value, counts)
    }

    fn mixed_par_fixture(depth: usize, width: usize) -> (Par, Counts) {
        let (mut value, mut counts) = par_fixture(depth, width);
        let (signature, signature_counts) = signature_fixture(depth, width);
        let (stack, stack_counts) = stack_fixture(depth, width);
        value.cost_signed_terms = vec![CostSignedTerm {
            body: Some(Par::default()),
            signature: Some(signature),
        }];
        value.cost_stacks = vec![stack];
        counts.pars += 1;
        counts.add(signature_counts);
        counts.add(stack_counts);
        (value, counts)
    }

    fn check_shapes(depth: usize, width: usize) {
        for explicit in [false, true] {
            for fixture in [par_fixture, mixed_par_fixture] {
                let (value, counts) = fixture(depth, width);
                check(value, counts, 0, |value| {
                    if explicit {
                        dismantle(value);
                    } else {
                        drop(value);
                    }
                });
            }
            let (value, counts) = signature_fixture(depth, width);
            check(value, counts, 1, |value| {
                if explicit {
                    dismantle_cost_signature(value);
                } else {
                    drop(value);
                }
            });
            let (value, counts) = stack_fixture(depth, width);
            check(value, counts, 0, |value| {
                if explicit {
                    dismantle_cost_stack(value);
                } else {
                    drop(value);
                }
            });
        }
    }

    #[test]
    fn fresh_cleanup_worklists_and_signature_wrappers_are_measured() {
        let par_counts = Counts {
            pars: 1,
            ..Counts::default()
        };
        assert_eq!(check(Par::default(), par_counts, 0, drop), (0, 0));
        assert_eq!(
            check(Par::default(), par_counts, 0, dismantle),
            (1, size_of::<Par>())
        );
        let signature_counts = Counts {
            signatures: 1,
            ..Counts::default()
        };
        assert_eq!(
            check(CostSignature::default(), signature_counts, 1, drop),
            (0, 0)
        );
        let signature_backing =
            size_of::<Par>() + 2 * size_of::<CostSignature>() + 2 * size_of::<CostStack>();
        assert_eq!(
            check(unit(), signature_counts, 1, drop),
            (5, signature_backing)
        );
        assert_eq!(
            check(unit(), signature_counts, 1, dismantle_cost_signature),
            (5, signature_backing)
        );
        let stack_counts = Counts {
            stacks: 1,
            ..Counts::default()
        };
        assert_eq!(check(CostStack::default(), stack_counts, 0, drop), (0, 0));
        assert_eq!(
            check(CostStack::default(), stack_counts, 0, dismantle_cost_stack),
            (1, size_of::<CostStack>())
        );
    }

    #[test]
    fn wide_and_deep_local_cleanup_fits_queue_family_backing() {
        for size in [0, 1, 3, 4, 7, 8, 31, 32, 255, 256] {
            for (depth, width) in [(0, size), (size, 0), (size.min(32), size.min(32))] {
                check_shapes(depth, width);
            }
        }
    }

    #[test]
    fn independent_signature_and_stack_drops_pay_each_fresh_queue() {
        for roots in [0, 1, 2, 4, 8, 31, 32, 128] {
            let signatures: Vec<_> = (0..roots).map(|_| unit()).collect();
            let counts = Counts {
                signatures: roots,
                ..Counts::default()
            };
            assert_eq!(
                check(signatures, counts, roots, drop),
                (
                    5 * roots,
                    roots
                        * (size_of::<Par>()
                            + 2 * size_of::<CostSignature>()
                            + 2 * size_of::<CostStack>())
                )
            );
            let stacks: Vec<_> = (0..roots)
                .map(|_| CostStack {
                    cells: vec![unit()],
                    parallel_tail: Vec::new(),
                })
                .collect();
            let counts = Counts {
                signatures: roots,
                stacks: roots,
                ..Counts::default()
            };
            assert_eq!(
                check(stacks, counts, 0, drop),
                (
                    2 * roots,
                    roots * (size_of::<CostSignature>() + size_of::<CostStack>())
                )
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn generated_non_pathmap_local_cleanup_fits_composed_queue_bounds(
            depth in 0usize..48,
            width in 0usize..48,
            roots in 0usize..8,
        ) {
            check_shapes(depth, width);
            let mut counts = Counts::default();
            let signatures: Vec<_> = (0..roots)
                .map(|index| {
                    let (value, local) = signature_fixture(depth / 2 + index, width / 2);
                    counts.add(local);
                    value
                })
                .collect();
            check(signatures, counts, roots, drop);
            let mut counts = Counts::default();
            let stacks: Vec<_> = (0..roots)
                .map(|index| {
                    let (value, local) = stack_fixture(depth / 2 + index, width / 2);
                    counts.add(local);
                    value
                })
                .collect();
            check(stacks, counts, 0, drop);
        }
    }
}

#[cfg(target_pointer_width = "64")]
mod cold_empty_epathmap_primitives {
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::Var;
    use models::rust::rhoapi_ext::EPathMap;

    use super::{counting_alloc, Par};

    fn check_construction_and_drop(make: impl FnOnce() -> EPathMap) {
        let (value, allocations, requested) = counting_alloc::measure(make);
        assert_eq!((allocations, requested), (2, 104));
        let ((), allocations, requested) = counting_alloc::measure(|| drop(value));
        assert_eq!((allocations, requested), (0, 0));
    }

    #[test]
    fn cold_default_uses_two_cache_allocations_on_pinned_64_bit_target() {
        check_construction_and_drop(EPathMap::default);
    }

    #[test]
    fn cold_empty_constructor_moves_metadata_without_additional_allocations() {
        for length in [0, 1, 31, 1024] {
            for connective_used in [false, true] {
                for has_remainder in [false, true] {
                    let entries = Vec::<Par>::new();
                    let locally_free = vec![0x5a; length];
                    let remainder = has_remainder.then_some(Var {
                        var_instance: Some(VarInstance::BoundVar(7)),
                    });
                    check_construction_and_drop(|| {
                        EPathMap::new(entries, locally_free, connective_used, remainder)
                    });
                }
            }
        }
    }
}

mod fresh_pathmap_reconstruction {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt::Debug;
    use std::mem::size_of;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use pathmap::PathMap;
    use proptest::prelude::*;

    use super::{corpus, counting_alloc, Par};

    fn check<V: Clone + Debug + PartialEq + Send + Sync + Unpin + 'static>(
        endpoints: &BTreeMap<Vec<u8>, bool>,
        value: impl Fn(usize) -> V,
    ) {
        let mut map = PathMap::new();
        let payload = size_of::<V>();
        let slot = payload + 24;
        let boxed = if payload > 8 { payload } else { 0 };
        for (ordinal, (path, has_value)) in endpoints.iter().enumerate() {
            if *has_value {
                let value = value(ordinal);
                let (previous, _, requested) = catch_unwind(AssertUnwindSafe(|| {
                    counting_alloc::measure(|| map.insert(path, value))
                }))
                .unwrap_or_else(|_| {
                    panic!(
                        "insert panicked at ordinal {ordinal}, path {path:?}, payload {payload}"
                    )
                });
                assert!(previous.is_none());
                let bound = 288 + 160 * path.len() + 384 * slot + (path.len() + 1) * boxed;
                assert!(requested <= bound,
                    "insert requested {requested}, bound {bound}, path {path:?}, payload {payload}");
            } else {
                assert!(
                    endpoints
                        .keys()
                        .skip(ordinal + 1)
                        .all(|later| !later.starts_with(path)),
                    "valueless endpoint {path:?} prefixes a later endpoint"
                );
                let (_, _, requested) = catch_unwind(AssertUnwindSafe(|| {
                    counting_alloc::measure(|| map.create_path(path))
                }))
                .unwrap_or_else(|_| {
                    panic!(
                        "create_path panicked at ordinal {ordinal}, path {path:?}, payload {payload}"
                    )
                });
                let bound = 512 + 161 * path.len() + 384 * slot;
                assert!(requested <= bound,
                    "create_path requested {requested}, bound {bound}, path {path:?}, payload {payload}");
            }
        }
        assert_eq!(
            map.val_count(),
            endpoints.values().filter(|has_value| **has_value).count()
        );
        for (ordinal, (path, has_value)) in endpoints.iter().enumerate() {
            assert!(map.path_exists_at(path), "missing endpoint {path:?}");
            let expected = has_value.then(|| value(ordinal));
            assert_eq!(map.get_val_at(path), expected.as_ref(), "path {path:?}");
        }
    }

    fn check_endpoint_payloads(endpoints: &BTreeMap<Vec<u8>, bool>) {
        check(endpoints, |_| ());
        check::<Par>(endpoints, |ordinal| corpus::gint(ordinal as i64));
    }

    fn check_payloads(paths: &BTreeSet<Vec<u8>>) {
        check_endpoint_payloads(&paths.iter().cloned().map(|path| (path, true)).collect());
    }

    fn terminal_valueless_endpoints(
        mut endpoints: BTreeMap<Vec<u8>, bool>,
    ) -> BTreeMap<Vec<u8>, bool> {
        let value_prefixes: Vec<_> = endpoints
            .keys()
            .zip(endpoints.keys().skip(1))
            .filter(|(path, next)| next.starts_with(path))
            .map(|(path, _)| path.clone())
            .collect();
        for path in value_prefixes {
            endpoints.insert(path, true);
        }
        endpoints
    }

    #[test]
    fn empty_and_root_endpoints_fit_source_bounds() {
        check_endpoint_payloads(&BTreeMap::new());
        check_endpoint_payloads(&BTreeMap::from([(Vec::new(), false)]));
        check_endpoint_payloads(&BTreeMap::from([(Vec::new(), true)]));
    }

    #[test]
    fn dense_growth_and_value_prefixes_fit_source_bounds() {
        let mut paths = BTreeSet::new();
        paths.insert(Vec::new());
        for byte in 0..=u8::MAX {
            paths.insert(vec![byte]);
            paths.insert(vec![byte, 0]);
            paths.insert(vec![byte, 255]);
        }
        check_payloads(&paths);
    }

    #[test]
    fn dense_values_and_dangling_leaves_fit_source_bounds() {
        let mut endpoints = BTreeMap::from([(Vec::new(), true)]);
        for byte in 0..=u8::MAX {
            endpoints.insert(vec![byte], true);
            endpoints.insert(vec![byte, 0], byte % 2 == 0);
            endpoints.insert(vec![byte, 255], byte % 2 != 0);
        }
        check_endpoint_payloads(&endpoints);
    }

    #[test]
    fn compressed_lines_and_zipper_growth_fit_source_bounds() {
        let mut paths = BTreeSet::new();
        for length in [1, 2, 41, 42, 43, 83, 84, 85, 127, 128, 129, 256, 1024, 4096] {
            let mut path = vec![17; length];
            paths.insert(path.clone());
            path[length - 1] = 18;
            paths.insert(path);
        }
        check_payloads(&paths);
        let endpoints = terminal_valueless_endpoints(
            paths.into_iter().map(|path| (path, false)).collect(),
        );
        check_endpoint_payloads(&endpoints);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn generated_fresh_reconstruction_fits_source_bounds(
            paths in proptest::collection::btree_set(
                proptest::collection::vec(any::<u8>(), 0..96), 0..96,
            ),
        ) {
            check_payloads(&paths);
        }

        #[test]
        fn generated_mixed_value_and_dangling_endpoints_fit_source_bounds(
            endpoints in proptest::collection::btree_map(
                proptest::collection::vec(any::<u8>(), 0..96), any::<bool>(), 0..96,
            ).prop_map(terminal_valueless_endpoints),
        ) {
            check_endpoint_payloads(&endpoints);
        }
    }
}

/// ★★ **The steady-state allocation table** — the acceptance criterion, measured
/// per shape and against the derived path in the same run.
///
/// | shape | machine | derived |
/// |---|---|---|
/// | ordinary terms (`gint`, wide, deep, non-ground `EPathMap`) | **0** | 1 every call, forever |
/// | lf-bearing `EPathMap` (the CBR-043 memo path) | **0** | 1 every call, forever |
/// | ground `EPathMap` | 40 | 77 |
///
/// The zero is not an approximation: the output buffer is `clear()`ed and
/// reused, the op-stack *allocation* is pooled per thread, and every `FieldVal`
/// borrows. The derived path allocates a fresh exactly-sized `Vec` on every
/// call and can never converge, because it must hand out ownership.
///
/// ⚠ The ground-`EPathMap` row is **not** the encoder allocating: it is
/// `ground_canonical_ps`, which constructs the canonical entry order (deduped
/// and recursively canonical) so a ground map's event-hash preimage is a pure
/// function of its entry set. That cost is inherent to the normalization and
/// the derived `Serialize` pays it too — nearly twice over, which is why the
/// machine's number is the smaller one.
#[test]
fn the_steady_state_allocation_table() {
    let ordinary: Vec<(&str, Par)> = vec![
        ("gint", corpus::gint(1)),
        ("wide(64)", wide(64)),
        ("deep(16)", deep(16)),
        (
            "nonground_pathmap",
            pathmap_par(corpus::nonground_pathmap()),
        ),
        // ★★ THE CBR-043 ROW, and it is not a duplicate of the one above it.
        //
        // `nonground_pathmap`'s entries are `GInt`s — `entries_stable`, so
        // `EntryTrie::bincode_path_stream` answers O(1) off the fold and the memo
        // cell is never touched. This shape's entry is lf-bearing and NOT
        // `eval_stable`, which is the only route into `blanked_stream()`: the
        // blanked entries are built through the cold-store codec pair, a
        // throwaway trie is filed, its key stream is compared, and the result is
        // memoized.
        //
        // ⚠ That work is real and it allocates — ONCE. What this row measures is
        // that it stays BEHIND the memo: a warm encode of an lf-bearing map must
        // still borrow, exactly like every other shape. An implementation that
        // recomputed the blanked stream per encode would pass every correctness
        // test in the tree and fail here, which is what this file is for.
        ("lf_bearing_pathmap", pathmap_par(lf_bearing_pathmap())),
    ];
    for (name, par) in &ordinary {
        warm(par);
        let (len, allocations, bytes) = counting_alloc::measure(|| with_encoded(par, <[u8]>::len));
        let (_, oracle_allocations, oracle_bytes) =
            counting_alloc::measure(|| bincode::serialize(par).expect("oracle").len());
        println!(
            "  {name:20} {len:8} B | machine {allocations:3} allocs {bytes:7} B \
             | derived {oracle_allocations:3} allocs {oracle_bytes:7} B"
        );
        assert_eq!(
            allocations, 0,
            "`{name}`: a warm encode made {allocations} allocations ({bytes} B). The steady \
             state must be ZERO — the output buffer is reused, the op-stack allocation is \
             pooled, and every `FieldVal` borrows. A non-zero count means one of those three \
             stopped holding."
        );
        assert!(
            oracle_allocations > 0,
            "`{name}`: the derived path must allocate, or the comparison is measuring nothing"
        );
    }

    // ★ The ground map: the canonicalization did not stop happening — it MOVED
    // INTO THE VALUE, so the encoder does not perform it and does not allocate
    // for it. Asserted as a RELATION, not as a magic number.
    let ground = pathmap_par(corpus::ground_pathmap());
    warm(&ground);
    let (_, allocations, bytes) = counting_alloc::measure(|| with_encoded(&ground, <[u8]>::len));
    let (_, oracle_allocations, oracle_bytes) =
        counting_alloc::measure(|| bincode::serialize(&ground).expect("oracle").len());
    println!(
        "  {:20} {:8} | machine {allocations:3} allocs {bytes:7} B \
         | derived {oracle_allocations:3} allocs {oracle_bytes:7} B",
        "ground_pathmap", ""
    );
    // ★ This assertion is INVERTED, deliberately.
    //
    // It read: *"a GROUND map must allocate — `ground_canonical_ps` constructs
    // the canonical order. Zero here would mean the canonicalization stopped
    // happening."* That inference was sound while the canonical order was
    // CONSTRUCTED at serialize time out of a `Vec` in the producer's order: the
    // construction was an allocation, so the allocation was evidence of the
    // canonicalization, and the encoder had to park the constructed vector in a
    // raw-pointer arena to hand it out by reference.
    //
    // The map stores the trie now, so the canonical order is a memoized
    // projection ON THE VALUE. The encoder BORROWS it, the arena is deleted, and
    // the ground map joins every other shape at zero. The canonicalization is
    // still happening — the leg below is what checks that, by BYTES rather than
    // by an allocation count standing in for them.
    assert_eq!(
        allocations, 0,
        "a GROUND map must now allocate NOTHING in the steady state: its canonical \
         order is memoized on the value and the encoder borrows it. A non-zero count \
         means the encoder went back to constructing the order at serialize time."
    );
    // The direct evidence the allocation count used to stand in for: a permuted
    // construction of the same entry set writes the SAME bytes.
    {
        let mut permuted_entries = corpus::ground_pathmap().entry_trie().entries_owned();
        permuted_entries.reverse();
        let permuted = pathmap_par(models::rhoapi::EPathMap::new(
            permuted_entries,
            Vec::new(),
            false,
            None,
        ));
        assert_eq!(
            with_encoded(&ground, <[u8]>::to_vec),
            with_encoded(&permuted, <[u8]>::to_vec),
            "the canonicalization must still be happening: a permuted construction \
             of one entry set must write identical bytes"
        );
    }
    assert!(
        allocations < oracle_allocations,
        "the machine allocated {allocations} and the derived path {oracle_allocations}; the \
         single-walk emitter must not cost MORE than the two-walk one on the shape where \
         both must canonicalize"
    );
}

/// An `EPathMap` whose single entry is `¬eval_stable` **and** carries
/// `locally_free` — the one shape that reaches `EntryTrie::blanked_stream`.
///
/// A bound-variable `EVar` is non-ground by CONTENT, so the entry takes
/// `encode_trie_path`'s `0x0F` escape arm, whose payload is canonical prost
/// bytes — and prost retains the bitset. That is the exact route by which an
/// entry's `locally_free` used to reach the bincode wire, and therefore the
/// exact route the memo now covers.
fn lf_bearing_pathmap() -> models::rust::rhoapi_ext::EPathMap {
    let entry = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EVarBody(models::rhoapi::EVar {
                v: Some(models::rhoapi::Var {
                    var_instance: Some(models::rhoapi::var::VarInstance::BoundVar(0)),
                }),
            })),
        }],
        locally_free: models::create_bit_vector(&[0]),
        ..Default::default()
    };
    models::rust::rhoapi_ext::EPathMap::new(vec![entry], Vec::new(), false, None)
}

/// One node holding one `EPathMap`.
fn pathmap_par(map: models::rust::rhoapi_ext::EPathMap) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }],
        ..Default::default()
    }
}

/// Drive the thread-local buffer and op-stack pool to their steady state.
fn warm(par: &Par) {
    for _ in 0..64 {
        with_encoded(par, <[u8]>::len);
    }
}

#[test]
fn encode_into_an_existing_buffer_reuses_its_capacity() {
    let par = corpus::all_par_fields();
    let size = encode(&par).len();

    let mut out = Vec::with_capacity(size * 4);
    for _ in 0..8 {
        out.clear();
        encode_into(&par, &mut out);
    }
    let before = out.capacity();
    let (_, _allocations, _) = counting_alloc::measure(|| {
        out.clear();
        encode_into(&par, &mut out);
    });
    assert_eq!(out.len(), size, "the encoding must be stable across reuse");
    assert_eq!(
        out.capacity(),
        before,
        "`encode_into` must not re-grow a buffer that already fits — appending into a \
         caller-owned buffer is what lets an intern-aware emitter splice without a copy"
    );
}

#[test]
fn the_reused_buffer_does_not_pin_a_pathological_high_water() {
    // One pathological term inflates the thread-local; the shrink policy must
    // return the capacity rather than hold it for the life of the thread.
    let huge = wide(200_000);
    let huge_len = with_encoded(&huge, <[u8]>::len);
    assert!(
        huge_len > 1 << 20,
        "the pathological fixture must exceed the shrink threshold, or this test proves \
         nothing (it encoded to {huge_len} B)"
    );

    // A small encode afterwards must not be paying for the huge one.
    let small = corpus::gint(1);
    for _ in 0..4 {
        with_encoded(&small, <[u8]>::len);
    }
    let (_, _, bytes) = counting_alloc::measure(|| {
        for _ in 0..16 {
            with_encoded(&small, <[u8]>::len);
        }
    });
    assert!(
        bytes < huge_len / 4,
        "after a {huge_len} B encode, sixteen small encodes allocated {bytes} B — the shrink \
         policy is not returning the pathological capacity"
    );
    println!("  after a {huge_len} B encode, 16 small encodes cost {bytes} B");
}

/// The op-stack POOL must not pin a deep term's high-water mark either.
#[test]
fn the_op_stack_pool_does_not_pin_a_deep_terms_high_water() {
    let deep_term = deep(8_192);
    let bytes = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let n = with_encoded(&deep_term, <[u8]>::len);
            std::mem::forget(deep_term);
            n
        })
        .expect("spawn")
        .join()
        .expect("a deep encode must survive a 2 MiB stack");
    assert!(bytes > 0);

    // ⚠ The deep encode ran on its OWN thread, so this thread's pool and buffer
    // were never touched by it — which is itself the point: the pool is
    // per-thread, so one thread's pathological term cannot inflate another's.
    // On the encoding thread the 8,192-deep stack (16,385 entries) exceeds
    // `MAX_POOLED_OPS` and is dropped rather than parked.
    let small = corpus::gint(1);
    warm(&small);
    let (_, allocations, _) = counting_alloc::measure(|| with_encoded(&small, <[u8]>::len));
    assert_eq!(
        allocations, 0,
        "a small encode after a deep one must still be allocation-free"
    );
}

// ===========================================================================
// §3  ⚠ Program addresses do NOT identify a type
// ===========================================================================

/// The measured fact behind [`models::rust::rholang::bincode_schema::BincodeNode::bincode_as_pathmap`].
///
/// If this test ever *fails* — i.e. the addresses become distinct — that is not
/// permission to reintroduce the pointer trick. Constant merging is a linker
/// and codegen-unit decision that varies with profile, LTO and target; a
/// downcast that is sound in one build and a type confusion in the next is
/// strictly worse than one that is always sound.
#[test]
fn program_addresses_do_not_identify_a_type() {
    let pathmap = corpus::ground_pathmap();
    let elist = EList::default();

    let same = program_address(&pathmap as &dyn BincodeNode)
        == program_address(&elist as &dyn BincodeNode);
    println!(
        "  EPATHMAP_PROGRAM @ {:#x}, ELIST_PROGRAM @ {:#x} — merged: {same}",
        program_address(&pathmap as &dyn BincodeNode),
        program_address(&elist as &dyn BincodeNode),
    );

    // Whatever the addresses are, the TYPE question must be answered by the
    // trait, and it must answer correctly.
    assert!(
        (&pathmap as &dyn BincodeNode)
            .bincode_as_pathmap()
            .is_some(),
        "an EPathMap must identify itself"
    );
    assert!(
        (&elist as &dyn BincodeNode).bincode_as_pathmap().is_none(),
        "an EList must NOT identify as an EPathMap — this is the type confusion that \
         SIGSEGV'd the differential on its first run"
    );
    for node in [
        &Par::default() as &dyn BincodeNode,
        &Send::default(),
        &Expr::default(),
        &models::rhoapi::ESet::default(),
        &models::rhoapi::EMap::default(),
    ] {
        assert!(
            node.bincode_as_pathmap().is_none(),
            "only EPathMap may answer to `bincode_as_pathmap`"
        );
    }

    // And the encoder survives the exact shape that used to crash: an `EList`
    // whose program is (content-)identical to `EPathMap`'s, nested under one.
    let mixed = models::par_from_default! {
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![corpus::gint(1)],
                    ..Default::default()
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(corpus::ground_pathmap())),
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        encode(&mixed),
        bincode::serialize(&mixed).expect("oracle"),
        "an EList beside an EPathMap must still encode identically"
    );
}

// ===========================================================================
// §4  ★★ Zero per-CLONE allocation — and the RED that makes it a measurement
// ===========================================================================

/// ★★ The clone half of the acceptance criterion, which had no executed guard.
///
/// The generated `<Par as Clone>::clone` routes through `drive_with` on
/// **thread-local pooled** stacks, so its steady state must be **0 allocations**
/// for the driver — exactly the criterion §2 already asserts for the encoder. The
/// clone's *output* is owned, so it allocates for the term it produces; the
/// statement being pinned is therefore a **relation**, not a zero: the driven
/// clone must allocate no more than the derived oracle it is byte-identical to.
///
/// ## ⚠ Why the RED is the OWNING `drive` and not a mutilated visitor
///
/// `drive` and `drive_with` differ in exactly one respect — `drive` performs
/// `Vec::with_capacity` twice per call, `drive_with` takes both stacks by `&mut`.
/// That difference is the entire reason `drive_with` exists, and its justification
/// is a measured one: 95.43% of production terms are at depth 2, where a `Par`
/// clone touches ~6 nodes and two mallocs are a double-digit regression.
///
/// So the RED here is **the owning entry point, called on the same term**. If the
/// pooled path's advantage is real, the owning path allocates strictly more; if it
/// is not, this test cannot tell them apart and says so. ★ That is a sharper RED
/// than a deliberately-broken visitor would be, because it is a control that
/// *ships* — somebody could reasonably reach for `drive` on this path, and this
/// test is what tells them what it costs.
#[test]
fn the_clone_steady_state_allocates_nothing_for_the_driver_itself() {
    use models::rust::rholang::drive::{drive, Step};
    use models::rust::rholang::term_ops::{oracle_clone_par, CloneNode, CloneTraversal, CloneVal};

    for (name, par) in [
        ("gint", corpus::gint(1)),
        ("wide(64)", wide(64)),
        ("deep(16)", deep(16)),
    ] {
        // Warm the thread-local stack pool: the criterion is the STEADY state.
        for _ in 0..8 {
            let warmed = par.clone();
            models::rust::rholang::par_children::dismantle(warmed);
        }

        let (pooled_allocs, oracle_allocs, owning_allocs) = {
            let (c, pooled_allocs, pooled_bytes) = counting_alloc::measure(|| par.clone());
            models::rust::rholang::par_children::dismantle(c);
            let (o, oracle_allocs, oracle_bytes) =
                counting_alloc::measure(|| oracle_clone_par(&par));
            models::rust::rholang::par_children::dismantle(o);
            // ⚠ THE RED: the OWNING entry point, same visitor, same term.
            let (w, owning_allocs, owning_bytes) = counting_alloc::measure(|| {
                match drive(
                    &mut CloneTraversal,
                    &mut (),
                    Step::Descend(CloneNode::Par(&par)),
                )
                .expect("the clone traversal is infallible")
                {
                    CloneVal::Par(p) => p,
                    CloneVal::CostSignature(_) => {
                        panic!("a Par-rooted clone walk produced a CostSignature root")
                    }
                    CloneVal::CostStack(_) => {
                        panic!("a Par-rooted clone walk produced a CostStack root")
                    }
                }
            });
            models::rust::rholang::par_children::dismantle(w);
            println!(
                "  {name:20} pooled {pooled_allocs:3} allocs {pooled_bytes:8} B \
                 | derived {oracle_allocs:3} allocs {oracle_bytes:8} B \
                 | OWNING drive {owning_allocs:3} allocs {owning_bytes:8} B"
            );
            (pooled_allocs, oracle_allocs, owning_allocs)
        };

        assert!(
            pooled_allocs <= oracle_allocs,
            "`{name}`: the driven clone made {pooled_allocs} allocations against the derived \
             oracle's {oracle_allocs}. The two produce BYTE-IDENTICAL terms \
             (`models/tests/clone_equivalence_corpus.rs`, eight axes), so every allocation \
             beyond the oracle's is the DRIVER's and the thread-local pool has stopped \
             holding. `drive_with` exists for exactly this number."
        );
        assert!(
            owning_allocs > pooled_allocs,
            "★ THE RED IS INERT. The owning `drive` made {owning_allocs} allocations and the \
             pooled `drive_with` path made {pooled_allocs} — the owning path must cost \
             STRICTLY MORE, because it performs two `Vec::with_capacity` calls per clone \
             where the pooled path performs none. If they are equal, this test cannot \
             distinguish a pooled driver from an allocating one, the assertion above is \
             vacuous, and `drive_with`'s entire justification is unmeasured."
        );
    }
}
