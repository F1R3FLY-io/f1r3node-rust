use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use casper::rust::engine::lfs_tuple_space_requester::{
    self, StatePartPath, TupleSpaceRequesterOps, PAGE_SIZE, ST,
};
use casper::rust::errors::CasperError;
use futures::StreamExt;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, ApprovedBlockCandidate, StoreItemsCursorProtocol, StoreItemsMessage,
};
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::import_session::{
    SealedStateImport, StateImportActivation, StateImportError, StateImportPage,
    StateImportPageReceipt, StateImportPublication, StateImportPublicationMode, StateImportSession,
    StateImportSpec,
};
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use shared::rust::ByteVector;
use tokio::sync::mpsc;

const STAGE_FAILURE: &str = "injected tuple import stage failure";
const SEAL_FAILURE: &str = "injected tuple import seal failure";
const PUBLISH_FAILURE: &str = "injected tuple import publish failure";
const REQUEST_FAILURE: &str = "injected tuple request failure";

fn create_hash(prefix: &str) -> Blake2b256Hash {
    Blake2b256Hash::from_hex(&format!("{:0<64}", prefix))
}

fn history_hash_1() -> Blake2b256Hash { create_hash("1a") }

fn history_path_1() -> StatePartPath { vec![(history_hash_1(), None)] }

fn continuation_path(seed: u8) -> StatePartPath {
    let mut path = vec![(history_hash_1(), None)];
    path.extend((0..5).map(|offset| (Blake2b256Hash::from_bytes(vec![seed + offset; 32]), None)));
    path
}

fn history_path_2() -> StatePartPath { continuation_path(0x20) }

fn unrequested_path() -> StatePartPath { vec![(create_hash("3a"), None)] }

fn history_1() -> Vec<(Blake2b256Hash, Bytes)> {
    vec![(create_hash("1a1"), Bytes::from_static(b"history-1"))]
}

fn history_2() -> Vec<(Blake2b256Hash, Bytes)> {
    vec![(create_hash("2a1"), Bytes::from_static(b"history-2"))]
}

fn data_1() -> Vec<(Blake2b256Hash, Bytes)> {
    vec![(create_hash("1b1"), Bytes::from_static(b"data-1"))]
}

fn data_2() -> Vec<(Blake2b256Hash, Bytes)> {
    vec![(create_hash("2b1"), Bytes::from_static(b"data-2"))]
}

