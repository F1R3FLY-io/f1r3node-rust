//! The Seq rules of P7 (design v3, W3: SQ-1 to SQ-8, decision 73).
//!
//! Every deploy plays natively at the v6 limits and certifies through its committed evidence,
//! as proposer self-certification and validator replay do (`support::native`). After each play,
//! the hot store holds no volatile channel (the residue check of VR-2).
//!
//! | Test | Property |
//! |---|---|
//! | `reducer_seq_payload` | SQ-2: a send whose data holds a seq name as data, in a quoted process, in a bundle, in a list, in a map or in the message of a handle `produce` aborts with `SeqSent`, and the deploy changes no state. The handle of a `seq` space may be sent, and the channel argument of the space's own `produce` and the forwarder hop are not scanned, so a sync `produce` runs. |
//! | `seq_scan_charge_is_schedule_independent` | Decision 83: the charge of the SQ-2 check is a function of the deploy. The creation job records a seq space, so the sends of one scheduling window read one book. Two shapes of the P9 step 3 probe, a touch of a stored space beside a seq creation, play 24 times each and certify, and play gives one host-work vector for each shape. |
//! | `reducer_seq_atomic_guard` | SQ-3 and SQ-6 on the traces of design v3, section 5: concurrent uses of one channel abort with `SeqConcurrent` (a send and a receive, a forged channel of `use S`, a PathMap name), a chain of sync uses runs, sync uses of two channels in parallel run, parallel reads and allocations use no channel, a join of two chained channels runs, and a re-issue at `FIRE_ROUND_CAP` is no use. |
//! | `prop_seq_claim_chain` | `SeqExclusive` (`SpaceCoordination.tla`): for generated uses of two channels by generated participants, the claims admit a use exactly when the last admitted use of its channel is a prefix of its participant path, so the admitted uses of each channel form one prefix chain. A job that fails stores nothing. |
//! | `prop_seq_qualifier_support` | `QualifierSupport` (`SpaceCoordination.tla`): for every family, qualifier and execution model, an accepted constructor of a HashSet family has `seq`, and an accepted `seq` constructor has native execution. Among the families with a schema, gate 3 refuses exactly `seq` in legacy execution (QT-4, QT-5). |
//! | `prop_use_context_is_lexical` | `UseBalance` (`SpaceCoordination.tla`): for generated nests of `use` blocks over two spaces, each send on a forged channel lands in the space of its innermost `use` block, or in the default space outside every block, in native execution. |
//! | `prop_seq_scan_finds_every_seq_name` | `SeqNotSent` (`SpaceCoordination.tla`): for generated data trees with ids at generated places, the scan of SQ-2 fails with `SeqSent` exactly when a tree holds a name of a live `seq` space, and it names that space. |

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use models::rhoapi::cost_signature::Value as SignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    Bundle, CostSignature, CostStack, EList, EMap, ETuple, Expr, KeyValuePair, Match, MatchCase,
    Par, Receive, ReceiveBind, Send,
};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::space_factory_urn::{
    SpaceFactoryUrn, SpaceOuter, SpaceQualifier as UrnQualifier,
};
use models::rust::utils::{new_gint_par, new_gstring_par};
use proptest::prelude::{any, prop_oneof, Just, Strategy};
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::reified::costs::ScanMeter;
use rholang::rust::interpreter::reified::seq::check_send;
use rholang::rust::interpreter::spaces::ephemeral::{ClaimRole, SeqClaims, VolatileRegistry};
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::factory::constructor::{Execution, FactoryConstructor};
use rholang::rust::interpreter::spaces::factory::defaults::constructor_schema;
use rholang::rust::interpreter::spaces::namespace::{
    self, gprivate, CollectionKey, Durability, NameTail, NativeSpace, ReifiedId, SpaceId, SID_LEN,
};
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rspace_plus_plus::rspace::operation_context::{self, OperationOrder, PathSegment};
use support::native::{
    assert_reproduces, certify, certify_with, host_work, native_play_with, NativeChain, Played,
    AMPLE_LIMIT, OUT,
};
use support::props::{check, pure_config, runtime_config, tokio_runtime};
use support::session::{error_codes, Injections};

