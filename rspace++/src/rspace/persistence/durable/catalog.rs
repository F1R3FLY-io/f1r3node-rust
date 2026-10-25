use super::error::DurableFormatError;
use super::{INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, PACK_HEADER_LEN_V1};
use crate::rspace::persistence::canonical::{OBJECT_DIGEST_LEN, ObjectRefV1, ReadCursor};
use crate::rspace::persistence::checksum::{
    CHECKSUM_FIELD_LEN, crc32c, decode_crc32c, encode_crc32c,
};
use crate::rspace::persistence::manifest::{SEMANTIC_PROFILE_ID_LEN, SemanticProfileId};
use crate::rspace::persistence::physical::{
    PHYSICAL_DIGEST_LEN, PhysicalDigest, STORE_ID_LEN, StoreId,
};
use crate::rspace::persistence::wal::{
    GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, TerminalCommitViewV1,
};
use crate::rspace::persistence::{
    CommitPublicationV1, LogPackAddress, LogPackSequence, StateVersionV1,
    TRANSACTION_COMMIT_RECORD_LEN_V1, TransactionCommitViewV1,
};

pub const CATALOG_SLOT_LEN: usize = 336;
pub const CATALOG_IMAGE_LEN: usize = CATALOG_SLOT_LEN * 2;
const CATALOG_AUTH_LEN: usize = CATALOG_SLOT_LEN - CHECKSUM_FIELD_LEN;
const CATALOG_DOMAIN: &[u8; 16] = b"RSpaceCatalogV1\0";
const CATALOG_VERSION: u64 = 1;
const CATALOG_REFERENCE_RESERVED_OFFSET: usize = 256;
const CATALOG_RESERVED_OFFSET: usize = 288;
const CATALOG_RESERVED_LEN: usize = CATALOG_AUTH_LEN - CATALOG_RESERVED_OFFSET;

/// Durable locations and byte inventories governed by one catalog authority.
///
/// `current_state_canonical_bytes` is the deterministic, deduplicated size of
/// the canonical object closure reachable from `CatalogSlotV1::state`.
/// `retained_canonical_bytes` is the unique canonical payload retained for the
/// current root plus explicit historical roots. Both are authenticated logical
/// inventories used to validate admission and compaction; neither is silently
/// substituted for the active consensus tariff's introduction, delivery, or
/// trace-byte ledgers. A future state-rent rule would require its own versioned
/// consensus definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogPackStateV1 {
    first_live_pack: u64,
    active_pack: u64,
    durable_pack_tail: u64,
    index_generation: u64,
    durable_index_tail: u64,
    retention_manifest: ObjectRefV1,
    current_state_canonical_bytes: u64,
    retained_canonical_bytes: u64,
}

