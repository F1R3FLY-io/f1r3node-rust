use std::future::{poll_fn, Future};
use std::task::Poll;
use std::time::Duration;

use super::*;

const BOUNDARY_TIMEOUT: Duration = Duration::from_secs(90);

fn lifecycle_deploy(timestamp: i64) -> Cosigned<DeployData> {
    protocol_v6_source(
        r#"new result in {
            result!(17) |
            result!(23) |
            for (@first <- result; @second <- result) {
                @"replay-lifecycle-result"!(first + second)
            }
        }"#
        .to_string(),
        timestamp,
        construct_deploy::DEFAULT_SEC.clone(),
    )
}

async fn lifecycle_block(
    manager: &RuntimeManager,
    genesis: &BlockMessage,
    block_data: &BlockData,
) -> BlockMessage {
    let (post_state, deploys, system_deploys) = manager
        .compute_state_cosigned(
            &genesis.body.state.post_state_hash,
            vec![lifecycle_deploy(block_data.time_stamp)],
            Vec::new(),
            block_data.clone(),
            None,
        )
        .await
        .expect("lifecycle proposal");
    assert_eq!(deploys.len(), 1);
    assert!(!deploys[0].is_failed);
    let mut block = genesis.clone();
    block.block_hash = vec![0x75; 32].into();
    block.header.parents_hash_list = vec![genesis.block_hash.clone()];
    block.header.timestamp = block_data.time_stamp;
    block.body.state.pre_state_hash = genesis.body.state.post_state_hash.clone();
    block.body.state.post_state_hash = post_state;
    block.body.state.block_number = block_data.block_number;
    block.body.deploys = deploys;
    block.body.system_deploys = system_deploys;
    block.sender = block_data.sender.bytes.clone();
    block.seq_num = block_data.seq_num;
    block
}

fn counter_value(
    snapshotter: &metrics_util::debugging::Snapshotter,
    name: &str,
    origin: Option<&str>,
) -> u64 {
    snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter_map(|(key, _, _, value)| {
            if key.key().name() != name
                || origin.is_some_and(|origin| {
                    !key.key()
                        .labels()
                        .any(|label| label.key() == "origin" && label.value() == origin)
                })
            {
                return None;
            }
            match value {
                metrics_util::debugging::DebugValue::Counter(count) => Some(count),
                _ => None,
            }
        })
        .sum()
}

async fn assert_historical_results(manager: &RuntimeManager, block: &BlockMessage) {
    let channel = new_gstring_par("replay-lifecycle-result".to_string(), Vec::new(), false);
    assert!(manager
        .get_data(block.body.state.pre_state_hash.clone(), &channel)
        .await
        .unwrap()
        .is_empty());
    let result = manager
        .get_data(block.body.state.post_state_hash.clone(), &channel)
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(RhoNumber::unapply(&result[0]), Some(40));
}

