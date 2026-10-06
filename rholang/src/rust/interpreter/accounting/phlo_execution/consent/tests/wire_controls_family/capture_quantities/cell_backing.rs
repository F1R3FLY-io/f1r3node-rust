use super::*;
use crate::rust::interpreter::accounting::phlo_execution::{
    RetainedCellBackingError, RetainedCellBackingLimits,
};

fn cells(
    wallets: usize,
    price: u64,
    quantity: u64,
    rotation: usize,
    reverse: bool,
) -> Vec<Vec<Vec<(Vec<u8>, u64)>>> {
    let mut result = Vec::new();
    with_capture(wallets, price, quantity, rotation, reverse, |capture| {
        let budget =
            || HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)));
        let limits = RetainedCellBackingLimits {
            cells: 4096,
            contributions: 8192,
        };
        for column in capture.obligations() {
            if !matches!(column.key(), PhloObligationKey::RetainedResource(_)) {
                assert!(matches!(
                    column.split_retained_cells(limits, &budget()),
                    Err(RetainedCellBackingError::NotRetained)
                ));
                continue;
            }
            let split = column.split_retained_cells(limits, &budget()).unwrap();
            assert_eq!(split.obligation().encoded_key(), column.encoded_key());
            assert_eq!(split.cells().len() as u64, column.quantity());
            assert_eq!(split.unit_value(), 7 * price);
            let mut totals = vec![0u64; wallets];
            let output = split
                .cells()
                .map(|cell| {
                    assert_eq!(
                        cell.iter().map(|row| row.amount).sum::<u64>(),
                        split.unit_value()
                    );
                    cell.iter()
                        .map(|row| {
                            totals[row.source_index] += row.amount;
                            (
                                capture.sources()[row.source_index]
                                    .source()
                                    .custody
                                    .to_vec(),
                                row.amount,
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                totals,
                column
                    .contributions()
                    .map(|(_, amount)| amount)
                    .collect::<Vec<_>>()
            );
            result.push(output);
        }
    });
    result
}

#[test]
fn checked_retained_cell_backing_binds_original_quantities_sources_and_purpose() {
    for wallets in [1, 3, 65] {
        for price in [0, 1, 19] {
            assert_eq!(
                cells(wallets, price, 7, 0, false),
                cells(wallets, price, 7, wallets / 2, true)
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn checked_retained_cell_backing_is_independent_of_source_and_column_input_order(
        wallets in 1usize..76, price in 0u64..100, quantity in 1u64..65, rotation in any::<usize>(),
    ) {
        prop_assert_eq!(cells(wallets, price, quantity, 0, false), cells(wallets, price, quantity, rotation, true));
    }
}
