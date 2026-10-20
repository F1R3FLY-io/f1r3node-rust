use std::sync::OnceLock;

use super::*;
use crate::rust::phlo_controls::{PhloControlsLimits, PhloControlsV1};
use crate::rust::phlo_quote_v2::{PhloQuoteEvidenceV2, PhloQuoteUseV2};
use crate::rust::phlo_source::PhloSourcePolicyV1;

fn limits() -> PhloFundingIntentV2Limits {
    let wire = PhloWireLimits {
        total_bytes: 8192,
        field_bytes: 4096,
    };
    PhloFundingIntentV2Limits {
        wire,
        base: PhloFundingIntentLimits {
            wire,
            controls: PhloControlsLimits {
                wire,
                owners: 2,
                schedules: 2,
                total_classes: 2,
            },
            sources: 2,
            resource_permissions: 2,
            authority_nodes: 2,
        },
        grant_uses: 2,
        grant_id_bytes: 32,
        quote_evidence_bytes: 2048,
    }
}

fn base() -> PhloFundingIntentV1<'static> {
    let source_limits = limits().base.source();
    PhloFundingIntentV1 {
        controls: PhloControlsV1 {
            limit: 10,
            price_ceiling: 2,
            required_owner_ceilings: vec![2],
            permitted_schedules: vec![],
        },
        schedule_commitment: [7; 32],
        total_exposure: 50,
        sources: vec![
            PhloSourcePolicyV1::new(b"purse-a", 50, 50, true, vec![], source_limits).unwrap(),
            PhloSourcePolicyV1::new(b"purse-b", 50, 50, true, vec![], source_limits).unwrap(),
        ],
    }
}

fn grant(id: &'static [u8], source_index: u32) -> PhloFundingGrantUseV2<'static> {
    PhloFundingGrantUseV2 {
        source_index,
        grant_id: id,
        authority_version: 3,
        operation_id: [4; 32],
        max_draw: 10,
        cumulative_ceiling: 100,
        valid_from: Some(5),
        valid_until: Some(10),
    }
}

fn intent(conversion: PhloConversionCompositionV2<'static>) -> PhloFundingIntentV2<'static> {
    PhloFundingIntentV2 {
        base: base(),
        grant_uses: vec![grant(b"grant-a", 0), grant(b"grant-b", 1)],
        conversion,
    }
}

fn quote() -> PhloConversionCompositionV2<'static> {
    let quote_evidence = quote_bytes();
    PhloConversionCompositionV2::AtomicQuote {
        quote_commitment: quote_evidence_commitment(quote_evidence),
        quote_evidence,
        output_source_index: 1,
        input_custody: b"input-purse",
        input_asset: b"INPUT",
        provider_custody: b"provider-purse",
        output_asset: b"REV",
        max_input_debit: 20,
        max_output_debit: 10,
        input_hold_cap: 30,
        provider_hold_cap: 15,
    }
}

fn quote_bytes() -> &'static [u8] {
    static QUOTE: OnceLock<Vec<u8>> = OnceLock::new();
    QUOTE.get_or_init(|| {
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
        .encode(PhloWireLimits {
            total_bytes: 2048,
            field_bytes: 2048,
        })
        .unwrap()
    })
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
    let mut decoder = PhloWireDecoder::new(input, limits().wire).unwrap();
    let mut output = Vec::new();
    while !decoder.remaining().is_empty() {
        output.push(decoder.bytes().unwrap());
    }
    output
}

fn reference_grant(grant: &PhloFundingGrantUseV2<'_>) -> Vec<u8> {
    let from = [vec![1], grant.valid_from.unwrap().to_be_bytes().to_vec()].concat();
    let until = [vec![1], grant.valid_until.unwrap().to_be_bytes().to_vec()].concat();
    pack(&[
        &grant.source_index.to_be_bytes(),
        grant.grant_id,
        &grant.authority_version.to_be_bytes(),
        &grant.operation_id,
        &grant.max_draw.to_be_bytes(),
        &grant.cumulative_ceiling.to_be_bytes(),
        &from,
        &until,
    ])
}

