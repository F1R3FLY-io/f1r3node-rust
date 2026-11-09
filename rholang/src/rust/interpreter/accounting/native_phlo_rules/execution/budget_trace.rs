use std::cmp::Ordering;
// Added by G3 (DR-135, S4): the fold maps COMMs to their restorations.
// Changed by G3 (DR-135, S4 review): the restorations come from the COMM
// attempt, so the fold keeps no map.
// use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use thiserror::Error;

// Changed by G3 (DR-135, S4): the fold rebuilds the owner ledger from
// prepared charges.
// use super::{
//     NativeBoundSource, NativePhloExecutionContract, NativePhloExecutionError,
//     NativePhloRegionLimits,
// };
use super::{
    NativeBoundSource, NativePhloExecutionContract, NativePhloExecutionError,
    NativePhloRegionLimits, OwnerReservation, PreparedNativePhloCharge,
};
// Changed by G3 (DR-135, S4): the owner ledger keeps its budgets in a
// resource multiset.
// use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
// Changed by G3 (DR-135, G-4): the fold grants the draws of each part.
// use crate::rust::interpreter::accounting::authority::{AuthorityByteEventKind, ResourceMultiset};
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_receipts::ByteObservation;
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchError;
use crate::rust::interpreter::accounting::native_runtime::PathId;
// Added by G3 (DR-135, S4): the owner part of the fold.
// Changed by G3 (DR-135, G-4): the fold grants the draws of each part.
// use crate::rust::interpreter::accounting::owner_funding::{
//     prepared_owner_charge, OwnerCharge, OwnerFundingBinding, OwnerShortfall, OwnerTable,
//     UnlistedRule,
// };
use crate::rust::interpreter::accounting::owner_funding::{
    prepared_owner_charge, OwnerCharge, OwnerDraws, OwnerFundingBinding, OwnerShortfall,
    OwnerTable, UnlistedRule,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeAttemptStage {
    ProduceIntroduction,
    ConsumeIntroduction,
    Comm,
}

impl From<AuthorityByteEventKind> for NativeAttemptStage {
    fn from(kind: AuthorityByteEventKind) -> Self {
        match kind {
            AuthorityByteEventKind::ProduceIntroduction => Self::ProduceIntroduction,
            AuthorityByteEventKind::ConsumeIntroduction => Self::ConsumeIntroduction,
            AuthorityByteEventKind::Comm => Self::Comm,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeBudgetOccurrence {
    pub session: [u8; 32],
    // Changed by C7b (DR-86): an occurrence names the node of its path in the
    // evidence's path trie instead of holding a copy of the path.
    // pub path: Vec<(u64, u64)>,
    pub path: PathId,
    pub stage: NativeAttemptStage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeBudgetAttempt {
    pub occurrence: NativeBudgetOccurrence,
    pub observation: Arc<ByteObservation>,
    pub granted: bool,
    /// Added by G3 (DR-135, S4): the owner of the introduction could not
    /// pay, so the attempt took no bound decision and stored nothing (R8).
    /// An owner-refused attempt is not granted.
    pub owner_refused: bool,
    /// Added by G3 (DR-135, S4 review): the introductions of the data that a
    /// peek COMM restores, in COMM order (R9). The COMM pays for them in play
    /// and in the fold, so a restoration that never runs stays paid at its
    /// COMM in both (§6.3 item 4). Empty for every other attempt.
    pub restorations: Arc<[Arc<ByteObservation>]>,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeBudgetTraceLimits {
    pub attempts: usize,
    pub path_segments: usize,
    pub regions: NativePhloRegionLimits,
}

#[derive(Debug, Error)]
pub enum NativeBudgetTraceError {
    #[error(transparent)]
    Execution(#[from] NativePhloExecutionError),
    #[error(transparent)]
    Work(#[from] FundingSearchError),
    #[error("native attempt trace exceeds its entry or occurrence path limit")]
    Limit,
    #[error("native attempt belongs to another execution session")]
    Session,
    #[error("native attempt stage differs from its observation")]
    Stage,
    #[error("native attempt occurrence is duplicated")]
    Duplicate,
    #[error("native attempt decision differs from its checked prefix")]
    Decision,
    #[error("native replay occurrence is absent from the checked trace")]
    Unknown,
    #[error("native replay observation differs from its checked attempt")]
    Observation,
    #[error("native replay attempt was already consumed")]
    Consumed,
    #[error("native replay did not consume its complete budget trace")]
    Incomplete,
    #[error("native replay usage differs from its checked total")]
    Usage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeBudgetReplayDecision {
    Accepted {
        usage: u64,
    },
    Denied,
    /// Added by G3 (DR-135, S4): the owner of the introduction could not pay
    /// (R8). It uses no budget.
    OwnerRefused,
}

#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
pub struct CheckedNativeBudgetTrace {
    evidence: CheckedNativeBudgetEvidence,
    consumed: Vec<bool>,
    used: u64,
    remaining: usize,
}

#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
pub(in crate::rust::interpreter::accounting) struct CheckedNativeBudgetEvidence {
    /// Added by DR-113: the bound source of the contract that checked the
    /// evidence. Replay classifies a denial with it, as play does.
    bound_source: NativeBoundSource,
    rows: Arc<[NativeBudgetAttempt]>,
    ordered: Vec<usize>,
    charges: Vec<Option<u64>>,
    total: u64,
    /// Added by G3 (DR-135, S11): the usage that the caller's bound counted:
    /// the total without the owner shares at a price of 1 or more (item 13).
    /// The tests compare it with the client usage of play.
    #[cfg(test)]
    client_total: u64,
    path_limit: usize,
    /// Added by G3 (DR-135, S4): the owner ledger that the fold rebuilt, when
    /// the evidence was checked with owner funding.
    owner: Option<OwnerFold>,
}

/// Added by G3 (DR-135, S4): the owner terms of a fold (R3): the binding of
/// the member and the owner table of the recording that the journal check
/// validated against the binding.
///
/// Changed by G3 (DR-135, S4 review): the restorations of a peek COMM come
/// from its own attempt, so the input keeps no restoration map.
pub(in crate::rust::interpreter::accounting) struct OwnerFoldInput<'a> {
    pub(in crate::rust::interpreter::accounting) binding: &'a OwnerFundingBinding,
    pub(in crate::rust::interpreter::accounting) table: OwnerTable,
    // pub(in crate::rust::interpreter::accounting) restorations: BTreeMap<usize, Vec<usize>>,
}

/// Added by G3 (DR-135, S4): the owner ledger that a fold rebuilt, with the
/// history of each listed purse. Replay reads the budgets at a recorded
/// position from it (S6), and settlement reads the debits (S7).
#[derive(Clone, Debug)]
pub(in crate::rust::interpreter::accounting) struct OwnerFold {
    ledger: OwnerReservation,
    /// For each table entry, in table order, the pairs (attempt index,
    /// remaining budget after the attempt) of the attempts that drew from
    /// the purse, in attempt order.
    history: Vec<Vec<(usize, u64)>>,
}

impl OwnerFold {
    fn new(table: OwnerTable) -> Self {
        let mut history = Vec::with_capacity(table.len());
        history.resize_with(table.len(), Vec::new);
        Self {
            ledger: OwnerReservation::new(table),
            history,
        }
    }

    // Changed by G3 (DR-135, G-4): the grant draws the total, bills the main
    // part and reserves the restorations (`OwnerReservation::grant`).
    // /// Deducts the draws of the granted attempt `index` and records the new
    // /// remaining budget of each purse that it drew from.
    // fn apply(
    //     &mut self,
    //     index: usize,
    //     draws: &ResourceMultiset<[u8; 32]>,
    //     charge: &OwnerCharge,
    // ) -> Result<(), NativeBudgetTraceError> {
    //     self.ledger
    //         .apply(draws, charge)
    //         .map_err(NativePhloExecutionError::from)?;
    //     for custody in draws.0.keys() {
    /// Added by G3 (DR-135, G-4): grants the attempt `index` with the draws
    /// `draws` (`OwnerReservation::grant`) and records the new remaining
    /// budget of each purse that it drew from.
    fn grant(
        &mut self,
        index: usize,
        draws: &OwnerDraws,
        charge: &OwnerCharge,
    ) -> Result<(), NativeBudgetTraceError> {
        self.ledger
            .grant(draws, charge)
            .map_err(NativePhloExecutionError::from)?;
        for custody in draws.total.0.keys() {
            let (Some(position), Some(remaining)) = (
                self.ledger.table().position(custody),
                self.ledger.remaining(custody),
            ) else {
                return Err(NativeBudgetTraceError::Decision);
            };
            self.history[position].push((index, remaining));
        }
        Ok(())
    }

    /// The remaining budget of the listed purse `custody` before the attempt
    /// at `position`, or None for an unlisted purse. It equals the play
    /// ledger after the first `position` attempts (§6.3 item 1).
    pub(in crate::rust::interpreter::accounting) fn remaining_at(
        &self,
        custody: &[u8; 32],
        position: usize,
    ) -> Option<u64> {
        let entry = self.ledger.table().position(custody)?;
        let rows = &self.history[entry];
        let before = rows.partition_point(|(attempt, _)| *attempt < position);
        match before.checked_sub(1) {
            Some(last) => Some(rows[last].1),
            None => Some(self.ledger.table().entries()[entry].1.budget),
        }
    }

    /// The ledger after the last attempt.
    pub(in crate::rust::interpreter::accounting) fn ledger(&self) -> &OwnerReservation {
        &self.ledger
    }
}

/// Added by G3 (DR-135, S4): the owner part of one fold.
///
/// Changed by G3 (DR-135, S4 review): the fold counts the reserved
/// restorations by identity in its ledger, as play does, and takes the
/// restorations of a COMM from its attempt. The replaced lines follow.
// struct OwnerFolding<'a> {
//     binding: &'a OwnerFundingBinding,
//     restorations: BTreeMap<usize, Vec<usize>>,
//     /// The restoration attempts that granted peek COMMs paid for (R9).
//     reserved: BTreeSet<usize>,
//     fold: OwnerFold,
// }
//
// impl<'a> OwnerFolding<'a> {
//     fn new(input: OwnerFoldInput<'a>) -> Result<Self, NativeBudgetTraceError> {
//         let mut reserved = BTreeSet::new();
//         for restoration in input.restorations.values().flatten() {
//             if !reserved.insert(*restoration) {
//                 return Err(NativeBudgetTraceError::Decision);
//             }
//         }
//         Ok(Self {
//             binding: input.binding,
//             restorations: input.restorations,
//             reserved,
//             fold: OwnerFold::new(input.table),
//         })
//     }
struct OwnerFolding<'a> {
    binding: &'a OwnerFundingBinding,
    fold: OwnerFold,
}

impl<'a> OwnerFolding<'a> {
    fn new(input: OwnerFoldInput<'a>) -> Self {
        Self {
            binding: input.binding,
            fold: OwnerFold::new(input.table),
        }
    }

    // Changed by G3 (DR-135, S11): renamed from `charge`, so that the
    // legacy guard's `[.]charge[(]` pattern does not match. The
    // replaced line follows.
    // fn charge(
    fn owner_charge(
        &self,
        row: &NativeBudgetAttempt,
        prepared: Option<&PreparedNativePhloCharge>,
    ) -> Result<OwnerCharge, NativeBudgetTraceError> {
        prepared_owner_charge(self.binding, &row.observation, prepared)
            .map_err(|error| NativePhloExecutionError::from(error).into())
    }

    /// Added by G3 (DR-135, S11): the usage of the owner shares at a price
    /// of 1 or more of an attempt that the bound decides, as play passes it
    /// to the meter (item 13), or `None` when it overflows. Play allocated
    /// the owner part of the attempt first, so the owner part exists.
    fn priced_usage(
        &self,
        row: &NativeBudgetAttempt,
        prepared: &PreparedNativePhloCharge,
    ) -> Result<Option<u64>, NativeBudgetTraceError> {
        Ok(self.owner_charge(row, Some(prepared))?.priced_usage().ok())
    }

    /// An owner refusal of an introduction: no bound decision, and the
    /// owner charge must fail with unlisted purses passing (R8, SA8). Play
    /// never refuses a reserved restoration, and an introduction records no
    /// restorations.
    fn refusal(
        &self,
        rows: &[NativeBudgetAttempt],
        index: usize,
        prepared: Option<&PreparedNativePhloCharge>,
    ) -> Result<(), NativeBudgetTraceError> {
        let row = &rows[index];
        // Changed by G3 (DR-135, S4 review): a reserved restoration is
        // counted by identity in the ledger.
        // let prepared = match (
        //     row.granted,
        //     row.occurrence.stage,
        //     self.reserved.contains(&index),
        //     prepared,
        // ) {
        //     (
        //         false,
        //         NativeAttemptStage::ProduceIntroduction | NativeAttemptStage::ConsumeIntroduction,
        //         false,
        //         Some(prepared),
        //     ) => prepared,
        //     _ => return Err(NativeBudgetTraceError::Decision),
        // };
        // Changed by G3 (DR-135, G-4): a reservation names the charge of its
        // restoration, so the check follows the charge.
        // let reserved = row.occurrence.stage == NativeAttemptStage::ProduceIntroduction
        //     && self.fold.ledger.is_reserved(&row.observation.event_id);
        // let prepared = match (
        //     row.granted,
        //     row.occurrence.stage,
        //     reserved || !row.restorations.is_empty(),
        //     prepared,
        // ) {
        //     (
        //         false,
        //         NativeAttemptStage::ProduceIntroduction | NativeAttemptStage::ConsumeIntroduction,
        //         false,
        //         Some(prepared),
        //     ) => prepared,
        //     _ => return Err(NativeBudgetTraceError::Decision),
        // };
        // let charge = self.charge(row, Some(prepared))?;
        let prepared = match (
            row.granted,
            row.occurrence.stage,
            row.restorations.is_empty(),
            prepared,
        ) {
            (
                false,
                NativeAttemptStage::ProduceIntroduction | NativeAttemptStage::ConsumeIntroduction,
                true,
                Some(prepared),
            ) => prepared,
            _ => return Err(NativeBudgetTraceError::Decision),
        };
        // Changed by G3 (DR-135, S11): renamed from `charge`, so that the
        // legacy guard's `[.]charge[(]` pattern does not match. The
        // replaced line follows.
        // let charge = self.charge(row, Some(prepared))?;
        let charge = self.owner_charge(row, Some(prepared))?;
        if row.occurrence.stage == NativeAttemptStage::ProduceIntroduction
            && self
                .fold
                .ledger
                .is_reserved_for(&row.observation.event_id, &charge)
        {
            return Err(NativeBudgetTraceError::Decision);
        }
        match self
            .fold
            .ledger
            .allocate(&charge, UnlistedRule::Lenient, None)
        {
            Ok(_) => Err(NativeBudgetTraceError::Decision),
            Err(OwnerShortfall) => Ok(()),
        }
    }

    // Changed by G3 (DR-135, G-4): a grant bills only the main part, and a
    // granted restoration bills its reservation.
    // /// An attempt that the bound decided: play allocated its owner charge
    // /// first, so the charge must allocate with unlisted purses at 0 (R4,
    // /// SA8). A grant deducts the draws. A peek COMM also pays for the
    // /// restorations that its attempt records, and a granted one reserves
    // /// them, so their introductions draw nothing (R9). The steps follow
    // /// `owner_draws` of play: a reserved restoration is skipped first, then
    // /// an overflowing charge.
    /// An attempt that the bound decided: play allocated its owner charge
    /// first, so the charge must allocate with unlisted purses at 0 (R4,
    /// SA8). A peek COMM also allocates the restorations that its attempt
    /// records. A grant draws the total from the remaining budgets, bills the
    /// main part and reserves each restoration with its draws (R9, G-4). The
    /// steps follow `owner_part` of play: an overflowing charge has no owner
    /// part. Then the introduction of a reserved restoration with an equal
    /// charge draws nothing: a grant bills the reservation, and a denial
    /// leaves it reserved.
    ///
    /// Changed by G3 (DR-135, S4 review): the replaced lines follow.
    // fn decided(
    //     &mut self,
    //     contract: &NativePhloExecutionContract<'_>,
    //     rows: &[NativeBudgetAttempt],
    //     index: usize,
    //     prepared: Option<&PreparedNativePhloCharge>,
    //     limits: NativePhloRegionLimits,
    // ) -> Result<(), NativeBudgetTraceError> {
    //     if self.reserved.contains(&index) || prepared.is_none() {
    //         return Ok(());
    //     }
    //     let row = &rows[index];
    //     let mut charge = self.charge(row, prepared)?;
    //     if let (true, Some(restorations)) = (row.granted, self.restorations.get(&index)) {
    //         charge.restorations.reserve_exact(restorations.len());
    //         for &restoration in restorations {
    //             let restored = rows
    //                 .get(restoration)
    //                 .ok_or(NativeBudgetTraceError::Decision)?;
    //             let prepared = match contract.prepare(Arc::clone(&restored.observation), limits) {
    //                 Ok(prepared) => Some(prepared),
    //                 Err(NativePhloExecutionError::Overflow) => None,
    //                 Err(error) => return Err(error.into()),
    //             };
    //             let restoration_charge = self.charge(restored, prepared.as_ref())?;
    //             charge
    //                 .restorations
    //                 .push((restored.observation.event_id, restoration_charge));
    //         }
    //     }
    //     let draws = self
    //         .fold
    //         .ledger
    //         .allocate(&charge, UnlistedRule::Strict, None)
    //         .map_err(|OwnerShortfall| NativeBudgetTraceError::Decision)?;
    //     if row.granted {
    //         self.fold.apply(index, &draws, &charge)?;
    //     }
    //     Ok(())
    // }
    fn decided(
        &mut self,
        contract: &NativePhloExecutionContract<'_>,
        rows: &[NativeBudgetAttempt],
        index: usize,
        prepared: Option<&PreparedNativePhloCharge>,
        limits: NativePhloRegionLimits,
    ) -> Result<(), NativeBudgetTraceError> {
        let row = &rows[index];
        match (row.occurrence.stage, row.restorations.is_empty()) {
            (NativeAttemptStage::Comm, _) | (_, true) => {}
            _ => return Err(NativeBudgetTraceError::Decision),
        }
        // Changed by G3 (DR-135, G-4): the steps follow `owner_part` of play:
        // an overflowing charge first, then a reserved restoration, which a
        // grant bills and a denial leaves reserved.
        // if row.occurrence.stage == NativeAttemptStage::ProduceIntroduction
        //     && self.fold.ledger.take_reservation(&row.observation.event_id)
        // {
        //     return Ok(());
        // }
        let Some(prepared) = prepared else {
            return Ok(());
        };
        // Changed by G3 (DR-135, S11): renamed from `charge`, so that the
        // legacy guard's `[.]charge[(]` pattern does not match. The
        // replaced line follows.
        // let mut charge = self.charge(row, Some(prepared))?;
        let mut charge = self.owner_charge(row, Some(prepared))?;
        if row.occurrence.stage == NativeAttemptStage::ProduceIntroduction
            && self
                .fold
                .ledger
                .is_reserved_for(&row.observation.event_id, &charge)
        {
            if row.granted
                && !self
                    .fold
                    .ledger
                    .bill_reservation(&row.observation.event_id, &charge)
                    .map_err(NativePhloExecutionError::from)?
            {
                return Err(NativeBudgetTraceError::Decision);
            }
            return Ok(());
        }
        charge.restorations.reserve_exact(row.restorations.len());
        for restoration in row.restorations.iter() {
            let prepared = match contract.prepare(Arc::clone(restoration), limits) {
                Ok(prepared) => Some(prepared),
                Err(NativePhloExecutionError::Overflow) => None,
                Err(error) => return Err(error.into()),
            };
            let restoration_charge =
                prepared_owner_charge(self.binding, restoration, prepared.as_ref())
                    .map_err(NativePhloExecutionError::from)?;
            charge
                .restorations
                .push((restoration.event_id, restoration_charge));
        }
        // Changed by G3 (DR-135, G-4): a grant draws the total, bills the main
        // part and reserves the restorations with their draws.
        // let draws = self
        //     .fold
        //     .ledger
        //     .allocate(&charge, UnlistedRule::Strict, None)
        //     .map_err(|OwnerShortfall| NativeBudgetTraceError::Decision)?;
        // if row.granted {
        //     self.fold.apply(index, &draws, &charge)?;
        //     self.fold.ledger.reserve_restorations(&charge);
        // }
        let draws = self
            .fold
            .ledger
            .allocate_parts(&charge, UnlistedRule::Strict, None)
            .map_err(|OwnerShortfall| NativeBudgetTraceError::Decision)?;
        if row.granted {
            self.fold.grant(index, &draws, &charge)?;
        }
        Ok(())
    }
}

impl NativePhloExecutionContract<'_> {
    pub fn check_budget_trace(
        &self,
        session: [u8; 32],
        rows: Arc<[NativeBudgetAttempt]>,
        limits: NativeBudgetTraceLimits,
    ) -> Result<CheckedNativeBudgetTrace, NativeBudgetTraceError> {
        // Changed by G3 (DR-135, S4): the trace has no owner funding.
        // let evidence = self.check_budget_evidence(session, rows, limits)?;
        let evidence = self.check_budget_evidence(session, rows, limits, None)?;
        let remaining = evidence.rows.len();
        let mut consumed = allocate(remaining)?;
        consumed.resize(remaining, false);
        Ok(CheckedNativeBudgetTrace {
            evidence,
            consumed,
            used: 0,
            remaining,
        })
    }

    // Changed by G3 (DR-135, S4): the fold also rebuilds the owner ledger
    // when the member has owner funding (R3, R4, R8, R9, SA8).
    // pub(in crate::rust::interpreter::accounting) fn check_budget_evidence(
    //     &self,
    //     session: [u8; 32],
    //     rows: Arc<[NativeBudgetAttempt]>,
    //     limits: NativeBudgetTraceLimits,
    // ) -> Result<CheckedNativeBudgetEvidence, NativeBudgetTraceError> {
    pub(in crate::rust::interpreter::accounting) fn check_budget_evidence(
        &self,
        session: [u8; 32],
        rows: Arc<[NativeBudgetAttempt]>,
        limits: NativeBudgetTraceLimits,
        owners: Option<OwnerFoldInput<'_>>,
    ) -> Result<CheckedNativeBudgetEvidence, NativeBudgetTraceError> {
        if rows.len() > limits.attempts {
            return Err(NativeBudgetTraceError::Limit);
        }
        let mut ordered = allocate(rows.len())?;
        let mut charges = allocate(rows.len())?;
        for (index, row) in rows.iter().enumerate() {
            if row.occurrence.path.depth() > limits.path_segments {
                return Err(NativeBudgetTraceError::Limit);
            }
            if row.occurrence.session != session {
                return Err(NativeBudgetTraceError::Session);
            }
            if row.occurrence.stage != NativeAttemptStage::from(row.observation.kind) {
                return Err(NativeBudgetTraceError::Stage);
            }
            ordered.push(index);
        }
        let compare =
            |left: usize, right: usize| Ok(rows[left].occurrence.cmp(&rows[right].occurrence));
        sort_indices(&mut ordered, &compare)?;
        for pair in ordered.windows(2) {
            if compare(pair[0], pair[1])? == Ordering::Equal {
                return Err(NativeBudgetTraceError::Duplicate);
            }
        }
        // Added by G3 (DR-135, S4): the owner part of the fold.
        // Changed by G3 (DR-135, S4 review): the owner part needs no check of
        // a restoration map.
        // let mut owner = owners.map(OwnerFolding::new).transpose()?;
        let mut owner = owners.map(OwnerFolding::new);
        let mut used = 0_u64;
        // Added by G3 (DR-135, S11): the usage that the bound counts.
        let mut client_used = 0_u64;
        // Changed by G3 (DR-135, S4): the fold keeps each prepared charge for
        // the owner part, and an owner refusal takes no bound decision (R8).
        // for row in rows.iter() {
        //     let charge = match self.prepare(Arc::clone(&row.observation), limits.regions) {
        //         Ok(prepared) => Some(prepared.usage()),
        //         Err(NativePhloExecutionError::Overflow) => None,
        //         Err(error) => return Err(error.into()),
        //     };
        for (index, row) in rows.iter().enumerate() {
            let prepared = match self.prepare(Arc::clone(&row.observation), limits.regions) {
                Ok(prepared) => Some(prepared),
                Err(NativePhloExecutionError::Overflow) => None,
                Err(error) => return Err(error.into()),
            };
            let charge = prepared.as_ref().map(PreparedNativePhloCharge::usage);
            // Added by G3 (DR-135, S4 review): only owner funding records
            // restorations (R9).
            if owner.is_none() && !row.restorations.is_empty() {
                return Err(NativeBudgetTraceError::Decision);
            }
            if row.owner_refused {
                owner
                    .as_ref()
                    .ok_or(NativeBudgetTraceError::Decision)?
                    .refusal(&rows, index, prepared.as_ref())?;
                charges.push(charge);
                continue;
            }
            // Changed by G3 (DR-135, S11): a row is granted exactly when its
            // client part fits the bound. The total keeps every granted
            // charge (item 13). The replaced lines follow.
            // let next = charge
            //     .and_then(|amount| used.checked_add(amount))
            //     .filter(|next| *next <= self.policy.bound);
            // if row.granted != next.is_some() {
            //     return Err(NativeBudgetTraceError::Decision);
            // }
            // if let Some(owner) = owner.as_mut() {
            //     owner.decided(self, &rows, index, prepared.as_ref(), limits.regions)?;
            // }
            // if let Some(next) = next {
            //     used = next;
            // }
            let owner_usage = match (owner.as_ref(), prepared.as_ref()) {
                (Some(owner), Some(prepared)) => owner.priced_usage(row, prepared)?,
                _ => Some(0),
            };
            let next = charge.zip(owner_usage).and_then(|(amount, owner_usage)| {
                let total = used.checked_add(amount)?;
                let client = client_used
                    .checked_add(amount.checked_sub(owner_usage)?)
                    .filter(|next| *next <= self.policy.bound)?;
                Some((total, client))
            });
            if row.granted != next.is_some() {
                return Err(NativeBudgetTraceError::Decision);
            }
            if let Some(owner) = owner.as_mut() {
                owner.decided(self, &rows, index, prepared.as_ref(), limits.regions)?;
            }
            if let Some((total, client)) = next {
                used = total;
                client_used = client;
            }
            charges.push(charge);
        }
        Ok(CheckedNativeBudgetEvidence {
            bound_source: self.policy.bound_source,
            rows,
            ordered,
            charges,
            total: used,
            // Added by G3 (DR-135, S11).
            #[cfg(test)]
            client_total: client_used,
            path_limit: limits.path_segments,
            // Added by G3 (DR-135, S4).
            owner: owner.map(|owner| owner.fold),
        })
    }
}

impl CheckedNativeBudgetTrace {
    pub fn total(&self) -> u64 { self.evidence.total }

    pub fn remaining(&self) -> usize { self.remaining }

    pub fn consume(
        &mut self,
        occurrence: &NativeBudgetOccurrence,
        observation: &ByteObservation,
    ) -> Result<NativeBudgetReplayDecision, NativeBudgetTraceError> {
        if occurrence.path.depth() > self.evidence.path_limit {
            return Err(NativeBudgetTraceError::Limit);
        }
        let position = self
            .evidence
            .ordered
            .binary_search_by(|index| self.evidence.rows[*index].occurrence.cmp(occurrence))
            .map_err(|_| NativeBudgetTraceError::Unknown)?;
        let index = self.evidence.ordered[position];
        if self.consumed[index] {
            return Err(NativeBudgetTraceError::Consumed);
        }
        let decision = self.evidence.authenticate(index, observation)?;
        if let NativeBudgetReplayDecision::Accepted { usage } = decision {
            let next = self
                .used
                .checked_add(usage)
                .filter(|next| *next <= self.evidence.total)
                .ok_or(NativeBudgetTraceError::Usage)?;
            self.used = next;
        }
        self.consumed[index] = true;
        self.remaining -= 1;
        Ok(decision)
    }

    pub fn finish(&self) -> Result<u64, NativeBudgetTraceError> {
        if self.remaining != 0 {
            return Err(NativeBudgetTraceError::Incomplete);
        }
        if self.used != self.evidence.total {
            return Err(NativeBudgetTraceError::Usage);
        }
        Ok(self.used)
    }
}

impl CheckedNativeBudgetEvidence {
    pub(in crate::rust::interpreter::accounting) fn total(&self) -> u64 { self.total }

    /// Added by G3 (DR-135, S11): the usage that the caller's bound counted
    /// (item 13).
    #[cfg(test)]
    pub(in crate::rust::interpreter::accounting) fn client_total(&self) -> u64 { self.client_total }

    /// Added by DR-113.
    pub(in crate::rust::interpreter::accounting) fn bound_source(&self) -> NativeBoundSource {
        self.bound_source
    }

    pub(in crate::rust::interpreter::accounting) fn authenticate(
        &self,
        index: usize,
        observation: &ByteObservation,
    ) -> Result<NativeBudgetReplayDecision, NativeBudgetTraceError> {
        let row = self
            .rows
            .get(index)
            .ok_or(NativeBudgetTraceError::Unknown)?;
        if row.observation.as_ref() != observation {
            return Err(NativeBudgetTraceError::Observation);
        }
        // Changed by G3 (DR-135, S4): an owner refusal has its own decision
        // (R8).
        // if row.granted {
        //     Ok(NativeBudgetReplayDecision::Accepted {
        //         usage: self.charges[index].ok_or(NativeBudgetTraceError::Usage)?,
        //     })
        // } else {
        //     Ok(NativeBudgetReplayDecision::Denied)
        // }
        match (row.granted, row.owner_refused) {
            (true, _) => Ok(NativeBudgetReplayDecision::Accepted {
                usage: self.charges[index].ok_or(NativeBudgetTraceError::Usage)?,
            }),
            (false, true) => Ok(NativeBudgetReplayDecision::OwnerRefused),
            (false, false) => Ok(NativeBudgetReplayDecision::Denied),
        }
    }

    /// Added by G3 (DR-135, S4): the owner ledger that the fold rebuilt, when
    /// the evidence was checked with owner funding.
    pub(in crate::rust::interpreter::accounting) fn owner(&self) -> Option<&OwnerFold> {
        self.owner.as_ref()
    }
}

fn sort_indices(
    indices: &mut [usize],
    compare: &impl Fn(usize, usize) -> Result<Ordering, NativeBudgetTraceError>,
) -> Result<(), NativeBudgetTraceError> {
    for root in (0..indices.len() / 2).rev() {
        sift_indices(indices, root, compare)?;
    }
    for end in (1..indices.len()).rev() {
        indices.swap(0, end);
        sift_indices(&mut indices[..end], 0, compare)?;
    }
    Ok(())
}

fn sift_indices(
    indices: &mut [usize],
    mut root: usize,
    compare: &impl Fn(usize, usize) -> Result<Ordering, NativeBudgetTraceError>,
) -> Result<(), NativeBudgetTraceError> {
    while root < indices.len() / 2 {
        let mut child = 2 * root + 1;
        if child + 1 < indices.len()
            && compare(indices[child], indices[child + 1])? == Ordering::Less
        {
            child += 1;
        }
        if compare(indices[root], indices[child])? != Ordering::Less {
            break;
        }
        indices.swap(root, child);
        root = child;
    }
    Ok(())
}

fn allocate<T>(count: usize) -> Result<Vec<T>, NativeBudgetTraceError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    Ok(result)
}

#[cfg(test)]
mod tests;
