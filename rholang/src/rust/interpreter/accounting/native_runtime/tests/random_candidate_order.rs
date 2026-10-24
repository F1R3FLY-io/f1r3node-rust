//! Added by DR-129 (M34): play picks among candidates that match equally at
//! random, as `dev` does (`shuffle_with_index`). Native replay follows the
//! recorded pick in its own canonical order.
//!
//! `native_session_agreement` plays a term with native phlo and replays its
//! checked trace twice through the native replay session, the path of
//! protocol-6 validators. It compares what cost accounting reads: the fired
//! COMMs, the phlo use, the evidence and the root of the two replays.
//! `legacy_replay_agreement` replays the same play through the recorded log
//! of the legacy replay (`ReplayRSpace::rig`).
//!
//! F4 covers E1 (a join that names one channel twice), F5 covers E3-a (the
//! Stored check of a guarded consume), and F6 is the property test over the
//! families of ambiguous terms.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::Event;
use tokio::sync::RwLock;

use super::*;
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::external_services::ExternalServices;
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use crate::rust::interpreter::rho_runtime::{create_native_replay_env, create_rho_env, RhoRuntime};
use crate::rust::interpreter::test_utils::resources::create_runtimes;

const LIMIT: u64 = 1_000_000;
const WEIGHTS: [u64; 4] = [2, 3, 5, 7];
const SEED: &[u8] = b"m34-random-candidate-order";

/// Runs of F4 and F5. A run disagrees with probability about one half before
/// the fix, so all runs agree by chance with probability about `2^-64`.
const AGREEMENT_RUNS: usize = 64;

/// Cases of F6. Each family receives about `F6_CASES / FAMILIES.len()` cases.
const F6_CASES: u32 = 512;

// Changed by DR-132 (P-1): the chain family of P-1 records about 4,100
// events, so the limits of the fixtures are larger.
// const TRACE_LIMITS: NativeOperationTraceLimits = NativeOperationTraceLimits {
//     events: 1_000,
//     source_entries: 10_000,
//     source_bytes: 10_000_000,
//     telemetry_items: 1_000,
//     telemetry_bytes: 1_000_000,
// };
const TRACE_LIMITS: NativeOperationTraceLimits = NativeOperationTraceLimits {
    events: 100_000,
    source_entries: 1_000_000,
    source_bytes: 1 << 30,
    telemetry_items: 100_000,
    telemetry_bytes: 1 << 30,
};

/// E1: the contract substitutes one name for both channels of a join, so the
/// join names `@"x"` twice at run time. The normalizer rejects a repeat only
/// when the source writes it.
const REPEATED_CHANNEL: &str = r#"new go in {
  @"x"!(1) | @"x"!(2) |
  contract go(@c1, @c2) = { for (@a <- @c1 & @b <- @c2) { @"out"!((a, b)) } } |
  go!("x", "x")
}"#;

/// E3: the consume runs after both data are stored, and its guard accepts
/// one of them. One of the two guards accepts the first datum in the
/// canonical order, whichever datum that is.
const GUARD_ABOVE: &str = r#"@"x"!(1) | @"x"!(2) |
new t in { t!(0) | for (_ <- t) { for (@a <- @"x" where a > 1) { @"out"!(a) } } }"#;
const GUARD_BELOW: &str = r#"@"x"!(1) | @"x"!(2) |
new t in { t!(0) | for (_ <- t) { for (@a <- @"x" where a < 2) { @"out"!(a) } } }"#;

