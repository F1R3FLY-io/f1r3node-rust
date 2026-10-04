use std::sync::Arc;

use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConfCell;
use shared::rust::shared::f1r3fly_events::{EventStream, StartupBuffer};

use crate::rust::api::grpc_package::{acquire_external_server, acquire_internal_server};
use crate::rust::configuration::NodeConf;
use crate::rust::web::routes::Routes;
use crate::rust::web::shared_handlers::AppState;

pub enum PreparedApplication {
    CbcCasper(Box<crate::rust::consensus::casper::api_compat::PreparedApplication>),
    Cordial(Box<crate::rust::consensus::cordial::PreparedApplication>),
}

pub struct ApplicationRoutes {
    pub external: tonic::transport::server::Router,
    pub internal: tonic::transport::server::Router,
    pub public_http: axum::Router,
    pub admin_http: axum::Router,
}

impl PreparedApplication {
    pub async fn routes(
        self,
        conf: &NodeConf,
        peer_conf: RPConfCell,
        connections: ConnectionsCell,
        discovery: Arc<dyn NodeDiscovery + Send + Sync>,
        events: EventStream,
        startup_events: StartupBuffer,
    ) -> eyre::Result<ApplicationRoutes> {
        match self {
            Self::Cordial(application) => application.routes(conf),
            Self::CbcCasper(application) => {
                let crate::rust::consensus::casper::api_compat::PreparedApplication {
                    api_servers,
                    reporting_routes,
                    web_api,
                    admin_web_api,
                    block_report_api,
                } = *application;
                drop(reporting_routes);
                let settings = &conf.api_server;
                let external = acquire_external_server(
                    api_servers.deploy.clone(),
                    settings.grpc_max_recv_message_size as usize,
                    settings.keep_alive_time,
                    settings.keep_alive_timeout,
                    settings.tcp_keepalive_time,
                    settings.request_timeout,
                    settings.max_connection_age,
                    settings.max_connection_age_grace,
                )
                .map_err(|error| eyre::eyre!("Failed to acquire external API server: {error}"))?;
                let internal = acquire_internal_server(
                    api_servers.repl,
                    api_servers.deploy,
                    api_servers.propose,
                    api_servers.lsp,
                    settings.grpc_max_recv_message_size as usize,
                    settings.keep_alive_time,
                    settings.keep_alive_timeout,
                    settings.tcp_keepalive_time,
                    settings.request_timeout,
                    settings.max_connection_age,
                    settings.max_connection_age_grace,
                )
                .await
                .map_err(|error| eyre::eyre!("Failed to acquire internal API server: {error}"))?;
                let state = AppState::new(
                    admin_web_api,
                    web_api,
                    block_report_api,
                    peer_conf,
                    Arc::new(connections),
                    discovery,
                    Arc::new(events.new_subscribe()),
                    startup_events,
                );
                let public_http = Routes::create_main_routes(
                    settings.enable_reporting,
                    settings.http_max_body_bytes as usize,
                )
                .with_state(state.clone());
                let admin_http = Routes::create_admin_routes(settings.http_max_body_bytes as usize)
                    .with_state(state);
                Ok(ApplicationRoutes {
                    external,
                    internal,
                    public_http,
                    admin_http,
                })
            }
        }
    }
}
