use std::cell::Cell;
use std::hash::Hasher;
use std::sync::Arc;

use shared::rust::clone_backing::{BackingError, Walker};

use super::*;
use crate::rspace::history::native_reader::measure_allocations;
use crate::rspace::trace::event::{Consume, Produce};

struct Meter {
    remaining: Cell<usize>,
    calls: Cell<usize>,
    backing: Cell<usize>,
    operations: Cell<usize>,
    scanned: Cell<usize>,
}

impl Meter {
    fn new(remaining: usize) -> Self {
        Self {
            remaining: Cell::new(remaining),
            calls: Cell::new(0),
            backing: Cell::new(0),
            operations: Cell::new(0),
            scanned: Cell::new(0),
        }
    }
}

impl SourceMeter for Meter {
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), RSpaceError> {
        if self.remaining.get() == 0 {
            return Err(RSpaceError::HostWorkRejected);
        }
        self.remaining.set(self.remaining.get() - 1);
        self.calls.set(self.calls.get() + 1);
        self.backing.set(self.backing.get() + backing);
        self.operations.set(self.operations.get() + operations);
        self.scanned.set(self.scanned.get() + scanned);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CollisionKey(usize, String);

impl Hash for CollisionKey {
    fn hash<H: Hasher>(&self, hasher: &mut H) { hasher.write_u8(0); }
}

impl CloneBacking for CollisionKey {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        walker.push(&self.0)?;
        walker.push(&self.1)
    }
}

fn map<K: Clone + Hash + Eq, V: Clone>() -> ShardedMap<K, V> {
    ShardedMap::from_shards(Box::new(std::array::from_fn(|_| imbl::HashMap::new())))
}

fn populate(map: &ShardedMap<CollisionKey, Vec<String>>, count: usize) {
    for index in 0..count {
        map.insert(CollisionKey(index, "key".repeat(31)), vec!["payload".repeat(200); 2]);
    }
}

#[test]
fn collision_copy_on_write_and_warm_clones_fit_prepaid_backing() {
    for count in [0, 1, 2, 15, 16, 17, 31, 32, 33, 64, 128] {
        let map = map();
        populate(&map, count);
        let checkpoint = map.snapshot_shards();
        let key = CollisionKey(count, "new key".repeat(35));
        let values = vec!["new payload".repeat(350); 2];
        let meter = Meter::new(usize::MAX);
        let (result, bytes) = measure_allocations(|| map.native_insert_new(&key, values, &meter));
        result.unwrap();
        assert!(
            bytes <= meter.backing.get(),
            "count={count}, allocated={bytes}, paid={}",
            meter.backing.get()
        );
        assert_eq!(map.len(), count + 1);
        assert_eq!(checkpoint.iter().map(imbl::HashMap::len).sum::<usize>(), count);
        let meter = Meter::new(usize::MAX);
        let (result, bytes) = measure_allocations(|| map.native_get(&key, &meter));
        assert_eq!(result.unwrap().unwrap(), vec!["new payload".repeat(350); 2]);
        assert!(bytes <= meter.backing.get());
        map.restore_shards(checkpoint);
        assert_eq!(map.len(), count);
        assert!(map.get(&key).is_none());
    }
}

#[test]
fn warm_cache_copy_prepays_cleanup_before_caller_rejection() {
    let map = map::<CollisionKey, Vec<String>>();
    let key = CollisionKey(7, "warm".to_owned());
    let payload = vec!["nested".to_owned(); 512];
    map.insert(key.clone(), payload.clone());
    let one_copy = Meter::new(usize::MAX);
    native_backing::reserve(&payload, &one_copy).unwrap();
    let baseline = Meter::new(usize::MAX);
    assert_eq!(map.native_get(&key, &baseline).unwrap(), Some(payload.clone()));
    let rejecting = Meter::new(baseline.calls.get());
    let result = (|| {
        let value = map.native_get(&key, &rejecting)?;
        rejecting.reserve(1, 0, 0)?;
        Ok::<_, RSpaceError>(value)
    })();
    assert_eq!(result, Err(RSpaceError::HostWorkRejected));
    assert!(rejecting.operations.get() >= one_copy.operations.get() * 2);
    assert_eq!(map.get(&key), Some(payload));
}

#[test]
fn shared_map_insertion_prepays_copied_entry_cleanup() {
    let small = vec!["payload".to_owned()];
    let large = vec!["payload".to_owned(); 512];
    let copy_work = |value: &Vec<String>| {
        let meter = Meter::new(usize::MAX);
        native_backing::reserve(value, &meter).unwrap();
        meter.operations.get()
    };
    let added_copy_work = copy_work(&large) - copy_work(&small);
    assert!(added_copy_work > 0);
    for replace in [false, true] {
        let work = |value: Vec<String>| {
            let map = map();
            let key = CollisionKey(1, "key".to_owned());
            map.insert(key.clone(), value);
            let _checkpoint = map.snapshot_shards();
            let meter = Meter::new(usize::MAX);
            if replace {
                map.native_insert_replace(&key, Vec::new(), &meter).unwrap();
            } else {
                map.native_insert_new(&CollisionKey(2, "candidate".to_owned()), Vec::new(), &meter)
                    .unwrap();
            }
            meter.operations.get()
        };
        assert!(work(large.clone()) - work(small.clone()) >= added_copy_work * 2);
    }
}

