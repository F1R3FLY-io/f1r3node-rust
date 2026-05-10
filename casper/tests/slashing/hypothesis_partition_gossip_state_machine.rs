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
