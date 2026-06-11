// See casper/src/main/scala/coop/rchain/casper/Validate.scala

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
use crate::rust::system_deploy::is_system_deploy_id;
use crate::rust::util::proto_util;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::ValidBlockProcessing;

pub type PublicKey = Vec<u8>;
pub type Data = Vec<u8>;
pub type Signature = Vec<u8>;

const DRIFT: i64 = 15000; // 15 seconds

pub struct Validate;

impl Validate {
    //TODO: It should be simplified once we remove &self from the verify function.
    fn signature_verifiers() -> HashMap<String, Box<dyn Fn(&Data, &Signature, &PublicKey) -> bool>>
    {
        let mut map: HashMap<String, Box<dyn Fn(&Data, &Signature, &PublicKey) -> bool>> =
            HashMap::new();
        map.insert(
            "secp256k1".to_string(),
            Box::new(|data: &Vec<u8>, signature: &Vec<u8>, pub_key: &Vec<u8>| {
                let secp256k1 = Secp256k1;
                secp256k1.verify(data, signature, pub_key)
            }) as Box<dyn Fn(&Data, &Signature, &PublicKey) -> bool>,
        );
        #[cfg(feature = "schnorr_secp256k1_experimental")]
        map.insert(
            SchnorrSecp256k1::name(),
            Box::new(|data: &Vec<u8>, signature: &Vec<u8>, pub_key: &Vec<u8>| {
                let schnorr = SchnorrSecp256k1;
                schnorr.verify(data, signature, pub_key)
            }) as Box<dyn Fn(&Data, &Signature, &PublicKey) -> bool>,
        );
        #[cfg(feature = "schnorr_secp256k1_experimental")]
        map.insert(
            FrostSecp256k1::name(),
            Box::new(|data: &Vec<u8>, signature: &Vec<u8>, pub_key: &Vec<u8>| {
                let frost = FrostSecp256k1;
                frost.verify(data, signature, pub_key)
            }) as Box<dyn Fn(&Data, &Signature, &PublicKey) -> bool>,
        );
        map
    }

