// P4 restart tests of the `hashmap:queue` space: its state lives in the one history trie of an
// LMDB store, so a node that restarts continues each collection where it stopped (P4 design
// D.5, plan phase P4 "space_queue_ord_survives_restart").
//
// Each test runs its steps in separate processes. The test binary starts itself again with
// `current_exe()` and the role variable `RRS_P4_RESTART_ROLE`, and each child runs one role of
// the same test. The children share an LMDB store in a directory under `RRS_SCRATCH`. The root
// of each step travels in a scratch file, and the space handle travels through
// `@"rrs-handle"` in the trie. The parent keeps the standard output of each child, so no
// test-harness line of a child reaches the output of the parent. It shows the standard error of
// a child that fails.
//
// Changed by decision 90 (C6, decision 69): spaces have one execution model, native funded
// execution. So each child plays its deploys on a native chain over the LMDB store
// (`support::native::NativeChain::open`), as the restart suite of P7 does
// (`reified_restart_rehydrate.rs`). Each play is certified by native replay from its pre-state,
// as proposer self-certification does. The player of the handle test also writes the evidence
// bytes of its deploy, and the parent certifies that deploy in a third process from the root of
// the creator: the journal check, the trace binding, native replay, the completeness check and
// the export. The legacy play runtimes and the rigged replay of the play log are commented out
// below.

// Added by decision 90 (C6): the native chain of the shared harness.
#[path = "support/mod.rs"]
mod support;

// Disabled by decision 90 (C6): the native chain plays the deploys.
// use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
// Disabled by decision 90 (C6): the native chain plays the deploys.
// use rholang::rust::interpreter::accounting::costs::Cost;
// use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
// Disabled by decision 90 (C6): the readers read the state of a native chain.
// use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::spaces::namespace::{self, CollectionKey, NativeSpace};
use rholang::rust::interpreter::spaces::record;
// Disabled by decision 90 (C6): the native chain opens the LMDB store and plays the deploys.
// use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
// Disabled by decision 90 (C6): the native chain opens the LMDB store.
// use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
// use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::MB;
// use rspace_plus_plus::rspace::shared::rspace_store_manager::mk_rspace_store_manager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, COMM};
// Added by decision 90 (C6): the native chain and the certification of the shared harness.
use support::native::{
    assert_reproduces, certify, decode_evidence, evidence_bytes, finish_injected,
    replay_environment_from, NativeChain, Played, AMPLE_LIMIT,
};
use support::session::Injections;

/// True when `event` is a COMM whose consume is on the channel with the hash `key`.
fn is_comm_on(event: &Event, key: &Blake2b256Hash) -> bool {
    match event {
        Event::Comm(comm) => comm.consume.channel_hashes.as_slice() == std::slice::from_ref(key),
        Event::IoEvent(_) => false,
    }
}

// Disabled by decision 90 (C6): the native chain funds its own deploy signature and limit, and it
// opens the LMDB store with the map size of the shared harness.
// const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-restart-deploy";
// const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-restart-payer";
// const PHLO: i64 = 100_000_000;
// const LMDB_MAP_SIZE: usize = 256 * MB;

/// The role of a child process.
const ROLE: &str = "RRS_P4_RESTART_ROLE";
/// The directory of the store and the scratch files of a run.
const DIRECTORY: &str = "RRS_P4_RESTART_DIR";
/// The scratch root of the verification jobs.
const SCRATCH: &str = "RRS_SCRATCH";

/// A deploy that creates a space and a name of it. The handle goes to `@"rrs-handle"` and the
/// name goes to `@"rrs-name"`.
const SETUP: &str = r#"
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?("default")) {
    @"rrs-handle"!(*queue) |
    for (a <- queue!?("gensym")) { @"rrs-name"!(*a) }
  }
}
"#;

/// Handle calls of a deploy after a restart. The handle continuation in the trie forwards each
/// call.
const CALLS: &str = r#"
for (queue <<- @"rrs-handle") { for (a <<- @"rrs-name") {
  for (fresh <- queue!?("gensym")) {
    for (@config <- queue!?("config")) {
      for (_ <- queue!?("produce", *a, [7, 8])) {
        for (@x, @y <- a) { @"rrs-calls"!(x, y, config) | fresh!(x) }
      }
    }
  }
} }
"#;

