//! Added by G3 (DR-135, S6): the owner checks of native replay (plan S6,
//! rules R5, R6, R7, R8, A5, SA4, SA5 and SA8).
//!
//! A member with owner funding replays with the owner binding of its runtime
//! and the owner fold of its checked journal. Both must exist and list one
//! owner table. The fold gives the budgets of a listed purse before each
//! attempt (`OwnerFold::remaining_at`), so replay never reads a live ledger
//! (plan §6.3 item 5):
//!
//! ```text
//! S_r(row)          = remaining_at(row.budget_start)              (§6.3 item 3)
//! live(refusal)     = remaining_at(refusal.position)              (§6.3 item 2)
//! after(attempt a)  = remaining_at(a + 1)
//! ```
//!
//! The checks, with the unlisted rule of each (SA8):
//!
//! ```text
//! Stored rule      skip E3-a and every continuation that the row refused,
//!                  then match with R6 and the item test at S_r with
//!                  unlisted purses passing (Lenient). A full tuple is a
//!                  mismatch (selection.rs).
//! refusal i        find the tuple by the refusal's COMM source, with R6.
//!                  (a) every item passes at S_r, unlisted purses at 0
//!                      (Strict, SA4);
//!                  (b) the COMM charge with its A5 transferee and its R9
//!                      restorations fails at live(refusal), unlisted purses
//!                      passing (Lenient).
//! Matched row      the identity filter, R6, and A5 after the draws of the
//!                  COMM attempt, unlisted purses at 0 (Strict).
//! skipped intro    the owner-refused introduction publishes and stores
//!                  nothing (R8). The ledger of the replay configuration
//!                  records the refusal (SA5, replay_authority.rs).
//! ```
//!
//! Play's item test, play's COMM charge and play's A5 test share their code
//! with these checks (`ItemTest`, `owner_charge_with_extras`,
//! `comm_transferee`, `transferee_funded`), so both roles measure and decide
//! in the same way.

use std::sync::Arc;

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::internal::ConsumeCandidate;
use rspace_plus_plus::rspace::replay_rspace::native_epoch::{
    NativeCandidateIdentity, NativeOwnerFilter,
};
use rspace_plus_plus::rspace::rspace_interface::CommParticipant;
use rspace_plus_plus::rspace::trace::event::Consume;

use super::*;
use crate::rust::interpreter::accounting::authority::ResidueContext;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloChargePreparer, NativePhloRegionLimits, OwnerFold,
};
use crate::rust::interpreter::accounting::native_runtime::owner_charge_with_extras;
use crate::rust::interpreter::accounting::observation_construction;
use crate::rust::interpreter::accounting::owner_funding::{
    comm_transferee, refusal_shortfall, refused_item_passes, restoration_observations,
    stored_item_passes, transferee_funded, ItemTest, OwnerFundingBinding, OwnerPurse,
};

/// The datum index of a produce's incoming datum in a tuple (rspace++
/// `selection.rs`, `INCOMING_INDEX`).
const INCOMING_INDEX: i32 = -1;

/// The owner terms of a replay with owner funding: the binding of the
/// runtime, the runtime budget (A5 resolves a signature with its metering),
/// the preparer and the region limits of the replay meter, and the residue
/// context of the deployment.
pub(super) struct ReplayOwnerChecks {
    binding: Arc<OwnerFundingBinding>,
    budget: RuntimeBudget,
    preparer: NativePhloChargePreparer,
    limits: NativePhloRegionLimits,
    residue: ResidueContext,
}

impl ReplayOwnerChecks {
    /// The owner checks of a replay bound to `budget`, or none without owner
    /// funding. The binding of the runtime and the owner fold of the checked
    /// journal must both exist and list one table, or neither exists.
    pub(super) fn bind(
        budget: &RuntimeBudget,
        trace: &CheckedNativeOperationTrace,
    ) -> Result<Option<Self>, NativeReplayError> {
        match (
            budget.owner_binding(),
            trace.journal().budget_evidence().owner(),
        ) {
            (None, None) => Ok(None),
            (Some(binding), Some(fold)) if binding.table() == fold.ledger().table() => {
                let (preparer, limits) = budget
                    .native_owner_meter()
                    .ok_or(NativeReplayError::OwnerFunding)?;
                let residue = ResidueContext::new(&budget.signature(), budget.deploy_id())
                    .map(|context| context.with_price(budget.region_price()))
                    .map_err(|error| NativeReplayError::Construction(error.to_string()))?;
                Ok(Some(Self {
                    binding,
                    budget: budget.clone(),
                    preparer,
                    limits,
                    residue,
                }))
            }
            (Some(_), Some(_)) | (Some(_), None) | (None, Some(_)) => {
                Err(NativeReplayError::OwnerFunding)
            }
        }
    }

