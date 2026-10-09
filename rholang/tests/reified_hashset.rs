//! The HashSet store of P7 (design v3, section 6: HS-1 to HS-8, decision 73).
//!
//! A HashSet space is the default space restricted to empty messages: `x!()` adds a unit, and
//! `for ( <- x)` takes one unit or waits. Every HashSet space has the qualifier `seq`, so each
//! program below makes its uses of a channel in one causal chain: sync handle calls, then
//! receives that find their units at once (SQ-6 (c)).
//!
//! Every deploy plays natively at the v6 limits and certifies through its committed evidence,
//! as proposer self-certification and validator replay do (`support::native`). After each play,
//! the hot store holds no volatile channel (the residue check of VR-2).
//!
//! | Test | Property |
//! |---|---|
//! | `reified_hashset_unit_sends` | HS-1 to HS-5 on the six families: a unit sent through the handle or routed, persistent or not, is taken by an empty receive, a peek leaves it, and a persistent receive takes it. |
//! | `reified_hashset_payload_not_empty` | HS-2: a routed send, a persistent send or a handle `produce` with a payload aborts with `PayloadNotEmpty` on every family. `x!(Nil)` has a payload. |
//! | `reified_hashset_pattern_not_empty` | HS-3: a binder, a wildcard, a literal, a remainder, a persistent or peeking bind with a binder, and a join with one binder abort with `PatternNotEmpty` on every family. |
//! | `reified_hashset_guards` | HS-3: the `where` guard of an empty bind names only outer variables. A guard that holds takes the unit, and a guard that refuses parks the receive. |
//! | `reified_hashset_joins` | HS-4: a join of two empty binds on two collections of one HashSet space fires on every family. |
//! | `reified_hashset_disciplines` | HS-5: the set keeps one unit class (two adds, one take), the bag and the ordered collections count their units, and the cell holds one (`CellOccupied`). |
//! | `reified_hashset_head_order` | HS-5: the queue takes its oldest unit, the stack its newest, and the priority queue its lowest level first or the level that a receive names, as the COMM of the receive shows. |
//! | `reified_hashset_seq_only` | HS-6: a HashSet factory refuses `default` and `temp` with `BadConstructorArgument`. |
//! | `reified_hashset_factory_params` | HS-7: a HashSet factory takes the params of its discipline only: an unknown param fails with `UnknownParam` and a suggestion, `priorities` below 1 with `InvalidParam`, and a valid bound is stored in the config. |
//! | `reified_hashset_absent_unit_waits` | SQ-6 (d): a receive of an absent unit waits for the rest of the deploy, so "None" is never observable, and the sweep removes the waiter. |
//! | `prop_hashset_unit_rules` | `hashset_unit_rules` (`HashMapStore.v`): for generated families, adds and last operations, a payload aborts with `PayloadNotEmpty`, a pattern with `PatternNotEmpty`, a second unit in a cell with `CellOccupied`, and an empty receive takes a unit exactly when one was added. |

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeSet;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::utils::new_gstring_par;
use proptest::prelude::{prop_oneof, Just, Strategy};
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::spaces::namespace::{Durability, NativeSpace};
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::trace::event::Event;
use support::native::{assert_reproduces, certify, native_play, Played, AMPLE_LIMIT, OUT};
use support::props::{check, runtime_config, tokio_runtime};
use support::session::{data_channel, data_produces, error_codes};

// ─── Helpers ───────────────────────────────────────────────────────────────────────────────────

/// The phlo limit of the deploys of this suite, as in the Seq suite.
const HASHSET_LIMIT: u64 = 20 * AMPLE_LIMIT;

/// The inner collections of the six HashSet families, in family-index order.
const INNERS: [&str; 6] = ["bag", "queue", "stack", "set", "cell", "priorityqueue"];

