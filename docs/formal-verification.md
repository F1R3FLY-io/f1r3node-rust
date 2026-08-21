# Formal Verification in f1r3node-rust

This document is the umbrella for how this repository uses formal methods:
what the stack is, the methodology every verified area follows, the index of
verified areas, and the obligations verification imposes on implementation
work. Per-directory READMEs under [`formal/`](../formal) stay lean and
model-specific; the process lives here.

## Philosophy

Two rules distinguish this repo's practice from decorative verification:

1. **Specs discover, they don't assume.** Every model is written to exhibit
   the defect class it guards against, not to flatter the implementation. A
   verified area therefore ships *violation configurations* alongside its
   gating configuration: the pre-fix configs reproduce the bug formally and
   are kept forever as counter-examples (run manually, excluded from CI),
   while the post-fix configs must stay clean in CI.

2. **Proof↔code divergence is a bug in the code, not the proof.** When the
   mechanization and the implementation disagree, the implementation moves
   — see commit `8763bc8e`, which closed a divergence between
   `compute_parents_post_state` and the Rocq `Selection.v` merge-scope
   proof by changing the Rust, with a backstop test pinning the alignment.

## The verification stack

| Layer | Tool | Location | What it proves |
| --- | --- | --- | --- |
| Protocol design | TLA+ (TLC, Apalache) | [`formal/tlaplus/`](../formal/tlaplus) | Safety invariants and liveness of concurrent protocols, bounded model checking |
| Mechanized theory | Rocq (Coq) | [`formal/rocq/`](../formal/rocq) | Axiom-free proofs of the algebraic core (finalized floor, fork choice, merge algebra, slashing) |
| Exhaustive code-level | Kani | `#[kani::proof]` harnesses (e.g. `casper/src/rust/slashing_authorization.rs`) | Bit-precise verification of arithmetic/authorization logic over the whole input domain |
| Property-based | proptest | `#[cfg(test)]` suites (e.g. `casper/.../replay_cache.rs`) | Op-sequence invariants at 10k cases PR-gate, 100k nightly |
| Concurrency | loom | slashing T-9.2 | Exhaustive small-thread-count interleaving checks on atomics |
| Search / analysis | Sage, Z3, Wolfram | [`formal/sage/`](../formal/sage), [`formal/z3/`](../formal/z3), [`formal/wolfram/`](../formal/wolfram) | Scenario search, hypothesis falsification, algebraic exploration |
| Adequacy | cargo-mutants | nightly | Mutation survival rate — does the suite actually constrain the code |

## The tier ladder

Defined by [`slashing-tests.yml`](../.github/workflows/slashing-tests.yml)
and reused by every verified area:

1. **example-based** — UC-* examples and integration tests (PR-gate)
2. **property-based** — T-* proptests, `PROPTEST_CASES=10000` (PR-gate)
3. **pre-fix regressions** — one deterministic counter-example test per
   historical bug, failing on the pre-fix code (PR-gate)
4. **loom interleavings** — exhaustive 2-thread model checks (PR-gate)
5. **TLA+ model check** — TLC over every gating `MC_*.cfg` via
   [`scripts/ci/check-tla-invariants.sh`](../scripts/ci/check-tla-invariants.sh)
   (nightly/dispatch: hosted-runner budgets, see the workflow header)
6. **Rocq build** — the mechanization must re-verify, axiom-free (PR-gate)
7. **mutation / extended fuzz** — nightly budgets

## Conventions for a verified area

- `formal/tlaplus/<area>/<Area>.tla` — the model, with a header mapping
  every action to the Rust it abstracts (file and function), and a *knob
  constant* per defect class distinguishing the fix from the regression.
- `MC_<Area>.cfg` — gating config, must stay clean; registered in
  `check-tla-invariants.sh` as `<area>/MC_<Area>`.
- `MC_<Area>_*_pre_fix.cfg` — expected-violation configs; excluded from CI,
  documented in the area README with the property they violate.
- `formal/<tool>/<area>/README.md` — model↔code table and config table only.
- Deep treatments (threat models, proofs of the design, test plans) go under
  `docs/theory/<area>/` — the slashing series
  ([`docs/theory/slashing/design/`](theory/slashing/design)) is the template.

