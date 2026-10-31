use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::offer_rejection::OfferRejection;
use models::rust::casper::protocol::casper_message::ProcessedDeploy;
use prost::bytes::Bytes;

use super::body::StateHash;
use super::executor::gate_inputs;
use super::gate::{
    gate_block, resolve_joint_wallets, PhloLimitDemand, Refusal, StoreReader, Verdict,
};
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

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

pub struct GateScreen {
    runtime_manager: Arc<RuntimeManager>,
}

impl GateScreen {
    pub fn new(runtime_manager: Arc<RuntimeManager>) -> Self { GateScreen { runtime_manager } }
}

pub fn refusal_reason(refusal: &Refusal) -> String {
    match refusal {
        Refusal::ForeignSignature => {
            "a signed term names a signature that did not sign the deployment".to_string()
        }
        Refusal::InvalidTerms => "the deployment's terms are invalid".to_string(),
        Refusal::Overflow => "the deployment's demand overflows".to_string(),
        Refusal::InsufficientFunds { wallet, need, have } => format!(
            "wallet {} has {have} uncommitted, the deployment needs {need}",
            hex::encode(wallet)
        ),
    }
}

#[async_trait]
impl InclusionScreen for GateScreen {
    async fn refused(
        &self,
        root: &StateHash,
        _timestamp: i64,
        envelopes: &[ProcessedDeploy],
    ) -> Result<Vec<(Bytes, String)>, CasperError> {
        if envelopes.is_empty() {
            return Ok(Vec::new());
        }
        let reader = StoreReader::at(&self.runtime_manager, root).await?;
        let mut inputs = gate_inputs(envelopes.iter().map(|p| &p.deploy))?;
        resolve_joint_wallets(&reader, &mut inputs)?;
        let outcome = gate_block(&reader, &PhloLimitDemand, &inputs)?;
        Ok(outcome
            .entries
            .into_iter()
            .filter_map(|entry| match &entry.verdict {
                Verdict::Admitted => None,
                Verdict::Refused(refusal) => {
                    Some((entry.envelope_sig.clone(), refusal_reason(refusal)))
                }
            })
            .collect())
    }
}

pub async fn screen_offers(
    screen: &dyn InclusionScreen,
    deploy_storage: &KeyValueDeployStorage,
    envelopes: Vec<ProcessedDeploy>,
    root: &StateHash,
    block_number: i64,
    timestamp: i64,
) -> Result<Vec<ProcessedDeploy>, CasperError> {
    if envelopes.is_empty() {
        return Ok(envelopes);
    }
    let refused = match screen.refused(root, timestamp, &envelopes).await {
        Ok(refused) => refused,
        Err(error) => {
            tracing::warn!(
                target: "f1r3fly.v6",
                %error,
                "v6 offer screen could not run; the block's gate decides"
            );
            return Ok(envelopes);
        }
    };
    if refused.is_empty() {
        return Ok(envelopes);
    }
    for (sig, reason) in &refused {
        deploy_storage.record_rejection(OfferRejection {
            deploy_id: sig.to_vec(),
            pre_state_root: Some(root.to_vec()),
            block_number,
            reason: reason.clone(),
        })?;
    }
    let refused: HashSet<Bytes> = refused.into_iter().map(|(sig, _)| sig).collect();
    Ok(envelopes
        .into_iter()
        .filter(|envelope| !refused.contains(&envelope.deploy.sig))
        .collect())
}
