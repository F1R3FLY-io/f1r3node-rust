//! Bounded cold reader for the shared retention V2 metadata.
//!
//! The appointed retention object is dispatched on its domain alone: a dual
//! reader parses the anchor domain as V2 and every other domain as the strict
//! V1 manifest, and never retries one format after the other refuses.
//!
//! A V2 history is read in two phases. Phase one reads each reachable radix
//! node once, checks its shape at the key width still unread where it is
//! reached, and counts entry occurrences with a memo keyed by (digest,
//! remaining width), so shared subtrees are counted without enumeration. Only
//! when that count equals the anchor's declared count does phase two reserve
//! exactly that many entries and enumerate them from the nodes already read.
//! Both phases keep at most one frame per key byte on an explicit stack.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt::{Display, Formatter};
use std::num::NonZeroU64;

use super::super::canonical::{
    CanonicalFormatError, OBJECT_DOMAIN_LEN, ObjectDigest, ObjectRefV1, checked_len,
};
use super::super::object::{CanonicalDagNodeView, DagChild, EndpointValue};
use super::super::store::ImmutableObjectStore;
use super::v2_format::{
    RETENTION_ANCHOR_DOMAIN, RETENTION_KEY_LEN, RETENTION_RADIX_DOMAIN, RetentionAnchorV2,
    RetentionFormat, RetentionKeyV2, RetentionReader,
};
use super::{RetainedStateV1, RetentionManifestView, validate_historical};

/// The retention object a catalog appoints, parsed for one reader capability.
#[derive(Clone, Copy, Debug)]
pub(crate) enum AppointedRetention<'a> {
    V1(RetentionManifestView<'a>),
    V2(RetentionAnchorV2),
}

impl<'a> AppointedRetention<'a> {
    pub(crate) fn parse(
        bytes: &'a [u8],
        reader: RetentionReader,
    ) -> Result<Self, CanonicalFormatError> {
        match bytes.get(..OBJECT_DOMAIN_LEN) {
            Some(domain)
                if domain == RETENTION_ANCHOR_DOMAIN && reader.admits(RetentionFormat::V2) =>
            {
                RetentionAnchorV2::parse(bytes).map(Self::V2)
            }
            _ => RetentionManifestView::parse(bytes).map(Self::V1),
        }
    }

    pub(crate) const fn format(self) -> RetentionFormat {
        match self {
            Self::V1(_) => RetentionFormat::V1,
            Self::V2(_) => RetentionFormat::V2,
        }
    }

    pub(crate) fn current(self) -> RetainedStateV1 {
        match self {
            Self::V1(manifest) => manifest.current(),
            Self::V2(anchor) => anchor.current(),
        }
    }

