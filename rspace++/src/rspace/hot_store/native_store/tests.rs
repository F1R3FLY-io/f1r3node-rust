//! D-S1 (D-C2c, DR-96): tests of the digest-keyed native store. The legacy
//! native store (`InMemHotStore`) is the oracle.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

use proptest::prelude::*;

use super::*;
use crate::rspace::hashing::native_source::channel_key;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::history::native_reader::measure_allocations;
use crate::rspace::trace::event::Consume;

type Store = NativeHotStore<String, String, String, String>;
type Legacy = Box<dyn HotStore<String, String, String, String>>;
type Waiting = WaitingContinuation<String, String>;

struct EmptyReader;

impl HistoryReaderBase<String, String, String, String> for EmptyReader {
    fn get_data_proj(&self, _: &String) -> Vec<Datum<String>> { vec![] }

    fn get_continuations_proj(&self, _: &Vec<String>) -> Vec<Waiting> { vec![] }

    fn get_joins_proj(&self, _: &String) -> Vec<Vec<String>> { vec![] }
}

fn legacy() -> Legacy { HotStoreInstances::create_from_hr(Box::new(EmptyReader)) }

/// A meter that sums every charge and rejects the charge at index `reject`.
#[derive(Default)]
struct Meter {
    calls: Cell<usize>,
    reject: Option<usize>,
    used: Cell<[usize; 3]>,
}

