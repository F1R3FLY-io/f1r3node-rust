use models::rust::phlo_schedule::PhloResourceClassV1;
use models::rust::phlo_wire::PhloWireLimits;
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

fn descriptor() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"network",
        shard: b"shard",
        settlement_asset: b"asset",
        settlement_unit: b"minor unit",
        decimal_scale: 8,
        actual_price: 2,
        compatibility_rule: [1; 32],
        classes: vec![PhloResourceClassV1 {
            identity: b"compute",
            measurement_unit: b"COMM",
            measurement_rule: [2; 32],
            valuation_rule: [3; 32],
            weight: 3,
        }],
    }
}

fn terms<'a>(permitted: &'a [PhloSchedule<'a>]) -> SignedPhloControls<'a> {
    SignedPhloControls {
        limit: 10,
        price_ceiling: 2,
        required_owner_ceilings: &[2],
        permitted_schedules: permitted,
    }
}

#[test]
fn signer_consent_does_not_authorize_changed_resource_policy() {
    let mut record = descriptor();
    record.classes.push(PhloResourceClassV1 {
        identity: b"second class",
        ..record.classes[0]
    });
    let required = PhloScheduleBinding::new(&record, limits()).unwrap();
    for field in 0..15 {
        let mut changed = record.clone();
        match field {
            0 => changed.protocol_version += 1,
            1 => changed.network = b"other network",
            2 => changed.shard = b"other shard",
            3 => changed.settlement_asset = b"other asset",
            4 => changed.settlement_unit = b"other denomination",
            5 => changed.decimal_scale += 1,
            6 => changed.compatibility_rule[0] ^= 1,
            7 => changed.classes[0].identity = b"other class",
            8 => changed.classes[0].measurement_unit = b"other resource unit",
            9 => changed.classes[0].measurement_rule[0] ^= 1,
            10 => changed.classes[0].valuation_rule[0] ^= 1,
            11 => changed.classes[0].weight = 0,
            12 => {
                changed.classes.pop();
            }
            13 => changed.classes.reverse(),
            _ => changed.classes.push(PhloResourceClassV1 {
                identity: b"extra class",
                ..changed.classes[0]
            }),
        }
        let binding = PhloScheduleBinding::new(&changed, limits()).unwrap();
        let selected = binding.schedule();
        let consent = [selected];
        assert!(
            check_phlo_controls(selected.environment, 0, 21, terms(&consent), selected, 10).is_ok()
        );
        assert_eq!(
            binding.bind_policy(required.policy()),
            Err(PhloSchedulePolicyMismatch)
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn arbitrary_prices_preserve_policy_but_remain_separate_signed_offers(
        original in any::<u64>(), offered in any::<u64>(),
        weights in prop::collection::vec(any::<u64>(), 1..128),
    ) {
        let identities: Vec<_> = (0..weights.len()).map(|index| index.to_be_bytes()).collect();
        let mut policy_record = descriptor();
        let prototype = policy_record.classes[0];
        policy_record.classes = identities.iter().zip(&weights).map(|(identity, weight)|
            PhloResourceClassV1 { identity, weight: *weight, ..prototype }).collect();
        policy_record.actual_price = original;
        let required = PhloScheduleBinding::new(&policy_record, limits()).unwrap();
        let offered_record = PhloScheduleV1 { actual_price: offered, ..policy_record.clone() };
        let candidate = PhloScheduleBinding::new(&offered_record, limits()).unwrap();
        let selected = candidate.bind_policy(required.policy()).unwrap();
        prop_assert_eq!(selected.actual_price, offered);
        prop_assert_eq!(selected.weights, weights.as_slice());
        prop_assert_eq!(candidate.policy(), required.policy());
        if original != offered { prop_assert_ne!(selected.commitment, required.schedule().commitment); }
    }
}

#[test]
fn binding_preserves_the_complete_digest_and_numeric_projection() {
    let record = descriptor();
    let binding = PhloScheduleBinding::new(&record, limits()).unwrap();
    let selected = binding.schedule();
    assert!(std::ptr::eq(binding.descriptor(), &record));
    assert_eq!(selected.commitment, record.digest(limits()).unwrap());
    assert_eq!(selected.weights, &[3]);
    assert_eq!(selected.actual_price, 2);
    assert_eq!(selected.environment.decimal_scale, 8);
    let permitted = [selected];
    let checked =
        check_phlo_controls(selected.environment, 0, 21, terms(&permitted), selected, 10).unwrap();
    assert_eq!(checked.schedule(), selected);
    let encoded = record.encode(limits()).unwrap();
    let decoded = PhloScheduleV1::decode(&encoded, limits()).unwrap();
    let second = PhloScheduleBinding::new(&decoded, limits()).unwrap();
    assert_eq!(second.schedule(), selected);
    assert!(check_phlo_controls(
        selected.environment,
        0,
        21,
        terms(&permitted),
        second.schedule(),
        10
    )
    .is_ok());
}

#[test]
fn interpretation_changes_require_consent_even_when_all_numeric_fields_match() {
    let record = descriptor();
    let original = PhloScheduleBinding::new(&record, limits()).unwrap();
    let permitted = [original.schedule()];
    for field in 0..5 {
        let mut changed = record.clone();
        match field {
            0 => changed.compatibility_rule[0] ^= 1,
            1 => changed.classes[0].identity = b"other class",
            2 => changed.classes[0].measurement_unit = b"other unit",
            3 => changed.classes[0].measurement_rule[31] ^= 1,
            _ => changed.classes[0].valuation_rule[31] ^= 1,
        }
        let binding = PhloScheduleBinding::new(&changed, limits()).unwrap();
        let selected = binding.schedule();
        assert_eq!(selected.weights, permitted[0].weights);
        assert_eq!(selected.actual_price, permitted[0].actual_price);
        assert_eq!(selected.environment, permitted[0].environment);
        assert_ne!(selected.commitment, permitted[0].commitment);
        assert_eq!(
            check_phlo_controls(
                permitted[0].environment,
                0,
                21,
                terms(&permitted),
                selected,
                10
            ),
            Err(PhloControlsError::ScheduleNotPermitted)
        );
    }
}

#[test]
fn explicit_schedule_consent_cannot_change_the_native_unit_scale() {
    let native_record = descriptor();
    let native = PhloScheduleBinding::new(&native_record, limits()).unwrap();
    for scale in [0, 7, 9, u8::MAX] {
        let altered = PhloScheduleV1 {
            decimal_scale: scale,
            ..native_record.clone()
        };
        let binding = PhloScheduleBinding::new(&altered, limits()).unwrap();
        let permitted = [binding.schedule()];
        assert_eq!(
            check_phlo_controls(
                native.schedule().environment,
                0,
                21,
                terms(&permitted),
                permitted[0],
                10
            ),
            Err(PhloControlsError::DecimalScaleMismatch)
        );
    }
}

#[test]
fn binding_rejects_invalid_descriptors_before_creating_a_projection() {
    let mut record = descriptor();
    assert_eq!(
        PhloScheduleBinding::new(&record, PhloScheduleLimits {
            classes: 0,
            ..limits()
        }),
        Err(PhloScheduleError::ClassLimit)
    );
    record.classes.push(record.classes[0]);
    assert_eq!(
        PhloScheduleBinding::new(&record, limits()),
        Err(PhloScheduleError::DuplicateClass)
    );
    record.classes.clear();
    assert_eq!(
        PhloScheduleBinding::new(&record, limits()),
        Err(PhloScheduleError::EmptyClasses)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_rule_commitments_never_disappear_from_control_checks(
        measurement in any::<[u8; 32]>(), valuation in any::<[u8; 32]>(),
        compatibility in any::<[u8; 32]>(), bit in 0usize..256,
    ) {
        let mut record = descriptor();
        record.classes[0].measurement_rule = measurement;
        record.classes[0].valuation_rule = valuation;
        record.compatibility_rule = compatibility;
        let binding = PhloScheduleBinding::new(&record, limits()).unwrap();
        let permitted = [binding.schedule()];
        for field in 0..3 {
            let mut changed = record.clone();
            let bytes = match field { 0 => &mut changed.classes[0].measurement_rule, 1 => &mut changed.classes[0].valuation_rule, _ => &mut changed.compatibility_rule };
            bytes[bit / 8] ^= 1 << (bit % 8);
            let changed_binding = PhloScheduleBinding::new(&changed, limits()).unwrap();
            prop_assert_eq!(check_phlo_controls(permitted[0].environment, 0, 21, terms(&permitted), changed_binding.schedule(), 10), Err(PhloControlsError::ScheduleNotPermitted));
        }
    }
}
