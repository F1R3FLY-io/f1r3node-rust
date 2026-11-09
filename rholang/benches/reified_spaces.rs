//! The operation benchmarks of Reified RSpaces (P10 step S8, design v2 section 2.8).
//!
//! Each group is the counterpart of groups of the legacy `spaces_benchmark.rs` at ff479042:
//!
//! | Group here | Legacy groups |
//! |---|---|
//! | `factory` | none: the cost of one space |
//! | `send` | `collection_types`, `produce_consume_cycle` (`produce_only`) |
//! | `send_receive` | `produce_consume_cycle` (`consume_immediate_match`) |
//! | `receive_from_n` | `collection_retrieval` |
//! | `send_to_n` | none: one send to a collection of n records, the write side of `receive_from_n` |
//! | `matcher` | `matcher_comparison`, with a join of two channels and a send that fires a parked receive |
//! | `vectordb_query` | `vectordb_cosine`, `vectordb_top_k`, `vectordb_batch_cosine` |
//! | `concurrency` | `lock_contention_same_channel`, `lock_contention_different_channels` |
//! | `queue_ordering` | `atomic_ordering` |
//! | `handle_produce` | none: the registry lookups of the handle `produce` path (decision 127) |
//!
//! The legacy groups `phlogiston` and `registry_lookup`/`registry_scale` have no counterpart:
//! one charging path exists (M-3), so each case records its phlo, and the registry is deleted
//! (D12). `checkpoint_operations` is in `reified_persistence.rs`.
// Changed by decision 89 (T-BENCH, ruling L-5): no host-work limit bounds a deploy, so the
// one-deploy series use fixed counts, and a collection that orders records by arrival fills with
// one send in each deploy. The text before:
// ```
// //! A measured deploy is one native play at the v6 limits, without its checkpoint. The host work
// //! of the v6 limits bounds the operations of one deploy, so each one-deploy series measures 1
// //! and 10 operations and the largest count that fits, which a calibration finds. The series
// //! then records the next count, which fails with `HostWorkRejected`, and stops there. The
// //! contract records the counts of each series, so only a capture of a new series calibrates.
// //! The read series fill their collections in setup deploys of [`common::CHUNK`] operations.
// ```
//!
//! A measured deploy is one native play, without its checkpoint. A one-deploy series measures 1,
//! 10 and 100 operations. A read series fills its collection in setup deploys of
//! [`common::CHUNK`] operations, and of one operation for a collection that orders its records
//! by arrival, so the order of those records is the order of the sends in every run.

#[path = "../tests/support/mod.rs"]
mod support;

#[allow(dead_code)]
mod common;

// Changed by decision 89 (T-BENCH, ruling L-5): the calibration is retired, so the bench uses
// neither its functions nor the filter. The text before:
// ```
// use common::{
//     bench_case, create, create_and_use, fill, group, host_fill, largest_fitting, matches, par,
//     recorded_counts, runtime, sizes, with_space, Case, Step,
// };
// ```
use common::{
    bench_case, create, create_and_use, fill, group, host_fill, par, runtime, sizes, with_space,
    Case, Step, PAYLOAD_BASE,
};
use criterion::{criterion_group, criterion_main, Criterion};
use tokio::runtime::Runtime;

/// The record counts of the read series.
const READ_SIZES: [u64; 4] = [10, 100, 1_000, 10_000];
// Retired by decision 89 (T-BENCH, ruling L-5): no series calibrates. The constant was also the
// least capacity of a VectorDB space in these benches, which MIN_CAPACITY now names. The text
// before:
// ```
// /// The first bound of a calibration. No space family fits 200 operations in one deploy, and the
// /// calibration doubles the bound for a series that does.
// const CALIBRATION_BOUND: u64 = 200;
// ```
/// The least capacity of a VectorDB space in these benches. The one-deploy series store at most
/// the largest count of [`ONE_DEPLOY_COUNTS`] records in one deploy.
const MIN_CAPACITY: u64 = 200;
/// The parallel operations of the concurrency group.
const CONCURRENT: u64 = 8;
/// The dimensions of the VectorDB records.
const DIMENSIONS: u64 = 4;
/// The operations of one deploy in the one-deploy series and in the `handle_produce` series
/// (decision 89, ruling L-5).
const ONE_DEPLOY_COUNTS: [u64; 3] = [1, 10, 100];
/// The sends of one deploy in the `queue_ordering` series, the counts that P10 step S8 timed. A
/// larger count can reach the fire round cap of the persistent receive (decision 89, ruling L-5).
const QUEUE_ORDERING_COUNTS: [u64; 3] = [1, 10, 31];

