use super::*;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::{CountTransition, ObjectDigest, ObjectRefV1, StateVersionV1};

struct EmptyHistory;

impl HistoryReaderBase<String, String, String, String> for EmptyHistory {
    fn get_data_canonical(&self, _key: &String) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_canonical(
        &self,
        _key: &[String],
    ) -> Vec<WaitingContinuation<String, String>> {
        Vec::new()
    }

    fn get_joins_canonical(&self, _key: &String) -> Vec<Vec<String>> { Vec::new() }
}

fn datum(value: &str) -> Datum<String> {
    Datum::create(&"channel".to_owned(), value.to_owned(), false)
}

fn base_version() -> StateVersionV1 {
    StateVersionV1::new(
        0,
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&[0; 32]), 32).unwrap(),
    )
}

fn populated_store() -> Box<dyn HotStore<String, String, String, String>> {
    let store =
        HotStoreInstances::create_from_hs_and_hr(HotStoreState::default(), Box::new(EmptyHistory));
    let channels = vec!["channel".to_owned(), "channel".to_owned()];
    let continuation = WaitingContinuation::create(
        &channels,
        &vec!["first".to_owned(), "second".to_owned()],
        &"body".to_owned(),
        false,
        BTreeSet::new(),
    );
    store.put_datum(&channels[0], datum("value"));
    assert_eq!(store.put_continuation(&channels, continuation), Some(true));
    store.put_join(&channels[0], &channels).unwrap();
    store
}

fn assert_same_pending(left: &PendingDeltas, right: &PendingDeltas) {
    assert_eq!(left.data, right.data);
    assert_eq!(left.continuations, right.continuations);
    assert_eq!(left.joins, right.joins);
    assert!(
        left.continuation_content.as_ref().unwrap().iter().eq(right
            .continuation_content
            .as_ref()
            .unwrap()
            .iter())
    );
    assert!(
        left.continuation_source_bytes
            .as_ref()
            .unwrap()
            .iter()
            .eq(right.continuation_source_bytes.as_ref().unwrap().iter())
    );
}

#[test]
fn acknowledgement_clears_five_populated_journals_and_preserves_live_records() {
    let store = populated_store();
    let pending = store.pending_deltas().unwrap();
    assert!(!pending.data.as_ref().unwrap().is_empty());
    assert!(!pending.continuations.as_ref().unwrap().is_empty());
    assert!(!pending.continuation_content.as_ref().unwrap().is_empty());
    assert!(
        !pending
            .continuation_source_bytes
            .as_ref()
            .unwrap()
            .is_empty()
    );
    assert!(!pending.joins.as_ref().unwrap().is_empty());
    let before = store.snapshot();
    store.commit_pending_deltas(&pending).unwrap();
    let cleared = store.pending_deltas().unwrap();
    assert!(cleared.data.unwrap().is_empty());
    assert!(cleared.continuations.unwrap().is_empty());
    assert!(cleared.continuation_content.unwrap().is_empty());
    assert!(cleared.continuation_source_bytes.unwrap().is_empty());
    assert!(cleared.joins.unwrap().is_empty());
    let after = store.snapshot();
    assert_eq!(before.data, after.data);
    assert_eq!(before.continuations, after.continuations);
    assert_eq!(before.joins, after.joins);
    assert_eq!(before.installed_continuations, after.installed_continuations);
    assert_eq!(before.installed_joins, after.installed_joins);
}

#[test]
fn mismatch_or_missing_capability_in_any_journal_rejects_without_partial_clear() {
    let store = populated_store();
    let pending = store.pending_deltas().unwrap();
    for omitted in [false, true] {
        for namespace in 0..5 {
            let mut wrong = pending.clone();
            match namespace {
                0 => wrong.data = (!omitted).then(BagDelta::new),
                1 => wrong.continuations = (!omitted).then(BagDelta::new),
                2 => wrong.continuation_content = (!omitted).then(PathMap::new),
                3 => wrong.continuation_source_bytes = (!omitted).then(PathMap::new),
                4 => wrong.joins = (!omitted).then(BagDelta::new),
                _ => unreachable!(),
            }
            assert!(matches!(
                store.commit_pending_deltas(&wrong),
                Err(RSpaceError::JournalConflict(_))
            ));
            assert_same_pending(&pending, &store.pending_deltas().unwrap());
        }
    }
}

