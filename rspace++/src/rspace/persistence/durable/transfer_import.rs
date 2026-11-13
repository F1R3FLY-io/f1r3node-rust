use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::{Display, Formatter};
use std::fs::{File, TryLockError};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};

use super::io::{
    DurableIoError, DurableIoOperation, open_directory_handle, validate_authority_file,
};
#[cfg(test)]
use super::io::{DurableIoFailpointV1, trip_durable_io_failpoint};
use super::transfer_export::{
    TRANSFER_PAGE_VERSION_V1, TransferBindingV1, TransferPageFormatErrorV1, TransferPageLimitsV1,
    TransferPageViewV1, TransferTokenV1,
};
use crate::rspace::persistence::canonical::{OBJECT_DIGEST_LEN, OBJECT_ENVELOPE_HEADER_LEN};
use crate::rspace::persistence::checksum::{
    CHECKSUM_FIELD_LEN, crc32c, decode_crc32c, encode_crc32c,
};
use crate::rspace::persistence::closure::StructuralValidationLimits;
use crate::rspace::persistence::manifest::SEMANTIC_PROFILE_ID_LEN;
use crate::rspace::persistence::object::CanonicalObjectView;
use crate::rspace::persistence::semantic::{
    SemanticValidationLimits, SemanticValidationReport, verify_rspace_semantics,
};
use crate::rspace::persistence::store::ImmutableObjectStore;
use crate::rspace::persistence::{
    CanonicalFormatError, CanonicalObjectKind, EndpointValue, ObjectDigest, ObjectRefV1,
    SemanticProfile, SemanticProfileId,
};

const STAGING_DIRECTORY_NAME_V1: &str = "transfer-import.v1";
const STAGED_OBJECTS_FILE_NAME_V1: &str = "objects.v1";
const FRONTIER_FILE_NAME_V1: &str = "frontier.v1";
const FRONTIER_TEMPORARY_FILE_NAME_V1: &str = "frontier.v1.tmp";
const FRONTIER_MAGIC_V1: &[u8; FRONTIER_MAGIC_LEN] = b"RSPFRNT1";
const FRONTIER_MAGIC_LEN: usize = 8;
const FRONTIER_VERSION_OFFSET: usize = FRONTIER_MAGIC_LEN;
const FRONTIER_ROOT_OFFSET: usize = FRONTIER_VERSION_OFFSET + 8;
const FRONTIER_PROFILE_OFFSET: usize = FRONTIER_ROOT_OFFSET + OBJECT_DIGEST_LEN;
const FRONTIER_POSITION_OFFSET: usize = FRONTIER_PROFILE_OFFSET + SEMANTIC_PROFILE_ID_LEN;
const FRONTIER_BODY_LEN: usize = FRONTIER_POSITION_OFFSET + 8;
const FRONTIER_RECORD_LEN_V1: usize = FRONTIER_BODY_LEN + CHECKSUM_FIELD_LEN;
const STAGED_LENGTH_FIELD_LEN: usize = 8;
const MIN_STAGED_RECORD_LEN: u64 = STAGED_LENGTH_FIELD_LEN as u64 + OBJECT_ENVELOPE_HEADER_LEN;
const STAGED_READ_BUFFER_LEN: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransferStagingOutcomeV1 {
    Staged(TransferTokenV1),
    Complete(TransferTokenV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransferImportBoundV1 {
    DistinctObjects,
    CanonicalBytes,
    References,
    Outstanding,
}

impl TransferImportBoundV1 {
    const fn name(self) -> &'static str {
        match self {
            Self::DistinctObjects => "distinct objects",
            Self::CanonicalBytes => "declared canonical bytes",
            Self::References => "references",
            Self::Outstanding => "announced objects not yet staged",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TransferObjectRefusalV1 {
    Duplicate {
        digest: ObjectDigest,
    },
    Unreferenced {
        digest: ObjectDigest,
    },
    LengthMismatch {
        digest: ObjectDigest,
        known: u64,
        found: u64,
    },
    NotCanonical {
        digest: ObjectDigest,
        error: CanonicalFormatError,
    },
    ForeignManifest {
        digest: ObjectDigest,
        profile: SemanticProfileId,
    },
    UnexpectedKind {
        digest: ObjectDigest,
        kind: CanonicalObjectKind,
    },
    Bound {
        bound: TransferImportBoundV1,
        limit: u64,
    },
    Allocation {
        resource: &'static str,
        requested: u64,
    },
}

impl Display for TransferObjectRefusalV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Duplicate { digest } => write!(formatter, "object {digest} is already staged"),
            Self::Unreferenced { digest } => write!(
                formatter,
                "object {digest} is neither the binding's manifest nor referenced by an already \
                 staged object"
            ),
            Self::LengthMismatch {
                digest,
                known,
                found,
            } => write!(
                formatter,
                "object {digest} is known with {known} bytes but {found} bytes were offered"
            ),
            Self::NotCanonical { digest, error } => {
                write!(formatter, "object {digest} is not canonical: {error}")
            }
            Self::ForeignManifest { digest, profile } => write!(
                formatter,
                "manifest {digest} belongs to semantic profile {}, not the binding's",
                hex::encode(profile.as_bytes())
            ),
            Self::UnexpectedKind { digest, kind } => write!(
                formatter,
                "object {digest} is a {} object, which a semantic state closure cannot hold there",
                kind.name()
            ),
            Self::Bound { bound, limit } => {
                write!(
                    formatter,
                    "the staged closure exceeds its bound of {limit} {}",
                    bound.name()
                )
            }
            Self::Allocation {
                resource,
                requested,
            } => write!(formatter, "cannot allocate {requested} more entries for the {resource}"),
        }
    }
}

impl std::error::Error for TransferObjectRefusalV1 {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransferFrontierFormatErrorV1 {
    Length { len: u64 },
    Magic,
    ChecksumOutOfRange(u64),
    Checksum { expected: u32, actual: u32 },
}

impl Display for TransferFrontierFormatErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Length { len } => write!(
                formatter,
                "a frontier record of {len} bytes is not exactly {FRONTIER_RECORD_LEN_V1} bytes"
            ),
            Self::Magic => write!(formatter, "the frontier record does not start with its magic"),
            Self::ChecksumOutOfRange(value) => {
                write!(formatter, "frontier checksum field {value:#x} exceeds 32 bits")
            }
            Self::Checksum { expected, actual } => write!(
                formatter,
                "frontier checksum mismatch: recorded {expected:#010x}, computed {actual:#010x}"
            ),
        }
    }
}

