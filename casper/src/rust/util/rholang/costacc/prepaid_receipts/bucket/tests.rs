use proptest::prelude::*;

use super::*;

const LIMITS: PrepaidReceiptBucketLimits = PrepaidReceiptBucketLimits {
    occurrences: 128,
    wire: PhloWireLimits {
        total_bytes: 32_768,
        field_bytes: 4096,
    },
};

#[test]
fn bucket_canonicalization_preserves_duplicate_provenance_and_stable_source() {
    let mut bucket =
        PrepaidReceiptBucket::new([7; 32], &[b"second", b"first", b"second"], LIMITS).unwrap();
    assert_eq!(bucket.receipts(), &[
        b"first".as_slice(),
        b"second",
        b"second"
    ]);
    let key = bucket.storage_key();
    assert_ne!(key, [7; 32]);
    assert_ne!(key, PrepaidReceiptBucket::key_for_source(&[8; 32]));
    assert_eq!(bucket.remove(0).unwrap(), b"first");
    assert_eq!(bucket.receipts(), &[b"second".as_slice(), b"second"]);
    assert_eq!(bucket.storage_key(), key);
    bucket.check_occurrences(&[7; 32], 2).unwrap();
    assert!(bucket.check_occurrences(&[8; 32], 2).is_err());
    assert!(bucket.check_occurrences(&[7; 32], 1).is_err());
    let before = bucket.clone();
    assert!(bucket.remove(2).is_err());
    assert_eq!(bucket, before);
    let bytes = bucket.encode(LIMITS).unwrap();
    assert_eq!(
        PrepaidReceiptBucket::decode(&bytes, LIMITS).unwrap(),
        bucket
    );
}

#[test]
fn decoder_rejects_every_truncation_trailing_bytes_and_noncanonical_fields() {
    let bucket = PrepaidReceiptBucket::new([7; 32], &[b"a", b"b"], LIMITS).unwrap();
    let bytes = bucket.encode(LIMITS).unwrap();
    for end in 0..bytes.len() {
        assert!(PrepaidReceiptBucket::decode(&bytes[..end], LIMITS).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(PrepaidReceiptBucket::decode(&trailing, LIMITS).is_err());
    for (domain, source, count, receipts) in [
        (b"wrong".as_slice(), vec![7; 32], 2, vec![
            b"a".as_slice(),
            b"b",
        ]),
        (BUCKET_DOMAIN, vec![7; 31], 2, vec![b"a".as_slice(), b"b"]),
        (BUCKET_DOMAIN, vec![7; 32], 3, vec![b"a".as_slice(), b"b"]),
        (BUCKET_DOMAIN, vec![7; 32], u64::MAX, vec![
            b"a".as_slice(),
            b"b",
        ]),
        (BUCKET_DOMAIN, vec![7; 32], 2, vec![b"b".as_slice(), b"a"]),
        (BUCKET_DOMAIN, vec![7; 32], 2, vec![b"".as_slice(), b"b"]),
    ] {
        let mut encoded = PhloWireEncoder::new(LIMITS.wire);
        encoded.bytes(domain).unwrap();
        encoded.bytes(&source).unwrap();
        encoded.u64(count).unwrap();
        for receipt in receipts {
            encoded.bytes(receipt).unwrap();
        }
        assert!(PrepaidReceiptBucket::decode(encoded.as_bytes(), LIMITS).is_err());
    }
}

#[test]
fn constructor_and_decoder_enforce_independent_count_field_and_total_limits() {
    let receipts = [b"first".as_slice(), b"second"];
    let bucket = PrepaidReceiptBucket::new([7; 32], &receipts, LIMITS).unwrap();
    let bytes = bucket.encode(LIMITS).unwrap();
    let exact = PrepaidReceiptBucketLimits {
        occurrences: 2,
        wire: PhloWireLimits {
            total_bytes: bytes.len(),
            field_bytes: BUCKET_DOMAIN.len().max(32),
        },
    };
    assert_eq!(bucket.encode(exact).unwrap(), bytes);
    assert_eq!(PrepaidReceiptBucket::decode(&bytes, exact).unwrap(), bucket);
    for limit in [
        PrepaidReceiptBucketLimits {
            occurrences: 1,
            ..exact
        },
        PrepaidReceiptBucketLimits {
            wire: PhloWireLimits {
                total_bytes: bytes.len() - 1,
                ..exact.wire
            },
            ..exact
        },
        PrepaidReceiptBucketLimits {
            wire: PhloWireLimits {
                field_bytes: 31,
                ..exact.wire
            },
            ..exact
        },
    ] {
        assert!(PrepaidReceiptBucket::new([7; 32], &receipts, limit).is_err());
        assert!(bucket.encode(limit).is_err());
        assert!(PrepaidReceiptBucket::decode(&bytes, limit).is_err());
    }
    assert!(PrepaidReceiptBucket::new([7; 32], &[b""], LIMITS).is_err());
    let empty = PrepaidReceiptBucket::new([7; 32], &[], LIMITS).unwrap();
    let encoded = empty.encode(LIMITS).unwrap();
    assert!(PrepaidReceiptBucket::decode(&encoded, LIMITS)
        .unwrap()
        .receipts()
        .is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn arbitrary_removal_histories_preserve_complete_provenance(
        source in prop::array::uniform32(any::<u8>()),
        input in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..33), 0..65),
        choices in prop::collection::vec(any::<usize>(), 0..96),
    ) {
        let references: Vec<_> = input.iter().map(Vec::as_slice).collect();
        let mut bucket = PrepaidReceiptBucket::new(source, &references, LIMITS).unwrap();
        let mut oracle = input.clone(); oracle.sort();
        let original = oracle.clone();
        let mut removed = Vec::new();
        let key = bucket.storage_key();
        let reverse: Vec<_> = references.iter().rev().copied().collect();
        prop_assert_eq!(bucket.encode(LIMITS).unwrap(), PrepaidReceiptBucket::new(source, &reverse, LIMITS).unwrap().encode(LIMITS).unwrap());
        for choice in choices {
            let index = choice % (oracle.len() + 1);
            let before = bucket.encode(LIMITS).unwrap();
            if index == oracle.len() {
                prop_assert!(bucket.remove(index).is_err());
                prop_assert_eq!(bucket.encode(LIMITS).unwrap(), before);
            } else {
                let expected = oracle.remove(index);
                prop_assert_eq!(bucket.remove(index).unwrap(), expected.as_slice());
                removed.push(expected);
            }
            prop_assert_eq!(bucket.storage_key(), key);
            bucket.check_occurrences(&source, oracle.len()).unwrap();
            let bytes = bucket.encode(LIMITS).unwrap();
            let decoded = PrepaidReceiptBucket::decode(&bytes, LIMITS).unwrap();
            prop_assert_eq!(decoded.receipts(), oracle.iter().map(Vec::as_slice).collect::<Vec<_>>());
            let mut conserved = removed.clone(); conserved.extend(oracle.iter().cloned()); conserved.sort();
            prop_assert_eq!(&conserved, &original);
        }
    }
}
