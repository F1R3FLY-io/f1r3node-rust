use models::rust::phlo_schedule::PhloScheduleV1;
use proptest::prelude::*;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloEnvironment, PhloSchedule, SignedPhloControls,
};

use super::*;

mod prepaid_cells;
mod measured_acquisition;

fn policy(minimum: u64, version: u64, shard: &str) -> GenesisResourcePolicy {
    let schedule = PhloScheduleV1 {
        protocol_version: version,
        network: b"test",
        shard: shard.as_bytes(),
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: vec![NativePhloDimension::Compute.resource_class(b"compute", 1)],
        actual_price: 1,
        compatibility_rule: native_resource_compatibility_rule(),
    };
    GenesisResourcePolicy {
        genesis_root: vec![9; 32].into(),
        record: PhloGenesisPolicy::from_schedule(&schedule).unwrap(),
        minimum_price: minimum,
    }
}

pub(in crate::rust::util::rholang::costacc) fn retained_record_context() -> AdoptedResourcePolicy {
    policy(0, 6, "root")
        .adopt(&CasperShardConf {
            min_phlo_price: 0,
            casper_version: 6,
            shard_name: "root".into(),
            ..CasperShardConf::new()
        })
        .unwrap()
}

fn check_capture(context: &AdoptedResourcePolicy, minimum: u64, version: u64, shard: &str) -> bool {
    let schedule = PhloSchedule {
        commitment: [1; 32],
        environment: PhloEnvironment {
            protocol_version: version,
            network: b"test",
            shard: shard.as_bytes(),
            asset: b"REV",
            unit: b"atomic-REV",
            decimal_scale: 8,
        },
        weights: &[1],
        actual_price: u64::MAX,
    };
    let schedules = [schedule];
    let controls = check_phlo_controls(
        schedule.environment,
        minimum,
        u64::MAX,
        SignedPhloControls {
            limit: 0,
            price_ceiling: u64::MAX,
            required_owner_ceilings: &[u64::MAX],
            permitted_schedules: &schedules,
        },
        schedule,
        0,
    )
    .unwrap();
    context.check_controls(controls).is_ok()
}

#[test]
fn retained_context_does_not_follow_later_configuration_changes() {
    let mut adopted = CasperShardConf {
        min_phlo_price: 10,
        casper_version: 6,
        shard_name: "root".into(),
        ..CasperShardConf::new()
    };
    let context = policy(10, 6, "root").adopt(&adopted).unwrap();
    adopted.min_phlo_price = 0;
    adopted.casper_version = 7;
    adopted.shard_name = "other".into();
    assert!(check_capture(&context, 10, 6, "root"));
    assert!(!check_capture(&context, 0, 6, "root"));
    assert!(!check_capture(&context, 10, 7, "root"));
    assert!(!check_capture(&context, 10, 6, "other"));
    assert!(context.genesis().clone().adopt(&adopted).is_err());
    assert_eq!(context.genesis().genesis_root().as_ref(), &[9; 32]);
}

#[test]
fn failed_adoption_does_not_authorize_a_context() {
    for (minimum, version, shard) in [
        (-1, 6, "root"),
        (11, 6, "root"),
        (10, -1, "root"),
        (10, 7, "root"),
        (10, 6, ""),
        (10, 6, "other"),
    ] {
        let adopted = CasperShardConf {
            min_phlo_price: minimum,
            casper_version: version,
            shard_name: shard.into(),
            ..CasperShardConf::new()
        };
        assert!(policy(10, 6, "root").adopt(&adopted).is_err());
    }
}

#[test]
fn unsupported_native_policy_rules_cannot_enter_adopted_context() {
    let adopted = CasperShardConf {
        min_phlo_price: 10,
        casper_version: 6,
        shard_name: "root".into(),
        ..CasperShardConf::new()
    };
    for field in 0..4 {
        let mut candidate = policy(10, 6, "root");
        let mut descriptor = candidate.record.schedule().unwrap();
        match field {
            0 => descriptor.classes[0].measurement_rule = [0; 32],
            1 => descriptor.classes[0].valuation_rule = [0; 32],
            2 => descriptor.classes[0].measurement_unit = b"unimplemented-unit",
            _ => descriptor.compatibility_rule = [0; 32],
        }
        candidate.record = PhloGenesisPolicy::from_schedule(&descriptor).unwrap();
        assert!(
            candidate.adopt(&adopted).is_err(),
            "unsupported rule field {field}"
        );
    }
}

fn acquisition_context() -> AdoptedResourcePolicy {
    let mut genesis = policy(10, 6, "root");
    let mut descriptor = genesis.record.schedule().unwrap();
    descriptor
        .classes
        .push(NativePhloDimension::Transfer.resource_class(b"transfer", 7));
    genesis.record = PhloGenesisPolicy::from_schedule(&descriptor).unwrap();
    genesis
        .adopt(&CasperShardConf {
            min_phlo_price: 10,
            casper_version: 6,
            shard_name: "root".into(),
            ..CasperShardConf::new()
        })
        .unwrap()
}

#[test]
fn acquisition_terms_preserve_original_bytes_price_and_adopted_context() {
    let adopted = acquisition_context();
    for price in [0, 1, 9, 10, 11, u64::MAX] {
        let mut original = adopted.genesis().record().schedule().unwrap();
        original.actual_price = price;
        let bytes = original.encode(PhloGenesisPolicy::LIMITS).unwrap();
        let checked = adopted.check_acquisition_terms(&bytes).unwrap();
        assert_eq!(checked.bytes(), bytes);
        assert_eq!(checked.schedule(), &original);
        assert_eq!(checked.schedule().actual_price, price);
        assert!(std::ptr::eq(checked.adopted(), &adopted));
        assert_eq!(checked.adopted().genesis().minimum_price(), 10);
    }
}

