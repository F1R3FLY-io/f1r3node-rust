use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::CostSignature;
use thiserror::Error;

use super::{
    allocate_capped_max_min, MonetaryAllocationError, MonetaryCursor, MonetaryCursorError,
    MonetaryCursorTransition,
};
use crate::rust::interpreter::accounting::authority::{
    cost_signature_to_sig, AuthorityBalanceSettlement, AuthorityError, AuthorityPhysicalInventory,
    ResourceMultiset,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonetaryPayer {
    custody: [u8; 32],
    logical_lanes: BTreeSet<[u8; 32]>,
}

impl MonetaryPayer {
    pub fn custody(&self) -> &[u8; 32] { &self.custody }

    pub fn logical_lanes(&self) -> &BTreeSet<[u8; 32]> { &self.logical_lanes }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonetaryCohort {
    payers: Vec<MonetaryPayer>,
    payer_cap: NonZeroUsize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonetaryCohortAllocation {
    pub settlement: AuthorityBalanceSettlement,
    pub next_cursor: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonetaryCohortPlan {
    settlement: AuthorityBalanceSettlement,
    cursor_transition: MonetaryCursorTransition,
}

impl MonetaryCohortPlan {
    pub fn settlement(&self) -> &AuthorityBalanceSettlement { &self.settlement }

    pub fn cursor_transition(&self) -> &MonetaryCursorTransition { &self.cursor_transition }

    pub fn into_parts(self) -> (AuthorityBalanceSettlement, MonetaryCursorTransition) {
        (self.settlement, self.cursor_transition)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllToAllContributionPlan {
    settlement: AuthorityBalanceSettlement,
    cursor_transition: Option<MonetaryCursorTransition>,
}

impl AllToAllContributionPlan {
    pub fn settlement(&self) -> &AuthorityBalanceSettlement { &self.settlement }
    pub fn cursor_transition(&self) -> Option<&MonetaryCursorTransition> {
        self.cursor_transition.as_ref()
    }
    pub fn into_parts(self) -> (AuthorityBalanceSettlement, Option<MonetaryCursorTransition>) {
        (self.settlement, self.cursor_transition)
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum MonetaryCohortError {
    #[error(transparent)]
    Authority(#[from] AuthorityError),
    #[error(transparent)]
    Allocation(#[from] MonetaryAllocationError),
    #[error(transparent)]
    Cursor(#[from] MonetaryCursorError),
    #[error("invalid monetary fee evidence")]
    InvalidEvidence,
}

pub fn scope_for_custodies<'a>(
    policy_context: &[u8; 32],
    payers: impl ExactSizeIterator<Item = &'a [u8; 32]>,
) -> [u8; 32] {
    Blake2b256::hash_stream(|update| {
        update(b"f1r3node:monetary-payer-cohort:v1");
        update(policy_context);
        update(&(payers.len() as u64).to_le_bytes());
        for payer in payers {
            update(payer);
        }
    })
    .try_into()
    .expect("Blake2b-256 digest length")
}

impl MonetaryCohort {
    pub fn from_inventory(
        eligible: &BTreeMap<[u8; 32], CostSignature>,
        inventory: &AuthorityPhysicalInventory,
        payer_cap: NonZeroUsize,
    ) -> Result<Self, MonetaryCohortError> {
        if eligible.is_empty() {
            return Err(MonetaryAllocationError::EmptyPayers.into());
        }
        let mut physical = BTreeMap::<[u8; 32], BTreeSet<[u8; 32]>>::new();
        for (lane, signature) in eligible {
            let authority = cost_signature_to_sig(signature)?;
            if authority == super::super::Sig::Unit || authority.lane_hash() != *lane {
                return Err(AuthorityError::EventSignatureConflict.into());
            }
            let custody = inventory
                .balance_custody
                .get(lane)
                .ok_or(AuthorityError::UnknownPhysicalCustody)?;
            physical.entry(*custody).or_default().insert(*lane);
            if physical.len() > payer_cap.get() {
                return Err(MonetaryAllocationError::TooManyPayers.into());
            }
        }
        Ok(Self {
            payers: physical
                .into_iter()
                .map(|(custody, logical_lanes)| MonetaryPayer {
                    custody,
                    logical_lanes,
                })
                .collect(),
            payer_cap,
        })
    }

    pub fn payers(&self) -> &[MonetaryPayer] { &self.payers }

    pub fn plan(
        &self,
        available: &ResourceMultiset<[u8; 32]>,
        obligation: u64,
        policy_context: &[u8; 32],
        cursor: MonetaryCursor,
    ) -> Result<MonetaryCohortPlan, MonetaryCohortError> {
        let count =
            NonZeroUsize::new(self.payers.len()).ok_or(MonetaryAllocationError::EmptyPayers)?;
        let position = cursor.position_index(count)?;
        cursor.next_revision()?;
        let allocation = self.allocate(available, obligation, position)?;
        let next_position = i64::try_from(allocation.next_cursor)
            .map_err(|_| MonetaryCursorError::InvalidPosition)?;
        Ok(MonetaryCohortPlan {
            settlement: allocation.settlement,
            cursor_transition: MonetaryCursorTransition::new(
                self.scope_id(policy_context),
                cursor,
                next_position,
                count,
            )?,
        })
    }

    pub fn scope_id(&self, policy_context: &[u8; 32]) -> [u8; 32] {
        scope_for_custodies(
            policy_context,
            self.payers.iter().map(MonetaryPayer::custody),
        )
    }

    pub fn plan_all_to_all_contribution(
        &self,
        available: &ResourceMultiset<[u8; 32]>,
        obligation: u64,
        policy_context: &[u8; 32],
        cursor: MonetaryCursor,
    ) -> Result<AllToAllContributionPlan, MonetaryCohortError> {
        if obligation == 0 {
            let count =
                NonZeroUsize::new(self.payers.len()).ok_or(MonetaryAllocationError::EmptyPayers)?;
            let position = cursor.position_index(count)?;
            let allocation = self.allocate(available, 0, position)?;
            return Ok(AllToAllContributionPlan {
                settlement: allocation.settlement,
                cursor_transition: None,
            });
        }
        let (settlement, cursor_transition) = self
            .plan(available, obligation, policy_context, cursor)?
            .into_parts();
        Ok(AllToAllContributionPlan {
            settlement,
            cursor_transition: Some(cursor_transition),
        })
    }

    pub fn allocate(
        &self,
        available: &ResourceMultiset<[u8; 32]>,
        obligation: u64,
        cursor: usize,
    ) -> Result<MonetaryCohortAllocation, MonetaryCohortError> {
        let capacities: Vec<_> = self
            .payers
            .iter()
            .map(|payer| available.get(&payer.custody))
            .collect();
        let plan = allocate_capped_max_min(&capacities, obligation, cursor, self.payer_cap)?;
        let mut settlement = AuthorityBalanceSettlement::default();
        for (payer, amount) in self.payers.iter().zip(plan.debits) {
            if amount > 0 {
                let lane = payer
                    .logical_lanes
                    .first()
                    .ok_or(AuthorityError::UnknownPhysicalSignature)?;
                settlement.logical_debit.0.insert(*lane, amount);
                settlement.custody_debit.0.insert(payer.custody, amount);
            }
        }
        Ok(MonetaryCohortAllocation {
            settlement,
            next_cursor: plan.next_cursor,
        })
    }
}

#[cfg(test)]
mod tests;
