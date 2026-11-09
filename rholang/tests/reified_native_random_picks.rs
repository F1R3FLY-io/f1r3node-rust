//! Added by decision 102: the random picks of play, certified through native replay. Each test
//! plays Rholang deploys natively (`support::native`). Each test runs on the racing runtimes
//! with 0, 2 and 8 workers (decision 111), and each run replays its own record (rule CR-28).
//!
//! * The tests with [`RUNS`] show that play picks each of three equal candidates (rule CR-1).
//! * The tests with [`AGREEMENT_RUNS`] cover a join that names one collection twice (E1) and a
//!   receive whose guard parks it under one of its picks (E3-a, rule CR-7).
//! * `prop_space_random_play_agrees_with_native_replay` plays [`F6_CASES`] cases over the
//!   families of ambiguous deploys.
//! * `space_vetoed_bag_join_on_a_repeated_collection_restores_every_datum` covers a refused
//!   join on a repeated collection ([`ROLLBACK_RUNS`]).
//! * `space_guarded_join_park_with_a_theory_replays` covers a guarded join in a space with a
//!   theory (G9).
//! * `set_cascade_fires_the_guarded_receive_that_a_vetoed_pick_parked` covers the set cascade
//!   after a refused pick (GF-3, GF-5).
//!
//! Other files hold the other tests of this suite.
//! `native_runtime/tests/random_candidate_order.rs` holds the racing property, because only the
//! tests of the crate can read the progress word of native replay.
//! `reified_native_owner_rows.rs` holds the forged records and the R-A chains, because their
//! helpers are private to that suite. The unit tests of `spaces/generic.rs` and
//! `spaces/pathmap/join.rs` hold the R-A tests of `SeqConcurrent` and of the PathMap source.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, ListParWithRandom, Par};
use rholang::rust::interpreter::accounting::{NativeOperationRecord, NativeOperationSource};
use rholang::rust::interpreter::spaces::record;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use support::family_programs::space_program;
use support::native::{
    assert_reproduces, certify, is_space_row, NativeChain, Played, AMPLE_LIMIT, OUT,
};
use support::props::{racing_runtime, RACING_WORKERS};

/// The runs of a test of a pick among three equal candidates on each runtime. One candidate is
/// missing from every run with a chance of at most `3 * (2/3)^200`, about `2 * 10^-35`.
const RUNS: usize = 200;

/// The runs of an agreement test on each runtime. An outcome with a chance of 1/2 is missing
/// from every run with a chance of `2^-64`.
const AGREEMENT_RUNS: usize = 64;

/// The cases of the agreement property on each runtime. Case `i` belongs to the family `i`
/// modulo the count of [`FAMILIES`], so each family gets 39 or 40 cases. A family of two
/// candidates shows one pick with a chance of at most `2 * 2^-39`. The union over the 13
/// families is about `4.7 * 10^-11` on each runtime.
const F6_CASES: usize = 512;

/// The cases of [`GUARD_FAMILY`] on each runtime, outside [`F6_CASES`]. This family has one
/// outcome, so it adds nothing to the miss bound of [`F6_CASES`].
const F6_GUARD_CASES: usize = 40;

/// The runs of the rollback test on each runtime. The send meets the refused join first with a
/// chance of 1/2 in each run. No run meets it first with a chance of `2^-64`.
const ROLLBACK_RUNS: usize = 64;

/// The runs of a test that needs a park on each runtime. No run parks with a chance of `2^-64`.
const PARK_RUNS: usize = 64;

/// The public channel that holds the handle of the space and the collections `a` and `b`.
const STASH: &str = "rnp-stash";

const BAG: &str = "rho:space:hashmap:bag";
const SET: &str = "rho:space:hashmap:set";
const QUEUE: &str = "rho:space:hashmap:queue";
const STACK: &str = "rho:space:hashmap:stack";
const CELL: &str = "rho:space:hashmap:cell";
const PRIORITY_QUEUE: &str = "rho:space:hashmap:priorityqueue";
const VECTORDB: &str = "rho:space:hashmap:vectordb";
const PATHMAP_BAG: &str = "rho:space:pathmap:bag";
const DEFAULT: &str = r#""default""#;
const VECTORDB_CTOR: &str = r#""default"; capacity=16, dimensions=2"#;
const THEORY_CTOR: &str = r#""default", free Nat()"#;

