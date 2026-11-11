//! `slash_authorization_paths` — differential testing of slash authorization.
//!
//! Production code under test: `slashing_authorization::authorized_slash_candidates`
//! and `validate_received_slash_deploys`. Each iteration runs both against
//! an independent oracle built from really signed blocks.
//!
//! Proposer side: one candidate per bonded offender the snapshot holds an
//! equivocation of, from its lowest-hash invalid block, with the evidence
//! ordered by hash.
//!
//! Receive side: a block's slashes are accepted iff every slash
//!   1. is issued by the block's sender,
//!   2. cites two blocks of this shard and chain,
//!   3. signed by one sender, at one sequence number,
//!   4. that are two blocks, ordered by hash, each bearing its sender's
//!      signature over its own content,
//!   5. whose sender is bonded with positive stake,
//! and no two slashes target one offender.

#![no_main]

use std::collections::{BTreeMap, HashMap, HashSet};

use arbitrary::Arbitrary;
use casper::rust::slashing_authorization::{
    authorized_slash_candidates, validate_received_slash_deploys, AuthorizedSlashCandidate,
};
use casper::rust::util::proto_util;
use libfuzzer_sys::fuzz_target;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
use num_bigint::BigInt;
use prost::bytes::Bytes;

mod support;

const VALIDATORS: u8 = 4;

#[derive(Arbitrary, Debug)]
struct PairInput {
    offender: u8,
    seq_num: u8,
    nonce_a: u8,
    nonce_b: u8,
}

/// Each condition the receiver checks is one flag, so the fuzzer reaches
/// slashes that fail exactly one of them.
#[derive(Arbitrary, Debug)]
struct SlashInput {
    kind: u8,
    foreign_issuer: bool,
    offender: u8,
    other_sender: bool,
    seq_num: u8,
    other_seq: bool,
    nonce_a: u8,
    nonce_b: u8,
    other_shard: bool,
    other_chain: bool,
    tamper: bool,
    swap: bool,
}

#[derive(Arbitrary, Debug)]
struct Input {
    proposer: u8,
    stakes: [i8; VALIDATORS as usize],
    pairs: Vec<PairInput>,
    slashes: Vec<SlashInput>,
}

fn validator(index: u8) -> Validator { support::public(&support::key(index % VALIDATORS)) }

fn bonded(bonds: &HashMap<Validator, BigInt>, validator: &Validator) -> bool {
    bonds
        .get(validator)
        .is_some_and(|stake| *stake > BigInt::from(0))
}

/// The slash `input` describes, and whether the oracle accepts it, with the
/// offender it targets when the evidence names one sender.
fn slash(
    input: &SlashInput,
    proposer_index: u8,
    bonds: &HashMap<Validator, BigInt>,
) -> (
    models::rust::casper::protocol::casper_message::ProcessedSystemDeploy,
    bool,
    Validator,
) {
    let offender = input.offender % VALIDATORS;
    let second_sender = offender + u8::from(input.other_sender);
    let seq_num = i32::from(input.seq_num % 4);
    let a = support::signed_block(
        &support::key(offender),
        seq_num,
        input.nonce_a,
        support::SHARD,
        &support::CHAIN,
    );
    let b = support::signed_block(
        &support::key(second_sender % VALIDATORS),
        seq_num + i32::from(input.other_seq),
        input.nonce_b,
        if input.other_shard {
            "other"
        } else {
            support::SHARD
        },
        if input.other_chain {
            &[0xC2; 32]
        } else {
            &support::CHAIN
        },
    );
    let mut evidence = support::evidence(&a, &b);
    if input.tamper {
        evidence.second.post_state_hash = Bytes::from(vec![0xAB; 32]);
    }
    if input.swap {
        std::mem::swap(&mut evidence.first, &mut evidence.second);
    }
    let issuer = validator(proposer_index + u8::from(input.foreign_issuer));
    let first_hash = proto_util::commitment_hash(&evidence.first);
    let second_hash = proto_util::commitment_hash(&evidence.second);
    let offender = a.sender.clone();
    let accepted = !input.foreign_issuer
        && !input.other_shard
        && !input.other_chain
        && a.sender == b.sender
        && a.seq_num == b.seq_num
        && a.block_hash != b.block_hash
        && !input.tamper
        && first_hash < second_hash
        && bonded(bonds, &offender);
    (support::slash_deploy(evidence, &issuer), accepted, offender)
}

fuzz_target!(|input: Input| {
    let bonds_list = (0..VALIDATORS)
        .map(|index| {
            (
                validator(index),
                i64::from(input.stakes[usize::from(index)]),
            )
        })
        .collect::<Vec<_>>();
    let bonds: HashMap<Validator, BigInt> = bonds_list
        .iter()
        .map(|(validator, stake)| (validator.clone(), BigInt::from(*stake)))
        .collect();
    let slashable: HashMap<Validator, BigInt> = bonds
        .iter()
        .filter(|(validator, _)| bonded(&bonds, validator))
        .map(|(validator, stake)| (validator.clone(), stake.clone()))
        .collect();

    let mut held = Vec::new();
    for pair in input.pairs.iter().take(6) {
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
    let store = support::block_store(&held);
    let blocks: HashMap<BlockHash, BlockMessage> = held
        .iter()
        .map(|item| (item.block.block_hash.clone(), item.block.clone()))
        .collect();

    let mut lowest: BTreeMap<Validator, (BlockHash, BlockHash)> = BTreeMap::new();
    for invalid in snapshot.dag.invalid_blocks() {
        if !bonded(&bonds, &invalid.sender) {
            continue;
        }
        let sibling = invalid
            .equivocates
            .clone()
            .expect("the snapshot stores every invalid block with its sibling");
        let entry = lowest
            .entry(invalid.sender.clone())
            .or_insert((invalid.block_hash.clone(), sibling.clone()));
        if invalid.block_hash < entry.0 {
            *entry = (invalid.block_hash.clone(), sibling);
        }
    }
    let expected_candidates = lowest
        .into_iter()
        .map(|(offender, (invalid, sibling))| AuthorizedSlashCandidate {
            offender,
            evidence: support::evidence(&blocks[&invalid], &blocks[&sibling]),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        authorized_slash_candidates(&snapshot, &store, &slashable)
            .expect("every held block is stored"),
        expected_candidates
    );

    let proposer_index = input.proposer % VALIDATORS;
    let proposer = validator(proposer_index);
    let mut expected_ok = true;
    let mut targeted = HashSet::new();
    let mut deploys = Vec::new();
    for item in input.slashes.iter().take(6) {
        match item.kind % 4 {
            0 => deploys.push(support::failed_deploy()),
            1 => deploys.push(support::close_deploy()),
            _ => {
                let (deploy, accepted, offender) = slash(item, proposer_index, &bonds);
                expected_ok &= accepted && targeted.insert(offender);
                deploys.push(deploy);
            }
        }
    }
    let block = support::block_with_system_deploys(input.proposer, proposer, 5, deploys);
    assert_eq!(
        validate_received_slash_deploys(&block, &slashable, support::SHARD, &support::CHAIN)
            .is_ok(),
        expected_ok
    );
});