/// A deploy that creates a `seq` space `S` of the family `hashset:inner`, draws the names `x`
/// and `y` of `S` with `gensym`, and runs `body`. The values on `out` go to the public channel
/// [`OUT`].
fn hashset_program(inner: &str, body: &str) -> String {
    format!(
        r#"new factory(`rho:space:hashset:{inner}`), out in {{
  for (S <- factory!?("seq")) {{
    for (x <- S!?("gensym"); y <- S!?("gensym")) {{
      {body}
    }}
  }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#
    )
}

/// Plays `term` natively at the v6 limits, checks that the hot store holds no volatile channel
/// after it, and certifies it.
async fn play_and_certify(label: &str, term: &str) -> Played {
    let played = native_play(term, HASHSET_LIMIT).await;
    assert!(
        played.result.native_budget_recording.is_some(),
        "{label}: the deploy evaluated: {:?}",
        played.result.errors
    );
    assert!(
        played.residue.is_empty(),
        "{label}: the sweep leaves no volatile channel: {:?}",
        played.residue
    );
    let certification = certify(&played, term, HASHSET_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    played
}

/// The distinct error codes of `played`.
fn codes(played: &Played) -> BTreeSet<String> {
    error_codes(&played.result.errors).into_iter().collect()
}

/// The single code `code`.
fn only(code: &str) -> BTreeSet<String> { BTreeSet::from([code.to_string()]) }

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

/// The label of an output `[label, …]`, when it has one.
fn label_of(value: &Par) -> Option<String> {
    match elements(value)?.first().and_then(sole_expr) {
        Some(ExprInstance::GString(label)) => Some(label.clone()),
        _ => None,
    }
}

/// The labels of the outputs of `played`, in bytewise order.
fn labels(played: &Played) -> Vec<String> {
    let mut labels: Vec<String> = played.out.iter().flatten().filter_map(label_of).collect();
    labels.sort();
    labels
}

/// The labels `expected`, in bytewise order.
fn expect(expected: &[&str]) -> Vec<String> {
    let mut labels: Vec<String> = expected.iter().map(|label| label.to_string()).collect();
    labels.sort();
    labels
}

/// The `seq` space whose config the program sent as `["config", c]`.
fn space_of(played: &Played) -> NativeSpace {
    let config = played
        .out
        .iter()
        .flatten()
        .find_map(|value| match elements(value)? {
            [label, config] if label_of_string(label) == Some("config") => Some(config.clone()),
            _ => None,
        })
        .expect("the program sends its config");
    let config = SpaceConfig::from_par(&config).expect("a config record");
    NativeSpace {
        sid: config.space_id,
        durability: Durability::Ephemeral,
    }
}

/// The string of `par`, when it is one string.
fn label_of_string(par: &Par) -> Option<&str> {
    match sole_expr(par) {
        Some(ExprInstance::GString(value)) => Some(value.as_str()),
        _ => None,
    }
}

/// The produce hashes of the unit records on the collection of `channel` in `space`, in log
/// order, and the produce hashes of the COMMs whose consume is on that collection alone.
fn units_and_takes(
    played: &Played,
    space: NativeSpace,
    channel: &Par,
) -> (Vec<Blake2b256Hash>, Vec<Blake2b256Hash>) {
    let data = data_channel(space, channel);
    let units = data_produces(&played.log, &data)
        .into_iter()
        .map(|produce| produce.hash)
        .collect();
    let key = hash(&data);
    let takes = played
        .log
        .iter()
        .filter_map(|event| match event {
            Event::Comm(comm) if comm.consume.channel_hashes == [key.clone()] => Some(
                comm.produces
                    .iter()
                    .map(|produce| produce.hash.clone())
                    .collect::<Vec<_>>(),
            ),
            Event::Comm(_) | Event::IoEvent(_) => None,
        })
        .flatten()
        .collect();
    (units, takes)
}

// ─── HS-1 to HS-5: units ─────────────────────────────────────────────────────────────────────────

/// HS-1 to HS-5: on every family, a unit that the handle adds is taken by an empty receive, a
/// routed unit and a persistent unit are stored, a peek leaves the unit for a take, and a
/// persistent receive takes a unit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hashset_unit_sends() {
    for inner in INNERS {
        let played = play_and_certify(
            &format!("take {inner}"),
            &hashset_program(
                inner,
                r#"S!?("produce", *x, []); for ( <- x) { out!(["took", 1]) }"#,
            ),
        )
        .await;
        assert!(
            played.result.errors.is_empty(),
            "{inner}: {:?}",
            played.result.errors
        );
        assert_eq!(labels(&played), expect(&["took"]), "{inner}");
        for (label, body) in [("routed", "x!()"), ("persistent", "x!!()")] {
            let played = play_and_certify(
                &format!("{label} {inner}"),
                &hashset_program(inner, &format!(r#"{body} | out!(["sent", 1])"#)),
            )
            .await;
            assert!(
                played.result.errors.is_empty(),
                "{label} {inner}: {:?}",
                played.result.errors
            );
            assert_eq!(labels(&played), expect(&["sent"]), "{label} {inner}");
        }
        let played = play_and_certify(
            &format!("peek {inner}"),
            &hashset_program(
                inner,
                r#"S!?("produce", *x, []);
for ( <<- x) { out!(["peeked", 1]) | for ( <- x) { out!(["took", 1]) } }"#,
            ),
        )
        .await;
        assert!(
            played.result.errors.is_empty(),
            "{inner}: {:?}",
            played.result.errors
        );
        assert_eq!(labels(&played), expect(&["peeked", "took"]), "{inner}");
        let played = play_and_certify(
            &format!("persistent receive {inner}"),
            &hashset_program(
                inner,
                r#"S!?("produce", *x, []); for ( <= x) { out!(["took", 1]) }"#,
            ),
        )
        .await;
        assert!(
            played.result.errors.is_empty(),
            "{inner}: {:?}",
            played.result.errors
        );
        assert_eq!(labels(&played), expect(&["took"]), "{inner}");
    }
}

/// HS-2: a send with a payload aborts with `PayloadNotEmpty` before any staging, on every
/// family. `x!(Nil)` carries one value, so it is not a unit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hashset_payload_not_empty() {
    for inner in INNERS {
        for (label, body) in [
            ("routed", "x!(1)"),
            ("routed Nil", "x!(Nil)"),
            ("persistent", "x!!(1)"),
            ("handle", r#"S!?("produce", *x, [1]); out!(["sent", 1])"#),
            (
                "handle Nil",
                r#"S!?("produce", *x, [Nil]); out!(["sent", 1])"#,
            ),
        ] {
            let played =
                play_and_certify(&format!("{label} {inner}"), &hashset_program(inner, body)).await;
            assert_eq!(codes(&played), only("PayloadNotEmpty"), "{label} {inner}");
            assert!(labels(&played).is_empty(), "{label} {inner}");
        }
    }
}

/// HS-3: a bind with a pattern, a binder or a remainder aborts with `PatternNotEmpty` before any
/// staging, on every family and with every arrow. A join with one such bind aborts too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hashset_pattern_not_empty() {
    for inner in INNERS {
        for (label, body) in [
            ("binder", "for (@v <- x) { Nil }"),
            ("wildcard", "for (_ <- x) { Nil }"),
            ("literal", "for (@Nil <- x) { Nil }"),
            ("remainder", "for (...@rest <- x) { Nil }"),
            ("persistent binder", "for (@v <= x) { Nil }"),
            ("peek binder", "for (@v <<- x) { Nil }"),
            ("join", "for ( <- x & @v <- y) { Nil }"),
        ] {
            let played =
                play_and_certify(&format!("{label} {inner}"), &hashset_program(inner, body)).await;
            assert_eq!(codes(&played), only("PatternNotEmpty"), "{label} {inner}");
        }
    }
}

/// HS-3: a bind on a HashSet collection binds no variable, so its `where` guard names only the
/// variables around the receive. A guard that holds lets the receive take the unit. A guard that
/// refuses parks the receive, under the guard rule of decision 37, and the unit stays.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hashset_guards() {
    for inner in INNERS {
        for (label, guard, expected) in [
            ("holds", "n == 1", vec!["took"]),
            ("refuses", "n == 2", Vec::new()),
        ] {
            let played = play_and_certify(
                &format!("guard {label} {inner}"),
                &hashset_program(
                    inner,
                    &format!(
                        r#"match 1 {{ n => {{ S!?("produce", *x, []); for ( <- x where {guard}) {{ out!(["took", n]) }} }} }}"#
                    ),
                ),
            )
            .await;
            assert!(
                played.result.errors.is_empty(),
                "{label} {inner}: {:?}",
                played.result.errors
            );
            assert_eq!(labels(&played), expect(&expected), "{label} {inner}");
        }
    }
}

/// HS-4: a join of two empty binds on two collections of one HashSet space fires once both
/// units are there, on every family.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hashset_joins() {
    for inner in INNERS {
        let played = play_and_certify(
            &format!("join {inner}"),
            &hashset_program(
                inner,
                r#"S!?("produce", *x, []); S!?("produce", *y, []);
for ( <- x & <- y) { out!(["joined", 1]) }"#,
            ),
        )
        .await;
        assert!(
            played.result.errors.is_empty(),
            "{inner}: {:?}",
            played.result.errors
        );
        assert_eq!(labels(&played), expect(&["joined"]), "{inner}");
    }
}

