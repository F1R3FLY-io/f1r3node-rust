//! Fuzz block-level authentication of replay cost fields.
//!
//! Mutating the scalar cost must affect the production block hash boundary
//! that validators sign and replay.

#![no_main]

use arbitrary::Arbitrary;
use casper::rust::util::proto_util::hash_block;
use libfuzzer_sys::fuzz_target;

mod cost_accounting_fuzz_support;

#[derive(Arbitrary, Debug)]
struct Input {
    seed: u8,
    cost: u64,
}

fuzz_target!(|input: Input| {
    let left = cost_accounting_fuzz_support::processed_deploy(input.seed, input.cost, false);
    let mut right = left.clone();
    right.cost.cost = right.cost.cost.wrapping_add(1);

    let left_hash = hash_block(&cost_accounting_fuzz_support::block_with_deploy(left));
    let right_hash = hash_block(&cost_accounting_fuzz_support::block_with_deploy(right));
    assert_ne!(left_hash, right_hash);
});
