# Slashing Sage findings

These findings are finite Sage model outputs. They are not proof authority. Each item below is a witness or theorem candidate to promote into Rocq, TLA+, implementation tests, or the slashing documents after review.

Status: findings 1 through 23 have been promoted into Rocq theorem targets, `TwoLevelSlashing.tla` invariants where finite checking applies, and `docs/theory/slashing/` documentation/test-plan entries. Findings 24 through 32 are tracked deterministic scenario-corpus findings. Findings 33 and later are Hypothesis-backed, deep-threat, DAG, and objective-frontier findings that have been reduced to deterministic Sage witnesses before promotion. Sage remains the witness generator, not the proof authority.

## Confirmed model findings

1. Unweighted neglect closure is reverse reachability to direct equivocators.
   The Rocq theorem `slash_iter_reachability_characterization` now proves the exact closure shape. Sage witness paths agree with that theorem.

2. Minimal unweighted quorum drop appears at `n=4, F=1`.
   Witness: equivocators `[1]`, edges `[[0, 1]]`, closure `[0, 1]`, active `2`, quorum bound `3`.

3. Weighted stake closure exposes a zero-stake edge case.
   Witness: `n=3`, stakes `[0, 2, 2]`, stake fault bound `1`, equivocators `[0]`, edge `[[1, 0]]`, closure `[0, 1]`, slashed stake `2`, active stake `2`, quorum `3`. This is only a protocol issue if a zero-stake validator can be a direct offender in the slashing evidence domain.

4. Damage optimization finds chain amplification.
   Witness: `n=4`, stakes `[3, 3, 3, 3]`, fault `3`, equivocators `[3]`, edges `[[0, 1], [1, 2], [2, 3]]`, closure `[0, 1, 2, 3]`, extra slashed stake `9`, depth `3`.

5. Validator-set boundary filtering has a candidate divergence.
   Witness: current validators `[0, 1, 2, 3]`, evidence validators `[0, 1, 2, 3, 4]`, stale equivocator `[4]`, edge `[[0, 4]]`. A filtered current-validator model slashes `[]`; an unfiltered projection slashes `[0]`.

6. Evidence withholding creates an accountability gap.
   Witness: `n=4`, equivocator `[0]`, visibility `[]`, reports `[]`. Partial visibility closure is `[0]`; full-visibility closure is `[0, 1, 2, 3]`; gap is `[1, 2, 3]`.

7. Duplicate, self-edge, and cycle cases behave as reverse reachability predicts.
   Duplicate edges do not change closure. A cycle disconnected from a direct offender is not slashed. A cycle with a path to a direct offender is slashed. Self-edges alone are idempotent and do not create new closure.

8. Bounded arithmetic projections diverge from exact arithmetic at the first overflow boundary.
   For signed 64-bit, `9223372036854775807 + 1` fails checked arithmetic, wraps to `-9223372036854775808`, or saturates to `9223372036854775807`. For unsigned 128-bit, `340282366920938463463374607431768211455 + 1` fails checked arithmetic, wraps to `0`, or saturates to the maximum value.

9. Differential bisimilarity search found no unexpected divergence in the small current-validator state space tested.
   It reports candidate boundary-filter divergences separately from the known permitted tracker atomicity bug-fix divergence.

10. Weighted quorum intersection held for all bounded stake vectors tested through `n=5`, stake `1..3`.
    This supports the promoted quorum-intersection theorem shape: any two active quorums whose combined weight exceeds active stake must intersect.

11. Closure certificates show first slash round equals shortest neglect-path distance in the chain witness.
    For `n=6`, offender `5`, chain `0 -> 1 -> 2 -> 3 -> 4 -> 5`, the fixed-point round is `5`, and validators are first slashed at distances `5,4,3,2,1,0`.

12. Batch slash order is observationally independent in the finite model.
    With bonds `[5,7,11,13]`, all `24` slash orders produce bonds `[0,0,0,0]`, vault `36`, and slashed set `[0,1,2,3]`.

13. Epoch/current-validator filtering separates stale and fresh evidence.
    Stale offender evidence outside the current epoch produces empty closure; fresh current-epoch evidence propagates through the current validator set.

14. Evidence propagation over time is not monotone once reports are modeled.
    A later report can remove a neglect edge and shrink accountability closure. The correct invariant is that every active neglect edge is visible and unreported, not that closure is monotone across reporting stages.

15. Arithmetic safe envelopes give implementation-level numeric bounds.
    For unsigned 64-bit with `100` validators and `maxBond=10`, the envelope is safe; with `2` validators and `maxBond=2^63`, it is unsafe by one unit over `u64::MAX`.

16. Record normalization is order- and duplicate-insensitive.
    All permutations of duplicate records normalize to the same key-to-hash-set meaning, and duplicate hashes are idempotent.

17. Adversarial timing search finds a minimal `n=4` quorum-drop and damage witness.
    With stakes `[1,1,1,2]`, adversary/direct equivocator `[0]`, and active neglect edge `3 -> 0`, closure is `[0,3]`, active count is `2`, and both count and stake quorum drop. This is only an honest-slash threat if an honest validator can be induced to create a visible-unreported neglect edge; otherwise the edge itself is slashable misconduct.

18. Evidence timing search shows local-view divergence and report-time nonmonotonicity.
    Two observers with different visible evidence can compute different closure sets. A later report can also remove a neglect edge and shrink closure, confirming that the invariant must be edge admissibility (`visible ∧ unreported`) rather than closure monotonicity over report time.

19. Epoch churn search exposes validator-identity policy boundaries.
    Loose public-key identity can apply stale evidence to a newly rejoined validator, while epoch-tagged identity filters it. Pending-slash rebond behavior is a protocol policy boundary: either stale direct evidence is intentionally carried to the new epoch identity or it is intentionally not carried.

20. The assumption counterexample catalog confirms the main theorem hypotheses are necessary.
    Removing closure bounds, strict quorum intersection bounds, `NoDup` quorum inputs, current-validator filters, report suppression, `s0 ⊆ universe`, arithmetic envelopes, or the weighted disjoint-stake bound yields small concrete failures.

21. Weighted MIP optimization finds high amplification under a violated closure bound.
    Sage `MixedIntegerLinearProgram` found `n=4`, stakes `[3,3,1,1]`, fault `1`, direct offender `[2]`, edges `0 -> 1 -> 2`, closure `[0,1,2]`, and extra slashed stake `6` for direct adversarial stake `1`. This is a theorem-precondition witness, not a failure under T-12W's bounded-closure hypothesis.

22. Differential trace generation now emits reviewable traces.
    The generator produces a bisimilar trace, a permitted tracker-atomicity bug-fix divergence trace, and a candidate current-validator boundary divergence trace. No unexpected divergence appears in the bounded trace set.

23. Implementation projection risks are now explicit finite witnesses.
    Abort-on-first-failure batch slash semantics are order-dependent; naive string concatenation of record keys collides for `(1,23)` and `(12,3)`; pruning evidence before final slash loses slashability; fixed-width arithmetic diverges at `max + 1`; duplicate records are state-equivalent only after normalization.

24. The scenario corpus generator covers every requested exploratory axis.
    `scenario_search/corpus_generator.sage` emits 14 records over 8 axes with classes `confirmed_safe`, `candidate_boundary`, `projection_risk`, and `assumption_counterexample`. The generated corpus found no unexpected differential edge-order divergence in its bounded search.

25. Multi-epoch search confirms rebond and carryover are explicit policy boundaries.
    Witness: stale identity `A@0` mapped loosely to current identity `A@1`, current validators `A@1,B@1`, edge `B@1 -> A@1`. Strict epoch-tagged identity yields empty closure; loose pubkey projection yields closure `[A@1,B@1]`. A pending-slash carryover policy similarly changes closure from `[]` to `[0,1]` when the carried direct offender is enabled.

26. Partial-synchrony view convergence separates pre-convergence divergence from post-convergence equality.
    Witness: validators `[0,1,2,3]`, direct `[0]`, view A edge `3 -> 0`, view B no edge. Before gossip, closures are `[0,3]` and `[0]`; after both views contain the same active edge, both closures are `[0,3]`.

27. Liveness depends on proposer inclusion fairness.
    Witness: with direct offender `[0]` and edge `2 -> 0`, a fair bonded proposer including evidence at slot `2` slashes at slot `2`. A horizon-4 schedule where every bonded proposer observes but withholds evidence has no slash slot. This is not a safety bug; it identifies the fairness or inclusion assumption needed for liveness statements.

28. Batch failure modeling distinguishes safe atomic policies from order-dependent partial abort.
    With bonds `[5,7,11]`, slash set `[0,1,2]`, and failure at validator `1`, preflight abort, rollback, and continue-on-error are order independent. Abort-after-partial-failure is order dependent, producing vault values `0`, `5`, `11`, or `16` depending on order.

29. Evidence retention has a concrete minimum window witness.
    With evidence observed at slot `0`, first slashable slot `2`, direct `[1]`, and edge `0 -> 1`, retention window `2` preserves closure `[0,1]`; retention window `1` prunes too early and loses slashability.

