#[cfg(test)]
use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, Range};
use std::sync::{Arc, Weak};

use verified_score_store::{SourceBorrowedLeaf, SourceNodeShape};

use super::canonical_bytes_arena::{
    CanonicalByteProvenance, CanonicalLocatedByte, FrozenCanonicalByteCursor, FrozenCanonicalBytes,
    FrozenCanonicalLocatedByteCursor,
};

#[cfg(test)]
#[path = "../../../tests/support/canonical_source_tree.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "../../../tests/support/source_tree_score_projection.rs"]
mod score_projection_tests;

const RELEASE_CAPACITY: usize = 1 + 2 * (usize::BITS as usize - 1);

type NodeRef<'src> = Arc<SourceNode<'src>>;
type Root<'src> = Option<NodeRef<'src>>;

struct SourceNode<'src> {
    shape: SourceNodeShape,
    payload: Option<SourcePayload<'src>>,
}

impl Deref for SourceNode<'_> {
    type Target = SourceNodeShape;

    fn deref(&self) -> &Self::Target { &self.shape }
}

enum SourcePayload<'src> {
    Borrowed(SourceBorrowedLeaf<'src>),
    Frozen(FrozenCanonicalBytes),
    Branch2([NodeRef<'src>; 2]),
    Branch3([NodeRef<'src>; 3]),
}

impl<'src> SourcePayload<'src> {
    fn borrowed(
        bytes: &'src [u8],
        range: Range<usize>,
        source: u32,
        absolute_start: usize,
    ) -> Option<(Self, SourceNodeShape)> {
        let leaf = SourceBorrowedLeaf::new(bytes, range.start, range.end, source, absolute_start)?;
        let shape = leaf.shape();
        Some((Self::Borrowed(leaf), shape))
    }
}

impl Drop for SourceNode<'_> {
    fn drop(&mut self) {
        match self.payload.take() {
            None | Some(SourcePayload::Borrowed(_)) => {}
            Some(SourcePayload::Frozen(value)) => drop(value),
            Some(payload) => release_branch(payload),
        }
    }
}

#[inline(never)]
fn release_branch(payload: SourcePayload<'_>) {
    let mut pending: [Option<NodeRef<'_>>; RELEASE_CAPACITY] = std::array::from_fn(|_| None);
    let mut count = 0;
    let mut active = Some(payload);
    loop {
        match active.take() {
            Some(SourcePayload::Branch2(children)) => {
                for child in children {
                    pending[count] = Some(child);
                    count += 1;
                }
            }
            Some(SourcePayload::Branch3(children)) => {
                for child in children {
                    pending[count] = Some(child);
                    count += 1;
                }
            }
            Some(SourcePayload::Frozen(value)) => drop(value),
            None | Some(SourcePayload::Borrowed(_)) => {}
        }
        loop {
            if count == 0 {
                return;
            }
            count -= 1;
            let child = pending[count].take().expect("occupied source release slot");
            if let Some(mut node) = Arc::into_inner(child) {
                active = node.payload.take();
                break;
            }
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct SourceTreeWindow<'src> {
    root: Root<'src>,
    start: usize,
    len: usize,
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) enum ProjectedSourcePayload {
    Borrowed(Vec<u8>),
    Frozen(Vec<u8>),
    Branch2([usize; 2]),
    Branch3([usize; 3]),
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct ProjectedSourceNode {
    pub(crate) identity: usize,
    pub(crate) len: usize,
    pub(crate) payload: ProjectedSourcePayload,
    pub(crate) anchors: Option<Vec<CanonicalByteProvenance>>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProjectedSourceWindow {
    pub(crate) root: Option<usize>,
    pub(crate) start: usize,
    pub(crate) len: usize,
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct SourceDagProjection {
    pub(crate) nodes: Vec<ProjectedSourceNode>,
    pub(crate) root: ProjectedSourceWindow,
    pub(crate) pending: Vec<ProjectedSourceWindow>,
}

#[cfg(test)]
impl SourceDagProjection {
    pub(crate) fn capture(source: &SourceTreeWindow<'_>, pending: &[SourceTreeWindow<'_>]) -> Self {
        let mut nodes: Vec<ProjectedSourceNode> = Vec::new();
        let mut ids = HashMap::new();
        for window in std::iter::once(source).chain(pending) {
            let Some(root) = window.root.as_ref() else {
                assert_eq!(window.len, 0);
                continue;
            };
            let mut walk = vec![(root, false)];
            while let Some((node, expanded)) = walk.pop() {
                let identity = Arc::as_ptr(node) as usize;
                if ids.contains_key(&identity) {
                    continue;
                }
                if !expanded {
                    walk.push((node, true));
                    match node.payload.as_ref().expect("live source node payload") {
                        SourcePayload::Branch2(children) => {
                            walk.extend(children.iter().rev().map(|child| (child, false)));
                        }
                        SourcePayload::Branch3(children) => {
                            walk.extend(children.iter().rev().map(|child| (child, false)));
                        }
                        SourcePayload::Borrowed(_) | SourcePayload::Frozen(_) => {}
                    }
                    continue;
                }
                let (payload, anchors) =
                    match node.payload.as_ref().expect("live source node payload") {
                        SourcePayload::Borrowed(leaf) => {
                            let (selected, _) = leaf
                                .selected_slice(0, leaf.len())
                                .expect("borrowed source leaf");
                            let anchors = (0..leaf.len())
                                .map(|at| {
                                    let located =
                                        leaf.located_at(at).expect("verified borrowed leaf");
                                    CanonicalByteProvenance::new(located.1, located.2)
                                })
                                .collect();
                            (
                                ProjectedSourcePayload::Borrowed(selected.to_vec()),
                                Some(anchors),
                            )
                        }
                        SourcePayload::Frozen(value) => {
                            let located = value.located_bytes().collect::<Vec<_>>();
                            let bytes = located.iter().map(|entry| entry.byte).collect();
                            let anchors = located.iter().map(|entry| entry.provenance).collect();
                            (ProjectedSourcePayload::Frozen(bytes), Some(anchors))
                        }
                        SourcePayload::Branch2(children) => (
                            ProjectedSourcePayload::Branch2(std::array::from_fn(|i| {
                                ids[&(Arc::as_ptr(&children[i]) as usize)]
                            })),
                            None,
                        ),
                        SourcePayload::Branch3(children) => (
                            ProjectedSourcePayload::Branch3(std::array::from_fn(|i| {
                                ids[&(Arc::as_ptr(&children[i]) as usize)]
                            })),
                            None,
                        ),
                    };
                let length = match &payload {
                    ProjectedSourcePayload::Borrowed(bytes)
                    | ProjectedSourcePayload::Frozen(bytes) => bytes.len(),
                    ProjectedSourcePayload::Branch2(children) => children
                        .iter()
                        .try_fold(0usize, |length, child| {
                            length.checked_add(nodes[*child].len)
                        })
                        .expect("checked branch length"),
                    ProjectedSourcePayload::Branch3(children) => children
                        .iter()
                        .try_fold(0usize, |length, child| {
                            length.checked_add(nodes[*child].len)
                        })
                        .expect("checked branch length"),
                };
                assert_eq!(length, node.len);
                let id = nodes.len();
                nodes.push(ProjectedSourceNode {
                    identity,
                    len: length,
                    payload,
                    anchors,
                });
                ids.insert(identity, id);
            }
        }
        let project_window = |window: &SourceTreeWindow<'_>| {
            let root = window
                .root
                .as_ref()
                .map(|node| ids[&(Arc::as_ptr(node) as usize)]);
            assert!(window
                .start
                .checked_add(window.len)
                .is_some_and(|end| end <= root.map_or(0, |id| nodes[id].len)));
            ProjectedSourceWindow {
                root,
                start: window.start,
                len: window.len,
            }
        };
        Self {
            root: project_window(source),
            pending: pending.iter().map(project_window).collect(),
            nodes,
        }
    }

    pub(crate) fn window_bytes(&self, window: ProjectedSourceWindow) -> Vec<u8> {
        self.window_located_bytes(window)
            .into_iter()
            .map(|entry| entry.byte)
            .collect()
    }

    /// Test-only byte/anchor image reconstructed from the retained DAG payloads.
    /// This is executable differential evidence, not an Arc-to-ghost proof.
    pub(crate) fn window_located_bytes(
        &self,
        window: ProjectedSourceWindow,
    ) -> Vec<CanonicalLocatedByte> {
        (0..window.len)
            .map(|offset| {
                let mut at = window.start + offset;
                let mut node_id = window.root.expect("nonempty projected window");
                loop {
                    let node = &self.nodes[node_id];
                    match &node.payload {
                        ProjectedSourcePayload::Borrowed(bytes)
                        | ProjectedSourcePayload::Frozen(bytes) => {
                            let anchor = node.anchors.as_ref().expect("leaf anchor image")[at];
                            break CanonicalLocatedByte::new(bytes[at], anchor);
                        }
                        ProjectedSourcePayload::Branch2(children) => {
                            let left = children[0];
                            if at < self.nodes[left].len {
                                node_id = left;
                            } else {
                                at -= self.nodes[left].len;
                                node_id = children[1];
                            }
                        }
                        ProjectedSourcePayload::Branch3(children) => {
                            let first = children[0];
                            let second = children[1];
                            if at < self.nodes[first].len {
                                node_id = first;
                            } else {
                                at -= self.nodes[first].len;
                                if at < self.nodes[second].len {
                                    node_id = second;
                                } else {
                                    at -= self.nodes[second].len;
                                    node_id = children[2];
                                }
                            }
                        }
                    }
                }
            })
            .collect()
    }
}

pub(crate) struct WeakSourceTreeWindow<'src> {
    root: Weak<SourceNode<'src>>,
    start: usize,
    len: usize,
}

impl<'src> WeakSourceTreeWindow<'src> {
    pub(crate) fn upgrade(&self) -> Option<SourceTreeWindow<'src>> {
        let root = self.root.upgrade()?;
        if self.start.checked_add(self.len)? > root.len {
            return None;
        }
        Some(SourceTreeWindow {
            root: Some(root),
            start: self.start,
            len: self.len,
        })
    }
}

impl fmt::Debug for SourceTreeWindow<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceTreeWindow")
            .field("start", &self.start)
            .field("len", &self.len)
            .field("height", &self.height())
            .finish()
    }
}

impl<'src> SourceTreeWindow<'src> {
    pub(crate) fn empty() -> Self { Self::default() }

    pub(crate) fn downgrade(&self) -> Option<WeakSourceTreeWindow<'src>> {
        let root = self.root.as_ref()?;
        if self.start.checked_add(self.len)? > root.len {
            return None;
        }
        Some(WeakSourceTreeWindow {
            root: Arc::downgrade(root),
            start: self.start,
            len: self.len,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_borrowed(
        bytes: &'src [u8],
        source: u32,
        absolute_start: usize,
    ) -> Option<Self> {
        Self::from_borrowed_range(bytes, 0..bytes.len(), source, absolute_start)
    }

    pub(crate) fn from_borrowed_range(
        bytes: &'src [u8],
        range: Range<usize>,
        source: u32,
        absolute_start: usize,
    ) -> Option<Self> {
        let (payload, shape) = SourcePayload::borrowed(bytes, range, source, absolute_start)?;
        if shape.len == 0 {
            return Some(Self::empty());
        }
        Some(Self::from_root(Some(Arc::new(SourceNode {
            shape,
            payload: Some(payload),
        }))))
    }

    pub(crate) fn from_frozen(value: FrozenCanonicalBytes) -> Self {
        if value.is_empty() {
            return Self::empty();
        }
        Self::from_root(Some(Arc::new(SourceNode {
            shape: verified_score_store::source_leaf_shape(value.len()),
            payload: Some(SourcePayload::Frozen(value)),
        })))
    }

    fn from_root(root: Root<'src>) -> Self {
        let len = root.as_ref().map_or(0, |node| node.len);
        Self {
            root,
            start: 0,
            len,
        }
    }

    pub(crate) fn len(&self) -> usize { self.len }

    pub(crate) fn is_empty(&self) -> bool { self.len == 0 }

    pub(crate) fn shares_root_with(&self, other: &SourceTreeWindow<'_>) -> bool {
        match (&self.root, &other.root) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            (None, None) => true,
            _ => false,
        }
    }

    pub(crate) fn same_view(&self, other: &SourceTreeWindow<'_>) -> bool {
        self.shares_root_with(other) && self.start == other.start && self.len == other.len
    }

    pub(crate) fn height(&self) -> usize {
        self.root
            .as_ref()
            .map_or(0, |node| usize::from(node.height))
    }

    pub(crate) fn subview(&self, range: Range<usize>) -> Option<Self> {
        if range.start > range.end || range.end > self.len {
            return None;
        }
        Some(Self {
            root: self.root.clone(),
            start: self.start.checked_add(range.start)?,
            len: range.end - range.start,
        })
    }

    pub(crate) fn try_coalesce(&self, right: &Self) -> Option<Self> {
        let len = self.len.checked_add(right.len)?;
        match (&self.root, &right.root) {
            (Some(left_root), Some(right_root))
                if Arc::ptr_eq(left_root, right_root)
                    && self.start.checked_add(self.len)? == right.start =>
            {
                Some(Self {
                    root: self.root.clone(),
                    start: self.start,
                    len,
                })
            }
            (None, _) if self.len == 0 => Some(right.clone()),
            (_, None) if right.len == 0 => Some(self.clone()),
            _ => self.coalesce_original_leaf(right, len),
        }
    }

    fn coalesce_original_leaf(&self, right: &Self, len: usize) -> Option<Self> {
        let (payload, shape) = match (
            self.root.as_ref()?.payload.as_ref()?,
            right.root.as_ref()?.payload.as_ref()?,
        ) {
            (SourcePayload::Borrowed(left_leaf), SourcePayload::Borrowed(right_leaf)) => {
                if !std::ptr::eq(left_leaf.root_slice(), right_leaf.root_slice())
                    || left_leaf.source() != right_leaf.source()
                    || left_leaf.absolute_start() != right_leaf.absolute_start()
                {
                    return None;
                }
                let start = left_leaf.start().checked_add(self.start)?;
                if start.checked_add(self.len)? != right_leaf.start().checked_add(right.start)? {
                    return None;
                }
                if len == 0 {
                    return Some(self.clone());
                }
                SourcePayload::borrowed(
                    left_leaf.root_slice(),
                    start..start.checked_add(len)?,
                    left_leaf.source(),
                    left_leaf.absolute_start(),
                )?
            }
            (SourcePayload::Frozen(left_value), SourcePayload::Frozen(right_value)) => {
                let left_value =
                    left_value.subview(self.start..self.start.checked_add(self.len)?)?;
                let right_value =
                    right_value.subview(right.start..right.start.checked_add(right.len)?)?;
                (
                    SourcePayload::Frozen(left_value.try_coalesce(&right_value)?),
                    verified_score_store::source_leaf_shape(len),
                )
            }
            _ => return None,
        };
        if len == 0 {
            return Some(self.clone());
        }
        Some(Self::from_root(Some(Arc::new(SourceNode {
            shape,
            payload: Some(payload),
        }))))
    }

    pub(crate) fn concat(&self, right: &Self) -> Option<Self> {
        self.len.checked_add(right.len)?;
        if self.is_empty() {
            return Some(right.clone());
        }
        if right.is_empty() {
            return Some(self.clone());
        }
        if let Some(joined) = self.try_coalesce(right) {
            return Some(joined);
        }
        let first = self.normalized_root()?;
        let last = right.normalized_root()?;
        Some(Self::from_root(join_optional(first, last)?))
    }

    #[cfg(test)]
    pub(crate) fn split(&self, cut: usize) -> Option<(Self, Self)> {
        if cut > self.len {
            return None;
        }
        let root = self.normalized_root()?;
        let (first, last) = split_root(root, cut)?;
        Some((Self::from_root(first), Self::from_root(last)))
    }

    pub(crate) fn trimmed_subview(&self, range: Range<usize>) -> Option<Self> {
        Some(Self::from_root(self.subview(range)?.normalized_root()?))
    }

    fn normalized_root(&self) -> Option<Root<'src>> {
        if self.is_empty() {
            return Some(None);
        }
        let root = self.root.as_ref()?;
        if self.start == 0 && self.len == root.len {
            return Some(Some(Arc::clone(root)));
        }
        let end = self.start.checked_add(self.len)?;
        let (prefix, _) = split_root(Some(Arc::clone(root)), end)?;
        let (_, selected) = split_root(prefix, self.start)?;
        Some(selected)
    }

    fn leaf_at(&self, offset: usize) -> Option<(&SourcePayload<'src>, usize)> {
        if offset >= self.len {
            return None;
        }
        let mut at = self.start.checked_add(offset)?;
        let mut node = self.root.as_deref()?;
        loop {
            match node.payload.as_ref()? {
                SourcePayload::Branch2(children) => {
                    let (child, local) = verified_score_store::source_branch2_descend(
                        node.shape,
                        children[0].shape,
                        children[1].shape,
                        at,
                    )?;
                    at = local;
                    node = if child == 0 {
                        &children[0]
                    } else {
                        &children[1]
                    };
                }
                SourcePayload::Branch3(children) => {
                    let (child, local) = verified_score_store::source_branch3_descend(
                        node.shape,
                        children[0].shape,
                        children[1].shape,
                        children[2].shape,
                        at,
                    )?;
                    at = local;
                    node = match child {
                        0 => &children[0],
                        1 => &children[1],
                        _ => &children[2],
                    };
                }
                leaf => return Some((leaf, at)),
            }
        }
    }

    pub(crate) fn byte_at(&self, offset: usize) -> Option<u8> {
        let (leaf, at) = self.leaf_at(offset)?;
        match leaf {
            SourcePayload::Borrowed(value) => value.located_at(at).map(|located| located.0),
            SourcePayload::Frozen(value) => value.byte_at(at),
            _ => None,
        }
    }

    pub(crate) fn provenance_at(&self, offset: usize) -> Option<CanonicalByteProvenance> {
        let (leaf, at) = self.leaf_at(offset)?;
        match leaf {
            SourcePayload::Borrowed(value) => value
                .located_at(at)
                .map(|located| CanonicalByteProvenance::new(located.1, located.2)),
            SourcePayload::Frozen(value) => value.provenance_at(at),
            _ => None,
        }
    }

    pub(crate) fn contiguous_bytes(&self, range: Range<usize>) -> Option<&[u8]> {
        if range.start > range.end || range.end > self.len {
            return None;
        }
        if range.is_empty() {
            return Some(&[]);
        }
        let (leaf, at) = self.leaf_at(range.start)?;
        let end = at.checked_add(range.end - range.start)?;
        match leaf {
            SourcePayload::Borrowed(value) => {
                value.selected_slice(at, end).map(|selected| selected.0)
            }
            SourcePayload::Frozen(value) => value.contiguous_bytes(at..end),
            _ => None,
        }
    }

    pub(crate) fn bytes(&self) -> SourceTreeByteCursor<'src> {
        SourceTreeByteCursor(SourceCursor::new(self.clone()))
    }

    pub(crate) fn located_bytes(&self) -> SourceTreeLocatedByteCursor<'src> {
        SourceTreeLocatedByteCursor(SourceCursor::new(self.clone()))
    }

    pub(crate) fn copy_exact(&self, start: usize, output: &mut [u8]) -> bool {
        if start
            .checked_add(output.len())
            .is_none_or(|end| end > self.len)
        {
            return false;
        }
        if let Some(bytes) = self.contiguous_bytes(start..start + output.len()) {
            output.copy_from_slice(bytes);
            return true;
        }
        let mut cursor = self.bytes();
        cursor.skip_exact(start);
        let mut written = 0;
        while written < output.len() {
            let chunk = cursor.chunk().expect("validated source copy range");
            let width = chunk.len().min(output.len() - written);
            output[written..written + width].copy_from_slice(&chunk[..width]);
            cursor.skip_exact(width);
            written += width;
        }
        true
    }

    pub(crate) fn content_eq(&self, other: &SourceTreeWindow<'_>) -> bool {
        if self.len != other.len {
            return false;
        }
        if self.len == 0 || (self.start == other.start && self.shares_root_with(other)) {
            return true;
        }
        if let (Some(left), Some(right)) = (
            self.contiguous_bytes(0..self.len),
            other.contiguous_bytes(0..other.len),
        ) {
            return left == right;
        }
        let mut left = self.bytes();
        let mut right = other.bytes();
        while left.len() != 0 {
            let left_chunk = left.chunk().expect("remaining left source bytes");
            let right_chunk = right.chunk().expect("remaining right source bytes");
            let width = left_chunk.len().min(right_chunk.len());
            if left_chunk[..width] != right_chunk[..width] {
                return false;
            }
            left.skip_exact(width);
            right.skip_exact(width);
        }
        true
    }
}

trait CursorMode: Clone {
    type Item;
    type Frozen: Clone + ExactSizeIterator<Item = Self::Item>;

    fn borrowed(byte: u8, source: u32, offset: usize) -> Self::Item;
    fn frozen(value: FrozenCanonicalBytes) -> Self::Frozen;
    fn skip(cursor: &mut Self::Frozen, width: usize);
}

#[derive(Clone)]
struct ByteMode;

impl CursorMode for ByteMode {
    type Item = u8;
    type Frozen = FrozenCanonicalByteCursor;

    fn borrowed(byte: u8, _: u32, _: usize) -> u8 { byte }
    fn frozen(value: FrozenCanonicalBytes) -> Self::Frozen { value.bytes() }
    fn skip(cursor: &mut Self::Frozen, width: usize) { cursor.fast_forward(width); }
}

#[derive(Clone)]
struct LocatedMode;

impl CursorMode for LocatedMode {
    type Item = CanonicalLocatedByte;
    type Frozen = FrozenCanonicalLocatedByteCursor;

    fn borrowed(byte: u8, source: u32, offset: usize) -> Self::Item {
        CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(source, offset))
    }
    fn frozen(value: FrozenCanonicalBytes) -> Self::Frozen { value.located_bytes() }
    fn skip(cursor: &mut Self::Frozen, width: usize) { cursor.fast_forward(width); }
}

