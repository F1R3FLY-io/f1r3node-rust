use std::collections::{BTreeMap, BTreeSet};

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[test]
fn uc_87_persistent_corpus_accumulates_without_reclassifying() {
    let mut corpus = BTreeMap::from([
        (
            "weighted_boundary",
            classify(DivergenceReason::PreconditionFuzzingBoundary),
        ),
        (
            "retention_projection",
            classify(DivergenceReason::ProjectionBoundary),
        ),
    ]);
    let before = corpus.clone();

    corpus.insert(
        "weighted_boundary",
        classify(DivergenceReason::PreconditionFuzzingBoundary),
    );
    corpus.insert(
        "proposer_fairness",
        classify(DivergenceReason::ProposerFairnessBoundary),
    );

    assert_eq!(
        corpus.get("weighted_boundary"),
        before.get("weighted_boundary")
    );
    assert!(corpus
        .values()
        .all(|class| frontier_classification_ok(*class)));
    assert!(corpus
        .values()
        .all(|class| *class != DivergenceClass::UnexpectedDivergence));
}

#[test]
fn uc_87_corpus_ids_are_deduplicated() {
    let ids = ["a", "b", "a", "c", "b"];
    let deduped = ids.into_iter().collect::<BTreeSet<_>>();
    assert_eq!(deduped, BTreeSet::from(["a", "b", "c"]));
}
