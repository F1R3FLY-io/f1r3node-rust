#![no_main]

use std::collections::{BTreeMap, BTreeSet};

use arbitrary::Arbitrary;
use casper::rust::causal_equivocation::{
    EvidenceDeltaVerdict, proposer_evidence_delta, validate_evidence_delta,
};
use libfuzzer_sys::fuzz_target;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::ObjectiveEquivocationEvidence;
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
    candidate_sender: u8,
    actual_mode: u8,
    evidences: Vec<EvidenceInput>,
}

fn validator_at(validators: &[Validator], index: u8) -> Validator {
    validators[usize::from(index) % validators.len()].clone()
}

fn expected_delta(evidences: &[support::Evidence]) -> Vec<ObjectiveEquivocationEvidence> {
    let mut first_by_hash = BTreeMap::new();
    for evidence in evidences {
        first_by_hash
            .entry(evidence.hash.clone())
            .or_insert_with(|| evidence.clone());
    }
    let mut groups = BTreeMap::<(Validator, i32), BTreeSet<_>>::new();
    for evidence in first_by_hash.into_values() {
        if evidence.sequence_number >= 0 {
            groups
                .entry((evidence.sender, evidence.sequence_number))
                .or_default()
                .insert(evidence.hash);
        }
    }
    let mut canonical = BTreeMap::<Validator, ObjectiveEquivocationEvidence>::new();
    for ((validator, sequence_number), hashes) in groups {
        if hashes.len() < 2 {
            continue;
        }
        let mut hashes = hashes.into_iter();
        let first = hashes.next().expect("two hashes");
        let second = hashes.next().expect("two hashes");
        let evidence = ObjectiveEquivocationEvidence::new(
            validator.clone(),
            BondGeneration::GENESIS,
            sequence_number,
            first,
            second,
        )
        .expect("canonical synthetic evidence");
        canonical
            .entry(validator)
            .and_modify(|current| {
                if evidence < *current {
                    *current = evidence.clone();
                }
            })
            .or_insert(evidence);
    }
    canonical.into_values().collect()
}

fn actual_delta(
    required: &[ObjectiveEquivocationEvidence],
    mode: u8,
) -> Vec<ObjectiveEquivocationEvidence> {
    let mut actual = required.to_vec();
    match mode % 8 {
        0 => {}
        1 => {
            actual.pop();
        }
        2 => {
            if let Some(first) = actual.first().cloned() {
                actual.push(first);
            }
        }
        3 => actual.reverse(),
        4 => actual.push(
            ObjectiveEquivocationEvidence::new(
                support::validator(250),
                BondGeneration::GENESIS,
                0,
                support::block_hash(240),
                support::block_hash(241),
            )
            .expect("foreign synthetic evidence"),
        ),
        5 => {
            if actual.len() >= 2 {
                actual.swap(0, 1);
            }
        }
        6 => actual.truncate(1),
        _ => actual.clear(),
    }
    actual
}

fn expected_verdict(
    required: &[ObjectiveEquivocationEvidence],
    actual: &[ObjectiveEquivocationEvidence],
) -> EvidenceDeltaVerdict {
    if actual == required {
        return EvidenceDeltaVerdict::Valid;
    }
    let actual_set = actual.iter().cloned().collect::<BTreeSet<_>>();
    let required_set = required.iter().cloned().collect::<BTreeSet<_>>();
    if actual.len() == actual_set.len()
        && actual.windows(2).all(|window| window[0] < window[1])
        && actual_set.is_subset(&required_set)
    {
        EvidenceDeltaVerdict::Neglected
    } else {
        EvidenceDeltaVerdict::Invalid
    }
}

fuzz_target!(|input: Input| {
    let validator_count = usize::from(input.validator_count % 6) + 1;
    let validators = (0..validator_count)
        .map(|index| support::validator(index as u8))
        .collect::<Vec<_>>();
    let evidences = input
        .evidences
        .iter()
        .enumerate()
        .take(12)
        .map(|(index, evidence)| support::Evidence {
            hash: support::block_hash(evidence.hash),
            sender: validator_at(&validators, evidence.sender),
            block_number: index as i64,
            sequence_number: i32::from(evidence.sequence_number.rem_euclid(16)),
            invalid: evidence.invalid,
        })
        .collect::<Vec<_>>();
    let snapshot = support::snapshot(&evidences, evidences.len() as i64, 8, Vec::new());
    let roots = evidences
        .iter()
        .map(|evidence| evidence.hash.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let required = expected_delta(&evidences);

    assert_eq!(
        proposer_evidence_delta(&roots, &snapshot.dag).expect("synthetic causal closure"),
        required
    );

    let actual = actual_delta(&required, input.actual_mode);
    let mut candidate = support::block_with_system_deploys(
        input.candidate_sender,
        validator_at(&validators, input.candidate_sender),
        evidences.len() as i64 + 1,
        Vec::new(),
    );
    candidate.header.parents_hash_list = roots;
    candidate.header.objective_equivocation_evidence_delta = actual.clone();

    assert_eq!(
        validate_evidence_delta(&candidate, &snapshot.dag).expect("synthetic evidence validation"),
        expected_verdict(&required, &actual)
    );
});
