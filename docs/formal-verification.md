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
| Concurrency | loom | [`formal/loom/`](../formal/loom) | Exhaustive small-thread-count interleaving checks on atomics and ownership transitions |
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
| Slashing and certified evidence readiness | [`formal/tlaplus/slashing/`](../formal/tlaplus/slashing), [`formal/tlaplus/finalized_floor/ObjectiveEvidenceAuthorization.tla`](../formal/tlaplus/finalized_floor/ObjectiveEvidenceAuthorization.tla), [`formal/rocq/slashing/`](../formal/rocq/slashing), [`formal/rocq/finalized_floor/theories/ObjectiveEquivocation.v`](../formal/rocq/finalized_floor/theories/ObjectiveEquivocation.v), [`formal/loom/cost_accounting/tests/loom_protocol_v5_dependency_readiness.rs`](../formal/loom/cost_accounting/tests/loom_protocol_v5_dependency_readiness.rs), [`formal/loom/cost_accounting/tests/loom_objective_equivocation.rs`](../formal/loom/cost_accounting/tests/loom_objective_equivocation.rs), kani harnesses, Rust example/property/integration/fuzz tests | Equivocation detection, generation-and-epoch filtering before canonical pair selection, complete canonical evidence scanning, bond and generation from one exact canonical merged-pre-state authority, pair-only activation, exact-key unary suppression, proposer/receiver authorization parity, admitted-metadata-only readiness for parents, justifications, unary evidence, objective pairs, and header-certified pairs, direct/buffer resolver parity, tracker and invalid-index noninterference, zero-bond exclusion, concurrent tracking, restart index derivation, and exact unsafe counterexamples for every weakened boundary. The historically named protocol-5 readiness model is a retained refinement layer inside the protocol-6 composition, not a runnable-version claim. |
| Finalized floor and LFB progress | [`formal/tlaplus/finalized_floor/`](../formal/tlaplus/finalized_floor), [`formal/rocq/finalized_floor/`](../formal/rocq/finalized_floor), Rust example/property/regression tests | Floor monotonicity, merge-scope scan correctness, complete deterministic finalizer candidate coverage, preservation of the original exact causal certificate, an additional exact state-preserving certificate over frozen latest-message state, current-LFB lineage admission, asymmetric 60/20/15 voting, off-main-parent state-preserving rebase progress, two-validator delivery-order convergence, committed-state preservation, eventual floor rebase, committee-transition separation, and crash-consistent parallel finalization publication. `FinalizationAtomicity` proves one-winner compare-and-append and no lost scheduling wake; `FinalizationWorkerRetry` proves that failed workers cannot falsely complete request coverage; `ProposalFloorReadiness` proves that only missing floor materialization schedules finalization while committee, validator, and permit defects remain isolated; `FinalizationBoundHead` proves that evaluation and append share one exact predecessor and refutes late-binding a DAG-valid but state-regressive candidate; `FinalizationRecovery` proves ordered projection, complete effect prefixes, and receipt compaction with $`0 \le C \le E \le P \le H`$; `FinalizationGenesisIdentity` proves atomic rooted bootstrap, immutable chain identity, write-free exact retries, and restart preservation while four controls refute head reset, root overwrite, split bootstrap, and unrooted backfill. `RecoveryCommitteeTransition` proves that serialized bonds are replayed post-state data, while exact justifications, sender membership, sequence context, recovery leadership, and synchrony use one structural floor authority; accepted registration precedes later floor activation. `ProtocolV5EndToEnd` composes three replicas from parallel sibling proposal, exact-prestate intrinsic admission, arbitrary delivery, generation-scoped evidence, crash repair, frozen-committee finality, withdraw/rebond/slash/redemption custody, and deterministic cost replay/settlement. Its verification is deliberately compositional: TLC exhausts the concurrent component models and twelve guided cross-boundary defect traces, Apalache checks the bounded whole-protocol product, and the Rocq capstone discharges the unbounded deductive obligations. The gate does not enumerate the unconstrained `FreeNext` Cartesian product with TLC because equivalent independent schedules cause a non-terminating practical frontier without adding a proof rule. Same-block self-authorization, head-local authority, premature promotion, cache mismatch, invalid registration, split finalization commit, stale publication, late-bound state regression, early effects, cursor gaps, lost wakes, failure-as-completion, readiness bypass, non-materialization retry, genesis reset, genesis overwrite, partial bootstrap, and inferred backfill are required controls. Fixed-prefix, work-budget restart, candidate-timeout starvation, unguarded certified stale-state promotion, causal-only rejected-parent promotion, erroneous main-spine admission, and its permanent rebase-starvation trace remain required negative controls. `StateLineageFinality.tla` is exhaustively checked over 144 safe states by TLC and bounded independently by mandatory Apalache; `CliqueOracle.v`, `StateLineageFinality.v`, `CommitteeTransition.v`, `FinalizationAtomicity.v`, and `ProposalFloorReadiness.v` are included in the axiom-free Rocq capstone. |
| Fork choice | [`formal/tlaplus/fork_choice/`](../formal/tlaplus/fork_choice), [`formal/rocq/fork_choice/`](../formal/rocq/fork_choice) | Estimator safety |
| Merge algebra | [`formal/rocq/merge_algebra/`](../formal/rocq/merge_algebra), [`formal/z3/merge_algebra/`](../formal/z3/merge_algebra) | Strict-total survivor selection, replay-authenticated exact execution deltas, causal-identity deduplication, and additive RSpace multiset projection; max-union and replicated whole-block deltas are negative models |
| Deploy lifecycle | [`formal/tlaplus/deploy_lifecycle/`](../formal/tlaplus/deploy_lifecycle) | No re-proposal of finalized/toxic deploys |
| Deploy occurrence consensus | [`formal/tlaplus/deploy_occurrence/`](../formal/tlaplus/deploy_occurrence), [`formal/rocq/finalized_floor/`](../formal/rocq/finalized_floor) | Source-specific rejection, one-winner preservation, observation-order convergence |
| Deploy recovery and protocol lifecycle | [`formal/tlaplus/deploy_recovery/`](../formal/tlaplus/deploy_recovery), [`formal/rocq/finalized_floor/theories/MergeRecoveryCoherence.v`](../formal/rocq/finalized_floor/theories/MergeRecoveryCoherence.v), [`formal/rocq/finalized_floor/theories/RejectionReasonConfluence.v`](../formal/rocq/finalized_floor/theories/RejectionReasonConfluence.v), [`formal/rocq/finalized_floor/theories/ProtocolActivationCoherence.v`](../formal/rocq/finalized_floor/theories/ProtocolActivationCoherence.v), [`formal/rocq/finalized_floor/theories/ProtocolVersionLifecycle.v`](../formal/rocq/finalized_floor/theories/ProtocolVersionLifecycle.v), [`formal/rocq/finalized_floor/theories/BootstrapReplayContext.v`](../formal/rocq/finalized_floor/theories/BootstrapReplayContext.v), [`formal/rocq/finalized_floor/theories/LocalFaultDeferral.v`](../formal/rocq/finalized_floor/theories/LocalFaultDeferral.v), [`formal/rocq/finalized_floor/theories/FundingAdmissionLifecycle.v`](../formal/rocq/finalized_floor/theories/FundingAdmissionLifecycle.v) | Retry only after every locally visible exact source is tombstoned; strict proposal-height lifespan closure; one recovery leader per committed finalized-height view; finalized-base receipt precedence; complete-chain rejection for exact tombstones and base duplicates; ordinary/mergeable state-record coherence; commutative, associative, and idempotent normalization of concurrent rejection causes; defensive active-version composition over historical floor metadata; homogeneous above-floor scope; version-bound record encoding; one authoritative protocol version from fresh-genesis ceremony through approval, fail-closed admission, adoption, proposal, recovery, and peer reception; historical bootstrap replay bound to each block's immutable consensus context; local validation faults deferred outside the ready queue without creating objective invalidity; ordinary descendants gated on a validated parent even after recovery transport failure; state-bound funding classified from the immutable proposal pre-state; underfunding recorded as terminal zero-effect rejection; fundable-rejection forgery rejected; bounded cross-view concurrency, eventual observation, and liveness past an offline leader. Protocol 6 is the sole runnable version; versions 1 through 5 and unknown approved versions are required negative startup cases. |
| Block admission and P2P transport residency | [`formal/tlaplus/block_admission/`](../formal/tlaplus/block_admission), [`formal/loom/cost_accounting/tests/loom_block_admission.rs`](../formal/loom/cost_accounting/tests/loom_block_admission.rs), [`formal/loom/cost_accounting/tests/loom_transport_payload_residency.rs`](../formal/loom/cost_accounting/tests/loom_transport_payload_residency.rs), Rust property/unit/network regressions | Count-and-byte-bounded block admission through replay; finite non-evicting dependency tracking and one-payload buffer scans; exact post-reservation inbound/outbound transport byte/item ownership; checked `handler limit × decoded-message limit` pre-reservation ownership; composed service residency; compressed wire plus decoded residency; lazy chunking and shared fanout; success only after remote completion; aligned finite pre-SETTINGS HTTP/2 and item windows with independent handler parallelism; linearizable peer initialization and idle retirement; request-local validation. Each historical weakening has a required TLC counterexample; safe state spaces pass TLC and bounded Apalache. |
| Replay cache | proptest invariants in `replay_cache.rs` | Entry/byte caps, accounting-equals-live-sum, admission contract, LRU order |
| End-to-end cost authority | [`formal/tlaplus/cost_accounted_rho/AtomicCommAccounting.tla`](../formal/tlaplus/cost_accounted_rho/AtomicCommAccounting.tla), [`formal/tlaplus/cost_accounted_rho/AtomicCommRejection.tla`](../formal/tlaplus/cost_accounted_rho/AtomicCommRejection.tla), [`formal/tlaplus/cost_accounted_rho/AtomicVaultSettlementRefinement.tla`](../formal/tlaplus/cost_accounted_rho/AtomicVaultSettlementRefinement.tla), [`formal/tlaplus/cost_accounted_rho/EndToEndCostConsensus.tla`](../formal/tlaplus/cost_accounted_rho/EndToEndCostConsensus.tla), [`formal/tlaplus/cost_accounted_rho/StateBoundAdmission.tla`](../formal/tlaplus/cost_accounted_rho/StateBoundAdmission.tla), [`formal/tlaplus/cost_accounted_rho/StateBoundValidatorConvergence.tla`](../formal/tlaplus/cost_accounted_rho/StateBoundValidatorConvergence.tla), [`formal/rocq/cost_accounted_rho/theories/AtomicCommAccounting.v`](../formal/rocq/cost_accounted_rho/theories/AtomicCommAccounting.v), [`formal/rocq/cost_accounted_rho/theories/AtomicVaultSettlementRefinement.v`](../formal/rocq/cost_accounted_rho/theories/AtomicVaultSettlementRefinement.v), [`formal/rocq/cost_accounted_rho/theories/EndToEndAuthority.v`](../formal/rocq/cost_accounted_rho/theories/EndToEndAuthority.v), [`formal/sage/cost_accounting/settlement_model.sage`](../formal/sage/cost_accounting/settlement_model.sage), [`formal/sage/cost_accounting/vault_backed_lifecycle.sage`](../formal/sage/cost_accounting/vault_backed_lifecycle.sage), Rust example/property/integration tests | One cost unit per successful atomic RSpace COMM; unmatched I/O costs zero; binary and join matches cost one; producer/consumer trigger symmetry; rejection-before-mutation; exact, replayable, idempotent canonical SystemVault genesis funding before admission; complete per-payer solvency over application transfer, physical settlement, quantitative-byte settlement, and fee; direct payer-to-proposer SystemVault fee transfer; structural conservative bounds for the closed fragment; dependent exact evidence for resident continuations; authority-derived finite single-play/replay capacity; exhaustion non-certifiability; root-chain, envelope, cost, status, exact causal event-log, settlement, and fee equality; terminating admission fixed point; replay-constrained independent-validator agreement even when scheduler-local traces have different event sets and costs; rejection of stale roots and mismatched block context; proposer/peer checkpoint and bond-validation parity; local-fault non-slashing; parent-order-independent finality; immutable ordered merge state; bounded block-index retention. Introduction charging, omission of any complete-debit component, genesis mismatch, genesis-funding reapplication, funding bypass, proposer-only checkpoint/bond bypass, local-fault slashing, structural ambient undercount, duplicate unconstrained play, exhausted admission, unbound certificate context, unchecked arrival-order execution, and acceptance of a scheduler-local trace instead of the certified witness are required negative controls. |
| Vault-backed quantitative cost and located lollipop | [`formal/tlaplus/cost_accounted_rho/VaultBackedByteAccounting.tla`](../formal/tlaplus/cost_accounted_rho/VaultBackedByteAccounting.tla), [`formal/tlaplus/cost_accounted_rho/LocatedVaultByteSettlement.tla`](../formal/tlaplus/cost_accounted_rho/LocatedVaultByteSettlement.tla), [`formal/tlaplus/cost_accounted_rho/WalletFundedLollipop.tla`](../formal/tlaplus/cost_accounted_rho/WalletFundedLollipop.tla), [`formal/tlaplus/cost_accounted_rho/FundingSlotBootstrap.tla`](../formal/tlaplus/cost_accounted_rho/FundingSlotBootstrap.tla), [`formal/tlaplus/cost_accounted_rho/IntroductionAuthorityRegistry.tla`](../formal/tlaplus/cost_accounted_rho/IntroductionAuthorityRegistry.tla), [`formal/rocq/cost_accounted_rho/theories/VaultBackedByteAccounting.v`](../formal/rocq/cost_accounted_rho/theories/VaultBackedByteAccounting.v), [`formal/rocq/cost_accounted_rho/theories/WalletFundedLollipop.v`](../formal/rocq/cost_accounted_rho/theories/WalletFundedLollipop.v), [`formal/rocq/cost_accounted_rho/theories/FundingSlotBootstrap.v`](../formal/rocq/cost_accounted_rho/theories/FundingSlotBootstrap.v), Rust example/property/Loom/replay tests | Protocol-4 canonical introduction, payload, and trace-byte tariffs are charged before RSpace mutation under one immutable RevVault reservation; persistent retries are idempotent while distinct occurrences retain multiplicity; producer/consumer trigger order, peek, and join arity preserve exact cost; top-ups conserve custody without expanding an in-flight certificate; physical authority and calculus COMM count remain separate; settlement and replay use exact component-wise located-purse allocations. Wallet-funded lollipop installation commits only an installer-paid authentication scaffold; a later finalized transfer atomically funds distinct outer and continuation purses before authenticated activation. Rejected funding preserves source balance, destination balances, and destination-vault existence. Retained capability possession and gateway authorization are both required; public deposit addresses never confer draw authority. Introduction sponsorship is linearizable and cannot mutate stored interaction authority. TLC and Apalache must pass each safe model and refute mutation-before-charge, trigger-side-only charging, omitted join participants, persistent recharge, peek credit, replay omission, top-up expansion, overflow wrapping, envelope-payer collapse, cross-purse rescue, eager located installation, candidate self-funding, slot-only funding, partial funding, rejected target creation, activation-before-funding, capability leakage, gateway bypass, missing outer authority, overcharge, replay omission, and split fallback registration. |
| Validator redemption custody | [`formal/tlaplus/cost_accounted_rho/ConcurrentRedemptionCustody.tla`](../formal/tlaplus/cost_accounted_rho/ConcurrentRedemptionCustody.tla), [`formal/rocq/cost_accounted_rho/theories/CanonicalRevRedemption.v`](../formal/rocq/cost_accounted_rho/theories/CanonicalRevRedemption.v), [`formal/rocq/cost_accounted_rho/theories/RedemptionCustodyAtomicity.v`](../formal/rocq/cost_accounted_rho/theories/RedemptionCustodyAtomicity.v), [`formal/rocq/cost_accounted_rho/theories/RedemptionMintResumption.v`](../formal/rocq/cost_accounted_rho/theories/RedemptionMintResumption.v), [`formal/loom/cost_accounting/tests/loom_redemption_custody.rs`](../formal/loom/cost_accounting/tests/loom_redemption_custody.rs), native Rust/Rholang examples and generated authorization properties | Redemption is authorized for one immutable validator bond generation and commits stake, fuel, lifecycle, mint-halt, and receipt state atomically. Vindication and strictly partial guilt restore the exact pre-quarantine lifecycle; burn retains the halt and removes circulating claims. Identical retries are idempotent, conflicting or stale requests are effect-free, and distinct validator keys commute without a global lock. Redemption never directly mints or changes an epoch receipt; only a later fresh epoch can credit the unhalted validator. TLC exhausts the staged concurrent graph, Apalache checks a complete transaction/retry horizon and the deeper missing-lock counterexample, Rocq proves the unbounded algebraic obligations, and Loom explores Rust memory schedules. |
| PoS stake-vault human control | [`formal/tlaplus/cost_accounted_rho/PoSVaultAuthority.tla`](../formal/tlaplus/cost_accounted_rho/PoSVaultAuthority.tla), [`formal/rocq/cost_accounted_rho/theories/PoSVaultAuthority.v`](../formal/rocq/cost_accounted_rho/theories/PoSVaultAuthority.v), Rholang template and Casper play/replay regressions | The controller embedded in the blessed PoS source is derived from the exact key that signs that deployment. Template compilation rejects every unresolved `$$` marker before parsing. Only the matching authenticated deployer may transfer from the unforgeable stake vault; any other key leaves custody unchanged. TLC and Apalache prove safety and authorized progress and must reproduce literal-controller and permissive-template counterexamples. Rocq proves fail-closed compilation, exact binding, unauthorized noninterference, one-unit transfer, and custody conservation without assumptions. |
| Failure-atomic stack introduction and evaluation transactions | [`formal/tlaplus/cost_accounted_rho/StackIntroductionAtomicity.tla`](../formal/tlaplus/cost_accounted_rho/StackIntroductionAtomicity.tla), [`formal/tlaplus/cost_accounted_rho/EvaluationTransactionIsolation.tla`](../formal/tlaplus/cost_accounted_rho/EvaluationTransactionIsolation.tla), [`formal/rocq/cost_accounted_rho/theories/StackIntroductionAtomicity.v`](../formal/rocq/cost_accounted_rho/theories/StackIntroductionAtomicity.v), [`formal/rocq/cost_accounted_rho/theories/EvaluationTransactionIsolation.v`](../formal/rocq/cost_accounted_rho/theories/EvaluationTransactionIsolation.v), exhaustive Loom models, Rust unit/property/replay regressions | Pending stack cells are capacity-consuming but witness-invisible until the byte-charged RSpace operation succeeds. Operation rejection restores pending capacity; enclosing-deployment rejection removes every committed stack debit and birth while retaining attempted work and restoring RSpace. Parser failure cannot reuse a predecessor witness; reducer failure cannot erase current attempted work; play validation reverts its base; replay post-validation explicitly resets the active history root even after a candidate checkpoint was created. Merge evidence is published only after exact final-state acceptance. TLC and Apalache must pass both safe models and refute six stack-transaction defects plus five evaluation-transaction defects. Rocq proves unbounded conservation, rollback, witness isolation, checkpoint discard, and evidence-publication gating without assumptions. |
| Authenticated mergeable evidence | [`formal/tlaplus/cost_accounted_rho/MergeableEvidenceAuthentication.tla`](../formal/tlaplus/cost_accounted_rho/MergeableEvidenceAuthentication.tla), [`formal/rocq/cost_accounted_rho/theories/MergeableEvidenceAuthentication.v`](../formal/rocq/cost_accounted_rho/theories/MergeableEvidenceAuthentication.v), [`formal/loom/cost_accounting/tests/loom_mergeable_evidence_authentication.rs`](../formal/loom/cost_accounting/tests/loom_mergeable_evidence_authentication.rs), V14 Sage source-graph oracle, Rust key/property/network/replay/garbage-collection regressions | The cache key binds complete execution identity modulo the existing replay-semantic event-log permutation equivalence; deployment order and all non-log fields remain bound. Only local accepted replay can publish, peer payloads cannot overwrite, distinct equivocations retain both entries, opposite arrival orders converge, and finalized-entry retirement removes only the exact complete key after every retention guard has concrete evidence, including a nonempty latest-message witness set and advancement through any DAG parent path. TLC and Apalache must pass the safe model and refute legacy-key aliasing, legacy-key retirement, peer trust, vacuous latest-message retirement, and main-spine-only retirement. Rocq proves permutation equivalence, component separation, insertion commutation, exact and idempotent deletion, distinct-entry preservation, deletion/replay commutation, complete retirement guards, and secondary-parent completeness without assumptions; Loom exhausts publication and retirement schedules. |
| Block-heap lifecycle | [`formal/tlaplus/cost_accounted_rho/BlockHeapLifecycle.tla`](../formal/tlaplus/cost_accounted_rho/BlockHeapLifecycle.tla), [`formal/rocq/cost_accounted_rho/theories/BlockHeapLifecycle.v`](../formal/rocq/cost_accounted_rho/theories/BlockHeapLifecycle.v), [`formal/loom/cost_accounting/tests/loom_block_heap_lifecycle.rs`](../formal/loom/cost_accounting/tests/loom_block_heap_lifecycle.rs), node example/property tests, and the six-node RSS-guarded workload | Incoming task completion has one bounded, overflow-free atomic reclamation cadence; the Linux/glibc production default requests reclamation after every completed task, and every proposal attempt closes the corresponding local boundary after unwinding its transient values. Reclamation is semantically invisible to committed block history. TLC and Apalache must pass all two-slot schedules and refute missing-boundary reclamation; Rocq proves the interval and resident-envelope refinements without assumptions; Loom exhausts concurrent completion schedules. The operating-system refinement remains measured because `malloc_trim(3)` attempts rather than guarantees page release. |
| Deploy-envelope admission algebra | [`formal/rocq/cost_accounted_rho/theories/CostAccountedSyntax.v`](../formal/rocq/cost_accounted_rho/theories/CostAccountedSyntax.v), `models::casper_message` example and proptests | The scalar `Cosigned` wire representation accepts exactly an all-required atom/tensor tree or one top-level threshold over atomic candidate signers. Capability connectives, malformed thresholds, nested threshold members, and thresholds composed under tensor reject. Axiom-free theorems prove broad-algebra validity, exact policy shape, and nonzero bounded quorum; the Rust property test exercises both tensor positions across threshold sizes and bounds. Algebra-bearing envelopes take precedence over every unused flat compatibility field. |

