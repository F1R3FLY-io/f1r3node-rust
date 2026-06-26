//! OpenAPI documentation generation for F1re3fly Web API

use utoipa::OpenApi;

use crate::rust::web::{
    admin_web_api_routes, reporting_routes, shared_handlers, status_info, web_api_routes,
};

/// Public API OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        status_info::status_info_handler,
        shared_handlers::deploy_handler,
        shared_handlers::explore_deploy_handler,
        shared_handlers::explore_deploy_by_block_hash_handler,
        shared_handlers::get_blocks_handler,
        shared_handlers::get_block_handler,
        web_api_routes::prepare_deploy_get_handler,
        web_api_routes::prepare_deploy_post_handler,
        web_api_routes::data_at_name_by_block_hash_handler,
        web_api_routes::last_finalized_block_handler,
        web_api_routes::get_blocks_by_heights_handler,
        web_api_routes::get_blocks_by_depth_handler,
        web_api_routes::find_deploy_handler,
        web_api_routes::is_finalized_handler,
        web_api_routes::deploy_finalization_status_handler,
        web_api_routes::balance_handler,
        web_api_routes::registry_handler,
        web_api_routes::validators_handler,
        web_api_routes::validator_handler,
        web_api_routes::bond_status_handler,
        web_api_routes::epoch_handler,
        web_api_routes::epoch_rewards_handler,
        web_api_routes::estimate_cost_handler,
        reporting_routes::trace_handler,
    ),
    info(
        title = "F1r3fly Node API",
        version = "1.0",
        description = "Public API for F1r3fly Node - a Casper blockchain node"
    )
)]
pub struct PublicApi;

/// Admin API OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        status_info::status_info_handler,
        shared_handlers::deploy_handler,
        shared_handlers::explore_deploy_handler,
        shared_handlers::explore_deploy_by_block_hash_handler,
        shared_handlers::get_blocks_handler,
        shared_handlers::get_block_handler,
        web_api_routes::prepare_deploy_get_handler,
        web_api_routes::prepare_deploy_post_handler,
        web_api_routes::data_at_name_by_block_hash_handler,
        web_api_routes::last_finalized_block_handler,
        web_api_routes::get_blocks_by_heights_handler,
        web_api_routes::get_blocks_by_depth_handler,
        web_api_routes::find_deploy_handler,
        web_api_routes::is_finalized_handler,
        web_api_routes::deploy_finalization_status_handler,
        web_api_routes::balance_handler,
        web_api_routes::registry_handler,
        web_api_routes::validators_handler,
        web_api_routes::validator_handler,
        web_api_routes::bond_status_handler,
        web_api_routes::epoch_handler,
        web_api_routes::epoch_rewards_handler,
        web_api_routes::estimate_cost_handler,
        reporting_routes::trace_handler,
        admin_web_api_routes::propose_handler,
    ),
    info(
        title = "F1r3fly Node API (admin)",
        version = "1.0",
        description = "Admin API for F1r3fly Node - includes all public endpoints plus administrative functions"
    )
)]
pub struct AdminApi;
