use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::RwLock;

use super::durable_publication::directory_snapshot;
use super::durable_recovery::limits;
use super::durable_rspace_authority::{
    Profile, Store, fixture as owner_fixture, poll_checkpoint_with, populate, publication_limits,
};
use super::support::PersistenceTrialDir;
use crate::rspace::hot_store::{DurableHotStoreBootstrapLimits, HotStoreInstances};
use crate::rspace::internal::Datum;
use crate::rspace::persistence::durable::{
    DurableAdmissionErrorV1, DurableAdmissionLimitsV1, DurableDirectoryV1, DurableIoError,
    DurableRSpaceAuthorityV1, DurableStartupErrorV1, DurableViewReadHandleV1, DurableViewReaderV1,
    index_file_name, pack_file_name, recover_read_authority,
};
use crate::rspace::persistence::{
    GenerationLeaseKeyV1, IndexGenerationV1, LogPackDecodeLimits, PackId,
};

const STORE_VARIABLE: &str = "RSPACE_GC_CROSS_PROCESS_STORE";
const READER_TEST: &str =
    "rspace::persistence::tests::durable_gc_cross_process::pinned_reader_child";
const REPORT_MARKER: &str = "rspace-gc-cross-process-report";
const REPORT_TIMEOUT: Duration = Duration::from_secs(120);
const CHANNEL: &str = "channel";
const KEY_FIELDS: usize = 4;
const READ_FIELDS: usize = 8;

#[derive(Debug, PartialEq, Eq)]
struct ReadOutcome {
    summary: String,
    rendered: String,
}

#[derive(Debug, PartialEq, Eq)]
enum OpenAttempt {
    Granted,
    Contended,
}

#[derive(Debug, PartialEq, Eq)]
struct RefusalRecord {
    writer_lock: OpenAttempt,
    store_open: OpenAttempt,
    durable_bytes_unchanged: bool,
}

#[derive(Debug)]
struct CompactionRecord {
    old: GenerationLeaseKeyV1,
    successor: GenerationLeaseKeyV1,
    names_after: BTreeSet<String>,
}

#[derive(Debug)]
struct CrossProcessRecord {
    admitted_key: GenerationLeaseKeyV1,
    admitted_reads: Vec<ReadOutcome>,
    refusals_while_admitted: RefusalRecord,
    lock_after_close: OpenAttempt,
    publication_appended_to_admitted: bool,
    first_compaction: CompactionRecord,
    reader_alive_after_first_compaction: bool,
    admitted_absent_before_read: bool,
    reads_after_reclamation: Vec<ReadOutcome>,
    admitted_absent_after_read: bool,
    reopened_key: GenerationLeaseKeyV1,
    reopened_reads: Vec<ReadOutcome>,
    retained_reads: Vec<ReadOutcome>,
    refusals_while_reopened: RefusalRecord,
    termination_signal: Option<i32>,
    names_after_death: BTreeSet<String>,
    lock_after_death: OpenAttempt,
    second_compaction: CompactionRecord,
}

struct PinnedReads {
    manifest_handle: DurableViewReadHandleV1<Profile>,
    reader: DurableViewReaderV1<Profile>,
}

impl PinnedReads {
    fn pin(owner: &DurableRSpaceAuthorityV1<Profile>) -> Result<Self, String> {
        let core = owner.published_core();
        let (_, manifest_handle) = core
            .read_handle(None)
            .map_err(|error| format!("the published core refused a read handle: {error:?}"))?;
        let (resolved, handle) = core
            .read_handle(None)
            .map_err(|error| format!("the published core refused a read handle: {error:?}"))?;
        Ok(Self {
            manifest_handle,
            reader: DurableViewReaderV1::new(handle, resolved.roots()),
        })
    }

    fn read(&self) -> Vec<String> {
        let channel = CHANNEL.to_owned();
        let group = [channel.clone(), channel.clone()];
        let mut fields = Vec::with_capacity(READ_FIELDS);
        push_read(
            &mut fields,
            self.manifest_handle
                .semantic_roots()
                .map(|roots| vec![roots]),
        );
        push_read(&mut fields, self.reader.try_get_data_canonical(&channel));
        push_read(&mut fields, self.reader.try_get_continuations_canonical(&group));
        push_read(&mut fields, self.reader.try_get_joins_canonical(&channel));
        fields
    }
}