// ─── Helpers ───────────────────────────────────────────────────────────────────────────────────

/// The phlo limit of the deploys of this suite. The trace of the round cap makes 65 handle calls
/// and 65 fires, so it needs more phlo than the other suites.
const SEQ_LIMIT: u64 = 20 * AMPLE_LIMIT;

/// A deploy that creates a `seq` space `S` of the family `urn`, draws the names `x` and `y` of
/// `S` with `gensym`, and runs `body`. The values on `out` go to the public channel [`OUT`].
fn seq_program(urn: &str, body: &str) -> String {
    format!(
        r#"new factory(`{urn}`), out in {{
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
    play_and_certify_with(label, term, host_work).await
}

/// A host-work budget of 2^40 units in each dimension, for the trace of the round cap. Its 65
/// records and 65 fires on one queue use about 1.03 * 10^9 `VerificationBytes` in play with
/// every qualifier (measured: 1,026,708,249 with `seq`, 1,024,973,702 with `temp`, 1,024,976,585
/// with `default`), above the v6 limit of 2^28. The test checks the claims, not the v6 limits.
fn cap_host_work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1 << 40)))
}

/// [`play_and_certify`] with the host-work budget that `host` makes for play and for replay.
async fn play_and_certify_with(label: &str, term: &str, host: fn() -> HostWorkBudget) -> Played {
    let played = native_play_with(term, SEQ_LIMIT, host()).await;
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
    let certification = certify_with(&played, term, SEQ_LIMIT, host()).await;
    assert_reproduces(label, &played, &certification);
    played
}

/// The distinct error codes of `played`.
fn codes(played: &Played) -> BTreeSet<String> {
    error_codes(&played.result.errors).into_iter().collect()
}

/// The single code `code`.
fn only(code: &str) -> BTreeSet<String> { BTreeSet::from([code.to_string()]) }

fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

// ─── SQ-2: the payload scan ──────────────────────────────────────────────────────────────────────

/// SQ-2 (design v3, section 5): a seq name never leaves its process as data. Each rejected send
/// aborts before any mutation, so the deploy reaches the post-state root of the same deploy with
/// the body `Nil`: the send made no produce.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reducer_seq_payload() {
    let urn = "rho:space:hashmap:queue";
    let empty = play_and_certify("empty body", &seq_program(urn, "Nil")).await;
    assert!(empty.result.errors.is_empty(), "{:?}", empty.result.errors);
    for (label, body) in [
        ("data", "out!(*x)"),
        ("quote", "out!(x!(1))"),
        ("bundle", "out!(bundle+{*x})"),
        ("list", "out!([1, *x])"),
        ("map", r#"out!({"k": *x})"#),
        ("message", r#"S!(*out, "produce", *x, [*x])"#),
        ("return name", r#"S!(*x, "qualifier")"#),
    ] {
        let played = play_and_certify(label, &seq_program(urn, body)).await;
        assert_eq!(codes(&played), only("SeqSent"), "{label}");
        assert_eq!(
            played.root, empty.root,
            "{label}: a rejected send changes no state"
        );
    }
    // The handle of a seq space is not a name, and the channel argument of the space's own
    // produce and the forwarder hop are exempt, so a sync produce runs.
    let played = play_and_certify(
        "exempt",
        &seq_program(
            urn,
            r#"out!(["handle", *S]) |
{ S!?("produce", *x, [1]); for (@v <- x) { out!(["got", v]) } }"#,
        ),
    )
    .await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert!(
        played
            .outputs
            .iter()
            .any(|output| output.contains("\"got\"")),
        "{:?}",
        played.outputs
    );
}

/// The number of runs of each shape of [`seq_scan_charge_is_schedule_independent`]. Before the
/// fix of decision 83, about one run in five gave a second play host-work vector.
const SCHEDULE_RUNS: usize = 24;

/// Block 1 of [`seq_scan_charge_is_schedule_independent`] (the probe of P9 step 3): a
/// `hashmap:bag` space with two records on `a` and a waiter on `b`, whose body sends to a
/// persistent forwarder. The handle and the names of the space wait on a public channel.
const SCHEDULE_SETUP: &str = r#"new factory(`rho:space:hashmap:bag`), out in {
  for (space <- factory!?("default")) {
    for (a <- space!?("gensym")) { for (b <- space!?("gensym")) {
      for (c <- space!?("gensym")) { for (d <- space!?("gensym")) {
        @"rrs-seq-schedule-stash"!(*space, *a, *b, *c, *d) |
        a!(1) | a!(2) | for (@v <- b) { out!(["fired", v]) }
      } }
    } }
  } |
  for (@value <= out) { @"rrs-seq-schedule-out"!(value) }
}"#;

/// The creation of a seq space.
const SEQ_CREATE: &str =
    r#"new sf(`rho:space:hashset:bag`) in { for (S <- sf!?("seq")) { out!(["seq", 0]) } }"#;

/// The creation of a seq space and one name of it.
const SEQ_GENSYM: &str = r#"new sf(`rho:space:hashset:bag`) in {
  for (S <- sf!?("seq")) { for (u <- S!?("gensym")) { out!(["seq", 0]) } }
}"#;