#[test]
fn join_journal_uses_native_key_and_repeated_channel_positions() {
    use crate::rspace::serializers::serializers::encode_join_record;
    let store = populated_store();
    let channels = vec!["channel".to_owned(), "channel".to_owned()];
    store.put_join(&channels[0], &channels).unwrap();
    let mut native_path = RhoPathV1::join_prefix(&channels[0]);
    let canonical = encode_join_record(&channels);
    native_path.extend_from_slice(&canonical);
    let observation = store.pending_mutation_observation().unwrap();
    let joins = observation.pending.joins.as_ref().unwrap();
    assert_eq!(joins.additions().count(&native_path), 1);
    assert_eq!(observation.join_counts.get(&native_path), Some(&1));
    assert_eq!(RhoPathV1::parse_join(&native_path).unwrap().join_identity(), canonical);
    let batch = mutation_batch_from_observation(base_version(), &observation).unwrap();
    assert_eq!(batch.join_transition(&native_path), Some(CountTransition::new(0, 1).unwrap()));
    store.commit_pending_deltas(&observation.pending).unwrap();
    store.remove_continuation(&channels, 0).unwrap();
    store.remove_join(&channels[0], &channels).unwrap();
    let removal = store.pending_mutation_observation().unwrap();
    assert_eq!(
        removal
            .pending
            .joins
            .as_ref()
            .unwrap()
            .removals()
            .count(&native_path),
        1
    );
    assert_eq!(removal.join_counts.get(&native_path), Some(&0));
    let batch = mutation_batch_from_observation(base_version(), &removal).unwrap();
    assert_eq!(batch.join_transition(&native_path), Some(CountTransition::new(1, 0).unwrap()));
}

#[test]
fn absent_count_evidence_is_not_a_captured_zero() {
    let store = populated_store();
    store
        .commit_pending_deltas(&store.pending_deltas().unwrap())
        .unwrap();
    store.remove_datum(&"channel".to_owned(), 0).unwrap();
    let mut observation = store.pending_mutation_observation().unwrap();
    assert!(mutation_batch_from_observation(base_version(), &observation).is_ok());
    observation.data_counts = PathMap::new();
    assert!(mutation_batch_from_observation(base_version(), &observation).is_err());
}

proptest! {
    #[test]
    fn journal_count_inversion_matches_live_multiplicities(
        before in 0usize..16, after in 0usize..16,
    ) {
        let mut state = HotStoreState::default();
        if before != 0 {
            state.data.insert("channel".to_owned(), vec![datum("value"); before]);
        }
        let store = HotStoreInstances::create_from_hs_and_hr(state, Box::new(EmptyHistory));
        let path = RSpaceDataIndex::datum_path(&"channel".to_owned(), &datum("value"));
        for _ in before..after { store.put_datum(&"channel".to_owned(), datum("value")); }
        for _ in after..before { store.remove_datum(&"channel".to_owned(), 0).unwrap(); }
        let observation = store.pending_mutation_observation().unwrap();
        if before == after {
            prop_assert!(matches!(mutation_batch_from_observation(base_version(), &observation),
                Err(MutationError::EmptyBatch)));
        } else {
            prop_assert_eq!(observation.data_counts.get(&path), Some(&(after as u64)));
            let batch = mutation_batch_from_observation(base_version(), &observation).unwrap();
            prop_assert_eq!(batch.data_transition(&path),
                Some(crate::rspace::persistence::CountTransition::new(before as u64, after as u64).unwrap()));
        }
    }
}

