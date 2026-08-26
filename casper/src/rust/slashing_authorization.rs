//! Authorization predicate for `Slash` system deploys.
//!
//! Every `Slash` system deploy carried in a block must reference current-epoch
//! evidence whose offender is positively bonded. Unary evidence names a
//! locally invalid block. Objective equivocation evidence names a canonical
//! pair of distinct blocks from one sender at one sequence number and does not
//! depend on either block's local invalid flag. This module provides both
//! halves of that contract:
//!
//! * [`authorized_slash_candidates`] — the proposer-side enumeration the block
//!   creator uses to decide which slashes to mint.
//! * [`validate_received_slash_deploys`] — the receive-side check that mirrors
//!   the predicate and rejects unauthorized slashes with
//!   `InvalidBlock::UnauthorizedSlashDeploy`.
//!
//! The unary predicate `received_slash_deploy_authorized` retains the
//! current-epoch, matching-evidence-epoch, positive-bond, and locally-invalid
//! requirements proven sufficient by Theorem T-9.13. Objective pairs replace
//! the final local predicate with an immutable same-sender/same-sequence
//! relation proven in `ObjectiveEquivocation.v`.
//!
//! Boundary helpers (`checked_base_seq`, `checked_next_seq`,
//! `epoch_for_block_number`) live here because their failure modes feed back
//! into the same authorization decision; they are also the surface that the
//! `kani_proofs` module models exhaustively at the bottom of the file.

// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::casper::CasperSnapshot;
use crate::rust::epoch::Epoch;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// Phase 9 (C-6): typed domain-level failure reasons for the epoch
/// arithmetic primitives. Replaces the prior `Option<i64>` /
/// `Option<bool>` shapes whose `None` arm conflated multiple causes
/// (invalid `epoch_length`, negative `block_number`). The new
/// `Result<_, DomainError>` shape lets callers either disambiguate
/// or, where the API surface is the same, map cleanly into
/// `SlashAuthError::InvalidEpochLength`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("invalid epoch length {0} (must be > 0)")]
    InvalidEpochLength(i32),
    #[error("negative block number {0} (must be >= 0)")]
    NegativeBlockNumber(i64),
}

impl From<DomainError> for SlashAuthError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::InvalidEpochLength(n) => SlashAuthError::InvalidEpochLength(n),
            DomainError::NegativeBlockNumber(n) => SlashAuthError::NegativeBlockNumber(n),
        }
    }
}

/// P4-1: typed authorization-failure reasons surfaced by
/// [`validate_received_slash_deploys`]. Replaces the eight previously
/// distinct `CasperError::RuntimeError("...")` messages with named
/// variants that carry the offending block/validator context. Operators
/// can now match on the variant instead of grepping log strings, and
/// the conjunctive predicate from Theorem T-9.13 is preserved one
/// variant per conjunct.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SlashAuthError {
    #[error(
        "slash deploy issuer does not match block sender (block={block_hash}, issuer={issuer}, sender={sender})"
    )]
    IssuerMismatch {
        block_hash: String,
        issuer: String,
        sender: String,
    },
    #[error("slash deploy targets non-current epoch (target={target}, current={current})")]
    EpochMismatch { target: Epoch, current: Epoch },
    #[error("slash deploy references unknown invalid block {hash}")]
    ReferencesUnknownBlock { hash: String },
    #[error("slash deploy references a valid block {hash}")]
    ReferencesValidBlock { hash: String },
    #[error("equivocation evidence pair is not in canonical hash order ({first}, {second})")]
    NonCanonicalEquivocationPair { first: String, second: String },
    #[error("equivocation evidence blocks do not share one sender and sequence")]
    EquivocationEvidenceMismatch,
    #[error("invalid epoch length {0}")]
    InvalidEpochLength(i32),
    #[error("negative block number {0}")]
    NegativeBlockNumber(i64),
    #[error(
        "slash deploy epoch ({evidence_epoch}) does not match invalid-block evidence epoch ({target_epoch})"
    )]
    EvidenceEpochMismatch {
        evidence_epoch: Epoch,
        target_epoch: Epoch,
    },
    #[error("slash deploy target {validator} is not currently bonded")]
    TargetNotBonded { validator: String },
    #[error(
        "slash deploy is not authorized by current invalid-block evidence (validator {validator})"
    )]
    NotAuthorizedByEvidence { validator: String },
    #[error(
        "duplicate slash deploy target in block (validator {validator}, generation {generation}, epoch {epoch})"
    )]
    DuplicateTarget {
        validator: String,
        generation: BondGeneration,
        epoch: Epoch,
    },
    #[error("negative block sequence number {seq_num} (block {block_hash})")]
    NegativeSequenceNumber { block_hash: String, seq_num: i32 },
    #[error(
        "slash deploy bond generation does not match certified evidence or current PoS state (validator {validator})"
    )]
    BondGenerationMismatch { validator: String },
}

