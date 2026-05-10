# Slashing Threat Model

This document records the defensive threat model used by the Sage,
Rocq, TLA+, and Rust slashing artifacts. It complements the
specification, verification, and design documents.

## 1. Terms

An **offender** is a validator with direct slashable evidence, such as
two distinct blocks at the same sequence number. A **neglecter** is a
validator whose latest-message view detects an existing offender record
but whose block does not acknowledge that offender. A **view** is the set
of latest-message justifications visible to a block. A **report** is a
slash target or record acknowledgement that removes a neglect edge from
the active evidence graph.

The two-level closure is reverse reachability from direct offenders:

```
Closure₀ = DirectOffenders
Closureᵢ₊₁ = Closureᵢ ∪ { v ∈ Validators | NeglectEdges(v) ∩ Closureᵢ ≠ ∅ }
```

The fixed Rust detector uses:

```
detectable(view) ≜ detected_hash_seen(view) ∨ |distinct_child_hashes(view)| ≥ 2
```

Missing pointers contribute `∅`. Duplicate paths to the same child are
canonicalized before cardinality is checked.

## 2. Adversary Model

The adversary may:

- equivocate by signing conflicting blocks;
- choose justification shapes, including incomplete or stale pointers;
- withhold or delay evidence before gossip convergence;
- attempt duplicate-edge, cyclic-graph, or duplicate-child amplification;
- exploit validator-set churn, stale evidence, or rebonded identities;
- exploit arithmetic boundaries in fixed-width projections;
- race equivocation-record insertion;
- replay old slash deploys;
- attempt proposer-schedule or evidence-inclusion suppression.

The adversary may not:

- forge validator signatures or system-auth tokens;
- break cryptographic hash collision resistance;
- mutate on-chain PoS state outside protocol transitions;
- violate the explicit theorem preconditions without being classified as
  a boundary or assumption counterexample.

## 3. Threat Coverage Matrix

The traceability status for each finding is maintained in
[`slashing-traceability.md`](./slashing-traceability.md). A Sage or
Hypothesis witness is not treated as a Rust vulnerability unless that
ledger marks it as production-source confirmed. Boundaries and projection
risks receive formal classifications and regression tests; they do not
authorize Rust source changes by themselves.