#[test]
fn every_collision_insertion_cut_keeps_the_checkpoint_and_map_unchanged() {
    let baseline = map();
    populate(&baseline, 17);
    let _checkpoint = baseline.snapshot_shards();
    let key = CollisionKey(17, "candidate".repeat(25));
    let meter = Meter::new(usize::MAX);
    baseline
        .native_insert_new(&key, vec!["new".repeat(700)], &meter)
        .unwrap();
    for accepted in 0..meter.calls.get() {
        let map = map();
        populate(&map, 17);
        let checkpoint = map.snapshot_shards();
        let meter = Meter::new(accepted);
        let values = vec!["new".repeat(700)];
        let (result, bytes) = measure_allocations(|| map.native_insert_new(&key, values, &meter));
        assert_eq!(result, Err(RSpaceError::HostWorkRejected), "cut={accepted}");
        assert!(
            bytes <= meter.backing.get(),
            "cut={accepted}, allocated={bytes}, paid={}",
            meter.backing.get()
        );
        assert_eq!(map.len(), 17);
        assert!(map.get(&key).is_none());
        assert_eq!(checkpoint.iter().map(imbl::HashMap::len).sum::<usize>(), 17);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(align(256))]
struct AlignedKey(u64);

impl Hash for AlignedKey {
    fn hash<H: Hasher>(&self, hasher: &mut H) { hasher.write_u8(0); }
}

impl CloneBacking for AlignedKey {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        walker.push(&self.0)
    }
}

#[derive(Clone)]
#[repr(align(256))]
struct AlignedValue(Vec<String>);

impl CloneBacking for AlignedValue {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        walker.push(&self.0)
    }
}