#[test]
fn v2_wire_field_order_matches_independent_reference_packing() {
    let record = intent(PhloConversionCompositionV2::NoConversion);
    let first = reference_grant(&record.grant_uses[0]);
    let second = reference_grant(&record.grant_uses[1]);
    let expected = pack(&[
        PHLO_FUNDING_INTENT_V2_DOMAIN,
        &record.base.encode(limits().base()).unwrap(),
        &pack(&[&2u32.to_be_bytes(), &first, &second]),
        &pack(&[&[0]]),
    ]);
    assert_eq!(record.encode(limits()).unwrap(), expected);

    let record = intent(PhloConversionCompositionV2::SeparatePrior {
        accepted_trade: [9; 32],
        output_source_index: 1,
        output_asset: b"REV",
    });
    let expected = pack(&[
        PHLO_FUNDING_INTENT_V2_DOMAIN,
        &record.base.encode(limits().base()).unwrap(),
        &pack(&[&2u32.to_be_bytes(), &first, &second]),
        &pack(&[&[1], &[9; 32], &1u32.to_be_bytes(), b"REV"]),
    ]);
    assert_eq!(record.encode(limits()).unwrap(), expected);
}

#[test]
fn v1_bytes_and_historical_decoder_are_preserved() {
    let old = base();
    let bytes = old.encode(limits().base()).unwrap();
    assert_eq!(
        PhloFundingIntentV1::decode(&bytes, limits().base()).unwrap(),
        old
    );
    assert_eq!(
        PhloFundingIntentVersioned::decode(&bytes, limits()).unwrap(),
        PhloFundingIntentVersioned::V1(old)
    );
    assert_eq!(
        PhloFundingIntentVersioned::decode(&bytes, limits())
            .unwrap()
            .encode(limits())
            .unwrap(),
        bytes
    );
}

#[test]
fn v2_roundtrips_all_three_explicit_compositions_with_distinct_signed_bytes() {
    let cases = [
        PhloConversionCompositionV2::NoConversion,
        PhloConversionCompositionV2::SeparatePrior {
            accepted_trade: [9; 32],
            output_source_index: 1,
            output_asset: b"REV",
        },
        quote(),
    ];
    let mut bytes = Vec::new();
    for conversion in cases {
        let record = intent(conversion);
        let encoded = record.encode(limits()).unwrap();
        assert_eq!(
            PhloFundingIntentV2::decode(&encoded, limits()).unwrap(),
            record
        );
        assert_eq!(
            PhloFundingIntentVersioned::decode(&encoded, limits()).unwrap(),
            PhloFundingIntentVersioned::V2(record)
        );
        assert_eq!(
            PhloFundingIntentV2::decode(&encoded, limits())
                .unwrap()
                .encode(limits())
                .unwrap(),
            encoded
        );
        bytes.push(encoded);
    }
    assert!(bytes[0] != bytes[1] && bytes[1] != bytes[2] && bytes[0] != bytes[2]);
}

