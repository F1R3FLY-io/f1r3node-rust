# Formal verification catalog

This directory contains source models, mechanized proofs, executable search
oracles, and concurrency refinements. A file's existence is not treated as
verification evidence. The repository accepts a verified claim only when its
source is substantive, its configured tool run succeeds, its required unsafe
control produces the named counterexample, and implementation-level tests bind
the model transition to production code.

The process, proof ladder, and verified-area matrix are maintained in
[`docs/formal-verification.md`](../docs/formal-verification.md). Cost-accounting
claim identifiers and their proof/test artifacts are indexed in
[`docs/theory/cost-accounted-rho-verification.md`](../docs/theory/cost-accounted-rho-verification.md).

## Source families

| Directory | Role |
| --- | --- |
| [`tlaplus/`](tlaplus) | Concurrent protocol state machines, liveness properties, safe configurations, and required counterexample configurations |
| [`rocq/`](rocq) | Axiom-free algebraic and refinement proofs checked by Rocq |
| [`lean/`](lean) | Independent cost-monad and validator witnesses |
| [`isabelle/`](isabelle) | Independent cost-accounting refinement witnesses |
| [`iris/`](iris) | Separation-logic reconciliation witness |
| [`loom/`](loom) | Exhaustive Rust concurrency shadow models tied to named production transitions |
| [`mcrl2/`](mcrl2) | Finite process-algebra cross-witnesses |
| [`storm/`](storm) | Parametric and finite-state stochastic reliability projections over declared implementation envelopes |
| [`rewriting/`](rewriting) | Term-rewriting confluence and conservation witnesses |
| [`sage/`](sage) | Bounded scenario enumeration, adversarial search, and hypothesis falsification |
| [`z3/`](z3) | SMT refinements and bounded counterexample searches |
| [`wolfram/`](wolfram) | Licensed, opt-in symbolic-region, graph, recurrence, and optimization exploration; discoveries are promoted to authoritative proof and implementation layers |

## TLA+ file layout

Each area has at least one substantive protocol module such as
[`tlaplus/block_admission/BlockAdmission.tla`](tlaplus/block_admission/BlockAdmission.tla),
[`tlaplus/block_admission/TransportPayloadResidency.tla`](tlaplus/block_admission/TransportPayloadResidency.tla),
[`tlaplus/block_admission/TransportConcurrency.tla`](tlaplus/block_admission/TransportConcurrency.tla),
and
[`tlaplus/block_admission/TransportPeerLifecycle.tla`](tlaplus/block_admission/TransportPeerLifecycle.tla).
Files named `MC_*.tla` that contain only a module declaration and `EXTENDS` are
intentional TLC instance wrappers. Their constants, invariants, temporal
properties, and safe/unsafe selection live in the adjacent `.cfg` file; the
wrapper imports the substantive protocol module. A thin wrapper is therefore
not an empty model, but it is also not counted as the model's substance.

The source-integrity gate
[`scripts/check-cost-accounted-rho-formal-source-substance.sh`](../scripts/check-cost-accounted-rho-formal-source-substance.sh)
rejects every tracked or untracked repository artifact under `formal/` that is
empty or whitespace-only, malformed TLA+ modules and
configs, unresolved thin wrappers, and proof escape hatches in Rocq, Lean, and
Isabelle.

## Consensus concurrency artifacts

The finalized-floor family includes
[`tlaplus/finalized_floor/ObjectiveEquivocation.tla`](tlaplus/finalized_floor/ObjectiveEquivocation.tla),
which separates parallel stale-snapshot validation from serialized durable DAG
insertion. Its unsafe controls cover unary arrival-order evidence,
local-invalid-dependent acceptance, incomplete dependency closure,
equivocator voting, incarnation and epoch confusion, unary-fallback scope,
post-state authority, missing duplicate repair, and volatile restart state.
The unbounded algebraic obligations are in
[`rocq/finalized_floor/theories/ObjectiveEquivocation.v`](rocq/finalized_floor/theories/ObjectiveEquivocation.v),
and the implementation-level interleavings are in
[`loom/cost_accounting/tests/loom_objective_equivocation.rs`](loom/cost_accounting/tests/loom_objective_equivocation.rs).
The local and CI entry point is
[`scripts/check-finalized-floor-ALL.sh`](../scripts/check-finalized-floor-ALL.sh).