/// Block 2: a touch of the space of block 1 in parallel with `seq`. The take stores a record
/// again, and the send on `b` fires the waiter of block 1. Their sends run in the scheduling
/// windows around the creation job of the seq space.
fn schedule_touch(seq: &str) -> String {
    format!(
        r#"new out in {{
  for (space, a, b, c, d <<- @"rrs-seq-schedule-stash") {{
    for (@x <- a) {{ out!(["take", x]) | a!(x + 10) }} | b!(3) | {seq}
  }} |
  for (@value <= out) {{ @"rrs-seq-schedule-out"!(value) }}
}}"#
    )
}

/// The host work of `budget` in each dimension.
fn host_vector(budget: &HostWorkBudget) -> Vec<u64> {
    budget
        .usages()
        .into_array()
        .iter()
        .map(|usage| usage.get())
        .collect()
}

/// Decision 83 (P7 design v3, VR-2): the charge of the SQ-2 scan is a function of the deploy.
/// The creation job records a seq space, and no participant runs during a job, so the sends of
/// one scheduling window read one set of seq spaces. Each shape plays [`SCHEDULE_RUNS`] times
/// on a fresh chain at the v6 limits, and native replay certifies each play. Play gives one
/// host-work vector for each shape. The test compares play only, as N9 does: native replay in
/// this tree reserves host work that depends on the schedule also for programs without spaces
/// (verification.md, section 34.4.1).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seq_scan_charge_is_schedule_independent() {
    let mut splits = Vec::with_capacity(2);
    for (shape, seq) in [
        ("a touch beside a seq creation", SEQ_CREATE),
        ("a touch beside a seq gensym", SEQ_GENSYM),
    ] {
        let term = schedule_touch(seq);
        let mut vectors: BTreeMap<Vec<u64>, usize> = BTreeMap::new();
        for _ in 0..SCHEDULE_RUNS {
            let mut chain = NativeChain::new().await;
            let setup = chain
                .play(SCHEDULE_SETUP, "seq schedule setup", AMPLE_LIMIT)
                .await;
            assert!(
                setup.result.errors.is_empty(),
                "{shape}: the setup fails: {:?}",
                setup.result.errors
            );
            let host = host_work();
            let played = chain
                .play_with(
                    &term,
                    "seq schedule touch",
                    SEQ_LIMIT,
                    host.clone(),
                    Injections::new(),
                )
                .await;
            assert!(
                played.result.errors.is_empty(),
                "{shape}: the deploy fails: {:?}",
                played.result.errors
            );
            assert!(
                played.residue.is_empty(),
                "{shape}: the sweep leaves no volatile channel: {:?}",
                played.residue
            );
            let certification = certify(&played, &term, SEQ_LIMIT).await;
            assert_reproduces(shape, &played, &certification);
            *vectors.entry(host_vector(&host)).or_insert(0) += 1;
        }
        if vectors.len() != 1 {
            splits.push(format!("{shape}: {vectors:?}"));
        }
    }
    assert!(
        splits.is_empty(),
        "the play host work of a shape depends on the schedule: {splits:#?}"
    );
}

