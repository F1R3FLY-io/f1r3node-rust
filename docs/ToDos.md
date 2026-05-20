---
doc_type: todos
version: "1.0"
last_updated: 2026-05-20
mr_status:
  ready: false
  target_branch: master
---

# Tasks and Epochs

<!--
TEMPLATE USAGE INSTRUCTIONS:
0. Update the frontmatter date when modifying this file
   (Update version only for significant structural changes to template)
1. Replace all [PROJECT_NAME] and [PROJECT_SPECIFIC] markers
2. Add new epochs using the YAML frontmatter format below
3. Move completed epochs to docs/CompletedTasks.md
4. Use /nextTask to find the next task to work on
5. Use /implement to execute tasks with full context
6. Remove these usage instruction comments before committing
-->

This document tracks implementation work through **epochs** (logical groupings of related tasks).

**Document Structure**
- Active work: This file (`docs/ToDos.md`)
- User stories: `docs/UserStories.md`
- Completed work: `docs/CompletedTasks.md`
- Backlog: `docs/Backlog.md`

**For LLM assistance in multi-repo workspace:**
See [Task Tracking Standard]([RELATIVE_PATH]/top-level-gitlab-profile/docs/common/task-tracking-standard.md)

**For reference (GitLab):**
[Task Tracking Standard](https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/common/task-tracking-standard.md)

---

## MR/PR Tracking

When all tasks in this file are complete and ready for merge, update the frontmatter:

```yaml
mr_status:
  ready: true
  target_branch: main
  title: "feat: [PROJECT_SPECIFIC: MR title]"
  description: |
    ## Summary
    - [Completed items]

    ## Test plan
    - [x] All tests passing
  labels: ["feature", "enhancement"]
```

---

## Active Epochs

<!-- Epochs are ordered by priority. Work on the highest priority epoch first. -->

---

### EPOCH-001: cargo-geiger Supply-Chain Unsafe Audit

```yaml
---
epoch_id: EPOCH-001
title: "cargo-geiger supply-chain unsafe audit"
status: pending
priority: p1
user_story: US-001
blocked_by: []
created_at: 2026-05-20
claimed_by: null
claimed_at: null
user_flow: FLOW-001
tasks:
  - id: TASK-001-1
    title: "Local tooling: Justfile recipes for cargo-geiger"
    status: pending
    acceptance:
      - "`just geiger` runs a workspace scan (default features) and prints a table summary"
      - "`just geiger-baseline` writes/refreshes `.cargo-geiger.baseline.json` at repo root"
      - "`just geiger-update` regenerates the baseline and shows the diff against the previous one"
      - "All three recipes appear in `just --list` with single-line docstrings"
      - "README/DEVELOPER.md links the recipes from the supply-chain section"

  - id: TASK-001-2
    title: "Commit initial `.cargo-geiger.baseline.json` and document update workflow"
    status: pending
    blocked_by: [TASK-001-1]
    acceptance:
      - "`.cargo-geiger.baseline.json` is tracked in git at repo root"
      - "Baseline is reproducible from a clean checkout via `just geiger-baseline` (deterministic ordering)"
      - "DEVELOPER.md describes when and how to bump the baseline (with an example PR snippet)"
      - "Baseline file format documented (raw `cargo geiger --output-format Json` or a normalized subset)"

  - id: TASK-001-3
    title: "Add `geiger` CI job to `.github/workflows/ci.yml` with baseline regression gate"
    status: pending
    blocked_by: [TASK-001-2]
    acceptance:
      - "New `geiger` job runs in parallel with `deny` on every PR (push to dev/master and PR to dev/master/feature/**)"
      - "Job installs cargo-geiger via `cargo install --locked cargo-geiger` with `Swatinem/rust-cache@v2` keyed for it"
      - "Job uses the pinned `nightly-2026-02-09` toolchain to match the rest of CI"
      - "Job runs workspace scan with default features, emits JSON, and uploads it as a build artifact (`geiger-report`)"
      - "Job fails when current scan introduces new unsafe vs. committed baseline; passes when scan matches or improves"
      - "Failure message points the developer at `just geiger-update` to refresh the baseline intentionally"

  - id: TASK-001-4
    title: "Add nightly scheduled cargo-geiger workflow"
    status: pending
    blocked_by: [TASK-001-3]
    acceptance:
      - "Scheduled workflow runs daily (cron) on `master` with the same scan as the PR job"
      - "Run uploads the JSON artifact with a date-suffixed name and surfaces diffs vs. committed baseline"
      - "Workflow does not modify the baseline; surfaces drift only (so transitive ecosystem changes show up without blocking PRs)"
      - "Schedule entry is colocated in `ci.yml` or in a sibling `geiger-nightly.yml` (documented choice in the PR description)"

  - id: TASK-001-5
    title: "Opt-in cargo-geiger integration in `.githooks/pre-push`"
    status: pending
    blocked_by: [TASK-001-1]
    acceptance:
      - "`RUN_GEIGER=1 git push` triggers a geiger scan alongside the existing clippy/test background jobs"
      - "Default `git push` does NOT run geiger (cost stays on CI by default)"
      - "Hook reuses the same baseline regression check as CI and reports PASS/FAIL in the same summary format"
      - "Hook degrades gracefully with a clear SKIP message if `cargo-geiger` is not installed locally"
      - "`scripts/setup-hooks.sh --status` and `--help` mention the `RUN_GEIGER` switch"

  - id: TASK-001-6
    title: "Document the cargo-geiger workflow in DEVELOPER.md"
    status: pending
    blocked_by: [TASK-001-3, TASK-001-5]
    acceptance:
      - "DEVELOPER.md gains a `Supply-Chain Audit (cargo-geiger)` section"
      - "Section explains: what cargo-geiger measures, where the baseline lives, how to refresh it, and when to refuse a baseline bump"
      - "Section cross-references the existing cargo-deny section so the two supply-chain tools are findable together"
      - "README.md's supply-chain bullet (if any) links to the new section"
---
```

