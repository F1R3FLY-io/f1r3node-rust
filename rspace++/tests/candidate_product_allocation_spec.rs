use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::BTreeSet;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rspace_plus_plus::rspace::candidate_order::deterministic_candidate_hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, legacy_prefix,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};
use tokio::runtime::{Builder, Runtime};

const CHANNEL: &str = "candidate-product-allocation";
const SMALL_RECORD_COUNT: usize = 1_024;
const LARGE_RECORD_COUNT: usize = 2_048;
const SHORT_VALUE_LENGTH: usize = 64;
const LONG_VALUE_LENGTH: usize = 128;
const WARM_BOUND: Bound = Bound {
    numerator: 5,
    denominator: 4,
};
const LINEAR_BOUND: Bound = Bound {
    numerator: 3,
    denominator: 1,
};

#[derive(Clone, Copy)]
struct Bound {
    numerator: u64,
    denominator: u64,
}

impl Bound {
    fn admits(self, small: u64, large: u64) -> bool {
        u128::from(large) * u128::from(self.denominator) <=
            u128::from(small) * u128::from(self.numerator)
    }

    fn factor(self) -> f64 { self.numerator as f64 / self.denominator as f64 }
}

#[derive(Clone, Copy, Debug, Default)]
struct Allocations {
    calls: u64,
    bytes: u64,
}

thread_local! {
    static TALLY: Cell<Option<Allocations>> = const { Cell::new(None) };
}

struct CountingAllocator;

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        record_allocation(pointer, layout.size());
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        record_allocation(pointer, layout.size());
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, new_size) };
        record_allocation(replacement, new_size);
        replacement
    }
}

fn record_allocation(pointer: *mut u8, bytes: usize) {
    if pointer.is_null() {
        return;
    }
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    let _ = TALLY.try_with(|tally| {
        if let Some(current) = tally.get() {
            tally.set(Some(Allocations {
                calls: current.calls.saturating_add(1),
                bytes: current.bytes.saturating_add(bytes),
            }));
        }
    });
}

