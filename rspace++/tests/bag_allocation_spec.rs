//! Representation-level allocation gate for the PathMap-backed Bag.
//!
//! This test binary owns its counting allocator, so unrelated workspace tests
//! cannot contaminate the measurements. The gate compares one logical
//! occurrence with `u64::MAX` indistinguishable occurrences at the same path.
//! A multiplicity is one fixed-width trie value; logical cardinality must not
//! allocate or serialize one object per occurrence.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::cmp::Ordering;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use rspace_plus_plus::rspace::bag::{Bag, BagDelta, Multiplicity, SortedUniqueBagError};
use rspace_plus_plus::rspace::data_index::{CandidatePool, RSpaceDataIndex, RSpaceDataStore};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::internal::{Datum, DatumAddress};
use rspace_plus_plus::rspace::rho_path::RhoPathEncode;
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, legacy_prefix,
};
use rspace_plus_plus::rspace::trace::event::Produce;
use serde::{Deserialize, Serialize};

struct CountingAllocator;

thread_local! {
    static COUNTS: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
    static DECODES: Cell<usize> = const { Cell::new(0) };
    static SIZED_ALLOCATIONS: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

fn record(pointer: *mut u8, bytes: usize) {
    if !pointer.is_null() {
        let _ = COUNTS.try_with(|counts| {
            if let Some((calls, allocated)) = counts.get() {
                counts.set(Some((calls + 1, allocated + bytes)));
            }
        });
        let _ = SIZED_ALLOCATIONS.try_with(|sized| {
            if let Some((size, matched)) = sized.get().filter(|&(size, _)| size == bytes) {
                sized.set(Some((size, matched + 1)));
            }
        });
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: this allocator delegates the unchanged allocation request to
        // the process System allocator.
        let pointer = unsafe { System.alloc(layout) };
        record(pointer, layout.size());
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: this allocator delegates the unchanged allocation request to
        // the process System allocator.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        record(pointer, layout.size());
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from the System allocator through this exact
        // wrapper and `layout` is the corresponding allocation layout.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `pointer` and `layout` identify a live System allocation, and
        // the request is forwarded without changing either value.
        let replacement = unsafe { System.realloc(pointer, layout, new_size) };
        record(replacement, new_size);
        replacement
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) { COUNTS.set(None); }
    }
    assert!(COUNTS.get().is_none());
    COUNTS.set(Some((0, 0)));
    let reset = Reset;
    let result = operation();
    let (calls, bytes) = COUNTS.get().unwrap();
    drop(reset);
    (result, calls, bytes)
}

fn count_sized_allocations<T>(size: usize, operation: impl FnOnce() -> T) -> (T, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) { SIZED_ALLOCATIONS.set(None); }
    }
    assert!(SIZED_ALLOCATIONS.get().is_none());
    SIZED_ALLOCATIONS.set(Some((size, 0)));
    let reset = Reset;
    let result = operation();
    let (_, matched) = SIZED_ALLOCATIONS
        .get()
        .expect("the sized-allocation counter stays armed during the operation");
    drop(reset);
    (result, matched)
}

fn singleton(path: &[u8], multiplicity: Multiplicity) -> Bag {
    let mut bag = Bag::new();
    bag.insert_many(path, multiplicity).unwrap();
    bag
}

#[test]
fn logical_cardinality_never_materializes_occurrence_sized_storage() {
    assert_eq!(std::mem::size_of::<Multiplicity>(), std::mem::size_of::<u64>());
    assert_eq!(std::mem::align_of::<Multiplicity>(), std::mem::align_of::<u64>());

    let path = vec![0xa5; 256];
    let (one, one_allocations, one_bytes) = measure(|| singleton(&path, Multiplicity::ONE));
    let (maximum, maximum_allocations, maximum_bytes) =
        measure(|| singleton(&path, Multiplicity::new(u64::MAX).unwrap()));

    assert_eq!(one_allocations, maximum_allocations);
    assert_eq!(one_bytes, maximum_bytes);
    assert_eq!(one.distinct_len(), 1);
    assert_eq!(maximum.distinct_len(), 1);
    assert_eq!(one.iter().count(), 1);
    assert_eq!(maximum.iter().count(), 1);
    assert_eq!(one.cardinality(), 1);
    assert_eq!(maximum.cardinality(), u64::MAX);
    assert!(!maximum.cardinality_is_saturated());

    let (one_image, one_image_allocations, one_image_bytes) = measure(|| one.compact_bytes());
    let (maximum_image, maximum_image_allocations, maximum_image_bytes) =
        measure(|| maximum.compact_bytes());
    // ACTree03 branchless-varint encodes a `u64` in one to nine bytes. The
    // largest multiplicity may therefore add eight bytes, but never storage
    // proportional to the number of logical occurrences.
    assert_eq!(maximum_image.len() - one_image.len(), 8);
    assert_eq!(one_image_allocations, maximum_image_allocations);
    assert_eq!(maximum_image_bytes - one_image_bytes, 8);
}

