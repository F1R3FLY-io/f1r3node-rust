use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::hash::{BuildHasher, RandomState};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};

use super::io::{
    DurableDirectoryV1, DurableIoError, DurableIoOperation, open_directory_handle, positioned_read,
    read_exact_at_with, validate_authority_file,
};
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::persistence::checksum::{
    CHECKSUM_FIELD_LEN, Crc32c, crc32c, decode_crc32c, encode_crc32c,
};
use crate::rspace::persistence::manifest::{SEMANTIC_PROFILE_ID_LEN, SemanticProfileId};

pub(crate) const PREIMAGE_FILE_NAME_V1: &str = "preimage.v1";
const PREIMAGE_KEY_LEN: usize = 32;
const KEY_WORDS: usize = PREIMAGE_KEY_LEN / 8;
const PREIMAGE_DOMAIN: &[u8; 16] = b"RSpacePreimg.V1\0";
const PREIMAGE_VERSION: u64 = 1;
const HEADER_VERSION_OFFSET: usize = PREIMAGE_DOMAIN.len();
const HEADER_PROFILE_OFFSET: usize = HEADER_VERSION_OFFSET + 8;
const HEADER_AUTH_LEN: usize = HEADER_PROFILE_OFFSET + SEMANTIC_PROFILE_ID_LEN;
const HEADER_LEN: usize = HEADER_AUTH_LEN + CHECKSUM_FIELD_LEN;
const ENTRY_PREFIX_LEN: usize = 1 + PREIMAGE_KEY_LEN + 4;
const ENTRY_OVERHEAD_LEN: usize = ENTRY_PREFIX_LEN + CHECKSUM_FIELD_LEN;
const SCAN_BUFFER_LEN: u64 = 1 << 20;
const RECORD_CHUNK_BASE: usize = 1024;
const RECORD_CHUNK_COUNT: usize = 44;
const SLOT_TABLE_MIN_LEN: usize = 1024;
const SLOT_TABLE_GENERATIONS: usize = 48;

