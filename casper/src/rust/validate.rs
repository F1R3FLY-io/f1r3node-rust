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
//! 1. `shard_identifier`, `chain_identifier` — the block belongs to this
//!    node's shard and chain.
//! 2. `EquivocationDetector::equivocation_verdict` — a second block of the
//!    sender at a held sequence number is an equivocation.
//! 3. `block_summary` — wire-format + parent + justification structural
//!    checks (T-1, T-2).
//! 4. `validate_block_checkpoint` — replay deploys against the pre-state
//!    hash and verify the resulting state matches the block's
//!    `post_state_hash`.
//! 5. `bonds_cache` — verify the block's bonds map matches the bonds
//!    computed from the parent post-state hash.
//! 6. `check_neglected_equivocations` — reject the block if both blocks of
//!    an equivocation are in its justifications and it leaves the
//!    equivocator bonded.
//! 7. `phlo_price` — minimum phlo-price check.
//!
//! ## Slashing-protocol position
//!
//! An equivocation verdict from step 2 is the only verdict stored: the
//! dispatcher (`dispatch_handle_invalid_block`) records the block invalid
//! together with its sibling.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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
    ApprovedBlock, BlockCommitment, BlockMessage, Bond, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;
use num_bigint::BigInt;
use prost::bytes::Bytes;
use prost::Message;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::dag::dag_ops;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::token_amount::TokenAmount;

use crate::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    CASPER_METRICS_SOURCE, REPEAT_DEPLOY_ANCESTOR_BODY_READS_METRIC,
    REPEAT_DEPLOY_ANCESTOR_METADATA_VISITS_METRIC, REPEAT_DEPLOY_ANCESTOR_SCAN_TIME_METRIC,
    REPEAT_DEPLOY_CARRIER_FALLBACK_SCAN_METRIC, REPEAT_DEPLOY_CARRIER_INDEX_ABSENCE_METRIC,
    REPEAT_DEPLOY_CARRIER_INDEX_HIT_METRIC, REPEAT_DEPLOY_CARRIER_INDEX_READ_FAILURE_METRIC,
    REPEAT_DEPLOY_CARRIER_PROBES_TIME_METRIC, REPEAT_DEPLOY_CARRIER_ROW_READS_METRIC,
    REPEAT_DEPLOY_CARRIER_WATERMARK_ENGAGED_METRIC,
    REPEAT_DEPLOY_CARRIER_WATERMARK_NOT_READY_METRIC, REPEAT_DEPLOY_CARRIER_WATERMARK_TIME_METRIC,
    REPEAT_DEPLOY_PARENTS_TIME_METRIC, REPEAT_DEPLOY_REJECTED_SIGS_TIME_METRIC,
    REPEAT_DEPLOY_RETRY_GATE_TIME_METRIC,
};
use crate::rust::slashing_authorization::validate_received_slash_deploys;
use crate::rust::system_deploy::is_system_deploy_id;
use crate::rust::util::proto_util;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::ValidBlockProcessing;

pub type PublicKey = Vec<u8>;
pub type Data = Vec<u8>;
pub type Signature = Vec<u8>;

/// Wall-clock tolerance of the timestamp validity rule; consensus-bearing,
/// so compiled.
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

fn sorted_bonds(bonds: &[Bond]) -> Vec<(&Validator, &TokenAmount)> {
    let mut sorted: Vec<(&Validator, &TokenAmount)> = bonds
        .iter()
        .map(|bond| (&bond.validator, &bond.stake))
        .collect();
    sorted.sort();
    sorted
}

impl Validate {
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

