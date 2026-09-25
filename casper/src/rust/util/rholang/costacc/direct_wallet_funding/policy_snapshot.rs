use std::mem::size_of;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block::state_hash::StateHash;
use models::rust::host_work::{HostWorkDimension, HostWorkReservationError, HostWorkUnits};
use rholang::rust::interpreter::accounting::monetary_allocation::{
    canonical_funding_key_order, monetary_scope_for_custodies, FundingSearchError, MonetaryCursor,
    MonetaryCursorError,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    CheckedPhloControls, PhloFundingTerms, PhloSchedulePolicy, PhloSchedulePolicyMismatch,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    CheckedNativeSignedPhloFamilyPolicy, CheckedPhloFundingFamily, NativePhloPolicyError,
    PhloFamilyCursorSnapshot, PhloFamilyFundingLimits, PhloFundingIntentView, PhloFundingSource,
    PhloPolicyCaptureError, PhloScopedCursorSnapshot, SignedPhloConsentLimits,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;

use super::*;
use crate::rust::casper::CasperShardConf;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::acceptance::monetary_fee::native_fee_policy_context;
use crate::rust::util::rholang::acceptance::{RuntimeManagerSupplyReader, SupplyReader};
use crate::rust::util::rholang::costacc::genesis_resource_policy::{
    AdoptedResourcePolicy, GenesisResourcePolicy,
};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Debug)]
pub struct DirectWalletPolicySnapshot<'a, A = FundedDeploy> {
    wallets: DirectWalletSnapshot<'a, A>,
    canonical_custodies: Vec<&'a [u8]>,
    resource_scope: [u8; 32],
    fee_scope: [u8; 32],
    resource_cursor: Option<MonetaryCursor>,
    fee_cursor: Option<MonetaryCursor>,
}

#[derive(Debug)]
pub struct CheckedDirectWalletPolicy<'a, A = FundedDeploy> {
    snapshot: &'a DirectWalletPolicySnapshot<'a, A>,
    policy: CheckedNativeSignedPhloFamilyPolicy<'a, A>,
}

