//! Added by DR-134 (DR-66 item 5): this node executes and replays only
//! offered-funded user deploys. A body-only user deploy and a genesis that the
//! retired legacy meter produced are historical execution. Each returns an
//! explicit compatibility error before any state reset, and a fresh protocol-6
//! genesis is required.

use models::rust::casper::protocol::casper_message::ProcessedDeploy;

use crate::rust::errors::CasperError;

pub const BODY_ONLY_DEPLOYS_UNSUPPORTED: &str =
    "body-only deploys are not supported: protocol 6 executes only offered-funded deploys, so a fresh protocol-6 genesis is required (DR-66 item 5)";

pub const LEGACY_METERED_GENESIS_UNSUPPORTED: &str =
    "a genesis that the legacy meter produced is not supported: protocol-6 genesis runs in the trusted install scope and records cost 0, so a fresh protocol-6 genesis is required (DR-66 item 5)";

pub const PROTOCOL_6_GENESIS_REQUIRED: &str =
    "this node requires a protocol-6 genesis with the active offered-funded resource policy (DR-66 item 5); the genesis has none";

pub fn protocol_6_genesis_required() -> CasperError {
    CasperError::RuntimeError(PROTOCOL_6_GENESIS_REQUIRED.to_string())
}

pub fn body_only_deploys_unsupported() -> CasperError {
    CasperError::RuntimeError(BODY_ONLY_DEPLOYS_UNSUPPORTED.to_string())
}

pub fn legacy_metered_genesis_unsupported() -> CasperError {
    CasperError::RuntimeError(LEGACY_METERED_GENESIS_UNSUPPORTED.to_string())
}

/// A genesis deploy that records a nonzero cost ran under the legacy meter.
pub fn is_legacy_metered_genesis(deploys: &[ProcessedDeploy]) -> bool {
    deploys.iter().any(|deploy| deploy.cost.cost != 0)
}

#[cfg(test)]
mod tests {
    use models::rhoapi::PCost;

    use super::*;

    fn genesis_deploy(cost: u64) -> ProcessedDeploy {
        let mut deploy = ProcessedDeploy::empty(
            crate::rust::util::construct_deploy::basic_deploy_data(0, None, None)
                .expect("a genesis-format deploy"),
        );
        deploy.cost = PCost { cost };
        deploy
    }

    #[test]
    fn a_trusted_install_genesis_records_cost_zero_and_is_supported() {
        assert!(!is_legacy_metered_genesis(&[
            genesis_deploy(0),
            genesis_deploy(0)
        ]));
        assert!(!is_legacy_metered_genesis(&[]));
    }

    #[test]
    fn any_nonzero_genesis_cost_marks_a_legacy_metered_genesis() {
        assert!(is_legacy_metered_genesis(&[
            genesis_deploy(0),
            genesis_deploy(1)
        ]));
        assert!(is_legacy_metered_genesis(&[genesis_deploy(200_245)]));
    }
}
