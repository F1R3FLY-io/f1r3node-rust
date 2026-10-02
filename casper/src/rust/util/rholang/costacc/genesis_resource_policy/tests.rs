use models::rust::phlo_schedule::PhloScheduleV1;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension,
};

use super::*;

fn policy(minimum: u64, version: u64, shard: &str) -> GenesisResourcePolicy {
    let schedule = PhloScheduleV1 {
        protocol_version: version,
        network: b"test",
        shard: shard.as_bytes(),
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: vec![NativePhloDimension::Compute.resource_class(b"compute", 1)],
        actual_price: 10,
        compatibility_rule: native_resource_compatibility_rule(),
    };
    GenesisResourcePolicy {
        genesis_root: vec![9; 32].into(),
        record: PhloGenesisPolicy::from_schedule(&schedule).unwrap(),
        minimum_price: minimum,
    }
}

fn shard_conf(minimum: i64, version: i64, shard: &str) -> CasperShardConf {
    CasperShardConf {
        min_phlo_price: minimum,
        casper_version: version,
        shard_name: shard.into(),
        ..CasperShardConf::new()
    }
}

pub(in crate::rust::util::rholang::costacc) fn retained_record_context() -> AdoptedResourcePolicy {
    policy(0, 6, "root")
        .adopt(&shard_conf(0, 6, "root"))
        .unwrap()
}

#[test]
fn adoption_requires_the_authenticated_genesis_context() {
    let adopted = policy(10, 6, "root")
        .adopt(&shard_conf(10, 6, "root"))
        .unwrap();
    assert_eq!(adopted.genesis().genesis_root().as_ref(), &[9; 32]);
    assert!(!adopted.offered_funded_v6_active());
    let mut fresh = policy(10, 6, "root");
    fresh.record = fresh.record.with_offered_funded_v6_active();
    assert!(fresh
        .adopt(&shard_conf(10, 6, "root"))
        .unwrap()
        .offered_funded_v6_active());
    for (minimum, version, shard) in [
        (-1, 6, "root"),
        (11, 6, "root"),
        (10, -1, "root"),
        (10, 7, "root"),
        (10, 6, "other"),
    ] {
        assert!(policy(10, 6, "root")
            .adopt(&shard_conf(minimum, version, shard))
            .is_err());
    }
}

#[test]
fn genesis_policy_commitment_binds_exact_v1_or_v2_record() {
    let historical = policy(10, 6, "root");
    let historical_bytes = historical.record().encode().unwrap();
    let historical_commitment = historical.commitment().unwrap();
    assert_eq!(
        historical_commitment,
        <[u8; 32]>::try_from(Blake2b256::hash_parts([
            GENESIS_POLICY_COMMITMENT_DOMAIN,
            historical_bytes.as_slice(),
        ]))
        .unwrap()
    );
    assert_eq!(
        historical
            .clone()
            .adopt(&shard_conf(10, 6, "root"))
            .unwrap()
            .genesis_policy_commitment()
            .unwrap(),
        historical_commitment
    );
    let mut fresh = historical;
    fresh.record = fresh.record.with_offered_funded_v6_active();
    assert_ne!(fresh.record().encode().unwrap(), historical_bytes);
    assert_ne!(fresh.commitment().unwrap(), historical_commitment);
}

#[test]
fn acquisition_terms_bind_policy_and_reject_changed_components() {
    let adopted = policy(10, 6, "root")
        .adopt(&shard_conf(10, 6, "root"))
        .unwrap();
    let original = adopted.genesis().record().schedule().unwrap();
    let bytes = original.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let checked = adopted.check_acquisition_terms(&bytes).unwrap();
    assert_eq!(checked.bytes(), bytes);
    assert_eq!(checked.schedule(), &original);

    let mut changed = original.clone();
    changed.shard = b"other";
    let changed_bytes = changed.encode(PhloGenesisPolicy::LIMITS).unwrap();
    assert!(adopted.check_acquisition_terms(&changed_bytes).is_err());

    changed = original.clone();
    changed.actual_price = 25;
    let repriced = changed.encode(PhloGenesisPolicy::LIMITS).unwrap();
    assert_eq!(
        adopted
            .check_acquisition_terms(&repriced)
            .unwrap()
            .schedule()
            .actual_price,
        25
    );
}

#[test]
fn acquisition_terms_reject_truncated_and_oversized_records() {
    let adopted = policy(10, 6, "root")
        .adopt(&shard_conf(10, 6, "root"))
        .unwrap();
    let bytes = adopted
        .genesis()
        .record()
        .schedule()
        .unwrap()
        .encode(PhloGenesisPolicy::LIMITS)
        .unwrap();
    for length in 0..bytes.len() {
        assert!(adopted.check_acquisition_terms(&bytes[..length]).is_err());
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(adopted.check_acquisition_terms(&trailing).is_err());
    assert!(adopted
        .check_acquisition_terms(&vec![0; PhloGenesisPolicy::LIMITS.wire.total_bytes + 1])
        .is_err());
}

#[test]
fn token_metadata_policy_getter_is_opt_in() {
    use crate::rust::genesis::contracts::standard_deploys;

    let record = policy(10, 6, "root").record;
    let historic = standard_deploys::token_metadata("F1R3", "REV", 8, "root");
    let absent = standard_deploys::token_metadata_with_policy("F1R3", "REV", 8, "root", None);
    assert_eq!(historic.data.term, absent.data.term);
    assert!(!historic.data.term.contains("resourcePolicy"));

    let present =
        standard_deploys::token_metadata_with_policy("F1R3", "REV", 8, "root", Some(&record));
    let encoded = hex::encode(record.encode().unwrap());
    assert!(present.data.term.contains("resourcePolicy"));
    assert!(present
        .data
        .term
        .contains(&format!("\"{encoded}\".hexToBytes()")));
}
