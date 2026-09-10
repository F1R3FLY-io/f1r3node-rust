use super::*;
use crate::allocation_probe::{interval, markers, measure, report};

struct ProbeLedger {
    ledger: FinalizationLedger,
    _directory: Option<tempfile::TempDir>,
}

fn probe_ledger(backend: &str, rounds: u8) -> ProbeLedger {
    let ledger = ledger_with_committed_rounds(rounds);
    if backend == "memory" {
        return ProbeLedger {
            ledger,
            _directory: None,
        };
    }
    assert_eq!(backend, "lmdb");
    let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/verification/pr216/ledger-startup");
    std::fs::create_dir_all(&scratch).unwrap();
    let directory = tempfile::Builder::new()
        .prefix("resource-lmdb-")
        .tempdir_in(scratch)
        .unwrap();
    let mut options = heed::EnvOpenOptions::new();
    options.map_size(256 * 1024 * 1024).max_dbs(1);
    let environment = Arc::new(unsafe { options.open(directory.path()).unwrap() });
    let database = {
        let mut writer = environment.write_txn().unwrap();
        let database = environment
            .create_database(&mut writer, Some(FinalizationLedger::STORE_NAME))
            .unwrap();
        writer.commit().unwrap();
        database
    };
    let store = Arc::new(
        shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore::new(environment, database),
    );
    store
        .put(
            ledger
                .store
                .raw_store()
                .to_map()
                .unwrap()
                .into_iter()
                .collect(),
        )
        .unwrap();
    ProbeLedger {
        ledger: FinalizationLedger::from_store(store),
        _directory: Some(directory),
    }
}

fn report_backend(
    backend: &str,
    phase: &str,
    items: usize,
    bytes: usize,
    sample: crate::allocation_probe::Sample,
) {
    report(&format!("{backend}_{phase}"), items, bytes, sample);
}

fn wide_hash(index: u64) -> BlockHash {
    let mut bytes = vec![0; 32];
    bytes[0] = 0xa5;
    bytes[1..9].copy_from_slice(&index.to_be_bytes());
    Bytes::from(bytes)
}

fn add_uncommitted_witnesses(ledger: &FinalizationLedger, count: u8, manifest: usize) -> usize {
    let head = ledger.head().unwrap().unwrap();
    let genesis = ledger.genesis_anchor().unwrap().unwrap();
    let mut last_size = 0;
    for index in 0..count {
        let target = hash(index + 2);
        let mut supporting = (0..manifest)
            .map(|entry| BlockHashSerde(wide_hash(entry as u64)))
            .collect::<BTreeSet<_>>();
        supporting.insert(head.block_hash.clone());
        supporting.insert(BlockHashSerde(target.clone()));
        let witness = FinalizationLedger::prepare_witness(
            6,
            "root".to_string(),
            genesis.block_hash.0.clone(),
            &head,
            target.clone(),
            head.certificate_digest.0.clone(),
            head.block_hash.0.clone(),
            2,
            hash(240),
            1,
            1,
            BTreeMap::from([(
                ValidatorSerde(Bytes::from(vec![7; models::rust::validator::LENGTH])),
                BlockHashSerde(target.clone()),
            )]),
            supporting,
            BlockHashSerde(hash(99)),
            BTreeSet::from([BlockHashSerde(target)]),
        )
        .unwrap();
        last_size = ledger
            .store
            .encode_value(&FinalizationLedgerValue::Witness(witness.clone()))
            .unwrap()
            .len();
        ledger.persist_witness(&head, &witness).unwrap();
    }
    last_size
}

