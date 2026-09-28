---
task: TASK-017-3
branch: formal/soak-casper-consensus
claimed_by: pi-casper-harness
started_at: 2026-09-17T01:43:26Z
handoff_status: blocked
execution_scope: prerequisite-review-only
next_steps:
  - Obtain separate authorization for prerequisite integration.
  - Confirm the budget wording and preserve both evidence inventories.
  - Select immutable image and workload configuration digests before dispatch.
---

# TASK-017-3 Prerequisite Review

**Historical record:** TASK-017-3 is now complete for its selected prerequisites and initial identities. The [completion review](task-017-1-3-completion.md) identifies remaining dispatch and upstream integration work.

## Scope and branch state

The user authorized continuation after TASK-017-2's contract work. This increment reviews prerequisites and prepares a non-dispatchable candidate matrix.

The [interface contract](../casper/design/soak-interface-contract.md) is specified. Its tracker closure remains blocked by the shared helper's unsupported `TASK-*` identifier.

The checkout advanced externally to `9f6acd1a24fed6c772ee0a2f08fe873c489799e3` during the previous task. It includes the model commit and most contract changes.

This assistant did not create those commits or push them. Remaining documentation changes are preserved.

The live GitHub `dev` ref is `a2fe60c7255bf4ba035d41fb65b6d6f1c0f02632`. The local `origin/dev` agrees.

PR metadata reports `4d8d9d79cb5bf613d7ac9d8b3cbaef3a5541c324` as PR #430's base commit. That field is not the current `dev` tip.

## Prerequisite snapshot

The following values were observed during this review through read-only GitHub queries.

