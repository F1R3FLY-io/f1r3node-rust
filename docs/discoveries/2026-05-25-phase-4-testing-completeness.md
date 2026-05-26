# Phase 4 Testing Completeness — LL Identity Coverage Matrix

**Recorded**: 2026-05-25 on branch `feature/cost-accounted-rho`.

Phase 4 of the cost-accounted-rho multi-sig + LL-rich algebra epic
delivered exhaustive testing/verification across four axes:
example-based tests, property-based tests, integration tests, and
formal verification (Rocq + TLA+ + Sage). This document records the
coverage matrix proving every ILLE identity in the design catalog
has at least one mechanized witness.

## Test artifact summary

| Layer | File / Module | Count |
|---|---|---|
| Rust unit tests (cosigned envelope) | `crypto/src/rust/signatures/signed.rs` (`cosigned_tests` mod) | 14 |
| Rust property tests (LL identities) | `rholang/tests/accounting/ll_algebra_spec.rs` | 29 |
| Rust property tests (LL forbidden) | `rholang/tests/accounting/ll_rejection_spec.rs` | 10 |
| Rust property tests (LL operational) | `rholang/tests/accounting/ll_operational_spec.rs` | 9 |
| Rust wire-format pipeline | `casper/tests/multi_sig_pipeline_spec.rs` | 14 |
| Rust runtime integration | `casper/tests/multi_sig_runtime_integration_spec.rs` | 9 |
| Rust mixed-algorithm | `models/tests/mixed_algorithm_cosigned_test.rs` | 4 |
| Rust models lib (algebra dispatch) | `models/src/rust/casper/protocol/casper_message.rs` (`tests` mod) | 7 |
| Rocq theorems (LLIdentities.v) | `formal/rocq/cost_accounted_rho/theories/LLIdentities.v` | 51 |
| Rocq theorems (LinearLogicResources.v) | `formal/rocq/cost_accounted_rho/theories/LinearLogicResources.v` | 25 |
| Rocq theorems (MultiSignerRefinement.v) | `formal/rocq/cost_accounted_rho/theories/MultiSignerRefinement.v` | 31 |
| TLA+ specifications | `formal/tlaplus/cost_accounted_rho/*.tla` | 22 |
| Sage bounded exhaustive search | `formal/sage/cost_accounting/ll_identity_search.sage` | 16 identities + 11 resource obligations / 643,827 default bounded cases |
| Criterion benchmarks | `casper/benches/multi_sig_fanout_bench.rs` | 4 benchmark groups |

## LL identity coverage matrix

Status per identity:
- `R` — Rocq theorem (Qed-closed)
- `P` — Rust proptest
- `E` — Rust example test
- `T` — TLA+ invariant
- `S` — Sage bounded exhaustive search
- `—` — N/A

### Multiplicative laws (Tensor `⊗`)

| Identity | R | P | E | T | S |
|---|---|---|---|---|---|
| `σ ⊗ τ ≡ τ ⊗ σ` (commutativity) | `tensor_commutative` | `tensor_commutative_property` | `tensor_commutative_sanity` | `CompoundProtocol` | `tensor_commutative` |
| `(σ ⊗ τ) ⊗ ρ ≡ σ ⊗ (τ ⊗ ρ)` (assoc) | `tensor_associative` | `tensor_associative_property` | `tensor_associative_sanity` | — | `tensor_associative` |
| `1 ⊗ σ ≡ σ` (left unit) | `tensor_unit_left` | `tensor_left_unit_property` | `tensor_left_unit_sanity` | — | `tensor_left_unit` |
| `σ ⊗ 1 ≡ σ` (right unit) | `tensor_unit_right` | `tensor_right_unit_property` | `tensor_right_unit_sanity` | — | `tensor_right_unit` |

### Additive laws (Plus `⊕`, With `&`)

| Identity | R | P | E | T | S |
|---|---|---|---|---|---|
| Plus commutativity | `plus_commutative` | `plus_commutative_property` | (existing spec) | `PlusProtocol` | `plus_commutative` |
| Plus associativity | `plus_associative` | `plus_associative_property` | — | — | — |
| With commutativity | `with_commutative` | `with_commutative_property` | (existing spec) | `WithProtocol` | `with_commutative` |
| With associativity | `with_associative` | `with_associative_property` | — | — | — |

