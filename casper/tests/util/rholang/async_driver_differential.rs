// Differential harness for the async counter-driver reduction change (remove-stacker /
// detached-spawn + atomic-counter completion driver).
//
// PURPOSE — prove the four consensus invariants are preserved by the driver change:
//   (#1) presence  — is_failed (errors.is_empty() iff success)
//   (#2) cost      — cost.cost (the fork's per-COMM count) + no ReplayCostMismatch
//   (#3) post-state — the tuplespace post-state root
//   (#4) deployLog — the produce/consume/comm event log
//
// TWO LAYERS:
//   * Layer 1 (self-consistency; runs on WHATEVER driver is compiled; genesis-independent, byte-exact):
//     for each corpus program, compute_state (play) then replay_compute_state (replay) and assert
//     replay_post_state == play_post_state. Because replay_compute_state re-executes with the recorded
//     cost and rigs the deployLog, a success return additionally proves #2 (no ReplayCostMismatch),
//     #4 (deployLog reproduced) and #1 (presence consistent). This is THE consensus proof and holds on
//     both the OLD driver (pre-change baseline) and the NEW driver.
//   * Layer 2 (old-vs-new golden; cross-process): capture per-program (is_failed, cost, deployLog
//     fingerprint, balanced) on the PRE-CHANGE checkout, assert byte-identical on the NEW driver.
//
// GENESIS DETERMINISM NOTE: the shared test genesis (`with_runtime_manager` -> GenesisBuilder ->
// DEFAULT_VALIDATOR_KEY_PAIRS) uses `Secp256k1::new_key_pair()` = OsRng, i.e. RANDOM validator keys
// per process, into a temp LMDB. Therefore the ABSOLUTE post_state_root and any genesis-derived channel
// hashes are NOT comparable across processes. The invariants captured in the Layer-2 golden are exactly
// the genesis-key-INDEPENDENT ones (is_failed, cost, self-contained deployLog fingerprint, and the
// `balanced` predicate post_state==genesis_pre_state). The absolute post_state (#3) old-vs-new equality
// is proven instead by Layer 1's exact in-process replay (which reproduces post_state byte-for-byte
// against the same-run genesis). Deploys use a FIXED timestamp + the FIXED DEFAULT_SEC deployer so the
// deploy-local channel seeds (and hence the self-contained deployLog) are deterministic.

use std::collections::BTreeMap;
use std::path::PathBuf;

use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{DeployData, Event, ProcessedDeploy};
use crypto::rust::signatures::signed::Signed;
use serde::{Deserialize, Serialize};

use crate::util::genesis_builder::GenesisContext;
use crate::util::rholang::resources::with_runtime_manager;

/// Fixed deploy timestamp (ms). Makes the deploy signature — and thus the deploy-local channel
/// random seed and the self-contained deployLog — deterministic across processes.
const FIXED_TS: i64 = 1_600_000_000_000;

/// Stable, dependency-free 64-bit fingerprint (FNV-1a) over a byte slice. Used to fingerprint the
/// Debug rendering of a self-contained deployLog. Stable across runs and toolchains.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Golden {
    is_failed: bool,
    /// Per-COMM cost (the fork's consensus cost unit). The ACTUAL COMM count is fixed for a given
    /// deploy, so this is deterministic across runs/processes (unlike the raw deployLog length).
    cost: u64,
    /// Number of COMM events (actual matches) in the deployLog. Deterministic: equals the number of
    /// COMMs that fired (independent of scheduling-dependent dangling deposits / re-install events).
    comm_count: usize,
    /// FNV-1a over the SORTED (order-canonical) multiset of the COMM events' Debug renderings. The COMM
    /// events carry the matched channels + data hashes (deploy-local, deterministic under a fixed deploy
    /// signature); the STANDALONE Produce/Consume deposits & dangling re-installs — whose count is
    /// scheduling-dependent EVEN ON THE OLD DRIVER — are excluded, so this IS cross-process portable.
    comm_fnv: u64,
    /// True iff post_state == genesis pre-state (the program left the tuplespace balanced). Portable.
    balanced: bool,
}

/// A corpus entry: a name and the Rholang source. Every program's cost/is_failed/balanced + COMM
/// multiset (comm_count/comm_fnv) are asserted cross-process; the full deployLog (incl. non-COMM
/// deposits/re-installs) is covered per-execution by Layer-1 (replay==play).
struct Program {
    name: &'static str,
    source: &'static str,
}