/// `body` with the handle bound to `queue` and the name bound to `a`.
fn with_space(body: &str) -> String {
    format!("for (queue <<- @\"rrs-handle\") {{ for (a <<- @\"rrs-name\") {{\n{body}\n}} }}")
}

/// The scratch root: `RRS_SCRATCH`, or the scratch directory of the verification scripts.
fn scratch_root() -> PathBuf {
    match std::env::var_os(SCRATCH) {
        Some(root) => PathBuf::from(root),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/verification/reified-rspaces/scratch"),
    }
}

/// A fresh directory for one run of `test`.
fn run_directory(test: &str) -> PathBuf {
    let directory = scratch_root().join(format!("p4-restart-{test}-{}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).expect("remove an old run directory");
    }
    std::fs::create_dir_all(&directory).expect("create the run directory");
    directory
}

/// The directory of the run of a child process.
fn child_directory() -> PathBuf {
    PathBuf::from(std::env::var_os(DIRECTORY).expect("a child knows its run directory"))
}

/// Runs the test `test` of this binary as a child process with the role `role`.
async fn spawn(test: &str, role: &str, directory: &Path) {
    let output =
        tokio::process::Command::new(std::env::current_exe().expect("the path of the test binary"))
            .args([test, "--exact", "--nocapture", "--test-threads", "1"])
            .env(ROLE, role)
            .env(DIRECTORY, directory)
            .output()
            .await
            .expect("run the child process");
    assert!(
        output.status.success(),
        "the {role} child of {test} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// Changed by decision 90 (C6): a child opens a native chain over the LMDB store
// (`NativeChain::open`), and [`play`] plays and certifies each deploy. The legacy runtimes follow.
// /// A play runtime and a replay runtime over the LMDB store of `directory`.
// async fn open(directory: &Path) -> (RhoRuntimeImpl, RhoRuntimeImpl) {
//     let mut manager = mk_rspace_store_manager(directory.join("lmdb"), LMDB_MAP_SIZE);
//     let stores = manager
//         .r_space_stores()
//         .await
//         .expect("open the LMDB RSpace stores");
//     let (play, replay, _history) = create_runtimes(stores, false, &mut Vec::new()).await;
//     for runtime in [&play, &replay] {
//         runtime
//             .cost
//             .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
//     }
//     (play, replay)
// }
//
// async fn evaluate(runtime: &RhoRuntimeImpl, source: &str, label: &str) -> i64 {
//     let result = runtime
//         .evaluate(
//             source,
//             Cost::create(PHLO, label.to_owned()),
//             HashMap::new(),
//             Blake2b512Random::create_from_bytes(label.as_bytes()),
//         )
//         .await
//         .unwrap_or_else(|error| panic!("{label}: the evaluation returns no result: {error:?}"));
//     assert!(
//         result.errors.is_empty(),
//         "{label}: the deploy fails: {:?}",
//         result.errors
//     );
//     result.cost.value
// }

/// Added by decision 90 (C6): plays `source` on `chain` with the random state of `label`, checks
/// that the deploy has no error, and certifies it by native replay from its pre-state, as
/// proposer self-certification does.
async fn play(chain: &mut NativeChain, source: &str, label: &str) -> Played {
    let played = chain.play(source, label, AMPLE_LIMIT).await;
    assert!(
        played.result.errors.is_empty(),
        "{label}: the deploy fails: {:?}",
        played.result.errors
    );
    let certification = certify(&played, source, AMPLE_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    played
}

fn write_root(directory: &Path, file: &str, root: &Blake2b256Hash) {
    std::fs::write(directory.join(file), root.bytes()).expect("write a root file");
}

fn read_root(directory: &Path, file: &str) -> Blake2b256Hash {
    Blake2b256Hash::from_bytes(std::fs::read(directory.join(file)).expect("read a root file"))
}

/// The tagged bincode bytes of a log: `0` and a COMM, or `1` and an IO event.
fn encode_log(log: &[Event]) -> Vec<u8> {
    let entries: Vec<(u8, Vec<u8>)> = log
        .iter()
        .map(|event| match event {
            Event::Comm(comm) => (0, bincode::serialize(comm).expect("serialize a COMM")),
            Event::IoEvent(io) => (1, bincode::serialize(io).expect("serialize an IO event")),
        })
        .collect();
    bincode::serialize(&entries).expect("serialize a log")
}

fn decode_log(bytes: &[u8]) -> Vec<Event> {
    let entries: Vec<(u8, Vec<u8>)> = bincode::deserialize(bytes).expect("decode a log");
    entries
        .into_iter()
        .map(|(tag, bytes)| match tag {
            0 => Event::Comm(bincode::deserialize::<COMM>(&bytes).expect("decode a COMM")),
            1 => {
                Event::IoEvent(bincode::deserialize::<IOEvent>(&bytes).expect("decode an IO event"))
            }
            other => panic!("unknown log entry tag {other}"),
        })
        .collect()
}

fn int(value: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }],
        ..Par::default()
    }
}

// Changed by decision 90 (C6): the reader reads the state of a native chain. The legacy reader
// follows.
// async fn sole_datum(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<Par> {
//     let channel = Par {
//         exprs: vec![Expr {
//             expr_instance: Some(ExprInstance::GString(channel.to_owned())),
//         }],
//         ..Par::default()
//     };
//     match <[_; 1]>::try_from(runtime.get_data(&channel).await) {
//         Ok([datum]) => datum.a.pars,
//         Err(data) => panic!("the channel holds {} data", data.len()),
//     }
// }
async fn sole_datum(chain: &NativeChain, channel: &str) -> Vec<Par> {
    match <[_; 1]>::try_from(chain.values(channel).await) {
        Ok([pars]) => pars,
        Err(data) => panic!("the channel holds {} data", data.len()),
    }
}

/// The space and the name of the setup, read from the trie.
// Changed by decision 90 (C6): the reader reads the state of a native chain.
// async fn space_and_name(runtime: &RhoRuntimeImpl) -> (NativeSpace, Par) {
//     let handle = sole_datum(runtime, "rrs-handle")
async fn space_and_name(chain: &NativeChain) -> (NativeSpace, Par) {
    let handle = sole_datum(chain, "rrs-handle")
        .await
        .into_iter()
        .next()
        .expect("the handle datum holds the handle");
    let space = match classify_space_ref(&handle) {
        Ok(SpaceRef::Native(space)) => space,
        other => panic!("the handle is not a native space: {other:?}"),
    };
    // Changed by decision 90 (C6): the reader reads the state of a native chain.
    // let name = sole_datum(runtime, "rrs-name")
    let name = sole_datum(chain, "rrs-name")
        .await
        .into_iter()
        .next()
        .expect("the name datum holds the name");
    (space, name)
}

/// The `ord` values of the records of the collection of `name`, in `ord` order.
// Changed by decision 90 (C6): the reader reads the state of a native chain.
// async fn ords(runtime: &RhoRuntimeImpl, space: NativeSpace, name: &Par) -> Vec<i64> {
//     let key = CollectionKey::of_channel(name).expect("a name has a collection key");
//     let mut ords: Vec<i64> = runtime
//         .get_data(&namespace::gprivate(namespace::data_channel(space, &key)))
async fn ords(chain: &NativeChain, space: NativeSpace, name: &Par) -> Vec<i64> {
    let key = CollectionKey::of_channel(name).expect("a name has a collection key");
    let mut ords: Vec<i64> = chain
        .data(&namespace::gprivate(namespace::data_channel(space, &key)))
        .await
        .iter()
        .map(|datum| record::datum_ord(&datum.a).expect("a datum record decodes"))
        .collect();
    ords.sort_unstable();
    ords
}

// Changed by decision 90 (C6): the reader reads the state of a native chain.
// async fn counter(runtime: &RhoRuntimeImpl, space: NativeSpace, name: &Par) -> i64 {
//     let key = CollectionKey::of_channel(name).expect("a name has a collection key");
//     let data = runtime
//         .get_data(&namespace::gprivate(namespace::collection_counter(
async fn counter(chain: &NativeChain, space: NativeSpace, name: &Par) -> i64 {
    let key = CollectionKey::of_channel(name).expect("a name has a collection key");
    let data = chain
        .data(&namespace::gprivate(namespace::collection_counter(
            space, &key,
        )))
        .await;
    match data.as_slice() {
        [datum] => record::counter_value(&datum.a).expect("a counter record decodes"),
        other => panic!("the counter channel holds {} records", other.len()),
    }
}

// Changed by decision 90 (C6): each role opens a native chain over the LMDB store, at the empty
// root for the writer and at the root of the writer for the reader. The legacy roles follow.
// /// The child roles of `space_queue_ord_survives_restart`.
// async fn ord_child(role: &str, directory: &Path) {
//     let (mut play, _replay) = open(directory).await;
//     match role {
//         "write" => {
//             evaluate(&play, SETUP, "rrs-p4-restart-setup").await;
//             evaluate(&play, &with_space("a!(1)"), "rrs-p4-restart-enqueue-1").await;
//             evaluate(&play, &with_space("a!(2)"), "rrs-p4-restart-enqueue-2").await;
//             let (space, name) = space_and_name(&play).await;
//             assert_eq!(ords(&play, space, &name).await, vec![0, 1]);
//             let root = play.create_checkpoint().await.root;
//             write_root(directory, "root-write", &root);
//         }
//         "read" => {
//             let root = read_root(directory, "root-write");
//             play.reset(&root)
//                 .await
//                 .expect("reset to the root of the writer");
//             let (space, name) = space_and_name(&play).await;
//             assert_eq!(
//                 ords(&play, space, &name).await,
//                 vec![0, 1],
//                 "the records survive"
//             );
//             assert_eq!(
//                 counter(&play, space, &name).await,
//                 2,
//                 "the counter survives"
//             );
//             evaluate(&play, &with_space("a!(3)"), "rrs-p4-restart-enqueue-3").await;
//             assert_eq!(
//                 ords(&play, space, &name).await,
//                 vec![0, 1, 2],
//                 "the first record after the restart takes the next ord"
//             );
//             evaluate(
//                 &play,
//                 &with_space(
//                     r#"for (@x <- a) { for (@y <- a) { for (@z <- a) { @"rrs-fifo"!([x, y, z]) } } }"#,
//                 ),
//                 "rrs-p4-restart-dequeue",
//             )
//             .await;
//             let root = play.create_checkpoint().await.root;
//             write_root(directory, "root-read", &root);
//         }
//         other => panic!("unknown role {other}"),
//     }
// }
/// The child roles of `space_queue_ord_survives_restart`.
async fn ord_child(role: &str, directory: &Path) {
    match role {
        "write" => {
            let mut chain = NativeChain::open(&directory.join("lmdb"), None).await;
            play(&mut chain, SETUP, "rrs-p4-restart-setup").await;
            play(&mut chain, &with_space("a!(1)"), "rrs-p4-restart-enqueue-1").await;
            play(&mut chain, &with_space("a!(2)"), "rrs-p4-restart-enqueue-2").await;
            let (space, name) = space_and_name(&chain).await;
            assert_eq!(ords(&chain, space, &name).await, vec![0, 1]);
            let root = chain.checkpoint().await;
            write_root(directory, "root-write", &root);
        }
        "read" => {
            let root = read_root(directory, "root-write");
            let mut chain = NativeChain::open(&directory.join("lmdb"), Some(&root)).await;
            let (space, name) = space_and_name(&chain).await;
            assert_eq!(
                ords(&chain, space, &name).await,
                vec![0, 1],
                "the records survive"
            );
            assert_eq!(
                counter(&chain, space, &name).await,
                2,
                "the counter survives"
            );
            play(&mut chain, &with_space("a!(3)"), "rrs-p4-restart-enqueue-3").await;
            assert_eq!(
                ords(&chain, space, &name).await,
                vec![0, 1, 2],
                "the first record after the restart takes the next ord"
            );
            play(
                &mut chain,
                &with_space(
                    r#"for (@x <- a) { for (@y <- a) { for (@z <- a) { @"rrs-fifo"!([x, y, z]) } } }"#,
                ),
                "rrs-p4-restart-dequeue",
            )
            .await;
            let root = chain.checkpoint().await;
            write_root(directory, "root-read", &root);
        }
        other => panic!("unknown role {other}"),
    }
}

// Changed by decision 90 (C6): each role opens a native chain over the LMDB store. The player
// also writes the evidence bytes of its deploy and the usage of its budget recording, so the
// parent can certify the deploy. The legacy roles follow.
// /// The child roles of `reified_handle_persisted_restart_replay`.
// async fn handle_child(role: &str, directory: &Path) {
//     let (mut play, _replay) = open(directory).await;
//     match role {
//         "create" => {
//             evaluate(&play, SETUP, "rrs-p4-restart-handle-setup").await;
//             let root = play.create_checkpoint().await.root;
//             write_root(directory, "root-create", &root);
//         }
//         "play" => {
//             let root = read_root(directory, "root-create");
//             play.reset(&root)
//                 .await
//                 .expect("reset to the root of the creator");
//             let cost = evaluate(&play, CALLS, "rrs-p4-restart-handle-calls").await;
//             let checkpoint = play.create_checkpoint().await;
//             write_root(directory, "root-play", &checkpoint.root);
//             std::fs::write(directory.join("log-play"), encode_log(&checkpoint.log))
//                 .expect("write the play log");
//             std::fs::write(directory.join("cost-play"), cost.to_le_bytes())
//                 .expect("write the play cost");
//         }
//         other => panic!("unknown role {other}"),
//     }
// }
/// The child roles of `reified_handle_persisted_restart_replay`.
async fn handle_child(role: &str, directory: &Path) {
    match role {
        "create" => {
            let mut chain = NativeChain::open(&directory.join("lmdb"), None).await;
            play(&mut chain, SETUP, "rrs-p4-restart-handle-setup").await;
            let root = chain.checkpoint().await;
            write_root(directory, "root-create", &root);
        }
        "play" => {
            let root = read_root(directory, "root-create");
            let mut chain = NativeChain::open(&directory.join("lmdb"), Some(&root)).await;
            let played = play(&mut chain, CALLS, "rrs-p4-restart-handle-calls").await;
            write_root(directory, "root-play", &played.root);
            std::fs::write(directory.join("log-play"), encode_log(&played.log))
                .expect("write the play log");
            let (budget_bytes, journal_bytes) = evidence_bytes(&played);
            std::fs::write(directory.join("budget-play"), budget_bytes)
                .expect("write the evidence bytes of the budget recording");
            std::fs::write(directory.join("journal-play"), journal_bytes)
                .expect("write the evidence bytes of the operation journal");
            let used = played
                .result
                .native_budget_recording
                .as_ref()
                .expect("native play records its budget")
                .used;
            std::fs::write(directory.join("cost-play"), used.to_le_bytes())
                .expect("write the usage of play");
        }
        other => panic!("unknown role {other}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_queue_ord_survives_restart() {
    if let Ok(role) = std::env::var(ROLE) {
        return ord_child(&role, &child_directory()).await;
    }
    let directory = run_directory("ord");
    spawn("space_queue_ord_survives_restart", "write", &directory).await;
    spawn("space_queue_ord_survives_restart", "read", &directory).await;
    // The parent is a third process. It reads the store at the root of the reader.
    // Changed by decision 90 (C6): the parent opens a native chain at the root of the reader. The
    // legacy reader follows.
    // let (mut play, _replay) = open(&directory).await;
    // let root = read_root(&directory, "root-read");
    // play.reset(&root)
    //     .await
    //     .expect("reset to the root of the reader");
    // assert_eq!(
    //     sole_datum(&play, "rrs-fifo").await,
    let root = read_root(&directory, "root-read");
    let chain = NativeChain::open(&directory.join("lmdb"), Some(&root)).await;
    assert_eq!(
        sole_datum(&chain, "rrs-fifo").await,
        vec![models::rust::utils::new_elist_par(
            vec![int(1), int(2), int(3)],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false
        )],
        "the queue delivers in arrival order across the restart"
    );
    // Changed by decision 90 (C6): the readers read the state of the native chain.
    // let (space, name) = space_and_name(&play).await;
    // assert!(
    //     ords(&play, space, &name).await.is_empty(),
    //     "the queue is empty"
    // );
    // assert_eq!(
    //     counter(&play, space, &name).await,
    //     6,
    //     "three enqueues and three dequeues bump the counter six times"
    // );
    // drop(play);
    let (space, name) = space_and_name(&chain).await;
    assert!(
        ords(&chain, space, &name).await.is_empty(),
        "the queue is empty"
    );
    assert_eq!(
        counter(&chain, space, &name).await,
        6,
        "three enqueues and three dequeues bump the counter six times"
    );
    drop(chain);
    std::fs::remove_dir_all(&directory).expect("remove the run directory");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_handle_persisted_restart_replay() {
    if let Ok(role) = std::env::var(ROLE) {
        return handle_child(&role, &child_directory()).await;
    }
    let directory = run_directory("handle");
    spawn(
        "reified_handle_persisted_restart_replay",
        "create",
        &directory,
    )
    .await;
    spawn(
        "reified_handle_persisted_restart_replay",
        "play",
        &directory,
    )
    .await;
    // The parent replays the deploy of the player from the root of the creator: the handle
    // continuation comes from the trie of the restarted store.
    // Changed by decision 90 (C6): the parent certifies the deploy natively from the evidence
    // bytes and the event log of the player, on a native chain at the root of the creator. The
    // legacy runtimes follow.
    // let (play, mut replay) = open(&directory).await;
    let created = read_root(&directory, "root-create");
    let played = read_root(&directory, "root-play");
    let log = decode_log(&std::fs::read(directory.join("log-play")).expect("read the play log"));
    let handle_op = namespace::gprivate(namespace::op_channel(namespace::HANDLE_OP));
    let handle_op_key = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&handle_op);
    assert_eq!(
        log.iter()
            .filter(|event| is_comm_on(event, &handle_op_key))
            .count(),
        3,
        "the three handle calls reach HANDLE_OP through the persisted handle continuation"
    );
    // Changed by decision 90 (C6): native replay certifies the deploy from its evidence, as a
    // validator does. The legacy rigged replay follows.
    // let cost = i64::from_le_bytes(
    //     std::fs::read(directory.join("cost-play"))
    //         .expect("read the play cost")
    //         .try_into()
    //         .expect("a cost has eight bytes"),
    // );
    // replay
    //     .reset(&created)
    //     .await
    //     .expect("reset the replay runtime to the root of the creator");
    // replay
    //     .rig(log)
    //     .await
    //     .expect("rig the replay runtime with the play log");
    // let replayed = evaluate(&replay, CALLS, "rrs-p4-restart-handle-calls").await;
    // replay
    //     .check_replay_data()
    //     .await
    //     .expect("the replay consumes the complete play log");
    // let root = replay.create_checkpoint().await.root;
    // assert_eq!(root, played, "the replay reaches the root of the player");
    // assert_eq!(replayed, cost, "the replay charges the play cost");
    // // The play result is in the trie of the player root.
    // let mut reader = play;
    // reader
    //     .reset(&played)
    //     .await
    //     .expect("reset to the root of the player");
    // let calls = sole_datum(&reader, "rrs-calls").await;
    // assert_eq!(calls.first(), Some(&int(7)));
    // assert_eq!(calls.get(1), Some(&int(8)));
    // drop(reader);
    // drop(replay);
    // std::fs::remove_dir_all(&directory).expect("remove the run directory");
    let used = u64::from_le_bytes(
        std::fs::read(directory.join("cost-play"))
            .expect("read the usage of play")
            .try_into()
            .expect("a usage has eight bytes"),
    );
    let (recording, journal) = decode_evidence(
        &std::fs::read(directory.join("budget-play")).expect("read the budget evidence"),
        &std::fs::read(directory.join("journal-play")).expect("read the journal evidence"),
    );
    assert_eq!(recording.used, used, "the evidence keeps the usage of play");
    let mut chain = NativeChain::open(&directory.join("lmdb"), Some(&created)).await;
    let environment =
        replay_environment_from(chain.history(), log, AMPLE_LIMIT, &recording, journal)
            .await
            .unwrap_or_else(|error| panic!("the evidence of the player checks: {error}"));
    let certification = finish_injected(
        environment,
        CALLS,
        Blake2b512Random::create_from_bytes("rrs-p4-restart-handle-calls".as_bytes()),
        Injections::new(),
    )
    .await;
    let evaluation = certification
        .evaluation
        .as_ref()
        .unwrap_or_else(|error| panic!("native replay evaluates: {error:?}"));
    assert!(
        evaluation.errors.is_empty(),
        "the replay reports errors: {:?}",
        evaluation.errors
    );
    assert_eq!(
        certification.complete,
        Ok(()),
        "the replay consumes every journal row of play"
    );
    assert_eq!(
        certification.usage,
        Ok(used),
        "the replay uses the phlo of play"
    );
    assert_eq!(
        certification.root,
        Ok(played.clone()),
        "the replay reaches the root of the player"
    );
    // The play result is in the trie of the player root.
    chain.reset(&played).await;
    let calls = sole_datum(&chain, "rrs-calls").await;
    assert_eq!(calls.first(), Some(&int(7)));
    assert_eq!(calls.get(1), Some(&int(8)));
    drop(chain);
    std::fs::remove_dir_all(&directory).expect("remove the run directory");
}