Protocol-6 finalized-floor evidence is split into four refinement boundaries.
[`tlaplus/finalized_floor/CertifiedFloorCommitment.tla`](tlaplus/finalized_floor/CertifiedFloorCommitment.tla)
checks target-bound signed commitment and receiver admission, while
[`tlaplus/finalized_floor/FinalizationCertificateRetrieval.tla`](tlaplus/finalized_floor/FinalizationCertificateRetrieval.tla)
checks typed bounded sidecar retrieval, failed-send retention, validated
content-addressed persistence, crash reconstruction, and one-time wakeup.
[`tlaplus/finalized_floor/DependencyMaintenanceRound.tla`](tlaplus/finalized_floor/DependencyMaintenanceRound.tla)
checks the production caller boundary: every block and certificate obligation in
one frozen maintenance snapshot is attempted before the caller returns its first
dispatch error. Their
unbounded contracts are
[`rocq/finalized_floor/theories/CertifiedFloorCommitment.v`](rocq/finalized_floor/theories/CertifiedFloorCommitment.v)
and
[`rocq/finalized_floor/theories/FinalizationCertificateRetrieval.v`](rocq/finalized_floor/theories/FinalizationCertificateRetrieval.v),
with the caller-level induction in
[`rocq/finalized_floor/theories/DependencyMaintenanceRound.v`](rocq/finalized_floor/theories/DependencyMaintenanceRound.v).
[`tlaplus/finalized_floor/WitnessEquivalentCarrier.tla`](tlaplus/finalized_floor/WitnessEquivalentCarrier.tla)
separately verifies that divergent honest witness digests remain interoperable
only when the accepted carrier commits the exact predecessor floor and replay
state, and that selection preserves the carrier block/digest pair. Its unbounded
refinement is
[`rocq/finalized_floor/theories/WitnessEquivalentCarrier.v`](rocq/finalized_floor/theories/WitnessEquivalentCarrier.v).
Together with the protocol-5 cost and validator-incarnation core, these artifacts
form the protocol-6 proof portfolio without duplicating the established core
transition system.

[`tlaplus/finalized_floor/ObjectiveEvidenceAuthorization.tla`](tlaplus/finalized_floor/ObjectiveEvidenceAuthorization.tla)
isolates the authorization boundary that the broader convergence model does
not abstract faithfully enough to prove on its own. It gives replicas opposite
delivery orders across an old generation, an old activation epoch, and two
current eligible siblings. The safe model requires generation and epoch
filtering before pair selection, positive bond and generation data from one
canonical merged-pre-state root, pair-only activation, fault-key-scoped unary
suppression, and one proposer/receiver predicate. Seven controls independently
restore each discovered or hypothesized defect. The corresponding unbounded
authority and predicate lemmas are part of
[`rocq/finalized_floor/theories/ObjectiveEquivocation.v`](rocq/finalized_floor/theories/ObjectiveEquivocation.v),
and the concrete interleavings are checked by the same Loom and Rust suites
listed above.

