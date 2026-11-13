use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use pathmap::ring::{AlgebraicResult, COUNTER_IDENT, DistributiveLattice, Lattice, SELF_IDENT};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::guarded_pathmap::{
    DisposeValue, GuardedPathMap, GuardedReadCursor, GuardedWriteCursor, TeardownBarrier,
};
use super::pathmap_codec::{CanonicalCompactTrieCursor, CompactTrieError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BagError {
    ZeroMultiplicity,
    MultiplicityOverflow { current: u64, added: u64 },
    MultiplicityProductOverflow { current: u64, factor: u64 },
    InsufficientMultiplicity { available: u64, requested: u64 },
    DistinctCountOverflow,
    CardinalityOverflow { current: u128, added: u64 },
}

impl fmt::Display for BagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroMultiplicity => f.write_str("a stored bag multiplicity must be positive"),
            Self::MultiplicityOverflow { current, added } => {
                write!(f, "bag multiplicity overflow: {current} + {added}")
            }
            Self::MultiplicityProductOverflow { current, factor } => {
                write!(f, "bag multiplicity overflow: {current} * {factor}")
            }
            Self::InsufficientMultiplicity {
                available,
                requested,
            } => write!(
                f,
                "insufficient bag multiplicity: requested {requested}, available {available}"
            ),
            Self::DistinctCountOverflow => f.write_str("bag distinct count exceeds usize::MAX"),
            Self::CardinalityOverflow { current, added } => {
                write!(f, "bag cardinality overflow: {current} + {added}")
            }
        }
    }
}

impl std::error::Error for BagError {}

/// Failure at the checked, strictly ascending bulk-construction boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortedUniqueBagError {
    NonAscending,
    DistinctCountOverflow,
    CardinalityOverflow,
}

impl fmt::Display for SortedUniqueBagError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonAscending => {
                formatter.write_str("Bag bulk input is duplicated or not strictly ascending")
            }
            Self::DistinctCountOverflow => {
                formatter.write_str("Bag distinct endpoint count exceeds usize::MAX")
            }
            Self::CardinalityOverflow => {
                formatter.write_str("Bag exact cardinality exceeds u128::MAX")
            }
        }
    }
}

impl std::error::Error for SortedUniqueBagError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniqueBagError {
    DuplicateKey,
    DistinctCountOverflow,
    CardinalityOverflow,
}

impl fmt::Display for UniqueBagError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DuplicateKey => "Bag unique stream repeats an endpoint",
            Self::DistinctCountOverflow => "Bag distinct endpoint count exceeds usize::MAX",
            Self::CardinalityOverflow => "Bag exact cardinality exceeds u128::MAX",
        })
    }
}

impl std::error::Error for UniqueBagError {}

pub struct UniqueBagWriter<'a, 'b> {
    inner: &'a mut SortedUniqueBagWriter<'b>,
}

impl UniqueBagWriter<'_, '_> {
    pub fn push(&mut self, path: &[u8], multiplicity: Multiplicity) -> Result<(), UniqueBagError> {
        self.inner.push_unique(path, multiplicity)
    }
}

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

    pub fn checked_mul(self, factor: u64) -> Result<Option<Self>, BagError> {
        if factor == 0 {
            return Ok(None);
        }
        self.0.checked_mul(factor).map(Self).map(Some).ok_or(
            BagError::MultiplicityProductOverflow {
                current: self.0,
                factor,
            },
        )
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

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MultiplicityDisposer;

impl DisposeValue<Multiplicity> for MultiplicityDisposer {
    #[inline]
    fn dispose(&self, _value: Multiplicity) {}
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MultiplicityBarrier;

impl TeardownBarrier<Multiplicity> for MultiplicityBarrier {
    #[inline]
    fn make_barrier() -> Multiplicity { Multiplicity::ONE }
}

pub(crate) type GuardedMultiplicityMap =
    GuardedPathMap<Multiplicity, MultiplicityDisposer, MultiplicityBarrier>;
pub(crate) type BagReadCursor<'map, 'path> =
    GuardedReadCursor<'map, 'path, Multiplicity, MultiplicityDisposer, MultiplicityBarrier>;

#[derive(Clone)]
pub struct Bag {
    entries: GuardedMultiplicityMap,
    distinct_len: usize,
    /// Exact internal sum used to keep the pointwise Bag lattice closed even
    /// when several valid `u64` endpoint multiplicities sum past `u64::MAX`.
    /// The public cardinality domain remains the `u64` domain of
    /// `Multiplicity` and projects this value conservatively by saturation.
    exact_cardinality: u128,
}

impl Default for Bag {
    fn default() -> Self {
        Self {
            entries: GuardedPathMap::new_with_barrier(MultiplicityDisposer),
            distinct_len: 0,
            exact_cardinality: 0,
        }
    }
}

/// Checked sink for a strictly ascending stream of unique Bag endpoints.
///
/// The sink retains one write zipper and moves it between consecutive paths,
/// so a canonical stream shares each adjacent longest common prefix instead
/// of performing a root-to-leaf `contains`/`get`/`insert` sequence per key.
/// The zipper's current path is also the previous-key order witness, so no
/// copied key or entry collection is materialized.
pub struct SortedUniqueBagWriter<'a> {
    cursor:
        GuardedWriteCursor<'a, 'static, Multiplicity, MultiplicityDisposer, MultiplicityBarrier>,
    has_previous: bool,
    distinct_len: usize,
    exact_cardinality: u128,
}

impl SortedUniqueBagWriter<'_> {
    pub fn push(
        &mut self,
        path: &[u8],
        multiplicity: Multiplicity,
    ) -> Result<(), SortedUniqueBagError> {
        if self.has_previous && self.cursor.path() >= path {
            return Err(SortedUniqueBagError::NonAscending);
        }
        self.push_unique(path, multiplicity)
            .map_err(|error| match error {
                UniqueBagError::DuplicateKey => SortedUniqueBagError::NonAscending,
                UniqueBagError::DistinctCountOverflow => {
                    SortedUniqueBagError::DistinctCountOverflow
                }
                UniqueBagError::CardinalityOverflow => SortedUniqueBagError::CardinalityOverflow,
            })
    }

    fn push_unique(
        &mut self,
        path: &[u8],
        multiplicity: Multiplicity,
    ) -> Result<(), UniqueBagError> {
        let distinct_len = self
            .distinct_len
            .checked_add(1)
            .ok_or(UniqueBagError::DistinctCountOverflow)?;
        let exact_cardinality = self
            .exact_cardinality
            .checked_add(u128::from(multiplicity.get()))
            .ok_or(UniqueBagError::CardinalityOverflow)?;

        self.cursor.move_to_path(path);
        if self.cursor.val().is_some() {
            return Err(UniqueBagError::DuplicateKey);
        }
        self.cursor.set_val(multiplicity);
        self.has_previous = true;
        self.distinct_len = distinct_len;
        self.exact_cardinality = exact_cardinality;
        Ok(())
    }
}

