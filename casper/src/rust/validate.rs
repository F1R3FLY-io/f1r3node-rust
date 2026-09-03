// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// See casper/src/main/scala/coop/rchain/casper/Validate.scala

//! Block validation — the per-step pipeline a peer block must pass
//! before being admitted into the DAG.
//!
//! ## Pipeline steps (in order)
//!
//! 1. `block_summary` — wire-format + parent + justification structural
//!    checks (T-1, T-2).
//! 2. Certified consensus-context validation — derive the immutable
//!    finalized-floor committee from the block closure and verify exact
//!    justifications plus sender membership.
//! 3. `validate_block_checkpoint` — replay deploys against the pre-state
//!    hash and verify the resulting state matches the block's
//!    `post_state_hash`.
//! 4. `bonds_cache` — verify the block's bonds map matches the bonds
//!    computed from the block's replayed post-state hash.
//! 5. `neglected_invalid_block` — reject a block that cites a rejected
//!    justification. Only eligible equivocation evidence can require a
//!    matching slash deploy.
//! 6. `check_neglected_equivocations_with_update` — see Bug #2 / T-9.2.
//! 7. `check_equivocations` — direct equivocation check against the
//!    sender's prior latest message.
//!
//! D3 (DR-9): the former per-block `phlo_price` minimum-price rule is REMOVED —
//! deploys carry no phlo price/limit; per-signature funding is settled at block
//! assembly by the acceptance gate (against Σ⟦s⟧).
//!
//! ## Slashing-protocol position
//!
//! Every certified rejection gets durable metadata. Only
//! `AdmissibleEquivocation` and `IgnorableEquivocation` create economic
//! evidence.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
#[cfg(feature = "schnorr_secp256k1_experimental")]
use crypto::rust::signatures::{
    frost_secp256k1::FrostSecp256k1, schnorr_secp256k1::SchnorrSecp256k1,
};
use models::casper::Signature as ProtoSignature;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, BlockMessage, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::deploy_id::DeployLookupId;
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use prost::Message;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::dag::dag_ops;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::estimator::declared_parent_depths_valid;
use crate::rust::finality::floor_context::FloorContext;
use crate::rust::slashing_authorization::{
    epoch_for_block_number, validate_received_slash_deploys, CanonicalSlashAuthority,
    SlashAuthError,
};
use crate::rust::system_deploy::is_system_deploy_id;
use crate::rust::util::proto_util;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::ValidBlockProcessing;

pub type PublicKey = Vec<u8>;
pub type Data = Vec<u8>;
pub type Signature = Vec<u8>;

const DRIFT: i64 = 15000; // 15 seconds

/// Namespace for the block-validation functions. P4-6 (slashing audit)
/// originally proposed converting these to module-level free functions,
/// but the unit-struct-as-namespace pattern is idiomatic Rust for
/// associated-function clusters with shared documentation, conditional
/// `cfg`, and call-site disambiguation (`Validate::block_summary` reads
/// at the call site as "a Validate operation" — moving everything to
/// `validate::block_summary` would conflict with the module name and
/// force every caller to either rename its import or use the full path).
/// 78 call sites gain no readability from the rename. The unit struct
/// stays.
pub struct Validate;

impl Validate {
    fn ceremony_threshold_is_authorized(
        local_minimum: i32,
        candidate_threshold: i32,
        bonded_validator_count: usize,
    ) -> bool {
        local_minimum >= 0
            && candidate_threshold >= local_minimum
            && candidate_threshold as usize <= bonded_validator_count
    }

    fn ceremony_signature_count_is_sufficient(
        candidate_threshold: i32,
        valid_distinct_signature_count: usize,
    ) -> bool {
        candidate_threshold >= 0 && valid_distinct_signature_count >= candidate_threshold as usize
    }

    /// Verify a single signature with the named algorithm.
    ///
    /// P1-6: previously implemented as a `HashMap<String, Box<dyn Fn>>` rebuilt
    /// per call; replaced with a `match` dispatch so the hot path
    /// (`signature`, `block_signature`, `approved_block`) does zero heap work.
    fn verify_signature(
        algorithm: &str,
        data: &Data,
        signature: &Signature,
        pub_key: &PublicKey,
    ) -> bool {
        match algorithm {
            "secp256k1" => {
                let secp256k1 = Secp256k1;
                secp256k1.verify(data, signature, pub_key)
            }
            #[cfg(feature = "schnorr_secp256k1_experimental")]
            a if a == SchnorrSecp256k1::name() => {
                let schnorr = SchnorrSecp256k1;
                schnorr.verify(data, signature, pub_key)
            }
            #[cfg(feature = "schnorr_secp256k1_experimental")]
            a if a == FrostSecp256k1::name() => {
                let frost = FrostSecp256k1;
                frost.verify(data, signature, pub_key)
            }
            _ => false,
        }
    }

    /// Returns true iff the named algorithm is supported by `verify_signature`.
    /// Used to distinguish "unsupported algorithm" from "valid algorithm,
    /// signature did not verify" at the block-signature surface.
    fn signature_algorithm_supported(algorithm: &str) -> bool {
        match algorithm {
            "secp256k1" => true,
            #[cfg(feature = "schnorr_secp256k1_experimental")]
            a if a == SchnorrSecp256k1::name() => true,
            #[cfg(feature = "schnorr_secp256k1_experimental")]
            a if a == FrostSecp256k1::name() => true,
            _ => false,
        }
    }

    pub fn signature(d: &Data, sig: &ProtoSignature) -> bool {
        Self::verify_signature(
            &sig.algorithm,
            d,
            &sig.sig.to_vec(),
            &sig.public_key.to_vec(),
        )
    }

    fn ignore(b: &BlockMessage, reason: &str) -> String {
        format!(
            "Ignoring block {} because {}",
            PrettyPrinter::build_string_bytes(&b.block_hash),
            reason
        )
    }

    pub fn approved_block(
        approved_block: &ApprovedBlock,
        expected_required_signatures: i32,
    ) -> bool {
        let block = &approved_block.candidate.block;
        if block.body.state.block_number != 0
            || !block.header.parents_hash_list.is_empty()
            || block.seq_num != 0
            || !block.justifications.is_empty()
            || !matches!(Self::block_hash(block), Either::Right(_))
        {
            tracing::warn!(
                "Received ApprovedBlock that is not a structurally valid genesis block."
            );
            return false;
        }
        if !crate::rust::casper::is_supported_casper_protocol_version(block.header.version) {
            tracing::warn!(
                version = block.header.version,
                "Received ApprovedBlock with unsupported Casper protocol version."
            );
            return false;
        }

        let bonded_validators: HashSet<Bytes> = block
            .body
            .state
            .bonds
            .iter()
            .filter(|bond| bond.stake > 0)
            .map(|bond| bond.validator.clone())
            .collect();
        let candidate_required_signatures = approved_block.candidate.required_sigs;
        if !Self::ceremony_threshold_is_authorized(
            expected_required_signatures,
            candidate_required_signatures,
            bonded_validators.len(),
        ) {
            tracing::warn!(
                expected_required_signatures,
                candidate_required_signatures,
                bonded_validators = bonded_validators.len(),
                "Received ApprovedBlock with an unauthorized or unsatisfiable ceremony threshold."
            );
            return false;
        }

        let candidate_bytes_digest =
            Blake2b256::hash(approved_block.clone().candidate.to_proto().encode_to_vec());

        let mut signatures = HashSet::new();
        for signature in &approved_block.sigs {
            if !bonded_validators.contains(&signature.public_key)
                || !Self::verify_signature(
                    &signature.algorithm,
                    &candidate_bytes_digest,
                    &signature.sig.to_vec(),
                    &signature.public_key.to_vec(),
                )
                || !signatures.insert(signature.public_key.clone())
            {
                tracing::warn!(
                    "Received ApprovedBlock with an unauthorized, invalid, or duplicate ceremony signature."
                );
                return false;
            }
        }

        let log_msg = match signatures.is_empty() {
            true => {
                "ApprovedBlock uses configured zero-signature ceremony authorization.".to_string()
            }
            false => {
                let sigs_str = signatures
                    .iter()
                    .map(|pk| {
                        let hex_str = hex::encode(pk);
                        format!("<{}...>", &hex_str[..10])
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("ApprovedBlock is signed by: {}", sigs_str)
            }
        };

        tracing::info!("{}", log_msg);
        let enough_sigs = Self::ceremony_signature_count_is_sufficient(
            candidate_required_signatures,
            signatures.len(),
        );

        if !enough_sigs {
            tracing::warn!(
                "Received invalid ApprovedBlock message not containing enough valid signatures."
            );
        }

        enough_sigs
    }

    pub fn block_signature(b: &BlockMessage) -> bool {
        if !Self::signature_algorithm_supported(&b.sig_algorithm) {
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    &format!("signature algorithm {} is unsupported.", b.sig_algorithm)
                )
            );
            return false;
        }
        let verified = Self::verify_signature(
            &b.sig_algorithm,
            &b.block_hash.to_vec(),
            &b.sig.to_vec(),
            &b.sender.to_vec(),
        );
        if !verified {
            tracing::warn!("{}", Self::ignore(b, "signature is invalid."));
        }
        verified
    }

