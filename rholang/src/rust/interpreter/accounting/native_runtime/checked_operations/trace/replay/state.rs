#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SlotState {
    Available,
    Reserved(u64),
    Completed,
}

pub(super) struct UndoEntry {
    slot: usize,
    serial: u64,
    usage: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LedgerError {
    Closed,
    Busy,
    Unavailable,
    Serial,
    Checkpoint,
    Incomplete,
}

pub(super) struct ReplayState {
    slots: Vec<SlotState>,
    undo: Vec<UndoEntry>,
    reserved: usize,
    next_serial: u64,
    boundary: bool,
    closed: bool,
    used: u64,
    total: u64,
}

impl ReplayState {
    pub(super) fn ready(
        &self,
        slot: usize,
        predecessors: impl IntoIterator<Item = usize>,
    ) -> Result<bool, LedgerError> {
        if self.closed {
            return Err(LedgerError::Closed);
        }
        if self.slots.get(slot) != Some(&SlotState::Available) {
            return Err(LedgerError::Unavailable);
        }
        Ok(!self.boundary
            && predecessors
                .into_iter()
                .all(|slot| self.slots.get(slot) == Some(&SlotState::Completed)))
    }

    pub(super) fn invalidate(&mut self) { self.closed = true; }

    pub(super) fn reserve_ready(
        &mut self,
        slot: usize,
        predecessors: impl IntoIterator<Item = usize>,
    ) -> Result<Option<u64>, LedgerError> {
        if self.closed {
            return Err(LedgerError::Closed);
        }
        if self.boundary {
            return Err(LedgerError::Busy);
        }
        if !self.ready(slot, predecessors)? {
            return Ok(None);
        }
        self.reserve(slot).map(Some)
    }

    #[cfg(test)]
    pub(super) fn completed_usage(&self) -> u64 { self.used }

    pub(super) fn new(slots: Vec<SlotState>, undo: Vec<UndoEntry>, total: u64) -> Self {
        assert!(undo.is_empty() && undo.capacity() >= slots.len());
        assert!(slots.iter().all(|slot| *slot == SlotState::Available));
        Self {
            slots,
            undo,
            reserved: 0,
            next_serial: 1,
            boundary: false,
            closed: false,
            used: 0,
            total,
        }
    }

    pub(super) fn reserve(&mut self, slot: usize) -> Result<u64, LedgerError> {
        if self.closed {
            return Err(LedgerError::Closed);
        }
        if self.boundary {
            return Err(LedgerError::Busy);
        }
        if self.slots.get(slot) != Some(&SlotState::Available) {
            return Err(LedgerError::Unavailable);
        }
        let serial = self.next_serial;
        let next = serial.checked_add(1).ok_or(LedgerError::Serial)?;
        self.slots[slot] = SlotState::Reserved(serial);
        self.reserved += 1;
        self.next_serial = next;
        Ok(serial)
    }

    pub(super) fn cancel(&mut self, slot: usize, serial: u64) {
        assert_eq!(self.slots[slot], SlotState::Reserved(serial));
        self.slots[slot] = SlotState::Available;
        self.reserved -= 1;
    }

    pub(super) fn publish(&mut self, slot: usize, serial: u64, usage: u64) {
        assert_eq!(self.slots[slot], SlotState::Reserved(serial));
        assert!(self.undo.len() < self.undo.capacity());
        let next = self
            .used
            .checked_add(usage)
            .expect("checked native replay usage");
        assert!(next <= self.total);
        self.undo.push(UndoEntry {
            slot,
            serial,
            usage,
        });
        self.used = next;
        self.slots[slot] = SlotState::Completed;
        self.reserved -= 1;
    }

    pub(super) fn begin_boundary(&mut self) -> Result<(), LedgerError> {
        if self.closed {
            return Err(LedgerError::Closed);
        }
        if self.boundary || self.reserved != 0 {
            return Err(LedgerError::Busy);
        }
        self.boundary = true;
        Ok(())
    }

