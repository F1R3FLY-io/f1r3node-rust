use std::convert::Infallible;

use super::support::{TestSemanticProfile, synthetic_ref};
use super::transaction_sealing::{base_state, data_batch, seal_data};
use crate::rspace::persistence::{
    CanonicalFormatError, CanonicalObjectStore, CanonicalSink, CommitPublicationV1,
    CommittedObjectComparison, CommittedObjectLookup, LogPackAddress, LogPackDecodeLimits,
    LogPackFormatError, LogPackObjectDispositionV1, LogPackObjectHeaderV1, LogPackObjectPlanV1,
    LogPackSequence, LogPackWriteError, OBJECT_RECORD_HEADER_LEN_V1, ObjectRefV1,
    PACK_HEADER_LEN_V1, PREPARE_DISCRIMINATOR_LEN_V1, PhysicalDigest, PrepareRecordKindV1,
    PreparedLogPackTransactionV1, StoreId, TRANSACTION_PREPARE_HEADER_LEN_V1,
    TransactionCommitRecordV1, TransactionCommitViewV1, TransactionPrepareRecordViewV1,
    decode_prepare_kind_v1, preflight_prepare_record_len_v1,
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

struct CollisionLookup;

impl CommittedObjectLookup for CollisionLookup {
    type Error = Infallible;

    fn compare_committed_object(
        &self,
        _reference: ObjectRefV1,
        _sealed_canonical_bytes: &[u8],
    ) -> Result<CommittedObjectComparison, Self::Error> {
        Ok(CommittedObjectComparison::Collision)
    }
}

fn publication(durable_index_tail: u64) -> CommitPublicationV1 {
    CommitPublicationV1::new(1, 3, durable_index_tail, synthetic_ref(0xe0), 2048, 4096).unwrap()
}

fn encoded_prepare() -> (Vec<u8>, crate::rspace::persistence::SealedTransitionV1, StoreId) {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 41, None);
    let mutation = data_batch(base, "WAL-round-trip");
    let sealed = seal_data(&store, &mutation);
    let receipt = sealed.receipt();
    let store_id = StoreId::from_array([0x5a; 16]);
    let sequence = LogPackSequence::new(7).unwrap();
    let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, &MissingLookup).unwrap(),
        store_id,
        sequence,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let mut bytes = Vec::new();
    let identity = prepared.write_to(&mut bytes).unwrap();
    assert_eq!(bytes.len() as u64, prepared.record_len());
    assert_eq!(identity.record_len(), prepared.record_len());
    (bytes, receipt, store_id)
}

fn repair_prepare_record_crc(bytes: &mut [u8]) {
    let payload_end = bytes.len() - 8;
    let crc = crate::rspace::persistence::checksum::crc32c(&bytes[..payload_end]);
    bytes[payload_end..].copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(crc));
}

#[test]
fn prepare_stream_round_trips_without_projecting_objects() {
    let (bytes, receipt, store_id) = encoded_prepare();
    let view = TransactionPrepareRecordViewV1::parse(&bytes, LogPackDecodeLimits::UNBOUNDED)
        .expect("the streamed prepare record validates");
    assert_eq!(view.sequence(), LogPackSequence::new(7).unwrap());
    assert_eq!(view.store_id(), store_id);
    assert_eq!(view.receipt(), receipt);
    assert_eq!(view.mutation().payload_digest(), receipt.mutation_digest());
    assert!(view.objects().len() >= 2);
    let mut previous = None;
    let mut next_record_offset = view.objects_offset();
    for object in view.objects() {
        if let Some(previous) = previous {
            assert!(previous < object.reference().digest());
        }
        assert_eq!(object.record_offset(), next_record_offset);
        let canonical_offset = object.canonical_offset().unwrap();
        assert_eq!(canonical_offset, object.record_offset() + 48);
        assert_eq!(
            &bytes[canonical_offset as usize..
                (canonical_offset + object.reference().encoded_len()) as usize],
            object.canonical_bytes().unwrap()
        );
        next_record_offset = canonical_offset + object.reference().encoded_len();
        previous = Some(object.reference().digest());
    }
    assert_eq!(next_record_offset + 8, view.record_len());
    assert_eq!(view.record_len(), bytes.len() as u64);
}

