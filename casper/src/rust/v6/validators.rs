use std::sync::Arc;

use async_trait::async_trait;
use models::rust::casper::protocol::casper_message::Bond;

use super::body::Attestation;
use super::handoff::HandoffRecord;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[async_trait]
pub trait ValidatorSetSource: Send + Sync {
    async fn bonds(
        &self,
        attestation: &Attestation,
        block_number: i64,
    ) -> Result<Vec<Bond>, CasperError>;
}

pub struct AttestedRootValidators {
    runtime_manager: Arc<RuntimeManager>,
}

impl AttestedRootValidators {
    pub fn new(runtime_manager: Arc<RuntimeManager>) -> Self {
        AttestedRootValidators { runtime_manager }
    }
}

pub async fn committee_at(
    runtime_manager: &RuntimeManager,
    root: &models::rust::block::state_hash::StateHash,
) -> Result<Vec<Bond>, CasperError> {
    let (current, _) = runtime_manager.get_handoff(root).await?;
    HandoffRecord::decode(&current)
        .map(|record| record.committee())
        .map_err(|e| CasperError::RuntimeError(e.to_string()))
}

#[async_trait]
impl ValidatorSetSource for AttestedRootValidators {
    async fn bonds(
        &self,
        attestation: &Attestation,
        _block_number: i64,
    ) -> Result<Vec<Bond>, CasperError> {
        committee_at(&self.runtime_manager, &attestation.root).await
    }
}
