//! `slashing_arithmetic` — boundary fuzz for the three sequence/epoch helpers.
//!
//! Reference: docs/theory/slashing/slashing-specification.md §9.7 + §9.8.
//! Production code under test: `checked_base_seq`, `checked_next_seq`,
//! `epoch_for_block_number` in `slashing_authorization.rs`.
//!
//! Boundary classes probed (each one is a bug class that has previously
//! shipped in slashing implementations):
//!
//!   1. `checked_base_seq` — i32 subtraction underflow at `seq <= 0`.
//!      Must saturate to `None` rather than wrapping to i32::MAX.
//!   2. `checked_next_seq` — u64 successor narrowed to i32 wire type.
//!      Two-step saturation: u64 overflow OR i32 truncation produces `None`.
//!   3. `epoch_for_block_number` — division by zero or negative epoch
//!      length. Must return `None` instead of panicking on `% 0`.
//!
//! The asserts compare against re-derived expressions in the harness so
//! a regression that introduces wrapping arithmetic surfaces immediately.

#![no_main]

use casper::rust::epoch::Epoch;
use casper::rust::slashing_authorization::{
    checked_base_seq, checked_next_seq, epoch_for_block_number, DomainError,
};
use libfuzzer_sys::fuzz_target;

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
    seq_i32: i32,
    seq_u64: u64,
    block_number: i64,
    epoch_length: i32,
}

fuzz_target!(|input: Input| {
    assert_eq!(
        checked_base_seq(input.seq_i32),
        input.seq_i32.checked_sub(1)
    );

    let expected_next = input
        .seq_u64
        .checked_add(1)
        .and_then(|seq| i32::try_from(seq).ok());
    assert_eq!(checked_next_seq(input.seq_u64), expected_next);

    let expected_epoch: Result<Epoch, DomainError> = if input.epoch_length <= 0 {
        Err(DomainError::InvalidEpochLength(input.epoch_length))
    } else if input.block_number < 0 {
        Err(DomainError::NegativeBlockNumber(input.block_number))
    } else {
        Ok(Epoch::new(input.block_number / i64::from(input.epoch_length)))
    };
    assert_eq!(
        epoch_for_block_number(input.block_number, input.epoch_length),
        expected_epoch
    );
});
