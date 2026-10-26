use std::sync::Arc;

use async_trait::async_trait;
use models::rust::casper::protocol::casper_message::{BlockMessage, Event, ProcessedSystemDeploy};
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::native_cost_evidence::NativeCostEvidenceV1;
use prost::bytes::Bytes;

use super::backend::{block_data, close_block};
use super::body::StateHash;
use super::gate::GateVerdict;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::direct_wallet_funding::OfferedRun;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeOutcome {
    NotRun,
    Completed,
    UserFailure,
    Exhausted,
    RolledBack,
    Conflicted,
}

impl EnvelopeOutcome {
    pub fn tag(self) -> u32 {
        match self {
            EnvelopeOutcome::NotRun => 0,
            EnvelopeOutcome::Completed => 1,
            EnvelopeOutcome::UserFailure => 2,
            EnvelopeOutcome::Exhausted => 3,
            EnvelopeOutcome::RolledBack => 4,
            EnvelopeOutcome::Conflicted => 5,
        }
    }

    pub fn from_tag(tag: u32) -> Result<Self, String> {
        match tag {
            0 => Ok(EnvelopeOutcome::NotRun),
            1 => Ok(EnvelopeOutcome::Completed),
            2 => Ok(EnvelopeOutcome::UserFailure),
            3 => Ok(EnvelopeOutcome::Exhausted),
            4 => Ok(EnvelopeOutcome::RolledBack),
            5 => Ok(EnvelopeOutcome::Conflicted),
            other => Err(format!("unknown envelope outcome {other}")),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutedEnvelope {
    pub envelope_id: Bytes,
    pub verdict: GateVerdict,
    pub outcome: EnvelopeOutcome,
    pub phlo_used: u64,
    pub budget_recording: Bytes,
    pub operation_journal: Bytes,
    pub user_log: Vec<Event>,
}

impl ExecutedEnvelope {
    pub fn check(&self) -> Result<(), String> {
        match (&self.verdict, self.outcome) {
            (GateVerdict::Rejected { .. }, EnvelopeOutcome::NotRun) => {
                if self.phlo_used != 0
                    || !self.budget_recording.is_empty()
                    || !self.operation_journal.is_empty()
                    || !self.user_log.is_empty()
                {
                    return Err("an envelope that did not run has no usage and no evidence".into());
                }
                Ok(())
            }
            (GateVerdict::Rejected { .. }, _) => {
                Err("an envelope that the gate rejected must not run".into())
            }
            (GateVerdict::Admitted { .. }, EnvelopeOutcome::NotRun) => {
                Err("an envelope that the gate admitted must run".into())
            }
            (GateVerdict::Admitted { .. }, _) => Ok(()),
        }
    }
}

#[derive(Clone)]
pub struct EnvelopeRun {
    pub executed: ExecutedEnvelope,
    pub run: Option<Arc<OfferedRun>>,
}

impl EnvelopeRun {
    pub fn recorded(executed: ExecutedEnvelope) -> Self {
        EnvelopeRun {
            executed,
            run: None,
        }
    }
}

pub fn executed_of(runs: &[EnvelopeRun]) -> Vec<ExecutedEnvelope> {
    runs.iter().map(|run| run.executed.clone()).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PurseDebit {
    pub purse: [u8; 32],
    pub resource_rev: u128,
    pub fee_rev: u128,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettledEnvelope {
    pub envelope_id: Bytes,
    pub rev_spent: u128,
    pub fee_rev: u128,
    pub debits: Vec<PurseDebit>,
    pub cost_evidence: Bytes,
    pub wallet_log: Vec<Event>,
    pub receipt_log: Vec<Event>,
}

impl SettledEnvelope {
    pub fn unsettled(envelope_id: Bytes) -> Self {
        SettledEnvelope {
            envelope_id,
            rev_spent: 0,
            fee_rev: 0,
            debits: Vec::new(),
            cost_evidence: Bytes::new(),
            wallet_log: Vec::new(),
            receipt_log: Vec::new(),
        }
    }

    pub fn check(&self) -> Result<(), String> {
        let debited = self.debits.iter().try_fold(0u128, |total, debit| {
            total
                .checked_add(debit.resource_rev)
                .and_then(|total| total.checked_add(debit.fee_rev))
        });
        let fees = self
            .debits
            .iter()
            .try_fold(0u128, |total, debit| total.checked_add(debit.fee_rev));
        if debited != Some(self.rev_spent) || fees != Some(self.fee_rev) {
            return Err("the purse debits must add up to the REV spent and the fee".into());
        }
        if self
            .debits
            .windows(2)
            .any(|pair| pair[0].purse >= pair[1].purse)
        {
            return Err("purse debits must be in strictly ascending purse order".into());
        }
        Ok(())
    }
}

fn evidence_difference(left: &[u8], right: &[u8]) -> String {
    let limits = offered_funded_v6_limits().evidence;
    let wire = models::rust::phlo_wire::PhloWireLimits {
        total_bytes: limits.total_bytes,
        field_bytes: limits.field_bytes,
    };
    let (Ok(a), Ok(b)) = (
        NativeCostEvidenceV1::decode(left, wire),
        NativeCostEvidenceV1::decode(right, wire),
    ) else {
        return "cost evidence that does not decode".to_string();
    };
    let fields = [
        (
            "settlement runtime root",
            a.settlement_runtime_root != b.settlement_runtime_root,
        ),
        ("post-state root", a.post_state_root != b.post_state_root),
        (
            "original funding root",
            a.original_funding_root != b.original_funding_root,
        ),
        (
            "phlo",
            (a.phlo_used, a.fresh_phlo, a.retained_phlo)
                != (b.phlo_used, b.fresh_phlo, b.retained_phlo),
        ),
        (
            "wallet event count",
            a.wallet_settlement_log_events != b.wallet_settlement_log_events,
        ),
        (
            "wallet settlement",
            a.wallet_settlement != b.wallet_settlement,
        ),
        ("funding case", a.funding_case != b.funding_case),
        ("prepaid delta", a.prepaid_delta != b.prepaid_delta),
        (
            "recording",
            (a.budget_recording, a.operation_journal) != (b.budget_recording, b.operation_journal),
        ),
    ];
    let differing: Vec<&str> = fields
        .iter()
        .filter(|(_, differs)| *differs)
        .map(|(name, _)| *name)
        .collect();
    format!("cost evidence ({})", differing.join(", "))
}

pub fn settled_difference(left: &SettledEnvelope, right: &SettledEnvelope) -> Option<String> {
    if left.rev_spent != right.rev_spent || left.fee_rev != right.fee_rev {
        return Some(format!(
            "charges {}/{} against {}/{}",
            left.rev_spent, left.fee_rev, right.rev_spent, right.fee_rev
        ));
    }
    if left.debits != right.debits {
        return Some("purse debits".to_string());
    }
    if left.cost_evidence != right.cost_evidence {
        return Some(evidence_difference(
            &left.cost_evidence,
            &right.cost_evidence,
        ));
    }
    if left.wallet_log != right.wallet_log {
        return Some(format!(
            "wallet log of {} events against {}",
            left.wallet_log.len(),
            right.wallet_log.len()
        ));
    }
    if left.receipt_log != right.receipt_log {
        return Some(format!(
            "receipt log of {} events against {}",
            left.receipt_log.len(),
            right.receipt_log.len()
        ));
    }
    None
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettlementOutcome {
    pub post_root: StateHash,
    pub settled: Vec<SettledEnvelope>,
    pub system_deploys: Vec<ProcessedSystemDeploy>,
}

pub fn envelope_ids(block: &BlockMessage) -> Vec<Bytes> {
    block
        .body
        .deploys
        .iter()
        .map(|deploy| Bytes::copy_from_slice(deploy.identity_bytes()))
        .collect()
}

pub fn check_executed(block: &BlockMessage, executed: &[ExecutedEnvelope]) -> Result<(), String> {
    let expected = envelope_ids(block);
    if executed.len() != expected.len()
        || executed
            .iter()
            .zip(&expected)
            .any(|(entry, id)| entry.envelope_id != *id)
    {
        return Err("the executed envelopes must be the block's envelopes in block order".into());
    }
    executed.iter().try_for_each(ExecutedEnvelope::check)
}

pub fn check_settled(
    executed: &[ExecutedEnvelope],
    settled: &[SettledEnvelope],
) -> Result<(), String> {
    if settled.len() != executed.len()
        || settled
            .iter()
            .zip(executed)
            .any(|(settled, executed)| settled.envelope_id != executed.envelope_id)
    {
        return Err("the settled envelopes must match the executed envelopes in order".into());
    }
    settled.iter().try_for_each(SettledEnvelope::check)
}

#[async_trait]
pub trait Settlement: Send + Sync {
    async fn settle(
        &self,
        block: &BlockMessage,
        pre_root: &StateHash,
        mid_root: &StateHash,
        runs: &[EnvelopeRun],
    ) -> Result<SettlementOutcome, CasperError>;

    async fn replay(
        &self,
        block: &BlockMessage,
        pre_root: &StateHash,
        mid_root: &StateHash,
        runs: &[EnvelopeRun],
        outcome: &SettlementOutcome,
    ) -> Result<StateHash, CasperError>;
}

fn invalid(message: String) -> CasperError { CasperError::RuntimeError(message) }

pub struct CloseOnlySettlement {
    runtime_manager: Arc<RuntimeManager>,
}

impl CloseOnlySettlement {
    pub fn new(runtime_manager: Arc<RuntimeManager>) -> Self {
        CloseOnlySettlement { runtime_manager }
    }
}

#[async_trait]
impl Settlement for CloseOnlySettlement {
    async fn settle(
        &self,
        block: &BlockMessage,
        _pre_root: &StateHash,
        mid_root: &StateHash,
        runs: &[EnvelopeRun],
    ) -> Result<SettlementOutcome, CasperError> {
        let executed = executed_of(runs);
        check_executed(block, &executed).map_err(invalid)?;
        let (post_root, _, system_deploys) = self
            .runtime_manager
            .compute_state(
                mid_root,
                vec![],
                vec![close_block(block)],
                block_data(block),
                None,
            )
            .await?;
        Ok(SettlementOutcome {
            post_root,
            settled: executed
                .iter()
                .map(|entry| SettledEnvelope::unsettled(entry.envelope_id.clone()))
                .collect(),
            system_deploys,
        })
    }

    async fn replay(
        &self,
        block: &BlockMessage,
        _pre_root: &StateHash,
        mid_root: &StateHash,
        runs: &[EnvelopeRun],
        outcome: &SettlementOutcome,
    ) -> Result<StateHash, CasperError> {
        let executed = executed_of(runs);
        check_executed(block, &executed).map_err(invalid)?;
        check_settled(&executed, &outcome.settled).map_err(invalid)?;
        if outcome
            .settled
            .iter()
            .any(|settled| *settled != SettledEnvelope::unsettled(settled.envelope_id.clone()))
        {
            return Err(invalid("close-only settlement charges nothing".to_string()));
        }
        let root = self
            .runtime_manager
            .replay_compute_state(
                mid_root,
                vec![],
                outcome.system_deploys.clone(),
                &block_data(block),
                None,
                false,
            )
            .await?;
        if root != outcome.post_root {
            return Err(invalid(
                "settlement replay reached another root".to_string(),
            ));
        }
        Ok(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admitted(id: &str, outcome: EnvelopeOutcome) -> ExecutedEnvelope {
        ExecutedEnvelope {
            envelope_id: Bytes::copy_from_slice(id.as_bytes()),
            verdict: GateVerdict::Admitted { holds: vec![] },
            outcome,
            phlo_used: 5,
            budget_recording: Bytes::from_static(b"recording"),
            operation_journal: Bytes::from_static(b"journal"),
            user_log: Vec::new(),
        }
    }

    fn rejected(id: &str) -> ExecutedEnvelope {
        ExecutedEnvelope {
            envelope_id: Bytes::copy_from_slice(id.as_bytes()),
            verdict: GateVerdict::Rejected {
                cause: "insufficient".into(),
            },
            outcome: EnvelopeOutcome::NotRun,
            phlo_used: 0,
            budget_recording: Bytes::new(),
            operation_journal: Bytes::new(),
            user_log: Vec::new(),
        }
    }

    #[test]
    fn outcome_tags_round_trip() {
        for outcome in [
            EnvelopeOutcome::NotRun,
            EnvelopeOutcome::Completed,
            EnvelopeOutcome::UserFailure,
            EnvelopeOutcome::Exhausted,
            EnvelopeOutcome::RolledBack,
            EnvelopeOutcome::Conflicted,
        ] {
            assert_eq!(EnvelopeOutcome::from_tag(outcome.tag()).unwrap(), outcome);
        }
        assert!(EnvelopeOutcome::from_tag(6).is_err());
    }

    #[test]
    fn the_gate_decides_whether_an_envelope_runs() {
        assert!(admitted("a", EnvelopeOutcome::Completed).check().is_ok());
        assert!(admitted("a", EnvelopeOutcome::Exhausted).check().is_ok());
        assert!(admitted("a", EnvelopeOutcome::NotRun).check().is_err());
        assert!(rejected("r").check().is_ok());
        let ran = ExecutedEnvelope {
            outcome: EnvelopeOutcome::Completed,
            ..rejected("r")
        };
        assert!(ran.check().is_err());
        let charged = ExecutedEnvelope {
            phlo_used: 1,
            ..rejected("r")
        };
        assert!(charged.check().is_err());
    }

    #[test]
    fn settled_debits_add_up_in_purse_order() {
        let settled = SettledEnvelope {
            envelope_id: Bytes::from_static(b"a"),
            rev_spent: 17,
            fee_rev: 2,
            debits: vec![
                PurseDebit {
                    purse: [1; 32],
                    resource_rev: 10,
                    fee_rev: 2,
                },
                PurseDebit {
                    purse: [2; 32],
                    resource_rev: 5,
                    fee_rev: 0,
                },
            ],
            cost_evidence: Bytes::new(),
            wallet_log: Vec::new(),
            receipt_log: Vec::new(),
        };
        assert!(settled.check().is_ok());
        assert!(SettledEnvelope {
            rev_spent: 18,
            ..settled.clone()
        }
        .check()
        .is_err());
        let mut unordered = settled.clone();
        unordered.debits.reverse();
        assert!(unordered.check().is_err());
        assert!(SettledEnvelope::unsettled(Bytes::from_static(b"x"))
            .check()
            .is_ok());
    }

    #[test]
    fn settled_entries_must_follow_the_executed_envelopes() {
        let executed = vec![admitted("a", EnvelopeOutcome::Completed), rejected("b")];
        let settled: Vec<_> = executed
            .iter()
            .map(|entry| SettledEnvelope::unsettled(entry.envelope_id.clone()))
            .collect();
        assert!(check_settled(&executed, &settled).is_ok());
        let mut swapped = settled.clone();
        swapped.swap(0, 1);
        assert!(check_settled(&executed, &swapped).is_err());
        assert!(check_settled(&executed, &settled[..1]).is_err());
    }
}
