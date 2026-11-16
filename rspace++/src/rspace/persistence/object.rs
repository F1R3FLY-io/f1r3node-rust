use std::num::NonZeroU64;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};

use super::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, OBJECT_DIGEST_LEN, OBJECT_DOMAIN_LEN,
    OBJECT_ENVELOPE_HEADER_LEN, ObjectDigest, ObjectRefV1, ReadCursor, checked_len, checked_sum,
    parse_envelope, write_envelope_header, write_u64,
};
use super::manifest::{
    ACCELERATOR_MANIFEST_DOMAIN, AcceleratorManifestView, SEMANTIC_MANIFEST_DOMAIN,
    SemanticStateManifestView,
};
use super::retention::v2_format::{
    RETENTION_ANCHOR_DOMAIN, RETENTION_RADIX_DOMAIN, RetentionAnchorV2, RetentionFormat,
    RetentionReader, empty_retention_radix_ref,
};
use super::retention::{RETENTION_MANIFEST_DOMAIN, RetentionManifestView};

pub(crate) const DAG_NODE_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.Node\0\0";
pub(crate) const DAG_BLOB_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.Blob\0\0";
const CHILD_BITMAP_LEN: usize = 32;
const NODE48_SLOT_TABLE_LEN: usize = 256;
const ABSENT_NODE48_SLOT: u8 = 0xff;
const OBJECT_REF_LEN: usize = 32 + 8;
const SMALL_CHILD_LEN: usize = 1 + OBJECT_REF_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointValue {
    Absent,
    /// An exact logical key with no typed collection value.
    ValueFree,
    /// Membership in a `PathMap<()>` namespace.
    Unit,
    /// Membership count in a `PathMap<Multiplicity>` namespace.
    Multiplicity(NonZeroU64),
    /// A reference to canonical immutable content.
    Content(ObjectRefV1),
}

impl EndpointValue {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::ValueFree => "value-free",
            Self::Unit => "unit",
            Self::Multiplicity(_) => "multiplicity",
            Self::Content(_) => "content",
        }
    }

    const fn canonical_len(self) -> u64 {
        match self {
            Self::Absent | Self::ValueFree | Self::Unit => 1,
            Self::Multiplicity(_) => 1 + 8,
            Self::Content(_) => 1 + OBJECT_REF_LEN as u64,
        }
    }

    fn write<S: CanonicalSink>(self, sink: &mut S) -> Result<(), S::Error> {
        let tag = match self {
            Self::Absent => 0,
            Self::ValueFree => 1,
            Self::Unit => 2,
            Self::Multiplicity(_) => 3,
            Self::Content(_) => 4,
        };
        sink.write_all(&[tag])?;
        match self {
            Self::Multiplicity(value) => write_u64(sink, value.get()),
            Self::Content(reference) => reference.write(sink),
            Self::Absent | Self::ValueFree | Self::Unit => Ok(()),
        }
    }

    fn parse(cursor: &mut ReadCursor<'_>) -> Result<Self, CanonicalFormatError> {
        let endpoint = match cursor.read_u8()? {
            0 => Self::Absent,
            1 => Self::ValueFree,
            2 => Self::Unit,
            3 => Self::Multiplicity(
                NonZeroU64::new(cursor.read_u64()?)
                    .ok_or(CanonicalFormatError::ZeroMultiplicity)?,
            ),
            4 => Self::Content(ObjectRefV1::parse(cursor)?),
            tag => return Err(CanonicalFormatError::InvalidEndpointTag(tag)),
        };
        Ok(endpoint)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BranchKind {
    Leaf = 0,
    Node4 = 1,
    Node16 = 2,
    Node48 = 3,
    Node256 = 4,
}

impl BranchKind {
    pub const fn for_child_count(child_count: usize) -> Self {
        match child_count {
            0 => Self::Leaf,
            1..=4 => Self::Node4,
            5..=16 => Self::Node16,
            17..=48 => Self::Node48,
            _ => Self::Node256,
        }
    }

    fn parse(tag: u8) -> Result<Self, CanonicalFormatError> {
        match tag {
            0 => Ok(Self::Leaf),
            1 => Ok(Self::Node4),
            2 => Ok(Self::Node16),
            3 => Ok(Self::Node48),
            4 => Ok(Self::Node256),
            value => Err(CanonicalFormatError::InvalidBranchTag(value)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DagChild {
    pub edge: u8,
    pub target: ObjectRefV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalDagNode {
    prefix: Box<[u8]>,
    endpoint: EndpointValue,
    children: Box<[DagChild]>,
    body_len: u64,
    canonical_len: u64,
}

impl CanonicalDagNode {
    pub fn new(
        prefix: impl Into<Box<[u8]>>,
        endpoint: EndpointValue,
        children: impl Into<Box<[DagChild]>>,
    ) -> Result<Self, CanonicalFormatError> {
        let prefix = prefix.into();
        let children = children.into();
        validate_node_shape(&prefix, endpoint, &children)?;
        let (body_len, canonical_len) =
            Self::canonical_lengths_for_shape(prefix.len(), endpoint, children.len())?;

        Ok(Self {
            prefix,
            endpoint,
            children,
            body_len,
            canonical_len,
        })
    }

    pub fn empty() -> Self {
        Self::new(Box::<[u8]>::default(), EndpointValue::Absent, Box::<[DagChild]>::default())
            .expect("the canonical empty node is valid")
    }

    pub fn prefix(&self) -> &[u8] { &self.prefix }

    pub const fn endpoint(&self) -> EndpointValue { self.endpoint }

    pub fn children(&self) -> &[DagChild] { &self.children }

    pub fn branch_kind(&self) -> BranchKind { BranchKind::for_child_count(self.children.len()) }

    /// Compute the exact canonical length before allocating a node's owned
    /// prefix/child representation. This is used by hostile-input resource
    /// preflight in the differential DAG editor.
    pub(crate) fn canonical_len_for_shape(
        prefix_len: usize,
        endpoint: EndpointValue,
        child_count: usize,
    ) -> Result<u64, CanonicalFormatError> {
        Self::canonical_lengths_for_shape(prefix_len, endpoint, child_count)
            .map(|(_, canonical_len)| canonical_len)
    }

    fn canonical_lengths_for_shape(
        prefix_len: usize,
        endpoint: EndpointValue,
        child_count: usize,
    ) -> Result<(u64, u64), CanonicalFormatError> {
        if child_count > 256 {
            return Err(CanonicalFormatError::NonCanonicalBranchClass {
                tag: BranchKind::Node256 as u8,
                child_count: checked_len(child_count, "node child count")?,
            });
        }
        let prefix_len = checked_len(prefix_len, "node prefix")?;
        let child_count_u64 = checked_len(child_count, "node child count")?;
        let child_refs = child_count_u64.checked_mul(OBJECT_REF_LEN as u64).ok_or(
            CanonicalFormatError::LengthOverflow {
                field: "node child references",
            },
        )?;
        let child_bytes = match BranchKind::for_child_count(child_count) {
            BranchKind::Leaf => 0,
            BranchKind::Node4 | BranchKind::Node16 => child_count_u64
                .checked_mul(SMALL_CHILD_LEN as u64)
                .ok_or(CanonicalFormatError::LengthOverflow {
                    field: "node child pairs",
                })?,
            BranchKind::Node48 => {
                checked_sum(NODE48_SLOT_TABLE_LEN as u64, child_refs, "node48 children")?
            }
            BranchKind::Node256 => {
                checked_sum(CHILD_BITMAP_LEN as u64, child_refs, "node256 children")?
            }
        };
        let mut body_len = 8_u64;
        body_len = checked_sum(body_len, prefix_len, "DAG node body")?;
        body_len = checked_sum(body_len, endpoint.canonical_len(), "DAG node body")?;
        body_len = checked_sum(body_len, 1 + 8, "DAG node body")?;
        body_len = checked_sum(body_len, child_bytes, "DAG node body")?;
        let canonical_len = checked_sum(OBJECT_ENVELOPE_HEADER_LEN, body_len, "DAG node envelope")?;
        Ok((body_len, canonical_len))
    }

    pub fn object_ref(&self) -> ObjectRefV1 {
        ObjectRefV1::new(self.canonical_digest(), self.canonical_len)
            .expect("validated node length includes the object envelope")
    }

    /// This node shape as a retention radix node: the same body under the
    /// retention radix domain, so the two images never share a digest.
    pub(crate) const fn as_retention_radix(&self) -> RetentionRadixNodeV2<'_> {
        RetentionRadixNodeV2(self)
    }

    fn write_body<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_u64(sink, self.prefix.len() as u64)?;
        sink.write_all(&self.prefix)?;
        self.endpoint.write(sink)?;

        let kind = self.branch_kind();
        sink.write_all(&[kind as u8])?;
        write_u64(sink, self.children.len() as u64)?;
        match kind {
            BranchKind::Leaf => {}
            BranchKind::Node4 | BranchKind::Node16 => {
                for child in &self.children {
                    sink.write_all(&[child.edge])?;
                    child.target.write(sink)?;
                }
            }
            BranchKind::Node48 => {
                let mut slots = [ABSENT_NODE48_SLOT; NODE48_SLOT_TABLE_LEN];
                for (slot, child) in self.children.iter().enumerate() {
                    slots[usize::from(child.edge)] = slot as u8;
                }
                sink.write_all(&slots)?;
                for child in &self.children {
                    child.target.write(sink)?;
                }
            }
            BranchKind::Node256 => {
                let mut bitmap = [0_u8; CHILD_BITMAP_LEN];
                for child in &self.children {
                    bitmap[usize::from(child.edge) / 8] |= 1 << (child.edge % 8);
                }
                sink.write_all(&bitmap)?;
                for child in &self.children {
                    child.target.write(sink)?;
                }
            }
        }
        Ok(())
    }
}

impl CanonicalEncode for CanonicalDagNode {
    fn canonical_len(&self) -> u64 { self.canonical_len }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_envelope_header(sink, DAG_NODE_DOMAIN, self.body_len)?;
        self.write_body(sink)
    }
}

/// A canonical node shape encoded as a retention radix node. Only the
/// 16-byte domain differs from the semantic DAG node encoding.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RetentionRadixNodeV2<'a>(&'a CanonicalDagNode);

impl RetentionRadixNodeV2<'_> {
    pub(crate) fn object_ref(self) -> ObjectRefV1 {
        ObjectRefV1::new(self.canonical_digest(), self.0.canonical_len)
            .expect("validated node length includes the object envelope")
    }
}

impl CanonicalEncode for RetentionRadixNodeV2<'_> {
    fn canonical_len(&self) -> u64 { self.0.canonical_len }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_envelope_header(sink, RETENTION_RADIX_DOMAIN, self.0.body_len)?;
        self.0.write_body(sink)
    }
}

