use models::rust::phlo_resource::PhloResourceLimits;
use models::rust::phlo_wire::PhloWireLimits;
use proptest::prelude::*;

use super::*;

const LIMITS: PhloExecutionLimits = PhloExecutionLimits {
    resource_entries: 1,
    authority_nodes: 8192,
    key_bytes: 1_048_576,
};
const WIRE: PhloResourceLimits = PhloResourceLimits {
    wire: PhloWireLimits {
        total_bytes: 1_048_576,
        field_bytes: 524_288,
    },
    authority_nodes: 8192,
};

fn resource(authority: &Sig) -> PhloResource<'_> {
    PhloResource {
        location: b"location",
        class: 2,
        acquisition_terms: b"terms",
        authority,
    }
}

fn restore_budget() -> HostWorkBudget {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

#[test]
fn phlo_wire_key_uses_the_same_authority_nodes_as_native_permissions() {
    let signature = Sig::And(
        Box::new(Sig::Ground(vec![1])),
        Box::new(Sig::And(
            Box::new(Sig::Quote(vec![2, 3])),
            Box::new(Sig::Ground(vec![1])),
        )),
    );
    let native = resource(&signature);
    let key = native.wire_key(LIMITS).unwrap();
    assert_eq!(key.authority, vec![
        AuthorityNode::And,
        AuthorityNode::Ground(&[1]),
        AuthorityNode::And,
        AuthorityNode::Quote(&[2, 3]),
        AuthorityNode::Ground(&[1])
    ]);
    let wire = key.encode(WIRE).unwrap();
    let decoded = PhloResourceKeyV1::decode(&wire, WIRE).unwrap();
    assert_eq!(decoded, key);
    let mut budget = WorkBudget {
        remaining_nodes: LIMITS.authority_nodes,
        remaining_bytes: LIMITS.key_bytes,
    };
    let internal = resource_key(native, &mut budget).unwrap();
    assert_eq!(internal.authority, decoded.authority);
    assert_eq!(internal.location, decoded.location);
    assert_eq!(internal.class, decoded.class as usize);
    assert_eq!(internal.acquisition_terms, decoded.acquisition_terms);
    let restored =
        RestoredPhloResource::from_wire_key(&decoded, LIMITS, &restore_budget()).unwrap();
    assert_eq!(restored.resource(), native);
}

#[test]
fn phlo_wire_key_preserves_existing_rejections_and_exact_limits() {
    for unsupported in [
        Sig::Threshold {
            threshold: 1,
            members: vec![Sig::Ground(vec![1])],
        },
        Sig::Plus(Box::new(Sig::Unit), Box::new(Sig::Unit)),
        Sig::With(Box::new(Sig::Unit), Box::new(Sig::Unit)),
    ] {
        assert_eq!(
            resource(&unsupported).wire_key(LIMITS),
            Err(PhloExecutionError::UnsupportedFundingAuthority)
        );
    }
    let signature = Sig::And(
        Box::new(Sig::Ground(vec![1])),
        Box::new(Sig::Quote(vec![2, 3])),
    );
    let native = resource(&signature);
    let exact = PhloExecutionLimits {
        resource_entries: 1,
        authority_nodes: 3,
        key_bytes: 16,
    };
    assert!(native.wire_key(exact).is_ok());
    assert_eq!(
        native.wire_key(PhloExecutionLimits {
            resource_entries: 0,
            ..exact
        }),
        Err(PhloExecutionError::TooManyResourceEntries)
    );
    assert_eq!(
        native.wire_key(PhloExecutionLimits {
            authority_nodes: 2,
            ..exact
        }),
        Err(PhloExecutionError::TooManyAuthorityNodes)
    );
    assert_eq!(
        native.wire_key(PhloExecutionLimits {
            key_bytes: 15,
            ..exact
        }),
        Err(PhloExecutionError::TooManyKeyBytes)
    );
    if usize::BITS > 32 {
        let oversized = PhloResource {
            class: u32::MAX as usize + 1,
            ..native
        };
        assert_eq!(
            oversized.wire_key(LIMITS),
            Err(PhloExecutionError::UnknownResourceClass)
        );
    }
}

#[test]
fn phlo_wire_key_does_not_limit_compound_authorities_to_two_wallets() {
    for count in [1, 2, 3, 64, 129, 1024] {
        let mut signature = Sig::Ground(vec![0]);
        for wallet in 1u32..count {
            signature = Sig::And(
                Box::new(signature),
                Box::new(Sig::Ground(wallet.to_be_bytes().to_vec())),
            );
        }
        let key = resource(&signature).wire_key(LIMITS).unwrap();
        assert_eq!(key.authority.len(), 2 * count as usize - 1);
        let wire = key.encode(WIRE).unwrap();
        assert_eq!(PhloResourceKeyV1::decode(&wire, WIRE).unwrap(), key);
        assert_eq!(
            RestoredPhloResource::from_wire_key(&key, LIMITS, &restore_budget())
                .unwrap()
                .resource(),
            resource(&signature)
        );
    }
}

fn signature() -> impl Strategy<Value = Sig> {
    prop_oneof![
        Just(Sig::Unit),
        prop::collection::vec(any::<u8>(), 0..32).prop_map(Sig::Ground),
        prop::collection::vec(any::<u8>(), 0..32).prop_map(Sig::Quote)
    ]
    .prop_recursive(7, 128, 2, |inner| {
        (inner.clone(), inner).prop_map(|(left, right)| Sig::And(Box::new(left), Box::new(right)))
    })
}

#[test]
fn key_stack_pop_and_push_reuse_reserved_storage_and_charge_only_insertions() {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000)));
    let mut stack = Vec::new();
    let mut reserved = 0;
    for value in 0..128u64 {
        push_key_entry(&mut stack, &mut reserved, value, Some(&host)).unwrap();
        assert!(stack.len() <= reserved);
        assert!(stack.capacity() >= reserved);
    }
    let bytes = host.usage(HostWorkDimension::SearchStateBytes);
    let operations = host.usage(HostWorkDimension::VerificationOperations).get();
    let pointer = stack.as_ptr();
    for _ in 0..1024 {
        let value = stack.pop().unwrap();
        push_key_entry(&mut stack, &mut reserved, value, Some(&host)).unwrap();
        assert_eq!(reserved, 128);
        assert_eq!(stack.as_ptr(), pointer);
        assert_eq!(host.usage(HostWorkDimension::SearchStateBytes), bytes);
    }
    assert_eq!(
        host.usage(HostWorkDimension::VerificationOperations).get(),
        operations + 1024
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn key_stack_generated_operations_preserve_capacity_and_reservation_accounting(
        operations in prop::collection::vec(any::<bool>(), 0..512),
    ) {
        use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
        let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000)));
        let mut stack = Vec::new();
        let mut reserved = 0;
        let mut model = Vec::new();
        let mut capacity = 0usize;
        let mut bytes = 0u64;
        let mut work = 0u64;
        for (index, push) in operations.into_iter().enumerate() {
            if push {
                if model.len() == capacity {
                    capacity = (capacity * 2).max(1);
                    bytes += (capacity * std::mem::size_of::<u64>()) as u64;
                    work += model.len() as u64;
                }
                work += 1;
                model.push(index as u64);
                push_key_entry(&mut stack, &mut reserved, index as u64, Some(&host)).unwrap();
            } else {
                prop_assert_eq!(stack.pop(), model.pop());
            }
            prop_assert_eq!(&stack, &model);
            prop_assert_eq!(reserved, capacity);
            prop_assert!(stack.len() <= reserved);
            prop_assert!(stack.capacity() >= reserved);
            prop_assert_eq!(host.usage(HostWorkDimension::SearchStateBytes).get(), bytes);
            prop_assert_eq!(host.usage(HostWorkDimension::VerificationOperations).get(), work);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn phlo_wire_identity_equality_matches_native_resource_identity(
        first in signature(), second in signature(), location in prop::collection::vec(any::<u8>(), 0..32),
        terms in prop::collection::vec(any::<u8>(), 0..32), class in any::<u32>(),
    ) {
        let first = PhloResource {authority: &first, location: &location, acquisition_terms: &terms, class: class as usize};
        let second = PhloResource {authority: &second, ..first};
        let budget = || WorkBudget { remaining_nodes: LIMITS.authority_nodes, remaining_bytes: LIMITS.key_bytes };
        let lhs = resource_key(first, &mut budget()).unwrap();
        let rhs = resource_key(second, &mut budget()).unwrap();
        let lhs_wire = first.wire_key(LIMITS).unwrap().encode(WIRE).unwrap();
        let rhs_wire = second.wire_key(LIMITS).unwrap().encode(WIRE).unwrap();
        prop_assert_eq!(lhs == rhs, lhs_wire == rhs_wire);
        prop_assert_eq!(PhloResourceKeyV1::decode(&lhs_wire, WIRE).unwrap(), first.wire_key(LIMITS).unwrap());
        let decoded = PhloResourceKeyV1::decode(&lhs_wire, WIRE).unwrap();
        let restored = RestoredPhloResource::from_wire_key(&decoded, LIMITS, &restore_budget()).unwrap();
        prop_assert_eq!(restored.resource(), first);
    }
}