#[tokio::test]
async fn active_replay_cancellation_preserves_publication_and_independent_replay() {
    with_runtime_manager(|manager, context, genesis| async move {
        let block_data = BlockData {
            time_stamp: 45,
            block_number: 1,
            sender: context.validator_pks()[0].clone(),
            seq_num: 1,
        };
        let block = lifecycle_block(&manager, &genesis, &block_data).await;
        let cache = manager.replay_cache.as_ref().unwrap();
        cache.clear();
        assert!(manager.delete_mergeable_channels(&block).unwrap());
        let before = manager.mergeable_store.raw_store().to_map().unwrap();
        let recorder = metrics_util::debugging::DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let mut replay = Box::pin(manager.replay_compute_state(
            &block.body.state.pre_state_hash,
            block.body.deploys.clone(),
            block.body.system_deploys.clone(),
            &block_data,
            None,
            false,
        ));
        tokio::time::timeout(
            BOUNDARY_TIMEOUT,
            poll_fn(|cx| {
                let result = metrics::with_local_recorder(&recorder, || replay.as_mut().poll(cx));
                assert!(
                    result.is_pending(),
                    "replay completed before the active cancellation cut"
                );
                let attempts = counter_value(
                    &snapshotter,
                    casper::rust::metrics_constants::USER_DEPLOY_EVALUATION_ATTEMPTS_METRIC,
                    Some("replay"),
                );
                let reductions = counter_value(&snapshotter, "reducer.eval_par.calls", None);
                if attempts > 0 && reductions > 0 {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }),
        )
        .await
        .expect("replay did not reach active reduction");
        drop(replay);
        assert!(cache.is_empty());
        assert_eq!(
            manager.mergeable_store.raw_store().to_map().unwrap(),
            before
        );
        let replay_lock = manager.replay_lock();
        let permit = tokio::time::timeout(BOUNDARY_TIMEOUT, replay_lock.acquire_reporting())
            .await
            .expect("cancelled replay retained its permit")
            .unwrap();
        drop(permit);
        let replayed = tokio::time::timeout(
            BOUNDARY_TIMEOUT,
            manager.replay_compute_state(
                &block.body.state.pre_state_hash,
                block.body.deploys.clone(),
                block.body.system_deploys.clone(),
                &block_data,
                None,
                false,
            ),
        )
        .await
        .expect("independent replay did not complete")
        .expect("independent replay failed");
        assert_eq!(replayed, block.body.state.post_state_hash);
        assert!(cache.is_empty());
        assert_eq!(
            manager.mergeable_store.raw_store().to_map().unwrap(),
            before
        );
        assert_historical_results(&manager, &block).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn post_publication_cancellation_preserves_complete_replay_evidence() {
    with_runtime_manager(|manager, context, genesis| async move {
        let block_data = BlockData {
            time_stamp: 46,
            block_number: 1,
            sender: context.validator_pks()[0].clone(),
            seq_num: 1,
        };
        let block = lifecycle_block(&manager, &genesis, &block_data).await;
        let cache = manager.replay_cache.as_ref().unwrap();
        cache.clear();
        assert!(manager.delete_mergeable_channels(&block).unwrap());
        let mut proposal = Box::pin(manager.compute_state_cosigned(
            &block.body.state.pre_state_hash,
            vec![lifecycle_deploy(block_data.time_stamp)],
            Vec::new(),
            block_data.clone(),
            None,
        ));
        tokio::time::timeout(
            BOUNDARY_TIMEOUT,
            poll_fn(|cx| {
                let result = proposal.as_mut().poll(cx);
                assert!(
                    result.is_pending(),
                    "proposal completed before the post-publication cut"
                );
                if cache.is_empty() {
                    Poll::Pending
                } else {
                    assert!(manager.has_mergeable_entry(&block).unwrap());
                    Poll::Ready(())
                }
            }),
        )
        .await
        .expect("proposal did not reach pending bonds computation after publication");
        let published = manager.mergeable_store.raw_store().to_map().unwrap();
        drop(proposal);
        assert!(!cache.is_empty());
        assert_eq!(
            manager.mergeable_store.raw_store().to_map().unwrap(),
            published
        );
        let mut cold = manager.clone();
        cold.replay_cache = None;
        for runtime in [&manager, &cold] {
            let replayed = tokio::time::timeout(
                BOUNDARY_TIMEOUT,
                runtime.replay_compute_state(
                    &block.body.state.pre_state_hash,
                    block.body.deploys.clone(),
                    block.body.system_deploys.clone(),
                    &block_data,
                    None,
                    false,
                ),
            )
            .await
            .expect("independent replay did not complete")
            .expect("published replay evidence failed");
            assert_eq!(replayed, block.body.state.post_state_hash);
            assert!(manager.has_mergeable_entry(&block).unwrap());
            assert_eq!(
                manager.mergeable_store.raw_store().to_map().unwrap(),
                published
            );
        }
        assert_historical_results(&manager, &block).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cancelled_reducer_releases_an_entered_handler_before_runtime_reuse() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use rholang::rust::interpreter::system_processes::BodyRefs;
    use tokio::sync::Notify;

    struct HandlerDrop(Arc<Notify>);

    impl Drop for HandlerDrop {
        fn drop(&mut self) { self.0.notify_one(); }
    }

    with_runtime_manager(|manager, _, genesis| async move {
        let runtime = manager.spawn_runtime().await;
        let mut observer = runtime.clone();
        let start = Blake2b256Hash::from_bytes_prost(&genesis.body.state.post_state_hash);
        observer.reset(&start).await.unwrap();
        let entered = Arc::new(Notify::new());
        let released = Arc::new(Notify::new());
        let continue_handler = Arc::new(Notify::new());
        let continued = Arc::new(AtomicBool::new(false));
        let dispatch = observer.reducer.dispatcher._dispatch_table.clone();
        let original = {
            let entered = entered.clone();
            let released = released.clone();
            let continue_handler = continue_handler.clone();
            let continued = continued.clone();
            dispatch.write().await.insert(
                BodyRefs::STDOUT,
                Box::new(move |_| {
                    let entered = entered.clone();
                    let released = released.clone();
                    let continue_handler = continue_handler.clone();
                    let continued = continued.clone();
                    Box::pin(async move {
                        let _release = HandlerDrop(released);
                        entered.notify_one();
                        continue_handler.notified().await;
                        continued.store(true, Ordering::SeqCst);
                        Ok(Vec::new())
                    })
                }),
            )
        };
        assert!(original.is_some());
        observer.reducer.reset_eval_work_stats();
        let before = manager.mergeable_store.raw_store().to_map().unwrap();
        let cache = manager.replay_cache.as_ref().unwrap();
        cache.clear();
        let deploy = protocol_v6_source(
            r#"new stdout(`rho:io:stdout`) in {
                stdout!("controlled cancellation") | @"cancel-private-result"!(1)
            }"#
                .to_string(),
            48,
            construct_deploy::DEFAULT_SEC.clone(),
        );
        let mut operations = RuntimeOps::new(runtime);
        let mut evaluation = Box::pin(operations.evaluate_cosigned(&deploy));
        tokio::select! {
            biased;
            result = &mut evaluation => panic!("evaluation completed before handler entry: {result:?}"),
            result = tokio::time::timeout(BOUNDARY_TIMEOUT, entered.notified()) => {
                result.expect("handler was not entered");
            }
        }
        assert!(observer.reducer.eval_work_stats().spawned_eval_tasks > 0);
        drop(evaluation);
        tokio::time::timeout(BOUNDARY_TIMEOUT, released.notified())
            .await
            .expect("cancelled child retained its handler");
        continue_handler.notify_one();
        assert!(!continued.load(Ordering::SeqCst));
        tokio::time::timeout(BOUNDARY_TIMEOUT, observer.create_checkpoint())
            .await
            .expect("cancelled children retained the checkpoint boundary");
        observer.reset(&start).await.unwrap();
        assert_eq!(observer.create_checkpoint().await.root, start);
        dispatch.write().await.insert(BodyRefs::STDOUT, original.unwrap());
        assert!(cache.is_empty());
        assert_eq!(manager.mergeable_store.raw_store().to_map().unwrap(), before);
        assert!(manager
            .get_data(
                genesis.body.state.post_state_hash,
                &new_gstring_par("cancel-private-result".to_string(), Vec::new(), false),
            )
            .await
            .unwrap()
            .is_empty());
    })
    .await
    .unwrap();
}

mod process_crash {
    use std::collections::{BTreeMap, VecDeque};
    use std::io::{BufRead, Read, Write};
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::sync::{mpsc, Arc, Mutex};

    use casper::rust::storage::rnode_key_value_store_manager::new_key_value_store_manager;
    use prost::Message;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    const CHILD_DIRECTORY: &str = "F1R3_REPLAY_PUBLICATION_CHILD_DIRECTORY";
    const CHILD_STAGE: &str = "F1R3_REPLAY_PUBLICATION_CHILD_STAGE";
    const CHILD_MODE: &str = "F1R3_REPLAY_PUBLICATION_CHILD_MODE";
    const READY: &str = "F1R3_REPLAY_PUBLICATION_BOUNDARY";
    const TEST_NAME: &str = concat!(
        module_path!(),
        "::process_crash_preserves_only_committed_mergeable_evidence"
    );

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum CrashStage {
        BeforeWrite,
        AfterWrite,
        AfterPublication,
    }

    impl CrashStage {
        fn name(self) -> &'static str {
            match self {
                Self::BeforeWrite => "before-write",
                Self::AfterWrite => "after-write",
                Self::AfterPublication => "after-publication",
            }
        }

        fn parse(value: &str) -> Self {
            match value {
                "before-write" => Self::BeforeWrite,
                "after-write" => Self::AfterWrite,
                "after-publication" => Self::AfterPublication,
                _ => panic!("invalid crash stage"),
            }
        }
    }

    fn write_boundary(output: &mut impl Write, event: &str) {
        writeln!(output, "\n{READY}:{event}").unwrap();
        output.flush().unwrap();
    }

    proptest::proptest! {
        #[test]
        fn boundary_frame_is_independent_of_test_runner_prefix(
            prefix in "[A-Za-z0-9_: .]{0,256}",
            stage in 0_usize..4,
        ) {
            let event = ["before-write", "after-write", "after-publication", "verified"][stage];
            let mut output = prefix.as_bytes().to_vec();
            write_boundary(&mut output, event);
            let output = String::from_utf8(output).unwrap();
            let expected = format!("{READY}:{event}");
            proptest::prop_assert_eq!(output.lines().last(), Some(expected.as_str()));
            proptest::prop_assert!(output.starts_with(&prefix));
        }
    }

    fn wait_for_parent_termination(stage: CrashStage) -> ! {
        write_boundary(&mut std::io::stdout().lock(), stage.name());
        let mut input = [0_u8; 1];
        let _ = std::io::stdin().read(&mut input);
        panic!("parent did not terminate the child at the requested boundary");
    }

    #[derive(Clone)]
    struct CrashBoundaryStore {
        inner: Arc<dyn KeyValueStore>,
        expected: Arc<BTreeMap<Vec<u8>, Vec<u8>>>,
        cache: Arc<casper::rust::util::rholang::replay_cache::InMemoryReplayCache>,
        stage: CrashStage,
    }

    impl KeyValueStore for CrashBoundaryStore {
        fn as_any(&self) -> &dyn std::any::Any { self }

        fn with_value(
            &self,
            key: &Vec<u8>,
            reader: &mut shared::rust::store::key_value_store::ValueReader<'_>,
        ) -> Result<(), KvStoreError> {
            self.inner.with_value(key, reader)
        }

        fn visit_entries(
            &self,
            reader: &mut shared::rust::store::key_value_store::EntryReader<'_>,
        ) -> Result<(), KvStoreError> {
            self.inner.visit_entries(reader)
        }

        fn get(&self, keys: &Vec<Vec<u8>>) -> Result<Vec<Option<Vec<u8>>>, KvStoreError> {
            self.inner.get(keys)
        }

        fn put(&self, pairs: Vec<(Vec<u8>, Vec<u8>)>) -> Result<(), KvStoreError> {
            assert_eq!(
                pairs.iter().cloned().collect::<BTreeMap<_, _>>(),
                *self.expected
            );
            assert!(!self.inner.non_empty()?);
            assert!(self.cache.is_empty());
            if self.stage == CrashStage::BeforeWrite {
                wait_for_parent_termination(self.stage);
            }
            self.inner.put(pairs)?;
            assert_eq!(self.inner.to_map()?, *self.expected);
            assert!(self.cache.is_empty());
            if self.stage == CrashStage::AfterWrite {
                wait_for_parent_termination(self.stage);
            }
            Ok(())
        }

        fn put_one_if_absent(&self, _key: Vec<u8>, _value: Vec<u8>) -> Result<bool, KvStoreError> {
            panic!("mergeable publication unexpectedly changed its write operation");
        }

        fn delete(&self, keys: Vec<Vec<u8>>) -> Result<usize, KvStoreError> {
            self.inner.delete(keys)
        }

        fn iterate(&self, f: fn(Vec<u8>, Vec<u8>)) -> Result<(), KvStoreError> {
            self.inner.iterate(f)
        }

        fn iterate_while(
            &self,
            f: &mut dyn FnMut(Vec<u8>, Vec<u8>) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            self.inner.iterate_while(f)
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

        fn to_map(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, KvStoreError> { self.inner.to_map() }

        fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

        fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

        fn size_bytes(&self) -> usize { self.inner.size_bytes() }
    }

    struct OwnedChild {
        child: Child,
        reader: Option<std::thread::JoinHandle<()>>,
        messages: mpsc::Receiver<String>,
        recent_output: Arc<Mutex<VecDeque<String>>>,
    }

    impl OwnedChild {
        fn spawn(directory: &Path, stage: CrashStage, mode: &str) -> Self {
            let test_name = TEST_NAME.split_once("::").unwrap().1;
            let mut child = Command::new(std::env::current_exe().unwrap())
                .arg(test_name)
                .arg("--exact")
                .arg("--nocapture")
                .arg("--test-threads=1")
                .env(CHILD_DIRECTORY, directory)
                .env(CHILD_STAGE, stage.name())
                .env(CHILD_MODE, mode)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            let output = child.stdout.take().unwrap();
            let (sender, messages) = mpsc::channel();
            let recent_output = Arc::new(Mutex::new(VecDeque::new()));
            let retained_output = recent_output.clone();
            let reader = std::thread::spawn(move || {
                for line in std::io::BufReader::new(output).lines() {
                    let Ok(line) = line else { break };
                    {
                        let mut retained = retained_output.lock().unwrap();
                        if retained.len() == 16 {
                            retained.pop_front();
                        }
                        retained.push_back(line.chars().take(1024).collect());
                    }
                    if line.contains(READY) {
                        let _ = sender.send(line);
                    }
                }
            });
            Self {
                child,
                reader: Some(reader),
                messages,
                recent_output,
            }
        }

        fn wait_for(&self, event: &str) {
            assert_eq!(
                self.messages
                    .recv_timeout(BOUNDARY_TIMEOUT)
                    .unwrap_or_else(|error| {
                        panic!(
                            "child did not confirm {event}: {error}; recent stdout: {:?}",
                            self.recent_output.lock().unwrap()
                        );
                    }),
                format!("{READY}:{event}")
            );
        }
    }

    impl Drop for OwnedChild {
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

    async fn child_run(directory: &Path, stage: CrashStage, verify: bool) {
        let encoded = std::fs::read(directory.join("block.pb")).unwrap();
        let block = BlockMessage::from_proto(
            models::casper::BlockMessageProto::decode(encoded.as_slice()).unwrap(),
        )
        .unwrap();
        let expected: BTreeMap<Vec<u8>, Vec<u8>> =
            bincode::deserialize(&std::fs::read(directory.join("mergeable.bin")).unwrap()).unwrap();
        let mut kvm = new_key_value_store_manager(directory.join("node"), None);
        let mut manager = resources::mk_runtime_manager_at(&mut kvm, None).await;
        let cache = manager.replay_cache.as_ref().unwrap().clone();
        assert!(
            cache.is_empty(),
            "a new process inherited memory cache state"
        );
        let raw_store = manager.mergeable_store.raw_store().clone();

        if verify {
            if stage == CrashStage::BeforeWrite {
                assert!(!raw_store.non_empty().unwrap());
                assert!(!manager.has_mergeable_entry(&block).unwrap());
            } else {
                assert_eq!(raw_store.to_map().unwrap(), expected);
                assert!(manager.has_mergeable_entry(&block).unwrap());
            }
            assert_historical_results(&manager, &block).await;
            let replayed = manager
                .replay_block_from_consensus_data(&block.body.state.pre_state_hash, &block, None)
                .await
                .expect("fresh process could not independently replay after the crash");
            assert_eq!(replayed, block.body.state.post_state_hash);
            assert_eq!(raw_store.to_map().unwrap(), expected);
            assert!(!cache.is_empty());
            assert_historical_results(&manager, &block).await;
            drop(raw_store);
            drop(manager);
            kvm.shutdown().await.unwrap();
            write_boundary(&mut std::io::stdout().lock(), "verified");
            return;
        }

        assert!(!raw_store.non_empty().unwrap());
        manager.mergeable_store = KeyValueTypedStoreImpl::new(Arc::new(CrashBoundaryStore {
            inner: raw_store,
            expected: Arc::new(expected),
            cache: cache.clone(),
            stage,
        }));
        let mut proposal = Box::pin(manager.compute_state_cosigned(
            &block.body.state.pre_state_hash,
            vec![lifecycle_deploy(block.header.timestamp)],
            Vec::new(),
            BlockData::from_block(&block),
            None,
        ));
        poll_fn(|cx| {
            let result = proposal.as_mut().poll(cx);
            if stage == CrashStage::AfterPublication && !cache.is_empty() {
                assert!(manager.has_mergeable_entry(&block).unwrap());
                wait_for_parent_termination(stage);
            }
            assert!(
                result.is_pending(),
                "proposal bypassed its requested crash cut"
            );
            Poll::<()>::Pending
        })
        .await;
        panic!("crash child unexpectedly completed");
    }

    #[tokio::test]
    async fn process_crash_preserves_only_committed_mergeable_evidence() {
        if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
            let stage = CrashStage::parse(&std::env::var(CHILD_STAGE).unwrap());
            let mode = std::env::var(CHILD_MODE).unwrap();
            assert!(mode == "crash" || mode == "verify");
            child_run(Path::new(&directory), stage, mode == "verify").await;
            return;
        }

        with_runtime_manager(|manager, context, genesis| async move {
            let mut source = resources::mk_test_rnode_store_manager_from_genesis(&context);
            let mut genesis_stores = Vec::new();
            for name in ["rspace-history", "rspace-roots", "rspace-cold"] {
                let values = source
                    .store(name.to_string())
                    .await
                    .unwrap()
                    .to_map()
                    .unwrap();
                genesis_stores.push((name, values.into_iter().collect::<Vec<_>>()));
            }
            let before = manager.mergeable_store.raw_store().to_map().unwrap();
            let block_data = BlockData {
                time_stamp: 47,
                block_number: 1,
                sender: context.validator_pks()[0].clone(),
                seq_num: 1,
            };
            let block = lifecycle_block(&manager, &genesis, &block_data).await;
            let expected = manager
                .mergeable_store
                .raw_store()
                .to_map()
                .unwrap()
                .into_iter()
                .filter(|(key, value)| before.get(key) != Some(value))
                .collect::<BTreeMap<_, _>>();
            assert_eq!(expected.len(), 1);
            let scratch = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("target/casper-test-scratch");
            std::fs::create_dir_all(&scratch).unwrap();
            for stage in [
                CrashStage::BeforeWrite,
                CrashStage::AfterWrite,
                CrashStage::AfterPublication,
            ] {
                let directory = tempfile::Builder::new()
                    .prefix("replay-publication-crash-")
                    .tempdir_in(&scratch)
                    .unwrap();
                std::fs::write(
                    directory.path().join("block.pb"),
                    block.to_proto().encode_to_vec(),
                )
                .unwrap();
                std::fs::write(
                    directory.path().join("mergeable.bin"),
                    bincode::serialize(&expected).unwrap(),
                )
                .unwrap();
                let mut disk = new_key_value_store_manager(directory.path().join("node"), None);
                for (name, values) in &genesis_stores {
                    disk.store((*name).to_string())
                        .await
                        .unwrap()
                        .put(values.clone())
                        .unwrap();
                }
                disk.shutdown().await.unwrap();
                drop(disk);

                let mut child = OwnedChild::spawn(directory.path(), stage, "crash");
                child.wait_for(stage.name());
                child.child.kill().unwrap();
                let status = child.child.wait().unwrap();
                assert!(
                    !status.success(),
                    "child was not terminated at the crash cut"
                );
                drop(child);

                let mut verifier = OwnedChild::spawn(directory.path(), stage, "verify");
                verifier.wait_for("verified");
                assert!(
                    verifier.child.wait().unwrap().success(),
                    "fresh-process verification failed"
                );
                drop(verifier);
                directory.close().unwrap();
            }
        })
        .await
        .unwrap();
    }
}