fn validate_node_shape(
    prefix: &[u8],
    endpoint: EndpointValue,
    children: &[DagChild],
) -> Result<(), CanonicalFormatError> {
    if matches!(endpoint, EndpointValue::Absent) && children.is_empty() && !prefix.is_empty() {
        return Err(CanonicalFormatError::EmptyNode);
    }
    if matches!(endpoint, EndpointValue::Absent) && children.len() == 1 {
        return Err(CanonicalFormatError::CompressibleUnaryNode);
    }
    if children.len() > 256 {
        return Err(CanonicalFormatError::NonCanonicalBranchClass {
            tag: BranchKind::Node256 as u8,
            child_count: children.len() as u64,
        });
    }
    for pair in children.windows(2) {
        if pair[0].edge >= pair[1].edge {
            return Err(CanonicalFormatError::ChildEdgesNotStrict {
                previous: pair[0].edge,
                current: pair[1].edge,
            });
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum ChildEncoding<'a> {
    Pairs(&'a [u8]),
    Node48 {
        slots: &'a [u8],
        references: &'a [u8],
    },
    Node256 {
        bitmap: &'a [u8],
        references: &'a [u8],
    },
}

#[derive(Clone, Copy, Debug)]
pub struct DagChildrenView<'a> {
    kind: BranchKind,
    count: usize,
    encoding: ChildEncoding<'a>,
}

impl<'a> DagChildrenView<'a> {
    pub const fn kind(self) -> BranchKind { self.kind }

    pub const fn len(self) -> usize { self.count }

    pub const fn is_empty(self) -> bool { self.count == 0 }

    /// Resolve one byte edge without projecting the branch into a collection.
    /// The adaptive encodings retain their intended lookup behavior: a tiny
    /// bounded binary search for B4/B16 and direct table/bitmap addressing for
    /// B48/B256.
    pub fn get(self, edge: u8) -> Option<ObjectRefV1> {
        match self.encoding {
            ChildEncoding::Pairs(bytes) => {
                let mut low = 0usize;
                let mut high = self.count;
                while low < high {
                    let middle = low + (high - low) / 2;
                    let candidate = bytes[middle * SMALL_CHILD_LEN];
                    match candidate.cmp(&edge) {
                        std::cmp::Ordering::Less => low = middle + 1,
                        std::cmp::Ordering::Greater => high = middle,
                        std::cmp::Ordering::Equal => {
                            let offset = middle * SMALL_CHILD_LEN + 1;
                            let mut cursor =
                                ReadCursor::new(&bytes[offset..offset + OBJECT_REF_LEN]);
                            return Some(
                                ObjectRefV1::parse(&mut cursor).expect("validated child reference"),
                            );
                        }
                    }
                }
                None
            }
            ChildEncoding::Node48 { slots, references } => {
                let slot = slots[usize::from(edge)];
                (slot != ABSENT_NODE48_SLOT)
                    .then(|| parse_reference_at(references, usize::from(slot)))
            }
            ChildEncoding::Node256 { bitmap, references } => {
                let byte_index = usize::from(edge) / 8;
                let bit_index = edge % 8;
                if bitmap[byte_index] & (1 << bit_index) == 0 {
                    return None;
                }
                let preceding_bytes = bitmap[..byte_index]
                    .iter()
                    .map(|byte| byte.count_ones() as usize)
                    .sum::<usize>();
                let preceding_bits =
                    (bitmap[byte_index] & ((1u8 << bit_index) - 1)).count_ones() as usize;
                Some(parse_reference_at(references, preceding_bytes + preceding_bits))
            }
        }
    }

    pub fn iter(self) -> DagChildrenIter<'a> {
        DagChildrenIter {
            view: self,
            index: 0,
            next_edge: 0,
        }
    }
}

pub struct DagChildrenIter<'a> {
    view: DagChildrenView<'a>,
    index: usize,
    next_edge: u16,
}

impl Iterator for DagChildrenIter<'_> {
    type Item = DagChild;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.view.count {
            return None;
        }
        let child = match self.view.encoding {
            ChildEncoding::Pairs(bytes) => {
                let offset = self.index * SMALL_CHILD_LEN;
                let edge = bytes[offset];
                let mut cursor = ReadCursor::new(&bytes[offset + 1..offset + SMALL_CHILD_LEN]);
                let target = ObjectRefV1::parse(&mut cursor).expect("validated child reference");
                DagChild { edge, target }
            }
            ChildEncoding::Node48 { slots, references } => {
                let edge = loop {
                    let edge = self.next_edge;
                    self.next_edge += 1;
                    let edge_u8 = u8::try_from(edge).expect("validated node48 table");
                    if slots[usize::from(edge_u8)] != ABSENT_NODE48_SLOT {
                        break edge_u8;
                    }
                };
                DagChild {
                    edge,
                    target: parse_reference_at(references, self.index),
                }
            }
            ChildEncoding::Node256 { bitmap, references } => {
                let edge = loop {
                    let edge = self.next_edge;
                    self.next_edge += 1;
                    let edge_u8 = u8::try_from(edge).expect("validated node256 bitmap");
                    if bitmap[usize::from(edge_u8) / 8] & (1 << (edge_u8 % 8)) != 0 {
                        break edge_u8;
                    }
                };
                DagChild {
                    edge,
                    target: parse_reference_at(references, self.index),
                }
            }
        };
        self.index += 1;
        Some(child)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.view.count - self.index;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for DagChildrenIter<'_> {}

fn parse_reference_at(bytes: &[u8], index: usize) -> ObjectRefV1 {
    let offset = index * OBJECT_REF_LEN;
    let mut cursor = ReadCursor::new(&bytes[offset..offset + OBJECT_REF_LEN]);
    ObjectRefV1::parse(&mut cursor).expect("validated child reference")
}

#[derive(Clone, Copy, Debug)]
pub struct CanonicalDagNodeView<'a> {
    bytes: &'a [u8],
    prefix: &'a [u8],
    endpoint: EndpointValue,
    children: DagChildrenView<'a>,
}

