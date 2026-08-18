use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use pathmap::PathMap;
use pathmap::arena_compact::ArenaCompactTree;
use pathmap::merkleization::MerkleizeResult;
use pathmap::ring::{AlgebraicResult, COUNTER_IDENT, DistributiveLattice, Lattice, SELF_IDENT};
use pathmap::zipper::{ZipperMoving, ZipperValues, ZipperWriting};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::pathmap_codec::{CompactTrie, CompactTrieError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BagError {
    ZeroMultiplicity,
    MultiplicityOverflow { current: u64, added: u64 },
    InsufficientMultiplicity { available: u64, requested: u64 },
}

impl fmt::Display for BagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroMultiplicity => f.write_str("a stored bag multiplicity must be positive"),
            Self::MultiplicityOverflow { current, added } => {
                write!(f, "bag multiplicity overflow: {current} + {added}")
            }
            Self::InsufficientMultiplicity {
                available,
                requested,
            } => write!(
                f,
                "insufficient bag multiplicity: requested {requested}, available {available}"
            ),
        }
    }
}

impl std::error::Error for BagError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BagCodecError {
    CompactTrie(CompactTrieError),
    MissingMultiplicity,
    ZeroMultiplicity,
}

impl fmt::Display for BagCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompactTrie(error) => write!(f, "invalid Bag compact trie: {error}"),
            Self::MissingMultiplicity => {
                f.write_str("Bag compact trie contains a value-free terminating path")
            }
            Self::ZeroMultiplicity => f.write_str("Bag compact trie contains a zero multiplicity"),
        }
    }
}

impl std::error::Error for BagCodecError {}

impl From<CompactTrieError> for BagCodecError {
    fn from(error: CompactTrieError) -> Self { Self::CompactTrie(error) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Multiplicity(u64);

impl Multiplicity {
    pub const ONE: Self = Self(1);

    pub const fn new(count: u64) -> Result<Self, BagError> {
        if count == 0 {
            Err(BagError::ZeroMultiplicity)
        } else {
            Ok(Self(count))
        }
    }

    pub const fn get(self) -> u64 { self.0 }

    pub fn checked_add(self, other: Self) -> Result<Self, BagError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(BagError::MultiplicityOverflow {
                current: self.0,
                added: other.0,
            })
    }

    pub fn checked_sub(self, other: Self) -> Result<Option<Self>, BagError> {
        match self.0.cmp(&other.0) {
            Ordering::Less => Err(BagError::InsufficientMultiplicity {
                available: self.0,
                requested: other.0,
            }),
            Ordering::Equal => Ok(None),
            Ordering::Greater => Ok(Some(Self(self.0 - other.0))),
        }
    }
}

impl TryFrom<u64> for Multiplicity {
    type Error = BagError;

    fn try_from(value: u64) -> Result<Self, Self::Error> { Self::new(value) }
}

impl From<Multiplicity> for u64 {
    fn from(value: Multiplicity) -> Self { value.get() }
}

impl Serialize for Multiplicity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        serializer.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for Multiplicity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let count = u64::deserialize(deserializer)?;
        Self::new(count).map_err(serde::de::Error::custom)
    }
}

impl Lattice for Multiplicity {
    fn pjoin(&self, other: &Self) -> AlgebraicResult<Self> {
        match self.cmp(other) {
            Ordering::Greater => AlgebraicResult::Identity(SELF_IDENT),
            Ordering::Less => AlgebraicResult::Identity(COUNTER_IDENT),
            Ordering::Equal => AlgebraicResult::Identity(SELF_IDENT | COUNTER_IDENT),
        }
    }

    fn pmeet(&self, other: &Self) -> AlgebraicResult<Self> {
        match self.cmp(other) {
            Ordering::Less => AlgebraicResult::Identity(SELF_IDENT),
            Ordering::Greater => AlgebraicResult::Identity(COUNTER_IDENT),
            Ordering::Equal => AlgebraicResult::Identity(SELF_IDENT | COUNTER_IDENT),
        }
    }
}