**Context:** F1R3node is a blockchain node where consensus correctness and key handling are safety-critical. We already enforce license/advisory hygiene via `cargo-deny`, but we have no signal on the **volume or growth of `unsafe` code** in our dependency tree. `cargo-geiger` quantifies that surface and, when paired with a committed baseline, gives us a regression gate that catches a new transitive dependency (or a dep version bump) silently adding `unsafe` blocks. The branch `cargo-gieger-integration` (sic) was created to land this.

**Scope:**
- Workspace-wide scan with default features (matches `cargo test` posture)
- Baseline + regression gate enforced in CI on every PR
- Nightly scheduled run to surface ecosystem drift without blocking PRs
- Opt-in pre-push integration (`RUN_GEIGER=1`) for developers who want pre-flight checks locally
- Justfile recipes and DEVELOPER.md documentation
- **Explicitly out of scope:** hard "forbid unsafe" gates on workspace crates; per-crate matrix scans; replacing or modifying `cargo-deny`; rewriting any workspace crate to remove unsafe.

**Notes:**
- cargo-geiger may need a `--locked` install and may itself depend on a recent stable; verify it builds under our pinned `nightly-2026-02-09`. If the nightly toolchain breaks geiger's build, install it under stable in CI and run it against the workspace lockfile (geiger reads `Cargo.lock`, not the toolchain).
- Workspace deps known to carry unsafe: `bytes`, `tokio`, `heed`/`lmdb`, `rustls`, `prost`, `parking_lot`. Initial baseline will reflect this; reviewers should compare deltas, not absolutes.
- Branch name typo (`cargo-gieger-integration`) is noted but not in scope to rename here; up to the PR author whether to rename before merge.
- Future enhancement (Backlog candidate): per-crate `#![forbid(unsafe_code)]` on the crates that don't legitimately need unsafe (e.g., `graphz`, `shared`).

---

### EPOCH-002: Restore `ulimit -n 65536` in pre-push hook and CI (b851666a regression)

```yaml
---
epoch_id: EPOCH-002
title: "Restore ulimit raise dropped by new-repo infrastructure import"
status: complete
priority: p1
user_story: US-TBD
blocked_by: []
created_at: 2026-05-20
claimed_by: claude-session-cargo-geiger
claimed_at: 2026-05-20
completed_at: 2026-05-20
tasks:
  - id: TASK-002-1
    title: "Restore `ulimit -n 65536 2>/dev/null || true` in `.githooks/pre-push`"
    status: complete
    acceptance:
      - "Line is inserted at the same position as commit `a5401484` placed it (between the GIT_DIR unset and the `cd \"$REPO_ROOT\"` step), with the same trailing `|| true` so a shell that disallows raising the limit does not fail the hook"
      - "Comment above the line cites the heed-0.22 fd pressure rationale"
      - "`cargo test --release -p rholang` invoked via `git push` no longer panics with `Os { code: 24, kind: Uncategorized, message: \"Too many open files\" }`"

  - id: TASK-002-2
    title: "Restore `ulimit -n 65536` in `.github/workflows/ci.yml` test job"
    status: complete
    acceptance:
      - "Line is inserted as the first command in the `run:` block of the test step, ahead of `mkdir -p /tmp/test-logs` and the cargo invocation"
      - "GitHub Actions Linux runners (soft default ~1024) lift past the LMDB-heavy parallel rholang threshold"

  - id: TASK-002-3
    title: "Verify the restored fix end-to-end"
    status: complete
    blocked_by: [TASK-002-1, TASK-002-2]
    acceptance:
      - "Locally: `git push` runs the pre-push hook with the new ulimit and the rholang test step completes green without `SKIP_TESTS=1`"
      - "CI: the next PR push triggers the workflow and the test matrix runs without EMFILE-class failures"
