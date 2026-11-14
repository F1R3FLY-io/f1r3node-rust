use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use rspace_plus_plus::rspace::candidate_order::deterministic_candidate_hash;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepositoryInstances;
use rspace_plus_plus::rspace::hot_store::{HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, legacy_prefix,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
enum Pattern {
    #[default]
    Wildcard,
    Exact(String),
}

impl rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize for Pattern {}

impl ColdStoreDecode for Pattern {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

#[derive(Clone)]
struct StringMatcher {
    retained_paths: bool,
}

impl Match<Pattern, String, String> for StringMatcher {
    fn get(&self, pattern: &Pattern, datum: &String) -> Option<String> {
        match pattern {
            Pattern::Wildcard => Some(datum.clone()),
            Pattern::Exact(expected) if expected == datum => Some(datum.clone()),
            Pattern::Exact(_) => None,
        }
    }

    fn retained_pattern_path(&self, pattern: &Pattern) -> Option<Vec<u8>> {
        if !self.retained_paths {
            return None;
        }
        match pattern {
            Pattern::Wildcard => None,
            Pattern::Exact(value) => Some(value.as_bytes().to_vec()),
        }
    }

    fn retained_datum_path(&self, datum: &String) -> Option<Vec<u8>> {
        self.retained_paths.then(|| datum.as_bytes().to_vec())
    }

    fn supports_rho_path_candidate_pruning(&self) -> bool { self.retained_paths }
}

#[cfg(feature = "retained-flat-dispatch-experiment")]
struct FlatDispatchMatcher(StringMatcher);

#[cfg(feature = "retained-flat-dispatch-experiment")]
impl Match<Pattern, String, String> for FlatDispatchMatcher {
    fn get(&self, pattern: &Pattern, datum: &String) -> Option<String> {
        self.0.get(pattern, datum)
    }

    fn retained_pattern_path(&self, pattern: &Pattern) -> Option<Vec<u8>> {
        self.0.retained_pattern_path(pattern)
    }

    fn retained_datum_path(&self, datum: &String) -> Option<Vec<u8>> {
        self.0.retained_datum_path(datum)
    }

    fn supports_rho_path_candidate_pruning(&self) -> bool {
        self.0.supports_rho_path_candidate_pruning()
    }

    fn retained_flat_dispatch_experiment(&self) -> bool { true }
}

#[cfg(feature = "retained-flat-dispatch-experiment")]
thread_local! {
    static FLAT_DISPATCH_SPACES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

type BenchSpace = RSpace<String, Pattern, String, String>;

async fn create_space(retained_paths: bool) -> BenchSpace {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager
        .r_space_stores()
        .await
        .expect("in-memory RSpace stores");
    let history = Arc::new(
        HistoryRepositoryInstances::<String, Pattern, String, String>::repository(
            stores.authority(),
        )
        .expect("history repository"),
    );
    let reader = history
        .get_history_reader(&history.root())
        .expect("history reader");
    let state: HotStoreState<String, Pattern, String, String> = HotStoreState::default();
    let hot_store = HotStoreInstances::create_from_hs_and_hr(state, reader.base());
    #[cfg(feature = "retained-flat-dispatch-experiment")]
    if FLAT_DISPATCH_SPACES.get() {
        return RSpace::apply(
            history,
            hot_store,
            Arc::new(Box::new(FlatDispatchMatcher(StringMatcher { retained_paths }))),
        );
    }
    RSpace::apply(history, hot_store, Arc::new(Box::new(StringMatcher { retained_paths })))
}

struct CommFixture {
    runtime: tokio::runtime::Runtime,
    space: BenchSpace,
    channels: Vec<String>,
    patterns: Vec<Pattern>,
}

impl CommFixture {
    fn measure(&self, iterations: u64) -> Duration {
        let mut elapsed = Duration::ZERO;
        for _ in 0..iterations {
            let channels = self.channels.clone();
            let patterns = self.patterns.clone();
            let start = Instant::now();
            let result = self
                .runtime
                .block_on(self.space.consume(
                    channels,
                    patterns,
                    "benchmark-continuation".to_string(),
                    false,
                    BTreeSet::new(),
                ))
                .expect("consume must succeed")
                .expect("persistent data must enable a COMM");
            black_box(&result);
            elapsed += start.elapsed();
            black_box(self.runtime.block_on(self.space.take_event_log()));
        }
        elapsed
    }
}

fn worst_ordered_value(channel: &str, values: &[String]) -> String {
    values
        .iter()
        .max_by_key(|value| {
            deterministic_candidate_hash(&Datum::create(
                &channel.to_string(),
                (*value).clone(),
                true,
            ))
        })
        .expect("benchmark corpus is nonempty")
        .clone()
}

fn distinct_fixture(cardinality: usize, shared_prefix: bool, retained_paths: bool) -> CommFixture {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime");
    let space = runtime.block_on(create_space(retained_paths));
    let channel = "bench-channel".to_string();
    let values = (0..cardinality)
        .map(|index| {
            if shared_prefix {
                format!("shared-prefix/segment/record-{index:08x}")
            } else {
                format!("{:02x}/{:08x}", index % 251, index.wrapping_mul(0x9E37_79B1))
            }
        })
        .collect::<Vec<_>>();
    let target = worst_ordered_value(&channel, &values);
    for value in values {
        assert!(
            runtime
                .block_on(space.produce(channel.clone(), value, true))
                .expect("produce must succeed")
                .is_none()
        );
    }
    drop(runtime.block_on(space.take_event_log()));
    CommFixture {
        runtime,
        space,
        channels: vec![channel],
        patterns: vec![Pattern::Exact(target)],
    }
}

fn duplicate_fixture(cardinality: usize) -> CommFixture {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime");
    let space = runtime.block_on(create_space(true));
    let channel = "duplicate-channel".to_string();
    for _ in 0..cardinality {
        assert!(
            runtime
                .block_on(space.produce(channel.clone(), "same-datum".to_string(), true))
                .expect("produce must succeed")
                .is_none()
        );
    }
    drop(runtime.block_on(space.take_event_log()));
    CommFixture {
        runtime,
        space,
        channels: vec![channel],
        patterns: vec![Pattern::Exact("same-datum".to_string())],
    }
}

fn join_fixture(arity: usize) -> CommFixture {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime");
    let space = runtime.block_on(create_space(true));
    let channels = (0..arity)
        .map(|index| format!("join-channel-{index:04}"))
        .collect::<Vec<_>>();
    for (index, channel) in channels.iter().enumerate() {
        assert!(
            runtime
                .block_on(space.produce(channel.clone(), format!("datum-{index:04}"), true))
                .expect("produce must succeed")
                .is_none()
        );
    }
    drop(runtime.block_on(space.take_event_log()));
    CommFixture {
        runtime,
        space,
        patterns: (0..arity)
            .map(|index| Pattern::Exact(format!("datum-{index:04}")))
            .collect(),
        channels,
    }
}

struct ProduceFixture {
    runtime: tokio::runtime::Runtime,
    space: BenchSpace,
    channel: String,
    datum: String,
}

impl ProduceFixture {
    fn measure(&self, iterations: u64) -> Duration {
        let mut elapsed = Duration::ZERO;
        for _ in 0..iterations {
            let start = Instant::now();
            let result = self
                .runtime
                .block_on(
                    self.space
                        .produce(self.channel.clone(), self.datum.clone(), false),
                )
                .expect("produce must succeed")
                .expect("a persistent receive must enable a COMM");
            black_box(&result);
            elapsed += start.elapsed();
            black_box(self.runtime.block_on(self.space.take_event_log()));
        }
        elapsed
    }
}

fn waiting_receive_fixture(
    cardinality: usize,
    shared_prefix: bool,
    retained_paths: bool,
) -> ProduceFixture {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime");
    let space = runtime.block_on(create_space(retained_paths));
    let channel = "waiting-receive-channel".to_string();
    let channels = vec![channel.clone()];
    let mut receives = (0..cardinality)
        .map(|index| {
            let value = if shared_prefix {
                format!("shared-prefix/segment/receive-{index:08x}")
            } else {
                format!("{:02x}/{:08x}", index % 251, index.wrapping_mul(0x9E37_79B1))
            };
            let patterns = vec![Pattern::Exact(value.clone())];
            let continuation = format!("continuation-{index:08x}");
            let waiting = WaitingContinuation::create(
                &channels,
                &patterns,
                &continuation,
                true,
                BTreeSet::new(),
            );
            (deterministic_candidate_hash(&waiting), index, value, patterns, continuation)
        })
        .collect::<Vec<_>>();
    let datum = receives
        .iter()
        .max_by_key(|(hash, index, _, _, _)| (hash.clone(), *index))
        .expect("benchmark corpus is nonempty")
        .2
        .clone();
    for (_, _, _, patterns, continuation) in receives.drain(..) {
        assert!(
            runtime
                .block_on(space.consume(
                    channels.clone(),
                    patterns,
                    continuation,
                    true,
                    BTreeSet::new(),
                ))
                .expect("consume must succeed")
                .is_none()
        );
    }
    drop(runtime.block_on(space.take_event_log()));
    ProduceFixture {
        runtime,
        space,
        channel,
        datum,
    }
}

#[cfg(feature = "retained-flat-dispatch-experiment")]
fn flat_dispatch_waiting_receive_fixture(
    cardinality: usize,
    shared_prefix: bool,
) -> ProduceFixture {
    FLAT_DISPATCH_SPACES.set(true);
    let fixture = waiting_receive_fixture(cardinality, shared_prefix, true);
    FLAT_DISPATCH_SPACES.set(false);
    fixture
}

#[cfg(feature = "retained-flat-dispatch-experiment")]
fn preflight_flat_dispatch_waiting_receive(cardinality: usize, shared_prefix: bool) {
    let pathmap = waiting_receive_fixture(cardinality, shared_prefix, true);
    let flat_dispatch = flat_dispatch_waiting_receive_fixture(cardinality, shared_prefix);
    let pathmap_result = pathmap
        .runtime
        .block_on(
            pathmap
                .space
                .produce(pathmap.channel.clone(), pathmap.datum.clone(), false),
        )
        .expect("PathMap preflight produce must succeed");
    let flat_dispatch_result = flat_dispatch
        .runtime
        .block_on(flat_dispatch.space.produce(
            flat_dispatch.channel.clone(),
            flat_dispatch.datum.clone(),
            false,
        ))
        .expect("flat-dispatch preflight produce must succeed");
    assert!(pathmap_result.is_some(), "a persistent receive must enable a COMM");
    assert_eq!(
        flat_dispatch_result, pathmap_result,
        "flat-dispatch produce preflight at cardinality {cardinality}, \
         shared_prefix={shared_prefix}"
    );
    assert_eq!(
        flat_dispatch
            .runtime
            .block_on(flat_dispatch.space.take_event_log()),
        pathmap.runtime.block_on(pathmap.space.take_event_log()),
        "flat-dispatch event-log preflight at cardinality {cardinality}, \
         shared_prefix={shared_prefix}"
    );
    assert_eq!(
        flat_dispatch.runtime.block_on(flat_dispatch.space.to_map()),
        pathmap.runtime.block_on(pathmap.space.to_map()),
        "flat-dispatch to_map preflight at cardinality {cardinality}, \
         shared_prefix={shared_prefix}"
    );
}

fn bench_rspace_comm(criterion: &mut Criterion) {
    for shared_prefix in [false, true] {
        let name = if shared_prefix {
            "waiting_receive_shared_prefix"
        } else {
            "waiting_receive_diverse_prefix"
        };
        let mut group = criterion.benchmark_group(name);
        for cardinality in [16usize, 256, 4096] {
            for (label, retained_paths) in [("control", false), ("pathmap", true)] {
                let fixture = waiting_receive_fixture(cardinality, shared_prefix, retained_paths);
                group.throughput(Throughput::Elements(1));
                group.bench_function(BenchmarkId::new(label, cardinality), |bencher| {
                    bencher.iter_custom(|iterations| fixture.measure(iterations));
                });
            }
            #[cfg(feature = "retained-flat-dispatch-experiment")]
            {
                preflight_flat_dispatch_waiting_receive(cardinality, shared_prefix);
                let fixture = flat_dispatch_waiting_receive_fixture(cardinality, shared_prefix);
                group.throughput(Throughput::Elements(1));
                group.bench_function(BenchmarkId::new("flat_dispatch", cardinality), |bencher| {
                    bencher.iter_custom(|iterations| fixture.measure(iterations));
                });
            }
        }
        group.finish();
    }

    for shared_prefix in [false, true] {
        let name = if shared_prefix {
            "distinct_shared_prefix"
        } else {
            "distinct_diverse_prefix"
        };
        let mut group = criterion.benchmark_group(name);
        for cardinality in [1usize, 16, 256, 4096] {
            for (label, retained_paths) in [("control", false), ("pathmap", true)] {
                let fixture = distinct_fixture(cardinality, shared_prefix, retained_paths);
                group.throughput(Throughput::Elements(1));
                group.bench_function(BenchmarkId::new(label, cardinality), |bencher| {
                    bencher.iter_custom(|iterations| fixture.measure(iterations));
                });
            }
        }
        group.finish();
    }

    let mut duplicates = criterion.benchmark_group("duplicate_multiplicity");
    for cardinality in [1usize, 16, 256, 4096] {
        let fixture = duplicate_fixture(cardinality);
        duplicates.throughput(Throughput::Elements(1));
        duplicates.bench_function(BenchmarkId::from_parameter(cardinality), |bencher| {
            bencher.iter_custom(|iterations| fixture.measure(iterations));
        });
    }
    duplicates.finish();

    let mut joins = criterion.benchmark_group("join_arity");
    for arity in [1usize, 2, 4, 8, 16] {
        let fixture = join_fixture(arity);
        joins.throughput(Throughput::Elements(1));
        joins.bench_function(BenchmarkId::from_parameter(arity), |bencher| {
            bencher.iter_custom(|iterations| fixture.measure(iterations));
        });
    }
    joins.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(51)
        .confidence_level(0.99)
        .significance_level(0.01)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(5));
    targets = bench_rspace_comm
}
criterion_main!(benches);
