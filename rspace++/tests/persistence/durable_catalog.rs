use std::convert::Infallible;

use super::durable_recovery::valid_checkpoint_image;
use super::support::{TEST_SEMANTIC_PROFILE_ID, TestSemanticProfile, synthetic_ref};
use super::transaction_sealing::{base_state, data_batch, seal_data};
use crate::rspace::persistence::retention::v2_format::{
    RetentionAnchorV2, empty_retention_radix_ref,
};
use crate::rspace::persistence::wal::GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1;
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CanonicalObjectStore, CatalogAuthorityTokenV1,
    CatalogPackStateV1, CatalogSelectionV1, CatalogSlotV1, CommittedObjectComparison,
    CommittedObjectLookup, DurableFormatError, LogPackAddress, LogPackDecodeLimits,
    LogPackFormatError, LogPackObjectPlanV1, LogPackSequence, ObjectRefV1, PhysicalDigest,
    PreparedLogPackTransactionV1, RetainedStateV1, RetentionManifestV1, SemanticProfileId,
    StateVersionV1, StoreId, TRANSACTION_COMMIT_RECORD_LEN_V1, TransactionCommitRecordV1,
    TransactionCommitViewV1,
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

fn pack(epoch: u64) -> CatalogPackStateV1 {
    CatalogPackStateV1::new(
        5,
        7,
        4096 + epoch * 512 + TRANSACTION_COMMIT_RECORD_LEN_V1,
        3,
        64 + epoch * 72,
        synthetic_ref(0xf0),
        epoch * 2048,
        epoch * 4096,
    )
    .unwrap()
}

fn slot(epoch: u64, generation: u64) -> CatalogSlotV1 {
    CatalogSlotV1::new(
        StoreId::from_array([0x44; 16]),
        epoch,
        StateVersionV1::new(generation, synthetic_ref(generation as u8 + 1)),
        TEST_SEMANTIC_PROFILE_ID,
        PhysicalDigest::of_bytes(&epoch.to_le_bytes()),
        LogPackSequence::new(epoch).unwrap(),
        LogPackAddress::new(7, 4096 + epoch * 512, TRANSACTION_COMMIT_RECORD_LEN_V1).unwrap(),
        pack(epoch),
    )
    .unwrap()
}

fn image(first: Option<CatalogSlotV1>, second: Option<CatalogSlotV1>) -> [u8; CATALOG_IMAGE_LEN] {
    let mut image = [0u8; CATALOG_IMAGE_LEN];
    if let Some(first) = first {
        image[..CATALOG_SLOT_LEN].copy_from_slice(&first.encode());
    }
    if let Some(second) = second {
        image[CATALOG_SLOT_LEN..].copy_from_slice(&second.encode());
    }
    image
}

fn bound_commit_and_slot() -> (CatalogSlotV1, TransactionCommitViewV1) {
    let object_store = CanonicalObjectStore::new();
    let base = base_state(&object_store, 80, None);
    let mutation = data_batch(base, "catalog-terminal-commit");
    let sealed = seal_data(&object_store, &mutation);
    let receipt = sealed.receipt();
    let store_id = StoreId::from_array([0x66; 16]);
    let sequence = LogPackSequence::new(17).unwrap();
    let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, &MissingLookup).unwrap(),
        store_id,
        sequence,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let mut prepare_bytes = Vec::new();
    let prepare = prepared.write_to(&mut prepare_bytes).unwrap();
    let prepare_address = LogPackAddress::new(11, 128, prepare.record_len()).unwrap();
    let commit_address =
        LogPackAddress::new(11, prepare_address.end(), TRANSACTION_COMMIT_RECORD_LEN_V1).unwrap();
    let pack = CatalogPackStateV1::new(
        9,
        11,
        commit_address.end(),
        4,
        64 + 3 * 72,
        synthetic_ref(0xd0),
        4096,
        8192,
    )
    .unwrap();
    let commit = TransactionCommitRecordV1::new(
        receipt,
        store_id,
        PhysicalDigest::of_bytes(b"previous terminal commit"),
        prepare_address,
        prepare,
        pack.commit_publication(),
    )
    .unwrap();
    let slot = CatalogSlotV1::new(
        store_id,
        21,
        receipt.target(),
        receipt.profile_id(),
        commit.digest(),
        sequence,
        commit_address,
        pack,
    )
    .unwrap();
    (slot, TransactionCommitViewV1::parse(commit.canonical_bytes()).unwrap())
}

