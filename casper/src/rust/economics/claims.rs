//! Claims recorded in PoS state, and the votes cast on them.

use std::collections::{HashMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use block_storage::rust::dag::vote_index::VoteEntry;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{
    Bond, ClaimId, ClaimOutcome, ClaimResolution, OverflowTestimony,
};
use models::rust::validator::Validator;

use super::bucket_view::Knowledge;
use crate::rust::errors::CasperError;
use crate::rust::finality::floor_context::FloorContext;
use crate::rust::rholang::runtime::FloorState;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Clone, Debug, PartialEq)]
pub struct ClaimRecord {
    pub claim: ClaimId,
    pub electorate: Vec<Bond>,
    pub terms: ClaimTerms,
}

/// The claims a state records, and the grace of every validator it holds
/// ejected.
#[derive(Clone, Debug, PartialEq)]
pub struct ClaimBook {
    pub records: Vec<ClaimRecord>,
    /// Each ejected validator and the last block of its grace.
    pub grace_ends: Vec<(Validator, i64)>,
}

impl ClaimBook {
    /// The book a block that slashes `slashed` resolves: a block's slashes run
    /// before its resolutions and close every proceeding against their
    /// validator, so that block resolves and expires none of them.
    pub fn without(&self, slashed: &HashSet<Validator>) -> ClaimBook {
        ClaimBook {
            records: self
                .records
                .iter()
                .filter(|record| !slashed.contains(&record.claim.target))
                .cloned()
                .collect(),
            grace_ends: self
                .grace_ends
                .iter()
                .filter(|(validator, _)| !slashed.contains(validator))
                .cloned()
                .collect(),
        }
    }

    /// The claims a block whose floor is at `floor_number` expires: every one
    /// its pre-state records whose deadline that floor has reached and which
    /// `resolved` leaves undecided. No vote is cast past the deadline, so such
    /// a claim can no longer cross. In identity order.
    pub fn expiries(
        &self,
        floor_number: i64,
        claim_deadline: i64,
        resolved: &[ClaimResolution],
    ) -> Vec<ClaimId> {
        let mut expired: Vec<ClaimId> = self
            .records
            .iter()
            .filter(|record| {
                record.claim.opened_at.saturating_add(claim_deadline) <= floor_number
                    && !resolved
                        .iter()
                        .any(|resolution| resolution.claim == record.claim)
            })
            .map(|record| record.claim.clone())
            .collect();
        expired.sort();
        expired
    }

    /// The ejected validators whose grace a floor at `floor_number` has
    /// passed, in identity order. Grace runs over floor blocks, from the floor
    /// the ejection passed at.
    pub fn grace_expiries(&self, floor_number: i64) -> Vec<Validator> {
        let mut expired: Vec<Validator> = self
            .grace_ends
            .iter()
            .filter(|(_, grace_end)| *grace_end < floor_number)
            .map(|(validator, _)| validator.clone())
            .collect();
        expired.sort();
        expired
    }
}

/// What a claim asks for, beyond its target, by family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimTerms {
    Eject,
    /// `topup` is paid on success. A rejoin can pass only at a floor up to
    /// `grace_end`, so only votes in blocks up to it can count.
    Rejoin {
        topup: i64,
        grace_end: i64,
    },
    /// `amount`, in bond units, is paid to the claimant on success.
    Overflow {
        amount: i64,
    },
}

impl ClaimRecord {
    pub fn is_open_at(&self, block_number: i64, claim_deadline: i64) -> bool {
        let within_deadline = self.claim.opened_at.saturating_add(claim_deadline) > block_number;
        match self.terms {
            ClaimTerms::Eject | ClaimTerms::Overflow { .. } => within_deadline,
            ClaimTerms::Rejoin { grace_end, .. } => within_deadline && block_number <= grace_end,
        }
    }