impl Bag {
    fn move_to_next_value(
        cursor: &mut GuardedWriteCursor<
            '_,
            '_,
            Multiplicity,
            MultiplicityDisposer,
            MultiplicityBarrier,
        >,
    ) -> bool {
        loop {
            if cursor.descend_first_byte() {
                if cursor.is_val() {
                    return true;
                }
                if cursor.descend_until() && cursor.is_val() {
                    return true;
                }
            } else {
                loop {
                    if cursor.to_next_sibling_byte() {
                        if cursor.is_val() {
                            return true;
                        }
                        break;
                    }
                    cursor.ascend_byte();
                    if cursor.at_root() {
                        return false;
                    }
                }
            }
        }
    }

    const fn project_cardinality(exact: u128) -> u64 {
        if exact > u64::MAX as u128 {
            u64::MAX
        } else {
            exact as u64
        }
    }

    pub fn new() -> Self { Self::default() }

    pub fn try_from_unique<E>(
        producer: impl FnOnce(&mut UniqueBagWriter<'_, '_>) -> Result<(), E>,
    ) -> Result<Self, E> {
        Self::try_from_sorted_unique(|inner| producer(&mut UniqueBagWriter { inner }))
    }

    /// Construct directly from a producer of strictly ascending unique paths.
    ///
    /// The producer can parse an external stream into a reusable key buffer
    /// and call [`SortedUniqueBagWriter::push`] immediately. This keeps the
    /// complete construction on one moving PathMap zipper and avoids an
    /// intermediate entry vector. If the producer or sink rejects, the private
    /// partial trie is dropped and no Bag is published.
    pub fn try_from_sorted_unique<E>(
        producer: impl FnOnce(&mut SortedUniqueBagWriter<'_>) -> Result<(), E>,
    ) -> Result<Self, E> {
        let mut entries = GuardedPathMap::new_with_barrier(MultiplicityDisposer);
        let (distinct_len, exact_cardinality) = entries.with_write_cursor(|cursor| {
            let mut writer = SortedUniqueBagWriter {
                cursor,
                has_previous: false,
                distinct_len: 0,
                exact_cardinality: 0,
            };
            producer(&mut writer)?;
            Ok((writer.distinct_len, writer.exact_cardinality))
        })?;
        Ok(Self {
            entries,
            distinct_len,
            exact_cardinality,
        })
    }

    pub(crate) fn from_guarded_with_metrics(
        entries: GuardedMultiplicityMap,
        distinct_len: usize,
        cardinality: u128,
    ) -> Self {
        Self {
            entries,
            distinct_len,
            exact_cardinality: cardinality,
        }
    }

    pub(crate) fn with_read_cursor<R>(
        &self,
        operation: impl for<'map> FnOnce(BagReadCursor<'map, 'static>) -> R,
    ) -> R {
        self.entries.with_read_cursor(operation)
    }

    #[cfg(test)]
    pub(crate) fn with_read_cursor_at<'path, R>(
        &self,
        path: &'path [u8],
        operation: impl for<'map> FnOnce(BagReadCursor<'map, 'path>) -> R,
    ) -> R {
        self.entries.with_read_cursor_at(path, operation)
    }

    #[cfg(test)]
    pub(crate) fn prefix_value_count(&self, prefix: &[u8]) -> usize {
        self.with_read_cursor_at(prefix, |cursor| cursor.val_count())
    }

    #[cfg(test)]
    fn prefix_metrics(&self, prefix: &[u8]) -> (usize, u128) {
        self.with_read_cursor_at(prefix, |mut cursor| {
            let mut distinct = usize::from(cursor.val().is_some());
            let mut exact = cursor
                .val()
                .map_or(0, |multiplicity| u128::from(multiplicity.get()));
            while let Some(multiplicity) = cursor.to_next_get_val() {
                distinct += 1;
                exact += u128::from(multiplicity.get());
            }
            (distinct, exact)
        })
    }

    #[cfg(test)]
    pub(crate) fn anchor_identity(&self) -> usize { self.entries.anchor_identity() }

    pub const fn distinct_len(&self) -> usize { self.distinct_len }