    /// The declared history length, known before any radix node is read.
    pub(crate) fn historical_count(self) -> Result<u64, CanonicalFormatError> {
        match self {
            Self::V1(manifest) => {
                checked_len(manifest.historical().len(), "retained historical states")
            }
            Self::V2(anchor) => Ok(anchor.historical_count()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RetentionRadixRefusalV2 {
    NotRadixDomain { object: ObjectDigest },
    Body(CanonicalFormatError),
    EndpointKind { endpoint: &'static str },
    EndpointDepth { remaining: usize, prefix: usize },
    EmptyBelowRoot,
    LeafKey(CanonicalFormatError),
    LeafClaim(CanonicalFormatError),
    CountMismatch { declared: u64, counted: u64 },
    CountOverflow,
    EntryAllocation { entries: u64 },
}

impl Display for RetentionRadixRefusalV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRadixDomain { object } => write!(
                formatter,
                "retention radix child {:?} is not a retention radix node",
                object.as_bytes()
            ),
            Self::Body(error) => write!(formatter, "retention radix node body: {error}"),
            Self::EndpointKind { endpoint } => {
                write!(formatter, "retention radix node carries a {endpoint} endpoint")
            }
            Self::EndpointDepth { remaining, prefix } => write!(
                formatter,
                "retention radix node with a {prefix}-byte prefix does not fit the {remaining} \
                 key bytes still unread"
            ),
            Self::EmptyBelowRoot => {
                formatter.write_str("an empty retention radix node is reachable below the root")
            }
            Self::LeafKey(error) => write!(formatter, "retention radix leaf key: {error}"),
            Self::LeafClaim(error) => write!(formatter, "retention radix leaf claim: {error}"),
            Self::CountMismatch { declared, counted } => write!(
                formatter,
                "retention anchor declares {declared} historical states, but its radix holds \
                 {counted}"
            ),
            Self::CountOverflow => {
                formatter.write_str("retention radix entry count overflows 64 bits")
            }
            Self::EntryAllocation { entries } => {
                write!(formatter, "cannot reserve {entries} retained historical states")
            }
        }
    }
}

impl std::error::Error for RetentionRadixRefusalV2 {}

#[derive(Debug)]
pub(crate) enum RetentionRadixErrorV2<E> {
    Store(E),
    Canonical(CanonicalFormatError),
    Refusal(RetentionRadixRefusalV2),
}

impl<E> From<RetentionRadixRefusalV2> for RetentionRadixErrorV2<E> {
    fn from(refusal: RetentionRadixRefusalV2) -> Self { Self::Refusal(refusal) }
}

impl<E> From<CanonicalFormatError> for RetentionRadixErrorV2<E> {
    fn from(error: CanonicalFormatError) -> Self { Self::Canonical(error) }
}

/// The complete V2 retention metadata closure: the anchor and every radix
/// node reachable from it, in digest order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RetentionMetadataInventoryV2 {
    objects: Box<[ObjectRefV1]>,
    canonical_bytes: u64,
}

impl RetentionMetadataInventoryV2 {
    pub(crate) fn objects(&self) -> &[ObjectRefV1] { &self.objects }

    pub(crate) fn object_count(&self) -> u64 { self.objects.len() as u64 }

    pub(crate) const fn canonical_bytes(&self) -> u64 { self.canonical_bytes }
}

/// An admitted V2 anchor together with its authenticated metadata closure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AdmittedRetentionMetadataV2 {
    anchor: RetentionAnchorV2,
    metadata: RetentionMetadataInventoryV2,
}

impl AdmittedRetentionMetadataV2 {
    pub(crate) const fn new(
        anchor: RetentionAnchorV2,
        metadata: RetentionMetadataInventoryV2,
    ) -> Self {
        Self { anchor, metadata }
    }

    pub(crate) const fn anchor(&self) -> RetentionAnchorV2 { self.anchor }

    pub(crate) const fn metadata(&self) -> &RetentionMetadataInventoryV2 { &self.metadata }
}

/// What cold admission retains about the appointed retention object beyond
/// its bytes: nothing for V1, the anchor and its metadata closure for V2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AdmittedRetention {
    V1,
    V2(AdmittedRetentionMetadataV2),
}

impl AdmittedRetention {
    pub(crate) const fn format(&self) -> RetentionFormat {
        match self {
            Self::V1 => RetentionFormat::V1,
            Self::V2(_) => RetentionFormat::V2,
        }
    }

    pub(crate) const fn metadata_v2(&self) -> Option<&AdmittedRetentionMetadataV2> {
        match self {
            Self::V1 => None,
            Self::V2(metadata) => Some(metadata),
        }
    }
}

/// A V2 history authenticated against its anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AdmittedRetentionHistoryV2 {
    entries: Vec<RetainedStateV1>,
    metadata: RetentionMetadataInventoryV2,
}

impl AdmittedRetentionHistoryV2 {
    pub(crate) fn entries(&self) -> &[RetainedStateV1] { &self.entries }

    pub(crate) const fn metadata(&self) -> &RetentionMetadataInventoryV2 { &self.metadata }

    pub(crate) fn into_parts(self) -> (Vec<RetainedStateV1>, RetentionMetadataInventoryV2) {
        (self.entries, self.metadata)
    }
}

#[derive(Clone, Copy)]
enum RadixEndpointV2 {
    Inner,
    Leaf(NonZeroU64),
}

