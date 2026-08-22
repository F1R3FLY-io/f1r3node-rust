---
doc_type: todos
version: "1.1"
last_updated: 2026-08-19
mr_status:
  ready: false
  target_branch: master
---

# Tasks and Epics

This document tracks implementation work through **epics** (logical groupings of related tasks).

**Document Structure**

- Active work: This file (`docs/ToDos.md`)
- User stories: `docs/UserStories.md`
- Completed work: `docs/CompletedTasks.md`
- Backlog: `docs/Backlog.md`

**Shared Coordination File:** `/tmp/migrationPlan.md` (read by agents in both f1r3node and f1r3node-rust)

---

## Active Coordination

<!-- Compact, current-state-only. This section replaces the free-form status
     entries that previously accumulated at the top of this file; the full
     narrative history is preserved verbatim in
     docs/work-logs/coordination-archive-2026-08-01T03Z.md.
     NOTE: docs/discoveries/*.md is gitignored here (.gitignore:123) — use
     docs/work-logs/ for durable cross-agent notes, and keep this section to
     current operative facts only. -->

- **System-integration PR #118 reviewed head repin prepared (2026-08-15).** PR #118 corrects four harness defects from f1r3node-rust CI run `31886226287`. FT convergence now requires `FT >= FTT` and monotonicity. Epoch-boundary bond blocks accept either valid closeBlock transition map. Phase 5 still requires exact activation. Retired log snapshots retain their owning test allowances without cross-test leakage. Bond assertions now use the finalization resolver's canonical deploy block instead of an orphaned first inclusion. Focused unit tests protect the log-scan bookkeeping. The local bonding suite passed in 208.90 seconds with 5,636 MB peak RSS. This branch pins all three `SYSTEM_INTEGRATION_REF` sites to immutable reviewed PR head `735a7b95a3af74677f9519a6f01049cbc004bca4`. It retains dedicated shards, canonical finalized-block selection, standard `--rss-ceiling-mb 10000` limits, and the weekend preflight's `45056` MB limit.
- **PR #182** (`hotfix/renormalize-system-integration-pin-post-79` → `dev`, head `121029f1`) normalizes all three `SYSTEM_INTEGRATION_REF` sites to system-integration `main` `369d49df2f97e65b3d0ad869aa668a7383b11179` (the post-#79/#80 promotion). Multi-agent review posted 2026-08-01: approved 3-0 (anthropic abstained on an API billing error). This completes and supersedes the 2026-07-31T19:33 PDT handoff; the similarly named local branch `hotfix/normalize-system-integration-pin-post-79` is stale and has no PR.
- **Soak memory envelope re-based; weekend soak awaiting 48GB VM (2026-08-10, claude-session-ecaee825).** The 2026-08-09 weekend-soak breaches (runs 31331480002/31332864501) were NOT a merge regression — local A/B exonerated master@eb4030c2 and the harness pin diff was empty; the 6-node shard's real envelope is 16.7–19.3GB. PR #217 (merged) raised SOAK_RSS_CEILING_MB→20480 / lowered floor→8192; run 31390673884 then showed 32GB cannot hold the envelope (orchestrator guardian, free 6524MB). FINAL (maintainer, both sessions): system-integration sets fleet default AMD64_MEM_GB=48 (branch hotfix/raise-soak-vm-memory there, awaiting push/PR/merge); this repo's hotfix/raise-soak-vm-memory then takes one commit: SYSTEM_INTEGRATION_REF bump ×3 sites + ceiling 20480→28672 + these doc updates, then re-dispatch weekend-60h. Budget follow-up RESOLVED (SI PR #99 verification): no dollar-denominated cap exists in the runner stack — all lifetime guards are time-based (reaper MAX_AGE_HOURS + soak-deadline-epoch exemption, cloud-init idle/wedge timeouts); the ~$12/~$33 figures in the TASK note below are cost ESTIMATES only, ≈$13 daily/≈$35 weekend at 48GB. Evidence: docs/work-logs/soak-rss-regression-2026-08-09.md (local).
- **RESOLVED (2026-08-09): the hold below was overtaken — dev→master promoted via PR #213 and the weekend-soak pin question is moot under the re-based envelope above.** ~~Hold `dev` → `master` until the weekend soak snapshot is verified.~~ The Friday 19:30 Pacific scheduled `Merge Recovery Soak` run must exist with its `headSha` recorded, confirming it launched from the pre-normalization `master` (PR #181 pin `79262d8b`), before promoting. Merging first would silently move the weekend soak to the post-#79 `369d49df` pin. If no scheduled run appears, hold the promotion and investigate or manually dispatch from the intended pre-normalization `master`. Known discrepancy: scheduled runs initialize `target_ref=dev` although comments say the Friday weekend run targets `master` — treat the captured workflow `headSha`/pin and the resolved target SHA as separate evidence.
- **Run 30661821085 (2026-07-31 dispatch) is CLOSED.** Root cause: a 52-minute OCI host stall froze the VM's userspace ("runner lost, VM healthy" class) — not a node or test failure; the node was healthy at block 141 when output stopped. Evidence was extracted and hash-verified, and the evidence VMs were released with durable OCI backups remaining. Full analysis: `../system-integration/docs/ToDos.md` and the archive work log.
- **Cross-agent INBOX** entries from claude-session-02f66bb7 are archived; their actionable items live in TASK-010-6, TASK-010-7, and TASK-010-8. Use tracked files (this file or `docs/work-logs/`) for inter-agent messages — never `docs/discoveries/`.

---

## MR/PR Tracking

When all tasks in this file are complete and ready for merge, update the frontmatter:

```yaml
mr_status:
  ready: true
  target_branch: master
  title: "feat: f1r3node -> f1r3node-rust migration"
  description: |
    ## Summary
    - Full migration from f1r3node monorepo to standalone Rust workspace
    - Code sync, CI/CD, Docker, issue migration, deprecation

    ## Test plan
    - [x] All 11 crates build and pass tests
    - [x] Docker image publishes under new name
    - [x] system-integration tests pass against new image
```

---

## Active Epics

<!-- Epics are ordered by priority. Work on the highest priority epic first. -->

---

### EPIC-013: Release Process and Deployment Trains

```yaml
---
epic_id: EPIC-013
title: "Release Process and Deployment Trains"
status: in_progress
priority: p1
user_story: null
blocked_by: []
created_at: 2026-08-19
claimed_by: claude-session-838e6241
claimed_at: 2026-08-19T00:00:00Z
tasks:
  - id: TASK-013-1
    title: "Specify and ratify the release process (docs/release-process.md)"
    status: complete
    completed_at: 2026-08-19T00:00:00Z
    branch: feature/release-process-implementation
    notes:
      - "All 13 Section 19 items ratified 2026-08-19. Two amendments: the regression verdict is advisory with maintainer review plus an OCI Notifications alert, and one infra-failure restart is permitted when 60h coverage is preserved."
      - "Includes the soak terminology rename (60h stability soak, dev integration soak, Shard soak-in), the Section 17.1 trigger and duration table, and glossary entries."
  - id: TASK-013-2
    title: "Phase 1: evidence-only release automation"
    status: complete
    completed_at: 2026-08-19T00:00:00Z
    branch: feature/release-process-implementation
    notes:
      - "release-evidence.yml generates exact-run evidence; release.yml and soak-in.yml are held-state stubs; release-evidence.sh plus unit tests and the test-release-workflows.sh contract guard are in place."
  - id: TASK-013-3
    title: "Phase 2: canary publication (canary-publish.yml)"
    status: in_progress
    branch: feature/release-process-implementation
    notes:
      - "Implemented as canary-publish.yml: workflow_run on CI completion publishes the immutable canary tag, prerelease, and Docker Hub images by digest from the run's own artifacts; ineligible runs skip cleanly. Evidence upgrades to publication_mode: canary via release-evidence.sh record-images. OCIR canary deferred to Phase 3 (registry location is secret material). Remains in_progress until the first live master run proves it."
    acceptance:
      - "canary-publish.yml publishes immutable canary tags, prereleases, and images from tested artifacts on release-eligible master runs"
  - id: TASK-013-4
    title: "Phase 3: artifact-based validation (candidate digest modes)"
    status: in_progress
    branch: feature/release-phase3-candidate-digest-validation
    notes:
      - "2026-08-22: stacked on Phase 4 (feature/release-phase4-promotion-controller). Registry decision ratified by the maintainer: OCIR is canonical for candidate gates, Docker Hub stays as the dual-published public mirror, no sync service."
      - "canary-publish.yml pushes the same index to OCIR; evidence records images.ocir_index_digest, which the validator requires to equal the Docker Hub index digest. The OCIR repository path never enters evidence."
      - "oci-validation.yml gains candidate_tag (exact-candidate mode); reusable-oci-validation.yml pulls each architecture image from OCIR by digest instead of building. merge-recovery-soak.yml gains candidate_tag, pulls the amd64 image by digest, and carries the tag through an in-window restart."
      - "Both gate workflows publish Section 8.1 documents with release-gate-evidence.sh from a publish_candidate_evidence job under release-credentials, plus the release-candidate marker that resumes release.yml. test-release-gate-evidence.sh proves the writer against release-gates.sh."
      - "promote-release.sh and release.yml copy the stable tag and latest into OCIR as well as Docker Hub."
      - "The regress alert reuses the soak's existing ONS verdict email; promotion holds until maintainer-review.json is uploaded."
      - "2026-08-22 multi-agent review of PR #325: gate documents carry candidate_evidence_sha256 and are written from the evidence file the run kept as a same-run artifact, never a re-downloaded release asset; release-gates.sh requires that digest to equal the evidence under evaluation. A candidate soak restart must name restart_of_run_id and match the original run's soak-window artifact (candidate, attempt 0, weekend, end epoch); coverage_preserved is true only for a verified restart."
    acceptance:
      - "merge-recovery-soak.yml and oci-validation.yml consume the candidate image digest without rebuilding"
      - "The canary publisher publishes the same index digest to OCIR and Docker Hub, and evidence records the OCIR digest"
      - "One exact-candidate OCI validation run and one candidate weekend soak publish Section 8.1 documents that release-gates.sh evaluates as pass"
      - "Optional hardening: a read-only OCIR pull token replaces OCIR_AUTH_TOKEN in the soak and OCI validation pull steps (the OCIR_* secrets are repository-scoped and already readable there, verified 2026-08-22)"
  - id: TASK-013-5
    title: "Phase 4: stable promotion controller"
    status: in_progress
    branch: feature/release-phase4-promotion-controller
    notes:
      - "2026-08-22: release-gates.sh evaluates the eight Section 8 gates from JSON documents only (pass, hold, or fail; exit 0, 10, or 20) and promote-release.sh plans Section 11 steps 4 to 14 from observed stable state, verifies binaries, emits stable-release-evidence.json, and bumps the next version. release.yml replaces the held stub: a read-only gates job, then a promote job under release-credentials that copies the image by digest with imagetools create, creates the verified stable tag and release, moves latest, and opens the next-version pull request. test-release-gates.sh, test-promote-release.sh, and the updated test-release-workflows.sh contract guard run in CI."
      - "Section 8.1 defines the gate-evidence contract that Phase 3 must publish as candidate prerelease assets. Until Phase 3 lands, the OCI, soak, and verdict gates hold, so no candidate is promotable end to end."
      - "The regress-verdict OCI Notifications alert belongs to the soak workflow (Phase 3); the controller holds on regress until maintainer-review.json accepts it."
      - "2026-08-22 multi-agent review of PR #323: fail-closed gate evaluation (a malformed document fails its gate, the report always holds all eight gates), API-verified run identity for the OCI and soak documents, API-verified reviewer permission for maintainer-review.json, resume verifies existing release assets and refuses when a newer stable exists, latest is verified after the move, the next-version step is idempotent and keeps the token out of the remote URL, and the workflow_run resume is bound to a default-branch dispatch of a gate workflow whose marker names the evidence source. Concurrency was already serialized by the concurrency group; documented in Section 11.1."
    acceptance:
      - "release.yml performs exact-candidate promotion via release-gates.sh and promote-release.sh"
      - "A regress verdict publishes an OCI Notifications alert to the soak-report list and holds promotion for documented maintainer review"
      - "The release-credentials environment exists with DOCKERHUB_USERNAME and DOCKERHUB_TOKEN and required reviewers"
      - "One live promotion of a Phase 3 candidate publishes a stable tag, release, and image whose digest equals the candidate index"
  - id: TASK-013-6
    title: "Phase 5: Deployment Trains"
    status: pending
    acceptance:
      - "deployment-train.yml validates manifests under .github/deployment-trains/ and starts trains"
      - "One non-publishing rehearsal completes, then the cost-accounting train (PR #216) publishes first"
  - id: TASK-013-7
    title: "Phase 6: Shard soak-in scheduling"
    status: in_progress
    blocked_by: [EPIC-014]
    notes:
      - "The release trigger is implemented: soak-in.yml fires on stable release publication (prereleases gate out) while enrollment stays held until the EPIC-014 test net exists."
    acceptance:
      - "soak-in.yml gains a release trigger: one enrollment per stable release tag"
      - "The deferred parameters (soak-in period length, Anchor criteria, test net composition) are set and ratified"
---
```

**Context:** `docs/release-process.md` is the ratified specification. This PR (feature/release-process-implementation, PR #279) delivers TASK-013-1 and TASK-013-2; later phases follow the Section 18 migration plan on follow-on branches.

---

### EPIC-014: Test Net (Continuously Running Shards)

```yaml
---
epic_id: EPIC-014
title: "Test Net (Continuously Running Shards)"
status: pending
priority: p2
user_story: null
blocked_by: [EPIC-013]
created_at: 2026-08-19
tasks:
  - id: TASK-014-1
    title: "Design the test net (topology, lifecycle, upgrade path)"
    status: pending
  - id: TASK-014-2
    title: "Stand up the test net on OCI from existing fleet tooling"
    status: pending
  - id: TASK-014-3
    title: "Wire Shard soak-in enrollment and Anchor promotion into the test net"
    status: pending
  - id: TASK-014-4
    title: "Open selected test net shards to partners and customers"
    status: pending
---
```

**Context:** Unlike the soaks, which create a fresh shard per iteration, this epic delivers the test net: shards that run continuously. **Mechanism sketch (brief by intent — a follow-on branch/PR carries the design):** long-lived OCI instances run stable releases as test net members, reusing the existing fleet tooling (runner launch, monitoring, ONS alerts, soak dashboard) as the foundation. Each weekly stable release enrolls new nodes through the Shard soak-in (`docs/release-process.md` Section 12); nodes that complete the soak period gain the Anchor role. The test net is primarily internal release-validation infrastructure and also serves select partners and customers. This epic delivers the test net that release-process Phase 6 requires; the deferred Shard soak-in parameters are set here.

---

### EPIC-012: Open-Issue Remediation PR Queue

```yaml
---
epic_id: EPIC-012
title: "Open-Issue Remediation PR Queue"
status: pending
priority: p0
user_story: null
blocked_by: []
created_at: 2026-08-12
claimed_by: null
claimed_at: null
execution_contract:
  base_branch: dev
  task_unit: "one task = one branch = one pull request"
  merge_policy: "Start each unblocked branch from current origin/dev; after a dependency merges, start or rebase its dependent branch onto the updated origin/dev. Do not silently stack unrelated tasks."
  issue_policy: "Put Refs #N in every PR body. Because these PRs target dev rather than the default branch, close an issue only after the fix is promoted to master and its acceptance evidence is confirmed."
  completion_policy: "A task reaches review only when its acceptance checks and focused regression tests pass; it reaches complete only when the PR is merged."
tasks:
  - id: TASK-012-1
    title: "Contain AI system-contract failures without crashing the node"
    status: pending
    issues: [11]
    base_branch: dev
    branch: fix/ai-contract-failure-isolation
    proposed_pr_title: "fix(rholang): isolate AI system-contract failures"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "HTTP 5xx, timeout, and malformed responses from every external AI system contract become typed deploy failures rather than process panics"
      - "The node remains responsive and can process a subsequent valid deploy"
      - "Focused tests cover the failure path and successful recovery"

  - id: TASK-012-2
    title: "Align node readiness with Casper readiness"
    status: pending
    issues: [12]
    base_branch: dev
    branch: fix/casper-aware-readiness
    proposed_pr_title: "fix(node): report ready only after Casper initialization"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "The wait/readiness surface remains false until Casper can serve exploratory deploys"
      - "Observer startup cannot report ready during the Casper-unavailable window"
      - "Regression coverage reproduces the documented observer --wait sequence"

  - id: TASK-012-3
    title: "Reject stale-chain peers after a network-ID relaunch"
    status: pending
    issues: [13]
    base_branch: dev
    branch: fix/genesis-bound-peer-handshake
    proposed_pr_title: "fix(network): bind peer sessions to genesis identity"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "The peer handshake carries an immutable genesis/chain identity in addition to networkId"
      - "Peers from an old chain with the same networkId are rejected before dependency fetching or block processing"
      - "Compatibility and mixed-version rollout behavior are documented and tested"

  - id: TASK-012-4
    title: "Publish deb and rpm node packages from CI"
    status: pending
    issues: [14]
    base_branch: dev
    branch: feat/linux-package-artifacts
    proposed_pr_title: "feat(ci): publish deb and rpm node packages"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Release CI builds amd64 and arm64 deb/rpm artifacts with the node binary, service definition, and default configuration"
      - "Package installation, version reporting, and clean removal are smoke-tested"
      - "Artifact naming and supported-platform documentation are updated"

  - id: TASK-012-5
    title: "Replace HOCON with a maintained configuration format"
    status: pending
    issues: [15]
    base_branch: dev
    branch: feat/config-format-migration
    proposed_pr_title: "feat(config): migrate node configuration away from HOCON"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "A maintained parser and canonical configuration format replace the unmaintained HOCON dependency"
      - "Existing operator configuration has a documented compatibility or conversion path"
      - "Configuration precedence, defaults, malformed-input behavior, and representative production configs are tested"

  - id: TASK-012-6
    title: "Extend DeployData parameters to all supported Rholang values"
    status: pending
    issues: [17]
    base_branch: dev
    branch: feat/deploy-parameter-types
    proposed_pr_title: "feat(api): extend DeployData parameter value types"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "DeployData parameters support tuple, list, set, map, Nil, and URI values without lossy conversion"
      - "Protobuf/JSON compatibility and deterministic normalization are covered"
      - "Existing parameter clients remain compatible or receive a documented migration path"

  - id: TASK-012-7
    title: "Expose DeployData parameters through the observer Web API"
    status: pending
    issues: [16]
    base_branch: dev
    branch: feat/web-api-deploy-parameters
    proposed_pr_title: "feat(web-api): expose DeployData parameters"
    claimed_by: null
    blocked_by: [TASK-012-6]
    acceptance:
      - "Observer HTTP responses expose the complete parameter representation introduced by TASK-012-6"
      - "OpenAPI documentation and serialization tests cover every supported parameter type"
      - "Responses for deploys without parameters remain backward compatible"

  - id: TASK-012-8
    title: "Exclude persistently nonparticipating validators from finality weight safely"
    status: pending
    issues: [18]
    base_branch: dev
    branch: fix/participation-based-finality-committee
    proposed_pr_title: "fix(consensus): derive finality committee from finalized participation"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "A validator that bonds and activates but never produces cannot stall finalization indefinitely"
      - "Participation is derived from finalized evidence; young-DAG and minority-finalization safety are preserved"
      - "Tests cover never-started, temporarily offline, resumed, and newly activated validators"

  - id: TASK-012-9
    title: "Synchronize canonical Rholang resources after the parser upgrade"
    status: pending
    issues: [19]
    base_branch: dev
    branch: chore/sync-rholang-resources
    proposed_pr_title: "chore(rholang): sync canonical resource contracts"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "The listed system and test .rho files are reconciled with their canonical implementations"
      - "Multi-binding and @=* pattern syntax are covered by parser regressions"
      - "The full affected contract test suites pass and obsolete parser workarounds are removed"

  - id: TASK-012-10
    title: "Cache fault tolerance for finalized-block API queries"
    status: pending
    issues: [22]
    base_branch: dev
    branch: perf/cache-finalized-fault-tolerance
    proposed_pr_title: "perf(block-api): cache finalized-block fault tolerance"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Finalized block metadata stores the computed fault-tolerance value at finalization time"
      - "Block API queries use the cache only for finalized blocks and retain live computation for non-finalized blocks"
      - "Correctness and deep-history performance regressions are tested"

  - id: TASK-012-11
    title: "Remove the 129-term random-split overflow"
    status: pending
    issues: [34]
    base_branch: dev
    branch: fix/rholang-random-split-overflow
    proposed_pr_title: "fix(rholang): prevent random-split identifier overflow"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Par values at the 128/129/256 boundaries reduce without panic"
      - "The split identifier uses a range-compatible type or returns a structured error"
      - "Generated regression cases cover both CLI and interpreter paths"

  - id: TASK-012-12
    title: "Keep LFS-synced observers inside the imported DAG horizon"
    status: pending
    issues: [37]
    base_branch: dev
    branch: fix/lfs-observer-parent-horizon
    proposed_pr_title: "fix(observer): bound post-LFS parent validation to imported history"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Normal block validation after LFS sync never requires DAG hashes or roots below the imported horizon"
      - "A fresh observer catches up beyond approved-state height on a long-running shard"
      - "The regression is distinct from reporter replay coverage in PR #210"

  - id: TASK-012-13
    title: "Install RhoSpecContract as a genesis resource"
    status: pending
    issues: [41]
    base_branch: dev
    branch: feat/genesis-rhospec-contract
    proposed_pr_title: "feat(genesis): install the RhoSpec contract"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "RhoSpecContract is available from ordinary nodes without test-resource paths"
      - "Genesis registration is deterministic and its URI is documented"
      - "Existing RhoSpec tests consume the genesis-installed contract"

  - id: TASK-012-14
    title: "Resolve the PR #488 deferred review checklist"
    status: pending
    issues: [44]
    base_branch: dev
    branch: refactor/rejected-deploy-storage-followups
    proposed_pr_title: "refactor(casper): resolve rejected-deploy review follow-ups"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Every still-applicable item in issue #44 is implemented or dispositioned explicitly in the PR body"
      - "Duplicated deploy/rejected-deploy storage logic is consolidated without weakening encapsulation"
      - "Recovery-cycle, persistence, and storage regressions pass"

  - id: TASK-012-15
    title: "Resolve the PR #491 mergeable-channel review checklist"
    status: pending
    issues: [46]
    base_branch: dev
    branch: refactor/mergeable-channel-followups
    proposed_pr_title: "refactor(rspace): resolve mergeable-channel review follow-ups"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Every still-applicable item in issue #46 is implemented or dispositioned explicitly in the PR body"
      - "Tag identity derivation has one source of truth and hot-path avoidable allocations are removed"
      - "Mergeable-channel semantic and performance regressions pass"

  - id: TASK-012-16
    title: "Widen token vault arithmetic to BigInt"
    status: pending
    issues: [49]
    base_branch: dev
    branch: feat/bigint-token-vaults
    proposed_pr_title: "feat(rholang): support BigInt token vault balances"
    claimed_by: null
    blocked_by: [TASK-012-9]
    acceptance:
      - "NonNegativeNumber and MakeMint support balances and transfers above 2^63-1"
      - "New registry URIs provide an explicit protocol migration boundary"
      - "10^30 round-trip, negative-value rejection, bridge compatibility, and genesis determinism are tested"

  - id: TASK-012-17
    title: "Characterize and improve intra-deploy Par execution scaling"
    status: pending
    issues: [50]
    base_branch: dev
    branch: perf/rholang-par-scaling
    proposed_pr_title: "perf(rholang): address intra-deploy Par scaling"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "A reproducible benchmark separates reducer serialization, RSpace contention, replay overhead, and scheduler limits"
      - "The PR either implements a measurable bounded speedup or documents and enforces the intended sequential contract"
      - "Play/replay determinism and cost accounting remain unchanged"

  - id: TASK-012-18
    title: "Accept and print unsuffixed floating-point literals consistently"
    status: pending
    issues: [75]
    base_branch: dev
    branch: fix/rholang-float-literals
    proposed_pr_title: "fix(rholang): normalize floating-point literal syntax"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Unsuffixed and f64-suffixed literals parse to the same f64 value"
      - "Printing has one canonical round-trippable representation"
      - "Parser, normalizer, protobuf, and CLI round-trip boundaries are tested"

  - id: TASK-012-19
    title: "Instrument finality-frontier and long-running merge cost"
    status: pending
    issues: [45, 105]
    base_branch: dev
    branch: perf/instrument-finality-frontier
    proposed_pr_title: "perf(consensus): instrument frontier and merge-cost growth"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Metrics separate finalizer/oracle time, merge-scope construction, fallback rate, frontier width, and per-block storage cost"
      - "A bounded automated scenario reproduces the growth signature from both issues"
      - "The evidence identifies which subsequent fix owns each bottleneck rather than inferring causality from wall-clock latency"

  - id: TASK-012-20
    title: "Bound finality work as the unfinalized frontier grows"
    status: pending
    issues: [105]
    base_branch: dev
    branch: fix/bounded-finality-frontier
    proposed_pr_title: "fix(consensus): bound finality work over the DAG frontier"
    claimed_by: null
    blocked_by: [TASK-012-19]
    acceptance:
      - "Empty-block finalization latency no longer grows monotonically with frontier width"
      - "Any incremental cache or bound is invalidated deterministically on DAG/finality changes"
      - "Long-running empty-block and adversarial wide-frontier regressions pass without weakening finality safety"

  - id: TASK-012-21
    title: "Prevent long-running multi-parent merge-scope degradation"
    status: pending
    issues: [45]
    base_branch: dev
    branch: fix/bounded-merge-scope-growth
    proposed_pr_title: "fix(casper): bound long-running multi-parent merge scope"
    claimed_by: null
    blocked_by: [TASK-012-19, TASK-012-20]
    acceptance:
      - "Multi-parent merge remains the normal path after thousands of heartbeat blocks"
      - "merge_scope_too_large fallback does not grow to dominate steady-state operation"
      - "A multi-hour-equivalent accelerated regression preserves state and finalization progress"

  - id: TASK-012-22
    title: "Profile the exhaustive TLA+ configurations without guessed caps"
    status: pending
    issues: [206]
    base_branch: dev
    branch: ci/profile-exhaustive-tla
    proposed_pr_title: "ci(formal): profile exhaustive TLA configurations"
    claimed_by: null
    blocked_by: [EPIC-011]
    acceptance:
      - "All three exhaustive configurations run uncapped on a sufficiently large runner"
      - "Wall-clock, peak memory, state count, and completion/violation outcomes are recorded per configuration"
      - "The PR contains no blind timeout increase and proposes measured caps only for configurations that complete"

  - id: TASK-012-23
    title: "Schedule an expected-green exhaustive formal-verification tier"
    status: pending
    issues: [206]
    base_branch: dev
    branch: ci/schedule-exhaustive-tla
    proposed_pr_title: "ci(formal): schedule measured exhaustive TLA coverage"
    claimed_by: null
    blocked_by: [TASK-012-22]
    acceptance:
      - "Completable exhaustive configurations run on a nightly or weekly schedule with evidence-derived caps"
      - "MC_EquivocationDetector receives an explicit retire/manual-reference/alternative-engine disposition"
      - "Timeout, violation, infrastructure failure, and success remain distinct outcomes"

  - id: TASK-012-24
    title: "Measure cgroup-accounted memory during high-cap load"
    status: pending
    issues: [244]
    base_branch: dev
    branch: perf/cgroup-memory-observability
    proposed_pr_title: "perf(soak): report cgroup-accounted node memory"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Load reports capture cgroup memory.current/usageBytes alongside workingSetBytes and container restart reason"
      - "Fast LMDB mmap or allocator spikes are visible before an OOM kill"
      - "A cap 60-75 sweep records the actual memory ceiling and distinguishes budget pressure from a leak"

  - id: TASK-012-25
    title: "Remove the post-jemalloc high-load OOM ceiling"
    status: pending
    issues: [244]
    base_branch: dev
    branch: fix/jemalloc-high-load-memory
    proposed_pr_title: "fix(node): bound allocator memory under concurrent load"
    claimed_by: null
    blocked_by: [TASK-012-24]
    acceptance:
      - "The fix follows measured evidence: tune jemalloc, reduce allocation-heavy hot paths, or change the supported resource envelope explicitly"
      - "The affected profile completes the agreed cap sweep without OOM restart"
      - "The #146 throughput/lock-contention improvement is retained"

  - id: TASK-012-26
    title: "Deduplicate protocol and runtime constants"
    status: pending
    issues: [245]
    base_branch: dev
    branch: refactor/shared-node-constants
    proposed_pr_title: "refactor(node): consolidate duplicated protocol constants"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Shared LFS backoff, retry, API threshold, keepalive, cache, and page-size values have one named source or an explicit documented distinction"
      - "Requester/responder agreement values cannot drift across sibling modules"
      - "Behavior remains unchanged and focused tests assert the shared values"

  - id: TASK-012-27
    title: "Move operator-tunable constants into configuration"
    status: pending
    issues: [245]
    base_branch: dev
    branch: feat/configurable-runtime-limits
    proposed_pr_title: "feat(config): expose runtime cache and retry limits"
    claimed_by: null
    blocked_by: [TASK-012-26, TASK-012-5]
    acceptance:
      - "Runtime-manager caches, block-retriever timings/caps, startup deadlines, and network timeouts are configurable with safe defaults"
      - "Configuration names, units, bounds, and disabled-value semantics are documented"
      - "Default behavior matches the pre-change constants and invalid values fail closed"

  - id: TASK-012-28
    title: "Finish safe legacy-name migration with compatibility fallbacks"
    status: pending
    issues: [246]
    base_branch: dev
    branch: chore/f1r3fly-name-cleanup
    proposed_pr_title: "chore(node): finish the F1R3FLY naming migration"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Key/config/data paths, binary naming, metrics service tag, user-facing token text, tests, and scripts use F1R3FLY names"
      - "Existing rnode.conf, key, and data paths have a warning-backed compatibility fallback for one release"
      - "rho:rchain alias behavior is decided, implemented consistently, and documented"

  - id: TASK-012-29
    title: "Introduce a compatible successor to the rnode peer URI scheme"
    status: pending
    issues: [246]
    base_branch: dev
    branch: feat/f1r3fly-peer-uri-scheme
    proposed_pr_title: "feat(network): introduce the f1r3fly peer URI scheme"
    claimed_by: null
    blocked_by: [TASK-012-28]
    acceptance:
      - "Nodes parse both legacy rnode:// and new f1r3fly:// addresses during a documented transition window"
      - "Nodes emit the new scheme only when compatibility permits"
      - "Mixed-version discovery, bootstrap configuration, CLI parsing, and eventual legacy-removal criteria are tested and documented"
---
```

**Context:** An audit on 2026-08-12 compared all 43 open issues with all 28 open PRs. Nineteen issues had substantive open-PR coverage, including partial or stacked fixes. The 24 issues represented here had no open PR that addressed their remaining scope. Mere GitHub cross-references were not treated as coverage: PR #224 explicitly excludes the underlying #105 frontier behavior, PR #210 explicitly excludes #37's normal block-processing path, and #18's own analysis says its participation-based core fix remains outstanding.

**PR/branch workflow:**

1. Select the first unblocked `pending` task by priority and dependency order.
2. Fetch `origin/dev`, create exactly the task's proposed branch from that tip, and set `status: in_progress`, `claimed_by`, and `claimed_at`.
3. Keep the branch scoped to the listed issue(s). If investigation proves multiple independently reviewable fixes are required, add new TASK-012 entries before opening extra PRs rather than expanding the branch silently.
4. Open one PR to `dev` using `proposed_pr_title`; include `Refs #N`, acceptance evidence, and explicit non-goals.
5. Set the task to `review` and record `pr`, validation evidence, and any replacement dependency.
6. After merge, set `status: complete`. Start dependent branches from the new `origin/dev`, not from the merged feature branch.
7. When the fix reaches `master`, confirm production/default-branch acceptance evidence and close the referenced issue manually if GitHub did not.

**Merge waves:**

- **Wave A — independent and bounded:** TASK-012-1 through TASK-012-6, TASK-012-8 through TASK-012-19, TASK-012-24, TASK-012-26, and TASK-012-28 may proceed independently from fresh `origin/dev` branches.
- **Wave B — evidence or API dependent:** TASK-012-7, TASK-012-20, TASK-012-22, TASK-012-25, TASK-012-27, and TASK-012-29 start only after their declared blockers merge.
- **Wave C — cumulative behavior:** TASK-012-21 and TASK-012-23 start last in their respective chains and must validate the merged behavior, not a stacked approximation.

**Scope:**

- Included: branch/PR-sized implementation work for open issues lacking an addressing PR as of the audit.
- Excluded: the 19 issues already addressed by open PRs; issue closure before promotion to `master`; unrelated cleanup discovered while implementing a task.

---

### EPIC-003: Merge Critical PRs into f1r3node

```yaml
---
epic_id: EPIC-003
title: "Merge Critical PRs into f1r3node"
status: pending
priority: p0
user_story: US-002
blocked_by: []
created_at: 2026-04-09
claimed_by: null
claimed_at: null
external: true
external_repo: F1R3FLY-io/f1r3node
coordination_note: "This epic is executed by the agent in f1r3node. Track progress via /tmp/migrationPlan.md phase_1_critical_prs status."
tasks:
  - id: TASK-003-1
    title: "Verify new_parser branch status"
    status: pending
    acceptance:
      - "new_parser branch is merged into rust/dev OR confirmed as base for Reified RSpaces chain"
      - "rholang-rs#83 dependency is resolved"

  - id: TASK-003-2
    title: "Merge Reified RSpaces chain (#328-#338)"
    status: pending
    blocked_by: [TASK-003-1]
    acceptance:
      - "All 11 PRs (#328 through #338) merged sequentially into rust/dev"
      - "CI passes after each merge"

  - id: TASK-003-3
    title: "Merge Tier 2 PRs if ready"
    status: pending
    acceptance:
      - "#466 (Embers) reviewed — merged or deferred"
      - "#186 (eval cost) reviewed — merged or deferred"
      - "#281 (LMDB fixes) reviewed — merged or deferred"

  - id: TASK-003-4
    title: "Tag final f1r3node release"
    status: pending
    blocked_by: [TASK-003-2, TASK-003-3]
    acceptance:
      - "Tag rust-v0.4.12 (or appropriate version) created on f1r3node rust/dev"
      - "phase_1_critical_prs.status set to 'complete' in /tmp/migrationPlan.md"
      - "phase_1_critical_prs.final_tag populated"
---
```

**Context:** The Reified RSpaces chain (#328-#338) is a major architectural change that must land before code sync. This phase is owned by the agent working in the f1r3node repository. Completion is signaled via the shared migration plan file.

**Scope:**

- Included: Merging blocking and ready PRs into f1r3node rust/dev
- Excluded: Any work in f1r3node-rust (that starts in EPIC-004)

**Notes:**

- The 11-PR Reified RSpaces chain has a sequential dependency — each PR targets the previous one
- Chain base (#328) depends on `new_parser` branch which depends on `rholang-rs#83`
- Monitor `/tmp/migrationPlan.md` for `phase_1_critical_prs.status` to know when to start EPIC-004

---

### EPIC-004: Code Sync to f1r3node-rust

```yaml
---
epic_id: EPIC-004
title: "Code Sync to f1r3node-rust"
status: in_progress
priority: p0
user_story: US-002
blocked_by: [EPIC-003]
created_at: 2026-04-09
claimed_by: claude-session-epic004
claimed_at: 2026-04-17T19:19:55Z
source_branch: rust/staging
source_head: fb59611fbf2be202a6d6450850de1435c9dec7a4
tasks:
  - id: TASK-004-1
    title: "Sync Rust workspace crates from f1r3node rust/staging"
    status: review
    claimed_by: claude-session-epic004
    claimed_at: 2026-04-17T19:19:55Z
    completed_at: 2026-04-29T18:50:45Z
    notes:
      - "Initial sync: 11 crates + root workspace files from f1r3node rust/staging @ 6ee5c390 (2026-04-17)"
      - "Re-sync: refreshed to f1r3node rust/staging @ fb59611f (2026-04-29) — 39 upstream commits, 539 files modified, 1 deleted, 5 new"
      - "Re-sync preserves local heed 0.22 upgrade (315b23b, 111e318): rspace++/Cargo.toml, shared/Cargo.toml pinned to heed = \"0.22.1\"; lmdb_*.rs files unchanged from HEAD"
      - "Per-crate Cargo.lock files added to .gitignore (only workspace /Cargo.lock is authoritative)"
      - "cargo build --workspace passes (49s)"
      - "./scripts/run_rust_tests.sh passes: 68 test runs, 0 failed"
      - "Full sync reports: docs/work-logs/task-004-1-2026-04-17T19-19-55Z.md (initial), docs/work-logs/task-004-1-resync-fb59611f-2026-04-29T18-50-45Z.md (current)"
      - "Not committed yet; user to invoke /quick-commit after review"
    acceptance:
      - "All 11 workspace crates updated from f1r3node rust/staging HEAD (fb59611f)"
      - "Cargo.toml workspace dependencies match source"
      - "cargo build --workspace succeeds"
      - "./scripts/run_rust_tests.sh passes per-crate"

  - id: TASK-004-2
    title: "Port CI/CD workflows"
    status: pending
    blocked_by: [TASK-004-1]
    acceptance:
      - "build-test-and-deploy.yml ported (Docker build, multi-arch, artifact publishing)"
      - "release.yml ported (automated versioning, changelog, tagging)"
      - "cliff.toml ported (changelog generation)"
      - ".github/apt-dependencies.txt ported"
      - "Docker image name set to f1r3fly-rust in CI"

  - id: TASK-004-3
    title: "Port Docker configuration"
    status: pending
    blocked_by: [TASK-004-1]
    acceptance:
      - "node/Dockerfile updated with correct image labels"
      - "docker/standalone.yml, shard.yml, observer.yml, validator4.yml ported"
      - "docker/monitoring/ (Prometheus, Grafana) ported"
      - "docker/conf/ (node config templates) ported"
      - "docker/genesis/ (bonds, wallets) ported"
      - "docker/.env.example ported"
      - "All compose files reference f1r3fly-rust image name"

  - id: TASK-004-4
    title: "Port scripts and local dev configuration"
    status: pending
    blocked_by: [TASK-004-1]
    acceptance:
      - "scripts/version.sh ported"
      - "scripts/clean_rust_libraries.sh ported"
      - "scripts/delete_data.sh ported"
      - "scripts/run_rust_tests.sh ported"
      - "run-local/ configuration ported"

  - id: TASK-004-5
    title: "Set version and create initial tag"
    status: pending
    blocked_by: [TASK-004-1, TASK-004-2]
    acceptance:
      - "node/Cargo.toml version continues from f1r3node's last release"
      - "Tag v0.4.12 (or matching version) created on f1r3node-rust"
      - "phase_2_code_sync.status set to 'complete' in /tmp/migrationPlan.md"
      - "phase_2_code_sync.synced_from_commit populated"
---
```

**Context:** Brings f1r3node-rust to full parity with post-merge f1r3node rust/dev. This is the core migration step — after this, f1r3node-rust becomes the canonical source of truth.

**Scope:**

- Included: All Rust crates, CI/CD, Docker, scripts, local dev config, version tagging
- Excluded: Issue migration (EPIC-005), external repo updates (EPIC-006)

**Notes:**

- The code delta is ~4 releases (v0.4.9-v0.4.11) plus the critical PRs from EPIC-003
- Docker image renamed from `f1r3fly-rust-node` to `f1r3fly-rust`
- Version drops the `rust-` tag prefix (no longer needed in a Rust-only repo)
- Run tests per-crate to avoid LMDB lock contention (see commit f2b4b5f)

---

### EPIC-006: External Repo Updates

```yaml
---
epic_id: EPIC-006
title: "External Repo Updates"
status: pending
priority: p1
user_story: US-002
blocked_by: [EPIC-004]
created_at: 2026-04-09
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-006-1
    title: "Update system-integration repo"
    status: pending
    acceptance:
      - "Docker image references updated from f1r3fly-rust-node to f1r3fly-rust"
      - "CI triggers updated to reference f1r3node-rust repo"
      - "Integration tests pass against new image"

  - id: TASK-006-2
    title: "Update pyf1r3fly repo"
    status: pending
    acceptance:
      - "Repo references in docs and CI updated"
      - "PR #4 cross-reference updated (references f1r3node #407)"

  - id: TASK-006-3
    title: "Verify rholang-rs compatibility"
    status: pending
    acceptance:
      - "rholang-rs git rev reference in Cargo.toml confirmed working"
      - "No changes needed (already independent)"
      - "phase_4_external.status set to 'complete' in /tmp/migrationPlan.md"
---
```

**Context:** Downstream consumers need to point at the new repo and Docker image name. system-integration and pyf1r3fly are the primary consumers. rholang-rs is already independent.

**Scope:**

- Included: system-integration, pyf1r3fly, rholang-rs verification
- Excluded: Any other F1R3FLY-io repos not listed

---

### EPIC-007: PR Cleanup & Redirect

```yaml
---
epic_id: EPIC-007
title: "PR Cleanup & Redirect"
status: pending
priority: p1
user_story: US-002
blocked_by: [EPIC-004]
created_at: 2026-04-09
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-007-1
    title: "Redirect Tier 3 PRs to f1r3node-rust"
    status: pending
    acceptance:
      - "PRs #457, #426, #424, #407, #405 receive redirect comment"
      - "Comment includes rebase instructions for f1r3node-rust"
      - "PRs closed on f1r3node"

  - id: TASK-007-2
    title: "Close Tier 4 (Scala) PRs"
    status: pending
    acceptance:
      - "PRs #470, #314, #185 receive deprecation comment"
      - "PRs closed on f1r3node"
      - "phase_5_pr_cleanup.status set to 'complete' in /tmp/migrationPlan.md"
---
```

**Context:** All open PRs on f1r3node must be resolved. Tier 3 PRs (viable Rust work) get redirect instructions. Tier 4 PRs (Scala) are closed with deprecation notice.

**Scope:**

- Included: Commenting and closing PRs on f1r3node
- Excluded: Tier 1/2 PRs (handled in EPIC-003)

---

### EPIC-008: Deprecation & Archive

```yaml
---
epic_id: EPIC-008
title: "Deprecation & Archive"
status: pending
priority: p2
user_story: US-002
blocked_by: [EPIC-005, EPIC-006, EPIC-007]
created_at: 2026-04-09
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-008-1
    title: "Update f1r3node README with deprecation notice"
    status: pending
    acceptance:
      - "README.md updated on rust/dev, main, and default branch"
      - "Notice points to F1R3FLY-io/f1r3node-rust"
      - "Last Rust release version documented"

  - id: TASK-008-2
    title: "Update GitHub repo metadata"
    status: pending
    acceptance:
      - "Repository description set to 'DEPRECATED - See F1R3FLY-io/f1r3node-rust'"

  - id: TASK-008-3
    title: "Disable CI and close remaining items"
    status: pending
    blocked_by: [TASK-008-1]
    acceptance:
      - "All GitHub Actions workflows disabled on f1r3node"
      - "Any remaining open issues closed with redirect comment"

  - id: TASK-008-4
    title: "Archive f1r3node repository"
    status: pending
    blocked_by: [TASK-008-1, TASK-008-2, TASK-008-3]
    acceptance:
      - "Repository archived (read-only) on GitHub"
      - "phase_6_deprecation.status set to 'complete' in /tmp/migrationPlan.md"
      - "phase_6_deprecation.archived set to true"
---
```

**Context:** Final step — makes f1r3node read-only and redirects all traffic to f1r3node-rust. This must not happen until all issues, PRs, and external repos are handled.

**Scope:**

- Included: README update, repo metadata, CI disable, archive
- Excluded: Any further development in f1r3node

**Notes:**

- Do NOT archive until Phases 5-7 are confirmed complete
- The other agent in f1r3node should NOT start this until signaled

---

### EPIC-009: Distributed OCI Testbed for Latency Benchmarking

```yaml
---
epic_id: EPIC-009
title: "Distributed OCI Testbed for Latency Benchmarking"
status: in_progress
priority: p2
user_story: US-003
blocked_by: []
created_at: 2026-04-13
claimed_by: claude-session-epic009
claimed_at: 2026-04-13T19:00:00Z
tasks:
  - id: TASK-009-1
    title: "OCI VPS provisioning scripts"
    status: review
    claimed_by: claude-session-epic009
    completed_at: 2026-04-13T19:05:00Z
    acceptance:
      - "scripts/remote/oci-provision.sh creates a dedicated f1r3node-rust-testbed-vcn in us-sanjose-1"
      - "Creates 2x VM.Standard.A1.Flex (arm64 Ampere) instances in f1r3fly-devops compartment"
      - "Security list opens TCP 40400-40405 and UDP 40404 to 0.0.0.0/0 (public testbed)"
      - "SSH access provisioned via a dedicated testbed keypair"
      - "Teardown script (oci-destroy.sh) removes VMs, VCN, and security rules cleanly"
    notes:
      - "Code complete (commit be7ad3f); dry-run validated end-to-end"
      - "Real --apply validation deferred to TASK-009-4+ integration"
      - "Security list range (40400-40405) may need widening in TASK-009-3 to accommodate 3 nodes on VPS-2"

  - id: TASK-009-2
    title: "Image distribution via docker save + scp + load"
    status: review
    claimed_by: claude-session-epic009
    blocked_by: [TASK-009-1]
    completed_at: 2026-04-13T19:10:00Z
    acceptance:
      - "scripts/remote/image-transfer.sh: local docker save | scp | remote docker load"
      - "Works against both VPSes in a single invocation (parallel transfer)"
      - "Image tag matches what distributed compose files reference"
      - "Migration note captured: once OCIR first-publish lands, switch to docker pull on VPS"
    notes:
      - "Code complete (commit 6e045c0); dry-run validated with fabricated state"
      - "Real --apply pending live VPSes"

  - id: TASK-009-3
    title: "Distributed compose file split"
    status: review
    claimed_by: claude-session-epic009
    claimed_at: 2026-04-13T19:15:00Z
    completed_at: 2026-04-13T19:30:00Z
    blocked_by: [TASK-009-1]
    acceptance:
      - "docker/shard.vps1.yml runs bootstrap only; parameterized by BOOTSTRAP_HOST env"
      - "docker/shard.vps2.yml runs 2 validators + observer; connects to BOOTSTRAP_HOST:40400"
      - "No reliance on Docker internal DNS for inter-host communication"
      - "Both files read from a shared .env.remote template"
    notes:
      - "VPS-2 runs 3 rnode processes sharing one public IP; each needs a distinct port-band to avoid protocol-port collision"
      - "Added 3 per-node conf files (validator1-remote.conf, validator2-remote.conf, readonly-remote.conf) that HOCON-include default.conf and override protocol-server.port / peers-discovery.port / api-server.port-*"
      - "Widened oci-provision.sh security list from 40400-40405/tcp+40404/udp to 40400-40455/tcp+40400-40455/udp to cover all 3 port-bands (supersedes TASK-009-1 AC wording)"
      - "Revisit: if node binary exposes --protocol-port / --discovery-port CLI flags, the per-node conf files could be replaced with inline compose args (would drop ~45 lines)"

  - id: TASK-009-4
    title: "Justfile recipes for end-to-end orchestration"
    status: review
    claimed_by: claude-session-epic009
    claimed_at: 2026-04-13T19:40:00Z
    completed_at: 2026-04-13T19:58:00Z
    blocked_by: [TASK-009-1, TASK-009-2, TASK-009-3]
    acceptance:
      - "just vps-up: provisions 2 VPSes and returns their public IPs"
      - "just vps-deploy: scp config + images, start bootstrap (VPS-1), then validators/observer (VPS-2)"
      - "just vps-status [target]: shows shard health via HTTP API and metrics endpoint"
      - "just vps-down: tears down all OCI resources created by vps-up"
    notes:
      - "Justfile prefix renamed oci- -> vps- per user direction to stay cloud-agnostic; BACKLOG-FI-002 captures the AWS/GCP generalization plan"
      - "Added scripts/remote/deploy.sh (renders .env.remote from template, parallel scp, bootstrap-then-followers startup, HTTP /api/status readiness poll)"
      - "Added scripts/remote/status.sh (per-node /api/status + /metrics check, non-zero exit on unhealthy)"
      - "Added scripts/remote/teardown.sh (docker compose down -v on both VPSes, separate from OCI termination)"
      - "Plus convenience recipe vps-image-push wrapping image-transfer.sh"
      - "Dry-run validated end-to-end; full apply-run deferred pending live VPS decision"

  - id: TASK-009-5
    title: "Port latency benchmark (Scala -> native grpcurl/curl)"
    status: review
    claimed_by: claude-session-epic009
    claimed_at: 2026-04-13T21:19:00Z
    completed_at: 2026-04-13T21:25:00Z
    blocked_by: [TASK-009-4]
    acceptance:
      - "scripts/bench/latency-benchmark.sh: drops rust-client external dependency, uses grpcurl + HTTP /api"
      - "Parameterized for arbitrary validator count (not hardcoded to 3)"
      - "Emits load-summary.txt and p50/p95 latency report"
      - "just bench-latency HOST DURATION wraps the script"
      - "scripts/bench/profile-casper-latency.sh ported for Rust node log format"
    notes:
      - "Implementation uses `node deploy` (via docker exec / ssh) for deploy signing rather than raw grpcurl — grpcurl can't produce secp256k1 signatures without a pre-signer binary. The AC intent (drop rust-client external dep) is met; interpretation documented in the script header."
      - "Uses curl for /api/status preflight and node CLI (show-blocks, last-finalized-block) for block/deploy matching. No external-repo dependencies."
      - "Parameterized via --duration, --rate, --host, --container, --http-port, --out-dir flags plus PHLO_LIMIT/PHLO_PRICE/DEPLOYER_KEY env"
      - "Default deployer key is bootstrap's (funded locally and in wallets.txt via commit 993c239 for distributed)"
      - "Justfile recipe: just vps-bench-latency host=<ip> duration=60 rate=2"
      - "profile-casper-latency.sh parses Rust JSON logs (targets f1r3fly.propose.timing + f1r3fly.casper) for per-validator propose_core_ms / block_replay_ms / finalizer_cycle_ms p50/p95"
      - "Real-apply validation against a live shard deferred (same as TASK-009-1..4)"
---
```

**Context:** Stands up a realistic multi-host deployment (single shard distributed across 2 VPSes) to measure network-latency-bound consensus performance. This is distinct from in-process or single-host Docker tests — it exercises the P2P transport, Kademlia discovery, and Casper finalization under real inter-host latency.

**Scope:**

- Included: OCI provisioning, image distribution, distributed compose, deploy/teardown automation, latency benchmark port
- Excluded: Inter-shard consensus (Option B, ~1,500+ LOC of consensus work — see BACKLOG-FI-001)
- Excluded: Non-OCI providers (Tata cloud, etc.)
- Excluded: Throughput, chaos, or whiteblock-plan benchmarks (future epics)
- Excluded: Production-grade secrets management (using `scp` for TLS keys for now)

**Notes:**

- Uses arm64 (VM.Standard.A1.Flex) for free-tier eligibility and production representativeness
- Image distribution intentionally uses `docker save/load` rather than registry pull, to keep this epic self-contained until the OCIR CI switch lands
- TLS keys for bootstrap are shipped via `scp` (acceptable for a throwaway testbed)

---

### EPIC-010: Soak Benchmark Metrics & Reporting

```yaml
---
epic_id: EPIC-010
title: "Soak Benchmark Metrics & Reporting"
status: in_progress
priority: p2
user_story: US-004
blocked_by: []
created_at: 2026-07-15
claimed_by: claude-session-810424d7
claimed_at: 2026-07-15T21:35:00Z
tasks:
  - id: TASK-010-1
    title: "Per-iteration metrics emission in run-merge-recovery-soak.sh"
    status: in_progress
    acceptance:
      - "Each iteration writes metrics.json to its ITERATION_DIR: wall-clock duration, pytest pass/fail/error counts (parsed from pytest.log), provider, exit code"
      - "Run-level summary.json aggregates: iterations, failure rate, iterations/hour throughput, per-provider split, target ref/sha, started/finished timestamps"
      - "summary.json uploaded as a workflow artifact by merge-recovery-soak.yml"

  - id: TASK-010-2
    title: "Node resource + finalization sampling during soak iterations"
    status: in_progress
    blocked_by: [TASK-010-1]
    acceptance:
      - "Peak node RSS per iteration captured (docker stats for docker provider; harness resource_monitor output for subprocess provider) into metrics.json"
      - "Deploy-to-finalized latency samples (p50/p95) extracted per iteration from test_load.py timings or node JSON logs (f1r3fly.propose.timing targets)"
      - "Both metrics roll up into summary.json"

  - id: TASK-010-3
    title: "Week-over-week compare step with release-gate verdict"
    status: in_progress
    blocked_by: [TASK-010-1, TASK-010-2]
    acceptance:
      - "Compare job fetches previous week's summary.json (from the Pages data history) and computes deltas for: failure rate, throughput, peak RSS, finalization latency"
      - "Regression thresholds are configurable in one place; PROPOSED DEFAULTS (need maintainer sign-off, see work log): failure rate +5 percentage points, RSS +20%, finalization p95 +20%, throughput -20%"
      - "Verdict (pass/regress + per-metric deltas) written to verdict.json artifact; a regression marks the soak workflow run failed"
      - "Release workflow refuses to promote unless the latest completed soak verdict is pass (explicit maintainer override documented)"

  - id: TASK-010-4
    title: "GitHub Pages trend dashboard"
    status: in_progress
    blocked_by: [TASK-010-1]
    acceptance:
      - "Pages enabled on the repo (source: GitHub Actions); site at f1r3fly-io.github.io/f1r3node-rust"
      - "Soak workflow appends each summary.json to a data history and redeploys the dashboard"
      - "Dashboard charts all four metrics across weeks with per-provider split and links to per-run artifacts"

  - id: TASK-010-5
    title: "OCI Notifications (ONS) Monday summary email"
    status: in_progress
    blocked_by: [TASK-010-3]
    acceptance:
      - "ONS topic (e.g. soak-benchmark-reports) exists; creation scripted (CLI or Terraform) OR documented as manually provisioned — OPEN QUESTION: who administers the topic (see work log)"
      - "Soak workflow publishes a plain-text Monday summary via instance-principal auth from the OCI runners (no new GitHub secrets): four metrics with week-over-week deltas, gate verdict, dashboard link"
      - "Subscription/unsubscription flow documented for contributors (ONS confirmation + unsubscribe links)"

  - id: TASK-010-6
    title: "Close the two failure modes that hid the soak breaking for days"
    status: pending
    acceptance:
      - "merge-recovery-soak.yml's SYSTEM_INTEGRATION_REF is covered by build_base's pin-drift check, alongside .github/oci-validation.env and _integration-pipeline.yml. It is a THIRD pin site that nobody knew existed: CI's pin advanced to 06f2020c while the soak's sat at a50eeb19, which predated system-integration 81284fc (adding integration-tests/certs/validator4). compose.py bind-mounts that path, so Docker created a directory and every node died on 'Failed to read the X.509 certificate: IO error: Is a directory (os error 21)'. Fixed for now by 4879a1f6; the guard is what stops it recurring."
      - "A schedule-gate no-op is distinguishable from a real pass without opening the run. Two cron slots fire nightly; the 19:30 Pacific slot runs the real soak and the 20:30 slot no-ops and reports success. From 2026-07-27 the real soak failed at bring-up every night while the workflow showed green, because the no-op is the later run. The job already prints a ::notice saying no soak was attempted — that is not enough, since the signal people read is the check mark."
      - "Regression coverage: the soak runs integration-tests/test/tests/custom/test_load.py, which the CI integration matrix explicitly --deselects. Any test only the soak runs needs either CI coverage or an explicit note that the soak is its sole gate, otherwise CI stays green through soak-only breakage."

  - id: TASK-010-7
    title: "Make system-integration's compartment reaper soak-aware (cross-repo)"
    status: pending
    external: true
    external_repo: F1R3FLY-io/system-integration
    coordination_note: "Executed by the agent working in ../system-integration. Coordinate via that repo's docs/ToDos.md — NOT docs/discoveries/, whose *.md contents are gitignored here (.gitignore:123) and so do not survive as a durable trace."
    acceptance:
      - "ci/oci-runners/reap-stale-runners.sh no longer terminates live soak runners. As of pin 9ebdde01 its OCI query filters ONLY on lifecycle-state == RUNNING and time-created < now - MAX_AGE_HOURS (default 6) — no display-name filter and no freeform-tag check — so it is blind to the soak-deadline-epoch exemption added by f1r3node-rust PR #169 and would kill a 22h/60h soak at hour 6. LATENT, NOT ACTIVE: no workflow schedules it at that SHA (.github/workflows contains only smoke-test.yml), so the hazard is a manual invocation. Fix mirrors ci-runner-reaper.yml: restrict to the ephemeral name prefixes and honour soak-deadline-epoch before terminating."
      - "Same script must also stop terminating long-lived golden images (ci-runner-golden-*), which the unfiltered age query sweeps up too; this is the reaper gap the system-integration agent previously supplied a diff for."
      - "Soak runners carry their own name prefix. launch-runner.sh builds RUNNER_NAME=ci-eph-$REPO_SLUG-$ARCH-$TS-$RAND, so a soak VM is indistinguishable from a 45-minute CI runner by name alone and any future age-based rule matches it by accident."
      - "cloud-init-runner.yml.tmpl schedules an on-instance self-destruct sized to a per-run dollar budget (~$12 daily / ~$33 weekend at VM.Standard.E6.Flex 16 OCPU / 32GB per state.env) — the last line of defence when both GitHub and the reaper fail."
      - "Soak VMs carry a cost-tracking freeform tag, with a monthly OCI budget and 80/100% alerts scoped to it. Note the enforcement is the VM lifetime, not the budget: OCI budgets are monthly and alert-only and cannot stop a running resource."
      - "launch-runner.sh tags the instance atomically at creation (oci compute instance launch --freeform-tags) rather than leaving it to a follow-up update. Validation run 30584775602 proved why: the launcher returns as soon as OCI accepts the launch call, but the instance keeps transitioning through PROVISIONING, and `instance update` against it is refused with HTTP 409 'currently being modified, try again later' — 3s after launch, which failed the whole launch job. f1r3node-rust now retries for ~3min (commit ea566d8a), which works but is a workaround: tagging at creation removes the race entirely and is the only way a tag can be guaranteed present from the instance's first instant, closing the window in which a reaper could see an untagged soak VM. Applies equally to the cost-tracking tags requested above."
      - "conftest.py's --rss-ceiling-mb default (5000, conftest.py:94) is raised to a host-relative value. This is a defect, not a tuning preference, and our SOAK_RSS_CEILING_MB override is a workaround that leaves it armed for every other caller. test_load.py fixes its shard at 6 nodes (test_load.py:220, '4 genesis validators (6 nodes total with boot + readonly)', include_readonly=True at :232), and that shard peaks ~9.9-10.8GB on ANY host — so the default sits at roughly half the working set of the harness's own primary load test, and kills it identically on a 64GB workstation. It is correct only on genuinely small hosts (<~12GB), where the test cannot run anyway, which is what makes the flat value look defensible. Why it went unnoticed: _integration-pipeline.yml:482 --deselects test_load.py, so CI never runs it and the soak was its only automated caller — and the soak never got past bring-up until 2026-07-30. Suggested shape: max(floor, MemTotal - headroom), keeping 5000 as the small-host case. Sequence after a clean soak: it is a shared default touching every caller. Also note --host-free-floor-mb (conftest.py:105, default 2000, subprocess-only) is a second always-on guard the ceiling override does not touch."

  - id: TASK-010-8
    title: "De-duplicate the CI runner compartment OCID without weakening the reaper"
    status: review
    claimed_by: claude-session-9f68c6fa
    completed_at: 2026-07-30T22:20:00Z
    branch: chore/reaper-compartment-invariant
    notes:
      - "Resolved by asserting equality rather than de-duplicating: check-workflow-invariants.sh gained invariant 5, which fails CI when the two literals diverge or when neither file pins one any more. Both sites now carry cross-referencing comments naming the other and the enforcing check."
      - "The de-duplication framing in the first acceptance line was the wrong shape and is superseded by the second: a repo variable is admin-mutable, and the reaper's blast-radius guarantee depends on the value being immutable in-repo. Equality-under-CI keeps both properties."
      - "Mutation-tested: a divergent OCID fails, and removing both literals fails with a message naming the cause. That testing caught a real defect in the guard itself — under set -e a no-match grep inside a command substitution killed the script before it could print why, making the 'nobody pins it any more' branch unreachable. Fixed with `|| true` on both greps; a guard that cannot explain itself is the failure mode this file exists to prevent."
      - "OPEN — cross-repo blind spot, raised by claude-session-02f66bb7. There is a THIRD site holding this OCID that the invariant cannot see: system-integration's ci/oci-runners/state.env COMP, which launch-runner.sh uses to CREATE instances and reap-stale-runners.sh uses to scan them. Verified byte-identical today. Not guarded here because the check would need a network fetch of the pinned SYSTEM_INTEGRATION_REF inside the Lint job, and because divergence there fails CLOSED rather than silently: the launcher would create the instance in one compartment while our tagging step lists the other, find no instance, and fail the launch. Loud and immediate, unlike the same-repo divergence this invariant guards, which would be silent until a soak died at 2h. Revisit if a cheap deterministic check appears — the ref is pinned, so a fetch would be reproducible."
    acceptance:
      - "CI_RUNNER_COMPARTMENT_OCID stops being hardcoded in two places — .github/workflows/ci-runner-reaper.yml and the 'Exempt runner from the CI reaper' step in .github/workflows/merge-recovery-soak.yml. A compartment migration currently needs coordinated edits, and changing only one side silently leaves soak runners either untagged (reaped mid-run) or un-reapable."
      - "The chosen mechanism does not weaken the reaper's blast-radius guarantee. A repo-level Actions variable is mutable by anyone with repo admin, whereas the present hardcoding is precisely why the reaper 'can never touch other compartments' (its own comment, which is load-bearing). Preferred option: keep both literals pinned in-repo and add an assertion to .github/scripts/check-workflow-invariants.sh that they match, so drift fails CI while the value stays immutable."
      - "Raised by xai in the PR #169 multi-review and deliberately deferred from that PR: it touches the reaper's security posture and should not ride a same-day hotfix."
---
```

**Context:** Implements US-004 plus the delivery/reporting design agreed 2026-07-15: the 72h soak concludes Mondays (weekly cadence); metrics are published to a GitHub Pages trend dashboard (pull) and a plain-text ONS email (push); regressions gate releases. Full design rationale, alternatives considered (email-only, Discussions, bot-committed reports), and open questions are in `docs/work-logs/task-EPIC-010-2026-07-15T20-57Z.md`.

**Scope:**

- Included: metrics emission, resource sampling, compare+gate, Pages dashboard, ONS email
- Excluded: PR #72 residual par-serialization benchmarking (separate concern)
- Excluded: HTML email formatting (ONS is plain-text; detail lives on the dashboard)

---

### EPIC-015: Casper Test Infrastructure Congruence

```yaml
---
epic_id: EPIC-015
title: "Casper Test Infrastructure Congruence"
status: pending
priority: p2
user_story: US-005
blocked_by: []
created_at: 2026-08-11
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-015-1
    title: "Deepen the Casper test node"
    status: pending
    priority: p2
    discovered_in: docs/work-logs/casper-test-node-congruence-baseline-2026-08-19.md
    notes:
      - "Baseline 2026-08-19: the duplicate casper/tests helper tree and the canonical casper/src/rust/test_utils tree diverged by roughly 1,000 lines; the duplicate tree holds create_network_with_deploy_lifespan and the MultiParentCasper-typed accessor, which the canonical tree lacks. The first consolidation attempt (976b7a252, PR #230) is superseded. See the discovered_in work log for the full baseline and provenance."
    glossary_terms:
      - docs/Glossary.md#test-node
      - docs/Glossary.md#block-proposal
      - docs/Glossary.md#block-validation
    dependency_category: local-substitutable
    accepted_design: common-caller
    tdd_plan: docs/tdd-plans/casper-test-node-2026-08-11T02-59-57Z.md
    acceptance:
      - "Standalone and network scenarios exercise production-shaped behavior through the test node interface documented at docs/Glossary.md#test-node"
      - "The common caller can create a standalone test node or a configured test network without learning storage, runtime, transport, or consensus-construction details"
      - "The test network interface exercises block proposal, publication, propagation, synchronization, and block validation while preserving existing observable outcomes"
      - "Empty-block behavior, bootstrap selection, parent limits, synchrony settings, and read-only nodes remain expressible as explicit configuration with behavior tests"
      - "Focused inspection required by tests crosses named test node accessors; tests do not initialize or copy fields of the consensus implementation"
      - "Local storage, runtime, and transport stand-ins remain at internal seams; tests do not mock internal collaborators"
      - "Features that exist only in the duplicate tree (at minimum create_network_with_deploy_lifespan and the MultiParentCasper-typed accessor) are ported to the canonical fixtures before the duplicate tree collapses to re-exports"
      - "Old duplicate fixture tests are replaced rather than layered, and removing the duplicate helper tree does not move construction complexity into callers"
      - "Each TDD cycle covers one behavior at a time, and test names cite docs/Glossary.md#test-node plus any applicable block-proposal or block-validation anchor"
---
```

**Context:** Candidate C1 from the Casper architecture regression diagnosis was accepted with the common-caller design. Two independently evolving helper trees currently expose overlapping test-node behavior, while integration tests retain direct access to consensus implementation fields.

**Scope:**

- Included: one canonical test-node module, standalone and configured-network entry points, network scenario operations, focused inspection accessors, caller migration, and duplicate-tree removal
- Excluded: changing consensus semantics, reopening settled slashing decisions, introducing remote ports, or implementing deploy-admission and block-validation candidates C2 and C3

---

## Epic Dependency Graph

```text
EPIC-011 (TLA exhaustive baseline, complete) ─> EPIC-012 / TASK-012-22
EPIC-012 (open-issue PR queue)              (all other lanes start independently)

EPIC-001 (system-integration alignment)    EPIC-003 (f1r3node: merge critical PRs)
EPIC-002 (monitoring separation)               |
                                                 v
                                            EPIC-004 (f1r3node-rust: code sync)
                                                 |
                                            +----+----+----+
                                            |    |    |    |
                                            v    v    v    v
                                          005  006  007
                                        (issues)(repos)(PRs)
                                            |    |    |
                                            +----+----+
                                                 |
                                                 v
                                            EPIC-008
                                         (deprecation/archive)
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
2. **Claim task**: Use the [Implementer Identification](https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/common/stigmergic-collaboration.md#implementer-identification) format for `claimed_by`. Set `status: in_progress`
3. **Implement**: Use `/implement` to execute with full context
4. **Complete**: Mark `status: complete` when acceptance criteria met
5. **Move epic**: When all tasks complete, move epic to `docs/CompletedTasks.md`

---

## References

- **Shared Migration Plan:** `/tmp/migrationPlan.md`
- **User Stories:** `docs/UserStories.md`
- **Completed Work:** `docs/CompletedTasks.md`
- **Backlog:** `docs/Backlog.md`
- **System-Integration Migration Plan:** `../system-integration/docs/migration-to-rust-node.md`