const THREE_RECORDS: &str = "a!(1) | a!(2) | a!(3)";
const TAKE: &str = r#"for (@x <- a) { out!(["took", x]) }"#;
const THREE_WAITERS: &str = r#"for (@x <- a) { out!(["w1", x]) } |
for (@x <- a) { out!(["w2", x]) } |
for (@x <- a) { out!(["w3", x]) }"#;
const SEND_NINE: &str = "a!(9)";
const JOIN_RECORDS: &str = "a!(1) | a!(2) | a!(3) | b!(10)";
const JOIN_ONCE: &str = r#"for (@x <- a & @y <- b) { out!(["join", x, y]) }"#;

/// One join that names `a` twice. A contract names `a` through its parameters, because the
/// normalizer refuses a join that names one channel twice in its text.
const JOIN_ON_A_TWICE: &str = r#"new twice in {
  contract twice(c1, c2) = { for (@x <- c1 & @y <- c2) { out!(["join", x, y]) } } |
  twice!(*a, *a)
}"#;

/// Three records that tie at the best score for the query `[1.0, 0.0]`, and one worse record.
const TIED_RECORDS: &str = r#"a!("c1"; embedding=[10.0, 0.0]) | a!("c2"; embedding=[10.0, 0.0]) |
a!("c3"; embedding=[10.0, 0.0]) | a!("z"; embedding=[0.0, 1.0])"#;
const RANKED_TAKE: &str = r#"for (@x <- a ~ ([1.0, 0.0])) { out!(["took", x]) }"#;

/// Three records with different scores. A read without a query does not use the scores.
const SCORED_RECORDS: &str = r#"a!("c1"; embedding=[10.0, 0.0]) | a!("c2"; embedding=[10.0, 1.0]) |
a!("c3"; embedding=[10.0, 5.0])"#;
const TWO_WAITERS: &str = r#"for (@x <- a) { out!(["w1", x]) } |
for (@x <- a) { out!(["w2", x]) }"#;
const VECTOR_SEND: &str = r#"a!("x"; embedding=[1.0, 0.0])"#;
const PATH_SEND: &str = "use space { @[*a, 1]!(5) }";
const PRIORITY_SENDS: &str = "a!(1; priority=1) | a!(2; priority=0)";

/// Two waiters whose guards admit different values. The send of 2 meets them in a random order.
const GUARD_WAITERS: &str = r#"for (@x <- a where x == 1) { out!(["refuses", x]) } |
for (@x <- a where x == 2) { out!(["admits", x]) }"#;
const SEND_TWO: &str = "a!(2)";

/// Two joins on `[a, a]` in one deploy body. A contract names `a` through its parameters. The
/// guard of the first join refuses every tuple.
const REPEATED_JOINS: &str = r#"new refused, admitted in {
  contract refused(c1, c2) = {
    for (@x <- c1 & @y <- c2 where x == "never") { out!(["vetoed", x, y]) }
  } |
  contract admitted(c1, c2) = { for (@x <- c1 & @y <- c2) { out!(["accepted", x, y]) } } |
  refused!(*a, *a) | admitted!(*a, *a)
}"#;

const CASCADE_RECORDS: &str = r#"a!("a") | a!("b")"#;

/// A join that never fires. Its group holds the channel of `a`, so each later operation on `a`
/// runs in the join engine. Only the join engine has the set cascade.
const NEVER_JOIN: &str = r#"for (@"none" <- a & @y <- b) { out!(["never", y]) }"#;
const GUARDED_RECEIVE: &str = r#"for (@x <- a where x == "b") { out!(["r1", x]) }"#;
const EXACT_RECEIVE: &str = r#"for (@"a" <- a) { out!(["r2", "a"]) }"#;

