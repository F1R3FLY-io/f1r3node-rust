//! The `temp` qualifier, the volatile sweep and the dangling ids of P7 (design v3, W1 and W2).
//!
//! Every deploy plays natively at the v6 limits and certifies through its committed evidence,
//! as proposer self-certification and validator replay do (`support::native`). After each play,
//! the hot store holds no volatile channel (the residue check of VR-2).
//!
//! | Test | Property |
//! |---|---|
//! | `space_temp_replay_equal_{hashmap,pathmap,array,vector}` | Each of the 24 families plays its program with the `temp` qualifier without error, delivers every part, leaves no volatile channel, and native replay reproduces the play (errors, complete journal, usage, authority events, byte observations, post-state root). |
//! | `space_temp_swept_at_deploy_end` | The program of each family with `default` leaves durable space channels with records or waiters before the checkpoint, and with `temp` leaves no space channel at all. |
//! | `space_temp_join_swept` | A parked join of a `temp` space and its join entries are swept. |
//! | `space_temp_failed_deploy_reverts` | A `temp` deploy that aborts (`CellOccupied`) is swept on the `Err` path, and native replay reproduces the failure. |
//! | `space_temp_many_volatile_keys_export` | A deploy with forty volatile collections certifies: the swept keys are root-neutral deletes that the native export meters (VR-10). |
//! | `reified_ephemeral_dangling_next_deploy` | A handle and a name of a `temp` space that an earlier deploy stored fail in a later deploy at every entry point of DG-1: a send and a receive on the name (`DanglingName`), a handle call, `use` and `new x : h` (`DanglingHandle`), `getSpaceAgent` (typed reply with the space id), and a parked continuation that fires into the dead space. Each error names the space id, and native replay reproduces each deploy. |
//! | `reified_ephemeral_dangling_charge_is_fixed` | The failing deploy costs the same phlo whatever the dead space held (DG-4). |
//! | `reified_ephemeral_cost_charged` | A `temp` program pays phlo, and it emits as many produce, consume and COMM events as the same program with `default` (QT-3). |
//! | `reified_sweep_runs_at_each_entry_point_native` | Native play, native replay and the exploratory entry `RhoRuntime::inj` sweep. The unit tests `reified_sweep_runs_at_each_entry_point_*` of the reducer hooks cover `inj`, `inj_with_observation` and `inj_with_host_work` directly. |
//! | `reducer_qualifier_matrix` | The factories accept `default` and `temp` on every family with a schema, refuse `seq` with `QualifierUnavailable` until the Seq rules land, and refuse `mirror` with `temp` (`InvalidParam`). The HashSet families have no factory yet (F1). |
//! | `reified_handle_qualifier_method_temp` | The handle method `qualifier` of a `temp` space replies `"temp"`. |
//! | `reified_fips_example_temp_pathmap` | The example code of the FIPS with a `temp` PathMap space: names of both spaces in one `new`, a forged name in `use`, a same-space join and a sequential receive, `getSpaceAgent` and `assertEqual`. The handles name the same spaces inside and outside the `new` block, the deploy certifies, and the sweep removes the `temp` space. |
//! | `prop_sweep_complete` | For generated `temp` programs of every family with extra stores and waiters, the hot store holds no volatile channel after the deploy, on the `Ok` and the `Err` path, and native replay reproduces the play. The property runs with a host-work budget above the v6 limits (`sweep_host_work`). |

#[path = "support/mod.rs"]
mod support;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::reified::builders;
use models::rust::reified::params::SendParam;
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter, SpaceQualifier};
use models::rust::utils::{new_elist_par, new_gbool_par, new_gint_par, new_gstring_par};
use proptest::prelude::{prop_oneof, Just, Strategy};
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::factory::constructor::FactoryConstructor;
use rholang::rust::interpreter::spaces::factory::defaults::constructor_schema;
use rholang::rust::interpreter::spaces::namespace::{self, Durability, ReifiedId, SpaceId};
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
use support::family_programs::{program_with, space_program, Inner, Outer, JOIN};
use support::native::{
    assert_reproduces, certify, certify_with, native_play, native_play_with, NativeChain, Played,
    AMPLE_LIMIT, OUT,
};
use support::props::{check, runtime_config, tokio_runtime};

