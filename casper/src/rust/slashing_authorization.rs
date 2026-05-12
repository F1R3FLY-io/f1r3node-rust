//! Authorization predicate for `Slash` system deploys.
//!
//! Every `Slash` system deploy carried in a block must reference current-epoch
//! invalid-block evidence whose offender is positively bonded. This module
//! provides both halves of that contract:
//!
//! * [`authorized_slash_candidates`] — the proposer-side enumeration the block
//!   creator uses to decide which slashes to mint.
//! * [`validate_received_slash_deploys`] — the receive-side check that mirrors
//!   the predicate and rejects unauthorized slashes with
//!   `InvalidBlock::UnauthorizedSlashDeploy`.
//!
//! The conjunctive predicate `received_slash_deploy_authorized` (current epoch
//! ∧ matching evidence epoch ∧ positive bond ∧ block flagged invalid) is the
//! precondition proven sufficient by Theorem T-9.8 (see
//! `formal/rocq/slashing/theories/BugFixSlashAuthorization.v` and
//! `docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.8`).
//!
//! Boundary helpers (`checked_base_seq`, `checked_next_seq`,
//! `epoch_for_block_number`) live here because their failure modes feed back
//! into the same authorization decision; they are also the surface that the
//! `kani_proofs` module models exhaustively at the bottom of the file.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;

use crate::rust::casper::CasperSnapshot;
use crate::rust::epoch::Epoch;
use crate::rust::errors::CasperError;

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
/// the conjunctive predicate from Theorem T-9.8 is preserved one
/// variant per conjunct.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SlashAuthError {
    #[error("slash deploy issuer does not match block sender (block={block_hash}, issuer={issuer}, sender={sender})")]
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
    #[error("invalid epoch length {0}")]
    InvalidEpochLength(i32),
    #[error("negative block number {0}")]
    NegativeBlockNumber(i64),
    #[error(
        "slash deploy epoch ({evidence_epoch}) does not match invalid-block evidence epoch ({target_epoch})"
    )]
    EvidenceEpochMismatch { evidence_epoch: Epoch, target_epoch: Epoch },
    #[error("slash deploy target {validator} is not currently bonded")]
    TargetNotBonded { validator: String },
    #[error("slash deploy is not authorized by current invalid-block evidence (validator {validator})")]
    NotAuthorizedByEvidence { validator: String },
    #[error("duplicate slash deploy target in block (validator {validator}, epoch {epoch})")]
    DuplicateTarget { validator: String, epoch: Epoch },
}

// Phase 9 (R-2): `From<SlashAuthError> for CasperError` now lives in
// `errors.rs` and routes to the new structured `CasperError::SlashAuth`
// variant — previous stringification at this boundary defeated the
// typed-error effort.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedSlashCandidate {
    pub offender: Validator,
    pub invalid_block_hash: BlockHash,
    /// Epoch under which the slash takes effect. By construction this equals
    /// the epoch of the offender's invalid block at commit time — the
    /// receiver reconstructs it from that evidence (see
    /// `slash_evidence_epoch_matches_target`), so the proposer cannot move the
    /// slash to a different epoch.
    ///
    /// Phase 10 (C-5): typed [`Epoch`] newtype replaces the raw `i64`;
    /// conversion at the protobuf boundary uses
    /// `Epoch::from(slash_deploy.target_activation_epoch)`.
    pub target_activation_epoch: Epoch,
}

