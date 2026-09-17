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
| PR #432 `scripts/ci/check-tla-invariants.sh --soak-pr` | Fixed two-minute per-configuration cap, one TLC worker, 13 positives, 61 negative controls. |
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

This increment adds no executable implementation or claim discharge. No TLC model, prerequisite fixture suite, or soak campaign was executed.
