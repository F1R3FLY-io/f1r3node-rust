use std::any::Any;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use shared::rust::ByteBuffer;
use shared::rust::store::key_value_store::{
    AtomicStoreMutation, AtomicStoreOperation, EntryReader, KeyValueStore, KvStoreError,
    ValueReader, strict_atomic_mutate,
};

use super::{AlwaysMatch, Cont, Wildcard, order};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::operation_context;
use crate::rspace::rspace::{RSpace, RSpaceStore};
use crate::rspace::rspace_interface::ISpace;
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use crate::rspace::trace::Log;
use crate::rspace::trace::event::{Event, IOEvent, Produce};

const READ_FAILURE: &str = "injected history read error after checkpoint publication";

#[derive(Default)]
struct ReadFailure {
    armed: bool,
    pending_root: Option<ByteBuffer>,
    published_root: Option<ByteBuffer>,
    failures: usize,
}

#[derive(Clone)]
struct CheckpointStore {
    inner: Arc<dyn KeyValueStore>,
    roots: bool,
    fault: Arc<Mutex<ReadFailure>>,
}

impl CheckpointStore {
    fn check_read(&self, key: &[u8]) -> Result<(), KvStoreError> {
        if !self.roots {
            let mut fault = self.fault.lock().unwrap();
            if fault.pending_root.as_deref() == Some(key) {
                fault.pending_root = None;
                fault.failures += 1;
                return Err(KvStoreError::IoError(READ_FAILURE.to_string()));
            }
        }
        Ok(())
    }

    fn observe_write(&self, key: &[u8], value: &[u8]) {
        if self.roots && key == b"current-root" {
            let mut fault = self.fault.lock().unwrap();
            if fault.armed {
                fault.armed = false;
                fault.pending_root = Some(value.to_vec());
                fault.published_root = Some(value.to_vec());
            }
        }
    }
}

