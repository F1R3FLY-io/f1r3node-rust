use proptest::prelude::*;

use super::*;

const LIMITS: PrepaidCellLimits = PrepaidCellLimits {
    cells: 64,
    wire: PhloWireLimits {
        total_bytes: 8192,
        field_bytes: 256,
    },
};

#[test]
fn ordered_cell_codec_rejects_invalid_shapes_and_bounds() {
    let bytes = OrderedPrepaidCells::encode(&[b"first", b"second", b"first"], LIMITS).unwrap();
    let record = OrderedPrepaidCells::decode(&bytes, LIMITS).unwrap();
    assert_eq!(record.cells(), &[b"first".as_slice(), b"second", b"first"]);
    assert_eq!(
        record.split_consumed(2).unwrap(),
        (&record.cells()[..2], &record.cells()[2..])
    );
    assert!(record.split_consumed(0).is_err());
    assert!(record.split_consumed(4).is_err());
    for end in 0..bytes.len() {
        assert!(OrderedPrepaidCells::decode(&bytes[..end], LIMITS).is_err());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(OrderedPrepaidCells::decode(&extra, LIMITS).is_err());
    let mut domain = bytes.clone();
    domain[8] ^= 1;
    assert!(OrderedPrepaidCells::decode(&domain, LIMITS).is_err());
    let mut count = bytes.clone();
    count[8 + DOMAIN.len()..16 + DOMAIN.len()].fill(255);
    assert!(OrderedPrepaidCells::decode(&count, LIMITS).is_err());
    assert!(OrderedPrepaidCells::encode(&[], LIMITS).is_err());
    assert!(OrderedPrepaidCells::encode(&[b""], LIMITS).is_err());
    for limited in [
        PrepaidCellLimits { cells: 2, ..LIMITS },
        PrepaidCellLimits {
            wire: PhloWireLimits {
                total_bytes: bytes.len() - 1,
                ..LIMITS.wire
            },
            ..LIMITS
        },
        PrepaidCellLimits {
            wire: PhloWireLimits {
                field_bytes: DOMAIN.len() - 1,
                ..LIMITS.wire
            },
            ..LIMITS
        },
    ] {
        assert!(OrderedPrepaidCells::decode(&bytes, limited).is_err());
        assert!(OrderedPrepaidCells::encode(record.cells(), limited).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn ordered_cell_consumption_refines_exact_prefix_conservation(
        cells in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..32), 1..32),
        counts in prop::collection::vec(0usize..40, 0..40),
    ) {
        let originals = cells.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let bytes = OrderedPrepaidCells::encode(&originals, LIMITS).unwrap();
        let decoded = OrderedPrepaidCells::decode(&bytes, LIMITS).unwrap();
        prop_assert_eq!(decoded.cells(), originals.as_slice());
        let mut remaining = originals.clone();
        let mut consumed = Vec::new();
        for count in counts {
            if remaining.is_empty() { break; }
            let bytes = OrderedPrepaidCells::encode(&remaining, LIMITS).unwrap();
            let decoded = OrderedPrepaidCells::decode(&bytes, LIMITS).unwrap();
            let result = decoded.split_consumed(count);
            prop_assert_eq!(result.is_ok(), count > 0 && count <= remaining.len());
            if let Ok((used, tail)) = result {
                prop_assert_eq!(used, &remaining[..count]);
                prop_assert_eq!(tail, &remaining[count..]);
                consumed.extend(remaining.drain(..count));
            }
            let complete = consumed.iter().chain(&remaining).copied().collect::<Vec<_>>();
            prop_assert_eq!(&complete, &originals);
        }
    }
}