impl DistributiveLattice for Multiplicity {
    fn psubtract(&self, other: &Self) -> AlgebraicResult<Self> {
        if other.0 >= self.0 {
            AlgebraicResult::None
        } else {
            AlgebraicResult::Element(Self(self.0 - other.0))
        }
    }
}

#[derive(Clone, Default)]
pub struct Bag {
    entries: PathMap<Multiplicity>,
    distinct_len: usize,
    /// Exact internal sum used to keep the pointwise Bag lattice closed even
    /// when several valid `u64` endpoint multiplicities sum past `u64::MAX`.
    /// The public cardinality domain remains the `u64` domain of
    /// `Multiplicity` and projects this value conservatively by saturation.
    exact_cardinality: u128,
}

impl Bag {
    const fn project_cardinality(exact: u128) -> u64 {
        if exact > u64::MAX as u128 {
            u64::MAX
        } else {
            exact as u64
        }
    }

    pub fn new() -> Self { Self::default() }

    pub fn from_pathmap(entries: PathMap<Multiplicity>) -> Self {
        let mut distinct_len = 0usize;
        let mut cardinality = 0u128;
        for (_, multiplicity) in entries.iter() {
            distinct_len += 1;
            cardinality += u128::from(multiplicity.get());
        }
        Self {
            entries,
            distinct_len,
            exact_cardinality: cardinality,
        }
    }

    pub(crate) fn from_pathmap_with_metrics(
        entries: PathMap<Multiplicity>,
        distinct_len: usize,
        cardinality: u128,
    ) -> Self {
        Self {
            entries,
            distinct_len,
            exact_cardinality: cardinality,
        }
    }

    pub fn as_pathmap(&self) -> &PathMap<Multiplicity> { &self.entries }

    pub fn into_pathmap(self) -> PathMap<Multiplicity> { self.entries }

    pub const fn distinct_len(&self) -> usize { self.distinct_len }

    /// Cardinality in the `u64` scalar domain used by `Multiplicity`.
    ///
    /// A Bag remains algebraically closed when distinct endpoint counts sum
    /// beyond that domain. Such an otherwise-valid Bag reports `u64::MAX`
    /// rather than truncating or wrapping; callers can distinguish that rare
    /// case with [`Self::cardinality_is_saturated`].
    pub const fn cardinality(&self) -> u64 { Self::project_cardinality(self.exact_cardinality) }

    pub const fn cardinality_is_saturated(&self) -> bool {
        self.exact_cardinality > u64::MAX as u128
    }

    pub fn is_empty(&self) -> bool { self.distinct_len == 0 }

    pub fn contains(&self, path: &[u8]) -> bool { self.entries.contains(path) }

    pub fn count(&self, path: &[u8]) -> u64 {
        self.entries.get(path).copied().map_or(0, Multiplicity::get)
    }

    pub fn insert(&mut self, path: &[u8]) -> Result<Multiplicity, BagError> {
        self.insert_many(path, Multiplicity::ONE)
    }

    pub fn insert_many(
        &mut self,
        path: &[u8],
        amount: Multiplicity,
    ) -> Result<Multiplicity, BagError> {
        let next = match self.entries.get(path).copied() {
            Some(current) => current.checked_add(amount)?,
            None => {
                self.distinct_len += 1;
                amount
            }
        };
        self.entries.insert(path, next);
        self.exact_cardinality += u128::from(amount.get());
        Ok(next)
    }

    pub fn remove(&mut self, path: &[u8]) -> Option<Multiplicity> {
        self.remove_many(path, Multiplicity::ONE)
    }

    pub fn remove_many(&mut self, path: &[u8], amount: Multiplicity) -> Option<Multiplicity> {
        let current = self.entries.get(path).copied()?;
        let removed = current.get().min(amount.get());
        self.exact_cardinality -= u128::from(removed);
        if removed == current.get() {
            self.entries.remove(path);
            self.distinct_len -= 1;
            None
        } else {
            let next = Multiplicity(current.get() - removed);
            self.entries.insert(path, next);
            Some(next)
        }
    }