#[test]
fn v2_grant_fields_are_signed_and_strictly_bounded() {
    let original = intent(PhloConversionCompositionV2::NoConversion);
    let bytes = original.encode(limits()).unwrap();
    let mut variants = Vec::new();
    let mut changed = original.clone();
    changed.grant_uses[0].authority_version += 1;
    variants.push(changed);
    let mut changed = original.clone();
    changed.grant_uses[0].operation_id[0] ^= 1;
    variants.push(changed);
    let mut changed = original.clone();
    changed.grant_uses[0].max_draw += 1;
    variants.push(changed);
    let mut changed = original.clone();
    changed.grant_uses[0].cumulative_ceiling += 1;
    variants.push(changed);
    let mut changed = original.clone();
    changed.grant_uses[0].valid_from = None;
    variants.push(changed);
    let mut changed = original.clone();
    changed.grant_uses[0].valid_until = None;
    variants.push(changed);
    let mut changed = original.clone();
    changed.grant_uses[0].grant_id = b"changed";
    variants.push(changed);
    for changed in variants {
        let altered = changed.encode(limits()).unwrap();
        assert_ne!(altered, bytes);
        assert_eq!(
            PhloFundingIntentV2::decode(&altered, limits()).unwrap(),
            changed
        );
    }
    let mut bad = original.clone();
    bad.grant_uses.swap(0, 1);
    assert_eq!(
        bad.encode(limits()),
        Err(PhloFundingIntentV2Error::GrantOrder)
    );
    let mut bad = original.clone();
    bad.grant_uses[1] = bad.grant_uses[0].clone();
    assert_eq!(
        bad.encode(limits()),
        Err(PhloFundingIntentV2Error::GrantOrder)
    );
    let mut bad = original.clone();
    bad.grant_uses[0].source_index = 2;
    assert_eq!(
        bad.encode(limits()),
        Err(PhloFundingIntentV2Error::SourceIndex)
    );
    let mut bad = original.clone();
    bad.grant_uses[0].grant_id = b"";
    assert_eq!(bad.encode(limits()), Err(PhloFundingIntentV2Error::GrantId));
    let mut bad = original.clone();
    bad.grant_uses[0].max_draw = 101;
    assert_eq!(
        bad.encode(limits()),
        Err(PhloFundingIntentV2Error::GrantBounds)
    );
    let mut bad = original.clone();
    bad.grant_uses[0].valid_from = Some(11);
    assert_eq!(
        bad.encode(limits()),
        Err(PhloFundingIntentV2Error::GrantBounds)
    );
    assert_eq!(
        original.encode(PhloFundingIntentV2Limits {
            grant_uses: 1,
            ..limits()
        }),
        Err(PhloFundingIntentV2Error::GrantUseLimit)
    );
    assert_eq!(
        PhloFundingIntentV2::decode(&bytes, PhloFundingIntentV2Limits {
            grant_uses: 1,
            ..limits()
        }),
        Err(PhloFundingIntentV2Error::GrantUseLimit)
    );
    let short_id = PhloFundingIntentV2Limits {
        grant_id_bytes: 6,
        ..limits()
    };
    assert_eq!(
        original.encode(short_id),
        Err(PhloFundingIntentV2Error::GrantId)
    );
    assert_eq!(
        PhloFundingIntentV2::decode(&bytes, short_id),
        Err(PhloFundingIntentV2Error::GrantId)
    );
    let outer = fields(&bytes);
    let first = reference_grant(&original.grant_uses[0]);
    let second = reference_grant(&original.grant_uses[1]);
    let reversed = pack(&[&2u32.to_be_bytes(), &second, &first]);
    let bad = pack(&[outer[0], outer[1], &reversed, outer[3]]);
    assert_eq!(
        PhloFundingIntentV2::decode(&bad, limits()),
        Err(PhloFundingIntentV2Error::GrantOrder)
    );
}