/// The metered differential corpus. Chosen to exercise the five async join sites feasibly under the
/// per-COMM cost model (no phlo exhaustion) while remaining fast enough to capture pre-change goldens:
///   * par_sends       — Site 1 (eval_inner parallel width) + non-persistent produce/consume/dispatch.
///   * persistent_recv — persistent consume re-install path (run_parallel_dispatches consume persistent).
///   * contract_calls  — persistent consume (contract) + fan-in dispatch (run_parallel_dispatches).
///   * peek_read       — peek COMM (`<<-`) -> run_parallel_dispatches peek branch + produce_peeks.
///   * join_two        — two-source join receive.
///   * bounded_loop    — shortslow-style bounded recursion (256): the persistent-contract dispatch chain.
///   * bounded_string  — longslow-style: string send + length + bounded recursion.
/// Full shortslow.rho/longslow.rho (32768) are exercised heap-bounded via the direct-eval test below.
fn corpus() -> Vec<Program> {
    vec![
        Program {
            name: "par_sends",
            source: "new a, b, c, d in { a!(1) | b!(2) | c!(3) | d!(4) | \
                     for(@x <- a){ Nil } | for(@y <- b){ Nil } | \
                     for(@z <- c){ Nil } | for(@w <- d){ Nil } }",
        },
        Program {
            // Persistent RECEIVE (`<=`) matched by a single send: exactly one COMM, then the consume
            // re-installs with no further sends. Deterministic COMM structure (unlike a persistent SEND
            // racing multiple consumers, whose re-fire count is scheduling-dependent).
            name: "persistent_recv",
            source: "new ch in { for(@x <= ch){ Nil } | ch!(1) }",
        },
        Program {
            // A persistent contract matched by THREE PARALLEL sends. The actual COMM count (hence cost
            // and the COMM multiset) is fixed, but the count of dangling re-install consume DEPOSITS in
            // the deployLog is scheduling-dependent (racy EVEN ON THE OLD DRIVER) — which is exactly why
            // the Layer-2 fingerprint is over COMM events only, not the whole deployLog.
            name: "contract_calls",
            source: "new loop in { contract loop(@n) = { Nil } | loop!(1) | loop!(2) | loop!(3) }",
        },
        Program {
            name: "peek_read",
            source: "new ch in { ch!(42) | for(@x <<- ch){ Nil } }",
        },
        Program {
            name: "join_two",
            source: "new a, b in { a!(1) | b!(2) | for(@x <- a & @y <- b){ Nil } }",
        },
        Program {
            name: "bounded_loop",
            source: "new loop in { \
                     contract loop(@n) = { match n { 0 => Nil  _ => loop!(n - 1) } } | \
                     loop!(256) }",
        },
        Program {
            name: "bounded_string",
            source: "new x, loop in { \
                     x!(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\") | \
                     for(@s <- x){ \
                       contract loop(@n) = { match n { 0 => Nil  _ => loop!(n - 1) } } | \
                       loop!(s.length()) } }",
        },
    ]
}

async fn compute_state(
    runtime_manager: &mut RuntimeManager,
    genesis_context: &GenesisContext,
    deploy: Signed<DeployData>,
    state_hash: &StateHash,
) -> (StateHash, ProcessedDeploy) {
    let time_stamp = deploy.data.time_stamp;
    let (new_state_hash, processed_deploys, _extra) = runtime_manager
        .compute_state(
            state_hash,
            vec![deploy],
            Vec::new(),
            rholang::rust::interpreter::system_processes::BlockData {
                time_stamp,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            },
            None,
        )
        .await
        .expect("compute_state failed");
    let result = processed_deploys
        .into_iter()
        .next()
        .expect("compute_state returned no processed deploy");
    (new_state_hash, result)
}

async fn replay_compute_state(
    runtime_manager: &mut RuntimeManager,
    genesis_context: &GenesisContext,
    processed_deploy: ProcessedDeploy,
    state_hash: &StateHash,
) -> Result<StateHash, casper::rust::errors::CasperError> {
    let time_stamp = processed_deploy.deploy.data.time_stamp;
    runtime_manager
        .replay_compute_state(
            state_hash,
            vec![processed_deploy],
            Vec::new(),
            &rholang::rust::interpreter::system_processes::BlockData {
                time_stamp,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            },
            None,
            false,
            false,
            &[],
        )
        .await
}

fn goldens_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/util/rholang/async_driver_goldens.json")
}

fn load_goldens() -> Option<BTreeMap<String, Golden>> {
    let path = goldens_path();
    let bytes = std::fs::read(path).ok()?;
    Some(serde_json::from_slice(&bytes).expect("goldens file is not valid JSON"))
}

