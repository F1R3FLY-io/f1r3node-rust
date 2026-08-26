use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

/// A verified RSpace root paired with its monotonic publication generation.
///
/// Root catalog mutation belongs exclusively to `RSpaceStateAuthority`; this
/// value type is shared by authorized history and state-publication code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentRoot {
    root: Blake2b256Hash,
    generation: u64,
}

impl CurrentRoot {
    pub(crate) fn new(root: Blake2b256Hash, generation: u64) -> Self { Self { root, generation } }

    pub fn root(&self) -> &Blake2b256Hash { &self.root }

    pub fn generation(&self) -> u64 { self.generation }
}