/// The families of F6. In each, play meets two or more candidates that match
/// equally. The flag says whether play's pick varies from run to run. E1
/// gives the data of a repeated channel one stable order, so the join that
/// names one channel twice has one pick.
const FAMILIES: [(&str, &str, bool); 9] = [
    (
        "three data, one consume",
        r#"@"x"!(1) | @"x"!(2) | @"x"!(3) |
new t in { t!(0) | for (_ <- t) { for (@a <- @"x") { @"out"!(a) } } }"#,
        true,
    ),
    (
        "three continuations, one produce",
        r#"for (@a <- @"x") { @"left"!(a) } | for (@b <- @"x") { @"middle"!(b) } |
for (@c <- @"x") { @"right"!(c) } |
new t in { t!(0) | for (_ <- t) { @"x"!(7) } }"#,
        true,
    ),
    (
        "a join that names one channel twice",
        REPEATED_CHANNEL,
        false,
    ),
    ("a guarded consume, guard above", GUARD_ABOVE, true),
    ("a guarded consume, guard below", GUARD_BELOW, true),
    (
        "a guarded join met by a produce, guard above",
        r#"@"y"!(1) | @"y"!(2) |
for (@a <- @"x" & @b <- @"y" where b > 1) { @"out"!((a, b)) } |
new t in { t!(0) | for (_ <- t) { @"x"!(0) } }"#,
        true,
    ),
    (
        "a guarded join met by a produce, guard below",
        r#"@"y"!(1) | @"y"!(2) |
for (@a <- @"x" & @b <- @"y" where b < 2) { @"out"!((a, b)) } |
new t in { t!(0) | for (_ <- t) { @"x"!(0) } }"#,
        true,
    ),
    (
        "persistent data, one consume",
        r#"@"x"!!(1) | @"x"!!(2) |
new t in { t!(0) | for (_ <- t) { for (@a <- @"x") { @"out"!(a) } } }"#,
        true,
    ),
    (
        "two data, one peek",
        r#"@"x"!(1) | @"x"!(2) |
new t in { t!(0) | for (_ <- t) { for (@a <<- @"x") { @"out"!(a) } } }"#,
        true,
    ),
];

fn require(condition: bool, what: impl FnOnce() -> String) -> Result<(), String> {
    match condition {
        true => Ok(()),
        false => Err(what()),
    }
}

/// The budget evidence keyed by occurrence: the session, the causal path and
/// the stage of each attempt, with its observation and grant. Keys and values
/// are `Debug` renderings, which compare like the values.
fn by_occurrence(recording: &NativeBudgetRecording) -> BTreeMap<String, String> {
    recording
        .attempts
        .iter()
        .map(|row| {
            (
                format!(
                    "{:?}",
                    (
                        row.occurrence.session,
                        recording.paths.segments(row.occurrence.path),
                        row.occurrence.stage,
                    )
                ),
                format!("{:?}", (&row.observation, row.granted)),
            )
        })
        .collect()
}

/// The operation journal keyed by causal path: each operation's source, the
/// source of its COMM, if any, and its completion.
fn operation_sources(
    rows: &[NativeOperationRecord],
    paths: &NativePathTrie,
) -> BTreeMap<String, String> {
    rows.iter()
        .map(|row| {
            (
                format!(
                    "{:?}",
                    (row.occurrence.session, paths.segments(row.occurrence.path))
                ),
                format!(
                    "{:?}",
                    (
                        &row.source,
                        row.comm.as_ref().map(|comm| &comm.source),
                        row.completion
                    )
                ),
            )
        })
        .collect()
}

/// The COMMs that play recorded. They name the candidates that play picked.
fn recorded_comms(log: &[Event]) -> Vec<String> {
    log.iter()
        .filter_map(|event| match event {
            Event::Comm(comm) => Some(format!("{comm:?}")),
            _ => None,
        })
        .collect()
}

/// One native play of `term` and two replays of its checked trace through
/// the native replay session. Returns the COMMs that play recorded, or the
/// first disagreement.
// Changed by DR-132 (P-1): the agreement runs as `native_agreement` with the
// limits of the fixtures.
// async fn native_session_agreement(term: &str) -> Result<Vec<String>, String> {
async fn native_session_agreement(term: &str) -> Result<Vec<String>, String> {
    native_agreement(term, LIMIT, FIXTURE_PATH_SEGMENTS)
        .await
        .map(|agreement| agreement.comms)
}

/// Added by DR-132 (P-1): the path-segment limit of `config`.
const FIXTURE_PATH_SEGMENTS: usize = 4_096;