#[test]
fn phlo_resource_restoration_enforces_exact_limits_and_host_work() {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    let authority = Sig::And(
        Box::new(Sig::Ground(vec![1])),
        Box::new(Sig::Quote(vec![2, 3])),
    );
    let key = resource(&authority).wire_key(LIMITS).unwrap();
    let exact = PhloExecutionLimits {
        resource_entries: 1,
        authority_nodes: 3,
        key_bytes: 16,
    };
    assert_eq!(
        RestoredPhloResource::from_wire_key(&key, exact, &restore_budget())
            .unwrap()
            .resource(),
        resource(&authority)
    );
    for (limits, error) in [
        (
            PhloExecutionLimits {
                resource_entries: 0,
                ..exact
            },
            PhloExecutionError::TooManyResourceEntries,
        ),
        (
            PhloExecutionLimits {
                authority_nodes: 2,
                ..exact
            },
            PhloExecutionError::TooManyAuthorityNodes,
        ),
        (
            PhloExecutionLimits {
                key_bytes: 15,
                ..exact
            },
            PhloExecutionError::TooManyKeyBytes,
        ),
    ] {
        assert_eq!(
            RestoredPhloResource::from_wire_key(&key, limits, &restore_budget()).unwrap_err(),
            error
        );
    }
    for dimension in [
        HostWorkDimension::SearchStateBytes,
        HostWorkDimension::VerificationOperations,
        HostWorkDimension::VerificationBytes,
    ] {
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
        limits.set(dimension, HostWorkLimit::new(0));
        let budget = HostWorkBudget::new(limits);
        assert!(matches!(
            RestoredPhloResource::from_wire_key(&key, exact, &budget),
            Err(PhloExecutionError::HostWork(_))
        ));
        assert!(budget.is_rejected());
    }
}

