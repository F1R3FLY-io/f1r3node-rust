// See casper/src/main/scala/coop/rchain/casper/engine/BlockApproverProtocol.scala

use std::collections::HashMap;
use std::sync::Arc;

use comm::rust::peer_node::PeerNode;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::{Blob, TransportLayer};
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlockCandidate, BlockApproval, ProcessedDeploy, UnapprovedBlock,
};
use models::rust::casper::protocol::packet_type_tag::ToPacket;
use prost::bytes::Bytes;
use prost::Message;
use tracing::{info, warn};

use crate::rust::errors::CasperError;
use crate::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use crate::rust::genesis::contracts::validator::Validator;
use crate::rust::genesis::contracts::vault::Vault;
use crate::rust::genesis::genesis::Genesis;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validator_identity::ValidatorIdentity;

/// Rust port of `coop.rchain.casper.engine.BlockApproverProtocol` from Scala.
/// The field layout and logic mirror the original as closely as possible.
#[derive(Clone)]
pub struct BlockApproverProtocol<T: TransportLayer + Send + Sync + 'static> {
    // Configuration / static data
    validator_id: ValidatorIdentity,
    pub deploy_timestamp: i64,
    pub vaults: Vec<Vault>,
    pub bonds_bytes: HashMap<Bytes, i64>, // helper map keyed by raw bytes
    pub minimum_bond: i64,
    pub maximum_bond: i64,
    pub epoch_length: i32,
    pub quarantine_length: i32,
    pub number_of_active_validators: u32,
    pub fault_tolerance_threshold_ppm: i64,
    pub required_sigs: i32,
    pub pos_multi_sig_public_keys: Vec<String>,
    pub pos_multi_sig_quorum: u32,
    pub max_cosigners_per_deploy: u32,
    pub initial_phlogiston: i64,
    pub epoch_phlogiston: i64,
    pub protocol_version: i64,
    pub client_fuel_allocations: Vec<(PublicKey, i64)>,
    pub native_token_name: String,
    pub native_token_symbol: String,
    pub native_token_decimals: u32,

    /// Static-provisioning bundle to hand to the FsGenesis deploy
    /// (Phase 7 slice 25 wire-up).  MUST equal the value used by
    /// `ApproveBlockProtocolFactory::create` on the proposer;
    /// otherwise validator reconstruction of the genesis blessed-
    /// contract sequence hashes differently and the block is
    /// rejected.  Empty vec preserves pre-slice-25 behavior.
    pub fs_bundle: Vec<crate::rust::genesis::contracts::fs_genesis::BundleEntry>,

    /// CRIT-2 fix (2026-08-06): shard-wide consensus filesystem
    /// snapshot cadence.  MUST equal the proposer's HOCON
    /// `storage.consensus-fs-snapshot-cadence` value at genesis
    /// composition time — a mismatch causes the composed fs_generator
    /// deploy to serialize differently on this validator vs. the
    /// proposer, and `validate_candidate`'s byte-for-byte deploy
    /// diff fails.  `None` = no consensus snapshotting on this shard
    /// (default).
    pub consensus_fs_snapshot_cadence: Option<u64>,

    // Infrastructure
    transport: Arc<T>,
    conf: Arc<RPConf>,
}

