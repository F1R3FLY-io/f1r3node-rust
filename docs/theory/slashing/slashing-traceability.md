# Slashing Traceability Ledger

This ledger records how every current Sage/Hypothesis finding is treated
by the Rust implementation, Rocq proofs, TLA+ models, tests, and
documentation. It is deliberately conservative: a generated witness is
not a Rust vulnerability unless it is reproduced on the production Rust
path or contradicts a production-path invariant.

## Status Vocabulary

| Status | Meaning |
|--------|---------|
| `confirmed_current_bug` | A behavior is reproduced on the current production Rust path and requires a Rust source fix. |
| `confirmed_fixed_bug` | A real pre-fix Scala or Rust behavior was confirmed and the current Rust/formal behavior intentionally differs. |
| `not_reproduced_in_rust` | The witness describes a model or projection behavior, but the current production Rust path does not exhibit the bug. |
| `model_boundary` | The behavior depends on an explicit scope, liveness, visibility, epoch, churn, or fairness boundary. |
| `projection_risk_guarded` | An unsafe adapter or implementation projection can diverge from the mathematical model; current Rust is guarded by tests/specification or needs only a regression guard. |
| `assumption_counterexample` | The witness proves that a theorem precondition is necessary. |
| `proof_or_model_strengthening` | The finding strengthens the mathematical specification without changing production behavior. |
| `needs_source_audit` | The finding is useful, but the production Rust path has not yet been audited closely enough to classify it as fixed, absent, or boundary-only. |

## Promotion Rule

Sage and Hypothesis generate finite witnesses. Rocq and TLA+ carry the
formal authority. Rust source changes are allowed only for
source-confirmed production-path bugs, protocol-hardening changes that
enforce the formal specification, or tests that lock an already-fixed
behavior in place.

The required trace for each finding is:

```
Sage/Hypothesis witness → classification → Rocq/TLA+ artifact → Rust test
                         → specification/design/threat-model entry
```

If the classification is `model_boundary`, `projection_risk_guarded`, or
`assumption_counterexample`, the correct action is documentation,
formalization, and regression coverage, not a production Rust behavior
change.

## Findings 1-84

