//! `equivocation_detector_paths` — fuzz the equivocation verdict against an
//! independent oracle.
//!
//! Oracle: a block is an equivocation when the snapshot holds another block
//! of its sender at its sequence number; the sibling is the lowest such
//! hash. The verdict is then admissible when the block was requested as a
//! dependency and ignorable otherwise; with no sibling it is valid.

#![no_main]

use arbitrary::Arbitrary;
use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::equivocation_detector::EquivocationDetector;
use libfuzzer_sys::fuzz_target;
use rspace_plus_plus::rspace::history::Either;

mod support;

#[derive(Arbitrary, Debug)]
struct HeldInput {
    sender: u8,
    seq_num: u8,
    nonce: u8,
}

#[derive(Arbitrary, Debug)]
struct Input {
    held: Vec<HeldInput>,
    sender: u8,
    seq_num: u8,
    nonce: u8,
    requested_as_dependency: bool,
}

fn block(
    sender: u8,
    seq_num: u8,
    nonce: u8,
) -> models::rust::casper::protocol::casper_message::BlockMessage {
    support::signed_block(
        &support::key(sender % 4),
        i32::from(seq_num % 8),
        nonce,
        support::SHARD,
        &support::CHAIN,
    )
}

fuzz_target!(|input: Input| {
    let held = input
        .held
        .iter()
        .take(12)
        .map(|item| support::Held {
            block: block(item.sender, item.seq_num, item.nonce),
            equivocates: None,
        })
        .collect::<Vec<_>>();
    let snapshot = support::snapshot(&held, 8, Vec::new());
    let candidate = block(input.sender, input.seq_num, input.nonce);

    let expected_sibling = held
        .iter()
        .map(|item| &item.block)
        .filter(|other| {
            other.sender == candidate.sender
                && other.seq_num == candidate.seq_num
                && other.block_hash != candidate.block_hash
        })
        .map(|other| other.block_hash.clone())
        .min();
    assert_eq!(
        EquivocationDetector::equivocation_of(&snapshot.dag, &candidate),
        expected_sibling
    );

    let expected_verdict = match (&expected_sibling, input.requested_as_dependency) {
        (None, _) => Either::Right(ValidBlock::Valid),
        (Some(_), true) => Either::Left(BlockError::Invalid(InvalidBlock::AdmissibleEquivocation)),
        (Some(_), false) => Either::Left(BlockError::Invalid(InvalidBlock::IgnorableEquivocation)),
    };
    assert_eq!(
        EquivocationDetector::equivocation_verdict(
            &snapshot.dag,
            &candidate,
            input.requested_as_dependency
        ),
        expected_verdict
    );
});
