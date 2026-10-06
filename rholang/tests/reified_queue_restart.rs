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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::spaces::namespace::{self, CollectionKey, NativeSpace};
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::MB;
use rspace_plus_plus::rspace::shared::rspace_store_manager::mk_rspace_store_manager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, COMM};

/// True when `event` is a COMM whose consume is on the channel with the hash `key`.
fn is_comm_on(event: &Event, key: &Blake2b256Hash) -> bool {
    match event {
        Event::Comm(comm) => comm.consume.channel_hashes.as_slice() == std::slice::from_ref(key),
        Event::IoEvent(_) => false,
    }
}

const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-restart-deploy";
const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-restart-payer";
const PHLO: i64 = 100_000_000;
const LMDB_MAP_SIZE: usize = 256 * MB;

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

/// A play runtime and a replay runtime over the LMDB store of `directory`.
async fn open(directory: &Path) -> (RhoRuntimeImpl, RhoRuntimeImpl) {
    let mut manager = mk_rspace_store_manager(directory.join("lmdb"), LMDB_MAP_SIZE);
    let stores = manager
        .r_space_stores()
        .await
        .expect("open the LMDB RSpace stores");
    let (play, replay, _history) = create_runtimes(stores, false, &mut Vec::new()).await;
    for runtime in [&play, &replay] {
        runtime
            .cost
            .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
    }
    (play, replay)
}

async fn evaluate(runtime: &RhoRuntimeImpl, source: &str, label: &str) -> i64 {
    let result = runtime
        .evaluate(
            source,
            Cost::create(PHLO, label.to_owned()),
            HashMap::new(),
            Blake2b512Random::create_from_bytes(label.as_bytes()),
        )
        .await
        .unwrap_or_else(|error| panic!("{label}: the evaluation returns no result: {error:?}"));
    assert!(
        result.errors.is_empty(),
        "{label}: the deploy fails: {:?}",
        result.errors
    );
    result.cost.value
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

async fn sole_datum(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<Par> {
    let channel = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(channel.to_owned())),
        }],
        ..Par::default()
    };
    match <[_; 1]>::try_from(runtime.get_data(&channel).await) {
        Ok([datum]) => datum.a.pars,
        Err(data) => panic!("the channel holds {} data", data.len()),
    }
}

/// The space and the name of the setup, read from the trie.
async fn space_and_name(runtime: &RhoRuntimeImpl) -> (NativeSpace, Par) {
    let handle = sole_datum(runtime, "rrs-handle")
        .await
        .into_iter()
        .next()
        .expect("the handle datum holds the handle");
    let space = match classify_space_ref(&handle) {
        Ok(SpaceRef::Native(space)) => space,
        other => panic!("the handle is not a native space: {other:?}"),
    };
    let name = sole_datum(runtime, "rrs-name")
        .await
        .into_iter()
        .next()
        .expect("the name datum holds the name");
    (space, name)
}

/// The `ord` values of the records of the collection of `name`, in `ord` order.
async fn ords(runtime: &RhoRuntimeImpl, space: NativeSpace, name: &Par) -> Vec<i64> {
    let key = CollectionKey::of_channel(name).expect("a name has a collection key");
    let mut ords: Vec<i64> = runtime
        .get_data(&namespace::gprivate(namespace::data_channel(space, &key)))
        .await
        .iter()
        .map(|datum| record::datum_ord(&datum.a).expect("a datum record decodes"))
        .collect();
    ords.sort_unstable();
    ords
}

async fn counter(runtime: &RhoRuntimeImpl, space: NativeSpace, name: &Par) -> i64 {
    let key = CollectionKey::of_channel(name).expect("a name has a collection key");
    let data = runtime
        .get_data(&namespace::gprivate(namespace::collection_counter(
            space, &key,
        )))
        .await;
    match data.as_slice() {
        [datum] => record::counter_value(&datum.a).expect("a counter record decodes"),
        other => panic!("the counter channel holds {} records", other.len()),
    }
}

