use std::collections::BTreeSet;

use imbl::OrdSet;
use proptest::prelude::*;

#[path = "../../../../block-storage/src/rust/util/ordered_snapshot.rs"]
mod ordered_snapshot;
#[path = "../../../../block-storage/src/rust/util/startup_scan.rs"]
mod startup_scan;

use ordered_snapshot::OrderedSnapshot;
use startup_scan::{StartupScan, StartupScanAction as Action, StartupScanError as Error};

fn scan(keys: impl IntoIterator<Item = u16>) -> StartupScan<u16> {
    let values: OrdSet<u16> = keys.into_iter().collect();
    let snapshot = OrderedSnapshot::new(values.clone());
    assert_eq!(snapshot.original_len(), values.len());
    StartupScan::new(snapshot)
}

#[test]
fn empty_and_metadata_only_snapshots_complete_without_capacity_checks() {
    for keys in [vec![], vec![1, 2, 3]] {
        let count = keys.len();
        let mut scan = scan(keys);
        for _ in 0..count {
            assert_eq!(
                scan.next_action(|| panic!("metadata must not load a body"), |_| false),
                Ok(Action::SkippedMetadata)
            );
        }
        assert_eq!(
            scan.next_action(|| panic!("phase end must not check capacity"), |_| true),
            Ok(Action::PhaseChanged)
        );
        assert_eq!(
            scan.next_action(|| panic!("empty admission must complete"), |_| true),
            Ok(Action::Complete)
        );
    }
}

#[test]
fn later_presence_error_prevents_any_admission() {
    let mut scan = scan([1, 2, 3]);
    for key in [1, 2] {
        assert_eq!(
            scan.next_action(|| true, |_| true),
            Ok(Action::CheckPresence(key))
        );
        scan.record_presence(true).unwrap();
    }
    assert_eq!(
        scan.next_action(|| true, |_| true),
        Ok(Action::CheckPresence(3))
    );
    scan.fail();
    assert_eq!(scan.next_action(|| true, |_| true), Ok(Action::Failed));
    assert_eq!(scan.record_presence(true), Err(Error::UnexpectedResult));
    scan.cancel();
    assert_eq!(scan.next_action(|| true, |_| true), Ok(Action::Failed));
}

#[test]
fn capacity_parking_keeps_exact_pending_hash_in_each_phase() {
    let mut scan = scan([u16::MAX]);
    for _ in 0..10 {
        assert_eq!(scan.next_action(|| false, |_| true), Ok(Action::Parked));
    }
    assert_eq!(
        scan.next_action(|| true, |_| true),
        Ok(Action::CheckPresence(u16::MAX))
    );
    assert_eq!(
        scan.next_action(|| true, |_| true),
        Err(Error::AwaitingResult)
    );
    assert_eq!(scan.record_processed(), Err(Error::UnexpectedResult));
    scan.record_presence(true).unwrap();
    assert_eq!(
        scan.next_action(|| false, |_| true),
        Ok(Action::PhaseChanged)
    );
    assert_eq!(scan.next_action(|| false, |_| true), Ok(Action::Parked));
    assert_eq!(
        scan.next_action(|| true, |_| true),
        Ok(Action::Process(u16::MAX))
    );
    assert_eq!(scan.record_presence(true), Err(Error::UnexpectedResult));
    scan.record_processed().unwrap();
    assert_eq!(scan.next_action(|| false, |_| true), Ok(Action::Complete));
}

#[test]
fn cancellation_discards_pending_work_and_cannot_report_success() {
    let mut scan = scan([1, 2]);
    assert_eq!(
        scan.next_action(|| true, |_| true),
        Ok(Action::CheckPresence(1))
    );
    scan.cancel();
    assert_eq!(scan.record_presence(true), Err(Error::UnexpectedResult));
    assert_eq!(scan.next_action(|| true, |_| true), Ok(Action::Cancelled));
    scan.fail();
    assert_eq!(scan.next_action(|| true, |_| true), Ok(Action::Cancelled));
}

