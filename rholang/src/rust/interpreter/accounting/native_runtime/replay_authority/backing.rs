use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use shared::rust::collection_backing::tree_backing;

use super::*;

fn tree_update<K, V>(
    host: &HostWorkBudget,
    insert: bool,
    visits: usize,
) -> Result<(), InterpreterError> {
    let height = usize::BITS as usize + 1;
    let comparisons = height
        .checked_mul(11)
        .and_then(|n| n.checked_mul(visits))
        .ok_or(InterpreterError::HostWorkRejected)?;
    work(host, HostWorkDimension::VerificationOperations, comparisons)?;
    work(
        host,
        HostWorkDimension::VerificationBytes,
        comparisons
            .checked_mul(size_of::<K>())
            .ok_or(InterpreterError::HostWorkRejected)?,
    )?;
    let (_, node) = tree_backing::<K, V>(1).ok_or(InterpreterError::HostWorkRejected)?;
    work(
        host,
        HostWorkDimension::VerificationBytes,
        node.checked_mul(height + 1)
            .and_then(|bytes| bytes.checked_mul(visits))
            .ok_or(InterpreterError::HostWorkRejected)?,
    )?;
    if insert {
        let bytes = node
            .checked_mul(height + 1)
            .ok_or(InterpreterError::HostWorkRejected)?;
        work(host, HostWorkDimension::SearchStateBytes, bytes)?;
    }
    Ok(())
}

pub(super) fn reserve_event_lookup(host: &HostWorkBudget) -> Result<(), InterpreterError> {
    tree_update::<[u8; 32], AuthorityRuntimeEvent>(host, false, 3)
}

pub(super) fn reserve_changes(
    state: &AuthorityRuntimeState,
    debit: Option<&ResourceMultiset<[u8; 32]>>,
    frontier: bool,
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    if state.enforce_allocation {
        for _ in &state.reserved.0 {
            tree_update::<[u8; 32], u64>(host, false, 1)?;
        }
    }
    if let Some(debit) = debit {
        for key in debit.0.keys() {
            tree_update::<[u8; 32], u64>(host, false, 2)?;
            tree_update::<[u8; 32], u64>(host, !state.reserved.0.contains_key(key), 9)?;
            tree_update::<[u8; 32], u64>(host, !state.realized.0.contains_key(key), 4)?;
        }
        tree_update::<[u8; 32], AuthorityRuntimeEvent>(host, true, 3)?;
        tree_update::<[u8; 32], AuthorityRuntimeEvent>(host, true, 1)?;
    }
    if frontier {
        tree_update::<[u8; 32], CostAuthority>(host, true, 1)?;
    }
    Ok(())
}

pub(super) fn reserve_result_vectors(
    state: &AuthorityRuntimeState,
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    for (count, size) in [
        (
            state.events.len(),
            size_of::<authority::AuthorityEvent<[u8; 32]>>(),
        ),
        (
            state.stack_births.len(),
            size_of::<authority::AuthorityStackBirth>(),
        ),
    ] {
        work(
            host,
            HostWorkDimension::SearchStateBytes,
            count
                .checked_mul(size)
                .ok_or(InterpreterError::HostWorkRejected)?,
        )?;
    }
    Ok(())
}
