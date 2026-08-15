use std::fmt;
use std::marker::PhantomData;

use pathmap::PathMap;
use serde::Serialize;

use super::bag::{Bag, BagError, Multiplicity};
use super::candidate_order::deterministic_candidate_hash;
use super::internal::Datum;
use super::rho_path::{RhoPathEncode, RhoPathError, RhoPathV1};
use super::serializers::cold_store_decode::{ColdStoreDecode, ColdStoreDecodeError};
use super::serializers::serializers::{CandidateOrderingBytes, decode_datum, encode_datum};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataIndexError {
    Bag(BagError),
    Path(RhoPathError),
    Decode(ColdStoreDecodeError),
    CardinalityOverflow(u128),
    AllocationFailure(usize),
    IndexOutOfBounds { index: i32, cardinality: u128 },
}

impl fmt::Display for DataIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bag(error) => error.fmt(f),
            Self::Path(error) => error.fmt(f),
            Self::Decode(error) => error.fmt(f),
            Self::CardinalityOverflow(cardinality) => {
                write!(f, "bag cardinality {cardinality} does not fit in usize")
            }
            Self::AllocationFailure(requested) => {
                write!(f, "unable to reserve space for {requested} bag elements")
            }
            Self::IndexOutOfBounds { index, cardinality } => {
                write!(f, "datum index {index} is out of bounds for cardinality {cardinality}")
            }
        }
    }
}

impl std::error::Error for DataIndexError {}

impl From<BagError> for DataIndexError {
    fn from(error: BagError) -> Self { Self::Bag(error) }
}

impl From<RhoPathError> for DataIndexError {
    fn from(error: RhoPathError) -> Self { Self::Path(error) }
}

impl From<ColdStoreDecodeError> for DataIndexError {
    fn from(error: ColdStoreDecodeError) -> Self { Self::Decode(error) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedDatum<A: Clone> {
    pub path: Vec<u8>,
    pub datum: Datum<A>,
    pub multiplicity: Multiplicity,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RSpaceDataIndex {
    bag: Bag,
}

impl RSpaceDataIndex {
    pub fn new() -> Self { Self::default() }

    pub fn from_bag(bag: Bag) -> Self { Self { bag } }

    pub fn as_bag(&self) -> &Bag { &self.bag }

    pub fn into_bag(self) -> Bag { self.bag }

    pub fn distinct_len(&self) -> usize { self.bag.distinct_len() }

    pub fn cardinality(&self) -> u128 { self.bag.cardinality() }

    pub fn is_empty(&self) -> bool { self.bag.is_empty() }

    pub fn clear_channel<C: RhoPathEncode>(&mut self, channel: &C) -> (usize, u128) {
        self.bag.remove_prefix(&RhoPathV1::datum_prefix(channel))
    }

    pub fn datum_path<C, A>(channel: &C, datum: &Datum<A>) -> Vec<u8>
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        RhoPathV1::datum_key(channel, &datum.a.rho_path_structure(), &encode_datum(datum))
    }

    pub fn insert<C, A>(
        &mut self,
        channel: &C,
        datum: &Datum<A>,
    ) -> Result<(Vec<u8>, Multiplicity), DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        let path = Self::datum_path(channel, datum);
        let multiplicity = self.bag.insert(&path)?;
        Ok((path, multiplicity))
    }

    pub fn count<C, A>(&self, channel: &C, datum: &Datum<A>) -> u64
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        self.bag.count(&Self::datum_path(channel, datum))
    }

    pub fn count_path(&self, path: &[u8]) -> u64 { self.bag.count(path) }

    pub fn remove_path(&mut self, path: &[u8]) -> Option<Multiplicity> { self.bag.remove(path) }

    pub fn channel_entries<C, A>(&self, channel: &C) -> Result<Vec<IndexedDatum<A>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode,
    {
        let prefix = RhoPathV1::datum_prefix(channel);
        self.entries_at_prefix(&prefix)
    }

    pub fn structure_prefix_entries<C, A>(
        &self,
        channel: &C,
        structure_prefix: &[u8],
    ) -> Result<Vec<IndexedDatum<A>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode,
    {
        let prefix = RhoPathV1::datum_structure_prefix(channel, structure_prefix);
        self.entries_at_prefix(&prefix)
    }

    pub fn ordered_channel_entries<C, A>(
        &self,
        channel: &C,
    ) -> Result<Vec<IndexedDatum<A>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        let entries = self.channel_entries(channel)?;
        let mut decorated = entries
            .into_iter()
            .map(|entry| (deterministic_candidate_hash(&entry.datum), entry))
            .collect::<Vec<_>>();
        decorated.sort_unstable_by(|(left_hash, left), (right_hash, right)| {
            left_hash
                .cmp(right_hash)
                .then_with(|| left.path.cmp(&right.path))
        });
        Ok(decorated.into_iter().map(|(_, entry)| entry).collect())
    }

    fn entries_at_prefix<A>(&self, prefix: &[u8]) -> Result<Vec<IndexedDatum<A>>, DataIndexError>
    where A: Clone + ColdStoreDecode {
        let mut entries = Vec::new();
        self.bag.try_visit_prefix(prefix, |path, multiplicity| {
            let datum = RhoPathV1::parse_datum(path)
                .map_err(DataIndexError::from)
                .and_then(|view| decode_datum(view.datum_identity()).map_err(DataIndexError::from));
            entries.push(IndexedDatum {
                path: path.to_vec(),
                datum: datum?,
                multiplicity,
            });
            Ok::<(), DataIndexError>(())
        })?;
        Ok(entries)
    }
}

