# Cost-Accounted Rho Threat Model

**Status:** Implementation-aligned security and thread-safety model
**Scope:** Cost-accounted rho calculus, runtime-budget refinement,
Casper replay/settlement integration, and slashing composition.

This document applies the slashing threat-model style to the
cost-accounted rho migration. It records the adversary model, the
security and concurrency vectors that matter for cost accounting, the
formal theorem that protects each vector, and the Rust/TLA+ tests that
exercise the production boundary.

The scoped publications define the cost-accounting semantics. They assume
familiarity with the existing F1R3node architecture, so their wallets,
purses, mints, fee channels, and located resources are semantic roles rather
than instructions to create a second ledger. The authority chain is: the two
scoped papers, the repo-local refinement into SystemVault/RSpace/PoS/replay,
the Rocq/TLA+/Sage models of that refinement, and the Rust implementation.

## 1. Terms

| Term | Meaning |
|---|---|
| Runtime fuel | The per-deploy source-token budget used during Rholang evaluation. |
| Settlement balance | Canonical SystemVault custody plus authenticated prepaid located-stack authority. Admission proves a certificate-bound maximum against the authenticated pre-state. Located-stack authority is consumed and maximum split, realized cost, fee transfer, and refund execute under one settlement checkpoint without creating a parallel balance or reservation ledger. |
| Cost trace | The recorded sequence of successful billable source-token events plus the optional out-of-phlo boundary event. It is required to compute `total_cost` and is retained alongside the signature as diagnostic/audit evidence. **As of TM-CA-151 its per-operation digest/event-count is no longer a consensus commitment** — consensus cost integrity is carried by `total_cost` (clamped) + status + post-state hash; the digest is diagnostic only. (Rows authored before TM-CA-151 — e.g. the "Cost trace" usage in TM-CA-006/007 and the §7 digest/count failure modes — describe the pre-decision consensus role and are superseded by TM-CA-151 on the digest/count point.) |
| Diagnostic log | Bounded observability data that is not consensus evidence. Clearing it cannot affect cost or replay. |
| Cost-invalid evidence | Replay-visible evidence that a block's cost accounting fields are invalid and may feed slashing only through the current evidence epoch, current target activation epoch, and parent pre-state bond boundary. |
| Bootstrap replay | Protocol-2 genesis replay, which reconstructs the same blessed SystemVault initialization before ordinary admission. It is not an accounting-off mode. |
| Cost-accounted replay | Ordinary protocol-2 replay. It reconstructs authority, cost, status, adjacent roots, settlement, and fee from authenticated consensus data. The per-operation trace digest and count remain diagnostic under TM-CA-151. |
| Thread vector | A concurrency vector in which evaluator workers race on frame queues, budget reservation, OOP ownership, or trace finalization. |

## 2. Adversary Model

The adversary may:

- Submit validly signed deploys with adversarial source terms, phlo
  limits, phlo prices, timestamps, and signatures.
- Attempt to forge or replay cost-trace fields in processed deploys,
  block payloads, replay-cache keys, and serialized wire messages.
- Control evaluator scheduling indirectly through deploy structure,
  causing concurrent branches to race on budget reservation and OOP
  boundaries.
- Cause deploy failures, parser failures, out-of-phlo failures,
  rollback paths, and mixed success/failure blocks.
- Submit blocks with low prices, missing traces, mutated costs,
  stale or ambient-only cost-invalid evidence, or unauthorized
  fee-settlement effects.
- Attempt protocol downgrade attacks so a retired block or replay
  representation bypasses protocol-2 cost accounting.
- Attempt denial of service with oversized event weights, large event
  descriptors, many deploys, and long finalization windows.

The adversary may not:

- Break the cryptographic hash and signature assumptions used by block
  signatures, deploy signatures, and domain-separated signature channels.
- Mutate private validator state without passing through consensus or
  replay validation.
- Bypass the slashing protocol's independently verified authorization
  rules. This threat model imports those rules as the slashing boundary.

## 3. STRIDE Classification

STRIDE classifies threats into six inclusive buckets: **S**poofing,
**T**ampering, **R**epudiation, **I**nformation disclosure, **D**enial
of service, and **E**levation of privilege. The labels in the §5 matrix
are inclusive, not partitioning; a single cost-accounting threat may carry
more than one bucket when the attack crosses replay, settlement, or source
boundaries.

| STRIDE bucket | Cost-accounting vector | Representative defense |
|---|---|---|
| **S** | Forged fuel channels, replayed deploy signatures, cross-deploy token reuse, spoofed source provenance | Domain-separated deploy signature channels, fuel-gate safety, source-anchor metadata. |
| **T** | Mutated processed-deploy cost, replay payload, block hash, settlement, slashing, source, or model fields (the per-op cost-trace digest/event count are diagnostics, not consensus fields — TM-CA-151) | Replay mismatch checks on the consensus quantities (`total_cost` + status + post-state hash), replay payload hashing, block hash tests, settlement proofs, and production-oracle fixtures. |
| **R** | Proposer or model output denies cost-invalid evidence, replay mismatch, source-witness status, or promotion traceability | Replay-failure records, source-anchor digests, witness classification, and promotion-gate tests. |
| **I** | Diagnostic data, API/source metadata, private-key debug surfaces, dependency advisory policy, or TLS key-path disclosure | Non-consensus diagnostic separation, audit classifications, TLS/source-graph fixtures, and dependency policy review. |
| **D** | Oversized weights, descriptor growth, trace/cache pressure, unbounded search, scheduler pressure, or CI resource exhaustion | Reject-before-mutation admission, production event caps, cache bounds, and bounded search envelopes. |
| **E** | System deploy authority leaks into user deploys, settlement replenishes runtime fuel, legacy activation bypasses replay, or slashing authority is spoofed | Scoped unmetered mode, post-evaluation settlement, activation guards, and slashing authorization checks. |

## 4. Attack Tree

Root: violate cost-accounted rho safety, replay determinism, settlement
authority, or parallelism.

1. Execute without fuel.
   - Forge signature channel.
   - Reuse another deploy's token.
   - Enter user execution through legacy charging.
2. Make validators disagree on cost.
   - Exploit parallel scheduling.
   - Exploit OOP race ownership.
   - Mutate cost trace or event count.
   - Exploit nondeterministic primitive descriptors or source-path identities.
3. Hide tampering from replay.
   - Drop trace fields.
   - Reuse replay cache after mutation.
   - Serialize through a default/empty trace field after activation.
   - Mutate block fields not covered by the hash/signature payload.
4. Manipulate settlement.
   - Refund during evaluation.
   - Over-refund after evaluation.
   - Use unauthorized system deploys.
   - Accept low deploy price as cost-valid.
5. Abuse slashing composition.
   - Present stale or ambient-only cost-invalid evidence.
   - Recover a rejected slash with non-current evidence.
   - Mutate the slash target activation epoch outside replay authentication.
   - Forge low-price or unauthorized-settlement evidence.
   - Apply slashing effects that mutate runtime fuel.
6. Exhaust validator resources.
   - Submit oversized event weights.
   - Produce large cost-trace descriptors.
   - Retain traces beyond finalization.
   - Force many mixed success/OOP deploys.

## 5. Threat Coverage Matrix

The coverage matrix uses cost-accounting categories plus inclusive STRIDE
labels. Category labels are review lenses; STRIDE labels describe the
security failure mode.

> **See also.** The linear-logic structure of the compound-signature
> authorization algebra under `CA-CAP` — including the *no-free-weakening*
> guarantee that a presented-but-invalid signature cannot be silently dropped
> while its phlo share still funds the envelope total — is documented in
> [*The Linear Logic of Compound Signatures*](cost-accounting-linear-logic.md).

| Category | Review lens |
|---|---|
| `CA-CAP` | Capability, auth, signature, and system-authority boundaries. |
| `CA-BUDGET` | Runtime fuel, admission, producer routing, trace-slot, and OOP accounting. |
| `CA-TRACE` | Cost-trace identity, digest, canonicalization, and deterministic semantics. |
| `CA-REPLAY` | Replay, replay cache, block authentication, activation, and legacy downgrade. |
| `CA-SETTLE` | Precharge, refund, fee settlement, and fuel-isolation boundaries. |
| `CA-SLASH` | Cost-invalid evidence and slashing composition. |
| `CA-RESOURCE` | Descriptor, trace-window, cache, validator, CI, and search resource pressure. |
| `CA-EXT` | External service, API, source-corpus, and production semantic boundaries. |
| `CA-SEARCH` | Model/search/frontier promotion and traceability governance. |
| `CA-SOURCE` | Current-source anchoring and source-graph security surfaces. |

| Category | Representative rows |
|---|---|
| `CA-CAP` | TM-CA-001, TM-CA-002, TM-CA-027, TM-CA-042, TM-CA-043, TM-CA-148, TM-CA-149, TM-CA-150 |
| `CA-BUDGET` | TM-CA-003, TM-CA-004, TM-CA-013, TM-CA-033, TM-CA-050, TM-CA-051, TM-CA-109 |
| `CA-TRACE` | TM-CA-005, TM-CA-007, TM-CA-014, TM-CA-031, TM-CA-066, TM-CA-111 |
| `CA-REPLAY` | TM-CA-006, TM-CA-009, TM-CA-010, TM-CA-011, TM-CA-045, TM-CA-080, TM-CA-119, TM-CA-151 |
| `CA-SETTLE` | TM-CA-016, TM-CA-017, TM-CA-018, TM-CA-028, TM-CA-053, TM-CA-095 |
| `CA-SLASH` | TM-CA-021, TM-CA-022, TM-CA-054, TM-CA-078, TM-CA-090, TM-CA-138 |
| `CA-RESOURCE` | TM-CA-024, TM-CA-025, TM-CA-038, TM-CA-040, TM-CA-069, TM-CA-074 |
| `CA-EXT` | TM-CA-026, TM-CA-030, TM-CA-055, TM-CA-093, TM-CA-107, TM-CA-113 |
| `CA-SEARCH` | TM-CA-049, TM-CA-057, TM-CA-062, TM-CA-085, TM-CA-108, TM-CA-142 |
| `CA-SOURCE` | TM-CA-124, TM-CA-128, TM-CA-132, TM-CA-136, TM-CA-137, TM-CA-140, TM-CA-141 |