    /// Remove exactly `amount`, leaving `self` unchanged when the requested
    /// multiplicity is unavailable.
    pub fn try_remove_many(
        &mut self,
        path: &[u8],
        amount: Multiplicity,
    ) -> Result<Option<Multiplicity>, BagError> {
        let current =
            self.entries
                .get(path)
                .copied()
                .ok_or(BagError::InsufficientMultiplicity {
                    available: 0,
                    requested: amount.get(),
                })?;
        let next = current.checked_sub(amount)?;
        self.exact_cardinality -= u128::from(amount.get());
        match next {
            Some(next) => {
                self.entries.insert(path, next);
                Ok(Some(next))
            }
            None => {
                self.entries.remove(path);
                self.distinct_len -= 1;
                Ok(None)
            }
        }
    }

    pub fn remove_prefix(&mut self, prefix: &[u8]) -> (usize, u64) {
        let mut removed_distinct = 0usize;
        let mut removed_cardinality = 0u128;
        self.visit_prefix(prefix, |_path, multiplicity| {
            removed_distinct += 1;
            removed_cardinality += u128::from(multiplicity.get());
        });
        if removed_distinct != 0 {
            self.entries.remove_branches_at(prefix, true);
            self.entries.remove_val_at(prefix, true);
            self.distinct_len -= removed_distinct;
            self.exact_cardinality -= removed_cardinality;
        }
        (removed_distinct, Self::project_cardinality(removed_cardinality))
    }

