//! The P5 tests of the reducer hooks (P5 design v3 A.8 and A.10, batch HOOKS).
//!
//! * A receive with a query routes the inner channel of the query. The agent of the space
//!   checks the query, the default space fails with `QuerySpecUnsupported`, and the inner channel
//!   takes the read rule of HEAD.
//! * The named params of a send to a collection reach the agent of the space, and positional
//!   params still abort at the hook.
//! * A `new` in a `use` block, a `new` with space annotations, and the handle method `gensym` run
//!   one allocation job for each space of their names (F11). The job reads the config of the
//!   space, so a missing space fails at `new`, and native execution fails at `new` (decision 46).
//! * Replay reproduces play for all of these deploys.
//!
//! The deploys run on the family `hashmap:queue`, the only family that the family agent wires
//! when this suite is written. Its store has no allocator, so each allocation job answers
//! `Baseline`: the job reads the config and commits nothing, and the names keep the random
//! draws of HEAD. The unit tests of `reified/reducer_hooks.rs` and `reified/handle.rs` drive the
//! index names of a store with an allocator.

#[path = "support/mod.rs"]
mod support;

use std::collections::HashMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::utils::{new_gint_par, new_gstring_par};
use proptest::prelude::{prop_assert, prop_assert_eq, prop_oneof, Just, Strategy};
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativeBudgetTraceLimits, NativePhloDimension,
    NativePhloExecutionContract, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use rholang::rust::interpreter::accounting::{
    funding_sig_compound, NativeOperationJournalLimits, NativeOperationTraceLimits,
    NativeRuntimeConfig, RuntimeBudget,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use rholang::rust::interpreter::reified::route::{classify_channel, ChannelClass};
use rholang::rust::interpreter::rho_runtime::{
    create_native_replay_env, create_rho_runtime, RhoRuntime, RhoRuntimeImpl,
};
use rholang::rust::interpreter::spaces::namespace::{
    self, Durability, NameTail, NativeSpace, ReifiedId, SID_LEN,
};
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::Event;
use support::props::{check, runtime_config, tokio_runtime};
use support::session::{
    config_channel, counter_channel, error_codes, events_touching, injected, native, with_space,
    Injections, Session, Space,
};
use tokio::sync::{Mutex, RwLock};

/// The factory of every space of this suite.
const QUEUE: &str = "rho:space:hashmap:queue";

/// The constructor of every space of this suite, as Rholang source.
const DEFAULT: &str = r#""default""#;

/// The text of HEAD for a receive on a bundle without the read flag (`unbundle_receive`).
const NOT_READABLE: &str = "Trying to read from non-readable channel.";

fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn gstring(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

/// A space that no factory created.
fn missing_space() -> NativeSpace {
    NativeSpace {
        sid: [0x5A; SID_LEN],
        durability: Durability::Durable,
    }
}

/// The bare handle of `space`.
fn handle_of(space: NativeSpace) -> Par { namespace::gprivate(namespace::handle_channel(space)) }

/// The allocator channel `A(sid)` of `space`.
fn allocator_of(space: NativeSpace) -> Par {
    namespace::gprivate(namespace::allocator_channel(space))
}

/// The injected names of the suite: the handle and a name of a space that no factory created.
fn missing_injections() -> Injections {
    let space = missing_space();
    Injections::from([
        (injected("missing-handle"), handle_of(space)),
        (
            injected("missing-name"),
            namespace::gprivate(namespace::name_id(space, NameTail::Random([0x11; 32]))),
        ),
    ])
}

/// A session with one queue space and its two names `a` and `b`.
async fn queue_session() -> (Session, Space) {
    let mut session = Session::new().await;
    let space = session.setup(QUEUE, DEFAULT).await;
    (session, space)
}

/// The 32-byte tail of a name of `space` with a random tail.
fn random_tail(name: &Par, space: NativeSpace) -> [u8; 32] {
    match namespace::reified_id_of(name) {
        Some(ReifiedId::Name(found, NameTail::Random(tail))) if found == space => tail,
        other => panic!("the name is not a random-tail name of the space: {other:?}"),
    }
}

/// The elements of a `Par` that holds exactly one list.
fn list_elements(par: &Par) -> Vec<Par> {
    match par.exprs.as_slice() {
        [models::rhoapi::Expr {
            expr_instance: Some(models::rhoapi::expr::ExprInstance::EListBody(list)),
        }] => list.ps.clone(),
        other => panic!("the value is not one list: {other:?}"),
    }
}

// ── QUERY ────────────────────────────────────────────────────────────────────────────────────

/// A query on a collection of a queue space reaches the agent of the space, which rejects it
/// with `QuerySpecUnsupported` before the bump, so the record stays and the counter does not
/// move. A forged channel in a `use` block and a readable bundle around a space name route to
/// the space too. A query on a name of a missing space fails in the agent with `DanglingName`,
/// which shows that the hook routes the inner channel instead of rejecting the tuple.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hooks_query_routes_the_inner_channel() {
    let (mut session, space) = queue_session().await;
    session
        .run(&with_space("a!(1)"), "rrs-p5-hooks-query-seed")
        .await;
    let counter = session.counter(space.space, &space.a).await;
    let injections = missing_injections();
    for (label, body, code) in [
        (
            "value",
            r#"for (@x <- a ~ 1) { @"rrs-query"!(x) }"#,
            "QuerySpecUnsupported",
        ),
        (
            "named",
            r#"for (@x <- a ~ (; k=1)) { @"rrs-query"!(x) }"#,
            "QuerySpecUnsupported",
        ),
        (
            "read-only-bundle",
            r#"for (@x <- @{bundle-{*a}} ~ 1) { @"rrs-query"!(x) }"#,
            "QuerySpecUnsupported",
        ),
        (
            "forged-in-use",
            r#"use space { for (@x <- @"docs" ~ (1; k=2)) { Nil } }"#,
            "QuerySpecUnsupported",
        ),
        (
            "missing-space",
            r#"new n(`rho:rrs:test:missing-name`) in { for (@x <- n ~ 1) { Nil } }"#,
            "DanglingName",
        ),
    ] {
        session
            .fail_with(
                &with_space(body),
                &format!("rrs-p5-hooks-query-{label}"),
                &injections,
                &[code],
            )
            .await;
    }
    assert!(session.values("rrs-query").await.is_empty());
    assert_eq!(
        session.records(space.space, &space.a).await,
        vec![(0, vec![gint(1)], false)],
        "a rejected query takes no record"
    );
    assert_eq!(
        session.counter(space.space, &space.a).await,
        counter,
        "and bumps no counter"
    );
    assert!(session
        .data(&counter_channel(space.space, &gstring("docs")))
        .await
        .is_empty());
}

/// A query on the default space fails with `QuerySpecUnsupported`, also in a join, and the
/// QUERY tuple never becomes a channel of the host: the receive does not park.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hooks_query_on_the_default_space_fails_closed() {
    let mut session = Session::new().await;
    for (label, body) in [
        ("host", r#"for (@x <- @"rrs-p5-host" ~ 1) { Nil }"#),
        (
            "unforgeable",
            r#"new p in { for (@x <- p ~ (1; k=2)) { Nil } }"#,
        ),
        (
            "join",
            r#"for (@x <- @"rrs-p5-left" ~ 1 & @y <- @"rrs-p5-right") { Nil }"#,
        ),
    ] {
        session
            .fail_with(
                body,
                &format!("rrs-p5-hooks-default-{label}"),
                &Injections::new(),
                &["QuerySpecUnsupported"],
            )
            .await;
    }
    // The same receive without a query parks on the host, as in HEAD.
    session
        .run(
            r#"for (@x <- @"rrs-p5-host") { @"rrs-p5-got"!(x) } | @"rrs-p5-host"!(5)"#,
            "rrs-p5-hooks-default-control",
        )
        .await;
    assert_eq!(session.values("rrs-p5-got").await, vec![vec![gint(5)]]);
}

/// The inner channel of a query takes the read rule of `unbundle_receive`, with the text of
/// HEAD: a bundle without the read flag fails, and a bundle with it gives its body.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hooks_query_inner_channel_takes_the_read_rule() {
    let mut session = Session::new().await;
    for (label, body) in [
        (
            "write-only",
            r#"new w in { for (@x <- @{bundle+{*w}} ~ 1) { Nil } }"#,
        ),
        (
            "no-flags",
            r#"new w in { for (@x <- @{bundle0{*w}} ~ 1) { Nil } }"#,
        ),
        ("head", r#"new w in { for (@x <- @{bundle+{*w}}) { Nil } }"#),
    ] {
        session
            .fail_with(
                body,
                &format!("rrs-p5-hooks-read-{label}"),
                &Injections::new(),
                &[NOT_READABLE],
            )
            .await;
    }
    session
        .fail_with(
            r#"new w in { for (@x <- @{bundle-{*w}} ~ 1) { Nil } }"#,
            "rrs-p5-hooks-read-readable",
            &Injections::new(),
            &["QuerySpecUnsupported"],
        )
        .await;
}

// ── Params ───────────────────────────────────────────────────────────────────────────────────

/// The named params of a send to a collection reach the agent (P5b). The queue takes no send
/// param, so its agent fails with `UnknownParam` before the bump. A name of a missing space fails
/// in the agent with `DanglingName`, which the hook of P4 reported as `UnknownParam`. Positional
/// params still abort at the hook, before the agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hooks_named_params_reach_the_agent() {
    let (mut session, space) = queue_session().await;
    let injections = missing_injections();
    for (label, body, code) in [
        ("named", "a!(1; k=1)", "UnknownParam"),
        ("persistent", "a!!(1; priority=0)", "UnknownParam"),
        ("forged", r#"use space { @"x"!(1; k=1) }"#, "UnknownParam"),
        ("positional", "a!(1; 2)", "PositionalParamsUnsupported"),
        ("missing-named", "n!(1; k=1)", "DanglingName"),
        (
            "missing-positional",
            "n!(1; 2)",
            "PositionalParamsUnsupported",
        ),
    ] {
        let body = format!("new n(`{}`) in {{ {body} }}", injected("missing-name"));
        let deploy = session
            .fail_with(
                &with_space(&body),
                &format!("rrs-p5-hooks-params-{label}"),
                &injections,
                &[code],
            )
            .await;
        if code == "UnknownParam" {
            assert!(
                format!("{:?}", deploy.result.errors).contains("is not a param of this channel"),
                "{label}"
            );
        }
    }
    assert!(session.records(space.space, &space.a).await.is_empty());
    assert!(session
        .data(&counter_channel(space.space, &space.a))
        .await
        .is_empty());
}

// ── Allocation jobs (F11) ────────────────────────────────────────────────────────────────────

/// A `new` in a `use` block runs one allocation job for the space of the block. The job of a
/// HashMap space reads the config and commits nothing, so the deploy makes no event on `C(sid)`
/// or `A(sid)`, and the name keeps the draw of HEAD as its tail. A missing space fails at `new`
/// with `DanglingHandle`. A `use` block without `new`, and a `new` without plain binders, run
/// no job.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hooks_new_in_use_runs_one_allocation_job() {
    let (mut session, space) = queue_session().await;
    let deploy = session
        .run(
            &with_space(
                r#"new out in { use space { new n in { out!(*n) | n!(1) } } | for (@n <- out) { @"rrs-use-new"!(n) } }"#,
            ),
            "rrs-p5-hooks-use-new",
        )
        .await;
    assert_eq!(
        events_touching(&deploy.log, &config_channel(space.space)),
        0
    );
    assert_eq!(events_touching(&deploy.log, &allocator_of(space.space)), 0);
    let minted = session.sole("rrs-use-new").await;
    assert_eq!(session.records(space.space, &minted).await, vec![(
        0,
        vec![gint(1)],
        false
    )]);
    session
        .run(
            &with_space(
                r#"new out in { new n in { out!(*n) } | for (@n <- out) { @"rrs-plain-new"!(n) } }"#,
            ),
            "rrs-p5-hooks-use-new",
        )
        .await;
    let plain = session.sole("rrs-plain-new").await;
    assert_eq!(
        namespace::sole_gprivate_id(&plain),
        Some(&random_tail(&minted, space.space)[..]),
        "the job keeps the draw of HEAD"
    );

    let injections = missing_injections();
    session
        .fail_with(
            r#"new h(`rho:rrs:test:missing-handle`) in { use h { new n in { Nil } } }"#,
            "rrs-p5-hooks-use-new-missing",
            &injections,
            &["DanglingHandle"],
        )
        .await;
    for (label, source) in [
        (
            "no-new",
            r#"new h(`rho:rrs:test:missing-handle`) in { use h { Nil } }"#,
        ),
        (
            "uris-only",
            r#"new h(`rho:rrs:test:missing-handle`) in { use h { new stdout(`rho:io:stdout`) in { Nil } } }"#,
        ),
    ] {
        let deploy = session
            .deploy_with(
                source,
                &format!("rrs-p5-hooks-use-new-{label}"),
                &injections,
            )
            .await;
        assert!(
            deploy.result.errors.is_empty(),
            "{label}: {:?}",
            deploy.result.errors
        );
    }
}