| Finding | Classification | Rust source status | Formal artifact | Rust test | Documentation action |
|---------|----------------|--------------------|-----------------|-----------|----------------------|
| 1 Unweighted neglect closure is reverse reachability | `proof_or_model_strengthening` | Not a Rust bug; formalizes intended two-level closure semantics | `slash_iter_reachability_characterization`; `Inv_SlashedWithinClosure` | UC-63, UC-109 | Keep as the canonical closure interpretation. |
| 2 Minimal unweighted quorum drop at `n=4` | `assumption_counterexample` | Not a Rust bug; witness violates T-12 closure-bound precondition | T-12 quorum preservation precondition; `Inv_ActiveSetAboveQuorum` under enforced bound | UC-26, UC-69 | Document as why `|closure| ≤ F` is required. |
| 3 Weighted zero-stake edge case | `model_boundary` / `assumption_counterexample` | Current source is guarded by active/bonded eligibility; zero-stake direct offenders are outside the production evidence domain | `zero_stake_not_direct_offender_under_bonded_precondition`; weighted closure theorems | UC-56 | Keep as eligibility precondition, not a source bug. |
| 4 Damage-optimization chain amplification | `assumption_counterexample` | Not a Rust bug; violates bounded slash-closure assumptions | `weighted_slash_iter_quorum_preservation`; `Inv_ActiveStakeAboveWeightedQuorum` | UC-55, UC-70, UC-94 | Keep as weighted-bound motivation. |
| 5 Validator-set boundary filtering | `model_boundary` | No current Rust exploit confirmed; stale/out-of-set evidence is a policy boundary | `restricted_closure_only_from_current_direct_offenders`; `BoundaryDivergenceClass` | UC-57, UC-64, UC-68 | Keep current-validator filtering explicit. |
| 6 Evidence withholding accountability gap | `model_boundary` | Not a source bug; describes visibility/fairness limits before evidence availability | `visible_unreported_graph_in`; `Inv_NeglectEdgesVisibleUnreported` | UC-58, UC-66, UC-74 | Document as evidence-availability boundary. |
| 7 Duplicate, self-edge, and cycle behavior | `proof_or_model_strengthening` | Not a Rust bug; graph normalization agrees with reachability semantics | `slash_iter_graph_equiv`; `no_reachability_no_level2_slash` | UC-59, UC-60, UC-78 | Keep as graph-edge regression corpus. |
| 8 Bounded arithmetic projection overflow | `projection_risk_guarded` | Current correctness requires checked/exact arithmetic envelopes; wrapping/saturating projections are unsafe | `unsigned_overflow_boundary_exact`; `signed_overflow_boundary_exact`; arithmetic TLA+ invariants | UC-61, UC-72, UC-85 | Keep fixed-width arithmetic as projection risk. |
| 9 Differential bisimilarity found no unexpected divergence | `proof_or_model_strengthening` | No current Rust divergence confirmed beyond classified bug fixes/boundaries | `DivergenceClass`; `divergence_allowed`; `Inv_NoUnexpectedDifferentialDivergence` | UC-39, UC-80 | Keep as differential search evidence. |
| 10 Weighted quorum intersection | `proof_or_model_strengthening` | Not a Rust bug; strengthens quorum theorem coverage | `quorum_intersection_by_size`; `weighted_quorum_intersection_from_disjoint_bound`; quorum TLA+ invariants | UC-62 | Keep as theorem/test link. |
| 11 Closure certificates and slash rounds | `proof_or_model_strengthening` | Not a Rust bug; confirms closure certificate structure | `slash_iter_fixed_point_after_universe_bound`; `Inv_ClosureDepthWithinUniverseBound` | UC-63 | Keep path-certificate fixture. |
| 12 Batch slash order independence | `proof_or_model_strengthening` | Not a Rust bug under no-failure/atomic policy | `bm_slash_many_order_independent`; `Inv_BatchNoFailureOrderIndependent` | UC-50, UC-71 | Keep partial-failure policy separated. |
| 13 Epoch/current-validator filtering | `model_boundary` | No current Rust exploit confirmed; stale evidence must be filtered or explicitly carried | `epoch_filter_in`; `Inv_EpochEligibleInCurrent`; `Inv_StaleEvidenceNotEligible` | UC-64, UC-68 | Keep epoch eligibility explicit. |
| 14 Report-time nonmonotonicity | `proof_or_model_strengthening` | Not a Rust bug; reports intentionally remove active neglect edges | `reported_edge_not_active`; `Inv_ReportsSuppressNeglectEdges`; `Inv_ReportGrowthCannotExpandViewClosure` | UC-67, UC-109 | Document report monotonicity correctly. |
| 15 Arithmetic safe envelope | `projection_risk_guarded` | No current source bug confirmed; unsafe only if runtime projections exceed checked envelope | `arithmetic_safe_envelope`; `Inv_ArithmeticSafeEnvelope` | UC-61, UC-85, UC-97 | Keep envelope bounds tied to implementation tests. |
| 16 Record normalization | `proof_or_model_strengthening` | Not a Rust bug; record equality is set/key based, not iteration-order based | `hashes_equiv_*`; record uniqueness/monotonicity theorems | UC-65, UC-72 | Keep normalization in record spec/design. |
| 17 Adversarial timing quorum-drop witness | `assumption_counterexample` / `model_boundary` | Not a source bug unless honest validators create slashable visible-unreported neglect edges | T-12 count/stake preconditions; `AssumptionDivergenceClass` | UC-69, UC-70 | Keep as adversarial boundary fixture. |
| 18 Local-view and report-time divergence | `model_boundary` | Not a source bug; local views may differ before convergence and reports shrink active edges | view/report closure theorems; `ViewDivergenceClass` | UC-66, UC-67, UC-90 | Keep view-indexed semantics documented. |
| 19 Epoch churn and rebond identity | `model_boundary` | No current Rust exploit confirmed from model alone; carryover is policy-controlled | `carryover_policy`; `RebondIdentityDivergenceClass` | UC-68, UC-76, UC-96 | Keep epoch-tagged identity/carryover policy explicit. |
| 20 Assumption counterexample catalog | `assumption_counterexample` | Not a Rust bug; each witness violates a theorem precondition | theorem assumption examples; `AssumptionDivergenceClass` | UC-69, UC-79, UC-98 | Keep as precondition test corpus. |
| 21 Weighted MIP amplification | `assumption_counterexample` | Not a Rust bug under T-12W; violates weighted closure-bound hypothesis | weighted closure theorem family; `AssumptionDivergenceClass` | UC-70, UC-94 | Keep MIP witness as bound motivation. |
| 22 Differential trace generation | `proof_or_model_strengthening` | No unexpected current Rust divergence confirmed | `DivergenceClass`; `BoundaryDivergenceClass`; proposer-fairness class | UC-73, UC-80 | Keep generated traces replayable. |
| 23 Implementation projection risks | `projection_risk_guarded` | No confirmed current Rust bug; unsafe projections are explicitly guarded | canonical-key, batch, retention, and arithmetic formal classes | UC-71, UC-72, UC-75 | Keep projection risks out of production semantics. |
| 24 Scenario corpus generator coverage | `proof_or_model_strengthening` | Not a Rust bug; deterministic corpus contains classified witnesses only | scenario-corpus classification; existing divergence classes | UC-73 | Keep as replay corpus. |
| 25 Multi-epoch rebond/carryover corpus | `model_boundary` | No current Rust exploit confirmed; loose identity/carryover are policy choices | `carryover_policy`; `RebondIdentityDivergenceClass` | UC-68, UC-76, UC-96 | Keep strict vs loose identity separated. |
| 26 Partial-synchrony view convergence | `model_boundary` | Not a source bug; pre-convergence local views may differ | `Inv_SameViewSameClosure`; `ViewDivergenceClass` | UC-66, UC-90 | Document convergence assumption. |
| 27 Proposer inclusion fairness | `model_boundary` | Not a safety bug; bounded liveness needs an inclusion fairness assumption | `proposer_fairness_boundary_requires_review`; `Inv_ProposerFairnessForBoundedLiveness` | UC-74, UC-83, UC-84 | Keep liveness assumption explicit. |
| 28 Batch failure policies | `projection_risk_guarded` | No current source bug confirmed; partial abort is unsafe unless atomic/rollback policy is enforced | `bm_slash_many_abort_order_dependent`; `Inv_PartialBatchFailureRequiresAtomicPolicy` | UC-71 | Keep batch failure policy guarded. |
| 29 Evidence retention minimum window | `projection_risk_guarded` | No current source bug confirmed; unsafe only if retention violates slash-delay lower bound | `TemporalWindowDivergenceClass`; `Inv_TemporalWindowBoundary` | UC-72, UC-95 | Document minimum retention window. |
| 30 Record canonicalization | `projection_risk_guarded` / `proof_or_model_strengthening` | Current formal design uses canonical pair keys; delimiter-free projections are unsafe | `canonical_key_pair_injective`; record-normalization theorems | UC-65, UC-75 | Keep canonical-key requirement. |
| 31 Integrated economic attack optimization | `assumption_counterexample` | Not a Rust bug under weighted closure-bound precondition | weighted closure theorem family; `AssumptionDivergenceClass` | UC-70, UC-94 | Keep as economic boundary witness. |
| 32 Differential trace corpus classes | `proof_or_model_strengthening` | No unexpected Rust divergence confirmed | `DivergenceClass`; `RustReplayDivergenceClass` | UC-73, UC-80 | Keep trace classes replayable. |
| 33 Hypothesis integration | `proof_or_model_strengthening` | No unexpected Rust behavior found in configured bounds | frontier classification families | UC-73, UC-76 through UC-92 | Keep Hypothesis as optional search, not proof authority. |
| 34 Active-edge admissibility state machine | `proof_or_model_strengthening` | Not a Rust bug; preserves active-edge precondition | `visible_unreported_graph_in`; `Inv_NeglectEdgesVisibleUnreported` | UC-58, UC-66, UC-81 | Keep active-edge invariant. |
| 35 Local-view divergence shrink | `model_boundary` | Not a source bug; equal active views agree after convergence | `Inv_SameViewSameClosure`; `ViewDivergenceClass` | UC-66, UC-90 | Keep as minimal view-boundary fixture. |
| 36 Early pruning retention boundary | `projection_risk_guarded` | Current Rust record deletion not reproduced; early pruning remains unsafe projection | `TemporalWindowDivergenceClass`; retention invariants | UC-72, UC-95, UC-112 | Keep retention/pruning guarded. |
| 37 Epoch identity projection shrink | `model_boundary` | No current Rust exploit confirmed; stale evidence depends on identity/carryover policy | `RebondIdentityDivergenceClass`; carryover theorem | UC-68, UC-76, UC-96 | Keep stale identity classified. |
| 38 Minimal proposer-fairness counterexample | `model_boundary` | Not a safety bug; bounded liveness requires fair evidence inclusion | proposer-fairness Rocq/TLA+ boundary | UC-74, UC-84 | Keep liveness/fairness assumption explicit. |
| 39 Partial batch abort shrink | `projection_risk_guarded` | No current source bug confirmed; partial abort semantics are unsafe projection | batch failure theorems; `Inv_PartialBatchFailureRequiresAtomicPolicy` | UC-71 | Keep atomic/rollback policy. |
| 40 Delimiter-free record-key collision | `projection_risk_guarded` | Current formal design requires canonical pair keys; delimiter-free projection is unsafe | `canonical_key_pair_injective`; collision examples | UC-75 | Keep canonical encoding mandatory. |
| 41 Minimal weighted closure-bound violation | `assumption_counterexample` | Not a Rust bug under T-12W; violates weighted precondition | weighted closure theorem family; `AssumptionDivergenceClass` | UC-70, UC-94 | Keep as minimized weighted witness. |
| 42 Edge-order permutation search | `proof_or_model_strengthening` | No source divergence confirmed; closure is edge-order insensitive | graph equivalence theorems; divergence classes | UC-78, UC-91 | Keep as metamorphic regression. |
| 43 Frontier-mode search expansion | `proof_or_model_strengthening` | No unexpected current Rust divergence found in configured frontier bounds | `frontier_expansion_reasons_require_review`; frontier TLA+ classes | UC-76 through UC-92, UC-100 | Keep frontier outputs classified before promotion. |
| 44 Novelty/coverage scoring | `proof_or_model_strengthening` | Not a Rust bug; coverage tool found no unexpected class | divergence classification family | UC-73, UC-80 | Keep as search coverage metric. |
| 45 Less-directed multi-epoch traces | `model_boundary` | No current source exploit confirmed; stale/carryover cases stay classified | epoch/carryover formal classes | UC-76, UC-96 | Keep multi-epoch traces classified. |
| 46 Exact-vs-projection frontier checks | `projection_risk_guarded` | No confirmed current bug; unsafe projection rows stay outside exact semantics | projection and arithmetic/retention/key/batch classes | UC-72, UC-75, UC-85 | Keep exact/projection split. |
| 47 Generated trace classification | `proof_or_model_strengthening` | No unexpected current Rust trace accepted | `DivergenceClass`; `RustReplayDivergenceClass` | UC-80, UC-100 | Keep classifier as promotion gate. |
| 48 Rule-based multi-epoch and campaign machines | `model_boundary` / `proof_or_model_strengthening` | No unexpected source behavior found; generated states remain classified | `SemanticCampaignDivergenceClass`; scheduler/frontier classes | UC-76, UC-77 | Keep rule-based state machines as search inputs. |
| 49 Semantic attack campaigns | `model_boundary` / `projection_risk_guarded` / `assumption_counterexample` | No unclassified current Rust exploit confirmed | `SemanticCampaignDivergenceClass` | UC-77 | Keep campaign witnesses classified. |
| 50 Attack-objective search | `model_boundary` / `projection_risk_guarded` / `assumption_counterexample` | No confirmed current source bug; objectives prioritize follow-up fixtures | `ObjectiveGuidedDivergenceClass`; semantic campaign classes | UC-88, UC-100 | Keep objective ranking non-authoritative. |
| 51 Metamorphic frontier checks | `proof_or_model_strengthening` | No source counterexample found in configured bounds | `slash_iter_graph_equiv`; record-normalization theorems | UC-78, UC-91 | Keep metamorphic fixtures. |
| 52 Assumption minimization and Rust corpus emission | `assumption_counterexample` / `proof_or_model_strengthening` | Not a source bug; corpus records violated preconditions and expected classes | `AssumptionDivergenceClass`; `RustReplayDivergenceClass` | UC-79, UC-80, UC-98 | Keep minimized witnesses reproducible. |
| 53 Bundle-based state-machine search | `proof_or_model_strengthening` | Not a source bug; preserves active-edge invariants | `Inv_NeglectEdgesVisibleUnreported` and closure-in-universe obligations | UC-81 | Keep bundle traces classified. |
| 54 Feature-combination coverage | `proof_or_model_strengthening` | No unexpected current Rust divergence found | semantic/frontier divergence classes | UC-82 | Keep combination coverage as search signal. |
| 55 Adversarial scheduler search | `model_boundary` / `projection_risk_guarded` | No source exploit confirmed; partitions/pruning/withholding are classified boundaries | `SchedulerDivergenceClass`; proposer/retention/view classes | UC-83, UC-90 | Keep scheduler witnesses in threat model. |
| 56 Bounded liveness-as-safety | `model_boundary` | Not a safety bug; finite liveness failure requires missing proposer fairness | eager detector/liveness safety invariants; proposer-fairness class | UC-84 | Keep liveness assumption explicit. |
| 57 Arithmetic projection stress | `projection_risk_guarded` | No current source bug confirmed; exact vs fixed-width behavior is guarded | `arithmetic_projection_stress_boundary_8bit`; `ArithmeticProjectionStressClass` | UC-85, UC-97 | Keep checked arithmetic requirement. |
| 58 Assumption weakening | `assumption_counterexample` / `projection_risk_guarded` | Not a source bug; each witness deliberately drops a required precondition | `AssumptionDivergenceClass`; projection classes | UC-86, UC-92 | Keep dropped-precondition corpus. |
| 59 Rust differential corpus expansion | `proof_or_model_strengthening` | No unexpected current Rust divergence confirmed | `RustReplayDivergenceClass` | UC-80, UC-89 | Keep Rust replay fixtures aligned with classes. |
| 60 Persistent Hypothesis corpus mode | `proof_or_model_strengthening` | Not a Rust behavior finding; search infrastructure only | Hypothesis corpus mode; deterministic quick/deep profiles | UC-87 | Keep persistent mode optional. |
| 61 Partition/gossip state machine | `model_boundary` | No current source exploit confirmed; partition/convergence cases remain classified | `PartitionGossipDivergenceClass`; scheduler/view classes | UC-90 | Keep partition/gossip boundary modeled. |
| 62 Objective-guided campaign scoring | `proof_or_model_strengthening` | Not a source bug; ranking prioritizes classified witnesses | `ObjectiveGuidedDivergenceClass` | UC-88, UC-99 | Keep ranking as triage, not proof. |
| 63 Rust-facing metamorphic and replay fixtures | `proof_or_model_strengthening` | No unexpected Rust replay divergence confirmed | `RustReplayDivergenceClass`; graph/record equivalence theorems | UC-89, UC-91 | Keep replay/metamorphic fixtures. |
| 64 Precondition fuzzing | `assumption_counterexample` / `projection_risk_guarded` / `model_boundary` | Not a source bug; every minimized witness is classified | `PreconditionFuzzingClass`; assumption/projection classes | UC-92 | Keep fuzzing as precondition audit. |
| 65 Deep Sage reverse-reachability attack search | `proof_or_model_strengthening` | Not a Rust bug; reinforces closure theorem/examples | `slash_iter_reachability_characterization`; `DagTraceDivergenceClass` | UC-93 | Keep reverse-reachability examples. |
| 66 Deep Sage stake-damage optimization | `assumption_counterexample` | Not a Rust bug under weighted bound | weighted closure theorem family; `DeepThreatModelDivergenceClass` | UC-94 | Keep outside-bound damage witness. |
| 67 Retention/pruning threshold optimization | `projection_risk_guarded` | Current record deletion not reproduced; unsafe pruning remains a projection risk | `TemporalWindowDivergenceClass`; record/retention classes | UC-95, UC-112 | Keep minimum retention requirements documented. |
| 68 Deep epoch/churn identity replay | `confirmed_fixed_bug` / `model_boundary` | Source audit confirmed the stale-evidence same-key rebond class on the slash-deploy path; current Rust fixes it by epoch-scoping slash authorization and checking received slash deploy epochs before replay | `stale_evidence_not_authorized`; `Inv_StaleEvidenceCannotSlashRebondedKey` | UC-96; `slash_authorization_regressions` | Keep epoch identity policy explicit and require target activation epoch on slash deploys. |
| 69 Economic safety envelopes | `projection_risk_guarded` | No current source bug confirmed; runtime must stay in checked safe envelope | `ArithmeticProjectionStressClass`; arithmetic boundary theorems | UC-97 | Keep arithmetic envelope tests. |
| 70 Minimal counterexample catalog | `assumption_counterexample` / `projection_risk_guarded` | Not a source bug; catalog records violated assumptions/projections | `AssumptionDivergenceClass`; projection classes | UC-98 | Keep catalog as review corpus. |
| 71 Threat-vector ranking | `proof_or_model_strengthening` | Not a Rust behavior finding; triage only | threat ranking model; divergence classes for ranked rows | UC-99 | Keep ranking separated from proof authority. |
| 72 Shared Sage scenario schema | `proof_or_model_strengthening` | Not a Rust behavior finding; schema standardizes witness interchange | `scenario_schema.sage`; replay/divergence schemas | UC-80, UC-89 | Keep schema as fixture contract. |
| 73 Production-shaped DAG behavior model | `assumption_counterexample` / `model_boundary` | No unexpected current Rust class found; multi-level DAG reachability needs closure-bound review | `DRDagTraceBoundary`; `DagTraceDivergenceClass` | UC-93, UC-100 | Keep DAG traces classified. |
| 74 Objective-frontier witness ranking | `proof_or_model_strengthening` | Not a source bug; Pareto ranking prioritizes classified rows | `ObjectiveGuidedDivergenceClass`; deep-threat classes | UC-88, UC-99 | Keep ranking non-authoritative. |
| 75 Deep exact attacker/envelope objectives | `assumption_counterexample` / `projection_risk_guarded` / `model_boundary` | No confirmed current Rust exploit; objectives strengthen threat catalog | `DeepThreatModelDivergenceClass`; weighted/retention/arithmetic classes | UC-94, UC-97, UC-98, UC-99 | Keep objectives tied to theorem preconditions. |
| 76 Hypothesis production-shaped DAG traces | `assumption_counterexample` / `model_boundary` | No unexpected Rust behavior accepted; traces are classified before promotion | `DagTraceDivergenceClass`; `RustReplayDivergenceClass` | UC-80, UC-100 | Keep generated DAG traces replayable. |
| 77 Rust replay coverage consumes Sage fixtures | `proof_or_model_strengthening` | No unexpected Rust replay divergence confirmed | `RustReplayDivergenceClass`; Rocq divergence reasons mirrored in Rust | UC-89, UC-100 | Keep fixture schema and Rust mirror aligned. |
| 78 DAG-trace frontier classification | `proof_or_model_strengthening` | No source bug; formal classes prevent treating DAG-trace assumptions as permitted bug fixes | `DRDagTraceBoundary`; `DagTraceDivergenceClass` | UC-100 | Keep DAG trace bucket non-unexpected. |
| 79 Defensive adversarial campaign modeling | `model_boundary` / `projection_risk_guarded` / `assumption_counterexample` | No unclassified current Rust exploit confirmed | `AdversarialCampaignDivergenceClass`; `DifferentialOraclePipelineClass` | UC-100 | Keep campaign witnesses review-required. |
| 80 Production DAG projection gaps | `projection_risk_guarded` / `model_boundary` | Direct-offender-only projections are not production behavior; production-shaped projection must be explicit | `RustViewDetectabilityClass`; DAG/differential classes | UC-89, UC-100 | Keep projection gaps documented. |
| 81 Multi-node local-view disagreement | `model_boundary` | Not a source bug; views may differ before convergence | `ViewDivergenceClass`; `PartitionGossipDivergenceClass` | UC-90, UC-100 | Keep convergence/fairness assumptions explicit. |
| 82 Exact-vs-runtime projection matrix | `projection_risk_guarded` | No confirmed current source bug; projection rows are regression guards | projection, arithmetic, retention, key, and batch classes | UC-72, UC-85, UC-100 | Keep exact semantics separate from unsafe projections. |
| 83 Adversarial vulnerability campaign frontier | `model_boundary` / `projection_risk_guarded` / `assumption_counterexample` | No unclassified current Rust exploit confirmed in configured search | `AdversarialCampaignDivergenceClass`; `DifferentialOraclePipelineClass` | UC-100 | Keep every minimized witness classified. |
| 84 Adversarial campaign classification in Rocq/TLA+/Rust | `proof_or_model_strengthening` | No production semantic change; classification prevents unauthorized bisimilarity deltas | `DRAdversarialCampaignBoundary`; `DRDifferentialOraclePipelineBoundary`; TLA+ campaign classes | UC-100 | Keep classes review-required, not permitted bug fixes. |

