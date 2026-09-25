use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use rholang::rust::interpreter::accounting::economic_failure::classify_errors;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloAcquisitionLimits, NativePhloPurseLimits, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, PhloFailure, PhloOutcome, PhloResourceAmount,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;

use super::NativeFundedAttempt;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::direct_wallet_funding::CheckedDirectWalletSettlement;
use crate::rust::util::rholang::costacc::prepaid_receipts::{
    NativeMeasuredSettlementLimits, NativePrepaidDemandInput, NativePrepaidDemandLimits,
    NativePrepaidInventory, PrepaidStackPop,
};

pub struct NativeAttemptSettlementInput<'r, 's, 'p> {
    pub inventory: &'r NativePrepaidInventory<'s, 'p>,
    pub schedule: &'r PhloScheduleBinding<'r>,
    pub terms: &'r [u8],
    pub draws: &'r [PrepaidStackPop],
    pub demand_positions: &'r [usize],
    pub retained: &'r [PhloResourceAmount<'r>],
}

#[derive(Clone, Copy, Debug)]
pub struct NativeAttemptSettlementLimits {
    pub observations: usize,
    pub regions: NativePhloRegionLimits,
    pub purses: NativePhloPurseLimits,
    pub acquisition: NativePhloAcquisitionLimits,
    pub demand: NativePrepaidDemandLimits,
    pub settlement: NativeMeasuredSettlementLimits,
}

fn invalid(error: impl std::fmt::Display) -> CasperError {
    CasperError::InvalidCostSettlement(error.to_string())
}

fn check_metered_usage(
    complete: bool,
    observed: Option<u64>,
    derived: u64,
) -> Result<(), CasperError> {
    if !complete || observed != Some(derived) {
        return Err(invalid(
            "native settlement requires complete observations and exact metered usage",
        ));
    }
    Ok(())
}

impl<'a> NativeFundedAttempt<'_, 'a> {
    pub fn capture_measured_settlement(
        &self,
        input: NativeAttemptSettlementInput<'_, '_, '_>,
        limits: NativeAttemptSettlementLimits,
        budget: &HostWorkBudget,
    ) -> Result<CheckedDirectWalletSettlement<'a, OfferedFundedDeploy>, CasperError> {
        let controls = self
            .policy
            .policy()
            .policy()
            .signed_intent()
            .intent()
            .bound()
            .consent()
            .family()
            .cases()[0]
            .obligations
            .execution()
            .controls();
        self.adopted
            .bind_execution_contract(controls, input.schedule)?;
        let measured = self
            .adopted
            .native_rules()
            .measure(&self.evaluation.byte_observations, limits.observations)
            .map_err(invalid)?;
        let regions = measured
            .region_demands(limits.regions, budget)
            .map_err(invalid)?;
        let located = regions
            .locate_purses(limits.purses, budget)
            .map_err(invalid)?;
        let demand = located
            .prepare_acquisition_demand(input.schedule, input.terms, limits.acquisition, budget)
            .map_err(invalid)?;
        let binding = input.inventory.bind_measured_demand(
            NativePrepaidDemandInput {
                expected_root: self.policy.snapshot().wallets().pre_state_root(),
                controls,
                demand: &demand,
                draws: input.draws,
                demand_positions: input.demand_positions,
            },
            limits.demand,
            budget,
        )?;
        let observed = check_counted_phlo_execution(
            controls,
            binding.witness(),
            limits.settlement.matching.execution,
        )
        .map_err(invalid)?
        .with_retained_acquisitions(input.retained, budget)
        .map_err(invalid)?;
        check_metered_usage(
            self.evaluation
                .byte_observations
                .has_complete_measurements(),
            self.evaluation.native_phlo_usage,
            observed.usage(),
        )?;
        let summary = self
            .evaluation
            .economic_failures
            .union(classify_errors(&self.evaluation.errors, Some(budget)).map_err(invalid)?);
        let mut failures = [PhloFailure::User; 4];
        let mut count = 0;
        for failure in [
            PhloFailure::User,
            PhloFailure::Platform,
            PhloFailure::Certificate,
            PhloFailure::Unclassified,
        ] {
            if summary.contains(failure) {
                failures[count] = failure;
                count += 1;
            }
        }
        self.policy
            .capture_settlement(
                observed,
                PhloOutcome::Accepted(&failures[..count]),
                limits.settlement.matching,
                limits.settlement.capture,
                budget,
            )
            .map(|settlement| settlement.bind_execution_root(self.execution_root))
            .map_err(invalid)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn metered_settlement_guard_refines_complete_exact_observation(
            complete in any::<bool>(), present in any::<bool>(),
            used in any::<u64>(), other in any::<u64>(), keep in any::<bool>(),
        ) {
            let derived = if keep { used } else { other };
            prop_assert_eq!(
                check_metered_usage(complete, present.then_some(used), derived).is_ok(),
                complete && present && used == derived,
            );
        }
    }
}