/// A `new` with space annotations runs one allocation job for each distinct space of its plain
/// binders, the space of the use-context included. Each name keeps the draw of HEAD, a binder
/// without a space keeps its name, and the jobs commit nothing on HashMap spaces. A missing
/// annotated space fails at `new` with `DanglingHandle`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hooks_newspace_runs_one_job_per_distinct_space() {
    let (mut session, space) = queue_session().await;
    session
        .run(
            r#"new factory(`rho:space:hashmap:queue`) in { for (other <- factory!?("default")) { @"rrs-other"!(*other) } }"#,
            "rrs-p5-hooks-other",
        )
        .await;
    let other = native(&session.sole("rrs-other").await);
    let mut injections = missing_injections();
    injections.insert(injected("other"), handle_of(other));
    let annotated = format!(
        r#"new o(`{}`), out in {{ new x : space, y, z : o, w : space in {{ out!([*x, *y, *z, *w]) }} | for (@names <- out) {{ @"rrs-annotated"!(names) }} }}"#,
        injected("other")
    );
    let deploy = session
        .deploy_with(
            &with_space(&annotated),
            "rrs-p5-hooks-newspace",
            &injections,
        )
        .await;
    assert!(
        deploy.result.errors.is_empty(),
        "{:?}",
        deploy.result.errors
    );
    for checked in [space.space, other] {
        assert_eq!(events_touching(&deploy.log, &config_channel(checked)), 0);
        assert_eq!(events_touching(&deploy.log, &allocator_of(checked)), 0);
    }
    let plain = format!(
        r#"new o(`{}`), out in {{ new x, y, z, w in {{ out!([*x, *y, *z, *w]) }} | for (@names <- out) {{ @"rrs-plain"!(names) }} }}"#,
        injected("other")
    );
    let deploy = session
        .deploy_with(&with_space(&plain), "rrs-p5-hooks-newspace", &injections)
        .await;
    assert!(
        deploy.result.errors.is_empty(),
        "{:?}",
        deploy.result.errors
    );
    let names = list_elements(&session.sole("rrs-annotated").await);
    let draws = list_elements(&session.sole("rrs-plain").await);
    assert_eq!(names.len(), 4);
    for (index, owner) in [
        (0, Some(space.space)),
        (1, None),
        (2, Some(other)),
        (3, Some(space.space)),
    ] {
        match owner {
            Some(owner) => {
                assert_eq!(
                    classify_channel(&names[index]),
                    ChannelClass::SpaceName(owner)
                );
                assert_eq!(
                    namespace::sole_gprivate_id(&draws[index]),
                    Some(&random_tail(&names[index], owner)[..]),
                    "binder {index} keeps the draw of HEAD"
                );
            }
            None => assert_eq!(names[index], draws[index], "binder {index} keeps its name"),
        }
    }
    let deploy = session
        .deploy_with(
            &with_space(&format!(
                r#"new o(`{}`), out in {{ use o {{ new u : space, v in {{ out!([*u, *v]) }} }} | for (@names <- out) {{ @"rrs-in-use"!(names) }} }}"#,
                injected("other")
            )),
            "rrs-p5-hooks-newspace-use",
            &injections,
        )
        .await;
    assert!(
        deploy.result.errors.is_empty(),
        "{:?}",
        deploy.result.errors
    );
    let in_use = list_elements(&session.sole("rrs-in-use").await);
    assert_eq!(
        classify_channel(&in_use[0]),
        ChannelClass::SpaceName(space.space)
    );
    assert_eq!(classify_channel(&in_use[1]), ChannelClass::SpaceName(other));
    session
        .fail_with(
            &with_space(
                r#"new m(`rho:rrs:test:missing-handle`) in { new x : space, y : m in { Nil } }"#,
            ),
            "rrs-p5-hooks-newspace-missing",
            &injections,
            &["DanglingHandle"],
        )
        .await;
}