impl std::error::Error for TransferFrontierFormatErrorV1 {}

#[derive(Debug)]
pub(crate) enum TransferImportErrorV1 {
    ForeignProfile {
        store: SemanticProfileId,
        requested: SemanticProfileId,
    },
    StagingExists {
        path: PathBuf,
    },
    NoStaging {
        path: PathBuf,
    },
    IncompleteStaging {
        path: PathBuf,
    },
    StagingInUse {
        path: PathBuf,
    },
    StagingDirectory {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Io(DurableIoError),
    Frontier {
        path: PathBuf,
        error: TransferFrontierFormatErrorV1,
    },
    UnsupportedTransferVersion {
        version: u64,
    },
    BindingMismatch {
        staged_root: ObjectDigest,
        staged_profile: SemanticProfileId,
        requested: TransferBindingV1,
    },
    StagedRecordLength {
        position: u64,
        len: u64,
        max_page_bytes: u64,
    },
    StagedObjectsMissing {
        frontier: u64,
        staged: u64,
    },
    StagedObject {
        position: u64,
        refusal: TransferObjectRefusalV1,
    },
    Allocation {
        resource: &'static str,
        requested: u64,
    },
    SessionFailed,
    Page(TransferPageFormatErrorV1),
    PageBinding {
        session: TransferBindingV1,
        page: TransferBindingV1,
    },
    PositionMismatch {
        expected: u64,
        found: u64,
    },
    IncompleteAtEnd {
        position: u64,
        outstanding: u64,
    },
    Object {
        position: u64,
        refusal: TransferObjectRefusalV1,
    },
    IncompleteClosure {
        position: u64,
        outstanding: u64,
    },
    StagedObjectBound {
        digest: ObjectDigest,
        staged: u64,
        limit: u64,
    },
    ColdAdmission(CanonicalFormatError),
}

impl Display for TransferImportErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ForeignProfile { store, requested } => write!(
                formatter,
                "a transfer of semantic profile {} cannot be staged into a store of semantic \
                 profile {}",
                hex::encode(requested.as_bytes()),
                hex::encode(store.as_bytes())
            ),
            Self::StagingExists { path } => write!(
                formatter,
                "import staging {} already exists; resume or abort it first",
                path.display()
            ),
            Self::NoStaging { path } => {
                write!(formatter, "no import staging exists at {}", path.display())
            }
            Self::IncompleteStaging { path } => write!(
                formatter,
                "import staging {} holds no complete session; abort it",
                path.display()
            ),
            Self::StagingInUse { path } => {
                write!(formatter, "import staging {} is held by a live session", path.display())
            }
            Self::StagingDirectory {
                action,
                path,
                source,
            } => write!(
                formatter,
                "cannot {action} import staging directory {}: {source}",
                path.display()
            ),
            Self::Io(error) => write!(formatter, "import staging I/O failed: {error}"),
            Self::Frontier { path, error } => {
                write!(formatter, "frontier record {} is malformed: {error}", path.display())
            }
            Self::UnsupportedTransferVersion { version } => write!(
                formatter,
                "the staged frontier belongs to transfer version {version}, not version \
                 {TRANSFER_PAGE_VERSION_V1}"
            ),
            Self::BindingMismatch {
                staged_root,
                staged_profile,
                requested,
            } => write!(
                formatter,
                "the staged frontier is bound to durable state root {staged_root} of semantic \
                 profile {}, not to {requested}",
                hex::encode(staged_profile.as_bytes())
            ),
            Self::StagedRecordLength {
                position,
                len,
                max_page_bytes,
            } => write!(
                formatter,
                "staged record {position} declares {len} bytes, outside 32 to {max_page_bytes}"
            ),
            Self::StagedObjectsMissing { frontier, staged } => write!(
                formatter,
                "the frontier acknowledges {frontier} objects but the staging log holds only \
                 {staged}"
            ),
            Self::StagedObject { position, refusal } => {
                write!(formatter, "staged object {position} is refused: {refusal}")
            }
            Self::Allocation {
                resource,
                requested,
            } => write!(formatter, "cannot allocate {requested} more entries for the {resource}"),
            Self::SessionFailed => write!(
                formatter,
                "the import session failed an I/O step and stages nothing more; resume it"
            ),
            Self::Page(error) => write!(formatter, "the transfer page is malformed: {error}"),
            Self::PageBinding { session, page } => write!(
                formatter,
                "a transfer page bound to {page} was offered to the import session of {session}"
            ),
            Self::PositionMismatch { expected, found } => write!(
                formatter,
                "a transfer page at position {found} was offered at frontier {expected}"
            ),
            Self::IncompleteAtEnd {
                position,
                outstanding,
            } => write!(
                formatter,
                "a terminal page at position {position} ends a stream with {outstanding} \
                 announced objects not staged"
            ),
            Self::Object { position, refusal } => {
                write!(formatter, "transfer object {position} is refused: {refusal}")
            }
            Self::IncompleteClosure {
                position,
                outstanding,
            } => write!(
                formatter,
                "a staging at position {position} with {outstanding} announced objects not staged \
                 holds no complete closure to seal"
            ),
            Self::StagedObjectBound {
                digest,
                staged,
                limit,
            } => write!(
                formatter,
                "staged object {digest} holds {staged} bytes, above the read bound of {limit} \
                 bytes"
            ),
            Self::ColdAdmission(error) => {
                write!(formatter, "cold admission refuses the staged closure: {error}")
            }
        }
    }
}