## Findings 85-117

| Finding | Classification | Rust source status | Formal artifact | Rust test | Documentation action |
|---------|----------------|--------------------|-----------------|-----------|----------------------|
| 85 Rust latest-message detectability broader than direct citation | `projection_risk_guarded` | Production rule is modeled; direct-citation-only projections are not production behavior | `rust_detectable_view_graph_in`, `same_rust_detectable_view_same_closure`; `Inv_RustViewEdgesDetectableUnreported`, `Inv_SameRustViewSameClosure` | UC-89, UC-100 | Keep direct-citation harnesses labeled as projections. |
| 86 Missing-pointer detector traversal | `confirmed_fixed_bug` | Current Rust skips missing direct/nested pointers and continues deterministically | T-9.11 detector totality; `Inv_DetectorTraversalFiniteFuel` | UC-101, UC-102, UC-104, UC-105, UC-107, property T-9.11 | Closed as permitted bug-fix delta. |
| 87 Duplicate detector child paths | `confirmed_fixed_bug` | Current Rust counts distinct child hashes | T-9.11 distinct-child detector lemmas | UC-106, UC-108, property T-9.11 | Closed as permitted bug-fix delta. |
| 88 Evidence-denial min-cut | `model_boundary` | No source bug confirmed; local views can legitimately differ before evidence availability/convergence | T-12V/T-12PF visibility/fairness boundary; TLA+ view classes | UC-83, UC-90, UC-95, UC-109 | Document as availability, retention, and inclusion boundary. |
| 89 Matrix-oracle closure cross-check | `proof_or_model_strengthening` | Not a Rust bug | `slash_iter_reachability_characterization`; closure invariants | UC-93, UC-109 | Keep as independent model sanity check. |
| 90 Detector-totality threat search | `confirmed_fixed_bug` for pre-fix behavior | Current Rust behavior matches fixed total detector | T-9.11; detector TLA+ invariants | UC-101 through UC-108 | Closed. |
| 91 Composite multi-axis attacks | `model_boundary`/`projection_risk_guarded` | No single current Rust exploit confirmed | TLA+ adversarial and differential-oracle classes | UC-77, UC-88, UC-100 | Keep as composed regression corpus. |
| 92 Candidate closure invariants | `proof_or_model_strengthening` | Not a Rust bug | `slash_iter_initial_graph_monotone`, fixed-point and reachability theorems; TLA+ closure invariants | UC-63, UC-93, UC-109 | Promote only stable invariants. |
| 93 Temporal retention window | `projection_risk_guarded` | No current source bug confirmed; unsafe if retention policy violates `retention_window ≥ gossip_delay + inclusion_delay` | `TemporalWindowDivergenceClass` | UC-95, UC-109 | Document retention lower bound and keep regression witnesses. |
| 94 Mutation oracle | `proof_or_model_strengthening` | Mutants are not current Rust behavior | Boundary and detector theorems already kill modeled mutants | UC-91, UC-100, UC-109 | Use as regression-quality evidence. |
| 95 Rebonded identity replay | `confirmed_fixed_bug` / `model_boundary` | Source audit confirmed that unepoched slash evidence could target a later same-key lifetime; current Rust requires evidence epoch = target activation epoch = current block epoch | `ValidatorLifetime.v`; `BugFixSlashAuthorization.v`; `Inv_StaleEvidenceCannotSlashRebondedKey` | UC-68, UC-76, UC-96; `slash_authorization_regressions` | Document epoch-tagged identity and keep carryover policy explicit. |
| 96 Equivocation-record lifecycle | `not_reproduced_in_rust` / `projection_risk_guarded` | Current Rust has no tracker delete/prune path; detector updates clone the existing record and insert the new detected hash, so previously detected hashes are retained under `access_equivocations_tracker` | `current_rust_record_update_retains_all_detected_hashes`; `RecordLifecycleDivergenceClass`; `Inv_CurrentRustRecordLifecycleRetainsRecords` | UC-72, UC-95, UC-109, UC-112 | Keep early deletion classified as a projection risk; no production source change is justified without a source-level reproduction. |
| 97 Closure-depth extremal | `proof_or_model_strengthening` | Not a Rust bug | `slash_iter_fixed_point_after_universe_bound`; closure-depth TLA+ coverage | UC-63, UC-93, UC-109 | Document worst-case depth `|Validators| - 1`. |
| 98 Evidence addition monotonicity | `proof_or_model_strengthening` | Not a Rust bug | `slash_iter_initial_graph_monotone`; `Inv_InitialEvidenceMonotonicity` | UC-109 | Closed. |
| 99 View-merge confluence | `proof_or_model_strengthening` | Not a Rust bug | `graph_union_closure_overapproximates_*`; `Inv_ViewMergeOverapproximatesInputs`, `Inv_ViewMergeCommutative` | UC-109 | Closed. |
| 100 Minimal accountability basis | `proof_or_model_strengthening` | Not a Rust bug | Reachability characterization | UC-109 | Closed as compact fixture class. |
| 101 Record-key namespace | `projection_risk_guarded` | Current formal design requires canonical pair keys; delimiter-free projection is unsafe | `canonical_key_pair_injective`; `Inv_CanonicalRecordKeyInjective` | UC-75, UC-109 | Closed as projection guard. |
| 102 Detector traversal cycle | `projection_risk_guarded` | Current Rust uses cycle/finite-domain protection in the detector path | `branch_traversal_fixed_after_domain_bound`; `Inv_DetectorTraversalFiniteFuel` | UC-101 through UC-108, UC-109 | Closed. |
| 103 Detector contribution order | `confirmed_fixed_bug` for pre-fix order dependence | Current Rust scans latest messages in deterministic validator order and treats missing pointers as non-contributing | T-9.11 permutation/totality lemmas | UC-102, UC-105, UC-109 | Closed. |
| 104 Closure fixed-point replay | `proof_or_model_strengthening` | Not a Rust bug | `slash_iter_fixed_point_stable`; TLA+ closure-stability invariants | UC-63, UC-109 | Closed. |
| 105 Report-retention reactivation | `projection_risk_guarded` | No current source bug confirmed; unsafe only if reports are pruned before visible evidence | `reported_edge_not_active`; `Inv_ReportsSuppressNeglectEdges` | UC-67, UC-72, UC-109 | Document report retention policy. |
| 106 No-seed cycle safety | `proof_or_model_strengthening` | Not a Rust bug | `slash_iter_empty_initial_empty`; `Inv_NoDirectSeedNoClosure` | UC-60, UC-109 | Closed. |
| 107 Slash-history prefix exactness | `proof_or_model_strengthening` | Not a Rust bug | Reachability characterization; `Inv_SlashedEqualsClosurePrefix` | UC-63, UC-109 | Closed. |
| 108 Edge orientation | `projection_risk_guarded` | Current model uses `neglecter → offender`; reversed adapters are unsafe | Reachability characterization | UC-109 | Closed as adapter guard. |
| 109 Redundant path denial cost | `model_boundary` | Not a Rust bug; describes availability robustness | Reachability characterization | UC-109 | Closed as threat-model fixture. |
| 110 Slash targets are not self-authorizing | `projection_risk_guarded` | No current source bug confirmed; slash targets are reports/acknowledgements, not direct seeds | `slash_iter_empty_initial_empty`; `Inv_NoDirectSeedNoClosure` | UC-109 | Closed as authorization guard. |
| 111 Reports are pair-scoped | `proof_or_model_strengthening` | Not a Rust bug | `unreported_visible_edge_remains_active`; `Inv_UnreportedVisibleEdgesRemainActive` | UC-109 | Closed. |
| 112 Report growth is closure-antitone | `proof_or_model_strengthening` | Not a Rust bug | `view_closure_reports_antimonotone`; `Inv_ReportGrowthCannotExpandViewClosure` | UC-109 | Closed. |
| 113 Reports do not suppress direct evidence | `proof_or_model_strengthening` | Not a Rust bug | `slash_iter_monotone`; `Inv_ReportsDoNotSuppressDirectEvidence` | UC-109 | Closed. |
| 114 Validator-renaming equivariance | `proof_or_model_strengthening` | Not a Rust bug | `slash_iter_validator_renaming_equiv`; `Inv_ValidatorRenamingEquivariance`; Rust property fixture | UC-91, UC-109 | Closed formally and by tests. |
| 115 Bisimilarity delta guard | `proof_or_model_strengthening` | No unexpected Rust divergence confirmed | `DivergenceClass`, `divergence_allowed`; `Inv_NoUnexpectedDifferentialDivergence` | UC-80, UC-89, UC-100, UC-109 | Closed as classification policy. |
| 116 Horizon campaigns | `model_boundary`/`projection_risk_guarded`/`assumption_counterexample` | No new Rust source bug confirmed; the stable cases are retained-policy, fairness, detector-gate, epoch-identity, weighted-bound, merge, arithmetic, and metamorphic guards | `DRHorizonCampaignBoundary`; `HorizonCampaignDivergenceClass`; existing retention, fairness, detector, reachability, report, arithmetic, and divergence families | UC-110 | Keep as cross-coupled regression suite and promote only theorem-shaped combined preconditions. |
| 117 Horizon-v2 Rust-aligned lifecycle frontier | `model_boundary`/`projection_risk_guarded`/`assumption_counterexample` | No new Rust source bug confirmed; the stable cases refine detector DAG projection, detected-hash retention, finality-aware retention, weighted damage plus evidence-denial cost, epoch identity, and exact-vs-projection classification | `DRHorizonV2Boundary`; `HorizonV2DivergenceClass`; existing detector, record-lifecycle, temporal-retention, weighted-bound, reachability, epoch-identity, and divergence families | UC-111 | Keep as Rust-shaped frontier regression suite; require a source-level reproduction before any production semantic change. |

