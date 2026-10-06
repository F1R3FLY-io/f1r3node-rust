use std::cell::RefCell;
use std::hash::Hash;
use std::mem::size_of;

use dashmap::DashMap;
pub(crate) use shared::rust::clone_backing::arc_allocation_bytes;
use shared::rust::clone_backing::{self, BackingError, CloneBacking, Walker};

use super::errors::RSpaceError;
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::hashing::native_source::SourceMeter;
use super::internal::{Datum, Install, WaitingContinuation};
use super::merger::merging_logic::MergeType;
use super::trace::event::{COMM, Consume, Event, IOEvent, Produce};

pub(crate) fn dashmap_shard_bytes<K: Eq + Hash, V>() -> usize {
    fn shard_bytes<K: Eq + Hash, V, T>(_: for<'a> fn(&'a DashMap<K, V>) -> &'a [T]) -> usize {
        size_of::<T>()
    }
    shard_bytes(DashMap::<K, V>::shards)
}

fn walk(
    meter: &dyn SourceMeter,
    action: impl FnOnce(&dyn clone_backing::BackingMeter) -> Result<(), BackingError>,
) -> Result<(), RSpaceError> {
    let failure = RefCell::new(None);
    let reserve = |operations, scanned, backing| {
        meter
            .reserve(operations, scanned, backing)
            .map_err(|error| {
                *failure.borrow_mut() = Some(error);
                BackingError::Rejected
            })
    };
    let result = action(&reserve);
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    result.map_err(|_| RSpaceError::HostWorkRejected)
}

#[cfg(test)]
pub(crate) fn reserve<T: CloneBacking>(
    value: &T,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    walk(meter, |reserve| clone_backing::reserve(value, reserve))
}
pub(crate) fn reserve_copy_and_cleanup<T: CloneBacking>(
    value: &T,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    walk(meter, |reserve| clone_backing::reserve_copy_and_cleanup(value, reserve))
}
pub(crate) fn reserve_slice_copy_and_cleanup<T: CloneBacking>(
    values: &[T],
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    walk(meter, |reserve| clone_backing::reserve_slice_copy_and_cleanup(values, reserve))
}
pub(crate) fn inspect<T: CloneBacking>(
    value: &T,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    walk(meter, |reserve| clone_backing::inspect(value, reserve))
}
pub(crate) fn reserve_cleanup<T: CloneBacking>(
    value: &T,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    inspect(value, meter)
}

/// C5 (DR-83): the copy and the cleanup of a store-owned vector of shared
/// pointers. The copy walk is the walker's own: a pointer copy carries no
/// payload. The cleanup walk visits each pointer and skips its payload,
/// because each payload's release was prepaid when it entered the cache
/// (C1, DR-81).
pub(crate) fn reserve_shared_copy_and_cleanup<T: CloneBacking>(
    values: &Vec<std::sync::Arc<T>>,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    walk(meter, |reserve| clone_backing::reserve(values, reserve))?;
    walk(meter, |reserve| clone_backing::inspect_shared_pointers(values, reserve))
}

/// C5 (DR-83): the cleanup of one store-owned shared pointer, without the
/// walk into its prepaid payload.
pub(crate) fn reserve_shared_cleanup<T: CloneBacking>(
    value: &std::sync::Arc<T>,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    walk(meter, |reserve| clone_backing::inspect_shared_pointers(value, reserve))
}
pub(crate) fn inspect_slice<T: CloneBacking>(
    values: &[T],
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    walk(meter, |reserve| clone_backing::inspect_slice(values, reserve))
}

impl CloneBacking for Blake2b256Hash {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        walker.push(&self.0)
    }
}
impl<A: Clone + CloneBacking> CloneBacking for Datum<A> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        let Self { a, persist, source } = self;
        walker.push(a)?;
        walker.push(persist)?;
        walker.push(source)
    }
}
impl<P: Clone + CloneBacking, K: Clone + CloneBacking> CloneBacking for WaitingContinuation<P, K> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        let Self {
            patterns,
            continuation,
            persist,
            peeks,
            source,
        } = self;
        walker.push(patterns)?;
        walker.push(continuation)?;
        walker.push(persist)?;
        walker.push(peeks)?;
        walker.push(source)
    }
}
impl<P: CloneBacking, K: CloneBacking> CloneBacking for Install<P, K> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        let Self {
            patterns,
            continuation,
        } = self;
        walker.push(patterns)?;
        walker.push(continuation)
    }
}
impl CloneBacking for MergeType {
    fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), BackingError> {
        match self {
            Self::IntegerAdd | Self::BitmaskOr => Ok(()),
        }
    }
}
macro_rules! fields {
    ($ty:ident { $($field:ident),* $(,)? }) => {
        impl CloneBacking for $ty {
            fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
                let Self { $($field),* } = self;
                $(walker.push($field)?;)*
                Ok(())
            }
        }
    };
}
fields!(Produce {
    channel_hash,
    hash,
    persistent,
    is_deterministic,
    output_value,
    failed
});
fields!(Consume {
    channel_hashes,
    hash,
    persistent
});
fields!(COMM {
    consume,
    produces,
    peeks,
    times_repeated
});
impl CloneBacking for Event {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        match self {
            Self::Comm(value) => walker.push(value),
            Self::IoEvent(value) => walker.push(value),
        }
    }
}
impl CloneBacking for IOEvent {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        match self {
            Self::Produce(value) => walker.push(value),
            Self::Consume(value) => walker.push(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::arc_allocation_bytes;
    use crate::rspace::history::native_reader::measure_allocations;

    #[test]
    fn arc_layout_matches_allocator_for_aligned_and_empty_values() {
        let (_, empty_bytes) = measure_allocations(|| Arc::new(()));
        assert_eq!(empty_bytes, arc_allocation_bytes::<()>().unwrap());

        let (_, aligned_bytes) = measure_allocations(|| Arc::new([0u64; 3]));
        assert_eq!(aligned_bytes, arc_allocation_bytes::<[u64; 3]>().unwrap());

        let (_, mutex_bytes) =
            measure_allocations(|| Arc::new(Mutex::new(HashMap::<u8, u8>::new())));
        assert_eq!(mutex_bytes, arc_allocation_bytes::<Mutex<HashMap<u8, u8>>>().unwrap());
    }
}
