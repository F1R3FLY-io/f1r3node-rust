use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

use super::*;
use crate::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;

struct AuditManager {
    inner: InMemoryStoreManager,
    ledger: Arc<dyn KeyValueStore>,
}

impl AuditManager {
    async fn new(ledger: Arc<dyn KeyValueStore>) -> Self {
        let mut inner = InMemoryStoreManager::new();
        drop(BlockDagKeyValueStorage::new(&mut inner).await.unwrap());
        Self { inner, ledger }
    }
}

#[async_trait]
impl KeyValueStoreManager for AuditManager {
    async fn store(&mut self, name: String) -> Result<Arc<dyn KeyValueStore>, heed::Error> {
        if name == FinalizationLedger::STORE_NAME {
            Ok(self.ledger.clone())
        } else {
            self.inner.store(name).await
        }
    }

    async fn shutdown(&mut self) -> Result<(), heed::Error> { self.inner.shutdown().await }
}

fn witness_key(ledger: &FinalizationLedger, revision: u64) -> Vec<u8> {
    let record = ledger.record(revision).unwrap().unwrap();
    ledger
        .store
        .encode_key(&FinalizationLedgerKey::Witness(record.witness_digest))
        .unwrap()
}

fn count_witnesses(keys: &[Vec<u8>], count: &AtomicUsize) {
    for key in keys {
        if matches!(
            bincode::deserialize::<FinalizationLedgerKey>(key).unwrap(),
            FinalizationLedgerKey::Witness(_)
        ) {
            count.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[tokio::test]
async fn constructor_propagates_each_audit_page_error_before_projection_or_readiness() {
    let original = ledger_with_committed_rounds(65);
    let before = original.store.raw_store().to_map().unwrap();
    for revision in [1, 31, 32, 33, 64, 65] {
        let failed_key = witness_key(&original, revision);
        let reads = Arc::new(AtomicUsize::new(0));
        let read_count = reads.clone();
        let error = KvStoreError::IoError(format!("constructor witness boundary {revision}"));
        let injected = error.clone();
        let observed = Arc::new(ObservedAuditStore {
            inner: original.store.raw_store().clone(),
            on_read: Arc::new(move |keys| {
                count_witnesses(keys, &read_count);
                if keys.contains(&failed_key) {
                    Err(injected.clone())
                } else {
                    Ok(())
                }
            }),
            on_delete: None,
            on_put: Some(Arc::new(|_| {
                panic!("failed constructor audit must not publish projection progress")
            })),
            _lifetime: Arc::new(AuditStoreLifetime(None)),
        });
        let mut manager = AuditManager::new(observed).await;
        let result = BlockDagKeyValueStorage::new(&mut manager).await;
        match result {
            Ok(_) => panic!("failed constructor audit returned usable storage"),
            Err(actual) => assert_eq!(actual, error),
        }
        assert_eq!(reads.load(Ordering::SeqCst), revision as usize);
        assert_eq!(original.projection_cursor().unwrap(), 0);
        assert_eq!(original.store.raw_store().to_map().unwrap(), before);
    }
}

#[tokio::test]
async fn cancelled_constructor_finishes_only_its_active_page_and_restart_reaudits_genesis() {
    for revision in [1, 33] {
        let original = ledger_with_committed_rounds(65);
        let before = original.store.raw_store().to_map().unwrap();
        let pause_key = witness_key(&original, revision);
        let reads = Arc::new(AtomicUsize::new(0));
        let read_count = reads.clone();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered_tx));
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
        let resume = Mutex::new(resume_rx);
        let (released_tx, released_rx) = tokio::sync::oneshot::channel();
        let observed = Arc::new(ObservedAuditStore {
            inner: original.store.raw_store().clone(),
            on_read: Arc::new(move |keys| {
                count_witnesses(keys, &read_count);
                if keys.contains(&pause_key) {
                    if let Some(sender) = entered.lock().take() {
                        let _ = sender.send(());
                        resume
                            .lock()
                            .recv_timeout(Duration::from_secs(10))
                            .map_err(|error| KvStoreError::IoError(error.to_string()))?;
                    }
                }
                Ok(())
            }),
            on_delete: None,
            on_put: Some(Arc::new(|_| {
                panic!("cancelled constructor audit must not publish projection progress")
            })),
            _lifetime: Arc::new(AuditStoreLifetime(Some(released_tx))),
        });
        let mut manager = AuditManager::new(observed).await;
        let worker = tokio::spawn(async move { BlockDagKeyValueStorage::new(&mut manager).await });
        tokio::time::timeout(Duration::from_secs(10), entered_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(!worker.is_finished());
        worker.abort();
        match worker.await {
            Err(error) => assert!(error.is_cancelled()),
            Ok(_) => panic!("cancelled constructor returned a readiness result"),
        }
        resume_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), released_rx)
            .await
            .unwrap()
            .unwrap();
        let page_size = FinalizationLedger::INTEGRITY_PAGE_RECORDS.get();
        assert_eq!(
            reads.load(Ordering::SeqCst),
            (revision as usize).div_ceil(page_size) * page_size
        );
        assert_eq!(original.store.raw_store().to_map().unwrap(), before);
        assert_eq!(original.projection_cursor().unwrap(), 0);

        let first = original.record(1).unwrap().unwrap();
        let mut witness = original.witness(&first.witness_digest).unwrap().unwrap();
        witness.target_post_state_hash = BlockHashSerde(hash(249));
        original
            .store
            .put_one(
                FinalizationLedgerKey::Witness(first.witness_digest),
                FinalizationLedgerValue::Witness(witness),
            )
            .unwrap();
        let expected = original.validate_integrity().unwrap_err();
        let mut restarted = AuditManager::new(original.store.raw_store().clone()).await;
        match BlockDagKeyValueStorage::new(&mut restarted).await {
            Ok(_) => panic!("constructor restart skipped previously audited corruption"),
            Err(actual) => assert_eq!(actual, expected),
        }
        assert_eq!(original.projection_cursor().unwrap(), 0);
    }
}
