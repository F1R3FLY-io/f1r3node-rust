//! D-S1 (D-C2c, DR-96): tests of the digest-keyed native store. The legacy
//! native store (`InMemHotStore`) is the oracle.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

use proptest::prelude::*;

use super::*;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::channel_key;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::history::history_repository::{HistoryRepository, HistoryRepositoryInstances};
use crate::rspace::history::native_checkpoint::NativeCheckpoint;
use crate::rspace::history::native_reader::measure_allocations;
use crate::rspace::hot_store_action::{
    DeleteAction, DeleteContinuations, DeleteData, DeleteJoins, InsertAction, InsertContinuations,
    InsertData, InsertJoins, NativeExportAction,
};
use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
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
            // Changed by D-C2e (D-S1, DR-96): the publication passes the
            // channel for the collision check.
            // prop_assert_eq!(
            //     store.put_datum(key(&name), datum.clone(), &free()),
            //     legacy.put_datum_metered(&name, datum, &free())
            // );
            prop_assert_eq!(
                store.put_datum(&name, key(&name), datum.clone(), &free()),
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

/// An empty history on in-memory stores.
fn empty_history() -> Box<dyn HistoryRepository<String, String, String, String> + Send + Sync> {
    HistoryRepositoryInstances::lmdb_repository(
        Arc::new(InMemoryKeyValueStore::new()),
        Arc::new(InMemoryKeyValueStore::new()),
        Arc::new(InMemoryKeyValueStore::new()),
    )
    .expect("an in-memory history")
}

/// The root that a checkpoint of `actions` would publish over `history`.
fn export_root(
    history: &(dyn HistoryRepository<String, String, String, String> + Send + Sync),
    actions: Vec<HotStoreAction<String, String, String, String>>,
) -> Result<Option<Blake2b256Hash>, RSpaceError> {
    history
        .prepare_native_checkpoint(actions, &free())
        .map(|prepared| prepared.root)
}

/// The map (continuations, data, joins) and the store key of an exported
/// action.
fn export_position(action: &HotStoreAction<String, String, String, String>) -> (u8, StoreKey) {
    match action {
        HotStoreAction::Insert(InsertAction::InsertContinuations(insert)) => {
            (0, keys(&insert.channels).group)
        }
        HotStoreAction::Delete(DeleteAction::DeleteContinuations(delete)) => {
            (0, keys(&delete.channels).group)
        }
        HotStoreAction::Insert(InsertAction::InsertData(insert)) => (1, key(&insert.channel)),
        HotStoreAction::Delete(DeleteAction::DeleteData(delete)) => (1, key(&delete.channel)),
        HotStoreAction::Insert(InsertAction::InsertJoins(insert)) => (2, key(&insert.channel)),
        HotStoreAction::Delete(DeleteAction::DeleteJoins(delete)) => (2, key(&delete.channel)),
    }
}

/// The operation with every channel group in ascending index order. The
/// Rholang normalizer orders the binds of a join by their channel first
/// (`ReceiveSortMatcher::sort_bind`), so two joins over the same channels
/// have one channel order.
fn canonical(op: Op) -> Op {
    let sorted = |mut channels: Vec<usize>| {
        channels.sort_unstable();
        channels
    };
    match op {
        Op::ReadContinuations(channels) => Op::ReadContinuations(sorted(channels)),
        Op::ViewContinuations(channels) => Op::ViewContinuations(sorted(channels)),
        Op::Consume {
            channels,
            body,
            persist,
            peek,
        } => Op::Consume {
            channels: sorted(channels),
            body,
            persist,
            peek,
        },
        Op::RetireMatch {
            channels,
            persistent,
        } => Op::RetireMatch {
            channels: sorted(channels),
            persistent,
        },
        Op::InstallContinuation { channels, body } => Op::InstallContinuation {
            channels: sorted(channels),
            body,
        },
        Op::InstallJoin { channel, join } => Op::InstallJoin {
            channel,
            join: sorted(join),
        },
        other => other,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// D-S1 (D-C2d, DR-96): the digest store exports each map in digest
    /// order, and the legacy store exports in the order of its hash shards.
    /// The checkpoint sorts the history keys, so the two exports give the
    /// same root, and the digest export reversed gives it too.
    #[test]
    fn export_root_equals_legacy_export_root(
        ops in prop::collection::vec(op().prop_map(canonical), 0..24),
    ) {
        let store = Store::new();
        let legacy = legacy();
        for op in &ops {
            apply(&store, &legacy, op)?;
        }
        let digest = store.changes(&free()).expect("the digest export");
        let positions: Vec<(u8, StoreKey)> = digest.iter().map(export_position).collect();
        prop_assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let oracle = legacy.changes_metered(&free()).expect("the legacy export");
        let history = empty_history();
        let root = export_root(history.as_ref(), digest.clone());
        prop_assert!(root.is_ok());
        prop_assert_eq!(&root, &export_root(history.as_ref(), oracle));
        let mut reversed = digest;
        reversed.reverse();
        prop_assert_eq!(&root, &export_root(history.as_ref(), reversed));
    }
}

/// D-S1 (D-C2d, DR-96): two cached groups that are permutations of each
/// other share one history projection, so a checkpoint rejects the export of
/// either store with the same error.
#[test]
fn permuted_groups_fail_both_exports_alike() {
    let store = Store::new();
    let legacy = legacy();
    for op in [Op::ViewContinuations(vec![2, 1]), Op::ViewContinuations(vec![1, 2])] {
        apply(&store, &legacy, &op).expect("the stores agree");
    }
    let history = empty_history();
    let digest = export_root(history.as_ref(), store.changes(&free()).expect("the digest export"));
    let oracle =
        export_root(history.as_ref(), legacy.changes_metered(&free()).expect("the legacy export"));
    assert!(digest.is_err());
    assert_eq!(digest, oracle);
    // D-C3 (D-S3, DR-97): the two groups are clean cold fills, so the dirty
    // export has no action and succeeds.
    let dirty = store.dirty_entries(&free()).expect("a free meter");
    let actions = dirty.actions(&free()).expect("a free meter");
    assert!(actions.is_empty());
    assert!(
        history
            .prepare_native_checkpoint_borrowed(&actions, &free())
            .is_ok()
    );
}

/// The observable state of a store: its entry counts and its export.
// Changed by D-C3 (D-S3, DR-97): the observable state includes the dirty
// keys, which decide the export.
// fn observed(store: &Store) -> ([usize; 5], Result<Vec<String>, RSpaceError>)
// {     (store.entry_counts(), store.changes(&free()).map(export_multiset))
// }
type Observed = ([usize; 5], Result<Vec<String>, RSpaceError>, Result<Vec<String>, RSpaceError>);

fn observed(store: &Store) -> Observed {
    (store.entry_counts(), store.changes(&free()).map(export_multiset), dirty_keys(store))
}

/// The keys of the dirty entries of the exported maps, in export order.
fn dirty_keys(store: &Store) -> Result<Vec<String>, RSpaceError> {
    let dirty = store.dirty_entries(&free())?;
    let mut keys = Vec::new();
    keys.extend(
        dirty
            .continuations
            .iter()
            .map(|entry| format!("continuations {:?}", entry.key)),
    );
    keys.extend(
        dirty
            .data
            .iter()
            .map(|entry| format!("data {:?}", entry.key)),
    );
    keys.extend(
        dirty
            .joins
            .iter()
            .map(|entry| format!("joins {:?}", entry.key)),
    );
    Ok(keys)
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
    // Changed by D-C2e (D-S1, DR-96): the publication takes the channel.
    // PutDatum(StoreKey, Datum<String>),
    PutDatum(String, StoreKey, Datum<String>),
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
        // Changed by D-C2e (D-S1, DR-96): the publication takes the channel.
        // ("put_datum", Call::PutDatum(key(&a), Datum::create(&a, "put".to_owned(), false))),
        (
            "put_datum",
            Call::PutDatum(a.clone(), key(&a), Datum::create(&a, "put".to_owned(), false)),
        ),
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
        // Changed by D-C2e (D-S1, DR-96): the publication takes the channel.
        // Call::PutDatum(key, datum) => store.put_datum(key, datum, meter),
        Call::PutDatum(name, key, datum) => store.put_datum(&name, key, datum, meter),
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
    // Changed by D-C2e (D-S1, DR-96): the publication takes the channel.
    // store
    //     .put_datum(key(&name), Datum::create(&name, "later".to_owned(), false),
    // &free())     .expect("a free meter");
    store
        .put_datum(&name, key(&name), Datum::create(&name, "later".to_owned(), false), &free())
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
        // Changed by D-C2e (D-S1, DR-96): the publication takes the channel.
        // _ => store.put_datum(key(name), Datum::create(name, "v".to_owned(), false), meter),
        _ => store.put_datum(name, key(name), Datum::create(name, "v".to_owned(), false), meter),
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

/// The orders of `items`.
fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut orders = Vec::new();
    for (index, first) in items.iter().enumerate() {
        let mut rest = items.to_vec();
        rest.remove(index);
        for mut order in permutations(&rest) {
            order.insert(0, *first);
            orders.push(order);
        }
    }
    orders
}

/// D-S1 (D-C2e, DR-96): the five maps of a store share one key limit. A cold
/// fill beyond the limit is rejected and changes nothing, in every order of
/// the fills, so the set of distinct keys decides the rejection. A restore
/// returns the count to the checkpoint.
#[test]
fn store_key_bound_rejects_deterministically() {
    let a = channel(0);
    let bb = channel(1);
    let pair = group(&[0, 1]);
    // Four cold fills of distinct keys in three maps.
    let fill = |store: &Store, step: usize| -> Result<(), RSpaceError> {
        match step {
            0 => store
                .data(&a, key(&a), &|| Ok(Vec::new()), &free())
                .map(drop),
            1 => store
                .data(&bb, key(&bb), &|| Ok(Vec::new()), &free())
                .map(drop),
            2 => store
                .joins(&a, key(&a), &|| Ok(Vec::new()), &free())
                .map(drop),
            _ => store
                .continuations(&pair, &keys(&pair), &|| Ok(Vec::new()), &free())
                .map(drop),
        }
    };
    let orders = permutations(&[0, 1, 2, 3]);
    assert_eq!(orders.len(), 24);
    for order in &orders {
        let store = Store::with_key_bound(3);
        for (index, step) in order.iter().enumerate() {
            let before = observed(&store);
            if index < 3 {
                assert_eq!(fill(&store, *step), Ok(()), "{order:?}");
            } else {
                assert_eq!(fill(&store, *step), Err(RSpaceError::HostWorkRejected), "{order:?}");
                assert_eq!(observed(&store), before, "{order:?}");
                assert_eq!(fill(&store, *step), Err(RSpaceError::HostWorkRejected), "{order:?}");
            }
        }
        assert_eq!(store.keys_used(), 3);
        assert_eq!(store.entry_counts().iter().sum::<usize>(), 3);
        fill(&store, order[0]).expect("a warm read adds no key");
        assert_eq!(store.keys_used(), 3);
    }
    let store = Store::with_key_bound(3);
    fill(&store, 0).expect("room for the first key");
    let checkpoint = store.snapshot();
    fill(&store, 1).expect("room for the second key");
    fill(&store, 2).expect("room for the third key");
    assert_eq!(fill(&store, 3), Err(RSpaceError::HostWorkRejected));
    store.restore(checkpoint);
    assert_eq!(store.keys_used(), 1);
    fill(&store, 3).expect("room after the restore");
    fill(&store, 1).expect("room after the restore");
    assert_eq!(fill(&store, 2), Err(RSpaceError::HostWorkRejected));
    assert_eq!(store.keys_used(), 3);
}

/// D-S1 (D-C2e, DR-96): two channels with one digest are a collision. The
/// test passes the key of one channel with another channel, which is what a
/// digest collision produces. Every lookup that finds the first channel's
/// entry for the second channel rejects the call and leaves the store
/// unchanged, including a key repeated inside one group.
#[test]
fn digest_collision_is_detected() {
    let collision =
        || Err(RSpaceError::InterpreterError("native store digest collision".to_owned()));
    let a = channel(0);
    let bb = channel(1);
    let ccc = channel(2);
    let single = group(&[0]);
    let store = Store::new();
    store
        .data(&a, key(&a), &|| Ok(history_data(&a)), &free())
        .expect("a free meter");
    store
        .joins(&a, key(&a), &|| Ok(history_joins(&a)), &free())
        .expect("a free meter");
    store
        .install_join(&a, key(&a), &group(&[0, 1]), &free())
        .expect("a free meter");
    store
        .install_continuation(
            &single,
            &keys(&single),
            waiting(&single, "installed", true, false),
            &free(),
        )
        .expect("a free meter");
    // A group [ccc, bb] with one stored continuation and its joins.
    let pair = group(&[2, 1]);
    store
        .continuations(&pair, &keys(&pair), &|| Ok(Vec::new()), &free())
        .expect("a free meter");
    for name in &pair {
        store
            .joins(name, key(name), &|| Ok(Vec::new()), &free())
            .expect("a free meter");
    }
    store
        .store_consume(&pair, &keys(&pair), waiting(&pair, "stored", false, false), &free())
        .expect("a free meter");
    let before = observed(&store);

    // `bb` with the digest of `a`, and the group [bb] with the digest of [a].
    let forged = key(&a);
    let lone = vec![bb.clone()];
    let forged_group = GroupKeys {
        channels: vec![forged],
        group: keys(&single).group,
    };
    let empty_data = || Ok(Vec::new());
    assert_eq!(store.data(&bb, forged, &empty_data, &free()).map(drop), collision());
    assert_eq!(store.data_view(&bb, forged, &empty_data, &free()).map(drop), collision());
    assert_eq!(
        store
            .joins(&bb, forged, &|| Ok(Vec::new()), &free())
            .map(drop),
        collision()
    );
    assert_eq!(
        store
            .continuations(&lone, &forged_group, &|| Ok(Vec::new()), &free())
            .map(drop),
        collision()
    );
    assert_eq!(
        store
            .continuation_views(&lone, &forged_group, &|| Ok(Vec::new()), &free())
            .map(drop),
        collision()
    );
    assert_eq!(
        store.put_datum(&bb, forged, Datum::create(&bb, "v".to_owned(), false), &free()),
        collision()
    );
    assert_eq!(
        store
            .store_consume(&lone, &forged_group, waiting(&lone, "w", false, false), &free())
            .map(drop),
        collision()
    );
    assert_eq!(store.install_join(&bb, forged, &lone, &free()), collision());
    assert_eq!(
        store.install_continuation(&lone, &forged_group, waiting(&lone, "i", true, false), &free()),
        collision()
    );
    let lone_result = vec![result_of("bb", "h3", false)];
    assert_eq!(store.retire_data(&lone_result, &[forged], &[(0, 0)], &free()), collision());
    assert_eq!(
        store.retire_produce_match(&lone, &forged_group, 0, false, &lone_result, &[], &free()),
        collision()
    );

    // A key repeated inside one group: the data of [a, bb] retired with the
    // digest of `a` twice compares the two channels.
    let both = vec![result_of("a", "h1", false), result_of("bb", "h3", false)];
    assert_eq!(
        store.retire_data(&both, &[forged, forged], &[(0, 0), (1, 0)], &free()),
        collision()
    );
    // The group [ccc, bb] with the digest of `ccc` twice: the consume and the
    // match retirement find the second channel in the first one's joins.
    let repeated = GroupKeys {
        channels: vec![key(&ccc), key(&ccc)],
        group: keys(&pair).group,
    };
    assert_eq!(
        store
            .store_consume(&pair, &repeated, waiting(&pair, "again", false, false), &free())
            .map(drop),
        collision()
    );
    let matched = vec![result_of("ccc", "x", true), result_of("bb", "y", true)];
    assert_eq!(
        store.retire_produce_match(&pair, &repeated, 0, false, &matched, &[], &free()),
        collision()
    );
    assert_eq!(observed(&store), before);
}

/// The history of the fake reads of these tests, committed to an empty
/// history: the data of "a" and "bb", the joins of "a", and the stored
/// continuation of ["a", "bb"].
fn fixture_history() -> Box<dyn HistoryRepository<String, String, String, String> + Send + Sync> {
    let history = empty_history();
    let a = channel(0);
    let bb = channel(1);
    let pair = group(&[0, 1]);
    let actions = vec![
        HotStoreAction::Insert(InsertAction::InsertData(InsertData {
            channel: a.clone(),
            data: history_data(&a),
        })),
        HotStoreAction::Insert(InsertAction::InsertData(InsertData {
            channel: bb.clone(),
            data: history_data(&bb),
        })),
        HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
            channel: a.clone(),
            joins: history_joins(&a),
        })),
        HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
            channels: pair.clone(),
            continuations: history_continuations(&pair),
        })),
    ];
    let prepared = history
        .prepare_native_checkpoint(actions, &free())
        .expect("the fixture checkpoint");
    history
        .commit_native_checkpoint(prepared)
        .expect("the fixture commit")
}