// ─── Helpers ───────────────────────────────────────────────────────────────────────────────────

/// The codes of `errors`: the `<Code>` of each `reified:<Code>:` error, the Debug form of every
/// other error. An aggregate error gives the codes of its parts.
fn error_codes(errors: &[InterpreterError]) -> Vec<String> {
    let mut codes = Vec::with_capacity(errors.len());
    for error in errors {
        push_codes(error, &mut codes);
    }
    codes
}

fn push_codes(error: &InterpreterError, codes: &mut Vec<String>) {
    match error {
        InterpreterError::AggregateError { interpreter_errors } => {
            for inner in interpreter_errors {
                push_codes(inner, codes);
            }
        }
        InterpreterError::ReduceError(message) => codes.push(
            message
                .strip_prefix("reified:")
                .and_then(|rest| rest.split(':').next())
                .map(str::to_owned)
                .unwrap_or_else(|| message.clone()),
        ),
        other => codes.push(format!("{other:?}")),
    }
}

/// The messages of the `ReduceError`s of `errors`.
fn messages(errors: &[InterpreterError]) -> Vec<String> {
    let mut messages = Vec::new();
    for error in errors {
        match error {
            InterpreterError::AggregateError { interpreter_errors } => {
                messages.extend(self::messages(interpreter_errors));
            }
            InterpreterError::ReduceError(message) => messages.push(message.clone()),
            _ => {}
        }
    }
    messages
}

/// True when some output of `played` carries the label `label`.
fn delivers(played: &Played, label: &str) -> bool {
    let quoted = format!("\"{label}\"");
    played.outputs.iter().any(|output| output.contains(&quoted))
}