| ID | Category | STRIDE | Threat / thread vector | Status | Formal anchor | Rust/TLA+ coverage |
|---|---|---|---|---|---|---|
| TM-CA-001 | `CA-CAP` | **S + E** | Forged fuel capability or synthetic billing token | Protected | `fuel_gate_no_app_channel_overlap`, `uc_ca_003_signature_channel_separation` | `signature_channels_are_deploy_isolated`, domain-separated signature tests |
| TM-CA-002 | `CA-CAP` | **S + T** | Signature channel collision or cross-deploy token reuse | Protected | `uc_ca_003_signature_channel_separation`, `rb_full_replay_payload_signature_change_detected` | `deploy_signature_scope_is_domain_separated_from_raw_signature_bytes` |
| TM-CA-003 | `CA-BUDGET` | **T + D** | Runtime budget overspend under parallel evaluation | Protected / strengthened | `rb_total_remaining_conservation`, `uc_ca_024_reservation_batch_preserves_budget_conservation` | canonical batch permit tests, `concurrent_runtime_budget_reservations_are_linearizable`, `RuntimeBudgetReplay.tla` |
| TM-CA-004 | `CA-BUDGET` | **T + R** | Misattributed OOP failure across live branches | Protected / strengthened | `uc_ca_025_reservation_batch_has_at_most_one_oop`, `uc_ca_063_threaded_oop_boundary_ownership` | canonical OOP batch tests, `loom_metering_ownership`, stress tests |
| TM-CA-005 | `CA-TRACE` | **T** | Schedule-dependent cost total or digest | Protected / strengthened | `ca_cost_deterministic`, `uc_ca_051_parallel_trace_and_cost_determinism` | batch permutation, parallel permutation, and repeatability tests |
| TM-CA-006 | `CA-REPLAY` | **T + R** | Cost-trace truncation, event-count mismatch, or missing digest | Protected | `uc_ca_039_post_activation_cost_trace_required`, `uc_ca_040_full_replay_payload_authenticates_cost_trace_fields` | replay cost-trace mismatch tests |
| TM-CA-007 | `CA-TRACE` | **T** | Digest canonicalization collision, duplicate omission, or domain confusion | Protected | `uc_ca_053_cost_trace_domain_separation_and_multiplicity` | descriptor, kind, multiplicity, and OOP digest tests |
| TM-CA-008 | `CA-REPLAY` | **T** | Processed deploy scalar cost tampering | Protected | `uc_ca_010_replay_cost_mismatch_sound` | replay cost mismatch tests |
| TM-CA-009 | `CA-REPLAY` | **T + R** | Replay cache or state-hash cache masks tampering | Protected | `uc_ca_048_replay_cache_key_authenticates_cost_trace_payload` | replay payload hash and replay mismatch tests |
| TM-CA-010 | `CA-REPLAY` | **T + R** | Block signature/hash omits cost fields | Protected | `uc_ca_047_block_authenticates_cost_trace_payload`, `uc_ca_062_block_validation_authenticates_cost_fields` | block hash mutation tests |
| TM-CA-011 | `CA-REPLAY` | **T + E** | Wire/protobuf downgrade reaches consensus without protocol-2 accounting fields | Protected | `ProtocolVersionLifecycle`; exact current-protocol admission | protocol-version, protobuf roundtrip, and replay tests |
| TM-CA-012 | `CA-REPLAY` | **T + E** | Retired broad charging or replay path bypasses the authority budget | Protected | `uc_ca_038_legacy_metering_quarantine`; protocol-2-only lifecycle | static legacy guard and unsupported-version regressions |
| TM-CA-013 | `CA-BUDGET` | **T** | Admission failure consumes or refunds incorrectly | Protected | `uc_ca_007_no_metered_step_without_token` | malformed source no-consumption tests |
| TM-CA-014 | `CA-TRACE` | **T** | Primitive/substitution weight nondeterminism | Protected | `uc_ca_050_billable_reservation_enters_cost_trace`, `uc_ca_059_deterministic_billable_descriptor_sensitivity` | typed billable event tests |
| TM-CA-015 | `CA-TRACE` | **T** | Host/order/path normalization divergence | Protected | `uc_ca_059_deterministic_billable_descriptor_sensitivity` | parallel permutation and descriptor-sensitivity tests |
| TM-CA-016 | `CA-SETTLE` | **T + E** | Precharge/refund overflow, underflow, or minting | Protected | `uc_ca_009_refund_is_bounded_by_escrow`, `uc_ca_027_settlement_exhaustion_and_zero_price` | refund boundary property tests |
| TM-CA-017 | `CA-SETTLE` | **T + E** | Refund mutates runtime fuel during evaluation | Protected | `uc_ca_009_post_evaluation_settlement_mints_no_fuel`, `uc_ca_058_refund_cannot_replenish_runtime_fuel` | settlement and unmetered-mode tests |
| TM-CA-018 | `CA-SETTLE` | **T + E** | Unauthorized fee settlement or system deploy authority leak | Protected | `uc_ca_055_unauthorized_settlement_and_budget_mutation_are_cost_invalid` | unauthorized boundary tests and threat-model adequacy proof |
| TM-CA-019 | `CA-CAP` | **E** | Unmetered system mode leaks into user deploys | Protected | `uc_ca_035_unmetered_system_mode_restoration`, `uc_ca_061_system_mode_cannot_leak_into_user_metering` | system mode restoration tests |
| TM-CA-020 | `CA-SETTLE` | **T + R** | Low deploy price accepted as cost-valid execution | Protected | `uc_ca_056_low_deploy_price_is_cost_invalid_evidence` | low-price evidence model and validation tests |
| TM-CA-021 | `CA-SLASH` | **T + R + E** | Cost-invalid slashing evidence is absent, stale, forged, ambient-only, or unauthenticated | Protected | `uc_ca_057_stale_cost_invalid_evidence_is_rejected`, `uc_ca_146_canonical_slash_candidate_requires_current_evidence`, `uc_ca_147_parent_pre_state_slash_authorization_preserves_cost_boundary` | canonical candidate and slashing boundary tests |
| TM-CA-022 | `CA-SLASH` | **T + E** | Slashing effects alter user runtime cost or settlement | Protected | `uc_ca_012_slashing_preserves_settlement_accounting`, `uc_ca_028_slashing_after_evaluation_cannot_add_fuel` | slashing replay/hash tests |
| TM-CA-023 | `CA-TRACE` | **I + T** | Diagnostic log affects consensus | Boundary protected | `uc_ca_036_diagnostic_retention_is_non_consensus` | diagnostic clearing tests |
| TM-CA-024 | `CA-RESOURCE` | **D + R** | Finalization-window trace retention leaks memory or loses evidence | Protected | `uc_ca_031_finalization_reads_completed_cost_trace`, `uc_ca_060_reset_clears_retained_trace_after_finalization` | RuntimeBudget finalization-read model, deploy-reset trace clearing, and TLA+ `RuntimeBudgetReplay` |
| TM-CA-025 | `CA-RESOURCE` | **D** | Huge descriptors/events cause DoS before charging | Protected | `uc_ca_044_oversized_weight_rejection_preserves_trace`, `uc_ca_060_reset_clears_retained_trace_after_finalization` | oversized event rejection tests |
| TM-CA-026 | `CA-EXT` | **T + R** | Nondeterministic external service output changes replay cost | Boundary protected | `uc_ca_064_external_nondeterminism_requires_replay_evidence` | nondeterministic-service replay fixtures |
| TM-CA-027 | `CA-CAP` | **T + E** | Unsafe FFI/API can set unlimited cost in consensus path | Protected by quarantine | `uc_ca_038_legacy_metering_quarantine`, `uc_ca_054_activation_replay_rejects_absent_commitment` | legacy guard script and unsafe escape-hatch audit |
| TM-CA-028 | `CA-SETTLE` | **T** | Multi-deploy block settlement cross-contaminates budgets | Protected | `uc_ca_034_multi_deploy_budget_isolation_and_settlement_sum`, `uc_ca_043_matched_unmatched_deploy_trace_and_settlement_isolation` | matched/unmatched deploy isolation test |
| TM-CA-029 | `CA-REPLAY` | **T + R** | Add-block validation fails to enforce replay cost fields | Protected | `uc_ca_062_block_validation_authenticates_cost_fields` | block hash and replay mutation tests |
| TM-CA-030 | `CA-EXT` | **T** | Generated term replay diverges from production execution | Protected | `uc_ca_005_well_reflected_replay_step_sound` | bounded generated play/replay tests |
| TM-CA-031 | `CA-TRACE` | **T** | Trace sequence omits tie-breaker data needed for deterministic replay | Protected | `uc_ca_053_cost_trace_domain_separation_and_multiplicity`, `uc_ca_059_deterministic_billable_descriptor_sensitivity` | source-path/redex/local-index digest tests |
| TM-CA-032 | `CA-SETTLE` | **T + R** | Precharge/refund weakens signatures or authentication | Protected | `uc_ca_022_replay_payload_signature_change_detected`, `uc_ca_058_refund_cannot_replenish_runtime_fuel` | signature and settlement tests |
| TM-CA-033 | `CA-BUDGET` | **T + D** | Zero-weight billable event grows authenticated trace without fuel | Protected | `uc_ca_065_zero_weight_billable_event_rejected` | zero-weight reservation rejection tests |
| TM-CA-034 | `CA-BUDGET` | **T** | Generic cost normalization hides invalid producers | Protected | `uc_ca_068_admitted_success_has_positive_bounded_weight` | `MeteredMachine` rejects zero billable source costs |
| TM-CA-035 | `CA-BUDGET` | **T** | Variable-work primitive has no work but still emits trace evidence | Protected | `uc_ca_045_nonbillable_frames_do_not_enter_cost_trace`, `uc_ca_068_admitted_success_has_positive_bounded_weight` | incremental primitive zero-work tests |
| TM-CA-036 | `CA-BUDGET` | **T** | Negative user method argument becomes negative or wrapped cost | Protected | `uc_ca_068_admitted_success_has_positive_bounded_weight` | normalized `slice`/`take` producer tests |
| TM-CA-037 | `CA-BUDGET` | **T** | Empty/default substitution produces zero-weight billable event | Protected | `uc_ca_068_admitted_success_has_positive_bounded_weight` | substitution producer floors standalone billable work |
| TM-CA-038 | `CA-RESOURCE` | **D** | Primitive descriptor memory amplification before replay hashing | Protected | `uc_ca_066_oversized_billable_event_rejected`, `rb_oversized_primitive_descriptor_admission_rejection_preserves_trace`, `uc_ca_059_deterministic_billable_descriptor_sensitivity` | descriptor bound rejection and exact-boundary admission tests |
| TM-CA-039 | `CA-RESOURCE` | **D** | Source-path descriptor memory amplification | Protected | `uc_ca_066_oversized_billable_event_rejected`, `rb_oversized_source_path_admission_rejection_preserves_trace`, `uc_ca_059_deterministic_billable_descriptor_sensitivity` | source-path bound rejection and exact-boundary admission tests |
| TM-CA-040 | `CA-RESOURCE` | **D** | Full cost-trace vector grows without retention bound or finalization hashing amplification | Protected / strengthened | `uc_ca_067_trace_cap_rejection_preserves_budget`, `uc_ca_060_reset_clears_retained_trace_after_finalization` | Rust `MAX_COST_TRACE_EVENTS`, streaming cost-trace hashing, reset clearing, and `RuntimeBudgetReplay.tla` retention bound |
| TM-CA-041 | `CA-TRACE` | **R + I** | Public diagnostic-clear API erases replay evidence | Protected | `uc_ca_036_diagnostic_retention_is_non_consensus` | diagnostic clearing leaves cost trace unchanged |
| TM-CA-042 | `CA-CAP` | **E** | Manual unmetered flag leaks after error return | Protected | `uc_ca_061_system_mode_cannot_leak_into_user_metering` | scoped unmetered guard test |
| TM-CA-043 | `CA-CAP` | **T + E** | Unsafe unlimited budget reaches consensus replay/add-block path | Protected by quarantine | `uc_ca_038_legacy_metering_quarantine`, `uc_ca_054_activation_replay_rejects_absent_commitment` | legacy guard and replay payload checks |
| TM-CA-044 | `CA-BUDGET` | **T** | Negative initial phlo silently maps to zero budget | Protected | `uc_ca_068_admitted_success_has_positive_bounded_weight` | negative initial phlo rejection test |
| TM-CA-045 | `CA-REPLAY` | **T + R** | Replay-cache eviction accepts stale trace without recomputation | Protected | `uc_ca_048_replay_cache_key_authenticates_cost_trace_payload` | replay cache payload field tests |
| TM-CA-046 | `CA-BUDGET` | **T + R** | Finalization observes budget before workers finish trace append | Protected | `uc_ca_041_concurrent_finalization_trace_completeness` | parallel digest/count tests and finalization documentation |
| TM-CA-047 | `CA-BUDGET` | **T + R** | OOP or user error hides the cost-invalid trace boundary | Protected | `uc_ca_042_oop_trace_survives_failed_deploy_boundary`, `uc_ca_063_threaded_oop_boundary_ownership` | OOP rollback and loom ownership tests |
| TM-CA-048 | `CA-SETTLE` | **T + E** | Malformed deploy precharge/refund path mutates runtime fuel | Protected | `uc_ca_058_refund_cannot_replenish_runtime_fuel`, `uc_ca_009_charged_plus_refund_equals_escrow` | settlement edge-case tests |
| TM-CA-049 | `CA-SEARCH` | **R** | Generated witness is treated as an implementation bug before Rust traceability | Protected by classification rule | `CostAccountingSearchFrontier.tla` | search-horizon witness classification model |
| TM-CA-050 | `CA-BUDGET` | **T** | Producer routing regresses and sends zero-capable work through strict billable reservation | Guarded-safe projection risk | `uc_ca_069_producer_routing_search_frontier` | Sage guard label `projection_risk_zero_weight_strict_route_rejects_before_trace_mutation`, Rust `projection_risk_witnesses_have_guarded_safe_disposition`, and zero-weight Rocq rejection |
| TM-CA-051 | `CA-BUDGET` | **T + D** | Trace-slot reservation leaks capacity after repeated OOP or invalid admission races | Protected / strengthened | `uc_ca_070_trace_slot_linearizability_frontier` | Loom trace-slot shadow model and runtime-budget fuzz |
| TM-CA-052 | `CA-REPLAY` | **T + R** | Replay field mutation is missed by generated frontier fixtures | Protected | `uc_ca_071_replay_mutation_frontier` | replay mutation Sage/TLA model and fuzz target |
| TM-CA-053 | `CA-SETTLE` | **T** | Multi-deploy block settlement aggregates non-locally and contaminates another deploy's refund | Protected / strengthened | `uc_ca_072_multi_deploy_settlement_frontier` | settlement Sage model and generated Rust fixture |
| TM-CA-054 | `CA-SLASH` | **T + E** | Cost-invalid slashing evidence mutates runtime fuel instead of staying post-evaluation evidence | Protected | `uc_ca_073_slashing_composition_frontier` | threat model plus slashing composition bridge tests |
| TM-CA-055 | `CA-EXT` | **T + R** | External service result changes cost, errors, or trace without replay-authenticated evidence | Boundary protected | `uc_ca_064_external_nondeterminism_requires_replay_evidence` | nondeterministic-service replay fixtures |
| TM-CA-056 | `CA-RESOURCE` | **D** | Descriptor, source path, or lifecycle trace causes resource exhaustion in search frontier | Protected | `uc_ca_074_resource_exhaustion_frontier`, `RuntimeBudgetReplay.ValidEvent` | descriptor/source-path bounds and cost lifecycle fuzz |
| TM-CA-057 | `CA-SEARCH` | **R** | Search objective over-focus hides lower-severity but novel vectors | Protected by frontier ranking | `CostAccountingSearchFrontier.tla` | Sage objective-frontier Pareto ranking |
| TM-CA-058 | `CA-SEARCH` | **T + R** | Kani/proptest/fuzz harness proves only a shadow helper and drifts from production source | Guarded by witness rule | `CostAccountingSearchFrontier.tla` | production-path replay requirement in search-horizon doc |
| TM-CA-059 | `CA-SEARCH` | **R** | Optional frontier tools silently skip all meaningful checks | Boundary protected | Search-horizon run metadata | smoke runner requires nextest and records skipped optional tools |
| TM-CA-060 | `CA-REPLAY` | **T + E** | Legacy broad cost path reappears outside the static guard | Protected by quarantine | `uc_ca_038_legacy_metering_quarantine` | legacy guard plus producer-routing guard |
| TM-CA-061 | `CA-TRACE` | **T** | Cost trace digest remains stable under a replay-relevant frontier mutation | Protected | `uc_ca_071_replay_mutation_frontier` | replay payload and digest field-sensitivity tests |
| TM-CA-062 | `CA-SEARCH` | **R** | Frontier fixture corpus grows without deterministic minimized promotion | Protected by promotion rule | Search-horizon promotion rules | deterministic fixture replay and generated JSON summaries |
| TM-CA-063 | `CA-SEARCH` | **R** | Sage/TLA witness is dismissed as a model artifact while violating production invariant | Protected by witness rule | `CostAccountingSearchFrontier.tla` | classification requires Rust or invariant traceability |
| TM-CA-064 | `CA-SEARCH` | **T + R** | Source changes are made from an unclassified witness | Protected by classification invariant | `NoSourceFixWithoutRustOrInvariantEvidence` | `CostAccountingSearchFrontier.tla` |
| TM-CA-065 | `CA-BUDGET` | **T + R** | Stateful lifecycle search finds finalization before worker trace completion | Guarded-safe projection risk | `uc_ca_041_concurrent_finalization_trace_completeness`, `uc_ca_070_trace_slot_linearizability_frontier` | Sage guard label `projection_risk_parallel_evaluation_result_waits_for_complete_cost_trace`, generated replay fixtures, Rust join-before-digest behavior, and `RuntimeBudgetReplay.tla` |
| TM-CA-066 | `CA-TRACE` | **T** | Metamorphic event permutation changes cost or digest when it should be canonicalized | Protected | `uc_ca_051_parallel_trace_and_cost_determinism` | generated metamorphic replay test |
| TM-CA-067 | `CA-TRACE` | **T** | Duplicate or descriptor-mutated event is incorrectly canonicalized away | Protected | `uc_ca_053_cost_trace_domain_separation_and_multiplicity`, `uc_ca_059_deterministic_billable_descriptor_sensitivity` | generated metamorphic replay test |
| TM-CA-068 | `CA-SEARCH` | **R** | Generated corpus fixture loses its terminal classification over time | Protected by corpus replay | `ClassifiedWitnessHasPromotionTarget` | persistent corpus replay via search-horizon runner |
| TM-CA-069 | `CA-RESOURCE` | **D** | Optional deepening tools create unbounded resource pressure in CI | Boundary protected | Search-horizon run metadata and 32GB memory caps | `SEARCH_RSS_LIMIT`, `TLC_MAX_HEAP`, `SYSTEMD_CPU_QUOTA`, `ALLOW_UNBOUNDED_SEARCH` |
| TM-CA-070 | `CA-SEARCH` | **R** | Objective-guided search overweights easy replay cases and misses settlement or slashing composition | Protected by objective selection | Sage objective/frontier schema | `SAGE_OBJECTIVES` and coverage summaries |
| TM-CA-071 | `CA-SEARCH` | **T + R** | Cross-product interactions hide a bug not visible in isolated budget, replay, settlement, or slashing checks | Protected by v2 horizon classification | Sage v2 cross-product frontier | generated differential Rust replay fixtures |
| TM-CA-072 | `CA-EXT` | **T** | Real Rholang source paths, primitive descriptors, or size profiles differ from synthetic search fixtures | Protected by source-aware seed replay | Sage v2 source-seed frontier | source-derived generated Rust replay fixtures |
| TM-CA-073 | `CA-SEARCH` | **T + R** | Replay-cache substitution, refund replenishment, slashing/refund confusion, descriptor inflation, or rollback/finalization campaigns bypass classification | Protected by exploit-campaign buckets | Sage v2 exploit campaign frontier | generated differential replay and threat ledger |
| TM-CA-074 | `CA-RESOURCE` | **D** | Deep search silently exceeds validator or CI memory limits and masks useful failures | Protected by enforced RSS cap | Search-horizon runner memory envelope | `systemd-run --user` `MemoryMax=32G`, `MemorySwapMax=0`, and `TLC_MAX_HEAP=28g` |
| TM-CA-075 | `CA-SEARCH` | **R** | Stateful campaign witnesses are promoted without naming operation steps, oracle, or production path | Protected by v3 frontier metadata invariants | `CostAccountingSearchFrontier.tla` and Sage v3 stateful search | `generated_frontier_stateful_campaign_fixtures_hold` |
| TM-CA-076 | `CA-SEARCH` | **T + R** | Production-path differential search drifts into shadow-helper-only evidence | Protected by v3 production-path metadata and production-shaped Rust replay tests | Sage v3 production-path records | `generated_frontier_stateful_campaign_fixtures_hold` |
| TM-CA-077 | `CA-EXT` | **T** | Source-corpus descriptors expose cost-trace behavior not covered by synthetic fixtures | Protected by v3 source-corpus projection | Sage v3 source-corpus records | generated stateful campaign replay fixtures |
| TM-CA-078 | `CA-SLASH` | **T + R + E** | Slashing, refund, replay authentication, and resource bounds fail only when composed together | Protected by v3 exploit cross-product search plus composed replay/block/settlement hardening coverage | Sage v3 exploit cross-product frontier | Rust `generated_frontier_stateful_campaign_fixtures_hold`; Sage guard labels `cross_product_replay_payload_and_block_hash_authenticates_user_cost_trace_and_slash_fields`, `refund_uses_scalar_cost_without_mutating_authenticated_trace_fields` |
| TM-CA-079 | `CA-BUDGET` | **T** | Repeated OOP or invalid billable events mutate trace or budget after the rejection boundary | Protected by v4 adversarial budget fixtures | Sage v4 adversarial horizon | `generated_frontier_adversarial_fixtures_hold` |
| TM-CA-080 | `CA-REPLAY` | **T + R** | Replay payload mutation across digest, count, signature, status, or block hash is not authenticated | Protected by v4 adversarial replay fixtures and composed Casper hash tests | Sage v4 adversarial horizon | `generated_frontier_adversarial_fixtures_hold` |
| TM-CA-081 | `CA-SETTLE` | **T + E** | Casper refund is treated as runtime fuel after settlement | Protected by v4 adversarial settlement fixtures | Sage v4 adversarial horizon | `generated_frontier_adversarial_fixtures_hold` |
| TM-CA-082 | `CA-SLASH` | **T + R + E** | Stale cost-invalid slashing evidence is replayed across a boundary | Protected | Sage v4 adversarial slashing fixtures, `stale_cost_evidence_sound`, and `stale_canonical_slash_candidate_not_authorized` | Rust `generated_frontier_adversarial_fixtures_hold`; canonical candidate tests reject non-current evidence |
| TM-CA-083 | `CA-BUDGET` | **T + R** | Finalization occurs before worker trace completion or rollback/reserve ordering is ambiguous | Guarded-safe projection risk | Sage v4 adversarial lifecycle fixtures and TLA+ runtime-budget replay | Rust `generated_frontier_adversarial_fixtures_hold`; Sage guard labels `projection_risk_parallel_evaluation_result_waits_for_complete_cost_trace`, `projection_risk_lifecycle_campaign_does_not_leak_budget_or_trace` |
| TM-CA-084 | `CA-EXT` | **T** | Real source-corpus descriptors collide or hide descriptor mutation sensitivity | Protected by v4 adversarial source-corpus fixtures | Sage v4 adversarial horizon | `generated_frontier_adversarial_fixtures_hold` |
| TM-CA-085 | `CA-SEARCH` | **R** | A mined candidate invariant is accepted without production replay evidence | Protected by v5 property fixture metadata | Sage v5 property/security horizon | `generated_frontier_property_fixtures_hold` requires candidate property, oracle strength, replay command, and classification metadata. |
| TM-CA-086 | `CA-REPLAY` | **T + R** | Negative replay-authentication mutations omit a cost-relevant signed field | Protected by v5 negative-auth mutation matrix | Sage v5 negative-auth horizon | `generated_frontier_negative_auth_fixtures_hold` replays digest, count, scalar cost, signature, block hash, status, slash, genesis, and trace-presence mutations. |
| TM-CA-087 | `CA-EXT` | **T** | Real source shapes differ from generated fixtures enough to hide path or primitive-descriptor bugs | Protected by v5 source-shape corpus replay | Sage v5 source-shape horizon | `generated_frontier_source_shape_fixtures_hold` requires source-seed evidence and production-shaped RuntimeBudget replay. |
| TM-CA-088 | `CA-TRACE` | **T** | Same primitive descriptor/path in different deploys collapses trace identity or settlement | Protected by v5 cross-deploy property fixtures | Sage v5 cross-deploy horizon | `generated_frontier_property_fixtures_hold` checks deploy-domain separation and deploy-local settlement witnesses. |
| TM-CA-089 | `CA-BUDGET` | **T + D** | Scheduler joins, rollbacks, and finalization preserve correctness only by serializing evaluation | Protected by v5 scheduler fixtures plus existing join guards | Sage v5 scheduler horizon and `RuntimeBudgetReplay.tla` | Scheduler witnesses require completion-before-finalization metadata while preserving parallel evaluation before the join boundary. |
| TM-CA-090 | `CA-SLASH` | **T + R + E** | Settlement, slashing, and replay-cache evidence compose into a stale or duplicate slash vector | Protected | Sage v5 settlement/slashing/cache horizon and slashing proof bridge | Confirmed-safe cache composition is replayed by v5 fixtures; canonical scanning excludes stale or absent evidence. |
| TM-CA-091 | `CA-RESOURCE` | **D** | Replay-cache churn or descriptor boundaries consume unbounded memory before rejection | Protected by v5 cache/resource fixtures | Sage v5 cache/resource horizon | Property fixtures cover bounded cache churn and descriptor-boundary/overflow admission behavior. |
| TM-CA-092 | `CA-SEARCH` | **T + R** | Model-only witnesses drift from the real RuntimeBudget production path | Protected by v6 production differential replay | Sage v6 production frontier | `generated_frontier_production_fixtures_hold` compares generated cost, digest, count, OOP, and invalid-admission outcomes against RuntimeBudget. |
| TM-CA-093 | `CA-EXT` | **T** | Generated Rholang source shape evaluates differently from the fixture projection | Protected by v6 Rholang evaluation replay | Sage v6 production frontier | `generated_frontier_rholang_eval_fixtures_hold` evaluates `rho_source` through `RhoRuntime::evaluate_with_term` before promotion. |
| TM-CA-094 | `CA-REPLAY` | **T + R + E** | Cost-accounted replay is downgraded by removing trace presence or mutating signed payload fields | Protected by v6 replay-downgrade fixtures | Sage v6 production frontier | Casper-boundary fixtures require digest, count, trace-presence, signature, and block-hash differential axes. |
| TM-CA-095 | `CA-SETTLE` | **T + E** | Casper settlement evidence mutates runtime fuel after production replay | Protected by v6 settlement isolation fixture | Sage v6 production frontier | Production-frontier settlement fixtures assert escrow/refund arithmetic without RuntimeBudget replenishment. |
| TM-CA-096 | `CA-SLASH` | **T + R + E** | Duplicate stale slashing evidence re-enters as valid user-cost mutation | Protected | Sage v6 production frontier, `stale_canonical_slash_candidate_not_authorized`, and slashing guard tests | Complete canonical scanning selects at most one candidate per target and excludes stale or absent evidence. |
| TM-CA-097 | `CA-BUDGET` | **T + R** | Scheduler finalization correctness relies on serializing evaluation instead of joining before finalization | Protected by v6 scheduler production frontier | Sage v6 production frontier and `RuntimeBudgetReplay.tla` | Scheduler witnesses compare join-before-finalize evidence while preserving parallelism before the join boundary. |
| TM-CA-098 | `CA-BUDGET` | **T** | Invalid admission mutates production cost or trace before rejection | Protected by v6 production resource boundary | Sage v6 production frontier | Invalid-admission fixtures reject before RuntimeBudget cost, digest, or event-count mutation. |
| TM-CA-099 | `CA-EXT` | **T** | Non-Nil Rholang source evaluates differently from the generated cost-trace projection | Protected by v7 production semantic eval | Sage v7 production semantic frontier | `generated_frontier_semantic_eval_fixtures_hold` evaluates non-trivial sources through `RhoRuntime::evaluate_with_phlo` and checks cost, digest, count, and errors. |
| TM-CA-100 | `CA-REPLAY` | **T + R** | Play/replay preserves state effects but loses or changes cost evidence | Protected by v7 play/replay frontier | Sage v7 production semantic frontier and `RuntimeBudgetReplay.tla` | `generated_frontier_play_replay_fixtures_hold` compares play and replay cost, digest, event count, and error classification after replay data is consumed. |
| TM-CA-101 | `CA-EXT` | **T + R** | Real source-corpus shapes remain metadata-only and never exercise production semantics | Protected by v7 source-corpus semantic replay | Sage v7 production semantic frontier | Source-seed metadata must be attached to a non-Nil `rho_source` that evaluates successfully before promotion. |
| TM-CA-102 | `CA-EXT` | **T + R** | Finite-phlo, user-abort, or parser-error boundaries are mistaken for cost-valid success | Protected by v7 error-boundary fixtures | Sage v7 production semantic frontier | `generated_frontier_phlo_boundary_fixtures_hold` requires explicit error-kind classification and cost-trace evidence. |
| TM-CA-103 | `CA-REPLAY` | **T + R** | State-root replay evidence is accepted without checking replay data consumption | Protected by v7 state-root fixtures | Sage v7 production semantic frontier | `generated_frontier_state_root_fixtures_hold` uses production event-log rigging and `check_replay_data`. |
| TM-CA-104 | `CA-REPLAY` | **T + R** | Replay-authenticated Casper payload axes drift from actual production eval cost evidence | Protected by v7 auth-composition fixtures | Sage v7 production semantic frontier | `generated_frontier_auth_composition_fixtures_hold` ties eval cost evidence to cost, digest, count, signature, block-hash, trace-presence, and refund axes. |
| TM-CA-105 | `CA-EXT` | **T** | Curated semantic fixtures miss generated grammar-family bugs | Protected by v8 generative semantic fixtures | Sage v8 generative semantic frontier | `generated_frontier_generative_semantic_fixtures_hold` covers bounded send, receive/join, arithmetic, auth/settlement/slashing, and OOP-boundary families. |
| TM-CA-106 | `CA-TRACE` | **T** | Source rewrites or parallel permutations alter cost evidence unexpectedly | Protected by v8 metamorphic fixtures | Sage v8 generative semantic frontier | `generated_frontier_semantic_metamorphic_fixtures_hold` checks canonical event digest permutation and semantic success/error preservation for source variants. |
| TM-CA-107 | `CA-EXT` | **T + R** | External nondeterminism hides replay or cost-trace drift | Protected by v8 mocked external-service replay | Sage v8 generative semantic frontier | `generated_frontier_external_service_replay_fixtures_hold` replays GPT/gRPC mock success and error cases and compares cost, digest, count, and error classification. |
| TM-CA-108 | `CA-SEARCH` | **R** | Search broadening silently loses required coverage | Protected by v8 adequacy gate | Sage v8 generative semantic frontier | `generated_frontier_coverage_adequacy_holds` fails if required families, features, or classifications are absent. |
| TM-CA-109 | `CA-BUDGET` | **T + D** | RuntimeBudget accepts generated event sequences with unsound cost, count, OOP, or digest behavior | Protected by v8 property test | Sage v8 generative semantic frontier and Rust proptest | `runtime_budget_event_sequence_properties_hold` generates valid event sequences and checks monotonic cost, bounded fuel, OOP ownership, and digest recomputation. |
| TM-CA-110 | `CA-EXT` | **T + R** | Source-corpus records are promoted without executable production semantics | Protected by v9 corpus semantic fixtures | Sage v9 differential corpus/security frontier | `generated_frontier_corpus_semantic_fixtures_hold` requires source-seed/corpus-case evidence and production evaluation axes. |
| TM-CA-111 | `CA-TRACE` | **T** | Grammar rewrites hide schedule-dependent cost or error behavior | Protected by v9 grammar mutation fixtures | Sage v9 differential corpus/security frontier | `generated_frontier_grammar_mutation_fixtures_hold` checks variant evaluation classification and canonical digest stability for regrouped independent events. |
| TM-CA-112 | `CA-SEARCH` | **T + R** | Differential oracles drift from production play/replay or error classification | Protected by v9 differential oracle fixtures | Sage v9 differential corpus/security frontier | `generated_frontier_differential_oracle_fixtures_hold` compares production play/replay projections and parser-error classification. |
| TM-CA-113 | `CA-EXT` | **T + R** | External-service classes are under-sampled or replay-unstable | Protected by v9 external-service matrix fixtures | Sage v9 differential corpus/security frontier | `generated_frontier_external_service_matrix_fixtures_hold` covers GPT, DALL-E, TTS, and gRPC mock success/error replay cases. |
| TM-CA-114 | `CA-REPLAY` | **T + R** | Casper authenticated payload fields or settlement/slashing axes are omitted from frontier replay | Protected by v9 Casper security matrix fixtures | Sage v9 differential corpus/security frontier | `generated_frontier_casper_security_matrix_fixtures_hold` requires cost, digest, count, signature, block hash, trace presence, refund, slashing, and settlement evidence. |
| TM-CA-115 | `CA-TRACE` | **T** | Multi-deploy trace interleavings collapse deploy identity or become schedule-dependent | Protected by v9 runtime trace interleaving fixtures | Sage v9 differential corpus/security frontier | `generated_frontier_runtime_trace_interleaving_properties_hold` checks canonical order stability and deploy-domain mutation sensitivity. |
| TM-CA-116 | `CA-SEARCH` | **R** | Search adequacy no longer covers corpus, grammar, production, service, Casper, and trace axes together | Protected by v9 adequacy gate | Sage v9 differential corpus/security frontier | `generated_frontier_v9_coverage_adequacy_holds` fails if required v9 families, coverage features, classifications, or bounded search-budget metadata disappear. |
| TM-CA-117 | `CA-SEARCH` | **R** | Fuzz or Kani witnesses are treated as implementation bugs without production replay traceability | Protected by V10 promotion-gate metadata | Sage v10 hybrid fuzz/security frontier | `generated_frontier_v10_fuzz_seed_fixtures_hold` requires bounded depth, replay target, promotion gate, and terminal classification metadata. |
| TM-CA-118 | `CA-BUDGET` | **T + R** | Lifecycle fuzzing misses finalize/replay/settlement ordering hazards | Protected by V10 lifecycle trace fixtures | Sage v10 hybrid fuzz/security frontier | `generated_frontier_v10_lifecycle_trace_fixtures_hold` keeps campaign steps explicit and verifies runtime fuel is not replenished. |
| TM-CA-119 | `CA-REPLAY` | **T + R** | Replay-payload fuzzing omits digest, count, or trace-presence mutation axes | Protected by V10 replay payload matrix fixtures | Sage v10 hybrid fuzz/security frontier | `generated_frontier_v10_replay_payload_matrix_fixtures_hold` requires replay or negative mutations tied to cost-trace axes. |
| TM-CA-120 | `CA-REPLAY` | **T + R** | Casper block authentication fuzzing omits refund or slashing composition | Protected by V10 Casper block-auth fixtures | Sage v10 hybrid fuzz/security frontier | `generated_frontier_v10_casper_block_auth_fixtures_hold` checks signature, block hash, trace presence, refund projection, and slash evidence together. |
| TM-CA-121 | `CA-BUDGET` | **T** | Parallel schedule stress becomes order-dependent or collapses deploy identity | Protected by V10 parallel schedule fixtures | Sage v10 hybrid fuzz/security frontier | `generated_frontier_v10_parallel_schedule_stress_fixtures_hold` checks permutation digest stability and deploy-domain mutation sensitivity. |
| TM-CA-122 | `CA-EXT` | **T + R** | Semantic corpus fuzzing promotes metadata-only source mutations | Protected by V10 semantic corpus fixtures | Sage v10 hybrid fuzz/security frontier | `generated_frontier_v10_semantic_corpus_mutation_fixtures_hold` evaluates primary and variant Rholang sources and preserves source-corpus evidence. |
| TM-CA-123 | `CA-SEARCH` | **R** | Search expansion drops fuzz, Kani, lifecycle, replay, Casper, settlement, slashing, or legacy coverage | Protected by V10 adequacy gate | Sage v10 hybrid fuzz/security frontier | `generated_frontier_v10_coverage_adequacy_holds` fails if required V10 families, features, classifications, replay targets, or promotion gates are absent. |
| TM-CA-124 | `CA-SOURCE` | **T + R** | Search witnesses drift away from current `f1r3node-rust` source surfaces | Protected by V11 source anchoring | Sage v11 source-anchored frontier | `generated_frontier_v11_source_anchored_fixtures_hold` requires file, symbol, line, surface, risk, reachability, and source-presence metadata for every source-anchored witness. |
| TM-CA-125 | `CA-SOURCE` | **T + R** | Runtime, metering, or parallel implementation changes silently invalidate model assumptions | Protected by V11 runtime-source checks | Sage v11 source-anchored frontier | `generated_frontier_v11_runtime_budget_source_risks_hold` ties admission, OOP, trace-slot, unmetered, pending-queue, local-index, and parallel scheduling risks to current Rust anchors. |
| TM-CA-126 | `CA-SOURCE` | **T + R** | Casper replay, settlement, slashing, or legacy quarantine changes are not reflected in the frontier | Protected by V11 Casper/source checks | Sage v11 source-anchored frontier | `generated_frontier_v11_casper_settlement_slashing_source_risks_hold` binds replay digest/count, replay payload hashing, refund arithmetic, slashing evidence, and absent legacy metering to current Rust anchors. |
| TM-CA-127 | `CA-SOURCE` | **R** | Source-anchored search omits a required cost surface | Protected by V11 adequacy gate | Sage v11 source-anchored frontier | `generated_frontier_v11_coverage_adequacy_holds` fails if runtime budget, metering, parallel evaluation, Casper replay, settlement, slashing, or legacy-quarantine surfaces are missing. |
| TM-CA-128 | `CA-SOURCE` | **R** | Source-anchored production-oracle witness is promoted without native Rust replay | Protected by V12 production-oracle replay | Sage v12 production-oracle frontier | `generated_frontier_v12_production_oracle_fixtures_hold` requires every v12 witness to carry oracle surface, mutation axis, expected disposition, and source-anchor metadata. |
| TM-CA-129 | `CA-SOURCE` | **T + R** | RuntimeBudget, metering, or parallel source anchors pass metadata checks while native behavior regresses | Protected by V12 runtime/metering/parallel oracles | Sage v12 production-oracle frontier | `generated_frontier_v12_runtime_metering_parallel_oracles_hold` checks accepted reservations, reject-before-mutation, OOP boundary commitment, canonical billable drain order, non-billable exclusion, and parallel digest stability. |
| TM-CA-130 | `CA-SOURCE` | **T + R** | Casper replay, settlement, slashing, or legacy quarantine source anchors pass metadata checks while production authentication or fuel isolation regresses | Protected by V12 Casper/settlement/slashing oracles | Sage v12 production-oracle frontier | `generated_frontier_v12_casper_settlement_slashing_oracles_hold`, `cost_accounting_v12_casper_replay_payload_oracles_hold`, and `cost_accounting_v12_slashing_replay_oracles_hold` check replay hash mutation sensitivity, bounded settlement, slashing isolation, and absent legacy broad charging. |
| TM-CA-131 | `CA-SOURCE` | **R** | Production-oracle search omits required surfaces, dispositions, or mutation axes | Protected by V12 adequacy gate | Sage v12 production-oracle frontier | `generated_frontier_v12_coverage_adequacy_holds` fails if runtime budget, metering, parallel evaluation, Casper replay, settlement, slashing, legacy quarantine, expected dispositions, replay targets, or promotion gates are missing. |
| TM-CA-132 | `CA-SOURCE` | **R** | Cross-surface source-semantic witness is promoted without source facets or source-anchor digest | Protected by V13 metadata gate | Sage v13 source-semantic frontier and TLA+ `CostAccountingSearchFrontier` | `generated_frontier_v13_source_semantic_oracles_hold` and `SourceSemanticWitnessHasFacets` require semantic oracle, source facets, source-anchor digest, cross-surface role, production path, oracle, and Rust reproducer metadata. |
| TM-CA-133 | `CA-SOURCE` | **T + R** | Runtime trace evidence reaches replay or settlement with stale source assumptions | Protected by V13 runtime/replay/settlement oracles | Sage v13 source-semantic frontier | `generated_frontier_v13_runtime_metering_parallel_oracles_hold`, `cost_accounting_v13_source_semantic_replay_payload_oracles_hold`, and `cost_accounting_v13_settlement_slashing_legacy_oracles_hold` check runtime-to-replay trace authentication and runtime-to-settlement fuel isolation against current Rust surfaces. |
| TM-CA-134 | `CA-SOURCE` | **T + R** | Metering or parallel scheduling regressions are hidden by source-anchor-only checks | Protected by V13 metering/parallel oracle | Sage v13 source-semantic frontier | `generated_frontier_v13_runtime_metering_parallel_oracles_hold` checks canonical billable drain order and completion-order digest stability while preserving maximum parallel evaluation before finalization. |
| TM-CA-135 | `CA-SOURCE` | **T + R + E** | Replay/slashing authentication or legacy quarantine regresses after V12 native oracles pass | Protected by V13 replay/slashing/legacy oracles | Sage v13 source-semantic frontier | `generated_frontier_v13_casper_settlement_slashing_oracles_hold`, `cost_accounting_v13_source_semantic_replay_payload_oracles_hold`, and `cost_accounting_v13_settlement_slashing_legacy_oracles_hold` bind slashing fields to replay payload hashing and keep the legacy runtime metering surface absent. |
| TM-CA-136 | `CA-SOURCE` | **S + T + R** | API ingress, runtime cost evidence, and replay payload hashing drift apart from current source anchors | Protected by V14 source-graph oracle | Sage v14 source-graph frontier | `generated_frontier_v14_source_graph_oracles_hold` and `cost_accounting_v14_replay_slashing_oracles_hold` bind API ingress to runtime/replay cost evidence. |
| TM-CA-137 | `CA-SOURCE` | **T + R** | Replay-cache payload binding is promoted without cost-trace digest/count mutation evidence | Protected by V14 source-graph oracle | Sage v14 source-graph frontier | `generated_frontier_v14_source_graph_oracles_hold` and `cost_accounting_v14_replay_slashing_oracles_hold` check replay-cache and payload-hash axes. |
| TM-CA-138 | `CA-SLASH` | **T + R + E** | Slashing authorization omits epoch, slash-field, block-hash, signature, target activation, evidence epoch, or parent pre-state payload binding | Protected by V14 source-graph oracle | Sage v14 source-graph frontier and slashing authorization boundary | `generated_frontier_v14_slashing_security_oracles_hold` and `cost_accounting_v14_replay_slashing_oracles_hold` require replay-invalid slashing mutation axes. |
| TM-CA-143 | `CA-SLASH` | **T + R + E** | Canonical slash candidate selection bypasses evidence presence, exact-current epochs, uniqueness, or parent pre-state authorization | Protected by V14 source-graph oracle and Rocq bridge | `uc_ca_146_canonical_slash_candidate_requires_current_evidence`, `uc_ca_147_parent_pre_state_slash_authorization_preserves_cost_boundary`, `uc_ca_148_slash_target_epoch_is_replay_authenticated`, `uc_ca_149_zero_bond_slash_noop_preserves_cost_boundary` | `generated_frontier_v14_slashing_security_oracles_hold`, `canonical_prestate_zero_bond_excludes_duplicate_slash`, and `slashing::slash_authorization_regressions` |
| TM-CA-139 | `CA-SOURCE` | **S + I** | TLS peer-certificate or key-path boundary becomes only implicit source metadata | Protected by V14 node-security fixture | Sage v14 source-graph frontier | `generated_frontier_v14_node_security_oracles_hold` retains peer-certificate and TLS key-path anchors outside runtime fuel mutation. |
| TM-CA-140 | `CA-SOURCE` | **I** | Private-key debug exposure is treated as a confirmed bug without source audit | Audit-classified by V14 source-graph oracle | Sage v14 source-graph frontier | `generated_frontier_v14_node_security_oracles_hold` keeps `crypto_key_material` as `needs_source_audit`. |
| TM-CA-141 | `CA-SOURCE` | **I + R** | Accepted RustSec advisory policy is hidden from source-graph security review | Audit-classified by V14 source-graph oracle | Sage v14 source-graph frontier | `generated_frontier_v14_node_security_oracles_hold` keeps `dependency_advisory` and `RUSTSEC-2026-0098` as `needs_source_audit`. |
| TM-CA-142 | `CA-SEARCH` | **R** | Source-graph search omits required runtime, replay-cache, slashing, TLS, crypto, API, or dependency surfaces | Protected by V14 adequacy gate | Sage v14 source-graph frontier | `generated_frontier_v14_coverage_adequacy_holds` fails if required source-graph surfaces or promotion metadata disappear. |
| TM-CA-143 | `CA-RESOURCE` | **D + T** | Low-phlo deploy forces high physical fanout before cost is charged | Protected by permit frontier | RuntimeBudget canonical batch permit refinement | `batch_commit_charges_only_granted_execution_permits`, metering permit-grant tests, and low-phlo parallel replay fixtures. |
| TM-CA-144 | `CA-TRACE` | **T + R** | Reducer-attempt identity depends on Tokio scheduling, producing different cost evidence across honest validators and triggering `ReplayCostTraceMismatch` → `InvalidTransaction` → `UnauthorizedSlashDeploy` | **Protected by changing the semantic boundary, not by hiding the evidence.** Native cost is observed once for each complete successful RSpace COMM while the channel group is locked. Unmatched introductions and scheduler-local reducer attempts are not charged. Replay observes the same causal COMM witness. The old digest remains diagnostic. | `AtomicCommAccounting` and `AtomicCommRejection`; DR-32 | `RhoCommObserver`; `COMM::cost_identity`; producer/consumer trigger equivalence; play/replay exact-cost and rejection-rollback tests. |
| TM-CA-145 | `CA-BUDGET` | **D** | Lock-free attempt log grows without bound under sustained reservation pressure, exhausting node memory | Protected by `MAX_COST_TRACE_EVENTS = 1_048_576` cap enforced inside `reconcile`'s sort-and-walk and runtime-side `cost_trace_event_count` check | RuntimeBudget Option E | `rb_reconcile` truncates the canonical attempt list at `MAX_COST_TRACE_EVENTS`; the existing `trace_cap_boundary` fixture covers the cap surface. |
| TM-CA-146 | `CA-TRACE` | **T** | Reconciliation cache leaks across deploys, causing the next deploy's `cost_trace_digest` to include the previous deploy's events | Protected by `reset_from_token` clearing both `attempt_log` and the `canonical_reconciliation` cache under the reset write-lock | RuntimeBudget Option E | `runtime_budget_reset_from_token_serializes_with_batch_commit` racing reset against batch commit; `reset_from_token` always clears both fields atomically. |
| TM-CA-147 | `CA-REPLAY` | **T + R** | Per-fork budget partitioning starves recursive Rholang contracts by geometric budget halving | **Protected.** A deploy uses one authority-derived finite capacity. Successful COMMs reserve from that capacity at the locked match boundary; forks do not receive private geometric shares. | `AtomicCommAccounting.BudgetNeverOverspent`; `EndToEndAuthority`; state-bound admission models | Recursive-contract determinism tests, state-bound capacity tests, and COMM observer tests. |
| TM-CA-148 | `CA-CAP` | **E** | A non-system principal treats `!A` as a mint and obtains unbounded native custody from one registration | **Protected.** `!` controls reusable capability presentation; it neither calls SystemVault `protocolMint` nor creates located-stack cells. Each funded execution still consumes a finite SystemVault reservation or authenticated stack cell, and bounded registrations remain counter-limited. | `BangProtocol.tla`; `WhyNotProtocol.tla`; `ll_bang_reuse_no_extra_linear_cost`; `user_ca_step_does_not_mint` | Capability-service counters, authority-capacity admission, and the SystemVault-only protocol-mint boundary |
| TM-CA-149 | `CA-CAP` | **T + E** | `A ⊸ 1` is mistaken for weakening and used to create a free funded continuation | **Protected by the linear semantics.** Firing the lollipop consumes the distinct source/rendezvous authority `A`; multiplicative unit `1` correctly requires no continuation resource and cannot be materialized as a funding-stack cell. No `A`, REV, or reusable capability is created or refunded. | lollipop source-consumption and no-weakening theorems; `unit_cannot_be_materialized_as_a_stack_cell`; LollyProtocol safe model | Runtime located-lollipop reductions consume source then continuation authority; Unit remains inert and non-funding |
| TM-CA-150 | `CA-CAP` | **T** | Partial funding: an under-funded multi-step process halts between credit and debit (application-currency non-conservation) | **Protected at the deploy boundary.** State-bound admission rejects any execution that exhausts its finite authority-derived capacity, so an underfunded execution cannot become block evidence. Independently, `process_deploy_cosigned_with_budget` reverts the complete user soft checkpoint on any failed evaluation. Application contracts that intentionally split a transfer across separate deploys remain responsible for their own transactional protocol. | `EndToEndAuthority.exhausted_execution_cannot_be_certified`; `StateBoundAdmission.tla`; `token_monotone_step` (`TokenConservation.v:59`) | Capacity-bounded state-bound fixed point; `process_deploy_cosigned_with_budget` (`runtime.rs:771-840`); exhaustion, rollback, and replay-equality tests |
| TM-CA-151 | `CA-REPLAY` | **T + R** | Concurrent play and replay discover the same COMM through different trigger paths or schedules, risking different cost identity or an underfunded partial mutation | **Protected.** Cost identity is the semantic COMM projection and excludes trigger direction and scheduler-local telemetry. The observer executes before mutation in both play and replay; reservation failure preserves tuplespace state and replay evidence. Exact cost is the number of committed COMMs, bounded by authenticated authority capacity. Diagnostic attempt digests are not independent consensus commitments. | Rocq `comm_trace_cost_permutation_invariant`, `trigger_side_does_not_change_cost`, `rejected_comm_is_atomic`; TLA+ `AtomicCommAccounting`, `AtomicCommRejection`; DR-32 | `cost_identity_ignores_produce_telemetry`; `cost_identity_commits_repetition_count`; trigger-side equivalence; replay rejection rollback; state-bound play/replay tests. |
| TM-CA-152 | `CA-REPLAY` | **T** | Forge cost authority by fabricating a payer purse or located funding stack | **Protected.** Native custody is addressed by verified signer identity in SystemVault, whose protocol-only methods require system authority. Located stacks are unforgeable RSpace capabilities and admission may consume only cells present in the authenticated pre-state. No user reduction can manufacture either kind of authority. | `WalletNaming.system_vault_name_injective`; `system_vault_funding_slot_domain_disjoint`; `user_ca_step_does_not_mint`; authority-presentation models | SystemVault authorization; verified `vault_payer`; funding-slot seed-domain separation; exact stack-pop certificates and replay checks |
| TM-CA-153 | `CA-REPLAY` | **T** | Double-spend or oversubscribe authority across deployments in a block | **Protected.** Proposal physically reserves SystemVault custody and pops selected pre-state stack cells through one canonical live residual ledger. Missing authority is zero. Replay reconstructs the certificate, residual ledger, and exact cost-plus-fee allocation from the authenticated body and parent root. | `EndToEndAuthority.certified_reservation`, `realized_le_reservation`, `deployment_kind_never_exempts_funding`; TLA+ `EveryExecutedDeploymentWasFunded`, `NoSupplyUnderflow`, `ReplayMatchesProposal`; Sage settlement model | state-bound admission; `recompute_settlement_debits`; `ReplayAdmissionMismatch`; fee-allocation tamper and over-admission regressions |
| TM-CA-154 | `CA-REPLAY` | **T + R** | Mint replay or multi-parent merge credits a validator's canonical SystemVault twice | **Protected.** `(validator, epoch)` idempotency permits one `protocolMint` credit. The mint target is the validator's canonical SystemVault, and proposal/replay derive the same eligible set and amount. | `epoch_mint_idempotent_on_balance`; `system_vault_credit_injective_in_pk`; `SlashFlow.tla` no-double-credit invariant; vault lifecycle model | `mintedEpochs`; SystemVault `protocolMint`; PoS close-block and play/replay mint regressions |
| TM-CA-155 | `CA-REPLAY` | **T + E** | A non-system principal calls the native mint path or credits a SystemVault without ownership authority | **Protected.** Only the system-authorized `protocolMint` entry point may increase aggregate native custody. Ordinary deposit moves existing REV under purse authority and user cost-accounting steps cannot mint. | `user_ca_step_does_not_mint`; `user_ca_step_does_not_increase_balance`; `system_vault_credit_injective_in_pk` | SystemVault system-auth gate, unforgeable purse handles, and forged/absent system-token regressions |
| TM-CA-156 | `CA-REPLAY` | **T** | A slashed validator retains spendable cost authority or continues receiving epoch mint | **Protected.** Slash marks the validator mint-halted, removes it from active eligibility, and moves its SystemVault custody plus stake into distinct quarantine custody. No spendable balance remains at the canonical validator address until authorized adjudication. | `halted_validator_supply_not_increased`; `halted_validator_not_minted`; `SlashFlow.tla`; SystemVault/quarantine domain-separation theorems | PoS slash deploy, SystemVault protocol quarantine, active-set transition, and slash/replay regressions |
| TM-CA-157 | `CA-REPLAY` | **T + E** | Redemption is unauthorized, replayed, or credits more custody than was quarantined | **Protected.** PoS adjudication and system authority resolve one quarantine record. Vindication restores the recorded amount, partial guilt splits that same amount, and total guilt removes it; no path can resolve a consumed quarantine twice. | quarantine-inclusive conservation and mint-halt theorems; `credit_implies_not_halted`; slash lifecycle TLA+ models | `RedeemDeploy`; SystemVault protocol resolution; quarantine-record consumption; innocent/partial/total and replay regressions |
| TM-CA-158 | `CA-REPLAY` | **T** | Fee settlement inflates supply or credits a proposer more than payers lost | **Protected.** The paper's collect-then-convert trace is linearized over fungible native custody into one atomic transfer from certified payer reservations to the proposer's canonical SystemVault. Total payer debit equals proposer credit, and no mint or epoch conversion occurs. | `fee_transfer_conserves`; `fee_recipient_credit_eq_client_debit`; `native_fee_credit_is_backed`; `bounded_fee_transfer_conserved_or_rejected`; `VaultBackedCostLifecycle.tla` | `fee_allocation` certificate, SystemVault settlement, checked arithmetic, proposer-recipient and play/replay tests |
| TM-CA-159 | `CA-REPLAY` | **T + E** | A forged Exchange call creates native custody or bypasses SystemVault authority | **Protected.** The blessed Exchange swaps two already-existing carrier resources 1:1 and cannot invoke SystemVault `protocolMint`. A native deposit still requires the source purse capability; a located-stack transfer still consumes the corresponding unforgeable cell. | `exchange_is_ca_step_not_amint`; `exchange_mints_nothing`; `exchange_requires_both_inputs`; `exchange_preserves_resource_multiset` | blessed Exchange two-sided rendezvous, SystemVault authorization, and carrier-conservation regressions |
| TM-CA-160 | `CA-REPLAY` | **T + R** | Play and replay derive different payer allocations, fee amount, or proposer recipient | **Protected.** Both paths recompute the exact cost allocation and the one-token fee allocation from the authenticated certificate, parent state, and block proposer. Replay rejects any altered allocation before settlement. Settlement then performs the same checked SystemVault transfer and verifies the resulting state root. | `fee_transfer_conserves`; `FeeIsCanonicalTransfer`; `ReplayMatchesCommit`; state-bound replay theorems | `certificate.fee_allocation`; `recompute_settlement_debits`; `replay_rejects_a_tampered_fee_allocation`; exact proposer-recipient and root-chain regressions |
| TM-CA-161 | `CA-CAP` | **S + T** | A deployed validator (built-in or custom) admits an unbacked communication, accepts an under-funded deploy, double-spends a linear token, slashes without authorizing evidence, or returns a schedule-dependent verdict — violating the contract the consensus surface relies on | **Protected (DR-12 — validator behavioral contract; reframed by DR-26).** **[DR-26: the `validator_contract_*` proofs below are now OPTIONAL ASSURANCE, not an enforced certificate — behavioral alignment is supplied by the compile-time type discipline (shapes); the `check-cost-accounted-rho-*` prover gates are ADVISORY by default, `CA_ENFORCE_PROOFS=1` to run the full strict gate. The contract content S1–S4/P1–P3 is retained.]** The contract obligations S1–S4 (token-presence §6.3, acceptance `Σ_s ≥ Δ_s` §7.6, linear no-double-spend / reject-both §7.7, atomic funded transaction §7.1) and P1–P3 (slash-authorization soundness, finalization safety, determinism / replay-equivalence) are each proven in TLA+, Rocq, and Lean and named by the `validator_contract_S1..S4` / `validator_contract_P1..P3` clauses (`formal/rocq/validator/theories/Contract.v`), which re-export already-proven, axiom-free obligations (S1–S4 from CostAccountedRho, P1/P2 from Slashing). This is the contract-level guarantee over the same surface the per-budget rows protect operationally (TM-CA-003 budget conservation, `uc_ca_003` signature-channel separation, TM-CA-153 acceptance gate). A custom validator re-discharges S1–S4 + P3 for its admission/decision functions and inherits P1/P2 from the fixed Rust platform shell. | `validator_contract_S1` = `fuel_gate_rejects_mismatched_token`, `validator_contract_S2` = `funding_decidable`, `validator_contract_S3` = `ll_no_double_spend_single_witness`, `validator_contract_S4`/`validator_contract_P3` = `ca_step_deterministic`, `validator_contract_P1` = `main_T9_12_stale_evidence_not_authorized`, `validator_contract_P2` = `main_T10_fork_choice_exclusion`; TLA+ `formal/tlaplus/validator/Validator.tla` (TLAPS), `RuntimeBudgetReplay.tla` `ConsumedAndVerdictScheduleIndependent`; Lean `formal/lean/Validator/Contract.lean` | `check-cost-accounted-rho-proofs.sh` re-queries each `validator_contract_*` clause's assumptions; the worked reference bundle `formal/{rocq,tlaplus,lean}/validator/`; `gate_decision_replay_determinism`, `reject_both_on_oversubscription` (Rust cross-check); see `cost-accounting-impl/workstream-e-validator-contract.md` |
| TM-CA-162 | `CA-REPLAY` | **T + E** | Threshold-placeholder wallet drain (R1-F4): a deploy keys funding to an UNSIGNED victim's pubkey wallet by listing the victim as an empty-`sig` placeholder cosigner in an M-of-N threshold envelope | **Protected (§D2.9, R1-F4).** `funding_sig` EXCLUDES empty-`sig` placeholder cosigners (`from_signed_data_threshold`); the FILTERED funder count (NOT `is_compound()`) drives the funding arity, so a 1-of-2 threshold with one real signer + one placeholder funds ONLY the real signer's `Σ⟦Ground(real_pk)⟧` — the victim's seeded wallet is never debited. Ingress `from_proto_cosigned` already verifies every non-placeholder `sig` against its `pk`, so a forger cannot present a victim's `pk` with a valid `sig` either. | (impl-level funding-key invariant: `accounting::funding_sig` filters placeholders; depends on the ingress sig-vs-pk verification) | `accounting::funding_sig` placeholder filter (`accounting/mod.rs`); test `threshold_placeholder_victim_wallet_is_never_debited` (`acceptance.rs::tests`) |
| TM-CA-163 | `CA-REPLAY` | **T + R** | Compound play/replay fork caused by checking different supply views | **Protected (§D2.9).** Proposal and replay both derive `effectiveΣ = Σ_compound + min(Σ_l, Σ_r)` from the same parent pre-state and consume it through the same residual-ledger helpers. Missing compound or component pools are zero on both paths. | `CAJoinConservation`; `EndToEndAuthority.pre_state_mismatch_rejects_context`; TLA+ `ReplayMatchesProposal` | `effective_supply_with`; `recompute_settlement_debits_with_logic`; `multi_sig_funds_balanced_over_cosigner_ground_pubkey_wallets`; `gate_decision_replay_determinism` |
| TM-CA-164 (DR-28) | `CA-REPLAY` | **T + R** | A hostile proposer includes compound demand above its effective supply | **Protected.** Replay validates every admitted deployment's certificate and re-runs the same cumulative cost-plus-fee reservation before residual-capped settlement. Any over-admitted group raises `ReplayAdmissionMismatch`; realized cost must also remain within the certified reservation. | `admit_prefix_maximal`; `certified_reservation`; TLA+ `EveryExecutedDeploymentWasFunded` | `recompute_settlement_debits_with_logic`; `compound_over_admission_rejected_on_replay`; `replay_rejects_malformed_admitted_deploy` |
| TM-CA-165 (DR-28) | `CA-REPLAY` | **T + R** | Distinct cosigner groups reuse one shared component balance | **Protected.** Proposal and replay use the same canonical, live cross-group residual ledger. Each accepted reservation immediately draws the shared residual, so later groups cannot contract the same authority token. The rule applies on every shard; missing pools are zero. | `cross_group_draw_le_supply`; `cross_group_admission_sound`; TLA+ `Inv_CrossGroupAdmissionBounded`; Sage cross-group sweep | `group_capacity`; `draw_group_from_ledger`; cross-group admission and replay tests |
| TM-CA-166 (DR-28) | `CA-REPLAY` | **T** | Single-component no-weakening over-credit (red-team, §D2.9-R2): the Split/Join closure `effective_supply_with` credited a single component's effective supply with the compound pool — `effective[s₁] = Σ_{s₁} + Σ_{s₁∘s₂}` — but a single-signature group settles ONLY on its own pool (`GroupShape::Single` draws `Σ_{s₁}`, never the compound pool). So once a compound pool `Σ⟦And(…)⟧` is provisioned, a single-sig `s₁` deploy could be admitted against `Σ_{s₁}+Σ_compound` while settlement can only draw `Σ_{s₁}` ⇒ `close_block_deploy` `checked_sub` underflow / invalid block. This is WEAKENING — consuming a compound token `s₁∘s₂` to discharge a single-`s₁` demand discards the `s₂` authority — which the paper and model forbid. Latent today (genesis seeds only per-pubkey wallets, so `Σ_compound=0` always), but a code-only outlier that would weaponize on any compound-pool provisioning. | **Protected (this fix, §D2.9-R2).** `effective_supply_with` drops the two single-component over-credit terms; only the Join term `effective[s₁∘s₂] = Σ_{s₁∘s₂} + min(Σ_{s₁},Σ_{s₂})` remains. A single component passes through at its raw balance (`effective[s₁] = Σ_{s₁}`), matching the settlement's `GroupShape::Single` own-pool-only draw EXACTLY (so the cross-group ledger's single-sig cap is its own-pool live residual). Funding a single component from a compound now requires the explicit, observable `Split` reduction that credits `Σ⟦s₂⟧` (the runtime Splitter), never a static admission credit. No-op on every current post-state (`Σ_compound=0`). | Rocq `CAJoinConservation.join_no_weakening` (axiom-free: `s₁∘s₂` carries strictly more signature atoms than `s₁`, so it cannot be discharged as `s₁` alone) — the model already proved R2; the code now matches it. Cost-Accounted Rho "Weakening Is Forbidden" (`cost-accounted-rho.tex:1175-1191`, unverified vs the canonical paper — confirm before relying) | `delta_sigma.rs::effective_supply_with` (the two single-component inserts removed); tests `effective_supply_split_join_closure_arithmetic` (s1 = Σ_s1, not Σ_s1+Σ_compound), `effective_supply_treats_absent_component_as_zero` (components unset), `single_sig_and_compound_sharing_component_bounded` |
| TM-CA-167 (DR-29/DR-36) | `CA-REPLAY` | **T** | Coalescing vault allocations or settlement entries overflows the bounded native amount type | **Protected.** Every coalescing addition and settlement total uses checked arithmetic and returns a deterministic invalid-cost-settlement error before state mutation. Genesis vault allocation uses the same bounded discipline. No path wraps or panics. | `checked_add_i64_conserved_or_rejected`; `checked_add_i64_never_wraps`; `vault_credit_conserved_or_rejected`; `bounded_settlement_conserved_or_rejected`; `bounded_fee_transfer_conserved_or_rejected` | `vault_cost_deploy` checked coalescing; `allocation_coalescing_rejects_overflow`; `settlement_coalescing_rejects_overflow`; genesis overflow regressions |
| TM-CA-168 (DR-30/DR-36) | `CA-REPLAY` | **T + R** | Circular or node-local genesis funding leaves block one unfunded or gives honest nodes different initial authority | **Protected.** Genesis deterministically folds validator/client allocations into blessed SystemVault initialization before the genesis checkpoint. Ceremony validation and historical replay rebuild those same blessed deploys and compare the resulting genesis content/root. Ordinary blocks have no genesis-funding payload or mirror write, so allocation cannot be replayed twice. | `genesis_system_vault_funding_is_exact`; `committed_genesis_system_vault_funding_is_idempotent`; `genesis_system_vault_replay_agrees`; TLA+ `GenesisCommitIsExact`, `AdmissionRequiresGenesisAgreement`, `SettlementDoesNotReapplyGenesisFunding` | `Genesis::vaults_with_protocol_funding`; blessed SystemVault genesis contracts; ceremony content comparison; direct replay, consensus replay, and first-block regressions |
| TM-CA-169 (DR-31) | `CA-GATE + CA-REPLAY + CA-RESOURCE` | **T + R + E** | Structural ambient-cost undercount, unbounded execution, and duplicate-play drift: the submitted `Par` contains a call but not a persistent continuation already resident in RSpace, so structural demand understates actual COMM cost; `unsafe_max` then lets an accepted deployment consume without a finite authority backstop. Even with correct scalar cost, running a concurrent deployment a second time without its replay witness can choose a different event interleaving and produce a different trie root. Late exact settlement or duplicate-play comparison then rejects proposer blocks, causing replay divergence, invalid-block cascades, stalled finality, and memory pressure. | **Protected (DR-31).** Production constructs a dependent state-bound witness from the authenticated merged root under authority-derived finite capacity. It records the complete processed-deploy witness for the canonical sequence, removes exhausted or underfunded candidates to a terminating fixed point, and carries the final completed execution in an opaque token bound to pre-state, exact block context, and invalid-block set. That bounded play is the committed user transition; system settlement continues from its exact post-state root, so no second unconstrained play exists. Top-level RSpace events preserve the causal witness rather than being byte-sorted. Replay independently derives the same capacity, rigs execution with the committed event witness, and checks cost, status, adjacent roots, settlement, and fee. Structural certification remains valid only for the closed fragment; proposer metadata is never trusted. | TLA+ `StateBoundAdmission` safe invariants plus structural/duplicate-play/exhaustion negative controls; `StateBoundValidatorConvergence` permits local schedules with different event sets and costs, while its context/order/local-schedule controls prove that only exact certificate replay can be accepted; Rocq `capacity_exactly_characterizes_funding`, `exhausted_execution_cannot_be_certified`, `state_bound_certificate_funds_committed_cost`, `state_bound_chain_preserves_adjacent_roots`, `admitted_costs_are_funded`, `state_bound_exact_settlement_conserves`; Sage `state_bound_fixed_point_search` | `RuntimeManager::{state_bound_cost_evidence,admit_with_state_bound_evidence,certify_state_bound_admission}`; `StateBoundAdmission`; `RuntimeOps::state_bound_cost_evidence_for_state_cosigned`; exact causal event serialization; replay finite-capacity path; ambient-cost, root-chain, envelope-substitution, context-binding, terminal-close, single-play, and exact-boundary tests |

| TM-CA-170 (DR-32) | `CA-TRACE + CA-REPLAY` | **T + R** | Atomic-COMM identity accidentally includes mutable `Produce` telemetry (`is_deterministic`, external output bytes, or failure status), or inherits producer-vector order, so play and replay assign different cost identities to the same semantic match. | **Protected.** `COMM::cost_identity` hashes only consume channel/hash/persistence, canonically sorted produce channel/hash/persistence, peeks, and repetition counts. Telemetry is excluded exactly as it is from `Produce` equality and hashing. The observer runs on the same locked semantic match in play and replay. | `AtomicCommAccounting.trigger_side_does_not_change_cost`; TLA+ `ReplayMatchesPlayAtCompletion`; DR-32 | `cost_identity_ignores_produce_telemetry`; `cost_identity_canonicalizes_producer_order`; `cost_identity_commits_repetition_count`; `observes_exactly_once_for_either_trigger_side`; replay observer rollback test |
| TM-CA-171 (DR-33) | `CA-CONSENSUS + CA-RESOURCE` | **T + R + D** | Signature-wide recovery authorization, expiry bypass, or multiple recovery proposers on one committed finalized view repeatedly re-propose one signed deploy across independently produced source blocks. Exact tombstones accumulate, an expired retry becomes objectively invalid, proposal/finality halt, and API unavailability plus memory growth cascade from the stopped shard. Suppressing non-leader heartbeat blocks can independently deadlock when the elected leader is offline. | **Protected.** Retry is authorized only when the validator-visible parent-closure occurrence projection has no active exact source, and only inside the ordinary strict proposal-height lifespan. One leader is derived from the normalized on-chain validator set for each committed finalized-height view. Transient leaders from different lagging views can prepare bounded, source-distinct retries; exact tombstones and eventual visibility make these safe. Non-leaders cannot package the retry for their view but continue heartbeat/finality progress. Missing parent, visible-source, finalized-ancestry bodies or finalized metadata fail closed. | TLA+ `DeployRecovery` asynchronous-view safe model and four expected-refutation controls; Rocq `finalized_floor_recovery_admission_correct` and `finalized_floor_recovery_leadership_correct`; DR-33 | `canonical_disposition_sets`; canonical `rejected_in_scope`; `recovered_deploy_leader`; strict backlog purge/probe/selection; surviving/all-tombstoned, proposal-height expiry, per-view leader uniqueness, fail-closed scan, and per-block rejection-count tests |
| TM-CA-172 (DR-35) | `CA-CONSENSUS + CA-RESOURCE` | **T + R + D** | Concurrent descendants observe the same exact rejected occurrence through different causal closures and serialize different valid diagnostic reasons. A reducer that requires byte-equal reasons, or uses last-writer replacement, either halts proposal/finality or makes the next block body depend on parent arrival order. | **Protected.** Exact occurrence identity authorizes suppression; reason is diagnostic. Every producer and reducer combines causes under the protocol order `duplicate_occurrence > merge_conflict > collateral_chain_drop > unspecified`. The induced join is commutative, associative, and idempotent, making equal evidence converge while retaining the strongest direct cause. | TLA+ `RejectionReasonConfluence` safe model and last-writer expected-refutation control; Rocq `finalized_floor_rejection_reason_confluence_correct`; DR-35 | Rust reason-join examples and proptests; concurrent causal-record context regression; isolated multi-validator pause/recovery convergence |
| TM-CA-173 (DR-37) | `CA-REPLAY + CA-CONSENSUS` | **T + R + D** | Replay queries the registry, SystemVault, or located funding slots through ReplayRSpace while it is rigged with the committed event log, or eagerly reads every recorded pre-state before an independent validator has materialized the block's intermediate roots. The former adds events absent from the witness; the latter makes a producer accept roots retained from proposal while peers report unknown roots. Both defects can cascade into invalid classifications, stalled finality, API failure, and resource growth. | **Protected.** For each deployment in canonical order, a separate ordinary runtime captures the immutable authority-lane purse inventory at the validator's current materialized root. ReplayRSpace consumes only that deployment's recorded causal witness and checkpoints the exact recorded post-state, thereby materializing the next deployment's pre-state before its snapshot is read. Missing or unexpected lanes and root-chain mismatches fail closed. | `ReplaySupplySnapshot.tla`; `ReplayRootMaterialization.tla`; live-query, eager-root, history-asymmetry, and replay-query expected refutations; Rocq `ReplayRootMaterialization.v`; DR-37; CA-P-191 | `ReplayPurseSnapshot`; `replay_purse_snapshot`; `ReplayRuntimeOps`; independent-validator and isolated-reporting multi-deployment regressions; ordinary Casper/checkpoint/genesis replay regressions; lifecycle-trace subset and directed SystemVault dependency regressions |
| TM-CA-174 (DR-38) | `CA-CONSENSUS + CA-REPLAY + CA-RESOURCE` | **T + R + D** | A transient maximum-cost reservation is persisted in one singleton RSpace map. Every paid branch consumes and rewrites that datum, so otherwise independent SystemVault deltas acquire a false global conflict. Honest nodes then discard a valid sibling source or fail to advance despite sufficient aggregate funds. | **Protected.** Maximum reservation, exact burn, fee transfer, and refund execute lexically inside one authenticated `SystemVault.applyCost` call and leave no reservation cell. The node checkpoint encloses located-stack removals and the call. Merge sees only durable native purse deltas and stack removals; funded aggregate effects commute and overdraw rejects without partial state. | `AtomicVaultSettlementRefinement.v`; `AtomicVaultSettlementRefinement.tla`; global-cell negative control; DR-38; CA-P-192 | `SystemVault.applyCost`; `ApplyCostDeploy`; atomic SystemVault example; 512-case request properties; same-key sibling merge regression; proposal/replay parity tests |
| TM-CA-175 (DR-39) | `CA-GATE + CA-REPLAY + CA-CONSENSUS` | **T + R + D** | Admission normalizes a deployment with an empty environment while execution installs authenticated deployer and cosigner bindings. A valid contract using `rho:system:deployerId` is rejected before certification, so ordinary deploys disappear from proposed blocks and finality/API tests cascade into timeouts. A less fail-closed mismatch could instead certify different bytes than execution. | **Protected.** `canonical_program_for_deploy` calls the same normalizer entry point and `normalizer_env_from_cosigned_deploy` construction as execution. Certification, retained play, and replay therefore resolve authenticated system references identically and bind the same canonical program hash. State-bound rejection diagnostics retain the exact failure class without changing consensus disposition. | `RuntimeBoundAuthority` authenticated-environment theorems; `NormalizerEnvironmentRefinement.tla`; empty-environment negative control; DR-39; CA-P-193 | funded deployer-ID SystemVault checkpoint/replay regression; workspace Clippy; ordinary state-bound proposal/replay suite |
| TM-CA-176 (DR-40) | `CA-RESOURCE + CA-CONSENSUS` | **D + R** | A valid deployment creates a long realized authority-event trace. Recursive exact-allocation proof search consumes one or more native frames per event and aborts the node process when trace depth exceeds the host worker stack, even though authority and cost are finite and fully funded. | **Protected.** The exact same canonically ordered depth-first search runs on a heap worklist. Reverse candidate insertion preserves recursive first-choice order; delayed failure markers preserve memo semantics; a persistent draw chain avoids prefix copying. Event count no longer increases native recursion depth. | `PhysicalSettlementWorklist.v`; `PhysicalSettlementWorklist.tla`; recursive bounded-stack refutation; DR-40; CA-P-194 | 4,096-event stack-safety regression; generated mixed-event exact-debit/order property; unchanged high-fanout play/replay and 48-test runtime-manager sweep |

**TM-CA-151 — guarded invariants.** Schedule-independence of `total_cost`
rests on the semantic RSpace boundary. A complete match is selected while the
relevant channel group is locked; the observer reserves one unit before the
COMM log or tuplespace mutation; rejection preserves both. Producer-triggered
and consumer-triggered discovery construct the same `COMM::cost_identity`, and
that identity excludes reducer paths, local indices, and mutable external-call
telemetry. Play and replay install the same observer. Canonical budget folding
therefore operates on successful semantic COMMs, not on send/receive
introductions.

**TM-CA-151 — faithfulness note (verified against
`publications/cost-accounting/cost-accounted-rho.tex`).** Dropping the
digest from consensus does **not** affect cost-accounting correctness or
faithfulness to the paper. The paper formalizes cost as token-gated COMM
(signatures → channels, tokens → messages, fuel-before-communication;
Appendix A), with faithfulness = operational bisimulation + capability
security (§4 and §5) and token conservation (Rules 1–5, §3.6). It
contains **no per-operation cost-trace or digest concept.** The runtime correlate of
the paper's cost is `total_cost` (= the conserved token total, clamped on
OOP), which remains consensus-checked; the mechanized token-conservation
and total-cost-determinism results (see
[`cost-accounted-rho-verification.md`](cost-accounted-rho-verification.md),
Rocq `ca_cost_deterministic`) do not reference the digest. The runtime
observer is a direct refinement of the paper's COMM-token granularity.
Diagnostic reducer events remain below that boundary and outside consensus;
removing their digest from consensus brings the consensus surface back to the
paper's cost granularity rather than away from it.

