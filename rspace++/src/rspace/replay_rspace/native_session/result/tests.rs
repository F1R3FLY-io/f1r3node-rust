use std::cell::{Cell, RefCell};

use proptest::prelude::*;

use super::*;
use crate::rspace::internal::Datum;
use crate::rspace::trace::event::Produce;

fn candidates(entries: &[(i32, bool)]) -> Vec<ConsumeCandidate<usize, usize>> {
    entries
        .iter()
        .enumerate()
        .map(|(position, &(index, persistent))| ConsumeCandidate {
            channel: position,
            datum: Datum {
                a: position + 1000,
                persist: persistent,
                source: Produce::default(),
            },
            datum_index: index,
            removed_datum: position + 2000,
        })
        .collect()
}

proptest! {
    #[test]
    fn prepared_data_preserves_match_order_and_exact_retirement(
        entries in prop::collection::vec((-1_i32..512, any::<bool>()), 0..128),
    ) {
        let reservations = RefCell::new(Vec::new());
        let prepared = prepare(candidates(&entries), |work, bytes| {
            reservations.borrow_mut().push((work, bytes));
            Ok(())
        }).unwrap();
        let expected_data: Vec<_> = entries.iter().enumerate().map(|(position, &(_, persistent))| {
            RSpaceResult {
                channel: position,
                matched_datum: position + 1000,
                removed_datum: position + 2000,
                persistent,
            }
        }).collect();
        let mut expected_retirement: Vec<_> = entries.iter().enumerate()
            .filter(|(_, (index, persistent))| *index >= 0 && !persistent)
            .map(|(position, &(index, _))| (position, index)).collect();
        expected_retirement.sort_by_key(|&(position, index)| (std::cmp::Reverse(index), position));
        prop_assert_eq!(&prepared.data, &expected_data);
        prop_assert_eq!(&prepared.retirement, &expected_retirement);
        prop_assert!(prepared.retirement.len() <= entries.len());
        let reservations = reservations.borrow();
        let expected_backing = entries.len() *
            (size_of::<RSpaceResult<usize, usize>>() + size_of::<(usize, i32)>());
        prop_assert_eq!(reservations[0], (3 * entries.len(), expected_backing));
        prop_assert!(reservations[1..].iter().all(|entry| *entry == (1, 0)));
    }
}

#[test]
fn every_reservation_failure_prevents_a_prepared_result() {
    let entries = [(7, false), (-1, false), (4, true), (0, false), (2, false)];
    let total = Cell::new(0);
    prepare(candidates(&entries), |_, _| {
        total.set(total.get() + 1);
        Ok(())
    })
    .unwrap();
    assert!(total.get() > 1);
    for failure in 0..total.get() {
        let calls = Cell::new(0);
        let prepared = prepare(candidates(&entries), |_, _| {
            let call = calls.get();
            calls.set(call + 1);
            if call == failure {
                Err(RSpaceError::HostWorkRejected)
            } else {
                Ok(())
            }
        });
        assert!(matches!(prepared, Err(RSpaceError::HostWorkRejected)));
        assert_eq!(calls.get(), failure + 1);
    }
}

#[test]
fn result_preparation_moves_channels_and_payloads() {
    #[derive(Debug)]
    struct NoClone(u32);
    impl Clone for NoClone {
        fn clone(&self) -> Self {
            panic!("result preparation must not clone payloads");
        }
    }
    let input = vec![ConsumeCandidate {
        channel: NoClone(1),
        datum: Datum {
            a: NoClone(2),
            persist: false,
            source: Produce::default(),
        },
        datum_index: 3,
        removed_datum: NoClone(4),
    }];
    let prepared = prepare(input, |_, _| Ok(())).unwrap();
    assert_eq!(prepared.data[0].channel.0, 1);
    assert_eq!(prepared.data[0].matched_datum.0, 2);
    assert_eq!(prepared.data[0].removed_datum.0, 4);
    assert_eq!(prepared.retirement, [(0, 3)]);
}

#[test]
fn backing_overflow_is_a_host_rejection() {
    assert!(matches!(backing::<u8, u8>(usize::MAX), Err(RSpaceError::HostWorkRejected)));
    assert!(matches!(backing::<(), ()>(usize::MAX), Err(RSpaceError::HostWorkRejected)));
    assert_eq!(backing::<u8, u8>(0).unwrap(), 0);
}

#[test]
fn persistent_and_incoming_data_never_enter_retirement() {
    let prepared =
        prepare(candidates(&[(-1, false), (-1, true), (0, true)]), |_, _| Ok(())).unwrap();
    assert_eq!(prepared.data.len(), 3);
    assert!(prepared.retirement.is_empty());
}