30. Record canonicalization remains essential in the scenario corpus.
    Hash order and duplicate hashes normalize to the same record meaning for `(validator=0, seq=10, hashes={h1,h2})`. Delimiter-free keys collide for `(1,23)` and `(12,3)`, while canonical delimiter keys do not.

31. Economic attack optimization appears in the integrated corpus as an assumption counterexample.
    The MIP witness remains `n=4`, stakes `[3,3,1,1]`, direct `[2]`, edges `0 -> 1 -> 2`, closure `[0,1,2]`, direct stake `1`, closure stake `7`, extra stake `6`, and weighted quorum drop. This violates the weighted closure-bound theorem precondition rather than refuting the theorem.

32. Differential trace corpus now ties exploratory scenarios to regression classes.
    The corpus emits one ordinary bisimilar trace, one permitted tracker atomicity bug-fix trace, one candidate stale-current-validator boundary trace, one partial-batch projection-risk trace, and one weighted-closure-bound assumption-counterexample trace.

33. Hypothesis is integrated as an optional Sage deep-search layer.
    `hypothesis_search/hypothesis_scenario_search.sage` ran with `profile=quick`, `max_examples=100`, and `state_steps=16`, producing 34 records across 29 axes. Class counts: 24 confirmed-safe, 2 candidate-boundary, 3 projection-risk, 5 assumption-counterexample, and 0 unexpected divergences.
    The supported deep profile also runs with `max_examples=2000` and `state_steps=64`; it exercises the same targeted and frontier axes with 0 unexpected divergences.

34. Hypothesis state-machine search preserved active-edge admissibility.
    The `EvidenceLifecycleMachine` explored direct evidence, visible neglect edges, reports, and closure updates. It preserved the invariant that active neglect edges are visible and unreported, and that closure stays inside the validator universe.

35. Hypothesis shrank local-view divergence to a one-edge witness.
    Witness: direct `[0]`, view A has no edge and closure `[0]`; view B has edge `1 -> 0` and closure `[0,1]`. After both views converge to edge `1 -> 0`, both closures are `[0,1]`.

36. Hypothesis shrank early pruning to the minimum retention boundary.
    Witness: evidence observed at slot `0`, slash slot `1`, retention window `0`, direct `[1]`, edge `0 -> 1`. Retained evidence closes to `[0,1]`; pruned evidence closes to `[]`.

37. Hypothesis shrank epoch identity projection to the two-validator boundary.
    Witness: current validators `[0,1]`, no carryover, loose identity projection enabled. Strict epoch-tagged filtering yields empty closure; loose projection of stale direct evidence yields closure `[0,1]`.

38. Hypothesis found the minimal proposer fairness liveness counterexample.
    Witness: a single bonded proposer observes evidence and does not include it. The first slash slot is absent. Appending one fair bonded proposer that includes evidence gives first slash slot `1`. Bounded liveness therefore requires proposer evidence-inclusion fairness or an explicit inclusion rule.

39. Hypothesis shrank partial batch abort to a two-validator witness.
    Witness: bonds `[1,1]`, failure at validator `0`. Order `[0,1]` aborts with vault `0`; order `[1,0]` slashes validator `1` first and aborts with vault `1`. Rollback semantics keep both orders at vault `0`.

40. Hypothesis shrank delimiter-free record-key collision to `(1,10)` and `(11,0)`.
    Delimiter-free keys are both `"110"`, while canonical delimiter keys are `"1:10"` and `"11:0"`. This complements the documented `(1,23)` and `(12,3)` delimiter-free collision.

41. Hypothesis found a minimal weighted closure-bound violation.
    Witness: `n=4`, stakes `[1,1,1,1]`, fault `1`, direct `[2]`, edges `0 -> 1 -> 2`, closure `[0,1,2]`, extra stake `2`, and weighted quorum drop. This is an assumption counterexample, not a failure under the weighted closure-bound theorem.

42. Hypothesis searched edge-order permutations and found no unexpected closure divergence.
    Within the configured quick bound, every generated edge-order permutation agreed with the sorted-edge closure baseline. This reinforces the existing differential-divergence classification but is not proof authority.

43. Frontier-mode Hypothesis search now explores beyond fixed target predicates.
    `--search-mode frontier` runs novelty/coverage scoring, feature-combination coverage, bundle state-machine search, rule-based multi-epoch state-machine search, less-directed multi-epoch trace search, adversarial scheduler search, partition/gossip state-machine search, production-shaped DAG trace generation, defensive adversarial vulnerability campaign search, liveness-as-safety checks, exact-vs-projection differential checks, arithmetic projection stress, generated trace classification, rule-based semantic attack campaign search, attack-objective and objective-guided search, metamorphic and Rust-metamorphic checks, assumption minimization, assumption weakening, precondition fuzzing, Rust differential-corpus emission, and Rust differential-replay fixtures. The quick frontier run covers all 22 frontier axes and finds 0 unexpected divergences. The deep all-mode run (`max_examples=2000`, `state_steps=64`) covers all 29 targeted+frontier axes and also finds 0 unexpected divergences.

44. Novelty/coverage scoring reached five classification buckets.
    The frontier scorer collected witnesses for `bisimilar`, `permitted_bug_fix`, `candidate_boundary`, `projection_risk`, and `assumption_counterexample`. It reached coverage score `12` across features including `atomicity`, `batch`, `closure`, `closure_bound_violation`, `local_divergence`, `lost_update`, `partial_abort`, `tracker`, `view`, and `weighted`. It did not find an `unexpected` witness.

45. Less-directed multi-epoch traces classify stale evidence boundaries automatically.
    The frontier multi-epoch generator searches event traces containing stale direct evidence, epoch advancement, validator rejoin, carryover toggles, loose identity toggles, and citation edges. It found both a `candidate_boundary` trace and a `bisimilar` trace with 0 unexpected classifications.

46. Exact-vs-projection frontier checks compare exact Sage semantics against runtime-style projections.
    The projection search covers retention pruning, fixed-width arithmetic, partial batch abort, and delimiter-free record-key projection. It found `projection_risk` and `bisimilar` witnesses and no unexpected projection divergence.

47. Generated traces are automatically classified.
    The frontier classifier maps generated traces into `bisimilar`, `permitted_bug_fix`, `candidate_boundary`, `projection_risk`, `assumption_counterexample`, or `unexpected`. The quick run covered all non-unexpected buckets and found 0 unexpected traces.

48. Rule-based state machines now cover multi-epoch and semantic campaign exploration.
    Beyond the original `EvidenceLifecycleMachine`, the Hypothesis frontier now runs `MultiEpochFrontierMachine` and `SemanticAttackCampaignMachine` using Hypothesis' rule-based state-machine API. These machines chain stale direct evidence, epoch advancement, rejoin, carryover, loose identity, citation, report, pruning, proposer, stake, and view-merge actions. The invariants require every reached small state to remain in a classified bucket and every closure to stay inside the current validator universe.

49. Semantic attack campaigns compose multiple risk surfaces without finding an unclassified exploit.
    The minimized campaign witnesses cover a bisimilar direct-offender case, a proposer-withholding candidate boundary, a stale-direct plus loose-identity plus pruning projection risk, and a one-edge stake-damage assumption counterexample. These are useful threat-model traces, but none is an unexpected correctness failure under the current assumptions.

50. Attack-objective search finds high-signal regression cases.
    Objective search found extra-stake amplification with direct `[1]`, edges `0 -> 1` and `2 -> 0`, rejoin of validator `2`, closure `[0,1,2]`, and extra stake `2`; a slash-delay schedule with two non-observing unbonded proposers; a direct-plus-prune epoch-advance projection risk; and the stale-direct plus loose-identity projection risk. These should feed regression tests and theorem-precondition documentation.

51. Metamorphic frontier checks found no counterexamples in the configured bounds.
    Edge-order invariance, duplicate-edge idempotence, report-suppression subset behavior, and record normalization all produced `null` counterexamples. Rocq now has a duplicate-edge graph-equivalence theorem specialized to the minimized witness shape, and existing graph-equivalence/record-normalization theorems cover the general proof obligations.

52. Assumption minimization and Rust corpus emission are now explicit outputs.
    Hypothesis minimized five assumption witnesses: closure bound, strict quorum intersection, duplicate-free quorum input, direct-offender universe inclusion, and report suppression. The `--rust-corpus-out` flag emits five deterministic traces covering `bisimilar`, `permitted_bug_fix`, `candidate_boundary`, `projection_risk`, and `assumption_counterexample` classifications for future Rust differential tests.

53. Bundle-based state-machine search reuses generated validators and edges.
    `BundleEvidenceMachine` uses Hypothesis `Bundle`s for validators and edges, then chains direct evidence, edge observation, and reporting rules. The checked invariant keeps active edges visible/unreported and keeps closure inside the current validator universe.

54. Feature-combination coverage targets deeper scenario mixtures.
    The frontier now searches for combinations rather than only individual features: epoch-prune projection risk, stale-direct loose-identity projection risk, stake damage with rejoin, proposer withholding boundary, and report-induced view gap. The quick run covered all five targeted combinations and found 0 unexpected divergences.

