---
task: TASK-017-2
branch: formal/soak-casper-consensus
claimed_by: pi-casper-harness
started_at: 2026-09-17T00:46:59Z
handoff_status: in_progress
---

# TASK-017-2 Interface Contract

## Scope

The user requested completion of the harness/profile specification task. No runtime implementation, shared CI edit, external repin, or Git write is authorized.

The starting checkout still had seventy staged files at `cc7e84b482887f0647277ccbacd6f65ae3cf749d`. This task preserves that index and adds only working-tree documentation changes.

## Deliverables

The [version-1 interface contract](../casper/design/soak-interface-contract.md) specifies manifests, requests, fault acknowledgments, observations, artifacts, results, and classification rules.

The [claim index](../claims/casper-soak-harness.md) links seven expanded profile contracts. Each profile specifies payload fields, positive fixtures, three negative controls, blockers, and post-merge adaptations.

The common contract names existing driver boundaries and proposed profile module interfaces. Proposed modules remain unimplemented and do not expand scope into node code.

## Findings

The selected external harness pin is `b3d14b27e3c6276b1eb4ab9ccef04e02b0c4e283` in `F1R3FLY-io/system-integration`.

The local sibling checkout was at `962effd17708192627bd249362761c0ccb1fd5fa` and lacked the selected commit object.

The audit therefore read selected-revision files through the GitHub API. It did not fetch into, modify, or repin the sibling checkout.

The pinned load test is a stress workload with explicit heartbeat overrides. It is not automatically a production-policy baseline.

Deploy/query and pause/restart primitives exist. Complete profile event identities, publication cut points, settlement extraction, ordered delivery, and carrier-path receipts still require qualification.

Adopted subprocess handles explicitly reject restart. Generic pause methods do not supply structured observed-state acknowledgments.

The existing driver uses fail-soft metric extraction and process-exit-derived `.ok`. Neither proves required profile coverage.

## Acceptance map

| TASK-017-2 criterion | Contract evidence |
| --- | --- |
| Inputs, outputs, assumptions, bounds, controls, fixtures | Common records and fixture matrix, plus all eight claim specifications |
| Ratified expectations without conflicting node claims | Profile-specific expectations and branch-plan authority links |
| Harness-only mandatory source scope | Unchanged twenty-one-artifact inventory and proposed harness modules only |
| No node-proof obligation | Explicit exclusions in common contract and every claim |
| Missing interfaces block scenarios; mocks are not product evidence | Capability admission, fault receipts, `evidence_kind`, and per-profile qualification limits |
| Post-merge adaptation for every profile | Per-profile field mappings and TASK-018 owner references |

## Verification

Verification results and completion-tool behavior will be recorded after the final checks. Model, runner, profile, and soak evidence remain distinct.
