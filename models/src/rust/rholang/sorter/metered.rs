use std::cell::Cell;
use std::mem::size_of;

use shared::rust::clone_backing::{self, BackingError, BackingMeter, CloneBacking};

use super::score_tree::{ScoreAtom, Tree};

pub struct SorterMeter<'a> {
    backing: &'a dyn BackingMeter,
}

thread_local! {
    static SORT_DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub struct SortDepthGuard;

impl Drop for SortDepthGuard {
    fn drop(&mut self) { SORT_DEPTH.with(|depth| depth.set(depth.get() - 1)); }
}

impl BackingMeter for SorterMeter<'_> {
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), BackingError> {
        self.backing.reserve(operations, scanned, backing)
    }
}

impl<'a> SorterMeter<'a> {
    pub fn new(backing: &'a dyn BackingMeter) -> Self { Self { backing } }

    pub fn enter(&self) -> Result<SortDepthGuard, BackingError> {
        self.reserve(1, size_of::<usize>(), 0)?;
        SORT_DEPTH.with(|depth| {
            let next = depth.get().checked_add(1).ok_or(BackingError::Overflow)?;
            if next > 256 {
                return Err(BackingError::Rejected);
            }
            depth.set(next);
            Ok(SortDepthGuard)
        })
    }

    pub fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), BackingError> {
        self.backing.reserve(operations, scanned, backing)
    }

    // Test-only by D-O1 (DR-111): every production copy uses `clone_blocks`.
    // The tests keep it as a reference charge.
    #[cfg(test)]
    pub fn clone<T: Clone + CloneBacking>(&self, value: &T) -> Result<T, BackingError> {
        clone_backing::reserve_copy_and_cleanup(value, self.backing)?;
        Ok(value.clone())
    }

    // Test-only by D-O1 (DR-111): every production inspection uses
    // `inspect_blocks`. The tests keep it as a reference charge.
    #[cfg(test)]
    pub fn inspect<T: CloneBacking>(&self, value: &T) -> Result<(), BackingError> {
        clone_backing::inspect(value, self.backing)
    }

    /// D-O1 (DR-92): `clone` with the walker's block accounting.
    pub fn clone_blocks<T: Clone + CloneBacking>(&self, value: &T) -> Result<T, BackingError> {
        clone_backing::reserve_blocks_copy_and_cleanup(value, self.backing)?;
        Ok(value.clone())
    }

    /// D-O1 (DR-92): `inspect` with the walker's block accounting.
    pub fn inspect_blocks<T: CloneBacking>(&self, value: &T) -> Result<(), BackingError> {
        clone_backing::inspect_blocks(value, self.backing)
    }

    /// D-O6 (DR-93): prepays `encoded_len()` and the prost encode of a nested
    /// message that writes `encoded_len` bytes.
    pub fn nested_encode<T: CloneBacking>(
        &self,
        value: &T,
        encoded_len: usize,
    ) -> Result<(), BackingError> {
        clone_backing::reserve_nested_encode(value, encoded_len, self.backing)
    }

    // Test-only by D-O1 (DR-111): every production slice copy uses
    // `clone_slice_blocks`. The tests keep it as a reference charge.
    #[cfg(test)]
    pub fn clone_slice<T: Clone + CloneBacking>(
        &self,
        values: &[T],
    ) -> Result<Vec<T>, BackingError> {
        clone_backing::reserve_slice_copy_and_cleanup(values, self.backing)?;
        Ok(values.to_vec())
    }

    /// D-E4 (DR-111): `clone_slice` with the walker's block accounting. It
    /// prepays the copy of the slice and the release of the copy.
    pub fn clone_slice_blocks<T: Clone + CloneBacking>(
        &self,
        values: &[T],
    ) -> Result<Vec<T>, BackingError> {
        clone_backing::reserve_blocks_slice_copy_and_cleanup(values, self.backing)?;
        Ok(values.to_vec())
    }

    /// D-E4 (DR-111): the reads of an insert of an owned value into a hash
    /// table whose capacity is reserved. The insert hashes the value once.
    /// Then it compares the value with an equal element in lockstep (at most
    /// two traversals), or it moves the value into the table (one traversal).
    /// Three block inspections prepay the larger case.
    pub fn hash_insert_blocks<T: CloneBacking>(&self, value: &T) -> Result<(), BackingError> {
        for _ in 0..3 {
            clone_backing::inspect_blocks(value, self.backing)?;
        }
        Ok(())
    }

    /// D-E4 (DR-111), Rule S: a vector of score trees. Each slot is read once
    /// after its birth, when its tree moves out or when the vector is
    /// released. So the vector prepays one read of each slot, besides the
    /// charge of `vec`.
    pub fn score_vec(&self, capacity: usize) -> Result<Vec<Tree<ScoreAtom>>, BackingError> {
        let slots = capacity
            .checked_mul(size_of::<Tree<ScoreAtom>>())
            .ok_or(BackingError::Overflow)?;
        self.backing.reserve(0, slots, 0)?;
        self.vec(capacity)
    }

    pub fn vec<T>(&self, capacity: usize) -> Result<Vec<T>, BackingError> {
        let bytes = capacity
            .checked_mul(size_of::<T>())
            .ok_or(BackingError::Overflow)?;
        self.backing.reserve(capacity, 0, bytes)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(capacity)
            .map_err(|_| BackingError::Allocation)?;
        Ok(values)
    }