/// The handle method `gensym` runs one allocation job first. On a HashMap space the job answers
/// `Baseline`, so the reply is the random-tail name of P4, and the call makes no event on
/// `C(sid)` or `A(sid)`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hooks_gensym_runs_an_allocation_job() {
    let (mut session, space) = queue_session().await;
    let deploy = session
        .run(
            &with_space(r#"for (n <- space!?("gensym")) { @"rrs-gensym"!(*n) }"#),
            "rrs-p5-hooks-gensym",
        )
        .await;
    let name = session.sole("rrs-gensym").await;
    random_tail(&name, space.space);
    assert_eq!(
        events_touching(&deploy.log, &config_channel(space.space)),
        0
    );
    assert_eq!(events_touching(&deploy.log, &allocator_of(space.space)), 0);
    assert!(session.data(&allocator_of(space.space)).await.is_empty());
}

// ── Replay ───────────────────────────────────────────────────────────────────────────────────

/// One deploy of the replay test: its label, its source and its expected error codes.
struct Program {
    label: &'static str,
    source: String,
    codes: Vec<&'static str>,
}

/// Plays `program` as Casper plays a user deploy: a soft checkpoint, the evaluation, the event
/// log, the revert of a failed deploy, and a checkpoint for the root.
async fn play_program(
    runtime: &mut RhoRuntimeImpl,
    program: &Program,
    injections: &Injections,
) -> (EvaluateResult, Vec<Event>, String) {
    let fallback = runtime.create_soft_checkpoint().await;
    let result = evaluate(runtime, program, injections).await;
    let log = runtime.take_event_log().await;
    if !result.errors.is_empty() {
        runtime.revert_to_soft_checkpoint(fallback).await;
    }
    let root = runtime.create_checkpoint().await.root;
    (result, log, format!("{root:?}"))
}

