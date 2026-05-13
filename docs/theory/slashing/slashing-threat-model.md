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
  a boundary or assumption counterexample;
- rationally defect beyond protocol incentives (the *cost* of the
  capabilities above is modeled at game-theoretic scope only — see
  **§5.A Economic and game-theoretic threats** below).

## 2.1 STRIDE classification of consensus-layer threats

STRIDE [Howard&LeBlanc] partitions threats into six categories:
**S**poofing, **T**ampering, **R**epudiation, **I**nformation
disclosure, **D**enial of service, **E**levation of privilege. The
slashing protocol's defensive surface maps as follows:

| Threat (from §3 coverage matrix)             | STRIDE bucket(s) | Why                                                                                            | Defense (formal artifact)                                         |
|----------------------------------------------|------------------|------------------------------------------------------------------------------------------------|-------------------------------------------------------------------|
| Direct equivocation                          | **T**            | A validator tampers with consensus state by signing two distinct blocks at one sequence number | Detector soundness/completeness (T-1, T-2, T-3)                   |
| Ignorable equivocation flooding              | **T** + **D**    | Tampering primitive used as a DoS amplifier when accumulated                                   | Ignorable promoted to slashable (T-9.1)                           |
| Tracker race (lost RMW)                      | **T** + **R**    | Atomicity violation enables repudiation of one of two concurrently inserted records            | Atomic RMW (T-9.2)                                                |
| Slashable invalid block not recorded         | **R**            | Adversary repudiates evidence by causing the dispatcher to drop it                             | Dispatcher catch-all (T-9.3)                                      |
| Transfer-failure rollback                    | **D** + **R**    | Failure path could otherwise leak funds or deadlock the slash transition                       | Retry-preserving fail-stop (T-9.4, T-9.10)                        |
| Stake-zero bonded state                      | **T**            | Tampering with the active-set invariant                                                        | Positive-bond invariant (T-9.5)                                   |
| Self-regression / LMD inconsistency          | **T**            | Sender tampers with their own latest-message pointers                                          | Self-regression check, Rust self-correction (T-9.6, T-9.9)        |
| Sequence-number density attack               | **T**            | Tampering by skipping seq numbers to evade canonical-self-chain walk                           | Canonical visible self-chain child rule (T-9.7)                   |
| Unbonded proposer slash emission             | **E**            | Elevation: unbonded principal performs slashing                                                | Proposer-bond guard (T-9.8)                                       |
| Detector partial / missing-pointer abort     | **R** + **T**    | Adversary repudiates evidence by shaping a view that aborts detection or fakes two children    | Total iterative detector (T-9.11)                                 |
| Unauthorized slash deploy                    | **E** + **S**    | Block author spoofs authority to slash; effectively elevation of privilege                     | Pre-replay `SlashAuthorizedByEvidence` filter (T-9.13)            |
| Stale-evidence rebond                        | **S** + **R**    | Evidence from a prior validator lifetime is reused to slash a new same-key lifetime            | Epoch-scoped slash authorization (T-9.12)                         |
| Spoofed system-auth token                    | **E** + **S**    | Forged authority token used to bypass the auth boundary                                        | Auth-token oracle, valid/invalid token theorems (T-Auth)          |
| Slash liveness gap                           | **D**            | Denial: detected offender never slashed because candidate index doesn't contain the evidence   | Authorized invalid-block evidence index (T-LivenessGap)           |
| Sequence-arithmetic panic / wrap             | **D**            | Boundary arithmetic crashes the proposer or corrupts record keys                               | Checked arithmetic + nonpositive rejection (T-9.14)               |
| Duplicate-justification projection ambiguity | **R** + **T**    | Adversary tampers with detector projection ordering                                            | Pre-projection validation rejects duplicates (T-9.15)             |
| Evidence-denial min-cut                      | **D**            | Adversary denies decisive evidence paths to a target view                                      | View-merge over-approximation (T-12V); proposer fairness (T-12PF) |
| Closure-depth latency                        | **D**            | Adversary tries to delay closure beyond practical bound                                        | `|V|−1` rounds bound (T-12C)                                      |
| Validator-renaming order dependence          | **T**            | Adversary tries to make outcomes depend on the validator-name labeling                         | Bijective renaming equivariance (T-12 renaming)                   |