## Verified areas

| Area | Models | Guards |
| --- | --- | --- |
| Slashing | [`formal/tlaplus/slashing/`](../formal/tlaplus/slashing), [`formal/rocq/slashing/`](../formal/rocq/slashing), kani harnesses, Rust example/property/integration tests | Equivocation detection, complete canonical evidence scanning, exact canonical merged-pre-state slash authority, proposer/receiver authorization parity, slash-evidence dependency fetch/resume, tracker-witness separation, zero-bond exclusion, concurrent tracking, and required counterexamples for ambient authority, omitted dependencies, tracker-only readiness, and merge-rejected-hint authorization |
| Finalized floor and LFB progress | [`formal/tlaplus/finalized_floor/`](../formal/tlaplus/finalized_floor), [`formal/rocq/finalized_floor/`](../formal/rocq/finalized_floor), Rust example/property/regression tests | Floor monotonicity, merge-scope scan correctness, complete deterministic finalizer candidate coverage, preservation of the original exact causal certificate, an additional exact state-preserving certificate over frozen latest-message state, current-LFB lineage admission, asymmetric 60/20/15 voting, off-main-parent state-preserving rebase progress, two-validator delivery-order convergence, committed-state preservation, and eventual floor rebase; fixed-prefix, work-budget restart, candidate-timeout starvation, unguarded certified stale-state promotion, causal-only rejected-parent promotion, erroneous main-spine admission, and its permanent rebase-starvation trace are required negative controls. `StateLineageFinality.tla` is exhaustively checked over 144 safe states by TLC and bounded independently by mandatory Apalache; `CliqueOracle.v` proves state certification refines causal certification, and `StateLineageFinality.v` is included in the axiom-free Rocq capstone. |
| Fork choice | [`formal/tlaplus/fork_choice/`](../formal/tlaplus/fork_choice), [`formal/rocq/fork_choice/`](../formal/rocq/fork_choice) | Estimator safety |
| Merge algebra | [`formal/rocq/merge_algebra/`](../formal/rocq/merge_algebra), [`formal/z3/merge_algebra/`](../formal/z3/merge_algebra) | Strict-total survivor selection, replay-authenticated exact execution deltas, causal-identity deduplication, and additive RSpace multiset projection; max-union and replicated whole-block deltas are negative models |
| Deploy lifecycle | [`formal/tlaplus/deploy_lifecycle/`](../formal/tlaplus/deploy_lifecycle) | No re-proposal of finalized/toxic deploys |
| Deploy occurrence consensus | [`formal/tlaplus/deploy_occurrence/`](../formal/tlaplus/deploy_occurrence), [`formal/rocq/finalized_floor/`](../formal/rocq/finalized_floor) | Source-specific rejection, one-winner preservation, observation-order convergence |
| Deploy recovery and protocol lifecycle | [`formal/tlaplus/deploy_recovery/`](../formal/tlaplus/deploy_recovery), [`formal/rocq/finalized_floor/theories/MergeRecoveryCoherence.v`](../formal/rocq/finalized_floor/theories/MergeRecoveryCoherence.v), [`formal/rocq/finalized_floor/theories/RejectionReasonConfluence.v`](../formal/rocq/finalized_floor/theories/RejectionReasonConfluence.v), [`formal/rocq/finalized_floor/theories/ProtocolActivationCoherence.v`](../formal/rocq/finalized_floor/theories/ProtocolActivationCoherence.v), [`formal/rocq/finalized_floor/theories/ProtocolVersionLifecycle.v`](../formal/rocq/finalized_floor/theories/ProtocolVersionLifecycle.v), [`formal/rocq/finalized_floor/theories/BootstrapReplayContext.v`](../formal/rocq/finalized_floor/theories/BootstrapReplayContext.v), [`formal/rocq/finalized_floor/theories/LocalFaultDeferral.v`](../formal/rocq/finalized_floor/theories/LocalFaultDeferral.v), [`formal/rocq/finalized_floor/theories/FundingAdmissionLifecycle.v`](../formal/rocq/finalized_floor/theories/FundingAdmissionLifecycle.v) | Retry only after every locally visible exact source is tombstoned; strict proposal-height lifespan closure; one recovery leader per committed finalized-height view; finalized-base receipt precedence; complete-chain rejection for exact tombstones and base duplicates; ordinary/mergeable state-record coherence; commutative, associative, and idempotent normalization of concurrent rejection causes; defensive active-version composition over historical floor metadata; homogeneous above-floor scope; version-bound record encoding; one authoritative protocol version from fresh-genesis ceremony through approval, fail-closed admission, adoption, proposal, recovery, and peer reception; historical bootstrap replay bound to each block's immutable consensus context; local validation faults deferred outside the ready queue without creating objective invalidity; ordinary descendants gated on a validated parent even after recovery transport failure; state-bound funding classified from the immutable proposal pre-state; underfunding recorded as terminal zero-effect rejection; fundable-rejection forgery rejected; bounded cross-view concurrency, eventual observation, and liveness past an offline leader. Protocol 4 is the sole runnable version; versions 1 through 3 and unknown approved versions are required negative startup cases. |
| Block admission | [`formal/tlaplus/block_admission/`](../formal/tlaplus/block_admission) | Byte-bounded inbound pipeline (below) |
| Replay cache | proptest invariants in `replay_cache.rs` | Entry/byte caps, accounting-equals-live-sum, admission contract, LRU order |
| End-to-end cost authority | [`formal/tlaplus/cost_accounted_rho/AtomicCommAccounting.tla`](../formal/tlaplus/cost_accounted_rho/AtomicCommAccounting.tla), [`formal/tlaplus/cost_accounted_rho/AtomicCommRejection.tla`](../formal/tlaplus/cost_accounted_rho/AtomicCommRejection.tla), [`formal/tlaplus/cost_accounted_rho/AtomicVaultSettlementRefinement.tla`](../formal/tlaplus/cost_accounted_rho/AtomicVaultSettlementRefinement.tla), [`formal/tlaplus/cost_accounted_rho/EndToEndCostConsensus.tla`](../formal/tlaplus/cost_accounted_rho/EndToEndCostConsensus.tla), [`formal/tlaplus/cost_accounted_rho/StateBoundAdmission.tla`](../formal/tlaplus/cost_accounted_rho/StateBoundAdmission.tla), [`formal/tlaplus/cost_accounted_rho/StateBoundValidatorConvergence.tla`](../formal/tlaplus/cost_accounted_rho/StateBoundValidatorConvergence.tla), [`formal/rocq/cost_accounted_rho/theories/AtomicCommAccounting.v`](../formal/rocq/cost_accounted_rho/theories/AtomicCommAccounting.v), [`formal/rocq/cost_accounted_rho/theories/AtomicVaultSettlementRefinement.v`](../formal/rocq/cost_accounted_rho/theories/AtomicVaultSettlementRefinement.v), [`formal/rocq/cost_accounted_rho/theories/EndToEndAuthority.v`](../formal/rocq/cost_accounted_rho/theories/EndToEndAuthority.v), [`formal/sage/cost_accounting/settlement_model.sage`](../formal/sage/cost_accounting/settlement_model.sage), [`formal/sage/cost_accounting/vault_backed_lifecycle.sage`](../formal/sage/cost_accounting/vault_backed_lifecycle.sage), Rust example/property/integration tests | One cost unit per successful atomic RSpace COMM; unmatched I/O costs zero; binary and join matches cost one; producer/consumer trigger symmetry; rejection-before-mutation; exact, replayable, idempotent canonical SystemVault genesis funding before admission; complete per-payer solvency over application transfer, physical settlement, quantitative-byte settlement, and fee; direct payer-to-proposer SystemVault fee transfer; structural conservative bounds for the closed fragment; dependent exact evidence for resident continuations; authority-derived finite single-play/replay capacity; exhaustion non-certifiability; root-chain, envelope, cost, status, exact causal event-log, settlement, and fee equality; terminating admission fixed point; replay-constrained independent-validator agreement even when scheduler-local traces have different event sets and costs; rejection of stale roots and mismatched block context; proposer/peer checkpoint and bond-validation parity; local-fault non-slashing; parent-order-independent finality; immutable ordered merge state; bounded block-index retention. Introduction charging, omission of any complete-debit component, genesis mismatch, genesis-funding reapplication, funding bypass, proposer-only checkpoint/bond bypass, local-fault slashing, structural ambient undercount, duplicate unconstrained play, exhausted admission, unbound certificate context, unchecked arrival-order execution, and acceptance of a scheduler-local trace instead of the certified witness are required negative controls. |
| Vault-backed quantitative cost and located lollipop | [`formal/tlaplus/cost_accounted_rho/VaultBackedByteAccounting.tla`](../formal/tlaplus/cost_accounted_rho/VaultBackedByteAccounting.tla), [`formal/tlaplus/cost_accounted_rho/LocatedVaultByteSettlement.tla`](../formal/tlaplus/cost_accounted_rho/LocatedVaultByteSettlement.tla), [`formal/tlaplus/cost_accounted_rho/WalletFundedLollipop.tla`](../formal/tlaplus/cost_accounted_rho/WalletFundedLollipop.tla), [`formal/tlaplus/cost_accounted_rho/FundingSlotBootstrap.tla`](../formal/tlaplus/cost_accounted_rho/FundingSlotBootstrap.tla), [`formal/tlaplus/cost_accounted_rho/IntroductionAuthorityRegistry.tla`](../formal/tlaplus/cost_accounted_rho/IntroductionAuthorityRegistry.tla), [`formal/rocq/cost_accounted_rho/theories/VaultBackedByteAccounting.v`](../formal/rocq/cost_accounted_rho/theories/VaultBackedByteAccounting.v), [`formal/rocq/cost_accounted_rho/theories/WalletFundedLollipop.v`](../formal/rocq/cost_accounted_rho/theories/WalletFundedLollipop.v), [`formal/rocq/cost_accounted_rho/theories/FundingSlotBootstrap.v`](../formal/rocq/cost_accounted_rho/theories/FundingSlotBootstrap.v), Rust example/property/Loom/replay tests | Protocol-4 canonical introduction, payload, and trace-byte tariffs are charged before RSpace mutation under one immutable RevVault reservation; persistent retries are idempotent while distinct occurrences retain multiplicity; producer/consumer trigger order, peek, and join arity preserve exact cost; top-ups conserve custody without expanding an in-flight certificate; physical authority and calculus COMM count remain separate; settlement and replay use exact component-wise located-purse allocations. Wallet-funded lollipop installation commits only an installer-paid authentication scaffold; a later finalized transfer atomically funds distinct outer and continuation purses before authenticated activation. Rejected funding preserves source balance, destination balances, and destination-vault existence. Retained capability possession and gateway authorization are both required; public deposit addresses never confer draw authority. Introduction sponsorship is linearizable and cannot mutate stored interaction authority. TLC and Apalache must pass each safe model and refute mutation-before-charge, trigger-side-only charging, omitted join participants, persistent recharge, peek credit, replay omission, top-up expansion, overflow wrapping, envelope-payer collapse, cross-purse rescue, eager located installation, candidate self-funding, slot-only funding, partial funding, rejected target creation, activation-before-funding, capability leakage, gateway bypass, missing outer authority, overcharge, replay omission, and split fallback registration. |
| Failure-atomic stack introduction and evaluation transactions | [`formal/tlaplus/cost_accounted_rho/StackIntroductionAtomicity.tla`](../formal/tlaplus/cost_accounted_rho/StackIntroductionAtomicity.tla), [`formal/tlaplus/cost_accounted_rho/EvaluationTransactionIsolation.tla`](../formal/tlaplus/cost_accounted_rho/EvaluationTransactionIsolation.tla), [`formal/rocq/cost_accounted_rho/theories/StackIntroductionAtomicity.v`](../formal/rocq/cost_accounted_rho/theories/StackIntroductionAtomicity.v), [`formal/rocq/cost_accounted_rho/theories/EvaluationTransactionIsolation.v`](../formal/rocq/cost_accounted_rho/theories/EvaluationTransactionIsolation.v), exhaustive Loom models, Rust unit/property/replay regressions | Pending stack cells are capacity-consuming but witness-invisible until the byte-charged RSpace operation succeeds. Operation rejection restores pending capacity; enclosing-deployment rejection removes every committed stack debit and birth while retaining attempted work and restoring RSpace. Parser failure cannot reuse a predecessor witness; reducer failure cannot erase current attempted work; play validation reverts its base; replay post-validation explicitly resets the active history root even after a candidate checkpoint was created. Merge evidence is published only after exact final-state acceptance. TLC and Apalache must pass both safe models and refute six stack-transaction defects plus five evaluation-transaction defects. Rocq proves unbounded conservation, rollback, witness isolation, checkpoint discard, and evidence-publication gating without assumptions. |
| Authenticated mergeable evidence | [`formal/tlaplus/cost_accounted_rho/MergeableEvidenceAuthentication.tla`](../formal/tlaplus/cost_accounted_rho/MergeableEvidenceAuthentication.tla), [`formal/rocq/cost_accounted_rho/theories/MergeableEvidenceAuthentication.v`](../formal/rocq/cost_accounted_rho/theories/MergeableEvidenceAuthentication.v), [`formal/loom/cost_accounting/tests/loom_mergeable_evidence_authentication.rs`](../formal/loom/cost_accounting/tests/loom_mergeable_evidence_authentication.rs), V14 Sage source-graph oracle, Rust key/property/network/replay/garbage-collection regressions | The cache key binds complete execution identity modulo the existing replay-semantic event-log permutation equivalence; deployment order and all non-log fields remain bound. Only local accepted replay can publish, peer payloads cannot overwrite, distinct equivocations retain both entries, opposite arrival orders converge, and finalized-entry retirement removes only the exact complete key after every retention guard has concrete evidence, including a nonempty latest-message witness set and advancement through any DAG parent path. TLC and Apalache must pass the safe model and refute legacy-key aliasing, legacy-key retirement, peer trust, vacuous latest-message retirement, and main-spine-only retirement. Rocq proves permutation equivalence, component separation, insertion commutation, exact and idempotent deletion, distinct-entry preservation, deletion/replay commutation, complete retirement guards, and secondary-parent completeness without assumptions; Loom exhausts publication and retirement schedules. |
| Block-heap lifecycle | [`formal/tlaplus/cost_accounted_rho/BlockHeapLifecycle.tla`](../formal/tlaplus/cost_accounted_rho/BlockHeapLifecycle.tla), [`formal/rocq/cost_accounted_rho/theories/BlockHeapLifecycle.v`](../formal/rocq/cost_accounted_rho/theories/BlockHeapLifecycle.v), [`formal/loom/cost_accounting/tests/loom_block_heap_lifecycle.rs`](../formal/loom/cost_accounting/tests/loom_block_heap_lifecycle.rs), node example/property tests, and the six-node RSS-guarded workload | Incoming task completion has one bounded, overflow-free atomic reclamation cadence; the Linux/glibc production default requests reclamation after every completed task, and every proposal attempt closes the corresponding local boundary after unwinding its transient values. Reclamation is semantically invisible to committed block history. TLC and Apalache must pass all two-slot schedules and refute missing-boundary reclamation; Rocq proves the interval and resident-envelope refinements without assumptions; Loom exhausts concurrent completion schedules. The operating-system refinement remains measured because `malloc_trim(3)` attempts rather than guarantees page release. |
| Deploy-envelope admission algebra | [`formal/rocq/cost_accounted_rho/theories/CostAccountedSyntax.v`](../formal/rocq/cost_accounted_rho/theories/CostAccountedSyntax.v), `models::casper_message` example and proptests | The scalar `Cosigned` wire representation accepts exactly an all-required atom/tensor tree or one top-level threshold over atomic candidate signers. Capability connectives, malformed thresholds, nested threshold members, and thresholds composed under tensor reject. Axiom-free theorems prove broad-algebra validity, exact policy shape, and nonzero bounded quorum; the Rust property test exercises both tensor positions across threshold sizes and bounds. Algebra-bearing envelopes take precedence over every unused flat compatibility field. |

