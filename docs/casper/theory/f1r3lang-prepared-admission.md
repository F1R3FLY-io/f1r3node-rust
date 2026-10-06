# Prepared-program admission proof

## Claim and scope

[PreparedProgramAdmission.v](../../formal/rocq/cost_accounted_rho/theories/PreparedProgramAdmission.v)
proves operation-level equivalence between the current source admission flow and
its proposed prepared-program extraction. The result applies to the host
handoff, not to the correctness of a parser, language theory, funding engine or
complete node. See the [frontend contract](../design/f1r3lang-frontend.md) for the
integration boundary and unactivated public routes.

An *operation tree* is a finite inductive description of host calls and their
continuations. For example, `ReadSignature` takes a continuation indexed by the
signature the runtime supplies. The theorem compares both flows for every
possible response to their corresponding operations. It does not assume those
responses are correct or introduce a second implementation of the accounting
engine.

A *trace* records the ordered calls and selected observations of a normal-return
execution. The proof is about these calls, their arguments and returned results.
It does not establish termination of an arbitrary reducer or preservation of
timing, tracing metrics, cancellation or concurrent-reset behavior.

## Model-to-source correspondence

The types for processes, random states, signatures, diagnostics, finalized costs
and host state are universally quantified. They have no assumed algebraic laws.
The theorem is therefore independent of a particular process representation;
it must not be read as a proof of canonical protobuf bytes.

| Model operation or value | Required Rust correspondence |
|---|---|
| `PrepareSource` | One `prepare_program` call with the exact source and environment; ABI rejection precedes the provider's `prepare` call |
| `PreparationFailed` | A distinct preparation error mapped to zero-cost `ParserError`, never generic stale-budget accounting |
| `ReadSignature` | `self.c.signature()` after successful preparation; not a frontend-supplied signature |
| `metered` | `SignedProcess::Par(Signed(...), Token(...))` constructed by `SignedProcess::metered` |
| `InitializeBudget` | The unchanged `reset_from_signed_process` on the runtime's shared budget |
| `process_projection` | Denotational identity of the process moved out through `into_source_process` |
| `ClearMerge` | Clear stored merge-channel tracking after budget initialization |
| `InvokeReducer` | Exactly one top-level `reducer.inj` with the prepared process and unchanged caller random state |
| `ReadMerge` | Snapshot stored merge channels on successful reduction, before cost finalization |
| `FinalizeCost` | The existing `total_cost()`, including reconciliation effects |
| `InvalidBudgetCost` | `Cost::create(0, "invalid initial phlo")` |
| `PreparationCost` | `Cost::create(0, "parse failure")`, including the existing direct operator-error cases |

These bindings refer to
[interpreter.rs](../../rholang/src/rust/interpreter/interpreter.rs),
[accounting/mod.rs](../../rholang/src/rust/interpreter/accounting/mod.rs) and
[rho_runtime.rs](../../rholang/src/rust/interpreter/rho_runtime.rs).
The model is a refinement specification for their extraction, not an automatic
proof that arbitrary Rust code implements the table.

The ABI check is part of the abstract preparation operation.
`source_prepares_exactly_once` counts that operation, not invocations of an
incompatible provider: an unsupported version invokes only `abi_version`,
not `prepare`. Both version and provider failures use the same preparation
error branch. A negative budget invokes neither method.

The existing [runtime-budget model](../../formal/rocq/cost_accounted_rho/theories/RuntimeBudgetRefinement.v)
provides separate accounting results. This admission proof does not import its
large syntax dependency closure or redefine its reconciliation algorithm.
Instead, both compared flows invoke the same host operations. In particular,
budget reset preserves unmetered mode; the admission extraction must not
silently change that policy or claim that a metered wrapper changes the mode.

## Closed obligations

`extracted_equals_monolithic` proves equality of the operation trees.
`extracted_trace_equivalence` transfers that equality to all normal-return
traces and results. Neither proof requires functional extensionality or another
axiom.

