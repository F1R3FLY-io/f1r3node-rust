use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::economics::claims::{ClaimRecord, FloorView, VoteHistory};
use casper::rust::rholang::runtime::FloorState;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, ClaimFamily, ClaimId, ClaimOutcome, Justification, LeakyBucketVote,
};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

fn block(
    number: i64,
    seq: i32,
    sender: &Validator,
    parent: &BlockMessage,
    own_previous: Option<&BlockMessage>,
) -> BlockMessage {
    let justifications = own_previous
        .map(|previous| {
            vec![Justification {
                validator: sender.clone(),
                latest_block_hash: previous.block_hash.clone(),
            }]
        })
        .unwrap_or_default();
    get_random_block(
        Some(number),
        Some(seq),
        None,
        None,
        Some(sender.clone()),
        None,
        None,
        Some(vec![parent.block_hash.clone()]),
        Some(justifications),
        Some(vec![]),
        None,
        Some(vec![]),
        None,
        None,
    )
}

/// A vote counts for a block only if it sits on that block's own chain: an
/// equivocator's vote on one branch says nothing about the other.
#[tokio::test]
async fn a_vote_counts_only_on_the_branch_that_carries_it() {
    let genesis = get_random_block(
        Some(0),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![]),
        None,
        None,
        None,
        Some(vec![]),
        None,
        None,
    );
    let voter: Validator = Bytes::from(vec![5u8; 65]);
    let claim = ClaimId {
        family: ClaimFamily::Eject,
        target: Bytes::from(vec![6u8; 65]),
        opened_at: 0,
        proposer: Bytes::from(vec![7u8; 65]),
    };

    let mut voting = block(1, 1, &voter, &genesis, None);
    voting.body.leaky_bucket_votes = vec![LeakyBucketVote {
        claim: claim.clone(),
        yea: true,
        testimony: None,
    }];
    let twin = block(1, 1, &voter, &genesis, None);
    let above_voting = block(2, 2, &voter, &voting, Some(&voting));
    let above_twin = block(2, 2, &voter, &twin, Some(&twin));

    let mut kvm = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    for b in [&voting, &twin, &above_voting, &above_twin] {
        storage.insert(b, InsertMode::Normal).unwrap();
    }
    let dag = storage.get_representation().unwrap();
    let blocks = KeyValueBlockStore::create_from_kvm(&mut kvm).await.unwrap();

    let history = VoteHistory::new(&storage, &blocks, &dag);
    let at = |b: &BlockMessage| {
        history
            .votes_at_or_below(&voter, &b.block_hash, [&claim])
            .map(|votes| votes.get(&claim).copied())
    };
    assert_eq!(at(&voting).unwrap(), Some(true));
    assert_eq!(at(&above_voting).unwrap(), Some(true));
    assert_eq!(at(&twin).unwrap(), None);
    assert_eq!(at(&above_twin).unwrap(), None);
}

fn validator(byte: u8) -> Validator { Bytes::from(vec![byte; 65]) }

fn bonds(stakes: &[(u8, i64)]) -> Vec<Bond> {
    stakes
        .iter()
        .map(|(byte, stake)| Bond {
            validator: validator(*byte),
            stake: u64::try_from(*stake)
                .expect("test stakes are non-negative")
                .into(),
        })
        .collect()
}

/// What a floor sent by `floor_sender` makes of one claim, when each of
/// `votes` was cast in its voter's latest block in the floor's view and the
/// floor itself carries `floor_vote`.
async fn tally(
    electorate: &[(u8, i64)],
    votes: &[(u8, bool)],
    floor_sender: u8,
    floor_vote: Option<bool>,
    bonds_at_floor: &[(u8, i64)],
    withdrawers_at_floor: &[(u8, i64)],
) -> Option<ClaimOutcome> {
    let genesis = get_random_block(
        Some(0),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![]),
        None,
        None,
        None,
        Some(vec![]),
        None,
        None,
    );
    let claim = ClaimId {
        family: ClaimFamily::Eject,
        target: validator(200),
        opened_at: 0,
        proposer: validator(201),
    };
    let vote = |yea: bool| LeakyBucketVote {
        claim: claim.clone(),
        yea,
        testimony: None,
    };

    let mut voter_blocks = Vec::new();
    for (voter, yea) in votes {
        let mut b = block(1, 1, &validator(*voter), &genesis, None);
        b.body.leaky_bucket_votes = vec![vote(*yea)];
        voter_blocks.push(b);
    }
    let mut floor = block(2, 1, &validator(floor_sender), &genesis, None);
    floor.justifications = voter_blocks
        .iter()
        .map(|b| Justification {
            validator: b.sender.clone(),
            latest_block_hash: b.block_hash.clone(),
        })
        .collect();
    floor.body.leaky_bucket_votes = floor_vote.map(vote).into_iter().collect();

    let mut kvm = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    for b in voter_blocks.iter().chain(std::iter::once(&floor)) {
        storage.insert(b, InsertMode::Normal).unwrap();
    }
    let dag = storage.get_representation().unwrap();
    let view = FloorView::new(
        &dag.lookup_unsafe(&floor.block_hash).unwrap(),
        &FloorState {
            bonds: bonds(bonds_at_floor),
            active_validators: bonds_at_floor
                .iter()
                .map(|(byte, _)| validator(*byte))
                .collect(),
            withdrawers: bonds(withdrawers_at_floor),
            claims: Vec::new(),
            ejected: Vec::new(),
            freezes: Vec::new(),
            claimed: Vec::new(),
        },
    );
    let record = ClaimRecord {
        claim,
        electorate: bonds(electorate),
        terms: casper::rust::economics::claims::ClaimTerms::Eject,
    };
    let blocks = KeyValueBlockStore::create_from_kvm(&mut kvm).await.unwrap();
    VoteHistory::new(&storage, &blocks, &dag)
        .resolutions(&[record], &view)
        .unwrap()
        .into_iter()
        .next()
        .map(|resolution| resolution.outcome)
}

