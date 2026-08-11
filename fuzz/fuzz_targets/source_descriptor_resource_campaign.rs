//! Fuzz source-path and descriptor resource campaigns.
//!
//! Oversized replay descriptors and source paths must be rejected before any
//! runtime fuel or authenticated trace evidence changes.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rholang::rust::interpreter::accounting::{
    MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES, MAX_COST_TRACE_SOURCE_PATH_COMPONENTS,
};

mod cost_accounting_fuzz_support;

#[derive(Arbitrary, Debug)]
struct Input {
    initial_budget: u16,
    tag: u8,
    weight: u8,
    descriptor_len: u16,
    path_len: u16,
}

fuzz_target!(|input: Input| {
    let budget =
        cost_accounting_fuzz_support::runtime_budget(input.initial_budget, "resource campaign");
    let descriptor_len =
        usize::from(input.descriptor_len) % (MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES + 16);
    let path_len = usize::from(input.path_len) % (MAX_COST_TRACE_SOURCE_PATH_COMPONENTS + 16);
    let event = cost_accounting_fuzz_support::billable_event(
        0,
        input.tag | 1,
        u64::from(input.weight),
        descriptor_len,
        path_len,
    );
    let invalid = cost_accounting_fuzz_support::event_is_invalid(&event);
    let before_cost = budget.total_cost().value;
    let before_remaining = budget.remaining().value;
    let before_count = budget.cost_trace_event_count();
    let result = budget.reserve_canonical(event);

    if invalid {
        assert!(result.is_err());
        assert_eq!(budget.total_cost().value, before_cost);
        assert_eq!(budget.remaining().value, before_remaining);
        assert_eq!(budget.cost_trace_event_count(), before_count);
    }
    assert_eq!(
        budget.total_cost().value + budget.remaining().value,
        i64::from(input.initial_budget)
    );
});
