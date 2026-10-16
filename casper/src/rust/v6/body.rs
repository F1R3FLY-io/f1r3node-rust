use models::casper::{EventProto, ProcessedSystemDeployProto};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{Event, ProcessedSystemDeploy};
use prost::bytes::Bytes;
use prost::Message;

pub type StateHash = Bytes;

const MAGIC: &[u8] = b"F1R3V6\x01";

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct AttestationProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub block_hash: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub root: Bytes,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RecordProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub target: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub pre_root: Bytes,
    #[prost(bytes = "bytes", tag = "3")]
    pub mid_root: Bytes,
    #[prost(bytes = "bytes", tag = "4")]
    pub post_root: Bytes,
    #[prost(bytes = "bytes", repeated, tag = "5")]
    pub envelope_sigs: Vec<Bytes>,
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

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct V6BodyProto {
    #[prost(message, optional, tag = "1")]
    pub attestation: Option<AttestationProto>,
    #[prost(message, repeated, tag = "2")]
    pub records: Vec<RecordProto>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Attestation {
    pub block_hash: BlockHash,
    pub root: StateHash,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub target: BlockHash,
    pub pre_root: StateHash,
    pub mid_root: StateHash,
    pub post_root: StateHash,
    pub envelope_sigs: Vec<Bytes>,
    pub comm_log: Vec<Event>,
    pub cost: u64,
    pub aborted: bool,
    pub failed: Vec<bool>,
    pub system_deploys: Vec<ProcessedSystemDeploy>,
}

impl Record {
    pub fn empty(target: BlockHash, pre_root: StateHash, post_root: StateHash) -> Self {
        Record {
            target,
            mid_root: pre_root.clone(),
            pre_root,
            post_root,
            envelope_sigs: vec![],
            comm_log: vec![],
            cost: 0,
            aborted: false,
            failed: vec![],
            system_deploys: vec![],
        }
    }

    fn to_proto(&self) -> RecordProto {
        RecordProto {
            target: self.target.clone(),
            pre_root: self.pre_root.clone(),
            mid_root: self.mid_root.clone(),
            post_root: self.post_root.clone(),
            envelope_sigs: self.envelope_sigs.clone(),
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

    fn from_proto(proto: RecordProto) -> Result<Self, String> {
        Ok(Record {
            target: proto.target,
            pre_root: proto.pre_root,
            mid_root: proto.mid_root,
            post_root: proto.post_root,
            envelope_sigs: proto.envelope_sigs,
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

#[derive(Clone, Debug, PartialEq, Default)]
pub struct V6Body {
    pub attestation: Option<Attestation>,
    pub records: Vec<Record>,
}

impl V6Body {
    pub fn encode(&self) -> Bytes {
        let proto = V6BodyProto {
            attestation: self.attestation.as_ref().map(|a| AttestationProto {
                block_hash: a.block_hash.clone(),
                root: a.root.clone(),
            }),
            records: self.records.iter().map(Record::to_proto).collect(),
        };
        let mut out = Vec::with_capacity(MAGIC.len() + proto.encoded_len());
        out.extend_from_slice(MAGIC);
        proto
            .encode(&mut out)
            .expect("encoding into a Vec cannot fail");
        Bytes::from(out)
    }

    pub fn decode(extra_bytes: &Bytes) -> Result<Option<Self>, String> {
        if extra_bytes.is_empty() {
            return Ok(None);
        }
        let payload = extra_bytes
            .strip_prefix(MAGIC)
            .ok_or_else(|| "body extra bytes are not a v6 extension".to_string())?;
        let proto = V6BodyProto::decode(payload).map_err(|e| e.to_string())?;
        Ok(Some(V6Body {
            attestation: proto.attestation.map(|a| Attestation {
                block_hash: a.block_hash,
                root: a.root,
            }),
            records: proto
                .records
                .into_iter()
                .map(Record::from_proto)
                .collect::<Result<_, _>>()?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    #[test]
    fn empty_extra_bytes_is_not_a_v6_body() {
        assert_eq!(V6Body::decode(&Bytes::new()).unwrap(), None);
    }

    #[test]
    fn foreign_extra_bytes_are_rejected() {
        assert!(V6Body::decode(&b("legacy")).is_err());
    }

    #[test]
    fn body_round_trips() {
        let body = V6Body {
            attestation: Some(Attestation {
                block_hash: b("E"),
                root: b("rE"),
            }),
            records: vec![Record::empty(b("t"), b("pre"), b("post"))],
        };
        assert_eq!(V6Body::decode(&body.encode()).unwrap(), Some(body));
    }
}
