use futures::{stream, StreamExt, TryStreamExt};
use rholang::rust::interpreter::accounting::phlo_execution::PhloFundingSource;

use super::*;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::supply::PurseInventory;
use crate::rust::util::rholang::costacc::supply_reader::SupplyReader;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectWalletSource<'a> {
    source: PhloFundingSource<'a>,
    inventory: PurseInventory,
}

impl<'a> DirectWalletSource<'a> {
    pub fn source(&self) -> PhloFundingSource<'a> { self.source }
    pub fn inventory(&self) -> &PurseInventory { &self.inventory }
}

#[derive(Debug)]
pub struct DirectWalletSnapshot<'a, A = FundedDeploy> {
    authorization: DirectWalletFunding<'a, A>,
    pre_state_root: [u8; 32],
    sources: Vec<DirectWalletSource<'a>>,
}

impl<'a, A> DirectWalletSnapshot<'a, A> {
    pub fn authorization(&self) -> &DirectWalletFunding<'a, A> { &self.authorization }
    pub fn pre_state_root(&self) -> [u8; 32] { self.pre_state_root }
    pub fn sources(&self) -> &[DirectWalletSource<'a>] { &self.sources }

    /// Added by DR-134 (DR-65 item 5, design §8.4, option B): caps the
    /// capacity of each source by the later holds on its custody. The
    /// settlement selection of a member then leaves room for every later
    /// member of the batch. Only the capacity changes, so the funding-family
    /// binding and the funding-case encoding read the capped value in play and
    /// in replay alike.
    pub(crate) fn cap_by_later_holds(&mut self, later: &BTreeMap<[u8; 32], u128>) {
        for source in &mut self.sources {
            if let Ok(custody) = <[u8; 32]>::try_from(source.source.custody) {
                let hold = later.get(&custody).copied().unwrap_or(0);
                source.source.capacity = capped_capacity(source.source.capacity, hold);
            }
        }
    }
}

/// Added by DR-134 (DR-65 item 5, design §8.4, option B): the capacity of a
/// source for the settlement selection of a member: the balance at the
/// member's execution root minus the later holds on the source, never below
/// zero. A later hold above the u64 range leaves no capacity.
pub(crate) fn capped_capacity(capacity: u64, later: u128) -> u64 {
    match u64::try_from(later) {
        Ok(later) => capacity.saturating_sub(later),
        Err(_) => 0,
    }
}

/// Added by G3 (DR-135): the budget of an owner purse for a batch member
/// (R3): the capped capacity of the purse at the member's execution root,
/// less the member's own hold on the purse, never below zero. The later
/// holds stay covered, and the member's own client debits fit its hold, so
/// owner and client debits together stay within the capped capacity.
pub(crate) fn owner_budget(balance: u64, later: u128, own_hold: u128) -> u64 {
    u64::try_from(u128::from(capped_capacity(balance, later)).saturating_sub(own_hold))
        .expect("bounded by the capped balance")
}

#[derive(Debug, Error)]
pub enum DirectWalletSnapshotError {
    #[error("direct-wallet snapshot reader has a different pre-state root")]
    WrongPreState,
    #[error("delegated funding proof has a different pre-state root")]
    WrongGrantRoot,
    #[error("direct-wallet snapshot has a negative native balance")]
    NegativeBalance,
    #[error("direct-wallet snapshot is missing an authorized source")]
    MissingAuthorizedSource,
    #[error(transparent)]
    Read(#[from] CasperError),
}

impl<'a, A> DirectWalletFunding<'a, A> {
    pub async fn read_snapshot(
        self,
        reader: &dyn SupplyReader,
        expected_pre_state: [u8; 32],
        parallel_reads: NonZeroUsize,
    ) -> Result<DirectWalletSnapshot<'a, A>, DirectWalletSnapshotError> {
        if reader.pre_state_root() != expected_pre_state {
            return Err(DirectWalletSnapshotError::WrongPreState);
        }
        if self
            .grant_root
            .is_some_and(|root| root != expected_pre_state)
        {
            return Err(DirectWalletSnapshotError::WrongGrantRoot);
        }
        let concurrency = parallel_reads.get().min(self.payers.len()).max(1);
        let payers = &self.payers;
        let policies = &self.record().sources;
        let sources = stream::iter((0..policies.len()).map(|index| async move {
            let policy = &policies[index];
            let custody = policy.custody();
            let key: [u8; 32] = custody
                .try_into()
                .map_err(|_| DirectWalletSnapshotError::MissingAuthorizedSource)?;
            let payer = payers
                .get(&key)
                .ok_or(DirectWalletSnapshotError::MissingAuthorizedSource)?;
            let inventory = reader.read_purse(&payer.signature).await?;
            let capacity = u64::try_from(inventory.balance.unwrap_or(0))
                .map_err(|_| DirectWalletSnapshotError::NegativeBalance)?;
            Ok::<_, DirectWalletSnapshotError>(DirectWalletSource {
                source: PhloFundingSource {
                    custody,
                    capacity,
                    exposure_limit: policy.hold_cap(),
                    debit_limit: policy.debit_cap(),
                },
                inventory,
            })
        }))
        .buffered(concurrency)
        .try_collect()
        .await?;
        if reader.pre_state_root() != expected_pre_state {
            return Err(DirectWalletSnapshotError::WrongPreState);
        }
        Ok(DirectWalletSnapshot {
            authorization: self,
            pre_state_root: expected_pre_state,
            sources,
        })
    }
}

// Added by G3 (DR-135): the owner budget of R3.
#[cfg(test)]
mod owner_budget_tests {
    use proptest::prelude::*;

    use super::owner_budget;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1024))]

        /// The budget is the balance less the later holds and the own hold,
        /// never below zero. The oracle subtracts the sum of the two holds
        /// in `u128` instead of applying the two saturating steps.
        #[test]
        fn budget_equals_capped_balance_minus_own_hold(
            balance in prop_oneof![Just(0_u64), Just(u64::MAX), any::<u64>()],
            later in prop_oneof![
                Just(0_u128),
                Just(u128::from(u64::MAX)),
                Just(u128::from(u64::MAX) + 1),
                Just(u128::MAX),
                any::<u64>().prop_map(u128::from),
                any::<u128>(),
            ],
            own_hold in prop_oneof![
                Just(0_u128),
                Just(u128::from(u64::MAX)),
                Just(u128::MAX),
                any::<u64>().prop_map(u128::from),
                any::<u128>(),
            ],
        ) {
            let budget = owner_budget(balance, later, own_hold);
            let held = later.saturating_add(own_hold);
            let oracle = u128::from(balance).saturating_sub(held);
            prop_assert_eq!(u128::from(budget), oracle);
            prop_assert!(budget <= balance);
            prop_assert!(
                budget == 0 || u128::from(budget) + later + own_hold == u128::from(balance),
                "a positive budget leaves exactly the two holds of the balance"
            );
        }
    }
}