    /// Cardinality in the `u64` scalar domain used by `Multiplicity`.
    ///
    /// A Bag remains algebraically closed when distinct endpoint counts sum
    /// beyond that domain. Such an otherwise-valid Bag reports `u64::MAX`
    /// rather than truncating or wrapping; callers can distinguish that rare
    /// case with [`Self::cardinality_is_saturated`].
    pub const fn cardinality(&self) -> u64 { Self::project_cardinality(self.exact_cardinality) }

    /// Exact cardinality for internal algorithms that compare Bag demand with
    /// Bag authority without losing information to the public `u64` saturation
    /// boundary.
    pub(crate) const fn exact_cardinality(&self) -> u128 { self.exact_cardinality }

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

    pub fn get_or_insert(
        &mut self,
        path: &[u8],
        proposed: Multiplicity,
    ) -> Result<(Multiplicity, bool), BagError> {
        if let Some(value) = self.entries.get(path).copied() {
            return Ok((value, false));
        }
        self.get_or_insert_from_cursor(path, proposed)
    }

    pub fn get_or_insert_from_cursor(
        &mut self,
        path: &[u8],
        proposed: Multiplicity,
    ) -> Result<(Multiplicity, bool), BagError> {
        let mut distinct = self.distinct_len;
        let mut cardinality = self.exact_cardinality;
        let result = self.entries.with_write_cursor(|mut cursor| {
            cursor.move_to_path(path);
            if let Some(value) = cursor.val() {
                return Ok((*value, false));
            }
            distinct = distinct
                .checked_add(1)
                .ok_or(BagError::DistinctCountOverflow)?;
            cardinality = cardinality.checked_add(u128::from(proposed.get())).ok_or(
                BagError::CardinalityOverflow {
                    current: cardinality,
                    added: proposed.get(),
                },
            )?;
            cursor.set_val(proposed);
            Ok((proposed, true))
        })?;
        self.distinct_len = distinct;
        self.exact_cardinality = cardinality;
        Ok(result)
    }

    pub fn try_reweight<E>(
        mut self,
        mut replace: impl FnMut(&[u8], Multiplicity) -> Result<Multiplicity, E>,
    ) -> Result<Self, E>
    where
        E: From<BagError>,
    {
        let mut cardinality = 0u128;
        let mut bytes = Vec::new();
        let mut spans = Vec::with_capacity(64);
        let mut resume = Vec::new();
        let mut first = true;
        let mut has_more = true;
        while has_more {
            bytes.clear();
            spans.clear();
            has_more = self.entries.with_read_cursor(|mut cursor| {
                if !first {
                    assert_eq!(cursor.descend_to_existing(&resume), resume.len());
                }
                let mut present = if first {
                    cursor.val().is_some() || cursor.to_next_val()
                } else {
                    cursor.to_next_val()
                };
                while present && spans.len() < 64 {
                    let start = bytes.len();
                    bytes.extend_from_slice(cursor.path());
                    spans.push((start, bytes.len(), *cursor.val().expect("visited Bag endpoint")));
                    present = cursor.to_next_val();
                }
                present
            });
            if spans.is_empty() {
                break;
            }
            let (start, end, _) = spans[spans.len() - 1];
            resume.clear();
            resume.extend_from_slice(&bytes[start..end]);
            self.entries.with_write_cursor(|mut cursor| {
                for &(start, end, prior) in &spans {
                    let path = &bytes[start..end];
                    let value = replace(path, prior)?;
                    cardinality = cardinality.checked_add(u128::from(value.get())).ok_or(
                        BagError::CardinalityOverflow {
                            current: cardinality,
                            added: value.get(),
                        },
                    )?;
                    cursor.move_to_existing_value(path);
                    cursor.replace_copy(value);
                }
                Ok::<_, E>(())
            })?;
            first = false;
        }
        self.exact_cardinality = cardinality;
        Ok(self)
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
        self.entries.set_val_at(path, next);
        self.exact_cardinality += u128::from(amount.get());
        Ok(next)
    }

    pub fn remove(&mut self, path: &[u8]) -> Option<Multiplicity> {
        self.remove_many(path, Multiplicity::ONE)
    }

