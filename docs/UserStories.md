---
doc_type: user_stories
version: "1.2"
last_updated: 2026-08-01
---

# User Stories

This document captures user stories that drive feature development. User stories are reverse-engineered from completed epics and updated as new features are planned.

## Document Structure

- Active stories: This file (`docs/UserStories.md`)
- Implementation tracking: `docs/ToDos.md` (epics and tasks)
- Completed work: `docs/CompletedTasks.md`

**Format:** Each story follows the standard template:
> As a [persona], I want [capability] so that [benefit].

---

## Completed Stories

<!-- Add completed user stories here -->

---

### US-004: 60-hour merge-recovery soak benchmark metrics

> As a **release engineer validating consensus changes**, I want **capture benchmark metrics from the 60-hour merge-recovery soak (per-iteration throughput, failure rate, and node resource/finalization measurements) with a machine-readable summary artifact** so that **sustained-load performance and stability are measurable and comparable across releases instead of pass/fail only**.

**Implemented in:** EPIC-010

**Status:** Planned

**Acceptance Criteria:**

- [ ] Each soak iteration records wall-clock duration, pytest pass/fail counts, and provider (docker/subprocess) in a per-iteration metrics file
- [ ] Node resource metrics (peak RSS, finalization latency) are sampled during each iteration and included in the metrics file
- [ ] A run-level summary artifact (JSON) aggregates iterations, failure rate, and throughput, and is uploaded by the merge-recovery-soak workflow
- [ ] Two soak runs can be diffed to detect performance regressions between refs

---

## Planned Stories

### US-001: System-Integration Compatibility

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

### US-002: Migrate to Standalone Rust Repository

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

### US-003: Distributed OCI testbed for latency benchmarking

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

### US-005: Versioned valid-operation exercise epochs

> As a **release engineer validating consensus changes**, I want **a versioned catalog of bounded, operationally valid workload epochs** so that **known and newly discovered failure modes become durable, provider-compatible regression exercises instead of one-off incident knowledge**.

**Implemented in:** EPIC-011, EPIC-012

**Status:** Planned

**Acceptance Criteria:**

- [ ] Every workload has a permanent `SOAK-EPOCH-NNN` identity, semantic revision, normalized digest, definition SHA, declared limits, and expected invariants
- [ ] The initial catalog covers steady traffic, bursts, channel contention, large valid deploys, dependent transaction chains, and mixed contracts
- [ ] Workloads enforce valid signatures, balances, phlo limits, dependencies, shard routing, and finalized prerequisites
- [ ] Every non-provider-specific workload passes contract tests with Docker and subprocess providers
- [ ] Newly discovered valid-workload failures can be minimized, linked to provenance, and registered as experimental epochs

---

### US-006: Reproducible randomized weekend exercise coverage

> As a **release engineer operating weekend soaks**, I want **the soak to execute a seeded, weighted, coverage-constrained sequence of exercise epochs** so that **each weekend explores varied valid behavior while every executed sequence remains exactly reproducible**.

**Implemented in:** EPIC-013

**Status:** Planned

**Acceptance Criteria:**

- [ ] A complete weekend plan executes every required compatible epoch at least once before weighted selection fills remaining capacity
- [ ] The planned and actual sequence, run seed, per-epoch seed, provider, revision, and definition digest are recorded in artifacts
- [ ] Selection is deterministic for the same catalog, seed, provider constraints, and segment budget
- [ ] Epoch admission respects segment checkpoint deadlines and never weakens RSS or host-free safety limits
- [ ] Ordinary epoch failures preserve evidence and continue after a verified shard reset; safety, host, or reset failures stop immediately

---

### US-007: Replayable soak failure intake and promotion

> As a **node developer investigating soak regressions**, I want **failed exercise epochs to produce classified evidence and a deterministic replay path** so that **I can reproduce defects, verify fixes, and promote stable regressions into release-gating coverage**.

**Implemented in:** EPIC-014

**Status:** Planned

**Acceptance Criteria:**

- [ ] Every epoch result distinguishes workload failure, safety breach, host breach, reset failure, and infrastructure loss
- [ ] A failed execution emits a replay manifest with immutable identities, effective limits, first failing operation, evidence paths, and checksums
- [ ] Replay fails closed when the recorded definition SHA, digest, revision, topology, or provider is unavailable or incompatible
- [ ] Regression intake proves failure before a fix and success afterward before registration
- [ ] Experimental epochs require stable evidence and maintainer approval before promotion to release-gating status
- [ ] Run summaries expose catalog coverage and per-epoch outcomes before dashboard trend work begins

---

### US-008: Multi-shard randomized operational exercises

> As a **platform engineer validating shard interoperability**, I want **the stable exercise-epoch model extended to valid multi-shard workloads** so that **routing, cross-shard dependencies, convergence, and recovery are exercised under reproducible sustained load**.

**Implemented in:** EPIC-015

**Status:** Planned

**Acceptance Criteria:**

- [ ] Multi-shard topology and capability requirements are represented in the epoch contract
- [ ] Valid inter-shard operation profiles define finalized prerequisites and cross-shard invariants
- [ ] Seeded scheduling preserves required coverage across compatible topologies and providers
- [ ] Replay reconstructs the recorded topology, workload revision, routing decisions, and seeds
- [ ] Multi-shard epochs remain experimental until single-shard reset, replay, and evidence contracts are stable

---

### US-009: Auditable single-source CI dependency pins

> As a **CI and release maintainer**, I want **trusted system-integration and OCI CLI dependencies resolved from one reviewed JSONC registry** so that **catalog updates require one auditable pin edit while privileged workflows cannot drift or consume untrusted configuration**.

**Implemented in:** EPIC-016

**Status:** Planned

**Acceptance Criteria:**

- [ ] `.github/ci-pins.jsonc` is the only source for immutable system-integration runner/catalog SHAs and OCI CLI URL, version, and checksums
- [ ] Runner and catalog refs are split so routine epoch additions do not implicitly change privileged launcher code
- [ ] Every consumer resolves validated outputs before resource provisioning, and no workflow retains inline copies
- [ ] Fork-triggered privileged jobs use the trusted base runner pin; reviewed same-repository pin PRs can validate candidate catalog pins
- [ ] Missing, malformed, duplicated, mutable, or incompatible values fail closed before OCI resources launch
- [ ] Experimental epochs become eligible after a catalog pin bump, while required and release-gating promotion remains explicitly controlled in this repository
- [ ] Automation may propose one-line catalog pin bumps, but merge remains review-gated

---

## Relationship to Epics

User stories capture the **why** (user need and benefit). Epics capture the **what** (technical implementation tasks).

| Artifact | Purpose | Location |
| ---------- | --------- | ---------- |
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
