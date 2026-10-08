//! The native certification suite of decision 47 (design v2, section 6).
//!
//! Each test plays Rholang deploys natively and certifies them as proposer self-certification
//! and validator replay do, at the v6 limits of the offered funded protocol
//! (`support::native`). The tests cover these properties:
//!
//! | Test | Property |
//! |---|---|
//! | `space_native_replay_equal_{hashmap,pathmap,array,vector}` | Every one of the 24 families plays natively without error, and native replay reproduces native play: the errors, a complete journal, the usage, the authority events, the byte observations and the post-state root. The programs store, park, fire on both sides, peek (Rule E), send and receive persistently, join, deliver by prefix (PathMap) and allocate in `use` (Array, Vector). |
//! | `space_native_journal_rows_per_operation` | The journal holds one row for each produce and each consume of the log, and one COMM record for each COMM. Each staged space operation has the sub-order path `parent ++ [(i, 2)]`, numbered from 0 in its transaction, and locks space channels only. |
//! | `space_native_handle_calls_after_factory` | The factory parks the handle first, and each handle call is a host operation that follows the previous operation on the handle. The deploy certifies. |
//! | `space_native_persistent_retry_rows` | A persistent element that a send offers again in flight (decision 30, DV-3) keeps its identity: play records the offer as a retry of a space row (decision 43), and native replay consumes the retry. |
//! | `space_native_denial_replay` | Out of phlo at each budget attempt of the largest space transaction, play and native replay agree. A bisection on the phlo limit finds the limit at which play first denies each attempt. |
//! | `space_native_scope_reset_after_restore` | After a restore, a second evaluation of the native replay environment reproduces play: the bound space store resets its scope (W2). |
//! | `space_native_forged_predecessor_fails_closed` | The journal check rejects a forged predecessor. A forged predecessor with a matching forged footprint passes the check, and the space row fails closed when it authenticates its footprint, with no hang. |
//!
//! The unit tests of the native backend (`native_session/space.rs`) and of the scheduler
//! (`reified/schedule.rs`) cover the other rows of section 6.
//!
//! The suite does not compare native play with legacy play. Native execution stamps the cost
//! authority of the deploy into the data of each produce, so the identities of the operations
//! differ between the two modes. The order of DR-75 sorts bag and set candidates by the hash of
//! their source produce, so the two modes can choose different candidates, and their roots
//! differ. Both modes are deterministic.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use models::rhoapi::Par;
use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use rholang::rust::interpreter::accounting::byte_receipts::ByteObservation;
use rholang::rust::interpreter::accounting::{
    NativeBudgetRecording, NativeObservationLink, NativeOperationRecord, NativeOperationSource,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::spaces::namespace::{self, ReifiedId};
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
use support::native::{
    assert_reproduces, certify, committed_evidence, finish, footprint_channels, host_work,
    is_space_channel, is_space_row, native_play, random, rejected, replay_environment,
    replay_environment_with, row_attempts, Played, AMPLE_LIMIT, OUT, WEIGHTS,
};

// ─── Programs ──────────────────────────────────────────────────────────────────────────────────

/// The outer stores of the families.
#[derive(Clone, Copy, Debug)]
enum Outer {
    HashMap,
    PathMap,
    Array,
    Vector,
}

impl Outer {
    fn name(self) -> &'static str {
        match self {
            Outer::HashMap => "hashmap",
            Outer::PathMap => "pathmap",
            Outer::Array => "array",
            Outer::Vector => "vector",
        }
    }

    /// The constructor of the factory call. An array needs its size (F7).
    fn ctor(self) -> &'static str {
        match self {
            Outer::Array => "\"default\"; size=8",
            Outer::HashMap | Outer::PathMap | Outer::Vector => "\"default\"",
        }
    }

    /// The part of the program that exercises the outer store, over the name `e`: a persistent
    /// waiter (HashMap), a prefix delivery (PathMap, decision 9.3), or an allocation by `new` in
    /// `use` and a persistent waiter (Array, Vector, F11).
    fn part(self) -> &'static str {
        match self {
            Outer::HashMap => r#"for (@v <= e) { out!(["each", v]) } | e!(7)"#,
            Outer::PathMap => {
                r#"use space { @[*e, 1]!(5) | @[*e, 2]!(6) } | for (@v <- e) { out!(["prefix", v]) }"#
            }
            Outer::Array | Outer::Vector => {
                r#"use space { new n in { n!(1) | for (@v <- n) { out!(["alloc", v]) } } } |
for (@v <= e) { out!(["each", v]) } | e!(7)"#
            }
        }
    }

    /// The label of the result that [`Outer::part`] delivers.
    fn part_label(self) -> &'static str {
        match self {
            Outer::HashMap | Outer::Array | Outer::Vector => "each",
            Outer::PathMap => "prefix",
        }
    }
}

