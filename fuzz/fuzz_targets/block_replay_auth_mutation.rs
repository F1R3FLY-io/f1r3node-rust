//! Fuzz block-level authentication of replay cost fields.
//!
//! Mutating scalar cost, cost-trace digest, or cost-trace event count must
//! affect the production block hash boundary that validators sign and replay.

#![no_main]

use arbitrary::Arbitrary;
use casper::rust::util::proto_util::hash_block;
use libfuzzer_sys::fuzz_target;

mod cost_accounting_fuzz_support;

#[derive(Arbitrary, Debug)]
struct Input {
    seed: u8,
    cost: u64,
    digest: Vec<u8>,
    event_count: u64,
    mutation: u8,
}

fuzz_target!(|input: Input| {
    let digest = input.digest.iter().copied().take(64).collect::<Vec<_>>();
    let left = cost_accounting_fuzz_support::processed_deploy(
        input.seed,
        input.cost,
        digest.clone(),
        input.event_count,
        false,
    );
    let mut right = left.clone();
    match input.mutation % 3 {
        0 => {
            right.cost.cost = right.cost.cost.wrapping_add(1);
        }
        1 => {
            let mut changed = digest;
            if changed.is_empty() {
                changed.push(1);
            } else {
                changed[0] = changed[0].wrapping_add(1);
            }
            right.cost_trace_digest = changed.into();
        }
        _ => {
            right.cost_trace_event_count = right.cost_trace_event_count.wrapping_add(1);
        }
    }

    let left_hash = hash_block(&cost_accounting_fuzz_support::block_with_deploy(left));
    let right_hash = hash_block(&cost_accounting_fuzz_support::block_with_deploy(right));
    assert_ne!(left_hash, right_hash);
});
