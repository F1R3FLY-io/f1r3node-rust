//! Nays on overflow claims that the voter's own signed ancestry contradicts.
//!
//! A validator that signs a block has accepted every block in its ancestry,
//! and each of the claimant's blocks there spilled at least its reward less
//! the claimant's stake in that block's committee into the claimant's
//! overflow, whatever the voter's bucket stood at. Under the freezes and the
//! payments the vote block's floor records, which the vote itself carries, the
//! sum of those spills less everything paid is overflow every honest view of
//! that ancestry holds. A nay on a claim for no more than that is dishonest.

use std::collections::HashSet;
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{ClaimFamily, ClaimId, OverflowTestimony};
use models::rust::validator::Validator;
use num_bigint::BigInt;

use super::reward::{compute_reward, fresh_in, Committee, ParentFact, RewardParams, PPM};
use crate::rust::errors::CasperError;

/// One validator's nay, in one block, on one overflow claim.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Accusation {
    pub accused: Validator,
    pub vote_block: BlockHash,
    pub claim: ClaimId,
}

/// The spill of each of `claimant`'s blocks in the ancestry of a block citing
/// `parents`, numbered at or above `lowest` and outside the testimony's
/// freezes, with the block's number. A block this node stores as an
/// equivocation counts too: which twin is stored so depends on arrival order.
fn spills(
    dag: &KeyValueDagRepresentation,
    parents: Vec<BlockHash>,
    claimant: &Validator,
    testimony: &OverflowTestimony,
    lowest: i64,
    reward: &RewardParams,
) -> Result<Vec<(i64, BigInt)>, CasperError> {
    let mut spills = Vec::new();
    let mut seen: HashSet<BlockHash> = HashSet::new();
    let mut pending = parents;
    while let Some(hash) = pending.pop() {
        if !seen.insert(hash.clone()) {
            continue;
        }
        let block = dag.lookup_unsafe(&hash)?;
        if block.block_number < lowest {
            continue;
        }
        pending.extend(block.parents.iter().cloned());
        if block.sender != *claimant
            || testimony.freezes.iter().any(|interval| {
                interval.covers(block.block_number, Some(i64::from(block.sequence_number)))
            })
        {
            continue;
        }
        let Some(stake) = block
            .weight_map
            .get(claimant)
            .filter(|stake| !stake.is_zero())
        else {
            continue;
        };
        let mut parents = Vec::with_capacity(block.parents.len());
        for parent in &block.parents {
            let parent = dag.lookup_unsafe(parent)?;
            parents.push(ParentFact {
                sender: parent.sender,
                block_number: parent.block_number,
            });
        }
        let fresh = fresh_in(
            dag,
            claimant,
            &block.justifications,
            block.block_number,
            reward.previous_reach,
        )?;
        let committee: Committee = block
            .weight_map
            .iter()
            .map(|(validator, stake)| (validator.clone(), stake.to_bigint()))
            .collect();
        let earned = compute_reward(
            &committee,
            claimant,
            &fresh,
            &parents,
            block.block_number,
            reward,
        );
        spills.push((
            block.block_number,
            (earned - stake.as_bigint() * PPM).max(BigInt::from(0)),
        ));
    }
    Ok(spills)
}

fn less_paid(spilled: BigInt, testimony: &OverflowTestimony) -> BigInt {
    (spilled - testimony.claimed_total.as_bigint() * PPM).max(BigInt::from(0))
}

fn proves(testimony: &OverflowTestimony, provable: &BigInt) -> bool {
    testimony.amount.as_bigint() * PPM <= *provable
}

/// The overflow every honest view of `vote_block`'s ancestry holds for
/// `claimant` under `testimony`: the spill of each of the claimant's blocks
/// numbered at or above `lowest` and outside its freezes, less all it was
/// paid.
pub fn provable_overflow(
    dag: &KeyValueDagRepresentation,
    vote_block: &BlockHash,
    claimant: &Validator,
    testimony: &OverflowTestimony,
    lowest: i64,
    reward: &RewardParams,
) -> Result<BigInt, CasperError> {
    provable_overflow_above(
        dag,
        dag.lookup_unsafe(vote_block)?.parents,
        claimant,
        testimony,
        lowest,
        reward,
    )
}

/// The same overflow for a block citing `parents`, before that block exists.
pub fn provable_overflow_above(
    dag: &KeyValueDagRepresentation,
    parents: Vec<BlockHash>,
    claimant: &Validator,
    testimony: &OverflowTestimony,
    lowest: i64,
    reward: &RewardParams,
) -> Result<BigInt, CasperError> {
    let spilled: BigInt = spills(dag, parents, claimant, testimony, lowest, reward)?
        .into_iter()
        .map(|(_, spill)| spill)
        .sum();
    Ok(less_paid(spilled, testimony))
}