struct RadixNodeV2 {
    prefix: [u8; RETENTION_KEY_LEN],
    prefix_len: usize,
    endpoint: RadixEndpointV2,
    children: Box<[DagChild]>,
}

impl RadixNodeV2 {
    fn prefix(&self) -> &[u8] { &self.prefix[..self.prefix_len] }
}

struct CountFrameV2 {
    digest: ObjectDigest,
    remaining: usize,
    below: usize,
    next_child: usize,
    total: u64,
}

enum CountStepV2 {
    Counted(u64),
    Opened(CountFrameV2),
}

struct EnumerationFrameV2 {
    digest: ObjectDigest,
    edge_offset: usize,
    next_child: usize,
}

/// Authenticate a V2 history: count before enumerating, enumerate exactly the
/// declared count, then apply the V1 history order and current-exclusion
/// rules to the decoded entries.
pub(crate) fn admit_retention_history_v2<S>(
    store: &S,
    anchor: RetentionAnchorV2,
) -> Result<AdmittedRetentionHistoryV2, RetentionRadixErrorV2<S::Error>>
where
    S: ImmutableObjectStore,
{
    let root = anchor.historical_root();
    let declared = anchor.historical_count();
    let mut nodes = BTreeMap::new();
    let entries = match declared {
        0 => {
            load_exact(store, root)?;
            Vec::new()
        }
        _ => {
            let counted = count_radix_history(store, root, &mut nodes)?;
            if counted != declared {
                return Err(RetentionRadixRefusalV2::CountMismatch { declared, counted }.into());
            }
            enumerate_radix_history(root.digest(), declared, &nodes)?
        }
    };
    validate_historical(anchor.current(), &entries)?;
    let metadata = match declared {
        0 => metadata_inventory(anchor.object_ref(), [root].into_iter())?,
        _ => metadata_inventory(
            anchor.object_ref(),
            nodes.values().map(|(reference, _)| *reference),
        )?,
    };
    Ok(AdmittedRetentionHistoryV2 { entries, metadata })
}

/// Merge the anchor into the digest-ordered radix references.
fn metadata_inventory(
    anchor: ObjectRefV1,
    radix: impl ExactSizeIterator<Item = ObjectRefV1>,
) -> Result<RetentionMetadataInventoryV2, CanonicalFormatError> {
    let mut objects = Vec::with_capacity(radix.len() + 1);
    let mut canonical_bytes = anchor.encoded_len();
    let mut anchor_pending = true;
    for reference in radix {
        if anchor_pending && anchor.digest() < reference.digest() {
            objects.push(anchor);
            anchor_pending = false;
        }
        canonical_bytes = canonical_bytes
            .checked_add(reference.encoded_len())
            .ok_or(CanonicalFormatError::CounterOverflow("retention metadata bytes"))?;
        objects.push(reference);
    }
    if anchor_pending {
        objects.push(anchor);
    }
    Ok(RetentionMetadataInventoryV2 {
        objects: objects.into_boxed_slice(),
        canonical_bytes,
    })
}

fn count_radix_history<S>(
    store: &S,
    root: ObjectRefV1,
    nodes: &mut BTreeMap<ObjectDigest, (ObjectRefV1, RadixNodeV2)>,
) -> Result<u64, RetentionRadixErrorV2<S::Error>>
where
    S: ImmutableObjectStore,
{
    let mut memo = BTreeMap::new();
    let mut stack = Vec::with_capacity(RETENTION_KEY_LEN);
    match open_count_frame(store, nodes, &mut memo, root, RETENTION_KEY_LEN)? {
        CountStepV2::Counted(count) => return Ok(count),
        CountStepV2::Opened(frame) => stack.push(frame),
    }
    loop {
        let (child, below) = {
            let frame = stack
                .last_mut()
                .expect("the count loop returns at the root");
            let (_, node) = nodes
                .get(&frame.digest)
                .expect("a frame is opened only for a node already read");
            match node.children.get(frame.next_child) {
                Some(child) => {
                    frame.next_child += 1;
                    (Some(*child), frame.below)
                }
                None => (None, frame.below),
            }
        };
        let finished = match child {
            Some(child) => match open_count_frame(store, nodes, &mut memo, child.target, below)? {
                CountStepV2::Counted(count) => count,
                CountStepV2::Opened(frame) => {
                    stack.push(frame);
                    continue;
                }
            },
            None => {
                let frame = stack
                    .pop()
                    .expect("the frame inspected above is still open");
                memo.insert((frame.digest, frame.remaining), frame.total);
                if stack.is_empty() {
                    return Ok(frame.total);
                }
                frame.total
            }
        };
        let parent = stack
            .last_mut()
            .expect("a counted child has an open parent");
        parent.total = parent
            .total
            .checked_add(finished)
            .ok_or(RetentionRadixRefusalV2::CountOverflow)?;
    }
}

