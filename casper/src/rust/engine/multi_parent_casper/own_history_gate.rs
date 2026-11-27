use std::fs::File;
use std::io::{ErrorKind, Read};

use models::rust::validator::Validator;
use uuid::Uuid;

use super::types::MultiParentCasperImpl;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;

impl MultiParentCasperImpl {
    pub(crate) fn awaiting_own_history(
        &self,
        snapshot: &CasperSnapshot,
        validator: &Validator,
    ) -> Result<bool, CasperError> {
        let Some(generation) = self.block_store.pending_validator_recovery(validator)? else {
            return Ok(false);
        };
        let recovery_id = Uuid::from_bytes(generation);
        let genesis = hex::encode(&snapshot.chain_id);
        let key = hex::encode(validator);
        let Some(dir) = self.block_store.validator_recovery_dir() else {
            return Ok(true);
        };
        let path = dir.join(format!("validator-recovery-{recovery_id}.confirm"));
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                tracing::info!(%recovery_id, %genesis, validator = %key, file = %path.display(),
                    "Proposals held pending operator confirmation of signing history");
                return Ok(true);
            }
            Err(e) => {
                return Err(CasperError::RuntimeError(format!(
                    "Reading {}: {e}",
                    path.display()
                )))
            }
        };
        let mut input = Vec::new();
        file.take(1025)
            .read_to_end(&mut input)
            .map_err(|e| CasperError::RuntimeError(format!("Reading {}: {e}", path.display())))?;
        let fields: Vec<_> = std::str::from_utf8(&input)
            .unwrap_or("")
            .split_ascii_whitespace()
            .collect();
        let latest = snapshot.dag.latest_message_hash(validator);
        let sequence = snapshot.max_seq_nums.get(validator);
        let matches = input.len() <= 1024
            && match fields.as_slice() {
                [network, signer, "first-use"] if *network == genesis && *signer == key => {
                    sequence.is_none()
                        && latest
                            .as_ref()
                            .is_none_or(|hash| hash == &snapshot.chain_id)
                }
                [network, signer, seq, hash] if *network == genesis && *signer == key => {
                    if let (Some(latest), Some(sequence)) = (latest, sequence) {
                        seq.parse::<u64>().ok() == Some(*sequence)
                            && *hash == hex::encode(&latest)
                            && snapshot
                                .justifications
                                .iter()
                                .any(|j| j.validator == *validator && j.latest_block_hash == latest)
                            && snapshot
                                .dag
                                .own_testimony(validator, &latest)?
                                .is_some_and(|m| {
                                    !m.is_invalid()
                                        && u64::try_from(m.sequence_number).ok() == Some(*sequence)
                                })
                            && self.block_store.disk_contains(&latest)?
                    } else {
                        false
                    }
                }
                _ => false,
            };
        if !matches {
            tracing::warn!(%recovery_id, %genesis, validator = %key,
                "Proposals held: confirmation does not match the validator's local signing history");
            return Ok(true);
        }
        self.block_store
            .confirm_validator_recovery(validator, &generation)?;
        tracing::info!(%recovery_id, %genesis, validator = %key, "Accepted operator confirmation of signing history");
        Ok(false)
    }
}
