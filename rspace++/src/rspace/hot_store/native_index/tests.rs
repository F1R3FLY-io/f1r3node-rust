use std::cell::Cell;
use std::cmp::Ordering as KeyOrdering;
use std::sync::Arc;

use proptest::prelude::*;

use super::*;
use crate::rspace::history::native_reader::measure_allocations;

/// A counting meter that sums every charge.
#[derive(Default)]
struct Totals {
    operations: Cell<usize>,
    scanned: Cell<usize>,
    backing: Cell<usize>,
}

impl SourceMeter for Totals {
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), RSpaceError> {
        self.operations.set(self.operations.get() + operations);
        self.scanned.set(self.scanned.get() + scanned);
        self.backing.set(self.backing.get() + backing);
        Ok(())
    }
}

impl Totals {
    fn get(&self) -> [usize; 3] { [self.operations.get(), self.scanned.get(), self.backing.get()] }
}

/// A key whose shard is `shard` and whose remaining bytes spread `n`
/// pseudo-randomly.
fn spread_key(shard: u8, n: u64) -> StoreKey {
    let mut bytes = [0; 32];
    bytes[0] = shard;
    bytes[1..9].copy_from_slice(&n.wrapping_mul(0x9E37_79B9_7F4A_7C15).to_be_bytes());
    bytes[24..32].copy_from_slice(&n.to_be_bytes());
    StoreKey(bytes)
}

/// A key that sorts by `n`.
fn ascending_key(n: u64) -> StoreKey {
    let mut bytes = [0; 32];
    bytes[24..32].copy_from_slice(&n.to_be_bytes());
    StoreKey(bytes)
}

/// The reference level bound: one, plus one for every h >= 0 with
/// 10 * 8^h <= entries.
fn reference_levels(entries: usize, base: u128) -> usize {
    let mut levels = 1;
    let mut minimum: u128 = 10;
    while minimum <= entries as u128 {
        levels += 1;
        minimum *= base;
    }
    levels
}

#[test]
fn ord_levels_bound_values_and_monotone() {
    assert_eq!(ord_levels_bound(0), 1);
    assert_eq!(ord_levels_bound(9), 1);
    assert_eq!(ord_levels_bound(10), 2);
    assert_eq!(ord_levels_bound(79), 2);
    assert_eq!(ord_levels_bound(80), 3);
    assert_eq!(ord_levels_bound(NATIVE_STORE_KEY_BOUND), 7);
    assert_eq!(ord_levels_bound(usize::MAX), reference_levels(usize::MAX, 8));
    let mut sizes: Vec<usize> = (0..5_000).collect();
    sizes.extend([65_535, 65_536, 1 << 20, (1 << 20) + 1, usize::MAX]);
    let mut previous = 0;
    for entries in sizes {
        let levels = ord_levels_bound(entries);
        assert_eq!(levels, reference_levels(entries, 8), "{entries} entries");
        assert!(previous <= levels);
        previous = levels;
    }
}

thread_local! {
    static COMPARISONS: Cell<usize> = const { Cell::new(0) };
}

/// A 32-byte key that counts its comparisons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CountingKey([u8; 32]);

impl PartialOrd for CountingKey {
    fn partial_cmp(&self, other: &Self) -> Option<KeyOrdering> { Some(self.cmp(other)) }
}

impl Ord for CountingKey {
    fn cmp(&self, other: &Self) -> KeyOrdering {
        COMPARISONS.with(|count| count.set(count.get() + 1));
        self.0.cmp(&other.0)
    }
}

fn counted<T>(action: impl FnOnce() -> T) -> (T, usize) {
    COMPARISONS.with(|count| count.set(0));
    let value = action();
    (value, COMPARISONS.with(Cell::get))
}

/// The levels of a map after removals: imbl lets a non-root leaf fall to 5
/// entries and a non-root branch to 6 children.
fn levels_after_removals(entries: usize) -> usize {
    let mut levels = 1;
    let mut minimum: u128 = 10;
    while minimum <= entries as u128 {
        levels += 1;
        minimum *= 6;
    }
    levels
}

fn assert_lookups_within(map: &imbl::OrdMap<CountingKey, u64>, present: CountingKey, bound: usize) {
    let (found, comparisons) = counted(|| map.get(&present).copied());
    assert!(found.is_some());
    assert!(comparisons <= bound, "{comparisons} > {bound} at {} entries", map.len());
    let mut absent = present;
    absent.0[31] ^= 0x55;
    absent.0[30] ^= 0xAA;
    let (_, comparisons) = counted(|| map.get(&absent).copied());
    assert!(comparisons <= bound, "{comparisons} > {bound} at {} entries", map.len());
}