    pub fn format_of_fields(b: &BlockMessage) -> bool {
        if b.block_hash.is_empty() {
            tracing::warn!("{}", Self::ignore(b, "block hash is empty."));
            false
        } else if b.sig.is_empty() {
            tracing::warn!("{}", Self::ignore(b, "block signature is empty."));
            false
        } else if b.sig_algorithm.is_empty() {
            tracing::warn!("{}", Self::ignore(b, "block signature algorithm is empty."));
            false
        } else if b.shard_id.is_empty() {
            tracing::warn!("{}", Self::ignore(b, "block shard identifier is empty."));
            false
        } else if b.body.state.post_state_hash.is_empty() {
            tracing::warn!("{}", Self::ignore(b, "block post state hash is empty."));
            false
        } else {
            true
        }
    }

    pub fn version(b: &BlockMessage, version: i64) -> bool {
        let block_version = b.header.version;
        if block_version == version {
            true
        } else {
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    &format!(
                        "received block version {} is the expected version {}.",
                        block_version, version
                    )
                )
            );
            false
        }
    }

    // Validator ordering inside `block_summary` is consensus-critical and
    // has been audited as of `feature/slashing`. The order encoded below
    // matches the spec in docs/casper/theory/slashing/slashing-specification.md
    // and is the same ordering proven correct in the corresponding Rocq
    // theorems for the `T-9.x` family.
    pub async fn block_summary(
        block: &BlockMessage,
        genesis: &BlockMessage,
        s: &mut CasperSnapshot,
        shard_id: &str,
        expiration_threshold: i32,
        max_number_of_parents: i32,
        max_parent_depth: i32,
        depth_buffer: i32,
        block_store: &KeyValueBlockStore,
        disable_validator_progress_check: bool,
    ) -> ValidBlockProcessing {
        Self::block_summary_at_floor(
            block,
            genesis,
            s,
            shard_id,
            expiration_threshold,
            max_number_of_parents,
            max_parent_depth,
            depth_buffer,
            block_store,
            disable_validator_progress_check,
            None,
        )
        .await
    }

    pub async fn block_summary_at_floor(
        block: &BlockMessage,
        genesis: &BlockMessage,
        s: &mut CasperSnapshot,
        shard_id: &str,
        expiration_threshold: i32,
        max_number_of_parents: i32,
        max_parent_depth: i32,
        depth_buffer: i32,
        block_store: &KeyValueBlockStore,
        disable_validator_progress_check: bool,
        floor_ctx: Option<&FloorContext>,
    ) -> ValidBlockProcessing {
        use crate::rust::metrics_constants::*;
        macro_rules! __step {
            ($metric:ident, $body:expr) => {{
                let __t0 = std::time::Instant::now();
                let __r = $body;
                metrics::histogram!($metric, "source" => CASPER_METRICS_SOURCE)
                    .record(__t0.elapsed().as_secs_f64());
                __r
            }};
        }

        tracing::debug!(target: "f1r3fly.casper", "before-block-hash-validation");
        match __step!(
            BLOCK_VALIDATION_BLOCK_HASH_TIME_METRIC,
            Self::block_hash(block)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-timestamp-validation");
        match __step!(
            BLOCK_VALIDATION_TIMESTAMP_TIME_METRIC,
            Self::timestamp(block, block_store)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-shard-identifier-validation");
        match __step!(
            BLOCK_VALIDATION_SHARD_IDENTIFIER_TIME_METRIC,
            Self::shard_identifier(block, shard_id)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-deploys-shard-identifier-validation");
        match __step!(
            BLOCK_VALIDATION_DEPLOYS_SHARD_IDENTIFIER_TIME_METRIC,
            Self::deploys_shard_identifier(block, shard_id)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-validation");
        match __step!(
            BLOCK_VALIDATION_REPEAT_DEPLOY_TIME_METRIC,
            Self::repeat_deploy_at_floor(block, s, block_store, expiration_threshold, floor_ctx,)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-block-number-validation");
        match __step!(
            BLOCK_VALIDATION_BLOCK_NUMBER_TIME_METRIC,
            Self::block_number(block, s)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-future-transaction-validation");
        match __step!(
            BLOCK_VALIDATION_FUTURE_TRANSACTION_TIME_METRIC,
            Self::future_transaction(block)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-transaction-expired-validation");
        match __step!(
            BLOCK_VALIDATION_TRANSACTION_EXPIRATION_TIME_METRIC,
            Self::transaction_expiration(block, expiration_threshold)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-time-based-expiration-validation");
        match __step!(
            BLOCK_VALIDATION_TIME_BASED_EXPIRATION_TIME_METRIC,
            Self::time_based_expiration(block)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-justification-shape-validation");
        match __step!(
            BLOCK_VALIDATION_JUSTIFICATION_FOLLOWS_TIME_METRIC,
            Self::justifications_well_formed(block)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        match Self::justification_provenance(block, genesis, block_store) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-parents-validation");
        match __step!(
            BLOCK_VALIDATION_PARENTS_TIME_METRIC,
            Self::parents(
                block,
                genesis,
                s,
                max_number_of_parents,
                max_parent_depth,
                depth_buffer,
                disable_validator_progress_check,
            )
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-sequence-number-validation");
        match __step!(
            BLOCK_VALIDATION_SEQUENCE_NUMBER_TIME_METRIC,
            Self::sequence_number(block, s)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-justification-regression-validation");
        match __step!(
            BLOCK_VALIDATION_JUSTIFICATION_REGRESSIONS_TIME_METRIC,
            Self::justification_regressions(block, s)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }

        // Equivalent to Scala's "} yield s).value" - return ValidBlock if all validations passed
        Either::Right(ValidBlock::Valid)
    }

    /// Validate no deploy with the same sig has been produced in the chain.
    /// Agnostic of non-parent justifications.
    ///
    /// Recovery exemption: a sig whose latest canonical disposition within
    /// the block's parent scope is a merge rejection can be re-included only
    /// after the rejection carrier is in the block's finalized floor.
    ///
    /// The exemption is a PURE FUNCTION OF THE BLOCK (its parents and the
    /// disposition records in their ancestry), never of the validating
    /// node's live view. An earlier version gated it on the sig's LOCAL
    /// finalization status (`deploy_finalization_status::resolve`) and the
    /// validator's own `rejected_in_scope` snapshot set — both node-local:
    /// two honest validators whose finalization progress differed by one
    /// step returned opposite verdicts for the same block, forking the
    /// network (the roaming `InvalidRepeatDeploy` Heavy Pipeline failures).
    ///
    /// The double-execution defense is preserved deterministically: if a
    /// re-inclusion already won above the rejection in the block's parent
    /// scope, the latest disposition is a win, and the
    /// ancestor scan below finds the canonical inclusion and flags the
    /// repeat. A win that exists only on a fork OUTSIDE the block's parent
    /// scope must NOT poison this block: judged in its own context the
    /// re-inclusion is legal recovery, and the eventual merge's keep-one
    /// dedup reconciles the duplicate.
    pub fn repeat_deploy(
        block: &BlockMessage,
        s: &mut CasperSnapshot,
        block_store: &KeyValueBlockStore,
        expiration_threshold: i32,
    ) -> ValidBlockProcessing {
        Self::repeat_deploy_at_floor(block, s, block_store, expiration_threshold, None)
    }

    pub fn repeat_deploy_at_floor(
        block: &BlockMessage,
        s: &mut CasperSnapshot,
        block_store: &KeyValueBlockStore,
        expiration_threshold: i32,
        floor_ctx: Option<&FloorContext>,
    ) -> ValidBlockProcessing {
        if block.body.deploys.is_empty() {
            return Either::Right(ValidBlock::Valid);
        }

        if block.header.version >= crate::rust::casper::CERTIFIED_FINALIZED_FLOOR_PROTOCOL_VERSION {
            for processed in &block.body.deploys {
                if processed
                    .deploy_id_for_protocol(block.header.version)
                    .and_then(|_| processed.to_cosigned().map(|_| ()))
                    .is_err()
                {
                    return Either::Left(BlockError::Invalid(InvalidBlock::InvalidFormat));
                }
            }
        }

        let mut block_deploy_ids = Vec::with_capacity(block.body.deploys.len());
        let mut unique_deploy_ids = HashSet::with_capacity(block.body.deploys.len());
        for deploy in &block.body.deploys {
            let deploy_id = match deploy.deploy_id_for_protocol(block.header.version) {
                Ok(deploy_id) => deploy_id,
                Err(error) => {
                    return Either::Left(BlockError::BlockException(CasperError::RuntimeError(
                        error,
                    )))
                }
            };
            if !unique_deploy_ids.insert(deploy_id.clone()) {
                return Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy));
            }
            block_deploy_ids.push(deploy_id);
        }

        let block_metadata = BlockMetadata::from_block(block, None, None);

        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-get-parents");
        let init_parents = match proto_util::get_parents_metadata(&s.dag, &block_metadata) {
            Ok(parents) => parents,
            Err(e) => return Either::Left(BlockError::BlockException(CasperError::from(e))),
        };

        // Calculate max block number and earliest acceptable block number
        let max_block_number = proto_util::max_block_number_metadata(&init_parents);
        let earliest_block_number = max_block_number + 1 - expiration_threshold as i64;

        let mut exempt = HashSet::new();
        if let Some(context) = floor_ctx {
            let rejected = match context.rejected_sigs(block_store, earliest_block_number) {
                Ok(sigs) => sigs,
                Err(error) => return Either::Left(BlockError::BlockException(error)),
            };
            for deploy_id in &block_deploy_ids {
                if !rejected.contains(deploy_id) {
                    continue;
                }
                match context.retry_gate_open(&s.dag, block_store, earliest_block_number, deploy_id)
                {
                    Ok(true) => {
                        exempt.insert(deploy_id.clone());
                    }
                    Ok(false) => {
                        return Either::Left(BlockError::Invalid(
                            InvalidBlock::PrematureDeployRetry,
                        ));
                    }
                    Err(error) => return Either::Left(BlockError::BlockException(error)),
                }
            }
        }

        let deploy_key_set: HashSet<DeployLookupId> = block_deploy_ids
            .into_iter()
            .filter(|deploy_id| !exempt.contains(deploy_id))
            .collect();
        if deploy_key_set.is_empty() {
            return Either::Right(ValidBlock::Valid);
        }

        // Repeat-deploy carrier-index fast path (CONSENSUS_PHILOSOPHY
        // §4.4). The index records every carrier from the watermark W
        // onward, so it engages only when the scan window starts at or
        // above W (`w <= max(earliest, 0)` — heights below zero do not
        // exist, so W = 0 means complete over every existing block).
        // Behind that gate, an index absence proves the deploy identity has no
        // in-window carrier, so it cannot be a repeat and skips the scan.
        // An index hit is NOT a verdict — the sig stays in the exact scan,
        // which is the window and parent-scope verification (a fork-only
        // carrier must not poison this block). Any read failure keeps the
        // deploy identity in the scan: unreadable index state is no information, never
        // an absence proof.
        let deploy_key_set: HashSet<DeployLookupId> = match s.dag.carrier_index_watermark() {
            Ok(Some(w)) if w <= earliest_block_number.max(0) => {
                let mut probe_failed = false;
                let scan_set: HashSet<DeployLookupId> = deploy_key_set
                    .into_iter()
                    .filter(|deploy_id| {
                        if probe_failed {
                            return true;
                        }
                        match s.dag.carrier_index_proves_absence(deploy_id) {
                            Ok(absent) => !absent,
                            Err(e) => {
                                tracing::warn!(
                                    "repeat-deploy carrier-index probe failed for block {}; \
                                     falling back to the ancestor scan: {}",
                                    PrettyPrinter::build_string_bytes(&block.block_hash),
                                    e,
                                );
                                probe_failed = true;
                                true
                            }
                        }
                    })
                    .collect();
                scan_set
            }
            Ok(_) => deploy_key_set,
            Err(e) => {
                tracing::warn!(
                    "repeat-deploy carrier-index watermark read failed for block {}; \
                     falling back to the ancestor scan: {}",
                    PrettyPrinter::build_string_bytes(&block.block_hash),
                    e,
                );
                deploy_key_set
            }
        };
        if deploy_key_set.is_empty() {
            return Either::Right(ValidBlock::Valid);
        }
        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-duplicate-block");
        let maybe_duplicated_block_metadata = match dag_ops::try_bf_traverse_find(
            init_parents,
            |block_metadata| {
                proto_util::get_parent_metadatas_above_block_number(
                    block_metadata,
                    earliest_block_number,
                    &s.dag,
                )
            },
            |block_metadata| {
                block_store
                    .has_any_deploy_id_strict(&block_metadata.block_hash, &deploy_key_set)
                    .map_err(CasperError::from)
            },
        ) {
            Ok(found) => found,
            Err(error) => return Either::Left(BlockError::from_validation_error(error)),
        };

        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-duplicate-block-log");
        let maybe_error = maybe_duplicated_block_metadata.map(|duplicated_block_metadata| {
      let duplicated_block = match block_store.get(&duplicated_block_metadata.block_hash) {
        Ok(Some(block)) => block,
        Ok(None) => {
          return BlockError::from_validation_error(CasperError::BlockNotHeld(
            duplicated_block_metadata.block_hash.clone(),
          ));
        }
        Err(error) => {
          return BlockError::from_validation_error(CasperError::from(error));
        }
      };
      let current_block_hash_string = PrettyPrinter::build_string_bytes(&block.block_hash);
      let block_hash_string = PrettyPrinter::build_string_bytes(&duplicated_block.block_hash);

      let duplicated_deploys = proto_util::deploys(&duplicated_block);
      // Convert the previously-panicking `.expect("Duplicated deploy
      // should exist")` into a typed BlockException. The
      // duplicate-deploy index claimed this block carries a matching
      // signature; if the block's own deploy list does NOT contain
      // such a deploy, the index is corrupt — surface as infrastructure
      // failure rather than panicking the validator on hostile or
      // corrupted state.
      let mut matching_deploy = None;
      for processed_deploy in &duplicated_deploys {
        let deploy_id = match processed_deploy
          .deploy_id_for_protocol(duplicated_block.header.version)
        {
          Ok(deploy_id) => deploy_id,
          Err(error) => {
            return BlockError::BlockException(CasperError::RuntimeError(format!(
              "InvalidRepeatDeploy could not decode the indexed deploy identity in block {}: {}",
              block_hash_string,
              error,
            )));
          }
        };
        if deploy_key_set.contains(&deploy_id) {
          matching_deploy = Some(&processed_deploy.deploy);
          break;
        }
      }
      let duplicated_deploy = match matching_deploy {
        Some(deploy) => deploy,
        None => {
          tracing::error!(
            "InvalidRepeatDeploy duplicate-deploy invariant violated: deploy-index claims block {} carries a deploy whose identity collides with current block {}, but no such deploy exists in that block's deploy list",
            block_hash_string,
            current_block_hash_string
          );
          return BlockError::BlockException(CasperError::RuntimeError(format!(
            "InvalidRepeatDeploy duplicate-deploy invariant violated: block {} indexed as duplicate-deploy carrier for current block {} contains no matching deploy",
            block_hash_string,
            current_block_hash_string,
          )));
        }
      };

      let term = &duplicated_deploy.data.term;
      let deployer_string = PrettyPrinter::build_string_bytes(&duplicated_deploy.pk.bytes);
      let timestamp_string = duplicated_deploy.data.time_stamp.to_string();

      let message = format!(
        "found deploy [{}] (user {}, millisecond timestamp {})] with the same identity in the block {} as current block {}",
        term,
        &deployer_string,
        timestamp_string,
        block_hash_string,
        current_block_hash_string
      );

      tracing::warn!("{}", Self::ignore(block, &message));
      BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy)
    });

        maybe_error.map_or(Either::Right(ValidBlock::Valid), Either::Left)
    }

    // This is not a slashable offence
    pub fn timestamp(b: &BlockMessage, block_store: &KeyValueBlockStore) -> ValidBlockProcessing {
        // Pre-epoch system clock is an infrastructure failure, not a
        // block defect. Surfacing it as BlockException (rather than
        // silently defaulting to 0 — which would then accept any
        // 0..+DRIFT timestamp regardless of true wall time) matches
        // the C3 fix for `traits.rs` and keeps the validator honest
        // on a broken clock.
        let current_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_millis() as i64,
            Err(e) => {
                return Either::Left(BlockError::BlockException(CasperError::from(e)));
            }
        };

        let timestamp = b.header.timestamp;

        // Checked addition: a corrupt or far-future system clock could push
        // `current_time + DRIFT` past i64::MAX (operationally ~292 years
        // out). Overflow ⇒ we treat the block as "outside the acceptable
        // future window" and reject. Matches the new "checked-everywhere"
        // discipline in `block_creator.rs`.
        let before_future = match current_time.checked_add(DRIFT) {
            Some(deadline) => deadline >= timestamp,
            None => false,
        };

        let latest_parent_timestamp =
            proto_util::parent_hashes(b)
                .iter()
                .fold(0i64, |latest_timestamp, parent_hash| {
                    let parent = block_store.get_unsafe(parent_hash);
                    let timestamp = parent.header.timestamp;
                    latest_timestamp.max(timestamp)
                });
        let after_latest_parent = timestamp >= latest_parent_timestamp;

        if before_future && after_latest_parent {
            Either::Right(ValidBlock::Valid)
        } else {
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    &format!(
                        "block timestamp {} is not between latest parent block time and current time.",
                        timestamp
                    )
                )
            );
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidTimestamp))
        }
    }

    /// Agnostic of non-parent justifications
    pub fn block_number(b: &BlockMessage, s: &mut CasperSnapshot) -> ValidBlockProcessing {
        let parents: Vec<BlockMetadata> = match proto_util::parent_hashes(b)
            .iter()
            .map(|parent_hash| match s.dag.lookup(parent_hash) {
                Ok(Some(parent_metadata)) => Ok(parent_metadata),
                Ok(None) => Err(KvStoreError::KeyNotFound(format!(
                    "Block dag store was missing {}",
                    PrettyPrinter::build_string_bytes(parent_hash)
                ))),
                Err(e) => Err(e),
            })
            .collect::<Result<Vec<BlockMetadata>, KvStoreError>>()
        {
            Ok(parents) => parents,
            Err(e) => return Either::Left(BlockError::BlockException(CasperError::from(e))),
        };

        let max_block_number = parents
            .iter()
            .fold(-1, |acc, parent| acc.max(parent.block_number));

        let number = proto_util::block_number(b);
        let result = max_block_number + 1 == number;

        if result {
            Either::Right(ValidBlock::Valid)
        } else {
            let log_message = if parents.is_empty() {
                format!(
                    "block number {} is not zero, but block has no parents.",
                    number
                )
            } else {
                format!(
                    "block number {} is not one more than maximum parent number {}.",
                    number, max_block_number
                )
            };

            tracing::warn!("{}", Self::ignore(b, &log_message));
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockNumber))
        }
    }

    pub fn future_transaction(b: &BlockMessage) -> ValidBlockProcessing {
        let block_number = proto_util::block_number(b);

        let processed_deploys = proto_util::deploys(b);
        let deploys: Vec<_> = processed_deploys
            .iter()
            .map(|processed_deploy| &processed_deploy.deploy)
            .collect();

        let maybe_future_deploy = deploys
            .iter()
            .find(|&deploy| deploy.data.valid_after_block_number >= block_number);

        let maybe_error = maybe_future_deploy.map(|future_deploy| {
            let message = format!(
                "block contains an future deploy with valid after block number of {}: {}",
                future_deploy.data.valid_after_block_number, future_deploy.data.term
            );

            tracing::warn!("{}", Self::ignore(b, &message));
            BlockError::Invalid(InvalidBlock::ContainsFutureDeploy)
        });

        maybe_error.map_or(Either::Right(ValidBlock::Valid), Either::Left)
    }

    pub fn transaction_expiration(
        b: &BlockMessage,
        expiration_threshold: i32,
    ) -> ValidBlockProcessing {
        let earliest_acceptable_valid_after_block_number =
            proto_util::block_number(b) - expiration_threshold as i64;

        let processed_deploys = proto_util::deploys(b);
        let deploys: Vec<_> = processed_deploys
            .iter()
            .map(|processed_deploy| &processed_deploy.deploy)
            .collect();

        let maybe_expired_deploy = deploys.iter().find(|&deploy| {
            deploy.data.valid_after_block_number <= earliest_acceptable_valid_after_block_number
        });

        let maybe_error = maybe_expired_deploy.map(|expired_deploy| {
            let message = format!(
                "block contains an expired deploy with valid after block number of {}: {}",
                expired_deploy.data.valid_after_block_number, expired_deploy.data.term
            );

            tracing::warn!("{}", Self::ignore(b, &message));
            BlockError::Invalid(InvalidBlock::ContainsExpiredDeploy)
        });

        maybe_error.map_or(Either::Right(ValidBlock::Valid), Either::Left)
    }

    /// Validates that the block does not contain deploys that have expired based on their
    /// expirationTimestamp field. A deploy is time-expired if its expirationTimestamp is
    /// set (> 0) and the block's timestamp exceeds the expirationTimestamp.
    pub fn time_based_expiration(b: &BlockMessage) -> ValidBlockProcessing {
        let block_timestamp = b.header.timestamp;
        let processed_deploys = proto_util::deploys(b);
        let deploys: Vec<_> = processed_deploys
            .iter()
            .map(|processed_deploy| &processed_deploy.deploy)
            .collect();

        let maybe_time_expired_deploy = deploys
            .iter()
            .find(|&deploy| deploy.data.is_expired_at(block_timestamp));

        let maybe_error = maybe_time_expired_deploy.map(|expired_deploy| {
            let message = format!(
                "block contains a time-expired deploy with expirationTimestamp={:?} but block timestamp is {}: {}",
                expired_deploy.data.expiration_timestamp.unwrap_or(0),
                block_timestamp,
                expired_deploy.data.term
            );

            tracing::warn!("{}", Self::ignore(b, &message));
            BlockError::Invalid(InvalidBlock::ContainsTimeExpiredDeploy)
        });

        maybe_error.map_or(Either::Right(ValidBlock::Valid), Either::Left)
    }

    /// Works with either efficient justifications or full explicit justifications.
    /// Specifically, with efficient justifications, if a block B doesn't update its
    /// creator justification, this check will fail as expected. The exception is when
    /// B's creator justification is the genesis block.
    pub fn sequence_number(b: &BlockMessage, s: &mut CasperSnapshot) -> ValidBlockProcessing {
        let creator_justification_seq_number =
            match proto_util::creator_justification_block_message(b) {
                Some(justification)
                    if s.dag.canonical_genesis_hash() == Some(&justification.latest_block_hash) =>
                {
                    0
                }
                Some(justification) => match s.dag.lookup(&justification.latest_block_hash) {
                    Ok(Some(block_metadata)) => block_metadata.sequence_number as i64,
                    Ok(None) => {
                        return Either::Left(BlockError::BlockException(CasperError::from(
                            KvStoreError::KeyNotFound(format!(
                                "Latest block hash {} is missing from block dag store.",
                                PrettyPrinter::build_string_bytes(&justification.latest_block_hash)
                            )),
                        )));
                    }
                    Err(e) => {
                        return Either::Left(BlockError::BlockException(CasperError::from(e)));
                    }
                },
                None => -1,
            };

        let number = b.seq_num as i64;
        let result = creator_justification_seq_number + 1 == number;

        if result {
            Either::Right(ValidBlock::Valid)
        } else {
            let message = format!(
                "seq number {} is not one more than creator justification number {}.",
                number, creator_justification_seq_number
            );

            tracing::warn!("{}", Self::ignore(b, &message));
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidSequenceNumber))
        }
    }

    // Agnostic of justifications
    pub fn shard_identifier(b: &BlockMessage, shard_id: &str) -> ValidBlockProcessing {
        if b.shard_id == shard_id {
            Either::Right(ValidBlock::Valid)
        } else {
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    &format!(
                        "got shard identifier {} while {} was expected.",
                        b.shard_id, shard_id
                    )
                )
            );
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidShardId))
        }
    }

    // Validator should only process deploys from its own shard
    pub fn deploys_shard_identifier(b: &BlockMessage, shard_id: &str) -> ValidBlockProcessing {
        if b.body
            .deploys
            .iter()
            .all(|deploy| deploy.deploy.data.shard_id == shard_id)
        {
            Either::Right(ValidBlock::Valid)
        } else {
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    &format!("not for all deploys shard identifier is {}.", shard_id)
                )
            );
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidShardId))
        }
    }

    // TODO: Scala message -> Double check this validation isn't shadowed by the blockSignature validation
    pub fn block_hash(b: &BlockMessage) -> ValidBlockProcessing {
        let block_hash_computed = proto_util::hash_block(b);
        if b.block_hash == block_hash_computed {
            Either::Right(ValidBlock::Valid)
        } else {
            let computed_hash_string = PrettyPrinter::build_string_bytes(&block_hash_computed);
            let hash_string = PrettyPrinter::build_string_bytes(&b.block_hash);
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    &format!(
                        "block hash {} does not match to computed value {}.",
                        hash_string, computed_hash_string
                    )
                )
            );
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockHash))
        }
    }

    /// Validates that a validator has made progress since their previous block.
    ///
    /// Rule: If validator V produced block B_prev, then V's next block B_new must have
    /// at least one parent that was not known to V when creating B_prev.
    ///
    /// Exception: Blocks containing user deploys are ALWAYS valid regardless of parent status.
    /// Users pay for their deploys, so validators must provide service immediately.
    ///
    /// This ensures validators only propose empty blocks when they have received new information,
    /// preventing spam while allowing immediate service for paying users.
    pub fn parents(
        b: &BlockMessage,
        genesis: &BlockMessage,
        s: &mut CasperSnapshot,
        max_number_of_parents: i32,
        max_parent_depth: i32,
        depth_buffer: i32,
        disable_validator_progress_check: bool,
    ) -> ValidBlockProcessing {
        // Check if block contains user deploys (non-system deploys)
        let has_user_deploys = b
            .body
            .deploys
            .iter()
            .any(|pd| !is_system_deploy_id(pd.deploy_id()));
        // Slash deploys are liveness-critical recovery actions and must not be blocked
        // by empty-block progress checks.
        let has_slash_system_deploys = b.body.system_deploys.iter().any(|system_deploy| {
            matches!(system_deploy, ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::Slash { .. },
                ..
            })
        });

        let maybe_parent_hashes = proto_util::parent_hashes(b);
        let parent_hashes: Vec<BlockHash> = match maybe_parent_hashes {
            hashes if hashes.is_empty() => vec![genesis.block_hash.clone()],
            hashes => hashes,
        };

        // C15 / Smell-3: shared wire-convention constant — see
        // `crate::rust::casper::UNLIMITED_PARENTS`. This is the
        // config-parsing convention `-1`, distinct from
        // `Estimator::UNLIMITED_PARENTS` (`i32::MAX`) used internally
        // by the GHOST estimator.
        if max_number_of_parents != crate::rust::casper::UNLIMITED_PARENTS
            && parent_hashes.len() > max_number_of_parents as usize
        {
            let message = format!(
                "block has {} parents, but maxNumberOfParents is {}",
                parent_hashes.len(),
                max_number_of_parents
            );
            tracing::warn!("{}", Self::ignore(b, &message));
            return Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents));
        }

        // Parent-depth enforcement: symmetric to proposer-side `Estimator::filterDeepParents`.
        // Reject any block whose parents fall outside the consensus-permitted horizon
        // (depth from highest tip > max_parent_depth + depth_buffer). An honest proposer
        // already drops these parents before signing; this check rejects blocks from
        // buggy or malicious proposers that would otherwise hit `UnknownRootError` on
        // joiners that don't carry pre-horizon rspace history.
        //
        // Sentinel: `max_parent_depth == i32::MAX` disables the check (matches the
        // proposer-side convention in `engine::multi_parent_casper::create_block`).
        //
        // Genesis is exempt: validators justify back to genesis as the ultimate ancestor,
        // and on a long-running chain genesis would always exceed the depth horizon.
        // We compare by hash to the passed `genesis` BlockMessage rather than to
        // `block_number == 0` so this works correctly regardless of how the chain's
        // genesis ended up indexed (test fixtures may assign genesis a non-zero
        // block_number; production assigns 0).
        if max_parent_depth != i32::MAX {
            let max_allowed_depth = (max_parent_depth as i64) + (depth_buffer as i64);
            let mut block_numbers = Vec::with_capacity(parent_hashes.len());
            let mut genesis_slots = Vec::with_capacity(parent_hashes.len());
            for parent_hash in &parent_hashes {
                let parent_meta = match s.dag.lookup_unsafe(parent_hash) {
                    Ok(meta) => meta,
                    Err(error) => {
                        return Either::Left(BlockError::BlockException(CasperError::from(error)));
                    }
                };
                block_numbers.push(parent_meta.block_number);
                genesis_slots.push(parent_hash == &genesis.block_hash);
            }
            match declared_parent_depths_valid(&block_numbers, &genesis_slots, max_allowed_depth) {
                Ok(true) => {}
                Ok(false) => {
                    tracing::warn!(
                        "{}",
                        Self::ignore(b, "a secondary parent exceeds the configured depth horizon")
                    );
                    return Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents));
                }
                Err(error) => {
                    return Either::Left(BlockError::BlockException(CasperError::from(error)));
                }
            }
        }

        let validator = &b.sender;

        let prev_block_hash_opt = b
            .justifications
            .iter()
            .find(|justification| justification.validator == *validator)
            .map(|justification| justification.latest_block_hash.clone());

        match prev_block_hash_opt {
            // First block from this validator - always valid
            None => Either::Right(ValidBlock::Valid),

            // Validator has previous blocks - check progress requirement
            Some(prev_block_hash) => {
                if s.dag.canonical_genesis_hash() == Some(&prev_block_hash) {
                    return Either::Right(ValidBlock::Valid);
                }
                // Get previous block metadata
                let prev_block_meta = match s.dag.lookup(&prev_block_hash) {
                    Ok(Some(meta)) => meta,
                    Ok(None) => {
                        return Either::Left(BlockError::BlockException(CasperError::from(
                            KvStoreError::KeyNotFound(format!(
                                "Previous block {} not found in DAG",
                                PrettyPrinter::build_string_bytes(&prev_block_hash)
                            )),
                        )));
                    }
                    Err(e) => {
                        return Either::Left(BlockError::BlockException(CasperError::from(e)));
                    }
                };

                // Special case: if previous block is genesis (no parents), allow proposal
                // This breaks the deadlock after genesis ceremony when all validators are at genesis
                let is_genesis = prev_block_meta.parents.is_empty();

                let ancestor_hashes: Vec<BlockHash> =
                    dag_ops::bf_traverse(vec![prev_block_hash.clone()], |hash| {
                        match s.dag.lookup(hash) {
                            Ok(Some(meta)) => meta.parents.clone(),
                            _ => vec![],
                        }
                    });
                let ancestor_set: HashSet<BlockHash> = ancestor_hashes.into_iter().collect();

                // Check if at least one parent is new (not in ancestor closure)
                let has_new_parent = parent_hashes.iter().any(|p| !ancestor_set.contains(p));
                // Heartbeat-empty block: no user deploys and only CloseBlock system deploy.
                // Allow these to keep liveness when cluster is stale and parent frontier does not move.
                let is_heartbeat_empty_block = !has_user_deploys
                    && b.body.system_deploys.len() == 1
                    && matches!(
                        &b.body.system_deploys[0],
                        ProcessedSystemDeploy::Succeeded {
                            system_deploy: SystemDeployData::CloseBlockSystemDeployData,
                            ..
                        }
                    );

                // Validation logic:
                // - Blocks with user deploys: always valid (users are paying for service)
                // - Empty blocks: must have new parents (must show progress)
                // - Slash-only blocks: always valid (network recovery action)
                // - Heartbeat-empty blocks: valid to recover from stale/no-progress deadlocks
                // - disable_validator_progress_check: skip progress check (for standalone mode)
                if has_user_deploys
                    || has_slash_system_deploys
                    || is_heartbeat_empty_block
                    || is_genesis
                    || has_new_parent
                    || disable_validator_progress_check
                {
                    Either::Right(ValidBlock::Valid)
                } else {
                    let parents_string = parent_hashes
                        .iter()
                        .map(|hash| PrettyPrinter::build_string_bytes(hash))
                        .collect::<Vec<String>>()
                        .join(",");
                    let prev_block_string = PrettyPrinter::build_string_bytes(&prev_block_hash);
                    let message = format!(
                        "validator {} has not made progress. \
                         Empty block parents [{}] are all ancestors of previous block {}. \
                         Validator must receive new blocks before proposing empty blocks.",
                        PrettyPrinter::build_string_bytes(validator),
                        parents_string,
                        prev_block_string
                    );
                    tracing::warn!("{}", Self::ignore(b, &message));
                    Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents))
                }
            }
        }
    }

    /// This check must come before Validate.parents
    pub fn justifications_well_formed(b: &BlockMessage) -> ValidBlockProcessing {
        if proto_util::parent_hashes(b).is_empty() {
            tracing::warn!("{}", Self::ignore(b, "non-approved block has no parents."));
            return Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents));
        }

        // Reject duplicate-validator justifications upstream. The
        // `justified_validators` HashSet built below silently collapses
        // duplicates, so without this guard a hostile block could list the
        // same validator twice (with two different latest-message pointers)
        // and survive the `bonded_validators == justified_validators`
        // equality check — masking an equivocation. See
        // `formal/rocq/slashing/theories/BugFixDuplicateJustifications.v`.
        let mut seen = HashSet::new();
        if b.justifications
            .iter()
            .any(|justification| !seen.insert(justification.validator.clone()))
        {
            tracing::warn!(
                "{}",
                Self::ignore(b, "block contains duplicate justifications.")
            );
            return Either::Left(BlockError::Invalid(InvalidBlock::InvalidFollows));
        }

        Either::Right(ValidBlock::Valid)
    }

    pub fn justification_provenance(
        b: &BlockMessage,
        genesis: &BlockMessage,
        block_store: &KeyValueBlockStore,
    ) -> ValidBlockProcessing {
        for justification in &b.justifications {
            if justification.latest_block_hash == genesis.block_hash {
                continue;
            }

            let cited = match block_store.get(&justification.latest_block_hash) {
                Ok(Some(block)) => block,
                Ok(None) => {
                    return Either::Left(BlockError::BlockException(CasperError::from(
                        KvStoreError::KeyNotFound(format!(
                            "justification block {} is missing from block store",
                            PrettyPrinter::build_string_bytes(&justification.latest_block_hash)
                        )),
                    )));
                }
                Err(error) => {
                    return Either::Left(BlockError::BlockException(CasperError::from(error)));
                }
            };

            if cited.sender != justification.validator {
                tracing::warn!(
                    "{}",
                    Self::ignore(
                        b,
                        "justification validator does not match the cited block sender",
                    )
                );
                return Either::Left(BlockError::Invalid(InvalidBlock::InvalidFollows));
            }
        }

        Either::Right(ValidBlock::Valid)
    }

    /// Tier-2 validation gate for received `Slash` system deploys. Delegates
    /// to `slashing_authorization::validate_received_slash_deploys` and
    /// distinguishes two failure classes:
    ///
    /// * `CasperError::SlashAuth(_)` — the receive-side authorization
    ///   predicate (4-conjunct check) rejected the slash deploy. The block
    ///   author supplied an invalid slash request. Collapse the result to
    ///   `InvalidBlock::UnauthorizedSlashDeploy`. This rejection cannot create
    ///   economic evidence.
    /// * any other `CasperError` (storage I/O, runtime, history) — the local
    ///   node experienced an infrastructure failure unrelated to the block
    ///   author's behavior. Propagate as `BlockError::BlockException(e)`;
    ///   do NOT slash the block sender for a fault attributable to local
    ///   infrastructure. Bug-fix rationale: see
    ///   docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.14.
    pub fn slash_deploy_authorization(
        block: &BlockMessage,
        s: &CasperSnapshot,
        authority: &CanonicalSlashAuthority,
    ) -> ValidBlockProcessing {
        Self::route_slash_validation_outcome(
            block,
            validate_received_slash_deploys(block, s, authority),
        )
    }

    /// Routes the outcome of `validate_received_slash_deploys` into the
    /// validator's `Either` shape. Exposed `pub` so the dispatching logic —
    /// which distinguishes Byzantine-author errors from local-infrastructure
    /// errors — can be unit-tested from integration tests.
    ///
    /// See `slash_deploy_authorization` for the full rationale and
    /// docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.14
    /// ("Error routing") for the design contract this helper enforces.
    pub fn route_slash_validation_outcome(
        block: &BlockMessage,
        result: Result<(), CasperError>,
    ) -> ValidBlockProcessing {
        match result {
            Ok(()) => Either::Right(ValidBlock::Valid),
            Err(CasperError::SlashAuth(auth_err)) => {
                tracing::warn!(
                    "{}",
                    Self::ignore(block, &format!("unauthorized slash deploy: {}", auth_err))
                );
                Either::Left(BlockError::Invalid(InvalidBlock::UnauthorizedSlashDeploy))
            }
            Err(infra_err) => {
                tracing::warn!(
                    "slash-deploy authorization failed for block {} with infrastructure error: {}; \
                     propagating as BlockException (NOT slashing the block sender)",
                    PrettyPrinter::build_string_bytes(&block.block_hash),
                    infra_err
                );
                Either::Left(BlockError::BlockException(infra_err))
            }
        }
    }

    /// Justification regression check.
    ///
    /// Compares justifications previously cited by `b.sender` in the sender's
    /// creator justification against justifications cited by the new block
    /// `b`, and rejects any regression, including a regression against the
    /// sender's own prior creator justification.
    ///
    /// Bug #6 / T-9.6 (post-fix behavior).
    ///
    /// The pre-fix code path skipped the sender's own creator-justification,
    /// delegating self-regression detection to `checkEquivocations`. That left
    /// a window where a block could point back to an earlier sequence number
    /// of its own sender without being slashed at the validation boundary.
    /// The fix is to walk the full `new_lms` map (built from `b.justifications`
    /// via `to_latest_message_hashes`) without filtering out `b.sender` and
    /// compare every entry against `cur_lms`; a self-regression therefore now
    /// produces `InvalidBlock::JustificationRegression` at the loop body below.
    ///
    /// Proven sound by `t_9_6_self_regression_detected`,
    /// `t_9_6_self_regression_complete`, and `t_9_6_self_regression_in_dag` in
    /// `formal/rocq/slashing/theories/BugFixSelfRegression.v`. See also
    /// `docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md` §9.6.
    pub fn justification_regressions(
        b: &BlockMessage,
        s: &mut CasperSnapshot,
    ) -> ValidBlockProcessing {
        let Some(creator_justification) = proto_util::creator_justification_block_message(b) else {
            return Either::Right(ValidBlock::Valid);
        };
        if s.dag.canonical_genesis_hash() == Some(&creator_justification.latest_block_hash) {
            return Either::Right(ValidBlock::Valid);
        }
        match s.dag.lookup(&creator_justification.latest_block_hash) {
            Ok(None) => Either::Left(BlockError::BlockException(CasperError::from(
                KvStoreError::KeyNotFound(format!(
                    "creator justification {} is missing from the block DAG",
                    PrettyPrinter::build_string_bytes(&creator_justification.latest_block_hash)
                )),
            ))),
            Ok(Some(cur_senders_block)) => {
                let new_sender_block = b;
                let new_lms =
                    proto_util::to_latest_message_hashes(&new_sender_block.justifications);
                let cur_lms =
                    proto_util::to_latest_message_hashes(&cur_senders_block.justifications);

                // Self-regression is checked here too: include the sender's
                // self-justification so a block that points back to its own
                // earlier sequence number is detected as JustificationRegression.
                // See docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.6.

                let log_warn =
                    |current_hash: &BlockHash, regressive_hash: &BlockHash, sender: &Validator| {
                        let msg = format!(
                            "block {} by {} has a lower sequence number than {}.",
                            PrettyPrinter::build_string_bytes(regressive_hash),
                            PrettyPrinter::build_string_bytes(sender),
                            PrettyPrinter::build_string_bytes(current_hash)
                        );
                        tracing::warn!("{}", Self::ignore(b, &msg));
                    };

                // P1-5: single linear scan over the new latest messages; no
                // O(n²) Vec rebuilds. The early-return on regression preserves
                // the prior semantics; the iterator skips senders absent from
                // `cur_lms` (no justification to compare against).
                for (sender, new_justification_hash) in &new_lms {
                    let Some(cur_justification_hash) = cur_lms.get(sender) else {
                        continue;
                    };

                    let new_justification = match s.dag.lookup_unsafe(new_justification_hash) {
                        Ok(metadata) => metadata,
                        Err(e) => {
                            return Either::Left(BlockError::BlockException(CasperError::from(e)))
                        }
                    };
                    let cur_justification = match s.dag.lookup_unsafe(cur_justification_hash) {
                        Ok(metadata) => metadata,
                        Err(e) => {
                            return Either::Left(BlockError::BlockException(CasperError::from(e)))
                        }
                    };

                    if new_justification.is_accepted()
                        && new_justification.sequence_number < cur_justification.sequence_number
                    {
                        log_warn(cur_justification_hash, new_justification_hash, sender);
                        return Either::Left(BlockError::Invalid(
                            InvalidBlock::JustificationRegression,
                        ));
                    }
                }

                Either::Right(ValidBlock::Valid)
            }
            Err(e) => Either::Left(BlockError::BlockException(CasperError::from(e))),
        }
    }

    /// If block contains an invalid justification block B and the creator of B is still bonded,
    /// return a RejectableBlock. Otherwise, return an IncludeableBlock.
    pub fn neglected_invalid_block(
        block: &BlockMessage,
        s: &CasperSnapshot,
        authority: &CanonicalSlashAuthority,
    ) -> ValidBlockProcessing {
        let epoch_length = s.on_chain_state.shard_conf.epoch_length;
        let current_epoch =
            match epoch_for_block_number(block.body.state.block_number, epoch_length) {
                Ok(epoch) => epoch,
                Err(error) => {
                    return Either::Left(BlockError::BlockException(CasperError::from(
                        SlashAuthError::from(error),
                    )))
                }
            };
        if let Err(error) = validate_received_slash_deploys(block, s, authority) {
            return Self::route_slash_validation_outcome(block, Err(error));
        }
        let mut slash_targets = HashSet::new();

        for system_deploy in &block.body.system_deploys {
            let ProcessedSystemDeploy::Succeeded {
                system_deploy:
                    SystemDeployData::Slash {
                        invalid_block_hash,
                        issuer_public_key,
                        target_bond_generation,
                        ..
                    },
                ..
            } = system_deploy
            else {
                continue;
            };
            if issuer_public_key.bytes != block.sender {
                continue;
            }

            let metadata = match s.dag.lookup(invalid_block_hash) {
                Ok(Some(metadata)) => metadata,
                Ok(None) => continue,
                Err(error) => {
                    return Either::Left(BlockError::BlockException(CasperError::from(error)))
                }
            };
            slash_targets.insert((metadata.sender, *target_bond_generation));
        }

        let structural_equivocation_keys = s.dag.structural_equivocation_keys();
        for justification in &block.justifications {
            let metadata = match s.dag.lookup(&justification.latest_block_hash) {
                Ok(Some(metadata)) => metadata,
                Ok(None) => continue,
                Err(error) => {
                    return Either::Left(BlockError::BlockException(CasperError::from(error)))
                }
            };
            if !metadata.is_rejected() {
                continue;
            }
            if !metadata.is_slash_evidence_eligible() {
                return Either::Left(BlockError::Invalid(InvalidBlock::NeglectedInvalidBlock));
            }

            let bond = authority.bond(&metadata.sender);
            let evidence_epoch = match epoch_for_block_number(metadata.block_number, epoch_length) {
                Ok(epoch) => epoch,
                Err(error) => {
                    return Either::Left(BlockError::BlockException(CasperError::from(
                        SlashAuthError::from(error),
                    )))
                }
            };
            let evidence_generation = metadata.sender_bond_generation();
            let structural_collision = evidence_generation.is_some_and(|generation| {
                structural_equivocation_keys.contains(&(
                    metadata.sender.clone(),
                    generation,
                    metadata.sequence_number,
                ))
            });
            let slash_required = evidence_epoch == current_epoch
                && bond > 0
                && evidence_generation.is_some_and(|generation| {
                    authority.generation(&metadata.sender) == Some(generation)
                })
                && !structural_collision;
            if slash_required
                && !metadata.sender_bond_generation().is_some_and(|generation| {
                    slash_targets.contains(&(metadata.sender.clone(), generation))
                })
            {
                return Either::Left(BlockError::Invalid(InvalidBlock::NeglectedInvalidBlock));
            }
        }

        Either::Right(ValidBlock::Valid)
    }

    pub async fn bonds_cache(
        b: &BlockMessage,
        runtime_manager: &RuntimeManager,
    ) -> ValidBlockProcessing {
        let bonds = proto_util::bonds(b);
        let tuplespace_hash = proto_util::post_state_hash(b);

        match tokio::try_join!(
            runtime_manager.compute_bonds(&tuplespace_hash),
            runtime_manager.compute_bond_generations(&tuplespace_hash),
            runtime_manager.get_active_validators(&tuplespace_hash)
        ) {
            Ok((computed_bonds, computed_generations, mut computed_active_validators)) => {
                let computed_generations = match computed_generations
                    .into_iter()
                    .map(|(validator, generation)| {
                        models::rust::bond_generation::BondGeneration::try_from(generation)
                            .map(|generation| (validator, generation))
                    })
                    .collect::<Result<HashMap<_, _>, _>>()
                {
                    Ok(generations) => generations,
                    Err(error) => {
                        return Either::Left(BlockError::BlockException(
                            CasperError::RuntimeError(format!(
                                "PoS returned an invalid bond generation: {error}"
                            )),
                        ));
                    }
                };
                let bonds_set: HashSet<_> = bonds
                    .iter()
                    .map(|bond| (&bond.validator, bond.stake))
                    .collect();
                let computed_bonds_set: HashSet<_> = computed_bonds
                    .iter()
                    .map(|bond| (&bond.validator, bond.stake))
                    .collect();
                let generation_cache = b
                    .body
                    .state
                    .bond_generations
                    .iter()
                    .map(|entry| (entry.validator.clone(), entry.generation))
                    .collect::<HashMap<_, _>>();
                computed_active_validators.sort_unstable();
                computed_active_validators.dedup();
                let active_validator_cache = &b.body.state.active_validators;
                let active_validator_cache_is_canonical = active_validator_cache
                    .iter()
                    .all(|validator| validator.len() == models::rust::validator::LENGTH)
                    && !active_validator_cache
                        .windows(2)
                        .any(|pair| pair[0] >= pair[1]);

                if bonds_set.len() == bonds.len()
                    && computed_bonds_set.len() == computed_bonds.len()
                    && bonds_set == computed_bonds_set
                    && generation_cache.len() == b.body.state.bond_generations.len()
                    && generation_cache == computed_generations
                    && active_validator_cache_is_canonical
                    && *active_validator_cache == computed_active_validators
                {
                    Either::Right(ValidBlock::Valid)
                } else {
                    tracing::warn!(
                        "Bonds in proof of stake contract do not match block's bond cache."
                    );
                    Either::Left(BlockError::Invalid(InvalidBlock::InvalidBondsCache))
                }
            }
            Err(ex) => {
                tracing::warn!("Failed to compute bonds from tuplespace hash: {}", ex);
                Either::Left(BlockError::BlockException(ex))
            }
        }
    }

    // D3 (DR-9, D.5): the `Validate::phlo_price` block rule (all deploys must
    // carry valid phlo terms and a price ≥ minPhloPrice) is REMOVED — deploys
    // carry no phlo price/limit. Funding is enforced at block assembly by the
    // per-signature acceptance gate (`util/rholang/acceptance.rs`) against
    // Σ⟦s⟧. `min_phlo_price` remains economic configuration and cannot certify
    // an otherwise unprovable finite demand bound.
}

