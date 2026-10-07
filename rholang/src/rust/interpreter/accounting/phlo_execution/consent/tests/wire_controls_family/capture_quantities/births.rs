use super::*;
use crate::rust::interpreter::accounting::authority::{sig_to_cost_signature, AuthorityBornStack};
use crate::rust::interpreter::accounting::phlo_execution::{
    RetainedBirthFunding, RetainedBirthFundingError, RetainedBirthFundingLimits,
};

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

fn check_births(capture: &CanonicalPhloFundingCapture<'_>, failures: bool) {
    let mut columns = Vec::new();
    let births = capture
        .obligations()
        .enumerate()
        .filter_map(|(position, column)| {
            let PhloObligationKey::RetainedResource(resource) = column.key() else {
                return None;
            };
            columns.push(vec![position; column.quantity() as usize]);
            Some(AuthorityBornStack {
                stack_id: [position as u8; 32],
                produce_hash: [position as u8 + 32; 32],
                cells: vec![
                    sig_to_cost_signature(resource.authority).unwrap();
                    column.quantity() as usize
                ],
            })
        })
        .collect::<Vec<_>>();
    let bindings = births
        .iter()
        .zip(&columns)
        .map(|(birth, positions)| RetainedBirthFunding {
            birth,
            obligation_positions: positions,
        })
        .collect::<Vec<_>>();
    let limits = RetainedBirthFundingLimits {
        births: bindings.len(),
        cells: births.iter().map(|birth| birth.cells.len()).sum(),
        obligations: capture.obligations().len(),
        authority_bytes: 1_048_576,
    };
    let checked = capture
        .bind_retained_births(&bindings, limits, &budget())
        .unwrap();
    assert!(std::ptr::eq(checked.capture(), capture));
    assert_eq!(checked.cells().count(), limits.cells);
    let output = checked
        .cells()
        .map(|(birth, index, column)| (birth.stack_id, index, column.encoded_key().to_vec()))
        .collect::<Vec<_>>();
    let reversed = bindings.iter().rev().copied().collect::<Vec<_>>();
    let reordered = capture
        .bind_retained_births(&reversed, limits, &budget())
        .unwrap();
    assert_eq!(
        output,
        reordered
            .cells()
            .map(|(birth, index, column)| (birth.stack_id, index, column.encoded_key().to_vec()))
            .collect::<Vec<_>>()
    );
    for column in capture.obligations() {
        let count = checked
            .cells()
            .filter(|(_, _, cell)| cell.encoded_key() == column.encoded_key())
            .count() as u64;
        assert_eq!(
            count,
            if matches!(column.key(), PhloObligationKey::RetainedResource(_)) {
                column.quantity()
            } else {
                0
            }
        );
    }
    if !failures {
        return;
    }
    assert!(matches!(
        capture.bind_retained_births(&[], limits, &budget()),
        Err(RetainedBirthFundingError::QuantityMismatch)
    ));
    for other in [
        RetainedBirthFundingLimits {
            births: limits.births - 1,
            ..limits
        },
        RetainedBirthFundingLimits {
            cells: limits.cells - 1,
            ..limits
        },
        RetainedBirthFundingLimits {
            obligations: limits.obligations - 1,
            ..limits
        },
        RetainedBirthFundingLimits {
            authority_bytes: 0,
            ..limits
        },
    ] {
        assert!(capture
            .bind_retained_births(&bindings, other, &budget())
            .is_err());
    }
    assert!(capture
        .bind_retained_births(
            &bindings,
            limits,
            &HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)))
        )
        .is_err());
    for field in 0..2 {
        let mut altered = births.clone();
        if field == 0 {
            altered[1].stack_id = altered[0].stack_id;
        } else {
            altered[1].produce_hash = altered[0].produce_hash;
        }
        let replaced = altered
            .iter()
            .zip(&columns)
            .map(|(birth, positions)| RetainedBirthFunding {
                birth,
                obligation_positions: positions,
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            capture.bind_retained_births(&replaced, limits, &budget()),
            Err(RetainedBirthFundingError::DuplicateBirth)
        ));
    }
    let mut altered = births.clone();
    altered[0].cells[0] = sig_to_cost_signature(&Sig::Ground(b"other owner".to_vec())).unwrap();
    let replaced = altered
        .iter()
        .zip(&columns)
        .map(|(birth, positions)| RetainedBirthFunding {
            birth,
            obligation_positions: positions,
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        capture.bind_retained_births(&replaced, limits, &budget()),
        Err(RetainedBirthFundingError::AuthorityMismatch)
    ));
    let nonretained = capture
        .obligations()
        .position(|column| matches!(column.key(), PhloObligationKey::Resource(_)))
        .unwrap();
    for position in [0, nonretained, usize::MAX] {
        let mut substituted = columns.clone();
        substituted[0][0] = position;
        let replaced = births
            .iter()
            .zip(&substituted)
            .map(|(birth, positions)| RetainedBirthFunding {
                birth,
                obligation_positions: positions,
            })
            .collect::<Vec<_>>();
        assert!(capture
            .bind_retained_births(&replaced, limits, &budget())
            .is_err());
    }
    let short = [RetainedBirthFunding {
        birth: &births[0],
        obligation_positions: &columns[0][1..],
    }];
    assert!(matches!(
        capture.bind_retained_births(&short, limits, &budget()),
        Err(RetainedBirthFundingError::CellCount)
    ));
    let mut substituted = columns.clone();
    substituted[0][0] = substituted[1][0];
    let replaced = births
        .iter()
        .zip(&substituted)
        .map(|(birth, positions)| RetainedBirthFunding {
            birth,
            obligation_positions: positions,
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        capture.bind_retained_births(&replaced, limits, &budget()),
        Err(RetainedBirthFundingError::QuantityMismatch)
    ));
}

#[test]
fn retained_birth_binding_checks_complete_physical_cell_funding() {
    for wallets in [1, 3, 65] {
        for price in [0, 1, 9] {
            with_capture(wallets, price, 3, 1, false, |capture| {
                check_births(capture, true)
            });
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn retained_birth_binding_preserves_exact_quantities_and_input_order(
        wallets in 1usize..76, price in 0u64..100, quantity in 1u64..65, rotation in any::<usize>(), reverse in any::<bool>(),
    ) {
        with_capture(wallets, price, quantity, rotation, reverse, |capture| check_births(capture, false));
    }
}