    pub(super) fn end_boundary(&mut self) {
        assert!(self.boundary);
        self.boundary = false;
    }

    pub(super) fn checkpoint(&self) -> (usize, u64) {
        assert!(self.boundary && self.reserved == 0);
        (
            self.undo.len(),
            self.undo.last().map_or(0, |entry| entry.serial),
        )
    }

    pub(super) fn validate_restore(&self, cursor: usize, serial: u64) -> Result<(), LedgerError> {
        assert!(self.boundary && self.reserved == 0);
        let actual = if cursor == 0 {
            Some(0)
        } else {
            self.undo.get(cursor - 1).map(|entry| entry.serial)
        };
        if actual != Some(serial) {
            return Err(LedgerError::Checkpoint);
        }
        Ok(())
    }

    pub(super) fn restore(&mut self, cursor: usize) {
        assert!(self.boundary && self.reserved == 0 && cursor <= self.undo.len());
        while self.undo.len() > cursor {
            let entry = self.undo.pop().expect("native replay undo suffix");
            assert_eq!(self.slots[entry.slot], SlotState::Completed);
            self.used = self
                .used
                .checked_sub(entry.usage)
                .expect("native replay undo usage");
            self.slots[entry.slot] = SlotState::Available;
        }
    }

    pub(super) fn close(&mut self) {
        assert!(self.boundary && self.reserved == 0);
        self.closed = true;
    }

    pub(super) fn check_complete(&self) -> Result<(), LedgerError> {
        if self.closed {
            return Err(LedgerError::Closed);
        }
        if self.boundary {
            return Err(LedgerError::Busy);
        }
        self.complete_slots()
    }

    pub(super) fn check_complete_at_boundary(&self) -> Result<(), LedgerError> {
        assert!(self.boundary && self.reserved == 0);
        if self.closed {
            return Err(LedgerError::Closed);
        }
        self.complete_slots()
    }

    pub(super) fn completed_usage_at_boundary(&self) -> Result<u64, LedgerError> {
        self.check_complete_at_boundary()?;
        Ok(self.used)
    }

