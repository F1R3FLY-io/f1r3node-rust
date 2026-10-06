---
doc_type: user_stories
version: "1.1"
last_updated: 2026-09-30
---

# User Stories

This document captures user stories that drive feature development. User stories are reverse-engineered from completed epics and updated as new features are planned.

**Document Structure**
- Active stories: This file (`docs/UserStories.md`)
- Implementation tracking: `docs/ToDos.md` (epics and tasks)
- Completed work: `docs/CompletedTasks.md`

**Format:** Each story follows the standard template:
> As a [persona], I want [capability] so that [benefit].

**User Stories Standard Reference** (canonical):
[user-stories-standard.md](https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/common/user-stories-standard.md)

---

## Completed Stories

---

#### US-004: 60-hour merge-recovery soak benchmark metrics

> As a **release engineer validating consensus changes**, I want **capture benchmark metrics from the 60-hour merge-recovery soak (per-iteration throughput, failure rate, and node resource/finalization measurements) with a machine-readable summary artifact** so that **sustained-load performance and stability are measurable and comparable across releases instead of pass/fail only**.

**Implemented in:** EPIC-010

**Status:** Planned

**Acceptance Criteria:**
- [ ] Each soak iteration records wall-clock duration, pytest pass/fail counts, and provider (docker/subprocess) in a per-iteration metrics file
- [ ] Node resource metrics (peak RSS, finalization latency) are sampled during each iteration and included in the metrics file
- [ ] A run-level summary artifact (JSON) aggregates iterations, failure rate, and throughput, and is uploaded by the merge-recovery-soak workflow
- [ ] Two soak runs can be diffed to detect performance regressions between refs

---


#### US-006: Verify the Casper soak harness

> As a **release engineer**, I want **to verify source-bound Casper harness behavior** so that **I can reject incomplete or misleading experiment evidence**.

**Implemented in:** EPIC-017
**User Flow:** FLOW-001

**Status:** In Progress

**Acceptance Criteria:**
- [ ] The harness preserves immutable identities and failure history.
- [ ] The workflow retains exact control verdicts and driver evidence.
- [ ] Claim verification remains separate from node observations.

---


#### US-010: Continuously running test net deployment

> As a **release maintainer**, I want **CI to deploy each test net candidate to continuously running test net shards on OCI** so that **releases soak on long-lived state beside earlier releases before stable promotion, and partners can test against a running network**.

**Implemented in:** EPIC-014

**Status:** In Progress

**Design source:** `docs/release-process.md` Section 12.1 (proposed amendment, 2026-10-06, pending ratification).

**Acceptance Criteria:**
- [ ] Only a test net candidate deploys to the test net. A test net candidate is a canary release that passed the 60h stability soak on its exact image digest.
- [ ] A CI workflow deploys the candidate image by digest to the test net on OCI with the existing fleet tooling.
- [ ] Each shard runs a mix of releases. Soaking nodes on the candidate run beside Anchors on earlier releases.
- [ ] All shards in one test net use the same consensus/SMR model. A candidate deploys only to the test net of its own model. The CBC Casper test net is the first test net.
- [ ] A candidate that cannot interoperate with its shard starts a new shard with a fresh genesis in the same test net.
- [ ] The Shard soak-in result publishes gate evidence, and stable promotion holds until that gate passes.
- [ ] A failed deployment removes the soaking nodes, keeps the Anchors running, and reports the failure.
- [ ] The deploy job runs only from protected refs with the release-credentials environment.

---

## Planned Stories

#### US-009: Bounded node behavior under resource faults

> As a **node operator**, I want **bounded retries and log storage** so that **resource faults do not fill deployment disks**.

**Implemented in:** EPIC-020

**User Flow:** FLOW-002

**Status:** In Progress

**Acceptance Criteria:**
- [x] TASK-020-1 bounds accept-error retries and log output while preserving error delivery and connection recovery.
- [x] TASK-020-1 applies no error backoff after a successful accept and releases listener resources when the consumer closes.
- [x] TASK-020-2 enforces byte limits for each log file and the complete log directory.
- [x] TASK-020-3 uses one deployment sink and verifies container log caps, including the coordinated system-integration change.
- [ ] TASK-020-4 stops the soak workload when node or container logs exceed their budgets.

**Verification Boundary:**

Hosted run 36651370411 passed all seven transport resource regressions and all 400 comm tests, with zero skipped tests.
The [evidence report](work-logs/evidence/task-020-1-hosted-20260930-01/report.json) records the tested source and assertion bounds.
These controlled tests do not verify complete deployment storage limits, the descriptor-exhaustion cause, or a live soak campaign.

TASK-020-2 adds 100 MiB per-file and 2 GiB directory limits for the file sink.
[Local verification](work-logs/task-020-2-byte-bounded-logging-20260930.md) passed 781 tests across shared, node, and comm, with zero skips.
TASK-020-3 now has [local deployment verification](work-logs/task-020-3-deployment-log-caps-20260930.md) for explicit single sinks and configured container caps.
The system-integration single-sink change merged into its `dev` branch through PR #146 at `ccd717195`.
The [external handoff](handoffs/task-020-3-system-integration-20260930.md) records the contract review and the sink contract for each deployment.
Guardian enforcement and a live deployment exercise remain separate obligations.
The story remains open until all applicable tasks and deployment checks pass.

