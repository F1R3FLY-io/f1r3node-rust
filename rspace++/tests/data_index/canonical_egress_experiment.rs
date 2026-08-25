//! Frozen control and production treatment for RSpace DATA checkpoint egress.
//!
//! This module is compiled only by `canonical-egress-experiment`. Keeping both
//! arms beside `RSpaceDataStore` lets the legacy control call the actual typed
//! `loaded_records` path and the treatment call the private production
//! `loaded_canonical_records` path. The benchmark therefore cannot drift into
//! a reconstructed approximation of either algorithm.

use super::*;
use crate::rspace::hashing::stable_hash_provider::hash;
use crate::rspace::serializers::serializers::encode_datums;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalEgressArm {
    LegacyTyped,
    CanonicalArena,
}

impl CanonicalEgressArm {
    pub const ALL: [Self; 2] = [Self::LegacyTyped, Self::CanonicalArena];

    pub const fn label(self) -> &'static str {
        match self {
            Self::LegacyTyped => "legacy_typed",
            Self::CanonicalArena => "canonical_arena",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct CanonicalEgressOutcome {
    pub channel_hash: Blake2b256Hash,
    pub encoded_datums: Vec<u8>,
    pub datum_count: u64,
}

impl CanonicalEgressOutcome {
    pub fn digest_hex(&self) -> String {
        hex::encode(Blake2b256Hash::new(&self.encoded_datums).bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefixShape {
    Shared,
    Unshared,
}

impl PrefixShape {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Unshared => "unshared",
        }
    }
}

pub struct CanonicalEgressFixture {
    store: RSpaceDataStore<String, String>,
    distinct: usize,
    multiplicity: u64,
    shape: PrefixShape,
}

impl CanonicalEgressFixture {
    pub fn new(distinct: usize, multiplicity: u64, shape: PrefixShape) -> Self {
        assert!(distinct > 0, "the egress fixture must contain a datum");
        assert!(multiplicity > 0, "the egress fixture multiplicity must be positive");
        let channel = "canonical-egress-benchmark".to_string();
        let mut store = RSpaceDataStore::new();
        for index in 0..distinct {
            let value = match shape {
                PrefixShape::Shared => {
                    format!("shared-prefix/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/{index:016x}")
                }
                PrefixShape::Unshared => {
                    let digest = Blake2b256Hash::new(&(index as u64).to_le_bytes());
                    hex::encode(digest.bytes())
                }
            };
            let datum = Datum::create(&channel, value, false);
            for _ in 0..multiplicity {
                store
                    .insert(&channel, &datum)
                    .expect("benchmark datum insertion");
            }
        }
        let fixture = Self {
            store,
            distinct,
            multiplicity,
            shape,
        };
        fixture.audit();
        fixture
    }

    pub const fn distinct(&self) -> usize { self.distinct }

    pub const fn multiplicity(&self) -> u64 { self.multiplicity }

    pub const fn shape(&self) -> PrefixShape { self.shape }

    pub fn logical_cardinality(&self) -> u64 {
        u64::try_from(self.distinct)
            .expect("benchmark distinct count fits u64")
            .checked_mul(self.multiplicity)
            .expect("preregistered benchmark cardinality fits u64")
    }

    pub fn run(&self, arm: CanonicalEgressArm) -> CanonicalEgressOutcome {
        match arm {
            CanonicalEgressArm::LegacyTyped => self.legacy_typed(),
            CanonicalEgressArm::CanonicalArena => self.canonical_arena(),
        }
    }

    pub fn audit(&self) {
        let legacy = self.legacy_typed();
        let canonical = self.canonical_arena();
        assert_eq!(canonical, legacy, "benchmark arms must be byte-identical");
        assert_eq!(canonical.datum_count, self.logical_cardinality());
    }

    fn legacy_typed(&self) -> CanonicalEgressOutcome {
        let mut records = self
            .store
            .loaded_records()
            .expect("legacy typed checkpoint projection");
        assert_eq!(records.len(), 1, "benchmark fixture has one channel");
        let (channel, data) = records.pop().expect("one legacy record");
        CanonicalEgressOutcome {
            channel_hash: hash(&channel),
            encoded_datums: encode_datums(&data),
            datum_count: u64::try_from(data.len()).expect("benchmark cardinality fits u64"),
        }
    }

    fn canonical_arena(&self) -> CanonicalEgressOutcome {
        let mut records = self
            .store
            .loaded_canonical_records()
            .expect("canonical checkpoint projection");
        assert_eq!(records.len(), 1, "benchmark fixture has one channel");
        let record = records.pop().expect("one canonical record");
        CanonicalEgressOutcome {
            channel_hash: record.channel_hash,
            encoded_datums: record.encoded_datums,
            datum_count: record.datum_count,
        }
    }
}
