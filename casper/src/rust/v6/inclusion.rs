use async_trait::async_trait;
use models::rust::casper::protocol::casper_message::ProcessedDeploy;
use prost::bytes::Bytes;

use super::body::StateHash;
use crate::rust::errors::CasperError;

#[async_trait]
pub trait InclusionScreen: Send + Sync {
    async fn refused(
        &self,
        root: &StateHash,
        timestamp: i64,
        envelopes: &[ProcessedDeploy],
    ) -> Result<Vec<(Bytes, String)>, CasperError>;
}

pub struct AdmitAll;

#[async_trait]
impl InclusionScreen for AdmitAll {
    async fn refused(
        &self,
        _root: &StateHash,
        _timestamp: i64,
        _envelopes: &[ProcessedDeploy],
    ) -> Result<Vec<(Bytes, String)>, CasperError> {
        Ok(Vec::new())
    }
}