fn approved_block() -> ApprovedBlock {
    ApprovedBlock {
        candidate: ApprovedBlockCandidate {
            block: get_random_block(
                None,
                None,
                None,
                Some(history_hash_1().to_bytes_prost()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            required_sigs: 0,
        },
        sigs: Vec::new(),
    }
}

fn message(
    start_path: StatePartPath,
    last_path: StatePartPath,
    history_items: Vec<(Blake2b256Hash, Bytes)>,
    data_items: Vec<(Blake2b256Hash, Bytes)>,
) -> StoreItemsMessage {
    let terminal = start_path == last_path;
    StoreItemsMessage {
        start_path,
        last_path,
        history_items,
        data_items,
        cursor_protocol: StoreItemsCursorProtocol::AuthenticatedTerminal,
        terminal,
    }
}

fn injected(error: &'static str) -> StateImportError {
    StateImportError::InternalInvariant(error.to_string())
}

#[derive(Clone, Debug, Default)]
struct FailureInjection {
    stage: bool,
    seal: bool,
    publish: bool,
    wrong_activation: bool,
}

#[derive(Debug, Default)]
struct ImportObservations {
    begun_specs: Vec<StateImportSpec>,
    staged_pages: Vec<StateImportPage>,
    seal_count: usize,
    publish_count: usize,
    activations: Vec<Blake2b256Hash>,
}

#[derive(Clone)]
struct MockRSpaceImporter {
    observations: Arc<Mutex<ImportObservations>>,
    failures: Arc<Mutex<FailureInjection>>,
}

impl MockRSpaceImporter {
    fn new(failures: FailureInjection) -> Self {
        Self {
            observations: Arc::new(Mutex::new(ImportObservations::default())),
            failures: Arc::new(Mutex::new(failures)),
        }
    }
}

impl RSpaceImporter for MockRSpaceImporter {
    fn begin_import(
        &self,
        spec: StateImportSpec,
    ) -> Result<Box<dyn StateImportSession>, StateImportError> {
        self.observations
            .lock()
            .expect("observation lock")
            .begun_specs
            .push(spec.clone());
        Ok(Box::new(MockOpenImport {
            spec,
            observations: Arc::clone(&self.observations),
            failures: Arc::clone(&self.failures),
        }))
    }

    fn get_history_item(
        &self,
        _hash: Blake2b256Hash,
    ) -> Result<Option<ByteVector>, StateImportError> {
        Ok(None)
    }
}

struct MockOpenImport {
    spec: StateImportSpec,
    observations: Arc<Mutex<ImportObservations>>,
    failures: Arc<Mutex<FailureInjection>>,
}

impl StateImportSession for MockOpenImport {
    fn spec(&self) -> &StateImportSpec { &self.spec }

    fn validate_and_stage_page(
        &mut self,
        page: StateImportPage,
    ) -> Result<StateImportPageReceipt, StateImportError> {
        let terminal = page.advertised_next_path == page.start_path;
        let next_path = page.advertised_next_path.clone();
        let history_objects_staged = page.history_items.len();
        let cold_leaves_staged = page.data_items.len();
        self.observations
            .lock()
            .expect("observation lock")
            .staged_pages
            .push(page);
        if self.failures.lock().expect("failure lock").stage {
            return Err(injected(STAGE_FAILURE));
        }
        Ok(StateImportPageReceipt {
            next_path,
            terminal,
            history_objects_staged,
            cold_leaves_staged,
        })
    }

    fn seal(self: Box<Self>) -> Result<Box<dyn SealedStateImport>, StateImportError> {
        self.observations
            .lock()
            .expect("observation lock")
            .seal_count += 1;
        if self.failures.lock().expect("failure lock").seal {
            return Err(injected(SEAL_FAILURE));
        }
        Ok(Box::new(MockSealedImport {
            spec: self.spec,
            observations: self.observations,
            failures: self.failures,
        }))
    }
}

struct MockSealedImport {
    spec: StateImportSpec,
    observations: Arc<Mutex<ImportObservations>>,
    failures: Arc<Mutex<FailureInjection>>,
}

impl SealedStateImport for MockSealedImport {
    fn spec(&self) -> &StateImportSpec { &self.spec }

    fn publish(self: Box<Self>) -> Result<StateImportPublication, StateImportError> {
        self.observations
            .lock()
            .expect("observation lock")
            .publish_count += 1;
        let failures = self.failures.lock().expect("failure lock").clone();
        if failures.publish {
            return Err(injected(PUBLISH_FAILURE));
        }
        let target = match self.spec.publication_mode() {
            StateImportPublicationMode::Activate(root) => root.clone(),
            StateImportPublicationMode::PreserveCurrent => {
                return Err(injected("mock expected activation authority"));
            }
        };
        let activated = if failures.wrong_activation {
            create_hash("ff")
        } else {
            target
        };
        self.observations
            .lock()
            .expect("observation lock")
            .activations
            .push(activated.clone());
        Ok(StateImportPublication {
            verified_roots: self.spec.roots().to_vec(),
            activation: Some(StateImportActivation {
                root: activated,
                generation: 1,
            }),
        })
    }
}

struct MockTupleSpaceRequesterOps {
    requests: mpsc::UnboundedSender<(StatePartPath, i32)>,
    fail_request: Arc<AtomicBool>,
}

#[async_trait]
impl TupleSpaceRequesterOps for MockTupleSpaceRequesterOps {
    async fn request_for_store_item(
        &self,
        path: &StatePartPath,
        page_size: i32,
        cursor_protocol: StoreItemsCursorProtocol,
    ) -> Result<(), CasperError> {
        assert_eq!(
            cursor_protocol,
            StoreItemsCursorProtocol::AuthenticatedTerminal
        );
        if self.fail_request.load(Ordering::Acquire) {
            return Err(CasperError::StreamError(REQUEST_FAILURE.to_string()));
        }
        self.requests
            .send((path.clone(), page_size))
            .map_err(|_| CasperError::StreamError("request observer closed".to_string()))
    }
}

struct Harness {
    responses: mpsc::Sender<StoreItemsMessage>,
    requests: mpsc::UnboundedReceiver<(StatePartPath, i32)>,
    stream_items: mpsc::UnboundedReceiver<Result<ST<StatePartPath>, CasperError>>,
    observations: Arc<Mutex<ImportObservations>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Harness {
    fn drop(&mut self) { self.task.abort(); }
}

impl Harness {
    async fn new(
        timeout: Duration,
        failures: FailureInjection,
        fail_request: bool,
    ) -> Result<Self, CasperError> {
        let (response_tx, response_rx) = mpsc::channel(16);
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let importer = MockRSpaceImporter::new(failures);
        let observations = Arc::clone(&importer.observations);
        let stream = lfs_tuple_space_requester::stream(
            &approved_block(),
            response_rx,
            Arc::new(AtomicUsize::new(0)),
            timeout,
            MockTupleSpaceRequesterOps {
                requests: request_tx,
                fail_request: Arc::new(AtomicBool::new(fail_request)),
            },
            Arc::new(importer),
        )
        .await?;
        let (stream_tx, stream_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            tokio::pin!(stream);
            while let Some(item) = stream.next().await {
                let terminal = item.is_err();
                if stream_tx.send(item).is_err() || terminal {
                    break;
                }
            }
        });
        Ok(Self {
            responses: response_tx,
            requests: request_rx,
            stream_items: stream_rx,
            observations,
            task,
        })
    }

    async fn next_request(&mut self) -> (StatePartPath, i32) {
        tokio::time::timeout(Duration::from_secs(1), self.requests.recv())
            .await
            .expect("request timeout")
            .expect("request channel closed")
    }

    async fn next_item(&mut self) -> Result<ST<StatePartPath>, CasperError> {
        tokio::time::timeout(Duration::from_secs(1), self.stream_items.recv())
            .await
            .expect("stream item timeout")
            .expect("stream item channel closed")
    }

    async fn send(&self, message: StoreItemsMessage) {
        self.responses
            .send(message)
            .await
            .expect("response channel closed");
    }

    async fn assert_no_request(&mut self) {
        match tokio::time::timeout(Duration::from_millis(50), self.requests.recv()).await {
            Err(_) | Ok(None) => {}
            Ok(Some(request)) => panic!("unexpected tuple-space request: {request:?}"),
        }
    }
}

async fn start(harness: &mut Harness) {
    assert_eq!(harness.next_request().await, (history_path_1(), PAGE_SIZE));
    assert!(!harness
        .next_item()
        .await
        .expect("initial state")
        .is_finished());
}

fn assert_exact_error(actual: CasperError, expected: StateImportError) {
    assert_eq!(actual, CasperError::StateImportError(expected));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_logger;

    #[tokio::test]
    async fn begins_one_activate_session_and_dispatches_only_authenticated_receipt_cursor() {
        init_logger();
        let mut harness = Harness::new(Duration::from_secs(60), FailureInjection::default(), false)
            .await
            .expect("create stream");
        start(&mut harness).await;

        harness
            .send(message(
                history_path_1(),
                history_path_2(),
                history_1(),
                data_1(),
            ))
            .await;
        assert_eq!(harness.next_request().await, (history_path_2(), PAGE_SIZE));
        assert!(!harness.next_item().await.expect("page state").is_finished());

        let observed = harness.observations.lock().expect("observation lock");
        assert_eq!(observed.begun_specs.len(), 1);
        assert_eq!(observed.begun_specs[0].roots(), &[history_hash_1()]);
        assert_eq!(
            observed.begun_specs[0].publication_mode(),
            &StateImportPublicationMode::Activate(history_hash_1())
        );
        assert_eq!(observed.staged_pages.len(), 1);
        let page = &observed.staged_pages[0];
        assert_eq!(page.start_path, history_path_1());
        assert_eq!(page.advertised_next_path, history_path_2());
        assert_eq!(page.skip, 0);
        assert_eq!(page.take, PAGE_SIZE);
        assert_eq!(
            page.history_items,
            history_1()
                .into_iter()
                .map(|(hash, bytes)| (hash, bytes.to_vec()))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            page.data_items,
            data_1()
                .into_iter()
                .map(|(hash, bytes)| (hash, bytes.to_vec()))
                .collect::<Vec<_>>()
        );
        assert_eq!(observed.seal_count, 0);
        assert_eq!(observed.publish_count, 0);
    }

    #[tokio::test]
    async fn unsolicited_response_is_ignored_without_staging_or_dispatch() {
        let mut harness = Harness::new(Duration::from_secs(60), FailureInjection::default(), false)
            .await
            .expect("create stream");
        start(&mut harness).await;
        harness
            .send(message(
                unrequested_path(),
                history_path_2(),
                history_2(),
                data_2(),
            ))
            .await;
        harness.assert_no_request().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(harness
            .observations
            .lock()
            .expect("observation lock")
            .staged_pages
            .is_empty());
    }

    #[tokio::test]
    async fn stale_response_does_not_trigger_or_consume_a_stage_failure() {
        let failures = FailureInjection {
            stage: true,
            ..FailureInjection::default()
        };
        let mut harness = Harness::new(Duration::from_secs(60), failures, false)
            .await
            .expect("create stream");
        start(&mut harness).await;

        harness
            .send(message(
                unrequested_path(),
                history_path_2(),
                history_2(),
                data_2(),
            ))
            .await;
        harness.assert_no_request().await;
        assert!(
            harness
                .observations
                .lock()
                .expect("observation lock")
                .staged_pages
                .is_empty(),
            "a stale response must not reach the affine staging capability"
        );

        harness
            .send(message(
                history_path_1(),
                history_path_2(),
                history_1(),
                data_1(),
            ))
            .await;
        assert_exact_error(
            harness.next_item().await.unwrap_err(),
            injected(STAGE_FAILURE),
        );
    }

    #[tokio::test]
    async fn delayed_duplicate_is_ignored_after_cursor_retirement() {
        let mut harness = Harness::new(Duration::from_secs(60), FailureInjection::default(), false)
            .await
            .expect("create stream");
        start(&mut harness).await;
        let first = message(history_path_1(), history_path_2(), history_1(), data_1());
        harness.send(first.clone()).await;
        assert_eq!(harness.next_request().await, (history_path_2(), PAGE_SIZE));
        harness.next_item().await.expect("page state");

        harness.send(first).await;
        harness.assert_no_request().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            harness
                .observations
                .lock()
                .expect("observation lock")
                .staged_pages
                .len(),
            1
        );

        harness
            .send(message(
                history_path_2(),
                history_path_2(),
                history_2(),
                data_2(),
            ))
            .await;
        assert!(harness
            .next_item()
            .await
            .expect("final state")
            .is_finished());
    }

    #[tokio::test]
    async fn active_request_state_is_bounded_by_live_cursors_not_transfer_length() {
        let mut harness = Harness::new(Duration::from_secs(60), FailureInjection::default(), false)
            .await
            .expect("create stream");
        start(&mut harness).await;

        let mut current = history_path_1();
        for seed in 1..=64 {
            let next = continuation_path(seed);
            harness
                .send(message(current, next.clone(), Vec::new(), Vec::new()))
                .await;
            assert_eq!(harness.next_request().await, (next.clone(), PAGE_SIZE));
            let state = harness.next_item().await.expect("page state");
            assert_eq!(
                state.active_count(),
                1,
                "completed cursors must be retired instead of accumulating"
            );
            current = next;
        }

        harness
            .send(message(current.clone(), current, Vec::new(), Vec::new()))
            .await;
        assert!(harness
            .next_item()
            .await
            .expect("final state")
            .is_finished());
    }

    #[tokio::test]
    async fn terminal_page_seals_then_publishes_exact_target_activation() {
        let mut harness = Harness::new(Duration::from_secs(60), FailureInjection::default(), false)
            .await
            .expect("create stream");
        start(&mut harness).await;
        harness
            .send(message(
                history_path_1(),
                history_path_1(),
                history_1(),
                data_1(),
            ))
            .await;
        assert!(harness
            .next_item()
            .await
            .expect("final state")
            .is_finished());
        let observed = harness.observations.lock().expect("observation lock");
        assert_eq!(observed.seal_count, 1);
        assert_eq!(observed.publish_count, 1);
        assert_eq!(observed.activations, vec![history_hash_1()]);
    }

    #[tokio::test]
    async fn stage_failure_is_an_exact_stream_error_and_does_not_advance() {
        let failures = FailureInjection {
            stage: true,
            ..FailureInjection::default()
        };
        let mut harness = Harness::new(Duration::from_secs(60), failures, false)
            .await
            .expect("create stream");
        start(&mut harness).await;
        harness
            .send(message(
                history_path_1(),
                history_path_2(),
                history_1(),
                data_1(),
            ))
            .await;
        assert_exact_error(
            harness.next_item().await.unwrap_err(),
            injected(STAGE_FAILURE),
        );
        harness.assert_no_request().await;
        let observed = harness.observations.lock().expect("observation lock");
        assert_eq!(observed.staged_pages.len(), 1);
        assert_eq!(observed.seal_count, 0);
        assert_eq!(observed.publish_count, 0);
    }

    #[tokio::test]
    async fn seal_failure_is_an_exact_stream_error_and_never_publishes() {
        let failures = FailureInjection {
            seal: true,
            ..FailureInjection::default()
        };
        let mut harness = Harness::new(Duration::from_secs(60), failures, false)
            .await
            .expect("create stream");
        start(&mut harness).await;
        harness
            .send(message(
                history_path_1(),
                history_path_1(),
                history_1(),
                data_1(),
            ))
            .await;
        assert_exact_error(
            harness.next_item().await.unwrap_err(),
            injected(SEAL_FAILURE),
        );
        let observed = harness.observations.lock().expect("observation lock");
        assert_eq!(observed.seal_count, 1);
        assert_eq!(observed.publish_count, 0);
    }

    #[tokio::test]
    async fn publish_failure_is_an_exact_stream_error() {
        let failures = FailureInjection {
            publish: true,
            ..FailureInjection::default()
        };
        let mut harness = Harness::new(Duration::from_secs(60), failures, false)
            .await
            .expect("create stream");
        start(&mut harness).await;
        harness
            .send(message(
                history_path_1(),
                history_path_1(),
                history_1(),
                data_1(),
            ))
            .await;
        assert_exact_error(
            harness.next_item().await.unwrap_err(),
            injected(PUBLISH_FAILURE),
        );
        let observed = harness.observations.lock().expect("observation lock");
        assert_eq!(observed.seal_count, 1);
        assert_eq!(observed.publish_count, 1);
    }

    #[tokio::test]
    async fn publication_with_wrong_activation_is_rejected() {
        let failures = FailureInjection {
            wrong_activation: true,
            ..FailureInjection::default()
        };
        let mut harness = Harness::new(Duration::from_secs(60), failures, false)
            .await
            .expect("create stream");
        start(&mut harness).await;
        harness
            .send(message(
                history_path_1(),
                history_path_1(),
                history_1(),
                data_1(),
            ))
            .await;
        assert_exact_error(
            harness.next_item().await.unwrap_err(),
            injected("tuple-space import publication did not exactly activate its target root"),
        );
    }

    #[tokio::test]
    async fn request_failure_is_observable_instead_of_normal_eof() {
        let mut harness = Harness::new(Duration::from_secs(60), FailureInjection::default(), true)
            .await
            .expect("create stream");
        assert_eq!(
            harness.next_item().await.unwrap_err(),
            CasperError::StreamError(REQUEST_FAILURE.to_string())
        );
        let observed = harness.observations.lock().expect("observation lock");
        assert_eq!(observed.begun_specs.len(), 1);
        assert!(observed.staged_pages.is_empty());
    }

    #[tokio::test]
    async fn requested_cursor_is_resent_after_idle_timeout() {
        let mut harness = Harness::new(
            Duration::from_millis(30),
            FailureInjection::default(),
            false,
        )
        .await
        .expect("create stream");
        start(&mut harness).await;
        assert_eq!(harness.next_request().await, (history_path_1(), PAGE_SIZE));
        assert!(!harness
            .next_item()
            .await
            .expect("resend state")
            .is_finished());
    }
}
