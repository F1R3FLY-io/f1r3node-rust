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
                candidate.target_activation_epoch,
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
                    candidate.target_activation_epoch,
                ),
                support::slash_deploy(
                    candidate.invalid_block_hash.clone(),
                    proposer,
                    candidate.target_activation_epoch,
                ),
            ],
        );
        assert!(validate_received_slash_deploys(&duplicate_block, &snapshot).is_err());
    }
});
