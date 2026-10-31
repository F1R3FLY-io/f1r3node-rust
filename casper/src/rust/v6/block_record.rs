use models::casper::{EventProto, ProcessedSystemDeployProto};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{Event, ProcessedSystemDeploy};
use prost::bytes::Bytes;
use prost::Message;

use super::body::StateHash;
use super::gate::{GateEntry, GateEntryProto};
use super::record::ExecutionRecord;
use super::settle::{Settlement, SettlementProto};

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RecordEntryProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub envelope_id: Bytes,
    #[prost(uint32, tag = "5")]
    pub outcome: u32,
    #[prost(uint64, tag = "6")]
    pub compute_phlo: u64,
    #[prost(message, repeated, tag = "13")]
    pub user_log: Vec<EventProto>,
    #[prost(uint64, tag = "16")]
    pub comm_phlo: u64,
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
    #[prost(message, repeated, tag = "11")]
    pub gate: Vec<GateEntryProto>,
    #[prost(message, optional, tag = "12")]
    pub settlement: Option<SettlementProto>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeOutcome {
    NotRun,
    Completed,
    UserFailure,
    Exhausted,
    Conflicted,
}

impl EnvelopeOutcome {
    pub fn tag(self) -> u32 {
        match self {
            EnvelopeOutcome::NotRun => 0,
            EnvelopeOutcome::Completed => 1,
            EnvelopeOutcome::UserFailure => 2,
            EnvelopeOutcome::Exhausted => 3,
            EnvelopeOutcome::Conflicted => 5,
        }
    }