/// The hashmap families of the disciplines, with their constructor arguments for `n` records.
fn disciplines(n: u64) -> Vec<(&'static str, String)> {
    vec![
        ("hashmap:bag", "\"default\"".to_string()),
        ("hashmap:set", "\"default\"".to_string()),
        ("hashmap:queue", "\"default\"".to_string()),
        ("hashmap:stack", "\"default\"".to_string()),
        ("hashmap:priorityqueue", "\"default\"".to_string()),
        (
            "hashmap:vectordb",
            format!(
                "\"default\"; capacity={}, dimensions={DIMENSIONS}",
                // Changed by decision 89 (T-BENCH, ruling L-5): the constant is renamed. The text
                // before:
                // n.max(CALIBRATION_BOUND)
                n.max(MIN_CAPACITY)
            ),
        ),
    ]
}

/// The payload of record `i` on a collection of `family`.
fn payload(family: &str, i: u64) -> String {
    if family.ends_with("vectordb") {
        format!("[{}, {}, {}, 1]", i % 7, i % 5, i % 3)
    } else {
        // Changed by decision 102 (T-BENCH): every integer payload has one encoded size, so a
        // random pick among records does not change the phlo. The text before:
        // i.to_string()
        (PAYLOAD_BASE + i).to_string()
    }
}

/// The send of record `i` on `@"c"`; a priority queue spreads its records over three levels.
fn send(family: &str, i: u64) -> String {
    if family.ends_with("priorityqueue") {
        format!("@\"c\"!({}; priority={})", payload(family, i), i % 3)
    } else {
        format!("@\"c\"!({})", payload(family, i))
    }
}

/// A receive on `@"c"` of `family` with the body `body`, which can use the bound value `x`.
fn receive(family: &str, body: &str) -> String {
    if family.ends_with("vectordb") {
        format!("for (@x <- @\"c\" ~ ([1, 1, 1, 1])) {{ {body} }}")
    } else {
        format!("for (@x <- @\"c\") {{ {body} }}")
    }
}

fn short(family: &str) -> &str {
    family
        .split(':')
        .nth(1)
        .expect("a family has an inner collection")
}

/// The setup steps that store `n` records of `family`, with the send `f(i)` for each record `i`.
/// A family that orders its records by arrival gets one send in each deploy, so the order of its
/// records is the order of `i` in every run. Every other family gets one fill step.
fn fill_steps(family: &str, n: u64, f: impl Fn(u64) -> String) -> Vec<Step> {
    match short(family) {
        "queue" | "stack" | "priorityqueue" => {
            (0..n).map(|i| Step::Deploy(with_space(&f(i)))).collect()
        }
        _ => vec![fill(n, f)],
    }
}

