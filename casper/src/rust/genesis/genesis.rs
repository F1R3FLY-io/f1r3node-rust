// See casper/src/main/scala/coop/rchain/casper/genesis/Genesis.scala

use std::collections::HashMap;

use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::Par;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, DeployData, F1r3flyState, ProcessedDeploy,
};
use prost::bytes::Bytes;
use rholang::rust::interpreter::merging::mergeable_tags;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;

use super::contracts::proof_of_stake::ProofOfStake;
use super::contracts::standard_deploys;
use super::contracts::vault::Vault;
use crate::rust::errors::CasperError;
use crate::rust::util::proto_util;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Genesis {
    pub shard_id: String,
    pub timestamp: i64,
    pub block_number: i64,
    pub proof_of_stake: ProofOfStake,
    pub vaults: Vec<Vault>,
    pub client_fuel_allocations: Vec<(PublicKey, i64)>,
    pub supply: i64,
    pub version: i64,
    /// Full display name of the native token (e.g. "F1R3CAP"). Baked into
    /// the `TokenMetadata` Rholang contract at genesis.
    pub native_token_name: String,
    /// Ticker symbol of the native token (e.g. "F1R3").
    pub native_token_symbol: String,
    /// Number of decimal places for native token display (dust per token = 10^decimals).
    pub native_token_decimals: u32,
}

impl Genesis {
    pub fn vaults_with_protocol_funding(
        proof_of_stake: &ProofOfStake,
        vaults: &[Vault],
        client_fuel_allocations: &[(PublicKey, i64)],
    ) -> Result<Vec<Vault>, CasperError> {
        Self::validate_cost_accounting_parameters(proof_of_stake, client_fuel_allocations)?;
        let mut balances = std::collections::BTreeMap::<String, (VaultAddress, u64)>::new();
        for vault in vaults {
            let entry = balances
                .entry(vault.vault_address.to_base58())
                .or_insert_with(|| (vault.vault_address.clone(), 0));
            entry.1 = entry.1.checked_add(vault.initial_balance).ok_or_else(|| {
                CasperError::RuntimeError("genesis vault balance overflow".to_string())
            })?;
        }
        let initial_phlogiston =
            u64::try_from(proof_of_stake.initial_phlogiston).map_err(|_| {
                CasperError::RuntimeError("initial_phlogiston must be non-negative".to_string())
            })?;
        for validator in &proof_of_stake.validators {
            let address = VaultAddress::from_public_key(&validator.pk).ok_or_else(|| {
                CasperError::RuntimeError(
                    "validator public key has no native vault address".to_string(),
                )
            })?;
            let entry = balances
                .entry(address.to_base58())
                .or_insert_with(|| (address, 0));
            entry.1 = entry.1.checked_add(initial_phlogiston).ok_or_else(|| {
                CasperError::RuntimeError("genesis validator fuel balance overflow".to_string())
            })?;
        }
        for (public_key, amount) in client_fuel_allocations {
            let address = VaultAddress::from_public_key(public_key).ok_or_else(|| {
                CasperError::RuntimeError(
                    "client public key has no native vault address".to_string(),
                )
            })?;
            let amount = u64::try_from(*amount).map_err(|_| {
                CasperError::RuntimeError("client fuel allocation must be non-negative".to_string())
            })?;
            let entry = balances
                .entry(address.to_base58())
                .or_insert_with(|| (address, 0));
            entry.1 = entry.1.checked_add(amount).ok_or_else(|| {
                CasperError::RuntimeError("genesis client fuel balance overflow".to_string())
            })?;
        }
        Ok(balances
            .into_values()
            .map(|(vault_address, initial_balance)| Vault {
                vault_address,
                initial_balance,
            })
            .collect())
    }