55. Adversarial scheduler search models partitions, gossip, pruning, reports, and proposers.
    The minimized scheduler witnesses include partition-induced view divergence, direct-evidence pruning projection risk, and proposer withholding delay. These are classified as candidate boundaries or projection risks, not unexpected exploits.

56. Bounded liveness-as-safety search is explicit.
    Hypothesis shrank the bounded liveness counterexample to bound `1` with one bonded proposer that observes evidence but does not include it. Appending one fair including proposer gives first slash slot `1`, matching the proposer-fairness formal boundary.

57. Arithmetic projection stress covers exact, checked, wrapping, and saturating behavior.
    The minimized stress witness is 8-bit exact sum `256`, with checked overflow, wrapped value `0`, and saturated value `255`; the safe witness is exact sum `0`. Rocq records the 8-bit projection boundary as `arithmetic_projection_stress_boundary_8bit`.

58. Assumption weakening now covers eight dropped preconditions.
    In addition to minimized theorem assumptions, the frontier records dropped-precondition witnesses for proposer fairness, arithmetic envelope, and canonical record-key encoding. The witness set has eight entries and 0 unexpected classifications.

59. The Rust differential corpus expanded from 5 to 11 traces.
    Added deterministic traces cover retention projection, arithmetic overflow, loose epoch identity, scheduler partition view divergence, unfair liveness schedule, and duplicate-edge metamorphic equivalence, while preserving the five classification buckets.

60. Persistent Hypothesis corpus mode is now available.
    `--profile corpus` enables a persistent Hypothesis example database so long-running searches can accumulate frontier examples across sessions. Quick and deep profiles remain deterministic and database-free for reproducible verification runs.

61. Partition/gossip behavior now has an explicit rule-based state machine.
    `PartitionGossipMachine` chains direct evidence, neglect edges, reports, partitions, gossip, merges, pruning, and proposer behavior. Every reached bounded state stayed in a documented `bisimilar`, `candidate_boundary`, or `projection_risk` bucket.

62. Objective-guided campaign scoring prioritizes high-signal traces.
    The new objective scorer ranks generated campaigns by classification severity, extra stake, view gap, slash delay, and feature count. It elevates projection-risk, assumption-counterexample, and candidate-boundary cases for regression and threat-model review without creating a new allowed bisimilarity delta.

63. Rust-facing metamorphic and replay fixtures were added.
    The frontier now emits replay records comparing formal-oracle, fixed-Rust, and Scala/projection expectations, plus metamorphic fixtures for edge-order invariance, duplicate-edge idempotence, validator-renaming equivariance, and record-hash normalization.

64. Precondition fuzzing confirms dropped assumptions are classified.
    The frontier deliberately drops closure-bound, quorum, NoDup, universe, report-suppression, proposer-fairness, arithmetic-envelope, canonical-key, visibility, batch-atomicity, and current-validator-filter preconditions. Every minimized witness landed in a documented boundary, projection-risk, or assumption-counterexample bucket.

65. Deep Sage graph-theoretic attack search documents reverse reachability.
    `deep_threat_model.sage` records the chain `3 -> 2 -> 1 -> 0` with direct offender `0`; closure reaches all four validators. This strengthens the documentation and Rocq examples around two-level closure as reverse reachability to direct equivocators.

66. Deep Sage stake-damage optimization uses Sage MIP with fallback.
    The optimization maximizes extra slashed stake on a fixed neglect-chain pattern under a direct-equivocator stake budget. It remains an assumption counterexample outside the weighted closure-bound theorem precondition.

67. Retention/pruning thresholds are modeled as an optimization surface.
    For each bounded slash delay, the model computes the minimum retention window preserving direct and induced slashability and records the one-slot pruning counterexample as a projection risk.

68. Epoch/churn identity boundaries are replayed in the deep model.
    Strict epoch-tagged identity yields empty current closure for stale evidence, while loose identity or explicit carryover maps the stale direct offender into current closure. This remains a policy boundary, not ordinary bisimilarity.

69. Economic safety envelopes are ranked as projection risks.
    The deep model records exact fixed-width bounds for vault-plus-bond accounting and a small overflow-shape witness, reinforcing that runtime projections must use checked arithmetic or enforce the safe envelope.

70. Minimal counterexamples are now cataloged in one Sage output.
    The deep model groups closure-bound, weighted-bound, current-filter, report-suppression, retention, batch-atomicity, record-key, arithmetic, and proposer-fairness witnesses for promotion into Rocq examples, TLA+ configs, and Rust tests.

71. Threat-vector ranking prioritizes follow-up work.
    The ranking scores projection risks above assumption counterexamples and policy boundaries, with bonuses for stake damage, overflow, retention, identity, withholding, and projection hazards. It is a triage tool for tests and documentation, not proof authority.

72. The Sage scenario schema is shared across frontier scripts.
    `scenario_schema.sage` standardizes validators, stakes, epochs, DAG blocks, direct offenders, neglect edges, reports, slash targets, expected classifications, coverage features, threat scores, and replay fixture shape. This makes DAG, objective, deep-threat, and Hypothesis outputs comparable and replayable.

73. Production-shaped DAG behavior is modeled directly.
    `dag_behavior_model.sage` derives direct-equivocation seeds from duplicate `(sender, seq)` blocks, derives visible-unreported citation edges, distinguishes explicit slash-target reports from neglect, and records retention, epoch/churn, and multi-level reverse-reachability witnesses. The model found no unexpected class; the multi-level DAG citation chain remains an assumption counterexample when the bounded-closure hypothesis is dropped.

74. Objective-frontier modeling ranks witnesses instead of hand-picking them.
    `objective_frontier_model.sage` computes exact Sage objective vectors over classification severity, closure size, extra stake, view gap, retention/pruning exposure, arithmetic boundary distance, and feature coverage. Its Pareto frontier prioritizes retention projection, arithmetic overflow boundary, weighted damage, epoch identity, report suppression, and reachability-assumption witnesses.

75. Deep threat modeling now includes exact attacker and envelope objectives.
    `deep_threat_model.sage` adds minimum attacker stake, maximum quorum loss, withholding-plus-pruning strategy search, and safe-envelope boundary distance. These strengthen the threat catalog without adding allowed bisimilarity deltas: stake/quorum amplification remains an assumption counterexample, withholding/pruning remains a projection/liveness boundary, and arithmetic envelope distance remains a checked projection boundary.

76. Hypothesis frontier generation now includes production-shaped DAG traces.
    `hypothesis_scenario_search.sage` emits DAG block/citation/report traces, classifies direct/report cases as bisimilar, and classifies multi-level reverse reachability as an assumption counterexample. The new generic `--fixture-out`, `--coverage-out`, `--schema-out`, `--top-k`, and `--objectives` flags let long-running Hypothesis searches promote ranked deterministic Sage fixtures.

77. Rust replay coverage consumes the shared Sage fixture shape.
    `casper/tests/slashing/hypothesis_rust_replay_fixtures.rs` parses representative Sage fixtures, checks classifications against the Rust divergence-class mirror, replays report suppression, and replays the weighted quorum-loss boundary. New Rust divergence reasons mirror the Rocq frontier classification reasons and remain candidate-boundary, not allowed bisimilarity deltas.

78. Rocq and TLA+ classify DAG-trace frontier witnesses explicitly.
    Rocq `DRDagTraceBoundary` and TLA+ `DagTraceDivergenceClass` place production-shaped DAG trace findings in documented non-unexpected buckets. This preserves the rule that ordinary behavior must remain bisimilar, while DAG trace assumption counterexamples require review rather than being treated as permitted divergences.

79. Defensive adversarial campaign modeling composes the bug-hunting surfaces.
    `adversarial_campaign_model.sage` now combines production-shaped DAG derivation, multi-node local-view splits, adaptive stake/quorum objectives, exact-vs-runtime projections, differential-oracle rows, mutation/metamorphic variants, and minimized threat-corpus ranking. Its witnesses are classified as candidate boundaries, projection risks, assumption counterexamples, or confirmed-safe rows; no unexpected class is accepted.

80. Production DAG projection gaps are now explicit review boundaries.
    The adversarial campaign model records a DAG where direct-offender-only neglect derivation and broader production-like invalid-citation projection produce different closure sets. This does not authorize a bisimilarity delta; it identifies where production DAG derivation must be pinned to the formal direct-offender evidence rule or documented as a policy boundary.

81. Multi-node local-view disagreement is modeled as a defensive replay case.
    The campaign model records a partitioned view witness where node A and node B compute different closures before convergence and equal closures after sharing the same active evidence view. This strengthens the view-indexed Rocq/TLA+ follow-up and Rust replay fixtures.

82. Exact-vs-runtime projection campaign rows are grouped for bug hunting.
    Retention pruning, fixed-width arithmetic, delimiter-free record keys, and partial batch failure are emitted as a single replayable projection matrix so implementation regressions can be checked against the exact Sage semantics.

83. Hypothesis now searches the adversarial vulnerability campaign frontier.
    `hypothesis_scenario_search.sage` adds `frontier_adversarial_vulnerability_campaign`, which generates and shrinks production DAG traces, multi-node scheduler traces, exact-vs-projection cases, arithmetic boundaries, and adaptive damage campaigns. The quick frontier search must classify every minimized witness and reports any unclassified disagreement as unexpected.