/// D-S1 (D-C2b, DR-96): a lookup in an insert-built map makes at most
/// 5 * ord_levels_bound(len) comparisons, for random, ascending and bulk
/// construction; after removals the weaker fill bound holds.
#[test]
fn ord_lookup_comparisons_within_bound() {
    let builders: [fn(u64) -> StoreKey; 2] = [|n| spread_key(7, n), ascending_key];
    for make in builders {
        let mut map = imbl::OrdMap::new();
        for n in 0..(1_u64 << 16) {
            map.insert(CountingKey(make(n).0), n);
            let len = map.len();
            if len.is_power_of_two() || len.is_multiple_of(997) {
                let probe = CountingKey(make(n / 2).0);
                assert_lookups_within(&map, probe, ORD_NODE_COMPARISONS * ord_levels_bound(len));
            }
        }
        let len = map.len();
        let probe = CountingKey(make(12_345).0);
        assert_lookups_within(&map, probe, ORD_NODE_COMPARISONS * ord_levels_bound(len));
    }
    let bulk: imbl::OrdMap<CountingKey, u64> = (0..(1_u64 << 16))
        .map(|n| (CountingKey(spread_key(3, n).0), n))
        .collect();
    let probe = CountingKey(spread_key(3, 777).0);
    assert_lookups_within(&bulk, probe, ORD_NODE_COMPARISONS * ord_levels_bound(bulk.len()));
    let mut removed = bulk.clone();
    for n in (0..(1_u64 << 16)).filter(|n| n % 3 != 0) {
        removed.remove(&CountingKey(spread_key(3, n).0));
        if removed.len().is_multiple_of(1_009) {
            let probe = CountingKey(spread_key(3, (n / 3) * 3).0);
            assert_lookups_within(
                &removed,
                probe,
                ORD_NODE_COMPARISONS * levels_after_removals(removed.len()),
            );
        }
    }
}

/// D-S1 (D-C2b, DR-96): with a live snapshot sharing every node, an insert
/// allocates at most (2 * ord_levels_bound(len) + 2) nodes and a replace
/// at most ord_levels_bound(len) nodes.
#[test]
fn ord_write_allocations_within_bound() {
    let node = ord_node_bytes().expect("node bytes");
    let mut map: imbl::OrdMap<StoreKey, Arc<u64>> = imbl::OrdMap::new();
    for n in 0..20_000_u64 {
        let key = if n % 2 == 0 {
            spread_key(9, n)
        } else {
            ascending_key(n)
        };
        let value = Arc::new(n);
        let snapshot = map.clone();
        let len = map.len();
        let ((), inserted) = measure_allocations(|| {
            map.insert(key, value);
        });
        assert!(
            inserted <= (2 * ord_levels_bound(len) + 2) * node,
            "insert at {len} entries allocated {inserted}"
        );
        drop(snapshot);
        let replacement = Arc::new(n + 1);
        let snapshot = map.clone();
        let len = map.len();
        let ((), replaced) = measure_allocations(|| {
            *map.get_mut(&key).expect("a present key") = replacement;
        });
        assert!(
            replaced <= ord_levels_bound(len) * node,
            "replace at {len} entries allocated {replaced}"
        );
        drop(snapshot);
    }
}

/// D-S1 (D-C2b, DR-96): the node sizes of the charge are the sizes that the
/// allocator sees. A replace in a one-leaf map copies one leaf; the 17th
/// insert into a full leaf copies the leaf and allocates the split sibling,
/// the transient default leaf and the new root; a replace in a two-level map
/// copies one branch and one leaf.
#[test]
fn ord_node_bytes_match_allocator() {
    let leaf = arc_allocation_bytes::<OrdLeafLayout>().expect("leaf bytes");
    let branch = arc_allocation_bytes::<OrdBranchLayout>().expect("branch bytes");
    assert_eq!(ord_node_bytes(), Some(leaf.max(branch)));

    let mut map: imbl::OrdMap<StoreKey, Arc<u64>> =
        (0..16).map(|n| (ascending_key(n), Arc::new(n))).collect();
    let snapshot = map.clone();
    let replacement = Arc::new(99);
    let ((), copied) = measure_allocations(|| {
        *map.get_mut(&ascending_key(3)).expect("a present key") = replacement;
    });
    assert_eq!(copied, leaf);
    drop(snapshot);

    let mut map: imbl::OrdMap<StoreKey, Arc<u64>> =
        (0..16).map(|n| (ascending_key(n), Arc::new(n))).collect();
    let snapshot = map.clone();
    let value = Arc::new(16);
    let ((), split) = measure_allocations(|| {
        map.insert(ascending_key(16), value);
    });
    assert_eq!(split, 3 * leaf + branch);
    drop(snapshot);

    let mut map: imbl::OrdMap<StoreKey, Arc<u64>> =
        (0..100).map(|n| (ascending_key(n), Arc::new(n))).collect();
    let snapshot = map.clone();
    let replacement = Arc::new(0);
    let ((), copied) = measure_allocations(|| {
        *map.get_mut(&ascending_key(50)).expect("a present key") = replacement;
    });
    assert_eq!(copied, leaf + branch);
    drop(snapshot);
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Read,
    Write(u64),
}