fn open_count_frame<S>(
    store: &S,
    nodes: &mut BTreeMap<ObjectDigest, (ObjectRefV1, RadixNodeV2)>,
    memo: &mut BTreeMap<(ObjectDigest, usize), u64>,
    reference: ObjectRefV1,
    remaining: usize,
) -> Result<CountStepV2, RetentionRadixErrorV2<S::Error>>
where
    S: ImmutableObjectStore,
{
    if let Some((cached, _)) = nodes.get(&reference.digest()) {
        if *cached != reference {
            return Err(CanonicalFormatError::ObjectLengthMismatch {
                expected: reference.encoded_len(),
                actual: cached.encoded_len(),
            }
            .into());
        }
    }
    if let Some(count) = memo.get(&(reference.digest(), remaining)) {
        return Ok(CountStepV2::Counted(*count));
    }
    let node = match nodes.entry(reference.digest()) {
        Entry::Occupied(entry) => &entry.into_mut().1,
        Entry::Vacant(entry) => {
            let node = read_radix_node(store, reference, remaining)?;
            &entry.insert((reference, node)).1
        }
    };
    let prefix = node.prefix_len;
    match (node.endpoint, node.children.is_empty(), prefix.cmp(&remaining)) {
        (RadixEndpointV2::Leaf(_), true, Ordering::Equal) => {
            memo.insert((reference.digest(), remaining), 1);
            Ok(CountStepV2::Counted(1))
        }
        (RadixEndpointV2::Inner, true, _) => Err(RetentionRadixRefusalV2::EmptyBelowRoot.into()),
        (RadixEndpointV2::Inner, false, Ordering::Less) => Ok(CountStepV2::Opened(CountFrameV2 {
            digest: reference.digest(),
            remaining,
            below: remaining - prefix - 1,
            next_child: 0,
            total: 0,
        })),
        _ => Err(RetentionRadixRefusalV2::EndpointDepth { remaining, prefix }.into()),
    }
}

fn enumerate_radix_history(
    root: ObjectDigest,
    declared: u64,
    nodes: &BTreeMap<ObjectDigest, (ObjectRefV1, RadixNodeV2)>,
) -> Result<Vec<RetainedStateV1>, RetentionRadixRefusalV2> {
    let allocation = RetentionRadixRefusalV2::EntryAllocation { entries: declared };
    let capacity = usize::try_from(declared).map_err(|_| allocation.clone())?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(capacity)
        .map_err(|_| allocation)?;
    let mut key = [0_u8; RETENTION_KEY_LEN];
    let mut stack = Vec::with_capacity(RETENTION_KEY_LEN);
    enter_radix_node(root, 0, nodes, &mut key, &mut entries, &mut stack)?;
    while let Some(frame) = stack.last_mut() {
        let (_, node) = nodes
            .get(&frame.digest)
            .expect("phase one read every node it counted");
        let Some(child) = node.children.get(frame.next_child).copied() else {
            stack.pop();
            continue;
        };
        frame.next_child += 1;
        let edge_offset = frame.edge_offset;
        key[edge_offset] = child.edge;
        enter_radix_node(
            child.target.digest(),
            edge_offset + 1,
            nodes,
            &mut key,
            &mut entries,
            &mut stack,
        )?;
    }
    debug_assert_eq!(entries.len(), capacity);
    Ok(entries)
}