// Changed by decision 89 (T-BENCH, ruling L-5): the series measures fixed counts. The text
// before:
// ```
// /// Measures the one-deploy series of `term(n)` on the pre-state of `setup`: 1 and 10
// /// operations, the largest count that fits, and the next count, which records the stop. The
// /// counts come from the contract, and a capture of a series that it does not record yet
// /// calibrates them.
// ```
/// Measures the one-deploy series of `term(n)` on the pre-state of `setup` for each count of
/// [`ONE_DEPLOY_COUNTS`]. `bench_case` applies the filter of the run to each case.
fn one_deploy_series(
    g: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    rt: &Runtime,
    name: &str,
    id: &str,
    setup: Vec<Step>,
    term: impl Fn(u64) -> String,
) {
    // Changed by decision 89 (T-BENCH, ruling L-5): the counts are fixed. The text before:
    // ```
    // let prefix = format!("{name}/{id}/");
    // let mut counts = recorded_counts(&prefix);
    // if counts.is_empty() {
    //     let largest = largest_fitting(rt, name, id, &setup, &term, CALIBRATION_BOUND);
    //     counts = vec![1, 10, largest, largest + 1];
    //     counts.retain(|n| *n >= 1);
    //     counts.sort_unstable();
    //     counts.dedup();
    // }
    // if !counts.iter().any(|n| matches(&format!("{prefix}{n}"))) {
    //     return;
    // }
    // for n in counts {
    // ```
    for n in sizes(&ONE_DEPLOY_COUNTS) {
        let case = Case {
            id: format!("{id}/{n}"),
            setup: setup.clone(),
            term: term(n),
            with_checkpoint: false,
        };
        bench_case(g, rt, name, &case);
    }
}

fn bench_factory(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "factory");
    let cases = [
        ("hashmap:bag", "\"default\""),
        (
            "hashmap:vectordb",
            "\"default\"; capacity=1000, dimensions=4",
        ),
    ];
    for (family, ctor) in cases {
        let case = Case {
            id: family.replace(':', "_"),
            setup: vec![],
            term: create(family, ctor),
            with_checkpoint: false,
        };
        bench_case(&mut g, &rt, "factory", &case);
    }
    g.finish();
}

fn bench_send(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "send");
    // Changed by decision 89 (T-BENCH, ruling L-5): the constant is renamed. The text before:
    // for (family, ctor) in disciplines(CALIBRATION_BOUND) {
    for (family, ctor) in disciplines(MIN_CAPACITY) {
        one_deploy_series(
            &mut g,
            &rt,
            "send",
            short(family),
            vec![Step::Deploy(create(family, &ctor))],
            |n| with_space(&par(n, |i| send(family, i))),
        );
    }
    // The cell holds one record on each channel, so its records go to n channels.
    one_deploy_series(
        &mut g,
        &rt,
        "send",
        "cell",
        vec![Step::Deploy(create("hashmap:cell", "\"default\""))],
        |n| with_space(&par(n, |i| format!("@[\"c\", {i}]!({i})"))),
    );
    // The other outer stores, each with a bag.
    one_deploy_series(
        &mut g,
        &rt,
        "send",
        "pathmap_bag",
        vec![Step::Deploy(create("pathmap:bag", "\"default\""))],
        |n| with_space(&par(n, |i| format!("@[0, 1]!({i})"))),
    );
    for (id, family, ctor) in [
        ("array_bag", "array:bag", "\"default\"; size=4"),
        ("vector_bag", "vector:bag", "\"default\""),
    ] {
        one_deploy_series(
            &mut g,
            &rt,
            "send",
            id,
            vec![Step::Deploy(create(family, ctor))],
            |n| {
                with_space(&format!(
                    "new x in {{\n{}\n}}",
                    par(n, |i| format!("x!({i})"))
                ))
            },
        );
    }
    // A HashSet space has the qualifier seq, which lives for one deploy, so the measured deploy
    // creates it. A seq channel takes no concurrent uses, so each unit has its own channel.
    one_deploy_series(&mut g, &rt, "send", "hashset_bag", vec![], |n| {
        create_and_use(
            "hashset:bag",
            "\"seq\"",
            &par(n, |i| format!("@[\"c\", {i}]!()")),
        )
    });
    // The equivalent host program: sends on an ordinary channel, with no space.
    one_deploy_series(&mut g, &rt, "send", "host", vec![], |n| {
        par(n, |i| format!("@\"c\"!({i})"))
    });
    g.finish();
}