**TM-CA-151 — rejected alternatives (considered and not adopted).** Three
designs that would have kept a per-operation commitment in consensus were
evaluated and rejected:

- **Homomorphic multiset-hash digest.** A commutative (multiset)
  hash would make the digest order-invariant for the *non-OOP* case, but
  it does not remove the OOP hazard: the *committed set itself* differs
  across schedules when a fork unwinds at a schedule-dependent point, so
  an order-invariant hash of a schedule-dependent set is still
  schedule-dependent. It also adds a new cryptographic primitive to the
  consensus trust base for a quantity the paper does not model. Rejected:
  it does not close the OOP case and enlarges the consensus surface.
- **Scope-keyed identity.** Keying each event by its metering scope was
  considered as a way to canonicalize the committed set independently of
  fork-unwind order. It does not make the OOP committed *set* identical
  across schedules (which events survive the unwind is still
  schedule-dependent), and it pushes additional structure into the
  consensus commitment. Rejected as unnecessary once the digest is not a
  consensus quantity.
- **Digest-encoding migration.** A staged migration of the on-wire digest
  encoding (e.g. a versioned or canonicalized field) was considered to
  preserve a consensus digest across the change. Because
  `cost_trace_digest`/`cost_trace_event_count` are **unreleased** (absent
  from `master` and tag `v0.4.15`; present only on this branch), there is
  zero migration cost to simply removing them; a migration would add
  gating machinery to retain a field that is not needed for consensus.
  Rejected as gratuitous given the unreleased status.

