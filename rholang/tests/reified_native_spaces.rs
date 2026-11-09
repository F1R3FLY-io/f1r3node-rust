//! The native certification suite of decision 47 (design v2, section 6).
//!
//! Each test plays Rholang deploys natively and certifies them as proposer self-certification
//! and validator replay do, at the v6 limits of the offered funded protocol
//! (`support::native`). The tests cover these properties:
//!
//! | Test | Property |
//! |---|---|
//! | `space_native_replay_equal_{hashmap,pathmap,array,vector}` | Every one of the 24 families plays natively without error, and native replay reproduces native play: the errors, a complete journal, the usage, the authority events, the byte observations and the post-state root. The programs store, park, fire on both sides, peek (Rule E), send and receive persistently, join, deliver by prefix (PathMap) and allocate in `use` (Array, Vector). |
// Changed by decision 92: since DR-132 a row is named by its journal index, and every space row
// names the space counter of its space in its footprint.
// //! | `space_native_journal_rows_per_operation` | The journal holds one row for each produce and each consume of the log, and one COMM record for each COMM. Each staged space operation has the sub-order path `parent ++ [(i, 2)]`, numbered from 0 in its transaction, and locks space channels only. |
//! | `space_native_journal_rows_per_operation` | The journal holds one row for each produce and each consume of the log, and one COMM record for each COMM. Each staged space operation locks space channels only, names the space counter `S(sid)` of its space in its footprint, and follows the previous space row of its space (decision 92). |
//! | `space_native_handle_calls_after_factory` | The factory parks the handle first, and each handle call is a host operation that follows the previous operation on the handle. The deploy certifies. |
//! | `space_native_persistent_retry_rows` | A persistent element that a send offers again in flight (decision 30, DV-3) keeps its identity: play records the offer as a retry of a space row (decision 43), and native replay consumes the retry. |
// Changed by decision 93: since DR-132 a row is named by its journal index, so no parent groups
// the rows of a transaction. The first denied attempt at a limit also depends on the racing
// order, so a bisection across separate plays has nothing monotone to search.
// //! | `space_native_denial_replay` | Out of phlo at each budget attempt of the largest space transaction, play and native replay agree. A bisection on the phlo limit finds the limit at which play first denies each attempt. |
//! | `space_native_denial_replay` | Decision 93: out of phlo at each charged budget attempt of a space row, native play and native replay agree. On each runtime of `RACING_WORKERS` (0, 2 and 8 workers, where 0 is the current-thread runtime), a probe plays at the limit that denies the attempt in the order of a reference play. Each probe is judged on its own record and certified by its own native replay. Non-vacuity: the probes deny produce introductions, consume introductions and COMMs of space rows, and some denied row names a data channel inside its transaction. |
// Added by plan v4 errata (V4-19, T-C4-39): the denial test of the other outers.
//! | `space_native_denial_replay_other_outers` | Plan v4 errata V4-19 (T-C4-39): the body of `space_native_denial_replay` for the PathMap, Array and Vector outers with the queue inner. Each probe is judged on its own record and certified by its own native replay. Non-vacuity for each outer: the checks of `space_native_denial_replay`, and some probe denies a creation row before the handle. Some probe denies an allocator take exactly when the outer allocates (Array, Vector). |
//! | `space_native_scope_reset_after_restore` | After a restore, a second evaluation of the native replay environment reproduces play: the bound space store resets its scope (W2). |
//! | `space_native_forged_predecessor_fails_closed` | The journal check rejects a forged predecessor. A forged predecessor with a matching forged footprint passes the check, and the space row fails closed when it authenticates its footprint, with no hang. |
// Changed by decision 93: racing evaluation can run two rows of a forged predecessor edge at the
// same time (DR-132), and the journal check rejects such a forgery.
// //! | `space_native_forged_footprints_never_hang` | Every forgery that makes a later host row wait for a staged space row ends: the space row fails closed and closes the epoch, so the waiting host operation wakes and fails, and the certification rejects the journal. Without the closure, 64 of 109 such forgeries hung (the probe of step 4). |
//! | `space_native_forged_footprints_never_hang` | Every forgery that makes a later host row wait for a staged space row ends. When racing evaluation ran two rows of a forged predecessor edge at the same time, the journal check rejects the forgery. Otherwise the space row fails closed, and its row stays incomplete. The waiting host operation parks counted, so the stall rule of the base replay ends the wait (DR-132), and the certification rejects the journal. The sweep covers one play on each runtime of `RACING_WORKERS` (0, 2 and 8 workers, where 0 is the current-thread runtime, decision 93). Before the counted park of DR-132, 64 of 109 such forgeries hung (the probe of step 4). |
// Changed by decision 93: in one deploy, racing evaluation decides whether a waiter parks before
// the operation that fires it, so two chains fix the orders that the shape claims need.
// //! | `space_native_vectordb_replay_equal` | P6 step 5: every query shape of `hashmap:vectordb` and `vector:vectordb` that a source program can write plays natively at the v6 limits, and native replay reproduces the play. A list receive fires in one COMM over `[D; m]`, and the lists of LM-b (CE-1, CE-2) fire through an offer. |
// Changed by decision 101 (C5.4): a list receive binds one record over `[D]` and retires the
// other members with `K_retire` (rule CR-13), so no COMM consumes over `[D; m]`. The vacuous
// check of LM-b becomes a check of the COMMs of the parked waiters.
//! | `space_native_vectordb_replay_equal` | P6 step 5 and decisions 93 and 101: every query shape of `hashmap:vectordb` and `vector:vectordb` that a source program can write plays natively at the v6 limits, and native replay reproduces the play, on each runtime of `RACING_WORKERS`, where 0 is the current-thread runtime. In a chain that stores the records first, a list receive of two fires at once in exactly two COMMs over `[D]`, each with one record: the bind carries the list, and `K_retire` takes the other member (rule CR-13). No COMM consumes over `[D; m]`. In the LM-b chain, the waiters park first, and the lists of LM-b (CE-1, CE-2) fire through an offer: each waiter parks one consume over `[D]`, and exactly one COMM of the removals consumes it, with one record. |
// Changed by decision 93: the cap program parks its waiters before the removal only in some
// racing orders, and no row names the transaction of the settle job since DR-132.
// //! | `space_native_vectordb_cascade_carries_at_the_cap` | P6 step 5: the receive of the round-cap program of LIST-CASCADE stops at the round cap, its settle job carries the other fires, and native replay reproduces the play. |
//! | `space_native_vectordb_cascade_carries_at_the_cap` | P6 step 5 and decision 93: three deploys store `img` and the documents, park the list waiters, and remove `img`. The cascade has more fires than one transaction holds, so the settle job of the receive carries the other fires. On each runtime of `RACING_WORKERS`, where 0 is the current-thread runtime, every play delivers each output in the same number of COMM rows, and native replay reproduces it. `reified_vectordb.rs` observes the stop at the round cap directly. |
//! | `space_native_vectordb_errors_reproduce` | P6 step 5: `CapacityExceeded` and `DimensionMismatch` abort the deploy in native play, and native replay reproduces the failure. |
// Changed by decision 93: in one deploy, LM-b parks its waiters before the removals only in some
// racing orders, and the largest transaction is not named since DR-132.
// //! | `space_native_vectordb_denial_replay` | P6 step 5: out of phlo at each charged budget attempt of the largest VectorDB transaction (LM-b), play and native replay agree. |
//! | `space_native_vectordb_denial_replay` | P6 step 5 and decision 93: LM-b as three deploys, the records, the two list waiters and the removals. Out of phlo at each charged budget attempt of a space row of the removals, play and native replay agree, as in `space_native_denial_replay`. Non-vacuity: some denied row follows an earlier row on its own collection. |
//! | `space_native_allocations_keep_an_authority` | P9 step 2 regression: on an Array and a Vector space, two deploys with one `new` in a `use` block each, and one deploy with two nested `new`s, play natively and certify. Each allocator record carries a cost authority, so the COMM of the next allocation is billable. |
//! | `space_native_anchor_charges_one_consume_introduction_and_one_comm` | Decision 92: a send to an occupied cell commits the anchor of its job only, a consume on `C(sid)` whose footprint names `S(sid)`. The anchor links one consume introduction, which measures what the take of the space counter measures, and one COMM, which transfers the config record. Native replay reproduces the play, the failure included. |
//! | `space_racing_replay_{never_stalls,never_fails_closed,agrees_with_play,completes_every_row}` | Decisions 92 and 93: one test for each invariant of `SpaceReplayOrder.tla` (NoStall, NoFailClosed, Agreement, ExactRows). Nine families race the space transactions of one or two spaces on each runtime of `RACING_WORKERS`, where 0 is the current-thread runtime, three runs each. Each run certifies its own play. Non-vacuity: some family shows two journal orders, and each positional failure commits an anchor. |
// Changed by decision 92: the scheduler of `reified/schedule.rs` is gone since decision 90, and
// racing native replay orders the space transactions.
// //! The unit tests of the native backend (`native_session/space.rs`) and of the scheduler
// //! (`reified/schedule.rs`) cover the other rows of section 6.
//!
//! The unit tests of the native backend (`native_session/space.rs`) cover the other rows of
//! section 6. The racing properties cover the order of the space transactions in native replay.
//!
//! The suite does not compare native play with legacy play. Native execution stamps the cost
//! authority of the deploy into the data of each produce, so the identities of the operations
//! differ between the two modes. The order of DR-75 sorts bag and set candidates by the hash of
//! their source produce, so the two modes can choose different candidates, and their roots
//! differ.
// Changed by decision 93: racing evaluation decides the order of parallel operations in play
// (DR-132), so no mode gives one canonical run.
// //! differ. Both modes are deterministic.
//! Racing evaluation decides the order of parallel operations in each play (DR-132), so each
//! test certifies a play against its own replay (decision 93).

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use models::rhoapi::Par;
use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use rholang::rust::interpreter::accounting::byte_accounting::{
    comm_trace_bytes, datum_transfer_bytes, ByteCharge,
};
use rholang::rust::interpreter::accounting::byte_receipts::ByteObservation;
use rholang::rust::interpreter::accounting::economic_failure::classify_errors;
use rholang::rust::interpreter::accounting::phlo_execution::PhloFailure;
use rholang::rust::interpreter::accounting::{
    NativeBudgetRecording, NativeObservationLink, NativeOperationRecord, NativeOperationSource,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::reified::costs::FIRE_ROUND_CAP;
use rholang::rust::interpreter::spaces::allocator;
use rholang::rust::interpreter::spaces::namespace::{self, ReifiedId};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
// Changed by decision 101 (C5.4): the checks of row 32 read the consumes and the COMMs.
// use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, COMM};
use support::family_programs::{program, space_program, Inner, Outer};
use support::native::{
    assert_reproduces, certify, committed_evidence, finish, footprint_channels, is_space_channel,
    is_space_row, native_play, random, rejected, replay_environment, row_attempts, trace_limits,
    with_contract, Certification, NativeChain, Played, AMPLE_LIMIT, OUT, WEIGHTS,
};

// ─── Programs ──────────────────────────────────────────────────────────────────────────────────

// The family programs moved to `support::family_programs` (P7), so the P7 suites play them too.

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
    // Changed by decision 92: a space row names the counter channel of its space, so
    // `is_space_row` reads only the row, not the recording.
    // let (recording, journal) = journal_of(&played);
    let (_, journal) = journal_of(&played);
    let space_rows = journal.iter().filter(|row| is_space_row(row)).count();
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
    // Changed by decision 92: the space rows are found by their footprint, not by their paths.
    // let (recording, journal) = journal_of(&played);
    let (_, journal) = journal_of(&played);

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

    // Changed by decision 92: since DR-132 a row is named `[(index, 0)]`, so a space row has no
    // sub-order segment, and the rows of one transaction have no common parent. Every row of a
    // space transaction names the space counter `S(sid)` in its footprint, so the space rows of
    // one space form one chain of predecessors in play order.
    // // The staged space operations: sub-order paths numbered from 0 in each transaction, over
    // // space channels only. A host operation touches no storage channel of a space.
    // let mut transactions: Vec<(_, Vec<u64>)> = Vec::new();
    // let mut space_rows = 0;
    // let mut fired = 0;
    // for row in journal {
    //     let channels = footprint_channels(row);
    //     if is_space_row(recording, row) {
    //         space_rows += 1;
    //         fired += usize::from(row.comm.is_some());
    //         assert!(
    //             channels.iter().all(is_space_channel),
    //             "{label}: a staged operation locks space channels only: {channels:?}"
    //         );
    //         let (index, _) = recording
    //             .paths
    //             .last_segment(row.occurrence.path)
    //             .expect("a space row has a sub-order segment");
    //         let parent = recording.paths.parent(row.occurrence.path);
    //         match transactions.iter_mut().find(|(known, _)| *known == parent) {
    //             Some((_, indices)) => indices.push(index),
    //             None => transactions.push((parent, vec![index])),
    //         }
    //     } else {
    //         assert!(
    //             !channels.iter().any(is_storage_channel),
    //             "{label}: a host operation touches no storage channel of a space: {channels:?}"
    //         );
    //     }
    // }
    // assert!(
    //     space_rows >= 10,
    //     "{label}: the journal holds the staged operations ({space_rows})"
    // );
    // assert!(fired > 0, "{label}: some staged operation fires");
    // assert!(
    //     transactions.iter().any(|(_, indices)| indices.len() >= 3),
    //     "{label}: some transaction stages three or more operations"
    // );
    // for (_, indices) in &transactions {
    //     let expected: Vec<u64> = (0..indices.len() as u64).collect();
    //     assert_eq!(
    //         *indices, expected,
    //         "{label}: the sub-orders of one transaction"
    //     );
    // }

    // The staged space operations: over space channels only, with one space counter in each
    // footprint, and each after the previous space row of its space. A host operation touches
    // no storage channel of a space.
    let mut last_of_space: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut space_rows = 0;
    let mut fired = 0;
    for (index, row) in journal.iter().enumerate() {
        let channels = footprint_channels(row);
        if is_space_row(row) {
            space_rows += 1;
            fired += usize::from(row.comm.is_some());
            assert!(
                channels.iter().all(is_space_channel),
                "{label}: a staged operation locks space channels only: {channels:?}"
            );
            let counters: Vec<Vec<u8>> = row
                .footprint
                .iter()
                .zip(&channels)
                .filter(|(_, channel)| {
                    matches!(
                        namespace::reified_id_of(channel),
                        Some(ReifiedId::SpaceCounter(_))
                    )
                })
                .map(|(bytes, _)| bytes.to_vec())
                .collect();
            let counter = match <[Vec<u8>; 1]>::try_from(counters) {
                Ok([counter]) => counter,
                Err(counters) => panic!(
                    "{label}: a space row names one space counter, not {}",
                    counters.len()
                ),
            };
            if let Some(previous) = last_of_space.insert(counter, index) {
                assert!(
                    row.predecessors.contains(&previous),
                    "{label}: space row {index} follows space row {previous} of its space"
                );
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
    // Changed by decision 92: a space row names the counter channel of its space, so
    // `is_space_row` reads only the row, not the recording.
    // let (recording, journal) = journal_of(&played);
    let (_, journal) = journal_of(&played);
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
        is_space_row(&journal[factory]),
        "the factory parks the handle in its transaction"
    );
    assert!(
        calls.len() >= 8,
        "five gensym calls, produce, qualifier and config call the handle ({})",
        calls.len()
    );
    for (position, &call) in calls.iter().enumerate() {
        let row = &journal[call];
        if is_space_row(row) {
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
            is_space_row(row) && matches!(row.introduction, NativeObservationLink::Retry(_))
        }),
        "a staged space produce links to a retry"
    );
    let certification = certify(&played, &term, AMPLE_LIMIT).await;
    assert_reproduces("persistent re-offer", &played, &certification);
}

// ─── Denials ───────────────────────────────────────────────────────────────────────────────────

// Changed by decision 124 (plan v2 C7.1 to C7.6, decision 99): weighted bytes charge phlo only
// through a priced region, and a machinery row can have none. The text before:
// /// True when the byte observation of an attempt has a quantity with a positive weight, so the
// /// attempt charges phlo and some limit denies it.
/// True when the byte observation of an attempt has a quantity with a positive weight. Such an
/// attempt charges phlo, and some limit denies it, when its authority has a priced region.
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

// Changed by decision 93: since DR-132 a row is named `[(index, 0)]`, so `paths.parent` groups
// the whole journal and names no transaction. Racing evaluation also decides which attempt a
// limit denies first, so the bisection across separate plays had nothing monotone to search.
// Each probe now plays at a limit from a reference play. Each probe is judged on its own record
// and certified by its own native replay.
// /// Native play of `term` at the phlo limit `limit`, and the index of the first attempt that it
// /// denies, or the number of its attempts when it denies none.
// async fn play_at(term: &str, limit: u64) -> (usize, Played) {
//     let played = native_play(term, limit).await;
//     let (recording, _) = journal_of(&played);
//     let denied = recording
//         .attempts
//         .iter()
//         .position(|attempt| !attempt.granted)
//         .unwrap_or(recording.attempts.len());
//     (denied, played)
// }
//
// /// Out of phlo at each charged budget attempt of the largest space transaction of `term`, native
// /// play and native replay agree.
// async fn check_denials(term: &str) {
//     let full = native_play(term, AMPLE_LIMIT).await;
//     assert!(full.result.errors.is_empty(), "{:?}", full.result.errors);
//     let (recording, journal) = journal_of(&full);
//
//     // The space transaction with the most staged operations, the first in the journal on a tie.
//     let mut transactions: Vec<(_, Vec<usize>)> = Vec::new();
//     for (index, row) in journal.iter().enumerate() {
//         if !is_space_row(recording, row) {
//             continue;
//         }
//         let parent = recording.paths.parent(row.occurrence.path);
//         match transactions.iter_mut().find(|(known, _)| *known == parent) {
//             Some((_, rows)) => rows.push(index),
//             None => transactions.push((parent, vec![index])),
//         }
//     }
//     let largest = transactions
//         .iter()
//         .map(|(_, rows)| rows.len())
//         .max()
//         .expect("play stages space operations");
//     let (_, rows) = transactions
//         .iter()
//         .find(|(_, rows)| rows.len() == largest)
//         .expect("the largest transaction");
//     assert!(largest >= 3, "the transaction stages {largest} operations");
//     let targets: BTreeSet<usize> = rows
//         .iter()
//         .flat_map(|row| row_attempts(&journal[*row]))
//         .filter(|attempt| charges(&recording.attempts[*attempt].observation))
//         .collect();
//     assert!(
//         targets.len() >= largest,
//         "each staged operation of the transaction has a charged attempt"
//     );
//
//     // The first denied attempt grows with the limit: a bisection finds a limit at which play
//     // first denies each target. At the limit of the full usage, play denies nothing.
//     let mut low = 0;
//     for target in targets {
//         let mut high = recording.used;
//         let (limit, played) = loop {
//             assert!(low <= high, "some limit denies attempt {target} first");
//             let middle = low + (high - low) / 2;
//             let (denied, played) = play_at(term, middle).await;
//             match denied.cmp(&target) {
//                 std::cmp::Ordering::Equal => break (middle, played),
//                 std::cmp::Ordering::Less => low = middle + 1,
//                 std::cmp::Ordering::Greater => {
//                     high = middle
//                         .checked_sub(1)
//                         .expect("play at the limit 0 denies its first charged attempt")
//                 }
//             }
//         };
//         low = limit;
//         let label = format!("out of phlo at attempt {target} (limit {limit})");
//         assert!(
//             played
//                 .result
//                 .errors
//                 .contains(&InterpreterError::OutOfPhlogistonsError),
//             "{label}: play runs out of phlo: {:?}",
//             played.result.errors
//         );
//         let certification = certify(&played, term, limit).await;
//         assert_reproduces(&label, &played, &certification);
//     }
// }
//
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_native_denial_replay() {
//     check_denials(&program(Outer::HashMap, Inner::Queue)).await
// }

// Changed by decision 111: 0 workers give the current-thread runtime, as in the base.
/// The runtime of a racing test: the current-thread runtime for 0 workers, else a multi-thread
/// runtime with `workers` worker threads (decisions 93 and 111).
fn racing_runtime(workers: usize) -> tokio::runtime::Runtime {
    match workers {
        0 => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread racing runtime"),
        _ => tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .enable_all()
            .build()
            .expect("a racing runtime"),
    }
}

/// The charge of each budget attempt of `recording`: the usage of its observation under the
/// native phlo contract, as the bound of native play counts it. A run without owner funding
/// records no restoration and no owner refusal, so the bound counts the whole charge.
fn attempt_costs(recording: &NativeBudgetRecording) -> Vec<u64> {
    let regions = trace_limits().regions;
    with_contract(AMPLE_LIMIT, |contract| {
        let mut costs = Vec::with_capacity(recording.attempts.len());
        for attempt in recording.attempts.iter() {
            assert!(
                attempt.restorations.is_empty() && !attempt.owner_refused,
                "a run without owner funding records no restoration and no owner refusal"
            );
            let charge = contract
                .prepare(Arc::clone(&attempt.observation), regions)
                .expect("the charge of a recorded attempt prepares");
            costs.push(charge.usage());
        }
        costs
    })
}

/// The probes of a denial test from the reference play `reference` at [`AMPLE_LIMIT`]. For each
/// charged attempt `t` of a space row, the probe is the pair of `t` and the limit
/// `p_t + c_t - 1`. Here `c_t` is the charge of `t`, and `p_t` is the sum of the charges before
/// `t`. A play in the order of the reference grants each attempt before `t` and denies `t`.
fn denial_limits(label: &str, reference: &Played) -> Vec<(usize, u64)> {
    assert!(
        reference.result.errors.is_empty(),
        "{label}: the reference play fails: {:?}",
        reference.result.errors
    );
    let (recording, journal) = journal_of(reference);
    assert!(
        recording.attempts.iter().all(|attempt| attempt.granted),
        "{label}: the reference play grants every attempt"
    );
    let costs = attempt_costs(recording);
    assert_eq!(
        costs.iter().sum::<u64>(),
        recording.used,
        "{label}: the charges of the reference sum to its usage"
    );
    let mut before = Vec::with_capacity(costs.len());
    let mut spent = 0_u64;
    for cost in &costs {
        before.push(spent);
        spent += cost;
    }
    let targets: BTreeSet<usize> = journal
        .iter()
        .filter(|row| is_space_row(row))
        .flat_map(row_attempts)
        .filter(|attempt| charges(&recording.attempts[*attempt].observation))
        // Added by decision 124 (plan v2 C7.1 to C7.6, decision 99): a machinery row carries the
        // payer introduction or a seal of system residue, so it can have weighted bytes and no
        // priced region. Its attempt charges nothing, and no limit denies it.
        .filter(|attempt| costs[*attempt] > 0)
        .collect();
    assert!(
        !targets.is_empty(),
        "{label}: the space rows of the reference have charged attempts"
    );
    let mut limits = Vec::with_capacity(targets.len());
    for target in targets {
        assert!(
            costs[target] > 0,
            "{label}: the charged attempt {target} costs phlo"
        );
        limits.push((target, before[target] + costs[target] - 1));
    }
    limits
}

/// The denials that the probes of a denial test observe, for the non-vacuity checks of the
/// test.
#[derive(Debug, Default)]
struct Denials {
    /// The probes.
    probes: usize,
    /// The probes that run out of phlo.
    exhausted: usize,
    /// The denied produce introductions of space rows.
    produce: usize,
    /// The denied consume introductions of space rows.
    consume: usize,
    /// The denied COMMs of space rows.
    comm: usize,
    /// The denied attempts of space rows for whose row the witness of the test holds.
    inside: usize,
}

impl Denials {
    /// Counts a denied attempt of a space row: the kind `kind` of its observation, and the
    /// witness `inside` of its row.
    fn add(&mut self, kind: AuthorityByteEventKind, inside: bool) {
        match kind {
            AuthorityByteEventKind::ProduceIntroduction => self.produce += 1,
            AuthorityByteEventKind::ConsumeIntroduction => self.consume += 1,
            AuthorityByteEventKind::Comm => self.comm += 1,
        }
        self.inside += usize::from(inside);
    }

    /// Checks that the probes deny each kind of attempt on space rows, and that the witness of
    /// the test holds for some denied space row.
    fn assert_non_vacuous(&self, label: &str) {
        for (part, count) in [
            ("probe", self.probes),
            ("probe that runs out of phlo", self.exhausted),
            ("denied produce introduction of a space row", self.produce),
            ("denied consume introduction of a space row", self.consume),
            ("denied COMM of a space row", self.comm),
            ("denied space row with the witness", self.inside),
        ] {
            assert!(count > 0, "{label}: no {part}: {self:?}");
        }
    }
}

/// Judges the probe `probe` at the limit `limit` on its own record. A probe that denies no
/// attempt fails nothing and uses at most its limit. A probe that denies an attempt runs out of
/// phlo. A row is rejected exactly when it links a denied attempt, and exactly one row links each
/// denied attempt. Each denied attempt of a space row adds the kind of its observation and the
/// witness `inside` of its row to `denials`.
fn judge_probe(
    label: &str,
    probe: &Played,
    limit: u64,
    inside: fn(&[NativeOperationRecord], usize) -> bool,
    denials: &mut Denials,
) {
    let (recording, journal) = journal_of(probe);
    denials.probes += 1;
    let denied = recording
        .attempts
        .iter()
        .filter(|attempt| !attempt.granted)
        .count();
    match denied {
        0 => {
            assert!(
                probe.result.errors.is_empty(),
                "{label}: a probe that denies nothing fails: {:?}",
                probe.result.errors
            );
            assert!(
                recording.used <= limit,
                "{label}: the probe uses {} over its limit",
                recording.used
            );
        }
        _ => {
            denials.exhausted += 1;
            assert!(
                probe
                    .result
                    .errors
                    .contains(&InterpreterError::OutOfPhlogistonsError),
                "{label}: a probe that denies {denied} attempts runs out of phlo: {:?}",
                probe.result.errors
            );
        }
    }
    let mut linking: Vec<Vec<usize>> = vec![Vec::new(); recording.attempts.len()];
    for (index, row) in journal.iter().enumerate() {
        let attempts = row_attempts(row);
        assert_eq!(
            matches!(row.completion, RSpaceOperationCompletion::Rejected),
            attempts
                .iter()
                .any(|attempt| !recording.attempts[*attempt].granted),
            "{label}: row {index} is rejected exactly when it links a denied attempt"
        );
        for attempt in attempts {
            linking[attempt].push(index);
        }
    }
    for (attempt, rows) in linking.iter().enumerate() {
        match (recording.attempts[attempt].granted, rows.as_slice()) {
            (true, _) => {}
            (false, &[row]) if is_space_row(&journal[row]) => denials.add(
                recording.attempts[attempt].observation.kind,
                inside(journal, row),
            ),
            (false, &[_]) => {}
            (false, rows) => {
                panic!("{label}: the rows {rows:?} link the denied attempt {attempt}")
            }
        }
    }
}

/// The data channels of the collections in the footprint of `row`.
fn data_channels(row: &NativeOperationRecord) -> Vec<Par> {
    footprint_channels(row)
        .into_iter()
        .filter(|channel| matches!(namespace::reified_id_of(channel), Some(ReifiedId::Data(..))))
        .collect()
}

/// The witness of `space_native_denial_replay`: row `index` of `journal` names a data channel.
/// On a queue, the bump of the collection counter stages first, so such a row is inside its
/// transaction.
fn names_data(journal: &[NativeOperationRecord], index: usize) -> bool {
    !data_channels(&journal[index]).is_empty()
}

/// Decision 93: out of phlo at each charged budget attempt of a space row of the queue program,
/// native play and native replay agree. On each runtime of `RACING_WORKERS`, a reference play at
/// [`AMPLE_LIMIT`] gives one probe for each such attempt ([`denial_limits`]). Each probe plays at
/// its limit, [`judge_probe`] judges it on its own record, and its own native replay certifies
/// it. Non-vacuity: the probes deny produce introductions, consume introductions and COMMs of
/// space rows, and some denied row names a data channel ([`names_data`]).
#[test]
fn space_native_denial_replay() {
    let term = program(Outer::HashMap, Inner::Queue);
    let mut denials = Denials::default();
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("hashmap:queue, {workers} workers");
            let reference = native_play(&term, AMPLE_LIMIT).await;
            let limits = denial_limits(&label, &reference);
            let certification = certify(&reference, &term, AMPLE_LIMIT).await;
            assert_reproduces(&label, &reference, &certification);
            for (target, limit) in limits {
                let label = format!("{label}, attempt {target} (limit {limit})");
                let probe = native_play(&term, limit).await;
                judge_probe(&label, &probe, limit, names_data, &mut denials);
                let certification = certify(&probe, &term, limit).await;
                assert_reproduces(&label, &probe, &certification);
            }
        });
    }
    denials.assert_non_vacuous("hashmap:queue");
}

