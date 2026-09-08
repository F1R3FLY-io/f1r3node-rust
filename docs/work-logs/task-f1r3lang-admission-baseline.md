---
task: rholang14-node-frozen-contract
status: review
handoff_status: ready
branch: feature/f1r3lang-mettail-only
next_steps:
  - Complete independent verification of the locally passing admission boundary
  - Decompose and connect the existing MeTTaIL neutral frontend and language provider
---

# F1R3Lang admission baseline handoff

## Delivered boundary

The [contract](../design/f1r3lang-frontend.md), source/ABI fixture and focused
integration suite identify the approved engine base, selected live engine
sources, MeTTaIL development snapshot and application source-route inventory.
Public parsing and execution behavior are unchanged. The node branch remains
isolated from other ongoing node refactors.

The MeTTaIL dependency is explicitly unfinished development work, not a clean
release pin. The source scanner is a conservative textual tripwire, not a proof
that every indirect parser call has been identified.

## Local verification

All commands ran in the isolated node worktree. Rust commands used one build
job, disabled incremental compilation, an 8 GiB hard memory limit and no swap.

| Check | Result | Log under `target/verification/` |
|---|---|---|
| Initial locked baseline test | Rejected stale engine lockfile before compilation | `f1r3lang-frontend-baseline-1.log` |
| Offline reconciliation and baseline suite | Passed, 7 tests | `f1r3lang-frontend-baseline-2.log` |
| Locked, offline baseline suite | Passed, 7 tests | `f1r3lang-frontend-baseline-3.log` |
| Focused strict Clippy, no dependency linting | Passed with warnings denied | `f1r3lang-frontend-baseline-clippy-1.log` |
| Admission diagram rendering | Passed under a 1 GiB cap | `f1r3lang-admission-diagram-2.log` |

Formatting and whitespace checks passed. These are local execution results,
not independent verification or proof of end-to-end node readiness.

The baseline already declared local legacy-parser patches and dependency
changes absent from its lockfile. Cargo reconciled those existing declarations
offline; no dependency manifest was changed. The resulting lockfile SHA-256 is
`b450f08a0189f6ee7e561bf1307df776396dce9e9937e689685544be86cbbeea`.
Legacy parsing remains a baseline build dependency until the separately gated
public cutover; this reconciliation does not activate or endorse a fallback.

## Prepared admission handoff

The [formal model and executable correspondence](../theory/f1r3lang-prepared-admission.md)
now cover negative budgets, frontend failure, metered initialization, merge
tracking, reducer invocation, error accounting and caller-owned rollback.
All 31 closed theorems compiled, followed by a separate successful kernel check,
each under a 1 GiB memory cap. The runtime extraction was implemented afterward.

The host API owns normalized `Par` values and reuses the existing funding,
reducer and stack-safe process lifecycle. It neither activates a new public
parser nor supplies the future neutral IR. Its ABI check and source preparation
are explicit; no global frontend setting was introduced.

| Admission check | Result | Log under `target/verification/` |
|---|---|---|
| New tests, initial build | Rejected missing mutable checkpoint fixture bindings | `prepared-program-admission-tests-1.log` |
| Corrected admission suite | Passed, 10 tests including depth 50,000 on a 128 KiB stack | `prepared-program-admission-tests-2.log` |
| Existing interpreter and frozen baseline | Passed, 5 and 7 tests respectively | `prepared-admission-existing-regressions-1.log` |
| Strict library and changed-test Clippy | Passed with warnings denied | `prepared-admission-clippy-1.log` |

The failed test build required only mutable fixture bindings; runtime behavior
was not changed to accommodate it. A nonunit signature and equal random state
are used for the source/prepared comparison. A deterministic failing receive
distinguishes raw admission's visible consumed-message effect from the existing
source wrapper's rollback. All test scratch files remain under `target/`.

The compiler reference moved from `interpreter.rs` into `frontend.rs`; the
baseline inventory records that reviewed move. Historical source witnesses and
live model/accounting digests were not relaxed. Formatting, whitespace checks
and the scoped read-only correspondence review passed. These remain local
implementation results, not trusted release-completion evidence.

## Guarded publication handoff

