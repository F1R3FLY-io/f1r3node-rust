use proptest::prelude::*;

use super::*;

fn limits() -> PhloScheduleLimits {
    PhloScheduleLimits {
        wire: PhloWireLimits {
            total_bytes: 65_536,
            field_bytes: 32_768,
        },
        classes: 256,
    }
}

#[test]
fn genesis_policy_rejects_schedule_bytes_prices_and_trailing_data() {
    let original = schedule();
    let policy = PhloGenesisPolicy::from_schedule(&original).unwrap();
    let bytes = policy.encode().unwrap();
    assert_eq!(PhloGenesisPolicy::decode(&bytes).unwrap(), policy);
    assert_eq!(policy.schedule().unwrap().actual_price, 0);
    assert_eq!(
        PhloGenesisPolicy::decode(&original.encode(limits()).unwrap()),
        Err(PhloScheduleError::FormatDomain)
    );
    let priced = pack(&[
        PHLO_GENESIS_POLICY_V1_DOMAIN.to_vec(),
        original.encode(limits()).unwrap(),
    ]);
    assert_eq!(
        PhloGenesisPolicy::decode(&priced),
        Err(PhloScheduleError::GenesisPolicyPrice)
    );
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(PhloGenesisPolicy::decode(&trailing).is_err());
    for prefix in 0..bytes.len() {
        assert!(PhloGenesisPolicy::decode(&bytes[..prefix]).is_err());
    }
    assert!(policy.validate_context(6, "shard", 8).is_ok());
    for (version, shard, scale) in [
        (7, "shard", 8),
        (6, "other", 8),
        (6, "shard", 9),
        (-1, "shard", 8),
    ] {
        assert_eq!(
            policy.validate_context(version, shard, scale),
            Err(PhloScheduleError::GenesisPolicyContext)
        );
    }
}

proptest! {
    #[test]
    fn genesis_policy_preserves_every_field_except_offer(
        first in any::<u64>(), second in any::<u64>(),
        weights in prop::collection::vec(any::<u64>(), 1..128),
        network in prop::collection::vec(any::<u8>(), 1..65),
        measurement in any::<[u8; 32]>(), valuation in any::<[u8; 32]>(),
        compatibility in any::<[u8; 32]>(),
    ) {
        let identities: Vec<_> = (0..weights.len()).map(|index| index.to_be_bytes()).collect();
        let mut original = schedule();
        original.network = &network;
        original.compatibility_rule = compatibility;
        original.classes = weights.iter().zip(&identities).map(|(weight, identity)| PhloResourceClassV1 {
            identity, measurement_unit: b"unit", measurement_rule: measurement,
            valuation_rule: valuation, weight: *weight,
        }).collect();
        original.actual_price = first;
        let policy = PhloGenesisPolicy::from_schedule(&original).unwrap();
        original.actual_price = second;
        prop_assert_eq!(&policy, &PhloGenesisPolicy::from_schedule(&original).unwrap());
        original.actual_price = 0;
        prop_assert_eq!(policy.schedule().unwrap(), original.clone());
        let bytes = policy.encode().unwrap();
        prop_assert_eq!(PhloGenesisPolicy::decode(&bytes).unwrap(), policy.clone());
        original.classes[0].weight ^= 1;
        prop_assert_ne!(PhloGenesisPolicy::from_schedule(&original).unwrap(), policy);
    }
}

fn schedule() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"network",
        shard: b"shard",
        settlement_asset: b"asset",
        settlement_unit: b"minor unit",
        decimal_scale: 8,
        classes: vec![
            PhloResourceClassV1 {
                identity: b"compute",
                measurement_unit: b"COMM",
                measurement_rule: [1; 32],
                valuation_rule: [2; 32],
                weight: 1,
            },
            PhloResourceClassV1 {
                identity: b"transfer",
                measurement_unit: b"byte",
                measurement_rule: [3; 32],
                valuation_rule: [4; 32],
                weight: 2,
            },
        ],
        actual_price: 3,
        compatibility_rule: [5; 32],
    }
}

