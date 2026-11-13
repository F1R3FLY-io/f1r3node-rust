//! `slashing_arithmetic` — boundary fuzz for the sequence-number helpers in
//! `slashing_authorization.rs`.
//!
//!   1. `checked_base_seq` — exclusive-lower-bound predecessor. `None` for
//!      every `seq <= 0`: sequence 0 has no predecessor and negatives are
//!      invalid wire values.
//!   2. `checked_next_seq` — u64 successor narrowed to the i32 wire type.
//!      `None` on u64 overflow or i32 truncation.
//!
//! The asserts compare against re-derived expressions, so a regression to
//! wrapping arithmetic surfaces immediately.

#![no_main]

use casper::rust::slashing_authorization::{checked_base_seq, checked_next_seq};
use libfuzzer_sys::fuzz_target;

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
    seq_i32: i32,
    seq_u64: u64,
}

fuzz_target!(|input: Input| {
    let expected_base = if input.seq_i32 <= 0 {
        None
    } else {
        Some(input.seq_i32 - 1)
    };
    assert_eq!(checked_base_seq(input.seq_i32), expected_base);

    let expected_next = input
        .seq_u64
        .checked_add(1)
        .and_then(|seq| i32::try_from(seq).ok());
    assert_eq!(checked_next_seq(input.seq_u64), expected_next);
});