// ─── SQ-3: the claims ────────────────────────────────────────────────────────────────────────────

/// SQ-3 and SQ-6 (design v3, section 5) on native deploys. Concurrent uses of one channel abort
/// with `SeqConcurrent`, the same on every run. A chain of sync uses, sync uses of two channels
/// in parallel, parallel reads and allocations, and a join of two chained channels run. A
/// re-issue at `FIRE_ROUND_CAP` is internal, so it is no use (A3).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reducer_seq_atomic_guard() {
    let queue = "rho:space:hashmap:queue";
    let bag = "rho:space:hashmap:bag";
    let pathmap = "rho:space:pathmap:queue";
    // (i) A send and a receive in parallel on one channel.
    for urn in [queue, bag, pathmap] {
        let played = play_and_certify(
            &format!("concurrent {urn}"),
            &seq_program(urn, "x!(1) | for (@v <- x) { Nil }"),
        )
        .await;
        assert_eq!(codes(&played), only("SeqConcurrent"), "{urn}");
    }
    // A forged channel that `use S` routes into S has a claim of its own.
    let played = play_and_certify(
        "forged channel",
        &seq_program(queue, r#"use S { @"f"!(1) | for (@v <- @"f") { Nil } }"#),
    )
    .await;
    assert_eq!(codes(&played), only("SeqConcurrent"));
    // (ii) A chain of sync uses of one channel.
    for urn in [queue, pathmap] {
        let played = play_and_certify(
            &format!("chain {urn}"),
            &seq_program(
                urn,
                r#"S!?("produce", *x, [1]); S!?("produce", *x, [2]);
for (@a <- x) { for (@b <- x) { out!(["chain", a, b]) } }"#,
            ),
        )
        .await;
        assert!(
            played.result.errors.is_empty(),
            "{urn}: {:?}",
            played.result.errors
        );
        assert!(
            played
                .outputs
                .iter()
                .any(|output| output.contains("\"chain\"") && output.contains("GInt(1)")),
            "{urn}: {:?}",
            played.outputs
        );
    }
    // (iii) Sync uses of two channels in parallel, and parallel reads and allocations, which
    // use no channel (decision 73).
    let played = play_and_certify(
        "parallel channels and reads",
        &seq_program(
            queue,
            r#"{ S!?("produce", *x, [1]); out!(["x", 1]) } |
{ S!?("produce", *y, [2]); out!(["y", 2]) } |
for (@q <- S!?("qualifier")) { out!(["qualifier", q]) } |
for (@c <- S!?("config")) { out!(["config", 1]) } |
for (_ <- S!?("gensym")) { out!(["gensym", 1]) } |
for (_ <- S!?("gensym")) { out!(["gensym", 2]) }"#,
        ),
    )
    .await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert_eq!(played.out.len(), 6, "{:?}", played.outputs);
    // A join of two channels, each used by the chain before it.
    let played = play_and_certify(
        "join",
        &seq_program(
            queue,
            r#"S!?("produce", *x, [1]); S!?("produce", *y, [2]);
for (@a <- x & @b <- y) { out!(["join", a, b]) }"#,
        ),
    )
    .await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert!(played
        .outputs
        .iter()
        .any(|output| output.contains("\"join\"")));
    // (v) A re-issue at the round cap. The persistent receive fires 64 times in its first
    // transaction, then the runtime re-issues it for the last record. The first body uses x
    // again, a descendant of the receive. The re-issue is internal, so it uses nothing, although
    // it is not a descendant of the first body.
    // A loop contract makes the 65 sync produces, each below the reply of the one before it.
    let body = r#"new loop in {
  contract loop(@n) = {
    if (n <= 65) { S!?("produce", *x, [n]); loop!(n + 1) }
    else { for (@v <= x) { match v { 1 => { for (_ <- x) { out!(["again", 1]) } } _ => Nil } } }
  } |
  loop!(1)
}"#;
    let played = play_and_certify_with("round cap", &seq_program(queue, body), cap_host_work).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert!(played
        .outputs
        .iter()
        .any(|output| output.contains("\"again\"")));
}

