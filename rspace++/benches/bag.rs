//! Comparative microbenchmarks for the PathMap-backed RSpace Bag.
//!
//! The legacy arm uses the `HashMultiSet<Vec<u8>>` dependency already present
//! in RSpace. Inputs are constructed outside the timed region. Construction,
//! exact lookup, and clone-then-update are intentionally separate treatments:
//! combining them would hide whether prefix compression, hash lookup, or
//! PathMap copy-on-write caused an observed result.

use std::time::Duration;

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};
use multiset::HashMultiSet;
use rspace_plus_plus::rspace::bag::{Bag, BagDelta, Multiplicity};

const CARDINALITIES: [usize; 5] = [1, 16, 256, 4_096, 16_384];

fn shared_prefix_keys(cardinality: usize) -> Vec<Vec<u8>> {
    (0..cardinality)
        .map(|ordinal| {
            let mut path = b"rho-path-v1/shared/channel/record/".to_vec();
            path.extend_from_slice(&(ordinal as u64).to_be_bytes());
            path
        })
        .collect()
}

fn pathmap_bag(keys: &[Vec<u8>]) -> Bag {
    let mut bag = Bag::new();
    for (ordinal, path) in keys.iter().enumerate() {
        bag.insert_many(path, Multiplicity::new((ordinal % 16 + 1) as u64).unwrap())
            .unwrap();
    }
    bag
}

fn legacy_hash_multiset(keys: &[Vec<u8>]) -> HashMultiSet<Vec<u8>> {
    let mut bag = HashMultiSet::new();
    for (ordinal, path) in keys.iter().enumerate() {
        bag.insert_times(path.clone(), ordinal % 16 + 1);
    }
    bag
}

fn bench_construction(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("bag_build_shared_prefix");
    for cardinality in CARDINALITIES {
        let keys = shared_prefix_keys(cardinality);
        group.throughput(Throughput::Elements(cardinality as u64));
        group.bench_with_input(BenchmarkId::new("pathmap", cardinality), &keys, |bencher, keys| {
            bencher.iter(|| black_box(pathmap_bag(black_box(keys))))
        });
        group.bench_with_input(
            BenchmarkId::new("legacy_hash_multiset", cardinality),
            &keys,
            |bencher, keys| bencher.iter(|| black_box(legacy_hash_multiset(black_box(keys)))),
        );
    }
    group.finish();
}

fn bench_exact_lookup(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("bag_exact_lookup_shared_prefix");
    for cardinality in CARDINALITIES {
        let keys = shared_prefix_keys(cardinality);
        let pathmap = pathmap_bag(&keys);
        let legacy = legacy_hash_multiset(&keys);
        group.throughput(Throughput::Elements(cardinality as u64));
        group.bench_with_input(BenchmarkId::new("pathmap", cardinality), &keys, |bencher, keys| {
            bencher.iter(|| {
                let sum = keys
                    .iter()
                    .map(|path| pathmap.count(black_box(path)))
                    .sum::<u64>();
                black_box(sum)
            });
        });
        group.bench_with_input(
            BenchmarkId::new("legacy_hash_multiset", cardinality),
            &keys,
            |bencher, keys| {
                bencher.iter(|| {
                    let sum = keys
                        .iter()
                        .map(|path| legacy.count_of(black_box(path)))
                        .sum::<usize>();
                    black_box(sum)
                });
            },
        );
    }
    group.finish();
}

fn bench_copy_on_write_update(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("bag_clone_then_update_shared_prefix");
    for cardinality in CARDINALITIES {
        let keys = shared_prefix_keys(cardinality);
        let pathmap = pathmap_bag(&keys);
        let legacy = legacy_hash_multiset(&keys);
        let update = b"rho-path-v1/shared/channel/record/new".to_vec();
        group.throughput(Throughput::Elements(1));
        group.bench_function(BenchmarkId::new("pathmap", cardinality), |bencher| {
            bencher.iter(|| {
                let mut fork = pathmap.clone();
                fork.insert(black_box(&update)).unwrap();
                black_box(fork)
            });
        });
        group.bench_function(BenchmarkId::new("legacy_hash_multiset", cardinality), |bencher| {
            bencher.iter(|| {
                let mut fork = legacy.clone();
                fork.insert(black_box(update.clone()));
                black_box(fork)
            });
        });
    }
    group.finish();
}

fn apply_with_redundant_outer_clone(delta: &BagDelta, target: &mut Bag) {
    // Frozen predecessor shape from RSpaceDataStore::apply_delta: the store
    // cloned its Bag, then BagDelta cloned that temporary root again before
    // publishing it. Keep this benchmark-only control so the removed work is
    // measured without reviving it in production.
    let mut updated = target.clone();
    delta.apply_to(&mut updated).unwrap();
    *target = updated;
}

fn bench_atomic_delta_remove(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("bag_atomic_delta_remove_shared_prefix");
    for cardinality in CARDINALITIES {
        let keys = shared_prefix_keys(cardinality);
        let pathmap = pathmap_bag(&keys);
        let removed_path = keys.first().expect("cardinality is positive");
        let mut delta = BagDelta::new();
        delta
            .record_removal(removed_path, Multiplicity::ONE)
            .unwrap();

        group.throughput(Throughput::Elements(1));
        group.bench_function(
            BenchmarkId::new("redundant_two_root_predecessor", cardinality),
            |bencher| {
                bencher.iter_batched(
                    || pathmap.clone(),
                    |mut fork| {
                        apply_with_redundant_outer_clone(black_box(&delta), &mut fork);
                        black_box(fork)
                    },
                    BatchSize::SmallInput,
                )
            },
        );
        group.bench_function(BenchmarkId::new("single_root", cardinality), |bencher| {
            bencher.iter_batched(
                || pathmap.clone(),
                |mut fork| {
                    black_box(&delta).apply_to(&mut fork).unwrap();
                    black_box(fork)
                },
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_logical_multiplicity(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("bag_logical_multiplicity");
    let path = b"rho-path-v1/shared/channel/identical-record";
    for multiplicity in [1u64, 16, 256, 4_096, 1_000_000, u64::MAX] {
        group.throughput(Throughput::Elements(1));
        group.bench_function(BenchmarkId::new("pathmap", multiplicity), |bencher| {
            bencher.iter(|| {
                let mut bag = Bag::new();
                bag.insert_many(
                    black_box(path),
                    Multiplicity::new(black_box(multiplicity)).unwrap(),
                )
                .unwrap();
                black_box(bag)
            });
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(51)
        .confidence_level(0.99)
        .significance_level(0.01)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(5));
    targets =
        bench_construction,
        bench_exact_lookup,
        bench_copy_on_write_update,
        bench_atomic_delta_remove,
        bench_logical_multiplicity
}
criterion_main!(benches);
