//! The theory seam of P7 (design v3, section 7: TH-1 to TH-10).
//!
//! A factory call may name a builtin theory, `free Th()` with `Th` one of `Nat`, `Int`, `String`,
//! `Bool` and `Any`. The config of the space persists the identity of the theory, and each config
//! read resolves it. The theory admits each value of a send before any staging, and it checks
//! each stored datum that a receive matches, after the `where` guard.
//!
//! Every deploy plays natively at the v6 limits and certifies through its committed evidence,
//! as proposer self-certification and validator replay do (`support::native`). The checkers with
//! a lease, a refusal at a match or a starved limit profile exist only in test builds (TH-10), so
//! their tests are library tests (`spaces::theory::tests`, `spaces::generic::tests`).
//!
//! | Test | Property |
//! |---|---|
//! | `space_theory_admission` | TH-4 to TH-6 on every family program, on each racing runtime: with `free Nat()` and with no theory, every play certifies and delivers an allowed label set. `free String()` refuses the first send with `TheoryRefuted`. |
// Changed by decision 93 (decision 111): the deliveries race, so each play certifies and delivers
// an allowed label set. The old row follows.
// //! | `space_theory_admission` | TH-4 to TH-6 on every family program: `free Nat()` admits every value of the program and delivers what the space without a theory delivers. `free String()` refuses the first send with `TheoryRefuted`. |
// Added by decision 124 (F2): the PathMap peek row.
//! | `space_theory_pathmap_peek_then_send_certifies` | F2, on each racing runtime: a PathMap bag with `free Nat()` parks a peek waiter on `a` in one deploy. In the next deploy a send on `[*a, 1]` fires it under the vehicle of the prefix and then stores the element. Play records the fire row before the Stored row, and both deploys certify. The family without a theory is the control. |
//! | `space_theory_admission_vectordb` | TH-5 and TH-6 on the two VectorDB families, on each racing runtime: with `free Any()` and with no theory, every play certifies and delivers one list and one near vector. `free Nat()` refuses a vector. |
// Changed by decision 93 (decision 111): the deliveries race, so each play certifies and delivers
// one list and one near vector. The old row follows.
// //! | `space_theory_admission_vectordb` | TH-5 and TH-6 on the two VectorDB families: `free Any()` admits vectors and delivers the lists of the space without a theory, and `free Nat()` refuses a vector. |
//! | `theory_nat_accepts_bigint_nonneg` | TH-1: `Nat` admits `GInt` values of at least 0 and non-negative `GBigInt` values, and a receive takes them. `Int` admits negative big integers. |
//! | `theory_nat_rejects_minus1_string_par` | TH-1 and TH-5: `Nat` refuses a negative number, the string `"Nat(42)"`, a par of two values and every other shape, through a routed send, a persistent send and the handle `produce`. The error names the index of the first refused value. |
//! | `theory_identity_persisted_no_handle` | TH-2: the config of the space holds the canonical identity map of the theory, which holds content only: two spaces with one theory have equal theory slots, and the slot holds no name. |
//! | `theory_unknown_name_suggestion` | TH-4: another name fails with `UnknownTheory` and the nearest builtin within two edits. |
//! | `theory_builtin_args_rejected` | TH-4: a builtin with arguments fails with `BadConstructorArgument`. |
//! | `space_guard_match` | TH-6: a stored datum that the guard and the theory admit fires its receive, and the same receive with a refusing guard parks, on each collection, a join and a prefix receive. |
//! | `prop_theory_builtin_decidable` | `builtin_decidable_structural` (`TheoryAdmission.v`): each builtin decides every value by its shape, never `Undetermined` with the structural limits, with one unit of work. |
//! | `prop_theory_fail_closed` | `fail_closed`: a send is admitted only when every value is `Proven`. Otherwise it fails with the index of its first value that is not. |
//! | `prop_theory_usage_cache_transparent` | `usage_cache_transparent`: the verdict and the usage of a check depend only on the identity and the value, not on earlier checks or on a fresh resolution. |
//! | `prop_resolve_identity_deterministic` | `resolve_identity_deterministic`: the resolution is a pure function of the identity, and it resolves exactly the structural builtin identities. |
//! | `prop_replay_preserves_theory_admission` | `replay_preserves_theory_admission`: native replay reproduces the admission or the refusal of every generated send, with the errors, the phlo and the root of play. |

#[path = "support/mod.rs"]
mod support;

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::BTreeSet;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, ListParWithRandom, Par};
use proptest::prelude::{any, prop_oneof, Just, Strategy};
use proptest::sample::select;
use proptest::test_runner::TestCaseError;
// Added by decision 124 (F2): the rows of the journal of a PathMap offer.
use rholang::rust::interpreter::accounting::{NativeOperationRecord, NativeOperationSource};
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::theory::builtin::BUILTIN_NAMES;
use rholang::rust::interpreter::spaces::theory::identity::{TheoryIdentity, STRUCTURAL_LIMITS};
use rholang::rust::interpreter::spaces::theory::{
    self, admit_values, SpaceTheory, TheoryUsage, TheoryVerdict,
};
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
// Added by decision 124 (F2): the completion of a row of a PathMap offer.
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
// Added by decision 124 (F2): the families of the PathMap peek test.
use support::family_programs::StoredFamily;
// Changed by decision 93 (decision 111): the admission tests check the allowed label sets and
// run on the racing runtimes. The old imports follow.
// use support::family_programs::{space_program, Inner, Outer, JOIN};
use support::family_programs::{allowed_labels, space_program, Inner, Outer, JOIN};
use support::native::{assert_reproduces, certify, native_play, Played, AMPLE_LIMIT, OUT};
// Added by decision 124 (F2): the rows of a committed journal and the deploys of a chain.
use support::native::{committed_evidence, is_space_row, NativeChain};
// use support::props::{check, pure_config, runtime_config, tokio_runtime};
use support::props::{
    check, pure_config, racing_runtime, runtime_config, tokio_runtime, RACING_WORKERS,
};
use support::session::error_codes;

// ─── Helpers ───────────────────────────────────────────────────────────────────────────────────

/// The phlo limit of the deploys of this suite, as in the Seq suite.
const THEORY_LIMIT: u64 = 20 * AMPLE_LIMIT;

/// Plays `term` natively at the v6 limits and certifies it.
async fn play_and_certify(label: &str, term: &str) -> Played {
    let played = native_play(term, THEORY_LIMIT).await;
    assert!(
        played.result.native_budget_recording.is_some(),
        "{label}: the deploy evaluated: {:?}",
        played.result.errors
    );
    let certification = certify(&played, term, THEORY_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    played
}

/// The distinct error codes of `played`.
fn codes(played: &Played) -> BTreeSet<String> {
    error_codes(&played.result.errors).into_iter().collect()
}

/// The single code `code`.
fn only(code: &str) -> BTreeSet<String> { BTreeSet::from([code.to_string()]) }

/// The Debug forms of the errors of `played`, joined.
fn messages(played: &Played) -> String { format!("{:?}", played.result.errors) }

/// The one expression of `par`, when it has exactly one.
fn sole_expr(par: &Par) -> Option<&ExprInstance> {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(instance),
        }] => Some(instance),
        _ => None,
    }
}

