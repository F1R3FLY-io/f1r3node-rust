use std::fmt::{Display, Formatter};
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use super::reclaim::{
    RECLAMATION_RECORD_LEN_V1, ReclamationCandidateV1, ReclamationCatalogRelationV1,
};
use super::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CatalogAuthorityTokenV1, CatalogSlotV1,
    INDEX_HEADER_LEN_V1, IndexGenerationV1, PACK_HEADER_LEN_V1,
};
use crate::rspace::persistence::retention::v2_format::{RetentionFormat, RetentionReader};
use crate::rspace::persistence::{CanonicalSink, GenerationLeaseRegistryV1, PackId};

pub(crate) const CATALOG_FILE_NAME_V1: &str = "catalog.v1";
pub(crate) const WRITER_LOCK_FILE_NAME_V1: &str = "writer.lock.v1";
pub(crate) const RECLAMATION_CANDIDATE_FILE_NAME_V1: &str = "reclaim.v1";
const RECLAMATION_CANDIDATE_TEMPORARY_FILE_NAME_V1: &str = "reclaim.v1.tmp";
const PACK_FILE_PREFIX_V1: &str = "pack-";
const PACK_FILE_SUFFIX_V1: &str = ".rsp";
const INDEX_FILE_PREFIX_V1: &str = "index-";
const INDEX_FILE_SUFFIX_V1: &str = ".rsi";

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DurableIoFailpointV1 {
    PhysicalWrite,
    PhysicalSync,
    CandidateWrite,
    CandidateSetLength,
    CandidateSync,
    BeforeReplace,
    AfterReplaceBeforeDirectorySync,
    DirectorySync,
    CatalogSlotWrite,
    CatalogSync,
    CatalogDirectorySync,
    ReclamationIntentCreate,
    ReclamationIntentSetLength,
    ReclamationIntentSync,
    ReclamationIntentBeforeRename,
    ReclamationIntentDirectorySync,
}

#[cfg(test)]
thread_local! {
    static DURABLE_IO_FAILPOINT: std::cell::Cell<Option<DurableIoFailpointV1>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
thread_local! {
    static CATALOG_SLOT_WRITE_PREFIX: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
thread_local! {
    static RECLAMATION_INTENT_WRITE_PREFIX: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn arm_durable_io_failpoint(failpoint: DurableIoFailpointV1) {
    DURABLE_IO_FAILPOINT.with(|armed| {
        assert!(armed.replace(Some(failpoint)).is_none(), "durable I/O failpoint already armed");
    });
}

#[cfg(test)]
pub(crate) fn armed_durable_io_failpoint() -> Option<DurableIoFailpointV1> {
    DURABLE_IO_FAILPOINT.get()
}

#[cfg(test)]
pub(crate) fn arm_catalog_slot_write_prefix(prefix_len: usize) {
    assert!(
        prefix_len < CATALOG_SLOT_LEN,
        "a torn catalog-slot prefix must be shorter than one complete slot"
    );
    CATALOG_SLOT_WRITE_PREFIX.with(|armed| {
        assert!(
            armed.replace(Some(prefix_len)).is_none(),
            "catalog prefix failpoint already armed"
        );
    });
}

#[cfg(test)]
pub(crate) fn arm_reclamation_intent_write_prefix(prefix_len: usize) {
    assert!(
        prefix_len < RECLAMATION_RECORD_LEN_V1,
        "a torn reclamation-intent prefix must be shorter than one complete record"
    );
    RECLAMATION_INTENT_WRITE_PREFIX.with(|armed| {
        assert!(
            armed.replace(Some(prefix_len)).is_none(),
            "reclamation-intent prefix failpoint already armed"
        );
    });
}

#[cfg(test)]
pub(super) fn trip_durable_io_failpoint(
    failpoint: DurableIoFailpointV1,
    operation: DurableIoOperation,
    path: &Path,
) -> Result<(), DurableIoError> {
    DURABLE_IO_FAILPOINT.with(|armed| {
        if armed.get() != Some(failpoint) {
            return Ok(());
        }
        armed.set(None);
        Err(DurableIoError::Io {
            operation,
            path: path.to_path_buf(),
            source: io::Error::other(format!("injected durable I/O failure at {failpoint:?}")),
        })
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DurableIoOperation {
    OpenDirectory,
    OpenFile,
    CreateFile,
    Metadata,
    AcquireWriterLock,
    PositionalRead,
    PositionalWrite,
    SetLength,
    SyncData,
    SyncFile,
    SyncDirectory,
    RemoveFile,
    ReplaceFile,
    PublishFile,
    ReadDirectory,
}

impl DurableIoOperation {
    const fn name(self) -> &'static str {
        match self {
            Self::OpenDirectory => "open durable directory",
            Self::OpenFile => "open durable file",
            Self::CreateFile => "create durable file",
            Self::Metadata => "read durable file metadata",
            Self::AcquireWriterLock => "acquire durable writer lock",
            Self::PositionalRead => "read durable file at offset",
            Self::PositionalWrite => "write durable file at offset",
            Self::SetLength => "set durable file length",
            Self::SyncData => "synchronize durable file data",
            Self::SyncFile => "synchronize durable file data and metadata",
            Self::SyncDirectory => "synchronize durable directory",
            Self::RemoveFile => "remove durable temporary file",
            Self::ReplaceFile => "atomically replace durable file",
            Self::PublishFile => "publish durable file without replacement",
            Self::ReadDirectory => "read durable directory entries",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileLengthExpectation {
    Exact(u64),
    AtLeast(u64),
}

#[derive(Debug)]
pub(crate) enum DurableIoError {
    Io {
        operation: DurableIoOperation,
        path: PathBuf,
        source: io::Error,
    },
    WriterLockContended {
        operation: DurableIoOperation,
        path: PathBuf,
    },
    WriterLock {
        operation: DurableIoOperation,
        path: PathBuf,
        source: io::Error,
    },
    ShortRead {
        operation: DurableIoOperation,
        path: PathBuf,
        offset: u64,
        expected: u64,
        actual: u64,
    },
    ZeroWrite {
        operation: DurableIoOperation,
        path: PathBuf,
        offset: u64,
        remaining: u64,
    },
    Length {
        operation: DurableIoOperation,
        path: PathBuf,
        expected: FileLengthExpectation,
        actual: u64,
    },
    OffsetOverflow {
        operation: DurableIoOperation,
        path: PathBuf,
        offset: u64,
        completed: u64,
    },
    CallbackCountOutOfBounds {
        operation: DurableIoOperation,
        path: PathBuf,
        offset: u64,
        offered: u64,
        reported: u64,
    },
    UnsafeFileType {
        operation: DurableIoOperation,
        path: PathBuf,
    },
    HardLinkedFile {
        operation: DurableIoOperation,
        path: PathBuf,
        links: u64,
    },
    UnsafePermissions {
        operation: DurableIoOperation,
        path: PathBuf,
        mode: u32,
    },
    UnsafeOwner {
        operation: DurableIoOperation,
        path: PathBuf,
        expected: u32,
        actual: u32,
    },
    #[cfg_attr(
        unix,
        expect(
            dead_code,
            reason = "every constructor of this variant is compiled only on non-Unix targets"
        )
    )]
    UnsupportedPlatform {
        operation: DurableIoOperation,
        path: PathBuf,
    },
    Allocation {
        operation: DurableIoOperation,
        path: PathBuf,
        requested: u64,
    },
    AppendOverrun {
        operation: DurableIoOperation,
        path: PathBuf,
        cursor: u64,
        offered: u64,
        appointed_end: u64,
    },
    AppendUnderrun {
        operation: DurableIoOperation,
        path: PathBuf,
        actual_end: u64,
        appointed_end: u64,
    },
    CatalogAuthorityChanged {
        path: PathBuf,
    },
    ReclamationPending {
        path: PathBuf,
    },
    SuccessorOverflow {
        path: PathBuf,
        artifact: &'static str,
        current: u64,
    },
}

impl Display for DurableIoError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => write!(formatter, "{} {}: {source}", operation.name(), path.display()),
            Self::WriterLockContended { operation, path } => {
                write!(formatter, "{} {}: already held", operation.name(), path.display())
            }
            Self::WriterLock {
                operation,
                path,
                source,
            } => write!(formatter, "{} {}: {source}", operation.name(), path.display()),
            Self::ShortRead {
                operation,
                path,
                offset,
                expected,
                actual,
            } => write!(
                formatter,
                "{} {}: short read at {offset}, expected {expected} bytes, read {actual}",
                operation.name(),
                path.display()
            ),
            Self::ZeroWrite {
                operation,
                path,
                offset,
                remaining,
            } => write!(
                formatter,
                "{} {}: zero-byte write at {offset} with {remaining} bytes remaining",
                operation.name(),
                path.display()
            ),
            Self::Length {
                operation,
                path,
                expected,
                actual,
            } => match expected {
                FileLengthExpectation::Exact(expected) => write!(
                    formatter,
                    "{} {}: expected exactly {expected} bytes, found {actual}",
                    operation.name(),
                    path.display()
                ),
                FileLengthExpectation::AtLeast(expected) => write!(
                    formatter,
                    "{} {}: expected at least {expected} bytes, found {actual}",
                    operation.name(),
                    path.display()
                ),
            },
            Self::OffsetOverflow {
                operation,
                path,
                offset,
                completed,
            } => write!(
                formatter,
                "{} {}: offset {offset} plus completed length {completed} overflows",
                operation.name(),
                path.display()
            ),
            Self::CallbackCountOutOfBounds {
                operation,
                path,
                offset,
                offered,
                reported,
            } => write!(
                formatter,
                "{} {}: callback reported {reported} bytes at {offset} after receiving only \
                 {offered}",
                operation.name(),
                path.display()
            ),
            Self::UnsafeFileType { operation, path } => write!(
                formatter,
                "{} {}: authority is not a regular file",
                operation.name(),
                path.display()
            ),
            Self::HardLinkedFile {
                operation,
                path,
                links,
            } => write!(
                formatter,
                "{} {}: authority has {links} hard links; exactly one is required",
                operation.name(),
                path.display()
            ),
            Self::UnsafePermissions {
                operation,
                path,
                mode,
            } => write!(
                formatter,
                "{} {}: authority mode {:04o} grants group or other access",
                operation.name(),
                path.display(),
                mode & 0o7777
            ),
            Self::UnsafeOwner {
                operation,
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "{} {}: authority owner {actual} does not equal process owner {expected}",
                operation.name(),
                path.display()
            ),
            Self::UnsupportedPlatform { operation, path } => write!(
                formatter,
                "{} {}: durable operation is unsupported on this platform",
                operation.name(),
                path.display()
            ),
            Self::Allocation {
                operation,
                path,
                requested,
            } => write!(
                formatter,
                "{} {}: cannot allocate bounded {requested}-byte append buffer",
                operation.name(),
                path.display()
            ),
            Self::AppendOverrun {
                operation,
                path,
                cursor,
                offered,
                appointed_end,
            } => write!(
                formatter,
                "{} {}: {offered}-byte append at {cursor} exceeds appointed end {appointed_end}",
                operation.name(),
                path.display()
            ),
            Self::AppendUnderrun {
                operation,
                path,
                actual_end,
                appointed_end,
            } => write!(
                formatter,
                "{} {}: append ended at {actual_end}, before appointed end {appointed_end}",
                operation.name(),
                path.display()
            ),
            Self::CatalogAuthorityChanged { path } => write!(
                formatter,
                "durable catalog {} changed before successor orphan cleanup",
                path.display()
            ),
            Self::ReclamationPending { path } => write!(
                formatter,
                "durable reclamation {} still waits for an old-generation reader",
                path.display()
            ),
            Self::SuccessorOverflow {
                path,
                artifact,
                current,
            } => write!(
                formatter,
                "durable catalog {} appoints {artifact} {current}, which has no successor",
                path.display()
            ),
        }
    }
}

impl std::error::Error for DurableIoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } | Self::WriterLock { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl DurableIoError {
    fn is_interrupted(&self) -> bool {
        matches!(
            self,
            Self::Io { source, .. } | Self::WriterLock { source, .. }
                if source.kind() == io::ErrorKind::Interrupted
        )
    }

    pub(crate) fn is_missing_or_short_file(&self) -> bool {
        matches!(
            self,
            Self::Io { source, .. } if source.kind() == io::ErrorKind::NotFound
        ) || matches!(self, Self::Length { .. } | Self::ShortRead { .. })
    }
}

#[derive(Debug)]
pub(crate) struct DurableFileV1 {
    file: File,
    path: PathBuf,
}

impl DurableFileV1 {
    pub(crate) fn path(&self) -> &Path { &self.path }

    pub(crate) fn len(&self) -> Result<u64, DurableIoError> {
        self.file
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|source| self.io_error(DurableIoOperation::Metadata, source))
    }

    pub(crate) fn read_exact_at(
        &self,
        offset: u64,
        bytes: &mut [u8],
    ) -> Result<(), DurableIoError> {
        read_exact_at_with(&self.path, offset, bytes, |buffer, position| {
            positioned_read(&self.file, buffer, position, &self.path)
        })
    }

    pub(crate) fn require_length(
        &self,
        expected: FileLengthExpectation,
    ) -> Result<u64, DurableIoError> {
        let actual = self.len()?;
        let valid = match expected {
            FileLengthExpectation::Exact(expected) => actual == expected,
            FileLengthExpectation::AtLeast(expected) => actual >= expected,
        };
        if valid {
            Ok(actual)
        } else {
            Err(DurableIoError::Length {
                operation: DurableIoOperation::Metadata,
                path: self.path.clone(),
                expected,
                actual,
            })
        }
    }

    pub(crate) fn read_catalog_image(&self) -> Result<[u8; CATALOG_IMAGE_LEN], DurableIoError> {
        self.require_length(FileLengthExpectation::Exact(CATALOG_IMAGE_LEN as u64))?;
        let mut image = [0u8; CATALOG_IMAGE_LEN];
        self.read_exact_at(0, &mut image)?;
        Ok(image)
    }

    fn io_error(&self, operation: DurableIoOperation, source: io::Error) -> DurableIoError {
        DurableIoError::Io {
            operation,
            path: self.path.clone(),
            source,
        }
    }
}

/// A mutation capability borrowed exclusively from the process writer lease.
///
/// The exclusive lease borrow ensures that this handle cannot outlive the
/// lock and that the process cannot hold two independently mutable durable
/// files at once.  A transaction may still update pack, index, and catalog in
/// order by dropping each capability before opening the next.
#[derive(Debug)]
pub(crate) struct DurableWritableFileV1<'lease> {
    file: DurableFileV1,
    _lease: PhantomData<&'lease mut DurableWriterLeaseV1>,
}

impl DurableWritableFileV1<'_> {
    pub(crate) fn path(&self) -> &Path { self.file.path() }

    pub(crate) fn len(&self) -> Result<u64, DurableIoError> { self.file.len() }

    pub(crate) fn read_exact_at(
        &self,
        offset: u64,
        bytes: &mut [u8],
    ) -> Result<(), DurableIoError> {
        self.file.read_exact_at(offset, bytes)
    }

    pub(crate) fn require_length(
        &self,
        expected: FileLengthExpectation,
    ) -> Result<u64, DurableIoError> {
        self.file.require_length(expected)
    }

    pub(crate) fn read_catalog_image(&self) -> Result<[u8; CATALOG_IMAGE_LEN], DurableIoError> {
        self.file.read_catalog_image()
    }

    pub(crate) fn write_all_at(&mut self, offset: u64, bytes: &[u8]) -> Result<(), DurableIoError> {
        let path = self.file.path().to_path_buf();
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::PhysicalWrite,
            DurableIoOperation::PositionalWrite,
            &path,
        )?;
        write_all_at_with(self, &path, offset, bytes, |authority, buffer, position| {
            positioned_write(&authority.file.file, buffer, position, authority.file.path())
        })
    }

    pub(crate) fn sync_data(&mut self) -> Result<(), DurableIoError> {
        validate_authority_file(&self.file.file, DurableIoOperation::SyncData, self.file.path())?;
        self.file
            .file
            .sync_data()
            .map_err(|source| self.file.io_error(DurableIoOperation::SyncData, source))
    }

    pub(crate) fn sync_all(&mut self) -> Result<(), DurableIoError> {
        validate_authority_file(&self.file.file, DurableIoOperation::SyncFile, self.file.path())?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::PhysicalSync,
            DurableIoOperation::SyncFile,
            self.file.path(),
        )?;
        self.file
            .file
            .sync_all()
            .map_err(|source| self.file.io_error(DurableIoOperation::SyncFile, source))
    }

    pub(crate) fn set_len(&mut self, length: u64) -> Result<(), DurableIoError> {
        self.file
            .file
            .set_len(length)
            .map_err(|source| self.file.io_error(DurableIoOperation::SetLength, source))
    }

    pub(crate) fn write_catalog_slot(
        &mut self,
        slot: CatalogSlotIndexV1,
        bytes: &[u8; CATALOG_SLOT_LEN],
    ) -> Result<(), DurableIoError> {
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::CatalogSlotWrite,
            DurableIoOperation::PositionalWrite,
            self.file.path(),
        )?;
        self.require_length(FileLengthExpectation::Exact(CATALOG_IMAGE_LEN as u64))?;
        #[cfg(test)]
        if let Some(prefix_len) = CATALOG_SLOT_WRITE_PREFIX.with(|armed| armed.take()) {
            if prefix_len != 0 {
                self.write_all_at(slot.offset(), &bytes[..prefix_len])?;
            }
            return Err(DurableIoError::Io {
                operation: DurableIoOperation::PositionalWrite,
                path: self.file.path().to_path_buf(),
                source: io::Error::other(format!(
                    "injected torn catalog-slot write after {prefix_len} bytes"
                )),
            });
        }
        self.write_all_at(slot.offset(), bytes)
    }

    pub(crate) fn sync_catalog_all(&mut self) -> Result<(), DurableIoError> {
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::CatalogSync,
            DurableIoOperation::SyncFile,
            self.file.path(),
        )?;
        self.sync_all()
    }
}