    pub fn validate_cost_accounting_parameters(
        proof_of_stake: &ProofOfStake,
        client_fuel_allocations: &[(PublicKey, i64)],
    ) -> Result<(), CasperError> {
        if proof_of_stake.epoch_length <= 0 {
            return Err(CasperError::RuntimeError(format!(
                "epoch_length must be positive; got {}",
                proof_of_stake.epoch_length
            )));
        }
        if proof_of_stake.max_cosigners_per_deploy == 0 {
            return Err(CasperError::RuntimeError(
                "max_cosigners_per_deploy must be at least 1".to_string(),
            ));
        }
        if proof_of_stake.initial_phlogiston < 0 {
            return Err(CasperError::RuntimeError(format!(
                "initial_phlogiston must be non-negative; got {}",
                proof_of_stake.initial_phlogiston
            )));
        }
        if proof_of_stake.epoch_phlogiston < 0 {
            return Err(CasperError::RuntimeError(format!(
                "epoch_phlogiston must be non-negative; got {}",
                proof_of_stake.epoch_phlogiston
            )));
        }
        for validator in &proof_of_stake.validators {
            if validator.pk.bytes.is_empty() {
                return Err(CasperError::RuntimeError(
                    "validator public key must be non-empty".to_string(),
                ));
            }
            VaultAddress::from_public_key(&validator.pk).ok_or_else(|| {
                CasperError::RuntimeError(
                    "validator public key has no native vault address".to_string(),
                )
            })?;
        }
        for (public_key, amount) in client_fuel_allocations {
            if public_key.bytes.is_empty() {
                return Err(CasperError::RuntimeError(
                    "client public key must be non-empty".to_string(),
                ));
            }
            if *amount < 0 {
                return Err(CasperError::RuntimeError(format!(
                    "client fuel allocation must be non-negative; got {amount}"
                )));
            }
            VaultAddress::from_public_key(public_key).ok_or_else(|| {
                CasperError::RuntimeError(
                    "client public key has no native vault address".to_string(),
                )
            })?;
        }
        Ok(())
    }
    pub fn non_negative_mergeable_tag_name() -> Par {
        mergeable_tags::non_negative_mergeable_tag_name()
    }

    pub fn bitmask_or_mergeable_tag_name() -> Par {
        mergeable_tags::bitmask_or_mergeable_tag_name()
    }

    pub fn default_mergeable_tags() -> HashMap<Par, MergeType> {
        mergeable_tags::default_mergeable_tags()
    }

    pub fn default_blessed_terms_with_timestamp(
        timestamp: i64,
        pos_params: &ProofOfStake,
        vaults: &Vec<Vault>,
        supply: i64,
        shard_id: &str,
        native_token_name: &str,
        native_token_symbol: &str,
        native_token_decimals: u32,
    ) -> Vec<Signed<DeployData>> {
        // Splits initial vaults creation in multiple deploys (batches)
        const BATCH_SIZE: usize = 100;

        // Create vault deploys only if vaults are not empty
        let mut vault_deploys = Vec::new();
        if !vaults.is_empty() {
            let batch_count = (vaults.len() + BATCH_SIZE - 1) / BATCH_SIZE;
            vault_deploys.reserve(batch_count);

            for (idx, chunk) in vaults.chunks(BATCH_SIZE).enumerate() {
                let is_last_batch = idx == batch_count - 1;
                let deploy_timestamp = timestamp + idx as i64;

                let batch_vaults = chunk.to_vec();

                let deploy = standard_deploys::vaults_generator(
                    batch_vaults,
                    supply,
                    deploy_timestamp,
                    is_last_batch,
                    shard_id,
                );

                vault_deploys.push(deploy);
            }
        }

        // Order of deploys is important for Registry to work correctly
        // - dependencies must be defined first in the list
        let registry = standard_deploys::registry(shard_id);
        let versioned_registry = standard_deploys::versioned_registry(shard_id);
        let list_ops = standard_deploys::list_ops(shard_id);
        let either = standard_deploys::either(shard_id);
        let non_negative_number = standard_deploys::non_negative_number(shard_id);
        let make_mint = standard_deploys::make_mint(shard_id);
        // Cost-Accounted Rho Stage D: the blessed `Exchange` (rho:lang:exchange)
        // — the spec's conserving 1:1 token swap (tex:3061-3084). Like
        // `make_mint` it depends on nothing beyond Registry, so it is deployed
        // right after the mint; the closeBlock per-epoch fee→v conversion
        // (PoS.rhox) and #13 clients resolve it via its `rho:lang:exchange`
        // shorthand.
        let exchange = standard_deploys::exchange(shard_id);
        let auth_key = standard_deploys::auth_key(shard_id);
        let system_vault = standard_deploys::system_vault(shard_id);
        let multi_sig_system_vault = standard_deploys::multi_sig_system_vault(shard_id);
        let stack = standard_deploys::stack(shard_id);
        let token_metadata = standard_deploys::token_metadata(
            native_token_name,
            native_token_symbol,
            native_token_decimals,
            shard_id,
        );
        let pos_generator = standard_deploys::pos_generator(pos_params, shard_id);
        let capabilities_registry = standard_deploys::capabilities_registry(shard_id);

        let mut all_deploys = Vec::with_capacity(13 + vault_deploys.len());
        all_deploys.push(registry);
        all_deploys.push(versioned_registry);
        all_deploys.push(list_ops);
        all_deploys.push(either);
        all_deploys.push(non_negative_number);
        all_deploys.push(make_mint);
        // Stage D blessed Exchange, immediately after the mint (see binding above).
        all_deploys.push(exchange);
        all_deploys.push(auth_key);
        all_deploys.push(system_vault);
        all_deploys.push(multi_sig_system_vault);
        all_deploys.push(stack);
        all_deploys.push(token_metadata);
        all_deploys.extend(vault_deploys);
        all_deploys.push(pos_generator);
        // Phase 3 LL-rich algebra: capability registry for Bang/Lolly.
        // Deployed last among system contracts because it has no
        // dependencies on any other genesis deploy.
        all_deploys.push(capabilities_registry);

        all_deploys
    }