/// The inner collections of the families.
#[derive(Clone, Copy, Debug)]
enum Inner {
    Bag,
    Queue,
    Stack,
    Set,
    Cell,
    PriorityQueue,
}

impl Inner {
    const ALL: [Inner; 6] = [
        Inner::Bag,
        Inner::Queue,
        Inner::Stack,
        Inner::Set,
        Inner::Cell,
        Inner::PriorityQueue,
    ];

    fn name(self) -> &'static str {
        match self {
            Inner::Bag => "bag",
            Inner::Queue => "queue",
            Inner::Stack => "stack",
            Inner::Set => "set",
            Inner::Cell => "cell",
            Inner::PriorityQueue => "priorityqueue",
        }
    }

    /// The part of the program that exercises the discipline, over the names `a` to `d`: the
    /// runtime programs of the determinism suite (N9), and a same-space join of `c` and `d`.
    fn part(self) -> &'static str {
        match self {
            Inner::Bag => {
                r#"a!(3) | a!(1) | a!(2) | a!(1) | b!!(7) |
for (@x <- a) { out!(["take", x]) } |
for (@1 <- a) { out!(["one", 1]) } |
for (@y <<- a) { out!(["peek", y]) } |
for (@z <- b) { out!(["persistent", z]) } |
for (@w <<- b) { out!(["peek persistent", w]) }"#
            }
            Inner::Queue | Inner::Stack => {
                r#"a!(1) | a!(2) | a!(3) |
for (@x <- a) { out!(["take", x]) } |
for (@y <<- a) { out!(["peek", y]) } |
for (@2 <- b) { out!(["two", 2]) } | b!(1) | b!(2)"#
            }
            Inner::Set => {
                r#"a!(3) | a!(1) | a!(2) | a!(1) | b!!(7) |
for (@1 <- a) { out!(["class", 1]) } |
for (@x <- a) { out!(["take", x]) } |
for (@y <<- a) { out!(["peek", y]) } |
for (@z <- b) { out!(["persistent", z]) }"#
            }
            Inner::Cell => {
                r#"a!(1) | for (@x <- a) { out!(["take", x]) | a!(x + 1) } |
for (@y <<- b) { out!(["peek", y]) } | b!(5)"#
            }
            Inner::PriorityQueue => {
                r#"a!(1; priority=1) | a!(2; priority=0) | a!(3) |
for (@x <- a) { out!(["take", x]) } |
for (@y <<- a) { out!(["peek", y]) } |
for (@z <- a ~ (; priority=1)) { out!(["lane", z]) } |
for (@2 <- b ~ (; priority=0)) { out!(["two", 2]) } | b!(1) | b!(2; priority=0)"#
            }
        }
    }
}

/// The join of `c` and `d` that every family program runs.
const JOIN: &str = r#"for (@p <- c & @q <- d) { out!(["join", p, q]) } | c!(5) | d!(6)"#;