The [guarded publication contract](../design/f1r3lang-frontend.md#guarded-system-contract-publication)
now has a generic host implementation. Play and replay share their ordinary
produce mutation with a synchronous one-shot authority callback. Replay uses
an identity-based pending-count overlay during preparation; observers and
receiver dispatch follow guard release. The existing reporting interface is
retained. ContractCall preserves its ordinary entry and adds an explicitly
guarded producer entry.

The MeTTaIL `GuardedReplyPublication.v` control model was compiled and separately
kernel-checked before production implementation under a 1 GiB memory cap.
All 15 printed theorem contexts were closed. Its source SHA-256 is
`d83441345efdbf302444facf1af85997c1c08a195100813deec5f9d68e4a4040`.
It proves the publication control boundary, not arbitrary Rust guards, all
COMM semantics, signed-counter overflow behavior or subsequent receiver effects.

| Host check | Local result | Log under `target/verification/` |
|---|---|---|
| Guarded matching and publication | Passed, 22 tests, including the existing 20,000-bind stack test | `guarded-publication-rspace-tests-3.log` |
| Actual ContractCall future and receiver | Passed, matched and unmatched cases | `guarded-publication-contract-call-tests-2.log` |
| Existing replay and reporting | Passed, 24 replay and four reporting tests | `guarded-publication-replay-regressions-1.log` |
| Strict RSpace library and focused-test Clippy | Passed with warnings denied | `guarded-publication-rspace-clippy-2.log` |

The failed checks are retained for reproducibility. The first publication tests
incorrectly treated soft checkpoints as passive reads; they drain logs and
counters. They also compared a cache miss with a lazily cached empty channel.
The revised harness restores checkpoint observations and separately checks
cold-refusal committed roots. Its second failure compared cache-only maps after
play had checkpointed and replaced its hot cache; the final comparison uses
retained counters and complete committed roots. The initial ContractCall fixture
used a literal pattern with zero captures while expecting a captured argument;
it now uses the existing free-variable binding convention. No production
checks were weakened for these test corrections. Strict Clippy's five new
redundant-borrow findings were removed without lint suppression.

Read-only review also identified a malformed trusted guard reporting refusal
after mutation and an introduced replay payload clone. The implementation
distinguishes guard protocol violations from atomic refusals, tests both invalid
invocation cases, and moves original replay candidate fields after notification.

MeTTaIL wrapper correspondence and installed-language wire integration have
separate checks. This local host checkpoint does not establish an active public
frontend, an installed-table guard implementation, a runnable Regex application
or independently verified release readiness.

## Next implementation boundary

### Owned contract-call handoff

The owned contract-call entry now moves the singleton request payload, previous
output roster and caller random state. Its producer accepts an owned reply and
channel with a late-bound optional guard and uses the existing asynchronous
produce/dispatch body. The borrowed APIs retain their signatures and delegate
to it. The installed semantic service must supply its complete retained guard;
the ordinary unguarded host API does not confer that authority.

Before the Rust extraction, the upstream `OwnedContractCall.v` model passed
compilation with eight closed theorem contexts and a separate kernel check,
both capped at 1 GiB. Its source SHA-256 is
`4a241df44997de3ae59702d908b4a1fb41b69a8f85d746cd16e73d3ef3a1f4fa`.
It establishes the exact accepted split, preserved values/random state and
reuse of the publication machine, not physical clone freedom throughout RSpace.

| Owned transport check | Local result | Log under `target/verification/` |
|---|---|---|
| Initial focused test build | Rejected a new test moving text out of the custom-drop error type | `owned-contract-call-tests-1.log` |
| Corrected focused suite | Passed, five tests | `owned-contract-call-tests-2.log` |
| Node library Clippy, dependency linting disabled | Passed with warnings denied | `owned-contract-call-clippy-1.log` |

The test correction borrows the error text; production behavior was unchanged.
Tests cover allocation identity for request/previous vectors, ordered repeated
reply values, random bytes, outer arity, matched/unmatched guarded publication,
unpolled future disposal, revocation before polling, and callback access after
guard release. Both APIs exercise deterministic and nondeterministic success
and failure branches. The incoming replay flag is returned unchanged while
callbacks receive the actual space replay state and event-derived prior output.
Read-only source review found no blocking deviation from the shared future.
Rust checks used one job, an 8 GiB cap and no swap. This remains a transport
checkpoint, not a runnable installed-language application or public cutover.

### Remaining frontend integration

The existing MeTTaIL lowerer, neutral frontend extraction, shared language-service
provider and actual public-route activation remain distinct follow-on work.
This baseline does not finish them or the Regex application.