| Threat class                                                   | Protection                                                                                                                                                                                                                                                            | Formal artifact                                                                                                                                             | Rust tests                                                  |
|----------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------|
| Direct equivocation                                            | Sound/complete detector; record insertion                                                                                                                                                                                                                             | T-1, T-2, T-3                                                                                                                                               | UC-01–UC-04, property T-1/T-2                               |
| Ignorable equivocation flooding                                | Ignorable is slashable                                                                                                                                                                                                                                                | T-9.1                                                                                                                                                       | UC-03, UC-41                                                |
| Tracker race                                                   | Atomic read-modify-write                                                                                                                                                                                                                                              | T-9.2                                                                                                                                                       | UC-12, loom T-9.2                                           |
| Slashable invalid block not recorded                           | Dispatcher catch-all inserts records                                                                                                                                                                                                                                  | T-9.3                                                                                                                                                       | UC-27–UC-36, UC-42                                          |
| Transfer failure during slash/withdraw                         | Failure path preserves state for retry                                                                                                                                                                                                                                | T-9.4, T-9.10                                                                                                                                               | UC-13, UC-40, T-9.10 property                               |
| Stake-zero bonded state                                        | Positive-bond invariant                                                                                                                                                                                                                                               | T-9.5                                                                                                                                                       | UC-11, UC-19, UC-56                                         |
| Self-regression/LMD inconsistency                              | Self-regression check and Rust self-correction rule                                                                                                                                                                                                                   | T-9.6, T-9.9                                                                                                                                                | UC-06, UC-23, UC-37                                         |
| Sequence-number gap and same-branch overcount                  | Canonical visible self-chain child above `baseSeq`; pass-local memoization is a refinement, not evidence                                                                                                                                                              | T-9.7                                                                                                                                                       | UC-20, UC-43                                                |
| Unbonded proposer slash emission                               | Proposer-bond guard                                                                                                                                                                                                                                                   | T-9.8                                                                                                                                                       | UC-22                                                       |
| Missing detector pointer                                       | Missing pointer contributes `∅`                                                                                                                                                                                                                                       | T-9.11                                                                                                                                                      | UC-101, UC-102, UC-104, UC-107                              |
| Duplicate detector child path                                  | Distinct child hashes are counted                                                                                                                                                                                                                                     | T-9.11                                                                                                                                                      | UC-106, UC-108                                              |
| Detected-hash ordering                                         | Detected hash is decisive                                                                                                                                                                                                                                             | T-9.11                                                                                                                                                      | UC-105                                                      |
| Complete-view bisimilarity                                     | Fixed detector preserves complete-pointer verdicts                                                                                                                                                                                                                    | T-9.11, T-15                                                                                                                                                | UC-103, property T-9.11                                     |
| Weighted closure amplification                                 | Explicit closure-bound precondition                                                                                                                                                                                                                                   | T-12W                                                                                                                                                       | UC-55, UC-70, UC-94                                         |
| Evidence visibility and withholding                            | View-indexed closure; proposer-fairness boundary                                                                                                                                                                                                                      | T-12V, T-12PF                                                                                                                                               | UC-58, UC-74, UC-83, UC-84                                  |
| Evidence-denial min-cut                                        | Gossip/retention/inclusion assumptions keep every decisive path available or classify the local view as a boundary                                                                                                                                                    | Sage Finding 88; T-12V/T-12PF; TLA+ view/fairness classes                                                                                                   | UC-83, UC-90, UC-95, UC-109                                 |
| Temporal evidence expiry                                       | Retention window must cover gossip plus inclusion delay                                                                                                                                                                                                               | Sage Finding 93; `TemporalWindowDivergenceClass`                                                                                                            | UC-95, UC-109                                               |
| Cross-coupled horizon campaign                                 | Retention, gossip, proposer inclusion, epoch identity, detector contribution gates, weighted closure bounds, view merge, checked arithmetic, and metamorphic closure checks are modeled together so an unsafe projection on one axis cannot be hidden by another axis | Sage Finding 116; `DRHorizonCampaignBoundary`; `HorizonCampaignDivergenceClass`                                                                             | UC-110                                                      |
| Rust-aligned horizon-v2 campaign                               | Detector DAG projection, multi-record lifecycle, finality-aware retention, weighted damage plus evidence-denial cost, and era identity boundaries are modeled together against the Rust-shaped contribution rule                                                      | Sage Finding 117; `DRHorizonV2Boundary`; `HorizonV2DivergenceClass`                                                                                         | UC-111                                                      |
| Epoch/churn identity confusion                                 | Current-epoch/current-validator authorization filters every slash deploy before replay                                                                                                                                                                                | T-12EID; `Inv_StaleEvidenceCannotSlashRebondedKey`                                                                                                          | UC-57, UC-64, UC-68, UC-96, slash authorization regressions |
| Rebonded identity replay                                       | Epoch-scoped slash authorization prevents stale evidence from targeting a later same-key lifetime                                                                                                                                                                     | `stale_evidence_not_authorized`; `main_T9_12_stale_evidence_not_authorized`; `Inv_StaleEvidenceCannotSlashRebondedKey`                                      | slash authorization regressions                             |
| Unauthorized received slash deploy                             | Block validation rejects issuer mismatch, unknown invalid hash, stale epoch, unbonded target, and duplicate target before replay                                                                                                                                      | `execute_unknown_evidence_noop`; `main_T9_13_unknown_slash_evidence_noop`; `Inv_RejectedSlashWithoutEvidenceNoPending`                                      | slash authorization regressions                             |
| Spoofed system auth token                                      | Invalid-auth slash deploys are rejected before any PoS state mutation; valid-auth execution is equivalent to the ordinary slash deploy semantics                                                                                                                      | `execute_invalid_auth_token_noop`; `main_TAuth_invalid_token_noop`; `execute_valid_auth_token_equiv`; `Inv_InvalidAuthSlashNoPending`                       | UC-21, `prop_t_auth_check`                                  |
| Invalid-latest slash liveness gap and candidate nondeterminism | Proposers derive slash candidates from the authorized invalid-block evidence index and canonicalize multiple same-epoch invalid hashes for one offender by minimum hash                                                                                               | `deploy_epoch_matches_target`; `Inv_NoInvalidLatestLivenessGap`                                                                                             | slash authorization regressions                             |
| Duplicate justification projection                             | Duplicate validator justifications are invalid before detector projection                                                                                                                                                                                             | `duplicate_head_rejected`; `main_T9_15_duplicate_justifications_rejected`; `Inv_DuplicateJustificationsRejected`                                            | slash authorization regressions                             |
| Duplicate/cyclic graph amplification                           | Set semantics and reachability certificates                                                                                                                                                                                                                           | T-12 graph equivalence                                                                                                                                      | UC-59, UC-60, UC-78, UC-91, UC-93                           |
| Closure-depth latency                                          | Closure reaches at most `|Validators| - 1` rounds in a finite validator set                                                                                                                                                                                           | Sage Finding 97; `slash_iter_fixed_point_after_universe_bound`; `Inv_SlashedEqualsClosurePrefix`                                                            | UC-63, UC-93, UC-109                                        |
| Closure oracle drift                                           | Iterative graph closure is cross-checked against matrix transitive closure                                                                                                                                                                                            | Sage Finding 89; `slash_iter_reachability_characterization`                                                                                                 | UC-93, UC-109                                               |
| Evidence-addition monotonicity                                 | In a fixed validator universe, adding direct evidence or neglect edges cannot shrink closure: `S₀ ⊆ S₁ ∧ G₀ ⊆ G₁ ⇒ closure(G₀,S₀) ⊆ closure(G₁,S₁)`                                                                                                                   | Sage Finding 98; `slash_iter_initial_graph_monotone`; `Inv_InitialEvidenceMonotonicity`                                                                     | monotonic evidence fixtures                                 |
| View-merge confluence                                          | Merged local evidence views over-approximate each input view and are merge-order independent                                                                                                                                                                          | Sage Finding 99; `graph_union_closure_overapproximates_*`; `Inv_ViewMergeOverapproximatesInputs`; `Inv_ViewMergeCommutative`                                | view-merge fixtures                                         |
| Minimal accountability basis                                   | Minimal evidence bases distinguish necessary reachability edges from redundant evidence                                                                                                                                                                               | Sage Finding 100; reachability characterization                                                                                                             | UC-109                                                      |
| Detector traversal cycle                                       | Creator-justification traversal must use finite-domain fuel or a visited set                                                                                                                                                                                          | Sage Finding 102; `branch_traversal_fixed_after_domain_bound`; `Inv_DetectorTraversalFiniteFuel`                                                            | traversal-cycle fixtures                                    |
| Detector contribution order                                    | Missing pointers, duplicate children, distinct children, and detected hashes are order-independent under fixed detector semantics                                                                                                                                     | Sage Finding 103; T-9.11 detector lemmas                                                                                                                    | detector permutation fixtures                               |
| Closure replay idempotence                                     | Replaying closure from its fixed point cannot change the result                                                                                                                                                                                                       | Sage Finding 104; fixed-point stability                                                                                                                     | closure replay fixtures                                     |
| No-seed cycle safety                                           | Cyclic neglect evidence cannot create slashability without a direct equivocator or retained slash record                                                                                                                                                              | Sage Finding 106; `slash_iter_empty_initial_empty`; `Inv_NoDirectSeedNoClosure`                                                                             | no-seed cycle fixtures                                      |
| Slash-history prefix exactness                                 | Operational slashing state equals the mathematical closure prefix at every level                                                                                                                                                                                      | Sage Finding 107; reachability characterization; `Inv_SlashedEqualsClosurePrefix`                                                                           | slash-history prefix fixtures                               |
| Edge-orientation mutation                                      | Neglect edges are `neglecter → offender`; reversing them changes accountability                                                                                                                                                                                       | Sage Finding 108; reachability characterization                                                                                                             | edge-orientation fixtures                                   |
| Redundant path denial cost                                     | Independent evidence paths require multi-edge denial before a target drops from closure                                                                                                                                                                               | Sage Finding 109; reachability characterization                                                                                                             | redundant-path fixtures                                     |
| Unsupported slash-target injection                             | Slash-target lists acknowledge/report evidence; they are not direct slash seeds                                                                                                                                                                                       | Sage Finding 110; `slash_iter_empty_initial_empty`; `Inv_NoDirectSeedNoClosure`                                                                             | unauthorized-target fixtures                                |
| Report namespace confusion                                     | Reports suppress only the exact reporter/offender pair, not all edges from a reporter                                                                                                                                                                                 | Sage Finding 111; `unreported_visible_edge_remains_active`; `Inv_UnreportedVisibleEdgesRemainActive`                                                        | report-namespace fixtures                                   |
| Report-growth expansion bug                                    | Adding reports cannot create slashability in a fixed visible view                                                                                                                                                                                                     | Sage Finding 112; `view_closure_reports_antimonotone`; `Inv_ReportGrowthCannotExpandViewClosure`                                                            | report-antitone property                                    |
| Direct evidence suppressed by report                           | Reports remove neglect edges but never remove direct-equivocation seeds                                                                                                                                                                                               | Sage Finding 113; `slash_iter_monotone`; `Inv_ReportsDoNotSuppressDirectEvidence`                                                                           | direct-seed property                                        |
| Validator-renaming/order dependence                            | Closure must be equivariant under bijective validator renaming                                                                                                                                                                                                        | Sage Finding 114; `slash_iter_validator_renaming_equiv`; `Inv_ValidatorRenamingEquivariance`; Rust metamorphic fixture                                      | UC-91, UC-109                                               |
| Unclassified bisimilarity delta                                | Representation deltas remain bisimilar; semantic deltas are documented bug fixes or projection risks                                                                                                                                                                  | Sage Finding 115; divergence classifications                                                                                                                | UC-80, UC-89, UC-100, UC-109                                |
| Bounded arithmetic projection                                  | Checked arithmetic/safe envelope for `seq − 1` and proposer `seq + 1`                                                                                                                                                                                                 | T-8, arithmetic boundary, `checked_pred_total_positive`, `checked_succ_bounded_sound`                                                                       | UC-61, UC-85, UC-97, slash authorization regressions        |
| Record-key collision                                           | Canonical pair key, not delimiter-free concatenation                                                                                                                                                                                                                  | Sage Finding 101; T-5DF; `Inv_CanonicalRecordKeyInjective`                                                                                                  | UC-65, UC-75                                                |
| Record lifecycle deletion                                      | Current Rust does not delete/prune tracker records; unsafe early deletion remains a projection risk, while detector updates must retain all prior detected hashes                                                                                                     | Sage Finding 96; `current_rust_record_update_retains_all_detected_hashes`; `RecordLifecycleDivergenceClass`; `Inv_CurrentRustRecordLifecycleRetainsRecords` | UC-72, UC-95, UC-109, UC-112                                |
| Report-retention reactivation                                  | Reports that suppress still-visible evidence must not be pruned before the evidence ages out                                                                                                                                                                          | Sage Finding 105; `reported_edge_not_active`; `Inv_ReportsSuppressNeglectEdges`                                                                             | report-retention fixtures                                   |
| Partial batch slash failure                                    | Atomic or rollback policy                                                                                                                                                                                                                                             | T-IdemMany boundary                                                                                                                                         | UC-71                                                       |
| Composite multi-axis attack                                    | Stake damage, view split, retention loss, and arithmetic overflow are classified together                                                                                                                                                                             | Sage Finding 91; `AdversarialCampaignDivergenceClass`; `DifferentialOraclePipelineClass`                                                                    | UC-77, UC-88, UC-100                                        |
| Unsafe semantic mutants                                        | Known unsafe mutants are killed by frontier witnesses                                                                                                                                                                                                                 | Sage Finding 94; detector/boundary theorem families                                                                                                         | UC-91, UC-100, UC-109                                       |
| Cross-implementation drift                                     | Bisimulation and divergence classification                                                                                                                                                                                                                            | T-13–T-15                                                                                                                                                   | UC-39, UC-80, UC-89, UC-100                                 |

