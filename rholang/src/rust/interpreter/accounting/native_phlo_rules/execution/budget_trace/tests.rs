use std::cell::Cell;

use proptest::prelude::*;

use super::{sort_indices, NativeBudgetTraceError};

#[test]
fn rejected_comparison_stops_sorting_immediately() {
    let mut indices = (0..64).rev().collect::<Vec<_>>();
    let calls = Cell::new(0);
    let compare = |left: usize, right: usize| {
        let call = calls.get() + 1;
        calls.set(call);
        assert!(call <= 3);
        if call == 3 {
            Err(NativeBudgetTraceError::Limit)
        } else {
            Ok(left.cmp(&right))
        }
    };
    assert!(matches!(
        sort_indices(&mut indices, &compare),
        Err(NativeBudgetTraceError::Limit)
    ));
    assert_eq!(calls.get(), 3);
}

proptest! {
    #[test]
    fn fallible_index_sort_matches_the_total_order(mut values in prop::collection::vec(any::<usize>(), 0..512)) {
        let mut expected = values.clone();
        expected.sort_unstable();
        sort_indices(&mut values, &|left, right| Ok(left.cmp(&right))).unwrap();
        prop_assert_eq!(values, expected);
    }
}