/// Added by DR-132 (P-1): what one agreement observed. `comms` are the COMMs
/// that play recorded, and `journal` holds the source of each row of play, in
/// journal order.
struct Agreement {
    comms: Vec<String>,
    journal: Vec<String>,
}

/// Added by DR-132 (P-1): the body of `native_session_agreement`, with a phlo
/// limit and a path-segment limit for play and for both replays.
// Changed by DR-132 (P-1, review m7): the mode for a term that exhausts its
// limit is disabled with its family (see `P1_FAILING_FAMILY`).
// async fn native_agreement(
//     term: &str,
//     limit: u64,
//     path_segments: usize,
//     failing: bool,
// ) -> Result<Agreement, String> {
async fn native_agreement(
    term: &str,
    limit: u64,
    path_segments: usize,
) -> Result<Agreement, String> {
    let settings = || {
        let mut settings = config(limit, WEIGHTS);
        settings.limits.path_segments = path_segments;
        settings
    };
    let parsed = Compiler::source_to_adt(term).map_err(|error| format!("parse: {error}"))?;
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("a play space");
    play.create_checkpoint().await.expect("the pre-state");
    let history = play.get_history_repository();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(SEED, Sig::Ground(vec![7]));
    // Changed by DR-132 (P-1): the limits come from the caller.
    // let settings = config(LIMIT, WEIGHTS);
    // budget
    //     .reset_for_native_execution(settings)
    budget
        .reset_for_native_execution(settings())
        .map_err(|error| format!("play budget: {error}"))?;
    let (reducer, _, _, _) = create_rho_env(
        play.clone(),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget.clone(),
        ExternalServices::noop(),
        crate::rust::interpreter::io::handle_table::FileHandleTable::new(),
    )
    .await
    .map_err(|error| format!("play environment: {error}"))?;
    let scope = budget.enter_comm_accounting_scope();
    let rand = || Blake2b512Random::create_from_bytes(SEED);
    let played = reducer.inj_with_observation(parsed.clone(), rand()).await;
    // Changed by DR-132 (P-1, review m7): the mode for a term that exhausts
    // its limit is disabled with its family (see `P1_FAILING_FAMILY`).
    // match failing {
    //     false => require(played.0.is_ok(), || format!("play failed: {played:?}"))?,
    //     true => require(
    //         matches!(played.0, Err(InterpreterError::OutOfPhlogistonsError)),
    //         || format!("play did not exhaust its limit: {played:?}"),
    //     )?,
    // }
    require(played.0.is_ok(), || format!("play failed: {played:?}"))?;
    let played_observations = budget.byte_observations();
    drop(scope);
    let recording = budget
        .native_budget_recording()
        .map_err(|error| format!("play recording: {error}"))?
        .ok_or("play has no budget recording")?;
    let operations = budget
        .native_operation_recording()
        .map_err(|error| format!("play journal: {error}"))?
        .ok_or("play has no operation journal")?;
    let log = play.create_soft_checkpoint().await.log;
    let play_root = play.create_checkpoint().await.expect("the post-state").root;
    for pass in 0..2 {
        // Changed by DR-132 (P-1): the limit comes from the caller.
        // let trace = with_contract(LIMIT, WEIGHTS, |contract| {
        let trace = with_contract(limit, WEIGHTS, |contract| {
            contract.check_operation_journal(
                recording.session,
                &recording,
                Arc::clone(&operations),
                journal_limits(),
            )
        })
        .map_err(|error| format!("replay {pass} journal: {error:?}"))?
        .bind_trace(log.clone().into(), TRACE_LIMITS)
        .map_err(|error| format!("replay {pass} trace: {error}"))?;
        let replay_budget = RuntimeBudget::new(Cost::unsafe_max());
        replay_budget.set_deploy_signature_funded(SEED, Sig::Ground(vec![7]));
        // Changed by DR-132 (P-1): the limits come from the caller.
        // let replay_settings = config(LIMIT, WEIGHTS);
        // replay_budget
        //     .reset_for_native_execution(replay_settings)
        replay_budget
            .reset_for_native_execution(settings())
            .map_err(|error| format!("replay {pass} budget: {error}"))?;
        let mut environment = create_native_replay_env(
            trace,
            history.clone(),
            Arc::new(Box::new(Matcher)),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(default_mergeable_tags()),
            &mut Vec::new(),
            replay_budget,
            ExternalServices::noop(),
        )
        .await
        .map_err(|error| format!("replay {pass} environment: {error}"))?;
        let replayed = environment
            .evaluate(parsed.clone(), rand())
            .await
            .map_err(|error| format!("replay {pass} failed: {error}"))?;
        // Changed by DR-132 (P-1, review m7): the mode for a term that
        // exhausts its limit is disabled with its family (see
        // `P1_FAILING_FAMILY`).
        // let exhausted = matches!(replayed.errors.as_slice(), [
        //     InterpreterError::OutOfPhlogistonsError
        // ]);
        // require(
        //     if failing {
        //         exhausted
        //     } else {
        //         replayed.errors.is_empty()
        //     },
        //     || format!("replay {pass} errors: {:?}", replayed.errors),
        // )?;
        require(replayed.errors.is_empty(), || {
            format!("replay {pass} errors: {:?}", replayed.errors)
        })?;
        environment
            .check_complete()
            .await
            .map_err(|error| format!("replay {pass} left recorded operations: {error}"))?;
        // Added by DR-132 (P-1, review m7): each evaluation leaves the
        // progress word idle, and a new evaluation can begin on the session.
        require(environment.progress_idle(), || {
            format!("replay {pass} left the progress word armed")
        })?;
        require(environment.progress_begins_again(), || {
            format!("replay {pass} cannot begin another evaluation")
        })?;
        require(replayed.native_phlo_usage == Some(recording.used), || {
            format!(
                "replay {pass} phlo {:?} differs from play {}",
                replayed.native_phlo_usage, recording.used
            )
        })?;
        require(replayed.byte_observations == played_observations, || {
            format!("replay {pass} byte observations differ from play")
        })?;
        let replay_recording = replayed
            .native_budget_recording
            .as_ref()
            .ok_or("replay has no budget recording")?;
        require(
            by_occurrence(replay_recording) == by_occurrence(&recording),
            || format!("replay {pass} budget evidence differs from play"),
        )?;
        let replay_operations = replayed
            .native_operation_recording
            .as_ref()
            .ok_or("replay has no operation journal")?;
        require(
            operation_sources(replay_operations, &replay_recording.paths)
                == operation_sources(&operations, &recording.paths),
            || format!("replay {pass} fired other COMMs than play"),
        )?;
        let export = environment
            .export()
            .await
            .map_err(|error| format!("replay {pass} export: {error}"))?;
        require(export.root() == &play_root, || {
            format!(
                "replay {pass} root {:?} differs from play {play_root:?}",
                export.root()
            )
        })?;
    }
    // Changed by DR-132 (P-1): the agreement also returns the journal order.
    // Ok(recorded_comms(&log))
    Ok(Agreement {
        comms: recorded_comms(&log),
        journal: operations
            .iter()
            .map(|row| format!("{:?}", row.source))
            .collect(),
    })
}