#[test]
fn slot_round_trip_and_adjacent_epoch_selection_are_exact() {
    let first = slot(11, 20);
    let second = slot(12, 21);
    assert_eq!(CatalogSlotV1::parse(&first.encode()).unwrap(), Some(first));
    let selected = CatalogSelectionV1::select(&image(Some(first), Some(second)))
        .unwrap()
        .unwrap();
    assert_eq!(selected.slot_index(), 1);
    assert_eq!(selected.inactive_slot_index(), 0);
    assert_eq!(selected.slot(), second);
    assert_eq!(selected.ignored_corrupt_slot(), None);
}

#[test]
fn catalog_accepts_only_the_two_typed_terminal_commit_lengths() {
    let checkpoint = valid_checkpoint_image();
    assert_eq!(
        CatalogSlotV1::parse(&checkpoint.catalog.encode()).unwrap(),
        Some(checkpoint.catalog),
    );

    for rejected in [383, 385, 399, 401] {
        let address = LogPackAddress::new(
            checkpoint.catalog.active_pack(),
            checkpoint.catalog.commit_address().offset(),
            rejected,
        )
        .unwrap();
        assert!(matches!(
            CatalogSlotV1::new(
                checkpoint.catalog.store_id(),
                checkpoint.catalog.epoch(),
                checkpoint.catalog.state(),
                checkpoint.catalog.profile_id(),
                checkpoint.catalog.commit_digest(),
                checkpoint.catalog.commit_sequence(),
                address,
                checkpoint.catalog.pack(),
            ),
            Err(DurableFormatError::CatalogCommitLength {
                incremental: TRANSACTION_COMMIT_RECORD_LEN_V1,
                checkpoint: GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
                actual,
            }) if actual == rejected
        ));
    }
}

#[test]
fn authority_token_compares_the_full_image_and_only_replaces_the_inactive_slot() {
    let current = slot(11, 20);
    let current_image = image(Some(current), None);
    let token = CatalogAuthorityTokenV1::select(current_image)
        .unwrap()
        .unwrap();
    assert!(token.matches_exactly(&current_image));

    let mut inactive_changed = current_image;
    inactive_changed[CATALOG_SLOT_LEN] = 1;
    assert!(!token.matches_exactly(&inactive_changed));

    let successor = slot(12, 21);
    let successor_image = token.successor_image(successor).unwrap();
    assert_eq!(&successor_image[..CATALOG_SLOT_LEN], &current.encode());
    assert_eq!(&successor_image[CATALOG_SLOT_LEN..], &successor.encode());
    assert_eq!(
        CatalogAuthorityTokenV1::select(successor_image)
            .unwrap()
            .unwrap()
            .slot(),
        successor
    );

    assert!(matches!(
        token.successor_image(slot(13, 22)),
        Err(DurableFormatError::CatalogSuccessorEpoch {
            expected: 12,
            actual: 13
        })
    ));
}

#[test]
fn authority_token_rejects_every_catalog_image_byte_mutation() {
    let current = slot(11, 20);
    let successor = slot(12, 21);
    let current_image = image(Some(current), Some(successor));
    let token = CatalogAuthorityTokenV1::select(current_image)
        .unwrap()
        .unwrap();

    for offset in 0..CATALOG_IMAGE_LEN {
        let mut changed = current_image;
        changed[offset] ^= 0x80;
        assert!(
            !token.matches_exactly(&changed),
            "catalog byte {offset} escaped exact-image compare-and-swap authority",
        );
    }
}

#[test]
fn every_observably_torn_inactive_slot_preserves_the_previous_authority() {
    let current = slot(30, 70);
    let next = slot(31, 71).encode();
    for prefix in 1..CATALOG_SLOT_LEN {
        let mut torn = [0u8; CATALOG_IMAGE_LEN];
        torn[..CATALOG_SLOT_LEN].copy_from_slice(&current.encode());
        torn[CATALOG_SLOT_LEN..CATALOG_SLOT_LEN + prefix].copy_from_slice(&next[..prefix]);
        let selected = CatalogSelectionV1::select(&torn).unwrap().unwrap();
        if prefix < CATALOG_SLOT_LEN - 4 {
            assert_eq!(selected.slot(), current, "torn prefix {prefix}");
            assert_eq!(selected.ignored_corrupt_slot(), Some(1));
        } else {
            // The checksum is CRC32C encoded in a checked u64. Once its low
            // four bytes have been written, the remaining high bytes already
            // have their required zero value in the empty inactive slot; the
            // image is byte-for-byte the complete successor.
            assert_eq!(selected.slot(), slot(31, 71), "complete prefix {prefix}");
            assert_eq!(selected.ignored_corrupt_slot(), None);
        }
    }
}

