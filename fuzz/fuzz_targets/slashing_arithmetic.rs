#![no_main]

use casper::rust::slashing_authorization::{
    checked_base_seq, checked_next_seq, epoch_for_block_number,
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

    let expected_epoch = if input.block_number < 0 || input.epoch_length <= 0 {
        None
    } else {
        Some(input.block_number / i64::from(input.epoch_length))
    };
    assert_eq!(
        epoch_for_block_number(input.block_number, input.epoch_length),
        expected_epoch
    );
});