## Findings 118-122

These rows are source-confirmed production-path issues promoted from the
epoch, authorization, liveness, arithmetic, and projection frontiers.

| Finding | Classification | Rust source status | Formal artifact | Rust test | Documentation action |
|---------|----------------|--------------------|-----------------|-----------|----------------------|
| 118 Same-key rebond stale-evidence slash | `confirmed_fixed_bug` | Current Rust includes `target_activation_epoch` on slash deploys and authorizes only evidence whose epoch matches the current block epoch | `ValidatorLifetime.v`; `BugFixSlashAuthorization.stale_evidence_not_authorized_candidate`; `Inv_StaleEvidenceCannotSlashRebondedKey` | `stale_invalid_evidence_is_not_an_authorized_slash_candidate`; `received_stale_slash_deploy_is_rejected_before_replay` | Specify validator lifetime identity as `(validator, activation epoch)`. |
| 119 Received slash deploy authorization | `confirmed_fixed_bug` | Current Rust rejects forged, stale, unknown, valid-block, unbonded-target, and duplicate-target slash deploys before replay | `BugFixSlashAuthorization.unknown_evidence_not_authorized`; `SlashDeploy.execute_unknown_evidence_noop`; `Inv_OnlyAuthorizedSlashCanBePending` | `received_stale_slash_deploy_is_rejected_before_replay`; `unauthorized_slash_status_is_slashable` | Treat received slash deploys as authorization-bearing protocol messages, not merely replay inputs. |
| 120 Invalid-latest slash liveness gap | `confirmed_fixed_bug` | Current Rust derives candidates from current-epoch invalid-block metadata rather than only `invalid_latest_messages`, deduping each offender to the canonical minimum hash | `BlockCreator.deploy_epoch_matches_target`; `Inv_NoInvalidLatestLivenessGap` | `current_epoch_invalid_evidence_is_authorized_once_per_offender` | Specify the invalid-block evidence index and minimum-hash rule as the canonical candidate source. |
| 121 Sequence arithmetic boundary | `confirmed_fixed_bug` | Current Rust uses checked predecessor/successor arithmetic for record base sequence and proposer next sequence; nonpositive predecessor domains are rejected | `BugFixSeqArithmetic.v`; exact arithmetic boundary theorems | `checked_sequence_arithmetic_rejects_boundaries`; property checks in `slash_authorization_regressions` | Require checked arithmetic at every fixed-width projection boundary. |
| 122 Duplicate justification projection | `confirmed_fixed_bug` | Current Rust rejects duplicate validator justifications instead of letting map projection hide evidence | `BugFixDuplicateJustifications.duplicate_head_rejected`; `accepted_implies_head_not_in_tail`; `Inv_DuplicateJustificationsRejected` | `duplicate_justification_validators_are_invalid` | Specify justifications as validator-unique lists before map projection. |

