use block_storage::rust::deploy::key_value_deploy_storage::EnvelopeRejection;
use models::rust::block_hash::BlockHash;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::native_cost_evidence::NativeCostEvidenceV1;

use super::block_record::BlockRecord;
use super::gate::GateVerdict;
use super::settlement::{EnvelopeOutcome, PurseDebit};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PoolStatus {
    Pending,
    Rejected(EnvelopeRejection),
    Absent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvidenceCharges {
    pub phlo_limit: u64,
    pub phlo_price: u64,
    pub fresh_phlo: u64,
    pub retained_phlo: u64,
    pub owner_phlo: u64,
    pub owner_rev: u128,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OfferedReceipt {
    pub block_hash: BlockHash,
    pub outcome: EnvelopeOutcome,
    pub verdict: GateVerdict,
    pub phlo_used: u64,
    pub rev_spent: u128,
    pub fee_rev: u128,
    pub debits: Vec<PurseDebit>,
    pub charges: Option<EvidenceCharges>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OfferedStatus {
    Unknown,
    Pending,
    Rejected(EnvelopeRejection),
    Ordered { block_hash: BlockHash },
    NotRetained { block_hash: BlockHash },
    Executed(OfferedReceipt),
}

pub fn receipt_of(
    record: &BlockRecord,
    block_hash: &BlockHash,
    deploy_id: &[u8],
) -> Option<OfferedReceipt> {
    let entry = record
        .entries()
        .iter()
        .find(|entry| entry.executed.envelope_id.as_ref() == deploy_id)?;
    let charges = NativeCostEvidenceV1::decode(
        &entry.settled.cost_evidence,
        offered_funded_v6_limits().evidence,
    )
    .ok()
    .map(|evidence| EvidenceCharges {
        phlo_limit: evidence.phlo_limit,
        phlo_price: evidence.phlo_price,
        fresh_phlo: evidence.fresh_phlo,
        retained_phlo: evidence.retained_phlo,
        owner_phlo: evidence.owner_phlo,
        owner_rev: evidence.owner_rev,
    });
    Some(OfferedReceipt {
        block_hash: block_hash.clone(),
        outcome: entry.executed.outcome,
        verdict: entry.executed.verdict.clone(),
        phlo_used: entry.executed.phlo_used,
        rev_spent: entry.settled.rev_spent,
        fee_rev: entry.settled.fee_rev,
        debits: entry.settled.debits.clone(),
        charges,
    })
}