pub struct RSpaceDataStore<C, A> {
    index: RSpaceDataIndex,
    loaded_channels: PathMap<()>,
    loaded_channel_count: usize,
    marker: PhantomData<fn() -> (C, A)>,
}

impl<C, A> Clone for RSpaceDataStore<C, A> {
    fn clone(&self) -> Self {
        Self {
            index: self.index.clone(),
            loaded_channels: self.loaded_channels.clone(),
            loaded_channel_count: self.loaded_channel_count,
            marker: PhantomData,
        }
    }
}

impl<C, A> Default for RSpaceDataStore<C, A> {
    fn default() -> Self {
        Self {
            index: RSpaceDataIndex::new(),
            loaded_channels: PathMap::new(),
            loaded_channel_count: 0,
            marker: PhantomData,
        }
    }
}

impl<C, A> RSpaceDataStore<C, A> {
    pub fn new() -> Self { Self::default() }

    pub fn index(&self) -> &RSpaceDataIndex { &self.index }

    pub fn loaded_channel_count(&self) -> usize { self.loaded_channel_count }

    pub fn distinct_len(&self) -> usize { self.index.distinct_len() }

    pub fn cardinality(&self) -> u128 { self.index.cardinality() }

    pub fn is_empty(&self) -> bool { self.loaded_channel_count == 0 }

    pub fn clear(&mut self) {
        self.index = RSpaceDataIndex::new();
        self.loaded_channels = PathMap::new();
        self.loaded_channel_count = 0;
    }

    pub fn is_loaded(&self, channel: &C) -> bool
    where C: RhoPathEncode {
        self.loaded_channels
            .contains(RhoPathV1::datum_prefix(channel))
    }

    pub fn set_channel(
        &mut self,
        channel: C,
        datums: impl IntoIterator<Item = Datum<A>>,
    ) -> Result<(), DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        let mut replacement = self.clone();
        replacement.index.clear_channel(&channel);
        let marker = RhoPathV1::datum_prefix(&channel);
        if replacement.loaded_channels.insert(&marker, ()).is_none() {
            replacement.loaded_channel_count += 1;
        }
        for datum in datums {
            replacement.index.insert(&channel, &datum)?;
        }
        *self = replacement;
        Ok(())
    }

    pub fn insert(&mut self, channel: &C, datum: &Datum<A>) -> Result<Multiplicity, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        let (_, multiplicity) = self.index.insert(channel, datum)?;
        let marker = RhoPathV1::datum_prefix(channel);
        if self.loaded_channels.insert(&marker, ()).is_none() {
            self.loaded_channel_count += 1;
        }
        Ok(multiplicity)
    }

    pub fn channel_entries(
        &self,
        channel: &C,
    ) -> Result<Option<Vec<IndexedDatum<A>>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        if self.is_loaded(channel) {
            self.index.ordered_channel_entries(channel).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn channel_data(&self, channel: &C) -> Result<Option<Vec<Datum<A>>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        self.channel_entries(channel)?
            .map(expand_entries)
            .transpose()
    }

    pub fn remove_index(&mut self, channel: &C, index: i32) -> Result<(), DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        let entries = self.channel_entries(channel)?.unwrap_or_default();
        let cardinality = channel_cardinality(&entries);
        let mut remaining = u64::try_from(index)
            .map_err(|_| DataIndexError::IndexOutOfBounds { index, cardinality })?;
        for entry in entries {
            if remaining < entry.multiplicity.get() {
                self.index.remove_path(&entry.path);
                return Ok(());
            }
            remaining -= entry.multiplicity.get();
        }
        Err(DataIndexError::IndexOutOfBounds { index, cardinality })
    }

    pub fn loaded_records(&self) -> Result<Vec<(C, Vec<Datum<A>>)>, DataIndexError>
    where
        C: Clone + ColdStoreDecode + RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        let mut records = Vec::with_capacity(self.loaded_channel_count);
        for (path, _) in self.loaded_channels.iter() {
            let view = RhoPathV1::parse_datum_prefix(&path)?;
            let channel = C::cold_decode(&view.channel_identity())?;
            let datums = self.channel_data(&channel)?.unwrap_or_default();
            records.push((channel, datums));
        }
        Ok(records)
    }
}

impl<C, A> fmt::Debug for RSpaceDataStore<C, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RSpaceDataStore")
            .field("loaded_channel_count", &self.loaded_channel_count)
            .field("distinct_len", &self.distinct_len())
            .field("cardinality", &self.cardinality())
            .finish()
    }
}

impl<C, A> PartialEq for RSpaceDataStore<C, A> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index &&
            self.loaded_channel_count == other.loaded_channel_count &&
            self.loaded_channels
                .iter()
                .all(|(path, _)| other.loaded_channels.contains(&path))
    }
}

impl<C, A> Eq for RSpaceDataStore<C, A> {}

fn channel_cardinality<A: Clone>(entries: &[IndexedDatum<A>]) -> u128 {
    entries
        .iter()
        .map(|entry| u128::from(entry.multiplicity.get()))
        .sum()
}

fn expand_entries<A: Clone>(
    entries: Vec<IndexedDatum<A>>,
) -> Result<Vec<Datum<A>>, DataIndexError> {
    let cardinality = channel_cardinality(&entries);
    let capacity = usize::try_from(cardinality)
        .map_err(|_| DataIndexError::CardinalityOverflow(cardinality))?;
    let mut datums = Vec::new();
    datums
        .try_reserve_exact(capacity)
        .map_err(|_| DataIndexError::AllocationFailure(capacity))?;
    for entry in entries {
        for _ in 0..entry.multiplicity.get() {
            datums.push(entry.datum.clone());
        }
    }
    Ok(datums)
}
