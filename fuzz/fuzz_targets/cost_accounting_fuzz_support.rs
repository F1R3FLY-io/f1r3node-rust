//! Shared helpers for cost-accounting fuzz targets.
//!
//! The builders stay deterministic and in-memory so each fuzz iteration checks
//! production runtime-budget and trace paths without depending on disk state.

#![allow(dead_code)]

use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    BillableKind, BillableTokenEvent, RedexId, RuntimeBudget, SourcePath,
    MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES, MAX_COST_TRACE_SOURCE_PATH_COMPONENTS,
};

pub fn runtime_budget(initial: u16, label: &'static str) -> RuntimeBudget {
    RuntimeBudget::new(Cost::create(i64::from(initial), label))
}

pub fn billable_event(
    index: u64,
    tag: u8,
    weight: u64,
    descriptor_len: usize,
    path_len: usize,
) -> BillableTokenEvent {
    // D3 (DR-9, OD-3): `Comm` is the consensus cost unit (cost 1); `Primitive` /
    // `Substitution` are diagnostic (cost 0). (`Reduction` is also diagnostic;
    // the COMM-vs-diagnostic split is what the per-COMM tally exercises.)
    let kind = match tag % 3 {
        0 => BillableKind::Comm,
        1 => BillableKind::Primitive("p".repeat(descriptor_len)),
        _ => BillableKind::Substitution,
    };
    BillableTokenEvent {
        deploy_id: [tag; 32],
        // D0: per-deploy lane key, keyed off the deploy tag (constant within
        // a deploy, distinct across deploys).
        sig_hash: [tag; 32],
        source_path: SourcePath(vec![u32::from(tag); path_len]),
        redex_id: RedexId(index),
        local_index: index,
        kind,
        weight,
    }
}

pub fn event_is_invalid(event: &BillableTokenEvent) -> bool {
    event.weight == 0
        || event.weight > i64::MAX as u64
        || event.source_path.0.len() > MAX_COST_TRACE_SOURCE_PATH_COMPONENTS
        || matches!(
            &event.kind,
            BillableKind::Primitive(name)
                if name.len() > MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES
        )
}
