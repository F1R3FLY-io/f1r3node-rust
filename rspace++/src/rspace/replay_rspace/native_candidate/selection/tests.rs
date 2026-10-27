use proptest::prelude::*;

use super::super::tests::Logical;
use super::*;
use crate::rspace::candidate_order::canonical_candidates;
use crate::rspace::history::native_reader::measure_allocations;
use crate::rspace::hot_store::native_store::NativeHotStore;
use crate::rspace::rspace::RSpace;
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

struct Matcher;

impl Match<u8, u8, u8> for Matcher {
    fn get(&self, pattern: &u8, value: &u8) -> Option<u8> {
        (*pattern == 0 || pattern == value).then_some(*value)
    }

    fn check_commit(&self, continuation: &u8, _: &[u8]) -> bool { *continuation != 255 }
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
    action: impl FnOnce(&CandidateReader<'_, u8, u8, u8, u8>) -> T,
) -> T {
    // Changed by D-C2c (D-S1, DR-96): the readers receive the keys; these
    // fixtures read the space's legacy store.
    // Changed by DR-131 S6: a new native store serves the values of the
    // legacy store, as the native session serves the values of the history.
    let store: NativeHotStore<u8, u8, u8, u8> = NativeHotStore::new();
    let data = |channel: &u8, key: StoreKey| {
        store.data_view(channel, key, &|| Ok(space.get_store().get_data(channel)))
    };
    let continuations = |channels: &[u8], keys: &GroupKeys| {
        store.continuation_views(channels, keys, &|| {
            Ok(space.get_store().get_continuations(channels))
        })
    };
    action(&CandidateReader {
        data: &data,
        continuations: &continuations,
    })
}

/// D-C2c (DR-96): the channel keys, computed before the work under test.
fn channel_keys(channels: &[u8]) -> Vec<StoreKey> {
    channels
        .iter()
        .map(|channel| {
            crate::rspace::hashing::native_source::channel_key(channel).expect("a channel key")
        })
        .collect()
}

/// D-C2c (DR-96): the keys of the join groups.
fn operation_keys(joins: &[Vec<u8>]) -> OperationKeys {
    OperationKeys::build(joins).expect("join keys")
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

/// D-D5 (DR-107) fixtures: the matched candidate of a produce selection and
/// its join group, or `None` when the selection gave the datum back.
fn matched<C, P: Clone + PartialEq, A: Clone, K: Clone + PartialEq>(
    selection: ProduceSelection<C, P, A, K>,
) -> Option<(PreparedProduceCandidate<C, P, A, K>, usize)> {
    match selection {
        ProduceSelection::Matched {
            prepared, group, ..
        } => Some((prepared, group)),
        ProduceSelection::Unmatched(_) => None,
    }
}

// Changed by D-D5 (DR-107): the comparison also serves the string space.
// fn same_data(left: &[ConsumeCandidate<u8, u8>], right: &[ConsumeCandidate<u8,
// u8>]) {
fn same_data<C: PartialEq + Debug, A: Clone + Serialize + PartialEq + Debug>(
    left: &[ConsumeCandidate<C, A>],
    right: &[ConsumeCandidate<C, A>],
) {
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

/// Changed by DR-131 S6: the test was
/// `counter_publication_prepays_growth_and_preserves_state_at_every_cut`.
/// The charge and cut checks went with host work. The test keeps its checks
/// of the publication, the persistent source and the overflow. The overflow
/// is an interpreter error, as in the produce selection.
#[tokio::test]
async fn counter_publication_preserves_state_until_publish() {
    let space = space().await;
    let source = Produce::create(&1u8, &7u8, false);
    let prepared = space
        .prepare_produce_counter(&source, false)
        .expect("counter preparation");
    drop(prepared);
    assert!(space.produce_counter.lock().unwrap().is_empty());
    space
        .prepare_produce_counter(&source, false)
        .expect("counter preparation")
        .publish();
    assert_eq!(space.produce_counter.lock().unwrap().get(&source), Some(&1));
    space.produce_counter.lock().unwrap().clear();
    space
        .prepare_produce_counter(&source, true)
        .expect("persistent preparation")
        .publish();
    assert!(space.produce_counter.lock().unwrap().is_empty());
    space
        .produce_counter
        .lock()
        .unwrap()
        .insert(source.clone(), i32::MAX);
    assert!(matches!(
        space.prepare_produce_counter(&source, false),
        Err(RSpaceError::InterpreterError(_))
    ));
    assert_eq!(space.produce_counter.lock().unwrap().get(&source), Some(&i32::MAX));
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
            let keys = channel_keys(&channels);
            // Changed by G3 (DR-135, S6): no owner checks.
            // let actual = with_reader(&space, |reader|
            // space.prepare_consume_candidate(&channels, &keys, &patterns, &9,
            // &source, &BTreeSet::new(), None, reader)).unwrap();
            let actual = with_reader(&space, |reader| space.prepare_consume_candidate(&channels, &keys, &patterns, &9, &source, &BTreeSet::new(), None, None, reader)).unwrap();
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
            let keys = operation_keys(&[channels.clone()]);
            // Changed by D-D5 (DR-107): the selection owns the incoming datum.
            // let actual = with_reader(&space, &meter, |reader| space.prepare_metered_produce_candidate(&1, &incoming, persistent, &source, vec![channels.clone()], &keys, None, reader)).unwrap().unwrap().0;
            // Changed by G3 (DR-135, S6): no owner checks.
            // let actual = with_reader(&space, |reader|
            // space.prepare_produce_candidate(&1, Datum::create(&1u8,
            // incoming, persistent), vec![channels.clone()], &keys, None,
            // reader)).map(matched).expect("candidate selection").expect("a
            // matched selection").0;
            let actual = with_reader(&space, |reader| space.prepare_produce_candidate(&1, Datum::create(&1u8, incoming, persistent), vec![channels.clone()], &keys, None, None, reader)).map(matched).expect("candidate selection").expect("a matched selection").0;
            same_data(&actual.candidate.data_candidates, &expected.candidate.data_candidates);
            for candidate in &actual.candidate.data_candidates {
                if candidate.datum_index == INCOMING_INDEX {
                    assert_eq!(candidate.removed_datum, incoming);
                }
            }
            assert_eq!(bincode::serialize(&actual.comm).unwrap(), bincode::serialize(&expected.comm).unwrap());
            let identity = Logical(expected.comm);
            // let selected = with_reader(&space, &meter, |reader| space.prepare_metered_produce_candidate(&1, &incoming, persistent, &source, vec![channels], &keys, Some(&identity), reader)).unwrap().unwrap().0;
            // Changed by G3 (DR-135, S6): no owner checks.
            // let selected = with_reader(&space, |reader|
            // space.prepare_produce_candidate(&1, Datum::create(&1u8,
            // incoming, persistent), vec![channels], &keys, Some(&identity),
            // reader)).map(matched).expect("candidate selection").expect("a
            // matched selection").0;
            let selected = with_reader(&space, |reader| space.prepare_produce_candidate(&1, Datum::create(&1u8, incoming, persistent), vec![channels], &keys, Some(&identity), None, reader)).map(matched).expect("candidate selection").expect("a matched selection").0;
            same_data(&selected.candidate.data_candidates, &actual.candidate.data_candidates);
            assert_eq!(state(&space), before);
        });
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

/// Changed by DR-131 S6: the test was
/// `produce_rolls_back_each_failed_probe_and_preserves_legacy_selection`.
/// The cut checks went with host work. The test keeps its check that the
/// selection after the failed probes equals the legacy selection.
#[tokio::test]
async fn produce_preserves_legacy_selection_after_failed_probes() {
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
    // Changed by D-D5 (DR-107): the selection owns the incoming datum.
    // let actual = with_reader(&space, &baseline, |reader| {
    //     space.prepare_metered_produce_candidate(
    //         &1,
    //         &6,
    //         false,
    //         &source,
    //         joins.clone(),
    //         &keys,
    //         None,
    //         reader,
    //     )
    // })
    // .unwrap()
    // .unwrap()
    // .0;
    let actual = with_reader(&space, |reader| {
        space.prepare_produce_candidate(
            &1,
            Datum::create(&1u8, 6u8, false),
            joins.clone(),
            &keys,
            None,
            // Added by G3 (DR-135, S6): no owner checks.
            None,
            reader,
        )
    })
    .map(matched)
    .expect("candidate selection")
    .expect("a matched selection")
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
    let actual = space.build_comm(&data, &consume, &BTreeSet::new(), None);
    assert_eq!(bincode::serialize(&actual).unwrap(), bincode::serialize(&expected).unwrap());
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
        let keys = channel_keys(&[1]);
        assert!(
            with_reader(&space, |reader| space.prepare_consume_candidate(
                &[1],
                &keys,
                &[0],
                &9,
                &source,
                &BTreeSet::new(),
                Some(&Logical(changed)),
                // Added by G3 (DR-135, S6): no owner checks.
                None,
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
    let input = vec![vec![1]];
    let keys = operation_keys(&input);
    // Changed by D-D5 (DR-107): the selection owns the incoming datum.
    // let (result, bytes) = measure_allocations(|| {
    //     with_reader(&space, &meter, |reader| {
    //         space.prepare_metered_produce_candidate(
    //             &1, &8, false, &trigger, input, &keys, None, reader,
    //         )
    //     })
    // });
    let incoming = Datum {
        a: 8u8,
        persist: false,
        source: trigger.clone(),
    };
    let result = with_reader(&space, |reader| {
        // Changed by G3 (DR-135, S6): no owner checks.
        // space.prepare_produce_candidate(&1, incoming, input, &keys, None,
        // reader)
        space.prepare_produce_candidate(&1, incoming, input, &keys, None, None, reader)
    });
    assert!(matches!(result, Err(RSpaceError::InterpreterError(_))));
    assert_eq!(state(&space), before);
}

/// D-M1 (DR-88) fixtures: a string space whose matcher reads only the first
/// byte of the pattern and the datum, and whose commit check reads nothing of
/// the continuation.
struct PrefixMatcher;

impl Match<String, String, String> for PrefixMatcher {
    fn get(&self, pattern: &String, value: &String) -> Option<String> {
        (pattern.as_bytes().first() == value.as_bytes().first()).then(|| value.clone())
    }

    /// Changed by DR-131 S6: the commit check reads nothing and copies
    /// nothing, as the deleted `check_commit_metered` of this fixture did.
    /// The default would copy the matched values, and the allocation checks
    /// of the selection would count those copies.
    fn check_commit_by_reference(&self, _: &String, _: &[&String]) -> bool { true }
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

/// D-D4 (DR-106) fixtures: `with_reader` for the string space.
fn with_string_reader<T>(
    space: &StringSpace,
    action: impl FnOnce(&CandidateReader<'_, String, String, String, String>) -> T,
) -> T {
    // Changed by DR-131 S6: a new native store serves the values of the
    // legacy store, as in `with_reader`.
    let store: NativeHotStore<String, String, String, String> = NativeHotStore::new();
    let data = |channel: &String, key: StoreKey| {
        store.data_view(channel, key, &|| Ok(space.get_store().get_data(channel)))
    };
    let continuations = |channels: &[String], keys: &GroupKeys| {
        store.continuation_views(channels, keys, &|| {
            Ok(space.get_store().get_continuations(channels))
        })
    };
    action(&CandidateReader {
        data: &data,
        continuations: &continuations,
    })
}

/// D-D4 (DR-106): a frozen copy of the selection before D-D4. For each
/// pattern it searched the channel data for the first entry equal to the
/// pattern's channel. It is the oracle of the selection tests. Changed by
/// DR-131 S6: the frozen copy lost its charges, and its selection did not
/// change.
fn legacy_match_data<C, P, A, K>(
    space: &ReplayRSpace<C, P, A, K>,
    channels: &[C],
    patterns: &[P],
    continuation: &K,
    data: &[ChannelData<'_, C, A>],
) -> Option<Vec<ConsumeCandidate<C, A>>>
where
    C: Clone + Eq,
    A: Clone,
{
    let count = channels.len().min(patterns.len());
    let mut chosen: Vec<(usize, usize)> = Vec::with_capacity(count);
    let mut candidates = Vec::with_capacity(count);
    let mut complete = true;
    for (channel, pattern) in channels.iter().zip(patterns) {
        let Some(channel_index) = channel_position(data, channel) else {
            complete = false;
            continue;
        };
        let mut found = false;
        for (position, (datum, index)) in data[channel_index].values.iter().enumerate() {
            if !datum.persist && chosen.contains(&(channel_index, position)) {
                continue;
            }
            let Some(matched) = space.matcher.get(pattern, &datum.a) else {
                continue;
            };
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
        return None;
    }
    let mut matched: Vec<&A> = Vec::with_capacity(candidates.len());
    for candidate in &candidates {
        matched.push(&candidate.datum.a);
    }
    if !space
        .matcher
        .check_commit_by_reference(continuation, &matched)
    {
        return None;
    }
    Some(candidates)
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
            // Changed by D-D5 (DR-107): the entries borrow the incoming datum.
            // let source = incoming.map(|(value, persistent)| {
            //     (value, persistent, Produce::create(&1u8, &value, persistent))
            // });
            let datum = incoming.map(|(value, persistent)| Datum::create(&1u8, value, persistent));
            with_reader(&space, |reader| {
                let views = space
                    .data_views(channels, &keys, reader)
                    .expect("data views");
                // let incoming = source
                //     .as_ref()
                //     .map(|(value, persistent, source)| (&1u8, value, *persistent, source));
                let incoming = datum.as_ref().map(|datum| (&1u8, datum));
                let (data, positions) = space.channel_data(channels, &views, incoming, None);
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
    /// datum and a failed commit. The values and the patterns come from a
    /// small range, so that about a fifth of the cases select data
    /// (measured: 27 of 128).
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
            let actual = space
                // Changed by G3 (DR-135, S6): no owner checks.
                // .match_data(&channels, positions, &patterns, &continuation, data)
                .match_data(&channels, positions, &patterns, &continuation, data, None)
                .expect("current selection");
            let expected = legacy_match_data(space, &channels, &patterns, &continuation, data);
            assert_eq!(actual.is_some(), expected.is_some());
            // Changed by D-D5 (DR-107): the selection leaves the removed value
            // of an incoming candidate for the produce preparation to fill.
            // if let (Some(actual), Some(expected)) = (&actual, &expected) {
            //     same_data(actual, expected);
            // }
            if let (Some(mut actual), Some(expected)) = (actual, &expected) {
                for candidate in &mut actual {
                    if candidate.datum_index == INCOMING_INDEX {
                        assert_eq!(candidate.removed_datum, u8::default());
                        candidate.removed_datum = incoming.expect("an incoming datum").0;
                    }
                }
                same_data(&actual, expected);
            }
        });
    }
}

/// Changed by DR-131 S6: the test was
/// `repeated_channel_consume_accepts_exact_credit_and_rejects_each_smaller_dimension`.
/// The credit checks went with host work. The test keeps its check that the
/// consume selection over interleaved repeated channels selects one datum
/// for each pattern.
#[tokio::test]
async fn repeated_channel_consume_selects_one_datum_for_each_pattern() {
    let space = space().await;
    for (channel, value) in [(1, 7), (2, 8), (1, 9), (2, 10)] {
        put(&space, channel, value, false);
    }
    let channels = [1, 2, 1, 2];
    let patterns = [0, 0, 0, 0];
    let source = Consume::create(&channels.to_vec(), &patterns.to_vec(), &9u8, false);
    let keys = channel_keys(&channels);
    let peeks = BTreeSet::new();
    let selection = with_reader(&space, |reader| {
        space.prepare_consume_candidate(
            // Changed by G3 (DR-135, S6): no owner checks.
            // &channels, &keys, &patterns, &9, &source, &peeks, None, reader,
            &channels, &keys, &patterns, &9, &source, &peeks, None, None, reader,
        )
    })
    .expect("consume selection")
    .expect("a candidate");
    assert_eq!(selection.data.len(), 4);
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
        space.match_data(
            std::slice::from_ref(&channel),
            positions,
            &["a".to_string()],
            &"k".to_string(),
            &data,
            // Added by G3 (DR-135, S6): no owner checks.
            None,
        )
    };
    assert!(matches!(run(&[0]), Ok(Some(_))));
    for positions in [&[][..], &[0, 0], &[1]] {
        assert!(matches!(run(positions), Err(RSpaceError::BugFoundError(_))), "{positions:?}");
    }
}

/// D-D5 (DR-107) fixtures: the keys of string join groups.
fn string_operation_keys(joins: &[Vec<String>]) -> OperationKeys {
    OperationKeys::build(joins).expect("join keys")
}

/// D-D5 (DR-107) fixtures: a waiting continuation on string channels.
fn string_waiting(space: &StringSpace, channels: &[String], patterns: Vec<String>) -> Consume {
    let continuation = "k".to_string();
    let source = Consume::create(&channels.to_vec(), &patterns, &continuation, false);
    space
        .get_store()
        .put_continuation(channels, WaitingContinuation {
            patterns,
            continuation,
            persist: false,
            peeks: BTreeSet::new(),
            source: source.clone(),
        });
    source
}

/// D-D5 (DR-107) fixtures: `state` for the string space.
fn string_state(space: &StringSpace) -> (Vec<u8>, String, String, String, i64) {
    let mut rows: Vec<_> = space
        .get_store()
        .to_map()
        .into_iter()
        .map(|(key, row)| (format!("{key:?}"), format!("{row:?}")))
        .collect();
    rows.sort();
    (
        bincode::serialize(&*space.produce_counter.lock().expect("produce counter lock"))
            .expect("counter encoding"),
        format!("{rows:?}"),
        format!("{:?}", space.event_log.lock().expect("event log lock")),
        format!("{:?}", space.replay_data.lock().expect("replay data lock")),
        space
            .replay_waiting_continuations_estimate
            .load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// D-D5 (DR-107): a frozen copy of `channel_data` before D-D5 (at
/// 5c101408b). The entry of the produced channel owned a copy of the
/// incoming value and of its source. Changed by DR-131 S6: the frozen
/// copies of the produce selection lost their charges and their copy
/// counts, and their selections did not change.
fn legacy_channel_data<'v, C, P, A, K>(
    space: &ReplayRSpace<C, P, A, K>,
    channels: &'v [C],
    views: &'v [NativeDataView<C, A>],
    incoming: Option<(&C, &A, bool, &Produce)>,
    expected: Option<&dyn NativeCandidateIdentity>,
) -> (Vec<ChannelData<'v, C, A>>, Vec<usize>)
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
{
    let mut result = Vec::with_capacity(channels.len());
    let mut positions = Vec::with_capacity(channels.len());
    for (channel, view) in channels.iter().zip(views) {
        let mut borrowed = Vec::with_capacity(view.values().len());
        for datum in view.values() {
            borrowed.push(Cow::Borrowed(datum));
        }
        let mut values = canonical_candidates(borrowed);
        if let Some((trigger, data, persist, source)) = incoming {
            if channel == trigger {
                let mut all = Vec::with_capacity(values.len() + 1);
                all.push((
                    Cow::Owned(Datum {
                        a: data.clone(),
                        persist,
                        source: source.clone(),
                    }),
                    -1,
                ));
                all.extend(values);
                values = all;
            }
        }
        let mut position = 0;
        while position < values.len() {
            let source = incoming.and_then(|(_, _, persist, source)| (!persist).then_some(source));
            if space.matches_identity(&values[position].0, expected, source) {
                position += 1;
            } else {
                values.remove(position);
            }
        }
        match channel_position(&result, channel) {
            Some(position) => {
                result[position].values = values;
                positions.push(position);
            }
            None => {
                positions.push(result.len());
                result.push(ChannelData { channel, values });
            }
        }
    }
    (result, positions)
}

/// D-D5 (DR-107): a frozen copy of `match_data` before D-D5 (at
/// 5c101408b). Each pushed candidate copied its removed value, also a
/// candidate of the incoming datum.
fn legacy_positioned_match_data<C, P, A, K>(
    space: &ReplayRSpace<C, P, A, K>,
    channels: &[C],
    positions: &[usize],
    patterns: &[P],
    continuation: &K,
    data: &[ChannelData<'_, C, A>],
) -> Result<Option<Vec<ConsumeCandidate<C, A>>>>
where
    C: Clone + Eq,
    A: Clone,
{
    if positions.len() != channels.len() {
        return Err(RSpaceError::BugFoundError(
            "frozen channel positions do not match the channels".to_owned(),
        ));
    }
    let count = channels.len().min(patterns.len());
    let mut chosen: Vec<(usize, usize)> = Vec::with_capacity(count);
    let mut candidates = Vec::with_capacity(count);
    let mut complete = true;
    for ((channel, pattern), recorded) in channels.iter().zip(patterns).zip(positions) {
        let channel_index = *recorded;
        let entry = data.get(channel_index).ok_or_else(|| {
            RSpaceError::BugFoundError(
                "a frozen channel position is outside the channel data".to_owned(),
            )
        })?;
        let mut found = false;
        for (position, (datum, index)) in entry.values.iter().enumerate() {
            if !datum.persist && chosen.contains(&(channel_index, position)) {
                continue;
            }
            let Some(matched) = space.matcher.get(pattern, &datum.a) else {
                continue;
            };
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
    let mut matched: Vec<&A> = Vec::with_capacity(candidates.len());
    for candidate in &candidates {
        matched.push(&candidate.datum.a);
    }
    if !space
        .matcher
        .check_commit_by_reference(continuation, &matched)
    {
        return Ok(None);
    }
    Ok(Some(candidates))
}

/// D-D5 (DR-107): a frozen copy of `prepare_produce_candidate`
/// before D-D5 (at 5c101408b), over the frozen channel data and selection.
#[allow(clippy::too_many_arguments)]
fn legacy_prepare_produce<C, P, A, K>(
    space: &ReplayRSpace<C, P, A, K>,
    channel: &C,
    value: &A,
    persist: bool,
    source: &Produce,
    grouped_channels: Vec<Vec<C>>,
    keys: &OperationKeys,
    expected: Option<&dyn NativeCandidateIdentity>,
    reader: &CandidateReader<'_, C, P, A, K>,
) -> Result<Option<(PreparedProduceCandidate<C, P, A, K>, usize)>>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
{
    if keys.groups.len() != grouped_channels.len() {
        return Err(RSpaceError::BugFoundError(
            "frozen join keys do not match the joins".to_owned(),
        ));
    }
    let next = if persist {
        None
    } else {
        let count = space.get_produce_count(source);
        let Some(next) = count.checked_add(1) else {
            let message = "Native replay produce counter overflow";
            return Err(RSpaceError::InterpreterError(message.to_owned()));
        };
        Some(next)
    };
    for (group, (channels, group_keys)) in
        grouped_channels.into_iter().zip(&keys.groups).enumerate()
    {
        let continuations = canonical_candidates((reader.continuations)(&channels, group_keys)?);
        let views = space.data_views(&channels, &group_keys.channels, reader)?;
        let (data, positions) = legacy_channel_data(
            space,
            &channels,
            &views,
            Some((channel, value, persist, source)),
            expected,
        );
        for (continuation, index) in continuations {
            if let Some(identity) = expected {
                if !identity.matches_consume(&continuation.source) {
                    continue;
                }
            }
            // Added by DR-129 (E3-a): the frozen selection mirrors the Stored
            // check of the live selection, which skips a continuation whose
            // commit can veto.
            if expected.is_none() && space.matcher.commit_may_veto(&continuation.continuation) {
                continue;
            }
            let Some(data_candidates) = legacy_positioned_match_data(
                space,
                &channels,
                &positions,
                &continuation.patterns,
                &continuation.continuation,
                &data,
            )?
            else {
                continue;
            };
            let comm = space.build_comm(
                &data_candidates,
                &continuation.source,
                &continuation.peeks,
                next.map(|next| (source, next)),
            );
            if let Some(identity) = expected {
                if !identity.matches_comm(&comm) {
                    return Ok(None);
                }
            }
            let continuation = continuation.as_ref().clone();
            let candidate = ProduceCandidate {
                channels,
                continuation,
                continuation_index: index,
                data_candidates,
            };
            return Ok(Some((PreparedProduceCandidate { candidate, comm }, group)));
        }
    }
    Ok(None)
}

/// D-D5 (DR-107) fixtures: one produce selection of `value` on the string
/// channel "c" against one waiting continuation with `patterns`. Returns
/// the selection, the allocated bytes and the address of the incoming
/// value.
async fn string_selection(
    value: String,
    persist: bool,
    patterns: &[&str],
) -> (ProduceSelection<String, String, String, String>, usize, usize) {
    let space = string_space().await;
    let channel = "c".to_string();
    let channels = vec![channel.clone(); patterns.len()];
    string_waiting(&space, &channels, patterns.iter().map(|pattern| pattern.to_string()).collect());
    let joins = vec![channels];
    let keys = string_operation_keys(&joins);
    warm_string_selection(&space, &joins, &keys, &value, persist);
    let incoming = Datum::create(&channel, value, persist);
    let address = incoming.a.as_ptr() as usize;
    let (selection, bytes) = measure_allocations(|| {
        with_string_reader(&space, |reader| {
            // Changed by G3 (DR-135, S6): no owner checks.
            // space.prepare_produce_candidate(&channel, incoming, joins,
            // &keys, None, reader)
            space.prepare_produce_candidate(&channel, incoming, joins, &keys, None, None, reader)
        })
    });
    (selection.expect("candidate selection"), bytes, address)
}

/// D-D5 (DR-107; `IncomingDatumMove.moved_charge_le_legacy`): the selection
/// borrows the incoming datum. A value of 65,536 bytes allocates exactly
/// what a value of 1 byte does, except the binding that the matcher makes
/// of a matched value. The matched removed value is the incoming value
/// itself.
#[tokio::test]
async fn incoming_datum_is_borrowed_during_selection() {
    let long = 65_536;
    let small = string_selection("z".to_string(), false, &["a"]).await;
    let large = string_selection(format!("z{}", "x".repeat(long - 1)), false, &["a"]).await;
    assert!(matches!(small.0, ProduceSelection::Unmatched(_)));
    assert!(matches!(large.0, ProduceSelection::Unmatched(_)));
    assert_eq!(small.1, large.1);
    let small = string_selection("a".to_string(), false, &["a"]).await;
    let large = string_selection(format!("a{}", "x".repeat(long - 1)), false, &["a"]).await;
    assert_eq!(large.1 - small.1, long - 1);
    let ProduceSelection::Matched { prepared, .. } = large.0 else {
        panic!("the pattern matches the value");
    };
    let removed = &prepared.candidate.data_candidates[0].removed_datum;
    assert_eq!(prepared.candidate.data_candidates[0].datum_index, INCOMING_INDEX);
    assert_eq!(removed.len(), long);
    assert_eq!(removed.as_ptr() as usize, large.2);
}

/// D-D5 (DR-107): the bytes that one copy of `value` allocates.
fn copy_allocation<T: Clone>(value: &T) -> usize { measure_allocations(|| value.clone()).1 }

/// D-D5 (DR-107) fixtures: one selection of the string channel "c". The
/// first selection on a space fills the caches of the legacy store, so the
/// allocation comparisons run warm.
fn warm_string_selection(
    space: &StringSpace,
    joins: &[Vec<String>],
    keys: &OperationKeys,
    value: &str,
    persist: bool,
) {
    let channel = "c".to_string();
    with_string_reader(space, |reader| {
        space.prepare_produce_candidate(
            &channel,
            Datum::create(&channel, value.to_string(), persist),
            joins.to_vec(),
            keys,
            None,
            // Added by G3 (DR-135, S6): no owner checks.
            None,
            reader,
        )
    })
    .expect("warm-up selection");
}

/// D-D5 (DR-107; `IncomingDatumMove.fill_without_incoming_moves_nothing`):
/// a produce that matches nothing, or whose match the replay identity
/// rejects, gets its own datum back, value and source, to store it.
#[tokio::test]
async fn stored_produce_stores_the_incoming_datum() {
    for persist in [false, true] {
        for rejected_identity in [false, true] {
            let space = string_space().await;
            let channel = "c".to_string();
            let pattern = if rejected_identity { "a" } else { "b" };
            string_waiting(&space, std::slice::from_ref(&channel), vec![pattern.to_string()]);
            let joins = vec![vec![channel.clone()]];
            let keys = string_operation_keys(&joins);
            warm_string_selection(&space, &joins, &keys, &"a-value".repeat(64), persist);
            let incoming = Datum::create(&channel, "a-value".repeat(64), persist);
            let identity = rejected_identity.then(|| {
                let play = space
                    .prepare_native_produce_candidate(
                        &channel,
                        &incoming.a,
                        persist,
                        &incoming.source,
                        joins.clone(),
                        None,
                    )
                    .expect("play selection")
                    .expect("a play match");
                let mut changed = play.comm;
                changed.peeks.insert(0);
                Logical(changed)
            });
            let encoded = bincode::serialize(&incoming).expect("datum encoding");
            let address = incoming.a.as_ptr() as usize;
            let before = string_state(&space);
            let selection = with_string_reader(&space, |reader| {
                space.prepare_produce_candidate(
                    &channel,
                    incoming,
                    joins.clone(),
                    &keys,
                    identity
                        .as_ref()
                        .map(|identity| identity as &dyn NativeCandidateIdentity),
                    // Added by G3 (DR-135, S6): no owner checks.
                    None,
                    reader,
                )
            })
            .expect("candidate selection");
            let ProduceSelection::Unmatched(returned) = selection else {
                panic!("persist {persist}, rejected identity {rejected_identity}: no match");
            };
            assert_eq!(returned.a.as_ptr() as usize, address);
            assert_eq!(bincode::serialize(&returned).expect("datum encoding"), encoded);
            assert_eq!(string_state(&space), before);
        }
    }
}

/// D-D5 (DR-107; `IncomingDatumMove.incoming_fill_equals_copies`,
/// `fill_moves_once`, `fill_copies_further`): every incoming candidate holds
/// the produce's value as its removed value. The first holds the value
/// itself and each further one a copy, as play gives each one a copy.
#[tokio::test]
async fn incoming_removed_datum_is_the_produce_value() {
    let value = format!("a{}", "y".repeat(4_095));
    for (persist, patterns) in [(false, vec!["a"]), (true, vec!["a", "a", "a"])] {
        let (selection, _, address) = string_selection(value.clone(), persist, &patterns).await;
        let ProduceSelection::Matched { prepared, .. } = selection else {
            panic!("the patterns match the value");
        };
        let candidates = &prepared.candidate.data_candidates;
        assert_eq!(candidates.len(), patterns.len());
        for (position, candidate) in candidates.iter().enumerate() {
            assert_eq!(candidate.datum_index, INCOMING_INDEX);
            assert_eq!(candidate.removed_datum, value);
            assert_eq!(candidate.removed_datum.as_ptr() as usize == address, position == 0);
        }
        let space = string_space().await;
        let channel = "c".to_string();
        let channels = vec![channel.clone(); patterns.len()];
        string_waiting(
            &space,
            &channels,
            patterns.iter().map(|pattern| pattern.to_string()).collect(),
        );
        let source = Produce::create(&channel, &value, persist);
        let play = space
            .prepare_native_produce_candidate(
                &channel,
                &value,
                persist,
                &source,
                vec![channels],
                None,
            )
            .expect("play selection")
            .expect("a play match");
        same_data(candidates, &play.candidate.data_candidates);
    }
}

/// D-D5 (DR-107): a replay identity can select stored data without the
/// produce's own datum. The selection then holds no incoming candidate, and
/// the value is dropped, as before.
#[tokio::test]
async fn matched_selection_without_the_trigger_drops_the_value() {
    let space = space().await;
    put(&space, 1, 7, false);
    let source = waiting(&space, &[1], vec![0], 9);
    let consumed = space
        .prepare_native_consume_candidate(&[1], &[0], &9, &source, &BTreeSet::new(), None)
        .expect("a play consume");
    let identity = Logical(consumed.comm);
    let trigger = Produce::create(&1u8, &8u8, false);
    let keys = operation_keys(&[vec![1]]);
    let selection = with_reader(&space, |reader| {
        space.prepare_produce_candidate(
            &1,
            Datum::create(&1u8, 8u8, false),
            vec![vec![1]],
            &keys,
            Some(&identity),
            // Added by G3 (DR-135, S6): no owner checks.
            None,
            reader,
        )
    })
    .expect("candidate selection");
    let ProduceSelection::Matched {
        prepared, source, ..
    } = selection
    else {
        panic!("the identity selects the stored datum");
    };
    assert_eq!(source, trigger);
    let candidates = &prepared.candidate.data_candidates;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].datum.a, 7);
    assert_eq!(candidates[0].removed_datum, 7);
    assert!(candidates[0].datum_index >= 0);
    assert!(!prepared.comm.produces.contains(&trigger));
}

/// Changed by DR-131 S6: the test was
/// `fill_incoming_charges_one_pass_and_each_further_copy`. The charge and
/// cut checks went with host work. The test keeps its checks that the fill
/// allocates exactly one copy for each incoming candidate after the first,
/// moves the value into the first incoming candidate and leaves stored
/// candidates unchanged (D-D5, DR-107;
/// `IncomingDatumMove.fill_trace_reserved`).
#[test]
fn fill_incoming_moves_the_value_once_and_copies_it_for_each_further_candidate() {
    let value = "v".repeat(4_096);
    let source = Produce::create(&"c".to_string(), &value, true);
    let build = |indices: &[i32]| -> Vec<ConsumeCandidate<String, String>> {
        indices
            .iter()
            .map(|index| ConsumeCandidate {
                channel: "c".to_string(),
                datum: Datum {
                    a: value.clone(),
                    persist: true,
                    source: source.clone(),
                },
                removed_datum: if *index == INCOMING_INDEX {
                    String::default()
                } else {
                    format!("stored-{index}")
                },
                datum_index: *index,
            })
            .collect()
    };
    for indices in [&[][..], &[0, 1], &[-1], &[-1, 0, -1, -1], &[2, -1, -1]] {
        let incoming = indices
            .iter()
            .filter(|index| **index == INCOMING_INDEX)
            .count();
        let further = incoming.saturating_sub(1);
        let mut candidates = build(indices);
        let moved = value.clone();
        let address = moved.as_ptr() as usize;
        let ((), bytes) = measure_allocations(|| fill_incoming(&mut candidates, moved));
        assert_eq!(bytes, further * copy_allocation(&value), "{indices:?}");
        let mut first = true;
        for (candidate, index) in candidates.iter().zip(indices) {
            if *index == INCOMING_INDEX {
                assert_eq!(candidate.removed_datum, value);
                assert_eq!(candidate.removed_datum.as_ptr() as usize == address, first);
                first = false;
            } else {
                assert_eq!(candidate.removed_datum, format!("stored-{index}"));
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// D-D5 (DR-107; `IncomingDatumMove.incoming_fill_equals_copies`): the
    /// produce selection equals the frozen legacy selection on repeated
    /// channels, several join groups, persistent and stored data, peeks,
    /// failed commits and replay identities. The selection returns the
    /// datum or the source.
    /// Wildcard patterns are three times as likely as the others, so that
    /// 57 of 129 cases select data (measured with the stored regression
    /// case: 10 with two or more incoming candidates, 4 with none, and 25
    /// under a replay identity).
    #[test]
    fn produce_selection_matches_frozen_legacy_produce(
        joins in prop::sample::subsequence(
            vec![vec![1u8], vec![1, 1], vec![1, 2], vec![2, 1], vec![1, 1, 2]],
            1..=3,
        ),
        stored in prop::collection::vec((1u8..3, 1u8..4, any::<bool>()), 0..8),
        waiting_specs in prop::collection::vec(
            (
                0usize..3,
                prop::collection::vec(prop_oneof![3 => Just(0u8), 1 => 1u8..4], 3),
                prop::bool::weighted(0.75),
                any::<bool>(),
            ),
            0..4,
        ),
        value in 1u8..4,
        persist in any::<bool>(),
        with_identity in any::<bool>(),
    ) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(async {
                let space = space().await;
                for (channel, stored_value, persistent) in &stored {
                    put(&space, *channel, *stored_value, *persistent);
                }
                for (group, patterns, commits, peek) in &waiting_specs {
                    let Some(channels) = joins.get(*group) else {
                        continue;
                    };
                    let patterns = patterns[..channels.len()].to_vec();
                    let body = if *commits { 9u8 } else { 255u8 };
                    let consume = Consume::create(channels, &patterns, &body, false);
                    let peeks = if *peek { BTreeSet::from([0]) } else { BTreeSet::new() };
                    space.get_store().put_continuation(channels, WaitingContinuation {
                        patterns,
                        continuation: body,
                        persist: false,
                        peeks,
                        source: consume,
                    });
                }
                let source = Produce::create(&1u8, &value, persist);
                let play = space
                    .prepare_native_produce_candidate(&1, &value, persist, &source, joins.clone(), None)
                    .expect("play selection");
                let identity = with_identity
                    .then(|| play.as_ref().map(|play| Logical(play.comm.clone())))
                    .flatten();
                let identity = identity.as_ref().map(|identity| identity as &dyn NativeCandidateIdentity);
                let keys = operation_keys(&joins);
                let before = state(&space);
                let expected = with_reader(&space, |reader| {
                    legacy_prepare_produce(
                        &space, &1, &value, persist, &source, joins.clone(), &keys, identity, reader,
                    )
                })
                .expect("legacy selection");
                let selection = with_reader(&space, |reader| {
                    space.prepare_produce_candidate(
                        &1,
                        Datum::create(&1u8, value, persist),
                        joins.clone(),
                        &keys,
                        identity,
                        // Added by G3 (DR-135, S6): no owner checks.
                        None,
                        reader,
                    )
                })
                .expect("candidate selection");
                match (selection, expected) {
                    (ProduceSelection::Matched { prepared, group, source: returned }, Some((expected, expected_group))) => {
                        assert_eq!(group, expected_group);
                        assert_eq!(returned, source);
                        assert_eq!(
                            bincode::serialize(&returned).expect("source encoding"),
                            bincode::serialize(&source).expect("source encoding")
                        );
                        let candidate = &prepared.candidate;
                        assert_eq!(candidate.continuation_index, expected.candidate.continuation_index);
                        assert_eq!(
                            bincode::serialize(&candidate.continuation).expect("continuation encoding"),
                            bincode::serialize(&expected.candidate.continuation).expect("continuation encoding")
                        );
                        same_data(&candidate.data_candidates, &expected.candidate.data_candidates);
                        // The oracle is the frozen replay selection, not the
                        // play selection: after a continuation that fails its
                        // commit matched a channel twice, play and the replay
                        // selection can choose other data (pgmcp bug 10137,
                        // before and after D-D5).
                        assert_eq!(
                            bincode::serialize(&prepared.comm).expect("comm encoding"),
                            bincode::serialize(&expected.comm).expect("comm encoding")
                        );
                    }
                    (ProduceSelection::Unmatched(returned), None) => {
                        assert_eq!(
                            bincode::serialize(&returned).expect("datum encoding"),
                            bincode::serialize(&Datum::create(&1u8, value, persist)).expect("datum encoding")
                        );
                    }
                    (selection, expected) => panic!(
                        "selection matched: {}, legacy matched: {}",
                        matches!(selection, ProduceSelection::Matched { .. }),
                        expected.is_some()
                    ),
                }
                assert_eq!(state(&space), before);
            });
    }
}

/// Added by G3 (DR-135, S6): the owner checks of a replay with owner
/// funding. They admit every item and name no refusal, so only R6 acts.
struct OwnDatum;

impl NativeOwnerFilter<u8, u8, u8, u8> for OwnDatum {
    fn admit_item(&self, _: &u8, _: bool, _: usize, _: &u8, _: bool) -> Result<bool> { Ok(true) }

    fn names_refusal_for(&self, _: &Consume) -> bool { false }

    fn requires_own_datum(&self) -> bool { true }
}

/// Added by G3 (DR-135, S6, R6): a produce keeps its own datum under owner
/// funding. A recorded COMM in which a produce matched a stored datum on a
/// channel that the join names once, and dropped its own datum, replays on
/// dev. With the owner checks the selection gives the datum back, so the
/// Matched row is a mismatch.
#[tokio::test]
async fn a_matched_produce_without_its_own_datum_is_rejected_under_owner_funding() {
    let space = space().await;
    put(&space, 1, 7, false);
    let consume = waiting(&space, &[1], vec![0], 9);
    let keys = channel_keys(&[1]);
    let stored = with_reader(&space, |reader| {
        space.prepare_consume_candidate(
            &[1],
            &keys,
            &[0],
            &9,
            &consume,
            &BTreeSet::new(),
            None,
            None,
            reader,
        )
    })
    .expect("consume selection")
    .expect("the stored datum matches");
    let identity = Logical(stored.comm);
    let joins = operation_keys(&[vec![1]]);
    let select = |owner: Option<&dyn NativeOwnerFilter<u8, u8, u8, u8>>| {
        with_reader(&space, |reader| {
            space.prepare_produce_candidate(
                &1,
                Datum::create(&1u8, 8u8, false),
                vec![vec![1]],
                &joins,
                Some(&identity),
                owner,
                reader,
            )
        })
        .map(matched)
        .expect("candidate selection")
    };
    assert!(select(None).is_some(), "dev replays the produce that drops its own datum");
    assert!(select(Some(&OwnDatum)).is_none(), "the owner checks require the produce's own datum");
}