/// The child roles of `space_queue_ord_survives_restart`.
async fn ord_child(role: &str, directory: &Path) {
    let (mut play, _replay) = open(directory).await;
    match role {
        "write" => {
            evaluate(&play, SETUP, "rrs-p4-restart-setup").await;
            evaluate(&play, &with_space("a!(1)"), "rrs-p4-restart-enqueue-1").await;
            evaluate(&play, &with_space("a!(2)"), "rrs-p4-restart-enqueue-2").await;
            let (space, name) = space_and_name(&play).await;
            assert_eq!(ords(&play, space, &name).await, vec![0, 1]);
            let root = play.create_checkpoint().await.root;
            write_root(directory, "root-write", &root);
        }
        "read" => {
            let root = read_root(directory, "root-write");
            play.reset(&root)
                .await
                .expect("reset to the root of the writer");
            let (space, name) = space_and_name(&play).await;
            assert_eq!(
                ords(&play, space, &name).await,
                vec![0, 1],
                "the records survive"
            );
            assert_eq!(
                counter(&play, space, &name).await,
                2,
                "the counter survives"
            );
            evaluate(&play, &with_space("a!(3)"), "rrs-p4-restart-enqueue-3").await;
            assert_eq!(
                ords(&play, space, &name).await,
                vec![0, 1, 2],
                "the first record after the restart takes the next ord"
            );
            evaluate(
                &play,
                &with_space(
                    r#"for (@x <- a) { for (@y <- a) { for (@z <- a) { @"rrs-fifo"!([x, y, z]) } } }"#,
                ),
                "rrs-p4-restart-dequeue",
            )
            .await;
            let root = play.create_checkpoint().await.root;
            write_root(directory, "root-read", &root);
        }
        other => panic!("unknown role {other}"),
    }
}

/// The child roles of `reified_handle_persisted_restart_replay`.
async fn handle_child(role: &str, directory: &Path) {
    let (mut play, _replay) = open(directory).await;
    match role {
        "create" => {
            evaluate(&play, SETUP, "rrs-p4-restart-handle-setup").await;
            let root = play.create_checkpoint().await.root;
            write_root(directory, "root-create", &root);
        }
        "play" => {
            let root = read_root(directory, "root-create");
            play.reset(&root)
                .await
                .expect("reset to the root of the creator");
            let cost = evaluate(&play, CALLS, "rrs-p4-restart-handle-calls").await;
            let checkpoint = play.create_checkpoint().await;
            write_root(directory, "root-play", &checkpoint.root);
            std::fs::write(directory.join("log-play"), encode_log(&checkpoint.log))
                .expect("write the play log");
            std::fs::write(directory.join("cost-play"), cost.to_le_bytes())
                .expect("write the play cost");
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
    let (mut play, _replay) = open(&directory).await;
    let root = read_root(&directory, "root-read");
    play.reset(&root)
        .await
        .expect("reset to the root of the reader");
    assert_eq!(
        sole_datum(&play, "rrs-fifo").await,
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
    let (space, name) = space_and_name(&play).await;
    assert!(
        ords(&play, space, &name).await.is_empty(),
        "the queue is empty"
    );
    assert_eq!(
        counter(&play, space, &name).await,
        6,
        "three enqueues and three dequeues bump the counter six times"
    );
    drop(play);
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
    let (play, mut replay) = open(&directory).await;
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
    let cost = i64::from_le_bytes(
        std::fs::read(directory.join("cost-play"))
            .expect("read the play cost")
            .try_into()
            .expect("a cost has eight bytes"),
    );
    replay
        .reset(&created)
        .await
        .expect("reset the replay runtime to the root of the creator");
    replay
        .rig(log)
        .await
        .expect("rig the replay runtime with the play log");
    let replayed = evaluate(&replay, CALLS, "rrs-p4-restart-handle-calls").await;
    replay
        .check_replay_data()
        .await
        .expect("the replay consumes the complete play log");
    let root = replay.create_checkpoint().await.root;
    assert_eq!(root, played, "the replay reaches the root of the player");
    assert_eq!(replayed, cost, "the replay charges the play cost");
    // The play result is in the trie of the player root.
    let mut reader = play;
    reader
        .reset(&played)
        .await
        .expect("reset to the root of the player");
    let calls = sole_datum(&reader, "rrs-calls").await;
    assert_eq!(calls.first(), Some(&int(7)));
    assert_eq!(calls.get(1), Some(&int(8)));
    drop(reader);
    drop(replay);
    std::fs::remove_dir_all(&directory).expect("remove the run directory");
}
