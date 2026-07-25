//! `slash_lifecycle_trace` — end-to-end fuzz of the slashing lifecycle.
//!
//! Reference: docs/theory/slashing/slashing-specification.md §6 (proposing),
//! §9.8 (authorization).
//!
//! Two phases per iteration:
//!
//!   **Phase 1 (happy path).** Build a synthetic snapshot, enumerate the
//!   authorized candidate set with `authorized_slash_candidates`, then
//!   feed those candidates back through `validate_received_slash_deploys`
//!   in a block whose sender is the proposer. The receive-side validation
//!   must accept — if it rejects its own author's candidate set, the
//!   two functions are out of sync.
//!
//!   **Phase 2 (adversary).** Take the first candidate and construct a
//!   block that issues *two* slashes for the same (offender, epoch).
//!   This models a malicious proposer attempting double-slashing within
//!   one block. The receive-side validation must reject — duplicate
//!   target-key is rule #7 in `validate_received_slash_deploys`.

#![no_main]

use arbitrary::Arbitrary;
use casper::rust::slashing_authorization::{
    authorized_slash_candidates, validate_received_slash_deploys,
};
use libfuzzer_sys::fuzz_target;
use models::rust::validator::Validator;

mod support;

#[derive(Arbitrary, Debug)]
struct EvidenceInput {
    hash: u8,
    sender: u8,
    sequence_number: i16,
    invalid: bool,
}

#[derive(Arbitrary, Debug)]
struct Input {
    validator_count: u8,
    proposer: u8,
    max_block_num: i16,
    epoch_length: i8,
    stakes: Vec<i16>,
    evidences: Vec<EvidenceInput>,
}

fn validator_at(validators: &[Validator], index: u8) -> Validator {
    validators[usize::from(index) % validators.len()].clone()
}

fn bounded_height(value: i16) -> i64 { i64::from(value.rem_euclid(16)) }

fuzz_target!(|input: Input| {
    let validator_count = usize::from(input.validator_count % 6) + 1;
    let validators = (0..validator_count)
        .map(|index| support::validator(index as u8))
        .collect::<Vec<_>>();
    let bonds = validators
        .iter()
        .enumerate()
        .map(|(index, validator)| {
            let stake = input.stakes.get(index).copied().unwrap_or(1);
            (validator.clone(), i64::from(stake))
        })
        .collect::<Vec<_>>();
    let evidences = input
        .evidences
        .iter()
        .enumerate()
        .take(10)
        .map(|(index, evidence)| support::Evidence {
            hash: support::block_hash(evidence.hash),
            sender: validator_at(&validators, evidence.sender),
            block_number: index as i64,
            sequence_number: i32::from(evidence.sequence_number),
            invalid: evidence.invalid,
        })
        .collect::<Vec<_>>();
    let max_block_num = bounded_height(input.max_block_num);
    let block_number = max_block_num + 1;
    let snapshot = support::snapshot(
        &evidences,
        max_block_num,
        i32::from(input.epoch_length),
        bonds,
    );

    // Inputs that fail authorization at the candidate-enumeration stage
    // (e.g. invalid epoch_length) carry no useful signal for *this*
    // harness — the lifecycle test wants snapshots where at least the
    // proposer-side path succeeded. `slash_authorization_paths` covers
    // the candidate-side rejection rules directly.
    let Ok(candidates) = authorized_slash_candidates(&snapshot) else {
        return;
    };

    let proposer = validator_at(&validators, input.proposer);
    let deploys = candidates
        .iter()
        .map(|candidate| {
            support::slash_deploy(
                candidate.invalid_block_hash.clone(),
                proposer.clone(),
                candidate.target_activation_epoch.get(),
            )
        })
        .collect::<Vec<_>>();
    let block =
        support::block_with_system_deploys(input.proposer, proposer.clone(), block_number, deploys);

    assert!(validate_received_slash_deploys(&block, &snapshot).is_ok());

    if let Some(candidate) = candidates.first() {
        let duplicate_block = support::block_with_system_deploys(
            input.proposer.wrapping_add(1),
            proposer.clone(),
            block_number,
            vec![
                support::slash_deploy(
                    candidate.invalid_block_hash.clone(),
                    proposer.clone(),
                    candidate.target_activation_epoch.get(),
                ),
                support::slash_deploy(
                    candidate.invalid_block_hash.clone(),
                    proposer,
                    candidate.target_activation_epoch.get(),
                ),
            ],
        );
        assert!(validate_received_slash_deploys(&duplicate_block, &snapshot).is_err());
    }
});
