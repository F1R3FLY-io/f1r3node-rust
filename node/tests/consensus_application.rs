use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::errors::CommError;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::{RPConf, RPConfCell};
use node::rust::configuration::NodeConf;
use node::rust::runtime::application::{
    ApplicationContext, ApplicationProvider, ApplicationRoutes, PreparedApplication,
};
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use tower::ServiceExt;

struct Discovery;

#[async_trait::async_trait]
impl NodeDiscovery for Discovery {
    async fn discover(&self) -> Result<(), CommError> { Ok(()) }
    fn peers(&self) -> Result<Vec<PeerNode>, CommError> { Ok(vec![]) }
    fn remove_peer(&self, _: &PeerNode) -> Result<(), CommError> { Ok(()) }
}

fn context() -> ApplicationContext {
    let conf: NodeConf = hocon::HoconLoader::new()
        .load_str(include_str!("../src/main/resources/defaults.conf"))
        .unwrap()
        .resolve()
        .unwrap();
    let events = F1r3flyEvents::new();
    ApplicationContext {
        settings: conf.api_server,
        peer_conf: RPConfCell::new(RPConf::new(
            PeerNode {
                id: NodeIdentifier {
                    key: vec![1].into(),
                },
                endpoint: Endpoint::new("127.0.0.1".into(), 1, 2),
            },
            "application-test".into(),
            None,
            Duration::from_secs(1),
            10,
            10,
        )),
        connections: ConnectionsCell::new(),
        discovery: Arc::new(Discovery),
        events: events.consume(),
        startup_events: events.startup_buffer(),
    }
}

struct IndependentApplication(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl ApplicationProvider for IndependentApplication {
    async fn routes(self: Box<Self>, _: ApplicationContext) -> eyre::Result<ApplicationRoutes> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ApplicationRoutes {
            external: tonic::transport::Server::builder()
                .add_routes(tonic::service::Routes::default()),
            internal: tonic::transport::Server::builder()
                .add_routes(tonic::service::Routes::default()),
            public_http: axum::Router::new()
                .route("/independent", axum::routing::get(|| async { "ready" })),
            admin_http: axum::Router::new().route(
                "/independent-admin",
                axum::routing::post(|| async { "accepted" }),
            ),
        })
    }
}

#[tokio::test]
async fn host_accepts_routes_without_a_concrete_protocol_variant() {
    let builds = Arc::new(AtomicUsize::new(0));
    let application: PreparedApplication = Box::new(IndependentApplication(builds.clone()));
    let routes = application.routes(context()).await.unwrap();
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    let response = routes
        .public_http
        .oneshot(
            axum::http::Request::get("/independent")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap(),
        "ready"
    );
    let response = routes
        .admin_http
        .oneshot(
            axum::http::Request::post("/independent-admin")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
}

struct FailedApplication;

#[async_trait::async_trait]
impl ApplicationProvider for FailedApplication {
    async fn routes(self: Box<Self>, _: ApplicationContext) -> eyre::Result<ApplicationRoutes> {
        eyre::bail!("application preparation failed")
    }
}

#[tokio::test]
async fn application_preparation_failure_reaches_the_host() {
    let application: PreparedApplication = Box::new(FailedApplication);
    let error = application.routes(context()).await.err().unwrap();
    assert_eq!(error.to_string(), "application preparation failed");
}
