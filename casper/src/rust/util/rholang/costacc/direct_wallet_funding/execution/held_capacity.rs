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
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// The path that evaluates a held-capacity guard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GuardRole {
    Play,
    Replay,
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

impl RuntimeManager {
    /// Added by G1-7: the SystemVault balance at `root` of each purse that
    /// the debits charge. A purse with a zero debit passes the guard with any
    /// balance, so the reader skips it. Play and replay call this with the
    /// same root, the root after the run.
    pub(super) async fn purse_balances_at(
        &self,
        snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
        root: [u8; 32],
        debits: &BTreeMap<[u8; 32], u128>,
        event_codec_epoch: rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch,
    ) -> Result<BTreeMap<[u8; 32], u128>, CasperError> {
        let reader = RuntimeManagerSupplyReader {
            runtime_manager: self,
            pre_state_hash: prost::bytes::Bytes::copy_from_slice(&root),
            event_codec_epoch,
        };
        let payers = snapshot.wallets().authorization().payers();
        let mut balances = BTreeMap::new();
        for (custody, debit) in debits {
            if *debit == 0 {
                continue;
            }
            let payer = payers
                .get(custody)
                .ok_or_else(|| invalid("a debited purse has no authorized payer"))?;
            let inventory = reader.read_purse(&payer.signature).await?;
            let balance = inventory
                .balance
                .ok_or_else(|| invalid("the balance of a debited purse is absent"))?;
            balances.insert(
                *custody,
                u128::try_from(balance)
                    .map_err(|_| invalid("the balance of a debited purse is negative"))?,
            );
        }
        Ok(balances)
    }

    /// Added by G1-7: the held-capacity guard of one offered member with no
    /// later holds (G1). Under the feature `test-utils`, the
    /// `ForceGuardFailureHook` of this node makes the guard fail, in play
    /// and in replay. The node records each outcome for the tests.
    pub(super) fn held_capacity_guard_outcome(
        &self,
        member: GuardMember<'_>,
        balances: &BTreeMap<[u8; 32], u128>,
        debits: &BTreeMap<[u8; 32], u128>,
    ) -> Result<(), Vec<[u8; 32]>> {
        let outcome = held_capacity_guard(balances, debits, &BTreeMap::new());
        #[cfg(any(test, feature = "test-utils"))]
        let outcome = match self.offered_guard_hook(member.deploy_id) {
            true => Err(debits.keys().copied().collect()),
            false => outcome,
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

    /// Added by G1-7, test only: a hook of this node. The held-capacity guard
    /// of the deploy fails on this node, in play and in replay, as a producer
    /// that claims a guard failure would make it. Other nodes recompute the
    /// guard.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn force_held_capacity_guard_failure_for_tests(&self, deploy_id: &[u8]) {
        self.record_offered_budget_usage(OfferedUsageKind::ForceGuardFailureHook, deploy_id, &[]);
    }
}
