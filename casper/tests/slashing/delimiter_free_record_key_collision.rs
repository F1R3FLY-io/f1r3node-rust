use std::collections::BTreeSet;

use super::types::{EqRecord, EqRecordSet};

fn delimiter_free_key(validator: &str, base_seq: u64) -> String { format!("{validator}{base_seq}") }

fn canonical_key(validator: &str, base_seq: u64) -> (String, u64) {
    (validator.to_string(), base_seq)
}

#[test]
fn uc_75_delimiter_free_projection_collides() {
    assert_eq!(delimiter_free_key("1", 23), delimiter_free_key("12", 3));
    assert_ne!(canonical_key("1", 23), canonical_key("12", 3));
}

#[test]
fn uc_75_record_store_keeps_canonical_pairs_distinct() {
    let mut records = EqRecordSet::default();
    records.insert_or_update(EqRecord {
        equivocator: "1".to_string(),
        base_seq: 23,
        witnesses: BTreeSet::from([101]),
    });
    records.insert_or_update(EqRecord {
        equivocator: "12".to_string(),
        base_seq: 3,
        witnesses: BTreeSet::from([202]),
    });

    assert_eq!(records.records.len(), 2);
    assert_eq!(records.witnesses("1", 23), BTreeSet::from([101]));
    assert_eq!(records.witnesses("12", 3), BTreeSet::from([202]));
}
