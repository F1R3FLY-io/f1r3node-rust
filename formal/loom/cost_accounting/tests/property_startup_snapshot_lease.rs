use std::collections::BTreeMap;

use proptest::prelude::*;

#[path = "../../../../casper/src/rust/blocks/block_processing_queue/startup_snapshot_lease.rs"]
mod startup_snapshot_lease;
use startup_snapshot_lease::{LeasePhase, LeaseRole, SnapshotLeases};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Reserved,
    Begun,
    Finished,
    Stored,
    Activated,
    Retired,
    Destroyed,
    Released,
}

#[derive(Default)]
struct Reference {
    histories: BTreeMap<u64, (u64, Vec<Event>)>,
}

impl Reference {
    fn last(&self, id: u64) -> Option<Event> { self.histories.get(&id)?.1.last().copied() }

    fn role(&self, id: u64) -> LeaseRole {
        if self
            .histories
            .get(&id)
            .is_some_and(|(_, events)| events.contains(&Event::Activated))
        {
            LeaseRole::Active
        } else {
            LeaseRole::Pending
        }
    }

    fn slot(&self, role: LeaseRole) -> Option<u64> {
        let ids: Vec<_> = self
            .histories
            .keys()
            .copied()
            .filter(|&id| self.last(id) != Some(Event::Released) && self.role(id) == role)
            .collect();
        assert!(ids.len() <= 1);
        ids.first().copied()
    }

    fn record(&mut self, id: u64, event: Event) {
        self.histories.get_mut(&id).unwrap().1.push(event);
    }

    fn verify(&self, actual: &SnapshotLeases<u64, u64>) {
        for role in [LeaseRole::Pending, LeaseRole::Active] {
            let entry = match role {
                LeaseRole::Pending => actual.pending(),
                LeaseRole::Active => actual.active(),
            };
            assert_eq!(entry.map(|entry| entry.identity), self.slot(role));
            if let Some(entry) = entry {
                assert_eq!(entry.key, self.histories[&entry.identity].0);
                assert_eq!(entry.phase, match self.last(entry.identity).unwrap() {
                    Event::Reserved => LeasePhase::Reserved,
                    Event::Begun => LeasePhase::Capturing,
                    Event::Finished => LeasePhase::Built,
                    Event::Stored => LeasePhase::Stored,
                    Event::Activated => LeasePhase::Active,
                    Event::Retired => LeasePhase::Retiring,
                    Event::Destroyed => LeasePhase::Destroyed,
                    Event::Released => panic!("released lease retains a slot"),
                });
            }
        }
        let mut physical = [0usize; 2];
        for (&id, (_, events)) in &self.histories {
            let allocations = events
                .iter()
                .filter(|&&event| event == Event::Begun)
                .count();
            assert!(
                allocations <= 1,
                "one lease admitted duplicate episode construction"
            );
            let destroyed = events.contains(&Event::Destroyed);
            if allocations > 0 && !destroyed {
                assert_ne!(self.last(id), Some(Event::Released));
                let role = self.role(id);
                assert_eq!(self.slot(role), Some(id));
                physical[usize::from(role == LeaseRole::Active)] += allocations;
            }
            if self.last(id) == Some(Event::Released) {
                assert!(destroyed);
            }
            if self.last(id) == Some(Event::Begun) {
                assert_eq!(self.slot(LeaseRole::Pending), Some(id));
            }
        }
        assert!(physical[0] <= 1 && physical[1] <= 1 && physical.iter().sum::<usize>() <= 2);
    }
}

