use std::convert::Infallible;
use std::fmt::{Display, Formatter};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};

pub const OBJECT_DIGEST_LEN: usize = 32;
pub(crate) const FORMAT_VERSION_V1: u64 = 1;
pub(crate) const OBJECT_DOMAIN_LEN: usize = 16;
pub(crate) const OBJECT_ENVELOPE_HEADER_LEN: u64 = (OBJECT_DOMAIN_LEN + 8 + 8) as u64;

/// A portable, fixed-width content address.
///
/// Unlike the legacy hash wrapper this type cannot contain a malformed-length
/// digest and never serializes a platform-sized integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectDigest([u8; OBJECT_DIGEST_LEN]);

impl ObjectDigest {
    pub const fn from_array(bytes: [u8; OBJECT_DIGEST_LEN]) -> Self { Self(bytes) }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, CanonicalFormatError> {
        let actual = bytes.len();
        let bytes = bytes
            .try_into()
            .map_err(|_| CanonicalFormatError::InvalidDigestLength { actual })?;
        Ok(Self(bytes))
    }

    pub const fn as_array(&self) -> &[u8; OBJECT_DIGEST_LEN] { &self.0 }

    pub const fn as_bytes(&self) -> &[u8] { &self.0 }

    pub fn of_canonical_bytes(bytes: &[u8]) -> Self {
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(bytes);
        let hash = hasher.finalize();
        let mut digest = [0_u8; OBJECT_DIGEST_LEN];
        digest.copy_from_slice(&hash);
        Self(digest)
    }
}

impl Display for ObjectDigest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

/// A content address paired with the exact canonical object length.
///
/// The length lets pack readers preflight checked reads before allocation and
/// makes a stale or substituted address observable before object parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectRefV1 {
    digest: ObjectDigest,
    encoded_len: u64,
}

impl ObjectRefV1 {
    pub fn new(digest: ObjectDigest, encoded_len: u64) -> Result<Self, CanonicalFormatError> {
        if encoded_len < OBJECT_ENVELOPE_HEADER_LEN {
            return Err(CanonicalFormatError::ObjectLengthTooSmall(encoded_len));
        }
        Ok(Self {
            digest,
            encoded_len,
        })
    }

    pub const fn digest(self) -> ObjectDigest { self.digest }

    pub const fn encoded_len(self) -> u64 { self.encoded_len }

    pub(crate) fn write<S: CanonicalSink>(self, sink: &mut S) -> Result<(), S::Error> {
        sink.write_all(self.digest.as_bytes())?;
        write_u64(sink, self.encoded_len)
    }

    pub(crate) fn parse(cursor: &mut ReadCursor<'_>) -> Result<Self, CanonicalFormatError> {
        Self::new(cursor.read_digest()?, cursor.read_u64()?)
    }
}

/// A non-seeking sink for canonical object bytes.
///
/// Pack files, WAL records, hashers, network writers, and exact compatibility
/// buffers can implement the same interface. Encoders never require rewind.
pub trait CanonicalSink {
    type Error;

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

impl CanonicalSink for Vec<u8> {
    type Error = Infallible;

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.extend_from_slice(bytes);
        Ok(())
    }
}

pub trait CanonicalEncode {
    fn canonical_len(&self) -> u64;

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error>;

    /// Materialize canonical bytes only when a contiguous image is required.
    /// Production pack and WAL paths should call [`Self::write_canonical`]
    /// directly so large objects are never buffered merely for serialization.
    fn try_canonical_bytes(&self) -> Result<Vec<u8>, CanonicalFormatError> {
        let encoded_len = self.canonical_len();
        let capacity =
            usize::try_from(encoded_len).map_err(|_| CanonicalFormatError::LengthOutOfRange {
                field: "canonical object",
                value: encoded_len,
            })?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| CanonicalFormatError::AllocationFailure {
                requested: encoded_len,
            })?;
        match self.write_canonical(&mut bytes) {
            Ok(()) => {}
            Err(never) => match never {},
        }
        let actual = checked_len(bytes.len(), "canonical object")?;
        if actual != encoded_len {
            return Err(CanonicalFormatError::ObjectLengthMismatch {
                expected: encoded_len,
                actual,
            });
        }
        Ok(bytes)
    }

    fn canonical_digest(&self) -> ObjectDigest {
        let mut sink = DigestSink(Blake2b::<U32>::new());
        match self.write_canonical(&mut sink) {
            Ok(()) => sink.finish(),
            Err(never) => match never {},
        }
    }
}