impl Meter {
    fn rejecting(reject: usize) -> Self {
        Self {
            reject: Some(reject),
            ..Self::default()
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
        let call = self.calls.get();
        self.calls.set(call + 1);
        if self.reject == Some(call) {
            return Err(RSpaceError::HostWorkRejected);
        }
        let [o, s, b] = self.used.get();
        self.used.set([o + operations, s + scanned, b + backing]);
        Ok(())
    }
}

fn free() -> Meter { Meter::default() }

const CHANNELS: [&str; 3] = ["a", "bb", "ccc"];

fn channel(index: usize) -> String { CHANNELS[index % CHANNELS.len()].to_owned() }

fn group(indices: &[usize]) -> Vec<String> { indices.iter().map(|index| channel(*index)).collect() }

fn key(channel: &String) -> StoreKey { channel_key(channel, &free()).expect("an unlimited meter") }

fn keys(channels: &[String]) -> GroupKeys {
    GroupKeys::build(channels, &free()).expect("an unlimited meter")
}

/// The history of the cold reads: a few data, one continuation group and
/// one join.
fn history_data(channel: &str) -> Vec<Datum<String>> {
    match channel {
        "a" => vec![
            Datum::create(&channel.to_owned(), "h1".to_owned(), false),
            Datum::create(&channel.to_owned(), "h2".to_owned(), true),
        ],
        "bb" => vec![Datum::create(&channel.to_owned(), "h3".to_owned(), false)],
        _ => vec![],
    }
}

fn waiting(channels: &[String], body: &str, persist: bool, peek: bool) -> Waiting {
    let patterns = vec!["p".to_owned(); channels.len()];
    let source = Consume::create(&channels.to_vec(), &patterns, &body.to_owned(), persist);
    WaitingContinuation {
        patterns,
        continuation: body.to_owned(),
        persist,
        peeks: if peek {
            BTreeSet::from([0])
        } else {
            BTreeSet::new()
        },
        source,
    }
}

fn history_continuations(channels: &[String]) -> Vec<Waiting> {
    if channels == ["a".to_owned(), "bb".to_owned()] {
        vec![waiting(channels, "stored", false, false)]
    } else {
        vec![]
    }
}

fn history_joins(channel: &str) -> Vec<Vec<String>> {
    if channel == "a" {
        vec![vec!["a".to_owned(), "bb".to_owned()]]
    } else {
        vec![]
    }
}

#[derive(Clone, Debug)]
enum Op {
    ReadData(usize),
    ViewData(usize),
    ReadJoins(usize),
    ReadContinuations(Vec<usize>),
    ViewContinuations(Vec<usize>),
    Consume {
        channels: Vec<usize>,
        body: u8,
        persist: bool,
        peek: bool,
    },
    Put {
        channel: usize,
        value: u8,
        persist: bool,
    },
    RetireData {
        channel: usize,
    },
    RetireMatch {
        channels: Vec<usize>,
        persistent: bool,
    },
    InstallContinuation {
        channels: Vec<usize>,
        body: u8,
    },
    InstallJoin {
        channel: usize,
        join: Vec<usize>,
    },
}

fn op() -> impl Strategy<Value = Op> {
    let channels = prop::collection::vec(0usize..3, 1..3);
    prop_oneof![
        (0usize..3).prop_map(Op::ReadData),
        (0usize..3).prop_map(Op::ViewData),
        (0usize..3).prop_map(Op::ReadJoins),
        channels.clone().prop_map(Op::ReadContinuations),
        channels.clone().prop_map(Op::ViewContinuations),
        (channels.clone(), 0u8..3, any::<bool>(), any::<bool>()).prop_map(
            |(channels, body, persist, peek)| Op::Consume {
                channels,
                body,
                persist,
                peek
            }
        ),
        (0usize..3, 0u8..4, any::<bool>()).prop_map(|(channel, value, persist)| Op::Put {
            channel,
            value,
            persist
        }),
        (0usize..3).prop_map(|channel| Op::RetireData { channel }),
        (channels.clone(), any::<bool>()).prop_map(|(channels, persistent)| Op::RetireMatch {
            channels,
            persistent
        }),
        (channels.clone(), 0u8..3)
            .prop_map(|(channels, body)| Op::InstallContinuation { channels, body }),
        (0usize..3, channels).prop_map(|(channel, join)| Op::InstallJoin { channel, join }),
    ]
}

fn read_data(
    store: &Store,
    legacy: &Legacy,
    name: &String,
) -> (Result<Vec<Datum<String>>, RSpaceError>, Result<Vec<Datum<String>>, RSpaceError>) {
    let read = || Ok(history_data(name));
    (store.data(name, key(name), &read, &free()), legacy.get_data_with_reader(name, &read, &free()))
}

fn prefetch_group(store: &Store, legacy: &Legacy, channels: &[String]) {
    let read = || Ok(history_continuations(channels));
    let _ = store.continuation_views(channels, &keys(channels), &read, &free());
    let _ = legacy.get_continuation_views_with_reader(channels, &read, &free());
    for name in channels {
        let read = || Ok(history_joins(name));
        let _ = store.joins(name, key(name), &read, &free());
        let _ = legacy.get_joins_with_reader(name, &read, &free());
    }
}

/// The first datum of each channel position whose cached data hold a
/// non-persistent datum at index 0, as the results of a match. Both stores
/// read the data, so they cache the same entries.
fn retirement(
    store: &Store,
    legacy: &Legacy,
    channels: &[String],
) -> (Vec<RSpaceResult<String, String>>, Vec<(usize, i32)>) {
    let mut data = Vec::new();
    let mut retirement = Vec::new();
    for (position, name) in channels.iter().enumerate() {
        let (digest, oracle) = read_data(store, legacy, name);
        assert_eq!(digest, oracle);
        let current = digest.unwrap_or_default();
        let first = current.first();
        data.push(RSpaceResult {
            channel: name.clone(),
            matched_datum: first.map_or_else(String::new, |datum| datum.a.clone()),
            removed_datum: first.map_or_else(String::new, |datum| datum.a.clone()),
            persistent: first.is_none_or(|datum| datum.persist),
        });
        if first.is_some_and(|datum| !datum.persist) {
            retirement.push((position, 0));
        }
    }
    (data, retirement)
}

fn apply(store: &Store, legacy: &Legacy, op: &Op) -> Result<(), TestCaseError> {
    match op {
        Op::ReadData(index) => {
            let (digest, oracle) = read_data(store, legacy, &channel(*index));
            prop_assert_eq!(digest, oracle);
        }
        Op::ViewData(index) => {
            let name = channel(*index);
            let read = || Ok(history_data(&name));
            let digest = store.data_view(&name, key(&name), &read, &free());
            let oracle = legacy.get_data_view_with_reader(&name, &read, &free());
            prop_assert_eq!(
                digest.map(|view| view.values().to_vec()),
                oracle.map(|view| view.values().to_vec())
            );
        }
        Op::ReadJoins(index) => {
            let name = channel(*index);
            let read = || Ok(history_joins(&name));
            prop_assert_eq!(
                store.joins(&name, key(&name), &read, &free()),
                legacy.get_joins_with_reader(&name, &read, &free())
            );
        }
        Op::ReadContinuations(indices) => {
            let channels = group(indices);
            let read = || Ok(history_continuations(&channels));
            prop_assert_eq!(
                store.continuations(&channels, &keys(&channels), &read, &free()),
                legacy.get_continuations_with_reader(&channels, &read, &free())
            );
        }
        Op::ViewContinuations(indices) => {
            let channels = group(indices);
            let read = || Ok(history_continuations(&channels));
            let digest = store.continuation_views(&channels, &keys(&channels), &read, &free());
            let oracle = legacy.get_continuation_views_with_reader(&channels, &read, &free());
            let owned = |views: Vec<Arc<Waiting>>| {
                views
                    .iter()
                    .map(|view| view.as_ref().clone())
                    .collect::<Vec<_>>()
            };
            prop_assert_eq!(digest.map(owned), oracle.map(owned));
        }
        Op::Consume {
            channels,
            body,
            persist,
            peek,
        } => {
            let channels = group(channels);
            prefetch_group(store, legacy, &channels);
            let value = waiting(&channels, &format!("body-{body}"), *persist, *peek);
            prop_assert_eq!(
                store.store_consume(&channels, &keys(&channels), value.clone(), &free()),
                legacy.store_consume_metered(&channels, value, &free())
            );
        }
        Op::Put {
            channel: index,
            value,
            persist,
        } => {
            let name = channel(*index);
            let read = || Ok(history_data(&name));
            let _ = store.data_view(&name, key(&name), &read, &free());
            let _ = legacy.get_data_view_with_reader(&name, &read, &free());
            let datum = Datum::create(&name, format!("v{value}"), *persist);
            prop_assert_eq!(
                store.put_datum(key(&name), datum.clone(), &free()),
                legacy.put_datum_metered(&name, datum, &free())
            );
        }
        Op::RetireData { channel: index } => {
            let channels = vec![channel(*index)];
            let (data, retired) = retirement(store, legacy, &channels);
            let channel_keys = vec![key(&channels[0])];
            prop_assert_eq!(
                store.retire_data(&data, &channel_keys, &retired, &free()),
                legacy.retire_data_metered(&data, &retired, &free())
            );
        }
        Op::RetireMatch {
            channels,
            persistent,
        } => {
            let channels = group(channels);
            prefetch_group(store, legacy, &channels);
            let count = legacy
                .get_continuation_views_with_reader(
                    &channels,
                    &|| Ok(history_continuations(&channels)),
                    &free(),
                )
                .map(|views| views.len())
                .unwrap_or_default();
            if count == 0 {
                return Ok(());
            }
            let (data, retired) = retirement(store, legacy, &channels);
            // The match retires the last continuation of the group: index
            // count - 1 in the merged list, which is regular when the group
            // has a regular continuation.
            let index = i32::try_from(count - 1).expect("a small index");
            prop_assert_eq!(
                store.retire_produce_match(
                    &channels,
                    &keys(&channels),
                    index,
                    *persistent,
                    &data,
                    &retired,
                    &free()
                ),
                legacy.retire_produce_match_metered(
                    &channels,
                    index,
                    *persistent,
                    &data,
                    &retired,
                    &free()
                )
            );
        }
        Op::InstallContinuation { channels, body } => {
            let channels = group(channels);
            let value = waiting(&channels, &format!("installed-{body}"), true, false);
            prop_assert_eq!(
                store.install_continuation(&channels, &keys(&channels), value.clone(), &free()),
                legacy.install_continuation_metered(&channels, value, &free())
            );
        }
        Op::InstallJoin {
            channel: index,
            join,
        } => {
            let name = channel(*index);
            let join = group(join);
            prop_assert_eq!(
                store.install_join(&name, key(&name), &join, &free()),
                legacy.install_join_metered(&name, &join, &free())
            );
        }
    }
    Ok(())
}

/// The export as a sorted multiset of formatted actions.
fn export_multiset(actions: Vec<HotStoreAction<String, String, String, String>>) -> Vec<String> {
    let mut formatted: Vec<String> = actions.iter().map(|action| format!("{action:?}")).collect();
    formatted.sort();
    formatted
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// D-S1 (D-C2c, DR-96): for random operation sequences, the digest store
    /// returns what the legacy native store returns (reads, duplicate
    /// decisions and depths, publications, retirements, installations and
    /// their errors), and the two exports are the same multiset.
    #[test]
    fn digest_store_matches_legacy_store(ops in prop::collection::vec(op(), 0..24)) {
        let store = Store::new();
        let legacy = legacy();
        for op in &ops {
            apply(&store, &legacy, op)?;
        }
        let digest = store.changes(&free()).map(export_multiset);
        let oracle = legacy.changes_metered(&free()).map(export_multiset);
        prop_assert_eq!(digest, oracle);
    }
}

/// The observable state of a store: its entry counts and its export.
fn observed(store: &Store) -> ([usize; 5], Result<Vec<String>, RSpaceError>) {
    (store.entry_counts(), store.changes(&free()).map(export_multiset))
}

/// A store with cached data, continuations and joins for the group
/// ["a", "bb"] and an installed continuation on ["ccc"].
fn prepared() -> Store {
    let store = Store::new();
    let channels = group(&[0, 1]);
    let read = || Ok(history_continuations(&channels));
    store
        .continuation_views(&channels, &keys(&channels), &read, &free())
        .expect("a free meter");
    for name in &channels {
        store
            .joins(name, key(name), &|| Ok(history_joins(name)), &free())
            .expect("a free meter");
        store
            .data_view(name, key(name), &|| Ok(history_data(name)), &free())
            .expect("a free meter");
    }
    let installed = group(&[2]);
    store
        .install_continuation(
            &installed,
            &keys(&installed),
            waiting(&installed, "installed", true, false),
            &free(),
        )
        .expect("a free meter");
    store
}

/// A history read that hands over a prebuilt value without allocating.
fn handover<T: Default>(value: &RefCell<T>) -> impl Fn() -> Result<T, RSpaceError> + '_ {
    move || Ok(std::mem::take(&mut *value.borrow_mut()))
}

