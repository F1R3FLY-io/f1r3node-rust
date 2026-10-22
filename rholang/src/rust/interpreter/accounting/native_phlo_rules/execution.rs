// Added by G3 (DR-135, S4): the owner ledger keeps its reservations by
// identity.
// Changed by G3 (DR-135, G-4): each identity keeps its reservations in order.
// use std::collections::{BTreeMap, BTreeSet};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use thiserror::Error;

use super::{
    NativePhloMeasurementError, NativePhloRegionError, NativePhloRegionLimits, NativePhloRuleError,
    NativePhloRules,
};
// Changed by G3 (DR-135, S4): the owner ledger keeps its budgets in a
// resource multiset.
// use crate::rust::interpreter::accounting::authority::AuthorityError;
use crate::rust::interpreter::accounting::authority::{AuthorityError, ResourceMultiset};
use crate::rust::interpreter::accounting::byte_receipts::{
    ByteObservation, ByteObservationSnapshot,
};
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchError;
// Added by G3 (DR-135, S4): the owner part of the meter.
// Changed by G3 (DR-135, G-4): the ledger keeps the draws of each part.
// use crate::rust::interpreter::accounting::owner_funding::{
//     allocate, OwnerCharge, OwnerNotes, OwnerPurse, OwnerShortfall, OwnerTable, UnlistedRule,
// };
use crate::rust::interpreter::accounting::owner_funding::{
    allocate, allocate_parts, OwnerCharge, OwnerDraws, OwnerNotes, OwnerPurse, OwnerShortfall,
    OwnerTable, UnlistedRule,
};
use crate::rust::interpreter::accounting::phlo_controls::{
    CheckedPhloControls, PhloScheduleBinding,
};

mod budget_trace;
// Changed by G3 (DR-135, S4): the fold also returns the owner ledger.
// pub(in crate::rust::interpreter::accounting) use budget_trace::CheckedNativeBudgetEvidence;
// Changed by G3 (DR-135, S6): replay reads the owner fold.
// pub(in crate::rust::interpreter::accounting) use budget_trace::{
//     CheckedNativeBudgetEvidence, OwnerFoldInput,
// };
pub(in crate::rust::interpreter::accounting) use budget_trace::{
    CheckedNativeBudgetEvidence, OwnerFold, OwnerFoldInput,
};
pub use budget_trace::{
    CheckedNativeBudgetTrace, NativeAttemptStage, NativeBudgetAttempt, NativeBudgetOccurrence,
    NativeBudgetReplayDecision, NativeBudgetTraceError, NativeBudgetTraceLimits, NativeDenialCause,
    NativeStackTransferEvent, NativeStackTransferKind,
};