    pub fn set_count(&mut self, path: &[u8], count: Option<Multiplicity>) {
        let previous = self.entries.get(path).copied();
        match (previous, count) {
            (None, None) => {}
            (None, Some(next)) => {
                self.entries.insert(path, next);
                self.distinct_len += 1;
                self.exact_cardinality += u128::from(next.get());
            }
            (Some(current), None) => {
                self.entries.remove(path);
                self.distinct_len -= 1;
                self.exact_cardinality -= u128::from(current.get());
            }
            (Some(current), Some(next)) => {
                self.entries.insert(path, next);
                self.exact_cardinality =
                    self.exact_cardinality - u128::from(current.get()) + u128::from(next.get());
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (Vec<u8>, Multiplicity)> + '_ {
        self.entries
            .iter()
            .map(|(path, multiplicity)| (path, *multiplicity))
    }

    pub fn entries_with_prefix(&self, prefix: &[u8]) -> Vec<(Vec<u8>, Multiplicity)> {
        let mut result = Vec::new();
        self.visit_prefix(prefix, |path, multiplicity| {
            result.push((path.to_vec(), multiplicity));
        });
        result
    }

    pub fn visit_prefix(&self, prefix: &[u8], mut visitor: impl FnMut(&[u8], Multiplicity)) {
        let result = self.try_visit_prefix(prefix, |path, multiplicity| {
            visitor(path, multiplicity);
            Ok::<(), std::convert::Infallible>(())
        });
        match result {
            Ok(()) => {}
            Err(error) => match error {},
        }
    }

    pub fn try_visit_prefix<E>(
        &self,
        prefix: &[u8],
        mut visitor: impl FnMut(&[u8], Multiplicity) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut absolute = Vec::new();
        for (relative, multiplicity) in self.entries.read_zipper_at_borrowed_path(prefix) {
            absolute.clear();
            absolute.reserve(prefix.len() + relative.len());
            absolute.extend_from_slice(prefix);
            absolute.extend_from_slice(&relative);
            visitor(&absolute, *multiplicity)?;
        }
        Ok(())
    }

    pub fn sum(&self, other: &Self) -> Result<Self, BagError> {
        let mut result = self.clone();
        for (path, multiplicity) in other.iter() {
            result.insert_many(&path, multiplicity)?;
        }
        Ok(result)
    }

    pub fn join(&self, other: &Self) -> Self {
        let (mut result, updates) = if self.distinct_len >= other.distinct_len {
            (self.clone(), other)
        } else {
            (other.clone(), self)
        };
        for (path, multiplicity) in updates.iter() {
            if multiplicity.get() > result.count(&path) {
                result.set_count(&path, Some(multiplicity));
            }
        }
        result
    }

    pub fn meet(&self, other: &Self) -> Self {
        // PathMap's generic lattice is prefix-aware: a value at `k` can
        // interact with descendants below `k`. Bag membership is an exact-key
        // finite-support function, so its pointwise lattice must use exact
        // trie lookups. Iterate only the smaller support and construct no
        // flattened entry collection.
        let (probe, index) = if self.distinct_len <= other.distinct_len {
            (self, other)
        } else {
            (other, self)
        };
        let mut result = Self::new();
        for (path, multiplicity) in probe.iter() {
            if let Some(counter) = index.entries.get(&path) {
                result.set_count(&path, Some(multiplicity.min(*counter)));
            }
        }
        result
    }

    pub fn subtract(&self, other: &Self) -> Self {
        let mut result = self.clone();
        for (path, removed) in other.iter() {
            let Some(current) = result.entries.get(&path).copied() else {
                continue;
            };
            let next = (current.get() > removed.get())
                .then(|| Multiplicity(current.get() - removed.get()));
            result.set_count(&path, next);
        }
        result
    }

    pub fn is_subbag_of(&self, other: &Self) -> bool {
        self.iter()
            .all(|(path, multiplicity)| other.count(&path) >= multiplicity.get())
    }

    /// Serialize this Bag as PathMap's raw prefix-compressed `ACTree03` arena.
    /// Multiplicities occupy ACTree's native `u64` value slots; there is no
    /// entry list, shadow index, or second value table.
    pub fn compact_bytes(&self) -> Vec<u8> {
        ArenaCompactTree::from_zipper(self.entries.read_zipper(), |value| value.get())
            .get_data()
            .to_vec()
    }

    /// Reconstruct a Bag directly from one canonical `ACTree03` byte image.
    /// The reader is iterative and rejects alternate encodings, zero values,
    /// and value-free endpoints that the Bag API cannot produce.
    pub fn from_compact_bytes(bytes: &[u8]) -> Result<Self, BagCodecError> {
        let trie = CompactTrie::parse_canonical(bytes)?;
        if trie.is_empty() {
            return Ok(Self::new());
        }

        let mut entries = PathMap::new();
        let mut distinct_len = 0usize;
        let mut cardinality = 0u128;
        trie.visit(|path, value| {
            let value = value.ok_or(BagCodecError::MissingMultiplicity)?;
            let multiplicity =
                Multiplicity::new(value).map_err(|_| BagCodecError::ZeroMultiplicity)?;
            entries.insert(path, multiplicity);
            distinct_len += 1;
            cardinality += u128::from(value);
            Ok::<(), BagCodecError>(())
        })?;
        Ok(Self {
            entries,
            distinct_len,
            exact_cardinality: cardinality,
        })
    }

    pub fn merkleize(&mut self) -> MerkleizeResult { self.entries.merkleize() }
}

impl Serialize for Bag {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        serializer.serialize_bytes(&self.compact_bytes())
    }
}

struct BagVisitor;

impl<'de> serde::de::Visitor<'de> for BagVisitor {
    type Value = Bag;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a canonical PathMap ACTree03 Bag byte image")
    }

    fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
    where E: serde::de::Error {
        Bag::from_compact_bytes(bytes).map_err(E::custom)
    }

    fn visit_borrowed_bytes<E>(self, bytes: &'de [u8]) -> Result<Self::Value, E>
    where E: serde::de::Error {
        self.visit_bytes(bytes)
    }

    fn visit_byte_buf<E>(self, bytes: Vec<u8>) -> Result<Self::Value, E>
    where E: serde::de::Error {
        self.visit_bytes(&bytes)
    }
}

impl<'de> Deserialize<'de> for Bag {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        deserializer.deserialize_bytes(BagVisitor)
    }
}

