use std::alloc::Layout;
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{BuildHasher, BuildHasherDefault, RandomState};
use std::mem::size_of;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use super::collection_backing::{hash_backing, tree_backing};

pub fn arc_allocation_bytes<T>() -> Option<usize> {
    let (layout, _) = Layout::new::<[AtomicUsize; 2]>()
        .extend(Layout::new::<T>())
        .ok()?;
    Some(layout.pad_to_align().size())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackingError {
    Rejected,
    Overflow,
    Allocation,
}

pub trait BackingMeter {
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), BackingError>;
}

impl<F> BackingMeter for F
where F: Fn(usize, usize, usize) -> Result<(), BackingError>
{
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), BackingError> {
        self(operations, scanned, backing)
    }
}

pub trait CloneBacking {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError>;

    fn inline() -> bool
    where Self: Sized {
        false
    }

    fn inline_inspection() -> bool
    where Self: Sized {
        Self::inline()
    }

    /// D-O1 (DR-92): true for shared pointers, whose copy also touches the
    /// strong count in the shared allocation's header.
    fn shared_header() -> bool
    where Self: Sized {
        false
    }

    /// D-O1 (DR-94): true when `children` would push nothing and reserve
    /// nothing, as for an empty container. A block-mode walk then charges the
    /// value as an inline field: its bytes lie in the enclosing block, and the
    /// walker reads only its length or discriminant there.
    fn walk_is_empty(&self) -> bool { false }
}

/// D-O1 (DR-92): block-accounting constants. A worklist entry is one fat
/// pointer: the chunked worklist writes it once at the push and reads it once
/// at the pop (it never moves an entry). The walker may re-read one word of
/// the entry that an enclosing enum shares with it as a niche, a consumer
/// traversal may re-read one word of it, and one more word pays the header
/// moves of the vector of chunks (at most 8 bytes per entry,
/// `WalkerBlockCharge.worklist_charge_covers_peak`).
pub const BLOCK_ENTRY_SCANNED: usize = 2 * size_of::<&dyn CloneBacking>() + 3 * size_of::<u64>();
/// D-B3 (DR-92): the chunks of the worklist and the vector of chunk headers
/// allocate at most four slots per entry of the peak, so at most four slots
/// per entry the walk pushes.
pub const BLOCK_ENTRY_BACKING: usize = 4 * size_of::<&dyn CloneBacking>();
/// D-O1 (DR-92): an inline field lies in its enclosing block, which is
/// charged once; a consumer may re-read one machine word of the field (a
/// discriminant or a scalar while hashing or encoding it).
pub const BLOCK_FIELD_SCANNED: usize = size_of::<u64>();
/// D-O1 (DR-92): a shared-pointer copy also reads and writes the strong
/// count in the shared allocation's header.
pub const BLOCK_SHARED_HEADER_SCANNED: usize = 2 * size_of::<usize>();

// D-B3 (DR-92): `WalkerBlockCharge.v` proves the worklist bounds for 16-byte
// entries and 24-byte chunk headers.
const _: () = assert!(size_of::<&dyn CloneBacking>() == 16);
const _: () = assert!(size_of::<Vec<&dyn CloneBacking>>() == 24);

/// D-O6 (DR-93): a worklist entry of a depth walk: the value and its nesting
/// depth (the root entry has depth 1).
struct DepthEntry<'a> {
    value: &'a dyn CloneBacking,
    depth: usize,
}

