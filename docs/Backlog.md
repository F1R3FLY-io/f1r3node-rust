---
doc_type: backlog
version: "1.1"
last_updated: 2026-08-13
---

# Backlog

This document captures deferred work, future ideas, and low-priority items that aren't ready for active development.

**Document Structure**
- Active work: `docs/ToDos.md`
- User stories: `docs/UserStories.md`
- Completed work: `docs/CompletedTasks.md`
- Deferred work: This file (`docs/Backlog.md`)

---

## Backlog Categories

Items are organized by category and rough priority within each category.

---

### Technical Debt

Items that improve code quality, performance, or maintainability but aren't blocking active development.

---

#### BACKLOG-TD-001: Deterministic test for the soak finalization-lag failure

```yaml
---
backlog_id: BACKLOG-TD-001
title: "Encode the 'N deploy(s) not finalized within 45s' soak failure as a deterministic test once root-caused"
category: technical_debt
priority: p2
added_at: 2026-08-11
blocked_by: none (root-caused 2026-08-12)
repo_scope: node-side (casper deploy throughput) and/or system-integration (test_load assertion)
---
```

**Evidence:** Canary run 31554271086 (2026-08-12T01:38Z, target dev
`3ed832a2`, first soak on the 64GB/32-core VM.Standard.E6.Flex shape)
failed iteration 1 ~14 minutes in with
`AssertionError: 234 deploy(s) not finalized within 45s`, immediately after
the 1200-deploy sustained phase (4.0/sec). The shard was otherwise healthy:
no guardian breach, RSS peak 16.6GB (vs the 31–37GB envelope observed on
prior shapes), finalization p95 5928ms over 786 samples earlier in the run.

