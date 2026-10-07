use std::sync::Mutex;

use proptest::prelude::*;

use super::super::tests::Logical;
use super::*;
use crate::rspace::candidate_order::{
    candidate_strategy, canonical_candidates, datum_with_source, encoded_order,
};
use crate::rspace::history::native_reader::measure_allocations;
use crate::rspace::rspace::RSpace;
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

#[derive(Default)]
struct Meter {
    calls: MeterCell<usize>,
    used: MeterCell<[usize; 3]>,
    reject: Option<usize>,
    limit: Option<[usize; 3]>,
}

#[derive(Default)]
struct MeterCell<T>(Mutex<T>);

impl<T: Copy> MeterCell<T> {
    fn get(&self) -> T { *self.0.lock().unwrap() }

    fn set(&self, value: T) { *self.0.lock().unwrap() = value }
}

impl SourceMeter for Meter {
    fn reserve(&self, operations: usize, scanned: usize, backing: usize) -> Result<()> {
        let call = self.calls.get();
        self.calls.set(call + 1);
        if self.reject == Some(call) {
            return Err(RSpaceError::HostWorkRejected);
        }
        let mut used = self.used.get();
        for (value, add) in used.iter_mut().zip([operations, scanned, backing]) {
            *value = value
                .checked_add(add)
                .ok_or(RSpaceError::HostWorkRejected)?;
        }
        if self
            .limit
            .is_some_and(|limit| used.iter().zip(limit).any(|(value, limit)| *value > limit))
        {
            return Err(RSpaceError::HostWorkRejected);
        }
        self.used.set(used);
        Ok(())
    }
}

struct Matcher;