## 4. Classification Policy

Every generated Sage/Hypothesis witness must be classified before it can
be promoted:

| Class                       | Meaning                                               | Action                    |
|-----------------------------|-------------------------------------------------------|---------------------------|
| `bisimilar`                 | Rust, formal model, and Scala/projection agree        | regression test           |
| `permitted_bug_fix`         | new behavior intentionally fixes a proven old bug     | theorem + test + docs     |
| `candidate_boundary`        | behavior depends on an explicit model boundary        | document precondition     |
| `projection_risk`           | exact model and implementation projection can diverge | add regression guard      |
| `assumption_counterexample` | theorem precondition is necessary                     | keep assumption explicit  |
| `unexpected`                | unclassified disagreement                             | CI failure; do not accept |

Bug #11 is `permitted_bug_fix`: complete latest-message views stay
bisimilar, but the old missing-pointer abort and duplicate-child
false-positive behaviors are rejected as bugs.

## 5. Exploratory Frontier Extensions

The Sage/Hypothesis frontier now includes these additional defensive
search axes:

- **Evidence-denial min-cut search:** computes the smallest hidden
  visible-unreported edge set that shrinks accountability closure.
- **Independent closure-oracle search:** compares iterative graph closure
  with adjacency-matrix transitive closure.
- **Detector-totality DAG search:** shrinks missing-pointer and
  duplicate-child witnesses for Bug #11.