#[cfg(test)]
mod merge_recovery_validation_tests {
    use std::collections::BTreeMap;

    use crypto::rust::public_key::PublicKey;
    use models::rust::block_metadata::BlockMetadata;
    use models::rust::casper::protocol::casper_message::{
        ApprovedBlock, ApprovedBlockCandidate, Body, Bond, F1r3flyState, Header, Justification,
    };

    use super::*;

    fn validator(byte: u8) -> Validator { Bytes::from(vec![byte; models::rust::validator::LENGTH]) }

    fn hash(byte: u8) -> BlockHash { Bytes::from(vec![byte; 32]) }

    fn zero_signature_approved_genesis() -> ApprovedBlock {
        let mut block = models::rust::block_implicits::get_random_block_default();
        block.header.parents_hash_list.clear();
        block.header.version = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        block.body.state.block_number = 0;
        block.seq_num = 0;
        block.justifications.clear();
        block.block_hash = proto_util::hash_block(&block);
        ApprovedBlock {
            candidate: ApprovedBlockCandidate {
                block,
                required_sigs: 0,
            },
            sigs: Vec::new(),
            floor_seed: None,
        }
    }

    fn signed_approved_genesis(
        required_sigs: i32,
        bonded_validator_count: usize,
        signature_count: usize,
    ) -> ApprovedBlock {
        let keypairs = (0..bonded_validator_count)
            .map(|_| Secp256k1.new_key_pair())
            .collect::<Vec<_>>();
        let mut block = models::rust::block_implicits::get_random_block_default();
        block.header.parents_hash_list.clear();
        block.header.version = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        block.body.state.block_number = 0;
        block.body.state.bonds = keypairs
            .iter()
            .map(|(_, public_key)| Bond {
                validator: public_key.bytes.clone(),
                stake: 1,
            })
            .collect();
        block.seq_num = 0;
        block.justifications.clear();
        block.block_hash = proto_util::hash_block(&block);
        let candidate = ApprovedBlockCandidate {
            block,
            required_sigs,
        };
        let candidate_digest = Blake2b256::hash(candidate.clone().to_proto().encode_to_vec());
        let sigs = keypairs
            .iter()
            .take(signature_count)
            .map(|(private_key, public_key)| ProtoSignature {
                public_key: public_key.bytes.clone(),
                algorithm: Secp256k1::name(),
                sig: Secp256k1.sign(&candidate_digest, &private_key.bytes).into(),
            })
            .collect();
        ApprovedBlock {
            candidate,
            sigs,
            floor_seed: None,
        }
    }