#[derive(Clone)]
enum ActiveLeaf<'src, M: CursorMode> {
    Borrowed {
        bytes: &'src [u8],
        source: u32,
        absolute_start: usize,
        at: usize,
    },
    Frozen(M::Frozen),
}

impl<'src, M: CursorMode> ActiveLeaf<'src, M> {
    fn new(payload: &SourcePayload<'src>, range: Range<usize>) -> Option<Self> {
        match payload {
            SourcePayload::Borrowed(value) => {
                let (bytes, absolute_start) = value.selected_slice(range.start, range.end)?;
                Some(Self::Borrowed {
                    bytes,
                    source: value.source(),
                    absolute_start,
                    at: 0,
                })
            }
            SourcePayload::Frozen(value) => Some(Self::Frozen(M::frozen(value.subview(range)?))),
            _ => None,
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Borrowed { bytes, at, .. } => bytes.len() - at,
            Self::Frozen(cursor) => cursor.len(),
        }
    }

    fn skip(&mut self, width: usize) {
        match self {
            Self::Borrowed { at, .. } => *at += width,
            Self::Frozen(cursor) => M::skip(cursor, width),
        }
    }

    fn next(&mut self) -> Option<M::Item> {
        match self {
            Self::Borrowed {
                bytes,
                source,
                absolute_start,
                at,
            } => {
                let byte = *bytes.get(*at)?;
                let value = M::borrowed(byte, *source, *absolute_start + *at);
                *at += 1;
                Some(value)
            }
            Self::Frozen(cursor) => cursor.next(),
        }
    }
}