/// The elements of the list `par`, when it is one list.
fn elements(par: &Par) -> Option<&[Par]> {
    match sole_expr(par) {
        Some(ExprInstance::EListBody(list)) => Some(list.ps.as_slice()),
        _ => None,
    }
}

/// The string of `par`, when it is one string.
fn string_of(par: &Par) -> Option<&str> {
    match sole_expr(par) {
        Some(ExprInstance::GString(value)) => Some(value.as_str()),
        _ => None,
    }
}

/// The outputs `[label, value…]` of `played` with the label `label`: the values after the label.
fn outputs(played: &Played, label: &str) -> Vec<Vec<Par>> {
    played
        .out
        .iter()
        .flatten()
        .filter_map(|value| match elements(value)? {
            [first, rest @ ..] if string_of(first) == Some(label) => Some(rest.to_vec()),
            _ => None,
        })
        .collect()
}

/// The labels of the outputs of `played`, in bytewise order.
fn labels(played: &Played) -> Vec<String> {
    let mut labels: Vec<String> = played
        .out
        .iter()
        .flatten()
        .filter_map(|value| {
            elements(value)?
                .first()
                .and_then(string_of)
                .map(str::to_string)
        })
        .collect();
    labels.sort();
    labels
}

/// The constructor of a factory call with the `default` qualifier, the theory `theory` and the
/// params of `outer`.
fn ctor(outer: Outer, theory: Option<&str>) -> String {
    let free = theory
        .map(|name| format!(", free {name}()"))
        .unwrap_or_default();
    match outer {
        Outer::Array => format!("\"default\"{free}; size=8"),
        Outer::HashMap | Outer::PathMap | Outer::Vector => format!("\"default\"{free}"),
    }
}

/// The family program of `outer:inner` with the theory `theory`.
fn family_program(outer: Outer, inner: Inner, theory: Option<&str>) -> String {
    space_program(
        &format!("rho:space:{}:{}", outer.name(), inner.name()),
        &ctor(outer, theory),
        &format!("{} |\n{} |\n{}", inner.part(), JOIN, outer.part()),
    )
}

/// A program over a space of `hashmap:inner` with the theory `theory`, which runs `body`.
fn theory_program(inner: &str, theory: &str, body: &str) -> String {
    space_program(
        &format!("rho:space:hashmap:{inner}"),
        &format!("\"default\", free {theory}()"),
        body,
    )
}

/// The value of the two's complement big-endian integer of `par`: a `GInt` or a `GBigInt` that
/// fits in an `i128`.
fn integer_of(par: &Par) -> Option<i128> {
    match sole_expr(par)? {
        ExprInstance::GInt(value) => Some(i128::from(*value)),
        ExprInstance::GBigInt(bytes) => match bytes.len() {
            0 => Some(0),
            1..=16 => {
                let fill = match bytes.first() {
                    Some(first) if first & 0x80 != 0 => 0xff,
                    Some(_) | None => 0x00,
                };
                let mut wide = [fill; 16];
                wide[16 - bytes.len()..].copy_from_slice(bytes);
                Some(i128::from_be_bytes(wide))
            }
            _ => None,
        },
        _ => None,
    }
}

/// The value of the key `key` of the map `map`.
fn entry(map: &Par, key: &str) -> Option<Par> {
    match sole_expr(map)? {
        ExprInstance::EMapBody(body) => body.kvs.iter().find_map(|pair| {
            match pair.key.as_ref().and_then(string_of) == Some(key) {
                true => pair.value.clone(),
                false => None,
            }
        }),
        _ => None,
    }
}

/// True when `par` holds a name anywhere: an unforgeable, a bundle, or a value that holds one.
fn holds_name(par: &Par) -> bool {
    !par.unforgeables.is_empty()
        || !par.bundles.is_empty()
        || par.exprs.iter().any(|expr| match &expr.expr_instance {
            Some(ExprInstance::EListBody(list)) => list.ps.iter().any(holds_name),
            Some(ExprInstance::ETupleBody(tuple)) => tuple.ps.iter().any(holds_name),
            Some(ExprInstance::EMapBody(map)) => map.kvs.iter().any(|pair| {
                pair.key.as_ref().is_some_and(holds_name)
                    || pair.value.as_ref().is_some_and(holds_name)
            }),
            Some(_) | None => false,
        })
}

// ─── TH-4 to TH-6 on the family programs ──────────────────────────────────────────────────────

/// TH-4 to TH-6: every family program sends only natural numbers, so `free Nat()` admits each
/// send and checks each stored datum that a receive, a join or a prefix receive matches. On each
/// racing runtime, the plays with `free Nat()` and with no theory certify and deliver an allowed
/// label set (decision 93). With `free String()`, the first send of the program aborts the deploy
/// with `TheoryRefuted`.
// Changed by decision 93 (decision 111): the deliveries race, so each play certifies and delivers
// an allowed label set, and the body moved to `theory_admission_run`. The old lines follow.
// /// space then delivers exactly what the space without a theory delivers. With `free String()`,
// /// the first send of the program aborts the deploy with `TheoryRefuted`.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_theory_admission() {
#[test]
fn space_theory_admission() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(theory_admission_run());
    }
}

/// Added by decision 93 (decision 111): the body of [`space_theory_admission`] on one racing
/// runtime.
async fn theory_admission_run() {
    for outer in Outer::ALL {
        for inner in Inner::ALL {
            let family = format!("{}:{}", outer.name(), inner.name());
            // Changed by decision 93 (decision 111): the baseline certifies too. The old line
            // follows.
            // let baseline = native_play(&family_program(outer, inner, None), THEORY_LIMIT).await;
            let baseline = play_and_certify(&family, &family_program(outer, inner, None)).await;
            assert!(
                baseline.result.errors.is_empty(),
                "{family}: {:?}",
                baseline.result.errors
            );
            assert!(
                allowed_labels(outer, inner, &labels(&baseline)),
                "{family}: the deliveries without a theory: {:?}",
                labels(&baseline)
            );
            let admitted = play_and_certify(
                &format!("{family} Nat"),
                &family_program(outer, inner, Some("Nat")),
            )
            .await;
            assert!(
                admitted.result.errors.is_empty(),
                "{family} Nat: {:?}",
                admitted.result.errors
            );
            // Changed by decision 93 (decision 111): the deliveries race, so each play delivers
            // an allowed label set. The old check follows.
            // assert_eq!(
            //     admitted.out, baseline.out,
            //     "{family}: the theory changes no delivery"
            // );
            assert!(
                allowed_labels(outer, inner, &labels(&admitted)),
                "{family} Nat: the deliveries: {:?}",
                labels(&admitted)
            );
            assert!(
                !labels(&admitted).is_empty(),
                "{family}: the program delivers"
            );
            let refused = play_and_certify(
                &format!("{family} String"),
                &family_program(outer, inner, Some("String")),
            )
            .await;
            assert_eq!(codes(&refused), only("TheoryRefuted"), "{family} String");
            assert!(
                messages(&refused).contains("the theory String refutes the value 0 of the send"),
                "{family}: {}",
                messages(&refused)
            );
        }
    }
}