---

#### US-005: Congruent Casper test infrastructure

> As a **consensus developer writing Casper specs**, I want **one canonical test-node and test-network fixture tree with a common-caller interface** so that **specs exercise production-shaped behavior without duplicated helpers that drift apart and silently lose capabilities**.

**Implemented in:** EPIC-015

**Status:** Planned

**Acceptance Criteria:**
- [ ] A common caller creates a standalone test node or a configured test network without learning storage, runtime, transport, or consensus-construction details
- [ ] Features that exist only in the duplicate `casper/tests` helper tree are ported to the canonical fixtures before the duplicate tree collapses to re-exports
- [ ] The duplicate helper tree is removed and the two fixture trees can no longer diverge
- [ ] Behavioral variation (parent limits, synchrony, read-only nodes, bootstrap selection, empty blocks, deploy lifespan) is explicit configuration with behavior tests

---

#### US-001: System-Integration Compatibility

> As a **platform operator**, I want **f1r3node-rust's Docker configuration to be directly compatible with the system-integration orchestration tooling** so that **the migration from dual Scala/Rust support to Rust-only can proceed without manual fixups**.

**Implemented in:** EPIC-001, EPIC-002

**Acceptance Criteria:**
- [x] Genesis wallets.txt identical between repos (20 wallets, correct balances)
- [x] Docker image env var standardized to `F1R3FLY_IMAGE`
- [x] Shard network name standardized to `f1r3fly-shard`
- [ ] Monitoring separated into its own compose file (matches system-integration pattern)
- [ ] Shard verified to start with updated configuration
- [ ] system-integration's `services.yml` can point to this repo's `master` branch

**Completed:** Planned

---

#### US-002: Migrate to Standalone Rust Repository

> As a **F1R3FLY developer**, I want **the Rust blockchain node to live in a standalone repository (f1r3node-rust) with clean Cargo-only tooling** so that **we can iterate faster without Nix/SBT/Scala build complexity and contributors only need standard Rust tooling**.

**Implemented in:** EPIC-003 through EPIC-008

**Acceptance Criteria:**
- [ ] All critical PRs (Reified RSpaces #328-#338) merged in f1r3node before cutover
- [ ] f1r3node-rust at full parity with f1r3node rust/dev HEAD
- [ ] CI/CD pipeline produces Docker images from f1r3node-rust
- [ ] All 22 Rust-relevant issues migrated to f1r3node-rust
- [ ] External repos (system-integration, pyf1r3fly) point at f1r3node-rust
- [ ] f1r3node archived with deprecation notice
- [ ] Docker image published as `f1r3fly-rust` to Oracle Container Registry (`us-sanjose-1.ocir.io/axd0qezqa9z3/f1r3fly-rust`, public)
- [ ] Version continuity maintained (v0.4.x series)

**Completed:** Planned

---

#### US-003: Distributed OCI testbed for latency benchmarking

> As a **platform engineer**, I want **to deploy a single F1R3FLY shard across two isolated OCI VPSes and run repeatable latency benchmarks against it** so that **we can measure network-latency-bound consensus performance and detect regressions as the node evolves**.

**Implemented in:** EPIC-009

**Status:** In Progress

**Acceptance Criteria:**
- [ ] Justfile recipes provision and deploy a 2-VPS OCI testbed in us-sanjose-1 f1r3fly-devops compartment
- [ ] VPS-1 runs the bootstrap node; VPS-2 runs 2 validators and 1 read-only observer (single shard)
- [ ] Nodes discover each other over public internet via Kademlia and bootstrap URL (no Docker internal DNS)
- [ ] Genesis ceremony completes and the shard finalizes blocks end-to-end
- [ ] Latency benchmark ported from f1r3node run-latency-benchmark.sh; emits load summary and p50/p95 reports
- [ ] `just oci-down` tears down the testbed and frees all OCI resources
- [ ] Option B (inter-shard consensus) captured separately in Backlog.md as BACKLOG-FI-001

**Completed:** Planned

---

## Relationship to Epics

User stories capture the **why** (user need and benefit). Epics capture the **what** (technical implementation tasks).

| Artifact | Purpose | Location |
|----------|---------|----------|
| User Story | Business/user need | `docs/UserStories.md` |
| Epic | Implementation scope | `docs/ToDos.md` |
| Task | Technical work item | Nested in epic YAML |
| Acceptance Criteria | Definition of done | In user story |

**Workflow:**
1. Identify user need -> Create user story
2. Design solution -> Create epic with tasks
3. Implement -> Work through tasks via `/nextTask` and `/implement`
4. Complete -> Mark epic complete, update story status

---

## References

- **Task Tracking:** `docs/ToDos.md`
- **Completed Work:** `docs/CompletedTasks.md`
- **User Stories Standard** (canonical): [user-stories-standard.md](https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/common/user-stories-standard.md)

## Story Template

Use this template when adding new user stories:

```markdown
#### US-XXX: [Short Title]

> As a **[persona]**, I want **[capability]** so that **[benefit]**.

**Implemented in:** [EPIC-ID or "Planned"]

**Acceptance Criteria:**
- [ ] Criterion 1
- [ ] Criterion 2
- [ ] Criterion 3

**Completed:** [Date or "Planned"]
```

---