/// The inputs of one store method, built before the measured call.
enum Call {
    Data(String, StoreKey, RefCell<Vec<Datum<String>>>),
    DataView(String, StoreKey, RefCell<Vec<Datum<String>>>),
    Joins(String, StoreKey, RefCell<Vec<Vec<String>>>),
    Continuations(Vec<String>, GroupKeys, RefCell<Vec<Waiting>>),
    ContinuationViews(Vec<String>, GroupKeys, RefCell<Vec<Waiting>>),
    StoreConsume(Vec<String>, GroupKeys, Waiting),
    PutDatum(StoreKey, Datum<String>),
    RetireData(Vec<RSpaceResult<String, String>>, Vec<StoreKey>, Vec<(usize, i32)>),
    RetireMatch(Vec<String>, GroupKeys, Vec<RSpaceResult<String, String>>, Vec<(usize, i32)>),
    InstallContinuation(Vec<String>, GroupKeys, Waiting),
    InstallJoin(String, StoreKey, Vec<String>),
    Changes,
}

fn result_of(name: &str, value: &str, persistent: bool) -> RSpaceResult<String, String> {
    RSpaceResult {
        channel: name.to_owned(),
        matched_datum: value.to_owned(),
        removed_datum: value.to_owned(),
        persistent,
    }
}

