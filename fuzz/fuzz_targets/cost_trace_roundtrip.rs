//! Fuzz canonical cost-trace digest behavior.
//!
//! Successful reservation order may vary under parallel execution, but the
//! finalized digest canonicalizes successful events while preserving
//! descriptor and multiplicity sensitivity.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    BillableKind, BillableTokenEvent, RedexId, RuntimeBudget, SourcePath,
};

#[derive(Arbitrary, Debug)]
struct EventInput {
    weight: u8,
    tag: u8,
}

#[derive(Arbitrary, Debug)]
struct Input {
    events: Vec<EventInput>,
}

fn event(input: &EventInput, index: u64) -> BillableTokenEvent {
    BillableTokenEvent {
        deploy_id: [input.tag; 32],
        source_path: SourcePath(vec![input.tag as u32, index as u32]),
        redex_id: RedexId(index),
        local_index: index,
        kind: if input.tag % 2 == 0 {
            BillableKind::SourceStep
        } else {
            BillableKind::Primitive(format!("primitive-{}", input.tag))
        },
        weight: u64::from(input.weight.max(1)),
    }
}

fuzz_target!(|input: Input| {
    let events = input
        .events
        .iter()
        .take(64)
        .enumerate()
        .map(|(index, item)| event(item, index as u64))
        .collect::<Vec<_>>();

    let total = events.iter().map(|event| event.weight as i64).sum::<i64>() + 1;
    let forward = RuntimeBudget::new(Cost::create(total, "forward trace"));
    let reverse = RuntimeBudget::new(Cost::create(total, "reverse trace"));

    for event in &events {
        forward.reserve_canonical(event.clone()).unwrap();
    }
    for event in events.iter().rev() {
        reverse.reserve_canonical(event.clone()).unwrap();
    }

    assert_eq!(forward.cost_trace_digest(), reverse.cost_trace_digest());

    if let Some(first) = events.first() {
        let mutated = RuntimeBudget::new(Cost::create(total + 1, "mutated trace"));
        let mut changed = first.clone();
        changed.weight = changed.weight.saturating_add(1);
        mutated.reserve_canonical(changed).unwrap();
        for event in events.iter().skip(1) {
            mutated.reserve_canonical(event.clone()).unwrap();
        }
        assert_ne!(forward.cost_trace_digest(), mutated.cost_trace_digest());
    }
});
