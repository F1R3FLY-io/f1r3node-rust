use std::cell::RefCell;

use super::*;
use crate::rust::finality::finalization_ledger::effect_observation::effect_is_complete;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Read {
    Cursor,
    Receipt,
}

#[test]
fn completion_observation_preserves_errors_and_short_circuit_read_order() {
    for first in [0, 1] {
        for receipt in [false, true] {
            for last in [first, 1] {
                let expected_reads = if first == 1 {
                    vec![Read::Cursor]
                } else if receipt {
                    vec![Read::Cursor, Read::Receipt]
                } else {
                    vec![Read::Cursor, Read::Receipt, Read::Cursor]
                };
                for failure in 0..=3 {
                    let reads = RefCell::new(Vec::new());
                    let result = effect_is_complete(
                        1,
                        || {
                            let mut reads = reads.borrow_mut();
                            reads.push(Read::Cursor);
                            if reads.len() == failure {
                                Err(failure)
                            } else {
                                Ok(if reads.len() == 1 { first } else { last })
                            }
                        },
                        || {
                            let mut reads = reads.borrow_mut();
                            reads.push(Read::Receipt);
                            if reads.len() == failure {
                                Err(failure)
                            } else {
                                Ok(receipt)
                            }
                        },
                    );
                    if failure > 0 && failure <= expected_reads.len() {
                        assert_eq!(result, Err(failure));
                        assert_eq!(*reads.borrow(), expected_reads[..failure]);
                    } else {
                        assert_eq!(result, Ok(first == 1 || receipt || last == 1));
                        assert_eq!(*reads.borrow(), expected_reads);
                    }
                }
            }
        }
    }
}

proptest! {
    #[test]
    fn completion_observation_refines_monotonic_snapshot_linearizability(
        revision in any::<u64>(),
        cursors in any::<[u64; 3]>(),
        receipts in any::<[bool; 3]>(),
    ) {
        let mut cursors = cursors;
        cursors.sort_unstable();
        let mut cursor_reads = 0;
        let result = effect_is_complete(
            revision,
            || {
                cursor_reads += 1;
                Ok::<_, ()>(cursors[if cursor_reads == 1 { 0 } else { 2 }])
            },
            || Ok(receipts[1]),
        ).unwrap();
        let snapshots = std::array::from_fn::<_, 3, _>(|i| revision <= cursors[i] || receipts[i]);
        prop_assert!(snapshots.contains(&result));
        if !result {
            prop_assert!(!snapshots[1]);
        }
        if snapshots[1] {
            prop_assert!(result);
        }
        prop_assert!(cursor_reads <= 2);
    }
}