#[derive(Clone, Debug)]
enum Step {
    Op(Op),
    Checkpoint,
    Restore,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        8 => op().prop_map(canonical).prop_map(Step::Op),
        1 => Just(Step::Checkpoint),
        1 => Just(Step::Restore),
    ]
}

/// The roots of the full export and of the dirty export of `store` over
/// `history`. A checkpoint without history actions keeps the root.
fn export_roots(
    store: &Store,
    history: &(dyn HistoryRepository<String, String, String, String> + Send + Sync),
) -> (Result<Blake2b256Hash, RSpaceError>, Result<Blake2b256Hash, RSpaceError>) {
    let full = store
        .changes(&free())
        .and_then(|actions| history.prepare_native_checkpoint(actions, &free()))
        .map(|prepared| prepared.root.unwrap_or_else(|| history.root()));
    let dirty = store
        .dirty_entries(&free())
        .and_then(|dirty| {
            let actions = dirty.actions(&free())?;
            history.prepare_native_checkpoint_borrowed(&actions, &free())
        })
        .map(|prepared| prepared.root.unwrap_or_else(|| history.root()));
    (full, dirty)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// D-S3 (D-C3, DR-97): over a history that holds what the fake reads
    /// return, the dirty export and the full export give the same root after
    /// every step of random operations, checkpoints and restores.
    #[test]
    fn dirty_export_root_equals_full_export_root(steps in prop::collection::vec(step(), 0..24)) {
        let history = fixture_history();
        let store = Store::new();
        let legacy = legacy();
        let mut saved = None;
        for step in &steps {
            match step {
                Step::Op(op) => apply(&store, &legacy, op)?,
                Step::Checkpoint => saved = Some((store.snapshot(), legacy.snapshot())),
                Step::Restore => {
                    if let Some((digest, oracle)) = saved.take() {
                        store.restore(digest);
                        legacy.set_state(oracle);
                    }
                }
            }
            let (full, dirty) = export_roots(&store, history.as_ref());
            prop_assert!(full.is_ok(), "{:?}", full);
            prop_assert_eq!(full, dirty);
        }
    }
}