/// A deploy that creates a space of the family `urn` with the constructor `ctor`, draws the
/// names `a` to `e` from it, and runs `body`. The values on `out` go to the public channel
/// [`OUT`].
fn space_program(urn: &str, ctor: &str, body: &str) -> String {
    format!(
        r#"new factory(`{urn}`), out in {{
  for (space <- factory!?({ctor})) {{
    for (a <- space!?("gensym")) {{
      for (b <- space!?("gensym")) {{
        for (c <- space!?("gensym")) {{
          for (d <- space!?("gensym")) {{
            for (e <- space!?("gensym")) {{
              {body}
            }}
          }}
        }}
      }}
    }}
  }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#
    )
}

/// The program of the family `outer:inner`.
fn program(outer: Outer, inner: Inner) -> String {
    space_program(
        &format!("rho:space:{}:{}", outer.name(), inner.name()),
        outer.ctor(),
        &format!("{} |\n{} |\n{}", inner.part(), JOIN, outer.part()),
    )
}

/// The rows of the journal of `played`, with its recording.
fn journal_of(played: &Played) -> (&NativeBudgetRecording, &[NativeOperationRecord]) {
    let recording = played
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget");
    let journal = played
        .result
        .native_operation_recording
        .as_ref()
        .expect("native play records its operations");
    (recording, journal.as_ref())
}

/// True when some output of `played` carries the label `label`.
fn delivers(played: &Played, label: &str) -> bool {
    let quoted = format!("\"{label}\"");
    played.outputs.iter().any(|output| output.contains(&quoted))
}

// ─── 24 families ───────────────────────────────────────────────────────────────────────────────

/// Plays the program of `outer:inner` natively and certifies the play.
async fn check_family(outer: Outer, inner: Inner) {
    let label = format!("{}:{}", outer.name(), inner.name());
    let term = program(outer, inner);
    let played = native_play(&term, AMPLE_LIMIT).await;
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
    let (recording, journal) = journal_of(&played);
    let space_rows = journal
        .iter()
        .filter(|row| is_space_row(recording, row))
        .count();
    assert!(
        space_rows >= 10,
        "{label}: the journal holds the staged space operations ({space_rows})"
    );
    let certification = certify(&played, &term, AMPLE_LIMIT).await;
    assert_reproduces(&label, &played, &certification);
}

async fn check_outer(outer: Outer) {
    for inner in Inner::ALL {
        check_family(outer, inner).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_replay_equal_hashmap() { check_outer(Outer::HashMap).await }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_replay_equal_pathmap() { check_outer(Outer::PathMap).await }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_replay_equal_array() { check_outer(Outer::Array).await }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_replay_equal_vector() { check_outer(Outer::Vector).await }

// ─── Journal rows ──────────────────────────────────────────────────────────────────────────────

/// True when `channel` is a storage channel of a space: a config, counter, data, path-index or
/// allocator id. A host operation never touches one.
fn is_storage_channel(channel: &Par) -> bool {
    matches!(
        namespace::reified_id_of(channel),
        Some(
            ReifiedId::Config(_)
                | ReifiedId::SpaceCounter(_)
                | ReifiedId::CollectionCounter(..)
                | ReifiedId::Data(..)
                | ReifiedId::PathIndex(..)
                | ReifiedId::Allocator(_)
        )
    )
}

/// Checks the rows of the journal of `term` against its log.
async fn check_rows(label: &str, term: &str) {
    let played = native_play(term, AMPLE_LIMIT).await;
    assert!(
        played.result.errors.is_empty(),
        "{label}: {:?}",
        played.result.errors
    );
    let (recording, journal) = journal_of(&played);

    // One row for each produce and each consume of the log, by kind and identity.
    let mut operations: HashMap<(bool, Vec<u8>), usize> = HashMap::with_capacity(played.log.len());
    let mut comms = 0;
    for event in &played.log {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => {
                *operations.entry((true, produce.hash.bytes())).or_insert(0) += 1;
            }
            Event::IoEvent(IOEvent::Consume(consume)) => {
                *operations.entry((false, consume.hash.bytes())).or_insert(0) += 1;
            }
            Event::Comm(_) => comms += 1,
        }
    }
    let mut rows: HashMap<(bool, Vec<u8>), usize> = HashMap::with_capacity(journal.len());
    for row in journal {
        let key = match &row.source {
            NativeOperationSource::Produce(produce) => (true, produce.hash.to_vec()),
            NativeOperationSource::Consume(consume) => (false, consume.hash.to_vec()),
        };
        *rows.entry(key).or_insert(0) += 1;
    }
    assert_eq!(
        rows, operations,
        "{label}: one journal row for each operation of the log"
    );
    assert_eq!(
        journal.iter().filter(|row| row.comm.is_some()).count(),
        comms,
        "{label}: one COMM record for each COMM of the log"
    );

    // The staged space operations: sub-order paths numbered from 0 in each transaction, over
    // space channels only. A host operation touches no storage channel of a space.
    let mut transactions: Vec<(_, Vec<u64>)> = Vec::new();
    let mut space_rows = 0;
    let mut fired = 0;
    for row in journal {
        let channels = footprint_channels(row);
        if is_space_row(recording, row) {
            space_rows += 1;
            fired += usize::from(row.comm.is_some());
            assert!(
                channels.iter().all(is_space_channel),
                "{label}: a staged operation locks space channels only: {channels:?}"
            );
            let (index, _) = recording
                .paths
                .last_segment(row.occurrence.path)
                .expect("a space row has a sub-order segment");
            let parent = recording.paths.parent(row.occurrence.path);
            match transactions.iter_mut().find(|(known, _)| *known == parent) {
                Some((_, indices)) => indices.push(index),
                None => transactions.push((parent, vec![index])),
            }
        } else {
            assert!(
                !channels.iter().any(is_storage_channel),
                "{label}: a host operation touches no storage channel of a space: {channels:?}"
            );
        }
    }
    assert!(
        space_rows >= 10,
        "{label}: the journal holds the staged operations ({space_rows})"
    );
    assert!(fired > 0, "{label}: some staged operation fires");
    assert!(
        transactions.iter().any(|(_, indices)| indices.len() >= 3),
        "{label}: some transaction stages three or more operations"
    );
    for (_, indices) in &transactions {
        let expected: Vec<u64> = (0..indices.len() as u64).collect();
        assert_eq!(
            *indices, expected,
            "{label}: the sub-orders of one transaction"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_journal_rows_per_operation() {
    check_rows("hashmap:queue", &program(Outer::HashMap, Inner::Queue)).await;
    check_rows("pathmap:bag", &program(Outer::PathMap, Inner::Bag)).await;
    check_rows("vector:set", &program(Outer::Vector, Inner::Set)).await;
}

// ─── Handle calls ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_handle_calls_after_factory() {
    let term = space_program(
        "rho:space:hashmap:queue",
        "\"default\"",
        r#"for (@r <- space!?("produce", *a, [1])) {
  for (@q <- space!?("qualifier")) {
    for (@config <- space!?("config")) {
      out!(["produced", r]) | out!(["qualifier", q]) | out!(["outer", config.get("outer")]) |
      for (@x <- a) { out!(["took", x]) }
    }
  }
}"#,
    );
    let played = native_play(&term, AMPLE_LIMIT).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    for wanted in ["produced", "qualifier", "outer", "took"] {
        assert!(
            delivers(&played, wanted),
            "the program delivers {wanted}: {:?}",
            played.outputs
        );
    }
    let (recording, journal) = journal_of(&played);
    let handle_rows: Vec<usize> = journal
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            footprint_channels(row).iter().any(|channel| {
                matches!(
                    namespace::reified_id_of(channel),
                    Some(ReifiedId::Handle(_))
                )
            })
        })
        .map(|(index, _)| index)
        .collect();
    let (&factory, calls) = handle_rows
        .split_first()
        .expect("the journal holds the operations on the handle");
    assert!(
        is_space_row(recording, &journal[factory]),
        "the factory parks the handle in its transaction"
    );
    assert!(
        calls.len() >= 8,
        "five gensym calls, produce, qualifier and config call the handle ({})",
        calls.len()
    );
    for (position, &call) in calls.iter().enumerate() {
        let row = &journal[call];
        if is_space_row(recording, row) {
            continue;
        }
        assert!(
            row.predecessors.contains(&handle_rows[position]),
            "a handle call follows the previous operation on the handle"
        );
    }
    let certification = certify(&played, &term, AMPLE_LIMIT).await;
    assert_reproduces("handle calls", &played, &certification);
}

// ─── Retries ───────────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_persistent_retry_rows() {
    let term = space_program(
        "rho:space:hashmap:bag",
        "\"default\"",
        r#"for (@z <- b) { out!(["take", z]) } |
for (@w <<- b) { out!(["peek", w]) } |
new go in { go!(Nil) | for (_ <- go) { b!!(7) } }"#,
    );
    let played = native_play(&term, AMPLE_LIMIT).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    for wanted in ["take", "peek"] {
        assert!(
            delivers(&played, wanted),
            "the persistent element fires {wanted}: {:?}",
            played.outputs
        );
    }
    let (recording, journal) = journal_of(&played);
    assert!(
        !recording.retries.is_empty(),
        "play records the re-offer of the persistent element as a retry"
    );
    assert!(
        journal.iter().any(|row| {
            is_space_row(recording, row)
                && matches!(row.introduction, NativeObservationLink::Retry(_))
        }),
        "a staged space produce links to a retry"
    );
    let certification = certify(&played, &term, AMPLE_LIMIT).await;
    assert_reproduces("persistent re-offer", &played, &certification);
}

