use std::cmp::Ordering;
use std::sync::Arc;

use models::rhoapi::CostAuthority;
use thiserror::Error;

use super::{
    NativeBoundSource, NativePhloExecutionContract, NativePhloExecutionError,
    NativePhloRegionLimits,
};
use crate::rust::interpreter::accounting::authority::{
    authority_demand, AuthorityByteEventKind, ResourceMultiset,
};
use crate::rust::interpreter::accounting::byte_receipts::ByteObservation;
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchError;
use crate::rust::interpreter::accounting::native_runtime::PathId;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeDenialCause {
    #[default]
    Budget,
    Allocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeBudgetAttempt {
    pub occurrence: NativeBudgetOccurrence,
    pub observation: Arc<ByteObservation>,
    pub granted: bool,
    pub cause: NativeDenialCause,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeStackTransferKind {
    Reserve {
        produce_hash: [u8; 32],
        authority: CostAuthority,
        cells: u64,
        granted: bool,
    },
    Release {
        reserve: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeStackTransferEvent {
    pub kind: NativeStackTransferKind,
    pub fresh_before: usize,
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
    #[error("native stack transfer differs from its checked prefix")]
    Transfer,
    #[error("native authority demand is invalid")]
    Allocation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeBudgetReplayDecision {
    Accepted { usage: u64 },
    Denied,
    DeniedAllocation,
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
    path_limit: usize,
    transfers: Arc<[NativeStackTransferEvent]>,
    transfer_index: Vec<([u8; 32], usize)>,
}

type Demand = ResourceMultiset<[u8; 32]>;

struct AllocationLedger<'a> {
    allocation: Option<&'a Demand>,
    reserved: Demand,
    transfers: &'a [NativeStackTransferEvent],
    demands: Vec<Option<Demand>>,
    released: Vec<bool>,
    cursor: usize,
}

impl<'a> AllocationLedger<'a> {
    fn new(
        allocation: Option<&'a Demand>,
        transfers: &'a [NativeStackTransferEvent],
    ) -> Result<Self, NativeBudgetTraceError> {
        let mut demands = allocate(transfers.len())?;
        demands.resize(transfers.len(), None);
        let mut released = allocate(transfers.len())?;
        released.resize(transfers.len(), false);
        Ok(Self {
            allocation,
            reserved: Demand::default(),
            transfers,
            demands,
            released,
            cursor: 0,
        })
    }

    fn admits(&self, demand: &Demand) -> Result<bool, NativeBudgetTraceError> {
        match self.allocation {
            Some(allocation) => {
                let next = self
                    .reserved
                    .checked_add(demand)
                    .map_err(|_| NativeBudgetTraceError::Allocation)?;
                Ok(allocation.dominates(&next))
            }
            None => Ok(true),
        }
    }

    fn reserve(&mut self, demand: &Demand) -> Result<(), NativeBudgetTraceError> {
        if self.allocation.is_some() {
            self.reserved = self
                .reserved
                .checked_add(demand)
                .map_err(|_| NativeBudgetTraceError::Allocation)?;
        }
        Ok(())
    }

    fn comm_demand(
        &self,
        row: &NativeBudgetAttempt,
    ) -> Result<Option<Demand>, NativeBudgetTraceError> {
        if self.allocation.is_none() || row.occurrence.stage != NativeAttemptStage::Comm {
            return Ok(None);
        }
        authority_demand(&row.observation.authority)
            .map(Some)
            .map_err(|_| NativeBudgetTraceError::Allocation)
    }

    fn advance(&mut self, position: usize) -> Result<(), NativeBudgetTraceError> {
        while let Some(event) = self.transfers.get(self.cursor) {
            if event.fresh_before > position {
                break;
            }
            if event.fresh_before < position {
                return Err(NativeBudgetTraceError::Transfer);
            }
            let index = self.cursor;
            match &event.kind {
                NativeStackTransferKind::Reserve {
                    authority,
                    cells,
                    granted,
                    ..
                } => {
                    if *cells == 0 {
                        return Err(NativeBudgetTraceError::Transfer);
                    }
                    if self.allocation.is_none() {
                        if !granted {
                            return Err(NativeBudgetTraceError::Decision);
                        }
                    } else {
                        let demand = transfer_demand(authority, *cells)?;
                        if self.admits(&demand)? != *granted {
                            return Err(NativeBudgetTraceError::Decision);
                        }
                        if *granted {
                            self.reserve(&demand)?;
                            self.demands[index] = Some(demand);
                        }
                    }
                }
                NativeStackTransferKind::Release { reserve } => {
                    let granted = *reserve < index
                        && matches!(
                            self.transfers[*reserve].kind,
                            NativeStackTransferKind::Reserve { granted: true, .. }
                        );
                    if !granted || self.released[*reserve] {
                        return Err(NativeBudgetTraceError::Transfer);
                    }
                    self.released[*reserve] = true;
                    if let Some(demand) = self.demands[*reserve].take() {
                        self.reserved = self
                            .reserved
                            .checked_sub(&demand)
                            .map_err(|_| NativeBudgetTraceError::Transfer)?;
                    }
                }
            }
            self.cursor += 1;
        }
        Ok(())
    }

    fn finish(mut self, rows: usize) -> Result<(), NativeBudgetTraceError> {
        self.advance(rows)?;
        if self.cursor != self.transfers.len() {
            return Err(NativeBudgetTraceError::Transfer);
        }
        Ok(())
    }
}

fn transfer_demand(
    authority: &CostAuthority,
    cells: u64,
) -> Result<Demand, NativeBudgetTraceError> {
    let demand = authority_demand(authority).map_err(|_| NativeBudgetTraceError::Allocation)?;
    let mut total = Demand::default();
    for (lane, amount) in demand.0 {
        let amount = amount
            .checked_mul(cells)
            .ok_or(NativeBudgetTraceError::Allocation)?;
        total.0.insert(lane, amount);
    }
    Ok(total)
}

fn transfer_index(
    transfers: &[NativeStackTransferEvent],
) -> Result<Vec<([u8; 32], usize)>, NativeBudgetTraceError> {
    let mut index = allocate(transfers.len())?;
    for (position, event) in transfers.iter().enumerate() {
        if let NativeStackTransferKind::Reserve { produce_hash, .. } = &event.kind {
            index.push((*produce_hash, position));
        }
    }
    index.sort_unstable();
    if index.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(NativeBudgetTraceError::Duplicate);
    }
    Ok(index)
}

impl NativePhloExecutionContract<'_> {
    pub fn check_budget_trace(
        &self,
        session: [u8; 32],
        rows: Arc<[NativeBudgetAttempt]>,
        limits: NativeBudgetTraceLimits,
    ) -> Result<CheckedNativeBudgetTrace, NativeBudgetTraceError> {
        let evidence = self.check_budget_evidence(session, rows, Arc::from([]), limits)?;
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

    pub(in crate::rust::interpreter::accounting) fn check_budget_evidence(
        &self,
        session: [u8; 32],
        rows: Arc<[NativeBudgetAttempt]>,
        transfers: Arc<[NativeStackTransferEvent]>,
        limits: NativeBudgetTraceLimits,
    ) -> Result<CheckedNativeBudgetEvidence, NativeBudgetTraceError> {
        if rows.len() > limits.attempts || transfers.len() > limits.attempts {
            return Err(NativeBudgetTraceError::Limit);
        }
        let transfer_index = transfer_index(&transfers)?;
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
        let mut used = 0_u64;
        let mut ledger = AllocationLedger::new(self.allocation.as_deref(), &transfers)?;
        for (index, row) in rows.iter().enumerate() {
            ledger.advance(index)?;
            let charge = match self.prepare(Arc::clone(&row.observation), limits.regions) {
                Ok(prepared) => Some(prepared.usage()),
                Err(NativePhloExecutionError::Overflow) => None,
                Err(error) => return Err(error.into()),
            };
            let demand = ledger.comm_demand(row)?;
            let admitted = match &demand {
                Some(demand) => ledger.admits(demand)?,
                None => true,
            };
            let next = charge
                .and_then(|amount| used.checked_add(amount))
                .filter(|next| *next <= self.policy.bound);
            let consistent = match (row.granted, row.cause) {
                (true, NativeDenialCause::Budget) => admitted && next.is_some(),
                (false, NativeDenialCause::Budget) => admitted && next.is_none(),
                (false, NativeDenialCause::Allocation) => !admitted,
                (true, NativeDenialCause::Allocation) => false,
            };
            if !consistent {
                return Err(NativeBudgetTraceError::Decision);
            }
            if row.granted {
                if let Some(next) = next {
                    used = next;
                }
                if let Some(demand) = &demand {
                    ledger.reserve(demand)?;
                }
            }
            charges.push(charge);
        }
        ledger.finish(rows.len())?;
        Ok(CheckedNativeBudgetEvidence {
            bound_source: self.policy.bound_source,
            rows,
            ordered,
            charges,
            total: used,
            path_limit: limits.path_segments,
            transfers,
            transfer_index,
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
        match (row.granted, row.cause) {
            (true, _) => Ok(NativeBudgetReplayDecision::Accepted {
                usage: self.charges[index].ok_or(NativeBudgetTraceError::Usage)?,
            }),
            (false, NativeDenialCause::Budget) => Ok(NativeBudgetReplayDecision::Denied),
            (false, NativeDenialCause::Allocation) => {
                Ok(NativeBudgetReplayDecision::DeniedAllocation)
            }
        }
    }

    pub(in crate::rust::interpreter::accounting) fn replay_transfers(
        &self,
    ) -> Arc<[([u8; 32], CostAuthority, u64, bool)]> {
        self.transfer_index
            .iter()
            .filter_map(|(hash, position)| match &self.transfers[*position].kind {
                NativeStackTransferKind::Reserve {
                    authority,
                    cells,
                    granted,
                    ..
                } => Some((*hash, authority.clone(), *cells, *granted)),
                NativeStackTransferKind::Release { .. } => None,
            })
            .collect()
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