#[test]
fn fixed_object_header_parser_is_exact_and_matches_contiguous_errors() {
    let (bytes, _, _) = encoded_prepare();
    let view =
        TransactionPrepareRecordViewV1::parse(&bytes, LogPackDecodeLimits::UNBOUNDED).unwrap();
    let object = view.objects().next().unwrap();
    let start = object.record_offset() as usize;
    let end = start + OBJECT_RECORD_HEADER_LEN_V1 as usize;
    let header = LogPackObjectHeaderV1::parse_exact(&bytes[start..end]).unwrap();
    assert_eq!(header.disposition(), object.disposition());
    assert_eq!(header.reference(), object.reference());
    assert!(matches!(
        LogPackObjectHeaderV1::parse_exact(&bytes[start..end - 1]),
        Err(LogPackFormatError::LengthMismatch {
            declared: OBJECT_RECORD_HEADER_LEN_V1,
            actual,
        }) if actual == OBJECT_RECORD_HEADER_LEN_V1 - 1
    ));
    let mut extended = bytes[start..end].to_vec();
    extended.push(0);
    assert!(matches!(
        LogPackObjectHeaderV1::parse_exact(&extended),
        Err(LogPackFormatError::LengthMismatch {
            declared: OBJECT_RECORD_HEADER_LEN_V1,
            actual,
        }) if actual == OBJECT_RECORD_HEADER_LEN_V1 + 1
    ));

    let mut invalid_tag_header = bytes[start..end].to_vec();
    invalid_tag_header[..8].copy_from_slice(&3u64.to_le_bytes());
    let tag_error = LogPackObjectHeaderV1::parse_exact(&invalid_tag_header).unwrap_err();
    assert_eq!(tag_error, LogPackFormatError::UnsupportedObjectDisposition(3));
    let mut invalid_tag_record = bytes.clone();
    invalid_tag_record[start..end].copy_from_slice(&invalid_tag_header);
    repair_prepare_record_crc(&mut invalid_tag_record);
    assert_eq!(
        TransactionPrepareRecordViewV1::parse(&invalid_tag_record, LogPackDecodeLimits::UNBOUNDED,)
            .unwrap_err(),
        tag_error,
    );

    let mut invalid_reference_header = bytes[start..end].to_vec();
    invalid_reference_header[40..48].copy_from_slice(&0u64.to_le_bytes());
    let reference_error =
        LogPackObjectHeaderV1::parse_exact(&invalid_reference_header).unwrap_err();
    assert_eq!(
        reference_error,
        LogPackFormatError::Canonical(CanonicalFormatError::ObjectLengthTooSmall(0)),
    );
    let mut invalid_reference_record = bytes.clone();
    invalid_reference_record[start..end].copy_from_slice(&invalid_reference_header);
    repair_prepare_record_crc(&mut invalid_reference_record);
    assert_eq!(
        TransactionPrepareRecordViewV1::parse(
            &invalid_reference_record,
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap_err(),
        reference_error,
    );
    assert_eq!(object.disposition(), LogPackObjectDispositionV1::Data);
}

#[test]
fn prepare_prefix_is_authenticated_and_bounded_before_variable_size_reads() {
    let (bytes, _, _) = encoded_prepare();
    let prefix = &bytes[..TRANSACTION_PREPARE_HEADER_LEN_V1 as usize];
    assert_eq!(
        preflight_prepare_record_len_v1(prefix, LogPackDecodeLimits::UNBOUNDED).unwrap(),
        bytes.len() as u64
    );

    let mut corrupt = prefix.to_vec();
    corrupt[80] ^= 1;
    assert!(matches!(
        preflight_prepare_record_len_v1(&corrupt, LogPackDecodeLimits::UNBOUNDED),
        Err(LogPackFormatError::HeaderChecksum { .. })
    ));

    let mut oversized = prefix.to_vec();
    oversized[32..40].copy_from_slice(&(bytes.len() as u64 + 1).to_le_bytes());
    let header_crc = crate::rspace::persistence::checksum::crc32c(&oversized[..232]);
    oversized[232..240]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(header_crc));
    let limits = LogPackDecodeLimits {
        max_record_bytes: bytes.len() as u64,
        ..LogPackDecodeLimits::UNBOUNDED
    };
    assert!(matches!(
        preflight_prepare_record_len_v1(&oversized, limits),
        Err(LogPackFormatError::ResourceLimit {
            resource: "record bytes",
            ..
        })
    ));
}

