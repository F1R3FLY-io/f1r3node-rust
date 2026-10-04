use crate::Error;
use casper::rust::{
    genesis::{
        contracts::{
            proof_of_stake::ProofOfStake, validator::Validator as GenesisValidator, vault::Vault,
        },
        genesis::Genesis,
    },
    storage::rnode_key_value_store_manager::new_key_value_store_manager,
    util::rholang::runtime_manager::RuntimeManager,
};
use cordial_consensus::{Chain, ChainSpec};
use crypto::rust::public_key::PublicKey;
use k256::ecdsa::VerifyingKey;
use rholang::rust::interpreter::{
    external_services::ExternalServices, util::vault_address::VaultAddress,
};
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path, sync::Arc};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultAllocation {
    pub public_key: Vec<u8>,
    pub balance: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GenesisSettings {
    pub timestamp: i64,
    pub supply: i64,
    pub token_name: String,
    pub token_symbol: String,
    pub token_decimals: u32,
    pub vaults: Vec<VaultAllocation>,
}

impl Default for GenesisSettings {
    fn default() -> Self {
        Self {
            timestamp: 1_700_000_000_000,
            supply: i64::MAX,
            token_name: "F1R3 Token".into(),
            token_symbol: "F1R3".into(),
            token_decimals: 8,
            vaults: vec![],
        }
    }
}

pub async fn initialize_runtime(
    path: &Path,
    spec: &ChainSpec,
    settings: &GenesisSettings,
) -> Result<(Box<dyn KeyValueStoreManager>, RuntimeManager, [u8; 32]), Error> {
    let chain = Chain::new(spec.clone())?;
    let spec = chain.spec();
    if settings.timestamp < 0
        || settings.supply <= 0
        || settings.token_name.is_empty()
        || settings.token_name.len() > 64
        || settings.token_symbol.is_empty()
        || settings.token_symbol.len() > 16
        || settings.token_decimals > 18
        || settings.vaults.len() > 1024
    {
        return Err(Error::Input("invalid genesis settings".into()));
    }
    let mut validators = Vec::new();
    for validator in &spec.validators {
        let key = VerifyingKey::from_sec1_bytes(&validator.public_key)
            .map_err(|_| Error::Input("invalid genesis validator".into()))?;
        let stake = i64::try_from(validator.weight)
            .map_err(|_| Error::Input("genesis stake exceeds VM range".into()))?;
        validators.push(GenesisValidator {
            pk: PublicKey::from_bytes(key.to_encoded_point(false).as_bytes()),
            stake,
        });
    }
    let mut seen = BTreeSet::new();
    let mut total = 0i64;
    let mut vaults = Vec::new();
    for allocation in &settings.vaults {
        let key = VerifyingKey::from_sec1_bytes(&allocation.public_key)
            .map_err(|_| Error::Input("invalid vault public key".into()))?;
        let public = PublicKey::from_bytes(key.to_encoded_point(false).as_bytes());
        if allocation.balance < 0 || !seen.insert(public.bytes.clone()) {
            return Err(Error::Input("invalid or duplicate vault allocation".into()));
        }
        total = total
            .checked_add(allocation.balance)
            .ok_or_else(|| Error::Input("vault supply overflow".into()))?;
        vaults.push(Vault {
            vault_address: VaultAddress::from_public_key(&public)
                .ok_or_else(|| Error::Input("invalid vault address".into()))?,
            initial_balance: u64::try_from(allocation.balance)
                .map_err(|_| Error::Input("invalid vault balance".into()))?,
        });
    }
    if total > settings.supply {
        return Err(Error::Input("vault allocations exceed supply".into()));
    }
    vaults.sort_by_key(|vault| vault.vault_address.to_base58());
    let keys = validators
        .iter()
        .map(|validator| hex::encode(&validator.pk.bytes))
        .collect();
    let genesis = Genesis {
        shard_id: spec.shard.clone(),
        timestamp: settings.timestamp,
        block_number: 0,
        version: 1,
        supply: settings.supply,
        vaults,
        native_token_name: settings.token_name.clone(),
        native_token_symbol: settings.token_symbol.clone(),
        native_token_decimals: settings.token_decimals,
        proof_of_stake: ProofOfStake {
            minimum_bond: 1,
            maximum_bond: i64::MAX,
            epoch_length: 1000,
            quarantine_length: 50000,
            number_of_active_validators: validators.len() as u32,
            fault_tolerance_threshold_ppm: 0,
            max_parent_depth: 15,
            deploy_lifespan: 50,
            min_phlo_price: 1,
            pos_multi_sig_public_keys: keys,
            pos_multi_sig_quorum: (validators.len() * 2 / 3 + 1) as u32,
            validators,
        },
    };
    let mut manager = new_key_value_store_manager(path.to_owned(), None);
    let stores = manager
        .r_space_stores()
        .await
        .map_err(|error| Error::Runtime(error.to_string()))?;
    let mergeable = RuntimeManager::mergeable_store(&mut manager)
        .await
        .map_err(|error| Error::Runtime(error.to_string()))?;
    let runtime = RuntimeManager::create_with_store(
        stores,
        mergeable,
        Arc::new(Genesis::default_mergeable_tags()),
        ExternalServices::noop(),
    );
    let block = {
        let runtime = runtime.clone();
        crate::DeterministicVm::start()?
            .run(async move { Genesis::create_genesis_block(&runtime, &genesis).await })
            .await?
            .map_err(|error| Error::Runtime(error.to_string()))?
    };
    let root = block
        .body
        .state
        .post_state_hash
        .as_ref()
        .try_into()
        .map_err(|_| Error::Runtime("invalid genesis root".into()))?;
    Ok((Box::new(manager), runtime, root))
}
