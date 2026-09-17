use std::sync::Arc;

use models::rhoapi::CostAuthority;

use super::authority::{AuthorityByteEvent, AuthorityByteEventKind};
use super::byte_accounting::ByteCharge;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ByteObservation {
    pub event_id: [u8; 32],
    pub kind: AuthorityByteEventKind,
    pub authority: CostAuthority,
    pub measurement: Option<ByteCharge>,
    pub legacy_amount: Option<u64>,
}

impl ByteObservation {
    pub fn legacy_event(&self) -> Option<AuthorityByteEvent> {
        self.legacy_amount.map(|amount| AuthorityByteEvent {
            event_id: self.event_id,
            kind: self.kind,
            authority: self.authority.clone(),
            amount,
        })
    }

    pub(super) fn accepts_retry(&self, requested: &Self) -> bool {
        requested.measurement.is_none() || self == requested
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ByteObservationSnapshot {
    pub rows: Vec<Arc<ByteObservation>>,
    pub metered_context: bool,
    pub history_lost: bool,
}

impl ByteObservationSnapshot {
    pub fn has_complete_measurements(&self) -> bool {
        self.metered_context
            && !self.history_lost
            && self.rows.iter().all(|row| row.measurement.is_some())
    }

    pub fn legacy_events(&self) -> Vec<AuthorityByteEvent> {
        let mut events: Vec<_> = self
            .rows
            .iter()
            .filter_map(|row| row.legacy_event())
            .collect();
        events.sort_by_key(AuthorityByteEvent::canonical_key);
        events
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ByteObservationLog {
    rows: Vec<Arc<ByteObservation>>,
    history_lost: bool,
}

impl ByteObservationLog {
    pub(super) fn try_reserve(
        &mut self,
        additional: usize,
    ) -> Result<(), std::collections::TryReserveError> {
        self.rows.try_reserve(additional)
    }

    pub(super) fn push(&mut self, row: Arc<ByteObservation>) { self.rows.push(row); }

    pub(super) fn snapshot(&self, metered_context: bool) -> ByteObservationSnapshot {
        ByteObservationSnapshot {
            rows: self.rows.clone(),
            metered_context,
            history_lost: self.history_lost,
        }
    }

    pub(super) fn clear_partial_history(&mut self, retained_identities: bool) {
        self.history_lost |= retained_identities || !self.rows.is_empty();
        self.rows.clear();
    }

    pub(super) fn mark_incomplete(&mut self) { self.history_lost = true; }

    pub(super) fn reset(&mut self) {
        self.rows.clear();
        self.history_lost = false;
    }
}

#[cfg(test)]
mod tests;
