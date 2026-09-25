use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use models::rhoapi::CostAuthority;

use super::*;
use crate::rust::interpreter::accounting::authority::{
    self, AuthorityByteEventKind, ResourceMultiset,
};
use crate::rust::interpreter::accounting::byte_receipts::ByteObservationLog;
use crate::rust::interpreter::accounting::AuthorityRuntimeEvent;

mod sparse_ledger;
mod backing;
mod result;

#[derive(Clone)]
pub(super) struct ReplayAuthorityBinding {
    budget: RuntimeBudget,
    generation: Arc<()>,
}

pub(super) struct ReplayAuthorityObservation {
    pub(super) observation: Arc<ByteObservation>,
    pub(super) granted: bool,
    pub(super) retry: bool,
}

pub(super) struct ReplayAuthorityPublication {
    binding: ReplayAuthorityBinding,
    rows: [Option<Arc<ByteObservation>>; 2],
    event: Option<[u8; 32]>,
    frontier: Option<Arc<ByteObservation>>,
    active: bool,
}

pub(crate) struct NativeAuthorityCheckpoint {
    generation: Arc<()>,
    events: BTreeMap<[u8; 32], AuthorityRuntimeEvent>,
    observations: ByteObservationLog,
    realized: ResourceMultiset<[u8; 32]>,
    reserved: ResourceMultiset<[u8; 32]>,
    frontier: BTreeMap<[u8; 32], CostAuthority>,
    births: BTreeMap<[u8; 32], authority::AuthorityStackBirth>,
    introductions: BTreeMap<([u8; 32], AuthorityByteEventKind), CostAuthority>,
}

fn invalid() -> InterpreterError {
    recording_error("native replay authority state differs from the authenticated execution")
}

impl ReplayAuthorityBinding {
    pub(super) fn new(budget: RuntimeBudget, session: [u8; 32]) -> Result<Self, InterpreterError> {
        let mut state = budget.authority_state.lock().expect("authority state");
        let native = state.native.as_ref().ok_or_else(invalid)?;
        if native.replay_bound
            || native.session != session
            || budget.has_comm_accounting_scope()
            || budget.is_unmetered()
            || !state.events.is_empty()
            || !state.stack_births.is_empty()
            || !state.pending_stack_transfers.is_empty()
            || !state.pending_replay_events.is_empty()
            || state.pending_replay_rows != 0
            || !native.recording.attempts.is_empty()
            || !native.recording.retries.is_empty()
        {
            return Err(invalid());
        }
        let generation = Arc::clone(&native.generation);
        state
            .native
            .as_mut()
            .expect("checked native configuration")
            .replay_bound = true;
        drop(state);
        Ok(Self { budget, generation })
    }

    pub(super) fn prepare(
        &self,
        observed: [Option<ReplayAuthorityObservation>; 2],
    ) -> Result<ReplayAuthorityPublication, InterpreterError> {
        let mut state = self.budget.authority_state.lock().expect("authority state");
        if !state
            .native
            .as_ref()
            .is_some_and(|native| Arc::ptr_eq(&native.generation, &self.generation))
            || !self.budget.has_comm_accounting_scope()
            || self.budget.is_unmetered()
        {
            return Err(invalid());
        }
        let host = state
            .native
            .as_ref()
            .expect("checked native configuration")
            .host_work();
        let mut rows = [None, None];
        let mut event = None;
        let mut frontier = None;
        for (slot, row) in observed.into_iter().enumerate() {
            let Some(row) = row else { continue };
            if row.observation.kind == AuthorityByteEventKind::Comm {
                backing::reserve_event_lookup(&host)?;
                clone_backing::reserve(&row.observation.authority, &host)?;
                let id = row.observation.event_id;
                if state.pending_stack_event_ids.contains(&id)
                    || state.pending_replay_events.contains_key(&id)
                {
                    return Err(invalid());
                }
                match (state.events.get(&id), row.retry) {
                    (Some(existing), true)
                        if row.granted
                            && existing.byte_observation.as_deref()
                                == Some(row.observation.as_ref()) => {}
                    (None, false) => {
                        if row.granted {
                            let demand = authority::authority_demand(&row.observation.authority)
                                .map_err(|error| recording_error(&error.to_string()))?;
                            event = Some((id, AuthorityRuntimeEvent {
                                authority: row.observation.authority.clone(),
                                debit: demand,
                                byte_observation: Some(Arc::clone(&row.observation)),
                            }));
                        } else {
                            frontier = Some(Arc::clone(&row.observation));
                        }
                    }
                    _ => return Err(invalid()),
                }
            }
            if row.granted && !row.retry {
                rows[slot] = Some(row.observation);
            }
        }
        backing::reserve_changes(
            &state,
            event.as_ref().map(|(_, event)| &event.debit),
            frontier.is_some(),
            &host,
        )?;
        if let Some((_, event)) = &event {
            sparse_ledger::validate_add(
                &state.reserved.0,
                &event.debit.0,
                state.enforce_allocation.then_some(&state.allocation.0),
            )
            .map_err(|_| invalid())?;
        } else if state.enforce_allocation && !state.allocation.dominates(&state.reserved) {
            return Err(invalid());
        }
        let count = rows.iter().flatten().count();
        let pending_rows = state
            .pending_replay_rows
            .checked_add(count)
            .ok_or_else(invalid)?;
        state
            .byte_observations
            .reserve_native(pending_rows, &host)?;
        let event_id = event.map(|(id, event)| {
            sparse_ledger::add_assign(&mut state.reserved.0, &event.debit.0)
                .expect("validated replay authority reservation");
            state.pending_replay_events.insert(id, event);
            id
        });
        state.pending_replay_rows = pending_rows;
        Ok(ReplayAuthorityPublication {
            binding: self.clone(),
            rows,
            event: event_id,
            frontier,
            active: true,
        })
    }
}

