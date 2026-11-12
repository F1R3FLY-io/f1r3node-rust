use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt::{Display, Formatter};

use super::admission::DurableAuthorityCoreV1;
use super::state_root::{DurableStateReadHandleErrorV1, DurableStateRootV1};
use super::{DurableAuthorityViewV1, DurableObjectReadErrorV1, DurableViewReadHandleV1};
use crate::rspace::persistence::canonical::{OBJECT_DIGEST_LEN, OBJECT_ENVELOPE_HEADER_LEN};
use crate::rspace::persistence::checksum::{
    CHECKSUM_FIELD_LEN, crc32c, decode_crc32c, encode_crc32c,
};
use crate::rspace::persistence::closure::{ClosureDomainV1, StructuralClosureLedgerV1};
use crate::rspace::persistence::manifest::SEMANTIC_PROFILE_ID_LEN;
use crate::rspace::persistence::object::CanonicalObjectView;
use crate::rspace::persistence::{
    CanonicalObjectKind, ClosureError, EndpointValue, ObjectDigest, ObjectRefV1, SemanticProfile,
    SemanticProfileId,
};

pub(super) const TRANSFER_PAGE_VERSION_V1: u64 = 1;
const ROOT_OFFSET: usize = 8;
const PROFILE_OFFSET: usize = ROOT_OFFSET + OBJECT_DIGEST_LEN;
const POSITION_OFFSET: usize = PROFILE_OFFSET + SEMANTIC_PROFILE_ID_LEN;
const COUNT_OFFSET: usize = POSITION_OFFSET + 8;
const PAGE_HEADER_LEN: usize = COUNT_OFFSET + 8;
const OBJECT_LENGTH_FIELD_LEN: usize = 8;
const EMPTY_PAGE_LEN: usize = PAGE_HEADER_LEN + CHECKSUM_FIELD_LEN;
const MIN_PAGE_BYTES: u64 =
    (EMPTY_PAGE_LEN + OBJECT_LENGTH_FIELD_LEN) as u64 + OBJECT_ENVELOPE_HEADER_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TransferBindingV1 {
    root: ObjectDigest,
    profile: SemanticProfileId,
}

impl TransferBindingV1 {
    pub(crate) const fn new(root: DurableStateRootV1, profile: SemanticProfileId) -> Self {
        Self {
            root: root.digest(),
            profile,
        }
    }

    pub(crate) const fn root_digest(self) -> ObjectDigest { self.root }

    pub(crate) const fn profile(self) -> SemanticProfileId { self.profile }
}

impl Display for TransferBindingV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "durable state root {} of semantic profile {}",
            self.root,
            hex::encode(self.profile.as_bytes())
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TransferTokenV1 {
    binding: TransferBindingV1,
    position: u64,
}

impl TransferTokenV1 {
    pub(crate) const fn new(binding: TransferBindingV1, position: u64) -> Self {
        Self { binding, position }
    }

    pub(crate) const fn binding(self) -> TransferBindingV1 { self.binding }

    pub(crate) const fn position(self) -> u64 { self.position }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TransferPageLimitsV1 {
    max_page_bytes: u64,
    max_page_objects: u64,
}

impl TransferPageLimitsV1 {
    pub(crate) fn new(
        max_page_bytes: u64,
        max_page_objects: u64,
    ) -> Result<Self, TransferExportErrorV1> {
        match (usize::try_from(max_page_bytes), max_page_objects) {
            (Ok(_), 1..) if max_page_bytes >= MIN_PAGE_BYTES => Ok(Self {
                max_page_bytes,
                max_page_objects,
            }),
            _ => Err(TransferExportErrorV1::PageLimits {
                max_page_bytes,
                max_page_objects,
            }),
        }
    }

    pub(crate) const fn max_page_bytes(self) -> u64 { self.max_page_bytes }

    pub(crate) const fn max_page_objects(self) -> u64 { self.max_page_objects }

