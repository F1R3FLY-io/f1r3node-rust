//! `slash_authorization_paths` — differential testing of slash authorization.
//!
//! Reference: docs/theory/slashing/slashing-specification.md §9 + §10.
//! Production code under test: `slashing_authorization::authorized_slash_candidates`
//! and `validate_received_slash_deploys`.
//!
//! Premise: this file contains an *independent re-implementation* of the
//! authorization predicate (`expected_validation_ok` and the
//! candidate-set construction below). Each fuzz iteration runs both the
//! production code and this oracle against the same synthetic snapshot
//! and asserts agreement on six properties:
//!
//!   1. The issuer's public key equals the block sender.
//!   2. `target_activation_epoch == current_epoch` for the block.
//!   3. The evidence block resolves in the snapshot DAG.
//!   4. The evidence block is flagged `invalid = true`.
//!   5. The offender currently carries a positive bond.
//!   6. No two slashes in the same block share (offender, target_epoch).
//!
//! A divergence between oracle and production means the production code
//! drifted from the spec — exactly the regression class T-9.8 forbids.

#![no_main]

use std::collections::{BTreeMap, BTreeSet};

use arbitrary::Arbitrary;
use casper::rust::epoch::Epoch;
use casper::rust::slashing_authorization::{
    authorized_slash_candidates, epoch_for_block_number, validate_received_slash_deploys,
};
use libfuzzer_sys::fuzz_target;
use models::rust::casper::protocol::casper_message::{ProcessedSystemDeploy, SystemDeployData};
use models::rust::validator::Validator;
use prost::bytes::Bytes;

mod support;

#[derive(Arbitrary, Debug)]
struct EvidenceInput {
    hash: u8,
    sender: u8,
    sequence_number: i16,
    invalid: bool,
}

#[derive(Arbitrary, Debug)]
struct DeployInput {
    hash: u8,
    issuer: u8,
    target_activation_epoch: i16,
    slash: bool,
    succeeded: bool,
}

#[derive(Arbitrary, Debug)]
struct Input {
    validator_count: u8,
    proposer: u8,
    max_block_num: i16,
    block_number: i16,
    epoch_length: i8,
    stakes: Vec<i16>,
    evidences: Vec<EvidenceInput>,
    deploys: Vec<DeployInput>,
}

fn validator_at(validators: &[Validator], index: u8) -> Validator {
    validators[usize::from(index) % validators.len()].clone()
}

/// Cap synthetic DAG height at 16. The bound is purely a search-space
/// cap — i16 inputs are wrapped to `[0, 16)` so libFuzzer doesn't waste
/// iterations on unreachable epoch arithmetic (epochs at i64::MAX
/// height never appear in production), while still covering the
/// epoch-boundary cases (heights 0, 1, 15 within one epoch_length=16).
fn bounded_height(value: i16) -> i64 { i64::from(value.rem_euclid(16)) }