impl std::error::Error for TransferImportErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StagingDirectory { source, .. } => Some(source),
            Self::Io(error) => Some(error),
            Self::Frontier { error, .. } => Some(error),
            Self::StagedObject { refusal, .. } | Self::Object { refusal, .. } => Some(refusal),
            Self::Page(error) => Some(error),
            Self::ForeignProfile { .. } |
            Self::StagingExists { .. } |
            Self::NoStaging { .. } |
            Self::IncompleteStaging { .. } |
            Self::StagingInUse { .. } |
            Self::UnsupportedTransferVersion { .. } |
            Self::BindingMismatch { .. } |
            Self::StagedRecordLength { .. } |
            Self::StagedObjectsMissing { .. } |
            Self::Allocation { .. } |
            Self::SessionFailed |
            Self::PageBinding { .. } |
            Self::PositionMismatch { .. } |
            Self::IncompleteAtEnd { .. } => None,
            Self::ColdAdmission(error) => Some(error),
            Self::IncompleteClosure { .. } | Self::StagedObjectBound { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct KnownObjectV1 {
    encoded_len: u64,
    staged: bool,
}

#[derive(Clone, Copy, Debug)]
enum ClosureUndoV1 {
    Inserted(ObjectDigest),
    Staged(ObjectDigest),
}

#[derive(Clone, Copy, Debug)]
struct ClosureCountersV1 {
    outstanding: u64,
    declared_bytes: u64,
    references: u64,
}

#[derive(Debug)]
struct StagedClosureV1 {
    binding: TransferBindingV1,
    bounds: StructuralValidationLimits,
    known: HashMap<ObjectDigest, KnownObjectV1>,
    counters: ClosureCountersV1,
    undo: Vec<ClosureUndoV1>,
}

impl StagedClosureV1 {
    fn new(binding: TransferBindingV1, bounds: StructuralValidationLimits) -> Self {
        Self {
            binding,
            bounds,
            known: HashMap::new(),
            counters: ClosureCountersV1 {
                outstanding: 0,
                declared_bytes: 0,
                references: 0,
            },
            undo: Vec::new(),
        }
    }

    fn reserve(&mut self, additional: usize) -> Result<(), TransferObjectRefusalV1> {
        let requested = u64::try_from(additional).unwrap_or(u64::MAX);
        self.known
            .try_reserve(additional)
            .map_err(|_| TransferObjectRefusalV1::Allocation {
                resource: "staged closure index",
                requested,
            })?;
        self.undo
            .try_reserve(additional)
            .map_err(|_| TransferObjectRefusalV1::Allocation {
                resource: "staged closure undo log",
                requested,
            })
    }

    fn admit(&mut self, position: u64, object: &[u8]) -> Result<(), TransferObjectRefusalV1> {
        let digest = ObjectDigest::of_canonical_bytes(object);
        let encoded_len = u64::try_from(object.len()).unwrap_or(u64::MAX);
        self.reserve(1)?;
        match self.known.entry(digest) {
            Entry::Occupied(entry) => match *entry.get() {
                KnownObjectV1 { staged: true, .. } => {
                    return Err(TransferObjectRefusalV1::Duplicate { digest });
                }
                KnownObjectV1 {
                    encoded_len: known, ..
                } if known != encoded_len => {
                    return Err(TransferObjectRefusalV1::LengthMismatch {
                        digest,
                        known,
                        found: encoded_len,
                    });
                }
                KnownObjectV1 { .. } => {
                    entry.into_mut().staged = true;
                    self.counters.outstanding = self
                        .counters
                        .outstanding
                        .checked_sub(1)
                        .expect("every announced object is counted as outstanding");
                    self.undo.push(ClosureUndoV1::Staged(digest));
                }
            },
            Entry::Vacant(entry) if position == 0 && digest == self.binding.root_digest() => {
                entry.insert(KnownObjectV1 {
                    encoded_len,
                    staged: true,
                });
                self.undo.push(ClosureUndoV1::Inserted(digest));
                self.declare(encoded_len)?;
            }
            Entry::Vacant(_) => return Err(TransferObjectRefusalV1::Unreferenced { digest }),
        }
        let parsed = CanonicalObjectView::parse(object)
            .map_err(|error| TransferObjectRefusalV1::NotCanonical { digest, error })?;
        match (position, parsed) {
            (0, CanonicalObjectView::SemanticManifest(manifest))
                if manifest.profile_id() == self.binding.profile() =>
            {
                let roots = manifest.roots().iter();
                self.reserve(roots.len())?;
                for (_, root) in roots {
                    self.reference(root)?;
                }
            }
            (0, CanonicalObjectView::SemanticManifest(manifest)) => {
                return Err(TransferObjectRefusalV1::ForeignManifest {
                    digest,
                    profile: manifest.profile_id(),
                });
            }
            (1.., CanonicalObjectView::DagNode(node)) => {
                let children = node.children();
                self.reserve(children.len() + 1)?;
                if let EndpointValue::Content(content) = node.endpoint() {
                    self.reference(content)?;
                }
                for child in children.iter() {
                    self.reference(child.target)?;
                }
            }
            (1.., CanonicalObjectView::Blob(_)) => {}
            (_, other) => {
                return Err(TransferObjectRefusalV1::UnexpectedKind {
                    digest,
                    kind: other.kind(),
                });
            }
        }
        Ok(())
    }

    fn reference(&mut self, reference: ObjectRefV1) -> Result<(), TransferObjectRefusalV1> {
        self.counters.references = match self.counters.references.checked_add(1) {
            Some(total) if total <= self.bounds.max_references => total,
            _ => {
                return Err(TransferObjectRefusalV1::Bound {
                    bound: TransferImportBoundV1::References,
                    limit: self.bounds.max_references,
                });
            }
        };
        let target = reference.digest();
        let referenced_len = reference.encoded_len();
        match self.known.entry(target) {
            Entry::Occupied(entry) => match entry.get().encoded_len {
                known if known != referenced_len => Err(TransferObjectRefusalV1::LengthMismatch {
                    digest: target,
                    known,
                    found: referenced_len,
                }),
                _ => Ok(()),
            },
            Entry::Vacant(entry) => {
                entry.insert(KnownObjectV1 {
                    encoded_len: referenced_len,
                    staged: false,
                });
                self.undo.push(ClosureUndoV1::Inserted(target));
                self.counters.outstanding = match self.counters.outstanding.checked_add(1) {
                    Some(total) if total <= self.bounds.max_worklist => total,
                    _ => {
                        return Err(TransferObjectRefusalV1::Bound {
                            bound: TransferImportBoundV1::Outstanding,
                            limit: self.bounds.max_worklist,
                        });
                    }
                };
                self.declare(referenced_len)
            }
        }
    }

    fn declare(&mut self, encoded_len: u64) -> Result<(), TransferObjectRefusalV1> {
        match u64::try_from(self.known.len()) {
            Ok(distinct) if distinct <= self.bounds.max_distinct_objects => {}
            _ => {
                return Err(TransferObjectRefusalV1::Bound {
                    bound: TransferImportBoundV1::DistinctObjects,
                    limit: self.bounds.max_distinct_objects,
                });
            }
        }
        self.counters.declared_bytes = match self.counters.declared_bytes.checked_add(encoded_len) {
            Some(total) if total <= self.bounds.max_canonical_bytes => total,
            _ => {
                return Err(TransferObjectRefusalV1::Bound {
                    bound: TransferImportBoundV1::CanonicalBytes,
                    limit: self.bounds.max_canonical_bytes,
                });
            }
        };
        Ok(())
    }

    fn rollback(&mut self, saved: ClosureCountersV1) {
        while let Some(entry) = self.undo.pop() {
            match entry {
                ClosureUndoV1::Inserted(digest) => {
                    self.known.remove(&digest);
                }
                ClosureUndoV1::Staged(digest) => {
                    if let Some(known) = self.known.get_mut(&digest) {
                        known.staged = false;
                    }
                }
            }
        }
        self.counters = saved;
    }

    fn commit(&mut self) { self.undo.clear(); }
}

#[derive(Clone, Copy, Debug)]
struct FrontierRecordV1 {
    version: u64,
    root: ObjectDigest,
    profile: SemanticProfileId,
    position: u64,
}

fn encode_frontier(token: TransferTokenV1) -> [u8; FRONTIER_RECORD_LEN_V1] {
    let binding = token.binding();
    let mut record = [0_u8; FRONTIER_RECORD_LEN_V1];
    record[..FRONTIER_VERSION_OFFSET].copy_from_slice(FRONTIER_MAGIC_V1);
    record[FRONTIER_VERSION_OFFSET..FRONTIER_ROOT_OFFSET]
        .copy_from_slice(&TRANSFER_PAGE_VERSION_V1.to_le_bytes());
    record[FRONTIER_ROOT_OFFSET..FRONTIER_PROFILE_OFFSET]
        .copy_from_slice(binding.root_digest().as_bytes());
    record[FRONTIER_PROFILE_OFFSET..FRONTIER_POSITION_OFFSET]
        .copy_from_slice(binding.profile().as_bytes());
    record[FRONTIER_POSITION_OFFSET..FRONTIER_BODY_LEN]
        .copy_from_slice(&token.position().to_le_bytes());
    let checksum = encode_crc32c(crc32c(&record[..FRONTIER_BODY_LEN]));
    record[FRONTIER_BODY_LEN..].copy_from_slice(&checksum);
    record
}

fn decode_frontier(
    record: &[u8; FRONTIER_RECORD_LEN_V1],
) -> Result<FrontierRecordV1, TransferFrontierFormatErrorV1> {
    let (body, trailer) = record.split_at(FRONTIER_BODY_LEN);
    if body[..FRONTIER_VERSION_OFFSET] != FRONTIER_MAGIC_V1[..] {
        return Err(TransferFrontierFormatErrorV1::Magic);
    }
    let expected = decode_crc32c(
        trailer
            .try_into()
            .expect("the frontier trailer is exactly one checksum field"),
    )
    .map_err(TransferFrontierFormatErrorV1::ChecksumOutOfRange)?;
    let actual = crc32c(body);
    if expected != actual {
        return Err(TransferFrontierFormatErrorV1::Checksum { expected, actual });
    }
    Ok(FrontierRecordV1 {
        version: read_u64_at(body, FRONTIER_VERSION_OFFSET),
        root: ObjectDigest::from_array(
            body[FRONTIER_ROOT_OFFSET..FRONTIER_PROFILE_OFFSET]
                .try_into()
                .expect("the frontier record holds one object digest"),
        ),
        profile: SemanticProfileId::from_array(
            body[FRONTIER_PROFILE_OFFSET..FRONTIER_POSITION_OFFSET]
                .try_into()
                .expect("the frontier record holds one semantic profile id"),
        ),
        position: read_u64_at(body, FRONTIER_POSITION_OFFSET),
    })
}

fn read_u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("the frontier record holds an eight-byte field at every fixed offset"),
    )
}