#[test]
fn exact_algebra_does_not_allocate_one_path_per_endpoint() {
    for cardinality in [64usize, 1024] {
        let keys = (0..cardinality)
            .map(|ordinal| {
                let mut key = vec![0xA5; 64];
                key.extend_from_slice(&(ordinal as u64).to_be_bytes());
                key
            })
            .collect::<Vec<_>>();
        let construct = |count| {
            Bag::try_from_sorted_unique(|writer| {
                for key in &keys {
                    writer.push(key, Multiplicity::new(count).unwrap())?;
                }
                Ok::<(), SortedUniqueBagError>(())
            })
            .unwrap()
        };
        let low = construct(1);
        let high = construct(2);
        let (baseline, baseline_calls, _) = measure(|| construct(1));
        let (meet, meet_calls, _) = measure(|| high.meet(&low));
        let (subtract, subtract_calls, _) = measure(|| high.subtract(&low));
        let (subbag, subbag_calls, _) = measure(|| low.is_subbag_of(&high));

        assert_eq!(baseline, low);
        assert_eq!(meet, low);
        assert_eq!(subtract, low);
        assert!(subbag);
        assert!(
            meet_calls <= baseline_calls + 32,
            "{cardinality}: {meet_calls} > {baseline_calls} + 32"
        );
        assert!(
            subtract_calls <= baseline_calls + 32,
            "{cardinality}: {subtract_calls} > {baseline_calls} + 32"
        );
        assert!(subbag_calls <= 32, "{cardinality}: {subbag_calls} > 32");
    }
}

fn ordinal_key_bag(cardinality: usize) -> Bag {
    Bag::try_from_sorted_unique(|writer| {
        let mut key = vec![0xA5; 72];
        for ordinal in 0..cardinality {
            key[64..].copy_from_slice(&(ordinal as u64).to_be_bytes());
            writer.push(&key, Multiplicity::ONE)?;
        }
        Ok::<(), SortedUniqueBagError>(())
    })
    .expect("ascending ordinal keys form a sorted-unique stream")
}

#[test]
fn comparing_and_hashing_bags_allocates_nothing_per_endpoint() {
    let observe = |cardinality: usize| {
        let left = ordinal_key_bag(cardinality);
        let right = ordinal_key_bag(cardinality);
        let ((equal, ordering, left_digest, right_digest), calls, bytes) = measure(|| {
            let equal = black_box(&left) == black_box(&right);
            let ordering = black_box(&left).cmp(black_box(&right));
            let mut left_state = DefaultHasher::new();
            black_box(&left).hash(&mut left_state);
            let mut right_state = DefaultHasher::new();
            black_box(&right).hash(&mut right_state);
            (equal, ordering, left_state.finish(), right_state.finish())
        });
        assert!(equal);
        assert_eq!(ordering, Ordering::Equal);
        assert_eq!(left_digest, right_digest);
        (calls, bytes)
    };
    let small = observe(64);
    let large = observe(1024);
    assert_eq!(
        large, small,
        "allocations grew with the endpoint count: (calls, bytes) at 1,024 endpoints, then at 64"
    );
}

const DELTA_PATH_BYTES: usize = 41;

const _: () = assert!(DELTA_PATH_BYTES % 2 == 1);

const DELTA_TARGET_ENDPOINTS: usize = 4096;

fn delta_path(ordinal: usize) -> [u8; DELTA_PATH_BYTES] {
    let mut path = [0xA5; DELTA_PATH_BYTES];
    path[DELTA_PATH_BYTES - 8..].copy_from_slice(&(ordinal as u64).to_be_bytes());
    path
}

fn delta_bag(entries: impl IntoIterator<Item = (usize, u64)>) -> Bag {
    let mut bag = Bag::new();
    for (ordinal, count) in entries {
        bag.insert_many(
            &delta_path(ordinal),
            Multiplicity::new(count).expect("delta fixture counts are positive"),
        )
        .expect("delta fixture counts fit the multiplicity domain");
    }
    bag
}

fn delta_target() -> Bag { delta_bag((0..DELTA_TARGET_ENDPOINTS).map(|ordinal| (ordinal, 2))) }

fn value_only_delta(endpoints: usize) -> BagDelta {
    let mut delta = BagDelta::new();
    for ordinal in 0..endpoints {
        delta
            .record_removal(&delta_path(ordinal), Multiplicity::ONE)
            .expect("distinct delta paths record independently");
    }
    delta
}

fn structural_delta(endpoints: usize) -> BagDelta {
    let mut delta = BagDelta::new();
    for index in 0..endpoints {
        let recorded = match index % 2 {
            0 => delta
                .record_addition(&delta_path(DELTA_TARGET_ENDPOINTS + index), Multiplicity::ONE),
            _ => delta.record_removal(&delta_path(index), Multiplicity::ONE),
        };
        recorded.expect("distinct delta paths record independently");
    }
    delta
}

