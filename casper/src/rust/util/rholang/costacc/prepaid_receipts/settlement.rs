use std::collections::HashMap;
use std::mem::size_of;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::Event;
use models::rust::host_work::HostWorkDimension;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;

use super::snapshot::reserve;
use super::{
    invalid, NativeRetainedBirthLimits, PrepaidReceiptChange, PrepaidReceiptLimits,
    PreparedNativeRetainedReceipts,
};
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::event_converter;

#[derive(Clone, Copy, Debug)]
pub struct NativeRetainedSettlementLimits {
    pub births: NativeRetainedBirthLimits,
    pub receipts: PrepaidReceiptLimits,
}

#[derive(Debug)]
pub struct NativeRetainedSettlement {
    pub log: Vec<Event>,
    pub mergeable: HashMap<Par, MergeType>,
}

impl RuntimeOps {
    pub async fn apply_retained_wallet_settlement<A: Send + Sync>(
        &mut self,
        prepared: PreparedNativeRetainedReceipts<'_, '_, '_, '_, A>,
        reservation_id: [u8; 32],
        fee_address: &VaultAddress,
        initial_rand: Blake2b512Random,
        limits: NativeRetainedSettlementLimits,
        budget: &HostWorkBudget,
    ) -> Result<NativeRetainedSettlement, CasperError> {
        if super::consumption::used_resources(
            prepared.births().settlement().capture().scoped().capture(),
        )
        .next()
        .is_some()
        {
            return Err(invalid(
                "prepaid execution requires checked physical consumption",
            ));
        }
        self.apply_retained_wallet_settlement_internal(
            prepared,
            reservation_id,
            fee_address,
            initial_rand,
            limits,
            budget,
        )
        .await
    }

    pub(super) async fn apply_retained_wallet_settlement_internal<A: Send + Sync>(
        &mut self,
        prepared: PreparedNativeRetainedReceipts<'_, '_, '_, '_, A>,
        reservation_id: [u8; 32],
        fee_address: &VaultAddress,
        initial_rand: Blake2b512Random,
        limits: NativeRetainedSettlementLimits,
        budget: &HostWorkBudget,
    ) -> Result<NativeRetainedSettlement, CasperError> {
        let births = prepared.births();
        births
            .settlement()
            .check_execution_root(&self.runtime.get_root().await.bytes())?;
        let count = prepared.changes().len();
        if count > limits.receipts.entries {
            return Err(invalid(
                "retained settlement receipt count exceeds its limit",
            ));
        }
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            count
                .checked_mul(size_of::<PrepaidReceiptChange>())
                .ok_or_else(|| invalid("retained settlement allocation overflow"))?,
        )?;
        let mut changes = Vec::new();
        changes
            .try_reserve_exact(count)
            .map_err(|_| invalid("retained settlement allocation failed"))?;
        changes.extend(prepared.changes());
        let funding = births
            .funding()
            .capture()
            .bind_retained_births(births.funding().births(), limits.births.funding, budget)
            .map_err(|error| invalid(&error.to_string()))?;
        let mut wallet = births
            .settlement()
            .prepare_request(reservation_id, fee_address, initial_rand, budget)
            .map_err(|error| invalid(&error.to_string()))?;
        if self
            .capture_retained_birth_stacks(&funding, limits.births, budget)
            .await?
            != births.stacks()
        {
            return Err(invalid("retained settlement physical capture is stale"));
        }
        let checkpoint = self.runtime.create_soft_checkpoint().await;
        let result = async {
            let mut settlement = NativeRetainedSettlement {
                log: Vec::new(),
                mergeable: HashMap::new(),
            };
            if let Some(mut request) = wallet.request() {
                let (log, result, mergeable) =
                    self.play_system_deploy_internal(&mut request).await?;
                if let Either::Left(error) = result {
                    return Err(invalid(&format!(
                        "wallet settlement rejected: {}",
                        error.error_message
                    )));
                }
                settlement.log = log;
                settlement.mergeable = mergeable;
            }
            if self
                .capture_retained_birth_stacks(&funding, limits.births, budget)
                .await?
                != births.stacks()
            {
                return Err(invalid(
                    "wallet settlement changed retained resource backing",
                ));
            }
            settlement.log.extend(
                self.replace_prepaid_receipts(&changes, limits.receipts)
                    .await?
                    .into_iter()
                    .map(event_converter::to_casper_event),
            );
            Ok(settlement)
        }
        .await;
        match result {
            Ok(mut settlement) => {
                let mut log: Vec<_> = checkpoint
                    .log
                    .into_iter()
                    .map(event_converter::to_casper_event)
                    .collect();
                log.append(&mut settlement.log);
                settlement.log = log;
                Ok(settlement)
            }
            Err(error) => {
                self.runtime.revert_to_soft_checkpoint(checkpoint).await;
                Err(error)
            }
        }
    }
}