/// Added by decision 124 (F2, base one row source for each produce, replay_rspace.rs:1058-1082):
/// the families of [`space_theory_pathmap_peek_then_send_certifies`]. Block 1 parks a peek
/// waiter on `a`. Block 2 sends 5 on the path `[*a, 1]`, whose prefix `[*a]` is the path of `a`.
/// The first family has the theory of [`space_theory_admission`], so the commit of the waiter
/// may veto. The second family has no theory. It is the control.
const PEEK_FAMILIES: [StoredFamily; 2] = [
    StoredFamily {
        label: "pathmap-peek-nat",
        urn: "rho:space:pathmap:bag",
        ctor: r#""default", free Nat()"#,
        state: r#"for (@y <<- a) { out!(["peek", y]) }"#,
        touch: r#"use space { @[*a, 1]!(5) }"#,
    },
    StoredFamily {
        label: "pathmap-peek",
        urn: "rho:space:pathmap:bag",
        ctor: r#""default""#,
        state: r#"for (@y <<- a) { out!(["peek", y]) }"#,
        touch: r#"use space { @[*a, 1]!(5) }"#,
    },
];

/// Added by decision 124 (F2, base one row source for each produce, replay_rspace.rs:1058-1082):
/// a PathMap offer records its fire under the vehicle of the element on the fired prefix and its
/// store under the element. On each racing runtime and for each family of [`PEEK_FAMILIES`],
/// the waiter parks in one deploy and the send fires it in the next, so the order is fixed.
/// Play records the peek fire row before the Stored row, and replay certifies both deploys.
#[test]
fn space_theory_pathmap_peek_then_send_certifies() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(pathmap_peek_run());
    }
}

/// Added by decision 124 (F2): the body of [`space_theory_pathmap_peek_then_send_certifies`] on
/// one racing runtime.
async fn pathmap_peek_run() {
    for family in &PEEK_FAMILIES {
        let label = family.label;
        let mut chain = NativeChain::new().await;
        let creation = family.creation();
        let created = chain
            .play(&creation, &format!("{label} creation"), THEORY_LIMIT)
            .await;
        assert!(
            created.result.errors.is_empty(),
            "{label} creation: {:?}",
            created.result.errors
        );
        assert!(
            outputs(&created, label).is_empty(),
            "{label}: the waiter parks"
        );
        let certification = certify(&created, &creation, THEORY_LIMIT).await;
        assert_reproduces(&format!("{label} creation"), &created, &certification);
        let send = family.with_stash(family.touch);
        let sent = chain
            .play(&send, &format!("{label} send"), THEORY_LIMIT)
            .await;
        assert!(
            sent.result.errors.is_empty(),
            "{label} send: {:?}",
            sent.result.errors
        );
        assert_offer_rows(label, &sent);
        let delivered = outputs(&sent, label);
        let peeks: Vec<Option<&str>> = delivered
            .iter()
            .map(|rest| match rest.as_slice() {
                [pair] => elements(pair)
                    .and_then(|pair| pair.first())
                    .and_then(string_of),
                _ => None,
            })
            .collect();
        assert_eq!(
            peeks,
            vec![Some("peek")],
            "{label}: the waiter peeks the element once"
        );
        let certification = certify(&sent, &send, THEORY_LIMIT).await;
        assert_reproduces(&format!("{label} send"), &sent, &certification);
    }
}

/// Added by decision 124 (F2): checks the rows of the offer of `played` before its replay: a
/// peek fire row of a produce, then a later Stored row of a produce on another channel, which
/// waits on the fire row or a later row. So a replay that stages the store first waits forever.
fn assert_offer_rows(label: &str, played: &Played) {
    let (_, journal) = committed_evidence(played);
    let space_rows: Vec<(usize, &NativeOperationRecord)> = journal
        .iter()
        .enumerate()
        .filter(|(_, row)| is_space_row(row))
        .collect();
    let Some((fire_at, fire)) = space_rows
        .iter()
        .find(|(_, row)| matches!(&row.comm, Some(comm) if !comm.source.peeks.is_empty()))
    else {
        panic!("{label}: the offer has a peek fire row: {space_rows:?}");
    };
    let Some((store_at, store)) = space_rows.iter().find(|(at, row)| {
        at > fire_at && matches!(row.completion, RSpaceOperationCompletion::Stored)
    }) else {
        panic!("{label}: a Stored row follows the fire row: {space_rows:?}");
    };
    assert!(
        matches!(fire.completion, RSpaceOperationCompletion::Matched),
        "{label}: the fire row is Matched: {:?}",
        fire.completion
    );
    match (&fire.source, &store.source) {
        (NativeOperationSource::Produce(vehicle), NativeOperationSource::Produce(element)) => {
            assert_ne!(
                vehicle.channel, element.channel,
                "{label}: the fire and the store are on two channels"
            );
        }
        (fire_source, store_source) => {
            panic!(
                "{label}: the fire and the store are produces: {fire_source:?}, {store_source:?}"
            )
        }
    }
    assert!(
        store.predecessors.iter().any(|at| at >= fire_at),
        "{label}: the Stored row {store_at} waits on the fire row {fire_at} or a later row: {:?}",
        store.predecessors
    );
}

/// TH-5 and TH-6 on the VectorDB families: `free Any()` admits the vectors. It checks every
/// stored member of a list and the stored record of a single-mode receive. On each racing
/// runtime, the plays with `free Any()` and with no theory certify and deliver one list and one
/// near vector (decision 93). `free Nat()` refuses a vector with `TheoryRefuted`.
// Changed by decision 93 (decision 111): the deliveries race, so each play certifies and delivers
// one list and one near vector, and the body moved to `theory_admission_vectordb_run`. The old
// lines follow.
// /// TH-5 and TH-6 on the VectorDB families: `free Any()` admits the vectors, checks every stored
// /// member of a list and the stored record of a single-mode receive, and delivers what the space
// /// without a theory delivers. `free Nat()` refuses a vector with `TheoryRefuted`.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_theory_admission_vectordb() {
#[test]
fn space_theory_admission_vectordb() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(theory_admission_vectordb_run());
    }
}

