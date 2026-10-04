use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::block::Block;
use crate::blocklace::Blocklace;
use crate::consensus::cordiality::is_weighted_supermajority;
use crate::consensus::cordiality::{hidden_equivocations, is_cordial_block, missing_known_tips};
use crate::consensus::fork_choice::collect_validator_tips;
#[cfg(feature = "trace")]
use crate::consensus::round::candidate_depth;
use crate::crypto;
#[cfg(feature = "trace")]
use crate::trace::{self, TraceEvent, ValidateBlockEvent};
use crate::types::{BlockIdentity, NodeId};

/// Reasons a block can be rejected during validation.
///
/// Modeled after f1r3node's `InvalidBlock` enum but reduced to what
/// the Cordial Miners protocol actually requires. CBC Casper has 23+
/// variants; many are eliminated because the blocklace unifies parents
/// and justifications.
#[derive(Debug, Clone, PartialEq)]
pub enum InvalidBlock {
    InsufficientRoundSupport {
        round: u64,
    },
    UnsupportedPrunedHistory,
    InvalidCausalHistory,
    /// Block's content_hash does not match hash(content).
    InvalidContentHash {
        expected: [u8; 32],
        actual: [u8; 32],
    },

    /// Block's signature does not verify against the creator's public key.
    InvalidSignature,

    /// Block creator is not a bonded validator.
    UnknownSender {
        creator: NodeId,
    },

    /// One or more predecessor blocks are not in the blocklace (closure violation).
    MissingPredecessors {
        missing: Vec<BlockIdentity>,
    },

    /// Inserting this block would violate the chain axiom for the creator.
    /// The creator already has a block that is not comparable to this one.
    Equivocation {
        conflicting: BlockIdentity,
    },

    /// Block does not satisfy the cordial condition (does not reference all known tips).
    NotCordial {
        missing_tips: Vec<BlockIdentity>,
    },

    /// Block hides a same-round equivocation already known to the local DAG view.
    HiddenEquivocation {
        creator: NodeId,
        round: u64,
        hidden: Vec<BlockIdentity>,
    },
}

/// Result of block validation.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    /// Block passed all checks and is safe to insert.
    Valid,
    /// Block failed one or more checks.
    Invalid(Vec<InvalidBlock>),
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        matches!(self, ValidationResult::Valid)
    }

    pub fn errors(&self) -> &[InvalidBlock] {
        match self {
            ValidationResult::Valid => &[],
            ValidationResult::Invalid(errors) => errors,
        }
    }
}

/// Configuration for which validation checks to run.
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Check that content_hash matches hash(content).
    pub check_content_hash: bool,
    /// Check that signature verifies against creator's public key.
    pub check_signature: bool,
    /// Check that creator is in the bonds map.
    pub check_sender: bool,
    /// Check closure axiom (predecessors exist).
    pub check_closure: bool,
    /// Check chain axiom (no equivocation).
    pub check_chain_axiom: bool,
    /// Check cordial condition (references all known tips).
    pub check_cordial: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            check_content_hash: true,
            check_signature: true,
            check_sender: true,
            check_closure: true,
            check_chain_axiom: true,
            check_cordial: false, // off by default — not all blocks need to be cordial
        }
    }
}

impl ValidationConfig {
    /// All checks enabled.
    pub fn strict() -> Self {
        Self {
            check_cordial: true,
            ..Default::default()
        }
    }
}

