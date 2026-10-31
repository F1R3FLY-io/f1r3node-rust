use block_storage::rust::deploy::offer_rejection::OfferRejection;
use models::rust::block_hash::BlockHash;

use super::block_record::{BlockRecord, EnvelopeOutcome};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PoolStatus {
    Pending,
    Rejected(OfferRejection),
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferedReceipt {
    pub block_hash: BlockHash,
    pub outcome: EnvelopeOutcome,
    pub comm_phlo: u64,
    pub compute_phlo: u64,
    pub event_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfferedStatus {
    Unknown,
    Pending,
    Rejected(OfferRejection),
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
    Some(OfferedReceipt {
        block_hash: block_hash.clone(),
        outcome: entry.executed.outcome,
        comm_phlo: entry.executed.comm_phlo,
        compute_phlo: entry.executed.compute_phlo,
        event_count: entry.executed.user_log.len(),
    })
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::v6::block_record::{ExecutedEnvelope, RecordEntry};

    fn h(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    fn entry(id: &str, outcome: EnvelopeOutcome, comm: u64, compute: u64) -> RecordEntry {
        RecordEntry {
            executed: ExecutedEnvelope {
                envelope_id: h(id),
                outcome,
                comm_phlo: comm,
                compute_phlo: compute,
                user_log: Vec::new(),
            },
        }
    }

    #[test]
    fn the_receipt_reads_the_offer_entry_from_the_block_record() {
        let record = BlockRecord::new(
            h("b1"),
            h("r0"),
            h("m"),
            h("r1"),
            vec![
                entry("a", EnvelopeOutcome::Completed, 2, 40),
                entry("b", EnvelopeOutcome::Conflicted, 0, 15),
            ],
            Vec::new(),
        )
        .unwrap();
        assert_eq!(
            receipt_of(&record, &h("b1"), b"b"),
            Some(OfferedReceipt {
                block_hash: h("b1"),
                outcome: EnvelopeOutcome::Conflicted,
                comm_phlo: 0,
                compute_phlo: 15,
                event_count: 0,
            })
        );
        assert_eq!(receipt_of(&record, &h("b1"), b"c"), None);
    }
}