[`tlaplus/finalized_floor/CertifiedObjectiveEquivocation.tla`](tlaplus/finalized_floor/CertifiedObjectiveEquivocation.tla)
models sender certification, durable metadata, secondary evidence indexing,
crash, duplicate retry, and reconciliation as separate concurrent transitions.
Its signed-sequence boundary configuration persists a certified negative-
sequence rejection while excluding it from objective-pair evidence on both
replicas; the adjacent unsafe configuration removes that eligibility gate and
must violate `Inv_IneligibleSequenceNeverBecomesEvidence`. The unbounded
refinement is
[`rocq/finalized_floor/theories/ObjectiveEvidenceSequenceEligibility.v`](rocq/finalized_floor/theories/ObjectiveEvidenceSequenceEligibility.v),
the implementation property spans every `i32` sequence, and the concurrent
repair paths are checked by
[`loom/cost_accounting/tests/loom_objective_evidence_sequence_boundary.rs`](loom/cost_accounting/tests/loom_objective_evidence_sequence_boundary.rs).

[`tlaplus/deploy_recovery/DeployRecovery.tla`](tlaplus/deploy_recovery/DeployRecovery.tla)
models independently lagging validator views and candidate-relative deploy
authorization. Its rehome invariant requires a deploy selected outside the
current parent closure to survive a historical self-chain scan; the pre-fix
configuration reproduces the stale-filter liveness failure. Rocq proves the
classifier in
[`rocq/finalized_floor/theories/CandidateScopeDeployRehome.v`](rocq/finalized_floor/theories/CandidateScopeDeployRehome.v),
and three memory-order refinements live in
[`loom/cost_accounting/tests/loom_candidate_scope_deploy_rehome.rs`](loom/cost_accounting/tests/loom_candidate_scope_deploy_rehome.rs).

[`tlaplus/finalized_floor/StaleSiblingRecovery.tla`](tlaplus/finalized_floor/StaleSiblingRecovery.tla)
composes the boundary that the parent-projection and occurrence-recovery models
previously checked separately. It interleaves three validators from accepted
siblings through floor advancement, exact-frontier settlement, source-bound
tombstone observation, rejected-occurrence buffering, elected recovery, and
converged finalization. TLC exhausts the complete fair state graph; Apalache
checks the end-to-end safe path and seven defect controls. Rocq proves the
unbounded sequential refinement in
[`rocq/finalized_floor/theories/StaleSiblingRecovery.v`](rocq/finalized_floor/theories/StaleSiblingRecovery.v),
and the weak-memory refinement is part of
[`loom/cost_accounting/tests/loom_consensus_projection_freeze.rs`](loom/cost_accounting/tests/loom_consensus_projection_freeze.rs).

[`tlaplus/finalized_floor/FinalizerFloorMaterialization.tla`](tlaplus/finalized_floor/FinalizerFloorMaterialization.tla)
closes the durable-finalizer discovery seam exposed when the proposal floor is
carried only through secondary-parent evidence. It models independent delivery
to two nodes, strict weighted certification, a state-rejected sibling, proposal
deferral, complete all-parent discovery, exact target binding, and local
materialization. Main-parent-only and causal-only configurations are mandatory
counterexamples. The unbounded refinement is
[`rocq/finalized_floor/theories/FinalizerFloorMaterialization.v`](rocq/finalized_floor/theories/FinalizerFloorMaterialization.v),
the concrete selector is compared with an exhaustive pairwise oracle, and the
frozen-target/latest-message race is checked by
[`loom/cost_accounting/tests/loom_finalization_atomicity.rs`](loom/cost_accounting/tests/loom_finalization_atomicity.rs).

Protocol-v5 block readiness is specified independently in
[`tlaplus/slashing/ProtocolV5DependencyReadiness.tla`](tlaplus/slashing/ProtocolV5DependencyReadiness.tla).
Its seven dependency identities represent a parent, a justification, historical
unary slash evidence, both members of objective slash evidence, and both members
of header-certified evidence. The safe configuration exhausts concurrent
metadata, invalid-index, tracker, direct-resolver, and buffer-resolver
interleavings. Four unsafe configurations each restore one historical or
hypothesized defect. Universal list and set obligations are discharged by
[`rocq/slashing/theories/ProtocolV5DependencyReadiness.v`](rocq/slashing/theories/ProtocolV5DependencyReadiness.v),
and memory-order interleavings are checked by
[`loom/cost_accounting/tests/loom_protocol_v5_dependency_readiness.rs`](loom/cost_accounting/tests/loom_protocol_v5_dependency_readiness.rs).
The executable gate is
[`scripts/check-slashing-ALL.sh`](../scripts/check-slashing-ALL.sh).

