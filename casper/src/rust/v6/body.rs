pub use models::rust::casper::protocol::casper_message::{Attestation, V6Body};
use prost::bytes::Bytes;

use super::record::ExecutionRecord;

pub type StateHash = Bytes;

pub trait V6Records: Sized {
    fn with_records<R: ExecutionRecord>(attestation: Attestation, records: &[R]) -> Self;

    fn decode_records<R: ExecutionRecord>(&self) -> Result<Vec<R>, String>;
}

impl V6Records for V6Body {
    fn with_records<R: ExecutionRecord>(attestation: Attestation, records: &[R]) -> Self {
        V6Body {
            attestation: Some(attestation),
            records: records.iter().map(ExecutionRecord::encode).collect(),
        }
    }

    fn decode_records<R: ExecutionRecord>(&self) -> Result<Vec<R>, String> {
        self.records.iter().map(|bytes| R::decode(bytes)).collect()
    }
}

#[cfg(test)]
mod tests {
    use models::casper::BodyProto;
    use models::rust::casper::protocol::casper_message::{Body, F1r3flyState};
    use prost::Message;

    use super::*;
    use crate::rust::v6::block_record::BlockRecord;

    fn b(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    fn body(v6: Option<V6Body>) -> Body {
        Body {
            state: F1r3flyState {
                pre_state_hash: b("pre"),
                post_state_hash: b("post"),
                bonds: vec![],
                block_number: 3,
            },
            deploys: vec![],
            rejected_deploys: vec![],
            system_deploys: vec![],
            extra_bytes: Bytes::new(),
            applied_from_scope: vec![],
            merge_base: b("base"),
            v6,
        }
    }

    #[test]
    fn the_v6_body_round_trips_in_its_field_with_opaque_records() {
        let record = BlockRecord::empty(b("t"), b("pre"), b("post"));
        let v6 = V6Body::with_records(
            Attestation {
                block_hash: b("E"),
                root: b("rE"),
            },
            std::slice::from_ref(&record),
        );
        let wire = body(Some(v6.clone())).to_proto().encode_to_vec();
        let decoded = Body::from_proto(BodyProto::decode(wire.as_slice()).unwrap()).unwrap();
        assert_eq!(decoded.v6, Some(v6.clone()));
        assert_eq!(
            decoded.v6.unwrap().decode_records::<BlockRecord>().unwrap(),
            vec![record]
        );
        assert_eq!(v6.record_bytes(), v6.records[0].len());
    }

    #[test]
    fn a_body_without_the_field_encodes_as_before() {
        let without = body(None).to_proto().encode_to_vec();
        let with = body(Some(V6Body::default())).to_proto().encode_to_vec();
        assert_eq!(&with[..without.len()], without.as_slice());
        assert_eq!(&with[without.len()..], &[0x42, 0x00]);
    }

    #[test]
    fn the_v6_body_carries_records_it_cannot_read() {
        let v6 = V6Body {
            attestation: None,
            records: vec![b("not a record")],
        };
        let decoded = V6Body::from_proto(v6.to_proto());
        assert_eq!(decoded.records, vec![b("not a record")]);
        assert!(decoded.decode_records::<BlockRecord>().is_err());
    }
}