// D-O6 (DR-93): `WalkerBlockCharge.v` proves the worklist bounds for entries
// of at least 16 bytes; a depth entry has 24.
const _: () = assert!(size_of::<DepthEntry<'static>>() == 24);
const _: () = assert!(size_of::<Vec<DepthEntry<'static>>>() == 24);

/// D-O6 (DR-93): the entry constants of a depth walk: the block-mode constants
/// for a 24-byte entry (one write, one read, three words).
pub const BLOCK_DEPTH_ENTRY_SCANNED: usize =
    2 * size_of::<DepthEntry<'static>>() + 3 * size_of::<u64>();
pub const BLOCK_DEPTH_ENTRY_BACKING: usize = 4 * size_of::<DepthEntry<'static>>();

const FIRST_CHUNK_SLOTS: usize = 4;

/// D-B3 (DR-92): the block-mode worklist, a stack of chunks. Chunk 0 holds
/// `FIRST_CHUNK_SLOTS` entries and chunk j holds `FIRST_CHUNK_SLOTS << j`. A
/// chunk is reserved with its exact capacity when the stack first grows into
/// it and is kept until the walk ends, so it is allocated at most once and a
/// full chunk is never reallocated: a push writes one entry and a pop reads
/// one entry. The chunks below the active one are full and the chunks above
/// it are empty.
struct ChunkedWorklist<T> {
    first: Vec<T>,
    rest: Vec<Vec<T>>,
    /// The chunk that holds the top entry: 0 for `first`, j for `rest[j - 1]`.
    active: usize,
}

impl<T> ChunkedWorklist<T> {
    const fn new() -> Self {
        Self {
            first: Vec::new(),
            rest: Vec::new(),
            active: 0,
        }
    }

    fn slots(chunk: usize) -> Result<usize, BackingError> {
        u32::try_from(chunk)
            .ok()
            .and_then(|exponent| 2_usize.checked_pow(exponent))
            .and_then(|scale| scale.checked_mul(FIRST_CHUNK_SLOTS))
            .ok_or(BackingError::Overflow)
    }

    fn chunk(&mut self, chunk: usize) -> &mut Vec<T> {
        match chunk {
            0 => &mut self.first,
            index => &mut self.rest[index - 1],
        }
    }

    fn push(&mut self, value: T) -> Result<(), BackingError> {
        loop {
            let slots = Self::slots(self.active)?;
            let chunk = self.chunk(self.active);
            if chunk.len() < slots {
                if chunk.capacity() < slots {
                    chunk
                        .try_reserve_exact(slots)
                        .map_err(|_| BackingError::Allocation)?;
                }
                chunk.push(value);
                return Ok(());
            }
            let next = self.active.checked_add(1).ok_or(BackingError::Overflow)?;
            if next > self.rest.len() {
                self.rest
                    .try_reserve(1)
                    .map_err(|_| BackingError::Allocation)?;
                self.rest.push(Vec::new());
            }
            self.active = next;
        }
    }

    fn pop(&mut self) -> Option<T> {
        loop {
            if let Some(value) = self.chunk(self.active).pop() {
                return Some(value);
            }
            self.active = self.active.checked_sub(1)?;
        }
    }
}

pub struct Walker<'a> {
    pending: Vec<&'a dyn CloneBacking>,
    capacity: usize,
    meter: &'a dyn BackingMeter,
    copy_payload: bool,
    /// C5 (DR-83): a cleanup walk that visits each shared pointer but not its
    /// payload, for store-owned pointers whose payload release was prepaid.
    shared_pointers: bool,
    /// D-O1 (DR-92): block accounting. Each memory block is charged once;
    /// a push charges only the worklist or field constants.
    blocks: bool,
    /// D-B3 (DR-92): the worklist of block mode.
    chunked: ChunkedWorklist<&'a dyn CloneBacking>,
    /// D-O6 (DR-93): a block-mode walk that also records the nesting depth
    /// of every entry, with its own worklist of depth entries.
    depths: bool,
    deep: ChunkedWorklist<DepthEntry<'a>>,
    current_depth: usize,
    max_depth: usize,
}

impl<'a> Walker<'a> {
    pub fn push<T: CloneBacking>(&mut self, value: &'a T) -> Result<(), BackingError> {
        if self.blocks {
            return self.push_block_entry(value);
        }
        self.meter.reserve(
            3,
            size_of::<T>()
                .checked_mul(3)
                .ok_or(BackingError::Overflow)?,
            0,
        )?;
        if !(if self.copy_payload {
            T::inline()
        } else {
            T::inline_inspection()
        }) {
            let needed = self
                .pending
                .len()
                .checked_add(1)
                .ok_or(BackingError::Overflow)?;
            if needed > self.capacity {
                let next = needed
                    .max(self.capacity.checked_mul(2).ok_or(BackingError::Overflow)?)
                    .max(8);
                let bytes = next
                    .checked_mul(size_of::<&dyn CloneBacking>())
                    .ok_or(BackingError::Overflow)?;
                let scanned = self
                    .pending
                    .len()
                    .checked_mul(size_of::<&dyn CloneBacking>())
                    .ok_or(BackingError::Overflow)?;
                self.meter.reserve(
                    self.pending
                        .len()
                        .checked_add(1)
                        .ok_or(BackingError::Overflow)?,
                    scanned,
                    bytes,
                )?;
                self.pending
                    .try_reserve_exact(next - self.pending.len())
                    .map_err(|_| BackingError::Allocation)?;
                self.capacity = next;
            }
            self.pending.push(value);
        }
        Ok(())
    }

    pub fn allocation(&self, bytes: usize) -> Result<(), BackingError> {
        if self.blocks {
            return self.block(bytes, true, true);
        }
        self.meter.reserve(
            0,
            bytes.checked_mul(2).ok_or(BackingError::Overflow)?,
            if self.copy_payload { bytes } else { 0 },
        )
    }

    /// D-O1 (DR-92): an allocation whose elements the walk does not visit
    /// (string bytes, slices of inline scalars). The legacy charge is the
    /// same as `allocation`.
    pub fn opaque_allocation(&self, bytes: usize) -> Result<(), BackingError> {
        if self.blocks {
            return self.block(bytes, false, true);
        }
        self.allocation(bytes)
    }

    /// D-O1 (DR-92): one memory block of `bytes`. An inspection reads it once
    /// for the consumer's traversal and once more when the walker visits its
    /// elements; a copy also writes it and, for a heap block, allocates it.
    fn block(&self, bytes: usize, visited: bool, allocated: bool) -> Result<(), BackingError> {
        let reads = usize::from(visited) + 1 + usize::from(self.copy_payload);
        self.meter.reserve(
            0,
            bytes.checked_mul(reads).ok_or(BackingError::Overflow)?,
            if self.copy_payload && allocated {
                bytes
            } else {
                0
            },
        )
    }

    /// D-O1 (DR-92): the inline bytes of a value read through a reference
    /// (the walk's root, a shared payload or a borrowed referent): a block
    /// that this walk does not allocate.
    fn referent_block<T: CloneBacking>(&self) -> Result<(), BackingError> {
        let visited = !self.is_inline::<T>();
        self.block(size_of::<T>(), visited, false)
    }

    fn is_inline<T: CloneBacking>(&self) -> bool {
        if self.copy_payload {
            T::inline()
        } else {
            T::inline_inspection()
        }
    }

    /// D-O1 (DR-92): a push in block mode. An inline field costs a constant
    /// (its bytes lie in the enclosing block); a worklist entry costs the
    /// entry constants, which also pay the worklist's chunks and the moves of
    /// its chunk headers (D-B3).
    fn push_block_entry<T: CloneBacking>(&mut self, value: &'a T) -> Result<(), BackingError> {
        if !self.is_inline::<T>() && value.walk_is_empty() {
            return self.meter.reserve(3, BLOCK_FIELD_SCANNED, 0);
        }
        if self.is_inline::<T>() {
            let header = if self.copy_payload && T::shared_header() {
                BLOCK_SHARED_HEADER_SCANNED
            } else {
                0
            };
            return self.meter.reserve(
                3,
                BLOCK_FIELD_SCANNED
                    .checked_add(header)
                    .ok_or(BackingError::Overflow)?,
                0,
            );
        }
        if self.depths {
            self.meter
                .reserve(3, BLOCK_DEPTH_ENTRY_SCANNED, BLOCK_DEPTH_ENTRY_BACKING)?;
            let depth = self
                .current_depth
                .checked_add(1)
                .ok_or(BackingError::Overflow)?;
            self.max_depth = self.max_depth.max(depth);
            return self.deep.push(DepthEntry { value, depth });
        }
        self.meter
            .reserve(3, BLOCK_ENTRY_SCANNED, BLOCK_ENTRY_BACKING)?;
        self.chunked.push(value)
    }

    pub fn collection(&self, operations: usize, bytes: usize) -> Result<(), BackingError> {
        self.meter.reserve(operations, 0, 0)?;
        self.allocation(bytes)
    }

    pub fn slice<T: CloneBacking>(&mut self, values: &'a [T]) -> Result<(), BackingError> {
        self.meter.reserve(
            values.len().checked_mul(2).ok_or(BackingError::Overflow)?,
            0,
            0,
        )?;
        let bytes = values
            .len()
            .checked_mul(size_of::<T>())
            .ok_or(BackingError::Overflow)?;
        if self.is_inline::<T>() {
            // D-O1 (DR-92): the elements are not visited (legacy: same charge).
            self.opaque_allocation(bytes)?;
            // D-O1 (DR-94): a block-mode copy of shared pointers also updates
            // each pointer's strong count, as a pushed shared pointer does.
            if self.blocks && self.copy_payload && T::shared_header() {
                self.meter.reserve(
                    0,
                    values
                        .len()
                        .checked_mul(BLOCK_SHARED_HEADER_SCANNED)
                        .ok_or(BackingError::Overflow)?,
                    0,
                )?;
            }
        } else {
            self.allocation(bytes)?;
            for value in values {
                self.push(value)?;
            }
        }
        Ok(())
    }

    fn drain(&mut self) -> Result<(), BackingError> {
        if self.depths {
            while let Some(DepthEntry { value, depth }) = self.deep.pop() {
                self.current_depth = depth;
                value.children(self)?;
            }
            return Ok(());
        }
        if self.blocks {
            while let Some(value) = self.chunked.pop() {
                value.children(self)?;
            }
            return Ok(());
        }
        while let Some(value) = self.pending.pop() {
            value.children(self)?;
        }
        Ok(())
    }
}

fn walk<T: CloneBacking>(
    value: &T,
    meter: &dyn BackingMeter,
    copy_payload: bool,
) -> Result<(), BackingError> {
    let mut walker = Walker {
        pending: Vec::new(),
        capacity: 0,
        meter,
        copy_payload,
        shared_pointers: false,
        blocks: false,
        chunked: ChunkedWorklist::new(),
        depths: false,
        deep: ChunkedWorklist::new(),
        current_depth: 0,
        max_depth: 0,
    };
    walker.push(value)?;
    walker.drain()
}

fn walk_slice<T: CloneBacking>(
    values: &[T],
    meter: &dyn BackingMeter,
    copy_payload: bool,
) -> Result<(), BackingError> {
    let mut walker = Walker {
        pending: Vec::new(),
        capacity: 0,
        meter,
        copy_payload,
        shared_pointers: false,
        blocks: false,
        chunked: ChunkedWorklist::new(),
        depths: false,
        deep: ChunkedWorklist::new(),
        current_depth: 0,
        max_depth: 0,
    };
    walker.slice(values)?;
    walker.drain()
}

/// D-O1 (DR-92): a block-accounting walk. The root's inline bytes are one
/// block (read through a reference, so not allocated by the walk); every
/// other block is charged where the walk meets it.
fn walk_blocks<T: CloneBacking>(
    value: &T,
    meter: &dyn BackingMeter,
    copy_payload: bool,
    shared_pointers: bool,
) -> Result<(), BackingError> {
    let mut walker = Walker {
        pending: Vec::new(),
        capacity: 0,
        meter,
        copy_payload,
        shared_pointers,
        blocks: true,
        chunked: ChunkedWorklist::new(),
        depths: false,
        deep: ChunkedWorklist::new(),
        current_depth: 0,
        max_depth: 0,
    };
    walker.referent_block::<T>()?;
    walker.push(value)?;
    walker.drain()
}

fn walk_slice_blocks<T: CloneBacking>(
    values: &[T],
    meter: &dyn BackingMeter,
    copy_payload: bool,
    shared_pointers: bool,
) -> Result<(), BackingError> {
    let mut walker = Walker {
        pending: Vec::new(),
        capacity: 0,
        meter,
        copy_payload,
        shared_pointers,
        blocks: true,
        chunked: ChunkedWorklist::new(),
        depths: false,
        deep: ChunkedWorklist::new(),
        current_depth: 0,
        max_depth: 0,
    };
    walker.slice(values)?;
    walker.drain()
}

/// D-O1 (DR-92): block-accounting inspection: prepays one linear traversal
/// of `value` (and the walk itself).
pub fn inspect_blocks<T: CloneBacking>(
    value: &T,
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    walk_blocks(value, meter, false, false)
}
/// D-O1 (DR-92): block-accounting copy: prepays a clone of `value`.
pub fn reserve_blocks<T: CloneBacking>(
    value: &T,
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    walk_blocks(value, meter, true, false)
}
/// D-O1 (DR-92): block-accounting copy and the release of the copy.
pub fn reserve_blocks_copy_and_cleanup<T: CloneBacking>(
    value: &T,
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    reserve_blocks(value, meter)?;
    inspect_blocks(value, meter)
}
pub fn inspect_blocks_slice<T: CloneBacking>(
    values: &[T],
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    walk_slice_blocks(values, meter, false, false)
}
pub fn reserve_blocks_slice<T: CloneBacking>(
    values: &[T],
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    walk_slice_blocks(values, meter, true, false)
}
pub fn reserve_blocks_slice_copy_and_cleanup<T: CloneBacking>(
    values: &[T],
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    reserve_blocks_slice(values, meter)?;
    inspect_blocks_slice(values, meter)
}
/// D-O1 (DR-92): block-accounting form of `inspect_shared_pointers`.
pub fn inspect_shared_pointers_blocks<T: CloneBacking>(
    value: &T,
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    walk_blocks(value, meter, false, true)
}
/// D-O1 (DR-92): block-accounting form of `inspect_shared_pointer_slice`.
pub fn inspect_shared_pointer_slice_blocks<T: CloneBacking>(
    values: &[T],
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    walk_slice_blocks(values, meter, false, true)
}

/// D-O6 (DR-93): the result of a depth walk: the deepest entry's nesting
/// depth (the root entry has depth 1; an inline root has none) and the
/// scanned bytes that the walk reserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockWalk {
    pub depth: usize,
    pub scanned: usize,
}