The `RuntimeBudget::cost_trace_digest()` /
`cost_trace_event_count()` / `last_oop_event()` methods remain callable as
diagnostics; they are simply no longer stored on `ProcessedDeploy`,
compared in replay, or folded into the block hash.

**TM-CA-151 — Rocq scoping (item 2509 naming).** To keep the Rocq model's scope
explicit, the digest-inclusive replay mode `RbCostAccountedReplay` is renamed
`RbDiagnosticRefinement` (with obligations `rb_diagnostic_refinement_requires_commitment`
/ `rb_diagnostic_refinement_rejects_absent_commitment`), since the digest is a
*strictly-finer diagnostic refinement*, not a production consensus requirement. The
full replay-payload equivalence `rb_full_replay_payload_equiv` is **split** into a
consensus-core half `rb_full_replay_payload_consensus_equiv` (RSpace events +
`consumed_units` + status — what production replay compares) and a diagnostic-only
digest half `rb_full_replay_payload_diagnostic_equiv` (the cost-trace fields), proven
equal to their conjunction (`rb_full_replay_payload_equiv_split`);
`rb_full_replay_payload_consensus_coarser_than_full` witnesses that two payloads
agreeing on the entire consensus surface stay consensus-equivalent even when their
digests differ. The theorem *content* is unchanged — only the scope naming is
clarified. Recorded in [DR-29](cost-accounting-decision-records.md).