#[derive(Clone)]
struct SourceCursor<'src, M: CursorMode> {
    pending: Vec<SourceTreeWindow<'src>>,
    active: Option<ActiveLeaf<'src, M>>,
    remaining: usize,
}

impl<'src, M: CursorMode> SourceCursor<'src, M> {
    fn new(source: SourceTreeWindow<'src>) -> Self {
        let remaining = source.len;
        let mut pending = Vec::new();
        let mut active = None;
        if remaining != 0 {
            active = source.root.as_ref().and_then(|node| {
                ActiveLeaf::new(
                    node.payload.as_ref()?,
                    source.start..source.start + source.len,
                )
            });
            if active.is_none() {
                pending.reserve(1 + 2 * source.height());
                pending.push(source);
            }
        }
        Self {
            pending,
            active,
            remaining,
        }
    }

    fn expand_children(&mut self, children: &[NodeRef<'src>], range: Range<usize>) {
        let mut end = children.iter().map(|child| child.len).sum::<usize>();
        for child in children.iter().rev() {
            let start = end - child.len;
            let selected_start = start.max(range.start);
            let selected_end = end.min(range.end);
            if selected_start < selected_end {
                self.pending.push(SourceTreeWindow {
                    root: Some(Arc::clone(child)),
                    start: selected_start - start,
                    len: selected_end - selected_start,
                });
            }
            end = start;
        }
    }

    fn prepare_active(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        loop {
            if self.active.as_ref().is_some_and(|active| active.len() != 0) {
                return true;
            }
            self.active = None;
            let Some(window) = self.pending.pop() else {
                return false;
            };
            let node = window.root.as_ref().expect("nonempty pending source root");
            let range = window.start..window.start + window.len;
            match node.payload.as_ref().expect("retained source payload") {
                SourcePayload::Branch2(children) => self.expand_children(children, range),
                SourcePayload::Branch3(children) => self.expand_children(children, range),
                leaf => self.active = ActiveLeaf::new(leaf, range),
            }
        }
    }

    fn next(&mut self) -> Option<M::Item> {
        if !self.prepare_active() {
            return None;
        }
        let value = self.active.as_mut()?.next()?;
        self.remaining -= 1;
        Some(value)
    }

    fn skip_exact(&mut self, mut width: usize) -> bool {
        if width > self.remaining {
            return false;
        }
        self.remaining -= width;
        if let Some(active) = &mut self.active {
            let consumed = width.min(active.len());
            active.skip(consumed);
            width -= consumed;
            if active.len() == 0 {
                self.active = None;
            }
        }
        while width != 0 {
            let last = self
                .pending
                .last_mut()
                .expect("remaining source cursor window");
            if width < last.len {
                last.start += width;
                last.len -= width;
                break;
            }
            width -= last.len;
            self.pending.pop();
        }
        true
    }
}

#[derive(Clone)]
pub(crate) struct SourceTreeByteCursor<'src>(SourceCursor<'src, ByteMode>);

