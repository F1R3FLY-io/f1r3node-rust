use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;

use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

use super::*;

const CHILD_DIRECTORY: &str = "F1R3_LEDGER_CRASH_DIRECTORY";
const CHILD_STAGE: &str = "F1R3_LEDGER_CRASH_STAGE";
const READY: &str = "F1R3_LEDGER_CRASH_READY";
const STAGES: [&str; 9] = [
    "audit-page",
    "round-before-projection",
    "partial-receipt",
    "marker-before-cursor",
    "effects-page",
    "deletion-before-cursor",
    "compaction-page",
    "migration-before-commit",
    "migration-after-commit",
];

struct OwnedChild {
    child: Child,
    output: Option<std::thread::JoinHandle<()>>,
    ready: mpsc::Receiver<String>,
    recent: Arc<Mutex<std::collections::VecDeque<String>>>,
}

impl OwnedChild {
    fn start(directory: &Path, stage: &str) -> Self {
        let test = format!(
            "{}::process_crash_reopens_only_durable_ledger_progress",
            module_path!()
        );
        let test = test.strip_prefix("block_storage::").unwrap_or(&test);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(CHILD_DIRECTORY, directory)
            .env(CHILD_STAGE, stage)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, ready) = mpsc::channel();
        let recent = Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let captured = recent.clone();
        let output = std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.starts_with(READY) {
                    let _ = send.send(line.clone());
                }
                let mut recent = captured.lock();
                if recent.len() == 16 {
                    recent.pop_front();
                }
                recent.push_back(line.chars().take(1024).collect());
            }
        });
        Self {
            child,
            output: Some(output),
            ready,
            recent,
        }
    }

    fn terminate_at_boundary(&mut self, stage: &str) {
        use std::os::unix::process::ExitStatusExt;

        let boundary = self
            .ready
            .recv_timeout(Duration::from_secs(30))
            .unwrap_or_else(|error| {
                panic!(
                    "child missed {stage}: {error}; stdout: {:?}",
                    self.recent.lock()
                );
            });
        assert_eq!(boundary, format!("{READY}:{stage}"));
        self.child.kill().unwrap();
        assert_eq!(self.child.wait().unwrap().signal(), Some(9));
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(output) = self.output.take() {
            let _ = output.join();
        }
    }
}

fn await_termination(stage: &str) -> ! {
    let mut output = std::io::stdout().lock();
    writeln!(output, "\n{READY}:{stage}").unwrap();
    output.flush().unwrap();
    drop(output);
    let mut byte = [0];
    let _ = std::io::stdin().read(&mut byte);
    panic!("parent did not terminate its child at {stage}");
}

fn open_ledger(directory: &Path) -> FinalizationLedger {
    let mut options = heed::EnvOpenOptions::new();
    options.map_size(10 * 1024 * 1024).max_dbs(1);
    let environment = Arc::new(unsafe { options.open(directory).unwrap() });
    let database = {
        let mut transaction = environment.write_txn().unwrap();
        let database = environment
            .create_database(&mut transaction, Some(FinalizationLedger::STORE_NAME))
            .unwrap();
        transaction.commit().unwrap();
        database
    };
    FinalizationLedger::from_store(Arc::new(LmdbKeyValueStore::new(environment, database)))
}

fn populate(ledger: &FinalizationLedger) {
    let mut head = initialize(ledger, hash(0), 0);
    for revision in 1..=3 {
        let record = prepare_record(
            ledger,
            &head,
            hash(revision),
            i64::from(revision),
            0.75,
            BTreeSet::from([BlockHashSerde(hash(revision))]),
        )
        .unwrap();
        head = match ledger.try_append(&head, &record).unwrap() {
            FinalizationAppendOutcome::Committed(head) => head,
            outcome => panic!("unexpected LMDB append outcome: {outcome:?}"),
        };
    }
}

fn migration_input() -> (RecoveryEpisodeId, Vec<SettledRecoveryCharge>) {
    let episode = recovery_episode(0, 10);
    let charges = (20..24)
        .map(|target| recovery_charge(&episode, target, 30, 1))
        .collect();
    (episode, charges)
}

