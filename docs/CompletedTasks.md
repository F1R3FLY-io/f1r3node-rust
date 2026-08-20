---
doc_type: completed_tasks
version: "1.0"
last_updated: 2026-08-19
---

# Completed Tasks

This document archives completed epics and tasks for historical reference and progress tracking.

**Document Structure**
- Active work: `docs/ToDos.md`
- User stories: `docs/UserStories.md`
- Completed work: This file (`docs/CompletedTasks.md`)
- Deferred work: `docs/Backlog.md`

---

## Completed Epics

<!-- Epics are listed in reverse chronological order (newest first) -->

---

### EPIC-011: TLA+ Exhaustive Tier Red→Green (3-Validator Detector Coverage)

```yaml
---
epic_id: EPIC-011
title: "TLA+ Exhaustive Tier Red→Green (3-Validator Detector Coverage)"
status: complete
completed_date: 2026-08-19
priority: p0
user_story: null
blocked_by: []
created_at: 2026-08-05
claimed_by: claude-session-917f64e8
claimed_at: 2026-08-05T00:00:00Z
tasks:
  - id: TASK-011-1
    title: "Add run_exhaustive workflow_dispatch input to slashing-tests.yml"
    status: complete
    claimed_by: claude-session-917f64e8
    completed_at: 2026-08-05T13:20:00Z
    branch: fix/tla-3v-liveness-split
    notes:
      - "Implemented in commit 7e38ab1f; YAML validated, all 9 check-workflow-invariants.sh invariants pass. Local-only for now per maintainer — branch not pushed."
    acceptance:
      - "workflow_dispatch gains a boolean input run_exhaustive (default false)"
      - "tla-model-check job sets RUN_EXHAUSTIVE_TLA=1 only when the input is true; push/pull_request/schedule behavior is unchanged (nightly keeps gating on the 8 fast configs)"
      - "Change lives on a feature branch from dev (fix/tla-3v-liveness-split) so the exhaustive tier can be dispatched against the branch before anything merges"

  - id: TASK-011-2
    title: "Dispatch the exhaustive tier and capture the red run"
    status: complete
    claimed_by: claude-session-917f64e8
    completed_at: 2026-08-05T13:58:00Z
    blocked_by: [TASK-011-1]
    notes:
      - "RESCOPED to local-only per maintainer (2026-08-05): red baseline captured via RUN_EXHAUSTIVE_TLA=1 TLC_PER_CONFIG_TIMEOUT=6m locally instead of CI dispatch. Result: 8 fast configs OK; MC_EquivocationDetector, MC_EquivocationDetectorEager_3v, MC_EquivocationDetector_safety each distinctly labeled TIMEOUT at the cap; run failed (red for the right reason). Full-45m evidence: 11 nightlies 2026-07-25..08-04 + 2026-08-04 local repro. CI-dispatch red baseline deferred to push time. Full log in TDD plan cycle_log (B1)."
      - "DISCOVERED: script roll-up says 'FAILED: N config(s) violated invariants' for cap timeouts — mislabels timeout as violation; candidate fix tracked as TDD plan B7 pending ratification."
    acceptance:
      - "gh workflow run slashing-tests.yml --ref <branch> -f run_exhaustive=true executed (env -u GITHUB_TOKEN)"
      - "Run goes red with all three exhaustive-tier configs (MC_EquivocationDetector, MC_EquivocationDetectorEager_3v, MC_EquivocationDetector_safety) reported as TIMEOUT at the 45m per-config cap — distinctly labeled as timeouts, not invariant violations"
      - "Run URL and per-config outcomes recorded in the epic work log as the red baseline"

  - id: TASK-011-3
    title: "Split MC_EquivocationDetectorEager_3v into safety + bounded liveness configs"
    status: complete
    claimed_by: claude-session-917f64e8
    completed_at: 2026-08-05T14:50:00Z
    blocked_by: [TASK-011-2]
    notes:
      - "PREMISE CORRECTED 2026-08-05 (maintainer-ratified; supersedes the title and first acceptance line): MC_EquivocationDetectorEager_3v is already safety-only — the Eager rewrite checks liveness as the Inv_LivenessAsSafety invariant and the .cfg has no PROPERTY line, so its 45m timeouts are state-space cost (3v×3s×2b), not liveness-graph blowup. There is no split to make. Fix = bound-tightening: new MC_EquivocationDetectorEager_3v2s (3 validators × 2 seqnums × 2 blocks, full _3v invariant list, symmetry) for the nightly tier; full 3v×3s×2b stays exhaustive. The liveness/safety split remains valid for MC_EquivocationDetector (its .cfg has PROPERTY Live_DetectionComplete) — that is TASK-011-5's scope. TDD plan B2 (merged with B3) tracks execution."
      - "GREEN 2026-08-05T14:48Z: MC_EquivocationDetectorEager_3v2s completes in 2m08s (57.2M states generated, 5.72M distinct, depth 37) with ZERO violations — first-ever completed detector check at 3 validators; the stop-on-violation contingency did not fire. Fast-tier regression suite clean (8/8 OK). Model files created; tier-list wiring is TASK-011-4."
    acceptance:
      - "formal/tlaplus/slashing/ gains MC_EquivocationDetectorEager_3v_safety.{tla,cfg} (INVARIANTS only) and MC_EquivocationDetectorEager_3v_liveness.{tla,cfg} (PROPERTIES, constants bounded to complete under the cap), mirroring the existing MC_EquivocationDetector_liveness pattern (~3s where the combined config times out)"
      - "Both new configs still model 3 validators — bounding must not reduce validator count, or the coverage-gap fix is illusory"
      - "Both complete locally well under TLC_PER_CONFIG_TIMEOUT=45m via scripts/ci/check-tla-invariants.sh"
      - "CONTINGENCY: if the liveness config, completing for the first time at 3 validators, reports a genuine counterexample, this task stops and the trace is reported — green then comes from an algorithm/model fix investigated under a new task, not from tuning the model until it passes"

  - id: TASK-011-4
    title: "Restore _3v coverage to the nightly tier, sync docs, capture the green run"
    status: complete
    claimed_by: claude-session-917f64e8
    completed_at: 2026-08-05T15:12:00Z
    blocked_by: [TASK-011-3]
    notes:
      - "MC_EquivocationDetectorEager_3v2s added to POST_FIX_CONFIGS (one line); 14-test-plan §14.6/§14.9 synced to the corrected diagnosis. Local green run: 9/9 OK, _3v2s 128s, ~4.2 min total. Per the local-only rescope, the CI-dispatch green run is deferred to push time (TASK-011-5 / plan B6)."
    acceptance:
      - "scripts/ci/check-tla-invariants.sh adds the two new _3v configs to the default (nightly-gating) tier; combined MC_EquivocationDetectorEager_3v stays in the exhaustive tier as the unbounded reference"
      - "docs/theory/slashing/design/14-test-plan.md §14.6/§14.9 updated to match the new tier membership"
      - "Default-tier dispatch (or PR run) goes green with the _3v configs included; run URL recorded next to the red baseline"
      - "Edits to check-tla-invariants.sh stay minimal to keep the pending PR #198 reconciliation conflict (namespaced entries + fail-on-missing) tractable"

  - id: TASK-011-5
    title: "Apply the same split to MC_EquivocationDetector"
    status: complete
    claimed_by: claude-session-917f64e8
    completed_at: 2026-08-05T19:40:00Z
    blocked_by: [TASK-011-4]
    notes:
      - "Split treatment landed 2026-08-05: MC_EquivocationDetector_liveness_2v (2v×1s×2b) verifies Live_DetectionComplete at 2 validators in 8s, wired into the nightly tier (10/10 green, ~4.3 min). Safety half (MC_EquivocationDetector_safety) already existed."
      - "Exhaustive-dispatch acceptance resolved via the DOCUMENTED arm (CI run 31027278093 at 36ea59b8): 10 fast configs OK in CI, 3 unbounded references TIMEOUT at 45m with the corrected roll-up ('3 cap timeout(s), 0 violation-or-error(s)'); accepted-unbounded status documented in 14-test-plan §14.6 and the run_exhaustive input description. Green-on-schedule exhaustive coverage is the ratified follow-up (profile → measured caps → schedule), outside EPIC-011."
    acceptance:
      - "MC_EquivocationDetector gets the same safety/liveness split treatment once the _3v recipe is proven (its safety half, MC_EquivocationDetector_safety, already exists — the liveness half is the new work)"
      - "Exhaustive tier dispatch goes fully green, or remaining timeouts are explicitly accepted and documented as unbounded-reference runs"
---
```