fn push_read<Value: Debug, Failure: Debug>(
    fields: &mut Vec<String>,
    read: Result<Vec<Value>, Failure>,
) {
    fields.push(match &read {
        Ok(values) => format!("ok {}", values.len()),
        Err(_) => "error".to_owned(),
    });
    fields.push(format!("{read:?}"));
}

fn keyed_fields<const GROUPS: usize>(
    key: GenerationLeaseKeyV1,
    groups: [Vec<String>; GROUPS],
) -> Vec<String> {
    let mut fields = Vec::with_capacity(KEY_FIELDS + groups.iter().map(Vec::len).sum::<usize>());
    fields.extend(
        [key.epoch(), key.first_live_pack(), key.active_pack(), key.index_generation()]
            .map(|value| value.to_string()),
    );
    for group in groups {
        fields.extend(group);
    }
    fields
}

fn report(phase: &str, fields: &[String]) -> Result<(), String> {
    let mut line = String::with_capacity(
        REPORT_MARKER.len() +
            phase.len() +
            2 +
            fields.iter().map(|field| field.len() + 1).sum::<usize>(),
    );
    line.push_str(REPORT_MARKER);
    line.push('\t');
    line.push_str(phase);
    for field in fields {
        line.push('\t');
        line.push_str(field);
    }
    line.push('\n');
    let mut output = std::io::stdout().lock();
    output
        .write_all(line.as_bytes())
        .and_then(|()| output.flush())
        .map_err(|error| format!("the {phase} report was not written: {error}"))
}

fn await_command(
    commands: &mut impl Iterator<Item = std::io::Result<String>>,
    expected: &str,
) -> Result<bool, String> {
    match commands.next() {
        None => Ok(false),
        Some(Ok(command)) if command == expected => Ok(true),
        Some(Ok(command)) => {
            Err(format!("the reading process expected {expected:?} and received {command:?}"))
        }
        Some(Err(error)) => Err(format!("the command pipe of the reading process failed: {error}")),
    }
}

fn open_owner(root: &Path) -> Result<(Store, DurableRSpaceAuthorityV1<Profile>), String> {
    let view = recover_read_authority::<Profile>(
        root,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .map_err(|error| format!("the store does not open: {error:?}"))?;
    let hot: Store = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .map_err(|error| format!("the hot store does not load: {error:?}"))?,
    );
    let owner = view
        .into_rspace_authority(&hot)
        .map_err(|failure| format!("the owner does not open: {:?}", failure.1))?;
    Ok((hot, owner))
}

fn serve_pinned_reader(root: &Path) -> Result<(), String> {
    let (hot, mut owner) = open_owner(root)?;
    let admitted = PinnedReads::pin(&owner)?;
    report("admitted", &keyed_fields(owner.generation_key(), [admitted.read()]))?;
    let mut commands = std::io::stdin().lock().lines();
    if !await_command(&mut commands, "close")? {
        return Ok(());
    }
    owner
        .close()
        .map_err(|error| format!("the owner of the reading process does not close: {error:?}"))?;
    drop((owner, hot));
    report("closed", &[])?;
    if !await_command(&mut commands, "read")? {
        return Ok(());
    }
    report("read", &admitted.read())?;
    if !await_command(&mut commands, "reopen")? {
        return Ok(());
    }
    let (reopened_hot, reopened_owner) = open_owner(root)?;
    let reopened = PinnedReads::pin(&reopened_owner)?;
    report(
        "reopened",
        &keyed_fields(reopened_owner.generation_key(), [reopened.read(), admitted.read()]),
    )?;
    commands.for_each(drop);
    drop((reopened, reopened_owner, reopened_hot, admitted));
    Ok(())
}

#[test]
#[ignore = "the reading process that the cross-process reclamation trial starts"]
fn pinned_reader_child() {
    let Some(store) = std::env::var_os(STORE_VARIABLE) else {
        return;
    };
    if let Err(failure) = serve_pinned_reader(Path::new(&store)) {
        let _ = report("failed", &[format!("{failure:?}")]);
    }
}