The finalized-floor area includes an explicit cross-component refinement for
durable materialization. `FinalizerFloorMaterialization.tla` composes two
independently delivered node views, proposal deferral, all-parent finalizer
discovery, exact target binding, dual certification, and local publication. TLC
exhausts 9,289 generated / 1,849 distinct states to depth 15; Apalache checks the
safe model through length 8. Main-parent-only discovery and causal-only target
substitution are mandatory counterexamples. The axiom-free Rocq module proves
coverage equivalence and target-bound selection, while the production property
test compares every support set, per-target decision, eligible set, and greatest
candidate with an exhaustive pairwise oracle. Loom covers a concurrent ambient
latest-message arrival against the frozen target.

Protocol 6 is verified compositionally rather than by relabeling the existing
protocol-5 core model. `ProtocolV5EndToEnd.tla` remains the bounded composition
for cost settlement, validator-incarnation custody, admission, and finality.
`CertifiedFloorCommitment.tla` adds the signed, target-bound floor-certificate
commitment, `FinalizationCertificateRetrieval.tla` adds typed bounded sidecar
retrieval, failed-send retention, response validation, restart reconstruction,
and one-time detached-block wakeup, and `DependencyMaintenanceRound.tla` proves
that the production caller attempts its complete mixed block/certificate
snapshot before returning a dispatch error. TLC exhausts the retrieval model's
11,879 distinct states and the maintenance model's 158 distinct states.
Apalache checks them through symbolic lengths 12 and 8. Seven paired unsafe
controls reproduce each weakened boundary under both tools, Rocq proves the
unbounded contracts, and Rust/Loom tests bind the transitions to production.

