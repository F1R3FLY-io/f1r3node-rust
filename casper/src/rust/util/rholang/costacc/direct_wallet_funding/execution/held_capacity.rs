//! G1-7 (DR-65 item 5, design §8.3): the inputs of the held-capacity guard
//! of one offered member. Play and replay read them the same way: the debit
//! of each custody from the settlement capture of the success path, and the
//! SystemVault balance of each debited purse at the settlement root, where
//! the run has changed it. The pure guard is `gate::held_capacity_guard`.

// Moved by dedup (task 11415) with the body of `capture_debits` to
// `custody_debits` in `settlement.rs`, which uses this import now.
// use std::mem::size_of;
use std::collections::BTreeMap;

// Moved by dedup (task 11415) with the body of `capture_debits` to
// `custody_debits` in `settlement.rs`, which uses this import now.
// use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use rholang::rust::interpreter::accounting::phlo_execution::NativePhloSourceAmounts;

use super::gate::held_capacity_guard;
// Changed by dedup (task 11415): the shared debit map of `settlement.rs`.
// use super::settlement::invalid;
use super::settlement::{custody_debits, invalid, CustodyDebitError};
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::direct_wallet_funding::DirectWalletPolicySnapshot;
#[cfg(any(test, feature = "test-utils"))]
use crate::rust::util::rholang::costacc::offered_acceptance::{OfferedCheckRole, OfferedUsageKind};
use crate::rust::util::rholang::costacc::supply_reader::{
    RuntimeManagerSupplyReader, SupplyReader,
};
// Added by DR-134 (DR-65 item 5): the payer of a purse with a later hold.
use crate::rust::util::rholang::costacc::vault_payer::VaultPayer;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// The path that evaluates a held-capacity guard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GuardRole {
    Play,
    Replay,
}

/// Added by DR-134 (DR-65 item 5, design §8.3): the later holds of one member
/// of a batch. For each purse, the hold is the sum of the holds at R0 that the
/// later members of the batch place on it. The payer of each held purse lets
/// the guard read the purse balance at the settlement root, because a purse
/// with a later hold may be outside the member's own sources. A member with no
/// later member, such as the only member of a batch, has no later holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LaterHolds {
    holds: BTreeMap<[u8; 32], u128>,
    payers: BTreeMap<[u8; 32], VaultPayer>,
}

/// Added by DR-134 (DR-65 item 5): why a set of later holds is malformed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum LaterHoldsError {
    #[error("a later hold has no authorized payer")]
    MissingPayer,
}

impl LaterHolds {
    /// The later holds with a positive amount, each with its payer. A zero
    /// hold constrains nothing, so it is dropped.
    pub(crate) fn new(
        holds: BTreeMap<[u8; 32], u128>,
        payers: &BTreeMap<[u8; 32], VaultPayer>,
    ) -> Result<Self, LaterHoldsError> {
        let mut positive = BTreeMap::new();
        let mut held_payers = BTreeMap::new();
        for (purse, hold) in holds {
            if hold == 0 {
                continue;
            }
            let payer = payers.get(&purse).ok_or(LaterHoldsError::MissingPayer)?;
            held_payers.insert(purse, payer.clone());
            positive.insert(purse, hold);
        }
        Ok(Self {
            holds: positive,
            payers: held_payers,
        })
    }

    /// The positive later hold of each purse, in ascending purse order.
    pub(crate) fn holds(&self) -> &BTreeMap<[u8; 32], u128> { &self.holds }

    pub(crate) fn is_empty(&self) -> bool { self.holds.is_empty() }
}

/// The member whose guard is evaluated: its deploy, its funding root and the
/// path. The deploy and the root key the test records and hooks.
#[derive(Clone, Copy, Debug)]
pub(super) struct GuardMember<'a> {
    pub(super) deploy_id: &'a [u8],
    pub(super) funding_root: &'a [u8],
    pub(super) role: GuardRole,
}

