# Economic failure observation

## Purpose and compatibility

An evaluation can report several failures from parallel processes.
The legacy reducer selects one public error, or an aggregate error, according to its existing precedence rules.
A user abort can hide a simultaneous platform failure in that public result.
The economic policy cannot use that result alone to authorize a charge.

The [ratified economic policy](economic-activation-policy-ratification.md) permits an authorized charge after a user-only failure.
Platform, certificate, and unclassified failures prevent economic publication.
The observation layer retains every observed failure class before the legacy reducer selects its public result.
It does not change the legacy error precedence, wire format, wallet balances, or consensus rules.

`EvaluateResult.economic_failures` is an owned summary of one evaluation.
The summary is necessary evidence for native funding settlement, not a complete authorization certificate.
Native settlement must also authenticate the realized resources, signed controls, captured funding sources, and selected outcome.

## Failure classes

The classifier uses error variants, not error messages.
The match covers every `InterpreterError` variant, so a new variant requires an explicit classification.

| Class | Typed cases | Economic meaning |
| --- | --- | --- |
| User | Explicit abort, undefined method or operator, argument-count mismatch, operand-type mismatch, and invalid condition type | May retain an otherwise authorized charge. |
| Platform | Storage failures, host-work rejection, internal defects, missing required protobuf fields, decoding failures, external-service failures, and nondeterministic replay failures | Prevent economic publication. |
| Certificate | Interpreter or RSpace phlogiston exhaustion | Prevent economic publication under the certified sufficient-funding contract. |
| Unclassified | Mixed-purpose errors such as `ReduceError`, parser errors, substitution failures, and remaining explicitly matched variants | Prevent economic publication until a more precise typed contract exists. |

Phlogiston exhaustion alone does not establish a certificate defect in legacy execution or a preacceptance trial.
The certificate interpretation requires accepted execution under a certified sufficient bound.
The new classification does not alter historical exhaustion handling.

An empty input error list means no observed failure.
An explicit `AggregateError` with no children means an unclassified failure.
A platform wrapper retains its platform class even when its nested cause is a user abort.

## Parallel collection and ownership

Each root `ReductionSession` owns a fresh atomic failure recorder.
Child reduction contexts share that recorder.
The recorder stores four bits, one for each failure class.
Atomic bitwise OR combines reports without a global lock or an ordering requirement.

Let $`F_i`$ be the failure-class set from report $`i`$.
Let $`F`$ be the summary after all relevant child tasks complete.

```math
F = \bigcup_i F_i
```

The union is associative, commutative, and idempotent.
Report order, grouping, and duplication cannot remove a failure class.
An authorized retained charge requires $`F \subseteq \{\mathrm{User}\}`$.
This condition does not independently establish authorization or a successful outcome.

The reducer records aggregate inputs before it selects the legacy error.
The interpreter entry point also records a direct reduction error before it closes the root evaluation.
After the evaluation completes, the interpreter copies the summary into its result.
Later reports cannot mutate that owned result.

A later evaluation receives a different recorder, even if both evaluations use the same deployment identifier.
The implementation does not reset a recorder that earlier tasks can still reference.
Parse failures, invalid initial phlo, and early host-work failures receive independent summaries without reading an earlier reduction session.

## Bounded observation

The classifier traverses nested errors iteratively with borrowed slices.
It reserves one verification operation for each visited error.
When traversal must retain a pending sibling slice, it reserves search-state bytes before allocating stack capacity.

For a metered root, the observation quota has the same limits as the supplied host-work budget but separate counters.
Observation must not exhaust the execution budget or change the legacy public error.
If observation exceeds its quota or cannot allocate, the recorder adds a platform failure.
This incomplete observation cannot authorize retained charges.
An unmetered legacy root has no observation quota.

The algorithm is:

```text
For each observed error:
    Reserve observation work.
    Add the class of its typed variant.
    Traverse nested causes and aggregate children.
If observation fails:
    Add the platform class.
Atomically combine the resulting classes with the session recorder.
Continue the existing legacy error selection.
```

## Verification boundaries

[`EconomicFailureSummary.v`](../../../../formal/rocq/cost_accounted_rho/theories/EconomicFailureSummary.v) proves union laws, bit encoding, completed update histories, and retained-charge refinement.
It also proves session isolation, non-user veto preservation, and the counterexample to legacy user-abort priority as an economic classifier.
These proofs assume correct variant mapping and complete reporting before the result snapshot.
They do not prove native source authentication, all reducer call paths, or wallet publication.

The Rust tests check all summary masks and pairs, typed variants, nested error forests, report duplication, and quota boundaries.
Loom tests exercise the production generic recorder with Loom atomics under concurrent schedules.
Reducer tests cover mixed failures before aggregation, nested aggregates, parallel child aggregation, and separate execution and observation budgets.
Runtime tests cover result propagation and isolation across aborts, parse failures, invalid initial phlo, host-work rejection, and success.

Source boundaries:

- [Classifier and atomic recorder](../../../../rholang/src/rust/interpreter/accounting/economic_failure.rs)
- [Reduction-session ownership](../../../../rholang/src/rust/interpreter/deterministic_reduction.rs)
- [Reducer regression tests](../../../../rholang/src/rust/interpreter/reduce_economic_failure_tests.rs)
- [Classification, property, and Loom tests](../../../../rholang/src/rust/interpreter/accounting/economic_failure/tests.rs)
- [Failure-policy rationale](economic-failure-policy-decisions.md)
