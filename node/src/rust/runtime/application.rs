use std::sync::Arc;

use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConfCell;
use shared::rust::shared::f1r3fly_events::{EventStream, StartupBuffer};

use crate::rust::configuration::model::ApiServer;

pub type PreparedApplication = Box<dyn ApplicationProvider>;

pub struct ApplicationContext {
    pub settings: ApiServer,
    pub peer_conf: RPConfCell,
    pub connections: ConnectionsCell,
    pub discovery: Arc<dyn NodeDiscovery + Send + Sync>,
    pub events: EventStream,
    pub startup_events: StartupBuffer,
}

pub struct ApplicationRoutes {
    pub external: tonic::transport::server::Router,
    pub internal: tonic::transport::server::Router,
    pub public_http: axum::Router,
    pub admin_http: axum::Router,
}

#[async_trait::async_trait]
pub trait ApplicationProvider: Send {
    async fn routes(
        self: Box<Self>,
        context: ApplicationContext,
    ) -> eyre::Result<ApplicationRoutes>;
}