proptest! {
    #[test]
    fn allocation_and_destruction_histories_refine_the_lease_protocol(
        operations in prop::collection::vec((0u8..12, any::<u16>(), any::<bool>(), 0u64..4), 0..512)
    ) {
        let mut actual = SnapshotLeases::default();
        let mut reference = Reference::default();
        let mut fresh = 1u64;
        for (operation, selector, active_role, key) in operations {
            let role = if active_role { LeaseRole::Active } else { LeaseRole::Pending };
            let id = match selector % 4 {
                0 => reference.slot(LeaseRole::Pending).unwrap_or(0),
                1 => reference.slot(LeaseRole::Active).unwrap_or(0),
                _ => u64::from(selector) % fresh,
            };
            let last = reference.last(id);
            let in_pending = reference.slot(LeaseRole::Pending) == Some(id);
            let in_active = reference.slot(LeaseRole::Active) == Some(id);
            let in_role = reference.slot(role) == Some(id);
            let (accepted, expected, event) = match operation {
                0 => {
                    let expected = reference.slot(LeaseRole::Pending).is_none();
                    let accepted = actual.reserve(key, fresh);
                    prop_assert_eq!(accepted, expected);
                    if accepted { reference.histories.insert(fresh, (key, vec![Event::Reserved])); }
                    fresh += 1;
                    reference.verify(&actual);
                    continue;
                }
                1 => (actual.begin_capture(&id), in_pending && last == Some(Event::Reserved), Event::Begun),
                2 => (actual.finish_capture(&id), in_pending && last == Some(Event::Begun), Event::Finished),
                3 => (actual.store(&key, &id), in_pending && last == Some(Event::Finished) && reference.histories[&id].0 == key, Event::Stored),
                4 => {
                    let pending = reference.slot(LeaseRole::Pending);
                    let expected = pending.filter(|&pending| reference.slot(LeaseRole::Active).is_none()
                        && reference.last(pending) == Some(Event::Stored) && reference.histories[&pending].0 == key);
                    let activated = actual.activate(&key);
                    prop_assert_eq!(activated, expected);
                    if let Some(id) = activated { reference.record(id, Event::Activated); }
                    reference.verify(&actual);
                    continue;
                }
                5 => (actual.retire_stored(&id), in_pending && last == Some(Event::Stored), Event::Retired),
                6 => (actual.abandon_capture(&id), in_pending && matches!(last, Some(Event::Reserved | Event::Begun | Event::Finished)), Event::Retired),
                7 => (actual.retire_active(&id), in_active && last == Some(Event::Activated), Event::Retired),
                8 => (actual.destroyed(role, &id), in_role && last == Some(Event::Retired), Event::Destroyed),
                9 => (actual.release(role, &id), in_role && last == Some(Event::Destroyed), Event::Released),
                10 => {
                    let key = reference.histories.get(&id).map_or(key, |entry| entry.0);
                    (actual.store(&key, &id), in_pending && last == Some(Event::Finished), Event::Stored)
                }
                _ => {
                    let key = reference.histories.get(&id).map_or(key, |entry| entry.0);
                    let expected = reference.slot(LeaseRole::Pending).filter(|&pending| reference.slot(LeaseRole::Active).is_none()
                        && reference.last(pending) == Some(Event::Stored) && reference.histories[&pending].0 == key);
                    let activated = actual.activate(&key);
                    prop_assert_eq!(activated, expected);
                    if let Some(id) = activated { reference.record(id, Event::Activated); }
                    reference.verify(&actual);
                    continue;
                }
            };
            prop_assert_eq!(accepted, expected);
            if accepted { reference.record(id, event); }
            reference.verify(&actual);
        }
    }
}

#[test]
fn all_phases_reject_early_release_and_same_identity_wrong_role() {
    let mut leases = SnapshotLeases::default();
    assert!(leases.reserve(7, 11));
    assert!(!leases.release(LeaseRole::Pending, &11));
    assert!(leases.activate(&7).is_none());
    assert!(leases.begin_capture(&11));
    assert!(!leases.begin_capture(&11));
    assert!(!leases.release(LeaseRole::Pending, &11));
    assert!(leases.finish_capture(&11));
    assert!(!leases.finish_capture(&11));
    assert!(!leases.store(&8, &11));
    assert!(leases.store(&7, &11));
    assert_eq!(leases.activate(&7), Some(11));
    assert!(!leases.reserve(8, 11));
    assert!(!leases.retire_stored(&11));
    assert!(leases.retire_active(&11));
    assert!(!leases.release(LeaseRole::Active, &11));
    assert!(!leases.destroyed(LeaseRole::Pending, &11));
    assert!(leases.destroyed(LeaseRole::Active, &11));
    assert!(!leases.release(LeaseRole::Pending, &11));
    assert!(leases.release(LeaseRole::Active, &11));
    assert!(!leases.release(LeaseRole::Active, &11));
    assert!(leases.reserve(8, 22));
    assert!(leases.begin_capture(&22));
    assert!(leases.finish_capture(&22));
    assert!(leases.store(&8, &22));
    assert!(leases.retire_stored(&22));
    assert!(!leases.destroyed(LeaseRole::Pending, &11));
    assert_eq!(leases.pending().unwrap().phase, LeasePhase::Retiring);
    assert!(leases.destroyed(LeaseRole::Pending, &22));
    assert!(!leases.release(LeaseRole::Pending, &11));
    assert_eq!(leases.pending().unwrap().identity, 22);
    assert!(leases.release(LeaseRole::Pending, &22));
}