struct DigestSink(Blake2b<U32>);

impl DigestSink {
    fn finish(self) -> ObjectDigest {
        let hash = self.0.finalize();
        let mut digest = [0_u8; OBJECT_DIGEST_LEN];
        digest.copy_from_slice(&hash);
        ObjectDigest::from_array(digest)
    }
}

impl CanonicalSink for DigestSink {
    type Error = Infallible;

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.0.update(bytes);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalFormatError {
    LengthOverflow {
        field: &'static str,
    },
    LengthOutOfRange {
        field: &'static str,
        value: u64,
    },
    Truncated {
        offset: usize,
        needed: usize,
        available: usize,
    },
    InvalidDomain {
        expected: &'static [u8; OBJECT_DOMAIN_LEN],
    },
    UnknownObjectDomain,
    UnsupportedVersion {
        domain: &'static str,
        version: u64,
    },
    SemanticProfileMismatch {
        expected: [u8; 16],
        actual: [u8; 16],
    },
    TrailingBytes {
        offset: usize,
        trailing: usize,
    },
    InvalidDigestLength {
        actual: usize,
    },
    ObjectLengthTooSmall(u64),
    ObjectLengthMismatch {
        expected: u64,
        actual: u64,
    },
    AllocationFailure {
        requested: u64,
    },
    InvalidEndpointTag(u8),
    ZeroMultiplicity,
    InvalidBranchTag(u8),
    NonCanonicalBranchClass {
        tag: u8,
        child_count: u64,
    },
    EmptyNode,
    CompressibleUnaryNode,
    ChildEdgesNotStrict {
        previous: u8,
        current: u8,
    },
    ChildBitmapCount {
        declared: u64,
        present: u64,
    },
    InvalidBlobDomain([u8; OBJECT_DOMAIN_LEN]),
    InvalidSemanticNamespace(u64),
    SemanticRootCount(u64),
    SemanticRootOrder {
        expected: u64,
        actual: u64,
    },
    InvalidRootPresence(u8),
    InvalidAcceleratorKind(u64),
    AcceleratorKindsNotStrict {
        previous: u64,
        current: u64,
    },
    RetentionStateBytes {
        generation: u64,
        minimum: u64,
        actual: u64,
    },
    RetentionContainsCurrent,
    RetainedStatesNotStrict,
    MissingObject(ObjectDigest),
    DigestMismatch {
        expected: ObjectDigest,
        actual: ObjectDigest,
    },
    ContentCollision(ObjectDigest),
    ObjectStorePoisoned,
    UnexpectedObjectKind {
        digest: ObjectDigest,
        expected: &'static str,
        actual: &'static str,
    },
    EndpointPolicy {
        namespace: &'static str,
        endpoint: &'static str,
    },
    PayloadKind {
        namespace: &'static str,
        expected: &'static str,
        actual: &'static str,
    },
    ObjectCycle(ObjectDigest),
    UnexpandedClosureCrossing(ObjectDigest),
    CounterOverflow(&'static str),
    SemanticKey {
        namespace: &'static str,
        detail: String,
    },
    SemanticPayload {
        namespace: &'static str,
        detail: String,
    },
    SemanticCoherence {
        detail: String,
    },
    ResourceLimit {
        resource: &'static str,
        limit: u64,
        attempted: u64,
    },
}

impl Display for CanonicalFormatError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LengthOverflow { field } => write!(formatter, "{field} length overflows u64"),
            Self::LengthOutOfRange { field, value } => {
                write!(formatter, "{field} length {value} does not fit this address space")
            }
            Self::Truncated {
                offset,
                needed,
                available,
            } => write!(
                formatter,
                "canonical object is truncated at {offset}: need {needed} bytes, have {available}"
            ),
            Self::InvalidDomain { expected } => {
                write!(formatter, "invalid canonical object domain; expected {:?}", expected)
            }
            Self::UnknownObjectDomain => formatter.write_str("unknown canonical object domain"),
            Self::UnsupportedVersion { domain, version } => {
                write!(formatter, "unsupported {domain} version {version}")
            }
            Self::SemanticProfileMismatch { expected, actual } => write!(
                formatter,
                "semantic profile mismatch: expected {expected:02x?}, got {actual:02x?}"
            ),
            Self::TrailingBytes { offset, trailing } => {
                write!(formatter, "{trailing} trailing canonical bytes after offset {offset}")
            }
            Self::InvalidDigestLength { actual } => {
                write!(formatter, "object digest has {actual} bytes, expected {OBJECT_DIGEST_LEN}")
            }
            Self::ObjectLengthTooSmall(length) => {
                write!(formatter, "canonical object length {length} is smaller than its envelope")
            }
            Self::ObjectLengthMismatch { expected, actual } => write!(
                formatter,
                "canonical object length mismatch: expected {expected}, got {actual}"
            ),
            Self::AllocationFailure { requested } => {
                write!(formatter, "unable to allocate {requested} canonical bytes")
            }
            Self::InvalidEndpointTag(tag) => write!(formatter, "invalid endpoint tag {tag}"),
            Self::ZeroMultiplicity => formatter.write_str("multiplicity endpoint must be nonzero"),
            Self::InvalidBranchTag(tag) => write!(formatter, "invalid branch tag {tag}"),
            Self::NonCanonicalBranchClass { tag, child_count } => {
                write!(formatter, "branch tag {tag} is noncanonical for {child_count} children")
            }
            Self::EmptyNode => formatter.write_str("an allocated DAG node cannot be empty"),
            Self::CompressibleUnaryNode => formatter.write_str(
                "an endpoint-free one-child node must be merged into its compressed prefix",
            ),
            Self::ChildEdgesNotStrict { previous, current } => write!(
                formatter,
                "child edges are not strictly increasing: {previous} then {current}"
            ),
            Self::ChildBitmapCount { declared, present } => {
                write!(formatter, "child bitmap contains {present} edges but declares {declared}")
            }
            Self::InvalidBlobDomain(domain) => write!(
                formatter,
                "unsupported canonical blob payload domain {}",
                hex::encode(domain)
            ),
            Self::InvalidSemanticNamespace(kind) => {
                write!(formatter, "invalid semantic namespace {kind}")
            }
            Self::SemanticRootCount(count) => {
                write!(formatter, "semantic manifest has {count} roots, expected 3")
            }
            Self::SemanticRootOrder { expected, actual } => write!(
                formatter,
                "semantic namespace tag {actual} is out of order; expected {expected}"
            ),
            Self::InvalidRootPresence(tag) => write!(formatter, "invalid root presence tag {tag}"),
            Self::InvalidAcceleratorKind(kind) => {
                write!(formatter, "invalid accelerator artifact kind {kind}")
            }
            Self::AcceleratorKindsNotStrict { previous, current } => write!(
                formatter,
                "accelerator artifact kinds are not strictly increasing: {previous} then {current}"
            ),
            Self::RetentionStateBytes {
                generation,
                minimum,
                actual,
            } => write!(
                formatter,
                "retained state generation {generation} declares {actual} canonical bytes, below \
                 its manifest length {minimum}"
            ),
            Self::RetentionContainsCurrent => {
                formatter.write_str("retention history repeats the current state")
            }
            Self::RetainedStatesNotStrict => formatter
                .write_str("retained historical states are not in strict canonical identity order"),
            Self::MissingObject(digest) => write!(formatter, "missing canonical object {digest}"),
            Self::DigestMismatch { expected, actual } => {
                write!(formatter, "object digest mismatch: expected {expected}, got {actual}")
            }
            Self::ContentCollision(digest) => {
                write!(formatter, "distinct canonical bytes share object digest {digest}")
            }
            Self::ObjectStorePoisoned => {
                formatter.write_str("canonical object store lock is poisoned")
            }
            Self::UnexpectedObjectKind {
                digest,
                expected,
                actual,
            } => write!(formatter, "object {digest} has kind {actual}, expected {expected}"),
            Self::EndpointPolicy {
                namespace,
                endpoint,
            } => write!(
                formatter,
                "endpoint {endpoint} is invalid in semantic namespace {namespace}"
            ),
            Self::PayloadKind {
                namespace,
                expected,
                actual,
            } => write!(
                formatter,
                "payload kind {actual} is invalid in {namespace}; expected {expected}"
            ),
            Self::ObjectCycle(digest) => {
                write!(formatter, "canonical object graph contains a cycle at {digest}")
            }
            Self::UnexpandedClosureCrossing(digest) => write!(
                formatter,
                "closure reference count of {digest} crossed from zero at a memoized entry"
            ),
            Self::CounterOverflow(counter) => write!(formatter, "{counter} counter overflow"),
            Self::SemanticKey { namespace, detail } => {
                write!(formatter, "invalid {namespace} key: {detail}")
            }
            Self::SemanticPayload { namespace, detail } => {
                write!(formatter, "invalid {namespace} payload: {detail}")
            }
            Self::SemanticCoherence { detail } => {
                write!(formatter, "incoherent RSpace semantic roots: {detail}")
            }
            Self::ResourceLimit {
                resource,
                limit,
                attempted,
            } => write!(
                formatter,
                "persistent-state {resource} budget exceeded: limit {limit}, attempted {attempted}"
            ),
        }
    }
}

