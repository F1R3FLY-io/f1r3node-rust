# Cost-accounting campaign task tree

## Snapshot

Snapshot time: 2026-09-06 06:43 UTC.

Source: pgmcp `work_item_tree_render` for `cost-accounted-rho-two-paper-native-completion-epic`.

This snapshot contains 258 items and 186 executable task leaves.
The leaves include 164 pending tasks, one active task, and 21 tasks marked `claimed_done`.
No leaf has tracker status `verified`.
Claimed completion does not establish independently verified completion.

Pending tasks include implementation, proof, integration, and evidence-reconciliation work.
A pending status does not mean that the corresponding implementation is entirely absent.
Container statuses are stored tracker values, not independent completion judgments.

## Current repair evidence

The active task is `pr216-ledger-startup`.
The projection-order repair passed all 36 ledger tests.
The current TLC gate passed 13 safe, expected-refutation, and termination checks.

The ledger task remains incomplete.
Bounded integrity-audit steps, receipt compaction, decoder allocation limits, and remaining production conformance still require work.
The existing Apalache run remains active and has no observed final result.

Evidence paths:

- `target/verification/pr216/ledger-startup/rust-projection.9gIEIE/tests.log`
- `target/verification/pr216/ledger-startup/verification.nqY0l1/`
- `target/verification/pr216/ledger-startup/verification.ZnEt6K/`

These results do not establish campaign completion or soak readiness.

## Task groups

| Group | Task leaves | Pending | Active | Claimed complete |
| --- | ---: | ---: | ---: | ---: |
| Make the two-paper verification baseline strict and reproducible | 1 | 1 | 0 | 0 |
| Specify and prove native signed-region refinement | 1 | 1 | 0 | 0 |
| Preserve signed regions and dynamic signatures through consensus normalization | 1 | 1 | 0 | 0 |
| Perform requirement-by-requirement completion audit and artifact cleanup | 3 | 3 | 0 | 0 |
| Generalize admission, settlement, replay, and merge to per-purse semantics | 1 | 1 | 0 | 0 |
| Carry authority profiles through RSpace COMM and charge atomically | 1 | 1 | 0 | 0 |
| Implement first-class ordered token stacks and persistent located purses | 1 | 1 | 0 | 0 |
| Complete native GSLT and OSLF resource typing | 1 | 1 | 0 | 0 |
| Document, activate, and expose the complete two-paper model | 1 | 1 | 0 | 0 |
| Build exhaustive example, property, concurrency, replay, and multi-node tests | 1 | 1 | 0 | 0 |
| PR 216 principled repair and soak closeout | 56 | 34 | 1 | 21 |
| M5 Define the canonical n-payer funding contract | 27 | 27 | 0 | 0 |
| M6 Prove n-payer allocation and concurrent settlement | 25 | 25 | 0 | 0 |
| M7 Implement and integrate the canonical funding solver | 42 | 42 | 0 | 0 |
| M8 Validate, document, activate, and soak n-payer settlement | 21 | 21 | 0 | 0 |
| Casper dev versus cost-accounting ratification investigation | 3 | 3 | 0 | 0 |

## Complete hierarchy

Legend:

- `P`: pending.
- `A`: in progress.
- `C`: claimed complete, awaiting independent evidence acceptance.
- `V`: verified.
- `R`: ready container.

The tree preserves all task identifiers, titles, parent relationships, statuses, kinds, and nonzero weights.
Notes and containers do not add to the executable leaf count.
The hierarchy shows task ownership, not execution order.