/// Plays `term` natively, checks that the hot store holds no volatile channel after it, and
/// certifies it.
async fn play_and_certify(label: &str, term: &str) -> Played {
    let played = native_play(term, AMPLE_LIMIT).await;
    assert!(
        played.residue.is_empty(),
        "{label}: the sweep leaves no volatile channel: {:?}",
        played.residue
    );
    let certification = certify(&played, term, AMPLE_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    played
}

/// Plays `term` on `chain`, checks the residue, and certifies it.
async fn chain_play(chain: &mut NativeChain, term: &str, label: &str) -> Played {
    let played = chain.play(term, label, AMPLE_LIMIT).await;
    assert!(
        played.residue.is_empty(),
        "{label}: the sweep leaves no volatile channel: {:?}",
        played.residue
    );
    let certification = certify(&played, term, AMPLE_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    played
}

// ─── 24 families with `temp` ───────────────────────────────────────────────────────────────────

async fn check_temp_family(outer: Outer, inner: Inner) {
    let label = format!("temp {}:{}", outer.name(), inner.name());
    let term = program_with(outer, inner, "temp");
    let played = play_and_certify(&label, &term).await;
    assert!(
        played.result.errors.is_empty(),
        "{label}: native play fails: {:?}",
        played.result.errors
    );
    for wanted in ["take", "peek", "join", outer.part_label()] {
        assert!(
            delivers(&played, wanted),
            "{label}: the program delivers {wanted}: {:?}",
            played.outputs
        );
    }
}

async fn check_temp_outer(outer: Outer) {
    for inner in Inner::ALL {
        check_temp_family(outer, inner).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_temp_replay_equal_hashmap() { check_temp_outer(Outer::HashMap).await }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_temp_replay_equal_pathmap() { check_temp_outer(Outer::PathMap).await }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_temp_replay_equal_array() { check_temp_outer(Outer::Array).await }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_temp_replay_equal_vector() { check_temp_outer(Outer::Vector).await }

/// The program of each family leaves records or waiters behind. With `default` they stay on
/// durable space channels; with `temp` the sweep removes them, and no space channel remains. Both
/// residues are read from the hot store after the evaluation and before the checkpoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_temp_swept_at_deploy_end() {
    for outer in Outer::ALL {
        for inner in Inner::ALL {
            let label = format!("{}:{}", outer.name(), inner.name());
            let durable = native_play(&program_with(outer, inner, "default"), AMPLE_LIMIT).await;
            assert!(durable.result.errors.is_empty(), "{label}: {:?}", durable.result.errors);
            assert!(
                !durable.durable.is_empty(),
                "{label}: the default program leaves records or waiters"
            );
            let ephemeral = play_and_certify(&label, &program_with(outer, inner, "temp")).await;
            assert!(
                ephemeral.durable.is_empty(),
                "{label}: a temp space writes no durable space channel: {:?}",
                ephemeral.durable
            );
        }
    }
}

/// A join of a `temp` space that never fires stays parked until the end of the deploy, with its
/// join entries; the sweep removes both (`SpaceTxnHost::sweep_volatile` removes join entries).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_temp_join_swept() {
    let term = space_program(
        "rho:space:hashmap:bag",
        r#""temp""#,
        r#"for (@p <- a & @q <- b) { out!(["join", p, q]) } | a!(1) | out!(["parked", 0])"#,
    );
    let played = play_and_certify("temp join", &term).await;
    assert!(played.result.errors.is_empty(), "{:?}", played.result.errors);
    assert!(delivers(&played, "parked"));
    assert!(!delivers(&played, "join"), "the join waits for b");
}

/// A `temp` cell that receives two records aborts with `CellOccupied`. The sweep runs on the
/// `Err` path, and native replay reproduces the failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_temp_failed_deploy_reverts() {
    let term = space_program(
        "rho:space:hashmap:cell",
        r#""temp""#,
        r#"for (@x <- b) { out!(["late", x]) } | a!(1) | a!(2)"#,
    );
    let played = play_and_certify("temp cell occupied", &term).await;
    assert_eq!(error_codes(&played.result.errors), vec!["CellOccupied"]);
}

/// Forty collections of one `temp` space in one deploy: the native export of the certification
/// meters every swept key (VR-10), and the play certifies.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_temp_many_volatile_keys_export() {
    let names: Vec<String> = (0..40).map(|i| format!("f{i}")).collect();
    let sends: Vec<String> = names
        .iter()
        .enumerate()
        .map(|(i, name)| format!("{name}!({i})"))
        .collect();
    let body = format!(
        "use space {{ new {} in {{ {} }} }} | out!([\"stored\", 40])",
        names.join(", "),
        sends.join(" | ")
    );
    let term = space_program("rho:space:hashmap:bag", r#""temp""#, &body);
    let played = play_and_certify("many volatile keys", &term).await;
    assert!(played.result.errors.is_empty(), "{:?}", played.result.errors);
    assert!(delivers(&played, "stored"));
}

// ─── Dangling ids ──────────────────────────────────────────────────────────────────────────────

const STASH: &str = "rrs-p7-stash";
const WAKE: &str = "rrs-p7-wake";
const REPLY: &str = "rrs-p7-reply";

/// A deploy that creates a `temp` queue, stores its handle and a name on the public channel
/// [`STASH`], parks a durable continuation on [`WAKE`] that sends on the name, and stores
/// `records` records on the name.
fn creation(records: usize) -> String {
    let sends: Vec<String> = (0..records).map(|i| format!("x!({i})")).collect();
    format!(
        r#"new factory(`rho:space:hashmap:queue`) in {{
  for (space <- factory!?("temp")) {{
    for (x <- space!?("gensym")) {{
      @"{STASH}"!(*space, *x) | for (_ <- @"{WAKE}") {{ x!(99) }} | {}
    }}
  }}
}}"#,
        sends.join(" | ")
    )
}

