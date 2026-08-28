use std::convert::Infallible;

use super::support::{TestSemanticProfile, synthetic_ref};
use super::transaction_sealing::{base_state, data_batch, seal_data};
use crate::rspace::persistence::durable::{CommittedObjectIndexV1, PreparedIndexAppendV1};
use crate::rspace::persistence::{
    CanonicalObjectStore, CommittedObjectComparison, CommittedObjectLookup, INDEX_HEADER_LEN_V1,
    INDEX_RECORD_LEN_V1, IndexFormatError, IndexGenerationV1, LogPackDecodeLimits,
    LogPackFormatError, LogPackObjectPlanV1, LogPackSequence, ObjectDigest, ObjectIndexFileViewV1,
    ObjectIndexHeaderV1, ObjectIndexRecordV1, ObjectLocationV1, ObjectRefV1, PACK_HEADER_LEN_V1,
    PackId, PreparedLogPackTransactionV1, StoreId,
};

struct MissingLookup;

impl CommittedObjectLookup for MissingLookup {
    type Error = Infallible;

    fn compare_committed_object(
        &self,
        _reference: ObjectRefV1,
        _sealed_canonical_bytes: &[u8],
    ) -> Result<CommittedObjectComparison, Self::Error> {
        Ok(CommittedObjectComparison::Missing)
    }
}

struct EqualOnly(ObjectRefV1);

impl CommittedObjectLookup for EqualOnly {
    type Error = Infallible;

    fn compare_committed_object(
        &self,
        reference: ObjectRefV1,
        _sealed_canonical_bytes: &[u8],
    ) -> Result<CommittedObjectComparison, Self::Error> {
        Ok(if reference == self.0 {
            CommittedObjectComparison::Equal
        } else {
            CommittedObjectComparison::Missing
        })
    }
}

fn rewrite_header_u64(bytes: &mut [u8; INDEX_HEADER_LEN_V1 as usize], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    let checksum = crate::rspace::persistence::checksum::crc32c(&bytes[..56]);
    bytes[56..].copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(checksum));
}

fn rewrite_record_u64(bytes: &mut [u8; INDEX_RECORD_LEN_V1 as usize], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    let checksum = crate::rspace::persistence::checksum::crc32c(&bytes[..64]);
    bytes[64..].copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(checksum));
}

fn record() -> ObjectIndexRecordV1 {
    ObjectIndexRecordV1::new(
        synthetic_ref(0x5a),
        ObjectLocationV1::new(PackId::new(3).unwrap(), 4096, LogPackSequence::new(7).unwrap())
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn index_preflight_matches_insert_errors_without_publishing_state() {
    let mut index = CommittedObjectIndexV1::new();
    let candidate = record();
    index.validate_insert(candidate).unwrap();
    assert_eq!(index.record_count(), 0);
    assert_eq!(index.get(candidate.reference().digest()), None);

    index.insert(candidate).unwrap();
    let before = index.record_count();
    assert_eq!(
        index.validate_insert(candidate),
        Err(IndexFormatError::DuplicateData(candidate.reference()))
    );
    assert_eq!(index.record_count(), before);
    assert_eq!(index.get(candidate.reference().digest()), Some(candidate));

    let conflicting_reference =
        ObjectRefV1::new(candidate.reference().digest(), candidate.reference().encoded_len() + 1)
            .unwrap();
    let conflict = ObjectIndexRecordV1::new(conflicting_reference, candidate.location()).unwrap();
    assert!(matches!(
        index.validate_insert(conflict),
        Err(IndexFormatError::DigestReferenceConflict { .. })
    ));
    assert_eq!(index.record_count(), before);
    assert_eq!(index.get(candidate.reference().digest()), Some(candidate));
}

#[test]
fn index_header_and_record_round_trip_with_frozen_portable_images() {
    let header = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x11; 16]),
        IndexGenerationV1::new(4).unwrap(),
    );
    let header_bytes = header.encode();
    assert_eq!(ObjectIndexHeaderV1::parse_exact(&header_bytes).unwrap(), header);
    assert_eq!(
        hex::encode(header_bytes),
        "525370616365496e6465782e5631000001000000000000004000000000000000111111111111111111111111111111110400000000000000a6210eba00000000"
    );

    let record = record();
    let record_bytes = record.encode();
    assert_eq!(ObjectIndexRecordV1::parse_exact(&record_bytes).unwrap(), record);
    assert_eq!(
        hex::encode(record_bytes),
        "5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a3200000000000000030000000000000000100000000000000700000000000000a769de2800000000"
    );
}

