use std::fmt::{Display, Formatter};
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use super::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, INDEX_HEADER_LEN_V1, IndexGenerationV1, PACK_HEADER_LEN_V1,
};
use crate::rspace::persistence::PackId;

pub(crate) const CATALOG_FILE_NAME_V1: &str = "catalog.v1";
pub(crate) const WRITER_LOCK_FILE_NAME_V1: &str = "writer.lock.v1";
const PACK_FILE_PREFIX_V1: &str = "pack-";
const PACK_FILE_SUFFIX_V1: &str = ".rsp";
const INDEX_FILE_PREFIX_V1: &str = "index-";
const INDEX_FILE_SUFFIX_V1: &str = ".rsi";

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DurableIoFailpointV1 {
    CandidateWrite,
    CandidateSetLength,
    CandidateSync,
    BeforeReplace,
    AfterReplaceBeforeDirectorySync,
    DirectorySync,
}

#[cfg(test)]
thread_local! {
    static DURABLE_IO_FAILPOINT: std::cell::Cell<Option<DurableIoFailpointV1>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn arm_durable_io_failpoint(failpoint: DurableIoFailpointV1) {
    DURABLE_IO_FAILPOINT.with(|armed| {
        assert!(armed.replace(Some(failpoint)).is_none(), "durable I/O failpoint already armed");
    });
}

#[cfg(test)]
fn trip_durable_io_failpoint(
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
    UnsupportedPlatform {
        operation: DurableIoOperation,
        path: PathBuf,
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
        write_all_at_with(self.file.path(), offset, bytes, |buffer, position| {
            positioned_write(&self.file.file, buffer, position, self.file.path())
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
        self.require_length(FileLengthExpectation::Exact(CATALOG_IMAGE_LEN as u64))?;
        self.write_all_at(slot.offset(), bytes)
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
}

impl DurableDirectoryV1 {
    pub(crate) fn open(path: impl Into<PathBuf>) -> Result<Self, DurableIoError> {
        let path = absolute_diagnostic_path(path.into())?;
        let directory = open_directory_handle(&path)?;
        Ok(Self { path, directory })
    }

    pub(crate) fn path(&self) -> &Path { &self.path }

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
        match remove_child_handle(&self.directory.directory, &name) {
            Ok(()) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(DurableIoError::Io {
                    operation: DurableIoOperation::RemoveFile,
                    path: self.directory.path.join(&name),
                    source,
                });
            }
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
fn open_directory_handle(path: &Path) -> Result<File, DurableIoError> {
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
fn open_directory_handle(path: &Path) -> Result<File, DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation: DurableIoOperation::OpenDirectory,
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

#[cfg(unix)]
fn ensure_child_matches_handle(
    directory: &File,
    name: &str,
    expected: &File,
    directory_path: &Path,
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
        return Err(DurableIoError::UnsafeFileType {
            operation: DurableIoOperation::ReplaceFile,
            path,
        });
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

#[cfg(not(unix))]
fn ensure_child_matches_handle(
    _directory: &File,
    _name: &str,
    _expected: &File,
    directory_path: &Path,
) -> Result<(), DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation: DurableIoOperation::ReplaceFile,
        path: directory_path.to_path_buf(),
    })
}

#[cfg(unix)]
fn validate_authority_file(
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
fn validate_authority_file(
    _file: &File,
    operation: DurableIoOperation,
    path: &Path,
) -> Result<(), DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation,
        path: path.to_path_buf(),
    })
}

fn read_exact_at_with(
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

fn write_all_at_with(
    path: &Path,
    offset: u64,
    bytes: &[u8],
    mut write: impl FnMut(&[u8], u64) -> Result<usize, DurableIoError>,
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
        let count = match write(&bytes[completed..], position) {
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
fn positioned_read(
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
fn positioned_read(
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
fn positioned_read(
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
        let error = write_all_at_with(&write_path, 19, &[1, 2], |_, _| Ok(0)).unwrap_err();
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
        write_all_at_with(&path, 13, &[5, 6, 7], |buffer, position| {
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
        let error =
            write_all_at_with(&path, 23, &[8, 9], |buffer, _| Ok(buffer.len() + 1)).unwrap_err();
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