84. Rocq, TLA+, and Rust classify adversarial campaign findings explicitly.
    Rocq adds `DRAdversarialCampaignBoundary` and `DRDifferentialOraclePipelineBoundary`; TLA+ adds `AdversarialCampaignDivergenceClass` and `DifferentialOraclePipelineClass`; Rust mirrors both divergence reasons. These classes remain review-required candidate boundaries, not permitted bug-fix divergences.

## Per-finding source traceability (1-84)

| Finding | Traceability status |
|---------|---------------------|
| 1 | `proof_or_model_strengthening`; not a Rust bug; covered by `slash_iter_reachability_characterization`, `Inv_SlashedWithinClosure`, UC-63, and UC-109. |
| 2 | `assumption_counterexample`; not a Rust bug; shows why T-12 requires the closure-bound precondition; covered by UC-26 and UC-69. |
| 3 | `model_boundary` / `assumption_counterexample`; current Rust is guarded by active/bonded eligibility; covered by `zero_stake_not_direct_offender_under_bonded_precondition` and UC-56. |
| 4 | `assumption_counterexample`; not a Rust bug under bounded weighted closure; covered by weighted closure theorems, TLA+ stake invariants, UC-55, UC-70, and UC-94. |
| 5 | `model_boundary`; no current Rust exploit confirmed; covered by current-validator closure filtering, `BoundaryDivergenceClass`, UC-57, UC-64, and UC-68. |
| 6 | `model_boundary`; not a source bug; covered by visible/unreported-edge semantics, `Inv_NeglectEdgesVisibleUnreported`, UC-58, UC-66, and UC-74. |
| 7 | `proof_or_model_strengthening`; not a Rust bug; covered by graph-equivalence/reachability theorems, UC-59, UC-60, and UC-78. |
| 8 | `projection_risk_guarded`; no current Rust bug confirmed; covered by exact overflow-boundary theorems, arithmetic TLA+ invariants, UC-61, UC-72, and UC-85. |
| 9 | `proof_or_model_strengthening`; no unexpected current Rust divergence confirmed; covered by `DivergenceClass`, `Inv_NoUnexpectedDifferentialDivergence`, UC-39, and UC-80. |
| 10 | `proof_or_model_strengthening`; not a Rust bug; covered by quorum-intersection theorems, active quorum TLA+ invariants, and UC-62. |
| 11 | `proof_or_model_strengthening`; not a Rust bug; covered by closure fixed-point/path-certificate theorems, `Inv_ClosureDepthWithinUniverseBound`, and UC-63. |
| 12 | `proof_or_model_strengthening`; not a Rust bug under no-failure/atomic batch policy; covered by `bm_slash_many_order_independent`, `Inv_BatchNoFailureOrderIndependent`, UC-50, and UC-71. |
| 13 | `model_boundary`; stale evidence must be filtered or explicitly carried; covered by epoch-filter theorems, epoch TLA+ invariants, UC-64, and UC-68. |
| 14 | `proof_or_model_strengthening`; not a Rust bug; reports intentionally remove active neglect edges; covered by report TLA+ invariants, UC-67, and UC-109. |
| 15 | `projection_risk_guarded`; no current Rust bug confirmed; covered by `arithmetic_safe_envelope`, `Inv_ArithmeticSafeEnvelope`, UC-61, UC-85, and UC-97. |
| 16 | `proof_or_model_strengthening`; not a Rust bug; covered by record-normalization theorems, UC-65, and UC-72. |
| 17 | `assumption_counterexample` / `model_boundary`; not a source bug under the honest-edge/precondition model; covered by T-12 preconditions, UC-69, and UC-70. |
| 18 | `model_boundary`; local-view/report-time divergence is expected before convergence; covered by view/report classes, UC-66, UC-67, and UC-90. |
| 19 | `model_boundary`; no current Rust exploit confirmed; covered by carryover policy, `RebondIdentityDivergenceClass`, UC-68, UC-76, and UC-96. |
| 20 | `assumption_counterexample`; not a source bug; covered by assumption examples, `AssumptionDivergenceClass`, UC-69, UC-79, and UC-98. |
| 21 | `assumption_counterexample`; not a Rust bug under T-12W; covered by weighted closure preconditions, UC-70, and UC-94. |
| 22 | `proof_or_model_strengthening`; no unexpected current Rust divergence confirmed; covered by divergence classes, UC-73, and UC-80. |
| 23 | `projection_risk_guarded`; no confirmed current Rust bug; covered by canonical-key, batch, retention, and arithmetic projection guards, UC-71, UC-72, and UC-75. |
| 24 | `proof_or_model_strengthening`; not a Rust bug; scenario corpus rows remain classified and replayed by UC-73. |
| 25 | `model_boundary`; no current Rust exploit confirmed; covered by rebond/carryover policy classes, UC-68, UC-76, and UC-96. |
| 26 | `model_boundary`; not a source bug; covered by view convergence classes, `Inv_SameViewSameClosure`, UC-66, and UC-90. |
| 27 | `model_boundary`; not a safety bug; covered by proposer-fairness boundary theorems/invariants, UC-74, UC-83, and UC-84. |
| 28 | `projection_risk_guarded`; no source bug confirmed; partial abort is unsafe unless atomic/rollback policy is enforced; covered by UC-71. |
| 29 | `projection_risk_guarded`; no current source bug confirmed; covered by temporal-window retention classes, UC-72, and UC-95. |
| 30 | `projection_risk_guarded` / `proof_or_model_strengthening`; canonical pair keys and record normalization cover the witness; covered by UC-65 and UC-75. |
| 31 | `assumption_counterexample`; not a Rust bug under weighted closure-bound assumptions; covered by UC-70 and UC-94. |
| 32 | `proof_or_model_strengthening`; no unexpected current Rust divergence confirmed; covered by `RustReplayDivergenceClass`, UC-73, and UC-80. |
| 33 | `proof_or_model_strengthening`; no unexpected Rust behavior found in configured Hypothesis/Sage bounds; covered by UC-73 and UC-76 through UC-92. |
| 34 | `proof_or_model_strengthening`; active-edge admissibility preserved; covered by `Inv_NeglectEdgesVisibleUnreported`, UC-58, UC-66, and UC-81. |
| 35 | `model_boundary`; local views may differ before convergence; covered by view closure classes, UC-66, and UC-90. |
| 36 | `projection_risk_guarded`; early pruning remains unsafe projection, not a current deletion path; covered by retention classes, UC-72, UC-95, and UC-112. |
| 37 | `model_boundary`; no current Rust exploit confirmed; covered by `RebondIdentityDivergenceClass`, UC-68, UC-76, and UC-96. |
| 38 | `model_boundary`; bounded liveness requires proposer fairness; covered by proposer-fairness classes, UC-74, and UC-84. |
| 39 | `projection_risk_guarded`; no current source bug confirmed; partial abort remains a guarded projection risk covered by UC-71. |
| 40 | `projection_risk_guarded`; current design uses canonical pair keys; delimiter-free projection is unsafe; covered by UC-75. |
| 41 | `assumption_counterexample`; not a Rust bug under T-12W; covered by weighted closure-bound tests UC-70 and UC-94. |
| 42 | `proof_or_model_strengthening`; no source divergence confirmed; covered by graph/metamorphic fixtures UC-78 and UC-91. |
| 43 | `proof_or_model_strengthening`; frontier expansion found no unexpected class; covered by frontier classification and UC-76 through UC-92 plus UC-100. |
| 44 | `proof_or_model_strengthening`; coverage scoring found no unexpected class; covered by divergence replay UC-73 and UC-80. |
| 45 | `model_boundary`; stale/carryover traces stay classified; covered by multi-epoch and epoch/churn tests UC-76 and UC-96. |
| 46 | `projection_risk_guarded`; exact-vs-projection rows remain guarded; covered by UC-72, UC-75, and UC-85. |
| 47 | `proof_or_model_strengthening`; generated traces are promotion-gated by classification; covered by UC-80 and UC-100. |
| 48 | `model_boundary` / `proof_or_model_strengthening`; rule-based state-machine states remain classified; covered by UC-76 and UC-77. |
| 49 | `model_boundary` / `projection_risk_guarded` / `assumption_counterexample`; no unclassified exploit confirmed; covered by `SemanticCampaignDivergenceClass` and UC-77. |
| 50 | `model_boundary` / `projection_risk_guarded` / `assumption_counterexample`; no confirmed current source bug; covered by objective/campaign classes, UC-88, and UC-100. |
| 51 | `proof_or_model_strengthening`; metamorphic checks found no counterexamples; covered by graph/record theorems, UC-78, and UC-91. |
| 52 | `assumption_counterexample` / `proof_or_model_strengthening`; minimized witnesses and corpus classes are replayed by UC-79, UC-80, and UC-98. |
| 53 | `proof_or_model_strengthening`; bundle state-machine traces preserve active-edge invariants; covered by UC-81. |
| 54 | `proof_or_model_strengthening`; feature combinations remain classified; covered by semantic/frontier classes and UC-82. |
| 55 | `model_boundary` / `projection_risk_guarded`; scheduler witnesses remain boundary/projection cases; covered by `SchedulerDivergenceClass`, UC-83, and UC-90. |
| 56 | `model_boundary`; bounded liveness-as-safety fails only without proposer fairness; covered by UC-84. |
| 57 | `projection_risk_guarded`; arithmetic projection stress is guarded by exact boundary theorem/classes; covered by UC-85 and UC-97. |
| 58 | `assumption_counterexample` / `projection_risk_guarded`; dropped preconditions classify as expected; covered by UC-86 and UC-92. |
| 59 | `proof_or_model_strengthening`; Rust differential corpus has no unexpected class; covered by `RustReplayDivergenceClass`, UC-80, and UC-89. |
| 60 | `proof_or_model_strengthening`; persistent corpus mode is search infrastructure, not Rust behavior; covered by UC-87. |
| 61 | `model_boundary`; partition/gossip behavior remains classified; covered by `PartitionGossipDivergenceClass` and UC-90. |
| 62 | `proof_or_model_strengthening`; objective-guided scoring is triage, not proof authority; covered by `ObjectiveGuidedDivergenceClass`, UC-88, and UC-99. |
| 63 | `proof_or_model_strengthening`; Rust-facing metamorphic/replay fixtures remain classified; covered by UC-89 and UC-91. |
| 64 | `assumption_counterexample` / `projection_risk_guarded` / `model_boundary`; precondition fuzzing found no unclassified witness; covered by `PreconditionFuzzingClass` and UC-92. |
| 65 | `proof_or_model_strengthening`; deep reverse-reachability strengthens closure examples; covered by `DagTraceDivergenceClass` and UC-93. |
| 66 | `assumption_counterexample`; stake-damage MIP remains outside weighted bound; covered by `DeepThreatModelDivergenceClass` and UC-94. |
| 67 | `projection_risk_guarded`; pruning thresholds are guarded and current record deletion is not reproduced; covered by UC-95 and UC-112. |
| 68 | `model_boundary`; strict epoch identity filters stale evidence; covered by `RebondIdentityDivergenceClass` and UC-96. |
| 69 | `projection_risk_guarded`; economic envelope violations are projection risks; covered by arithmetic classes and UC-97. |
| 70 | `assumption_counterexample` / `projection_risk_guarded`; minimal catalog witnesses are replayed by UC-98. |
| 71 | `proof_or_model_strengthening`; threat ranking is triage only; covered by UC-99 and documented divergence classes. |
| 72 | `proof_or_model_strengthening`; shared scenario schema is fixture infrastructure; covered by replay fixtures UC-80 and UC-89. |
| 73 | `assumption_counterexample` / `model_boundary`; production-shaped DAG traces are classified by `DRDagTraceBoundary` / `DagTraceDivergenceClass` and replayed by UC-93/UC-100. |
| 74 | `proof_or_model_strengthening`; objective frontier ranks classified rows; covered by `ObjectiveGuidedDivergenceClass`, UC-88, and UC-99. |
| 75 | `assumption_counterexample` / `projection_risk_guarded` / `model_boundary`; deep objectives strengthen threat catalog; covered by `DeepThreatModelDivergenceClass`, UC-94, UC-97, UC-98, and UC-99. |
| 76 | `assumption_counterexample` / `model_boundary`; production-shaped DAG traces classify before promotion; covered by `DagTraceDivergenceClass`, `RustReplayDivergenceClass`, UC-80, and UC-100. |
| 77 | `proof_or_model_strengthening`; Rust replay coverage consumes Sage fixture shape without unexpected divergence; covered by UC-89 and UC-100. |
| 78 | `proof_or_model_strengthening`; DAG-trace frontier witnesses are classified explicitly; covered by `DRDagTraceBoundary`, `DagTraceDivergenceClass`, and UC-100. |
| 79 | `model_boundary` / `projection_risk_guarded` / `assumption_counterexample`; no unclassified adversarial exploit confirmed; covered by adversarial/differential classes and UC-100. |
| 80 | `projection_risk_guarded` / `model_boundary`; production DAG projection gaps are review boundaries, not permitted bug fixes; covered by `RustViewDetectabilityClass` and UC-89/UC-100. |
| 81 | `model_boundary`; multi-node local-view disagreement is expected before convergence; covered by view/partition classes, UC-90, and UC-100. |
| 82 | `projection_risk_guarded`; exact-vs-runtime projection matrix rows are regression guards; covered by UC-72, UC-85, and UC-100. |
| 83 | `model_boundary` / `projection_risk_guarded` / `assumption_counterexample`; vulnerability-campaign frontier found no unclassified current Rust exploit; covered by UC-100. |
| 84 | `proof_or_model_strengthening`; Rocq/TLA+/Rust classify adversarial campaign reasons as review-required, not permitted bug-fix deltas; covered by UC-100. |