#[derive(Debug)]
struct StagingDirectoryV1 {
    store: File,
    store_path: PathBuf,
    directory: File,
    path: PathBuf,
    objects_path: PathBuf,
    frontier_path: PathBuf,
    temporary_path: PathBuf,
}

impl StagingDirectoryV1 {
    fn with_handles(store: File, store_path: &Path, directory: File, path: PathBuf) -> Self {
        Self {
            store,
            store_path: store_path.to_path_buf(),
            directory,
            objects_path: path.join(STAGED_OBJECTS_FILE_NAME_V1),
            frontier_path: path.join(FRONTIER_FILE_NAME_V1),
            temporary_path: path.join(FRONTIER_TEMPORARY_FILE_NAME_V1),
            path,
        }
    }

    fn create(store_path: &Path) -> Result<Self, TransferImportErrorV1> {
        let store = open_directory_handle(store_path).map_err(TransferImportErrorV1::Io)?;
        let path = store_path.join(STAGING_DIRECTORY_NAME_V1);
        match create_staging_directory(&path) {
            Ok(()) => {}
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                return Err(TransferImportErrorV1::StagingExists { path });
            }
            Err(source) => {
                return Err(TransferImportErrorV1::StagingDirectory {
                    action: "create",
                    path,
                    source,
                });
            }
        }
        let directory = open_directory_handle(&path).map_err(TransferImportErrorV1::Io)?;
        store
            .sync_all()
            .map_err(io_error(DurableIoOperation::SyncDirectory, store_path))
            .map_err(TransferImportErrorV1::Io)?;
        Ok(Self::with_handles(store, store_path, directory, path))
    }

    fn open(store_path: &Path) -> Result<Option<Self>, TransferImportErrorV1> {
        let store = open_directory_handle(store_path).map_err(TransferImportErrorV1::Io)?;
        let path = store_path.join(STAGING_DIRECTORY_NAME_V1);
        match open_directory_handle(&path) {
            Ok(directory) => Ok(Some(Self::with_handles(store, store_path, directory, path))),
            Err(DurableIoError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(TransferImportErrorV1::Io(error)),
        }
    }

    fn open_objects(&self) -> Result<Option<StagedLogV1>, TransferImportErrorV1> {
        match open_staging_file(&self.objects_path, StagingFileOpenV1::ReadWrite) {
            Ok(objects) => lock_staged_objects(objects, &self.path).map(Some),
            Err(DurableIoError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(TransferImportErrorV1::Io(error)),
        }
    }

    fn read_frontier(&self) -> Result<FrontierRecordV1, TransferImportErrorV1> {
        let file = match open_staging_file(&self.frontier_path, StagingFileOpenV1::ReadOnly) {
            Ok(file) => file,
            Err(DurableIoError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Err(TransferImportErrorV1::IncompleteStaging {
                    path: self.path.clone(),
                });
            }
            Err(error) => return Err(TransferImportErrorV1::Io(error)),
        };
        let len = file
            .metadata()
            .map_err(io_error(DurableIoOperation::Metadata, &self.frontier_path))
            .map_err(TransferImportErrorV1::Io)?
            .len();
        if len != FRONTIER_RECORD_LEN_V1 as u64 {
            return Err(TransferImportErrorV1::Frontier {
                path: self.frontier_path.clone(),
                error: TransferFrontierFormatErrorV1::Length { len },
            });
        }
        let mut record = [0_u8; FRONTIER_RECORD_LEN_V1];
        (&file)
            .read_exact(&mut record)
            .map_err(io_error(DurableIoOperation::PositionalRead, &self.frontier_path))
            .map_err(TransferImportErrorV1::Io)?;
        decode_frontier(&record).map_err(|error| TransferImportErrorV1::Frontier {
            path: self.frontier_path.clone(),
            error,
        })
    }

    fn write_frontier(&self, token: TransferTokenV1) -> Result<(), DurableIoError> {
        let record = encode_frontier(token);
        remove_staging_file(&self.temporary_path)?;
        let mut candidate = open_staging_file(&self.temporary_path, StagingFileOpenV1::Create)?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::CandidateWrite,
            DurableIoOperation::PositionalWrite,
            &self.temporary_path,
        )?;
        candidate
            .write_all(&record)
            .map_err(io_error(DurableIoOperation::PositionalWrite, &self.temporary_path))?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::CandidateSync,
            DurableIoOperation::SyncFile,
            &self.temporary_path,
        )?;
        candidate
            .sync_all()
            .map_err(io_error(DurableIoOperation::SyncFile, &self.temporary_path))?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::BeforeReplace,
            DurableIoOperation::ReplaceFile,
            &self.temporary_path,
        )?;
        validate_authority_file(&candidate, DurableIoOperation::ReplaceFile, &self.temporary_path)?;
        std::fs::rename(&self.temporary_path, &self.frontier_path)
            .map_err(io_error(DurableIoOperation::ReplaceFile, &self.frontier_path))?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::AfterReplaceBeforeDirectorySync,
            DurableIoOperation::SyncDirectory,
            &self.path,
        )?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::DirectorySync,
            DurableIoOperation::SyncDirectory,
            &self.path,
        )?;
        self.directory
            .sync_all()
            .map_err(io_error(DurableIoOperation::SyncDirectory, &self.path))
    }

    fn remove(&self) -> Result<(), TransferImportErrorV1> {
        for path in [&self.frontier_path, &self.temporary_path, &self.objects_path] {
            remove_staging_file(path).map_err(TransferImportErrorV1::Io)?;
        }
        std::fs::remove_dir(&self.path).map_err(|source| {
            TransferImportErrorV1::StagingDirectory {
                action: "remove",
                path: self.path.clone(),
                source,
            }
        })?;
        self.store
            .sync_all()
            .map_err(io_error(DurableIoOperation::SyncDirectory, &self.store_path))
            .map_err(TransferImportErrorV1::Io)
    }
}