#[test]
fn every_index_header_and_record_truncation_fails_closed() {
    let header =
        ObjectIndexHeaderV1::new(StoreId::from_array([0x22; 16]), IndexGenerationV1::FIRST)
            .encode();
    for length in 0..header.len() {
        assert!(ObjectIndexHeaderV1::parse_exact(&header[..length]).is_err());
    }

    let record = record().encode();
    for length in 0..record.len() {
        assert!(ObjectIndexRecordV1::parse_exact(&record[..length]).is_err());
    }
}

#[test]
fn index_checksums_widths_and_valid_checksum_field_mutants_fail_closed() {
    let header = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x33; 16]),
        IndexGenerationV1::new(8).unwrap(),
    )
    .encode();
    let mut corrupt_header = header;
    corrupt_header[32] ^= 1;
    assert!(matches!(
        ObjectIndexHeaderV1::parse_exact(&corrupt_header),
        Err(IndexFormatError::Checksum { .. })
    ));
    let mut wide_header = header;
    wide_header[63] = 1;
    assert!(matches!(
        ObjectIndexHeaderV1::parse_exact(&wide_header),
        Err(IndexFormatError::ChecksumOutOfRange { .. })
    ));

    let mut domain = header;
    domain[0] ^= 1;
    let checksum = crate::rspace::persistence::checksum::crc32c(&domain[..56]);
    domain[56..].copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(checksum));
    assert_eq!(ObjectIndexHeaderV1::parse_exact(&domain), Err(IndexFormatError::InvalidDomain));

    let mut version = header;
    rewrite_header_u64(&mut version, 16, 2);
    assert_eq!(
        ObjectIndexHeaderV1::parse_exact(&version),
        Err(IndexFormatError::UnsupportedVersion(2))
    );
    let mut length = header;
    rewrite_header_u64(&mut length, 24, 63);
    assert_eq!(
        ObjectIndexHeaderV1::parse_exact(&length),
        Err(IndexFormatError::DeclaredHeaderLength(63))
    );
    let mut generation = header;
    rewrite_header_u64(&mut generation, 48, 0);
    assert_eq!(
        ObjectIndexHeaderV1::parse_exact(&generation),
        Err(IndexFormatError::ZeroGeneration)
    );

    let record = record().encode();
    let mut corrupt_record = record;
    corrupt_record[0] ^= 1;
    assert!(matches!(
        ObjectIndexRecordV1::parse_exact(&corrupt_record),
        Err(IndexFormatError::Checksum { .. })
    ));
    let mut wide_record = record;
    wide_record[71] = 1;
    assert!(matches!(
        ObjectIndexRecordV1::parse_exact(&wide_record),
        Err(IndexFormatError::ChecksumOutOfRange { .. })
    ));
    let mut pack_zero = record;
    rewrite_record_u64(&mut pack_zero, 40, 0);
    assert!(matches!(
        ObjectIndexRecordV1::parse_exact(&pack_zero),
        Err(IndexFormatError::LogPack(LogPackFormatError::ZeroPackId))
    ));
    let mut sequence_zero = record;
    rewrite_record_u64(&mut sequence_zero, 56, 0);
    assert!(matches!(
        ObjectIndexRecordV1::parse_exact(&sequence_zero),
        Err(IndexFormatError::LogPack(LogPackFormatError::ZeroSequence))
    ));
    let mut inside_header = record;
    rewrite_record_u64(&mut inside_header, 48, PACK_HEADER_LEN_V1 - 1);
    assert_eq!(
        ObjectIndexRecordV1::parse_exact(&inside_header),
        Err(IndexFormatError::CanonicalOffsetBeforePackHeader(PACK_HEADER_LEN_V1 - 1))
    );
}

