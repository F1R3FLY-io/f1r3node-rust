use std::collections::{BTreeMap, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::Event;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::native_wallet_receipt::{
    NativeWalletReceiptLimits, NativeWalletReceiptRow, NativeWalletReceiptV1,
};
use models::rust::phlo_grant_creation::VerifiedGrantCreationV1;
use models::rust::phlo_intent::PhloFundingIntentVersioned;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use rholang::rust::interpreter::accounting::economic_failure::classify_errors;
use rholang::rust::interpreter::accounting::monetary_allocation::FundingOutcomeCursorTransition;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloAcquisitionLimits, NativePhloPurseLimits, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, PhloFailure, PhloOutcome, PhloResourceAmount,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;

use super::NativeFundedAttempt;
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::acceptance::{
    CandidateDraftSettlementPermit, CandidateReplayPermit,
};
use crate::rust::util::rholang::costacc::direct_wallet_funding::CheckedDirectWalletSettlement;
use crate::rust::util::rholang::costacc::offered_grants::{
    OfferedGrantChange, OfferedGrantLimits, OfferedGrantSnapshot, VerifiedOfferedGrantSources,
};
use crate::rust::util::rholang::costacc::prepaid_receipts::{
    CanonicalPrepaidSelection, NativeMeasuredSettlementLimits, NativePrepaidDemandInput,
    NativePrepaidDemandLimits, NativePrepaidInventory,
};
use crate::rust::util::rholang::costacc::supply_reader::{
    RuntimeManagerSupplyReader, SupplyReader,
};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub struct NativeAttemptSettlementInput<'r, 's, 'p> {
    pub inventory: &'r NativePrepaidInventory<'s, 'p>,
    pub schedule: &'r PhloScheduleBinding<'r>,
    pub terms: &'r [u8],
    pub retained: &'r [PhloResourceAmount<'r>],
}

pub struct NativeGrantSettlementInput<'a> {
    pub snapshot: &'a OfferedGrantSnapshot,
    pub verified: &'a VerifiedOfferedGrantSources,
    pub authenticated_time: u64,
    pub limits: OfferedGrantLimits,
}

#[derive(Clone, Copy)]
pub struct NativeGrantIssuanceInput<'a> {
    pub verified: &'a [VerifiedGrantCreationV1],
    pub limits: OfferedGrantLimits,
}

pub struct NativeCapturedSettlement<'a> {
    checked: CheckedDirectWalletSettlement<'a, OfferedFundedDeploy>,
    selection: CanonicalPrepaidSelection,
    cursor_transition: FundingOutcomeCursorTransition,
}

impl<'a> NativeCapturedSettlement<'a> {
    pub fn checked(&self) -> &CheckedDirectWalletSettlement<'a, OfferedFundedDeploy> {
        &self.checked
    }

    pub fn selection(&self) -> &CanonicalPrepaidSelection { &self.selection }

    pub fn cursor_transition(&self) -> &FundingOutcomeCursorTransition { &self.cursor_transition }
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

pub struct NativeFundedSettlement {
    pub post_state_root: StateHash,
    pub rev_spent: u128,
    pub resource_rev: u128,
    pub fee_rev: u128,
    pub wallet_receipt: Vec<u8>,
    pub wallet_balances: Vec<(VaultAddress, i64)>,
    pub settlement_log: Vec<Event>,
    pub mergeable: HashMap<Par, MergeType>,
    pub grant_changes: Vec<OfferedGrantChange>,
}

pub(super) fn invalid(error: impl std::fmt::Display) -> CasperError {
    CasperError::RuntimeError(error.to_string())
}