#[cfg(test)]
type PreimagePartialWriteHookV1 = (usize, Box<dyn FnOnce() + 'static>);

#[cfg(test)]
thread_local! {
    static PREIMAGE_PARTIAL_WRITE_HOOK: std::cell::RefCell<Option<PreimagePartialWriteHookV1>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn arm_preimage_partial_write_hook(prefix_len: usize, hook: impl FnOnce() + 'static) {
    PREIMAGE_PARTIAL_WRITE_HOOK.with(|armed| {
        let previous = armed.borrow_mut().replace((prefix_len, Box::new(hook)));
        assert!(previous.is_none(), "preimage partial-write hook already armed");
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreimageKindV1 {
    Channel,
    OrderedGroup,
}

impl PreimageKindV1 {
    const fn code(self) -> u8 {
        match self {
            Self::Channel => 1,
            Self::OrderedGroup => 2,
        }
    }

    const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Channel),
            2 => Some(Self::OrderedGroup),
            _ => None,
        }
    }

    const fn admits_value_len(self, length: u32) -> bool {
        match self {
            Self::Channel => length > 0,
            Self::OrderedGroup => length > 0 && length.is_multiple_of(PREIMAGE_KEY_LEN as u32),
        }
    }
}

impl Display for PreimageKindV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Channel => "channel",
            Self::OrderedGroup => "ordered group",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PreimageEntryV1 {
    Channel {
        key: [u8; PREIMAGE_KEY_LEN],
        identity: Vec<u8>,
    },
    OrderedGroup {
        key: [u8; PREIMAGE_KEY_LEN],
        channels: Vec<[u8; PREIMAGE_KEY_LEN]>,
    },
}

impl PreimageEntryV1 {
    pub(crate) fn channel<C: StableHashSerialize>(channel: &C) -> Self {
        let identity = channel.stable_hash_bytes();
        Self::Channel {
            key: blake2b256([identity.as_slice()]),
            identity,
        }
    }

    pub(crate) fn ordered_group<C: StableHashSerialize>(channels: &[C]) -> Self {
        let mut hashes = Vec::with_capacity(channels.len());
        hashes.extend(
            channels
                .iter()
                .map(|channel| blake2b256([channel.stable_hash_bytes().as_slice()])),
        );
        Self::OrderedGroup {
            key: bucket_key(&hashes),
            channels: hashes,
        }
    }

    const fn kind(&self) -> PreimageKindV1 {
        match self {
            Self::Channel { .. } => PreimageKindV1::Channel,
            Self::OrderedGroup { .. } => PreimageKindV1::OrderedGroup,
        }
    }

    const fn key(&self) -> &[u8; PREIMAGE_KEY_LEN] {
        match self {
            Self::Channel { key, .. } | Self::OrderedGroup { key, .. } => key,
        }
    }

    fn value(&self) -> &[u8] {
        match self {
            Self::Channel { identity, .. } => identity,
            Self::OrderedGroup { channels, .. } => channels.as_flattened(),
        }
    }

    fn admitted_value_len(&self) -> Result<u32, PreimageIndexError> {
        let kind = self.kind();
        let length = self.value().len();
        match u32::try_from(length) {
            Ok(admitted) if kind.admits_value_len(admitted) => Ok(admitted),
            _ => Err(PreimageIndexError::InvalidEntryLength {
                kind,
                length: len_u64(length),
            }),
        }
    }

    fn encode_into(&self, value_len: u32, output: &mut Vec<u8>) {
        let start = output.len();
        output.push(self.kind().code());
        output.extend_from_slice(self.key());
        output.extend_from_slice(&value_len.to_le_bytes());
        output.extend_from_slice(self.value());
        let checksum = crc32c(&output[start..]);
        output.extend_from_slice(&encode_crc32c(checksum));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreimageEntryDamageV1 {
    ChecksumOutOfRange(u64),
    Checksum {
        expected: u32,
        actual: u32,
    },
    KindMismatch {
        expected: PreimageKindV1,
        actual: u8,
    },
    KeyMismatch,
    LengthMismatch {
        expected: u32,
        actual: u32,
    },
}

impl Display for PreimageEntryDamageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChecksumOutOfRange(encoded) => {
                write!(formatter, "checksum field {encoded:#x} has a nonzero high half")
            }
            Self::Checksum { expected, actual } => write!(
                formatter,
                "checksum {actual:#010x} does not equal the recorded {expected:#010x}"
            ),
            Self::KindMismatch { expected, actual } => {
                write!(formatter, "entry kind {actual} where a {expected} entry was indexed")
            }
            Self::KeyMismatch => formatter.write_str("entry key differs from the indexed key"),
            Self::LengthMismatch { expected, actual } => {
                write!(formatter, "value length {actual} where {expected} was indexed")
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum PreimageIndexError {
    Io(DurableIoError),
    Poisoned,
    Allocation {
        resource: &'static str,
        requested: u64,
    },
    CapacityExhausted(&'static str),
    UnknownMagic,
    UnsupportedVersion(u64),
    ForeignProfile {
        expected: SemanticProfileId,
        actual: SemanticProfileId,
    },
    DamagedEntry {
        offset: u64,
        damage: PreimageEntryDamageV1,
    },
    ForgedEntry {
        offset: u64,
        kind: PreimageKindV1,
    },
    UnhashedEntry {
        kind: PreimageKindV1,
        key: [u8; PREIMAGE_KEY_LEN],
    },
    InvalidEntryLength {
        kind: PreimageKindV1,
        length: u64,
    },
    LengthOverflow {
        end: u64,
    },
}

impl Display for PreimageIndexError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "preimage index I/O failed: {error}"),
            Self::Poisoned => formatter.write_str("preimage index writer lock is poisoned"),
            Self::Allocation {
                resource,
                requested,
            } => write!(formatter, "cannot allocate the {resource} for {requested} elements"),
            Self::CapacityExhausted(resource) => write!(formatter, "the {resource} is exhausted"),
            Self::UnknownMagic => {
                formatter.write_str("preimage index header does not start with the preimage domain")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "preimage index format version {version} is unsupported")
            }
            Self::ForeignProfile { expected, actual } => write!(
                formatter,
                "preimage index belongs to semantic profile {actual:?}, not {expected:?}"
            ),
            Self::DamagedEntry { offset, damage } => {
                write!(formatter, "preimage entry at offset {offset} is damaged: {damage}")
            }
            Self::ForgedEntry { offset, kind } => write!(
                formatter,
                "{kind} preimage entry at offset {offset} does not hash to its key"
            ),
            Self::UnhashedEntry { kind, key } => write!(
                formatter,
                "{kind} preimage entry for key {} does not hash to its key",
                hex::encode(key)
            ),
            Self::InvalidEntryLength { kind, length } => {
                write!(formatter, "{kind} preimage value of {length} bytes is not admissible")
            }
            Self::LengthOverflow { end } => {
                write!(formatter, "appending at preimage index end {end} overflows the file length")
            }
        }
    }
}

impl std::error::Error for PreimageIndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DurableIoError> for PreimageIndexError {
    fn from(error: DurableIoError) -> Self { Self::Io(error) }
}

#[derive(Debug, Default)]
struct PreimageRecordV1 {
    key: [AtomicU64; KEY_WORDS],
    offset: AtomicU64,
    value_len: AtomicU32,
    next: AtomicU64,
}

impl PreimageRecordV1 {
    fn write(&self, words: &[u64; KEY_WORDS], offset: u64, value_len: u32) {
        for (stored, word) in self.key.iter().zip(words) {
            stored.store(*word, Ordering::Relaxed);
        }
        self.offset.store(offset, Ordering::Relaxed);
        self.value_len.store(value_len, Ordering::Relaxed);
    }

    fn holds_key(&self, words: &[u64; KEY_WORDS]) -> bool {
        self.key
            .iter()
            .zip(words)
            .all(|(stored, word)| stored.load(Ordering::Relaxed) == *word)
    }

    fn key(&self) -> [u8; PREIMAGE_KEY_LEN] {
        let mut key = [0_u8; PREIMAGE_KEY_LEN];
        for (bytes, stored) in key.as_chunks_mut::<8>().0.iter_mut().zip(&self.key) {
            *bytes = stored.load(Ordering::Relaxed).to_le_bytes();
        }
        key
    }

    fn offset(&self) -> u64 { self.offset.load(Ordering::Relaxed) }

    fn value_len(&self) -> u32 { self.value_len.load(Ordering::Relaxed) }
}

struct PreimageRecordArenaV1 {
    chunks: [OnceLock<Box<[PreimageRecordV1]>>; RECORD_CHUNK_COUNT],
}

impl PreimageRecordArenaV1 {
    const fn new() -> Self {
        Self {
            chunks: [const { OnceLock::new() }; RECORD_CHUNK_COUNT],
        }
    }

    fn locate(index: usize) -> (usize, usize) {
        let chunk = (index / RECORD_CHUNK_BASE + 1).ilog2() as usize;
        (chunk, index - RECORD_CHUNK_BASE * ((1 << chunk) - 1))
    }

    fn record(&self, index: usize) -> &PreimageRecordV1 {
        let (chunk, position) = Self::locate(index);
        &self.chunks[chunk]
            .get()
            .expect("every reserved or published record lies in an allocated chunk")[position]
    }

    fn reserve(&self, len: usize) -> Result<(), PreimageIndexError> {
        let Some(last) = len.checked_sub(1) else {
            return Ok(());
        };
        let (last_chunk, _) = Self::locate(last);
        let chunks = self
            .chunks
            .get(..=last_chunk)
            .ok_or(PreimageIndexError::CapacityExhausted("preimage record arena"))?;
        if chunks.last().is_some_and(|cell| cell.get().is_some()) {
            return Ok(());
        }
        for (chunk, cell) in chunks.iter().enumerate() {
            if cell.get().is_none() {
                let records = allocate(
                    RECORD_CHUNK_BASE << chunk,
                    "preimage record arena",
                    PreimageRecordV1::default,
                )?;
                cell.set(records)
                    .expect("only the writer allocates an unset record chunk");
            }
        }
        Ok(())
    }

    fn chain(&self, head: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(Some(head), move |&index| {
            match self.record(index).next.load(Ordering::Acquire) {
                0 => None,
                next => Some(record_index(next)),
            }
        })
    }

    fn link(&self, head: usize, index: usize) {
        let tail = self.chain(head).last().expect("a chain holds its head");
        self.record(tail)
            .next
            .store(slot_value(index), Ordering::Release);
    }
}

struct PreimageSlotTableV1 {
    generations: [OnceLock<Box<[AtomicU64]>>; SLOT_TABLE_GENERATIONS],
    current: AtomicUsize,
}

impl PreimageSlotTableV1 {
    const fn new() -> Self {
        Self {
            generations: [const { OnceLock::new() }; SLOT_TABLE_GENERATIONS],
            current: AtomicUsize::new(0),
        }
    }

    fn find(
        &self,
        records: &PreimageRecordArenaV1,
        hash: u64,
        words: &[u64; KEY_WORDS],
    ) -> Option<usize> {
        let slots = self.generations[self.current.load(Ordering::Acquire)].get()?;
        probe(slots, hash)
            .map_while(|slot| match slot.load(Ordering::Acquire) {
                0 => None,
                stored => Some(record_index(stored)),
            })
            .find(|&index| records.record(index).holds_key(words))
    }

    fn insert(&self, hash: u64, index: usize) {
        let slots = self.generations[self.current.load(Ordering::Relaxed)]
            .get()
            .expect("a reserved slot table is allocated");
        probe(slots, hash)
            .find(|slot| slot.load(Ordering::Relaxed) == 0)
            .expect("a slot table at load at most one half holds an empty slot")
            .store(slot_value(index), Ordering::Release);
    }

    fn reserve(
        &self,
        records: &PreimageRecordArenaV1,
        hash_state: &RandomState,
        used: usize,
        additional: usize,
    ) -> Result<(), PreimageIndexError> {
        if additional == 0 {
            return Ok(());
        }
        let required = used
            .checked_add(additional)
            .and_then(|keys| keys.checked_mul(2))
            .ok_or(PreimageIndexError::CapacityExhausted("preimage slot table"))?;
        let generation = self.current.load(Ordering::Relaxed);
        let current = self.generations[generation].get();
        if current.is_some_and(|slots| slots.len() >= required) {
            return Ok(());
        }
        let target = match current {
            Some(_) => generation + 1,
            None => generation,
        };
        let cell = self
            .generations
            .get(target)
            .ok_or(PreimageIndexError::CapacityExhausted("preimage slot table generations"))?;
        let len = required
            .checked_next_power_of_two()
            .ok_or(PreimageIndexError::CapacityExhausted("preimage slot table"))?
            .max(SLOT_TABLE_MIN_LEN);
        let slots = allocate(len, "preimage slot table", AtomicU64::default)?;
        let occupied = current
            .into_iter()
            .flat_map(|slots| slots.iter())
            .filter_map(|slot| match slot.load(Ordering::Relaxed) {
                0 => None,
                stored => Some(stored),
            });
        for stored in occupied {
            let key = records.record(record_index(stored)).key();
            probe(&slots, hash_state.hash_one(key))
                .find(|slot| slot.load(Ordering::Relaxed) == 0)
                .expect("a grown slot table holds an empty slot")
                .store(stored, Ordering::Relaxed);
        }
        cell.set(slots)
            .expect("only the writer installs an unset slot table generation");
        self.current.store(target, Ordering::Release);
        Ok(())
    }
}

struct PreimageLookupV1 {
    records: PreimageRecordArenaV1,
    channels: PreimageSlotTableV1,
    groups: PreimageSlotTableV1,
    hash_state: RandomState,
}

impl PreimageLookupV1 {
    fn new() -> Self {
        Self {
            records: PreimageRecordArenaV1::new(),
            channels: PreimageSlotTableV1::new(),
            groups: PreimageSlotTableV1::new(),
            hash_state: RandomState::new(),
        }
    }

    const fn table(&self, kind: PreimageKindV1) -> &PreimageSlotTableV1 {
        match kind {
            PreimageKindV1::Channel => &self.channels,
            PreimageKindV1::OrderedGroup => &self.groups,
        }
    }

    fn find(&self, kind: PreimageKindV1, key: &[u8; PREIMAGE_KEY_LEN]) -> Option<usize> {
        self.table(kind)
            .find(&self.records, self.hash_state.hash_one(key), &key_words(key))
    }

    fn reserve(
        &self,
        writer: &PreimageWriterStateV1,
        records: usize,
        channel_keys: usize,
        group_keys: usize,
    ) -> Result<(), PreimageIndexError> {
        let total = writer
            .records
            .checked_add(records)
            .ok_or(PreimageIndexError::CapacityExhausted("preimage record arena"))?;
        self.records.reserve(total)?;
        self.channels.reserve(
            &self.records,
            &self.hash_state,
            writer.channel_keys,
            channel_keys,
        )?;
        self.groups
            .reserve(&self.records, &self.hash_state, writer.group_keys, group_keys)
    }

    fn publish(
        &self,
        writer: &mut PreimageWriterStateV1,
        kind: PreimageKindV1,
        key: &[u8; PREIMAGE_KEY_LEN],
        offset: u64,
        value_len: u32,
    ) {
        let index = writer.records;
        writer.records += 1;
        let words = key_words(key);
        self.records.record(index).write(&words, offset, value_len);
        let hash = self.hash_state.hash_one(key);
        let table = self.table(kind);
        match table.find(&self.records, hash, &words) {
            Some(head) => self.records.link(head, index),
            None => {
                table.insert(hash, index);
                match kind {
                    PreimageKindV1::Channel => writer.channel_keys += 1,
                    PreimageKindV1::OrderedGroup => writer.group_keys += 1,
                }
            }
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreimageWriteCountsV1 {
    pub(crate) appended_batches: u64,
    pub(crate) file_syncs: u64,
}

#[cfg(test)]
impl PreimageWriteCountsV1 {
    fn count_appended_batch(&mut self) { self.appended_batches += 1; }

    fn count_file_sync(&mut self) { self.file_syncs += 1; }
}

struct PreimageWriterStateV1 {
    end: u64,
    needs_header: bool,
    dirty_tail: bool,
    directory_unsynced: bool,
    records: usize,
    channel_keys: usize,
    group_keys: usize,
    #[cfg(test)]
    counts: PreimageWriteCountsV1,
}

impl PreimageWriterStateV1 {
    const fn empty() -> Self {
        Self {
            end: 0,
            needs_header: true,
            dirty_tail: false,
            directory_unsynced: false,
            records: 0,
            channel_keys: 0,
            group_keys: 0,
            #[cfg(test)]
            counts: PreimageWriteCountsV1 {
                appended_batches: 0,
                file_syncs: 0,
            },
        }
    }
}

pub(crate) struct PreimageIndexV1 {
    path: PathBuf,
    directory_path: PathBuf,
    profile: SemanticProfileId,
    file: OnceLock<File>,
    lookup: PreimageLookupV1,
    writer: Mutex<PreimageWriterStateV1>,
}

impl PreimageIndexV1 {
    pub(crate) fn open(
        directory: &DurableDirectoryV1,
        profile: SemanticProfileId,
    ) -> Result<Self, PreimageIndexError> {
        const { super::require_shareable_read_handle::<PreimageIndexV1>() };
        let path = directory.path().join(PREIMAGE_FILE_NAME_V1);
        let lookup = PreimageLookupV1::new();
        let mut writer = PreimageWriterStateV1::empty();
        let file = match open_preimage_file(&path, PreimageFileOpenV1::ReadWrite) {
            Ok(file) => {
                recover(&file, &path, profile, &lookup, &mut writer)?;
                writer.directory_unsynced = true;
                OnceLock::from(file)
            }
            Err(DurableIoError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                OnceLock::new()
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            directory_path: directory.path().to_path_buf(),
            profile,
            file,
            lookup,
            writer: Mutex::new(writer),
        })
    }

    pub(crate) fn append(&self, entries: &[PreimageEntryV1]) -> Result<bool, PreimageIndexError> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| PreimageIndexError::Poisoned)?;
        let mut admitted = Vec::with_capacity(entries.len());
        for entry in entries {
            let value_len = entry.admitted_value_len()?;
            if !value_hashes_to_key(entry.kind(), entry.key(), entry.value()) {
                return Err(PreimageIndexError::UnhashedEntry {
                    kind: entry.kind(),
                    key: *entry.key(),
                });
            }
            admitted.push((entry, value_len));
        }
        let mut seen = HashSet::with_capacity(entries.len());
        let mut staged = Vec::with_capacity(entries.len());
        for (entry, value_len) in admitted {
            if seen.insert(entry) && !self.holds(entry)? {
                staged.push((entry, value_len));
            }
        }
        if staged.is_empty() {
            return Ok(false);
        }
        let start = writer.end;
        let header_len = match writer.needs_header {
            true => HEADER_LEN,
            false => 0,
        };
        let overflow = || PreimageIndexError::LengthOverflow { end: start };
        let batch_len = staged
            .iter()
            .try_fold(len_u64(header_len), |total, (_, value_len)| {
                total.checked_add(entry_extent(*value_len))
            })
            .ok_or_else(overflow)?;
        let end = start.checked_add(batch_len).ok_or_else(overflow)?;
        let allocation = || PreimageIndexError::Allocation {
            resource: "preimage append buffer",
            requested: batch_len,
        };
        let capacity = usize::try_from(batch_len).map_err(|_| allocation())?;
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(capacity)
            .map_err(|_| allocation())?;
        if writer.needs_header {
            buffer.extend_from_slice(&encode_header(self.profile));
        }
        let mut placements = Vec::with_capacity(staged.len());
        let (mut channel_entries, mut group_entries) = (0, 0);
        for &(entry, value_len) in &staged {
            placements.push((entry, start + len_u64(buffer.len()), value_len));
            entry.encode_into(value_len, &mut buffer);
            match entry.kind() {
                PreimageKindV1::Channel => channel_entries += 1,
                PreimageKindV1::OrderedGroup => group_entries += 1,
            }
        }
        self.lookup
            .reserve(&writer, staged.len(), channel_entries, group_entries)?;
        let file = self.writable_file(&mut writer)?;
        write_batch(file, &self.path, start, &buffer).inspect_err(|_| writer.dirty_tail = true)?;
        for (entry, offset, value_len) in placements {
            self.lookup
                .publish(&mut writer, entry.kind(), entry.key(), offset, value_len);
        }
        writer.end = end;
        writer.needs_header = false;
        #[cfg(test)]
        writer.counts.count_appended_batch();
        Ok(true)
    }

    pub(crate) fn sync(&self) -> Result<(), PreimageIndexError> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| PreimageIndexError::Poisoned)?;
        let Some(file) = self.file.get() else {
            return Ok(());
        };
        validate_authority_file(file, DurableIoOperation::SyncFile, &self.path)?;
        #[cfg(test)]
        writer.counts.count_file_sync();
        file.sync_all()
            .map_err(io_error(DurableIoOperation::SyncFile, &self.path))?;
        if writer.directory_unsynced {
            open_directory_handle(&self.directory_path)?
                .sync_all()
                .map_err(io_error(DurableIoOperation::SyncDirectory, &self.directory_path))?;
            writer.directory_unsynced = false;
        }
        Ok(())
    }

    pub(crate) fn channel(
        &self,
        hash: &[u8; PREIMAGE_KEY_LEN],
    ) -> Result<Option<Vec<u8>>, PreimageIndexError> {
        let (Some(file), Some(head)) =
            (self.file.get(), self.lookup.find(PreimageKindV1::Channel, hash))
        else {
            return Ok(None);
        };
        self.read_verified_value(file, head, PreimageKindV1::Channel, hash)
            .map(Some)
    }

    pub(crate) fn ordered_groups(
        &self,
        bucket: &[u8; PREIMAGE_KEY_LEN],
    ) -> Result<Vec<Vec<[u8; PREIMAGE_KEY_LEN]>>, PreimageIndexError> {
        let (Some(file), Some(head)) =
            (self.file.get(), self.lookup.find(PreimageKindV1::OrderedGroup, bucket))
        else {
            return Ok(Vec::new());
        };
        let count = self.lookup.records.chain(head).count();
        let mut groups = Vec::with_capacity(count);
        for index in self.lookup.records.chain(head).take(count) {
            let value =
                self.read_verified_value(file, index, PreimageKindV1::OrderedGroup, bucket)?;
            groups.push(value.as_chunks::<PREIMAGE_KEY_LEN>().0.to_vec());
        }
        Ok(groups)
    }

    #[cfg(test)]
    pub(crate) fn write_counts_for_test(&self) -> PreimageWriteCountsV1 {
        match self.writer.lock() {
            Ok(writer) => writer.counts,
            Err(poisoned) => poisoned.into_inner().counts,
        }
    }

    fn holds(&self, entry: &PreimageEntryV1) -> Result<bool, PreimageIndexError> {
        let (kind, key) = (entry.kind(), entry.key());
        let (Some(file), Some(head)) = (self.file.get(), self.lookup.find(kind, key)) else {
            return Ok(false);
        };
        for index in self.lookup.records.chain(head) {
            if self.read_verified_value(file, index, kind, key)? == entry.value() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn read_verified_value(
        &self,
        file: &File,
        index: usize,
        kind: PreimageKindV1,
        key: &[u8; PREIMAGE_KEY_LEN],
    ) -> Result<Vec<u8>, PreimageIndexError> {
        let record = self.lookup.records.record(index);
        let (offset, value_len) = (record.offset(), record.value_len());
        let value_end = ENTRY_PREFIX_LEN +
            usize::try_from(value_len).expect("a 32-bit value length fits usize");
        let extent = value_end + CHECKSUM_FIELD_LEN;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(extent)
            .map_err(|_| PreimageIndexError::Allocation {
                resource: "preimage entry buffer",
                requested: len_u64(extent),
            })?;
        bytes.resize(extent, 0);
        read_exact_at_with(&self.path, offset, &mut bytes, |buffer, position| {
            positioned_read(file, buffer, position, &self.path)
        })?;
        let damaged = |damage| PreimageIndexError::DamagedEntry { offset, damage };
        let field = bytes[value_end..]
            .try_into()
            .expect("an entry ends with one checksum field");
        let expected = decode_crc32c(field)
            .map_err(|encoded| damaged(PreimageEntryDamageV1::ChecksumOutOfRange(encoded)))?;
        let actual = crc32c(&bytes[..value_end]);
        if expected != actual {
            return Err(damaged(PreimageEntryDamageV1::Checksum { expected, actual }));
        }
        let (prefix, _) = bytes
            .split_first_chunk::<ENTRY_PREFIX_LEN>()
            .expect("an entry starts with its prefix");
        let (code, stored_key, stored_len) = decode_prefix(*prefix);
        if code != kind.code() {
            return Err(damaged(PreimageEntryDamageV1::KindMismatch {
                expected: kind,
                actual: code,
            }));
        }
        if stored_key != *key {
            return Err(damaged(PreimageEntryDamageV1::KeyMismatch));
        }
        if stored_len != value_len {
            return Err(damaged(PreimageEntryDamageV1::LengthMismatch {
                expected: value_len,
                actual: stored_len,
            }));
        }
        let value = &bytes[ENTRY_PREFIX_LEN..value_end];
        if !value_hashes_to_key(kind, key, value) {
            return Err(PreimageIndexError::ForgedEntry { offset, kind });
        }
        bytes.truncate(value_end);
        bytes.drain(..ENTRY_PREFIX_LEN);
        Ok(bytes)
    }

    fn writable_file(
        &self,
        writer: &mut PreimageWriterStateV1,
    ) -> Result<&File, PreimageIndexError> {
        let file = match self.file.get() {
            Some(file) => file,
            None => {
                let created = open_preimage_file(&self.path, PreimageFileOpenV1::CreateNew)?;
                writer.directory_unsynced = true;
                self.file.get_or_init(|| created)
            }
        };
        if writer.dirty_tail {
            file.set_len(writer.end)
                .map_err(io_error(DurableIoOperation::SetLength, &self.path))?;
            writer.dirty_tail = false;
        }
        Ok(file)
    }
}

struct ScannedEntryV1 {
    kind: PreimageKindV1,
    key: [u8; PREIMAGE_KEY_LEN],
    value_len: u32,
    extent: u64,
}

fn recover(
    file: &File,
    path: &Path,
    profile: SemanticProfileId,
    lookup: &PreimageLookupV1,
    writer: &mut PreimageWriterStateV1,
) -> Result<(), PreimageIndexError> {
    let length = file
        .metadata()
        .map_err(io_error(DurableIoOperation::Metadata, path))?
        .len();
    let header_len = len_u64(HEADER_LEN);
    if length < header_len {
        return cut_tail(file, path, length, 0);
    }
    let capacity = usize::try_from(length.min(SCAN_BUFFER_LEN))
        .expect("the preimage scan buffer length fits usize");
    let mut reader = BufReader::with_capacity(capacity, file);
    let mut header = [0_u8; HEADER_LEN];
    reader
        .read_exact(&mut header)
        .map_err(io_error(DurableIoOperation::PositionalRead, path))?;
    if !header_is_whole(&header, profile)? {
        return cut_tail(file, path, length, 0);
    }
    let mut position = header_len;
    while let Some(scanned) = scan_entry(&mut reader, path, length - position)? {
        let (channel_keys, group_keys) = match scanned.kind {
            PreimageKindV1::Channel => (1, 0),
            PreimageKindV1::OrderedGroup => (0, 1),
        };
        lookup.reserve(writer, 1, channel_keys, group_keys)?;
        lookup.publish(writer, scanned.kind, &scanned.key, position, scanned.value_len);
        position += scanned.extent;
    }
    cut_tail(file, path, length, position)?;
    writer.end = position;
    writer.needs_header = false;
    Ok(())
}

fn scan_entry(
    reader: &mut BufReader<&File>,
    path: &Path,
    remaining: u64,
) -> Result<Option<ScannedEntryV1>, PreimageIndexError> {
    if remaining < len_u64(ENTRY_PREFIX_LEN) {
        return Ok(None);
    }
    let mut prefix = [0_u8; ENTRY_PREFIX_LEN];
    reader
        .read_exact(&mut prefix)
        .map_err(io_error(DurableIoOperation::PositionalRead, path))?;
    let (code, key, value_len) = decode_prefix(prefix);
    let extent = entry_extent(value_len);
    let kind = match PreimageKindV1::from_code(code) {
        Some(kind) if kind.admits_value_len(value_len) && extent <= remaining => kind,
        _ => return Ok(None),
    };
    let mut checksum = Crc32c::new();
    checksum.update(&prefix);
    let mut unread = u64::from(value_len);
    while unread > 0 {
        let buffered = match reader.fill_buf() {
            Ok(buffered) => buffered,
            Err(source) if source.kind() == io::ErrorKind::Interrupted => continue,
            Err(source) => {
                return Err(io_error(DurableIoOperation::PositionalRead, path)(source).into());
            }
        };
        let taken =
            usize::try_from(unread).map_or(buffered.len(), |unread| unread.min(buffered.len()));
        if taken == 0 {
            return Err(io_error(DurableIoOperation::PositionalRead, path)(
                io::ErrorKind::UnexpectedEof.into(),
            )
            .into());
        }
        checksum.update(&buffered[..taken]);
        reader.consume(taken);
        unread -= len_u64(taken);
    }
    let mut field = [0_u8; CHECKSUM_FIELD_LEN];
    reader
        .read_exact(&mut field)
        .map_err(io_error(DurableIoOperation::PositionalRead, path))?;
    let actual = checksum.finish();
    Ok(match decode_crc32c(field) {
        Ok(recorded) if recorded == actual => Some(ScannedEntryV1 {
            kind,
            key,
            value_len,
            extent,
        }),
        _ => None,
    })
}

fn cut_tail(file: &File, path: &Path, length: u64, end: u64) -> Result<(), PreimageIndexError> {
    if end < length {
        file.set_len(end)
            .map_err(io_error(DurableIoOperation::SetLength, path))?;
        file.sync_all()
            .map_err(io_error(DurableIoOperation::SyncFile, path))?;
    }
    Ok(())
}

fn encode_header(profile: SemanticProfileId) -> [u8; HEADER_LEN] {
    let mut header = [0_u8; HEADER_LEN];
    header[..HEADER_VERSION_OFFSET].copy_from_slice(PREIMAGE_DOMAIN);
    header[HEADER_VERSION_OFFSET..HEADER_PROFILE_OFFSET]
        .copy_from_slice(&PREIMAGE_VERSION.to_le_bytes());
    header[HEADER_PROFILE_OFFSET..HEADER_AUTH_LEN].copy_from_slice(profile.as_bytes());
    let checksum = crc32c(&header[..HEADER_AUTH_LEN]);
    header[HEADER_AUTH_LEN..].copy_from_slice(&encode_crc32c(checksum));
    header
}

fn header_is_whole(
    header: &[u8; HEADER_LEN],
    expected: SemanticProfileId,
) -> Result<bool, PreimageIndexError> {
    let (authenticated, field) = header
        .split_first_chunk::<HEADER_AUTH_LEN>()
        .expect("the header holds its authenticated fields");
    let (domain, rest) = authenticated
        .split_first_chunk::<HEADER_VERSION_OFFSET>()
        .expect("the header starts with its domain");
    let (version, profile) = rest
        .split_first_chunk::<8>()
        .expect("the header holds its version");
    if domain != PREIMAGE_DOMAIN {
        return Err(PreimageIndexError::UnknownMagic);
    }
    let version = u64::from_le_bytes(*version);
    if version != PREIMAGE_VERSION {
        return Err(PreimageIndexError::UnsupportedVersion(version));
    }
    let recorded = decode_crc32c(
        field
            .try_into()
            .expect("the header ends with one checksum field"),
    );
    if recorded != Ok(crc32c(authenticated)) {
        return Ok(false);
    }
    let actual = SemanticProfileId::from_array(
        profile
            .try_into()
            .expect("the header holds one semantic profile id"),
    );
    if actual != expected {
        return Err(PreimageIndexError::ForeignProfile { expected, actual });
    }
    Ok(true)
}

fn decode_prefix(prefix: [u8; ENTRY_PREFIX_LEN]) -> (u8, [u8; PREIMAGE_KEY_LEN], u32) {
    let [code, key @ .., l0, l1, l2, l3] = prefix;
    (code, key, u32::from_le_bytes([l0, l1, l2, l3]))
}

fn blake2b256<'a>(chunks: impl IntoIterator<Item = &'a [u8]>) -> [u8; PREIMAGE_KEY_LEN] {
    let mut hasher = Blake2b::<U32>::new();
    for chunk in chunks {
        hasher.update(chunk);
    }
    let mut digest = [0_u8; PREIMAGE_KEY_LEN];
    digest.copy_from_slice(&hasher.finalize());
    digest
}

fn bucket_key(channels: &[[u8; PREIMAGE_KEY_LEN]]) -> [u8; PREIMAGE_KEY_LEN] {
    let mut sorted = channels.to_vec();
    sorted.sort_unstable();
    blake2b256(sorted.iter().map(<[u8; PREIMAGE_KEY_LEN]>::as_slice))
}

fn value_hashes_to_key(kind: PreimageKindV1, key: &[u8; PREIMAGE_KEY_LEN], value: &[u8]) -> bool {
    match kind {
        PreimageKindV1::Channel => blake2b256([value]) == *key,
        PreimageKindV1::OrderedGroup => bucket_key(value.as_chunks::<PREIMAGE_KEY_LEN>().0) == *key,
    }
}

fn key_words(key: &[u8; PREIMAGE_KEY_LEN]) -> [u64; KEY_WORDS] {
    let (words, _) = key.as_chunks::<8>();
    std::array::from_fn(|word| u64::from_le_bytes(words[word]))
}

fn probe(slots: &[AtomicU64], hash: u64) -> impl Iterator<Item = &AtomicU64> {
    let mask = slots.len() - 1;
    let start = (hash as usize) & mask;
    (0..slots.len()).map(move |step| &slots[(start + step) & mask])
}

fn allocate<T>(
    len: usize,
    resource: &'static str,
    element: impl FnMut() -> T,
) -> Result<Box<[T]>, PreimageIndexError> {
    let mut elements = Vec::new();
    elements
        .try_reserve_exact(len)
        .map_err(|_| PreimageIndexError::Allocation {
            resource,
            requested: len_u64(len),
        })?;
    elements.extend(std::iter::repeat_with(element).take(len));
    Ok(elements.into_boxed_slice())
}

fn slot_value(index: usize) -> u64 { len_u64(index) + 1 }

fn record_index(stored: u64) -> usize {
    usize::try_from(stored - 1).expect("a published record index fits usize")
}

fn len_u64(len: usize) -> u64 { u64::try_from(len).expect("a length held in memory fits u64") }

fn entry_extent(value_len: u32) -> u64 { len_u64(ENTRY_OVERHEAD_LEN) + u64::from(value_len) }

fn io_error(
    operation: DurableIoOperation,
    path: &Path,
) -> impl FnOnce(io::Error) -> DurableIoError + '_ {
    move |source| DurableIoError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn write_batch(file: &File, path: &Path, offset: u64, bytes: &[u8]) -> Result<(), DurableIoError> {
    #[cfg(test)]
    if let Some((prefix_len, hook)) =
        PREIMAGE_PARTIAL_WRITE_HOOK.with(|armed| armed.borrow_mut().take())
    {
        let split = prefix_len.min(bytes.len());
        write_all_at(file, path, offset, &bytes[..split])?;
        hook();
        return write_all_at(file, path, offset + len_u64(split), &bytes[split..]);
    }
    write_all_at(file, path, offset, bytes)
}

#[cfg(unix)]
fn write_all_at(file: &File, path: &Path, offset: u64, bytes: &[u8]) -> Result<(), DurableIoError> {
    use std::os::unix::fs::FileExt;

    file.write_all_at(bytes, offset)
        .map_err(io_error(DurableIoOperation::PositionalWrite, path))
}

#[cfg(not(unix))]
fn write_all_at(
    _file: &File,
    path: &Path,
    _offset: u64,
    _bytes: &[u8],
) -> Result<(), DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation: DurableIoOperation::PositionalWrite,
        path: path.to_path_buf(),
    })
}

#[derive(Clone, Copy, Debug)]
enum PreimageFileOpenV1 {
    CreateNew,
    ReadWrite,
}

impl PreimageFileOpenV1 {
    const fn operation(self) -> DurableIoOperation {
        match self {
            Self::CreateNew => DurableIoOperation::CreateFile,
            Self::ReadWrite => DurableIoOperation::OpenFile,
        }
    }
}

#[cfg(unix)]
fn open_preimage_file(path: &Path, mode: PreimageFileOpenV1) -> Result<File, DurableIoError> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let operation = mode.operation();
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .write(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    match mode {
        PreimageFileOpenV1::CreateNew => {
            options.create_new(true).mode(0o600);
        }
        PreimageFileOpenV1::ReadWrite => {}
    }
    let file = options.open(path).map_err(io_error(operation, path))?;
    if let PreimageFileOpenV1::CreateNew = mode {
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(io_error(operation, path))?;
    }
    validate_authority_file(&file, operation, path)?;
    Ok(file)
}

#[cfg(not(unix))]
fn open_preimage_file(path: &Path, mode: PreimageFileOpenV1) -> Result<File, DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation: mode.operation(),
        path: path.to_path_buf(),
    })
}