`WitnessEquivalentCarrier.tla` exhausts 961 states across divergent honest
local witness digests, semantic carrier selection, parking, wakeup, restart, and
exact block/digest pairing. Its four unsafe controls reproduce exact-digest
parking, floor-only state substitution, proof-pair splicing, and missed wakeup in
both TLC and Apalache. `WitnessEquivalentCarrier.v` proves the corresponding
proof-irrelevance and exact-pair refinement axiom-free for arbitrary carrier
types.

`ParallelValidatorConsensus.tla` uses a $`40/35/25`$ committee so no validator
can certify alone. TLC exhausts the baseline's 12,877 generated / 3,411 distinct
states and separately checks crash/restart. A paired concurrency-window model
starts after one candidate was accepted but before a newer candidate became the
current floor: the correct model exhausts 150 generated / 58 distinct states and
preserves every committed effect, while removing only the current-floor guard
violates `CommittedEffectsRemainInFloor` in one transition. Apalache checks the
same safe window through bound 2 and finds the same unsafe one-step trace. This
pair prevents a stale locally accepted candidate from being mistaken for a valid
successor merely because it later accumulates a certificate; it does not change
certificate weight, clique formation, or parallel validator execution.

The accepted-stale-sibling lifecycle has a separate composed refinement.
`StaleSiblingRecovery.tla` interleaves three validator views through accepted
sibling delivery, floor advancement, complete-frontier settlement, exact
source-tombstone propagation, rejected-occurrence buffering, unique recovery
ownership, and converged finalization. TLC exhausts 1,508 generated / 451
distinct states to depth 20 and proves fair completion; Apalache checks the safe
path through bound 14. Seven mandatory controls independently violate stale
causal retention, exact frontier use, source identity, atomic buffering,
selected recovery, floor-effect preservation, and committed-view leadership.
`StaleSiblingRecovery.v` proves the unbounded sequential composition, while the
staged Casper regression and Loom model bind it to exact block/rejection
identity and concurrent recovery attempts.