#[test]
fn admission_error_after_prior_work_prevents_startup_completion() {
    let mut scan = scan([1, 2]);
    for key in [1, 2] {
        assert_eq!(
            scan.next_action(|| true, |_| true),
            Ok(Action::CheckPresence(key))
        );
        scan.record_presence(true).unwrap();
    }
    assert_eq!(
        scan.next_action(|| false, |_| true),
        Ok(Action::PhaseChanged)
    );
    assert_eq!(scan.next_action(|| true, |_| true), Ok(Action::Process(1)));
    scan.record_processed().unwrap();
    assert_eq!(scan.record_processed(), Err(Error::UnexpectedResult));
    assert_eq!(scan.next_action(|| true, |_| true), Ok(Action::Process(2)));
    scan.fail();
    assert_eq!(scan.record_processed(), Err(Error::UnexpectedResult));
    assert_eq!(scan.next_action(|| true, |_| true), Ok(Action::Failed));
}

#[test]
fn selected_membership_remains_fixed_when_body_appears_after_presence() {
    let mut scan = scan([1, 2]);
    let mut bodies = BTreeSet::from([2]);
    assert_eq!(
        scan.next_action(|| true, |_| true),
        Ok(Action::CheckPresence(1))
    );
    scan.record_presence(bodies.contains(&1)).unwrap();
    bodies.insert(1);
    assert_eq!(
        scan.next_action(|| true, |_| true),
        Ok(Action::CheckPresence(2))
    );
    scan.record_presence(bodies.contains(&2)).unwrap();
    assert_eq!(
        scan.next_action(|| true, |_| true),
        Ok(Action::PhaseChanged)
    );
    assert_eq!(scan.next_action(|| true, |_| true), Ok(Action::Process(2)));
    scan.record_processed().unwrap();
    assert_eq!(scan.next_action(|| true, |_| true), Ok(Action::Complete));
}

proptest! {
    #[test]
    fn two_phase_trace_matches_eager_snapshot_with_capacity_and_membership_changes(
        entries in proptest::collection::vec((any::<u16>(), any::<bool>()), 0..256),
        presence_mode in 0u8..3,
        changes in proptest::collection::vec((any::<u16>(), any::<bool>(), any::<bool>()), 0..512),
    ) {
        let keys: Vec<_> = entries.iter().map(|(key, _)| *key).collect();
        let presence: BTreeSet<_> = entries.iter().filter(|(_, present)| match presence_mode {
            0 => false,
            1 => true,
            _ => *present,
        }).map(|(key, _)| *key).collect();
        let captured: BTreeSet<_> = keys.iter().copied().collect();
        let expected_presence: Vec<_> = captured.iter().copied().filter(|key| key % 3 != 0).collect();
        let expected_process: Vec<_> = expected_presence.iter().copied().filter(|key| presence.contains(key)).collect();
        let mut live: OrdSet<u16> = keys.into_iter().collect();
        let mut scan = StartupScan::new(OrderedSnapshot::new(live.clone()));
        let mut visited_presence = Vec::new();
        let mut visited_process = Vec::new();
        let mut changes = changes.into_iter();
        loop {
            let capacity = match changes.next() {
                Some((key, insert, capacity)) => {
                    if insert { live.insert(key); } else { live.remove(&key); }
                    capacity
                }
                None => true,
            };
            match scan.next_action(|| capacity, |key| key % 3 != 0).unwrap() {
                Action::CheckPresence(key) => {
                    prop_assert!(capacity);
                    prop_assert!(visited_process.is_empty());
                    visited_presence.push(key);
                    scan.record_presence(presence.contains(&key)).unwrap();
                }
                Action::Process(key) => {
                    prop_assert!(capacity);
                    prop_assert_eq!(&visited_presence, &expected_presence);
                    visited_process.push(key);
                    scan.record_processed().unwrap();
                }
                Action::SkippedMetadata | Action::PhaseChanged | Action::Parked => {}
                Action::Complete => break,
                other => panic!("unexpected action: {other:?}"),
            }
        }
        prop_assert_eq!(visited_presence, expected_presence);
        prop_assert_eq!(visited_process, expected_process);
        prop_assert_eq!(scan.next_action(|| false, |_| true), Ok(Action::Complete));
    }
}