struct ReaderProcess {
    child: Child,
    commands: Option<ChildStdin>,
    reports: Receiver<String>,
    forwarder: Option<JoinHandle<()>>,
}

impl ReaderProcess {
    fn spawn(root: &Path) -> Result<Self, String> {
        let test_binary = std::env::current_exe()
            .map_err(|error| format!("the test binary cannot be located: {error}"))?;
        let child = Command::new(test_binary)
            .args(["--ignored", "--exact", READER_TEST, "--nocapture"])
            .env(STORE_VARIABLE, root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("the reading process does not start: {error}"))?;
        let (sender, reports) = std::sync::mpsc::channel();
        let mut process = Self {
            child,
            commands: None,
            reports,
            forwarder: None,
        };
        process.commands = process.child.stdin.take();
        let output = process
            .child
            .stdout
            .take()
            .ok_or("the reading process has no output pipe")?;
        let forwarder = std::thread::Builder::new()
            .name("gc-cross-process-reports".to_owned())
            .spawn(move || {
                for line in BufReader::new(output).lines().map_while(Result::ok) {
                    let Some(start) = line.find(REPORT_MARKER) else {
                        continue;
                    };
                    if sender.send(line[start..].to_owned()).is_err() {
                        return;
                    }
                }
            })
            .map_err(|error| format!("the report forwarder does not start: {error}"))?;
        process.forwarder = Some(forwarder);
        Ok(process)
    }

    fn send(&mut self, command: &str) -> Result<(), String> {
        let commands = self
            .commands
            .as_mut()
            .ok_or("the command pipe of the reading process is closed")?;
        writeln!(commands, "{command}")
            .and_then(|()| commands.flush())
            .map_err(|error| format!("the {command} command was not delivered: {error}"))
    }

    fn expect_report(&self, phase: &str) -> Result<Vec<String>, String> {
        let line = match self.reports.recv_timeout(REPORT_TIMEOUT) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!(
                    "the reading process sent no {phase} report within {} seconds",
                    REPORT_TIMEOUT.as_secs()
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!("the reading process ended before its {phase} report"));
            }
        };
        let mut fields = line.split('\t');
        match (fields.next(), fields.next()) {
            (Some(REPORT_MARKER), Some(reported)) if reported == phase => {
                Ok(fields.map(str::to_owned).collect())
            }
            _ => Err(format!("the reading process sent {line:?} instead of its {phase} report")),
        }
    }

    fn is_running(&mut self) -> Result<bool, String> {
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|error| format!("the state of the reading process is unknown: {error}"))
    }

    fn kill(&mut self) -> Result<Option<i32>, String> {
        self.child
            .kill()
            .map_err(|error| format!("the reading process was not killed: {error}"))?;
        self.child
            .wait()
            .map(|status| status.signal())
            .map_err(|error| format!("the killed reading process was not reaped: {error}"))
    }
}

impl Drop for ReaderProcess {
    fn drop(&mut self) {
        drop(self.commands.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(forwarder) = self.forwarder.take() {
            let _ = forwarder.join();
        }
    }
}

fn parse_number(field: &str) -> Result<u64, String> {
    field
        .parse()
        .map_err(|error| format!("the field {field:?} is not a number: {error}"))
}

fn split_key(fields: &[String]) -> Result<(GenerationLeaseKeyV1, &[String]), String> {
    match fields.split_at_checked(KEY_FIELDS) {
        Some(([epoch, first, active, index], rest)) => Ok((
            GenerationLeaseKeyV1::new(
                parse_number(epoch)?,
                parse_number(first)?,
                parse_number(active)?,
                parse_number(index)?,
            ),
            rest,
        )),
        Some(_) | None => Err(format!(
            "a report of {} fields carries no generation key of {KEY_FIELDS} fields",
            fields.len()
        )),
    }
}

fn parse_reads(fields: &[String]) -> Result<Vec<ReadOutcome>, String> {
    match fields.len() {
        READ_FIELDS => Ok(fields
            .chunks_exact(2)
            .filter_map(|pair| match pair {
                [summary, rendered] => Some(ReadOutcome {
                    summary: summary.clone(),
                    rendered: rendered.clone(),
                }),
                _ => None,
            })
            .collect()),
        length => Err(format!("the reads of a handle are {READ_FIELDS} fields, not {length}")),
    }
}

fn summaries(reads: &[ReadOutcome]) -> Vec<&str> {
    reads.iter().map(|read| read.summary.as_str()).collect()
}

fn physical(key: GenerationLeaseKeyV1) -> [u64; 3] {
    [key.first_live_pack(), key.active_pack(), key.index_generation()]
}

fn generation_file_names(key: GenerationLeaseKeyV1) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    for pack in key.first_live_pack()..=key.active_pack() {
        let pack =
            PackId::new(pack).map_err(|error| format!("invalid live pack {pack}: {error:?}"))?;
        names.insert(pack_file_name(pack));
    }
    let index = IndexGenerationV1::new(key.index_generation())
        .map_err(|error| format!("invalid index generation: {error:?}"))?;
    names.insert(index_file_name(index));
    Ok(names)
}