## Rocq promotion status

1. Done: `weighted_slash_iter_quorum_preservation` proves that if the total stake of the slash closure is at most the stake fault bound, active stake remains above quorum.

2. Done: `zero_stake_not_direct_offender_under_bonded_precondition` records the eligibility precondition for zero-stake or inactive validators.

3. Done: `restricted_closure_only_from_current_direct_offenders` states the current-validator boundary theorem.

4. Done: `visible_unreported_graph_in` and `visible_reachability_first_edge` state the evidence-visibility admissibility condition.

5. Done: `slash_iter_graph_equiv` and `no_reachability_no_level2_slash` cover duplicate/cycle invariance as corollaries of reachability.

6. Done: `unsigned_overflow_boundary_exact` and `signed_overflow_boundary_exact` identify the exact overflow boundary for bounded projections.

7. Done: `DivergenceClass` and `divergence_allowed` classify bisimilar, permitted bug-fix, candidate-boundary, proposer-fairness boundary, and unexpected divergences.

8. Done: `semantic_campaign_boundary_reasons_require_review`, `adversarial_scheduler_boundary_reasons_require_review`, and `frontier_expansion_reasons_require_review` prove that combined campaign/scheduler/frontier boundary reasons, including DAG trace and adversarial campaign boundaries, are not allowed bisimilarity deltas. The minimized Hypothesis examples for closure-bound, direct-offender universe, duplicate-edge idempotence, report suppression, and 8-bit arithmetic projection stress are closed Rocq examples/theorems.

## TLA+ promotion status

1. Done: `TwoLevelSlashing.tla` includes `BondWeight`, `StakeSum`, `Inv_ActiveStakeAboveWeightedQuorum`, and weighted closure-bound enforcement.

2. Done in the main model as invariant coverage: the witness classes are represented by weighted closure and current-validator eligibility invariants. The concrete negative witnesses remain in Sage because they intentionally violate the closure-bound precondition.

3. Done: `CurrentValidators`, `EvidenceValidators`, `FilteredCurrentClosure`, and `EvidenceProjectionClosure` are modeled.

4. Done: `Visibility`, `Reports`, `VisibleUnreported`, and `Inv_NeglectEdgesVisibleUnreported` are modeled.

5. Done through reachability and graph-equivalence invariants in Rocq, and through Sage edge-case witnesses; TLA+ mirrors the reachability/filtering constraints.

6. Done for exact TLA+/Rocq boundary facts via `Inv_UnsignedArithmeticBoundary` and `Inv_SignedArithmeticBoundary`; checked/wrapping/saturating projection behavior remains a Sage implementation-risk model.

7. Done: `BoundaryDivergenceClass`, `ProposerFairnessDivergenceClass`, and `Inv_NoUnexpectedDifferentialDivergence` classify TLA+ current-boundary and proposer-fairness divergence.

8. Done: `AssumptionDivergenceClass`, `SemanticCampaignDivergenceClass`, `SchedulerDivergenceClass`, `ArithmeticProjectionStressClass`, `PartitionGossipDivergenceClass`, `ObjectiveGuidedDivergenceClass`, `PreconditionFuzzingClass`, `RustReplayDivergenceClass`, `DeepThreatModelDivergenceClass`, `DagTraceDivergenceClass`, `AdversarialCampaignDivergenceClass`, and `DifferentialOraclePipelineClass` classify the combined frontier and deep-threat surfaces in `TwoLevelSlashing.tla`; `Inv_NoUnexpectedDifferentialDivergence` admits only documented buckets.

## Documentation and test follow-ups

Done: `slashing-verification.md`, `slashing-specification.md`, `design/08-two-level-and-collusion.md`, `design/12-failure-modes.md`, `design/14-test-plan.md`, and the top-level design README now document the first finding set and UC-55 through UC-61.

Second promotion status: quorum intersection, closure certificates,
slash-order independence, epoch filtering, evidence propagation/report
suppression, arithmetic safe envelopes, differential fences, and record
normalization have corresponding Sage scripts. The theorem-shaped parts
are promoted to Rocq/TLA+ as `quorum_intersection_by_size`,
`weighted_quorum_intersection_from_disjoint_bound`,
`slash_iter_fixed_point_after_universe_bound`,
`slash_iter_fixed_point_stable`, `bm_slash_many_order_independent`,
`epoch_filter_in`, `arithmetic_safe_envelope`, and
`hashes_equiv_*`; TLA+ mirrors them through active quorum intersection,
weighted quorum intersection, fixed-point-at-bound, epoch eligibility,
report suppression, and safe-envelope invariants.