impl Match<u8, u8, u8> for Matcher {
    fn get(&self, pattern: &u8, value: &u8) -> Option<u8> {
        (*pattern == 0 || pattern == value).then_some(*value)
    }
    fn get_metered(
        &self,
        pattern: &u8,
        value: &u8,
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<Option<u8>> {
        meter.reserve(1, 2, 0)?;
        Ok(self.get(pattern, value))
    }
    fn check_commit(&self, continuation: &u8, _: &[u8]) -> bool { *continuation != 255 }

    fn check_commit_metered(
        &self,
        continuation: &u8,
        matched: &[&u8],
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<bool> {
        meter.reserve(1, 1, 0)?;
        let owned: Vec<u8> = matched.iter().map(|value| **value).collect();
        Ok(self.check_commit(continuation, &owned))
    }
}

type Space = ReplayRSpace<u8, u8, u8, u8>;

async fn space() -> Space {
    let mut stores = InMemoryStoreManager::new();
    RSpace::create_with_replay(stores.r_space_stores().await.unwrap(), Arc::new(Box::new(Matcher)))
        .unwrap()
        .1
}

fn with_reader<T>(
    space: &Space,
    meter: &Meter,
    action: impl FnOnce(&CandidateReader<'_, u8, u8, u8, u8>) -> T,
) -> T {
    // Changed by D-C2c (D-S1, DR-96): the readers receive the keys; these
    // fixtures read the space's legacy store.
    let data = |channel: &u8, _key: StoreKey| {
        space
            .get_store()
            .get_data_view_with_reader(channel, &|| Ok(Vec::new()), meter)
    };
    let continuations = |channels: &[u8], _keys: &GroupKeys| {
        space
            .get_store()
            .get_continuation_views_with_reader(channels, &|| Ok(Vec::new()), meter)
    };
    action(&CandidateReader {
        meter,
        data: &data,
        continuations: &continuations,
    })
}

/// D-C2c (DR-96): channel keys on an unlimited meter, computed before the
/// metered work under test.
fn channel_keys(channels: &[u8]) -> Vec<StoreKey> {
    let free = |_: usize, _: usize, _: usize| Ok::<(), RSpaceError>(());
    channels
        .iter()
        .map(|channel| native_source::channel_key(channel, &free).expect("an unlimited meter"))
        .collect()
}

/// D-C2c (DR-96): the keys of the join groups on an unlimited meter.
fn operation_keys(joins: &[Vec<u8>]) -> OperationKeys {
    let free = |_: usize, _: usize, _: usize| Ok::<(), RSpaceError>(());
    OperationKeys::build(joins, &free).expect("an unlimited meter")
}

fn put(space: &Space, channel: u8, value: u8, persistent: bool) {
    let datum = Datum::create(&channel, value, persistent);
    space.increment_produce_counter(&datum.source, persistent);
    space.get_store().put_datum(&channel, datum);
}

fn waiting(space: &Space, channels: &[u8], patterns: Vec<u8>, body: u8) -> Consume {
    let source = Consume::create(&channels.to_vec(), &patterns, &body, false);
    space
        .get_store()
        .put_continuation(channels, WaitingContinuation {
            patterns,
            continuation: body,
            persist: false,
            peeks: BTreeSet::new(),
            source: source.clone(),
        });
    source
}

fn state(space: &Space) -> (Vec<u8>, String, String, String, i64) {
    let mut rows: Vec<_> = space
        .get_store()
        .to_map()
        .into_iter()
        .map(|(key, row)| (key, format!("{row:?}")))
        .collect();
    rows.sort();
    (
        bincode::serialize(&*space.produce_counter.lock().unwrap()).unwrap(),
        format!("{rows:?}"),
        format!("{:?}", space.event_log.lock().unwrap()),
        format!("{:?}", space.replay_data.lock().unwrap()),
        space
            .replay_waiting_continuations_estimate
            .load(std::sync::atomic::Ordering::Relaxed),
    )
}

fn same_data(left: &[ConsumeCandidate<u8, u8>], right: &[ConsumeCandidate<u8, u8>]) {
    assert_eq!(left.len(), right.len());
    for (left, right) in left.iter().zip(right) {
        assert_eq!(left.channel, right.channel);
        assert_eq!(
            bincode::serialize(&left.datum).unwrap(),
            bincode::serialize(&right.datum).unwrap()
        );
        assert_eq!(left.removed_datum, right.removed_datum);
        assert_eq!(left.datum_index, right.datum_index);
    }
}

#[tokio::test]
async fn counter_publication_prepays_growth_and_preserves_state_at_every_cut() {
    let space = space().await;
    let source = Produce::create(&1u8, &7u8, false);
    let full = Meter::default();
    space
        .prepare_metered_produce_counter(&source, false, &full)
        .unwrap()
        .publish();
    let calls = full.calls.get();
    let required = full.used.get();
    assert!(calls > 1);
    assert!(required.iter().all(|value| *value > 0));
    space.produce_counter.lock().unwrap().clear();
    let prepared = space
        .prepare_metered_produce_counter(&source, false, &Meter::default())
        .unwrap();
    drop(prepared);
    assert!(space.produce_counter.lock().unwrap().is_empty());
    space
        .prepare_metered_produce_counter(&source, false, &Meter::default())
        .unwrap()
        .publish();
    assert_eq!(space.produce_counter.lock().unwrap().get(&source), Some(&1));
    space.produce_counter.lock().unwrap().clear();
    for cut in 0..calls {
        let meter = Meter {
            reject: Some(cut),
            ..Meter::default()
        };
        assert!(matches!(
            space.prepare_metered_produce_counter(&source, false, &meter),
            Err(RSpaceError::HostWorkRejected)
        ));
        assert!(space.produce_counter.lock().unwrap().is_empty());
    }
    for dimension in 0..3 {
        let mut limit = required;
        limit[dimension] -= 1;
        let meter = Meter {
            limit: Some(limit),
            ..Meter::default()
        };
        assert!(matches!(
            space.prepare_metered_produce_counter(&source, false, &meter),
            Err(RSpaceError::HostWorkRejected)
        ));
        assert!(space.produce_counter.lock().unwrap().is_empty());
    }
    let persistent = Meter::default();
    space
        .prepare_metered_produce_counter(&source, true, &persistent)
        .unwrap()
        .publish();
    assert_eq!(persistent.calls.get(), 0);
    assert!(space.produce_counter.lock().unwrap().is_empty());
    space
        .produce_counter
        .lock()
        .unwrap()
        .insert(source.clone(), i32::MAX);
    let overflow = Meter::default();
    assert!(matches!(
        space.prepare_metered_produce_counter(&source, false, &overflow),
        Err(RSpaceError::HostWorkRejected)
    ));
    assert_eq!(space.produce_counter.lock().unwrap().get(&source), Some(&i32::MAX));
}

#[tokio::test]
async fn counter_preparation_prepays_nested_source_cleanup() {
    let space = space().await;
    let small = Produce::create(&1u8, &7u8, false);
    let mut large = small.clone();
    large.output_value = vec![vec![7; 64]; 512];
    let measure = |source: &Produce| {
        let meter = Meter::default();
        space
            .prepare_metered_produce_counter(source, false, &meter)
            .unwrap();
        meter.used.get()[0]
    };
    let copy_work = |source: &Produce| {
        let meter = Meter::default();
        native_backing::reserve(source, &meter).unwrap();
        meter.used.get()[0]
    };
    let added_work = measure(&large) - measure(&small);
    let added_copy_work = copy_work(&large) - copy_work(&small);
    assert!(added_copy_work > 0);
    assert!(added_work >= added_copy_work * 2);
    assert!(space.produce_counter.lock().unwrap().is_empty());
}

/// C3 (DR-78; `OrderedLookupBound.search_within_size_bound`): a
/// produce-counter lookup charges `tree_search_bound(len)` comparisons, and
/// each comparison reads two hashes. The charge grows with the height of the
/// map, not with its size. Preparation charges two lookups (the read and the
/// insert of `publish`), the source copy and cleanup, and the tree growth.
#[tokio::test]
async fn counter_charge_is_logarithmic() {
    let space = space().await;
    let probe = Produce::create(&0u8, &0u8, false);
    let hash_bytes = probe.hash.0.len();
    let lookup = |entries: usize| {
        let bound = tree_search_bound(entries);
        [bound, bound * 2 * hash_bytes, 0]
    };
    let copy = Meter::default();
    native_backing::reserve_copy_and_cleanup(&probe, &copy).expect("copy charge");
    let copy = copy.used.get();
    for size in [0_usize, 1, 10, 11, 70, 71, 430, 431, 2_000] {
        {
            let mut counters = space.produce_counter.lock().expect("produce counter lock");
            counters.clear();
            for index in 0..size {
                let channel = u8::try_from(index / 256 + 1).expect("channel fits in u8");
                let value = u8::try_from(index % 256).expect("value fits in u8");
                counters.insert(Produce::create(&channel, &value, false), 1);
            }
            assert_eq!(counters.len(), size);
        }
        let count = Meter::default();
        assert_eq!(space.metered_produce_count(&probe, &count).expect("count"), 0);
        assert_eq!(count.used.get(), lookup(size), "count charge at {size} entries");

        let prepare = Meter::default();
        drop(
            space
                .prepare_metered_produce_counter(&probe, false, &prepare)
                .expect("prepare"),
        );
        let (growth_operations, growth_bytes) =
            tree_growth::<Produce, i32>(size, 1).expect("tree growth");
        assert_eq!(
            prepare.used.get(),
            [
                2 * lookup(size)[0] + copy[0] + growth_operations,
                2 * lookup(size)[1] + copy[1] + growth_bytes,
                copy[2] + growth_bytes,
            ],
            "preparation charge at {size} entries"
        );
        assert_eq!(
            space
                .produce_counter
                .lock()
                .expect("produce counter lock")
                .len(),
            size
        );
    }
    // The legacy count charge at 2,000 entries was 2,001 * 32 + 2,000
    // operations and 4,001 hash reads.
    assert_eq!(lookup(2_000), [44, 44 * 2 * hash_bytes, 0]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    // Disabled by I1 (DR-75): the metered order no longer equals the legacy
    // (digest, index) order, and `Vec<u8>` has no source hash. Replaced by
    // `paid_order_matches_unmetered_canonical_order`.
    // #[test]
    // fn paid_order_matches_legacy_hash_and_original_index(
    //     values in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..256), 0..64),
    // ) {
    //     let expected = deterministic_candidates(values.clone());
    //     let meter = Meter::default();
    //     let (actual, bytes) = measure_allocations(|| sorted(values, &meter));
    //     prop_assert_eq!(actual.unwrap(), expected);
    //     prop_assert!(bytes <= meter.used.get()[2], "requested {}, reserved {}", bytes, meter.used.get()[2]);
    // }

    /// Metered native replay and play produce the same canonical order
    /// (`CandidateSourceOrder.lazy_two_phase_is_canonical`), and every metered
    /// allocation was reserved first.
    #[test]
    fn paid_order_matches_unmetered_canonical_order(values in candidate_strategy()) {
        let expected = canonical_candidates(values.clone());
        let meter = Meter::default();
        let (actual, bytes) = measure_allocations(|| sorted(values, &meter));
        prop_assert_eq!(encoded_order(&actual.unwrap()), encoded_order(&expected));
        prop_assert!(bytes <= meter.used.get()[2], "requested {}, reserved {}", bytes, meter.used.get()[2]);
    }

    #[test]
    fn consume_matches_legacy_for_repeated_channels_and_partial_misses(
        values in prop::collection::vec(1u8..64, 0..8),
        patterns in prop::collection::vec(0u8..70, 0..8),
        persistent in any::<bool>(),
    ) {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let space = space().await;
            for value in values { put(&space, 1, value, persistent); }
            let channels = vec![1; patterns.len()];
            let source = Consume::create(&channels, &patterns, &9u8, false);
            let expected = space.prepare_native_consume_candidate(&channels, &patterns, &9, &source, &BTreeSet::new(), None);
            let meter = Meter::default();
            let keys = channel_keys(&channels);
            let actual = with_reader(&space, &meter, |reader| space.prepare_metered_consume_candidate(&channels, &keys, &patterns, &9, &source, &BTreeSet::new(), None, reader)).unwrap();
            assert_eq!(actual.is_some(), expected.is_some());
            if let (Some(actual), Some(expected)) = (actual, expected) {
                same_data(&actual.data, &expected.data);
                assert_eq!(bincode::serialize(&actual.comm).unwrap(), bincode::serialize(&expected.comm).unwrap());
            }
        });
    }

    #[test]
    fn produce_preserves_repeated_channels_persistence_peeks_and_prestate_selection(
        values in prop::collection::vec(1u8..64, 1..8),
        persistent in any::<bool>(),
        peek in any::<bool>(),
        incoming in 1u8..64,
    ) {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let space = space().await;
            for value in &values { put(&space, 1, *value, persistent); }
            let channels = vec![1; values.len()];
            let patterns = vec![0; values.len()];
            let peeks = if peek { BTreeSet::from([0]) } else { BTreeSet::new() };
            let consume = Consume::create(&channels, &patterns, &9u8, false);
            space.get_store().put_continuation(&channels, WaitingContinuation { patterns, continuation: 9, persist: false, peeks, source: consume });
            let source = Produce::create(&1u8, &incoming, persistent);
            let expected = space.prepare_native_produce_candidate(&1, &incoming, persistent, &source, vec![channels.clone()], None).unwrap().unwrap();
            let before = state(&space);
            let meter = Meter::default();
            let keys = operation_keys(&[channels.clone()]);
            let actual = with_reader(&space, &meter, |reader| space.prepare_metered_produce_candidate(&1, &incoming, persistent, &source, vec![channels.clone()], &keys, None, reader)).unwrap().unwrap().0;
            same_data(&actual.candidate.data_candidates, &expected.candidate.data_candidates);
            assert_eq!(bincode::serialize(&actual.comm).unwrap(), bincode::serialize(&expected.comm).unwrap());
            let identity = Logical(expected.comm);
            let selected = with_reader(&space, &meter, |reader| space.prepare_metered_produce_candidate(&1, &incoming, persistent, &source, vec![channels], &keys, Some(&identity), reader)).unwrap().unwrap().0;
            same_data(&selected.candidate.data_candidates, &actual.candidate.data_candidates);
            assert_eq!(state(&space), before);
        });
    }
}