#[test]
fn slot_disagreement_epoch_gaps_and_corrupt_only_images_fail_closed() {
    let lower = slot(7, 7);
    let mut different_store = slot(8, 8).encode();
    different_store[24] ^= 1;
    let auth_len = CATALOG_SLOT_LEN - 8;
    let crc = crate::rspace::persistence::checksum::crc32c(&different_store[..auth_len]);
    different_store[auth_len..]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(crc));
    let different_store = CatalogSlotV1::parse(&different_store).unwrap().unwrap();
    assert!(matches!(
        CatalogSelectionV1::select(&image(Some(lower), Some(different_store))),
        Err(DurableFormatError::CatalogStoreMismatch { .. })
    ));

    assert!(matches!(
        CatalogSelectionV1::select(&image(Some(lower), Some(slot(10, 10)))),
        Err(DurableFormatError::CatalogEpochGap { .. })
    ));

    let mut corrupt = [0x55u8; CATALOG_IMAGE_LEN];
    corrupt[100] ^= 1;
    assert!(matches!(
        CatalogSelectionV1::select(&corrupt),
        Err(DurableFormatError::NoValidCatalog { .. })
    ));
}

#[test]
fn catalog_successor_is_an_exact_state_cas_and_epoch_never_wraps() {
    let current = slot(4, 9);
    let target = StateVersionV1::new(10, synthetic_ref(99));
    let next = current
        .successor(
            current.state(),
            target,
            PhysicalDigest::of_bytes(b"next"),
            LogPackSequence::new(5).unwrap(),
            LogPackAddress::new(7, 9000, TRANSACTION_COMMIT_RECORD_LEN_V1).unwrap(),
            CatalogPackStateV1::new(
                5,
                7,
                9000 + TRANSACTION_COMMIT_RECORD_LEN_V1,
                4,
                496,
                synthetic_ref(0xf1),
                8192,
                12288,
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(next.epoch(), 5);
    assert_eq!(next.state(), target);
    assert!(matches!(
        current.successor(
            StateVersionV1::new(current.state().generation(), synthetic_ref(98)),
            target,
            PhysicalDigest::ZERO,
            LogPackSequence::new(5).unwrap(),
            LogPackAddress::new(7, 9000, TRANSACTION_COMMIT_RECORD_LEN_V1).unwrap(),
            CatalogPackStateV1::new(
                5,
                7,
                9000 + TRANSACTION_COMMIT_RECORD_LEN_V1,
                4,
                496,
                synthetic_ref(0xf1),
                8192,
                12288,
            )
            .unwrap(),
        ),
        Err(DurableFormatError::CatalogCompareAndSwap { .. })
    ));

    let exhausted = CatalogSlotV1::new(
        current.store_id(),
        u64::MAX,
        current.state(),
        current.profile_id(),
        current.commit_digest(),
        current.commit_sequence(),
        current.commit_address(),
        current.pack(),
    )
    .unwrap();
    assert!(matches!(
        exhausted.successor(
            exhausted.state(),
            target,
            PhysicalDigest::ZERO,
            LogPackSequence::new(5).unwrap(),
            LogPackAddress::new(7, 9000, TRANSACTION_COMMIT_RECORD_LEN_V1).unwrap(),
            CatalogPackStateV1::new(
                5,
                7,
                9000 + TRANSACTION_COMMIT_RECORD_LEN_V1,
                4,
                496,
                synthetic_ref(0xf1),
                8192,
                12288,
            )
            .unwrap(),
        ),
        Err(DurableFormatError::CatalogEpochOverflow(u64::MAX))
    ));
}

#[test]
fn catalog_rejects_wide_checksums_reserved_gaps_misaligned_indexes_and_inexact_tails() {
    let current = slot(6, 12);
    let mut wide_checksum = current.encode();
    *wide_checksum.last_mut().unwrap() = 1;
    assert!(matches!(
        CatalogSlotV1::parse(&wide_checksum),
        Err(DurableFormatError::CatalogChecksumOutOfRange(_))
    ));

    let mut reserved_gap = current.encode();
    reserved_gap[256] = 1;
    let auth_len = CATALOG_SLOT_LEN - 8;
    let crc = crate::rspace::persistence::checksum::crc32c(&reserved_gap[..auth_len]);
    reserved_gap[auth_len..]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(crc));
    assert_eq!(
        CatalogSlotV1::parse(&reserved_gap),
        Err(DurableFormatError::NonzeroCatalogReserved)
    );

    assert!(matches!(
        CatalogPackStateV1::new(1, 1, 4096, 1, 65, synthetic_ref(1), 128, 128),
        Err(DurableFormatError::CatalogIndexTail(65))
    ));
    assert!(matches!(
        crate::rspace::persistence::CommitPublicationV1::new(1, 1, 65, synthetic_ref(1), 128, 128,),
        Err(LogPackFormatError::IndexTailUnaligned(65))
    ));

    let exact_end = current.commit_address().end();
    let inexact_pack = CatalogPackStateV1::new(
        current.pack().first_live_pack(),
        current.active_pack(),
        exact_end + 1,
        current.pack().index_generation(),
        current.pack().durable_index_tail(),
        current.pack().retention_manifest(),
        current.pack().current_state_canonical_bytes(),
        current.pack().retained_canonical_bytes(),
    )
    .unwrap();
    assert!(matches!(
        CatalogSlotV1::new(
            current.store_id(),
            current.epoch(),
            current.state(),
            current.profile_id(),
            current.commit_digest(),
            current.commit_sequence(),
            current.commit_address(),
            inexact_pack,
        ),
        Err(DurableFormatError::CatalogPackTail { .. })
    ));

    let inside_header = LogPackAddress::new(
        current.active_pack(),
        crate::rspace::persistence::PACK_HEADER_LEN_V1 - 1,
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    )
    .unwrap();
    let inside_header_pack = CatalogPackStateV1::new(
        current.pack().first_live_pack(),
        current.active_pack(),
        inside_header.end(),
        current.pack().index_generation(),
        current.pack().durable_index_tail(),
        current.pack().retention_manifest(),
        current.pack().current_state_canonical_bytes(),
        current.pack().retained_canonical_bytes(),
    )
    .unwrap();
    assert!(matches!(
        CatalogSlotV1::new(
            current.store_id(),
            current.epoch(),
            current.state(),
            current.profile_id(),
            current.commit_digest(),
            current.commit_sequence(),
            inside_header,
            inside_header_pack,
        ),
        Err(DurableFormatError::CatalogCommitBeforePackHeader { .. })
    ));
}

#[test]
fn selected_catalog_authority_is_bound_to_every_terminal_commit_field() {
    let (slot, commit) = bound_commit_and_slot();
    slot.verify_commit(commit).unwrap();

    let rebuild = |store_id, state, profile_id, commit_digest, commit_sequence, pack| {
        CatalogSlotV1::new(
            store_id,
            slot.epoch(),
            state,
            profile_id,
            commit_digest,
            commit_sequence,
            slot.commit_address(),
            pack,
        )
        .unwrap()
    };

    assert!(matches!(
        rebuild(
            StoreId::from_array([0x67; 16]),
            slot.state(),
            slot.profile_id(),
            slot.commit_digest(),
            slot.commit_sequence(),
            slot.pack(),
        )
        .verify_commit(commit),
        Err(DurableFormatError::CatalogCommitStore { .. })
    ));
    assert!(matches!(
        rebuild(
            slot.store_id(),
            slot.state(),
            slot.profile_id(),
            slot.commit_digest(),
            LogPackSequence::new(slot.commit_sequence().get() + 1).unwrap(),
            slot.pack(),
        )
        .verify_commit(commit),
        Err(DurableFormatError::CatalogCommitSequence { .. })
    ));
    assert!(matches!(
        rebuild(
            slot.store_id(),
            slot.state(),
            slot.profile_id(),
            PhysicalDigest::of_bytes(b"not the selected terminal commit"),
            slot.commit_sequence(),
            slot.pack(),
        )
        .verify_commit(commit),
        Err(DurableFormatError::CatalogCommitDigest { .. })
    ));
    assert!(matches!(
        rebuild(
            slot.store_id(),
            StateVersionV1::new(slot.state().generation() + 1, slot.state().manifest()),
            slot.profile_id(),
            slot.commit_digest(),
            slot.commit_sequence(),
            slot.pack(),
        )
        .verify_commit(commit),
        Err(DurableFormatError::CatalogCommitState { .. })
    ));
    assert!(matches!(
        rebuild(
            slot.store_id(),
            slot.state(),
            SemanticProfileId::from_array(*b"Other.Profile.V1"),
            slot.commit_digest(),
            slot.commit_sequence(),
            slot.pack(),
        )
        .verify_commit(commit),
        Err(DurableFormatError::CatalogCommitProfile { .. })
    ));
    let different_pack = CatalogPackStateV1::new(
        slot.pack().first_live_pack(),
        slot.active_pack(),
        slot.durable_pack_tail(),
        slot.pack().index_generation(),
        slot.pack().durable_index_tail(),
        slot.pack().retention_manifest(),
        slot.pack().current_state_canonical_bytes(),
        slot.pack().retained_canonical_bytes() + 1,
    )
    .unwrap();
    assert!(matches!(
        rebuild(
            slot.store_id(),
            slot.state(),
            slot.profile_id(),
            slot.commit_digest(),
            slot.commit_sequence(),
            different_pack,
        )
        .verify_commit(commit),
        Err(DurableFormatError::CatalogCommitPublication { .. })
    ));
}

#[derive(Clone, Copy, Debug)]
enum AppointedRetention {
    V1,
    V2,
}

fn slot_with_retention(epoch: u64, generation: u64, retention: ObjectRefV1) -> CatalogSlotV1 {
    let template = slot(epoch, generation);
    let pack = template.pack();
    CatalogSlotV1::new(
        template.store_id(),
        template.epoch(),
        template.state(),
        template.profile_id(),
        template.commit_digest(),
        template.commit_sequence(),
        template.commit_address(),
        CatalogPackStateV1::new(
            pack.first_live_pack(),
            pack.active_pack(),
            pack.durable_pack_tail(),
            pack.index_generation(),
            pack.durable_index_tail(),
            retention,
            pack.current_state_canonical_bytes(),
            pack.retained_canonical_bytes(),
        )
        .expect("only the retention reference changes"),
    )
    .expect("only the retention reference changes")
}

fn appointed_retention(epoch: u64, generation: u64, format: AppointedRetention) -> CatalogSlotV1 {
    let template = slot(epoch, generation);
    let current =
        RetainedStateV1::new(template.state(), template.pack().current_state_canonical_bytes())
            .expect("the catalog current closure covers its manifest");
    let retention = match format {
        AppointedRetention::V1 => RetentionManifestV1::new(current, Vec::new())
            .expect("an empty V1 history is valid")
            .object_ref(),
        AppointedRetention::V2 => RetentionAnchorV2::new(current, 0, empty_retention_radix_ref())
            .expect("an empty V2 history appoints the empty radix node")
            .object_ref(),
    };
    slot_with_retention(epoch, generation, retention)
}

fn slot_pair(
    first: [u8; CATALOG_SLOT_LEN],
    second: [u8; CATALOG_SLOT_LEN],
) -> [u8; CATALOG_IMAGE_LEN] {
    let mut image = [0u8; CATALOG_IMAGE_LEN];
    image[..CATALOG_SLOT_LEN].copy_from_slice(&first);
    image[CATALOG_SLOT_LEN..].copy_from_slice(&second);
    image
}

#[test]
fn catalog_selection_is_blind_to_the_appointed_retention_format() {
    let v1 = appointed_retention(41, 81, AppointedRetention::V1).encode();
    let v2 = appointed_retention(41, 81, AppointedRetention::V2).encode();
    let retention_extent = 216..256;
    let checksum_extent = CATALOG_SLOT_LEN - 8..CATALOG_SLOT_LEN;
    let differing = (0..CATALOG_SLOT_LEN)
        .filter(|offset| v1[*offset] != v2[*offset])
        .collect::<Vec<_>>();
    assert!(
        differing
            .iter()
            .any(|offset| retention_extent.contains(offset))
    );
    assert!(
        differing
            .iter()
            .all(|offset| retention_extent.contains(offset) || checksum_extent.contains(offset))
    );
    for format in [AppointedRetention::V1, AppointedRetention::V2] {
        let slot = appointed_retention(41, 81, format);
        assert_eq!(CatalogSlotV1::parse(&slot.encode()), Ok(Some(slot)));
    }

    for (older_format, newer_format) in [
        (AppointedRetention::V1, AppointedRetention::V1),
        (AppointedRetention::V1, AppointedRetention::V2),
        (AppointedRetention::V2, AppointedRetention::V2),
    ] {
        let older = appointed_retention(40, 80, older_format);
        let newer = appointed_retention(41, 81, newer_format);
        for (first, second, newest_index) in [(older, newer, 1), (newer, older, 0)] {
            let selected = CatalogSelectionV1::select(&image(Some(first), Some(second)))
                .expect("adjacent valid slots select")
                .expect("a populated catalog selects a slot");
            assert_eq!(selected.slot(), newer, "{older_format:?} -> {newer_format:?}");
            assert_eq!(selected.slot_index(), newest_index);
            assert_eq!(selected.ignored_corrupt_slot(), None);
        }
    }
}

#[test]
fn every_catalog_envelope_change_makes_old_readers_select_the_stale_slot() {
    fn resealed(mut bytes: [u8; CATALOG_SLOT_LEN]) -> [u8; CATALOG_SLOT_LEN] {
        let auth_len = CATALOG_SLOT_LEN - 8;
        let crc = crate::rspace::persistence::checksum::crc32c(&bytes[..auth_len]);
        bytes[auth_len..]
            .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(crc));
        bytes
    }
    fn with_u64(
        mut bytes: [u8; CATALOG_SLOT_LEN],
        offset: usize,
        value: u64,
    ) -> [u8; CATALOG_SLOT_LEN] {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        bytes
    }

    let older = appointed_retention(50, 90, AppointedRetention::V1);
    let newer = appointed_retention(51, 91, AppointedRetention::V2);
    let newer_bytes = newer.encode();
    for (first, second, newest_index) in
        [(older.encode(), newer_bytes, 1), (newer_bytes, older.encode(), 0)]
    {
        let selected = CatalogSelectionV1::select(&slot_pair(first, second))
            .expect("adjacent valid slots select")
            .expect("a populated catalog selects a slot");
        assert_eq!(selected.slot(), newer);
        assert_eq!(selected.slot_index(), newest_index);
        assert_eq!(selected.ignored_corrupt_slot(), None);
    }

    let reserved_extent = 288..CATALOG_SLOT_LEN - 8;
    let mut changes = Vec::with_capacity(5 + reserved_extent.len());
    let mut domain = newer_bytes;
    domain[..16].copy_from_slice(b"RSpaceCatalogV2\0");
    changes.push((domain, DurableFormatError::InvalidCatalogDomain));
    changes.push((with_u64(newer_bytes, 16, 2), DurableFormatError::UnsupportedCatalogVersion(2)));
    changes.push((with_u64(newer_bytes, 256, 1), DurableFormatError::NonzeroCatalogReserved));
    changes.push((with_u64(newer_bytes, 280, 1), DurableFormatError::UnsupportedCatalogFlags(1)));
    for offset in reserved_extent {
        let mut reserved = newer_bytes;
        reserved[offset] = 1;
        changes.push((reserved, DurableFormatError::NonzeroCatalogReserved));
    }
    let longer_commit = TRANSACTION_COMMIT_RECORD_LEN_V1 + 32;
    changes.push((
        with_u64(
            with_u64(newer_bytes, 168, longer_commit),
            192,
            newer.pack().durable_pack_tail() + 32,
        ),
        DurableFormatError::CatalogCommitLength {
            incremental: TRANSACTION_COMMIT_RECORD_LEN_V1,
            checkpoint: GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
            actual: longer_commit,
        },
    ));

    for (changed, expected) in changes {
        let changed = resealed(changed);
        assert_eq!(CatalogSlotV1::parse(&changed), Err(expected));
        for (first, second, older_index, changed_index) in
            [(older.encode(), changed, 0, 1), (changed, older.encode(), 1, 0)]
        {
            let selected = CatalogSelectionV1::select(&slot_pair(first, second))
                .expect("one valid slot selects")
                .expect("a populated catalog selects a slot");
            assert_eq!(selected.slot(), older);
            assert_eq!(selected.slot_index(), older_index);
            assert_eq!(selected.ignored_corrupt_slot(), Some(changed_index));
        }
    }
}
