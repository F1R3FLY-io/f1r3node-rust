use std::convert::Infallible;

use super::support::TestSemanticProfile;
use super::transaction_sealing::{base_state, data_batch, seal_data};
use crate::rspace::persistence::{
    CanonicalObjectStore, CanonicalSink, LogPackAddress, LogPackDecodeLimits, LogPackFormatError,
    LogPackSequence, LogPackWriteError, PhysicalDigest, PreparedLogPackTransactionV1, StoreId,
    TransactionCommitRecordV1, TransactionCommitViewV1, TransactionPrepareRecordViewV1,
};

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
    for object in view.objects() {
        if let Some(previous) = previous {
            assert!(previous < object.reference().digest());
        }
        previous = Some(object.reference().digest());
    }
    assert_eq!(view.record_len(), bytes.len() as u64);
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
    let commit =
        TransactionCommitRecordV1::new(receipt, store_id, previous, address, identity, 4096)
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
        LogPackAddress::new(0, 0, prepare.record_len()).unwrap(),
        crate::rspace::persistence::PrepareRecordIdentityV1::from_record_view(prepare),
        0,
    )
    .unwrap();
    assert_eq!(commit.canonical_bytes().len(), 308);
    let mut corrupted = *commit.canonical_bytes();
    corrupted[200] ^= 1;
    assert!(matches!(
        TransactionCommitViewV1::parse(&corrupted),
        Err(LogPackFormatError::RecordChecksum { .. })
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

fn replace_u64_and_commit_crc(bytes: &mut [u8; 308], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    let crc = crate::rspace::persistence::checksum::crc32c(&bytes[..304]);
    bytes[304..].copy_from_slice(&crc.to_le_bytes());
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
    let commit =
        TransactionCommitRecordV1::new(receipt, store_id, previous, address, identity, 4096)
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
    let crc = crate::rspace::persistence::checksum::crc32c(&changed_store[..304]);
    changed_store[304..].copy_from_slice(&crc.to_le_bytes());
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
    let crc = crate::rspace::persistence::checksum::crc32c(&changed_digest[..304]);
    changed_digest[304..].copy_from_slice(&crc.to_le_bytes());
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
    let crc = crate::rspace::persistence::checksum::crc32c(&changed_previous[..304]);
    changed_previous[304..].copy_from_slice(&crc.to_le_bytes());
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
        32_768,
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
        "len=1099;header=52537061636557414c2e507265700000010000000000000001000000000000004b0400000000000007000000000000005a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a290000000000000008a2bad5d01cd13ce6c39000aa50c93509172aebb94fed4f47858b8414404bcfd1000000000000002a0000000000000024b1f6c14483eeb76ebb2e72dcad8846267740bde2cc0ce364cef24b98131dd0d1000000000000005253706163652e546573742e56310000cd05aa8ec69624595dd5c844dbc40f03d891154be925157f4ce25c95c8d6cba458010000000000000200000000000000b301000000000000;digest=fe911cfa497db5184253d12cdef290ed07df53f867fa166645461d336971f3a9;crc=c9ce76b8;commit=52537061636557414c2e436f6d6d000001000000000000000200000000000000340100000000000007000000000000005a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a80f5dfe8cb9b5cec6d10570022f58c7d18aa77808b16ac31784bcdfee68ac6a2070000000000000000400000000000004b04000000000000fe911cfa497db5184253d12cdef290ed07df53f867fa166645461d336971f3a9290000000000000008a2bad5d01cd13ce6c39000aa50c93509172aebb94fed4f47858b8414404bcfd1000000000000002a0000000000000024b1f6c14483eeb76ebb2e72dcad8846267740bde2cc0ce364cef24b98131dd0d1000000000000005253706163652e546573742e56310000cd05aa8ec69624595dd5c844dbc40f03d891154be925157f4ce25c95c8d6cba400800000000000009c89e8da"
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
    let address = LogPackAddress::new(0, 4096, prepare.record_len()).unwrap();
    assert!(matches!(
        TransactionCommitRecordV1::new(
            unrelated,
            store_id,
            PhysicalDigest::ZERO,
            address,
            identity,
            0,
        ),
        Err(LogPackFormatError::ReceiptMismatch)
    ));
    assert!(matches!(
        LogPackAddress::new(0, u64::MAX, 1),
        Err(LogPackFormatError::AddressOverflow { .. })
    ));
    assert!(matches!(LogPackAddress::new(0, 0, 0), Err(LogPackFormatError::ZeroAddressLength)));
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