/// A deploy that creates a space of the family `urn` with the constructor `ctor`, stores the
/// handle and the collections `a` and `b` on [`STASH`], and runs `state`.
fn stashing(urn: &str, ctor: &str, state: &str) -> String {
    space_program(
        urn,
        ctor,
        &format!(r#"@"{STASH}"!(*space, *a, *b) | {state}"#),
    )
}

/// A deploy that runs `body` over the handle `space` and the collections `a` and `b` on
/// [`STASH`]. The values on `out` go to the public channel [`OUT`].
fn with_stash(body: &str) -> String {
    format!(
        r#"new out in {{
  for (space, a, b <<- @"{STASH}") {{
    {body}
  }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#
    )
}

/// The rows of the journal of `played`.
fn journal_of(played: &Played) -> &[NativeOperationRecord] {
    played
        .result
        .native_operation_recording
        .as_ref()
        .expect("native play records its operations")
        .as_ref()
}

/// Plays `term` on `chain` with the random state of `label`. The deploy certifies through its
/// own native replay (rule CR-28).
async fn play_certified(chain: &mut NativeChain, term: &str, label: &str) -> Played {
    let played = chain.play(term, label, AMPLE_LIMIT).await;
    let certification = certify(&played, term, AMPLE_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    played
}

/// Checks that the deploy `played` runs to its end.
fn assert_runs(label: &str, played: &Played) {
    assert!(
        played.result.errors.is_empty(),
        "{label}: {:?}",
        played.result.errors
    );
}

/// The source hashes of the waiters that the deploy `played` parks: its space consume rows
/// without a COMM.
fn parked_waiters(played: &Played) -> BTreeSet<[u8; 32]> {
    let mut waiters = BTreeSet::<[u8; 32]>::new();
    for row in journal_of(played) {
        match (&row.source, &row.comm) {
            (NativeOperationSource::Consume(waiter), None) if is_space_row(row) => {
                waiters.insert(waiter.hash);
            }
            (NativeOperationSource::Consume(_), _) | (NativeOperationSource::Produce(_), _) => {}
        }
    }
    waiters
}

/// The count of space produce rows of `journal` whose COMM names one of the waiters `waiters`.
fn fires_of(journal: &[NativeOperationRecord], waiters: &BTreeSet<[u8; 32]>) -> usize {
    let mut fires = 0_usize;
    for row in journal {
        match (&row.source, &row.comm) {
            (NativeOperationSource::Produce(_), Some(comm))
                if is_space_row(row) && waiters.contains(&comm.source.consume.hash) =>
            {
                fires += 1;
            }
            (NativeOperationSource::Produce(_), _) | (NativeOperationSource::Consume(_), _) => {}
        }
    }
    fires
}

/// The data channel on which the deploy `played` stores `count` records: its space produce rows
/// without a COMM.
fn stored_channel(played: &Played, count: usize) -> [u8; 32] {
    let mut held = BTreeMap::<[u8; 32], usize>::new();
    for row in journal_of(played) {
        match (&row.source, &row.comm) {
            (NativeOperationSource::Produce(record), None) if is_space_row(row) => {
                *held.entry(record.channel).or_insert(0) += 1;
            }
            (NativeOperationSource::Produce(_), _) | (NativeOperationSource::Consume(_), _) => {}
        }
    }
    let mut channels = Vec::<[u8; 32]>::with_capacity(held.len());
    for (channel, records) in held {
        if records == count {
            channels.push(channel);
        }
    }
    let [channel] = channels.as_slice() else {
        panic!("one channel holds {count} stored records: {channels:?}");
    };
    *channel
}

/// The records on the data channel `channel` in the post-state of `played`, in the canonical
/// order of DR-75: each source hash with the value that `value_of` reads from the record.
fn stored_values(
    played: &Played,
    channel: &[u8; 32],
    value_of: fn(&ListParWithRandom) -> Par,
) -> Vec<([u8; 32], Par)> {
    let data = played
        .history
        .get_history_reader(&played.root)
        .expect("read the post-state")
        .get_data(&Blake2b256Hash::from_bytes(channel.to_vec()))
        .expect("the records of the channel");
    let mut values = Vec::<([u8; 32], Par)>::with_capacity(data.len());
    for datum in &data {
        let source: [u8; 32] = datum
            .source
            .hash
            .bytes()
            .try_into()
            .expect("a source hash has 32 bytes");
        values.push((source, value_of(&datum.a)));
    }
    values.sort_by_key(|(source, _)| *source);
    values
}

/// The value of a stored record of a bag or a set: the one datum of its send.
fn set_value(record: &ListParWithRandom) -> Par {
    let unwrapped = record::unwrap_datum(record).expect("a set record unwraps");
    match unwrapped.datum.pars.as_slice() {
        [value] => value.clone(),
        values => panic!(
            "a record of this suite holds one value, not {}",
            values.len()
        ),
    }
}

/// The integer `par`.
fn int_of(par: &Par) -> i64 {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }] => *value,
        _ => panic!("an integer: {par:?}"),
    }
}

