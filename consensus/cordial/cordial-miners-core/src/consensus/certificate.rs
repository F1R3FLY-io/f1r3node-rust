//! Threshold certificates: the evidence behind a weighted quorum decision.
//!
//! Issue #189 (gap 1): weighted ratification and super-ratification already
//! compute exactly which validators supported a block, with which blocks, for
//! how much stake, against which weight table — and then discard all of it,
//! returning a bare `bool`. The conclusion survives; the evidence does not.
//!
//! That forces anyone who wants to check the decision — a light client, an
//! auditor, the Lean conformance replay — to re-derive the whole computation
//! from the blocklace and trust that their answer matches. A
//! [`ThresholdCertificate`] carries the evidence out with the decision, so the
//! boolean becomes a derived, independently re-checkable fact.
//!
//! The field list mirrors the canonical trace's `ThresholdCertificateEvent`
//! one-for-one, so the object and the event it serializes into cannot drift.

use std::collections::BTreeSet;

use crate::consensus::weight_snapshot::{WeightSnapshot, WeightSnapshotId};
use crate::trace;
use crate::types::{BlockIdentity, NodeId};

/// Which rung of the approval ladder this certificate attests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CertificateKind {
    /// One block observed a two-thirds stake majority approving the target.
    Ratification,
    /// A two-thirds stake majority ratified the target.
    SuperRatification,
}

impl CertificateKind {
    /// The discriminant as it appears in the canonical trace.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ratification => "ratification",
            Self::SuperRatification => "super_ratification",
        }
    }
}

/// Portable evidence that a weighted quorum supported a block.
///
/// Everything needed to re-check the decision is here: who supported it, with
/// which blocks, how much stake that was, out of what total, and which weight
/// table those numbers were measured against.
///
/// Fields are public and `new` does not deduplicate, so a certificate from
/// outside this module is not self-validating: check it with
/// [`verify_quorum_against`](Self::verify_quorum_against).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThresholdCertificate {
    pub kind: CertificateKind,
    /// The block this certificate attests.
    pub leader: BlockIdentity,
    /// For ratification, the block whose view observed the quorum.
    /// `None` for super-ratification, which has no single observer.
    pub ratifier: Option<BlockIdentity>,
    /// Stable id linking a finality decision to this certificate.
    pub certificate_id: String,
    /// The evidence blocks, in canonical identity order.
    pub approver_blocks: Vec<BlockIdentity>,
    /// The distinct supporting validators, in canonical order.
    pub approvers: Vec<NodeId>,
    /// Combined bonded stake of `approvers`.
    pub approver_weight: u128,
    /// Total bonded stake in the weight table this was judged against.
    pub total_weight: u128,
    /// Identity of that weight table.
    pub weight_snapshot: WeightSnapshotId,
}

impl ThresholdCertificate {
    /// Assemble a certificate from the evidence a quorum decision produced.
    ///
    /// Both collections are sorted here so canonical order is a structural
    /// property of the type rather than a convention each call site has to
    /// remember.
    pub(crate) fn new(
        kind: CertificateKind,
        leader: &BlockIdentity,
        ratifier: Option<&BlockIdentity>,
        mut approver_blocks: Vec<BlockIdentity>,
        mut approvers: Vec<NodeId>,
        weights: &WeightSnapshot,
    ) -> Self {
        approver_blocks.sort();
        approvers.sort();

        let leader_hash = trace::hex(&leader.content_hash);
        let ratifier_hash = ratifier.map(|id| trace::hex(&id.content_hash));
        let certificate_id =
            trace::certificate_id(kind.as_str(), &leader_hash, ratifier_hash.as_deref());

        // Overflow folds to zero, matching the fail-closed accumulation the
        // quorum predicate itself uses.
        let approver_weight = approvers
            .iter()
            .try_fold(0u128, |total, creator| {
                total.checked_add(u128::from(weights.weight_of(creator)))
            })
            .unwrap_or(0);

        Self {
            kind,
            leader: leader.clone(),
            ratifier: ratifier.cloned(),
            certificate_id,
            approver_blocks,
            approvers,
            approver_weight,
            total_weight: weights.total().unwrap_or(0),
            weight_snapshot: weights.id().clone(),
        }
    }

