use std::collections::HashMap;

use crypto::rust::public_key::PublicKey;
use models::rust::block_hash::BlockHash;
use models::rust::cost_protocol_limits::{
    offered_funded_v6_host_work_limits, offered_funded_v6_limits,
};
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::validator::Validator;
use proptest::prelude::*;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::system_processes::BlockData;

use super::*;

fn block_data(sender: &[u8]) -> BlockData {
    BlockData {
        time_stamp: 0,
        block_number: 1,
        sender: PublicKey::from_bytes(sender),
        seq_num: 1,
    }
}

fn publication_comparison_bound() -> u64 {
    let protocol = offered_funded_v6_limits();
    [
        protocol.evidence.total_bytes,
        protocol.deploy_log_bytes,
        protocol.envelope.payload.deploy_bytes,
        protocol.envelope.payload.signing.total_bytes,
        protocol.envelope.payload.funding.wire.total_bytes,
    ]
    .into_iter()
    .try_fold(0_u64, |total, bytes| {
        total.checked_add(u64::try_from(bytes).expect("a protocol bound fits u64"))
    })
    .expect("the comparison bound does not overflow")
}

/// S3: a validator's acceptance work equals work that the producer's execution
/// budget already admitted, so the acceptance limits may never be smaller.
#[test]
fn acceptance_limits_dominate_execution_limits() {
    let acceptance = offered_acceptance_host_work_limits();
    let execution = offered_funded_v6_host_work_limits();
    for dimension in HostWorkDimension::ALL {
        assert!(
            acceptance.get(dimension).get() >= execution.get(dimension).get(),
            "{dimension:?}"
        );
    }
}

/// The producer's only acceptance work fits its acceptance budget, so the
/// acceptance budget can never stop an honest publication.
#[test]
fn publication_comparison_bound_fits_the_acceptance_limits() {
    let bound = publication_comparison_bound();
    assert_eq!(bound, 14_417_920);
    let limits = offered_acceptance_host_work_limits();
    for dimension in [
        HostWorkDimension::VerificationBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        assert!(bound <= limits.get(dimension).get(), "{dimension:?}");
    }
    let acceptance = OfferedAcceptanceBudget::new();
    for dimension in [
        HostWorkDimension::VerificationBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        acceptance
            .meter()
            .reserve(dimension, HostWorkUnits::new(bound))
            .expect("the comparison fits a fresh acceptance budget");
    }
    assert!(!acceptance.meter().is_rejected());
}

/// Result (e): exhausting an acceptance budget leaves every replay budget
/// untouched, so acceptance work can never reject the replay.
#[test]
fn acceptance_exhaustion_never_reaches_the_replay_budget() {
    let replay = HostWorkBudget::new(offered_funded_v6_host_work_limits());
    let acceptance = OfferedAcceptanceBudget::new();
    let limit = acceptance
        .meter()
        .limits()
        .get(HostWorkDimension::SearchStateBytes)
        .get();
    acceptance
        .meter()
        .reserve(
            HostWorkDimension::SearchStateBytes,
            HostWorkUnits::new(limit),
        )
        .expect("the acceptance budget admits its own limit");
    assert!(acceptance
        .meter()
        .reserve(HostWorkDimension::SearchStateBytes, HostWorkUnits::new(1))
        .is_err());
    assert!(acceptance.meter().is_rejected());
    assert!(!replay.is_rejected());
    replay
        .reserve(HostWorkDimension::SearchStateBytes, HostWorkUnits::new(1))
        .expect("the replay budget keeps its own counters");
}

/// The validator preflight charges the evidence bytes, eight times the evidence
/// bytes of search state and the deploy-log events. Their maxima fit the
/// limits, so the preflight never rejects a block whose candidate fits the
/// protocol limits that it checks first.
#[test]
fn validator_preflight_maxima_fit_the_replay_limits() {
    let protocol = offered_funded_v6_limits();
    let limits = offered_funded_v6_host_work_limits();
    let evidence = u64::try_from(protocol.evidence.total_bytes).expect("fits u64");
    let events = u64::try_from(protocol.deploy_log_events).expect("fits u64");
    assert!(evidence <= limits.get(HostWorkDimension::VerificationBytes).get());
    assert!(
        evidence.checked_add(events).expect("no overflow")
            <= limits.get(HostWorkDimension::VerificationOperations).get()
    );
    assert!(
        evidence.checked_mul(8).expect("no overflow")
            <= limits.get(HostWorkDimension::SearchStateBytes).get()
    );
}

#[test]
fn replay_context_copy_counts_the_sender_and_each_slashed_block() {
    let sender = [4_u8; 65];
    let mut slashed = HashMap::new();
    slashed.insert(
        BlockHash::from(vec![1_u8; 32]),
        Validator::from(vec![2_u8; 65]),
    );
    slashed.insert(
        BlockHash::from(vec![3_u8; 32]),
        Validator::from(vec![5_u8; 33]),
    );
    assert_eq!(
        offered_replay_context_copy_bytes(&block_data(&sender), &HashMap::new())
            .expect("no overflow"),
        65
    );
    assert_eq!(
        offered_replay_context_copy_bytes(&block_data(&sender), &slashed).expect("no overflow"),
        65 + (32 + 65) + (32 + 33)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The copy charge is a function of the entries alone. Two maps with the
    /// same entries, built in different orders with their own random hash
    /// states, give the closed form, so the verdict cannot depend on a node's
    /// hash seed or insertion order.
    #[test]
    fn replay_context_copy_does_not_depend_on_map_order(
        sender in proptest::collection::vec(any::<u8>(), 0..80),
        entries in proptest::collection::vec(
            (
                proptest::collection::vec(any::<u8>(), 32),
                proptest::collection::vec(any::<u8>(), 0..80),
            ),
            0..12,
        ),
    ) {
        let forward: HashMap<BlockHash, Validator> = entries
            .iter()
            .map(|(hash, validator)| {
                (BlockHash::from(hash.clone()), Validator::from(validator.clone()))
            })
            .collect();
        let backward: HashMap<BlockHash, Validator> = entries
            .iter()
            .rev()
            .map(|(hash, validator)| {
                (BlockHash::from(hash.clone()), Validator::from(validator.clone()))
            })
            .collect();
        let closed_form = forward.iter().fold(sender.len(), |bytes, (hash, validator)| {
            bytes + hash.len() + validator.len()
        });
        let data = block_data(&sender);
        prop_assert_eq!(
            offered_replay_context_copy_bytes(&data, &forward).expect("no overflow"),
            closed_form
        );
        prop_assert_eq!(
            offered_replay_context_copy_bytes(&data, &backward).expect("no overflow"),
            offered_replay_context_copy_bytes(&data, &forward).expect("no overflow")
        );
    }
}