**Context:** The "TLA+ invariant check" nightly job was red on every run from its start (2026-07-25) until hotfix PR #201 — not from invariant violations, but because `MC_EquivocationDetector` and `MC_EquivocationDetectorEager_3v` hit the 45-minute per-config cap (interleaved liveness checking goes superlinear; locally reproduced over a 106M-state graph). PR #201 parked them behind `RUN_EXHAUSTIVE_TLA=1`, but no CI path sets that variable, so the exhaustive tier currently never runs anywhere. Meanwhile the nightly tier checks the inherently multi-validator equivocation property at ≤2 validators, because `_3v` is the only 3-validator detector model (flagged by spreston8 in the PR #201 review).

**Plan (confirmed with maintainer 2026-08-05):** make the exhaustive tier dispatchable, run it **red** (honest timeout-red — TLC has never found a violation in these models; every config that completes, passes), then make it **green** via the causal liveness/safety split — not by raising the cap (see the causal-diagnosis-before-resources rule). The one open risk is deliberate: these liveness properties have never completed at 3 validators, so the split may surface a real counterexample, in which case the green path becomes an algorithm/model fix (TASK-011-3 contingency).

**Scope:**

- Included: dispatch input, red baseline run, `_3v` safety/liveness split, nightly-tier restoration, docs sync, follow-on `MC_EquivocationDetector` split
- Excluded: raising `TLC_PER_CONFIG_TIMEOUT`; reducing validator count to make models cheap; the cargo-mutants nightly deadline overrun (separate, second independent nightly red — still untracked)
- Coordination: touches `scripts/ci/check-tla-invariants.sh`, which the pending PR #198 reconciliation also modifies — keep tier-list edits minimal

