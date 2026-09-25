use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::rust::store::key_value_store::KvStoreError;

#[derive(Clone, Copy, Debug)]
pub enum WorkKind {
    Traversal,
    Metadata,
    Oracle,
    Clique,
    Signature,
    Allocation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkLimits {
    pub operations: u64,
    pub allocated_bytes: u64,
    pub clique_expansions: u64,
    pub recursion_depth: usize,
}

impl WorkLimits {
    pub fn validate(&self) -> Result<(), KvStoreError> {
        if !(1..=2_000_000).contains(&self.operations)
            || !(1..=268_435_456).contains(&self.allocated_bytes)
            || !(1..=100_000).contains(&self.clique_expansions)
            || !(1..=64).contains(&self.recursion_depth)
        {
            return Err(limit_error("invalid_limits"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct WorkUsage {
    pub traversal: u64,
    pub metadata: u64,
    pub oracle: u64,
    pub clique: u64,
    pub signature: u64,
    pub allocation: u64,
    pub operations: u64,
    pub allocated_bytes: u64,
    pub clique_expansions: u64,
    pub maximum_depth: usize,
}

fn limit_error(reason: &str) -> KvStoreError {
    KvStoreError::InvalidArgument(format!("observation_work:{reason}"))
}

pub trait WorkMeter: Sync {
    const ENABLED: bool;

    fn charge(&self, kind: WorkKind, operations: u64, bytes: u64) -> Result<(), KvStoreError>;
    fn expand(&self, depth: usize) -> Result<(), KvStoreError>;
    fn reject(&self, reason: &str) -> KvStoreError { limit_error(reason) }
    fn metadata_bytes(&self) -> u64 { 0 }
    fn body_bytes(&self) -> u64 { 0 }

    fn step(&self, kind: WorkKind) -> Result<(), KvStoreError> { self.charge(kind, 1, 0) }

    fn allocate(&self, count: usize, size: usize) -> Result<(), KvStoreError> {
        if !Self::ENABLED {
            return Ok(());
        }
        let bytes = count
            .checked_mul(size)
            .and_then(|n| u64::try_from(n).ok())
            .ok_or_else(|| self.reject("allocation_overflow"))?;
        self.charge(WorkKind::Allocation, 1, bytes)
    }

    fn lookup(&self) -> Result<(), KvStoreError> {
        self.charge(WorkKind::Metadata, 1, self.metadata_bytes())
    }
}

#[derive(Clone, Copy, Default)]
pub struct NoopWork;

impl WorkMeter for NoopWork {
    const ENABLED: bool = false;

    #[inline]
    fn charge(&self, _: WorkKind, _: u64, _: u64) -> Result<(), KvStoreError> { Ok(()) }

    #[inline]
    fn expand(&self, _: usize) -> Result<(), KvStoreError> { Ok(()) }
}

struct BudgetState {
    total: WorkUsage,
    paths: [WorkUsage; 4],
    failure: Option<String>,
}

struct Budget {
    limits: WorkLimits,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    metadata_bytes: u64,
    body_bytes: u64,
    state: Mutex<BudgetState>,
}

#[derive(Clone)]
pub struct CheckedWork {
    budget: Arc<Budget>,
    path: usize,
}

impl CheckedWork {
    pub fn new(
        limits: WorkLimits,
        deadline: Instant,
        cancelled: Arc<AtomicBool>,
        metadata_bytes: u64,
        body_bytes: u64,
    ) -> Result<Self, KvStoreError> {
        limits.validate()?;
        Ok(Self {
            budget: Arc::new(Budget {
                limits,
                deadline,
                cancelled,
                metadata_bytes,
                body_bytes,
                state: Mutex::new(BudgetState {
                    total: WorkUsage::default(),
                    paths: std::array::from_fn(|_| WorkUsage::default()),
                    failure: None,
                }),
            }),
            path: 0,
        })
    }

    pub fn set_storage_bounds(
        &mut self,
        metadata_bytes: u64,
        body_bytes: u64,
    ) -> Result<(), KvStoreError> {
        let budget =
            Arc::get_mut(&mut self.budget).ok_or_else(|| limit_error("shared_storage_bounds"))?;
        budget.metadata_bytes = metadata_bytes;
        budget.body_bytes = body_bytes;
        Ok(())
    }

    pub fn for_path(&self, path: usize) -> Result<Self, KvStoreError> {
        if path >= 4 {
            return Err(limit_error("invalid_path"));
        }
        Ok(Self {
            budget: self.budget.clone(),
            path,
        })
    }

    pub fn usage(&self) -> (WorkUsage, [WorkUsage; 4], Option<String>) {
        let state = self
            .budget
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        (
            state.total.clone(),
            state.paths.clone(),
            state.failure.clone(),
        )
    }

    fn update(
        &self,
        kind: WorkKind,
        operations: u64,
        bytes: u64,
        depth: Option<usize>,
    ) -> Result<(), KvStoreError> {
        let mut state = self
            .budget
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(reason) = &state.failure {
            return Err(limit_error(reason));
        }
        let total = &state.total;
        let next_operations = total.operations.checked_add(operations);
        let next_bytes = total.allocated_bytes.checked_add(bytes);
        let next_expansions = total
            .clique_expansions
            .checked_add(u64::from(depth.is_some()));
        let failure = if self.budget.cancelled.load(Ordering::Acquire) {
            Some("cancelled")
        } else if Instant::now() >= self.budget.deadline {
            Some("deadline")
        } else if next_operations.is_none_or(|n| n > self.budget.limits.operations) {
            Some("operations")
        } else if next_bytes.is_none_or(|n| n > self.budget.limits.allocated_bytes) {
            Some("allocated_bytes")
        } else if next_expansions.is_none_or(|n| n > self.budget.limits.clique_expansions) {
            Some("clique_expansions")
        } else if depth.is_some_and(|n| n > self.budget.limits.recursion_depth) {
            Some("recursion_depth")
        } else {
            None
        };
        if let Some(reason) = failure {
            state.failure = Some(reason.to_string());
            return Err(limit_error(reason));
        }
        fn apply(
            usage: &mut WorkUsage,
            kind: WorkKind,
            operations: u64,
            bytes: u64,
            depth: Option<usize>,
        ) {
            usage.operations += operations;
            usage.allocated_bytes += bytes;
            match kind {
                WorkKind::Traversal => usage.traversal += operations,
                WorkKind::Metadata => usage.metadata += operations,
                WorkKind::Oracle => usage.oracle += operations,
                WorkKind::Clique => usage.clique += operations,
                WorkKind::Signature => usage.signature += operations,
                WorkKind::Allocation => usage.allocation += operations,
            }
            if let Some(depth) = depth {
                usage.clique_expansions += 1;
                usage.maximum_depth = usage.maximum_depth.max(depth);
            }
        }
        apply(&mut state.total, kind, operations, bytes, depth);
        apply(&mut state.paths[self.path], kind, operations, bytes, depth);
        Ok(())
    }
}

impl WorkMeter for CheckedWork {
    const ENABLED: bool = true;

    fn charge(&self, kind: WorkKind, operations: u64, bytes: u64) -> Result<(), KvStoreError> {
        self.update(kind, operations, bytes, None)
    }

    fn expand(&self, depth: usize) -> Result<(), KvStoreError> {
        self.update(WorkKind::Clique, 1, 0, Some(depth))
    }

    fn reject(&self, reason: &str) -> KvStoreError {
        let mut state = self
            .budget
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let failure = state.failure.get_or_insert_with(|| reason.to_string());
        limit_error(failure)
    }

    fn metadata_bytes(&self) -> u64 { self.budget.metadata_bytes }
    fn body_bytes(&self) -> u64 { self.budget.body_bytes }
}

pub fn sort_by_metered<T, W: WorkMeter>(
    meter: &W,
    values: &mut [T],
    mut compare: impl FnMut(&T, &T) -> std::cmp::Ordering,
) -> Result<(), KvStoreError> {
    if !W::ENABLED {
        values.sort_by(compare);
        return Ok(());
    }
    for end in 1..values.len() {
        let mut cursor = end;
        while cursor > 0 {
            meter.step(WorkKind::Traversal)?;
            if compare(&values[cursor], &values[cursor - 1]) != std::cmp::Ordering::Less {
                break;
            }
            meter.step(WorkKind::Traversal)?;
            values.swap(cursor, cursor - 1);
            cursor -= 1;
        }
    }
    Ok(())
}