    pub fn push<T>(&self, values: &mut Vec<T>, value: T) -> Result<(), BackingError> {
        if values.len() == values.capacity() {
            let next = values
                .len()
                .checked_mul(2)
                .and_then(|n| n.checked_add(1))
                .ok_or(BackingError::Overflow)?;
            let bytes = next
                .checked_mul(size_of::<T>())
                .ok_or(BackingError::Overflow)?;
            let scanned = values
                .len()
                .checked_mul(size_of::<T>())
                .ok_or(BackingError::Overflow)?;
            self.backing.reserve(1, scanned, bytes)?;
            values
                .try_reserve_exact(next - values.len())
                .map_err(|_| BackingError::Allocation)?;
        }
        values.push(value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn clones_reject_when_nested_cleanup_is_unfunded() {
        let source = vec![Arc::<str>::from("payload".repeat(1024))];
        let copied_scans = Cell::new(0usize);
        clone_backing::reserve(&source, &|_, scanned, _| {
            copied_scans.set(copied_scans.get() + scanned);
            Ok(())
        })
        .unwrap();
        let used = Cell::new(0usize);
        let reserve = |_: usize, scanned: usize, _: usize| {
            let next = used.get() + scanned;
            if next > copied_scans.get() {
                Err(BackingError::Rejected)
            } else {
                used.set(next);
                Ok(())
            }
        };
        assert_eq!(
            SorterMeter::new(&reserve).clone(&source),
            Err(BackingError::Rejected)
        );

        copied_scans.set(0);
        clone_backing::reserve_slice(&source, &|_, scanned, _| {
            copied_scans.set(copied_scans.get() + scanned);
            Ok(())
        })
        .unwrap();
        used.set(0);
        assert_eq!(
            SorterMeter::new(&reserve).clone_slice(&source),
            Err(BackingError::Rejected)
        );
    }

    /// Runs `action` under a meter that admits at most `limit` scanned bytes.
    fn within_scanned<R>(
        limit: usize,
        action: impl FnOnce(&SorterMeter<'_>) -> Result<R, BackingError>,
    ) -> Result<R, BackingError> {
        let used = Cell::new(0usize);
        let reserve = |_: usize, scanned: usize, _: usize| {
            let next = used.get() + scanned;
            if next > limit {
                Err(BackingError::Rejected)
            } else {
                used.set(next);
                Ok(())
            }
        };
        action(&SorterMeter::new(&reserve))
    }

    /// D-E4 (DR-111): the block restatement of
    /// `clones_reject_when_nested_cleanup_is_unfunded`. A block clone prepays
    /// the release of its copy, so a credit that covers only the copy, or the
    /// whole charge less one byte, is rejected, and the exact credit is
    /// accepted.
    #[test]
    fn block_clones_reject_when_nested_cleanup_is_unfunded() {
        let source = vec![Arc::<str>::from("payload".repeat(1024))];
        let copy = charged(|meter| clone_backing::reserve_blocks(&source, meter));
        let full = charged(|meter| clone_backing::reserve_blocks_copy_and_cleanup(&source, meter));
        assert!(copy[1] < full[1], "the release of the copy is charged");
        for limit in [copy[1], full[1] - 1] {
            assert_eq!(
                within_scanned(limit, |meter| meter.clone_blocks(&source)),
                Err(BackingError::Rejected),
                "{limit} scanned bytes"
            );
        }
        assert_eq!(
            within_scanned(full[1], |meter| meter.clone_blocks(&source)),
            Ok(source.clone())
        );

        let copy = charged(|meter| clone_backing::reserve_blocks_slice(&source, meter));
        let full =
            charged(|meter| clone_backing::reserve_blocks_slice_copy_and_cleanup(&source, meter));
        assert!(
            copy[1] < full[1],
            "the release of the slice copy is charged"
        );
        for limit in [copy[1], full[1] - 1] {
            assert_eq!(
                within_scanned(limit, |meter| meter.clone_slice_blocks(&source)),
                Err(BackingError::Rejected),
                "{limit} scanned bytes"
            );
        }
        assert_eq!(
            within_scanned(full[1], |meter| meter.clone_slice_blocks(&source)),
            Ok(source.clone())
        );
    }

    fn charged(action: impl FnOnce(&dyn BackingMeter) -> Result<(), BackingError>) -> [usize; 3] {
        let used = Cell::new([0usize; 3]);
        let reserve = |operations: usize, scanned: usize, backing: usize| {
            let [total_operations, total_scanned, total_backing] = used.get();
            used.set([
                total_operations + operations,
                total_scanned + scanned,
                total_backing + backing,
            ]);
            Ok(())
        };
        action(&reserve).expect("charge");
        used.get()
    }

    /// D-O1 (DR-92): the block-mode sorter methods charge the walker's block
    /// charges, and a block clone is rejected before the copy when its charge
    /// does not fit.
    #[test]
    fn block_clones_and_inspections_charge_the_walker_block_charges() {
        let source = vec![Arc::<str>::from("payload".repeat(1024)), Arc::from("x")];
        assert_eq!(
            charged(|meter| SorterMeter::new(meter).inspect_blocks(&source)),
            charged(|meter| clone_backing::inspect_blocks(&source, meter))
        );
        assert_eq!(
            charged(|meter| SorterMeter::new(meter).clone_blocks(&source).map(drop)),
            charged(|meter| clone_backing::reserve_blocks_copy_and_cleanup(&source, meter))
        );
        assert_eq!(
            SorterMeter::new(&|_: usize, _: usize, _: usize| Ok(()))
                .clone_blocks(&source)
                .expect("clone"),
            source
        );
        let reject = |_: usize, scanned: usize, _: usize| {
            if scanned > 0 {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        assert_eq!(
            SorterMeter::new(&reject).clone_blocks(&source),
            Err(BackingError::Rejected)
        );
    }
}