fn bench_send_receive(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "send_receive");
    // Changed by decision 89 (T-BENCH, ruling L-5): the constant is renamed. The text before:
    // for (family, ctor) in disciplines(CALIBRATION_BOUND) {
    for (family, ctor) in disciplines(MIN_CAPACITY) {
        one_deploy_series(
            &mut g,
            &rt,
            "send_receive",
            short(family),
            vec![Step::Deploy(create(family, &ctor))],
            |n| {
                with_space(&par(n, |i| {
                    format!("{} | {}", send(family, i), receive(family, "Nil"))
                }))
            },
        );
    }
    one_deploy_series(&mut g, &rt, "send_receive", "host", vec![], |n| {
        par(n, |i| {
            format!("@\"c\"!({i}) | for (@x <- @\"c\") {{ Nil }}")
        })
    });
    g.finish();
}

fn bench_receive_from_n(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "receive_from_n");
    for n in sizes(&READ_SIZES) {
        for (family, ctor) in disciplines(n) {
            let mut setup = vec![Step::Deploy(create(family, &ctor))];
            // Changed by decision 111 (T-BENCH): the fill of a family that orders its records by
            // arrival plays one send in each deploy. The text before:
            // setup.push(fill(n, |i| send(family, i)));
            setup.extend(fill_steps(family, n, |i| send(family, i)));
            let case = Case {
                id: format!("{}/{n}", short(family)),
                setup,
                term: with_space(&receive(family, "out!(x)")),
                with_checkpoint: false,
            };
            bench_case(&mut g, &rt, "receive_from_n", &case);
        }
        // The equivalent host program: a receive from an ordinary channel with n data.
        let host = Case {
            id: format!("host/{n}"),
            // Changed by decision 102 (T-BENCH): every record has one encoded size, so a random
            // pick does not change the phlo. The text before:
            // setup: vec![host_fill(n, |i| format!("@\"c\"!({i})"))],
            setup: vec![host_fill(n, |i| format!("@\"c\"!({})", PAYLOAD_BASE + i))],
            term: format!("for (@x <- @\"c\") {{ @\"{}\"!(x) }}", common::OUT),
            with_checkpoint: false,
        };
        bench_case(&mut g, &rt, "receive_from_n", &host);
    }
    g.finish();
}

fn bench_send_to_n(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "send_to_n");
    for n in sizes(&READ_SIZES) {
        for (family, ctor) in disciplines(n + 1) {
            let mut setup = vec![Step::Deploy(create(family, &ctor))];
            // Changed by decision 111 (T-BENCH): the fill of a family that orders its records by
            // arrival plays one send in each deploy. The text before:
            // setup.push(fill(n, |i| send(family, i)));
            setup.extend(fill_steps(family, n, |i| send(family, i)));
            let case = Case {
                id: format!("{}/{n}", short(family)),
                setup,
                term: with_space(&send(family, n)),
                with_checkpoint: false,
            };
            bench_case(&mut g, &rt, "send_to_n", &case);
        }
        let host = Case {
            id: format!("host/{n}"),
            setup: vec![host_fill(n, |i| format!("@\"c\"!({i})"))],
            term: format!("@\"c\"!({n})"),
            with_checkpoint: false,
        };
        bench_case(&mut g, &rt, "send_to_n", &host);
    }
    g.finish();
}