/// D-O6 (DR-93): a block-mode inspection that also returns the nesting
/// depth of `value`. It prepays one linear traversal of `value`, like
/// `inspect_blocks`, with the depth-entry constants.
pub fn inspect_blocks_depth<T: CloneBacking>(
    value: &T,
    meter: &dyn BackingMeter,
) -> Result<BlockWalk, BackingError> {
    let scanned = Cell::new(0_usize);
    let counting = |operations: usize, bytes: usize, backing: usize| {
        scanned.set(
            scanned
                .get()
                .checked_add(bytes)
                .ok_or(BackingError::Overflow)?,
        );
        meter.reserve(operations, bytes, backing)
    };
    let mut walker = Walker {
        pending: Vec::new(),
        capacity: 0,
        meter: &counting,
        copy_payload: false,
        shared_pointers: false,
        blocks: true,
        chunked: ChunkedWorklist::new(),
        depths: true,
        deep: ChunkedWorklist::new(),
        current_depth: 0,
        max_depth: 0,
    };
    walker.referent_block::<T>()?;
    walker.push(value)?;
    walker.drain()?;
    let depth = walker.max_depth;
    Ok(BlockWalk {
        depth,
        scanned: scanned.get(),
    })
}

/// D-O6 (DR-93): prepays `encoded_len()` and a prost encode of `value` that
/// writes `encoded_len` bytes. Prost computes the length of every nested
/// message again at each enclosing level, so a site that computes the length
/// and then encodes reads the value at most `2 + h` times, where `h` is the
/// message height. The depth walk prepays one traversal, and the walk depth
/// `d` is at least `1 + h`, so `d` more traversals of the walk's charge and
/// the output bytes cover the rest (`NestedEncodeCost.v`).
pub fn reserve_nested_encode<T: CloneBacking>(
    value: &T,
    encoded_len: usize,
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    let walk = inspect_blocks_depth(value, meter)?;
    let traversals = walk
        .depth
        .checked_mul(walk.scanned)
        .and_then(|bytes| bytes.checked_add(encoded_len))
        .ok_or(BackingError::Overflow)?;
    meter.reserve(0, traversals, 0)
}