/// HS-5: two adds and two takes. The set keeps one unit class, so its second take waits. The
/// bag and the ordered collections count their units. The cell holds one unit, so its second
/// add aborts with `CellOccupied`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hashset_disciplines() {
    let body = r#"S!?("produce", *x, []); S!?("produce", *x, []);
for ( <- x) { out!(["first", 1]) | for ( <- x) { out!(["second", 1]) } }"#;
    for inner in INNERS {
        let played =
            play_and_certify(&format!("two units {inner}"), &hashset_program(inner, body)).await;
        match inner {
            "cell" => assert_eq!(codes(&played), only("CellOccupied"), "{inner}"),
            "set" => {
                assert!(
                    played.result.errors.is_empty(),
                    "{inner}: {:?}",
                    played.result.errors
                );
                assert_eq!(labels(&played), expect(&["first"]), "{inner}");
            }
            _ => {
                assert!(
                    played.result.errors.is_empty(),
                    "{inner}: {:?}",
                    played.result.errors
                );
                assert_eq!(labels(&played), expect(&["first", "second"]), "{inner}");
            }
        }
    }
}

/// HS-5: the head of an ordered collection of units. The program adds three units to the
/// collection `"m"` through the handle, then takes one from `@"m"` under `use S`, which routes
/// to the same collection. The COMM of the take
/// names the record that it took: the oldest unit in a queue and the newest in a stack. A
/// priority queue takes its lowest level first, and a receive that names a level takes that
/// level.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hashset_head_order() {
    let member = new_gstring_par("m".to_string(), Vec::new(), false);
    let program = |inner: &str, adds: &str, take: &str| {
        hashset_program(
            inner,
            &format!(
                r#"for (@c <- S!?("config")) {{
  out!(["config", c]) |
  {{ {adds} use S {{ for ( <- @"m"{take}) {{ out!(["took", 1]) }} }} }}
}}"#
            ),
        )
    };
    let three = r#"S!?("produce", "m", []); S!?("produce", "m", []); S!?("produce", "m", []);"#;
    for (inner, taken) in [("queue", 0), ("stack", 2)] {
        let played = play_and_certify(&format!("head {inner}"), &program(inner, three, "")).await;
        assert!(
            played.result.errors.is_empty(),
            "{inner}: {:?}",
            played.result.errors
        );
        assert_eq!(labels(&played), expect(&["config", "took"]), "{inner}");
        let (units, takes) = units_and_takes(&played, space_of(&played), &member);
        assert_eq!(units.len(), 3, "{inner}");
        assert_eq!(takes, vec![units[taken].clone()], "{inner}");
    }
    // A unit at level 2, then a unit at level 0. A receive without a level takes level 0, and a
    // receive that names level 2 takes that unit.
    let levels = r#"S!?("produce", "m", []; priority=2); S!?("produce", "m", []; priority=0);"#;
    for (take, taken) in [("", 1), (" ~ (; priority=2)", 0)] {
        let played = play_and_certify(
            &format!("levels{take}"),
            &program("priorityqueue", levels, take),
        )
        .await;
        assert!(
            played.result.errors.is_empty(),
            "{take}: {:?}",
            played.result.errors
        );
        assert_eq!(labels(&played), expect(&["config", "took"]), "{take}");
        let (units, takes) = units_and_takes(&played, space_of(&played), &member);
        assert_eq!(units.len(), 2, "{take}");
        assert_eq!(takes, vec![units[taken].clone()], "{take}");
    }
    // A receive that names an empty level waits.
    let played = play_and_certify(
        "empty level",
        &program(
            "priorityqueue",
            r#"S!?("produce", "m", []; priority=2);"#,
            " ~ (; priority=0)",
        ),
    )
    .await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert_eq!(labels(&played), expect(&["config"]));
}