fn child_run(directory: &Path, stage: &str) -> ! {
    let ledger = open_ledger(directory);
    populate(&ledger);
    if stage == "audit-page" {
        let mut scan = ledger.begin_integrity_scan().unwrap();
        assert!(!scan
            .validate_next_page(NonZeroUsize::new(2).unwrap())
            .unwrap());
        await_termination(stage);
    }
    if stage == "round-before-projection" {
        await_termination(stage);
    }
    if stage.starts_with("migration-") {
        let (episode, charges) = migration_input();
        if stage == "migration-before-commit" {
            await_termination(stage);
        }
        assert_eq!(
            ledger
                .commit_settled_recovery_migration(&episode, &charges, 0)
                .unwrap(),
            4
        );
        await_termination(stage);
    }
    for revision in 1..=3 {
        ledger.record_projection_completed(revision).unwrap();
    }
    if stage == "partial-receipt" {
        ledger
            .record_effect(FinalizationEffectId {
                revision: 1,
                block_hash: BlockHashSerde(hash(1)),
                kind: FinalizationEffectKind::DeployRemoval,
            })
            .unwrap();
        await_termination(stage);
    }
    for revision in 1..=3 {
        receipt_all_effects(&ledger, revision, hash(revision as u8));
        let _ = ledger.commit_round_effects_completed(revision).unwrap();
        if stage == "marker-before-cursor" {
            await_termination(stage);
        }
    }
    let mut advancement = ledger.begin_effects_cursor_advance().unwrap();
    assert!(!advancement
        .advance_next_page(NonZeroUsize::new(1).unwrap())
        .unwrap());
    if stage == "effects-page" {
        await_termination(stage);
    }
    assert_eq!(advancement.finish().unwrap(), 3);
    if stage == "deletion-before-cursor" {
        let observed = FinalizationLedger::from_store(Arc::new(ObservedAuditStore {
            inner: ledger.store.raw_store().clone(),
            on_read: Arc::new(|_| Ok(())),
            on_delete: None,
            on_put: Some(Arc::new(move |pairs| {
                for (key, _) in pairs {
                    if bincode::deserialize::<FinalizationLedgerKey>(key).unwrap()
                        == FinalizationLedgerKey::EffectsCompactionCursor
                    {
                        await_termination("deletion-before-cursor");
                    }
                }
                Ok(())
            })),
            _lifetime: Arc::new(AuditStoreLifetime(None)),
        }));
        observed.reconcile_effect_compaction().unwrap();
        panic!("compaction bypassed its cursor-write boundary");
    }
    assert_eq!(stage, "compaction-page");
    assert!(!ledger
        .begin_effect_compaction()
        .unwrap()
        .delete_next_page(NonZeroUsize::new(5).unwrap())
        .unwrap());
    await_termination(stage);
}

