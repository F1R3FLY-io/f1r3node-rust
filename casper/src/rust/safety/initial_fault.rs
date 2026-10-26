use std::collections::HashMap;
use std::hash::Hash;

pub(crate) fn equivocating_weight<'a, K: Eq + Hash + 'a>(
    weights: &HashMap<K, u64>,
    equivocators: impl IntoIterator<Item = &'a K>,
) -> u64 {
    equivocators
        .into_iter()
        .filter_map(|equivocator| weights.get(equivocator))
        .sum()
}

pub(crate) fn total_weight<K: Eq + Hash>(weights: &HashMap<K, u64>) -> u64 {
    weights.values().sum()
}

pub(crate) fn normalized_initial_fault(equivocating_weight: u64, total_weight: u64) -> f32 {
    if total_weight == 0 {
        0.0
    } else {
        equivocating_weight as f32 / total_weight as f32
    }
}

pub(crate) fn display_projection(base: f32, initial_fault: f32) -> f32 { base - initial_fault }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_arithmetic_preserves_record_multiplicity_and_display_bits() {
        let weights = HashMap::from([(1u8, 3u64), (2, 5)]);
        let records = [1u8, 1, 9];
        let matched = equivocating_weight(&weights, records.iter());
        let total = total_weight(&weights);
        assert_eq!((matched, total), (6, 8));
        let fault = normalized_initial_fault(matched, total);
        assert_eq!(fault.to_bits(), 0.75f32.to_bits());
        assert_eq!(
            display_projection(0.5, fault).to_bits(),
            (-0.25f32).to_bits()
        );
    }

    #[test]
    fn helpers_match_frozen_handoff_arithmetic_on_boundary_inputs() {
        let cases: Vec<(HashMap<u8, u64>, Vec<u8>)> = vec![
            (HashMap::new(), vec![]),
            (HashMap::from([(1, 0)]), vec![1, 1, 9]),
            (HashMap::from([(1, 3), (2, 5)]), vec![1, 1, 9]),
            (HashMap::from([(1, 16_777_217), (2, 16_777_219)]), vec![
                1, 2,
            ]),
            (HashMap::from([(1, u64::MAX)]), vec![1]),
            (HashMap::from([(1, u64::MAX / 2), (2, u64::MAX / 2)]), vec![
                1, 2,
            ]),
        ];
        for (weights, records) in cases {
            let frozen_matched: u64 = records.iter().filter_map(|key| weights.get(key)).sum();
            let frozen_total: u64 = weights.values().sum();
            let frozen_fault = if frozen_total == 0 {
                0.0
            } else {
                frozen_matched as f32 / frozen_total as f32
            };
            let fault = normalized_initial_fault(
                equivocating_weight(&weights, records.iter()),
                total_weight(&weights),
            );
            assert_eq!(fault.to_bits(), frozen_fault.to_bits());
            for base in [0.0, -0.0, 1.0, -1.0, f32::from_bits(1), f32::MAX, f32::MIN] {
                assert_eq!(
                    display_projection(base, fault).to_bits(),
                    (base - frozen_fault).to_bits()
                );
            }
        }
        assert_eq!(
            normalized_initial_fault(u64::MAX, 0).to_bits(),
            0.0f32.to_bits()
        );
    }

    #[test]
    fn ordinary_sum_overflow_matches_the_compiler_profile() {
        let weights = HashMap::from([(1u8, u64::MAX), (2, 1)]);
        let frozen = std::panic::catch_unwind(|| weights.values().sum::<u64>());
        let actual = std::panic::catch_unwind(|| total_weight(&weights));
        match (frozen, actual) {
            (Ok(left), Ok(right)) => assert_eq!(left, right),
            (Err(_), Err(_)) => {}
            _ => panic!("production overflow policy changed"),
        }
    }
}