impl<'a> CanonicalDagNodeView<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, CanonicalFormatError> {
        Self::parse_in_domain(bytes, DAG_NODE_DOMAIN, "RSpace DAG node")
    }

    pub(crate) fn parse_in_domain(
        bytes: &'a [u8],
        domain: &'static [u8; OBJECT_DOMAIN_LEN],
        name: &'static str,
    ) -> Result<Self, CanonicalFormatError> {
        #[cfg(test)]
        VALIDATING_NODE_PARSES.set(VALIDATING_NODE_PARSES.get().saturating_add(1));
        let body = parse_envelope(bytes, domain, name)?;
        let mut cursor = ReadCursor::new(body);
        let prefix_len = cursor.read_len("node prefix")?;
        let prefix = cursor.read_exact(prefix_len)?;
        let endpoint = EndpointValue::parse(&mut cursor)?;
        let branch_tag = cursor.read_u8()?;
        let kind = BranchKind::parse(branch_tag)?;
        let child_count_u64 = cursor.read_u64()?;
        let child_count = usize::try_from(child_count_u64).map_err(|_| {
            CanonicalFormatError::LengthOutOfRange {
                field: "node child count",
                value: child_count_u64,
            }
        })?;
        if child_count > 256 || kind != BranchKind::for_child_count(child_count) {
            return Err(CanonicalFormatError::NonCanonicalBranchClass {
                tag: branch_tag,
                child_count: child_count_u64,
            });
        }

        let reference_len = child_count.checked_mul(OBJECT_REF_LEN).ok_or(
            CanonicalFormatError::LengthOverflow {
                field: "node child references",
            },
        )?;
        let encoding = match kind {
            BranchKind::Leaf => ChildEncoding::Pairs(cursor.read_exact(0)?),
            BranchKind::Node4 | BranchKind::Node16 => {
                let pair_len = child_count.checked_mul(SMALL_CHILD_LEN).ok_or(
                    CanonicalFormatError::LengthOverflow {
                        field: "node child pairs",
                    },
                )?;
                let pairs = cursor.read_exact(pair_len)?;
                let mut previous = None;
                for pair in pairs.chunks_exact(SMALL_CHILD_LEN) {
                    let edge = pair[0];
                    if let Some(previous) = previous {
                        if previous >= edge {
                            return Err(CanonicalFormatError::ChildEdgesNotStrict {
                                previous,
                                current: edge,
                            });
                        }
                    }
                    let mut reference_cursor = ReadCursor::new(&pair[1..]);
                    let _ = ObjectRefV1::parse(&mut reference_cursor)?;
                    previous = Some(edge);
                }
                ChildEncoding::Pairs(pairs)
            }
            BranchKind::Node48 => {
                let slots = cursor.read_exact(NODE48_SLOT_TABLE_LEN)?;
                let mut present = 0_usize;
                for slot in slots {
                    if *slot != ABSENT_NODE48_SLOT {
                        if usize::from(*slot) != present {
                            return Err(CanonicalFormatError::ChildBitmapCount {
                                declared: child_count_u64,
                                present: present as u64,
                            });
                        }
                        present += 1;
                    }
                }
                if present != child_count {
                    return Err(CanonicalFormatError::ChildBitmapCount {
                        declared: child_count_u64,
                        present: present as u64,
                    });
                }
                let references = cursor.read_exact(reference_len)?;
                validate_references(references)?;
                ChildEncoding::Node48 { slots, references }
            }
            BranchKind::Node256 => {
                let bitmap = cursor.read_exact(CHILD_BITMAP_LEN)?;
                let present = bitmap.iter().map(|byte| byte.count_ones() as u64).sum();
                if present != child_count_u64 {
                    return Err(CanonicalFormatError::ChildBitmapCount {
                        declared: child_count_u64,
                        present,
                    });
                }
                let references = cursor.read_exact(reference_len)?;
                validate_references(references)?;
                ChildEncoding::Node256 { bitmap, references }
            }
        };
        cursor.finish()?;
        validate_decoded_node_shape(prefix, endpoint, child_count)?;
        Ok(Self {
            bytes,
            prefix,
            endpoint,
            children: DagChildrenView {
                kind,
                count: child_count,
                encoding,
            },
        })
    }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub const fn canonical_bytes(self) -> &'a [u8] { self.bytes }

    pub const fn prefix(self) -> &'a [u8] { self.prefix }

    pub const fn endpoint(self) -> EndpointValue { self.endpoint }

    pub const fn children(self) -> DagChildrenView<'a> { self.children }

    pub fn object_ref(self) -> ObjectRefV1 {
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(self.bytes), self.bytes.len() as u64)
            .expect("parsed object length includes the canonical envelope")
    }
}

fn validate_references(bytes: &[u8]) -> Result<(), CanonicalFormatError> {
    for reference in bytes.chunks_exact(OBJECT_REF_LEN) {
        let mut cursor = ReadCursor::new(reference);
        let _ = ObjectRefV1::parse(&mut cursor)?;
    }
    Ok(())
}

fn validate_decoded_node_shape(
    prefix: &[u8],
    endpoint: EndpointValue,
    child_count: usize,
) -> Result<(), CanonicalFormatError> {
    if matches!(endpoint, EndpointValue::Absent) && child_count == 0 && !prefix.is_empty() {
        return Err(CanonicalFormatError::EmptyNode);
    }
    if matches!(endpoint, EndpointValue::Absent) && child_count == 1 {
        return Err(CanonicalFormatError::CompressibleUnaryNode);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CanonicalBlobKind {
    ContinuationSourceV1,
}

impl CanonicalBlobKind {
    const CONTINUATION_SOURCE_DOMAIN: &'static [u8; OBJECT_DOMAIN_LEN] = b"RSpace.Blob.CSrc";

    pub const fn name(self) -> &'static str {
        match self {
            Self::ContinuationSourceV1 => "continuation-source-v1",
        }
    }

    const fn domain(self) -> &'static [u8; OBJECT_DOMAIN_LEN] {
        match self {
            Self::ContinuationSourceV1 => Self::CONTINUATION_SOURCE_DOMAIN,
        }
    }

    fn parse_domain(domain: &[u8]) -> Result<Self, CanonicalFormatError> {
        if domain == Self::CONTINUATION_SOURCE_DOMAIN {
            Ok(Self::ContinuationSourceV1)
        } else {
            let actual: [u8; OBJECT_DOMAIN_LEN] = domain
                .try_into()
                .expect("the blob payload domain has fixed width");
            Err(CanonicalFormatError::InvalidBlobDomain(actual))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalBlob {
    kind: CanonicalBlobKind,
    bytes: Box<[u8]>,
    body_len: u64,
    canonical_len: u64,
}

impl CanonicalBlob {
    pub fn new(
        kind: CanonicalBlobKind,
        bytes: impl Into<Box<[u8]>>,
    ) -> Result<Self, CanonicalFormatError> {
        let bytes = bytes.into();
        let payload_len = checked_len(bytes.len(), "blob payload")?;
        let body_len =
            checked_sum(OBJECT_DOMAIN_LEN as u64 + 8 + 8, payload_len, "canonical blob body")?;
        let canonical_len =
            checked_sum(OBJECT_ENVELOPE_HEADER_LEN, body_len, "canonical blob envelope")?;
        Ok(Self {
            kind,
            bytes,
            body_len,
            canonical_len,
        })
    }

    pub const fn kind(&self) -> CanonicalBlobKind { self.kind }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub fn payload(&self) -> &[u8] { &self.bytes }

    pub fn object_ref(&self) -> ObjectRefV1 {
        ObjectRefV1::new(self.canonical_digest(), self.canonical_len)
            .expect("validated blob length includes the canonical envelope")
    }
}

impl CanonicalEncode for CanonicalBlob {
    fn canonical_len(&self) -> u64 { self.canonical_len }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_envelope_header(sink, DAG_BLOB_DOMAIN, self.body_len)?;
        sink.write_all(self.kind.domain())?;
        write_u64(sink, 1)?;
        write_u64(sink, self.bytes.len() as u64)?;
        sink.write_all(&self.bytes)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CanonicalBlobView<'a> {
    bytes: &'a [u8],
    kind: CanonicalBlobKind,
    payload: &'a [u8],
}

impl<'a> CanonicalBlobView<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, CanonicalFormatError> {
        let body = parse_envelope(bytes, DAG_BLOB_DOMAIN, "RSpace DAG blob")?;
        let mut cursor = ReadCursor::new(body);
        let kind = CanonicalBlobKind::parse_domain(cursor.read_exact(OBJECT_DOMAIN_LEN)?)?;
        let payload_version = cursor.read_u64()?;
        if payload_version != 1 {
            return Err(CanonicalFormatError::UnsupportedVersion {
                domain: kind.name(),
                version: payload_version,
            });
        }
        let payload_len = cursor.read_len("blob payload")?;
        let payload = cursor.read_exact(payload_len)?;
        cursor.finish()?;
        Ok(Self {
            bytes,
            kind,
            payload,
        })
    }

    pub const fn kind(self) -> CanonicalBlobKind { self.kind }

    pub const fn payload(self) -> &'a [u8] { self.payload }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub const fn canonical_bytes(self) -> &'a [u8] { self.bytes }

    pub fn object_ref(self) -> ObjectRefV1 {
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(self.bytes), self.bytes.len() as u64)
            .expect("parsed object length includes the canonical envelope")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalObjectKind {
    DagNode,
    Blob,
    SemanticManifest,
    AcceleratorManifest,
    RetentionManifest,
    RetentionAnchor,
    RetentionRadixNode,
}

impl CanonicalObjectKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::DagNode => "DAG-node",
            Self::Blob => "blob",
            Self::SemanticManifest => "semantic-manifest",
            Self::AcceleratorManifest => "accelerator-manifest",
            Self::RetentionManifest => "retention-manifest",
            Self::RetentionAnchor => "retention-anchor",
            Self::RetentionRadixNode => "retention-radix-node",
        }
    }

    /// Classify the domain prefix for a reader with the given retention
    /// capability. A V1-only reader is exactly [`Self::from_domain_prefix`];
    /// a dual reader additionally recognizes the V2 metadata domains, so no
    /// reader ever retries an unknown domain under another parser.
    pub(crate) fn from_domain_prefix_for(
        bytes: &[u8],
        reader: RetentionReader,
    ) -> Result<Self, CanonicalFormatError> {
        match (Self::from_domain_prefix(bytes), reader.admits(RetentionFormat::V2)) {
            (Err(CanonicalFormatError::UnknownObjectDomain), true) => {
                match bytes.get(..OBJECT_DOMAIN_LEN) {
                    Some(domain) if domain == RETENTION_ANCHOR_DOMAIN => Ok(Self::RetentionAnchor),
                    Some(domain) if domain == RETENTION_RADIX_DOMAIN => {
                        Ok(Self::RetentionRadixNode)
                    }
                    _ => Err(CanonicalFormatError::UnknownObjectDomain),
                }
            }
            (classified, _) => classified,
        }
    }

    /// Classify only the fixed canonical domain prefix. This is sufficient for
    /// bounded physical recovery to recognize checkpoint-designated objects;
    /// complete canonical and semantic parsing remains mandatory admission.
    pub(crate) fn from_domain_prefix(bytes: &[u8]) -> Result<Self, CanonicalFormatError> {
        let Some(domain) = bytes.get(..OBJECT_DOMAIN_LEN) else {
            return Err(CanonicalFormatError::Truncated {
                offset: 0,
                needed: OBJECT_DOMAIN_LEN,
                available: bytes.len(),
            });
        };
        if domain == DAG_NODE_DOMAIN {
            Ok(Self::DagNode)
        } else if domain == DAG_BLOB_DOMAIN {
            Ok(Self::Blob)
        } else if domain == SEMANTIC_MANIFEST_DOMAIN {
            Ok(Self::SemanticManifest)
        } else if domain == ACCELERATOR_MANIFEST_DOMAIN {
            Ok(Self::AcceleratorManifest)
        } else if domain == RETENTION_MANIFEST_DOMAIN {
            Ok(Self::RetentionManifest)
        } else {
            Err(CanonicalFormatError::UnknownObjectDomain)
        }
    }
}