Documentation/test-plan status: the second finding set is tracked as
Theorem 8.8 in `slashing-specification.md`, T-12C/T-12I/T-12D/T-12E/T-12A
and T-5N/T-IdemMany coverage in `design/14-test-plan.md`, and UC-62
through UC-65 for quorum intersection, fixed-point certificates, epoch
rollover filtering, and record normalization.

Third exploratory pass status: findings 17 through 23 are implemented as
Sage search/model scripts and promoted to Rocq/TLA+/documentation as
view-indexed evidence semantics, epoch-tagged validator identity or
explicit carryover policy, batch slash failure atomicity, canonical
record-key injectivity, evidence-retention preconditions, divergence
classification, and regression tests for every assumption counterexample.

Fourth exploratory corpus status: findings 24 through 32 are implemented
by `scenario_search/corpus_generator.sage`. Existing Rocq/TLA+/docs
coverage already accounts for the epoch, view, batch, retention,
canonicalization, weighted-bound, and differential-classification parts.
The proposer-schedule liveness finding has been promoted as an explicit
proposer-fairness boundary.

Hypothesis, deep-threat, DAG, objective-frontier, and adversarial-campaign
integration status: findings 33 through 84 are implemented by
`hypothesis_search/hypothesis_scenario_search.sage`,
`deep_threat_model.sage`, `dag_behavior_model.sage`,
`objective_frontier_model.sage`, `adversarial_campaign_model.sage`, and the shared
`scenario_schema.sage`. Minimized witnesses are replayed by deterministic
corpus and fixture outputs where applicable. Hypothesis and Sage threat
ranking remain optional search tools; promotion requires a classified
deterministic witness before Rocq, TLA+, or documentation changes.

## Finding 85 - Rust latest-message detectability is broader than direct citation

The adversarial DAG campaign was realigned with production Rust
`EquivocationDetector::is_equivocation_detectable`. A neglect edge is
now modeled from an existing equivocation record, a bonded offender, and
a latest-message view that can expose either multiple equivocation
children or a previously detected hash. Direct citation to one invalid
block is retained only as a projection/harness shortcut. The minimized
witness shows both projection directions: a direct-only projection can
slash an oblivious block that saw only one child, and it can miss a block
that sees a previously detected hash through a nested latest-message
pointer. This is classified as `projection_risk`, not an unexpected
production divergence.

Promotion status: `rust_detectable_view_graph_in` and
`same_rust_detectable_view_same_closure` were added to Rocq;
`RustViewGraph`, `RustViewDetectabilityClass`,
`Inv_RustViewEdgesDetectableUnreported`, and
`Inv_SameRustViewSameClosure` were added to TLA+; the specification and
design docs now state latest-message detectability as the production rule.

## Finding 86 - Rust detector missing-pointer traversal was not total

The Rust-aligned Sage campaign now models the pre-fix behavior where a
latest-message entry whose nested justification omitted the offender's
latest block could abort traversal with `KeyNotFound`. Because the
pre-fix detector iterated a `HashMap`, the same evidence set could either
abort before reaching later decisive evidence or detect a neglected
equivocation if the decisive children happened to be visited first. The
fixed rule treats a missing direct or nested pointer as a
non-contributing view element, then continues over the deterministic
latest-message ordering.

Promotion status: represented by
`sage_rust_detector_totality_and_distinct_child_regression`;
exercised by Rust UC-101, UC-102, UC-104, UC-105, and UC-107, plus
property tests T-9.11 totality and permutation invariance. This is a
`permitted_bug_fix` divergence from the pre-fix Scala/Rust behavior.

## Finding 87 - Rust detector duplicate child paths were over-counted

The Sage model also records a pre-fix false positive: two validators can
cite paths to the same offender child. The old Rust detector stored
children in a `Vec`, so two paths to one child made `len(children) > 1`
and incorrectly classified the current block as
`NeglectedEquivocation`. The fixed rule counts distinct child block
hashes: `detectable ≜ detected_hash_seen ∨ |distinct_child_hashes| ≥ 2`.
This preserves the original two-child semantics for complete views while
eliminating the duplicate-path exploit.

Promotion status: represented by
`sage_rust_detector_totality_and_distinct_child_regression`;
exercised by Rust UC-106, UC-108, and T-9.11 complete-pointer
bisimilarity. This is a `permitted_bug_fix` divergence because the old
behavior could slash a block that had only one distinct offender child in
view.

## Finding 88 - Evidence-denial min-cuts identify the visibility assumption

The expanded Sage frontier computes minimum edge-removal witnesses for
withholding visible-unreported evidence. In the canonical chain
`3 → 2 → 1 → 0`, where `0` is a direct equivocator, removing a single
edge on the path is enough to remove the upstream validator from the
local accountability closure. This is not classified as an unexpected
protocol bug by itself; it is a `candidate_boundary` that makes the
evidence-availability, gossip, retention, and proposer-inclusion
assumptions concrete.

Traceability status: represented by
`sage_evidence_denial_min_cut_search` and
`hypothesis_frontier_adaptive_evidence_denial`, classified as
`model_boundary` in `docs/theory/slashing/slashing-traceability.md`, and
covered by the TLA+ visibility/fairness classes plus Rust UC-83, UC-90,
UC-95, and UC-109. No production Rust bug is confirmed by this witness.

## Finding 89 - Closure has an independent matrix-oracle cross-check

The deep Sage model now checks iterative `DiGraph` reverse closure
against an adjacency-matrix transitive-closure oracle over the bounded
state space. The Hypothesis frontier also searches for discrepancies
between the two oracles. No counterexample was found in the configured
bounds, which strengthens confidence that the executable Sage closure
model is not accidentally baking in an implementation artifact.

Traceability status: represented by
`sage_cross_oracle_closure_consistency` and
`hypothesis_frontier_cross_oracle_closure_consistency`, classified as
`proof_or_model_strengthening`, and covered by the Rocq reachability
characterization, TLA+ closure invariants, and Rust UC-93/UC-109.

## Finding 90 - Detector-totality threats are now searched directly

Hypothesis now generates detector-view contributions rather than only
using fixed deterministic detector witnesses. It shrinks two classes:
missing-pointer order dependence and duplicate-child over-counting. Both
land in the documented `permitted_bug_fix` bucket, matching Bug #11:
missing pointers are non-contributing, and child evidence is counted by
distinct block hash.

Promotion status: represented by
`sage_detector_totality_threat_search` and
`hypothesis_frontier_detector_totality_dag_search`; already promoted to
Rocq/TLA+ through the fixed detector theorems and invariants, and to Rust
through UC-101 through UC-108.

## Finding 91 - Composite attacks need multi-axis fixtures

The new composite frontier forces stake amplification, partition/view
divergence, retention projection, and bounded arithmetic boundaries into
one generated scenario. The highest-priority outcome is still a
classified projection or assumption boundary, not an unexpected
divergence, but the combined witness is useful because it checks that
single-axis mitigations compose.

Traceability status: represented by
`hypothesis_frontier_composite_attack_search`, classified as
`model_boundary`/`projection_risk_guarded`, and covered by TLA+
adversarial/differential-oracle classes plus Rust UC-77, UC-88, and
UC-100. No single current Rust exploit is confirmed by this composite
model witness.

## Finding 92 - Candidate closure invariants survived bounded mining

The expanded frontier mined for counterexamples to direct-subset,
edge-monotonicity, closure-idempotence, duplicate-edge idempotence, and
matrix-oracle consistency. No counterexamples were found in the bounded
Sage/Hypothesis searches. These are useful theorem-strengthening
candidates rather than proof results.

Traceability status: represented by `sage_candidate_invariant_mining`
and `hypothesis_frontier_candidate_invariant_mining`, classified as
`proof_or_model_strengthening`, and covered by the current Rocq
monotonicity/fixed-point/reachability theorems and TLA+ closure
invariants where those properties are promoted.

## Finding 93 - Temporal retention must cover gossip plus inclusion delay

The temporal-window Sage and Hypothesis frontiers synthesize the
inequality:

```
retention_window ≥ gossip_delay + inclusion_delay
```

The minimized unsafe witness has `gossip_delay = 0`,
`inclusion_delay = 1`, and `retention_window = 0`; evidence expires
before the slash can be included, so the projected closure is empty even
though the retained closure is `[0, 1]`. The safe boundary witness raises
`retention_window` to `1`.

Traceability status: represented by `sage_temporal_window_synthesis` and
`hypothesis_frontier_temporal_window_synthesis`, classified as
`projection_risk_guarded`, and covered by `TemporalWindowDivergenceClass`
plus Rust UC-95 and UC-109. The specification states the retention lower
bound; no current Rust source bug is confirmed by the witness alone.

## Finding 94 - Mutation oracles kill known unsafe semantic mutants

The mutation-oracle frontier checks whether existing witnesses distinguish
the fixed semantics from intentionally unsafe mutants. The killed mutants
are: ignoring reported edges, accepting stale identities as current,
counting duplicate detector children twice, and aborting detector
traversal on a missing pointer before a later detected hash.