    /// Number of distinct supporting validators.
    pub fn approver_count(&self) -> usize {
        self.approvers.len()
    }

    /// The strict two-thirds inequality over the numbers as carried.
    ///
    /// Says nothing about whether they are truthful — a certificate naming one
    /// validator while claiming five validators' stake passes. Prefer
    /// [`verify_quorum_against`](Self::verify_quorum_against) unless this
    /// process produced the numbers. Spelled out rather than delegated to the
    /// consensus predicate, so it stays an independent check.
    pub fn satisfies_threshold(&self) -> bool {
        let (Some(support), Some(threshold)) = (
            self.approver_weight.checked_mul(3),
            self.total_weight.checked_mul(2),
        ) else {
            return false;
        };

        self.total_weight > 0 && support > threshold
    }

    /// Recompute the quorum from the listed approvers against `weights`.
    ///
    /// Establishes that the distinct validators in `approvers` hold
    /// `approver_weight` of `total_weight` under this table and clear the
    /// threshold. Two things it does not establish:
    ///
    /// 1. That `approver_blocks` approve or ratify [`Self::leader`] — that
    ///    needs the blocklace and is separate evidence verification.
    /// 2. That `weights` governed this decision. The matching id is a 64-bit
    ///    fingerprint: it catches accidents, not substitution.
    pub fn verify_quorum_against(&self, weights: &WeightSnapshot) -> bool {
        if weights.id() != &self.weight_snapshot {
            return false;
        }

        // A Vec with no dedup in `new`, so a repeat would count twice.
        let distinct: BTreeSet<&NodeId> = self.approvers.iter().collect();
        if distinct.len() != self.approvers.len() {
            return false;
        }

        let Some(support) = self.approvers.iter().try_fold(0u128, |total, creator| {
            total.checked_add(u128::from(weights.weight_of(creator)))
        }) else {
            return false;
        };

        support == self.approver_weight
            && weights.total() == Some(self.total_weight)
            && self.satisfies_threshold()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn node(id: u8) -> NodeId {
        NodeId(vec![id])
    }

    fn block(seed: u8, creator: u8) -> BlockIdentity {
        let mut content_hash = [0u8; 32];
        content_hash[0] = seed;
        BlockIdentity {
            content_hash,
            creator: node(creator),
            signature: vec![],
        }
    }

    fn weights(per_validator: u64, count: u8) -> WeightSnapshot {
        let bonds: HashMap<NodeId, u64> = (1..=count).map(|id| (node(id), per_validator)).collect();
        WeightSnapshot::from_bonds(&bonds)
    }

    fn certificate(supporters: &[u8]) -> ThresholdCertificate {
        ThresholdCertificate::new(
            CertificateKind::SuperRatification,
            &block(1, 1),
            None,
            supporters.iter().map(|id| block(*id, *id)).collect(),
            supporters.iter().map(|id| node(*id)).collect(),
            &weights(100, 7),
        )
    }

    /// The capability gap 1 exists to create: check a finality decision from
    /// the certificate plus the weight table, with no blocklace in sight.
    #[test]
    fn quorum_verifies_against_the_weight_table() {
        let cert = certificate(&[1, 2, 3, 4, 5]);
        assert_eq!(cert.approver_weight, 500);
        assert_eq!(cert.total_weight, 700);
        assert_eq!(cert.approver_count(), 5);
        // 3 * 500 = 1500 > 2 * 700 = 1400
        assert!(cert.satisfies_threshold());
        assert!(cert.verify_quorum_against(&weights(100, 7)));
    }

    /// Five validators' stake claimed while only one is listed.
    #[test]
    fn overstated_support_passes_the_arithmetic_but_fails_verification() {
        let mut forged = certificate(&[1, 2, 3, 4, 5]);
        forged.approvers = vec![node(1)]; // approver_weight stays at 500

        assert!(
            forged.satisfies_threshold(),
            "the inequality still holds over the carried numbers"
        );
        assert!(
            !forged.verify_quorum_against(&weights(100, 7)),
            "recomputing from the listed approvers gives 100, not 500"
        );
    }

    #[test]
    fn a_repeated_approver_is_not_counted_twice() {
        let mut forged = certificate(&[1]);
        forged.approvers = vec![node(1); 5];
        forged.approver_weight = 500;

        assert!(forged.satisfies_threshold());
        assert!(!forged.verify_quorum_against(&weights(100, 7)));
    }

    #[test]
    fn an_understated_total_fails_verification() {
        let mut forged = certificate(&[1, 2, 3, 4]);
        forged.total_weight = 500; // the real table totals 700

        assert!(forged.satisfies_threshold()); // 1200 > 1000
        assert!(!forged.verify_quorum_against(&weights(100, 7)));
    }

    #[test]
    fn verification_rejects_a_table_the_certificate_does_not_name() {
        let cert = certificate(&[1, 2, 3, 4, 5]);
        // Same validators, different stakes, so a different fingerprint.
        assert!(!cert.verify_quorum_against(&weights(50, 7)));
    }

    #[test]
    fn quorum_rejects_a_bare_majority() {
        let cert = certificate(&[1, 2, 3, 4]);
        assert_eq!(cert.approver_weight, 400);
        // 3 * 400 = 1200, not > 2 * 700 = 1400 — a simple majority is not enough
        assert!(!cert.satisfies_threshold());
    }

    #[test]
    fn quorum_is_strict_at_the_exact_boundary() {
        // Six of nine equal validators: 3 * 600 == 2 * 900, so not strictly greater.
        let cert = ThresholdCertificate::new(
            CertificateKind::SuperRatification,
            &block(1, 1),
            None,
            (1..=6).map(|id| block(id, id)).collect(),
            (1..=6).map(node).collect(),
            &weights(100, 9),
        );
        assert_eq!((cert.approver_weight, cert.total_weight), (600, 900));
        assert!(!cert.satisfies_threshold());
    }

    #[test]
    fn evidence_is_stored_in_canonical_order() {
        let cert = certificate(&[5, 1, 3]);
        let mut sorted_approvers = cert.approvers.clone();
        sorted_approvers.sort();
        assert_eq!(cert.approvers, sorted_approvers);

        let mut sorted_blocks = cert.approver_blocks.clone();
        sorted_blocks.sort();
        assert_eq!(cert.approver_blocks, sorted_blocks);
    }

    /// The id must match what the canonical trace carries, or replay cannot
    /// link a finality decision to the certificate that justified it.
    #[test]
    fn certificate_id_matches_the_canonical_trace_encoding() {
        let leader = block(1, 1);
        let ratifier = block(2, 2);
        let leader_hash = trace::hex(&leader.content_hash);
        let ratifier_hash = trace::hex(&ratifier.content_hash);

        let cert = ThresholdCertificate::new(
            CertificateKind::Ratification,
            &leader,
            Some(&ratifier),
            vec![],
            vec![node(1)],
            &weights(100, 7),
        );

        assert_eq!(
            cert.certificate_id,
            trace::certificate_id("ratification", &leader_hash, Some(&ratifier_hash))
        );
    }

    #[test]
    fn an_empty_weight_table_never_reaches_quorum() {
        let bonds: HashMap<NodeId, u64> = HashMap::new();
        let cert = ThresholdCertificate::new(
            CertificateKind::SuperRatification,
            &block(1, 1),
            None,
            vec![],
            vec![],
            &WeightSnapshot::from_bonds(&bonds),
        );
        assert_eq!(cert.total_weight, 0);
        assert!(!cert.satisfies_threshold());
    }
}