impl<T: TransportLayer + Send + Sync + 'static> BlockApproverProtocol<T> {
    /// Corresponds to Scala `BlockApproverProtocol.of` – constructor with basic validation.
    pub fn new(
        validator_id: ValidatorIdentity,
        deploy_timestamp: i64,
        vaults: Vec<Vault>,
        bonds: HashMap<crypto::rust::public_key::PublicKey, i64>,
        minimum_bond: i64,
        maximum_bond: i64,
        epoch_length: i32,
        quarantine_length: i32,
        number_of_active_validators: u32,
        fault_tolerance_threshold_ppm: i64,
        required_sigs: i32,
        pos_multi_sig_public_keys: Vec<String>,
        pos_multi_sig_quorum: u32,
        max_cosigners_per_deploy: u32,
        initial_phlogiston: i64,
        epoch_phlogiston: i64,
        protocol_version: i64,
        client_fuel_allocations: Vec<(PublicKey, i64)>,
        native_token_name: String,
        native_token_symbol: String,
        native_token_decimals: u32,
        fs_bundle: Vec<crate::rust::genesis::contracts::fs_genesis::BundleEntry>,
        consensus_fs_snapshot_cadence: Option<u64>,
        transport: Arc<T>,
        conf: Arc<RPConf>,
    ) -> Result<Self, CasperError> {
        crate::rust::casper::ensure_supported_casper_protocol_version(protocol_version)?;

        tracing::info!(
            required_sigs = required_sigs,
            "Validator configured required_sigs"
        );

        if bonds.len() <= required_sigs as usize {
            return Err(CasperError::RuntimeError(format!(
                "Required sigs ({}) must be smaller than the number of bonded validators ({})",
                required_sigs,
                bonds.len()
            )));
        }

        let bonds_bytes: HashMap<Bytes, i64> = bonds
            .iter()
            .map(|(pk, stake)| (pk.bytes.clone(), *stake))
            .collect();

        Ok(Self {
            validator_id,
            deploy_timestamp,
            vaults,
            bonds_bytes,
            minimum_bond,
            maximum_bond,
            epoch_length,
            quarantine_length,
            number_of_active_validators,
            fault_tolerance_threshold_ppm,
            required_sigs,
            pos_multi_sig_public_keys,
            pos_multi_sig_quorum,
            max_cosigners_per_deploy,
            initial_phlogiston,
            epoch_phlogiston,
            protocol_version,
            client_fuel_allocations,
            native_token_name,
            native_token_symbol,
            native_token_decimals,
            fs_bundle,
            consensus_fs_snapshot_cadence,
            transport,
            conf,
        })
    }

    /// Corresponds to Scala `BlockApproverProtocol.getBlockApproval` / `getApproval` –
    /// signs candidate ApprovedBlockCandidate and creates `BlockApproval`.
    pub fn get_block_approval(&self, candidate: &ApprovedBlockCandidate) -> BlockApproval {
        let sig_data = Blake2b256::hash(candidate.clone().to_proto().encode_to_vec());
        let sig = self.validator_id.signature(&sig_data);
        BlockApproval {
            candidate: candidate.clone(),
            sig,
        }
    }

    /// NOTE: Why is this a public static method instead of an instance method?
    ///
    /// This design matches the Scala implementation where `validateCandidate` is a static
    /// method in the companion object
    ///
    /// Reasons for static method:
    /// 1. **Testing flexibility**: Tests need to validate candidates with intentionally
    ///    wrong parameters (wrong bonds, wrong vaults, wrong genesis params) to verify
    ///    rejection logic. With an instance method, we'd need to create new protocol
    ///    instances for each test case, which is cumbersome and verbose.
    ///
    /// 2. **Separation of concerns**: Validation is a pure function that doesn't require
    ///    the protocol's network/transport infrastructure. It only needs validation
    ///    parameters and a RuntimeManager.
    ///
    /// 3. **1:1 Scala port compliance**: Keeping the same API structure as Scala ensures
    ///    behavioral equivalence and makes cross-referencing easier during porting.
    ///
    /// Corresponds to Scala `BlockApproverProtocol.validateCandidate` –
    /// performs full validation of the candidate genesis block.
    pub async fn validate_candidate(
        runtime_manager: &RuntimeManager,
        candidate: &ApprovedBlockCandidate,
        required_sigs: i32,
        _deploy_timestamp: i64,
        vaults: &Vec<Vault>,
        bonds: &HashMap<Bytes, i64>,
        minimum_bond: i64,
        maximum_bond: i64,
        epoch_length: i32,
        quarantine_length: i32,
        number_of_active_validators: u32,
        fault_tolerance_threshold_ppm: i64,
        shard_id: &str,
        pos_multi_sig_public_keys: &[String],
        pos_multi_sig_quorum: u32,
        max_cosigners_per_deploy: u32,
        initial_phlogiston: i64,
        epoch_phlogiston: i64,
        protocol_version: i64,
        client_fuel_allocations: &[(PublicKey, i64)],
        native_token_name: &str,
        native_token_symbol: &str,
        native_token_decimals: u32,
        fs_bundle: &[crate::rust::genesis::contracts::fs_genesis::BundleEntry],
        // CRIT-2 (2026-08-06): the validator's local HOCON cadence
        // flows into the composed fs_generator deploy term.  If the
        // leader's cadence differs from ours, the reconstructed
        // deploy's serialized term won't match `block.body.deploys`
        // and validation fails at the deploy-diff site below — the
        // consensus check that closes the "shared Genesis hash but
        // silently divergent snapshot cadence" CRIT-2 gap.
        consensus_fs_snapshot_cadence: Option<u64>,
    ) -> Result<(), String> {
        // Basic checks – required sigs, absence of system deploys, bonds equality
        if candidate.required_sigs < required_sigs {
            return Err(format!(
                "Candidate required_sigs mismatch: expected {}, got {}",
                required_sigs, candidate.required_sigs
            ));
        }

        let block = &candidate.block;
        if block.header.version != protocol_version {
            return Err(format!(
                "Candidate protocol version mismatch: expected {}, got {}",
                protocol_version, block.header.version
            ));
        }
        if !block.body.system_deploys.is_empty() {
            return Err("Candidate must not contain system deploys.".to_string());
        }

        let block_bonds: HashMap<Bytes, i64> = block
            .body
            .state
            .bonds
            .iter()
            .map(|b| (b.validator.clone(), b.stake))
            .collect();

        if &block_bonds != bonds {
            return Err("Block bonds don't match expected.".to_string());
        }

        // Prepare PoS params
        let validators: Vec<Validator> = block_bonds
            .iter()
            .map(|(pk_bytes, stake)| Validator {
                pk: crypto::rust::public_key::PublicKey::new(pk_bytes.clone()),
                stake: *stake,
            })
            .collect();

        let pos_params = ProofOfStake {
            minimum_bond,
            maximum_bond,
            validators,
            epoch_length,
            quarantine_length,
            number_of_active_validators,
            // Must match the ceremony master's value: the pos_generator deploy is
            // replayed byte-for-byte, so a ppm mismatch fails genesis validation —
            // ceremony participants must agree on the protocol FTT like every
            // other genesis parameter.
            fault_tolerance_threshold_ppm,
            pos_multi_sig_public_keys: pos_multi_sig_public_keys.to_vec(),
            pos_multi_sig_quorum,
            max_cosigners_per_deploy,
            initial_phlogiston,
            epoch_phlogiston,
        };
        let funded_vaults =
            Genesis::vaults_with_protocol_funding(&pos_params, vaults, client_fuel_allocations)
                .map_err(|error| error.to_string())?;

        tracing::info!(
            shard_id = %shard_id,
            pos_minimum_bond = pos_params.minimum_bond,
            pos_maximum_bond = pos_params.maximum_bond,
            pos_epoch_length = pos_params.epoch_length,
            pos_quarantine_length = pos_params.quarantine_length,
            vault_count = vaults.len(),
            "genesis parameters resolved",
        );

        // Expected blessed contracts.  Slice 25 (C-25-1 review
        // fix): use the fs_bundle passed to `validate_candidate`
        // rather than a hardcoded empty vec.  MUST equal the
        // proposer's bundle byte-for-byte; the caller
        // (`get_block_approval` chain) reads it from
        // `self.fs_bundle` which was set at BlockApproverProtocol
        // construction — same config source as
        // `ApproveBlockProtocolFactory::create`.
        //
        // Cost-accounted merge: use `&funded_vaults` (vaults after
        // protocol-funding transform) rather than raw `vaults`, per
        // cost-accounted-rho's vault-backed parallel execution
        // semantics.
        let genesis_blessed_contracts =
            crate::rust::genesis::genesis::Genesis::default_blessed_terms(
                &pos_params,
                &funded_vaults,
                i64::MAX,
                shard_id,
                native_token_name,
                native_token_symbol,
                native_token_decimals,
                fs_bundle,
                consensus_fs_snapshot_cadence,
            );

        let block_deploys: &Vec<ProcessedDeploy> = &block.body.deploys;

        if block_deploys.len() != genesis_blessed_contracts.len() {
            return Err(
                "Mismatch between number of candidate deploys and expected number of deploys."
                    .to_string(),
            );
        }

        // Check deploys equality (order matters)
        let wrong_deploys: Vec<String> = block_deploys
            .iter()
            .zip(genesis_blessed_contracts.iter())
            .filter(|(candidate_deploy, expected_contract)| {
                candidate_deploy.deploy.data.term != expected_contract.data.term
            })
            .map(|(candidate_deploy, _)| {
                let term = &candidate_deploy.deploy.data.term;
                term.chars().take(100).collect::<String>()
            })
            .take(5)
            .collect();

        if !wrong_deploys.is_empty() {
            return Err(format!(
                "Genesis candidate deploys do not match expected blessed contracts.\nBad contracts (5 first):\n{}",
                wrong_deploys.join("\n")
            ));
        }

        // State hash checks
        let empty_state_hash = RuntimeManager::empty_state_hash_fixed();
        let state_hash = runtime_manager
            .replay_block_from_consensus_data(&empty_state_hash, block, None)
            .await
            .map_err(|e| format!("Failed status during replay: {:?}.", e))?;

        if state_hash != block.body.state.post_state_hash {
            return Err("Tuplespace hash mismatch.".to_string());
        }

        // Bonds computed from tuplespace
        let tuplespace_bonds = runtime_manager
            .compute_bonds(&block.body.state.post_state_hash)
            .await
            .map_err(|e| format!("{:?}", e))?;

        let tuplespace_bonds_map: HashMap<Bytes, i64> = tuplespace_bonds
            .into_iter()
            .map(|b| (b.validator, b.stake))
            .collect();

        if &tuplespace_bonds_map != bonds {
            return Err("Tuplespace bonds don't match expected ones.".to_string());
        }

        Ok(())
    }

    /// Internal instance method that delegates to the static validate_candidate.
    /// This provides a convenient API for unapproved_block_packet_handler which
    /// already has all parameters in self.
    async fn validate_candidate_internal(
        &self,
        runtime_manager: &RuntimeManager,
        candidate: &ApprovedBlockCandidate,
        shard_id: &str,
    ) -> Result<(), String> {
        Self::validate_candidate(
            runtime_manager,
            candidate,
            self.required_sigs,
            self.deploy_timestamp,
            &self.vaults,
            &self.bonds_bytes,
            self.minimum_bond,
            self.maximum_bond,
            self.epoch_length,
            self.quarantine_length,
            self.number_of_active_validators,
            self.fault_tolerance_threshold_ppm,
            shard_id,
            &self.pos_multi_sig_public_keys,
            self.pos_multi_sig_quorum,
            self.max_cosigners_per_deploy,
            self.initial_phlogiston,
            self.epoch_phlogiston,
            self.protocol_version,
            &self.client_fuel_allocations,
            &self.native_token_name,
            &self.native_token_symbol,
            self.native_token_decimals,
            &self.fs_bundle,
            self.consensus_fs_snapshot_cadence,
        )
        .await
    }

    /// Corresponds to Scala `BlockApproverProtocol.unapprovedBlockPacketHandler` –
    /// verifies candidate message from peer and streams approval if valid.
    pub async fn unapproved_block_packet_handler(
        &self,
        runtime_manager: &RuntimeManager,
        peer: &PeerNode,
        unapproved_block: UnapprovedBlock,
        shard_id: &str,
    ) -> Result<(), CasperError> {
        let candidate = unapproved_block.candidate.clone();
        info!(
            "Received expected genesis block candidate from {}. Verifying...",
            peer.endpoint.host
        );

        match self
            .validate_candidate_internal(runtime_manager, &candidate, shard_id)
            .await
        {
            Ok(_) => {
                let approval = self.get_block_approval(&candidate);
                let packet = approval.to_proto().mk_packet();
                let blob = Blob {
                    sender: self.conf.local.clone(),
                    packet,
                };

                self.transport.stream(peer, &blob).await.map_err(|e| {
                    CasperError::RuntimeError(format!(
                        "Failed to stream BlockApproval to peer: {}",
                        e
                    ))
                })?;

                info!(
                    "Approved genesis block candidate from {}. Approval sent in response.",
                    peer.endpoint.host
                );
            }
            Err(err_msg) => {
                warn!(
                    "Received unexpected genesis block candidate from {} because: {}",
                    peer.endpoint.host, err_msg
                );
            }
        }

        Ok(())
    }
}