fn bench_matcher(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "matcher");
    for n in sizes(&READ_SIZES) {
        let cases = [
            // A pattern that no record matches: the bag scans every record, then the receive waits.
            (
                "bag_scan_miss",
                "hashmap:bag",
                "for (@-1 <- @\"c\") { out!(\"found\") }",
            ),
            // A variable pattern: the first record in the canonical order matches.
            ("bag_first", "hashmap:bag", "for (@x <- @\"c\") { out!(x) }"),
            // A head-only collection: only the head is a candidate.
            (
                "queue_head",
                "hashmap:queue",
                "for (@x <- @\"c\") { out!(x) }",
            ),
        ];
        for (id, family, body) in cases {
            let mut setup = vec![Step::Deploy(create(family, "\"default\""))];
            // Changed by decisions 102 and 111 (T-BENCH): every record has one encoded size, and
            // the fill of a queue plays one send in each deploy. The text before:
            // setup.push(fill(n, |i| format!("@\"c\"!({i})")));
            setup.extend(fill_steps(family, n, |i| {
                format!("@\"c\"!({})", PAYLOAD_BASE + i)
            }));
            let case = Case {
                id: format!("{id}/{n}"),
                setup,
                term: with_space(body),
                with_checkpoint: false,
            };
            bench_case(&mut g, &rt, "matcher", &case);
        }
        // A join of two channels of a bag: the receive reserves one record on each channel (a
        // multi-channel reservation). The fill alternates the channels, so each holds n records.
        let join = Case {
            id: format!("join_two/{n}"),
            setup: vec![
                Step::Deploy(create("hashmap:bag", "\"default\"")),
                fill(2 * n, |i| {
                    // Changed by decision 102 (T-BENCH): every record has one encoded size, so a
                    // random pick does not change the phlo. The text before:
                    // format!("@\"{}\"!({})", if i % 2 == 0 { "a" } else { "b" }, i / 2)
                    format!(
                        "@\"{}\"!({})",
                        if i % 2 == 0 { "a" } else { "b" },
                        PAYLOAD_BASE + i / 2
                    )
                }),
            ],
            term: with_space("for (@x <- @\"a\" & @y <- @\"b\") { out!([x, y]) }"),
            with_checkpoint: false,
        };
        bench_case(&mut g, &rt, "matcher", &join);
        // The produce side of a COMM: n receives wait on the channel of a bag, and the measured
        // send fires one of them. The fill of a waiter series counts its waiters as its records.
        let waiters = Case {
            id: format!("send_fires_waiter/{n}"),
            setup: vec![
                Step::Deploy(create("hashmap:bag", "\"default\"")),
                fill(n, |_| "for (@x <- @\"c\") { Nil }".to_string()),
            ],
            term: with_space("@\"c\"!(0)"),
            with_checkpoint: false,
        };
        bench_case(&mut g, &rt, "matcher", &waiters);
    }
    g.finish();
}

fn bench_vectordb_query(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "vectordb_query");
    let family = "hashmap:vectordb";
    for n in sizes(&READ_SIZES) {
        let ctor = format!("\"default\"; capacity={n}, dimensions={DIMENSIONS}");
        let queries = [
            (
                "threshold",
                "for (@v <- @\"c\" ~ ([1, 1, 1, 1]; threshold=0.5)) { out!(v) }",
            ),
            ("best", "for (@v <- @\"c\" ~ ([1, 1, 1, 1])) { out!(v) }"),
            (
                "k8",
                "for (@vs <- @\"c\" ~ ([1, 1, 1, 1]; k=8)) { out!(vs) }",
            ),
            (
                "list_all",
                "for (@vs <- @\"c\" ~ (; results=\"all\")) { out!(vs) }",
            ),
        ];
        for (id, body) in queries {
            let mut setup = vec![Step::Deploy(create(family, &ctor))];
            setup.push(fill(n, |i| send(family, i)));
            let case = Case {
                id: format!("{id}/{n}"),
                setup,
                term: with_space(body),
                with_checkpoint: false,
            };
            bench_case(&mut g, &rt, "vectordb_query", &case);
        }
    }
    g.finish();
}