const DURABLE_APPEND_BUFFER_LEN: usize = 64 * 1024;

/// Bounded, exact-extent canonical stream over one positional durable file.
///
/// Large canonical records are never materialized. Small encoder writes are
/// coalesced into one fallibly allocated buffer, while chunks at least as
/// large as that buffer bypass it. Every accepted byte is checked against the
/// appointed end before any write can extend the file beyond authority.
pub(crate) struct DurableCanonicalAppendSinkV1<'file, 'lease> {
    file: &'file mut DurableWritableFileV1<'lease>,
    flushed_end: u64,
    appointed_end: u64,
    buffer: Vec<u8>,
}

impl<'file, 'lease> DurableCanonicalAppendSinkV1<'file, 'lease> {
    pub(crate) fn new(
        file: &'file mut DurableWritableFileV1<'lease>,
        start: u64,
        appointed_end: u64,
    ) -> Result<Self, DurableIoError> {
        let span =
            appointed_end
                .checked_sub(start)
                .ok_or_else(|| DurableIoError::AppendOverrun {
                    operation: DurableIoOperation::PositionalWrite,
                    path: file.path().to_path_buf(),
                    cursor: start,
                    offered: 0,
                    appointed_end,
                })?;
        let capacity = usize::try_from(span.min(DURABLE_APPEND_BUFFER_LEN as u64))
            .expect("bounded append capacity fits usize");
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(capacity)
            .map_err(|_| DurableIoError::Allocation {
                operation: DurableIoOperation::PositionalWrite,
                path: file.path().to_path_buf(),
                requested: capacity as u64,
            })?;
        Ok(Self {
            file,
            flushed_end: start,
            appointed_end,
            buffer,
        })
    }

    pub(crate) fn cursor(&self) -> u64 {
        self.flushed_end
            .checked_add(self.buffer.len() as u64)
            .expect("accepted append extent was checked before buffering")
    }

    fn flush_buffer(&mut self) -> Result<(), DurableIoError> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        self.file.write_all_at(self.flushed_end, &self.buffer)?;
        self.flushed_end = self
            .flushed_end
            .checked_add(self.buffer.len() as u64)
            .ok_or_else(|| DurableIoError::OffsetOverflow {
                operation: DurableIoOperation::PositionalWrite,
                path: self.file.path().to_path_buf(),
                offset: self.flushed_end,
                completed: self.buffer.len() as u64,
            })?;
        self.buffer.clear();
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<u64, DurableIoError> {
        self.flush_buffer()?;
        if self.flushed_end != self.appointed_end {
            return Err(DurableIoError::AppendUnderrun {
                operation: DurableIoOperation::PositionalWrite,
                path: self.file.path().to_path_buf(),
                actual_end: self.flushed_end,
                appointed_end: self.appointed_end,
            });
        }
        self.file
            .require_length(FileLengthExpectation::Exact(self.appointed_end))?;
        Ok(self.flushed_end)
    }
}

impl CanonicalSink for DurableCanonicalAppendSinkV1<'_, '_> {
    type Error = DurableIoError;

    fn write_all(&mut self, mut bytes: &[u8]) -> Result<(), Self::Error> {
        let cursor = self.cursor();
        let offered = u64::try_from(bytes.len()).map_err(|_| DurableIoError::OffsetOverflow {
            operation: DurableIoOperation::PositionalWrite,
            path: self.file.path().to_path_buf(),
            offset: cursor,
            completed: u64::MAX,
        })?;
        let end = cursor
            .checked_add(offered)
            .ok_or_else(|| DurableIoError::OffsetOverflow {
                operation: DurableIoOperation::PositionalWrite,
                path: self.file.path().to_path_buf(),
                offset: cursor,
                completed: offered,
            })?;
        if end > self.appointed_end {
            return Err(DurableIoError::AppendOverrun {
                operation: DurableIoOperation::PositionalWrite,
                path: self.file.path().to_path_buf(),
                cursor,
                offered,
                appointed_end: self.appointed_end,
            });
        }

        while !bytes.is_empty() {
            if self.buffer.is_empty() && bytes.len() >= DURABLE_APPEND_BUFFER_LEN {
                self.file.write_all_at(self.flushed_end, bytes)?;
                self.flushed_end = end;
                return Ok(());
            }
            let available = self.buffer.capacity() - self.buffer.len();
            if available == 0 {
                self.flush_buffer()?;
                continue;
            }
            let take = available.min(bytes.len());
            self.buffer.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
        }
        Ok(())
    }
}

