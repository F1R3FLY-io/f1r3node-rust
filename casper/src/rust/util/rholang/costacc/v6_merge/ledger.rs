//! DR-120: the number-channel ledger of the v6 rule (C3, C4, LOW-4, HIGH-1).
//!
//! The merge folds IntegerAdd diffs with checked addition in orders that the
//! ledger does not control. If the positive diffs of a list sum to at most
//! `i64::MAX` and its negative diffs sum to at least `i64::MIN`, then every
//! partial sum of the list, in every order, lies between the two sums, so no
//! fold of the list overflows. The ledger keeps these two sums in `i128`, in
//! which they cannot overflow.

use std::collections::BTreeMap;

use rspace_plus_plus::rspace::errors::HistoryError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
use shared::rust::hashable_set::HashableSet;

use crate::rust::merging::deploy_chain_index::DeployChainIndex;

const MAX: i128 = i64::MAX as i128;
const MIN: i128 = i64::MIN as i128;

/// Why a final balance is invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The balance is below zero.
    Negative,
    /// The balance is above `i64::MAX`.
    Overflow,
}

/// The sum of the positive diffs and the sum of the negative diffs of a list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SignSplit {
    pub pos: i128,
    pub neg: i128,
}

impl SignSplit {
    fn add(&mut self, diff: i64) {
        match diff > 0 {
            true => self.pos += i128::from(diff),
            false => self.neg += i128::from(diff),
        }
    }

    fn plus(self, other: SignSplit) -> SignSplit {
        SignSplit {
            pos: self.pos + other.pos,
            neg: self.neg + other.neg,
        }
    }

    /// Every partial sum of the list fits `i64`.
    pub(crate) fn fits(self) -> bool { self.pos <= MAX && self.neg >= MIN }

    pub(crate) fn total(self) -> i128 { self.pos + self.neg }
}

#[derive(Clone, Copy, Debug)]
struct ChannelSums {
    merge_type: MergeType,
    split: SignSplit,
}

fn same_type<'a>(
    types: &mut BTreeMap<&'a Blake2b256Hash, MergeType>,
    channel: &'a Blake2b256Hash,
    merge_type: MergeType,
) -> bool {
    *types.entry(channel).or_insert(merge_type) == merge_type
}

/// C4 and LOW-4: the pre-check of one branch. `compute_branch_derived` folds
/// the user parts and the system parts of the branch's chains separately and
/// then adds the two totals (dag_merger.rs:1408-1438). The availability walk
/// folds the chains' combined parts (dag_merger.rs:122-163). Each fold stays
/// inside `i64` when the user split fits, the system split fits and the two
/// splits together fit, because a combined part is at most the sum of its
/// positive halves and at least the sum of its negative halves. One merge
/// type per channel avoids the mismatch error of those folds.
pub(crate) fn branch_valid(branch: &HashableSet<DeployChainIndex>) -> bool {
    let mut types: BTreeMap<&Blake2b256Hash, MergeType> = BTreeMap::new();
    let mut user: BTreeMap<&Blake2b256Hash, SignSplit> = BTreeMap::new();
    let mut system: BTreeMap<&Blake2b256Hash, SignSplit> = BTreeMap::new();
    for chain in branch.0.iter() {
        for (channel, &(diff, merge_type)) in chain.user_event_log_index.number_channels_data.iter()
        {
            if !same_type(&mut types, channel, merge_type) {
                return false;
            }
            user.entry(channel).or_default().add(diff);
        }
        for (channel, &(diff, merge_type)) in
            chain.system_event_log_index.number_channels_data.iter()
        {
            if !same_type(&mut types, channel, merge_type) {
                return false;
            }
            system.entry(channel).or_default().add(diff);
        }
        for (channel, &(_, merge_type)) in chain.event_log_index.number_channels_data.iter() {
            if !same_type(&mut types, channel, merge_type) {
                return false;
            }
        }
    }
    types.iter().all(|(channel, merge_type)| match merge_type {
        MergeType::BitmaskOr => true,
        MergeType::IntegerAdd => {
            let user_split = user.get(channel).copied().unwrap_or_default();
            let system_split = system.get(channel).copied().unwrap_or_default();
            user_split.fits() && system_split.fits() && user_split.plus(system_split).fits()
        }
    })
}

