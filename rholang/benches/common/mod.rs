//! The shared parts of the Reified RSpaces benchmarks (P10 step S8, design v2 section 2.8).
// Changed by decision 89: no host-work limit bounds a deploy. The text before:
// ```
// //! A case is a chain of setup deploys and one measured deploy. Each deploy plays natively at the
// //! v6 limits on the test chain of `rholang/tests/support/native.rs`. Before it measures, a case
// ```
//!
//! A case is a chain of setup deploys and one measured deploy. Each deploy plays natively on the
//! test chain of `rholang/tests/support/native.rs`. Before it measures, a case
// Changed by decisions 102 and 111 (T-BENCH): the check compares the fields of the class of the
// case. The text before:
// ```
// //! plays its measured deploy once and checks the result, the phlo and the root against the
// //! frozen contract `test-vectors/reified-bench-contract-v1.json` (the baseline contract of leaf
// //! 9214). Every run checks it, `--test` included.
// ```
//! plays its measured deploy once and checks the fields of its class (`compare_of`) against the
//! frozen contract `test-vectors/reified-bench-contract-v2.json` (the baseline contract of leaf
//! 9214). Every run checks it, `--test` included.
//!
//! With `RRS_BENCH_CAPTURE=FILE`, a case checks what it observed against FILE, which can be a copy
//! of the contract, and adds the cases that FILE does not hold yet. A capture creates or extends
//! the contract, and a reviewer copies FILE to `test-vectors/`.
//! `RRS_BENCH_MAX_SIZE` bounds the sizes of the series, and the ALL gate sets it to 1000.
//! `RRS_BENCH_RECORD=FILE` appends one JSON line for each case: its group, id, phlo, root,
//! result, the time of its setup and the time of its reference play.
//!
//! The filter of a run is the first argument that is not an option. Criterion matches it as a
//! regular expression against the id `group/case` of each benchmark, and [`matches`] does the
//! same, so a case that the filter does not select is not set up. An option that takes a value
//! must be written as `--option=value`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion, SamplingMode};
use regex::Regex;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

use crate::support::native::NativeChain;
use crate::support::session::error_codes;

/// The path of the frozen contract, relative to the `rholang` crate.
// Changed by decision 112: the v1 contract stays frozen, and the fields form writes v2. The text
// before:
// const CONTRACT: &str = "../test-vectors/reified-bench-contract-v1.json";
const CONTRACT: &str = "../test-vectors/reified-bench-contract-v2.json";
/// The schema of the contract.
const SCHEMA: &str = "reified-rspaces-bench-contract/1";
// Changed by decision 89: no host-work limit bounds a deploy. The text before:
// ```
// /// The phlo limit of every deploy of the benchmarks. No deploy of the benchmarks reaches it, so
// /// the host-work limits of protocol version 6 bound each deploy, the host program included. The
// /// test suites use the smaller `AMPLE_LIMIT` of the test support, which a one-deploy series of
// /// the host program reaches at 1,503 sends.
// ```
/// The phlo limit of every deploy of the benchmarks.
pub const PHLO_LIMIT: u64 = 1_000_000_000;
/// The size bound when `RRS_BENCH_MAX_SIZE` is not set.
pub const DEFAULT_MAX_SIZE: u64 = 1_000_000;
// Changed by decision 89: no host-work limit bounds a deploy. The text before:
// /// The operations of one setup deploy. Ten operations fit the v6 limits for every family.
/// The operations of one setup deploy.
pub const CHUNK: u64 = 10;
/// The first integer payload of a record. A `GInt` holds a zigzag varint, so each integer from
/// 8,192 to 1,048,575 takes three bytes. Every record of a series then has one encoded size, so
/// a random pick among records does not change the phlo.
pub const PAYLOAD_BASE: u64 = 10_000;
/// The channel where a setup deploy publishes the handle of its space.
pub const HANDLE: &str = "rrs.bench.handle";
/// The channel where a case reports its result.
pub const OUT: &str = "rrs.bench.out";