impl super::recovery::DurableReadAt for DurableFileV1 {
    type Error = DurableIoError;

    fn durable_len(&self) -> Result<u64, Self::Error> { self.len() }

    fn read_exact_at(&self, offset: u64, destination: &mut [u8]) -> Result<(), Self::Error> {
        DurableFileV1::read_exact_at(self, offset, destination)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CatalogSlotIndexV1 {
    First,
    Second,
}

impl CatalogSlotIndexV1 {
    const fn offset(self) -> u64 {
        match self {
            Self::First => 0,
            Self::Second => CATALOG_SLOT_LEN as u64,
        }
    }
}

#[derive(Debug)]
pub(crate) struct DurableDirectoryV1 {
    path: PathBuf,
    directory: File,
    generation_leases: GenerationLeaseRegistryV1,
    retention_reader: RetentionReader,
    retention_writer: RetentionFormat,
}

impl DurableDirectoryV1 {
    pub(crate) fn open(path: impl Into<PathBuf>) -> Result<Self, DurableIoError> {
        let path = absolute_diagnostic_path(path.into())?;
        let directory = open_directory_handle(&path)?;
        #[cfg(unix)]
        let generation_leases = {
            use std::os::unix::fs::MetadataExt;

            let metadata = directory.metadata().map_err(|source| DurableIoError::Io {
                operation: DurableIoOperation::Metadata,
                path: path.clone(),
                source,
            })?;
            let guard = directory.try_clone().map_err(|source| DurableIoError::Io {
                operation: DurableIoOperation::OpenDirectory,
                path: path.clone(),
                source,
            })?;
            GenerationLeaseRegistryV1::for_directory_identity(
                (metadata.dev(), metadata.ino()),
                guard,
            )
            .map_err(|error| DurableIoError::Io {
                operation: DurableIoOperation::Metadata,
                path: path.clone(),
                source: io::Error::other(error),
            })?
        };
        #[cfg(not(unix))]
        let generation_leases = GenerationLeaseRegistryV1::default();
        Ok(Self {
            path,
            directory,
            generation_leases,
            retention_reader: RetentionReader::Dual,
            retention_writer: RetentionFormat::V2,
        })
    }

    pub(crate) fn path(&self) -> &Path { &self.path }

    /// The retention formats recovery and admission accept from this
    /// directory. Opened directories read both formats.
    pub(crate) const fn retention_reader(&self) -> RetentionReader { self.retention_reader }

    /// Downgrade this directory to a strict V1 reader. With the V1 writer, it
    /// stands in for a node built before the V2 retention reader existed.
    #[cfg(test)]
    pub(crate) fn with_retention_reader_for_test(mut self, reader: RetentionReader) -> Self {
        self.retention_reader = reader;
        self
    }

    /// The retention format in which this directory's writer seals successors
    /// and generation checkpoints. Opened directories write V2.
    pub(crate) const fn retention_writer(&self) -> RetentionFormat { self.retention_writer }

    /// Select the retention writer. The V1 writer stands in for a node built
    /// before the V2 retention writer was activated.
    #[cfg(test)]
    pub(crate) fn with_retention_writer_for_test(mut self, writer: RetentionFormat) -> Self {
        self.retention_writer = writer;
        self
    }

    pub(crate) fn generation_leases(&self) -> GenerationLeaseRegistryV1 {
        self.generation_leases.clone()
    }

    /// Duplicate the already-open directory capability without resolving its
    /// diagnostic path again.  Recovery uses this reader while a writable
    /// temporary index holds the exclusive lease borrow, so both capabilities
    /// remain pinned to the same directory object even if its pathname is
    /// concurrently replaced.
    pub(crate) fn try_clone_reader(&self) -> Result<Self, DurableIoError> {
        let directory = self
            .directory
            .try_clone()
            .map_err(|source| DurableIoError::Io {
                operation: DurableIoOperation::OpenDirectory,
                path: self.path.clone(),
                source,
            })?;
        Ok(Self {
            path: self.path.clone(),
            directory,
            generation_leases: self.generation_leases.clone(),
            retention_reader: self.retention_reader,
            retention_writer: self.retention_writer,
        })
    }

    pub(crate) fn try_acquire_writer(self) -> Result<DurableWriterLeaseV1, DurableIoError> {
        let lock_path = self.path.join(WRITER_LOCK_FILE_NAME_V1);
        let lock = match self.open_child(WRITER_LOCK_FILE_NAME_V1, ChildOpenMode::CreateNew) {
            Err(DurableIoError::Io { source, .. })
                if source.kind() == io::ErrorKind::AlreadyExists =>
            {
                self.open_child(WRITER_LOCK_FILE_NAME_V1, ChildOpenMode::ReadWrite)?
            }
            result => result?,
        };
        match lock.file.try_lock() {
            Ok(()) => Ok(DurableWriterLeaseV1 {
                directory: self,
                _lock: lock,
            }),
            Err(TryLockError::WouldBlock) => Err(DurableIoError::WriterLockContended {
                operation: DurableIoOperation::AcquireWriterLock,
                path: lock_path,
            }),
            Err(TryLockError::Error(source)) => Err(DurableIoError::WriterLock {
                operation: DurableIoOperation::AcquireWriterLock,
                path: lock_path,
                source,
            }),
        }
    }

    pub(crate) fn open_catalog(&self) -> Result<DurableFileV1, DurableIoError> {
        self.open_existing(
            CATALOG_FILE_NAME_V1,
            false,
            FileLengthExpectation::Exact(CATALOG_IMAGE_LEN as u64),
        )
    }

    pub(crate) fn open_pack(&self, pack: PackId) -> Result<DurableFileV1, DurableIoError> {
        self.open_existing(
            &pack_file_name(pack),
            false,
            FileLengthExpectation::AtLeast(PACK_HEADER_LEN_V1),
        )
    }

    pub(crate) fn open_index(
        &self,
        generation: IndexGenerationV1,
    ) -> Result<DurableFileV1, DurableIoError> {
        self.open_existing(
            &index_file_name(generation),
            false,
            FileLengthExpectation::AtLeast(INDEX_HEADER_LEN_V1),
        )
    }

    pub(crate) fn store_file_names(&self) -> Result<Vec<String>, DurableIoError> {
        require_path_names_pinned_directory(&self.directory, &self.path)?;
        let read_error = |source| DurableIoError::Io {
            operation: DurableIoOperation::ReadDirectory,
            path: self.path.clone(),
            source,
        };
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&self.path).map_err(read_error)? {
            let name = entry.map_err(read_error)?.file_name();
            match name.to_str() {
                Some(name) if is_store_file_name(name) => names.push(name.to_owned()),
                _ => {}
            }
        }
        require_path_names_pinned_directory(&self.directory, &self.path)?;
        names.sort_unstable();
        Ok(names)
    }

    fn create_new(&self, name: &str) -> Result<DurableFileV1, DurableIoError> {
        self.open_child(name, ChildOpenMode::CreateNew)
    }

    fn open_existing(
        &self,
        name: &str,
        writable: bool,
        expected: FileLengthExpectation,
    ) -> Result<DurableFileV1, DurableIoError> {
        let mode = if writable {
            ChildOpenMode::ReadWrite
        } else {
            ChildOpenMode::ReadOnly
        };
        let file = self.open_child(name, mode)?;
        file.require_length(expected)?;
        Ok(file)
    }

    fn open_child(&self, name: &str, mode: ChildOpenMode) -> Result<DurableFileV1, DurableIoError> {
        let path = self.path.join(name);
        let operation = mode.operation();
        if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
            return Err(DurableIoError::Io {
                operation,
                path,
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "durable authority name must be one ordinary path component",
                ),
            });
        }
        let file = open_child_handle(&self.directory, name, mode).map_err(|source| {
            DurableIoError::Io {
                operation,
                path: path.clone(),
                source,
            }
        })?;
        if matches!(mode, ChildOpenMode::CreateNew) {
            set_owner_only_permissions(&file, operation, &path)?;
        }
        validate_authority_file(&file, operation, &path)?;
        Ok(DurableFileV1 { file, path })
    }
}

#[derive(Debug)]
pub(crate) struct DurableWriterLeaseV1 {
    directory: DurableDirectoryV1,
    _lock: DurableFileV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReclamationOutcomeV1 {
    pub(crate) deleted_packs: u64,
    pub(crate) deleted_index: bool,
    pub(crate) deferred_for_reader: bool,
}

impl Drop for DurableWriterLeaseV1 {
    fn drop(&mut self) {
        // `File` closes on drop, but a concurrently spawned process or an
        // explicit descriptor duplicate can retain the same open file
        // description past this affine Rust lifetime.  Unlock the shared open
        // description first so no duplicate can extend mutation authority.
        // Closing this descriptor remains the fail-safe if the OS reports an
        // unlock error during an infallible destructor.
        let _ = self._lock.file.unlock();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UnpublishedSuccessorArtifactsV1 {
    pack: PackId,
    index: IndexGenerationV1,
    removed_pack: bool,
    removed_index: bool,
}

impl UnpublishedSuccessorArtifactsV1 {
    pub(crate) const fn pack(self) -> PackId { self.pack }

    pub(crate) const fn index(self) -> IndexGenerationV1 { self.index }

    #[cfg(test)]
    pub(crate) const fn removed_pack(self) -> bool { self.removed_pack }

    #[cfg(test)]
    pub(crate) const fn removed_index(self) -> bool { self.removed_index }
}

/// Unpublished index bytes tied to the exclusive writer-lease lifetime.
pub(crate) struct DurableRebuildCandidateV1<'lease> {
    file: DurableWritableFileV1<'lease>,
    directory: File,
    directory_path: PathBuf,
    generation: IndexGenerationV1,
}

/// A rebuild candidate whose exact length, data, and metadata have been
/// synchronized. Atomic publication is available only from this state.
pub(crate) struct DurableSyncedRebuildCandidateV1<'lease>(DurableRebuildCandidateV1<'lease>);

impl<'lease> DurableRebuildCandidateV1<'lease> {
    pub(crate) fn write_all_at(&mut self, offset: u64, bytes: &[u8]) -> Result<(), DurableIoError> {
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::CandidateWrite,
            DurableIoOperation::PositionalWrite,
            self.file.path(),
        )?;
        self.file.write_all_at(offset, bytes)
    }

    pub(crate) fn sync_and_seal(
        mut self,
        exact_length: u64,
    ) -> Result<DurableSyncedRebuildCandidateV1<'lease>, DurableIoError> {
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::CandidateSetLength,
            DurableIoOperation::SetLength,
            self.file.path(),
        )?;
        self.file.set_len(exact_length)?;
        self.file
            .require_length(FileLengthExpectation::Exact(exact_length))?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::CandidateSync,
            DurableIoOperation::SyncFile,
            self.file.path(),
        )?;
        self.file.sync_all()?;
        Ok(DurableSyncedRebuildCandidateV1(self))
    }
}