/// The elements of the list `par`.
fn list_items(par: &Par) -> &[Par] {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EListBody(list)),
        }] => &list.ps,
        _ => panic!("a list: {par:?}"),
    }
}

/// The text of the delivery `value`, a list of strings and integers, with a space between items.
fn delivery_text(value: &Par) -> String {
    let items = list_items(value);
    let mut parts = Vec::<String>::with_capacity(items.len());
    for item in items {
        parts.push(match item.exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::GInt(number)),
            }] => number.to_string(),
            [Expr {
                expr_instance: Some(ExprInstance::GString(text)),
            }] => text.clone(),
            _ => panic!("a delivery holds strings and integers: {item:?}"),
        });
    }
    parts.join(" ")
}

/// The sorted texts of the deliveries on [`OUT`] in the post-state of the deploy `played`.
fn deliveries_of(played: &Played) -> Vec<String> {
    let mut texts = Vec::<String>::with_capacity(played.out.len());
    for datum in &played.out {
        let [value] = datum.as_slice() else {
            panic!("a datum on OUT holds one value: {datum:?}");
        };
        texts.push(delivery_text(value));
    }
    texts.sort_unstable();
    texts
}

/// The text of the one delivery on [`OUT`] in the post-state of the deploy `played`.
fn delivery_of(label: &str, played: &Played) -> String {
    let texts = deliveries_of(played);
    let [text] = texts.as_slice() else {
        panic!("{label}: one delivery, not {texts:?}");
    };
    text.clone()
}

/// The state of a chain before its runs: a space, and the deploys `steps` one after the other.
#[derive(Clone, Copy)]
struct Setup {
    urn: &'static str,
    ctor: &'static str,
    steps: &'static [&'static str],
}

const BAG_RECORDS: Setup = Setup {
    urn: BAG,
    ctor: DEFAULT,
    steps: &[THREE_RECORDS],
};
const BAG_WAITERS: Setup = Setup {
    urn: BAG,
    ctor: DEFAULT,
    steps: &[THREE_WAITERS],
};
const SET_RECORDS: Setup = Setup {
    urn: SET,
    ctor: DEFAULT,
    steps: &[THREE_RECORDS],
};
const JOIN_PARTNERS: Setup = Setup {
    urn: BAG,
    ctor: DEFAULT,
    steps: &[JOIN_RECORDS],
};
const VECTORDB_TIES: Setup = Setup {
    urn: VECTORDB,
    ctor: VECTORDB_CTOR,
    steps: &[TIED_RECORDS],
};
const VECTORDB_SCORES: Setup = Setup {
    urn: VECTORDB,
    ctor: VECTORDB_CTOR,
    steps: &[SCORED_RECORDS],
};
const VECTORDB_WAITERS: Setup = Setup {
    urn: VECTORDB,
    ctor: VECTORDB_CTOR,
    steps: &[TWO_WAITERS],
};
const PATHMAP_WAITERS: Setup = Setup {
    urn: PATHMAP_BAG,
    ctor: DEFAULT,
    steps: &[TWO_WAITERS],
};
const QUEUE_RECORDS: Setup = Setup {
    urn: QUEUE,
    ctor: DEFAULT,
    steps: &["a!(1)", "a!(2)"],
};
const STACK_RECORDS: Setup = Setup {
    urn: STACK,
    ctor: DEFAULT,
    steps: &["a!(1)", "a!(2)"],
};
const CELL_RECORD: Setup = Setup {
    urn: CELL,
    ctor: DEFAULT,
    steps: &["a!(1)"],
};
const PRIORITY_RECORDS: Setup = Setup {
    urn: PRIORITY_QUEUE,
    ctor: DEFAULT,
    steps: &[PRIORITY_SENDS],
};
const GUARD_SETUP: Setup = Setup {
    urn: BAG,
    ctor: DEFAULT,
    steps: &[GUARD_WAITERS],
};

/// Plays the deploy that creates a space of the family `urn`.
async fn create_space(chain: &mut NativeChain, label: &str, urn: &str, ctor: &str) -> Played {
    let term = stashing(urn, ctor, "Nil");
    let space_label = format!("{label}: the space");
    let played = play_certified(chain, &term, &space_label).await;
    assert_runs(&space_label, &played);
    played
}