/// A meter that records every reservation in order.
#[derive(Default)]
struct Recording(RefCell<Vec<(usize, usize, usize)>>);

impl SourceMeter for Recording {
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), RSpaceError> {
        self.0.borrow_mut().push((operations, scanned, backing));
        Ok(())
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// D-S3 (D-C3, DR-97): owned actions and borrowed views with shared
    /// continuations encode the same leaves with the same reservations, give
    /// the same root, and the borrowed checkpoint allocates at most what it
    /// reserved.
    #[test]
    fn borrowed_export_encodes_identical_leaves(
        data in prop::collection::vec(("[a-c]{0,3}", any::<bool>()), 0..4),
        bodies in prop::collection::vec(("[a-c]{0,3}", any::<bool>(), any::<bool>()), 0..4),
        joins in prop::collection::vec(prop::collection::vec(0usize..3, 1..3), 0..3),
    ) {
        let a = channel(0);
        let bb = channel(1);
        let pair = group(&[0, 1]);
        let data: Vec<Datum<String>> = data
            .into_iter()
            .map(|(value, persist)| Datum::create(&a, value, persist))
            .collect();
        let continuations: Vec<Waiting> = bodies
            .into_iter()
            .map(|(body, persist, peek)| waiting(&pair, &body, persist, peek))
            .collect();
        let joins: Vec<Vec<String>> = joins.iter().map(|indices| group(indices)).collect();
        let shared: Vec<Arc<Waiting>> = continuations.iter().cloned().map(Arc::new).collect();
        let owned = vec![
            if data.is_empty() {
                HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData { channel: a.clone() }))
            } else {
                HotStoreAction::Insert(InsertAction::InsertData(InsertData {
                    channel: a.clone(),
                    data: data.clone(),
                }))
            },
            if continuations.is_empty() {
                HotStoreAction::Delete(DeleteAction::DeleteContinuations(DeleteContinuations {
                    channels: pair.clone(),
                }))
            } else {
                HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
                    channels: pair.clone(),
                    continuations: continuations.clone(),
                }))
            },
            if joins.is_empty() {
                HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins { channel: bb.clone() }))
            } else {
                HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
                    channel: bb.clone(),
                    joins: joins.clone(),
                }))
            },
        ];
        let views = [
            if data.is_empty() {
                NativeExportAction::DeleteData { channel: &a }
            } else {
                NativeExportAction::InsertData { channel: &a, data: &data }
            },
            if shared.is_empty() {
                NativeExportAction::DeleteContinuations { channels: &pair }
            } else {
                NativeExportAction::InsertContinuations { channels: &pair, continuations: &shared }
            },
            if joins.is_empty() {
                NativeExportAction::DeleteJoins { channel: &bb }
            } else {
                NativeExportAction::InsertJoins { channel: &bb, joins: &joins }
            },
        ];
        let owned_meter = Recording::default();
        let borrowed_meter = Recording::default();
        let from_owned =
            NativeCheckpoint::prepare(owned.clone(), &owned_meter).expect("a free meter");
        let from_views =
            NativeCheckpoint::prepare_borrowed(&views, &borrowed_meter).expect("a free meter");
        prop_assert_eq!(&from_owned.cold_actions, &from_views.cold_actions);
        prop_assert_eq!(
            format!("{:?}", from_owned.history_actions),
            format!("{:?}", from_views.history_actions)
        );
        prop_assert_eq!(owned_meter.0.into_inner(), borrowed_meter.0.into_inner());
        let history = empty_history();
        let owned_root = history
            .prepare_native_checkpoint(owned, &free())
            .map(|prepared| prepared.root);
        let meter = Meter::default();
        let (borrowed_root, allocated) = measure_allocations(|| {
            history
                .prepare_native_checkpoint_borrowed(&views, &meter)
                .map(|prepared| prepared.root)
        });
        prop_assert_eq!(owned_root, borrowed_root);
        prop_assert!(allocated <= meter.used.get()[2]);
    }
}