/// The number channels of a kept set: per channel, the merge type and the
/// sign split of the kept chains' combined parts, which `compute_merged_state`
/// folds (conflict_set_merger.rs:547-579). A kept chain whose mergeable map
/// holds a channel, with any diff including zero, is a contributor to it.
#[derive(Clone, Debug, Default)]
pub(crate) struct PurseLedger {
    channels: BTreeMap<Blake2b256Hash, ChannelSums>,
}

impl PurseLedger {
    /// The ledger of a set that already passed `try_add`. A subset of such a
    /// set keeps one merge type per channel, and removal only moves both sums
    /// toward zero, so the subset's splits fit too.
    pub(crate) fn from_branches<'a>(
        branches: impl IntoIterator<Item = &'a HashableSet<DeployChainIndex>>,
    ) -> Self {
        let mut ledger = PurseLedger::default();
        for branch in branches {
            for chain in branch.0.iter() {
                for (channel, &(diff, merge_type)) in
                    chain.event_log_index.number_channels_data.iter()
                {
                    ledger
                        .channels
                        .entry(channel.clone())
                        .or_insert(ChannelSums {
                            merge_type,
                            split: SignSplit::default(),
                        })
                        .split
                        .add(diff);
                }
            }
        }
        ledger
    }

    /// Adds `branch` when every channel keeps one merge type and every
    /// IntegerAdd split of the kept set still fits (LOW-4). Both checks are
    /// monotone: a set that fails keeps failing when chains are added.
    pub(crate) fn try_add(&mut self, branch: &HashableSet<DeployChainIndex>) -> bool {
        let mut delta: BTreeMap<&Blake2b256Hash, ChannelSums> = BTreeMap::new();
        for chain in branch.0.iter() {
            for (channel, &(diff, merge_type)) in chain.event_log_index.number_channels_data.iter()
            {
                let sums = delta.entry(channel).or_insert(ChannelSums {
                    merge_type,
                    split: SignSplit::default(),
                });
                if sums.merge_type != merge_type {
                    return false;
                }
                sums.split.add(diff);
            }
        }
        for (channel, sums) in &delta {
            let after = match self.channels.get(*channel) {
                Some(kept) if kept.merge_type != sums.merge_type => return false,
                Some(kept) => kept.split.plus(sums.split),
                None => sums.split,
            };
            if let (MergeType::IntegerAdd, false) = (sums.merge_type, after.fits()) {
                return false;
            }
        }
        for (channel, sums) in delta {
            self.channels
                .entry(channel.clone())
                .and_modify(|kept| kept.split = kept.split.plus(sums.split))
                .or_insert(sums);
        }
        true
    }

    /// HIGH-1: the first channel, in channel order, whose final balance
    /// leaves `[0, i64::MAX]`. Only channels with a contributor enter the
    /// ledger, so a channel without contributors cannot fail. Like dev's
    /// resolver (conflict_set_merger.rs:236-239), it reads the base value of
    /// every channel before it judges any, so a read error always surfaces.
    pub(crate) fn first_failure(
        &self,
        base_value: &mut dyn FnMut(&Blake2b256Hash) -> Result<i64, HistoryError>,
    ) -> Result<Option<(Blake2b256Hash, Failure)>, HistoryError> {
        let mut bases = Vec::with_capacity(self.channels.len());
        for channel in self.channels.keys() {
            bases.push(base_value(channel)?);
        }
        for ((channel, sums), base) in self.channels.iter().zip(bases) {
            if let MergeType::IntegerAdd = sums.merge_type {
                let end = i128::from(base) + sums.split.total();
                if end < 0 {
                    return Ok(Some((channel.clone(), Failure::Negative)));
                }
                if end > MAX {
                    return Ok(Some((channel.clone(), Failure::Overflow)));
                }
            }
        }
        Ok(None)
    }
}