impl std::error::Error for CanonicalFormatError {}

pub(crate) fn checked_len(value: usize, field: &'static str) -> Result<u64, CanonicalFormatError> {
    u64::try_from(value).map_err(|_| CanonicalFormatError::LengthOverflow { field })
}

pub(crate) fn checked_sum(
    left: u64,
    right: u64,
    field: &'static str,
) -> Result<u64, CanonicalFormatError> {
    left.checked_add(right)
        .ok_or(CanonicalFormatError::LengthOverflow { field })
}

pub(crate) fn write_u64<S: CanonicalSink>(sink: &mut S, value: u64) -> Result<(), S::Error> {
    sink.write_all(&value.to_le_bytes())
}

pub(crate) struct ReadCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ReadCursor<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self { Self { bytes, offset: 0 } }

    pub(crate) const fn offset(&self) -> usize { self.offset }

    pub(crate) fn remaining(&self) -> usize { self.bytes.len() - self.offset }

    pub(crate) fn read_exact(&mut self, len: usize) -> Result<&'a [u8], CanonicalFormatError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(CanonicalFormatError::LengthOutOfRange {
                field: "cursor extent",
                value: u64::MAX,
            })?;
        if end > self.bytes.len() {
            return Err(CanonicalFormatError::Truncated {
                offset: self.offset,
                needed: len,
                available: self.remaining(),
            });
        }
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, CanonicalFormatError> {
        Ok(self.read_exact(1)?[0])
    }

    pub(crate) fn read_u64(&mut self) -> Result<u64, CanonicalFormatError> {
        let bytes: [u8; 8] = self
            .read_exact(8)?
            .try_into()
            .expect("read_exact returned eight bytes");
        Ok(u64::from_le_bytes(bytes))
    }

    pub(crate) fn read_len(&mut self, field: &'static str) -> Result<usize, CanonicalFormatError> {
        let value = self.read_u64()?;
        usize::try_from(value).map_err(|_| CanonicalFormatError::LengthOutOfRange { field, value })
    }

    pub(crate) fn read_digest(&mut self) -> Result<ObjectDigest, CanonicalFormatError> {
        ObjectDigest::from_slice(self.read_exact(OBJECT_DIGEST_LEN)?)
    }

    pub(crate) fn finish(self) -> Result<(), CanonicalFormatError> {
        if self.offset != self.bytes.len() {
            return Err(CanonicalFormatError::TrailingBytes {
                offset: self.offset,
                trailing: self.bytes.len() - self.offset,
            });
        }
        Ok(())
    }
}

pub(crate) fn write_envelope_header<S: CanonicalSink>(
    sink: &mut S,
    domain: &[u8; OBJECT_DOMAIN_LEN],
    body_len: u64,
) -> Result<(), S::Error> {
    sink.write_all(domain)?;
    write_u64(sink, FORMAT_VERSION_V1)?;
    write_u64(sink, body_len)
}

pub(crate) fn parse_envelope<'a>(
    bytes: &'a [u8],
    domain: &'static [u8; OBJECT_DOMAIN_LEN],
    name: &'static str,
) -> Result<&'a [u8], CanonicalFormatError> {
    let mut cursor = ReadCursor::new(bytes);
    if cursor.read_exact(OBJECT_DOMAIN_LEN)? != domain {
        return Err(CanonicalFormatError::InvalidDomain { expected: domain });
    }
    let version = cursor.read_u64()?;
    if version != FORMAT_VERSION_V1 {
        return Err(CanonicalFormatError::UnsupportedVersion {
            domain: name,
            version,
        });
    }
    let body_len = cursor.read_len("object body")?;
    let body = cursor.read_exact(body_len)?;
    cursor.finish()?;
    Ok(body)
}
