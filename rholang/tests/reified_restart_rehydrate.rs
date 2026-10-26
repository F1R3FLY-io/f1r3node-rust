//! The restart of P7 across three processes (design v3 section 10, plan revision M-5).
//!
//! The test binary starts itself again with `current_exe()` and the role variable
//! `RRS_P7_RESTART_ROLE`, and each child runs one role over one LMDB store. The store is in a
//! run directory under `RRS_SCRATCH`, or under `target/verification/reified-rspaces/scratch`,
//! and never in the RAM-backed temporary directory of the host. The children take these steps:
//!
//! 1. The child `create` opens a new store and plays block 1: one deploy for each stored family
//!    (a queue, a set, a priority queue, a VectorDB space and a PathMap space). Each deploy
//!    leaves records and a waiter in its space and stores the handle and two names on a public
//!    channel. The last checkpoint is the root R1.
//! 2. The child `touch` opens the store at R1 in a new process, so each space attaches lazily
//!    from the trie. It plays block 2 for each family (a handle call `config`, a take, a fire of
//!    the stored waiter and a store), and the last checkpoint is the root R2.
//! 3. The child `replay` opens the store in a third process. It certifies each deploy of block 2
//!    from its pre-state through the committed evidence bytes and the event log, as a validator
//!    does: the journal check, the trace binding, native replay, the completeness check and the
//!    export. Each certification reaches the post-state root of play with the phlo of play.
//!
//! Each child that plays also certifies its plays, as proposer self-certification does, and it
//! writes a record of each deploy: the pre-state and post-state roots, the cost vector, the
//! evidence bytes and the event log. The parent then plays both blocks warm, in one process over
//! an in-memory store, and checks every record of the children against its own: R1, R2, the root
//! of each deploy, each cost vector, the evidence bytes and the event logs.
//!
//! The parent keeps the standard output of each child, so no test-harness line of a child
//! reaches the output of the parent. It shows the standard error of a child that fails.

#[path = "support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use rholang::rust::interpreter::spaces::namespace::Durability;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use support::family_programs::{StoredFamily, STORED_FAMILIES};
use support::native::{
    assert_reproduces, certify, decode_evidence, decode_log, encode_log, evidence_bytes,
    finish_injected, replay_environment_from, NativeChain, Played, AMPLE_LIMIT,
};
use support::session::Injections;

/// The role of a child process.
const ROLE: &str = "RRS_P7_RESTART_ROLE";
/// The run directory of a child process.
const DIRECTORY: &str = "RRS_P7_RESTART_DIR";
/// The scratch root of the verification jobs.
const SCRATCH: &str = "RRS_SCRATCH";
/// The name of the test, which the children run.
const TEST: &str = "space_restart_rehydrate_three_processes";

// ─── Records ───────────────────────────────────────────────────────────────────────────────────

/// The record of one deploy that a child writes for the next process.
#[derive(Clone, Debug, PartialEq)]
struct DeployRecord {
    /// The label of the deploy, which seeds its random state.
    label: String,
    /// The root of its pre-state.
    pre_root: Vec<u8>,
    /// The root of its post-state.
    post_root: Vec<u8>,
    /// The phlo usage of native play.
    phlo: Option<u64>,
    /// The usage of its budget recording.
    used: u64,
    /// The evidence bytes of the budget recording.
    budget_bytes: Vec<u8>,
    /// The evidence bytes of the operation journal.
    journal_bytes: Vec<u8>,
    /// The bytes of the event log (`encode_log`).
    log: Vec<u8>,
}

/// The fields of a [`DeployRecord`] in its order, as bincode encodes them.
type RecordFields = (
    String,
    Vec<u8>,
    Vec<u8>,
    Option<u64>,
    u64,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
);

impl DeployRecord {
    fn fields(&self) -> RecordFields {
        (
            self.label.clone(),
            self.pre_root.clone(),
            self.post_root.clone(),
            self.phlo,
            self.used,
            self.budget_bytes.clone(),
            self.journal_bytes.clone(),
            self.log.clone(),
        )
    }

    fn of_fields(fields: RecordFields) -> Self {
        let (label, pre_root, post_root, phlo, used, budget_bytes, journal_bytes, log) = fields;
        Self {
            label,
            pre_root,
            post_root,
            phlo,
            used,
            budget_bytes,
            journal_bytes,
            log,
        }
    }
}

fn write_records(directory: &Path, block: &str, records: &[DeployRecord]) {
    let fields: Vec<RecordFields> = records.iter().map(DeployRecord::fields).collect();
    std::fs::write(
        directory.join(format!("records-{block}")),
        bincode::serialize(&fields).expect("serialize the records"),
    )
    .expect("write the records");
}

