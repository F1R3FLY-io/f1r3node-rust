# Slashing — TLA+ Specifications and Model Checking

This directory contains seven TLA+ specifications and their TLC model-checking
instances for the slashing subsystem. The verification model complements the
Rocq proofs at `formal/rocq/slashing/` by exhaustively model-checking finite
configurations of the protocol.

## Specifications

| File | Purpose |
|---|---|
| `EquivocationDetector.tla` | Pure detector state machine: validator equivocates → detection → status (admissible / ignorable / neglected). |
| `ConcurrentTracker.tla` | Models the lock-free vs. locked equivocation-tracker access. The lock-free version *demonstrates* the Rust-introduced race condition (Bug #2); the locked version proves the fix restores monotonicity. |
| `SlashFlow.tla` | End-to-end pipeline: detection → record → propose/recover → SlashDeploy → PoS bond zeroing/idempotent reissue → fork-choice exclusion. |
| `TwoLevelSlashing.tla` | Level 1 + Level 2 slashing closure; proves termination, fixed-point stabilization, count-weighted quorum under the closure bound, stake-weighted quorum under the weighted closure bound, active-quorum intersection, current-validator and epoch filtering, visibility/report admissibility, validator-renaming equivariance, bounded arithmetic envelopes, and differential-divergence classification. |
| `AuthorizedSlashFlow.tla` | Slash-authorization state machine for current-epoch invalid-block evidence, same-key rebond stale-evidence rejection, received-deploy authorization, invalid-index slash liveness, merge-rejected slash recovery, and duplicate target/hash suppression. |
| `JustificationProjection.tla` | Justification-validator projection model proving duplicate validators are rejected before any validator-key map projection can hide the malformed input. |
| `WithdrawFlow.tla` | Post-quarantine withdrawal flow modelling Bug #10 (T-9.10). Verifies that a failed `posVault.transfer` leaves the validator's `withdrawers` entry intact, the `total_funds` invariant is preserved across success and failure, every removed validator was paid in full, and every withdrawer is eventually paid under fair retry scheduling. Companion Rocq theorem set: `BugFixWithdrawTransferFailure.v` (T-9.10 / T-9.10' / T-9.10″). |

Each `*.tla` has a corresponding `MC_*.tla` instance with TLC parameters
(validator count, max DAG depth, max equivocations) calibrated to keep the
state space ≤ 10⁵.

## Running

```sh
# All seven model-check passes
tlc -workers 12 MC_EquivocationDetector.tla
tlc -workers 12 MC_ConcurrentTracker.tla     # NB: must FAIL pre-fix, PASS post-fix
tlc -workers 12 MC_SlashFlow.tla
tlc -workers 12 MC_TwoLevelSlashing.tla
tlc -workers 12 MC_AuthorizedSlashFlow.tla
tlc -workers 12 MC_JustificationProjection.tla
tlc -workers 12 MC_WithdrawFlow.tla
```

`MC_ConcurrentTracker.tla` is parameterized by `Locked ∈ BOOLEAN`. With
`Locked = FALSE` the spec violates `Inv_NoOverwrite` (this is the bug); with
`Locked = TRUE` it passes. Both runs must be executed and recorded.

## Invariants

| Spec | Invariant | Meaning |
|---|---|---|
| EquivocationDetector | `Inv_DetectionSound` | Every emitted Admissible/Ignorable/Neglected status corresponds to a real equivocation in the trace. |
| EquivocationDetector | `Inv_DetectionComplete` | Every real equivocation is eventually emitted. |
| EquivocationDetector | `Inv_TaxonomyCorrect` | `is_slashable(s) = TRUE` iff `s ∈ {17 slashable variants}`. |
| EquivocationDetector | `Inv_NeglectedHasDetectableView` | Every Neglected status has a Rust latest-message detectability witness. |
| EquivocationDetector | `Inv_FixedDetectorTotal` / `Inv_MissingPointerNonContributing` | Missing latest-message pointers are non-contributing, not fatal. |
| EquivocationDetector | `Inv_DuplicateChildNeedsDistinctChildren` / `Inv_TwoDistinctChildrenDetect` | Duplicate paths to one child do not count as two children; two distinct children detect. |
| EquivocationDetector | `Inv_DetectedHashDetects` | Previously detected hashes remain decisive. |
| ConcurrentTracker | `Inv_NoOverwrite` | The accumulated `equivocationDetectedBlockHashes` set never shrinks. |
| ConcurrentTracker | `Inv_RecordMonotone` | The set of `EquivocationRecord` entries grows monotonically. |
| ConcurrentTracker (temporal) | `[]<>RecordPersists` | Once recorded, a record persists. |
| SlashFlow | `Inv_SlashedExcludedFromFC` | After `SlashDeploy` succeeds, the offender's latest message is filtered from the fork-choice estimator. |
| SlashFlow | `Inv_BondsZeroAfterSlash` | `bondsMap[offender] = 0` after a successful slash. |
| SlashFlow | `Inv_ForfeitedToCoopVault` | `coopVaultBalance` increases by exactly the offender's pre-slash bond. |
| SlashFlow (temporal) | `<>SlashedEventually` | Every detected equivocation eventually results in a slash, given a live proposer schedule. |
| TwoLevelSlashing | `Inv_ActiveSetAboveQuorum` | `|activeValidators| ≥ n − ⌊(n−1)/3⌋` at every reachable state. |
| TwoLevelSlashing | `Inv_ActiveStakeAboveWeightedQuorum` | Active stake remains above the weighted quorum bound when the weighted closure bound is enforced. |
| TwoLevelSlashing | `Inv_ActiveQuorumsIntersect` | Any two active count quorums intersect under the active-size bound. |
| TwoLevelSlashing | `Inv_ActiveStakeQuorumsIntersect` | Any two active stake quorums intersect under the active-stake bound. |
| TwoLevelSlashing | `Inv_ClosureStableAtMaxLevel` | The closure computed for `MaxLevel` is stable under one more closure step. |
| TwoLevelSlashing | `Inv_FilteredClosureInCurrentValidators` | Current-validator filtering prevents stale/off-era evidence from escaping the current validator universe. |
| TwoLevelSlashing | `Inv_EpochEligibleInCurrent` | Epoch-eligible direct equivocators belong to the current epoch's active validator set. |
| TwoLevelSlashing | `Inv_StaleEvidenceNotEligible` | Stale evidence cannot seed the current-epoch closure. |
| TwoLevelSlashing | `Inv_NeglectEdgesVisibleUnreported` | Every neglect edge is backed by visible evidence that was not already reported by the citing validator. |
| TwoLevelSlashing | `Inv_RustViewEdgesDetectableUnreported` | The Rust-view graph is exactly latest-message detectable evidence minus reports. |
| TwoLevelSlashing | `Inv_ReportsSuppressNeglectEdges` | A reported offender is not treated as neglected for that reporter's edge. |
| TwoLevelSlashing | `Inv_NoUnexpectedDifferentialDivergence` | Differential classifications are limited to bisimilar or candidate-boundary classes in this model. |
| TwoLevelSlashing | `Inv_UnsignedArithmeticBoundary` / `Inv_SignedArithmeticBoundary` | Fixed-width arithmetic boundaries agree with the exact `max + 1` Rocq facts. |
| TwoLevelSlashing | `Inv_ArithmeticSafeEnvelope` | Slash accounting fits the configured arithmetic limit whenever the vault plus all bonds fits. |
| TwoLevelSlashing | `Inv_ViewEdgesVisibleUnreported` | View-indexed active evidence edges are visible and unreported. |
| TwoLevelSlashing | `Inv_SameViewSameClosure` | Equal active evidence views compute equal closure. |
| TwoLevelSlashing | `Inv_SameRustViewSameClosure` | Equal Rust latest-message evidence views compute equal closure. |
| TwoLevelSlashing | `Inv_ValidatorRenamingEquivariance` | Bijective validator renaming preserves closure modulo the same renaming. |
| TwoLevelSlashing | `Inv_CarryoverPolicyCurrent` / `Inv_NoCarryoverNoMappedDirect` | Epoch carryover is explicit and current-validator bounded. |
| TwoLevelSlashing | `Inv_EvidenceRetentionForDirectOffenders` | Direct offender evidence is retained when retention enforcement is enabled. |
| TwoLevelSlashing | `Inv_CanonicalRecordKeyInjective` | Canonical record keys are injective pairs. |
| TwoLevelSlashing | `Inv_BatchNoFailureOrderIndependent` / `Inv_PartialBatchFailureRequiresAtomicPolicy` | Successful batch slashing is order-independent; partial failure requires atomic policy. |
| TwoLevelSlashing | `Inv_ProposerFairnessForBoundedLiveness` | Bounded slash liveness requires an observed scheduled proposer to include the evidence when proposer fairness is enforced. |
| TwoLevelSlashing | `AssumptionDivergenceClass` / `SemanticCampaignDivergenceClass` | Combined frontier campaign classifications stay in documented bisimilar, candidate-boundary, projection-risk, or assumption-counterexample buckets. |
| TwoLevelSlashing | `SchedulerDivergenceClass` / `ArithmeticProjectionStressClass` | Deeper scheduler and arithmetic frontier classifications stay in documented buckets. |
| TwoLevelSlashing | `PartitionGossipDivergenceClass` / `ObjectiveGuidedDivergenceClass` / `PreconditionFuzzingClass` | Expanded Hypothesis frontier classifications for partition/gossip, objective-guided campaign scoring, and dropped preconditions stay in documented buckets. |
| TwoLevelSlashing | `RustReplayDivergenceClass` / `DeepThreatModelDivergenceClass` / `DagTraceDivergenceClass` / `AdversarialCampaignDivergenceClass` / `DifferentialOraclePipelineClass` | Rust replay fixtures, deep Sage threat-model witnesses, production-shaped DAG traces, defensive adversarial campaigns, and differential-oracle replay rows are classified as bisimilar, candidate-boundary, projection-risk, or assumption-counterexample cases, never unexpected. |
| TwoLevelSlashing | Search-horizon v3 replay classes | Coverage-gap, detector-traversal-depth, retention-window-boundary, stake-damage-Pareto, and replay-divergence witnesses remain inputs to the existing divergence-class invariants until Rust traceability promotes a new normative behavior. |
| TwoLevelSlashing | `Inv_LevelClosureTerminates` | Iterated Level-2 slashing reaches a fixed point. |
| AuthorizedSlashFlow | `Inv_StaleEvidenceCannotSlashRebondedKey` | Stale invalid-block evidence cannot slash a validator lifetime in a later epoch. |
| AuthorizedSlashFlow | `Inv_OnlyAuthorizedSlashCanBePending` | Every pending slash deploy has matching current-epoch invalid-block evidence. |
| AuthorizedSlashFlow | `Inv_NoInvalidLatestLivenessGap` | Current invalid-block evidence is sufficient to create a pending slash candidate without relying on `invalid_latest_messages`. |
| AuthorizedSlashFlow | `Inv_RejectedSlashWithoutEvidenceNoPending` | Rejected slash deploys do not create pending slash authorization. |
| AuthorizedSlashFlow | `Inv_InvalidAuthSlashNoPending` | Bad-auth slash deploy receipt cannot create pending slash authorization without independent valid evidence. |
| AuthorizedSlashFlow | `Inv_BondsZeroAfterSlash` | Executed authorized slash deploys zero the offender's bond. |
| AuthorizedSlashFlow | `Inv_RecoveredSlashHasEvidence` / `Inv_RecoveredSlashCoveredByPendingOrExecuted` / `Inv_PendingSlashHashUnique` | Merge-rejected slash recovery preserves evidence, avoids uncovered recovered entries, and keeps one pending entry per invalid hash. |
| SlashFlow | `Inv_PendingSlashHasEvidence` / `Inv_RecoveredSlashHasEvidence` / `Inv_RecoveredSlashCovered` / `Inv_SlashSeedInputInjectiveByHash` | Recovered slashes have invalid-block evidence, are pending or already executed, and use seed inputs injective in invalid hash. |
| JustificationProjection | `Inv_DuplicateJustificationsRejected` | A justification list with duplicate validator keys is rejected before projection. |
| JustificationProjection | `Inv_AcceptedImpliesUniqueJustifications` / `Inv_AcceptedProjectionCardinality` | Accepted justification lists preserve one entry per validator under map/set projection. |
| WithdrawFlow | `Inv_NoFundsLost` | A failed `posVault.transfer` does not remove the validator from `withdrawers`; equivalently, every removed validator was paid in full (T-9.10). |
| WithdrawFlow | `Inv_TotalFundsConst` | `posBalance + Σ paidOut = InitialTotal` invariant across all reachable states (T-9.10'). |
| WithdrawFlow | `Inv_RemovedImpliesPaid` | Every validator removed from `withdrawers` was paid `bond + reward`. |
| WithdrawFlow | `Inv_RewardsConsistent` | No validator gets paid more than once or beyond their entitled amount. |
| WithdrawFlow (temporal) | `Live_AllEventuallyPaid` | Every withdrawer is eventually paid out under fair scheduling of `WithdrawSucceeds` and `RetryFromFailed`. |

## Correspondence to Rocq

See `slashing-verification.md` §10.5 for the explicit Rocq↔TLA+ correspondence
table. In summary:

| TLA+ invariant | Rocq theorem |
|---|---|
| `Inv_DetectionSound` | T-1 (`detection_sound` in `EquivocationDetector.v`) |
| `Inv_FixedDetectorTotal`, `Inv_MissingPointerNonContributing`, `Inv_DuplicateChildNeedsDistinctChildren`, `Inv_TwoDistinctChildrenDetect`, `Inv_DetectedHashDetects` | T-9.11 (`fixed_detectable_*` in `EquivocationDetector.v`) |
| `Inv_RecordMonotone` (with Locked=TRUE) | T-9.2 (`t_9_2_atomic_no_overwrite` in `BugFixAtomicTracker.v`) |
| `Inv_BondsZeroAfterSlash` | T-7 (`slash_zeros_bond` in `PoSContract.v`) |
| `Inv_ForfeitedToCoopVault` | T-8 (`slash_transfers_stake` in `PoSContract.v`) |
| `Inv_SlashedExcludedFromFC` | T-10 (`fork_choice_exclusion` in `ForkChoice.v`) |
| `Inv_StakeConservation` | T-7 + T-8 corollary (combination of `slash_zeros_bond` and `slash_transfers_stake`) |
| `Inv_LevelClosureTerminates` | T-11 (`t_11_level_2_termination` in `TwoLevelSlashing.v`) |
| `Inv_ActiveStakeAboveWeightedQuorum` | `weighted_slash_iter_quorum_preservation` in `TwoLevelSlashing.v` |
| `Inv_ActiveQuorumsIntersect` | `quorum_intersection_by_size` in `TwoLevelSlashing.v` |
| `Inv_ActiveStakeQuorumsIntersect` | `weighted_quorum_intersection_from_disjoint_bound` in `TwoLevelSlashing.v` |
| `Inv_ClosureStableAtMaxLevel` | `slash_iter_fixed_point_after_universe_bound` in `TwoLevelSlashing.v` |
| `Inv_FilteredClosureInCurrentValidators` | `restricted_closure_only_from_current_direct_offenders` in `TwoLevelSlashing.v` |
| `Inv_EpochEligibleInCurrent` / `Inv_StaleEvidenceNotEligible` | `epoch_filter_in` in `TwoLevelSlashing.v` |
| `Inv_NeglectEdgesVisibleUnreported` | `visible_unreported_graph_in` in `TwoLevelSlashing.v` |
| `Inv_RustViewEdgesDetectableUnreported` | `rust_detectable_view_graph_in` in `TwoLevelSlashing.v` |
| `Inv_ReportsSuppressNeglectEdges` | `visible_unreported_graph_in` in `TwoLevelSlashing.v` |
| `Inv_NoUnexpectedDifferentialDivergence` | `DivergenceClass` / `divergence_allowed` in `Bisimulation.v` |
| `Inv_UnsignedArithmeticBoundary` / `Inv_SignedArithmeticBoundary` | `unsigned_overflow_boundary_exact` / `signed_overflow_boundary_exact` in `TwoLevelSlashing.v` |
| `Inv_ArithmeticSafeEnvelope` | `arithmetic_safe_envelope` in `TwoLevelSlashing.v` |
| `Inv_ViewEdgesVisibleUnreported` | `visible_unreported_graph_in` / `reported_edge_not_active` in `TwoLevelSlashing.v` |
| `Inv_SameViewSameClosure` | `view_closure_equiv_by_active_edges` in `TwoLevelSlashing.v` |
| `Inv_SameRustViewSameClosure` | `same_rust_detectable_view_same_closure` in `TwoLevelSlashing.v` |
| `Inv_ValidatorRenamingEquivariance` | `slash_iter_validator_renaming_equiv` in `TwoLevelSlashing.v` |
| `Inv_CarryoverPolicyCurrent` / `Inv_NoCarryoverNoMappedDirect` | `carryover_policy_sound` in `TwoLevelSlashing.v` |
| `Inv_EvidenceRetentionForDirectOffenders` | `restricted_closure_only_from_current_direct_offenders` precondition in `TwoLevelSlashing.v` |
| `Inv_CanonicalRecordKeyInjective` | `canonical_key_pair_injective` in `EquivocationRecord.v` |
| `Inv_CurrentRustRecordLifecycleRetainsRecords` | `current_rust_record_update_retains_all_detected_hashes` in `EquivocationRecord.v` |
| `Inv_BatchNoFailureOrderIndependent` / `Inv_PartialBatchFailureRequiresAtomicPolicy` | `bm_slash_many_order_independent` / `bm_slash_many_abort_order_dependent` in `Validator.v` |
| `Inv_ProposerFairnessForBoundedLiveness` | `proposer_fairness_boundary_requires_review` in `Bisimulation.v` |
| `SemanticCampaignDivergenceClass` | `semantic_campaign_boundary_reasons_require_review` in `Bisimulation.v` |
| `AssumptionDivergenceClass` | minimized assumption examples in `TwoLevelSlashing.v` |
| `SchedulerDivergenceClass` | `adversarial_scheduler_boundary_reasons_require_review` in `Bisimulation.v` |
| `ArithmeticProjectionStressClass` | `arithmetic_projection_stress_boundary_8bit` in `TwoLevelSlashing.v` |
| `PartitionGossipDivergenceClass` / `ObjectiveGuidedDivergenceClass` / `PreconditionFuzzingClass` / `RustReplayDivergenceClass` / `DeepThreatModelDivergenceClass` / `DagTraceDivergenceClass` / `AdversarialCampaignDivergenceClass` / `DifferentialOraclePipelineClass` | `frontier_expansion_reasons_require_review` in `Bisimulation.v`; `deep_threat_chain_closure_bound_assumption_needed` in `TwoLevelSlashing.v` |
| `Inv_LivenessAsSafety` (Eager) | T-2 (`detection_complete` in `EquivocationDetector.v`) |
| `Inv_StaleEvidenceCannotSlashRebondedKey`, `Inv_OnlyAuthorizedSlashCanBePending`, `Inv_RejectedSlashWithoutEvidenceNoPending`, `Inv_InvalidAuthSlashNoPending`, `Inv_BondsZeroAfterSlash` | `ValidatorLifetime.v`, `BugFixSlashAuthorization.v`, and `SlashDeploy.v` authorization/no-op theorems |
| `Inv_RecoveredSlashHasEvidence`, `Inv_RecoveredSlashCoveredByPendingOrExecuted`, `Inv_PendingSlashHashUnique`, `Inv_SlashSeedInputInjectiveByHash` | `BlockCreator.v` recoverable rejected-slash hash theorems and `SlashDeploy.v` seed-input injectivity |
| `Inv_NoInvalidLatestLivenessGap` | `BlockCreator.deploy_epoch_matches_target` and `BugFixSlashAuthorization.authorized_execution_zeros_offender` |
| `Inv_DuplicateJustificationsRejected`, `Inv_AcceptedImpliesUniqueJustifications`, `Inv_AcceptedProjectionCardinality` | `BugFixDuplicateJustifications.v` |

Note: `Inv_DetectionComplete` is the temporal property `Live_DetectionComplete`
in `EquivocationDetector.tla`; under the eager rewrite
`EquivocationDetectorEager.tla` it becomes the safety invariant
`Inv_LivenessAsSafety` (see §10.4 of the verification doc).
`Inv_NoOverwrite` is defined in `ConcurrentTracker.tla` for documentation
but the actually-checked invariant in both `MC_ConcurrentTracker.cfg` and
`MC_ConcurrentTracker_pre_fix.cfg` is the stronger `Inv_RecordMonotone`.
`Inv_ActiveSetAboveQuorum` is checked in `MC_TwoLevelSlashing.cfg` with
`EnforceClosureBound = TRUE`; the corresponding universal BFT-style claim is
mechanized in Rocq as `t_12_bft_quorum_preservation`.

## What TLA+ proves and does not

**TLA+ proves:** That for the modeled finite configurations (e.g. up to 4
validators, DAG depth ≤ 6, ≤ 3 equivocations), the protocol satisfies every
listed invariant on every reachable state and every fair execution.

**TLA+ does not prove:** Universal claims for arbitrary `n`, arbitrary DAG
depth, or arbitrary equivocation count. Those are the province of the Rocq
proofs. TLA+ is here to catch specification bugs the Rocq proofs might mask
(e.g. an inadvertently strong hypothesis), not to certify the protocol on
unbounded state.