    fn max_page_len(self) -> usize {
        usize::try_from(self.max_page_bytes)
            .expect("transfer page limits are validated to fit the address space")
    }
}

#[derive(Debug)]
pub(crate) enum TransferExportErrorV1 {
    Open(DurableStateReadHandleErrorV1),
    PageLimits {
        max_page_bytes: u64,
        max_page_objects: u64,
    },
    PinReleased {
        binding: TransferBindingV1,
    },
    BindingMismatch {
        session: TransferBindingV1,
        requested: TransferBindingV1,
    },
    PositionNotServed {
        requested: u64,
        served_end: u64,
    },
    ObjectExceedsPage {
        digest: ObjectDigest,
        encoded_len: u64,
        max_page_bytes: u64,
    },
    UnexpectedObjectKind {
        digest: ObjectDigest,
        position: u64,
        kind: CanonicalObjectKind,
    },
    Object(ClosureError<DurableObjectReadErrorV1>),
    AllocationFailure {
        resource: &'static str,
        requested: u64,
    },
}

impl Display for TransferExportErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(error) => write!(formatter, "durable state cannot be exported: {error}"),
            Self::PageLimits {
                max_page_bytes,
                max_page_objects,
            } => write!(
                formatter,
                "transfer page limits of {max_page_bytes} bytes and {max_page_objects} objects \
                 admit no object; a page needs at least {MIN_PAGE_BYTES} bytes and one object"
            ),
            Self::PinReleased { binding } => write!(
                formatter,
                "the export session of {binding} released its pin and serves no page"
            ),
            Self::BindingMismatch { session, requested } => write!(
                formatter,
                "a transfer token bound to {requested} was presented to the export session of \
                 {session}"
            ),
            Self::PositionNotServed {
                requested,
                served_end,
            } => write!(
                formatter,
                "transfer position {requested} lies beyond the {served_end} objects this export \
                 session has served"
            ),
            Self::ObjectExceedsPage {
                digest,
                encoded_len,
                max_page_bytes,
            } => write!(
                formatter,
                "object {digest} of {encoded_len} bytes does not fit a transfer page of \
                 {max_page_bytes} bytes"
            ),
            Self::UnexpectedObjectKind {
                digest,
                position,
                kind,
            } => write!(
                formatter,
                "object {digest} at transfer position {position} is a {} object, which a semantic \
                 state closure cannot hold there",
                kind.name()
            ),
            Self::Object(error) => {
                write!(formatter, "an object of the exported state cannot be read: {error}")
            }
            Self::AllocationFailure {
                resource,
                requested,
            } => write!(formatter, "cannot allocate {requested} more entries for the {resource}"),
        }
    }
}

impl std::error::Error for TransferExportErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open(error) => Some(error),
            Self::Object(error) => Some(error),
            Self::PageLimits { .. } |
            Self::PinReleased { .. } |
            Self::BindingMismatch { .. } |
            Self::PositionNotServed { .. } |
            Self::ObjectExceedsPage { .. } |
            Self::UnexpectedObjectKind { .. } |
            Self::AllocationFailure { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransferPageFormatErrorV1 {
    TooShort {
        len: u64,
    },
    TooLong {
        len: u64,
        max_page_bytes: u64,
    },
    ChecksumOutOfRange(u64),
    Checksum {
        expected: u32,
        actual: u32,
    },
    UnsupportedVersion(u64),
    TooManyObjects {
        count: u64,
        max_page_objects: u64,
    },
    TruncatedObjectLength {
        index: u64,
    },
    ObjectTooShort {
        index: u64,
        len: u64,
    },
    ObjectOverrunsPage {
        index: u64,
        len: u64,
        remaining: u64,
    },
    TrailingBytes {
        remaining: u64,
    },
    PositionOverflow {
        position: u64,
        count: u64,
    },
}

impl Display for TransferPageFormatErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { len } => write!(
                formatter,
                "a transfer page of {len} bytes is shorter than its {EMPTY_PAGE_LEN}-byte header \
                 and checksum"
            ),
            Self::TooLong {
                len,
                max_page_bytes,
            } => write!(
                formatter,
                "a transfer page of {len} bytes exceeds the {max_page_bytes}-byte page limit"
            ),
            Self::ChecksumOutOfRange(value) => {
                write!(formatter, "transfer page checksum field {value:#x} exceeds 32 bits")
            }
            Self::Checksum { expected, actual } => write!(
                formatter,
                "transfer page checksum mismatch: recorded {expected:#010x}, computed \
                 {actual:#010x}"
            ),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported transfer page version {version}")
            }
            Self::TooManyObjects {
                count,
                max_page_objects,
            } => write!(
                formatter,
                "a transfer page declares {count} objects, above the limit of {max_page_objects}"
            ),
            Self::TruncatedObjectLength { index } => {
                write!(formatter, "transfer page object {index} has no complete length field")
            }
            Self::ObjectTooShort { index, len } => write!(
                formatter,
                "transfer page object {index} declares {len} bytes, fewer than a canonical object \
                 envelope"
            ),
            Self::ObjectOverrunsPage {
                index,
                len,
                remaining,
            } => write!(
                formatter,
                "transfer page object {index} declares {len} bytes but only {remaining} remain"
            ),
            Self::TrailingBytes { remaining } => write!(
                formatter,
                "a transfer page carries {remaining} bytes after its declared objects"
            ),
            Self::PositionOverflow { position, count } => write!(
                formatter,
                "a transfer page at position {position} with {count} objects overflows the stream \
                 position"
            ),
        }
    }
}

