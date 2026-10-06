use std::collections::BTreeSet;

use proptest::prelude::*;

use super::*;
use crate::rust::phlo_resource::PhloAuthorityNode;

fn limits() -> PhloSourceLimits {
    PhloSourceLimits {
        wire: PhloWireLimits {
            total_bytes: 1_048_576,
            field_bytes: 524_288,
        },
        resource_permissions: 1024,
        authority_nodes: 8192,
    }
}

fn key(class: u32) -> PhloResourceKeyV1<'static> {
    PhloResourceKeyV1 {
        location: b"slot",
        class,
        acquisition_terms: b"terms",
        authority: vec![
            PhloAuthorityNode::And,
            PhloAuthorityNode::Ground(b"A"),
            PhloAuthorityNode::Ground(b"B"),
        ],
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

fn permission_bytes(keys: &[PhloResourceKeyV1<'_>]) -> Vec<u8> {
    let mut bytes = pack(&[&(keys.len() as u32).to_be_bytes()]);
    for key in keys {
        bytes.extend(pack(&[&key.encode(limits().resource(8192)).unwrap()]));
    }
    bytes
}

fn raw(custody: &[u8], hold: &[u8], debit: &[u8], fee: &[u8], permissions: &[u8]) -> Vec<u8> {
    pack(&[
        PHLO_SOURCE_V1_DOMAIN,
        custody,
        hold,
        debit,
        fee,
        permissions,
    ])
}

fn oracle(policy: &PhloSourcePolicyV1<'_>) -> Vec<u8> {
    raw(
        policy.custody(),
        &policy.hold_cap().to_be_bytes(),
        &policy.debit_cap().to_be_bytes(),
        &[u8::from(policy.fee_permitted())],
        &permission_bytes(policy.resources()),
    )
}

#[test]
fn phlo_source_normalizes_permissions_without_changing_limits_or_occurrences() {
    let input = vec![key(7), key(1), key(7), key(3)];
    let policy = PhloSourcePolicyV1::new(b"custody", 100, 40, false, input, limits()).unwrap();
    assert_eq!(policy.resources(), &[key(1), key(3), key(7)]);
    assert_eq!(policy.hold_cap(), 100);
    assert_eq!(policy.debit_cap(), 40);
    assert!(!policy.fee_permitted());
    assert_eq!(policy.resources()[0].authority.len(), 3);
    let wire = policy.encode(limits()).unwrap();
    assert_eq!(wire, oracle(&policy));
    assert_eq!(PhloSourcePolicyV1::decode(&wire, limits()).unwrap(), policy);
}

#[test]
fn phlo_source_binds_custody_caps_fee_and_each_permission_independently() {
    let original =
        PhloSourcePolicyV1::new(b"A", 100, 40, true, vec![key(1), key(2)], limits()).unwrap();
    let bytes = original.encode(limits()).unwrap();
    for (custody, hold, debit, fee, keys) in [
        (b"B".as_slice(), 100, 40, true, vec![key(1), key(2)]),
        (b"A", 101, 40, true, vec![key(1), key(2)]),
        (b"A", 100, 41, true, vec![key(1), key(2)]),
        (b"A", 100, 40, false, vec![key(1), key(2)]),
        (b"A", 100, 40, true, vec![key(1), key(3)]),
        (b"A", 100, 40, true, vec![key(1)]),
    ] {
        let changed = PhloSourcePolicyV1::new(custody, hold, debit, fee, keys, limits()).unwrap();
        let wire = changed.encode(limits()).unwrap();
        assert_ne!(wire, bytes);
        assert_eq!(
            PhloSourcePolicyV1::decode(&wire, limits()).unwrap(),
            changed
        );
    }
}

#[test]
fn phlo_source_fee_only_and_zero_caps_remain_explicit() {
    for hold in [0, 1, u64::MAX] {
        for debit in [0, 1, u64::MAX] {
            for fee in [false, true] {
                let policy =
                    PhloSourcePolicyV1::new(b"A", hold, debit, fee, vec![], limits()).unwrap();
                let wire = policy.encode(limits()).unwrap();
                assert_eq!(PhloSourcePolicyV1::decode(&wire, limits()).unwrap(), policy);
                assert!(policy.resources().is_empty());
                assert_eq!(policy.fee_permitted(), fee);
            }
        }
    }
}

#[test]
fn phlo_source_rejects_noncanonical_permissions_and_header_fields() {
    let h = 10u64.to_be_bytes();
    for keys in [vec![key(1), key(1)], vec![key(2), key(1)]] {
        assert_eq!(
            PhloSourcePolicyV1::decode(
                &raw(b"A", &h, &h, &[1], &permission_bytes(&keys)),
                limits()
            ),
            Err(PhloSourceError::NonCanonicalPermissions)
        );
    }
    for fee in 2..=255 {
        assert_eq!(
            PhloSourcePolicyV1::decode(
                &raw(b"A", &h, &h, &[fee], &permission_bytes(&[])),
                limits()
            ),
            Err(PhloSourceError::FeePermission)
        );
    }
    for width in [0, 1, 7, 9, 16] {
        let malformed = vec![0; width];
        for (hold, debit) in [(&malformed[..], &h[..]), (&h[..], &malformed[..])] {
            assert_eq!(
                PhloSourcePolicyV1::decode(
                    &raw(b"A", hold, debit, &[1], &permission_bytes(&[])),
                    limits()
                ),
                Err(PhloSourceError::FieldWidth)
            );
        }
    }
    for malformed in [vec![], vec![0, 1]] {
        assert_eq!(
            PhloSourcePolicyV1::decode(
                &raw(b"A", &h, &h, &malformed, &permission_bytes(&[])),
                limits()
            ),
            Err(PhloSourceError::FieldWidth)
        );
    }
    assert_eq!(
        PhloSourcePolicyV1::decode(&raw(b"", &h, &h, &[1], &permission_bytes(&[])), limits()),
        Err(PhloSourceError::EmptyCustody)
    );
    assert_eq!(
        PhloSourcePolicyV1::new(b"", 0, 0, true, vec![], limits()),
        Err(PhloSourceError::EmptyCustody)
    );
    let unknown = pack(&[b"other", b"A", &h, &h, &[1], &permission_bytes(&[])]);
    assert_eq!(
        PhloSourcePolicyV1::decode(&unknown, limits()),
        Err(PhloSourceError::FormatDomain)
    );
}

#[test]
fn phlo_source_rejects_truncation_trailing_fields_and_malformed_resources() {
    let policy =
        PhloSourcePolicyV1::new(b"A", 100, 50, true, vec![key(1), key(2)], limits()).unwrap();
    let wire = policy.encode(limits()).unwrap();
    for end in 0..wire.len() {
        assert!(PhloSourcePolicyV1::decode(&wire[..end], limits()).is_err());
    }
    let keys = permission_bytes(policy.resources());
    for end in 0..keys.len() {
        let truncated = raw(
            b"A",
            &100u64.to_be_bytes(),
            &50u64.to_be_bytes(),
            &[1],
            &keys[..end],
        );
        assert!(PhloSourcePolicyV1::decode(&truncated, limits()).is_err());
    }
    let mut extra = wire;
    extra.extend(pack(&[b"extra"]));
    assert_eq!(
        PhloSourcePolicyV1::decode(&extra, limits()),
        Err(PhloSourceError::Wire(PhloWireError::TrailingBytes))
    );
    let mut extra = keys;
    extra.extend(pack(&[b"extra"]));
    assert_eq!(
        PhloSourcePolicyV1::decode(
            &raw(
                b"A",
                &100u64.to_be_bytes(),
                &50u64.to_be_bytes(),
                &[1],
                &extra
            ),
            limits()
        ),
        Err(PhloSourceError::Wire(PhloWireError::TrailingBytes))
    );
    let malformed = pack(&[&1u32.to_be_bytes(), b"bad resource"]);
    assert!(matches!(
        PhloSourcePolicyV1::decode(
            &raw(
                b"A",
                &100u64.to_be_bytes(),
                &50u64.to_be_bytes(),
                &[1],
                &malformed
            ),
            limits()
        ),
        Err(PhloSourceError::Resource(_))
    ));
}

#[test]
fn phlo_source_enforces_aggregate_nodes_bytes_and_permission_counts() {
    let policy =
        PhloSourcePolicyV1::new(b"A", 100, 40, true, vec![key(1), key(2)], limits()).unwrap();
    let wire = policy.encode(limits()).unwrap();
    let exact = PhloSourceLimits {
        wire: PhloWireLimits {
            total_bytes: wire.len(),
            field_bytes: permission_bytes(policy.resources()).len(),
        },
        resource_permissions: 2,
        authority_nodes: 6,
    };
    assert_eq!(policy.encode(exact).unwrap(), wire);
    assert_eq!(PhloSourcePolicyV1::decode(&wire, exact).unwrap(), policy);
    for reduced in [
        PhloSourceLimits {
            authority_nodes: 5,
            ..exact
        },
        PhloSourceLimits {
            resource_permissions: 1,
            ..exact
        },
        PhloSourceLimits {
            wire: PhloWireLimits {
                total_bytes: wire.len() - 1,
                ..exact.wire
            },
            ..exact
        },
        PhloSourceLimits {
            wire: PhloWireLimits {
                field_bytes: exact.wire.field_bytes - 1,
                ..exact.wire
            },
            ..exact
        },
    ] {
        assert!(policy.encode(reduced).is_err());
        assert!(PhloSourcePolicyV1::decode(&wire, reduced).is_err());
        assert!(
            PhloSourcePolicyV1::new(b"A", 100, 40, true, vec![key(1), key(2)], reduced).is_err()
        );
    }
    assert_eq!(
        PhloSourcePolicyV1::new(b"A", 1, 1, false, vec![key(1), key(1), key(1)], exact),
        Err(PhloSourceError::PermissionLimit)
    );
    let forged = raw(
        b"A",
        &1u64.to_be_bytes(),
        &1u64.to_be_bytes(),
        &[0],
        &pack(&[&u32::MAX.to_be_bytes()]),
    );
    assert_eq!(
        PhloSourcePolicyV1::decode(&forged, limits()),
        Err(PhloSourceError::PermissionLimit)
    );
    assert_eq!(
        PhloSourcePolicyV1::decode(&forged, PhloSourceLimits {
            resource_permissions: usize::MAX,
            ..limits()
        }),
        Err(PhloSourceError::Wire(PhloWireError::Truncated))
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn phlo_source_generated_normalization_roundtrip_and_mutation_preserve_consent(
        classes in prop::collection::vec(any::<u32>(), 0..129), hold in any::<u64>(), debit in any::<u64>(),
        fee in any::<bool>(), custody in prop::collection::vec(any::<u8>(), 1..65), mutation in any::<usize>(),
    ) {
        let input = classes.iter().copied().map(key).collect();
        let policy = PhloSourcePolicyV1::new(&custody, hold, debit, fee, input, limits()).unwrap();
        let actual: BTreeSet<_> = policy.resources().iter().map(|resource| resource.class).collect();
        prop_assert_eq!(actual, classes.iter().copied().collect::<BTreeSet<_>>());
        let mut alternate: Vec<_> = classes.iter().rev().copied().map(key).collect();
        alternate.extend(classes.iter().copied().map(key));
        let reordered = PhloSourcePolicyV1::new(&custody, hold, debit, fee, alternate, limits()).unwrap();
        prop_assert_eq!(&policy, &reordered);
        let wire = policy.encode(limits()).unwrap();
        prop_assert_eq!(&wire, &oracle(&policy));
        prop_assert_eq!(PhloSourcePolicyV1::decode(&wire, limits()).unwrap(), policy.clone());
        let mut changed = wire.clone(); let index = mutation % changed.len(); changed[index] ^= 1;
        if let Ok(decoded) = PhloSourcePolicyV1::decode(&changed, limits()) {
            prop_assert_ne!(&decoded, &policy);
            prop_assert_eq!(decoded.encode(limits()).unwrap(), changed);
        }
    }
}