| PR | Head revision | Target branch | GitHub state | Returned required checks |
| --- | --- | --- | --- | --- |
| [430](https://github.com/F1R3FLY-io/f1r3node-rust/pull/430) | `dedb3add172098efcbc63d72b2ebdc612f2fddcd` | `dev` | Open, mergeable, merge status BLOCKED | 12 successful |
| [431](https://github.com/F1R3FLY-io/f1r3node-rust/pull/431) | `0e176e486a028d10704b96add6e5eb50525682cb` | `formal/deploy-storage-bound` | Open, mergeable, merge status BLOCKED | 12 successful |
| [432](https://github.com/F1R3FLY-io/f1r3node-rust/pull/432) | `e0380392bcc66d9774e403edb8a08415798c1e0e` | `fix/soak-driver-disk-protection` | Open, mergeable, merge status BLOCKED | 12 successful |
| [433](https://github.com/F1R3FLY-io/f1r3node-rust/pull/433) | `65f7f6daa832c0acb6fddf2b462db1b9d5461729` | `formal/soak-disk-models` | Open, mergeable, merge status BLOCKED | 12 successful |

Required-check results come from `gh pr checks --required`, not an interpretation of duplicate historical rollup entries. They do not establish complete merge eligibility.

The active `dev` rules require fourteen distinct check contexts, an up-to-date branch, and code-owner review where applicable.

The twelve-item CLI response omits `Integration Tests (amd64)` and `Integration Tests (arm64)`. The broader rollup contains duplicate entries with different conclusions.

The branch-rule endpoint returned no active rules for the three intermediate target branches. Recheck all rules and context identities after each retarget.

The exact reason for GitHub's BLOCKED merge status was not established. A maintainer must resolve it before merging.

Each lower head is an ancestor of the next head. `git merge-base --is-ancestor` returned zero for all three stack edges.

None of the four heads is an ancestor of this branch. Each membership check returned one. Local object availability does not mean integration occurred.

PR #390 remains open at `ce266ddcd8369107441dd9fc9b8e9ff6719146f1`. Its meeting review remains the ratification reference, not blanket authority for historical proposals.

PR #216 remains open at `619beb4a4a7ad3f8967d4586daf0f5c552bd150e` with merge status DIRTY and conflicting changes.

PR #216 is optional for the pre-merge matrix. It cannot satisfy EPIC-018's actual-merge gate.

## Budget reconciliation

The source distinguishes a whole-job cap from a per-configuration cap.

| Source at the reviewed revision | Observed behavior |
| --- | --- |
| PR #432 `.github/workflows/slashing-tests.yml`, `tla-model-check` | Pull-request/push job cap: 15 minutes. Scheduled/manual job cap: 240 minutes. |
| PR #432 `scripts/ci/check-tla-invariants.sh --soak-pr` | Fixed two-minute per-configuration cap, two TLC workers, 13 positives, 61 negative controls. |
| Same script's timeout command | Sends TERM at the cap and permits another 60 seconds before KILL. |
| Same script without the bounded option | Defaults to 45 minutes per configuration, subject to the enclosing job cap. |
| PR #433 `docs/cbc-verification-tiers.md` | Describes the pull-request refutation tier as “in two minutes” without distinguishing those budgets. |
| This branch's local Casper runner | 120-second per-configuration cap across eleven controls. It is not registered in shared CI. |

The implemented two-minute value is not a two-minute whole-suite guarantee. Seventy-four sequential controls can exceed the fifteen-minute job budget in worst-case timeout behavior.

Recommended wording: the bounded tier has a fifteen-minute job cap and a two-minute per-configuration cap, followed by the declared termination grace period.

This recommendation needs approval before shared documentation or workflow changes. No model, bound, or required control may be dropped to make the wording true.

New Casper controls require measured CI-shaped runtime and upload headroom before registration. A fast local run does not establish that combined budget.

The shared gate checks negative exit 12 and the exact invariant line. The local Casper runner additionally requires a trace and a completed-search marker for positives.

Integration must preserve those stronger checks for Casper controls. Timeout, cancellation, missing output, and unexpected success remain non-passing.

## Containment and inherited evidence

PR #431 explicitly leaves B44 open: simultaneous driver and crash-monitor death can leave writers active under the normal workflow.

Its containment launcher is a prototype that the normal workflow does not use. A passing model or launcher fixture cannot close production containment coverage.

The inherited driver ledger also retains conditional rate, storage-cap, durability, and termination assumptions. This review does not replace them with unconditional guarantees.

Its historical manifests are not all carried in the stack's final tree. Retrieve the referenced packages and verify their digests before relying on those results.

Three committed paths overlap between this branch and the stack tip: the soak workflow and the driver/TLC-gate evidence records.

Current glossary edits and intermediate task-tracking edits also need preservation during integration. Overlap detection is not a merge-conflict test.

The imported `CLAIM-SOAK-001` evidence and this branch's `CLAIM-CASPER-SOAK-*` evidence cover different obligations.

Retain separate claim-specific evidence links. Do not replace either record with the other or inherit its discharge status.

The single-status-per-artifact ledger limitation still applies. Preserve the current pending Casper claim bundle, canonical evidence locations, and compatibility symlinks.

## Candidate matrix preparation

This matrix pins known source identities only. It is not approved for dispatch, and null values are blockers rather than defaults.

```yaml
matrix_status: not-dispatchable
reviewed_harness_revision: 9f6acd1a24fed6c772ee0a2f08fe873c489799e3
external_harness_revision: b3d14b27e3c6276b1eb4ab9ccef04e02b0c4e283
model_sha256: e08ce354f1e09b9f0873e6f730e509c4be6b394acc0bce7ff5ef53805d91b0c4
clean_model_configuration_sha256: cc4438b578ccc5c898897e7b66b7472253d2532093f8a5338bf8d924002e197b
candidates:
  - id: dev-baseline
    phase: pre_pr216_merge
    role: baseline
    node_revision: a2fe60c7255bf4ba035d41fb65b6d6f1c0f02632
    image_digest: null
    node_binary_digest: null
    workload_configuration_digest: null
    activation: current-dev-policy-only
    admission: blocked
  - id: pr216-experiment
    phase: pre_pr216_merge
    role: optional-experiment
    node_revision: 619beb4a4a7ad3f8967d4586daf0f5c552bd150e
    image_digest: null
    node_binary_digest: null
    workload_configuration_digest: null
    activation: separate-approval-required
    admission: blocked
post_merge:
  phase: post_pr216_merge
  actual_merge_revision: null
  selected_dev_revision: null
  accepted_handoff: false
  admission: blocked
```

Both candidates lack qualified profile adapters, approved resources, image/binary identity, and selected workload configuration. The matrix must be refreshed after integration.

The existing stress-load configuration cannot stand in for a production-policy baseline. No campaign or node build was launched during this review.

## Safe integration sequence

1. Obtain separate authorization for the exact integration operations.
2. Confirm the interface contract and the budget wording with the maintainer.
3. Resolve GitHub merge blockers without bypassing required checks or review.
4. Integrate upstream PRs in order: #430, #431, #432, then #433.
5. Recheck each successor's revised head, base, required checks, and evidence after its predecessor merges.
6. Obtain authorization to update this branch from the reviewed integrated baseline.
7. Preserve claim inventories and resolve evidence-record collisions without losing either claim bundle.
8. Rerun source, fixture, model, workflow, and digest checks on the actual integrated tree.
9. Select immutable image/binary and workload configuration digests before approving any campaign.

The PR bodies prohibit merging a higher PR while its lower prerequisite remains open. A local combined tree is not proof of upstream integration.

No merge, cherry-pick, rebase, repin, commit, push, workflow dispatch, or external repository change occurred in this increment.

## Remaining gates

TASK-017-3 remains in progress. Integration authorization, upstream merge blockers, budget-text approval, evidence preservation, and complete campaign identities remain open.

TASK-017-4 shared integration remains blocked. The existing bounded model result and TASK-017-2 contract do not satisfy these remaining gates.

## Verification

Task YAML, work-log metadata, local links, and the blocked candidate-matrix structure passed validation.

Source snapshot comparisons, model digests, stack ancestry, unrelated task preservation, empty index, and unchanged HEAD checks passed.

All twelve existing runner unit tests passed. These regression checks do not verify prerequisite implementations, profile bindings, or node behavior.

The deterministic STE Check and `git diff --check` passed. Human STE Review remains necessary.

The initial increment added no executable implementation or claim discharge. It executed no TLC model, prerequisite fixture suite, or soak campaign.

## Fixture review continuation

The continuation inspected source at `65f7f6daa832c0acb6fddf2b462db1b9d5461729` without merging or changing the checkout's scripts.

The starting checkout was clean at `8b9f5d61614a529b2047d6c04d68f17405b89e17`. Staging and commits remain with the other agent.

GitHub's BLOCKED status does not establish merge conflicts. No local merge was attempted, and this review found no local merge conflicts.

### Source and evidence corrections

The bounded shared gate sets `TLC_WORKERS=2`. The budget table above corrects the earlier one-worker transcription error.

The inherited gate ledger claims sixty-one controls times seven fixture outcomes. The current fixture tests six rejection outcomes on one control per registered area.

There are three registered areas, so that rejection loop contains eighteen cases. A separate full-gate fixture checks acceptance for all sixty-one registered controls.

These are source-derived coverage counts, not newly executed results. The inherited ledger's blanket case-count description cannot establish current coverage.

The shared gate accepts a positive exit zero without a completed-search marker. Its negative branch requires exit twelve and the expected invariant line, but not a trace.

Keep the stricter Casper runner checks when shared registration becomes authorized. This review does not weaken the contract to match the inherited gate.

### Executed summary fixture

The unchanged `scripts/bench/test-write-soak-summary.sh` passed in an isolated source snapshot. It exercised five data cases and a jq keyword scan.

The cases cover base aggregation, per-core aggregation, empty input, mixed iteration outcomes, and sparse metrics. They do not verify Casper scenario verdicts.

The observed jq version was 1.8.2. The keyword scan is not execution under jq 1.6.

A separate malformed-metric probe invoked the unchanged summary writer with two declared iterations, one valid metric file, and one malformed metric file.

The writer exited zero, retained one iteration entry, and reported two iterations with zero failures. It emitted neither `scenario_verdict` nor `soak_verdict`.

This reproduces an evidence-completeness gap at DR-SUMMARY. It does not establish a product pass or a node failure.

TASK-017-4 must validate artifact completeness separately from dashboard aggregation. Malformed required input must remain explicit and prevent a passing conformance result.

The [retained report](../casper/cbc-evidence/runs/casper-prerequisite-fixtures-20260917-01/report.json) records source digests, tool versions, invocation, duration, and fixture outcome.

The [probe record](../casper/cbc-evidence/runs/casper-prerequisite-fixtures-20260917-01/malformed-metric-probe/result.json) retains input/output digests and the observed gap.

The malformed input uses a `.txt` archive suffix. Its mapping records the original runtime filename for reproduction.

To reproduce these observations without repository changes:

1. Create a new temporary source directory.
2. Extract the report's source paths from its exact `source_revision` with `git show`.
3. Preserve executable permissions on the extracted shell scripts.
4. Run `bash scripts/bench/test-write-soak-summary.sh` inside the source directory.
5. Copy the probe's two input files into a separate temporary output directory.
6. Restore the malformed input's runtime filename from `input_archive_mapping`.
7. Set the probe record's environment with temporary paths replacing its placeholders.
8. Run the extracted `scripts/bench/write-soak-summary.sh` with that environment.
9. Compare its exit status and summary fields with `actual_observation`.

Do not replay into the retained evidence directory. The writer replaces its summary files.

### Deferred execution and ownership

The gate fixture writes fixed `/tmp/tlc-*.log` paths. Ninety-two matching paths already existed, so this review did not execute that fixture or overwrite those files.

An isolated log destination remains necessary before concurrent gate execution. A copied source tree alone does not isolate these absolute output paths.

The driver fixture received source review only. Its three scenarios test resource-stop handling, deadline handling, and disk-band refusal through controlled tools.

Those scenarios do not supply the ten Casper lifecycle bindings. In particular, graceful deadline exit zero must not become a passing conformance verdict.

No driver, shared gate, workflow, pin, task status, or claim ledger changed. No node, Docker workload, or TLC model ran.

The new evidence supports prerequisite review only. All full Casper harness/profile claims remain pending.

## Historical evidence audit

The next increment started from clean commit `7d362bb9b`. It inspected inherited references without changing source, pins, task status, or Git state.

The ledger snapshot is `65f7f6daa832c0acb6fddf2b462db1b9d5461729`. The examined historical tree is `2388a8eedf33d07018f0630bced51a6e054ba439`.

Thirty-six of thirty-eight historical manifest/observation references were present in that tree. All thirty-six matched their exact ledger SHA-256 values.

Two references were absent from the examined tree:

- `g0-hosted-d6aaba962-2026-09-08/manifest.json`
- `repin-962effd-2026-09-08/manifest.json`

This result does not establish absence from other revisions or external raw stores. Retrieval of those two records remains open.

### Retrieved hosted artifacts

| Artifact | Run | Archive digest check | Retrieved content |
| --- | --- | --- | --- |
| 10056856375 | 34228660038 | Matches GitHub metadata and the historical hosted-observation record | Two clean transcripts and two invariant-violation transcripts |
| 10063312912 | 34244230926 | Matches GitHub metadata and the inherited ledger | Two clean transcripts and two invariant-violation transcripts |

Both artifacts were available and unexpired. Their reported expiration date is December 7, 2026.

Each archive contains the carrier-index clean configuration, two carrier-index negative controls, and the replay hot-loop clean configuration.

The clean transcripts report completed searches with 222 and nine distinct states. The negative transcripts contain `IndexCompleteForWindow` and `AbsenceProofSound` violations with traces.

The archives do not independently record each TLC process exit code. A successful workflow result does not supply that missing exit-code evidence.

The first hosted record distinguishes PR head `43af06dab41ddf81b8dd1fcc5cf9e1303027aab7` from synthetic checkout `bad72c4c299b73ede9abb1b5adc0f70fbdfb8e7b`.

Its workflow-control identity remains null. The second run reports head `d6aaba9628536245fc42183d1682539583d6550b`.

Neither historical head establishes verification of the current prerequisite stack or Casper harness.

### Nested references and retention

Forty-five nested references across the three gate manifests and hosted-observation record matched their digests. These checks cover thirty-seven unique resolved paths, not forty-five independent files.

The audit resolved renamed `.jsonc` files explicitly. It recovered hosted TLC logs from artifact 10056856375 where the historical tree did not contain them.

Eight retained transcripts replace the CI workspace prefix with `[WORKSPACE]`. The report keeps both original and retained digests, archive digests, seeds, workers, and state counts.

The [audit report](../casper/cbc-evidence/runs/casper-prerequisite-evidence-audit-20260917-01/report.json) records every reference, resolution, digest comparison, and limitation.

The raw driver stores and all nested driver outputs remain unaudited. Manifest byte integrity does not establish complete package coverage or semantic validity.

No model or node was rerun. No claim was discharged. TASK-017-3 remains in progress.

### Reproduction

1. Read the two ledgers from the report's `ledger_revision` with `git show`.
2. Compare their manifest references against bytes at `historical_tree_revision`.
3. Retrieve artifact metadata with `gh api repos/F1R3FLY-io/f1r3node-rust/actions/artifacts/<id>`.
4. Download each archive through that endpoint's `/zip` suffix into a new temporary directory.
5. Compare archive and member digests with the audit report.
6. Apply the recorded workspace-prefix substitution before comparing retained transcript digests.

Keep downloaded archives outside existing evidence directories. Do not replace historical records with regenerated results.