/// Added by plan v4 errata (V4-19, T-C4-39): the witness of a creation row before the handle.
/// No row up to row `index` of `journal` names a handle channel. The creation parks the handle
/// in its last row, and no other space row comes before that row, so row `index` is the config,
/// the space counter or a record of the outer store of the creation.
fn before_handle(journal: &[NativeOperationRecord], index: usize) -> bool {
    !journal[..=index].iter().any(|row| {
        footprint_channels(row).iter().any(|channel| {
            matches!(
                namespace::reified_id_of(channel),
                Some(ReifiedId::Handle(_))
            )
        })
    })
}

/// Added by plan v4 errata (V4-19, T-C4-39): the witness of an allocator take. Row `index` of
/// `journal` is a consume whose footprint names the allocator channel `A(sid)` of its space.
fn takes_allocator(journal: &[NativeOperationRecord], index: usize) -> bool {
    matches!(journal[index].source, NativeOperationSource::Consume(_))
        && footprint_channels(&journal[index]).iter().any(|channel| {
            matches!(
                namespace::reified_id_of(channel),
                Some(ReifiedId::Allocator(_))
            )
        })
}

/// Added by plan v4 errata (V4-19, T-C4-39): true when the probe `probe` rejects a space row
/// for which the witness `witness` holds. A row is rejected exactly when it links a denied
/// attempt ([`judge_probe`]).
fn denies(probe: &Played, witness: fn(&[NativeOperationRecord], usize) -> bool) -> bool {
    let (_, journal) = journal_of(probe);
    journal.iter().enumerate().any(|(index, row)| {
        matches!(row.completion, RSpaceOperationCompletion::Rejected)
            && is_space_row(row)
            && witness(journal, index)
    })
}