    pub fn remove_many(&mut self, path: &[u8], amount: Multiplicity) -> Option<Multiplicity> {
        let current = self.entries.get(path).copied()?;
        let removed = current.get().min(amount.get());
        if removed == current.get() {
            let mut removal = Self::new();
            removal
                .insert_many(
                    path,
                    Multiplicity::new(removed).expect("a resident Bag count removes positively"),
                )
                .expect("a singleton removal Bag cannot overflow");
            *self = self.subtract(&removal);
            None
        } else {
            let next = Multiplicity(current.get() - removed);
            let replaced = self.entries.set_val_at(path, next);
            debug_assert!(replaced, "the preflighted Bag endpoint remains in the candidate");
            self.exact_cardinality -= u128::from(removed);
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
        if let Some(next) = next {
            let replaced = self.entries.set_val_at(path, next);
            debug_assert!(replaced, "the preflighted Bag endpoint remains in the candidate");
            self.exact_cardinality -= u128::from(amount.get());
            return Ok(Some(next));
        }
        let mut removal = Self::new();
        removal
            .insert_many(path, amount)
            .expect("a singleton removal Bag cannot overflow");
        *self = self.subtract(&removal);
        Ok(next)
    }

    fn rebuild_without_prefix(&self, prefix: &[u8]) -> (Self, usize, u128) {
        let mut removed_distinct = 0usize;
        let mut removed_cardinality = 0u128;
        let retained = self
            .with_read_cursor(|mut source| {
                Self::try_from_sorted_unique(|writer| {
                    let mut replay = |path: &[u8], multiplicity: Multiplicity| {
                        if path.starts_with(prefix) {
                            removed_distinct = removed_distinct
                                .checked_add(1)
                                .expect("a removed resident endpoint fits usize");
                            removed_cardinality = removed_cardinality
                                .checked_add(u128::from(multiplicity.get()))
                                .expect(
                                    "removed resident multiplicities fit exact u128 cardinality",
                                );
                            Ok(())
                        } else {
                            writer.push(path, multiplicity)
                        }
                    };

                    if let Some(multiplicity) = source.val().copied() {
                        replay(source.path(), multiplicity)?;
                    }
                    while let Some(multiplicity) = source.to_next_get_val().copied() {
                        replay(source.path(), multiplicity)?;
                    }
                    Ok::<(), SortedUniqueBagError>(())
                })
            })
            .expect("a retained canonical Bag stream remains sorted and within resident metrics");
        (retained, removed_distinct, removed_cardinality)
    }

    pub fn remove_prefix(&mut self, prefix: &[u8]) -> (usize, u64) {
        if self.is_empty() || !self.entries.path_exists(prefix) {
            return (0, 0);
        }

        // Structural deletion publishes an independently owned trie.  A
        // PathMap header clone is intentionally insufficient here: destructive
        // pruning may traverse compressed physical nodes shared by immutable
        // versions.  Stream the retained canonical endpoints through one
        // moving writer zipper; no key collection or sorting pass is created.
        let (retained, removed_distinct, removed_cardinality) = self.rebuild_without_prefix(prefix);
        debug_assert_ne!(removed_distinct, 0);
        *self = retained;
        (removed_distinct, Self::project_cardinality(removed_cardinality))
    }

    /// Replace one absolute trie prefix from a prepared channel-relative Bag.
    ///
    /// Preparation is complete before this method is entered.  Publication
    /// therefore contains no fallible semantic step. Structural replacement
    /// first streams retained endpoints into one independently owned trie, then
    /// one prefix-rooted write zipper consumes the prepared Bag's borrowed,
    /// canonical suffix order. Neither complete keys nor entries are projected
    /// into an intermediate collection.
    #[cfg(test)]
    pub(crate) fn replace_prefix_with_relative(
        &mut self,
        prefix: &[u8],
        prepared: &Self,
        clear_existing: bool,
    ) {
        if !clear_existing {
            debug_assert_eq!(
                self.prefix_metrics(prefix).0,
                0,
                "an unloaded channel cannot already own resident data"
            );
            if !prepared.is_empty() {
                prepared.with_read_cursor(|mut source| {
                    self.entries.with_write_cursor_at(prefix, |mut target| {
                        if let Some(multiplicity) = source.val().copied() {
                            debug_assert!(target.val().is_none());
                            target.set_val(multiplicity);
                        }
                        while let Some(multiplicity) = source.to_next_get_val().copied() {
                            target.move_to_path(source.path());
                            debug_assert!(target.val().is_none());
                            target.set_val(multiplicity);
                        }
                    });
                });
            }
            self.distinct_len = self
                .distinct_len
                .checked_add(prepared.distinct_len)
                .expect("resident distinct endpoints fit usize");
            self.exact_cardinality = self
                .exact_cardinality
                .checked_add(prepared.exact_cardinality)
                .expect("resident endpoint multiplicities fit exact u128 cardinality");
            return;
        }

        // Structural replacement preserves the authoritative source until a
        // fresh candidate has been rebuilt and replayed completely.
        let (mut candidate, removed_distinct, removed_exact) = self.rebuild_without_prefix(prefix);
        let next_distinct = self
            .distinct_len
            .checked_sub(removed_distinct)
            .and_then(|retained| retained.checked_add(prepared.distinct_len))
            .expect("resident distinct endpoints fit usize");
        let next_exact = self
            .exact_cardinality
            .checked_sub(removed_exact)
            .and_then(|retained| retained.checked_add(prepared.exact_cardinality))
            .expect("resident endpoint multiplicities fit exact u128 cardinality");
        if !prepared.is_empty() {
            prepared.with_read_cursor(|mut source| {
                candidate
                    .entries
                    .with_write_cursor_at(prefix, |mut target| {
                        if let Some(multiplicity) = source.val().copied() {
                            debug_assert!(target.val().is_none());
                            target.set_val(multiplicity);
                        }
                        while let Some(multiplicity) = source.to_next_get_val().copied() {
                            target.move_to_path(source.path());
                            debug_assert!(target.val().is_none());
                            target.set_val(multiplicity);
                        }
                    });
            });
        }
        candidate.distinct_len = next_distinct;
        candidate.exact_cardinality = next_exact;
        *self = candidate;
    }

    pub fn set_count(&mut self, path: &[u8], count: Option<Multiplicity>) {
        let previous = self.entries.get(path).copied();
        match (previous, count) {
            (None, None) => {}
            (None, Some(next)) => {
                self.entries.set_val_at(path, next);
                self.distinct_len += 1;
                self.exact_cardinality += u128::from(next.get());
            }
            (Some(current), None) => {
                let removed = self.remove_many(path, current);
                debug_assert!(removed.is_none());
            }
            (Some(current), Some(next)) => match next.cmp(&current) {
                Ordering::Less => {
                    let amount = Multiplicity(current.get() - next.get());
                    let remaining = self.remove_many(path, amount);
                    debug_assert_eq!(remaining, Some(next));
                }
                Ordering::Greater => {
                    let amount = Multiplicity(next.get() - current.get());
                    let updated = self
                        .insert_many(path, amount)
                        .expect("the requested absolute Bag count already fits u64");
                    debug_assert_eq!(updated, next);
                }
                Ordering::Equal => {}
            },
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

    /// Visit the suffix of every endpoint below `prefix` without rebuilding
    /// its absolute trie key.
    ///
    /// PathMap's borrowed-prefix zipper already exposes paths relative to the
    /// requested root. Consumers that know the prefix from their schema can
    /// parse that suffix directly and avoid one temporary allocation per
    /// visited endpoint. The callback must not retain the borrowed suffix.
    pub fn visit_relative_prefix(
        &self,
        prefix: &[u8],
        mut visitor: impl FnMut(&[u8], Multiplicity),
    ) {
        let result = self.try_visit_relative_prefix(prefix, |relative, multiplicity| {
            visitor(relative, multiplicity);
            Ok::<(), std::convert::Infallible>(())
        });
        match result {
            Ok(()) => {}
            Err(error) => match error {},
        }
    }

    /// Fallible counterpart to [`Self::visit_relative_prefix`].
    pub fn try_visit_relative_prefix<E>(
        &self,
        prefix: &[u8],
        mut visitor: impl FnMut(&[u8], Multiplicity) -> Result<(), E>,
    ) -> Result<(), E> {
        self.entries.with_read_cursor_at(prefix, |mut cursor| {
            if !cursor.path_exists() {
                return Ok(());
            }
            if let Some(multiplicity) = cursor.val().copied() {
                visitor(cursor.path(), multiplicity)?;
            }
            while let Some(multiplicity) = cursor.to_next_get_val().copied() {
                visitor(cursor.path(), multiplicity)?;
            }
            Ok(())
        })
    }

    pub fn try_visit_prefix<E>(
        &self,
        prefix: &[u8],
        mut visitor: impl FnMut(&[u8], Multiplicity) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut absolute = Vec::new();
        self.try_visit_relative_prefix(prefix, |relative, multiplicity| {
            absolute.clear();
            absolute.reserve(prefix.len() + relative.len());
            absolute.extend_from_slice(prefix);
            absolute.extend_from_slice(relative);
            visitor(&absolute, multiplicity)
        })
    }

    pub fn sum(&self, other: &Self) -> Result<Self, BagError> {
        let mut result = self.clone();
        for (path, multiplicity) in other.iter() {
            result.insert_many(&path, multiplicity)?;
        }
        Ok(result)
    }

    /// Multiply every exact-path multiplicity with one copy-on-write zipper
    /// traversal. A zero factor yields the additive identity; an overflowing
    /// endpoint rejects the entire operation and leaves the source unchanged.
    pub fn scale(&self, factor: u64) -> Result<Self, BagError> {
        match factor {
            0 => return Ok(Self::new()),
            1 => return Ok(self.clone()),
            _ => {}
        }

        let mut entries = self.entries.clone();
        entries.with_write_cursor(|mut cursor| -> Result<(), BagError> {
            if cursor.is_val() {
                let current = cursor
                    .val()
                    .copied()
                    .expect("a value-bearing Bag cursor has a multiplicity");
                cursor.set_val(
                    current
                        .checked_mul(factor)?
                        .expect("a positive factor preserves a positive multiplicity"),
                );
            }
            while Self::move_to_next_value(&mut cursor) {
                let current = cursor
                    .val()
                    .copied()
                    .expect("a Bag value cursor has a multiplicity");
                cursor.set_val(
                    current
                        .checked_mul(factor)?
                        .expect("a positive factor preserves a positive multiplicity"),
                );
            }
            Ok(())
        })?;
        let exact_cardinality = self
            .exact_cardinality
            .checked_mul(u128::from(factor))
            .expect("scaled resident endpoint multiplicities fit exact u128 cardinality");
        Ok(Self::from_guarded_with_metrics(entries, self.distinct_len, exact_cardinality))
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
        let (probe, index) = if self.distinct_len <= other.distinct_len {
            (self, other)
        } else {
            (other, self)
        };
        if probe.distinct_len == 0 {
            return probe.clone();
        }
        if probe.distinct_len == 1 {
            let mut result = Self::new();
            for (path, multiplicity) in probe.iter() {
                let counter = index.count(&path);
                if counter != 0 {
                    result.set_count(
                        &path,
                        Some(multiplicity.min(Multiplicity::new(counter).expect("stored count"))),
                    );
                }
            }
            return result;
        }
        Self::try_from_sorted_unique(|writer| {
            probe.with_read_cursor(|mut cursor| {
                let mut present = cursor.val().is_some() || cursor.to_next_val();
                while present {
                    if let Some(counter) = index.entries.get(cursor.path()) {
                        let multiplicity = *cursor.val().expect("visited Bag endpoint");
                        writer.push(cursor.path(), multiplicity.min(*counter))?;
                    }
                    present = cursor.to_next_val();
                }
                Ok::<(), SortedUniqueBagError>(())
            })
        })
        .expect("ordered resident Bag endpoints form a valid sorted-unique stream")
    }

    pub fn subtract(&self, other: &Self) -> Self {
        if self.is_empty() || other.is_empty() {
            return self.clone();
        }
        if self.distinct_len == 1 {
            let (path, current) = self.iter().next().expect("singleton Bag has one endpoint");
            let removed = other.count(&path);
            if removed == 0 {
                return self.clone();
            }
            if removed >= current.get() {
                return Self::new();
            }
            let mut result = Self::new();
            result.set_count(&path, Some(Multiplicity(current.get() - removed)));
            return result;
        }
        Self::try_from_sorted_unique(|writer| {
            self.with_read_cursor(|mut current| {
                other.with_read_cursor(|mut removals| {
                    let mut current_present = current.val().is_some() || current.to_next_val();
                    let mut removal_present = removals.val().is_some() || removals.to_next_val();
                    while current_present {
                        while removal_present && removals.path() < current.path() {
                            removal_present = removals.to_next_val();
                        }
                        let current_count = *current.val().expect("visited Bag endpoint");
                        let next = if removal_present && removals.path() == current.path() {
                            let removed = *removals.val().expect("visited Bag endpoint");
                            (current_count.get() > removed.get())
                                .then(|| Multiplicity(current_count.get() - removed.get()))
                        } else {
                            Some(current_count)
                        };
                        if let Some(next) = next {
                            writer.push(current.path(), next)?;
                        }
                        current_present = current.to_next_val();
                    }
                    Ok::<(), SortedUniqueBagError>(())
                })
            })
        })
        .expect("ordered resident Bag endpoints form a valid sorted-unique stream")
    }

    pub fn is_subbag_of(&self, other: &Self) -> bool {
        self.with_read_cursor(|mut cursor| {
            let mut present = cursor.val().is_some() || cursor.to_next_val();
            while present {
                let multiplicity = *cursor.val().expect("visited Bag endpoint");
                if other.count(cursor.path()) < multiplicity.get() {
                    return false;
                }
                present = cursor.to_next_val();
            }
            true
        })
    }

    /// Serialize this Bag as PathMap's raw prefix-compressed `ACTree03` arena.
    /// Multiplicities occupy ACTree's native `u64` value slots; there is no
    /// entry list, shadow index, or second value table.
    pub fn compact_bytes(&self) -> Vec<u8> { self.entries.compact_u64_bytes(|value| value.get()) }

    /// Reconstruct a Bag directly from one canonical `ACTree03` byte image.
    /// The reader is iterative and rejects alternate encodings, zero values,
    /// and value-free endpoints that the Bag API cannot produce.
    pub fn from_compact_bytes(bytes: &[u8]) -> Result<Self, BagCodecError> {
        let mut trie = CanonicalCompactTrieCursor::new(bytes)?;
        if trie.is_empty() {
            return Ok(Self::new());
        }

        let mut entries = GuardedPathMap::new_with_barrier(MultiplicityDisposer);
        let (distinct_len, cardinality) =
            entries.with_write_cursor(|mut cursor| -> Result<_, BagCodecError> {
                let mut distinct_len = 0usize;
                let mut cardinality = 0u128;
                while let Some(endpoint) = trie.next_endpoint() {
                    let value = endpoint.value.ok_or(BagCodecError::MissingMultiplicity)?;
                    let multiplicity =
                        Multiplicity::new(value).map_err(|_| BagCodecError::ZeroMultiplicity)?;
                    cursor.move_to_path(endpoint.path);
                    debug_assert!(cursor.val().is_none(), "canonical endpoints are unique");
                    cursor.set_val(multiplicity);
                    distinct_len += 1;
                    cardinality += u128::from(value);
                }
                Ok((distinct_len, cardinality))
            })?;
        Ok(Self {
            entries,
            distinct_len,
            exact_cardinality: cardinality,
        })
    }
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
        self.record_addition_with_prior_removal(path, amount)
            .map(|_| ())
    }

    pub fn record_removal(&mut self, path: &[u8], amount: Multiplicity) -> Result<(), BagError> {
        Self::record_change(&mut self.removals, &mut self.additions, path, amount).map(|_| ())
    }

    pub(crate) fn record_addition_with_prior_removal(
        &mut self,
        path: &[u8],
        amount: Multiplicity,
    ) -> Result<Option<Multiplicity>, BagError> {
        Self::record_change(&mut self.additions, &mut self.removals, path, amount)
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
    ) -> Result<Option<Multiplicity>, BagError> {
        let prior_cancellation = cancellation.count(path);
        let cancelled = prior_cancellation.min(amount.get());
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
        Ok(Multiplicity::new(prior_cancellation).ok())
    }

    fn apply_parts(target: &mut Bag, additions: &Bag, removals: &Bag) -> Result<(), BagError> {
        if additions.is_empty() && removals.is_empty() {
            return Ok(());
        }

        // Preflight every semantic and metadata transition against the
        // immutable source. No mutation occurs until every error path has
        // returned, so failure is strongly atomic.
        let mut distinct_len = target.distinct_len;
        let mut exact_cardinality = target.exact_cardinality;
        let mut has_structural_removal = false;
        for (path, amount) in removals.iter() {
            let current =
                target
                    .entries
                    .get(&path)
                    .copied()
                    .ok_or(BagError::InsufficientMultiplicity {
                        available: 0,
                        requested: amount.get(),
                    })?;
            if current.checked_sub(amount)?.is_none() {
                has_structural_removal = true;
                distinct_len = distinct_len
                    .checked_sub(1)
                    .expect("a resident Bag endpoint contributes one distinct entry");
            }
            exact_cardinality = exact_cardinality
                .checked_sub(u128::from(amount.get()))
                .expect("a resident Bag endpoint contributes its exact multiplicity");
        }
        let mut has_structural_addition = false;
        for (path, amount) in additions.iter() {
            match target.entries.get(&path).copied() {
                Some(current) => {
                    current.checked_add(amount)?;
                }
                None => {
                    has_structural_addition = true;
                    distinct_len = distinct_len
                        .checked_add(1)
                        .ok_or(BagError::DistinctCountOverflow)?;
                }
            }
            exact_cardinality = exact_cardinality
                .checked_add(u128::from(amount.get()))
                .ok_or(BagError::CardinalityOverflow {
                    current: exact_cardinality,
                    added: amount.get(),
                })?;
        }

        // A topology-preserving value update needs no speculative structural
        // candidate after every transition has been preflighted.  Mutate the
        // guarded authority directly: a unique Arc retains its anchor, while
        // Arc::make_mut gives a shared authority one private anchor before the
        // first write.  This is the ownership distinction proved by
        // PathMapPhysicalOwnership.v.
        if !has_structural_removal && !has_structural_addition {
            target.entries.with_write_cursor(|mut cursor| {
                for (path, amount) in removals.iter() {
                    cursor.move_to_path(&path);
                    let current = cursor
                        .val()
                        .copied()
                        .expect("a preflighted value-only removal remains resident");
                    let next = current
                        .checked_sub(amount)
                        .expect("BagDelta removal was preflighted")
                        .expect("a value-only removal preserves its endpoint");
                    cursor.set_val(next);
                }
                for (path, amount) in additions.iter() {
                    cursor.move_to_path(&path);
                    let current = cursor
                        .val()
                        .copied()
                        .expect("a preflighted value-only addition remains resident");
                    cursor.set_val(
                        current
                            .checked_add(amount)
                            .expect("BagDelta addition was preflighted"),
                    );
                }
            });
            target.distinct_len = distinct_len;
            target.exact_cardinality = exact_cardinality;
            return Ok(());
        }

        let mut entries = if has_structural_removal {
            target.subtract(removals).entries
        } else {
            target.entries.clone()
        };
        if !has_structural_removal && !removals.is_empty() {
            entries.with_write_cursor(|mut cursor| {
                for (path, amount) in removals.iter() {
                    cursor.move_to_path(&path);
                    let current = cursor
                        .val()
                        .copied()
                        .expect("a preflighted value-only removal remains resident");
                    let next = current
                        .checked_sub(amount)
                        .expect("BagDelta removal was preflighted")
                        .expect("a value-only removal preserves its endpoint");
                    cursor.set_val(next);
                }
            });
        }
        if !additions.is_empty() {
            entries.with_write_cursor(|mut cursor| {
                for (path, amount) in additions.iter() {
                    cursor.move_to_path(&path);
                    let next = match cursor.val().copied() {
                        Some(current) => current
                            .checked_add(amount)
                            .expect("BagDelta addition was preflighted"),
                        None => amount,
                    };
                    cursor.set_val(next);
                }
            });
        }
        *target = Bag::from_guarded_with_metrics(entries, distinct_len, exact_cardinality);
        Ok(())
    }
}

impl PartialEq for Bag {
    fn eq(&self, other: &Self) -> bool {
        self.distinct_len == other.distinct_len &&
            self.exact_cardinality == other.exact_cardinality &&
            self.cmp(other).is_eq()
    }
}

impl Eq for Bag {}

impl PartialOrd for Bag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for Bag {
    fn cmp(&self, other: &Self) -> Ordering {
        self.with_read_cursor(|mut left| {
            other.with_read_cursor(|mut right| {
                let mut left_present = left.val().is_some() || left.to_next_val();
                let mut right_present = right.val().is_some() || right.to_next_val();
                loop {
                    match (left_present, right_present) {
                        (false, false) => return Ordering::Equal,
                        (false, true) => return Ordering::Less,
                        (true, false) => return Ordering::Greater,
                        (true, true) => {}
                    }
                    let left_count = left.val().expect("visited Bag endpoint");
                    let right_count = right.val().expect("visited Bag endpoint");
                    let ordering = left
                        .path()
                        .cmp(right.path())
                        .then_with(|| left_count.cmp(right_count));
                    match ordering {
                        Ordering::Equal => {
                            left_present = left.to_next_val();
                            right_present = right.to_next_val();
                        }
                        unequal => return unequal,
                    }
                }
            })
        })
    }
}

impl Hash for Bag {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.distinct_len.hash(state);
        self.exact_cardinality.hash(state);
        self.with_read_cursor(|mut cursor| {
            let mut present = cursor.val().is_some() || cursor.to_next_val();
            while present {
                cursor.path().hash(state);
                cursor.val().expect("visited Bag endpoint").hash(state);
                present = cursor.to_next_val();
            }
        });
    }
}

struct DebugEntries<'a>(&'a Bag);