## Worked example: byte-bounded block admission

The 2026-08-04 daily soak (run 30880995655) breached the host RSS ceiling
after the replay-cache runaway fix held. Per-node attribution isolated
role-shaped retention on the receive-only path. The historical processor
queue admitted by message count alone; the refined queue retains the count
guard and additionally owns an encoded-byte reservation from admission until
replay completion.

The problem decomposes into seven claims, each owned by a layer of the stack:

| # | Claim | Layer | Artifact |
| --- | --- | --- | --- |
| 1 | Retained bytes (queued **and** in-flight) never exceed the cap, under any arrival sequence | TLA+ | `Inv_RetainedBytesBounded`, gated by `MC_BlockAdmission.cfg`; violated by the historical design in `MC_BlockAdmission_pre_fix.cfg` |
| 2 | Byte accounting never wraps, exceeds the cap, or drifts across arbitrary reserve/release sequences | Kani + proptest | `block_processing_queue::verification::{successful_reservation_is_exact_and_bounded,failed_reservation_cannot_fit}` and `block_processing_queue::tests` |
| 3 | Under fair reannouncement, peer delivery, and processor progress, a finite announced work set is eventually processed even when `RequestCap < Cardinality(Blocks)` | TLA+ liveness | `Live_AllBroadcastProcessed`; `MC_BlockAdmission.cfg` exercises three blocks through two request slots, while the naive drop-based fix violates the property in `MC_BlockAdmission_drop_pre_fix.cfg` |
| 4 | Admission counter updates and release/retry races are linearizable | loom | `formal/loom/cost_accounting/tests/loom_block_admission.rs` |
| 5 | Request tracking remains bounded without evicting already-admitted unresolved work, and queue ownership remains sound without a tracker slot | TLA+ + Rust example tests | `tracked`, `unsolicited`, `DeliverUntracked`, `Reannounce`, `Inv_RetrieverTrackingBounded`, and `request_capacity_preserves_existing_work_and_defers_new_hashes` |
| 6 | A dependency-buffer scan materializes at most one full block outside the admission budget | TLA+ + Rust example/property tests | `BufferScanResidency.Inv_ScannerSinglePayload`, the required `MC_BufferScanResidency_pre_fix` counterexample, and `buffer_resolver::tests` |
| 7 | Every finite persisted dependency-buffer work set drains under fair scanner and processor progress | TLA+ liveness | `BufferScanResidency.Live_AllPersistedProcessed` in `MC_BufferScanResidency.cfg` |

