use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use proptest::prelude::*;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::delta_sigma::{comm_bound, compute_bound, DeltaEstimate, ScheduleBound};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

async fn runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (runtime, _, _) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

async fn comms(runtime: &mut RhoRuntimeImpl, term: &str) -> u64 {
    runtime.cost.comm_ledger().reset();
    let result = runtime
        .evaluate(
            term,
            Cost::create(1_000_000, "delta sigma spec".to_string()),
            Default::default(),
            Blake2b512Random::create_from_bytes(&[7]),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{term}: {:?}", result.errors);
    runtime.cost.comm_ledger().take().comms
}

fn bound(term: &str) -> Option<u64> { comm_bound(&Compiler::source_to_adt(term).unwrap()) }

const BOUNDED: &[(&str, u64)] = &[
    ("Nil", 0),
    ("new x in { x!(1) }", 0),
    ("new x in { x!(1) | for (_ <- x) { Nil } }", 1),
    (
        "new x in { x!(1) | for (_ <- x) { Nil } | x!(2) | for (_ <- x) { Nil } }",
        2,
    ),
    (
        "new x, y in { x!(1) | y!(2) | for (_ <- x & _ <- y) { Nil } }",
        1,
    ),
    (
        "new x, r in { x!(5) | for (@v <<- x) { r!(v) } | for (_ <- r) { Nil } }",
        2,
    ),
    (
        "new x, y in { x!(1) | for (_ <- x) { y!(2) | for (_ <- y) { Nil } } }",
        2,
    ),
    (
        "new x in { x!(1) | match 3 { 3 => for (_ <- x) { Nil } _ => Nil } }",
        1,
    ),
    (
        "new x in { x!(1) | if (true) { for (_ <- x) { Nil } } else { Nil } }",
        1,
    ),
    ("new x in { for (_ <- x) { Nil } }", 1),
];

const UNBOUNDED: &[&str] = &[
    "new x in { contract x(@n) = { Nil } | x!(1) }",
    "new x in { for (_ <= x) { Nil } | x!(1) }",
    "new x in { x!!(1) | for (_ <- x) { Nil } }",
    r#"@"public"!(1)"#,
    r#"for (_ <- @"public") { Nil }"#,
    "new out(`rho:io:stdout`) in { out!(1) }",
    "new x in { x!(1) | for (@p <- x) { p } }",
    "new x in { for (r <- x) { r!(1) } | new y in { x!(*y) } }",
    "new s in { s :: () }",
];

#[tokio::test]
async fn the_bound_covers_every_comm_of_the_corpus() {
    let mut runtime = runtime().await;
    for (term, expected) in BOUNDED {
        assert_eq!(bound(term), Some(*expected), "{term}");
        let actual = comms(&mut runtime, term).await;
        assert!(actual <= *expected, "{term}: {actual} > {expected}");
    }
}

#[test]
fn unprovable_terms_have_no_bound_and_delta_falls_back_to_the_limit() {
    for term in UNBOUNDED {
        let par = Compiler::source_to_adt(term).unwrap();
        assert_eq!(comm_bound(&par), None, "{term}");
        assert_eq!(DeltaEstimate::of(&par).delta(500), 500, "{term}");
    }
}

#[test]
fn delta_never_passes_the_phlo_limit() {
    for (term, _) in BOUNDED {
        let par = Compiler::source_to_adt(term).unwrap();
        let estimate = DeltaEstimate::of(&par);
        assert_eq!(estimate.compute, None);
        for limit in [0, 1, 7, 1_000] {
            assert!(estimate.delta(limit) <= limit, "{term}");
        }
        let both = DeltaEstimate {
            comm: estimate.comm,
            compute: Some(3),
        };
        assert_eq!(both.delta(1_000), estimate.comm.unwrap() + 3);
        assert_eq!(both.delta(1), 1);
    }
}

fn process(depth: u32) -> BoxedStrategy<String> {
    let leaf = prop_oneof![
        Just("Nil".to_string()),
        (0..3usize, 0..9i64).prop_map(|(c, v)| format!("x{c}!({v})")),
    ];
    leaf.prop_recursive(depth, 24, 3, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{a} | {b}")),
            (0..3usize, inner.clone()).prop_map(|(c, p)| format!("for (_ <- x{c}) {{ {p} }}")),
            (0..3usize, 1..3usize, inner.clone()).prop_map(|(a, k, p)| {
                let b = (a + k) % 3;
                format!("for (_ <- x{a} & _ <- x{b}) {{ {p} }}")
            }),
            (any::<bool>(), inner.clone(), inner.clone())
                .prop_map(|(c, a, b)| format!("if ({c}) {{ {a} }} else {{ {b} }}")),
            (0..3i64, inner.clone(), inner)
                .prop_map(|(v, a, b)| format!("match {v} {{ 1 => {{ {a} }} _ => {{ {b} }} }}")),
        ]
    })
    .boxed()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_closed_terms_never_exceed_their_bound(body in process(4)) {
        let term = format!("new x0, x1, x2 in {{ {body} }}");
        let expected = bound(&term).expect("a closed linear term has a bound");
        let actual = tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut runtime = runtime().await;
            comms(&mut runtime, &term).await
        });
        prop_assert!(actual <= expected, "{}: {} > {}", term, actual, expected);
    }
}

async fn spent(runtime: &mut RhoRuntimeImpl, term: &str) -> (u64, usize) {
    runtime.cost.comm_ledger().reset();
    runtime.cost.charge_storage_left_in_space();
    let result = runtime
        .evaluate(
            term,
            Cost::create(100_000_000, "delta sigma spec".to_string()),
            Default::default(),
            Blake2b512Random::create_from_bytes(&[7]),
        )
        .await
        .unwrap();
    let comms = runtime.cost.comm_ledger().take().comms;
    (u64::try_from(result.cost.value).unwrap() + comms, result.errors.len())
}