fn enter_radix_node(
    digest: ObjectDigest,
    start: usize,
    nodes: &BTreeMap<ObjectDigest, (ObjectRefV1, RadixNodeV2)>,
    key: &mut [u8; RETENTION_KEY_LEN],
    entries: &mut Vec<RetainedStateV1>,
    stack: &mut Vec<EnumerationFrameV2>,
) -> Result<(), RetentionRadixRefusalV2> {
    let (_, node) = nodes
        .get(&digest)
        .expect("phase one read every node it counted");
    let end = start + node.prefix_len;
    key[start..end].copy_from_slice(node.prefix());
    match node.endpoint {
        RadixEndpointV2::Leaf(claim) => {
            let state = RetentionKeyV2::parse(&key[..])
                .map_err(RetentionRadixRefusalV2::LeafKey)?
                .state();
            let retained = RetainedStateV1::new(state, claim.get())
                .map_err(RetentionRadixRefusalV2::LeafClaim)?;
            entries.push(retained);
        }
        RadixEndpointV2::Inner => stack.push(EnumerationFrameV2 {
            digest,
            edge_offset: end,
            next_child: 0,
        }),
    }
    Ok(())
}

fn read_radix_node<S>(
    store: &S,
    reference: ObjectRefV1,
    remaining: usize,
) -> Result<RadixNodeV2, RetentionRadixErrorV2<S::Error>>
where
    S: ImmutableObjectStore,
{
    let bytes = load_exact(store, reference)?;
    let bytes = bytes.as_ref();
    match bytes.get(..OBJECT_DOMAIN_LEN) {
        Some(domain) if domain == RETENTION_RADIX_DOMAIN => {}
        _ => {
            return Err(RetentionRadixRefusalV2::NotRadixDomain {
                object: reference.digest(),
            }
            .into());
        }
    }
    let view = CanonicalDagNodeView::parse_in_domain(
        bytes,
        RETENTION_RADIX_DOMAIN,
        "RSpace retention radix node",
    )
    .map_err(RetentionRadixRefusalV2::Body)?;
    let endpoint = match view.endpoint() {
        EndpointValue::Absent => RadixEndpointV2::Inner,
        EndpointValue::Multiplicity(claim) => RadixEndpointV2::Leaf(claim),
        endpoint => {
            return Err(RetentionRadixRefusalV2::EndpointKind {
                endpoint: endpoint.name(),
            }
            .into());
        }
    };
    let prefix_len = view.prefix().len();
    if prefix_len > remaining {
        return Err(RetentionRadixRefusalV2::EndpointDepth {
            remaining,
            prefix: prefix_len,
        }
        .into());
    }
    let mut prefix = [0_u8; RETENTION_KEY_LEN];
    prefix[..prefix_len].copy_from_slice(view.prefix());
    let mut children = Vec::with_capacity(view.children().len());
    children.extend(view.children().iter());
    Ok(RadixNodeV2 {
        prefix,
        prefix_len,
        endpoint,
        children: children.into_boxed_slice(),
    })
}

fn load_exact<S>(
    store: &S,
    reference: ObjectRefV1,
) -> Result<S::Object, RetentionRadixErrorV2<S::Error>>
where
    S: ImmutableObjectStore,
{
    let object = store
        .get_bounded(&reference.digest(), reference.encoded_len())
        .map_err(RetentionRadixErrorV2::Store)?
        .ok_or(CanonicalFormatError::MissingObject(reference.digest()))?;
    let bytes = object.as_ref();
    let actual_len = checked_len(bytes.len(), "retention metadata object")?;
    if actual_len != reference.encoded_len() {
        return Err(CanonicalFormatError::ObjectLengthMismatch {
            expected: reference.encoded_len(),
            actual: actual_len,
        }
        .into());
    }
    let actual = ObjectDigest::of_canonical_bytes(bytes);
    if actual != reference.digest() {
        return Err(CanonicalFormatError::DigestMismatch {
            expected: reference.digest(),
            actual,
        }
        .into());
    }
    Ok(object)
}
