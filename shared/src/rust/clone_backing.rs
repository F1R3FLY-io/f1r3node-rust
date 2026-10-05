use std::alloc::Layout;
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
}

pub struct Walker<'a> {
    pending: Vec<&'a dyn CloneBacking>,
    capacity: usize,
    meter: &'a dyn BackingMeter,
    copy_payload: bool,
}

impl<'a> Walker<'a> {
    pub fn push<T: CloneBacking>(&mut self, value: &'a T) -> Result<(), BackingError> {
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
        self.meter.reserve(
            0,
            bytes.checked_mul(2).ok_or(BackingError::Overflow)?,
            if self.copy_payload { bytes } else { 0 },
        )
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
        self.allocation(
            values
                .len()
                .checked_mul(size_of::<T>())
                .ok_or(BackingError::Overflow)?,
        )?;
        if !(if self.copy_payload {
            T::inline()
        } else {
            T::inline_inspection()
        }) {
            for value in values {
                self.push(value)?;
            }
        }
        Ok(())
    }

    fn drain(&mut self) -> Result<(), BackingError> {
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
    };
    walker.slice(values)?;
    walker.drain()
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
}
impl<T: CloneBacking> CloneBacking for Option<T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        if let Some(value) = self {
            walker.push(value)?;
        }
        Ok(())
    }
}
impl<T: CloneBacking> CloneBacking for Box<T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        walker.allocation(size_of::<T>())?;
        walker.push(self.as_ref())
    }
}
impl CloneBacking for String {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        walker.allocation(self.len())
    }
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
            std::borrow::Cow::Borrowed(value) => walker.push(*value),
            std::borrow::Cow::Owned(value) => walker.push(value),
        }
    }
}
impl<T: CloneBacking> CloneBacking for Arc<T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        if walker.copy_payload {
            Ok(())
        } else {
            walker.push(self.as_ref())
        }
    }
    fn inline() -> bool { true }
    fn inline_inspection() -> bool { false }
}
impl<T: CloneBacking> CloneBacking for Arc<[T]> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        if walker.copy_payload {
            Ok(())
        } else {
            walker.slice(self.as_ref())
        }
    }
    fn inline() -> bool { true }
    fn inline_inspection() -> bool { false }
}
impl CloneBacking for Arc<str> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        if walker.copy_payload {
            Ok(())
        } else {
            walker.allocation(self.len())
        }
    }
    fn inline() -> bool { true }
    fn inline_inspection() -> bool { false }
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
