use std::sync::Arc;

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
                a: Arc::new(position + 1000),
                persist: persistent,
                source: Produce::default(),
            },
            datum_index: index,
            removed_datum: Arc::new(position + 2000),
        })
        .collect()
}

proptest! {
    /// Changed by DR-131 S6: the preparation no longer reserves host work.
    /// The test keeps its checks of the match order and of the retirement.
    #[test]
    fn prepared_data_preserves_match_order_and_exact_retirement(
        entries in prop::collection::vec((-1_i32..512, any::<bool>()), 0..128),
    ) {
        let prepared = prepare(candidates(&entries));
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
            a: Arc::new(NoClone(2)),
            persist: false,
            source: Produce::default(),
        },
        datum_index: 3,
        removed_datum: Arc::new(NoClone(4)),
    }];
    let prepared = prepare(input);
    assert_eq!(prepared.data[0].channel.0, 1);
    assert_eq!(prepared.data[0].matched_datum.0, 2);
    assert_eq!(prepared.data[0].removed_datum.0, 4);
    assert_eq!(prepared.retirement, [(0, 3)]);
}

#[test]
fn persistent_and_incoming_data_never_enter_retirement() {
    let prepared = prepare(candidates(&[(-1, false), (-1, true), (0, true)]));
    assert_eq!(prepared.data.len(), 3);
    assert!(prepared.retirement.is_empty());
}
