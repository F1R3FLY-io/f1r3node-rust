//! DR-120 (C9): the evidence predicate P2 of the fast path.

use models::rust::casper::protocol::casper_message::{BlockMessage, ProcessedUserDeploy};
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::native_cost_evidence::NativeFundingCaseV1;

use crate::rust::errors::CasperError;

/// A committed funding case records the cohort's next fee cursor exactly
/// when the outcome pays a fee (funding_family_cursor.rs:580-608), and every
/// accepted offer pays a fee of 1 (acceptance.rs:22,414).
pub(crate) fn has_fee_transition(funding: &NativeFundingCaseV1<'_>) -> bool {
    funding.fee_next_cursor.is_some()
}

/// P2: every user deploy of `block` is offered, and its committed funding
/// case records a fee-cursor transition (native_cost_evidence/sections.rs:
/// 40-50). Its settlement then moves its cohort's fee cursor
/// (settlement.rs:179-199), which the cursor-linearity argument L7 needs. A
/// legacy user deploy cannot be in a v6 scope (validation_dispatcher.rs:
/// 76-82,124-130), so it is an error.
pub(crate) fn every_deploy_has_fee_transition(block: &BlockMessage) -> Result<bool, CasperError> {
    let protocol = offered_funded_v6_limits();
    for deploy in &block.body.deploys {
        match deploy {
            ProcessedUserDeploy::Legacy(_) => {
                return Err(CasperError::RuntimeError(format!(
                    "v6 merge scope block {} holds a legacy user deploy",
                    hex::encode(&block.block_hash[..8.min(block.block_hash.len())])
                )));
            }
            ProcessedUserDeploy::Offered(offered) => {
                let evidence = offered
                    .evidence(protocol.evidence)
                    .map_err(CasperError::RuntimeError)?;
                let (funding, _) = evidence
                    .decoded_sections(protocol.funding_case, protocol.prepaid_delta)
                    .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
                if !has_fee_transition(&funding) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}