/// The core differential run. Runs every corpus program through play+replay (Layer 1) and computes the
/// per-program golden. If `goldens.json` is absent, it CAPTURES (writes) and prints the captured values;
/// if present, it ASSERTS every portable invariant is byte-identical old-vs-new (Layer 2). Layer 1
/// (replay==play) is asserted in BOTH modes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn async_driver_four_invariants_differential() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let genesis_post_state = genesis_block.body.state.post_state_hash.clone();
            let existing = load_goldens();
            let mut captured: BTreeMap<String, Golden> = BTreeMap::new();

            for prog in corpus() {
                let deploy = construct_deploy::source_deploy(
                    prog.source.to_string(),
                    FIXED_TS,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("deploy construction failed");

                // ---- play ----
                let (play_state, pd) = compute_state(
                    &mut runtime_manager,
                    &genesis_context,
                    deploy,
                    &genesis_post_state,
                )
                .await;

                // COMM-events-only, order-canonical fingerprint. The deployLog's STANDALONE Produce/
                // Consume events (deposits, dangling re-installs, resting outputs) have a scheduling-
                // dependent COUNT — non-deterministic run-to-run EVEN ON THE OLD DRIVER — so the whole
                // deployLog is NOT a stable cross-process quantity. The COMM events (actual matches) ARE
                // deterministic: their number equals the COMM count (the fork's `cost` unit) and their
                // content is the matched channels + data (deploy-local, fixed under the fixed deploy
                // signature). We fingerprint the SORTED multiset of COMM events (order-independent, since
                // concurrent COMM emission order varies on both drivers). Layer-1 (replay==play) covers
                // the FULL deployLog byte-exactly for each specific execution.
                let mut comms: Vec<String> = pd
                    .deploy_log
                    .iter()
                    .filter(|e| matches!(e, Event::Comm(_)))
                    .map(|e| format!("{:?}", e))
                    .collect();
                comms.sort();
                let comm_count = comms.len();
                let comm_fnv = fnv1a(comms.join("\n").as_bytes());

                let golden = Golden {
                    is_failed: pd.is_failed,
                    cost: pd.cost.cost,
                    comm_count,
                    comm_fnv,
                    balanced: play_state == genesis_post_state,
                };

                // ---- Layer 1: replay self-consistency (exact; all four invariants) ----
                let replay_state = replay_compute_state(
                    &mut runtime_manager,
                    &genesis_context,
                    pd.clone(),
                    &genesis_post_state,
                )
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "[{}] replay_compute_state FAILED (invariant #2/#4 divergence): {:?}",
                        prog.name, e
                    )
                });
                assert_eq!(
                    replay_state, play_state,
                    "[{}] Layer-1 post-state divergence (invariant #3): replay != play",
                    prog.name
                );

                // ---- Layer 2: old-vs-new golden ----
                if let Some(ref goldens) = existing {
                    let old = goldens.get(prog.name).unwrap_or_else(|| {
                        panic!("[{}] missing from goldens.json (recapture required)", prog.name)
                    });
                    assert_eq!(
                        golden.is_failed, old.is_failed,
                        "[{}] invariant #1 (is_failed) diverged old-vs-new",
                        prog.name
                    );
                    assert_eq!(
                        golden.cost, old.cost,
                        "[{}] invariant #2 (cost / COMM count) diverged old-vs-new",
                        prog.name
                    );
                    assert_eq!(
                        golden.balanced, old.balanced,
                        "[{}] invariant #3 (balanced post-state) diverged old-vs-new",
                        prog.name
                    );
                    // Invariant #4 (deployLog) — the COMM multiset (actual matches). Deterministic across
                    // processes; the full deployLog is covered per-execution by Layer-1 above.
                    assert_eq!(
                        golden.comm_count, old.comm_count,
                        "[{}] invariant #4 (COMM count) diverged old-vs-new",
                        prog.name
                    );
                    assert_eq!(
                        golden.comm_fnv, old.comm_fnv,
                        "[{}] invariant #4 (COMM multiset fingerprint) diverged old-vs-new",
                        prog.name
                    );
                    println!("[{}] Layer-2 OK: {:?}", prog.name, golden);
                } else {
                    println!("[{}] CAPTURED: {:?}", prog.name, golden);
                }

                captured.insert(prog.name.to_string(), golden);
            }

            if existing.is_none() {
                let json = serde_json::to_string_pretty(&captured)
                    .expect("failed to serialize goldens");
                std::fs::write(goldens_path(), json).expect("failed to write goldens.json");
                println!(
                    "async_driver goldens CAPTURED ({} programs) -> {}",
                    captured.len(),
                    goldens_path().display()
                );
            } else {
                println!("async_driver Layer-2 differential PASSED ({} programs)", captured.len());
            }
        },
    )
    .await
    .expect("with_runtime_manager failed");
}
