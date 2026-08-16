use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use pathmap::PathMap;
use pathmap::merkleization::MerkleizeResult;
use pathmap::ring::{AlgebraicResult, COUNTER_IDENT, DistributiveLattice, Lattice, SELF_IDENT};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
    cardinality: u128,
}

impl Bag {
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
            cardinality,
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
            cardinality,
        }
    }

    pub fn as_pathmap(&self) -> &PathMap<Multiplicity> { &self.entries }

    pub fn into_pathmap(self) -> PathMap<Multiplicity> { self.entries }

    pub const fn distinct_len(&self) -> usize { self.distinct_len }

    pub const fn cardinality(&self) -> u128 { self.cardinality }

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
        self.cardinality += u128::from(amount.get());
        Ok(next)
    }

    pub fn remove(&mut self, path: &[u8]) -> Option<Multiplicity> {
        self.remove_many(path, Multiplicity::ONE)
    }

    pub fn remove_many(&mut self, path: &[u8], amount: Multiplicity) -> Option<Multiplicity> {
        let current = self.entries.get(path).copied()?;
        let removed = current.get().min(amount.get());
        self.cardinality -= u128::from(removed);
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
        self.cardinality -= u128::from(amount.get());
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

    pub fn remove_prefix(&mut self, prefix: &[u8]) -> (usize, u128) {
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
            self.cardinality -= removed_cardinality;
        }
        (removed_distinct, removed_cardinality)
    }

    pub fn set_count(&mut self, path: &[u8], count: Option<Multiplicity>) {
        let previous = self.entries.get(path).copied();
        match (previous, count) {
            (None, None) => {}
            (None, Some(next)) => {
                self.entries.insert(path, next);
                self.distinct_len += 1;
                self.cardinality += u128::from(next.get());
            }
            (Some(current), None) => {
                self.entries.remove(path);
                self.distinct_len -= 1;
                self.cardinality -= u128::from(current.get());
            }
            (Some(current), Some(next)) => {
                self.entries.insert(path, next);
                self.cardinality =
                    self.cardinality - u128::from(current.get()) + u128::from(next.get());
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
        Self::from_pathmap(self.entries.join(&other.entries))
    }

    pub fn meet(&self, other: &Self) -> Self {
        Self::from_pathmap(self.entries.meet(&other.entries))
    }

    pub fn subtract(&self, other: &Self) -> Self {
        Self::from_pathmap(self.entries.subtract(&other.entries))
    }

    pub fn is_subbag_of(&self, other: &Self) -> bool {
        self.iter()
            .all(|(path, multiplicity)| other.count(&path) >= multiplicity.get())
    }

    pub fn merkleize(&mut self) -> MerkleizeResult { self.entries.merkleize() }
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
        for (path, requested) in removals.iter() {
            let available = target.count(&path);
            if available < requested.get() {
                return Err(BagError::InsufficientMultiplicity {
                    available,
                    requested: requested.get(),
                });
            }
        }
        let reduced = target.subtract(removals);
        let updated = reduced.sum(additions)?;
        *target = updated;
        Ok(())
    }
}

impl PartialEq for Bag {
    fn eq(&self, other: &Self) -> bool {
        self.distinct_len == other.distinct_len &&
            self.cardinality == other.cardinality &&
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
        self.cardinality.hash(state);
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
            .field("cardinality", &self.cardinality)
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