#[test]
fn applying_value_only_bag_deltas_allocates_nothing_per_endpoint() {
    let observe = |endpoints: usize| {
        let delta = value_only_delta(endpoints);
        let mut target = delta_target();
        let applied =
            delta_bag((0..DELTA_TARGET_ENDPOINTS).map(|ordinal| match ordinal < endpoints {
                true => (ordinal, 1),
                false => (ordinal, 2),
            }));
        let original = delta_target();
        let (applying, apply_calls, apply_bytes) = measure(|| delta.apply_to(&mut target));
        applying.expect("a value-only delta applies");
        assert_eq!(target, applied);
        let (reverting, revert_calls, revert_bytes) = measure(|| delta.revert_from(&mut target));
        reverting.expect("a value-only delta reverts");
        assert_eq!(target, original);
        (apply_calls, apply_bytes, revert_calls, revert_bytes)
    };
    let single = observe(1);
    let wide = observe(1024);
    assert_eq!(
        wide, single,
        "value-only delta allocations grew with the endpoint count: (apply calls, apply bytes, \
         revert calls, revert bytes) at 1,024 endpoints, then at 1"
    );
}

#[test]
fn applying_structural_bag_deltas_allocates_no_path_buffer_per_endpoint() {
    let observe = |endpoints: usize| {
        let delta = structural_delta(endpoints);
        let mut target = delta_target();
        let applied = delta_bag(
            (0..DELTA_TARGET_ENDPOINTS)
                .map(|ordinal| match (ordinal < endpoints, ordinal % 2) {
                    (true, 1) => (ordinal, 1),
                    _ => (ordinal, 2),
                })
                .chain(
                    (0..endpoints)
                        .step_by(2)
                        .map(|index| (DELTA_TARGET_ENDPOINTS + index, 1)),
                ),
        );
        let original = delta_target();
        let (applying, apply_buffers) =
            count_sized_allocations(DELTA_PATH_BYTES, || delta.apply_to(&mut target));
        applying.expect("a structural delta applies");
        assert_eq!(target, applied);
        let (reverting, revert_buffers) =
            count_sized_allocations(DELTA_PATH_BYTES, || delta.revert_from(&mut target));
        reverting.expect("a structural delta reverts");
        assert_eq!(target, original);
        (apply_buffers, revert_buffers)
    };
    let single = observe(1);
    let wide = observe(1024);
    assert_eq!(
        wide, single,
        "path-sized buffers grew with the delta endpoint count: (apply, revert) at 1,024 \
         endpoints, then at 1"
    );
}

fn overlay_operand(endpoints: usize, shared_count: u64) -> Bag {
    delta_bag((0..endpoints).map(|index| match index % 2 {
        0 => (DELTA_TARGET_ENDPOINTS + index, 1),
        _ => (index, shared_count),
    }))
}

fn overlay_expectation(endpoints: usize, shared_result: u64) -> Bag {
    delta_bag(
        (0..DELTA_TARGET_ENDPOINTS)
            .map(|ordinal| match (ordinal < endpoints, ordinal % 2) {
                (true, 1) => (ordinal, shared_result),
                _ => (ordinal, 2),
            })
            .chain(
                (0..endpoints)
                    .step_by(2)
                    .map(|index| (DELTA_TARGET_ENDPOINTS + index, 1)),
            ),
    )
}

#[test]
fn summing_and_joining_bags_allocates_no_path_buffer_per_endpoint() {
    let observe = |endpoints: usize| {
        let target = delta_target();
        let operand = overlay_operand(endpoints, 3);
        let expected_sum = overlay_expectation(endpoints, 5);
        let expected_join = overlay_expectation(endpoints, 3);
        let (summed, sum_buffers) =
            count_sized_allocations(DELTA_PATH_BYTES, || target.sum(&operand));
        assert_eq!(summed.expect("the overlay sum fits"), expected_sum);
        let (reversed_sum, reversed_sum_buffers) =
            count_sized_allocations(DELTA_PATH_BYTES, || operand.sum(&target));
        assert_eq!(reversed_sum.expect("the overlay sum fits"), expected_sum);
        let (joined, join_buffers) =
            count_sized_allocations(DELTA_PATH_BYTES, || target.join(&operand));
        assert_eq!(joined, expected_join);
        let (reversed_join, reversed_join_buffers) =
            count_sized_allocations(DELTA_PATH_BYTES, || operand.join(&target));
        assert_eq!(reversed_join, expected_join);
        (sum_buffers, reversed_sum_buffers, join_buffers, reversed_join_buffers)
    };
    let single = observe(1);
    let wide = observe(1024);
    assert_eq!(
        wide, single,
        "path-sized buffers grew with the operand's endpoint count: (sum, reversed sum, join, \
         reversed join) at 1,024 endpoints, then at 1"
    );
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct CatalogValue(String);

impl StableHashSerialize for CatalogValue {}

impl RhoPathEncode for CatalogValue {
    fn rho_path_structure(&self) -> Vec<u8> { self.0.as_bytes().to_vec() }
}

impl ColdStoreDecode for CatalogValue {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        DECODES.set(DECODES.get() + 1);
        legacy_prefix(bytes)
    }
}