---

---

### EPIC-001: System-Integration Alignment

```yaml
---
epic_id: EPIC-001
title: "System-Integration Alignment"
status: complete
completed_date: 2026-08-19
priority: p1
user_story: US-001
blocked_by: []
created_at: 2026-03-19
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-001-1
    title: "Align genesis wallets.txt with system-integration (20 wallets, validator3=500T)"
    status: complete
    acceptance:
      - "docker/genesis/wallets.txt matches system-integration/genesis/wallets.txt (20 lines)"
      - "Validator3 balance is 500000000000000000 (500T)"
      - "All 12 additional test wallets present"

  - id: TASK-001-2
    title: "Standardize compose env var naming (F1R3FLY_RUST_IMAGE -> F1R3FLY_IMAGE)"
    status: complete
    acceptance:
      - "All compose files use F1R3FLY_IMAGE instead of F1R3FLY_RUST_IMAGE"
      - "DEVELOPER.md and docker/README.md updated"

  - id: TASK-001-3
    title: "Standardize Docker network name to f1r3fly-shard"
    status: complete
    acceptance:
      - "shard.yml network named f1r3fly-shard"
      - "observer.yml and validator4.yml reference f1r3fly-shard as external network"

  - id: TASK-001-4
    title: "Verify shard starts with updated genesis and network config"
    status: complete
    claimed_by: claude-session-epic009
    completed_at: 2026-04-13T20:55:00Z
    blocked_by: []
    acceptance:
      - "docker compose -f docker/shard.yml up succeeds"
      - "Genesis ceremony completes with 20-wallet wallets.txt"
      - "Observer and validator4 can join via f1r3fly-shard network"
    notes:
      - "All 3 written ACs verified end-to-end with locally built f1r3fly-rust:local image"
      - "Bonding extension also verified: added validator4's REV address (1111La6tHaCt...jtEi3M) to wallets.txt as genesis funding, then deployed bond.rho signed by validator4, propose included in block with errored=false and cost=167749 phlo, bond-status flipped to 'Validator is bonded', validator4 proceeded to produce 6+ blocks via heartbeat"
      - "Root cause of earlier insufficient-funds error: validator4.yml was designed for runtime bonding but validator4's REV address was never added to genesis wallets.txt. Fix is a single-line addition."
      - "REV-address computation done via `node eval` on 1.know_ones_vaultaddress.rho (output in docker stdout of the evaluating node)"
---
```