fn pack(fields: &[Vec<u8>]) -> Vec<u8> {
    let mut output = Vec::new();
    for field in fields {
        let count = field.len() as u64;
        for shift in (0..8).rev() {
            output.push((count >> (8 * shift)) as u8);
        }
        output.extend_from_slice(field);
    }
    output
}

fn unpack(input: &[u8]) -> Vec<Vec<u8>> {
    let mut decoder = PhloWireDecoder::new(input, limits().wire).unwrap();
    let mut fields = Vec::new();
    while !decoder.remaining().is_empty() {
        fields.push(decoder.bytes().unwrap().to_vec());
    }
    fields
}

fn reference(schedule: &PhloScheduleV1<'_>) -> Vec<u8> {
    let mut classes = vec![(schedule.classes.len() as u32).to_be_bytes().to_vec()];
    for class in &schedule.classes {
        classes.push(pack(&[
            class.identity.to_vec(),
            class.measurement_unit.to_vec(),
            class.measurement_rule.to_vec(),
            class.valuation_rule.to_vec(),
            class.weight.to_be_bytes().to_vec(),
        ]));
    }
    pack(&[
        b"f1r3node:phlo-schedule:v1".to_vec(),
        schedule.protocol_version.to_be_bytes().to_vec(),
        schedule.network.to_vec(),
        schedule.shard.to_vec(),
        schedule.settlement_asset.to_vec(),
        schedule.settlement_unit.to_vec(),
        vec![schedule.decimal_scale],
        pack(&classes),
        schedule.actual_price.to_be_bytes().to_vec(),
        1u64.to_be_bytes().to_vec(),
        schedule.compatibility_rule.to_vec(),
    ])
}

#[test]
fn schedule_encoding_matches_the_complete_field_contract() {
    let original = schedule();
    let encoded = original.encode(limits()).unwrap();
    assert_eq!(encoded, reference(&original));
    let decoded = PhloScheduleV1::decode(&encoded, limits()).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(decoded.encode(limits()).unwrap(), encoded);
    assert_eq!(
        original.digest(limits()).unwrap().as_slice(),
        Blake2b256::hash(reference(&original))
    );
    assert_eq!(unpack(&encoded).len(), 11);
}

#[test]
fn every_schedule_component_changes_its_commitment() {
    let original = schedule();
    let mut mutations = Vec::new();
    for field in 0..9 {
        let mut changed = original.clone();
        match field {
            0 => changed.protocol_version += 1,
            1 => changed.network = b"other network",
            2 => changed.shard = b"other shard",
            3 => changed.settlement_asset = b"other asset",
            4 => changed.settlement_unit = b"other unit",
            5 => changed.decimal_scale += 1,
            6 => changed.actual_price += 1,
            7 => changed.compatibility_rule[31] ^= 1,
            _ => changed.classes.reverse(),
        }
        mutations.push(changed);
    }
    for index in 0..original.classes.len() {
        for field in 0..5 {
            let mut changed = original.clone();
            match field {
                0 => changed.classes[index].identity = b"other class",
                1 => changed.classes[index].measurement_unit = b"other measurement unit",
                2 => changed.classes[index].measurement_rule[31] ^= 1,
                3 => changed.classes[index].valuation_rule[31] ^= 1,
                _ => changed.classes[index].weight += 1,
            }
            mutations.push(changed);
        }
    }
    let mut shorter = original.clone();
    shorter.classes.pop();
    mutations.push(shorter);
    let original_bytes = original.encode(limits()).unwrap();
    let original_digest = original.digest(limits()).unwrap();
    for changed in mutations {
        let bytes = changed.encode(limits()).unwrap();
        assert_ne!(bytes, original_bytes);
        assert_ne!(changed.digest(limits()).unwrap(), original_digest);
        assert_eq!(PhloScheduleV1::decode(&bytes, limits()).unwrap(), changed);
    }
}