pub(super) fn check_metered_usage(
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
    pub async fn settle_wallet_and_capture_balances(
        &mut self,
        manager: &RuntimeManager,
        checked: &CheckedDirectWalletSettlement<'a, OfferedFundedDeploy>,
        reservation_id: [u8; 32],
        fee_address: &VaultAddress,
        initial_rand: Blake2b512Random,
        receipt_limits: NativeWalletReceiptLimits,
        grants: Option<NativeGrantSettlementInput<'_>>,
        issuances: Option<NativeGrantIssuanceInput<'_>>,
        budget: &HostWorkBudget,
    ) -> Result<NativeFundedSettlement, CasperError> {
        let permit = self.draft_permit.as_ref().ok_or_else(|| {
            invalid("native wallet settlement requires an authorized private draft permit")
        })?;
        if !std::ptr::eq(checked.snapshot(), self.policy.snapshot()) {
            return Err(invalid(
                "wallet settlement belongs to another funding snapshot",
            ));
        }
        self.runtime
            .settle_checked_wallet_and_capture_balances(
                manager,
                checked,
                permit,
                reservation_id,
                fee_address,
                initial_rand,
                receipt_limits,
                grants,
                issuances,
                budget,
            )
            .await
    }
}

impl RuntimeOps {
    pub(crate) async fn settle_checked_wallet_and_capture_balances<'a>(
        &mut self,
        manager: &RuntimeManager,
        checked: &CheckedDirectWalletSettlement<'a, OfferedFundedDeploy>,
        permit: &CandidateDraftSettlementPermit,
        reservation_id: [u8; 32],
        fee_address: &VaultAddress,
        initial_rand: Blake2b512Random,
        receipt_limits: NativeWalletReceiptLimits,
        grants: Option<NativeGrantSettlementInput<'_>>,
        issuances: Option<NativeGrantIssuanceInput<'_>>,
        budget: &HostWorkBudget,
    ) -> Result<NativeFundedSettlement, CasperError> {
        self.settle_checked_wallet_inner(
            manager,
            checked,
            &permit.envelope_identity(),
            reservation_id,
            fee_address,
            initial_rand,
            receipt_limits,
            grants,
            issuances,
            budget,
        )
        .await
    }

    pub(super) async fn prepare_replayed_wallet_settlement<'a>(
        &mut self,
        manager: &RuntimeManager,
        checked: &CheckedDirectWalletSettlement<'a, OfferedFundedDeploy>,
        permit: &CandidateReplayPermit<'_>,
        reservation_id: [u8; 32],
        fee_address: &VaultAddress,
        initial_rand: Blake2b512Random,
        receipt_limits: NativeWalletReceiptLimits,
        grants: Option<NativeGrantSettlementInput<'_>>,
        issuances: Option<NativeGrantIssuanceInput<'_>>,
        budget: &HostWorkBudget,
    ) -> Result<NativeFundedSettlement, CasperError> {
        if self.runtime.get_root().await.bytes() != permit.settlement_runtime_root() {
            return Err(invalid(
                "replay wallet settlement starts from another runtime root",
            ));
        }
        let baseline_root = self.runtime.get_root().await;
        let settled = self
            .settle_checked_wallet_inner(
                manager,
                checked,
                permit.envelope_identity(),
                reservation_id,
                fee_address,
                initial_rand,
                receipt_limits,
                grants,
                issuances,
                budget,
            )
            .await?;
        let verified: Result<(), CasperError> = async {
            self.runtime.check_replay_data().await?;
            permit.verify_wallet_prefix(
                &settled.wallet_receipt,
                settled.resource_rev,
                settled.fee_rev,
            )
        }
        .await;
        if let Err(error) = verified {
            self.runtime.reset(&baseline_root).await?;
            if self.runtime.get_root().await != baseline_root {
                return Err(invalid("replay wallet failed to restore settlement root"));
            }
            return Err(error);
        }
        Ok(settled)
    }

    async fn settle_checked_wallet_inner<'a>(
        &mut self,
        manager: &RuntimeManager,
        checked: &CheckedDirectWalletSettlement<'a, OfferedFundedDeploy>,
        authorized_identity: &[u8],
        reservation_id: [u8; 32],
        fee_address: &VaultAddress,
        initial_rand: Blake2b512Random,
        receipt_limits: NativeWalletReceiptLimits,
        grants: Option<NativeGrantSettlementInput<'_>>,
        issuances: Option<NativeGrantIssuanceInput<'_>>,
        budget: &HostWorkBudget,
    ) -> Result<NativeFundedSettlement, CasperError> {
        let identity = checked
            .capture()
            .scoped()
            .signed_intent()
            .envelope()
            .envelope_commitment()
            .map_err(invalid)?;
        if authorized_identity != identity.as_ref() {
            return Err(invalid(
                "wallet settlement permit belongs to another envelope",
            ));
        }
        let mut prepared_issuances = if let Some(input) = &issuances {
            let original = checked.snapshot().wallets().pre_state_root();
            let snapshot = manager.capture_offered_grant_issuances(
                original,
                input.verified,
                input.limits,
                budget,
            )?;
            Some(snapshot.prepare_issuances(original, input.verified, input.limits, budget)?)
        } else {
            None
        };
        let authorization = checked.snapshot().wallets().authorization();
        let grant_uses = match authorization.versioned_record() {
            PhloFundingIntentVersioned::V1(_) => &[][..],
            PhloFundingIntentVersioned::V2(record) => record.grant_uses.as_slice(),
        };
        if grant_uses.is_empty() != grants.is_none() {
            return Err(invalid(
                "grant settlement presence differs from signed funding use",
            ));
        }
        let mut prepared_grants = if let Some(input) = grants {
            let funding_root = authorization
                .grant_root()
                .ok_or_else(|| invalid("grant source proof is absent"))?;
            if input.snapshot.root() != funding_root || input.verified.root() != funding_root {
                return Err(invalid("grant settlement belongs to another funding root"));
            }
            let mut debits = BTreeMap::new();
            let count = checked.capture().amounts().len();
            let size = count
                .checked_mul(std::mem::size_of::<([u8; 32], u128)>())
                .ok_or_else(|| invalid("grant debit projection size overflow"))?;
            budget
                .reserve(
                    HostWorkDimension::SearchStateBytes,
                    HostWorkUnits::new(
                        u64::try_from(size)
                            .map_err(|_| invalid("grant debit projection size overflow"))?,
                    ),
                )
                .map_err(invalid)?;
            for amount in checked.capture().amounts() {
                let key: [u8; 32] = amount
                    .custody()
                    .try_into()
                    .map_err(|_| invalid("grant debit custody width"))?;
                let acquisition = u128::try_from(amount.acquisition())
                    .map_err(|_| invalid("negative resource charge"))?;
                let fee = u128::try_from(amount.fee()).map_err(|_| invalid("negative fee"))?;
                let debit = acquisition
                    .checked_add(fee)
                    .ok_or_else(|| invalid("grant debit overflow"))?;
                if debits.insert(key, debit).is_some() {
                    return Err(invalid("duplicate grant debit custody"));
                }
            }
            let mut measured = Vec::new();
            measured
                .try_reserve_exact(grant_uses.len())
                .map_err(|_| invalid("grant debit allocation failed"))?;
            for use_record in grant_uses {
                let source = authorization
                    .record()
                    .sources
                    .get(use_record.source_index as usize)
                    .ok_or_else(|| invalid("grant source index differs from signed funding"))?;
                let custody: [u8; 32] = source
                    .custody()
                    .try_into()
                    .map_err(|_| invalid("grant source custody width"))?;
                let debit = *debits
                    .get(&custody)
                    .ok_or_else(|| invalid("grant source has no measured debit"))?;
                measured.push((use_record.source_index, debit));
            }
            Some((
                input.snapshot.prepare_measured_draws(
                    input.verified,
                    input.authenticated_time,
                    &measured,
                    input.limits,
                    budget,
                )?,
                input.limits,
            ))
        } else {
            None
        };
        checked.check_execution_root(&self.runtime.get_root().await.bytes())?;
        let mut prepared = checked
            .prepare_request(reservation_id, fee_address, initial_rand, budget)
            .map_err(invalid)?;
        let mut resource_rev = 0u128;
        let mut fee_rev = 0u128;
        for row in checked.capture().amounts() {
            let acquisition = u128::try_from(row.acquisition())
                .map_err(|_| invalid("negative resource charge"))?;
            let fee = u128::try_from(row.fee()).map_err(|_| invalid("negative fee"))?;
            resource_rev = resource_rev
                .checked_add(acquisition)
                .ok_or_else(|| invalid("aggregate REV charge overflow"))?;
            fee_rev = fee_rev
                .checked_add(fee)
                .ok_or_else(|| invalid("aggregate REV fee overflow"))?;
        }
        let rev_spent = resource_rev
            .checked_add(fee_rev)
            .ok_or_else(|| invalid("aggregate REV spent overflow"))?;
        let baseline_root = self.runtime.get_root().await;
        let checkpoint = self.runtime.create_soft_checkpoint().await;
        let mut committed = false;
        let result = async {
            let (wallet_log, mergeable) = if let Some(mut request) = prepared.request() {
                self.runtime.cost.reset_for_system_deploy();
                let (log, result, mergeable) =
                    self.play_system_deploy_internal(&mut request).await?;
                if let Either::Left(error) = result {
                    return Err(invalid(format!(
                        "wallet settlement rejected: {}",
                        error.error_message
                    )));
                }
                (log, mergeable)
            } else {
                (Vec::new(), HashMap::new())
            };
            let mut settlement_log = Vec::new();
            settlement_log
                .try_reserve_exact(wallet_log.len())
                .map_err(|_| invalid("wallet settlement log allocation failed"))?;
            settlement_log.extend(wallet_log);
            if let (Some(input), Some(prepared)) = (&issuances, &prepared_issuances) {
                let grant_log = self
                    .apply_offered_grant_transitions(
                        prepared,
                        checked.snapshot().wallets().pre_state_root(),
                        input.limits,
                    )
                    .await?;
                settlement_log.extend(
                    grant_log
                        .into_iter()
                        .map(crate::rust::util::event_converter::to_casper_event),
                );
            }
            if let Some((prepared, limits)) = &prepared_grants {
                let grant_log = self
                    .apply_offered_grant_transitions(
                        prepared,
                        checked.snapshot().wallets().pre_state_root(),
                        *limits,
                    )
                    .await?;
                settlement_log.extend(
                    grant_log
                        .into_iter()
                        .map(crate::rust::util::event_converter::to_casper_event),
                );
            }
            let root = self.runtime.create_checkpoint().await.root.to_bytes_prost();
            committed = true;
            let reader = RuntimeManagerSupplyReader {
                runtime_manager: manager,
                pre_state_hash: root.clone(),
            };
            let mut wallet_balances = Vec::new();
            let payers = checked.snapshot().wallets().authorization().payers();
            wallet_balances
                .try_reserve_exact(payers.len())
                .map_err(|_| invalid("wallet balance allocation failed"))?;
            for payer in payers.values() {
                let inventory = reader.read_purse(&payer.signature, budget).await?;
                let balance = inventory
                    .balance
                    .ok_or_else(|| invalid("wallet balance absent"))?;
                wallet_balances.push((payer.address.clone(), balance));
            }
            let amounts = checked.capture().amounts();
            if amounts.len() != payers.len() || wallet_balances.len() != payers.len() {
                return Err(invalid(
                    "wallet receipt payer count differs from settlement",
                ));
            }
            let mut rows = Vec::new();
            rows.try_reserve_exact(payers.len())
                .map_err(|_| invalid("wallet receipt allocation failed"))?;
            for (((custody, payer), amount), (address, balance)) in payers
                .iter()
                .zip(amounts.iter())
                .zip(wallet_balances.iter())
            {
                if amount.custody() != custody.as_slice() || payer.address != *address {
                    return Err(invalid(
                        "wallet receipt payer differs from authorized settlement",
                    ));
                }
                rows.push((
                    payer.address.to_base58(),
                    u128::try_from(amount.acquisition())
                        .map_err(|_| invalid("negative resource charge"))?,
                    u128::try_from(amount.fee()).map_err(|_| invalid("negative fee"))?,
                    u64::try_from(*balance).map_err(|_| invalid("negative wallet balance"))?,
                ));
            }
            rows.sort_unstable_by(|left, right| left.0.cmp(&right.0));
            let receipt = NativeWalletReceiptV1 {
                rows: rows
                    .iter()
                    .map(|row| NativeWalletReceiptRow {
                        address: row.0.as_bytes(),
                        resource_rev: row.1,
                        fee_rev: row.2,
                        post_balance: row.3,
                    })
                    .collect(),
                resource_rev,
                fee_rev,
            };
            let wallet_receipt = receipt.encode(receipt_limits).map_err(invalid)?;
            if receipt.rev_spent().map_err(invalid)? != rev_spent {
                return Err(invalid("wallet receipt REV spent differs from settlement"));
            }
            let issuance_count = prepared_issuances
                .as_ref()
                .map_or(0, |prepared| prepared.changes().len());
            let draw_count = prepared_grants
                .as_ref()
                .map_or(0, |(prepared, _)| prepared.changes().len());
            let mut grant_changes = Vec::new();
            grant_changes
                .try_reserve_exact(
                    issuance_count
                        .checked_add(draw_count)
                        .ok_or_else(|| invalid("grant transition count overflow"))?,
                )
                .map_err(|_| invalid("grant transition allocation failed"))?;
            if let Some(prepared) = prepared_issuances.take() {
                grant_changes.extend(prepared.into_changes());
            }
            if let Some((prepared, _)) = prepared_grants.take() {
                grant_changes.extend(prepared.into_changes());
            }
            Ok(NativeFundedSettlement {
                post_state_root: root,
                rev_spent,
                resource_rev,
                fee_rev,
                wallet_receipt,
                wallet_balances,
                settlement_log,
                mergeable,
                grant_changes,
            })
        }
        .await;
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if committed {
                    self.runtime
                        .reset(&baseline_root)
                        .await
                        .map_err(CasperError::InterpreterError)?;
                }
                self.runtime.revert_to_soft_checkpoint(checkpoint).await;
                Err(error)
            }
        }
    }
}