#[derive(Debug, Error)]
pub enum DirectWalletPolicySnapshotError {
    #[error(transparent)]
    SchedulePolicy(#[from] PhloSchedulePolicyMismatch),
    #[error("adopted phlo context has a negative minimum or version, or an empty shard")]
    InvalidChainContext,
    #[error("captured phlo minimum differs from the adopted chain minimum")]
    ChainMinimumMismatch,
    #[error("funding schedule version differs from the adopted protocol version")]
    ChainVersionMismatch,
    #[error("funding schedule shard differs from the adopted shard")]
    ChainShardMismatch,
    #[error("funding policy requires a nonempty canonical wallet cohort")]
    InvalidCohort,
    #[error("resource and fee cursors must use different storage scopes")]
    ScopeCollision,
    #[error("funding snapshot allocation failed")]
    AllocationFailed,
    #[error("funding snapshot work calculation overflowed")]
    Overflow,
    #[error(transparent)]
    Wallet(#[from] DirectWalletSnapshotError),
    #[error(transparent)]
    Binding(#[from] DirectWalletBindingError),
    #[error(transparent)]
    Policy(#[from] PhloPolicyCaptureError),
    #[error(transparent)]
    Native(#[from] NativePhloPolicyError),
    #[error(transparent)]
    Cursor(#[from] MonetaryCursorError),
    #[error(transparent)]
    Search(#[from] FundingSearchError),
    #[error(transparent)]
    HostWork(#[from] HostWorkReservationError),
    #[error(transparent)]
    Read(#[from] CasperError),
}

fn check_adopted_chain_context(
    controls: CheckedPhloControls<'_>,
    shard: &CasperShardConf,
) -> Result<(), DirectWalletPolicySnapshotError> {
    let minimum = u64::try_from(shard.min_phlo_price)
        .map_err(|_| DirectWalletPolicySnapshotError::InvalidChainContext)?;
    let version = u64::try_from(shard.casper_version)
        .map_err(|_| DirectWalletPolicySnapshotError::InvalidChainContext)?;
    if shard.shard_name.is_empty() {
        return Err(DirectWalletPolicySnapshotError::InvalidChainContext);
    }
    if controls.minimum_price() != minimum {
        return Err(DirectWalletPolicySnapshotError::ChainMinimumMismatch);
    }
    let environment = controls.schedule().environment;
    if environment.protocol_version != version {
        return Err(DirectWalletPolicySnapshotError::ChainVersionMismatch);
    }
    if environment.shard != shard.shard_name.as_bytes() {
        return Err(DirectWalletPolicySnapshotError::ChainShardMismatch);
    }
    Ok(())
}

fn check_genesis_minimum(
    genesis_minimum: u64,
    adopted: &CasperShardConf,
) -> Result<(), DirectWalletPolicySnapshotError> {
    let minimum = u64::try_from(adopted.min_phlo_price)
        .map_err(|_| DirectWalletPolicySnapshotError::InvalidChainContext)?;
    if minimum != genesis_minimum {
        return Err(DirectWalletPolicySnapshotError::ChainMinimumMismatch);
    }
    Ok(())
}

fn reserve(
    budget: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), DirectWalletPolicySnapshotError> {
    let amount = u64::try_from(amount).map_err(|_| DirectWalletPolicySnapshotError::Overflow)?;
    budget.reserve(dimension, HostWorkUnits::new(amount))?;
    Ok(())
}

fn resource_policy_context() -> [u8; 32] {
    Blake2b256::hash(
        b"f1r3node:monetary-resource:canonical-family-lexicographic-minimax:v1:SystemVault:General"
            .to_vec(),
    )
    .try_into()
    .expect("Blake2b-256 digest length")
}

fn check_root(
    reader: &dyn SupplyReader,
    expected: [u8; 32],
) -> Result<(), DirectWalletPolicySnapshotError> {
    if reader.pre_state_root() != expected {
        return Err(DirectWalletSnapshotError::WrongPreState.into());
    }
    Ok(())
}

async fn read_cursor_pair(
    reader: &dyn SupplyReader,
    expected: [u8; 32],
    scopes: [[u8; 32]; 2],
    count: NonZeroUsize,
    parallel: NonZeroUsize,
    budget: &HostWorkBudget,
) -> Result<[Option<MonetaryCursor>; 2], DirectWalletPolicySnapshotError> {
    check_root(reader, expected)?;
    if scopes[0] == scopes[1] {
        return Err(DirectWalletPolicySnapshotError::ScopeCollision);
    }
    reserve(budget, HostWorkDimension::SearchCandidates, 2)?;
    reserve(budget, HostWorkDimension::VerificationOperations, 2)?;
    let resource = reader.read_monetary_cursor(scopes[0], count);
    let fee = reader.read_monetary_cursor(scopes[1], count);
    let pair = if parallel.get() > 1 {
        let (resource, fee) = tokio::try_join!(resource, fee)?;
        [resource, fee]
    } else {
        [resource.await?, fee.await?]
    };
    check_root(reader, expected)?;
    for cursor in pair.iter().flatten() {
        cursor.validate(count)?;
    }
    Ok(pair)
}

impl<'a, A> DirectWalletFunding<'a, A> {
    pub async fn read_policy_snapshot(
        self,
        runtime_manager: &RuntimeManager,
        pre_state_hash: StateHash,
        parallel_reads: NonZeroUsize,
        budget: &HostWorkBudget,
    ) -> Result<DirectWalletPolicySnapshot<'a, A>, DirectWalletPolicySnapshotError> {
        let expected = pre_state_hash
            .as_ref()
            .try_into()
            .map_err(|_| DirectWalletSnapshotError::WrongPreState)?;
        let count = NonZeroUsize::new(self.payers.len())
            .ok_or(DirectWalletPolicySnapshotError::InvalidCohort)?;
        if count.get() != self.record.sources.len() {
            return Err(DirectWalletPolicySnapshotError::InvalidCohort);
        }
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            count
                .get()
                .checked_mul(2 * size_of::<&[u8]>() + size_of::<DirectWalletSource<'a>>())
                .ok_or(DirectWalletPolicySnapshotError::Overflow)?,
        )?;
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            count
                .get()
                .checked_mul(96)
                .and_then(|n| n.checked_add(512))
                .ok_or(DirectWalletPolicySnapshotError::Overflow)?,
        )?;
        reserve(budget, HostWorkDimension::SearchCandidates, count.get())?;
        let mut original = Vec::new();
        original
            .try_reserve_exact(count.get())
            .map_err(|_| DirectWalletPolicySnapshotError::AllocationFailed)?;
        original.extend(self.record.sources.iter().map(|source| source.custody()));
        let order = canonical_funding_key_order(&original, budget)?;
        let mut canonical_custodies = Vec::new();
        canonical_custodies
            .try_reserve_exact(count.get())
            .map_err(|_| DirectWalletPolicySnapshotError::AllocationFailed)?;
        for (index, key) in order.into_iter().zip(self.payers.keys()) {
            let custody = original[index];
            if custody != key.as_slice() {
                return Err(DirectWalletPolicySnapshotError::InvalidCohort);
            }
            canonical_custodies.push(custody);
        }
        let resource_scope =
            monetary_scope_for_custodies(&resource_policy_context(), self.payers.keys());
        let fee_scope =
            monetary_scope_for_custodies(&native_fee_policy_context(), self.payers.keys());
        if resource_scope == fee_scope {
            return Err(DirectWalletPolicySnapshotError::ScopeCollision);
        }
        let reader = RuntimeManagerSupplyReader {
            runtime_manager,
            pre_state_hash,
        };
        let wallets = self
            .read_snapshot(&reader, expected, parallel_reads)
            .await?;
        let [resource_cursor, fee_cursor] = read_cursor_pair(
            &reader,
            expected,
            [resource_scope, fee_scope],
            count,
            parallel_reads,
            budget,
        )
        .await?;
        Ok(DirectWalletPolicySnapshot {
            wallets,
            canonical_custodies,
            resource_scope,
            fee_scope,
            resource_cursor,
            fee_cursor,
        })
    }
}

impl<'a, A> DirectWalletPolicySnapshot<'a, A> {
    pub fn wallets(&self) -> &DirectWalletSnapshot<'a, A> { &self.wallets }
    pub fn canonical_custodies(&self) -> &[&'a [u8]] { &self.canonical_custodies }
    pub fn resource_scope(&self) -> [u8; 32] { self.resource_scope }
    pub fn fee_scope(&self) -> [u8; 32] { self.fee_scope }
    pub fn resource_cursor(&self) -> Option<MonetaryCursor> { self.resource_cursor }
    pub fn fee_cursor(&self) -> Option<MonetaryCursor> { self.fee_cursor }

    fn bind_native_family_with<'s>(
        &'s self,
        policy_limits: PhloFamilyFundingLimits,
        budget: &HostWorkBudget,
        bind: impl FnOnce() -> Result<CheckedDirectWalletFunding<'s, A>, DirectWalletBindingError>,
    ) -> Result<Option<CheckedDirectWalletPolicy<'s, A>>, DirectWalletPolicySnapshotError> {
        let source_count = self.wallets.sources().len();
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            source_count,
        )?;
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            source_count
                .checked_mul(size_of::<(&[u8], PhloFundingSource<'_>)>())
                .ok_or(DirectWalletPolicySnapshotError::Overflow)?,
        )?;
        let bound = bind()?;
        let snapshots = PhloFamilyCursorSnapshot {
            canonical_custodies: &self.canonical_custodies,
            resource: PhloScopedCursorSnapshot {
                scope: self.resource_scope,
                cursor: self.resource_cursor.unwrap_or(MonetaryCursor::INITIAL),
            },
            fee: PhloScopedCursorSnapshot {
                scope: self.fee_scope,
                cursor: self.fee_cursor.unwrap_or(MonetaryCursor::INITIAL),
            },
        };
        let Some(policy) = bound
            .consent()
            .plan_funding_policy(snapshots, policy_limits, budget)?
        else {
            return Ok(None);
        };
        Ok(Some(CheckedDirectWalletPolicy {
            snapshot: self,
            policy: policy.into_native(budget)?,
        }))
    }
}

impl<'a, A> CheckedDirectWalletPolicy<'a, A> {
    pub fn snapshot(&self) -> &'a DirectWalletPolicySnapshot<'a, A> { self.snapshot }
    pub fn policy(&self) -> &CheckedNativeSignedPhloFamilyPolicy<'a, A> { &self.policy }
}

macro_rules! impl_wallet_policy_binding {
    ($envelope:ty $(, $context:ident)?) => {
        impl DirectWalletPolicySnapshot<'_, $envelope> {
            pub fn bind_native_family<'s>(
                &'s self,
                view: &'s PhloFundingIntentView<'s>,
                family: &'s CheckedPhloFundingFamily<'s>,
                terms: PhloFundingTerms<'s>,
                signed_limits: SignedPhloConsentLimits,
                policy_limits: PhloFamilyFundingLimits,
                $($context: &CasperShardConf, resource_policy: PhloSchedulePolicy<'_>,)?
                budget: &HostWorkBudget,
            ) -> Result<
                Option<CheckedDirectWalletPolicy<'s, $envelope>>,
                DirectWalletPolicySnapshotError,
            > {
                $(check_adopted_chain_context(
                    family.cases()[0].obligations.execution().controls(), $context,
                )?;
                view.check_schedule_policy(resource_policy)?;)?
                self.bind_native_family_with(policy_limits, budget, || {
                    self.wallets.bind_family(view, family, terms, signed_limits)
                })
            }
        }
    };
}

impl_wallet_policy_binding!(FundedDeploy);
impl_wallet_policy_binding!(OfferedFundedDeploy, adopted_shard);

impl DirectWalletPolicySnapshot<'_, OfferedFundedDeploy> {
    pub fn bind_native_family_from_genesis<'s>(
        &'s self,
        view: &'s PhloFundingIntentView<'s>,
        family: &'s CheckedPhloFundingFamily<'s>,
        terms: PhloFundingTerms<'s>,
        signed_limits: SignedPhloConsentLimits,
        policy_limits: PhloFamilyFundingLimits,
        adopted_shard: &CasperShardConf,
        genesis_policy: &GenesisResourcePolicy,
        budget: &HostWorkBudget,
    ) -> Result<
        Option<CheckedDirectWalletPolicy<'s, OfferedFundedDeploy>>,
        DirectWalletPolicySnapshotError,
    > {
        check_genesis_minimum(genesis_policy.minimum_price(), adopted_shard)?;
        let context = genesis_policy.clone().adopt(adopted_shard)?;
        self.bind_native_family_in_context(
            view,
            family,
            terms,
            signed_limits,
            policy_limits,
            &context,
            budget,
        )
    }

    pub fn bind_native_family_in_context<'s>(
        &'s self,
        view: &'s PhloFundingIntentView<'s>,
        family: &'s CheckedPhloFundingFamily<'s>,
        terms: PhloFundingTerms<'s>,
        signed_limits: SignedPhloConsentLimits,
        policy_limits: PhloFamilyFundingLimits,
        context: &AdoptedResourcePolicy,
        budget: &HostWorkBudget,
    ) -> Result<
        Option<CheckedDirectWalletPolicy<'s, OfferedFundedDeploy>>,
        DirectWalletPolicySnapshotError,
    > {
        context.check_controls(family.cases()[0].obligations.execution().controls())?;
        context.genesis().check_funding_intent(view)?;
        self.bind_native_family_with(policy_limits, budget, || {
            self.wallets.bind_family(view, family, terms, signed_limits)
        })
    }
}

#[cfg(test)]
mod tests;