#[test]
fn every_order_reservation_cut_rejects_without_unpaid_allocation() {
    // I1 (DR-75): the metered order needs source hashes; the fixture keeps a
    // tie run (source 1) so the cut also covers digest reservations.
    // let values = vec![vec![7u8; 2048], vec![3; 512], vec![7; 2048], Vec::new()];
    let values = vec![
        datum_with_source(vec![7u8; 2048], 1, false),
        datum_with_source(vec![3u8; 512], 2, false),
        datum_with_source(vec![7u8; 2048], 1, true),
        datum_with_source(Vec::new(), 0, false),
    ];
    let baseline = Meter::default();
    sorted(values.clone(), &baseline).unwrap();
    for reject in 0..baseline.calls.get() {
        let input = values.clone();
        let meter = Meter {
            reject: Some(reject),
            ..Default::default()
        };
        let (result, actual) = measure_allocations(|| sorted(input, &meter));
        assert!(matches!(result, Err(RSpaceError::HostWorkRejected)), "cut {reject}");
        assert_eq!(meter.calls.get(), reject + 1);
        assert!(
            actual <= meter.used.get()[2],
            "cut {reject}: requested {actual}, reserved {}",
            meter.used.get()[2]
        );
    }
}

#[test]
fn ordering_accepts_exact_credit_and_rejects_each_smaller_dimension() {
    // I1 (DR-75): the metered order needs source hashes; the fixture keeps a
    // tie run (source 1) so the cut also covers digest reservations.
    // let values = vec![vec![7u8; 2048], vec![3; 512], vec![7; 2048], Vec::new()];
    let values = vec![
        datum_with_source(vec![7u8; 2048], 1, false),
        datum_with_source(vec![3u8; 512], 2, false),
        datum_with_source(vec![7u8; 2048], 1, true),
        datum_with_source(Vec::new(), 0, false),
    ];
    let baseline = Meter::default();
    let expected = sorted(values.clone(), &baseline).unwrap();
    let exact = Meter {
        limit: Some(baseline.used.get()),
        ..Default::default()
    };
    assert_eq!(sorted(values.clone(), &exact).unwrap(), expected);
    for dimension in 0..3 {
        let mut limit = baseline.used.get();
        limit[dimension] -= 1;
        let meter = Meter {
            limit: Some(limit),
            ..Default::default()
        };
        let input = values.clone();
        let (result, actual) = measure_allocations(|| sorted(input, &meter));
        assert!(matches!(result, Err(RSpaceError::HostWorkRejected)));
        assert!(actual <= meter.used.get()[2]);
        assert!(meter.used.get()[dimension] <= limit[dimension]);
    }
}