    #[test]
    fn approved_block_accepts_only_canonical_genesis_shape() {
        let approved = zero_signature_approved_genesis();
        assert!(Validate::approved_block(&approved, 0));

        let mut checkpoint = approved.clone();
        checkpoint.candidate.block.body.state.block_number = 1;
        checkpoint.candidate.block.block_hash = proto_util::hash_block(&checkpoint.candidate.block);
        assert!(!Validate::approved_block(&checkpoint, 0));

        let mut parented = approved.clone();
        parented
            .candidate
            .block
            .header
            .parents_hash_list
            .push(hash(9));
        parented.candidate.block.block_hash = proto_util::hash_block(&parented.candidate.block);
        assert!(!Validate::approved_block(&parented, 0));

        let mut wrong_hash = approved.clone();
        wrong_hash.candidate.block.block_hash = hash(8);
        assert!(!Validate::approved_block(&wrong_hash, 0));
    }

    #[test]
    fn approved_block_threshold_is_local_trust_policy() {
        let approved = zero_signature_approved_genesis();
        assert!(!Validate::approved_block(&approved, 1));

        let mut negative = approved;
        negative.candidate.required_sigs = -1;
        assert!(!Validate::approved_block(&negative, -1));
    }

    #[test]
    fn approved_block_accepts_candidate_threshold_above_local_minimum() {
        let approved = signed_approved_genesis(2, 2, 2);
        assert!(Validate::approved_block(&approved, 1));
    }