/// A store with dirty entries: `prepared()`, a publication on "a", and a
/// stored consume on ["a", "bb"].
fn dirty_store() -> Store {
    let store = prepared();
    let a = channel(0);
    store
        .put_datum(&a, key(&a), Datum::create(&a, "put".to_owned(), false), &free())
        .expect("a free meter");
    let pair = group(&[0, 1]);
    store
        .store_consume(&pair, &keys(&pair), waiting(&pair, "new", false, true), &free())
        .expect("a free meter");
    store
}

/// D-S3 (D-C3, DR-97): a restore returns the dirty flags of its checkpoint,
/// and a write that changes nothing keeps an entry clean.
#[test]
fn restore_restores_dirty_flags() {
    let store = prepared();
    let a = channel(0);
    let bb = channel(1);
    let pair = group(&[0, 1]);
    assert_eq!(dirty_keys(&store), Ok(Vec::new()));
    let clean = store.snapshot();
    store
        .put_datum(&a, key(&a), Datum::create(&a, "put".to_owned(), false), &free())
        .expect("a free meter");
    let data_a = vec![format!("data {:?}", Arc::new(a.clone()))];
    assert_eq!(dirty_keys(&store), Ok(data_a.clone()));
    let written = store.snapshot();
    store
        .store_consume(&pair, &keys(&pair), waiting(&pair, "new", false, true), &free())
        .expect("a free meter");
    let consumed = dirty_keys(&store).expect("a free meter");
    assert!(consumed.contains(&format!("continuations {:?}", Arc::new(pair.clone()))));
    assert!(consumed.contains(&format!("joins {:?}", Arc::new(bb.clone()))));
    // The joins of "a" already held the group, so the consume left them clean.
    assert!(!consumed.contains(&format!("joins {:?}", Arc::new(a.clone()))));
    store.restore(written);
    assert_eq!(dirty_keys(&store), Ok(data_a));
    let dirty = store.dirty_entries(&free()).expect("a free meter");
    assert_eq!(dirty.data.len(), 1);
    assert_eq!(dirty.data[0].value[0].a, "put");
    store.restore(clean);
    assert_eq!(dirty_keys(&store), Ok(Vec::new()));
}