#[cfg(test)]
mod tests;

impl ReplayAuthorityPublication {
    pub(super) fn publish(&mut self) {
        assert!(self.active);
        let mut state = self
            .binding
            .budget
            .authority_state
            .lock()
            .expect("authority state");
        assert!(state
            .native
            .as_ref()
            .is_some_and(|native| Arc::ptr_eq(&native.generation, &self.binding.generation)));
        if let Some(id) = self.event.take() {
            let event = state
                .pending_replay_events
                .remove(&id)
                .expect("reserved replay authority event");
            sparse_ledger::add_assign(&mut state.realized.0, &event.debit.0)
                .expect("reserved replay authority demand");
            assert!(state.events.insert(id, event).is_none());
        }
        for row in &mut self.rows {
            if let Some(row) = row.take() {
                state.byte_observations.push(row);
                state.pending_replay_rows -= 1;
            }
        }
        if let Some(row) = self.frontier.take() {
            state.frontier.insert(row.event_id, row.authority.clone());
        }
        self.active = false;
    }
}

impl Drop for ReplayAuthorityPublication {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = self
            .binding
            .budget
            .authority_state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !state
            .native
            .as_ref()
            .is_some_and(|native| Arc::ptr_eq(&native.generation, &self.binding.generation))
        {
            return;
        }
        if let Some(id) = self.event {
            let event = state
                .pending_replay_events
                .remove(&id)
                .expect("reserved replay authority event");
            sparse_ledger::sub_assign(&mut state.reserved.0, &event.debit.0)
                .expect("reserved replay authority demand");
        }
        state.pending_replay_rows -= self.rows.iter().flatten().count();
    }
}

impl RuntimeBudget {
    pub(crate) fn has_exclusive_authority_owner(&self) -> bool {
        Arc::strong_count(&self.authority_state) == 1
    }

    pub(crate) fn native_authority_checkpoint(
        &self,
    ) -> Result<NativeAuthorityCheckpoint, InterpreterError> {
        let introductions = self
            .introduction_authorities
            .lock()
            .expect("introduction authority map");
        let state = self.authority_state.lock().expect("authority state");
        let native = state.native.as_ref().ok_or_else(invalid)?;
        if self.has_comm_accounting_scope()
            || self.is_unmetered()
            || !state.pending_stack_transfers.is_empty()
            || !state.pending_replay_events.is_empty()
            || state.pending_replay_rows != 0
        {
            return Err(invalid());
        }
        let host = native.host_work();
        clone_backing::reserve(&state.events, &host)?;
        clone_backing::reserve_slice(state.byte_observations.rows(), &host)?;
        clone_backing::reserve(&state.realized, &host)?;
        clone_backing::reserve(&state.reserved, &host)?;
        clone_backing::reserve(&state.frontier, &host)?;
        clone_backing::reserve(&state.stack_births, &host)?;
        clone_backing::reserve(&*introductions, &host)?;
        Ok(NativeAuthorityCheckpoint {
            generation: Arc::clone(&native.generation),
            events: state.events.clone(),
            observations: state.byte_observations.clone(),
            realized: state.realized.clone(),
            reserved: state.reserved.clone(),
            frontier: state.frontier.clone(),
            births: state.stack_births.clone(),
            introductions: introductions.clone(),
        })
    }

    pub(crate) fn reserve_native_result_backing(&self) -> Result<(), InterpreterError> {
        let state = self.authority_state.lock().expect("authority state");
        let Some(native) = &state.native else {
            return Ok(());
        };
        let host = native.host_work();
        clone_backing::reserve(&state.events, &host)?;
        clone_backing::reserve(&state.realized, &host)?;
        clone_backing::reserve(&state.stack_births, &host)?;
        clone_backing::reserve_slice(state.byte_observations.rows(), &host)?;
        backing::reserve_result_vectors(&state, &host)
    }

    pub(crate) fn restore_native_authority(
        &self,
        checkpoint: NativeAuthorityCheckpoint,
    ) -> Result<(), InterpreterError> {
        let mut introductions = self
            .introduction_authorities
            .lock()
            .expect("introduction authority map");
        let mut state = self.authority_state.lock().expect("authority state");
        if self.has_comm_accounting_scope()
            || self.unmetered.load(Ordering::Acquire) != 0
            || !state
                .native
                .as_ref()
                .is_some_and(|native| Arc::ptr_eq(&native.generation, &checkpoint.generation))
            || !state.pending_stack_transfers.is_empty()
            || !state.pending_replay_events.is_empty()
            || state.pending_replay_rows != 0
        {
            return Err(invalid());
        }
        state.events = checkpoint.events;
        state.byte_observations = checkpoint.observations;
        state.realized = checkpoint.realized;
        state.reserved = checkpoint.reserved;
        state.frontier = checkpoint.frontier;
        state.stack_births = checkpoint.births;
        *introductions = checkpoint.introductions;
        Ok(())
    }
}