fn store_names(key: GenerationLeaseKeyV1) -> BTreeSet<String> {
    let mut names =
        generation_file_names(key).expect("a recorded generation key names valid files");
    names.insert("catalog.v1".to_owned());
    names.insert("writer.lock.v1".to_owned());
    names
}

fn directory_names(root: &Path) -> Result<BTreeSet<String>, String> {
    fs::read_dir(root)
        .map_err(|error| format!("the store directory does not list: {error}"))?
        .map(|entry| {
            entry
                .map_err(|error| format!("a store directory entry does not read: {error}"))?
                .file_name()
                .into_string()
                .map_err(|name| format!("the store file name {name:?} is not UTF-8"))
        })
        .collect()
}

fn file_length(root: &Path, name: &str) -> Result<u64, String> {
    fs::metadata(root.join(name))
        .map(|metadata| metadata.len())
        .map_err(|error| format!("{name} has no metadata: {error}"))
}

fn appendable_lengths(root: &Path, key: GenerationLeaseKeyV1) -> Result<[u64; 2], String> {
    let pack = PackId::new(key.active_pack())
        .map_err(|error| format!("invalid active pack: {error:?}"))?;
    let index = IndexGenerationV1::new(key.index_generation())
        .map_err(|error| format!("invalid index generation: {error:?}"))?;
    Ok([file_length(root, &pack_file_name(pack))?, file_length(root, &index_file_name(index))?])
}

fn attempt_writer_lock(root: &Path) -> Result<OpenAttempt, String> {
    match DurableDirectoryV1::open(root).and_then(DurableDirectoryV1::try_acquire_writer) {
        Ok(_) => Ok(OpenAttempt::Granted),
        Err(DurableIoError::WriterLockContended { .. }) => Ok(OpenAttempt::Contended),
        Err(error) => Err(format!("the writer lock attempt failed: {error:?}")),
    }
}

fn attempt_store_open(root: &Path) -> Result<OpenAttempt, String> {
    match recover_read_authority::<Profile>(
        root,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    ) {
        Ok(_) => Ok(OpenAttempt::Granted),
        Err(DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Io(
            DurableIoError::WriterLockContended { .. },
        ))) => Ok(OpenAttempt::Contended),
        Err(error) => Err(format!("the store open attempt failed: {error:?}")),
    }
}

fn attempt_opens(root: &Path) -> Result<RefusalRecord, String> {
    let before = directory_snapshot(root);
    let writer_lock = attempt_writer_lock(root)?;
    let store_open = attempt_store_open(root)?;
    Ok(RefusalRecord {
        writer_lock,
        store_open,
        durable_bytes_unchanged: directory_snapshot(root) == before,
    })
}

fn prepare_store() -> Result<PersistenceTrialDir, String> {
    let (scratch, hot, gate, mut owner) = owner_fixture();
    populate(&hot);
    poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .map_err(|error| format!("the setup checkpoint was refused: {error:?}"))?;
    owner
        .close()
        .map_err(|error| format!("the setup owner does not close: {error:?}"))?;
    drop((owner, gate, hot));
    Ok(scratch)
}