**Root-caused (2026-08-12):** not a flake, shape effect, or recent
regression — a sustained deploy-throughput ceiling (~3.4 deploys/s vs the
test's 4/s) present on both `dev` and `master` and both VM shapes; the
un-finalized cone never builds (tip−LFB stays 0). Full attribution with
per-block cost breakdown in
`docs/discoveries/2026-08-12-finalization-lag-root-cause.md`; node-side
fixes tracked on `fix/sustained-deploy-throughput`.

**Done means:** the failure mode is reproduced in a deterministic test —
now known to mean pinning sustained-rate deploy inclusion/finalization
throughput (deploys/s absorbed with a bounded queue) rather than cone
depth — either a node-side test under a fixed burst-deploy schedule
(precedent: the planned floor-divergence regression test) or a
system-integration harness test with a fixed seed/schedule — so the soak
stops being the only detector. The 45s assertion itself lives in
system-integration's `test_load.py`; node-side work belongs in `casper`.

### Feature Ideas

Future features that have been identified but aren't yet prioritized.

---

#### BACKLOG-FI-003: Per-core CPU sampling in the soak harness monitor

```yaml
---
backlog_id: BACKLOG-FI-003
title: "Sample per-CPU cgroup counters per node container so the dashboard CPU grid gains real core rows"
status: implemented_pending_merge
repo_scope: system-integration (monitor, PR #103), f1r3node-rust (driver + rollup, feature/enhance-soak-data-emission)
---
```

Implemented on paired branches (2026-08-11). system-integration PR #103
(`feature/enhance-soak-data-emission`) adds the monitor half: a per-sample
docker-exec probe reads cgroup v1 `cpuacct.usage_percpu` (falling back on
cgroup v2 — which has no per-CPU accounting — to attributing per-thread
`/proc` CPU-time deltas to each thread's current core) and emits
`resource-percore-timeseries.csv` (`elapsed_s,node,core,cpu_percent`) as a
separate file so the aggregate awk extractors cannot double-count. This
repo's same-named branch consumes it: the soak driver snapshots the CSV and
emits nested `cpu_peak_per_node_core_pct` per iteration, and
`write-soak-summary.sh` rolls real core rows into `cpu_peak_core_grid_pct`
per node, keeping the `"all"` fallback row for nodes without per-core data
(pre-emission history, providers without the hook). Remove this entry once
both branches merge.

#### BACKLOG-FI-002: Genericize testbed scripts for AWS / GCP

```yaml
---
backlog_id: BACKLOG-FI-002
title: "Abstract provisioning layer so testbed can run on AWS or GCP, not just OCI"
category: feature_idea
priority: p3
added_at: 2026-04-13
related_epic: EPIC-009
---
```

**Description:** Today `scripts/remote/oci-*.sh` directly call the `oci` CLI for VCN / subnet / security list / instance creation. The cloud-agnostic scripts (`deploy.sh`, `status.sh`, `teardown.sh`, `image-transfer.sh`) already work over generic SSH once a state file with public IPs exists, so only the provisioning/teardown layer needs abstraction. The `vps-*` Justfile prefix is deliberately neutral to keep the user-facing interface stable across providers.

**Probable approach:**
1. Define a provider interface (bash functions or a minimal YAML contract) — create_vcn, create_subnet, launch_instance, terminate_instance, destroy_vcn — with inputs/outputs matching the existing state-file schema
2. Rename `oci-*.sh` to `provision/oci.sh` and add peers `provision/aws.sh` (via `aws ec2 ...`) and `provision/gcp.sh` (via `gcloud compute ...`)
3. Front-end dispatcher (`scripts/remote/provision.sh`) picks a provider from `$TESTBED_PROVIDER` env (default `oci`)
4. Update `docs/vps-cloud-testing.md` Part C to one section per provider (OCI/AWS/GCP)
5. Justfile recipes (`vps-up`, `vps-down`) stay untouched — they call `provision.sh` which delegates

**When Unblocked:** After a second concrete deployment target is requested (e.g. user explicitly wants AWS for a production benchmark). Premature to abstract against one known provider only.

**Related work:** EPIC-009 establishes the OCI implementation that this would generalize. `vps-*` Justfile prefix is already chosen to outlive OCI-only.

---

#### BACKLOG-FI-001: Inter-Shard Consensus (Option B)

```yaml
---
backlog_id: BACKLOG-FI-001
title: "Inter-shard consensus (cross-shard bridge between two independent shards)"
category: feature_idea
priority: p3
added_at: 2026-04-13
related_epic: EPIC-009
---
```

**Description:** Make two independent F1R3FLY shards (e.g. `/root/east` and `/root/west`) agree on cross-shard state — relaying finalized blocks, bridging value, or anchoring child-shard finality into a parent shard. Today the `shard-name` and `parent-shard-id` config fields exist, blocks carry a `shard_id`, and bootstrap validates the shard name at genesis, but there is **zero** cross-shard consensus coordination. Two independent shards running simultaneously ignore each other entirely.

**Current state (as of 2026-04-13 research):**
- `shard-name` and `parent-shard-id` in `docker/conf/default.conf:214-215` — wired ✓
- `casper/src/rust/casper_conf.rs:19-22` — config struct deserialized ✓
- Block `shard_id` field in `models/src/rust/casper/protocol/casper_message.rs` — set at creation, validated at genesis only ✓
- `parent_shard_id` read after initialization — **never**
- Cross-shard routing in `comm/` — **not implemented**
- Bridge contracts in `rholang/` — **not implemented**
- Cross-shard deploy routing in `node/src/rust/api/` — **not implemented**

**Estimated scope:** ~1,500+ lines of net-new code across:
1. Bridge protocol + Rholang bridge contracts (~500 lines)
2. Cross-shard routing in `comm/` transport layer (~200 lines)
3. Fork-choice modifications for parent-shard ancestry weighting (~300 lines)
4. Deploy API shard routing (~150 lines)
5. Multi-shard genesis ceremony + configuration schema (~50 lines)
6. Integration tests for multi-shard deployments (~400 lines)

**When Unblocked:** Requires design doc + architectural review. Not ready for promotion to active epic until the hierarchical-shard model is fully specified and the bridge protocol has a reviewed spec.

**Related work:** EPIC-009 stands up a **single-shard** distributed testbed on OCI. If BACKLOG-FI-001 is promoted, the testbed from EPIC-009 would extend naturally to a 4-VPS multi-shard topology.

---

### Research & Exploration

Items that need investigation before they can become actionable tasks.

---

### Documentation

#### BACKLOG-DOC-001: Unify slashing notation glossary into docs/Glossary.md

```yaml
---
backlog_id: BACKLOG-DOC-001
title: "Fold docs/theory/slashing/design/02-glossary-and-notation.md into docs/Glossary.md"
category: documentation
priority: p3
added_at: 2026-08-05
related_epic: EPIC-011
requested_by: human-jeff (2026-08-05, during /review-codebase --glossary-only)
---
```

**Description:** `docs/Glossary.md` (created 2026-08-05, 12 canonical terms in
the load-bearing Preferred-usage format) and
`docs/theory/slashing/design/02-glossary-and-notation.md` (acronyms, symbol
tables, LTS labels, InvalidBlock taxonomy, theorem-naming conventions) should
become one document at `docs/Glossary.md`. Until then, `docs/Glossary.md`
links to `02` as authoritative for mathematical notation, and `02` remains
the citation target of the design-doc series.

**Probable approach:** Migrate `02`'s tables into `docs/Glossary.md` sections
(keeping GFM anchor compatibility), turn `02` into a redirect stub, and update
the design-doc series' internal cross-references (`§02` citations appear
throughout `03`–`15`; the maintenance rule requires every anchor to keep
resolving).

**When Unblocked:** After the current EPIC-011 tasks complete (maintainer's
explicit sequencing: "when complete with tasks").

---

### Dependencies & Blockers

Items waiting on external factors (upstream releases, third-party APIs, etc.)

---

#### BACKLOG-DB-001: system-integration Branch Reference

```yaml
---
backlog_id: BACKLOG-DB-001
title: "system-integration services.yml targets branch dev, repo uses master"
category: blocked_external
priority: p2
added_at: 2026-03-19
blocked_by_external: "system-integration migration Phase 2"
expected_resolution: "When system-integration updates services.yml to point to f1r3node-rust.git"
---
```

**Description:** system-integration's `services.yml` currently references `branch: rust/dev` on the old `f1r3node.git` repo. When it switches to `f1r3node-rust.git` (Phase 2 of migration), it needs to target `master` instead of `dev`.

**When Unblocked:** Coordinate with system-integration to ensure `services.yml` uses `branch: master`.

---

## Promoting Items to Active Work

When a backlog item is ready for active development:

1. Create an epic in `docs/ToDos.md` based on the backlog item
2. Create or link a user story in `docs/UserStories.md` if needed
3. Remove the item from this backlog (or mark as `promoted: true`)
4. Add a note referencing the original backlog ID

---

## References

- **Active Work:** `docs/ToDos.md`
- **User Stories:** `docs/UserStories.md`
- **Completed Work:** `docs/CompletedTasks.md`
