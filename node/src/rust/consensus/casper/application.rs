use std::sync::Arc;

use super::api_compat::PreparedApplication;
use crate::rust::api::grpc_package::{acquire_external_server, acquire_internal_server};
use crate::rust::runtime::application::{
    ApplicationContext, ApplicationProvider, ApplicationRoutes,
};
use crate::rust::web::routes::Routes;
use crate::rust::web::shared_handlers::AppState;

#[async_trait::async_trait]
impl ApplicationProvider for PreparedApplication {
    async fn routes(
        self: Box<Self>,
        context: ApplicationContext,
    ) -> eyre::Result<ApplicationRoutes> {
        let PreparedApplication {
            api_servers,
            reporting_routes,
            web_api,
            admin_web_api,
            block_report_api,
        } = *self;
        drop(reporting_routes);
        let settings = &context.settings;
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
            context.peer_conf,
            Arc::new(context.connections),
            context.discovery,
            Arc::new(context.events.new_subscribe()),
            context.startup_events,
        );
        let public_http = Routes::create_main_routes(
            settings.enable_reporting,
            settings.http_max_body_bytes as usize,
        )
        .with_state(state.clone());
        let admin_http =
            Routes::create_admin_routes(settings.http_max_body_bytes as usize).with_state(state);
        Ok(ApplicationRoutes {
            external,
            internal,
            public_http,
            admin_http,
        })
    }
}