    #[test]
    fn approved_block_enforces_candidate_threshold_and_bonded_capacity() {
        let insufficient = signed_approved_genesis(2, 2, 1);
        assert!(!Validate::approved_block(&insufficient, 1));

        let downgrade = signed_approved_genesis(1, 2, 1);
        assert!(!Validate::approved_block(&downgrade, 2));

        let unsatisfiable = signed_approved_genesis(3, 2, 2);
        assert!(!Validate::approved_block(&unsatisfiable, 1));
    }

    proptest::proptest! {
        #[test]
        fn ceremony_authorization_matches_threshold_contract(
            local_minimum in -2i32..6,
            candidate_threshold in -2i32..6,
            bonded_validator_count in 0usize..6,
            signature_count in 0usize..6,
        ) {
            let authorized = Validate::ceremony_threshold_is_authorized(
                local_minimum,
                candidate_threshold,
                bonded_validator_count,
            ) && Validate::ceremony_signature_count_is_sufficient(
                candidate_threshold,
                signature_count,
            );
            let expected = local_minimum >= 0
                && candidate_threshold >= local_minimum
                && candidate_threshold as usize <= bonded_validator_count
                && signature_count >= candidate_threshold as usize;
            proptest::prop_assert_eq!(authorized, expected);
        }
    }

