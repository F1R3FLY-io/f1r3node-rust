//! Protocol-7 replay ordering. The old IO-to-COMM multiset remains the
//! candidate index; it is not evidence that the next global event is legal.

use std::sync::{Arc, Mutex, PoisonError};

use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::native_source::SourceMeter;
use crate::rspace::native_backing;
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

pub(super) enum ReplayEventRef<'a> {
    Produce(&'a Produce),
    Consume(&'a Consume),
    Comm(&'a COMM),
    Resting(&'a RestingCOMM),
}

impl ReplayEventRef<'_> {
    fn inspect(&self, meter: &dyn SourceMeter) -> Result<(), RSpaceError> {
        match self {
            Self::Produce(value) => native_backing::inspect_blocks(*value, meter),
            Self::Consume(value) => native_backing::inspect_blocks(*value, meter),
            Self::Comm(value) => native_backing::inspect_blocks(*value, meter),
            Self::Resting(value) => native_backing::inspect_blocks(*value, meter),
        }
    }

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

/// Charge and inspect outside the cursor lock. A hostile or reentrant meter
/// may invoke replay; it can never deadlock on this lock or install a cursor
/// after another rig changed the generation.
pub(super) fn install(
    cursor: &Arc<Mutex<OrderedReplayCursor>>,
    log: Log,
    meter: &(dyn SourceMeter + Send + Sync),
) -> Result<(), RSpaceError> {
    let (previous, generation, position) = {
        let guard = cursor.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.pending {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        (guard.expected.clone(), guard.generation, guard.next)
    };
    native_backing::inspect_blocks_slice(&log, meter)?;
    if let Some(previous) = &previous {
        native_backing::reserve_blocks_cleanup(previous.as_ref(), meter)?;
    }
    let allocation = shared::rust::clone_backing::arc_allocation_bytes::<Log>()
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(1, allocation, allocation)?;
    let next_generation = generation
        .checked_add(1)
        .ok_or(RSpaceError::ProduceCommitProtocolViolation)?;
    let mut guard = cursor.lock().unwrap_or_else(PoisonError::into_inner);
    if guard.pending || guard.generation != generation || guard.next != position {
        return Err(RSpaceError::ProduceCommitProtocolViolation);
    }
    guard.expected = Some(Arc::new(log));
    guard.next = 0;
    guard.generation = next_generation;
    Ok(())
}

#[must_use = "the preflight lease must be committed only after its store transition"]
pub(super) struct CursorLease {
    cursor: Arc<Mutex<OrderedReplayCursor>>,
    generation: u64,
    start: usize,
    count: usize,
    committed: bool,
}

impl CursorLease {
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

pub(super) fn preflight(
    cursor: &Arc<Mutex<OrderedReplayCursor>>,
    events: &[ReplayEventRef<'_>],
    meter: &(dyn SourceMeter + Send + Sync),
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
        native_backing::inspect_blocks(expected, meter)?;
        actual.inspect(meter)?;
        meter.reserve(1, 0, 0)?;
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
        let free = |_: usize, _: usize, _: usize| Ok(());
        let first = Produce::default();
        let mut second = Produce::default();
        second.hash =
            crate::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes(vec![7; 32]);
        let expected = vec![
            Event::IoEvent(IOEvent::Produce(first.clone())),
            Event::IoEvent(IOEvent::Produce(second.clone())),
        ];
        let cursor = Arc::new(Mutex::new(OrderedReplayCursor::default()));
        install(&cursor, expected, &free).unwrap();

        let reject = |_: usize, _: usize, _: usize| Err(RSpaceError::HostWorkRejected);
        assert!(matches!(
            preflight(&cursor, &[ReplayEventRef::Produce(&first)], &reject),
            Err(RSpaceError::HostWorkRejected)
        ));
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 0);

        assert!(preflight(&cursor, &[ReplayEventRef::Produce(&second)], &free).is_err());
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 0);
        let mut altered_metadata = first.clone();
        altered_metadata.output_value.push(vec![1]);
        assert!(preflight(&cursor, &[ReplayEventRef::Produce(&altered_metadata)], &free).is_err());
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 0);
        preflight(&cursor, &[ReplayEventRef::Produce(&first)], &free)
            .unwrap()
            .commit();
        assert!(preflight(&cursor, &[ReplayEventRef::Produce(&first)], &free).is_err());
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 1);
        preflight(&cursor, &[ReplayEventRef::Produce(&second)], &free)
            .unwrap()
            .commit();
        assert!(cursor.lock().unwrap().is_complete());
    }

    #[test]
    fn checkpoint_restores_only_same_rig_generation() {
        let free = |_: usize, _: usize, _: usize| Ok(());
        let source = Produce::default();
        let cursor = Arc::new(Mutex::new(OrderedReplayCursor::default()));
        install(&cursor, vec![Event::IoEvent(IOEvent::Produce(source.clone()))], &free).unwrap();
        let snapshot = cursor.lock().unwrap().checkpoint();
        preflight(&cursor, &[ReplayEventRef::Produce(&source)], &free)
            .unwrap()
            .commit();
        cursor.lock().unwrap().restore(snapshot).unwrap();
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 0);
        install(&cursor, Vec::new(), &free).unwrap();
        assert!(cursor.lock().unwrap().restore(snapshot).is_err());
    }

    #[test]
    fn reentrant_meter_observes_pending_without_deadlocking() {
        let free = |_: usize, _: usize, _: usize| Ok(());
        let source = Produce::default();
        let cursor = Arc::new(Mutex::new(OrderedReplayCursor::default()));
        install(&cursor, vec![Event::IoEvent(IOEvent::Produce(source.clone()))], &free).unwrap();
        let reentrant = |_: usize, _: usize, _: usize| {
            let nested = preflight(&cursor, &[ReplayEventRef::Produce(&source)], &free)?;
            drop(nested);
            Ok(())
        };
        let lease = preflight(&cursor, &[ReplayEventRef::Produce(&source)], &reentrant).unwrap();
        assert!(preflight(&cursor, &[ReplayEventRef::Produce(&source)], &free).is_err());
        lease.commit();
        assert!(cursor.lock().unwrap().is_complete());
    }

    #[test]
    fn resting_event_requires_exact_ordered_data_indices() {
        let free = |_: usize, _: usize, _: usize| Ok(());
        let first = Produce::default();
        let mut second = Produce::default();
        second.hash =
            crate::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes(vec![9; 32]);
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
        install(&cursor, vec![Event::RestingComm(Arc::new(expected.clone()))], &free).unwrap();
        assert!(preflight(&cursor, &[ReplayEventRef::Resting(&reversed)], &free).is_err());
        assert_eq!(cursor.lock().unwrap().checkpoint().unwrap().next, 0);
        preflight(&cursor, &[ReplayEventRef::Resting(&expected)], &free)
            .unwrap()
            .commit();
        assert!(cursor.lock().unwrap().is_complete());
    }
}