#[test]
fn prepared_data_layouts_create_exact_locations_while_reuse_creates_none() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 63, None);
    let mutation = data_batch(base, "index-prepared-layout");
    let sealed = seal_data(&store, &mutation);
    let reused = sealed.objects()[0].reference();
    let prepare_offset = 8192;
    let pack_id = PackId::new(6).unwrap();
    let sequence = LogPackSequence::new(12).unwrap();
    let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, &EqualOnly(reused)).unwrap(),
        StoreId::from_array([0x44; 16]),
        sequence,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();

    let mut inline = 0;
    for layout in prepared.object_layouts() {
        match layout.canonical_offset() {
            Some(relative) => {
                let record =
                    ObjectIndexRecordV1::from_inline(pack_id, prepare_offset, sequence, layout)
                        .unwrap();
                assert_eq!(
                    ObjectIndexRecordV1::from_streamed_data(
                        pack_id,
                        prepare_offset,
                        sequence,
                        layout.reference(),
                        relative,
                    )
                    .unwrap(),
                    record,
                );
                assert_eq!(record.reference(), layout.reference());
                assert_eq!(record.location().canonical_offset(), prepare_offset + relative);
                assert_eq!(record.location().pack_id(), pack_id);
                assert_eq!(record.location().sequence(), sequence);
                inline += 1;
            }
            None => assert!(matches!(
                ObjectIndexRecordV1::from_inline(pack_id, prepare_offset, sequence, layout),
                Err(IndexFormatError::ReuseHasNoIndexLocation(reference)) if reference == reused
            )),
        }
    }
    assert_eq!(inline, prepared.inline_count());

    let cold = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, &MissingLookup).unwrap(),
        StoreId::from_array([0x44; 16]),
        sequence,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let layout = cold.object_layouts().next().unwrap();
    assert!(matches!(
        ObjectIndexRecordV1::from_inline(pack_id, u64::MAX, sequence, layout),
        Err(IndexFormatError::AbsoluteOffsetOverflow { .. })
    ));
    assert!(matches!(
        ObjectIndexRecordV1::from_streamed_data(
            pack_id,
            u64::MAX,
            sequence,
            layout.reference(),
            layout.canonical_offset().unwrap(),
        ),
        Err(IndexFormatError::AbsoluteOffsetOverflow { .. })
    ));

    let header = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x44; 16]),
        IndexGenerationV1::new(2).unwrap(),
    );
    let warm_append =
        PreparedIndexAppendV1::new(header, INDEX_HEADER_LEN_V1, pack_id, 128, &prepared).unwrap();
    assert_eq!(
        warm_append.new_tail() - warm_append.old_tail(),
        warm_append.record_count() * INDEX_RECORD_LEN_V1
    );
    assert_eq!(
        warm_append
            .records()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .len() as u64,
        warm_append.record_count()
    );
    let cold_append =
        PreparedIndexAppendV1::new(header, INDEX_HEADER_LEN_V1, pack_id, 128, &cold).unwrap();
    assert_eq!(cold_append.new_tail() - warm_append.new_tail(), INDEX_RECORD_LEN_V1);

    let wrong_store = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x45; 16]),
        IndexGenerationV1::new(2).unwrap(),
    );
    assert!(matches!(
        PreparedIndexAppendV1::new(
            wrong_store,
            INDEX_HEADER_LEN_V1,
            pack_id,
            PACK_HEADER_LEN_V1,
            &prepared,
        ),
        Err(IndexFormatError::PrepareStore { .. })
    ));
    assert!(matches!(
        PreparedIndexAppendV1::new(
            header,
            INDEX_HEADER_LEN_V1 + 1,
            pack_id,
            PACK_HEADER_LEN_V1,
            &prepared,
        ),
        Err(IndexFormatError::UnalignedDurableTail(_))
    ));
    assert!(matches!(
        PreparedIndexAppendV1::new(
            header,
            INDEX_HEADER_LEN_V1,
            pack_id,
            PACK_HEADER_LEN_V1 - 1,
            &prepared,
        ),
        Err(IndexFormatError::PrepareOffsetBeforePackHeader(_))
    ));
    assert!(matches!(
        PreparedIndexAppendV1::new(header, INDEX_HEADER_LEN_V1, pack_id, u64::MAX, &prepared,),
        Err(IndexFormatError::PrepareEndOverflow { .. })
    ));
    let largest_aligned_tail = u64::MAX - (u64::MAX - INDEX_HEADER_LEN_V1) % INDEX_RECORD_LEN_V1;
    assert!(matches!(
        PreparedIndexAppendV1::new(
            header,
            largest_aligned_tail,
            pack_id,
            PACK_HEADER_LEN_V1,
            &prepared,
        ),
        Err(IndexFormatError::IndexTailOverflow { .. })
    ));
}

