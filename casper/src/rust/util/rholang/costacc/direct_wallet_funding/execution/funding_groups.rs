//! Added by DR-134 (DR-65 items 1 to 5, O-8, O-9, O-10): the funding groups of
//! an offered batch. The decision pass at the block pre-state R0 checks every
//! candidate alone first (DR-64). This module then groups the candidates that
//! share a purse or a grant, rejects every group whose holds do not fit its
//! supply at R0 (all or none, P1:2271-2287), gives each accepted member its
//! later holds, and decides when a batch that the play budget stops must run
//! again. Every function here is pure and iterative, with no recursion.

use std::collections::{BTreeMap, BTreeSet};

/// One draw of a candidate on a grant (O-8): the grant, the hold that the
/// candidate places on it, and the remaining ceiling of the grant at R0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GrantDraw {
    pub(crate) grant: Vec<u8>,
    pub(crate) hold: u128,
    pub(crate) remaining: u128,
}

/// One candidate of a batch, as the decision pass at R0 sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BatchCandidate {
    pub(crate) identity: [u8; 32],
    /// The canonical custody of each source of the candidate.
    pub(crate) custodies: Vec<[u8; 32]>,
    /// The hold of the candidate on each purse, from its check before
    /// execution (`OfferedGate::holds`).
    pub(crate) holds: Vec<([u8; 32], u128)>,
    /// The supply of each purse of the candidate at R0, its balance there.
    pub(crate) supply: Vec<([u8; 32], u128)>,
    pub(crate) grants: Vec<GrantDraw>,
}

/// Why a funding group is rejected. Each message starts with "insufficient
/// phlogiston" (DR-123, P1:2210).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum GroupRejection {
    #[error("insufficient phlogiston: the holds {holds} of the group on a purse exceed its supply {supply}")]
    Purse {
        purse: [u8; 32],
        holds: u128,
        supply: u128,
    },
    #[error(
        "insufficient phlogiston: the holds {holds} of the group on a grant exceed its remaining ceiling {remaining}"
    )]
    Grant {
        grant: Vec<u8>,
        holds: u128,
        remaining: u128,
    },
    #[error("insufficient phlogiston: the holds of the group overflow")]
    Overflow,
}

/// A rejected group: its members (candidate positions, ascending) and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RejectedGroup {
    pub(crate) members: Vec<usize>,
    pub(crate) cause: GroupRejection,
}

/// A malformed decision input. These are defects of the caller, not
/// rejections of candidates.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum FundingGroupError {
    #[error("two candidates report different supplies for one purse")]
    SupplyDisagrees { purse: [u8; 32] },
    #[error("a later hold overflows")]
    LaterOverflow,
    #[error("a member position is outside the batch")]
    MemberOutOfRange,
}

/// The decision over a batch: the group of each candidate (the smallest
/// candidate position in it), the accepted candidates in batch order, and the
/// rejected groups in order of their smallest member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GroupDecision {
    pub(crate) group_of: Vec<usize>,
    pub(crate) accepted: Vec<usize>,
    pub(crate) rejected: Vec<RejectedGroup>,
}

/// Union-find over candidate positions, with path halving and union by size.
struct Partition {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl Partition {
    fn new(count: usize) -> Self {
        Self {
            parent: (0..count).collect(),
            size: vec![1; count],
        }
    }

    fn find(&mut self, mut node: usize) -> usize {
        while self.parent[node] != node {
            self.parent[node] = self.parent[self.parent[node]];
            node = self.parent[node];
        }
        node
    }