    fn complete_slots(&self) -> Result<(), LedgerError> {
        if self.reserved != 0 || self.undo.len() != self.slots.len() || self.used != self.total {
            return Err(LedgerError::Incomplete);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn readiness_rechecks_restored_predecessors_without_changing_slots() {
        let mut state = ReplayState::new(vec![SlotState::Available; 3], Vec::with_capacity(3), 0);
        assert!(!state.ready(0, [2]).unwrap());
        assert!(state.ready(1, []).unwrap());
        let serial = state.reserve_ready(2, []).unwrap().unwrap();
        assert!(!state.ready(0, [2]).unwrap());
        state.publish(2, serial, 0);
        assert!(state.ready(0, [2]).unwrap());
        state.begin_boundary().unwrap();
        assert!(!state.ready(0, [2]).unwrap());
        state.restore(0);
        state.end_boundary();
        assert!(!state.ready(0, [2]).unwrap());
        assert_eq!(state.slots, vec![SlotState::Available; 3]);
        state.invalidate();
        assert_eq!(state.ready(0, []), Err(LedgerError::Closed));
    }

    proptest! {
        #[test]
        fn readiness_matches_every_predecessor_without_restricting_independent_slots(
            completed in prop::collection::vec(any::<bool>(), 0..64),
            predecessors in prop::collection::vec(any::<usize>(), 0..64),
        ) {
            let count = completed.len();
            let mut state = ReplayState::new(vec![SlotState::Available; count + 1], Vec::with_capacity(count + 1), 0);
            for (slot, complete) in completed.iter().enumerate() {
                if *complete {
                    let serial = state.reserve(slot).unwrap();
                    state.publish(slot, serial, 0);
                }
            }
            let predecessors: Vec<_> = predecessors.iter().map(|index| index % (count + 1)).collect();
            let expected = predecessors.iter().all(|index| completed.get(*index) == Some(&true));
            prop_assert_eq!(state.ready(count, predecessors).unwrap(), expected);
            prop_assert!(state.ready(count, []).unwrap());
            prop_assert_eq!(state.slots[count], SlotState::Available);
        }
    }

    proptest! {
        #[test]
        fn weighted_publication_and_undo_match_the_completed_subset(
            maximum in any::<u64>(),
            amounts in prop::collection::vec(any::<u64>(), 1..32),
            commands in prop::collection::vec((any::<bool>(), any::<usize>()), 0..128),
        ) {
            let mut remaining = maximum;
            let charges: Vec<_> = amounts.into_iter().map(|amount| {
                let charge = amount.min(remaining);
                remaining -= charge;
                charge
            }).collect();
            let total = maximum - remaining;
            let mut state = ReplayState::new(vec![SlotState::Available; charges.len()], Vec::with_capacity(charges.len()), total);
            let mut completed = Vec::new();
            for (restore, choice) in commands {
                if restore {
                    let cursor = choice % (completed.len() + 1);
                    state.begin_boundary().unwrap();
                    state.restore(cursor);
                    state.end_boundary();
                    completed.truncate(cursor);
                } else {
                    let slot = choice % charges.len();
                    if !completed.contains(&slot) {
                        let serial = state.reserve(slot).unwrap();
                        state.publish(slot, serial, charges[slot]);
                        completed.push(slot);
                    }
                }
                let expected: u64 = completed.iter().map(|slot| charges[*slot]).sum();
                prop_assert_eq!(state.completed_usage(), expected);
                prop_assert!(expected <= total);
                prop_assert_eq!(state.check_complete().is_ok(), completed.len() == charges.len());
                state.begin_boundary().unwrap();
                prop_assert_eq!(state.completed_usage_at_boundary(), if completed.len() == charges.len() {
                    Ok(total)
                } else {
                    Err(LedgerError::Incomplete)
                });
                state.end_boundary();
            }
            for (slot, charge) in charges.into_iter().enumerate() {
                if !completed.contains(&slot) {
                    let serial = state.reserve(slot).unwrap();
                    state.publish(slot, serial, charge);
                }
            }
            state.check_complete().unwrap();
            prop_assert_eq!(state.completed_usage(), total);
            state.begin_boundary().unwrap();
            prop_assert_eq!(state.completed_usage_at_boundary(), Ok(total));
            state.close();
            prop_assert_eq!(state.completed_usage_at_boundary(), Err(LedgerError::Closed));
            state.end_boundary();
        }
    }

    #[test]
    fn full_width_usage_and_zero_charge_slots_restore_exactly() {
        let mut state = ReplayState::new(
            vec![SlotState::Available; 3],
            Vec::with_capacity(3),
            u64::MAX,
        );
        for (slot, charge) in [(2, 0), (1, u64::MAX - 1), (0, 1)] {
            let serial = state.reserve(slot).unwrap();
            state.publish(slot, serial, charge);
        }
        state.check_complete().unwrap();
        state.begin_boundary().unwrap();
        state.restore(2);
        assert_eq!(state.completed_usage(), u64::MAX - 1);
        state.restore(1);
        assert_eq!(state.completed_usage(), 0);
        assert_eq!(
            state.check_complete_at_boundary(),
            Err(LedgerError::Incomplete)
        );
        state.restore(0);
        state.end_boundary();
    }

    #[test]
    fn serial_exhaustion_preserves_available_slot() {
        let mut state = ReplayState::new(vec![SlotState::Available], Vec::with_capacity(1), 0);
        state.next_serial = u64::MAX;
        assert_eq!(state.reserve(0), Err(LedgerError::Serial));
        assert_eq!(state.slots, [SlotState::Available]);
        assert_eq!(state.reserved, 0);
        assert!(state.undo.is_empty());
    }
}