/// The storage-independent result of bounded canonical-object validation.
///
/// Unlike [`CanonicalObjectView`], this identity does not borrow the encoded
/// object.  It is intended for durable readers that already know the exact
/// object extent and must validate it without retaining a whole-object buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalObjectStreamIdentityV1 {
    kind: CanonicalObjectKind,
    reference: ObjectRefV1,
}

impl CanonicalObjectStreamIdentityV1 {
    pub(crate) const fn kind(self) -> CanonicalObjectKind { self.kind }

    pub(crate) const fn reference(self) -> ObjectRefV1 { self.reference }
}

/// Progress reported by [`CanonicalObjectStreamValidatorV1::feed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalObjectStreamProgressV1 {
    NeedMore,
    Complete(CanonicalObjectStreamIdentityV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamReferencePurpose {
    DagEndpoint,
    DagChild,
    SemanticRoot,
    AcceleratorSemantic,
    AcceleratorArtifact,
    RetentionCurrent,
    RetentionHistorical,
    AnchorRoot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CanonicalObjectStreamStateV1 {
    ObjectDomain,
    EnvelopeVersion(CanonicalObjectKind),
    EnvelopeBodyLen(CanonicalObjectKind),

    DagPrefixLen,
    DagPrefixSkip,
    DagEndpointTag,
    DagEndpointMultiplicity,
    DagBranchTag,
    DagChildCount,
    DagSmallEdge,
    DagNode48Slots,
    DagNode256Bitmap,
    DagFinish,

    BlobPayloadDomain,
    BlobPayloadVersion,
    BlobPayloadLen,
    BlobPayloadSkip,
    BlobFinish,

    SemanticSchemaEpoch,
    SemanticProfileId,
    SemanticRootCount,
    SemanticRootTag,
    SemanticLegacyPresence,
    SemanticLegacyDigest,
    SemanticFinish,

    AcceleratorEpoch,
    AcceleratorCount,
    AcceleratorArtifactKind,
    AcceleratorFinish,

    RetentionCurrentGeneration,
    RetentionCurrentBytes(ObjectRefV1),
    RetentionHistoricalCount,
    RetentionHistoricalGeneration,
    RetentionHistoricalBytes(ObjectRefV1),
    RetentionFinish,

    AnchorHistoricalCount,
    AnchorFinish(ObjectRefV1),

    ReferenceDigest(StreamReferencePurpose),
    ReferenceLen(StreamReferencePurpose, ObjectDigest),
    Complete(CanonicalObjectKind),
}

/// Constant-state canonical-object validator for arbitrary input chunks.
///
/// `total_len` is the complete, addressable object extent (the same extent a
/// contiguous `&[u8]` oracle would receive).  Calls to [`Self::feed`] must form
/// an exact, ordered partition of that extent.  The validator retains only a
/// 40-byte partial-field buffer, fixed counters/identities, and the digest
/// state: prefixes, blob payloads, adaptive-node tables, artifact arrays, and
/// retention histories are consumed without retention.
pub(crate) struct CanonicalObjectStreamValidatorV1 {
    total_len: usize,
    consumed: usize,
    body_len: usize,
    reader: RetentionReader,
    body_kind: CanonicalObjectKind,
    state: CanonicalObjectStreamStateV1,
    field: [u8; OBJECT_REF_LEN],
    field_len: usize,
    digest: Blake2b<U32>,

    skip_remaining: usize,
    remaining: usize,
    index: usize,
    present: usize,
    previous_u8: Option<u8>,
    previous_u64: Option<u64>,
    pending_u64: u64,

    dag_prefix_nonempty: bool,
    dag_endpoint_absent: bool,
    dag_branch_tag: u8,
    dag_branch_kind: BranchKind,
    dag_child_count: usize,

    retention_current: Option<(u64, ObjectRefV1)>,
    retention_previous: Option<(u64, ObjectRefV1)>,
}

impl CanonicalObjectStreamValidatorV1 {
    pub(crate) fn new(total_len: usize) -> Self {
        Self::new_for(total_len, RetentionReader::V1Only)
    }

    pub(crate) fn new_for(total_len: usize, reader: RetentionReader) -> Self {
        Self {
            total_len,
            consumed: 0,
            body_len: 0,
            reader,
            body_kind: CanonicalObjectKind::DagNode,
            state: CanonicalObjectStreamStateV1::ObjectDomain,
            field: [0; OBJECT_REF_LEN],
            field_len: 0,
            digest: Blake2b::<U32>::new(),
            skip_remaining: 0,
            remaining: 0,
            index: 0,
            present: 0,
            previous_u8: None,
            previous_u64: None,
            pending_u64: 0,
            dag_prefix_nonempty: false,
            dag_endpoint_absent: false,
            dag_branch_tag: 0,
            dag_branch_kind: BranchKind::Leaf,
            dag_child_count: 0,
            retention_current: None,
            retention_previous: None,
        }
    }

    pub(crate) fn feed(
        &mut self,
        bytes: &[u8],
    ) -> Result<CanonicalObjectStreamProgressV1, CanonicalFormatError> {
        let attempted = self.consumed.checked_add(bytes.len()).ok_or(
            CanonicalFormatError::LengthOutOfRange {
                field: "streaming canonical object",
                value: u64::MAX,
            },
        )?;
        if attempted > self.total_len {
            return Err(CanonicalFormatError::TrailingBytes {
                offset: self.total_len,
                trailing: attempted - self.total_len,
            });
        }

        let mut offset = 0;
        while offset < bytes.len() {
            self.drive_zero_width_states()?;
            if matches!(self.state, CanonicalObjectStreamStateV1::Complete(_)) {
                return Err(CanonicalFormatError::TrailingBytes {
                    offset: self.consumed,
                    trailing: bytes.len() - offset,
                });
            }

            match self.state {
                CanonicalObjectStreamStateV1::DagPrefixSkip |
                CanonicalObjectStreamStateV1::BlobPayloadSkip => {
                    let take = self.skip_remaining.min(bytes.len() - offset);
                    self.consume(&bytes[offset..offset + take]);
                    self.skip_remaining -= take;
                    offset += take;
                }
                CanonicalObjectStreamStateV1::DagNode48Slots => {
                    let byte = bytes[offset];
                    self.consume(&bytes[offset..offset + 1]);
                    offset += 1;
                    if byte != ABSENT_NODE48_SLOT {
                        if usize::from(byte) != self.present {
                            return Err(CanonicalFormatError::ChildBitmapCount {
                                declared: self.dag_child_count as u64,
                                present: self.present as u64,
                            });
                        }
                        self.present += 1;
                    }
                    self.index += 1;
                    if self.index == NODE48_SLOT_TABLE_LEN {
                        if self.present != self.dag_child_count {
                            return Err(CanonicalFormatError::ChildBitmapCount {
                                declared: self.dag_child_count as u64,
                                present: self.present as u64,
                            });
                        }
                        let reference_len = self
                            .dag_child_count
                            .checked_mul(OBJECT_REF_LEN)
                            .ok_or(CanonicalFormatError::LengthOverflow {
                                field: "node child references",
                            })?;
                        self.require_body_aggregate(reference_len)?;
                        self.remaining = self.dag_child_count;
                        self.state = if self.remaining == 0 {
                            CanonicalObjectStreamStateV1::DagFinish
                        } else {
                            CanonicalObjectStreamStateV1::ReferenceDigest(
                                StreamReferencePurpose::DagChild,
                            )
                        };
                    }
                }
                CanonicalObjectStreamStateV1::DagNode256Bitmap => {
                    let byte = bytes[offset];
                    self.consume(&bytes[offset..offset + 1]);
                    offset += 1;
                    self.present += byte.count_ones() as usize;
                    self.index += 1;
                    if self.index == CHILD_BITMAP_LEN {
                        if self.present != self.dag_child_count {
                            return Err(CanonicalFormatError::ChildBitmapCount {
                                declared: self.dag_child_count as u64,
                                present: self.present as u64,
                            });
                        }
                        let reference_len = self
                            .dag_child_count
                            .checked_mul(OBJECT_REF_LEN)
                            .ok_or(CanonicalFormatError::LengthOverflow {
                                field: "node child references",
                            })?;
                        self.require_body_aggregate(reference_len)?;
                        self.remaining = self.dag_child_count;
                        self.state = if self.remaining == 0 {
                            CanonicalObjectStreamStateV1::DagFinish
                        } else {
                            CanonicalObjectStreamStateV1::ReferenceDigest(
                                StreamReferencePurpose::DagChild,
                            )
                        };
                    }
                }
                state => {
                    let width = Self::field_width(state)
                        .expect("every non-skip streaming state has a fixed field width");
                    if self.field_len == 0 {
                        self.require_field(width)?;
                    }
                    let take = (width - self.field_len).min(bytes.len() - offset);
                    self.field[self.field_len..self.field_len + take]
                        .copy_from_slice(&bytes[offset..offset + take]);
                    self.consume(&bytes[offset..offset + take]);
                    self.field_len += take;
                    offset += take;
                    if self.field_len == width {
                        self.complete_field(state, width)?;
                        self.field_len = 0;
                    }
                }
            }
        }
        self.drive_zero_width_states()?;
        Ok(match self.completed_identity()? {
            Some(identity) => CanonicalObjectStreamProgressV1::Complete(identity),
            None => CanonicalObjectStreamProgressV1::NeedMore,
        })
    }

    pub(crate) fn finish(
        mut self,
    ) -> Result<CanonicalObjectStreamIdentityV1, CanonicalFormatError> {
        self.drive_zero_width_states()?;
        if let Some(identity) = self.completed_identity()? {
            return Ok(identity);
        }
        if self.field_len == 0 {
            if let Some(width) = Self::field_width(self.state) {
                self.require_field(width)?;
            }
        }
        Err(CanonicalFormatError::Truncated {
            offset: self.error_offset(),
            needed: self.total_len.saturating_sub(self.consumed),
            available: 0,
        })
    }

    fn consume(&mut self, bytes: &[u8]) {
        self.digest.update(bytes);
        self.consumed += bytes.len();
    }

    fn body_offset(&self) -> usize {
        self.consumed
            .saturating_sub(OBJECT_ENVELOPE_HEADER_LEN as usize)
    }

    fn body_remaining(&self) -> usize { self.body_len.saturating_sub(self.body_offset()) }

    fn error_offset(&self) -> usize {
        if self.consumed < OBJECT_ENVELOPE_HEADER_LEN as usize {
            self.consumed.saturating_sub(self.field_len)
        } else {
            self.body_offset().saturating_sub(self.field_len)
        }
    }

    fn require_field(&self, width: usize) -> Result<(), CanonicalFormatError> {
        let (offset, available) = if self.consumed < OBJECT_ENVELOPE_HEADER_LEN as usize {
            (self.consumed, self.total_len.saturating_sub(self.consumed))
        } else {
            (self.body_offset(), self.body_remaining())
        };
        if available < width {
            return Err(CanonicalFormatError::Truncated {
                offset,
                needed: width,
                available,
            });
        }
        Ok(())
    }

    fn require_body_aggregate(&self, width: usize) -> Result<(), CanonicalFormatError> {
        let offset = self.body_offset();
        if offset.checked_add(width).is_none() {
            return Err(CanonicalFormatError::LengthOutOfRange {
                field: "cursor extent",
                value: u64::MAX,
            });
        }
        let available = self.body_remaining();
        if available < width {
            return Err(CanonicalFormatError::Truncated {
                offset,
                needed: width,
                available,
            });
        }
        Ok(())
    }

    fn require_exact_body_aggregate(&self, width: usize) -> Result<(), CanonicalFormatError> {
        self.require_body_aggregate(width)?;
        let available = self.body_remaining();
        if available > width {
            return Err(CanonicalFormatError::TrailingBytes {
                offset: self.body_offset() + width,
                trailing: available - width,
            });
        }
        Ok(())
    }

    fn field_width(state: CanonicalObjectStreamStateV1) -> Option<usize> {
        use CanonicalObjectStreamStateV1 as State;
        Some(match state {
            State::ObjectDomain | State::BlobPayloadDomain | State::SemanticProfileId => {
                OBJECT_DOMAIN_LEN
            }
            State::ReferenceDigest(_) | State::SemanticLegacyDigest => OBJECT_DIGEST_LEN,
            State::EnvelopeVersion(_) |
            State::EnvelopeBodyLen(_) |
            State::DagPrefixLen |
            State::DagEndpointMultiplicity |
            State::DagChildCount |
            State::BlobPayloadVersion |
            State::BlobPayloadLen |
            State::SemanticSchemaEpoch |
            State::SemanticRootCount |
            State::SemanticRootTag |
            State::AcceleratorEpoch |
            State::AcceleratorCount |
            State::AcceleratorArtifactKind |
            State::RetentionCurrentGeneration |
            State::RetentionCurrentBytes(_) |
            State::RetentionHistoricalCount |
            State::RetentionHistoricalGeneration |
            State::RetentionHistoricalBytes(_) |
            State::AnchorHistoricalCount |
            State::ReferenceLen(_, _) => 8,
            State::DagEndpointTag |
            State::DagBranchTag |
            State::DagSmallEdge |
            State::SemanticLegacyPresence => 1,
            State::DagPrefixSkip |
            State::DagNode48Slots |
            State::DagNode256Bitmap |
            State::DagFinish |
            State::BlobPayloadSkip |
            State::BlobFinish |
            State::SemanticFinish |
            State::AcceleratorFinish |
            State::RetentionFinish |
            State::AnchorFinish(_) |
            State::Complete(_) => return None,
        })
    }

    fn complete_field(
        &mut self,
        state: CanonicalObjectStreamStateV1,
        width: usize,
    ) -> Result<(), CanonicalFormatError> {
        use CanonicalObjectStreamStateV1 as State;
        match state {
            State::ObjectDomain => {
                let kind =
                    CanonicalObjectKind::from_domain_prefix_for(&self.field[..width], self.reader)?;
                self.state = State::EnvelopeVersion(kind);
            }
            State::EnvelopeVersion(kind) => {
                let version = self.field_u64();
                if version != super::canonical::FORMAT_VERSION_V1 {
                    return Err(CanonicalFormatError::UnsupportedVersion {
                        domain: Self::object_envelope_name(kind),
                        version,
                    });
                }
                self.state = State::EnvelopeBodyLen(kind);
            }
            State::EnvelopeBodyLen(kind) => {
                let body_len_u64 = self.field_u64();
                let body_len = usize::try_from(body_len_u64).map_err(|_| {
                    CanonicalFormatError::LengthOutOfRange {
                        field: "object body",
                        value: body_len_u64,
                    }
                })?;
                if (OBJECT_ENVELOPE_HEADER_LEN as usize)
                    .checked_add(body_len)
                    .is_none()
                {
                    return Err(CanonicalFormatError::LengthOutOfRange {
                        field: "cursor extent",
                        value: u64::MAX,
                    });
                }
                let available = self.total_len - OBJECT_ENVELOPE_HEADER_LEN as usize;
                if available < body_len {
                    return Err(CanonicalFormatError::Truncated {
                        offset: OBJECT_ENVELOPE_HEADER_LEN as usize,
                        needed: body_len,
                        available,
                    });
                }
                if available > body_len {
                    return Err(CanonicalFormatError::TrailingBytes {
                        offset: OBJECT_ENVELOPE_HEADER_LEN as usize + body_len,
                        trailing: available - body_len,
                    });
                }
                self.body_len = body_len;
                self.body_kind = kind;
                self.state = match kind {
                    CanonicalObjectKind::DagNode | CanonicalObjectKind::RetentionRadixNode => {
                        State::DagPrefixLen
                    }
                    CanonicalObjectKind::Blob => State::BlobPayloadDomain,
                    CanonicalObjectKind::SemanticManifest => State::SemanticSchemaEpoch,
                    CanonicalObjectKind::AcceleratorManifest => {
                        State::ReferenceDigest(StreamReferencePurpose::AcceleratorSemantic)
                    }
                    CanonicalObjectKind::RetentionManifest |
                    CanonicalObjectKind::RetentionAnchor => State::RetentionCurrentGeneration,
                };
            }
            State::DagPrefixLen => {
                let value = self.field_u64();
                self.skip_remaining =
                    usize::try_from(value).map_err(|_| CanonicalFormatError::LengthOutOfRange {
                        field: "node prefix",
                        value,
                    })?;
                self.require_body_aggregate(self.skip_remaining)?;
                self.dag_prefix_nonempty = self.skip_remaining != 0;
                self.state = State::DagPrefixSkip;
            }
            State::DagEndpointTag => match self.field[0] {
                0 => {
                    self.dag_endpoint_absent = true;
                    self.state = State::DagBranchTag;
                }
                1 | 2 => {
                    self.dag_endpoint_absent = false;
                    self.state = State::DagBranchTag;
                }
                3 => {
                    self.dag_endpoint_absent = false;
                    self.state = State::DagEndpointMultiplicity;
                }
                4 => {
                    self.dag_endpoint_absent = false;
                    self.state = State::ReferenceDigest(StreamReferencePurpose::DagEndpoint);
                }
                tag => return Err(CanonicalFormatError::InvalidEndpointTag(tag)),
            },
            State::DagEndpointMultiplicity => {
                if self.field_u64() == 0 {
                    return Err(CanonicalFormatError::ZeroMultiplicity);
                }
                self.state = State::DagBranchTag;
            }
            State::DagBranchTag => {
                self.dag_branch_tag = self.field[0];
                self.dag_branch_kind = BranchKind::parse(self.dag_branch_tag)?;
                self.state = State::DagChildCount;
            }
            State::DagChildCount => {
                let count_u64 = self.field_u64();
                let count = usize::try_from(count_u64).map_err(|_| {
                    CanonicalFormatError::LengthOutOfRange {
                        field: "node child count",
                        value: count_u64,
                    }
                })?;
                if count > 256 || self.dag_branch_kind != BranchKind::for_child_count(count) {
                    return Err(CanonicalFormatError::NonCanonicalBranchClass {
                        tag: self.dag_branch_tag,
                        child_count: count_u64,
                    });
                }
                self.dag_child_count = count;
                self.remaining = count;
                self.previous_u8 = None;
                match self.dag_branch_kind {
                    BranchKind::Leaf => self.state = State::DagFinish,
                    BranchKind::Node4 | BranchKind::Node16 => {
                        let pair_len = count.checked_mul(SMALL_CHILD_LEN).ok_or(
                            CanonicalFormatError::LengthOverflow {
                                field: "node child pairs",
                            },
                        )?;
                        self.require_body_aggregate(pair_len)?;
                        self.state = State::DagSmallEdge;
                    }
                    BranchKind::Node48 => {
                        self.require_body_aggregate(NODE48_SLOT_TABLE_LEN)?;
                        self.index = 0;
                        self.present = 0;
                        self.state = State::DagNode48Slots;
                    }
                    BranchKind::Node256 => {
                        self.require_body_aggregate(CHILD_BITMAP_LEN)?;
                        self.index = 0;
                        self.present = 0;
                        self.state = State::DagNode256Bitmap;
                    }
                }
            }
            State::DagSmallEdge => {
                let edge = self.field[0];
                if let Some(previous) = self.previous_u8 {
                    if previous >= edge {
                        return Err(CanonicalFormatError::ChildEdgesNotStrict {
                            previous,
                            current: edge,
                        });
                    }
                }
                self.previous_u8 = Some(edge);
                self.state = State::ReferenceDigest(StreamReferencePurpose::DagChild);
            }
            State::BlobPayloadDomain => {
                let _ = CanonicalBlobKind::parse_domain(&self.field[..width])?;
                self.state = State::BlobPayloadVersion;
            }
            State::BlobPayloadVersion => {
                let version = self.field_u64();
                if version != 1 {
                    return Err(CanonicalFormatError::UnsupportedVersion {
                        domain: CanonicalBlobKind::ContinuationSourceV1.name(),
                        version,
                    });
                }
                self.state = State::BlobPayloadLen;
            }
            State::BlobPayloadLen => {
                let value = self.field_u64();
                self.skip_remaining =
                    usize::try_from(value).map_err(|_| CanonicalFormatError::LengthOutOfRange {
                        field: "blob payload",
                        value,
                    })?;
                self.require_body_aggregate(self.skip_remaining)?;
                self.state = State::BlobPayloadSkip;
            }
            State::SemanticSchemaEpoch => {
                let version = self.field_u64();
                if version != super::manifest::SEMANTIC_SCHEMA_EPOCH_V1 {
                    return Err(CanonicalFormatError::UnsupportedVersion {
                        domain: "RSpace semantic schema",
                        version,
                    });
                }
                self.state = State::SemanticProfileId;
            }
            State::SemanticProfileId => self.state = State::SemanticRootCount,
            State::SemanticRootCount => {
                let count = self.field_u64();
                if count != 3 {
                    return Err(CanonicalFormatError::SemanticRootCount(count));
                }
                self.index = 0;
                self.state = State::SemanticRootTag;
            }
            State::SemanticRootTag => {
                let actual = self.field_u64();
                let expected = (self.index + 1) as u64;
                if actual != expected {
                    return Err(CanonicalFormatError::SemanticRootOrder { expected, actual });
                }
                self.state = State::ReferenceDigest(StreamReferencePurpose::SemanticRoot);
            }
            State::SemanticLegacyPresence => match self.field[0] {
                0 => self.state = State::SemanticFinish,
                1 => self.state = State::SemanticLegacyDigest,
                tag => return Err(CanonicalFormatError::InvalidRootPresence(tag)),
            },
            State::SemanticLegacyDigest => self.state = State::SemanticFinish,
            State::AcceleratorEpoch => self.state = State::AcceleratorCount,
            State::AcceleratorCount => {
                let value = self.field_u64();
                let count =
                    usize::try_from(value).map_err(|_| CanonicalFormatError::LengthOutOfRange {
                        field: "accelerator artifact count",
                        value,
                    })?;
                let encoded_len = count.checked_mul(8 + OBJECT_REF_LEN).ok_or(
                    CanonicalFormatError::LengthOverflow {
                        field: "accelerator artifacts",
                    },
                )?;
                self.require_exact_body_aggregate(encoded_len)?;
                self.remaining = count;
                self.previous_u64 = None;
                self.state = if count == 0 {
                    State::AcceleratorFinish
                } else {
                    State::AcceleratorArtifactKind
                };
            }
            State::AcceleratorArtifactKind => {
                let tag = self.field_u64();
                let _ = super::manifest::AcceleratorArtifactKind::parse(tag)?;
                self.pending_u64 = tag;
                self.state = State::ReferenceDigest(StreamReferencePurpose::AcceleratorArtifact);
            }
            State::RetentionCurrentGeneration => {
                self.pending_u64 = self.field_u64();
                self.state = State::ReferenceDigest(StreamReferencePurpose::RetentionCurrent);
            }
            State::RetentionCurrentBytes(reference) => {
                let canonical_bytes = self.field_u64();
                Self::require_retention_bytes(self.pending_u64, reference, canonical_bytes)?;
                self.retention_current = Some((self.pending_u64, reference));
                self.state = match self.body_kind {
                    CanonicalObjectKind::RetentionAnchor => State::AnchorHistoricalCount,
                    _ => State::RetentionHistoricalCount,
                };
            }
            State::AnchorHistoricalCount => {
                self.pending_u64 = self.field_u64();
                self.state = State::ReferenceDigest(StreamReferencePurpose::AnchorRoot);
            }
            State::RetentionHistoricalCount => {
                let value = self.field_u64();
                let count =
                    usize::try_from(value).map_err(|_| CanonicalFormatError::LengthOutOfRange {
                        field: "retained historical states",
                        value,
                    })?;
                let encoded_len =
                    count
                        .checked_mul(56)
                        .ok_or(CanonicalFormatError::LengthOverflow {
                            field: "retained historical states",
                        })?;
                self.require_exact_body_aggregate(encoded_len)?;
                self.remaining = count;
                self.retention_previous = None;
                self.state = if count == 0 {
                    State::RetentionFinish
                } else {
                    State::RetentionHistoricalGeneration
                };
            }
            State::RetentionHistoricalGeneration => {
                self.pending_u64 = self.field_u64();
                self.state = State::ReferenceDigest(StreamReferencePurpose::RetentionHistorical);
            }
            State::RetentionHistoricalBytes(reference) => {
                let canonical_bytes = self.field_u64();
                let current = (self.pending_u64, reference);
                Self::require_retention_bytes(current.0, current.1, canonical_bytes)?;
                if Some(current) == self.retention_current {
                    return Err(CanonicalFormatError::RetentionContainsCurrent);
                }
                if self
                    .retention_previous
                    .is_some_and(|previous| previous >= current)
                {
                    return Err(CanonicalFormatError::RetainedStatesNotStrict);
                }
                self.retention_previous = Some(current);
                self.remaining -= 1;
                self.state = if self.remaining == 0 {
                    State::RetentionFinish
                } else {
                    State::RetentionHistoricalGeneration
                };
            }
            State::ReferenceDigest(purpose) => {
                let digest = ObjectDigest::from_slice(&self.field[..width])?;
                self.state = State::ReferenceLen(purpose, digest);
            }
            State::ReferenceLen(purpose, digest) => {
                let reference = ObjectRefV1::new(digest, self.field_u64())?;
                self.after_reference(purpose, reference)?;
            }
            State::DagPrefixSkip |
            State::DagNode48Slots |
            State::DagNode256Bitmap |
            State::DagFinish |
            State::BlobPayloadSkip |
            State::BlobFinish |
            State::SemanticFinish |
            State::AcceleratorFinish |
            State::RetentionFinish |
            State::AnchorFinish(_) |
            State::Complete(_) => unreachable!("non-field state reached fixed-field completion"),
        }
        Ok(())
    }

    fn after_reference(
        &mut self,
        purpose: StreamReferencePurpose,
        reference: ObjectRefV1,
    ) -> Result<(), CanonicalFormatError> {
        use CanonicalObjectStreamStateV1 as State;
        self.state = match purpose {
            StreamReferencePurpose::DagEndpoint => State::DagBranchTag,
            StreamReferencePurpose::DagChild => {
                self.remaining -= 1;
                if self.remaining == 0 {
                    State::DagFinish
                } else if matches!(self.dag_branch_kind, BranchKind::Node4 | BranchKind::Node16) {
                    State::DagSmallEdge
                } else {
                    State::ReferenceDigest(StreamReferencePurpose::DagChild)
                }
            }
            StreamReferencePurpose::SemanticRoot => {
                self.index += 1;
                if self.index == 3 {
                    State::SemanticLegacyPresence
                } else {
                    State::SemanticRootTag
                }
            }
            StreamReferencePurpose::AcceleratorSemantic => State::AcceleratorEpoch,
            StreamReferencePurpose::AcceleratorArtifact => {
                if let Some(previous) = self.previous_u64 {
                    if previous >= self.pending_u64 {
                        return Err(CanonicalFormatError::AcceleratorKindsNotStrict {
                            previous,
                            current: self.pending_u64,
                        });
                    }
                }
                self.previous_u64 = Some(self.pending_u64);
                self.remaining -= 1;
                if self.remaining == 0 {
                    State::AcceleratorFinish
                } else {
                    State::AcceleratorArtifactKind
                }
            }
            StreamReferencePurpose::RetentionCurrent => State::RetentionCurrentBytes(reference),
            StreamReferencePurpose::RetentionHistorical => {
                State::RetentionHistoricalBytes(reference)
            }
            StreamReferencePurpose::AnchorRoot => State::AnchorFinish(reference),
        };
        Ok(())
    }

    fn drive_zero_width_states(&mut self) -> Result<(), CanonicalFormatError> {
        use CanonicalObjectStreamStateV1 as State;
        loop {
            match self.state {
                State::DagPrefixSkip if self.skip_remaining == 0 => {
                    self.state = State::DagEndpointTag;
                }
                State::BlobPayloadSkip if self.skip_remaining == 0 => {
                    self.state = State::BlobFinish;
                }
                State::DagFinish => {
                    self.require_body_finished()?;
                    if self.dag_endpoint_absent &&
                        self.dag_child_count == 0 &&
                        self.dag_prefix_nonempty
                    {
                        return Err(CanonicalFormatError::EmptyNode);
                    }
                    if self.dag_endpoint_absent && self.dag_child_count == 1 {
                        return Err(CanonicalFormatError::CompressibleUnaryNode);
                    }
                    self.complete(self.body_kind)?;
                }
                State::BlobFinish => {
                    self.require_body_finished()?;
                    self.complete(CanonicalObjectKind::Blob)?;
                }
                State::SemanticFinish => {
                    self.require_body_finished()?;
                    self.complete(CanonicalObjectKind::SemanticManifest)?;
                }
                State::AcceleratorFinish => {
                    self.require_body_finished()?;
                    self.complete(CanonicalObjectKind::AcceleratorManifest)?;
                }
                State::RetentionFinish => {
                    self.require_body_finished()?;
                    self.complete(CanonicalObjectKind::RetentionManifest)?;
                }
                State::AnchorFinish(root) => {
                    self.require_body_finished()?;
                    let count = self.pending_u64;
                    if (count == 0) != (root == empty_retention_radix_ref()) {
                        return Err(CanonicalFormatError::RetentionHistoryRootMismatch { count });
                    }
                    self.complete(CanonicalObjectKind::RetentionAnchor)?;
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn require_body_finished(&self) -> Result<(), CanonicalFormatError> {
        let offset = self.body_offset();
        if offset != self.body_len {
            return Err(CanonicalFormatError::TrailingBytes {
                offset,
                trailing: self.body_len - offset,
            });
        }
        Ok(())
    }

    fn complete(&mut self, kind: CanonicalObjectKind) -> Result<(), CanonicalFormatError> {
        if self.consumed != self.total_len {
            return Err(CanonicalFormatError::Truncated {
                offset: self.consumed,
                needed: self.total_len - self.consumed,
                available: 0,
            });
        }
        let hash = self.digest.clone().finalize();
        let mut digest = [0; OBJECT_DIGEST_LEN];
        digest.copy_from_slice(&hash);
        let reference = ObjectRefV1::new(ObjectDigest::from_array(digest), self.total_len as u64)?;
        self.field[..OBJECT_DIGEST_LEN].copy_from_slice(reference.digest().as_bytes());
        self.field[OBJECT_DIGEST_LEN..OBJECT_REF_LEN]
            .copy_from_slice(&reference.encoded_len().to_le_bytes());
        self.state = CanonicalObjectStreamStateV1::Complete(kind);
        Ok(())
    }

    fn completed_identity(
        &self,
    ) -> Result<Option<CanonicalObjectStreamIdentityV1>, CanonicalFormatError> {
        let CanonicalObjectStreamStateV1::Complete(kind) = self.state else {
            return Ok(None);
        };
        let digest = ObjectDigest::from_slice(&self.field[..OBJECT_DIGEST_LEN])?;
        let encoded_len = u64::from_le_bytes(
            self.field[OBJECT_DIGEST_LEN..OBJECT_REF_LEN]
                .try_into()
                .expect("object reference length has eight bytes"),
        );
        let reference = ObjectRefV1::new(digest, encoded_len)?;
        Ok(Some(CanonicalObjectStreamIdentityV1 { kind, reference }))
    }

    fn field_u64(&self) -> u64 {
        u64::from_le_bytes(
            self.field[..8]
                .try_into()
                .expect("u64 field has eight bytes"),
        )
    }

    fn object_envelope_name(kind: CanonicalObjectKind) -> &'static str {
        match kind {
            CanonicalObjectKind::DagNode => "RSpace DAG node",
            CanonicalObjectKind::Blob => "RSpace DAG blob",
            CanonicalObjectKind::SemanticManifest => "RSpace semantic manifest",
            CanonicalObjectKind::AcceleratorManifest => "RSpace accelerator manifest",
            CanonicalObjectKind::RetentionManifest => "RSpace retention manifest",
            CanonicalObjectKind::RetentionAnchor => "RSpace retention anchor",
            CanonicalObjectKind::RetentionRadixNode => "RSpace retention radix node",
        }
    }

    fn require_retention_bytes(
        generation: u64,
        reference: ObjectRefV1,
        actual: u64,
    ) -> Result<(), CanonicalFormatError> {
        if actual < reference.encoded_len() {
            return Err(CanonicalFormatError::RetentionStateBytes {
                generation,
                minimum: reference.encoded_len(),
                actual,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CanonicalObjectView<'a> {
    DagNode(CanonicalDagNodeView<'a>),
    Blob(CanonicalBlobView<'a>),
    SemanticManifest(SemanticStateManifestView),
    AcceleratorManifest(AcceleratorManifestView<'a>),
    RetentionManifest(RetentionManifestView<'a>),
}

impl<'a> CanonicalObjectView<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, CanonicalFormatError> {
        match CanonicalObjectKind::from_domain_prefix(bytes)? {
            CanonicalObjectKind::DagNode => Ok(Self::DagNode(CanonicalDagNodeView::parse(bytes)?)),
            CanonicalObjectKind::Blob => Ok(Self::Blob(CanonicalBlobView::parse(bytes)?)),
            CanonicalObjectKind::SemanticManifest => {
                Ok(Self::SemanticManifest(SemanticStateManifestView::parse(bytes)?))
            }
            CanonicalObjectKind::AcceleratorManifest => {
                Ok(Self::AcceleratorManifest(AcceleratorManifestView::parse(bytes)?))
            }
            CanonicalObjectKind::RetentionManifest => {
                Ok(Self::RetentionManifest(RetentionManifestView::parse(bytes)?))
            }
            CanonicalObjectKind::RetentionAnchor | CanonicalObjectKind::RetentionRadixNode => {
                Err(CanonicalFormatError::UnknownObjectDomain)
            }
        }
    }

    pub const fn kind(self) -> CanonicalObjectKind {
        match self {
            Self::DagNode(_) => CanonicalObjectKind::DagNode,
            Self::Blob(_) => CanonicalObjectKind::Blob,
            Self::SemanticManifest(_) => CanonicalObjectKind::SemanticManifest,
            Self::AcceleratorManifest(_) => CanonicalObjectKind::AcceleratorManifest,
            Self::RetentionManifest(_) => CanonicalObjectKind::RetentionManifest,
        }
    }
}

/// Fully parse one contiguous canonical object for a reader with the given
/// retention capability. A V1-only reader is exactly
/// [`CanonicalObjectView::parse`]; a dual reader also admits the retention
/// anchor and radix-node domains, each under its own parser.
pub(crate) fn validate_canonical_object_for(
    bytes: &[u8],
    reader: RetentionReader,
) -> Result<CanonicalObjectKind, CanonicalFormatError> {
    match CanonicalObjectKind::from_domain_prefix_for(bytes, reader)? {
        CanonicalObjectKind::RetentionAnchor => {
            RetentionAnchorV2::parse(bytes)?;
            Ok(CanonicalObjectKind::RetentionAnchor)
        }
        CanonicalObjectKind::RetentionRadixNode => {
            CanonicalDagNodeView::parse_in_domain(
                bytes,
                RETENTION_RADIX_DOMAIN,
                "RSpace retention radix node",
            )?;
            Ok(CanonicalObjectKind::RetentionRadixNode)
        }
        _ => CanonicalObjectView::parse(bytes).map(CanonicalObjectView::kind),
    }
}

const ENVELOPE_HEADER_LEN: usize = OBJECT_ENVELOPE_HEADER_LEN as usize;
const BRANCH_HEADER_LEN: usize = 1 + 8;

#[cfg(test)]
std::thread_local! {
    static VALIDATING_NODE_PARSES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_validating_node_parses() { VALIDATING_NODE_PARSES.set(0); }

#[cfg(test)]
pub(crate) fn validating_node_parses() -> u64 { VALIDATING_NODE_PARSES.get() }

#[derive(Clone, Copy, Debug)]
pub(crate) struct DagChildrenLayout {
    offset: usize,
    count: usize,
}

impl CanonicalDagNodeView<'_> {
    pub(crate) fn children_layout(self) -> DagChildrenLayout {
        let encoded_len = match self.children.encoding {
            ChildEncoding::Pairs(pairs) => pairs.len(),
            ChildEncoding::Node48 { slots, references } => slots.len() + references.len(),
            ChildEncoding::Node256 { bitmap, references } => bitmap.len() + references.len(),
        };
        DagChildrenLayout {
            offset: self.bytes.len() - encoded_len,
            count: self.children.count,
        }
    }
}

impl DagChildrenLayout {
    pub(crate) fn children(
        self,
        bytes: &[u8],
    ) -> Result<DagChildrenView<'_>, CanonicalFormatError> {
        let mut cursor = ReadCursor::new(bytes);
        cursor.read_exact(self.offset)?;
        self.read_children(cursor)
    }

    pub(crate) fn node(
        self,
        bytes: &[u8],
    ) -> Result<CanonicalDagNodeView<'_>, CanonicalFormatError> {
        let mut cursor = ReadCursor::new(bytes);
        let mut head = ReadCursor::new(cursor.read_exact(self.offset)?);
        head.read_exact(ENVELOPE_HEADER_LEN)?;
        let prefix_len = head.read_len("node prefix")?;
        let prefix = head.read_exact(prefix_len)?;
        let endpoint = EndpointValue::parse(&mut head)?;
        head.read_exact(BRANCH_HEADER_LEN)?;
        head.finish()?;
        let children = self.read_children(cursor)?;
        Ok(CanonicalDagNodeView {
            bytes,
            prefix,
            endpoint,
            children,
        })
    }

    fn read_children(
        self,
        mut cursor: ReadCursor<'_>,
    ) -> Result<DagChildrenView<'_>, CanonicalFormatError> {
        let kind = BranchKind::for_child_count(self.count);
        let reference_len = self.count.saturating_mul(OBJECT_REF_LEN);
        let encoding = match kind {
            BranchKind::Leaf | BranchKind::Node4 | BranchKind::Node16 => {
                ChildEncoding::Pairs(cursor.read_exact(self.count.saturating_mul(SMALL_CHILD_LEN))?)
            }
            BranchKind::Node48 => ChildEncoding::Node48 {
                slots: cursor.read_exact(NODE48_SLOT_TABLE_LEN)?,
                references: cursor.read_exact(reference_len)?,
            },
            BranchKind::Node256 => ChildEncoding::Node256 {
                bitmap: cursor.read_exact(CHILD_BITMAP_LEN)?,
                references: cursor.read_exact(reference_len)?,
            },
        };
        cursor.finish()?;
        Ok(DagChildrenView {
            kind,
            count: self.count,
            encoding,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DagChildPosition {
    index: u8,
    edge: u8,
}

impl DagChildrenView<'_> {
    pub(crate) fn highest(self) -> Option<DagChildPosition> {
        self.position_under(self.count, NODE48_SLOT_TABLE_LEN)
    }

    pub(crate) fn below(self, position: DagChildPosition) -> Option<DagChildPosition> {
        self.position_under(usize::from(position.index), usize::from(position.edge))
    }

    pub(crate) fn at(self, position: DagChildPosition) -> Option<DagChild> {
        let index = usize::from(position.index);
        if index >= self.count {
            return None;
        }
        let target = match self.encoding {
            ChildEncoding::Pairs(pairs) => {
                let start = index * SMALL_CHILD_LEN + 1;
                let mut cursor = ReadCursor::new(&pairs[start..start + OBJECT_REF_LEN]);
                ObjectRefV1::parse(&mut cursor).expect("validated child reference")
            }
            ChildEncoding::Node48 { references, .. } |
            ChildEncoding::Node256 { references, .. } => parse_reference_at(references, index),
        };
        Some(DagChild {
            edge: position.edge,
            target,
        })
    }

    fn position_under(self, index_bound: usize, edge_bound: usize) -> Option<DagChildPosition> {
        let index = index_bound.min(self.count).checked_sub(1)?;
        let edge = match self.encoding {
            ChildEncoding::Pairs(pairs) => usize::from(pairs[index * SMALL_CHILD_LEN]),
            ChildEncoding::Node48 { slots, .. } => slots
                .get(..edge_bound)?
                .iter()
                .rposition(|slot| *slot != ABSENT_NODE48_SLOT)?,
            ChildEncoding::Node256 { bitmap, .. } => highest_edge_below(bitmap, edge_bound)?,
        };
        Some(DagChildPosition {
            index: u8::try_from(index).ok()?,
            edge: u8::try_from(edge).ok()?,
        })
    }
}

fn highest_edge_below(bitmap: &[u8], bound: usize) -> Option<usize> {
    let (whole, bits) = (bound / 8, bound % 8);
    let partial = bitmap
        .get(whole)
        .map_or(0, |byte| byte & ((1_u8 << bits) - 1));
    let (byte_index, byte) = match partial {
        0 => {
            let byte_index = bitmap.get(..whole)?.iter().rposition(|byte| *byte != 0)?;
            (byte_index, bitmap[byte_index])
        }
        partial => (whole, partial),
    };
    Some(byte_index * 8 + 7 - byte.leading_zeros() as usize)
}
