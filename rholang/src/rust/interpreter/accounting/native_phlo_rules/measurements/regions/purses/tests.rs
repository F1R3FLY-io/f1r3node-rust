use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{CostSignatureCompound, GPrivate, GUnforgeable};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use proptest::prelude::*;

use super::super::super::tests::{row, snapshot};
use super::*;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_receipts::ByteObservation;
use crate::rust::interpreter::accounting::native_phlo_rules::tests::schedule;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloRegionLimits, NativePhloRules,
};

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn region_limits() -> NativePhloRegionLimits {
    NativePhloRegionLimits {
        regions: 100_000,
        encoded_authority_bytes: 10_000_000,
    }
}

fn limits() -> NativePhloPurseLimits {
    NativePhloPurseLimits {
        bindings: 100_000,
        encoded_binding_bytes: 10_000_000,
    }
}

fn independent_channel(atoms: impl IntoIterator<Item = Vec<u8>>) -> Par {
    let channel = Par {
        unforgeables: atoms
            .into_iter()
            .map(|bytes| GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: Blake2b256::hash(bytes),
                })),
            })
            .collect(),
        ..Par::default()
    };
    ParSortMatcher::sort_match(&channel).term
}

fn distinct_owner_row(quantities: [u64; 3], owners: usize) -> Arc<ByteObservation> {
    let mut observation = row(AuthorityByteEventKind::Comm, quantities, 0);
    let signature = CostSignature {
        value: Some(Value::Compound(CostSignatureCompound {
            elements: (0..owners)
                .map(|owner| CostSignature {
                    value: Some(Value::Ground((owner as u64).to_be_bytes().to_vec())),
                })
                .collect(),
        })),
    };
    Arc::make_mut(&mut observation).authority.regions[0].signature =
        Some(sort_signature(&signature).term);
    observation
}

#[test]
fn purse_aliases_preserve_both_original_authorities_and_every_occurrence() {
    let rules = NativePhloRules::resolve(&schedule()).unwrap();
    let mut observation = row(AuthorityByteEventKind::Comm, [3, 5, 7], 0);
    let regions = &mut Arc::make_mut(&mut observation).authority.regions;
    regions[0].signature = Some(CostSignature {
        value: Some(Value::Quote(Par::default())),
    });
    let mut named = regions[0].clone();
    named.instance_id = vec![4; 32];
    named.signature = Some(CostSignature {
        value: Some(Value::Name(Par::default())),
    });
    regions.push(named);
    let input = snapshot(vec![observation.clone(), observation]);
    let measured = rules.measure(&input, 2).unwrap();
    let demands = measured.region_demands(region_limits(), &budget()).unwrap();
    let located = demands.locate_purses(limits(), &budget()).unwrap();
    assert_eq!(located.binding_count(), 2);
    let original: Vec<_> = demands.occurrences().collect();
    let projected: Vec<_> = located.occurrences().collect();
    assert_eq!(projected.len(), 16);
    assert_eq!(
        projected
            .iter()
            .map(|item| item.demand())
            .collect::<Vec<_>>(),
        original
    );
    let channel = independent_channel([Vec::new()]);
    for pair in projected.chunks_exact(2) {
        assert_ne!(pair[0].purse().authority(), pair[1].purse().authority());
        assert_ne!(
            pair[0].purse().original_authority(),
            pair[1].purse().original_authority()
        );
        for item in pair {
            assert_eq!(item.purse().channel(), &channel);
            assert_eq!(item.purse().encoded_channel(), channel.encode_to_vec());
            assert_eq!(
                item.purse().original_authority(),
                item.demand().region().signature.as_ref().unwrap()
            );
        }
    }
    assert!(std::ptr::eq(projected[0].purse(), projected[8].purse()));
}

