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
}