/// Added by decision 93 (decision 111): the body of [`space_theory_admission_vectordb`] on one
/// racing runtime.
async fn theory_admission_vectordb_run() {
    let body = r#"a!([1.0, 0.0]) | a!([0.0, 1.0]) | a!([2.0, 0.1]) |
for (@list <- a ~ ([1.0, 0.0]; k=2)) { out!(["list", list]) } |
for (@x <- b ~ ([1.0, 0.0]; threshold=0.9)) { out!(["near", x]) } | b!([0.0, 1.0]) | b!([3.0, 0.2])"#;
    for outer in ["hashmap", "vector"] {
        let urn = format!("rho:space:{outer}:vectordb");
        let program = |theory: &str| {
            space_program(
                &urn,
                &format!("\"default\"{theory}; capacity=10, dimensions=2"),
                body,
            )
        };
        // Changed by decision 93 (decision 111): the baseline certifies too. The old line
        // follows.
        // let baseline = native_play(&program(""), THEORY_LIMIT).await;
        let baseline = play_and_certify(&format!("{outer} no theory"), &program("")).await;
        assert!(
            baseline.result.errors.is_empty(),
            "{outer}: {:?}",
            baseline.result.errors
        );
        let admitted = play_and_certify(&format!("{outer} Any"), &program(", free Any()")).await;
        assert!(
            admitted.result.errors.is_empty(),
            "{outer} Any: {:?}",
            admitted.result.errors
        );
        // Changed by decision 93 (decision 111): the deliveries race, so each play delivers one
        // list and one near vector. The old check follows.
        // assert_eq!(
        //     admitted.out, baseline.out,
        //     "{outer}: the theory changes no delivery"
        // );
        assert_eq!(labels(&baseline), vec!["list", "near"], "{outer} no theory");
        assert_eq!(labels(&admitted), vec!["list", "near"], "{outer}");
        let refused = play_and_certify(&format!("{outer} Nat"), &program(", free Nat()")).await;
        assert_eq!(codes(&refused), only("TheoryRefuted"), "{outer} Nat");
    }
}

// ─── TH-1: the builtins ───────────────────────────────────────────────────────────────────────