/// What one play of the measured deploy of a case gives.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed {
    /// The values on [`OUT`] in Rholang syntax, sorted, or the error codes of the deploy.
    pub result: Vec<String>,
    /// The native phlo of the deploy. A deploy that fails before its phlo is known has none.
    pub phlo: Option<u64>,
    /// The root of the checkpoint after the deploy, in hex.
    pub root: String,
}

/// The fields of [`Observed`] that the check of a case compares. [`compare_of`] gives the class
/// of each case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Compare {
    /// The result, the phlo and the root.
    All,
    /// The result and the phlo.
    NoRoot,
    /// The form of the result, with `#` in the place of each number, and the phlo.
    Pick,
}

impl Observed {
    /// This observation as the contract holds it under the class `compare`. A root that the
    /// class does not compare is empty, and the result of [`Compare::Pick`] holds its form.
    fn project(&self, compare: Compare) -> Observed {
        match compare {
            Compare::All => self.clone(),
            Compare::NoRoot => Observed {
                root: String::new(),
                ..self.clone()
            },
            Compare::Pick => {
                let number = Regex::new(r"\d+").expect("the pattern of a number is valid");
                let mut result = Vec::with_capacity(self.result.len());
                result.extend(
                    self.result
                        .iter()
                        .map(|value| number.replace_all(value, "#").into_owned()),
                );
                Observed {
                    result,
                    phlo: self.phlo,
                    root: String::new(),
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Contract {
    schema: String,
    note: String,
    cases: BTreeMap<String, Observed>,
}

static CAPTURED: Mutex<Option<BTreeMap<String, Observed>>> = Mutex::new(None);

// Changed by decision 89: no host-work limit bounds a deploy, so a fill finds no full collection.
// The text before:
// ```
// /// The setups that stopped because a collection was full, by their text, with the records that
// /// they stored. A case with the same setup stops at the same record, so it does not build it
// /// again.
// ```
/// The setups whose fill stopped because a deploy of one send failed, by their text, with the
/// records that they stored. A case with the same setup stops at the same record, so it does not
/// build it again.
static FULL: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

/// One step of the setup of a case.
#[derive(Clone)]
pub enum Step {
    /// One deploy.
    Deploy(String),
    // Changed by decision 89: no host-work limit bounds a deploy, so a fill finds no full
    // collection. The text before:
    // ```
    // /// Sends that store records in the space on [`HANDLE`], one send for each record. The setup
    // /// plays [`CHUNK`] sends in each deploy while that fits the v6 limits, and then one send in
    // /// each deploy. When one send no longer fits, the collection is full.
    // ```
    /// Sends that store records in the space on [`HANDLE`], one send for each record. The setup
    /// plays [`CHUNK`] sends in each deploy.
    Fill(Vec<String>),
    /// Sends of the equivalent host program, on ordinary channels with no space, played like
    /// [`Step::Fill`].
    HostFill(Vec<String>),
}

/// One benchmark case.
pub struct Case {
    /// The id inside its group, for example `bag/100`.
    pub id: String,
    /// The steps that build the pre-state, played in order and not measured.
    pub setup: Vec<Step>,
    /// The measured deploy.
    pub term: String,
    /// Whether the measurement includes the checkpoint after the measured deploy.
    pub with_checkpoint: bool,
}

/// The filter of the command line: the first argument that is not an option, as a regular
/// expression, as Criterion reads it.
fn filter() -> Option<Regex> {
    std::env::args()
        .skip(1)
        .find(|arg| !arg.starts_with('-'))
        .map(|arg| Regex::new(&arg).expect("the filter of the run is a regular expression"))
}

/// Whether the filter of the command line selects the case `key`, as Criterion selects the
/// benchmark with that id. A run without a filter selects every case.
pub fn matches(key: &str) -> bool { filter().is_none_or(|filter| filter.is_match(key)) }

// Retired by decision 89 (T-BENCH, ruling L-5): the one-deploy series use fixed counts, so no
// series reads its counts from the contract. The text before:
// ```
// /// The counts of the one-deploy series whose keys start with `prefix`, in ascending order, as
// /// the contract records them, or as the capture file records them in a capture. The counts are
// /// 1, 10, the largest count that fits the v6 limits and the next count. Empty when the series is
// /// not recorded yet, so a capture calibrates it. The check of the largest count and of the next
// /// count verifies the boundary in every run, so a recorded series needs no calibration.
// pub fn recorded_counts(prefix: &str) -> Vec<u64> {
//     let mut counts: Vec<u64> = known_cases()
//         .keys()
//         .filter_map(|key| key.strip_prefix(prefix))
//         .filter_map(|count| count.parse().ok())
//         .collect();
//     counts.sort_unstable();
//     counts.dedup();
//     counts
// }
// ```

/// The size bound of the series.
pub fn max_size() -> u64 {
    std::env::var("RRS_BENCH_MAX_SIZE")
        .ok()
        .map(|v| v.parse().expect("RRS_BENCH_MAX_SIZE is an integer"))
        .unwrap_or(DEFAULT_MAX_SIZE)
}

/// The sizes of `sizes` up to the bound.
pub fn sizes(all: &[u64]) -> Vec<u64> {
    let bound = max_size();
    all.iter().copied().filter(|n| *n <= bound).collect()
}

/// A multi-thread tokio runtime with two workers, as the tests use.
pub fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a tokio runtime")
}

fn contract_path() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CONTRACT) }

fn load_contract() -> BTreeMap<String, Observed> {
    let text = std::fs::read_to_string(contract_path()).expect("read the bench contract");
    let contract: Contract = serde_json::from_str(&text).expect("the bench contract is valid JSON");
    assert_eq!(contract.schema, SCHEMA, "the schema of the bench contract");
    contract.cases
}

/// The cases of the capture file `path`, or none when the file does not exist yet.
fn load_capture(path: &str) -> BTreeMap<String, Observed> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Contract>(&text).ok())
        .map(|contract| contract.cases)
        .unwrap_or_default()
}