#[test]
fn prepare_discriminator_routes_only_exact_known_fixed_headers() {
    let (bytes, _, _) = encoded_prepare();
    let discriminator_len = PREPARE_DISCRIMINATOR_LEN_V1 as usize;
    let discriminator = &bytes[..discriminator_len];
    let incremental = decode_prepare_kind_v1(discriminator).unwrap();
    assert_eq!(incremental, PrepareRecordKindV1::Incremental);
    assert_eq!(incremental.fixed_header_len(), TRANSACTION_PREPARE_HEADER_LEN_V1);

    let mut checkpoint = discriminator.to_vec();
    checkpoint[24..32].copy_from_slice(&3_u64.to_le_bytes());
    let checkpoint = decode_prepare_kind_v1(&checkpoint).unwrap();
    assert_eq!(checkpoint, PrepareRecordKindV1::GenerationCheckpoint);
    assert!(checkpoint.fixed_header_len() > TRANSACTION_PREPARE_HEADER_LEN_V1);

    let mut unsupported = discriminator.to_vec();
    unsupported[24..32].copy_from_slice(&99_u64.to_le_bytes());
    assert!(matches!(
        decode_prepare_kind_v1(&unsupported),
        Err(LogPackFormatError::UnsupportedPrepareKind(99))
    ));
    assert!(matches!(
        decode_prepare_kind_v1(&discriminator[..discriminator_len - 1]),
        Err(LogPackFormatError::LengthMismatch { .. })
    ));
    let mut extended = discriminator.to_vec();
    extended.push(0);
    assert!(matches!(
        decode_prepare_kind_v1(&extended),
        Err(LogPackFormatError::LengthMismatch { .. })
    ));
}

#[test]
fn incremental_parser_reports_checkpoint_kind_without_trusting_its_extents() {
    let (mut bytes, _, _) = encoded_prepare();
    bytes[24..32].copy_from_slice(&3_u64.to_le_bytes());
    let header_crc = crate::rspace::persistence::checksum::crc32c(&bytes[..232]);
    bytes[232..240]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(header_crc));
    assert!(matches!(
        TransactionPrepareRecordViewV1::parse(&bytes, LogPackDecodeLimits::UNBOUNDED),
        Err(LogPackFormatError::UnexpectedRecordKind {
            expected: 1,
            actual: 3,
        })
    ));
}

#[test]
fn prepared_layouts_are_the_single_source_for_parsed_object_offsets() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 56, None);
    let mutation = data_batch(base, "prepared-layouts");
    let sealed = seal_data(&store, &mutation);
    let reused = sealed.objects()[0].reference();
    let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, &EqualOnly(reused)).unwrap(),
        StoreId::from_array([0x62; 16]),
        LogPackSequence::new(10).unwrap(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let mut bytes = Vec::new();
    prepared.write_to(&mut bytes).unwrap();
    let parsed =
        TransactionPrepareRecordViewV1::parse(&bytes, LogPackDecodeLimits::UNBOUNDED).unwrap();
    let planned = prepared.object_layouts().collect::<Vec<_>>();
    let observed = parsed.objects().collect::<Vec<_>>();
    assert_eq!(planned.len(), observed.len());
    for (planned, observed) in planned.into_iter().zip(observed) {
        assert_eq!(planned.disposition(), observed.disposition());
        assert_eq!(planned.reference(), observed.reference());
        assert_eq!(planned.record_offset(), observed.record_offset());
        assert_eq!(planned.canonical_offset(), observed.canonical_offset());
    }
}