Promotion status: represented by `sage_mutation_oracle_detection` and
`hypothesis_frontier_mutation_oracle_detection`. These are regression
quality checks rather than new correctness theorems, but they identify
where mutation-style Rust tests would be high value.

## Finding 95 - Rebonded identity requires epoch-tagged identity or carryover

The rebond lifecycle frontier now requires the intended churn shape:
stale evidence for one validator nonce, an epoch advance, loose identity
projection, and rebond of the same validator under a different nonce. The
strict epoch-tagged model produces an empty current closure, while loose
identity projection can resurrect stale evidence against the rebonded
identity.

Traceability status: represented by `sage_rebond_identity_lifecycle` and
`hypothesis_frontier_rebond_identity_lifecycle`, classified as
`model_boundary`, and covered by `carryover_policy`,
`RebondIdentityDivergenceClass`, and Rust UC-68, UC-76, and UC-96. The
specification distinguishes epoch-tagged identity from explicit
pending-slash carryover policy.

## Finding 96 - Equivocation-record lifecycle is a retention surface

The record-lifecycle frontier checks monotone insertion, detected-hash
normalization, report filtering, and the early-delete projection risk. A
record with detected hashes `{1, 2}` that is deleted before finalization
can erase the exact evidence later latest-message views need for
detectability.

Traceability status: represented by `sage_record_lifecycle_projection`
and `hypothesis_frontier_record_lifecycle_state_machine`, classified as
`not_reproduced_in_rust` for the current Rust source and
`projection_risk_guarded` for unsafe projections. The current Rust tracker
has no delete/prune path, detector updates retain existing detected hashes,
and guarded insertion avoids overwriting an existing record. Formal coverage
is `current_rust_record_update_retains_all_detected_hashes`,
`RecordLifecycleDivergenceClass`, and
`Inv_CurrentRustRecordLifecycleRetainsRecords`; Rust coverage is UC-72,
UC-95, UC-109, and UC-112.

## Finding 97 - Worst-case closure depth reaches |Validators| - 1

Closure-depth extremal search found a four-validator chain whose
accountability closure reaches depth `3`, matching the candidate bound
`depth ≤ |Validators| - 1`. No counterexample above that bound was found
in the bounded Hypothesis frontier.

Traceability status: represented by `sage_closure_depth_extremal` and
`hypothesis_frontier_closure_depth_extremal_search`, classified as
`proof_or_model_strengthening`, and covered by
`slash_iter_fixed_point_after_universe_bound`, TLA+ closure-prefix
coverage, and Rust UC-63, UC-93, and UC-109.

## Finding 98 - Evidence addition is monotone in a fixed validator universe

The Sage and Hypothesis monotonicity frontiers searched for a case where
adding direct equivocation evidence or adding active neglect edges removes
a validator from the closure. No such case was found. The modeled law is:

```
S₀ ⊆ S₁ ∧ G₀ ⊆ G₁ ⇒ closure(G₀, S₀) ⊆ closure(G₁, S₁)
```

This is useful because production gossip and record replay often merge
evidence incrementally. The fixed-universe theorem says those merges can
only preserve or increase accountable validators; any apparent shrinkage
must come from a documented projection boundary such as validator-set
filtering, report suppression, pruning, or epoch carryover.

Promotion status: represented by `sage_evidence_monotonicity_analysis`
and `hypothesis_frontier_evidence_monotonicity`. Promoted into Rocq as
`slash_iter_initial_graph_monotone` and into TLA+ as
`Inv_InitialEvidenceMonotonicity`.

## Finding 99 - Merged evidence views over-approximate each local view

The view-merge frontier exhaustively checked small graphs and searched
larger generated witnesses for merge-order dependence. It found no
counterexample to:

```
closure(G₁, S) ⊆ closure(G₁ ∪ G₂, S)
closure(G₂, S) ⊆ closure(G₁ ∪ G₂, S)
closure(G₁ ∪ G₂, S) = closure(G₂ ∪ G₁, S)
```

This supports a confluence-style design rule: merging independently
observed evidence views should not hide a slashable neglect path. It does
not remove visibility or retention assumptions; it says that once the
same evidence is retained and in scope, merge order is not a threat.

Promotion status: represented by `sage_view_merge_confluence` and
`hypothesis_frontier_view_merge_confluence`. Promoted into Rocq as the
`union_neglect_graph` closure theorems and into TLA+ as
`Inv_ViewMergeOverapproximatesInputs` and `Inv_ViewMergeCommutative`.

## Finding 100 - Minimal slash bases give compact accountability fixtures

Minimal-basis search extracts the smallest edge set that explains a
target slash in a transitive neglect chain. For the chain
`3 → 2 → 1 → 0` with direct offender `0`, target `3` requires all three
active evidence edges; removing any one edge breaks the proof of
reachability to direct evidence.

This is not a vulnerability by itself. It is useful for regression tests,
documentation examples, and threat analysis because it distinguishes
minimal accountability evidence from redundant evidence.

Promotion status: represented by `sage_minimal_slash_basis_catalog` and
`hypothesis_frontier_minimal_slash_basis`. The property is covered by the
Rocq reachability characterization and should be replayed in TLA+/Rust
fixtures as compact use cases.

## Finding 101 - Record-key namespace separation remains a projection risk

The record-key namespace frontier rechecked delimiter-free encodings. A
projection such as `validator_digits || seq_digits` collides:

```
([1], [1, 0]) ↦ [1, 1, 0]
([1, 1], [0]) ↦ [1, 1, 0]
```

Canonical tuple keys do not collide. This confirms the existing design
choice that equivocation records must be keyed by the pair
`(validator, baseSeqNum)` or by an encoding with explicit domain
separation.

Promotion status: represented by `sage_record_key_namespace_projection`
and `hypothesis_frontier_record_key_namespace_projection`. Rocq already
proves `canonical_key_pair_injective` and records delimiter-free
collision examples; TLA+ checks `Inv_CanonicalRecordKeyInjective`.

## Finding 102 - Detector traversal needs finite-domain branching fuel

The detector traversal frontier generated reachable cycles in an
abstract creator-justification graph. A traversal that follows edges
without a visited set or finite fuel can loop forever on a cycle such as:

```
0 → 1 → 2 → 1
```

The fixed model uses finite-domain branching closure with fuel bounded
by the block universe. This is a projection risk for unsafe traversal
implementations, not a bug in the current formal model once the
branching fuel theorem is present.

Promotion status: represented by `sage_detector_traversal_termination`
and `hypothesis_frontier_detector_traversal_termination`. Promoted into
Rocq as `branch_traversal_fixed_after_domain_bound` and into TLA+ as
`Inv_DetectorTraversalFiniteFuel` and `Inv_DetectorTraversalInDomain`.

## Finding 103 - Detector contributions are order-independent

The detector-contribution frontier permuted latest-message contributions
including missing pointers, duplicate offender-child hashes, distinct
child hashes, and already-detected hashes. No permutation changed the
fixed detector result:

```
missing ↦ ∅
duplicate child ↦ one child
two distinct children ↦ detectable
detected hash ↦ detectable
```

Promotion status: represented by `sage_detector_contribution_confluence`
and `hypothesis_frontier_detector_contribution_confluence`. This is
covered by the existing T-9.11 Rocq detector lemmas, TLA+ fixed-detector
invariants, and Rust detector permutation tests.

## Finding 104 - Closure fixed points are replay-idempotent

The closure fixed-point frontier searched for a case where replaying
closure from its own result changes the result again. No counterexample
was found. This is the executable version of the fixed-point theorem:

```
closure(G, closure(G, S)) = closure(G, S)
```

Promotion status: represented by `sage_closure_fixed_point_idempotence`
and `hypothesis_frontier_closure_fixed_point_idempotence`. Covered by
Rocq fixed-point stability and TLA+ closure-stability invariants.

## Finding 105 - Report retention prevents acknowledged-edge reactivation

The report-retention frontier modeled an acknowledged neglect edge that
remains visible while its report is pruned. If the report is removed too
early, the active-edge projection can reactivate an already acknowledged
edge and re-expand closure.

Promotion status: represented by `sage_report_retention_reactivation`
and `hypothesis_frontier_report_retention_reactivation`. This should be
documented as a retention-window policy: reports and records must remain
at least as long as the visible evidence they suppress, or the local view
must be classified as a projection risk.

## Finding 106 - Neglect cycles do not create slash seeds

The no-seed cycle frontier generated cyclic neglect graphs with an empty
direct-equivocator set. The closure stayed empty:

```
closure(G, ∅) = ∅
```

This blocks a class of false-positive slashing bugs where cyclic
citations are incorrectly treated as self-justifying slash evidence.

Promotion status: represented by `sage_no_seed_cycle_safety` and
`hypothesis_frontier_no_seed_cycle_safety`. Promoted into Rocq as
`slash_iter_empty_initial_empty` and into TLA+ as
`Inv_NoDirectSeedNoClosure`.

## Finding 107 - Operational slash history matches closure prefixes

The slash-history frontier compares the state-machine update