#[derive(Clone, Copy, Debug)]
struct CatalogCase {
    distinct: usize,
    width: usize,
    siblings: usize,
    multiplicity: usize,
    pools: usize,
    dispersed: bool,
}

fn catalog_cases() -> Vec<CatalogCase> {
    let base = CatalogCase {
        distinct: 64,
        width: 128,
        siblings: 0,
        multiplicity: 1,
        pools: 1,
        dispersed: false,
    };
    let mut cases = vec![CatalogCase {
        distinct: 0,
        ..base
    }];
    for distinct in [128, 256, 512, 1024, 2048] {
        cases.push(CatalogCase { distinct, ..base });
    }
    for dispersed in [false, true] {
        for width in [128, 256, 512, 1024, 2048] {
            cases.push(CatalogCase {
                width,
                dispersed,
                ..base
            });
        }
    }
    for siblings in [0, 128, 512, 2048] {
        cases.push(CatalogCase { siblings, ..base });
    }
    for multiplicity in [1, 16, 256] {
        cases.push(CatalogCase {
            multiplicity,
            ..base
        });
    }
    for distinct in [128, 2048] {
        for pools in [1, 16, 64, 256, 1024] {
            cases.push(CatalogCase {
                distinct,
                pools,
                ..base
            });
        }
    }
    cases
}

struct CatalogFixture {
    store: RSpaceDataStore<String, CatalogValue>,
    channel: String,
    paths: Vec<Vec<u8>>,
    sibling_paths: Vec<Vec<u8>>,
    ordered_positions: Vec<usize>,
}

fn catalog_fixture(case: CatalogCase) -> CatalogFixture {
    fn datums(count: usize, width: usize, dispersed: bool) -> Vec<Datum<CatalogValue>> {
        (0..count)
            .map(|index| {
                let mut payload = vec![b'x'; width];
                if dispersed {
                    let mut state = 41316445_u64 ^ index as u64;
                    for byte in &mut payload {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        *byte = b'a' + (state % 26) as u8;
                    }
                }
                let identity = format!("{index:016x}");
                payload[width - 16..].copy_from_slice(identity.as_bytes());
                Datum {
                    a: Arc::new(CatalogValue(String::from_utf8(payload).unwrap())),
                    persist: false,
                    source: Produce::new(
                        Blake2b256Hash::new(&index.to_le_bytes()),
                        Blake2b256Hash::new(b"catalog-resource-fixture"),
                        false,
                    ),
                }
            })
            .collect()
    }
    let channel = "catalog-resource-target".to_owned();
    let sibling = "catalog-resource-sibling".to_owned();
    let requested = datums(case.distinct, case.width, case.dispersed);
    let siblings = datums(case.siblings, 128, false);
    let identities: Vec<_> = requested
        .iter()
        .map(|datum| {
            let bytes =
                bincode::serialize(&(datum.a.as_ref(), datum.persist, &datum.source)).unwrap();
            (Blake2b256Hash::new(&bytes), bytes)
        })
        .collect();
    let mut ordered_positions: Vec<_> = (0..requested.len()).collect();
    ordered_positions.sort_unstable_by_key(|position| &identities[*position]);
    let paths = requested
        .iter()
        .map(|datum| RSpaceDataIndex::datum_path(&channel, datum))
        .collect();
    let sibling_paths = siblings
        .iter()
        .map(|datum| RSpaceDataIndex::datum_path(&sibling, datum))
        .collect();
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(
            channel.clone(),
            requested
                .into_iter()
                .flat_map(|datum| std::iter::repeat_n(datum, case.multiplicity)),
        )
        .unwrap();
    store.set_channel(sibling, siblings).unwrap();
    CatalogFixture {
        store,
        channel,
        paths,
        sibling_paths,
        ordered_positions,
    }
}

fn check_pool(fixture: &CatalogFixture, pool: &CandidatePool<CatalogValue>) {
    for (position, fixture_position) in fixture.ordered_positions.iter().enumerate() {
        let (datum, address) = pool.candidate(position).unwrap();
        let DatumAddress::Stored(path) = address else {
            panic!("unexpected fresh datum")
        };
        assert_eq!(path.as_ref(), fixture.paths[*fixture_position]);
        assert_eq!(
            RSpaceDataIndex::datum_path(&fixture.channel, datum),
            fixture.paths[*fixture_position]
        );
    }
    assert!(pool.candidate(fixture.ordered_positions.len()).is_none());
}