#[test]
fn every_truncated_outer_or_nested_record_is_rejected() {
    let encoded = schedule().encode(limits()).unwrap();
    for length in 0..encoded.len() {
        assert!(PhloScheduleV1::decode(&encoded[..length], limits()).is_err());
    }
    let fields = unpack(&encoded);
    let class_fields = unpack(&fields[7]);
    for length in 0..class_fields[1].len() {
        let mut shortened_classes = class_fields.clone();
        shortened_classes[1].truncate(length);
        let mut changed = fields.clone();
        changed[7] = pack(&shortened_classes);
        assert!(PhloScheduleV1::decode(&pack(&changed), limits()).is_err());
    }
    for nesting in 0..3 {
        let mut changed = fields.clone();
        match nesting {
            0 => changed.push(vec![]),
            1 => changed[7].push(0),
            _ => {
                let mut classes = class_fields.clone();
                classes[1].push(0);
                changed[7] = pack(&classes);
            }
        }
        assert_eq!(
            PhloScheduleV1::decode(&pack(&changed), limits()),
            Err(PhloWireError::TrailingBytes.into())
        );
    }
}

#[test]
fn unsupported_formats_fees_and_noncanonical_widths_are_rejected() {
    let fields = unpack(&schedule().encode(limits()).unwrap());
    let mut altered = fields.clone();
    altered[0] = b"f1r3node:phlo-schedule:v2".to_vec();
    assert_eq!(
        PhloScheduleV1::decode(&pack(&altered), limits()),
        Err(PhloScheduleError::FormatDomain)
    );
    for fee in [0u64, 2, u64::MAX] {
        altered = fields.clone();
        altered[9] = fee.to_be_bytes().to_vec();
        assert_eq!(
            PhloScheduleV1::decode(&pack(&altered), limits()),
            Err(PhloScheduleError::FeePolicy)
        );
    }
    for index in [1, 6, 8, 9, 10] {
        for increase in [false, true] {
            altered = fields.clone();
            if increase {
                altered[index].push(0);
            } else {
                altered[index].pop();
            }
            assert_eq!(
                PhloScheduleV1::decode(&pack(&altered), limits()),
                Err(PhloScheduleError::FieldWidth)
            );
        }
    }
    let table = unpack(&fields[7]);
    for index in [2, 3, 4] {
        let mut class = unpack(&table[1]);
        class[index].push(0);
        let mut classes = table.clone();
        classes[1] = pack(&class);
        altered = fields.clone();
        altered[7] = pack(&classes);
        assert_eq!(
            PhloScheduleV1::decode(&pack(&altered), limits()),
            Err(PhloScheduleError::FieldWidth)
        );
    }
    let mut classes = table;
    classes[0].push(0);
    altered = fields;
    altered[7] = pack(&classes);
    assert_eq!(
        PhloScheduleV1::decode(&pack(&altered), limits()),
        Err(PhloScheduleError::FieldWidth)
    );
}

#[test]
fn empty_identities_and_duplicate_classes_fail_on_encode_and_decode() {
    let original = schedule();
    for field in 0..6 {
        let mut changed = original.clone();
        match field {
            0 => changed.network = &[],
            1 => changed.shard = &[],
            2 => changed.settlement_asset = &[],
            3 => changed.settlement_unit = &[],
            4 => changed.classes[0].identity = &[],
            _ => changed.classes[0].measurement_unit = &[],
        }
        assert_eq!(
            changed.encode(limits()),
            Err(PhloScheduleError::EmptyIdentity)
        );
        assert_eq!(
            PhloScheduleV1::decode(&reference(&changed), limits()),
            Err(PhloScheduleError::EmptyIdentity)
        );
    }
    let mut duplicate = original.clone();
    let separate_identity = duplicate.classes[0].identity.to_vec();
    duplicate.classes[1].identity = &separate_identity;
    assert_eq!(
        duplicate.encode(limits()),
        Err(PhloScheduleError::DuplicateClass)
    );
    assert_eq!(
        PhloScheduleV1::decode(&reference(&duplicate), limits()),
        Err(PhloScheduleError::DuplicateClass)
    );
    let mut empty = original;
    empty.classes.clear();
    assert_eq!(empty.encode(limits()), Err(PhloScheduleError::EmptyClasses));
    assert_eq!(
        PhloScheduleV1::decode(&reference(&empty), limits()),
        Err(PhloScheduleError::EmptyClasses)
    );
}