// ─── Properties ──────────────────────────────────────────────────────────────────────────────────
// ─── QualifierSupport and UseBalance ───────────────────────────────────────────────────────────

/// `QualifierSupport` through gate 3 (QT-4, QT-5): every accepted constructor of a HashSet
/// family has `seq`, and every accepted `seq` constructor has native execution. Among the
/// families with a schema, gate 3 refuses exactly `seq` in legacy execution.
#[test]
fn prop_seq_qualifier_support() {
    let families: Vec<SpaceFactoryUrn> = SpaceFactoryUrn::all().collect();
    check(
        pure_config(),
        (0..families.len(), 0usize..3, any::<bool>()),
        |(index, qualifier, native)| {
            let family = families[index];
            let qualifier = ["default", "temp", "seq"][qualifier];
            let execution = match native {
                true => Execution::Native,
                false => Execution::Legacy,
            };
            let ctor = models::rust::utils::new_elist_par(
                vec![string(qualifier)],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            );
            let result = FactoryConstructor::validate_in(family, &ctor, execution);
            if let Ok(validated) = &result {
                if family.outer == SpaceOuter::HashSet && validated.qualifier != UrnQualifier::Seq {
                    return Err(TestCaseError::fail(format!(
                        "{family}: a HashSet space without seq"
                    )));
                }
                if validated.qualifier == UrnQualifier::Seq && !native {
                    return Err(TestCaseError::fail(format!(
                        "{family}: seq in legacy execution"
                    )));
                }
            }
            if constructor_schema(family).is_some() {
                let refused = matches!(result, Err(SpaceError::QualifierUnavailable(_)));
                if refused != (qualifier == "seq" && !native) {
                    return Err(TestCaseError::fail(format!(
                        "{family} {qualifier} native {native}: {result:?}"
                    )));
                }
            }
            Ok(())
        },
    );
}

/// A generated process: a send on a forged channel, a `use` block of the space A (0) or B (1),
/// or a parallel composition.
#[derive(Clone, Debug)]
enum Shape {
    Send,
    Use(u8, Vec<Shape>),
    Par(Vec<Shape>),
}

fn shapes() -> impl Strategy<Value = Shape> {
    Just(Shape::Send).prop_recursive(3, 12, 3, |inner| {
        prop_oneof![
            (0u8..2, proptest::collection::vec(inner.clone(), 1..3))
                .prop_map(|(space, children)| Shape::Use(space, children)),
            proptest::collection::vec(inner, 1..3).prop_map(Shape::Par),
        ]
    })
}

/// The label of the public channel of send `index`.
fn use_label(index: usize) -> String { format!("rrs-p7-use-{index}") }