/// Plays the deploy `body` over the stash of the chain.
async fn play_step(chain: &mut NativeChain, label: &str, part: &str, body: &str) -> Played {
    let term = with_stash(body);
    let step_label = format!("{label}: {part}");
    let played = play_certified(chain, &term, &step_label).await;
    assert_runs(&step_label, &played);
    played
}

/// Plays the deploys of `setup` on `chain` and returns the play of the last deploy.
async fn set_up(chain: &mut NativeChain, label: &str, setup: Setup) -> Played {
    let mut last = create_space(chain, label, setup.urn, setup.ctor).await;
    for (index, step) in setup.steps.iter().enumerate() {
        last = play_step(chain, label, &format!("step {index}"), step).await;
    }
    last
}

/// Plays `run` [`RUNS`] times from the state of `setup`. Returns the last deploy of the setup
/// and the delivery of each run.
async fn picks_of(setup: Setup, run: &str, label: &str) -> (Played, Vec<String>) {
    let mut chain = NativeChain::new().await;
    let stored = set_up(&mut chain, label, setup).await;
    let term = with_stash(run);
    let mut picks = Vec::<String>::with_capacity(RUNS);
    for index in 0..RUNS {
        chain.reset(&stored.root).await;
        let run_label = format!("{label}: run {index}");
        let played = play_certified(&mut chain, &term, &run_label).await;
        assert_runs(&run_label, &played);
        picks.push(delivery_of(&run_label, &played));
    }
    (stored, picks)
}

/// Checks on each runtime that the deliveries of the runs are exactly `candidates`.
fn assert_each_candidate_picked(name: &str, setup: Setup, run: &str, candidates: &[&str]) {
    let expected: BTreeSet<String> = candidates.iter().map(|text| (*text).to_string()).collect();
    for workers in RACING_WORKERS {
        let label = format!("{name}, {workers} workers");
        let (_, picks) = racing_runtime(workers).block_on(picks_of(setup, run, &label));
        let picked: BTreeSet<String> = picks.into_iter().collect();
        assert_eq!(picked, expected, "{label}: play picks each candidate");
    }
}

/// Plays `body` `runs` times from the state `root`. Each run must deliver `delivery` or park
/// one waiter. Returns the count of fires and the count of parks.
async fn fires_and_parks(
    chain: &mut NativeChain,
    root: &Blake2b256Hash,
    body: &str,
    delivery: &str,
    runs: usize,
    label: &str,
) -> (usize, usize) {
    let term = with_stash(body);
    let (mut fired, mut parked) = (0_usize, 0_usize);
    for run in 0..runs {
        chain.reset(root).await;
        let run_label = format!("{label}: run {run}");
        let played = play_certified(chain, &term, &run_label).await;
        assert_runs(&run_label, &played);
        match played.out.len() {
            0 => {
                assert_eq!(
                    parked_waiters(&played).len(),
                    1,
                    "{run_label}: one waiter parks"
                );
                parked += 1;
            }
            1 => {
                assert_eq!(delivery_of(&run_label, &played), delivery, "{run_label}");
                fired += 1;
            }
            count => panic!("{run_label}: {count} deliveries"),
        }
    }
    (fired, parked)
}

#[test]
fn space_bag_play_picks_each_of_three_records() {
    assert_each_candidate_picked("bag records", BAG_RECORDS, TAKE, &[
        "took 1", "took 2", "took 3",
    ]);
}

#[test]
fn space_bag_play_picks_each_of_three_waiters() {
    assert_each_candidate_picked("bag waiters", BAG_WAITERS, SEND_NINE, &[
        "w1 9", "w2 9", "w3 9",
    ]);
}

#[test]
fn space_set_play_picks_each_of_three_classes() {
    assert_each_candidate_picked("set classes", SET_RECORDS, TAKE, &[
        "took 1", "took 2", "took 3",
    ]);
}

#[test]
fn space_join_window_play_picks_each_of_three_records() {
    assert_each_candidate_picked("join window", JOIN_PARTNERS, JOIN_ONCE, &[
        "join 1 10",
        "join 2 10",
        "join 3 10",
    ]);
}