#[test]
fn conflicting_region_identity_rejects_even_zero_quantity_rows_and_channel_aliases() {
    let rules = NativePhloRules::resolve(&schedule()).unwrap();
    let mut first = row(AuthorityByteEventKind::Comm, [1; 3], 0);
    Arc::make_mut(&mut first).authority.regions[0].signature = Some(CostSignature {
        value: Some(Value::Quote(Par::default())),
    });
    let mut conflict = row(AuthorityByteEventKind::ProduceIntroduction, [0; 3], 0);
    Arc::make_mut(&mut conflict).authority.regions[0].signature = Some(CostSignature {
        value: Some(Value::Name(Par::default())),
    });
    for rows in [vec![first.clone(), conflict.clone()], vec![conflict, first]] {
        let input = snapshot(rows);
        let original = input.clone();
        let measured = rules.measure(&input, 2).unwrap();
        let demands = measured.region_demands(region_limits(), &budget()).unwrap();
        assert_eq!(
            demands.locate_purses(limits(), &budget()).unwrap_err(),
            NativePhloPurseError::Authority(AuthorityError::RegionIdentityConflict)
        );
        assert_eq!(input, original);
    }
}

#[test]
fn purse_limits_are_exact_and_include_unit_and_repeated_binding_work() {
    let rules = NativePhloRules::resolve(&schedule()).unwrap();
    for owners in [0, 1, 3, 65, 1000] {
        let observation = distinct_owner_row([u64::MAX; 3], owners);
        let input = snapshot(vec![observation.clone()]);
        let measured = rules.measure(&input, 1).unwrap();
        let demands = measured.region_demands(region_limits(), &budget()).unwrap();
        let channel =
            independent_channel((0..owners).map(|owner| (owner as u64).to_be_bytes().to_vec()));
        let exact = NativePhloPurseLimits {
            bindings: 1,
            encoded_binding_bytes: observation.authority.regions[0]
                .signature
                .as_ref()
                .unwrap()
                .encoded_len()
                + channel.encoded_len(),
        };
        let located = demands.locate_purses(exact, &budget()).unwrap();
        assert_eq!(located.binding_count(), 1);
        for item in located.occurrences() {
            assert_eq!(item.purse().channel(), &channel);
        }
        for (limit, error) in [
            (
                NativePhloPurseLimits {
                    bindings: 0,
                    ..exact
                },
                NativePhloPurseError::BindingLimit,
            ),
            (
                NativePhloPurseLimits {
                    encoded_binding_bytes: exact.encoded_binding_bytes - 1,
                    ..exact
                },
                NativePhloPurseError::BindingByteLimit,
            ),
        ] {
            assert_eq!(demands.locate_purses(limit, &budget()).unwrap_err(), error);
        }
        for dimension in [
            HostWorkDimension::StructuralItems,
            HostWorkDimension::StructuralBytes,
            HostWorkDimension::VerificationOperations,
            HostWorkDimension::VerificationBytes,
        ] {
            let mut caps = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
            caps.set(dimension, HostWorkLimit::new(0));
            let work = HostWorkBudget::new(caps);
            assert_eq!(
                demands.locate_purses(exact, &work).unwrap_err(),
                NativePhloPurseError::Authority(AuthorityError::HostWorkRejected)
            );
            assert!(work.is_rejected());
        }
        let observation = distinct_owner_row([1; 3], owners);
        let repeated = snapshot(vec![observation.clone(), observation]);
        let measured = rules.measure(&repeated, 2).unwrap();
        let demands = measured.region_demands(region_limits(), &budget()).unwrap();
        assert_eq!(
            demands
                .locate_purses(exact, &budget())
                .unwrap()
                .occurrences()
                .count(),
            8
        );
        let mut caps = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
        caps.set(
            HostWorkDimension::VerificationOperations,
            HostWorkLimit::new(3),
        );
        assert!(demands
            .locate_purses(exact, &HostWorkBudget::new(caps))
            .is_err());
    }
    let input = snapshot(Vec::new());
    let measured = rules.measure(&input, 0).unwrap();
    let demands = measured.region_demands(region_limits(), &budget()).unwrap();
    let located = demands
        .locate_purses(
            NativePhloPurseLimits {
                bindings: 0,
                encoded_binding_bytes: 0,
            },
            &budget(),
        )
        .unwrap();
    assert_eq!(located.binding_count(), 0);
    assert_eq!(located.occurrences().count(), 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn mixed_authority_channels_preserve_quoted_bytes_and_duplicate_atoms(
        payloads in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..64), 1..24),
    ) {
        let rules = NativePhloRules::resolve(&schedule()).unwrap();
        let mut elements = Vec::new();
        let mut reflected = Vec::new();
        for payload in payloads {
            let quoted = independent_channel([payload.clone()]);
            let quoted_bytes = quoted.encode_to_vec();
            elements.push(CostSignature { value: Some(Value::Ground(payload.clone())) });
            elements.push(CostSignature { value: Some(Value::Quote(quoted.clone())) });
            elements.push(CostSignature { value: Some(Value::Name(quoted)) });
            reflected.extend([payload, quoted_bytes.clone(), quoted_bytes]);
        }
        let signature = sort_signature(&CostSignature {
            value: Some(Value::Compound(CostSignatureCompound { elements })),
        }).term;
        let mut observation = row(AuthorityByteEventKind::Comm, [1; 3], 0);
        Arc::make_mut(&mut observation).authority.regions[0].signature = Some(signature.clone());
        let input = snapshot(vec![observation]);
        let measured = rules.measure(&input, 1).unwrap();
        let demands = measured.region_demands(region_limits(), &budget()).unwrap();
        let channel = independent_channel(reflected);
        let located = demands.locate_purses(NativePhloPurseLimits {
            bindings: 1,
            encoded_binding_bytes: signature.encoded_len() + channel.encoded_len(),
        }, &budget()).unwrap();
        for item in located.occurrences() {
            prop_assert_eq!(item.purse().original_authority(), &signature);
            prop_assert_eq!(item.purse().channel(), &channel);
        }
    }

    #[test]
    fn purse_projection_preserves_full_evidence_under_permutation_and_repetition(
        samples in prop::collection::vec((0_usize..96, prop::array::uniform3(0_u64..10_000), 1_usize..5), 0..12),
        reverse in any::<bool>(),
    ) {
        let rules = NativePhloRules::resolve(&schedule()).unwrap();
        let mut rows = Vec::new();
        let mut channels = BTreeMap::new();
        for (index, (owners, quantities, repeats)) in samples.iter().enumerate() {
            let mut observation = row(AuthorityByteEventKind::Comm, *quantities, *owners);
            Arc::make_mut(&mut observation).authority.regions[0].instance_id = vec![index as u8; 32];
            channels.insert(vec![index as u8; 32], independent_channel((0..*owners).map(|owner| vec![owner as u8])));
            rows.extend(std::iter::repeat_n(observation, *repeats));
        }
        if reverse { rows.reverse(); }
        let input = snapshot(rows);
        let measured = rules.measure(&input, 48).unwrap();
        let demands = measured.region_demands(region_limits(), &budget()).unwrap();
        let located = demands.locate_purses(limits(), &budget()).unwrap();
        prop_assert_eq!(located.binding_count(), samples.len());
        let original: Vec<_> = demands.occurrences().collect();
        let actual: Vec<_> = located.occurrences().collect();
        prop_assert_eq!(actual.len(), original.len());
        for (projected, expected) in actual.iter().zip(&original) {
            prop_assert_eq!(projected.demand(), *expected);
            prop_assert_eq!(projected.purse().channel(), &channels[&expected.region().instance_id]);
            prop_assert_eq!(projected.purse().encoded_channel(), channels[&expected.region().instance_id].encode_to_vec());
            prop_assert_eq!(projected.purse().original_authority(), expected.region().signature.as_ref().unwrap());
        }
    }
}
