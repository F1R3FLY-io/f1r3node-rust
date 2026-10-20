//! Protocol-7 replay ordering. The old IO-to-COMM multiset remains the
//! candidate index; it is not evidence that the next global event is legal.

use std::sync::{Arc, Mutex, PoisonError};

use crate::rspace::errors::RSpaceError;
use crate::rspace::trace::Log;
use crate::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce, RestingCOMM};

#[derive(Default)]
pub(super) struct OrderedReplayCursor {
    expected: Option<Arc<Log>>,
    next: usize,
    generation: u64,
    pending: bool,
}

#[derive(Clone, Copy)]
pub(super) struct CursorCheckpoint {
    pub(super) generation: u64,
    pub(super) next: usize,
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the protocol-7 ordered cursor is not wired into replay yet; only this module's \
                  tests drive it"
    )
)]
pub(super) enum ReplayEventRef<'a> {
    Produce(&'a Produce),
    Consume(&'a Consume),
    Comm(&'a COMM),
    Resting(&'a RestingCOMM),
}

impl ReplayEventRef<'_> {
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the protocol-7 ordered cursor is not wired into replay yet; only this \
                      module's tests drive it"
        )
    )]
    fn matches(&self, expected: &Event) -> bool {
        match (self, expected) {
            (Self::Produce(actual), Event::IoEvent(IOEvent::Produce(expected))) => {
                super::native_directive::exact_produce(actual, expected)
            }
            (Self::Consume(actual), Event::IoEvent(IOEvent::Consume(expected))) => {
                *actual == expected
            }
            (Self::Comm(actual), Event::Comm(expected)) => exact_comm(actual, expected),
            (Self::Resting(actual), Event::RestingComm(expected)) => *actual == expected.as_ref(),
            _ => false,
        }
    }
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the protocol-7 ordered cursor is not wired into replay yet; only this module's \
                  tests drive it"
    )
)]
fn exact_comm(actual: &COMM, expected: &COMM) -> bool {
    actual.consume == expected.consume &&
        actual.peeks == expected.peeks &&
        actual.produces.len() == expected.produces.len() &&
        actual
            .produces
            .iter()
            .zip(&expected.produces)
            .all(|(a, b)| super::native_directive::exact_produce(a, b)) &&
        actual.times_repeated.len() == expected.times_repeated.len() &&
        actual
            .times_repeated
            .iter()
            .zip(&expected.times_repeated)
            .all(|((a_key, a_count), (e_key, e_count))| {
                super::native_directive::exact_produce(a_key, e_key) && a_count == e_count
            })
}

impl OrderedReplayCursor {
    pub(super) fn clear(&mut self) {
        assert!(!self.pending, "reset crossed an active replay cursor lease");
        self.expected = None;
        self.next = 0;
        // Saturation refuses any later rig rather than reusing a checkpoint
        // generation after counter exhaustion.
        self.generation = self.generation.saturating_add(1);
    }

    pub(super) fn checkpoint(&self) -> Option<CursorCheckpoint> {
        assert!(!self.pending, "checkpoint crossed an active replay cursor lease");
        self.expected.as_ref().map(|_| CursorCheckpoint {
            generation: self.generation,
            next: self.next,
        })
    }