/// Added by plan v4 errata (V4-19, T-C4-39): the body of [`space_native_denial_replay`] for the
/// PathMap, Array and Vector outers with the queue inner. Each probe is judged on its own record
/// and certified by its own native replay. Non-vacuity for each outer: the checks of
/// [`Denials::assert_non_vacuous`], and some probe denies a creation row before the handle
/// ([`before_handle`]), so the staging of the creation ends at the record end (C7.3). Some
/// probe denies an allocator take ([`takes_allocator`]) exactly when the outer allocates, so
/// the staging of an allocation ends at the record end. The PathMap outer has no allocator.
#[test]
fn space_native_denial_replay_other_outers() {
    for (outer, allocates) in [
        (Outer::PathMap, false),
        (Outer::Array, true),
        (Outer::Vector, true),
    ] {
        let term = program(outer, Inner::Queue);
        let family = format!("{}:queue", outer.name());
        let mut denials = Denials::default();
        let mut creations = 0_usize;
        let mut takes = 0_usize;
        for workers in RACING_WORKERS {
            racing_runtime(workers).block_on(async {
                let label = format!("{family}, {workers} workers");
                let reference = native_play(&term, AMPLE_LIMIT).await;
                let limits = denial_limits(&label, &reference);
                let certification = certify(&reference, &term, AMPLE_LIMIT).await;
                assert_reproduces(&label, &reference, &certification);
                for (target, limit) in limits {
                    let label = format!("{label}, attempt {target} (limit {limit})");
                    let probe = native_play(&term, limit).await;
                    judge_probe(&label, &probe, limit, names_data, &mut denials);
                    creations += usize::from(denies(&probe, before_handle));
                    takes += usize::from(denies(&probe, takes_allocator));
                    let certification = certify(&probe, &term, limit).await;
                    assert_reproduces(&label, &probe, &certification);
                }
            });
        }
        denials.assert_non_vacuous(&family);
        assert!(
            creations > 0,
            "{family}: no probe denies a creation row before the handle: {denials:?}"
        );
        assert_eq!(
            takes > 0,
            allocates,
            "{family}: some probe denies an allocator take exactly when the outer allocates"
        );
    }
}

// ─── Restore ───────────────────────────────────────────────────────────────────────────────────

/// A queue program without the join and the outer part, for one evaluation, a restore and a
/// second evaluation.
const RESTORE_BODY: &str = r#"a!(1) | a!(2) | a!(3) |
for (@x <- a) { out!(["take", x]) } |
for (@y <<- a) { out!(["peek", y]) } |
for (@2 <- b) { out!(["two", 2]) } | b!(1) | b!(2)"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_scope_reset_after_restore() {
    let term = space_program("rho:space:hashmap:queue", "\"default\"", RESTORE_BODY);
    let played = native_play(&term, AMPLE_LIMIT).await;
    let (recording, journal) = committed_evidence(&played);
    let mut environment = replay_environment(&played, AMPLE_LIMIT, &recording, journal)
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
        .rposition(is_space_row)
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

/// The predecessors of each row as `check_dependencies` derives them: the last earlier row that
/// touches each channel of its footprint.
fn derive_predecessors(rows: &mut [NativeOperationRecord]) {
    let mut last: HashMap<Arc<[u8]>, usize> = HashMap::with_capacity(rows.len());
    for (index, row) in rows.iter_mut().enumerate() {
        let mut expected: Vec<usize> = row
            .footprint
            .iter()
            .filter_map(|channel| last.get(channel).copied())
            .collect();
        expected.sort_unstable();
        expected.dedup();
        row.predecessors = expected.into();
        for channel in row.footprint.iter() {
            last.insert(Arc::clone(channel), index);
        }
    }
}

// Changed by decision 93: racing evaluation can run a space row and a later host row at the same
// time (DR-132). A forged predecessor of the host row then opens before the space row closes,
// and the journal check rejects the forgery (`check_dependencies`). The test now expects that
// rejection for such a forgery, and it sweeps the journal of a play on each racing runtime.
// /// Decision 47: a forged journal never hangs native replay through a space row. For each staged
// /// space row and each later host row, the forgery adds a channel of the host row to the journal
// /// footprint of the space row, and the predecessors follow from the forged footprints, so the
// /// forgery passes the journal check. The host row then waits for the space row. The space row
// /// fails closed at its footprint check, and its failure closes the epoch, so the waiting host
// /// operation wakes and fails. Every forgery ends within the timeout and is rejected.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_native_forged_footprints_never_hang() {
//     let term = space_program("rho:space:hashmap:queue", "\"default\"", RESTORE_BODY);
//     let played = native_play(&term, AMPLE_LIMIT).await;
//     assert!(
//         played.result.errors.is_empty(),
//         "{:?}",
//         played.result.errors
//     );
//     let (recording, journal) = committed_evidence(&played);
//     let rows: Vec<NativeOperationRecord> = journal.to_vec();
//     let mut forgeries = 0;
//     for target in 0..rows.len() {
//         if !is_space_row(&recording, &rows[target]) {
//             continue;
//         }
//         for host in (target + 1)..rows.len() {
//             if is_space_row(&recording, &rows[host]) {
//                 continue;
//             }
//             let Some(channel) = rows[host]
//                 .footprint
//                 .iter()
//                 .find(|channel| !rows[target].footprint.contains(channel))
//                 .map(Arc::clone)
//             else {
//                 continue;
//             };
//             let mut forged = rows.clone();
//             let mut footprint = forged[target].footprint.to_vec();
//             footprint.push(channel);
//             footprint.sort();
//             forged[target].footprint = footprint.into();
//             derive_predecessors(&mut forged);
//             let environment = replay_environment(&played, AMPLE_LIMIT, &recording, forged.into())
//                 .await
//                 .expect("a footprint-consistent forgery passes the journal check");
//             let certification =
//                 tokio::time::timeout(Duration::from_secs(60), finish(environment, &term))
//                     .await
//                     .unwrap_or_else(|_| {
//                         panic!("native replay hangs: space row {target}, host row {host}")
//                     });
//             assert!(
//                 rejected(&certification),
//                 "the certification rejects the forgery: space row {target}, host row {host}"
//             );
//             forgeries += 1;
//         }
//     }
//     assert!(
//         forgeries >= 20,
//         "the sweep covers the forgeries of the program ({forgeries})"
//     );
// }

