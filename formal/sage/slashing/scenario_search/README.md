# Slashing scenario search

`corpus_generator.sage` builds a deterministic Sage witness corpus for slashing exploratory work. It covers:

- multi-epoch stale evidence, rebond, and carryover boundaries
- partial-synchrony local view divergence and convergence
- proposer-schedule liveness assumptions
- atomic batch slash failure semantics
- evidence retention and pruning
- equivocation-record canonicalization
- weighted economic attack optimization
- differential trace classification

Run:

```sh
DOT_SAGE=/tmp/codex-sage sage formal/sage/slashing/scenario_search/corpus_generator.sage -- --self-test
DOT_SAGE=/tmp/codex-sage sage formal/sage/slashing/scenario_search/corpus_generator.sage -- --json-out /tmp/slashing-scenario-corpus.json
```

The corpus is not proof authority. Its records are finite witnesses and theorem candidates to promote into Rocq, TLA+, implementation tests, and the slashing documents after review.
