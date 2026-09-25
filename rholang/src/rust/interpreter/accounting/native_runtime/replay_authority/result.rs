use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use prost::Message;

use super::*;
use crate::rust::interpreter::accounting::byte_receipts::ByteObservationSnapshot;

impl RuntimeBudget {
    pub(crate) fn result_legacy_events(
        &self,
        observations: &ByteObservationSnapshot,
    ) -> Result<Vec<authority::AuthorityByteEvent>, InterpreterError> {
        let host = self
            .authority_state
            .lock()
            .expect("authority state")
            .native
            .as_ref()
            .map(NativeRuntimeConfig::host_work);
        match host {
            None => Ok(observations.legacy_events()),
            Some(host) => bounded_legacy_events(observations, &host),
        }
    }
}

fn bounded_legacy_events(
    observations: &ByteObservationSnapshot,
    host: &HostWorkBudget,
) -> Result<Vec<authority::AuthorityByteEvent>, InterpreterError> {
    work(
        host,
        HostWorkDimension::VerificationOperations,
        observations.rows.len(),
    )?;
    let mut keyed = Vec::new();
    let mut capacity = 0;
    for row in &observations.rows {
        let Some(amount) = row.legacy_amount else {
            continue;
        };
        clone_backing::reserve(&row.authority, host)?;
        let encoded_len = row.authority.encoded_len();
        let length = encoded_len
            .checked_add(49)
            .ok_or(InterpreterError::HostWorkRejected)?;
        work(host, HostWorkDimension::SearchStateBytes, length)?;
        work(host, HostWorkDimension::VerificationBytes, length)?;
        let mut key = Vec::new();
        key.try_reserve_exact(length)
            .map_err(|_| InterpreterError::HostWorkRejected)?;
        key.extend_from_slice(&row.event_id);
        key.push(row.kind.tag());
        key.extend_from_slice(&(encoded_len as u64).to_le_bytes());
        row.authority
            .encode(&mut key)
            .map_err(|error| recording_error(&error.to_string()))?;
        key.extend_from_slice(&amount.to_le_bytes());
        index::reserve_vector(&mut keyed, &mut capacity, 1, host)?;
        keyed.push((key, authority::AuthorityByteEvent {
            event_id: row.event_id,
            kind: row.kind,
            authority: row.authority.clone(),
            amount,
        }));
    }
    let moved_entry = size_of::<(Vec<u8>, authority::AuthorityByteEvent)>();
    work(host, HostWorkDimension::VerificationOperations, keyed.len())?;
    work(
        host,
        HostWorkDimension::VerificationBytes,
        keyed
            .len()
            .checked_mul(moved_entry)
            .ok_or(InterpreterError::HostWorkRejected)?,
    )?;
    shared::rust::fallible_sort::sort(&mut keyed, |left, right| {
        work(host, HostWorkDimension::VerificationOperations, 1)?;
        work(
            host,
            HostWorkDimension::VerificationBytes,
            left.0
                .len()
                .max(right.0.len())
                .checked_add(
                    moved_entry
                        .checked_mul(2)
                        .ok_or(InterpreterError::HostWorkRejected)?,
                )
                .ok_or(InterpreterError::HostWorkRejected)?,
        )?;
        Ok::<_, InterpreterError>(left.0.cmp(&right.0))
    })?;
    work(
        host,
        HostWorkDimension::SearchStateBytes,
        keyed
            .len()
            .checked_mul(size_of::<authority::AuthorityByteEvent>())
            .ok_or(InterpreterError::HostWorkRejected)?,
    )?;
    work(host, HostWorkDimension::VerificationOperations, keyed.len())?;
    let mut events = Vec::new();
    work(
        host,
        HostWorkDimension::VerificationBytes,
        keyed
            .len()
            .checked_mul(size_of::<authority::AuthorityByteEvent>())
            .ok_or(InterpreterError::HostWorkRejected)?,
    )?;
    events
        .try_reserve_exact(keyed.len())
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    events.extend(keyed.into_iter().map(|(_, event)| event));
    Ok(events)
}

#[cfg(test)]
mod tests {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn bounded_legacy_result_preserves_canonical_order(
            rows in prop::collection::vec((any::<u8>(), prop::option::of(any::<u64>()), 0_usize..8), 0..65),
        ) {
            let observations = ByteObservationSnapshot {
                rows: rows.into_iter().map(|(id, amount, owners)| Arc::new(ByteObservation {
                    event_id: [id; 32],
                    kind: AuthorityByteEventKind::Comm,
                    authority: crate::rust::interpreter::accounting::native_runtime::tests::authority(owners),
                    measurement: None,
                    legacy_amount: amount,
                })).collect(),
                metered_context: true,
                history_lost: false,
            };
            let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)));
            prop_assert_eq!(bounded_legacy_events(&observations, &host).unwrap(), observations.legacy_events());
        }
    }
}