## Worked example: byte-bounded block admission

The 2026-08-04 daily soak (run 30880995655) breached the host RSS ceiling
*after* the replay-cache runaway fix held. Per-node attribution showed the
readonly observer at 6,492MB peak against a 947–3,371MB validator baseline:
role-shaped retention on the receive-only path, whose block-processor queue
is bounded by message **count** (2048), not bytes.

The problem decomposes into four claims, each owned by a layer of the stack:

| # | Claim | Layer | Artifact |
| --- | --- | --- | --- |
| 1 | Retained bytes (queued **and** in-flight) never exceed the cap, under any arrival sequence | TLA+ | `Inv_RetainedBytesBounded`, gated by `MC_BlockAdmission.cfg`; violated by the current design in `MC_BlockAdmission_pre_fix.cfg` |
| 2 | Byte accounting never drifts from the sum over live messages | Kani + proptest | to be written against the implementation (replay-cache suite is the template) |
| 3 | Backpressure never wedges the shard: every broadcast block is eventually processed | TLA+ liveness | `Live_AllBroadcastProcessed`; the naive drop-based fix violates it in `MC_BlockAdmission_drop_pre_fix.cfg` |
| 4 | Admission counter updates are race-free across recv/drain | loom | to be written against the implementation |

### Implementation obligations