#[derive(Debug, Error)]
pub enum NativePhloExecutionError {
    #[error(transparent)]
    Measurement(#[from] NativePhloMeasurementError),
    #[error(transparent)]
    Region(#[from] NativePhloRegionError),
    #[error(transparent)]
    Rule(#[from] NativePhloRuleError),
    #[error(transparent)]
    Authority(#[from] AuthorityError),
    #[error(transparent)]
    Work(#[from] FundingSearchError),
    #[error("native execution controls differ from the selected schedule")]
    ControlsMismatch,
    #[error("native execution charge belongs to another execution contract")]
    ContractMismatch,
    #[error("native execution charge arithmetic overflow")]
    Overflow,
    #[error("native execution exceeds the approved resource bound")]
    BoundExceeded,
    /// Added by DR-113: a signed-limit bound differs from the signed limit.
    #[error("a signed-limit resource bound must equal the signed phlo limit")]
    BoundSourceMismatch,
}

/// Added by DR-113: the source of the scalar resource bound of a native
/// execution contract. Exhaustion of the signed limit is a classified user
/// failure. Exhaustion of a certified bound is a certificate failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeBoundSource {
    /// The bound is the signed phlo limit of the offer.
    SignedLimit,
    /// The bound is a certified bound on the demand of the offer.
    Certificate,
}

#[derive(Debug)]
pub struct NativePhloExecutionContract<'a> {
    controls: CheckedPhloControls<'a>,
    policy: Arc<NativePhloExecutionPolicy>,
    allocation: Option<Arc<ResourceMultiset<[u8; 32]>>>,
}

#[derive(Debug)]
struct NativePhloExecutionPolicy {
    rules: NativePhloRules,
    weights: Vec<u64>,
    bound: u64,
    /// Added by DR-113.
    bound_source: NativeBoundSource,
}

#[derive(Clone, Debug)]
pub struct PreparedNativePhloCharge {
    policy: Arc<NativePhloExecutionPolicy>,
    observation: Arc<ByteObservation>,
    usage: u64,
    /// Added by G3 (DR-135): the usage of each region of the observation's
    /// authority, by position. The entries sum to `usage` (R4).
    region_usages: Arc<[u64]>,
}

#[derive(Debug)]
pub struct NativePhloReservation {
    policy: Arc<NativePhloExecutionPolicy>,
    used: u64,
    /// Added by G3 (DR-135, S11): the usage that the caller's bound counts:
    /// the total without the owner shares at a price of 1 or more (item 13).
    /// `used` stays the total of every granted charge.
    client_used: u64,
    /// Added by G3 (DR-135, S4): the owner ledger of a run with owner
    /// funding (R11). None keeps the meter of dev.
    owners: Option<OwnerReservation>,
}

/// Added by G3 (DR-135, G-4): a restoration that a granted peek COMM
/// reserved: its owner charge, which the COMM computed from the observation
/// of the restoration, and the draws that the COMM took for it from the
/// remaining budgets. The granted introduction of the restoration bills the
/// draws.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ReservedRestoration {
    charge: OwnerCharge,
    draws: ResourceMultiset<[u8; 32]>,
}

/// Added by G3 (DR-135, S4): the owner part of the meter (R3, R4, R8, R9).
/// It keeps the remaining budget of each purse that the owner table T lists,
/// the draws so far, the owner phlo and REV that the run took over from its
/// client, the restorations that granted peek COMMs paid for, and the
/// introductions that their owners could not pay. Play changes it in the
/// critical section of each attempt. The fold of replay rebuilds it in the
/// recorded order of the attempts.
///
/// Changed by G3 (DR-135, G-4): the remaining budgets hold the
/// reservations. The debits, the owner phlo and the owner REV are the billed
/// sums. A granted COMM draws its shares and the shares of its restorations
/// from the remaining budgets, and bills only its own shares. The granted
/// introduction of a restoration bills its reservation and draws nothing
/// again. A reservation that no granted introduction bills stays unbilled.
#[derive(Clone, Debug)]
pub(crate) struct OwnerReservation {
    table: OwnerTable,
    /// The remaining budgets S_r. A grant replaces the multiset, so a
    /// snapshot that an operation holds never changes.
    remaining: Arc<ResourceMultiset<[u8; 32]>>,
    debits: ResourceMultiset<[u8; 32]>,
    usage: u64,
    rev: u128,
    // Changed by G3 (DR-135, G-4): each reservation keeps its charge and its
    // draws, in the order of the grants.
    // /// The restorations that granted peek COMMs paid for, counted by their
    // /// introduction identity (R9). Two peeks of equal data restore two data
    // /// with one identity.
    // reserved: BTreeMap<[u8; 32], usize>,
    /// The restorations that granted peek COMMs reserved and that no granted
    /// introduction billed yet, by their introduction identity, in the order
    /// of the grants (R9). Two peeks of equal data restore two data with one
    /// identity.
    reserved: BTreeMap<[u8; 32], VecDeque<ReservedRestoration>>,
    refused: BTreeSet<[u8; 32]>,
}

impl OwnerReservation {
    /// The ledger at the start of a run: each listed purse has its budget.
    pub(crate) fn new(table: OwnerTable) -> Self {
        let mut remaining = BTreeMap::new();
        for (custody, entry) in table.entries() {
            if entry.budget > 0 {
                remaining.insert(*custody, entry.budget);
            }
        }
        Self {
            table,
            remaining: Arc::new(ResourceMultiset(remaining)),
            debits: ResourceMultiset::default(),
            usage: 0,
            rev: 0,
            reserved: BTreeMap::new(),
            refused: BTreeSet::new(),
        }
    }

    pub(crate) fn table(&self) -> &OwnerTable { &self.table }

    /// The remaining budgets S_r of the listed purses. Later grants do not
    /// change the snapshot.
    pub(crate) fn snapshot(&self) -> Arc<ResourceMultiset<[u8; 32]>> { Arc::clone(&self.remaining) }

    /// The remaining budget of a listed purse, or None for an unlisted purse.
    pub(crate) fn remaining(&self, custody: &[u8; 32]) -> Option<u64> {
        self.table.get(custody).map(|_| self.remaining.get(custody))
    }

    /// Allocates `charge` against the remaining budgets, with `rule` for an
    /// unlisted purse (SA8).
    pub(crate) fn allocate(
        &self,
        charge: &OwnerCharge,
        rule: UnlistedRule,
        notes: Option<&OwnerNotes>,
    ) -> Result<ResourceMultiset<[u8; 32]>, OwnerShortfall> {
        let view = |purse: &OwnerPurse| self.remaining(&purse.custody);
        allocate(charge, &view, rule, self.table.has_room(), notes)
    }

    /// Added by G3 (DR-135, G-4): `allocate` with the draws of each part of
    /// the charge (`OwnerDraws`).
    pub(crate) fn allocate_parts(
        &self,
        charge: &OwnerCharge,
        rule: UnlistedRule,
        notes: Option<&OwnerNotes>,
    ) -> Result<OwnerDraws, OwnerShortfall> {
        let view = |purse: &OwnerPurse| self.remaining(&purse.custody);
        allocate_parts(charge, &view, rule, self.table.has_room(), notes)
    }

    // Changed by G3 (DR-135, G-4): `grant` deducts the total, bills the main
    // part and reserves the restorations with their draws.
    // /// Deducts the draws of a granted charge and adds the owner phlo and REV
    // /// of the charge. On an error nothing changes.
    // pub(crate) fn apply(
    //     &mut self,
    //     draws: &ResourceMultiset<[u8; 32]>,
    //     charge: &OwnerCharge,
    // ) -> Result<(), AuthorityError> {
    //     let (usage, rev) = charge.totals()?;
    //     let usage = self
    //         .usage
    //         .checked_add(usage)
    //         .ok_or(AuthorityError::ArithmeticOverflow)?;
    //     let rev = self
    //         .rev
    //         .checked_add(rev)
    //         .ok_or(AuthorityError::ArithmeticOverflow)?;
    //     let debits = self.debits.checked_add(draws)?;
    //     let remaining = self.remaining.checked_sub(draws)?;
    //     self.remaining = Arc::new(remaining);
    //     self.debits = debits;
    //     self.usage = usage;
    //     self.rev = rev;
    //     Ok(())
    // }
    //
    // /// Reserves the restorations of a granted COMM charge (R9).
    // pub(crate) fn reserve_restorations(&mut self, charge: &OwnerCharge) {
    //     for (identity, _) in &charge.restorations {
    //         let count = self.reserved.entry(*identity).or_insert(0);
    //         *count = count.saturating_add(1);
    //     }
    // }
    //
    // /// Takes one reservation of the restoration with `identity`, when a
    // /// granted peek COMM paid for it (R9).
    // pub(crate) fn take_reservation(&mut self, identity: &[u8; 32]) -> bool {
    //     match self.reserved.get_mut(identity) {
    //         None => false,
    //         Some(count) if *count > 1 => {
    //             *count -= 1;
    //             true
    //         }
    //         Some(_) => {
    //             self.reserved.remove(identity);
    //             true
    //         }
    //     }
    // }
    //
    // /// Added by G3 (DR-135, S4 review): whether a granted peek COMM paid for
    // /// a restoration with `identity` that no introduction has taken yet (R9).
    // pub(crate) fn is_reserved(&self, identity: &[u8; 32]) -> bool {
    //     self.reserved.contains_key(identity)
    // }

    /// Added by G3 (DR-135, G-4): a granted charge with the draws `draws`
    /// that `allocate_parts` gave for it. The remaining budgets lose the
    /// total draw. The debits gain the main draws, and the owner phlo and
    /// REV gain the share totals of the main charge. Each restoration of the
    /// charge gets one reservation with its charge and its draws (R9). On an
    /// error nothing changes.
    pub(crate) fn grant(
        &mut self,
        draws: &OwnerDraws,
        charge: &OwnerCharge,
    ) -> Result<(), AuthorityError> {
        if draws.restorations.len() != charge.restorations.len() {
            return Err(AuthorityError::AllocationMismatch);
        }
        let (usage, rev) = charge.share_totals()?;
        let usage = self
            .usage
            .checked_add(usage)
            .ok_or(AuthorityError::ArithmeticOverflow)?;
        let rev = self
            .rev
            .checked_add(rev)
            .ok_or(AuthorityError::ArithmeticOverflow)?;
        let debits = self.debits.checked_add(&draws.main)?;
        let remaining = self.remaining.checked_sub(&draws.total)?;
        self.remaining = Arc::new(remaining);
        self.debits = debits;
        self.usage = usage;
        self.rev = rev;
        for ((identity, restoration), part) in charge.restorations.iter().zip(&draws.restorations) {
            self.reserved
                .entry(*identity)
                .or_default()
                .push_back(ReservedRestoration {
                    charge: restoration.clone(),
                    draws: part.clone(),
                });
        }
        Ok(())
    }

    /// Added by G3 (DR-135, G-4): the position of the first reservation of
    /// the restoration with `identity` whose charge is `charge`, the owner
    /// charge that the introduction computes from its own observation (SA7).
    fn reservation_position(&self, identity: &[u8; 32], charge: &OwnerCharge) -> Option<usize> {
        self.reserved
            .get(identity)?
            .iter()
            .position(|reserved| reserved.charge == *charge)
    }

    /// Added by G3 (DR-135, G-4): whether an introduction with `identity` and
    /// the owner charge `charge` restores a datum that a granted peek COMM
    /// reserved (R9). Such an introduction draws nothing and is never
    /// refused by its owner.
    pub(crate) fn is_reserved_for(&self, identity: &[u8; 32], charge: &OwnerCharge) -> bool {
        self.reservation_position(identity, charge).is_some()
    }

    /// Added by G3 (DR-135, G-4): bills the granted introduction of a
    /// reserved restoration. It takes the first reservation of `identity`
    /// with the charge `charge`. The debits gain the reserved draws, and the
    /// owner phlo and REV gain the share totals of `charge`. The remaining
    /// budgets do not change. It returns false, with nothing changed, when no
    /// such reservation exists. On an error nothing changes.
    pub(crate) fn bill_reservation(
        &mut self,
        identity: &[u8; 32],
        charge: &OwnerCharge,
    ) -> Result<bool, AuthorityError> {
        let Some(position) = self.reservation_position(identity, charge) else {
            return Ok(false);
        };
        let Some(reserved) = self
            .reserved
            .get(identity)
            .and_then(|queue| queue.get(position))
        else {
            return Ok(false);
        };
        let (usage, rev) = charge.share_totals()?;
        let usage = self
            .usage
            .checked_add(usage)
            .ok_or(AuthorityError::ArithmeticOverflow)?;
        let rev = self
            .rev
            .checked_add(rev)
            .ok_or(AuthorityError::ArithmeticOverflow)?;
        let debits = self.debits.checked_add(&reserved.draws)?;
        let emptied = match self.reserved.get_mut(identity) {
            Some(queue) => {
                queue.remove(position);
                queue.is_empty()
            }
            None => false,
        };
        if emptied {
            self.reserved.remove(identity);
        }
        self.debits = debits;
        self.usage = usage;
        self.rev = rev;
        Ok(true)
    }

    /// Records that the owner of the introduction with `identity` could not
    /// pay (R8).
    pub(crate) fn refuse(&mut self, identity: [u8; 32]) { self.refused.insert(identity); }

    /// Whether the owner of the introduction with `identity` could not pay
    /// (SA5).
    pub(crate) fn is_refused(&self, identity: &[u8; 32]) -> bool { self.refused.contains(identity) }

    pub(crate) fn debits(&self) -> &ResourceMultiset<[u8; 32]> { &self.debits }

    pub(crate) fn usage(&self) -> u64 { self.usage }

    pub(crate) fn rev(&self) -> u128 { self.rev }
}

#[derive(Clone, Debug)]
pub struct NativePhloChargePreparer {
    policy: Arc<NativePhloExecutionPolicy>,
}

impl<'a> NativePhloExecutionContract<'a> {
    // Changed by DR-113: the contract names the source of its bound.
    // pub fn new(
    //     controls: CheckedPhloControls<'a>,
    //     binding: &PhloScheduleBinding<'_>,
    // ) -> Result<Self, NativePhloExecutionError> {
    pub fn new(
        controls: CheckedPhloControls<'a>,
        binding: &PhloScheduleBinding<'_>,
        bound_source: NativeBoundSource,
    ) -> Result<Self, NativePhloExecutionError> {
        if controls.schedule() != binding.schedule() {
            return Err(NativePhloExecutionError::ControlsMismatch);
        }
        // Added by DR-113: a signed-limit bound is the signed limit itself.
        // A certified bound is at most the limit, as `check_controls`
        // already requires.
        if bound_source == NativeBoundSource::SignedLimit
            && controls.resource_bound() != controls.terms().limit
        {
            return Err(NativePhloExecutionError::BoundSourceMismatch);
        }
        let rules = NativePhloRules::resolve(binding.descriptor())?;
        let mut weights = Vec::new();
        weights
            .try_reserve_exact(controls.schedule().weights.len())
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        weights.extend_from_slice(controls.schedule().weights);
        Ok(Self {
            controls,
            policy: Arc::new(NativePhloExecutionPolicy {
                rules,
                weights,
                bound: controls.resource_bound(),
                bound_source,
            }),
            allocation: None,
        })
    }

    pub fn with_authority_allocation(mut self, allocation: ResourceMultiset<[u8; 32]>) -> Self {
        self.allocation = Some(Arc::new(allocation));
        self
    }

    pub fn authority_allocation(&self) -> Option<&ResourceMultiset<[u8; 32]>> {
        self.allocation.as_deref()
    }

    pub fn controls(&self) -> CheckedPhloControls<'a> { self.controls }

    /// Added by DR-113.
    pub fn bound_source(&self) -> NativeBoundSource { self.policy.bound_source }

    pub fn reservation(self) -> NativePhloReservation {
        NativePhloReservation {
            policy: self.policy,
            used: 0,
            // Added by G3 (DR-135, S11).
            client_used: 0,
            // Added by G3 (DR-135, S4).
            owners: None,
        }
    }

    pub fn prepare(
        &self,
        observation: Arc<ByteObservation>,
        limits: NativePhloRegionLimits,
    ) -> Result<PreparedNativePhloCharge, NativePhloExecutionError> {
        self.policy.prepare(observation, limits)
    }
}

impl NativePhloExecutionPolicy {
    fn prepare(
        self: &Arc<Self>,
        observation: Arc<ByteObservation>,
        limits: NativePhloRegionLimits,
    ) -> Result<PreparedNativePhloCharge, NativePhloExecutionError> {
        let mut rows = Vec::new();
        rows.try_reserve_exact(1)
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        rows.push(Arc::clone(&observation));
        let snapshot = ByteObservationSnapshot {
            rows,
            metered_context: true,
            history_lost: false,
        };
        let measured = self.rules.measure(&snapshot, 1)?;
        let regions = measured.region_demands(limits)?;
        let mut usage = 0_u64;
        // Added by G3 (DR-135): the usage of each region position (R4).
        let mut region_usages = Vec::new();
        region_usages
            .try_reserve_exact(observation.authority.regions.len())
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        region_usages.resize(observation.authority.regions.len(), 0_u64);
        // Changed by G3 (DR-135): the loop also keeps the position of each
        // region.
        // for demand in regions.occurrences() {
        for (index, demand) in regions.indexed_occurrences() {
            let signature = demand
                .region()
                .signature
                .as_ref()
                .ok_or(AuthorityError::MissingSignature)?;
            let units = authority_units(signature)?;
            let measurement = demand.measurement();
            let weight = self.weights[measurement.class()];
            // Changed by DR-113: a zero factor gives an exact zero charge, so
            // an overflow below means that the exact charge exceeds u64::MAX
            // and therefore every bound.
            // usage = weight
            //     .checked_mul(units)
            //     .and_then(|amount| amount.checked_mul(measurement.quantity()))
            //     .and_then(|amount| usage.checked_add(amount))
            //     .ok_or(NativePhloExecutionError::Overflow)?;
            let amount = if weight == 0 || units == 0 || measurement.quantity() == 0 {
                0
            } else {
                weight
                    .checked_mul(units)
                    .and_then(|amount| amount.checked_mul(measurement.quantity()))
                    .ok_or(NativePhloExecutionError::Overflow)?
            };
            usage = usage
                .checked_add(amount)
                .ok_or(NativePhloExecutionError::Overflow)?;
            // Added by G3 (DR-135): each region amount is at most `usage`,
            // which did not overflow, so the sum per region does not either.
            let slot = region_usages
                .get_mut(index)
                .ok_or(AuthorityError::MissingAuthority)?;
            *slot = slot
                .checked_add(amount)
                .ok_or(NativePhloExecutionError::Overflow)?;
        }
        Ok(PreparedNativePhloCharge {
            policy: Arc::clone(self),
            observation,
            usage,
            region_usages: Arc::from(region_usages),
        })
    }
}

impl PreparedNativePhloCharge {
    pub fn usage(&self) -> u64 { self.usage }
    pub fn observation(&self) -> &Arc<ByteObservation> { &self.observation }

    /// Added by G3 (DR-135): the usage of each region of the observation's
    /// authority, by position (R4).
    pub fn region_usages(&self) -> &[u64] { &self.region_usages }
}

impl NativePhloReservation {
    pub fn used(&self) -> u64 { self.used }

    /// Added by G3 (DR-135, S11): the usage that the caller's bound counts
    /// (item 13).
    pub fn client_used(&self) -> u64 { self.client_used }

    /// Added by G3 (DR-135, S4): installs the owner ledger of a run with
    /// owner funding (R11).
    pub(crate) fn set_owners(&mut self, owners: OwnerReservation) { self.owners = Some(owners); }

    /// Added by G3 (DR-135, S4): the owner ledger, when the run has owner
    /// funding.
    pub(crate) fn owners(&self) -> Option<&OwnerReservation> { self.owners.as_ref() }

    /// Added by G3 (DR-135, S4).
    pub(crate) fn owners_mut(&mut self) -> Option<&mut OwnerReservation> { self.owners.as_mut() }

    /// Added by DR-113.
    pub fn bound_source(&self) -> NativeBoundSource { self.policy.bound_source }

    pub fn preparer(&self) -> NativePhloChargePreparer {
        NativePhloChargePreparer {
            policy: Arc::clone(&self.policy),
        }
    }

    pub fn prepare(
        &self,
        observation: Arc<ByteObservation>,
        limits: NativePhloRegionLimits,
    ) -> Result<PreparedNativePhloCharge, NativePhloExecutionError> {
        self.policy.prepare(observation, limits)
    }

    // Changed by G3 (DR-135, S11): a charge with no owner usage, so the
    // bound counts the whole charge, as before (item 13). The replaced body
    // follows.
    // pub fn reserve(
    //     &mut self,
    //     charge: &PreparedNativePhloCharge,
    // ) -> Result<(), NativePhloExecutionError> {
    //     if !Arc::ptr_eq(&self.policy, &charge.policy) {
    //         return Err(NativePhloExecutionError::ContractMismatch);
    //     }
    //     reserve_native_usage(&mut self.used, self.policy.bound, charge.usage)
    // }
    pub fn reserve(
        &mut self,
        charge: &PreparedNativePhloCharge,
    ) -> Result<(), NativePhloExecutionError> {
        self.reserve_with_owner_usage(charge, 0)
    }

    /// Added by G3 (DR-135, S11): reserves `charge`, of which the owners pay
    /// `owner_usage`: the usage of its owner shares at a price of 1 or more
    /// (item 13). The caller's bound counts only the client part,
    /// `charge.usage - owner_usage`, and `used` counts the whole charge. An
    /// error changes nothing.
    pub fn reserve_with_owner_usage(
        &mut self,
        charge: &PreparedNativePhloCharge,
        owner_usage: u64,
    ) -> Result<(), NativePhloExecutionError> {
        if !Arc::ptr_eq(&self.policy, &charge.policy) {
            return Err(NativePhloExecutionError::ContractMismatch);
        }
        let used = self
            .used
            .checked_add(charge.usage)
            .ok_or(NativePhloExecutionError::Overflow)?;
        let client = charge
            .usage
            .checked_sub(owner_usage)
            .ok_or(NativePhloExecutionError::Overflow)?;
        let mut client_used = self.client_used;
        reserve_native_usage(&mut client_used, self.policy.bound, client)?;
        self.used = used;
        self.client_used = client_used;
        Ok(())
    }
}

impl NativePhloChargePreparer {
    /// Added by DR-113.
    pub fn bound_source(&self) -> NativeBoundSource { self.policy.bound_source }

    pub fn prepare(
        &self,
        observation: Arc<ByteObservation>,
        limits: NativePhloRegionLimits,
    ) -> Result<PreparedNativePhloCharge, NativePhloExecutionError> {
        self.policy.prepare(observation, limits)
    }
}

fn reserve_native_usage(
    used: &mut u64,
    limit: u64,
    charge: u64,
) -> Result<(), NativePhloExecutionError> {
    let next = used
        .checked_add(charge)
        .ok_or(NativePhloExecutionError::Overflow)?;
    if next > limit {
        return Err(NativePhloExecutionError::BoundExceeded);
    }
    *used = next;
    Ok(())
}

fn authority_units(signature: &CostSignature) -> Result<u64, NativePhloExecutionError> {
    // Moved by dedup (task 11415) to `signature_leaf_units` in this file. The
    // native meter keeps its charges in `NativeUnitsMeter` below.
    // let mut pending = Vec::new();
    // reserve_work(
    //     budget,
    //     HostWorkDimension::SearchStateBytes,
    //     size_of::<&CostSignature>(),
    // )?;
    // pending
    //     .try_reserve_exact(1)
    //     .map_err(|_| FundingSearchError::AllocationFailed)?;
    // pending.push(signature);
    // let mut units = 0_u64;
    // while let Some(current) = pending.pop() {
    //     reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
    //     match current.value.as_ref() {
    //         Some(Value::Unit(true)) => {}
    //         Some(Value::Ground(_) | Value::Name(_) | Value::Quote(_)) => {
    //             // DR-113: this overflow needs more than 2^64 signature
    //             // leaves, and each leaf reserves one verification operation
    //             // first, so the host-work budget rejects long before it.
    //             units = units
    //                 .checked_add(1)
    //                 .ok_or(NativePhloExecutionError::Overflow)?;
    //         }
    //         Some(Value::Compound(compound)) => {
    //             reserve_work(
    //                 budget,
    //                 HostWorkDimension::SearchStateBytes,
    //                 // Changed by DR-113: a sizing overflow is host work, never
    //                 // a charge.
    //                 // compound
    //                 //     .elements
    //                 //     .len()
    //                 //     .checked_mul(size_of::<&CostSignature>())
    //                 //     .ok_or(NativePhloExecutionError::Overflow)?,
    //                 compound
    //                     .elements
    //                     .len()
    //                     .checked_mul(size_of::<&CostSignature>())
    //                     .ok_or(FundingSearchError::AllocationFailed)?,
    //             )?;
    //             pending
    //                 .try_reserve(compound.elements.len())
    //                 .map_err(|_| FundingSearchError::AllocationFailed)?;
    //             pending.extend(&compound.elements);
    //         }
    //         _ => return Err(AuthorityError::UnsupportedFundingSignature.into()),
    //     }
    // }
    // Ok(units)
    signature_leaf_units(signature, &NativeUnitsMeter)
}

/// Added by dedup (task 11415, D8): the policy of a walk over the leaves of a
/// cost signature. Changed by DR-131: no policy charges host work. The native
/// policy rejects an unsupported node and checks the unit count for overflow.
/// The estimate accepts the node and saturates the count.
pub(crate) trait SignatureUnitsMeter {
    type Error;
    /// A node that is not a leaf, not a compound and not the true unit.
    fn other(&self) -> Result<(), Self::Error>;
    /// The units after one more leaf.
    fn add(&self, units: u64) -> Result<u64, Self::Error>;
    /// The error of a failed allocation of the stack.
    fn allocation(&self) -> Self::Error;
}

/// Added by dedup (task 11415, D8): the units of a lane, the number of Ground,
/// Name and Quote leaves of its signature. The walk uses a heap stack
/// (DR-67), so its stack depth does not grow with the nesting of the
/// signature. The native meter (`authority_units`) and the unit function of
/// the estimate (`signature_units` in `delta_sigma/estimate.rs`) share it.
pub(crate) fn signature_leaf_units<M: SignatureUnitsMeter>(
    signature: &CostSignature,
    meter: &M,
) -> Result<u64, M::Error> {
    let mut pending = Vec::new();
    pending
        .try_reserve_exact(1)
        .map_err(|_| meter.allocation())?;
    pending.push(signature);
    let mut units = 0_u64;
    while let Some(current) = pending.pop() {
        match current.value.as_ref() {
            Some(Value::Unit(true)) => {}
            Some(Value::Ground(_) | Value::Name(_) | Value::Quote(_)) => {
                units = meter.add(units)?;
            }
            Some(Value::Compound(compound)) => {
                pending
                    .try_reserve(compound.elements.len())
                    .map_err(|_| meter.allocation())?;
                pending.extend(&compound.elements);
            }
            _ => meter.other()?,
        }
    }
    Ok(units)
}

/// Added by dedup (task 11415, D8): the errors of `authority_units`, as the
/// native meter reported them before the walk moved.
struct NativeUnitsMeter;

impl SignatureUnitsMeter for NativeUnitsMeter {
    type Error = NativePhloExecutionError;

    fn other(&self) -> Result<(), Self::Error> {
        Err(AuthorityError::UnsupportedFundingSignature.into())
    }

    fn add(&self, units: u64) -> Result<u64, Self::Error> {
        // DR-113: this overflow needs more than 2^64 signature leaves, more
        // than any signature in memory holds.
        units
            .checked_add(1)
            .ok_or(NativePhloExecutionError::Overflow)
    }

    fn allocation(&self) -> Self::Error { FundingSearchError::AllocationFailed.into() }
}

#[cfg(test)]
mod tests;