fn read_records(directory: &Path, block: &str) -> Vec<DeployRecord> {
    let bytes =
        std::fs::read(directory.join(format!("records-{block}"))).expect("read the records");
    let fields: Vec<RecordFields> = bincode::deserialize(&bytes).expect("decode the records");
    fields.into_iter().map(DeployRecord::of_fields).collect()
}

fn root(bytes: &[u8]) -> Blake2b256Hash { Blake2b256Hash::from_bytes(bytes.to_vec()) }

// ─── Plays ─────────────────────────────────────────────────────────────────────────────────────

/// Plays `term` on `chain` with the random state of `label`, checks that it has no error and
/// leaves no volatile channel, certifies it, and returns the play and its record.
async fn play_recorded(chain: &mut NativeChain, term: &str, label: &str) -> (Played, DeployRecord) {
    let pre_root = chain.history().root();
    let played = chain
        .play_with(term, label, AMPLE_LIMIT, Injections::new())
        .await;
    assert!(
        played.result.errors.is_empty(),
        "{label}: the deploy fails: {:?}",
        played.result.errors
    );
    assert!(
        played.residue.is_empty(),
        "{label}: the sweep leaves no volatile channel: {:?}",
        played.residue
    );
    let certification = certify(&played, term, AMPLE_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    let (budget_bytes, journal_bytes) = evidence_bytes(&played);
    let record = DeployRecord {
        label: label.to_string(),
        pre_root: pre_root.bytes(),
        post_root: played.root.bytes(),
        phlo: played.result.native_phlo_usage,
        used: played
            .result
            .native_budget_recording
            .as_ref()
            .expect("native play records its budget")
            .used,
        budget_bytes,
        journal_bytes,
        log: encode_log(&played.log),
    };
    (played, record)
}

fn creation_label(family: &StoredFamily) -> String { format!("create {}", family.label) }

fn touch_label(family: &StoredFamily) -> String { format!("touch {}", family.label) }

/// True when `played` holds on the public output channel the pair `[family, [what, …]]`.
fn delivers(played: &Played, family: &str, what: &str) -> bool {
    let string = |par: &Par| match par.exprs.as_slice() {
        [expr] => match &expr.expr_instance {
            Some(ExprInstance::GString(text)) => Some(text.clone()),
            _ => None,
        },
        _ => None,
    };
    let list = |par: &Par| match par.exprs.as_slice() {
        [expr] => match &expr.expr_instance {
            Some(ExprInstance::EListBody(list)) => Some(list.ps.clone()),
            _ => None,
        },
        _ => None,
    };
    played.out.iter().any(|pars| {
        let [pair] = pars.as_slice() else {
            return false;
        };
        let Some([first, inner]) = list(pair).and_then(|items| <[Par; 2]>::try_from(items).ok())
        else {
            return false;
        };
        let head = list(&inner).and_then(|items| items.first().and_then(string));
        string(&first).as_deref() == Some(family) && head.as_deref() == Some(what)
    })
}

// ─── Children ──────────────────────────────────────────────────────────────────────────────────

/// The scratch root: `RRS_SCRATCH`, or the scratch directory of the verification scripts.
fn scratch_root() -> PathBuf {
    match std::env::var_os(SCRATCH) {
        Some(root) => PathBuf::from(root),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/verification/reified-rspaces/scratch"),
    }
}

/// A fresh run directory.
fn run_directory() -> PathBuf {
    let directory = scratch_root().join(format!("p7-restart-{}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).expect("remove an old run directory");
    }
    std::fs::create_dir_all(&directory).expect("create the run directory");
    directory
}

/// Runs the role `role` of the test in a child process over `directory`.
async fn spawn(role: &str, directory: &Path) {
    let output =
        tokio::process::Command::new(std::env::current_exe().expect("the path of the test binary"))
            .args([TEST, "--exact", "--nocapture", "--test-threads", "1"])
            .env(ROLE, role)
            .env(DIRECTORY, directory)
            .output()
            .await
            .expect("run the child process");
    assert!(
        output.status.success(),
        "the {role} child failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The child `create`: block 1 on a new store.
async fn create(directory: &Path) {
    let mut chain = NativeChain::open(&directory.join("lmdb"), None).await;
    let mut records = Vec::with_capacity(STORED_FAMILIES.len());
    for family in &STORED_FAMILIES {
        let (_, record) =
            play_recorded(&mut chain, &family.creation(), &creation_label(family)).await;
        records.push(record);
    }
    write_records(directory, "create", &records);
}

/// The child `touch`: block 2 on the store at R1, in a process that holds no state of block 1.
async fn touch(directory: &Path) {
    let created = read_records(directory, "create");
    let r1 = root(&created.last().expect("block 1 has deploys").post_root);
    let mut chain = NativeChain::open(&directory.join("lmdb"), Some(&r1)).await;
    assert!(
        chain.space_channels(Durability::Durable).await.is_empty(),
        "the restarted process holds no space channel before its first operation"
    );
    let mut records = Vec::with_capacity(STORED_FAMILIES.len());
    for family in &STORED_FAMILIES {
        let (played, record) =
            play_recorded(&mut chain, &family.touching(), &touch_label(family)).await;
        for what in ["config", "take", "fired"] {
            assert!(
                delivers(&played, family.label, what),
                "{}: block 2 delivers {what} after the restart: {:?}",
                family.label,
                played.outputs
            );
        }
        records.push(record);
    }
    write_records(directory, "touch", &records);
}

/// The child `replay`: certifies each deploy of block 2 from its pre-state through the evidence
/// bytes and the event log.
async fn replay(directory: &Path) {
    let touched = read_records(directory, "touch");
    let first = touched.first().expect("block 2 has deploys");
    let mut chain = NativeChain::open(&directory.join("lmdb"), Some(&root(&first.pre_root))).await;
    for (family, record) in STORED_FAMILIES.iter().zip(&touched) {
        let label = &record.label;
        chain.reset(&root(&record.pre_root)).await;
        let (recording, journal) = decode_evidence(&record.budget_bytes, &record.journal_bytes);
        let environment = replay_environment_from(
            chain.history(),
            decode_log(&record.log),
            AMPLE_LIMIT,
            &recording,
            journal,
        )
        .await
        .unwrap_or_else(|error| panic!("{label}: the evidence checks: {error}"));
        let certification = finish_injected(
            environment,
            &family.touching(),
            Blake2b512Random::create_from_bytes(label.as_bytes()),
            Injections::new(),
        )
        .await;
        let evaluation = certification
            .evaluation
            .as_ref()
            .unwrap_or_else(|error| panic!("{label}: native replay evaluates: {error:?}"));
        assert!(
            evaluation.errors.is_empty(),
            "{label}: {:?}",
            evaluation.errors
        );
        assert_eq!(
            certification.complete,
            Ok(()),
            "{label}: replay consumes every journal row"
        );
        assert_eq!(
            certification.usage,
            Ok(record.used),
            "{label}: replay uses the phlo of play"
        );
        assert_eq!(
            evaluation.native_phlo_usage, record.phlo,
            "{label}: native phlo usage"
        );
        assert_eq!(
            certification.root,
            Ok(root(&record.post_root)),
            "{label}: replay reaches the post-state root of play"
        );
    }
}

/// Checks the record `warm` of the warm run against the record `child` of a child.
fn assert_same_record(warm: &DeployRecord, child: &DeployRecord) {
    let label = &child.label;
    assert_eq!(warm.label, child.label);
    assert_eq!(warm.pre_root, child.pre_root, "{label}: pre-state root");
    assert_eq!(warm.post_root, child.post_root, "{label}: post-state root");
    assert_eq!(warm.phlo, child.phlo, "{label}: phlo usage");
    assert_eq!(warm.used, child.used, "{label}: budget usage");
    assert_eq!(
        warm.budget_bytes, child.budget_bytes,
        "{label}: evidence of the budget recording"
    );
    assert_eq!(
        warm.journal_bytes, child.journal_bytes,
        "{label}: evidence of the operation journal"
    );
    assert_eq!(warm.log, child.log, "{label}: event log");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_restart_rehydrate_three_processes() {
    if let Ok(role) = std::env::var(ROLE) {
        let directory =
            PathBuf::from(std::env::var_os(DIRECTORY).expect("a child knows its run directory"));
        return match role.as_str() {
            "create" => create(&directory).await,
            "touch" => touch(&directory).await,
            "replay" => replay(&directory).await,
            other => panic!("unknown role {other}"),
        };
    }
    let directory = run_directory();
    spawn("create", &directory).await;
    spawn("touch", &directory).await;
    spawn("replay", &directory).await;
    let created = read_records(&directory, "create");
    let touched = read_records(&directory, "touch");
    assert_eq!(created.len(), STORED_FAMILIES.len());
    assert_eq!(touched.len(), STORED_FAMILIES.len());
    assert_eq!(
        touched.first().map(|record| &record.pre_root),
        created.last().map(|record| &record.post_root),
        "block 2 starts at R1"
    );
    // The warm run: both blocks in one process, over an in-memory store.
    let mut warm = NativeChain::new().await;
    for (family, record) in STORED_FAMILIES.iter().zip(&created) {
        let (_, own) = play_recorded(&mut warm, &family.creation(), &creation_label(family)).await;
        assert_same_record(&own, record);
    }
    for (family, record) in STORED_FAMILIES.iter().zip(&touched) {
        let (_, own) = play_recorded(&mut warm, &family.touching(), &touch_label(family)).await;
        assert_same_record(&own, record);
    }
    drop(warm);
    std::fs::remove_dir_all(&directory).expect("remove the run directory");
}