#[test]
fn warm_store_reuse_omits_only_the_equal_body_and_collision_prevents_preflight() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 55, None);
    let mutation = data_batch(base, "WAL-reuse");
    let sealed = seal_data(&store, &mutation);
    let reused = sealed.objects()[0].reference();
    let reused_len = reused.encoded_len();
    let store_id = StoreId::from_array([0x61; 16]);
    let sequence = LogPackSequence::new(9).unwrap();

    let cold = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, &MissingLookup).unwrap(),
        store_id,
        sequence,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let warm = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, &EqualOnly(reused)).unwrap(),
        store_id,
        sequence,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    assert_eq!(cold.record_len(), warm.record_len() + reused_len);

    let mut warm_bytes = Vec::new();
    warm.write_to(&mut warm_bytes).unwrap();
    let view =
        TransactionPrepareRecordViewV1::parse(&warm_bytes, LogPackDecodeLimits::UNBOUNDED).unwrap();
    let reused_record = view
        .objects()
        .find(|object| object.reference() == reused)
        .unwrap();
    assert_eq!(
        reused_record.disposition(),
        crate::rspace::persistence::LogPackObjectDispositionV1::Reuse
    );
    assert_eq!(reused_record.canonical_offset(), None);
    assert_eq!(reused_record.canonical_bytes(), None);

    assert!(matches!(
        LogPackObjectPlanV1::classify(&sealed, &CollisionLookup),
        Err(crate::rspace::persistence::LogPackObjectPlanError::Collision(reference))
            if reference == sealed.objects()[0].reference()
    ));
}

#[test]
fn single_object_limit_applies_to_reuse_references_as_well_as_inline_data() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 55, None);
    let mutation = data_batch(base, &"r".repeat(32_000));
    let sealed = seal_data(&store, &mutation);
    let largest = sealed
        .objects()
        .iter()
        .max_by_key(|object| object.reference().encoded_len())
        .unwrap()
        .reference();
    assert!(
        sealed
            .objects()
            .iter()
            .filter(|object| object.reference() != largest)
            .all(|object| object.reference().encoded_len() < largest.encoded_len()),
        "the fixture must isolate one oversized REUSE reference",
    );
    let plan = LogPackObjectPlanV1::classify(&sealed, &EqualOnly(largest)).unwrap();
    let limit = largest.encoded_len() - 1;
    let limits = LogPackDecodeLimits {
        max_single_object_bytes: limit,
        ..LogPackDecodeLimits::UNBOUNDED
    };
    assert!(matches!(
        PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
            &sealed,
            plan.clone(),
            StoreId::from_array([0x63; 16]),
            LogPackSequence::new(11).unwrap(),
            limits,
        ),
        Err(LogPackFormatError::ResourceLimit {
            resource: "single referenced object bytes",
            limit: observed_limit,
            attempted,
        }) if observed_limit == limit && attempted == largest.encoded_len()
    ));

    let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        plan,
        StoreId::from_array([0x63; 16]),
        LogPackSequence::new(11).unwrap(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let mut bytes = Vec::new();
    prepared.write_to(&mut bytes).unwrap();
    let reused = TransactionPrepareRecordViewV1::parse(&bytes, LogPackDecodeLimits::UNBOUNDED)
        .unwrap()
        .objects()
        .find(|object| object.reference() == largest)
        .unwrap();
    assert_eq!(reused.disposition(), crate::rspace::persistence::LogPackObjectDispositionV1::Reuse,);
    assert!(matches!(
        TransactionPrepareRecordViewV1::parse(&bytes, limits),
        Err(LogPackFormatError::ResourceLimit {
            resource: "single referenced object bytes",
            limit: observed_limit,
            attempted,
        }) if observed_limit == limit && attempted == largest.encoded_len()
    ));
}

