// See casper/src/main/scala/coop/rchain/casper/genesis/Genesis.scala

use std::collections::HashMap;

use crypto::rust::signatures::signed::Signed;
use models::rhoapi::Par;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, DeployData, F1r3flyState, ProcessedDeploy,
};
use prost::bytes::Bytes;
use rholang::rust::interpreter::merging::mergeable_tags;
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
        let blessed_terms = Self::default_blessed_terms(
            &genesis.proof_of_stake,
            &genesis.vaults,
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

        // ★★ S3 — A LOAD-BEARING CANONICALISATION. ⚠ DO NOT REMOVE THIS CALL.
        //
        // What it masks: the PER-DEPLOY RSPACE EVENT LOG ORDER. `deploy.deploy_log` is the
        // sequence of RSpace comm/produce/consume events the deploy generated while
        // executing, and the order it ARRIVES in is not stable — it is the order in which
        // the reducer's concurrently-evaluated `Par` members reached the logging seams.
        // Unlike its siblings S1 (`contracts/proof_of_stake.rs`) and S2 (`bonds_proto`
        // below), which each mask ONE identifiable unordered `HashMap`, there is no single
        // container here to canonicalise at the source. That root cause is still open.
        //
        // ⚠ Removing this INTRODUCES a break that is currently latent: `deploy_log` is part
        // of `ProcessedDeploy`, `ProcessedDeploy` is part of `Body.deploys`, and `Body` is
        // hashed into the **`block_hash`**, so an unsorted log yields a different genesis
        // block hash per run. That is the `f5b2e820` trap — deleting a sort that is masking
        // an unordered source turns a hidden defect into a live one.
        //
        // ★ CORRECTION (2026-07-30) to what this comment used to claim. It said the genesis
        // ceremony's `block_approver_protocol` "comparison would reject a block it should
        // have accepted". ⚠ **That is not what the approver does.**
        // `BlockApproverProtocol::validate_candidate`
        // (`casper/src/rust/engine/block_approver_protocol.rs:151-316`) compares the bonds
        // SET, compares each deploy's `deploy.data.term` in order, REPLAYS the candidate's
        // deploys and compares `post_state_hash`, then compares the tuplespace bonds. It
        // never recomputes the candidate's block hash and never inspects `deploy_log` order,
        // and an approver signs `Blake2b256::hash(candidate.to_proto().encode_to_vec())`
        // over the bytes it RECEIVED. So the exposure is NOT cross-node approval; it is that
        // a node re-deriving genesis from identical inputs — a restart mid-ceremony, a
        // second builder in the same shard, the `GENESIS_CACHE` key — gets a different block
        // hash each time. Still a defect, still load-bearing, but attributed correctly.
        //
        // ★ SETTLED (2026-07-30): REPLAY DOES NOT APPLY THIS, AND MUST NOT NEED TO.
        // `ReplayRSpace::rig` reduces the log to a `HashSet<IOEvent>` plus a
        // `MultisetMultiMap<IOEvent, COMM>` of per-key COUNTERS, both functions of the event
        // MULTISET — so permuting the log cannot move `replay_data`, and canonicalising it
        // here cannot break replay. See
        // [`crate::rust::util::event_log_canonical`] for the derivation and
        // `casper/tests/deploy_log_canonicalization_and_replay.rs` for both halves as
        // executable claims.
        let sorted_deploys = processed_deploys
            .into_iter()
            .filter(|deploy| !deploy.is_failed)
            .map(|mut deploy| {
                crate::rust::util::event_log_canonical::canonicalize_deploy_log(
                    &mut deploy.deploy_log,
                );
                deploy
            })
            .collect();

        let body = Body {
            state,
            deploys: sorted_deploys,
            rejected_deploys: Vec::new(),
            system_deploys: Vec::new(),
            extra_bytes: Bytes::new(),
        };

        let header = proto_util::block_header(Vec::new(), genesis.version, genesis.timestamp);
        proto_util::unsigned_block_proto(body, header, Vec::new(), genesis.shard_id.clone(), None)
    }

    /// ★★ **S2 — A LOAD-BEARING SORT. Do not remove it.**
    ///
    /// # What it masks
    ///
    /// The same unordered source as **S1**
    /// ([`super::contracts::proof_of_stake::ProofOfStake::initial_bonds`]):
    /// `ProofOfStake::validators` is collected straight out of a `HashMap` at both production
    /// construction sites — `engine/approve_block_protocol.rs:167` (from
    /// `BondsParser::parse_with_autogen`) and `engine/block_approver_protocol.rs:199` (from
    /// `block_bonds: HashMap<Bytes, i64>`) — so its order is random per map instance.
    ///
    /// # Why removing it would be a consensus break
    ///
    /// The `Vec<Bond>` this returns becomes `F1r3flyState::bonds`, which sits in `Body.state`
    /// and is hashed into the **`block_hash`**. `BlockApproverProtocol` then compares a
    /// candidate genesis against its own locally computed expectation; an unsorted bonds list
    /// makes that comparison fail on ordering alone, so no node would ever approve another
    /// node's genesis.
    ///
    /// # ⚠ S1 and S2 are two independent repairs of ONE defect
    ///
    /// They canonicalise the same random order for two different digests — S1 for
    /// `post_state_hash` (via rendered Rholang source), S2 for `block_hash` (via the block
    /// body). Because either alone makes a *local* run look self-consistent, it is tempting to
    /// read one as redundant. It is not: **deleting either INTRODUCES a consensus break that
    /// was always latent** (the `f5b2e820` lesson). The real fix is to canonicalise
    /// `validators` at its unordered source — a `BTreeMap`, or a `Vec` sorted at parse time —
    /// after which both sorts become provably redundant and can be retired together.
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