- **Composite attack search:** combines stake amplification, partitioned
  views, retention projection, and bounded arithmetic projection in one
  classified scenario.
- **Candidate invariant mining:** searches for counterexamples to
  direct-subset, monotonicity, idempotence, duplicate-edge, and
  matrix-oracle closure properties.
- **Temporal-window synthesis:** derives evidence-retention bounds from
  gossip and inclusion delay.
- **Mutation-oracle detection:** checks that witnesses kill unsafe
  semantic mutants.
- **Rebond identity lifecycle search:** separates epoch-tagged identity
  from loose public-key projection.
- **Record-lifecycle state machines:** check monotone records and
  detected-hash retention.
- **Closure-depth extremal search:** records worst-case closure latency.
- **Evidence-addition monotonicity:** searches for closure shrinkage after
  direct evidence or active edges are added in the same universe.
- **View-merge confluence:** checks that merged local views
  over-approximate each input and are order-independent.
- **Minimal slash-basis extraction:** finds compact reachability witnesses
  for target slashes.
- **Record-key namespace projection:** rechecks delimiter-free key
  projections against canonical pair encodings.
- **Detector traversal termination:** searches cyclic traversal graphs
  and requires finite-domain fuel or visited-set semantics.
- **Detector contribution confluence:** permutes missing pointers,
  duplicate child hashes, distinct child hashes, and detected hashes.
