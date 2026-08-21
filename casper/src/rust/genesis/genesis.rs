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

// H-25-1 slice-25 review fix: manual `Hash` impl.  `Vec<BundleEntry>`'s
// derived Hash is order-sensitive, but `format_bundle_for_rholang` sorts
// before emission — so two Genesis structs with reordered `fs_bundle`
// produce the same composed source but would hash differently under the
// derived impl.  We sort `fs_bundle` by logical_name before hashing to
// keep `hash()` consistent with the composed-source identity.  The
// derived `PartialEq` remains order-sensitive; that's a known caveat
// (documented above the struct) but no consumer today relies on Genesis
// equality across bundle orderings.
#[derive(Clone, PartialEq, Eq)]
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
    /// File I/O FIP static-provisioning bundle (Phase 7 slice 25).
    /// Threaded into `fs_generator`'s composed source as the 4th
    /// argument of `Fs!?(0, 1, 2, <bundle>)`.  Node's boot pipeline
    /// projects this from the merged config+CLI `FileIoProvisioning`
    /// via `provisioning_merge::project_bundle`.  Empty vec if no
    /// provisioning is configured (preserves pre-slice-25 behavior).
    pub fs_bundle: Vec<super::contracts::fs_genesis::BundleEntry>,
    /// Slice 30c (PB-M-15 cadence-in-DAG fix): consensus-mode
    /// filesystem snapshot cadence, in blocks.  Shard-wide parameter
    /// agreed at genesis so all validators decide identically which
    /// blocks are snapshot boundaries — required for the join
    /// protocol to have a canonical "give me the snapshot at
    /// finalized block N" answer.
    ///
    /// `None` = no consensus filesystem snapshotting on this shard
    /// (default; also the setting for shards with no `consensus-
    /// static-*` provisioning).  `Some(n)` = snapshot every `n`
    /// finalized blocks; `n >= 1` enforced by boot validation.
    ///
    /// Pre-slice-30c, cadence was a per-node HOCON key
    /// (`storage.consensus-fs-snapshot-cadence`); that key is now
    /// deprecated and ignored (`build_snapshot_writer` reads
    /// cadence from this Genesis field).  Retention (`retain`)
    /// remains per-node local (see
    /// `NodeConfig.storage.consensus_fs_snapshot_retain`).
    pub consensus_fs_snapshot_cadence: Option<u64>,
}

impl std::hash::Hash for Genesis {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.shard_id.hash(state);
        self.timestamp.hash(state);
        self.block_number.hash(state);
        self.proof_of_stake.hash(state);
        self.vaults.hash(state);
        self.supply.hash(state);
        self.version.hash(state);
        self.native_token_name.hash(state);
        self.native_token_symbol.hash(state);
        self.native_token_decimals.hash(state);
        // H-25-1: sort by logical_name before hashing so two Genesis
        // with reordered bundles hash the same (matches the
        // sort-then-emit invariant in `format_bundle_for_rholang`).
        let mut sorted: Vec<&super::contracts::fs_genesis::BundleEntry> =
            self.fs_bundle.iter().collect();
        sorted.sort_by(|a, b| a.logical_name.cmp(&b.logical_name));
        for e in sorted {
            e.hash(state);
        }
        // Slice 30c: shard-wide cadence is part of the Genesis
        // identity — a shard whose operators disagree on cadence
        // would fork the join protocol.
        self.consensus_fs_snapshot_cadence.hash(state);
    }
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
        fs_bundle: &[crate::rust::genesis::contracts::fs_genesis::BundleEntry],
        // CRIT-2 (2026-08-06): plumbed to the fs_generator deploy so
        // cadence becomes part of the composed deploy term.  Leader
        // and validator with different HOCON cadence produce
        // different fs_generator deploys → validate_candidate rejects.
        consensus_fs_snapshot_cadence: Option<u64>,
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
        let fs_generator =
            standard_deploys::fs_generator(shard_id, fs_bundle, consensus_fs_snapshot_cadence);
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
        all_deploys.push(fs_generator);
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
        fs_bundle: &[crate::rust::genesis::contracts::fs_genesis::BundleEntry],
        // CRIT-2 (2026-08-06): forwarded to fs_generator.  See
        // `default_blessed_terms_with_timestamp` for rationale.
        consensus_fs_snapshot_cadence: Option<u64>,
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
            fs_bundle,
            consensus_fs_snapshot_cadence,
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
            &genesis.fs_bundle,
            genesis.consensus_fs_snapshot_cadence,
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

#[cfg(test)]
mod tests {
    use std::hash::{DefaultHasher, Hash as _, Hasher};
    use std::path::PathBuf;

    use super::*;
    use crate::rust::genesis::contracts::fs_genesis::{
        BundleConsensusMode, BundleEntry, BundleEntryKind,
    };
    use crate::rust::genesis::contracts::proof_of_stake::ProofOfStake;