// Retired by decision 89 (T-BENCH, ruling L-5): only `recorded_counts` read the known cases, so
// this function has no caller. The text before:
// ```
// /// The cases that the checks of this run compare with: those of the capture file in a capture,
// /// and those of the contract otherwise.
// fn known_cases() -> BTreeMap<String, Observed> {
//     match std::env::var("RRS_BENCH_CAPTURE") {
//         Ok(path) => CAPTURED
//             .lock()
//             .expect("the capture lock")
//             .get_or_insert_with(|| load_capture(&path))
//             .clone(),
//         Err(_) => load_contract(),
//     }
// }
// ```

/// The class of the case `key`. The program of the case gives the class: a field that depends on
/// the order of arrival in a `par`, or on a random pick among equal candidates, has no stable
/// value between two honest runs, so the contract holds no such value.
///
/// - [`Compare::Pick`]: one receive publishes a record that a pick chose among two or more records
///   of one encoded size. The pick decides the result and the records that stay.
/// - [`Compare::NoRoot`]: two or more operations of one deploy write one counter record (a
///   collection counter, the space counter of a cell or the token of a PathMap), and the last
///   arrival decides its random state, or a pick among equal candidates removes one and leaves
///   the others. Each operation takes one distinct counter value, so the multiset of the encoded
///   sizes and the phlo do not depend on the arrival.
/// - [`Compare::All`]: every other case. A new series starts here, so a series whose outcome
///   varies fails on an honest run until it gets an arm here.
fn compare_of(key: &str) -> Compare {
    match key.split('/').collect::<Vec<_>>().as_slice() {
        // A receive publishes the record that the pick chose.
        ["receive_from_n", "bag" | "set" | "host", _]
        | ["matcher", "bag_first" | "join_two", _]
        | ["restart_attach", "bag", _] => Compare::Pick,
        // Two or more operations of one deploy write one counter record.
        ["send", "queue" | "stack" | "priorityqueue" | "cell" | "pathmap_bag", "10" | "100"]
        | ["send_receive", "queue" | "stack" | "priorityqueue", _]
        | ["handle_produce", "queue", "10" | "100"]
        | ["queue_ordering", "send_and_drain", _]
        | ["commit", "queue" | "stack" | "priorityqueue", _]
        | ["replay", "queue_two_blocks", _]
        | ["checkpoint_size", "queue" | "stack" | "priorityqueue", _] => Compare::NoRoot,
        // A pick removes one of n equal waiters, or one of the records that tie for the best
        // score of a VectorDB read. The fill holds two `[1, 1, 1, 1]` records from 107 records on.
        ["matcher", "send_fires_waiter", _]
        | ["receive_from_n", "vectordb", "1000" | "10000"]
        | ["vectordb_query", "best" | "threshold", "1000" | "10000"] => Compare::NoRoot,
        _ => Compare::All,
    }
}

