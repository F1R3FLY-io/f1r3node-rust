use proptest::prelude::*;

use super::*;
use crate::rspace::rspace::RSpace;
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

struct Matcher;

impl Match<u8, u8, u8> for Matcher {
    fn get(&self, pattern: &u8, datum: &u8) -> Option<u8> {
        (*pattern == 0 || pattern == datum).then_some(*datum)
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

struct Logical(COMM);

fn same_produce(a: &Produce, b: &Produce) -> bool {
    a.hash == b.hash && a.channel_hash == b.channel_hash && a.persistent == b.persistent
}

impl NativeCandidateIdentity for Logical {
    fn matches_consume(&self, source: &Consume) -> bool { source == &self.0.consume }

    fn matches_produce(&self, source: &Produce) -> bool {
        self.0.produces.iter().any(|p| same_produce(p, source))
    }

    fn repetition(&self, source: &Produce) -> Option<i32> {
        self.0
            .times_repeated
            .iter()
            .find_map(|(p, count)| same_produce(p, source).then_some(*count))
    }

    fn matches_comm(&self, source: &COMM) -> bool {
        self.matches_consume(&source.consume) &&
            source.peeks == self.0.peeks &&
            source.produces.len() == self.0.produces.len() &&
            source
                .produces
                .iter()
                .zip(&self.0.produces)
                .all(|(a, b)| same_produce(a, b)) &&
            source.times_repeated.len() == self.0.times_repeated.len() &&
            source
                .times_repeated
                .iter()
                .zip(&self.0.times_repeated)
                .all(|((a, n), (b, m))| n == m && same_produce(a, b))
    }
}

fn put(space: &Space, channel: u8, value: u8, persistent: bool) -> Produce {
    let datum = Datum::create(&channel, value, persistent);
    let source = datum.source.clone();
    space.get_store().put_datum(&channel, datum);
    space.increment_produce_counter(&source, persistent);
    source
}

fn waiting(space: &Space, channels: &[u8], persistent: bool, body: u8) -> Consume {
    let patterns = vec![0; channels.len()];
    let source = Consume::create(&channels.to_vec(), &patterns, &body, persistent);
    space
        .get_store()
        .put_continuation(channels, WaitingContinuation {
            patterns,
            continuation: body,
            persist: persistent,
            peeks: BTreeSet::new(),
            source: source.clone(),
        });
    for channel in channels {
        space.get_store().put_join(channel, channels);
    }
    source
}

fn fingerprint(space: &Space) -> (String, String, Vec<u8>, String, i64) {
    space.get_store().get_data(&1);
    space.get_store().get_joins(&1);
    space.get_store().get_continuations(&[1]);
    let mut rows: Vec<_> = space
        .get_store()
        .to_map()
        .into_iter()
        .map(|(key, row)| (key, format!("{row:?}")))
        .collect();
    rows.sort();
    (
        format!("{rows:?}"),
        format!("{:?}", space.event_log.lock().unwrap()),
        bincode::serialize(&*space.produce_counter.lock().unwrap()).unwrap(),
        format!("{:?}", space.replay_data.lock().unwrap()),
        space
            .replay_waiting_continuations_estimate
            .load(Ordering::Relaxed),
    )
}

#[tokio::test]
async fn native_candidate_preparation_does_not_need_participant_bindings_or_mutate_state() {
    let space = space().await;
    put(&space, 1, 7, false);
    let source = waiting(&space, &[1], false, 9);
    let before = fingerprint(&space);
    let candidate = space
        .prepare_native_consume_candidate(&[1], &[0], &9, &source, &BTreeSet::new(), None)
        .unwrap();
    assert_eq!(candidate.data[0].datum.a, 7);
    let identity = Logical(candidate.comm);
    assert!(
        space
            .prepare_native_consume_candidate(
                &[1],
                &[0],
                &9,
                &source,
                &BTreeSet::new(),
                Some(&identity)
            )
            .is_some()
    );
    assert!(space.replay_data.lock().unwrap().is_empty());
    assert_eq!(fingerprint(&space), before);
}

#[tokio::test]
async fn native_candidate_can_select_prestate_data_without_selecting_the_produce_trigger() {
    let space = space().await;
    put(&space, 1, 7, false);
    let source = waiting(&space, &[1], false, 9);
    let expected = space
        .prepare_native_consume_candidate(&[1], &[0], &9, &source, &BTreeSet::new(), None)
        .unwrap();
    let before = fingerprint(&space);
    let trigger = Produce::create(&1u8, &8u8, false);
    let identity = Logical(expected.comm);
    let prepared = space
        .prepare_native_produce_candidate(&1, &8, false, &trigger, vec![vec![1]], Some(&identity))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.candidate.data_candidates[0].datum.a, 7);
    assert!(prepared.candidate.data_candidates[0].datum_index >= 0);
    assert!(!prepared.comm.produces.contains(&trigger));
    assert_eq!(fingerprint(&space), before);
}

#[tokio::test]
async fn native_candidate_logical_identity_ignores_postdispatch_telemetry() {
    let space = space().await;
    let source = waiting(&space, &[1], false, 9);
    let trigger = Produce::create(&1u8, &7u8, false);
    let actual = space
        .prepare_native_produce_candidate(&1, &7, false, &trigger, vec![vec![1]], None)
        .unwrap()
        .unwrap();
    assert_eq!(actual.comm.consume, source);
    let mut imported = actual.comm;
    imported.produces[0].failed = true;
    imported.produces[0].is_deterministic = false;
    imported.produces[0].output_value = vec![vec![1, 2, 3]];
    let (mut key, count) = imported.times_repeated.pop_first().unwrap();
    key.output_value = vec![vec![4, 5, 6]];
    imported.times_repeated.insert(key, count);
    let identity = Logical(imported);
    let before = fingerprint(&space);
    let prepared = space
        .prepare_native_produce_candidate(&1, &7, false, &trigger, vec![vec![1]], Some(&identity))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.comm.produces[0].output_value, trigger.output_value);
    assert_eq!(prepared.candidate.data_candidates[0].datum_index, -1);
    assert_eq!(fingerprint(&space), before);
}

