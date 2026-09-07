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

## Next implementation boundary

The existing MeTTaIL lowerer, neutral frontend extraction, shared language-service
provider and actual public-route activation remain distinct follow-on work.
This baseline does not finish them or the Regex application.