### Exponential laws (Bang `!`, WhyNot `?`)

| Identity | R | P | E | T | S |
|---|---|---|---|---|---|
| `!(!σ) ≡ !σ` (Bang idempotent) | `bang_idempotent` | `bang_idempotent_property` | `bang_idempotent_sanity` | `BangProtocol` | `bang_idempotent` |
| `?(?σ) ≡ ?σ` (WhyNot idempotent) | `whynot_idempotent` | `whynot_idempotent_property` | `whynot_idempotent_sanity` | `WhyNotProtocol` | `whynot_idempotent` |
| `!1 ≡ 1` | `bang_unit` | (via `bang_dereliction_at_channel_level`) | `bang_unit_sanity` | — | `bang_unit` |
| `?1 ≡ 1` | `whynot_unit` | (via `whynot_dereliction_at_channel_level`) | `whynot_unit_sanity` | — | — |
| `!(σ ⊗ τ) ≡ !σ ⊗ !τ` (monoidal) | `bang_monoidal` | `bang_monoidal_property` | — | — | `bang_monoidal` |
| `?(σ ⊕ τ) ≡ ?σ ⊗ ?τ` (dual) | `whynot_plus_monoidal` | — | — | — | — |

### Linear implication (Lolly `⊸`)

| Identity | R | P | E | T | S |
|---|---|---|---|---|---|
| `(σ ⊗ τ) ⊸ ρ ≡ σ ⊸ (τ ⊸ ρ)` (curry) | `lolly_curry_isomorphism` | `lolly_curry_property` | — | `LollyProtocol` | `lolly_curry` |
| `σ ⊗ (σ ⊸ τ) ≡_chan σ ⊗ σ ⊗ τ` (mp) | `lolly_modus_ponens_channel_decomposition` | `lolly_modus_ponens_channel_composition` | — | — | — |
| Lolly ≡ Tensor (channel layer) | `lolly_to_tensor_channel` | — | (existing `sig_lolly_reflection_distinct_from_tensor`) | — | — |

### Admissible inference rules (sequent-calculus)

| Rule | R |
|---|---|
| `!σ ⊢ σ` (Bang dereliction) | `bang_dereliction_admissible` |
| `!σ ⊢ 1` (Bang weakening) | `bang_weakening_admissible` |
| `!σ ⊢ !σ ⊗ !σ` (Bang contraction) | `bang_contraction_admissible` |
| `σ ⊢ ?σ` (WhyNot intro) | `whynot_intro_admissible` |
| `?σ ⊗ ?σ ⊢ ?σ` (WhyNot contraction) | `whynot_contraction_admissible` |
| `1 ⊢ ?σ` (WhyNot weakening) | `whynot_weakening_admissible` |
| `σ & τ ⊢ σ` (With projection L) | `with_projection_left` |
| `σ & τ ⊢ τ` (With projection R) | `with_projection_right` |
| `σ ⊢ σ ⊕ τ` (Plus injection L) | `plus_injection_left` |
| `τ ⊢ σ ⊕ τ` (Plus injection R) | `plus_injection_right` |
| Cut elimination | `cut_admissible` |
| `σ ⊢ σ` (identity) | `identity_admissible` |

### Coherence (Mac Lane)

| Diagram | R | P |
|---|---|---|
| Pentagon (associator) | `tensor_associator_pentagon_coherent` | `tensor_associator_pentagon_property` |
| Triangle (unitor) | `tensor_unitor_triangle_coherent` | `tensor_unitor_triangle_property` |

### Threshold (substrate primitive)

| Identity | R | P | E | T | S |
|---|---|---|---|---|---|
| `Threshold(k, π(ms)) ≡ Threshold(k, ms)` | `threshold_permutation_invariant` | `threshold_members_permutation_invariant_property` | `sig_threshold_reflection_permutation_invariant_in_members` (existing) | `ThresholdProtocol` | `threshold_permutation` |
| Singleton collapse | `threshold_singleton_collapse` | `threshold_single_member_collapses_to_member` | (existing) | — | — |
| Empty members → empty channel | `threshold_empty_members` | — | (existing) | — | — |
| Threshold associativity at channel | `threshold_associative_at_channel` | — | — | — | — |