The STRIDE columns are **inclusive**, not partitioning — many
threats span multiple buckets. The "Defense" column links to the
formal artifact in `formal/rocq/slashing/` (Rocq) or
`formal/tlaplus/slashing/` (TLA+) that discharges the threat. For
the complete coverage matrix with Rust-test pointers see §3.

> *Citation note.* STRIDE was introduced by M. Howard and D. LeBlanc,
> *Writing Secure Code* (2nd ed., Microsoft Press, 2003, ISBN
> 0-7356-1722-8), where it is presented as a structured method for
> enumerating attack surfaces.

## 2.2 Attack tree — paths to a successful slashing-safety violation

Following Schneier's *Attack Trees* methodology [Schneier1999], we
root the goal at "**Violate slashing safety**" — i.e., either
(a) cause an honest validator's bond to be slashed without their
having committed a slashable offence, or (b) prevent a detected
offender from being slashed despite the offence being attributable.
The four internal nodes ("Avoid detection", "Avoid record
insertion", "Avoid effect", "Avoid fork-choice exclusion") match the
five-layer architecture from §03 of the design suite. Leaves are
labeled with the relevant headline theorem; a path from root to a
leaf is *blocked* iff the leaf's theorem holds.

```
Goal: Violate slashing safety
│
├─ A. Cause unjust slashing of an honest validator
│  │
│  ├─ A1. Forge slash deploy with adversarial authority              [BLOCKED by T-Auth]
│  ├─ A2. Submit slash referencing fake evidence                     [BLOCKED by T-9.13]
│  ├─ A3. Replay stale evidence against a rebonded same-key lifetime [BLOCKED by T-9.12]
│  ├─ A4. Cause projection ambiguity via duplicate justifications    [BLOCKED by T-9.15]
│  └─ A5. Cause sequence-arithmetic panic / wrap to corrupt key      [BLOCKED by T-9.14]
│
└─ B. Prevent slashing of a detected offender
   │
   ├─ B1. Avoid detection
   │  │
   │  ├─ B1.1 Ignorable equivocation flooding                        [BLOCKED by T-9.1]
   │  ├─ B1.2 Tracker race lose evidence                             [BLOCKED by T-9.2]
   │  ├─ B1.3 Missing-pointer detector abort                         [BLOCKED by T-9.11]
   │  ├─ B1.4 Duplicate-child false positive (becomes false neg)     [BLOCKED by T-9.11]
   │  └─ B1.5 Self-regression slips through                          [BLOCKED by T-9.6]
   │
   ├─ B2. Avoid record insertion
   │  │
   │  ├─ B2.1 Dispatcher stub drops slashable variants               [BLOCKED by T-9.3]
   │  └─ B2.2 Record-key collision (delimiter-free)                  [BLOCKED by T-5DF]
   │
   ├─ B3. Avoid slash effect
   │  │
   │  ├─ B3.1 PoS transfer-failure FIXME leaves SlashPending         [BLOCKED by T-9.4]
   │  ├─ B3.2 Withdrawal transfer-failure variant                    [BLOCKED by T-9.10]
   │  ├─ B3.3 Stake-0 silent classification                          [BLOCKED by T-9.5]
   │  ├─ B3.4 Unbonded proposer cannot emit slash deploy             [BLOCKED by T-9.8]
   │  └─ B3.5 Liveness gap from invalid-latest-message dependency    [BLOCKED by T-LivenessGap]
   │
   └─ B4. Avoid fork-choice exclusion
      │
      ├─ B4.1 Slashed validator still votes in GHOST tally           [BLOCKED by T-10]
      └─ B4.2 Sub-quorum coalition slashes ≥ F+1 honest validators   [BLOCKED by T-12 BFT bound]
```

**Reading the tree.** A *successful* attack would require an adversary
to traverse from root to a leaf without all branches on that path being
blocked. Because every leaf above is blocked by an explicit theorem,
*and* the OR-tree shape means a single unblocked leaf suffices, the
slashing protocol's safety property holds iff every theorem above
holds simultaneously — which is exactly what T-15 (Rust ↔ Scala
bisimilarity, modulo the documented bug-fix deltas) summarizes.

The economic threats in §5.A live at a *higher* layer: they assume
all of the above leaves remain blocked, and ask whether the
*incentive* alignment still pays. If economic incentive fails, the
adversary need not breach any of the leaves above; they instead
choose not to play honestly.

> *Citation note.* Attack-tree methodology: B. Schneier, *Attack
> Trees: Modeling security threats*, **Dr. Dobb's Journal**, Dec.
> 1999. The original method generalizes from *AND/OR* trees over
> goal states.

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

### 3.1 Docker / wire-protocol coverage

The "Rust tests" column above lists in-tree (in-process) tests that
exercise each threat against the consensus logic. Several threats
additionally have **Docker / wire-protocol** coverage in
`F1R3FLY-io/system-integration` under
`integration-tests/test/test_slash.py`. These tests spawn a
multi-validator shard, ship crafted blocks over the deployed TLS-
encrypted P2P transport, and assert the receiving validator records
the offense and slashes the offender on the wire — confirming that
the deployed binary, gossip layer, gRPC bonds output, and operator-
observable log lines all handle the offense correctly under real
network conditions. The mapping:

| Threat class                                | Docker test                                                                            |
|---------------------------------------------|----------------------------------------------------------------------------------------|
| Direct equivocation (T-1)                   | `test_slash_admissible_equivocation`                                                   |
| Ignorable equivocation flooding (T-9.1)     | `test_slash_ignorable_equivocation` (Bug #1 wire-level regression)                     |
| Slashable invalid block not recorded (T-9.3, Level-2 closure) | `test_slash_neglected_equivocation`, `test_slash_invalid_validator_approve_evil_block` |
| Self-regression/LMD inconsistency (T-9.6)   | `test_slash_self_regression`                                                           |
| Self-correcting block (T-9.9, bug #9)       | `test_slash_self_correcting_block_admitted` (admission via bug-#9 widening)            |
| Evidence visibility and withholding (T-12V, §5.A.5) | `test_slash_late_released_equivocation`                                                |
| Rebonded identity replay (T-9.12)           | `test_slash_stale_evidence_rebond`                                                     |
| Unauthorized received slash deploy (T-9.13) | `test_slash_unauthorized_slash_deploy` (rule #3 unknown evidence), `test_slash_references_valid_block` (rule #4 valid-but-not-invalid evidence) |
| Spoofed system auth token (T-Auth)          | T-Auth itself is not Docker-testable — the auth check lives inside `PoS.rhox`, auth tokens are unforgeable Rholang names. `test_slash_references_valid_block` substitutes a sibling predicate that IS Docker-testable; T-Auth proper stays covered by `uc_21_auth_token_check.rs` in-tree. |

T-37 / T-12PF (Evidence-denial min-cut, censorship — **liveness arm**)
is intentionally NOT in the Docker suite, and the reason is stronger
than "no runtime detector exists" — **the conventional censorship
threat is structurally undefined in this protocol's wire semantics**.
Deploys are not gossiped: the deploy gRPC endpoint
(`casper/src/rust/casper_engine/block_admission.rs:60-94 admit_deploy`)
stores deploys in the local node's `KeyValueDeployStorage`; the block
creator (`casper/src/rust/blocks/proposer/block_creator.rs:52-130
prepare_user_deploys`) reads from that same local storage; no code
path broadcasts a deploy to peers. A deploy submitted to v2 stays on
v2 until v2 proposes it. v1 cannot "censor" v2's deploys because v1
never has them. T-12PF is therefore correctly classified as a
*boundary assumption* — see `slashing-traceability.md` finding 88
(`model_boundary`, "No source bug confirmed"). The boundary status
is a *positive design finding* about the protocol's author-local-
mempool semantics, not a deferred TODO. In-tree property tests
(`proposer_fairness_boundary.rs`, `hypothesis_adversarial_scheduler.rs`,
`hypothesis_liveness_as_safety.rs`, and five other UC tests) cover the
formal threat-model objective for the liveness arm.

The **safety arm** of T-12PF (no *wrongful* slashing under proposer
unfairness) IS Docker-tested by
`test_no_false_positive_slash_on_propose_imbalance` (B1). That test
pins the absence of over-eager behavioral-pattern detectors: it
exercises proposer dominance (one validator proposes 5+ blocks in a
row), validator silence (another stays inactive), and asserts that
no bonds drop. If a future regression introduces an over-eager
"inactive validator" or "proposer dominance" detector, this test
catches it immediately. The wire-level "withholding" theme is
additionally covered by `test_slash_late_released_equivocation`.

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

## 4.1 Vocabulary mapping to traceability ledger

The traceability ledger at `slashing-traceability.md §1` defines an
eight-status vocabulary used to track each Sage / Hypothesis finding
from raw witness through promotion. The six classes above are the
*defensive-classification* projection; the eight statuses are the
*workflow* projection. The mapping is:

| Threat-model class (this §4) | Traceability status (`slashing-traceability.md`)               |
|------------------------------|----------------------------------------------------------------|
| `bisimilar`                  | `confirmed_no_change_needed` (and intermediate `under_review`) |
| `permitted_bug_fix`          | `confirmed_fixed_bug` (after fix is shipped)                   |
| `candidate_boundary`         | `confirmed_boundary` (precondition documented)                 |
| `projection_risk`            | `confirmed_projection_risk` (regression guard added)           |
| `assumption_counterexample`  | `confirmed_assumption_necessary`                               |
| `unexpected`                 | `pending_investigation` (must not remain in this state)        |

The traceability ledger has additional sub-statuses (`shipped`,
`reverted`) that record the deployment outcome of `permitted_bug_fix`
findings; for the threat-model classification this is one bucket.

## 5.A Economic and game-theoretic threats

The threats in §3 are consensus-layer: an adversary tries to subvert
the *protocol* via Byzantine behavior. The threats below are
*game-theoretic*: even when every protocol guard from §3 holds, an
adversary may choose not to play by the protocol's intended
incentives. The defensive primitives at this layer are economic
(stake, slashing penalty, opportunity cost) rather than formal.

The reference economic model for Proof-of-Stake security is
**[Sal21]** F. Saleh, *Blockchain Without Waste: Proof-of-Stake*,
*Review of Financial Studies* 34(3):1156–1190, 2021,
**doi:10.1093/rfs/hhaa075**.

### 5.A.1 Rational adversary

**Threat.** A profit-maximizing adversary computes the expected
profit `π` of an attack and compares it to the slashing penalty
`σ · stake` (where `σ ∈ (0, 1]` is the slashing fraction). If
`π > σ · stake`, the attack is rationally pursued.

**Assumption.** Adversary knows their stake, the slashing fraction,
and the expected attack profit (e.g., a successful double-spend
amount). The protocol does not assume the adversary is honest; it
assumes they are *cost-rational*.

**Defense.** Set `σ` and the maximum-stake-per-validator cap such
that `σ · stake` exceeds any single-attack profit by a margin chosen
by the operator. The F1R3FLY default is `σ = 1` (the entire bond is
forfeited), which makes the rational-adversary condition reduce to
`π > stake` — i.e., the attack must extract more value than the
validator has staked. *Crypto-economic security* is then bounded
below by the aggregate bonded stake.

**Residual risk.** A *very* well-funded attacker may stake the
attack capital, execute the attack, and exit. The protocol relies
on operators choosing `σ` and bond caps such that this is
unprofitable. T-7 (`slash_zeros_bond`) formally guarantees the
penalty is applied; the *economic* claim that `σ · stake > π` is
empirical.

**Formal artifact.** None at the consensus layer; `[Sal21] §3`
formalizes the assumption.

### 5.A.2 Bribery

**Threat.** An external party (a *briber*) offers a payment `β` to
a validator in exchange for misbehavior. If `β + π > σ · stake`,
the validator may rationally accept.

**Assumption.** Bribes are exogenous to the protocol; the protocol
cannot observe them.

**Defense.** (1) Slashing makes the penalty *immediate* and *fully
attributable* — the bribed validator pays publicly. (2) Slashing
is a *public good*: every honest validator's stake benefits from
each successful slash, so honest validators have a positive
expected return from detecting and reporting misbehavior. (3) The
protocol does not assume validators are unbribable; it assumes the
*marginal* validator finds bribes unprofitable at margin.

**Residual risk.** A briber who can target a coalition of size
`> F = ⌊(n−1)/3⌋` can attempt a Byzantine attack. T-12 (BFT-quorum
preservation) ensures that *if* such a coalition acts, the protocol
correctly identifies them; the briber and the bribed coalition all
forfeit stake.

**Formal artifact.** T-12 (collusion-resistance / BFT bound)
provides the upper bound on coalition size before safety is at risk;
the economic-incentive layer is treated qualitatively.

### 5.A.3 Long-range attack

**Threat.** An adversary acquires the private keys of validators
who unbonded long ago (perhaps the keys were sold or stolen after
unbonding). They use these keys to sign a *historical* alternative
chain branching from before the validators unbonded, claiming the
alternative branch is the canonical history.

**Assumption.** Unbonded validators may behave arbitrarily with
their old keys; PKI revocation cannot be assumed.

**Defense.** (1) **Weak-subjectivity checkpointing.** A newly-
joining or long-offline node must consult a recent trusted
checkpoint (e.g., a snapshot signed by current validators) before
deciding the canonical chain. The slashing protocol does not on its
own defend against long-range; weak-subjectivity is required at the
network/operations layer. (2) **Bounded evidence retention window.**
T-12RET ensures stale evidence does not authorize current slashes;
symmetrically, stale signatures from an unbonded key cannot
*demote* a current validator.

**Residual risk.** A node that has been *offline longer than the
weak-subjectivity period* may be tricked by a long-range
alternative chain. Operations protocol (not consensus): rejoin via
a fresh checkpoint.

**Formal artifact.** T-12RET (`Inv_TemporalRetentionBoundary`
family). Out-of-scope for the bisimilarity claim T-15 (weak
subjectivity is in the §13 scope-boundaries clause).

### 5.A.4 Censorship-as-attack

**Threat.** A proposer omits valid evidence — specifically, an
`EquivocationRecord` or `SlashDeploy` it could have included — to
delay or prevent slashing.

**Assumption.** Proposer rotation is fair; eventually a non-
censoring proposer takes over.

**Defense.** (1) T-12V (view-merge over-approximation): merged
local evidence views over-approximate each input, so a censoring
proposer cannot *erase* evidence from the merged view of the next
honest proposer. (2) T-12PF (proposer fairness boundary): under
the fair-proposer assumption, every offender is eventually
included by a non-censoring proposer.

**Residual risk.** If *every* proposer in the active set is
censoring (i.e., the active set is `> F` Byzantine), T-12 is
violated and the protocol is already in BFT-violation territory.

**Formal artifact.** T-12V, T-12PF; Sage Finding 88
(evidence-denial min-cut).

### 5.A.5 Withholding-as-attack

**Threat.** A validator signs an invalid or equivocating block but
withholds it until releasing it is maximally profitable (e.g.,
after a high-value finalization window passes).

**Assumption.** Gossip-fairness: every signed block eventually
propagates to honest validators.

**Defense.** T-12RET (temporal retention boundary): evidence remains
authorizable for a configured retention window, so a withheld block
released within the window still triggers slashing. T-9.10 ensures
withdrawal cannot complete inside the slash window.

**Residual risk.** Releasing outside the retention window: the
attack succeeds *but the attacker has also forfeited the
profitability of the withheld signing* (they cannot earn rewards
from the withheld branch). The economic balance is delicate; the
operator-chosen retention window must cover gossip + inclusion
latency with a margin.

**Formal artifact.** T-12RET, T-9.10.

### 5.A.6 Nothing-at-stake / costless simulation

**Threat.** In a *naïve* PoS without slashing, a validator can sign
multiple competing chains "just in case" — there is no cost to
voting on every fork. This is the *original* motivation for
slashing.

**Defense.** This is the threat slashing *exists to address*. T-7
(`slash_zeros_bond`) is the slashing penalty; T-9.1 (slashable
ignorable equivocation) extends the penalty to ignorable variants
that a naïve nothing-at-stake validator would emit.

**Formal artifact.** T-7, T-9.1 (the entire slashing protocol).

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