async fn evaluate(
    runtime: &RhoRuntimeImpl,
    program: &Program,
    injections: &Injections,
) -> EvaluateResult {
    runtime
        .evaluate(
            &program.source,
            Cost::create(support::session::PHLO, program.label.to_string()),
            injections.clone(),
            Blake2b512Random::create_from_bytes(program.label.as_bytes()),
        )
        .await
        .unwrap_or_else(|error| panic!("{}: no result: {error:?}", program.label))
}

/// Replay reproduces play for the deploys of the hooks: a `new` in a `use` block, a `new` with
/// annotations, `gensym`, a rejected query and rejected params. Each replay has the codes, the
/// cost and the root of its play, and it uses every event of the play log.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hooks_replay_equal() {
    let (mut session, _space) = queue_session().await;
    session.play.create_checkpoint().await;
    let start = session.play.get_root().await;
    let injections = missing_injections();
    let programs = [
        Program {
            label: "rrs-p5-hooks-replay-use-new",
            source: with_space(
                r#"use space { new n in { n!(1) | for (@v <- n) { @"rrs-replay-v"!(v) } } }"#,
            ),
            codes: Vec::new(),
        },
        Program {
            label: "rrs-p5-hooks-replay-newspace",
            source: with_space(r#"new x : space, y in { x!(2) | y!(3) }"#),
            codes: Vec::new(),
        },
        Program {
            label: "rrs-p5-hooks-replay-gensym",
            source: with_space(r#"for (n <- space!?("gensym")) { @"rrs-replay-g"!(*n) }"#),
            codes: Vec::new(),
        },
        Program {
            label: "rrs-p5-hooks-replay-query",
            source: with_space(r#"for (@x <- a ~ 1) { Nil }"#),
            codes: vec!["QuerySpecUnsupported"],
        },
        Program {
            label: "rrs-p5-hooks-replay-params",
            source: with_space("a!(1; k=1)"),
            codes: vec!["UnknownParam"],
        },
        Program {
            label: "rrs-p5-hooks-replay-missing",
            source: r#"new h(`rho:rrs:test:missing-handle`) in { use h { new n in { Nil } } }"#
                .to_string(),
            codes: vec!["DanglingHandle"],
        },
    ];
    let mut played = Vec::with_capacity(programs.len());
    for program in &programs {
        let (result, log, root) = play_program(&mut session.play, program, &injections).await;
        assert_eq!(
            error_codes(&result.errors),
            program.codes,
            "{}: the play codes",
            program.label
        );
        played.push((result.cost.value, log, root));
    }
    session
        .replay
        .reset(&start)
        .await
        .expect("reset the replay runtime to the start root");
    for (program, (cost, log, root)) in programs.iter().zip(played) {
        session
            .replay
            .rig(log)
            .await
            .expect("rig the replay runtime with the play log");
        let fallback = session.replay.create_soft_checkpoint().await;
        let result = evaluate(&session.replay, program, &injections).await;
        session.replay.take_event_log().await;
        if !result.errors.is_empty() {
            session.replay.revert_to_soft_checkpoint(fallback).await;
        }
        session
            .replay
            .check_replay_data()
            .await
            .unwrap_or_else(|error| {
                panic!("{}: the replay leaves events: {error:?}", program.label)
            });
        assert_eq!(
            error_codes(&result.errors),
            program.codes,
            "{}: the replay codes",
            program.label
        );
        assert_eq!(
            result.cost.value, cost,
            "{}: the replay cost",
            program.label
        );
        let replayed = session.replay.create_checkpoint().await.root;
        assert_eq!(
            format!("{replayed:?}"),
            root,
            "{}: the replay root",
            program.label
        );
    }
}

// ── Native execution (decision 46) ───────────────────────────────────────────────────────────

/// The deploy signature of every native run.
const NATIVE_DEPLOY: &[u8] = b"rrs-p5-hooks-native";
/// The native phlo limit of a deploy.
const NATIVE_LIMIT: u64 = 1_000_000;
/// The weights of the compute, introduction, transfer and trace classes, and the price.
const NATIVE_WEIGHTS: [u64; 4] = [0, 1, 1, 0];
const NATIVE_PRICE: u64 = 3;
/// The seed of the random state of a native deploy.
const NATIVE_SEED: &[u8] = b"rrs-p5-hooks-native-random";

/// A native deploy whose only space work is the allocation job of `new` in a `use` block.
const NATIVE_NEW_IN_USE: &str =
    r#"new h(`rho:rrs:test:missing-handle`) in { use h { new n in { Nil } } }"#;

/// The control: the same `use` block without `new` does no space work.
const NATIVE_USE_ONLY: &str = r#"new h(`rho:rrs:test:missing-handle`) in { use h { Nil } }"#;

type RhoHistoryRepository = Arc<
    Box<
        dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
            + Send
            + Sync,
    >,
>;

fn native_schedule() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"native-test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(NATIVE_WEIGHTS)
            .zip([b"c".as_slice(), b"i", b"t", b"r"])
            .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
            .collect(),
        actual_price: NATIVE_PRICE,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

/// Runs `action` with the native phlo contract of every native run of this suite.
fn with_contract<T>(action: impl FnOnce(NativePhloExecutionContract<'_>) -> T) -> T {
    let descriptor = native_schedule();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS)
        .expect("the test schedule binds");
    let selected = binding.schedule();
    let schedules = [selected];
    let owner_ceilings = [NATIVE_PRICE];
    let controls = check_phlo_controls(
        selected.environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: NATIVE_LIMIT,
            price_ceiling: NATIVE_PRICE,
            required_owner_ceilings: &owner_ceilings,
            permitted_schedules: &schedules,
        },
        selected,
        NATIVE_LIMIT,
    )
    .expect("the test controls check");
    let contract =
        NativePhloExecutionContract::new(controls, &binding).expect("the test contract builds");
    action(contract)
}

fn native_budget_limits() -> NativeBudgetTraceLimits {
    NativeBudgetTraceLimits {
        attempts: 100_000,
        path_segments: 4096,
        regions: NativePhloRegionLimits {
            regions: 1000,
            encoded_authority_bytes: 10_000_000,
        },
    }
}

fn native_host_work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn native_config(host: HostWorkBudget) -> NativeRuntimeConfig {
    with_contract(|contract| NativeRuntimeConfig::new(contract, native_budget_limits(), host))
}

fn native_random() -> Blake2b512Random { Blake2b512Random::create_from_bytes(NATIVE_SEED) }

fn native_funding() -> rholang::rust::interpreter::accounting::Sig {
    funding_sig_compound(&[&1u64.to_be_bytes()])
}

/// The play of one native deploy.
struct NativePlay {
    result: EvaluateResult,
    log: Vec<Event>,
    history: RhoHistoryRepository,
    /// The channels of the hot store that belong to a native space: every Reified id but the
    /// 2-byte op channels, whose system processes the runtime installs.
    space_channels: usize,
}

/// Plays `term` as a native deploy on a fresh runtime over the empty state, with the injected
/// names of the suite.
async fn native_play(term: &str) -> NativePlay {
    let mut stores = InMemoryStoreManager::new();
    let (space, _) =
        RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory play and replay space");
    space.create_checkpoint().await.expect("the empty root");
    let history = space.get_history_repository();
    let mut runtime = create_rho_runtime(
        space,
        Arc::new(default_mergeable_tags()),
        false,
        &mut Vec::new(),
        ExternalServices::noop(),
    )
    .await
    .expect("a native play runtime");
    runtime
        .cost
        .set_deploy_signature_funded(NATIVE_DEPLOY, native_funding());
    let result = runtime
        .evaluate_with_native_phlo(
            term,
            missing_injections(),
            native_random(),
            None,
            native_config(native_host_work()),
        )
        .await
        .expect("native play evaluates");
    let log = runtime.create_soft_checkpoint().await.log;
    let space_channels = runtime
        .get_hot_changes()
        .await
        .keys()
        .flatten()
        .filter(|channel| {
            matches!(
                namespace::reified_id_of(channel),
                Some(id) if !matches!(id, ReifiedId::Op(_))
            )
        })
        .count();
    NativePlay {
        result,
        log: log.into_iter().collect(),
        history,
        space_channels,
    }
}

/// Replays the play in the native replay environment, as proposer self-certification and
/// validator replay do. It returns the replay result, the completeness of the replay, and the
/// phlo that the replay used.
async fn native_replay(
    played: &NativePlay,
    term: &str,
) -> (EvaluateResult, Result<(), String>, u64) {
    let recording = played
        .result
        .native_budget_recording
        .clone()
        .expect("native play records its budget");
    let operations = played
        .result
        .native_operation_recording
        .clone()
        .expect("native play records its operations");
    let host = native_host_work();
    let trace = with_contract(|contract| {
        contract.check_operation_journal(
            recording.session,
            &recording,
            operations,
            NativeOperationJournalLimits {
                budget: native_budget_limits(),
                operations: 100_000,
                total_path_segments: 1_000_000,
                source_entries: 1_000_000,
                footprint_entries: 1_000_000,
                footprint_bytes: 10_000_000,
                predecessor_edges: 1_000_000,
            },
            &host,
        )
    })
    .expect("the play journal checks")
    .bind_trace(
        played.log.clone().into(),
        NativeOperationTraceLimits {
            events: 100_000,
            source_entries: 1_000_000,
            source_bytes: 10_000_000,
            telemetry_items: 100_000,
            telemetry_bytes: 10_000_000,
        },
        &host,
    )
    .expect("the play log binds to the journal");
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(NATIVE_DEPLOY, native_funding());
    budget
        .reset_for_native_execution(native_config(host.clone()))
        .expect("the replay budget enters native execution");
    let mut environment = create_native_replay_env(
        trace,
        played.history.clone(),
        Arc::new(Box::new(Matcher)),
        host,
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget,
        ExternalServices::noop(),
    )
    .await
    .expect("the native replay environment builds");
    let parsed = Compiler::source_to_adt_with_normalizer_env(term, missing_injections())
        .expect("the program parses");
    let replayed = environment
        .evaluate(parsed, native_random())
        .await
        .expect("native replay evaluates");
    let complete = environment
        .check_complete()
        .await
        .map_err(|error| error.to_string());
    let usage = environment.completed_usage().await.unwrap_or(u64::MAX);
    (replayed, complete, usage)
}

// Disabled by decision 47: native execution runs the allocation job of `new` in a `use`
// block, so no deploy fails with `SpacesUnsupportedInNativeExecution`. Its successor is
// `hooks_new_in_use_native_play_and_replay_agree`.
//
// ```
// /// Decision 46: the allocation job of `new` in a `use` block is a space entry, so native play
// /// rejects the deploy at `new` with `SpacesUnsupportedInNativeExecution`, before any space work,
// /// and native replay reproduces the rejection. The same `use` block without `new` makes no space
// /// entry, so it plays natively without an error.
// #[tokio::test]
// async fn hooks_new_in_use_fails_closed_in_native_execution() {
//     let played = native_play(NATIVE_NEW_IN_USE).await;
//     assert_eq!(
//         played.result.errors,
//         vec![InterpreterError::ReduceError(
//             "reified:SpacesUnsupportedInNativeExecution: native execution does not support Reified RSpaces"
//                 .to_owned(),
//         )]
//     );
//     assert_eq!(
//         played.space_channels, 0,
//         "native play staged no record on a channel of a space"
//     );
//     let (replayed, complete, usage) = native_replay(&played, NATIVE_NEW_IN_USE).await;
//     assert_eq!(replayed.errors, played.result.errors);
//     assert_eq!(
//         complete,
//         Ok(()),
//         "replay consumed every journal row of play"
//     );
//     let recording = played
//         .result
//         .native_budget_recording
//         .as_ref()
//         .expect("native play records its budget");
//     assert_eq!(usage, recording.used, "replay used the phlo of play");
//
//     let control = native_play(NATIVE_USE_ONLY).await;
//     assert!(
//         control.result.errors.is_empty(),
//         "{:?}",
//         control.result.errors
//     );
//     assert_eq!(control.space_channels, 0);
// }
// ```

/// Decision 47: native execution runs the allocation job of `new` in a `use` block. The handle
/// names a missing space, so the job fails with `DanglingHandle` in native play, after the read
/// of the config and before any staged operation. Since P7 (DG-3), the error names the space id
/// of the handle. Native replay reproduces the failure: the same error, a complete journal and
/// the phlo usage of play. The same `use` block without `new` makes no space entry, so it plays
/// natively without an error.
#[tokio::test]
async fn hooks_new_in_use_native_play_and_replay_agree() {
    let played = native_play(NATIVE_NEW_IN_USE).await;
    assert_eq!(played.result.errors, vec![InterpreterError::ReduceError(
        format!(
            "reified:DanglingHandle: the space {} of this handle does not exist",
            "5a".repeat(32)
        )
    )]);
    assert_eq!(
        played.space_channels, 0,
        "native play stored no record on a channel of a space"
    );
    let (replayed, complete, usage) = native_replay(&played, NATIVE_NEW_IN_USE).await;
    assert_eq!(replayed.errors, played.result.errors);
    assert_eq!(
        complete,
        Ok(()),
        "replay consumed every journal row of play"
    );
    let recording = played
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget");
    assert_eq!(usage, recording.used, "replay used the phlo of play");

    let control = native_play(NATIVE_USE_ONLY).await;
    assert!(
        control.result.errors.is_empty(),
        "{:?}",
        control.result.errors
    );
    assert_eq!(control.space_channels, 0);
}

// ── Property ─────────────────────────────────────────────────────────────────────────────────

/// The inner channel of a generated query: a host channel, a fresh unforgeable name, the space
/// name `a` of the queue space, a name of a missing space, or a forged channel in a `use` block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Inner {
    Host,
    Unforgeable,
    SpaceName,
    MissingSpace,
    ForgedInUse,
}

