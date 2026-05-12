---
doc_type: backlog
version: "1.0"
last_updated: 2026-04-15
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

<!-- Items that improve code quality, performance, or maintainability -->

---

### Feature Ideas

<!-- Future features that have been identified but aren't yet prioritized -->

#### BACKLOG-FI-002: Genericize testbed scripts for AWS / GCP

```yaml
---
backlog_id: BACKLOG-FI-002
title: "Abstract provisioning layer so testbed can run on AWS or GCP, not just OCI"
category: feature_idea
priority: p3
added_at: 2026-04-13
related_epoch: EPOCH-009
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

**Related work:** EPOCH-009 establishes the OCI implementation that this would generalize. `vps-*` Justfile prefix is already chosen to outlive OCI-only.

---

#### BACKLOG-FI-001: Inter-Shard Consensus (Option B)

```yaml
---
backlog_id: BACKLOG-FI-001
title: "Inter-shard consensus (cross-shard bridge between two independent shards)"
category: feature_idea
priority: p3
added_at: 2026-04-13
related_epoch: EPOCH-009
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

**When Unblocked:** Requires design doc + architectural review. Not ready for promotion to active epoch until the hierarchical-shard model is fully specified and the bridge protocol has a reviewed spec.

**Related work:** EPOCH-009 stands up a **single-shard** distributed testbed on OCI. If BACKLOG-FI-001 is promoted, the testbed from EPOCH-009 would extend naturally to a 4-VPS multi-shard topology.

---

### Research & Exploration

<!-- Items that need investigation before they can become actionable -->

---

### Dependencies & Blockers

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

1. Create an epoch in `docs/ToDos.md` based on the backlog item
2. Create or link a user story in `docs/UserStories.md` if needed
3. Remove the item from this backlog (or mark as `promoted: true`)
4. Add a note referencing the original backlog ID

---

## References

- **Active Work:** `docs/ToDos.md`
- **User Stories:** `docs/UserStories.md`
- **Completed Work:** `docs/CompletedTasks.md`