pub(crate) struct DurableImportSessionV1<P: SemanticProfile> {
    staging: StagingDirectoryV1,
    objects: StagedLogV1,
    limits: TransferPageLimitsV1,
    closure: StagedClosureV1,
    frontier: u64,
    staged_len: u64,
    failed: bool,
    profile: PhantomData<fn() -> P>,
}

impl<P: SemanticProfile> DurableImportSessionV1<P> {
    pub(crate) fn begin_import(
        store_directory: &Path,
        binding: TransferBindingV1,
        limits: TransferPageLimitsV1,
        bounds: StructuralValidationLimits,
    ) -> Result<Self, TransferImportErrorV1> {
        require_store_profile::<P>(binding)?;
        let staging = StagingDirectoryV1::create(store_directory)?;
        let objects = open_staging_file(&staging.objects_path, StagingFileOpenV1::Create)
            .map_err(TransferImportErrorV1::Io)?;
        let objects = lock_staged_objects(objects, &staging.path)?;
        objects
            .file
            .sync_all()
            .map_err(io_error(DurableIoOperation::SyncFile, &staging.objects_path))
            .map_err(TransferImportErrorV1::Io)?;
        staging
            .write_frontier(TransferTokenV1::new(binding, 0))
            .map_err(TransferImportErrorV1::Io)?;
        Ok(Self {
            staging,
            objects,
            limits,
            closure: StagedClosureV1::new(binding, bounds),
            frontier: 0,
            staged_len: 0,
            failed: false,
            profile: PhantomData,
        })
    }

    pub(crate) fn resume_import(
        store_directory: &Path,
        binding: TransferBindingV1,
        limits: TransferPageLimitsV1,
        bounds: StructuralValidationLimits,
    ) -> Result<Self, TransferImportErrorV1> {
        require_store_profile::<P>(binding)?;
        let Some(staging) = StagingDirectoryV1::open(store_directory)? else {
            return Err(TransferImportErrorV1::NoStaging {
                path: store_directory.join(STAGING_DIRECTORY_NAME_V1),
            });
        };
        let Some(mut objects) = staging.open_objects()? else {
            return Err(TransferImportErrorV1::IncompleteStaging { path: staging.path });
        };
        let record = staging.read_frontier()?;
        match record.version {
            TRANSFER_PAGE_VERSION_V1 => {}
            version => return Err(TransferImportErrorV1::UnsupportedTransferVersion { version }),
        }
        match (record.root == binding.root_digest(), record.profile == binding.profile()) {
            (true, true) => {}
            _ => {
                return Err(TransferImportErrorV1::BindingMismatch {
                    staged_root: record.root,
                    staged_profile: record.profile,
                    requested: binding,
                });
            }
        }
        let mut closure = StagedClosureV1::new(binding, bounds);
        let staged_len = readmit_staged_objects(
            &mut objects.file,
            &staging.objects_path,
            &mut closure,
            record.position,
            limits,
        )?;
        Ok(Self {
            staging,
            objects,
            limits,
            closure,
            frontier: record.position,
            staged_len,
            failed: false,
            profile: PhantomData,
        })
    }

    pub(crate) const fn frontier(&self) -> TransferTokenV1 {
        TransferTokenV1::new(self.closure.binding, self.frontier)
    }

