use rholang::rust::interpreter::accounting::authority::{
    monetary_funding_signatures_with_host_work, AuthorityBalanceSettlement,
};
use rholang::rust::interpreter::accounting::monetary_allocation::{
    MonetaryAllocationError, MonetaryCohort, MonetaryCohortError, MonetaryFeeEvidence,
};

use super::*;

pub(crate) fn native_fee_policy_context() -> [u8; 32] {
    Blake2b256::hash(
        b"f1r3node:monetary-fee:rotating-capped-max-min:v1:SystemVault:General".to_vec(),
    )
    .try_into()
    .expect("Blake2b-256 digest length")
}

pub(crate) fn fee_cohort(
    deploy: &Cosigned<DeployData>,
    inventory: &AuthorityPhysicalInventory<SigKey>,
    host_work: Option<&HostWorkBudget>,
) -> Result<MonetaryCohort, CasperError> {
    let eligible = monetary_funding_signatures_with_host_work(
        &fee_authority_event(deploy)?,
        &deploy.data().authority_presentations,
        host_work,
    )
    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let cap = NonZeroUsize::new(eligible.len()).ok_or_else(|| {
        CasperError::InvalidCostSettlement("monetary fee requires an authorized payer".to_string())
    })?;
    MonetaryCohort::from_inventory(&eligible, inventory, cap)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))
}

pub(crate) type PlannedMonetaryFee = (AuthorityBalanceSettlement, MonetaryFeeEvidence);

pub(crate) fn plan_fee_from_cursor(
    cohort: &MonetaryCohort,
    available: &ResourceMultiset<SigKey>,
    cursor: MonetaryCursor,
) -> Result<Option<PlannedMonetaryFee>, CasperError> {
    let context = native_fee_policy_context();
    let plan = match cohort.plan(available, 1, &context, cursor) {
        Ok(plan) => plan,
        Err(MonetaryCohortError::Allocation(MonetaryAllocationError::InsufficientCapacity)) => {
            return Ok(None);
        }
        Err(error) => return Err(CasperError::InvalidCostSettlement(error.to_string())),
    };
    let evidence = MonetaryFeeEvidence::from_plan(cohort, &plan, context, 1)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    Ok(Some((plan.into_parts().0, evidence)))
}

pub(crate) async fn plan_monetary_fee(
    deploy: &Cosigned<DeployData>,
    inventory: &AuthorityPhysicalInventory<SigKey>,
    available: &ResourceMultiset<SigKey>,
    supply_reader: &dyn SupplyReader,
    accepted_cursors: &BTreeMap<SigKey, MonetaryCursor>,
    host_work: Option<&HostWorkBudget>,
) -> Result<Option<PlannedMonetaryFee>, CasperError> {
    let cohort = fee_cohort(deploy, inventory, host_work)?;
    let scope = cohort.scope_id(&native_fee_policy_context());
    let cursor = match accepted_cursors.get(&scope) {
        Some(cursor) => *cursor,
        None => supply_reader
            .read_monetary_cursor(scope, NonZeroUsize::new(cohort.payers().len()).unwrap())
            .await?
            .unwrap_or(MonetaryCursor::INITIAL),
    };
    plan_fee_from_cursor(&cohort, available, cursor)
}

pub(crate) fn required_fee_plan(
    certificate: &FundingCertificate<SigKey>,
) -> Result<&MonetaryFeeEvidence, CasperError> {
    let evidence = certificate.fee_plan.as_ref().ok_or_else(|| {
        CasperError::InvalidCostSettlement(
            "funding certificate is missing its monetary fee plan".to_string(),
        )
    })?;
    if certificate.protocol_version != AUTHORITY_ACCOUNTING_PROTOCOL_VERSION
        || evidence.fields().policy_context != native_fee_policy_context()
        || evidence.fields().obligation != 1
        || certificate
            .fee_allocation
            .0
            .values()
            .map(|amount| u128::from(*amount))
            .sum::<u128>()
            != 1
    {
        return Err(CasperError::InvalidCostSettlement(
            "funding certificate monetary fee policy is invalid".to_string(),
        ));
    }
    Ok(evidence)
}
