use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

fn limits() -> RetainedCellBackingLimits {
    RetainedCellBackingLimits {
        cells: 4096,
        contributions: 16384,
    }
}

fn check(quantity: u64, value: u64, sources: &[u64]) -> CellBacking {
    let actual = split_backing(
        quantity,
        value,
        sources.iter().copied(),
        limits(),
        &budget(),
    )
    .unwrap();
    assert_eq!(actual.offsets.len(), quantity as usize + 1);
    let mut totals = vec![0u64; sources.len()];
    for (cell, bounds) in actual.offsets.windows(2).enumerate() {
        let rows = &actual.contributions[bounds[0]..bounds[1]];
        assert!(rows
            .windows(2)
            .all(|pair| pair[0].source_index < pair[1].source_index));
        assert!(rows.iter().all(|row| row.amount > 0));
        assert_eq!(rows.iter().map(|row| row.amount).sum::<u64>(), value);
        let mut start = 0u128;
        for (source, amount) in sources.iter().copied().enumerate() {
            let end = start + u128::from(amount);
            let cell_start = cell as u128 * u128::from(value);
            let cell_end = cell_start + u128::from(value);
            let expected = end.min(cell_end).saturating_sub(start.max(cell_start)) as u64;
            let observed = rows
                .iter()
                .find(|row| row.source_index == source)
                .map_or(0, |row| row.amount);
            assert_eq!(observed, expected);
            totals[source] = totals[source].checked_add(observed).unwrap();
            start = end;
        }
    }
    assert_eq!(totals, sources);
    if value == 0 {
        assert!(actual.contributions.is_empty());
    } else {
        let positive = sources.iter().filter(|amount| **amount > 0).count();
        assert!(actual.contributions.len() < positive + quantity as usize);
    }
    actual
}

#[test]
fn retained_cell_backing_preserves_source_and_cell_totals_without_dense_expansion() {
    let actual = check(3, 5, &[4, 7, 4]);
    assert_eq!(actual.offsets, [0, 2, 3, 5]);
    check(1, u64::MAX, &[i64::MAX as u64, 0, i64::MAX as u64, 1]);
    check(3, u64::MAX / 3, &[u64::MAX]);
    for count in [1usize, 3, 65, 1000] {
        for quantity in [1, 3, 31] {
            let total = quantity * 17;
            let sources = (0..count)
                .map(|i| total / count as u64 + u64::from((i as u64) < total % count as u64))
                .collect::<Vec<_>>();
            check(quantity, 17, &sources);
            check(quantity, 0, &vec![0; count]);
        }
    }
}

#[test]
fn retained_cell_backing_rejects_amount_mismatch_overflow_and_limits() {
    for (quantity, value, sources) in [
        (3, 5, vec![4, 7, 3]),
        (3, 5, vec![4, 7, 5]),
        (3, 0, vec![0, 1]),
        (2, u64::MAX, vec![u64::MAX, u64::MAX]),
        (0, 0, vec![]),
        (u64::MAX, 0, vec![0]),
    ] {
        assert!(split_backing(quantity, value, sources.into_iter(), limits(), &budget()).is_err());
    }
    for bound in [
        RetainedCellBackingLimits {
            cells: 2,
            ..limits()
        },
        RetainedCellBackingLimits {
            contributions: 4,
            ..limits()
        },
    ] {
        assert!(split_backing(3, 5, [4, 7, 4].into_iter(), bound, &budget()).is_err());
    }
    assert!(split_backing(
        3,
        5,
        [4, 7, 4].into_iter(),
        limits(),
        &HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)))
    )
    .is_err());
    let exact = RetainedCellBackingLimits {
        cells: 3,
        contributions: 5,
    };
    assert!(split_backing(3, 5, [4, 7, 4].into_iter(), exact, &budget()).is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn retained_cell_backing_matches_interval_model_for_arbitrary_partitions(
        quantity in 1u64..65,
        value in 0u64..100_000,
        mut cuts in prop::collection::vec(any::<u64>(), 0..130),
    ) {
        let total = quantity * value;
        cuts.iter_mut().for_each(|cut| *cut %= total + 1);
        cuts.extend([0, total]);
        cuts.sort_unstable();
        let sources = cuts.windows(2).map(|pair| pair[1] - pair[0]).collect::<Vec<_>>();
        check(quantity, value, &sources);
    }
}