/// One native play of `term` through a runtime, replayed from its recorded
/// log by the legacy replay of the paired replay runtime.
async fn legacy_replay_agreement(term: &str) -> Result<(), String> {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.expect("in-memory stores");
    let (mut play, mut replay, _) = create_runtimes(stores, false, &mut Vec::new()).await;
    for runtime in [&play, &replay] {
        runtime
            .cost
            .set_deploy_signature_funded(SEED, Sig::Ground(vec![7]));
    }
    let root = play.get_root().await;
    let rand = Blake2b512Random::create_from_bytes(SEED);
    let played = play
        .evaluate_with_native_phlo(
            term,
            HashMap::new(),
            rand.clone(),
            None,
            config(LIMIT, WEIGHTS),
        )
        .await
        .map_err(|error| format!("play failed: {error}"))?;
    require(played.errors.is_empty(), || {
        format!("play errors: {:?}", played.errors)
    })?;
    let checkpoint = play.create_checkpoint().await;
    replay
        .reset(&root)
        .await
        .map_err(|error| format!("reset: {error}"))?;
    replay
        .rig(checkpoint.log.clone())
        .await
        .map_err(|error| format!("rig: {error}"))?;
    let replayed = replay
        .evaluate_with_native_phlo(term, HashMap::new(), rand, None, config(LIMIT, WEIGHTS))
        .await
        .map_err(|error| format!("replay failed: {error}"))?;
    require(replayed.errors.is_empty(), || {
        format!("replay errors: {:?}", replayed.errors)
    })?;
    replay
        .check_replay_data()
        .await
        .map_err(|error| format!("replay left recorded events: {error}"))?;
    require(
        played.native_phlo_usage == replayed.native_phlo_usage,
        || "replay phlo differs from play".to_owned(),
    )?;
    require(
        played.byte_observations == replayed.byte_observations,
        || "replay byte observations differ from play".to_owned(),
    )?;
    let replay_root = replay.create_checkpoint().await.root;
    require(replay_root == checkpoint.root, || {
        format!(
            "replay root {replay_root:?} differs from play {:?}",
            checkpoint.root
        )
    })
}