#[test]
fn aligned_entry_layouts_fit_the_persistent_map_bound() {
    for count in [0, 1, 16, 32, 64] {
        let map = map();
        for key in 0..count {
            map.insert(AlignedKey(key), AlignedValue(vec!["payload".repeat(100); 2]));
        }
        let checkpoint = map.snapshot_shards();
        let meter = Meter::new(usize::MAX);
        let value = AlignedValue(vec!["new payload".repeat(150)]);
        let (result, allocated) =
            measure_allocations(|| map.native_insert_new(&AlignedKey(count), value, &meter));
        result.unwrap();
        assert!(
            allocated <= meter.backing.get(),
            "count={count}, actual={allocated}, paid={}",
            meter.backing.get()
        );
        assert_eq!(checkpoint.iter().map(imbl::HashMap::len).sum::<usize>(), count as usize);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn persistent_hash_promotions_fit_the_metadata_and_payload_bound(
        keys in prop::collection::vec(any::<u64>(), 0..150),
        payload in "[a-z]{0,160}",
        candidate in any::<u64>(),
    ) {
        let map = map();
        {
            let mut shard = map.shards[shard_of(&candidate)].write().unwrap();
            for key in keys {
                shard.insert(key, vec![payload.clone(); 3]);
            }
        }
        let checkpoint = map.snapshot_shards();
        let before = map.len();
        let already_present = map.get(&candidate).is_some();
        let value = vec![payload; 3];
        let meter = Meter::new(usize::MAX);
        let (result, allocated) = measure_allocations(|| map.native_insert_new(&candidate, value, &meter));
        prop_assert!(allocated <= meter.backing.get());
        prop_assert_eq!(result.is_err(), already_present);
        prop_assert_eq!(map.len(), before + usize::from(!already_present));
        prop_assert_eq!(checkpoint.iter().map(imbl::HashMap::len).sum::<usize>(), before);
    }
}

struct EmptyReader;

impl HistoryReaderBase<String, String, String, String> for EmptyReader {
    fn get_data_proj(&self, _: &String) -> Vec<Datum<String>> { vec![] }
    fn get_continuations_proj(&self, _: &Vec<String>) -> Vec<WaitingContinuation<String, String>> {
        vec![]
    }
    fn get_joins_proj(&self, _: &String) -> Vec<Vec<String>> { vec![] }
}

type Store = Box<dyn HotStore<String, String, String, String>>;

fn store() -> Store { HotStoreInstances::create_from_hr(Box::new(EmptyReader)) }

#[test]
fn native_datum_publication_reserves_before_mutation_and_preserves_snapshots() {
    let channel = "channel".repeat(60);
    let datum = Datum::create(&channel, "new value".repeat(150), false);
    let prepare = || {
        let store = store();
        store.put_datum(&channel, Datum::create(&channel, "old value".repeat(120), true));
        let snapshot = store.snapshot();
        (store, snapshot)
    };
    let (baseline, original) = prepare();
    let meter = Meter::new(usize::MAX);
    let candidate = datum.clone();
    let (result, allocated) =
        measure_allocations(|| baseline.put_datum_metered(&channel, candidate, &meter));
    result.unwrap();
    assert!(allocated <= meter.backing.get());
    assert_eq!(baseline.get_data(&channel).len(), 2);
    assert_eq!(original.data_flat().get(&channel).unwrap().len(), 1);
    for accepted in 0..meter.calls.get() {
        let (store, snapshot) = prepare();
        let meter = Meter::new(accepted);
        let candidate = datum.clone();
        let (result, allocated) =
            measure_allocations(|| store.put_datum_metered(&channel, candidate, &meter));
        assert_eq!(result, Err(RSpaceError::HostWorkRejected), "cut={accepted}");
        assert!(allocated <= meter.backing.get());
        assert_eq!(store.get_data(&channel), snapshot.data_flat()[&channel]);
    }
    let absent = store();
    assert!(
        absent
            .put_datum_metered(&channel, datum, &Meter::new(usize::MAX))
            .is_err()
    );
}

#[test]
fn stored_consume_reserves_all_continuation_and_join_writes_before_mutation() {
    let channels = vec!["left".repeat(15), "right".repeat(15), "left".repeat(15)];
    let old = WaitingContinuation {
        patterns: vec!["old".to_owned(); 3],
        continuation: "old body".to_owned(),
        persist: true,
        peeks: Default::default(),
        source: Consume::default(),
    };
    let waiting = WaitingContinuation {
        patterns: vec!["new pattern".repeat(10); 3],
        continuation: "new body".repeat(30),
        persist: false,
        peeks: [0, 2].into_iter().collect(),
        source: Consume::default(),
    };
    let prepare = || {
        let store = store();
        store.put_continuation(&channels, old.clone());
        let setup = Meter::new(usize::MAX);
        for channel in &channels {
            store
                .get_joins_with_reader(channel, &|| Ok(Vec::new()), &setup)
                .unwrap();
        }
        let snapshot = store.snapshot();
        (store, snapshot)
    };
    let (baseline, original) = prepare();
    let meter = Meter::new(usize::MAX);
    let candidate = waiting.clone();
    let (result, allocated) =
        measure_allocations(|| baseline.store_consume_metered(&channels, candidate, &meter));
    assert_eq!(result.unwrap(), (true, 2));
    assert!(allocated <= meter.backing.get());
    assert_eq!(baseline.get_continuations(&channels), vec![waiting.clone(), old.clone()]);
    assert_eq!(original.continuations_flat()[&channels], vec![old.clone()]);
    for channel in &channels {
        assert_eq!(baseline.get_joins(channel), vec![channels.clone()]);
    }
    let duplicate = Meter::new(usize::MAX);
    assert_eq!(
        baseline
            .store_consume_metered(&channels, waiting.clone(), &duplicate)
            .unwrap(),
        (false, 2)
    );
    assert_eq!(baseline.get_continuations(&channels).len(), 2);
    for accepted in 0..meter.calls.get() {
        let (store, snapshot) = prepare();
        let meter = Meter::new(accepted);
        let candidate = waiting.clone();
        let (result, allocated) =
            measure_allocations(|| store.store_consume_metered(&channels, candidate, &meter));
        assert_eq!(result, Err(RSpaceError::HostWorkRejected), "cut={accepted}");
        assert!(allocated <= meter.backing.get(), "cut={accepted}");
        let state = store.snapshot();
        assert_eq!(state.continuations_flat(), snapshot.continuations_flat());
        assert_eq!(state.joins_flat(), snapshot.joins_flat());
    }
}

#[test]
fn native_data_retirement_prepares_every_channel_before_publication() {
    let left = "left".repeat(35);
    let right = "right".repeat(35);
    let prepare = || {
        let store = store();
        for index in 0..3 {
            store.put_datum(&left, Datum::create(&left, format!("left {index}"), false));
        }
        for index in 0..2 {
            store.put_datum(&right, Datum::create(&right, format!("right {index}"), false));
        }
        let snapshot = store.snapshot();
        (store, snapshot)
    };
    let data = [left.clone(), right.clone(), left.clone()]
        .into_iter()
        .map(|channel| RSpaceResult {
            channel,
            matched_datum: String::new(),
            removed_datum: String::new(),
            persistent: false,
        })
        .collect::<Vec<_>>();
    let retirement = [(0, 2), (1, 0), (2, 0)];
    let (baseline, original) = prepare();
    let meter = Meter::new(usize::MAX);
    let (result, allocated) =
        measure_allocations(|| baseline.retire_data_metered(&data, &retirement, &meter));
    result.unwrap();
    assert!(allocated <= meter.backing.get());
    let (legacy, _) = prepare();
    for (position, index) in retirement {
        legacy.remove_datum(&data[position].channel, index).unwrap();
    }
    assert_eq!(baseline.snapshot().data_flat(), legacy.snapshot().data_flat());
    assert_eq!(original.data_flat()[&left].len(), 3);
    for accepted in 0..meter.calls.get() {
        let (store, snapshot) = prepare();
        let meter = Meter::new(accepted);
        let (result, allocated) =
            measure_allocations(|| store.retire_data_metered(&data, &retirement, &meter));
        assert_eq!(result, Err(RSpaceError::HostWorkRejected), "cut={accepted}");
        assert!(allocated <= meter.backing.get(), "cut={accepted}");
        assert_eq!(store.snapshot().data_flat(), snapshot.data_flat());
    }
    let (store, snapshot) = prepare();
    assert!(
        store
            .retire_data_metered(&data, &[(0, 2), (1, 99)], &Meter::new(usize::MAX))
            .is_err()
    );
    assert_eq!(store.snapshot().data_flat(), snapshot.data_flat());
}

#[test]
fn matched_produce_retirement_prepares_data_continuation_and_joins_together() {
    let left = "left".repeat(35);
    let right = "right".repeat(35);
    let channels = vec![left.clone(), right.clone()];
    let waiting = WaitingContinuation {
        patterns: vec!["left pattern".to_owned(), "right pattern".to_owned()],
        continuation: "body".repeat(40),
        persist: false,
        peeks: Default::default(),
        source: Consume::default(),
    };
    let prepare = || {
        let store = store();
        for channel in &channels {
            store.put_datum(channel, Datum::create(channel, "old".repeat(70), false));
            store.put_datum(channel, Datum::create(channel, "new".repeat(70), false));
            store.put_join(channel, &channels);
        }
        store.put_continuation(&channels, waiting.clone());
        let snapshot = store.snapshot();
        (store, snapshot)
    };
    let data = channels
        .iter()
        .cloned()
        .map(|channel| RSpaceResult {
            channel,
            matched_datum: String::new(),
            removed_datum: String::new(),
            persistent: false,
        })
        .collect::<Vec<_>>();
    let retirement = [(0, 1), (1, 0)];
    let (baseline, original) = prepare();
    let meter = Meter::new(usize::MAX);
    let (result, allocated) = measure_allocations(|| {
        baseline.retire_produce_match_metered(&channels, 0, false, &data, &retirement, &meter)
    });
    result.unwrap();
    assert!(allocated <= meter.backing.get());
    let (legacy, _) = prepare();
    for (position, index) in retirement {
        legacy.remove_datum(&data[position].channel, index).unwrap();
    }
    legacy.remove_continuation(&channels, 0).unwrap();
    for datum in &data {
        legacy.remove_join(&datum.channel, &channels).unwrap();
    }
    let state = baseline.snapshot();
    let expected = legacy.snapshot();
    assert_eq!(state.data_flat(), expected.data_flat());
    assert_eq!(state.continuations_flat(), expected.continuations_flat());
    assert_eq!(state.joins_flat(), expected.joins_flat());
    assert_eq!(original.continuations_flat()[&channels].len(), 1);
    for accepted in 0..meter.calls.get() {
        let (store, snapshot) = prepare();
        let meter = Meter::new(accepted);
        let (result, allocated) = measure_allocations(|| {
            store.retire_produce_match_metered(&channels, 0, false, &data, &retirement, &meter)
        });
        assert_eq!(result, Err(RSpaceError::HostWorkRejected), "cut={accepted}");
        assert!(allocated <= meter.backing.get(), "cut={accepted}");
        let state = store.snapshot();
        assert_eq!(state.data_flat(), snapshot.data_flat(), "cut={accepted}");
        assert_eq!(state.continuations_flat(), snapshot.continuations_flat(), "cut={accepted}");
        assert_eq!(state.joins_flat(), snapshot.joins_flat(), "cut={accepted}");
    }
    let (store, snapshot) = prepare();
    assert!(
        store
            .retire_produce_match_metered(
                &channels,
                99,
                false,
                &data,
                &retirement,
                &Meter::new(usize::MAX),
            )
            .is_err()
    );
    let state = store.snapshot();
    assert_eq!(state.data_flat(), snapshot.data_flat());
    assert_eq!(state.continuations_flat(), snapshot.continuations_flat());
    assert_eq!(state.joins_flat(), snapshot.joins_flat());
}

#[test]
fn matched_produce_retirement_preserves_persistent_and_installed_joins() {
    let channels = vec!["left".repeat(25), "right".repeat(25)];
    let data = channels
        .iter()
        .cloned()
        .map(|channel| RSpaceResult {
            channel,
            matched_datum: String::new(),
            removed_datum: String::new(),
            persistent: false,
        })
        .collect::<Vec<_>>();
    for persistent in [false, true] {
        for installed in [false, true] {
            let prepare = || {
                let store = store();
                let waiting = WaitingContinuation {
                    patterns: vec!["pattern".to_owned(); 2],
                    continuation: "body".repeat(30),
                    persist: persistent,
                    peeks: Default::default(),
                    source: Consume::default(),
                };
                if installed {
                    store.install_continuation(&channels, waiting.clone());
                }
                store.put_continuation(&channels, waiting);
                for channel in &channels {
                    store.put_datum(channel, Datum::create(channel, "value".repeat(30), false));
                    store.put_join(channel, &channels);
                }
                store
            };
            let baseline = prepare();
            let legacy = prepare();
            let meter = Meter::new(usize::MAX);
            baseline
                .retire_produce_match_metered(
                    &channels,
                    i32::from(installed),
                    persistent,
                    &data,
                    &[(0, 0), (1, 0)],
                    &meter,
                )
                .unwrap();
            for channel in &channels {
                legacy.remove_datum(channel, 0).unwrap();
            }
            if !persistent {
                legacy
                    .remove_continuation(&channels, i32::from(installed))
                    .unwrap();
            }
            for channel in &channels {
                legacy.remove_join(channel, &channels).unwrap();
            }
            let state = baseline.snapshot();
            let expected = legacy.snapshot();
            assert_eq!(state.data_flat(), expected.data_flat());
            assert_eq!(state.continuations_flat(), expected.continuations_flat());
            assert_eq!(state.joins_flat(), expected.joins_flat());
            assert_eq!(
                state.installed_continuations_flat(),
                expected.installed_continuations_flat()
            );
        }
    }
}

struct Rows {
    data: Vec<Datum<String>>,
    continuations: Vec<WaitingContinuation<String, String>>,
    joins: Vec<Vec<String>>,
}

impl Rows {
    fn new() -> Self {
        Self {
            data: vec![
                Datum {
                    a: "datum".repeat(1000),
                    persist: true,
                    source: Produce::default()
                };
                3
            ],
            continuations: vec![
                WaitingContinuation {
                    patterns: vec!["pattern".repeat(100); 3],
                    continuation: "continuation".repeat(500),
                    persist: true,
                    peeks: [0, 2].into_iter().collect(),
                    source: Consume::default(),
                };
                2
            ],
            joins: vec![vec!["channel".repeat(500); 3]; 2],
        }
    }

    fn query(&self, store: &Store, kind: usize, meter: &Meter) -> Result<(), RSpaceError> {
        let channel = String::new();
        match kind {
            0 => {
                let result = store.get_data_with_reader(
                    &channel,
                    &|| {
                        native_backing::reserve_copy_and_cleanup(&self.data, meter)?;
                        Ok(self.data.clone())
                    },
                    meter,
                )?;
                assert_eq!(result, self.data);
            }
            1 => {
                let result = store.get_continuations_with_reader(
                    &[],
                    &|| {
                        native_backing::reserve_copy_and_cleanup(&self.continuations, meter)?;
                        Ok(self.continuations.clone())
                    },
                    meter,
                )?;
                assert_eq!(result[0].continuation, "installed");
                assert_eq!(&result[1..], self.continuations);
            }
            _ => {
                let result = store.get_joins_with_reader(
                    &channel,
                    &|| {
                        native_backing::reserve_copy_and_cleanup(&self.joins, meter)?;
                        Ok(self.joins.clone())
                    },
                    meter,
                )?;
                assert_eq!(result[0], ["installed"]);
                assert_eq!(&result[1..], self.joins);
            }
        }
        Ok(())
    }
}

fn installed() -> Store {
    let store = store();
    store.install_continuation(&[], WaitingContinuation {
        continuation: "installed".to_owned(),
        ..Default::default()
    });
    store.install_join(&String::new(), &["installed".to_owned()]);
    store
}

#[test]
fn owned_cache_copy_prepays_rollback_cleanup() {
    let data = Rows::new().data;
    let copy = Meter::new(usize::MAX);
    native_backing::reserve(&data, &copy).unwrap();
    let cleanup = Meter::new(usize::MAX);
    native_backing::inspect(&data, &cleanup).unwrap();
    let copy_and_cleanup = Meter::new(usize::MAX);
    native_backing::reserve_copy_and_cleanup(&data, &copy_and_cleanup).unwrap();
    assert_eq!(copy_and_cleanup.operations.get(), copy.operations.get() + cleanup.operations.get());
    assert_eq!(copy_and_cleanup.scanned.get(), copy.scanned.get() + cleanup.scanned.get());
    assert_eq!(copy_and_cleanup.backing.get(), copy.backing.get() + cleanup.backing.get());
    let rejected = Meter::new(0);
    let (result, allocated) = measure_allocations(|| {
        native_backing::reserve_copy_and_cleanup(&data, &rejected)?;
        Ok::<_, RSpaceError>(data.clone())
    });
    assert_eq!(result, Err(RSpaceError::HostWorkRejected));
    assert_eq!(allocated, 0);
}

#[test]
fn owned_copy_prepays_nested_arc_cleanup() {
    let data = vec![Arc::<str>::from("payload".repeat(1024))];
    let copy = Meter::new(usize::MAX);
    native_backing::reserve(&data, &copy).unwrap();
    let cleanup = Meter::new(usize::MAX);
    native_backing::inspect(&data, &cleanup).unwrap();
    let both = Meter::new(usize::MAX);
    native_backing::reserve_copy_and_cleanup(&data, &both).unwrap();
    assert!(cleanup.scanned.get() > copy.scanned.get());
    assert_eq!(both.operations.get(), copy.operations.get() + cleanup.operations.get());
    assert_eq!(both.scanned.get(), copy.scanned.get() + cleanup.scanned.get());
    assert_eq!(both.backing.get(), copy.backing.get() + cleanup.backing.get());
}

fn export_store() -> Store {
    let store = store();
    let channel = "export-channel".to_owned();
    store.put_datum(&channel, Datum {
        a: "export-data".repeat(240),
        persist: false,
        source: Produce::default(),
    });
    store.put_continuation(std::slice::from_ref(&channel), WaitingContinuation {
        patterns: vec!["export-pattern".repeat(90)],
        continuation: "export-body".repeat(120),
        persist: true,
        peeks: Default::default(),
        source: Consume::default(),
    });
    store.put_join(&channel, std::slice::from_ref(&channel));
    let empty = "empty-export-row".to_owned();
    store.get_data(&empty);
    store.get_continuations(std::slice::from_ref(&empty));
    store.get_joins(&empty);
    store
}

#[test]
fn export_changes_match_legacy_order_and_reject_every_short_reservation() {
    let store = export_store();
    let expected = store.changes();
    let before = store.snapshot();
    let meter = Meter::new(usize::MAX);
    let (actual, allocated) = measure_allocations(|| store.changes_metered(&meter));
    assert_eq!(actual.unwrap(), expected);
    assert!(allocated <= meter.backing.get());
    for accepted in 0..meter.calls.get() {
        let meter = Meter::new(accepted);
        let (result, allocated) = measure_allocations(|| store.changes_metered(&meter));
        assert_eq!(result, Err(RSpaceError::HostWorkRejected), "cut={accepted}");
        assert!(
            allocated <= meter.backing.get(),
            "cut={accepted}, allocated={allocated}, paid={}",
            meter.backing.get()
        );
        let after = store.snapshot();
        assert_eq!(after.data_flat(), before.data_flat());
        assert_eq!(after.continuations_flat(), before.continuations_flat());
        assert_eq!(after.joins_flat(), before.joins_flat());
    }
}

#[test]
fn export_late_rejection_uses_prepaid_cleanup_for_earlier_payloads() {
    let store = store();
    let channel = "first-export".to_owned();
    let waiting = WaitingContinuation {
        patterns: vec!["pattern".to_owned(); 512],
        continuation: "body".to_owned(),
        persist: true,
        peeks: Default::default(),
        source: Consume::default(),
    };
    let one_copy = Meter::new(usize::MAX);
    native_backing::reserve(&waiting, &one_copy).unwrap();
    store.put_continuation(std::slice::from_ref(&channel), waiting);
    let first = Meter::new(usize::MAX);
    assert_eq!(store.changes_metered(&first).unwrap().len(), 1);
    store.put_datum(&"later-export".to_owned(), Datum {
        a: "later".to_owned(),
        persist: false,
        source: Produce::default(),
    });
    let before = store.snapshot();
    let rejected = Meter::new(first.calls.get());
    assert_eq!(store.changes_metered(&rejected), Err(RSpaceError::HostWorkRejected),);
    assert!(rejected.operations.get() >= one_copy.operations.get() * 2);
    let after = store.snapshot();
    assert_eq!(after.data_flat(), before.data_flat());
    assert_eq!(after.continuations_flat(), before.continuations_flat());
}

#[test]
fn cold_and_warm_reads_pay_before_all_cache_and_result_copies() {
    let rows = Rows::new();
    for kind in 0..3 {
        let store = installed();
        let checkpoint = store.snapshot();
        for _ in 0..2 {
            let meter = Meter::new(usize::MAX);
            let (result, allocated) = measure_allocations(|| rows.query(&store, kind, &meter));
            result.unwrap();
            assert!(
                allocated <= meter.backing.get(),
                "kind={kind}, allocated={allocated}, paid={}",
                meter.backing.get()
            );
        }
        assert!(checkpoint.data_flat().is_empty());
        assert!(checkpoint.continuations_flat().is_empty());
        assert!(checkpoint.joins_flat().is_empty());
    }
}

#[test]
fn every_cold_read_cut_preserves_cache_and_installed_rows() {
    let rows = Rows::new();
    for kind in 0..3 {
        let meter = Meter::new(usize::MAX);
        rows.query(&installed(), kind, &meter).unwrap();
        for accepted in 0..meter.calls.get() {
            let store = installed();
            let before = store.snapshot();
            let meter = Meter::new(accepted);
            let (result, allocated) = measure_allocations(|| rows.query(&store, kind, &meter));
            assert_eq!(result, Err(RSpaceError::HostWorkRejected), "kind={kind}, cut={accepted}");
            assert!(
                allocated <= meter.backing.get(),
                "kind={kind}, cut={accepted}, allocated={allocated}, paid={}",
                meter.backing.get()
            );
            let after = store.snapshot();
            assert_eq!(after.data_flat(), before.data_flat());
            assert_eq!(after.continuations_flat(), before.continuations_flat());
            assert_eq!(after.joins_flat(), before.joins_flat());
            assert_eq!(after.installed_continuations_flat(), before.installed_continuations_flat());
            assert_eq!(after.installed_joins_flat(), before.installed_joins_flat());
        }
    }
}

#[test]
fn every_warm_read_cut_keeps_owned_copies_within_accepted_credit() {
    let rows = Rows::new();
    for kind in 0..3 {
        let store = installed();
        rows.query(&store, kind, &Meter::new(usize::MAX)).unwrap();
        let before = store.snapshot();
        let meter = Meter::new(usize::MAX);
        rows.query(&store, kind, &meter).unwrap();
        for accepted in 0..meter.calls.get() {
            let meter = Meter::new(accepted);
            let (result, allocated) = measure_allocations(|| rows.query(&store, kind, &meter));
            assert_eq!(result, Err(RSpaceError::HostWorkRejected), "kind={kind}, cut={accepted}");
            assert!(
                allocated <= meter.backing.get(),
                "kind={kind}, cut={accepted}, allocated={allocated}, paid={}",
                meter.backing.get()
            );
            let after = store.snapshot();
            assert_eq!(after.data_flat(), before.data_flat());
            assert_eq!(after.continuations_flat(), before.continuations_flat());
            assert_eq!(after.joins_flat(), before.joins_flat());
        }
    }
}

fn owned_continuations(
    store: &Store,
    rows: &Rows,
    meter: &Meter,
) -> Vec<WaitingContinuation<String, String>> {
    store
        .get_continuations_with_reader(&[], &|| Ok(rows.continuations.clone()), meter)
        .expect("owned continuation read")
}

fn continuation_views(
    store: &Store,
    rows: &Rows,
    meter: &Meter,
) -> Result<Vec<Arc<WaitingContinuation<String, String>>>, RSpaceError> {
    store.get_continuation_views_with_reader(&[], &|| Ok(rows.continuations.clone()), meter)
}

/// C1 (DR-81; `NativeSharedReads.shared_selection_equals_deep_selection`):
/// shared views hold the same continuations, in the same order, as owned
/// reads, on a cold and on a warm cache.
#[test]
fn shared_reads_select_like_deep_reads() {
    let rows = Rows::new();
    for view_first in [true, false] {
        let store = installed();
        let unlimited = Meter::new(usize::MAX);
        let (views, owned) = if view_first {
            let views = continuation_views(&store, &rows, &unlimited).expect("cold view read");
            (views, owned_continuations(&store, &rows, &unlimited))
        } else {
            let owned = owned_continuations(&store, &rows, &unlimited);
            (continuation_views(&store, &rows, &unlimited).expect("warm view read"), owned)
        };
        let viewed: Vec<_> = views.iter().map(|view| view.as_ref().clone()).collect();
        assert_eq!(viewed, owned, "view first: {view_first}");
        assert_eq!(owned[0].continuation, "installed");
        assert_eq!(&owned[1..], rows.continuations);
    }
}

/// C1 (DR-81): a warm view read allocates no continuation payload. Its
/// allocation does not change when the payload grows 16 times, while a warm
/// owned read copies every payload.
#[test]
fn shared_read_allocates_no_payload() {
    let measure = |scale: usize| {
        let continuations = vec![
            WaitingContinuation {
                patterns: vec!["pattern".repeat(100 * scale); 3],
                continuation: "continuation".repeat(500 * scale),
                persist: true,
                peeks: [0, 2].into_iter().collect(),
                source: Consume::default(),
            };
            2
        ];
        let store = store();
        let unlimited = Meter::new(usize::MAX);
        let read = || Ok(continuations.clone());
        store
            .get_continuation_views_with_reader(&[], &read, &unlimited)
            .expect("cache fill");
        let (views, view_bytes) = measure_allocations(|| {
            store.get_continuation_views_with_reader(&[], &read, &unlimited)
        });
        assert_eq!(views.expect("warm view read").len(), continuations.len());
        let (owned, owned_bytes) =
            measure_allocations(|| store.get_continuations_with_reader(&[], &read, &unlimited));
        assert_eq!(owned.expect("warm owned read"), continuations);
        (view_bytes, owned_bytes)
    };
    let (small_view, small_owned) = measure(1);
    let (large_view, large_owned) = measure(16);
    assert_eq!(small_view, large_view, "view bytes depend on the payload");
    assert!(large_owned > 8 * small_owned, "{large_owned} vs {small_owned} owned bytes");
    assert!(small_owned > small_view, "{small_owned} owned vs {small_view} view bytes");
}

/// C1 (DR-81; `NativeSharedReads.prefetch_then_read_equals_read`): a view
/// read used as a prefetch fills the cache and leaves every later read equal
/// to a read without the prefetch.
#[test]
fn prefetch_leaves_state_and_selection_unchanged() {
    let rows = Rows::new();
    let unlimited = Meter::new(usize::MAX);
    let plain = installed();
    let expected = owned_continuations(&plain, &rows, &unlimited);
    let prefetched = installed();
    continuation_views(&prefetched, &rows, &unlimited).expect("prefetch");
    let reads = Cell::new(0);
    let after = prefetched
        .get_continuations_with_reader(
            &[],
            &|| {
                reads.set(reads.get() + 1);
                Ok(rows.continuations.clone())
            },
            &unlimited,
        )
        .expect("read after prefetch");
    assert_eq!(after, expected);
    assert_eq!(reads.get(), 0, "the prefetch filled the cache");
    let views = continuation_views(&prefetched, &rows, &unlimited).expect("view after prefetch");
    let viewed: Vec<_> = views.iter().map(|view| view.as_ref().clone()).collect();
    assert_eq!(viewed, expected);
}

/// C1 (DR-81; `NativeSharedReads.every_release_was_prepaid`): every
/// reservation cut of a cold view read rejects and leaves the cache empty,
/// so a later read decodes the history again.
#[test]
fn every_view_read_cut_rejects_without_filling_the_cache() {
    let rows = Rows::new();
    let full = Meter::new(usize::MAX);
    continuation_views(&installed(), &rows, &full).expect("full read");
    for cut in 0..full.calls.get() {
        let store = installed();
        let limited = Meter::new(cut);
        assert_eq!(
            continuation_views(&store, &rows, &limited).map(|views| views.len()),
            Err(RSpaceError::HostWorkRejected),
            "cut {cut}"
        );
        let reads = Cell::new(0);
        store
            .get_continuations_with_reader(
                &[],
                &|| {
                    reads.set(reads.get() + 1);
                    Ok(rows.continuations.clone())
                },
                &Meter::new(usize::MAX),
            )
            .expect("read after cut");
        assert_eq!(reads.get(), 1, "cut {cut} left the cache empty");
    }
}

fn owned_data(store: &Store, data: &[Datum<String>], meter: &Meter) -> Vec<Datum<String>> {
    store
        .get_data_with_reader(&String::new(), &|| Ok(data.to_vec()), meter)
        .expect("owned data read")
}

fn data_view(
    store: &Store,
    data: &[Datum<String>],
    meter: &Meter,
) -> Result<NativeDataView<String, String>, RSpaceError> {
    store.get_data_view_with_reader(&String::new(), &|| Ok(data.to_vec()), meter)
}

/// C2 (DR-82; `NativeSharedReads.shared_selection_equals_deep_selection`): a
/// data view holds the same datums, in the same order, as an owned read, on a
/// cold and on a warm cache.
#[test]
fn data_view_selection_matches_owned() {
    let rows = Rows::new();
    for view_first in [true, false] {
        let store = store();
        let unlimited = Meter::new(usize::MAX);
        let (view, owned) = if view_first {
            let view = data_view(&store, &rows.data, &unlimited).expect("cold data view");
            (view, owned_data(&store, &rows.data, &unlimited))
        } else {
            let owned = owned_data(&store, &rows.data, &unlimited);
            (data_view(&store, &rows.data, &unlimited).expect("warm data view"), owned)
        };
        assert_eq!(view.values(), owned.as_slice(), "view first: {view_first}");
        assert_eq!(owned, rows.data);
    }
}

/// C2 (DR-82): a warm data view allocates no datum payload. Its allocation
/// does not change when the payload grows 16 times, while a warm owned read
/// copies every datum.
#[test]
fn data_view_allocates_no_payload() {
    let measure = |scale: usize| {
        let data = vec![
            Datum {
                a: "datum".repeat(500 * scale),
                persist: false,
                source: Produce::default(),
            };
            2
        ];
        let store = store();
        let unlimited = Meter::new(usize::MAX);
        data_view(&store, &data, &unlimited).expect("cache fill");
        let (view, view_bytes) = measure_allocations(|| data_view(&store, &data, &unlimited));
        assert_eq!(view.expect("warm data view").values(), data.as_slice());
        let (owned, owned_bytes) = measure_allocations(|| owned_data(&store, &data, &unlimited));
        assert_eq!(owned, data);
        (view_bytes, owned_bytes)
    };
    let (small_view, small_owned) = measure(1);
    let (large_view, large_owned) = measure(16);
    assert_eq!(small_view, large_view, "view bytes depend on the payload");
    assert!(large_owned > 8 * small_owned, "{large_owned} vs {small_owned} owned bytes");
    assert!(small_owned > small_view, "{small_owned} owned vs {small_view} view bytes");
}

/// C2 (DR-82): a data view is a snapshot. A later write to the channel does
/// not change the view, and a new read sees the write.
#[test]
fn data_view_is_a_stable_snapshot() {
    let rows = Rows::new();
    let store = store();
    let unlimited = Meter::new(usize::MAX);
    let view = data_view(&store, &rows.data, &unlimited).expect("data view");
    let added = Datum {
        a: "later".to_owned(),
        persist: false,
        source: Produce::default(),
    };
    store.put_datum(&String::new(), added.clone());
    assert_eq!(view.values(), rows.data.as_slice());
    let fresh = data_view(&store, &rows.data, &unlimited).expect("fresh data view");
    assert_eq!(fresh.values().len(), rows.data.len() + 1);
    assert!(fresh.values().contains(&added));
}

/// C2 (DR-82; `NativeSharedReads.every_release_was_prepaid`): every
/// reservation cut of a cold data view rejects and leaves the cache empty, so
/// a later read decodes the history again.
#[test]
fn every_data_view_cut_rejects_without_filling_the_cache() {
    let rows = Rows::new();
    let full = Meter::new(usize::MAX);
    data_view(&store(), &rows.data, &full).expect("full read");
    for cut in 0..full.calls.get() {
        let store = store();
        let limited = Meter::new(cut);
        assert_eq!(
            data_view(&store, &rows.data, &limited).map(|view| view.values().len()),
            Err(RSpaceError::HostWorkRejected),
            "cut {cut}"
        );
        let reads = Cell::new(0);
        store
            .get_data_with_reader(
                &String::new(),
                &|| {
                    reads.set(reads.get() + 1);
                    Ok(rows.data.clone())
                },
                &Meter::new(usize::MAX),
            )
            .expect("read after cut");
        assert_eq!(reads.get(), 1, "cut {cut} left the cache empty");
    }
}