impl DurableSyncedRebuildCandidateV1<'_> {
    /// Atomically install the synchronized candidate, synchronize the pinned
    /// directory, and return the same inode as a read-only capability for
    /// post-publication verification.
    pub(crate) fn publish_replace(self) -> Result<DurableFileV1, DurableIoError> {
        let mut candidate = self.0;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::BeforeReplace,
            DurableIoOperation::ReplaceFile,
            candidate.file.path(),
        )?;
        validate_authority_file(
            &candidate.file.file.file,
            DurableIoOperation::ReplaceFile,
            candidate.file.path(),
        )?;
        let temporary = index_rebuild_file_name(candidate.generation);
        ensure_child_matches_handle(
            &candidate.directory,
            &temporary,
            &candidate.file.file.file,
            &candidate.directory_path,
            DurableIoOperation::ReplaceFile,
        )?;
        let final_name = index_file_name(candidate.generation);
        replace_child_handle(&candidate.directory, &temporary, &final_name).map_err(|source| {
            DurableIoError::Io {
                operation: DurableIoOperation::ReplaceFile,
                path: candidate.directory_path.join(&final_name),
                source,
            }
        })?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::AfterReplaceBeforeDirectorySync,
            DurableIoOperation::SyncDirectory,
            &candidate.directory_path,
        )?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::DirectorySync,
            DurableIoOperation::SyncDirectory,
            &candidate.directory_path,
        )?;
        candidate
            .directory
            .sync_all()
            .map_err(|source| DurableIoError::Io {
                operation: DurableIoOperation::SyncDirectory,
                path: candidate.directory_path.clone(),
                source,
            })?;
        candidate.file.file.path = candidate.directory_path.join(final_name);
        Ok(candidate.file.file)
    }
}

impl DurableWriterLeaseV1 {
    pub(crate) fn directory(&self) -> &DurableDirectoryV1 { &self.directory }

    pub(crate) fn generation_leases(&self) -> GenerationLeaseRegistryV1 {
        self.directory.generation_leases()
    }

    /// Publish the advisory reclamation record without ever exposing a torn
    /// record under its final name: write and synchronize it under a temporary
    /// name, rename it to the final name without replacing an existing record,
    /// and synchronize the directory. A failure before the rename leaves only
    /// the temporary, which recovery removes. Readers still validate the exact
    /// length, version, checksum, and catalog identity before using it.
    pub(crate) fn write_reclamation_candidate(
        &mut self,
        candidate: ReclamationCandidateV1,
    ) -> Result<(), DurableIoError> {
        let temporary = RECLAMATION_CANDIDATE_TEMPORARY_FILE_NAME_V1;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::ReclamationIntentCreate,
            DurableIoOperation::CreateFile,
            &self.directory.path.join(temporary),
        )?;
        let mut file = DurableWritableFileV1 {
            file: self.directory.create_new(temporary)?,
            _lease: PhantomData,
        };
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::ReclamationIntentSetLength,
            DurableIoOperation::SetLength,
            file.path(),
        )?;
        file.set_len(RECLAMATION_RECORD_LEN_V1 as u64)?;
        let encoded = candidate.encode();
        #[cfg(test)]
        if let Some(prefix_len) = RECLAMATION_INTENT_WRITE_PREFIX.with(|armed| armed.take()) {
            if prefix_len != 0 {
                file.write_all_at(0, &encoded[..prefix_len])?;
            }
            return Err(DurableIoError::Io {
                operation: DurableIoOperation::PositionalWrite,
                path: file.path().to_path_buf(),
                source: io::Error::other(format!(
                    "injected torn reclamation-intent write after {prefix_len} bytes"
                )),
            });
        }
        file.write_all_at(0, &encoded)?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::ReclamationIntentSync,
            DurableIoOperation::SyncFile,
            file.path(),
        )?;
        file.sync_all()?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::ReclamationIntentBeforeRename,
            DurableIoOperation::PublishFile,
            file.path(),
        )?;
        validate_authority_file(&file.file.file, DurableIoOperation::PublishFile, file.path())?;
        ensure_child_matches_handle(
            &self.directory.directory,
            temporary,
            &file.file.file,
            &self.directory.path,
            DurableIoOperation::PublishFile,
        )?;
        publish_child_handle_without_replace(
            &self.directory.directory,
            temporary,
            RECLAMATION_CANDIDATE_FILE_NAME_V1,
        )
        .map_err(|source| DurableIoError::Io {
            operation: DurableIoOperation::PublishFile,
            path: self.directory.path.join(RECLAMATION_CANDIDATE_FILE_NAME_V1),
            source,
        })?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::ReclamationIntentDirectorySync,
            DurableIoOperation::SyncDirectory,
            &self.directory.path,
        )?;
        self.sync_directory()
    }

    pub(crate) fn read_reclamation_candidate(
        &self,
    ) -> Result<Option<ReclamationCandidateV1>, DurableIoError> {
        let file = match self
            .directory
            .open_child(RECLAMATION_CANDIDATE_FILE_NAME_V1, ChildOpenMode::ReadOnly)
        {
            Ok(file) => file,
            Err(DurableIoError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        file.require_length(FileLengthExpectation::Exact(RECLAMATION_RECORD_LEN_V1 as u64))?;
        let mut bytes = vec![0u8; RECLAMATION_RECORD_LEN_V1];
        file.read_exact_at(0, &mut bytes)?;
        ReclamationCandidateV1::decode(&bytes)
            .map(Some)
            .map_err(|message| DurableIoError::Io {
                operation: DurableIoOperation::Metadata,
                path: file.path().to_path_buf(),
                source: io::Error::new(io::ErrorKind::InvalidData, message),
            })
    }

    pub(crate) fn remove_reclamation_candidate(&mut self) -> Result<(), DurableIoError> {
        let name = RECLAMATION_CANDIDATE_FILE_NAME_V1;
        let opened = self.directory.open_child(name, ChildOpenMode::ReadOnly)?;
        ensure_child_matches_handle(
            &self.directory.directory,
            name,
            &opened.file,
            &self.directory.path,
            DurableIoOperation::RemoveFile,
        )?;
        self.remove_child(name)?;
        self.sync_directory()
    }

    pub(crate) fn remove_reclamation_temporary(&mut self) -> Result<(), DurableIoError> {
        let name = RECLAMATION_CANDIDATE_TEMPORARY_FILE_NAME_V1;
        let opened = match self.directory.open_child(name, ChildOpenMode::ReadOnly) {
            Ok(opened) => opened,
            Err(DurableIoError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        ensure_child_matches_handle(
            &self.directory.directory,
            name,
            &opened.file,
            &self.directory.path,
            DurableIoOperation::RemoveFile,
        )?;
        self.remove_child(name)?;
        self.sync_directory()
    }

    pub(crate) fn resolve_reclamation_before_checkpoint(
        &mut self,
        selected: CatalogSlotV1,
    ) -> Result<(), DurableIoError> {
        let Some(candidate) = self.read_reclamation_candidate()? else {
            return Ok(());
        };
        match candidate.relation_to_catalog(selected) {
            ReclamationCatalogRelationV1::PreCas => self.remove_reclamation_candidate(),
            ReclamationCatalogRelationV1::SelectedSuccessor => {
                if self
                    .reclaim_reclamation_candidate(candidate, selected)?
                    .deferred_for_reader
                {
                    Err(DurableIoError::ReclamationPending {
                        path: self.directory.path.join(RECLAMATION_CANDIDATE_FILE_NAME_V1),
                    })
                } else {
                    Ok(())
                }
            }
            ReclamationCatalogRelationV1::Unrelated => {
                Err(DurableIoError::CatalogAuthorityChanged {
                    path: self.directory.path.clone(),
                })
            }
        }
    }

    /// Reclaim exactly the superseded generation named by an authenticated
    /// candidate. The catalog image must already have selected the candidate's
    /// successor; active leases on the old generation defer deletion without
    /// consuming the sidecar.
    pub(crate) fn reclaim_reclamation_candidate(
        &mut self,
        candidate: ReclamationCandidateV1,
        selected_successor: CatalogSlotV1,
    ) -> Result<ReclamationOutcomeV1, DurableIoError> {
        if candidate.relation_to_catalog(selected_successor) !=
            ReclamationCatalogRelationV1::SelectedSuccessor
        {
            return Err(DurableIoError::CatalogAuthorityChanged {
                path: self.directory.path.clone(),
            });
        }
        // This check and the exclusion of future old-generation leases must
        // linearize as one registry operation. A separate has_active check
        // permits acquisition between the check and the first unlink.
        if !self
            .generation_leases()
            .retire_pack_range_if_unleased(candidate.old())
            .map_err(|error| DurableIoError::Io {
                operation: DurableIoOperation::Metadata,
                path: self.directory.path.clone(),
                source: io::Error::other(error.to_string()),
            })?
        {
            return Ok(ReclamationOutcomeV1 {
                deleted_packs: 0,
                deleted_index: false,
                deferred_for_reader: true,
            });
        }

        let old = candidate.old();
        let mut deleted_packs = 0u64;
        let mut pack = old.first_live_pack();
        while pack <= old.active_pack() {
            let name = pack_file_name(PackId::new(pack).expect("validated nonzero pack id"));
            match self.remove_child(&name) {
                Ok(()) => {
                    deleted_packs = deleted_packs.checked_add(1).ok_or_else(|| {
                        DurableIoError::SuccessorOverflow {
                            path: self.directory.path.clone(),
                            artifact: "reclamation pack count",
                            current: deleted_packs,
                        }
                    })?
                }
                Err(DurableIoError::Io { source, .. })
                    if source.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            pack = pack
                .checked_add(1)
                .ok_or_else(|| DurableIoError::SuccessorOverflow {
                    path: self.directory.path.clone(),
                    artifact: "reclamation pack range",
                    current: pack,
                })?;
        }

        let index_name = index_file_name(
            IndexGenerationV1::new(old.index_generation()).expect("validated index generation"),
        );
        let deleted_index = match self.remove_child(&index_name) {
            Ok(()) => true,
            Err(DurableIoError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                false
            }
            Err(error) => return Err(error),
        };
        self.sync_directory()?;
        self.remove_reclamation_candidate()?;
        Ok(ReclamationOutcomeV1 {
            deleted_packs,
            deleted_index,
            deferred_for_reader: false,
        })
    }

    /// Remove one validated child while retaining compiler-visible exclusive
    /// writer authority at the physical mutation boundary.
    fn remove_child(&mut self, name: &str) -> Result<(), DurableIoError> {
        remove_child_handle(&self.directory.directory, name).map_err(|source| DurableIoError::Io {
            operation: DurableIoOperation::RemoveFile,
            path: self.directory.path.join(name),
            source,
        })
    }

    pub(crate) fn create_catalog(&mut self) -> Result<DurableWritableFileV1<'_>, DurableIoError> {
        let file = self.directory.create_new(CATALOG_FILE_NAME_V1)?;
        file.file
            .set_len(CATALOG_IMAGE_LEN as u64)
            .map_err(|source| file.io_error(DurableIoOperation::SetLength, source))?;
        Ok(DurableWritableFileV1 {
            file,
            _lease: PhantomData,
        })
    }

    pub(crate) fn open_catalog(&mut self) -> Result<DurableWritableFileV1<'_>, DurableIoError> {
        let file = self.directory.open_existing(
            CATALOG_FILE_NAME_V1,
            true,
            FileLengthExpectation::Exact(CATALOG_IMAGE_LEN as u64),
        )?;
        Ok(DurableWritableFileV1 {
            file,
            _lease: PhantomData,
        })
    }

    pub(crate) fn create_pack(
        &mut self,
        pack: PackId,
    ) -> Result<DurableWritableFileV1<'_>, DurableIoError> {
        let file = self.directory.create_new(&pack_file_name(pack))?;
        Ok(DurableWritableFileV1 {
            file,
            _lease: PhantomData,
        })
    }

    pub(crate) fn open_pack(
        &mut self,
        pack: PackId,
    ) -> Result<DurableWritableFileV1<'_>, DurableIoError> {
        let file = self.directory.open_existing(
            &pack_file_name(pack),
            true,
            FileLengthExpectation::AtLeast(PACK_HEADER_LEN_V1),
        )?;
        Ok(DurableWritableFileV1 {
            file,
            _lease: PhantomData,
        })
    }

    pub(crate) fn create_index(
        &mut self,
        generation: IndexGenerationV1,
    ) -> Result<DurableWritableFileV1<'_>, DurableIoError> {
        let file = self.directory.create_new(&index_file_name(generation))?;
        Ok(DurableWritableFileV1 {
            file,
            _lease: PhantomData,
        })
    }

    /// Discard any crash-left rebuild candidate and create a fresh temporary
    /// index in the pinned durable directory. The returned typestate retains
    /// its open handle through synchronization and atomic publication.
    pub(crate) fn create_rebuilt_index(
        &mut self,
        generation: IndexGenerationV1,
    ) -> Result<DurableRebuildCandidateV1<'_>, DurableIoError> {
        let name = index_rebuild_file_name(generation);
        match self.remove_child(&name) {
            Ok(()) => {}
            Err(DurableIoError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let file = self.directory.create_new(&name)?;
        let directory =
            self.directory
                .directory
                .try_clone()
                .map_err(|source| DurableIoError::Io {
                    operation: DurableIoOperation::OpenDirectory,
                    path: self.directory.path.clone(),
                    source,
                })?;
        Ok(DurableRebuildCandidateV1 {
            file: DurableWritableFileV1 {
                file,
                _lease: PhantomData,
            },
            directory,
            directory_path: self.directory.path.clone(),
            generation,
        })
    }

    pub(crate) fn open_index(
        &mut self,
        generation: IndexGenerationV1,
    ) -> Result<DurableWritableFileV1<'_>, DurableIoError> {
        let file = self.directory.open_existing(
            &index_file_name(generation),
            true,
            FileLengthExpectation::AtLeast(INDEX_HEADER_LEN_V1),
        )?;
        Ok(DurableWritableFileV1 {
            file,
            _lease: PhantomData,
        })
    }

    /// Derive and clear only the exact successor names that are unreachable
    /// from one retained catalog authority.
    ///
    /// All 672 catalog bytes are compared before any removal. Both possible
    /// entries are then opened relative to the pinned directory without
    /// following links and validated before either is unlinked. Thus callers
    /// cannot select arbitrary or current-authority names, and a symlink,
    /// FIFO, device, hard link, unsafe owner/mode, or concurrent substitution
    /// fails closed. Successful removal is directory-synchronized before the
    /// final names may be recreated.
    pub(crate) fn prepare_unpublished_successor_artifacts(
        &mut self,
        authority: &CatalogAuthorityTokenV1,
    ) -> Result<UnpublishedSuccessorArtifactsV1, DurableIoError> {
        let catalog_path = self.directory.path.join(CATALOG_FILE_NAME_V1);
        let current_image = self.directory.open_catalog()?.read_catalog_image()?;
        if !authority.matches_exactly(&current_image) {
            return Err(DurableIoError::CatalogAuthorityChanged { path: catalog_path });
        }

        let current = authority.slot().pack();
        let pack_value = current.active_pack().checked_add(1).ok_or_else(|| {
            DurableIoError::SuccessorOverflow {
                path: self.directory.path.clone(),
                artifact: "pack",
                current: current.active_pack(),
            }
        })?;
        let pack = PackId::new(pack_value).expect("checked nonzero pack successor is valid");
        let index_value = current.index_generation().checked_add(1).ok_or_else(|| {
            DurableIoError::SuccessorOverflow {
                path: self.directory.path.clone(),
                artifact: "index generation",
                current: current.index_generation(),
            }
        })?;
        let index = IndexGenerationV1::new(index_value)
            .expect("checked nonzero index-generation successor is valid");
        let pack_name = pack_file_name(pack);
        let index_name = index_file_name(index);

        let open_optional = |name: &str| match self
            .directory
            .open_child(name, ChildOpenMode::ReadOnly)
        {
            Ok(opened) => Ok(Some(opened)),
            Err(DurableIoError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(error),
        };
        let pack_opened = open_optional(&pack_name)?;
        let index_opened = open_optional(&index_name)?;

        for (name, opened) in
            [(&pack_name, pack_opened.as_ref()), (&index_name, index_opened.as_ref())]
        {
            if let Some(opened) = opened {
                ensure_child_matches_handle(
                    &self.directory.directory,
                    name,
                    &opened.file,
                    &self.directory.path,
                    DurableIoOperation::RemoveFile,
                )?;
            }
        }

        let removed_pack = if pack_opened.is_some() {
            self.remove_child(&pack_name)?;
            true
        } else {
            false
        };
        let removed_index = if index_opened.is_some() {
            self.remove_child(&index_name)?;
            true
        } else {
            false
        };
        if removed_pack || removed_index {
            self.sync_directory()?;
        }
        Ok(UnpublishedSuccessorArtifactsV1 {
            pack,
            index,
            removed_pack,
            removed_index,
        })
    }

    pub(crate) fn sync_directory(&mut self) -> Result<(), DurableIoError> {
        self.directory
            .directory
            .sync_all()
            .map_err(|source| DurableIoError::Io {
                operation: DurableIoOperation::SyncDirectory,
                path: self.directory.path.clone(),
                source,
            })
    }

    pub(crate) fn sync_catalog_directory(&mut self) -> Result<(), DurableIoError> {
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::CatalogDirectorySync,
            DurableIoOperation::SyncDirectory,
            self.directory.path(),
        )?;
        self.sync_directory()
    }
}

