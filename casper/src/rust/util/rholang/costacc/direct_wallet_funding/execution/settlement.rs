use std::collections::{BTreeMap, HashMap};

// Added by G3 (DR-135, S7): the owner rows of a settlement (R10).
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::Event;
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
// Added by G3 (DR-135, S7).
use rholang::rust::interpreter::accounting::owner_funding::OwnerTotals;
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, PhloFailure, PhloOutcome, PhloResourceAmount,
};
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
// Added by G3 (DR-135, S7).
use crate::rust::util::rholang::costacc::vault_cost_deploy::{
    ApplyCostDeploy, VaultAllocation, VaultSettlement,
};
// Added by G3 (DR-135, S7).
use crate::rust::util::rholang::costacc::vault_payer::{vault_payer, VaultPayer};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// Added by G3 (DR-135, S7): the domain of the reservation id of the owner
/// rows of a settlement.
const OWNER_SETTLEMENT_DOMAIN: &[u8] = b"f1r3node:owner-settlement:v1";

/// Added by G3 (DR-135, S7): the split of the settlement seed that plays the
/// owner rows. The client request keeps the seed itself.
const OWNER_SETTLEMENT_SPLIT: u8 = 0x0b;

/// Added by G3 (DR-135, S7): the owner rows of the settlement of one offered
/// member (R10, G-4): the billed debit of each owner purse with a positive
/// debit, with its payer, in ascending custody order. Their sum is the owner
/// REV. The rows burn their debits through `ApplyCostDeploy` with no cursor,
/// so the client's cursor transitions do not count them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerSettlement {
    rows: Vec<([u8; 32], VaultPayer, u128)>,
    owner_rev: u128,
}

impl OwnerSettlement {
    /// The owner rows of the billed sums `totals`. The payer of a row is the
    /// payer of the signature of its table entry. The debits must sum to the
    /// owner REV, as the ledger bills them.
    pub(super) fn new(totals: &OwnerTotals) -> Result<Self, CasperError> {
        let mut rows = Vec::with_capacity(totals.debits.len());
        let mut owner_rev = 0u128;
        for (custody, debit) in &totals.debits {
            if *debit == 0 {
                continue;
            }
            let entry = totals
                .table
                .get(custody)
                .ok_or_else(|| invalid("an owner debit names no listed purse"))?;
            let payer = vault_payer(&entry.signature).map_err(invalid)?;
            if payer.custody_key != *custody {
                return Err(invalid("an owner debit names another purse than its entry"));
            }
            owner_rev = owner_rev
                .checked_add(u128::from(*debit))
                .ok_or_else(|| invalid("owner REV overflows"))?;
            rows.push((*custody, payer, u128::from(*debit)));
        }
        if owner_rev != totals.rev {
            return Err(invalid("owner debits differ from the billed owner REV"));
        }
        Ok(Self { rows, owner_rev })
    }

    /// The rows, in ascending custody order.
    pub(super) fn rows(&self) -> &[([u8; 32], VaultPayer, u128)] { &self.rows }

    /// The REV that the owners pay.
    pub(super) fn owner_rev(&self) -> u128 { self.owner_rev }

    /// Adds the debit of each owner purse to the guard debits `debits`, so
    /// the held-capacity guard covers the owner purses (§7 "Guard").
    pub(super) fn add_debits(
        &self,
        debits: &mut BTreeMap<[u8; 32], u128>,
    ) -> Result<(), CasperError> {
        for (custody, _, debit) in &self.rows {
            let total = debits.entry(*custody).or_insert(0);
            *total = total
                .checked_add(*debit)
                .ok_or_else(|| invalid("an owner guard debit overflows"))?;
        }
        Ok(())
    }

    /// The payer of each owner purse, for the held-capacity guard.
    pub(super) fn payers(&self) -> BTreeMap<[u8; 32], VaultPayer> {
        self.rows
            .iter()
            .map(|(custody, payer, _)| (*custody, payer.clone()))
            .collect()
    }

