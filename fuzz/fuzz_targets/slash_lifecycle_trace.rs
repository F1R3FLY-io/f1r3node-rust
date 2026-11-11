//! `slash_lifecycle_trace` — the proposer's slashes pass the receiver.
//!
//! Two phases per iteration:
//!
//!   **Phase 1.** Build a snapshot holding fuzzed equivocations, take the
//!   proposer's candidates from `authorized_slash_candidates`, and issue
//!   them in a block the proposer sends. `validate_received_slash_deploys`,
//!   judging against the same bonds, shard and chain, must accept it.
//!
//!   **Phase 2.** Issue the first candidate twice in one block. The
//!   receiver must reject it: a block slashes an offender at most once.

#![no_main]

use std::collections::HashMap;

use arbitrary::Arbitrary;
use casper::rust::slashing_authorization::{
    authorized_slash_candidates, validate_received_slash_deploys,
};
use libfuzzer_sys::fuzz_target;
use models::rust::validator::Validator;
use num_bigint::BigInt;

mod support;

const VALIDATORS: u8 = 4;

#[derive(Arbitrary, Debug)]
struct PairInput {
    offender: u8,
    seq_num: u8,
    nonce_a: u8,
    nonce_b: u8,
}

#[derive(Arbitrary, Debug)]
struct Input {
    proposer: u8,
    stakes: [i8; VALIDATORS as usize],
    pairs: Vec<PairInput>,
}

fn validator(index: u8) -> Validator { support::public(&support::key(index % VALIDATORS)) }

fuzz_target!(|input: Input| {
    let bonds_list = (0..VALIDATORS)
        .map(|index| {
            (
                validator(index),
                i64::from(input.stakes[usize::from(index)]),
            )
        })
        .collect::<Vec<_>>();
    let slashable: HashMap<Validator, BigInt> = bonds_list
        .iter()
        .filter(|(_, stake)| *stake > 0)
        .map(|(validator, stake)| (validator.clone(), BigInt::from(*stake)))
        .collect();

    let mut held = Vec::new();
    for pair in input.pairs.iter().take(8) {
        let key = support::key(pair.offender % VALIDATORS);
        let seq_num = i32::from(pair.seq_num % 4);
        let a = support::signed_block(&key, seq_num, pair.nonce_a, support::SHARD, &support::CHAIN);
        let b = support::signed_block(&key, seq_num, pair.nonce_b, support::SHARD, &support::CHAIN);
        if a.block_hash == b.block_hash {
            continue;
        }
        held.push(support::Held {
            block: a.clone(),
            equivocates: None,
        });
        held.push(support::Held {
            block: b,
            equivocates: Some(a.block_hash),
        });
    }
    let snapshot = support::snapshot(&held, 8, bonds_list);
    let candidates =
        authorized_slash_candidates(&snapshot, &support::block_store(&held), &slashable)
            .expect("every held block is stored");

    let proposer = validator(input.proposer);
    let carrier =
        |deploys| support::block_with_system_deploys(input.proposer, proposer.clone(), 5, deploys);

    let all = carrier(
        candidates
            .iter()
            .map(|candidate| support::slash_deploy(candidate.evidence.clone(), &proposer))
            .collect(),
    );
    assert!(
        validate_received_slash_deploys(&all, &slashable, support::SHARD, &support::CHAIN).is_ok(),
        "the receiver accepts the proposer's own candidates"
    );

    if let Some(first) = candidates.first() {
        let twice = carrier(vec![
            support::slash_deploy(first.evidence.clone(), &proposer),
            support::slash_deploy(first.evidence.clone(), &proposer),
        ]);
        assert!(
            validate_received_slash_deploys(&twice, &slashable, support::SHARD, &support::CHAIN)
                .is_err(),
            "a block slashing one offender twice is rejected"
        );
    }
});
