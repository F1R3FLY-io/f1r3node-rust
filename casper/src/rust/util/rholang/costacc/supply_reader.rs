use std::future::Future;
use std::mem::size_of;
use std::num::NonZeroUsize;
use std::pin::Pin;

use models::rhoapi::{CostSignature, ListParWithRandom, Par};
use models::rust::block::state_hash::StateHash;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use prost::Message;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
use rholang::rust::interpreter::accounting::monetary_allocation::MonetaryCursor;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::history::native_reader::{
    decode_record, NativeLeafKind, NativeReadCharge, NativeReadError, NativeReadMeter,
};
use rspace_plus_plus::rspace::internal::Datum;

use super::supply::{self, PurseInventory, PurseStack};
use super::{monetary_cursor, vault_payer};
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

type ReadFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, CasperError>> + Send + 'a>>;

pub trait SupplyReader: Send + Sync {
    fn pre_state_root(&self) -> [u8; 32];
    fn read_monetary_cursor(
        &self,
        scope: [u8; 32],
        payer_count: NonZeroUsize,
    ) -> ReadFuture<'_, Option<MonetaryCursor>>;
    fn read_purse<'a>(
        &'a self,
        signature: &'a CostSignature,
        budget: &'a HostWorkBudget,
    ) -> ReadFuture<'a, PurseInventory>;
}

struct SupplyReadMeter<'a>(&'a HostWorkBudget);

impl NativeReadMeter for SupplyReadMeter<'_> {
    type Error = CasperError;

    fn reserve(&self, charge: NativeReadCharge) -> Result<(), Self::Error> {
        for (dimension, amount) in [
            (HostWorkDimension::VerificationOperations, charge.operations),
            (HostWorkDimension::VerificationBytes, charge.scanned_bytes),
            (HostWorkDimension::SearchStateBytes, charge.backing_bytes),
        ] {
            self.0
                .reserve(
                    dimension,
                    HostWorkUnits::new(u64::try_from(amount).map_err(|_| {
                        CasperError::RuntimeError("purse read work overflow".into())
                    })?),
                )
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        }
        Ok(())
    }
}

fn native_read_error(error: NativeReadError<CasperError>) -> CasperError {
    match error {
        NativeReadError::Host(error) | NativeReadError::Consumer(error) => error,
        NativeReadError::Store(error) => {
            CasperError::RuntimeError(format!("purse history read failed: {error}"))
        }
        NativeReadError::Invalid(error) => {
            CasperError::RuntimeError(format!("purse history is invalid: {error:?}"))
        }
    }
}

pub struct RuntimeManagerSupplyReader<'a> {
    pub runtime_manager: &'a RuntimeManager,
    pub pre_state_hash: StateHash,
}

impl RuntimeManagerSupplyReader<'_> {
    fn data_at_root(
        &self,
        channel: &Par,
        budget: &HostWorkBudget,
    ) -> Result<Vec<Datum<ListParWithRandom>>, CasperError> {
        let root = Blake2b256Hash::from_bytes(self.pre_state_root().to_vec());
        let repo = &self.runtime_manager.history_repo;
        if !repo.contains_root(&root).map_err(|error| {
            CasperError::RuntimeError(format!("purse root lookup failed: {error}"))
        })? {
            return Err(CasperError::RuntimeError(
                "purse root is not registered".into(),
            ));
        }
        let channel_hash = stable_hash_provider::hash(channel);
        let channel_hash: [u8; 32] = channel_hash
            .0
            .as_slice()
            .try_into()
            .map_err(|_| CasperError::RuntimeError("purse channel hash is invalid".into()))?;
        repo.native_history_reader(self.pre_state_root())
            .with_records(
                NativeLeafKind::Data,
                &channel_hash,
                &SupplyReadMeter(budget),
                |records| {
                    SupplyReadMeter(budget).reserve(NativeReadCharge {
                        operations: records.len(),
                        scanned_bytes: 0,
                        backing_bytes: records
                            .len()
                            .checked_mul(size_of::<Datum<ListParWithRandom>>())
                            .ok_or_else(|| {
                                CasperError::RuntimeError("purse inventory size overflow".into())
                            })?,
                    })?;
                    let mut data = Vec::new();
                    data.try_reserve_exact(records.len()).map_err(|_| {
                        CasperError::RuntimeError("purse inventory allocation failed".into())
                    })?;
                    for raw in records.iter() {
                        data.push(
                            decode_record(raw, &SupplyReadMeter(budget))
                                .map_err(native_read_error)?,
                        );
                    }
                    Ok(data)
                },
            )
            .map_err(native_read_error)
            .map(Option::unwrap_or_default)
    }

    async fn query_at_root(&self, source: String) -> Result<Vec<Par>, CasperError> {
        let query = Compiler::source_to_adt(&source).map_err(CasperError::InterpreterError)?;
        let mut runtime = RuntimeOps::new(self.runtime_manager.spawn_runtime().await);
        runtime
            .play_exploratory_par_strict(query, &self.pre_state_hash)
            .await
    }
}

