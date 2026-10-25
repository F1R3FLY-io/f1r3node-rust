use models::casper::{EventProto, ProcessedSystemDeployProto};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{Event, ProcessedSystemDeploy};
use prost::bytes::Bytes;
use prost::Message;

use super::body::StateHash;
use super::gate::{GateVerdict, PurseHold};
use super::record::ExecutionRecord;
use super::settlement::{EnvelopeOutcome, ExecutedEnvelope, PurseDebit, SettledEnvelope};

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PurseAmountProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub purse: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub amount: Bytes,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PurseDebitProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub purse: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub resource_rev: Bytes,
    #[prost(bytes = "bytes", tag = "3")]
    pub fee_rev: Bytes,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RecordEntryProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub envelope_id: Bytes,
    #[prost(bool, tag = "2")]
    pub admitted: bool,
    #[prost(message, repeated, tag = "3")]
    pub holds: Vec<PurseAmountProto>,
    #[prost(string, tag = "4")]
    pub rejection_cause: String,
    #[prost(uint32, tag = "5")]
    pub outcome: u32,
    #[prost(uint64, tag = "6")]
    pub phlo_used: u64,
    #[prost(bytes = "bytes", tag = "7")]
    pub budget_recording: Bytes,
    #[prost(bytes = "bytes", tag = "8")]
    pub operation_journal: Bytes,
    #[prost(bytes = "bytes", tag = "9")]
    pub rev_spent: Bytes,
    #[prost(bytes = "bytes", tag = "10")]
    pub fee_rev: Bytes,
    #[prost(message, repeated, tag = "11")]
    pub debits: Vec<PurseDebitProto>,
    #[prost(bytes = "bytes", tag = "12")]
    pub cost_evidence: Bytes,
    #[prost(message, repeated, tag = "13")]
    pub user_log: Vec<EventProto>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockRecordProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub target: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub pre_root: Bytes,
    #[prost(bytes = "bytes", tag = "3")]
    pub mid_root: Bytes,
    #[prost(bytes = "bytes", tag = "4")]
    pub post_root: Bytes,
    #[prost(message, repeated, tag = "5")]
    pub entries: Vec<RecordEntryProto>,
    #[prost(message, repeated, tag = "6")]
    pub system_deploys: Vec<ProcessedSystemDeployProto>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordEntry {
    pub executed: ExecutedEnvelope,
    pub settled: SettledEnvelope,
}

