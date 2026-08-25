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
    Opposed,
}

impl PrefixShape {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Unshared => "unshared",
            Self::Opposed => "opposed",
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
                PrefixShape::Opposed => {
                    let width = 1 + index % 251;
                    format!("{:04x}/{}", distinct - index, "x".repeat(width))
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

/// Experimental per-channel index whose relative keys are complete canonical
/// datum identities. A production multi-channel form would prepend one
/// versioned, prefix-free channel identity frame and retain this exact suffix
/// order; it would not allocate one PathMap or one persistence object per
/// channel.
pub struct IdentityOrderFixture {
    control: CanonicalEgressFixture,
    identity_order: IdentityOrderIndex,
    channel: String,
    channel_prefix: Vec<u8>,
    extra_datum: Datum<String>,
    extra_identity: Vec<u8>,
}

#[derive(Clone)]
struct IdentityOrderIndex {
    bag: Bag,
    /// Exact legacy `Vec<Vec<u8>>` length, including its outer `u64` count.
    /// Maintaining this alongside multiplicity makes trusted egress one
    /// zipper pass without weakening preflight-before-publication.
    encoded_len: u128,
}

impl IdentityOrderIndex {
    fn new() -> Self {
        Self {
            bag: Bag::new(),
            encoded_len: u128::from(u64::BITS / 8),
        }
    }

    fn cardinality(&self) -> u64 { self.bag.cardinality() }

    fn insert_many(
        &mut self,
        identity: &[u8],
        multiplicity: Multiplicity,
    ) -> Result<(), DataIndexError> {
        let added_len = framed_identity_bytes(identity, multiplicity)?;
        let next_len = self
            .encoded_len
            .checked_add(added_len)
            .ok_or(DataIndexError::EncodedLengthOverflow(u128::MAX))?;
        self.bag.insert_many(identity, multiplicity)?;
        self.encoded_len = next_len;
        Ok(())
    }

    fn insert(&mut self, identity: &[u8]) -> Result<(), DataIndexError> {
        self.insert_many(identity, Multiplicity::ONE)
    }

    fn remove(&mut self, identity: &[u8]) -> Result<(), DataIndexError> {
        if self.bag.count(identity) == 0 {
            return Ok(());
        }
        let removed_len = framed_identity_bytes(identity, Multiplicity::ONE)?;
        self.bag.remove(identity);
        self.encoded_len = self
            .encoded_len
            .checked_sub(removed_len)
            .expect("identity length metadata must cover every indexed occurrence");
        Ok(())
    }
}

fn framed_identity_bytes(
    identity: &[u8],
    multiplicity: Multiplicity,
) -> Result<u128, DataIndexError> {
    u128::from(u64::BITS / 8)
        .checked_add(identity.len() as u128)
        .and_then(|framed| framed.checked_mul(u128::from(multiplicity.get())))
        .ok_or(DataIndexError::EncodedLengthOverflow(u128::MAX))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityOrderArm {
    ArenaSort,
    MaintainedPathMap,
}

impl IdentityOrderArm {
    pub const ALL: [Self; 2] = [Self::ArenaSort, Self::MaintainedPathMap];

    pub const fn label(self) -> &'static str {
        match self {
            Self::ArenaSort => "arena_sort",
            Self::MaintainedPathMap => "identity_pathmap",
        }
    }
}

impl IdentityOrderFixture {
    pub fn new(distinct: usize, multiplicity: u64, shape: PrefixShape) -> Self {
        let control = CanonicalEgressFixture::new(distinct, multiplicity, shape);
        let channel = "canonical-egress-benchmark".to_string();
        let channel_prefix = RhoPathV1::datum_prefix(&channel);
        let identity_order = build_identity_order_index(&control.store, &channel_prefix)
            .expect("valid benchmark authority must build an identity-order index");
        if shape == PrefixShape::Opposed {
            assert!(
                structural_and_identity_orders_differ(&control.store, &channel_prefix),
                "opposed fixture must exercise the ordering bridge",
            );
        }
        let extra_datum =
            Datum::create(&channel, "identity-order-index/mutation-sentinel".to_string(), false);
        let extra_identity = encode_datum(&extra_datum);
        let fixture = Self {
            control,
            identity_order,
            channel,
            channel_prefix,
            extra_datum,
            extra_identity,
        };
        fixture.audit();
        fixture
    }

    pub const fn distinct(&self) -> usize { self.control.distinct() }

    pub const fn multiplicity(&self) -> u64 { self.control.multiplicity() }

    pub const fn shape(&self) -> PrefixShape { self.control.shape() }

    pub fn logical_cardinality(&self) -> u64 { self.control.logical_cardinality() }

    pub fn run_egress(&self, arm: IdentityOrderArm) -> CanonicalEgressOutcome {
        match arm {
            IdentityOrderArm::ArenaSort => self.control.canonical_arena(),
            IdentityOrderArm::MaintainedPathMap => self.identity_pathmap_egress(),
        }
    }

    pub fn run_rebuild_egress(&self, arm: IdentityOrderArm) -> CanonicalEgressOutcome {
        match arm {
            IdentityOrderArm::ArenaSort => self.control.canonical_arena(),
            IdentityOrderArm::MaintainedPathMap => {
                let rebuilt = build_identity_order_index(&self.control.store, &self.channel_prefix)
                    .expect("valid benchmark authority must rebuild");
                encode_identity_order_bag(&self.channel, &rebuilt)
                    .expect("valid benchmark identity index must encode")
            }
        }
    }

    pub fn mutation_fixture(&self, arm: IdentityOrderArm) -> IdentityOrderMutationFixture {
        IdentityOrderMutationFixture {
            store: self.control.store.clone(),
            identity_order: match arm {
                IdentityOrderArm::ArenaSort => None,
                IdentityOrderArm::MaintainedPathMap => Some(self.identity_order.clone()),
            },
            channel: self.channel.clone(),
            extra_datum: self.extra_datum.clone(),
            extra_identity: self.extra_identity.clone(),
            expected_cardinality: self.logical_cardinality(),
        }
    }

    pub fn run_clone_update(&self, arm: IdentityOrderArm) -> u64 {
        let mut store = self.control.store.clone();
        let mut identity_order = match arm {
            IdentityOrderArm::ArenaSort => None,
            IdentityOrderArm::MaintainedPathMap => Some(self.identity_order.clone()),
        };
        store
            .insert(&self.channel, &self.extra_datum)
            .expect("clone-update authority insertion");
        if let Some(index) = identity_order.as_mut() {
            index
                .insert(&self.extra_identity)
                .expect("clone-update identity insertion");
        }
        std::hint::black_box(
            identity_order
                .as_ref()
                .map_or(0, IdentityOrderIndex::cardinality),
        );
        store.cardinality()
    }

    pub fn audit(&self) {
        let expected = self.control.canonical_arena();
        assert_eq!(self.identity_pathmap_egress(), expected);
        assert_eq!(self.run_rebuild_egress(IdentityOrderArm::MaintainedPathMap), expected,);
        for arm in IdentityOrderArm::ALL {
            let mut mutation = self.mutation_fixture(arm);
            assert_eq!(mutation.run_insert_remove_cycle(), self.logical_cardinality());
            assert_eq!(mutation.store.cardinality(), self.logical_cardinality());
        }
    }

    fn identity_pathmap_egress(&self) -> CanonicalEgressOutcome {
        encode_identity_order_bag(&self.channel, &self.identity_order)
            .expect("valid benchmark identity index must encode")
    }
}

pub struct IdentityOrderMutationFixture {
    store: RSpaceDataStore<String, String>,
    identity_order: Option<IdentityOrderIndex>,
    channel: String,
    extra_datum: Datum<String>,
    extra_identity: Vec<u8>,
    expected_cardinality: u64,
}

impl IdentityOrderMutationFixture {
    pub fn run_insert_remove_cycle(&mut self) -> u64 {
        let path = RSpaceDataIndex::datum_path(&self.channel, &self.extra_datum);
        self.store
            .insert(&self.channel, &self.extra_datum)
            .expect("mutation authority insertion");
        if let Some(index) = self.identity_order.as_mut() {
            index
                .insert(&self.extra_identity)
                .expect("mutation identity insertion");
        }
        assert!(self.store.remove_path(&path), "inserted authority path must remove");
        if let Some(index) = self.identity_order.as_mut() {
            index
                .remove(&self.extra_identity)
                .expect("mutation identity removal");
        }
        assert_eq!(self.store.cardinality(), self.expected_cardinality);
        if let Some(index) = &self.identity_order {
            assert_eq!(index.cardinality(), self.expected_cardinality);
        }
        self.expected_cardinality
    }
}

fn build_identity_order_index(
    store: &RSpaceDataStore<String, String>,
    channel_prefix: &[u8],
) -> Result<IdentityOrderIndex, DataIndexError> {
    let mut identity_order = IdentityOrderIndex::new();
    store
        .index
        .as_bag()
        .try_visit_relative_prefix(channel_prefix, |path, multiplicity| {
            let identity = RhoPathV1::relative_datum_identity(path)?;
            if identity_order.bag.contains(identity) {
                return Err(DataIndexError::DuplicateDatumIdentity);
            }
            identity_order.insert_many(identity, multiplicity)?;
            Ok(())
        })?;
    Ok(identity_order)
}

fn structural_and_identity_orders_differ(
    store: &RSpaceDataStore<String, String>,
    channel_prefix: &[u8],
) -> bool {
    let mut structural_order = Vec::new();
    store
        .index
        .as_bag()
        .try_visit_relative_prefix(channel_prefix, |path, _| {
            structural_order.push(RhoPathV1::relative_datum_identity(path)?.to_vec());
            Ok::<(), DataIndexError>(())
        })
        .expect("valid benchmark paths");
    let mut identity_order = structural_order.clone();
    identity_order.sort_unstable();
    structural_order != identity_order
}

fn encode_identity_order_bag(
    channel: &String,
    identity_order: &IdentityOrderIndex,
) -> Result<CanonicalEgressOutcome, DataIndexError> {
    let datum_count = identity_order.bag.cardinality();
    if identity_order.bag.cardinality_is_saturated() {
        return Err(DataIndexError::EncodedCardinalityOverflow(
            identity_order.bag.exact_cardinality(),
        ));
    }
    let capacity = usize::try_from(identity_order.encoded_len)
        .map_err(|_| DataIndexError::EncodedLengthOverflow(identity_order.encoded_len))?;
    let mut encoded_datums = Vec::new();
    encoded_datums
        .try_reserve_exact(capacity)
        .map_err(|_| DataIndexError::AllocationFailure(capacity))?;
    encoded_datums.extend_from_slice(&datum_count.to_le_bytes());
    identity_order
        .bag
        .visit_relative_prefix(&[], |identity, multiplicity| {
            let identity_len = u64::try_from(identity.len())
                .expect("preflighted identity length must fit bincode u64");
            for _ in 0..multiplicity.get() {
                encoded_datums.extend_from_slice(&identity_len.to_le_bytes());
                encoded_datums.extend_from_slice(identity);
            }
        });
    debug_assert_eq!(encoded_datums.len(), capacity);
    Ok(CanonicalEgressOutcome {
        channel_hash: hash(channel),
        encoded_datums,
        datum_count,
    })
}

#[cfg(test)]
mod identity_order_tests {
    use super::*;

    #[test]
    fn empty_identity_index_encodes_the_frozen_empty_datum_vector() {
        let channel = "empty-identity-order".to_string();
        let actual = encode_identity_order_bag(&channel, &IdentityOrderIndex::new()).unwrap();
        assert_eq!(actual.channel_hash, hash(&channel));
        assert_eq!(actual.encoded_datums, encode_datums::<String>(&Vec::new()));
        assert_eq!(actual.datum_count, 0);
    }

    #[test]
    fn opposed_fixture_exercises_different_structural_and_identity_orders() {
        let fixture = IdentityOrderFixture::new(256, 1, PrefixShape::Opposed);
        assert_eq!(
            fixture.run_egress(IdentityOrderArm::ArenaSort),
            fixture.run_egress(IdentityOrderArm::MaintainedPathMap),
        );
    }

    #[test]
    fn wide_identity_order_lifecycle_is_stack_safe_on_a_small_thread() {
        std::thread::Builder::new()
            .name("identity-order-small-stack".to_string())
            .stack_size(128 * 1024)
            .spawn(|| {
                for shape in [PrefixShape::Shared, PrefixShape::Unshared, PrefixShape::Opposed] {
                    let fixture = IdentityOrderFixture::new(4_096, 1, shape);
                    assert_eq!(
                        fixture.run_egress(IdentityOrderArm::ArenaSort),
                        fixture.run_egress(IdentityOrderArm::MaintainedPathMap),
                    );
                    assert_eq!(
                        fixture.run_rebuild_egress(IdentityOrderArm::ArenaSort),
                        fixture.run_rebuild_egress(IdentityOrderArm::MaintainedPathMap),
                    );
                    for arm in IdentityOrderArm::ALL {
                        let mut mutation = fixture.mutation_fixture(arm);
                        assert_eq!(
                            mutation.run_insert_remove_cycle(),
                            fixture.logical_cardinality()
                        );
                        assert_eq!(
                            fixture.run_clone_update(arm),
                            fixture.logical_cardinality() + 1
                        );
                    }
                }
            })
            .expect("spawn identity-order small-stack gate")
            .join()
            .expect("identity-order small-stack gate must not overflow or panic");
    }
}