fn publish_datum(root: &Path, value: &str) -> Result<(), String> {
    let (hot, mut owner) = open_owner(root)?;
    let gate = Arc::new(RwLock::new(()));
    let channel = CHANNEL.to_owned();
    hot.put_datum(&channel, Datum::create(&channel, value.to_owned(), false));
    poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .map_err(|error| format!("the publication was refused: {error:?}"))?;
    owner
        .close()
        .map_err(|error| format!("the publishing owner does not close: {error:?}"))
}

fn compact_store(root: &Path) -> Result<CompactionRecord, String> {
    let writer = DurableDirectoryV1::open(root)
        .map_err(|error| format!("the store directory does not open: {error:?}"))?
        .recover_and_admit_v1::<Profile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| format!("the store was not admitted for compaction: {error:?}"))?;
    let old = GenerationLeaseKeyV1::from_catalog(writer.catalog());
    let compacted = writer
        .compact_generation_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| format!("the compaction was refused: {error:?}"))?;
    let successor = GenerationLeaseKeyV1::from_catalog(compacted.catalog());
    let names_after = directory_names(root)?;
    drop(compacted);
    Ok(CompactionRecord {
        old,
        successor,
        names_after,
    })
}

fn run_cross_process_trial() -> Result<CrossProcessRecord, String> {
    let scratch = prepare_store()?;
    let root = scratch.path();
    let mut reader = ReaderProcess::spawn(root)?;
    let admitted = reader.expect_report("admitted")?;
    let (admitted_key, admitted_fields) = split_key(&admitted)?;
    let admitted_reads = parse_reads(admitted_fields)?;
    let refusals_while_admitted = attempt_opens(root)?;
    reader.send("close")?;
    reader.expect_report("closed")?;
    let lock_after_close = attempt_writer_lock(root)?;
    let admitted_names = generation_file_names(admitted_key)?;
    let lengths_before_publication = appendable_lengths(root, admitted_key)?;
    publish_datum(root, "successor")?;
    let lengths_after_publication = appendable_lengths(root, admitted_key)?;
    let first_compaction = compact_store(root)?;
    let reader_alive_after_first_compaction = reader.is_running()?;
    let admitted_absent_before_read = directory_names(root)?.is_disjoint(&admitted_names);
    reader.send("read")?;
    let reads_after_reclamation = parse_reads(&reader.expect_report("read")?)?;
    let admitted_absent_after_read = directory_names(root)?.is_disjoint(&admitted_names);
    reader.send("reopen")?;
    let reopened = reader.expect_report("reopened")?;
    let (reopened_key, reopened_fields) = split_key(&reopened)?;
    let (successor_fields, retained_fields) = reopened_fields
        .split_at_checked(READ_FIELDS)
        .ok_or("the reopened report carries no reads of the reopened handles")?;
    let reopened_reads = parse_reads(successor_fields)?;
    let retained_reads = parse_reads(retained_fields)?;
    let refusals_while_reopened = attempt_opens(root)?;
    let termination_signal = reader.kill()?;
    let names_after_death = directory_names(root)?;
    let lock_after_death = attempt_writer_lock(root)?;
    let second_compaction = compact_store(root)?;
    drop(reader);
    Ok(CrossProcessRecord {
        admitted_key,
        admitted_reads,
        refusals_while_admitted,
        lock_after_close,
        publication_appended_to_admitted: lengths_after_publication
            .iter()
            .zip(&lengths_before_publication)
            .all(|(after, before)| after > before),
        first_compaction,
        reader_alive_after_first_compaction,
        admitted_absent_before_read,
        reads_after_reclamation,
        admitted_absent_after_read,
        reopened_key,
        reopened_reads,
        retained_reads,
        refusals_while_reopened,
        termination_signal,
        names_after_death,
        lock_after_death,
        second_compaction,
    })
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    match (panic.downcast_ref::<String>(), panic.downcast_ref::<&str>()) {
        (Some(message), _) => format!("the cross-process trial panicked: {message}"),
        (None, Some(message)) => format!("the cross-process trial panicked: {message}"),
        (None, None) => "the cross-process trial panicked".to_owned(),
    }
}

