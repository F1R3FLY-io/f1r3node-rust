//! `equivocation_detector_paths` — fuzz the detector boundary against an
//! independent three-way oracle.
//!
//! Reference: docs/theory/slashing/slashing-specification.md §4
//! (detection), §12 UC-01..UC-04.
//!
//! Oracle: a block whose creator-justification equals the snapshot's latest
//! message for that validator is Valid; otherwise, if the block was pulled
//! in as a dependency, AdmissibleEquivocation; otherwise IgnorableEquivocation.
//! The production detector must agree on every synthetic input.
//!
//! The `.expect(...)` at the bottom asserts the detector is *total* for
//! synthetic DAGs — synthetic DAGs cannot reach the store-error branch
//! because `support::snapshot` builds an in-memory DAG with no I/O. A
//! failure of that expect would mean the production detector grew a new
//! error path that synthetic input can trigger (regression of T-9.11).

#![no_main]

use arbitrary::Arbitrary;
use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::equivocation_detector::EquivocationDetector;
use futures::executor::block_on;
use libfuzzer_sys::fuzz_target;
use models::rust::casper::protocol::casper_message::Justification;
use rspace_plus_plus::rspace::history::Either;

mod support;

#[derive(Arbitrary, Debug)]
struct JustificationInput {
    validator: u8,
    hash: u8,
}

#[derive(Arbitrary, Debug)]
struct Input {
    requested_as_dependency: bool,
    sender: u8,
    block_hash: u8,
    block_number: i16,
    seq_num: i16,
    latest_present: bool,
    latest_hash: u8,
    creator_justification_present: bool,
    creator_justification_hash: u8,
    extra_justifications: Vec<JustificationInput>,
}

fuzz_target!(|input: Input| {
    let sender = support::validator(input.sender);
    let mut justifications = Vec::new();
    if input.creator_justification_present {
        justifications.push(Justification {
            validator: sender.clone(),
            latest_block_hash: support::block_hash(input.creator_justification_hash),
        });
    }
    // `.take(6)` bounds the synthetic justification list at 6 entries.
    // The bound is a search-space cap, not a semantic limit — production
    // blocks may have arbitrarily many justifications; 6 is the smallest
    // value that exposes both the creator-justification path and at least
    // a few cross-validator citations.
    for item in input.extra_justifications.iter().take(6) {
        justifications.push(Justification {
            validator: support::validator(item.validator),
            latest_block_hash: support::block_hash(item.hash),
        });
    }

    let mut block = support::block_with_system_deploys(
        input.block_hash,
        sender.clone(),
        i64::from(input.block_number),
        Vec::new(),
    );
    block.seq_num = i32::from(input.seq_num);
    block.justifications = justifications;

    let mut snapshot = support::snapshot(&[], i64::from(input.block_number), 1, Vec::new());
    if input.latest_present {
        snapshot
            .dag
            .latest_messages_map
            .insert(sender.clone(), support::block_hash(input.latest_hash));
    }

    let expected = if EquivocationDetector::creator_justification_hash(&block)
        == snapshot.dag.latest_message_hash(&sender)
    {
        Either::Right(ValidBlock::Valid)
    } else if input.requested_as_dependency {
        Either::Left(BlockError::Invalid(InvalidBlock::AdmissibleEquivocation))
    } else {
        Either::Left(BlockError::Invalid(InvalidBlock::IgnorableEquivocation))
    };

    let actual = block_on(EquivocationDetector::check_equivocations(
        input.requested_as_dependency,
        &block,
        &snapshot.dag,
    ))
    .expect("equivocation detector check is total for synthetic DAGs");

    assert_eq!(actual, expected);
});