fn check_fixture(fixture: &CatalogFixture, case: CatalogCase) {
    assert_eq!(fixture.store.distinct_len(), case.distinct + case.siblings);
    assert_eq!(
        fixture.store.cardinality(),
        (case.distinct * case.multiplicity + case.siblings) as u64
    );
    for path in &fixture.paths {
        assert_eq!(fixture.store.index().count_path(path), case.multiplicity as u64);
    }
    for path in &fixture.sibling_paths {
        assert_eq!(fixture.store.index().count_path(path), 1);
    }
}

#[test]
fn warm_catalog_pools_share_storage_independently_of_catalog_size() {
    let mut per_pool = None;
    for case in catalog_cases()
        .into_iter()
        .filter(|case| case.distinct != 0)
    {
        let fixture = catalog_fixture(case);
        drop(
            fixture
                .store
                .channel_pool(&fixture.channel)
                .unwrap()
                .unwrap(),
        );
        let mut pools = Vec::with_capacity(case.pools);
        DECODES.set(0);
        let ((), calls, bytes) = measure(|| {
            for _ in 0..case.pools {
                pools.push(
                    fixture
                        .store
                        .channel_pool(&fixture.channel)
                        .unwrap()
                        .unwrap(),
                );
            }
        });
        assert_eq!(DECODES.get(), 0);
        let (unit_calls, unit_bytes) = *per_pool.get_or_insert((calls, bytes));
        assert_eq!(calls, unit_calls * case.pools, "{case:?}");
        assert_eq!(bytes, unit_bytes * case.pools, "{case:?}");
        assert!(pools.iter().all(|pool| pool.len() == case.distinct));
        check_pool(&fixture, &pools[0]);
        drop(pools);
        check_fixture(&fixture, case);
    }
}

#[cfg(target_os = "linux")]
fn resident_kib() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")
                .map(|rss| rss.split_whitespace().next().unwrap().parse().unwrap())
        })
        .unwrap()
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "isolated resource measurement; set CATALOG_RESOURCE_CASE and CATALOG_RESOURCE_PHASE"]
fn catalog_resource_profile() {
    let cell: usize = std::env::var("CATALOG_RESOURCE_CASE")
        .unwrap()
        .parse()
        .unwrap();
    let phase = std::env::var("CATALOG_RESOURCE_PHASE").unwrap();
    assert!(phase == "cold" || phase == "warm");
    let case = catalog_cases()[cell];
    let pool_count = if phase == "cold" { 1 } else { case.pools };
    for sample in 0..35 {
        let fixture = catalog_fixture(case);
        if phase == "warm" {
            drop(
                fixture
                    .store
                    .channel_pool(&fixture.channel)
                    .unwrap()
                    .unwrap(),
            );
        }
        let encoded_bytes: usize = fixture.paths.iter().map(Vec::len).sum();
        let sibling_bytes: usize = fixture.sibling_paths.iter().map(Vec::len).sum();
        let mut pools = Vec::with_capacity(pool_count);
        let rss_before = resident_kib();
        DECODES.set(0);
        let (((), nanos), calls, bytes) = measure(|| {
            let start = Instant::now();
            for _ in 0..pool_count {
                pools.push(
                    fixture
                        .store
                        .channel_pool(&fixture.channel)
                        .unwrap()
                        .unwrap(),
                );
            }
            ((), start.elapsed().as_nanos())
        });
        let rss_after = resident_kib();
        assert_eq!(DECODES.get(), 0);
        assert!(pools.iter().all(|pool| pool.len() == case.distinct));
        assert!(
            pools
                .iter()
                .all(|pool| pool.is_empty() == (case.distinct == 0))
        );
        check_fixture(&fixture, case);
        check_pool(&fixture, &pools[0]);
        drop(pools);
        check_fixture(&fixture, case);
        if sample >= 5 {
            println!(
                concat!(
                    "CATALOG\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t",
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}"
                ),
                cell,
                phase,
                sample - 5,
                case.distinct,
                case.width,
                case.siblings,
                case.multiplicity,
                pool_count,
                case.dispersed,
                encoded_bytes,
                sibling_bytes,
                calls,
                bytes,
                nanos,
                rss_before,
                rss_after
            );
        }
    }
}

fn allocated_key_meet(left: &Bag, right: &Bag) -> Bag {
    let (probe, index) = if left.distinct_len() <= right.distinct_len() {
        (left, right)
    } else {
        (right, left)
    };
    let mut result = Bag::new();
    for (path, multiplicity) in probe.iter() {
        let counter = index.count(&path);
        if counter != 0 {
            result.set_count(&path, Some(multiplicity.min(Multiplicity::new(counter).unwrap())));
        }
    }
    result
}