The model proves the *design*; these are the obligations it places on the
Rust that implements it. A PR implementing byte-bounded admission that does
not discharge all four is diverging from the proof:

1. **Budget queued + in-flight bytes, not queued alone.** A dequeued
   `BlockMessage` stays resident through its replay
   (`block_processor_instance.rs` holds it across the semaphore-gated
   task), so releasing budget at dequeue would under-count exactly the
   memory the observer node accumulated.
2. **Defer, never drop.** An over-budget block must remain requestable via
   the block-retriever's requested-blocks/dependency-recovery loop
   (`block_retriever.rs`). `MC_BlockAdmission_drop_pre_fix` is the standing
   proof that shedding converts a bounded-memory problem into a wedged
   shard — a strictly worse failure. Any future load-shedding transition
   must re-open the liveness argument.
3. **Deferral releases the payload buffer.** The model's `Defer` transition
   moves a block from `resident` (bytes counted) back to `pending` (no
   bytes retained): deferring must free the decoded `BlockMessage`, with
   re-delivery coming from a retriever re-request — never from a buffer
   held aside, which would re-create the unbounded retention off the
   books. `Inv_TotalResidencyBounded` (admission budget plus the bounded
   delivery window `MaxDeliveries × MaxBlockBytes`) is the checked form of
   this accounting.
