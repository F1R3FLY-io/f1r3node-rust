//! The queue's client APIs, over real HTTP, WebSocket and gRPC connections,
//! answering in the node's own shapes.

mod common;

use std::time::Duration;

use common::{deploy, eventually, finalized_verdict, FakeNode, Queue};
use deploy_queue::grpc::QueueGrpc;
use deploy_queue::http::{router, HttpState};
use deploy_queue::node::NodeHttp;
use deploy_queue::record::QueueState;
use futures::StreamExt;
use metrics_exporter_prometheus::PrometheusBuilder;
use models::casper::v1::deploy_service_client::DeployServiceClient;
use models::casper::v1::deploy_service_server::DeployServiceServer;
use models::casper::v1::{deploy_finalization_status_response, deploy_response};
use models::casper::{DeployFinalizationStateProto, DeployFinalizationStatusQuery};
use models::deployqueue::v1::deploy_queue_client::DeployQueueClient;
use models::deployqueue::v1::deploy_queue_server::DeployQueueServer;
use models::deployqueue::v1::WatchDeploysQuery;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_request::DeployRequest;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use tonic::transport::server::TcpIncoming;
use tonic::transport::{Channel, Endpoint, Server};

struct Served {
    http: String,
    grpc: Channel,
}

async fn serve(queue: &Queue, node_url: &str) -> Served {
    let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_address = http.local_addr().unwrap();
    let state = HttpState {
        engine: queue.handle.clone(),
        node: NodeHttp::new(node_url).unwrap(),
        metrics: PrometheusBuilder::new().build_recorder().handle(),
    };
    tokio::spawn(async move { axum::serve(http, router(state)).await.unwrap() });

    let incoming = TcpIncoming::bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let grpc_address = incoming.local_addr().unwrap();
    // The stand-in node has no gRPC API; nothing these tests send reaches it.
    let node_channel = Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
    let grpc = QueueGrpc::new(queue.handle.clone(), node_channel);
    tokio::spawn(async move {
        Server::builder()
            .add_service(DeployServiceServer::new(grpc.clone()))
            .add_service(DeployQueueServer::new(grpc))
            .serve_with_incoming(incoming)
            .await
            .unwrap()
    });
    let channel = Endpoint::from_shared(format!("http://{}", grpc_address))
        .unwrap()
        .connect()
        .await
        .unwrap();
    Served {
        http: format!("http://{}", http_address),
        grpc: channel,
    }
}

async fn post_deploy(served: &Served, body: String) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/api/deploy", served.http))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap()
}

fn request_body(deploy: &crypto::rust::signatures::signed::Signed<DeployData>) -> String {
    serde_json::to_string(&DeployRequest::from_signed(deploy)).unwrap()
}