    pub(crate) const fn outstanding(&self) -> u64 { self.closure.counters.outstanding }

    #[cfg(test)]
    pub(crate) fn duplicate_log_description(&self) -> io::Result<File> {
        self.objects.file.try_clone()
    }

    pub(crate) fn stage_page(
        &mut self,
        page: &[u8],
    ) -> Result<TransferStagingOutcomeV1, TransferImportErrorV1> {
        if self.failed {
            return Err(TransferImportErrorV1::SessionFailed);
        }
        let view =
            TransferPageViewV1::parse(page, self.limits).map_err(TransferImportErrorV1::Page)?;
        if view.binding() != self.closure.binding {
            return Err(TransferImportErrorV1::PageBinding {
                session: self.closure.binding,
                page: view.binding(),
            });
        }
        if view.position() != self.frontier {
            return Err(TransferImportErrorV1::PositionMismatch {
                expected: self.frontier,
                found: view.position(),
            });
        }
        if view.object_count() == 0 {
            return match (self.frontier, self.closure.counters.outstanding) {
                (1.., 0) => Ok(TransferStagingOutcomeV1::Complete(self.frontier())),
                (position, outstanding) => Err(TransferImportErrorV1::IncompleteAtEnd {
                    position,
                    outstanding,
                }),
            };
        }
        let saved = self.closure.counters;
        for (object, position) in view.objects().zip(self.frontier..) {
            if let Err(refusal) = self.closure.admit(position, object) {
                self.closure.rollback(saved);
                return Err(TransferImportErrorV1::Object { position, refusal });
            }
        }
        let frontier = self.frontier + view.object_count();
        match self.persist(view.records(), frontier) {
            Ok(staged_len) => {
                self.closure.commit();
                self.frontier = frontier;
                self.staged_len = staged_len;
                Ok(TransferStagingOutcomeV1::Staged(self.frontier()))
            }
            Err(error) => {
                self.closure.rollback(saved);
                self.failed = true;
                Err(TransferImportErrorV1::Io(error))
            }
        }
    }

    fn persist(&mut self, records: &[u8], frontier: u64) -> Result<u64, DurableIoError> {
        let path = &self.staging.objects_path;
        let appended = u64::try_from(records.len()).unwrap_or(u64::MAX);
        let end = self.staged_len.checked_add(appended).ok_or_else(|| {
            DurableIoError::OffsetOverflow {
                operation: DurableIoOperation::PositionalWrite,
                path: path.clone(),
                offset: self.staged_len,
                completed: appended,
            }
        })?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::PhysicalWrite,
            DurableIoOperation::PositionalWrite,
            path,
        )?;
        self.objects
            .file
            .seek(SeekFrom::Start(self.staged_len))
            .map_err(io_error(DurableIoOperation::PositionalWrite, path))?;
        self.objects
            .file
            .write_all(records)
            .map_err(io_error(DurableIoOperation::PositionalWrite, path))?;
        validate_authority_file(&self.objects.file, DurableIoOperation::SyncFile, path)?;
        #[cfg(test)]
        trip_durable_io_failpoint(
            DurableIoFailpointV1::PhysicalSync,
            DurableIoOperation::SyncFile,
            path,
        )?;
        self.objects
            .file
            .sync_all()
            .map_err(io_error(DurableIoOperation::SyncFile, path))?;
        self.staging
            .write_frontier(TransferTokenV1::new(self.closure.binding, frontier))?;
        Ok(end)
    }

    pub(crate) fn abort(self) -> Result<(), TransferImportErrorV1> { self.staging.remove() }

    pub(crate) fn seal(
        mut self,
        limits: SemanticValidationLimits,
    ) -> Result<DurableSealedImportV1<P>, Box<(Self, TransferImportErrorV1)>> {
        let manifest = match self.complete_manifest() {
            Ok(manifest) => manifest,
            Err(error) => return Err(Box::new((self, error))),
        };
        let locations = match index_staged_objects(
            &mut self.objects.file,
            &self.staging.objects_path,
            self.frontier,
            self.staged_len,
            self.limits,
        ) {
            Ok(locations) => locations,
            Err(error) => return Err(Box::new((self, error))),
        };
        let Self {
            staging,
            objects,
            limits: page_limits,
            closure,
            frontier,
            staged_len,
            failed,
            profile,
        } = self;
        let store = StagedObjectStoreV1 {
            path: staging.objects_path.clone(),
            log: objects,
            locations,
        };
        match verify_rspace_semantics::<P, _>(&store, manifest, limits) {
            Ok(admission) => Ok(DurableSealedImportV1 {
                staging,
                store,
                manifest,
                admission,
                profile,
            }),
            Err(error) => {
                let session = Self {
                    staging,
                    objects: store.log,
                    limits: page_limits,
                    closure,
                    frontier,
                    staged_len,
                    failed,
                    profile,
                };
                Err(Box::new((session, cold_admission_refusal(error))))
            }
        }
    }

    fn complete_manifest(&self) -> Result<ObjectRefV1, TransferImportErrorV1> {
        let root = self.closure.binding.root_digest();
        match (
            self.failed,
            self.frontier,
            self.closure.counters.outstanding,
            self.closure.known.get(&root),
        ) {
            (true, ..) => Err(TransferImportErrorV1::SessionFailed),
            (
                false,
                1..,
                0,
                Some(KnownObjectV1 {
                    encoded_len,
                    staged: true,
                }),
            ) => ObjectRefV1::new(root, *encoded_len).map_err(TransferImportErrorV1::ColdAdmission),
            (false, position, outstanding, _) => Err(TransferImportErrorV1::IncompleteClosure {
                position,
                outstanding,
            }),
        }
    }
}

pub(crate) struct DurableSealedImportV1<P: SemanticProfile> {
    staging: StagingDirectoryV1,
    store: StagedObjectStoreV1,
    manifest: ObjectRefV1,
    admission: SemanticValidationReport,
    profile: PhantomData<fn() -> P>,
}

impl<P: SemanticProfile> DurableSealedImportV1<P> {
    pub(crate) const fn manifest(&self) -> ObjectRefV1 { self.manifest }

    pub(crate) const fn admission(&self) -> SemanticValidationReport { self.admission }

    pub(crate) const fn store(&self) -> &StagedObjectStoreV1 { &self.store }

    pub(crate) fn abort(self) -> Result<(), TransferImportErrorV1> { self.staging.remove() }
}

