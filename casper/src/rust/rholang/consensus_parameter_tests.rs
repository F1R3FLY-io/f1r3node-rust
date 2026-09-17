use models::rhoapi::{ETuple, Expr};
use proptest::prelude::*;

use super::*;

fn expression(value: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(value),
        }],
        ..Par::default()
    }
}

fn tuple(values: &[i64]) -> Par {
    expression(ExprInstance::ETupleBody(ETuple {
        ps: values
            .iter()
            .map(|value| expression(ExprInstance::GInt(*value)))
            .collect(),
        ..ETuple::default()
    }))
}

#[test]
fn every_integer_boundary_matches_upstream_parameter_ranges() {
    let boundaries = [
        i64::MIN,
        -1,
        0,
        1,
        15,
        i64::from(i32::MAX),
        i64::from(i32::MAX) + 1,
        i64::MAX,
    ];
    for depth in boundaries {
        for lifespan in boundaries {
            for minimum in boundaries {
                let expected = (1..=i64::from(i32::MAX)).contains(&depth)
                    && (1..=i64::from(i32::MAX)).contains(&lifespan)
                    && minimum >= 0;
                let result = RuntimeOps::decode_consensus_parameters(
                    &[tuple(&[depth, lifespan, minimum])],
                    &StateHash::new(),
                );
                assert_eq!(result.is_ok(), expected, "{depth}/{lifespan}/{minimum}");
                if expected {
                    assert_eq!(result.unwrap(), Some((depth as i32, lifespan, minimum)));
                }
            }
        }
    }
}

#[test]
fn missing_duplicate_malformed_and_noninteger_results_are_not_adopted() {
    let root = StateHash::new();
    assert_eq!(
        RuntimeOps::decode_consensus_parameters(&[], &root).unwrap(),
        None
    );
    let valid = tuple(&[15, 50, 1]);
    assert!(RuntimeOps::decode_consensus_parameters(&[valid.clone(), valid], &root).is_err());
    for count in [0, 1, 2, 4, 5] {
        assert!(RuntimeOps::decode_consensus_parameters(&[tuple(&vec![1; count])], &root).is_err());
    }
    for slot in 0..3 {
        let mut values = vec![expression(ExprInstance::GInt(1)); 3];
        values[slot] = expression(ExprInstance::GBool(true));
        let result = expression(ExprInstance::ETupleBody(ETuple {
            ps: values,
            ..ETuple::default()
        }));
        assert!(RuntimeOps::decode_consensus_parameters(&[result], &root).is_err());
    }
    assert!(RuntimeOps::decode_consensus_parameters(&[Par::default()], &root).is_err());
    assert!(
        RuntimeOps::decode_consensus_parameters(&[expression(ExprInstance::GInt(1))], &root)
            .is_err()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn decoding_matches_the_formal_range_predicate(
        depth in prop_oneof![any::<i64>(), 1i64..=i64::from(i32::MAX)],
        lifespan in prop_oneof![any::<i64>(), 1i64..=i64::from(i32::MAX)],
        minimum in any::<i64>(),
    ) {
        let expected = depth > 0 && depth <= i64::from(i32::MAX)
            && lifespan > 0 && lifespan <= i64::from(i32::MAX) && minimum >= 0;
        let result = RuntimeOps::decode_consensus_parameters(&[tuple(&[depth, lifespan, minimum])], &StateHash::new());
        prop_assert_eq!(result.is_ok(), expected);
        if let Ok(value) = result { prop_assert_eq!(value, Some((depth as i32, lifespan, minimum))); }
    }
}
