// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
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
//! 2. `validate_block_checkpoint` — replay deploys against the pre-state
//!    hash and verify the resulting state matches the block's
//!    `post_state_hash`.
//! 3. `bonds_cache` — verify the block's bonds map matches the bonds
//!    computed from the parent post-state hash.
//! 4. `neglected_invalid_block` — reject the block if it has invalid
//!    justifications whose bonded sender is *still* bonded (T-9.7).
//! 5. `check_neglected_equivocations_with_update` — see Bug #2 / T-9.2.
//! 6. `check_equivocations` — direct equivocation check against the
//!    sender's prior latest message.
//!
//! D3 (DR-9): the former per-block `phlo_price` minimum-price rule is REMOVED —
//! deploys carry no phlo price/limit; per-signature funding is settled at block
//! assembly by the acceptance gate (against Σ⟦s⟧).
//!
//! ## Slashing-protocol position
//!
//! Steps 4, 5, 7 each surface `InvalidBlock::*Equivocation` /
//! `NeglectedInvalidBlock` to the dispatcher, which then mints
//! `EquivocationRecord` evidence and routes the block to the
//! `engine::multi_parent_casper::validation_dispatcher::dispatch_handle_invalid_block`
//! path.

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
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use prost::Message;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::dag::dag_ops;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::slashing_authorization::validate_received_slash_deploys;
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

    pub fn block_sender_has_weight(
        b: &BlockMessage,
        genesis: &BlockMessage,
        block_store: &mut KeyValueBlockStore,
    ) -> Result<bool, KvStoreError> {
        if b == genesis {
            Ok(true)
        } else {
            proto_util::weight_from_sender(block_store, b).map(|weight| {
                if weight > 0 {
                    true
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
                    false
                }
            })
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
    // matches the spec in docs/theory/slashing/slashing-specification.md
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
            Self::repeat_deploy(block, s, block_store, expiration_threshold)
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
        // The slash-authorization predicate (T-9.8) requires "target is
        // currently bonded". For a received block whose `body.system_deploys`
        // contains a SlashDeploy, "currently bonded" semantically means
        // bonded at the BLOCK'S pre-state — i.e., in the bonds map carried
        // by the block's actual parents (per `block.header.parents_hash_list`),
        // not in whatever `s.on_chain_state.bonds_map` the validator's
        // current snapshot picked from its independent `valid_latest_msgs`.
        //
        // These can diverge in multi-parent merge scenarios: the snapshot's
        // chosen parents may include a sibling that has already applied the
        // same slash (and so reports the target at stake 0), while the
        // block-being-validated's actual parents may all be from a chain
        // where the slash hadn't landed yet (so the target's stake is
        // still positive). Without this rebind, slash-recovery proposals
        // are spuriously rejected as `UnauthorizedSlashDeploy`.
        //
        // We rebind a transient bonds_map view from the block's actual
        // parents (looked up via block_store) before delegating to
        // `slash_deploy_authorization`, then restore the original. Using
        // the union of validator stakes (max across parents) keeps the
        // most-lenient view, which matches the proposer-side
        // `authorized_slash_candidates` snapshot context.
        let _saved_bonds_map = if block.body.system_deploys.iter().any(|sd| {
            matches!(sd, ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::Slash { .. },
                ..
            })
        }) {
            let mut parent_bonds: std::collections::HashMap<Validator, i64> =
                std::collections::HashMap::new();
            for parent_hash in &block.header.parents_hash_list {
                let parent_block = match block_store.get(parent_hash) {
                    Ok(Some(parent_block)) => parent_block,
                    Ok(None) => return Either::Left(BlockError::MissingBlocks),
                    Err(err) => {
                        return Either::Left(BlockError::BlockException(CasperError::from(err)));
                    }
                };
                for bond in &parent_block.body.state.bonds {
                    parent_bonds
                        .entry(bond.validator.clone())
                        .and_modify(|existing| {
                            if bond.stake > *existing {
                                *existing = bond.stake;
                            }
                        })
                        .or_insert(bond.stake);
                }
            }
            if parent_bonds.is_empty() {
                None
            } else {
                let saved = std::mem::replace(&mut s.on_chain_state.bonds_map, parent_bonds);
                Some(saved)
            }
        } else {
            None
        };
        let slash_auth_outcome = Self::slash_deploy_authorization(block, s);
        if let Some(saved) = _saved_bonds_map {
            s.on_chain_state.bonds_map = saved;
        }
        match slash_auth_outcome {
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

    /// Validate no deploy with the same sig has been produced in the chain
    /// Agnostic of non-parent justifications.
    ///
    /// Recovery exemption: sigs present in `s.rejected_in_scope` (rejected
    /// by a descendant merge within deploy_lifespan) may be legitimate
    /// recovery candidates — the rejected-deploy buffer pipeline re-includes
    /// them so their effects can land in canonical state. Without this
    /// exemption, every recovery-path block would fail `InvalidRepeatDeploy`.
    ///
    /// The exemption is gated on the sig's current finalization status. A
    /// sig in `rejected_in_scope` falls into one of two cases:
    ///
    ///   - `Pending` / `Expired` / `Failed`: the deploy's effects are NOT
    ///     in canonical state (no clean canonical inclusion that survived
    ///     descendant rejection). Re-inclusion is the only way to land
    ///     them. Exempt from the repeat check.
    ///
    ///   - `Finalized`: the deploy has a clean canonical inclusion that
    ///     was NOT invalidated by a canonical-descendant rejection. Its
    ///     effects ARE already in canonical state. Re-inclusion would be
    ///     double-execution, not recovery. Do NOT exempt — let the
    ///     ancestor scan find the canonical inclusion and flag the
    ///     repeat. The catchup gate (`should_admit_to_rejected_buffer`)
    ///     is the primary defense against this; the validator-side
    ///     check is the second line of defense.
    pub fn repeat_deploy(
        block: &BlockMessage,
        s: &mut CasperSnapshot,
        block_store: &KeyValueBlockStore,
        expiration_threshold: i32,
    ) -> ValidBlockProcessing {
        use crate::rust::api::deploy_finalization_status::{
            resolve as resolve_finalization_status, DeployFinalizationState,
        };

        let deploy_key_set: HashSet<Vec<u8>> = block
            .body
            .deploys
            .iter()
            .filter(|pd| {
                if !s.rejected_in_scope.contains(&pd.deploy.sig) {
                    return true; // not rejected — must check
                }
                // Sig is in rejected_in_scope. Apply the exemption only if
                // the sig is NOT Finalized — otherwise re-inclusion is
                // double-execution and the repeat check must catch it.
                match resolve_finalization_status(
                    &s.dag,
                    block_store,
                    expiration_threshold as i64,
                    &pd.deploy.sig,
                ) {
                    Ok(status) if status.state == DeployFinalizationState::Finalized => {
                        let canonical_block_str = status
                            .latest_block_hash
                            .as_ref()
                            .map(|h| PrettyPrinter::build_string_bytes(h))
                            .unwrap_or_else(|| "<none>".to_string());
                        tracing::warn!(
                            "repeat_deploy: sig {} is in rejected_in_scope but \
                             resolves to Finalized (clean canonical inclusion at \
                             {}); declining the recovery exemption to prevent \
                             double-execution",
                            hex::encode(&pd.deploy.sig),
                            canonical_block_str,
                        );
                        true // keep in check set so the ancestor scan finds the repeat
                    }
                    Ok(_) => false, // status != Finalized → exempt (recovery)
                    Err(err) => {
                        // Resolver failures are conservative-fail: keep the sig
                        // in the check set so an inconsistency surfaces as
                        // InvalidRepeatDeploy rather than being silently
                        // exempted as a recovery candidate.
                        tracing::warn!(
                            "repeat_deploy: deploy_finalization_status::resolve \
                             failed for sig {}: {} — keeping sig in check set \
                             rather than granting recovery exemption",
                            hex::encode(&pd.deploy.sig),
                            err,
                        );
                        true
                    }
                }
            })
            .map(|pd| pd.deploy.sig.to_vec())
            .collect();
        if deploy_key_set.is_empty() {
            return Either::Right(ValidBlock::Valid);
        }

        let block_metadata = BlockMetadata::from_block(block, false, None, None);

        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-get-parents");
        let init_parents = match proto_util::get_parents_metadata(&s.dag, &block_metadata) {
            Ok(parents) => parents,
            Err(e) => return Either::Left(BlockError::BlockException(CasperError::from(e))),
        };

        // Calculate max block number and earliest acceptable block number
        let max_block_number = proto_util::max_block_number_metadata(&init_parents);
        let earliest_block_number = max_block_number + 1 - expiration_threshold as i64;

        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-duplicate-block");
        let maybe_duplicated_block_metadata = dag_ops::bf_traverse_find(
            init_parents,
            |block_metadata| {
                proto_util::get_parent_metadatas_above_block_number(
                    block_metadata,
                    earliest_block_number,
                    &s.dag,
                )
                .unwrap_or_default()
            },
            |block_metadata| {
                block_store.has_any_deploy_sig_unsafe(&block_metadata.block_hash, &deploy_key_set)
            },
        );

        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-duplicate-block-log");
        let maybe_error = maybe_duplicated_block_metadata.map(|duplicated_block_metadata| {
      let duplicated_block = block_store.get_unsafe(&duplicated_block_metadata.block_hash);
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
            .any(|pd| !is_system_deploy_id(&pd.deploy.sig));
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
            let highest_tip_height = s.dag.latest_block_number();
            for parent_hash in &parent_hashes {
                if parent_hash == &genesis.block_hash {
                    continue; // genesis exempt
                }
                let parent_meta = match s.dag.lookup_unsafe(parent_hash) {
                    Ok(meta) => meta,
                    Err(_) => continue, // missing-parent handled by dependency gate, not here
                };
                let depth = highest_tip_height - parent_meta.block_number;
                if depth > max_allowed_depth {
                    let message = format!(
                        "parent {} at block_number {} is at depth {} from highest tip {} \
                         (exceeds max_parent_depth + depth_buffer = {})",
                        PrettyPrinter::build_string_bytes(parent_hash),
                        parent_meta.block_number,
                        depth,
                        highest_tip_height,
                        max_allowed_depth
                    );
                    tracing::warn!("{}", Self::ignore(b, &message));
                    return Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents));
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

                // BFS traverse to get ancestor closure of previous block
                // Stop traversal at finalized blocks to prevent unbounded traversal on long chains
                let ancestor_hashes: Vec<BlockHash> =
                    dag_ops::bf_traverse(vec![prev_block_hash.clone()], |hash| {
                        match s.dag.lookup(hash) {
                            Ok(Some(meta)) if !s.dag.is_finalized(hash) => meta.parents.clone(),
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

    /// Tier-2 validation gate for received `Slash` system deploys. Delegates
    /// to `slashing_authorization::validate_received_slash_deploys` and
    /// distinguishes two failure classes:
    ///
    /// * `CasperError::SlashAuth(_)` — the receive-side authorization
    ///   predicate (4-conjunct check) rejected the slash deploy. The block
    ///   author is Byzantine; collapse to
    ///   `InvalidBlock::UnauthorizedSlashDeploy`, which is itself slashable
    ///   per `block_status::is_slashable` and the T-9.3 catch-all dispatcher.
    /// * any other `CasperError` (storage I/O, runtime, history) — the local
    ///   node experienced an infrastructure failure unrelated to the block
    ///   author's behavior. Propagate as `BlockError::BlockException(e)`;
    ///   do NOT slash the block sender for a fault attributable to local
    ///   infrastructure. Bug-fix rationale: see
    ///   docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.14.
    pub fn slash_deploy_authorization(
        block: &BlockMessage,
        s: &CasperSnapshot,
    ) -> ValidBlockProcessing {
        Self::route_slash_validation_outcome(block, validate_received_slash_deploys(block, s))
    }

    /// Routes the outcome of `validate_received_slash_deploys` into the
    /// validator's `Either` shape. Exposed `pub` so the dispatching logic —
    /// which distinguishes Byzantine-author errors from local-infrastructure
    /// errors — can be unit-tested from integration tests.
    ///
    /// See `slash_deploy_authorization` for the full rationale and
    /// docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.14
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
    /// Compares justifications previously cited by `b.sender` (taken from
    /// `cur_senders_block`, the sender's current latest message in the DAG)
    /// against justifications cited by the new block `b`, and rejects any
    /// regression — including a regression against the sender's own prior
    /// creator-justification.
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
    /// `docs/theory/slashing/design/09-bug-fixes-and-rationale.md` §9.6.
    pub fn justification_regressions(
        b: &BlockMessage,
        s: &mut CasperSnapshot,
    ) -> ValidBlockProcessing {
        match s.dag.latest_message(&b.sender) {
            Ok(None) => {
                // `b` is first message from sender of `b`, so regression is not possible
                Either::Right(ValidBlock::Valid)
            }
            Ok(Some(cur_senders_block)) => {
                // Latest Message from sender of `b` is present in the DAG
                // Here we comparing view on the network by sender from the standpoint of
                // his previous block created (current Latest Message of sender)
                // and new block `b` (potential new Latest Message of sender)
                let new_sender_block = b;
                let new_lms =
                    proto_util::to_latest_message_hashes(&new_sender_block.justifications);
                let cur_lms =
                    proto_util::to_latest_message_hashes(&cur_senders_block.justifications);

                // Self-regression is checked here too: include the sender's
                // self-justification so a block that points back to its own
                // earlier sequence number is detected as JustificationRegression.
                // See docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.6.

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

                    if !new_justification.invalid
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
        s: &mut CasperSnapshot,
    ) -> ValidBlockProcessing {
        let mut invalid_justifications = Vec::new();
        for justification in &block.justifications {
            let latest_block_opt = match s.dag.lookup(&justification.latest_block_hash) {
                Ok(opt) => opt,
                Err(e) => return Either::Left(BlockError::BlockException(CasperError::from(e))),
            };
            if latest_block_opt.is_some_and(|block_metadata| block_metadata.invalid) {
                invalid_justifications.push(justification);
            }
        }

        // P2-10: build a single bonds index up-front (O(B)) so the
        // any-invalid-justification check is O(J + B) instead of the prior
        // O(J · B) `.iter().find(...)` linear scan per justification.
        let bonds = proto_util::bonds(block);
        let bonds_by_validator: HashMap<&Validator, i64> = bonds
            .iter()
            .map(|bond| (&bond.validator, bond.stake))
            .collect();
        let neglected_invalid_justification = invalid_justifications.iter().any(|justification| {
            bonds_by_validator
                .get(&justification.validator)
                .copied()
                .is_some_and(|stake| stake > 0)
        });

        // Recovery path: if this block carries slash system deploys, allow it through so
        // validators can converge by slashing the offending branch.
        let has_slash_system_deploys = block.body.system_deploys.iter().any(|system_deploy| {
            matches!(system_deploy, ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::Slash { .. },
                ..
            })
        });

        if neglected_invalid_justification && !has_slash_system_deploys {
            Either::Left(BlockError::Invalid(InvalidBlock::NeglectedInvalidBlock))
        } else {
            Either::Right(ValidBlock::Valid)
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
                let bonds_set: HashSet<_> = bonds
                    .iter()
                    .map(|bond| (&bond.validator, bond.stake))
                    .collect();
                let computed_bonds_set: HashSet<_> = computed_bonds
                    .iter()
                    .map(|bond| (&bond.validator, bond.stake))
                    .collect();

                if bonds_set == computed_bonds_set {
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
    // Σ⟦s⟧, with `min_phlo_price` repurposed as that gate's safety margin.
}