/// A bundle around the inner channel: none, read-only (`bundle-`), write-only (`bundle+`) or
/// without flags (`bundle0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wrapper {
    Bare,
    ReadOnly,
    WriteOnly,
    NoFlags,
}

/// One case: the inner channel, its bundle, the payload, and an optional named param `k`.
#[derive(Clone, Debug)]
struct QueryCase {
    inner: Inner,
    wrapper: Wrapper,
    payload: Vec<i64>,
    named: Option<i64>,
}

fn arb_query_case() -> impl Strategy<Value = QueryCase> {
    (
        prop_oneof![
            Just(Inner::Host),
            Just(Inner::Unforgeable),
            Just(Inner::SpaceName),
            Just(Inner::MissingSpace),
            Just(Inner::ForgedInUse),
        ],
        prop_oneof![
            Just(Wrapper::Bare),
            Just(Wrapper::ReadOnly),
            Just(Wrapper::WriteOnly),
            Just(Wrapper::NoFlags),
        ],
        proptest::collection::vec(0_i64..100, 0..3),
        proptest::option::of(1_i64..10),
    )
        .prop_map(|(inner, wrapper, mut payload, named)| {
            if payload.is_empty() && named.is_none() {
                payload.push(0);
            }
            QueryCase {
                inner,
                wrapper,
                payload,
                named,
            }
        })
}