### Implementation obligations

The model proves the *design*; these are the obligations it places on the
Rust that implements it. A change that does not discharge all seven diverges
from the proof:

1. **Budget queued + in-flight bytes, not queued alone.** A dequeued
   `BlockMessage` stays resident through its replay
   (`block_processor_instance.rs` holds it across the semaphore-gated
   task), so releasing budget at dequeue would under-count exactly the
   memory the observer node accumulated.
2. **Move reservation ownership with the block.** The queue item contains its
   RAII reservation. A rejected send drops only its attempted reservation; a
   successful dequeue moves the reservation into replay; task completion,
   cancellation, or receiver teardown releases it exactly once. There is no
   hash-indexed reservation side table to leak or become inconsistent.
3. **Defer tracked work, never shed it.** An over-budget requested block must remain requestable via
   the block-retriever's requested-blocks/dependency-recovery loop
   (`block_retriever.rs`). `MC_BlockAdmission_drop_pre_fix` is the standing
   proof that shedding converts a bounded-memory problem into a wedged
   shard — a strictly worse failure. Any future load-shedding transition
   must re-open the liveness argument.
4. **Deferral releases the payload buffer.** The model's `Defer` transition
   moves a block from `resident` (bytes counted) back to `pending` (no
   bytes retained): deferring must free the decoded `BlockMessage`, with
   re-delivery coming from a retriever re-request — never from a buffer
   held aside, which would re-create the unbounded retention off the
   books. `Inv_TotalResidencyBounded` (admission budget plus the bounded
   delivery window `MaxDeliveries × MaxBlockBytes`) is the checked form of
   this accounting.