## Current Rust-Source Conclusion

The confirmed production-relevant bugs in the latest finding set are now
represented as permitted bug-fix deltas or specification-enforcing
hardening:

- missing-pointer traversal could abort or depend on iteration order;
- duplicate paths to the same offender child could be over-counted;
- off-by-one seq-number density could miss the canonical visible child.
- stale invalid-block evidence could target a later same-key validator
  lifetime;
- received slash deploys lacked a complete pre-replay authorization gate;
- slash liveness depended on `invalid_latest_messages` instead of the
  invalid-block evidence index;
- fixed-width sequence arithmetic had unchecked predecessor/successor
  boundaries;
- duplicate justifications could be collapsed by projection before the
  validity check.

The current Rust detector uses deterministic latest-message ordering,
non-fatal missing-pointer handling, distinct child hashes, canonical
self-chain child selection, pass-local memoization, and cycle protection.
The current Rust proposer path implements the Bug #8/T-9.8 unbonded-proposer
guard before generating slash deploys, derives slash candidates from
authorized current-epoch invalid-block evidence, carries
`target_activation_epoch`, and validates received slash deploys before
replay. The record-lifecycle audit confirms that early
equivocation-record deletion is not a current Rust path: the tracker exposes
no delete/prune API, the detector update preserves existing detected hashes,
and record insertion is guarded by an existence check under the tracker
lock. Rows 118-122 required source changes and integration/property tests;
the remaining non-promoted rows require documentation, formalization, and
regression coverage only.