impl CatalogPackStateV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        first_live_pack: u64,
        active_pack: u64,
        durable_pack_tail: u64,
        index_generation: u64,
        durable_index_tail: u64,
        retention_manifest: ObjectRefV1,
        current_state_canonical_bytes: u64,
        retained_canonical_bytes: u64,
    ) -> Result<Self, DurableFormatError> {
        if first_live_pack == 0 || active_pack < first_live_pack {
            return Err(DurableFormatError::CatalogPackRange {
                first: first_live_pack,
                active: active_pack,
            });
        }
        if index_generation == 0 {
            return Err(DurableFormatError::ZeroCatalogIndexGeneration);
        }
        if durable_index_tail < INDEX_HEADER_LEN_V1 ||
            !(durable_index_tail - INDEX_HEADER_LEN_V1).is_multiple_of(INDEX_RECORD_LEN_V1)
        {
            return Err(DurableFormatError::CatalogIndexTail(durable_index_tail));
        }
        if current_state_canonical_bytes > retained_canonical_bytes {
            return Err(DurableFormatError::CatalogByteInventory {
                current_state: current_state_canonical_bytes,
                retained: retained_canonical_bytes,
            });
        }
        Ok(Self {
            first_live_pack,
            active_pack,
            durable_pack_tail,
            index_generation,
            durable_index_tail,
            retention_manifest,
            current_state_canonical_bytes,
            retained_canonical_bytes,
        })
    }

    pub const fn first_live_pack(self) -> u64 { self.first_live_pack }

    pub const fn active_pack(self) -> u64 { self.active_pack }

    pub const fn durable_pack_tail(self) -> u64 { self.durable_pack_tail }

    pub const fn index_generation(self) -> u64 { self.index_generation }

    pub const fn durable_index_tail(self) -> u64 { self.durable_index_tail }

    pub const fn retention_manifest(self) -> ObjectRefV1 { self.retention_manifest }

    pub const fn current_state_canonical_bytes(self) -> u64 { self.current_state_canonical_bytes }

    pub const fn retained_canonical_bytes(self) -> u64 { self.retained_canonical_bytes }

    pub fn commit_publication(self) -> CommitPublicationV1 {
        CommitPublicationV1::new(
            self.first_live_pack,
            self.index_generation,
            self.durable_index_tail,
            self.retention_manifest,
            self.current_state_canonical_bytes,
            self.retained_canonical_bytes,
        )
        .expect("validated catalog pack state is valid terminal-commit metadata")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogSlotV1 {
    store_id: StoreId,
    epoch: u64,
    state: StateVersionV1,
    profile_id: SemanticProfileId,
    commit_digest: PhysicalDigest,
    commit_sequence: LogPackSequence,
    commit_address: LogPackAddress,
    pack: CatalogPackStateV1,
}

impl CatalogSlotV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store_id: StoreId,
        epoch: u64,
        state: StateVersionV1,
        profile_id: SemanticProfileId,
        commit_digest: PhysicalDigest,
        commit_sequence: LogPackSequence,
        commit_address: LogPackAddress,
        pack: CatalogPackStateV1,
    ) -> Result<Self, DurableFormatError> {
        if epoch == 0 {
            return Err(DurableFormatError::ZeroCatalogEpoch);
        }
        if pack.current_state_canonical_bytes() < state.manifest().encoded_len() {
            return Err(DurableFormatError::CatalogCurrentStateBytes {
                minimum: state.manifest().encoded_len(),
                actual: pack.current_state_canonical_bytes(),
            });
        }
        if commit_address.segment() != pack.active_pack() {
            return Err(DurableFormatError::CatalogCommitSegment {
                commit: commit_address.segment(),
                active: pack.active_pack(),
            });
        }
        if commit_address.length() != TRANSACTION_COMMIT_RECORD_LEN_V1 &&
            commit_address.length() != GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1
        {
            return Err(DurableFormatError::CatalogCommitLength {
                incremental: TRANSACTION_COMMIT_RECORD_LEN_V1,
                checkpoint: GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
                actual: commit_address.length(),
            });
        }
        if commit_address.offset() < PACK_HEADER_LEN_V1 {
            return Err(DurableFormatError::CatalogCommitBeforePackHeader {
                offset: commit_address.offset(),
                header_len: PACK_HEADER_LEN_V1,
            });
        }
        let commit_end = commit_address.end();
        if commit_end != pack.durable_pack_tail() {
            return Err(DurableFormatError::CatalogPackTail {
                commit_end,
                durable_tail: pack.durable_pack_tail(),
            });
        }
        Ok(Self {
            store_id,
            epoch,
            state,
            profile_id,
            commit_digest,
            commit_sequence,
            commit_address,
            pack,
        })
    }

    pub const fn store_id(self) -> StoreId { self.store_id }

    pub const fn epoch(self) -> u64 { self.epoch }

    pub const fn state(self) -> StateVersionV1 { self.state }

    pub const fn profile_id(self) -> SemanticProfileId { self.profile_id }

    pub const fn commit_digest(self) -> PhysicalDigest { self.commit_digest }

    pub const fn commit_sequence(self) -> LogPackSequence { self.commit_sequence }

    pub const fn commit_address(self) -> LogPackAddress { self.commit_address }

    pub const fn pack(self) -> CatalogPackStateV1 { self.pack }

    pub const fn active_pack(self) -> u64 { self.pack.active_pack() }

    pub const fn durable_pack_tail(self) -> u64 { self.pack.durable_pack_tail() }

    pub const fn durable_index_tail(self) -> u64 { self.pack.durable_index_tail() }

    /// Bind a selected catalog authority to the terminal commit loaded from
    /// `commit_address`. The caller still owns the positional read; this check
    /// prevents a valid commit from another store, sequence, state, profile,
    /// or publication inventory from being substituted at recovery time.
    pub fn verify_commit(self, commit: TransactionCommitViewV1) -> Result<(), DurableFormatError> {
        self.verify_terminal_commit(commit.into())
    }

    pub(crate) fn verify_terminal_commit(
        self,
        commit: TerminalCommitViewV1,
    ) -> Result<(), DurableFormatError> {
        if self.store_id != commit.store_id() {
            return Err(DurableFormatError::CatalogCommitStore {
                catalog: self.store_id,
                commit: commit.store_id(),
            });
        }
        if self.commit_sequence != commit.sequence() {
            return Err(DurableFormatError::CatalogCommitSequence {
                catalog: self.commit_sequence.get(),
                commit: commit.sequence().get(),
            });
        }
        if self.commit_digest != commit.digest() {
            return Err(DurableFormatError::CatalogCommitDigest {
                catalog: self.commit_digest,
                commit: commit.digest(),
            });
        }
        if self.state != commit.state() {
            return Err(DurableFormatError::CatalogCommitState {
                catalog: self.state,
                commit: commit.state(),
            });
        }
        if self.profile_id != commit.profile_id() {
            return Err(DurableFormatError::CatalogCommitProfile {
                catalog: self.profile_id,
                commit: commit.profile_id(),
            });
        }
        let catalog_publication = self.pack.commit_publication();
        if catalog_publication != commit.publication() {
            return Err(DurableFormatError::CatalogCommitPublication {
                catalog: catalog_publication,
                commit: commit.publication(),
            });
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn successor(
        self,
        expected_base: StateVersionV1,
        state: StateVersionV1,
        commit_digest: PhysicalDigest,
        commit_sequence: LogPackSequence,
        commit_address: LogPackAddress,
        pack: CatalogPackStateV1,
    ) -> Result<Self, DurableFormatError> {
        if self.state != expected_base {
            return Err(DurableFormatError::CatalogCompareAndSwap {
                expected: expected_base,
                actual: self.state,
            });
        }
        let epoch = self
            .epoch
            .checked_add(1)
            .ok_or(DurableFormatError::CatalogEpochOverflow(self.epoch))?;
        Self::new(
            self.store_id,
            epoch,
            state,
            self.profile_id,
            commit_digest,
            commit_sequence,
            commit_address,
            pack,
        )
    }

    pub fn encode(self) -> [u8; CATALOG_SLOT_LEN] {
        let mut bytes = [0u8; CATALOG_SLOT_LEN];
        bytes[..16].copy_from_slice(CATALOG_DOMAIN);
        put_u64(&mut bytes, 16, CATALOG_VERSION);
        bytes[24..40].copy_from_slice(self.store_id.as_bytes());
        put_u64(&mut bytes, 40, self.epoch);
        put_state(&mut bytes, 48, self.state);
        bytes[96..112].copy_from_slice(self.profile_id.as_bytes());
        bytes[112..144].copy_from_slice(self.commit_digest.as_bytes());
        put_u64(&mut bytes, 144, self.commit_sequence.get());
        put_u64(&mut bytes, 152, self.commit_address.segment());
        put_u64(&mut bytes, 160, self.commit_address.offset());
        put_u64(&mut bytes, 168, self.commit_address.length());
        put_u64(&mut bytes, 176, self.pack.first_live_pack());
        put_u64(&mut bytes, 184, self.pack.active_pack());
        put_u64(&mut bytes, 192, self.pack.durable_pack_tail());
        put_u64(&mut bytes, 200, self.pack.index_generation());
        put_u64(&mut bytes, 208, self.pack.durable_index_tail());
        put_reference(&mut bytes, 216, self.pack.retention_manifest());
        debug_assert_eq!(get_u64(&bytes, CATALOG_REFERENCE_RESERVED_OFFSET), 0);
        put_u64(&mut bytes, 264, self.pack.current_state_canonical_bytes());
        put_u64(&mut bytes, 272, self.pack.retained_canonical_bytes());
        put_u64(&mut bytes, 280, 0);
        debug_assert!(
            bytes[CATALOG_RESERVED_OFFSET..CATALOG_AUTH_LEN]
                .iter()
                .all(|byte| *byte == 0)
        );
        let checksum = crc32c(&bytes[..CATALOG_AUTH_LEN]);
        bytes[CATALOG_AUTH_LEN..].copy_from_slice(&encode_crc32c(checksum));
        bytes
    }

    pub fn parse(bytes: &[u8; CATALOG_SLOT_LEN]) -> Result<Option<Self>, DurableFormatError> {
        if bytes.iter().all(|byte| *byte == 0) {
            return Ok(None);
        }
        let expected = decode_crc32c(
            bytes[CATALOG_AUTH_LEN..]
                .try_into()
                .expect("fixed catalog checksum extent"),
        )
        .map_err(DurableFormatError::CatalogChecksumOutOfRange)?;
        let actual = crc32c(&bytes[..CATALOG_AUTH_LEN]);
        if expected != actual {
            return Err(DurableFormatError::CatalogChecksum { expected, actual });
        }
        if &bytes[..16] != CATALOG_DOMAIN {
            return Err(DurableFormatError::InvalidCatalogDomain);
        }
        let version = get_u64(bytes, 16);
        if version != CATALOG_VERSION {
            return Err(DurableFormatError::UnsupportedCatalogVersion(version));
        }
        if bytes[CATALOG_RESERVED_OFFSET..CATALOG_AUTH_LEN]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(DurableFormatError::NonzeroCatalogReserved);
        }
        if get_u64(bytes, CATALOG_REFERENCE_RESERVED_OFFSET) != 0 {
            return Err(DurableFormatError::NonzeroCatalogReserved);
        }
        let flags = get_u64(bytes, 280);
        if flags != 0 {
            return Err(DurableFormatError::UnsupportedCatalogFlags(flags));
        }
        let store_id = StoreId::from_array(
            bytes[24..40]
                .try_into()
                .expect("fixed catalog store ID extent"),
        );
        let epoch = get_u64(bytes, 40);
        let state = get_state(bytes, 48)?;
        let profile_id = SemanticProfileId::from_array(
            bytes[96..112]
                .try_into()
                .expect("fixed catalog profile extent"),
        );
        let commit_digest = PhysicalDigest::from_array(
            bytes[112..144]
                .try_into()
                .expect("fixed catalog digest extent"),
        );
        let commit_sequence = LogPackSequence::new(get_u64(bytes, 144))?;
        let commit_address =
            LogPackAddress::new(get_u64(bytes, 152), get_u64(bytes, 160), get_u64(bytes, 168))?;
        let pack = CatalogPackStateV1::new(
            get_u64(bytes, 176),
            get_u64(bytes, 184),
            get_u64(bytes, 192),
            get_u64(bytes, 200),
            get_u64(bytes, 208),
            get_reference(bytes, 216)?,
            get_u64(bytes, 264),
            get_u64(bytes, 272),
        )?;
        Self::new(
            store_id,
            epoch,
            state,
            profile_id,
            commit_digest,
            commit_sequence,
            commit_address,
            pack,
        )
        .map(Some)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogSelectionV1 {
    slot_index: usize,
    slot: CatalogSlotV1,
    ignored_corrupt_slot: Option<usize>,
}

impl CatalogSelectionV1 {
    pub fn select(image: &[u8; CATALOG_IMAGE_LEN]) -> Result<Option<Self>, DurableFormatError> {
        let first_bytes: &[u8; CATALOG_SLOT_LEN] = image[..CATALOG_SLOT_LEN]
            .try_into()
            .expect("fixed first catalog slot");
        let second_bytes: &[u8; CATALOG_SLOT_LEN] = image[CATALOG_SLOT_LEN..]
            .try_into()
            .expect("fixed second catalog slot");
        let first = CatalogSlotV1::parse(first_bytes);
        let second = CatalogSlotV1::parse(second_bytes);
        match (first, second) {
            (Ok(None), Ok(None)) => Ok(None),
            (Ok(Some(slot)), Ok(None)) => Ok(Some(Self {
                slot_index: 0,
                slot,
                ignored_corrupt_slot: None,
            })),
            (Ok(None), Ok(Some(slot))) => Ok(Some(Self {
                slot_index: 1,
                slot,
                ignored_corrupt_slot: None,
            })),
            (Ok(Some(slot)), Err(_)) => Ok(Some(Self {
                slot_index: 0,
                slot,
                ignored_corrupt_slot: Some(1),
            })),
            (Err(_), Ok(Some(slot))) => Ok(Some(Self {
                slot_index: 1,
                slot,
                ignored_corrupt_slot: Some(0),
            })),
            (Ok(None), Err(second)) => Err(DurableFormatError::NoValidCatalog {
                first: None,
                second: Some(Box::new(second)),
            }),
            (Err(first), Ok(None)) => Err(DurableFormatError::NoValidCatalog {
                first: Some(Box::new(first)),
                second: None,
            }),
            (Err(first), Err(second)) => Err(DurableFormatError::NoValidCatalog {
                first: Some(Box::new(first)),
                second: Some(Box::new(second)),
            }),
            (Ok(Some(first)), Ok(Some(second))) => select_two(first, second),
        }
    }

    pub const fn slot_index(self) -> usize { self.slot_index }

    pub const fn slot(self) -> CatalogSlotV1 { self.slot }

    pub const fn ignored_corrupt_slot(self) -> Option<usize> { self.ignored_corrupt_slot }

    pub const fn inactive_slot_index(self) -> usize { 1 - self.slot_index }
}

/// Exact compare-and-swap token for one selected two-slot catalog image.
///
/// Holding only the decoded slot is insufficient: a concurrent writer can
/// change the inactive slot without changing the selected semantic state.
/// Publication therefore compares all 672 bytes captured here immediately
/// before it overwrites the inactive slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogAuthorityTokenV1 {
    image: [u8; CATALOG_IMAGE_LEN],
    selection: CatalogSelectionV1,
}

impl CatalogAuthorityTokenV1 {
    pub fn select(image: [u8; CATALOG_IMAGE_LEN]) -> Result<Option<Self>, DurableFormatError> {
        Ok(CatalogSelectionV1::select(&image)?.map(|selection| Self { image, selection }))
    }

    pub const fn selection(&self) -> CatalogSelectionV1 { self.selection }

    pub const fn slot(&self) -> CatalogSlotV1 { self.selection.slot() }

    pub const fn image(&self) -> &[u8; CATALOG_IMAGE_LEN] { &self.image }

    pub fn matches_exactly(&self, candidate: &[u8; CATALOG_IMAGE_LEN]) -> bool {
        self.image == *candidate
    }

    /// Construct the exact successor image without mutating the held token.
    /// The successor must be the next epoch and preserve store/profile
    /// identity; deeper state-transition checks belong to `CatalogSlotV1`.
    pub fn successor_image(
        &self,
        successor: CatalogSlotV1,
    ) -> Result<[u8; CATALOG_IMAGE_LEN], DurableFormatError> {
        let current = self.slot();
        let expected_epoch = current
            .epoch()
            .checked_add(1)
            .ok_or(DurableFormatError::CatalogEpochOverflow(current.epoch()))?;
        if successor.epoch() != expected_epoch {
            return Err(DurableFormatError::CatalogSuccessorEpoch {
                expected: expected_epoch,
                actual: successor.epoch(),
            });
        }
        if successor.store_id() != current.store_id() {
            return Err(DurableFormatError::CatalogSuccessorStore {
                expected: current.store_id(),
                actual: successor.store_id(),
            });
        }
        if successor.profile_id() != current.profile_id() {
            return Err(DurableFormatError::CatalogSuccessorProfile {
                expected: current.profile_id(),
                actual: successor.profile_id(),
            });
        }
        let mut image = self.image;
        let offset = self.selection.inactive_slot_index() * CATALOG_SLOT_LEN;
        image[offset..offset + CATALOG_SLOT_LEN].copy_from_slice(&successor.encode());
        Ok(image)
    }
}

fn select_two(
    first: CatalogSlotV1,
    second: CatalogSlotV1,
) -> Result<Option<CatalogSelectionV1>, DurableFormatError> {
    let (lower_index, lower, upper_index, upper) = if first.epoch <= second.epoch {
        (0, first, 1, second)
    } else {
        (1, second, 0, first)
    };
    if lower.store_id != upper.store_id {
        return Err(DurableFormatError::CatalogStoreMismatch {
            lower: lower.store_id,
            upper: upper.store_id,
        });
    }
    if lower.profile_id != upper.profile_id {
        return Err(DurableFormatError::CatalogProfileMismatch {
            lower: lower.profile_id,
            upper: upper.profile_id,
        });
    }
    if lower.epoch == upper.epoch {
        if lower != upper {
            return Err(DurableFormatError::CatalogEpochConflict(lower.epoch));
        }
        return Ok(Some(CatalogSelectionV1 {
            slot_index: lower_index,
            slot: lower,
            ignored_corrupt_slot: None,
        }));
    }
    if upper.epoch - lower.epoch != 1 {
        return Err(DurableFormatError::CatalogEpochGap {
            lower: lower.epoch,
            upper: upper.epoch,
        });
    }
    Ok(Some(CatalogSelectionV1 {
        slot_index: upper_index,
        slot: upper,
        ignored_corrupt_slot: None,
    }))
}

fn put_state(bytes: &mut [u8], offset: usize, state: StateVersionV1) {
    put_u64(bytes, offset, state.generation());
    bytes[offset + 8..offset + 8 + OBJECT_DIGEST_LEN]
        .copy_from_slice(state.manifest().digest().as_bytes());
    put_u64(bytes, offset + 8 + OBJECT_DIGEST_LEN, state.manifest().encoded_len());
}

fn put_reference(bytes: &mut [u8], offset: usize, reference: ObjectRefV1) {
    bytes[offset..offset + OBJECT_DIGEST_LEN].copy_from_slice(reference.digest().as_bytes());
    put_u64(bytes, offset + OBJECT_DIGEST_LEN, reference.encoded_len());
}

fn get_reference(bytes: &[u8], offset: usize) -> Result<ObjectRefV1, DurableFormatError> {
    let mut cursor = ReadCursor::new(&bytes[offset..offset + OBJECT_DIGEST_LEN + 8]);
    let reference = ObjectRefV1::parse(&mut cursor)?;
    cursor.finish()?;
    Ok(reference)
}

fn get_state(bytes: &[u8], offset: usize) -> Result<StateVersionV1, DurableFormatError> {
    let mut cursor = ReadCursor::new(&bytes[offset..offset + 48]);
    let generation = cursor.read_u64()?;
    let manifest = ObjectRefV1::parse(&mut cursor)?;
    cursor.finish()?;
    Ok(StateVersionV1::new(generation, manifest))
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("fixed catalog integer extent"),
    )
}

const _: () = {
    assert!(STORE_ID_LEN == 16);
    assert!(PHYSICAL_DIGEST_LEN == 32);
    assert!(SEMANTIC_PROFILE_ID_LEN == 16);
    assert!(CATALOG_RESERVED_LEN == 40);
};