5. **Cap ≥ max block size.** The module `ASSUME`s
   `MaxBlockBytes <= ByteCap`; the implementation must couple the byte cap
   to the protocol's streamed-message ceiling. `setup_node_program` constructs
   the queue with `protocol-server.grpc-max-recv-stream-message-size`, and an
   impossible oversized local block fails loudly instead of entering an
   infinite defer loop.
6. **Bound no-payload request state without evicting current work.** A new hash
   is admitted only while the request map has capacity. Existing unresolved
   hashes retain their slots. A full block that is already available can still
   enter the independently byte-bounded queue without a request slot; if local
   admission pressure instead releases that payload, a later announcement or
   dependency scan reconsiders its hash. The safe TLC configuration uses
   fewer request slots than blocks and proves progress under per-block weak
   fairness instead of assuming the complete work universe fits at once.
   Admission deferral resets transport retry accounting so successful delivery
   under temporary local pressure cannot exhaust a network-failure retry
   budget.
7. **Do not materialize the durable dependency buffer.** Production obtains a
   deterministic sorted hash list while loading and releasing one candidate
   `BlockMessage` at a time. The queue coordinator's shared async mutex permits
   only one startup or replay-completion scan at once; selected hashes are
   loaded individually and moved, not cloned, into the byte-owning queue. `BufferScanResidency.tla` proves the
   single-payload and total-residency bounds and requires the historical
   all-candidates implementation to fail its negative-control configuration.