    /// The request that burns each debit with no fee and no cursor. Its
    /// reservation id is the digest of a fixed domain and the reservation id
    /// of the member.
    fn request(
        &self,
        reservation_id: [u8; 32],
        fee_address: &VaultAddress,
        rand: Blake2b512Random,
    ) -> Result<ApplyCostDeploy, CasperError> {
        let mut identity = Vec::with_capacity(OWNER_SETTLEMENT_DOMAIN.len() + reservation_id.len());
        identity.extend_from_slice(OWNER_SETTLEMENT_DOMAIN);
        identity.extend_from_slice(&reservation_id);
        let owner_reservation: [u8; 32] = Blake2b256::hash(identity)
            .try_into()
            .map_err(|_| invalid("the owner reservation id is not 32 bytes"))?;
        let mut allocations = Vec::with_capacity(self.rows.len());
        let mut settlements = Vec::with_capacity(self.rows.len());
        for (_, payer, debit) in &self.rows {
            let amount = i64::try_from(*debit)
                .map_err(|_| invalid("an owner debit exceeds the vault amount range"))?;
            allocations.push(VaultAllocation::new(payer.address.to_base58(), amount)?);
            settlements.push(VaultSettlement::new(payer.address.to_base58(), amount, 0)?);
        }
        ApplyCostDeploy::new(
            owner_reservation,
            allocations,
            settlements,
            fee_address.to_base58(),
            rand,
        )
    }
}

/// Added by G3 (DR-135, S7): merges the owner rows `owners` into the client
/// rows `rows` of a wallet receipt (R10). An owner row is the address, the
/// debit and the post balance of an owner purse. A purse that a client row
/// already holds keeps that one row, and the owner debit joins its resource
/// REV, because the receipt rejects duplicate addresses. Any other purse
/// gets a row of its own with no fee.
fn merge_owner_rows(
    rows: &mut Vec<(String, u128, u128, u64)>,
    owners: &[(String, u128, u64)],
) -> Result<(), CasperError> {
    rows.try_reserve(owners.len())
        .map_err(|_| invalid("wallet receipt allocation failed"))?;
    for (address, debit, balance) in owners {
        match rows.iter_mut().find(|row| row.0 == *address) {
            Some(row) => {
                row.1 = row
                    .1
                    .checked_add(*debit)
                    .ok_or_else(|| invalid("owner row REV overflows"))?;
            }
            None => rows.push((address.clone(), *debit, 0, *balance)),
        }
    }
    Ok(())
}

// Changed by G1-7 (DR-65 item 5): the producer passes the same inputs to the
// pass after a failed held-capacity guard, so the inputs are copies of
// references.
#[derive(Clone, Copy)]
pub struct NativeAttemptSettlementInput<'r, 's, 'p> {
    pub inventory: &'r NativePrepaidInventory<'s, 'p>,
    pub schedule: &'r PhloScheduleBinding<'r>,
    pub terms: &'r [u8],
    pub retained: &'r [PhloResourceAmount<'r>],
}

// Changed by G1-7 (DR-65 item 5): copied for the pass after a failed guard.
#[derive(Clone, Copy)]
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

/// Added by dedup (task 11415, D4): why `custody_debits` fails. Grant
/// settlement and the held-capacity guard map each case to their own message.
#[derive(Debug)]
pub(super) enum CustodyDebitError {
    /// A custody key is not 32 bytes.
    Width,
    /// An acquisition is negative.
    NegativeCharge,
    /// A fee is negative.
    NegativeFee,
    /// A debit overflows.
    Overflow,
    /// Two amounts name one custody.
    Duplicate,
}