    /// Whether a block whose floor is at `floor_number` may resolve this
    /// claim. Votes are cast only within the claim's lifetime and are visible
    /// only once a floor shows them, so the tally decides whenever it crosses,
    /// however far past the deadline the floor lies. A rejoin may pass only at
    /// a floor within its grace.
    pub fn is_decidable_at(&self, floor_number: i64) -> bool {
        match self.terms {
            ClaimTerms::Eject | ClaimTerms::Overflow { .. } => true,
            ClaimTerms::Rejoin { grace_end, .. } => floor_number <= grace_end,
        }
    }

    pub fn may_vote(&self, voter: &Validator) -> bool {
        self.electorate
            .iter()
            .any(|bond| bond.validator == voter && bond.stake > 0)
    }
}

/// A validator proposing a block: the block it last proposed, and the number
/// of the block it is proposing.
pub struct Voter<'a> {
    pub validator: &'a Validator,
    pub last_own_block: Option<&'a BlockHash>,
    pub block_number: i64,
}

/// The votes valid blocks carry, read against one view of the DAG.
pub struct VoteHistory<'a> {
    storage: &'a BlockDagKeyValueStorage,
    blocks: &'a KeyValueBlockStore,
    dag: &'a KeyValueDagRepresentation,
}

impl<'a> VoteHistory<'a> {
    pub fn new(
        storage: &'a BlockDagKeyValueStorage,
        blocks: &'a KeyValueBlockStore,
        dag: &'a KeyValueDagRepresentation,
    ) -> Self {
        Self {
            storage,
            blocks,
            dag,
        }
    }