/// Phase 9 (C-6): returns a typed [`DomainError`] when the input
/// constraints fail. The two failure modes are now distinguishable:
/// non-positive `epoch_length` is configuration-derived, while a
/// negative `block_number` indicates an arithmetic or wire-format bug.
/// The caller (`authorized_slash_candidates` /
/// `validate_received_slash_deploys`) maps either into the appropriate
/// `SlashAuthError` variant; never panic here — shard configuration can
/// legally hand us `epoch_length == 0` at startup.
pub fn epoch_for_block_number(
    block_number: i64,
    epoch_length: i32,
) -> Result<Epoch, DomainError> {
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
/// conjunction is the precondition proven sufficient by Theorem T-9.8
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

/// Proposer-side enumeration of slash candidates for the block being built.
///
/// At most one candidate per offender is emitted, even when the DAG contains
/// multiple invalid blocks from the same validator in the current epoch — the
/// receiver is required to enforce that uniqueness, so the proposer must
/// mirror it. When two evidence blocks tie, we keep the one with the
/// lexicographically smallest `invalid_block_hash`; this tie-break is
/// load-bearing for cross-node replay determinism (every node must select the
/// same candidate set from the same snapshot).
///
/// `max_block_num + 1` is the block number of the block we are *about to
/// propose*, not the latest existing block — so `current_epoch` is the epoch
/// the new block will land in. Slashing decisions belong to that epoch.
pub fn authorized_slash_candidates(
    snapshot: &CasperSnapshot,
) -> Result<Vec<AuthorizedSlashCandidate>, CasperError> {
    let epoch_length = snapshot.on_chain_state.shard_conf.epoch_length;
    // P2-9: surface overflow on `max_block_num + 1` as a typed error.
    let proposed_block_num = snapshot.max_block_num.checked_add(1).ok_or_else(|| {
        CasperError::RuntimeError(format!(
            "max_block_num overflow: {} + 1 wraps i64",
            snapshot.max_block_num
        ))
    })?;
    // Phase 9 (C-6): `epoch_for_block_number` now returns
    // `Result<i64, DomainError>`. Map directly to the corresponding
    // `SlashAuthError` (and on into `CasperError::SlashAuth` via the
    // `From` impl in `errors.rs`).
    let current_epoch = epoch_for_block_number(proposed_block_num, epoch_length)
        .map_err(SlashAuthError::from)?;

    // BTreeMap (not HashMap) gives deterministic iteration order across nodes;
    // the resulting Vec is what feeds the block body.
    let mut by_offender: BTreeMap<Validator, AuthorizedSlashCandidate> = BTreeMap::new();
    for metadata in snapshot.dag.invalid_blocks() {
        if !metadata.invalid {
            continue;
        }
        // Phase 9 (C-6): skip blocks whose own (sender's) metadata has a
        // domain-invalid block number — protocol invariant says this
        // can't happen for already-stored blocks, but the typed Result
        // makes the explicit-skip choice visible.
        let Ok(target_activation_epoch) = evidence_epoch(&metadata, epoch_length) else {
            continue;
        };
        if target_activation_epoch != current_epoch {
            continue;
        }
        let bond = snapshot
            .on_chain_state
            .bonds_map
            .get(&metadata.sender)
            .copied()
            .unwrap_or(0);
        if bond <= 0 {
            continue;
        }
        let candidate = AuthorizedSlashCandidate {
            offender: metadata.sender.clone(),
            invalid_block_hash: metadata.block_hash.clone(),
            target_activation_epoch,
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
/// `Slash` system deploy in `block` must satisfy seven rules; any violation
/// returns `Err` and the caller (`Validate::slash_deploy_authorization`)
/// collapses that into `InvalidBlock::UnauthorizedSlashDeploy`:
///
/// 1. The deploy issuer must equal the block sender.
/// 2. `target_activation_epoch` must equal the *current* epoch of the
///    receiving block (so a slash cannot reference a different epoch's rules).
/// 3. `invalid_block_hash` must resolve to a known block in the DAG.
/// 4. That block must be flagged `invalid`.
/// 5. The evidence block's own epoch must equal `target_activation_epoch`.
/// 6. The offender must currently carry a positive bond.
/// 7. No two slashes in the same block may share `(offender, target_epoch)`.
///
/// See `docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.8` and
/// the Rocq proof in `formal/rocq/slashing/theories/BugFixSlashAuthorization.v`.
pub fn validate_received_slash_deploys(
    block: &BlockMessage,
    snapshot: &CasperSnapshot,
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
    // key `(offender, target_epoch)` is the uniqueness rule from item (7).
    // Phase 10 (C-5): typed `Epoch` key replaces raw `i64`.
    let mut seen = BTreeMap::<(Validator, Epoch), BlockHash>::new();

    // P4-9: defensive check — block sequence numbers must be non-negative
    // (a negative seq num would indicate a malformed or tampered block; the
    // protocol invariant is `seq_num >= 0`). We assert here rather than
    // returning an error because this is a class-of-input bug, not a
    // protocol-level violation.
    debug_assert!(
        block.seq_num >= 0,
        "block.seq_num must be non-negative; got {}",
        block.seq_num
    );

    for system_deploy in &block.body.system_deploys {
        let ProcessedSystemDeploy::Succeeded {
            system_deploy:
                SystemDeployData::Slash {
                    invalid_block_hash,
                    issuer_public_key,
                    target_activation_epoch,
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

        if !metadata.invalid {
            return Err(SlashAuthError::ReferencesValidBlock {
                hash: hex::encode(invalid_block_hash),
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

        let bond = snapshot
            .on_chain_state
            .bonds_map
            .get(&metadata.sender)
            .copied()
            .unwrap_or(0);
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
            metadata.invalid,
        )
        .map_err(SlashAuthError::from)?;
        if !authorized {
            return Err(SlashAuthError::NotAuthorizedByEvidence {
                validator: hex::encode(&metadata.sender),
            }
            .into());
        }

        let key = slash_target_key(&metadata.sender, target_activation_epoch);
        if seen.insert(key, invalid_block_hash.clone()).is_some() {
            return Err(SlashAuthError::DuplicateTarget {
                validator: hex::encode(&metadata.sender),
                epoch: target_activation_epoch,
            }
            .into());
        }
    }

    Ok(())
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    #[kani::proof]
    fn checked_base_seq_rejects_nonpositive() {
        let seq: i32 = kani::any();
        kani::assume(seq <= 0);
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
        assert_eq!(epoch_for_block_number(block_number, epoch_length), None);
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
            Some(block_number / i64::from(epoch_length))
        );
    }

    #[kani::proof]
    fn slash_target_epoch_is_current_matches_epoch_projection() {
        let reference_block_number: u16 = kani::any();
        let target_activation_epoch: i16 = kani::any();
        let epoch_length: u8 = kani::any();
        kani::assume(epoch_length > 0);
        let reference_block_number = i64::from(reference_block_number);
        let target_activation_epoch = i64::from(target_activation_epoch);
        let epoch_length = i32::from(epoch_length);
        let expected = target_activation_epoch == reference_block_number / i64::from(epoch_length);
        assert_eq!(
            slash_target_epoch_is_current(
                reference_block_number,
                target_activation_epoch,
                epoch_length
            ),
            Some(expected)
        );
    }

    #[kani::proof]
    fn slash_evidence_epoch_matches_target_matches_epoch_projection() {
        let evidence_block_number: u16 = kani::any();
        let target_activation_epoch: i16 = kani::any();
        let epoch_length: u8 = kani::any();
        kani::assume(epoch_length > 0);
        let evidence_block_number = i64::from(evidence_block_number);
        let target_activation_epoch = i64::from(target_activation_epoch);
        let epoch_length = i32::from(epoch_length);
        let expected = target_activation_epoch == evidence_block_number / i64::from(epoch_length);
        assert_eq!(
            slash_evidence_epoch_matches_target(
                evidence_block_number,
                target_activation_epoch,
                epoch_length
            ),
            Some(expected)
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
        assert_eq!(
            received_slash_deploy_authorized(
                reference_block_number,
                evidence_block_number,
                target_activation_epoch,
                epoch_length,
                bond,
                invalid
            ),
            None
        );
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
        let target_activation_epoch = i64::from(target_activation_epoch);
        let epoch_length = i32::from(epoch_length);
        let bond = i64::from(bond);
        let expected = target_activation_epoch == reference_block_number / i64::from(epoch_length)
            && target_activation_epoch == evidence_block_number / i64::from(epoch_length)
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
            Some(expected)
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
        let target_activation_epoch = reference_block_number / i64::from(epoch_length);
        kani::assume(target_activation_epoch == evidence_block_number / i64::from(epoch_length));
        assert_eq!(
            received_slash_deploy_authorized(
                reference_block_number,
                evidence_block_number,
                target_activation_epoch,
                epoch_length,
                i64::from(bond),
                true
            ),
            Some(false)
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
        let target_activation_epoch = reference_block_number / i64::from(epoch_length);
        kani::assume(target_activation_epoch == evidence_block_number / i64::from(epoch_length));
        assert_eq!(
            received_slash_deploy_authorized(
                reference_block_number,
                evidence_block_number,
                target_activation_epoch,
                epoch_length,
                i64::from(bond),
                false
            ),
            Some(false)
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
        let target_activation_epoch = i64::from(target_activation_epoch);
        let epoch_length = i32::from(epoch_length);
        kani::assume(target_activation_epoch != reference_block_number / i64::from(epoch_length));
        assert_eq!(
            received_slash_deploy_authorized(
                reference_block_number,
                evidence_block_number,
                target_activation_epoch,
                epoch_length,
                i64::from(bond),
                true
            ),
            Some(false)
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
        let target_activation_epoch = reference_block_number / i64::from(epoch_length);
        kani::assume(target_activation_epoch != evidence_block_number / i64::from(epoch_length));
        assert_eq!(
            received_slash_deploy_authorized(
                reference_block_number,
                evidence_block_number,
                target_activation_epoch,
                epoch_length,
                i64::from(bond),
                true
            ),
            Some(false)
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
                i64::from(left_epoch),
                &right_offender,
                i64::from(right_epoch)
            ),
            left_offender == right_offender && left_epoch == right_epoch
        );
    }
}
