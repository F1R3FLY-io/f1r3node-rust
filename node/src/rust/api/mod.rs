pub mod admin_web_api;
pub mod deploy_grpc_service_v1;
pub mod grpc_package;
pub mod lsp_grpc_service;
pub mod propose_grpc_service_v1;
pub mod repl_grpc_service;
pub mod serde_types;
pub mod web_api;

pub(crate) fn effective_readiness(running: bool, last_finalized_block_number: i64) -> bool {
    running && last_finalized_block_number >= 0
}

#[cfg(test)]
mod tests {
    use super::effective_readiness;

    #[test]
    fn readiness_requires_running_casper_with_finalized_state() {
        assert!(effective_readiness(true, 0));
        assert!(effective_readiness(true, 42));
        assert!(!effective_readiness(true, -1));
        assert!(!effective_readiness(false, 42));
    }
}