- **Closure fixed-point idempotence:** checks replay from a fixed closure.
- **Report-retention reactivation:** searches for early report pruning
  that can reactivate already acknowledged evidence edges.
- **Horizon campaign search:** composes retention windows, gossip delay,
  proposer inclusion, epoch identity, detector contribution gates,
  weighted damage, partition merge, checked arithmetic, and closure
  metamorphism in one classified campaign frontier.
- **Horizon-v2 Rust-aligned search:** keeps detector DAG contribution
  semantics, record lifecycle, finality-aware retention, weighted
  objectives, evidence-denial cost, and epoch/era identity in one
  classified campaign frontier.
- **Coverage-guided Rust fuzzing:** uses `cargo-fuzz` and structure-aware
  fuzz targets to drive sequence arithmetic, epoch projection, slash
  deploy serialization, and block-message normalization through Rust
  coverage feedback.
- **Symbolic Rust helper verification:** uses Kani proof harnesses for
  bounded Rust helper properties before they are relied on by production
  authorization and validation code.
- **Symbolic TLA+ expansion:** uses Apalache as a bounded symbolic
  complement to TLC when explicit-state enumeration is too expensive for
  larger validator, epoch, and churn domains.
- **System adversarial testing:** targets production-shaped multi-node
  schedules with partitions, delayed gossip, proposer withholding, node
  restarts, stale evidence, and validator churn.

These axes are not proof authority. They generate witnesses and theorem
candidates for Rocq/TLA+ promotion.

## 6. Residual Boundaries

The current formalization intentionally abstracts network Sybil
resistance, validator key management, cryptographic signature validity,
and full economic finality. Those boundaries are documented in the
specification scope table. A witness that depends on one of them must be
classified as a boundary or assumption counterexample, not as a slashing
protocol exploit.