// Changed by decisions 102 and 111 (T-BENCH): the check compares the fields of the class of the
// case. The text before:
// /// Checks `observed` of the case `key` against the contract, or captures it.
/// Checks the fields of `observed` that the class of the case `key` compares against the
/// contract, or captures them.
pub fn check(key: &str, observed: &Observed) {
    let projected = observed.project(compare_of(key));
    if let Ok(path) = std::env::var("RRS_BENCH_CAPTURE") {
        let mut captured = CAPTURED.lock().expect("the capture lock");
        let cases = captured.get_or_insert_with(|| load_capture(&path));
        if let Some(known) = cases.get(key) {
            // Changed by decisions 102 and 111 (T-BENCH): the capture compares the fields of the
            // class of the case, so a file that holds a varying field cannot arise. The text
            // before:
            // ```
            // assert_eq!(
            //     known, observed,
            //     "{key}: the captured contract holds another result, phlo or root"
            // );
            // ```
            assert_eq!(
                known, &projected,
                "{key}: the captured contract holds another result, phlo or root"
            );
        }
        // Changed by decisions 102 and 111 (T-BENCH): the capture stores the fields of the class
        // of the case. The text before:
        // cases.insert(key.to_string(), observed.clone());
        cases.insert(key.to_string(), projected);
        let contract = Contract {
            schema: SCHEMA.to_string(),
            // Changed by decision 89 (T-BENCH): the file holds no host work, and each entry holds
            // the fields of the class of its case. The text before:
            // ```
            // note: "The result, phlo, root and host work of the measured deploy of each benchmark case \
            //        (P10 step S8). Every benchmark run checks its cases against this file."
            //     .to_string(),
            // ```
            note: "The result, phlo and root of the measured deploy of each benchmark case (P10 \
                   step S8), in the form that the class of the case keeps (compare_of in \
                   rholang/benches/common/mod.rs). Every benchmark run checks its cases against \
                   this file."
                .to_string(),
            cases: cases.clone(),
        };
        let text = serde_json::to_string_pretty(&contract).expect("serialize the contract");
        std::fs::write(&path, text + "\n").expect("write the captured contract");
        return;
    }
    let cases = load_contract();
    let expected = cases
        .get(key)
        .unwrap_or_else(|| panic!("the bench contract has no case {key}"));
    // Changed by decisions 102 and 111 (T-BENCH): the check compares the fields of the class of
    // the case. The text before:
    // ```
    // assert_eq!(
    //     expected, observed,
    //     "{key}: the result, phlo and root of the bench contract"
    // );
    // ```
    assert_eq!(
        expected, &projected,
        "{key}: the fields of the class of the case in the bench contract"
    );
}

/// Appends the record line of the case `key` to `RRS_BENCH_RECORD`: what its reference play
/// observed, the time of its setup and the time of its reference play.
pub fn record(key: &str, observed: &Observed, setup: Duration, reference: Duration) {
    if let Ok(path) = std::env::var("RRS_BENCH_RECORD") {
        use std::io::Write;
        let line = serde_json::json!({
            "case": key,
            "phlo": observed.phlo,
            "root": observed.root,
            "result": observed.result,
            "setup_ns": setup.as_nanos() as u64,
            "reference_play_ns": reference.as_nanos() as u64,
        });
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open the bench record");
        writeln!(file, "{line}").expect("write the bench record");
    }
}

/// Appends one JSON line `value` to `RRS_BENCH_RECORD`.
pub fn record_value(value: serde_json::Value) {
    if let Ok(path) = std::env::var("RRS_BENCH_RECORD") {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open the bench record");
        writeln!(file, "{value}").expect("write the bench record");
    }
}