    fn stub_genesis(bundle: Vec<BundleEntry>) -> Genesis {
        Genesis {
            shard_id: "root".into(),
            timestamp: 0,
            block_number: 0,
            proof_of_stake: ProofOfStake {
                minimum_bond: 1,
                maximum_bond: 1_000_000,
                validators: vec![],
                epoch_length: 100,
                quarantine_length: 100,
                number_of_active_validators: 1,
                fault_tolerance_threshold_ppm: 0,
                pos_multi_sig_public_keys: vec![],
                pos_multi_sig_quorum: 1,
            },
            vaults: vec![],
            supply: 0,
            version: 1,
            native_token_name: "F1R3fly".into(),
            native_token_symbol: "F1R".into(),
            native_token_decimals: 8,
            fs_bundle: bundle,
            consensus_fs_snapshot_cadence: None,
        }
    }

    fn hash_of(g: &Genesis) -> u64 {
        let mut h = DefaultHasher::new();
        g.hash(&mut h);
        h.finish()
    }

    // H-25-1: `Genesis::hash` must be order-insensitive on
    // `fs_bundle` so two structs with reordered entries hash
    // identically — matching the sort-then-emit invariant in
    // `format_bundle_for_rholang` (which controls the composed source
    // that goes on-chain).  A per-field derived `Hash` would violate
    // this.
    #[test]
    fn genesis_hash_ignores_fs_bundle_order() {
        let a = BundleEntry {
            logical_name: "a".into(),
            canon_path: PathBuf::from("/1"),
            kind: BundleEntryKind::File,
            mode: "r".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        };
        let b = BundleEntry {
            logical_name: "b".into(),
            canon_path: PathBuf::from("/2"),
            kind: BundleEntryKind::Dir,
            mode: "rw".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        };
        let g1 = stub_genesis(vec![a.clone(), b.clone()]);
        let g2 = stub_genesis(vec![b, a]);
        assert_eq!(
            hash_of(&g1),
            hash_of(&g2),
            "Genesis hash must be order-insensitive on fs_bundle"
        );
    }

    #[test]
    fn genesis_hash_differs_when_bundle_content_differs() {
        let a = BundleEntry {
            logical_name: "a".into(),
            canon_path: PathBuf::from("/1"),
            kind: BundleEntryKind::File,
            mode: "r".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        };
        let b = BundleEntry {
            logical_name: "b".into(),
            canon_path: PathBuf::from("/2"),
            kind: BundleEntryKind::File,
            mode: "r".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        };
        let g1 = stub_genesis(vec![a.clone()]);
        let g2 = stub_genesis(vec![a, b]);
        assert_ne!(
            hash_of(&g1),
            hash_of(&g2),
            "distinct fs_bundle content should hash differently"
        );
    }

    // Slice 30c: cadence is a shard-wide Genesis parameter.  Two
    // Genesises with different cadence values MUST produce
    // different hashes — otherwise a shard whose validators
    // disagreed on cadence would still hash-agree at genesis, and
    // the join-protocol divergence would only surface when
    // finalization advanced.  Pin here.
    #[test]
    fn genesis_hash_differs_when_only_cadence_differs() {
        let mut g1 = stub_genesis(vec![]);
        let mut g2 = stub_genesis(vec![]);
        g1.consensus_fs_snapshot_cadence = Some(100);
        g2.consensus_fs_snapshot_cadence = Some(200);
        assert_ne!(
            hash_of(&g1),
            hash_of(&g2),
            "Genesis hash must differ when only the shard-wide cadence differs"
        );
    }

    // Companion: None vs Some(n) must also differ — a shard that
    // opts out of snapshotting is a different shard than one that
    // opts in.
    #[test]
    fn genesis_hash_differs_when_cadence_toggles_none_to_some() {
        let mut g1 = stub_genesis(vec![]);
        let mut g2 = stub_genesis(vec![]);
        g1.consensus_fs_snapshot_cadence = None;
        g2.consensus_fs_snapshot_cadence = Some(1);
        assert_ne!(
            hash_of(&g1),
            hash_of(&g2),
            "None vs Some(_) cadence must produce distinct Genesis hashes"
        );
    }

    // MT-26-19 review fix: changing ONLY the consensus_mode field
    // must change the Genesis hash.  Guards against a derive-Hash
    // slip that omitted the field (which would silently launder a
    // consensus cap as oracular in the block hash).
    #[test]
    fn genesis_hash_differs_when_only_cmode_differs() {
        let base = BundleEntry {
            logical_name: "n".into(),
            canon_path: PathBuf::from("/p"),
            kind: BundleEntryKind::File,
            mode: "r".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        };
        let flipped = BundleEntry {
            consensus_mode: BundleConsensusMode::Consensus,
            ..base.clone()
        };
        let g1 = stub_genesis(vec![base]);
        let g2 = stub_genesis(vec![flipped]);
        assert_ne!(
            hash_of(&g1),
            hash_of(&g2),
            "Genesis hash must differ when only cmode differs"
        );
    }

    // ST-26-20 review fix: two BundleEntrys differing ONLY in cmode
    // must be unequal — load-bearing for merge dedup so an accidental
    // partial-Eq derive that dropped the field trips a test.
    #[test]
    fn bundle_entry_ne_when_only_cmode_differs() {
        let base = BundleEntry {
            logical_name: "n".into(),
            canon_path: PathBuf::from("/p"),
            kind: BundleEntryKind::File,
            mode: "r".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        };
        let flipped = BundleEntry {
            consensus_mode: BundleConsensusMode::Consensus,
            ..base.clone()
        };
        assert_ne!(base, flipped);
    }
}