/// One operation of a key's program on the index: a read inserts an absent
/// key (a cold fill) or finds a present one, and a write replaces a present
/// key.
fn run_step(index: &DigestShards<u64, u64>, key: StoreKey, step: Step, meter: &Totals) {
    let mut shard = index.write(&key);
    match step {
        Step::Read => {
            let present = DigestShards::get(&shard, &key, meter)
                .expect("an unlimited meter")
                .is_some();
            if !present {
                index
                    .insert_new(&mut shard, key, Arc::new(1), 0, false, meter)
                    .expect("an absent key");
            }
        }
        Step::Write(value) => {
            let present = DigestShards::get(&shard, &key, meter)
                .expect("an unlimited meter")
                .is_some();
            assert!(present);
            DigestShards::<u64, u64>::reserve_replace(meter).expect("an unlimited meter");
            DigestShards::replace(&mut shard, &key, value);
        }
    }
}

const PROGRAM: [Step; 3] = [Step::Read, Step::Write(5), Step::Read];

/// The total charge of a schedule: `schedule[i]` names the key of step i,
/// and each key runs PROGRAM in order.
fn schedule_total(keys: &[StoreKey], schedule: &[usize]) -> [usize; 3] {
    let index = DigestShards::<u64, u64>::new();
    let meter = Totals::default();
    let mut positions = vec![0; keys.len()];
    for &key in schedule {
        run_step(&index, keys[key], PROGRAM[positions[key]], &meter);
        positions[key] += 1;
    }
    meter.get()
}

/// Every interleaving of two programs.
fn interleavings(left: usize, right: usize) -> Vec<Vec<usize>> {
    if left == 0 {
        return vec![vec![1; right]];
    }
    if right == 0 {
        return vec![vec![0; left]];
    }
    let mut all = Vec::new();
    for mut rest in interleavings(left - 1, right) {
        rest.insert(0, 0);
        all.push(rest);
    }
    for mut rest in interleavings(left, right - 1) {
        rest.insert(0, 1);
        all.push(rest);
    }
    all
}

/// D-S1 (D-C2b, DR-96): two keys in one shard, every interleaving of their
/// programs: the totals are equal. The legacy store gives these schedules
/// different totals (legacy_population_charges_depend_on_schedule).
#[test]
fn native_store_charges_are_schedule_independent() {
    let keys = [spread_key(42, 1), spread_key(42, 2)];
    let schedules = interleavings(PROGRAM.len(), PROGRAM.len());
    assert_eq!(schedules.len(), 20);
    let expected = schedule_total(&keys, &schedules[0]);
    for schedule in &schedules {
        assert_eq!(schedule_total(&keys, schedule), expected, "{schedule:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// D-S1 (D-C2b, DR-96): any number of keys in one shard and any schedule
    /// that keeps each key's program order have the total of running the
    /// keys one after another.
    #[test]
    fn random_schedules_have_the_sequential_total(
        count in 1_usize..6,
        choices in prop::collection::vec(any::<prop::sample::Index>(), 0..40),
    ) {
        let keys: Vec<StoreKey> = (0..count as u64).map(|n| spread_key(17, n)).collect();
        let mut remaining = vec![PROGRAM.len(); count];
        let mut schedule = Vec::new();
        for choice in choices {
            let open: Vec<usize> = (0..count).filter(|&key| remaining[key] > 0).collect();
            if open.is_empty() {
                break;
            }
            let key = open[choice.index(open.len())];
            remaining[key] -= 1;
            schedule.push(key);
        }
        for (key, left) in remaining.iter().enumerate() {
            schedule.extend(std::iter::repeat_n(key, *left));
        }
        let sequential: Vec<usize> =
            (0..count).flat_map(|key| std::iter::repeat_n(key, PROGRAM.len())).collect();
        prop_assert_eq!(schedule_total(&keys, &schedule), schedule_total(&keys, &sequential));
    }
}

/// D-S1 (D-C2b, DR-96): the charges do not depend on the store's key limit
/// in use or on the shard: a search, a replace and an insert each have one
/// fixed charge.
#[test]
fn index_charges_are_fixed() {
    let search = search_charge();
    assert_eq!(search.operations, 35);
    assert_eq!(search.scanned, 7 * (5 * 64 + 24));
    assert_eq!(search.backing, 0);
    let node = ord_node_bytes().expect("node bytes");
    let replace = replace_charge::<u64, u64>().expect("replace charge");
    let insert = insert_charge::<u64, u64>().expect("insert charge");
    assert_eq!(insert.backing - replace.backing, (ORD_LEVELS + 2) * node);
    assert!(insert.scanned >= 15 * 1024);
}
