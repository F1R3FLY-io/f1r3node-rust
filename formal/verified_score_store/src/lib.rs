use vstd::prelude::*;
pub mod admission_observable_spec;
pub mod admission_event_checker;

verus! {

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScoreHandle(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotAdmissionError {
    OutOfRange,
    Decreasing,
    Duplicate,
}

pub struct VerifiedMessageSlots {
    last_slot: Option<usize>,
    seen: Vec<bool>,
}

pub struct MessageSlotsImage {
    pub last_slot: Option<usize>,
    pub ghost seen: Seq<bool>,
}

impl VerifiedMessageSlots {
    pub closed spec fn image(&self) -> MessageSlotsImage {
        MessageSlotsImage {
            last_slot: self.last_slot,
            seen: self.seen@,
        }
    }

    pub open spec fn well_formed(&self) -> bool {
        match self.image().last_slot {
            Some(slot) => slot < self.image().seen.len(),
            None => true,
        }
    }

    pub fn new(slot_count: usize) -> (slots: Self)
        ensures
            slots.well_formed(),
            slots.image().last_slot.is_none(),
            slots.image().seen.len() == slot_count,
            forall|i: int| 0 <= i < slot_count ==> !slots.image().seen[i],
    {
        let mut seen: Vec<bool> = Vec::with_capacity(slot_count);
        while seen.len() < slot_count
            invariant
                seen.len() <= slot_count,
                forall|i: int| 0 <= i < seen.len() ==> !seen@[i],
            decreases slot_count - seen.len(),
        {
            seen.push(false);
        }
        Self { last_slot: None, seen }
    }

    pub fn len(&self) -> (length: usize)
        ensures length == self.image().seen.len()
    {
        self.seen.len()
    }

    pub fn is_empty(&self) -> (empty: bool)
        ensures empty == (self.image().seen.len() == 0)
    {
        self.seen.is_empty()
    }

    pub fn last_slot(&self) -> (slot: Option<usize>)
        ensures slot == self.image().last_slot
    {
        self.last_slot
    }

    pub fn seen_at(&self, slot: usize) -> (seen: Option<bool>)
        ensures
            seen == if slot < self.image().seen.len() {
                Some(self.image().seen[slot as int])
            } else {
                None
            }
    {
        if slot < self.seen.len() {
            Some(self.seen[slot])
        } else {
            None
        }
    }

    pub fn seen_slice(&self) -> (seen: &[bool])
        ensures seen@ == self.image().seen
    {
        self.seen.as_slice()
    }

    pub fn admit(&mut self, slot: usize, repeated: bool) -> (result: Result<(), SlotAdmissionError>)
        requires self.well_formed()
        ensures
            final(self).well_formed(),
            match result {
                Ok(()) => {
                    &&& slot < old(self).image().seen.len()
                    &&& match old(self).image().last_slot {
                        Some(last) => last <= slot,
                        None => true,
                    }
                    &&& (repeated || !old(self).image().seen[slot as int])
                    &&& final(self).image().last_slot == Some(slot)
                    &&& final(self).image().seen
                        == old(self).image().seen.update(slot as int, true)
                },
                Err(SlotAdmissionError::OutOfRange) => {
                    &&& slot >= old(self).image().seen.len()
                    &&& final(self).image() == old(self).image()
                },
                Err(SlotAdmissionError::Decreasing) => {
                    &&& slot < old(self).image().seen.len()
                    &&& match old(self).image().last_slot {
                        Some(last) => slot < last,
                        None => false,
                    }
                    &&& final(self).image() == old(self).image()
                },
                Err(SlotAdmissionError::Duplicate) => {
                    &&& slot < old(self).image().seen.len()
                    &&& match old(self).image().last_slot {
                        Some(last) => last <= slot,
                        None => true,
                    }
                    &&& !repeated
                    &&& old(self).image().seen[slot as int]
                    &&& final(self).image() == old(self).image()
                },
            }
    {
        if slot >= self.seen.len() {
            return Err(SlotAdmissionError::OutOfRange);
        }
        if let Some(last) = self.last_slot {
            if slot < last {
                return Err(SlotAdmissionError::Decreasing);
            }
        }
        if !repeated && self.seen[slot] {
            return Err(SlotAdmissionError::Duplicate);
        }
        self.seen.set(slot, true);
        self.last_slot = Some(slot);
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct BorrowedIndexedWindow<'a> {
    bytes: &'a [u8],
    start: usize,
    len: usize,
    source: u32,
    absolute_start: usize,
    end_source: Option<u32>,
    end_offset: usize,
}

pub struct BorrowedIndexedWindowImage {
    pub ghost bytes: Seq<u8>,
    pub start: usize,
    pub len: usize,
    pub source: u32,
    pub absolute_start: usize,
    pub end_source: Option<u32>,
    pub end_offset: usize,
}

impl<'a> BorrowedIndexedWindow<'a> {
    pub closed spec fn image(&self) -> BorrowedIndexedWindowImage {
        BorrowedIndexedWindowImage {
            bytes: self.bytes@,
            start: self.start,
            len: self.len,
            source: self.source,
            absolute_start: self.absolute_start,
            end_source: self.end_source,
            end_offset: self.end_offset,
        }
    }
}

impl<'a> View for BorrowedIndexedWindow<'a> {
    type V = Seq<u8>;

    closed spec fn view(&self) -> Self::V {
        self.image().bytes.subrange(
            self.image().start as int,
            self.image().start as int + self.image().len as int,
        )
    }
}

impl<'a> BorrowedIndexedWindow<'a> {
    pub open spec fn well_formed(&self) -> bool {
        self.image().start as int + self.image().len as int <= self.image().bytes.len()
    }

    pub open spec fn position_image(&self, index: usize) -> Option<(u32, usize)> {
        if index > self.image().len {
            None
        } else if index == self.image().len && self.image().end_source.is_some() {
            Some((self.image().end_source.unwrap(), self.image().end_offset))
        } else if self.image().absolute_start as int + self.image().start as int + index as int > usize::MAX as int {
            None
        } else {
            Some((self.image().source, (self.image().absolute_start as int + self.image().start as int + index as int) as usize))
        }
    }

    proof fn borrowed_bounds(&self)
        requires self.well_formed()
        ensures self.start as int + self.len as int <= self.bytes.len() as int
    {
        reveal(BorrowedIndexedWindow::image);
        assert(self.image().bytes == self.bytes@);
        assert(self.image().start == self.start);
        assert(self.image().len == self.len);
        assert(self.bytes@.len() == self.bytes.len());
        assert(self.start as int + self.len as int <= self.bytes@.len());
    }

    pub fn new(bytes: &'a [u8], source: u32, absolute_start: usize) -> (window: Self)
        ensures window.well_formed(), window@ == bytes@
    {
        Self { bytes, start: 0, len: bytes.len(), source, absolute_start, end_source: None, end_offset: 0 }
    }

    pub fn root_slice(&self) -> (bytes: &'a [u8])
        ensures bytes@ == self.image().bytes
    {
        self.bytes
    }

    pub fn start(&self) -> (start: usize)
        ensures start == self.image().start
    {
        self.start
    }

    pub fn len(&self) -> (len: usize)
        ensures len == self.image().len, self.well_formed() ==> len == self@.len()
    {
        self.len
    }

    pub fn is_empty(&self) -> (empty: bool)
        ensures empty == (self.image().len == 0), self.well_formed() ==> empty == (self@.len() == 0)
    {
        self.len == 0
    }

    pub fn source(&self) -> (source: u32)
        ensures source == self.image().source
    {
        self.source
    }

    pub fn absolute_start(&self) -> (absolute_start: usize)
        ensures absolute_start == self.image().absolute_start
    {
        self.absolute_start
    }

    pub fn end_source(&self) -> (end_source: Option<u32>)
        ensures end_source == self.image().end_source
    {
        self.end_source
    }

    pub fn end_offset(&self) -> (end_offset: usize)
        ensures end_offset == self.image().end_offset
    {
        self.end_offset
    }

    pub fn subview(&self, start: usize, end: usize) -> (view: Option<Self>)
        requires self.well_formed()
        ensures
            match view {
                Some(view) => {
                    &&& start <= end && end <= self.image().len
                    &&& view.well_formed()
                    &&& view@ == self@.subrange(start as int, end as int)
                    &&& view.image().source == self.image().source
                    &&& view.image().absolute_start == self.image().absolute_start
                    &&& view.image().end_source == if end == self.image().len { self.image().end_source } else { None }
                    &&& view.image().end_offset == if end == self.image().len { self.image().end_offset } else { 0 }
                },
                None => start > end || end > self.image().len,
            }
    {
        if start > end || end > self.len {
            return None;
        }
        proof {
            self.borrowed_bounds();
            assert(self.start as int + start as int <= self.bytes.len() as int);
            assert(self.start as int + start as int <= usize::MAX as int);
        }
        let offset = self.start + start;
        let length = end - start;
        Some(Self {
            bytes: self.bytes,
            start: offset,
            len: length,
            source: self.source,
            absolute_start: self.absolute_start,
            end_source: if end == self.len { self.end_source } else { None },
            end_offset: if end == self.len { self.end_offset } else { 0 },
        })
    }

    pub fn with_end(&self, source: u32, offset: usize) -> (view: Self)
        ensures
            view@ == self@,
            view.image().source == self.image().source,
            view.image().absolute_start == self.image().absolute_start,
            view.image().end_source == Some(source),
            view.image().end_offset == offset,
    {
        Self {
            bytes: self.bytes,
            start: self.start,
            len: self.len,
            source: self.source,
            absolute_start: self.absolute_start,
            end_source: Some(source),
            end_offset: offset,
        }
    }

    pub fn rewindow(&self, end: usize, end_source: Option<u32>, end_offset: usize) -> (view: Option<Self>)
        requires self.well_formed()
        ensures
            match view {
                Some(view) => {
                    &&& self.image().start <= end && end as int <= self.image().bytes.len()
                    &&& view.well_formed()
                    &&& view@ == self.image().bytes.subrange(self.image().start as int, end as int)
                    &&& view.image().end_source == end_source
                    &&& view.image().end_offset == end_offset
                    &&& view.image().source == self.image().source
                    &&& view.image().absolute_start == self.image().absolute_start
                },
                None => end < self.image().start || end as int > self.image().bytes.len(),
            }
    {
        if end < self.start || end > self.bytes.len() {
            return None;
        }
        Some(Self {
            bytes: self.bytes,
            start: self.start,
            len: end - self.start,
            source: self.source,
            absolute_start: self.absolute_start,
            end_source,
            end_offset,
        })
    }

    pub fn as_slice(&self) -> (bytes: &'a [u8])
        requires self.well_formed()
        ensures bytes@ == self@
    {
        proof { self.borrowed_bounds(); }
        let end = self.start + self.len;
        vstd::slice::slice_subrange(self.bytes, self.start, end)
    }

    pub fn byte_at(&self, index: usize) -> (byte: Option<u8>)
        requires self.well_formed()
        ensures byte == if index < self.image().len { Some(self@[index as int]) } else { None }
    {
        if index >= self.len {
            None
        } else {
            proof {
                self.borrowed_bounds();
                assert((self.start as int + index as int) < (self.bytes.len() as int));
                assert(self.start as int + index as int <= usize::MAX as int);
            }
            Some(self.bytes[self.start + index])
        }
    }

    pub fn position_at(&self, index: usize) -> (position: Option<(u32, usize)>)
        requires self.well_formed()
        ensures position == self.position_image(index)
    {
        if index > self.len {
            return None;
        }
        if index == self.len {
            if let Some(source) = self.end_source {
                return Some((source, self.end_offset));
            }
        }
        let offset = self.start.checked_add(index)?;
        Some((self.source, self.absolute_start.checked_add(offset)?))
    }
}

/// A single frozen leaf uses the same page slot for its byte and provenance.
/// The slices are the retained page payloads; this image does not flatten them.
pub open spec fn frozen_single_leaf_slot_image<P>(
    bytes: Seq<u8>, provenance: Seq<P>, page_start: usize, data_len: usize,
    start: usize, len: usize, offset: usize,
) -> Option<usize> {
    if data_len == 0
        || start as int + len as int > data_len as int
        || page_start as int + data_len as int > bytes.len()
        || page_start as int + data_len as int > provenance.len()
        || offset >= len
    {
        None
    } else {
        Some((page_start as int + start as int + offset as int) as usize)
    }
}

pub fn frozen_single_leaf_slot<P>(
    bytes: &[u8], provenance: &[P], page_start: usize, data_len: usize,
    start: usize, len: usize, offset: usize,
) -> (slot: Option<usize>)
    ensures slot == frozen_single_leaf_slot_image(
        bytes@, provenance@, page_start, data_len, start, len, offset,
    )
{
    proof {
        assert(bytes@.len() == bytes.len());
        assert(provenance@.len() == provenance.len());
    }
    if data_len == 0 || offset >= len {
        return None;
    }
    let page_end = match page_start.checked_add(data_len) {
        Some(end) => end,
        None => {
            proof {
                assert(page_start as int + data_len as int > usize::MAX as int);
                assert(page_start as int + data_len as int > bytes@.len());
            }
            return None;
        },
    };
    let view_end = match start.checked_add(len) {
        Some(end) => end,
        None => {
            proof {
                assert(start as int + len as int > usize::MAX as int);
                assert(start as int + len as int > data_len as int);
            }
            return None;
        },
    };
    proof {
        assert(page_end as int == page_start as int + data_len as int);
        assert(view_end as int == start as int + len as int);
    }
    if page_end > bytes.len() || page_end > provenance.len() || view_end > data_len {
        return None;
    }
    proof {
        assert((start as int + offset as int) < data_len as int);
        assert(start as int + offset as int <= usize::MAX as int);
        assert((page_start as int + start as int + offset as int)
            < page_start as int + data_len as int);
        assert(page_start as int + start as int + offset as int <= usize::MAX as int);
    }
    let absolute = start.checked_add(offset)?;
    let slot = page_start.checked_add(absolute)?;
    proof {
        assert(absolute as int == start as int + offset as int);
        assert(slot as int == page_start as int + start as int + offset as int);
    }
    Some(slot)
}

/// Return both actual page entries at the verified common slot. No payload is copied.
pub fn frozen_single_leaf_at<'a, P>(
    bytes: &'a [u8], provenance: &'a [P], page_start: usize, data_len: usize,
    start: usize, len: usize, offset: usize,
) -> (entry: Option<(u8, &'a P)>)
    ensures
        entry.is_some() == frozen_single_leaf_slot_image(
            bytes@, provenance@, page_start, data_len, start, len, offset,
        ).is_some(),
        entry.is_some() ==> {
            let slot = frozen_single_leaf_slot_image(
                bytes@, provenance@, page_start, data_len, start, len, offset,
            ).unwrap();
            &&& entry.unwrap().0 == bytes@[slot as int]
            &&& *entry.unwrap().1 == provenance@[slot as int]
        },
{
    let slot = frozen_single_leaf_slot(
        bytes, provenance, page_start, data_len, start, len, offset,
    )?;
    proof {
        assert(slot < bytes.len());
        assert(slot < provenance.len());
    }
    Some((bytes[slot], &provenance[slot]))
}

#[derive(Clone, Copy)]
pub struct FrozenOneLeafView {
    page_start: usize,
    data_len: usize,
    start: usize,
    len: usize,
}

pub struct FrozenOneLeafViewImage {
    pub page_start: usize,
    pub data_len: usize,
    pub start: usize,
    pub len: usize,
}

impl FrozenOneLeafView {
    pub closed spec fn image(&self) -> FrozenOneLeafViewImage {
        FrozenOneLeafViewImage {
            page_start: self.page_start,
            data_len: self.data_len,
            start: self.start,
            len: self.len,
        }
    }

    pub open spec fn well_formed(&self) -> bool {
        let image = self.image();
        &&& image.data_len > 0
        &&& image.start as int + image.len as int <= image.data_len as int
        &&& image.page_start as int + image.data_len as int <= usize::MAX as int
    }

    pub fn new<P>(
        bytes: &[u8], origins: &[P], page_start: usize, used: usize,
    ) -> (view: Option<Self>)
        ensures
            view.is_some() == frozen_single_leaf_slot_image(
                bytes@, origins@, page_start, used, 0, used, 0,
            ).is_some(),
            view.is_some() ==> {
                let selected = view.unwrap();
                &&& selected.well_formed()
                &&& selected.image().page_start == page_start
                &&& selected.image().data_len == used
                &&& selected.image().start == 0
                &&& selected.image().len == used
            },
    {
        if used == 0 {
            return None;
        }
        let Some(page_end) = page_start.checked_add(used) else {
            proof {
                assert(page_start as int + used as int > usize::MAX as int);
                assert(bytes@.len() == bytes.len());
                assert(origins@.len() == origins.len());
                assert(page_start as int + used as int > bytes@.len());
                assert(page_start as int + used as int > origins@.len());
            }
            return None;
        };
        if page_end > bytes.len() || page_end > origins.len() {
            return None;
        }
        let view = Self { page_start, data_len: used, start: 0, len: used };
        proof {
            reveal(FrozenOneLeafView::image);
        }
        Some(view)
    }

    pub fn page_start(&self) -> (page_start: usize)
        ensures page_start == self.image().page_start,
    {
        proof { reveal(FrozenOneLeafView::image); }
        self.page_start
    }

    pub fn data_len(&self) -> (data_len: usize)
        ensures data_len == self.image().data_len,
    {
        proof { reveal(FrozenOneLeafView::image); }
        self.data_len
    }

    pub fn start(&self) -> (start: usize)
        ensures start == self.image().start,
    {
        proof { reveal(FrozenOneLeafView::image); }
        self.start
    }

    pub fn len(&self) -> (len: usize)
        ensures len == self.image().len,
    {
        proof { reveal(FrozenOneLeafView::image); }
        self.len
    }

    pub fn is_empty(&self) -> (empty: bool)
        ensures empty == (self.image().len == 0),
    {
        proof { reveal(FrozenOneLeafView::image); }
        self.len == 0
    }

    pub fn subview(&self, from: usize, to: usize) -> (view: Option<Self>)
        requires self.well_formed(),
        ensures
            view.is_some() == (from <= to && to <= self.image().len),
            view.is_some() ==> {
                let selected = view.unwrap();
                &&& selected.well_formed()
                &&& selected.image().page_start == self.image().page_start
                &&& selected.image().data_len == self.image().data_len
                &&& selected.image().start as int == self.image().start as int + from as int
                &&& selected.image().len as int == to as int - from as int
            },
    {
        if from > to || to > self.len {
            return None;
        }
        proof {
            reveal(FrozenOneLeafView::image);
            assert(self.start as int + to as int <= self.data_len as int);
            assert(self.start as int + from as int <= usize::MAX as int);
        }
        Some(Self {
            page_start: self.page_start,
            data_len: self.data_len,
            start: self.start + from,
            len: to - from,
        })
    }

    pub fn join_right(&self, right: &Self) -> (view: Option<Self>)
        requires self.well_formed(), right.well_formed(),
        ensures
            view.is_some() == (
                self.image().page_start == right.image().page_start
                && self.image().data_len == right.image().data_len
                && self.image().start as int + self.image().len as int
                    == right.image().start as int
            ),
            view.is_some() ==> {
                let joined = view.unwrap();
                &&& joined.well_formed()
                &&& joined.image().page_start == self.image().page_start
                &&& joined.image().data_len == self.image().data_len
                &&& joined.image().start == self.image().start
                &&& joined.image().len as int == self.image().len as int
                    + right.image().len as int
            },
    {
        if self.page_start != right.page_start || self.data_len != right.data_len {
            return None;
        }
        proof {
            reveal(FrozenOneLeafView::image);
            assert(self.start as int + self.len as int <= self.data_len as int);
        }
        if self.start + self.len != right.start {
            return None;
        }
        proof {
            assert(self.len as int + right.len as int <= self.data_len as int);
            assert(self.len as int + right.len as int <= usize::MAX as int);
        }
        Some(Self {
            page_start: self.page_start,
            data_len: self.data_len,
            start: self.start,
            len: self.len + right.len,
        })
    }

    pub fn bind<'a, P>(
        &self, bytes: &'a [u8], origins: &'a [P],
    ) -> (read: Option<FrozenOneLeafRead<'a, P>>)
        requires self.well_formed(),
        ensures
            read.is_some() == (
                self.image().page_start as int + self.image().data_len as int <= bytes@.len()
                && self.image().page_start as int + self.image().data_len as int
                    <= origins@.len()
            ),
            read.is_some() ==> {
                let selected = read.unwrap();
                &&& selected.well_formed()
                &&& selected.image().bytes == bytes@
                &&& selected.image().origins == origins@
                &&& selected.image().page_start == self.image().page_start
                &&& selected.image().data_len == self.image().data_len
                &&& selected.image().start == self.image().start
                &&& selected.image().len == self.image().len
            },
    {
        proof {
            reveal(FrozenOneLeafView::image);
        }
        let page_end = self.page_start + self.data_len;
        if page_end > bytes.len() || page_end > origins.len() {
            return None;
        }
        Some(FrozenOneLeafRead { bytes, origins, view: *self })
    }
}

pub struct FrozenOneLeafRead<'a, P> {
    bytes: &'a [u8],
    origins: &'a [P],
    view: FrozenOneLeafView,
}

pub struct FrozenOneLeafReadImage<P> {
    pub ghost bytes: Seq<u8>,
    pub ghost origins: Seq<P>,
    pub page_start: usize,
    pub data_len: usize,
    pub start: usize,
    pub len: usize,
}

impl<'a, P> FrozenOneLeafRead<'a, P> {
    pub closed spec fn image(&self) -> FrozenOneLeafReadImage<P> {
        FrozenOneLeafReadImage {
            bytes: self.bytes@,
            origins: self.origins@,
            page_start: self.view.page_start,
            data_len: self.view.data_len,
            start: self.view.start,
            len: self.view.len,
        }
    }

    pub open spec fn well_formed(&self) -> bool {
        let image = self.image();
        &&& image.data_len > 0
        &&& image.start as int + image.len as int <= image.data_len as int
        &&& image.page_start as int + image.data_len as int <= image.bytes.len()
        &&& image.page_start as int + image.data_len as int <= image.origins.len()
    }

    pub fn selected_slice(&self, from: usize, to: usize) -> (selected: Option<&'a [u8]>)
        requires self.well_formed(),
        ensures
            selected.is_some() == (from <= to && to <= self.image().len),
            selected.is_some() ==> {
                let bytes = selected.unwrap();
                bytes@ == self.image().bytes.subrange(
                    self.image().page_start as int + self.image().start as int + from as int,
                    self.image().page_start as int + self.image().start as int + to as int,
                )
            },
    {
        proof {
            reveal(FrozenOneLeafRead::image);
        }
        if from > to || to > self.view.len {
            return None;
        }
        proof {
            assert(self.bytes@.len() == self.bytes.len());
            assert(self.view.page_start as int + self.view.start as int + to as int
                <= self.bytes.len() as int);
        }
        let base = self.view.page_start + self.view.start;
        let begin = base + from;
        let end = base + to;
        Some(vstd::slice::slice_subrange(self.bytes, begin, end))
    }

    pub fn at(&self, offset: usize) -> (entry: Option<(u8, &'a P)>)
        requires self.well_formed(),
        ensures
            entry.is_some() == (offset < self.image().len),
            entry.is_some() ==> {
                let slot = self.image().page_start as int + self.image().start as int
                    + offset as int;
                &&& entry.unwrap().0 == self.image().bytes[slot]
                &&& *entry.unwrap().1 == self.image().origins[slot]
            },
    {
        proof {
            reveal(FrozenOneLeafRead::image);
        }
        let entry = frozen_single_leaf_at(
            self.bytes, self.origins, self.view.page_start, self.view.data_len,
            self.view.start, self.view.len, offset,
        );
        proof {
            if offset < self.view.len {
                let slot = self.view.page_start as int + self.view.start as int
                    + offset as int;
                assert(self.bytes@.len() == self.bytes.len());
                assert(self.origins@.len() == self.origins.len());
                assert(0 <= slot);
                assert(slot < self.bytes@.len());
                assert(slot < self.origins@.len());
                assert(slot <= usize::MAX as int);
                assert(frozen_single_leaf_slot_image(
                    self.bytes@, self.origins@, self.view.page_start,
                    self.view.data_len, self.view.start, self.view.len, offset,
                ) == Some(slot as usize));
                assert(entry.unwrap().0 == self.bytes@[slot]);
                assert(*entry.unwrap().1 == self.origins@[slot]);
            }
        }
        entry
    }

    pub fn step(&self, offset: usize) -> (step: Option<(u8, &'a P, usize)>)
        requires self.well_formed(),
        ensures
            step.is_some() == (offset < self.image().len),
            step.is_some() ==> {
                let selected = step.unwrap();
                let slot = self.image().page_start as int + self.image().start as int
                    + offset as int;
                &&& selected.0 == self.image().bytes[slot]
                &&& *selected.1 == self.image().origins[slot]
                &&& selected.2 as int == offset as int + 1
                &&& selected.2 <= self.image().len
            },
    {
        let (byte, origin) = self.at(offset)?;
        proof {
            reveal(FrozenOneLeafRead::image);
            assert(offset as int + 1 <= self.view.len as int);
            assert(offset as int + 1 <= usize::MAX as int);
        }
        Some((byte, origin, offset + 1))
    }
}

/// Checked lengths and routes for the fixed-arity nodes of a retained source tree.
/// The route images use child lengths only; neither operation reads or copies a leaf.
pub open spec fn source_branch2_len_image(left: usize, right: usize) -> Option<usize> {
    if left as int + right as int > usize::MAX as int {
        None
    } else {
        Some((left as int + right as int) as usize)
    }
}

pub open spec fn source_branch3_len_image(first: usize, middle: usize, last: usize) -> Option<usize> {
    match source_branch2_len_image(first, middle) {
        Some(prefix) => source_branch2_len_image(prefix, last),
        None => None,
    }
}

pub fn source_branch2_len(left: usize, right: usize) -> (len: Option<usize>)
    ensures len == source_branch2_len_image(left, right)
{
    left.checked_add(right)
}

pub fn source_branch3_len(first: usize, middle: usize, last: usize) -> (len: Option<usize>)
    ensures len == source_branch3_len_image(first, middle, last)
{
    let prefix = source_branch2_len(first, middle)?;
    source_branch2_len(prefix, last)
}

pub open spec fn source_branch2_route_image(left: usize, at: usize) -> (usize, usize) {
    if at < left { (0, at) } else { (1, (at as int - left as int) as usize) }
}

pub open spec fn source_branch3_route_image(first: usize, middle: usize, at: usize) -> (usize, usize) {
    if at < first {
        (0, at)
    } else if at - first < middle {
        (1, (at as int - first as int) as usize)
    } else {
        (2, (at as int - first as int - middle as int) as usize)
    }
}

/// Routes every offset exactly as the production branch accessor does, including
/// offsets outside a malformed branch. Valid checked branches additionally locate
/// a byte within the selected child.
pub fn source_branch2_route(
    left: usize,
    #[cfg_attr(
        not(verus_keep_ghost),
        expect(
            unused_variables,
            reason = "read only by the ensures clause, which verus! erases from rustc builds"
        )
    )]
    right: usize,
    at: usize,
) -> (route: (usize, usize))
    ensures
        route == source_branch2_route_image(left, at),
        source_branch2_len_image(left, right).is_some()
            && at < source_branch2_len_image(left, right).unwrap()
            ==> (if route.0 == 0 { route.1 < left } else { route.0 == 1 && route.1 < right }),
{
    if at < left { (0, at) } else { (1, at - left) }
}

pub fn source_branch3_route(
    first: usize,
    middle: usize,
    #[cfg_attr(
        not(verus_keep_ghost),
        expect(
            unused_variables,
            reason = "read only by the ensures clause, which verus! erases from rustc builds"
        )
    )]
    last: usize,
    at: usize,
) -> (route: (usize, usize))
    ensures
        route == source_branch3_route_image(first, middle, at),
        source_branch3_len_image(first, middle, last).is_some()
            && at < source_branch3_len_image(first, middle, last).unwrap()
            ==> (if route.0 == 0 {
                route.1 < first
            } else if route.0 == 1 {
                route.1 < middle
            } else {
                route.0 == 2 && route.1 < last
            }),
{
    if at < first {
        (0, at)
    } else {
        let after_first = at - first;
        if after_first < middle {
            (1, after_first)
        } else {
            (2, after_first - middle)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceNodeShape {
    pub len: usize,
    pub height: u8,
}

pub open spec fn source_borrowed_located_image(
    bytes: Seq<u8>, start: usize, len: usize, source: u32,
    absolute_start: usize, at: usize,
) -> Option<(u8, u32, usize)> {
    if at >= len || start as int + at as int >= bytes.len()
        || absolute_start as int + start as int + at as int > usize::MAX as int
    {
        None
    } else {
        Some((bytes[start as int + at as int], source,
            (absolute_start as int + start as int + at as int) as usize))
    }
}

pub fn source_borrowed_located_at(
    bytes: &[u8], start: usize, len: usize, source: u32,
    absolute_start: usize, at: usize,
) -> (located: Option<(u8, u32, usize)>)
    ensures located == source_borrowed_located_image(
        bytes@, start, len, source, absolute_start, at,
    )
{
    if at >= len {
        return None;
    }
    let slot = start.checked_add(at)?;
    let byte = bytes.get(slot)?;
    let offset = absolute_start.checked_add(slot)?;
    Some((*byte, source, offset))
}

pub open spec fn source_borrowed_leaf_shape_image(
    bytes: Seq<u8>, start: usize, end: usize, absolute_start: usize,
) -> Option<SourceNodeShape> {
    if start > end || end as int > bytes.len()
        || absolute_start as int + start as int > usize::MAX as int
        || (start < end
            && absolute_start as int + end as int - 1 > usize::MAX as int)
    {
        None
    } else {
        Some(SourceNodeShape { len: (end as int - start as int) as usize, height: 0 })
    }
}

pub fn source_borrowed_leaf_shape(
    bytes: &[u8], start: usize, end: usize, absolute_start: usize,
) -> (shape: Option<SourceNodeShape>)
    ensures shape == source_borrowed_leaf_shape_image(bytes@, start, end, absolute_start)
{
    if start > end || end > bytes.len() {
        return None;
    }
    absolute_start.checked_add(start)?;
    if start < end && absolute_start.checked_add(end - 1).is_none() {
        return None;
    }
    Some(source_leaf_shape(end - start))
}

#[derive(Clone, Copy)]
pub struct SourceBorrowedLeaf<'a> {
    bytes: &'a [u8],
    start: usize,
    len: usize,
    source: u32,
    absolute_start: usize,
}

pub struct SourceBorrowedLeafImage {
    pub ghost bytes: Seq<u8>,
    pub start: usize,
    pub len: usize,
    pub source: u32,
    pub absolute_start: usize,
}

impl<'a> SourceBorrowedLeaf<'a> {
    pub closed spec fn image(&self) -> SourceBorrowedLeafImage {
        SourceBorrowedLeafImage {
            bytes: self.bytes@,
            start: self.start,
            len: self.len,
            source: self.source,
            absolute_start: self.absolute_start,
        }
    }

    pub open spec fn well_formed(&self) -> bool {
        let image = self.image();
        &&& image.start as int + image.len as int <= image.bytes.len()
        &&& image.absolute_start as int + image.start as int <= usize::MAX as int
        &&& (image.len == 0 || image.absolute_start as int + image.start as int
            + image.len as int - 1 <= usize::MAX as int)
    }

    pub open spec fn selection_valid(&self, from: usize, to: usize) -> bool {
        let image = self.image();
        &&& from <= to
        &&& to <= image.len
        &&& image.absolute_start as int + image.start as int + from as int
            <= usize::MAX as int
    }

    pub fn new(
        bytes: &'a [u8], start: usize, end: usize, source: u32, absolute_start: usize,
    ) -> (leaf: Option<Self>)
        ensures
            leaf.is_some() == source_borrowed_leaf_shape_image(
                bytes@, start, end, absolute_start,
            ).is_some(),
            leaf.is_some() ==> {
                let selected = leaf.unwrap();
                &&& selected.well_formed()
                &&& selected.image().bytes == bytes@
                &&& selected.image().start == start
                &&& selected.image().len == source_borrowed_leaf_shape_image(
                    bytes@, start, end, absolute_start,
                ).unwrap().len
                &&& selected.image().source == source
                &&& selected.image().absolute_start == absolute_start
            },
    {
        let shape = source_borrowed_leaf_shape(bytes, start, end, absolute_start)?;
        let leaf = Self { bytes, start, len: shape.len, source, absolute_start };
        proof {
            reveal(SourceBorrowedLeaf::image);
        }
        Some(leaf)
    }

    pub fn shape(&self) -> (shape: SourceNodeShape)
        ensures shape.len == self.image().len, shape.height == 0,
    {
        proof {
            reveal(SourceBorrowedLeaf::image);
        }
        source_leaf_shape(self.len)
    }

    pub fn root_slice(&self) -> (bytes: &'a [u8])
        ensures bytes@ == self.image().bytes,
    {
        proof {
            reveal(SourceBorrowedLeaf::image);
        }
        self.bytes
    }

    pub fn start(&self) -> (start: usize)
        ensures start == self.image().start,
    {
        proof {
            reveal(SourceBorrowedLeaf::image);
        }
        self.start
    }

    pub fn len(&self) -> (len: usize)
        ensures len == self.image().len,
    {
        proof {
            reveal(SourceBorrowedLeaf::image);
        }
        self.len
    }

    pub fn is_empty(&self) -> (empty: bool)
        ensures empty == (self.image().len == 0),
    {
        proof {
            reveal(SourceBorrowedLeaf::image);
        }
        self.len == 0
    }

    pub fn source(&self) -> (source: u32)
        ensures source == self.image().source,
    {
        proof {
            reveal(SourceBorrowedLeaf::image);
        }
        self.source
    }

    pub fn absolute_start(&self) -> (absolute_start: usize)
        ensures absolute_start == self.image().absolute_start,
    {
        proof {
            reveal(SourceBorrowedLeaf::image);
        }
        self.absolute_start
    }

    pub fn located_at(&self, at: usize) -> (located: Option<(u8, u32, usize)>)
        requires self.well_formed(),
        ensures
            located == source_borrowed_located_image(
                self.image().bytes, self.image().start, self.image().len,
                self.image().source, self.image().absolute_start, at,
            ),
            located.is_some() == (at < self.image().len),
    {
        proof {
            reveal(SourceBorrowedLeaf::image);
            if at < self.len {
                assert((self.start as int + at as int) < self.bytes@.len());
                assert(self.absolute_start as int + self.start as int + at as int
                    <= usize::MAX as int);
            }
        }
        source_borrowed_located_at(
            self.bytes, self.start, self.len, self.source, self.absolute_start, at,
        )
    }

    pub fn selected_slice(
        &self, from: usize, to: usize,
    ) -> (selected: Option<(&'a [u8], usize)>)
        requires self.well_formed(),
        ensures
            selected.is_some() == self.selection_valid(from, to),
            selected.is_some() ==> {
                let bytes = selected.unwrap().0;
                let physical_start = selected.unwrap().1;
                &&& bytes@ == self.image().bytes.subrange(
                    self.image().start as int + from as int,
                    self.image().start as int + to as int,
                )
                &&& physical_start as int == self.image().absolute_start as int
                    + self.image().start as int + from as int
            },
    {
        if from > to || to > self.len {
            return None;
        }
        proof {
            reveal(SourceBorrowedLeaf::image);
            assert(self.start as int + to as int <= self.bytes@.len());
            assert(self.bytes@.len() == self.bytes.len());
            assert(self.start as int + to as int <= usize::MAX as int);
            assert(self.start as int + from as int <= usize::MAX as int);
        }
        let start = self.start + from;
        let end = self.start + to;
        let physical_start = self.absolute_start.checked_add(start)?;
        let bytes = vstd::slice::slice_subrange(self.bytes, start, end);
        Some((bytes, physical_start))
    }

    pub fn subleaf(&self, from: usize, to: usize) -> (leaf: Option<Self>)
        requires self.well_formed(),
        ensures
            leaf.is_some() == self.selection_valid(from, to),
            leaf.is_some() ==> {
                let selected = leaf.unwrap();
                &&& selected.well_formed()
                &&& selected.image().bytes == self.image().bytes
                &&& selected.image().start as int == self.image().start as int + from as int
                &&& selected.image().len as int == to as int - from as int
                &&& selected.image().source == self.image().source
                &&& selected.image().absolute_start == self.image().absolute_start
            },
    {
        if from > to || to > self.len {
            return None;
        }
        proof {
            reveal(SourceBorrowedLeaf::image);
            assert(self.start as int + to as int <= self.bytes@.len());
            assert(self.bytes@.len() == self.bytes.len());
            assert(self.start as int + to as int <= usize::MAX as int);
            assert(self.start as int + from as int <= usize::MAX as int);
        }
        let start = self.start + from;
        let end = self.start + to;
        self.absolute_start.checked_add(start)?;
        proof {
            if from < to {
                assert(self.absolute_start as int + end as int - 1
                    <= usize::MAX as int);
            }
        }
        Self::new(self.bytes, start, end, self.source, self.absolute_start)
    }
}

pub open spec fn source_branch2_shape_image(
    left: SourceNodeShape, right: SourceNodeShape,
) -> Option<SourceNodeShape> {
    if left.height != right.height || left.height == u8::MAX {
        None
    } else {
        match source_branch2_len_image(left.len, right.len) {
            Some(len) => Some(SourceNodeShape { len, height: (left.height as int + 1) as u8 }),
            None => None,
        }
    }
}

pub open spec fn source_branch3_shape_image(
    first: SourceNodeShape, middle: SourceNodeShape, last: SourceNodeShape,
) -> Option<SourceNodeShape> {
    if first.height != middle.height || first.height != last.height || first.height == u8::MAX {
        None
    } else {
        match source_branch3_len_image(first.len, middle.len, last.len) {
            Some(len) => Some(SourceNodeShape { len, height: (first.height as int + 1) as u8 }),
            None => None,
        }
    }
}

pub fn source_leaf_shape(len: usize) -> (shape: SourceNodeShape)
    ensures shape.len == len, shape.height == 0,
{
    SourceNodeShape { len, height: 0 }
}

pub fn source_branch2_shape(
    left: SourceNodeShape, right: SourceNodeShape,
) -> (shape: Option<SourceNodeShape>)
    ensures shape == source_branch2_shape_image(left, right),
{
    if left.height != right.height {
        return None;
    }
    let len = source_branch2_len(left.len, right.len)?;
    let height = left.height.checked_add(1)?;
    Some(SourceNodeShape { len, height })
}

pub fn source_branch3_shape(
    first: SourceNodeShape, middle: SourceNodeShape, last: SourceNodeShape,
) -> (shape: Option<SourceNodeShape>)
    ensures shape == source_branch3_shape_image(first, middle, last),
{
    if first.height != middle.height || first.height != last.height {
        return None;
    }
    let len = source_branch3_len(first.len, middle.len, last.len)?;
    let height = first.height.checked_add(1)?;
    Some(SourceNodeShape { len, height })
}

pub fn source_branch2_descend(
    parent: SourceNodeShape, left: SourceNodeShape, right: SourceNodeShape, at: usize,
) -> (route: Option<(usize, usize)>)
    ensures
        route.is_some() == (source_branch2_shape_image(left, right) == Some(parent) && at < parent.len),
        route.is_some() ==> {
            let selected = route.unwrap();
            &&& selected == source_branch2_route_image(left.len, at)
            &&& (if selected.0 == 0 {
                selected.1 < left.len && selected.1 == at
            } else {
                selected.0 == 1 && selected.1 < right.len
                    && left.len as int + selected.1 as int == at as int
            })
            &&& left.height < parent.height
            &&& right.height < parent.height
        },
{
    let computed = source_branch2_shape(left, right)?;
    if computed.len != parent.len || computed.height != parent.height || at >= parent.len {
        return None;
    }
    proof {
        assert(computed == parent);
        assert(source_branch2_shape_image(left, right) == Some(parent));
        assert(source_branch2_len_image(left.len, right.len) == Some(parent.len));
    }
    let selected = source_branch2_route(left.len, right.len, at);
    Some(selected)
}

pub fn source_branch3_descend(
    parent: SourceNodeShape, first: SourceNodeShape, middle: SourceNodeShape,
    last: SourceNodeShape, at: usize,
) -> (route: Option<(usize, usize)>)
    ensures
        route.is_some() == (source_branch3_shape_image(first, middle, last) == Some(parent)
            && at < parent.len),
        route.is_some() ==> {
            let selected = route.unwrap();
            &&& selected == source_branch3_route_image(first.len, middle.len, at)
            &&& (if selected.0 == 0 {
                selected.1 < first.len && selected.1 == at
            } else if selected.0 == 1 {
                selected.1 < middle.len
                    && first.len as int + selected.1 as int == at as int
            } else {
                selected.0 == 2 && selected.1 < last.len
                    && first.len as int + middle.len as int + selected.1 as int == at as int
            })
            &&& first.height < parent.height
            &&& middle.height < parent.height
            &&& last.height < parent.height
        },
{
    let computed = source_branch3_shape(first, middle, last)?;
    if computed.len != parent.len || computed.height != parent.height || at >= parent.len {
        return None;
    }
    proof {
        assert(computed == parent);
        assert(source_branch3_shape_image(first, middle, last) == Some(parent));
        assert(source_branch3_len_image(first.len, middle.len, last.len) == Some(parent.len));
    }
    let selected = source_branch3_route(first.len, middle.len, last.len, at);
    Some(selected)
}

impl ScoreHandle {
    pub fn index(self) -> (index: usize)
        ensures index == self.0
    {
        self.0
    }
}

pub enum ScoreNode<'a, B> {
    Integer(i64),
    StringOwned(Vec<u8>),
    BytesOwned(Vec<u8>),
    String(B),
    Bytes(B),
    StringBorrowed(BorrowedIndexedWindow<'a>),
    BytesBorrowed(BorrowedIndexedWindow<'a>),
    Node(Vec<ScoreHandle>),
}

pub struct ScoreEntry<'a, B> {
    pub node: ScoreNode<'a, B>,
    pub encoded_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreStoreError {
    LengthOverflow,
}

pub struct ScoreStore<'a, B> {
    entries: Vec<ScoreEntry<'a, B>>,
}

impl<'a, B> View for ScoreStore<'a, B> {
    type V = Seq<ScoreEntry<'a, B>>;

    closed spec fn view(&self) -> Self::V {
        self.entries@
    }
}

impl<'a, B> ScoreStore<'a, B> {
    pub fn new() -> (store: Self)
        ensures store@ == Seq::<ScoreEntry<'a, B>>::empty()
    {
        Self { entries: Vec::new() }
    }

    pub fn len(&self) -> (length: usize)
        ensures length == self@.len()
    {
        self.entries.len()
    }

    pub fn is_empty(&self) -> (empty: bool)
        ensures empty == (self@.len() == 0)
    {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> (entries: &[ScoreEntry<'a, B>])
        ensures entries@ == self@
    {
        self.entries.as_slice()
    }

    pub fn source_entry_at(&self, index: usize) -> (entry: Option<&ScoreEntry<'a, B>>)
        ensures
            entry.is_some() == (index < self@.len()),
            entry.is_some() ==> *entry.unwrap() == self@[index as int],
    {
        if index < self.entries.len() {
            Some(&self.entries[index])
        } else {
            None
        }
    }

    fn append_entry(&mut self, entry: ScoreEntry<'a, B>) -> (result: Result<ScoreHandle, ScoreStoreError>)
        ensures
            match result {
                Ok(handle) => {
                    handle.0 == old(self)@.len()
                        && final(self)@ == old(self)@.push(entry)
                },
                Err(_) => final(self)@ == old(self)@,
            },
    {
        let index = self.entries.len();
        if self.entries.try_reserve(1).is_err() {
            return Err(ScoreStoreError::LengthOverflow);
        }
        self.entries.push(entry);
        Ok(ScoreHandle(index))
    }

    pub fn append_integer(&mut self, value: i64) -> (result: Result<ScoreHandle, ScoreStoreError>)
        ensures
            match result {
                Ok(handle) => {
                    handle.0 == old(self)@.len()
                        && final(self)@ == old(self)@.push(ScoreEntry {
                            node: ScoreNode::Integer(value), encoded_len: 10,
                        })
                },
                Err(_) => final(self)@ == old(self)@,
            },
    {
        self.append_entry(ScoreEntry { node: ScoreNode::Integer(value), encoded_len: 10 })
    }

    fn append_owned_atom(&mut self, bytes: Vec<u8>, string: bool) -> (result: Result<ScoreHandle, ScoreStoreError>)
        ensures
            match result {
                Ok(handle) => {
                    handle.0 == old(self)@.len()
                        && final(self)@ == old(self)@.push(ScoreEntry {
                            node: if string { ScoreNode::StringOwned(bytes) } else { ScoreNode::BytesOwned(bytes) },
                            encoded_len: owned_atom_prefix_len(bytes@, bytes@.len()) as usize,
                        })
                },
                Err(_) => final(self)@ == old(self)@,
            },
    {
        let mut length: usize = 4;
        let mut index: usize = 0;
        while index < bytes.len()
            invariant
                index <= bytes@.len(),
                self.entries@ == old(self).entries@,
                length as nat == owned_atom_prefix_len(bytes@, index as nat),
            decreases bytes.len() - index,
        {
            let step: usize = if bytes[index] == 0 { 2 } else { 1 };
            match length.checked_add(step) {
                Some(next) => length = next,
                None => return Err(ScoreStoreError::LengthOverflow),
            }
            index += 1;
        }
        let node = if string { ScoreNode::StringOwned(bytes) } else { ScoreNode::BytesOwned(bytes) };
        self.append_entry(ScoreEntry { node, encoded_len: length })
    }

    pub fn append_owned_string(&mut self, bytes: Vec<u8>) -> (result: Result<ScoreHandle, ScoreStoreError>)
        ensures
            match result {
                Ok(handle) => {
                    handle.0 == old(self)@.len()
                        && final(self)@ == old(self)@.push(ScoreEntry {
                            node: ScoreNode::StringOwned(bytes),
                            encoded_len: owned_atom_prefix_len(bytes@, bytes@.len()) as usize,
                        })
                },
                Err(_) => final(self)@ == old(self)@,
            },
    {
        self.append_owned_atom(bytes, true)
    }

    pub fn append_owned_bytes(&mut self, bytes: Vec<u8>) -> (result: Result<ScoreHandle, ScoreStoreError>)
        ensures
            match result {
                Ok(handle) => {
                    handle.0 == old(self)@.len()
                        && final(self)@ == old(self)@.push(ScoreEntry {
                            node: ScoreNode::BytesOwned(bytes),
                            encoded_len: owned_atom_prefix_len(bytes@, bytes@.len()) as usize,
                        })
                },
                Err(_) => final(self)@ == old(self)@,
            },
    {
        self.append_owned_atom(bytes, false)
    }

    fn append_borrowed_atom(&mut self, window: BorrowedIndexedWindow<'a>, string: bool) -> (result: Result<ScoreHandle, ScoreStoreError>)
        requires window.well_formed()
        ensures
            match result {
                Ok(handle) => {
                    handle.0 == old(self)@.len()
                        && final(self)@ == old(self)@.push(ScoreEntry {
                            node: if string { ScoreNode::StringBorrowed(window) } else { ScoreNode::BytesBorrowed(window) },
                            encoded_len: owned_atom_prefix_len(window@, window@.len()) as usize,
                        })
                },
                Err(_) => final(self)@ == old(self)@,
            },
    {
        let bytes = window.as_slice();
        let mut length: usize = 4;
        let mut index: usize = 0;
        while index < bytes.len()
            invariant
                index <= bytes@.len(),
                bytes@ == window@,
                self.entries@ == old(self).entries@,
                length as nat == owned_atom_prefix_len(bytes@, index as nat),
            decreases bytes.len() - index,
        {
            let step: usize = if bytes[index] == 0 { 2 } else { 1 };
            match length.checked_add(step) {
                Some(next) => length = next,
                None => return Err(ScoreStoreError::LengthOverflow),
            }
            index += 1;
        }
        let node = if string { ScoreNode::StringBorrowed(window) } else { ScoreNode::BytesBorrowed(window) };
        self.append_entry(ScoreEntry { node, encoded_len: length })
    }

    pub fn append_borrowed_string(&mut self, window: BorrowedIndexedWindow<'a>) -> (result: Result<ScoreHandle, ScoreStoreError>)
        requires window.well_formed()
        ensures
            match result {
                Ok(handle) => {
                    handle.0 == old(self)@.len()
                        && final(self)@ == old(self)@.push(ScoreEntry {
                            node: ScoreNode::StringBorrowed(window),
                            encoded_len: owned_atom_prefix_len(window@, window@.len()) as usize,
                        })
                },
                Err(_) => final(self)@ == old(self)@,
            },
    {
        self.append_borrowed_atom(window, true)
    }

    pub fn append_borrowed_bytes(&mut self, window: BorrowedIndexedWindow<'a>) -> (result: Result<ScoreHandle, ScoreStoreError>)
        requires window.well_formed()
        ensures
            match result {
                Ok(handle) => {
                    handle.0 == old(self)@.len()
                        && final(self)@ == old(self)@.push(ScoreEntry {
                            node: ScoreNode::BytesBorrowed(window),
                            encoded_len: owned_atom_prefix_len(window@, window@.len()) as usize,
                        })
                },
                Err(_) => final(self)@ == old(self)@,
            },
    {
        self.append_borrowed_atom(window, false)
    }

    pub fn append_string(&mut self, bytes: B, encoded_len: usize) -> (result: Result<ScoreHandle, ScoreStoreError>)
        ensures
            match result {
                Ok(handle) => {
                    handle.0 == old(self)@.len()
                        && final(self)@ == old(self)@.push(ScoreEntry {
                            node: ScoreNode::String(bytes), encoded_len,
                        })
                },
                Err(_) => final(self)@ == old(self)@,
            },
    {
        self.append_entry(ScoreEntry { node: ScoreNode::String(bytes), encoded_len })
    }

    pub fn append_bytes(&mut self, bytes: B, encoded_len: usize) -> (result: Result<ScoreHandle, ScoreStoreError>)
        ensures
            match result {
                Ok(handle) => {
                    handle.0 == old(self)@.len()
                        && final(self)@ == old(self)@.push(ScoreEntry {
                            node: ScoreNode::Bytes(bytes), encoded_len,
                        })
                },
                Err(_) => final(self)@ == old(self)@,
            },
    {
        self.append_entry(ScoreEntry { node: ScoreNode::Bytes(bytes), encoded_len })
    }

    pub fn append_node(&mut self, children: Vec<ScoreHandle>) -> (result: Result<ScoreHandle, ScoreStoreError>)
        ensures
            match result {
                Ok(handle) => {
                    handle.0 == old(self)@.len()
                        && final(self)@ == old(self)@.push(ScoreEntry {
                            node: ScoreNode::Node(children),
                            encoded_len: node_prefix_len(children@, old(self)@, children@.len()) as usize,
                        })
                        && valid_children(children@, old(self)@.len())
                },
                Err(_) => final(self)@ == old(self)@,
            },
    {
        let mut length: usize = 2;
        let mut index: usize = 0;
        while index < children.len()
            invariant
                index <= children@.len(),
                self.entries@ == old(self).entries@,
                valid_children_prefix(children@, self.entries@.len(), index as nat),
                length as nat == node_prefix_len(children@, self.entries@, index as nat),
            decreases children.len() - index,
        {
            let child = children[index];
            if child.0 >= self.entries.len() {
                return Err(ScoreStoreError::LengthOverflow);
            }
            let child_length = self.entries[child.0].encoded_len;
            match length.checked_add(child_length) {
                Some(next) => length = next,
                None => return Err(ScoreStoreError::LengthOverflow),
            }
            index += 1;
        }
        self.append_entry(ScoreEntry { node: ScoreNode::Node(children), encoded_len: length })
    }
}

impl<'a, B> Default for ScoreStore<'a, B> {
    fn default() -> Self {
        Self::new()
    }
}

pub open spec fn valid_children_prefix(children: Seq<ScoreHandle>, entry_count: nat, count: nat) -> bool {
    forall|i: int| 0 <= i < count ==> children[i].0 < entry_count
}

pub open spec fn owned_atom_prefix_len(bytes: Seq<u8>, count: nat) -> nat
    recommends count <= bytes.len()
    decreases count
{
    if count == 0 {
        4
    } else {
        let previous: nat = (count - 1) as nat;
        owned_atom_prefix_len(bytes, previous)
            + (if bytes[previous as int] == 0 { 2nat } else { 1nat })
    }
}

pub open spec fn valid_children(children: Seq<ScoreHandle>, entry_count: nat) -> bool {
    valid_children_prefix(children, entry_count, children.len())
}

pub open spec fn node_prefix_len<'a, B>(children: Seq<ScoreHandle>, entries: Seq<ScoreEntry<'a, B>>, count: nat) -> nat
    recommends count <= children.len(), valid_children_prefix(children, entries.len(), count)
    decreases count
{
    if count == 0 {
        2
    } else {
        let previous: nat = (count - 1) as nat;
        node_prefix_len(children, entries, previous)
            + entries[children[previous as int].0 as int].encoded_len as nat
    }
}

}

#[cfg(test)]
mod tests {
    use super::{ScoreHandle, ScoreNode, ScoreStore, ScoreStoreError};

    #[test]
    fn node_append_keeps_source_order_and_rejects_forward_handles() {
        let mut store = ScoreStore::<()>::new();
        let leaf = store.append_integer(-4).unwrap();
        assert_eq!(leaf.index(), 0);
        assert_eq!(store.source_entry_at(0).unwrap().encoded_len, 10);

        let invalid = store.append_node(vec![ScoreHandle(1)]);
        assert_eq!(invalid, Err(ScoreStoreError::LengthOverflow));
        assert_eq!(store.len(), 1);

        let parent = store.append_node(vec![leaf, leaf]).unwrap();
        assert_eq!(parent.index(), 1);
        let entry = store.source_entry_at(parent.index()).unwrap();
        assert_eq!(entry.encoded_len, 22);
        match &entry.node {
            ScoreNode::Node(children) => assert_eq!(children, &vec![leaf, leaf]),
            _ => panic!("expected node"),
        }
        assert!(store.source_entry_at(2).is_none());
    }

    #[test]
    fn owned_atoms_derive_escaped_lengths_from_retained_bytes() {
        let mut store = ScoreStore::<()>::new();
        let string = store.append_owned_string(vec![b'a', 0, b'b']).unwrap();
        let bytes = store.append_owned_bytes(vec![0, 0, 255]).unwrap();
        assert_eq!(string.index(), 0);
        assert_eq!(bytes.index(), 1);
        let string_entry = store.source_entry_at(string.index()).unwrap();
        assert_eq!(string_entry.encoded_len, 8);
        match &string_entry.node {
            ScoreNode::StringOwned(payload) => assert_eq!(payload, &vec![b'a', 0, b'b']),
            _ => panic!("expected owned string"),
        }
        let bytes_entry = store.source_entry_at(bytes.index()).unwrap();
        assert_eq!(bytes_entry.encoded_len, 9);
        match &bytes_entry.node {
            ScoreNode::BytesOwned(payload) => assert_eq!(payload, &vec![0, 0, 255]),
            _ => panic!("expected owned bytes"),
        }
    }
}