/// Net, reversible multiplicity changes represented by two disjoint Bags.
///
/// Recording an addition cancels an existing removal of the same path first,
/// and recording a removal cancels an existing addition first. Consequently a
/// path is present in at most one side and application performs one atomic
/// PathMap copy-on-write replacement: either every exact removal and checked
/// addition succeeds, or the target Bag is unchanged.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct BagDelta {
    additions: Bag,
    removals: Bag,
}

impl BagDelta {
    pub fn new() -> Self { Self::default() }

    pub fn is_empty(&self) -> bool { self.additions.is_empty() && self.removals.is_empty() }

    pub fn additions(&self) -> &Bag { &self.additions }

    pub fn removals(&self) -> &Bag { &self.removals }

    pub fn record_addition(&mut self, path: &[u8], amount: Multiplicity) -> Result<(), BagError> {
        Self::record_change(&mut self.additions, &mut self.removals, path, amount)
    }

    pub fn record_removal(&mut self, path: &[u8], amount: Multiplicity) -> Result<(), BagError> {
        Self::record_change(&mut self.removals, &mut self.additions, path, amount)
    }

    pub fn inverse(&self) -> Self {
        Self {
            additions: self.removals.clone(),
            removals: self.additions.clone(),
        }
    }

    pub fn apply_to(&self, target: &mut Bag) -> Result<(), BagError> {
        Self::apply_parts(target, &self.additions, &self.removals)
    }

    pub fn revert_from(&self, target: &mut Bag) -> Result<(), BagError> {
        Self::apply_parts(target, &self.removals, &self.additions)
    }

    fn record_change(
        destination: &mut Bag,
        cancellation: &mut Bag,
        path: &[u8],
        amount: Multiplicity,
    ) -> Result<(), BagError> {
        let cancelled = cancellation.count(path).min(amount.get());
        if cancelled != 0 {
            let remaining = amount.get() - cancelled;
            if remaining != 0 {
                let addition =
                    Multiplicity::new(remaining).expect("a positive remainder is a multiplicity");
                if let Some(current) = destination.entries.get(path).copied() {
                    current.checked_add(addition)?;
                }
            }
            cancellation.try_remove_many(
                path,
                Multiplicity::new(cancelled).expect("a positive cancellation is a multiplicity"),
            )?;
            if remaining != 0 {
                destination.insert_many(
                    path,
                    Multiplicity::new(remaining).expect("a positive remainder is a multiplicity"),
                )?;
            }
        } else {
            destination.insert_many(path, amount)?;
        }
        Ok(())
    }

    fn apply_parts(target: &mut Bag, additions: &Bag, removals: &Bag) -> Result<(), BagError> {
        // There is no inter-key prefix to reuse for an empty or singleton
        // delta. Avoid constructing a moving zipper in that case: experiment
        // 219 measured the cursor's fixed setup cost as an 8.3% singleton
        // regression, while the direct exact-key path is already atomic behind
        // this private COW root.
        match additions.distinct_len.saturating_add(removals.distinct_len) {
            0 => return Ok(()),
            1 => {
                let mut updated = target.clone();
                for (path, amount) in removals.iter() {
                    updated.try_remove_many(&path, amount)?;
                }
                for (path, amount) in additions.iter() {
                    updated.insert_many(&path, amount)?;
                }
                *target = updated;
                return Ok(());
            }
            _ => {}
        }

        // Fork exactly one private COW root and retain one write zipper across
        // the delta's lexicographically ordered paths. `move_to_path` ascends
        // only to each consecutive pair's longest common prefix, avoiding a
        // root-to-key replay for every exact update. A deleted leaf is pruned
        // and the zipper ascends to the surviving prefix before continuing.
        //
        // Metrics remain local until the trie update succeeds. Consequently an
        // underflow or overflow discards the private root and leaves `target`
        // byte-for-byte unchanged.
        let mut entries = target.entries.clone();
        let mut distinct_len = target.distinct_len;
        let mut exact_cardinality = target.exact_cardinality;
        {
            let mut cursor = entries.write_zipper();
            for (path, amount) in removals.iter() {
                cursor.move_to_path(&path);
                let current = cursor
                    .val()
                    .copied()
                    .ok_or(BagError::InsufficientMultiplicity {
                        available: 0,
                        requested: amount.get(),
                    })?;
                match current.checked_sub(amount)? {
                    Some(next) => {
                        cursor.set_val(next);
                    }
                    None => {
                        cursor.remove_val(false);
                        cursor.prune_ascend();
                        distinct_len -= 1;
                    }
                }
                exact_cardinality -= u128::from(amount.get());
            }
            for (path, amount) in additions.iter() {
                cursor.move_to_path(&path);
                let next = match cursor.val().copied() {
                    Some(current) => current.checked_add(amount)?,
                    None => {
                        distinct_len += 1;
                        amount
                    }
                };
                cursor.set_val(next);
                exact_cardinality += u128::from(amount.get());
            }
        }
        *target = Bag::from_pathmap_with_metrics(entries, distinct_len, exact_cardinality);
        Ok(())
    }
}

