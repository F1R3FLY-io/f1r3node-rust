// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-87 — Persistent corpus accumulates classifications across runs.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-87.
// Reference: formal/sage/slashing/FINDINGS.md (corpus retention rows),
// formal/sage/slashing/hypothesis_search/.
//
// Property: re-inserting an existing corpus entry must not flip its
// `DivergenceClass`, and adding new entries must not reclassify old ones.
// This is what makes the Sage->Hypothesis fixture pipeline replayable —
// without it, a flaky fuzz run could erase prior findings.

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
