use super::*;

fn limits() -> PhloWireLimits {
    PhloWireLimits {
        total_bytes: 4096,
        field_bytes: 1024,
    }
}

fn evidence<'a>(payload: &'a [u8]) -> NativeCostEvidenceV1<'a> {
    NativeCostEvidenceV1 {
        envelope_commitment: [1; 32],
        genesis_policy_commitment: [2; 32],
        schedule_commitment: [3; 32],
        original_funding_root: [4; 32],
        settlement_runtime_root: [5; 32],
        post_state_root: [6; 32],
        phlo_used: 17,
        fresh_phlo: 17,
        retained_phlo: 0,
        phlo_limit: 23,
        phlo_price: 7,
        fee_rev: 11,
        failure_class: NativeCostFailureClass::Success,
        wallet_settlement_log_events: 5,
        budget_recording: payload,
        operation_journal: payload,
        funding_case: payload,
        prepaid_delta: payload,
        wallet_settlement: payload,
    }
}

#[test]
fn evidence_round_trips_and_includes_separate_fee_in_rev_amounts() {
    let record = evidence(b"bounded");
    let encoded = record.encode(limits()).unwrap();
    assert_eq!(record.encoded_len(limits()).unwrap(), encoded.len());
    assert_eq!(
        NativeCostEvidenceV1::decode(&encoded, limits()).unwrap(),
        record
    );
    assert_eq!(record.rev_ceiling().unwrap(), 23 * 7 + 11);
    assert_eq!(record.rev_spent().unwrap(), 17 * 7 + 11);
}

#[test]
fn evidence_rejects_truncation_trailing_bytes_and_malformed_fields() {
    let encoded = evidence(b"bounded").encode(limits()).unwrap();
    for size in 0..encoded.len() {
        assert!(NativeCostEvidenceV1::decode(&encoded[..size], limits()).is_err());
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert_eq!(
        NativeCostEvidenceV1::decode(&trailing, limits()),
        Err(NativeCostEvidenceError::Wire(PhloWireError::TrailingBytes))
    );
    let mut bad_domain = encoded.clone();
    bad_domain[8] ^= 1;
    assert_eq!(
        NativeCostEvidenceV1::decode(&bad_domain, limits()),
        Err(NativeCostEvidenceError::FormatDomain)
    );
    assert!(NativeCostEvidenceV1::decode(&encoded, PhloWireLimits {
        total_bytes: encoded.len() - 1,
        field_bytes: 1024,
    })
    .is_err());
}

#[test]
fn evidence_rejects_over_limit_usage_and_field_limit() {
    let mut record = evidence(b"bounded");
    record.phlo_used = record.phlo_limit + 1;
    assert_eq!(
        record.encode(limits()),
        Err(NativeCostEvidenceError::PhloLimit)
    );
    let record = evidence(&[8; 1025]);
    assert_eq!(
        record.encode(limits()),
        Err(NativeCostEvidenceError::Wire(PhloWireError::LimitExceeded))
    );
}

#[test]
fn evidence_prices_fresh_and_retained_phlo_within_the_execution_limit() {
    let mut record = evidence(b"bounded");
    record.fresh_phlo = 9;
    record.retained_phlo = 4;
    assert_eq!(record.resource_rev().unwrap(), 13 * 7);
    assert_eq!(record.rev_spent().unwrap(), 13 * 7 + 11);
    assert_eq!(
        NativeCostEvidenceV1::decode(&record.encode(limits()).unwrap(), limits()).unwrap(),
        record
    );
    record.retained_phlo = 7;
    assert_eq!(
        record.encode(limits()),
        Err(NativeCostEvidenceError::PhloLimit)
    );
    record.retained_phlo = 0;
    record.fresh_phlo = 18;
    assert_eq!(
        record.encode(limits()),
        Err(NativeCostEvidenceError::PhloBreakdown)
    );
}