#[test]
fn durable_index_prefix_is_exactly_aligned_and_ignores_only_catalog_uncommitted_tail() {
    let header = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x55; 16]),
        IndexGenerationV1::new(2).unwrap(),
    );
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header.encode());
    bytes.extend_from_slice(&record().encode());
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    let durable_tail = INDEX_HEADER_LEN_V1 + INDEX_RECORD_LEN_V1;
    let view = ObjectIndexFileViewV1::parse_durable_prefix(&bytes, durable_tail).unwrap();
    assert_eq!(view.header(), header);
    assert_eq!(view.record_count(), 1);
    assert_eq!(view.ignored_tail_bytes(), 3);
    assert_eq!(view.records().next().unwrap().unwrap(), record());

    assert!(matches!(
        ObjectIndexFileViewV1::parse_durable_prefix(&bytes, durable_tail + 1),
        Err(IndexFormatError::UnalignedDurableTail(_))
    ));
    assert!(matches!(
        ObjectIndexFileViewV1::parse_durable_prefix(&bytes, bytes.len() as u64 + 1),
        Err(IndexFormatError::DurableTailBeyondFile { .. })
    ));

    let mut corrupt_record = bytes;
    corrupt_record[INDEX_HEADER_LEN_V1 as usize] ^= 1;
    let view = ObjectIndexFileViewV1::parse_durable_prefix(&corrupt_record, durable_tail).unwrap();
    assert!(matches!(view.records().next().unwrap(), Err(IndexFormatError::Checksum { .. })));
}

#[test]
fn index_identity_counters_and_object_extent_overflow_are_checked() {
    let header = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x66; 16]),
        IndexGenerationV1::new(3).unwrap(),
    );
    assert!(matches!(
        header.verify_identity(StoreId::from_array([0x67; 16]), IndexGenerationV1::new(3).unwrap()),
        Err(IndexFormatError::Store { .. })
    ));
    assert!(matches!(
        header.verify_identity(header.store_id(), IndexGenerationV1::new(4).unwrap()),
        Err(IndexFormatError::Generation { .. })
    ));
    assert_eq!(
        IndexGenerationV1::new(u64::MAX).unwrap().successor(),
        Err(IndexFormatError::GenerationOverflow(u64::MAX))
    );

    let location =
        ObjectLocationV1::new(PackId::new(1).unwrap(), u64::MAX - 10, LogPackSequence::FIRST)
            .unwrap();
    assert!(matches!(
        ObjectIndexRecordV1::new(synthetic_ref(1), location),
        Err(IndexFormatError::ObjectEndOverflow { .. })
    ));
}

#[test]
fn committed_index_is_a_cow_pathmap_and_rejects_every_second_data_record() {
    let first = record();
    let mut index = CommittedObjectIndexV1::new();
    index.insert(first).unwrap();
    assert_eq!(index.record_count(), 1);
    assert_eq!(index.get(first.reference().digest()), Some(first));

    let cloned = index.clone();
    assert_eq!(cloned.get(first.reference().digest()), Some(first));
    assert_eq!(index.insert(first), Err(IndexFormatError::DuplicateData(first.reference())));
    assert_eq!(index.record_count(), 1);

    let conflicting_reference = ObjectRefV1::new(
        ObjectDigest::from_array(*first.reference().digest().as_array()),
        first.reference().encoded_len() + 1,
    )
    .unwrap();
    let conflicting = ObjectIndexRecordV1::new(conflicting_reference, first.location()).unwrap();
    assert!(matches!(
        index.insert(conflicting),
        Err(IndexFormatError::DigestReferenceConflict {
            digest,
            existing,
            candidate,
        }) if digest == first.reference().digest()
            && existing == first.reference()
            && candidate == conflicting_reference
    ));
    assert_eq!(index.record_count(), 1);
}