/// The record `z` scores worse than the ties, so no run binds it.
#[test]
fn space_vectordb_play_picks_each_of_three_best_ties() {
    assert_each_candidate_picked("VectorDB ties", VECTORDB_TIES, RANKED_TAKE, &[
        "took c1", "took c2", "took c3",
    ]);
}

#[test]
fn space_vectordb_plain_read_picks_each_of_three_records() {
    assert_each_candidate_picked("VectorDB plain read", VECTORDB_SCORES, TAKE, &[
        "took c1", "took c2", "took c3",
    ]);
}

/// Each run picks the DR-75 first record with a chance of 1/3, so every run picks it with a
/// chance of `(1/3)^200`.
#[test]
fn space_bag_play_does_not_follow_the_canonical_order() {
    for workers in RACING_WORKERS {
        let label = format!("canonical order, {workers} workers");
        let (stored, picks) = racing_runtime(workers).block_on(picks_of(BAG_RECORDS, TAKE, &label));
        let channel = stored_channel(&stored, 3);
        let values = stored_values(&stored, &channel, set_value);
        let (_, first) = values.first().expect("the bag holds three records");
        let canonical = format!("took {}", int_of(first));
        let others = picks.iter().filter(|pick| **pick != canonical).count();
        assert!(
            others > 0,
            "{label}: every run picks the DR-75 first record"
        );
    }
}

/// E1: the join names `a` twice, so it binds the two DR-75 first records in every run. The third
/// record stays. The test does not name the bind of each record, because the normalizer orders
/// the binds.
#[test]
fn space_join_on_one_collection_twice_replays_its_own_record() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("E1, {workers} workers");
            let mut chain = NativeChain::new().await;
            let stored = set_up(&mut chain, &label, BAG_RECORDS).await;
            let channel = stored_channel(&stored, 3);
            let values = stored_values(&stored, &channel, set_value);
            let [_, _, (_, third)] = values.as_slice() else {
                panic!("{label}: the bag holds three records");
            };
            let term = with_stash(JOIN_ON_A_TWICE);
            let mut outcomes = BTreeSet::<String>::new();
            for run in 0..AGREEMENT_RUNS {
                chain.reset(&stored.root).await;
                let run_label = format!("{label}: run {run}");
                let played = play_certified(&mut chain, &term, &run_label).await;
                assert_runs(&run_label, &played);
                outcomes.insert(delivery_of(&run_label, &played));
                let rest = stored_values(&played, &channel, set_value);
                let [(_, kept)] = rest.as_slice() else {
                    panic!("{run_label}: one record stays");
                };
                assert_eq!(
                    int_of(kept),
                    int_of(third),
                    "{run_label}: the third record stays"
                );
            }
            assert_eq!(outcomes.len(), 1, "{label}: one pick: {outcomes:?}");
        });
    }
}

/// E3-a: the guard admits one of two records, so the receive fires or parks by its pick. Replay
/// parks a Stored row without a search. An outcome of one guard is missing on one runtime with a
/// chance of `2^-64`.
#[test]
fn space_guarded_receive_park_replays_under_either_pick() {
    let setup = Setup {
        urn: BAG,
        ctor: DEFAULT,
        steps: &["a!(1) | a!(2)"],
    };
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            for guard in [1_i64, 2] {
                let label = format!("the guard {guard}, {workers} workers");
                let mut chain = NativeChain::new().await;
                let stored = set_up(&mut chain, &label, setup).await;
                let body = format!(r#"for (@x <- a where x == {guard}) {{ out!(["took", x]) }}"#);
                let delivery = format!("took {guard}");
                let (fired, parked) = fires_and_parks(
                    &mut chain,
                    &stored.root,
                    &body,
                    &delivery,
                    AGREEMENT_RUNS,
                    &label,
                )
                .await;
                assert!(
                    fired > 0 && parked > 0,
                    "{label}: fired {fired}, parked {parked}"
                );
            }
        });
    }
}

/// How many outcomes a family must show.
enum Picks {
    Several,
    One,
}

/// A family of ambiguous deploys: its state, the deploy of each case, and its count of outcomes.
struct Family {
    name: &'static str,
    setup: Setup,
    run: &'static str,
    picks: Picks,
}

const fn family(name: &'static str, setup: Setup, run: &'static str, picks: Picks) -> Family {
    Family {
        name,
        setup,
        run,
        picks,
    }
}