/// Whether a nay carrying `testimony`, cast in a block citing `parents`, is
/// one its own ancestry proves dishonest at the vote's floor. A judge counts
/// from the higher of that floor and its own, so no judge proves more.
pub fn contradicts_own_ancestry(
    dag: &KeyValueDagRepresentation,
    parents: Vec<BlockHash>,
    claimant: &Validator,
    testimony: &OverflowTestimony,
    reach: Option<i64>,
    reward: &RewardParams,
) -> Result<bool, CasperError> {
    let lowest = lowest(testimony, testimony.floor_number, reach);
    let provable = provable_overflow_above(dag, parents, claimant, testimony, lowest, reward)?;
    Ok(proves(testimony, &provable))
}

/// Whether a judge at a floor numbered `floor_number` can count anything
/// for `accusation`: its vote block is a block of the accused, stored as an
/// equivocation or not, numbered no lower than the reach below that floor. Below the reach
/// nothing is counted, and a node may hold neither the vote block's body nor
/// its parents, so the answer must not depend on reading them. A vote block
/// this node does not hold is `BlockNotHeld`, so the judge fetches it rather
/// than deciding without it.
fn countable(
    dag: &KeyValueDagRepresentation,
    accusation: &Accusation,
    floor_number: i64,
    reach: Option<i64>,
) -> Result<bool, CasperError> {
    if accusation.claim.family != ClaimFamily::Overflow {
        return Ok(false);
    }
    let floor_lowest = reach.map_or(0, |reach| floor_number.saturating_sub(reach));
    let vote_block = dag.lookup_unsafe(&accusation.vote_block)?;
    Ok(vote_block.sender == accusation.accused && vote_block.block_number >= floor_lowest)
}

/// The testimony the vote block's nay on the accused claim carries, if it
/// carries one.
fn testimony(
    blocks: &KeyValueBlockStore,
    accusation: &Accusation,
) -> Result<Option<OverflowTestimony>, CasperError> {
    let body = blocks.get(&accusation.vote_block)?.ok_or_else(|| {
        CasperError::RuntimeError(format!(
            "the body of held block {} is missing",
            hex::encode(&accusation.vote_block)
        ))
    })?;
    Ok(body
        .body
        .leaky_bucket_votes
        .into_iter()
        .find(|vote| vote.claim == accusation.claim && !vote.yea)
        .and_then(|vote| vote.testimony))
}

/// The lowest block number counted for a nay carrying `testimony`, judged at
/// a floor numbered `floor_number`: `reach` below the higher of that floor
/// and the vote's.
fn lowest(testimony: &OverflowTestimony, floor_number: i64, reach: Option<i64>) -> i64 {
    reach.map_or(0, |reach| {
        testimony
            .floor_number
            .max(floor_number)
            .saturating_sub(reach)
    })
}

/// Whether `accusation` proves its nay dishonest, judged by a block whose
/// floor is numbered `floor_number`. The claimant's blocks counted reach back
/// `reach` blocks from the higher of that floor and the vote's, a span every
/// node that can judge the accusing block holds.
pub fn proves_dishonesty(
    dag: &KeyValueDagRepresentation,
    blocks: &KeyValueBlockStore,
    accusation: &Accusation,
    floor_number: i64,
    reach: Option<i64>,
    reward: &RewardParams,
) -> Result<bool, CasperError> {
    if !countable(dag, accusation, floor_number, reach)? {
        return Ok(false);
    }
    let Some(testimony) = testimony(blocks, accusation)? else {
        return Ok(false);
    };
    let provable = provable_overflow(
        dag,
        &accusation.vote_block,
        &accusation.claim.target,
        &testimony,
        lowest(&testimony, floor_number, reach),
        reward,
    )?;
    Ok(proves(&testimony, &provable))
}

/// An accusation's vote block as read once: its nay's testimony, and the
/// spill of each counted block numbered at or above `from`. Every parent is
/// numbered below its child, so every counted block at or above a higher
/// cutoff is reached through blocks above that cutoff too, and the spills
/// kept answer any cutoff at or above `from` exactly.
#[derive(Debug)]
struct Evidence {
    testimony: OverflowTestimony,
    from: i64,
    spills: Vec<(i64, BigInt)>,
}

impl Evidence {
    fn proves(&self, lowest: i64) -> bool {
        let spilled: BigInt = self
            .spills
            .iter()
            .filter(|(number, _)| *number >= lowest)
            .map(|(_, spill)| spill)
            .sum();
        proves(&self.testimony, &less_paid(spilled, &self.testimony))
    }
}