#[test]
fn resource_probe_borrowed_enumeration_does_not_copy_unselected_witness_payloads() {
    for backend in ["memory", "lmdb"] {
        for manifest in [256, 4096] {
            let mut baseline = None;
            for count in [0, 8, 64] {
                let fixture = probe_ledger(backend, 1);
                let ledger = &fixture.ledger;
                let row_bytes = add_uncommitted_witnesses(ledger, count, manifest);
                drop(ledger.settled_recovery_state().unwrap());
                let (state, sample) = measure(|| ledger.settled_recovery_state().unwrap());
                assert!(state.charges.is_empty());
                assert!(state.usage.is_empty());
                assert_eq!(sample.live_delta, 0);
                if let Some(expected) = baseline {
                    assert_eq!(sample.peak_delta, expected);
                } else {
                    baseline = Some(sample.peak_delta);
                }
                if count != 0 {
                    assert!(sample.max_request_bytes < row_bytes);
                }
                report_backend(
                    backend,
                    "borrowed_enumeration",
                    usize::from(count),
                    row_bytes,
                    sample,
                );
            }
        }
    }
}

#[test]
fn resource_probe_recovery_retention_and_migration_are_separate_phases() {
    for backend in ["memory", "lmdb"] {
        for count in [0, 16, 256, 600] {
            let fixture = probe_ledger(backend, 1);
            let ledger = &fixture.ledger;
            let episode = recovery_episode(1, 1);
            let charges = (0..count)
                .map(|index| {
                    let mut charge = recovery_charge(&episode, 0, 7, 0);
                    charge.target_block_hash = BlockHashSerde(wide_hash(index as u64));
                    charge
                })
                .collect::<Vec<_>>();
            let (committed, total) = measure(|| {
                ledger
                    .commit_settled_recovery_migration(&episode, &charges, 0)
                    .unwrap()
            });
            assert_eq!(committed, count as u64);
            let cuts = markers();
            report_backend(backend, "migration_total", count, 0, total);
            if count > 0 {
                let prepared = cuts[0].unwrap();
                let published = cuts[1].unwrap();
                report_backend(backend, "migration_preparation", count, 0, prepared);
                report_backend(
                    backend,
                    "migration_publication",
                    count,
                    0,
                    interval(prepared, published),
                );
                assert!(prepared.live_delta > 0);
                assert!(published.allocated_bytes >= prepared.allocated_bytes);
            } else {
                assert!(cuts.iter().all(Option::is_none));
            }
            drop(ledger.settled_recovery_state().unwrap());
            let (state, retained) = measure(|| ledger.settled_recovery_state().unwrap());
            assert_eq!(state.charges.len(), count);
            assert_eq!(state.usage.values().sum::<u64>(), count as u64);
            if count > 0 {
                assert!(retained.live_delta > 0);
            }
            report_backend(backend, "recovery_retention", count, 0, retained);
            let (_, released) = measure(|| drop(state));
            assert_eq!(released.live_delta, -retained.live_delta);
        }
    }
}

#[test]
fn resource_probe_integrity_working_memory_does_not_retain_the_history() {
    for backend in ["memory", "lmdb"] {
        let mut baseline = None;
        for rounds in [2, 8, 32, 33, 34, 64, 65, 66, 96, 128] {
            let fixture = probe_ledger(backend, rounds);
            let ledger = &fixture.ledger;
            ledger.validate_integrity().unwrap();
            let (head, head_sample) = measure(|| {
                let record = ledger.record(1).unwrap().unwrap();
                FinalizationLedger::record_head(&record)
            });
            drop(head);
            let (_, sample) = measure(|| ledger.validate_integrity().unwrap());
            assert_eq!(sample.live_delta, 0);
            if let Some(expected) = baseline {
                let retained_page_head =
                    if usize::from(rounds) > FinalizationLedger::INTEGRITY_PAGE_RECORDS.get() + 1 {
                        head_sample.live_delta
                    } else {
                        0
                    };
                assert_eq!(sample.peak_delta, expected + retained_page_head);
            } else {
                baseline = Some(sample.peak_delta);
            }
            report_backend(backend, "integrity_audit", usize::from(rounds), 0, sample);
        }
    }
}
