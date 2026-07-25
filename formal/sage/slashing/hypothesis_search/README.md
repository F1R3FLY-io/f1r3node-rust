# Hypothesis-backed Sage search

`hypothesis_scenario_search.sage` is an optional deep-search layer for the slashing Sage models. It uses Hypothesis to shrink generated scenarios, then emits deterministic Sage/JSON witnesses for review and promotion.

Run:

```sh
DOT_SAGE=/tmp/codex-sage sage formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage -- --self-test
DOT_SAGE=/tmp/codex-sage sage formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage -- --profile quick --search-mode all --json-out /tmp/slashing-hypothesis-findings.json --rust-corpus-out /tmp/slashing-rust-corpus.json
DOT_SAGE=/tmp/codex-sage sage formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage -- --profile quick --search-mode frontier --json-out /tmp/slashing-hypothesis-frontier.json --fixture-out /tmp/slashing-hypothesis-frontier-fixtures.json --coverage-out /tmp/slashing-hypothesis-frontier-coverage.json
DOT_SAGE=/tmp/codex-sage sage formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage -- --profile quick --search-mode horizon --json-out /tmp/slashing-hypothesis-horizon.json --rust-corpus-out /tmp/slashing-horizon-rust-corpus.json --rust-fixtures-out /tmp/slashing-horizon-rust-fixtures.json
DOT_SAGE=/tmp/codex-sage sage formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage -- --profile quick --search-mode horizon-v2 --json-out /tmp/slashing-hypothesis-horizon-v2.json --rust-corpus-out /tmp/slashing-horizon-v2-rust-corpus.json --rust-fixtures-out /tmp/slashing-horizon-v2-rust-fixtures.json
DOT_SAGE=/tmp/codex-sage sage formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage -- --profile deep --search-mode all --max-examples 2000 --state-steps 64 --json-out /tmp/slashing-hypothesis-deep.json --rust-corpus-out /tmp/slashing-rust-corpus-deep.json --rust-fixtures-out /tmp/slashing-rust-fixtures-deep.json --fixture-out /tmp/slashing-frontier-fixtures.json
DOT_SAGE=/tmp/codex-sage sage formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage -- --profile corpus --search-mode all --persistent-db /tmp/slashing-hypothesis-corpus-db --json-out /tmp/slashing-hypothesis-corpus.json
DOT_SAGE=/tmp/codex-sage sage formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage -- --profile corpus-deep --search-mode all --persistent-db /tmp/slashing-hypothesis-corpus-deep-db --json-out /tmp/slashing-hypothesis-corpus-deep.json
DOT_SAGE=/tmp/codex-sage sage formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage -- --replay-json /tmp/slashing-hypothesis-findings.json
```

`--search-mode targeted` runs the known risk-surface searches.
`--search-mode frontier` runs less-directed novelty/coverage,
feature-combination coverage, bundle state-machine, multi-epoch trace,
adversarial scheduler, partition/gossip state machine,
liveness-as-safety, production-shaped DAG trace generation,
detector-totality DAG search, cross-oracle closure consistency,
adaptive evidence-denial search, composite multi-factor attack search,
candidate invariant mining, temporal-window synthesis,
mutation-oracle detection, rebond identity lifecycle search,
record-lifecycle state-machine search, closure-depth extremal search,
defensive adversarial vulnerability campaigns, exact-vs-projection, arithmetic stress,
generated-trace classification, semantic attack campaign,
attack-objective, objective-guided campaign scoring, metamorphic
property, Rust metamorphic fixtures, assumption-minimization,
assumption-weakening, precondition-fuzzing, Rust differential-corpus,
Rust differential-replay, evidence-addition monotonicity, view-merge
confluence, minimal slash-basis extraction, record-key namespace
projection searches, detector traversal termination, detector
contribution confluence, closure fixed-point idempotence,
report-retention reactivation, no-seed cycle safety, slash-history
prefix exactness, neglect-edge orientation sanity, redundant-path
evidence-denial cost, slash-target authorization, report namespace
isolation, report-antitone closure, direct-seed report dominance,
validator-renaming equivariance, and bisimilarity delta guarding.
`--search-mode horizon` composes retention, gossip, proposer inclusion,
epoch/rebond identity, weighted damage, Rust detector contribution
gates, arithmetic projection, partition/merge, and metamorphic
cross-oracle checks into longer campaigns. `--search-mode horizon-v2`
adds Rust-aligned detector DAG state machines, multi-record lifecycle
state machines, finality-aware evidence availability, weighted damage
plus evidence-denial objective search, and generated differential
classification. Mutable evidence lifecycle, bundle, multi-epoch,
partition/gossip, semantic campaign, horizon, and horizon-v2 campaign
checks use Hypothesis `RuleBasedStateMachine`; replayable differential
corpora use generated trace strategies. `--search-mode all` runs
targeted, frontier, horizon, and horizon-v2 searches and is the default.

`--schema-out`, `--fixture-out`, and `--coverage-out` emit the shared
Sage fixture schema, top-ranked replay fixtures, and coverage features.
`--top-k` controls fixture selection, and `--objectives` filters fixture
output by axis, classification, or coverage feature.
The integrated Rust runner passes `SAGE_OBJECTIVES`, so coverage-guided
follow-up runs can focus fixture emission on features such as
`uncovered_rust`, `coverage_gap`, `detector_traversal_depth`,
`retention_window_boundary`, `stake_damage_pareto`, and
`replay_divergence`.

`--profile quick`, `--profile deep`, `--profile stress`, and
`--profile rust-replay` run deterministically with no Hypothesis
database unless `--persistent-db` is supplied. `--profile corpus` and
`--profile corpus-deep` use persistent example databases, defaulting to
`/tmp/slashing-hypothesis-corpus-db` and
`/tmp/slashing-hypothesis-corpus-deep-db`, so long-running searches can
accumulate frontier examples across sessions.

The search is not proof authority. A Hypothesis failure must be reduced to a deterministic witness, classified, and promoted to Rocq/TLA+/documentation only after review.