/// Renders `shape` in the use context `context`, and records the context of each send in send
/// order.
fn render(shape: &Shape, context: Option<u8>, sends: &mut Vec<Option<u8>>) -> String {
    match shape {
        Shape::Send => {
            let index = sends.len();
            sends.push(context);
            format!(r#"@"{}"!({index})"#, use_label(index))
        }
        Shape::Use(space, children) => {
            let body: Vec<String> = children
                .iter()
                .map(|child| render(child, Some(*space), sends))
                .collect();
            format!(
                "use {} {{ {} }}",
                ["sa", "sb"][usize::from(*space)],
                body.join(" | ")
            )
        }
        Shape::Par(children) => {
            let parts: Vec<String> = children
                .iter()
                .map(|child| render(child, context, sends))
                .collect();
            format!("{{ {} }}", parts.join(" | "))
        }
    }
}

/// `UseBalance` end to end in native execution: each send on a forged channel lands in the
/// space of its innermost `use` block, or in the default space outside every block. The sends
/// of one block share one space, and the two blocks have two spaces.
#[test]
fn prop_use_context_is_lexical() {
    check(runtime_config(), shapes(), |shape| {
        let mut sends = Vec::new();
        let body = render(&shape, None, &mut sends);
        let term = format!(
            r#"new FA(`rho:space:hashmap:bag`), FB(`rho:space:hashmap:bag`) in {{
  for (sa <- FA!?("default"); sb <- FB!?("default")) {{ {body} }}
}}"#
        );
        tokio_runtime().block_on(async {
            let mut chain = NativeChain::new().await;
            let played = chain.play(&term, "rrs-p7-use", SEQ_LIMIT).await;
            if !played.result.errors.is_empty() {
                return Err(TestCaseError::fail(format!("{:?}", played.result.errors)));
            }
            let certification = certify(&played, &term, SEQ_LIMIT).await;
            assert_reproduces("use context", &played, &certification);
            let mut space_of: BTreeMap<u8, SpaceId> = BTreeMap::new();
            for (index, context) in sends.iter().enumerate() {
                let label = use_label(index);
                let key = CollectionKey::of_channel(&string(&label)).expect("a key");
                let homes: BTreeSet<SpaceId> = played
                    .durable
                    .iter()
                    .filter_map(|channel| match namespace::reified_id_of(channel) {
                        Some(ReifiedId::Data(space, tail)) if tail == key.as_bytes() => Some(space.sid),
                        _ => None,
                    })
                    .collect();
                let default = chain.values(&label).await;
                match context {
                    None => {
                        if default.len() != 1 || !homes.is_empty() {
                            return Err(TestCaseError::fail(format!(
                                "send {index} outside every block: default {default:?}, spaces {homes:?}"
                            )));
                        }
                    }
                    Some(space) => {
                        let [home] = homes.iter().copied().collect::<Vec<_>>()[..] else {
                            return Err(TestCaseError::fail(format!(
                                "send {index} in block {space}: spaces {homes:?}"
                            )));
                        };
                        if !default.is_empty() || *space_of.entry(*space).or_insert(home) != home {
                            return Err(TestCaseError::fail(format!(
                                "send {index} in block {space}: default {default:?}, space {home:?}"
                            )));
                        }
                    }
                }
            }
            if let (Some(a), Some(b)) = (space_of.get(&0), space_of.get(&1)) {
                if a == b {
                    return Err(TestCaseError::fail("the two blocks share one space"));
                }
            }
            Ok(())
        })
    });
}

/// One generated use: a channel (0 or 1) and the participant path of the job that uses it, and
/// whether the job fails after its claims.
#[derive(Clone, Debug)]
struct Use {
    channel: u8,
    path: Vec<PathSegment>,
    fails: bool,
}

fn uses() -> impl Strategy<Value = Vec<Use>> {
    let segment = (0u64..3, 0u64..3);
    let path = proptest::collection::vec(segment, 0..4);
    let one =
        (0u8..2, path, proptest::bool::weighted(0.2)).prop_map(|(channel, path, fails)| Use {
            channel,
            path,
            fails,
        });
    proptest::collection::vec(one, 1..24)
}

/// `SeqExclusive` through the claims of the engine (`SeqClaims`): each use runs as a job in the
/// operation order of its participant path. The oracle admits a use exactly when the last
/// admitted use of its channel is a prefix of its path. A failing job claims and then stores
/// nothing (SQ-4).
#[test]
fn prop_seq_claim_chain() {
    let space = NativeSpace {
        sid: [6; SID_LEN],
        durability: Durability::Ephemeral,
    };
    let channels = [0u8, 1].map(|byte| {
        gprivate(namespace::data_channel(
            space,
            &CollectionKey::from_bytes(vec![byte]).expect("a key"),
        ))
    });
    check(pure_config(), uses(), |uses| {
        tokio_runtime().block_on(async {
            let registry = Arc::new(VolatileRegistry::default());
            let mut last: BTreeMap<u8, Vec<PathSegment>> = BTreeMap::new();
            let mut admitted: BTreeMap<u8, Vec<Vec<PathSegment>>> = BTreeMap::new();
            for Use {
                channel,
                path,
                fails,
            } in uses
            {
                let mut order = path.clone();
                order.push((9, 0));
                let order = OperationOrder {
                    session: [3; 32],
                    path: order.into(),
                };
                let registry = Arc::clone(&registry);
                let claimed = &channels[usize::from(channel)];
                let result = operation_context::scope(order, async move {
                    let meter = ScanMeter::new(None);
                    let mut claims =
                        SeqClaims::open(&registry, space, SpaceQualifier::Seq, false, &meter)
                            .expect("a seq job opens")
                            .expect("a program job claims");
                    claims.claim(claimed, ClaimRole::Data, &meter)?;
                    if !fails {
                        claims.store();
                    }
                    Ok::<(), SpaceError>(())
                })
                .await;
                let expected = last
                    .get(&channel)
                    .is_none_or(|claim| path.starts_with(claim));
                match (expected, result) {
                    (true, Ok(())) => {
                        if !fails {
                            last.insert(channel, path.clone());
                            admitted.entry(channel).or_default().push(path);
                        }
                    }
                    (false, Err(SpaceError::SeqConcurrent { space: refused })) => {
                        if refused != space.sid {
                            return Err(TestCaseError::fail("the error names another space"));
                        }
                    }
                    (expected, result) => {
                        return Err(TestCaseError::fail(format!(
                            "channel {channel} path {path:?}: expected {expected}, got {result:?}"
                        )));
                    }
                }
            }
            for chain in admitted.values() {
                if chain.windows(2).any(|pair| !pair[1].starts_with(&pair[0])) {
                    return Err(TestCaseError::fail(format!(
                        "not one prefix chain: {chain:?}"
                    )));
                }
            }
            Ok(())
        })
    });
}

