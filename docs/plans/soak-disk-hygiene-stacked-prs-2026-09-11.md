# Soak Disk Hygiene: Stacked Pull Request Plan

**Status:** plan, not started. The cut begins when the source agent confirms its last cycle.

**Staging branch:** `fix/parsimonious-maintainble-soak-disk-hygiene`, draft PR #406. It closes unmerged after the cut.

**Lifetime of this plan:** PR 1 carries this file onto dev so that reviewers of the stack can read it. PR 4 deletes it on the final merge. Git history keeps it.

**Base:** `dev` at the time of the cut. On 2026-09-11 that is `6f48d638c`.

**Work log:** [task-soak-disk-hygiene-parsimonious-2026-09-09.md](../work-logs/task-soak-disk-hygiene-parsimonious-2026-09-09.md), which records the decisions this plan applies.

---

## 1. Shape of the stack

Four pull requests, each based on the one before it, merged bottom-up:

| Order | Branch | Base | Concern | Size against its base |
| --- | --- | --- | --- | --- |
| PR 1 | `formal/deploy-storage-bound` | `dev` | The deploy storage bound, a machine A area | 6 files, about 130 lines |
| PR 2 | `fix/soak-driver-disk-protection` | PR 1 | The soak driver fix, its host suite, the Docker harness, the real-daemon checks, and the driver evidence record | 48 files, about 4,900 changed lines |
| PR 3 | `formal/soak-disk-models` | PR 2 | One control registry with a bounded PR tier, the gate fixture test, the two consolidated soak models, and the storage budget | 105 files, about 3,400 lines |
| PR 4 | `docs/consensus-neutral-execution` | PR 3 | The architecture note and the verification split by machine and medium | 3 files, about 650 lines |

The staging branch holds 105 commits and 2,163 changed files against dev. The four PRs together hold about 160 files. Everything else is legacy and is not carried. Section 5 lists it.

### Why this order

- **PR 1 first** because it is small, isolated, and has no reference to the soak work. It stays isolated so it can move with the execution split later.
- **PR 2 before PR 3** because the driver fix is the substance. The models refute the pre-fix driver, so a reviewer reads the driver first. The driver evidence record names the controls PR 3 registers. Those are text references and do not break PR 2 on its own.
- **PR 3 carries the gate registry** because dev has no registry, no tiers, and no fixture test today. The registry was designed for the soak controls, and the soak README, the claim, and the gate record describe it. PR 3 also registers the deploy storage control from PR 1 and the two carrier index controls that dev keeps as manual controls.
- **PR 4 last** because the note links the formal-verification guide rows that PR 1 and PR 3 add.

### Merge order

Merge PR 1 into dev first. GitHub retargets PR 2 to dev when the PR 1 branch is deleted. Rebase PR 2 on dev, let CI run, and merge. Repeat for PR 3 and PR 4. Never merge a higher PR while a lower one is open.

---

## 2. Contents of each pull request

Paths are relative to the repository root. "Whole file" means the file is copied from the staging branch as is. "Split" means only some hunks go into that PR, and the rest go into a later one. Section 3 lists every split file.

### PR 1: deploy storage bound

Whole files:

- `formal/tlaplus/deploy_storage/DeployStorageBound.tla`
- `formal/tlaplus/deploy_storage/MC_DeployStorageBound.cfg` and `.tla`
- `formal/tlaplus/deploy_storage/MC_DeployStorageBound_unmetered_pre_fix.cfg` and `.tla`
- `formal/tlaplus/deploy_storage/README.md`
- `docs/plans/soak-disk-hygiene-stacked-prs-2026-09-11.md`, this plan, for the reviewers of the stack

Split files:

- `scripts/ci/check-tla-invariants.sh`: add `deploy_storage/MC_DeployStorageBound` to dev's `POST_FIX_CONFIGS` only. The negative control stays a manual control, documented in the area README, until PR 3 registers it.
- `docs/formal-verification.md`: the "Deploy storage" row only.

Verification: TLC on both configurations, 237 states for the positive and an exit 12 with `RetainedWithinPhlo` for the control. Dev's full gate on schedule runs the positive.

### PR 2: soak driver disk protection

