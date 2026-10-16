//! D-S1 (D-C2c, DR-96): tests of the digest-keyed native store. Changed by
//! DR-131 S6: dev's hot store (`InMemHotStore`) is the oracle, because the
//! legacy native methods of that store are deleted with host work.

use std::collections::BTreeSet;

use proptest::prelude::*;

use super::*;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::channel_key;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::history::history_repository::{HistoryRepository, HistoryRepositoryInstances};
use crate::rspace::history::native_checkpoint::NativeCheckpoint;
use crate::rspace::hot_store_action::{
    DeleteAction, DeleteContinuations, DeleteData, DeleteJoins, InsertAction, InsertContinuations,
    InsertData, InsertJoins, NativeExportAction,
};
use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use crate::rspace::trace::event::Consume;

type Store = NativeHotStore<String, String, String, String>;
type Legacy = Box<dyn HotStore<String, String, String, String>>;
type Waiting = WaitingContinuation<String, String>;

/// Changed by DR-131 S6: the history of the oracle returns what the fake
/// reads of the native store return, because dev's hot store reads its own
/// history.
struct FixtureReader;

impl HistoryReaderBase<String, String, String, String> for FixtureReader {
    fn get_data_proj(&self, channel: &String) -> Vec<Datum<String>> { history_data(channel) }

    fn get_continuations_proj(&self, channels: &Vec<String>) -> Vec<Waiting> {
        history_continuations(channels)
    }

    fn get_joins_proj(&self, channel: &String) -> Vec<Vec<String>> { history_joins(channel) }
}

fn legacy() -> Legacy { HotStoreInstances::create_from_hr(Box::new(FixtureReader)) }

const CHANNELS: [&str; 3] = ["a", "bb", "ccc"];

fn channel(index: usize) -> String { CHANNELS[index % CHANNELS.len()].to_owned() }

fn group(indices: &[usize]) -> Vec<String> { indices.iter().map(|index| channel(*index)).collect() }

fn key(channel: &String) -> StoreKey { channel_key(channel).expect("a channel key") }

fn keys(channels: &[String]) -> GroupKeys { GroupKeys::build(channels).expect("group keys") }

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
    (store.data(name, key(name), &read), Ok(legacy.get_data(name)))
}