// Disabled by I1 (DR-75): the legacy order inspected every payload, and
// `Arc<String>` has no source hash. Replaced by
// `shared_order_inspects_only_tie_members_and_preserves_shared_ownership`.
// #[test]
// fn shared_payload_order_inspects_the_payload_and_preserves_shared_ownership()
// {     let values: Vec<Arc<String>> =
//         vec![Arc::new("large".repeat(4096)), Arc::new("other".repeat(1024))];
//     let expected = deterministic_candidates(values.clone());
//     let meter = Meter::default();
//     let input = values.clone();
//     let (actual, bytes) = measure_allocations(|| sorted(input, &meter));
//     let actual = actual.unwrap();
//     assert_eq!(actual, expected);
//     assert!(meter.used.get()[1] > values.iter().map(|value|
// value.len()).sum());     assert!(bytes <= meter.used.get()[2]);
//     for (value, index) in actual {
//         assert!(Arc::ptr_eq(&value, &values[index as usize]));
//     }
// }

/// `CandidateSourceOrder.digests_only_for_ties`: candidates with distinct
/// source hashes are never inspected or digested; tie-run members are.
#[test]
fn shared_order_inspects_only_tie_members_and_preserves_shared_ownership() {
    let distinct = "large".repeat(1 << 18);
    let tie = "tie".repeat(1024);
    let values: Vec<Arc<Datum<String>>> = vec![
        Arc::new(datum_with_source(distinct.clone(), 9, false)),
        Arc::new(datum_with_source(tie.clone(), 4, false)),
        Arc::new(datum_with_source(distinct.clone(), 8, false)),
        Arc::new(datum_with_source(tie.clone(), 4, true)),
    ];
    let expected = canonical_candidates(values.clone());
    let meter = Meter::default();
    let input = values.clone();
    let (actual, bytes) = measure_allocations(|| sorted(input, &meter));
    let actual = actual.unwrap();
    assert_eq!(encoded_order(&actual), encoded_order(&expected));
    let scanned = meter.used.get()[1];
    assert!(scanned >= 2 * tie.len(), "tie members are digested: scanned {scanned}");
    assert!(scanned < distinct.len(), "distinct sources are not inspected: scanned {scanned}");
    assert!(bytes <= meter.used.get()[2]);
    for (value, index) in actual {
        assert!(Arc::ptr_eq(&value, &values[index as usize]));
    }
}

#[tokio::test]
async fn every_consume_cut_preserves_warm_state_and_prepays_coordinator_allocations() {
    let space = space().await;
    for value in [7, 8, 9] {
        put(&space, 1, value, false);
    }
    let channels = [1, 1, 1];
    let patterns = [0, 0, 0];
    let source = Consume::create(&channels.to_vec(), &patterns.to_vec(), &9u8, false);
    space.get_store().get_continuations(&channels);
    let keys = channel_keys(&channels);
    let baseline = Meter::default();
    with_reader(&space, &baseline, |reader| {
        space.prepare_metered_consume_candidate(
            &channels,
            &keys,
            &patterns,
            &9,
            &source,
            &BTreeSet::from([0]),
            None,
            reader,
        )
    })
    .unwrap()
    .unwrap();
    let before = state(&space);
    let peeks = BTreeSet::from([0]);
    for reject in 0..baseline.calls.get() {
        let meter = Meter {
            reject: Some(reject),
            ..Default::default()
        };
        let (result, actual) = measure_allocations(|| {
            with_reader(&space, &meter, |reader| {
                space.prepare_metered_consume_candidate(
                    &channels, &keys, &patterns, &9, &source, &peeks, None, reader,
                )
            })
        });
        assert!(matches!(result, Err(RSpaceError::HostWorkRejected)), "cut {reject}");
        assert!(
            actual <= meter.used.get()[2],
            "cut {reject}: requested {actual}, reserved {}",
            meter.used.get()[2]
        );
        assert_eq!(state(&space), before);
    }
}

#[tokio::test]
async fn produce_rolls_back_each_failed_probe_and_preserves_legacy_selection() {
    let space = space().await;
    put(&space, 1, 7, false);
    put(&space, 2, 8, false);
    waiting(&space, &[1, 1], vec![0, 99], 9);
    waiting(&space, &[1, 2], vec![0, 0], 255);
    waiting(&space, &[1, 2], vec![0, 0], 9);
    let joins = vec![vec![1, 1], vec![1, 2]];
    let source = Produce::create(&1u8, &6u8, false);
    let expected = space
        .prepare_native_produce_candidate(&1, &6, false, &source, joins.clone(), None)
        .unwrap()
        .unwrap();
    let keys = operation_keys(&joins);
    let baseline = Meter::default();
    let actual = with_reader(&space, &baseline, |reader| {
        space.prepare_metered_produce_candidate(
            &1,
            &6,
            false,
            &source,
            joins.clone(),
            &keys,
            None,
            reader,
        )
    })
    .unwrap()
    .unwrap()
    .0;
    same_data(&actual.candidate.data_candidates, &expected.candidate.data_candidates);
    assert_eq!(actual.candidate.continuation_index, expected.candidate.continuation_index);
    assert_eq!(
        bincode::serialize(&actual.candidate.continuation).unwrap(),
        bincode::serialize(&expected.candidate.continuation).unwrap()
    );
    assert_eq!(
        bincode::serialize(&actual.comm).unwrap(),
        bincode::serialize(&expected.comm).unwrap()
    );
    let before = state(&space);
    for reject in 0..baseline.calls.get() {
        let input = joins.clone();
        let meter = Meter {
            reject: Some(reject),
            ..Default::default()
        };
        let (result, actual) = measure_allocations(|| {
            with_reader(&space, &meter, |reader| {
                space.prepare_metered_produce_candidate(
                    &1, &6, false, &source, input, &keys, None, reader,
                )
            })
        });
        assert!(matches!(result, Err(RSpaceError::HostWorkRejected)), "cut {reject}");
        assert!(
            actual <= meter.used.get()[2],
            "cut {reject}: requested {actual}, reserved {}",
            meter.used.get()[2]
        );
        assert_eq!(state(&space), before);
    }
}