pub fn reserve<T: CloneBacking>(value: &T, meter: &dyn BackingMeter) -> Result<(), BackingError> {
    walk(value, meter, true)
}
pub fn reserve_copy_and_cleanup<T: CloneBacking>(
    value: &T,
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    reserve(value, meter)?;
    inspect(value, meter)
}
pub fn reserve_slice<T: CloneBacking>(
    values: &[T],
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    walk_slice(values, meter, true)
}
pub fn reserve_slice_copy_and_cleanup<T: CloneBacking>(
    values: &[T],
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    reserve_slice(values, meter)?;
    inspect_slice(values, meter)
}
pub fn inspect<T: CloneBacking>(value: &T, meter: &dyn BackingMeter) -> Result<(), BackingError> {
    walk(value, meter, false)
}
/// C5 (DR-83): the cleanup walk of a value whose shared pointers are
/// store-owned and whose payload releases were prepaid when the payloads
/// entered the cache. The walk visits each pointer and skips its payload,
/// so pointers nested inside a payload are never reached.
pub fn inspect_shared_pointers<T: CloneBacking>(
    value: &T,
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    let mut walker = Walker {
        pending: Vec::new(),
        capacity: 0,
        meter,
        copy_payload: false,
        shared_pointers: true,
        blocks: false,
        chunked: ChunkedWorklist::new(),
        depths: false,
        deep: ChunkedWorklist::new(),
        current_depth: 0,
        max_depth: 0,
    };
    walker.push(value)?;
    walker.drain()
}
/// D-O4 (DR-89): `inspect_shared_pointers` for a slice: the cleanup walk of a
/// copied slice of shared pointers whose payload releases were prepaid when
/// the payloads were born. It charges the slice allocation and each pointer,
/// and skips every payload.
pub fn inspect_shared_pointer_slice<T: CloneBacking>(
    values: &[T],
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    let mut walker = Walker {
        pending: Vec::new(),
        capacity: 0,
        meter,
        copy_payload: false,
        shared_pointers: true,
        blocks: false,
        chunked: ChunkedWorklist::new(),
        depths: false,
        deep: ChunkedWorklist::new(),
        current_depth: 0,
        max_depth: 0,
    };
    walker.slice(values)?;
    walker.drain()
}
pub fn inspect_slice<T: CloneBacking>(
    values: &[T],
    meter: &dyn BackingMeter,
) -> Result<(), BackingError> {
    walk_slice(values, meter, false)
}