/// Oracle predicate: enumerate the six authorization rules from §9.8
/// independently and return whether the block should validate. This is
/// the differential-testing counterpart of `validate_received_slash_deploys`
/// — every disagreement is a candidate bug.
fn expected_validation_ok(
    block: &models::rust::casper::protocol::casper_message::BlockMessage,
    snapshot: &casper::rust::casper::CasperSnapshot,
) -> bool {
    let slash_deploys = block
        .body
        .system_deploys
        .iter()
        .filter_map(|system_deploy| {
            let ProcessedSystemDeploy::Succeeded {
                system_deploy:
                    SystemDeployData::Slash {
                        invalid_block_hash,
                        issuer_public_key,
                        target_activation_epoch,
                    },
                ..
            } = system_deploy
            else {
                return None;
            };
            Some((
                invalid_block_hash.clone(),
                issuer_public_key.bytes.clone(),
                *target_activation_epoch,
            ))
        })
        .collect::<Vec<_>>();

    if slash_deploys.is_empty() {
        return true;
    }

    let epoch_length = snapshot.on_chain_state.shard_conf.epoch_length;
    let Ok(current_epoch) = epoch_for_block_number(block.body.state.block_number, epoch_length)
    else {
        return false;
    };
    let mut seen = BTreeSet::<(Bytes, i64)>::new();

    for (invalid_block_hash, issuer, target_activation_epoch) in slash_deploys {
        if issuer != block.sender {
            return false;
        }
        if target_activation_epoch != current_epoch.get() {
            return false;
        }
        let metadata = match snapshot.dag.lookup(&invalid_block_hash) {
            Ok(Some(metadata)) => metadata,
            _ => return false,
        };
        if !metadata.invalid {
            return false;
        }
        if epoch_for_block_number(metadata.block_number, epoch_length)
            != Ok(Epoch::from(target_activation_epoch))
        {
            return false;
        }
        let bond = snapshot
            .on_chain_state
            .bonds_map
            .get(&metadata.sender)
            .copied()
            .unwrap_or(0);
        if bond <= 0 {
            return false;
        }
        if !seen.insert((metadata.sender, target_activation_epoch)) {
            return false;
        }
    }

    true
}

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
        .take(8)
        .map(|(index, evidence)| support::Evidence {
            hash: support::block_hash(evidence.hash),
            sender: validator_at(&validators, evidence.sender),
            block_number: index as i64,
            sequence_number: i32::from(evidence.sequence_number),
            invalid: evidence.invalid,
        })
        .collect::<Vec<_>>();
    let epoch_length = i32::from(input.epoch_length);
    let snapshot = support::snapshot(
        &evidences,
        bounded_height(input.max_block_num),
        epoch_length,
        bonds,
    );

    let candidate_result = authorized_slash_candidates(&snapshot);
    let current_candidate_epoch =
        epoch_for_block_number(bounded_height(input.max_block_num) + 1, epoch_length);
    match current_candidate_epoch {
        Err(_) => assert!(candidate_result.is_err()),
        Ok(current_epoch) => {
            let candidates = candidate_result.expect("candidate authorization domain");
            let mut expected = BTreeMap::<Validator, (Bytes, Epoch)>::new();
            for metadata in snapshot.dag.invalid_blocks() {
                if !metadata.invalid {
                    continue;
                }
                if epoch_for_block_number(metadata.block_number, epoch_length)
                    != Ok(current_epoch)
                {
                    continue;
                }
                let bond = snapshot
                    .on_chain_state
                    .bonds_map
                    .get(&metadata.sender)
                    .copied()
                    .unwrap_or(0);
                if bond <= 0 {
                    continue;
                }
                // Lex-smallest-hash tie-breaker mirrors the production rule
                // in `authorized_slash_candidates` — see the BTreeMap
                // dedup loop there. The oracle must use the *same* rule
                // or the differential check would fire on every tie.
                expected
                    .entry(metadata.sender.clone())
                    .and_modify(|(hash, _)| {
                        if metadata.block_hash < *hash {
                            *hash = metadata.block_hash.clone();
                        }
                    })
                    .or_insert((metadata.block_hash.clone(), current_epoch));
            }
            assert_eq!(candidates.len(), expected.len());
            for candidate in candidates {
                let expected_candidate = expected
                    .remove(&candidate.offender)
                    .expect("candidate offender is expected");
                assert_eq!(candidate.invalid_block_hash, expected_candidate.0);
                assert_eq!(candidate.target_activation_epoch, expected_candidate.1);
            }
            assert!(expected.is_empty());
        }
    }

    let proposer = validator_at(&validators, input.proposer);
    let deploys = input
        .deploys
        .iter()
        .take(8)
        .map(|deploy| {
            if !deploy.succeeded {
                support::failed_deploy()
            } else if deploy.slash {
                support::slash_deploy(
                    support::block_hash(deploy.hash),
                    validator_at(&validators, deploy.issuer),
                    i64::from(deploy.target_activation_epoch),
                )
            } else {
                support::close_deploy()
            }
        })
        .collect::<Vec<_>>();
    let block = support::block_with_system_deploys(
        input.proposer,
        proposer,
        bounded_height(input.block_number),
        deploys,
    );
    let expected_ok = expected_validation_ok(&block, &snapshot);
    let actual_ok = validate_received_slash_deploys(&block, &snapshot).is_ok();
    assert_eq!(actual_ok, expected_ok);
});