    pub fn default_blessed_terms(
        pos_params: &ProofOfStake,
        vaults: &Vec<Vault>,
        supply: i64,
        shard_id: &str,
        native_token_name: &str,
        native_token_symbol: &str,
        native_token_decimals: u32,
    ) -> Vec<Signed<DeployData>> {
        // Use hardcoded timestamp for backwards compatibility
        const BASE_TIMESTAMP: i64 = 1565818101792;
        Self::default_blessed_terms_with_timestamp(
            BASE_TIMESTAMP,
            pos_params,
            vaults,
            supply,
            shard_id,
            native_token_name,
            native_token_symbol,
            native_token_decimals,
        )
    }

    pub async fn create_genesis_block(
        runtime_manager: &RuntimeManager,
        genesis: &Genesis,
    ) -> Result<BlockMessage, CasperError> {
        let funded_vaults = Self::vaults_with_protocol_funding(
            &genesis.proof_of_stake,
            &genesis.vaults,
            &genesis.client_fuel_allocations,
        )?;
        let blessed_terms = Self::default_blessed_terms(
            &genesis.proof_of_stake,
            &funded_vaults,
            genesis.supply,
            &genesis.shard_id,
            &genesis.native_token_name,
            &genesis.native_token_symbol,
            genesis.native_token_decimals,
        );

        let (start_hash, state_hash, processed_deploys) = runtime_manager
            .compute_genesis(blessed_terms, genesis.timestamp, genesis.block_number)
            .await?;

        let block_message =
            Self::create_processed_deploy(genesis, start_hash, state_hash, processed_deploys);

        Ok(block_message)
    }

    fn create_processed_deploy(
        genesis: &Genesis,
        start_hash: StateHash,
        state_hash: StateHash,
        processed_deploys: Vec<ProcessedDeploy>,
    ) -> BlockMessage {
        let state = F1r3flyState {
            pre_state_hash: start_hash,
            post_state_hash: state_hash,
            block_number: genesis.block_number,
            bonds: Self::bonds_proto(&genesis.proof_of_stake),
        };

        let failed_deploys: Vec<_> = processed_deploys
            .iter()
            .filter(|deploy| deploy.is_failed)
            .collect();

        assert!(failed_deploys.is_empty(), "Failed deploys found");

        let sorted_deploys = processed_deploys
            .into_iter()
            .filter(|deploy| !deploy.is_failed)
            .map(|mut deploy| {
                crate::rust::util::event_converter::canonicalize_casper_events(
                    &mut deploy.deploy_log,
                );
                deploy
            })
            .collect();

        let body = Body {
            state,
            deploys: sorted_deploys,
            rejected_deploys: Vec::new(),
            rejected_state_effects: Vec::new(),
            system_deploys: Vec::new(),
            extra_bytes: Bytes::new(),
        };

        let header = proto_util::block_header(Vec::new(), genesis.version, genesis.timestamp);
        proto_util::unsigned_block_proto(body, header, Vec::new(), genesis.shard_id.clone(), None)
    }

    fn bonds_proto(proof_of_stake: &ProofOfStake) -> Vec<Bond> {
        let mut bonds: Vec<_> = proof_of_stake
            .validators
            .iter()
            .map(|validator| (validator.pk.clone(), validator.stake))
            .collect();

        bonds.sort_by_key(|(pk, _)| pk.bytes.clone());

        bonds
            .into_iter()
            .map(|(pk, stake)| Bond {
                validator: pk.bytes.into(),
                stake,
            })
            .collect()
    }
}