#[test]
fn commit_binds_store_sequence_prepare_receipt_and_chain() {
    let (prepare_bytes, receipt, store_id) = encoded_prepare();
    let prepare =
        TransactionPrepareRecordViewV1::parse(&prepare_bytes, LogPackDecodeLimits::UNBOUNDED)
            .unwrap();
    let identity = crate::rspace::persistence::PrepareRecordIdentityV1::from_record_view(prepare);
    let address = LogPackAddress::new(3, 8192, prepare.record_len()).unwrap();
    let previous = PhysicalDigest::of_bytes(b"previous commit");
    let commit = TransactionCommitRecordV1::new(
        receipt,
        store_id,
        previous,
        address,
        identity,
        publication(4096),
    )
    .unwrap();
    let view = TransactionCommitViewV1::parse(commit.canonical_bytes()).unwrap();
    assert_eq!(view.digest(), commit.digest());
    assert_eq!(view.previous_commit(), previous);
    assert_eq!(view.prepare_address(), address);
    assert_eq!(view.durable_index_tail(), 4096);
    view.verify_prepare(prepare).unwrap();
}

#[test]
fn prepare_rejects_every_truncation_without_panicking() {
    let (bytes, _, _) = encoded_prepare();
    for length in 0..bytes.len() {
        assert!(
            TransactionPrepareRecordViewV1::parse(
                &bytes[..length],
                LogPackDecodeLimits::UNBOUNDED,
            )
                .is_err(),
            "truncation at {length} must fail",
        );
    }
}

#[test]
fn checksums_reject_header_and_payload_corruption() {
    let (bytes, _, _) = encoded_prepare();
    let mut header = bytes.clone();
    header[64] ^= 0x80;
    assert!(matches!(
        TransactionPrepareRecordViewV1::parse(&header, LogPackDecodeLimits::UNBOUNDED),
        Err(LogPackFormatError::HeaderChecksum { .. })
    ));

    let mut payload = bytes;
    payload[240] ^= 0x01;
    assert!(matches!(
        TransactionPrepareRecordViewV1::parse(&payload, LogPackDecodeLimits::UNBOUNDED),
        Err(LogPackFormatError::RecordChecksum { .. })
    ));
}

#[test]
fn persisted_checksums_reject_nonzero_high_u64_bits() {
    let (bytes, receipt, store_id) = encoded_prepare();
    let mut header = bytes.clone();
    header[236] = 1;
    assert!(matches!(
        TransactionPrepareRecordViewV1::parse(&header, LogPackDecodeLimits::UNBOUNDED),
        Err(LogPackFormatError::ChecksumOutOfRange {
            field: "prepare header",
            ..
        })
    ));

    let mut record = bytes.clone();
    *record.last_mut().unwrap() = 1;
    assert!(matches!(
        TransactionPrepareRecordViewV1::parse(&record, LogPackDecodeLimits::UNBOUNDED),
        Err(LogPackFormatError::ChecksumOutOfRange {
            field: "prepare record",
            ..
        })
    ));

    let prepare =
        TransactionPrepareRecordViewV1::parse(&bytes, LogPackDecodeLimits::UNBOUNDED).unwrap();
    let mut commit = *TransactionCommitRecordV1::new(
        receipt,
        store_id,
        PhysicalDigest::ZERO,
        LogPackAddress::new(1, 128, prepare.record_len()).unwrap(),
        crate::rspace::persistence::PrepareRecordIdentityV1::from_record_view(prepare),
        publication(64),
    )
    .unwrap()
    .canonical_bytes();
    *commit.last_mut().unwrap() = 1;
    assert!(matches!(
        TransactionCommitViewV1::parse(&commit),
        Err(LogPackFormatError::ChecksumOutOfRange {
            field: "commit record",
            ..
        })
    ));
}