// Phase 9 (R-2): `From<SlashAuthError> for CasperError` now lives in
// `errors.rs` and routes to the new structured `CasperError::SlashAuth`
// variant — previous stringification at this boundary defeated the
// typed-error effort.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedSlashCandidate {
    pub offender: Validator,
    pub invalid_block_hash: BlockHash,
    pub equivocation_block_hash: Option<BlockHash>,
    /// Epoch under which the slash takes effect. By construction this equals
    /// the epoch of every evidence block at commit time. The receiver
    /// reconstructs it from that evidence (see
    /// `slash_evidence_epoch_matches_target`), so the proposer cannot move the
    /// slash to a different epoch.
    ///
    /// Phase 10 (C-5): typed [`Epoch`] newtype replaces the raw `i64`;
    /// conversion at the protobuf boundary uses
    /// `Epoch::from(slash_deploy.target_activation_epoch)`.
    pub target_activation_epoch: Epoch,
    pub target_bond_generation: BondGeneration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalSlashAuthority {
    state_hash: StateHash,
    bonds: HashMap<Validator, i64>,
    generations: HashMap<Validator, BondGeneration>,
}

impl CanonicalSlashAuthority {
    pub async fn load(
        runtime_manager: &RuntimeManager,
        state_hash: &StateHash,
    ) -> Result<Self, CasperError> {
        let (bonds, generations) = tokio::try_join!(
            runtime_manager.compute_bonds(state_hash),
            runtime_manager.compute_bond_generations(state_hash)
        )?;
        let bonds = bonds
            .into_iter()
            .map(|bond| (bond.validator, bond.stake))
            .collect();
        let generations = generations
            .into_iter()
            .map(|(validator, generation)| {
                BondGeneration::try_from(generation)
                    .map(|generation| (validator, generation))
                    .map_err(|error| {
                        CasperError::RuntimeError(format!(
                            "PoS returned an invalid bond generation: {error}"
                        ))
                    })
            })
            .collect::<Result<_, _>>()?;
        Self::new_checked(state_hash.clone(), bonds, generations)
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn from_parts(
        state_hash: StateHash,
        bonds: HashMap<Validator, i64>,
        generations: HashMap<Validator, BondGeneration>,
    ) -> Result<Self, CasperError> {
        Self::new_checked(state_hash, bonds, generations)
    }

    fn new_checked(
        state_hash: StateHash,
        bonds: HashMap<Validator, i64>,
        generations: HashMap<Validator, BondGeneration>,
    ) -> Result<Self, CasperError> {
        if let Some((validator, _)) = bonds
            .iter()
            .find(|(validator, bond)| **bond > 0 && !generations.contains_key(*validator))
        {
            return Err(CasperError::RuntimeError(format!(
                "PoS state {} has a positive bond without a bond generation for validator {}",
                hex::encode(&state_hash),
                hex::encode(validator)
            )));
        }
        Ok(Self {
            state_hash,
            bonds,
            generations,
        })
    }

    pub fn state_hash(&self) -> &StateHash { &self.state_hash }

    pub fn bonds(&self) -> &HashMap<Validator, i64> { &self.bonds }

    pub fn generations(&self) -> &HashMap<Validator, BondGeneration> { &self.generations }

    pub fn bond(&self, validator: &Validator) -> i64 {
        self.bonds.get(validator).copied().unwrap_or(0)
    }

    pub fn generation(&self, validator: &Validator) -> Option<BondGeneration> {
        self.generations.get(validator).copied()
    }
}

/// Phase 9 (C-6): returns a typed [`DomainError`] when the input
/// constraints fail. The two failure modes are now distinguishable:
/// non-positive `epoch_length` is configuration-derived, while a
/// negative `block_number` indicates an arithmetic or wire-format bug.
/// The caller (`authorized_slash_candidates` /
/// `validate_received_slash_deploys`) maps either into the appropriate
/// `SlashAuthError` variant; never panic here — shard configuration can
/// legally hand us `epoch_length == 0` at startup.
pub fn epoch_for_block_number(block_number: i64, epoch_length: i32) -> Result<Epoch, DomainError> {
    if epoch_length <= 0 {
        Err(DomainError::InvalidEpochLength(epoch_length))
    } else if block_number < 0 {
        Err(DomainError::NegativeBlockNumber(block_number))
    } else {
        Ok(Epoch::new(block_number / i64::from(epoch_length)))
    }
}

/// Predecessor of a sequence number used as the *exclusive* lower bound for
/// self-justification walks. The boundary is `seq_num <= 0`, not `<= 1`:
/// sequence 1 is a valid genesis-child and must round-trip to `Some(0)`. See
/// commit `db0b979` ("Fix slashing sequence base boundary") and the
/// `kani_proofs::checked_base_seq_*` proofs.
pub fn checked_base_seq(seq_num: i32) -> Option<i32> {
    if seq_num <= 0 {
        None
    } else {
        Some(seq_num - 1)
    }
}

/// Successor of a `u64` sequence width, narrowed to the wire-format `i32`.
/// The double check (`u64::checked_add` then `i32::try_from`) saturates to
/// `None` on either u64 overflow or i32 truncation — silently wrapping would
/// let an attacker craft a sequence-number rollover. Modeled exhaustively by
/// `kani_proofs::checked_next_seq_matches_i32_successor`.
pub fn checked_next_seq(max_seq: u64) -> Option<i32> {
    max_seq
        .checked_add(1)
        .and_then(|seq| i32::try_from(seq).ok())
}

pub fn slash_target_epoch_is_current(
    reference_block_number: i64,
    target_activation_epoch: Epoch,
    epoch_length: i32,
) -> Result<bool, DomainError> {
    epoch_for_block_number(reference_block_number, epoch_length)
        .map(|current_epoch| target_activation_epoch == current_epoch)
}

pub fn slash_evidence_epoch_matches_target(
    evidence_block_number: i64,
    target_activation_epoch: Epoch,
    epoch_length: i32,
) -> Result<bool, DomainError> {
    epoch_for_block_number(evidence_block_number, epoch_length)
        .map(|evidence_epoch| target_activation_epoch == evidence_epoch)
}

pub fn slash_target_has_positive_bond(bond: i64) -> bool { bond > 0 }

pub fn slash_target_key(
    offender: &Validator,
    target_activation_epoch: Epoch,
) -> (Validator, Epoch) {
    (offender.clone(), target_activation_epoch)
}

pub fn slash_target_key_collides<T: Eq>(
    left_offender: &T,
    left_epoch: Epoch,
    right_offender: &T,
    right_epoch: Epoch,
) -> bool {
    left_offender == right_offender && left_epoch == right_epoch
}

/// Core authorization predicate: a `Slash` system deploy is admissible iff
/// all four conditions hold simultaneously —
/// 1. the deploy's `target_activation_epoch` equals the *current* epoch
///    (computed from `reference_block_number`),
/// 2. the *evidence* block's epoch equals the same `target_activation_epoch`
///    (so the proposer cannot reuse stale evidence under a fresh epoch label),
/// 3. the offender carries a positive bond, and
/// 4. the referenced block is flagged invalid in the DAG.
///
/// Returns `None` only when the domain conditions of `epoch_for_block_number`
/// fail (non-positive `epoch_length` or negative block number). The
/// conjunction is the precondition proven sufficient by Theorem T-9.13
/// (`formal/rocq/slashing/theories/BugFixSlashAuthorization.v`) and modeled
/// in `kani_proofs::received_slash_deploy_authorized_*`.
pub fn received_slash_deploy_authorized(
    reference_block_number: i64,
    evidence_block_number: i64,
    target_activation_epoch: Epoch,
    epoch_length: i32,
    bond: i64,
    invalid: bool,
) -> Result<bool, DomainError> {
    let current = slash_target_epoch_is_current(
        reference_block_number,
        target_activation_epoch,
        epoch_length,
    )?;
    let evidence = slash_evidence_epoch_matches_target(
        evidence_block_number,
        target_activation_epoch,
        epoch_length,
    )?;
    Ok(current && evidence && slash_target_has_positive_bond(bond) && invalid)
}

fn evidence_epoch(metadata: &BlockMetadata, epoch_length: i32) -> Result<Epoch, DomainError> {
    epoch_for_block_number(metadata.block_number, epoch_length)
}

fn objective_evidence_pair_authorized(
    first_hash: &BlockHash,
    first: &BlockMetadata,
    second_hash: &BlockHash,
    second: &BlockMetadata,
    target_epoch: Epoch,
    target_generation: BondGeneration,
    epoch_length: i32,
    authority: &CanonicalSlashAuthority,
) -> Result<(), SlashAuthError> {
    if first_hash >= second_hash {
        return Err(SlashAuthError::NonCanonicalEquivocationPair {
            first: hex::encode(first_hash),
            second: hex::encode(second_hash),
        });
    }
    if first.sender != second.sender
        || first.sequence_number != second.sequence_number
        || first.sequence_number < 0
    {
        return Err(SlashAuthError::EquivocationEvidenceMismatch);
    }
    if first.sender_bond_generation() != Some(target_generation)
        || second.sender_bond_generation() != Some(target_generation)
        || authority.generation(&first.sender) != Some(target_generation)
    {
        return Err(SlashAuthError::BondGenerationMismatch {
            validator: hex::encode(&first.sender),
        });
    }
    for metadata in [first, second] {
        let epoch = evidence_epoch(metadata, epoch_length).map_err(SlashAuthError::from)?;
        if epoch != target_epoch {
            return Err(SlashAuthError::EvidenceEpochMismatch {
                evidence_epoch: epoch,
                target_epoch,
            });
        }
    }
    if authority.bond(&first.sender) <= 0 {
        return Err(SlashAuthError::TargetNotBonded {
            validator: hex::encode(&first.sender),
        });
    }
    Ok(())
}

pub fn has_slash_evidence(snapshot: &CasperSnapshot) -> bool {
    snapshot
        .dag
        .invalid_blocks()
        .iter()
        .any(BlockMetadata::is_rejected)
        || snapshot
            .dag
            .equivocation_observations()
            .values()
            .any(|hashes| hashes.len() >= 2)
}

/// Proposer-side enumeration of slash candidates for the block being built.
///
/// At most one candidate per offender is emitted. A canonical objective pair
/// takes precedence over unary local-invalid evidence. Once a structural pair
/// exists, unary fallback from that `(offender, sequence)` is suppressed even
/// when the pair crosses an epoch boundary and is therefore ineligible. An
/// independent unary fault at another sequence remains eligible. Eligible
/// pairs and unary candidates use lexicographic tie-breaking so every node
/// selects the same candidate set from the same immutable evidence.
///
/// `proposed_block_num` is the actual number of the block being built, so
/// `current_epoch` is the epoch the new block will land in. Slashing decisions
/// belong to that epoch and never infer it from a mutable snapshot maximum.
pub fn authorized_slash_candidates(
    snapshot: &CasperSnapshot,
    proposed_block_num: i64,
    authority: &CanonicalSlashAuthority,
) -> Result<Vec<AuthorizedSlashCandidate>, CasperError> {
    let epoch_length = snapshot.on_chain_state.shard_conf.epoch_length;
    // Phase 9 (C-6): `epoch_for_block_number` now returns
    // `Result<i64, DomainError>`. Map directly to the corresponding
    // `SlashAuthError` (and on into `CasperError::SlashAuth` via the
    // `From` impl in `errors.rs`).
    let current_epoch =
        epoch_for_block_number(proposed_block_num, epoch_length).map_err(SlashAuthError::from)?;

    // BTreeMap (not HashMap) gives deterministic iteration order across nodes;
    // the resulting Vec is what feeds the block body.
    let mut by_offender: BTreeMap<Validator, AuthorizedSlashCandidate> = BTreeMap::new();
    let mut objective_authorized_offenders = BTreeSet::new();
    let structural_equivocation_keys = snapshot.dag.structural_equivocation_keys();
    for ((validator, generation, sequence), hashes) in snapshot.dag.equivocation_observations() {
        if hashes.len() < 2
            || sequence < 0
            || authority.generation(&validator) != Some(generation)
            || authority.bond(&validator) <= 0
        {
            continue;
        }
        let mut current_epoch_evidence = Vec::new();
        for hash in hashes {
            let metadata = snapshot
                .dag
                .lookup(&hash)
                .map_err(CasperError::from)?
                .ok_or_else(|| {
                    CasperError::KvStoreError(KvStoreError::InvalidArgument(format!(
                        "equivocation evidence index references unknown block {}",
                        hex::encode(&hash)
                    )))
                })?;
            if metadata.sender != validator
                || metadata.sender_bond_generation() != Some(generation)
                || metadata.sequence_number != sequence
            {
                return Err(CasperError::KvStoreError(KvStoreError::InvalidArgument(
                    format!(
                        "equivocation evidence index key disagrees with block metadata for {}",
                        hex::encode(&hash)
                    ),
                )));
            }
            let Ok(epoch) = evidence_epoch(&metadata, epoch_length) else {
                continue;
            };
            if epoch == current_epoch {
                current_epoch_evidence.push((hash, metadata));
            }
        }
        if current_epoch_evidence.len() < 2 {
            continue;
        }
        let (first_hash, first) = &current_epoch_evidence[0];
        let (second_hash, second) = &current_epoch_evidence[1];
        objective_evidence_pair_authorized(
            first_hash,
            first,
            second_hash,
            second,
            current_epoch,
            generation,
            epoch_length,
            authority,
        )
        .map_err(CasperError::from)?;
        let candidate = AuthorizedSlashCandidate {
            offender: validator.clone(),
            invalid_block_hash: first_hash.clone(),
            equivocation_block_hash: Some(second_hash.clone()),
            target_activation_epoch: current_epoch,
            target_bond_generation: generation,
        };
        match by_offender.entry(validator) {
            Entry::Vacant(entry) => {
                entry.insert(candidate);
            }
            Entry::Occupied(mut entry) => {
                let existing = entry.get();
                if (
                    &candidate.invalid_block_hash,
                    &candidate.equivocation_block_hash,
                ) < (
                    &existing.invalid_block_hash,
                    &existing.equivocation_block_hash,
                ) {
                    entry.insert(candidate);
                }
            }
        }
        objective_authorized_offenders.insert(first.sender.clone());
    }
    for metadata in snapshot.dag.invalid_blocks() {
        if !metadata.is_rejected() {
            continue;
        }
        if structural_equivocation_keys.contains(&(
            metadata.sender.clone(),
            metadata
                .sender_bond_generation()
                .unwrap_or(BondGeneration::GENESIS),
            metadata.sequence_number,
        )) || objective_authorized_offenders.contains(&metadata.sender)
        {
            continue;
        }
        // Phase 9 (C-6): skip blocks whose own (sender's) metadata has a
        // domain-invalid block number — protocol invariant says this
        // can't happen for already-stored blocks, but the typed Result
        // makes the explicit-skip choice visible. We `warn!` because a
        // hit here means either the configured `epoch_length` is invalid
        // (operator misconfiguration) or the metadata store contains a
        // domain-invalid record (corruption). Silently producing an empty
        // slash list would mask both modes.
        let target_activation_epoch = match evidence_epoch(&metadata, epoch_length) {
            Ok(epoch) => epoch,
            Err(e) => {
                tracing::warn!(
                    "authorized_slash_candidates: skipping invalid-block metadata for {} \
                     (sender {}): {}; check `epoch_length` configuration",
                    hex::encode(&metadata.block_hash),
                    hex::encode(&metadata.sender),
                    e
                );
                continue;
            }
        };
        if target_activation_epoch != current_epoch {
            continue;
        }
        let Some(target_bond_generation) = metadata.sender_bond_generation() else {
            continue;
        };
        if authority.generation(&metadata.sender) != Some(target_bond_generation) {
            continue;
        }
        let bond = authority.bond(&metadata.sender);
        if bond <= 0 {
            continue;
        }
        let candidate = AuthorizedSlashCandidate {
            offender: metadata.sender.clone(),
            invalid_block_hash: metadata.block_hash.clone(),
            equivocation_block_hash: None,
            target_activation_epoch,
            target_bond_generation,
        };
        match by_offender.entry(metadata.sender.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(candidate);
            }
            Entry::Occupied(mut entry) => {
                // Deterministic tie-break: keep the lex-smallest hash so every
                // node selects the same evidence block for this offender.
                if candidate.invalid_block_hash < entry.get().invalid_block_hash {
                    entry.insert(candidate);
                }
            }
        }
    }