    pub fn signature(d: &Data, sig: &ProtoSignature) -> bool {
        Self::signature_verifiers()
            .get(&sig.algorithm)
            .is_some_and(|verify| verify(d, &sig.sig.to_vec(), &sig.public_key.to_vec()))
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

        let signature_verifiers = Self::signature_verifiers();

        let signatures: HashSet<Bytes> = approved_block
            .sigs
            .iter()
            .filter_map(|signature| {
                signature_verifiers
                    .get(&signature.algorithm)
                    .and_then(|verify_sig| {
                        if verify_sig(
                            &candidate_bytes_digest,
                            &signature.sig.to_vec(),
                            &signature.public_key.to_vec(),
                        ) {
                            Some(signature.public_key.clone())
                        } else {
                            None
                        }
                    })
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
        let result = Self::signature_verifiers()
            .get(&b.sig_algorithm)
            .map(|verify| {
                match verify(&b.block_hash.to_vec(), &b.sig.to_vec(), &b.sender.to_vec()) {
                    true => true,
                    false => {
                        tracing::warn!("{}", Self::ignore(b, "signature is invalid."));
                        false
                    }
                }
            });

        result.unwrap_or_else(|| {
            tracing::warn!(
                "{}",
                Self::ignore(
                    b,
                    &format!("signature algorithm {} is unsupported.", b.sig_algorithm)
                )
            );
            false
        })
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

    //TODO: Scala message -> Double check ordering of validity checks
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
    /// Repeat-deploy validation, computed purely from the block's own ancestry.
    ///
    /// A deploy sig already included in an ancestor block is a REPEAT — invalid —
    /// unless its most recent inclusion was subsequently rejected (a strictly
    /// later-or-equal ancestor records the sig in `body.rejected_deploys`), in
    /// which case re-inclusion is the recovery path re-proposing merge-rejected
    /// work. Every input is an ancestor block BODY — consensus data — so every
    /// node reaches the same verdict for the same block. (The previous form keyed
    /// the recovery exemption on the validator's own `rejected_in_scope` snapshot
    /// state and local finalization view, which split verdicts across nodes with
    /// different attach times.)
    pub fn repeat_deploy(
        block: &BlockMessage,
        s: &mut CasperSnapshot,
        block_store: &KeyValueBlockStore,
        expiration_threshold: i32,
    ) -> ValidBlockProcessing {
        let checked_sigs: HashSet<prost::bytes::Bytes> = block
            .body
            .deploys
            .iter()
            .map(|pd| pd.deploy.sig.clone())
            .collect();
        if checked_sigs.is_empty() {
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

        // One traversal of the ancestry window collecting, per checked sig, the
        // most recent prior INCLUSION (body.deploys) and the most recent
        // REJECTION record (body.rejected_deploys).
        tracing::debug!(target: "f1r3fly.casper", "before-repeat-deploy-ancestry-scan");
        let visited = dag_ops::bf_traverse(init_parents, |metadata| {
            proto_util::get_parent_metadatas_above_block_number(
                metadata,
                earliest_block_number,
                &s.dag,
            )
            .unwrap_or_default()
        });

        let mut latest_inclusion: HashMap<prost::bytes::Bytes, (i64, BlockHash)> = HashMap::new();
        let mut latest_rejection: HashMap<prost::bytes::Bytes, i64> = HashMap::new();
        for metadata in &visited {
            let ancestor = block_store.get_unsafe(&metadata.block_hash);
            for pd in &ancestor.body.deploys {
                if checked_sigs.contains(&pd.deploy.sig) {
                    let entry = latest_inclusion
                        .entry(pd.deploy.sig.clone())
                        .or_insert((i64::MIN, BlockHash::default()));
                    if metadata.block_number > entry.0 {
                        *entry = (metadata.block_number, metadata.block_hash.clone());
                    }
                }
            }
            for rd in &ancestor.body.rejected_deploys {
                if checked_sigs.contains(&rd.sig) {
                    let entry = latest_rejection.entry(rd.sig.clone()).or_insert(i64::MIN);
                    if metadata.block_number > *entry {
                        *entry = metadata.block_number;
                    }
                }
            }
        }

        for sig in &checked_sigs {
            let Some((inclusion_number, inclusion_block)) = latest_inclusion.get(sig) else {
                continue; // never included before — clean
            };
            // A rejection recorded at-or-above the latest inclusion height means
            // that inclusion's effect was merged away; re-proposal is legal.
            let rejection_number = latest_rejection.get(sig).copied().unwrap_or(i64::MIN);
            if rejection_number >= *inclusion_number {
                tracing::debug!(
                    target: "f1r3fly.casper",
                    sig = %hex::encode(&sig[..sig.len().min(16)]),
                    inclusion_number,
                    rejection_number,
                    "repeat_deploy: prior inclusion was rejected in ancestry; re-proposal exempt"
                );
                continue;
            }

            let duplicated_block = block_store.get_unsafe(inclusion_block);
            let duplicated_deploy = duplicated_block
                .body
                .deploys
                .iter()
                .map(|processed_deploy| &processed_deploy.deploy)
                .find(|deploy| deploy.sig == *sig)
                .expect("inclusion was recorded from this block's deploys above");
            let message = format!(
                "found deploy [{}] (user {}, millisecond timestamp {}) with the same sig in the block {} as current block {} (latest rejection at #{} < inclusion at #{})",
                &duplicated_deploy.data.term,
                PrettyPrinter::build_string_bytes(&duplicated_deploy.pk.bytes),
                duplicated_deploy.data.time_stamp,
                PrettyPrinter::build_string_bytes(&duplicated_block.block_hash),
                PrettyPrinter::build_string_bytes(&block.block_hash),
                rejection_number,
                inclusion_number,
            );
            tracing::warn!("{}", Self::ignore(block, &message));
            return Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy));
        }

        Either::Right(ValidBlock::Valid)
    }

    // This is not a slashable offence
    pub fn timestamp(b: &BlockMessage, block_store: &KeyValueBlockStore) -> ValidBlockProcessing {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let timestamp = b.header.timestamp;

        let before_future = current_time + DRIFT >= timestamp;

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

        // Check maxNumberOfParents constraint
        // Note: We use -1 as "unlimited" here (matching config file convention) rather than
        // Estimator::UNLIMITED_PARENTS (i32::MAX) since this value comes from config parsing.
        const UNLIMITED_PARENTS: i32 = -1;
        if max_number_of_parents != UNLIMITED_PARENTS
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
        // proposer-side convention in `multi_parent_casper_impl.rs::create_block`).
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

    /// Justification regression check.
    /// Compares justifications that has been already used by sender and recorded in the DAG with
    /// justifications used by the same sender in new block `b` and assures that there is no
    /// regression.
    ///
    /// When we switch between equivocation forks for a slashed validator, we will potentially get a
    /// justification regression that is valid. We cannot ignore this as the creator only drops the
    /// justification block created by the equivocator on the following block.
    /// Hence, we ignore justification regressions involving the block's sender and
    /// let checkEquivocations handle it instead.
    // TODO double check this logic
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
                    proto_util::to_latest_message_hashes(new_sender_block.justifications.clone());
                let cur_lms =
                    proto_util::to_latest_message_hashes(cur_senders_block.justifications.clone());

                // We let checkEquivocations handle when sender uses old self-justification
                let new_lms_no_self: HashMap<Validator, BlockHash> = new_lms
                    .into_iter()
                    .filter(|(validator, _)| validator != &b.sender)
                    .collect();

                // Check each Latest Message for regression (block seq num goes backwards)
                let mut remaining_lms: Vec<(Validator, BlockHash)> =
                    new_lms_no_self.into_iter().collect();

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

                loop {
                    match remaining_lms.as_slice() {
                        // No more Latest Messages to check
                        [] => break,
                        // Check if sender of LatestMessage does justification regression
                        [new_lm, tail @ ..] => {
                            let (sender, new_justification_hash) = new_lm;
                            let no_sender_in_cur_lms = !cur_lms.contains_key(sender);

                            if no_sender_in_cur_lms {
                                // If there is no justification to compare with - regression is not possible
                                remaining_lms = tail.to_vec();
                                continue;
                            }

                            let cur_justification_hash = &cur_lms[sender];

                            // Compare and check for regression
                            let new_justification =
                                match s.dag.lookup_unsafe(new_justification_hash) {
                                    Ok(metadata) => metadata,
                                    Err(e) => {
                                        return Either::Left(BlockError::BlockException(
                                            CasperError::from(e),
                                        ))
                                    }
                                };
                            let cur_justification =
                                match s.dag.lookup_unsafe(cur_justification_hash) {
                                    Ok(metadata) => metadata,
                                    Err(e) => {
                                        return Either::Left(BlockError::BlockException(
                                            CasperError::from(e),
                                        ))
                                    }
                                };

                            let regression_detected = {
                                let regression = !new_justification.invalid
                                    && new_justification.sequence_number
                                        < cur_justification.sequence_number;

                                if regression {
                                    log_warn(
                                        cur_justification_hash,
                                        new_justification_hash,
                                        sender,
                                    );
                                }

                                regression
                            };

                            // Exit when regression detected, or continue to check remaining Latest Messages
                            if regression_detected {
                                return Either::Left(BlockError::Invalid(
                                    InvalidBlock::JustificationRegression,
                                ));
                            } else {
                                remaining_lms = tail.to_vec();
                            }
                        }
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

        let bonds = proto_util::bonds(block);
        let neglected_invalid_justification = invalid_justifications.iter().any(|justification| {
            let slashed_validator_bond = bonds
                .iter()
                .find(|bond| bond.validator == justification.validator);

            match slashed_validator_bond {
                Some(bond) => bond.stake > 0,
                None => false,
            }
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

        tracing::debug!(
            target: "f1r3fly.casper.bonds_validation",
            block = %hex::encode(&b.block_hash),
            post_state = %hex::encode(&tuplespace_hash),
            block_bonds_count = bonds.len(),
            "bonds cache validate entry",
        );
        for bond in bonds.iter() {
            tracing::trace!(
                target: "f1r3fly.casper.bonds_validation",
                block = %hex::encode(&b.block_hash),
                validator = %hex::encode(&bond.validator),
                stake = bond.stake,
                "bonds-cache block bond",
            );
        }

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

                tracing::debug!(
                    target: "f1r3fly.casper.bonds_validation",
                    block = %hex::encode(&b.block_hash),
                    computed_bonds_count = computed_bonds.len(),
                    "computed bonds",
                );
                if bonds_set == computed_bonds_set {
                    tracing::debug!(
                        target: "f1r3fly.casper.bonds_validation",
                        block = %hex::encode(&b.block_hash),
                        "bonds cache match",
                    );
                    Either::Right(ValidBlock::Valid)
                } else {
                    tracing::warn!(
                        "Bonds in proof of stake contract do not match block's bond cache."
                    );
                    tracing::warn!(
                        target: "f1r3fly.casper.bonds_validation",
                        block = %hex::encode(&b.block_hash),
                        post_state = %hex::encode(&tuplespace_hash),
                        block_count = bonds_set.len(),
                        computed_count = computed_bonds_set.len(),
                        "bonds cache mismatch (InvalidBondsCache)",
                    );
                    Either::Left(BlockError::Invalid(InvalidBlock::InvalidBondsCache))
                }
            }
            Err(ex) => {
                tracing::warn!("Failed to compute bonds from tuplespace hash: {}", ex);
                tracing::warn!(
                    target: "f1r3fly.casper.bonds_validation",
                    block = %hex::encode(&b.block_hash),
                    error = %ex,
                    "compute bonds failed",
                );
                Either::Left(BlockError::BlockException(ex))
            }
        }
    }

    /// All of deploys must have greater or equal phloPrice than minPhloPrice
    pub fn phlo_price(b: &BlockMessage, min_phlo_price: i64) -> ValidBlockProcessing {
        if b.body
            .deploys
            .iter()
            .all(|deploy| deploy.deploy.data.phlo_price >= min_phlo_price)
        {
            Either::Right(ValidBlock::Valid)
        } else {
            Either::Left(BlockError::Invalid(InvalidBlock::LowDeployCost))
        }
    }
}