## 6. Classification Policy

Every generated Sage, TLA+, fuzz, Kani, or Rust replay witness must be
classified before it can motivate a source change:

| Class | Meaning | Action |
|---|---|---|
| `confirmed_safe` | The searched behavior is protected by the current source, proof, or replay oracle. | Keep as regression or coverage evidence. |
| `bisimilar` | Model and production projection agree. | Record or promote as a replay fixture. |
| `projection_risk` | A bounded model-to-code projection can diverge while production remains guarded. | Add or retain a guarded-safe regression. |
| `assumption_counterexample` | A theorem precondition is necessary. | Keep the assumption explicit in proof and docs. |
| `proof_or_model_strengthening` | The property is true but underrepresented in formal artifacts. | Promote to a theorem, invariant, or model check. |
| `needs_source_audit` | The witness touches production behavior, but reproduction is inconclusive or policy-driven. | Audit before changing source. |
| `confirmed_current_bug` | The witness reproduces on production Rust or violates a production invariant. | Fix Rust and add deterministic regression coverage. |

No generated witness may remain unclassified after promotion, and no source
change is authorized by model-only evidence.

## 7. Failure Modes and Recovery

| Failure | Required behavior |
|---|---|
| Cost scalar mismatch | Reject replay with cost-invalid evidence; do not alter settlement arithmetic. |
| Cost trace digest mismatch | Report diagnostic drift; consensus replay is decided by exact cost, status, causal event witness, roots, and settlement. |
| Cost trace count mismatch | Report diagnostic drift; never substitute the count for authenticated causal events. |
| Missing diagnostic cost trace | Preserve consensus behavior and report the missing telemetry. |
| Out-of-phlogiston rollback | Reject before logging or mutating the triggering COMM; an exhausted proof cannot authorize admission. |
| Unauthorized fee settlement | Classify as cost-invalid evidence; system deploy authority remains required. |
| Low deploy price | Classify as cost-invalid evidence before treating execution as cost-valid. |
| Stale cost-invalid evidence | Reject at the slashing boundary; canonical candidate scanning requires present evidence, exact-current evidence and target activation epochs, and positive canonical pre-state bond. |
| Ambient-only slash authorization | Reject unless the parent pre-state bond is positive. |
| Slash target epoch mutation | Reject through replay-payload authentication. |
| Diagnostic log truncation | Leave consensus cost, causal event witness, roots, settlement, remaining fuel, and OOP outcome unchanged. |
| Zero-weight billable event | Reject before appending to the cost trace or consuming tokens. |
| Oversized weight | Reject before appending to the cost trace or consuming tokens. |
| Oversized descriptor or trace window | Reject before appending to replay evidence or consuming tokens. |
| Worker race at OOP boundary | Linearize reservation under the RSpace match lock; the rejected match commits no semantic event or state mutation. |
| State-bound proof exhausts authority capacity | Reject the candidate, remove it from the retained sequence, and recompute later evidence; never issue an admission token from clamped cost. |
| Proof/commit/replay root, cost, status, or event drift | Reject before close settlement; do not record the mismatch as locally manufactured slash evidence. |
| Missing, extra, or lane-mismatched replay supply snapshot | Reject before trace replay; never issue a live SystemVault or funding-slot query through ReplayRSpace. |