    pub fn from_tag(tag: u32) -> Result<Self, String> {
        match tag {
            0 => Ok(EnvelopeOutcome::NotRun),
            1 => Ok(EnvelopeOutcome::Completed),
            2 => Ok(EnvelopeOutcome::UserFailure),
            3 => Ok(EnvelopeOutcome::Exhausted),
            5 => Ok(EnvelopeOutcome::Conflicted),
            other => Err(format!("unknown envelope outcome {other}")),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutedEnvelope {
    pub envelope_id: Bytes,
    pub outcome: EnvelopeOutcome,
    pub comm_phlo: u64,
    pub compute_phlo: u64,
    pub user_log: Vec<Event>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordEntry {
    pub executed: ExecutedEnvelope,
}

impl RecordEntry {
    pub fn check(&self) -> Result<(), String> {
        if self.executed.envelope_id.is_empty() {
            return Err("a record entry names its envelope".into());
        }
        Ok(())
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
    gate: Vec<GateEntry>,
    settlement: Option<Settlement>,
}

impl BlockRecord {
    #[cfg(test)]
    pub(crate) fn empty(target: BlockHash, pre_root: StateHash, post_root: StateHash) -> Self {
        Self::with_mid(target, pre_root.clone(), pre_root, post_root)
    }

    #[cfg(test)]
    pub(crate) fn with_mid(
        target: BlockHash,
        pre_root: StateHash,
        mid_root: StateHash,
        post_root: StateHash,
    ) -> Self {
        Self::new(
            target,
            pre_root,
            mid_root,
            post_root,
            Vec::new(),
            Vec::new(),
        )
        .expect("a record with no entries")
    }

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
            gate: Vec::new(),
            settlement: None,
        })
    }

    pub fn with_gate(mut self, gate: Vec<GateEntry>) -> Result<Self, String> {
        if !gate.is_empty() && gate.len() != self.entries.len() {
            return Err("a record holds one gate entry per envelope".into());
        }
        for (entry, g) in self.entries.iter().zip(&gate) {
            if entry.executed.envelope_id != g.envelope_sig {
                return Err("the gate entries follow the envelope order".into());
            }
        }
        self.gate = gate;
        Ok(self)
    }

    pub fn gate(&self) -> &[GateEntry] { &self.gate }

    pub fn with_settlement(mut self, settlement: Settlement) -> Self {
        self.settlement = Some(settlement);
        self
    }

    pub fn settlement(&self) -> Option<&Settlement> { self.settlement.as_ref() }

    pub fn mid_root(&self) -> &StateHash { &self.mid_root }

    pub fn entries(&self) -> &[RecordEntry] { &self.entries }

    pub fn system_deploys(&self) -> &[ProcessedSystemDeploy] { &self.system_deploys }

    pub fn executed(&self) -> Vec<ExecutedEnvelope> {
        self.entries
            .iter()
            .map(|entry| entry.executed.clone())
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
            gate: self.gate.iter().map(GateEntry::to_proto).collect(),
            settlement: self.settlement.as_ref().map(Settlement::to_proto),
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
        let gate = proto
            .gate
            .into_iter()
            .map(GateEntry::from_proto)
            .collect::<Result<Vec<_>, _>>()?;
        BlockRecord::new(
            proto.target,
            proto.pre_root,
            proto.mid_root,
            proto.post_root,
            entries,
            system_deploys,
        )?
        .with_gate(gate)
        .and_then(|record| match proto.settlement {
            Some(s) => Settlement::from_proto(s).map(|s| record.with_settlement(s)),
            None => Ok(record),
        })
    }
}

fn entry_to_proto(entry: &RecordEntry) -> RecordEntryProto {
    RecordEntryProto {
        envelope_id: entry.executed.envelope_id.clone(),
        outcome: entry.executed.outcome.tag(),
        compute_phlo: entry.executed.compute_phlo,
        user_log: entry
            .executed
            .user_log
            .iter()
            .map(Event::to_proto)
            .collect(),
        comm_phlo: entry.executed.comm_phlo,
    }
}

fn entry_from_proto(proto: RecordEntryProto) -> Result<RecordEntry, String> {
    Ok(RecordEntry {
        executed: ExecutedEnvelope {
            envelope_id: proto.envelope_id,
            outcome: EnvelopeOutcome::from_tag(proto.outcome)?,
            comm_phlo: proto.comm_phlo,
            compute_phlo: proto.compute_phlo,
            user_log: proto
                .user_log
                .into_iter()
                .map(Event::from_proto)
                .collect::<Result<Vec<_>, _>>()?,
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

    fn entry(id: &str, outcome: EnvelopeOutcome) -> RecordEntry {
        RecordEntry {
            executed: ExecutedEnvelope {
                envelope_id: b(id),
                outcome,
                comm_phlo: 3,
                compute_phlo: 12,
                user_log: vec![log_event()],
            },
        }
    }

    fn record() -> BlockRecord {
        BlockRecord::new(
            b("target"),
            b("pre"),
            b("mid"),
            b("post"),
            vec![
                entry("a", EnvelopeOutcome::Completed),
                entry("b", EnvelopeOutcome::Conflicted),
                entry("c", EnvelopeOutcome::UserFailure),
            ],
            Vec::new(),
        )
        .unwrap()
    }

    #[test]
    fn a_block_record_round_trips() {
        let record = record();
        let decoded = BlockRecord::decode(&record.encode()).unwrap();
        assert_eq!(decoded, record);
        assert_eq!(decoded.envelope_ids(), &[b("a"), b("b"), b("c")]);
        assert_eq!(decoded.mid_root(), &b("mid"));
        assert_eq!(decoded.encoded_len(), record.encode().len());
    }

    #[test]
    fn a_non_canonical_encoding_is_refused() {
        let mut bytes = record().encode().to_vec();
        bytes.extend_from_slice(&[0x08, 0x01]);
        assert!(BlockRecord::decode(&bytes).is_err());
    }

    #[test]
    fn an_unknown_outcome_is_refused() {
        let mut proto = record().to_proto();
        proto.entries[0].outcome = 4;
        assert!(BlockRecord::decode(&proto.encode_to_vec()).is_err());
    }

    fn gate_entry(id: &str, admitted: bool) -> GateEntry {
        use super::super::gate::{Commitment, Refusal, Verdict};
        GateEntry {
            envelope_sig: b(id),
            delta: 100,
            verdict: if admitted {
                Verdict::Admitted
            } else {
                Verdict::Refused(Refusal::InsufficientFunds {
                    wallet: vec![7; 32],
                    need: 300,
                    have: 20,
                })
            },
            commitments: if admitted {
                vec![Commitment {
                    wallet: vec![7; 32],
                    amount: 300,
                }]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn the_gate_entries_ride_in_field_11() {
        let gated = record()
            .with_gate(vec![
                gate_entry("a", true),
                gate_entry("b", true),
                gate_entry("c", false),
            ])
            .unwrap();
        let decoded = BlockRecord::decode(&gated.encode()).unwrap();
        assert_eq!(decoded, gated);
        assert_eq!(decoded.gate().len(), 3);
        assert_eq!(gated.to_proto().gate.len(), 3);
        assert!(record().with_gate(vec![gate_entry("a", true)]).is_err());
        assert!(record()
            .with_gate(vec![
                gate_entry("b", true),
                gate_entry("a", true),
                gate_entry("c", true),
            ])
            .is_err());
        assert_eq!(BlockRecord::decode(&record().encode()).unwrap().gate(), &[]);
    }
}
