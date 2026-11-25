use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::validator::Validator;

use super::types::MultiParentCasperImpl;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;

pub const OWN_HISTORY_WAIT_LIMIT: Duration = Duration::from_secs(120);

#[derive(Debug)]
struct Hold {
    peer_sequences: HashMap<Validator, i32>,
    until: Instant,
}

#[derive(Debug, Default)]
pub struct OwnHistoryGate {
    hold: Mutex<Option<Arc<Hold>>>,
}

impl OwnHistoryGate {
    fn close(&self, peer_sequences: HashMap<Validator, i32>, until: Instant) {
        *self.hold.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(Hold {
            peer_sequences,
            until,
        }));
    }

    pub fn is_closed(&self) -> bool { self.current().is_some() }

    fn current(&self) -> Option<Arc<Hold>> {
        self.hold.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn open(&self) { *self.hold.lock().unwrap_or_else(|e| e.into_inner()) = None; }
}

impl MultiParentCasperImpl {
    pub fn hold_proposals_until_own_history_known(&self) -> Result<bool, CasperError> {
        let Some(identity) = &self.validator_id else {
            return Ok(false);
        };
        let dag = self.block_dag_storage.get_representation()?;
        let mut peer_sequences = HashMap::new();
        for (validator, hash) in dag.latest_message_hashes() {
            if validator == identity.public_key.bytes {
                continue;
            }
            if let Some(latest) = dag.own_testimony(&validator, &hash)? {
                peer_sequences.insert(validator, latest.sequence_number);
            }
        }
        self.own_history_gate
            .close(peer_sequences, Instant::now() + OWN_HISTORY_WAIT_LIMIT);
        tracing::info!(
            "Holding proposals after the state restore until every other active validator \
             that has proposed advances from the restored DAG and cites this validator's \
             latest message (limit {:?})",
            OWN_HISTORY_WAIT_LIMIT
        );
        Ok(true)
    }

    pub(crate) fn awaiting_own_history(
        &self,
        snapshot: &CasperSnapshot,
    ) -> Result<bool, CasperError> {
        let (Some(hold), Some(identity)) = (self.own_history_gate.current(), &self.validator_id)
        else {
            return Ok(false);
        };
        let own = &identity.public_key.bytes;
        let own_latest = snapshot.justifications.iter().find(|j| j.validator == *own);
        let mut awaited = Vec::new();
        for validator in &snapshot.on_chain_state.active_validators {
            if validator == own {
                continue;
            }
            let Some(hash) = snapshot.dag.latest_message_hash(validator) else {
                continue;
            };
            let Some(latest) = snapshot.dag.own_testimony(validator, &hash)? else {
                continue;
            };
            let cited_own = latest.justifications.iter().find(|j| j.validator == *own);
            if latest.is_invalid()
                || hold
                    .peer_sequences
                    .get(validator)
                    .is_some_and(|&restored_seq| latest.sequence_number <= restored_seq)
                || cited_own != own_latest
            {
                awaited.push(validator);
            }
        }
        if awaited.is_empty() {
            self.own_history_gate.open();
            tracing::info!(
                "Every other active validator that has proposed advanced from the restored DAG \
                 and cites this validator's latest message; proposals resume"
            );
            return Ok(false);
        }
        if Instant::now() >= hold.until {
            self.own_history_gate.open();
            tracing::warn!(
                "Proposals resume after {:?} without updated own-history evidence from {}",
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
            "Not proposing: waiting for updated own-history evidence from {}",
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
    use super::*;

    #[test]
    fn the_gate_is_open_until_closed_and_after_it_opens() {
        let gate = OwnHistoryGate::default();
        assert!(!gate.is_closed());
        gate.close(HashMap::new(), Instant::now() + OWN_HISTORY_WAIT_LIMIT);
        assert!(gate.is_closed());
        gate.open();
        assert!(!gate.is_closed());
    }
}