/// One call of each store method, on the state of `prepared`.
fn calls() -> Vec<(&'static str, Call)> {
    let a = channel(0);
    let bb = channel(1);
    let ccc = channel(2);
    let pair = group(&[0, 1]);
    vec![
        ("data", Call::Data(ccc.clone(), key(&ccc), RefCell::new(history_data(&ccc)))),
        ("data_view", Call::DataView(bb.clone(), key(&bb), RefCell::new(history_data(&bb)))),
        ("data cold", Call::Data(a.clone(), key(&a), RefCell::new(Vec::new()))),
        ("joins", Call::Joins(ccc.clone(), key(&ccc), RefCell::new(vec![group(&[2, 0])]))),
        (
            "continuations",
            Call::Continuations(
                group(&[1, 0]),
                keys(&group(&[1, 0])),
                RefCell::new(vec![waiting(&group(&[1, 0]), "cold", false, false)]),
            ),
        ),
        (
            "continuation_views",
            Call::ContinuationViews(group(&[2]), keys(&group(&[2])), RefCell::new(Vec::new())),
        ),
        (
            "store_consume",
            Call::StoreConsume(pair.clone(), keys(&pair), waiting(&pair, "new", false, true)),
        ),
        ("put_datum", Call::PutDatum(key(&a), Datum::create(&a, "put".to_owned(), false))),
        (
            "retire_data",
            Call::RetireData(vec![result_of("a", "h1", false)], vec![key(&a)], vec![(0, 0)]),
        ),
        (
            "retire_produce_match",
            Call::RetireMatch(
                pair.clone(),
                keys(&pair),
                vec![result_of("a", "h1", false), result_of("bb", "h3", false)],
                vec![(0, 0), (1, 0)],
            ),
        ),
        (
            "install_continuation",
            Call::InstallContinuation(
                pair.clone(),
                keys(&pair),
                waiting(&pair, "installed", true, false),
            ),
        ),
        ("install_join", Call::InstallJoin(bb.clone(), key(&bb), group(&[1, 2]))),
        ("changes", Call::Changes),
    ]
}