impl fmt::Debug for DebugEntries<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.with_read_cursor(|mut cursor| {
            let mut entries = f.debug_map();
            let mut present = cursor.val().is_some() || cursor.to_next_val();
            while present {
                entries.entry(&cursor.path(), cursor.val().expect("visited Bag endpoint"));
                present = cursor.to_next_val();
            }
            entries.finish()
        })
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

#[cfg(test)]
mod prefix_replacement_tests {
    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::{Bag, Multiplicity};

    fn add(bag: &mut Bag, path: &[u8], amount: u8) {
        bag.insert_many(path, Multiplicity::new(u64::from(amount)).unwrap())
            .unwrap();
    }

    proptest! {
        #[test]
        fn relative_prefix_publication_matches_an_independent_bag_rebuild(
            outside in vec((any::<u8>(), 1u8..5), 0..32),
            old_inside in vec((any::<u8>(), 1u8..5), 0..32),
            prepared_entries in vec((any::<u8>(), 1u8..5), 0..32),
        ) {
            let prefix = [0xAA, 0x00];
            let mut actual = Bag::new();
            let mut expected = Bag::new();
            for (key, amount) in outside {
                add(&mut actual, &[0xBB, key], amount);
                add(&mut expected, &[0xBB, key], amount);
            }
            for (key, amount) in old_inside {
                add(&mut actual, &[prefix[0], prefix[1], key], amount);
            }
            let mut prepared = Bag::new();
            for (key, amount) in prepared_entries {
                add(&mut prepared, &[key], amount);
                add(&mut expected, &[prefix[0], prefix[1], key], amount);
            }

            actual.replace_prefix_with_relative(&prefix, &prepared, true);

            prop_assert_eq!(actual, expected);
        }
    }