impl<'a> NativeFundedAttempt<'_, 'a> {
    pub fn capture_measured_settlement(
        &self,
        input: NativeAttemptSettlementInput<'_, '_, '_>,
        limits: NativeAttemptSettlementLimits,
        budget: &HostWorkBudget,
    ) -> Result<NativeCapturedSettlement<'a>, CasperError> {
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
        // DR-113: this family path has no caller. It keeps the measured family
        // bound, unbilled as before.
        self.adopted
            .bind_execution_contract(controls, input.schedule, rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource::Certificate)?;
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
        let selection = input
            .inventory
            .select_measured_demand(&demand, limits.demand, budget)?;
        let binding = input.inventory.bind_measured_demand(
            NativePrepaidDemandInput {
                expected_root: self.policy.snapshot().wallets().pre_state_root(),
                controls,
                demand: &demand,
                draws: selection.draws(),
                demand_positions: selection.demand_positions(),
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
        let checked = self
            .policy
            .capture_settlement(
                observed,
                PhloOutcome::Accepted(&failures[..count]),
                limits.settlement.matching,
                limits.settlement.capture,
                budget,
            )
            .map(|settlement| settlement.bind_execution_root(self.execution_root))
            .map_err(invalid)?;
        let branch = checked.capture().scoped().capture().branch();
        let transition = self
            .policy
            .policy()
            .policy()
            .selection()
            .cursor_transitions()
            .get(branch)
            .ok_or_else(|| invalid("captured funding branch has no cursor transition"))?;
        let witness_bytes = transition
            .resource_restriction_witness()
            .map_or(Some(0), |witness| {
                witness.len().checked_mul(std::mem::size_of::<u64>())
            })
            .ok_or_else(|| invalid("cursor transition copy size overflow"))?;
        let bytes = std::mem::size_of::<FundingOutcomeCursorTransition>()
            .checked_add(witness_bytes)
            .and_then(|n| n.checked_add(transition.possible_fee_payers().len()))
            .ok_or_else(|| invalid("cursor transition copy size overflow"))?;
        budget
            .reserve(
                HostWorkDimension::SearchStateBytes,
                HostWorkUnits::new(
                    u64::try_from(bytes)
                        .map_err(|_| invalid("cursor transition copy size overflow"))?,
                ),
            )
            .map_err(invalid)?;
        Ok(NativeCapturedSettlement {
            checked,
            selection,
            cursor_transition: transition.clone(),
        })
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