**Context:** The `system-integration` repo orchestrates this node via Docker Compose and shardctl. It has a 6-phase migration plan (see `system-integration/docs/migration-to-rust-node.md`) to make f1r3node-rust the sole node implementation. Phase 1 requires genesis and compose alignment in this repo.

**Scope:**

- Genesis wallets.txt sync (critical blocker for system-integration Phase 1)
- Compose env var and network name standardization
- Validation that shard starts correctly

**Notes:**

- system-integration currently targets branch `dev` in its services.yml, but this repo uses `master` as its working branch. system-integration will need to update its branch reference.
- standalone.yml keeps its own network name (`f1r3fly-standalone`) since it's isolated by design.

---

---

### EPIC-002: Separate Monitoring from Shard Compose

```yaml
---
epic_id: EPIC-002
title: "Separate Monitoring from Shard Compose"
status: complete
completed_date: 2026-08-19
priority: p2
user_story: US-001
blocked_by: []
created_at: 2026-03-19
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-002-1
    title: "Extract Prometheus and Grafana into docker/monitoring.yml"
    status: complete
    claimed_by: claude-session-epic009
    completed_at: 2026-04-13T21:35:00Z
    acceptance:
      - "docker/monitoring.yml contains prometheus and grafana services"
      - "monitoring.yml joins f1r3fly-shard as external network"
      - "shard.yml no longer contains prometheus/grafana services"
      - "docker/README.md updated to reflect new file"
    notes:
      - "Verbatim service-block move; same container names, ports, volumes, env"
      - "Also updated Justfile shard-down to include monitoring.yml teardown"
      - "Also updated docker/vps-cloud-testing.md Part A to reflect opt-in monitoring"
---
```

**Context:** system-integration manages monitoring as a separate compose file (`compose/monitoring.yml`). Aligning this repo's structure makes compose files directly usable as upstream sources during the migration (Phase 3).

**Scope:**

- Move prometheus and grafana service definitions from `docker/shard.yml` to `docker/monitoring.yml`
- Update documentation

---

---

### EPIC-005: Issue Migration

```yaml
---
epic_id: EPIC-005
title: "Issue Migration"
status: complete
completed_date: 2026-08-19
priority: p1
user_story: US-002
blocked_by: [EPIC-004]
created_at: 2026-04-09
claimed_by: claude-session-migrate
claimed_at: 2026-04-17T19:35:00Z
completed_at: 2026-04-17T19:35:00Z
tasks:
  - id: TASK-005-1
    title: "Migrate 22 Rust-relevant issues to f1r3node-rust"
    status: complete
    claimed_by: claude-session-migrate
    completed_at: 2026-04-17T19:35:00Z
    acceptance:
      - "22 Rust-relevant issues created on f1r3node-rust as #5-#26 with original context"
      - "Each new issue has migration header with source #, author, filed date, and link"
      - "Original labels (bug/enhancement/question) preserved where applicable"
      - "Original issues on f1r3node received redirect comments pointing to new issue numbers"
    notes:
      - "Spec called for 22 total (16 Rust-specific + 6 triage/design); actual open count was 22"
      - "#437 excluded from migration — already fixed on rust/staging by commit 89ac4a7a, closed with reference"
      - "Mapping table: /tmp/issue-migration/issue-map.tsv"

  - id: TASK-005-2
    title: "Close 5 Scala-only issues on f1r3node"
    status: complete
    claimed_by: claude-session-migrate
    completed_at: 2026-04-17T19:35:00Z
    acceptance:
      - "Issues #452, #366, #321, #221 closed with deprecation comment (reason: not planned)"
      - "Comment directs reporter to f1r3node-rust if bug still reproduces there"
      - "phase_3_issues.status set to 'complete' in /tmp/migrationPlan.md"
    notes:
      - "#184 from the original spec was already closed pre-migration (unrelated genesis refactor), so effective count is 4 Scala + 1 already-fixed (#437) = 5 closures"
---
```