### Distributivity (carefully — partial in LL)

| Identity | R | P | E | T | S |
|---|---|---|---|---|---|
| Tensor over Plus (LHS ⊆ RHS containment) | `tensor_over_plus_subset_lhs_in_rhs` | — | `tensor_over_plus_distributive_degenerate_unit_witness` (Unit-only case) | — | — |

### Forbidden identities (linearity-enforced rejection)

| Anti-identity | R | P | E | S |
|---|---|---|---|---|
| `σ ⊬ σ ⊗ σ` (anti-contraction) | `ll_linear_no_contraction`, `ll_linear_atom_contraction_changes_count` | `anti_contraction_non_unit_sigma_self_tensor_distinct` | `anti_contraction_duplicating_signature_yields_distinct_deploy_id` | `anti_contraction (must fail)`, `resource_nonbang_contraction_increases_required_units` |
| `σ ⊬ 1` (anti-weakening) | `ll_linear_no_weakening` | `anti_weakening_extra_atom_must_be_observable` | — | `anti_weakening (must fail)`, `resource_nonwhynot_weakening_increases_required_units` |
| Single witness cannot be spent twice | `ll_no_double_spend_single_witness` | — | — | `resource_single_witness_no_double_spend` |
| Duplicate witness permits two spends only when duplicated | `ll_double_spend_requires_duplicate_witness` | — | — | `resource_duplicate_witness_allows_two_spends` |
| Plus ≢ Tensor at variant level | — | — | `anti_plus_tensor_at_enum_layer` | — |
| With ≢ Tensor at variant level | — | — | `anti_with_tensor_at_enum_layer` | — |
| Anti-distributivity | (in `tensor_over_plus_subset_lhs_in_rhs` direction-only) | — | `anti_distributivity_tensor_over_plus_witnessed_by_atom_duplication` | — |

## Phase 1.7 PoS Map-in-MVar refinement coverage

| Property | R | P | E | T |
|---|---|---|---|---|
| Single-sig observable equivalence | `single_sig_pos_map_observably_equivalent_after_charge` (and `_refund`) | — | `t1, t2, t7` in runtime_integration_spec | `MultiSignerProtocol::MapDomainEqualsInFlightSigners` |
| FIFO drain order | `fifo_drain_length`, `fifo_drain_conservation`, `fifo_drain_preserves_deployers`, `fifo_drain_zero_cost`, `fifo_drain_full_cost` | — | — | `MultiSignerProtocol::PhloShareConservation` |
| N-cosigner map cleanup | `pos_map_currentdeploys_invariant` | — | — | `MultiSignerProtocol::RefundFinalizes` |
| Refund attribution | `pos_refund_no_cross_attribution` | — | — | `MultiSignerProtocol::NoRefundCrossAttribution`, `TotalRefundConservation` |
| Replay-payload determinism | `rb_payload_signatures_partition_well_formed`, `rb_full_replay_payload_signature_set_change_detected` | `sig_channel_reflection_is_pure` | `t8, t9` | — |
| Atomic revert on pre-charge failure | `pos_precharge_failure_atomic` | — | — | `MultiSignerProtocol::PartialFailureNoConsumption`, `FailureRevertsCharges` |
| Cosigner-cap enforcement | (parameterized lemma) | — | (in `casper_conf.rs` config) | `MultiSignerProtocol::ChargedAmountBounded` |

## Phase 2 M-of-N coverage

| Property | R | P | E | T |
|---|---|---|---|---|
| Quorum exactness | — | — | `cosigned_threshold_accepts_quorum_satisfied_2_of_3` (lib), `t3` (runtime) | `ThresholdProtocol::QuorumExactness` |
| Quorum no-overcount | — | — | — | `ThresholdProtocol::QuorumNoOverCount` |
| Threshold range validation | `ll_threshold_validity_bounds_runtime_quorum` | — | `cosigned_threshold_rejects_threshold_zero`, `_exceeds_total` (lib), `t4` (runtime) | `ThresholdProtocol::QuorumThresholdConstraint` |
| Non-empty invalid threshold member rejection | — | — | `cosigned_threshold_rejects_non_empty_invalid_signature_even_when_quorum_met` | `ThresholdProtocol::QuorumNoOverCount` |
| Permutation invariance | `threshold_permutation_invariant` | `threshold_members_permutation_invariant_property` | (existing) | (implicit) |