    pub fn approved_block(approved_block: &ApprovedBlock) -> bool {
        if matches!(
            Self::block_hash(&approved_block.candidate.block),
            Either::Left(_)
        ) {
            tracing::warn!(
                "Received invalid ApprovedBlock message: the candidate block does not match its block hash."
            );
            return false;
        }

        let candidate_bytes_digest =
            Blake2b256::hash(approved_block.clone().candidate.to_proto().encode_to_vec());
        let required_signatures = approved_block.candidate.required_sigs;

        let signatures: HashSet<Bytes> = approved_block
            .sigs
            .iter()
            .filter_map(|signature| {
                if Self::verify_signature(
                    &signature.algorithm,
                    &candidate_bytes_digest,
                    &signature.sig.to_vec(),
                    &signature.public_key.to_vec(),
                ) {
                    Some(signature.public_key.clone())
                } else {
                    None
                }
            })
            .collect();

        let log_msg = match signatures.is_empty() {
            true => "ApprovedBlock is self-signed by ceremony master.".to_string(),
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
        let enough_sigs = signatures.len() >= required_signatures as usize;

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

    /// Whether `commitment`'s sender signed `hash`, the hash it commits to.
    pub fn commitment_signature(commitment: &BlockCommitment, hash: &BlockHash) -> bool {
        Self::signature_algorithm_supported(&commitment.sig_algorithm)
            && Self::verify_signature(
                &commitment.sig_algorithm,
                &hash.to_vec(),
                &commitment.sig.to_vec(),
                &commitment.sender.to_vec(),
            )
    }

    pub fn block_hash_and_signature(b: &BlockMessage) -> Result<(), InvalidBlock> {
        if b.block_hash != proto_util::hash_block(b) {
            return Err(InvalidBlock::InvalidBlockHash);
        }
        if !Self::verify_signature(
            &b.sig_algorithm,
            &b.block_hash.to_vec(),
            &b.sig.to_vec(),
            &b.sender.to_vec(),
        ) {
            return Err(InvalidBlock::InvalidSignature);
        }
        Ok(())
    }

    pub fn historical_block(b: &BlockMessage) -> bool {
        if proto_util::block_number(b) == 0 {
            b.block_hash == proto_util::hash_block(b)
        } else {
            Self::block_hash_and_signature(b).is_ok()
        }
    }

    pub fn block_sender_has_weight(
        b: &BlockMessage,
        genesis: &BlockMessage,
        block_store: &KeyValueBlockStore,
    ) -> Result<bool, KvStoreError> {
        if b == genesis {
            return Ok(true);
        }
        // A parentless block that is not the genesis names no main parent, so
        // its sender is judged against the genesis bonds, never its own.
        let weight = if b.header.parents_hash_list.is_empty() {
            proto_util::weight_map(genesis)
                .get(&b.sender)
                .cloned()
                .unwrap_or_default()
        } else {
            proto_util::weight_from_sender(block_store, b)?
        };
        if weight > BigInt::from(0) {
            Ok(true)
        } else {
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    &format!(
                        "block creator {} has 0 weight.",
                        PrettyPrinter::build_string_bytes(&b.sender)
                    )
                )
            );
            Ok(false)
        }
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
        } else if !b
            .justifications
            .windows(2)
            .all(|pair| pair[0].validator < pair[1].validator)
        {
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    "block justifications are not in strictly ascending validator order."
                )
            );
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
        block_store: &KeyValueBlockStore,
        runtime_manager: &RuntimeManager,
        disable_validator_progress_check: bool,
        // Per-operation derivation slot, filled at the first step that
        // consumes the block's floor (transaction expiration) and reused by
        // the caller for the checkpoint and bonds steps. Filling lazily
        // keeps every earlier step's error order exactly as it was.
        floor_ctx_slot: &mut Option<crate::rust::finality::floor_context::FloorContext>,
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
        tracing::debug!(target: "f1r3fly.casper", "before-deploys-shard-identifier-validation");
        match __step!(
            BLOCK_VALIDATION_DEPLOYS_SHARD_IDENTIFIER_TIME_METRIC,
            Self::deploys_shard_identifier(block, shard_id)
        ) {
            Either::Left(err) => return Either::Left(err),
            Either::Right(_) => {}
        }
        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-validation");
        // Fill the floor slot here — the first consumer (the retry gate
        // reads the block's frozen floor). Only deploy-carrying blocks pay
        // the derivation; a derivation failure surfaces at this step as the
        // same BlockException class every step's storage failure uses.
        if floor_ctx_slot.is_none()
            && !block.body.deploys.is_empty()
            && !block.header.parents_hash_list.is_empty()
        {
            let latest_messages: BTreeMap<Validator, BlockHash> = block
                .justifications
                .iter()
                .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
                .collect();
            match crate::rust::finality::floor_context::FloorContext::derive(
                &s.dag,
                block_store,
                &block.header.parents_hash_list,
                &latest_messages,
                crate::rust::safety::clique_oracle::FtThreshold::from_ppm(
                    s.on_chain_state.chain.fault_tolerance_threshold_ppm,
                ),
            )
            .await
            {
                Ok(ctx) => *floor_ctx_slot = Some(ctx),
                // A derivation that needed history this node lacks is not a
                // verdict on the block; `from_validation_error` keeps it out of
                // the exception class that becomes a slashable InvalidTransaction.
                Err(ex) => return Either::Left(BlockError::from_validation_error(ex)),
            }
        }
        match __step!(
            BLOCK_VALIDATION_REPEAT_DEPLOY_TIME_METRIC,
            Self::repeat_deploy(
                block,
                s,
                block_store,
                expiration_threshold,
                floor_ctx_slot.as_ref()
            )
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
        tracing::debug!(target: "f1r3fly.casper", "before-slash-deploy-authorization-validation");
        match Self::slash_deploy_authorization(
            block,
            s,
            block_store,
            runtime_manager,
            floor_ctx_slot,
        )
        .await
        {
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
        // Fill the floor slot here — the first consumer. Only deploy-carrying
        // blocks pay the derivation; a derivation failure surfaces at this
        // step as the same BlockException class every step's storage failure
        // uses.
        if floor_ctx_slot.is_none()
            && !block.body.deploys.is_empty()
            && !block.header.parents_hash_list.is_empty()
        {
            let latest_messages: BTreeMap<Validator, BlockHash> = block
                .justifications
                .iter()
                .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
                .collect();
            match crate::rust::finality::floor_context::FloorContext::derive(
                &s.dag,
                block_store,
                &block.header.parents_hash_list,
                &latest_messages,
                crate::rust::safety::clique_oracle::FtThreshold::from_ppm(
                    s.on_chain_state.chain.fault_tolerance_threshold_ppm,
                ),
            )
            .await
            {
                Ok(ctx) => *floor_ctx_slot = Some(ctx),
                Err(ex) => {
                    return Either::Left(BlockError::from_validation_error(ex));
                }
            }
        }
        match __step!(
            BLOCK_VALIDATION_TRANSACTION_EXPIRATION_TIME_METRIC,
            Self::transaction_expiration(
                block,
                expiration_threshold,
                floor_ctx_slot.as_ref().map(|ctx| ctx.floor.block_number),
            )
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
        tracing::debug!(target: "f1r3fly.casper", "before-justification-follows-validation");
        match __step!(
            BLOCK_VALIDATION_JUSTIFICATION_FOLLOWS_TIME_METRIC,
            Self::justification_follows(block, block_store)
        ) {
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

    /// Validate no deploy with the same sig has been produced in the chain
    /// Agnostic of non-parent justifications.
    ///
    /// Recovery exemption, GATED on the block's frozen floor: a sig whose
    /// LATEST canonical disposition within the BLOCK'S OWN parent scope is
    /// a merge rejection is a legitimate recovery re-inclusion — but only
    /// once the retry gate opens (`FloorContext::retry_gate_open`: the
    /// latest kept rejection is settled in the block's floor closure).
    /// Re-inclusion against a live contest is `PrematureDeployRetry` —
    /// non-slashable, never admitted — because ungated re-proposal
    /// regenerated same-sig sibling copies faster than merges could
    /// adjudicate them and livelocked the shard under sustained contention.
    ///
    /// The exemption is a PURE FUNCTION OF THE BLOCK (its parents, the
    /// disposition records in their ancestry, and its justification-derived
    /// floor), never of the validating node's live view. An earlier version
    /// gated it on the sig's LOCAL finalization status and the validator's
    /// own `rejected_in_scope` snapshot set — both node-local: two honest
    /// validators whose finalization progress differed by one step returned
    /// opposite verdicts for the same block, forking the network (the
    /// roaming `InvalidRepeatDeploy` Heavy Pipeline failures).
    ///
    /// The double-execution defense is preserved deterministically: if a
    /// re-inclusion already WON above the rejection in the block's parent
    /// scope, the latest disposition is a win — not exempt — and the
    /// ancestor scan below finds the canonical inclusion and flags the
    /// repeat. A win that exists only on a fork OUTSIDE the block's parent
    /// scope must NOT poison this block: judged in its own context the
    /// re-inclusion is legal recovery, and the eventual merge's keep-one
    /// dedup reconciles the duplicate.
    ///
    /// `floor_ctx` is the operation's derivation context; `None` (the
    /// parentless shape, where no records are visible and no floor is
    /// derivable) grants no exemption at all — every sig goes through the
    /// ancestor scan unchanged.
    pub fn repeat_deploy(
        block: &BlockMessage,
        s: &mut CasperSnapshot,
        block_store: &KeyValueBlockStore,
        expiration_threshold: i32,
        floor_ctx: Option<&crate::rust::finality::floor_context::FloorContext>,
    ) -> ValidBlockProcessing {
        if block.body.deploys.is_empty() {
            return Either::Right(ValidBlock::Valid);
        }

        let block_metadata = BlockMetadata::from_block(block, None, None, None);

        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-get-parents");
        let parents_start = Instant::now();
        let parents_result = proto_util::get_parents_metadata(&s.dag, &block_metadata);
        metrics::histogram!(REPEAT_DEPLOY_PARENTS_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(parents_start.elapsed().as_secs_f64());
        let init_parents = match parents_result {
            Ok(parents) => parents,
            Err(e) => return Either::Left(BlockError::from_validation_error(e)),
        };

        // Calculate max block number and earliest acceptable block number
        let max_block_number = proto_util::max_block_number_metadata(&init_parents);
        let tip_scan_start = max_block_number + 1 - expiration_threshold as i64;
        let oldest_valid_after = block
            .body
            .deploys
            .iter()
            .map(|pd| pd.deploy.data.valid_after_block_number)
            .min();
        let earliest_block_number = match (floor_ctx, oldest_valid_after) {
            (Some(ctx), Some(oldest_valid_after)) => {
                match crate::rust::util::deploy_window::repeat_scan_start(
                    tip_scan_start,
                    ctx.floor.block_number,
                    expiration_threshold as i64,
                    oldest_valid_after,
                ) {
                    Ok(start) => start,
                    Err(e) => return Either::Left(BlockError::from_validation_error(e)),
                }
            }
            _ => tip_scan_start,
        };

        // Latest canonical dispositions within the block's parent scope,
        // over the same expiration window the ancestor scan uses, from the
        // operation's ONE record walk. The proposer's admission logic reads
        // the same walk on its side, so proposer and every validator
        // evaluate the same predicate on the same inputs.
        let mut exempt: HashSet<Bytes> = HashSet::new();
        if let Some(ctx) = floor_ctx {
            let rejected_start = Instant::now();
            let rejected_result = ctx.rejected_sigs(block_store, earliest_block_number);
            metrics::histogram!(REPEAT_DEPLOY_REJECTED_SIGS_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                .record(rejected_start.elapsed().as_secs_f64());
            let rejected = match rejected_result {
                Ok(sigs) => sigs,
                Err(e) => return Either::Left(BlockError::from_validation_error(e)),
            };
            for pd in &block.body.deploys {
                let sig = &pd.deploy.sig;
                if !rejected.contains(sig) {
                    continue;
                }
                let retry_start = Instant::now();
                let retry_result =
                    ctx.retry_gate_open(&s.dag, block_store, earliest_block_number, sig);
                metrics::histogram!(REPEAT_DEPLOY_RETRY_GATE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(retry_start.elapsed().as_secs_f64());
                match retry_result {
                    Ok(true) => {
                        exempt.insert(sig.clone());
                    }
                    Ok(false) => {
                        tracing::warn!(
                            "{}",
                            Self::ignore(
                                block,
                                &format!(
                                    "deploy {} re-includes a rejected sig whose latest kept \
                                     rejection is not settled in the block's floor (#{}) — \
                                     premature retry",
                                    hex::encode(&sig[..8.min(sig.len())]),
                                    ctx.floor.block_number,
                                )
                            )
                        );
                        return Either::Left(BlockError::Invalid(
                            InvalidBlock::PrematureDeployRetry,
                        ));
                    }
                    Err(e) => return Either::Left(BlockError::from_validation_error(e)),
                }
            }
        }

        let deploy_key_set: HashSet<Vec<u8>> = block
            .body
            .deploys
            .iter()
            .filter(|pd| !exempt.contains(&pd.deploy.sig))
            .map(|pd| pd.deploy.sig.to_vec())
            .collect();
        if deploy_key_set.is_empty() {
            return Either::Right(ValidBlock::Valid);
        }

        // Repeat-deploy carrier-index fast path (CONSENSUS_PHILOSOPHY
        // §4.4). The index records every carrier from the watermark W
        // onward, so it engages only when the scan window starts at or
        // above W (`w <= max(earliest, 0)` — heights below zero do not
        // exist, so W = 0 means complete over every existing block).
        // Behind that gate, an index absence proves the sig has no
        // in-window carrier, so it cannot be a repeat and skips the scan.
        // An index hit is NOT a verdict — the sig stays in the exact scan,
        // which is the window and parent-scope verification (a fork-only
        // carrier must not poison this block). Any read failure keeps the
        // sig in the scan: unreadable index state is no information, never
        // an absence proof. Pruning follows the node's own LFB, so a window
        // can also start below the prune cutoff (a deep start, or a late
        // block's tip window); the index engages only at or above both
        // bounds. A prune walk runs concurrently and records its cutoff
        // before removing rows, so the cutoff is re-read after probing and a
        // moved cutoff discards the absences.
        let index_read_failures = metrics::counter!(
            REPEAT_DEPLOY_CARRIER_INDEX_READ_FAILURE_METRIC,
            "source" => CASPER_METRICS_SOURCE
        );
        let index_start = earliest_block_number.max(0);
        let covers_window = |cutoff: Option<i64>| cutoff.is_none_or(|c| c <= index_start);
        let watermark_start = Instant::now();
        let index_bounds = s
            .dag
            .carrier_index_watermark()
            .and_then(|w| Ok((w, s.dag.carrier_index_prune_cutoff()?)));
        metrics::histogram!(REPEAT_DEPLOY_CARRIER_WATERMARK_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(watermark_start.elapsed().as_secs_f64());
        let deploy_key_set: HashSet<Vec<u8>> = match index_bounds {
            Ok((Some(w), cutoff)) if w <= index_start && covers_window(cutoff) => {
                metrics::counter!(REPEAT_DEPLOY_CARRIER_WATERMARK_ENGAGED_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .increment(1);
                let row_reads = metrics::counter!(
                    REPEAT_DEPLOY_CARRIER_ROW_READS_METRIC,
                    "source" => CASPER_METRICS_SOURCE
                );
                let index_absences = metrics::counter!(
                    REPEAT_DEPLOY_CARRIER_INDEX_ABSENCE_METRIC,
                    "source" => CASPER_METRICS_SOURCE
                );
                let index_hits = metrics::counter!(
                    REPEAT_DEPLOY_CARRIER_INDEX_HIT_METRIC,
                    "source" => CASPER_METRICS_SOURCE
                );
                let probes_start = Instant::now();
                let mut probe_failed = false;
                let scan_set: HashSet<Vec<u8>> = deploy_key_set
                    .iter()
                    .filter(|sig| {
                        if probe_failed {
                            return true;
                        }
                        row_reads.increment(1);
                        match s.dag.carrier_index_proves_absence(sig) {
                            Ok(true) => {
                                index_absences.increment(1);
                                false
                            }
                            Ok(false) => {
                                index_hits.increment(1);
                                true
                            }
                            Err(e) => {
                                index_read_failures.increment(1);
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
                    .cloned()
                    .collect();
                metrics::histogram!(REPEAT_DEPLOY_CARRIER_PROBES_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(probes_start.elapsed().as_secs_f64());
                match s.dag.carrier_index_prune_cutoff() {
                    Ok(cutoff) if covers_window(cutoff) => scan_set,
                    Ok(cutoff) => {
                        tracing::debug!(
                            target: "f1r3fly.casper",
                            "repeat-deploy carrier-index prune cutoff moved to {:?} above scan start {} \
                             while probing block {}; falling back to the ancestor scan",
                            cutoff,
                            index_start,
                            PrettyPrinter::build_string_bytes(&block.block_hash),
                        );
                        deploy_key_set
                    }
                    Err(e) => {
                        index_read_failures.increment(1);
                        tracing::warn!(
                            "repeat-deploy carrier-index prune cutoff read failed for block {}; \
                             falling back to the ancestor scan: {}",
                            PrettyPrinter::build_string_bytes(&block.block_hash),
                            e,
                        );
                        deploy_key_set
                    }
                }
            }
            Ok(_) => {
                metrics::counter!(REPEAT_DEPLOY_CARRIER_WATERMARK_NOT_READY_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .increment(1);
                deploy_key_set
            }
            Err(e) => {
                index_read_failures.increment(1);
                tracing::warn!(
                    "repeat-deploy carrier-index bounds read failed for block {}; \
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
        metrics::counter!(REPEAT_DEPLOY_CARRIER_FALLBACK_SCAN_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(1);

        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-duplicate-block");
        // A failed expansion is not an empty one: swallowing it ends the scan
        // early, and "found no duplicate" is then a statement about what could
        // be read, not about the chain. That verdict admits the repeat.
        let ancestor_metadata_visits = metrics::counter!(
            REPEAT_DEPLOY_ANCESTOR_METADATA_VISITS_METRIC,
            "source" => CASPER_METRICS_SOURCE
        );
        let ancestor_body_reads = metrics::counter!(
            REPEAT_DEPLOY_ANCESTOR_BODY_READS_METRIC,
            "source" => CASPER_METRICS_SOURCE
        );
        let scan_start = Instant::now();
        let scan_result = proto_util::walk_blocks_in_scope(
            &s.dag,
            init_parents,
            earliest_block_number,
            proto_util::UnheldParent::Surface,
            |block_hash| {
                ancestor_body_reads.increment(1);
                let found = block_store
                    .has_any_deploy_sig_strict(block_hash, &deploy_key_set)
                    .map_err(CasperError::from)?;
                if !found {
                    ancestor_metadata_visits.increment(1);
                }
                Ok(found)
            },
        );
        metrics::histogram!(REPEAT_DEPLOY_ANCESTOR_SCAN_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(scan_start.elapsed().as_secs_f64());
        let maybe_duplicated_block_hash = match scan_result {
            Ok(found) => found,
            Err(e) => return Either::Left(BlockError::from_validation_error(e)),
        };

        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-duplicate-block-log");
        let maybe_error = maybe_duplicated_block_hash.map(|duplicated_block_hash| {
      let duplicated_block = block_store.get_unsafe(&duplicated_block_hash);
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
      let duplicated_deploy = match duplicated_deploys
        .iter()
        .map(|processed_deploy| &processed_deploy.deploy)
        .find(|deploy| deploy_key_set.contains(deploy.sig.as_ref()))
      {
        Some(d) => d,
        None => {
          tracing::error!(
            "InvalidRepeatDeploy duplicate-deploy invariant violated: deploy-index claims block {} carries a deploy whose signature collides with current block {}, but no such deploy exists in that block's deploy list",
            block_hash_string,
            current_block_hash_string
          );
          return BlockError::from_validation_error(CasperError::RuntimeError(format!(
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
        "found deploy [{}] (user {}, millisecond timestamp {})] with the same sig in the block {} as current block {}",
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
                return Either::Left(BlockError::from_validation_error(CasperError::from(e)));
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
        let parent_numbers: Vec<i64> = match proto_util::parent_hashes(b)
            .iter()
            .map(|parent_hash| {
                s.dag.block_number(parent_hash)?.ok_or_else(|| {
                    KvStoreError::KeyNotFound(format!(
                        "Block dag store was missing {}",
                        PrettyPrinter::build_string_bytes(parent_hash)
                    ))
                })
            })
            .collect::<Result<Vec<i64>, KvStoreError>>()
        {
            Ok(parent_numbers) => parent_numbers,
            Err(e) => return Either::Left(BlockError::from_validation_error(CasperError::from(e))),
        };

        let max_block_number = parent_numbers
            .iter()
            .fold(-1, |acc, number| acc.max(*number));

        let number = proto_util::block_number(b);
        let result = max_block_number + 1 == number;

        if result {
            Either::Right(ValidBlock::Valid)
        } else {
            let log_message = if parent_numbers.is_empty() {
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

    /// Expiry validity reads the block's own frozen floor when one is
    /// derivable — the same clock the merge window rule and the buffer
    /// retain close windows on, so honest floor-clock retries stay valid
    /// under floor lag. A block with no derivable floor (parentless, or no
    /// deploys to judge) falls back to its own block number, which is never
    /// looser than the floor's bound.
    pub fn transaction_expiration(
        b: &BlockMessage,
        expiration_threshold: i32,
        block_floor_number: Option<i64>,
    ) -> ValidBlockProcessing {
        let earliest_acceptable_valid_after_block_number = block_floor_number
            .unwrap_or_else(|| proto_util::block_number(b))
            - expiration_threshold as i64;

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
    /// expiration_timestamp field. A deploy is time-expired if its expiration_timestamp is
    /// set (> 0) and the block's timestamp exceeds the expiration_timestamp.
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
                Some(justification) => match s.dag.lookup(&justification.latest_block_hash) {
                    Ok(Some(block_metadata)) => block_metadata.sequence_number as i64,
                    Ok(None) => {
                        return Either::Left(BlockError::from_validation_error(CasperError::from(
                            KvStoreError::KeyNotFound(format!(
                                "Latest block hash {} is missing from block dag store.",
                                PrettyPrinter::build_string_bytes(&justification.latest_block_hash)
                            )),
                        )));
                    }
                    Err(e) => {
                        return Either::Left(BlockError::from_validation_error(CasperError::from(
                            e,
                        )));
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

    pub fn chain_identifier(b: &BlockMessage, chain_id: &BlockHash) -> ValidBlockProcessing {
        if b.header.chain_id == *chain_id {
            Either::Right(ValidBlock::Valid)
        } else {
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    &format!(
                        "got chain identifier {} while {} was expected.",
                        hex::encode(&b.header.chain_id),
                        hex::encode(chain_id)
                    )
                )
            );
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidChainId))
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
        disable_validator_progress_check: bool,
    ) -> ValidBlockProcessing {
        // Check if block contains user deploys (non-system deploys)
        let has_user_deploys = b
            .body
            .deploys
            .iter()
            .any(|pd| !is_system_deploy_id(&pd.deploy.sig));
        // Whether the block's own content settles this rule, independent of
        // its parents. Classified in one exhaustive place so that a system
        // deploy added later cannot land on the wrong side of it unnoticed.
        let content_shows_progress = crate::rust::system_deploy::deploy_content_shows_progress(
            has_user_deploys,
            &b.body.system_deploys,
        );

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
        // (depth from the block's highest parent > max_parent_depth). An honest proposer
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
            // ANCHOR: the block's OWN parent frontier, never this node's tip.
            // The proposer keeps only parents within a `max_parent_depth` span
            // around its main parent (`snapshot.rs`), so the declared parents
            // satisfy this pairwise rule by construction.
            //
            // Reading `latest_block_number()` here instead made a validity
            // verdict depend on how far ahead the validating node happened to
            // be: two nodes at different heights could reach different verdicts
            // on the same block, and a node catching up (or joined by LFS)
            // rejected history that was perfectly legal when it was produced.
            // `depth_buffer` existed only to absorb that skew, and the skew it
            // absorbed was unbounded, so it is gone with the anchor.
            let mut parent_numbers: Vec<(BlockHash, i64)> = Vec::with_capacity(parent_hashes.len());
            let mut frontier_known = true;
            for parent_hash in &parent_hashes {
                match s.dag.lookup(parent_hash) {
                    Ok(Some(meta)) => parent_numbers.push((parent_hash.clone(), meta.block_number)),
                    // A parent with no metadata leaves the frontier unknown, and
                    // an anchor derived from a partial parent set could reject a
                    // legal block. Skip the check and let the dependency gate
                    // handle the missing parent.
                    Ok(None) => {
                        frontier_known = false;
                        break;
                    }
                    Err(error) => {
                        return Either::Left(BlockError::from_validation_error(error.into()))
                    }
                }
            }
            if frontier_known {
                // Stated as what the rule actually is — no two of this block's
                // parents may spread more than `max_parent_depth` apart — rather
                // than as a comparison against a derived maximum. There is then
                // no "highest parent" to be absent, so no default to invent: an
                // empty parent set simply has no pair that violates it, which is
                // the right answer. The pass is quadratic in the parent count,
                // which `max_number_of_parents` has already bounded above.
                for (deep_hash, deep_number) in &parent_numbers {
                    if deep_hash == &genesis.block_hash {
                        continue; // genesis exempt
                    }
                    for (_, other_number) in &parent_numbers {
                        let spread = *other_number - *deep_number;
                        if spread > max_parent_depth as i64 {
                            let message = format!(
                                "parent {} at block_number {} is {} below the block's parent \
                                 at block_number {} (exceeds max_parent_depth = {})",
                                PrettyPrinter::build_string_bytes(deep_hash),
                                deep_number,
                                spread,
                                other_number,
                                max_parent_depth
                            );
                            tracing::warn!("{}", Self::ignore(b, &message));
                            return Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents));
                        }
                    }
                }
            }
        }

        let validator = &b.sender;

        // Get validator's previous block (if any)
        let prev_block_hash_opt = s.dag.latest_message_hash(validator);

        match prev_block_hash_opt {
            // First block from this validator - always valid
            None => Either::Right(ValidBlock::Valid),

            // Validator has previous blocks - check progress requirement
            Some(prev_block_hash) => {
                // Get previous block metadata
                let prev_block_meta = match s.dag.lookup(&prev_block_hash) {
                    Ok(Some(meta)) => meta,
                    Ok(None) => {
                        return Either::Left(BlockError::from_validation_error(CasperError::from(
                            KvStoreError::KeyNotFound(format!(
                                "Previous block {} not found in DAG",
                                PrettyPrinter::build_string_bytes(&prev_block_hash)
                            )),
                        )));
                    }
                    Err(e) => {
                        return Either::Left(BlockError::from_validation_error(CasperError::from(
                            e,
                        )));
                    }
                };

                // Special case: if previous block is genesis (no parents), allow proposal
                // This breaks the deadlock after genesis ceremony when all validators are at genesis
                let is_genesis = prev_block_meta.parents.is_empty();

                // BFS the ancestor closure of the previous block, bounded by
                // HEIGHT rather than by finality.
                //
                // The bound exists only to stop the traversal running the length
                // of the chain — the original stopped at finalized blocks for
                // that reason alone, and finality carries no meaning in this
                // rule. But `is_finalized` is node-local state, so it made a
                // VALIDITY verdict depend on how much the validating node had
                // finalized: a node holding fewer markers walks deeper, sees a
                // larger closure, finds fewer parents "new", and can reject a
                // block its peers accepted. The node that is behind is the strict
                // one, so catching up rejects legal history.
                //
                // The lowest declared parent is the exact bound. Ancestry is
                // strictly height-decreasing, so every path from the previous
                // block down to a parent stays at or above that parent's height;
                // nothing below the lowest parent can BE a parent, and no path to
                // a parent passes through it. Truncating there is therefore
                // lossless, not merely conservative. Seeded from the block's own
                // number — every parent is below it — so there is no empty case
                // and no default.
                let mut lowest_parent_height = b.body.state.block_number;
                for parent_hash in &parent_hashes {
                    match s.dag.lookup(parent_hash) {
                        Ok(Some(meta)) => {
                            lowest_parent_height = lowest_parent_height.min(meta.block_number)
                        }
                        Ok(None) => {}
                        Err(error) => {
                            return Either::Left(BlockError::from_validation_error(error.into()))
                        }
                    }
                }
                let ancestor_hashes: Vec<BlockHash> =
                    match dag_ops::try_bf_traverse(vec![prev_block_hash.clone()], |hash| {
                        Ok::<_, shared::rust::store::key_value_store::KvStoreError>(
                            match s.dag.lookup(hash)? {
                                Some(meta) if meta.block_number >= lowest_parent_height => {
                                    meta.parents.clone()
                                }
                                _ => vec![],
                            },
                        )
                    }) {
                        Ok(hashes) => hashes,
                        Err(error) => {
                            return Either::Left(BlockError::from_validation_error(error.into()))
                        }
                    };
                let ancestor_set: HashSet<BlockHash> = ancestor_hashes.into_iter().collect();

                // Check if at least one parent is new (not in ancestor closure)
                let has_new_parent = parent_hashes.iter().any(|p| !ancestor_set.contains(p));
                // Validation logic:
                // - Blocks whose content shows progress: always valid — paying
                //   users served, a consensus action landed, or the well-formed
                //   empty block the heartbeat uses to break a stale frontier
                // - Everything else: must cite a parent the proposer had not
                //   already seen
                // - disable_validator_progress_check: skip progress check (for standalone mode)
                if content_shows_progress
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
    pub fn justification_follows(
        b: &BlockMessage,
        block_store: &KeyValueBlockStore,
    ) -> ValidBlockProcessing {
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

        let justified_validators: HashSet<Bytes> = b
            .justifications
            .iter()
            .map(|justification| justification.validator.clone())
            .collect();

        let parent_hashes = proto_util::parent_hashes(b);
        let main_parent_hash = match parent_hashes.first() {
            Some(hash) => hash,
            None => return Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents)),
        };

        let main_parent = block_store.get_unsafe(main_parent_hash);
        let bonded_validators: HashSet<Bytes> = proto_util::bonds(&main_parent)
            .iter()
            .map(|bond| bond.validator.clone())
            .collect();

        if bonded_validators == justified_validators {
            Either::Right(ValidBlock::Valid)
        } else {
            let justified_validators_pp: HashSet<String> = justified_validators
                .iter()
                .map(|validator| PrettyPrinter::build_string_bytes(validator))
                .collect();
            let bonded_validators_pp: HashSet<String> = bonded_validators
                .iter()
                .map(|validator| PrettyPrinter::build_string_bytes(validator))
                .collect();

            let message = format!(
                "the justified validators, {:?}, do not match the bonded validators, {:?}.",
                justified_validators_pp, bonded_validators_pp
            );

            tracing::warn!("{}", Self::ignore(b, &message));
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidFollows))
        }
    }

    /// Judges the block's `Slash` system deploys against the stake a slash
    /// can take at the block's floor: bonds and stake in quarantine. The
    /// floor depends on the block alone, so every node reaches the same
    /// verdict; a block without parents slashes nobody.
    pub async fn slash_deploy_authorization(
        block: &BlockMessage,
        s: &CasperSnapshot,
        block_store: &KeyValueBlockStore,
        runtime_manager: &RuntimeManager,
        floor_ctx_slot: &mut Option<crate::rust::finality::floor_context::FloorContext>,
    ) -> ValidBlockProcessing {
        let carries_slash = block.body.system_deploys.iter().any(|deploy| {
            matches!(deploy, ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::Slash { .. },
                ..
            })
        });
        if !carries_slash {
            return Either::Right(ValidBlock::Valid);
        }
        if floor_ctx_slot.is_none() && !block.header.parents_hash_list.is_empty() {
            let latest_messages: BTreeMap<Validator, BlockHash> = block
                .justifications
                .iter()
                .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
                .collect();
            match crate::rust::finality::floor_context::FloorContext::derive(
                &s.dag,
                block_store,
                &block.header.parents_hash_list,
                &latest_messages,
                crate::rust::safety::clique_oracle::FtThreshold::from_ppm(
                    s.on_chain_state.chain.fault_tolerance_threshold_ppm,
                ),
            )
            .await
            {
                Ok(ctx) => *floor_ctx_slot = Some(ctx),
                Err(ex) => return Either::Left(BlockError::from_validation_error(ex)),
            }
        }
        let slashable = match floor_ctx_slot.as_ref() {
            Some(ctx) => match runtime_manager.slashable_stakes(&ctx.floor_state).await {
                Ok(slashable) => slashable,
                Err(ex) => return Either::Left(BlockError::from_validation_error(ex)),
            },
            None => HashMap::new(),
        };
        Self::route_slash_validation_outcome(
            block,
            validate_received_slash_deploys(
                block,
                &slashable,
                &s.on_chain_state.shard_conf.shard_name,
                &s.chain_id,
            ),
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
                Either::Left(BlockError::from_validation_error(infra_err))
            }
        }
    }

    /// Justification regression check.
    ///
    /// Compares the justifications `b`'s own previous block cites for other
    /// validators against those `b` cites, and rejects any that move back to a
    /// lower sequence number. Both blocks are signed and `b` cites the
    /// previous one, so every node holding `b` reaches the same verdict. The
    /// sender's own entry is not compared: the equivocation check judges it.
    pub fn justification_regressions(
        b: &BlockMessage,
        s: &mut CasperSnapshot,
    ) -> ValidBlockProcessing {
        let previous = proto_util::creator_justification_block_message(b)
            .map(|justification| s.dag.lookup_unsafe(&justification.latest_block_hash))
            .transpose();
        match previous {
            Ok(None) => Either::Right(ValidBlock::Valid),
            Ok(Some(previous)) => {
                let new_lms = proto_util::to_latest_message_hashes(&b.justifications);
                let cur_lms = proto_util::to_latest_message_hashes(&previous.justifications);

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
                    if *sender == b.sender {
                        continue;
                    }
                    let Some(cur_justification_hash) = cur_lms.get(sender) else {
                        continue;
                    };

                    let new_justification = match s.dag.lookup_unsafe(new_justification_hash) {
                        Ok(metadata) => metadata,
                        Err(e) => {
                            return Either::Left(BlockError::from_validation_error(
                                CasperError::from(e),
                            ))
                        }
                    };
                    let cur_justification = match s.dag.lookup_unsafe(cur_justification_hash) {
                        Ok(metadata) => metadata,
                        Err(e) => {
                            return Either::Left(BlockError::from_validation_error(
                                CasperError::from(e),
                            ))
                        }
                    };

                    if new_justification.sequence_number < cur_justification.sequence_number {
                        log_warn(cur_justification_hash, new_justification_hash, sender);
                        return Either::Left(BlockError::Invalid(
                            InvalidBlock::JustificationRegression,
                        ));
                    }
                }

                Either::Right(ValidBlock::Valid)
            }
            Err(e) => Either::Left(BlockError::from_validation_error(CasperError::from(e))),
        }
    }

    /// Every vote names a claim open at the block's pre-state that the
    /// proposer may vote on and has not voted on before, and the block votes
    /// on as many of those claims as the vote list bound allows, each nay on
    /// an overflow claim carrying what the block's floor records.
    /// The block resolves exactly the claims open at its pre-state whose
    /// tally crossed at its floor, and expires exactly the graces of its
    /// pre-state that its floor has passed and the claims whose deadline its
    /// floor has reached, leaving out every one naming a validator the block
    /// slashes.
    pub async fn leaky_bucket_claims(
        b: &BlockMessage,
        runtime_manager: &RuntimeManager,
        votes: &crate::rust::economics::claims::VoteHistory<'_>,
        floor: &crate::rust::economics::claims::FloorView,
        params: &crate::rust::economics::bucket_view::LeakyBucketParams,
    ) -> ValidBlockProcessing {
        let book = match runtime_manager
            .claim_book(&b.body.state.pre_state_hash)
            .await
        {
            Ok(book) => book,
            Err(error) => return Either::Left(BlockError::from_validation_error(error)),
        };
        let records = &book.records;
        let last_own_block = proto_util::creator_justification_block_message(b)
            .map(|justification| justification.latest_block_hash);
        let voter = crate::rust::economics::claims::Voter {
            validator: &b.sender,
            last_own_block: last_own_block.as_ref(),
            block_number: b.body.state.block_number,
        };
        let owed = match votes.owed(records, &voter, params.claim_deadline_blocks) {
            Ok(owed) => owed,
            Err(error) => return Either::Left(BlockError::from_validation_error(error)),
        };

        let required = owed.len().min(params.vote_list_bound.get() as usize);
        let owed_claims: HashMap<_, _> = owed
            .iter()
            .take(required)
            .map(|record| (&record.claim, *record))
            .collect();
        let mut voted = HashSet::new();
        for vote in &b.body.leaky_bucket_votes {
            let Some(record) = owed_claims.get(&vote.claim) else {
                tracing::warn!(
                    "{}",
                    Self::ignore(
                        b,
                        &format!(
                            "vote on a claim that is not among the oldest it owes: {:?}",
                            vote.claim
                        )
                    )
                );
                return Either::Left(BlockError::Invalid(InvalidBlock::InvalidLeakyBucketVote));
            };
            if !voted.insert(&vote.claim) {
                tracing::warn!(
                    "{}",
                    Self::ignore(b, &format!("votes twice on {:?}", vote.claim))
                );
                return Either::Left(BlockError::Invalid(InvalidBlock::InvalidLeakyBucketVote));
            }
            let required = crate::rust::economics::claims::testimony(record, vote.yea, floor);
            if vote.testimony != required {
                tracing::warn!(
                    "{}",
                    Self::ignore(
                        b,
                        &format!(
                            "vote on {:?} carries testimony {:?} where its floor requires {:?}",
                            vote.claim, vote.testimony, required
                        )
                    )
                );
                return Either::Left(BlockError::Invalid(InvalidBlock::InvalidLeakyBucketVote));
            }
        }
        if b.body.leaky_bucket_votes.len() != required {
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    &format!(
                        "carries {} votes where it owes {}",
                        b.body.leaky_bucket_votes.len(),
                        required
                    )
                )
            );
            return Either::Left(BlockError::Invalid(InvalidBlock::MissingLeakyBucketVotes));
        }

        let slashed: HashSet<Validator> = b
            .body
            .system_deploys
            .iter()
            .filter_map(|deploy| match deploy {
                ProcessedSystemDeploy::Succeeded {
                    system_deploy: SystemDeployData::Slash { evidence, .. },
                    ..
                } => Some(evidence.first.sender.clone()),
                _ => None,
            })
            .collect();
        let resolvable = book.without(&slashed);
        let required = match votes.resolutions(&resolvable.records, floor) {
            Ok(required) => required,
            Err(error) => return Either::Left(BlockError::from_validation_error(error)),
        };
        let required_grace_expiries = resolvable.grace_expiries(floor.number);
        let required_expiries =
            resolvable.expiries(floor.number, params.claim_deadline_blocks, &required);
        let nothing_required = required.is_empty()
            && required_grace_expiries.is_empty()
            && required_expiries.is_empty();
        let carried: Vec<&SystemDeployData> = b
            .body
            .system_deploys
            .iter()
            .filter_map(|deploy| match deploy {
                ProcessedSystemDeploy::Succeeded {
                    system_deploy: data @ SystemDeployData::ResolveClaims { .. },
                    ..
                } => Some(data),
                _ => None,
            })
            .collect();
        let exact = match carried.as_slice() {
            [] => nothing_required,
            [SystemDeployData::ResolveClaims {
                resolutions,
                grace_expiries,
                expiries,
                floor_hash,
                floor_number,
            }] => {
                !nothing_required
                    && *resolutions == required
                    && *grace_expiries == required_grace_expiries
                    && *expiries == required_expiries
                    && *floor_hash == floor.hash
                    && *floor_number == floor.number
            }
            _ => false,
        };
        if !exact {
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    &format!(
                        "resolves {:?} where its floor requires {:?}, grace expiries {:?} \
                         and claim expiries {:?}",
                        carried, required, required_grace_expiries, required_expiries
                    )
                )
            );
            return Either::Left(BlockError::Invalid(InvalidBlock::InvalidClaimResolution));
        }

        Either::Right(ValidBlock::Valid)
    }

    /// Every dishonest-vote deploy the block carries names a different
    /// accused, and its evidence proves the nay dishonest against the block's
    /// floor.
    pub fn dishonest_votes(
        b: &BlockMessage,
        dag: &block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        floor_number: i64,
        reach: Option<i64>,
        reward: &crate::rust::economics::reward::RewardParams,
    ) -> ValidBlockProcessing {
        let mut accused = HashSet::new();
        for deploy in &b.body.system_deploys {
            let ProcessedSystemDeploy::Succeeded {
                system_deploy:
                    SystemDeployData::DishonestVote {
                        accused: target,
                        vote_block,
                        claim,
                    },
                ..
            } = deploy
            else {
                continue;
            };
            let accusation = crate::rust::economics::dishonest_vote::Accusation {
                accused: target.clone(),
                vote_block: vote_block.clone(),
                claim: claim.clone(),
            };
            let proven = match crate::rust::economics::dishonest_vote::proves_dishonesty(
                dag,
                block_store,
                &accusation,
                floor_number,
                reach,
                reward,
            ) {
                Ok(proven) => proven,
                Err(error) => return Either::Left(BlockError::from_validation_error(error)),
            };
            if !accused.insert(target) || !proven {
                tracing::warn!(
                    "{}",
                    Self::ignore(
                        b,
                        &format!("carries evidence that proves nothing: {accusation:?}")
                    )
                );
                return Either::Left(BlockError::Invalid(InvalidBlock::InvalidDishonestVote));
            }
        }
        Either::Right(ValidBlock::Valid)
    }

    pub async fn bonds_cache_from_floor(
        b: &BlockMessage,
        block_store: &KeyValueBlockStore,
        snapshot: &CasperSnapshot,
        runtime_manager: &RuntimeManager,
        floor_ctx: Option<&crate::rust::finality::floor_context::FloorContext>,
    ) -> ValidBlockProcessing {
        let parent_hashes = b.header.parents_hash_list.clone();
        if parent_hashes.is_empty() {
            return Self::bonds_cache(b, runtime_manager).await;
        }

        let floor_state = match floor_ctx {
            Some(ctx) => ctx.floor_state.clone(),
            None => {
                let latest_messages: BTreeMap<Validator, BlockHash> = b
                    .justifications
                    .iter()
                    .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
                    .collect();
                let floor = match crate::rust::finality::floor::finalized_floor(
                    &snapshot.dag,
                    block_store,
                    &parent_hashes,
                    &latest_messages,
                    crate::rust::safety::clique_oracle::FtThreshold::from_ppm(
                        snapshot.on_chain_state.chain.fault_tolerance_threshold_ppm,
                    ),
                )
                .await
                {
                    Ok(floor) => floor,
                    Err(ex) => {
                        tracing::warn!("Failed to derive finalized floor for bonds cache: {}", ex);
                        return Either::Left(BlockError::from_validation_error(ex));
                    }
                };
                let floor_block = match block_store.get(&floor.hash) {
                    Ok(Some(block)) => block,
                    Ok(None) => {
                        let err = CasperError::RuntimeError(format!(
                            "finalized-floor block {} not in block store for bonds cache",
                            PrettyPrinter::build_string_bytes(&floor.hash)
                        ));
                        tracing::warn!("{}", err);
                        return Either::Left(BlockError::from_validation_error(err));
                    }
                    Err(ex) => {
                        tracing::warn!(
                            "Failed to read finalized-floor block for bonds cache: {}",
                            ex
                        );
                        return Either::Left(BlockError::from_validation_error(ex.into()));
                    }
                };
                proto_util::post_state_hash(&floor_block)
            }
        };

        // Same shared helper the proposer uses to package `block.bonds`, called on
        // the same floor state ⇒ the recomputed committee is identical by
        // construction (PLAY≡REPLAY); a mismatching block-bonds set is rejected.
        match crate::rust::finality::floor::floor_committee(runtime_manager, &floor_state).await {
            Ok(committee) => {
                // Compared as sorted lists, not sets: a block listing one
                // validator twice would otherwise pass, and the reward would be
                // computed over a committee the floor does not hold.
                if sorted_bonds(&proto_util::bonds(b)) == sorted_bonds(&committee) {
                    Either::Right(ValidBlock::Valid)
                } else {
                    tracing::warn!(
                        "Bonds in finalized-floor proof of stake contract do not match block's bond cache."
                    );
                    Either::Left(BlockError::Invalid(InvalidBlock::InvalidBondsCache))
                }
            }
            Err(ex) => {
                tracing::warn!("Failed to compute bonds from finalized-floor state: {}", ex);
                Either::Left(BlockError::from_validation_error(ex))
            }
        }
    }

    pub async fn bonds_cache(
        b: &BlockMessage,
        runtime_manager: &RuntimeManager,
    ) -> ValidBlockProcessing {
        let bonds = proto_util::bonds(b);
        let tuplespace_hash = proto_util::post_state_hash(b);

        match runtime_manager.compute_bonds(&tuplespace_hash).await {
            Ok(computed_bonds) => {
                if sorted_bonds(&bonds) == sorted_bonds(&computed_bonds) {
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
                Either::Left(BlockError::from_validation_error(ex))
            }
        }
    }

    /// Every deploy pays at least minPhloPrice.
    pub fn phlo_price(b: &BlockMessage, min_phlo_price: &TokenAmount) -> ValidBlockProcessing {
        if b.body.deploys.iter().all(|deploy| {
            deploy.deploy.data.validate_amounts().is_ok()
                && deploy.deploy.data.phlo_price >= *min_phlo_price
        }) {
            Either::Right(ValidBlock::Valid)
        } else {
            Either::Left(BlockError::Invalid(InvalidBlock::LowDeployCost))
        }
    }
}

#[cfg(test)]
mod justification_order_tests {
    use models::rust::block_implicits::get_random_block_default;
    use models::rust::casper::protocol::casper_message::Justification;

    use super::*;

    #[test]
    fn format_of_fields_requires_strictly_ascending_justifications() {
        let justification = |validator: u8| Justification {
            validator: vec![validator; 65].into(),
            latest_block_hash: vec![0; 32].into(),
        };
        let mut block = get_random_block_default();
        block.sig = vec![1].into();
        block.sig_algorithm = "secp256k1".to_string();

        block.justifications = vec![justification(1), justification(2)];
        assert!(Validate::format_of_fields(&block));

        block.justifications = vec![justification(2), justification(1)];
        assert!(!Validate::format_of_fields(&block));

        block.justifications = vec![justification(1), justification(1)];
        assert!(!Validate::format_of_fields(&block));
    }
}

#[cfg(test)]
mod previous_hash_rule_tests {
    use models::rust::block_implicits::get_random_block_default;
    use models::rust::casper::protocol::casper_message::{ApprovedBlock, ApprovedBlockCandidate};
    use prost::Message;

    use super::*;
    use crate::rust::engine::casper_launch::ensure_compatible_approved_block;

    fn previous_rule_hash(block: &BlockMessage) -> BlockHash {
        let bytes: Vec<u8> = block
            .header
            .to_proto()
            .encode_to_vec()
            .into_iter()
            .chain(block.body.to_proto().encode_to_vec())
            .chain(block.sender.clone())
            .chain(block.sig_algorithm.as_bytes().to_vec())
            .chain(block.seq_num.to_le_bytes())
            .chain(block.shard_id.as_bytes().to_vec())
            .chain(block.extra_bytes.clone())
            .collect();
        Blake2b256::hash(bytes).into()
    }

    fn approved(block: BlockMessage) -> ApprovedBlock {
        ApprovedBlock {
            candidate: ApprovedBlockCandidate {
                block,
                required_sigs: 0,
            },
            sigs: vec![],
            floor_seed: None,
        }
    }

    #[test]
    fn blocks_hashed_by_the_previous_rule_are_refused() {
        let mut block = get_random_block_default();
        block.block_hash = previous_rule_hash(&block);

        assert!(matches!(
            Validate::block_hash(&block),
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockHash))
        ));
        assert!(!Validate::approved_block(&approved(block.clone())));
        assert!(ensure_compatible_approved_block(&approved(block.clone())).is_err());

        block.block_hash = proto_util::hash_block(&block);
        assert!(Validate::approved_block(&approved(block.clone())));
        assert!(ensure_compatible_approved_block(&approved(block)).is_ok());
    }
}

#[cfg(test)]
mod historical_block_tests {
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rust::block_implicits::get_random_block_default;

    use super::*;
    use crate::rust::validator_identity::ValidatorIdentity;

    #[test]
    fn historical_blocks_need_a_valid_signature_except_genesis() {
        let (secret_key, _) = Secp256k1.new_key_pair();
        let mut block = get_random_block_default();
        block.body.state.block_number = 5;
        let signed = ValidatorIdentity::new(&secret_key).sign_block(&block);
        assert!(Validate::historical_block(&signed));

        let mut tampered = signed.clone();
        tampered.sig = vec![0u8; 70].into();
        assert!(matches!(Validate::block_hash(&tampered), Either::Right(_)));
        assert!(!Validate::historical_block(&tampered));

        let mut genesis = get_random_block_default();
        genesis.body.state.block_number = 0;
        genesis.sender = Bytes::new();
        genesis.sig = Bytes::new();
        genesis.sig_algorithm = String::new();
        genesis.block_hash = proto_util::hash_block(&genesis);
        assert!(Validate::historical_block(&genesis));
    }
}