#[tokio::test]
async fn comm_ties_preserve_metadata_order_and_counter_key_replacement() {
    let space = space().await;
    let source = Produce::create(&1u8, &7u8, false);
    let mut first = source.clone();
    first.output_value = vec![vec![1; 128]];
    let mut second = source;
    second.output_value = vec![vec![2; 256]];
    let data: Vec<_> = [first, second]
        .into_iter()
        .enumerate()
        .map(|(index, source)| ConsumeCandidate {
            channel: 1,
            datum: Datum {
                a: 7,
                persist: false,
                source,
            },
            removed_datum: 7,
            datum_index: index as i32,
        })
        .collect();
    let consume = Consume::create(&vec![1u8, 1], &vec![0u8, 0], &9u8, false);
    let expected = COMM::new(&data, consume.clone(), BTreeSet::new(), |produces| {
        space.produce_counters(produces)
    });
    let meter = Meter::default();
    let (actual, bytes) =
        measure_allocations(|| space.metered_comm(&data, &consume, &BTreeSet::new(), None, &meter));
    assert_eq!(
        bincode::serialize(&actual.unwrap()).unwrap(),
        bincode::serialize(&expected).unwrap()
    );
    assert!(bytes <= meter.used.get()[2]);
}

#[tokio::test]
async fn exact_identity_rejection_and_counter_overflow_preserve_state() {
    let space = space().await;
    put(&space, 1, 7, false);
    let source = waiting(&space, &[1], vec![0], 9);
    let candidate = space
        .prepare_native_consume_candidate(&[1], &[0], &9, &source, &BTreeSet::new(), None)
        .unwrap();
    let before = state(&space);
    for field in 0..8 {
        let mut changed = candidate.comm.clone();
        match field {
            0 => changed.consume.persistent = true,
            1 => changed.consume.channel_hashes.clear(),
            2 => changed.produces[0].persistent = true,
            3 => changed.produces[0].channel_hash = Produce::create(&2u8, &7u8, false).channel_hash,
            4 => {
                changed.peeks.insert(0);
            }
            5 => changed
                .times_repeated
                .values_mut()
                .for_each(|count| *count += 1),
            6 => changed.produces.push(changed.produces[0].clone()),
            _ => changed.times_repeated.clear(),
        }
        let meter = Meter::default();
        let keys = channel_keys(&[1]);
        assert!(
            with_reader(&space, &meter, |reader| space.prepare_metered_consume_candidate(
                &[1],
                &keys,
                &[0],
                &9,
                &source,
                &BTreeSet::new(),
                Some(&Logical(changed)),
                reader
            ))
            .unwrap()
            .is_none()
        );
        assert_eq!(state(&space), before);
    }
    let trigger = Produce::create(&1u8, &8u8, false);
    space
        .produce_counter
        .lock()
        .unwrap()
        .insert(trigger.clone(), i32::MAX);
    let before = state(&space);
    let meter = Meter::default();
    let input = vec![vec![1]];
    let keys = operation_keys(&input);
    let (result, bytes) = measure_allocations(|| {
        with_reader(&space, &meter, |reader| {
            space.prepare_metered_produce_candidate(
                &1, &8, false, &trigger, input, &keys, None, reader,
            )
        })
    });
    assert!(matches!(result, Err(RSpaceError::InterpreterError(_))));
    assert!(bytes <= meter.used.get()[2]);
    assert_eq!(state(&space), before);
}

/// D-M1 (DR-88) fixtures: a string space whose matcher reads only the first
/// byte of the pattern and the datum (and meters exactly that read), and
/// whose commit check reads nothing of the continuation.
struct PrefixMatcher;

impl Match<String, String, String> for PrefixMatcher {
    fn get(&self, pattern: &String, value: &String) -> Option<String> {
        (pattern.as_bytes().first() == value.as_bytes().first()).then(|| value.clone())
    }