    pub(super) fn restore(
        &mut self,
        checkpoint: Option<CursorCheckpoint>,
    ) -> Result<(), RSpaceError> {
        if self.pending {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        match checkpoint {
            None if self.expected.is_none() => Ok(()),
            Some(saved) if self.expected.is_some() && saved.generation == self.generation => {
                let length = self.expected.as_ref().map_or(0, |events| events.len());
                if saved.next > length {
                    return Err(RSpaceError::ProduceCommitProtocolViolation);
                }
                self.next = saved.next;
                Ok(())
            }
            _ => Err(RSpaceError::ProduceCommitProtocolViolation),
        }
    }

    pub(super) fn is_complete(&self) -> bool {
        !self.pending &&
            self.expected
                .as_ref()
                .is_none_or(|log| self.next == log.len())
    }

    pub(super) fn is_pending(&self) -> bool { self.pending }
}

// Changed by the merge of feature/cost-accounted-rho (finding R2-08): HEAD
// released the lock while its host-work meter ran, so that a reentrant meter
// could not deadlock, and then took the lock again. DR-131 removed the meter,
// so one lock covers the check and the update. The replaced log drops after
// the lock is released.
// ```text
// pub(super) fn install(
//     cursor: &Arc<Mutex<OrderedReplayCursor>>,
//     log: Log,
// ) -> Result<(), RSpaceError> {
//     let (generation, position) = {
//         let guard = cursor.lock().unwrap_or_else(PoisonError::into_inner);
//         if guard.pending {
//             return Err(RSpaceError::ProduceCommitProtocolViolation);
//         }
//         (guard.generation, guard.next)
//     };
//     let next_generation = generation
//         .checked_add(1)
//         .ok_or(RSpaceError::ProduceCommitProtocolViolation)?;
//     let mut guard = cursor.lock().unwrap_or_else(PoisonError::into_inner);
//     if guard.pending || guard.generation != generation || guard.next != position {
//         return Err(RSpaceError::ProduceCommitProtocolViolation);
//     }
//     guard.expected = Some(Arc::new(log));
//     guard.next = 0;
//     guard.generation = next_generation;
//     Ok(())
// }
// ```
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the protocol-7 ordered cursor is not wired into replay yet; only this module's \
                  tests drive it"
    )
)]
pub(super) fn install(
    cursor: &Arc<Mutex<OrderedReplayCursor>>,
    log: Log,
) -> Result<(), RSpaceError> {
    let replaced = {
        let mut guard = cursor.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.pending {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        let next_generation = guard
            .generation
            .checked_add(1)
            .ok_or(RSpaceError::ProduceCommitProtocolViolation)?;
        guard.next = 0;
        guard.generation = next_generation;
        guard.expected.replace(Arc::new(log))
    };
    drop(replaced);
    Ok(())
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the protocol-7 ordered cursor is not wired into replay yet; only this module's \
                  tests drive it"
    )
)]
#[must_use = "the preflight lease must be committed only after its store transition"]
pub(super) struct CursorLease {
    cursor: Arc<Mutex<OrderedReplayCursor>>,
    generation: u64,
    start: usize,
    count: usize,
    committed: bool,
}

impl CursorLease {
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the protocol-7 ordered cursor is not wired into replay yet; only this \
                      module's tests drive it"
        )
    )]
    pub(super) fn commit(mut self) {
        let mut cursor = self.cursor.lock().unwrap_or_else(PoisonError::into_inner);
        assert!(
            cursor.pending && cursor.generation == self.generation && cursor.next == self.start,
            "replay cursor lease lost its transition boundary"
        );
        cursor.next += self.count;
        cursor.pending = false;
        self.committed = true;
    }
}