#[test]
fn committed_journal_clears_only_the_exact_revision() {
    let store = HotStoreInstances::create_from_hs_and_hr::<String, String, String, String>(
        HotStoreState::default(),
        Box::new(EmptyHistory),
    );
    // The first write populates a cold channel from history; the second write
    // exercises the resident PathMap mutation/journal path.
    store.put_datum(&"channel".to_owned(), datum("seed"));
    store.put_datum(&"channel".to_owned(), datum("value"));
    let snapshot = store.pending_deltas().expect("journal snapshot");
    let before = store.snapshot();

    store
        .commit_pending_deltas(&snapshot)
        .expect("matching journal commits");

    assert!(store.pending_deltas().unwrap().data.unwrap().is_empty());
    assert!(
        store
            .pending_deltas()
            .unwrap()
            .continuations
            .unwrap()
            .is_empty()
    );
    assert!(store.pending_deltas().unwrap().joins.unwrap().is_empty());
    let after = store.snapshot();
    assert_eq!(after.data, before.data, "data contents are unchanged");
    assert_eq!(after.continuations, before.continuations, "continuations unchanged");
    assert_eq!(after.joins, before.joins, "joins unchanged");
    assert_eq!(
        after.installed_continuations, before.installed_continuations,
        "installed continuations unchanged"
    );
    assert_eq!(after.installed_joins, before.installed_joins, "installed joins unchanged");
}

#[test]
fn stale_journal_cannot_clear_a_newer_revision() {
    let store = HotStoreInstances::create_from_hs_and_hr::<String, String, String, String>(
        HotStoreState::default(),
        Box::new(EmptyHistory),
    );
    store.put_datum(&"channel".to_owned(), datum("seed"));
    store.put_datum(&"channel".to_owned(), datum("old"));
    let stale = store.pending_deltas().expect("journal snapshot");
    store.put_datum(&"channel".to_owned(), datum("new"));

    let error = store
        .commit_pending_deltas(&stale)
        .expect_err("stale journal must be rejected");
    assert!(matches!(error, RSpaceError::JournalConflict(_)));
    assert!(!store.pending_deltas().unwrap().data.unwrap().is_empty());
}

#[test]
fn first_write_to_a_cold_channel_is_journaled() {
    let store = HotStoreInstances::create_from_hs_and_hr::<String, String, String, String>(
        HotStoreState::default(),
        Box::new(EmptyHistory),
    );
    store.put_datum(&"channel".to_owned(), datum("first"));
    let pending = store.pending_deltas().expect("journal snapshot");
    assert_eq!(pending.data.as_ref().unwrap().additions().distinct_len(), 1);
    assert!(pending.data.as_ref().unwrap().removals().is_empty());
}

#[test]
fn mutation_adapter_reads_counts_from_resident_pathmaps() {
    let store = HotStoreInstances::create_from_hs_and_hr::<String, String, String, String>(
        HotStoreState::default(),
        Box::new(EmptyHistory),
    );
    store.put_datum(&"channel".to_owned(), datum("seed"));
    store.put_datum(&"channel".to_owned(), datum("value"));
    let observation = store
        .pending_mutation_observation()
        .expect("coherent journal/count observation");
    let path = observation
        .pending
        .data
        .as_ref()
        .unwrap()
        .additions()
        .iter()
        .next()
        .expect("one data path")
        .0;
    assert_eq!(store.data_count_at_path(&path).unwrap(), 1);
    assert_eq!(store.join_count_at_path(&path).unwrap(), 0);

    let base_bytes = [0_u8; 32];
    let base = StateVersionV1::new(
        0,
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&base_bytes), base_bytes.len() as u64)
            .unwrap(),
    );
    assert_eq!(observation.data_counts.get(&path), Some(&1));
    let batch = mutation_batch_from_observation(base, &observation)
        .expect("resident lookup adapter builds an exact batch");
    assert_eq!(
        batch.data_transition(&path),
        Some(CountTransition::new(0, 1).unwrap()),
        "the pending addition is reconstructed from the current resident count"
    );
}
