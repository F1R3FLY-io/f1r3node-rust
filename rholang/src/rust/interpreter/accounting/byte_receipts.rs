use std::sync::Arc;

use models::rhoapi::CostAuthority;
use thiserror::Error;

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

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ByteMeasurementError {
    #[error("byte measurement history is incomplete")]
    Incomplete,
    #[error("byte measurement entry limit exceeded")]
    EntryLimit,
    #[error("byte measurement total overflow")]
    Overflow,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CheckedByteMeasurements<'a> {
    rows: &'a [Arc<ByteObservation>],
    totals: ByteCharge,
}

impl CheckedByteMeasurements<'_> {
    pub fn rows(&self) -> &[Arc<ByteObservation>] { self.rows }

    pub fn totals(&self) -> ByteCharge { self.totals }
}

impl ByteObservationSnapshot {
    pub fn checked_measurements(
        &self,
        maximum_entries: usize,
    ) -> Result<CheckedByteMeasurements<'_>, ByteMeasurementError> {
        if self.rows.len() > maximum_entries {
            return Err(ByteMeasurementError::EntryLimit);
        }
        if !self.metered_context || self.history_lost {
            return Err(ByteMeasurementError::Incomplete);
        }
        let mut totals = ByteCharge::default();
        for row in &self.rows {
            let raw = row.measurement.ok_or(ByteMeasurementError::Incomplete)?;
            totals.introduction_bytes = totals
                .introduction_bytes
                .checked_add(raw.introduction_bytes)
                .ok_or(ByteMeasurementError::Overflow)?;
            totals.transfer_bytes = totals
                .transfer_bytes
                .checked_add(raw.transfer_bytes)
                .ok_or(ByteMeasurementError::Overflow)?;
            totals.trace_bytes = totals
                .trace_bytes
                .checked_add(raw.trace_bytes)
                .ok_or(ByteMeasurementError::Overflow)?;
        }
        Ok(CheckedByteMeasurements {
            rows: &self.rows,
            totals,
        })
    }

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

#[derive(Debug, Default)]
pub(super) struct ByteObservationLog {
    rows: Vec<Arc<ByteObservation>>,
    history_lost: bool,
    native_capacity: usize,
}

impl Clone for ByteObservationLog {
    fn clone(&self) -> Self {
        Self {
            rows: self.rows.clone(),
            history_lost: self.history_lost,
            native_capacity: self.native_capacity.min(self.rows.len()),
        }
    }
}

impl PartialEq for ByteObservationLog {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows && self.history_lost == other.history_lost
    }
}

impl Eq for ByteObservationLog {}

impl ByteObservationLog {
    pub(super) fn has_complete_history(&self) -> bool { !self.history_lost }

    pub(super) fn rows(&self) -> &[Arc<ByteObservation>] { &self.rows }

    pub(super) fn reserve_native(
        &mut self,
        additional: usize,
        host: &crate::rust::interpreter::host_work::HostWorkBudget,
    ) -> Result<(), crate::rust::interpreter::errors::InterpreterError> {
        super::native_runtime::index::reserve_vector(
            &mut self.rows,
            &mut self.native_capacity,
            additional,
            host,
        )
    }

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
        self.native_capacity = 0;
    }
}

#[cfg(test)]
mod tests;
