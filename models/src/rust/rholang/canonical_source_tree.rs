use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use super::canonical_bytes_arena::{
    CanonicalByteProvenance, CanonicalLocatedByte, FrozenCanonicalByteCursor, FrozenCanonicalBytes,
    FrozenCanonicalLocatedByteCursor,
};

#[cfg(test)]
#[path = "../../../tests/support/canonical_source_tree.rs"]
pub(crate) mod tests;

const RELEASE_CAPACITY: usize = 1 + 2 * (usize::BITS as usize - 1);

type NodeRef<'src> = Arc<SourceNode<'src>>;
type Root<'src> = Option<NodeRef<'src>>;

struct SourceNode<'src> {
    len: usize,
    height: u8,
    payload: Option<SourcePayload<'src>>,
}

enum SourcePayload<'src> {
    Borrowed {
        bytes: &'src [u8],
        start: usize,
        len: usize,
        source: u32,
        absolute_start: usize,
    },
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
    ) -> Option<Self> {
        let len = range.end.checked_sub(range.start)?;
        bytes.get(range.clone())?;
        absolute_start.checked_add(range.start)?;
        if len != 0 {
            absolute_start.checked_add(range.end - 1)?;
        }
        Some(Self::Borrowed {
            bytes,
            start: range.start,
            len,
            source,
            absolute_start,
        })
    }
}

impl Drop for SourceNode<'_> {
    fn drop(&mut self) {
        match self.payload.take() {
            None | Some(SourcePayload::Borrowed { .. }) => {}
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
            None | Some(SourcePayload::Borrowed { .. }) => {}
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
        let payload = SourcePayload::borrowed(bytes, range.clone(), source, absolute_start)?;
        let len = range.end - range.start;
        if len == 0 {
            return Some(Self::empty());
        }
        Some(Self::from_root(Some(Arc::new(SourceNode {
            len,
            height: 0,
            payload: Some(payload),
        }))))
    }

    pub(crate) fn from_frozen(value: FrozenCanonicalBytes) -> Self {
        if value.is_empty() {
            return Self::empty();
        }
        Self::from_root(Some(Arc::new(SourceNode {
            len: value.len(),
            height: 0,
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
        let payload = match (
            self.root.as_ref()?.payload.as_ref()?,
            right.root.as_ref()?.payload.as_ref()?,
        ) {
            (
                SourcePayload::Borrowed {
                    bytes,
                    start,
                    source,
                    absolute_start,
                    ..
                },
                SourcePayload::Borrowed {
                    bytes: right_bytes,
                    start: right_start,
                    source: right_source,
                    absolute_start: right_absolute_start,
                    ..
                },
            ) => {
                if !std::ptr::eq(*bytes, *right_bytes)
                    || source != right_source
                    || absolute_start != right_absolute_start
                {
                    return None;
                }
                let start = start.checked_add(self.start)?;
                if start.checked_add(self.len)? != right_start.checked_add(right.start)? {
                    return None;
                }
                if len == 0 {
                    return Some(self.clone());
                }
                SourcePayload::borrowed(
                    bytes,
                    start..start.checked_add(len)?,
                    *source,
                    *absolute_start,
                )?
            }
            (SourcePayload::Frozen(left_value), SourcePayload::Frozen(right_value)) => {
                let left_value =
                    left_value.subview(self.start..self.start.checked_add(self.len)?)?;
                let right_value =
                    right_value.subview(right.start..right.start.checked_add(right.len)?)?;
                SourcePayload::Frozen(left_value.try_coalesce(&right_value)?)
            }
            _ => return None,
        };
        if len == 0 {
            return Some(self.clone());
        }
        Some(Self::from_root(Some(Arc::new(SourceNode {
            len,
            height: 0,
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
                    if at < children[0].len {
                        node = &children[0];
                    } else {
                        at -= children[0].len;
                        node = &children[1];
                    }
                }
                SourcePayload::Branch3(children) => {
                    if at < children[0].len {
                        node = &children[0];
                    } else {
                        at -= children[0].len;
                        if at < children[1].len {
                            node = &children[1];
                        } else {
                            at -= children[1].len;
                            node = &children[2];
                        }
                    }
                }
                leaf => return Some((leaf, at)),
            }
        }
    }

    pub(crate) fn byte_at(&self, offset: usize) -> Option<u8> {
        let (leaf, at) = self.leaf_at(offset)?;
        match leaf {
            SourcePayload::Borrowed { bytes, start, .. } => {
                bytes.get(start.checked_add(at)?).copied()
            }
            SourcePayload::Frozen(value) => value.byte_at(at),
            _ => None,
        }
    }

    pub(crate) fn provenance_at(&self, offset: usize) -> Option<CanonicalByteProvenance> {
        let (leaf, at) = self.leaf_at(offset)?;
        match leaf {
            SourcePayload::Borrowed {
                start,
                source,
                absolute_start,
                ..
            } => Some(CanonicalByteProvenance::new(
                *source,
                absolute_start.checked_add(start.checked_add(at)?)?,
            )),
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
            SourcePayload::Borrowed {
                bytes, start, len, ..
            } if end <= *len => bytes.get(start.checked_add(at)?..start.checked_add(end)?),
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
            SourcePayload::Borrowed {
                bytes,
                start,
                source,
                absolute_start,
                ..
            } => {
                let selected = start.checked_add(range.start)?..start.checked_add(range.end)?;
                Some(Self::Borrowed {
                    bytes: bytes.get(selected.clone())?,
                    source: *source,
                    absolute_start: absolute_start.checked_add(selected.start)?,
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
    if left.height != right.height {
        return None;
    }
    Some(Arc::new(SourceNode {
        len: left.len.checked_add(right.len)?,
        height: left.height.checked_add(1)?,
        payload: Some(SourcePayload::Branch2([left, right])),
    }))
}

fn node3<'src>(
    first: NodeRef<'src>,
    middle: NodeRef<'src>,
    last: NodeRef<'src>,
) -> Option<NodeRef<'src>> {
    if first.height != middle.height || first.height != last.height {
        return None;
    }
    Some(Arc::new(SourceNode {
        len: first.len.checked_add(middle.len)?.checked_add(last.len)?,
        height: first.height.checked_add(1)?,
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
    let payload = match node.payload.as_ref()? {
        SourcePayload::Borrowed {
            bytes,
            start,
            source,
            absolute_start,
            ..
        } => {
            let start = start.checked_add(range.start)?;
            SourcePayload::borrowed(
                bytes,
                start..start.checked_add(len)?,
                *source,
                *absolute_start,
            )?
        }
        SourcePayload::Frozen(value) => SourcePayload::Frozen(value.subview(range)?),
        _ => return None,
    };
    Some(Arc::new(SourceNode {
        len,
        height: 0,
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