pub(crate) fn pack_file_name(pack: PackId) -> String {
    format!("{PACK_FILE_PREFIX_V1}{:020}{PACK_FILE_SUFFIX_V1}", pack.get())
}

pub(crate) fn index_file_name(generation: IndexGenerationV1) -> String {
    format!("{INDEX_FILE_PREFIX_V1}{:020}{INDEX_FILE_SUFFIX_V1}", generation.get())
}

fn index_rebuild_file_name(generation: IndexGenerationV1) -> String {
    format!("{}.rebuild", index_file_name(generation))
}

fn is_store_file_name(name: &str) -> bool {
    match name {
        CATALOG_FILE_NAME_V1 |
        RECLAMATION_CANDIDATE_FILE_NAME_V1 |
        RECLAMATION_CANDIDATE_TEMPORARY_FILE_NAME_V1 => true,
        _ => is_pack_file_name(name) || is_index_file_name(name),
    }
}

fn store_file_number(name: &str, prefix: &str, suffix: &str) -> Option<u64> {
    let (digits, _) = name.strip_prefix(prefix)?.split_once(suffix)?;
    digits.parse().ok()
}

fn is_pack_file_name(name: &str) -> bool {
    matches!(
        store_file_number(name, PACK_FILE_PREFIX_V1, PACK_FILE_SUFFIX_V1).map(PackId::new),
        Some(Ok(pack)) if pack_file_name(pack) == name
    )
}

fn is_index_file_name(name: &str) -> bool {
    matches!(
        store_file_number(name, INDEX_FILE_PREFIX_V1, INDEX_FILE_SUFFIX_V1)
            .map(IndexGenerationV1::new),
        Some(Ok(generation))
            if index_file_name(generation) == name || index_rebuild_file_name(generation) == name
    )
}

#[derive(Clone, Copy, Debug)]
enum ChildOpenMode {
    ReadOnly,
    ReadWrite,
    CreateNew,
}

impl ChildOpenMode {
    const fn operation(self) -> DurableIoOperation {
        match self {
            Self::CreateNew => DurableIoOperation::CreateFile,
            _ => DurableIoOperation::OpenFile,
        }
    }
}

fn absolute_diagnostic_path(path: PathBuf) -> Result<PathBuf, DurableIoError> {
    if path.is_absolute() {
        return Ok(path);
    }
    std::env::current_dir()
        .map(|current| current.join(&path))
        .map_err(|source| DurableIoError::Io {
            operation: DurableIoOperation::OpenDirectory,
            path,
            source,
        })
}

#[cfg(unix)]
pub(super) fn open_directory_handle(path: &Path) -> Result<File, DurableIoError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|source| DurableIoError::Io {
            operation: DurableIoOperation::OpenDirectory,
            path: path.to_path_buf(),
            source,
        })?;
    let metadata = file.metadata().map_err(|source| DurableIoError::Io {
        operation: DurableIoOperation::Metadata,
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(DurableIoError::Io {
            operation: DurableIoOperation::OpenDirectory,
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::NotADirectory, "not a directory"),
        });
    }
    // A pinned descriptor prevents pathname substitution, but another user
    // must also be unable to mutate entries inside the directory. Read/search
    // permission is harmless because every authority file is owner-only.
    let mode = metadata.permissions().mode();
    if mode & 0o022 != 0 {
        return Err(DurableIoError::UnsafePermissions {
            operation: DurableIoOperation::OpenDirectory,
            path: path.to_path_buf(),
            mode,
        });
    }
    // SAFETY: geteuid has no preconditions and returns the caller's effective
    // Unix user identifier.
    let expected = unsafe { libc::geteuid() };
    if metadata.uid() != expected {
        return Err(DurableIoError::UnsafeOwner {
            operation: DurableIoOperation::OpenDirectory,
            path: path.to_path_buf(),
            expected,
            actual: metadata.uid(),
        });
    }
    Ok(file)
}

#[cfg(not(unix))]
pub(super) fn open_directory_handle(path: &Path) -> Result<File, DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation: DurableIoOperation::OpenDirectory,
        path: path.to_path_buf(),
    })
}

