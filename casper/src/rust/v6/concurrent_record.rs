use models::casper::{EventProto, ProcessedSystemDeployProto};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{Event, ProcessedSystemDeploy};
use prost::bytes::Bytes;
use prost::Message;

use super::body::StateHash;
use super::record::ExecutionRecord;

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConcurrentRecordProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub target: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub pre_root: Bytes,
    #[prost(bytes = "bytes", tag = "3")]
    pub mid_root: Bytes,
    #[prost(bytes = "bytes", tag = "4")]
    pub post_root: Bytes,
    #[prost(bytes = "bytes", repeated, tag = "5")]
    pub envelope_ids: Vec<Bytes>,
    #[prost(message, repeated, tag = "6")]
    pub comm_log: Vec<EventProto>,
    #[prost(uint64, tag = "7")]
    pub cost: u64,
    #[prost(bool, tag = "8")]
    pub aborted: bool,
    #[prost(bool, repeated, tag = "9")]
    pub failed: Vec<bool>,
    #[prost(message, repeated, tag = "10")]
    pub system_deploys: Vec<ProcessedSystemDeployProto>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConcurrentRecord {
    pub target: BlockHash,
    pub pre_root: StateHash,
    pub mid_root: StateHash,
    pub post_root: StateHash,
    pub envelope_ids: Vec<Bytes>,
    pub comm_log: Vec<Event>,
    pub cost: u64,
    pub aborted: bool,
    pub failed: Vec<bool>,
    pub system_deploys: Vec<ProcessedSystemDeploy>,
}

impl ConcurrentRecord {
    pub fn empty(target: BlockHash, pre_root: StateHash, post_root: StateHash) -> Self {
        ConcurrentRecord {
            target,
            mid_root: pre_root.clone(),
            pre_root,
            post_root,
            envelope_ids: vec![],
            comm_log: vec![],
            cost: 0,
            aborted: false,
            failed: vec![],
            system_deploys: vec![],
        }
    }

    fn to_proto(&self) -> ConcurrentRecordProto {
        ConcurrentRecordProto {
            target: self.target.clone(),
            pre_root: self.pre_root.clone(),
            mid_root: self.mid_root.clone(),
            post_root: self.post_root.clone(),
            envelope_ids: self.envelope_ids.clone(),
            comm_log: self.comm_log.iter().map(Event::to_proto).collect(),
            cost: self.cost,
            aborted: self.aborted,
            failed: self.failed.clone(),
            system_deploys: self
                .system_deploys
                .iter()
                .cloned()
                .map(ProcessedSystemDeploy::to_proto)
                .collect(),
        }
    }

    fn from_proto(proto: ConcurrentRecordProto) -> Result<Self, String> {
        Ok(ConcurrentRecord {
            target: proto.target,
            pre_root: proto.pre_root,
            mid_root: proto.mid_root,
            post_root: proto.post_root,
            envelope_ids: proto.envelope_ids,
            comm_log: proto
                .comm_log
                .into_iter()
                .map(Event::from_proto)
                .collect::<Result<_, _>>()?,
            cost: proto.cost,
            aborted: proto.aborted,
            failed: proto.failed,
            system_deploys: proto
                .system_deploys
                .into_iter()
                .map(ProcessedSystemDeploy::from_proto)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl ExecutionRecord for ConcurrentRecord {
    fn target(&self) -> &BlockHash { &self.target }

    fn pre_root(&self) -> &StateHash { &self.pre_root }

    fn post_root(&self) -> &StateHash { &self.post_root }

    fn envelope_ids(&self) -> &[Bytes] { &self.envelope_ids }

    fn encode(&self) -> Bytes { Bytes::from(self.to_proto().encode_to_vec()) }

    fn decode(bytes: &[u8]) -> Result<Self, String> {
        let proto = ConcurrentRecordProto::decode(bytes).map_err(|e| e.to_string())?;
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

    fn record() -> ConcurrentRecord {
        ConcurrentRecord {
            envelope_ids: vec![b("one"), b("two")],
            cost: 7,
            failed: vec![false, true],
            ..ConcurrentRecord::empty(b("t"), b("pre"), b("post"))
        }
    }

    #[test]
    fn a_record_round_trips_through_its_bytes() {
        let record = record();
        let bytes = record.encode();
        assert_eq!(bytes.len(), record.encoded_len());
        assert_eq!(ConcurrentRecord::decode(&bytes).unwrap(), record);
    }

    #[test]
    fn the_digest_covers_every_field_not_only_the_roots() {
        let base = record();
        let verdicts = ConcurrentRecord {
            failed: vec![true, true],
            ..base.clone()
        };
        assert_eq!(base.post_root(), verdicts.post_root());
        assert_ne!(base.digest(), verdicts.digest());
        assert_eq!(base.digest(), base.clone().digest());
    }

    #[test]
    fn a_noncanonical_encoding_is_rejected() {
        let mut bytes = record().encode().to_vec();
        bytes.extend_from_slice(&[0x38, 0x07]);
        assert!(ConcurrentRecord::decode(&bytes).is_err());
    }
}