    /// The vote `voter` cast on each of `claims` in `at` or in a block below
    /// it on `voter`'s own chain. A vote on another branch of an equivocating
    /// voter does not count. The chain is walked once for all the claims the
    /// vote index answers, and once more through block bodies for those it
    /// has pruned, so the answer never depends on how far this node pruned.
    pub fn votes_at_or_below<'c>(
        &self,
        voter: &Validator,
        at: &BlockHash,
        claims: impl IntoIterator<Item = &'c ClaimId>,
    ) -> Result<HashMap<&'c ClaimId, bool>, CasperError> {
        let mut found = Vec::new();
        let mut pruned = Vec::new();
        for claim in claims {
            match self.storage.leaky_bucket_votes(claim, voter)? {
                Some(entries) => found.extend(entries.into_iter().map(|entry| (claim, entry))),
                None => pruned.push(claim),
            }
        }
        let mut votes = self.indexed_votes(at, found)?;
        votes.extend(self.votes_in_bodies(at, pruned)?);
        Ok(votes)
    }

    fn indexed_votes<'c>(
        &self,
        at: &BlockHash,
        found: Vec<(&'c ClaimId, VoteEntry)>,
    ) -> Result<HashMap<&'c ClaimId, bool>, CasperError> {
        if found.is_empty() {
            return Ok(HashMap::new());
        }

        let at_seq = self.dag.lookup_unsafe(at)?.sequence_number;
        let Some(lowest) = found
            .iter()
            .map(|(_, entry)| entry.sequence_number)
            .filter(|seq| *seq <= at_seq)
            .min()
        else {
            return Ok(HashMap::new());
        };
        // A valid block's sequence number is one more than its
        // self-justification's, so the chain reaches `lowest` in this many
        // steps.
        let mut chain = HashSet::new();
        let mut cursor = at.clone();
        chain.insert(cursor.to_vec());
        for _ in lowest..at_seq {
            cursor = self.dag.self_justification(&cursor)?.ok_or_else(|| {
                CasperError::RuntimeError(format!(
                    "self-justification chain ends before sequence number {lowest}"
                ))
            })?;
            chain.insert(cursor.to_vec());
        }

        Ok(found
            .into_iter()
            .filter(|(_, entry)| chain.contains(&entry.block_hash))
            .map(|(claim, entry)| (claim, entry.yea))
            .collect())
    }

    /// The votes on `claims` read from the bodies of the blocks on the
    /// voter's own chain down from `at`. A vote on a claim sits in a block
    /// numbered above the one that opened it, so the walk stops there.
    fn votes_in_bodies<'c>(
        &self,
        at: &BlockHash,
        claims: Vec<&'c ClaimId>,
    ) -> Result<HashMap<&'c ClaimId, bool>, CasperError> {
        let mut votes = HashMap::new();
        let Some(lowest_opened) = claims.iter().map(|claim| claim.opened_at).min() else {
            return Ok(votes);
        };
        let mut cursor = at.clone();
        loop {
            let metadata = self.dag.lookup_unsafe(&cursor)?;
            if metadata.block_number <= lowest_opened {
                break;
            }
            let block = self.blocks.get(&cursor)?.ok_or_else(|| {
                CasperError::RuntimeError(format!(
                    "the body of block {} on a voter's chain is not held",
                    hex::encode(&cursor)
                ))
            })?;
            for vote in &block.body.leaky_bucket_votes {
                if let Some(claim) = claims.iter().find(|claim| ***claim == vote.claim) {
                    votes.entry(*claim).or_insert(vote.yea);
                }
            }
            match self.dag.self_justification(&cursor)? {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(votes)
    }

    /// The claims `voter` owes a vote on in a block whose pre-state holds
    /// `records`: every one open at that block that `voter` may vote on and
    /// has not voted on, oldest first.
    pub fn owed<'r>(
        &self,
        records: &'r [ClaimRecord],
        voter: &Voter,
        claim_deadline: i64,
    ) -> Result<Vec<&'r ClaimRecord>, CasperError> {
        let eligible: Vec<&ClaimRecord> = records
            .iter()
            .filter(|record| {
                record.is_open_at(voter.block_number, claim_deadline)
                    && record.may_vote(voter.validator)
            })
            .collect();
        let voted = match voter.last_own_block {
            Some(at) => self.votes_at_or_below(
                voter.validator,
                at,
                eligible.iter().map(|record| &record.claim),
            )?,
            None => HashMap::new(),
        };

        let mut owed: Vec<&ClaimRecord> = eligible
            .into_iter()
            .filter(|record| !voted.contains_key(&record.claim))
            .collect();
        owed.sort_by(oldest_first);
        Ok(owed)
    }

    /// The outcome the votes visible at `floor` give each claim that floor
    /// may decide, oldest claim first, per [`tally`].
    pub fn resolutions(
        &self,
        records: &[ClaimRecord],
        floor: &FloorView,
    ) -> Result<Vec<ClaimResolution>, CasperError> {
        let mut open: Vec<&ClaimRecord> = records
            .iter()
            .filter(|record| record.is_decidable_at(floor.number))
            .collect();
        open.sort_by(oldest_first);

        let mut claims_by_voter: HashMap<Validator, Vec<&ClaimId>> = HashMap::new();
        for record in &open {
            for bond in counted(&record.electorate, &floor.slashed) {
                claims_by_voter
                    .entry(bond.validator.clone())
                    .or_default()
                    .push(&record.claim);
            }
        }
        let mut votes: HashMap<(&ClaimId, Validator), bool> = HashMap::new();
        for (voter, claims) in claims_by_voter {
            let Some(latest) = floor.latest.get(&voter) else {
                continue;
            };
            for (claim, yea) in self.votes_at_or_below(&voter, latest, claims)? {
                votes.insert((claim, voter.clone()), yea);
            }
        }

        open.into_iter()
            .filter_map(|record| {
                let outcome = tally(&record.electorate, &floor.slashed, &floor.seated, |voter| {
                    votes.get(&(&record.claim, voter.clone())).copied()
                })?;
                Some(ClaimResolution {
                    claim: record.claim.clone(),
                    outcome,
                    from_sequence: None,
                })
            })
            .map(|mut resolution| {
                if resolution.freezes_target() {
                    resolution.from_sequence =
                        Some(self.sequence_at(&resolution.claim.target, floor)?);
                }
                Ok(resolution)
            })
            .collect()
    }

    /// `validator`'s sequence number in its latest block at `floor`, or 0 if
    /// the floor holds no block of its own: its slot then holds genesis or
    /// another sender's block, and every block it signs is above 0.
    fn sequence_at(&self, validator: &Validator, floor: &FloorView) -> Result<i64, CasperError> {
        let Some(latest) = floor.latest.get(validator) else {
            return Ok(0);
        };
        let metadata = self.dag.lookup_unsafe(latest)?;
        Ok(if metadata.sender == *validator {
            i64::from(metadata.sequence_number)
        } else {
            0
        })
    }
}