## 8. Tier Architecture

| Tier | Role | Artifacts |
|---|---|---|
| Formal specification | Proves unbounded semantic invariants and bounded replay/threat state machines | Rocq theories, `RuntimeBudgetReplay.tla`, `CostAccountingThreats.tla` |
| Oracle and harness | Exercises generated terms, replay fixtures, concurrency races, and digest mutation campaigns | Rust property tests, loom shadow model, TLA+ model checking |
| Production | Enforces invariants at runtime, replay, block hashing, settlement, and slashing boundaries | `RuntimeBudget`, `RuntimeManager`, Casper processed deploys, replay cache, block hash/signature payloads |

## 9. Security Conclusions

The cost-accounted model is protected against the practical security
vectors that follow from moving cost from an external RSpace wrapper into
the calculus:

- Runtime fuel is capability-scoped and cannot be minted by refund or
  slashing effects.
- Replay authenticates consensus cost, failure status, complete cosigned
  envelope, system-deploy kind, slashing fields, slash target activation epoch,
  event logs, state-root chain, settlement, fee carve, and genesis mode.
- Parallel evaluation preserves exact atomic-COMM cost and the authenticated
  causal event witness; scheduler-local telemetry never defines consensus
  identity.
- Replay supply is captured from ordinary RSpace at each authenticated
  deployment pre-state; ReplayRSpace performs no live authority lookup that
  could contaminate the committed causal witness.
- Production accepts protocol 2 only. Retired charging and replay
  representations remain absent from the consensus path and cannot select an
  accounting-off behavior.
- Slashing consumes current cost-invalid evidence as a post-evaluation
  system effect, uses parent pre-state bond authorization, and does not
  mutate user fuel or settlement inputs.
- Resource-exhaustion vectors are bounded by oversized-event rejection,
  diagnostic/non-consensus separation, the production trace-event cap, and
  deploy-reset trace clearing after replay commitment recording. Every committed
  user execution and constrained replay also has a finite authority-derived
  capacity; exhaustion is non-certifiable.

The remaining trust base is cryptographic collision resistance,
signature validity, the independently verified slashing authorization
suite, and faithful execution of the Rust production paths tested by the
implementation harness.