/// The end of the Display form of the rejection of a predecessor by the journal check
/// (`NativeOperationJournalError::Dependency`).
const DEPENDENCY_REJECTION: &str =
    "native operation journal predecessors differ from its declared conflicts";

/// True when each predecessor of each row of `rows` closed its budget interval before the row
/// opened its own. The journal check rejects every other predecessor (`check_dependencies`).
fn intervals_ordered(rows: &[NativeOperationRecord]) -> bool {
    rows.iter().all(|row| {
        row.predecessors
            .iter()
            .all(|prior| rows[*prior].budget_end <= row.budget_start)
    })
}

/// Decision 47 and decision 93: a forged journal never hangs native replay through a space row.
/// For each staged space row and each later host row, the forgery adds a channel of the host row
/// to the journal footprint of the space row, and the predecessors follow from the forged
/// footprints. When racing evaluation ran two rows of a new predecessor edge at the same time,
/// the journal check rejects the forgery. Every other forgery passes the journal check, and the
/// host row waits for the space row. The space row fails closed at its footprint check, and its
/// row stays incomplete. The waiting host operation parks counted, so the stall rule of the base
/// replay ends the wait (DR-132).
///
/// Each forgery that passes the journal check ends within the timeout and is rejected. The sweep
/// runs over the journal of one play on each runtime of `RACING_WORKERS`.
#[test]
fn space_native_forged_footprints_never_hang() {
    let term = space_program("rho:space:hashmap:queue", "\"default\"", RESTORE_BODY);
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let played = native_play(&term, AMPLE_LIMIT).await;
            assert!(
                played.result.errors.is_empty(),
                "{workers} workers: {:?}",
                played.result.errors
            );
            let (recording, journal) = committed_evidence(&played);
            let rows: Vec<NativeOperationRecord> = journal.to_vec();
            let mut forgeries = 0;
            let mut overlaps = 0;
            for target in 0..rows.len() {
                if !is_space_row(&rows[target]) {
                    continue;
                }
                for host in (target + 1)..rows.len() {
                    if is_space_row(&rows[host]) {
                        continue;
                    }
                    let Some(channel) = rows[host]
                        .footprint
                        .iter()
                        .find(|channel| !rows[target].footprint.contains(channel))
                        .map(Arc::clone)
                    else {
                        continue;
                    };
                    let label = format!("{workers} workers, space row {target}, host row {host}");
                    let mut forged = rows.clone();
                    let mut footprint = forged[target].footprint.to_vec();
                    footprint.push(channel);
                    footprint.sort();
                    forged[target].footprint = footprint.into();
                    derive_predecessors(&mut forged);
                    let ordered = intervals_ordered(&forged);
                    let checked =
                        replay_environment(&played, AMPLE_LIMIT, &recording, forged.into()).await;
                    let environment = match (ordered, checked) {
                        (true, Ok(environment)) => environment,
                        (false, Err(error)) if error.ends_with(DEPENDENCY_REJECTION) => {
                            overlaps += 1;
                            continue;
                        }
                        (true, Err(error)) => {
                            panic!("{label}: a footprint-consistent forgery fails: {error}")
                        }
                        (false, Err(error)) => {
                            panic!("{label}: an overlapping predecessor fails otherwise: {error}")
                        }
                        (false, Ok(_)) => {
                            panic!("{label}: the journal check accepts an overlapping predecessor")
                        }
                    };
                    let certification =
                        tokio::time::timeout(Duration::from_secs(60), finish(environment, &term))
                            .await
                            .unwrap_or_else(|_| panic!("{label}: native replay hangs"));
                    assert!(
                        rejected(&certification),
                        "{label}: the certification rejects the forgery"
                    );
                    forgeries += 1;
                }
            }
            assert!(
                forgeries >= 20,
                "{workers} workers: the sweep covers the forgeries of the program ({forgeries} \
                 pass the journal check, {overlaps} overlap)"
            );
        });
    }
}

// ─── VectorDB (P6, step 5) ─────────────────────────────────────────────────────────────────────

/// The VectorDB families (P6).
const VECTORDB_FAMILIES: [&str; 2] = ["rho:space:hashmap:vectordb", "rho:space:vector:vectordb"];

/// The constructor of a VectorDB space of two dimensions with the default metric `cosine`.
const VECTORDB_CTOR: &str = r#""default"; capacity=16, dimensions=2"#;

/// The single-collection query shapes of a VectorDB space that a source program can write
/// (design v3 4.4 and 4.5): single mode, a peek, a threshold, a radius over a persistent record,
/// and a list of all records with scores. Only a linear bind takes a query
/// (`syntax-contract.md`), so the peek reads the canonical order. The payload is the vector by
/// default, and `embedding` gives the vector of the document.
const VECTORDB_SINGLE: &str = r#"a!([1.0, 0.0]) | a!([0.6, 0.8]) | a!("doc"; embedding=[0.0, 1.0]) | b!!([1.0, 1.0]) |
for (@x <- a ~ ([1.0, 0.0])) { out!(["take", x]) } |
for (@y <<- a) { out!(["peek", y]) } |
for (@z <- a ~ ([1.0, 0.0]; threshold=0.5)) { out!(["near", z]) } |
for (@w <- b ~ ([1.0, 1.0]; metric="euclidean", radius=0.5)) { out!(["persistent", w]) } |
for (@all <- b ~ ([1.0, 1.0]; results="all", scores=true)) { out!(["all", all]) }"#;

/// The labels that [`VECTORDB_SINGLE`] delivers.
const VECTORDB_SINGLE_LABELS: [&str; 5] = ["take", "peek", "near", "persistent", "all"];

// Changed by decision 93: racing evaluation decides whether the sends on `e` store before the
// list runs (DR-132), so the list can also fire through an offer. The list chain of
// `space_native_vectordb_replay_equal` fixes that order.
// /// The grouped query shapes: a join whose binds have queries of their own, a persistent receive,
// /// and a list of two. The list runs after the sends of its collection, so it fires in one COMM
// /// over `[D; 2]`. The shapes of [`VECTORDB_SINGLE`] and these shapes are two deploys.
// Changed by decision 101 (C5.4): a list receive binds one record over `[D]` and retires the
// other member with `K_retire` (rule CR-13), so no COMM consumes over `[D; 2]`.
/// The grouped query shapes: a join whose binds have queries of their own, a persistent receive,
/// and a list of two. The list delivers in every racing order. It fires at once, in one bind
/// over `[D]` and one `K_retire` over `[D]` (decision 101, rule CR-13), or through an offer. The
/// shapes of [`VECTORDB_SINGLE`] and these shapes are two deploys.
const VECTORDB_GROUPS: &str = r#"for (@p <- c ~ ([1.0, 0.0]) & @q <- d ~ ([0.0, 1.0])) { out!(["join", p, q]) } |
c!([1.0, 0.0]) | d!([0.0, 1.0]) |
e!([1.0, 0.0]) | e!([0.8, 0.6]) | e!([0.0, 1.0]) |
for (f <- space!?("gensym")) { for (@v <= f) { out!(["each", v]) } | f!([1.0, 0.0]) | f!([0.0, 1.0]) } |
new go in { go!(Nil) | for (_ <- go) {
  for (@list <- e ~ ([1.0, 0.0]; k=2, scores=true)) { out!(["list", list]) }
} }"#;

/// The labels that [`VECTORDB_GROUPS`] delivers.
const VECTORDB_GROUP_LABELS: [&str; 3] = ["join", "each", "list"];

// Changed by decision 93: in one deploy, racing evaluation decides whether each waiter parks
// before the operation that fires it (DR-132). The LM-b chain fixes that order.
// /// The paths of LM-b (decision 66) as one program. CE-1: a list waiter refuses the list `[x1]`
// /// and parks, and a later send `x2` fires it with the incoming datum, and `x1` leaves through
// /// `K_retire`. CE-2: a list waiter refuses `[img, doc]` and parks, and a later receive removes
// /// `img`, so LIST-CASCADE fires the waiter with `[doc, doc]`.
/// The paths of LM-b (decision 66) as one program. CE-1: a list waiter refuses the list `[x1]`
/// and parks, and a later send `x2` fires it with the incoming datum, and `x1` leaves through
/// `K_retire`. CE-2: a list waiter refuses `[img, doc]` and parks, and a later receive removes
/// `img`, so LIST-CASCADE fires the waiter with `[doc, doc]`. In one deploy, racing evaluation
/// decides whether each waiter parks first, and every order delivers each label. The chain of
/// [`LMB_RECORDS`], [`LMB_WAITERS`] and [`LMB_REMOVALS`] fixes the order of LM-b.
const VECTORDB_LIST_PATHS: &str = r#"a!([1.0, 0.0]) |
for (@[p, q] <- a ~ ([1.0, 0.0]; k=2)) { out!(["pair", p, q]) } |
b!("img"; embedding=[10.0, 1.0]) | b!("doc"; embedding=[10.0, 2.0]) | b!("doc"; embedding=[10.0, 3.0]) |
for (@["doc", "doc"] <- b ~ ([1.0, 0.0]; k=2)) { out!(["docs", 2]) } |
new go in { go!(Nil) | for (_ <- go) {
  a!([1.0, 0.5]) |
  for (@"img" <- b ~ ([1.0, 0.0])) { out!(["img", 1]) }
} }"#;

/// The labels that [`VECTORDB_LIST_PATHS`] delivers.
const VECTORDB_LIST_LABELS: [&str; 3] = ["pair", "docs", "img"];

// Changed by decision 93: in one deploy, the waiters park before the removal only in some
// racing orders (DR-132). The cap program is now a chain of three deploys, whose parts are
// `cap_records`, `cap_waiters` and `CAP_REMOVAL`.
// /// The number of list waiters of [`vectordb_cap_program`]: more cascade fires than one
// /// transaction holds.
// const CAP_WAITERS: usize = FIRE_ROUND_CAP + 6;
//
// /// The cascade at the round cap (decision 49) as one program: `img`, [`CAP_WAITERS`] documents,
// /// and as many list waiters for `[doc]`, which refuse `[img]` and park. A later receive removes
// /// `img`, and LIST-CASCADE fires one waiter for each document. The fires exceed the round cap,
// /// so the transaction re-issues a settle job, which cascades on.
// fn vectordb_cap_program() -> String {
//     let mut body = String::with_capacity(CAP_WAITERS * 160);
//     body.push_str(r#"a!("img"; embedding=[10.0, 0.0])"#);
//     for index in 1..=CAP_WAITERS {
//         body.push_str(&format!(r#" | a!("doc"; embedding=[10.0, {index}.0])"#));
//     }
//     for index in 0..CAP_WAITERS {
//         body.push_str(&format!(
//             "\n| for (@[\"doc\"] <- a ~ ([1.0, 0.0]; k=1)) {{ out!([\"cascade\", {index}]) }}"
//         ));
//     }
//     body.push_str(
//         r#"
// | new go in { go!(Nil) | for (_ <- go) { for (@"img" <- a ~ ([1.0, 0.0])) { out!(["img", 0]) } } }"#,
//     );
//     space_program(
//         "rho:space:hashmap:vectordb",
//         r#""default"; capacity=200, dimensions=2"#,
//         &body,
//     )
// }

