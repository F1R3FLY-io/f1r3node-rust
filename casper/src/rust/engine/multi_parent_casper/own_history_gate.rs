use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::validator::Validator;

use super::types::MultiParentCasperImpl;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;

pub const OWN_HISTORY_WAIT_LIMIT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy)]
struct Hold {
    since_millis: i64,
    until: Instant,
}

#[derive(Debug, Default)]
pub struct OwnHistoryGate {
    hold: Mutex<Option<Hold>>,
}

impl OwnHistoryGate {
    pub fn close(&self, since_millis: i64, until: Instant) {
        *self.hold.lock().unwrap_or_else(|e| e.into_inner()) = Some(Hold {
            since_millis,
            until,
        });
    }

    pub fn is_closed(&self) -> bool { self.current().is_some() }

    fn current(&self) -> Option<Hold> { *self.hold.lock().unwrap_or_else(|e| e.into_inner()) }

    fn open(&self) { *self.hold.lock().unwrap_or_else(|e| e.into_inner()) = None; }
}

fn awaited_validators(
    since_millis: i64,
    latest_timestamps: impl IntoIterator<Item = (Validator, i64)>,
) -> Vec<Validator> {
    latest_timestamps
        .into_iter()
        .filter(|(_, timestamp)| *timestamp < since_millis)
        .map(|(validator, _)| validator)
        .collect()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl MultiParentCasperImpl {
    pub fn hold_proposals_until_own_history_known(&self) -> bool {
        if self.validator_id.is_none() {
            return false;
        }
        self.own_history_gate
            .close(now_millis(), Instant::now() + OWN_HISTORY_WAIT_LIMIT);
        tracing::info!(
            "Holding proposals after the state restore until every other active validator \
             that has proposed has a block created after now, so this validator's own \
             latest block is known (limit {:?})",
            OWN_HISTORY_WAIT_LIMIT
        );
        true
    }

    pub(crate) fn awaiting_own_history(
        &self,
        snapshot: &CasperSnapshot,
    ) -> Result<bool, CasperError> {
        let Some(hold) = self.own_history_gate.current() else {
            return Ok(false);
        };
        let own: Option<Validator> = self
            .validator_id
            .as_ref()
            .map(|identity| identity.public_key.bytes.clone());
        let mut latest_timestamps = Vec::new();
        for validator in &snapshot.on_chain_state.active_validators {
            if Some(validator) == own.as_ref() {
                continue;
            }
            let Some(hash) = snapshot.dag.latest_message_hash(validator) else {
                continue;
            };
            if snapshot.dag.own_testimony(validator, &hash)?.is_none() {
                continue;
            }
            let timestamp = self
                .block_store
                .get(&hash)?
                .map_or(i64::MIN, |block| block.header.timestamp);
            latest_timestamps.push((validator.clone(), timestamp));
        }
        let awaited = awaited_validators(hold.since_millis, latest_timestamps);
        if awaited.is_empty() {
            self.own_history_gate.open();
            tracing::info!(
                "Every other active validator that has proposed has a block created after the \
                 state restore; proposals resume"
            );
            return Ok(false);
        }
        if Instant::now() >= hold.until {
            self.own_history_gate.open();
            tracing::warn!(
                "Proposals resume after {:?} without a block created after the state restore \
                 from {}",
                OWN_HISTORY_WAIT_LIMIT,
                awaited
                    .iter()
                    .map(|v| PrettyPrinter::build_string_bytes(v))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return Ok(false);
        }
        tracing::info!(
            "Not proposing: waiting for a block created after the state restore from {}",
            awaited
                .iter()
                .map(|v| PrettyPrinter::build_string_bytes(v))
                .collect::<Vec<_>>()
                .join(", ")
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;

    fn validator(tag: u8) -> Validator { Bytes::from(vec![tag; 4]) }

    #[test]
    fn a_validator_without_a_block_after_the_restore_is_awaited() {
        let awaited = awaited_validators(1_000, vec![
            (validator(1), 1_000),
            (validator(2), 999),
            (validator(3), i64::MIN),
            (validator(4), 1_500),
        ]);
        assert_eq!(awaited, vec![validator(2), validator(3)]);
    }

    #[test]
    fn the_gate_is_open_until_closed_and_after_it_opens() {
        let gate = OwnHistoryGate::default();
        assert!(!gate.is_closed());
        gate.close(1_000, Instant::now() + OWN_HISTORY_WAIT_LIMIT);
        assert!(gate.is_closed());
        gate.open();
        assert!(!gate.is_closed());
    }
}