impl QueryCase {
    /// The receive of the case as a deploy body of `with_space`.
    fn body(&self) -> String {
        let channel = match self.inner {
            Inner::Host => r#"@"rrs-p5-prop-host""#.to_string(),
            Inner::Unforgeable => "p".to_string(),
            Inner::SpaceName => "a".to_string(),
            Inner::MissingSpace => "n".to_string(),
            Inner::ForgedInUse => r#"@"rrs-p5-prop-docs""#.to_string(),
        };
        let channel = match (self.wrapper, self.inner) {
            (Wrapper::Bare, _) => channel,
            (wrapper, Inner::Host | Inner::ForgedInUse) => {
                let process = channel.trim_start_matches('@').to_string();
                format!("@{{{}{{{process}}}}}", bundle_keyword(wrapper))
            }
            (wrapper, Inner::Unforgeable | Inner::SpaceName | Inner::MissingSpace) => {
                format!("@{{{}{{*{channel}}}}}", bundle_keyword(wrapper))
            }
        };
        let payload = self
            .payload
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let query = match self.named {
            Some(value) => format!("({payload}; k={value})"),
            None => format!("({payload};)"),
        };
        let receive = format!("for (@x <- {channel} ~ {query}) {{ @\"rrs-p5-prop-got\"!(x) }}");
        let receive = match self.inner {
            Inner::ForgedInUse => format!("use space {{ {receive} }}"),
            Inner::Host | Inner::Unforgeable | Inner::SpaceName | Inner::MissingSpace => receive,
        };
        format!(
            "new p, n(`{}`) in {{ {receive} }}",
            injected("missing-name")
        )
    }