fn estimate(term: &str) -> Option<u64> {
    DeltaEstimate::of_source(&Compiler::source_to_adt(term).unwrap(), term.len() as u64).bound()
}

const DATA_FLOW: &[&str] = &[
    "new x, y in { x!(1) | for (@v <- x) { y!(v) } | for (@w <- y) { Nil } }",
    "new x, y in { x!([1, 2, 3]) | for (@v <- x) { y!([v, \"a\", v0]) } | for (_ <- y) { Nil } }",
    "new x, y in { x!(7) | for (@v <- x) { y!(v + 1) } | for (@w <- y) { if (w == 8) { Nil } else { Nil } } }",
    "new x, y, z in { x!({\"k\": 1}) | y!((2, 3)) | for (@a <- x & @b <- y) { z!([a, b]) } | for (_ <- z) { Nil } }",
    "new x, y in { x!(1) | y!(2) | for (@a <- x & @b <- y) { Nil } }",
    "new x in { x!(5) | for (@v <- x) { match v { 5 => Nil _ => Nil } } }",
];

const NO_COMPUTE_BOUND: &[&str] = &[
    "new x, y in { x!(1) | for (@v <- x) { y!(v) | y!(v) } }",
    "new x in { x!(\"a\".length()) }",
    "new x in { x!(\"a\" ++ \"b\") }",
    "new x in { x!(\"${a}\" %% {\"a\": 1}) }",
    "new x in { x!(5) | for (@v <- x where v > 1) { Nil } }",
];

#[tokio::test]
async fn the_static_estimate_covers_every_run_of_the_corpus() {
    let corpus = BOUNDED.iter().map(|(term, _)| term.to_string()).chain(
        DATA_FLOW
            .iter()
            .map(|term| term.replace("v0", "1")),
    );
    for term in corpus {
        let bound = estimate(&term).unwrap_or_else(|| panic!("{term} has an estimate"));
        let (actual, errors) = spent(&mut runtime().await, &term).await;
        eprintln!("estimate {bound}, spent {actual}: {term}");
        assert_eq!(errors, 0, "{term}");
        assert!(actual <= bound, "{term}: {actual} > {bound}");
    }
}

#[test]
fn unbounded_compute_has_no_estimate_and_delta_is_the_limit() {
    for term in NO_COMPUTE_BOUND {
        let par = Compiler::source_to_adt(term).unwrap_or_else(|e| panic!("{term}: {e:?}"));
        assert_eq!(compute_bound(&par, term.len() as u64, &ScheduleBound::default()), None, "{term}");
        assert_eq!(DeltaEstimate::of_source(&par, term.len() as u64).delta(10_000_000), 10_000_000, "{term}");
    }
    for term in UNBOUNDED {
        assert_eq!(estimate(term), None, "{term}");
    }
}

#[test]
fn the_estimate_is_the_comm_bound_plus_the_compute_bound() {
    let term = "new x in { x!(1) | for (_ <- x) { Nil } }";
    let par = Compiler::source_to_adt(term).unwrap();
    let compute = compute_bound(&par, term.len() as u64, &ScheduleBound::default()).unwrap();
    assert!(compute > term.len() as u64);
    assert_eq!(estimate(term), Some(1 + compute));
    let both = DeltaEstimate::of_source(&par, term.len() as u64);
    assert_eq!(both.delta(u64::MAX), 1 + compute);
    assert_eq!(both.delta(10), 10);
}

fn flowing(depth: u32) -> BoxedStrategy<String> {
    let leaf = prop_oneof![
        Just("Nil".to_string()),
        (0..3usize, 0..9i64).prop_map(|(c, v)| format!("x{c}!({v})")),
        (0..3usize, 0..9i64).prop_map(|(c, v)| format!("x{c}!([{v}, \"s\", ({v}, true)])")),
    ];
    leaf.prop_recursive(depth, 24, 3, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{a} | {b}")),
            (0..3usize, inner.clone()).prop_map(|(c, p)| format!("for (_ <- x{c}) {{ {p} }}")),
            (0..3usize, 0..3usize, inner.clone())
                .prop_map(|(c, d, p)| format!("for (@v <- x{c}) {{ x{d}!([v, 1]) | {p} }}")),
            (0..3usize, 0..3usize, inner.clone())
                .prop_map(|(c, d, p)| format!("for (@v <- x{c}) {{ x{d}!(v == 2) | {p} }}")),
            (0..3usize, 1..3usize, inner.clone()).prop_map(|(a, k, p)| {
                let b = (a + k) % 3;
                format!("for (@u <- x{a} & _ <- x{b}) {{ x{a}!(u) | {p} }}")
            }),
            (any::<bool>(), inner.clone(), inner.clone())
                .prop_map(|(c, a, b)| format!("if ({c}) {{ {a} }} else {{ {b} }}")),
        ]
    })
    .boxed()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_runs_never_spend_more_than_their_estimate(body in flowing(4)) {
        let term = format!("new x0, x1, x2 in {{ {body} }}");
        let bound = estimate(&term).expect("a closed affine term has an estimate");
        let (actual, errors) = tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut runtime = runtime().await;
            spent(&mut runtime, &term).await
        });
        prop_assert_eq!(errors, 0, "{}", term);
        prop_assert!(actual <= bound, "{}: {} > {}", term, actual, bound);
    }
}