/// The 13 families of the agreement property. The windows of a PathMap keep DR-75 (decision 124,
/// L4), so the random pick of the PathMap family is the waiters inside a prefix.
const FAMILIES: [Family; 13] = [
    family("bag records", BAG_RECORDS, TAKE, Picks::Several),
    family("bag waiters", BAG_WAITERS, SEND_NINE, Picks::Several),
    family("set classes", SET_RECORDS, TAKE, Picks::Several),
    family(
        "a join named once",
        JOIN_PARTNERS,
        JOIN_ONCE,
        Picks::Several,
    ),
    family("a join on [c, c]", BAG_RECORDS, JOIN_ON_A_TWICE, Picks::One),
    family(
        "a VectorDB single tie",
        VECTORDB_TIES,
        RANKED_TAKE,
        Picks::Several,
    ),
    family(
        "a VectorDB plain read",
        VECTORDB_SCORES,
        TAKE,
        Picks::Several,
    ),
    family(
        "a VectorDB send waiter",
        VECTORDB_WAITERS,
        VECTOR_SEND,
        Picks::Several,
    ),
    family(
        "PathMap bag waiters inside a prefix",
        PATHMAP_WAITERS,
        PATH_SEND,
        Picks::Several,
    ),
    family("queue", QUEUE_RECORDS, TAKE, Picks::One),
    family("stack", STACK_RECORDS, TAKE, Picks::One),
    family("cell", CELL_RECORD, TAKE, Picks::One),
    family("priority queue", PRIORITY_RECORDS, TAKE, Picks::One),
];

/// A send meets a refusing guard and an admitting waiter in a random order. A guard refusal
/// moves the send to the next waiter, so the admitting waiter always fires. Each play must
/// charge what its replay charges.
const GUARD_FAMILY: Family = family(
    "a refusing guard before the admitting waiter",
    GUARD_SETUP,
    SEND_TWO,
    Picks::One,
);

/// Plays `cases` cases of `family` and returns the set of the outputs of the cases.
async fn family_picks(family: &Family, cases: usize, workers: usize) -> BTreeSet<Vec<String>> {
    let label = format!("{}, {workers} workers", family.name);
    let mut chain = NativeChain::new().await;
    let stored = set_up(&mut chain, &label, family.setup).await;
    let term = with_stash(family.run);
    let mut picks = BTreeSet::<Vec<String>>::new();
    for case in 0..cases {
        chain.reset(&stored.root).await;
        let case_label = format!("{label}: case {case}");
        let played = play_certified(&mut chain, &term, &case_label).await;
        assert_runs(&case_label, &played);
        picks.insert(played.outputs.clone());
    }
    picks
}

/// Checks the count of outcomes of `family`.
fn assert_picks(family: &Family, workers: usize, seen: &BTreeSet<Vec<String>>) {
    let name = family.name;
    match family.picks {
        Picks::Several => assert!(
            seen.len() >= 2,
            "{name}, {workers} workers: one pick: {seen:?}"
        ),
        Picks::One => assert_eq!(seen.len(), 1, "{name}, {workers} workers: {seen:?}"),
    }
}

/// F6, round-robin (F24). Each case certifies through its own native replay. The phlo, the byte
/// observations, the authority events and the root of play and replay must be equal. Non-vacuity:
/// a random family shows two or more outcomes, and every other family shows exactly one.
#[test]
fn prop_space_random_play_agrees_with_native_replay() {
    let mut cases = [0_usize; FAMILIES.len()];
    for case in 0..F6_CASES {
        cases[case % FAMILIES.len()] += 1;
    }
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            for (family, count) in FAMILIES.iter().zip(cases) {
                let seen = family_picks(family, count, workers).await;
                assert_picks(family, workers, &seen);
            }
            let seen = family_picks(&GUARD_FAMILY, F6_GUARD_CASES, workers).await;
            assert_picks(&GUARD_FAMILY, workers, &seen);
        });
    }
}