fn bench_concurrency(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "concurrency");
    let m = CONCURRENT;
    let one_channel = Case {
        id: format!("one_channel/{m}"),
        setup: vec![Step::Deploy(create("hashmap:bag", "\"default\""))],
        term: with_space(&par(m, |i| format!("@\"c\"!({i})"))),
        with_checkpoint: false,
    };
    let many_channels = Case {
        id: format!("many_channels/{m}"),
        setup: vec![Step::Deploy(create("hashmap:bag", "\"default\""))],
        term: with_space(&par(m, |i| format!("@[\"c\", {i}]!({i})"))),
        with_checkpoint: false,
    };
    let mixed = Case {
        id: format!("mixed_one_channel/{m}"),
        setup: vec![Step::Deploy(create("hashmap:bag", "\"default\""))],
        term: with_space(&par(m / 2, |i| {
            format!("@\"c\"!({i}) | for (@x <- @\"c\") {{ Nil }}")
        })),
        with_checkpoint: false,
    };
    let spaces_setup = par(m, |k| {
        format!(
            "new factory(`rho:space:hashmap:bag`) in {{ for (space <- factory!?(\"default\")) {{ @[\"{}\", {k}]!(*space) }} }}",
            common::HANDLE
        )
    });
    let many_spaces = Case {
        id: format!("many_spaces/{m}"),
        setup: vec![Step::Deploy(spaces_setup)],
        term: par(m, |k| {
            format!(
                "for (space <- @[\"{h}\", {k}]) {{ @[\"{h}\", {k}]!(*space) | use space {{ @\"c\"!({k}) }} }}",
                h = common::HANDLE
            )
        }),
        with_checkpoint: false,
    };
    for case in [one_channel, many_channels, mixed, many_spaces] {
        bench_case(&mut g, &rt, "concurrency", &case);
    }
    g.finish();
}

fn bench_queue_ordering(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "queue_ordering");
    // n sends take their orders from the counter of the collection, and one persistent receive
    // then takes the records in FIFO order, one record in each round.
    // Changed by decision 89 (T-BENCH, ruling L-5): the counts are fixed, and the series keeps
    // its program. The receive arrives after any number of the sends, and a persistent receive
    // that meets the fire round cap of stored records re-issues and bumps the counter once more,
    // so a count that can reach the cap has no stable phlo. The text before:
    // ```
    // one_deploy_series(
    //     &mut g,
    //     &rt,
    //     "queue_ordering",
    //     "send_and_drain",
    //     vec![Step::Deploy(create("hashmap:queue", "\"default\""))],
    //     |n| {
    //         with_space(&format!(
    //             "{} |\nfor (@x <= @\"c\") {{ Nil }}",
    //             par(n, |i| format!("@\"c\"!({i})"))
    //         ))
    //     },
    // );
    // ```
    for n in sizes(&QUEUE_ORDERING_COUNTS) {
        let case = Case {
            id: format!("send_and_drain/{n}"),
            setup: vec![Step::Deploy(create("hashmap:queue", "\"default\""))],
            term: with_space(&format!(
                "{} |\nfor (@x <= @\"c\") {{ Nil }}",
                par(n, |i| format!("@\"c\"!({i})"))
            )),
            with_checkpoint: false,
        };
        bench_case(&mut g, &rt, "queue_ordering", &case);
    }
    g.finish();
}

/// Added by decision 127 (ruling RK2 2.1): one deploy of `n` handle `produce` calls on a hashmap
/// queue space. Each call resolves the family of the space from its config and reads the send
/// schema of the family (`param_schema`), so the series measures the two registry lookups of
/// that path. The counts are those of the one-deploy series.
fn bench_handle_produce(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "handle_produce");
    for n in sizes(&ONE_DEPLOY_COUNTS) {
        let case = Case {
            id: format!("queue/{n}"),
            setup: vec![Step::Deploy(create("hashmap:queue", "\"default\""))],
            term: format!(
                "for (space <- @\"{handle}\") {{\n  @\"{handle}\"!(*space) |\n{calls}\n}}",
                handle = common::HANDLE,
                calls = par(n, |i| {
                    format!("for (_ <- space!?(\"produce\", \"c\", [{i}])) {{ Nil }}")
                }),
            ),
            with_checkpoint: false,
        };
        bench_case(&mut g, &rt, "handle_produce", &case);
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_factory,
    bench_send,
    bench_send_receive,
    bench_receive_from_n,
    bench_send_to_n,
    bench_matcher,
    bench_vectordb_query,
    bench_concurrency,
    bench_handle_produce,
    bench_queue_ordering
);
criterion_main!(benches);