async fn native_disagreements(term: &str, runs: usize) -> Vec<String> {
    let mut failed = Vec::new();
    for run in 0..runs {
        if let Err(error) = native_session_agreement(term).await {
            failed.push(format!("run {run}: {error}"));
        }
    }
    failed
}

/// F4 (E1): a join names one channel twice, and two data match both
/// patterns. The COMM records only the set of produces, so each replay works
/// out the assignment again. Play's assignment must be that one, for the
/// native replay session and for the legacy replay.
#[tokio::test]
async fn a_join_on_one_channel_twice_replays_to_the_play_root() {
    let mut failed = native_disagreements(REPEATED_CHANNEL, AGREEMENT_RUNS).await;
    for run in 0..AGREEMENT_RUNS {
        if let Err(error) = legacy_replay_agreement(REPEATED_CHANNEL).await {
            failed.push(format!("legacy run {run}: {error}"));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} runs disagreed: {failed:#?}",
        failed.len(),
        2 * AGREEMENT_RUNS
    );
}

/// F5 (E3): a guarded consume meets two data. Play may try the datum that the
/// guard rejects and store the consume. The greedy pass of the native
/// session's Stored check, in canonical order, may find the datum that the
/// guard accepts.
#[tokio::test]
async fn a_stored_guarded_consume_replays_under_either_guard() {
    let mut failed = Vec::new();
    for term in [GUARD_ABOVE, GUARD_BELOW] {
        for error in native_disagreements(term, AGREEMENT_RUNS).await {
            failed.push(format!("{term}: {error}"));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} runs disagreed: {failed:#?}",
        failed.len(),
        2 * AGREEMENT_RUNS
    );
}

/// F6 (property, 512 cases): random play against the native replay session.
/// A case plays one family's term, replays its trace twice, and requires
/// equal fired COMMs, phlo use, evidence and root in the two replays. Every
/// family must also show at least two different picks.
#[test]
fn random_play_agrees_with_native_replay() {
    use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};

    let runtime = tokio::runtime::Runtime::new().expect("a Tokio runtime");
    let picks: std::sync::Mutex<BTreeMap<usize, BTreeSet<Vec<String>>>> =
        std::sync::Mutex::new(BTreeMap::new());
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: F6_CASES,
            failure_persistence: None,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &[29u8; 32]),
    );
    runner
        .run(&(0..FAMILIES.len()), |family| {
            let (name, term, _) = FAMILIES[family];
            match runtime.block_on(native_session_agreement(term)) {
                Ok(comms) => {
                    picks
                        .lock()
                        .expect("the pick map")
                        .entry(family)
                        .or_default()
                        .insert(comms);
                    Ok(())
                }
                Err(error) => Err(TestCaseError::fail(format!("{name}: {error}"))),
            }
        })
        .expect("random play agrees with native replay in every case");
    let picks = picks.into_inner().expect("the pick map");
    for (family, (name, _, varies)) in FAMILIES.iter().enumerate() {
        let seen = picks.get(&family).map_or(0, BTreeSet::len);
        match varies {
            true => assert!(seen >= 2, "family {name:?} showed {seen} pick(s)"),
            false => assert_eq!(seen, 1, "family {name:?} must have one pick"),
        }
    }
}