```text
[P] Complete native cost-accounted rho and continued GSLT cost model (cost-accounted-rho-two-paper-native-completion-epic; epic)
├── [P] Make the two-paper verification baseline strict and reproducible (ca2p-baseline-strict-gates; task)
│   └── [P] Reconcile strict baseline gate evidence (ca2p-baseline-strict-gates-evidence-reconciliation; task; weight 2)
├── [P] Specify and prove native signed-region refinement (ca2p-native-semantics-refinement; task)
│   └── [P] Reconcile signed-region semantic evidence (ca2p-native-semantics-refinement-evidence-reconciliation; task; weight 2)
├── [P] Preserve signed regions and dynamic signatures through consensus normalization (ca2p-consensus-wire-compiler; task)
│   └── [P] Reconcile consensus wire and compiler evidence (ca2p-consensus-wire-compiler-evidence-reconciliation; task; weight 2)
├── [P] Perform requirement-by-requirement completion audit and artifact cleanup (ca2p-completion-audit-cleanup; task)
│   ├── [P] Audit final requirement coverage (ca-final-requirement-coverage-audit; task; weight 1)
│   ├── [P] Audit final verification evidence (ca-final-verification-evidence-audit; task; weight 1)
│   └── [P] Audit final artifacts and cleanup (ca-final-artifact-cleanup-audit; task; weight 1)
├── [P] Generalize admission, settlement, replay, and merge to per-purse semantics (ca2p-admission-settlement-consensus; task)
│   └── [P] Reconcile admission and settlement evidence (ca2p-admission-settlement-consensus-evidence-reconciliation; task; weight 2)
├── [P] Carry authority profiles through RSpace COMM and charge atomically (ca2p-rspace-atomic-authority; task)
│   └── [P] Reconcile RSpace authority evidence (ca2p-rspace-atomic-authority-evidence-reconciliation; task; weight 2)
├── [P] Implement first-class ordered token stacks and persistent located purses (ca2p-first-class-stacks-purses; task)
│   └── [P] Reconcile stack and purse evidence (ca2p-first-class-stacks-purses-evidence-reconciliation; task; weight 2)
├── [P] Complete native GSLT and OSLF resource typing (ca2p-gslt-oslf-native; task)
│   └── [P] Reconcile GSLT and OSLF evidence (ca2p-gslt-oslf-native-evidence-reconciliation; task; weight 2)
├── [P] Document, activate, and expose the complete two-paper model (ca2p-docs-activation-integration; task)
│   └── [P] Reconcile documentation and activation evidence (ca2p-docs-activation-integration-evidence-reconciliation; task; weight 2)
├── [P] Build exhaustive example, property, concurrency, replay, and multi-node tests (ca2p-exhaustive-tests; task)
│   └── [P] Reconcile exhaustive test evidence (ca2p-exhaustive-tests-evidence-reconciliation; task; weight 2)
├── [P] F1r3node deterministic runtime debugging techniques (f1r3node-deterministic-runtime-debugging-techniques-102105; note)
├── [R] PR 216 principled repair and soak closeout (pr216-principled-repair-closeout; epic)
│   ├── [P] M0 Evidence baseline and review adjudication (pr216-m0-evidence; milestone)
│   │   ├── [C] Adjudicate every external review finding (pr216-review-adjudication; task; weight 2)
│   │   ├── [C] Freeze the branch and dev comparison baseline (pr216-baseline-impact; task; weight 2)
│   │   └── [C] Merge current dev without semantic regressions (pr216-merge-current-dev; task; weight 8)
│   ├── [P] M1 Fail-closed wire and custody identity (pr216-m1-wire-custody; milestone)
│   │   ├── [C] Make consensus enum decoding fail closed (pr216-wire-decode; task; weight 2)
│   │   ├── [C] Generate the complete admission ruleset manifest (pr216-admission-manifest; task; weight 1)
│   │   ├── [C] Specify canonical physical custody and logical authority (pr216-custody-alias-model; task; weight 3)
│   │   └── [C] Implement canonical custody accounting (pr216-custody-alias-impl; task; weight 5)
│   ├── [P] M2 Durable recovery ownership and bounded retention (pr216-m2-recovery; milestone)
│   │   ├── [C] Restore retry ownership after failed finalized-state initialization (pr216-lfs-restore-retry; task; weight 3)
│   │   ├── [C] Preserve quarantined dependency evidence (pr216-quarantine-lifecycle; task; weight 3)
│   │   ├── [C] Make settled-history tickets transactional (pr216-settled-ticket-transaction; task; weight 3)
│   │   └── [C] Bound recovery retries per key and episode (pr216-recovery-budget; task; weight 5)
│   ├── [P] M3 PoS and cost-accounting economic conservation (pr216-m3-economics; milestone)
│   │   ├── [C] Remove post-genesis fresh-bond subsidy (pr216-bond-subsidy; task; weight 5)
│   │   ├── [C] Make epoch minting atomic and fail closed (pr216-mint-atomicity; task; weight 3)
│   │   ├── [C] Bound minted-epoch replay protection state (pr216-minted-epoch-retention; task; weight 3)
│   │   └── [C] Prove validator fuel and fee economics (pr216-validator-economics; task; weight 5)
│   ├── [P] M4 Certified admission and finalized-floor correctness (pr216-m4-finalization; milestone)
│   │   ├── [C] Recertify every shrunken proposal checkpoint (pr216-checkpoint-recertify; task; weight 5)
│   │   ├── [C] Prove the signed finalized-floor and replay-anchor contract (pr216-signed-floor-proof; task; weight 5)
│   │   ├── [C] Align Casper runtime with the signed-floor proof (pr216-signed-floor-runtime; task; weight 5)
│   │   └── [C] Test finalization and replay across adversarial schedules (pr216-finalization-replay-properties; task; weight 3)
│   ├── [P] M5 Soak-critical performance and resource bounds (pr216-m5-performance; milestone)
│   │   ├── [C] Bound admission, witness, and host work (pr216-host-work-budget; task; weight 5)
│   │   ├── [C] Eliminate proven duplicate execution paths (pr216-duplicate-execution; task; weight 5)
│   │   ├── [P] Bound replay runtime and RSpace object lifetimes (pr216-runtime-lifetime; task)
│   │   │   ├── [P] Inventory runtime and RSpace ownership (pr216-runtime-ownership-inventory; task; weight 2)
│   │   │   ├── [P] Bound runtime and cache release (pr216-runtime-cache-release; task; weight 3)
│   │   │   ├── [P] Bound root and history retention (pr216-root-history-retention; task; weight 3)
│   │   │   └── [P] Measure long-horizon resource behavior (pr216-long-horizon-resource-measurement; task; weight 3)
│   │   ├── [A] Remove unbounded finalization-ledger startup scans (pr216-ledger-startup; task; weight 5)
│   │   ├── [P] Bound admission and recovery backlog growth (pr216-admission-backpressure; task; weight 5)
│   │   ├── [P] Add soak resource and progress observability (pr216-soak-observability; task; weight 3)
│   │   └── [P] Run targeted regression, lint, and proof gates (pr216-targeted-validation; task; weight 3)
│   ├── [P] M6 Cross-prover assurance and documentation (pr216-m6-assurance; milestone)
│   │   ├── [P] Complete cross-prover knotted cost models and negative controls (pr216-cross-prover-models; task)
│   │   │   ├── [P] Complete knotted-cost Rocq proofs (pr216-cross-prover-rocq; task; weight 3)
│   │   │   ├── [P] Check all safe knotted-cost TLA+ models (pr216-cross-prover-tlc-safe; task; weight 3)
│   │   │   ├── [P] Refute all knotted-cost unsafe controls (pr216-cross-prover-unsafe-controls; task; weight 3)
│   │   │   └── [P] Check symbolic knotted-cost models with Apalache (pr216-cross-prover-apalache; task; weight 3)
│   │   ├── [P] Extract knotted cost invariants into property, Loom, and integration tests (pr216-property-loom-suite; task)
│   │   │   ├── [P] Extract the knotted-cost property corpus (pr216-knotted-property-corpus; task; weight 3)
│   │   │   ├── [P] Model knotted-cost concurrency with Loom (pr216-knotted-loom; task; weight 3)
│   │   │   └── [P] Test formal and runtime parity (pr216-knotted-parity-integration; task; weight 3)
│   │   ├── [P] Document three-paper cost semantics and proof conformance (pr216-docs-conformance; task)
│   │   │   ├── [P] Document three-paper traceability (pr216-docs-traceability; task; weight 3)
│   │   │   ├── [P] Document runtime and API conformance (pr216-docs-runtime-api; task; weight 3)
│   │   │   └── [P] Audit claims, citations, and STE prose (pr216-docs-claim-citation-ste; task; weight 2)
│   │   ├── [P] Make three-paper verification gates strict and reproducible (pr216-gate-integration; task; weight 5)
│   │   ├── [P] Define the three-paper executable semantic domain (pr216-knotted-topoi-calibration; task; weight 5)
│   │   ├── [P] Audit all three-paper semantic claims and proof boundaries (pr216-semantic-claim-audit; task; weight 3)
│   │   ├── [P] Prove rooted semantic location identity and runtime refinement (pr216-rooted-location-identity; task; weight 5)
│   │   ├── [P] Prove native reflection and persistent-listener coherence (pr216-operational-reflection-coherence; task; weight 5)
│   │   ├── [P] Define the paired context-cost transition system (pr216-context-cost-lts; task; weight 8)
│   │   ├── [P] Prove bidirectional native Rholang operational correspondence (pr216-rho-operational-correspondence; task; weight 8)
│   │   ├── [P] Prove paired-label OSLF adequacy for funded episodes (pr216-specialized-oslf-classifier; task; weight 8)
│   │   ├── [P] Calibrate Rholang contextual and weak-barbed equivalence (pr216-rholang-observational-calibration; task; weight 8)
│   │   ├── [P] Prove unbounded persistent-service prefix safety (pr216-persistent-lifetime-semantics; task; weight 8)
│   │   └── [P] Close every supported external review finding (pr216-review-findings-closure; task; weight 3)
│   └── [P] M7 CI and 24-hour soak release gate (pr216-m7-release; milestone)
│       ├── [P] Run multi-node consensus and recovery validation (pr216-integration-validation; task; weight 5)
│       ├── [P] Pass the 24-hour merge-recovery soak (pr216-soak-24h; task; weight 8)
│       ├── [P] Audit completeness and prepare the PR update (pr216-final-audit; task)
│       │   └── [P] PR audit supersession record (pr216-final-audit-supersession-record; note)
│       ├── [P] Pass the final aggregate CI gate (ca-final-aggregate-ci; task; weight 5)
│       ├── [P] Publish the final revision-bound evidence manifest (ca-final-evidence-manifest; task; weight 2)
│       ├── [P] Prepare the evidence-backed PR update (pr216-update-preparation; task; weight 1)
│       └── [P] Harden the 24-hour soak qualification contract (pr216-soak-qualification-hardening; task; weight 2)
├── [P] M5 Define the canonical n-payer funding contract (ca-npayer-m1-contract; milestone)
│   ├── [P] Ratify n-payer economic and failure policies (ca-npayer-policy-adr; task)
│   │   ├── [P] Ratify authority and allocation policy (ca-npayer-adr-authority-allocation; task; weight 1)
│   │   ├── [P] Ratify economic and failure policy (ca-npayer-adr-economics-failures; task; weight 1)
│   │   └── [P] Ratify activation and migration policy (ca-npayer-adr-activation-migration; task; weight 1)
│   ├── [P] Audit all binary and multiplied-debit assumptions (ca-npayer-binary-path-audit; task)
│   │   ├── [P] Audit binary assumptions in production paths (ca-npayer-audit-code-paths; task; weight 1)
│   │   ├── [P] Audit binary assumptions in proofs and tests (ca-npayer-audit-proof-test-paths; task; weight 1)
│   │   └── [P] Publish the binary-assumption findings matrix (ca-npayer-audit-findings-matrix; task; weight 1)
│   ├── [P] Specify resource dimensions and signed limits (ca-npayer-resource-contract; task)
│   │   ├── [P] Specify resource units and measurement points (ca-npayer-resource-units-measurement; task; weight 1)
│   │   ├── [P] Specify resource bounds and exhaustion (ca-npayer-resource-bounds-exhaustion; task; weight 1)
│   │   └── [P] Specify persistence and storage liability (ca-npayer-resource-persistence-liability; task; weight 1)
│   ├── [P] Specify consensus prices and client consent (ca-npayer-price-contract; task)
│   │   ├── [P] Specify price schedules and denominations (ca-npayer-price-schedule-units; task; weight 1)
│   │   ├── [P] Specify signed price consent (ca-npayer-price-signed-consent; task; weight 1)
│   │   └── [P] Specify price transitions and replay (ca-npayer-price-transition-replay; task; weight 1)
│   ├── [P] Specify payer authorization and exposure (ca-npayer-authorization-contract; task)
│   │   ├── [P] Specify authority and custody identity (ca-npayer-auth-identity-custody; task; weight 1)
│   │   ├── [P] Specify thresholds and payer caps (ca-npayer-auth-threshold-cap; task; weight 1)
│   │   └── [P] Specify delegation and persistent authority (ca-npayer-auth-delegation-persistence; task; weight 1)
│   ├── [P] Specify conversion and multi-agent withdrawal authority (ca-npayer-conversion-contract; task)
│   │   ├── [P] Specify conversion quotes and rates (ca-npayer-conversion-quote-rate; task; weight 1)
│   │   ├── [P] Specify conversion and withdrawal authority (ca-npayer-conversion-authority-contract; task; weight 1)
│   │   └── [P] Specify conversion provenance and refunds (ca-npayer-conversion-provenance-contract; task; weight 1)
│   ├── [P] Specify the deterministic linear funding solver (ca-npayer-solver-contract; task)
│   │   ├── [P] Specify solver inputs and rejection contract (ca-npayer-solver-input-rejection-contract; task; weight 1)
│   │   ├── [P] Specify allocation and residual contract (ca-npayer-solver-allocation-residual-contract; task; weight 1)
│   │   ├── [P] Specify settlement and refund contract (ca-npayer-solver-settlement-contract; task; weight 1)
│   │   └── [P] Specify replay and concurrency contract (ca-npayer-solver-replay-contract; task; weight 1)
│   ├── [P] Specify protocol identity and migration (ca-npayer-wire-contract; task)
│   │   ├── [P] Specify wire schema and reserved tags (ca-npayer-wire-contract-schema; task; weight 1)
│   │   ├── [P] Specify signing and deploy identity (ca-npayer-wire-contract-identity; task; weight 1)
│   │   └── [P] Specify activation and legacy compatibility (ca-npayer-wire-contract-compatibility; task; weight 1)
│   ├── [P] Specify signed phlo intent and solver semantics (ca-phlo-signed-semantics; task; weight 3)
│   └── [P] Bind phlo controls to wire identity and signatures (ca-phlo-wire-signing; task; weight 3)
├── [P] M6 Prove n-payer allocation and concurrent settlement (ca-npayer-m2-formal; milestone)
│   ├── [P] Prove arbitrary-list authority preservation (ca-npayer-rocq-authority; task)
│   │   ├── [P] Prove arbitrary-list authority structure (ca-npayer-rocq-authority-lists; task; weight 1)
│   │   ├── [P] Prove custody alias conservation (ca-npayer-rocq-authority-custody; task; weight 1)
│   │   └── [P] Prove operator authority preservation (ca-npayer-rocq-authority-operators; task; weight 1)
│   ├── [P] Prove capped max-min allocation and residuals (ca-npayer-rocq-allocation; task)
│   │   ├── [P] Prove max-min fairness and residual bounds (ca-npayer-rocq-allocation-fairness; task; weight 1)
│   │   ├── [P] Prove allocation conservation and no overdraw (ca-npayer-rocq-allocation-conservation; task; weight 1)
│   │   └── [P] Prove canonical batch serializability (ca-npayer-rocq-allocation-batch; task; weight 1)
│   ├── [P] Prove pricing, reservation, refund, and conversion conservation (ca-npayer-rocq-pricing; task)
│   │   ├── [P] Prove checked pricing and resource bounds (ca-npayer-rocq-pricing-checked-bounds; task; weight 3)
│   │   ├── [P] Prove reservation and refund conservation (ca-npayer-rocq-pricing-reservation-refund; task; weight 3)
│   │   ├── [P] Prove fee conservation and routing (ca-npayer-rocq-pricing-fees; task; weight 2)
│   │   └── [P] Prove exchange and withdrawal conservation (ca-npayer-rocq-pricing-exchange-withdrawal; task; weight 3)
│   ├── [P] Prove three-paper semantic refinement (ca-npayer-rocq-refinement; task)
│   │   ├── [P] Prove the paper-to-contract refinement (ca-npayer-rocq-refinement-papers; task; weight 1)
│   │   ├── [P] Prove the contract-to-runtime refinement (ca-npayer-rocq-refinement-runtime; task; weight 1)
│   │   └── [P] Prove consensus and replay refinement (ca-npayer-rocq-refinement-consensus; task; weight 1)
│   ├── [P] Model parallel n-payer settlement in TLA+ (ca-npayer-tla-concurrency; task)
│   │   ├── [P] Model concurrent reservation and publication (ca-npayer-tla-reservation-publication; task; weight 3)
│   │   ├── [P] Model replay and restart settlement (ca-npayer-tla-replay-restart; task; weight 3)
│   │   └── [P] Model parallel validators and shards (ca-npayer-tla-validator-shard; task; weight 3)
│   ├── [P] Check symbolic allocation and concurrency with Apalache (ca-npayer-apalache; task)
│   │   ├── [P] Check safe symbolic n-payer models (ca-npayer-apalache-safe; task; weight 1)
│   │   ├── [P] Check symbolic unsafe controls (ca-npayer-apalache-controls; task; weight 1)
│   │   └── [P] Publish Apalache bounds and coverage (ca-npayer-apalache-bounds-report; task; weight 1)
│   ├── [P] Validate formal unsafe controls (ca-npayer-negative-controls; task)
│   │   ├── [P] Refute allocation defects (ca-npayer-controls-allocation; task; weight 1)
│   │   ├── [P] Refute authority defects (ca-npayer-controls-authority; task; weight 1)
│   │   └── [P] Refute replay and settlement defects (ca-npayer-controls-replay-settlement; task; weight 1)
│   ├── [P] Complete formal requirement traceability (ca-npayer-formal-traceability; task)
│   │   ├── [P] Inventory n-payer formal obligations (ca-npayer-formal-traceability-inventory; task; weight 2)
│   │   └── [P] Audit n-payer formal evidence (ca-npayer-formal-traceability-audit; task; weight 2)
│   └── [P] Prove phlo, host-work, and result-cache conformance (ca-phlo-host-work-cache-conformance; task; weight 3)
├── [P] M7 Implement and integrate the canonical funding solver (ca-npayer-m3-implementation; milestone)
│   ├── [P] Implement versioned funding-plan wire types (ca-npayer-wire-types; task)
│   │   ├── [P] Implement funding-plan schema and round trips (ca-npayer-wire-schema-roundtrip; task; weight 3)
│   │   ├── [P] Implement funding-plan signing and hash domains (ca-npayer-wire-signing-hash; task; weight 3)
│   │   └── [P] Implement legacy wire dispatch and migration (ca-npayer-wire-legacy-dispatch; task; weight 3)
│   ├── [P] Implement the pure n-payer solver kernel (ca-npayer-solver-kernel; task)
│   │   ├── [P] Implement canonical solver types (ca-npayer-solver-kernel-types; task; weight 1)
│   │   ├── [P] Implement checked max-min allocation (ca-npayer-solver-kernel-allocation; task; weight 1)
│   │   ├── [P] Implement plan and settlement transitions (ca-npayer-solver-kernel-settlement; task; weight 1)
│   │   └── [P] Verify solver kernel properties (ca-npayer-solver-kernel-properties; task; weight 1)
│   ├── [P] Implement proof-carrying funding presentations (ca-npayer-presentation-validation; task)
│   │   ├── [P] Validate funding presentation syntax and authority (ca-npayer-presentation-decode-auth; task; weight 1)
│   │   ├── [P] Recompute and compare funding plans (ca-npayer-presentation-recompute; task; weight 1)
│   │   └── [P] Test presentation mutation and failure cases (ca-npayer-presentation-negative-tests; task; weight 1)
│   ├── [P] Implement the canonical reservation ledger (ca-npayer-reservation-ledger; task)
│   │   ├── [P] Implement atomic reservation and commit (ca-npayer-ledger-reserve-commit; task; weight 1)
│   │   ├── [P] Implement refund and failure recovery (ca-npayer-ledger-refund-failure; task; weight 1)
│   │   └── [P] Verify ledger concurrency and restart (ca-npayer-ledger-concurrency-restart; task; weight 1)
│   ├── [P] Integrate SystemVault settlement and refunds (ca-npayer-system-vault; task)
│   │   ├── [P] Implement atomic vault debit and refund (ca-npayer-vault-atomic-debit-refund; task; weight 3)
│   │   ├── [P] Implement top-up and joint custody (ca-npayer-vault-topup-joint-custody; task; weight 3)
│   │   └── [P] Implement vault failure and recovery (ca-npayer-vault-failure-recovery; task; weight 3)
│   ├── [P] Integrate multidimensional runtime budgets (ca-npayer-runtime-metering; task)
│   │   ├── [P] Integrate compute metering (ca-npayer-meter-compute; task; weight 2)
│   │   ├── [P] Integrate retained-storage metering (ca-npayer-meter-retained-storage; task; weight 2)
│   │   ├── [P] Integrate transferred-byte metering (ca-npayer-meter-transferred-bytes; task; weight 2)
│   │   ├── [P] Integrate trace-byte metering (ca-npayer-meter-trace-bytes; task; weight 2)
│   │   └── [P] Compose resource meters and replay (ca-npayer-meter-composition-replay; task; weight 3)
│   ├── [P] Integrate admission and deterministic batch reconciliation (ca-npayer-admission-batch; task)
│   │   ├── [P] Integrate canonical funding admission (ca-npayer-admission-plan; task; weight 1)
│   │   ├── [P] Integrate canonical batch reconciliation (ca-npayer-admission-canonical-batch; task; weight 1)
│   │   └── [P] Verify admission replay and merge (ca-npayer-admission-replay-merge; task; weight 1)
│   ├── [P] Integrate every linear and located operator (ca-npayer-operators; task)
│   │   ├── [P] Integrate multiplicative and additive operators (ca-npayer-operators-multiplicatives-additives; task; weight 3)
│   │   ├── [P] Integrate arbitrary-arity joins (ca-npayer-operators-joins; task; weight 3)
│   │   ├── [P] Integrate persistent and modal operators (ca-npayer-operators-persistence; task; weight 3)
│   │   └── [P] Integrate lollipop, locations, and capability transfer (ca-npayer-operators-capability-transfer; task; weight 3)
│   ├── [P] Integrate replay, merge, finality, restart, and recovery (ca-npayer-replay-consensus; task)
│   │   ├── [P] Recompute funding plans during validation (ca-npayer-replay-validator-recompute; task; weight 3)
│   │   ├── [P] Deduplicate settlement across merge (ca-npayer-replay-merge-dedup; task; weight 3)
│   │   ├── [P] Preserve settlement across restart and recovery (ca-npayer-replay-restart-recovery; task; weight 3)
│   │   └── [P] Preserve historical protocol replay (ca-npayer-replay-historical; task; weight 3)
│   ├── [P] Integrate fees and failure settlement (ca-npayer-fees-failures; task)
│   │   ├── [P] Integrate deterministic fee routing (ca-npayer-fee-routing; task; weight 2)
│   │   └── [P] Separate exhaustion from platform faults (ca-npayer-exhaustion-platform-faults; task; weight 3)
│   ├── [P] Implement authorized exchange and withdrawal flows (ca-npayer-conversion-runtime; task)
│   │   ├── [P] Implement authorized conversion (ca-npayer-conversion-authorized; task; weight 3)
│   │   ├── [P] Implement joint and threshold withdrawal (ca-npayer-withdrawal-joint-threshold; task; weight 3)
│   │   └── [P] Refund converted reservations to original assets (ca-npayer-conversion-original-asset-refund; task; weight 3)
│   ├── [P] Integrate configuration, APIs, clients, and migration (ca-npayer-client-migration; task)
│   │   ├── [P] Integrate node APIs and configuration (ca-npayer-node-api-config; task; weight 3)
│   │   ├── [P] Integrate client signing and diagnostics (ca-npayer-client-signing-diagnostics; task; weight 3)
│   │   └── [P] Implement activation and migration tooling (ca-npayer-activation-migration; task; weight 3)
│   ├── [P] Restore phlo controls across clients and node APIs (ca-phlo-client-surfaces; task; weight 3)
│   └── [P] Integrate phlo maximums with reservation and refund (ca-phlo-reservation-refund; task; weight 3)
├── [P] M8 Validate, document, activate, and soak n-payer settlement (ca-npayer-m4-validation; milestone)
│   ├── [P] Add unit, example, and golden-vector tests (ca-npayer-unit-golden; task)
│   │   ├── [P] Add solver unit and boundary tests (ca-npayer-unit-solver; task; weight 1)
│   │   ├── [P] Add wire and signature golden vectors (ca-npayer-golden-wire; task; weight 1)
│   │   └── [P] Add runtime example tests (ca-npayer-example-runtime; task; weight 1)
│   ├── [P] Extract all formal invariants into property tests (ca-npayer-properties; task)
│   │   ├── [P] Extract allocator properties (ca-npayer-properties-allocator; task; weight 3)
│   │   ├── [P] Extract authorization properties (ca-npayer-properties-authorization; task; weight 3)
│   │   └── [P] Extract pricing and lifecycle properties (ca-npayer-properties-pricing-lifecycle; task; weight 3)
│   ├── [P] Add production-state Loom concurrency tests (ca-npayer-loom; task)
│   │   ├── [P] Model concurrent reservations with Loom (ca-npayer-loom-reservation; task; weight 3)
│   │   ├── [P] Model settlement, top-up, and refund with Loom (ca-npayer-loom-settlement-topup-refund; task; weight 3)
│   │   └── [P] Model exchange and retry with Loom (ca-npayer-loom-exchange-retry; task; weight 3)
│   ├── [P] Add replay, restart, and multi-validator integration tests (ca-npayer-integration; task)
│   │   ├── [P] Test n-payer play and replay (ca-npayer-integration-play-replay; task; weight 3)
│   │   ├── [P] Test n-payer restart and recovery (ca-npayer-integration-restart-recovery; task; weight 3)
│   │   └── [P] Test parallel validators and shards (ca-npayer-integration-validator-shard; task; weight 3)
│   ├── [P] Complete adversarial security validation (ca-npayer-security; task)
│   │   ├── [P] Validate authority and sponsor security (ca-npayer-security-authority-sponsor; task; weight 3)
│   │   └── [P] Validate arithmetic and resource security (ca-npayer-security-arithmetic-resource; task; weight 3)
│   ├── [P] Document end-to-end n-payer cost accounting (ca-npayer-documentation; task)
│   │   ├── [P] Document n-payer theory and architecture (ca-npayer-docs-theory-architecture; task; weight 3)
│   │   ├── [P] Document n-payer syntax, APIs, and clients (ca-npayer-docs-syntax-api-client; task; weight 3)
│   │   └── [P] Document n-payer security, operations, and migration (ca-npayer-docs-security-ops-migration; task; weight 3)
│   ├── [P] Integrate strict formal and CI gates (ca-npayer-gates; task)
│   │   ├── [P] Integrate n-payer formal gates (ca-npayer-gate-formal; task; weight 1)
│   │   ├── [P] Integrate n-payer code and test gates (ca-npayer-gate-code-tests; task; weight 1)
│   │   └── [P] Integrate n-payer evidence integrity (ca-npayer-gate-evidence-integrity; task; weight 1)
│   ├── [P] Pass the 24-hour multi-shard soak (ca-npayer-soak; task)
│   │   └── [P] N-payer soak supersession record (ca-npayer-soak-supersession-record; note)
│   ├── [P] Perform the n-payer completion audit (ca-npayer-final-audit; task)
│   │   └── [P] N-payer audit supersession record (ca-npayer-final-audit-supersession-record; note)
│   └── [P] Activate restored phlo semantics at a protocol boundary (ca-phlo-production-activation; task; weight 5)
├── [P] Casper dev versus cost-accounting ratification investigation (casper-dev-cost-accounting-ratification-investigation-2026-09-05; note)
│   ├── [P] Classify current dev and feature Casper differences (ca-casper-current-dev-classification; task; weight 3)
│   ├── [P] Maintain the Casper ratification decision ledger (ca-casper-decision-status-ledger; task; weight 2)
│   └── [P] Record corrections and retained findings from PR 390 (ca-pr390-correction-record; task; weight 2)
└── [P] Astra audit of the cost-accounting campaign plan (cost-accounting-plan-audit-2026-09-06; note)
```

## Planning limitations

Review note 9082 records proposed corrections that the tracker does not yet incorporate.
Those corrections include economic decisions, acceptance criteria, sequencing conflicts, and task estimates.
The current tree does not imply approval of unresolved economic or Casper decisions.

The required release order remains:

1. Complete current repairs and focused validation.
2. Complete arbitrary-payer funding, restored phlo controls, and remaining semantic assurance.
3. Pass final aggregate CI and multi-node integration.
4. Pass a pinned-candidate soak with at least 86,400 qualified seconds.
5. Audit the final evidence and prepare the PR update.

Scheduler duration priors are not calibrated completion estimates.
