use std::collections::BTreeMap;

use block_storage::rust::dag::carrier_index::CarrierIndex;
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Operation {
    Record { sig: u8, height: u8, block: u8 },
    Prune { cutoff: u8 },
    SetWatermark { height: u8 },
}

fn operation() -> impl Strategy<Value = Operation> {
    prop_oneof![
        (0u8..8, 0u8..192, any::<u8>()).prop_map(|(sig, height, block)| Operation::Record {
            sig,
            height,
            block
        }),
        (0u8..192).prop_map(|cutoff| Operation::Prune { cutoff }),
        (0u8..192).prop_map(|height| Operation::SetWatermark { height }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn carrier_index_matches_reference_operation_trace(
        operations in prop::collection::vec(operation(), 0..256),
    ) {
        let index = CarrierIndex::in_memory();
        let mut carriers: BTreeMap<u8, BTreeMap<u8, i64>> = BTreeMap::new();
        let mut watermark = None;
        let mut last_prune = None;

        for operation in operations {
            match operation {
                Operation::Record { sig, height, block } => {
                    index
                        .record_once(&[sig], i64::from(height), vec![block; 32])
                        .expect("record carrier");
                    carriers
                        .entry(sig)
                        .or_default()
                        .entry(block)
                        .or_insert(i64::from(height));
                }
                Operation::Prune { cutoff } => {
                    let cutoff = i64::from(cutoff);
                    let removed = index.prune_below(cutoff).expect("prune carriers");
                    let before = carriers.values().map(BTreeMap::len).sum::<usize>();
                    let should_prune = last_prune
                        .is_none_or(|last: i64| cutoff >= last.saturating_add(64));
                    if should_prune {
                        for row in carriers.values_mut() {
                            row.retain(|_, height| *height >= cutoff);
                        }
                        carriers.retain(|_, row| !row.is_empty());
                        last_prune = Some(cutoff);
                    }
                    let after = carriers.values().map(BTreeMap::len).sum::<usize>();
                    prop_assert_eq!(removed as usize, before - after);
                }
                Operation::SetWatermark { height } => {
                    let stored = index
                        .set_watermark_if_absent(i64::from(height))
                        .expect("set watermark");
                    let expected = *watermark.get_or_insert(i64::from(height));
                    prop_assert_eq!(stored, expected);
                }
            }

            prop_assert_eq!(index.watermark().expect("read watermark"), watermark);
            for sig in 0u8..8 {
                let expected_absent = carriers.get(&sig).is_none_or(BTreeMap::is_empty);
                prop_assert_eq!(
                    index.proves_absence(&[sig]).expect("read carrier row"),
                    expected_absent,
                    "signature {}",
                    sig
                );
            }
        }
    }
}
