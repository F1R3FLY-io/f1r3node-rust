use std::collections::HashMap;
use std::hash::Hash;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::ToPrimitive;

pub(crate) fn equivocating_weight<'a, K: Eq + Hash + 'a>(
    weights: &HashMap<K, BigInt>,
    equivocators: impl IntoIterator<Item = &'a K>,
) -> BigInt {
    equivocators
        .into_iter()
        .filter_map(|equivocator| weights.get(equivocator))
        .sum()
}

pub(crate) fn total_weight<K: Eq + Hash>(weights: &HashMap<K, BigInt>) -> BigInt {
    weights.values().sum()
}

pub(crate) fn normalized_initial_fault(equivocating_weight: BigInt, total_weight: BigInt) -> f32 {
    if total_weight <= BigInt::from(0) {
        0.0
    } else {
        BigRational::new(equivocating_weight, total_weight)
            .to_f32()
            .unwrap_or(f32::INFINITY)
    }
}

pub(crate) fn display_projection(base: f32, initial_fault: f32) -> f32 { base - initial_fault }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_fault_preserves_duplicate_record_weights() {
        let weights = HashMap::from([(1u8, BigInt::from(3)), (2, BigInt::from(5))]);
        let matched = equivocating_weight(&weights, [1u8, 1, 9].iter());
        let total = total_weight(&weights);
        assert_eq!(matched, BigInt::from(6));
        assert_eq!(total, BigInt::from(8));
        assert_eq!(normalized_initial_fault(matched, total), 0.75);
        assert_eq!(display_projection(0.5, 0.75), -0.25);
    }

    #[test]
    fn initial_fault_is_scale_invariant_above_float_and_integer_ranges() {
        for shift in [0, 64, 256, 1024] {
            let unit = BigInt::from(1) << shift;
            let weights = HashMap::from([(1u8, &unit * 3), (2, &unit * 5)]);
            assert_eq!(total_weight(&weights), &unit * 8);
            assert_eq!(
                normalized_initial_fault(
                    equivocating_weight(&weights, [1u8, 1].iter()),
                    total_weight(&weights)
                ),
                0.75
            );
        }
        assert_eq!(normalized_initial_fault(1.into(), 0.into()), 0.0);
    }
}