    Ok(by_offender.into_values().collect())
}

/// Receive-side mirror of [`authorized_slash_candidates`]. Every successful
/// `Slash` system deploy in `block` must satisfy the common authorization
/// rules below. Any violation returns `Err` and the caller
/// (`Validate::slash_deploy_authorization`) collapses it into
/// `InvalidBlock::UnauthorizedSlashDeploy`:
///
/// 1. The deploy issuer must equal the block sender.
/// 2. `target_activation_epoch` must equal the *current* epoch of the
///    receiving block (so a slash cannot reference a different epoch's rules).
/// 3. Every evidence hash must resolve to a known block in the DAG.
/// 4. Unary evidence must be locally invalid; pair evidence must be distinct,
///    canonically ordered, and identify one sender and sequence number.
/// 5. Every evidence block's epoch must equal `target_activation_epoch`.
/// 6. The evidence generation must equal the canonical pre-state generation.
/// 7. The offender must carry a positive bond at that same pre-state root.
/// 8. No two slashes in the same block may share `(offender, target_generation)`.
///
/// See `docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.14` and
/// the Rocq proof in `formal/rocq/slashing/theories/BugFixSlashAuthorization.v`.
pub fn validate_received_slash_deploys(
    block: &BlockMessage,
    snapshot: &CasperSnapshot,
    authority: &CanonicalSlashAuthority,
) -> Result<(), CasperError> {
    let has_slash_deploy = block.body.system_deploys.iter().any(|system_deploy| {
        matches!(system_deploy, ProcessedSystemDeploy::Succeeded {
            system_deploy: SystemDeployData::Slash { .. },
            ..
        })
    });
    // Fast path: most blocks contain no slash deploys; avoid the per-deploy
    // loop and the epoch division (which can fail on an invalid epoch_length).
    if !has_slash_deploy {
        return Ok(());
    }

    let epoch_length = snapshot.on_chain_state.shard_conf.epoch_length;
    let current_epoch = epoch_for_block_number(block.body.state.block_number, epoch_length)
        .map_err(SlashAuthError::from)?;
    // BTreeMap gives deterministic iteration order for the error path; the
    // key `(offender, target_generation)` is the uniqueness rule from item 8.
    let mut seen = BTreeMap::<(Validator, BondGeneration), BlockHash>::new();

    // Defensive check — block sequence numbers must be non-negative.
    // `debug_assert!` would compile out in release; a tampered wire-protocol
    // block with `seq_num < 0` would then propagate into
    // `received_slash_deploy_authorized` (whose `epoch_for_block_number`
    // checks `block_number`, not `seq_num`). Returning a typed error makes
    // the rejection release-safe — and the `From<SlashAuthError>` impl at
    // `errors.rs:52` routes it correctly through `slash_deploy_authorization`
    // so the block is recorded as `UnauthorizedSlashDeploy`.
    if block.seq_num < 0 {
        return Err(SlashAuthError::NegativeSequenceNumber {
            block_hash: hex::encode(&block.block_hash),
            seq_num: block.seq_num,
        }
        .into());
    }

    for system_deploy in &block.body.system_deploys {
        let ProcessedSystemDeploy::Succeeded {
            system_deploy:
                SystemDeployData::Slash {
                    invalid_block_hash,
                    equivocation_block_hash,
                    issuer_public_key,
                    target_activation_epoch,
                    target_bond_generation,
                },
            ..
        } = system_deploy
        else {
            continue;
        };

        // P4-9: issuer public key must be present and well-formed before
        // we even consider matching it against the sender. A zero-length
        // key would silently match a malformed `block.sender` of the same
        // shape; the check below catches the (uncommon) case.
        if issuer_public_key.bytes.is_empty() {
            return Err(SlashAuthError::IssuerMismatch {
                block_hash: hex::encode(&block.block_hash),
                issuer: "<empty>".to_string(),
                sender: hex::encode(&block.sender),
            }
            .into());
        }

        if issuer_public_key.bytes != block.sender {
            return Err(SlashAuthError::IssuerMismatch {
                block_hash: hex::encode(&block.block_hash),
                issuer: hex::encode(&issuer_public_key.bytes),
                sender: hex::encode(&block.sender),
            }
            .into());
        }
        // Phase 10 (C-5): convert the protobuf-side raw `i64` to `Epoch`
        // at the boundary; downstream arithmetic and comparisons are typed.
        let target_activation_epoch = Epoch::from(*target_activation_epoch);
        if target_activation_epoch != current_epoch {
            return Err(SlashAuthError::EpochMismatch {
                target: target_activation_epoch,
                current: current_epoch,
            }
            .into());
        }

        let metadata = snapshot
            .dag
            .lookup(invalid_block_hash)
            .map_err(CasperError::KvStoreError)?
            .ok_or_else(|| SlashAuthError::ReferencesUnknownBlock {
                hash: hex::encode(invalid_block_hash),
            })?;

        if let Some(equivocation_block_hash) = equivocation_block_hash {
            let conflicting = snapshot
                .dag
                .lookup(equivocation_block_hash)
                .map_err(CasperError::from)?
                .ok_or_else(|| SlashAuthError::ReferencesUnknownBlock {
                    hash: hex::encode(equivocation_block_hash),
                })?;
            objective_evidence_pair_authorized(
                invalid_block_hash,
                &metadata,
                equivocation_block_hash,
                &conflicting,
                target_activation_epoch,
                *target_bond_generation,
                epoch_length,
                authority,
            )?;
        } else {
            if !metadata.is_rejected() {
                return Err(SlashAuthError::ReferencesValidBlock {
                    hash: hex::encode(invalid_block_hash),
                }
                .into());
            }
            if metadata.sender_bond_generation() != Some(*target_bond_generation)
                || authority.generation(&metadata.sender) != Some(*target_bond_generation)
            {
                return Err(SlashAuthError::BondGenerationMismatch {
                    validator: hex::encode(&metadata.sender),
                }
                .into());
            }
            let evidence_epoch =
                evidence_epoch(&metadata, epoch_length).map_err(SlashAuthError::from)?;
            if evidence_epoch != target_activation_epoch {
                return Err(SlashAuthError::EvidenceEpochMismatch {
                    evidence_epoch,
                    target_epoch: target_activation_epoch,
                }
                .into());
            }
            let bond = authority.bond(&metadata.sender);
            if bond <= 0 {
                return Err(SlashAuthError::TargetNotBonded {
                    validator: hex::encode(&metadata.sender),
                }
                .into());
            }
            let authorized = received_slash_deploy_authorized(
                block.body.state.block_number,
                metadata.block_number,
                target_activation_epoch,
                epoch_length,
                bond,
                metadata.is_rejected(),
            )
            .map_err(SlashAuthError::from)?;
            if !authorized {
                return Err(SlashAuthError::NotAuthorizedByEvidence {
                    validator: hex::encode(&metadata.sender),
                }
                .into());
            }
        }

        let key = (metadata.sender.clone(), *target_bond_generation);
        if seen.insert(key, invalid_block_hash.clone()).is_some() {
            return Err(SlashAuthError::DuplicateTarget {
                validator: hex::encode(&metadata.sender),
                generation: *target_bond_generation,
                epoch: target_activation_epoch,
            }
            .into());
        }
    }

    Ok(())
}

