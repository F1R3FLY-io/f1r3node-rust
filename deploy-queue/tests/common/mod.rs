//! A stand-in node serving the node's HTTP API and event stream, and a queue
//! engine and event link running against it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Signed;
use deploy_queue::engine::{Engine, EngineHandle, COMMAND_BUFFER};
use deploy_queue::events::run_event_link;
use deploy_queue::node::NodeHttp;
use deploy_queue::record::QueueState;
use deploy_queue::store::QueueStore;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_request::DeployRequest;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct FakeNode {
    pub status: Arc<Mutex<Value>>,
    pub deploy_answer: Arc<Mutex<(StatusCode, Value)>>,
    pub deploy_status: Arc<Mutex<Value>>,
    received: Arc<Mutex<Vec<DeployRequest>>>,
    events: broadcast::Sender<String>,
}

impl FakeNode {
    pub fn new(tip: i64, lfb: i64) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            status: Arc::new(Mutex::new(status_body(tip, lfb))),
            deploy_answer: Arc::new(Mutex::new((StatusCode::OK, json!("Success!")))),
            deploy_status: Arc::new(Mutex::new(
                json!({"state":"Pending","rejection_count":0,"latest_block_hash":null}),
            )),
            received: Arc::new(Mutex::new(Vec::new())),
            events,
        }
    }

    pub fn received(&self) -> Vec<DeployRequest> { self.received.lock().unwrap().clone() }

    pub fn emit(&self, event: Value) { self.events.send(event.to_string()).unwrap(); }

    pub async fn serve(self) -> String {
        let router = Router::new()
            .route("/api/status", get(status))
            .route("/api/deploy", post(take_deploy))
            .route("/api/deploy-finalization-status/{sig}", get(deploy_status))
            .route("/ws/events", get(events))
            .with_state(self);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        format!("http://{}", address)
    }
}

fn status_body(tip: i64, lfb: i64) -> Value {
    json!({
        "shardId": "root",
        "minPhloPrice": "1",
        "lastFinalizedBlockNumber": lfb,
        "latestBlockNumber": tip,
        "deployLifespan": 50,
        "maxUserDeploysPerBlock": 4,
        "maxParentDepth": 25,
    })
}

async fn status(State(node): State<FakeNode>) -> Json<Value> {
    Json(node.status.lock().unwrap().clone())
}

async fn deploy_status(State(node): State<FakeNode>) -> Json<Value> {
    Json(node.deploy_status.lock().unwrap().clone())
}

async fn take_deploy(State(node): State<FakeNode>, Json(request): Json<DeployRequest>) -> Response {
    node.received.lock().unwrap().push(request);
    let (code, body) = node.deploy_answer.lock().unwrap().clone();
    (code, Json(body)).into_response()
}

async fn events(State(node): State<FakeNode>, upgrade: WebSocketUpgrade) -> Response {
    let events = node.events.subscribe();
    upgrade.on_upgrade(move |socket| forward(socket, events))
}

async fn forward(mut socket: WebSocket, mut events: broadcast::Receiver<String>) {
    while let Ok(event) = events.recv().await {
        if socket.send(Message::Text(event.into())).await.is_err() {
            return;
        }
    }
}

pub struct Queue {
    pub handle: EngineHandle,
    tasks: Vec<JoinHandle<()>>,
}

impl Queue {
    pub async fn start(node_url: &str, data_dir: &std::path::Path) -> Self {
        let node = NodeHttp::new(node_url).unwrap();
        let store = QueueStore::open(data_dir).unwrap();
        let (commands, receiver) = mpsc::channel(COMMAND_BUFFER);
        let engine = Engine::new(store, node.clone(), commands.clone()).unwrap();
        let tasks = vec![
            tokio::spawn(async move { engine.run(receiver).await.unwrap() }),
            tokio::spawn(run_event_link(node, commands.clone())),
        ];
        let queue = Self {
            handle: EngineHandle::new(commands),
            tasks,
        };
        eventually("the queue connects", || async {
            queue.handle.snapshot().await.unwrap().connected
        })
        .await;
        queue
    }

    pub async fn state(&self, sig: &[u8]) -> Option<QueueState> {
        self.handle
            .status(sig.to_vec())
            .await
            .unwrap()
            .map(|view| view.state)
    }

    pub fn stop(self) {
        for task in self.tasks {
            task.abort();
        }
    }
}

pub async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..250 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting until {}", what);
}

pub fn deploy(valid_after: i64, shard_id: &str, seed: u8) -> Signed<DeployData> {
    Signed::create(
        DeployData {
            term: "new x in { x!(1) }".to_string(),
            time_stamp: i64::from(seed),
            phlo_price: 1.into(),
            phlo_limit: 100_000,
            valid_after_block_number: valid_after,
            shard_id: shard_id.to_string(),
            expiration_timestamp: None,
            parameters: Vec::new(),
        },
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[seed; 32]),
    )
    .unwrap()
}

pub fn finalized_verdict(sig: &[u8], block: i64) -> Value {
    json!({
        "event": "deploy-verdicts",
        "schema-version": 1,
        "payload": {"verdicts": [{"deploy-id": hex::encode(sig), "state": "finalized",
                    "rejection-count": 0, "block-hash": format!("{:064x}", block)}]}
    })
}