/// F7: the guard of one join on `[a, a]` refuses every tuple. The send still fires the other
/// join with both records, so a refused tuple leaves every record visible.
#[test]
fn space_vetoed_bag_join_on_a_repeated_collection_restores_every_datum() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("F7, {workers} workers");
            let mut chain = NativeChain::new().await;
            create_space(&mut chain, &label, BAG, DEFAULT).await;
            let stored = play_step(&mut chain, &label, "the record", r#"a!("stored")"#).await;
            let channel = stored_channel(&stored, 1);
            let parked = play_step(&mut chain, &label, "the joins", REPEATED_JOINS).await;
            assert_eq!(parked_waiters(&parked).len(), 2, "{label}: both joins park");
            let send = with_stash(r#"a!("incoming")"#);
            for run in 0..ROLLBACK_RUNS {
                chain.reset(&parked.root).await;
                let run_label = format!("{label}: run {run}");
                let played = play_certified(&mut chain, &send, &run_label).await;
                assert_runs(&run_label, &played);
                let delivery = delivery_of(&run_label, &played);
                match delivery.as_str() {
                    "accepted stored incoming" | "accepted incoming stored" => {}
                    other => panic!("{run_label}: the next waiter does not fire: {other}"),
                }
                assert!(
                    stored_values(&played, &channel, set_value).is_empty(),
                    "{run_label}: the fire takes the stored record, and the send stores nothing"
                );
            }
        });
    }
}

/// G9: a theory counts as a veto for a join too. The guard admits the record 1 only, so the join
/// fires or parks by its pick. Replay parks a Stored row without a search. An outcome is missing
/// on one runtime with a chance of `2^-64`.
#[test]
fn space_guarded_join_park_with_a_theory_replays() {
    let setup = Setup {
        urn: BAG,
        ctor: THEORY_CTOR,
        steps: &["a!(1) | a!(2) | b!(6)"],
    };
    let body = r#"for (@p <- a & @q <- b where p == 1) { out!(["join", p, q]) }"#;
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("G9, {workers} workers");
            let mut chain = NativeChain::new().await;
            let stored = set_up(&mut chain, &label, setup).await;
            let (fired, parked) = fires_and_parks(
                &mut chain,
                &stored.root,
                body,
                "join 1 6",
                PARK_RUNS,
                &label,
            )
            .await;
            assert!(
                fired > 0 && parked > 0,
                "{label}: fired {fired}, parked {parked}"
            );
        });
    }
}

/// GF-3, GF-5: R1 picks a class at random. A pick of `a` fails its guard, so R1 parks. R2 then
/// takes `a`, and the set cascade fires R1 with `b` in the transaction of R2. The join
/// [`NEVER_JOIN`] holds the channel of `a`, so each operation runs in the join engine. The
/// one-bind route has no cascade. One runtime shows no park with a chance of `2^-64`.
#[test]
fn set_cascade_fires_the_guarded_receive_that_a_vetoed_pick_parked() {
    let guarded = with_stash(GUARDED_RECEIVE);
    let exact = with_stash(EXACT_RECEIVE);
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("GF-3, {workers} workers");
            let mut chain = NativeChain::new().await;
            create_space(&mut chain, &label, SET, DEFAULT).await;
            let stored = play_step(&mut chain, &label, "the records", CASCADE_RECORDS).await;
            let channel = stored_channel(&stored, 2);
            let joined = play_step(&mut chain, &label, "the join", NEVER_JOIN).await;
            assert_eq!(parked_waiters(&joined).len(), 1, "{label}: the join parks");
            let mut parks = 0_usize;
            for run in 0..PARK_RUNS {
                chain.reset(&joined.root).await;
                let first_label = format!("{label}: R1, run {run}");
                let first = play_certified(&mut chain, &guarded, &first_label).await;
                assert_runs(&first_label, &first);
                let waiters = parked_waiters(&first);
                let second_label = format!("{label}: R2, run {run}");
                let second = play_certified(&mut chain, &exact, &second_label).await;
                assert_runs(&second_label, &second);
                assert_eq!(deliveries_of(&second), ["r1 b", "r2 a"], "{second_label}");
                assert!(
                    stored_values(&second, &channel, set_value).is_empty(),
                    "{second_label}: the set ends empty"
                );
                match (waiters.len(), first.out.len()) {
                    (1, 0) => {
                        parks += 1;
                        assert_eq!(
                            fires_of(journal_of(&second), &waiters),
                            1,
                            "{second_label}: R1 fires inside the transaction of R2"
                        );
                    }
                    (0, 1) => {
                        assert_eq!(delivery_of(&first_label, &first), "r1 b", "{first_label}");
                    }
                    other => panic!("{first_label}: {other:?}"),
                }
            }
            assert!(parks > 0, "{label}: no run parks R1");
        });
    }
}