// ─── Denials ───────────────────────────────────────────────────────────────────────────────────

/// True when the byte observation of an attempt has a quantity with a positive weight, so the
/// attempt charges phlo and some limit denies it.
fn charges(observation: &ByteObservation) -> bool {
    let measurement = observation
        .measurement
        .expect("a native attempt has a measurement");
    let quantities = [
        u64::from(observation.kind == AuthorityByteEventKind::Comm),
        measurement.introduction_bytes,
        measurement.transfer_bytes,
        measurement.trace_bytes,
    ];
    quantities
        .iter()
        .zip(WEIGHTS)
        .any(|(quantity, weight)| quantity * weight > 0)
}

/// Native play of `term` at the phlo limit `limit`, and the index of the first attempt that it
/// denies, or the number of its attempts when it denies none.
async fn play_at(term: &str, limit: u64) -> (usize, Played) {
    let played = native_play(term, limit).await;
    let (recording, _) = journal_of(&played);
    let denied = recording
        .attempts
        .iter()
        .position(|attempt| !attempt.granted)
        .unwrap_or(recording.attempts.len());
    (denied, played)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_denial_replay() {
    let term = program(Outer::HashMap, Inner::Queue);
    let full = native_play(&term, AMPLE_LIMIT).await;
    assert!(full.result.errors.is_empty(), "{:?}", full.result.errors);
    let (recording, journal) = journal_of(&full);

    // The space transaction with the most staged operations, the first in the journal on a tie.
    let mut transactions: Vec<(_, Vec<usize>)> = Vec::new();
    for (index, row) in journal.iter().enumerate() {
        if !is_space_row(recording, row) {
            continue;
        }
        let parent = recording.paths.parent(row.occurrence.path);
        match transactions.iter_mut().find(|(known, _)| *known == parent) {
            Some((_, rows)) => rows.push(index),
            None => transactions.push((parent, vec![index])),
        }
    }
    let largest = transactions
        .iter()
        .map(|(_, rows)| rows.len())
        .max()
        .expect("play stages space operations");
    let (_, rows) = transactions
        .iter()
        .find(|(_, rows)| rows.len() == largest)
        .expect("the largest transaction");
    assert!(largest >= 3, "the transaction stages {largest} operations");
    let targets: BTreeSet<usize> = rows
        .iter()
        .flat_map(|row| row_attempts(&journal[*row]))
        .filter(|attempt| charges(&recording.attempts[*attempt].observation))
        .collect();
    assert!(
        targets.len() >= largest,
        "each staged operation of the transaction has a charged attempt"
    );

    // The first denied attempt grows with the limit: a bisection finds a limit at which play
    // first denies each target. At the limit of the full usage, play denies nothing.
    let mut low = 0;
    for target in targets {
        let mut high = recording.used;
        let (limit, played) = loop {
            assert!(low <= high, "some limit denies attempt {target} first");
            let middle = low + (high - low) / 2;
            let (denied, played) = play_at(&term, middle).await;
            match denied.cmp(&target) {
                std::cmp::Ordering::Equal => break (middle, played),
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => {
                    high = middle
                        .checked_sub(1)
                        .expect("play at the limit 0 denies its first charged attempt")
                }
            }
        };
        low = limit;
        let label = format!("out of phlo at attempt {target} (limit {limit})");
        assert!(
            played
                .result
                .errors
                .contains(&InterpreterError::OutOfPhlogistonsError),
            "{label}: play runs out of phlo: {:?}",
            played.result.errors
        );
        let certification = certify(&played, &term, limit).await;
        assert_reproduces(&label, &played, &certification);
    }
}

// ─── Restore ───────────────────────────────────────────────────────────────────────────────────

/// A queue program without the join and the outer part. A certification has one host-work
/// budget, and a restore does not refund it, so two evaluations of the full family program
/// exceed the v6 limit of `VerificationBytes`.
const RESTORE_BODY: &str = r#"a!(1) | a!(2) | a!(3) |
for (@x <- a) { out!(["take", x]) } |
for (@y <<- a) { out!(["peek", y]) } |
for (@2 <- b) { out!(["two", 2]) } | b!(1) | b!(2)"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_scope_reset_after_restore() {
    let term = space_program("rho:space:hashmap:queue", "\"default\"", RESTORE_BODY);
    let played = native_play(&term, AMPLE_LIMIT).await;
    let (recording, journal) = committed_evidence(&played);
    let host = host_work();
    let mut environment =
        replay_environment_with(&played, AMPLE_LIMIT, &recording, journal, host.clone())
            .await
            .expect("the committed evidence checks");
    let checkpoint = environment
        .checkpoint()
        .await
        .expect("the environment takes a checkpoint");
    let parsed = Compiler::source_to_adt(&term).expect("the program parses");
    let first = environment
        .evaluate(parsed, random())
        .await
        .expect("the first evaluation runs");
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    assert_eq!(
        environment
            .check_complete()
            .await
            .map_err(|error| error.to_string()),
        Ok(()),
        "the first evaluation consumes the journal"
    );
    environment
        .restore(checkpoint)
        .await
        .expect("the environment restores its checkpoint");
    let certification = finish(environment, &term).await;
    assert!(
        host.rejection().is_none(),
        "the two evaluations fit the v6 host work: {:?}",
        host.rejection()
    );
    assert_reproduces("the evaluation after the restore", &played, &certification);
}