fn allocated_key_subtract(left: &Bag, right: &Bag) -> Bag {
    let mut removals = right.iter().peekable();
    Bag::try_from_sorted_unique(|writer| {
        for (path, current) in left.iter() {
            while removals
                .peek()
                .is_some_and(|(removed_path, _)| removed_path < &path)
            {
                removals.next();
            }
            let next = match removals.peek() {
                Some((removed_path, removed)) if removed_path == &path => (current.get() >
                    removed.get())
                .then(|| Multiplicity::new(current.get() - removed.get()).unwrap()),
                _ => Some(current),
            };
            if let Some(next) = next {
                writer.push(&path, next)?;
            }
        }
        Ok::<(), SortedUniqueBagError>(())
    })
    .unwrap()
}

fn allocated_key_subbag(left: &Bag, right: &Bag) -> bool {
    left.iter()
        .all(|(path, multiplicity)| right.count(&path) >= multiplicity.get())
}

#[derive(Clone, Copy)]
struct AlgebraProfileCase {
    name: &'static str,
    left: usize,
    right: usize,
    right_offset: usize,
    shared_prefix: bool,
    prefix_related: bool,
    maximum_count: bool,
}

fn algebra_profile_cases() -> [AlgebraProfileCase; 13] {
    [
        AlgebraProfileCase {
            name: "empty",
            left: 0,
            right: 0,
            right_offset: 0,
            shared_prefix: true,
            prefix_related: false,
            maximum_count: false,
        },
        AlgebraProfileCase {
            name: "one_equal",
            left: 1,
            right: 1,
            right_offset: 0,
            shared_prefix: true,
            prefix_related: false,
            maximum_count: false,
        },
        AlgebraProfileCase {
            name: "one_disjoint",
            left: 1,
            right: 1,
            right_offset: 1,
            shared_prefix: true,
            prefix_related: false,
            maximum_count: false,
        },
        AlgebraProfileCase {
            name: "one_maximum",
            left: 1,
            right: 1,
            right_offset: 0,
            shared_prefix: true,
            prefix_related: false,
            maximum_count: true,
        },
        AlgebraProfileCase {
            name: "shared_64_equal",
            left: 64,
            right: 64,
            right_offset: 0,
            shared_prefix: true,
            prefix_related: false,
            maximum_count: false,
        },
        AlgebraProfileCase {
            name: "shared_64_half",
            left: 64,
            right: 64,
            right_offset: 32,
            shared_prefix: true,
            prefix_related: false,
            maximum_count: false,
        },
        AlgebraProfileCase {
            name: "prefix_chain_64_half",
            left: 64,
            right: 64,
            right_offset: 32,
            shared_prefix: true,
            prefix_related: true,
            maximum_count: false,
        },
        AlgebraProfileCase {
            name: "shared_1024_equal",
            left: 1024,
            right: 1024,
            right_offset: 0,
            shared_prefix: true,
            prefix_related: false,
            maximum_count: false,
        },
        AlgebraProfileCase {
            name: "shared_1024_half",
            left: 1024,
            right: 1024,
            right_offset: 512,
            shared_prefix: true,
            prefix_related: false,
            maximum_count: false,
        },
        AlgebraProfileCase {
            name: "shared_1024_disjoint",
            left: 1024,
            right: 1024,
            right_offset: 1024,
            shared_prefix: true,
            prefix_related: false,
            maximum_count: false,
        },
        AlgebraProfileCase {
            name: "left_one_right_1024",
            left: 1,
            right: 1024,
            right_offset: 0,
            shared_prefix: true,
            prefix_related: false,
            maximum_count: false,
        },
        AlgebraProfileCase {
            name: "left_1024_right_one",
            left: 1024,
            right: 1,
            right_offset: 0,
            shared_prefix: true,
            prefix_related: false,
            maximum_count: false,
        },
        AlgebraProfileCase {
            name: "dispersed_1024_equal",
            left: 1024,
            right: 1024,
            right_offset: 0,
            shared_prefix: false,
            prefix_related: false,
            maximum_count: false,
        },
    ]
}