**Context:** Transfer the 27 open issues from f1r3node to their appropriate destinations. 22 issues migrate to f1r3node-rust, 5 Scala-only issues are closed.

**Scope:**

- Included: Issue creation, cross-referencing, closing Scala issues
- Excluded: Fixing any of the migrated issues

---

---

### EPIC-R03: CI/CD Pipeline

```yaml
---
epic_id: EPIC-R03
title: "CI/CD Pipeline"
status: complete
priority: p1
completed_at: 2026-03-19
completed_by: human + claude
tasks:
  - id: TASK-R03-1
    title: "Add GitHub Actions workflow with lint and per-crate test matrix"
    status: complete
  - id: TASK-R03-2
    title: "Fix pre-push hooks to run tests per-crate (LMDB lock contention)"
    status: complete
---
```

**Summary:** Added GitHub Actions CI with cargo fmt, clippy linting, and per-crate test matrix. Fixed pre-push hooks to avoid LMDB lock contention by running crate tests sequentially.

**Key Changes:**
- `.github/workflows/` — lint + per-crate test jobs
- `hooks/pre-push` — sequential per-crate test execution

---

### EPIC-R02: Developer Tooling and Hooks

```yaml
---
epic_id: EPIC-R02
title: "Developer Tooling and Hooks"
status: complete
priority: p1
completed_at: 2026-03-19
completed_by: human + claude
tasks:
  - id: TASK-R02-1
    title: "Add pre-commit and pre-push git hooks (lint/test gates)"
    status: complete
  - id: TASK-R02-2
    title: "Fix hook executable permissions in git index"
    status: complete
  - id: TASK-R02-3
    title: "Add LMDB system dependency for hook test runs"
    status: complete
  - id: TASK-R02-4
    title: "Fix wallet test data corrupted by rustfmt format_strings"
    status: complete
  - id: TASK-R02-5
    title: "Fix doc comment fencing broken by wrap_comments"
    status: complete
  - id: TASK-R02-6
    title: "Expand local node and Docker setup instructions"
    status: complete
---
```

**Summary:** Established developer guardrails with pre-commit (fmt + clippy) and pre-push (test) hooks. Fixed several issues caused by aggressive rustfmt settings and missing system dependencies.

**Key Changes:**
- `hooks/pre-commit`, `hooks/pre-push` — git hook scripts
- `casper/` — restored test data and doc comments damaged by rustfmt
- `DEVELOPER.md` — expanded setup instructions for macOS, Ubuntu, Fedora

---

### EPIC-R01: Repository Extraction

```yaml
---
epic_id: EPIC-R01
title: "Repository Extraction"
status: complete
priority: p0
completed_at: 2026-03-19
completed_by: human
tasks:
  - id: TASK-R01-1
    title: "Extract pure Rust workspace from f1r3node rust/dev branch"
    status: complete
---
```

**Summary:** Extracted all 11 Rust crates from the `F1R3FLY-io/f1r3fly` repository's `rust/dev` branch into a standalone Cargo workspace. Removed Nix flake, SBT build, Scala source, `.envrc`, and JVM tooling. Added native dependency install instructions.

**Key Changes:**
- Standalone Cargo workspace with 11 crates
- Removed: Nix, SBT, Scala, JVM dependencies
- Added: Homebrew/apt install instructions, Justfile, Docker configs

---

## Completion Statistics

| Period | Epics Completed | Tasks Completed | Notes |
|--------|------------------|-----------------|-------|
| 2026-03 | 3 | 9 | Repo bootstrap: extraction, tooling, CI |

---

## References

- **Active Work:** `docs/ToDos.md`
- **User Stories:** `docs/UserStories.md`
- **Backlog:** `docs/Backlog.md`