    #[test]
    fn new_empty_channel_publication_changes_no_resident_endpoint() {
        let mut actual = Bag::new();
        actual.insert(b"outside").unwrap();
        let before = actual.clone();
        actual.replace_prefix_with_relative(b"new-channel", &Bag::new(), false);
        assert_eq!(actual, before);
    }

    #[test]
    fn empty_replacement_publishes_metrics_and_leaves_the_live_root_write_ready() {
        let maximum = Multiplicity::new(u64::MAX).unwrap();
        let mut actual = Bag::new();
        actual.insert_many(b"target/old", maximum).unwrap();

        actual.replace_prefix_with_relative(b"target/", &Bag::new(), true);

        assert_eq!(actual.distinct_len(), 0);
        assert_eq!(actual.exact_cardinality(), 0);
        assert_eq!(actual.count(b"target/old"), 0);
        assert!(actual.insert(b"replacement").is_ok());
        assert_eq!(actual.distinct_len(), 1);
        assert_eq!(actual.exact_cardinality(), 1);
        assert_eq!(actual.count(b"replacement"), 1);
    }

    #[test]
    fn prefix_replacement_preflights_exact_metrics_at_u64_boundaries() {
        let maximum = Multiplicity::new(u64::MAX).unwrap();
        let mut actual = Bag::new();
        actual.insert_many(b"outside", maximum).unwrap();
        actual.insert_many(b"target/old", maximum).unwrap();
        let mut prepared = Bag::new();
        prepared.insert_many(b"new", maximum).unwrap();

        actual.replace_prefix_with_relative(b"target/", &prepared, true);

        assert_eq!(actual.distinct_len(), 2);
        assert_eq!(actual.exact_cardinality(), u128::from(u64::MAX) * 2);
        assert_eq!(actual.count(b"outside"), u64::MAX);
        assert_eq!(actual.count(b"target/new"), u64::MAX);
        assert_eq!(actual.count(b"target/old"), 0);
    }
}