/// The number of list waiters of the cap chain ([`cap_waiters`]): more cascade fires than one
/// transaction holds.
const CAP_WAITERS: usize = FIRE_ROUND_CAP + 6;

// The deduction of `space_native_vectordb_cascade_carries_at_the_cap` needs more outputs than
// one transaction fires.
const _: () = assert!(CAP_WAITERS + 1 > FIRE_ROUND_CAP);

/// The stash of the cap chain of `space_native_vectordb_cascade_carries_at_the_cap`.
const CAP_STASH: &str = "rrs-native-cap";

/// The constructor of the space of the cap chain, with room for `img` and every document.
const CAP_CTOR: &str = r#""default"; capacity=200, dimensions=2"#;

/// The records of the cap chain as one deploy: `img` and [`CAP_WAITERS`] documents on `a`.
fn cap_records() -> String {
    let mut records = String::with_capacity((CAP_WAITERS + 1) * 48);
    records.push_str(r#"a!("img"; embedding=[10.0, 0.0])"#);
    for index in 1..=CAP_WAITERS {
        records.push_str(&format!(r#" | a!("doc"; embedding=[10.0, {index}.0])"#));
    }
    records
}

/// The list waiters of the cap chain as one deploy: [`CAP_WAITERS`] receives of `[doc]`, which
/// refuse `[img]` and park.
fn cap_waiters() -> String {
    (0..CAP_WAITERS)
        .map(|index| {
            format!(r#"for (@["doc"] <- a ~ ([1.0, 0.0]; k=1)) {{ out!(["cascade", {index}]) }}"#)
        })
        .collect::<Vec<_>>()
        .join(" |\n")
}

/// The removal of the cap chain as one deploy: the receive of `img`. LIST-CASCADE then fires one
/// waiter for each document.
const CAP_REMOVAL: &str = r#"for (@"img" <- a ~ ([1.0, 0.0])) { out!(["img", 0]) }"#;

/// A deploy that creates a space of the family `urn` with the constructor `ctor`, stores the
/// handle and the names `a` and `b` on the public channel `stash`, and runs `state`.
fn stashing(urn: &str, ctor: &str, stash: &str, state: &str) -> String {
    space_program(
        urn,
        ctor,
        &format!(r#"@"{stash}"!(*space, *a, *b) | {state}"#),
    )
}

/// A deploy that runs `body` over the handle `space` and the names `a` and `b` on the public
/// channel `stash`. The values on `out` go to the public channel [`OUT`].
fn with_stash(stash: &str, body: &str) -> String {
    format!(
        r#"new out in {{
  for (space, a, b <<- @"{stash}") {{
    {body}
  }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#
    )
}

/// The number of space rows of `journal` with a COMM.
fn space_comms(journal: &[NativeOperationRecord]) -> usize {
    journal
        .iter()
        .filter(|row| is_space_row(row) && row.comm.is_some())
        .count()
}

/// Checks that the deploy `played` parks its waiters: it fails nothing, delivers nothing, and
/// fires no space operation.
fn assert_parks(label: &str, played: &Played) {
    assert!(
        played.result.errors.is_empty(),
        "{label}: {:?}",
        played.result.errors
    );
    assert!(
        played.outputs.is_empty(),
        "{label}: a waiter delivers: {:?}",
        played.outputs
    );
    let (_, journal) = journal_of(played);
    assert_eq!(space_comms(journal), 0, "{label}: a waiter fires");
}

// Changed by decision 101 (C5.4): rule CR-13 removed the list fire over `[D; m]`, so the list
// chain now checks that no such COMM occurs.
// /// True when some COMM of `log` consumes from one channel two times or more: the list fire over
// /// `[D; m]` of a receive (design v3 4.4).
/// True when some COMM of `log` consumes from one channel two times or more: the list fire over
/// `[D; m]` of a receive before decision 101 (design v3 4.4). Rule CR-13 removed that shape, so
/// [`assert_list_fire`] checks that no such COMM occurs.
fn has_list_comm(log: &[Event]) -> bool {
    log.iter().any(|event| match event {
        Event::Comm(comm) => {
            let hashes = &comm.consume.channel_hashes;
            hashes.len() >= 2 && hashes.iter().all(|hash| *hash == hashes[0])
        }
        Event::IoEvent(_) => false,
    })
}

/// Added by decision 101 (C5.4): the hashes of the data channels that the journal rows of
/// `plays` name in their footprints ([`data_channels`]).
fn data_hashes(plays: &[Played]) -> BTreeSet<Blake2b256Hash> {
    plays
        .iter()
        .flat_map(|played| journal_of(played).1)
        .flat_map(data_channels)
        .map(|channel| hash(&channel))
        .collect()
}

/// Added by decision 101 (C5.4): the COMMs of `played` that consume over one data channel of
/// `data`, in the order of the log.
fn data_comms<'a>(played: &'a Played, data: &BTreeSet<Blake2b256Hash>) -> Vec<&'a COMM> {
    played
        .log
        .iter()
        .filter_map(|event| match event {
            Event::Comm(comm) => match comm.consume.channel_hashes.as_slice() {
                [channel] if data.contains(channel) => Some(comm),
                _ => None,
            },
            Event::IoEvent(_) => None,
        })
        .collect()
}

/// Added by decision 101 (C5.4, rule CR-13): checks the fire of the list receive of the list
/// chain `lists`, which plays [`LIST_RECORDS`] and then [`LIST_RECEIVE`]. The receive of two
/// fires at once in exactly two COMMs over the data channel `[D]` of its collection. Each COMM
/// takes one record, and the two records differ: the bind takes the first member, which carries
/// the list, and `K_retire` takes the other member. No COMM consumes over `[D; m]`
/// ([`has_list_comm`]).
fn assert_list_fire(label: &str, lists: &[Played]) {
    let data = data_hashes(lists);
    let list = lists.last().expect("the list chain plays its receive");
    assert!(
        !has_list_comm(&list.log),
        "{label}: no COMM of the list receive consumes over [D; m]"
    );
    let comms = data_comms(list, &data);
    let [first, second] = comms.as_slice() else {
        panic!("{label}: the list receive fires in two COMMs over [D]: {comms:?}");
    };
    assert_eq!(
        first.consume.channel_hashes, second.consume.channel_hashes,
        "{label}: the bind and the retire consume over the data channel of the collection"
    );
    match (first.produces.as_slice(), second.produces.as_slice()) {
        ([taken], [other]) => assert_ne!(
            taken, other,
            "{label}: the bind and the retire take two different records"
        ),
        _ => panic!("{label}: each COMM of the list receive takes one record: {comms:?}"),
    }
}

/// Added by decision 101 (C5.4): checks that each list of the LM-b chain `lmb` fires through an
/// offer. The chain plays [`LMB_RECORDS`], [`LMB_WAITERS`] and [`LMB_REMOVALS`]. The waiters
/// deploy parks two consumes, one over the data channel `[D]` of each collection. The removals
/// hold exactly one COMM of each parked consume, and that COMM takes one record (rule CR-13). A
/// receive of the removals has a consume of its own, so the COMM of a parked consume shows that
/// the parked waiter fired: CE-1 through the send of `x2`, and CE-2 through LIST-CASCADE.
fn assert_offer_fires(label: &str, lmb: &[Played]) {
    let data = data_hashes(lmb);
    let [_, parked, removals] = lmb else {
        panic!("{label}: the LM-b chain plays three deploys");
    };
    let waiters: Vec<&Consume> = parked
        .log
        .iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Consume(consume)) => match consume.channel_hashes.as_slice() {
                [channel] if data.contains(channel) => Some(consume),
                _ => None,
            },
            Event::IoEvent(IOEvent::Produce(_)) | Event::Comm(_) => None,
        })
        .collect();
    assert_eq!(
        waiters.len(),
        2,
        "{label}: the LM-b waiters park two consumes over [D]: {waiters:?}"
    );
    for waiter in waiters {
        let fires: Vec<&COMM> = removals
            .log
            .iter()
            .filter_map(|event| match event {
                Event::Comm(comm) if comm.consume == *waiter => Some(comm),
                Event::Comm(_) | Event::IoEvent(_) => None,
            })
            .collect();
        match fires.as_slice() {
            [fire] => assert_eq!(
                fire.produces.len(),
                1,
                "{label}: the COMM of the parked waiter {waiter:?} takes one record: {fire:?}"
            ),
            _ => panic!(
                "{label}: exactly one COMM of the removals consumes the parked waiter {waiter:?}: \
                 {fires:?}"
            ),
        }
    }
}

/// Plays `term` natively, checks that it delivers each label of `labels`, and certifies it.
async fn check_vectordb(label: &str, term: &str, labels: &[&str]) -> Played {
    let played = native_play(term, AMPLE_LIMIT).await;
    assert!(
        played.result.errors.is_empty(),
        "{label}: native play fails: {:?}",
        played.result.errors
    );
    for wanted in labels {
        assert!(
            delivers(&played, wanted),
            "{label}: the program delivers {wanted}: {:?}",
            played.outputs
        );
    }
    let certification = certify(&played, term, AMPLE_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    played
}

// Changed by decision 93: in one deploy, racing evaluation decides whether a waiter parks before
// the operation that fires it (DR-132). So the list of the groups can fire through an offer, and
// a list of LM-b can fire in one COMM over `[D; 2]`. Two chains now fix the orders that these
// claims need, and the test runs on each runtime of `RACING_WORKERS`, where 0 is the
// current-thread runtime.
// /// Every query shape of both VectorDB families plays natively at the v6 limits, and native
// /// replay reproduces the play. A list receive fires in one COMM over `[D; m]`. The two lists of
// /// LM-b fire through an offer instead: CE-1 through the send of `x2`, and CE-2 through
// /// LIST-CASCADE. The journal of LM-b holds one row for each operation of its log.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_native_vectordb_replay_equal() {
//     for urn in VECTORDB_FAMILIES {
//         let single = space_program(urn, VECTORDB_CTOR, VECTORDB_SINGLE);
//         check_vectordb(&format!("{urn} single"), &single, &VECTORDB_SINGLE_LABELS).await;
//         let groups = space_program(urn, VECTORDB_CTOR, VECTORDB_GROUPS);
//         let played =
//             check_vectordb(&format!("{urn} groups"), &groups, &VECTORDB_GROUP_LABELS).await;
//         assert!(
//             has_list_comm(&played.log),
//             "{urn}: a list receive fires in one COMM over [D; m]"
//         );
//         let paths = space_program(urn, VECTORDB_CTOR, VECTORDB_LIST_PATHS);
//         let played = check_vectordb(&format!("{urn} LM-b"), &paths, &VECTORDB_LIST_LABELS).await;
//         assert!(
//             !has_list_comm(&played.log),
//             "{urn}: each list of LM-b fires through an offer"
//         );
//         check_rows(urn, &paths).await;
//     }
// }

/// The stash of the list chain of `space_native_vectordb_replay_equal`.
const LIST_STASH: &str = "rrs-native-list";

/// The records of the list chain as one deploy: three vectors on `a`.
const LIST_RECORDS: &str = "a!([1.0, 0.0]) | a!([0.8, 0.6]) | a!([0.0, 1.0])";

// Changed by decision 101 (C5.4): a list receive binds one record over `[D]` and retires the
// other member with `K_retire` (rule CR-13), so no COMM consumes over `[D; 2]`.
/// The list of two of the list chain as one deploy. It runs after the records of its collection,
/// so it fires at once in two COMMs over `[D]`: the bind takes the first member, which carries
/// the list, and `K_retire` takes the other member (decision 101, rule CR-13).
const LIST_RECEIVE: &str =
    r#"for (@list <- a ~ ([1.0, 0.0]; k=2, scores=true)) { out!(["list", list]) }"#;

/// Plays the deploys `terms` in order on a new chain at [`AMPLE_LIMIT`], and certifies each play
/// through its own native replay. Each deploy has the random state of its label. It returns the
/// plays in order.
async fn play_chain(label: &str, terms: &[String]) -> Vec<Played> {
    let mut chain = NativeChain::new().await;
    let mut plays = Vec::with_capacity(terms.len());
    for (position, term) in terms.iter().enumerate() {
        let label = format!("{label}, deploy {position}");
        let played = chain.play(term, &label, AMPLE_LIMIT).await;
        assert!(
            played.result.errors.is_empty(),
            "{label}: {:?}",
            played.result.errors
        );
        let certification = certify(&played, term, AMPLE_LIMIT).await;
        assert_reproduces(&label, &played, &certification);
        plays.push(played);
    }
    plays
}

// Changed by decision 101 (C5.4): a list receive binds one record over `[D]` and retires the
// other members with `K_retire` (rule CR-13), so no COMM consumes over `[D; m]`. The check that
// no COMM of LM-b consumes over `[D; m]` became vacuous, so the test now finds the COMM of each
// parked waiter.
/// P6 step 5 and decisions 93 and 101: every query shape of both VectorDB families plays
/// natively at the v6 limits, and native replay reproduces the play. Two chains fix the orders
/// that the claims on the shape of a fire need. In the list chain, a list receive of two runs
/// after its records and fires at once in exactly two COMMs over the data channel `[D]`, each
/// with one record: the bind carries the list, and `K_retire` takes the other member (rule
/// CR-13). No COMM consumes over `[D; m]` ([`assert_list_fire`]). In the LM-b chain, the waiters
/// park before the removals, so the two lists fire through an offer: CE-1 through the send of
/// `x2`, and CE-2 through LIST-CASCADE. Each waiter parks one consume over `[D]`, and exactly one
/// COMM of the removals consumes it, with one record ([`assert_offer_fires`]). The journal of
/// LM-b as one deploy holds one row for each operation of its log.
#[test]
fn space_native_vectordb_replay_equal() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            for urn in VECTORDB_FAMILIES {
                let label = format!("{urn}, {workers} workers");
                let single = space_program(urn, VECTORDB_CTOR, VECTORDB_SINGLE);
                check_vectordb(
                    &format!("{label}, single"),
                    &single,
                    &VECTORDB_SINGLE_LABELS,
                )
                .await;
                let groups = space_program(urn, VECTORDB_CTOR, VECTORDB_GROUPS);
                check_vectordb(&format!("{label}, groups"), &groups, &VECTORDB_GROUP_LABELS).await;
                let lists = play_chain(&format!("{label}, list chain"), &[
                    stashing(urn, VECTORDB_CTOR, LIST_STASH, LIST_RECORDS),
                    with_stash(LIST_STASH, LIST_RECEIVE),
                ])
                .await;
                let list = lists.last().expect("the list chain plays its receive");
                assert!(
                    delivers(list, "list"),
                    "{label}: the list chain delivers the list: {:?}",
                    list.outputs
                );
                // Changed by decision 101 (C5.4): rule CR-13 removed the COMM over `[D; m]`. The
                // list receive now fires in one bind and one `K_retire`, each over `[D]`.
                assert_list_fire(&label, &lists);
                let paths = space_program(urn, VECTORDB_CTOR, VECTORDB_LIST_PATHS);
                check_vectordb(&format!("{label}, LM-b"), &paths, &VECTORDB_LIST_LABELS).await;
                check_rows(&label, &paths).await;
                let lmb = play_chain(&format!("{label}, LM-b chain"), &[
                    stashing(urn, VECTORDB_CTOR, LMB_STASH, LMB_RECORDS),
                    with_stash(LMB_STASH, LMB_WAITERS),
                    with_stash(LMB_STASH, LMB_REMOVALS),
                ])
                .await;
                let [_, parked, removals] = lmb.as_slice() else {
                    panic!("{label}: the LM-b chain plays three deploys");
                };
                assert_parks(&format!("{label}, the LM-b waiters"), parked);
                for wanted in VECTORDB_LIST_LABELS {
                    assert!(
                        delivers(removals, wanted),
                        "{label}: the LM-b removals deliver {wanted}: {:?}",
                        removals.outputs
                    );
                }
                // Changed by decision 101 (C5.4): no COMM consumes over `[D; m]` since rule
                // CR-13, so this check passes for any fire. The new check finds the COMM of each
                // parked waiter in the removals.
                assert_offer_fires(&label, &lmb);
            }
        });
    }
}