fn trial_record() -> &'static CrossProcessRecord {
    static RECORD: OnceLock<Result<CrossProcessRecord, String>> = OnceLock::new();
    RECORD
        .get_or_init(|| {
            std::panic::catch_unwind(run_cross_process_trial)
                .unwrap_or_else(|panic| Err(panic_message(&*panic)))
        })
        .as_ref()
        .expect("the cross-process trial completes")
}

#[test]
fn a_pinned_read_handle_returns_its_admitted_bytes_after_another_process_reclaims_its_packs() {
    let record = trial_record();
    assert_eq!(
        (summaries(&record.admitted_reads), summaries(&record.reopened_reads)),
        (Vec::from(["ok 1", "ok 1", "ok 1", "ok 1"]), Vec::from(["ok 1", "ok 2", "ok 1", "ok 1"])),
        "GC-16: process A's handle must read the manifest, the datum, the continuation and the \
         join it was admitted with, and a handle on process B's successor must read B's second \
         datum, so that the admitted state is not the current one"
    );
    assert_eq!(
        (
            physical(record.first_compaction.old) == physical(record.admitted_key),
            record.first_compaction.old.epoch() > record.admitted_key.epoch(),
            record.publication_appended_to_admitted,
            record.admitted_absent_before_read,
            record.admitted_absent_after_read,
        ),
        (true, true, true, true, true),
        "GC-16: process B must append to the pack and the index that A's handle was admitted on \
         and then reclaim them, and their names must stay absent while A reads"
    );
    assert_eq!(
        record.reads_after_reclamation, record.admitted_reads,
        "GC-16: every read by process A after process B's reclamation must return the admitted \
         bytes"
    );
    assert_eq!(
        record.retained_reads, record.admitted_reads,
        "GC-16: the reads of process A's admitted handle after A reopened the store at process \
         B's successor must return the admitted bytes"
    );
}

#[test]
fn another_process_reclaims_unblocked_by_a_live_or_killed_reader_and_leaves_nothing_stale() {
    let record = trial_record();
    let first = &record.first_compaction;
    let second = &record.second_compaction;
    assert_eq!(
        (
            &record.lock_after_close,
            physical(first.old) == physical(record.admitted_key),
            record.reader_alive_after_first_compaction,
        ),
        (&OpenAttempt::Granted, true, true),
        "GC-17: once process A has closed its owner, A's handles must not refuse process B's \
         writer lock, and B must compact A's generation while A is alive"
    );
    assert_eq!(
        first.names_after,
        store_names(first.successor),
        "GC-17: process B's compaction must reclaim A's generation within the call while A is \
         alive, leaving no old file and no reclaim.v1"
    );
    assert_eq!(
        (record.termination_signal, &record.lock_after_death, second.old),
        (Some(libc::SIGKILL), &OpenAttempt::Granted, record.reopened_key),
        "GC-17: process A must end by SIGKILL while it holds its owner and handles on the \
         generation that process B compacts next, and B's first writer lock attempt after the \
         death must be granted"
    );
    assert_eq!(
        record.names_after_death,
        store_names(first.successor),
        "GC-17: the death of process A must leave no file or record beyond the catalog, the lock \
         file and the current generation"
    );
    assert_eq!(
        second.names_after,
        store_names(second.successor),
        "GC-17: process B's compaction after A's death must reclaim the generation that A's \
         handles held within the call, leaving no old file and no reclaim.v1"
    );
}

#[test]
fn an_owner_open_in_another_process_refuses_the_writer_lock_as_contended() {
    let record = trial_record();
    let refused = RefusalRecord {
        writer_lock: OpenAttempt::Contended,
        store_open: OpenAttempt::Contended,
        durable_bytes_unchanged: true,
    };
    assert_eq!(
        (
            &record.refusals_while_admitted,
            &record.refusals_while_reopened,
            &record.lock_after_close
        ),
        (&refused, &refused, &OpenAttempt::Granted),
        "GC-18: while process A's owner is open, process B's writer lock and B's open of the \
         store must be refused with WriterLockContended and change no durable byte, and the lock \
         must be granted once A closes its owner"
    );
}