fn algebra_profile_key(ordinal: usize, shared_prefix: bool) -> Vec<u8> {
    if shared_prefix {
        let mut key = vec![0xA5; 64];
        key.extend_from_slice(&(ordinal as u64).to_be_bytes());
        key
    } else {
        let mut state = (ordinal as u64)
            .wrapping_add(1)
            .wrapping_mul(0x9e3779b97f4a7c15);
        let mut key = vec![0u8; 64];
        for byte in &mut key {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        key.extend_from_slice(&(ordinal as u64).to_be_bytes());
        key
    }
}

fn algebra_profile_bag(size: usize, offset: usize, case: AlgebraProfileCase) -> Bag {
    let mut result = Bag::new();
    for ordinal in offset..offset + size {
        let count = if case.maximum_count {
            u64::MAX
        } else {
            ordinal as u64 % 5 + 1
        };
        let key = if case.prefix_related {
            vec![0xA5; ordinal + 1]
        } else {
            algebra_profile_key(ordinal, case.shared_prefix)
        };
        result
            .insert_many(&key, Multiplicity::new(count).unwrap())
            .unwrap();
    }
    result
}

fn algebra_profile_batch(operation: &str, arm: &str, left: &Bag, right: &Bag, repetitions: usize) {
    for _ in 0..repetitions {
        match (operation, arm) {
            ("meet", "control") => {
                black_box(allocated_key_meet(black_box(left), black_box(right)));
            }
            ("meet", "treatment") => {
                black_box(black_box(left).meet(black_box(right)));
            }
            ("subtract", "control") => {
                black_box(allocated_key_subtract(black_box(left), black_box(right)));
            }
            ("subtract", "treatment") => {
                black_box(black_box(left).subtract(black_box(right)));
            }
            ("subbag", "control") => {
                black_box(allocated_key_subbag(black_box(left), black_box(right)));
            }
            ("subbag", "treatment") => {
                black_box(black_box(left).is_subbag_of(black_box(right)));
            }
            _ => unreachable!(),
        }
    }
}

#[test]
#[ignore = "isolated preregistered old/new Bag algebra resource experiment"]
fn bag_algebra_old_new_resource_profile() {
    for case in algebra_profile_cases() {
        let left = algebra_profile_bag(case.left, 0, case);
        let right = algebra_profile_bag(case.right, case.right_offset, case);
        assert_eq!(allocated_key_meet(&left, &right), left.meet(&right));
        assert_eq!(allocated_key_subtract(&left, &right), left.subtract(&right));
        assert_eq!(allocated_key_subbag(&left, &right), left.is_subbag_of(&right));
        let repetitions = (32768 / case.left.max(case.right).max(1)).clamp(32, 4096);
        for operation in ["meet", "subtract", "subbag"] {
            for sample in 0..51 {
                let arms = if sample % 2 == 0 {
                    ["control", "treatment"]
                } else {
                    ["treatment", "control"]
                };
                for arm in arms {
                    let start = Instant::now();
                    let ((), calls, bytes) = measure(|| {
                        algebra_profile_batch(operation, arm, &left, &right, repetitions);
                    });
                    let nanos = start.elapsed().as_nanos();
                    if sample >= 20 {
                        println!(
                            "BAGALG\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                            case.name,
                            operation,
                            arm,
                            sample - 20,
                            repetitions,
                            calls,
                            bytes,
                            nanos
                        );
                    }
                }
            }
        }
    }
}

const EXHAUSTION_POPULATION_KEY_BYTES: usize = 137;
const EXHAUSTION_REMOVED_KEY_BYTES: usize = 264;

fn exhaustion_population_key(ordinal: usize) -> Vec<u8> {
    let mut key = Vec::with_capacity(EXHAUSTION_POPULATION_KEY_BYTES);
    key.extend_from_slice(&[0xA5; 64]);
    key.push(0x00);
    key.extend_from_slice(&[0x3C; 64]);
    key.extend_from_slice(&(ordinal as u64).to_be_bytes());
    key
}

fn exhaustion_removed_key() -> Vec<u8> {
    let mut key = Vec::with_capacity(EXHAUSTION_REMOVED_KEY_BYTES);
    key.extend_from_slice(&[0xA5; 64]);
    key.push(0xFF);
    key.extend_from_slice(&[0x5A; 199]);
    key
}

fn exhaustion_bag(population: usize, with_removed: bool) -> Bag {
    let removed = exhaustion_removed_key();
    Bag::try_from_sorted_unique(|writer| {
        for ordinal in 0..population {
            writer.push(&exhaustion_population_key(ordinal), Multiplicity::ONE)?;
        }
        match with_removed {
            true => writer.push(&removed, Multiplicity::ONE),
            false => Ok::<(), SortedUniqueBagError>(()),
        }
    })
    .expect("the population keys ascend below the removed key")
}

fn exhaustion_delta(population: usize) -> BagDelta {
    let mut delta = BagDelta::new();
    for ordinal in 0..population {
        delta
            .record_addition(&exhaustion_population_key(ordinal), Multiplicity::ONE)
            .expect("distinct population keys record independently");
    }
    delta
        .record_addition(&exhaustion_removed_key(), Multiplicity::ONE)
        .expect("the removed key records once");
    delta
}

#[test]
fn exhausting_single_key_removal_allocates_independently_of_bag_size() {
    let observe = |population: usize| {
        let removed = exhaustion_removed_key();
        let expected = exhaustion_bag(population, false).compact_bytes();
        let check = |bag: &Bag| {
            assert_eq!(bag.count(&removed), 0);
            assert_eq!(bag.distinct_len(), population);
            assert_eq!(bag.compact_bytes(), expected);
        };

        let mut bag = exhaustion_bag(population, true);
        let (outcome, try_calls, try_bytes) =
            measure(|| bag.try_remove_many(&removed, Multiplicity::ONE));
        assert_eq!(outcome, Ok(None));
        check(&bag);

        let mut bag = exhaustion_bag(population, true);
        let (outcome, many_calls, many_bytes) =
            measure(|| bag.remove_many(&removed, Multiplicity::ONE));
        assert_eq!(outcome, None);
        check(&bag);

        let mut bag = exhaustion_bag(population, true);
        let (outcome, remove_calls, remove_bytes) = measure(|| bag.remove(&removed));
        assert_eq!(outcome, None);
        check(&bag);

        let mut bag = exhaustion_bag(population, true);
        let ((), set_calls, set_bytes) = measure(|| bag.set_count(&removed, None));
        check(&bag);

        let mut delta = exhaustion_delta(population);
        let (outcome, delta_calls, delta_bytes) =
            measure(|| delta.record_removal(&removed, Multiplicity::ONE));
        assert_eq!(outcome, Ok(()));
        check(delta.additions());
        assert!(delta.removals().is_empty());

        [
            (try_calls, try_bytes),
            (many_calls, many_bytes),
            (remove_calls, remove_bytes),
            (set_calls, set_bytes),
            (delta_calls, delta_bytes),
        ]
    };
    let single = observe(16);
    let wide = observe(4096);
    assert_eq!(
        wide, single,
        "exhausting single-key removal allocations grew with the Bag size: (calls, bytes) for \
         try_remove_many, remove_many, remove, set_count(None) and BagDelta cancellation at 4,096 \
         resident keys, then at 16"
    );
}

fn delta_gate_datum(label: &str) -> Datum<CatalogValue> {
    Datum {
        a: Arc::new(CatalogValue(format!("delta-gate-{label}"))),
        persist: false,
        source: Produce::new(
            Blake2b256Hash::new(label.as_bytes()),
            Blake2b256Hash::new(b"delta-gate-fixture"),
            false,
        ),
    }
}

fn delta_gate_target() -> String { format!("g1{}", "z".repeat(64)) }

fn delta_gate_store(population: usize) -> RSpaceDataStore<String, CatalogValue> {
    let mut store = RSpaceDataStore::new();
    for i in 0..population {
        store
            .set_channel(format!("g0{}{i:016x}", "c".repeat(64)), [delta_gate_datum("resident")])
            .expect("a population channel loads with one datum");
    }
    store
        .set_channel(delta_gate_target(), [delta_gate_datum("target")])
        .expect("the target channel loads with one datum");
    assert!(store.pending_delta().is_empty(), "set_channel writes no pending delta");
    store
}

#[test]
fn one_channel_delta_allocates_independently_of_loaded_channel_count() {
    let target = delta_gate_target();
    let resident = RSpaceDataIndex::datum_path(&target, &delta_gate_datum("target"));
    let added = RSpaceDataIndex::datum_path(&target, &delta_gate_datum("added"));
    let cases: [(&[u8], bool, (u64, u64)); 3] = [
        (added.as_slice(), true, (1, 1)),
        (resident.as_slice(), false, (0, 0)),
        (resident.as_slice(), true, (2, 0)),
    ];
    let counts = |store: &RSpaceDataStore<String, CatalogValue>| {
        (store.index().count_path(&resident), store.index().count_path(&added))
    };
    let observe = |population: usize, aliased: bool| {
        cases.map(|(path, addition, expected)| {
            let mut store = delta_gate_store(population);
            let mut delta = BagDelta::new();
            let recorded = match addition {
                true => delta.record_addition(path, Multiplicity::ONE),
                false => delta.record_removal(path, Multiplicity::ONE),
            };
            recorded.expect("the gate delta records one change");
            let alias = aliased.then(|| store.clone());
            let (outcome, calls, bytes) = measure(|| store.apply_delta(&delta));
            outcome.expect("the one-channel delta applies");
            assert_eq!(counts(&store), expected);
            assert_eq!(store.loaded_channel_count(), population + 1);
            if let Some(alias) = alias {
                assert_eq!(counts(&alias), (1, 0), "the alias keeps its version");
                drop(alias);
            }
            (calls, bytes)
        })
    };
    let single = [observe(16, false), observe(16, true)];
    let wide = [observe(4096, false), observe(4096, true)];
    assert_eq!(
        wide[1], single[1],
        "one-channel delta allocations with a live alias grew with the loaded channel count: \
         (calls, bytes) of apply_delta for adding a new datum, removing the resident datum and \
         adding a second copy of it, at 4,097 loaded channels, then at 17"
    );
    assert_eq!(
        wide[0], single[0],
        "one-channel delta allocations grew with the loaded channel count: (calls, bytes) of \
         apply_delta for adding a new datum, removing the resident datum and adding a second copy \
         of it, at 4,097 loaded channels, then at 17"
    );
}