/// The debit of each custody in a settlement capture: the acquisition plus
/// the fee, as settlement charges them (`settlement.rs`, grant debits).
pub(super) fn capture_debits(
    amounts: &[NativePhloSourceAmounts<'_>],
) -> Result<BTreeMap<[u8; 32], u128>, CasperError> {
    // Moved by dedup (task 11415) to `custody_debits` in
    // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/settlement.rs`.
    // let size = amounts
    //     .len()
    //     .checked_mul(size_of::<([u8; 32], u128)>())
    //     .ok_or_else(|| invalid("guard debit map size overflows"))?;
    // budget
    //     .reserve(
    //         HostWorkDimension::SearchStateBytes,
    //         HostWorkUnits::new(
    //             u64::try_from(size).map_err(|_| invalid("guard debit map size overflows"))?,
    //         ),
    //     )
    //     .map_err(invalid)?;
    // let mut debits = BTreeMap::new();
    // for amount in amounts {
    //     let custody: [u8; 32] = amount
    //         .custody()
    //         .try_into()
    //         .map_err(|_| invalid("guard debit custody is not 32 bytes"))?;
    //     let acquisition = u128::try_from(amount.acquisition())
    //         .map_err(|_| invalid("negative resource charge"))?;
    //     let fee = u128::try_from(amount.fee()).map_err(|_| invalid("negative fee"))?;
    //     let debit = acquisition
    //         .checked_add(fee)
    //         .ok_or_else(|| invalid("guard debit overflows"))?;
    //     if debits.insert(custody, debit).is_some() {
    //         return Err(invalid("duplicate guard debit custody"));
    //     }
    // }
    // Ok(debits)
    custody_debits(amounts).map_err(|error| match error {
        CustodyDebitError::Width => invalid("guard debit custody is not 32 bytes"),
        CustodyDebitError::NegativeCharge => invalid("negative resource charge"),
        CustodyDebitError::NegativeFee => invalid("negative fee"),
        CustodyDebitError::Overflow => invalid("guard debit overflows"),
        CustodyDebitError::Duplicate => invalid("duplicate guard debit custody"),
    })
}

/// Added by DR-134 (DR-65 item 5, design §8.3): the purses that the guard of a
/// member checks, in ascending order: each purse with a positive debit or a
/// positive later hold. The flag tells whether the member debits the purse. A
/// purse with neither constrains nothing, so it is left out. The walk merges
/// the two sorted key sets in one pass, with no recursion.
pub(super) fn guarded_purses(
    debits: &BTreeMap<[u8; 32], u128>,
    later: &BTreeMap<[u8; 32], u128>,
) -> Vec<([u8; 32], bool)> {
    let mut purses = Vec::with_capacity(debits.len() + later.len());
    let mut debit_entries = debits.iter().filter(|(_, debit)| **debit > 0).peekable();
    let mut later_entries = later.iter().filter(|(_, hold)| **hold > 0).peekable();
    loop {
        let entry = match (debit_entries.peek(), later_entries.peek()) {
            (None, None) => break,
            (Some((purse, _)), None) => {
                let entry = (**purse, true);
                debit_entries.next();
                entry
            }
            (None, Some((purse, _))) => {
                let entry = (**purse, false);
                later_entries.next();
                entry
            }
            (Some((debit_purse, _)), Some((later_purse, _))) => {
                match debit_purse.cmp(later_purse) {
                    std::cmp::Ordering::Less => {
                        let entry = (**debit_purse, true);
                        debit_entries.next();
                        entry
                    }
                    std::cmp::Ordering::Greater => {
                        let entry = (**later_purse, false);
                        later_entries.next();
                        entry
                    }
                    std::cmp::Ordering::Equal => {
                        let entry = (**debit_purse, true);
                        debit_entries.next();
                        later_entries.next();
                        entry
                    }
                }
            }
        };
        purses.push(entry);
    }
    purses
}

impl RuntimeManager {
    /// Added by G1-7: the SystemVault balance at `root` of each purse that
    /// the debits charge. A purse with a zero debit passes the guard with any
    /// balance, so the reader skips it. Play and replay call this with the
    /// same root, the root after the run.
    // Changed by DR-134 (DR-65 item 5, design §8.3): the reader also reads each
    // purse that carries a later hold, because the guard checks it. The walk
    // merges the debited purses and the held purses in one ascending pass, as
    // `held_capacity_guard` does, with no recursion. A debited purse with an
    // absent balance stays an error. A purse with only a later hold and an
    // absent balance has balance 0, so the guard fails it.
    // pub(super) async fn purse_balances_at(
    //     &self,
    //     snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
    //     root: [u8; 32],
    //     debits: &BTreeMap<[u8; 32], u128>,
    // ) -> Result<BTreeMap<[u8; 32], u128>, CasperError> {
    //     let reader = RuntimeManagerSupplyReader {
    //         runtime_manager: self,
    //         pre_state_hash: prost::bytes::Bytes::copy_from_slice(&root),
    //     };
    //     let payers = snapshot.wallets().authorization().payers();
    //     let mut balances = BTreeMap::new();
    //     for (custody, debit) in debits {
    //         if *debit == 0 {
    //             continue;
    //         }
    //         let payer = payers
    //             .get(custody)
    //             .ok_or_else(|| invalid("a debited purse has no authorized payer"))?;
    //         let inventory = reader.read_purse(&payer.signature).await?;
    //         let balance = inventory
    //             .balance
    //             .ok_or_else(|| invalid("the balance of a debited purse is absent"))?;
    //         balances.insert(
    //             *custody,
    //             u128::try_from(balance)
    //                 .map_err(|_| invalid("the balance of a debited purse is negative"))?,
    //         );
    //     }
    //     Ok(balances)
    // }
    // Changed by G3 (DR-135, S7): the payer of a purse may also be an owner
    // payer, and the reads go through `read_balances_at`, which the owner
    // discovery and the owner table check share (R3). The replaced function
    // follows.
    // pub(super) async fn purse_balances_at(
    //     &self,
    //     snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
    //     root: [u8; 32],
    //     debits: &BTreeMap<[u8; 32], u128>,
    //     later: &LaterHolds,
    // ) -> Result<BTreeMap<[u8; 32], u128>, CasperError> {
    //     let reader = RuntimeManagerSupplyReader {
    //         runtime_manager: self,
    //         pre_state_hash: prost::bytes::Bytes::copy_from_slice(&root),
    //     };
    //     let payers = snapshot.wallets().authorization().payers();
    //     let mut balances = BTreeMap::new();
    //     for (purse, debited) in guarded_purses(debits, later.holds()) {
    //         let payer = match (payers.get(&purse), later.payers.get(&purse), debited) {
    //             (Some(payer), _, _) | (None, Some(payer), _) => payer,
    //             (None, None, true) => {
    //                 return Err(invalid("a debited purse has no authorized payer"));
    //             }
    //             (None, None, false) => {
    //                 return Err(invalid("a held purse has no authorized payer"));
    //             }
    //         };
    //         let inventory = reader.read_purse(&payer.signature).await?;
    //         let balance = match (inventory.balance, debited) {
    //             (Some(balance), _) => u128::try_from(balance)
    //                 .map_err(|_| invalid("the balance of a guarded purse is negative"))?,
    //             (None, true) => {
    //                 return Err(invalid("the balance of a debited purse is absent"));
    //             }
    //             (None, false) => 0,
    //         };
    //         balances.insert(purse, balance);
    //     }
    //     Ok(balances)
    // }
    pub(super) async fn purse_balances_at(
        &self,
        snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
        root: [u8; 32],
        debits: &BTreeMap<[u8; 32], u128>,
        later: &LaterHolds,
        owners: &BTreeMap<[u8; 32], VaultPayer>,
    ) -> Result<BTreeMap<[u8; 32], u128>, CasperError> {
        let payers = snapshot.wallets().authorization().payers();
        let guarded = guarded_purses(debits, later.holds());
        let mut located = Vec::with_capacity(guarded.len());
        for (purse, debited) in &guarded {
            let payer = match (
                payers.get(purse),
                later.payers.get(purse),
                owners.get(purse),
                *debited,
            ) {
                (Some(payer), _, _, _)
                | (None, Some(payer), _, _)
                | (None, None, Some(payer), _) => payer,
                (None, None, None, true) => {
                    return Err(invalid("a debited purse has no authorized payer"));
                }
                (None, None, None, false) => {
                    return Err(invalid("a held purse has no authorized payer"));
                }
            };
            located.push(payer);
        }
        let read = self.read_balances_at(root, &located).await?;
        let mut balances = BTreeMap::new();
        for ((purse, debited), balance) in guarded.into_iter().zip(read) {
            let balance = match (balance, debited) {
                (Some(balance), _) => u128::try_from(balance)
                    .map_err(|_| invalid("the balance of a guarded purse is negative"))?,
                (None, true) => {
                    return Err(invalid("the balance of a debited purse is absent"));
                }
                (None, false) => 0,
            };
            balances.insert(purse, balance);
        }
        Ok(balances)
    }

    /// Added by G3 (DR-135, S7): the SystemVault balance at `root` of the
    /// vault of each payer in `payers`, in order, or `None` for an absent
    /// vault. The held-capacity guard, the owner discovery of play and the
    /// owner table check of replay read balances with it (R3).
    pub(super) async fn read_balances_at(
        &self,
        root: [u8; 32],
        payers: &[&VaultPayer],
    ) -> Result<Vec<Option<i64>>, CasperError> {
        let reader = RuntimeManagerSupplyReader {
            runtime_manager: self,
            pre_state_hash: prost::bytes::Bytes::copy_from_slice(&root),
        };
        let mut balances = Vec::with_capacity(payers.len());
        for payer in payers {
            balances.push(reader.read_purse(&payer.signature).await?.balance);
        }
        Ok(balances)
    }

    /// Added by G1-7: the held-capacity guard of one offered member with no
    /// later holds (G1). Under the feature `test-utils`, the
    /// `ForceGuardFailureHook` of this node makes the guard fail, in play
    /// and in replay. The node records each outcome for the tests.
    // Changed by DR-134 (DR-65 item 5, design §8.3): the guard takes the later
    // holds of the member. The only member of a batch passes none.
    // pub(super) fn held_capacity_guard_outcome(
    //     &self,
    //     member: GuardMember<'_>,
    //     balances: &BTreeMap<[u8; 32], u128>,
    //     debits: &BTreeMap<[u8; 32], u128>,
    // ) -> Result<(), Vec<[u8; 32]>> {
    //     let outcome = held_capacity_guard(balances, debits, &BTreeMap::new());
    pub(super) fn held_capacity_guard_outcome(
        &self,
        member: GuardMember<'_>,
        balances: &BTreeMap<[u8; 32], u128>,
        debits: &BTreeMap<[u8; 32], u128>,
        later: &LaterHolds,
    ) -> Result<(), Vec<[u8; 32]>> {
        let outcome = held_capacity_guard(balances, debits, later.holds());
        // Changed by DR-134 (DR-65 O-13): the test hook that ignores the later
        // holds of the deploy on this node.
        // #[cfg(any(test, feature = "test-utils"))]
        // let outcome = match self.offered_guard_hook(member.deploy_id) {
        //     true => Err(debits.keys().copied().collect()),
        //     false => outcome,
        // };
        #[cfg(any(test, feature = "test-utils"))]
        let outcome = match (
            self.offered_guard_hook(member.deploy_id),
            self.offered_later_holds_hook(member.deploy_id),
        ) {
            (true, _) => Err(debits.keys().copied().collect()),
            (false, true) => held_capacity_guard(balances, debits, &BTreeMap::new()),
            (false, false) => outcome,
        };
        #[cfg(any(test, feature = "test-utils"))]
        self.record_offered_budget_usage(
            OfferedUsageKind::HeldCapacityGuard {
                role: match member.role {
                    GuardRole::Play => OfferedCheckRole::Play,
                    GuardRole::Replay => OfferedCheckRole::Replay,
                },
                holds: outcome.is_ok(),
            },
            member.deploy_id,
            member.funding_root,
        );
        tracing::debug!(
            target: "f1r3fly.cost_accounting.guard",
            deploy = %hex::encode(member.deploy_id),
            funding_root = %hex::encode(member.funding_root),
            role = ?member.role,
            holds = outcome.is_ok(),
            "held-capacity guard"
        );
        outcome
    }

    /// Added by G1-7, test only: whether this node has the guard hook for the
    /// deploy.
    #[cfg(any(test, feature = "test-utils"))]
    fn offered_guard_hook(&self, deploy_id: &[u8]) -> bool {
        self.offered_budget_usages().iter().any(|record| {
            matches!(record.kind, OfferedUsageKind::ForceGuardFailureHook)
                && record.deploy_id == deploy_id
        })
    }

    /// Added by DR-134 (DR-65 O-13), test only: whether this node ignores the
    /// later holds of the deploy.
    #[cfg(any(test, feature = "test-utils"))]
    fn offered_later_holds_hook(&self, deploy_id: &[u8]) -> bool {
        self.offered_budget_usages().iter().any(|record| {
            matches!(record.kind, OfferedUsageKind::IgnoreLaterHoldsHook)
                && record.deploy_id == deploy_id
        })
    }

    /// Added by DR-134 (DR-65 O-13), test only: a hook of this node. The guard
    /// of the deploy ignores its later holds on this node, in play and in
    /// replay, as a producer that ignores them would. Other nodes count them.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn ignore_later_holds_for_tests(&self, deploy_id: &[u8]) {
        self.record_offered_budget_usage(OfferedUsageKind::IgnoreLaterHoldsHook, deploy_id, &[]);
    }

    /// Added by G1-7, test only: a hook of this node. The held-capacity guard
    /// of the deploy fails on this node, in play and in replay, as a producer
    /// that claims a guard failure would make it. Other nodes recompute the
    /// guard.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn force_held_capacity_guard_failure_for_tests(&self, deploy_id: &[u8]) {
        self.record_offered_budget_usage(OfferedUsageKind::ForceGuardFailureHook, deploy_id, &[]);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::guarded_purses;
    use crate::rust::util::rholang::costacc::direct_wallet_funding::capped_capacity;

    fn purse_map() -> impl Strategy<Value = BTreeMap<[u8; 32], u128>> {
        proptest::collection::btree_map(any::<u8>().prop_map(|byte| [byte; 32]), 0u128..4, 0..8)
    }

    proptest! {
        // Added by DR-134 (DR-65 item 5): the guard reads exactly the purses
        // with a positive debit or a positive later hold, once each, in
        // ascending order, and flags the debited ones.
        #[test]
        fn guarded_purses_are_the_positive_debits_and_later_holds(
            debits in purse_map(),
            later in purse_map(),
        ) {
            let purses = guarded_purses(&debits, &later);
            prop_assert!(purses.windows(2).all(|pair| pair[0].0 < pair[1].0));
            let expected: Vec<([u8; 32], bool)> = debits
                .keys()
                .chain(later.keys())
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .filter_map(|purse| {
                    let debit = debits.get(&purse).copied().unwrap_or(0);
                    let hold = later.get(&purse).copied().unwrap_or(0);
                    match (debit > 0, hold > 0) {
                        (false, false) => None,
                        (debited, _) => Some((purse, debited)),
                    }
                })
                .collect();
            prop_assert_eq!(purses, expected);
        }

        // Added by DR-134 (DR-65 item 5, option B): the capped capacity never
        // exceeds the balance, never goes below zero, and leaves room for the
        // later hold whenever the balance covers it.
        #[test]
        fn capped_capacity_leaves_room_for_the_later_holds(
            capacity in any::<u64>(),
            later in any::<u128>(),
        ) {
            let capped = capped_capacity(capacity, later);
            prop_assert!(capped <= capacity);
            match u64::try_from(later) {
                Ok(later) if later <= capacity => prop_assert_eq!(capped, capacity - later),
                _ => prop_assert_eq!(capped, 0),
            }
        }
    }
}