#[test]
fn declared_resource_use_is_bounded_before_payload_parsing() {
    let (bytes, _, _) = encoded_prepare();
    let error = TransactionPrepareRecordViewV1::parse(&bytes, LogPackDecodeLimits {
        max_record_bytes: bytes.len() as u64,
        max_mutation_bytes: 1,
        ..LogPackDecodeLimits::UNBOUNDED
    })
    .unwrap_err();
    assert!(matches!(error, LogPackFormatError::ResourceLimit {
        resource: "mutation bytes",
        ..
    }));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InjectedFailure;

struct FailingSink {
    successful_calls_left: usize,
}

impl CanonicalSink for FailingSink {
    type Error = InjectedFailure;

    fn write_all(&mut self, _bytes: &[u8]) -> Result<(), Self::Error> {
        if self.successful_calls_left == 0 {
            return Err(InjectedFailure);
        }
        self.successful_calls_left -= 1;
        Ok(())
    }
}

#[test]
fn sink_failure_cannot_produce_a_prepare_identity() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 4, None);
    let mutation = data_batch(base, "short-write");
    let sealed = seal_data(&store, &mutation);
    let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, &MissingLookup).unwrap(),
        StoreId::from_array([9; 16]),
        LogPackSequence::new(1).unwrap(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let mut sink = FailingSink {
        successful_calls_left: 2,
    };
    assert_eq!(prepared.write_to(&mut sink), Err(LogPackWriteError::Sink(InjectedFailure)));
}

#[test]
fn commit_has_a_fixed_portable_image_and_rejects_corruption() {
    let (prepare_bytes, receipt, store_id) = encoded_prepare();
    let prepare =
        TransactionPrepareRecordViewV1::parse(&prepare_bytes, LogPackDecodeLimits::UNBOUNDED)
            .unwrap();
    let commit = TransactionCommitRecordV1::new(
        receipt,
        store_id,
        PhysicalDigest::ZERO,
        LogPackAddress::new(1, PACK_HEADER_LEN_V1, prepare.record_len()).unwrap(),
        crate::rspace::persistence::PrepareRecordIdentityV1::from_record_view(prepare),
        publication(64),
    )
    .unwrap();
    assert_eq!(commit.canonical_bytes().len(), 384);
    let mut corrupted = *commit.canonical_bytes();
    corrupted[200] ^= 1;
    assert!(matches!(
        TransactionCommitViewV1::parse(&corrupted),
        Err(LogPackFormatError::RecordChecksum { .. })
    ));

    let mut inside_header = *commit.canonical_bytes();
    replace_u64_and_commit_crc(&mut inside_header, 104, PACK_HEADER_LEN_V1 - 1);
    assert!(matches!(
        TransactionCommitViewV1::parse(&inside_header),
        Err(LogPackFormatError::AddressBeforePackHeader { .. })
    ));

    let mut streamed = Vec::new();
    let result: Result<(), Infallible> = commit.write_to(&mut streamed);
    result.unwrap();
    assert_eq!(streamed, commit.canonical_bytes());

    for length in 0..commit.canonical_bytes().len() {
        assert!(
            TransactionCommitViewV1::parse(&commit.canonical_bytes()[..length]).is_err(),
            "commit truncation at {length} must fail",
        );
    }
}

fn replace_u64_and_commit_crc(bytes: &mut [u8; 384], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    let crc = crate::rspace::persistence::checksum::crc32c(&bytes[..376]);
    bytes[376..].copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(crc));
}