/// Validate a block against the blocklace and bonds map.
///
/// Runs each enabled check and collects all errors (does not short-circuit).
/// This allows the caller to see every issue at once rather than fixing
/// them one at a time.
pub fn validate_block(
    block: &Block,
    blocklace: &Blocklace,
    bonds: &HashMap<NodeId, u64>,
    config: &ValidationConfig,
) -> ValidationResult {
    let mut errors = Vec::new();

    // 1. Content hash verification
    if config.check_content_hash {
        let expected = crypto::hash_content(&block.content);
        if block.identity.content_hash != expected {
            errors.push(InvalidBlock::InvalidContentHash {
                expected,
                actual: block.identity.content_hash,
            });
        }
    }

    // 2. Signature verification (FIXED for Secp256k1 + variable-length DER)
    if config.check_signature {
        let public_key = &block.identity.creator.0;

        // NOTE:
        // - DO NOT assume 64-byte signature (Ed25519 assumption)
        // - Secp256k1 signatures are DER encoded (variable length ~70–72 bytes)
        // - Public key may be 33 or 65 bytes
        if !crypto::verify(
            &block.identity.content_hash,
            public_key,
            &block.identity.signature,
        ) {
            errors.push(InvalidBlock::InvalidSignature);
        }
    }

    // 3. Sender is bonded
    if config.check_sender && !bonds.contains_key(&block.identity.creator) {
        errors.push(InvalidBlock::UnknownSender {
            creator: block.identity.creator.clone(),
        });
    }

    // 4. Closure axiom — all predecessors must exist
    let missing_predecessors: Vec<BlockIdentity> = block
        .content
        .predecessors
        .iter()
        .filter(|pred_id| blocklace.content(pred_id).is_none())
        .cloned()
        .collect();

    if config.check_closure && !missing_predecessors.is_empty() {
        errors.push(InvalidBlock::MissingPredecessors {
            missing: missing_predecessors.clone(),
        });
    }

    // 5. Chain axiom — inserting this block must not create equivocation.
    //
    // Deferred while predecessors are missing: the comparability walk cannot
    // succeed without the block's history, so running it early misreports an
    // honest block as `Equivocation` and gets it dropped instead of buffered.
    if config.check_chain_axiom && missing_predecessors.is_empty() {
        let creator = &block.identity.creator;
        let creator_blocks = blocklace.blocks_by(creator);

        for existing in &creator_blocks {
            let new_has_existing_in_ancestry = block
                .content
                .predecessors
                .iter()
                .any(|pred_id| blocklace.preceedes_or_equals(&existing.identity, pred_id));

            let existing_has_new_in_ancestry =
                blocklace.precedes(&block.identity, &existing.identity);

            if !new_has_existing_in_ancestry
                && !existing_has_new_in_ancestry
                && block.identity != existing.identity
            {
                errors.push(InvalidBlock::Equivocation {
                    conflicting: existing.identity.clone(),
                });
                break; // one conflict is enough
            }
        }
    }

    // 6. Cordial condition — block references known tips and does not hide
    //    globally known equivocations already present in the local DAG view.
    //
    // Deferred while predecessors are missing, same as the chain axiom: these
    // checks judge the block against a closure rebuilt from the local view,
    // and any extra error here would stop callers from buffering the block.
    if config.check_cordial && missing_predecessors.is_empty() {
        let known_tips = collect_validator_tips(blocklace, bonds);
        let missing_tips = missing_known_tips(block, &known_tips);

        if !missing_tips.is_empty() {
            errors.push(InvalidBlock::NotCordial { missing_tips });
        }

        for hidden in hidden_equivocations(blocklace, block) {
            errors.push(InvalidBlock::HiddenEquivocation {
                creator: hidden.creator,
                round: hidden.round,
                hidden: hidden.hidden,
            });
        }

        let _ = is_cordial_block(blocklace, block, &known_tips);
    }

    if errors.is_empty() {
        #[cfg(feature = "trace")]
        {
            let round = candidate_depth(blocklace, &block.content);
            trace::emit(TraceEvent::ValidateBlock(ValidateBlockEvent {
                node_id: trace::hex(&block.identity.creator.0),
                wave: None,
                round,
                block_hash: trace::hex(&block.identity.content_hash),
                parent_hashes: trace::sorted_block_hashes(&block.content.predecessors),
                creator: trace::hex(&block.identity.creator.0),
                weight_table_hash: None,
                outcome: "valid".into(),
                errors: vec![],
            }));
        }
        ValidationResult::Valid
    } else {
        #[cfg(feature = "trace")]
        {
            let round = candidate_depth(blocklace, &block.content);
            trace::emit(TraceEvent::ValidateBlock(ValidateBlockEvent {
                node_id: trace::hex(&block.identity.creator.0),
                wave: None,
                round,
                block_hash: trace::hex(&block.identity.content_hash),
                parent_hashes: trace::sorted_block_hashes(&block.content.predecessors),
                creator: trace::hex(&block.identity.creator.0),
                weight_table_hash: None,
                outcome: "invalid".into(),
                errors: errors.iter().map(|e| format!("{:?}", e)).collect(),
            }));
        }
        ValidationResult::Invalid(errors)
    }
}

/// Validate and insert a block into the blocklace.
///
/// Convenience function that runs validation then inserts on success.
/// Returns the validation result (which includes errors on failure).
pub fn validated_insert(
    block: Block,
    blocklace: &mut Blocklace,
    bonds: &HashMap<NodeId, u64>,
    config: &ValidationConfig,
) -> ValidationResult {
    let result = validate_block(&block, blocklace, bonds, config);
    if result.is_valid() {
        // Closure is already verified by validation, so commit directly.
        blocklace.commit_validated(block.identity.clone(), block.content);
    }
    result
}

