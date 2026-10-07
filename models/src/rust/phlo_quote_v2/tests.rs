use super::*;
use crate::rust::phlo_intent::quote_evidence_commitment;

fn limits() -> PhloWireLimits {
    PhloWireLimits {
        total_bytes: 2048,
        field_bytes: 256,
    }
}

fn sample() -> PhloQuoteEvidenceV2<'static> {
    PhloQuoteEvidenceV2 {
        quote_id: [1; 32],
        network: b"testnet",
        shard: b"root",
        context_commitment: [2; 32],
        input_custody: b"input-purse",
        input_asset: b"INPUT",
        input_authority: b"input-owner",
        provider_custody: b"provider-purse",
        provider_authority: b"provider-owner",
        output_asset: b"REV",
        output_recipient: b"purse-b",
        fee_recipient: b"fee-purse",
        schedule_commitment: [7; 32],
        valid_from: Some(5),
        valid_until: Some(10),
        use_policy: PhloQuoteUseV2::Once,
        max_output: 10,
        max_input_debit: 20,
        input_hold_cap: 30,
        provider_hold_cap: 15,
        rate_numerator: 3,
        rate_denominator: 2,
        fixed_input_fee: 2,
        input_scale: 1,
        output_scale: 1,
    }
}

fn pack(fields: &[&[u8]]) -> Vec<u8> {
    fields
        .iter()
        .flat_map(|field| {
            (field.len() as u64)
                .to_be_bytes()
                .into_iter()
                .chain(field.iter().copied())
        })
        .collect()
}

fn fields(input: &[u8]) -> Vec<&[u8]> {
    let mut decoder = PhloWireDecoder::new(input, limits()).unwrap();
    let mut output = Vec::new();
    while !decoder.remaining().is_empty() {
        output.push(decoder.bytes().unwrap());
    }
    output
}

#[test]
fn quote_roundtrip_and_canonical_field_order() {
    let quote = sample();
    let bytes = quote.encode(limits()).unwrap();
    let decoded = PhloQuoteEvidenceV2::decode(&bytes, limits()).unwrap();
    assert_eq!(decoded, quote);
    assert_eq!(decoded.encode(limits()).unwrap(), bytes);
    let packed = pack(&[
        PHLO_QUOTE_V2_DOMAIN,
        &[1; 32],
        b"testnet",
        b"root",
        &[2; 32],
        b"input-purse",
        b"INPUT",
        b"input-owner",
        b"provider-purse",
        b"provider-owner",
        b"REV",
        b"purse-b",
        b"fee-purse",
        &[7; 32],
        &[1, 0, 0, 0, 0, 0, 0, 0, 5],
        &[1, 0, 0, 0, 0, 0, 0, 0, 10],
        &[0; 17],
        &10u128.to_be_bytes(),
        &20u128.to_be_bytes(),
        &30u128.to_be_bytes(),
        &15u128.to_be_bytes(),
        &[1],
        &3u128.to_be_bytes(),
        &2u128.to_be_bytes(),
        &2u128.to_be_bytes(),
        &1u128.to_be_bytes(),
        &1u128.to_be_bytes(),
        &[PHLO_ATOMIC_ORIGINAL_INPUT_RELEASE_V2],
    ]);
    assert_eq!(bytes, packed);
}

#[test]
fn fee_recipient_is_mandatory_and_changes_the_signed_quote_commitment() {
    let original = sample().encode(limits()).unwrap();
    let mut changed = sample();
    changed.fee_recipient = b"another-fee-purse";
    let changed_bytes = changed.encode(limits()).unwrap();
    assert_ne!(
        quote_evidence_commitment(&original),
        quote_evidence_commitment(&changed_bytes)
    );
    assert_eq!(
        PhloQuoteEvidenceV2::decode(&changed_bytes, limits())
            .unwrap()
            .fee_recipient,
        b"another-fee-purse"
    );
    let mut missing = fields(&original);
    missing.remove(12);
    assert!(PhloQuoteEvidenceV2::decode(&pack(&missing), limits()).is_err());
    let mut empty = fields(&original);
    empty[12] = b"";
    assert_eq!(
        PhloQuoteEvidenceV2::decode(&pack(&empty), limits()),
        Err(PhloQuoteV2Error::EmptyIdentity)
    );
}

#[test]
fn exact_output_rounds_up_before_fee_and_zero_is_free() {
    let quote = sample();
    for (output, base, total) in [(0, 0, 0), (1, 2, 4), (2, 3, 5), (3, 5, 7), (10, 15, 17)] {
        assert_eq!(
            quote.evaluate_exact_output(output),
            Ok(PhloQuoteAmountV2 {
                base_input: base,
                conversion_fee: if output == 0 { 0 } else { 2 },
                total_input: total,
            })
        );
    }
    assert_eq!(
        quote.evaluate_exact_output(11),
        Err(PhloQuoteV2Error::OutputDomain)
    );
    for numerator in 1..8 {
        for denominator in 1..8 {
            for output in 0..=10 {
                let mut quote = sample();
                quote.rate_numerator = numerator;
                quote.rate_denominator = denominator;
                quote.max_input_debit = 100;
                quote.input_hold_cap = 100;
                let expected = if output == 0 {
                    0
                } else {
                    (numerator * output + denominator - 1) / denominator + 2
                };
                assert_eq!(
                    quote.evaluate_exact_output(output).unwrap().total_input,
                    expected
                );
            }
        }
    }
}

