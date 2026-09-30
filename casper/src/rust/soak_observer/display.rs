use std::collections::{BTreeSet, HashMap};

use block_storage::rust::dag::soak_equivocations::EquivocationSnapshot;
use block_storage::rust::dag::soak_snapshot::DetachedDagSnapshot;
use models::rust::block_hash::BlockHash;
use models::rust::validator::Validator;
use serde::Serialize;
use shared::rust::dag::observation_work::{CheckedWork, WorkKind, WorkMeter};

use crate::rust::safety::initial_fault;

pub const DISPLAY_SCOPE: &str = "batch-e-detached-display-projection";

#[derive(Clone, Debug, Serialize)]
pub struct DisplayInputs {
    pub base_source: &'static str,
    pub base_bits: u32,
    pub initial_fault_bits: u32,
    pub equivocating_weight: String,
    pub total_weight: String,
    pub matched_records: usize,
    pub distinct_equivocators: usize,
    pub finalized_set_member: bool,
    pub metadata_finalized: bool,
    pub equivocation_digest: String,
}

pub fn calculate(
    snapshot: &DetachedDagSnapshot,
    tracker: &EquivocationSnapshot,
    hash: &BlockHash,
    base: Option<(&'static str, u32)>,
    meter: &CheckedWork,
) -> Result<(u32, DisplayInputs), String> {
    meter.step(WorkKind::Metadata).map_err(|e| e.to_string())?;
    let block = snapshot.blocks.get(hash).ok_or("target_not_held")?;
    meter.step(WorkKind::Metadata).map_err(|e| e.to_string())?;
    let finalized_set_member = snapshot.finalized_block_set.contains(hash);
    let metadata_finalized = block.metadata.finalized;
    let (base_source, base_bits) = if finalized_set_member || metadata_finalized {
        (
            "persisted_metadata",
            block.metadata.fault_tolerance_value.to_bits(),
        )
    } else {
        base.ok_or("display_base_unavailable")?
    };
    meter
        .allocate(
            block.metadata.weight_map.len(),
            std::mem::size_of::<(Validator, u64)>(),
        )
        .map_err(|e| e.to_string())?;
    let mut weights = HashMap::with_capacity(block.metadata.weight_map.len());
    let mut checked_total = 0u64;
    for (validator, weight) in &block.metadata.weight_map {
        meter
            .charge(WorkKind::Allocation, 1, validator.len() as u64)
            .map_err(|e| e.to_string())?;
        meter
            .charge(
                WorkKind::Oracle,
                (validator.len() as u64)
                    .checked_add(1)
                    .ok_or("display_work_overflow")?,
                0,
            )
            .map_err(|e| e.to_string())?;
        let weight = *weight as u64;
        checked_total = checked_total
            .checked_add(weight)
            .ok_or("initial_fault_weight_overflow")?;
        weights.insert(validator.clone(), weight);
    }
    meter
        .allocate(tracker.rows().len(), 32 * std::mem::size_of::<usize>())
        .map_err(|e| e.to_string())?;
    let mut checked_matched = 0u64;
    let mut matched_records = 0usize;
    let mut scans = (weights.len() as u64)
        .checked_add(4)
        .ok_or("display_work_overflow")?;
    let mut distinct = BTreeSet::new();
    for row in tracker.rows() {
        let key_cost = (row.equivocator.len() as u64)
            .checked_add(1)
            .ok_or("display_work_overflow")?;
        scans = scans.checked_add(key_cost).ok_or("display_work_overflow")?;
        let compare_cost = key_cost
            .checked_mul(
                (tracker.rows().len() as u64)
                    .checked_add(1)
                    .ok_or("display_work_overflow")?,
            )
            .ok_or("display_work_overflow")?;
        meter
            .charge(WorkKind::Oracle, compare_cost, 0)
            .map_err(|e| e.to_string())?;
        if let Some(weight) = weights.get(&row.equivocator) {
            checked_matched = checked_matched
                .checked_add(*weight)
                .ok_or("initial_fault_weight_overflow")?;
            matched_records += 1;
            distinct.insert(&row.equivocator);
        }
    }
    meter
        .charge(WorkKind::Oracle, scans, 0)
        .map_err(|e| e.to_string())?;
    let matched =
        initial_fault::equivocating_weight(&weights, tracker.rows().iter().map(|r| &r.equivocator));
    let total = initial_fault::total_weight(&weights);
    if matched != checked_matched || total != checked_total {
        return Err("initial_fault_sum_mismatch".into());
    }
    let fault = initial_fault::normalized_initial_fault(matched, total);
    let projection = initial_fault::display_projection(f32::from_bits(base_bits), fault);
    meter.allocate(1, 256).map_err(|e| e.to_string())?;
    Ok((projection.to_bits(), DisplayInputs {
        base_source,
        base_bits,
        initial_fault_bits: fault.to_bits(),
        equivocating_weight: matched.to_string(),
        total_weight: total.to_string(),
        matched_records,
        distinct_equivocators: distinct.len(),
        finalized_set_member,
        metadata_finalized,
        equivocation_digest: hex::encode(tracker.digest()),
    }))
}