impl KeyValueStore for CheckpointStore {
    fn as_any(&self) -> &dyn Any { self }

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        for key in keys {
            self.check_read(key)?;
        }
        self.inner.get(keys)
    }

    fn with_value(
        &self,
        key: &ByteBuffer,
        reader: &mut ValueReader<'_>,
    ) -> Result<(), KvStoreError> {
        self.check_read(key)?;
        self.inner.with_value(key, reader)
    }

    fn visit_entries(&self, reader: &mut EntryReader<'_>) -> Result<(), KvStoreError> {
        self.inner.visit_entries(reader)
    }

    fn put(&self, rows: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        self.inner.put(rows.clone())?;
        for (key, value) in rows {
            self.observe_write(&key, &value);
        }
        Ok(())
    }

    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        let inserted = self.inner.put_one_if_absent(key.clone(), value.clone())?;
        if inserted {
            self.observe_write(&key, &value);
        }
        Ok(inserted)
    }

    fn strict_atomic_mutate(
        &self,
        mutations: &[AtomicStoreMutation<'_>],
    ) -> Result<(), KvStoreError> {
        let translated: Vec<_> = mutations
            .iter()
            .map(|mutation| AtomicStoreMutation {
                store: mutation
                    .store
                    .as_any()
                    .downcast_ref::<Self>()
                    .map_or(mutation.store, |wrapped| wrapped.inner.as_ref()),
                key: mutation.key.clone(),
                operation: mutation.operation.clone(),
            })
            .collect();
        strict_atomic_mutate(&translated)?;
        for mutation in mutations {
            if let Some(wrapped) = mutation.store.as_any().downcast_ref::<Self>() {
                match &mutation.operation {
                    AtomicStoreOperation::Put(value) |
                    AtomicStoreOperation::PutIfAbsentOrEqual(value) |
                    AtomicStoreOperation::CompareAndSwap {
                        replacement: Some(value),
                        ..
                    } => {
                        wrapped.observe_write(&mutation.key, value);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        self.inner.delete(keys)
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        self.inner.iterate(f)
    }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.inner.iterate_while(f)
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        self.inner.to_map()
    }

    fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

    fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

    fn size_bytes(&self) -> usize { self.inner.size_bytes() }
}

async fn fault_store() -> (RSpaceStore, Arc<Mutex<ReadFailure>>) {
    let mut manager = InMemoryStoreManager::new();
    let store = manager.r_space_stores().await.unwrap();
    let fault = Arc::new(Mutex::new(ReadFailure::default()));
    (
        RSpaceStore {
            history: Arc::new(CheckpointStore {
                inner: store.history,
                roots: false,
                fault: fault.clone(),
            }),
            roots: Arc::new(CheckpointStore {
                inner: store.roots,
                roots: true,
                fault: fault.clone(),
            }),
            cold: store.cold,
        },
        fault,
    )
}

fn expected_log() -> Log {
    ["ordinary", "ordered-first", "ordered-second"]
        .into_iter()
        .map(|datum| {
            Event::IoEvent(IOEvent::Produce(Produce::create(
                &"checkpoint-channel".to_string(),
                &datum.to_string(),
                false,
            )))
        })
        .collect()
}

fn installed_state(
    space: &RSpace<String, Wildcard, String, Cont>,
) -> BTreeMap<Vec<String>, (Vec<Wildcard>, Cont)> {
    space
        .installs
        .lock()
        .unwrap()
        .iter()
        .map(|(channels, install)| {
            (channels.clone(), (install.patterns.clone(), install.continuation.clone()))
        })
        .collect()
}

async fn prepare_space(space: &impl ISpace<String, Wildcard, String, Cont>) {
    assert!(
        space
            .install(vec!["installed-service".to_string()], vec![Wildcard], Cont)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        space
            .produce("checkpoint-channel".to_string(), "ordinary".to_string(), false)
            .await
            .unwrap()
            .is_none()
    );
    for (step, datum) in [(2, "ordered-second"), (1, "ordered-first")] {
        assert!(
            operation_context::scope(
                order(step),
                space.produce("checkpoint-channel".to_string(), datum.to_string(), false,)
            )
            .await
            .unwrap()
            .is_none()
        );
    }
}

#[tokio::test]
async fn checkpoint_handoff_control_preserves_play_replay_root_and_trace() {
    let (store, fault) = fault_store().await;
    let (play, replay) = RSpace::<String, Wildcard, String, Cont>::create_with_replay(
        store,
        Arc::new(Box::new(AlwaysMatch)),
    )
    .unwrap();
    prepare_space(&play).await;
    let checkpoint = play.create_checkpoint().await.unwrap();
    assert_eq!(checkpoint.log, expected_log());
    replay.rig(checkpoint.log).await.unwrap();
    prepare_space(&replay).await;
    let replay_checkpoint = replay.create_checkpoint().await.unwrap();
    assert_eq!(replay_checkpoint.root, checkpoint.root);
    assert!(replay_checkpoint.log.is_empty());
    let next = play.create_checkpoint().await.unwrap();
    assert_eq!(next.root, checkpoint.root);
    assert!(next.log.is_empty());
    assert_eq!(fault.lock().unwrap().failures, 0);
}

#[tokio::test]
async fn checkpoint_handoff_read_failure_retains_play_state_and_retry_trace() {
    let (store, fault) = fault_store().await;
    let roots = store.roots.clone();
    let play =
        RSpace::<String, Wildcard, String, Cont>::create(store, Arc::new(Box::new(AlwaysMatch)))
            .unwrap();
    prepare_space(&play).await;
    let before_repository = play.get_history_repository();
    let before_store = play.get_store();
    let before_actions = before_store.changes();
    let before_log = play.event_log.lock().unwrap().clone();
    let before_ordered = play.ordered_event_log.lock().unwrap().clone();
    let before_counters: Vec<_> = play
        .produce_counter
        .iter()
        .map(|stripe| stripe.lock().unwrap().clone())
        .collect();
    let before_installs = installed_state(&play);
    assert!(!before_log.is_empty());
    assert!(!before_ordered.is_empty());
    assert!(before_counters.iter().any(|stripe| !stripe.is_empty()));
    fault.lock().unwrap().armed = true;
    let failure = play.create_checkpoint().await;
    assert!(matches!(failure, Err(ref error) if error.to_string().contains(READ_FAILURE)));
    let published = {
        let state = fault.lock().unwrap();
        assert_eq!(state.failures, 1);
        assert!(state.pending_root.is_none());
        state.published_root.clone().unwrap()
    };
    assert_eq!(roots.get_one(&b"current-root".to_vec()).unwrap(), Some(published.clone()));
    let checks = [
        ("local repository", Arc::ptr_eq(&before_repository, &play.get_history_repository())),
        ("local root", before_repository.root() == play.get_history_repository().root()),
        ("hot-store owner", Arc::ptr_eq(&before_store, &play.get_store())),
        ("pending actions", before_actions == play.get_store().changes()),
        ("ordinary log", before_log == *play.event_log.lock().unwrap()),
        ("ordered log", before_ordered == *play.ordered_event_log.lock().unwrap()),
        (
            "produce counters",
            before_counters ==
                play.produce_counter
                    .iter()
                    .map(|stripe| stripe.lock().unwrap().clone())
                    .collect::<Vec<_>>(),
        ),
        ("installed continuations", before_installs == installed_state(&play)),
    ];
    let mut lost: Vec<_> = checks
        .into_iter()
        .filter_map(|(name, retained)| (!retained).then_some(name))
        .collect();
    let retry = play.create_checkpoint().await.unwrap();
    assert_eq!(retry.root, Blake2b256Hash::from_bytes(published));
    if retry.log != expected_log() {
        lost.push("retry trace");
    }
    let next = play.create_checkpoint().await.unwrap();
    assert_eq!(next.root, retry.root);
    assert!(next.log.is_empty());
    assert!(lost.is_empty(), "checkpoint read failure lost local state: {lost:?}");
}

#[tokio::test]
async fn checkpoint_handoff_read_failure_retains_replay_state() {
    let (store, fault) = fault_store().await;
    let (play, replay) = RSpace::<String, Wildcard, String, Cont>::create_with_replay(
        store,
        Arc::new(Box::new(AlwaysMatch)),
    )
    .unwrap();
    prepare_space(&play).await;
    let checkpoint = play.create_checkpoint().await.unwrap();
    replay.rig(checkpoint.log).await.unwrap();
    prepare_space(&replay).await;
    let before_repository = replay.get_history_repository();
    let before_store = replay.get_store();
    let before_actions = before_store.changes();
    fault.lock().unwrap().armed = true;
    let failure = replay.create_checkpoint().await;
    assert!(matches!(failure, Err(ref error) if error.to_string().contains(READ_FAILURE)));
    assert_eq!(fault.lock().unwrap().failures, 1);
    let checks = [
        ("local repository", Arc::ptr_eq(&before_repository, &replay.get_history_repository())),
        ("local root", before_repository.root() == replay.get_history_repository().root()),
        ("hot-store owner", Arc::ptr_eq(&before_store, &replay.get_store())),
        ("pending actions", before_actions == replay.get_store().changes()),
    ];
    let lost: Vec<_> = checks
        .into_iter()
        .filter_map(|(name, retained)| (!retained).then_some(name))
        .collect();
    let retry = replay.create_checkpoint().await.unwrap();
    assert_eq!(retry.root, checkpoint.root);
    assert!(retry.log.is_empty());
    replay.check_replay_data().await.unwrap();
    assert!(lost.is_empty(), "checkpoint read failure lost replay state: {lost:?}");
}