#[test]
fn quote_rejects_overflow_and_independent_caps() {
    let mut quote = sample();
    quote.max_input_debit = 3;
    assert_eq!(
        quote.evaluate_exact_output(1),
        Err(PhloQuoteV2Error::InputCap)
    );
    quote.max_input_debit = u128::MAX;
    quote.input_hold_cap = u128::MAX;
    quote.rate_numerator = u128::MAX;
    assert_eq!(
        quote.evaluate_exact_output(2),
        Err(PhloQuoteV2Error::ArithmeticOverflow)
    );
    quote.rate_numerator = u128::MAX - 1;
    quote.rate_denominator = 1;
    assert_eq!(
        quote.evaluate_exact_output(1),
        Err(PhloQuoteV2Error::ArithmeticOverflow)
    );
    let mut zero_only = sample();
    zero_only.max_output = 0;
    zero_only.max_input_debit = 0;
    zero_only.fixed_input_fee = u128::MAX;
    assert_eq!(zero_only.evaluate_exact_output(0).unwrap().total_input, 0);
    quote.max_input_debit = 31;
    quote.input_hold_cap = 30;
    assert_eq!(quote.encode(limits()), Err(PhloQuoteV2Error::Bounds));
    quote.max_input_debit = 20;
    quote.max_output = 16;
    assert_eq!(quote.encode(limits()), Err(PhloQuoteV2Error::Bounds));
}

#[test]
fn quote_rejects_unknown_rules_malformed_fields_and_invalid_validity() {
    let bytes = sample().encode(limits()).unwrap();
    let original = fields(&bytes);
    assert_eq!(original.len(), 28);
    let mut changed = original.clone();
    changed[0] = b"wrong-format";
    assert_eq!(
        PhloQuoteEvidenceV2::decode(&pack(&changed), limits()),
        Err(PhloQuoteV2Error::Format)
    );
    let mut changed = original.clone();
    changed[16] = &[2; 17];
    assert_eq!(
        PhloQuoteEvidenceV2::decode(&pack(&changed), limits()),
        Err(PhloQuoteV2Error::UseRule)
    );
    let mut changed = original.clone();
    changed[21] = &[2];
    assert_eq!(
        PhloQuoteEvidenceV2::decode(&pack(&changed), limits()),
        Err(PhloQuoteV2Error::PricingRule)
    );
    let mut changed = original.clone();
    changed[23] = &[0; 16];
    assert_eq!(
        PhloQuoteEvidenceV2::decode(&pack(&changed), limits()),
        Err(PhloQuoteV2Error::PricingRule)
    );
    let mut changed = original.clone();
    changed[27] = &[2];
    assert_eq!(
        PhloQuoteEvidenceV2::decode(&pack(&changed), limits()),
        Err(PhloQuoteV2Error::FailureRule)
    );
    let mut changed = original.clone();
    changed[14] = &[1];
    assert_eq!(
        PhloQuoteEvidenceV2::decode(&pack(&changed), limits()),
        Err(PhloQuoteV2Error::FieldWidth)
    );
    let mut changed = original.clone();
    changed[1] = &[1; 31];
    assert_eq!(
        PhloQuoteEvidenceV2::decode(&pack(&changed), limits()),
        Err(PhloQuoteV2Error::FieldWidth)
    );
    let mut quote = sample();
    quote.valid_from = Some(11);
    assert_eq!(quote.encode(limits()), Err(PhloQuoteV2Error::Bounds));
    quote = sample();
    quote.rate_numerator = 0;
    assert_eq!(quote.encode(limits()), Err(PhloQuoteV2Error::PricingRule));
    quote = sample();
    quote.network = b"";
    assert_eq!(quote.encode(limits()), Err(PhloQuoteV2Error::EmptyIdentity));
    quote = sample();
    quote.fee_recipient = b"";
    assert_eq!(quote.encode(limits()), Err(PhloQuoteV2Error::EmptyIdentity));
    for cut in 0..bytes.len() {
        assert!(PhloQuoteEvidenceV2::decode(&bytes[..cut], limits()).is_err());
    }
    assert!(PhloQuoteEvidenceV2::decode(&[bytes.as_slice(), b"extra"].concat(), limits()).is_err());
}

#[test]
fn quote_use_policy_has_canonical_cap_and_presence() {
    let mut quote = sample();
    quote.use_policy = PhloQuoteUseV2::Cumulative { output_ceiling: 9 };
    assert_eq!(quote.encode(limits()), Err(PhloQuoteV2Error::Bounds));
    quote.use_policy = PhloQuoteUseV2::Cumulative {
        output_ceiling: 100,
    };
    let bytes = quote.encode(limits()).unwrap();
    assert_eq!(
        PhloQuoteEvidenceV2::decode(&bytes, limits()).unwrap(),
        quote
    );
    let mut changed = fields(&bytes);
    changed[16] = &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        PhloQuoteEvidenceV2::decode(&pack(&changed), limits()),
        Err(PhloQuoteV2Error::UseRule)
    );
}
