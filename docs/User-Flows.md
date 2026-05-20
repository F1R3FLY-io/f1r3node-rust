---
doc_type: user_flows
version: "1.0"
---

# User Flows

Detailed user interaction patterns for this project. Each flow captures the
journey, ordered steps, key interactions (which become integration test
assertions), and success metrics (which become test budgets). Flows are
linked bidirectionally to user stories (`docs/UserStories.md`) and to
epochs (`docs/ToDos.md`).

**Document Structure**
- User flows: this file (`docs/User-Flows.md`)
- User stories: `docs/UserStories.md`
- Implementation tracking: `docs/ToDos.md` (epochs + tasks)

**Linkage**
- A flow lists `Related Stories` and an `Implemented in` epoch
- Stories carry a `User Flow:` back-reference
- Epoch YAML carries a `user_flow:` field

---

## Personas

Add reusable persona definitions here (Name + one-line description) and
reference them by name from each flow's `Personas:` field.

---

## Core Workflows

<!-- Created flows are inserted above the "Planned Flows" section below. -->

---


### FLOW-001: cargo-geiger Supply-Chain Audit Integration & Usage

**Status:** In Progress
**Implemented in:** EPOCH-001
**Related Stories:** US-001
**Related Flows:** None
**Personas:** F1R3node Rust developer maintaining workspace dependencies, Security/CI reviewer auditing supply-chain regressions

**Journey:** Local install -> Optional pre-push gate -> PR submission -> CI baseline gate -> Intentional baseline refresh -> Nightly drift visibility

**Steps:**
1. **Local Install & Baseline View** - Developer runs cargo install --locked cargo-geiger and just geiger to see workspace unsafe surface; reviews committed .cargo-geiger.baseline.json to understand current posture
2. **Optional Pre-Push Gate** - Developer opts into local gating with RUN_GEIGER=1 git push; pre-push hook runs geiger alongside clippy/test and applies the same regression rule as CI
3. **PR Submission** - Developer pushes branch and opens PR; GitHub Actions geiger job runs in parallel with deny and test on every PR to dev/master/feature/**
4. **CI Baseline Gate Outcome** - Job compares current scan against committed baseline. Clean diff: green check + JSON artifact uploaded. Regression: red X with failure message pointing at just geiger-update
5. **Intentional Baseline Refresh** - For legitimate dep updates that add unsafe, developer runs just geiger-update, commits the refreshed baseline alongside the version bump in the same PR; reviewer audits the diff before approving
6. **Nightly Drift Monitoring** - Scheduled workflow scans master daily and uploads a dated JSON artifact without modifying the committed baseline, surfacing transitive ecosystem changes

**Key Interactions:**
- cargo install --locked cargo-geiger succeeds under the pinned nightly toolchain
- just geiger, just geiger-baseline, and just geiger-update recipes appear in just --list with single-line docstrings
- .cargo-geiger.baseline.json is tracked in git and reproducible from a clean checkout
- CI geiger job runs in parallel with cargo-deny and does not depend on lint
- CI geiger job fails when a PR introduces new unsafe relative to the committed baseline
- CI geiger job passes when the scan matches or improves upon the baseline
- CI failure message explicitly instructs the developer to run just geiger-update
- Default git push does NOT run geiger; only RUN_GEIGER=1 git push triggers the local scan
- Pre-push hook degrades gracefully with a SKIP message when cargo-geiger is not installed
- Nightly workflow uploads a dated artifact and never mutates .cargo-geiger.baseline.json

**Success Metrics:**
- Local just geiger completes in <300s on a warm Cargo cache
- CI geiger job adds <8min to PR wall-clock (runs in parallel with deny and test jobs)
- Baseline regression detection rate: 100% of test scenarios that introduce new unsafe are caught
- Mean time to detect upstream dep version bump that adds unsafe: <24h via nightly cron
- False-positive rate of CI gate on no-op PRs: 0%

---

## Planned Flows

(Empty)

---

## Related Documentation

- [User Stories](UserStories.md)
- [ToDos](ToDos.md)