#[derive(Clone, Copy, Debug)]
struct StagedObjectLocationV1 {
    offset: u64,
    encoded_len: u64,
}

pub(crate) struct StagedObjectStoreV1 {
    log: StagedLogV1,
    path: PathBuf,
    locations: HashMap<ObjectDigest, StagedObjectLocationV1>,
}

impl StagedObjectStoreV1 {
    fn read(&self, location: StagedObjectLocationV1) -> Result<Box<[u8]>, TransferImportErrorV1> {
        let object_len = usize::try_from(location.encoded_len)
            .expect("a staged object is bounded by the page limit, which fits the address space");
        let mut object = Vec::new();
        object
            .try_reserve_exact(object_len)
            .map_err(|_| TransferImportErrorV1::Allocation {
                resource: "staged object buffer",
                requested: location.encoded_len,
            })?;
        object.resize(object_len, 0);
        super::io::read_exact_at_with(
            &self.path,
            location.offset,
            &mut object,
            |buffer, position| {
                super::io::positioned_read(&self.log.file, buffer, position, &self.path)
            },
        )
        .map_err(TransferImportErrorV1::Io)?;
        Ok(object.into_boxed_slice())
    }
}

impl ImmutableObjectStore for StagedObjectStoreV1 {
    type Error = TransferImportErrorV1;
    type Object = Box<[u8]>;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error> {
        match self.locations.get(digest) {
            None => Ok(None),
            Some(location) if location.encoded_len > max_encoded_len => {
                Err(TransferImportErrorV1::StagedObjectBound {
                    digest: *digest,
                    staged: location.encoded_len,
                    limit: max_encoded_len,
                })
            }
            Some(location) => self.read(*location).map(Some),
        }
    }
}

fn cold_admission_refusal(
    error: crate::rspace::persistence::ClosureError<TransferImportErrorV1>,
) -> TransferImportErrorV1 {
    use crate::rspace::persistence::ClosureError;

    match error {
        ClosureError::Format(error) => TransferImportErrorV1::ColdAdmission(error),
        ClosureError::Store(error) => error,
    }
}

fn index_staged_objects(
    objects: &mut File,
    path: &Path,
    frontier: u64,
    staged_len: u64,
    limits: TransferPageLimitsV1,
) -> Result<HashMap<ObjectDigest, StagedObjectLocationV1>, TransferImportErrorV1> {
    let log_len = objects
        .metadata()
        .map_err(io_error(DurableIoOperation::Metadata, path))
        .map_err(TransferImportErrorV1::Io)?
        .len()
        .min(staged_len);
    let indexed = frontier.min(log_len / MIN_STAGED_RECORD_LEN);
    let capacity = usize::try_from(indexed).unwrap_or(usize::MAX);
    let mut locations = HashMap::new();
    locations
        .try_reserve(capacity)
        .map_err(|_| TransferImportErrorV1::Allocation {
            resource: "staged object index",
            requested: indexed,
        })?;
    objects
        .seek(SeekFrom::Start(0))
        .map_err(io_error(DurableIoOperation::PositionalRead, path))
        .map_err(TransferImportErrorV1::Io)?;
    let reader_capacity = usize::try_from(log_len.min(STAGED_READ_BUFFER_LEN))
        .expect("the staged read buffer length fits the address space");
    let mut reader = BufReader::with_capacity(reader_capacity, &*objects);
    let mut offset = 0_u64;
    for position in 0..frontier {
        let remaining = log_len - offset;
        if remaining < STAGED_LENGTH_FIELD_LEN as u64 {
            return Err(TransferImportErrorV1::StagedObjectsMissing {
                frontier,
                staged: position,
            });
        }
        let mut length_field = [0_u8; STAGED_LENGTH_FIELD_LEN];
        reader
            .read_exact(&mut length_field)
            .map_err(io_error(DurableIoOperation::PositionalRead, path))
            .map_err(TransferImportErrorV1::Io)?;
        let len = u64::from_le_bytes(length_field);
        if len < OBJECT_ENVELOPE_HEADER_LEN || len > limits.max_page_bytes() {
            return Err(TransferImportErrorV1::StagedRecordLength {
                position,
                len,
                max_page_bytes: limits.max_page_bytes(),
            });
        }
        if len > remaining - STAGED_LENGTH_FIELD_LEN as u64 {
            return Err(TransferImportErrorV1::StagedObjectsMissing {
                frontier,
                staged: position,
            });
        }
        let digest =
            digest_staged_object(&mut reader, len, path).map_err(TransferImportErrorV1::Io)?;
        let location = StagedObjectLocationV1 {
            offset: offset + STAGED_LENGTH_FIELD_LEN as u64,
            encoded_len: len,
        };
        match locations.entry(digest) {
            Entry::Vacant(entry) => {
                entry.insert(location);
            }
            Entry::Occupied(_) => {
                return Err(TransferImportErrorV1::StagedObject {
                    position,
                    refusal: TransferObjectRefusalV1::Duplicate { digest },
                });
            }
        }
        offset += STAGED_LENGTH_FIELD_LEN as u64 + len;
    }
    Ok(locations)
}

fn digest_staged_object(
    reader: &mut BufReader<&File>,
    len: u64,
    path: &Path,
) -> Result<ObjectDigest, DurableIoError> {
    let mut remaining = usize::try_from(len)
        .expect("a staged object is bounded by the page limit, which fits the address space");
    let mut hasher = Blake2b::<U32>::new();
    while remaining > 0 {
        let buffered = match io::BufRead::fill_buf(reader) {
            Ok(buffered) => buffered,
            Err(source) if source.kind() == io::ErrorKind::Interrupted => continue,
            Err(source) => return Err(io_error(DurableIoOperation::PositionalRead, path)(source)),
        };
        let taken = remaining.min(buffered.len());
        if taken == 0 {
            return Err(io_error(DurableIoOperation::PositionalRead, path)(
                io::ErrorKind::UnexpectedEof.into(),
            ));
        }
        hasher.update(&buffered[..taken]);
        io::BufRead::consume(reader, taken);
        remaining -= taken;
    }
    let mut digest = [0_u8; OBJECT_DIGEST_LEN];
    digest.copy_from_slice(&hasher.finalize());
    Ok(ObjectDigest::from_array(digest))
}

pub(crate) fn abort_import(store_directory: &Path) -> Result<(), TransferImportErrorV1> {
    let Some(staging) = StagingDirectoryV1::open(store_directory)? else {
        return Ok(());
    };
    let _lock = staging.open_objects()?;
    staging.remove()
}