/// TH-1: `Nat` admits `GInt` values of at least 0 and non-negative `GBigInt` values, through a
/// routed send and the handle `produce`, and a receive takes each of them. `Int` admits the
/// negative ones too. `Nat` refuses a negative big integer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn theory_nat_accepts_bigint_nonneg() {
    let naturals: [(&str, i128); 6] = [
        ("0", 0),
        ("9223372036854775807", i128::from(i64::MAX)),
        ("0n", 0),
        ("7n", 7),
        ("18446744073709551616n", 1 << 64),
        (
            "123456789012345678901234567890n",
            123_456_789_012_345_678_901_234_567_890,
        ),
    ];
    let negatives: [(&str, i128); 3] = [
        ("-1", -1),
        ("-5n", -5),
        (
            "-123456789012345678901234567890n",
            -123_456_789_012_345_678_901_234_567_890,
        ),
    ];
    for (theory, values) in [
        ("Nat", naturals.to_vec()),
        ("Int", naturals.iter().chain(&negatives).copied().collect()),
    ] {
        let receive = r#"for (@x <= a) { out!(["took", x]) }"#;
        // The handle variant runs three calls in each deploy.
        for (via, width) in [("routed", values.len()), ("handle", 3)] {
            for chunk in values.chunks(width) {
                let body = match via {
                    "routed" => {
                        let sends: Vec<String> = chunk
                            .iter()
                            .map(|(source, _)| format!("a!({source})"))
                            .collect();
                        format!("{} | {receive}", sends.join(" | "))
                    }
                    _ => {
                        let calls: Vec<String> = chunk
                            .iter()
                            .map(|(source, _)| format!(r#"space!?("produce", *a, [{source}])"#))
                            .collect();
                        format!("{}; {receive}", calls.join("; "))
                    }
                };
                let label = format!("{theory} {via} {:?}", chunk.first());
                let played = play_and_certify(&label, &theory_program("bag", theory, &body)).await;
                assert!(
                    played.result.errors.is_empty(),
                    "{label}: {:?}",
                    played.result.errors
                );
                let mut took: Vec<i128> = outputs(&played, "took")
                    .iter()
                    .map(|value| match value.as_slice() {
                        [x] => integer_of(x).expect("an integer"),
                        other => panic!("{label}: one value: {other:?}"),
                    })
                    .collect();
                took.sort();
                let mut expected: Vec<i128> = chunk.iter().map(|(_, value)| *value).collect();
                expected.sort();
                assert_eq!(took, expected, "{label}");
            }
        }
    }
    for (source, _) in negatives {
        let played = play_and_certify(
            &format!("Nat {source}"),
            &theory_program("bag", "Nat", &format!("a!({source})")),
        )
        .await;
        assert_eq!(codes(&played), only("TheoryRefuted"), "Nat {source}");
    }
}

/// TH-1 and TH-5: `Nat` refuses every value that is not a natural number: a negative number,
/// the string `"Nat(42)"` (which the legacy prefix check admitted), a par of two values, `Nil`, a
/// Boolean, a list, a name and a float. A routed send, a persistent send and the handle
/// `produce` each abort with `TheoryRefuted`, which names the index of the first refused value,
/// and the space stores nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn theory_nat_rejects_minus1_string_par() {
    for (label, value) in [
        ("minus one", "-1"),
        ("least", "-9223372036854775808"),
        ("negative big integer", "-1n"),
        ("string", r#""Nat(42)""#),
        ("par", "1 | 2"),
        ("nil", "Nil"),
        ("boolean", "true"),
        ("list", "[1]"),
        ("name", "*a"),
        ("float", "1.5"),
    ] {
        for (via, body) in [
            ("routed", format!(r#"a!({value}) | out!(["sent", 1])"#)),
            ("persistent", format!(r#"a!!({value}) | out!(["sent", 1])"#)),
            (
                "handle",
                format!(r#"space!?("produce", *a, [{value}]); out!(["sent", 1])"#),
            ),
        ] {
            let played = play_and_certify(
                &format!("{label} {via}"),
                &theory_program("bag", "Nat", &body),
            )
            .await;
            assert_eq!(codes(&played), only("TheoryRefuted"), "{label} {via}");
            assert!(
                messages(&played).contains("the theory Nat refutes the value 0 of the send"),
                "{label} {via}: {}",
                messages(&played)
            );
        }
    }
    for (values, index) in [("1, -1", 1), ("0, 1, \"2\"", 2), ("-1, 1", 0)] {
        let played = play_and_certify(
            &format!("several {values}"),
            &theory_program("bag", "Nat", &format!("a!({values})")),
        )
        .await;
        assert_eq!(codes(&played), only("TheoryRefuted"), "{values}");
        assert!(
            messages(&played).contains(&format!(
                "the theory Nat refutes the value {index} of the send"
            )),
            "{values}: {}",
            messages(&played)
        );
    }
    // The control: the same sends of natural numbers are admitted.
    let played = play_and_certify(
        "admitted",
        &theory_program(
            "bag",
            "Nat",
            r#"a!(1) | a!!(2) | space!?("produce", *a, [3]); a!(4, 5) |
for (@x <- a) { out!(["took", x]) }"#,
        ),
    )
    .await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert_eq!(labels(&played), vec!["took"]);
}

// ─── TH-2 and TH-4: the identity and the gate ─────────────────────────────────────────────────

/// TH-2 and decision 74: the `config` reply shows the canonical identity map of the theory, and
/// its decoder gives the identity of the builtin. The identity holds content only: two spaces
/// with one theory show equal identities and different space ids, and the identity holds no
/// name. A space without a theory shows `Nil`, and its reply decodes as its stored config.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn theory_identity_persisted_no_handle() {
    let program = format!(
        r#"new factory(`rho:space:hashmap:bag`), out in {{
  for (first <- factory!?("default", free Nat()); second <- factory!?("default", free Nat()); plain <- factory!?()) {{
    for (@one <- first!?("config"); @two <- second!?("config"); @none <- plain!?("config")) {{
      out!(["configs", one, two, none])
    }}
  }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#
    );
    let played = play_and_certify("identity", &program).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    let configs = outputs(&played, "configs");
    let [one, two, none] = configs.first().expect("the configs").as_slice() else {
        panic!("three configs: {configs:?}");
    };
    let nat = TheoryIdentity::builtin("Nat");
    assert_eq!(entry(one, "theory").as_ref(), Some(&nat.to_par()));
    assert_eq!(entry(two, "theory").as_ref(), Some(&nat.to_par()));
    assert_ne!(entry(one, "spaceId"), entry(two, "spaceId"), "two spaces");
    let none = SpaceConfig::from_par(none).expect("the reply of a space without a theory");
    assert_eq!(none.theory, None);
    assert_eq!(entry(&none.to_par(), "theory"), Some(Par::default()));
    let slot = nat.to_par();
    assert!(!holds_name(&slot), "the identity holds no name: {slot:?}");
    assert_eq!(TheoryIdentity::from_par(&slot), Some(nat.clone()));
    assert_eq!(nat.theorem_id, "Nat");
    assert_eq!(nat.checker_abi, "rrs:structural:v1");
    assert_eq!(nat.language_fingerprint, "rrs:builtin");
    assert_eq!(nat.limit_profile, STRUCTURAL_LIMITS);
    assert!(nat.args.is_empty());
}

/// TH-4: a theory name that is not a builtin fails with `UnknownTheory`, which names the nearest
/// builtin within two edits, or no builtin.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn theory_unknown_name_suggestion() {
    for (name, expected) in [
        ("Natt", r#""Natt" is not a theory, did you mean "Nat"?"#),
        (
            "Strng",
            r#""Strng" is not a theory, did you mean "String"?"#,
        ),
        ("Boll", r#""Boll" is not a theory, did you mean "Bool"?"#),
        ("Bounded", r#""Bounded" is not a theory""#),
    ] {
        let played = play_and_certify(
            name,
            &theory_program("bag", name, r#"out!(["created", 1])"#),
        )
        .await;
        assert_eq!(codes(&played), only("UnknownTheory"), "{name}");
        let text = messages(&played).replace("\\\"", "\"");
        assert!(text.contains(expected), "{name}: {text}");
        assert!(
            name != "Bounded" || !text.contains("did you mean"),
            "{name}: no builtin is within two edits: {text}"
        );
        assert!(labels(&played).is_empty(), "{name}: no space");
    }
}

/// TH-4: a builtin theory takes no arguments, so `free Nat(1)` fails with
/// `BadConstructorArgument`. An unknown theory with arguments fails as an unknown theory.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn theory_builtin_args_rejected() {
    for (call, code, detail) in [
        (
            "free Nat(1)",
            "BadConstructorArgument",
            "the builtin theory Nat takes no arguments",
        ),
        (
            "free Any(1, 2)",
            "BadConstructorArgument",
            "the builtin theory Any takes no arguments",
        ),
        (
            "free Bool(true)",
            "BadConstructorArgument",
            "the builtin theory Bool takes no arguments",
        ),
        (
            "free Bounded(1, 2)",
            "UnknownTheory",
            r#""Bounded" is not a theory"#,
        ),
    ] {
        let program = space_program(
            "rho:space:hashmap:bag",
            &format!("\"default\", {call}"),
            r#"out!(["created", 1])"#,
        );
        let played = play_and_certify(call, &program).await;
        assert_eq!(codes(&played), only(code), "{call}");
        let text = messages(&played).replace("\\\"", "\"");
        assert!(text.contains(detail), "{call}: {text}");
    }
}

// ─── TH-6: the guard and the theory at a match ────────────────────────────────────────────────

/// TH-6: on a space with `free Nat()`, a receive whose guard admits a stored datum fires, after
/// the theory checked the datum, on each collection of the hashmap store, on a join and on a
/// prefix receive, whose theory checks the own payload of the datum and not its re-rooted
/// value. The control of each case is the same receive with a guard that refuses: it parks.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_guard_match() {
    let guarded = |admits: bool, receive: &str| {
        receive
            .replace("{x}", if admits { "1" } else { "2" })
            .replace("{sum}", if admits { "11" } else { "12" })
            .replace("{suffix}", if admits { "[1, 5]" } else { "[1, 6]" })
    };
    let mut cases: Vec<(String, &str, &str)> = Inner::ALL
        .iter()
        .map(|inner| {
            (
                format!("rho:space:hashmap:{}", inner.name()),
                r#"a!(1) | for (@x <- a where x == {x}) { out!(["took", x]) }"#,
                "took",
            )
        })
        .collect();
    cases.push((
        "rho:space:hashmap:bag".to_string(),
        r#"c!(5) | d!(6) | for (@p <- c & @q <- d where p + q == {sum}) { out!(["join", p, q]) }"#,
        "join",
    ));
    cases.push((
        "rho:space:pathmap:bag".to_string(),
        r#"use space { @[*e, 1]!(5) } | for (@v <- e where v == {suffix}) { out!(["prefix", v]) }"#,
        "prefix",
    ));
    for (urn, receive, label) in cases {
        for admits in [true, false] {
            let program = space_program(&urn, "\"default\", free Nat()", &guarded(admits, receive));
            let played = play_and_certify(&format!("{urn} {label} {admits}"), &program).await;
            assert!(
                played.result.errors.is_empty(),
                "{urn} {label}: {:?}",
                played.result.errors
            );
            let expected: Vec<String> = match admits {
                true => vec![label.to_string()],
                false => Vec::new(),
            };
            assert_eq!(labels(&played), expected, "{urn} {label} admits={admits}");
        }
    }
}

// ─── The invariant map: one property per headline of TheoryAdmission.v ───────────────────────

/// The structural predicate of TH-1, restated as the oracle of the properties: the kind of the
/// sole expression of `value`, without the code of the checker.
fn in_builtin(name: &str, value: &Par) -> bool {
    let sole = value.sends.is_empty()
        && value.receives.is_empty()
        && value.news.is_empty()
        && value.matches.is_empty()
        && value.unforgeables.is_empty()
        && value.bundles.is_empty()
        && value.connectives.is_empty()
        && value.conditionals.is_empty()
        && value.locally_free.is_empty()
        && value.cost_signed_terms.is_empty()
        && value.cost_stacks.is_empty()
        && !value.connective_used;
    let expr = match (sole, value.exprs.as_slice()) {
        (
            true,
            [Expr {
                expr_instance: Some(expr),
            }],
        ) => Some(expr),
        _ => None,
    };
    match (name, expr) {
        ("Any", _) => true,
        ("Nat", Some(ExprInstance::GInt(n))) => *n >= 0,
        ("Nat", Some(ExprInstance::GBigInt(bytes))) => bytes.first().is_none_or(|b| b & 0x80 == 0),
        ("Int", Some(ExprInstance::GInt(_) | ExprInstance::GBigInt(_))) => true,
        ("String", Some(ExprInstance::GString(_))) => true,
        ("Bool", Some(ExprInstance::GBool(_))) => true,
        _ => false,
    }
}

/// Values for the properties: the generated pars of P2 and big integers of both signs.
fn arb_value() -> impl Strategy<Value = Par> {
    prop_oneof![
        4 => reified_strategies::arb_par(),
        1 => proptest::collection::vec(any::<u8>(), 0..10).prop_map(|bytes| {
            reified_strategies::expr_par(ExprInstance::GBigInt(bytes), Vec::new(), false)
        }),
    ]
}

/// `builtin_decidable_structural`: with the structural limits, each builtin gives `Proven` or
/// `Refuted` on every value, exactly as the structural predicate says, with the usage `{1, 0}`.
/// A match checks every value of the stored payload.
#[test]
fn prop_theory_builtin_decidable() {
    check(
        pure_config(),
        (
            select(BUILTIN_NAMES.to_vec()),
            proptest::collection::vec(arb_value(), 1..4),
        ),
        |(name, values)| {
            let checker = theory::resolve(&TheoryIdentity::builtin(name))
                .map_err(|error| TestCaseError::fail(format!("{name}: {error:?}")))?;
            for value in &values {
                let verdict = checker
                    .admit(value)
                    .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
                proptest::prop_assert!(
                    !matches!(verdict, TheoryVerdict::Undetermined { .. }),
                    "{name} decides {value:?}"
                );
                proptest::prop_assert_eq!(verdict.is_proven(), in_builtin(name, value));
                proptest::prop_assert_eq!(verdict.usage(), TheoryUsage::new(1, 0));
            }
            let stored = ListParWithRandom {
                pars: values.clone(),
                ..ListParWithRandom::default()
            };
            let verdict = checker
                .check_match(&stored, &ListParWithRandom::default())
                .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
            proptest::prop_assert_eq!(
                verdict.is_proven(),
                values.iter().all(|value| in_builtin(name, value))
            );
            Ok(())
        },
    );
}

/// `fail_closed`: the admission of the values of a send succeeds only when every value is
/// `Proven`. Otherwise it fails at the first value that is not: `TheoryRefuted` for a value
/// outside the theory, and `TheoryUndetermined` for every value under a starved limit profile.
/// An identity that names no checker of this build resolves to `TheoryUnavailable`.
#[test]
fn prop_theory_fail_closed() {
    check(
        pure_config(),
        (
            select(BUILTIN_NAMES.to_vec()),
            any::<bool>(),
            proptest::collection::vec(arb_value(), 0..5),
            prop_oneof![
                Just(None),
                Just(Some("Real")),
                Just(Some("rrs:structural:v2"))
            ],
        ),
        |(name, starved, values, unknown)| {
            let identity = TheoryIdentity {
                limit_profile: match starved {
                    true => TheoryUsage::new(0, 0),
                    false => STRUCTURAL_LIMITS,
                },
                ..TheoryIdentity::builtin(name)
            };
            let checker = theory::resolve(&identity)
                .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
            let result = admit_values(checker.as_ref(), &values).map(|leases| leases.len());
            let first = values.iter().position(|value| !in_builtin(name, value));
            let expected = match (starved, values.is_empty(), first) {
                (true, false, _) => Err(SpaceError::TheoryUndetermined {
                    theory: name.to_string(),
                    index: 0,
                }),
                (false, _, Some(index)) => Err(SpaceError::TheoryRefuted {
                    theory: name.to_string(),
                    index,
                }),
                (true, true, _) | (false, _, None) => Ok(0),
            };
            proptest::prop_assert_eq!(result, expected);
            if let Some(field) = unknown {
                let other = match field {
                    "Real" => TheoryIdentity::builtin("Real"),
                    abi => TheoryIdentity {
                        checker_abi: abi.to_string(),
                        ..identity.clone()
                    },
                };
                proptest::prop_assert_eq!(
                    theory::resolve(&other).map(|_| ()),
                    Err(SpaceError::TheoryUnavailable(other.theorem_id.clone()))
                );
            }
            Ok(())
        },
    );
}

/// `usage_cache_transparent`: the verdict and the usage of each check depend only on the identity
/// and the value. A check repeated after other checks, and a check by a checker of a fresh
/// resolution, give the same verdict and usage as the first check.
#[test]
fn prop_theory_usage_cache_transparent() {
    check(
        pure_config(),
        (
            select(BUILTIN_NAMES.to_vec()),
            any::<bool>(),
            proptest::collection::vec(arb_value(), 1..5),
        ),
        |(name, starved, values)| {
            let identity = TheoryIdentity {
                limit_profile: match starved {
                    true => TheoryUsage::new(0, 0),
                    false => STRUCTURAL_LIMITS,
                },
                ..TheoryIdentity::builtin(name)
            };
            let shared = theory::resolve(&identity)
                .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
            let check_one = |checker: &dyn SpaceTheory, value: &Par| checker.admit(value);
            let first: Vec<_> = values
                .iter()
                .map(|value| check_one(shared.as_ref(), value))
                .collect();
            let again: Vec<_> = values
                .iter()
                .rev()
                .map(|value| check_one(shared.as_ref(), value))
                .collect();
            let fresh: Vec<_> = values
                .iter()
                .map(|value| {
                    let checker = theory::resolve(&identity).expect("the identity resolves");
                    check_one(checker.as_ref(), value)
                })
                .collect();
            let reversed: Vec<_> = again.into_iter().rev().collect();
            proptest::prop_assert_eq!(&first, &reversed);
            proptest::prop_assert_eq!(&first, &fresh);
            for verdict in &first {
                let usage = verdict
                    .as_ref()
                    .map_err(|error| TestCaseError::fail(format!("{error:?}")))?
                    .usage();
                proptest::prop_assert_eq!(usage, TheoryUsage::new(1, 0));
            }
            Ok(())
        },
    );
}

/// `resolve_identity_deterministic`: the resolution is a pure function of the identity. It
/// resolves an identity exactly when the identity is a structural builtin with any limit
/// profile, the resolved checker carries the identity, and the identity survives its canonical
/// map.
#[test]
fn prop_resolve_identity_deterministic() {
    check(
        pure_config(),
        (
            select(vec![
                "Any", "Bool", "Int", "Nat", "String", "Real", "nat", "",
            ]),
            0u8..8,
            (0u64..3, 0u64..3),
        ),
        |(name, edit, (work_units, evidence_bytes))| {
            let base = TheoryIdentity {
                limit_profile: TheoryUsage::new(work_units, evidence_bytes),
                ..TheoryIdentity::builtin(name)
            };
            let identity = match edit {
                0 => TheoryIdentity { version: 2, ..base },
                1 => TheoryIdentity {
                    language_fingerprint: "mettail:rholang".to_string(),
                    ..base
                },
                2 => TheoryIdentity {
                    category: "Name".to_string(),
                    ..base
                },
                3 => TheoryIdentity {
                    checker_abi: "rrs:structural:v2".to_string(),
                    ..base
                },
                4 => TheoryIdentity {
                    args: vec![reified_strategies::gint(1)],
                    ..base
                },
                _ => base,
            };
            let structural = edit >= 5 && BUILTIN_NAMES.contains(&name);
            let first = theory::resolve(&identity).map(|checker| checker.identity().clone());
            let second = theory::resolve(&identity).map(|checker| checker.identity().clone());
            proptest::prop_assert_eq!(&first, &second);
            match structural {
                true => proptest::prop_assert_eq!(first, Ok(identity.clone())),
                false => proptest::prop_assert_eq!(
                    first,
                    Err(SpaceError::TheoryUnavailable(identity.theorem_id.clone()))
                ),
            }
            let decoded = TheoryIdentity::from_par(&identity.to_par());
            proptest::prop_assert_eq!(decoded.as_ref(), Some(&identity));
            Ok(())
        },
    );
}

/// A value of a generated send, as source text and as the oracle sees it.
#[derive(Clone, Debug)]
enum SentValue {
    Int(i64),
    Big(i128),
    Text(&'static str),
    Flag(bool),
    Nil,
    List,
}

impl SentValue {
    fn source(&self) -> String {
        match self {
            SentValue::Int(value) => value.to_string(),
            SentValue::Big(value) => format!("{value}n"),
            SentValue::Text(text) => format!("{text:?}"),
            SentValue::Flag(flag) => flag.to_string(),
            SentValue::Nil => "Nil".to_string(),
            SentValue::List => "[1, 2]".to_string(),
        }
    }

    fn in_theory(&self, name: &str) -> bool {
        match (name, self) {
            ("Any", _) => true,
            ("Nat", SentValue::Int(value)) => *value >= 0,
            ("Nat", SentValue::Big(value)) => *value >= 0,
            ("Int", SentValue::Int(_) | SentValue::Big(_)) => true,
            ("String", SentValue::Text(_)) => true,
            ("Bool", SentValue::Flag(_)) => true,
            _ => false,
        }
    }
}

fn arb_sent_value() -> impl Strategy<Value = SentValue> {
    prop_oneof![
        3 => (-3i64..1000).prop_map(SentValue::Int),
        2 => prop_oneof![
            Just(SentValue::Big(1 << 70)),
            Just(SentValue::Big(-(1 << 70))),
            Just(SentValue::Big(0)),
        ],
        1 => select(vec!["", "Nat(42)", "x"]).prop_map(SentValue::Text),
        1 => any::<bool>().prop_map(SentValue::Flag),
        1 => Just(SentValue::Nil),
        1 => Just(SentValue::List),
    ]
}

/// `replay_preserves_theory_admission`: for a generated theory and a generated send, native
/// play admits the send exactly when every value is in the theory. A refused send aborts with
/// the index of its first value outside the theory, and an admitted send is received whole.
/// Native replay reproduces the errors, the phlo and the root of play.
#[test]
fn prop_replay_preserves_theory_admission() {
    check(
        runtime_config(),
        (
            select(BUILTIN_NAMES.to_vec()),
            proptest::collection::vec(arb_sent_value(), 1..4),
            select(vec!["routed", "handle"]),
        ),
        |(name, values, via)| tokio_runtime().block_on(replay_preserves_case(name, values, via)),
    );
}

/// One case of `prop_replay_preserves_theory_admission`.
async fn replay_preserves_case(
    name: &str,
    values: Vec<SentValue>,
    via: &str,
) -> Result<(), TestCaseError> {
    let sources: Vec<String> = values.iter().map(SentValue::source).collect();
    let binders: Vec<String> = (0..values.len())
        .map(|index| format!("@x{index}"))
        .collect();
    let names: Vec<String> = (0..values.len()).map(|index| format!("x{index}")).collect();
    let receive = format!(
        "for ({} <- a) {{ out!([\"took\", {}]) }}",
        binders.join(", "),
        names.join(", ")
    );
    let body = match via {
        "routed" => format!("a!({}) | {receive}", sources.join(", ")),
        _ => format!(
            r#"space!?("produce", *a, [{}]); {receive}"#,
            sources.join(", ")
        ),
    };
    let term = theory_program("bag", name, &body);
    let played = native_play(&term, THEORY_LIMIT).await;
    let certification = certify(&played, &term, THEORY_LIMIT).await;
    assert_reproduces(
        &format!("{name} {via} {sources:?}"),
        &played,
        &certification,
    );
    match values.iter().position(|value| !value.in_theory(name)) {
        Some(index) => {
            proptest::prop_assert_eq!(codes(&played), only("TheoryRefuted"));
            let expected = format!("the theory {name} refutes the value {index} of the send");
            proptest::prop_assert!(
                messages(&played).contains(&expected),
                "{}",
                messages(&played)
            );
            proptest::prop_assert!(labels(&played).is_empty());
        }
        None => {
            proptest::prop_assert!(
                played.result.errors.is_empty(),
                "{:?}",
                played.result.errors
            );
            proptest::prop_assert_eq!(outputs(&played, "took").len(), 1);
        }
    }
    Ok(())
}

/// An identity with every field generated: any version, Unicode names, any limits and up to
/// two args.
fn arb_identity() -> impl Strategy<Value = TheoryIdentity> {
    (
        any::<i64>(),
        proptest::collection::vec("\\PC{0,12}", 5),
        any::<u64>(),
        any::<u64>(),
        proptest::collection::vec(reified_strategies::arb_ground_par(), 0..3),
    )
        .prop_map(|(version, names, work, evidence, args)| TheoryIdentity {
            version,
            language_fingerprint: names[0].clone(),
            category: names[1].clone(),
            category_name: names[2].clone(),
            theorem_id: names[3].clone(),
            checker_abi: names[4].clone(),
            limit_profile: TheoryUsage::new(work, evidence),
            args,
        })
}

/// `identity_bytes_roundtrip` (TA6, decision 74): the bytes of every identity decode to it, an
/// edited, cut or extended input either fails or decodes to an identity whose bytes are that
/// input, and the map form of the `config` reply names the same identity.
#[test]
fn prop_theory_identity_bytes_roundtrip() {
    check(
        pure_config(),
        (
            arb_identity(),
            proptest::collection::vec((any::<u8>(), any::<proptest::sample::Index>()), 0..3),
            0u8..4,
        ),
        |(identity, edits, cut)| {
            let bytes = identity.to_bytes();
            proptest::prop_assert_eq!(TheoryIdentity::from_bytes(&bytes), Some(identity.clone()));
            let mut edited = bytes.clone();
            for (byte, index) in edits {
                let at = index.index(edited.len());
                edited[at] ^= byte;
            }
            match cut {
                0 => {}
                1 => {
                    edited.pop();
                }
                2 => edited.push(0),
                _ => edited.truncate(edited.len() / 2),
            }
            if let Some(decoded) = TheoryIdentity::from_bytes(&edited) {
                proptest::prop_assert_eq!(decoded.to_bytes(), edited);
            }
            proptest::prop_assert_eq!(TheoryIdentity::from_par(&identity.to_par()), Some(identity));
            Ok(())
        },
    );
}

/// The Levenshtein distance of `a` and `b` over chars: the oracle of the suggestion.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = Vec::with_capacity(b.len() + 1);
    for (row, x) in a.iter().enumerate() {
        current.clear();
        current.push(row + 1);
        for (column, y) in b.iter().enumerate() {
            let substitution = previous[column] + usize::from(x != y);
            current.push(
                substitution
                    .min(previous[column + 1] + 1)
                    .min(current[column] + 1),
            );
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// The suggestion of gate 4, restated: the builtin at the least distance within two edits, the
/// bytewise least on a tie.
fn suggested_builtin(name: &str) -> Option<String> {
    BUILTIN_NAMES
        .iter()
        .map(|builtin| (levenshtein(name, builtin), *builtin))
        .filter(|(distance, _)| *distance <= 2)
        .min()
        .map(|(_, builtin)| builtin.to_string())
}

/// Theory names near the builtins: the builtins, one edit of a builtin, and other names.
fn arb_theory_name() -> impl Strategy<Value = String> {
    let edited = (
        select(BUILTIN_NAMES.to_vec()),
        any::<proptest::sample::Index>(),
        proptest::char::range('a', 'z'),
        0u8..3,
    )
        .prop_map(|(builtin, index, letter, edit)| {
            let mut chars: Vec<char> = builtin.chars().collect();
            let at = index.index(chars.len());
            match edit {
                0 => {
                    chars.remove(at);
                }
                1 => chars.insert(at, letter),
                _ => chars[at] = letter,
            }
            chars.into_iter().collect()
        });
    prop_oneof![
        2 => select(BUILTIN_NAMES.to_vec()).prop_map(String::from),
        2 => edited,
        1 => "[A-Z][a-z]{0,6}",
    ]
}

/// `theory_gate_validated` (SpaceFactory.v headline 15, TH-4): gate 4 gives the identity of a
/// builtin without arguments, `BadConstructorArgument` for a builtin with arguments, and
/// `UnknownTheory` with the least builtin within two edits for every other name. It runs before
/// the params gates, so a refused theory decides also when an unknown param follows it.
#[test]
fn prop_theory_gate() {
    use models::rust::reified::encoding::{encode_free, encode_params, FreeTheory, SendParams};
    use models::rust::space_factory_urn::SpaceFactoryUrn;
    use rholang::rust::interpreter::spaces::factory::constructor::FactoryConstructor;

    check(
        pure_config(),
        (arb_theory_name(), 0usize..3),
        |(name, arity)| {
            let args: Vec<Par> = (0..arity)
                .map(|value| reified_strategies::gint(value as i64))
                .collect();
            let free = encode_free(&FreeTheory {
                name: name.clone(),
                args: args.clone(),
            });
            proptest::prop_assume!(free.is_ok());
            let free = free.expect("the name is a theory name");
            let expected = match (BUILTIN_NAMES.contains(&name.as_str()), arity) {
                (true, 0) => Ok(Some(TheoryIdentity::builtin(&name))),
                (true, _) => Err(SpaceError::BadConstructorArgument(format!(
                    "the builtin theory {name} takes no arguments"
                ))),
                (false, _) => Err(SpaceError::UnknownTheory {
                    name: name.clone(),
                    suggestion: suggested_builtin(&name),
                }),
            };
            let validated = FactoryConstructor::validate(
                SpaceFactoryUrn::HASHMAP_QUEUE,
                &reified_strategies::list(vec![free.clone()]),
            )
            .map(|ctor| ctor.theory);
            proptest::prop_assert_eq!(&validated, &expected);
            let params = encode_params(&SendParams {
                positional: Vec::new(),
                named: [("zz".to_string(), reified_strategies::gint(1))].into(),
            })
            .expect("params encode")
            .expect("params are not empty");
            let with_params = FactoryConstructor::validate(
                SpaceFactoryUrn::HASHMAP_QUEUE,
                &reified_strategies::list(vec![free, params]),
            )
            .map(|ctor| ctor.theory);
            match expected {
                Ok(_) => {
                    let refused_by_params =
                        matches!(with_params, Err(SpaceError::UnknownParam { .. }));
                    proptest::prop_assert!(refused_by_params, "{:?}", with_params);
                }
                Err(error) => proptest::prop_assert_eq!(with_params, Err(error)),
            }
            Ok(())
        },
    );
}