    fn get_metered(
        &self,
        pattern: &String,
        value: &String,
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<Option<String>> {
        meter.reserve(1, 2, 0)?;
        if pattern.as_bytes().first() != value.as_bytes().first() {
            return Ok(None);
        }
        meter.reserve(1, value.len(), value.len())?;
        Ok(Some(value.clone()))
    }

    fn check_commit_metered(
        &self,
        _: &String,
        _: &[&String],
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<bool> {
        meter.reserve(1, 0, 0)?;
        Ok(true)
    }
}

type StringSpace = ReplayRSpace<String, String, String, String>;

async fn string_space() -> StringSpace {
    let mut stores = InMemoryStoreManager::new();
    RSpace::create_with_replay(
        stores.r_space_stores().await.expect("string space stores"),
        Arc::new(Box::new(PrefixMatcher)),
    )
    .expect("string replay space")
    .1
}

/// The charge of one `metered_match_data` call over a single stored datum.
fn match_data_charge(
    space: &StringSpace,
    pattern: &str,
    datum: &Datum<String>,
    continuation: &str,
) -> (bool, [usize; 3]) {
    let channel = "channel".to_string();
    let data = vec![ChannelData {
        // Changed by D-D4 (DR-106): the entry borrows its channel.
        // channel: channel.clone(),
        channel: &channel,
        values: vec![(Cow::Borrowed(datum), 0)],
    }];
    let meter = Meter::default();
    let matched = space
        .metered_match_data(
            std::slice::from_ref(&channel),
            &[0],
            &[pattern.to_string()],
            &continuation.to_string(),
            &data,
            &meter,
        )
        .expect("metered match data");
    (matched.is_some(), meter.used.get())
}

/// D-M1 (DR-88): a failed match attempt charges only what the matcher reads,
/// so unread tails of the pattern and the datum do not change the charge.
#[tokio::test]
async fn match_attempt_charge_excludes_unread_pattern_and_datum() {
    let space = string_space().await;
    let channel = "channel".to_string();
    let short_datum = Datum::create(&channel, "a".to_string(), false);
    let long_datum = Datum::create(&channel, format!("a{}", "y".repeat(4_096)), false);
    let short = match_data_charge(&space, "b", &short_datum, "k");
    let long = match_data_charge(&space, &format!("b{}", "x".repeat(4_096)), &long_datum, "k");
    assert!(!short.0 && !long.0);
    assert_eq!(short.1, long.1);
}

/// D-M1 (DR-88): the commit check reads only what its matcher reads, so the
/// unread continuation body does not change the charge of a successful match.
#[tokio::test]
async fn commit_check_charge_is_independent_of_continuation_body() {
    let space = string_space().await;
    let channel = "channel".to_string();
    let datum = Datum::create(&channel, "a-value".to_string(), false);
    let short = match_data_charge(&space, "a", &datum, "k");
    let long = match_data_charge(&space, "a", &datum, &"k".repeat(4_096));
    assert!(short.0 && long.0);
    assert_eq!(short.1, long.1);
}

/// Negative control for D-M1 and D-M6 (DR-88): the legacy selection walked
/// the pattern, the datum and the whole continuation, and copied every
/// matched datum, so its charge grew with bytes that no step read.
#[tokio::test]
async fn legacy_selection_charge_grew_with_unread_values() {
    let space = string_space().await;
    let channel = "channel".to_string();
    let datum = Datum::create(&channel, "a-value".to_string(), false);
    let legacy = |pattern: &str, continuation: &str| {
        let (_, current) = match_data_charge(&space, pattern, &datum, continuation);
        let walks = Meter::default();
        native_backing::inspect(&pattern.to_string(), &walks).expect("pattern walk");
        native_backing::inspect(&datum.a, &walks).expect("datum walk");
        native_backing::reserve_copy_and_cleanup(&datum.a, &walks).expect("matched copy");
        native_backing::inspect(&continuation.to_string(), &walks).expect("continuation walk");
        let walks = walks.used.get();
        [current[0] + walks[0], current[1] + walks[1], current[2] + walks[2]]
    };
    let short = legacy("a", "k");
    let long = legacy(&format!("a{}", "x".repeat(4_096)), &"k".repeat(4_096));
    let (_, current_short) = match_data_charge(&space, "a", &datum, "k");
    let (_, current_long) =
        match_data_charge(&space, &format!("a{}", "x".repeat(4_096)), &datum, &"k".repeat(4_096));
    assert_eq!(current_short, current_long);
    assert!(long[1] > short[1] + 8_000, "{short:?} vs {long:?}");
}

/// D-D4 (DR-106) fixtures: the keys of string channels on an unlimited meter.
fn string_channel_keys(channels: &[String]) -> Vec<StoreKey> {
    let free = |_: usize, _: usize, _: usize| Ok::<(), RSpaceError>(());
    channels
        .iter()
        .map(|channel| native_source::channel_key(channel, &free).expect("an unlimited meter"))
        .collect()
}

/// D-D4 (DR-106) fixtures: `with_reader` for the string space.
fn with_string_reader<T>(
    space: &StringSpace,
    meter: &Meter,
    action: impl FnOnce(&CandidateReader<'_, String, String, String, String>) -> T,
) -> T {
    let data = |channel: &String, _key: StoreKey| {
        space
            .get_store()
            .get_data_view_with_reader(channel, &|| Ok(Vec::new()), meter)
    };
    let continuations = |channels: &[String], _keys: &GroupKeys| {
        space
            .get_store()
            .get_continuation_views_with_reader(channels, &|| Ok(Vec::new()), meter)
    };
    action(&CandidateReader {
        meter,
        data: &data,
        continuations: &continuations,
    })
}

/// D-D4 (DR-106): a frozen copy of the selection before D-D4. For each
/// pattern it searched the channel data for the first entry equal to the
/// pattern's channel, so its charge grew with the position of that entry and
/// with the sizes of the channels. It is the oracle of the selection tests
/// and of the charge controls.
fn legacy_match_data<C, P, A, K>(
    space: &ReplayRSpace<C, P, A, K>,
    channels: &[C],
    patterns: &[P],
    continuation: &K,
    data: &[ChannelData<'_, C, A>],
    meter: &(dyn SourceMeter + Send + Sync),
) -> Result<Option<Vec<ConsumeCandidate<C, A>>>>
where
    C: Clone + Eq + CloneBacking,
    A: Clone + CloneBacking,
{
    let count = channels.len().min(patterns.len());
    let mut chosen = buffer::<(usize, usize)>(count, meter)?;
    let mut candidates = buffer(count, meter)?;
    let mut complete = true;
    for (channel, pattern) in channels.iter().zip(patterns) {
        let Some(channel_index) = channel_position(data, channel, meter)? else {
            complete = false;
            continue;
        };
        let mut found = false;
        for (position, (datum, index)) in data[channel_index].values.iter().enumerate() {
            meter.reserve(
                chosen
                    .len()
                    .checked_add(1)
                    .ok_or(RSpaceError::HostWorkRejected)?,
                chosen
                    .len()
                    .checked_mul(size_of::<(usize, usize)>())
                    .ok_or(RSpaceError::HostWorkRejected)?,
                0,
            )?;
            if !datum.persist && chosen.contains(&(channel_index, position)) {
                continue;
            }
            let Some(matched) = space.matcher.get_metered(pattern, &datum.a, meter)? else {
                continue;
            };
            native_backing::reserve_copy_and_cleanup(channel, meter)?;
            native_backing::reserve_copy_and_cleanup(&datum.source, meter)?;
            native_backing::reserve_copy_and_cleanup(&datum.a, meter)?;
            candidates.push(ConsumeCandidate {
                channel: channel.clone(),
                datum: Datum {
                    a: matched,
                    persist: datum.persist,
                    source: datum.source.clone(),
                },
                removed_datum: datum.a.clone(),
                datum_index: *index,
            });
            if !datum.persist {
                chosen.push((channel_index, position));
            }
            found = true;
            break;
        }
        if !found {
            complete = false;
        }
    }
    if !complete {
        return Ok(None);
    }
    let mut matched = buffer::<&A>(candidates.len(), meter)?;
    for candidate in &candidates {
        matched.push(&candidate.datum.a);
    }
    if !space
        .matcher
        .check_commit_metered(continuation, &matched, meter)?
    {
        return Ok(None);
    }
    Ok(Some(candidates))
}

/// D-D4 (DR-106) fixtures: stores `stored` on a fresh space, then builds the
/// channel data of `channels` with an optional incoming datum on channel 1,
/// and passes the space, the entries and the recorded positions to `check`.
fn with_channel_data(
    channels: &[u8],
    stored: &[(u8, u8, bool)],
    incoming: Option<(u8, bool)>,
    check: impl FnOnce(&Space, &[ChannelData<'_, u8, u8>], &[usize]),
) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(async {
            let space = space().await;
            for (channel, value, persistent) in stored {
                put(&space, *channel, *value, *persistent);
            }
            let keys = channel_keys(channels);
            let source = incoming.map(|(value, persistent)| {
                (value, persistent, Produce::create(&1u8, &value, persistent))
            });
            let meter = Meter::default();
            with_reader(&space, &meter, |reader| {
                let views = space
                    .metered_data_views(channels, &keys, reader)
                    .expect("data views");
                let incoming = source
                    .as_ref()
                    .map(|(value, persistent, source)| (&1u8, value, *persistent, source));
                let (data, positions) = space
                    .metered_channel_data(channels, &views, incoming, None, reader)
                    .expect("channel data");
                check(&space, &data, &positions);
            });
        });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// D-D4 (DR-106; `ChannelPositions.recorded_position_equals_first_equal_search`,
    /// `entries_pairwise_unequal`): the position recorded for each channel is
    /// the position that the first-equal search finds in the finished entries.
    /// The entries are pairwise unequal, one for each distinct channel, and
    /// each borrows the first occurrence of its channel.
    #[test]
    fn recorded_positions_equal_first_equal_search(
        channels in prop::collection::vec(1u8..5, 0..10),
        stored in prop::collection::vec((1u8..5, 1u8..64, any::<bool>()), 0..12),
        incoming in prop::option::of((1u8..64, any::<bool>())),
    ) {
        with_channel_data(&channels, &stored, incoming, |_, data, positions| {
            assert_eq!(positions.len(), channels.len());
            for (channel, position) in channels.iter().zip(positions) {
                let first = data.iter().position(|entry| entry.channel == channel);
                assert_eq!(Some(*position), first);
            }
            for (later, entry) in data.iter().enumerate() {
                for earlier in &data[..later] {
                    assert_ne!(entry.channel, earlier.channel);
                }
                let first = channels
                    .iter()
                    .position(|channel| channel == entry.channel)
                    .expect("an entry has a channel");
                assert!(std::ptr::eq(entry.channel, &channels[first]));
            }
            assert_eq!(data.len(), channels.iter().collect::<BTreeSet<_>>().len());
        });
    }

    /// D-D4 (DR-106; `ChannelPositions.first_equal_search_equals_recorded_positions`):
    /// the selection that reads the recorded positions equals the frozen
    /// legacy selection on repeated channels, persistent data, an incoming
    /// datum and a failed commit. The charges differ by exactly the legacy
    /// searches less one position word for each pattern. The values and the
    /// patterns come from a small range, so that about a fifth of the cases
    /// select data (measured: 27 of 128).
    #[test]
    fn repeated_channel_selection_matches_frozen_legacy_selection(
        channels in prop::collection::vec(1u8..4, 0..8),
        patterns in prop::collection::vec(0u8..4, 0..8),
        stored in prop::collection::vec((1u8..4, 1u8..4, any::<bool>()), 0..16),
        incoming in prop::option::of((1u8..4, any::<bool>())),
        commits in prop::bool::weighted(0.75),
    ) {
        let continuation = if commits { 9u8 } else { 255u8 };
        with_channel_data(&channels, &stored, incoming, |space, data, positions| {
            let current = Meter::default();
            let actual = space
                .metered_match_data(&channels, positions, &patterns, &continuation, data, &current)
                .expect("current selection");
            let legacy = Meter::default();
            let expected = legacy_match_data(space, &channels, &patterns, &continuation, data, &legacy)
                .expect("legacy selection");
            assert_eq!(actual.is_some(), expected.is_some());
            if let (Some(actual), Some(expected)) = (&actual, &expected) {
                same_data(actual, expected);
            }
            let searches = Meter::default();
            for channel in channels.iter().take(patterns.len()) {
                channel_position(data, channel, &searches).expect("legacy search");
            }
            let pairs = channels.len().min(patterns.len());
            let word = [1, POSITION_BYTES, 0];
            for dimension in 0..3 {
                assert_eq!(
                    current.used.get()[dimension] + searches.used.get()[dimension],
                    legacy.used.get()[dimension] + pairs * word[dimension],
                    "dimension {dimension}"
                );
            }
        });
    }
}

/// D-D4 (DR-106) fixtures: eight string channels of `size` bytes after a
/// one-digit prefix, so each holds `size + 1` bytes.
fn sized_channels(size: usize) -> Vec<String> {
    (0..8)
        .map(|index| format!("{index}{}", "c".repeat(size)))
        .collect()
}

/// D-D4 (DR-106; `ChannelPositions.recorded_attempt_charge`): the match reads
/// one recorded position for each pattern. Over entries without data, the
/// charge is the two selection buffers and one word for each pattern, so it
/// does not depend on the position of the entry or on the sizes of the
/// channels.
#[tokio::test]
async fn match_data_position_charge_is_one_word_per_pattern() {
    let space = string_space().await;
    let charge = |count: usize, position: usize, size: usize| {
        let channels = sized_channels(size);
        let data: Vec<_> = channels
            .iter()
            .map(|channel| ChannelData {
                channel,
                values: Vec::new(),
            })
            .collect();
        let meter = Meter::default();
        let matched = space
            .metered_match_data(
                &vec![channels[position].clone(); count],
                &vec![position; count],
                &vec!["a".to_string(); count],
                &"k".to_string(),
                &data,
                &meter,
            )
            .expect("metered match data");
        assert!(matched.is_none());
        meter.used.get()
    };
    for count in 1..4 {
        let expected = Meter::default();
        buffer::<(usize, usize)>(count, &expected).expect("chosen buffer");
        buffer::<ConsumeCandidate<String, String>>(count, &expected).expect("candidate buffer");
        expected
            .reserve(count, count * POSITION_BYTES, 0)
            .expect("position words");
        for (position, size) in [(0, 1), (7, 1), (0, 4_096), (7, 4_096)] {
            assert_eq!(
                charge(count, position, size),
                expected.used.get(),
                "{count} patterns, position {position}, size {size}"
            );
        }
    }
}

/// Negative control for D-D4 (DR-106;
/// `ChannelPositions.legacy_position_search_charge_example`): the frozen
/// legacy selection searched for the entry of each pattern's channel. Each
/// visited entry charged an inspection of both channels and one comparison,
/// so the charge grew with the position of the entry and with the sizes of
/// the channels.
#[tokio::test]
async fn legacy_position_search_charge_grew_with_position_and_channel_size() {
    let space = string_space().await;
    let charge = |position: usize, size: usize| {
        let channels = sized_channels(size);
        let data: Vec<_> = channels
            .iter()
            .map(|channel| ChannelData {
                channel,
                values: Vec::new(),
            })
            .collect();
        let meter = Meter::default();
        let matched = legacy_match_data(
            &space,
            std::slice::from_ref(&channels[position]),
            &["a".to_string()],
            &"k".to_string(),
            &data,
            &meter,
        )
        .expect("legacy match data");
        assert!(matched.is_none());
        meter.used.get()
    };
    let inspection = |size: usize| {
        let meter = Meter::default();
        native_backing::inspect(&sized_channels(size)[0], &meter).expect("channel inspection");
        meter.used.get()
    };
    for size in [1, 4_096] {
        let near = charge(0, size);
        let far = charge(7, size);
        let visit = inspection(size);
        for dimension in 0..3 {
            let comparison = usize::from(dimension == 0);
            assert_eq!(
                far[dimension],
                near[dimension] + 7 * (2 * visit[dimension] + comparison),
                "size {size}, dimension {dimension}"
            );
        }
    }
    let small = charge(0, 1);
    let large = charge(0, 4_096);
    assert_eq!(large[1], small[1] + 2 * (inspection(4_096)[1] - inspection(1)[1]));
    assert!(large[1] >= small[1] + 2 * 4_095, "{small:?} vs {large:?}");
}

/// D-D4 (DR-106): the entries borrow their channels, so building the channel
/// data of one channel charges the same for a channel of 2 bytes and one of
/// 4,097 bytes. The legacy entries copied each new channel and charged its
/// copy and cleanup.
#[tokio::test]
async fn channel_data_charge_excludes_channel_copies() {
    let space = string_space().await;
    let charge = |size: usize| {
        let channels = vec![sized_channels(size)[0].clone()];
        space
            .get_store()
            .put_datum(&channels[0], Datum::create(&channels[0], "a".to_string(), false));
        let keys = string_channel_keys(&channels);
        let meter = Meter::default();
        with_string_reader(&space, &meter, |reader| {
            let views = space
                .metered_data_views(&channels, &keys, reader)
                .expect("data views");
            let before = meter.used.get();
            let (data, positions) = space
                .metered_channel_data(&channels, &views, None, None, reader)
                .expect("channel data");
            assert_eq!(positions, [0]);
            assert_eq!(data.len(), 1);
            assert_eq!(data[0].values.len(), 1);
            let after = meter.used.get();
            [after[0] - before[0], after[1] - before[1], after[2] - before[2]]
        })
    };
    assert_eq!(charge(1), charge(4_096));
    let copy = Meter::default();
    native_backing::reserve_copy_and_cleanup(&sized_channels(4_096)[0], &copy)
        .expect("legacy channel copy");
    assert!(copy.used.get()[1] >= 2 * 4_097, "{:?}", copy.used.get());
}

/// D-D4 (DR-106): the consume selection over repeated channels succeeds with
/// exactly the credit that it reserves. A credit one unit short in any
/// dimension rejects it before an unpaid allocation and leaves the state
/// unchanged.
#[tokio::test]
async fn repeated_channel_consume_accepts_exact_credit_and_rejects_each_smaller_dimension() {
    let space = space().await;
    for (channel, value) in [(1, 7), (2, 8), (1, 9), (2, 10)] {
        put(&space, channel, value, false);
    }
    let channels = [1, 2, 1, 2];
    let patterns = [0, 0, 0, 0];
    let source = Consume::create(&channels.to_vec(), &patterns.to_vec(), &9u8, false);
    let keys = channel_keys(&channels);
    let peeks = BTreeSet::new();
    let run = |meter: &Meter| {
        with_reader(&space, meter, |reader| {
            space.prepare_metered_consume_candidate(
                &channels, &keys, &patterns, &9, &source, &peeks, None, reader,
            )
        })
    };
    let baseline = Meter::default();
    let expected = run(&baseline)
        .expect("baseline selection")
        .expect("a candidate");
    assert_eq!(expected.data.len(), 4);
    let before = state(&space);
    let exact = Meter {
        limit: Some(baseline.used.get()),
        ..Meter::default()
    };
    let actual = run(&exact).expect("exact credit").expect("a candidate");
    same_data(&actual.data, &expected.data);
    assert_eq!(
        bincode::serialize(&actual.comm).expect("comm encoding"),
        bincode::serialize(&expected.comm).expect("comm encoding")
    );
    for dimension in 0..3 {
        let mut limit = baseline.used.get();
        limit[dimension] -= 1;
        let meter = Meter {
            limit: Some(limit),
            ..Meter::default()
        };
        let (result, bytes) = measure_allocations(|| run(&meter));
        assert!(matches!(result, Err(RSpaceError::HostWorkRejected)), "dimension {dimension}");
        assert!(bytes <= meter.used.get()[2], "dimension {dimension}");
        assert_eq!(state(&space), before);
    }
}

/// D-D4 (DR-106): a position list whose length differs from the channels', or
/// a position outside the entries, rejects the match.
#[tokio::test]
async fn match_data_rejects_inconsistent_positions() {
    let space = string_space().await;
    let channel = "channel".to_string();
    let datum = Datum::create(&channel, "a".to_string(), false);
    let data = vec![ChannelData {
        channel: &channel,
        values: vec![(Cow::Borrowed(&datum), 0)],
    }];
    let run = |positions: &[usize]| {
        space.metered_match_data(
            std::slice::from_ref(&channel),
            positions,
            &["a".to_string()],
            &"k".to_string(),
            &data,
            &Meter::default(),
        )
    };
    assert!(matches!(run(&[0]), Ok(Some(_))));
    for positions in [&[][..], &[0, 0], &[1]] {
        assert!(matches!(run(positions), Err(RSpaceError::HostWorkRejected)), "{positions:?}");
    }
}

/// D-D4 (DR-106): the positions buffer charges one operation for each slot
/// plus one, and one word for each slot in scanned and in backing bytes. The
/// reservation covers the allocation.
#[test]
fn position_buffer_charges_one_word_per_slot_and_prepays_it() {
    for length in [0, 1, 7, 64] {
        let meter = Meter::default();
        let (positions, bytes) = measure_allocations(|| position_buffer(length, &meter));
        let positions = positions.expect("position buffer");
        assert!(positions.capacity() >= length);
        assert_eq!(meter.used.get(), [
            length + 1,
            length * POSITION_BYTES,
            length * POSITION_BYTES
        ]);
        assert!(bytes <= meter.used.get()[2], "length {length}: requested {bytes}");
    }
}