impl SourceTreeByteCursor<'_> {
    pub(crate) fn skip_exact(&mut self, width: usize) -> bool { self.0.skip_exact(width) }

    pub(crate) fn chunk(&mut self) -> Option<&[u8]> {
        if !self.0.prepare_active() {
            return None;
        }
        match self.0.active.as_mut()? {
            ActiveLeaf::Borrowed { bytes, at, .. } => Some(&bytes[*at..]),
            ActiveLeaf::Frozen(cursor) => cursor.chunk(),
        }
    }
}

impl Iterator for SourceTreeByteCursor<'_> {
    type Item = u8;
    fn next(&mut self) -> Option<u8> { self.0.next() }
    fn size_hint(&self) -> (usize, Option<usize>) { (self.0.remaining, Some(self.0.remaining)) }
}

impl ExactSizeIterator for SourceTreeByteCursor<'_> {}

#[derive(Clone)]
pub(crate) struct SourceTreeLocatedByteCursor<'src>(SourceCursor<'src, LocatedMode>);

impl SourceTreeLocatedByteCursor<'_> {
    pub(crate) fn skip_exact(&mut self, width: usize) -> bool { self.0.skip_exact(width) }
}

impl Iterator for SourceTreeLocatedByteCursor<'_> {
    type Item = CanonicalLocatedByte;
    fn next(&mut self) -> Option<Self::Item> { self.0.next() }
    fn size_hint(&self) -> (usize, Option<usize>) { (self.0.remaining, Some(self.0.remaining)) }
}