fn reference_restore(nodes: &[AuthorityNode<'_>], index: &mut usize) -> Option<Sig> {
    let node = nodes.get(*index)?;
    *index += 1;
    Some(match node {
        AuthorityNode::Unit => Sig::Unit,
        AuthorityNode::Ground(bytes) => Sig::Ground(bytes.to_vec()),
        AuthorityNode::Quote(bytes) => Sig::Quote(bytes.to_vec()),
        AuthorityNode::And => Sig::And(
            Box::new(reference_restore(nodes, index)?),
            Box::new(reference_restore(nodes, index)?),
        ),
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn phlo_resource_restoration_rejects_incomplete_or_multiple_trees(tags in prop::collection::vec(0_u8..4, 0..128)) {
        let nodes: Vec<_> = tags.iter().map(|tag| match tag {
            0 => AuthorityNode::Unit,
            1 => AuthorityNode::Ground(b"owner"),
            2 => AuthorityNode::Quote(b"owner"),
            _ => AuthorityNode::And,
        }).collect();
        let key = PhloResourceKeyV1 { location: b"slot", class: u32::MAX, acquisition_terms: b"original", authority: nodes };
        let original = key.clone();
        let mut index = 0;
        let expected = reference_restore(&key.authority, &mut index).filter(|_| index == key.authority.len());
        let actual = RestoredPhloResource::from_wire_key(&key, LIMITS, &restore_budget());
        match expected {
            Some(authority) => {
                let restored = actual.unwrap();
                prop_assert_eq!(restored.resource().authority, &authority);
                prop_assert_eq!(restored.resource().wire_key(LIMITS).unwrap(), key.clone());
            }
            None => prop_assert_eq!(actual.unwrap_err(), PhloExecutionError::MalformedFundingAuthority),
        }
        prop_assert_eq!(key, original);
    }
}
