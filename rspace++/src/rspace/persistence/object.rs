use std::num::NonZeroU64;

use super::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, OBJECT_DOMAIN_LEN,
    OBJECT_ENVELOPE_HEADER_LEN, ObjectDigest, ObjectRefV1, ReadCursor, checked_len, checked_sum,
    parse_envelope, write_envelope_header, write_u64,
};
use super::manifest::{
    ACCELERATOR_MANIFEST_DOMAIN, AcceleratorManifestView, SEMANTIC_MANIFEST_DOMAIN,
    SemanticStateManifestView,
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
}

impl CanonicalEncode for CanonicalDagNode {
    fn canonical_len(&self) -> u64 { self.canonical_len }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_envelope_header(sink, DAG_NODE_DOMAIN, self.body_len)?;
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
        let body = parse_envelope(bytes, DAG_NODE_DOMAIN, "RSpace DAG node")?;
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
}

impl CanonicalObjectKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::DagNode => "DAG-node",
            Self::Blob => "blob",
            Self::SemanticManifest => "semantic-manifest",
            Self::AcceleratorManifest => "accelerator-manifest",
            Self::RetentionManifest => "retention-manifest",
        }
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
        let Some(domain) = bytes.get(..OBJECT_DOMAIN_LEN) else {
            return Err(CanonicalFormatError::Truncated {
                offset: 0,
                needed: OBJECT_DOMAIN_LEN,
                available: bytes.len(),
            });
        };
        if domain == DAG_NODE_DOMAIN {
            Ok(Self::DagNode(CanonicalDagNodeView::parse(bytes)?))
        } else if domain == DAG_BLOB_DOMAIN {
            Ok(Self::Blob(CanonicalBlobView::parse(bytes)?))
        } else if domain == SEMANTIC_MANIFEST_DOMAIN {
            Ok(Self::SemanticManifest(SemanticStateManifestView::parse(bytes)?))
        } else if domain == ACCELERATOR_MANIFEST_DOMAIN {
            Ok(Self::AcceleratorManifest(AcceleratorManifestView::parse(bytes)?))
        } else if domain == RETENTION_MANIFEST_DOMAIN {
            Ok(Self::RetentionManifest(RetentionManifestView::parse(bytes)?))
        } else {
            Err(CanonicalFormatError::UnknownObjectDomain)
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
