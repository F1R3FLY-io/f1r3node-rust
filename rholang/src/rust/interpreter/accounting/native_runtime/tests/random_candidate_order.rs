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
//! F4 covers E1 (a join that names one channel twice). F5 covers the Stored
//! check of a guarded consume, and F6 is the property test over the families
//! of ambiguous terms. Changed by the merge of feature/cost-accounted-rho
//! (decision 6): the Stored check has no commit-veto skip (E3-a) now, so F5
//! no longer covers E3-a.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
// Added by the merge of feature/cost-accounted-rho (finding RT-03).
use models::rust::utils::new_gstring_par;
// Added by the merge of feature/cost-accounted-rho (finding RT-03).
use rspace_plus_plus::rspace::hashing::stable_hash_provider::channel_hash_for_epoch;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
// Changed by the merge of feature/cost-accounted-rho (finding RT-03): the
// anti-vacuity check of F5 reads the produces of play's log.
// use rspace_plus_plus::rspace::trace::event::Event;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
use tokio::sync::RwLock;

use super::*;
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::external_services::ExternalServices;
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use crate::rust::interpreter::rho_runtime::{create_native_replay_env, create_rho_env, RhoRuntime};
// Changed by the merge of feature/cost-accounted-rho (finding R1-33): the
// runtimes have the codec epoch of the native harnesses (`native_runtimes`).
// use crate::rust::interpreter::test_utils::resources::create_runtimes;

const LIMIT: u64 = 1_000_000;
const WEIGHTS: [u64; 4] = [2, 3, 5, 7];
const SEED: &[u8] = b"m34-random-candidate-order";

/// Runs of F4 and F5. A run disagrees with probability about one half before
/// the fix, so all runs agree by chance with probability about `2^-64`.
const AGREEMENT_RUNS: usize = 64;

/// Cases of F6. Each family receives about `F6_CASES / FAMILIES.len()` cases.
const F6_CASES: u32 = 512;

const TRACE_LIMITS: NativeOperationTraceLimits = NativeOperationTraceLimits {
    events: 1_000,
    source_entries: 10_000,
    source_bytes: 10_000_000,
    telemetry_items: 1_000,
    telemetry_bytes: 1_000_000,
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

/// Added by the merge of feature/cost-accounted-rho (finding RT-03): an honest
/// guarded consume that play stores. No resting datum passes the guard, so
/// the consume waits until `7` arrives. Native replay must accept the Stored
/// operation, and its Stored check has no commit-veto skip now (decision 6).
const STORED_GUARDED_CONSUME: &str = r#"@"x"!(1) | @"x"!(2) |
new t, u in {
  t!(0) |
  for (_ <- t) { for (@a <- @"x" where a > 5) { @"out"!(a) } | u!(0) } |
  for (_ <- u) { @"x"!(7) }
}"#;

/// Added by the merge of feature/cost-accounted-rho (finding RT-04): the join
/// of `REPEATED_CHANNEL` with a guard, over three data. Four assignments pass
/// the guard, so play and native replay must agree on the first one.
const GUARDED_REPEATED_CHANNEL: &str = r#"new go in {
  @"x"!(1) | @"x"!(2) | @"x"!(3) |
  contract go(@c1, @c2) = { for (@a <- @c1 & @b <- @c2 where a != 2) { @"out"!((a, b)) } } |
  go!("x", "x")
}"#;

