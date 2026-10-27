use std::collections::HashSet;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;
use prost::Message;

use super::body::StateHash;

pub const EVIDENCE_DIGEST_DOMAIN: &[u8] = b"f1r3node:v6-evidence:v1";

pub type EvidenceDigest = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EvidenceClass {
    Safety,
    Inclusion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidRecordReason {
    Replay,
    Envelopes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Offence {
    InvalidRecord {
        reason: InvalidRecordReason,
        record: Bytes,
    },
    Misattestation {
        attested: StateHash,
        executed: StateHash,
        canonical_record: Option<Bytes>,
    },
    InclusionViolation {
        envelope_id: Bytes,
        cause: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub offender: Bytes,
    pub citer: BlockHash,
    pub target: BlockHash,
    pub offence: Offence,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EvidenceProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub offender: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub citer: Bytes,
    #[prost(bytes = "bytes", tag = "3")]
    pub target: Bytes,
    #[prost(uint32, tag = "4")]
    pub kind: u32,
    #[prost(uint32, tag = "5")]
    pub reason: u32,
    #[prost(bytes = "bytes", tag = "6")]
    pub record: Bytes,
    #[prost(bytes = "bytes", tag = "7")]
    pub attested: Bytes,
    #[prost(bytes = "bytes", tag = "8")]
    pub executed: Bytes,
    #[prost(bytes = "bytes", tag = "9")]
    pub envelope_id: Bytes,
    #[prost(string, tag = "10")]
    pub cause: String,
}

const KIND_INVALID_RECORD: u32 = 1;
const KIND_MISATTESTATION: u32 = 2;
const KIND_INCLUSION: u32 = 3;
const REASON_REPLAY: u32 = 1;
const REASON_ENVELOPES: u32 = 2;

impl Offence {
    pub fn class(&self) -> EvidenceClass {
        match self {
            Offence::InvalidRecord { .. } | Offence::Misattestation { .. } => EvidenceClass::Safety,
            Offence::InclusionViolation { .. } => EvidenceClass::Inclusion,
        }
    }
}

impl Evidence {
    pub fn class(&self) -> EvidenceClass { self.offence.class() }

    fn to_proto(&self) -> EvidenceProto {
        let mut proto = EvidenceProto {
            offender: self.offender.clone(),
            citer: self.citer.clone(),
            target: self.target.clone(),
            ..EvidenceProto::default()
        };
        match &self.offence {
            Offence::InvalidRecord { reason, record } => {
                proto.kind = KIND_INVALID_RECORD;
                proto.reason = match reason {
                    InvalidRecordReason::Replay => REASON_REPLAY,
                    InvalidRecordReason::Envelopes => REASON_ENVELOPES,
                };
                proto.record = record.clone();
            }
            Offence::Misattestation {
                attested,
                executed,
                canonical_record,
            } => {
                proto.kind = KIND_MISATTESTATION;
                proto.attested = attested.clone();
                proto.executed = executed.clone();
                proto.record = canonical_record.clone().unwrap_or_default();
            }
            Offence::InclusionViolation { envelope_id, cause } => {
                proto.kind = KIND_INCLUSION;
                proto.envelope_id = envelope_id.clone();
                proto.cause = cause.clone();
            }
        }
        proto
    }

    pub fn encode(&self) -> Bytes { Bytes::from(self.to_proto().encode_to_vec()) }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let proto = EvidenceProto::decode(bytes).map_err(|error| error.to_string())?;
        if proto.encode_to_vec() != bytes {
            return Err("v6 evidence must be canonically encoded".to_string());
        }
        let unused = |present: bool, field: &str| {
            if present {
                Err(format!("v6 evidence carries an unused {field}"))
            } else {
                Ok(())
            }
        };
        let offence = match proto.kind {
            KIND_INVALID_RECORD => {
                unused(
                    !proto.attested.is_empty() || !proto.executed.is_empty(),
                    "root",
                )?;
                unused(
                    !proto.envelope_id.is_empty() || !proto.cause.is_empty(),
                    "inclusion field",
                )?;
                if proto.record.is_empty() {
                    return Err("invalid-record evidence needs the record".to_string());
                }
                Offence::InvalidRecord {
                    reason: match proto.reason {
                        REASON_REPLAY => InvalidRecordReason::Replay,
                        REASON_ENVELOPES => InvalidRecordReason::Envelopes,
                        other => return Err(format!("unknown invalid-record reason {other}")),
                    },
                    record: proto.record,
                }
            }
            KIND_MISATTESTATION => {
                unused(proto.reason != 0, "reason")?;
                unused(
                    !proto.envelope_id.is_empty() || !proto.cause.is_empty(),
                    "inclusion field",
                )?;
                if proto.attested == proto.executed {
                    return Err("misattestation evidence needs two different roots".to_string());
                }
                Offence::Misattestation {
                    attested: proto.attested,
                    executed: proto.executed,
                    canonical_record: (!proto.record.is_empty()).then_some(proto.record),
                }
            }
            KIND_INCLUSION => {
                unused(proto.reason != 0, "reason")?;
                unused(!proto.record.is_empty(), "record")?;
                unused(
                    !proto.attested.is_empty() || !proto.executed.is_empty(),
                    "root",
                )?;
                if proto.envelope_id.is_empty() {
                    return Err("inclusion evidence needs the envelope id".to_string());
                }
                Offence::InclusionViolation {
                    envelope_id: proto.envelope_id,
                    cause: proto.cause,
                }
            }
            other => return Err(format!("unknown v6 evidence kind {other}")),
        };
        Ok(Evidence {
            offender: proto.offender,
            citer: proto.citer,
            target: proto.target,
            offence,
        })
    }

    pub fn digest(&self) -> EvidenceDigest {
        Blake2b256::hash_parts([EVIDENCE_DIGEST_DOMAIN, self.encode().as_ref()])
            .try_into()
            .expect("Blake2b256 emits 32 bytes")
    }
}

pub trait EvidenceSink: Send + Sync {
    fn report(&self, evidence: Evidence);

    fn evidence(&self) -> Vec<Evidence>;
}

#[derive(Default)]
pub struct MemoryEvidenceSink {
    state: parking_lot::Mutex<(HashSet<EvidenceDigest>, Vec<Evidence>)>,
}

impl MemoryEvidenceSink {
    pub fn new() -> Self { Self::default() }
}

impl EvidenceSink for MemoryEvidenceSink {
    fn report(&self, evidence: Evidence) {
        let mut state = self.state.lock();
        if !state.0.insert(evidence.digest()) {
            return;
        }
        tracing::warn!(
            target: "f1r3fly.v6",
            class = ?evidence.class(),
            offender = %hex::encode(&evidence.offender),
            citer = %hex::encode(&evidence.citer),
            target_block = %hex::encode(&evidence.target),
            offence = ?std::mem::discriminant(&evidence.offence),
            "v6 evidence"
        );
        state.1.push(evidence);
    }

    fn evidence(&self) -> Vec<Evidence> { self.state.lock().1.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    fn samples() -> Vec<Evidence> {
        let base = |offence| Evidence {
            offender: b("key"),
            citer: b("citer"),
            target: b("target"),
            offence,
        };
        vec![
            base(Offence::InvalidRecord {
                reason: InvalidRecordReason::Replay,
                record: b("record"),
            }),
            base(Offence::InvalidRecord {
                reason: InvalidRecordReason::Envelopes,
                record: b("record"),
            }),
            base(Offence::Misattestation {
                attested: b("a"),
                executed: b("e"),
                canonical_record: Some(b("record")),
            }),
            base(Offence::Misattestation {
                attested: b("a"),
                executed: b("e"),
                canonical_record: None,
            }),
            base(Offence::InclusionViolation {
                envelope_id: b("id"),
                cause: "unfunded".to_string(),
            }),
        ]
    }

    #[test]
    fn evidence_round_trips_canonically_with_distinct_digests() {
        let mut digests = HashSet::new();
        for evidence in samples() {
            let bytes = evidence.encode();
            assert_eq!(Evidence::decode(&bytes), Ok(evidence.clone()));
            assert!(digests.insert(evidence.digest()));
            let mut trailing = bytes.to_vec();
            trailing.extend_from_slice(&[0x08, 0x00]);
            assert!(Evidence::decode(&trailing).is_err());
        }
    }

    #[test]
    fn the_class_follows_the_offence() {
        let classes: Vec<_> = samples().iter().map(Evidence::class).collect();
        assert_eq!(classes, vec![
            EvidenceClass::Safety,
            EvidenceClass::Safety,
            EvidenceClass::Safety,
            EvidenceClass::Safety,
            EvidenceClass::Inclusion,
        ]);
    }

    #[test]
    fn malformed_evidence_is_refused() {
        let wire = |proto: EvidenceProto| proto.encode_to_vec();
        for proto in [
            EvidenceProto {
                kind: 9,
                ..EvidenceProto::default()
            },
            EvidenceProto {
                kind: KIND_INVALID_RECORD,
                reason: REASON_REPLAY,
                ..EvidenceProto::default()
            },
            EvidenceProto {
                kind: KIND_INVALID_RECORD,
                reason: 7,
                record: b("r"),
                ..EvidenceProto::default()
            },
            EvidenceProto {
                kind: KIND_INVALID_RECORD,
                reason: REASON_REPLAY,
                record: b("r"),
                attested: b("a"),
                ..EvidenceProto::default()
            },
            EvidenceProto {
                kind: KIND_MISATTESTATION,
                attested: b("x"),
                executed: b("x"),
                ..EvidenceProto::default()
            },
            EvidenceProto {
                kind: KIND_INCLUSION,
                ..EvidenceProto::default()
            },
            EvidenceProto {
                kind: KIND_INCLUSION,
                envelope_id: b("id"),
                record: b("r"),
                ..EvidenceProto::default()
            },
        ] {
            assert!(Evidence::decode(&wire(proto.clone())).is_err(), "{proto:?}");
        }
    }

    #[test]
    fn the_memory_sink_keeps_each_piece_of_evidence_once() {
        let sink = MemoryEvidenceSink::new();
        for evidence in samples().into_iter().chain(samples()) {
            sink.report(evidence);
        }
        assert_eq!(sink.evidence(), samples());
    }
}