impl PartialEq for Bag {
    fn eq(&self, other: &Self) -> bool {
        self.distinct_len == other.distinct_len &&
            self.exact_cardinality == other.exact_cardinality &&
            self.iter().all(|(path, multiplicity)| {
                other.entries.get(&path).copied() == Some(multiplicity)
            })
    }
}

impl Eq for Bag {}

impl PartialOrd for Bag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for Bag {
    fn cmp(&self, other: &Self) -> Ordering { self.iter().cmp(other.iter()) }
}

impl Hash for Bag {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.distinct_len.hash(state);
        self.exact_cardinality.hash(state);
        for entry in self.iter() {
            entry.hash(state);
        }
    }
}

struct DebugEntries<'a>(&'a Bag);

impl fmt::Debug for DebugEntries<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.iter()).finish()
    }
}

impl fmt::Debug for Bag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bag")
            .field("distinct_len", &self.distinct_len)
            .field("cardinality", &self.cardinality())
            .field("cardinality_is_saturated", &self.cardinality_is_saturated())
            .field("entries", &DebugEntries(self))
            .finish()
    }
}

impl fmt::Debug for BagDelta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BagDelta")
            .field("additions", &self.additions)
            .field("removals", &self.removals)
            .finish()
    }
}

impl Lattice for Bag {
    fn pjoin(&self, other: &Self) -> AlgebraicResult<Self> {
        let result = self.join(other);
        if result == *self && result == *other {
            AlgebraicResult::Identity(SELF_IDENT | COUNTER_IDENT)
        } else if result == *self {
            AlgebraicResult::Identity(SELF_IDENT)
        } else if result == *other {
            AlgebraicResult::Identity(COUNTER_IDENT)
        } else {
            AlgebraicResult::Element(result)
        }
    }

    fn pmeet(&self, other: &Self) -> AlgebraicResult<Self> {
        let result = self.meet(other);
        if result == *self && result == *other {
            AlgebraicResult::Identity(SELF_IDENT | COUNTER_IDENT)
        } else if result == *self {
            AlgebraicResult::Identity(SELF_IDENT)
        } else if result == *other {
            AlgebraicResult::Identity(COUNTER_IDENT)
        } else if result.is_empty() {
            AlgebraicResult::None
        } else {
            AlgebraicResult::Element(result)
        }
    }
}

impl DistributiveLattice for Bag {
    fn psubtract(&self, other: &Self) -> AlgebraicResult<Self> {
        let result = self.subtract(other);
        if result.is_empty() {
            AlgebraicResult::None
        } else if result == *self {
            AlgebraicResult::Identity(SELF_IDENT)
        } else {
            AlgebraicResult::Element(result)
        }
    }
}