`scripts/check-cost-accounted-rho-block-admission.sh` runs TLC and Apalache over
the safe models, TLC over all exact expected counterexamples, the production
unit/property suite, the Kani arithmetic harnesses, and the Loom interleaving
model. The full umbrella discovers that gate automatically.

## Transport refinement adjacent to block admission

Transport resource safety is verified beside block admission but is not an
economic or consensus transition. The complete specification is
[P2P Transport Resource and Completion Semantics](node/transport-resource-lifecycle.md).
Its refinement ladder consists of:

1. `TransportPayloadResidency.tla`, which proves exact byte/item ownership,
   compressed-wire coverage, bounded actual residency, shared fanout, terminal
   release, and success only after remote completion.
2. `TransportConcurrency.tla`, which proves that the finite client
   pre-SETTINGS window, server HTTP/2 window, global item capacity, smaller
   handler execution limit, and decoded-message ceiling compose without
   refusal, resource rejection, or pre-reservation byte escape.
3. `TransportPeerLifecycle.tla`, which proves that initialization and live work
   retain mapped owners, ACKed work cannot be aborted by cleanup, and each
   parallel request uses its own immutable validation context.
4. Required unsafe configurations that reproduce count-only memory growth,
   compressed-wire undercount, eager chunk copies, enqueue-before-completion
   success, HTTP/2 `REFUSED_STREAM`, payload-item rejection, handler-limit
   bypass, initialization orphaning, active eviction, and shared-context
   misvalidation.
