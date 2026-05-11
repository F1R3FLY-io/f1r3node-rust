// UC-90 — Partition / gossip state machine: divergent evidence views merge
// monotonically, never losing active-unreported entries.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-90.
// Reference: formal/sage/evidence_propagation_model.sage,
// formal/sage/slashing/FINDINGS.md.
//
// Threat model: a network partition causes two halves to develop disjoint
// `evidence` and `reports` sets. When the partition heals and views merge,
// every active-unreported evidence in either half must remain active-
// unreported in the union — gossip-merge must not silently mark an evidence
// "reported" without an actual report. The state machine in this file is
// the smallest model that exhibits the property.

use std::collections::BTreeSet;

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct View {
    evidence: BTreeSet<u8>,
    reports: BTreeSet<u8>,
}

impl View {
    fn merge(&self, other: &Self) -> Self {
        Self {
            evidence: self.evidence.union(&other.evidence).copied().collect(),
            reports: self.reports.union(&other.reports).copied().collect(),
        }
    }

    fn active_unreported(&self) -> BTreeSet<u8> {
        self.evidence.difference(&self.reports).copied().collect()
    }
}

#[test]
fn uc_90_partition_gossip_merge_restores_shared_evidence_view() {
    let left = View {
        evidence: BTreeSet::from([0]),
        reports: BTreeSet::new(),
    };
    let right = View {
        evidence: BTreeSet::new(),
        reports: BTreeSet::from([0]),
    };
    let merged = left.merge(&right);

    assert_eq!(merged.evidence, BTreeSet::from([0]));
    assert_eq!(merged.reports, BTreeSet::from([0]));
    assert!(merged.active_unreported().is_empty());

    let class = classify(DivergenceReason::PartitionGossipBoundary);
    assert_eq!(class, DivergenceClass::CandidateBoundaryDivergence);
    assert!(frontier_classification_ok(class));
}

#[test]
fn uc_90_unmerged_partition_is_documented_boundary() {
    let class = classify(DivergenceReason::PartitionGossipBoundary);
    assert!(frontier_classification_ok(class));
}
