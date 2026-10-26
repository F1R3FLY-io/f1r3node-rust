//! The child processes of the P9 suites (P9 design v2, section 3, amendment A13).
//!
//! A test binary starts itself again with `current_exe()` and a role variable, and each child
//! runs one role in a fresh process. A child keeps its store in a run directory under
//! `RRS_SCRATCH`, or under `target/verification/reified-rspaces/scratch`, and never in the
//! RAM-backed temporary directory of the host. Each child writes one record for each deploy, and
//! the parent compares the records of its children with its own.
//!
//! The child of a test runs the same test function, so it has the tokio worker count of the
//! parent (finding F-47-2: the session charge of native execution depends on the thread count).
//! The parent keeps the standard output of each child, so no test-harness line of a child
//! reaches the output of the parent. It shows the standard error of a child that fails.
//!
//! The P7 restart suite keeps its own copy of these helpers (`reified_restart_rehydrate.rs`).

use std::path::{Path, PathBuf};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::native::{
    decode_evidence, decode_log, encode_log, evidence_bytes, finish_injected,
    replay_environment_from, NativeChain, Played,
};
use super::session::Injections;

/// The variable that names the scratch root of the verification jobs.
const SCRATCH: &str = "RRS_SCRATCH";

/// The scratch root: `RRS_SCRATCH`, or the scratch directory of the verification scripts.
pub fn scratch_root() -> PathBuf {
    match std::env::var_os(SCRATCH) {
        Some(root) => PathBuf::from(root),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/verification/reified-rspaces/scratch"),
    }
}

/// A fresh run directory `p9-{name}-{pid}` under the scratch root.
pub fn run_directory(name: &str) -> PathBuf {
    let directory = scratch_root().join(format!("p9-{name}-{}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).expect("remove an old run directory");
    }
    std::fs::create_dir_all(&directory).expect("create the run directory");
    directory
}

/// The variables that tell a child its role and its run directory.
pub struct Child<'a> {
    /// The name of the test function that the child runs.
    pub test: &'a str,
    /// The variable of the role.
    pub role_variable: &'a str,
    /// The variable of the run directory.
    pub directory_variable: &'a str,
}

impl Child<'_> {
    /// The role of this process, with its run directory, when it is a child.
    pub fn role(&self) -> Option<(String, PathBuf)> {
        let role = std::env::var(self.role_variable).ok()?;
        let directory = std::env::var_os(self.directory_variable)
            .map(PathBuf::from)
            .expect("a child knows its run directory");
        Some((role, directory))
    }

    /// Runs the role `role` in a child process over `directory`, and checks that it succeeds.
    pub async fn spawn(&self, role: &str, directory: &Path) {
        let output = tokio::process::Command::new(
            std::env::current_exe().expect("the path of the test binary"),
        )
        .args([self.test, "--exact", "--nocapture", "--test-threads", "1"])
        .env(self.role_variable, role)
        .env(self.directory_variable, directory)
        .output()
        .await
        .expect("run the child process");
        assert!(
            output.status.success(),
            "the {role} child failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// The record of one deploy that a process writes for the parent or for the next process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployRecord {
    /// The label of the deploy, which seeds its random state.
    pub label: String,
    /// The term of the deploy.
    pub term: String,
    /// The root of its pre-state.
    pub pre_root: Vec<u8>,
    /// The root of its post-state.
    pub post_root: Vec<u8>,
    /// The values on the public output channel after the deploy, as Debug forms.
    pub outputs: Vec<String>,
    /// The phlo usage of native play.
    pub phlo: Option<u64>,
    /// The usage of its budget recording.
    pub used: u64,
    /// The evidence bytes of the budget recording.
    pub budget_bytes: Vec<u8>,
    /// The evidence bytes of the operation journal.
    pub journal_bytes: Vec<u8>,
    /// The bytes of the event log (`encode_log`).
    pub log: Vec<u8>,
}

/// The fields of a [`DeployRecord`] in its order, as bincode encodes them.
type RecordFields = (
    String,
    String,
    Vec<u8>,
    Vec<u8>,
    Vec<String>,
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
            self.term.clone(),
            self.pre_root.clone(),
            self.post_root.clone(),
            self.outputs.clone(),
            self.phlo,
            self.used,
            self.budget_bytes.clone(),
            self.journal_bytes.clone(),
            self.log.clone(),
        )
    }

    fn of_fields(fields: RecordFields) -> Self {
        let (
            label,
            term,
            pre_root,
            post_root,
            outputs,
            phlo,
            used,
            budget_bytes,
            journal_bytes,
            log,
        ) = fields;
        Self {
            label,
            term,
            pre_root,
            post_root,
            outputs,
            phlo,
            used,
            budget_bytes,
            journal_bytes,
            log,
        }
    }

    /// The root of the post-state.
    pub fn post(&self) -> Blake2b256Hash { Blake2b256Hash::from_bytes(self.post_root.clone()) }

    /// The root of the pre-state.
    pub fn pre(&self) -> Blake2b256Hash { Blake2b256Hash::from_bytes(self.pre_root.clone()) }
}