// Changed by decision 93: since DR-132 a row is named `[(index, 0)]`, so `paths.parent` names no
// transaction, and the COMM rows of the receive and of its settle job have one parent. In one
// deploy, the waiters also park before the removal only in some racing orders. The test now
// plays the cap chain on each runtime of `RACING_WORKERS`, where 0 is the current-thread
// runtime, and counts every space COMM row.
// /// LIST-CASCADE past the round cap (decision 49) in native execution. Play runs the program to
// /// its end: the receive that removes `img` stops at the round cap, and its settle job fires the
// /// other waiters in a later transaction. Native replay reproduces the play.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_native_vectordb_cascade_carries_at_the_cap() {
//     let term = vectordb_cap_program();
//     let played = native_play(&term, AMPLE_LIMIT).await;
//     assert!(
//         played.result.errors.is_empty(),
//         "the cap program runs to its end: {:?}",
//         played.result.errors
//     );
//     // The sends and the parks make no COMM, so only the receive of `img` and its settle job
//     // stage COMM rows. The receive makes FIRE_ROUND_CAP fire rounds: its own fire, then one
//     // cascade fire after each lift. The settle job lifts and fires the other waiters.
//     let (recording, journal) = journal_of(&played);
//     let mut comms: Vec<(_, usize)> = Vec::new();
//     for row in journal {
//         if !is_space_row(recording, row) || row.comm.is_none() {
//             continue;
//         }
//         let parent = recording.paths.parent(row.occurrence.path);
//         match comms.iter_mut().find(|(known, _)| *known == parent) {
//             Some((_, count)) => *count += 1,
//             None => comms.push((parent, 1)),
//         }
//     }
//     let carried = CAP_WAITERS - (FIRE_ROUND_CAP - 1);
//     assert_eq!(
//         comms.iter().map(|(_, count)| *count).collect::<Vec<_>>(),
//         vec![2 * FIRE_ROUND_CAP - 1, 2 * carried],
//         "the receive stops at the round cap, and its settle job carries {carried} fires"
//     );
//     assert_eq!(
//         played.outputs.len(),
//         CAP_WAITERS + 1,
//         "one output for each document and one for img: {:?}",
//         played.outputs
//     );
//     let certification = certify(&played, &term, AMPLE_LIMIT).await;
//     assert_reproduces("the carry at the cap", &played, &certification);
// }

/// P6 step 5 and decision 93: LIST-CASCADE past the round cap (decision 49) in native
/// execution, as a chain of three deploys. The first deploy stores `img` and the documents, the
/// second parks the list waiters, and the third removes `img`. One transaction fires at most
/// `FIRE_ROUND_CAP` rounds, so the transaction of the receive delivers at most that many
/// outputs. Every document delivers, so the settle job of the receive carries the other fires.
/// On each runtime of `RACING_WORKERS`, where 0 is the current-thread runtime, each deploy
/// certifies through its own native replay.
#[test]
fn space_native_vectordb_cascade_carries_at_the_cap() {
    let records = stashing(
        "rho:space:hashmap:vectordb",
        CAP_CTOR,
        CAP_STASH,
        &cap_records(),
    );
    let waiters = with_stash(CAP_STASH, &cap_waiters());
    let removal = with_stash(CAP_STASH, CAP_REMOVAL);
    let carried = CAP_WAITERS - (FIRE_ROUND_CAP - 1);
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("the cap chain, {workers} workers");
            let mut chain = NativeChain::new().await;
            let stored = chain
                .play(&records, "rrs-native-cap-records", AMPLE_LIMIT)
                .await;
            assert!(
                stored.result.errors.is_empty(),
                "{label}: the records: {:?}",
                stored.result.errors
            );
            let certification = certify(&stored, &records, AMPLE_LIMIT).await;
            assert_reproduces(&format!("{label}, the records"), &stored, &certification);
            let parked = chain
                .play(&waiters, "rrs-native-cap-waiters", AMPLE_LIMIT)
                .await;
            assert_parks(&format!("{label}, the waiters"), &parked);
            let certification = certify(&parked, &waiters, AMPLE_LIMIT).await;
            assert_reproduces(&format!("{label}, the waiters"), &parked, &certification);
            let played = chain
                .play(&removal, "rrs-native-cap-removal", AMPLE_LIMIT)
                .await;
            assert!(
                played.result.errors.is_empty(),
                "{label}: the removal runs to its end: {:?}",
                played.result.errors
            );
            // The receive makes `FIRE_ROUND_CAP` fire rounds: its own fire, then one cascade fire
            // after each lift. The settle job lifts and fires the other waiters.
            let (_, journal) = journal_of(&played);
            assert_eq!(
                space_comms(journal),
                2 * FIRE_ROUND_CAP - 1 + 2 * carried,
                "{label}: the receive fires {FIRE_ROUND_CAP} rounds, and its settle job carries \
                 {carried} fires"
            );
            assert_eq!(
                played.outputs.len(),
                CAP_WAITERS + 1,
                "{label}: one output for each document and one for img: {:?}",
                played.outputs
            );
            let certification = certify(&played, &removal, AMPLE_LIMIT).await;
            assert_reproduces(&label, &played, &certification);
        });
    }
}

/// A semantic failure of a VectorDB operation aborts the deploy with its code in native play,
/// and native replay reproduces the failure: a send to a full collection (`CapacityExceeded`)
/// and a vector of the wrong length (`DimensionMismatch`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_vectordb_errors_reproduce() {
    for (label, ctor, body, code) in [
        (
            "capacity",
            r#""default"; capacity=2, dimensions=2"#,
            "a!([1.0, 0.0]) | a!([0.0, 1.0]) | a!([1.0, 1.0])",
            "CapacityExceeded",
        ),
        (
            "dimensions",
            VECTORDB_CTOR,
            "a!([1.0, 2.0, 3.0])",
            "DimensionMismatch",
        ),
    ] {
        let term = space_program("rho:space:hashmap:vectordb", ctor, body);
        let played = native_play(&term, AMPLE_LIMIT).await;
        let errors = format!("{:?}", played.result.errors);
        assert!(errors.contains(code), "{label}: {errors}");
        let certification = certify(&played, &term, AMPLE_LIMIT).await;
        assert_reproduces(label, &played, &certification);
    }
}

// Changed by decision 93: in one deploy, LM-b parks its waiters before the removals only in
// some racing orders (DR-132), and `check_denials` is gone. The test now plays LM-b as a chain
// of three deploys and probes the removals.
// /// Out of phlo at each charged budget attempt of the largest VectorDB transaction of LM-b,
// /// native play and native replay agree.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_native_vectordb_denial_replay() {
//     check_denials(&space_program(
//         "rho:space:hashmap:vectordb",
//         VECTORDB_CTOR,
//         VECTORDB_LIST_PATHS,
//     ))
//     .await
// }

/// The stash of the LM-b chain of `space_native_vectordb_denial_replay`.
const LMB_STASH: &str = "rrs-native-lmb";

/// The records of LM-b as one deploy: `x1` on `a`, and `img` and two documents on `b`.
const LMB_RECORDS: &str = r#"a!([1.0, 0.0]) |
b!("img"; embedding=[10.0, 1.0]) | b!("doc"; embedding=[10.0, 2.0]) | b!("doc"; embedding=[10.0, 3.0])"#;

/// The list waiters of LM-b as one deploy. The waiter of CE-1 refuses the list `[x1]`, and the
/// waiter of CE-2 refuses `[img, doc]`, so both park.
const LMB_WAITERS: &str = r#"for (@[p, q] <- a ~ ([1.0, 0.0]; k=2)) { out!(["pair", p, q]) } |
for (@["doc", "doc"] <- b ~ ([1.0, 0.0]; k=2)) { out!(["docs", 2]) }"#;