// ─── Forged predecessors ───────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_forged_predecessor_fails_closed() {
    let term = program(Outer::HashMap, Inner::Queue);
    let played = native_play(&term, AMPLE_LIMIT).await;
    let (recording, journal) = committed_evidence(&played);
    let target = journal
        .iter()
        .rposition(|row| is_space_row(&recording, row))
        .expect("play stages space operations");

    // An earlier row with a channel that no other row touches.
    let mut touches: HashMap<&Arc<[u8]>, usize> = HashMap::with_capacity(journal.len());
    for row in journal.iter() {
        for channel in row.footprint.iter() {
            *touches.entry(channel).or_insert(0) += 1;
        }
    }
    let (forged, channel) = journal[..target]
        .iter()
        .enumerate()
        .find_map(|(index, row)| {
            row.footprint
                .iter()
                .find(|channel| touches[channel] == 1)
                .map(|channel| (index, Arc::clone(channel)))
        })
        .expect("an earlier row touches a channel of its own");
    assert!(
        !journal[target].predecessors.contains(&forged),
        "the target does not depend on the forged row"
    );

    // A forged predecessor alone: the journal check rejects it.
    let mut rows: Vec<NativeOperationRecord> = journal.to_vec();
    let mut predecessors = rows[target].predecessors.to_vec();
    predecessors.push(forged);
    predecessors.sort_unstable();
    rows[target].predecessors = predecessors.clone().into();
    let error = replay_environment(&played, AMPLE_LIMIT, &recording, rows.into())
        .await
        .err()
        .expect("the journal check rejects a forged predecessor");
    assert!(
        error.starts_with("the journal check rejects the journal"),
        "{error}"
    );

    // A forged predecessor with a matching forged footprint passes the journal check. The row
    // fails closed when it authenticates its footprint, and replay ends.
    let mut rows: Vec<NativeOperationRecord> = journal.to_vec();
    let mut footprint = rows[target].footprint.to_vec();
    footprint.push(channel);
    footprint.sort();
    rows[target].footprint = footprint.into();
    rows[target].predecessors = predecessors.into();
    let environment = replay_environment(&played, AMPLE_LIMIT, &recording, rows.into())
        .await
        .expect("the forged footprint and predecessor pass the journal check");
    let certification = tokio::time::timeout(Duration::from_secs(600), finish(environment, &term))
        .await
        .expect("native replay ends without a hang");
    assert!(
        rejected(&certification),
        "the certification rejects the forged journal"
    );
    assert!(
        certification.complete.is_err(),
        "the forged row stays incomplete"
    );
}