5. Production proptests and examples, a full TLS two-node integration suite,
   and Loom models for reservation arithmetic, fanout, completion ordering,
   ingress bursts, initialization, retirement, and validation interleavings.

Let $`P`$ denote `max-message-consumers`. The model parameters scale the
production relationship $`\mathit{Http2Limit} = \mathit{ItemLimit} =
\max(P, 100)`$ while separately requiring $`\lvert\mathit{Handling}\rvert \le
P`$. If $`M`$ is the decoded-message limit and $`B`$ the post-reservation
payload ceiling, the checked service-owned byte envelope is $`PM+B`$. The
payload byte ceiling remains an independent conjunct, so aligning item
capacity does not allow large retained payloads to exceed memory. $`M`$ is a
configurable resource-admission parameter rather than a protobuf capability
claim; a replacement decoder must refine the same finite envelope or acquire
reservations incrementally.

## Running the tools locally

```bash
# TLC (pinned jar, same release + sha256 as CI)
mkdir -p ~/.tla
curl -sSL -o ~/.tla/tla2tools.jar \
  https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar
echo "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88  $HOME/.tla/tla2tools.jar" | shasum -a 256 -c -

# Full gating frontier (what nightly CI runs)
TLA_TOOLS_JAR=~/.tla/tla2tools.jar bash scripts/ci/check-tla-invariants.sh

# One area, including its exact expected-violation configs, bounded JVM heap,
# bounded worker count, and repository-backed model-checker state
scripts/check-cost-accounted-rho-block-admission.sh

# Property tiers
PROPTEST_CASES=10000 cargo test -p casper --lib block_processing_queue

# Kani (requires cargo-kani)
cargo kani -p casper --harness successful_reservation_is_exact_and_bounded
cargo kani -p casper --harness failed_reservation_cannot_fit
```
