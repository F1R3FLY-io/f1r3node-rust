use proptest::prelude::*;

use super::*;
use crate::rust::phlo_schedule::PhloResourceClassV1;

fn limits() -> PhloControlsLimits {
    PhloControlsLimits {
        wire: PhloWireLimits {
            total_bytes: 1_048_576,
            field_bytes: 524_288,
        },
        owners: 4096,
        schedules: 64,
        total_classes: 256,
    }
}

fn schedule(price: u64) -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
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
            weight: 3,
        }],
        actual_price: price,
        compatibility_rule: [4; 32],
    }
}

fn controls() -> PhloControlsV1<'static> {
    PhloControlsV1 {
        limit: 100,
        price_ceiling: 7,
        required_owner_ceilings: vec![7, 9, 7],
        permitted_schedules: vec![schedule(2), schedule(3)],
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

fn owners(values: &[u64]) -> Vec<u8> {
    let mut output = pack(&[&(values.len() as u32).to_be_bytes()]);
    for value in values {
        output.extend(pack(&[&value.to_be_bytes()]));
    }
    output
}

fn schedules(values: &[PhloScheduleV1<'_>]) -> Vec<u8> {
    let mut output = pack(&[&(values.len() as u32).to_be_bytes()]);
    for value in values {
        output.extend(pack(&[&value.encode(limits().schedule(256)).unwrap()]));
    }
    output
}

fn raw(domain: &[u8], limit: &[u8], price: &[u8], owners: &[u8], schedules: &[u8]) -> Vec<u8> {
    pack(&[domain, limit, price, owners, schedules])
}

fn oracle(record: &PhloControlsV1<'_>) -> Vec<u8> {
    raw(
        PHLO_CONTROLS_V1_DOMAIN,
        &record.limit.to_be_bytes(),
        &record.price_ceiling.to_be_bytes(),
        &owners(&record.required_owner_ceilings),
        &schedules(&record.permitted_schedules),
    )
}

#[test]
fn phlo_controls_wire_preserves_order_multiplicity_and_full_width_values() {
    for count in [0, 1, 2, 3, 64, 129, 1024, 4096] {
        let record = PhloControlsV1 {
            limit: u64::MAX,
            price_ceiling: 0,
            required_owner_ceilings: (0..count)
                .map(|index| if index % 2 == 0 { 0 } else { u64::MAX })
                .collect(),
            permitted_schedules: vec![schedule(u64::MAX), schedule(0), schedule(u64::MAX)],
        };
        let wire = record.encode(limits()).unwrap();
        assert_eq!(wire, oracle(&record));
        assert_eq!(PhloControlsV1::decode(&wire, limits()).unwrap(), record);
    }
    let record = PhloControlsV1 {
        limit: 0,
        price_ceiling: 0,
        required_owner_ceilings: vec![],
        permitted_schedules: vec![],
    };
    let wire = record.encode(limits()).unwrap();
    assert_eq!(PhloControlsV1::decode(&wire, limits()).unwrap(), record);
}

#[test]
fn phlo_controls_wire_binds_every_term_and_owner_not_only_the_minimum() {
    let original = controls();
    let wire = original.encode(limits()).unwrap();
    let mut mutations = Vec::new();
    let mut record = original.clone();
    record.limit += 1;
    mutations.push(record);
    let mut record = original.clone();
    record.price_ceiling += 1;
    mutations.push(record);
    let mut record = original.clone();
    record.required_owner_ceilings[1] += 1;
    mutations.push(record);
    let mut record = original.clone();
    record.required_owner_ceilings.swap(0, 1);
    mutations.push(record);
    let mut record = original.clone();
    record.required_owner_ceilings.pop();
    mutations.push(record);
    let mut record = original.clone();
    record.permitted_schedules.swap(0, 1);
    mutations.push(record);
    let mut record = original.clone();
    record.permitted_schedules.push(schedule(2));
    mutations.push(record);
    let mut record = original.clone();
    record.permitted_schedules[0].classes[0].valuation_rule[0] ^= 1;
    mutations.push(record);
    let mut record = original.clone();
    record.permitted_schedules[0].compatibility_rule[0] ^= 1;
    mutations.push(record);
    let mut record = original.clone();
    record.permitted_schedules[0].actual_price += 1;
    mutations.push(record);
    for record in mutations {
        let changed = record.encode(limits()).unwrap();
        assert_ne!(changed, wire);
        assert_eq!(PhloControlsV1::decode(&changed, limits()).unwrap(), record);
    }
}

#[test]
fn phlo_controls_wire_rejects_wrong_domains_widths_counts_and_trailing_data() {
    let l = 100u64.to_be_bytes();
    let p = 7u64.to_be_bytes();
    let owner_bytes = owners(&[7, 9]);
    let schedule_bytes = schedules(&[schedule(2)]);
    let good = raw(
        PHLO_CONTROLS_V1_DOMAIN,
        &l,
        &p,
        &owner_bytes,
        &schedule_bytes,
    );
    for input in [
        raw(b"other domain", &l, &p, &owner_bytes, &schedule_bytes),
        raw(
            PHLO_CONTROLS_V1_DOMAIN,
            &l[..7],
            &p,
            &owner_bytes,
            &schedule_bytes,
        ),
        raw(
            PHLO_CONTROLS_V1_DOMAIN,
            &l,
            &[0; 9],
            &owner_bytes,
            &schedule_bytes,
        ),
        raw(
            PHLO_CONTROLS_V1_DOMAIN,
            &l,
            &p,
            &pack(&[&1u64.to_be_bytes(), &p]),
            &schedule_bytes,
        ),
        raw(
            PHLO_CONTROLS_V1_DOMAIN,
            &l,
            &p,
            &pack(&[&1u32.to_be_bytes(), &[0; 7]]),
            &schedule_bytes,
        ),
        raw(
            PHLO_CONTROLS_V1_DOMAIN,
            &l,
            &p,
            &pack(&[&0u32.to_be_bytes(), &p]),
            &schedule_bytes,
        ),
        raw(
            PHLO_CONTROLS_V1_DOMAIN,
            &l,
            &p,
            &pack(&[&u32::MAX.to_be_bytes()]),
            &schedule_bytes,
        ),
        raw(
            PHLO_CONTROLS_V1_DOMAIN,
            &l,
            &p,
            &owner_bytes,
            &pack(&[&u32::MAX.to_be_bytes()]),
        ),
        raw(
            PHLO_CONTROLS_V1_DOMAIN,
            &l,
            &p,
            &owner_bytes,
            &pack(&[&0u32.to_be_bytes(), b"extra"]),
        ),
        [good.as_slice(), b"extra"].concat(),
    ] {
        assert!(PhloControlsV1::decode(&input, limits()).is_err());
    }
    let unlimited_counts = PhloControlsLimits {
        owners: usize::MAX,
        schedules: usize::MAX,
        ..limits()
    };
    for input in [
        raw(
            PHLO_CONTROLS_V1_DOMAIN,
            &l,
            &p,
            &pack(&[&u32::MAX.to_be_bytes()]),
            &schedule_bytes,
        ),
        raw(
            PHLO_CONTROLS_V1_DOMAIN,
            &l,
            &p,
            &owner_bytes,
            &pack(&[&u32::MAX.to_be_bytes()]),
        ),
    ] {
        assert!(PhloControlsV1::decode(&input, unlimited_counts).is_err());
    }
    for cut in 0..good.len() {
        assert!(PhloControlsV1::decode(&good[..cut], limits()).is_err());
    }
    for cut in 0..schedule_bytes.len() {
        let input = raw(
            PHLO_CONTROLS_V1_DOMAIN,
            &l,
            &p,
            &owner_bytes,
            &schedule_bytes[..cut],
        );
        assert!(PhloControlsV1::decode(&input, limits()).is_err());
    }
}

#[test]
fn phlo_controls_wire_applies_aggregate_class_and_exact_byte_limits() {
    let record = controls();
    let wire = record.encode(limits()).unwrap();
    let largest = schedules(&record.permitted_schedules).len();
    let exact = PhloControlsLimits {
        wire: PhloWireLimits {
            total_bytes: wire.len(),
            field_bytes: largest,
        },
        owners: 3,
        schedules: 2,
        total_classes: 2,
    };
    assert_eq!(record.encode(exact).unwrap(), wire);
    assert_eq!(PhloControlsV1::decode(&wire, exact).unwrap(), record);
    for smaller in [
        PhloControlsLimits { owners: 2, ..exact },
        PhloControlsLimits {
            schedules: 1,
            ..exact
        },
        PhloControlsLimits {
            total_classes: 1,
            ..exact
        },
        PhloControlsLimits {
            wire: PhloWireLimits {
                total_bytes: wire.len() - 1,
                ..exact.wire
            },
            ..exact
        },
        PhloControlsLimits {
            wire: PhloWireLimits {
                field_bytes: largest - 1,
                ..exact.wire
            },
            ..exact
        },
    ] {
        assert!(record.encode(smaller).is_err());
        assert!(PhloControlsV1::decode(&wire, smaller).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn phlo_controls_wire_generated_roundtrip_and_term_mutations(
        limit in any::<u64>(), ceiling in any::<u64>(),
        owner_ceilings in prop::collection::vec(any::<u64>(), 0..130),
        prices in prop::collection::vec(any::<u64>(), 0..9),
        rule in any::<[u8; 32]>(),
        mutation in any::<usize>(), mask in any::<u8>(),
    ) {
        let record = PhloControlsV1 { limit, price_ceiling: ceiling, required_owner_ceilings: owner_ceilings,
            permitted_schedules: prices.iter().map(|price| { let mut value = schedule(*price); value.compatibility_rule = rule; value }).collect() };
        let wire = record.encode(limits()).unwrap();
        prop_assert_eq!(&wire, &oracle(&record));
        let decoded = PhloControlsV1::decode(&wire, limits()).unwrap();
        prop_assert_eq!(&decoded, &record);
        prop_assert_eq!(decoded.encode(limits()).unwrap(), wire.clone());
        let mut changed = record.clone(); changed.limit ^= 1;
        prop_assert_ne!(changed.encode(limits()).unwrap(), wire.clone());
        let mut changed = record.clone(); changed.price_ceiling ^= 1;
        prop_assert_ne!(changed.encode(limits()).unwrap(), wire.clone());
        for index in 0..record.required_owner_ceilings.len() {
            let mut changed = record.clone(); changed.required_owner_ceilings[index] ^= 1;
            prop_assert_ne!(changed.encode(limits()).unwrap(), wire.clone());
        }
        for index in 0..record.permitted_schedules.len() {
            let mut changed = record.clone(); changed.permitted_schedules[index].compatibility_rule[0] ^= 1;
            prop_assert_ne!(changed.encode(limits()).unwrap(), wire.clone());
        }
        let mut mutated_wire = wire.clone();
        mutated_wire[mutation % wire.len()] ^= mask;
        if let Ok(decoded) = PhloControlsV1::decode(&mutated_wire, limits()) {
            prop_assert_eq!(decoded.encode(limits()).unwrap(), mutated_wire);
        }
    }

    #[test]
    fn phlo_controls_wire_any_accepted_bytes_reencode_identically(input in prop::collection::vec(any::<u8>(), 0..4096)) {
        if let Ok(decoded) = PhloControlsV1::decode(&input, limits()) {
            prop_assert_eq!(decoded.encode(limits()).unwrap(), input);
        }
    }
}