/// The removals of LM-b as one deploy. The send of `x2` fires the waiter of CE-1 with the
/// incoming datum. The receive of `img` removes it, so LIST-CASCADE fires the waiter of CE-2
/// with `[doc, doc]`.
const LMB_REMOVALS: &str =
    r#"a!([1.0, 0.5]) | for (@"img" <- b ~ ([1.0, 0.0])) { out!(["img", 1]) }"#;

/// The label of the removals. The reference and every probe have the random state of this
/// label.
const LMB_REMOVALS_LABEL: &str = "rrs-native-lmb-removals";

/// The witness of `space_native_vectordb_denial_replay`: an earlier row of `journal` names a
/// data channel of row `index`, so the row follows an earlier operation on its own collection.
fn follows_on_its_collection(journal: &[NativeOperationRecord], index: usize) -> bool {
    let channels = data_channels(&journal[index]);
    journal[..index].iter().any(|row| {
        data_channels(row)
            .iter()
            .any(|channel| channels.contains(channel))
    })
}

/// P6 step 5 and decision 93: LM-b as a chain of three deploys. The first deploy stores the
/// records, and the second parks the two list waiters. The third makes the removals, and its
/// play at [`AMPLE_LIMIT`] is the reference. Out of phlo at each charged budget attempt of a
/// space row of the removals, native play and native replay agree, as in
/// `space_native_denial_replay`. Each probe plays the removals again from the root of the parked
/// waiters. Non-vacuity: some denied row follows an earlier row on its own collection.
#[test]
fn space_native_vectordb_denial_replay() {
    let records = stashing(
        "rho:space:hashmap:vectordb",
        VECTORDB_CTOR,
        LMB_STASH,
        LMB_RECORDS,
    );
    let waiters = with_stash(LMB_STASH, LMB_WAITERS);
    let removals = with_stash(LMB_STASH, LMB_REMOVALS);
    let mut denials = Denials::default();
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("LM-b, {workers} workers");
            let mut chain = NativeChain::new().await;
            let stored = chain
                .play(&records, "rrs-native-lmb-records", AMPLE_LIMIT)
                .await;
            assert!(
                stored.result.errors.is_empty(),
                "{label}: the records: {:?}",
                stored.result.errors
            );
            let certification = certify(&stored, &records, AMPLE_LIMIT).await;
            assert_reproduces(&format!("{label}, the records"), &stored, &certification);
            let parked = chain
                .play(&waiters, "rrs-native-lmb-waiters", AMPLE_LIMIT)
                .await;
            assert_parks(&format!("{label}, the waiters"), &parked);
            let certification = certify(&parked, &waiters, AMPLE_LIMIT).await;
            assert_reproduces(&format!("{label}, the waiters"), &parked, &certification);
            let reference = chain.play(&removals, LMB_REMOVALS_LABEL, AMPLE_LIMIT).await;
            for wanted in VECTORDB_LIST_LABELS {
                assert!(
                    delivers(&reference, wanted),
                    "{label}: the removals deliver {wanted}: {:?}",
                    reference.outputs
                );
            }
            let (_, journal) = journal_of(&reference);
            assert!(
                (0..journal.len()).any(|index| {
                    is_space_row(&journal[index]) && follows_on_its_collection(journal, index)
                }),
                "{label}: some space row of the removals follows an earlier row on its collection"
            );
            let limits = denial_limits(&label, &reference);
            let certification = certify(&reference, &removals, AMPLE_LIMIT).await;
            assert_reproduces(&label, &reference, &certification);
            for (target, limit) in limits {
                let label = format!("{label}, attempt {target} (limit {limit})");
                chain.reset(&parked.root).await;
                let probe = chain.play(&removals, LMB_REMOVALS_LABEL, limit).await;
                judge_probe(
                    &label,
                    &probe,
                    limit,
                    follows_on_its_collection,
                    &mut denials,
                );
                let certification = certify(&probe, &removals, limit).await;
                assert_reproduces(&label, &probe, &certification);
            }
        });
    }
    denials.assert_non_vacuous("LM-b");
}

// ─── Allocations (P9 step 2) ───────────────────────────────────────────────────────────────────

/// Regression of P9 step 2. An allocation job took the cost authority of the process that
/// evaluates the `new`, and that authority is empty for a root term and for the body of a fired
/// continuation. So the allocator record of the job had no cost authority. The next `new` in a
/// `use` block of the space then made a COMM over that record with an empty merged authority,
/// which is not billable, and native play failed with an incomplete recording. A job now takes
/// the authority of the call `s!gensym()` (`ReducerCore::call_authority`). On an Array and a
/// Vector space, two deploys with one `new` each and one deploy with two nested `new`s play
/// natively and certify, and each allocator record carries a cost authority.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_allocations_keep_an_authority() {
    for (store, urn, ctor) in [
        ("array", "rho:space:array:queue", r#""default"; size=8"#),
        ("vector", "rho:space:vector:queue", r#""default""#),
    ] {
        let mut chain = support::native::NativeChain::new().await;
        let setup = format!(
            r#"new factory(`{urn}`) in {{ for (space <- factory!?({ctor})) {{ @"rrs-p9-allocations"!(*space) }} }}"#
        );
        let deploys = [
            setup.as_str(),
            r#"for (space <<- @"rrs-p9-allocations") { use space { new n in { Nil } } }"#,
            r#"for (space <<- @"rrs-p9-allocations") { use space { new n in { Nil } } }"#,
            r#"for (space <<- @"rrs-p9-allocations") { use space { new n in { new m in { Nil } } } }"#,
        ];
        let mut space = None;
        for (step, term) in deploys.into_iter().enumerate() {
            let label = format!("rrs-p9-allocations-{store}-{step}");
            let played = chain.play(term, &label, AMPLE_LIMIT).await;
            assert!(
                played.result.errors.is_empty(),
                "{label}: native play fails: {:?}",
                played.result.errors
            );
            let certification = certify(&played, term, AMPLE_LIMIT).await;
            assert_reproduces(&label, &played, &certification);
            if space.is_none() {
                let stash = chain.values("rrs-p9-allocations").await;
                assert_eq!(stash.len(), 1, "{label}: one datum holds the handle");
                space = Some(support::session::native(&stash[0][0]));
            }
            let native = space.expect("the setup stores the handle");
            let records = chain.data(&allocator::allocator_channel(native)).await;
            assert_eq!(records.len(), 1, "{label}: one allocator record");
            assert!(
                records[0]
                    .a
                    .cost_authority
                    .as_ref()
                    .is_some_and(|authority| !authority.regions.is_empty()),
                "{label}: the allocator record carries a cost authority"
            );
        }
    }
}

// ─── Anchors (decision 92) ─────────────────────────────────────────────────────────────────────

/// The kinds of the channels of the footprint of `row`, sorted: `C` for a config channel, `S`
/// for a space counter, and `?` for every other channel.
fn footprint_kinds(row: &NativeOperationRecord) -> Vec<&'static str> {
    let mut kinds: Vec<&'static str> = footprint_channels(row)
        .iter()
        .map(|channel| match namespace::reified_id_of(channel) {
            Some(ReifiedId::Config(_)) => "C",
            Some(ReifiedId::SpaceCounter(_)) => "S",
            _ => "?",
        })
        .collect();
    kinds.sort_unstable();
    kinds
}

/// The consume rows of `journal` whose footprint has exactly the kinds `kinds`.
fn consumes_with<'a>(
    journal: &'a [NativeOperationRecord],
    kinds: &[&str],
) -> Vec<&'a NativeOperationRecord> {
    journal
        .iter()
        .filter(|row| matches!(row.source, NativeOperationSource::Consume(_)))
        .filter(|row| footprint_kinds(row) == kinds)
        .collect()
}

/// The anchor rows of `journal`: the consumes without a peek whose footprint names `C(sid)` and
/// `S(sid)` only.
fn anchors_of(journal: &[NativeOperationRecord]) -> Vec<&NativeOperationRecord> {
    consumes_with(journal, &["C", "S"])
        .into_iter()
        .filter(|row| matches!(row.consume_peeks.as_deref(), None | Some([])))
        .collect()
}

/// Decision 92: a send to an occupied cell fails at a position of the state, so its transaction
/// commits the anchor of its job only. The anchor row is a consume on `C(sid)`, and its footprint
/// also names `S(sid)`. The row links one consume introduction and one COMM. The introduction
/// measures what the take of the space counter measures, as decision 92 accepts. The COMM
/// transfers the config record. Native replay reproduces the play, the failure included.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_native_anchor_charges_one_consume_introduction_and_one_comm() {
    let term = space_program("rho:space:hashmap:cell", "\"default\"", "a!(1) | a!(2)");
    let played = native_play(&term, AMPLE_LIMIT).await;
    let errors = format!("{:?}", played.result.errors);
    assert!(errors.contains("CellOccupied"), "{errors}");
    let (recording, journal) = journal_of(&played);
    let anchors = anchors_of(journal);
    let [anchor] = anchors.as_slice() else {
        panic!("one send fails, so one anchor: {}", anchors.len());
    };
    let takes = consumes_with(journal, &["S"]);
    let [take] = takes.as_slice() else {
        panic!("one send opens the collection: {}", takes.len());
    };
    assert!(is_space_row(anchor));
    let attempt = |index: usize| &recording.attempts[index];
    let [introduction, comm] = row_attempts(anchor)[..] else {
        panic!("the anchor links an introduction and a COMM");
    };
    let [take_introduction, _] = row_attempts(take)[..] else {
        panic!("the take links an introduction and a COMM");
    };
    assert_eq!(
        attempt(introduction).observation.kind,
        AuthorityByteEventKind::ConsumeIntroduction
    );
    assert_eq!(attempt(comm).observation.kind, AuthorityByteEventKind::Comm);
    assert!(attempt(introduction).granted && attempt(comm).granted);
    assert_eq!(
        attempt(introduction).observation.measurement,
        attempt(take_introduction).observation.measurement,
        "the anchor introduces what the take of the space counter introduces"
    );
    let config_channel = footprint_channels(anchor)
        .into_iter()
        .find(|channel| {
            matches!(
                namespace::reified_id_of(channel),
                Some(ReifiedId::Config(_))
            )
        })
        .expect("the anchor names C(sid)");
    let config = played
        .history
        .get_history_reader(&played.root)
        .expect("read the post-state")
        .get_data(&hash(&config_channel))
        .expect("the data of C(sid)");
    let [record] = config.as_slice() else {
        panic!("one config record: {}", config.len());
    };
    assert!(record.persist, "the anchor keeps the config record");
    assert_eq!(
        attempt(comm).observation.measurement,
        Some(ByteCharge {
            introduction_bytes: 0,
            transfer_bytes: datum_transfer_bytes(&record.a).expect("the record bytes"),
            trace_bytes: comm_trace_bytes(1).expect("the trace bytes of one channel"),
        })
    );
    let certification = certify(&played, &term, AMPLE_LIMIT).await;
    assert_reproduces("anchor", &played, &certification);
}

// ─── Racing replay (decisions 92 and 93) ───────────────────────────────────────────────────────

/// The runs of each racing family on each runtime (decision 93).
const RACING_RUNS: usize = 3;

// Changed by decision 111: the racing runtimes also hold the current-thread runtime, as the base
// runs its racing tests (`random_candidate_order.rs`, P-1 and P-6 of DR-132).
/// The worker threads of the racing runtimes, where 0 is the current-thread runtime (decision
/// 111). Each run replays its own record, and no test needs the current-thread runtime to pass.
const RACING_WORKERS: [usize; 3] = [0, 2, 8];

/// The Display form of `NativeReplayError::Stalled` (`trace/replay.rs`). A stall reaches a test
/// as this text in an interpreter error.
const STALL: &str = "native replay stalled";