#[test]
fn valid_commit_images_cannot_rebind_prepare_fields() {
    let (prepare_bytes, receipt, store_id) = encoded_prepare();
    let prepare =
        TransactionPrepareRecordViewV1::parse(&prepare_bytes, LogPackDecodeLimits::UNBOUNDED)
            .unwrap();
    let identity = crate::rspace::persistence::PrepareRecordIdentityV1::from_record_view(prepare);
    let address = LogPackAddress::new(3, 8192, prepare.record_len()).unwrap();
    let previous = PhysicalDigest::of_bytes(b"previous commit");
    let commit = TransactionCommitRecordV1::new(
        receipt,
        store_id,
        previous,
        address,
        identity,
        publication(4096),
    )
    .unwrap();

    let mut changed_sequence = *commit.canonical_bytes();
    replace_u64_and_commit_crc(&mut changed_sequence, 40, 8);
    assert!(matches!(
        TransactionCommitViewV1::parse(&changed_sequence)
            .unwrap()
            .verify_prepare(prepare),
        Err(LogPackFormatError::Sequence { .. })
    ));

    let mut changed_store = *commit.canonical_bytes();
    changed_store[48] ^= 1;
    let crc = crate::rspace::persistence::checksum::crc32c(&changed_store[..376]);
    changed_store[376..].copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(crc));
    assert!(matches!(
        TransactionCommitViewV1::parse(&changed_store)
            .unwrap()
            .verify_prepare(prepare),
        Err(LogPackFormatError::Store { .. })
    ));

    let mut changed_length = *commit.canonical_bytes();
    replace_u64_and_commit_crc(&mut changed_length, 112, prepare.record_len() + 1);
    assert!(matches!(
        TransactionCommitViewV1::parse(&changed_length)
            .unwrap()
            .verify_prepare(prepare),
        Err(LogPackFormatError::AddressLength { .. })
    ));

    let mut changed_digest = *commit.canonical_bytes();
    changed_digest[120] ^= 1;
    let crc = crate::rspace::persistence::checksum::crc32c(&changed_digest[..376]);
    changed_digest[376..]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(crc));
    assert!(matches!(
        TransactionCommitViewV1::parse(&changed_digest)
            .unwrap()
            .verify_prepare(prepare),
        Err(LogPackFormatError::RecordDigest { .. })
    ));

    let mut changed_receipt = *commit.canonical_bytes();
    replace_u64_and_commit_crc(&mut changed_receipt, 200, receipt.target().generation() + 1);
    assert!(matches!(
        TransactionCommitViewV1::parse(&changed_receipt)
            .unwrap()
            .verify_prepare(prepare),
        Err(LogPackFormatError::ReceiptMismatch)
    ));

    let mut changed_previous = *commit.canonical_bytes();
    changed_previous[64] ^= 1;
    let crc = crate::rspace::persistence::checksum::crc32c(&changed_previous[..376]);
    changed_previous[376..]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(crc));
    let changed_previous = TransactionCommitViewV1::parse(&changed_previous).unwrap();
    assert_ne!(changed_previous.previous_commit(), previous);
    assert_ne!(changed_previous.digest(), commit.digest());

    let mut changed_offset = *commit.canonical_bytes();
    replace_u64_and_commit_crc(&mut changed_offset, 104, 8193);
    let changed_offset = TransactionCommitViewV1::parse(&changed_offset).unwrap();
    assert_eq!(changed_offset.prepare_address().offset(), 8193);
    assert_ne!(changed_offset.digest(), commit.digest());
}

#[test]
fn canonical_v1_prepare_and_commit_images_are_frozen() {
    let (prepare_bytes, receipt, store_id) = encoded_prepare();
    let prepare =
        TransactionPrepareRecordViewV1::parse(&prepare_bytes, LogPackDecodeLimits::UNBOUNDED)
            .unwrap();
    let identity = crate::rspace::persistence::PrepareRecordIdentityV1::from_record_view(prepare);
    let commit = TransactionCommitRecordV1::new(
        receipt,
        store_id,
        PhysicalDigest::of_bytes(b"frozen previous commit"),
        LogPackAddress::new(7, 16_384, prepare.record_len()).unwrap(),
        identity,
        publication(32_752),
    )
    .unwrap();
    let fixture = format!(
        "len={};header={};digest={};crc={:08x};commit={}",
        prepare_bytes.len(),
        hex::encode(&prepare_bytes[..232]),
        prepare.digest(),
        prepare.crc32c(),
        hex::encode(commit.canonical_bytes()),
    );
    assert_eq!(
        fixture,
        "len=1123;header=52537061636557414c2e50726570000001000000000000000100000000000000630400000000000007000000000000005a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a290000000000000008a2bad5d01cd13ce6c39000aa50c93509172aebb94fed4f47858b8414404bcfd1000000000000002a0000000000000024b1f6c14483eeb76ebb2e72dcad8846267740bde2cc0ce364cef24b98131dd0d1000000000000005253706163652e546573742e56310000cd05aa8ec69624595dd5c844dbc40f03d891154be925157f4ce25c95c8d6cba458010000000000000200000000000000b301000000000000;digest=9b459e919000bfaa225df9b9bf521d79f4070aa98f8727af6b28b418714c842f;crc=e6202329;commit=52537061636557414c2e436f6d6d000001000000000000000200000000000000800100000000000007000000000000005a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a80f5dfe8cb9b5cec6d10570022f58c7d18aa77808b16ac31784bcdfee68ac6a20700000000000000004000000000000063040000000000009b459e919000bfaa225df9b9bf521d79f4070aa98f8727af6b28b418714c842f290000000000000008a2bad5d01cd13ce6c39000aa50c93509172aebb94fed4f47858b8414404bcfd1000000000000002a0000000000000024b1f6c14483eeb76ebb2e72dcad8846267740bde2cc0ce364cef24b98131dd0d1000000000000005253706163652e546573742e56310000cd05aa8ec69624595dd5c844dbc40f03d891154be925157f4ce25c95c8d6cba4f07f00000000000001000000000000000300000000000000e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0320000000000000000080000000000000010000000000000b9f2d03400000000"
    );
}

