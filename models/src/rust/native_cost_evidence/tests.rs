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
        // Added by G3 (DR-135, S7): no owner funding.
        owner_phlo: 0,
        owner_rev: 0,
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

/// Added by G3 (DR-135, S7): the v1 encoding of `record`, which has no owner
/// terms.
fn encode_v1(record: &NativeCostEvidenceV1<'_>) -> Vec<u8> {
    let mut wire = PhloWireEncoder::new(limits());
    for field in [
        NATIVE_COST_EVIDENCE_V1_DOMAIN,
        &record.envelope_commitment,
        &record.genesis_policy_commitment,
        &record.schedule_commitment,
        &record.original_funding_root,
        &record.settlement_runtime_root,
        &record.post_state_root,
        &record.phlo_used.to_be_bytes(),
        &record.fresh_phlo.to_be_bytes(),
        &record.retained_phlo.to_be_bytes(),
        &record.phlo_limit.to_be_bytes(),
        &record.phlo_price.to_be_bytes(),
        &record.fee_rev.to_be_bytes(),
        &[record.failure_class.as_byte()],
        &record.wallet_settlement_log_events.to_be_bytes(),
        record.budget_recording,
        record.operation_journal,
        record.funding_case,
        record.prepaid_delta,
        record.wallet_settlement,
    ] {
        wire.bytes(field).expect("a v1 field fits the limits");
    }
    wire.into_bytes()
}

/// Added by G3 (DR-135, S7): v2 evidence commits the owner phlo and the owner
/// REV (plan §5). The client pays the fresh phlo that no owner paid at the
/// signed price, and the owners pay the owner REV at their own prices. With
/// equal prices the v1 amount holds. A v1 record decodes with no owner terms.
#[test]
fn evidence_v2_resource_rev_counts_owner_rev() {
    let mut record = evidence(b"bounded");
    record.fresh_phlo = 12;
    record.retained_phlo = 3;
    record.owner_phlo = 5;
    record.owner_rev = 5 * 9;
    assert_eq!(record.resource_rev().unwrap(), (12 - 5 + 3) * 7 + 5 * 9);
    assert_eq!(record.rev_spent().unwrap(), (12 - 5 + 3) * 7 + 5 * 9 + 11);
    let encoded = record.encode(limits()).unwrap();
    assert_eq!(record.encoded_len(limits()).unwrap(), encoded.len());
    assert_eq!(
        NativeCostEvidenceV1::decode(&encoded, limits()).unwrap(),
        record
    );
    record.owner_rev = 5 * 7;
    assert_eq!(record.resource_rev().unwrap(), (12 + 3) * 7);
    record.owner_phlo = 13;
    assert_eq!(
        record.encode(limits()),
        Err(NativeCostEvidenceError::OwnerPhlo)
    );
    let v1 = evidence(b"bounded");
    assert_eq!(
        NativeCostEvidenceV1::decode(&encode_v1(&v1), limits()).unwrap(),
        v1
    );
}

/// Added by G3 (DR-135, S11): item 13. The signed limit bounds only the
/// caller's part of the used phlo. A run that uses 5 past the limit is
/// accepted when its owners paid those 5, and it exceeds the limit when
/// they paid 4. An owner part above the used phlo is rejected.
#[test]
fn the_signed_limit_bounds_only_the_client_part() {
    let mut record = evidence(b"bounded");
    record.phlo_limit = record.phlo_used - 5;
    record.owner_phlo = 5;
    record.owner_rev = 5 * 9;
    assert_eq!(
        record
            .resource_rev()
            .expect("the owners paid the 5 past the limit"),
        (17 - 5) * 7 + 5 * 9
    );
    let encoded = record
        .encode(limits())
        .expect("the client part fits the limit");
    assert_eq!(
        NativeCostEvidenceV1::decode(&encoded, limits()).expect("the evidence decodes"),
        record
    );
    record.owner_phlo = 4;
    assert_eq!(
        record.encode(limits()),
        Err(NativeCostEvidenceError::PhloLimit)
    );
    record.owner_phlo = record.phlo_used + 1;
    assert_eq!(
        record.encode(limits()),
        Err(NativeCostEvidenceError::OwnerPhlo)
    );
}