fn counted<T>(work: impl FnOnce() -> T) -> (T, Allocations) {
    TALLY.with(|tally| tally.set(Some(Allocations::default())));
    let value = work();
    let allocations = TALLY
        .with(|tally| tally.replace(None))
        .expect("the tally counts while the work runs");
    (value, allocations)
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct Pattern(String);

impl StableHashSerialize for Pattern {}

impl ColdStoreDecode for Pattern {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

#[derive(Clone, Copy)]
enum Configuration {
    PrefixPruned,
    UnprunedCloning,
}

struct ExactMatcher {
    configuration: Configuration,
    calls: Arc<AtomicUsize>,
}

impl Match<Pattern, String, String> for ExactMatcher {
    fn get(&self, pattern: &Pattern, datum: &String) -> Option<String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let equal = match self.configuration {
            Configuration::PrefixPruned => pattern.0 == *datum,
            Configuration::UnprunedCloning => pattern.0 == black_box(datum.clone()),
        };
        equal.then(|| datum.clone())
    }

    fn retained_pattern_path(&self, pattern: &Pattern) -> Option<Vec<u8>> {
        Some(pattern.0.as_bytes().to_vec())
    }

    fn retained_datum_path(&self, datum: &String) -> Option<Vec<u8>> {
        Some(datum.as_bytes().to_vec())
    }

    fn supports_rho_path_candidate_pruning(&self) -> bool {
        match self.configuration {
            Configuration::PrefixPruned => true,
            Configuration::UnprunedCloning => false,
        }
    }
}

type TestSpace = RSpace<String, Pattern, String, String>;

#[derive(Debug)]
struct Measurement {
    records: usize,
    cold: Allocations,
    warm: Allocations,
    warm_matcher_calls: usize,
}

fn stored_values(records: usize, value_length: usize) -> Vec<String> {
    (0..records)
        .map(|index| {
            let prefix = format!("record/{index:08x}/");
            let padding = "v".repeat(value_length - prefix.len());
            prefix + &padding
        })
        .collect()
}

fn last_canonical_value(values: &[String]) -> String {
    values
        .iter()
        .max_by_key(|value| {
            deterministic_candidate_hash(&Datum::create(
                &CHANNEL.to_string(),
                (*value).clone(),
                true,
            ))
        })
        .expect("a nonempty corpus")
        .clone()
}

async fn populated_space(
    configuration: Configuration,
    values: &[String],
) -> (TestSpace, Arc<AtomicUsize>) {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.expect("in-memory stores");
    let calls = Arc::new(AtomicUsize::new(0));
    let matcher = ExactMatcher {
        configuration,
        calls: Arc::clone(&calls),
    };
    let space = RSpace::create(stores, Arc::new(Box::new(matcher))).expect("RSpace");
    for value in values {
        assert!(
            space
                .produce(CHANNEL.to_string(), value.clone(), true)
                .await
                .expect("produce")
                .is_none(),
            "no continuation waits on the channel"
        );
    }
    (space, calls)
}

fn consume_exactly(runtime: &Runtime, space: &TestSpace, target: &str) -> (String, Allocations) {
    drop(runtime.block_on(space.take_event_log()));
    let channels = vec![CHANNEL.to_string()];
    let patterns = vec![Pattern(target.to_string())];
    let continuation = "continuation".to_string();
    let (outcome, allocations) = counted(|| {
        runtime.block_on(space.consume(channels, patterns, continuation, false, BTreeSet::new()))
    });
    assert!(allocations.calls > 0, "the counting allocator counted no allocation of the consume");
    let (_, data) = outcome
        .expect("consume")
        .expect("the resting datum matches the arriving consume");
    assert_eq!(data.len(), 1, "the consume has one bind");
    let matched = data
        .into_iter()
        .next()
        .expect("one matched datum")
        .matched_datum;
    (matched, allocations)
}

fn measure(configuration: Configuration, records: usize, value_length: usize) -> Measurement {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime");
    let values = stored_values(records, value_length);
    let target = last_canonical_value(&values);
    let (space, calls) = runtime.block_on(populated_space(configuration, &values));
    let (cold_match, cold) = consume_exactly(&runtime, &space, &target);
    calls.store(0, Ordering::Relaxed);
    let (warm_match, warm) = consume_exactly(&runtime, &space, &target);
    assert_eq!(cold_match, target, "the cold consume matches the last value in canonical order");
    assert_eq!(warm_match, target, "the warm consume matches the last value in canonical order");
    Measurement {
        records,
        cold,
        warm,
        warm_matcher_calls: calls.load(Ordering::Relaxed),
    }
}

fn growth_violations(
    rule: &str,
    bound: Bound,
    small: Allocations,
    large: Allocations,
) -> Vec<String> {
    [("allocation calls", small.calls, large.calls), ("allocated bytes", small.bytes, large.bytes)]
        .into_iter()
        .filter(|(_, small, large)| !bound.admits(*small, *large))
        .map(|(metric, small, large)| {
            format!(
                "{rule}: the {metric} grow from {small} to {large}, a factor of {:.3}, above {}",
                large as f64 / small as f64,
                bound.factor()
            )
        })
        .collect()
}

#[test]
fn doubling_the_resting_data_keeps_warm_and_cold_consume_allocations_within_bounds() {
    let small = measure(Configuration::PrefixPruned, SMALL_RECORD_COUNT, SHORT_VALUE_LENGTH);
    let large = measure(Configuration::PrefixPruned, LARGE_RECORD_COUNT, SHORT_VALUE_LENGTH);
    println!("{small:?}\n{large:?}");
    for measurement in [&small, &large] {
        assert_eq!(
            measurement.warm_matcher_calls, 1,
            "the prefix-pruned warm consume compares one resting datum: {measurement:?}"
        );
    }
    let mut violations =
        growth_violations("rule 1 (warm consume)", WARM_BOUND, small.warm, large.warm);
    violations.extend(growth_violations(
        "rule 2 (cold consume)",
        LINEAR_BOUND,
        small.cold,
        large.cold,
    ));
    assert!(violations.is_empty(), "{violations:#?}\n{small:?}\n{large:?}");
}

#[test]
fn doubling_the_value_length_keeps_warm_consume_allocations_within_the_linear_bound() {
    let short = measure(Configuration::PrefixPruned, SMALL_RECORD_COUNT, SHORT_VALUE_LENGTH);
    let long = measure(Configuration::PrefixPruned, SMALL_RECORD_COUNT, LONG_VALUE_LENGTH);
    println!("{short:?}\n{long:?}");
    for measurement in [&short, &long] {
        assert_eq!(
            measurement.warm_matcher_calls, 1,
            "the prefix-pruned warm consume compares one resting datum: {measurement:?}"
        );
    }
    let violations = growth_violations("rule 3 (query size)", LINEAR_BOUND, short.warm, long.warm);
    assert!(violations.is_empty(), "{violations:#?}\n{short:?}\n{long:?}");
}

#[test]
fn rule_one_refuses_an_unpruned_scan_whose_matcher_clones_every_resting_datum() {
    let small = measure(Configuration::UnprunedCloning, SMALL_RECORD_COUNT, SHORT_VALUE_LENGTH);
    let large = measure(Configuration::UnprunedCloning, LARGE_RECORD_COUNT, SHORT_VALUE_LENGTH);
    println!("{small:?}\n{large:?}");
    for measurement in [&small, &large] {
        assert_eq!(
            measurement.warm_matcher_calls, measurement.records,
            "without prefix pruning the warm consume compares every resting datum: {measurement:?}"
        );
    }
    let violations = growth_violations("rule 1 (warm consume)", WARM_BOUND, small.warm, large.warm);
    assert_eq!(
        violations.len(),
        2,
        "rule 1 refuses both the allocation calls and the allocated bytes of the cloning scan: \
         {violations:#?}\n{small:?}\n{large:?}"
    );
}