fn prefetch_group(store: &Store, legacy: &Legacy, channels: &[String]) {
    let read = || Ok(history_continuations(channels));
    let _ = store.continuation_views(channels, &keys(channels), &read);
    let _ = legacy.get_continuations(channels);
    for name in channels {
        let read = || Ok(history_joins(name));
        let _ = store.joins(name, key(name), &read);
        let _ = legacy.get_joins(name);
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

/// DR-131 S6: the oracle of `store_consume`. Dev's space stores a waiting
/// continuation with `put_continuation` and puts its join on each channel.
/// The depth counts the continuations of the group, the installed one
/// included.
fn store_consume_oracle(legacy: &Legacy, channels: &[String], value: Waiting) -> (bool, usize) {
    let inserted = legacy
        .put_continuation(channels, value)
        .expect("dev's put_continuation returns a value");
    for name in channels {
        legacy.put_join(name, channels);
    }
    (inserted, legacy.get_continuations(channels).len())
}

/// DR-131 S6: the check of a data retirement against copies of the data.
/// The native store retires all the data or none, so the oracle checks each
/// index before it removes any datum.
fn check_data_retirement(
    legacy: &Legacy,
    data: &[RSpaceResult<String, String>],
    retirement: &[(usize, i32)],
) -> Result<(), RSpaceError> {
    let mut staged: Vec<(&String, Vec<Datum<String>>)> = Vec::with_capacity(retirement.len());
    for (position, datum_index) in retirement {
        let name = &data[*position].channel;
        let found = staged
            .iter()
            .position(|(staged_name, _)| *staged_name == name);
        let values = match found {
            Some(found) => &mut staged[found].1,
            None => {
                staged.push((name, legacy.get_data(name)));
                &mut staged.last_mut().expect("a staged channel").1
            }
        };
        let Some(index) = usize::try_from(*datum_index)
            .ok()
            .filter(|index| *index < values.len())
        else {
            return Err(RSpaceError::InterpreterError(
                "native datum retirement index is invalid".to_owned(),
            ));
        };
        values.remove(index);
    }
    Ok(())
}

/// DR-131 S6: the oracle of `retire_data`, dev's `remove_datum` for each
/// retirement after the check.
fn retire_data_oracle(
    legacy: &Legacy,
    data: &[RSpaceResult<String, String>],
    retirement: &[(usize, i32)],
) -> Result<(), RSpaceError> {
    check_data_retirement(legacy, data, retirement)?;
    for (position, datum_index) in retirement {
        legacy.remove_datum(&data[*position].channel, *datum_index)?;
    }
    Ok(())
}

/// DR-131 S6: the oracle of `retire_produce_match`. It retires the data,
/// removes the matched continuation with dev's `remove_continuation` unless
/// the continuation persists, and calls dev's `remove_join` on each
/// channel, which removes the join only when the group has no continuation
/// left. The native store publishes all or nothing, so the oracle checks
/// the data and the continuation index before it changes the store.
fn retire_produce_match_oracle(
    legacy: &Legacy,
    channels: &[String],
    continuation_index: i32,
    continuation_persistent: bool,
    data: &[RSpaceResult<String, String>],
    retirement: &[(usize, i32)],
) -> Result<(), RSpaceError> {
    check_data_retirement(legacy, data, retirement)?;
    if !continuation_persistent &&
        legacy
            .remove_continuation(channels, continuation_index)
            .is_none()
    {
        return Err(RSpaceError::InterpreterError(
            "native continuation retirement index is invalid".to_owned(),
        ));
    }
    for (position, datum_index) in retirement {
        legacy.remove_datum(&data[*position].channel, *datum_index)?;
    }
    for name in channels {
        legacy.remove_join(name, channels);
    }
    Ok(())
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
            let digest = store.data_view(&name, key(&name), &read);
            prop_assert_eq!(digest.map(|view| view.values().to_vec()), Ok(legacy.get_data(&name)));
        }
        Op::ReadJoins(index) => {
            let name = channel(*index);
            let read = || Ok(history_joins(&name));
            prop_assert_eq!(store.joins(&name, key(&name), &read), Ok(legacy.get_joins(&name)));
        }
        Op::ReadContinuations(indices) => {
            let channels = group(indices);
            let read = || Ok(history_continuations(&channels));
            prop_assert_eq!(
                store.continuations(&channels, &keys(&channels), &read),
                Ok(legacy.get_continuations(&channels))
            );
        }
        Op::ViewContinuations(indices) => {
            let channels = group(indices);
            let read = || Ok(history_continuations(&channels));
            let digest = store.continuation_views(&channels, &keys(&channels), &read);
            let owned = |views: Vec<Arc<Waiting>>| {
                views
                    .iter()
                    .map(|view| view.as_ref().clone())
                    .collect::<Vec<_>>()
            };
            prop_assert_eq!(digest.map(owned), Ok(legacy.get_continuations(&channels)));
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
                store.store_consume(&channels, &keys(&channels), value.clone()),
                Ok(store_consume_oracle(legacy, &channels, value))
            );
        }
        Op::Put {
            channel: index,
            value,
            persist,
        } => {
            let name = channel(*index);
            let read = || Ok(history_data(&name));
            let _ = store.data_view(&name, key(&name), &read);
            let _ = legacy.get_data(&name);
            let datum = Datum::create(&name, format!("v{value}"), *persist);
            // Changed by D-C2e (D-S1, DR-96): the publication passes the
            // channel for the collision check.
            // prop_assert_eq!(
            //     store.put_datum(key(&name), datum.clone(), &free()),
            //     legacy.put_datum_metered(&name, datum, &free())
            // );
            prop_assert_eq!(store.put_datum(&name, key(&name), datum.clone()), Ok(()));
            legacy.put_datum(&name, datum);
        }
        Op::RetireData { channel: index } => {
            let channels = vec![channel(*index)];
            let (data, retired) = retirement(store, legacy, &channels);
            let channel_keys = vec![key(&channels[0])];
            prop_assert_eq!(
                store.retire_data(&data, &channel_keys, &retired),
                retire_data_oracle(legacy, &data, &retired)
            );
        }
        Op::RetireMatch {
            channels,
            persistent,
        } => {
            let channels = group(channels);
            prefetch_group(store, legacy, &channels);
            let count = legacy.get_continuations(&channels).len();
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
                    &retired
                ),
                retire_produce_match_oracle(legacy, &channels, index, *persistent, &data, &retired)
            );
        }
        Op::InstallContinuation { channels, body } => {
            let channels = group(channels);
            let value = waiting(&channels, &format!("installed-{body}"), true, false);
            prop_assert_eq!(
                store.install_continuation(&channels, &keys(&channels), value.clone()),
                Ok(())
            );
            legacy.install_continuation(&channels, value);
        }
        Op::InstallJoin {
            channel: index,
            join,
        } => {
            let name = channel(*index);
            let join = group(join);
            prop_assert_eq!(store.install_join(&name, key(&name), &join), Ok(()));
            legacy.install_join(&name, &join);
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
    /// returns what dev's store returns (reads, duplicate decisions and
    /// depths, publications, retirements, installations and their errors),
    /// and the two exports are the same multiset.
    #[test]
    fn digest_store_matches_legacy_store(ops in prop::collection::vec(op(), 0..24)) {
        let store = Store::new();
        let legacy = legacy();
        for op in &ops {
            apply(&store, &legacy, op)?;
        }
        prop_assert_eq!(export_multiset(store.changes()), export_multiset(legacy.changes()));
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
        .prepare_native_checkpoint(actions)
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
    /// order, and dev's store exports in the order of its hash shards. The
    /// checkpoint sorts the history keys, so the two exports give the same
    /// root, and the digest export reversed gives it too.
    #[test]
    fn export_root_equals_legacy_export_root(
        ops in prop::collection::vec(op().prop_map(canonical), 0..24),
    ) {
        let store = Store::new();
        let legacy = legacy();
        for op in &ops {
            apply(&store, &legacy, op)?;
        }
        let digest = store.changes();
        let positions: Vec<(u8, StoreKey)> = digest.iter().map(export_position).collect();
        prop_assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let oracle = legacy.changes();
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
    let digest = export_root(history.as_ref(), store.changes());
    let oracle = export_root(history.as_ref(), legacy.changes());
    assert!(digest.is_err());
    assert_eq!(digest, oracle);
    // D-C3 (D-S3, DR-97): the two groups are clean cold fills, so the dirty
    // export has no action and succeeds.
    let dirty = store.dirty_entries();
    let actions = dirty.actions();
    assert!(actions.is_empty());
    assert!(history.prepare_native_checkpoint_borrowed(&actions).is_ok());
}

/// The observable state of a store: its entry counts and its export.
// Changed by D-C3 (D-S3, DR-97): the observable state includes the dirty
// keys, which decide the export.
// fn observed(store: &Store) -> ([usize; 5], Result<Vec<String>, RSpaceError>)
// {     (store.entry_counts(), store.changes(&free()).map(export_multiset))
// }
type Observed = ([usize; 5], Vec<String>, Vec<String>);

fn observed(store: &Store) -> Observed {
    (store.entry_counts(), export_multiset(store.changes()), dirty_keys(store))
}

/// The keys of the dirty entries of the exported maps, in export order.
fn dirty_keys(store: &Store) -> Vec<String> {
    let dirty = store.dirty_entries();
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
    keys
}

/// A store with cached data, continuations and joins for the group
/// ["a", "bb"] and an installed continuation on ["ccc"].
fn prepared() -> Store {
    let store = Store::new();
    let channels = group(&[0, 1]);
    let read = || Ok(history_continuations(&channels));
    store
        .continuation_views(&channels, &keys(&channels), &read)
        .expect("a cold fill");
    for name in &channels {
        store
            .joins(name, key(name), &|| Ok(history_joins(name)))
            .expect("a cold fill");
        store
            .data_view(name, key(name), &|| Ok(history_data(name)))
            .expect("a cold fill");
    }
    let installed = group(&[2]);
    store
        .install_continuation(
            &installed,
            &keys(&installed),
            waiting(&installed, "installed", true, false),
        )
        .expect("an installation");
    store
}

fn result_of(name: &str, value: &str, persistent: bool) -> RSpaceResult<String, String> {
    RSpaceResult {
        channel: name.to_owned(),
        matched_datum: value.to_owned(),
        removed_datum: value.to_owned(),
        persistent,
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
        .data_view(&name, key(&name), &read)
        .expect("a warm read");
    let values = before.values().to_vec();
    // Changed by D-C2e (D-S1, DR-96): the publication takes the channel.
    // store
    //     .put_datum(key(&name), Datum::create(&name, "later".to_owned(), false),
    // &free())     .expect("a free meter");
    store
        .put_datum(&name, key(&name), Datum::create(&name, "later".to_owned(), false))
        .expect("a publication");
    assert_eq!(before.values(), values.as_slice());
    let after = store
        .data_view(&name, key(&name), &read)
        .expect("a warm read");
    assert_eq!(after.values().len(), values.len() + 1);
    assert_eq!(after.values()[0].a, "later");
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
        .data(&a, key(&a), &|| Ok(history_data(&a)))
        .expect("a cold fill");
    store
        .joins(&a, key(&a), &|| Ok(history_joins(&a)))
        .expect("a cold fill");
    store
        .install_join(&a, key(&a), &group(&[0, 1]))
        .expect("an installation");
    store
        .install_continuation(&single, &keys(&single), waiting(&single, "installed", true, false))
        .expect("an installation");
    // A group [ccc, bb] with one stored continuation and its joins.
    let pair = group(&[2, 1]);
    store
        .continuations(&pair, &keys(&pair), &|| Ok(Vec::new()))
        .expect("a cold fill");
    for name in &pair {
        store
            .joins(name, key(name), &|| Ok(Vec::new()))
            .expect("a cold fill");
    }
    store
        .store_consume(&pair, &keys(&pair), waiting(&pair, "stored", false, false))
        .expect("a stored consume");
    let before = observed(&store);

    // `bb` with the digest of `a`, and the group [bb] with the digest of [a].
    let forged = key(&a);
    let lone = vec![bb.clone()];
    let forged_group = GroupKeys {
        channels: vec![forged],
        group: keys(&single).group,
    };
    let empty_data = || Ok(Vec::new());
    assert_eq!(store.data(&bb, forged, &empty_data).map(drop), collision());
    assert_eq!(store.data_view(&bb, forged, &empty_data).map(drop), collision());
    assert_eq!(store.joins(&bb, forged, &|| Ok(Vec::new())).map(drop), collision());
    assert_eq!(
        store
            .continuations(&lone, &forged_group, &|| Ok(Vec::new()))
            .map(drop),
        collision()
    );
    assert_eq!(
        store
            .continuation_views(&lone, &forged_group, &|| Ok(Vec::new()))
            .map(drop),
        collision()
    );
    assert_eq!(
        store.put_datum(&bb, forged, Datum::create(&bb, "v".to_owned(), false)),
        collision()
    );
    assert_eq!(
        store
            .store_consume(&lone, &forged_group, waiting(&lone, "w", false, false))
            .map(drop),
        collision()
    );
    assert_eq!(store.install_join(&bb, forged, &lone), collision());
    assert_eq!(
        store.install_continuation(&lone, &forged_group, waiting(&lone, "i", true, false)),
        collision()
    );
    let lone_result = vec![result_of("bb", "h3", false)];
    assert_eq!(store.retire_data(&lone_result, &[forged], &[(0, 0)]), collision());
    assert_eq!(
        store.retire_produce_match(&lone, &forged_group, 0, false, &lone_result, &[]),
        collision()
    );

    // A key repeated inside one group: the data of [a, bb] retired with the
    // digest of `a` twice compares the two channels.
    let both = vec![result_of("a", "h1", false), result_of("bb", "h3", false)];
    assert_eq!(store.retire_data(&both, &[forged, forged], &[(0, 0), (1, 0)]), collision());
    // The group [ccc, bb] with the digest of `ccc` twice: the consume and the
    // match retirement find the second channel in the first one's joins.
    let repeated = GroupKeys {
        channels: vec![key(&ccc), key(&ccc)],
        group: keys(&pair).group,
    };
    assert_eq!(
        store
            .store_consume(&pair, &repeated, waiting(&pair, "again", false, false))
            .map(drop),
        collision()
    );
    let matched = vec![result_of("ccc", "x", true), result_of("bb", "y", true)];
    assert_eq!(store.retire_produce_match(&pair, &repeated, 0, false, &matched, &[]), collision());
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
        .prepare_native_checkpoint(actions)
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
    let full = history
        .prepare_native_checkpoint(store.changes())
        .map(|prepared| prepared.root.unwrap_or_else(|| history.root()));
    let dirty = store.dirty_entries();
    let actions = dirty.actions();
    let dirty = history
        .prepare_native_checkpoint_borrowed(&actions)
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// D-S3 (D-C3, DR-97): owned actions and borrowed views with shared
    /// continuations encode the same leaves and give the same root.
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
        let from_owned = NativeCheckpoint::prepare(owned.clone()).expect("the owned preparation");
        let from_views =
            NativeCheckpoint::prepare_borrowed(&views).expect("the borrowed preparation");
        prop_assert_eq!(&from_owned.cold_actions, &from_views.cold_actions);
        prop_assert_eq!(
            format!("{:?}", from_owned.history_actions),
            format!("{:?}", from_views.history_actions)
        );
        let history = empty_history();
        let owned_root = history
            .prepare_native_checkpoint(owned)
            .map(|prepared| prepared.root);
        let borrowed_root = history
            .prepare_native_checkpoint_borrowed(&views)
            .map(|prepared| prepared.root);
        prop_assert_eq!(owned_root, borrowed_root);
    }
}

/// D-S3 (D-C3, DR-97): a restore returns the dirty flags of its checkpoint,
/// and a write that changes nothing keeps an entry clean.
#[test]
fn restore_restores_dirty_flags() {
    let store = prepared();
    let a = channel(0);
    let bb = channel(1);
    let pair = group(&[0, 1]);
    assert_eq!(dirty_keys(&store), Vec::<String>::new());
    let clean = store.snapshot();
    store
        .put_datum(&a, key(&a), Datum::create(&a, "put".to_owned(), false))
        .expect("a publication");
    let data_a = vec![format!("data {:?}", Arc::new(a.clone()))];
    assert_eq!(dirty_keys(&store), data_a.clone());
    let written = store.snapshot();
    store
        .store_consume(&pair, &keys(&pair), waiting(&pair, "new", false, true))
        .expect("a stored consume");
    let consumed = dirty_keys(&store);
    assert!(consumed.contains(&format!("continuations {:?}", Arc::new(pair.clone()))));
    assert!(consumed.contains(&format!("joins {:?}", Arc::new(bb.clone()))));
    // The joins of "a" already held the group, so the consume left them clean.
    assert!(!consumed.contains(&format!("joins {:?}", Arc::new(a.clone()))));
    store.restore(written);
    assert_eq!(dirty_keys(&store), data_a);
    let dirty = store.dirty_entries();
    assert_eq!(dirty.data.len(), 1);
    assert_eq!(dirty.data[0].value[0].a, "put");
    store.restore(clean);
    assert_eq!(dirty_keys(&store), Vec::<String>::new());
}

/// D-S3 (D-C3, DR-97), negative control: the dirty export keeps the root only
/// over the history that the cold fills read. Over an empty history, the
/// full export of the clean cold fills writes their values, and the dirty
/// export does not.
#[test]
fn dirty_export_needs_the_cold_fill_history() {
    let store = prepared();
    assert_eq!(dirty_keys(&store), Vec::<String>::new());
    let (full, dirty) = export_roots(&store, empty_history().as_ref());
    assert_ne!(full.expect("the full export"), dirty.expect("the dirty export"));
    let (full, dirty) = export_roots(&store, fixture_history().as_ref());
    assert_eq!(full.expect("the full export"), dirty.expect("the dirty export"));
}