For a nonnegative budget, `source_trace_shape` splits the complete source entry
into preparation failure or successful handoff, and
`source_prepares_exactly_once` counts exactly one preparation event.
`negative_source_has_no_events` covers rejection before preparation.
The negative check remains before frontend invocation in the Rust source entry.

For successful prepared admission, `admitted_trace_shape` establishes the
prefix: runtime signature read, initialization with that signature and budget,
merge clearing, and reducer invocation with the exact prepared process and
caller random state. Its remaining events can only be the original merge/cost
finalization calls. `admitted_once_without_reparse` derives zero further
preparations, one initialization and one invocation.

`metered_token_exact` and `metered_process_exact` establish the projections
of the actual two-branch wrapper shape, including a zero-token budget.
`nonnegative_budget_roundtrip` establishes the integer conversion law.
The Rust budget is an `i64`; its nonnegative range fits the wrapper's `u64`
count. General signed-tree disposal and physical move ordering remain Rust
correspondence obligations.

## Errors and state framing

The error model preserves the current top-level classification:

| Failure | Cost operation | Returned errors |
|---|---|---|
| Direct parser, undefined operator or expected operator | None; return zero cost | Original error, once |
| Aggregate | Existing cost finalization | Exact member vector, including an empty vector |
| Located or other error | Existing cost finalization | Original outer error, once |

`LocatedFailure` abstracts away the location path: the theorem establishes
classification, not complete diagnostic fidelity. All error cases return the
empty merge map. Success returns the sampled stored merge map. Existing
`InterpreterImpl` handling returns an inner evaluation result for every such
error; outer runtime errors are modeled separately for the generic wrapper.

`replay_trace` gives preparation an explicit frame: it preserves budget,
stored merge tracking and RSpace, while the other host operations use an
arbitrary interpretation. Empty traces are identity.
`preparation_failure_preserves_host_state` checks the frame for the sole
preparation event. This framing is part of the model's operation interpretation;
the implementation must establish frontend purity through its API and tests.
The trace interpretation does not independently prove that observed signatures,
merge maps or cost values reflect actual runtime state.

## Ownership and checkpoints

A prepared slot is either ready or consumed. Borrowing leaves it ready;
consuming makes it unavailable. `prepared_dispatch` couples consumption to
emission of the exact `prepared_handoff` operation tree.
`dispatched_execution_once` then connects ownership to the invocation count.
A consumed slot cannot dispatch again.

This is logical single-use per modeled slot, not proof of global uniqueness,
Rust memory safety or absence of physical copies. The Rust correspondence must
use a non-`Clone` owner, a borrowing display accessor and a consuming process
accessor. Dropping a rejected prepared process must retain the existing
stack-safe destruction discipline.

The wrapper model restores RSpace when the returned error vector is nonempty or
when the generic runtime returns an outer error. It preserves the final budget
and stored merge tracking. It deliberately proves the counterexample
`empty_aggregate_does_not_rollback`: an empty aggregate produces no returned
errors and therefore does not trigger the wrapper's revert branch.

These theorems cover post-result restoration. Checkpoint creation, convenience
wrapper random-state generation and their order remain unchanged source
operations. Raw admission gains no implicit checkpoint or blanket rollback
guarantee.

## Executable correspondence checks

The [admission regression suite](../../rholang/tests/prepared_program_admission_spec.rs)
checks the extracted boundary with the existing evaluator. A counting frontend
checks exact source/environment inputs and exactly one preparation; negative
budgets and incompatible ABIs reject before the relevant provider calls.
Preparation failures follow a real charged deployment: the previous budget,
signature, cost log, stored merge map and RSpace root remain unchanged.

