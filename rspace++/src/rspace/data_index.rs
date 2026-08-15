use std::fmt;

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
}

impl fmt::Display for DataIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bag(error) => error.fmt(f),
            Self::Path(error) => error.fmt(f),
            Self::Decode(error) => error.fmt(f),
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
