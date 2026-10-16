//! D-S2 (DR-95): the closed world of the metered history decoder.
//!
//! The History mode of the metered history decoder charges backing only for
//! the allocations that it can see while it decodes: the growth of a vector,
//! the nodes of a tagged B-tree set or of a B-tree map, and the exact bytes of
//! a string or byte buffer. A type whose decoding allocates anything else (a
//! box, a shared pointer, a hash table, an untagged B-tree set) would make
//! that charge too small.

use std::collections::BTreeMap;

/// A type whose serde decoding allocates only vector growth, tagged B-tree
/// sets, B-tree maps and exact strings or byte buffers.
///
/// # Safety
///
/// An implementation promises that deserializing the type, and every type
/// that its deserialization reaches, allocates nothing else: no `Box`, `Rc`,
/// `Arc`, hash table or B-tree set that does not go through the tree-set tag,
/// and no custom `Deserialize` implementation that allocates. The History
/// mode of the metered history decoder relies on this promise to charge the
/// decode's allocations before they happen (`NativeDecodeBacking.v`).
pub unsafe trait ClosedDecode {}

macro_rules! closed_decode_scalars {
    ($($ty:ty),* $(,)?) => { $(unsafe impl ClosedDecode for $ty {})* };
}

closed_decode_scalars!(
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
    f64,
    String
);

unsafe impl<T: ClosedDecode> ClosedDecode for Vec<T> {}
unsafe impl<T: ClosedDecode> ClosedDecode for Option<T> {}
unsafe impl<K: ClosedDecode, V: ClosedDecode> ClosedDecode for BTreeMap<K, V> {}
unsafe impl<T: ClosedDecode, const N: usize> ClosedDecode for [T; N] {}
unsafe impl<A: ClosedDecode, B: ClosedDecode> ClosedDecode for (A, B) {}
unsafe impl<A: ClosedDecode, B: ClosedDecode, C: ClosedDecode> ClosedDecode for (A, B, C) {}