impl RecordEntry {
    pub fn check(&self) -> Result<(), String> {
        if self.executed.envelope_id != self.settled.envelope_id {
            return Err("a record entry settles another envelope".into());
        }
        self.executed.check()?;
        self.settled.check()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockRecord {
    target: BlockHash,
    pre_root: StateHash,
    mid_root: StateHash,
    post_root: StateHash,
    entries: Vec<RecordEntry>,
    system_deploys: Vec<ProcessedSystemDeploy>,
    envelope_ids: Vec<Bytes>,
}

impl BlockRecord {
    pub fn new(
        target: BlockHash,
        pre_root: StateHash,
        mid_root: StateHash,
        post_root: StateHash,
        entries: Vec<RecordEntry>,
        system_deploys: Vec<ProcessedSystemDeploy>,
    ) -> Result<Self, String> {
        entries.iter().try_for_each(RecordEntry::check)?;
        let envelope_ids = entries
            .iter()
            .map(|entry| entry.executed.envelope_id.clone())
            .collect();
        Ok(BlockRecord {
            target,
            pre_root,
            mid_root,
            post_root,
            entries,
            system_deploys,
            envelope_ids,
        })
    }

    pub fn mid_root(&self) -> &StateHash { &self.mid_root }

    pub fn entries(&self) -> &[RecordEntry] { &self.entries }

    pub fn system_deploys(&self) -> &[ProcessedSystemDeploy] { &self.system_deploys }

    pub fn executed(&self) -> Vec<ExecutedEnvelope> {
        self.entries
            .iter()
            .map(|entry| entry.executed.clone())
            .collect()
    }

    pub fn settled(&self) -> Vec<SettledEnvelope> {
        self.entries
            .iter()
            .map(|entry| entry.settled.clone())
            .collect()
    }

    fn to_proto(&self) -> BlockRecordProto {
        BlockRecordProto {
            target: self.target.clone(),
            pre_root: self.pre_root.clone(),
            mid_root: self.mid_root.clone(),
            post_root: self.post_root.clone(),
            entries: self.entries.iter().map(entry_to_proto).collect(),
            system_deploys: self
                .system_deploys
                .iter()
                .cloned()
                .map(ProcessedSystemDeploy::to_proto)
                .collect(),
        }
    }

    fn from_proto(proto: BlockRecordProto) -> Result<Self, String> {
        let entries = proto
            .entries
            .into_iter()
            .map(entry_from_proto)
            .collect::<Result<Vec<_>, _>>()?;
        let system_deploys = proto
            .system_deploys
            .into_iter()
            .map(ProcessedSystemDeploy::from_proto)
            .collect::<Result<Vec<_>, _>>()?;
        BlockRecord::new(
            proto.target,
            proto.pre_root,
            proto.mid_root,
            proto.post_root,
            entries,
            system_deploys,
        )
    }
}

fn amount_bytes(amount: u128) -> Bytes { Bytes::copy_from_slice(&amount.to_be_bytes()) }

fn amount(bytes: &Bytes) -> Result<u128, String> {
    let fixed: [u8; 16] = bytes
        .as_ref()
        .try_into()
        .map_err(|_| "a REV amount must be 16 bytes".to_string())?;
    Ok(u128::from_be_bytes(fixed))
}

fn purse(bytes: &Bytes) -> Result<[u8; 32], String> {
    bytes
        .as_ref()
        .try_into()
        .map_err(|_| "a purse must be 32 bytes".to_string())
}

fn entry_to_proto(entry: &RecordEntry) -> RecordEntryProto {
    let (admitted, holds, rejection_cause) = match &entry.executed.verdict {
        GateVerdict::Admitted { holds } => (
            true,
            holds
                .iter()
                .map(|(purse, amount)| PurseAmountProto {
                    purse: Bytes::copy_from_slice(purse),
                    amount: amount_bytes(*amount),
                })
                .collect(),
            String::new(),
        ),
        GateVerdict::Rejected { cause } => (false, Vec::new(), cause.clone()),
    };
    RecordEntryProto {
        envelope_id: entry.executed.envelope_id.clone(),
        admitted,
        holds,
        rejection_cause,
        outcome: entry.executed.outcome.tag(),
        phlo_used: entry.executed.phlo_used,
        budget_recording: entry.executed.budget_recording.clone(),
        operation_journal: entry.executed.operation_journal.clone(),
        rev_spent: amount_bytes(entry.settled.rev_spent),
        fee_rev: amount_bytes(entry.settled.fee_rev),
        debits: entry
            .settled
            .debits
            .iter()
            .map(|debit| PurseDebitProto {
                purse: Bytes::copy_from_slice(&debit.purse),
                resource_rev: amount_bytes(debit.resource_rev),
                fee_rev: amount_bytes(debit.fee_rev),
            })
            .collect(),
        cost_evidence: entry.settled.cost_evidence.clone(),
        user_log: entry
            .executed
            .user_log
            .iter()
            .map(Event::to_proto)
            .collect(),
    }
}

fn entry_from_proto(proto: RecordEntryProto) -> Result<RecordEntry, String> {
    let verdict = if proto.admitted {
        if !proto.rejection_cause.is_empty() {
            return Err("an admitted envelope has no rejection cause".into());
        }
        let holds = proto
            .holds
            .iter()
            .map(|hold| Ok::<PurseHold, String>((purse(&hold.purse)?, amount(&hold.amount)?)))
            .collect::<Result<Vec<_>, _>>()?;
        GateVerdict::Admitted { holds }
    } else {
        if !proto.holds.is_empty() || proto.rejection_cause.is_empty() {
            return Err("a rejected envelope has a cause and no holds".into());
        }
        GateVerdict::Rejected {
            cause: proto.rejection_cause,
        }
    };
    let debits = proto
        .debits
        .iter()
        .map(|debit| {
            Ok(PurseDebit {
                purse: purse(&debit.purse)?,
                resource_rev: amount(&debit.resource_rev)?,
                fee_rev: amount(&debit.fee_rev)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(RecordEntry {
        executed: ExecutedEnvelope {
            envelope_id: proto.envelope_id.clone(),
            verdict,
            outcome: EnvelopeOutcome::from_tag(proto.outcome)?,
            phlo_used: proto.phlo_used,
            budget_recording: proto.budget_recording,
            operation_journal: proto.operation_journal,
            user_log: proto
                .user_log
                .into_iter()
                .map(Event::from_proto)
                .collect::<Result<Vec<_>, _>>()?,
        },
        settled: SettledEnvelope {
            envelope_id: proto.envelope_id,
            rev_spent: amount(&proto.rev_spent)?,
            fee_rev: amount(&proto.fee_rev)?,
            debits,
            cost_evidence: proto.cost_evidence,
        },
    })
}

impl ExecutionRecord for BlockRecord {
    fn target(&self) -> &BlockHash { &self.target }

    fn pre_root(&self) -> &StateHash { &self.pre_root }

    fn post_root(&self) -> &StateHash { &self.post_root }

    fn envelope_ids(&self) -> &[Bytes] { &self.envelope_ids }

    fn encode(&self) -> Bytes { Bytes::from(self.to_proto().encode_to_vec()) }

    fn decode(bytes: &[u8]) -> Result<Self, String> {
        let proto = BlockRecordProto::decode(bytes).map_err(|e| e.to_string())?;
        if proto.encode_to_vec() != bytes {
            return Err("a v6 record must be canonically encoded".to_string());
        }
        Self::from_proto(proto)
    }

    fn encoded_len(&self) -> usize { self.to_proto().encoded_len() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    fn log_event() -> Event {
        Event::Consume(
            models::rust::casper::protocol::casper_message::ConsumeEvent {
                channels_hashes: vec![b("channel")],
                hash: b("consume"),
                persistent: false,
            },
        )
    }

    fn entry(id: &str, admitted: bool) -> RecordEntry {
        let executed = if admitted {
            ExecutedEnvelope {
                envelope_id: b(id),
                verdict: GateVerdict::Admitted {
                    holds: vec![([3; 32], 40)],
                },
                outcome: EnvelopeOutcome::Completed,
                phlo_used: 12,
                budget_recording: b("recording"),
                operation_journal: b("journal"),
                user_log: vec![log_event()],
            }
        } else {
            ExecutedEnvelope {
                envelope_id: b(id),
                verdict: GateVerdict::Rejected {
                    cause: "insufficient funding".into(),
                },
                outcome: EnvelopeOutcome::NotRun,
                phlo_used: 0,
                budget_recording: Bytes::new(),
                operation_journal: Bytes::new(),
                user_log: Vec::new(),
            }
        };
        let settled = if admitted {
            SettledEnvelope {
                envelope_id: b(id),
                rev_spent: 15,
                fee_rev: 3,
                debits: vec![PurseDebit {
                    purse: [3; 32],
                    resource_rev: 12,
                    fee_rev: 3,
                }],
                cost_evidence: b("evidence"),
            }
        } else {
            SettledEnvelope::unsettled(b(id))
        };
        RecordEntry { executed, settled }
    }

    fn record() -> BlockRecord {
        BlockRecord::new(
            b("target"),
            b("pre"),
            b("mid"),
            b("post"),
            vec![entry("a", true), entry("b", false)],
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn a_block_record_round_trips_and_names_its_envelopes() {
        let record = record();
        assert_eq!(record.envelope_ids(), &[b("a"), b("b")]);
        let bytes = record.encode();
        assert_eq!(bytes.len(), record.encoded_len());
        assert_eq!(BlockRecord::decode(&bytes).unwrap(), record);
    }

    #[test]
    fn the_digest_changes_with_any_verdict_or_settlement_field() {
        let base = record();
        let digest = base.digest();
        let mut variants = Vec::new();
        let mut verdict = base.entries.clone();
        verdict[0].executed.verdict = GateVerdict::Admitted {
            holds: vec![([3; 32], 41)],
        };
        variants.push(verdict);
        let mut outcome = base.entries.clone();
        outcome[0].executed.outcome = EnvelopeOutcome::UserFailure;
        variants.push(outcome);
        let mut log = base.entries.clone();
        log[0].executed.user_log.clear();
        variants.push(log);
        let mut evidence = base.entries.clone();
        evidence[0].settled.cost_evidence = b("other");
        variants.push(evidence);
        let mut debit = base.entries.clone();
        debit[0].settled.debits[0].purse = [4; 32];
        variants.push(debit);
        for entries in variants {
            let changed =
                BlockRecord::new(b("target"), b("pre"), b("mid"), b("post"), entries, vec![])
                    .unwrap();
            assert_eq!(changed.post_root(), base.post_root());
            assert_ne!(changed.digest(), digest);
        }
    }

    #[test]
    fn inconsistent_entries_are_rejected() {
        let mut ran = entry("b", false);
        ran.executed.outcome = EnvelopeOutcome::Completed;
        assert!(BlockRecord::new(b("t"), b("p"), b("m"), b("q"), vec![ran], vec![]).is_err());
        let mut other = entry("a", true);
        other.settled.envelope_id = b("z");
        assert!(BlockRecord::new(b("t"), b("p"), b("m"), b("q"), vec![other], vec![]).is_err());
        let mut sum = entry("a", true);
        sum.settled.rev_spent = 99;
        assert!(BlockRecord::new(b("t"), b("p"), b("m"), b("q"), vec![sum], vec![]).is_err());
    }

    #[test]
    fn malformed_bytes_are_rejected() {
        let mut proto = record().to_proto();
        proto.entries[0].rev_spent = Bytes::from_static(&[1, 2]);
        assert!(BlockRecord::decode(&proto.encode_to_vec()).is_err());
        let mut proto = record().to_proto();
        proto.entries[1].rejection_cause = String::new();
        assert!(BlockRecord::decode(&proto.encode_to_vec()).is_err());
        let mut proto = record().to_proto();
        proto.entries[0].outcome = 9;
        assert!(BlockRecord::decode(&proto.encode_to_vec()).is_err());
        let mut bytes = record().encode().to_vec();
        bytes.extend_from_slice(&[0x0a, 0x00]);
        assert!(BlockRecord::decode(&bytes).is_err());
    }
}