impl ExactSizeIterator for SourceTreeLocatedByteCursor<'_> {}

fn node2<'src>(left: NodeRef<'src>, right: NodeRef<'src>) -> Option<NodeRef<'src>> {
    let shape = verified_score_store::source_branch2_shape(left.shape, right.shape)?;
    Some(Arc::new(SourceNode {
        shape,
        payload: Some(SourcePayload::Branch2([left, right])),
    }))
}

fn node3<'src>(
    first: NodeRef<'src>,
    middle: NodeRef<'src>,
    last: NodeRef<'src>,
) -> Option<NodeRef<'src>> {
    let shape = verified_score_store::source_branch3_shape(first.shape, middle.shape, last.shape)?;
    Some(Arc::new(SourceNode {
        shape,
        payload: Some(SourcePayload::Branch3([first, middle, last])),
    }))
}

enum Pieces<'src> {
    One(NodeRef<'src>),
    Two(NodeRef<'src>, NodeRef<'src>),
}

enum JoinFrame<'src> {
    Right2(NodeRef<'src>),
    Right3(NodeRef<'src>, NodeRef<'src>),
    Left2(NodeRef<'src>),
    Left3(NodeRef<'src>, NodeRef<'src>),
}

fn absorb<'src>(frame: JoinFrame<'src>, pieces: Pieces<'src>) -> Option<Pieces<'src>> {
    Some(match (frame, pieces) {
        (JoinFrame::Right2(first), Pieces::One(last)) => Pieces::One(node2(first, last)?),
        (JoinFrame::Right2(first), Pieces::Two(middle, last)) => {
            Pieces::One(node3(first, middle, last)?)
        }
        (JoinFrame::Right3(first, middle), Pieces::One(last)) => {
            Pieces::One(node3(first, middle, last)?)
        }
        (JoinFrame::Right3(first, middle), Pieces::Two(next, last)) => {
            Pieces::Two(node2(first, middle)?, node2(next, last)?)
        }
        (JoinFrame::Left2(last), Pieces::One(first)) => Pieces::One(node2(first, last)?),
        (JoinFrame::Left2(last), Pieces::Two(first, middle)) => {
            Pieces::One(node3(first, middle, last)?)
        }
        (JoinFrame::Left3(middle, last), Pieces::One(first)) => {
            Pieces::One(node3(first, middle, last)?)
        }
        (JoinFrame::Left3(middle, last), Pieces::Two(first, next)) => {
            Pieces::Two(node2(first, next)?, node2(middle, last)?)
        }
    })
}

fn join<'src>(mut left: NodeRef<'src>, mut right: NodeRef<'src>) -> Option<Pieces<'src>> {
    left.len.checked_add(right.len)?;
    let mut frames = Vec::with_capacity(left.height.abs_diff(right.height).into());
    while left.height != right.height {
        if left.height > right.height {
            let next = match left.payload.as_ref()? {
                SourcePayload::Branch2([first, last]) => {
                    frames.push(JoinFrame::Right2(Arc::clone(first)));
                    Arc::clone(last)
                }
                SourcePayload::Branch3([first, middle, last]) => {
                    frames.push(JoinFrame::Right3(Arc::clone(first), Arc::clone(middle)));
                    Arc::clone(last)
                }
                _ => return None,
            };
            left = next;
        } else {
            let next = match right.payload.as_ref()? {
                SourcePayload::Branch2([first, last]) => {
                    frames.push(JoinFrame::Left2(Arc::clone(last)));
                    Arc::clone(first)
                }
                SourcePayload::Branch3([first, middle, last]) => {
                    frames.push(JoinFrame::Left3(Arc::clone(middle), Arc::clone(last)));
                    Arc::clone(first)
                }
                _ => return None,
            };
            right = next;
        }
    }
    let mut result = Pieces::Two(left, right);
    while let Some(frame) = frames.pop() {
        result = absorb(frame, result)?;
    }
    Some(result)
}

fn finish(pieces: Pieces<'_>) -> Option<NodeRef<'_>> {
    match pieces {
        Pieces::One(root) => Some(root),
        Pieces::Two(first, last) => node2(first, last),
    }
}

fn join_optional<'src>(left: Root<'src>, right: Root<'src>) -> Option<Root<'src>> {
    Some(match (left, right) {
        (None, value) | (value, None) => value,
        (Some(first), Some(last)) => Some(finish(join(first, last)?)?),
    })
}

enum SplitFrame<'src> {
    First2(NodeRef<'src>),
    Last2(NodeRef<'src>),
    First3(NodeRef<'src>, NodeRef<'src>),
    Middle3(NodeRef<'src>, NodeRef<'src>),
    Last3(NodeRef<'src>, NodeRef<'src>),
}

fn leaf_window<'src>(node: &SourceNode<'src>, range: Range<usize>) -> Option<NodeRef<'src>> {
    if range.start >= range.end || range.end > node.len {
        return None;
    }
    let len = range.end - range.start;
    let (payload, shape) = match node.payload.as_ref()? {
        SourcePayload::Borrowed(value) => {
            let selected = value.subleaf(range.start, range.end)?;
            let shape = selected.shape();
            (SourcePayload::Borrowed(selected), shape)
        }
        SourcePayload::Frozen(value) => (
            SourcePayload::Frozen(value.subview(range)?),
            verified_score_store::source_leaf_shape(len),
        ),
        _ => return None,
    };
    Some(Arc::new(SourceNode {
        shape,
        payload: Some(payload),
    }))
}

fn split_root<'src>(root: Root<'src>, mut cut: usize) -> Option<(Root<'src>, Root<'src>)> {
    let Some(mut node) = root else {
        return (cut == 0).then_some((None, None));
    };
    if cut > node.len {
        return None;
    }
    let mut frames = Vec::with_capacity(usize::from(node.height));
    let (mut before, mut after) = loop {
        if cut == 0 {
            break (None, Some(node));
        }
        if cut == node.len {
            break (Some(node), None);
        }
        let next = match node.payload.as_ref()? {
            SourcePayload::Branch2([first, last]) => {
                if cut <= first.len {
                    frames.push(SplitFrame::First2(Arc::clone(last)));
                    Arc::clone(first)
                } else {
                    cut -= first.len;
                    frames.push(SplitFrame::Last2(Arc::clone(first)));
                    Arc::clone(last)
                }
            }
            SourcePayload::Branch3([first, middle, last]) => {
                if cut <= first.len {
                    frames.push(SplitFrame::First3(Arc::clone(middle), Arc::clone(last)));
                    Arc::clone(first)
                } else if cut - first.len <= middle.len {
                    cut -= first.len;
                    frames.push(SplitFrame::Middle3(Arc::clone(first), Arc::clone(last)));
                    Arc::clone(middle)
                } else {
                    cut -= first.len + middle.len;
                    frames.push(SplitFrame::Last3(Arc::clone(first), Arc::clone(middle)));
                    Arc::clone(last)
                }
            }
            _ => {
                break (
                    Some(leaf_window(&node, 0..cut)?),
                    Some(leaf_window(&node, cut..node.len)?),
                );
            }
        };
        node = next;
    };
    while let Some(frame) = frames.pop() {
        match frame {
            SplitFrame::First2(last) => after = join_optional(after, Some(last))?,
            SplitFrame::Last2(first) => before = join_optional(Some(first), before)?,
            SplitFrame::First3(middle, last) => {
                after = Some(match after {
                    None => node2(middle, last)?,
                    Some(value) => finish(absorb(JoinFrame::Left2(last), join(value, middle)?)?)?,
                });
            }
            SplitFrame::Middle3(first, last) => {
                before = join_optional(Some(first), before)?;
                after = join_optional(after, Some(last))?;
            }
            SplitFrame::Last3(first, middle) => {
                before = Some(match before {
                    None => node2(first, middle)?,
                    Some(value) => finish(absorb(JoinFrame::Right2(first), join(middle, value)?)?)?,
                });
            }
        }
    }
    Some((before, after))
}