#[test]
fn commit_construction_rejects_an_unrelated_receipt_and_invalid_extent() {
    let (prepare_bytes, receipt, store_id) = encoded_prepare();
    let prepare =
        TransactionPrepareRecordViewV1::parse(&prepare_bytes, LogPackDecodeLimits::UNBOUNDED)
            .unwrap();
    let identity = crate::rspace::persistence::PrepareRecordIdentityV1::from_record_view(prepare);
    let unrelated = crate::rspace::persistence::SealedTransitionV1::new_for_persistence(
        receipt.base(),
        crate::rspace::persistence::StateVersionV1::new(
            receipt.target().generation() + 1,
            receipt.target().manifest(),
        ),
        receipt.mutation_digest(),
        receipt.profile_id(),
    );
    let address = LogPackAddress::new(1, 4096, prepare.record_len()).unwrap();
    assert!(matches!(
        TransactionCommitRecordV1::new(
            unrelated,
            store_id,
            PhysicalDigest::ZERO,
            address,
            identity,
            publication(64),
        ),
        Err(LogPackFormatError::ReceiptMismatch)
    ));
    assert!(matches!(
        LogPackAddress::new(1, u64::MAX, 1),
        Err(LogPackFormatError::AddressOverflow { .. })
    ));
    assert!(matches!(LogPackAddress::new(1, 0, 0), Err(LogPackFormatError::ZeroAddressLength)));
    assert!(matches!(LogPackAddress::new(0, 0, 1), Err(LogPackFormatError::ZeroPackId)));
    assert!(matches!(
        TransactionCommitRecordV1::new(
            receipt,
            store_id,
            PhysicalDigest::ZERO,
            LogPackAddress::new(1, PACK_HEADER_LEN_V1 - 1, prepare.record_len()).unwrap(),
            identity,
            publication(64),
        ),
        Err(LogPackFormatError::AddressBeforePackHeader {
            field: "prepare record",
            offset,
            header_len: PACK_HEADER_LEN_V1,
        }) if offset == PACK_HEADER_LEN_V1 - 1
    ));
}

#[test]
fn hundred_thousand_byte_transition_streams_and_validates_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let store = CanonicalObjectStore::new();
            let base = base_state(&store, 9, None);
            let identity = "w".repeat(100_000);
            let mutation = data_batch(base, &identity);
            let sealed = seal_data(&store, &mutation);
            let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
                &sealed,
                LogPackObjectPlanV1::classify(&sealed, &MissingLookup).unwrap(),
                StoreId::from_array([0x44; 16]),
                LogPackSequence::new(1).unwrap(),
                LogPackDecodeLimits::UNBOUNDED,
            )
            .unwrap();
            let mut bytes = Vec::new();
            prepared.write_to(&mut bytes).unwrap();
            let view =
                TransactionPrepareRecordViewV1::parse(&bytes, LogPackDecodeLimits::UNBOUNDED)
                    .unwrap();
            let key = view.mutation().data().next().unwrap().key;
            assert!(key.len() > identity.len());
        })
        .unwrap()
        .join()
        .unwrap();
}