    fn union(&mut self, first: usize, second: usize) {
        let (first, second) = (self.find(first), self.find(second));
        if first == second {
            return;
        }
        let (large, small) = match self.size[first] >= self.size[second] {
            true => (first, second),
            false => (second, first),
        };
        self.parent[small] = large;
        self.size[large] += self.size[small];
    }
}

/// The group of each candidate: candidates that share a custody or a grant
/// are connected, and each group is named by its smallest position.
fn group_positions(candidates: &[BatchCandidate]) -> Vec<usize> {
    let mut partition = Partition::new(candidates.len());
    let mut custody_owner: BTreeMap<[u8; 32], usize> = BTreeMap::new();
    let mut grant_owner: BTreeMap<&[u8], usize> = BTreeMap::new();
    for (position, candidate) in candidates.iter().enumerate() {
        for custody in &candidate.custodies {
            match custody_owner.get(custody) {
                Some(owner) => partition.union(*owner, position),
                None => {
                    custody_owner.insert(*custody, position);
                }
            }
        }
        for draw in &candidate.grants {
            match grant_owner.get(draw.grant.as_slice()) {
                Some(owner) => partition.union(*owner, position),
                None => {
                    grant_owner.insert(draw.grant.as_slice(), position);
                }
            }
        }
    }
    let mut smallest: BTreeMap<usize, usize> = BTreeMap::new();
    let mut roots = Vec::with_capacity(candidates.len());
    for position in 0..candidates.len() {
        let root = partition.find(position);
        smallest.entry(root).or_insert(position);
        roots.push(root);
    }
    roots.into_iter().map(|root| smallest[&root]).collect()
}

/// Why `members` (one group) cannot all be funded at R0, if they cannot.
fn check_group(
    candidates: &[BatchCandidate],
    members: &[usize],
) -> Result<Option<GroupRejection>, FundingGroupError> {
    let mut holds: BTreeMap<[u8; 32], u128> = BTreeMap::new();
    let mut supply: BTreeMap<[u8; 32], u128> = BTreeMap::new();
    let mut grants: BTreeMap<&[u8], (usize, u128, u128)> = BTreeMap::new();
    for member in members {
        let candidate = &candidates[*member];
        for (purse, hold) in &candidate.holds {
            let total = holds.entry(*purse).or_insert(0);
            match total.checked_add(*hold) {
                Some(sum) => *total = sum,
                None => return Ok(Some(GroupRejection::Overflow)),
            }
        }
        for (purse, amount) in &candidate.supply {
            match supply.insert(*purse, *amount) {
                Some(previous) if previous != *amount => {
                    return Err(FundingGroupError::SupplyDisagrees { purse: *purse });
                }
                _ => {}
            }
        }
        for draw in &candidate.grants {
            let entry = grants
                .entry(draw.grant.as_slice())
                .or_insert((0, 0, u128::MAX));
            entry.0 += 1;
            match entry.1.checked_add(draw.hold) {
                Some(sum) => entry.1 = sum,
                None => return Ok(Some(GroupRejection::Overflow)),
            }
            entry.2 = entry.2.min(draw.remaining);
        }
    }
    for (purse, total) in &holds {
        let available = supply.get(purse).copied().unwrap_or(0);
        if *total > available {
            return Ok(Some(GroupRejection::Purse {
                purse: *purse,
                holds: *total,
                supply: available,
            }));
        }
    }
    // O-8: only a grant that two or more members draw is checked here. The
    // settlement of a single draw checks its own remaining ceiling.
    for (grant, (draws, total, remaining)) in grants {
        if draws >= 2 && total > remaining {
            return Ok(Some(GroupRejection::Grant {
                grant: grant.to_vec(),
                holds: total,
                remaining,
            }));
        }
    }
    Ok(None)
}

/// The decision over a batch of candidates that each passed alone (DR-64):
/// group them, reject each group whose holds do not fit (all or none, DR-65
/// item 4), and accept the members of every other group in batch order.
pub(crate) fn decide_groups(
    candidates: &[BatchCandidate],
) -> Result<GroupDecision, FundingGroupError> {
    let group_of = group_positions(candidates);
    let mut members_of: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (position, group) in group_of.iter().enumerate() {
        members_of.entry(*group).or_default().push(position);
    }
    let mut rejected_positions = BTreeSet::new();
    let mut rejected = Vec::new();
    for members in members_of.values() {
        if let Some(cause) = check_group(candidates, members)? {
            rejected_positions.extend(members.iter().copied());
            rejected.push(RejectedGroup {
                members: members.clone(),
                cause,
            });
        }
    }
    let accepted = (0..candidates.len())
        .filter(|position| !rejected_positions.contains(position))
        .collect();
    Ok(GroupDecision {
        group_of,
        accepted,
        rejected,
    })
}

/// The later holds of each of `members` (candidate positions in execution
/// order): for member i, the sum on each purse of the holds of the members
/// after it, with zero sums left out. The suffix sums are built from the last
/// member back, in one pass.
pub(crate) fn later_holds(
    candidates: &[BatchCandidate],
    members: &[usize],
) -> Result<Vec<BTreeMap<[u8; 32], u128>>, FundingGroupError> {
    if members.iter().any(|member| *member >= candidates.len()) {
        return Err(FundingGroupError::MemberOutOfRange);
    }
    let mut later = vec![BTreeMap::new(); members.len()];
    let mut running: BTreeMap<[u8; 32], u128> = BTreeMap::new();
    for (index, member) in members.iter().enumerate().rev() {
        later[index] = running.clone();
        for (purse, hold) in &candidates[*member].holds {
            if *hold == 0 {
                continue;
            }
            let total = running.entry(*purse).or_insert(0);
            *total = total
                .checked_add(*hold)
                .ok_or(FundingGroupError::LaterOverflow)?;
        }
    }
    Ok(later)
}

#[cfg(test)]
mod tests;