pub fn validate_received_block(
    block: &Block,
    blocklace: &Blocklace,
    bonds: &HashMap<NodeId, u64>,
) -> ValidationResult {
    let mut errors = Vec::new();
    let expected = crypto::hash_content(&block.content);
    if block.identity.content_hash != expected {
        errors.push(InvalidBlock::InvalidContentHash {
            expected,
            actual: block.identity.content_hash,
        });
    }
    if !crypto::verify(
        &block.identity.content_hash,
        &block.identity.creator.0,
        &block.identity.signature,
    ) {
        errors.push(InvalidBlock::InvalidSignature);
    }
    if bonds.get(&block.identity.creator).copied().unwrap_or(0) == 0 {
        errors.push(InvalidBlock::UnknownSender {
            creator: block.identity.creator.clone(),
        });
    }
    if !errors.is_empty() {
        return ValidationResult::Invalid(errors);
    }
    if blocklace.checkpoint().is_some() {
        return ValidationResult::Invalid(vec![InvalidBlock::UnsupportedPrunedHistory]);
    }
    let depths = match received_history_depths(block, blocklace) {
        Ok(depths) => depths,
        Err(error) => return ValidationResult::Invalid(vec![error]),
    };
    if let Some(round) = block.content.predecessors.iter().map(|id| depths[id]).max() {
        let supporters = block
            .content
            .predecessors
            .iter()
            .filter(|id| depths[*id] == round)
            .map(|id| id.creator.clone())
            .collect();
        if !is_weighted_supermajority(&supporters, bonds) {
            errors.push(InvalidBlock::InsufficientRoundSupport { round });
        }
    }
    let mut own_history: Vec<_> = depths
        .keys()
        .filter(|id| id.creator == block.identity.creator)
        .collect();
    own_history.sort_unstable_by_key(|id| (depths[*id], *id));
    for pair in own_history.windows(2) {
        if !blocklace.preceedes_or_equals(pair[0], pair[1]) {
            errors.push(InvalidBlock::Equivocation {
                conflicting: pair[1].clone(),
            });
            break;
        }
    }
    if errors.is_empty() {
        ValidationResult::Valid
    } else {
        ValidationResult::Invalid(errors)
    }
}

fn received_history_depths(
    block: &Block,
    blocklace: &Blocklace,
) -> Result<BTreeMap<BlockIdentity, u64>, InvalidBlock> {
    let mut depths = BTreeMap::new();
    let mut visiting = HashSet::new();
    let mut missing = BTreeSet::new();
    let mut pending: Vec<_> = block
        .content
        .predecessors
        .iter()
        .map(|id| (id.clone(), false))
        .collect();
    while let Some((id, expanded)) = pending.pop() {
        if depths.contains_key(&id) {
            continue;
        }
        let Some(content) = blocklace.content(&id) else {
            missing.insert(id);
            continue;
        };
        if expanded {
            visiting.remove(&id);
            let mut maximum = None;
            for predecessor in &content.predecessors {
                let Some(&value) = depths.get(predecessor) else {
                    continue;
                };
                maximum = Some(maximum.unwrap_or(0u64).max(value));
            }
            let round = match maximum {
                Some(value) => value
                    .checked_add(1)
                    .ok_or(InvalidBlock::InvalidCausalHistory)?,
                None => 0,
            };
            depths.insert(id, round);
        } else {
            if !visiting.insert(id.clone()) {
                return Err(InvalidBlock::InvalidCausalHistory);
            }
            pending.push((id, true));
            pending.extend(
                content
                    .predecessors
                    .iter()
                    .map(|predecessor| (predecessor.clone(), false)),
            );
        }
    }
    if missing.is_empty() {
        Ok(depths)
    } else {
        Err(InvalidBlock::MissingPredecessors {
            missing: missing.into_iter().collect(),
        })
    }
}

pub fn validated_received_insert(
    block: Block,
    blocklace: &mut Blocklace,
    bonds: &HashMap<NodeId, u64>,
) -> ValidationResult {
    let result = validate_received_block(&block, blocklace, bonds);
    if result.is_valid() && blocklace.content(&block.identity).is_none() {
        blocklace.commit_validated(block.identity, block.content);
    }
    result
}