/// The body of a cell race. One of the two sends on `a` fails at the occupied cell, so its job
/// commits an anchor only. The send and the receive on `b` race with it.
const CELL_RACE: &str = r#"a!(1) | a!(2) | b!(5) | for (@y <- b) { out!(["take", y]) }"#;

/// The body of a persistent send that three receives take.
const PERSISTENT_SEND: &str = r#"a!!(1) | for (@x <- a) { out!(["o1", x]) } |
for (@y <- a) { out!(["o2", y]) } | for (@z <- a) { out!(["o3", z]) }"#;

/// The body of a race on one collection: `pairs` sends on `a`, and one receive on `a` for each
/// send.
fn collection_race(pairs: usize) -> String {
    (1..=pairs)
        .map(|value| format!(r#"a!({value}) | for (@x <- a) {{ out!(["take", x]) }}"#))
        .collect::<Vec<_>>()
        .join(" |\n")
}

/// The families of the racing properties, each with a label and a deploy. Each family races the
/// space transactions of one space or of two spaces. The families hold a positional failure,
/// many stores and fires on one collection, joins, peeks, and persistent sends and receives. A
/// family holds at most one failure, because the aggregate of two failures keeps the order in
/// which their tasks end.
///
/// Added by decision 124 (L1, plan-join-batch Step 7 item 4): the families also hold a set
/// cascade, whose guarded joins park in the racing orders where their random picks take class 0
/// and then fire in the cascade of a later removal, and a VectorDB cascade: the cap program of
/// `space_native_vectordb_cascade_carries_at_the_cap` as one deploy. Its list waiters park
/// before the removal of `img` only in some racing orders, so LIST-CASCADE runs in some runs
/// only. One deploy rarely parks enough waiters to reach the round cap, which the chain of that
/// test reaches.
fn racing_families() -> Vec<(&'static str, String)> {
    let default = "\"default\"";
    vec![
        (
            "a cell race with a positional failure",
            space_program("rho:space:hashmap:cell", default, CELL_RACE),
        ),
        (
            "sixteen sends and sixteen receives on one queue",
            space_program("rho:space:hashmap:queue", default, &collection_race(16)),
        ),
        (
            "two joins race their produces on a bag",
            space_program(
                "rho:space:hashmap:bag",
                default,
                r#"for (@x <- a & @y <- b) { out!(["join", x, y]) } |
for (@x <- a & @y <- b) { out!(["join", x, y]) } | a!(1) | b!(2) | a!(3) | b!(4)"#,
            ),
        ),
        (
            "two peeks and a take on a stack",
            space_program(
                "rho:space:hashmap:stack",
                default,
                r#"a!(1) | a!(2) | for (@x <<- a) { out!(["peek", x]) } |
for (@y <<- a) { out!(["peek", y]) } | for (@z <- a) { out!(["take", z]) }"#,
            ),
        ),
        (
            "a persistent send and three receives on a bag",
            space_program("rho:space:hashmap:bag", default, PERSISTENT_SEND),
        ),
        (
            "a persistent send and three receives on a set",
            space_program("rho:space:hashmap:set", default, PERSISTENT_SEND),
        ),
        (
            "a persistent receive on a priority queue",
            space_program(
                "rho:space:hashmap:priorityqueue",
                default,
                r#"a!(1; priority=1) | a!(2; priority=0) | a!(3) |
for (@x <= a) { out!(["each", x]) }"#,
            ),
        ),
        (
            "two receives race for the prefix of a PathMap",
            space_program(
                "rho:space:pathmap:bag",
                default,
                r#"use space { @[*a, 1]!(5) | @[*a, 2]!(6) } |
for (@v <- a) { out!(["prefix", v]) } | for (@w <- a) { out!(["prefix", w]) }"#,
            ),
        ),
        (
            "a cell race and a queue race in two spaces",
            format!(
                "{} |\n{}",
                space_program("rho:space:hashmap:cell", default, CELL_RACE),
                space_program("rho:space:hashmap:queue", default, &collection_race(4)),
            ),
        ),
        // Added by decision 124 (L1, plan-join-batch Step 7 item 4): a set cascade. A guarded
        // join parks when its random pick takes class 0, and the cascade of a later removal fires
        // it on the DR-75 tuple once that tuple passes its guard: after the removal of class 0
        // when class 0 comes first in DR-75. This happens in the racing orders where the joins
        // park before the removals.
        (
            "two guarded joins and the removal of class 0 on a set",
            space_program("rho:space:hashmap:set", default, SET_CASCADE_RACE),
        ),
        // Added by decision 124 (L1, plan-join-batch Step 7 item 4): the VectorDB cascade of the
        // cap program, whose records, list waiters and removal of `img` race. LIST-CASCADE runs
        // in the racing orders where list waiters park before the removal.
        (
            "the VectorDB cascade of the cap program",
            space_program(
                "rho:space:hashmap:vectordb",
                CAP_CTOR,
                &format!("{} |\n{} |\n{CAP_REMOVAL}", cap_records(), cap_waiters()),
            ),
        ),
    ]
}

/// Added by decision 124 (L1, plan-join-batch Step 7 item 4): the body of the set cascade family
/// of [`racing_families`]: two members of class 0, the values 1 and 2 on `a` and the values 7
/// and 8 on `b`, two joins that admit `x != 0`, and the receive of class 0, which retires the
/// class.
const SET_CASCADE_RACE: &str = r#"a!(0) | a!(0) | a!(1) | a!(2) | b!(7) | b!(8) |
for (@x <- a & @y <- b where x != 0) { out!(["join", 1, x, y]) } |
for (@x <- a & @y <- b where x != 0) { out!(["join", 2, x, y]) } |
for (@0 <- a) { out!(["zero", 0]) }"#;

/// Plays each racing family `RACING_RUNS` times on a runtime with each worker count of
/// `RACING_WORKERS`, and certifies each play through its own committed evidence. A run that
/// breaks `invariant` adds the reason of `check` to the failures, and the property holds when
/// no run fails. Non-vacuity: some family shows two or more journal orders (DR-132). Each run
/// whose play fails at an occupied cell commits an anchor, the witness `NoFailureVerdict`.
fn check_racing(invariant: &str, check: fn(&Played, &Certification) -> Result<(), String>) {
    let families = racing_families();
    let mut orders: BTreeMap<&str, BTreeSet<Vec<String>>> = BTreeMap::new();
    let mut anchored = 0_usize;
    let mut failures = Vec::new();
    for workers in RACING_WORKERS {
        let runtime = racing_runtime(workers);
        for (family, term) in &families {
            for run in 0..RACING_RUNS {
                let label = format!("{family}, {workers} workers, run {run}");
                let (played, certification) = runtime.block_on(async {
                    let played = native_play(term, AMPLE_LIMIT).await;
                    let certification = certify(&played, term, AMPLE_LIMIT).await;
                    (played, certification)
                });
                let (_, journal) = journal_of(&played);
                orders.entry(*family).or_default().insert(
                    journal
                        .iter()
                        .map(|row| format!("{:?}", row.source))
                        .collect(),
                );
                match (
                    format!("{:?}", played.result.errors).contains("CellOccupied"),
                    anchors_of(journal).is_empty(),
                ) {
                    (true, true) => {
                        failures.push(format!("{label}: a positional failure commits no anchor"))
                    }
                    (true, false) => anchored += 1,
                    (false, _) => {}
                }
                if let Err(reason) = check(&played, &certification) {
                    failures.push(format!("{label}: {reason}"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{invariant}: {} failures: {failures:#?}",
        failures.len()
    );
    assert!(
        anchored > 0,
        "{invariant}: no run fails at an occupied cell"
    );
    assert!(
        orders.values().any(|seen| seen.len() >= 2),
        "{invariant}: no family shows two journal orders: {:?}",
        orders
            .iter()
            .map(|(family, seen)| (family, seen.len()))
            .collect::<Vec<_>>()
    );
}

/// NoStall: native replay declares no stall, in its evaluation or in its later checks.
fn no_stall(_: &Played, certification: &Certification) -> Result<(), String> {
    let evaluation = match &certification.evaluation {
        Ok(evaluation) => format!("{:?}", evaluation.errors),
        Err(error) => format!("{error:?}"),
    };
    let results = [
        evaluation,
        format!("{:?}", certification.complete),
        format!("{:?}", certification.usage),
        format!("{:?}", certification.root),
    ];
    match results.iter().find(|result| result.contains(STALL)) {
        Some(result) => Err(format!("native replay declares a stall: {result}")),
        None => Ok(()),
    }
}

/// NoFailClosed: native replay evaluates, each failure of its deploy is a user failure, and it
/// exports a post-state root.
fn no_fail_closed(_: &Played, certification: &Certification) -> Result<(), String> {
    let evaluation = certification
        .evaluation
        .as_ref()
        .map_err(|error| format!("native replay fails: {error:?}"))?;
    let failures = classify_errors(&evaluation.errors).map_err(|error| error.to_string())?;
    let closed: Vec<PhloFailure> = [
        PhloFailure::Platform,
        PhloFailure::Certificate,
        PhloFailure::Unclassified,
    ]
    .into_iter()
    .filter(|failure| failures.contains(*failure))
    .collect();
    match (closed.as_slice(), &certification.root) {
        ([], Ok(_)) => Ok(()),
        ([], Err(error)) => Err(format!("native replay exports no root: {error}")),
        (closed, _) => Err(format!(
            "native replay fails closed ({closed:?}): {:?}",
            evaluation.errors
        )),
    }
}

/// Agreement: native replay ends with the verdict of play and reaches the state of play. The
/// errors, the usage, the native phlo usage, the authority events, the byte observations and
/// the post-state root of replay are those of play.
fn agreement(played: &Played, certification: &Certification) -> Result<(), String> {
    let evaluation = certification
        .evaluation
        .as_ref()
        .map_err(|error| format!("native replay fails: {error:?}"))?;
    let recording = played
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget");
    let differences: Vec<&str> = [
        ("errors", evaluation.errors == played.result.errors),
        ("usage", certification.usage == Ok(recording.used)),
        (
            "native phlo usage",
            evaluation.native_phlo_usage == played.result.native_phlo_usage,
        ),
        (
            "authority events",
            evaluation.authority_events == played.result.authority_events,
        ),
        (
            "byte observations",
            evaluation.byte_observations.rows == played.result.byte_observations.rows,
        ),
        (
            "post-state root",
            certification.root.as_ref() == Ok(&played.root),
        ),
    ]
    .into_iter()
    .filter_map(|(part, equal)| (!equal).then_some(part))
    .collect();
    match differences.as_slice() {
        [] => Ok(()),
        differences => Err(format!(
            "native replay differs from play in {differences:?}"
        )),
    }
}

/// ExactRows: native replay completes every journal row of play.
fn exact_rows(_: &Played, certification: &Certification) -> Result<(), String> {
    certification
        .complete
        .clone()
        .map_err(|error| format!("native replay leaves journal rows open: {error}"))
}

/// Decision 92, NoStall of `SpaceReplayOrder.tla`: the racing replay of an honest journal never
/// declares a stall. A job whose first row is not ready waits for a change, and the chain of
/// `S(sid)` makes that row ready.
#[test]
fn space_racing_replay_never_stalls() { check_racing("NoStall", no_stall) }

/// Decision 92, NoFailClosed of `SpaceReplayOrder.tla`: the racing replay of an honest journal
/// never fails closed. A job stages from the state that play gave it, so each later row of its
/// transaction fits.
#[test]
fn space_racing_replay_never_fails_closed() { check_racing("NoFailClosed", no_fail_closed) }

/// Decision 92, Agreement of `SpaceReplayOrder.tla`: each racing replay ends with the verdict of
/// its play and reaches the post-state of its play. A positional failure repeats through its
/// anchor.
#[test]
fn space_racing_replay_agrees_with_play() { check_racing("Agreement", agreement) }

/// Decision 92, ExactRows of `SpaceReplayOrder.tla`: each racing replay completes every journal
/// row of its play.
#[test]
fn space_racing_replay_completes_every_row() { check_racing("ExactRows", exact_rows) }
