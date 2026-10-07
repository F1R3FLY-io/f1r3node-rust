use proptest::prelude::*;

use super::*;
use crate::rust::phlo_resource::{PhloAuthorityNode, PhloResourceKeyV1};
use crate::rust::phlo_schedule::{PhloResourceClassV1, PhloScheduleV1};

fn limits() -> PhloFundingIntentLimits {
    let wire = PhloWireLimits {
        total_bytes: 4_194_304,
        field_bytes: 2_097_152,
    };
    PhloFundingIntentLimits {
        wire,
        controls: PhloControlsLimits {
            wire,
            owners: 4096,
            schedules: 16,
            total_classes: 64,
        },
        sources: 4096,
        resource_permissions: 4096,
        authority_nodes: 16_384,
    }
}

fn controls() -> PhloControlsV1<'static> {
    PhloControlsV1 {
        limit: 10,
        price_ceiling: 3,
        required_owner_ceilings: vec![3, 5, 8],
        permitted_schedules: vec![PhloScheduleV1 {
            protocol_version: 6,
            network: b"network",
            shard: b"root",
            settlement_asset: b"REV",
            settlement_unit: b"smallest REV unit",
            decimal_scale: 8,
            classes: vec![PhloResourceClassV1 {
                identity: b"COMM",
                measurement_unit: b"authority demand",
                measurement_rule: [1; 32],
                valuation_rule: [2; 32],
                weight: 1,
            }],
            actual_price: 2,
            compatibility_rule: [3; 32],
        }],
    }
}

fn policy(custody: &[u8], cap: u64) -> PhloSourcePolicyV1<'_> {
    PhloSourcePolicyV1::new(
        custody,
        cap,
        cap,
        true,
        vec![PhloResourceKeyV1 {
            location: b"slot",
            class: 0,
            acquisition_terms: b"terms",
            authority: vec![
                PhloAuthorityNode::And,
                PhloAuthorityNode::Ground(b"A"),
                PhloAuthorityNode::Ground(b"B"),
            ],
        }],
        limits().source(),
    )
    .unwrap()
}