impl std::error::Error for TransferPageFormatErrorV1 {}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TransferPageViewV1<'a> {
    binding: TransferBindingV1,
    position: u64,
    object_count: u64,
    objects: &'a [u8],
}

impl<'a> TransferPageViewV1<'a> {
    pub(crate) fn parse(
        bytes: &'a [u8],
        limits: TransferPageLimitsV1,
    ) -> Result<Self, TransferPageFormatErrorV1> {
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if bytes.len() < EMPTY_PAGE_LEN {
            return Err(TransferPageFormatErrorV1::TooShort { len });
        }
        if len > limits.max_page_bytes {
            return Err(TransferPageFormatErrorV1::TooLong {
                len,
                max_page_bytes: limits.max_page_bytes,
            });
        }
        let (body, trailer) = bytes.split_at(bytes.len() - CHECKSUM_FIELD_LEN);
        let expected = decode_crc32c(
            trailer
                .try_into()
                .expect("the page trailer is exactly one checksum field"),
        )
        .map_err(TransferPageFormatErrorV1::ChecksumOutOfRange)?;
        let actual = crc32c(body);
        if expected != actual {
            return Err(TransferPageFormatErrorV1::Checksum { expected, actual });
        }
        match read_u64_at(body, 0) {
            TRANSFER_PAGE_VERSION_V1 => {}
            version => return Err(TransferPageFormatErrorV1::UnsupportedVersion(version)),
        }
        let root = ObjectDigest::from_array(
            body[ROOT_OFFSET..PROFILE_OFFSET]
                .try_into()
                .expect("the page header holds one object digest"),
        );
        let profile = SemanticProfileId::from_array(
            body[PROFILE_OFFSET..POSITION_OFFSET]
                .try_into()
                .expect("the page header holds one semantic profile id"),
        );
        let position = read_u64_at(body, POSITION_OFFSET);
        let object_count = read_u64_at(body, COUNT_OFFSET);
        if object_count > limits.max_page_objects {
            return Err(TransferPageFormatErrorV1::TooManyObjects {
                count: object_count,
                max_page_objects: limits.max_page_objects,
            });
        }
        let objects = &body[PAGE_HEADER_LEN..];
        let mut remaining = objects;
        for index in 0..object_count {
            let Some((length_field, rest)) =
                remaining.split_first_chunk::<OBJECT_LENGTH_FIELD_LEN>()
            else {
                return Err(TransferPageFormatErrorV1::TruncatedObjectLength { index });
            };
            let object_len = u64::from_le_bytes(*length_field);
            if object_len < OBJECT_ENVELOPE_HEADER_LEN {
                return Err(TransferPageFormatErrorV1::ObjectTooShort {
                    index,
                    len: object_len,
                });
            }
            let available = u64::try_from(rest.len()).unwrap_or(u64::MAX);
            match usize::try_from(object_len) {
                Ok(object_len_usize) if object_len <= available => {
                    remaining = &rest[object_len_usize..];
                }
                _ => {
                    return Err(TransferPageFormatErrorV1::ObjectOverrunsPage {
                        index,
                        len: object_len,
                        remaining: available,
                    });
                }
            }
        }
        if !remaining.is_empty() {
            return Err(TransferPageFormatErrorV1::TrailingBytes {
                remaining: u64::try_from(remaining.len()).unwrap_or(u64::MAX),
            });
        }
        if position.checked_add(object_count).is_none() {
            return Err(TransferPageFormatErrorV1::PositionOverflow {
                position,
                count: object_count,
            });
        }
        Ok(Self {
            binding: TransferBindingV1 { root, profile },
            position,
            object_count,
            objects,
        })
    }