4. **Cap ≥ max block size.** The module `ASSUME`s
   `MaxBlockBytes <= ByteCap`; the implementation must couple the byte cap
   to the protocol's block-size validation limit, otherwise an oversized
   block is unadmittable forever and liveness is forfeit by configuration.

The remaining ladder for this area, once soak attribution confirms the
queue as the retention site: implement admission extending the
byte-bounded-admission pattern from `8763bc8e`; kani harnesses on the
accounting arithmetic; a proptest op-sequence suite; a loom check on the
counter; and a pinned pre-fix regression test encoding the observer-node
profile from run 30880995655.

## Running the tools locally

```bash
# TLC (pinned jar, same release + sha256 as CI)
mkdir -p ~/.tla
curl -sSL -o ~/.tla/tla2tools.jar \
  https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar
echo "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88  $HOME/.tla/tla2tools.jar" | shasum -a 256 -c -

# Full gating frontier (what nightly CI runs)
TLA_TOOLS_JAR=~/.tla/tla2tools.jar bash scripts/ci/check-tla-invariants.sh

# One area, including its expected-violation configs
cd formal/tlaplus/block_admission
java -jar ~/.tla/tla2tools.jar -workers auto -config MC_BlockAdmission.cfg MC_BlockAdmission.tla
java -jar ~/.tla/tla2tools.jar -workers auto -config MC_BlockAdmission_pre_fix.cfg MC_BlockAdmission_pre_fix.tla   # expected: invariant violation

# Property tiers
PROPTEST_CASES=10000 cargo test -p casper --lib replay_cache

# Kani (requires cargo-kani)
cargo kani -p casper --harness <harness_name>
```
