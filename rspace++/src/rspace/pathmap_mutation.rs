use std::marker::PhantomData;

use pathmap::PathMap;
use pathmap::utils::BitMask;
use pathmap::zipper::{
    TrieRefBorrowed, WriteZipperUntracked, Zipper, ZipperForking, ZipperMoving,
    ZipperReadOnlySubtries, ZipperValues, ZipperWriting,
};

use super::guarded_pathmap::{GuardedPathMap, ValueFreeDisposer};

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExtensionPhase {
    Parked,
    Grafted,
    Restored,
}
pub use super::guarded_pathmap::{
    BarrierProvider, DisposeValue, TeardownBarrier, remove_value_preserving_path, retire_pathmap,
};

enum EndpointAuthority<'a> {
    Uncounted,
    Counted(&'a mut usize),
}

impl EndpointAuthority<'_> {
    #[cfg(test)]
    fn observed_count(&self) -> Option<usize> {
        match self {
            Self::Uncounted => None,
            Self::Counted(count) => Some(**count),
        }
    }

    fn added(&self, changed: bool) -> Option<usize> {
        match self {
            Self::Uncounted => None,
            Self::Counted(count) => Some(
                count
                    .checked_add(usize::from(changed))
                    .expect("PathMap endpoint count fits usize"),
            ),
        }
    }

    fn removed(&self, changed: bool) -> Option<usize> {
        match self {
            Self::Uncounted => None,
            Self::Counted(count) => Some(
                count
                    .checked_sub(usize::from(changed))
                    .expect("a resident PathMap value contributes one endpoint"),
            ),
        }
    }

    fn commit(&mut self, next: Option<usize>) {
        if let (Self::Counted(count), Some(next)) = (self, next) {
            **count = next;
        }
    }
}

pub(crate) struct OwnedValue<'a, V, D: DisposeValue<V>> {
    pub(crate) value: Option<V>,
    pub(crate) dispose: &'a D,
}

impl<V, D: DisposeValue<V>> Drop for OwnedValue<'_, V, D> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            self.dispose.dispose(value);
        }
    }
}

fn materialize_missing_suffix<V, D, B>(
    inner: &mut WriteZipperUntracked<'_, '_, V>,
    suffix: &[u8],
    dispose: &D,
    endpoints: &mut EndpointAuthority<'_>,
) where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    if inner.child_count() == 0 {
        let donor = GuardedPathMap::<V, ValueFreeDisposer, B>::value_free_suffix(suffix);
        let parked_count = endpoints.removed(inner.is_val());
        let mut parked = OwnedValue {
            value: inner.remove_val(false),
            dispose,
        };
        endpoints.commit(parked_count);
        #[cfg(test)]
        tests::observe_extension_stage(ExtensionPhase::Parked, endpoints.observed_count());
        donor.graft_value_free_suffix(inner);
        #[cfg(test)]
        tests::observe_extension_stage(ExtensionPhase::Grafted, endpoints.observed_count());
        if parked.value.is_some() {
            let restored_count = endpoints.added(true);
            let replaced = inner.set_val(parked.value.take().unwrap());
            endpoints.commit(restored_count);
            let unexpected = OwnedValue {
                value: replaced,
                dispose,
            };
            assert!(unexpected.value.is_none(), "leaf extension restores a vacant boundary");
        }
        #[cfg(test)]
        tests::observe_extension_stage(ExtensionPhase::Restored, endpoints.observed_count());
        drop(donor);
        inner.descend_to(suffix);
    } else {
        inner.descend_to(suffix);
        inner.create_path();
    }
    assert!(inner.path_exists(), "materialization establishes the selected path");
}

pub fn create_child_path<V, D, B>(
    writer: &mut WriteZipperUntracked<'_, '_, V>,
    byte: u8,
    dispose: &D,
) -> bool
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    assert!(writer.path_exists(), "child materialization starts at an existing focus");
    if writer.child_mask().test_bit(byte) {
        writer.descend_to_byte(byte);
        false
    } else {
        materialize_missing_suffix::<V, D, B>(
            writer,
            &[byte],
            dispose,
            &mut EndpointAuthority::Uncounted,
        );
        true
    }
}

pub struct NativePathCursor<'map, 'path, V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    inner: WriteZipperUntracked<'map, 'static, V>,
    prefix: &'path [u8],
    pending: Vec<u8>,
    missing: bool,
    dispose: &'map D,
    endpoints: EndpointAuthority<'map>,
    barrier: PhantomData<fn() -> B>,
}