/// The space id of the `temp` space whose name the chain stored on [`STASH`].
async fn stashed_space(chain: &NativeChain) -> SpaceId {
    let stashed = chain.values(STASH).await;
    let [entry] = stashed.as_slice() else {
        panic!("one stash entry: {stashed:?}");
    };
    let name = entry.get(1).expect("the stash holds the name");
    match namespace::reified_id_of(name) {
        Some(ReifiedId::Name(space, _)) => {
            assert_eq!(space.durability, Durability::Ephemeral);
            space.sid
        }
        other => panic!("the stashed name is a space name: {other:?}"),
    }
}

/// Plays `term` on `chain` and checks that it fails with the one code `code` and names `sid`.
async fn expect_dangling(chain: &mut NativeChain, term: &str, label: &str, code: &str, sid: SpaceId) {
    let played = chain_play(chain, term, label).await;
    assert_eq!(
        error_codes(&played.result.errors),
        vec![code.to_string()],
        "{label}"
    );
    let hex = hex::encode(sid);
    assert!(
        messages(&played.result.errors)
            .iter()
            .any(|message| message.contains(&hex)),
        "{label}: the error names the space id {hex}: {:?}",
        played.result.errors
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_ephemeral_dangling_next_deploy() {
    let mut chain = NativeChain::new().await;
    let created = chain_play(&mut chain, &creation(2), "create").await;
    assert!(created.result.errors.is_empty(), "{:?}", created.result.errors);
    let sid = stashed_space(&chain).await;
    let peek = format!(r#"for (handle, name <<- @"{STASH}")"#);
    for (label, body, code) in [
        ("send on the name", "{ name!(2) }", "DanglingName"),
        (
            "receive on the name",
            "{ for (@v <- name) { Nil } }",
            "DanglingName",
        ),
        (
            "handle call",
            r#"{ for (_ <- handle!?("config")) { Nil } }"#,
            "DanglingHandle",
        ),
        ("use", "{ use handle { Nil } }", "DanglingHandle"),
        ("new in the space", "{ new y : handle in { Nil } }", "DanglingHandle"),
    ] {
        expect_dangling(&mut chain, &format!("{peek} {body}"), label, code, sid).await;
    }
    expect_dangling(
        &mut chain,
        &format!(r#"@"{WAKE}"!(Nil)"#),
        "parked continuation fires into the dead space",
        "DanglingName",
        sid,
    )
    .await;
    let agent = format!(
        r#"new gsa(`rho:space:getSpaceAgent`) in {{
  {peek} {{ for (@reply <- gsa!?(*handle)) {{ @"{REPLY}"!(reply) }} }}
}}"#
    );
    let played = chain_play(&mut chain, &agent, "getSpaceAgent").await;
    assert!(played.result.errors.is_empty(), "{:?}", played.result.errors);
    let replies = chain.values(REPLY).await;
    let [reply] = replies.as_slice() else {
        panic!("one reply: {replies:?}");
    };
    let debug = format!("{reply:?}");
    assert!(
        debug.contains("rho:space:error") && debug.contains("DanglingHandle"),
        "{debug}"
    );
    let detail = sid.iter().map(|byte| byte.to_string()).collect::<Vec<_>>();
    assert!(
        debug.contains("spaceId") && debug.contains(&format!("[{}]", detail.join(", "))),
        "the reply names the space id: {debug}"
    );
}

/// The phlo of the failing deploy of a dead name, for a dead space that held `records` records.
async fn dangling_usage(records: usize) -> u64 {
    let mut chain = NativeChain::new().await;
    chain_play(&mut chain, &creation(records), "create").await;
    let played = chain_play(
        &mut chain,
        &format!(r#"for (handle, name <<- @"{STASH}") {{ name!(2) }}"#),
        "dangling",
    )
    .await;
    assert_eq!(error_codes(&played.result.errors), vec!["DanglingName"]);
    played
        .result
        .native_phlo_usage
        .expect("native play reports its usage")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_ephemeral_dangling_charge_is_fixed() {
    assert_eq!(dangling_usage(1).await, dangling_usage(20).await);
}

// ─── Charges ───────────────────────────────────────────────────────────────────────────────────

/// The produce, consume and COMM events of `log`.
fn event_counts(log: &[Event]) -> (usize, usize, usize) {
    log.iter().fold((0, 0, 0), |(produces, consumes, comms), event| match event {
        Event::IoEvent(IOEvent::Produce(_)) => (produces + 1, consumes, comms),
        Event::IoEvent(IOEvent::Consume(_)) => (produces, consumes + 1, comms),
        Event::Comm(_) => (produces, consumes, comms + 1),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_ephemeral_cost_charged() {
    for (outer, inner) in [(Outer::HashMap, Inner::Bag), (Outer::PathMap, Inner::Queue)] {
        let label = format!("{}:{}", outer.name(), inner.name());
        let durable = native_play(&program_with(outer, inner, "default"), AMPLE_LIMIT).await;
        let ephemeral = play_and_certify(&label, &program_with(outer, inner, "temp")).await;
        let usage = ephemeral
            .result
            .native_phlo_usage
            .expect("native play reports its usage");
        assert!(usage > 0, "{label}: a temp program pays phlo");
        assert_eq!(
            event_counts(&ephemeral.log),
            event_counts(&durable.log),
            "{label}: temp operations emit the events of durable operations"
        );
    }
}

// ─── Entry points ──────────────────────────────────────────────────────────────────────────────

/// Native play and native replay sweep (`play_and_certify` checks the residue of play and the
/// post-state root of replay), and the exploratory entry `RhoRuntime::inj` sweeps too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_sweep_runs_at_each_entry_point_native() {
    let term = program_with(Outer::HashMap, Inner::Queue, "temp");
    play_and_certify("native play and replay", &term).await;
    let chain = NativeChain::new().await;
    chain.explore(&term).await.expect("the exploratory evaluation succeeds");
    assert!(chain.residue().await.is_empty(), "RhoRuntime::inj sweeps");
}

// ─── Qualifiers ────────────────────────────────────────────────────────────────────────────────

/// The constructor list `[qualifier, PARAMS?]` of `family` with its required params and `extra`.
fn constructor(family: SpaceFactoryUrn, qualifier: &str, extra: Vec<SendParam>) -> Par {
    let mut params = extra;
    match (family.outer, family.inner) {
        (SpaceOuter::Array, _) => {
            params.push(SendParam::Named("size".to_string(), new_gint_par(8, Vec::new(), false)))
        }
        (_, SpaceInner::VectorDb) => {
            params.push(SendParam::Named(
                "capacity".to_string(),
                new_gint_par(4, Vec::new(), false),
            ));
            params.push(SendParam::Named(
                "dimensions".to_string(),
                new_gint_par(2, Vec::new(), false),
            ));
        }
        _ => {}
    }
    let mut ctor = vec![new_gstring_par(qualifier.to_string(), Vec::new(), false)];
    if let Some(tuple) = builders::send_params(params).expect("valid params") {
        ctor.push(tuple);
    }
    new_elist_par(ctor, Vec::new(), false, None, Vec::new(), false)
}

#[test]
fn reducer_qualifier_matrix() {
    for family in SpaceFactoryUrn::all() {
        let Some(_) = constructor_schema(family) else {
            assert!(
                matches!(
                    FactoryConstructor::validate(family, &constructor(family, "temp", Vec::new())),
                    Err(SpaceError::FamilyUnavailable { .. })
                ),
                "{family}: a family without a schema has no factory"
            );
            continue;
        };
        assert!(!matches!(family.outer, SpaceOuter::HashSet), "{family}");
        for (qualifier, expected) in [
            ("default", SpaceQualifier::Default),
            ("temp", SpaceQualifier::Temp),
        ] {
            let validated = FactoryConstructor::validate(family, &constructor(family, qualifier, Vec::new()))
                .unwrap_or_else(|error| panic!("{family} {qualifier}: {error:?}"));
            assert_eq!(validated.qualifier, expected, "{family}");
        }
        assert_eq!(
            FactoryConstructor::validate(family, &constructor(family, "seq", Vec::new())),
            Err(SpaceError::QualifierUnavailable("seq".to_string())),
            "{family}"
        );
        if matches!(family.inner, SpaceInner::VectorDb) {
            let mirror = || {
                vec![SendParam::Named(
                    "mirror".to_string(),
                    new_gstring_par("chroma".to_string(), Vec::new(), false),
                )]
            };
            assert!(
                FactoryConstructor::validate(family, &constructor(family, "default", mirror()))
                    .is_ok(),
                "{family}: a default space takes a mirror"
            );
            assert_eq!(
                FactoryConstructor::validate(family, &constructor(family, "temp", mirror())),
                Err(SpaceError::InvalidParam {
                    key: "mirror".to_string()
                }),
                "{family}: a temp space takes no mirror"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_handle_qualifier_method_temp() {
    let term = format!(
        r#"new factory(`rho:space:hashmap:bag`) in {{
  for (space <- factory!?("temp")) {{
    for (@qualifier <- space!?("qualifier")) {{ @"{OUT}"!(qualifier) }}
  }}
}}"#
    );
    let played = play_and_certify("qualifier method", &term).await;
    assert!(played.result.errors.is_empty(), "{:?}", played.result.errors);
    assert_eq!(
        played.out,
        vec![vec![new_gstring_par("temp".to_string(), Vec::new(), false)]]
    );
}

// ─── The FIPS example ──────────────────────────────────────────────────────────────────────────

/// The elements after the label of each output list of `played` whose first element is the
/// string `label`.
fn labelled(played: &Played, label: &str) -> Vec<Vec<Par>> {
    let label = new_gstring_par(label.to_string(), Vec::new(), false);
    played
        .out
        .iter()
        .filter_map(|pars| match pars.as_slice() {
            [value] => match value.exprs.as_slice() {
                [Expr {
                    expr_instance: Some(ExprInstance::EListBody(list)),
                }] => Some(list.ps.clone()),
                _ => None,
            },
            _ => None,
        })
        .filter_map(|elements| match elements.split_first() {
            Some((first, rest)) if *first == label => Some(rest.to_vec()),
            _ => None,
        })
        .collect()
}

/// The example code of the FIPS (Reifying RSpaces) with a `temp` PathMap space and without
/// theories. Names of both spaces come from one `new` in `use space_1`. A forged name goes to
/// `space_1`, and a same-space join of the `temp` space precedes a sequential receive. The
/// agents of both spaces equal their FIPS aliases, and the handles name the same spaces inside
/// and outside the `new` block. The deploy certifies, and the sweep removes the `temp` space.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_fips_example_temp_pathmap() {
    let term = format!(
        r#"new HMB(`rho:space:HashMapBagSpace`), PM(`rho:space:PathMapSpace`),
    getSpaceAgent(`rho:space:getSpaceAgent`), assertEqual(`rho:assert:assertEqual`), out in {{
  for (space_1 <- HMB!?("default"); space_2 <- PM!?("temp")) {{
    use space_1 {{
      new x_1 : space_2, x_2 : space_2, x_3, x_4 in {{
        x_1!(1) | x_2!(2) | x_3!(3) | x_4!(4) |
        @{{*x_3 | *x_4}}!(5) |
        for (@y_1 <- x_1 & @y_2 <- x_2; @y_3 <- x_3) {{ out!(["join", y_1, y_2, y_3]) }} |
        for (@f <- @{{*x_3 | *x_4}}) {{ out!(["forged", f]) }} |
        for (@agent_1 <- getSpaceAgent!?(*space_1); @agent_2 <- getSpaceAgent!?(*space_2)) {{
          for (@hmb <- assertEqual!?(agent_1, *HMB); @pm <- assertEqual!?(agent_2, *PM)) {{
            out!(["agents", hmb, pm])
          }}
        }} |
        out!(["inside", *space_1, *space_2])
      }} |
      out!(["outside", *space_1, *space_2])
    }}
  }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#
    );
    let played = play_and_certify("FIPS example", &term).await;
    assert!(played.result.errors.is_empty(), "{:?}", played.result.errors);
    let int = |value: i64| new_gint_par(value, Vec::new(), false);
    let yes = new_gbool_par(true, Vec::new(), false);
    assert_eq!(labelled(&played, "join"), vec![vec![int(1), int(2), int(3)]]);
    assert_eq!(labelled(&played, "forged"), vec![vec![int(5)]]);
    assert_eq!(labelled(&played, "agents"), vec![vec![yes.clone(), yes]]);
    let inside = labelled(&played, "inside");
    assert_eq!(inside.len(), 1, "{:?}", played.outputs);
    assert_eq!(
        inside,
        labelled(&played, "outside"),
        "the handles name the same spaces inside and outside the new block"
    );
    assert!(!played.durable.is_empty(), "the default space keeps the record of x_4");
}

// ─── Property ──────────────────────────────────────────────────────────────────────────────────

/// One generated `temp` program: a family, extra records on `e` and extra waiters on `e` whose
/// pattern matches nothing.
#[derive(Clone, Copy, Debug)]
struct SweepCase {
    outer: Outer,
    inner: Inner,
    records: usize,
    waiters: usize,
}

impl SweepCase {
    /// The family program of `program_with` with the extra records and waiters beside its
    /// parts, inside the scope of the names `a` to `e`.
    fn term(self) -> String {
        let records = (0..self.records).map(|i| format!("e!({})", 1_000 + i));
        let waiters = (0..self.waiters).map(|_| r#"for (@"never" <- e) { Nil }"#.to_string());
        let mut parts = vec![
            self.inner.part().to_string(),
            JOIN.to_string(),
            self.outer.part().to_string(),
        ];
        parts.extend(records.chain(waiters));
        space_program(
            &format!("rho:space:{}:{}", self.outer.name(), self.inner.name()),
            &self.outer.ctor("temp"),
            &parts.join(" |\n"),
        )
    }
}

fn sweep_cases() -> impl Strategy<Value = SweepCase> {
    let outer = prop_oneof![
        Just(Outer::HashMap),
        Just(Outer::PathMap),
        Just(Outer::Array),
        Just(Outer::Vector)
    ];
    let inner = prop_oneof![
        Just(Inner::Bag),
        Just(Inner::Queue),
        Just(Inner::Stack),
        Just(Inner::Set),
        Just(Inner::Cell),
        Just(Inner::PriorityQueue)
    ];
    (outer, inner, 0usize..3, 0usize..3).prop_map(|(outer, inner, records, waiters)| SweepCase {
        outer,
        inner,
        records,
        waiters,
    })
}

/// The host-work budget of the property: 2^40 units in each dimension. Native replay also pays
/// the journal check, the trace binding and the export, so a PathMap program with extra records
/// and waiters exceeds the v6 `VerificationBytes` limit of 2^28 in replay with either qualifier.
/// Measured for `pathmap:set` with two records and one waiter: 281,191,770 bytes with `temp` and
/// 284,583,181 with `default`. The property checks the sweep, not the v6 limits.
fn sweep_host_work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1 << 40)))
}

#[test]
fn prop_sweep_complete() {
    check(runtime_config(), sweep_cases(), |case| {
        let term = case.term();
        tokio_runtime().block_on(async {
            let played = native_play_with(&term, AMPLE_LIMIT, sweep_host_work()).await;
            if !played.residue.is_empty() {
                return Err(TestCaseError::fail(format!(
                    "{case:?}: volatile residue {:?}",
                    played.residue
                )));
            }
            let certification =
                certify_with(&played, &term, AMPLE_LIMIT, sweep_host_work()).await;
            assert_reproduces(&format!("{case:?}"), &played, &certification);
            Ok(())
        })
    });
}