impl SupplyReader for RuntimeManagerSupplyReader<'_> {
    fn pre_state_root(&self) -> [u8; 32] {
        self.pre_state_hash
            .as_ref()
            .try_into()
            .expect("consensus root length")
    }

    fn read_monetary_cursor(
        &self,
        scope: [u8; 32],
        payer_count: NonZeroUsize,
    ) -> ReadFuture<'_, Option<MonetaryCursor>> {
        Box::pin(async move {
            let values = self
                .query_at_root(monetary_cursor::query_source(&scope))
                .await?;
            monetary_cursor::decode_snapshot(&values, payer_count)
        })
    }

    fn read_purse<'a>(
        &'a self,
        signature: &'a CostSignature,
        budget: &'a HostWorkBudget,
    ) -> ReadFuture<'a, PurseInventory> {
        Box::pin(async move {
            let payer = vault_payer::vault_payer(signature)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
            let sig = cost_signature_to_sig(&payer.signature)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
            let data = self.data_at_root(&supply::supply_channel(&sig), budget)?;
            let mut decoded_bytes = 0usize;
            for datum in &data {
                decoded_bytes = decoded_bytes
                    .checked_add(datum.a.encoded_len())
                    .ok_or_else(|| {
                        CasperError::RuntimeError("purse inventory size overflow".into())
                    })?;
            }
            let reserve_bytes = decoded_bytes
                .checked_mul(8)
                .and_then(|amount| {
                    data.len()
                        .checked_mul(size_of::<PurseStack>() + 128)
                        .and_then(|overhead| amount.checked_add(overhead))
                })
                .ok_or_else(|| CasperError::RuntimeError("purse inventory size overflow".into()))?;
            SupplyReadMeter(budget).reserve(NativeReadCharge {
                operations: reserve_bytes,
                scanned_bytes: decoded_bytes,
                backing_bytes: reserve_bytes,
            })?;
            let mut inventory = supply::decode_purse_inventory(&data, signature)?;
            let values = self
                .query_at_root(vault_payer::balance_query_source(&payer.address))
                .await?;
            let [value] = values.as_slice() else {
                return Err(CasperError::RuntimeError(
                    "SystemVault balance response is not singular".into(),
                ));
            };
            let balance = rholang::rust::interpreter::rho_type::RhoNumber::unapply(value)
                .ok_or_else(|| {
                    CasperError::RuntimeError(
                        "SystemVault balance response is not an integer".into(),
                    )
                })?;
            if balance < 0 {
                return Err(CasperError::RuntimeError(
                    "SystemVault balance is negative".into(),
                ));
            }
            inventory.balance = Some(balance);
            Ok(inventory)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::CostStack;
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use rholang::rust::interpreter::accounting::Sig;
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rspace_plus_plus::rspace::rspace::RSpaceStore;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    #[tokio::test]
    async fn rooted_purse_reader_matches_live_records_and_rejects_exhaustion() {
        let stores = RSpaceStore {
            history: Arc::new(InMemoryKeyValueStore::new()),
            roots: Arc::new(InMemoryKeyValueStore::new()),
            cold: Arc::new(InMemoryKeyValueStore::new()),
        };
        let (manager, _) = RuntimeManager::create_with_history(
            stores,
            KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            Arc::new(Default::default()),
            ExternalServices::noop(),
        );
        let mut runtime = manager.spawn_runtime().await;
        let head = CostSignature {
            value: Some(Value::Ground(b"rooted-purse".to_vec())),
        };
        let channel = supply::supply_channel(&Sig::Ground(b"rooted-purse".to_vec()));
        let datum = ListParWithRandom {
            pars: Vec::new(),
            random_state: vec![9],
            cost_authority: None,
            cost_stack: Some(CostStack { cells: vec![head] }),
        };
        runtime
            .reducer
            .space
            .produce(channel.clone(), datum, false)
            .await
            .unwrap();
        let expected = runtime.reducer.space.get_data(&channel).await;
        let root = runtime.create_checkpoint().await.root.to_bytes_prost();
        let reader = RuntimeManagerSupplyReader {
            runtime_manager: &manager,
            pre_state_hash: root.clone(),
        };
        let ample = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(10_000_000)));
        assert_eq!(reader.data_at_root(&channel, &ample).unwrap(), expected);
        let exhausted = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
        assert!(reader.data_at_root(&channel, &exhausted).is_err());
        assert!(exhausted.is_rejected());
        let mut other = root.as_ref().to_vec();
        other[0] ^= 1;
        let wrong = RuntimeManagerSupplyReader {
            runtime_manager: &manager,
            pre_state_hash: other.into(),
        };
        assert!(wrong.data_at_root(&channel, &ample).is_err());
    }
}