    fn item_test(&self) -> ItemTest<'_> {
        ItemTest {
            binding: &self.binding,
            preparer: &self.preparer,
            limits: self.limits,
            residue: &self.residue,
        }
    }

    /// Whether the table can list one more purse, which decides the unlisted
    /// rule (SA8).
    fn room_free(&self) -> bool { self.binding.table().has_room() }
}

/// The budget of a listed purse in the fold before the attempt at
/// `position`, or `None` for an unlisted purse.
fn view_at(fold: &OwnerFold, position: usize) -> impl Fn(&OwnerPurse) -> Option<u64> + '_ {
    move |purse: &OwnerPurse| fold.remaining_at(&purse.custody, position)
}

impl NativeReplayReservation {
    /// The owner checks and the owner fold of the replay, when the member has
    /// owner funding.
    fn owner_terms(&self) -> Option<(&ReplayOwnerChecks, &OwnerFold)> {
        let checks = self.replay.inner.owners.as_ref()?;
        let fold = self
            .replay
            .inner
            .trace
            .journal()
            .budget_evidence()
            .owner()?;
        Some((checks, fold))
    }

    /// Verifies the refusal `index` of the row with its live tuple: the COMM
    /// `comm` of `continuation` with `data`. `incoming` is the stored value
    /// of a produce's own datum, which `data` holds at `INCOMING_INDEX` with
    /// a default removed value. The refusals are verified in their order.
    pub(super) fn verify_owner_refusal(
        &mut self,
        index: usize,
        comm: &COMM,
        continuation: &TaggedContinuation,
        persistent: bool,
        data: &[ConsumeCandidate<Par, ListParWithRandom>],
        incoming: Option<&ListParWithRandom>,
    ) -> Result<(), NativeReplayError> {
        self.check_owner_refusal(index, comm, continuation, persistent, data, incoming)?;
        self.verified_refusals += 1;
        Ok(())
    }

    /// The checks of `verify_owner_refusal`, which change nothing.
    fn check_owner_refusal(
        &self,
        index: usize,
        comm: &COMM,
        continuation: &TaggedContinuation,
        persistent: bool,
        data: &[ConsumeCandidate<Par, ListParWithRandom>],
        incoming: Option<&ListParWithRandom>,
    ) -> Result<(), NativeReplayError> {
        let (checks, fold) = self.owner_terms().ok_or(NativeReplayError::Refusal)?;
        let row = self.operation();
        let refusal = row.refusals.get(index).ok_or(NativeReplayError::Refusal)?;
        if index != self.verified_refusals || !refusal.source.matches(comm) {
            return Err(NativeReplayError::Refusal);
        }
        // (a) SA4: each item of the tuple passes the item test at S_r, with
        // unlisted purses at 0. Honest play forms only such tuples (R5).
        let start = view_at(fold, row.budget_start);
        let test = checks.item_test();
        for candidate in data {
            // Changed by G3 (DR-135, S11): renamed from `charge`, so that the
            // legacy guard's `[.]charge[(]` pattern does not match. The
            // replaced lines follow.
            // let charge = test
            //     .charge(
            let charge = test
                .owner_charge(
                    continuation,
                    persistent,
                    data.len(),
                    &candidate.datum.a,
                    candidate.datum.persist,
                )
                .map_err(|error| NativeReplayError::Construction(error.to_string()))?;
            if let Some(charge) = charge {
                if !refused_item_passes(&charge, &start, checks.room_free()) {
                    return Err(NativeReplayError::Refusal);
                }
            }
        }
        // (b) The charge of the COMM, as play's recorder builds it, fails at
        // the position of the refusal, with unlisted purses passing.
        let mut matched = Vec::with_capacity(data.len());
        matched.extend(
            data.iter()
                .map(|candidate| (&candidate.datum.a, candidate.datum.persist)),
        );
        let observation = Arc::new(
            observation_construction::comm(
                comm,
                continuation,
                persistent,
                &matched,
                &checks.residue,
            )
            .map_err(|error| NativeReplayError::Construction(error.to_string()))?
            .into_native()
            .map_err(|error| NativeReplayError::Construction(error.to_string()))?,
        );
        // An overflowing COMM has no owner charge, so play never refuses it.
        let prepared = checks
            .preparer
            .prepare(Arc::clone(&observation), checks.limits)
            .map_err(|_| NativeReplayError::Refusal)?;
        let mut values = Vec::with_capacity(data.len());
        values.extend(data.iter().map(|candidate| &candidate.datum.a));
        let transferee = comm_transferee(&checks.binding, &checks.budget, continuation, &values)?;
        let mut participants = Vec::with_capacity(data.len());
        for candidate in data {
            let stored = match candidate.datum_index {
                INCOMING_INDEX => incoming.ok_or(NativeReplayError::Refusal)?,
                _ => &candidate.removed_datum,
            };
            participants.push(CommParticipant {
                channel: &candidate.channel,
                matched: &candidate.datum.a,
                stored,
                persistent: candidate.datum.persist,
            });
        }
        let restorations = restoration_observations(comm, &participants, &checks.residue)?;
        let charge = owner_charge_with_extras(
            &checks.binding,
            &checks.preparer,
            checks.limits,
            &observation,
            &prepared,
            transferee,
            &restorations,
        )?;
        let live = view_at(fold, refusal.position);
        match refusal_shortfall(&charge, &live, checks.room_free()) {
            true => Ok(()),
            false => Err(NativeReplayError::Refusal),
        }
    }