impl Drop for CursorLease {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut cursor = self.cursor.lock().unwrap_or_else(PoisonError::into_inner);
        if cursor.pending && cursor.generation == self.generation && cursor.next == self.start {
            cursor.pending = false;
        }
    }
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the protocol-7 ordered cursor is not wired into replay yet; only this module's \
                  tests drive it"
    )
)]
pub(super) fn preflight(
    cursor: &Arc<Mutex<OrderedReplayCursor>>,
    events: &[ReplayEventRef<'_>],
) -> Result<CursorLease, RSpaceError> {
    let (expected, start, generation) = {
        let guard = cursor.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.pending {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        (
            Arc::clone(
                guard
                    .expected
                    .as_ref()
                    .ok_or(RSpaceError::ProduceCommitProtocolViolation)?,
            ),
            guard.next,
            guard.generation,
        )
    };
    let end = start
        .checked_add(events.len())
        .ok_or(RSpaceError::ProduceCommitProtocolViolation)?;
    let next = expected
        .get(start..end)
        .ok_or(RSpaceError::ProduceCommitProtocolViolation)?;
    for (actual, expected) in events.iter().zip(next) {
        if !actual.matches(expected) {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
    }
    let mut guard = cursor.lock().unwrap_or_else(PoisonError::into_inner);
    if guard.pending || guard.generation != generation || guard.next != start {
        return Err(RSpaceError::ProduceCommitProtocolViolation);
    }
    guard.pending = true;
    drop(guard);
    Ok(CursorLease {
        cursor: Arc::clone(cursor),
        generation,
        start,
        count: events.len(),
        committed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_order_and_payload_refuse_before_cursor_advance() {
        let first = Produce::default();
        // Changed by the merge of feature/cost-accounted-rho
        // (clippy gate): the struct literal sets the field
        // instead of a later assignment.
        // let mut second = Produce::default();
        // second.hash =
        //     crate::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes(vec![7; 32]);
        let second = Produce {
            hash: crate::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes(vec![7; 32]),
            ..Default::default()
        };
        let expected = vec![
            Event::IoEvent(IOEvent::Produce(first.clone())),
            Event::IoEvent(IOEvent::Produce(second.clone())),
        ];
        let cursor = Arc::new(Mutex::new(OrderedReplayCursor::default()));
        install(&cursor, expected).unwrap();

        assert!(preflight(&cursor, &[ReplayEventRef::Produce(&second)]).is_err());
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 0);
        let mut altered_metadata = first.clone();
        altered_metadata.output_value.push(vec![1]);
        assert!(preflight(&cursor, &[ReplayEventRef::Produce(&altered_metadata)]).is_err());
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 0);
        preflight(&cursor, &[ReplayEventRef::Produce(&first)])
            .unwrap()
            .commit();
        assert!(preflight(&cursor, &[ReplayEventRef::Produce(&first)]).is_err());
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 1);
        preflight(&cursor, &[ReplayEventRef::Produce(&second)])
            .unwrap()
            .commit();
        assert!(cursor.lock().unwrap().is_complete());
    }

    #[test]
    fn checkpoint_restores_only_same_rig_generation() {
        let source = Produce::default();
        let cursor = Arc::new(Mutex::new(OrderedReplayCursor::default()));
        install(&cursor, vec![Event::IoEvent(IOEvent::Produce(source.clone()))]).unwrap();
        let snapshot = cursor.lock().unwrap().checkpoint();
        preflight(&cursor, &[ReplayEventRef::Produce(&source)])
            .unwrap()
            .commit();
        cursor.lock().unwrap().restore(snapshot).unwrap();
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 0);
        install(&cursor, Vec::new()).unwrap();
        assert!(cursor.lock().unwrap().restore(snapshot).is_err());
    }

    // Added by the merge of feature/cost-accounted-rho (finding R2-05): DR-131
    // removed the metered
    // `reentrant_meter_observes_pending_without_deadlocking`. Its claims
    // without a meter stay pinned here. A pending lease refuses a
    // second preflight, a reinstall and a restore. A lease dropped without a
    // commit releases the cursor at the same position.
    #[test]
    fn pending_lease_refuses_reentry_and_drop_releases() {
        let source = Produce::default();
        let cursor = Arc::new(Mutex::new(OrderedReplayCursor::default()));
        install(&cursor, vec![Event::IoEvent(IOEvent::Produce(source.clone()))]).unwrap();
        let snapshot = cursor.lock().unwrap().checkpoint();
        let lease = preflight(&cursor, &[ReplayEventRef::Produce(&source)]).unwrap();
        assert!(cursor.lock().unwrap().is_pending());
        assert!(matches!(
            preflight(&cursor, &[ReplayEventRef::Produce(&source)]),
            Err(RSpaceError::ProduceCommitProtocolViolation)
        ));
        assert_eq!(install(&cursor, Vec::new()), Err(RSpaceError::ProduceCommitProtocolViolation));
        assert_eq!(
            cursor.lock().unwrap().restore(snapshot),
            Err(RSpaceError::ProduceCommitProtocolViolation)
        );
        drop(lease);
        assert!(!cursor.lock().unwrap().is_pending());
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 0);
        preflight(&cursor, &[ReplayEventRef::Produce(&source)])
            .unwrap()
            .commit();
        assert!(cursor.lock().unwrap().is_complete());
    }

    // Added by the merge of feature/cost-accounted-rho (lint fix): the other
    // tests build only `Produce` and `Resting` events. This test builds the two
    // other kinds. A consume must equal the recorded consume. A COMM must equal
    // the recorded COMM in every field of each produce and in each repetition
    // count, so `Produce`'s hash-only equality is not enough.
    #[test]
    fn consume_and_comm_events_require_exact_payloads() {
        let consume = Consume::default();
        let produce = Produce::default();
        let comm = COMM {
            consume: consume.clone(),
            produces: vec![produce.clone()],
            peeks: Default::default(),
            times_repeated: [(produce.clone(), 1)].into_iter().collect(),
        };
        let other_consume = Consume {
            hash: crate::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes(vec![5; 32]),
            ..Default::default()
        };
        let mut repeated_twice = comm.clone();
        repeated_twice.times_repeated.insert(produce.clone(), 2);
        let mut altered_output = comm.clone();
        altered_output.produces[0].output_value.push(vec![1]);
        assert_eq!(altered_output, comm);
        let cursor = Arc::new(Mutex::new(OrderedReplayCursor::default()));
        install(&cursor, vec![
            Event::IoEvent(IOEvent::Consume(consume.clone())),
            Event::Comm(comm.clone()),
        ])
        .unwrap();
        assert!(preflight(&cursor, &[ReplayEventRef::Consume(&other_consume)]).is_err());
        assert!(preflight(&cursor, &[ReplayEventRef::Comm(&comm)]).is_err());
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 0);
        preflight(&cursor, &[ReplayEventRef::Consume(&consume)])
            .unwrap()
            .commit();
        assert!(preflight(&cursor, &[ReplayEventRef::Comm(&repeated_twice)]).is_err());
        assert!(preflight(&cursor, &[ReplayEventRef::Comm(&altered_output)]).is_err());
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 1);
        preflight(&cursor, &[ReplayEventRef::Comm(&comm)])
            .unwrap()
            .commit();
        assert!(cursor.lock().unwrap().is_complete());
    }

    #[test]
    fn resting_event_requires_exact_ordered_data_indices() {
        let first = Produce::default();
        // Changed by the merge of feature/cost-accounted-rho
        // (clippy gate): the struct literal sets the field
        // instead of a later assignment.
        // let mut second = Produce::default();
        // second.hash =
        //     crate::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes(vec![9; 32]);
        let second = Produce {
            hash: crate::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes(vec![9; 32]),
            ..Default::default()
        };
        let expected = RestingCOMM {
            consume: Consume::default(),
            continuation_index: 0,
            ordered_produces: vec![first, second],
            ordered_data_indices: vec![0, 1],
            peeks: Default::default(),
        };
        let mut reversed = expected.clone();
        reversed.ordered_data_indices = vec![1, 0];
        let cursor = Arc::new(Mutex::new(OrderedReplayCursor::default()));
        install(&cursor, vec![Event::RestingComm(Arc::new(expected.clone()))]).unwrap();
        assert!(preflight(&cursor, &[ReplayEventRef::Resting(&reversed)]).is_err());
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 0);
        preflight(&cursor, &[ReplayEventRef::Resting(&expected)])
            .unwrap()
            .commit();
        assert!(cursor.lock().unwrap().is_complete());
    }
}