    #[test]
    fn approved_block_rejects_noncurrent_protocol_versions() {
        for version in [
            crate::rust::casper::LEGACY_CASPER_PROTOCOL_VERSION,
            crate::rust::casper::STATE_EFFECT_PROVENANCE_PROTOCOL_VERSION - 1,
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION + 1,
        ] {
            let mut block = models::rust::block_implicits::get_random_block_default();
            block.header.version = version;
            let approved = ApprovedBlock {
                candidate: ApprovedBlockCandidate {
                    block,
                    required_sigs: 0,
                },
                sigs: Vec::new(),
                floor_seed: None,
            };

            assert!(!Validate::approved_block(&approved, 0));
        }
    }

    fn add_metadata(
        snapshot: &mut CasperSnapshot,
        block_hash: BlockHash,
        sender: Validator,
        block_number: i64,
        invalid: bool,
    ) {
        add_metadata_with_reason(
            snapshot,
            block_hash,
            sender,
            block_number,
            invalid.then_some(
                models::rust::block_metadata::AdmissionRejectionReason::AdmissibleEquivocation,
            ),
        );
    }

    fn add_metadata_with_reason(
        snapshot: &mut CasperSnapshot,
        block_hash: BlockHash,
        sender: Validator,
        block_number: i64,
        rejection_reason: Option<models::rust::block_metadata::AdmissionRejectionReason>,
    ) {
        snapshot.on_chain_state.bond_generations.insert(
            sender.clone(),
            models::rust::bond_generation::BondGeneration::GENESIS,
        );
        snapshot.dag.dag_set.insert(block_hash.clone());
        let metadata = BlockMetadata {
            block_hash,
            post_state_hash: Bytes::from(vec![
                block_number as u8;
                models::rust::block_hash::LENGTH
            ]),
            parents: Vec::new(),
            sender: sender.clone(),
            justifications: Vec::new(),
            weight_map: BTreeMap::new(),
            bond_generation_map: BTreeMap::from([(
                sender.clone(),
                models::rust::bond_generation::BondGeneration::GENESIS,
            )]),
            active_validator_set: std::collections::BTreeSet::from([sender.clone()]),
            block_number,
            sequence_number: block_number as i32,
            admission_outcome: None,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
            successful_state_effect_indices: Default::default(),
            rejected_state_effects: Default::default(),
            applied_state_effects: Default::default(),
            protocol_version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            objective_equivocation_evidence_delta: Vec::new(),
            sender_authority: None,
            finalized_floor_commitment: None,
            admission_schema_version: models::rust::block_metadata::ADMISSION_SCHEMA_VERSION,
            approved_genesis: false,
            merge_base: Bytes::new(),
        };
        let metadata = if let Some(reason) = rejection_reason {
            crate::rust::test_metadata::certify_rejected(
                metadata,
                models::rust::bond_generation::BondGeneration::GENESIS,
                reason,
            )
        } else {
            crate::rust::test_metadata::certify(
                metadata,
                models::rust::bond_generation::BondGeneration::GENESIS,
            )
        };
        snapshot
            .dag
            .block_metadata_index
            .write()
            .add(metadata)
            .expect("metadata inserted");
    }