#[test]
fn v2_quote_and_composition_caps_are_canonical_and_signed() {
    let original = intent(quote());
    let bytes = original.encode(limits()).unwrap();
    let mut changed = original.clone();
    if let PhloConversionCompositionV2::AtomicQuote { quote_evidence, .. } = &mut changed.conversion
    {
        *quote_evidence = b"changed quote";
    }
    assert_eq!(
        changed.encode(limits()),
        Err(PhloFundingIntentV2Error::QuoteCommitment)
    );
    let mut changed = original.clone();
    if let PhloConversionCompositionV2::AtomicQuote {
        quote_commitment,
        quote_evidence,
        ..
    } = &mut changed.conversion
    {
        let mut record = PhloQuoteEvidenceV2::decode(quote_evidence, PhloWireLimits {
            total_bytes: 2048,
            field_bytes: 2048,
        })
        .unwrap();
        record.quote_id[0] ^= 1;
        *quote_evidence = Box::leak(
            record
                .encode(PhloWireLimits {
                    total_bytes: 2048,
                    field_bytes: 2048,
                })
                .unwrap()
                .into_boxed_slice(),
        );
        *quote_commitment = quote_evidence_commitment(quote_evidence);
    }
    assert_ne!(changed.encode(limits()).unwrap(), bytes);
    let mut bad = original.clone();
    if let PhloConversionCompositionV2::AtomicQuote { input_asset, .. } = &mut bad.conversion {
        *input_asset = b"OTHER";
    }
    assert_eq!(
        bad.encode(limits()),
        Err(PhloFundingIntentV2Error::QuoteMismatch)
    );
    let mut bad = original.clone();
    if let PhloConversionCompositionV2::AtomicQuote {
        max_input_debit, ..
    } = &mut bad.conversion
    {
        *max_input_debit = 31;
    }
    assert_eq!(
        bad.encode(limits()),
        Err(PhloFundingIntentV2Error::ConversionBounds)
    );
    let mut bad = original.clone();
    if let PhloConversionCompositionV2::AtomicQuote {
        max_output_debit, ..
    } = &mut bad.conversion
    {
        *max_output_debit = 16;
    }
    assert_eq!(
        bad.encode(limits()),
        Err(PhloFundingIntentV2Error::ConversionBounds)
    );
    let mut bad = original.clone();
    if let PhloConversionCompositionV2::AtomicQuote { quote_evidence, .. } = &mut bad.conversion {
        *quote_evidence = b"";
    }
    assert_eq!(
        bad.encode(limits()),
        Err(PhloFundingIntentV2Error::ConversionIdentity)
    );
    assert_eq!(
        PhloFundingIntentV2::decode(&bytes, PhloFundingIntentV2Limits {
            quote_evidence_bytes: 5,
            ..limits()
        }),
        Err(PhloFundingIntentV2Error::ConversionIdentity)
    );
}

#[test]
fn v2_rejects_unknown_tags_noncanonical_endpoints_wrong_widths_and_trailing_bytes() {
    let original = intent(PhloConversionCompositionV2::NoConversion);
    let bytes = original.encode(limits()).unwrap();
    let outer = fields(&bytes);
    assert_eq!(outer.len(), 4);
    let bad_tag = pack(&[outer[0], outer[1], outer[2], &pack(&[&[3]])]);
    assert_eq!(
        PhloFundingIntentV2::decode(&bad_tag, limits()),
        Err(PhloFundingIntentV2Error::ConversionTag)
    );
    let extra_conversion = pack(&[outer[0], outer[1], outer[2], &pack(&[&[0], b"extra"])]);
    assert!(PhloFundingIntentV2::decode(&extra_conversion, limits()).is_err());
    let bad_domain = pack(&[b"unknown-v2", outer[1], outer[2], outer[3]]);
    assert_eq!(
        PhloFundingIntentVersioned::decode(&bad_domain, limits()),
        Err(PhloFundingIntentV2Error::FormatDomain)
    );
    let grant_list = fields(outer[2]);
    let first_grant = fields(grant_list[1]);
    let mut malformed = first_grant.clone();
    malformed[6] = &[1];
    let bad_first = pack(&malformed);
    let bad_grants = pack(&[grant_list[0], &bad_first, grant_list[2]]);
    let bad = pack(&[outer[0], outer[1], &bad_grants, outer[3]]);
    assert_eq!(
        PhloFundingIntentV2::decode(&bad, limits()),
        Err(PhloFundingIntentV2Error::FieldWidth)
    );
    let mut malformed = first_grant.clone();
    malformed[0] = &[0; 3];
    let bad_first = pack(&malformed);
    let bad_grants = pack(&[grant_list[0], &bad_first, grant_list[2]]);
    let bad = pack(&[outer[0], outer[1], &bad_grants, outer[3]]);
    assert_eq!(
        PhloFundingIntentV2::decode(&bad, limits()),
        Err(PhloFundingIntentV2Error::FieldWidth)
    );
    for cut in 0..bytes.len() {
        assert!(PhloFundingIntentV2::decode(&bytes[..cut], limits()).is_err());
    }
    let trailing = [bytes.as_slice(), b"extra"].concat();
    assert!(PhloFundingIntentV2::decode(&trailing, limits()).is_err());
}