    /// The model of `query_source_routes_inner_channel` (`SpaceRefs.v`): a bundle without the
    /// read flag fails with the text of HEAD. Otherwise the inner channel routes: the default
    /// space fails with `QuerySpecUnsupported`, the queue space rejects the query in its agent
    /// with the same code, and a missing space fails in its agent with `DanglingName`.
    fn expected(&self) -> &'static str {
        match (self.wrapper, self.inner) {
            (Wrapper::WriteOnly | Wrapper::NoFlags, _) => NOT_READABLE,
            (Wrapper::Bare | Wrapper::ReadOnly, Inner::MissingSpace) => "DanglingName",
            (
                Wrapper::Bare | Wrapper::ReadOnly,
                Inner::Host | Inner::Unforgeable | Inner::SpaceName | Inner::ForgedInUse,
            ) => "QuerySpecUnsupported",
        }
    }
}

fn bundle_keyword(wrapper: Wrapper) -> &'static str {
    match wrapper {
        Wrapper::Bare => "",
        Wrapper::ReadOnly => "bundle-",
        Wrapper::WriteOnly => "bundle+",
        Wrapper::NoFlags => "bundle0",
    }
}

/// Property of `query_source_routes_inner_channel` (`SpaceRefs.v`, P5b), end to end through
/// Rholang: a receive with a query routes the inner channel of the query, under the read rule of
/// HEAD. Each deploy fails with the code of the model, leaves no space data, takes no record of
/// the queue and bumps no counter, so the receive never becomes a waiter.
#[test]
fn prop_query_source_routes_inner_channel() {
    let runtime = tokio_runtime();
    let (session, space) = runtime.block_on(async {
        let (mut session, space) = queue_session().await;
        session.run(&with_space("a!(1)"), "rrs-p5-prop-seed").await;
        (session, space)
    });
    let session = Mutex::new(session);
    let counter =
        runtime.block_on(async { session.lock().await.counter(space.space, &space.a).await });
    let injections = missing_injections();
    check(runtime_config(), arb_query_case(), |case| {
        runtime.block_on(async {
            let mut session = session.lock().await;
            let label = format!("rrs-p5-prop-{case:?}");
            let deploy = session
                .deploy_with(&with_space(&case.body()), &label, &injections)
                .await;
            prop_assert_eq!(
                deploy.codes(),
                vec![case.expected().to_string()],
                "{:?}",
                case
            );
            prop_assert!(deploy.residue.is_empty(), "{:?} left space data", case);
            prop_assert_eq!(session.records(space.space, &space.a).await, vec![(
                0,
                vec![gint(1)],
                false
            )]);
            prop_assert_eq!(session.counter(space.space, &space.a).await, counter);
            prop_assert!(session.values("rrs-p5-prop-got").await.is_empty());
            Ok::<(), TestCaseError>(())
        })
    });
}