fn intent() -> PhloFundingIntentV1<'static> {
    let controls = controls();
    let schedule_commitment = controls.permitted_schedules[0]
        .digest(limits().controls().schedule(1))
        .unwrap();
    PhloFundingIntentV1 {
        controls,
        schedule_commitment,
        total_exposure: 30,
        sources: vec![policy(b"A", 10), policy(b"B", 10), policy(b"C", 10)],
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

fn source_bytes(sources: &[PhloSourcePolicyV1<'_>]) -> Vec<u8> {
    let mut output = pack(&[&(sources.len() as u32).to_be_bytes()]);
    for source in sources {
        output.extend(pack(&[&source.encode(limits().source()).unwrap()]));
    }
    output
}

fn raw(
    domain: &[u8],
    controls: &[u8],
    schedule: &[u8],
    exposure: &[u8],
    sources: &[u8],
) -> Vec<u8> {
    pack(&[domain, controls, schedule, exposure, sources])
}

fn oracle(intent: &PhloFundingIntentV1<'_>) -> Vec<u8> {
    raw(
        PHLO_FUNDING_INTENT_V1_DOMAIN,
        &intent.controls.encode(limits().controls()).unwrap(),
        &intent.schedule_commitment,
        &intent.total_exposure.to_be_bytes(),
        &source_bytes(&intent.sources),
    )
}

#[test]
fn phlo_intent_binds_complete_controls_exposure_schedule_and_source_order() {
    let original = intent();
    let bytes = original.encode(limits()).unwrap();
    assert_eq!(bytes, oracle(&original));
    assert_eq!(
        PhloFundingIntentV1::decode(&bytes, limits()).unwrap(),
        original
    );
    let mut mutations = Vec::new();
    let mut changed = original.clone();
    changed.controls.limit += 1;
    mutations.push(changed);
    let mut changed = original.clone();
    changed.controls.price_ceiling += 1;
    mutations.push(changed);
    let mut changed = original.clone();
    changed.controls.required_owner_ceilings[1] += 1;
    mutations.push(changed);
    let mut changed = original.clone();
    changed.controls.permitted_schedules[0].classes[0].weight += 1;
    mutations.push(changed);
    let mut changed = original.clone();
    changed.schedule_commitment[31] ^= 1;
    mutations.push(changed);
    let mut changed = original.clone();
    changed.total_exposure += 1;
    mutations.push(changed);
    let mut changed = original.clone();
    changed.sources.swap(0, 1);
    mutations.push(changed);
    let mut changed = original.clone();
    changed.sources[1] = policy(b"B", 9);
    mutations.push(changed);
    let mut changed = original.clone();
    changed.sources[1] = policy(b"D", 10);
    mutations.push(changed);
    let mut changed = original.clone();
    changed.sources.pop();
    mutations.push(changed);
    for changed in mutations {
        let wire = changed.encode(limits()).unwrap();
        assert_ne!(wire, bytes);
        assert_eq!(
            PhloFundingIntentV1::decode(&wire, limits()).unwrap(),
            changed
        );
    }
}

#[test]
fn phlo_intent_rejects_duplicate_custody_without_coalescing_consents() {
    let mut record = intent();
    for duplicate in [policy(b"A", 10), policy(b"A", 20)] {
        record.sources[1] = duplicate;
        assert_eq!(
            record.encode(limits()),
            Err(PhloFundingIntentError::DuplicateCustody)
        );
        let bytes = oracle(&record);
        assert_eq!(
            PhloFundingIntentV1::decode(&bytes, limits()),
            Err(PhloFundingIntentError::DuplicateCustody)
        );
    }
}

#[test]
fn phlo_intent_enforces_cumulative_limits_across_sources_and_nested_controls() {
    let record = intent();
    let bytes = record.encode(limits()).unwrap();
    let largest = source_bytes(&record.sources)
        .len()
        .max(record.controls.encode(limits().controls()).unwrap().len());
    let exact = PhloFundingIntentLimits {
        wire: PhloWireLimits {
            total_bytes: bytes.len(),
            field_bytes: largest,
        },
        sources: 3,
        resource_permissions: 3,
        authority_nodes: 9,
        controls: PhloControlsLimits {
            owners: 3,
            schedules: 1,
            total_classes: 1,
            ..limits().controls
        },
    };
    assert_eq!(record.encode(exact).unwrap(), bytes);
    assert_eq!(PhloFundingIntentV1::decode(&bytes, exact).unwrap(), record);
    for bounded in [
        PhloFundingIntentLimits {
            sources: 2,
            ..exact
        },
        PhloFundingIntentLimits {
            resource_permissions: 2,
            ..exact
        },
        PhloFundingIntentLimits {
            authority_nodes: 8,
            ..exact
        },
        PhloFundingIntentLimits {
            controls: PhloControlsLimits {
                owners: 2,
                ..exact.controls
            },
            ..exact
        },
        PhloFundingIntentLimits {
            controls: PhloControlsLimits {
                total_classes: 0,
                ..exact.controls
            },
            ..exact
        },
        PhloFundingIntentLimits {
            wire: PhloWireLimits {
                total_bytes: bytes.len() - 1,
                ..exact.wire
            },
            ..exact
        },
        PhloFundingIntentLimits {
            wire: PhloWireLimits {
                field_bytes: largest - 1,
                ..exact.wire
            },
            ..exact
        },
    ] {
        assert!(record.encode(bounded).is_err());
        assert!(PhloFundingIntentV1::decode(&bytes, bounded).is_err());
    }
}

#[test]
fn phlo_intent_rejects_wrong_widths_domains_forged_counts_and_truncations() {
    let record = intent();
    let controls = record.controls.encode(limits().controls()).unwrap();
    let sources = source_bytes(&record.sources);
    let exposure = record.total_exposure.to_be_bytes();
    let bytes = record.encode(limits()).unwrap();
    for bad in [
        raw(
            b"wrong domain",
            &controls,
            &record.schedule_commitment,
            &exposure,
            &sources,
        ),
        raw(
            PHLO_FUNDING_INTENT_V1_DOMAIN,
            &controls,
            &[0; 31],
            &exposure,
            &sources,
        ),
        raw(
            PHLO_FUNDING_INTENT_V1_DOMAIN,
            &controls,
            &record.schedule_commitment,
            &[0; 8],
            &sources,
        ),
        raw(
            PHLO_FUNDING_INTENT_V1_DOMAIN,
            &controls,
            &record.schedule_commitment,
            &[0; 17],
            &sources,
        ),
        raw(
            PHLO_FUNDING_INTENT_V1_DOMAIN,
            &controls,
            &record.schedule_commitment,
            &exposure,
            &pack(&[&u32::MAX.to_be_bytes()]),
        ),
        raw(
            PHLO_FUNDING_INTENT_V1_DOMAIN,
            &controls,
            &record.schedule_commitment,
            &exposure,
            &pack(&[&0u32.to_be_bytes(), b"extra"]),
        ),
        raw(
            PHLO_FUNDING_INTENT_V1_DOMAIN,
            &controls,
            &record.schedule_commitment,
            &exposure,
            &pack(&[&0u64.to_be_bytes()]),
        ),
        [bytes.as_slice(), b"extra"].concat(),
    ] {
        assert!(PhloFundingIntentV1::decode(&bad, limits()).is_err());
    }
    let forged = raw(
        PHLO_FUNDING_INTENT_V1_DOMAIN,
        &controls,
        &record.schedule_commitment,
        &exposure,
        &pack(&[&u32::MAX.to_be_bytes()]),
    );
    assert!(
        PhloFundingIntentV1::decode(&forged, PhloFundingIntentLimits {
            sources: usize::MAX,
            ..limits()
        })
        .is_err()
    );
    for cut in 0..bytes.len() {
        assert!(PhloFundingIntentV1::decode(&bytes[..cut], limits()).is_err());
    }
    for cut in 0..sources.len() {
        let bad = raw(
            PHLO_FUNDING_INTENT_V1_DOMAIN,
            &controls,
            &record.schedule_commitment,
            &exposure,
            &sources[..cut],
        );
        assert!(PhloFundingIntentV1::decode(&bad, limits()).is_err());
    }
}

#[test]
fn phlo_intent_preserves_large_cohorts_zero_caps_and_full_width_exposure() {
    for count in [0, 1, 2, 3, 64, 129, 1024, 4096] {
        let ids: Vec<_> = (0..count)
            .map(|index| (index as u64).to_be_bytes())
            .collect();
        let mut record = intent();
        record.sources = ids.iter().map(|id| policy(id, 0)).collect();
        record.total_exposure = u128::MAX;
        let bytes = record.encode(limits()).unwrap();
        assert_eq!(
            PhloFundingIntentV1::decode(&bytes, limits()).unwrap(),
            record
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn phlo_intent_generated_roundtrip_mutation_and_accepted_bytes_are_exact(
        caps in prop::collection::vec(any::<u64>(), 0..130), exposure in any::<u128>(),
        schedule in any::<[u8; 32]>(), limit in any::<u64>(), ceiling in any::<u64>(),
        position in any::<usize>(), mask in any::<u8>(),
    ) {
        let ids: Vec<_> = (0..caps.len()).map(|index| (index as u64).to_be_bytes()).collect();
        let mut record = intent();
        record.total_exposure = exposure; record.schedule_commitment = schedule;
        record.controls.limit = limit; record.controls.price_ceiling = ceiling;
        record.sources = ids.iter().zip(&caps).map(|(id, cap)| policy(id, *cap)).collect();
        let bytes = record.encode(limits()).unwrap();
        prop_assert_eq!(&bytes, &oracle(&record));
        let decoded = PhloFundingIntentV1::decode(&bytes, limits()).unwrap();
        prop_assert_eq!(&decoded, &record);
        prop_assert_eq!(decoded.encode(limits()).unwrap(), bytes.clone());
        let mut changed = record.clone(); changed.total_exposure ^= 1;
        prop_assert_ne!(changed.encode(limits()).unwrap(), bytes.clone());
        for index in 0..record.sources.len() {
            let mut changed = record.clone(); changed.sources[index] = policy(&ids[index], caps[index] ^ 1);
            prop_assert_ne!(changed.encode(limits()).unwrap(), bytes.clone());
        }
        let mut mutated = bytes.clone(); mutated[position % bytes.len()] ^= mask;
        if let Ok(decoded) = PhloFundingIntentV1::decode(&mutated, limits()) {
            prop_assert_eq!(decoded.encode(limits()).unwrap(), mutated);
        }
    }
}
