//! Causal benchmark for exact-member subtraction in the EPathMap AC matcher.
//!
//! A wildcard remainder forces the production PathMap matcher even though all
//! explicit set members or map pairs are exact. Fixture cloning is excluded
//! from the timed region, so the measurement isolates validation, exact-key
//! classification, native trie subtraction, and PDA finalization.

use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EPathMap, Expr, Par};
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_gint_par, new_wildcard_var};
use rholang::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;

fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn carrier(pathmap: EPathMap, connective_used: bool) -> Par {
    vector_par(Vec::new(), connective_used).with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
    }])
}

struct Fixture {
    target: Par,
    pattern: Par,
}

impl Fixture {
    fn set(cardinality: usize) -> Self {
        let target = EPathMap::from_set_iter(
            (0..cardinality).map(|index| gint(i64::try_from(index).expect("fixture index"))),
            Vec::new(),
            false,
            None,
        );
        let pattern = EPathMap::from_set_iter(
            (0..cardinality).map(|index| gint(i64::try_from(index).expect("fixture index"))),
            Vec::new(),
            true,
            Some(new_wildcard_var()),
        );
        Self {
            target: carrier(target, false),
            pattern: carrier(pattern, true),
        }
    }

    fn map(cardinality: usize) -> Self {
        let entries = || {
            (0..cardinality).map(|index| {
                let key = i64::try_from(index).expect("fixture index");
                (gint(key), gint(key.wrapping_mul(17).wrapping_add(3)))
            })
        };
        let target = EPathMap::new_map(entries(), Vec::new(), false, None);
        let pattern = EPathMap::new_map(entries(), Vec::new(), true, Some(new_wildcard_var()));
        Self {
            target: carrier(target, false),
            pattern: carrier(pattern, true),
        }
    }

    fn run(target: Par, pattern: Par) {
        let mut matcher = SpatialMatcherContext::new();
        let matched = matcher.spatial_match_result(target, pattern).is_some();
        assert!(matched, "an identical exact EPathMap must match");
        assert!(
            matcher.free_map.is_empty(),
            "a wildcard remainder binds nothing"
        );
        black_box(matched);
    }
}

fn bench_exact_epathmap(criterion: &mut Criterion) {
    for (name, make_fixture) in [
        (
            "epathmap_ac_exact_set",
            Fixture::set as fn(usize) -> Fixture,
        ),
        (
            "epathmap_ac_exact_map",
            Fixture::map as fn(usize) -> Fixture,
        ),
    ] {
        let mut group = criterion.benchmark_group(name);
        for cardinality in [16usize, 256, 4_096] {
            let fixture = make_fixture(cardinality);
            group.throughput(Throughput::Elements(cardinality as u64));
            group.bench_function(BenchmarkId::new("production", cardinality), |bencher| {
                bencher.iter_batched(
                    || (fixture.target.clone(), fixture.pattern.clone()),
                    |(target, pattern)| Fixture::run(target, pattern),
                    BatchSize::SmallInput,
                );
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
    targets = bench_exact_epathmap
}
criterion_main!(benches);
