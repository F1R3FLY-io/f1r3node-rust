use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use shared::rust::collection_backing::{tree_backing, tree_height_bound, tree_search_bound};

use super::*;

/// `visits` B-tree searches in a map that holds at most `entries` entries
/// when the searches run: `tree_search_bound(entries)` comparisons and
/// `tree_height_bound(entries) + 1` nodes for each visit, and the same nodes
/// again for an insert (C14, DR-79; `OrderedLookupBound.search_within_bound`).
fn tree_update<K, V>(
    host: &HostWorkBudget,
    insert: bool,
    visits: usize,
    entries: usize,
) -> Result<(), InterpreterError> {
    // Disabled by C14 (DR-79): every visit was charged as a search in a tree
    // of 65 levels, while a map with n entries has at most
    // tree_height_bound(n) levels.
    // let height = usize::BITS as usize + 1;
    // let comparisons = height
    //     .checked_mul(11)
    //     .and_then(|n| n.checked_mul(visits))
    //     .ok_or(InterpreterError::HostWorkRejected)?;
    let height = tree_height_bound(entries);
    let comparisons = tree_search_bound(entries)
        .checked_mul(visits)
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

/// The three identity searches of one COMM row, in `pending_stack_event_ids`,
/// `pending_replay_events` and `events`. Each search is charged at the largest
/// of the three live sizes.
pub(super) fn reserve_event_lookup(
    state: &AuthorityRuntimeState,
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    // Disabled by C14 (DR-79): the 65-level charge needed no map size.
    // tree_update::<[u8; 32], AuthorityRuntimeEvent>(host, false, 3)
    let entries = state
        .pending_stack_event_ids
        .len()
        .max(state.pending_replay_events.len())
        .max(state.events.len());
    tree_update::<[u8; 32], AuthorityRuntimeEvent>(host, false, 3, entries)
}

pub(super) fn reserve_changes(
    state: &AuthorityRuntimeState,
    debit: Option<&ResourceMultiset<[u8; 32]>>,
    frontier: bool,
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    // C14 (DR-79): the publish and abort searches run later than this
    // charge, so each group is sized after the operation. The debit adds at
    // most `lanes` keys to `reserved` and `realized`, and the publication adds
    // one entry to `pending_replay_events`, `events` or `frontier`.
    let lanes = debit.map_or(0, |debit| debit.0.len());
    let mut ledger = state
        .reserved
        .0
        .len()
        .max(state.realized.0.len())
        .checked_add(lanes)
        .ok_or(InterpreterError::HostWorkRejected)?;
    if state.enforce_allocation {
        ledger = ledger.max(state.allocation.0.len());
        for _ in &state.reserved.0 {
            tree_update::<[u8; 32], u64>(host, false, 1, state.allocation.0.len())?;
        }
    }
    if let Some(debit) = debit {
        for key in debit.0.keys() {
            tree_update::<[u8; 32], u64>(host, false, 2, ledger)?;
            tree_update::<[u8; 32], u64>(host, !state.reserved.0.contains_key(key), 9, ledger)?;
            tree_update::<[u8; 32], u64>(host, !state.realized.0.contains_key(key), 4, ledger)?;
        }
        let events = state
            .pending_replay_events
            .len()
            .max(state.events.len())
            .checked_add(1)
            .ok_or(InterpreterError::HostWorkRejected)?;
        tree_update::<[u8; 32], AuthorityRuntimeEvent>(host, true, 3, events)?;
        tree_update::<[u8; 32], AuthorityRuntimeEvent>(host, true, 1, events)?;
    }
    if frontier {
        let entries = state
            .frontier
            .len()
            .checked_add(1)
            .ok_or(InterpreterError::HostWorkRejected)?;
        tree_update::<[u8; 32], CostAuthority>(host, true, 1, entries)?;
    }
    Ok(())
}

/// D-O4 (DR-89): the copies that `authority_events` makes for the result:
/// for each event its 32-byte id, its authority and its debit, each with the
/// release of the copy. The events map, its tree backing and the shared
/// byte-observation payloads of the events are not copied.
pub(super) fn reserve_event_copies(
    state: &AuthorityRuntimeState,
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    for (event_id, event) in &state.events {
        // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
        // clone_backing::reserve_copy_and_cleanup(event_id, host)?;
        // clone_backing::reserve_copy_and_cleanup(&event.authority, host)?;
        // clone_backing::reserve_copy_and_cleanup(&event.debit, host)?;
        clone_backing::reserve_blocks_copy_and_cleanup(event_id, host)?;
        clone_backing::reserve_blocks_copy_and_cleanup(&event.authority, host)?;
        clone_backing::reserve_blocks_copy_and_cleanup(&event.debit, host)?;
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
