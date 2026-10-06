use super::*;

fn funding_limits() -> NativeFundingCaseLimits {
    NativeFundingCaseLimits {
        wire: PhloWireLimits {
            total_bytes: 4096,
            field_bytes: 512,
        },
        sources: 2,
        obligations: 2,
        cells: 4,
        custody_bytes: 16,
        key_bytes: 16,
    }
}

fn funding_case() -> NativeFundingCaseV1<'static> {
    NativeFundingCaseV1 {
        sources: vec![
            NativeFundingCaseSource {
                custody: b"alice",
                capacity: 30,
                exposure_limit: 20,
                debit_limit: 20,
                hold: 12,
                debit: 9,
                fee: 1,
                refund: 3,
            },
            NativeFundingCaseSource {
                custody: b"bob",
                capacity: 20,
                exposure_limit: 20,
                debit_limit: 20,
                hold: 8,
                debit: 6,
                fee: 0,
                refund: 2,
            },
        ],
        obligations: vec![
            NativeFundingObligation {
                key: b"fee",
                quantity: 1,
                amount: 1,
            },
            NativeFundingObligation {
                key: b"resource",
                quantity: 2,
                amount: 14,
            },
        ],
        eligible: vec![vec![true, true], vec![false, true]],
        assignment: vec![vec![1, 8], vec![0, 6]],
        resource_next_cursor: Some(1),
        fee_next_cursor: Some(0),
        resource_unrestricted: false,
        resource_restriction_witness: Some(vec![1, 0]),
        possible_fee_payers: vec![true, false],
    }
}

fn prepaid_limits() -> NativePrepaidDeltaLimits {
    NativePrepaidDeltaLimits {
        wire: PhloWireLimits {
            total_bytes: 4096,
            field_bytes: 512,
        },
        draws: 2,
        positions: 3,
        births: 1,
        replacements: 1,
    }
}

#[test]
fn funding_case_roundtrip_and_conservation_rejection() {
    let case = funding_case();
    let encoded = case.encode(funding_limits()).unwrap();
    assert_eq!(
        NativeFundingCaseV1::decode(&encoded, funding_limits()).unwrap(),
        case
    );
    for end in 0..encoded.len() {
        assert!(NativeFundingCaseV1::decode(&encoded[..end], funding_limits()).is_err());
    }
    let mut incorrect = case.clone();
    incorrect.assignment[1][1] += 1;
    assert_eq!(
        incorrect.encode(funding_limits()),
        Err(NativeSectionError::Conservation)
    );
    let mut incorrect = case;
    incorrect.sources.swap(0, 1);
    incorrect.eligible.swap(0, 1);
    incorrect.assignment.swap(0, 1);
    assert_eq!(
        incorrect.encode(funding_limits()),
        Err(NativeSectionError::Noncanonical)
    );
}

#[test]
fn funding_case_rejects_excess_count_before_allocation() {
    let mut wire = PhloWireEncoder::new(funding_limits().wire);
    wire.bytes(FUNDING_CASE_DOMAIN).unwrap();
    wire.u32(u32::MAX).unwrap();
    wire.u32(1).unwrap();
    assert_eq!(
        NativeFundingCaseV1::decode(wire.as_bytes(), funding_limits()),
        Err(NativeSectionError::Limit)
    );
}

#[test]
fn prepaid_delta_roundtrip_and_repeated_position_rejection() {
    let delta = NativePrepaidDeltaV1 {
        draws: vec![
            NativePrepaidDraw {
                stack_id: [1; 32],
                receipt_index: 0,
                positions: vec![0, 2],
            },
            NativePrepaidDraw {
                stack_id: [2; 32],
                receipt_index: 1,
                positions: vec![1],
            },
        ],
        births: vec![NativePrepaidBirth {
            stack_id: [3; 32],
            source_hash: [4; 32],
            encoded_cells_hash: [5; 32],
        }],
        replacements: vec![NativePrepaidReplacement {
            receipt_id: [6; 32],
            expected_hash: None,
            replacement_hash: Some([7; 32]),
        }],
    };
    let encoded = delta.encode(prepaid_limits()).unwrap();
    assert_eq!(
        NativePrepaidDeltaV1::decode(&encoded, prepaid_limits()).unwrap(),
        delta
    );
    let mut repeated = delta.clone();
    repeated.draws[1].positions[0] = 2;
    assert_eq!(
        repeated.encode(prepaid_limits()),
        Err(NativeSectionError::Noncanonical)
    );
    let mut wrong_order = delta;
    wrong_order.draws.swap(0, 1);
    assert_eq!(
        wrong_order.encode(prepaid_limits()),
        Err(NativeSectionError::Noncanonical)
    );
}

#[test]
fn prepaid_delta_accepts_consumption_and_rejects_noop_receipt_transition() {
    let mut delta = NativePrepaidDeltaV1 {
        draws: Vec::new(),
        births: Vec::new(),
        replacements: vec![NativePrepaidReplacement {
            receipt_id: [1; 32],
            expected_hash: Some([2; 32]),
            replacement_hash: None,
        }],
    };
    let encoded = delta.encode(prepaid_limits()).unwrap();
    assert_eq!(
        NativePrepaidDeltaV1::decode(&encoded, prepaid_limits()).unwrap(),
        delta
    );
    delta.replacements[0].replacement_hash = Some([2; 32]);
    assert_eq!(
        delta.encode(prepaid_limits()),
        Err(NativeSectionError::Noncanonical)
    );
}
