//! Causal microbenchmark for datum-first retained-automaton × Bag selection.
//!
//! The control models the predecessor's resting-data fallback: visit every
//! retained continuation in canonical order and invoke the authoritative
//! matcher against the fresh datum and then the one resting datum. The
//! treatment first intersects the production retained-transition PathMap with
//! the production Bag, then invokes that same matcher only for selected
//! candidates. Both arms must return the same deliberately last candidate.

use std::sync::Arc;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use rspace_plus_plus::rspace::bag::Bag;
use rspace_plus_plus::rspace::internal::WaitingContinuation;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::retained_set_automaton::RetainedSetAutomaton;
use rspace_plus_plus::rspace::rho_path::RhoPathV1;

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
struct Exact(Vec<u8>);

struct ExactMatcher;

impl Match<Exact, Vec<u8>, ()> for ExactMatcher {
    fn get(&self, pattern: &Exact, datum: &Vec<u8>) -> Option<Vec<u8>> {
        (pattern.0 == *datum).then(|| datum.clone())
    }

    fn retained_pattern_path(&self, pattern: &Exact) -> Option<Vec<u8>> { Some(pattern.0.clone()) }

    fn retained_datum_path(&self, datum: &Vec<u8>) -> Option<Vec<u8>> { Some(datum.clone()) }

    fn supports_rho_path_candidate_pruning(&self) -> bool { true }
}

fn waiting(pattern: Exact) -> WaitingContinuation<Exact, ()> {
    WaitingContinuation {
        patterns: Arc::new(vec![pattern]),
        ..WaitingContinuation::default()
    }
}

struct Fixture {
    candidates: Vec<WaitingContinuation<Exact, ()>>,
    automaton: RetainedSetAutomaton,
    resting_bag: Bag,
    fresh: Vec<u8>,
    resting: Vec<u8>,
}

impl Fixture {
    fn new(cardinality: usize, shared_prefix: bool) -> Self {
        let values = (0..cardinality)
            .map(|index| {
                if shared_prefix {
                    format!("resting/shared-prefix/record-{index:08x}").into_bytes()
                } else {
                    format!("{:02x}/{:08x}", index % 251, index.wrapping_mul(0x9E37_79B1))
                        .into_bytes()
                }
            })
            .collect::<Vec<_>>();
        let expected = cardinality - 1;
        let resting = values[expected].clone();
        let candidates = values
            .into_iter()
            .map(|value| waiting(Exact(value)))
            .collect::<Vec<_>>();
        let automaton = RetainedSetAutomaton::compile(&ExactMatcher, &candidates);
        let mut resting_bag = Bag::new();
        resting_bag
            .insert(&RhoPathV1::relative_datum_key(&resting, b"stable/resting-identity"))
            .expect("one resting occurrence");
        let fixture = Self {
            candidates,
            automaton,
            resting_bag,
            fresh: b"fresh/does-not-match-any-retained-pattern".to_vec(),
            resting,
        };
        assert_eq!(fixture.control(), expected);
        assert_eq!(
            fixture.try_treatment(),
            Some(expected),
            "product preflight at cardinality {cardinality}, shared_prefix={shared_prefix}"
        );
        fixture
    }

    fn control(&self) -> usize {
        self.candidates
            .iter()
            .position(|candidate| {
                let pattern = &candidate.patterns[0];
                ExactMatcher.get(pattern, &self.fresh).is_some() ||
                    ExactMatcher.get(pattern, &self.resting).is_some()
            })
            .expect("the final continuation matches resting data")
    }

    fn treatment(&self) -> usize {
        self.try_treatment()
            .expect("the preflight-proven retained candidate remains present")
    }

    fn try_treatment(&self) -> Option<usize> {
        let selected = self.automaton.matching_candidates_with_resting(
            &self.fresh,
            &[0],
            Some(&self.resting_bag),
        );
        selected
            .iter()
            .zip(&self.candidates)
            .position(|(selected, candidate)| {
                *selected && {
                    let pattern = &candidate.patterns[0];
                    ExactMatcher.get(pattern, &self.fresh).is_some() ||
                        ExactMatcher.get(pattern, &self.resting).is_some()
                }
            })
    }
}

fn bench_retained_bag_product(criterion: &mut Criterion) {
    for shared_prefix in [false, true] {
        let name = if shared_prefix {
            "retained_bag_product_shared_prefix"
        } else {
            "retained_bag_product_diverse_prefix"
        };
        let mut group = criterion.benchmark_group(name);
        for cardinality in [16usize, 256, 4_096] {
            let fixture = Fixture::new(cardinality, shared_prefix);
            group.throughput(Throughput::Elements(1));
            group.bench_function(BenchmarkId::new("control", cardinality), |bencher| {
                bencher.iter(|| black_box(fixture.control()))
            });
            group.bench_function(BenchmarkId::new("pathmap", cardinality), |bencher| {
                bencher.iter(|| black_box(fixture.treatment()))
            });
        }
        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(82)
        .confidence_level(0.99)
        .significance_level(0.01)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(5));
    targets = bench_retained_bag_product
}
criterion_main!(benches);
