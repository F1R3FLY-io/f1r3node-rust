use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use models::rhoapi::CostAuthority;

use super::*;
use crate::rust::interpreter::accounting::authority::{
    self, AuthorityByteEventKind, ResourceMultiset,
};
use crate::rust::interpreter::accounting::byte_receipts::ByteObservationLog;
use crate::rust::interpreter::accounting::{AuthorityRuntimeEvent, IntroductionRecord};

mod sparse_ledger;

#[derive(Clone)]
pub(super) struct ReplayAuthorityBinding {
    budget: RuntimeBudget,
    generation: Arc<()>,
}

pub(super) struct ReplayAuthorityObservation {
    pub(super) observation: Arc<ByteObservation>,
    pub(super) granted: bool,
    pub(super) retry: bool,
    /// Added by G3 (DR-135, S6): the owner of the introduction could not pay
    /// (R8).
    pub(super) owner_refused: bool,
}

pub(super) struct ReplayAuthorityPublication {
    binding: ReplayAuthorityBinding,
    rows: [Option<Arc<ByteObservation>>; 2],
    event: Option<[u8; 32]>,
    frontier: Option<Arc<ByteObservation>>,
    /// Added by G3 (DR-135, S6, SA5): the identity of an owner-refused
    /// introduction. The publication records it in the owner ledger of the
    /// replay configuration, so `RuntimeBudget::introduction_refused` gives
    /// the answer of play.
    refused: Option<[u8; 32]>,
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
    // Changed by D-F2 (DR-118): a registry entry is an introduction record.
    // introductions: BTreeMap<([u8; 32], AuthorityByteEventKind), CostAuthority>,
    introductions: BTreeMap<([u8; 32], AuthorityByteEventKind), IntroductionRecord>,
}

fn invalid() -> InterpreterError {
    recording_error("native replay authority state differs from the authenticated execution")
}

fn replay_demand(
    authority: &CostAuthority,
) -> Result<ResourceMultiset<[u8; 32]>, InterpreterError> {
    authority::authority_demand(authority).map_err(|error| recording_error(&error.to_string()))
}

impl ReplayAuthorityBinding {
    /// DR-101: the payer and deployment that resolve residue seals.
    pub(super) fn residue_context(&self) -> Result<authority::ResidueContext, InterpreterError> {
        // Changed by G3 (DR-135): a resolved payer region carries the region
        // price, as in play.
        // authority::ResidueContext::new(&self.budget.signature(), self.budget.deploy_id())
        //     .map_err(|error| recording_error(&error.to_string()))
        authority::ResidueContext::new(&self.budget.signature(), self.budget.deploy_id())
            .map(|context| context.with_price(self.budget.region_price()))
            .map_err(|error| recording_error(&error.to_string()))
    }

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
        let mut rows = [None, None];
        let mut event = None;
        let mut frontier = None;
        // Added by G3 (DR-135, S6, SA5): the identity of an owner-refused
        // introduction.
        let mut refused = None;
        for (slot, row) in observed.into_iter().enumerate() {
            let Some(row) = row else { continue };
            // Added by G3 (DR-135, S6, SA5): an owner-refused introduction is
            // not granted, so it adds no row and no event.
            if row.owner_refused {
                if row.granted || row.retry || row.observation.kind == AuthorityByteEventKind::Comm
                {
                    return Err(invalid());
                }
                refused = Some(row.observation.event_id);
            }
            if row.observation.kind == AuthorityByteEventKind::Comm {
                // Changed by C14 (DR-79): the lookup is charged at the live sizes
                // of the three identity maps.
                // backing::reserve_event_lookup(&host)?;
                // Disabled by D-O5 (DR-89): only the retry branch compares the
                // recorded observation, and only the (None, false) branch copies
                // the authority; each branch reserves its own reads and copies.
                // clone_backing::inspect(row.observation.as_ref(), &host)?;
                // clone_backing::reserve_copy_and_cleanup(&row.observation.authority, &host)?;
                let id = row.observation.event_id;
                if state.pending_stack_event_ids.contains(&id)
                    || state.pending_replay_events.contains_key(&id)
                {
                    return Err(invalid());
                }
                match (state.events.get(&id), row.retry) {
                    (Some(existing), true) if row.granted => {
                        let saved = existing.byte_observation.as_deref().ok_or_else(invalid)?;
                        // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
                        // clone_backing::inspect(saved, &host)?;
                        // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
                        // clone_backing::inspect(row.observation.as_ref(), &host)?;
                        if saved != row.observation.as_ref() {
                            return Err(invalid());
                        }
                    }
                    (None, false) => {
                        // D-O5 (DR-89): the granted event copies the authority now;
                        // a frontier row copies it at publication. Both keep a
                        // shared pointer to the recorded observation.
                        // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
                        // clone_backing::reserve_copy_and_cleanup(&row.observation.authority, &host)?;
                        // clone_backing::reserve(&row.observation, &host)?;
                        if row.granted {
                            let demand = replay_demand(&row.observation.authority)?;
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
        state.byte_observations.reserve(pending_rows);
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
            // Added by G3 (DR-135, S6, SA5).
            refused,
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
        // Added by G3 (DR-135, S6, SA5): the ledger of the replay
        // configuration records the owner-refused introduction, as the ledger
        // of play does when it refuses it.
        if let (Some(id), Some(owners)) = (
            self.refused.take(),
            state
                .native
                .as_mut()
                .and_then(|native| native.reservation.owners_mut()),
        ) {
            owners.refuse(id);
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
        // Changed by D-O1 (DR-110): block accounting charges inline bytes once
        // per enclosing block. The events and the rows hold shared byte
        // observations whose payloads were prepaid at birth, so the cleanup of
        // their copies visits each shared pointer but not its payload (the C5
        // rule, DR-83).
        // clone_backing::reserve_copy_and_cleanup(&state.events, &host)?;
        // clone_backing::reserve_slice_copy_and_cleanup(state.byte_observations.rows(), &host)?;
        // clone_backing::reserve_copy_and_cleanup(&state.realized, &host)?;
        // clone_backing::reserve_copy_and_cleanup(&state.reserved, &host)?;
        // clone_backing::reserve_copy_and_cleanup(&state.frontier, &host)?;
        // clone_backing::reserve_copy_and_cleanup(&state.stack_births, &host)?;
        // clone_backing::reserve_copy_and_cleanup(&*introductions, &host)?;
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