impl<'map, 'path, V, D, B> NativePathCursor<'map, 'path, V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    pub fn new(map: &'map mut PathMap<V>, prefix: &'path [u8], dispose: &'map D) -> Self {
        Self::with_authority(map, prefix, dispose, EndpointAuthority::Uncounted)
    }

    pub(crate) fn new_counted(
        map: &'map mut PathMap<V>,
        prefix: &'path [u8],
        dispose: &'map D,
        value_count: &'map mut usize,
    ) -> Self {
        Self::with_authority(map, prefix, dispose, EndpointAuthority::Counted(value_count))
    }

    fn with_authority(
        map: &'map mut PathMap<V>,
        prefix: &'path [u8],
        dispose: &'map D,
        endpoints: EndpointAuthority<'map>,
    ) -> Self {
        if map.is_empty() {
            *map = PathMap::new();
        }
        let mut cursor = Self {
            inner: map.write_zipper(),
            prefix,
            pending: Vec::new(),
            missing: false,
            dispose,
            endpoints,
            barrier: PhantomData,
        };
        cursor.move_to_path(&[]);
        cursor
    }

    pub fn path(&self) -> &[u8] {
        let absolute = if self.missing {
            self.pending.as_slice()
        } else {
            self.inner.path()
        };
        &absolute[self.prefix.len()..]
    }

    pub fn path_exists(&self) -> bool { !self.missing }

    pub fn at_root(&self) -> bool { self.path().is_empty() }

    pub fn val(&self) -> Option<&V> { if self.missing { None } else { self.inner.val() } }

    pub fn is_val(&self) -> bool { self.val().is_some() }

    pub fn move_to_path(&mut self, relative: &[u8]) {
        let prefix = self.prefix;
        let overlap = self
            .inner
            .path()
            .iter()
            .copied()
            .zip(prefix.iter().chain(relative).copied())
            .take_while(|(old, new)| old == new)
            .count();
        self.inner.ascend(self.inner.path().len() - overlap);
        let (first, second) = if overlap < prefix.len() {
            (&prefix[overlap..], relative)
        } else {
            (&relative[overlap - prefix.len()..], &[][..])
        };
        let (first_count, second_count) = {
            let origin = self.inner.fork_read_zipper();
            let focus: TrieRefBorrowed<'_, V> = origin.trie_ref_at_path([]);
            let mut read = focus.fork_read_zipper();
            let first_count = read.descend_to_existing(first);
            let second_count = if first_count == first.len() {
                read.descend_to_existing(second)
            } else {
                0
            };
            (first_count, second_count)
        };
        self.inner.descend_to(&first[..first_count]);
        self.inner.descend_to(&second[..second_count]);
        self.missing = first_count != first.len() || second_count != second.len();
        self.pending.clear();
        if self.missing {
            self.pending.extend_from_slice(prefix);
            self.pending.extend_from_slice(relative);
        }
    }

    pub fn create_path(&mut self) -> bool {
        if !self.missing {
            return false;
        }
        let suffix = &self.pending[self.inner.path().len()..];
        materialize_missing_suffix::<V, D, B>(
            &mut self.inner,
            suffix,
            self.dispose,
            &mut self.endpoints,
        );
        self.missing = false;
        self.pending.clear();
        true
    }

    pub fn set_val(&mut self, value: V) -> Option<V> {
        let mut incoming = OwnedValue {
            value: Some(value),
            dispose: self.dispose,
        };
        let next = self.endpoints.added(!self.is_val());
        self.create_path();
        let replaced = self.inner.set_val(incoming.value.take().unwrap());
        self.endpoints.commit(next);
        replaced
    }

    pub fn remove_val(&mut self) -> Option<V> {
        if self.missing {
            return None;
        }
        let next = self.endpoints.removed(self.inner.is_val());
        let removed = remove_value_preserving_path(&mut self.inner);
        self.endpoints.commit(next);
        removed
    }

    pub fn ascend_byte(&mut self) -> bool {
        if self.at_root() {
            return false;
        }
        if self.missing {
            self.pending.pop();
            if self.pending.len() == self.inner.path().len() {
                self.missing = false;
                self.pending.clear();
            }
            true
        } else {
            self.inner.ascend_byte()
        }
    }

    pub fn descend_first_byte(&mut self) -> bool {
        if self.missing {
            return false;
        }
        let Some(byte) = self.inner.child_mask().iter().next() else {
            return false;
        };
        self.inner.descend_to_byte(byte);
        true
    }

    pub fn descend_until(&mut self) -> bool { !self.missing && self.inner.descend_until() }

    pub fn to_next_sibling_byte(&mut self) -> bool {
        if self.at_root() {
            return false;
        }
        if !self.missing {
            return self.inner.to_next_sibling_byte();
        }
        if self.pending.len() != self.inner.path().len() + 1 {
            return false;
        }
        let previous = *self.pending.last().unwrap();
        let Some(byte) = self.inner.child_mask().iter().find(|byte| *byte > previous) else {
            return false;
        };
        self.inner.descend_to_byte(byte);
        self.missing = false;
        self.pending.clear();
        true
    }

    pub fn remove_val_and_prune(&mut self) -> bool {
        if !self.is_val() {
            return false;
        }
        if self.at_root() || self.inner.child_count() != 0 {
            let value = self.remove_val().unwrap();
            self.dispose.dispose(value);
        } else {
            let count = match &mut self.endpoints {
                EndpointAuthority::Uncounted => None,
                EndpointAuthority::Counted(count) => Some(&mut **count),
            };
            super::guarded_pathmap::remove_leaf_with_floor::<V, D, B>(
                &mut self.inner,
                self.prefix.len(),
                self.dispose,
                count,
            );
        }
        true
    }
}

impl<V, D, B> Drop for NativePathCursor<'_, '_, V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    fn drop(&mut self) { self.inner.reset(); }
}

#[cfg(test)]
#[path = "../../tests/support/pathmap_mutation.rs"]
mod tests;