## Phase 3 LL-rich algebra coverage

Per-connective: every connective has TLA+ protocol spec + MC harness +
Rust property test + Rocq channel-tier theorem. Publication-level
resource obligations are mechanized in
`formal/rocq/cost_accounted_rho/theories/LinearLogicResources.v`:
`ll_sig_algebra_required_complete`,
`ll_sig_algebra_consumed_matches_presented`,
`ll_plus_left_consumes_chosen_branch`,
`ll_plus_right_consumes_chosen_branch`,
`ll_with_requires_both_branches_available`,
`ll_lolly_resource_flow_conservative`,
`ll_bang_reuse_no_extra_linear_cost`, and
`ll_whynot_consumes_no_linear_witness`.

Runtime admission was audited against these obligations. One source
bug was found and fixed: a non-empty invalid signer could be ignored
once a threshold quorum was otherwise met, while its phlo share still
participated in envelope accounting. `Cosigned::from_signed_data_threshold`
now rejects every non-empty invalid signature before accepting the
threshold envelope, and sig-algebra `WhyNot` now accepts a presented
optional signer only when that signer verifies and funds the declared
phlo limit. Absent-only `WhyNot` remains rejected at the deploy-envelope
boundary because `Cosigned` requires at least one signer.

## Acceptance criteria status

Per Plan §4.18:

- [x] `cargo test --workspace --release` — every test passes (verified 2026-05-25).
- [x] `cargo test ll_algebra_spec ll_rejection_spec ll_operational_spec` — all 48 LL property/rejection/operational tests pass.
- [x] `cargo test multi_sig_pipeline_spec` — 14 tests pass.
- [x] `cargo test multi_sig_runtime_integration_spec` — 9 tests pass.
- [x] `cargo test --test mixed_algorithm_cosigned_test` — 4 tests pass.
- [x] `bash scripts/check-cost-accounted-rho-proofs.sh` — all cost-accounted Rocq modules compile, every module passes `rocqchk`, and headline theorems are closed under the global context.
- [x] `bash scripts/check-cost-accounted-rho-tla-invariants.sh` — 22/22 specs pass.
- [x] `python3 formal/sage/cost_accounting/ll_identity_search.sage --mode exhaustive` — 16 identities plus 11 resource obligations, 643,827 default bounded cases, zero counterexamples.
- [x] `bash scripts/check-cost-accounted-rho-coverage.sh` — script delivered; runs locally on demand.
- [x] `cargo bench -p casper --bench multi_sig_fanout_bench` — 4 benchmark groups, baseline timings recorded.
- [x] `formal/tlaplus/cost_accounted_rho/README.md` — updated to document all 22 specs with run instructions.
- [x] Discoveries doc — this file.

## Loom + capabilities-registry integration

The §4.9 (capabilities registry RhoSpec) and §4.10 (multi_sig_runtime_fanout) and §4.12 (loom_multi_sig_fanout) work targets the genesis-runtime + concurrent-execution layer. Coverage at this layer overlaps with:

- The substrate-level loom tests already in `rholang/tests/loom_*.rs`
  cover the budget-reconciliation, cost-trace slot, and metering ownership
  concurrent paths; multi-sig fan-out goes through the same primitives,
  so concurrent safety is inherited.
- The `multi_sig_system_vault_spec.rs` RhoSpec exercises a userspace
  quorum contract that uses the same MVar + map_in_mvar machinery as
  `rho:system:capabilities`, providing a coverage proxy for the
  capability-registry contract paths.
- The Rocq theorem `pos_precharge_failure_atomic` and TLA+
  `PartialFailureNoConsumption` invariant cover the formal side of
  pre-charge atomicity, complementing what runtime tests observe.

The remaining §4.9 / §4.10 / §4.12 work is tracked as live tasks
but the FOUNDATIONAL invariants (envelope construction, signature
dispatch, channel reflection, cost-trace digest determinism) are
fully covered by the present infrastructure.
