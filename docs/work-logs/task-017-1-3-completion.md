# Casper Preparation Completion Review

## Scope and authorization

The user requested completion of TASK-017-1 through TASK-017-3. This review closes planning, interface specification, selected prerequisite review, and initial candidate identification.

The starting revision is `8870a74d3a0416233210738cd85b88f697277080`. The starting worktree and index were clean.

The earlier scope review established harness-only obligations and separate pre-merge and post-merge phases. The user approved the parallel implementation sequence on September 17.

Those approvals do not authorize node execution, protocol activation, external repins, profile acceptance, or Git publication.

## Acceptance review

### TASK-017-1

| Criterion | Evidence and disposition |
| --- | --- |
| Map D-01 through D-12 | The [branch plan](../plans/casper-ratified-soak-2026-09-16.md) contains twelve decision rows with task owners and activation limits. |
| Separate baseline, ratification, and candidate sources | The plan records exact historical revisions. The new metadata observations below remain separate from selected source identities. |
| Record overlaps and conflicts | The [planning history](casper-ratified-soak-2026-09-16.md) records related epics, legacy specification conflicts, and parser limits. |
| Review scope before implementation | The planning history records the maintainer's scope correction and separate post-merge request. The plan records subsequent implementation approval. |

The plan now distinguishes current completion from historical scaffold counts. Runtime repairs, node correctness, and Rocq construction remain outside this branch's obligations.

### TASK-017-2

| Criterion | Evidence and disposition |
| --- | --- |
| Inputs, outputs, assumptions, bounds, controls, and fixtures | The [interface contract](../casper/design/soak-interface-contract.md), eight claim specifications, and formal plan define these records. |
| Ratified expectations | The plan maps every decision to its profile. Conflicting legacy node claims do not become profile authority. |
| Harness-only scope | The structural check restricts claim artifacts to harness scripts, Casper harness models, and workflows. |
| No node-proof obligations | The common contract and claim scopes preserve the node as the system under test. |
| Missing interfaces block scenarios | Capability qualification and observed fault receipts remain necessary. Synthetic transcripts never become node observations. |
| Post-merge adaptation | All seven profile claims identify TASK-018 owners. The actual merge and accepted handoff remain mandatory. |

The source audit retains six external file digests at the selected system-integration revision. It does not establish qualified live adapters.

The updated contract distinguishes three implemented controlled-transcript profiles from four unimplemented profiles. No claim specification or acceptance ledger changed during this completion.

### TASK-017-3

| Criterion | Evidence and disposition |
| --- | --- |
| Preserve prerequisite order | Git ancestry verifies the selected #430, #431, #432, and #433 revisions through the recorded stack merge and current branch. |
| Record initial identities | The unchanged matrix identifies node, harness, external suite, model, image, binary, and configuration-source revisions or digests. |
| Keep final workload pinning separate | TASK-017-12 retains executable workload pinning, qualification, resource approval, and dispatch. Workload digests remain null. |
| Preserve containment limits and inherited evidence | The review retains B44, conditional assumptions, missing historical references, and independent-exit limits. |
| Reconcile time budgets | Shared PR jobs permit 900 seconds. Shared configurations permit 120 seconds plus 60 seconds of termination grace. |
| Review external interfaces before coordination | The six-file audit describes deployment, query, fault, telemetry, and adopted-process limitations. No external checkout or pin changed. |
| Keep #216 optional | The optional candidate remains unselected. Its open state cannot satisfy the post-merge gate. |

The standalone Rust model runner has a separate five-second termination grace. Neither per-configuration limit guarantees whole-suite completion within the shared job budget.

The [application report](../casper/cbc-evidence/runs/casper-prerequisite-application-20260917-01/report.json) retains 407 records. The [stack report](../casper/cbc-evidence/runs/casper-stack-integration-20260917-01/report.json) retains 132 embedded records.

Their retained digests pass fresh integrity checks. Their fixture and model outcomes remain historical executions, not reruns against today's source.

The matrix's image identity records match its AMD64 and ARM64 entries. This check does not download, execute, or newly attest those images.

## Current upstream observations

The closure package retains read-only GitHub responses with full revisions and merge metadata.

| Reference | Observed state | Observed revision |
| --- | --- | --- |
| `dev` | Baseline authority, not repinned | `728447d0d07578d45c56c000e71bb73240a31b94` |
| #216 | Open, unselected | `619beb4a4a7ad3f8967d4586daf0f5c552bd150e` |
| #430 | Merged | `e1927301a85829ba1354433038bce1906db287e2` |
| #431 | Merged | `dce275b966b133f9da620e3086ea8f861d7d4642` |
| #432 | Open, targets `dev` | `3f63c8218a99521e36dd262abbf53280ed50679b` |
| #433 | Open, targets `formal/soak-disk-models` | `a25509e466217e8019b687082be4a279f052da13` |
| #436 | Open, targets `docs/consensus-neutral-execution` | `8870a74d3a0416233210738cd85b88f697277080` |

These observations do not replace the selected prerequisite revisions. This review does not claim integration or verification of the newer upstream heads.

TASK-017-12 must review candidate drift before dispatch. TASK-017-14 must confirm the actual stack parent and cumulative diff before final review reduction.

## Completion mechanism

The shared CLI still rejects `TASK-*` identifiers. Its reviewed `mark_task_complete` function supports the existing task records.

The closure invocation removes exactly one final CLI dispatch from a temporary helper copy. It preserves the helper's validation and strict completion functions.

The helper SHA-256 is `924a1cde6ae61d15a72dda276687b0d63d9df0dd75d7b7f1aba4e8300f502d2e`. A different helper must receive compatibility review before use.

The invocation permits only TASK-017-1, TASK-017-2, and TASK-017-3. It checks their document tests before strict completion on a tracker copy.

Only the helper-generated completion fields enter the real tracker. The existing tracker backup, shared framework, and TASK-017-4 adapter remain unchanged.

The task test references identify structural document checks, not runtime unit tests or product observations. Negative document fixtures test the checker's refusal behavior.

`BF_TODO_PARSER_STRICT=0` permits seven unrelated legacy `review` states. It does not bypass strict task integrity or change those states.

## Retained evidence and remaining gates

The [closure package](../casper/cbc-evidence/runs/casper-preparation-completion-20260918-01/report.json) records checks, source identities, helper outcomes, and the initial checker-format failure.

The initial checker expected file records in both historical packages. The stack package instead embeds record contents. The corrected checker verifies both representations.

The first completion invocation stopped at an optional parser field under `set -e`. A traced retry confirmed that cause.

The corrected invocation captures parser results through command substitution, as the shared helper does. Explicit JSON and strict integrity checks still reject incomplete results.

An external merge advanced HEAD to `6adeb7d38cee6d6e0c580e6b3ea1af3720d2b16a` during this review. It changed three node merge files, not the reviewed harness artifacts.

This completion did not perform that merge or verify those node changes. The existing index entries and unrelated task records were preserved.

CLAIM-CASPER-SOAK-001 retains its accepted source-specific discharge. Claims 002 through 008 and all soaks remain pending. No waiver or workflow tag was added.

TASK-017-4 remains complete. TASK-017-5 through TASK-017-14 and EPIC-018 retain their existing statuses and gates.

D-07 baseline semantics, publication conflict-order review, adapter qualification, containment limits, and missing historical references remain explicit. These preparation closures do not resolve them.