Whole files:

- `scripts/run-merge-recovery-soak.sh`
- `scripts/bench/soak-disk-test.Dockerfile`
- `scripts/bench/test-run-merge-recovery-soak.sh`, the host suite
- `scripts/bench/test-soak-disk-admission.sh`, the table-driven Docker harness
- `scripts/bench/test-soak-disk-emergency.sh` and the 27 `scripts/bench/test-soak-*.sh` host fixtures it runs
- `scripts/bench/test-soak-real-*.sh`, the eight real-daemon checks
- `scripts/bench/collect-soak-metrics.sh` and `scripts/bench/write-soak-summary.sh`
- `scripts/bench/run-soak-contained.sh`, `scripts/bench/soak-containment.py`, and `scripts/bench/test-soak-native-containment.py`, the B44 launcher prototype and its fixture, flagged as a prototype the normal workflow does not use
- `docs/cbc-evidence/scripts-run-merge-recovery-soak-sh.md`, the driver evidence record with the B1 to B43 rows and the digest table
- `docs/cbc-evidence/github-workflows-slashing-tests-yml.md`
- `docs/work-logs/task-soak-disk-hygiene-parsimonious-2026-09-09.md`

Split files:

- `.github/workflows/ci.yml`: the "Verify isolated disk admission and emergency scenarios" step only.
- `docs/Glossary.md`: the six disk terms, which are disk hygiene, disk admission, disk probe, disk guardian, crash monitor, and controller loss.
- `docs/ToDos.md`: the three soak bullets.
- `.github/workflows/merge-recovery-soak.yml`: four comment lines, or drop them. They add no behavior.

Verification: the host suite on Linux, since the driver needs pidfd and Python 3. Also the Docker harness in CI, the workflow invariants, and the release workflow tests. The evidence record's digest table binds manifests that PR 2 does not carry. Section 6 records that decision.

### PR 3: control registry and soak formal models

Whole files:

- `formal/tlaplus/soak_disk/SoakDiskAdmission.tla`, `SoakDiskGuardian.tla`, `SoakStorageBudget.tla`
- The 48 `MC_SoakDisk*` and `MC_SoakStorageBudget*` configuration and stub pairs
- `formal/tlaplus/soak_disk/README.md`
- `scripts/ci/test-check-tla-invariants.sh`, the gate fixture test
- `docs/claims/soak-disk-protection.md`
- `docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md`

Split files:

- `scripts/ci/check-tla-invariants.sh`: everything PR 1 did not take. That is the `NEGATIVE_CONTROLS` registry, the `REGISTERED_CONTROL_AREAS` list, the `--soak-pr` bounded tier, the fixture hooks, and the soak positives.
- `.github/workflows/slashing-tests.yml`: the TLA+ job runs on every pull request with the 15-minute bounded tier and keeps the 240-minute nightly budget.
- `.github/workflows/ci.yml`: the "Verify TLA+ gate classification and routing" step only.
- `formal/tlaplus/carrier_index/README.md`: the registered-controls text.
- `docs/formal-verification.md`: the "Soak disk protection" row and the gate text.

Verification: TLC on the three positives, with 25146, 22500, and 5616 states. Then the bounded gate with 6 positives and 46 controls, and the fixture test with 46 controls times seven outcomes plus the routing scenarios.

### PR 4: consensus-neutral execution note

Whole files:

- `docs/artifacts/f1r3fly-consensus-neutral-sm.md`
- `docs/cbc-verification-tiers.md`

Split files:

- `docs/README.md`: the two index rows.
- `docs/formal-verification.md`: the link paragraph to the note and the link to the tiers document.
- `docs/Glossary.md`: the three CbC terms, which are committed outcome, correct by construction, and work bound.

Deletions:

- `docs/plans/soak-disk-hygiene-stacked-prs-2026-09-11.md`, this plan. The stack is complete when PR 4 merges, so the plan leaves the tree with it.
- The work log's link to this plan becomes a permalink to the plan at its last staging-branch commit. The link then still resolves after the deletion.

Verification: the STE check on the note, and every relative link resolves.

---

## 3. Split files