#[derive(Debug, Default)]
enum Kept {
    #[default]
    Unread,
    NoNay,
    Evidence(Evidence),
}

/// An accusation a proposer may offer, which keeps what it read of its vote
/// block across proposals. It is judged as `proves_dishonesty` judges it, and
/// walks the ancestry again only for a cutoff below the one it kept.
#[derive(Clone, Debug)]
pub struct PendingAccusation {
    pub accusation: Accusation,
    kept: Arc<parking_lot::Mutex<Kept>>,
}

/// What is kept is a function of the accusation, so it plays no part in
/// equality.
impl PartialEq for PendingAccusation {
    fn eq(&self, other: &Self) -> bool { self.accusation == other.accusation }
}

impl Eq for PendingAccusation {}

impl PendingAccusation {
    pub fn new(accusation: Accusation) -> Self {
        Self {
            accusation,
            kept: Arc::default(),
        }
    }

    /// Whether the accusation proves its nay dishonest, judged by a block
    /// whose floor is numbered `floor_number`.
    pub fn proves(
        &self,
        dag: &KeyValueDagRepresentation,
        blocks: &KeyValueBlockStore,
        floor_number: i64,
        reach: Option<i64>,
        reward: &RewardParams,
    ) -> Result<bool, CasperError> {
        if !countable(dag, &self.accusation, floor_number, reach)? {
            return Ok(false);
        }
        let mut kept = self.kept.lock();
        let testimony = match &*kept {
            Kept::NoNay => return Ok(false),
            Kept::Evidence(evidence) => evidence.testimony.clone(),
            Kept::Unread => match testimony(blocks, &self.accusation)? {
                Some(testimony) => testimony,
                None => {
                    *kept = Kept::NoNay;
                    return Ok(false);
                }
            },
        };
        let lowest = lowest(&testimony, floor_number, reach);
        if let Kept::Evidence(evidence) = &*kept {
            if evidence.from <= lowest {
                return Ok(evidence.proves(lowest));
            }
        }
        let evidence = Evidence {
            spills: spills(
                dag,
                dag.lookup_unsafe(&self.accusation.vote_block)?.parents,
                &self.accusation.claim.target,
                &testimony,
                lowest,
                reward,
            )?,
            testimony,
            from: lowest,
        };
        let proven = evidence.proves(lowest);
        *kept = Kept::Evidence(evidence);
        Ok(proven)
    }
}

#[cfg(test)]
mod tests {
    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, InsertMode,
    };
    use models::rust::block_implicits::get_random_block;
    use models::rust::casper::protocol::casper_message::BlockMessage;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    fn block(
        number: i64,
        seq: i32,
        sender: Option<&Validator>,
        parents: Vec<BlockHash>,
    ) -> BlockMessage {
        get_random_block(
            Some(number),
            Some(seq),
            None,
            None,
            sender.cloned(),
            None,
            None,
            Some(parents),
            None,
            Some(vec![]),
            None,
            Some(vec![]),
            None,
            None,
        )
    }

    /// Whether a nay in an equivocator's block can be judged does not depend
    /// on which of its twins a node received first.
    #[tokio::test]
    async fn an_accusation_is_countable_whichever_twin_arrived_first() {
        let genesis = block(0, 0, None, vec![]);
        let accused: Validator = Bytes::from(vec![5u8; 65]);
        let voting = block(1, 1, Some(&accused), vec![genesis.block_hash.clone()]);
        let twin = block(1, 1, Some(&accused), vec![genesis.block_hash.clone()]);
        let accusation = Accusation {
            accused: accused.clone(),
            vote_block: voting.block_hash.clone(),
            claim: ClaimId {
                family: ClaimFamily::Overflow,
                target: Bytes::from(vec![6u8; 65]),
                opened_at: 0,
                proposer: Bytes::from(vec![6u8; 65]),
            },
        };

        let mut answers = Vec::new();
        for (first, second) in [(&voting, &twin), (&twin, &voting)] {
            let mut kvm = InMemoryStoreManager::new();
            let storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
            storage.insert(&genesis, InsertMode::Approved).unwrap();
            storage.insert(first, InsertMode::Normal).unwrap();
            let dag = storage
                .insert(second, InsertMode::Equivocation {
                    sibling: first.block_hash.clone(),
                })
                .unwrap();
            answers.push(countable(&dag, &accusation, 1, None).unwrap());
        }
        assert_eq!(answers, [true, true], "(voting block first, twin first)");
    }
}