---
```

**Context:** A previous fix (commit `a5401484`, Apr 30 2026: "fix(ci): raise ulimit -n to 65536 for LMDB-heavy parallel tests") added one line to `.githooks/pre-push` and one to `.github/workflows/ci.yml` to lift the OS file-descriptor ceiling for the rholang test suite (heed 0.22 holds more fds per Env than 0.11). That fix was **silently overwritten** by commit `b851666a` (May 12 2026: "chore: import new-repo native infrastructure on top of legacy"), which wholesale replaced both files with the new-repo versions (`393 insertions(+)` for `.githooks/`). The ulimit lines went with them. Symptom returned: `Os { code: 24, kind: Uncategorized, message: "Too many open files" }` in `rholang/src/rust/interpreter/test_utils/resources.rs:56` during `cargo test --release -p rholang` under the pre-push hook on macOS.

**Scope:**
- Re-add the exact same two lines that `a5401484` originally inserted, at the same code locations in the new (post-`b851666a`) versions of those files.
- **Explicitly out of scope:** porting `env_cache.rs` (the Weak<Env> cache from `origin/sync/legacy-v0.4.15` commit `4ecd73a6`). That is a more aggressive fix that the cherry-pick `4b486b4f` deliberately deferred for the eventual `dev→master` merge ("the broader upstream sync... intentionally NOT included"). Pulling it into this branch entangles two unrelated streams; let it land via the planned merge.

**Notes:**
- Reference: `git show a5401484 -- .githooks/pre-push .github/workflows/ci.yml` for the exact original diff.
- Insertion points selected to match the prior commit's structure: after the GIT_DIR unset in the hook, before the `mkdir -p` in the workflow.
- The two lines complement each other: the hook's `ulimit -n 65536 2>/dev/null || true` covers local pre-push test runs; the workflow's `ulimit -n 65536` covers CI Linux runners.
- **Follow-up Backlog candidate:** if the broader supply-chain effort wants every developer's `cargo test -p rholang` invocation (outside the hook) to also pass on macOS default `ulimit -n 256`, the `env_cache.rs` port from `4ecd73a6` is the architectural fix. File when the dev→master merge timing is right; treat as a separate epoch.

---

<!-- Add more epochs following the same format -->

---

## Epoch Template

Use this template when adding new epochs:

```yaml
---
epoch_id: EPOCH-XXX
title: "Short descriptive title"
status: pending
priority: p2
user_story: US-XXX
blocked_by: []
created_at: YYYY-MM-DD
claimed_by: null         # Implementer ID: human-{email}, {tool}-session[-{id}], or {team}/{role}
claimed_at: null
tasks:
  - id: TASK-XXX-1
    title: "Task description"
    status: pending
    acceptance:
      - "Measurable acceptance criterion"
---
```

---

## Task States

| Status | Meaning | Next Action |
|--------|---------|-------------|
| `pending` | Not started | Available to claim |
| `in_progress` | Being worked on | Continue or handoff |
| `blocked` | Waiting on dependency | Check `blocked_by` |
| `review` | Ready for review | Review and approve |
| `complete` | Done | Move to CompletedTasks.md |

---

## Workflow

1. **Find next task**: Use `/nextTask` to identify the highest priority unclaimed task
2. **Claim task**: Set `claimed_by` using [Implementer Identification](../common/stigmergic-collaboration.md#implementer-identification) format and `status: in_progress`
3. **Implement**: Use `/implement` to execute with full context
4. **Complete**: Mark `status: complete` when acceptance criteria met
5. **Move epoch**: When all tasks complete, move epoch to `docs/CompletedTasks.md`

---

## References

- **User Stories:** `docs/UserStories.md`
- **Completed Work:** `docs/CompletedTasks.md`
- **Backlog:** `docs/Backlog.md`
- **MR/PR Tracking Standard:** [docs/common/todos-mr_pr-tracking-standard.md]([RELATIVE_PATH]/top-level-gitlab-profile/docs/common/todos-mr_pr-tracking-standard.md)