const THREE: &[(u8, i64)] = &[(1, 1), (2, 1), (3, 1)];

/// Exactly two thirds decides nothing.
#[tokio::test]
async fn a_claim_passes_or_fails_only_beyond_two_thirds() {
    let yea = |v: &[u8]| v.iter().map(|b| (*b, true)).collect::<Vec<_>>();
    let nay = |v: &[u8]| v.iter().map(|b| (*b, false)).collect::<Vec<_>>();
    assert_eq!(tally(THREE, &yea(&[1, 2]), 9, None, THREE, &[]).await, None);
    assert_eq!(
        tally(THREE, &yea(&[1, 2, 3]), 9, None, THREE, &[]).await,
        Some(ClaimOutcome::Pass)
    );
    assert_eq!(tally(THREE, &nay(&[1, 2]), 9, None, THREE, &[]).await, None);
    assert_eq!(
        tally(THREE, &nay(&[1, 2, 3]), 9, None, THREE, &[]).await,
        Some(ClaimOutcome::Fail)
    );
}

/// A voter slashed by the floor counts in neither the votes nor the
/// electorate; one that unbonded honestly still counts.
#[tokio::test]
async fn a_slashed_voter_is_dropped_and_an_unbonded_one_is_not() {
    let votes = [(1, true), (2, true), (3, false)];
    assert_eq!(tally(THREE, &votes, 9, None, THREE, &[]).await, None);
    assert_eq!(
        tally(THREE, &votes, 9, None, &[(1, 1), (2, 1), (3, 0)], &[]).await,
        Some(ClaimOutcome::Pass)
    );
    assert_eq!(
        tally(THREE, &votes, 9, None, &[(1, 1), (2, 1)], &[(3, 1)]).await,
        None
    );
    assert_eq!(
        tally(THREE, &votes, 9, None, &[(1, 1), (2, 1)], &[(3, 0)]).await,
        Some(ClaimOutcome::Pass)
    );
}

/// A member that left the committee without voting counts for nothing; one
/// that voted before leaving still counts.
#[tokio::test]
async fn a_member_that_left_without_voting_is_dropped() {
    let yeas = [(1, true), (2, true)];
    assert_eq!(tally(THREE, &yeas, 9, None, THREE, &[]).await, None);
    assert_eq!(
        tally(THREE, &yeas, 9, None, &[(1, 1), (2, 1)], &[(3, 1)]).await,
        Some(ClaimOutcome::Pass)
    );
    assert_eq!(
        tally(THREE, &yeas, 9, None, &[(1, 1), (2, 1)], &[]).await,
        Some(ClaimOutcome::Pass)
    );
    let split = [(1, true), (2, true), (3, false)];
    assert_eq!(
        tally(THREE, &split, 9, None, &[(1, 1), (2, 1)], &[]).await,
        None
    );
}

/// Only the electorate frozen into the claim votes on it.
#[tokio::test]
async fn a_vote_from_outside_the_electorate_counts_for_nothing() {
    let votes = [(1, true), (2, true), (4, true)];
    assert_eq!(tally(THREE, &votes, 9, None, THREE, &[]).await, None);
}

/// The floor's own body is in its view.
#[tokio::test]
async fn the_floor_counts_its_own_vote() {
    let votes = [(1, true), (2, true)];
    assert_eq!(
        tally(THREE, &votes, 3, Some(true), THREE, &[]).await,
        Some(ClaimOutcome::Pass)
    );
}
