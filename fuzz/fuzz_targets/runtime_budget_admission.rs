//! Structure-aware fuzzing for runtime-budget admission.
//!
//! The target drives the production `RuntimeBudget` admission path with valid
//! and invalid billable events. Invalid events must be rejected before fuel or
//! replay evidence changes; valid/OOP events must preserve conservation.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    BillableKind, BillableTokenEvent, RedexId, RuntimeBudget, SourcePath,
    MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES, MAX_COST_TRACE_SOURCE_PATH_COMPONENTS,
};

#[derive(Arbitrary, Debug)]
struct EventInput {
    weight: u64,
    kind: u8,
    descriptor_len: u16,
    path_len: u16,
}

#[derive(Arbitrary, Debug)]
struct Input {
    initial_budget: u16,
    events: Vec<EventInput>,
}

fn event(input: &EventInput, index: u64) -> BillableTokenEvent {
    let descriptor_len =
        usize::from(input.descriptor_len) % (MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES + 8);
    let path_len = usize::from(input.path_len) % (MAX_COST_TRACE_SOURCE_PATH_COMPONENTS + 8);
    let kind = match input.kind % 3 {
        0 => BillableKind::Comm,
        1 => BillableKind::Primitive("x".repeat(descriptor_len)),
        _ => BillableKind::Substitution,
    };
    BillableTokenEvent {
        deploy_id: [input.kind; 32],
        // D0: per-deploy lane key, constant within a deploy.
        sig_hash: [input.kind; 32],
        source_path: SourcePath(vec![index as u32; path_len]),
        redex_id: RedexId(index),
        local_index: index,
        kind,
        weight: input.weight,
    }
}

fn invalid_before_budget(event: &BillableTokenEvent) -> bool {
    event.weight == 0
        || event.weight > i64::MAX as u64
        || event.source_path.0.len() > MAX_COST_TRACE_SOURCE_PATH_COMPONENTS
        || matches!(
            &event.kind,
            BillableKind::Primitive(name)
                if name.len() > MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES
        )
}

fuzz_target!(|input: Input| {
    let initial = i64::from(input.initial_budget);
    let budget = RuntimeBudget::new(Cost::create(initial, "fuzz runtime budget"));

    for (index, raw) in input.events.iter().take(128).enumerate() {
        let event = event(raw, index as u64);
        let before_cost = budget.total_cost().value;
        let before_remaining = budget.remaining().value;
        let before_count = budget.cost_trace_event_count();
        let invalid = invalid_before_budget(&event);
        let result = budget.reserve_canonical(event);

        assert_eq!(
            budget.total_cost().value + budget.remaining().value,
            initial
        );
        assert!(budget.total_cost().value >= 0);
        assert!(budget.remaining().value >= 0);

        if invalid {
            assert!(result.is_err());
            assert_eq!(budget.total_cost().value, before_cost);
            assert_eq!(budget.remaining().value, before_remaining);
            assert_eq!(budget.cost_trace_event_count(), before_count);
        }
    }
});