    fn candidate(
        block_number: i64,
        sender: Validator,
        offender: Validator,
        invalid_hash: BlockHash,
        system_deploys: Vec<ProcessedSystemDeploy>,
    ) -> BlockMessage {
        let mut bond_generations = vec![
            models::rust::casper::protocol::casper_message::ValidatorBondGeneration {
                validator: offender.clone(),
                generation: models::rust::bond_generation::BondGeneration::GENESIS,
            },
            models::rust::casper::protocol::casper_message::ValidatorBondGeneration {
                validator: sender.clone(),
                generation: models::rust::bond_generation::BondGeneration::GENESIS,
            },
        ];
        bond_generations.sort();
        let active_validators = bond_generations
            .iter()
            .map(|generation| generation.validator.clone())
            .collect();
        BlockMessage {
            block_hash: hash(0xf0),
            header: Header {
                parents_hash_list: Vec::new(),
                timestamp: block_number,
                version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
                extra_bytes: Bytes::new(),
                sender_bond_generation: Some(
                    models::rust::bond_generation::BondGeneration::GENESIS,
                ),
                objective_equivocation_evidence_delta: Vec::new(),
                finalized_floor: None,
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: Bytes::new(),
                    post_state_hash: Bytes::new(),
                    bonds: vec![
                        Bond {
                            validator: offender.clone(),
                            stake: 1000,
                        },
                        Bond {
                            validator: sender.clone(),
                            stake: 1000,
                        },
                    ],
                    bond_generations,
                    active_validators,
                    block_number,
                },
                deploys: Vec::new(),
                rejected_deploys: Vec::new(),
                rejected_state_effects: Vec::new(),
                applied_state_effects: Vec::new(),
                system_deploys,
                extra_bytes: Bytes::new(),
                applied_from_scope: Vec::new(),
                merge_base: Bytes::new(),
            },
            justifications: vec![Justification {
                validator: offender,
                latest_block_hash: invalid_hash,
            }],
            sender,
            seq_num: block_number as i32,
            sig: Bytes::new(),
            sig_algorithm: "test".to_string(),
            shard_id: "test".to_string(),
            extra_bytes: Bytes::new(),
            finalized_floor_certificate: None,
        }
    }

    fn slash(
        invalid_hash: BlockHash,
        issuer: Validator,
        target_activation_epoch: i64,
    ) -> ProcessedSystemDeploy {
        ProcessedSystemDeploy::Succeeded {
            event_list: Vec::new(),
            system_deploy: SystemDeployData::Slash {
                invalid_block_hash: invalid_hash,
                equivocation_block_hash: None,
                issuer_public_key: PublicKey::new(issuer),
                target_activation_epoch,
                target_bond_generation: models::rust::bond_generation::BondGeneration::GENESIS,
            },
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
        }
    }

    fn slash_authority(
        snapshot: &CasperSnapshot,
        bonds: HashMap<Validator, i64>,
    ) -> CanonicalSlashAuthority {
        CanonicalSlashAuthority::from_parts(
            Bytes::new(),
            bonds,
            snapshot.on_chain_state.bond_generations.clone(),
        )
        .expect("slash authority")
    }

    #[test]
    fn prior_epoch_invalid_justification_is_not_slash_obligating() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.on_chain_state.shard_conf.epoch_length = 10;
        let offender = validator(2);
        let proposer = validator(3);
        let invalid = hash(17);
        add_metadata(&mut snapshot, invalid.clone(), offender.clone(), 391, true);
        let block = candidate(4334, proposer, offender.clone(), invalid, Vec::new());
        let authority = slash_authority(&snapshot, HashMap::from([(offender, 1000)]));

        assert!(matches!(
            Validate::neglected_invalid_block(&block, &snapshot, &authority),
            Either::Right(ValidBlock::Valid)
        ));
    }

    #[test]
    fn current_non_evidence_rejection_rejects_the_child_without_slash_evidence() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.on_chain_state.shard_conf.epoch_length = 10;
        let offender = validator(2);
        let proposer = validator(3);
        let rejected = hash(18);
        add_metadata_with_reason(
            &mut snapshot,
            rejected.clone(),
            offender.clone(),
            94,
            Some(models::rust::block_metadata::AdmissionRejectionReason::InvalidSequenceNumber),
        );
        let block = candidate(95, proposer, offender.clone(), rejected, Vec::new());
        let authority = slash_authority(&snapshot, HashMap::from([(offender, 1000)]));

        assert!(matches!(
            Validate::neglected_invalid_block(&block, &snapshot, &authority),
            Either::Left(BlockError::Invalid(InvalidBlock::NeglectedInvalidBlock))
        ));
    }

    #[test]
    fn prior_generation_invalid_justification_does_not_obligate_rebonded_incarnation() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.on_chain_state.shard_conf.epoch_length = 10;
        let offender = validator(2);
        let proposer = validator(3);
        let invalid = hash(17);
        add_metadata(&mut snapshot, invalid.clone(), offender.clone(), 391, true);
        snapshot.on_chain_state.bond_generations.insert(
            offender.clone(),
            models::rust::bond_generation::BondGeneration::GENESIS
                .next()
                .expect("next bond generation"),
        );
        let block = candidate(4334, proposer, offender.clone(), invalid, Vec::new());
        let authority = slash_authority(&snapshot, HashMap::from([(offender, 1000)]));

        assert!(matches!(
            Validate::neglected_invalid_block(&block, &snapshot, &authority),
            Either::Right(ValidBlock::Valid)
        ));
    }

    #[test]
    fn current_invalid_justification_requires_matching_slash() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.on_chain_state.shard_conf.epoch_length = 10;
        let offender = validator(4);
        let proposer = validator(5);
        let invalid = hash(34);
        add_metadata(&mut snapshot, invalid.clone(), offender.clone(), 94, true);
        let unrelated = hash(51);
        add_metadata(&mut snapshot, unrelated.clone(), validator(6), 94, true);
        let authority = slash_authority(
            &snapshot,
            HashMap::from([(offender.clone(), 1000), (validator(6), 1000)]),
        );
        let block = candidate(
            95,
            proposer.clone(),
            offender.clone(),
            invalid.clone(),
            vec![slash(unrelated, proposer.clone(), 9)],
        );

        assert!(matches!(
            Validate::neglected_invalid_block(&block, &snapshot, &authority),
            Either::Left(BlockError::Invalid(InvalidBlock::NeglectedInvalidBlock))
        ));

        let block = candidate(95, proposer.clone(), offender, invalid.clone(), vec![
            slash(invalid, proposer, 9),
        ]);
        assert!(matches!(
            Validate::neglected_invalid_block(&block, &snapshot, &authority),
            Either::Right(ValidBlock::Valid)
        ));
    }

    #[test]
    fn same_block_unbond_cannot_erase_pre_state_slash_obligation() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.on_chain_state.shard_conf.epoch_length = 10;
        let offender = validator(7);
        let proposer = validator(8);
        let invalid = hash(68);
        add_metadata(&mut snapshot, invalid.clone(), offender.clone(), 94, true);
        let mut block = candidate(95, proposer, offender.clone(), invalid, Vec::new());
        block.body.state.bonds = vec![Bond {
            validator: offender.clone(),
            stake: 0,
        }];
        let authority = slash_authority(&snapshot, HashMap::from([(offender, 1000)]));

        assert!(matches!(
            Validate::neglected_invalid_block(&block, &snapshot, &authority),
            Either::Left(BlockError::Invalid(InvalidBlock::NeglectedInvalidBlock))
        ));
    }
}