    pub(crate) const fn binding(self) -> TransferBindingV1 { self.binding }

    pub(crate) const fn position(self) -> u64 { self.position }

    pub(crate) const fn object_count(self) -> u64 { self.object_count }

    pub(crate) const fn next_token(self) -> TransferTokenV1 {
        TransferTokenV1::new(self.binding, self.position + self.object_count)
    }

    pub(crate) const fn objects(self) -> TransferPageObjectsV1<'a> {
        TransferPageObjectsV1 {
            remaining: self.objects,
        }
    }

    pub(super) const fn records(self) -> &'a [u8] { self.objects }
}

pub(crate) struct TransferPageObjectsV1<'a> {
    remaining: &'a [u8],
}

impl<'a> Iterator for TransferPageObjectsV1<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let (length_field, rest) = self
            .remaining
            .split_first_chunk::<OBJECT_LENGTH_FIELD_LEN>()?;
        let object_len = usize::try_from(u64::from_le_bytes(*length_field))
            .expect("a parsed transfer page bounds every object length");
        let (object, rest) = rest.split_at(object_len);
        self.remaining = rest;
        Some(object)
    }
}

fn read_u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("the page header holds an eight-byte field at every fixed offset"),
    )
}

#[derive(Clone, Copy)]
struct PendingObjectV1 {
    reference: ObjectRefV1,
    shared: bool,
}

struct ExportTraversalV1 {
    manifest: ObjectRefV1,
    pending: Vec<ObjectRefV1>,
    emitted_shared: HashSet<ObjectDigest>,
    cursor: u64,
}

impl ExportTraversalV1 {
    fn new(manifest: ObjectRefV1) -> Self {
        Self {
            manifest,
            pending: vec![manifest],
            emitted_shared: HashSet::new(),
            cursor: 0,
        }
    }

    fn restart(&mut self) {
        self.pending.clear();
        self.pending.push(self.manifest);
        self.emitted_shared.clear();
        self.cursor = 0;
    }

    fn release(&mut self) {
        self.pending = Vec::new();
        self.emitted_shared = HashSet::new();
    }

    fn next_pending(&mut self, ledger: &StructuralClosureLedgerV1) -> Option<PendingObjectV1> {
        while let Some(&reference) = self.pending.last() {
            let digest = reference.digest();
            match ledger.count(ClosureDomainV1::Retained, digest) {
                1 => {
                    return Some(PendingObjectV1 {
                        reference,
                        shared: false,
                    });
                }
                _ if self.emitted_shared.contains(&digest) => {
                    self.pending.pop();
                }
                _ => {
                    return Some(PendingObjectV1 {
                        reference,
                        shared: true,
                    });
                }
            }
        }
        None
    }

    fn reserve_pending(&mut self, additional: usize) -> Result<(), TransferExportErrorV1> {
        self.pending
            .try_reserve(additional)
            .map_err(|_| TransferExportErrorV1::AllocationFailure {
                resource: "transfer traversal stack",
                requested: u64::try_from(additional).unwrap_or(u64::MAX),
            })
    }

