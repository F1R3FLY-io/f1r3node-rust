use std::cmp::Ordering;

use models::rust::deploy_id::DeployIdV6;
use serde::{Deserialize, Serialize};

use super::deploy_lifecycle_types::TerminalState;

pub const DEPLOY_OCCURRENCE_SCHEMA_VERSION: u32 = 1;
pub const DEPLOY_OCCURRENCE_PROTOCOL_VERSION: i64 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OccurrenceAdmissionMode {
    Normal,
    SettledHistory,
    ApprovedGenesis,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployOccurrence {
    pub schema_version: u32,
    pub deploy_id: DeployIdV6,
    pub protocol_version: i64,
    pub source_block_hash: [u8; 32],
    pub source_block_height: i64,
    pub source_validator: Vec<u8>,
    pub deploy_ordinal: u32,
    pub admission_mode: OccurrenceAdmissionMode,
    pub admission_ruleset_digest: Vec<u8>,
    pub admission_context_digest: Vec<u8>,
    pub sender_authority_digest: Vec<u8>,
    pub is_failed: bool,
}

impl DeployOccurrence {
    pub fn rank_cmp(&self, other: &Self) -> Ordering {
        self.source_block_height
            .cmp(&other.source_block_height)
            .then_with(|| other.source_block_hash.cmp(&self.source_block_hash))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != DEPLOY_OCCURRENCE_SCHEMA_VERSION {
            return Err(format!(
                "unsupported deploy occurrence schema {}",
                self.schema_version
            ));
        }
        if self.protocol_version != DEPLOY_OCCURRENCE_PROTOCOL_VERSION {
            return Err(format!(
                "unsupported deploy occurrence protocol {}",
                self.protocol_version
            ));
        }
        if self.source_block_height < 0 {
            return Err("deploy occurrence source height must be nonnegative".to_string());
        }
        match self.admission_mode {
            OccurrenceAdmissionMode::ApprovedGenesis => {
                if self.source_block_height != 0
                    || !self.source_validator.is_empty()
                    || !self.admission_ruleset_digest.is_empty()
                    || !self.admission_context_digest.is_empty()
                    || !self.sender_authority_digest.is_empty()
                    || self.is_failed
                {
                    return Err(
                        "approved-genesis occurrence has non-genesis admission metadata"
                            .to_string(),
                    );
                }
            }
            OccurrenceAdmissionMode::Normal | OccurrenceAdmissionMode::SettledHistory => {
                if self.source_validator.len() != models::rust::validator::LENGTH {
                    return Err(
                        "deploy occurrence source validator has an invalid length".to_string()
                    );
                }
                if self.admission_ruleset_digest.len() != 32
                    || self.admission_context_digest.len() != 32
                    || self.sender_authority_digest.len() != 32
                {
                    return Err(
                        "deploy occurrence admission digest has an invalid length".to_string()
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenOccurrenceSummary {
    pub schema_version: u32,
    pub deploy_id: DeployIdV6,
    pub canonical: DeployOccurrence,
    pub archive_count: u64,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalOccurrenceSummary {
    pub schema_version: u32,
    pub deploy_id: DeployIdV6,
    pub terminal_state: TerminalState,
    pub frozen_source: DeployOccurrence,
    pub current_representative: DeployOccurrence,
    pub rejection_count: u32,
    pub archive_count: u64,
    pub archive_digest: [u8; 32],
    pub finalization_revision: u64,
    pub finalized_floor_hash: [u8; 32],
    pub finalized_floor_height: i64,
    pub compaction_horizon: i64,
    pub digest_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccurrenceActivation {
    pub schema_version: u32,
    pub protocol_version: i64,
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn approved_genesis() -> DeployOccurrence {
        DeployOccurrence {
            schema_version: DEPLOY_OCCURRENCE_SCHEMA_VERSION,
            deploy_id: DeployIdV6::try_from(&[1; 32][..]).unwrap(),
            protocol_version: DEPLOY_OCCURRENCE_PROTOCOL_VERSION,
            source_block_hash: [2; 32],
            source_block_height: 0,
            source_validator: Vec::new(),
            deploy_ordinal: 0,
            admission_mode: OccurrenceAdmissionMode::ApprovedGenesis,
            admission_ruleset_digest: Vec::new(),
            admission_context_digest: Vec::new(),
            sender_authority_digest: Vec::new(),
            is_failed: false,
        }
    }

    #[test]
    fn approved_genesis_accepts_only_the_governance_anchor_shape() {
        assert!(approved_genesis().validate().is_ok());

        let mut nonzero_height = approved_genesis();
        nonzero_height.source_block_height = 1;
        assert!(nonzero_height.validate().is_err());

        let mut sender = approved_genesis();
        sender.source_validator = vec![3; models::rust::validator::LENGTH];
        assert!(sender.validate().is_err());

        let mut ruleset = approved_genesis();
        ruleset.admission_ruleset_digest = vec![4; 32];
        assert!(ruleset.validate().is_err());

        let mut context = approved_genesis();
        context.admission_context_digest = vec![5; 32];
        assert!(context.validate().is_err());

        let mut authority = approved_genesis();
        authority.sender_authority_digest = vec![6; 32];
        assert!(authority.validate().is_err());

        let mut failed = approved_genesis();
        failed.is_failed = true;
        assert!(failed.validate().is_err());
    }

    proptest! {
        #[test]
        fn approved_genesis_rejects_every_positive_height(height in 1_i64..=i64::MAX) {
            let mut occurrence = approved_genesis();
            occurrence.source_block_height = height;
            prop_assert!(occurrence.validate().is_err());
        }

        #[test]
        fn approved_genesis_rejects_every_nonempty_admission_field(
            selector in 0_usize..4,
            bytes in prop::collection::vec(any::<u8>(), 1..96),
        ) {
            let mut occurrence = approved_genesis();
            match selector {
                0 => occurrence.source_validator = bytes,
                1 => occurrence.admission_ruleset_digest = bytes,
                2 => occurrence.admission_context_digest = bytes,
                _ => occurrence.sender_authority_digest = bytes,
            }
            prop_assert!(occurrence.validate().is_err());
        }
    }
}