/// Writes `records` to the file `records-{name}` of `directory`.
pub fn write_records(directory: &Path, name: &str, records: &[DeployRecord]) {
    let fields: Vec<RecordFields> = records.iter().map(DeployRecord::fields).collect();
    std::fs::write(
        directory.join(format!("records-{name}")),
        bincode::serialize(&fields).expect("serialize the records"),
    )
    .expect("write the records");
}

/// Reads the records that [`write_records`] wrote.
pub fn read_records(directory: &Path, name: &str) -> Vec<DeployRecord> {
    let bytes = std::fs::read(directory.join(format!("records-{name}"))).expect("read the records");
    let fields: Vec<RecordFields> = bincode::deserialize(&bytes).expect("decode the records");
    fields.into_iter().map(DeployRecord::of_fields).collect()
}

/// Plays `term` on `chain` with the random state of `label` and the phlo limit `limit`, checks
/// that it has no error and that the sweep leaves no volatile channel, and returns the play and
/// its record. The play is not certified: a validator certifies each deploy once (amendment
/// A16).
pub async fn play_recorded(
    chain: &mut NativeChain,
    term: &str,
    label: &str,
    limit: u64,
) -> (Played, DeployRecord) {
    let pre_root = chain.history().root();
    let played = chain.play_with(term, label, limit, Injections::new()).await;
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
    let record = record_of(&played, term, label, &pre_root);
    (played, record)
}

/// The record of the deploy `term` with the label `label`, played as `played` from `pre_root`.
fn record_of(played: &Played, term: &str, label: &str, pre_root: &Blake2b256Hash) -> DeployRecord {
    let (budget_bytes, journal_bytes) = evidence_bytes(played);
    DeployRecord {
        label: label.to_string(),
        term: term.to_string(),
        pre_root: pre_root.bytes(),
        post_root: played.root.bytes(),
        outputs: played.outputs.clone(),
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
    }
}

/// Certifies the deploy of `record` from its pre-state in the store of `chain`, as a validator
/// does: it decodes the committed evidence, runs the journal check, the trace binding, native
/// replay, the completeness check and the export. The certification must reach the post-state
/// root of play with the phlo of play.
pub async fn certify_record(chain: &mut NativeChain, record: &DeployRecord, limit: u64) {
    let label = &record.label;
    chain.reset(&record.pre()).await;
    let (recording, journal) = decode_evidence(&record.budget_bytes, &record.journal_bytes);
    assert_eq!(
        recording.used, record.used,
        "{label}: the evidence keeps the usage"
    );
    let environment = replay_environment_from(
        chain.history(),
        decode_log(&record.log),
        limit,
        &recording,
        journal,
    )
    .await
    .unwrap_or_else(|error| panic!("{label}: the committed evidence checks: {error}"));
    let certification = finish_injected(
        environment,
        &record.term,
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
        "{label}: native replay fails: {:?}",
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
        certification.root.as_ref(),
        Ok(&record.post()),
        "{label}: replay reaches the post-state root of play"
    );
}

/// Checks that two runs give the same record of one deploy: the roots, the outputs, the cost
/// vector, the evidence bytes and the event log.
pub fn assert_same_record(own: &DeployRecord, other: &DeployRecord) {
    assert_eq!(own, other, "{}: the records of two runs differ", own.label);
}