/// Added by dedup (task 11415, D4): the debit of each custody in a settlement
/// capture, the acquisition plus the fee. Grant settlement (below) and the
/// held-capacity guard (`held_capacity.rs`) share it, so both read the same
/// debits.
pub(super) fn custody_debits(
    amounts: &[rholang::rust::interpreter::accounting::phlo_execution::NativePhloSourceAmounts<
        '_,
    >],
) -> Result<BTreeMap<[u8; 32], u128>, CustodyDebitError> {
    let mut debits = BTreeMap::new();
    for amount in amounts {
        let custody: [u8; 32] = amount
            .custody()
            .try_into()
            .map_err(|_| CustodyDebitError::Width)?;
        let acquisition =
            u128::try_from(amount.acquisition()).map_err(|_| CustodyDebitError::NegativeCharge)?;
        let fee = u128::try_from(amount.fee()).map_err(|_| CustodyDebitError::NegativeFee)?;
        let debit = acquisition
            .checked_add(fee)
            .ok_or(CustodyDebitError::Overflow)?;
        if debits.insert(custody, debit).is_some() {
            return Err(CustodyDebitError::Duplicate);
        }
    }
    Ok(debits)
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
    ) -> Result<NativeFundedSettlement, CasperError> {
        let permit = self.draft_permit.as_ref().ok_or_else(|| {
            invalid("native wallet settlement requires an authorized private draft permit")
        })?;
        if !std::ptr::eq(checked.snapshot(), self.policy.snapshot()) {
            return Err(invalid(
                "wallet settlement belongs to another funding snapshot",
            ));
        }
        let result = self
            .runtime
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
                // Added by G3 (DR-135, S7): the family path has no owner
                // funding.
                None,
            )
            .await;
        self.runtime
            .finish_native_fs_scope(result.is_ok() && self.evaluation.errors.is_empty());
        result
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
        // Added by G3 (DR-135, S7): the owner rows of the member (R10).
        owners: Option<&OwnerSettlement>,
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
            // Added by G3 (DR-135, S7).
            owners,
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
        // Added by G3 (DR-135, S7): the owner rows of the member (R10).
        owners: Option<&OwnerSettlement>,
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
                // Added by G3 (DR-135, S7).
                owners,
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
        // Added by G3 (DR-135, S7): the owner rows of the member (R10). They
        // settle after the client request and its grant transitions, from a
        // fixed split of the seed, and their receipt rows merge with the
        // client rows by address.
        owners: Option<&OwnerSettlement>,
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
            let snapshot =
                manager.capture_offered_grant_issuances(original, input.verified, input.limits)?;
            Some(snapshot.prepare_issuances(original, input.verified, input.limits)?)
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
            // Moved by dedup (task 11415) to `custody_debits` in
            // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/settlement.rs`.
            // let mut debits = BTreeMap::new();
            // let count = checked.capture().amounts().len();
            // let size = count
            //     .checked_mul(std::mem::size_of::<([u8; 32], u128)>())
            //     .ok_or_else(|| invalid("grant debit projection size overflow"))?;
            // budget
            //     .reserve(
            //         HostWorkDimension::SearchStateBytes,
            //         HostWorkUnits::new(
            //             u64::try_from(size)
            //                 .map_err(|_| invalid("grant debit projection size overflow"))?,
            //         ),
            //     )
            //     .map_err(invalid)?;
            // for amount in checked.capture().amounts() {
            //     let key: [u8; 32] = amount
            //         .custody()
            //         .try_into()
            //         .map_err(|_| invalid("grant debit custody width"))?;
            //     let acquisition = u128::try_from(amount.acquisition())
            //         .map_err(|_| invalid("negative resource charge"))?;
            //     let fee = u128::try_from(amount.fee()).map_err(|_| invalid("negative fee"))?;
            //     let debit = acquisition
            //         .checked_add(fee)
            //         .ok_or_else(|| invalid("grant debit overflow"))?;
            //     if debits.insert(key, debit).is_some() {
            //         return Err(invalid("duplicate grant debit custody"));
            //     }
            // }
            let debits =
                custody_debits(checked.capture().amounts()).map_err(|error| match error {
                    CustodyDebitError::Width => invalid("grant debit custody width"),
                    CustodyDebitError::NegativeCharge => invalid("negative resource charge"),
                    CustodyDebitError::NegativeFee => invalid("negative fee"),
                    CustodyDebitError::Overflow => invalid("grant debit overflow"),
                    CustodyDebitError::Duplicate => invalid("duplicate grant debit custody"),
                })?;
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
                )?,
                input.limits,
            ))
        } else {
            None
        };
        checked.check_execution_root(&self.runtime.get_root().await.bytes())?;
        // Added by G3 (DR-135, S7): the seed of the owner request, split
        // before the client request takes the seed.
        let owner_rand = initial_rand.split_byte(OWNER_SETTLEMENT_SPLIT);
        let mut prepared = checked
            .prepare_request(reservation_id, fee_address, initial_rand)
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
        // Added by G3 (DR-135, S7): the owners pay the owner REV, so the
        // resource REV of the receipt holds it (R10).
        let resource_rev = resource_rev
            .checked_add(owners.map_or(0, OwnerSettlement::owner_rev))
            .ok_or_else(|| invalid("aggregate REV charge overflow"))?;
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
            // Added by G3 (DR-135, S7): the owner rows burn their debits with
            // no fee and no cursor (R10).
            let mut mergeable = mergeable;
            if let Some(owners) = owners.filter(|owners| !owners.rows().is_empty()) {
                let mut request = owners.request(reservation_id, fee_address, owner_rand)?;
                self.runtime.cost.reset_for_system_deploy();
                let (owner_log, result, owner_mergeable) =
                    self.play_system_deploy_internal(&mut request).await?;
                if let Either::Left(error) = result {
                    return Err(invalid(format!(
                        "owner settlement rejected: {}",
                        error.error_message
                    )));
                }
                settlement_log
                    .try_reserve_exact(owner_log.len())
                    .map_err(|_| invalid("owner settlement log allocation failed"))?;
                settlement_log.extend(owner_log);
                mergeable
                    .try_reserve(owner_mergeable.len())
                    .map_err(|_| invalid("owner settlement mergeable allocation failed"))?;
                mergeable.extend(owner_mergeable);
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
                let inventory = reader.read_purse(&payer.signature).await?;
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
            // Added by G3 (DR-135, S7): each owner row joins the receipt. A
            // purse that is also a client payer has one row, which adds the
            // owner debit to its resource REV (R10).
            if let Some(owners) = owners {
                let mut owner_rows = Vec::with_capacity(owners.rows().len());
                for (_, payer, debit) in owners.rows() {
                    let inventory = reader.read_purse(&payer.signature).await?;
                    let balance = inventory
                        .balance
                        .ok_or_else(|| invalid("owner wallet balance absent"))?;
                    owner_rows.push((
                        payer.address.to_base58(),
                        *debit,
                        u64::try_from(balance)
                            .map_err(|_| invalid("negative owner wallet balance"))?,
                    ));
                }
                merge_owner_rows(&mut rows, &owner_rows)?;
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
        let regions = measured.region_demands(limits.regions).map_err(invalid)?;
        let located = regions.locate_purses(limits.purses).map_err(invalid)?;
        let demand = located
            .prepare_acquisition_demand(input.schedule, input.terms, limits.acquisition)
            .map_err(invalid)?;
        let selection = input
            .inventory
            .select_measured_demand(&demand, limits.demand)?;
        let binding = input.inventory.bind_measured_demand(
            NativePrepaidDemandInput {
                expected_root: self.policy.snapshot().wallets().pre_state_root(),
                controls,
                demand: &demand,
                draws: selection.draws(),
                demand_positions: selection.demand_positions(),
            },
            limits.demand,
        )?;
        let observed = check_counted_phlo_execution(
            controls,
            binding.witness(),
            limits.settlement.matching.execution,
        )
        .map_err(invalid)?
        .with_retained_acquisitions(input.retained)
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
            .union(classify_errors(&self.evaluation.errors).map_err(invalid)?);
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

    // Added by G3 (DR-135, S7): the owner rows of a settlement (R10).

    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{CostSignature, GPrivate, GUnforgeable};
    use rholang::rust::interpreter::accounting::owner_funding::{OwnerTable, OwnerTableEntry};
    use rholang::rust::interpreter::rho_type::{RhoList, RhoNil};

    use crate::rust::util::rholang::system_deploy::SystemDeployTrait;

    /// The signature of the private name with the id `[tag; 32]`, an owner
    /// purse of kind `PrivateName`.
    fn private_owner(tag: u8) -> CostSignature {
        CostSignature {
            value: Some(CostSignatureValue::Name(Par::default().with_unforgeables(
                vec![GUnforgeable {
                    unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![tag; 32] })),
                }],
            ))),
        }
    }

    /// The billed owner sums of one run that debits each purse `(tag, debit)`
    /// and lists each of them.
    fn owner_totals(debits: &[(u8, u64)]) -> OwnerTotals {
        let mut entries = Vec::with_capacity(debits.len());
        let mut billed = BTreeMap::new();
        let mut rev = 0u128;
        for (tag, debit) in debits {
            let payer = vault_payer(&private_owner(*tag)).expect("a private name has a payer");
            entries.push((payer.custody_key, OwnerTableEntry {
                signature: payer.signature,
                budget: *debit,
            }));
            billed.insert(payer.custody_key, *debit);
            rev += u128::from(*debit);
        }
        entries.sort_by_key(|(custody, _)| *custody);
        OwnerTotals {
            debits: billed,
            usage: 0,
            rev,
            table: OwnerTable::new(entries, 64).expect("the entries ascend and fit the room"),
        }
    }

    fn fee_address() -> VaultAddress {
        VaultAddress::from_unforgeable(&GPrivate { id: vec![9; 32] })
    }

    #[test]
    fn owner_rows_merge_with_a_client_row_of_the_same_purse() {
        let mut rows = vec![("a".to_string(), 10, 1, 90), ("b".to_string(), 4, 0, 40)];
        merge_owner_rows(&mut rows, &[
            ("b".to_string(), 6, 40),
            ("c".to_string(), 3, 30),
        ])
        .expect("the owner rows merge");
        rows.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(rows, vec![
            ("a".to_string(), 10, 1, 90),
            ("b".to_string(), 10, 0, 40),
            ("c".to_string(), 3, 0, 30),
        ]);
        let limits = NativeWalletReceiptLimits {
            wire: models::rust::phlo_wire::PhloWireLimits {
                total_bytes: 1 << 16,
                field_bytes: 1 << 16,
            },
            payers: 64,
        };
        let receipt = |rows: &[(String, u128, u128, u64)]| {
            NativeWalletReceiptV1 {
                rows: rows
                    .iter()
                    .map(|row| NativeWalletReceiptRow {
                        address: row.0.as_bytes(),
                        resource_rev: row.1,
                        fee_rev: row.2,
                        post_balance: row.3,
                    })
                    .collect(),
                resource_rev: rows.iter().map(|row| row.1).sum(),
                fee_rev: rows.iter().map(|row| row.2).sum(),
            }
            .encode(limits)
        };
        assert!(receipt(&rows).is_ok(), "the merged rows form a receipt");
        let mut duplicated = vec![("a".to_string(), 10, 1, 90), ("b".to_string(), 4, 0, 40)];
        duplicated.push(("b".to_string(), 6, 0, 40));
        assert!(
            receipt(&duplicated).is_err(),
            "the receipt rejects a second row of one purse"
        );
        let mut full = vec![("a".to_string(), u128::MAX, 0, 0)];
        assert!(merge_owner_rows(&mut full, &[("a".to_string(), 1, 0)]).is_err());
    }

    #[test]
    fn the_cursor_free_owner_settlement_burns_each_debit() {
        let totals = owner_totals(&[(1, 5), (2, 0), (3, 7)]);
        let owners = OwnerSettlement::new(&totals).expect("the owner rows form");
        assert_eq!(owners.rows().len(), 2, "a zero debit has no row");
        assert_eq!(owners.owner_rev(), 12);
        let mut request = owners
            .request(
                [4; 32],
                &fee_address(),
                Blake2b512Random::create_from_bytes(&[4]),
            )
            .expect("the owner request forms");
        let mut expected = owners
            .rows()
            .iter()
            .map(|(_, payer, debit)| {
                (
                    payer.address.to_base58(),
                    i64::try_from(*debit).expect("a small debit"),
                )
            })
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(
            request
                .allocations
                .iter()
                .map(|allocation| (allocation.address.clone(), allocation.amount))
                .collect::<Vec<_>>(),
            expected,
            "each owner purse reserves its debit"
        );
        assert_eq!(
            request
                .settlements
                .iter()
                .map(|settlement| (settlement.address.clone(), settlement.burn, settlement.fee))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|(address, debit)| (address.clone(), *debit, 0))
                .collect::<Vec<_>>(),
            "each owner purse burns its debit and pays no fee"
        );
        let cursor = request
            .env()
            .remove("sys:casper:costFeeCursor")
            .expect("the request carries its cursor transport");
        assert_eq!(
            RhoList::unapply(&cursor).expect("the transport is a list"),
            vec![RhoNil::create_par()],
            "the owner request has no cursor"
        );
        let mut identity = OWNER_SETTLEMENT_DOMAIN.to_vec();
        identity.extend_from_slice(&[4; 32]);
        assert_eq!(request.reservation_id, Blake2b256::hash(identity));
        let mut debits = BTreeMap::from([(owners.rows()[0].0, 1u128)]);
        owners
            .add_debits(&mut debits)
            .expect("the guard debits add");
        assert_eq!(debits.values().sum::<u128>(), 13);

        let huge = OwnerSettlement::new(&owner_totals(&[(1, u64::MAX)]))
            .expect("a large debit forms a row");
        assert!(
            huge.request(
                [4; 32],
                &fee_address(),
                Blake2b512Random::create_from_bytes(&[4])
            )
            .is_err(),
            "a debit beyond the vault amount range is rejected"
        );
        let mut unlisted = owner_totals(&[(1, 5)]);
        unlisted.debits.insert([0xff; 32], 1);
        unlisted.rev += 1;
        assert!(
            OwnerSettlement::new(&unlisted).is_err(),
            "a debit names a listed purse"
        );
        let mut unbilled = owner_totals(&[(1, 5)]);
        unbilled.rev = 6;
        assert!(
            OwnerSettlement::new(&unbilled).is_err(),
            "the debits sum to the billed owner REV"
        );
    }
}