/// D-S3 (D-C3, DR-97): the export by reference reserves before it
/// allocates: at every reservation cut it returns the host error, allocates
/// at most what it reserved, and leaves the store unchanged.
#[test]
fn every_dirty_export_cut_preserves_state() {
    let history = empty_history();
    let export = |store: &Store, meter: &Meter| -> Result<(), RSpaceError> {
        let dirty = store.dirty_entries(meter)?;
        let actions = dirty.actions(meter)?;
        history
            .prepare_native_checkpoint_borrowed(&actions, meter)
            .map(drop)
    };
    let baseline = Meter::default();
    export(&dirty_store(), &baseline).expect("a free meter");
    let before = observed(&dirty_store());
    for cut in 0..baseline.calls.get() {
        let store = dirty_store();
        let meter = Meter::rejecting(cut);
        let (result, allocated) = measure_allocations(|| export(&store, &meter));
        assert_eq!(result, Err(RSpaceError::HostWorkRejected), "cut {cut}");
        assert!(
            allocated <= meter.used.get()[2],
            "cut {cut}: allocated {allocated}, reserved {}",
            meter.used.get()[2]
        );
        assert_eq!(observed(&store), before, "cut {cut}");
    }
}

/// D-S3 (D-C3, DR-97), negative control: the dirty export keeps the root only
/// over the history that the cold fills read. Over an empty history, the
/// full export of the clean cold fills writes their values, and the dirty
/// export does not.
#[test]
fn dirty_export_needs_the_cold_fill_history() {
    let store = prepared();
    assert_eq!(dirty_keys(&store), Ok(Vec::new()));
    let (full, dirty) = export_roots(&store, empty_history().as_ref());
    assert_ne!(full.expect("the full export"), dirty.expect("the dirty export"));
    let (full, dirty) = export_roots(&store, fixture_history().as_ref());
    assert_eq!(full.expect("the full export"), dirty.expect("the dirty export"));
}