fn run(store: &Store, call: Call, meter: &Meter) -> Result<(), RSpaceError> {
    match call {
        Call::Data(name, key, read) => store.data(&name, key, &handover(&read), meter).map(drop),
        Call::DataView(name, key, read) => store
            .data_view(&name, key, &handover(&read), meter)
            .map(drop),
        Call::Joins(name, key, read) => store.joins(&name, key, &handover(&read), meter).map(drop),
        Call::Continuations(channels, keys, read) => store
            .continuations(&channels, &keys, &handover(&read), meter)
            .map(drop),
        Call::ContinuationViews(channels, keys, read) => store
            .continuation_views(&channels, &keys, &handover(&read), meter)
            .map(drop),
        Call::StoreConsume(channels, keys, value) => store
            .store_consume(&channels, &keys, value, meter)
            .map(drop),
        Call::PutDatum(key, datum) => store.put_datum(key, datum, meter),
        Call::RetireData(data, keys, retired) => store.retire_data(&data, &keys, &retired, meter),
        Call::RetireMatch(channels, keys, data, retired) => {
            store.retire_produce_match(&channels, &keys, 0, false, &data, &retired, meter)
        }
        Call::InstallContinuation(channels, keys, value) => {
            store.install_continuation(&channels, &keys, value, meter)
        }
        Call::InstallJoin(name, key, join) => store.install_join(&name, key, &join, meter),
        Call::Changes => store.changes(meter).map(drop),
    }
}