#[test]
fn configured_limits_include_nested_records_and_accept_exact_boundaries() {
    let original = schedule();
    let encoded = original.encode(limits()).unwrap();
    let field_max = unpack(&encoded).iter().map(Vec::len).max().unwrap();
    let exact = PhloScheduleLimits {
        wire: PhloWireLimits {
            total_bytes: encoded.len(),
            field_bytes: field_max,
        },
        classes: 2,
    };
    assert_eq!(original.encode(exact).unwrap(), encoded);
    assert_eq!(PhloScheduleV1::decode(&encoded, exact).unwrap(), original);
    for shortened in [
        PhloScheduleLimits {
            wire: PhloWireLimits {
                total_bytes: encoded.len() - 1,
                ..exact.wire
            },
            ..exact
        },
        PhloScheduleLimits {
            wire: PhloWireLimits {
                field_bytes: field_max - 1,
                ..exact.wire
            },
            ..exact
        },
        PhloScheduleLimits {
            classes: 1,
            ..exact
        },
    ] {
        assert!(original.encode(shortened).is_err());
        assert!(PhloScheduleV1::decode(&encoded, shortened).is_err());
    }
}

#[test]
fn malicious_class_counts_do_not_trigger_count_sized_allocations() {
    let mut fields = unpack(&schedule().encode(limits()).unwrap());
    let mut classes = unpack(&fields[7]);
    classes[0] = u32::MAX.to_be_bytes().to_vec();
    fields[7] = pack(&classes);
    let encoded = pack(&fields);
    assert_eq!(
        PhloScheduleV1::decode(&encoded, limits()),
        Err(PhloScheduleError::ClassLimit)
    );
    let permissive_count = PhloScheduleLimits {
        classes: usize::MAX,
        ..limits()
    };
    assert_eq!(
        PhloScheduleV1::decode(&encoded, permissive_count),
        Err(PhloWireError::Truncated.into())
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_schedules_preserve_full_width_values_and_every_class(
        protocol in any::<u64>(), price in any::<u64>(), scale in any::<u8>(),
        network in prop::collection::vec(any::<u8>(), 1..16),
        rules in prop::collection::vec((any::<u64>(), any::<[u8; 32]>(), any::<[u8; 32]>()), 1..33),
        compatibility in any::<[u8; 32]>(),
    ) {
        let identities: Vec<_> = (0..rules.len()).map(|index| (index as u32).to_be_bytes()).collect();
        let original = PhloScheduleV1 {
            protocol_version: protocol, network: &network, decimal_scale: scale, actual_price: price,
            compatibility_rule: compatibility,
            classes: rules.iter().zip(&identities).map(|((weight, measurement, valuation), identity)| PhloResourceClassV1 {
                identity, measurement_unit: b"raw unit", measurement_rule: *measurement, valuation_rule: *valuation, weight: *weight,
            }).collect(),
            ..schedule()
        };
        let encoded = original.encode(limits()).unwrap();
        prop_assert_eq!(&encoded, &reference(&original));
        let decoded = PhloScheduleV1::decode(&encoded, limits()).unwrap();
        prop_assert_eq!(&decoded, &original);
        prop_assert_eq!(decoded.encode(limits()).unwrap(), encoded.clone());
        prop_assert_eq!(decoded.digest(limits()).unwrap(), original.digest(limits()).unwrap());
    }

    #[test]
    fn accepted_mutations_have_exact_canonical_reencoding(index in any::<usize>(), bit in 0u8..8) {
        let mut encoded = schedule().encode(limits()).unwrap();
        let position = index % encoded.len(); encoded[position] ^= 1u8 << bit;
        if let Ok(decoded) = PhloScheduleV1::decode(&encoded, limits()) {
            prop_assert_eq!(&decoded.encode(limits()).unwrap(), &encoded);
            prop_assert_ne!(decoded, schedule());
        }
    }
}