fn verify_reopened(ledger: &FinalizationLedger, stage: &str) {
    ledger.validate_integrity().unwrap();
    assert_eq!(ledger.head().unwrap().unwrap().revision, 3);
    let (projection, effects, compaction) = match stage {
        "audit-page"
        | "round-before-projection"
        | "migration-before-commit"
        | "migration-after-commit" => (0, 0, 0),
        "partial-receipt" | "marker-before-cursor" => (3, 0, 0),
        "effects-page" => (3, 1, 0),
        "deletion-before-cursor" => (3, 3, 0),
        "compaction-page" => (3, 3, 1),
        _ => panic!("unknown crash stage"),
    };
    assert_eq!(ledger.projection_cursor().unwrap(), projection, "{stage}");
    assert_eq!(ledger.effects_cursor().unwrap(), effects, "{stage}");
    assert_eq!(
        ledger.effects_compaction_cursor().unwrap(),
        compaction,
        "{stage}"
    );
    if stage == "audit-page" {
        let mut earlier = ledger.record(2).unwrap().unwrap();
        earlier.block_number += 1;
        ledger
            .store
            .put_one(
                FinalizationLedgerKey::Round(2),
                FinalizationLedgerValue::Round(earlier),
            )
            .unwrap();
        assert!(ledger
            .begin_integrity_scan()
            .unwrap()
            .validate_next_page(NonZeroUsize::new(2).unwrap())
            .is_err());
        return;
    }
    if stage.starts_with("migration-") {
        let (episode, charges) = migration_input();
        let expected = if stage == "migration-before-commit" {
            0
        } else {
            4
        };
        assert_eq!(ledger.settled_recovery_usage(&episode).unwrap(), expected);
        assert_eq!(
            ledger.settled_recovery_state().unwrap().charges.len(),
            expected as usize
        );
        assert_eq!(
            ledger
                .commit_settled_recovery_migration(&episode, &charges, expected)
                .unwrap(),
            4
        );
        let rows = ledger.store.raw_store().to_map().unwrap();
        assert_eq!(
            ledger
                .commit_settled_recovery_migration(&episode, &charges, 4)
                .unwrap(),
            4
        );
        assert_eq!(ledger.store.raw_store().to_map().unwrap(), rows);
        return;
    }
    let first = FinalizationEffectId {
        revision: 1,
        block_hash: BlockHashSerde(hash(1)),
        kind: FinalizationEffectKind::DeployRemoval,
    };
    if stage == "partial-receipt" {
        assert!(ledger.effect_completed(&first).unwrap());
        assert!(!ledger
            .effect_completed(&FinalizationEffectId {
                kind: FinalizationEffectKind::FinalizedEvent,
                ..first.clone()
            })
            .unwrap());
        assert!(!ledger.round_effects_complete(1).unwrap());
    }
    if stage == "marker-before-cursor" {
        assert!(ledger.round_effects_complete(1).unwrap());
    }
    if matches!(stage, "deletion-before-cursor" | "compaction-page") {
        assert_eq!(
            ledger
                .read_value(&FinalizationLedgerKey::Effect(first.clone()))
                .unwrap(),
            None
        );
        assert!(ledger.effect_completed(&first).unwrap());
        assert_eq!(
            ledger
                .read_value(&FinalizationLedgerKey::EffectsComplete(1))
                .unwrap(),
            None
        );
    }
    for revision in 1..=3 {
        if revision > ledger.projection_cursor().unwrap() {
            ledger.record_projection_completed(revision).unwrap();
        }
        receipt_all_effects(ledger, revision, hash(revision as u8));
        ledger.record_round_effects_completed(revision).unwrap();
    }
    assert_eq!(ledger.effects_cursor().unwrap(), 3);
    assert_eq!(ledger.effects_compaction_cursor().unwrap(), 3);
    ledger.validate_integrity().unwrap();
    let rows = ledger.store.raw_store().to_map().unwrap();
    ledger.reconcile_effect_compaction().unwrap();
    assert_eq!(ledger.store.raw_store().to_map().unwrap(), rows);
}

#[test]
fn process_crash_reopens_only_durable_ledger_progress() {
    if let Some(stage) = std::env::var_os(CHILD_STAGE) {
        let stage = stage.to_str().unwrap();
        assert!(STAGES.contains(&stage));
        let directory = std::env::var_os(CHILD_DIRECTORY).unwrap();
        child_run(Path::new(&directory), stage);
    }
    let scratch = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/verification/pr216/ledger-startup");
    std::fs::create_dir_all(&scratch).unwrap();
    for stage in STAGES {
        let directory = tempfile::Builder::new()
            .prefix("ledger-crash-")
            .tempdir_in(&scratch)
            .unwrap();
        let mut child = OwnedChild::start(directory.path(), stage);
        child.terminate_at_boundary(stage);
        drop(child);
        let ledger = open_ledger(directory.path());
        verify_reopened(&ledger, stage);
        drop(ledger);
        directory.close().unwrap();
        println!("PASS LMDB process-crash boundary: {stage}");
    }
}