#[cfg(test)]
#[path = "../../tests/bag/cow_root_publication.rs"]
mod cow_root_publication_tests;

#[cfg(test)]
mod reweight_tests {
    use super::*;

    #[test]
    fn reweight_preserves_keys_snapshot_and_exact_cardinality() {
        let mut source = Bag::new();
        for (path, count) in [(b"a".as_slice(), 1), (b"ab".as_slice(), 2), (b"z".as_slice(), 3)] {
            source
                .insert_many(path, Multiplicity::new(count).unwrap())
                .unwrap();
        }
        let snapshot = source.clone();
        let result = source
            .try_reweight(|_, id| Multiplicity::new(u64::MAX - id.get()))
            .unwrap();
        assert_eq!(result.distinct_len(), 3);
        assert_eq!(result.exact_cardinality(), 3 * u128::from(u64::MAX) - 6);
        assert_eq!(snapshot.exact_cardinality(), 6);
        for (path, id) in [(b"a".as_slice(), 1), (b"ab".as_slice(), 2), (b"z".as_slice(), 3)] {
            assert_eq!(snapshot.count(path), id);
            assert_eq!(result.count(path), u64::MAX - id);
        }
    }

    #[test]
    fn rejected_partial_reweight_publishes_no_result_or_snapshot_change() {
        let mut source = Bag::new();
        source.insert(b"a").unwrap();
        source
            .insert_many(b"z", Multiplicity::new(2).unwrap())
            .unwrap();
        let snapshot = source.clone();
        let error = source
            .try_reweight(|_, id| {
                if id.get() == 2 {
                    Err(BagError::ZeroMultiplicity)
                } else {
                    Multiplicity::new(9)
                }
            })
            .err()
            .unwrap();
        assert_eq!(error, BagError::ZeroMultiplicity);
        assert_eq!(snapshot.count(b"a"), 1);
        assert_eq!(snapshot.count(b"z"), 2);
        assert_eq!(snapshot.exact_cardinality(), 3);
    }

    #[test]
    fn reweight_resumes_across_bounded_chunks_in_key_order() {
        let mut source = Bag::new();
        source.insert(b"").unwrap();
        source.insert(b"k").unwrap();
        for index in 0..130 {
            source.insert(format!("k-{index:03}").as_bytes()).unwrap();
        }
        let mut visited = Vec::new();
        let result = source
            .try_reweight(|path, prior| {
                visited.push(path.to_vec());
                Multiplicity::new(prior.get() + 1)
            })
            .unwrap();
        assert_eq!(visited.len(), 132);
        assert!(visited.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(result.distinct_len(), 132);
        assert_eq!(result.exact_cardinality(), 264);
        for key in visited {
            assert_eq!(result.count(&key), 2);
        }
    }
}