/// HS-6: every HashSet space has the qualifier `seq`. A factory call with `default`, with no
/// qualifier or with `temp` aborts with `BadConstructorArgument` and creates no space.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hashset_seq_only() {
    for inner in INNERS {
        for ctor in [r#""default""#, "", r#""temp""#] {
            let term = format!(
                r#"new factory(`rho:space:hashset:{inner}`) in {{
  for (S <- factory!?({ctor})) {{ @"{OUT}"!(["created", 1]) }}
}}"#
            );
            let played = play_and_certify(&format!("{inner} {ctor}"), &term).await;
            assert_eq!(
                codes(&played),
                only("BadConstructorArgument"),
                "{inner} {ctor}"
            );
            assert!(labels(&played).is_empty(), "{inner} {ctor}");
        }
    }
}

/// HS-7: a HashSet factory takes the params of its discipline only, in native execution. An
/// unknown param fails with `UnknownParam` and the nearest schema key within two edits, a
/// `priorities` below 1 fails with `InvalidParam`, and a valid bound is stored in the config.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hashset_factory_params() {
    let factory = |inner: &str, arguments: &str, body: &str| {
        format!(
            r#"new factory(`rho:space:hashset:{inner}`) in {{
  for (S <- factory!?({arguments})) {{ {body} }}
}}"#
        )
    };
    let created = format!(r#"@"{OUT}"!(["created", 1])"#);
    for inner in INNERS {
        let played = play_and_certify(
            &format!("unknown {inner}"),
            &factory(inner, r#""seq"; zeta=1"#, &created),
        )
        .await;
        assert_eq!(codes(&played), only("UnknownParam"), "{inner}");
        assert!(labels(&played).is_empty(), "{inner}");
    }
    let played = play_and_certify(
        "near priorities",
        &factory("priorityqueue", r#""seq"; prioritie=2"#, &created),
    )
    .await;
    assert_eq!(codes(&played), only("UnknownParam"));
    let errors = format!("{:?}", played.result.errors);
    assert!(
        errors.contains(r#"did you mean \"priorities\"?"#),
        "{errors}"
    );
    let played = play_and_certify(
        "priorities on a set",
        &factory("set", r#""seq"; priorities=2"#, &created),
    )
    .await;
    assert_eq!(codes(&played), only("UnknownParam"));
    let played = play_and_certify(
        "invalid priorities",
        &factory("priorityqueue", r#""seq"; priorities=0"#, &created),
    )
    .await;
    assert_eq!(codes(&played), only("InvalidParam"));
    let played = play_and_certify(
        "valid priorities",
        &factory(
            "priorityqueue",
            r#""seq"; priorities=2"#,
            &format!(r#"for (@c <- S!?("config")) {{ @"{OUT}"!(["config", c]) }}"#),
        ),
    )
    .await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    let config = played
        .out
        .iter()
        .flatten()
        .find_map(|value| match elements(value)? {
            [label, config] if label_of_string(label) == Some("config") => Some(config.clone()),
            _ => None,
        })
        .expect("the program sends its config");
    let config = SpaceConfig::from_par(&config).expect("a config record");
    assert_eq!(config.outer, "hashset");
    assert_eq!(config.inner, "priorityqueue");
    assert_eq!(
        config.params.get("priorities"),
        Some(&models::rust::utils::new_gint_par(2, Vec::new(), false))
    );
}

/// SQ-6 (d): a receive of a unit that is not there waits, and its wait ends the chain of its
/// channel, so a process never learns that a unit is absent. The sweep at the end of the deploy
/// removes the waiter.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hashset_absent_unit_waits() {
    for inner in INNERS {
        let played = play_and_certify(
            &format!("absent {inner}"),
            &hashset_program(
                inner,
                r#"for ( <- x) { out!(["present", 1]) } | out!(["waiting", 1])"#,
            ),
        )
        .await;
        assert!(
            played.result.errors.is_empty(),
            "{inner}: {:?}",
            played.result.errors
        );
        assert_eq!(labels(&played), expect(&["waiting"]), "{inner}");
        let played = play_and_certify(
            &format!("taken {inner}"),
            &hashset_program(
                inner,
                r#"S!?("produce", *x, []);
for ( <- x) { out!(["first", 1]) | for ( <- x) { out!(["second", 1]) } }"#,
            ),
        )
        .await;
        assert!(
            played.result.errors.is_empty(),
            "{inner}: {:?}",
            played.result.errors
        );
        assert_eq!(labels(&played), expect(&["first"]), "{inner}");
    }
}

// ─── Properties ──────────────────────────────────────────────────────────────────────────────────

/// The last operation of a generated HashSet program.
#[derive(Clone, Debug)]
enum Last {
    /// `for ( <- x)`.
    Take,
    /// A send of `values` values: a routed `x!(…)` or a handle `produce`.
    Payload { routed: bool, values: usize },
    /// A receive whose bind has a binder, a wildcard, a literal or a remainder.
    Pattern(usize),
}

/// The binds of [`Last::Pattern`].
const PATTERNS: [&str; 4] = ["@v", "_", "@Nil", "...@rest"];

fn lasts() -> impl Strategy<Value = Last> {
    prop_oneof![
        Just(Last::Take),
        (proptest::bool::ANY, 1usize..4)
            .prop_map(|(routed, values)| Last::Payload { routed, values }),
        (0..PATTERNS.len()).prop_map(Last::Pattern),
    ]
}

/// The body of a generated case: `adds` units through the handle, then `last`.
fn render(adds: usize, last: &Last) -> String {
    let mut body = String::new();
    for _ in 0..adds {
        body.push_str(r#"S!?("produce", *x, []); "#);
    }
    match last {
        Last::Take => body.push_str(r#"for ( <- x) { out!(["took", 1]) }"#),
        Last::Payload { routed, values } => {
            let values: Vec<String> = (1..=*values).map(|value| value.to_string()).collect();
            match routed {
                true => body.push_str(&format!("x!({})", values.join(", "))),
                false => body.push_str(&format!(
                    r#"S!?("produce", *x, [{}]); out!(["sent", 1])"#,
                    values.join(", ")
                )),
            }
        }
        Last::Pattern(index) => {
            body.push_str(&format!("for ({} <- x) {{ Nil }}", PATTERNS[*index]))
        }
    }
    body
}

/// `hashset_unit_rules` (`HashMapStore.v`) end to end: for generated families, numbers of adds
/// and last operations, a payload aborts with `PayloadNotEmpty`, a pattern aborts with
/// `PatternNotEmpty`, a second unit in a cell aborts with `CellOccupied` before the last
/// operation runs, and otherwise an empty receive takes a unit exactly when the program added
/// one. Every case plays natively and certifies.
#[test]
fn prop_hashset_unit_rules() {
    check(
        runtime_config(),
        (0..INNERS.len(), 0usize..3, lasts()),
        |(index, adds, last)| {
            let inner = INNERS[index];
            let term = hashset_program(inner, &render(adds, &last));
            let played = tokio_runtime().block_on(play_and_certify(inner, &term));
            let expected_codes = match (&last, inner == "cell" && adds >= 2) {
                (_, true) => only("CellOccupied"),
                (Last::Payload { .. }, false) => only("PayloadNotEmpty"),
                (Last::Pattern(_), false) => only("PatternNotEmpty"),
                (Last::Take, false) => BTreeSet::new(),
            };
            if codes(&played) != expected_codes {
                return Err(TestCaseError::fail(format!(
                    "{inner} {adds} {last:?}: {:?}",
                    played.result.errors
                )));
            }
            let expected_labels = match (&last, expected_codes.is_empty(), adds >= 1) {
                (Last::Take, true, true) => expect(&["took"]),
                _ => Vec::new(),
            };
            if labels(&played) != expected_labels {
                return Err(TestCaseError::fail(format!(
                    "{inner} {adds} {last:?}: {:?}",
                    played.outputs
                )));
            }
            Ok(())
        },
    );
}