/// An id that a generated data tree holds.
#[derive(Clone, Copy, Debug)]
enum Id {
    /// A name of the live `seq` space.
    Seq,
    /// A name of a `seq` space of another evaluation.
    Ended,
    /// A name of a `temp` space.
    Temp,
    /// A name of a durable space.
    Durable,
    /// The handle of the live `seq` space.
    Handle,
    /// An ordinary unforgeable name.
    Plain,
}

/// A generated data tree: the places of the `Par` tree where a name can hide.
#[derive(Clone, Debug)]
enum Tree {
    Leaf(Id),
    Int(i64),
    List(Vec<Tree>),
    Tuple(Vec<Tree>),
    MapValue(Box<Tree>),
    SendData(Box<Tree>),
    Bundle(Box<Tree>),
    Pattern(Box<Tree>),
    Guard(Box<Tree>),
    CostQuote(Box<Tree>),
}

const LIVE: SpaceId = [1; SID_LEN];
const ENDED: SpaceId = [2; SID_LEN];

impl Tree {
    /// True when the tree holds a name of the live `seq` space.
    fn holds_seq_name(&self) -> bool {
        match self {
            Tree::Leaf(id) => matches!(id, Id::Seq),
            Tree::Int(_) => false,
            Tree::List(trees) | Tree::Tuple(trees) => trees.iter().any(Tree::holds_seq_name),
            Tree::MapValue(tree)
            | Tree::SendData(tree)
            | Tree::Bundle(tree)
            | Tree::Pattern(tree)
            | Tree::Guard(tree)
            | Tree::CostQuote(tree) => tree.holds_seq_name(),
        }
    }