#[test]
fn acquisition_terms_reject_every_changed_policy_component() {
    let adopted = acquisition_context();
    for field in 0..15 {
        let mut changed = adopted.genesis().record().schedule().unwrap();
        changed.actual_price = 25;
        match field {
            0 => changed.protocol_version += 1,
            1 => changed.network = b"another-network",
            2 => changed.shard = b"another-shard",
            3 => changed.settlement_asset = b"another-asset",
            4 => changed.settlement_unit = b"another-unit",
            5 => changed.decimal_scale += 1,
            6 => changed.classes[0].identity = b"another-class",
            7 => changed.classes[0].measurement_unit = b"another-measure",
            8 => changed.classes[0].measurement_rule[0] ^= 1,
            9 => changed.classes[0].valuation_rule[0] ^= 1,
            10 => changed.classes[0].weight += 1,
            11 => changed.classes.swap(0, 1),
            12 => {
                changed.classes.pop();
            }
            13 => changed
                .classes
                .push(NativePhloDimension::Introduction.resource_class(b"introduction", 1)),
            _ => changed.compatibility_rule[0] ^= 1,
        }
        let bytes = changed.encode(PhloGenesisPolicy::LIMITS).unwrap();
        assert!(
            matches!(
                adopted.check_acquisition_terms(&bytes),
                Err(CasperError::InvalidCostSettlement(_))
            ),
            "changed policy field {field}"
        );
    }
}

#[test]
fn acquisition_terms_reject_malformed_and_oversized_records() {
    let adopted = acquisition_context();
    let mut original = adopted.genesis().record().schedule().unwrap();
    original.actual_price = 27;
    let bytes = original.encode(PhloGenesisPolicy::LIMITS).unwrap();
    for length in 0..bytes.len() {
        assert!(adopted.check_acquisition_terms(&bytes[..length]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(adopted.check_acquisition_terms(&trailing).is_err());
    let mut wrong_domain = bytes;
    wrong_domain[8] ^= 1;
    assert!(adopted.check_acquisition_terms(&wrong_domain).is_err());
    let oversized = vec![0; PhloGenesisPolicy::LIMITS.wire.total_bytes + 1];
    assert!(adopted.check_acquisition_terms(&oversized).is_err());
    let mut invalid_length = vec![0xff; 8];
    invalid_length.extend_from_slice(b"small");
    assert!(adopted.check_acquisition_terms(&invalid_length).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn acquisition_compatibility_preserves_arbitrary_original_prices(
        original_price in any::<u64>(),
        subsequent_price in any::<u64>(),
        class in 0usize..2,
        changed_weight in any::<u64>(),
    ) {
        let adopted = acquisition_context();
        let mut original = adopted.genesis().record().schedule().unwrap();
        original.actual_price = original_price;
        let encoded = original.encode(PhloGenesisPolicy::LIMITS).unwrap();
        let checked = adopted.check_acquisition_terms(&encoded).unwrap();
        prop_assert_eq!(checked.schedule(), &original);
        prop_assert_eq!(checked.bytes(), encoded.as_slice());
        let mut later = original.clone();
        later.actual_price = subsequent_price;
        let later_bytes = later.encode(PhloGenesisPolicy::LIMITS).unwrap();
        let later_checked = adopted.check_acquisition_terms(&later_bytes).unwrap();
        prop_assert_eq!(later_checked.schedule().actual_price, subsequent_price);
        prop_assert_eq!(checked.schedule().actual_price, original_price);
        prop_assert_eq!(encoded == later_bytes, original_price == subsequent_price);
        let old_weight = original.classes[class].weight;
        original.classes[class].weight = changed_weight;
        let changed = original.encode(PhloGenesisPolicy::LIMITS).unwrap();
        prop_assert_eq!(adopted.check_acquisition_terms(&changed).is_ok(), changed_weight == old_weight);
    }

    #[test]
    fn adoption_and_capture_refine_the_genesis_bound_context(
        minimum in 0i64..=i64::MAX,
        version in 0i64..=i64::MAX,
        local_minimum in any::<i64>(),
        local_version in any::<i64>(),
        shard in "[a-z]{1,24}",
        changed_shard in any::<bool>(),
        capture_minimum in any::<u64>(),
        capture_version in any::<u64>(),
    ) {
        let record = policy(minimum as u64, version as u64, &shard);
        let other_shard = format!("{shard}/other");
        let candidate_shard = if changed_shard { &other_shard } else { &shard };
        let adopted = CasperShardConf {
            min_phlo_price: local_minimum,
            casper_version: local_version,
            shard_name: candidate_shard.clone(),
            ..CasperShardConf::new()
        };
        prop_assert_eq!(record.clone().adopt(&adopted).is_ok(),
            local_minimum == minimum && local_version == version && !changed_shard);
        let context = record.adopt(&CasperShardConf {
            min_phlo_price: minimum,
            casper_version: version,
            shard_name: shard.clone(),
            ..CasperShardConf::new()
        }).unwrap();
        prop_assert!(check_capture(&context, minimum as u64, version as u64, &shard));
        prop_assert_eq!(check_capture(&context, capture_minimum, capture_version, candidate_shard),
            capture_minimum == minimum as u64 && capture_version == version as u64 && !changed_shard);
    }
}