#[cfg(unix)]
fn require_path_names_pinned_directory(
    directory: &File,
    path: &Path,
) -> Result<(), DurableIoError> {
    use std::os::unix::fs::MetadataExt;

    let metadata_error = |source| DurableIoError::Io {
        operation: DurableIoOperation::Metadata,
        path: path.to_path_buf(),
        source,
    };
    let pinned = directory.metadata().map_err(metadata_error)?;
    let named = std::fs::symlink_metadata(path).map_err(metadata_error)?;
    if (named.dev(), named.ino()) != (pinned.dev(), pinned.ino()) {
        return Err(DurableIoError::Io {
            operation: DurableIoOperation::ReadDirectory,
            path: path.to_path_buf(),
            source: io::Error::other("the path no longer names the pinned durable directory"),
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn require_path_names_pinned_directory(_: &File, path: &Path) -> Result<(), DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation: DurableIoOperation::ReadDirectory,
        path: path.to_path_buf(),
    })
}

#[cfg(unix)]
fn open_child_handle(directory: &File, name: &str, mode: ChildOpenMode) -> Result<File, io::Error> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};

    let name = CString::new(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "durable authority name contains an interior NUL",
        )
    })?;
    let access = match mode {
        ChildOpenMode::ReadOnly => libc::O_RDONLY,
        ChildOpenMode::ReadWrite | ChildOpenMode::CreateNew => libc::O_RDWR,
    };
    let creation = match mode {
        ChildOpenMode::CreateNew => libc::O_CREAT | libc::O_EXCL,
        ChildOpenMode::ReadOnly | ChildOpenMode::ReadWrite => 0,
    };
    // O_NONBLOCK prevents an untrusted FIFO/device directory entry from
    // hanging the process before the post-open regular-file fstat can reject
    // it. It has no effect on ordinary files.
    let flags = access | creation | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK;

    // SAFETY: `directory` is a live descriptor pinned to the already-checked
    // durable directory; `name` is a NUL-terminated single path component;
    // flags request an ordinary file descriptor and mode is supplied whenever
    // O_CREAT is present.  A successful descriptor is transferred exactly
    // once into `File` below.
    let descriptor =
        unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, 0o600 as libc::mode_t) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `openat` returned a new owned descriptor and no other owner has
    // been constructed for it.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(unix)]
fn remove_child_handle(directory: &File, name: &str) -> Result<(), io::Error> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;

    let name = CString::new(name).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "durable name contains an interior NUL")
    })?;
    // SAFETY: `directory` is the pinned durable-directory descriptor and
    // `name` is one generated single component.  Flags zero remove only a
    // non-directory entry and never follow a symlink stored at that name.
    let result = unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn replace_child_handle(directory: &File, source: &str, target: &str) -> Result<(), io::Error> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;

    let source = CString::new(source).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "durable source name contains an interior NUL")
    })?;
    let target = CString::new(target).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "durable target name contains an interior NUL")
    })?;
    // SAFETY: both names are generated single components and both directory
    // descriptors are the same pinned directory.  renameat therefore cannot
    // cross a filesystem and atomically replaces only the named entry.
    let result = unsafe {
        libc::renameat(
            directory.as_raw_fd(),
            source.as_ptr(),
            directory.as_raw_fd(),
            target.as_ptr(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn publish_child_handle_without_replace(
    directory: &File,
    source: &str,
    target: &str,
) -> Result<(), io::Error> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;

    let source = CString::new(source).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "durable source name contains an interior NUL")
    })?;
    let target = CString::new(target).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "durable target name contains an interior NUL")
    })?;
    let result = unsafe {
        libc::renameat2(
            directory.as_raw_fd(),
            source.as_ptr(),
            directory.as_raw_fd(),
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    match result {
        0 => Ok(()),
        _ => Err(io::Error::last_os_error()),
    }
}

#[cfg(target_vendor = "apple")]
fn publish_child_handle_without_replace(
    directory: &File,
    source: &str,
    target: &str,
) -> Result<(), io::Error> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;

    let source = CString::new(source).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "durable source name contains an interior NUL")
    })?;
    let target = CString::new(target).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "durable target name contains an interior NUL")
    })?;
    let result = unsafe {
        libc::renameatx_np(
            directory.as_raw_fd(),
            source.as_ptr(),
            directory.as_raw_fd(),
            target.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    match result {
        0 => Ok(()),
        _ => Err(io::Error::last_os_error()),
    }
}

#[cfg(unix)]
fn ensure_child_matches_handle(
    directory: &File,
    name: &str,
    expected: &File,
    directory_path: &Path,
    operation: DurableIoOperation,
) -> Result<(), DurableIoError> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;

    let path = directory_path.join(name);
    let expected_metadata = expected.metadata().map_err(|source| DurableIoError::Io {
        operation: DurableIoOperation::Metadata,
        path: path.clone(),
        source,
    })?;
    let name = CString::new(name).map_err(|_| DurableIoError::Io {
        operation: DurableIoOperation::Metadata,
        path: path.clone(),
        source: io::Error::new(
            io::ErrorKind::InvalidInput,
            "durable name contains an interior NUL",
        ),
    })?;
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the descriptor and generated component are live and `stat` is
    // writable. NOFOLLOW authenticates the named entry rather than a target.
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(DurableIoError::Io {
            operation: DurableIoOperation::Metadata,
            path,
            source: io::Error::last_os_error(),
        });
    }
    // SAFETY: successful fstatat initialized the structure.
    let stat = unsafe { stat.assume_init() };
    if stat.st_dev != expected_metadata.dev() as libc::dev_t ||
        stat.st_ino != expected_metadata.ino() as libc::ino_t
    {
        return Err(DurableIoError::UnsafeFileType { operation, path });
    }
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_permissions(
    file: &File,
    operation: DurableIoOperation,
    path: &Path,
) -> Result<(), DurableIoError> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|source| DurableIoError::Io {
            operation,
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn set_owner_only_permissions(
    _file: &File,
    operation: DurableIoOperation,
    path: &Path,
) -> Result<(), DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation,
        path: path.to_path_buf(),
    })
}

#[cfg(not(unix))]
fn open_child_handle(
    _directory: &File,
    _name: &str,
    _mode: ChildOpenMode,
) -> Result<File, io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "handle-relative no-follow durable opens are unsupported on this platform",
    ))
}

#[cfg(not(unix))]
fn remove_child_handle(_directory: &File, _name: &str) -> Result<(), io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "handle-relative durable unlink is unsupported on this platform",
    ))
}

#[cfg(not(unix))]
fn replace_child_handle(_directory: &File, _source: &str, _target: &str) -> Result<(), io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "handle-relative durable replace is unsupported on this platform",
    ))
}

#[cfg(not(any(all(target_os = "linux", target_env = "gnu"), target_vendor = "apple")))]
fn publish_child_handle_without_replace(
    _directory: &File,
    _source: &str,
    _target: &str,
) -> Result<(), io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "handle-relative no-replace durable rename is unsupported on this platform",
    ))
}

#[cfg(not(unix))]
fn ensure_child_matches_handle(
    _directory: &File,
    _name: &str,
    _expected: &File,
    directory_path: &Path,
    operation: DurableIoOperation,
) -> Result<(), DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation,
        path: directory_path.to_path_buf(),
    })
}

#[cfg(unix)]
pub(super) fn validate_authority_file(
    file: &File,
    operation: DurableIoOperation,
    path: &Path,
) -> Result<(), DurableIoError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = file.metadata().map_err(|source| DurableIoError::Io {
        operation: DurableIoOperation::Metadata,
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.file_type().is_file() {
        return Err(DurableIoError::UnsafeFileType {
            operation,
            path: path.to_path_buf(),
        });
    }
    let links = metadata.nlink();
    if links != 1 {
        return Err(DurableIoError::HardLinkedFile {
            operation,
            path: path.to_path_buf(),
            links,
        });
    }
    let mode = metadata.permissions().mode();
    if mode & 0o077 != 0 {
        return Err(DurableIoError::UnsafePermissions {
            operation,
            path: path.to_path_buf(),
            mode,
        });
    }
    // SAFETY: geteuid has no preconditions and returns the caller's effective
    // Unix user identifier.
    let expected = unsafe { libc::geteuid() };
    if metadata.uid() != expected {
        return Err(DurableIoError::UnsafeOwner {
            operation,
            path: path.to_path_buf(),
            expected,
            actual: metadata.uid(),
        });
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn validate_authority_file(
    _file: &File,
    operation: DurableIoOperation,
    path: &Path,
) -> Result<(), DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation,
        path: path.to_path_buf(),
    })
}

pub(super) fn read_exact_at_with(
    path: &Path,
    offset: u64,
    bytes: &mut [u8],
    mut read: impl FnMut(&mut [u8], u64) -> Result<usize, DurableIoError>,
) -> Result<(), DurableIoError> {
    let expected = u64::try_from(bytes.len()).map_err(|_| DurableIoError::OffsetOverflow {
        operation: DurableIoOperation::PositionalRead,
        path: path.to_path_buf(),
        offset,
        completed: u64::MAX,
    })?;
    let mut completed = 0usize;
    while completed < bytes.len() {
        let completed_u64 = u64::try_from(completed).expect("slice position fits u64");
        let position =
            offset
                .checked_add(completed_u64)
                .ok_or_else(|| DurableIoError::OffsetOverflow {
                    operation: DurableIoOperation::PositionalRead,
                    path: path.to_path_buf(),
                    offset,
                    completed: completed_u64,
                })?;
        let offered = bytes.len() - completed;
        let count = match read(&mut bytes[completed..], position) {
            Ok(count) => count,
            Err(error) if error.is_interrupted() => continue,
            Err(error) => return Err(error),
        };
        if count > offered {
            return Err(DurableIoError::CallbackCountOutOfBounds {
                operation: DurableIoOperation::PositionalRead,
                path: path.to_path_buf(),
                offset: position,
                offered: u64::try_from(offered).expect("offered slice length fits u64"),
                reported: u64::try_from(count).unwrap_or(u64::MAX),
            });
        }
        if count == 0 {
            return Err(DurableIoError::ShortRead {
                operation: DurableIoOperation::PositionalRead,
                path: path.to_path_buf(),
                offset,
                expected,
                actual: completed_u64,
            });
        }
        completed = completed
            .checked_add(count)
            .ok_or_else(|| DurableIoError::OffsetOverflow {
                operation: DurableIoOperation::PositionalRead,
                path: path.to_path_buf(),
                offset,
                completed: completed_u64,
            })?;
    }
    Ok(())
}

