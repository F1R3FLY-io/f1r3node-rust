use std::cell::Cell;
use std::io::{BufRead, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use super::*;

const CRASH_POINT: &str = "F1R3_LMDB_TRANSACTION_CRASH_POINT";
const CRASH_DIRECTORY: &str = "F1R3_LMDB_TRANSACTION_CRASH_DIRECTORY";
const READY: &str = "F1R3_LMDB_TRANSACTION_READY";

thread_local! {
    static MUTATION_INDEX: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn checkpoint(kind: &str) {
    let Ok(requested) = std::env::var(CRASH_POINT) else {
        return;
    };
    let point = if kind == "mutation" {
        MUTATION_INDEX.with(|index| {
            index.set(index.get() + 1);
            format!("mutation-{}", index.get())
        })
    } else {
        kind.to_string()
    };
    if point != requested {
        return;
    }
    let mut output = std::io::stdout().lock();
    writeln!(output, "\n{READY}:{point}").unwrap();
    output.flush().unwrap();
    drop(output);
    let mut byte = [0];
    let _ = std::io::stdin().read(&mut byte);
    panic!("parent did not stop its transaction child at {point}");
}

struct TransactionChild {
    child: Child,
    reader: Option<std::thread::JoinHandle<()>>,
    ready: mpsc::Receiver<String>,
}

impl TransactionChild {
    fn start(directory: &Path, point: &str) -> Self {
        let test = format!(
            "{}::process_crash_preserves_atomic_cross_store_transactions",
            module_path!()
        );
        let test = test.strip_prefix("shared::").unwrap_or(&test);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(CRASH_DIRECTORY, directory)
            .env(CRASH_POINT, point)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, ready) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.starts_with(READY) {
                    let _ = send.send(line);
                }
            }
        });
        Self {
            child,
            reader: Some(reader),
            ready,
        }
    }

    fn stop_at(&mut self, point: &str) {
        assert_eq!(
            self.ready.recv_timeout(Duration::from_secs(30)).unwrap(),
            format!("{READY}:{point}")
        );
        self.child.kill().unwrap();
        assert_eq!(self.child.wait().unwrap().signal(), Some(9));
    }
}

impl Drop for TransactionChild {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn open_stores(directory: &Path) -> [LmdbKeyValueStore; 2] {
    let mut options = heed::EnvOpenOptions::new();
    options.map_size(10 * 1024 * 1024).max_dbs(2);
    let environment = Arc::new(unsafe { options.open(directory).unwrap() });
    let databases = {
        let mut writer = environment.write_txn().unwrap();
        let first = environment
            .create_database(&mut writer, Some("first"))
            .unwrap();
        let second = environment
            .create_database(&mut writer, Some("second"))
            .unwrap();
        writer.commit().unwrap();
        [first, second]
    };
    databases.map(|database| LmdbKeyValueStore::new(environment.clone(), database))
}

fn mutations(stores: &[LmdbKeyValueStore; 2]) -> Vec<AtomicStoreMutation<'_>> {
    vec![
        AtomicStoreMutation {
            store: &stores[0],
            key: vec![1],
            operation: AtomicStoreOperation::Put(vec![11]),
        },
        AtomicStoreMutation {
            store: &stores[1],
            key: vec![1],
            operation: AtomicStoreOperation::Put(vec![22]),
        },
        AtomicStoreMutation {
            store: &stores[0],
            key: vec![0],
            operation: AtomicStoreOperation::CompareAndSwap {
                expected: Some(vec![0]),
                replacement: Some(vec![1]),
            },
        },
        AtomicStoreMutation {
            store: &stores[1],
            key: vec![2],
            operation: AtomicStoreOperation::Delete,
        },
        AtomicStoreMutation {
            store: &stores[1],
            key: vec![3],
            operation: AtomicStoreOperation::PutIfAbsentOrEqual(vec![4]),
        },
        AtomicStoreMutation {
            store: &stores[0],
            key: vec![1],
            operation: AtomicStoreOperation::PutIfAbsentOrEqual(vec![11]),
        },
        AtomicStoreMutation {
            store: &stores[1],
            key: vec![3],
            operation: AtomicStoreOperation::CompareAndSwap {
                expected: Some(vec![4]),
                replacement: None,
            },
        },
    ]
}

#[test]
fn process_crash_preserves_atomic_cross_store_transactions() {
    if let Ok(point) = std::env::var(CRASH_POINT) {
        let directory = std::env::var_os(CRASH_DIRECTORY).unwrap();
        let stores = open_stores(Path::new(&directory));
        stores[0].strict_atomic_mutate(&mutations(&stores)).unwrap();
        panic!("transaction child passed its requested crash point {point}");
    }
    let scratch = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/shared-test-scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let points = (1..=7)
        .map(|index| format!("mutation-{index}"))
        .chain(["before-commit".to_string(), "after-commit".to_string()]);
    for point in points {
        let directory = tempfile::Builder::new()
            .prefix("lmdb-transaction-crash-")
            .tempdir_in(&scratch)
            .unwrap();
        let stores = open_stores(directory.path());
        stores[0]
            .put(vec![(vec![0], vec![0]), (vec![9], vec![7])])
            .unwrap();
        stores[1]
            .put(vec![(vec![2], vec![9]), (vec![3], vec![4])])
            .unwrap();
        let before = stores.each_ref().map(|store| store.to_map().unwrap());
        drop(stores);
        let mut child = TransactionChild::start(directory.path(), &point);
        child.stop_at(&point);
        drop(child);

        let stores = open_stores(directory.path());
        let expected = [
            BTreeMap::from([(vec![0], vec![1]), (vec![1], vec![11]), (vec![9], vec![7])]),
            BTreeMap::from([(vec![1], vec![22])]),
        ];
        let actual = stores.each_ref().map(|store| store.to_map().unwrap());
        assert_eq!(
            actual,
            if point == "after-commit" {
                expected.clone()
            } else {
                before
            }
        );
        let retry = stores[0].strict_atomic_mutate(&mutations(&stores));
        if point == "after-commit" {
            assert!(matches!(retry, Err(KvStoreError::TransactionConflict(_))));
        } else {
            retry.unwrap();
        }
        assert_eq!(
            stores.each_ref().map(|store| store.to_map().unwrap()),
            expected
        );
        drop(stores);
        directory.close().unwrap();
        println!("PASS LMDB transaction crash boundary: {point}");
    }
}
