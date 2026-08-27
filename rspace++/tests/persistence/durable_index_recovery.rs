use super::support::synthetic_ref;
use crate::rspace::persistence::durable::{CanonicalIndexStreamV1, ObjectIndexPrefixVerifierV1};
use crate::rspace::persistence::{
    INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexFormatError, IndexGenerationV1, LogPackSequence,
    ObjectIndexHeaderV1, ObjectIndexRecordV1, ObjectLocationV1, PackId, StoreId,
};

fn record(seed: u8, pack: u64, offset: u64, sequence: u64) -> ObjectIndexRecordV1 {
    ObjectIndexRecordV1::new(
        synthetic_ref(seed),
        ObjectLocationV1::new(
            PackId::new(pack).unwrap(),
            offset,
            LogPackSequence::new(sequence).unwrap(),
        )
        .unwrap(),
    )
    .unwrap()
}

fn rebuild(header: ObjectIndexHeaderV1, records: &[ObjectIndexRecordV1]) -> (Vec<u8>, u64) {
    let mut stream = CanonicalIndexStreamV1::new(header);
    let mut bytes = stream.header_image().to_vec();
    for record in records {
        let image = stream.next_record(*record).unwrap();
        assert_eq!(image.offset(), bytes.len() as u64);
        bytes.extend_from_slice(image.bytes());
    }
    (bytes, stream.durable_tail())
}

#[test]
fn wal_derived_stream_rebuilds_a_byte_identical_index_without_collecting_records() {
    let header = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x71; 16]),
        IndexGenerationV1::new(9).unwrap(),
    );
    let records = [record(0x11, 4, 4096, 17), record(0x22, 4, 8192, 18), record(0x33, 5, 1024, 19)];
    let (first, first_tail) = rebuild(header, &records);
    let (second, second_tail) = rebuild(header, &records);
    assert_eq!(first, second);
    assert_eq!(first_tail, second_tail);
    assert_eq!(first_tail, first.len() as u64);
    assert_eq!(first_tail, INDEX_HEADER_LEN_V1 + records.len() as u64 * INDEX_RECORD_LEN_V1);
}

#[test]
fn exact_prefix_verifier_binds_every_physical_record_to_wal_order_and_catalog_tail() {
    let header = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x72; 16]),
        IndexGenerationV1::new(10).unwrap(),
    );
    let records = [record(0x41, 6, 2048, 31), record(0x42, 6, 4096, 32), record(0x43, 7, 1024, 33)];
    let (mut bytes, durable_tail) = rebuild(header, &records);
    bytes.extend_from_slice(&[0xa5; 19]);

    let mut unverified =
        ObjectIndexPrefixVerifierV1::new(header, durable_tail, bytes.len() as u64).unwrap();
    assert_eq!(
        unverified.verify_next_record(records[0], &records[0].encode()),
        Err(IndexFormatError::IndexHeaderNotVerified)
    );

    let mut verifier =
        ObjectIndexPrefixVerifierV1::new(header, durable_tail, bytes.len() as u64).unwrap();
    verifier
        .verify_header(&bytes[..INDEX_HEADER_LEN_V1 as usize])
        .unwrap();
    for (position, expected) in records.iter().enumerate() {
        let start = INDEX_HEADER_LEN_V1 as usize + position * INDEX_RECORD_LEN_V1 as usize;
        verifier
            .verify_next_record(*expected, &bytes[start..start + INDEX_RECORD_LEN_V1 as usize])
            .unwrap();
    }
    verifier.finish().unwrap();
}

#[test]
fn missing_corrupt_reordered_and_wrong_generation_accelerators_require_rebuild() {
    let store = StoreId::from_array([0x73; 16]);
    let generation = IndexGenerationV1::new(11).unwrap();
    let header = ObjectIndexHeaderV1::new(store, generation);
    let records = [record(0x51, 8, 2048, 41), record(0x52, 8, 4096, 42)];
    let (bytes, durable_tail) = rebuild(header, &records);

    assert_eq!(
        ObjectIndexPrefixVerifierV1::new(header, durable_tail, durable_tail - 1),
        Err(IndexFormatError::DurableTailBeyondFile {
            durable_tail,
            available: durable_tail - 1,
        })
    );

    let wrong_header = ObjectIndexHeaderV1::new(store, generation.successor().unwrap()).encode();
    let mut verifier =
        ObjectIndexPrefixVerifierV1::new(header, durable_tail, durable_tail).unwrap();
    assert!(matches!(
        verifier.verify_header(&wrong_header),
        Err(IndexFormatError::Generation { .. })
    ));

    let first_start = INDEX_HEADER_LEN_V1 as usize;
    let second_start = first_start + INDEX_RECORD_LEN_V1 as usize;
    let mut reordered =
        ObjectIndexPrefixVerifierV1::new(header, durable_tail, durable_tail).unwrap();
    reordered.verify_header(&header.encode()).unwrap();
    assert!(matches!(
        reordered.verify_next_record(
            records[0],
            &bytes[second_start..second_start + INDEX_RECORD_LEN_V1 as usize],
        ),
        Err(IndexFormatError::RecordMismatch { offset, .. }) if offset == INDEX_HEADER_LEN_V1
    ));

    let mut damaged = bytes[first_start..second_start].to_vec();
    damaged[0] ^= 1;
    let mut corrupt = ObjectIndexPrefixVerifierV1::new(header, durable_tail, durable_tail).unwrap();
    corrupt.verify_header(&header.encode()).unwrap();
    assert!(matches!(
        corrupt.verify_next_record(records[0], &damaged),
        Err(IndexFormatError::Checksum { .. })
    ));

    let (repaired, repaired_tail) = rebuild(header, &records);
    assert_eq!(repaired, bytes);
    assert_eq!(repaired_tail, durable_tail);
}

#[test]
fn wal_and_catalog_record_counts_must_end_at_the_same_exact_tail() {
    let header = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x74; 16]),
        IndexGenerationV1::new(12).unwrap(),
    );
    let first = record(0x61, 9, 2048, 51);
    let second = record(0x62, 9, 4096, 52);

    let one_record_tail = INDEX_HEADER_LEN_V1 + INDEX_RECORD_LEN_V1;
    let mut short =
        ObjectIndexPrefixVerifierV1::new(header, one_record_tail, one_record_tail).unwrap();
    short.verify_header(&header.encode()).unwrap();
    short.verify_next_record(first, &first.encode()).unwrap();
    assert!(matches!(
        short.verify_next_record(second, &second.encode()),
        Err(IndexFormatError::DerivedRecordBeyondAppointedTail {
            record_offset,
            appointed_tail,
        }) if record_offset == one_record_tail && appointed_tail == one_record_tail
    ));

    let two_record_tail = one_record_tail + INDEX_RECORD_LEN_V1;
    let mut long =
        ObjectIndexPrefixVerifierV1::new(header, two_record_tail, two_record_tail).unwrap();
    long.verify_header(&header.encode()).unwrap();
    long.verify_next_record(first, &first.encode()).unwrap();
    assert_eq!(
        long.finish(),
        Err(IndexFormatError::CanonicalStreamTail {
            expected: one_record_tail,
            appointed: two_record_tail,
        })
    );
}
