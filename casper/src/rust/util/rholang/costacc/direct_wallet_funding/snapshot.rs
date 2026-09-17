use futures::{stream, StreamExt, TryStreamExt};
use rholang::rust::interpreter::accounting::phlo_execution::PhloFundingSource;

use super::*;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::acceptance::SupplyReader;
use crate::rust::util::rholang::supply::PurseInventory;

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
}

#[derive(Debug, Error)]
pub enum DirectWalletSnapshotError {
    #[error("direct-wallet snapshot reader has a different pre-state root")]
    WrongPreState,
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
        let concurrency = parallel_reads.get().min(self.payers.len()).max(1);
        let sources = stream::iter(self.record.sources.iter().map(|policy| async {
            let key: [u8; 32] = policy
                .custody()
                .try_into()
                .map_err(|_| DirectWalletSnapshotError::MissingAuthorizedSource)?;
            let payer = self
                .payers
                .get(&key)
                .ok_or(DirectWalletSnapshotError::MissingAuthorizedSource)?;
            let inventory = reader.read_purse(&payer.signature).await?;
            let capacity = u64::try_from(inventory.balance.unwrap_or(0))
                .map_err(|_| DirectWalletSnapshotError::NegativeBalance)?;
            Ok::<_, DirectWalletSnapshotError>(DirectWalletSource {
                source: PhloFundingSource {
                    custody: policy.custody(),
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