fn readmit_staged_objects(
    objects: &mut File,
    path: &Path,
    closure: &mut StagedClosureV1,
    frontier: u64,
    limits: TransferPageLimitsV1,
) -> Result<u64, TransferImportErrorV1> {
    let log_len = objects
        .metadata()
        .map_err(io_error(DurableIoOperation::Metadata, path))
        .map_err(TransferImportErrorV1::Io)?
        .len();
    let indexed = frontier
        .min(log_len / MIN_STAGED_RECORD_LEN)
        .min(closure.bounds.max_distinct_objects);
    let capacity = usize::try_from(indexed).unwrap_or(usize::MAX);
    closure
        .known
        .try_reserve(capacity)
        .map_err(|_| TransferImportErrorV1::Allocation {
            resource: "staged closure index",
            requested: indexed,
        })?;
    objects
        .seek(SeekFrom::Start(0))
        .map_err(io_error(DurableIoOperation::PositionalRead, path))
        .map_err(TransferImportErrorV1::Io)?;
    let reader_capacity = usize::try_from(log_len.min(STAGED_READ_BUFFER_LEN))
        .expect("the staged read buffer length fits the address space");
    let mut reader = BufReader::with_capacity(reader_capacity, &*objects);
    let mut object = Vec::new();
    let mut offset = 0_u64;
    for position in 0..frontier {
        let remaining = log_len - offset;
        if remaining < STAGED_LENGTH_FIELD_LEN as u64 {
            return Err(TransferImportErrorV1::StagedObjectsMissing {
                frontier,
                staged: position,
            });
        }
        let mut length_field = [0_u8; STAGED_LENGTH_FIELD_LEN];
        reader
            .read_exact(&mut length_field)
            .map_err(io_error(DurableIoOperation::PositionalRead, path))
            .map_err(TransferImportErrorV1::Io)?;
        let len = u64::from_le_bytes(length_field);
        if len < OBJECT_ENVELOPE_HEADER_LEN || len > limits.max_page_bytes() {
            return Err(TransferImportErrorV1::StagedRecordLength {
                position,
                len,
                max_page_bytes: limits.max_page_bytes(),
            });
        }
        if len > remaining - STAGED_LENGTH_FIELD_LEN as u64 {
            return Err(TransferImportErrorV1::StagedObjectsMissing {
                frontier,
                staged: position,
            });
        }
        let object_len = usize::try_from(len)
            .expect("a staged object is bounded by the page limit, which fits the address space");
        object.clear();
        object
            .try_reserve_exact(object_len)
            .map_err(|_| TransferImportErrorV1::Allocation {
                resource: "staged object buffer",
                requested: len,
            })?;
        object.resize(object_len, 0);
        reader
            .read_exact(&mut object)
            .map_err(io_error(DurableIoOperation::PositionalRead, path))
            .map_err(TransferImportErrorV1::Io)?;
        closure
            .admit(position, &object)
            .map_err(|refusal| TransferImportErrorV1::StagedObject { position, refusal })?;
        closure.commit();
        offset += STAGED_LENGTH_FIELD_LEN as u64 + len;
    }
    drop(reader);
    if log_len != offset {
        objects
            .set_len(offset)
            .map_err(io_error(DurableIoOperation::SetLength, path))
            .map_err(TransferImportErrorV1::Io)?;
        objects
            .sync_all()
            .map_err(io_error(DurableIoOperation::SyncFile, path))
            .map_err(TransferImportErrorV1::Io)?;
    }
    Ok(offset)
}

fn require_store_profile<P: SemanticProfile>(
    binding: TransferBindingV1,
) -> Result<(), TransferImportErrorV1> {
    match binding.profile() == P::ID {
        true => Ok(()),
        false => Err(TransferImportErrorV1::ForeignProfile {
            store: P::ID,
            requested: binding.profile(),
        }),
    }
}

struct StagedLogV1 {
    file: File,
}

impl Drop for StagedLogV1 {
    fn drop(&mut self) { let _ = self.file.unlock(); }
}

fn lock_staged_objects(
    objects: File,
    staging: &Path,
) -> Result<StagedLogV1, TransferImportErrorV1> {
    match objects.try_lock() {
        Ok(()) => Ok(StagedLogV1 { file: objects }),
        Err(TryLockError::WouldBlock) => Err(TransferImportErrorV1::StagingInUse {
            path: staging.to_path_buf(),
        }),
        Err(TryLockError::Error(source)) => Err(TransferImportErrorV1::Io(DurableIoError::Io {
            operation: DurableIoOperation::AcquireWriterLock,
            path: staging.join(STAGED_OBJECTS_FILE_NAME_V1),
            source,
        })),
    }
}

fn remove_staging_file(path: &Path) -> Result<(), DurableIoError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(DurableIoError::Io {
            operation: DurableIoOperation::RemoveFile,
            path: path.to_path_buf(),
            source,
        }),
    }
}

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

#[derive(Clone, Copy, Debug)]
enum StagingFileOpenV1 {
    Create,
    ReadWrite,
    ReadOnly,
}

impl StagingFileOpenV1 {
    const fn operation(self) -> DurableIoOperation {
        match self {
            Self::Create => DurableIoOperation::CreateFile,
            Self::ReadWrite | Self::ReadOnly => DurableIoOperation::OpenFile,
        }
    }
}

#[cfg(unix)]
fn create_staging_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    std::fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_staging_directory(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "owner-only import staging directories are unsupported on this platform",
    ))
}

#[cfg(unix)]
fn open_staging_file(path: &Path, mode: StagingFileOpenV1) -> Result<File, DurableIoError> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let operation = mode.operation();
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    match mode {
        StagingFileOpenV1::Create => {
            options.write(true).create_new(true).mode(0o600);
        }
        StagingFileOpenV1::ReadWrite => {
            options.write(true);
        }
        StagingFileOpenV1::ReadOnly => {}
    }
    let file = options.open(path).map_err(io_error(operation, path))?;
    if let StagingFileOpenV1::Create = mode {
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(io_error(operation, path))?;
    }
    validate_authority_file(&file, operation, path)?;
    Ok(file)
}

#[cfg(not(unix))]
fn open_staging_file(path: &Path, mode: StagingFileOpenV1) -> Result<File, DurableIoError> {
    Err(DurableIoError::UnsupportedPlatform {
        operation: mode.operation(),
        path: path.to_path_buf(),
    })
}