fn counted<'e>(
    electorate: &'e [Bond],
    slashed: &'e HashSet<Validator>,
) -> impl Iterator<Item = &'e Bond> {
    electorate
        .iter()
        .filter(move |bond| bond.stake > 0 && !slashed.contains(&bond.validator))
}

/// The outcome `vote_of` gives a claim with `electorate`: it passes when its
/// yea stake exceeds two thirds of the electorate's, and fails when its nay
/// stake does. A voter in `slashed` counts in neither the votes nor the
/// electorate. A member outside `seated` that has not voted counts for
/// nothing either: it holds no seat, so it can never vote.
pub fn tally(
    electorate: &[Bond],
    slashed: &HashSet<Validator>,
    seated: &HashSet<Validator>,
    vote_of: impl Fn(&Validator) -> Option<bool>,
) -> Option<ClaimOutcome> {
    let (mut total, mut yea, mut nay) = (0i128, 0i128, 0i128);
    for bond in counted(electorate, slashed) {
        let stake = bond.stake as i128;
        match vote_of(&bond.validator) {
            Some(true) => yea += stake,
            Some(false) => nay += stake,
            None if seated.contains(&bond.validator) => {}
            None => continue,
        }
        total += stake;
    }
    if 3 * yea > 2 * total {
        Some(ClaimOutcome::Pass)
    } else if 3 * nay > 2 * total {
        Some(ClaimOutcome::Fail)
    } else {
        None
    }
}

fn oldest_first(a: &&ClaimRecord, b: &&ClaimRecord) -> std::cmp::Ordering {
    (
        a.claim.opened_at,
        &a.claim.target,
        &a.claim.proposer,
        a.claim.family,
    )
        .cmp(&(
            b.claim.opened_at,
            &b.claim.target,
            &b.claim.proposer,
            b.claim.family,
        ))
}

/// What a nay on `record` must carry: for an overflow claim, the amount and
/// what `floor` records of the claimant; for anything else, nothing.
pub fn testimony(record: &ClaimRecord, yea: bool, floor: &FloorView) -> Option<OverflowTestimony> {
    match record.terms {
        ClaimTerms::Overflow { amount } if !yea => Some(OverflowTestimony {
            amount,
            floor_number: floor.number,
            claimed_total: floor.knowledge.claimed_total(&record.claim.target),
            freezes: floor.knowledge.freezes_of(&record.claim.target).to_vec(),
        }),
        _ => None,
    }
}

/// What a finalized floor shows: each validator's latest block in its view,
/// who has been slashed by it, who holds a seat on its committee, and what its
/// state records of the buckets.
pub struct FloorView {
    pub hash: BlockHash,
    pub number: i64,
    pub knowledge: Knowledge,
    latest: HashMap<Validator, BlockHash>,
    slashed: HashSet<Validator>,
    seated: HashSet<Validator>,
}

impl FloorView {
    pub async fn read(
        dag: &KeyValueDagRepresentation,
        runtime_manager: &RuntimeManager,
        floor: &FloorContext,
    ) -> Result<Self, CasperError> {
        let metadata = dag.lookup_unsafe(&floor.floor.hash)?;
        let state = runtime_manager.floor_state(&floor.floor_state).await?;
        Ok(Self::new(&metadata, &state))
    }

