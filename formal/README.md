# Formal verification catalog

Casper-specific production bindings described below belong to
`feature/casper-cost-accounting-completion`. This branch retains abstract
cost-accounting models and the native RSpace/Rholang bindings; the removed
Casper tests and their implementation claims are not part of its gate.

This directory contains source models, mechanized proofs, executable search
oracles, and concurrency refinements. A file's existence is not treated as
verification evidence. The repository accepts a verified claim only when its
source is substantive, its configured tool run succeeds, its required unsafe
control produces the named counterexample, and implementation-level tests bind
the model transition to production code.

The process, proof ladder, and verified-area matrix are maintained in
[`docs/formal-verification.md`](../docs/formal-verification.md). Cost-accounting
claim identifiers and their proof/test artifacts are indexed in
[`docs/casper/theory/cost-accounted-rho-verification.md`](../docs/casper/theory/cost-accounted-rho-verification.md).

## Source families

| Directory | Role |
| --- | --- |
| [`tlaplus/`](tlaplus) | Concurrent protocol state machines, liveness properties, safe configurations, and required counterexample configurations |
| [`rocq/`](rocq) | Axiom-free algebraic and refinement proofs checked by Rocq |
| [`lean/`](lean) | Independent cost-monad and validator witnesses |
| [`isabelle/`](isabelle) | Independent cost-accounting refinement witnesses |
| [`iris/`](iris) | Separation-logic reconciliation witness |
| [`loom/`](loom) | Bounded Rust concurrency checks with explicit production and abstraction boundaries |
| [`mcrl2/`](mcrl2) | Finite process-algebra cross-witnesses |
| [`rewriting/`](rewriting) | Term-rewriting confluence and conservation witnesses |
| [`sage/`](sage) | Bounded scenario enumeration, adversarial search, and hypothesis falsification |
| [`z3/`](z3) | SMT refinements and bounded counterexample searches |
| [`wolfram/`](wolfram) | Licensed, opt-in symbolic-region, graph, recurrence, and optimization exploration; discoveries are promoted to authoritative proof and implementation layers |

## TLA+ file layout

Each area has at least one substantive protocol module, such as
[`tlaplus/block_admission/BlockAdmission.tla`](tlaplus/block_admission/BlockAdmission.tla).
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

## Host-work budget artifacts

The [host-work TLA+ package](tlaplus/host_work_budget/README.md) models deterministic resource limits for consensus host work.
It separates host-work units from Rholang costs and economic balances.

The permanent [host-work specification](../docs/casper/theory/host-work-budget.md) defines each production unit, reservation boundary, rollback rule, cache rule, and protocol activation requirement.

The model contains two validators, two shards, two concurrent branches, checkpoints, replay, and eight host-work phases.
Sixteen independent dimensions prevent weighted capacity transfer between unlike resources.
Ten unsafe controls expose arrival-order dependence, arithmetic defects, premature mutation, replay defects, shard coupling, economic debit, and premature allocation.

The [host-work Rocq package](rocq/host_work_budget/README.md) proves the unbounded arithmetic and refinement obligations.
Its theorems cover checked bounds, failure non-mutation, overflow rejection, economic separation, rollback, replay agreement, and independent-shard commutation.

Run [`scripts/check-host-work-budget-formal.sh`](../scripts/check-host-work-budget-formal.sh) to check the complete focused package.

## Cost-accounting refinement

The [cost-accounted Rho TLA+ models](tlaplus/cost_accounted_rho) cover native
admission, execution budgets, funding, settlement, and replay. The
[host-work package](tlaplus/host_work_budget/README.md) specifies independent
host-resource limits. The [Rocq theories](rocq/cost_accounted_rho/theories)
establish algebraic and arithmetic obligations, and the
[Loom crate](loom/cost_accounting/README.md) checks bounded interleavings.

The models are specifications, not evidence that deferred Casper production
paths implement them. The Casper production bindings and their integration
checks remain on `feature/casper-cost-accounting-completion`. This branch's
production refinement is through the Rholang interpreter and native RSpace
interfaces.

## Deterministic reducer concurrency

[`tlaplus/deterministic_parallel_reduction/DeterministicParallelReduction.tla`](tlaplus/deterministic_parallel_reduction/DeterministicParallelReduction.tla)
models a complete intra-deploy communication frontier, transitive channel and
linear-authority conflict components, canonical conflicting commitment, and
parallel execution of truly disjoint work. Its five unsafe configurations
independently remove the complete frontier, canonical order, checkpoint
quiescence, disjoint parallelism, or authority-region conflicts.

[`tlaplus/deterministic_parallel_reduction/EvaluationBoundary.tla`](tlaplus/deterministic_parallel_reduction/EvaluationBoundary.tla)
models structured cancellation separately. Root cancellation aborts all child
tasks. A shared evaluation permit remains owned until all children terminate.
The unsafe configuration detaches the children and checkpoints before their
mutations complete.

[`tlaplus/deterministic_parallel_reduction/ReductionDriverLifecycle.tla`](tlaplus/deterministic_parallel_reduction/ReductionDriverLifecycle.tla)
models one driver per complete frontier and release before result delivery.
[`tlaplus/deterministic_parallel_reduction/SingleParticipantFastPath.tla`](tlaplus/deterministic_parallel_reduction/SingleParticipantFastPath.tla)
models direct execution after exactly one live participant remains.

TLC exhausts all finite state spaces. Apalache independently checks each
bounded horizon and reproduces the targeted defect traces.

The unbounded algebraic refinement is
[`rocq/cost_accounted_rho/theories/DeterministicParallelReduction.v`](rocq/cost_accounted_rho/theories/DeterministicParallelReduction.v).
It proves disjoint commutation, compound-authority overlap independent of
encoding order, the canonical minimal counterexample, causal path monotonicity,
and cancellation/checkpoint exclusion without assumptions. Bounded Rust memory
schedules are exhausted by
[`loom/cost_accounting/tests/loom_deterministic_reduction_frontier.rs`](loom/cost_accounting/tests/loom_deterministic_reduction_frontier.rs).
The production contract and regression mapping are documented in
[`docs/casper/theory/cost-accounting-impl/deterministic-parallel-reduction.md`](../docs/casper/theory/cost-accounting-impl/deterministic-parallel-reduction.md).

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