fn write_all_at_with<Authority>(
    authority: &mut Authority,
    path: &Path,
    offset: u64,
    bytes: &[u8],
    mut write: impl FnMut(&mut Authority, &[u8], u64) -> Result<usize, DurableIoError>,
) -> Result<(), DurableIoError> {
    let mut completed = 0usize;
    while completed < bytes.len() {
        let completed_u64 = u64::try_from(completed).expect("slice position fits u64");
        let position =
            offset
                .checked_add(completed_u64)
                .ok_or_else(|| DurableIoError::OffsetOverflow {
                    operation: DurableIoOperation::PositionalWrite,
                    path: path.to_path_buf(),
                    offset,
                    completed: completed_u64,
                })?;
        let offered = bytes.len() - completed;
        let count = match write(authority, &bytes[completed..], position) {
            Ok(count) => count,
            Err(error) if error.is_interrupted() => continue,
            Err(error) => return Err(error),
        };
        if count > offered {
            return Err(DurableIoError::CallbackCountOutOfBounds {
                operation: DurableIoOperation::PositionalWrite,
                path: path.to_path_buf(),
                offset: position,
                offered: u64::try_from(offered).expect("offered slice length fits u64"),
                reported: u64::try_from(count).unwrap_or(u64::MAX),
            });
        }
        if count == 0 {
            return Err(DurableIoError::ZeroWrite {
                operation: DurableIoOperation::PositionalWrite,
                path: path.to_path_buf(),
                offset: position,
                remaining: u64::try_from(bytes.len() - completed)
                    .expect("remaining slice length fits u64"),
            });
        }
        completed = completed
            .checked_add(count)
            .ok_or_else(|| DurableIoError::OffsetOverflow {
                operation: DurableIoOperation::PositionalWrite,
                path: path.to_path_buf(),
                offset,
                completed: completed_u64,
            })?;
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn positioned_read(
    file: &File,
    bytes: &mut [u8],
    offset: u64,
    path: &Path,
) -> Result<usize, DurableIoError> {
    use std::os::unix::fs::FileExt;

    file.read_at(bytes, offset)
        .map_err(|source| DurableIoError::Io {
            operation: DurableIoOperation::PositionalRead,
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(windows)]
pub(super) fn positioned_read(
    file: &File,
    bytes: &mut [u8],
    offset: u64,
    path: &Path,
) -> Result<usize, DurableIoError> {
    use std::os::windows::fs::FileExt;

    file.seek_read(bytes, offset)
        .map_err(|source| DurableIoError::Io {
            operation: DurableIoOperation::PositionalRead,
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(not(any(unix, windows)))]
pub(super) fn positioned_read(
    _file: &File,
    _bytes: &mut [u8],
    _offset: u64,
    path: &Path,
) -> Result<usize, DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation: DurableIoOperation::PositionalRead,
        path: path.to_path_buf(),
    })
}

#[cfg(unix)]
fn positioned_write(
    file: &File,
    bytes: &[u8],
    offset: u64,
    path: &Path,
) -> Result<usize, DurableIoError> {
    use std::os::unix::fs::FileExt;

    file.write_at(bytes, offset)
        .map_err(|source| DurableIoError::Io {
            operation: DurableIoOperation::PositionalWrite,
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(windows)]
fn positioned_write(
    file: &File,
    bytes: &[u8],
    offset: u64,
    path: &Path,
) -> Result<usize, DurableIoError> {
    use std::os::windows::fs::FileExt;

    file.seek_write(bytes, offset)
        .map_err(|source| DurableIoError::Io {
            operation: DurableIoOperation::PositionalWrite,
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(not(any(unix, windows)))]
fn positioned_write(
    _file: &File,
    _bytes: &[u8],
    _offset: u64,
    path: &Path,
) -> Result<usize, DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation: DurableIoOperation::PositionalWrite,
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use shared::rust::test_scratch;

    use super::*;

    #[test]
    fn durable_names_are_frozen_and_numeric_order_is_lexical() {
        assert_eq!(CATALOG_FILE_NAME_V1, "catalog.v1");
        assert_eq!(WRITER_LOCK_FILE_NAME_V1, "writer.lock.v1");
        assert_eq!(RECLAMATION_CANDIDATE_FILE_NAME_V1, "reclaim.v1");
        assert_eq!(RECLAMATION_CANDIDATE_TEMPORARY_FILE_NAME_V1, "reclaim.v1.tmp");
        assert_eq!(pack_file_name(PackId::GENESIS), "pack-00000000000000000001.rsp");
        assert_eq!(pack_file_name(PackId::new(u64::MAX).unwrap()), "pack-18446744073709551615.rsp");
        assert_eq!(index_file_name(IndexGenerationV1::FIRST), "index-00000000000000000001.rsi");
    }

    #[test]
    fn writer_lease_is_exclusive_for_its_entire_file_lifetime() {
        let scratch = test_scratch::acquire("rspace-durable-io-lock-");
        let first = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();
        let error = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap_err();
        assert!(matches!(
            error,
            DurableIoError::WriterLockContended {
                operation: DurableIoOperation::AcquireWriterLock,
                ref path,
            } if path == &scratch.path().join(WRITER_LOCK_FILE_NAME_V1)
        ));
        drop(first);
        DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();
    }

    /// A process spawn may briefly duplicate every open file description
    /// before close-on-exec takes effect.  Releasing the affine Rust lease
    /// must therefore issue an explicit unlock; merely closing its descriptor
    /// would leave the duplicate holding the writer lock until it also closes.
    #[test]
    fn dropping_writer_lease_unlocks_a_duplicated_open_description() {
        let scratch = test_scratch::acquire("rspace-durable-io-duplicated-lock-");
        let lease = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();
        let duplicated_description = lease._lock.file.try_clone().unwrap();

        drop(lease);
        let reacquired = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer();
        drop(duplicated_description);

        reacquired.expect("dropping the affine lease must explicitly release its OS lock");
    }

    #[test]
    fn writable_capability_signature_is_lease_borrowed_and_exclusive() {
        let _create: for<'lease> fn(
            &'lease mut DurableWriterLeaseV1,
            PackId,
        )
            -> Result<DurableWritableFileV1<'lease>, DurableIoError> =
            DurableWriterLeaseV1::create_pack;
    }

    #[test]
    fn catalog_io_is_fixed_length_and_slot_positional() {
        let scratch = test_scratch::acquire("rspace-durable-io-catalog-");
        let mut lease = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();
        let second = [0x5a; CATALOG_SLOT_LEN];
        {
            let mut catalog = lease.create_catalog().unwrap();
            assert_eq!(catalog.len().unwrap(), CATALOG_IMAGE_LEN as u64);
            catalog
                .write_catalog_slot(CatalogSlotIndexV1::Second, &second)
                .unwrap();
            catalog.sync_data().unwrap();
        }
        lease.sync_directory().unwrap();

        let catalog = lease.directory().open_catalog().unwrap();
        let image = catalog.read_catalog_image().unwrap();
        assert_eq!(&image[..CATALOG_SLOT_LEN], &[0; CATALOG_SLOT_LEN]);
        assert_eq!(&image[CATALOG_SLOT_LEN..], &second);
    }

    #[test]
    fn pack_and_index_creation_never_truncates_existing_authorities() {
        let scratch = test_scratch::acquire("rspace-durable-io-create-");
        let mut lease = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();

        {
            let mut pack = lease.create_pack(PackId::GENESIS).unwrap();
            pack.write_all_at(0, &[0x31; PACK_HEADER_LEN_V1 as usize])
                .unwrap();
            pack.sync_data().unwrap();
        }
        assert!(lease.create_pack(PackId::GENESIS).is_err());
        {
            let pack = lease.open_pack(PackId::GENESIS).unwrap();
            assert_eq!(pack.len().unwrap(), PACK_HEADER_LEN_V1);
        }

        {
            let mut index = lease.create_index(IndexGenerationV1::FIRST).unwrap();
            index
                .write_all_at(0, &[0x41; INDEX_HEADER_LEN_V1 as usize])
                .unwrap();
            index.sync_data().unwrap();
        }
        assert!(lease.create_index(IndexGenerationV1::FIRST).is_err());
        let index = lease.open_index(IndexGenerationV1::FIRST).unwrap();
        assert_eq!(index.len().unwrap(), INDEX_HEADER_LEN_V1);
    }

    #[test]
    fn canonical_append_sink_streams_exact_extent_without_record_materialization() {
        let scratch = test_scratch::acquire("rspace-durable-io-append-");
        let mut lease = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();
        let prefix = [0x31; PACK_HEADER_LEN_V1 as usize];
        let large = vec![0x7d; DURABLE_APPEND_BUFFER_LEN + 17];
        let suffix = [0xa4; 19];
        let appointed_end = PACK_HEADER_LEN_V1 + large.len() as u64 + suffix.len() as u64;

        {
            let mut pack = lease.create_pack(PackId::GENESIS).unwrap();
            pack.write_all_at(0, &prefix).unwrap();
            let mut sink =
                DurableCanonicalAppendSinkV1::new(&mut pack, PACK_HEADER_LEN_V1, appointed_end)
                    .unwrap();
            sink.write_all(&large).unwrap();
            sink.write_all(&suffix[..7]).unwrap();
            sink.write_all(&suffix[7..]).unwrap();
            assert_eq!(sink.cursor(), appointed_end);
            assert_eq!(sink.finish().unwrap(), appointed_end);
        }

        let pack = lease.directory().open_pack(PackId::GENESIS).unwrap();
        let mut actual = vec![0; appointed_end as usize];
        pack.read_exact_at(0, &mut actual).unwrap();
        assert_eq!(&actual[..prefix.len()], &prefix);
        assert_eq!(&actual[prefix.len()..prefix.len() + large.len()], &large);
        assert_eq!(&actual[prefix.len() + large.len()..], &suffix);
    }

    #[test]
    fn canonical_append_sink_rejects_overrun_before_extending_the_file() {
        let scratch = test_scratch::acquire("rspace-durable-io-append-overrun-");
        let mut lease = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();
        let mut pack = lease.create_pack(PackId::GENESIS).unwrap();
        pack.write_all_at(0, &[0x21; PACK_HEADER_LEN_V1 as usize])
            .unwrap();
        let appointed_end = PACK_HEADER_LEN_V1 + 3;
        let mut sink =
            DurableCanonicalAppendSinkV1::new(&mut pack, PACK_HEADER_LEN_V1, appointed_end)
                .unwrap();
        let error = sink.write_all(&[1, 2, 3, 4]).unwrap_err();
        assert!(matches!(
            error,
            DurableIoError::AppendOverrun {
                cursor: PACK_HEADER_LEN_V1,
                offered: 4,
                appointed_end: end,
                ..
            } if end == appointed_end
        ));
        drop(sink);
        assert_eq!(pack.len().unwrap(), PACK_HEADER_LEN_V1);
    }

    #[test]
    fn canonical_append_sink_reports_underrun_and_invalid_extent() {
        let scratch = test_scratch::acquire("rspace-durable-io-append-underrun-");
        let mut lease = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();
        let mut pack = lease.create_pack(PackId::GENESIS).unwrap();
        pack.write_all_at(0, &[0x21; PACK_HEADER_LEN_V1 as usize])
            .unwrap();
        let appointed_end = PACK_HEADER_LEN_V1 + 4;
        let mut sink =
            DurableCanonicalAppendSinkV1::new(&mut pack, PACK_HEADER_LEN_V1, appointed_end)
                .unwrap();
        sink.write_all(&[1, 2, 3]).unwrap();
        let error = sink.finish().unwrap_err();
        assert!(matches!(
            error,
            DurableIoError::AppendUnderrun {
                actual_end,
                appointed_end: end,
                ..
            } if actual_end == PACK_HEADER_LEN_V1 + 3 && end == appointed_end
        ));
        assert_eq!(pack.len().unwrap(), PACK_HEADER_LEN_V1 + 3);

        let error = match DurableCanonicalAppendSinkV1::new(&mut pack, 10, 9) {
            Ok(_) => panic!("a reversed append extent must be rejected"),
            Err(error) => error,
        };
        assert!(matches!(error, DurableIoError::AppendOverrun {
            cursor: 10,
            offered: 0,
            appointed_end: 9,
            ..
        }));
    }

    #[test]
    fn metadata_and_exact_io_fail_with_typed_path_bound_errors() {
        let scratch = test_scratch::acquire("rspace-durable-io-errors-");
        let mut lease = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();
        let short_pack = lease.create_pack(PackId::GENESIS).unwrap();
        let pack_path = short_pack.path().to_path_buf();
        drop(short_pack);
        let error = lease.directory().open_pack(PackId::GENESIS).unwrap_err();
        assert!(matches!(
            error,
            DurableIoError::Length {
                operation: DurableIoOperation::Metadata,
                ref path,
                expected: FileLengthExpectation::AtLeast(PACK_HEADER_LEN_V1),
                actual: 0,
            } if path == &pack_path
        ));

        let read_path = scratch.path().join("short-read");
        let mut target = [0u8; 8];
        let mut calls = 0;
        let error = read_exact_at_with(&read_path, 11, &mut target, |buffer, _| {
            calls += 1;
            if calls == 1 {
                buffer[..3].copy_from_slice(&[1, 2, 3]);
                Ok(3)
            } else {
                Ok(0)
            }
        })
        .unwrap_err();
        assert!(matches!(
            error,
            DurableIoError::ShortRead {
                operation: DurableIoOperation::PositionalRead,
                ref path,
                offset: 11,
                expected: 8,
                actual: 3,
            } if path == &read_path
        ));

        let write_path = scratch.path().join("zero-write");
        let mut authority = ();
        let error = write_all_at_with(&mut authority, &write_path, 19, &[1, 2], |_, _, _| Ok(0))
            .unwrap_err();
        assert!(matches!(
            error,
            DurableIoError::ZeroWrite {
                operation: DurableIoOperation::PositionalWrite,
                ref path,
                offset: 19,
                remaining: 2,
            } if path == &write_path
        ));
    }

    #[test]
    fn physical_truncation_is_rejected_by_exact_file_reads() {
        let scratch = test_scratch::acquire("rspace-durable-io-physical-short-read-");
        let mut lease = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();
        let mut pack = lease.create_pack(PackId::GENESIS).unwrap();
        let payload = [0x61, 0x62, 0x63, 0x64];
        pack.write_all_at(PACK_HEADER_LEN_V1, &payload).unwrap();
        pack.sync_data().unwrap();
        pack.set_len(PACK_HEADER_LEN_V1 + 2).unwrap();

        let mut destination = [0u8; 4];
        let pack_path = pack.path().to_path_buf();
        let error = pack
            .read_exact_at(PACK_HEADER_LEN_V1, &mut destination)
            .unwrap_err();
        assert!(matches!(
            error,
            DurableIoError::ShortRead {
                operation: DurableIoOperation::PositionalRead,
                ref path,
                offset,
                expected: 4,
                actual: 2,
            } if path == &pack_path && offset == PACK_HEADER_LEN_V1
        ));
    }

    #[test]
    fn physical_write_and_sync_failpoints_intercept_real_file_boundaries() {
        let scratch = test_scratch::acquire("rspace-durable-io-physical-failpoints-");
        let mut lease = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();
        let mut pack = lease.create_pack(PackId::GENESIS).unwrap();
        let path = pack.path().to_path_buf();

        arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalWrite);
        let error = pack
            .write_all_at(PACK_HEADER_LEN_V1, &[1, 2, 3])
            .unwrap_err();
        assert!(matches!(
            error,
            DurableIoError::Io {
                operation: DurableIoOperation::PositionalWrite,
                path: ref actual,
                ..
            } if actual == &path
        ));
        assert_eq!(pack.len().unwrap(), 0);

        pack.write_all_at(PACK_HEADER_LEN_V1, &[1, 2, 3]).unwrap();
        arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalSync);
        let error = pack.sync_all().unwrap_err();
        assert!(matches!(
            error,
            DurableIoError::Io {
                operation: DurableIoOperation::SyncFile,
                path: ref actual,
                ..
            } if actual == &path
        ));
    }

    #[test]
    fn exact_io_retries_interruptions_and_rejects_impossible_counts() {
        let path = PathBuf::from("/diagnostic/exact-io");
        let mut read_target = [0u8; 4];
        let mut read_calls = 0;
        read_exact_at_with(&path, 7, &mut read_target, |buffer, position| {
            read_calls += 1;
            if read_calls == 1 {
                return Err(DurableIoError::Io {
                    operation: DurableIoOperation::PositionalRead,
                    path: path.clone(),
                    source: io::Error::new(io::ErrorKind::Interrupted, "signal"),
                });
            }
            assert_eq!(position, 7);
            buffer.copy_from_slice(&[1, 2, 3, 4]);
            Ok(buffer.len())
        })
        .unwrap();
        assert_eq!(read_calls, 2);
        assert_eq!(read_target, [1, 2, 3, 4]);

        let mut write_calls = 0;
        let mut authority = ();
        write_all_at_with(&mut authority, &path, 13, &[5, 6, 7], |_, buffer, position| {
            write_calls += 1;
            if write_calls == 1 {
                return Err(DurableIoError::Io {
                    operation: DurableIoOperation::PositionalWrite,
                    path: path.clone(),
                    source: io::Error::new(io::ErrorKind::Interrupted, "signal"),
                });
            }
            assert_eq!(position, 13);
            Ok(buffer.len())
        })
        .unwrap();
        assert_eq!(write_calls, 2);

        let mut partial_write_calls = 0;
        write_all_at_with(&mut authority, &path, 31, &[10, 11, 12, 13], |_, buffer, position| {
            partial_write_calls += 1;
            if partial_write_calls == 1 {
                assert_eq!(position, 31);
                assert_eq!(buffer, &[10, 11, 12, 13]);
                Ok(1)
            } else {
                assert_eq!(position, 32);
                assert_eq!(buffer, &[11, 12, 13]);
                Ok(buffer.len())
            }
        })
        .unwrap();
        assert_eq!(partial_write_calls, 2);

        let mut mixed_fault_calls = 0;
        write_all_at_with(&mut authority, &path, 41, &[14, 15, 16, 17], |_, buffer, position| {
            mixed_fault_calls += 1;
            match mixed_fault_calls {
                1 | 3 => Err(DurableIoError::Io {
                    operation: DurableIoOperation::PositionalWrite,
                    path: path.clone(),
                    source: io::Error::new(io::ErrorKind::Interrupted, "signal"),
                }),
                2 => {
                    assert_eq!(position, 41);
                    assert_eq!(buffer, &[14, 15, 16, 17]);
                    Ok(2)
                }
                4 => {
                    assert_eq!(position, 43);
                    assert_eq!(buffer, &[16, 17]);
                    Ok(buffer.len())
                }
                _ => panic!("write loop made an unexpected extra call"),
            }
        })
        .unwrap();
        assert_eq!(mixed_fault_calls, 4);

        let mut target = [0u8; 2];
        let error = read_exact_at_with(&path, 17, &mut target, |buffer, _| Ok(buffer.len() + 1))
            .unwrap_err();
        assert!(matches!(error, DurableIoError::CallbackCountOutOfBounds {
            operation: DurableIoOperation::PositionalRead,
            offset: 17,
            offered: 2,
            reported: 3,
            ..
        }));
        let error = write_all_at_with(&mut authority, &path, 23, &[8, 9], |_, buffer, _| {
            Ok(buffer.len() + 1)
        })
        .unwrap_err();
        assert!(matches!(error, DurableIoError::CallbackCountOutOfBounds {
            operation: DurableIoOperation::PositionalWrite,
            offset: 23,
            offered: 2,
            reported: 3,
            ..
        }));
    }

    #[cfg(unix)]
    #[test]
    fn child_authorities_reject_symlinks_and_hard_links() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::{PermissionsExt, symlink};

        let scratch = test_scratch::acquire("rspace-durable-io-identity-");
        let outside = scratch.path().join("outside");
        File::create(&outside).unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&outside, scratch.path().join(CATALOG_FILE_NAME_V1)).unwrap();
        let directory = DurableDirectoryV1::open(scratch.path()).unwrap();
        let error = directory.open_catalog().unwrap_err();
        assert!(matches!(error, DurableIoError::Io { .. }));

        std::fs::remove_file(scratch.path().join(CATALOG_FILE_NAME_V1)).unwrap();
        let pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
        let pack = File::create(&pack_path).unwrap();
        pack.set_len(PACK_HEADER_LEN_V1).unwrap();
        std::fs::set_permissions(&pack_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(&pack_path, scratch.path().join("pack-alias")).unwrap();
        let error = directory.open_pack(PackId::GENESIS).unwrap_err();
        assert!(matches!(error, DurableIoError::HardLinkedFile { links: 2, .. }));

        let index_path = scratch
            .path()
            .join(index_file_name(IndexGenerationV1::FIRST));
        let index_path_c = CString::new(index_path.as_os_str().as_bytes()).unwrap();
        // SAFETY: the CString is a live NUL-terminated pathname and the mode is
        // a valid permission mask. The path does not exist yet.
        assert_eq!(unsafe { libc::mkfifo(index_path_c.as_ptr(), 0o600) }, 0);
        let error = directory.open_index(IndexGenerationV1::FIRST).unwrap_err();
        assert!(matches!(error, DurableIoError::UnsafeFileType { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn durable_directory_rejects_a_final_symlink() {
        use std::os::unix::fs::symlink;

        let scratch = test_scratch::acquire("rspace-durable-io-directory-link-");
        let store_path = scratch.path().join("store");
        let link_path = scratch.path().join("store-link");
        std::fs::create_dir(&store_path).unwrap();
        symlink(&store_path, &link_path).unwrap();
        let error = DurableDirectoryV1::open(&link_path).unwrap_err();
        assert!(matches!(error, DurableIoError::Io {
            operation: DurableIoOperation::OpenDirectory,
            ..
        }));
    }

    #[cfg(unix)]
    #[test]
    fn pinned_directory_identity_survives_path_replacement() {
        let scratch = test_scratch::acquire("rspace-durable-io-pinned-");
        let store_path = scratch.path().join("store");
        let pinned_path = scratch.path().join("pinned");
        std::fs::create_dir(&store_path).unwrap();
        let directory = DurableDirectoryV1::open(&store_path).unwrap();

        std::fs::rename(&store_path, &pinned_path).unwrap();
        std::fs::create_dir(&store_path).unwrap();
        let _lease = directory.try_acquire_writer().unwrap();

        assert!(pinned_path.join(WRITER_LOCK_FILE_NAME_V1).is_file());
        assert!(!store_path.join(WRITER_LOCK_FILE_NAME_V1).exists());
    }

    #[cfg(unix)]
    #[test]
    fn authority_permissions_fail_closed() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = test_scratch::acquire("rspace-durable-io-permissions-");
        let pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
        let pack = File::create(&pack_path).unwrap();
        pack.set_len(PACK_HEADER_LEN_V1).unwrap();
        std::fs::set_permissions(&pack_path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let directory = DurableDirectoryV1::open(scratch.path()).unwrap();
        let error = directory.open_pack(PackId::GENESIS).unwrap_err();
        assert!(matches!(
            error,
            DurableIoError::UnsafePermissions { mode, .. } if mode & 0o777 == 0o640
        ));
    }
}