    /// A slashed validator keeps its key with no stake, in `allBonds` or, if
    /// it was unbonding, in `withdrawers`; a validator that unbonded honestly
    /// keeps its stake there until paid out.
    pub fn new(floor: &BlockMetadata, state: &FloorState) -> Self {
        let mut latest: HashMap<Validator, BlockHash> = floor
            .justifications
            .iter()
            .map(|justification| {
                (
                    justification.validator.clone(),
                    justification.latest_block_hash.clone(),
                )
            })
            .collect();
        latest.insert(floor.sender.clone(), floor.block_hash.clone());
        let slashed = state
            .bonds
            .iter()
            .chain(state.withdrawers.iter())
            .filter(|bond| bond.stake == 0)
            .map(|bond| bond.validator.clone())
            .collect();
        let seated = state
            .committee()
            .filter(|bond| bond.stake > 0)
            .map(|bond| bond.validator.clone())
            .collect();
        Self {
            hash: floor.block_hash.clone(),
            number: floor.block_number,
            knowledge: Knowledge::of(state, floor.block_number),
            latest,
            slashed,
            seated,
        }
    }
}

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::ClaimFamily;

    use super::*;

    fn validator(byte: u8) -> Validator { Validator::from(vec![byte; 33]) }

    fn rejoin(opened_at: i64, grace_end: i64) -> ClaimRecord {
        ClaimRecord {
            claim: ClaimId {
                family: ClaimFamily::Rejoin,
                target: validator(1),
                opened_at,
                proposer: validator(1),
            },
            electorate: Vec::new(),
            terms: ClaimTerms::Rejoin {
                topup: 1,
                grace_end,
            },
        }
    }

    /// Claims opened in the same block by the same validator on the same
    /// target sort the same way whatever order they are read in.
    #[test]
    fn claims_tied_on_block_target_and_proposer_sort_by_family() {
        let rejoin = rejoin(5, 100);
        let overflow = ClaimRecord {
            claim: ClaimId {
                family: ClaimFamily::Overflow,
                ..rejoin.claim.clone()
            },
            electorate: Vec::new(),
            terms: ClaimTerms::Overflow { amount: 1 },
        };
        let mut read_one_way = vec![&rejoin, &overflow];
        let mut read_the_other = vec![&overflow, &rejoin];
        read_one_way.sort_by(oldest_first);
        read_the_other.sort_by(oldest_first);
        assert_eq!(read_one_way, read_the_other);
    }

    /// A rejoin succeeds when its tally crosses at a floor within grace,
    /// even in a block past it.
    #[test]
    fn a_rejoin_is_decided_by_its_floor_against_grace() {
        let record = rejoin(95, 100);
        assert!(record.is_decidable_at(100));
        assert!(!record.is_decidable_at(101));
        assert!(!record.is_open_at(101, 10), "no vote past grace can count");
    }

    /// Votes are owed only within a claim's lifetime, but its tally decides
    /// at any floor, and a claim expires only once a floor reaches its
    /// deadline with the tally still undecided.
    #[test]
    fn a_claim_is_decided_and_expired_by_its_floor() {
        let record = ClaimRecord {
            claim: ClaimId {
                family: ClaimFamily::Overflow,
                target: validator(1),
                opened_at: 1,
                proposer: validator(1),
            },
            electorate: Vec::new(),
            terms: ClaimTerms::Overflow { amount: 1 },
        };
        assert!(
            !record.is_open_at(11, 10),
            "no vote is owed past the deadline"
        );
        assert!(
            record.is_decidable_at(11),
            "a floor past the deadline still decides"
        );

        let book = ClaimBook {
            records: vec![record.clone()],
            grace_ends: Vec::new(),
        };
        assert!(book.expiries(10, 10, &[]).is_empty());
        assert_eq!(book.expiries(11, 10, &[]), vec![record.claim.clone()]);
        let passed = ClaimResolution {
            claim: record.claim,
            outcome: ClaimOutcome::Pass,
            from_sequence: None,
        };
        assert!(
            book.expiries(11, 10, &[passed]).is_empty(),
            "a claim its floor decides does not expire"
        );
    }

    /// Grace runs over floor blocks, so the floor decides when it ends.
    #[test]
    fn grace_expires_once_the_floor_passes_it() {
        let book = ClaimBook {
            records: Vec::new(),
            grace_ends: vec![
                (validator(2), 100),
                (validator(1), 100),
                (validator(3), 101),
            ],
        };
        assert!(book.grace_expiries(100).is_empty());
        assert_eq!(book.grace_expiries(101), vec![validator(1), validator(2)]);
    }
}