    fn to_par(&self) -> Par {
        let ephemeral = |sid: SpaceId| NativeSpace {
            sid,
            durability: Durability::Ephemeral,
        };
        match self {
            Tree::Leaf(Id::Seq) => {
                gprivate(namespace::name_id(ephemeral(LIVE), NameTail::Index(1)))
            }
            Tree::Leaf(Id::Ended) => gprivate(namespace::name_id(
                ephemeral(ENDED),
                NameTail::Random([4; 32]),
            )),
            Tree::Leaf(Id::Temp) => gprivate(namespace::name_id(
                ephemeral([3; SID_LEN]),
                NameTail::Index(2),
            )),
            Tree::Leaf(Id::Durable) => gprivate(namespace::name_id(
                NativeSpace {
                    sid: LIVE,
                    durability: Durability::Durable,
                },
                NameTail::Index(3),
            )),
            Tree::Leaf(Id::Handle) => gprivate(namespace::handle_channel(ephemeral(LIVE))),
            Tree::Leaf(Id::Plain) => gprivate(vec![7; 32]),
            Tree::Int(value) => int(*value),
            Tree::List(trees) => expr(ExprInstance::EListBody(EList {
                ps: trees.iter().map(Tree::to_par).collect(),
                ..Default::default()
            })),
            Tree::Tuple(trees) => expr(ExprInstance::ETupleBody(ETuple {
                ps: trees.iter().map(Tree::to_par).collect(),
                ..Default::default()
            })),
            Tree::MapValue(tree) => expr(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: Some(string("k")),
                    value: Some(tree.to_par()),
                }],
                ..Default::default()
            })),
            Tree::SendData(tree) => Par {
                sends: vec![Send {
                    chan: Some(string("c")),
                    data: vec![tree.to_par()],
                    ..Default::default()
                }],
                ..Default::default()
            },
            Tree::Bundle(tree) => Par {
                bundles: vec![Bundle {
                    body: Some(tree.to_par()),
                    write_flag: true,
                    read_flag: true,
                }],
                ..Default::default()
            },
            Tree::Pattern(tree) => Par {
                receives: vec![Receive {
                    binds: vec![ReceiveBind {
                        patterns: vec![tree.to_par()],
                        source: Some(string("c")),
                        ..Default::default()
                    }],
                    body: Some(Par::default()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            Tree::Guard(tree) => Par {
                matches: vec![Match {
                    target: Some(int(0)),
                    cases: vec![MatchCase {
                        pattern: Some(int(0)),
                        source: Some(Par::default()),
                        free_count: 0,
                        guard: Some(tree.to_par()),
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
            Tree::CostQuote(tree) => Par {
                cost_stacks: vec![CostStack {
                    cells: vec![CostSignature {
                        value: Some(SignatureValue::Quote(tree.to_par())),
                    }],
                }],
                ..Default::default()
            },
        }
    }
}

fn expr(instance: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

fn trees() -> impl Strategy<Value = Tree> {
    let leaf = prop_oneof![
        Just(Tree::Leaf(Id::Seq)),
        Just(Tree::Leaf(Id::Ended)),
        Just(Tree::Leaf(Id::Temp)),
        Just(Tree::Leaf(Id::Durable)),
        Just(Tree::Leaf(Id::Handle)),
        Just(Tree::Leaf(Id::Plain)),
        any::<i64>().prop_map(Tree::Int),
        any::<i64>().prop_map(Tree::Int),
    ];
    leaf.prop_recursive(5, 48, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..4).prop_map(Tree::List),
            proptest::collection::vec(inner.clone(), 0..4).prop_map(Tree::Tuple),
            inner
                .clone()
                .prop_map(|tree| Tree::MapValue(Box::new(tree))),
            inner
                .clone()
                .prop_map(|tree| Tree::SendData(Box::new(tree))),
            inner.clone().prop_map(|tree| Tree::Bundle(Box::new(tree))),
            inner.clone().prop_map(|tree| Tree::Pattern(Box::new(tree))),
            inner.clone().prop_map(|tree| Tree::Guard(Box::new(tree))),
            inner.prop_map(|tree| Tree::CostQuote(Box::new(tree))),
        ]
    })
}

/// `SeqNotSent` through the scan of SQ-2: for generated data trees, a send on an ordinary
/// channel fails with `SeqSent` naming the live space exactly when a tree holds one of its names.
/// The names of an ended `seq` space, of a `temp` space and of a durable space, the handle of the
/// live space and ordinary names pass.
#[test]
fn prop_seq_scan_finds_every_seq_name() {
    let live = BTreeSet::from([LIVE]);
    check(
        pure_config(),
        proptest::collection::vec(trees(), 0..4),
        |trees| {
            let pars: Vec<Par> = trees.iter().map(Tree::to_par).collect();
            let result = check_send(&string("out"), &pars, &live, &ScanMeter::new(None));
            match (trees.iter().any(Tree::holds_seq_name), result) {
                (true, Err(SpaceError::SeqSent { space })) if space == LIVE => Ok(()),
                (false, Ok(())) => Ok(()),
                (holds, result) => Err(TestCaseError::fail(format!(
                    "holds a seq name: {holds}, scan: {result:?}"
                ))),
            }
        },
    );
}