/// D-S1 (D-C2c, DR-96): every reservation of every store method comes
/// before the work it pays for. At each cut the method returns the host
/// error, allocates at most the backing it reserved, and leaves the store
/// unchanged.
#[test]
fn every_native_store_cut_preserves_state() {
    let count = calls().len();
    for index in 0..count {
        let (name, call) = calls().swap_remove(index);
        let baseline = Meter::default();
        run(&prepared(), call, &baseline).unwrap_or_else(|error| panic!("{name}: {error:?}"));
        let before = observed(&prepared());
        for cut in 0..baseline.calls.get() {
            let store = prepared();
            let (_, call) = calls().swap_remove(index);
            let meter = Meter::rejecting(cut);
            let (result, allocated) = measure_allocations(|| run(&store, call, &meter));
            assert_eq!(result, Err(RSpaceError::HostWorkRejected), "{name}, cut {cut}");
            assert!(
                allocated <= meter.used.get()[2],
                "{name}, cut {cut}: allocated {allocated}, reserved {}",
                meter.used.get()[2]
            );
            assert_eq!(observed(&store), before, "{name}, cut {cut}");
        }
    }
}

/// D-S1 (D-C2c, DR-96): a data view holds the entry it read. A later
/// publication replaces the entry and leaves the view's values unchanged.
#[test]
fn views_are_stable_entry_snapshots() {
    let store = prepared();
    let name = channel(0);
    let read = || Ok(history_data(&name));
    let before = store
        .data_view(&name, key(&name), &read, &free())
        .expect("a free meter");
    let values = before.values().to_vec();
    store
        .put_datum(key(&name), Datum::create(&name, "later".to_owned(), false), &free())
        .expect("a free meter");
    assert_eq!(before.values(), values.as_slice());
    let after = store
        .data_view(&name, key(&name), &read, &free())
        .expect("a free meter");
    assert_eq!(after.values().len(), values.len() + 1);
    assert_eq!(after.values()[0].a, "later");
}

/// Two channels whose keys share a shard.
fn same_shard_channels() -> (String, String) {
    let mut by_shard: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    for index in 0.. {
        let name = format!("shared-{index}");
        let shard = key(&name).shard();
        if let Some(previous) = by_shard.insert(shard, name.clone()) {
            return (previous, name);
        }
    }
    unreachable!("the shard space is finite")
}

/// D-S1 (D-C2c, DR-96): the store's charges for one channel do not depend
/// on how the operations on another channel in the same shard interleave.
#[test]
fn store_charges_are_schedule_independent() {
    let (first, second) = same_shard_channels();
    assert_eq!(key(&first).shard(), key(&second).shard());
    let program = |store: &Store, name: &String, step: usize, meter: &Meter| match step {
        0 | 2 => store
            .data_view(name, key(name), &|| Ok(Vec::new()), meter)
            .map(drop),
        _ => store.put_datum(key(name), Datum::create(name, "v".to_owned(), false), meter),
    };
    let run = |schedule: &[usize]| {
        let store = Store::new();
        let meter = Meter::default();
        let mut steps = [0, 0];
        for &which in schedule {
            let name = if which == 0 { &first } else { &second };
            program(&store, name, steps[which], &meter).expect("an unlimited meter");
            steps[which] += 1;
        }
        meter.used.get()
    };
    let expected = run(&[0, 0, 0, 1, 1, 1]);
    for schedule in [[0, 1, 0, 1, 0, 1], [1, 1, 1, 0, 0, 0], [0, 1, 1, 0, 1, 0], [1, 0, 0, 1, 0, 1]]
    {
        assert_eq!(run(&schedule), expected, "{schedule:?}");
    }
}