```
slashed₀ = ∅
slashed₁ = direct
slashedᵢ₊₁ = slashedᵢ ∪ {v | G[v] ∩ slashedᵢ ≠ ∅}
```

against the mathematical closure prefix at each level. No mismatch was
found. The same witness also records that accumulated slash history must
not be undone by a later pruned evidence projection.

Promotion status: represented by `sage_slash_history_prefix` and
`hypothesis_frontier_slash_history_prefix`. Promoted into TLA+ as
`Inv_SlashedEqualsClosurePrefix`; Rocq already proves the corresponding
closure reachability characterization.

## Finding 108 - Neglect-edge orientation is semantically load-bearing

The edge-orientation frontier found the minimal witness:

```
direct = {0}
edge    = 1 → 0
```

With the specified orientation, validator `1` is slashable because it
neglected offender `0`. If an implementation reverses the edge, closure
stays `{0}`. This is a projection-risk witness for any implementation or
adapter that flips `neglecter → offender` into `offender → neglecter`.

Promotion status: represented by `sage_edge_orientation_sanity` and
`hypothesis_frontier_edge_orientation_sanity`. Covered by the Rocq
reverse-reachability characterization and Rust orientation regression
fixtures; a TLA+ classification can be added if a separate orientation
projection model is introduced.

## Finding 109 - Redundant evidence paths increase denial cost

The redundant-path frontier models two independent paths from target `3`
to direct offender `0`:

```
3 → 1 → 0
3 → 2 → 0
```

Removing any single edge is insufficient to remove `3` from closure; the
minimal evidence-denial set has size `2`. This is useful for threat
modeling because evidence redundancy converts a single withholding point
into a multi-edge denial requirement.

Promotion status: represented by `sage_redundant_path_denial_cost` and
`hypothesis_frontier_redundant_path_denial_cost`. Covered by the
reachability characterization and promoted to Rust regression fixtures.

## Finding 110 - Slash targets are not self-authorizing evidence

The slash-target authorization frontier treats block slash-target lists
as reports/acknowledgements, not as direct slash seeds. With no direct
equivocation evidence, the authorized closure is empty; a projection that
turns slash targets into direct offenders would slash unsupported
validators.

Promotion status: represented by `sage_slash_target_authorization` and
`hypothesis_frontier_slash_target_authorization`. Promoted into Rocq via
`slash_iter_empty_initial_empty`, into TLA+ via
`Inv_NoDirectSeedNoClosure`, and into Rust unauthorized-target fixtures.

## Finding 111 - Reports are pair-scoped

The report-namespace frontier modeled one reporter with two visible
edges:

```
1 → 0
1 → 2
```

Reporting `1 → 0` suppresses only that pair. A projection that treats a
report by `1` as suppressing every edge from `1` loses the still-active
`1 → 2` edge and can miss an accountable validator.

Promotion status: represented by `sage_report_namespace_isolation` and
`hypothesis_frontier_report_namespace_isolation`. Promoted into Rocq as
`unreported_visible_edge_remains_active`, into TLA+ as
`Inv_UnreportedVisibleEdgesRemainActive`, and into Rust report-namespace
fixtures.

## Finding 112 - Report growth is closure-antitone

For a fixed visible evidence view and fixed direct seed, adding reports
can only remove active edges:

```
R₀ ⊆ R₁ ⇒ closure(visible ∖ R₁, S) ⊆ closure(visible ∖ R₀, S)
```

This captures the intended report semantics: acknowledgements suppress
neglect edges; they never create new slashability.

Promotion status: represented by `sage_report_antitone_closure` and
`hypothesis_frontier_report_antitone_closure`. Promoted into Rocq as
`view_closure_reports_antimonotone`, into TLA+ as
`Inv_ReportGrowthCannotExpandViewClosure`, and into Rust property tests.

## Finding 113 - Reports do not suppress direct evidence

Reports acknowledge already-handled evidence, but they do not erase the
direct equivocator seed. The frontier searched for report sets that
remove a direct offender from closure and found none.

Promotion status: represented by `sage_direct_seed_report_dominance` and
`hypothesis_frontier_direct_seed_report_dominance`. Promoted into Rocq
through `slash_iter_monotone`, into TLA+ as
`Inv_ReportsDoNotSuppressDirectEvidence`, and into Rust property tests.

## Finding 114 - Closure is validator-renaming equivariant

The validator-renaming frontier permuted validator identifiers and
checked that closure permutes with them. This catches accidental
dependence on numeric validator order or map iteration order in
implementations and adapters.

Promotion status: represented by `sage_validator_renaming_equivariance`
and `hypothesis_frontier_validator_renaming_equivariance`. Covered by
Rust property fixtures, the Rocq theorem
`slash_iter_validator_renaming_equiv`, and the TLA+ invariant
`Inv_ValidatorRenamingEquivariance`.

## Finding 115 - Bisimilarity deltas are classified explicitly

The delta-guard frontier separates harmless representation differences
from semantic projection risks:

```
duplicate edges  ↦ bisimilar
edge order       ↦ bisimilar
reversed edges   ↦ projection_risk
slash target seed ↦ projection_risk
```

This is the executable policy behind "maintain bisimilarity except for
documented bug fixes or documented projection boundaries."

Promotion status: represented by `sage_bisimilarity_delta_guard` and
`hypothesis_frontier_bisimilarity_delta_guard`. Covered by existing
Rocq/TLA+ divergence classifications and Rust delta-guard fixtures.

## Finding 116 - Horizon campaigns compose independent boundaries

`horizon_search_model.sage` and Hypothesis `--search-mode horizon`
compose retention, gossip delay, proposer inclusion, epoch/rebond
identity, weighted damage, Rust detector contribution gates, arithmetic
projection, partition/merge, and metamorphic cross-oracle checks into
one search frontier. The deterministic Sage self-test covers eight
horizon axes and the Hypothesis horizon profile covers five generated
axes; both classify every witness as confirmed-safe, projection-risk,
candidate-boundary, or assumption-counterexample and found no unexpected
class in the configured bounds.

The promoted details are:

- retention must cover the combined gossip and proposer-inclusion delay,
  not only local observation time;
- proposer evidence-inclusion fairness remains the liveness assumption
  that turns visible evidence into bounded slash inclusion;
- missing detector pointers are non-contributing, duplicate child paths
  count once, and two distinct children or a detected hash remain
  decisive;
- stale rebond evidence is filtered by epoch-tagged identity unless an
  explicit carryover policy maps it forward;
- weighted damage remains an assumption counterexample outside the
  bounded-closure precondition;
- retained view merge over-approximates partitioned local views;
- checked arithmetic rejects vault totals that wrapping arithmetic would
  silently project; and
- edge-order invariance agrees with an independent adjacency-matrix
  reverse-reachability oracle.

Promotion status: represented by `sage_horizon_*` records and
`hypothesis_horizon_*` records. Rust UC-110 replays the stable horizon
fixtures. Rocq `DRHorizonCampaignBoundary` and TLA+
`HorizonCampaignDivergenceClass` classify the composed horizon frontier
as review-required rather than a permitted bug-fix delta; the component
properties are covered through retention, fairness, detector,
reachability, report, arithmetic, and divergence families.

## Finding 117 - Horizon-v2 aligns the search frontier with Rust detector DAG and lifecycle details

`horizon_v2_search_model.sage` and Hypothesis
`--search-mode horizon-v2` add a Rust-shaped second horizon. The model
keeps the exact detector contribution rule in view:

```
detectable(view) ⇔ detected_hash(view) ∨ |canonical_child_hashes(view)| ≥ 2
```

Missing latest-message pointers contribute `∅`; duplicate or same-branch
canonical child paths count once; two distinct canonical children or a
previously detected hash are decisive. The same campaign also combines
multi-record tracker lifecycle, finality-aware retention, weighted damage
objectives, redundant evidence-denial paths, epoch/era identity
boundaries, and generated exact-vs-projection classification rows.

The promoted details are:

- detected hashes must remain available through the dependency checks
  that may use them; early record deletion is a projection risk, not a
  confirmed current Rust source bug;
- finality-aware evidence retention needs
  `retention_window ≥ finality_depth + gossip_delay + inclusion_delay`
  for bounded availability claims;
- low direct stake can still amplify into high closure stake when the
  weighted closure-bound precondition is removed;
- redundant evidence paths increase the minimum withheld-edge cost for
  hiding a target from closure;
- stale evidence across an era boundary stays filtered unless the
  protocol explicitly defines an epoch-tagged carryover policy; and
- generated rows are classified only as `bisimilar`,
  `candidate_boundary`, `projection_risk`, or
  `assumption_counterexample` in the configured bounds.

Promotion status: represented by `sage_horizon_v2_*` records and
`hypothesis_horizon_v2_*` records. Rust UC-111 replays the stable
horizon-v2 fixtures. Rocq `DRHorizonV2Boundary` and TLA+
`HorizonV2DivergenceClass` classify the new frontier as review-required
rather than a permitted bisimilarity delta; the component properties are
covered through detector, record-lifecycle, temporal-retention,
weighted-bound, reachability, epoch-identity, and divergence families.
