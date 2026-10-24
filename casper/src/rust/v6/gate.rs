use std::sync::Arc;

use async_trait::async_trait;
use models::rust::casper::protocol::casper_message::ProcessedUserDeploy;
use models::rust::deploy_envelope::DeployEnvelope;

use super::body::StateHash;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub type PurseHold = ([u8; 32], u128);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    Admitted { holds: Vec<PurseHold> },
    Rejected { cause: String },
}

impl GateVerdict {
    pub fn is_admitted(&self) -> bool { matches!(self, GateVerdict::Admitted { .. }) }
}

#[derive(Clone, Copy, Debug)]
pub enum GateInput<'a> {
    Legacy,
    Offered(&'a DeployEnvelope),
}

impl<'a> From<&'a ProcessedUserDeploy> for GateInput<'a> {
    fn from(deploy: &'a ProcessedUserDeploy) -> Self {
        match deploy {
            ProcessedUserDeploy::Legacy(_) => GateInput::Legacy,
            ProcessedUserDeploy::Offered(offered) => GateInput::Offered(offered.envelope()),
            ProcessedUserDeploy::Ordered(envelope) => GateInput::Offered(envelope),
        }
    }
}

#[async_trait]
pub trait BlockGate: Send + Sync {
    async fn gate(
        &self,
        root: &StateHash,
        block_time: i64,
        envelopes: &[GateInput<'_>],
    ) -> Result<Vec<GateVerdict>, CasperError>;
}

pub struct AdmitAll;

#[async_trait]
impl BlockGate for AdmitAll {
    async fn gate(
        &self,
        _root: &StateHash,
        _block_time: i64,
        envelopes: &[GateInput<'_>],
    ) -> Result<Vec<GateVerdict>, CasperError> {
        Ok(envelopes
            .iter()
            .map(|_| GateVerdict::Admitted { holds: Vec::new() })
            .collect())
    }
}

pub struct RhoBlockGate {
    runtime_manager: Arc<RuntimeManager>,
    policy: Arc<AdoptedResourcePolicy>,
}

impl RhoBlockGate {
    pub fn new(runtime_manager: Arc<RuntimeManager>, policy: Arc<AdoptedResourcePolicy>) -> Self {
        RhoBlockGate {
            runtime_manager,
            policy,
        }
    }
}

#[async_trait]
impl BlockGate for RhoBlockGate {
    async fn gate(
        &self,
        root: &StateHash,
        block_time: i64,
        envelopes: &[GateInput<'_>],
    ) -> Result<Vec<GateVerdict>, CasperError> {
        let root: [u8; 32] = root
            .as_ref()
            .try_into()
            .map_err(|_| CasperError::RuntimeError("the gate root is not 32 bytes".to_string()))?;
        let offered: Vec<_> = envelopes
            .iter()
            .map(|envelope| match envelope {
                GateInput::Offered(envelope) => Some(*envelope),
                GateInput::Legacy => None,
            })
            .collect();
        Ok(self
            .runtime_manager
            .gate_offered_block(root, block_time, &self.policy, &offered)
            .await?
            .into_iter()
            .map(|admission| match admission {
                Ok(holds) => GateVerdict::Admitted { holds },
                Err(cause) => GateVerdict::Rejected { cause },
            })
            .collect())
    }
}

pub async fn admitted(
    gate: &dyn BlockGate,
    root: &StateHash,
    block_time: i64,
    envelopes: Vec<ProcessedUserDeploy>,
) -> Result<Vec<ProcessedUserDeploy>, CasperError> {
    let inputs: Vec<GateInput<'_>> = envelopes.iter().map(GateInput::from).collect();
    let verdicts = gate.gate(root, block_time, &inputs).await?;
    if verdicts.len() != envelopes.len() {
        return Err(CasperError::RuntimeError(format!(
            "the block gate returned {} verdicts for {} envelopes",
            verdicts.len(),
            envelopes.len()
        )));
    }
    Ok(envelopes
        .into_iter()
        .zip(verdicts)
        .filter(|(_, verdict)| verdict.is_admitted())
        .map(|(envelope, _)| envelope)
        .collect())
}

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::ProcessedDeploy;
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::util::construct_deploy;

    struct RejectOdd;

    #[async_trait]
    impl BlockGate for RejectOdd {
        async fn gate(
            &self,
            _root: &StateHash,
            _block_time: i64,
            envelopes: &[GateInput<'_>],
        ) -> Result<Vec<GateVerdict>, CasperError> {
            Ok((0..envelopes.len())
                .map(|index| match index % 2 {
                    0 => GateVerdict::Admitted {
                        holds: vec![([index as u8; 32], index as u128)],
                    },
                    _ => GateVerdict::Rejected {
                        cause: "odd".to_string(),
                    },
                })
                .collect())
        }
    }

    struct Short;

    #[async_trait]
    impl BlockGate for Short {
        async fn gate(
            &self,
            _root: &StateHash,
            _block_time: i64,
            _envelopes: &[GateInput<'_>],
        ) -> Result<Vec<GateVerdict>, CasperError> {
            Ok(Vec::new())
        }
    }

    fn envelopes(count: i64) -> Vec<ProcessedUserDeploy> {
        (0..count)
            .map(|index| {
                ProcessedDeploy::empty(
                    construct_deploy::source_deploy(
                        format!("@{index}!({index})"),
                        index,
                        Some(1000),
                        None,
                        None,
                        None,
                        None,
                    )
                    .expect("a signed deploy"),
                )
                .into()
            })
            .collect()
    }

    #[tokio::test]
    async fn admit_all_keeps_every_envelope_in_order() {
        let all = envelopes(3);
        let kept = admitted(&AdmitAll, &Bytes::new(), 0, all.clone())
            .await
            .unwrap();
        assert_eq!(kept, all);
    }

    #[tokio::test]
    async fn rejected_envelopes_are_dropped_and_the_order_is_kept() {
        let all = envelopes(5);
        let kept = admitted(&RejectOdd, &Bytes::new(), 0, all.clone())
            .await
            .unwrap();
        assert_eq!(kept, vec![all[0].clone(), all[2].clone(), all[4].clone()]);
    }

    #[tokio::test]
    async fn a_gate_must_give_one_verdict_per_envelope() {
        assert!(admitted(&Short, &Bytes::new(), 0, envelopes(2))
            .await
            .is_err());
        assert!(admitted(&Short, &Bytes::new(), 0, Vec::new())
            .await
            .unwrap()
            .is_empty());
    }
}
