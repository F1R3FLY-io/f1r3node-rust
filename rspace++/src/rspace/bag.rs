use std::cmp::Ordering;
use std::fmt;

use pathmap::PathMap;
use pathmap::merkleization::MerkleizeResult;
use pathmap::ring::{AlgebraicResult, COUNTER_IDENT, DistributiveLattice, Lattice, SELF_IDENT};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BagError {
    ZeroMultiplicity,
    MultiplicityOverflow { current: u64, added: u64 },
}

impl fmt::Display for BagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroMultiplicity => f.write_str("a stored bag multiplicity must be positive"),
            Self::MultiplicityOverflow { current, added } => {
                write!(f, "bag multiplicity overflow: {current} + {added}")
            }
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
        let mut absolute = Vec::new();
        let mut result = Vec::new();
        for (relative, multiplicity) in self.entries.read_zipper_at_borrowed_path(prefix) {
            absolute.clear();
            absolute.reserve(prefix.len() + relative.len());
            absolute.extend_from_slice(prefix);
            absolute.extend_from_slice(&relative);
            result.push((absolute.clone(), *multiplicity));
        }
        result
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

impl fmt::Debug for Bag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bag")
            .field("distinct_len", &self.distinct_len)
            .field("cardinality", &self.cardinality)
            .field("entries", &self.iter().collect::<Vec<_>>())
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