These files receive hunks from more than one PR. Each is copied from the staging branch in the last PR that touches it, and hand-edited to the earlier PR's subset before that.

| File | PR 1 | PR 2 | PR 3 | PR 4 |
| --- | --- | --- | --- | --- |
| `scripts/ci/check-tla-invariants.sh` | positive line | | registry, tiers, fixture hooks | |
| `.github/workflows/ci.yml` | | harness step | fixture step | |
| `docs/formal-verification.md` | deploy storage row | | soak row, gate text | note and tiers links |
| `docs/Glossary.md` | | six disk terms | | three CbC terms |
| `docs/ToDos.md` | | three bullets | | |
| `docs/plans/soak-disk-hygiene-stacked-prs-2026-09-11.md` | added | | | deleted |

---

## 4. Mechanics

Each PR is one commit built from paths, not from history. The 105 staging commits carry merge noise, source cycles, and attribution trailers that do not belong on dev.

For each PR, in order:

1. Create the branch from its base. PR 1 starts from `origin/dev`. Each later PR starts from the previous PR's branch.
2. Copy the whole files from the staging branch tip with `git restore --source=<staging tip> -- <paths>`.
3. Hand-edit the split files to the subset in section 3.
4. Run the verification for that PR.
5. Run the STE check on the prose the PR adds.
6. Commit through `/quick-commit` with no attribution trailers, and push.
7. Open the PR against its base, not against dev, with the title and body drafted in advance. Mark PR 2, PR 3, and PR 4 as drafts until the PR below each one merges.

Every git-state action, which means branch creation, staging, pushes, and PR creation, belongs to the maintainer. The assistant prepares files and verification and hands each step back.

---

## 5. What is not carried

The staging branch holds 2,070 legacy files, about 247,000 lines, that stay behind:

- The per-defect TLA+ modules, 36 standalone specs with their configurations and correspondence notes, superseded by the two consolidated models.
- The 27 generated evidence packages under `docs/cbc-evidence/soak-*` and the run inspection under `docs/soak-evidence`, bound by digest in the driver evidence record instead.
- The digest inventory `docs/claims/soak-claim-inventory.jsonc`, the retention record, the inventory tests, and the formal-gate claim.
- The root `.gitignore` rerun patterns and the package `.gitattributes` files, which only serve the packages.
- Every Ruby file. None is tracked on the staging branch now, and none is carried.

The phase-two list in the work log names each item.

---

## 6. Decisions taken before the cut

The maintainer took these decisions on 2026-09-11:

| Question | Decision |
| --- | --- |
| The source agent's plan, its tdd-plan with the B-number entries, and its work log: 3 files, about 2,400 lines | Not carried. The driver evidence record states each behavior in its own row. |
| The JSONC policy text in `CLAUDE.md`, `AGENTS.md`, and `GEMINI.md`: about 100 changed lines | Not carried in this stack. The workspace has not standardized on JSONC, and the text is not a soak concern. |
| The digest table in the driver evidence record binds 24 manifests that PR 2 does not carry | Kept. The record now says that the packages live in the history of the source branch and of the staging branch. |
| The deploy storage negative control stays manual between PR 1 and PR 3 | Accepted. The area README says so, and PR 3 registers it. |
| Source cycles that land after the cut starts | Merge them into the staging branch as before, then port the delta to the affected PR branch by path. |
| This plan file | PR 1 carries it, and PR 4 deletes it. |
| The B44 launcher prototype: the containment launcher, its Python module, and its fixture | Carried in PR 2, flagged as a prototype the normal workflow does not use. The guardian model's `ManagedContainment` control then describes code that is in the tree. B44 stays open. |

## 7. Risks

- **Dev drift.** Each PR rebases on the merged one below it. A conflict at that point is resolved on the PR branch, not on the staging branch.
- **The gate on PR 1.** Dev's TLA+ job runs only on schedule, so PR 1's CI does not run TLC. The local TLC run is the evidence until PR 3 lands.
- **Docker in CI for PR 2.** The harness step needs the slim fixture image with Python 3. PR 2 carries the Dockerfile that provides it.
- **The staging branch stays open** until PR 4 merges. Any source cycle in that window goes through section 6's last row.
