use proptest::prelude::*;

use super::*;

pub(super) fn schedule() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip([
                b"compute".as_slice(),
                b"introduction",
                b"transfer",
                b"trace",
            ])
            .map(|(dimension, identity)| dimension.resource_class(identity, 1))
            .collect(),
        actual_price: 1,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

#[test]
fn every_rule_field_is_required_and_unknown_rules_are_rejected() {
    let original = schedule();
    NativePhloRules::resolve(&original).unwrap();
    for class in 0..4 {
        for byte in 0..32 {
            for bit in 0..8 {
                for field in 0..3 {
                    let mut altered = original.clone();
                    match field {
                        0 => altered.classes[class].measurement_rule[byte] ^= 1 << bit,
                        1 => altered.classes[class].valuation_rule[byte] ^= 1 << bit,
                        _ => altered.compatibility_rule[byte] ^= 1 << bit,
                    }
                    assert!(NativePhloRules::resolve(&altered).is_err());
                }
            }
        }
        let mut altered = original.clone();
        altered.classes[class].measurement_unit = b"unimplemented-unit";
        assert_eq!(
            NativePhloRules::resolve(&altered),
            Err(NativePhloRuleError::Unit(class))
        );
    }
}

#[test]
fn all_class_orders_preserve_measurement_identity_and_exact_indices() {
    let original = schedule();
    let mut orders = 0;
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                for d in 0..4 {
                    let order = [a, b, c, d];
                    if (0..4).any(|i| order[..i].contains(&order[i])) {
                        continue;
                    }
                    orders += 1;
                    let mut reordered = original.clone();
                    reordered.classes = order.iter().map(|&i| original.classes[i]).collect();
                    let resolved = NativePhloRules::resolve(&reordered).unwrap();
                    for (position, &index) in order.iter().enumerate() {
                        assert_eq!(
                            resolved.class_for(NativePhloDimension::ALL[index]),
                            Ok(position)
                        );
                    }
                }
            }
        }
    }
    assert_eq!(orders, 24);
}

#[test]
fn duplicate_missing_and_excess_classes_never_receive_an_implicit_mapping() {
    let mut input = schedule();
    input.classes[1] = NativePhloDimension::Compute.resource_class(b"another-compute", 7);
    assert_eq!(
        NativePhloRules::resolve(&input),
        Err(NativePhloRuleError::Duplicate(NativePhloDimension::Compute))
    );
    input.classes.truncate(1);
    let resolved = NativePhloRules::resolve(&input).unwrap();
    assert_eq!(resolved.class_for(NativePhloDimension::Compute), Ok(0));
    for dimension in NativePhloDimension::ALL.into_iter().skip(1) {
        assert_eq!(
            resolved.class_for(dimension),
            Err(NativePhloRuleError::Missing(dimension))
        );
    }
    input.classes.clear();
    assert_eq!(
        NativePhloRules::resolve(&input),
        Err(NativePhloRuleError::ClassCount)
    );
    input.classes = vec![NativePhloDimension::Compute.resource_class(b"extra", 0); 5];
    assert_eq!(
        NativePhloRules::resolve(&input),
        Err(NativePhloRuleError::ClassCount)
    );
}

#[test]
fn rule_identifiers_are_distinct_across_all_rule_kinds() {
    let mut identifiers = NativePhloDimension::ALL
        .map(|d| d.measurement_rule())
        .to_vec();
    identifiers.extend([
        native_authority_valuation_rule(),
        native_resource_compatibility_rule(),
    ]);
    identifiers.sort();
    identifiers.dedup();
    assert_eq!(identifiers.len(), 6);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn resolution_never_replaces_chain_weights_prices_or_identities(
        weights in prop::array::uniform4(any::<u64>()),
        price in any::<u64>(),
        mask in 1_u8..16,
    ) {
        let mut input = schedule();
        for (class, weight) in input.classes.iter_mut().zip(weights) {
            class.weight = weight;
        }
        input.actual_price = price;
        input.classes = input.classes.into_iter().enumerate()
            .filter_map(|(i, class)| (mask & (1 << i) != 0).then_some(class)).collect();
        let captured = input.clone();
        let resolved = NativePhloRules::resolve(&input).unwrap();
        let mut index = 0;
        for dimension in NativePhloDimension::ALL {
            if mask & (1 << dimension.index()) != 0 {
                prop_assert_eq!(resolved.class_for(dimension), Ok(index));
                index += 1;
            } else {
                prop_assert_eq!(resolved.class_for(dimension), Err(NativePhloRuleError::Missing(dimension)));
            }
        }
        prop_assert_eq!(input, captured);
    }
}
