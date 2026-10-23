use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;
use prost::Message;

use super::record::ExecutionRecord;

pub type StateHash = Bytes;

const MAGIC: &[u8] = b"F1R3V6\x02";

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct AttestationProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub block_hash: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub root: Bytes,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct V6BodyProto {
    #[prost(message, optional, tag = "1")]
    pub attestation: Option<AttestationProto>,
    #[prost(bytes = "bytes", repeated, tag = "2")]
    pub records: Vec<Bytes>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Attestation {
    pub block_hash: BlockHash,
    pub root: StateHash,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct V6Body {
    pub attestation: Option<Attestation>,
    pub records: Vec<Bytes>,
}

impl V6Body {
    pub fn with_records<R: ExecutionRecord>(attestation: Attestation, records: &[R]) -> Self {
        V6Body {
            attestation: Some(attestation),
            records: records.iter().map(ExecutionRecord::encode).collect(),
        }
    }

    pub fn decode_records<R: ExecutionRecord>(&self) -> Result<Vec<R>, String> {
        self.records.iter().map(|bytes| R::decode(bytes)).collect()
    }

    pub fn record_bytes(&self) -> usize { self.records.iter().map(Bytes::len).sum() }

    pub fn encode(&self) -> Bytes {
        let proto = V6BodyProto {
            attestation: self.attestation.as_ref().map(|a| AttestationProto {
                block_hash: a.block_hash.clone(),
                root: a.root.clone(),
            }),
            records: self.records.clone(),
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
            records: proto.records,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::v6::concurrent_record::ConcurrentRecord;

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
    fn body_round_trips_with_opaque_records() {
        let record = ConcurrentRecord::empty(b("t"), b("pre"), b("post"));
        let body = V6Body::with_records(
            Attestation {
                block_hash: b("E"),
                root: b("rE"),
            },
            std::slice::from_ref(&record),
        );
        let decoded = V6Body::decode(&body.encode()).unwrap().unwrap();
        assert_eq!(decoded, body);
        assert_eq!(decoded.decode_records::<ConcurrentRecord>().unwrap(), vec![
            record
        ]);
        assert_eq!(decoded.record_bytes(), body.records[0].len());
    }

    #[test]
    fn the_body_carries_records_it_cannot_read() {
        let body = V6Body {
            attestation: None,
            records: vec![b("not a record")],
        };
        let decoded = V6Body::decode(&body.encode()).unwrap().unwrap();
        assert_eq!(decoded.records, vec![b("not a record")]);
        assert!(decoded.decode_records::<ConcurrentRecord>().is_err());
    }
}