/// Plays `term` once on `chain`, reads the values on [`OUT`], takes the checkpoint, and returns
/// what it observed with the root.
pub async fn observe(
    chain: &mut NativeChain,
    term: &str,
    label: &str,
) -> (Observed, Blake2b256Hash) {
    let evaluated = chain.evaluate(term, label, PHLO_LIMIT).await;
    let (errors, phlo) = match &evaluated {
        Ok(result) => (error_codes(&result.errors), result.native_phlo_usage),
        Err(error) => (error_codes(std::slice::from_ref(error)), None),
    };
    let result = if errors.is_empty() {
        let mut values: Vec<String> = chain
            .values(OUT)
            .await
            .iter()
            .map(|pars| {
                pars.iter()
                    .map(|par| PrettyPrinter::new().build_string_from_message(par))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .collect();
        values.sort();
        values
    } else {
        errors
            .into_iter()
            .map(|code| format!("error {code}"))
            .collect()
    };
    let root = chain.checkpoint().await;
    let observed = Observed {
        result,
        phlo,
        root: hex::encode(root.bytes()),
    };
    (observed, root)
}

// Retired by decision 89 (T-BENCH, ruling L-5): no host-work limit bounds a deploy, so the
// calibration finds no boundary. The one-deploy series use fixed counts. The text before:
// ```
// /// The largest count of one deploy that a calibration searches. A deploy of more operations
// /// than this is not a measure of these benchmarks.
// const CALIBRATION_LIMIT: u64 = 1 << 16;
//
// /// The largest `n` whose deploy `term(n)` fits the v6 limits on the pre-state of `setup`, or 0
// /// when none fits. The host work of `term(n)` grows with `n`. The search doubles the bound `hi`
// /// until a count does not fit, then a binary search finds the boundary below it. Each probe
// /// resets the chain to the pre-state.
// pub fn largest_fitting(
//     rt: &Runtime,
//     group: &str,
//     id: &str,
//     setup: &[Step],
//     term: impl Fn(u64) -> String,
//     hi: u64,
// ) -> u64 {
//     let case = Case {
//         id: format!("{id}/calibration"),
//         setup: setup.to_vec(),
//         term: String::new(),
//         with_checkpoint: false,
//     };
//     let (mut chain, root) = rt
//         .block_on(prepare(&case, group))
//         .unwrap_or_else(|reached| {
//             panic!("{group}/{id}: the calibration setup stops at {reached} records")
//         });
//     let mut fits = |n: u64| -> bool {
//         rt.block_on(async {
//             chain.reset(&root).await;
//             let label = format!("{group}/{id} calibration {n}");
//             matches!(chain.evaluate(&term(n), &label, PHLO_LIMIT).await, Ok(result) if result.errors.is_empty())
//         })
//     };
//     // `lo` fits, or is 0, and `hi` does not fit.
//     let (mut lo, mut hi) = (0, hi);
//     while fits(hi) {
//         assert!(
//             hi < CALIBRATION_LIMIT,
//             "{group}/{id}: {hi} operations fit in one deploy, more than the calibration searches"
//         );
//         lo = hi;
//         hi *= 2;
//     }
//     while lo + 1 < hi {
//         let mid = lo + (hi - lo) / 2;
//         if fits(mid) {
//             lo = mid;
//         } else {
//             hi = mid;
//         }
//     }
//     lo
// }
// ```

/// The setup step that stores `n` records with the send `f(i)` for each record `i`.
pub fn fill(n: u64, f: impl Fn(u64) -> String) -> Step { Step::Fill((0..n).map(f).collect()) }

/// The setup step that stores `n` data on ordinary channels with the send `f(i)` for each `i`.
pub fn host_fill(n: u64, f: impl Fn(u64) -> String) -> Step {
    Step::HostFill((0..n).map(f).collect())
}

/// The text of the setup steps, which identifies a setup.
fn setup_text(setup: &[Step]) -> String {
    setup
        .iter()
        .map(|step| match step {
            Step::Deploy(term) => format!("deploy {term}"),
            Step::Fill(sends) => format!("fill {}", sends.join(" | ")),
            Step::HostFill(sends) => format!("host fill {}", sends.join(" | ")),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Plays `term` and returns whether it succeeds without an error.
async fn succeeds(chain: &mut NativeChain, term: &str, label: &str) -> bool {
    matches!(chain.evaluate(term, label, PHLO_LIMIT).await, Ok(result) if result.errors.is_empty())
}

/// Builds the pre-state of `case` on a new chain over an in-memory store and returns the chain
/// with its root. A fill that cannot store all its records gives `Err` with the records that it
/// stored.
pub async fn prepare(case: &Case, group: &str) -> Result<(NativeChain, Blake2b256Hash), u64> {
    prepare_on(NativeChain::new().await, case, group).await
}

/// Builds the pre-state of `case` on `chain`, such as a new chain over an LMDB store, as
/// [`prepare`] does on a new in-memory chain.
pub async fn prepare_on(
    mut chain: NativeChain,
    case: &Case,
    group: &str,
) -> Result<(NativeChain, Blake2b256Hash), u64> {
    let setup_key = setup_text(&case.setup);
    if let Some(reached) = FULL.lock().expect("the full-setup lock").get(&setup_key) {
        return Err(*reached);
    }
    let mut root = chain.checkpoint().await;
    for (index, step) in case.setup.iter().enumerate() {
        match step {
            Step::Deploy(term) => {
                let label = format!("{group}/{} setup {index}", case.id);
                assert!(
                    succeeds(&mut chain, term, &label).await,
                    "{label}: the setup deploy fails"
                );
                root = chain.checkpoint().await;
            }
            Step::Fill(sends) | Step::HostFill(sends) => {
                let in_space = matches!(step, Step::Fill(_));
                let mut stored = 0usize;
                let mut chunk = CHUNK as usize;
                while stored < sends.len() {
                    let last = (stored + chunk).min(sends.len());
                    let sends = sends[stored..last].join(" |\n");
                    let term = if in_space { with_space(&sends) } else { sends };
                    let label = format!("{group}/{} setup {index} records {stored}", case.id);
                    if succeeds(&mut chain, &term, &label).await {
                        stored = last;
                        root = chain.checkpoint().await;
                    } else if chunk > 1 {
                        chain.reset(&root).await;
                        chunk = 1;
                    } else {
                        FULL.lock()
                            .expect("the full-setup lock")
                            .insert(setup_key, stored as u64);
                        return Err(stored as u64);
                    }
                }
            }
        }
    }
    Ok((chain, root))
}

/// Prepares `case` on a new in-memory chain, checks its reference play against the contract, and
/// measures its deploy.
pub fn bench_case(group: &mut BenchmarkGroup<'_, WallTime>, rt: &Runtime, name: &str, case: &Case) {
    if !matches(&format!("{name}/{}", case.id)) {
        return;
    }
    let setup_started = Instant::now();
    let prepared = rt.block_on(prepare(case, name));
    measure_prepared(group, rt, name, case, prepared, setup_started.elapsed());
}

/// Checks the reference play of `case` on its prepared chain against the contract, and measures
/// its deploy, as [`bench_case`] does. `prepared` is the result of [`prepare`] or [`prepare_on`],
/// and `setup` is the time that it took. Gives back the chain and its pre-state root when the
/// deploy fits, so a caller can measure more on the same pre-state.
pub fn measure_prepared(
    group: &mut BenchmarkGroup<'_, WallTime>,
    rt: &Runtime,
    name: &str,
    case: &Case,
    prepared: Result<(NativeChain, Blake2b256Hash), u64>,
    setup: Duration,
) -> Option<(NativeChain, Blake2b256Hash)> {
    let key = format!("{name}/{}", case.id);
    let label = format!("{key} measured");
    let (mut chain, pre_root) = match prepared {
        Ok(prepared) => prepared,
        Err(reached) => {
            // Changed by decision 89: no limit stops a fill, so this arm no longer means a full
            // collection. The text before:
            // ```
            // // The collection is full before it holds the records of the case: one more send
            // // exceeds the v6 limits. The contract records where the series stops.
            // ```
            // A fill deploy of one send failed, so the setup stopped with `reached` records. The
            // check fails the run unless the contract records this outcome.
            let observed = Observed {
                result: vec![format!("error the collection is full at {reached} records")],
                phlo: None,
                root: String::new(),
            };
            check(&key, &observed);
            record(&key, &observed, setup, Duration::ZERO);
            return None;
        }
    };
    let started = Instant::now();
    let (observed, _) = rt.block_on(observe(&mut chain, &case.term, &label));
    let reference = started.elapsed();
    check(&key, &observed);
    record(&key, &observed, setup, reference);
    if observed
        .result
        .iter()
        .any(|value| value.starts_with("error "))
    {
        // Changed by decision 89: no limit bounds a deploy, and a series runs each of its fixed
        // counts. The text before:
        // ```
        // // The deploy does not fit the v6 limits: the contract records its error, and the series
        // // stops here, with nothing to measure.
        // ```
        // The deploy failed, and the contract records its error, so the case has nothing to
        // measure.
        return None;
    }
    group.bench_function(case.id.as_str(), |b| {
        b.iter_custom(|iterations| {
            let mut total = Duration::ZERO;
            for _ in 0..iterations {
                rt.block_on(chain.reset(&pre_root));
                let start = Instant::now();
                rt.block_on(async {
                    let evaluated = chain.evaluate(&case.term, &label, PHLO_LIMIT).await;
                    std::hint::black_box(&evaluated);
                    if case.with_checkpoint {
                        std::hint::black_box(chain.checkpoint().await);
                    }
                });
                total += start.elapsed();
            }
            total
        })
    });
    Some((chain, pre_root))
}

/// A group with the sample size and the times of the slow deploys of these benchmarks. The
/// sample size is 10, or `RRS_BENCH_SAMPLES` for an experiment run that needs more replicates.
/// Flat sampling gives each sample the same number of iterations, so the mean time of each
/// sample is one replicate of the case: the experiments compare the replicates of two cases with
/// Welch's t-test, which assumes one distribution in each arm. Criterion also recommends flat
/// sampling for iterations of milliseconds.
pub fn group<'c>(c: &'c mut Criterion, name: &str) -> BenchmarkGroup<'c, WallTime> {
    let mut group = c.benchmark_group(name);
    group.sampling_mode(SamplingMode::Flat);
    let samples = std::env::var("RRS_BENCH_SAMPLES")
        .ok()
        .map(|v| v.parse().expect("RRS_BENCH_SAMPLES is an integer"))
        .unwrap_or(10);
    group.sample_size(samples);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
    group
}

// ---------------------------------------------------------------------------
// Rholang terms
// ---------------------------------------------------------------------------

/// A deploy that creates a space of `family` with the constructor arguments `ctor` and
/// publishes its handle on [`HANDLE`].
pub fn create(family: &str, ctor: &str) -> String {
    format!(
        "new factory(`rho:space:{family}`) in {{\n  for (space <- factory!?({ctor})) {{ @\"{HANDLE}\"!(*space) }}\n}}"
    )
}

/// A deploy that takes the handle on [`HANDLE`], publishes it again, and runs `body` in a
/// `use` block of the space. The unforgeable name `out` of `body` routes to the default space,
/// and each value on it reaches [`OUT`].
pub fn with_space(body: &str) -> String {
    format!(
        "new out in {{\n  for (space <- @\"{HANDLE}\") {{\n    @\"{HANDLE}\"!(*space) |\n    use space {{\n{body}\n    }}\n  }} |\n  for (@v <= out) {{ @\"{OUT}\"!(v) }}\n}}"
    )
}

/// A deploy that creates a space and runs `body` in a `use` block of it, in one deploy.
pub fn create_and_use(family: &str, ctor: &str, body: &str) -> String {
    format!(
        "new factory(`rho:space:{family}`) in {{\n  for (space <- factory!?({ctor})) {{\n    use space {{\n{body}\n    }}\n  }}\n}}"
    )
}

/// `n` parallel processes, process `i` given by `f(i)`, joined by `|`.
pub fn par(n: u64, f: impl Fn(u64) -> String) -> String {
    (0..n).map(f).collect::<Vec<_>>().join(" |\n")
}