/// The families of F6. In each, play meets two or more candidates that match
/// equally. The flag says whether play's pick varies from run to run. E1
/// gives the data of a repeated channel one stable order, so the join that
/// names one channel twice has one pick.
// Changed by the merge of feature/cost-accounted-rho (finding RT-03): the
// families also hold an honest guarded consume that play stores.
// const FAMILIES: [(&str, &str, bool); 9] = [
const FAMILIES: [(&str, &str, bool); 10] = [
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
    // Changed by the merge of feature/cost-accounted-rho (finding R1-23): the
    // complete guard-aware search (decision 4) makes play fire on a datum that
    // the guard accepts, so a family with one such datum has one pick. A third
    // datum gives each guarded family two admissible data, and the random
    // pick still varies. F5 keeps the terms with one admissible datum.
    // ("a guarded consume, guard above", GUARD_ABOVE, true),
    // ("a guarded consume, guard below", GUARD_BELOW, true),
    // (
    //     "a guarded join met by a produce, guard above",
    //     r#"@"y"!(1) | @"y"!(2) |
    // for (@a <- @"x" & @b <- @"y" where b > 1) { @"out"!((a, b)) } |
    // new t in { t!(0) | for (_ <- t) { @"x"!(0) } }"#,
    //     true,
    // ),
    // (
    //     "a guarded join met by a produce, guard below",
    //     r#"@"y"!(1) | @"y"!(2) |
    // for (@a <- @"x" & @b <- @"y" where b < 2) { @"out"!((a, b)) } |
    // new t in { t!(0) | for (_ <- t) { @"x"!(0) } }"#,
    //     true,
    // ),
    (
        "a guarded consume, guard above",
        r#"@"x"!(1) | @"x"!(2) | @"x"!(3) |
new t in { t!(0) | for (_ <- t) { for (@a <- @"x" where a > 1) { @"out"!(a) } } }"#,
        true,
    ),
    (
        "a guarded consume, guard below",
        r#"@"x"!(1) | @"x"!(2) | @"x"!(3) |
new t in { t!(0) | for (_ <- t) { for (@a <- @"x" where a < 3) { @"out"!(a) } } }"#,
        true,
    ),
    (
        "a guarded join met by a produce, guard above",
        r#"@"y"!(1) | @"y"!(2) | @"y"!(3) |
for (@a <- @"x" & @b <- @"y" where b > 1) { @"out"!((a, b)) } |
new t in { t!(0) | for (_ <- t) { @"x"!(0) } }"#,
        true,
    ),
    (
        "a guarded join met by a produce, guard below",
        r#"@"y"!(1) | @"y"!(2) | @"y"!(3) |
for (@a <- @"x" & @b <- @"y" where b < 3) { @"out"!((a, b)) } |
new t in { t!(0) | for (_ <- t) { @"x"!(0) } }"#,
        true,
    ),
    // Added by the merge of feature/cost-accounted-rho (finding RT-03). No
    // datum passes the guard until `7` arrives, so play has one pick.
    (
        "an honest guarded consume that play stores",
        STORED_GUARDED_CONSUME,
        false,
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

/// Added by the merge of feature/cost-accounted-rho (finding RT-03): whether a
/// COMM on `@"x"` fires on the produce just before it in play's log. Then the
/// consume of that COMM rested before the produce arrived.
fn fires_on_a_produce_on_x(log: &[Event]) -> bool {
    let x = channel_hash_for_epoch(
        NATIVE_EPOCH,
        &new_gstring_par("x".to_owned(), Vec::new(), false),
    )
    .expect("the channel has a key in the native epoch");
    log.windows(2).any(|pair| match pair {
        [Event::IoEvent(IOEvent::Produce(produce)), Event::Comm(comm)] => {
            produce.channel_hash == x && comm.produces.iter().any(|copy| copy.hash == produce.hash)
        }
        _ => false,
    })
}

/// One native play of `term` and two replays of its checked trace through
/// the native replay session. Returns the COMMs that play recorded, or the
/// first disagreement.
///
/// Changed by the merge of feature/cost-accounted-rho (finding RT-03): it
/// returns play's log, so that a test can read the order of play's events.
/// F6 reads the COMMs of the log with `recorded_comms`.
// async fn native_session_agreement(term: &str) -> Result<Vec<String>, String> {
async fn native_session_agreement(term: &str) -> Result<Vec<Event>, String> {
    let parsed = Compiler::source_to_adt(term).map_err(|error| format!("parse: {error}"))?;
    let mut stores = InMemoryStoreManager::new();
    // Changed by the merge of feature/cost-accounted-rho (finding R1-33): the
    // play space has the codec epoch of the native harnesses.
    // let (play, _) = RSpace::create_with_replay(
    //     stores.r_space_stores().await.expect("in-memory stores"),
    //     Arc::new(Box::new(Matcher)),
    // )
    // .expect("a play space");
    let (play, _) = RSpace::create_with_replay_for_epoch(
        NATIVE_EPOCH,
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("a play space");
    play.create_checkpoint().await.expect("the pre-state");
    let history = play.get_history_repository();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(SEED, Sig::Ground(vec![7]));
    // Changed by the merge of feature/cost-accounted-rho (finding R1-33): the
    // budget binds the codec epoch of the play space.
    // let settings = config(LIMIT, WEIGHTS);
    let settings = native_config(LIMIT, WEIGHTS);
    budget
        .reset_for_native_execution(settings)
        .map_err(|error| format!("play budget: {error}"))?;
    let (reducer, _, _, _) = create_rho_env(
        play.clone(),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget.clone(),
        ExternalServices::noop(),
    )
    .await
    .map_err(|error| format!("play environment: {error}"))?;
    let scope = budget.enter_comm_accounting_scope();
    let rand = || Blake2b512Random::create_from_bytes(SEED);
    let played = reducer.inj_with_observation(parsed.clone(), rand()).await;
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
        let trace = with_contract(LIMIT, WEIGHTS, |contract| {
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
        // Changed by the merge of feature/cost-accounted-rho (finding R1-33):
        // the replay budget binds the codec epoch of the play space.
        // let replay_settings = config(LIMIT, WEIGHTS);
        let replay_settings = native_config(LIMIT, WEIGHTS);
        replay_budget
            .reset_for_native_execution(replay_settings)
            .map_err(|error| format!("replay {pass} budget: {error}"))?;
        // Changed by the merge of feature/cost-accounted-rho (finding R1-11):
        // F1R3Lang's native replay takes the codec epoch of its budget first.
        let mut environment = create_native_replay_env(
            // Changed by the merge of feature/cost-accounted-rho (finding
            // R1-33): native replay has the codec epoch of the play space.
            // EventCodecEpoch::Legacy,
            NATIVE_EPOCH,
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
        require(replayed.errors.is_empty(), || {
            format!("replay {pass} errors: {:?}", replayed.errors)
        })?;
        environment
            .check_complete()
            .await
            .map_err(|error| format!("replay {pass} left recorded operations: {error}"))?;
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
    // Changed by the merge of feature/cost-accounted-rho (finding RT-03).
    // Ok(recorded_comms(&log))
    Ok(log)
}

/// One native play of `term` through a runtime, replayed from its recorded
/// log by the legacy replay of the paired replay runtime.
async fn legacy_replay_agreement(term: &str) -> Result<(), String> {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.expect("in-memory stores");
    // Changed by the merge of feature/cost-accounted-rho (finding R1-33): the
    // runtimes have the codec epoch of the native harnesses.
    // let (mut play, mut replay, _) = create_runtimes(stores, false, &mut Vec::new()).await;
    let (mut play, mut replay) = native_runtimes(stores).await;
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
            // Changed by the merge of feature/cost-accounted-rho (finding R1-33).
            // config(LIMIT, WEIGHTS),
            native_config(LIMIT, WEIGHTS),
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
    // Changed by the merge of feature/cost-accounted-rho (finding R1-33).
    // let replayed = replay
    //     .evaluate_with_native_phlo(term, HashMap::new(), rand, None, config(LIMIT, WEIGHTS))
    //     .await
    //     .map_err(|error| format!("replay failed: {error}"))?;
    let replayed = replay
        .evaluate_with_native_phlo(
            term,
            HashMap::new(),
            rand,
            None,
            native_config(LIMIT, WEIGHTS),
        )
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
    // Added by the merge of feature/cost-accounted-rho (finding RT-04): the
    // guarded join of one channel twice, through the native replay session.
    for error in native_disagreements(GUARDED_REPEATED_CHANNEL, AGREEMENT_RUNS).await {
        failed.push(format!("guarded: {error}"));
    }
    for run in 0..AGREEMENT_RUNS {
        if let Err(error) = legacy_replay_agreement(REPEATED_CHANNEL).await {
            failed.push(format!("legacy run {run}: {error}"));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} runs disagreed: {failed:#?}",
        failed.len(),
        // Changed by the merge of feature/cost-accounted-rho (finding RT-04).
        // 2 * AGREEMENT_RUNS
        3 * AGREEMENT_RUNS
    );
}

/// F5: a guarded consume meets resting data.
///
/// Changed by the merge of feature/cost-accounted-rho (decision 4, finding
/// R1-23): play's complete guard-aware search fires on the datum that the
/// guard accepts, and native replay must agree. Before, play could try the
/// datum that the guard rejects and store the consume. The greedy pass of the
/// native session's Stored check, in canonical order, could then find the
/// datum that the guard accepts.
///
/// Added by the merge (finding RT-03): an honest guarded consume that play
/// stores must replay too. The Stored check has no commit-veto skip now
/// (decision 6). At least one run must store the consume before `7` arrives,
/// or the Stored check of that term never runs.
#[tokio::test]
async fn a_stored_guarded_consume_replays_under_either_guard() {
    let mut failed = Vec::new();
    for term in [GUARD_ABOVE, GUARD_BELOW] {
        for error in native_disagreements(term, AGREEMENT_RUNS).await {
            failed.push(format!("{term}: {error}"));
        }
    }
    // Added by the merge of feature/cost-accounted-rho (finding RT-03).
    let mut stored_first = 0;
    for run in 0..AGREEMENT_RUNS {
        match native_session_agreement(STORED_GUARDED_CONSUME).await {
            Ok(log) => stored_first += usize::from(fires_on_a_produce_on_x(&log)),
            Err(error) => failed.push(format!("{STORED_GUARDED_CONSUME}: run {run}: {error}")),
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} runs disagreed: {failed:#?}",
        failed.len(),
        // Changed by the merge of feature/cost-accounted-rho (finding RT-03).
        // 2 * AGREEMENT_RUNS
        3 * AGREEMENT_RUNS
    );
    assert!(
        stored_first > 0,
        "no run stored the guarded consume before 7 arrived"
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
                // Changed by the merge of feature/cost-accounted-rho (finding
                // RT-03): the agreement returns play's log.
                // Ok(comms) => {
                Ok(log) => {
                    picks
                        .lock()
                        .expect("the pick map")
                        .entry(family)
                        .or_default()
                        // .insert(comms);
                        .insert(recorded_comms(&log));
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