#[tokio::test]
async fn native_candidate_counter_overflow_and_guard_failure_leave_state_unchanged() {
    let space = space().await;
    waiting(&space, &[1], false, 255);
    let trigger = Produce::create(&1u8, &7u8, false);
    let before = fingerprint(&space);
    assert!(
        space
            .prepare_native_produce_candidate(&1, &7, false, &trigger, vec![vec![1]], None)
            .unwrap()
            .is_none()
    );
    assert_eq!(fingerprint(&space), before);
    space
        .produce_counter
        .lock()
        .unwrap()
        .insert(trigger.clone(), i32::MAX);
    let before = fingerprint(&space);
    assert!(
        space
            .prepare_native_produce_candidate(&1, &7, false, &trigger, vec![vec![1]], None)
            .is_err()
    );
    assert_eq!(fingerprint(&space), before);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn native_candidate_repeated_channels_preserve_indices_and_state(
        values in prop::collection::vec(1u8..250, 1..8),
        persistent in any::<bool>(), peek in any::<bool>(),
    ) {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let space = space().await;
            for value in &values { put(&space, 1, *value, persistent); }
            let channels = vec![1; values.len()];
            let patterns = vec![0; values.len()];
            let source = Consume::create(&channels, &patterns, &9u8, false);
            let peeks = if peek { BTreeSet::from([0]) } else { BTreeSet::new() };
            let before = fingerprint(&space);
            let prepared = space.prepare_native_consume_candidate(&channels, &patterns, &9, &source, &peeks, None).unwrap();
            prop_assert_eq!(prepared.data.len(), values.len());
            let indices: BTreeSet<_> = prepared.data.iter().map(|datum| datum.datum_index).collect();
            prop_assert_eq!(indices.len(), if persistent { 1 } else { values.len() });
            let identity = Logical(prepared.comm);
            let selected = space.prepare_native_consume_candidate(&channels, &patterns, &9, &source, &peeks, Some(&identity)).unwrap();
            prop_assert_eq!(selected.data.iter().map(|d| d.datum_index).collect::<Vec<_>>(), prepared.data.iter().map(|d| d.datum_index).collect::<Vec<_>>());
            prop_assert_eq!(fingerprint(&space), before);
            Ok(())
        })?;
    }

    #[test]
    fn native_candidate_mismatch_preserves_every_observable_state(field in 0usize..8) {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let space = space().await;
            put(&space, 1, 7, false);
            let source = waiting(&space, &[1], false, 9);
            let candidate = space.prepare_native_consume_candidate(&[1], &[0], &9, &source, &BTreeSet::new(), None).unwrap();
            let mut changed = candidate.comm;
            match field {
                0 => changed.consume.persistent = true,
                1 => changed.consume.channel_hashes.clear(),
                2 => changed.produces[0].persistent = true,
                3 => changed.produces[0].channel_hash = Produce::create(&2u8, &7u8, false).channel_hash,
                4 => { changed.peeks.insert(0); },
                5 => { changed.times_repeated.values_mut().for_each(|n| *n += 1); },
                6 => { changed.produces.push(changed.produces[0].clone()); },
                _ => { changed.times_repeated.clear(); },
            }
            let before = fingerprint(&space);
            prop_assert!(space.prepare_native_consume_candidate(&[1], &[0], &9, &source, &BTreeSet::new(), Some(&Logical(changed))).is_none());
            prop_assert_eq!(fingerprint(&space), before);
            Ok(())
        })?;
    }
}