    /// A5 for the granted COMM attempt `index` of a Matched row: when the
    /// body of `continuation` runs for a signer, the signer keeps a positive
    /// budget after the draws of the attempt, with unlisted purses at 0. The
    /// fold does not know the transferee, because the attempt does not
    /// record it, so replay checks it here.
    pub(super) fn check_transferee(
        &self,
        index: usize,
        continuation: &TaggedContinuation,
        data: &[(&ListParWithRandom, bool)],
    ) -> Result<(), NativeReplayError> {
        let Some((checks, fold)) = self.owner_terms() else {
            return Ok(());
        };
        let mut values = Vec::with_capacity(data.len());
        values.extend(data.iter().map(|(datum, _)| *datum));
        let Some(transferee) =
            comm_transferee(&checks.binding, &checks.budget, continuation, &values)?
        else {
            return Ok(());
        };
        let after = index.checked_add(1).ok_or(NativeReplayError::Transferee)?;
        let mut remaining =
            |purse: &OwnerPurse| u128::from(fold.remaining_at(&purse.custody, after).unwrap_or(0));
        transferee_funded(&transferee, &mut remaining)
            .then_some(())
            .ok_or(NativeReplayError::Transferee)
    }

    /// The owner checks of the ticket, when the member has owner funding.
    pub(super) fn owner_checks(
        &self,
    ) -> Option<&dyn NativeOwnerFilter<Par, BindPattern, ListParWithRandom, TaggedContinuation>>
    {
        self.owner_terms().map(|_| {
            self as &dyn NativeOwnerFilter<Par, BindPattern, ListParWithRandom, TaggedContinuation>
        })
    }
}

impl NativeOwnerFilter<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    for NativeReplayReservation
{
    /// The item test of the Stored rule: the item passes at S_r with
    /// unlisted purses passing when the table has room (SA8).
    fn admit_item(
        &self,
        continuation: &TaggedContinuation,
        continuation_persistent: bool,
        channel_count: usize,
        datum: &ListParWithRandom,
        datum_persistent: bool,
    ) -> Result<bool, RSpaceError> {
        let Some((checks, fold)) = self.owner_terms() else {
            return Ok(true);
        };
        // Changed by G3 (DR-135, S11): renamed from `charge`, so that the
        // legacy guard's `[.]charge[(]` pattern does not match. The
        // replaced line follows.
        // let Some(charge) = checks.item_test().charge(
        let Some(charge) = checks.item_test().owner_charge(
            continuation,
            continuation_persistent,
            channel_count,
            datum,
            datum_persistent,
        )?
        else {
            return Ok(true);
        };
        let start = view_at(fold, self.operation().budget_start);
        Ok(stored_item_passes(&charge, &start, checks.room_free()))
    }

    fn names_refusal_for(&self, consume: &Consume) -> bool {
        self.operation()
            .refusals
            .iter()
            .any(|refusal| refusal.source.matches_consume(consume))
    }

    fn requires_own_datum(&self) -> bool { self.owner_terms().is_some() }
}
