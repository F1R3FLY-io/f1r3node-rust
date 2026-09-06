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
    assert!(store
        .pending_deltas()
        .unwrap()
        .continuations
        .unwrap()
        .is_empty());
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
fn mutation_adapter_reads_counts_from_resident_pathmaps() {
    let store = HotStoreInstances::create_from_hs_and_hr::<String, String, String, String>(
        HotStoreState::default(),
        Box::new(EmptyHistory),
    );
    store.put_datum(&"channel".to_owned(), datum("seed"));
    store.put_datum(&"channel".to_owned(), datum("value"));
    let pending = store.pending_deltas().expect("journal snapshot");
    let path = pending
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
        ObjectRefV1::new(
            ObjectDigest::of_canonical_bytes(&base_bytes),
            base_bytes.len() as u64,
        )
        .unwrap(),
    );
    let batch = mutation_batch_from_pending_with_counts(
        base,
        &pending,
        |candidate| store.data_count_at_path(candidate).unwrap(),
        |candidate| store.join_count_at_path(candidate).unwrap(),
    )
    .expect("resident lookup adapter builds an exact batch");
    assert_eq!(
        batch.data_transition(&path),
        Some(CountTransition::new(0, 1).unwrap()),
        "the pending addition is reconstructed from the current resident count"
    );
}