The source/prepared corpus compares costs, diagnostics, merge results and
checkpoint roots with the same nonunit deployment signature and random state.
Fixtures cover empty processes, sends, private names, communication, token
exhaustion and reducer errors. A failed receive demonstrates that raw prepared
admission leaves its consumed-message effect visible, while the existing
source convenience wrapper restores RSpace. Both retain the existing
accounting and merge-tracking behavior.

The lifecycle test builds and disposes 50,000 nested process nodes on a 128 KiB
native stack, both with and without consuming the artifact first. It reuses
the generated `Par` destructor. A shallow allocation-address and exact
protobuf-byte check verifies borrowing and moving without cloning. These are
concrete regressions, not universal Rust memory proofs or substitutes for
neutral-IR/canonical-byte conformance.

The first test build rejected missing mutable bindings in checkpoint fixtures;
correcting them changed no runtime logic. All ten focused tests subsequently
passed, as did the seven frozen-contract and five existing interpreter tests.
Logs are `target/verification/prepared-program-admission-tests-2.log` and
`target/verification/prepared-admission-existing-regressions-1.log`.

```sh
mkdir -p target/test-tmp
systemd-run --user --scope --quiet \
  -p MemoryMax=8G -p MemoryHigh=7680M -p MemorySwapMax=0 \
  env CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 TMPDIR="$PWD/target/test-tmp" \
  cargo test --locked --offline -p rholang \
  --test prepared_program_admission_spec --test interpreter_spec \
  -- --test-threads=1
```

These are local results. Independent completion verification and public
frontend activation remain separate gates.

Strict Clippy passed for the changed library and admission/baseline tests with
warnings denied; the log is `target/verification/prepared-admission-clippy-1.log`.
Formatting and whitespace checks also passed. The independent read-only review
found no blocking code/proof correspondence defect; that review does not replace
execution evidence or the separate end-to-end release gate.

## Reproduction and bounded compilation

Run from the node workspace root:

```sh
mkdir -p target/verification/prepared-admission
systemd-run --user --scope --quiet \
  -p MemoryMax=1G -p MemoryHigh=900M -p MemorySwapMax=0 -p TasksMax=8 \
  coqc -q -Q target/verification/prepared-admission CostAccountedRho \
  -o target/verification/prepared-admission/PreparedProgramAdmission.vo \
  formal/rocq/cost_accounted_rho/theories/PreparedProgramAdmission.v
systemd-run --user --scope --quiet \
  -p MemoryMax=1G -p MemoryHigh=900M -p MemorySwapMax=0 -p TasksMax=8 \
  coqchk -silent -Q target/verification/prepared-admission CostAccountedRho \
  CostAccountedRho.PreparedProgramAdmission
```

The output directory needs the logical package mapping because `coqc -o`
derives the compiled module's name from that directory. All proof artifacts
remain under `target/`. The source is also registered in the package's
`_CoqProject`; this focused command does not build its other modules.

Every listed theorem has a `Print Assumptions` check. A successful run must
report closed global contexts, with no admissions, axioms or disabled checking.
Keep the separate kernel-check exit status alongside the compilation log.

Local verification passed for all 31 theorems with Rocq 9.1.1, followed by a
successful separate kernel check. Logs are
`target/verification/prepared-admission-compile-6.log` and
`target/verification/prepared-admission-kernel-check-2.log`. Both operations ran
under the limits above. A read-only semantic review found no blocking defect in
this scoped model after the correspondence refinements described below.

## Proof-development notes

The initial binder names `left` and `right` collided with imported Rocq
constructors and were replaced by `first` and `second`. Destructing the budget
comparison already simplified both branches, so an additional rewrite was
removed. An explicit theorem application was needed for the trace-shape
parameters. The source-count corollary uses the exact successor form of the
count rather than unfolding away its reusable lemma. None of these changes
weakened a theorem.

The semantic review added the runtime signature-read operation, full-source
trace connection, ownership-to-dispatch relation and explicit state frame before
runtime extraction. Compilation and kernel checking remain distinct from that
read-only semantic review.