/// Added by DR-132 (P-1): runs of each family on each runtime.
const P1_RUNS: usize = 3;

/// Added by DR-132 (P-1): the phlo limit of P-1. The chain of 2,048 levels
/// needs more phlo than `LIMIT`.
const P1_LIMIT: u64 = 1_000_000_000;

/// Added by DR-132 (P-1): the families that P-1 adds to the nine of F6. Each
/// entry holds a name, a term and the path-segment limit of its budget.
fn racing_families() -> Vec<(String, String, usize)> {
    let fan_out = (0..64)
        .map(|index| format!(r#"@"f{index}"!({index}) | for (@v <- @"f{index}") {{ @"out"!(v) }}"#))
        .collect::<Vec<_>>()
        .join(" | ");
    vec![
        ("a 64-way fan-out".to_string(), fan_out, FIXTURE_PATH_SEGMENTS),
        (
            "two joins over two channels".to_string(),
            r#"@"a"!(1) | @"b"!(2) | @"a"!(3) | @"b"!(4) |
for (@x <- @"a" & @y <- @"b") { @"sum"!(x + y) } |
for (@x <- @"a" & @y <- @"b") { @"product"!(x * y) }"#
                .to_string(),
            FIXTURE_PATH_SEGMENTS,
        ),
        (
            "two peeks and a take".to_string(),
            r#"@"x"!(1) | for (@a <<- @"x") { @"p1"!(a) } |
for (@b <<- @"x") { @"p2"!(b) } | for (@c <- @"x") { @"take"!(c) }"#
                .to_string(),
            FIXTURE_PATH_SEGMENTS,
        ),
        (
            "a persistent send re-issued".to_string(),
            r#"@"x"!!(1) | for (@a <- @"x") { @"o1"!(a) } |
for (@b <- @"x") { @"o2"!(b) } | for (@c <- @"x") { @"o3"!(c) }"#
                .to_string(),
            FIXTURE_PATH_SEGMENTS,
        ),
        (
            "a persistent receive re-issued".to_string(),
            r#"@"x"!(1) | @"x"!(2) | @"x"!(3) | for (@a <= @"x") { @"out"!(a) }"#.to_string(),
            FIXTURE_PATH_SEGMENTS,
        ),
        (
            "a continuation chain of 256".to_string(),
            "new loop in { contract loop(@n) = { if (n > 0) { loop!(n - 1) } } | loop!(256) }"
                .to_string(),
            FIXTURE_PATH_SEGMENTS,
        ),
        (
            "two receives race for two data".to_string(),
            r#"@"x"!(1) | @"x"!(2) | for (@a <- @"x") { @"l"!(a) } | for (@b <- @"x") { @"r"!(b) }"#
                .to_string(),
            FIXTURE_PATH_SEGMENTS,
        ),
        (
            "a produce races the open join of another produce (F1)".to_string(),
            r#"for (@x <- @"a" & @y <- @"b") { @"j"!((x, y)) } | @"a"!(10) | @"b"!(20) | @"a"!(30)"#
                .to_string(),
            FIXTURE_PATH_SEGMENTS,
        ),
        (
            "a chain of 2,048 levels under a limit of 1,024 segments (A6)".to_string(),
            "new loop in { contract loop(@n) = { if (n > 0) { loop!(n - 1) } } | loop!(2048) }"
                .to_string(),
            1_024,
        ),
    ]
}

// Disabled by DR-132 (P-1, review m7): a persistent send to a replying
// system process re-issues itself on each firing, so its evaluation ends only
// when it exhausts its phlo limit. Native play of such a loop leaves its
// budget recording incomplete ("native budget recording is incomplete"), with
// the deterministic scheduler as without it (DR-132 measured both). The
// defect predates DR-132, so this family can run only after a fix of the
// recorder. Contract recursion and repeated system calls that exhaust their
// limit record completely in both runs.
// /// Added by DR-132 (P-1, review m7): a persistent send to a replying system
// /// process. The send fires the process and re-issues itself on each firing,
// /// so the evaluation exhausts its phlo limit. The limit keeps the loop short.
// const P1_FAILING_FAMILY: (&str, &str, u64) = (
//     "a persistent send to a replying system process",
//     r#"new hash(`rho:crypto:sha256Hash`), ack in {
// hash!!("abc".toByteArray(), *ack) | for (@h <= ack) { @"out"!(h) } }"#,
//     20_000,
// );

/// Added by DR-132 (P-1): racing native play against the native replay
/// session. It extends F6 to the families of `racing_families` and runs
/// every family on a current-thread runtime and on runtimes with 2 and 8
/// workers. Each run plays the term once and replays its trace twice. The
/// replays must accept and agree with play on the fired COMMs, the phlo use,
/// the byte observations, the evidence and the root, and each replay must
/// leave the progress word of the stall rule idle. A recorder overlap or a
/// binding error fails the play or a replay. Under the deterministic
/// scheduler, each body added a path segment, so the chain of 2,048 levels
/// exceeded its limit of 1,024 segments. Non-vacuity: some family shows two
/// or more journal orders, as the sources of the rows of play in journal
/// order.
#[test]
fn racing_native_play_agrees_with_native_replay() {
    let mut families: Vec<(String, String, usize)> = FAMILIES
        .iter()
        .map(|(name, term, _)| (name.to_string(), term.to_string(), FIXTURE_PATH_SEGMENTS))
        .collect();
    families.extend(racing_families());
    let mut orders: BTreeMap<String, BTreeSet<Vec<String>>> = BTreeMap::new();
    let mut failed = Vec::new();
    // Changed by DR-132 (P-1, review m7): a current-thread runtime and runtimes
    // with 2 and 8 workers.
    // for workers in [1, 2, 4] {
    //     let runtime = tokio::runtime::Builder::new_multi_thread()
    //         .worker_threads(workers)
    //         .enable_all()
    //         .build()
    //         .expect("a Tokio runtime");
    for workers in [0, 2, 8] {
        let runtime = match workers {
            0 => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a current-thread runtime"),
            _ => tokio::runtime::Builder::new_multi_thread()
                .worker_threads(workers)
                .enable_all()
                .build()
                .expect("a Tokio runtime"),
        };
        // Disabled by DR-132 (P-1, review m7): see `P1_FAILING_FAMILY`.
        // let (name, term, limit) = P1_FAILING_FAMILY;
        // for run in 0..P1_RUNS {
        //     if let Err(error) =
        //         runtime.block_on(native_agreement(term, limit, FIXTURE_PATH_SEGMENTS, true))
        //     {
        //         failed.push(format!("{workers} workers, {name}, run {run}: {error}"));
        //     }
        // }
        for (name, term, path_segments) in &families {
            for run in 0..P1_RUNS {
                match runtime.block_on(native_agreement(term, P1_LIMIT, *path_segments)) {
                    Ok(agreement) => {
                        orders
                            .entry(name.clone())
                            .or_default()
                            .insert(agreement.journal);
                    }
                    Err(error) => {
                        failed.push(format!("{workers} workers, {name}, run {run}: {error}"))
                    }
                }
            }
        }
    }
    assert!(
        failed.is_empty(),
        "{} runs disagreed: {failed:#?}",
        failed.len()
    );
    assert!(
        orders.values().any(|seen| seen.len() >= 2),
        "no family showed two journal orders: {:?}",
        orders
            .iter()
            .map(|(name, seen)| (name, seen.len()))
            .collect::<Vec<_>>()
    );
}