The fork-choice family treats validator and frontier concurrency as first-class.
[`tlaplus/fork_choice/GhostTerminalFrontier.tla`](tlaplus/fork_choice/GhostTerminalFrontier.tla)
explores every expansion order of a multi-parent DAG containing the aggregate-subtree
counterexample. Its unsafe configuration restores global terminal-leaf selection and
must violate the greedy-head invariant. The unbounded structural obligations are
proved in
[`rocq/fork_choice/theories/TerminalFrontier.v`](rocq/fork_choice/theories/TerminalFrontier.v),
and production-backed randomized, pinned, and diamond regressions live in
[`casper/tests/fork_choice/prop_ghost_argmax.rs`](../casper/tests/fork_choice/prop_ghost_argmax.rs).
The local entry point is
[`scripts/check-fork-choice-ALL.sh`](../scripts/check-fork-choice-ALL.sh).

## Long-horizon uptime evidence

[`storm/uptime/`](storm/uptime) evaluates a 720-hour service-live predicate
around the hard consensus, replay, accounting, admission, and ownership
authorities. Checked-in engineering profiles expose every assumption and never
substitute validator count for unequal stake. A historical backtest binds the
last reconstructable long daily soak to its workflow, node, and
system-integration revisions, then holds its aggregate outcome out from the
preceding observations. Because that soak rebuilt each shard and its detailed
artifact expired, it cannot calibrate continuous lifetime rates; the gate
records that non-identifiability rather than filling it with a point estimate.

[`mcrl2/uptime/`](mcrl2/uptime) verifies that independent shard replay and
validation can overlap and that the safe service interface is deadlock-free.
The global-mutex control must fail the overlap property. Optional
[`wolfram/uptime/`](wolfram/uptime) exploration minimizes only over exported
Storm results and cannot strengthen a proof or certify a release.
[`tlaplus/uptime/UptimeEnvelopeDominance.tla`](tlaplus/uptime/UptimeEnvelopeDominance.tla)
proves the event-coupling order that makes the adverse and favorable Storm
corners genuine bounds within the declared rate box; its unsafe control adds a
favorable-only failure and must violate the order.

[`scripts/check-uptime-ALL.sh`](../scripts/check-uptime-ALL.sh) is the local
entry point. It writes a machine-readable engineering envelope and a
human-facing report below `target/verification/uptime/`, with exact Git,
implementation, model, and profile identities. Any relevant change produces a
different identity. A calibrated release projection additionally requires a
clean implementation and a current profile conforming to
[`storm/uptime/profiles/calibrated-profile.schema.json`](storm/uptime/profiles/calibrated-profile.schema.json).

## Generated files

Rocq compiler products such as `.vo`, `.vok`, `.vos`, `.glob`, and `.aux` are
ignored build artifacts. Some tools create zero-length marker files while a
proof is being compiled; those files are not source, are not committed, and are
never accepted as proof evidence. Verification output belongs under
`target/verification/`, not under this source tree or `/tmp`.

## Completion criterion

A formal area is incomplete if any one of the following is missing:

1. a substantive model or proof source;
2. a checked safe configuration and its declared properties;
3. a checked unsafe control for each historical or hypothesized defect;
4. a model-to-code map naming the production transition;
5. example, property, and concurrency tests appropriate to that transition;
6. a gate that executes the artifacts without skipped mandatory tools; or
7. documentation that states the bounded assumptions and the evidence actually
   obtained.

This criterion prevents a model that merely describes intended behavior from
being presented as verification of an implementation that has not refined it.