// Kani proofs — symbolic-model-check the boundary helpers against the
// post-Phase-9 typed API. Production functions now return
// `Result<_, DomainError>` (where applicable) and use the `Epoch` newtype.
// CI does not currently run kani; run locally with `cargo kani -p casper`.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    #[kani::proof]
    fn checked_base_seq_rejects_nonpositive() {
        let seq: i32 = kani::any();
        kani::assume(seq <= 0);
        // Still `Option`-typed in production — i32 invariants live here.
        assert_eq!(checked_base_seq(seq), None);
    }

    #[kani::proof]
    fn checked_base_seq_matches_positive_i32_predecessor() {
        let seq: i32 = kani::any();
        kani::assume(seq > 0);
        assert_eq!(checked_base_seq(seq), Some(seq - 1));
    }

    #[kani::proof]
    fn checked_next_seq_matches_i32_successor() {
        let seq: u64 = kani::any();
        let expected = seq.checked_add(1).and_then(|next| i32::try_from(next).ok());
        assert_eq!(checked_next_seq(seq), expected);
    }

    #[kani::proof]
    fn epoch_for_block_number_rejects_invalid_domain() {
        let block_number: i64 = kani::any();
        let epoch_length: i32 = kani::any();
        kani::assume(block_number < 0 || epoch_length <= 0);
        let result = epoch_for_block_number(block_number, epoch_length);
        assert!(result.is_err());
        // Phase 9 (C-6) typed DomainError variants — assert specific routing.
        if epoch_length <= 0 {
            assert!(matches!(result, Err(DomainError::InvalidEpochLength(_))));
        } else {
            assert!(matches!(result, Err(DomainError::NegativeBlockNumber(_))));
        }
    }

    #[kani::proof]
    fn epoch_for_block_number_matches_bounded_floor_division() {
        let block_number: u16 = kani::any();
        let epoch_length: u8 = kani::any();
        kani::assume(epoch_length > 0);
        let block_number = i64::from(block_number);
        let epoch_length = i32::from(epoch_length);
        assert_eq!(
            epoch_for_block_number(block_number, epoch_length),
            Ok(Epoch::new(block_number / i64::from(epoch_length)))
        );
    }

    #[kani::proof]
    fn slash_target_epoch_is_current_matches_epoch_projection() {
        let reference_block_number: u16 = kani::any();
        let target_activation_epoch: i16 = kani::any();
        let epoch_length: u8 = kani::any();
        kani::assume(epoch_length > 0);
        let reference_block_number = i64::from(reference_block_number);
        let target_activation_epoch_raw = i64::from(target_activation_epoch);
        let target_activation_epoch = Epoch::new(target_activation_epoch_raw);
        let epoch_length = i32::from(epoch_length);
        let expected =
            target_activation_epoch_raw == reference_block_number / i64::from(epoch_length);
        assert_eq!(
            slash_target_epoch_is_current(
                reference_block_number,
                target_activation_epoch,
                epoch_length
            ),
            Ok(expected)
        );
    }

    #[kani::proof]
    fn slash_evidence_epoch_matches_target_matches_epoch_projection() {
        let evidence_block_number: u16 = kani::any();
        let target_activation_epoch: i16 = kani::any();
        let epoch_length: u8 = kani::any();
        kani::assume(epoch_length > 0);
        let evidence_block_number = i64::from(evidence_block_number);
        let target_activation_epoch_raw = i64::from(target_activation_epoch);
        let target_activation_epoch = Epoch::new(target_activation_epoch_raw);
        let epoch_length = i32::from(epoch_length);
        let expected =
            target_activation_epoch_raw == evidence_block_number / i64::from(epoch_length);
        assert_eq!(
            slash_evidence_epoch_matches_target(
                evidence_block_number,
                target_activation_epoch,
                epoch_length
            ),
            Ok(expected)
        );
    }

    #[kani::proof]
    fn received_slash_deploy_authorized_rejects_invalid_domain() {
        let reference_block_number: i64 = kani::any();
        let evidence_block_number: i64 = kani::any();
        let target_activation_epoch: i64 = kani::any();
        let epoch_length: i32 = kani::any();
        let bond: i64 = kani::any();
        let invalid: bool = kani::any();
        kani::assume(reference_block_number < 0 || evidence_block_number < 0 || epoch_length <= 0);
        assert!(received_slash_deploy_authorized(
            reference_block_number,
            evidence_block_number,
            Epoch::new(target_activation_epoch),
            epoch_length,
            bond,
            invalid
        )
        .is_err());
    }

    #[kani::proof]
    fn received_slash_deploy_authorized_is_conjunction_on_bounded_domain() {
        let reference_block_number: u16 = kani::any();
        let evidence_block_number: u16 = kani::any();
        let target_activation_epoch: i16 = kani::any();
        let epoch_length: u8 = kani::any();
        let bond: i16 = kani::any();
        let invalid: bool = kani::any();
        kani::assume(epoch_length > 0);
        let reference_block_number = i64::from(reference_block_number);
        let evidence_block_number = i64::from(evidence_block_number);
        let target_activation_epoch_raw = i64::from(target_activation_epoch);
        let target_activation_epoch = Epoch::new(target_activation_epoch_raw);
        let epoch_length = i32::from(epoch_length);
        let bond = i64::from(bond);
        let expected = target_activation_epoch_raw
            == reference_block_number / i64::from(epoch_length)
            && target_activation_epoch_raw == evidence_block_number / i64::from(epoch_length)
            && bond > 0
            && invalid;
        assert_eq!(
            received_slash_deploy_authorized(
                reference_block_number,
                evidence_block_number,
                target_activation_epoch,
                epoch_length,
                bond,
                invalid
            ),
            Ok(expected)
        );
    }

    #[kani::proof]
    fn slash_target_has_positive_bond_matches_positive() {
        let bond: i64 = kani::any();
        assert_eq!(slash_target_has_positive_bond(bond), bond > 0);
    }

    #[kani::proof]
    fn received_authorization_requires_positive_bond_on_bounded_domain() {
        let reference_block_number: u16 = kani::any();
        let evidence_block_number: u16 = kani::any();
        let epoch_length: u8 = kani::any();
        let bond: i16 = kani::any();
        kani::assume(epoch_length > 0);
        kani::assume(bond <= 0);
        let reference_block_number = i64::from(reference_block_number);
        let evidence_block_number = i64::from(evidence_block_number);
        let epoch_length = i32::from(epoch_length);
        let target_epoch_raw = reference_block_number / i64::from(epoch_length);
        kani::assume(target_epoch_raw == evidence_block_number / i64::from(epoch_length));
        assert_eq!(
            received_slash_deploy_authorized(
                reference_block_number,
                evidence_block_number,
                Epoch::new(target_epoch_raw),
                epoch_length,
                i64::from(bond),
                true
            ),
            Ok(false)
        );
    }

    #[kani::proof]
    fn received_authorization_requires_invalid_evidence_on_bounded_domain() {
        let reference_block_number: u16 = kani::any();
        let evidence_block_number: u16 = kani::any();
        let epoch_length: u8 = kani::any();
        let bond: u16 = kani::any();
        kani::assume(epoch_length > 0);
        kani::assume(bond > 0);
        let reference_block_number = i64::from(reference_block_number);
        let evidence_block_number = i64::from(evidence_block_number);
        let epoch_length = i32::from(epoch_length);
        let target_epoch_raw = reference_block_number / i64::from(epoch_length);
        kani::assume(target_epoch_raw == evidence_block_number / i64::from(epoch_length));
        assert_eq!(
            received_slash_deploy_authorized(
                reference_block_number,
                evidence_block_number,
                Epoch::new(target_epoch_raw),
                epoch_length,
                i64::from(bond),
                false
            ),
            Ok(false)
        );
    }

    #[kani::proof]
    fn received_authorization_requires_current_epoch_on_bounded_domain() {
        let reference_block_number: u16 = kani::any();
        let evidence_block_number: u16 = kani::any();
        let target_activation_epoch: i16 = kani::any();
        let epoch_length: u8 = kani::any();
        let bond: u16 = kani::any();
        kani::assume(epoch_length > 0);
        kani::assume(bond > 0);
        let reference_block_number = i64::from(reference_block_number);
        let evidence_block_number = i64::from(evidence_block_number);
        let target_epoch_raw = i64::from(target_activation_epoch);
        let epoch_length = i32::from(epoch_length);
        kani::assume(target_epoch_raw != reference_block_number / i64::from(epoch_length));
        assert_eq!(
            received_slash_deploy_authorized(
                reference_block_number,
                evidence_block_number,
                Epoch::new(target_epoch_raw),
                epoch_length,
                i64::from(bond),
                true
            ),
            Ok(false)
        );
    }

    #[kani::proof]
    fn received_authorization_requires_evidence_epoch_on_bounded_domain() {
        let reference_block_number: u16 = kani::any();
        let evidence_block_number: u16 = kani::any();
        let epoch_length: u8 = kani::any();
        let bond: u16 = kani::any();
        kani::assume(epoch_length > 0);
        kani::assume(bond > 0);
        let reference_block_number = i64::from(reference_block_number);
        let evidence_block_number = i64::from(evidence_block_number);
        let epoch_length = i32::from(epoch_length);
        let target_epoch_raw = reference_block_number / i64::from(epoch_length);
        kani::assume(target_epoch_raw != evidence_block_number / i64::from(epoch_length));
        assert_eq!(
            received_slash_deploy_authorized(
                reference_block_number,
                evidence_block_number,
                Epoch::new(target_epoch_raw),
                epoch_length,
                i64::from(bond),
                true
            ),
            Ok(false)
        );
    }

    #[kani::proof]
    fn slash_target_key_collides_matches_pair_equality() {
        let left_offender: u8 = kani::any();
        let right_offender: u8 = kani::any();
        let left_epoch: i16 = kani::any();
        let right_epoch: i16 = kani::any();
        assert_eq!(
            slash_target_key_collides(
                &left_offender,
                Epoch::new(i64::from(left_epoch)),
                &right_offender,
                Epoch::new(i64::from(right_epoch))
            ),
            left_offender == right_offender && left_epoch == right_epoch
        );
    }
}