    fn advance<P: SemanticProfile>(
        &mut self,
        core: &DurableAuthorityCoreV1<P>,
        next: PendingObjectV1,
    ) -> Result<Box<[u8]>, TransferExportErrorV1> {
        let digest = next.reference.digest();
        let bytes = core
            .verified_state()
            .load_object(next.reference)
            .map_err(TransferExportErrorV1::Object)?;
        let object = CanonicalObjectView::parse(&bytes)
            .map_err(|error| TransferExportErrorV1::Object(ClosureError::Format(error)))?;
        if next.shared {
            self.emitted_shared.try_reserve(1).map_err(|_| {
                TransferExportErrorV1::AllocationFailure {
                    resource: "record of emitted shared objects",
                    requested: 1,
                }
            })?;
        }
        match (self.cursor, object) {
            (0, CanonicalObjectView::SemanticManifest(manifest)) => {
                let roots = manifest.roots().iter();
                self.reserve_pending(roots.len())?;
                self.pending.pop();
                self.pending
                    .extend(roots.into_iter().rev().map(|(_, root)| root));
            }
            (1.., CanonicalObjectView::DagNode(node)) => {
                let children = node.children();
                self.reserve_pending(children.len() + 1)?;
                self.pending.pop();
                let first_child = self.pending.len();
                self.pending
                    .extend(children.iter().map(|child| child.target));
                self.pending[first_child..].reverse();
                if let EndpointValue::Content(content) = node.endpoint() {
                    self.pending.push(content);
                }
            }
            (1.., CanonicalObjectView::Blob(_)) => {
                self.pending.pop();
            }
            (position, other) => {
                return Err(TransferExportErrorV1::UnexpectedObjectKind {
                    digest,
                    position,
                    kind: other.kind(),
                });
            }
        }
        if next.shared {
            self.emitted_shared.insert(digest);
        }
        self.cursor += 1;
        Ok(bytes)
    }

    fn skip_to<P: SemanticProfile>(
        &mut self,
        core: &DurableAuthorityCoreV1<P>,
        position: u64,
    ) -> Result<(), TransferExportErrorV1> {
        while self.cursor < position {
            let Some(next) = self.next_pending(core.retained_closure()) else {
                return Err(TransferExportErrorV1::PositionNotServed {
                    requested: position,
                    served_end: self.cursor,
                });
            };
            self.advance(core, next)?;
        }
        Ok(())
    }
}

pub(crate) struct DurableExportSessionV1<P: SemanticProfile> {
    pin: Option<DurableViewReadHandleV1<P>>,
    binding: TransferBindingV1,
    limits: TransferPageLimitsV1,
    traversal: ExportTraversalV1,
    page: Vec<u8>,
    page_position: u64,
    served_end: u64,
}

impl<P: SemanticProfile> DurableAuthorityViewV1<P> {
    pub(crate) fn open_export(
        &self,
        root: DurableStateRootV1,
        limits: TransferPageLimitsV1,
    ) -> Result<DurableExportSessionV1<P>, TransferExportErrorV1> {
        let resolved = self.resolve(root).map_err(|error| {
            TransferExportErrorV1::Open(DurableStateReadHandleErrorV1::Root(error))
        })?;
        let pin = self
            .read_handle_at(&resolved)
            .map_err(TransferExportErrorV1::Open)?;
        Ok(DurableExportSessionV1 {
            pin: Some(pin),
            binding: TransferBindingV1::new(resolved.root(), P::ID),
            limits,
            traversal: ExportTraversalV1::new(resolved.manifest()),
            page: Vec::new(),
            page_position: 0,
            served_end: 0,
        })
    }
}

impl<P: SemanticProfile> DurableExportSessionV1<P> {
    pub(crate) const fn binding(&self) -> TransferBindingV1 { self.binding }

    pub(crate) const fn first_token(&self) -> TransferTokenV1 {
        TransferTokenV1::new(self.binding, 0)
    }

