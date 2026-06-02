// UC-75 — Delimiter-free record key collision (string-concat attack).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-75.
// Reference: docs/theory/slashing/design/05-storage-and-records.md.
//
// Threat: a string-concat key like `format!("{validator}{base_seq}")`
// makes ("1", 23) and ("12", 3) collide on the same key — a hostile
// validator could pick its identity to alias another's record. The
// canonical record key is the tuple `(String, u64)` itself, which the
// store keeps distinct. This file asserts (a) the delimiter-free form
// collides, (b) the canonical form does not, and (c) `EqRecordSet`
// uses the canonical form.

use std::collections::BTreeSet;

use super::types::{EqRecord, EqRecordSet};

fn delimiter_free_key(validator: &str, base_seq: u64) -> String {
    format!("{validator}{base_seq}")
}

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