impl<T: CloneBacking> CloneBacking for Vec<T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        walker.slice(self)
    }
    fn walk_is_empty(&self) -> bool { self.is_empty() }
}
impl<T: CloneBacking> CloneBacking for Option<T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        if let Some(value) = self {
            walker.push(value)?;
        }
        Ok(())
    }
    fn walk_is_empty(&self) -> bool { self.is_none() }
}
impl<T: CloneBacking> CloneBacking for Box<T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        if walker.is_inline::<T>() {
            // D-O1 (DR-92): an inline payload is not visited (legacy: same charge).
            walker.opaque_allocation(size_of::<T>())?;
        } else {
            walker.allocation(size_of::<T>())?;
        }
        walker.push(self.as_ref())
    }
}
impl CloneBacking for String {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        // D-O1 (DR-92): string bytes are not visited (legacy: same charge).
        walker.opaque_allocation(self.len())
    }
    fn walk_is_empty(&self) -> bool { self.is_empty() }
}
impl<K: CloneBacking, V: CloneBacking> CloneBacking for BTreeMap<K, V> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        let (operations, bytes) = tree_backing::<K, V>(self.len()).ok_or(BackingError::Overflow)?;
        walker.collection(operations, bytes)?;
        for (key, value) in self {
            walker.push(key)?;
            walker.push(value)?;
        }
        Ok(())
    }
    fn walk_is_empty(&self) -> bool { self.is_empty() }
}
impl<T: CloneBacking> CloneBacking for BTreeSet<T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        let (operations, bytes) =
            tree_backing::<T, ()>(self.len()).ok_or(BackingError::Overflow)?;
        walker.collection(operations, bytes)?;
        for value in self {
            walker.push(value)?;
        }
        Ok(())
    }
    fn walk_is_empty(&self) -> bool { self.is_empty() }
}
impl<K: CloneBacking, V: CloneBacking, S: BuildHasher + CloneBacking> CloneBacking
    for HashMap<K, V, S>
{
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        let (operations, bytes) =
            hash_backing::<K, V>(self.capacity()).ok_or(BackingError::Overflow)?;
        walker.collection(operations, bytes)?;
        walker.push(self.hasher())?;
        for (key, value) in self {
            walker.push(key)?;
            walker.push(value)?;
        }
        Ok(())
    }
}
impl<T: CloneBacking, S: BuildHasher + CloneBacking> CloneBacking for HashSet<T, S> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        let (operations, bytes) =
            hash_backing::<T, ()>(self.capacity()).ok_or(BackingError::Overflow)?;
        walker.collection(operations, bytes)?;
        walker.push(self.hasher())?;
        for value in self {
            walker.push(value)?;
        }
        Ok(())
    }
}
/// A borrowed value copies only its reference and inspects its referent. An
/// owned value copies and inspects its payload (C2, DR-82).
impl<T: CloneBacking + Clone> CloneBacking for std::borrow::Cow<'_, T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        match self {
            std::borrow::Cow::Borrowed(_) if walker.copy_payload => Ok(()),
            std::borrow::Cow::Borrowed(value) => {
                // D-O1 (DR-92): the referent is a block of its own.
                if walker.blocks {
                    walker.referent_block::<T>()?;
                }
                walker.push(*value)
            }
            std::borrow::Cow::Owned(value) => walker.push(value),
        }
    }
}
impl<T: CloneBacking> CloneBacking for Arc<T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        if walker.copy_payload || walker.shared_pointers {
            Ok(())
        } else {
            // D-O1 (DR-92): the shared payload is a block of its own.
            if walker.blocks {
                walker.referent_block::<T>()?;
            }
            walker.push(self.as_ref())
        }
    }
    fn inline() -> bool { true }
    fn inline_inspection() -> bool { false }
    fn shared_header() -> bool { true }
}
impl<T: CloneBacking> CloneBacking for Arc<[T]> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        if walker.copy_payload || walker.shared_pointers {
            Ok(())
        } else {
            walker.slice(self.as_ref())
        }
    }
    fn inline() -> bool { true }
    fn inline_inspection() -> bool { false }
    fn shared_header() -> bool { true }
}
impl CloneBacking for Arc<str> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        if walker.copy_payload {
            Ok(())
        } else {
            // D-O1 (DR-92): string bytes are not visited (legacy: same charge).
            walker.opaque_allocation(self.len())
        }
    }
    fn inline() -> bool { true }
    fn inline_inspection() -> bool { false }
    fn shared_header() -> bool { true }
}
impl CloneBacking for RandomState {
    fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), BackingError> { Ok(()) }
    fn inline() -> bool { true }
}
impl<T> CloneBacking for BuildHasherDefault<T> {
    fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), BackingError> { Ok(()) }
    fn inline() -> bool { true }
}
impl<T: CloneBacking, const N: usize> CloneBacking for [T; N] {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        for value in self {
            walker.push(value)?;
        }
        Ok(())
    }
    fn inline() -> bool { T::inline() }
    fn inline_inspection() -> bool { T::inline_inspection() }
}
impl<A: CloneBacking, B: CloneBacking> CloneBacking for (A, B) {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        walker.push(&self.0)?;
        walker.push(&self.1)
    }
}
macro_rules! inline {
    ($($ty:ty),+ $(,)?) => { $(impl CloneBacking for $ty {
        fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), BackingError> { let _: Self = *self; Ok(()) }
        fn inline() -> bool { true }
    })+ };
}
inline!(
    (),
    bool,
    char,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    f32,
    f64
);

#[cfg(test)]
mod tests;