    pub(crate) fn page(&mut self, token: TransferTokenV1) -> Result<&[u8], TransferExportErrorV1> {
        let Some(pin) = self.pin.as_ref() else {
            return Err(TransferExportErrorV1::PinReleased {
                binding: self.binding,
            });
        };
        let position = token.position();
        match (token.binding() == self.binding, position.cmp(&self.served_end)) {
            (false, _) => Err(TransferExportErrorV1::BindingMismatch {
                session: self.binding,
                requested: token.binding(),
            }),
            (true, Ordering::Greater) => Err(TransferExportErrorV1::PositionNotServed {
                requested: position,
                served_end: self.served_end,
            }),
            (true, _) if !self.page.is_empty() && position == self.page_position => Ok(&self.page),
            (true, _) => {
                let core = pin.core.as_ref();
                if position != self.traversal.cursor {
                    self.traversal.restart();
                    self.traversal.skip_to(core, position)?;
                }
                self.page_position = position;
                match encode_page(
                    &mut self.traversal,
                    &mut self.page,
                    core,
                    self.binding,
                    self.limits,
                ) {
                    Ok(()) => {
                        self.served_end = self.served_end.max(self.traversal.cursor);
                        Ok(&self.page)
                    }
                    Err(error) => {
                        self.page.clear();
                        Err(error)
                    }
                }
            }
        }
    }

    pub(crate) fn release_pin(&mut self) {
        self.pin = None;
        self.page = Vec::new();
        self.traversal.release();
    }

    #[cfg(test)]
    pub(crate) fn footprint_for_test(&self) -> (usize, usize, usize) {
        (
            self.page.capacity(),
            self.traversal.pending.capacity(),
            self.traversal.emitted_shared.len(),
        )
    }
}

fn encode_page<P: SemanticProfile>(
    traversal: &mut ExportTraversalV1,
    page: &mut Vec<u8>,
    core: &DurableAuthorityCoreV1<P>,
    binding: TransferBindingV1,
    limits: TransferPageLimitsV1,
) -> Result<(), TransferExportErrorV1> {
    let max_page_len = limits.max_page_len();
    page.clear();
    reserve_page(page, EMPTY_PAGE_LEN, max_page_len)?;
    page.extend_from_slice(&TRANSFER_PAGE_VERSION_V1.to_le_bytes());
    page.extend_from_slice(binding.root.as_bytes());
    page.extend_from_slice(binding.profile.as_bytes());
    page.extend_from_slice(&traversal.cursor.to_le_bytes());
    page.extend_from_slice(&0_u64.to_le_bytes());
    let mut objects = 0_u64;
    while let Some(next) = traversal.next_pending(core.retained_closure()) {
        let encoded_len = next.reference.encoded_len();
        let page_len_with_object = u64::try_from(page.len())
            .unwrap_or(u64::MAX)
            .saturating_add(OBJECT_LENGTH_FIELD_LEN as u64)
            .saturating_add(encoded_len)
            .saturating_add(CHECKSUM_FIELD_LEN as u64);
        match (
            objects < limits.max_page_objects && page_len_with_object <= limits.max_page_bytes,
            objects,
        ) {
            (true, _) => {}
            (false, 0) => {
                return Err(TransferExportErrorV1::ObjectExceedsPage {
                    digest: next.reference.digest(),
                    encoded_len,
                    max_page_bytes: limits.max_page_bytes,
                });
            }
            (false, _) => break,
        }
        let bytes = traversal.advance(core, next)?;
        reserve_page(page, OBJECT_LENGTH_FIELD_LEN + bytes.len(), max_page_len)?;
        page.extend_from_slice(&encoded_len.to_le_bytes());
        page.extend_from_slice(&bytes);
        objects += 1;
    }
    page[COUNT_OFFSET..PAGE_HEADER_LEN].copy_from_slice(&objects.to_le_bytes());
    let checksum = encode_crc32c(crc32c(page));
    reserve_page(page, CHECKSUM_FIELD_LEN, max_page_len)?;
    page.extend_from_slice(&checksum);
    Ok(())
}

fn reserve_page(
    page: &mut Vec<u8>,
    additional: usize,
    max_page_len: usize,
) -> Result<(), TransferExportErrorV1> {
    let required = page.len() + additional;
    if page.capacity() >= required {
        return Ok(());
    }
    let target = required
        .max(page.capacity().saturating_mul(2))
        .min(max_page_len);
    page.try_reserve_exact(target - page.len()).map_err(|_| {
        TransferExportErrorV1::AllocationFailure {
            resource: "transfer page",
            requested: u64::try_from(target).unwrap_or(u64::MAX),
        }
    })
}
