use std::sync::Arc;

use async_trait::async_trait;
use models::rust::casper::protocol::casper_message::Bond;

use super::body::Attestation;
use crate::rust::errors::CasperError;
use crate::rust::finality::floor::floor_committee;
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

#[async_trait]
impl ValidatorSetSource for AttestedRootValidators {
    async fn bonds(
        &self,
        attestation: &Attestation,
        _block_number: i64,
    ) -> Result<Vec<Bond>, CasperError> {
        floor_committee(&self.runtime_manager, &attestation.root).await
    }
}