async fn get_json(url: String) -> (reqwest::StatusCode, Value) {
    let response = reqwest::get(url).await.unwrap();
    (response.status(), response.json().await.unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn http_intake_reports_the_queue_state_beside_the_node_status() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    let served = serve(&queue, &url).await;

    let held = deploy(5, "root", 1);
    let sig = hex::encode(held.sig.as_ref());
    let response = post_deploy(&served, request_body(&held)).await;
    assert_eq!(response.status(), 200);
    let answer: String = response.json().await.unwrap();
    assert_eq!(answer, format!("Success!\nDeployId is: {}", sig));
    eventually("the node receives the deploy", || async {
        node.received().len() == 1
    })
    .await;

    let (code, status) = get_json(format!(
        "{}/api/deploy-finalization-status/{}",
        served.http, sig
    ))
    .await;
    assert_eq!(code, 200);
    assert_eq!(status["state"], "Pending");
    assert_eq!(status["queue_state"], "submitted");

    let (code, snapshot) = get_json(format!("{}/queue/status", served.http)).await;
    assert_eq!(code, 200);
    assert_eq!(snapshot["connected"], true);
    assert_eq!(snapshot["submitted"], 1);

    let metrics = reqwest::get(format!("{}/queue/metrics", served.http))
        .await
        .unwrap();
    assert_eq!(metrics.status(), 200);
    assert!(metrics.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn http_refusals_keep_the_node_error_envelope() {
    let node = FakeNode::new(100, 99);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    let served = serve(&queue, &url).await;

    let refusal = |response: reqwest::Response| async move {
        let code = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .map(|value| value.to_str().unwrap().to_string());
        let body: Value = response.json().await.unwrap();
        (
            code,
            body["error"].as_str().unwrap().to_string(),
            retry_after,
        )
    };

    let (code, kind, _) = refusal(post_deploy(&served, "{".to_string()).await).await;
    assert_eq!(
        (code.as_u16(), kind.as_str()),
        (400, "invalid_request_body")
    );

    let other_shard = request_body(&deploy(90, "other", 3));
    let (code, kind, _) = refusal(post_deploy(&served, other_shard).await).await;
    assert_eq!((code.as_u16(), kind.as_str()), (400, "illegal_argument"));

    let held = request_body(&deploy(95, "root", 5));
    assert_eq!(post_deploy(&served, held.clone()).await.status(), 200);
    let (code, kind, _) = refusal(post_deploy(&served, held).await).await;
    assert_eq!((code.as_u16(), kind.as_str()), (409, "duplicate_deploy"));

    // Open through block 109 at lifespan 50, but landing from tip 100 needs a
    // height of inclusion plus the 25-height margin.
    let too_late = request_body(&deploy(60, "root", 6));
    let (code, kind, retry_after) = refusal(post_deploy(&served, too_late).await).await;
    assert_eq!((code.as_u16(), kind.as_str()), (503, "deploy_queue_full"));
    assert!(retry_after.unwrap().parse::<u64>().is_ok());
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn http_forwards_what_the_queue_does_not_answer() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    let served = serve(&queue, &url).await;

    let (code, status) = get_json(format!("{}/api/status", served.http)).await;
    assert_eq!(code, 200);
    assert_eq!(status["shardId"], "root");

    let unknown = hex::encode([7u8; 64]);
    let (code, status) = get_json(format!(
        "{}/api/deploy-finalization-status/{}",
        served.http, unknown
    ))
    .await;
    assert_eq!(code, 200);
    assert_eq!(status["state"], "Pending");
    assert!(
        status.get("queue_state").is_none(),
        "the node answered, not the queue"
    );

    let (code, error) = get_json(format!(
        "{}/api/deploy-finalization-status/not-hex",
        served.http
    ))
    .await;
    assert_eq!(
        (code.as_u16(), &error["error"]),
        (400, &json!("invalid_hash"))
    );
    queue.stop();
}

async fn next_text<S>(socket: &mut S) -> Value
where S: futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("a message within 5s")
        .expect("the socket is open")
        .unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn websockets_follow_deploys_and_relay_node_events() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    let served = serve(&queue, &url).await;
    let ws = served.http.replace("http://", "ws://");

    let held = deploy(5, "root", 2);
    let sig = held.sig.to_vec();
    assert_eq!(
        post_deploy(&served, request_body(&held)).await.status(),
        200
    );
    eventually("the deploy is submitted", || async {
        queue.state(&sig).await == Some(QueueState::Submitted)
    })
    .await;

    let (mut watch, _) =
        tokio_tungstenite::connect_async(format!("{}/ws/deploys?sigs={}", ws, hex::encode(&sig)))
            .await
            .unwrap();
    let first = next_text(&mut watch).await;
    assert_eq!(first["deploy_id"], hex::encode(&sig));
    assert_eq!(first["status"]["queue_state"], "submitted");
    node.emit(finalized_verdict(&sig, 11));
    let verdict = next_text(&mut watch).await;
    assert_eq!(verdict["status"]["state"], "Finalized");
    let closing = tokio::time::timeout(Duration::from_secs(5), watch.next())
        .await
        .unwrap();
    assert!(
        matches!(closing, Some(Ok(Message::Close(_))) | None),
        "the watch closes once every named deploy is terminal"
    );

    let (mut relay, _) = tokio_tungstenite::connect_async(format!("{}/ws/events", ws))
        .await
        .unwrap();
    let event = json!({"event": "block-finalised", "payload": {"block-number": 12}});
    let mut relayed = None;
    for _ in 0..50 {
        node.emit(event.clone());
        if let Ok(Some(Ok(message))) =
            tokio::time::timeout(Duration::from_millis(100), relay.next()).await
        {
            relayed = Some(serde_json::from_str::<Value>(message.to_text().unwrap()).unwrap());
            break;
        }
    }
    assert_eq!(relayed, Some(event));
    relay.close(None).await.unwrap();

    let refused = tokio_tungstenite::connect_async(format!("{}/ws/deploys?sigs=not-hex", ws)).await;
    assert!(matches!(
        refused,
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status() == 400
    ));
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_intake_answers_as_the_node_does() {
    let node = FakeNode::new(100, 99);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    let served = serve(&queue, &url).await;
    let mut client = DeployServiceClient::new(served.grpc.clone());

    let held = deploy(95, "root", 8);
    let answer = client
        .do_deploy(DeployData::to_proto_ref(&held))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        answer.message,
        Some(deploy_response::Message::Result(format!(
            "Success!\nDeployId is: {}",
            hex::encode(held.sig.as_ref())
        )))
    );

    let duplicate = client
        .do_deploy(DeployData::to_proto_ref(&held))
        .await
        .unwrap()
        .into_inner();
    assert!(
        matches!(duplicate.message, Some(deploy_response::Message::Error(_))),
        "a refusal the node would also make is a ServiceError"
    );

    let mut forged = DeployData::to_proto_ref(&deploy(95, "root", 9));
    forged.sig = vec![0; 64].into();
    let forged = client.do_deploy(forged).await.unwrap().into_inner();
    assert!(matches!(
        forged.message,
        Some(deploy_response::Message::Error(_))
    ));

    let full = client
        .do_deploy(DeployData::to_proto_ref(&deploy(60, "root", 10)))
        .await
        .unwrap_err();
    assert_eq!(full.code(), tonic::Code::ResourceExhausted);
    assert!(full
        .metadata()
        .get("retry-after")
        .unwrap()
        .to_str()
        .unwrap()
        .parse::<u64>()
        .is_ok());

    let status = client
        .deploy_finalization_status(DeployFinalizationStatusQuery {
            deploy_sig: held.sig.clone(),
        })
        .await
        .unwrap()
        .into_inner();
    let Some(deploy_finalization_status_response::Message::Status(info)) = status.message else {
        panic!("a held deploy has a status");
    };
    assert_eq!(
        info.state,
        DeployFinalizationStateProto::DeployStatePending as i32
    );
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_watch_streams_until_the_verdict() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    let served = serve(&queue, &url).await;
    let mut deploys = DeployServiceClient::new(served.grpc.clone());
    let mut watcher = DeployQueueClient::new(served.grpc.clone());

    let held = deploy(5, "root", 11);
    let sig = held.sig.to_vec();
    deploys
        .do_deploy(DeployData::to_proto_ref(&held))
        .await
        .unwrap();
    eventually("the deploy is submitted", || async {
        queue.state(&sig).await == Some(QueueState::Submitted)
    })
    .await;

    let mut stream = watcher
        .watch_deploys(WatchDeploysQuery {
            deploy_sigs: vec![sig.clone()],
        })
        .await
        .unwrap()
        .into_inner();
    let first = stream.message().await.unwrap().unwrap();
    assert_eq!(
        (first.deploy_sig.clone(), first.queue_state.as_str()),
        (sig.clone(), "submitted")
    );
    node.emit(finalized_verdict(&sig, 11));
    let verdict = stream.message().await.unwrap().unwrap();
    assert_eq!(verdict.state, "Finalized");
    assert!(
        stream.message().await.unwrap().is_none(),
        "the stream ends once every named deploy is terminal"
    );

    let empty = watcher
        .watch_deploys(WatchDeploysQuery {
            deploy_sigs: Vec::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(empty.code(), tonic::Code::InvalidArgument);
    queue.stop();
}
