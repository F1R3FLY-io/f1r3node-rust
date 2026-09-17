---
task: TASK-017-2
branch: formal/soak-casper-consensus
claimed_by: pi-casper-harness
started_at: 2026-09-17T00:46:59Z
handoff_status: blocked
contract_status: complete
tracking_status: completion-helper-incompatible
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

## Retrieved source digests

These SHA-256 values identify retrieved bytes at the selected external revision. Paths are relative to that repository, not the local sibling checkout.

| External path | SHA-256 |
| --- | --- |
| `integration-tests/test/infra/node.py` | `26ee175079b5fc2b064de3bdb4c9d0402860ab2388038d380917356b4e89a9bc` |
| `integration-tests/test/infra/providers/base.py` | `50ee05e50befc4e897fa9cdfea61716d5013b8777a556ee6c9511de000844352` |
| `integration-tests/test/infra/providers/docker.py` | `29906538de5921829e50c1911523d0a6e47fa02efc5e3db35c3ae1502251e3a8` |
| `integration-tests/test/infra/providers/subprocess.py` | `9149e1e272f1504809307cc52c91cfdf88797d38ab3d9d4877bae1a3c52535da` |
| `integration-tests/test/tests/custom/test_load.py` | `aa06c14d0268f36c58eb3102ae2d2224b1f968f335631b8161d43d4d4999c19b` |
| `integration-tests/test/conftest.py` | `f76a78baeee1ebfda24c9bdc745602a137e8de84221183e55588542c4fc5ef82` |

## Acceptance map

| TASK-017-2 criterion | Contract evidence |
| --- | --- |
| Inputs, outputs, assumptions, bounds, controls, fixtures | Common records and fixture matrix, plus all eight claim specifications |
| Ratified expectations without conflicting node claims | Profile-specific expectations and branch-plan authority links |
| Harness-only mandatory source scope | Unchanged twenty-one-artifact inventory and proposed harness modules only |
| No node-proof obligation | Explicit exclusions in common contract and every claim |
| Missing interfaces block scenarios. Mocks are not product evidence. | Capability admission, fault receipts, `evidence_kind`, and per-profile qualification limits |
| Post-merge adaptation for every profile | Per-profile field mappings and TASK-018 owner references |

## Completion-tool limitation

The shared `task-complete/scripts/task-complete.sh TASK-017-2` invocation returned exit 2: `target must start with TODO- or EPIC-`.

The canonical framework copy has the same dispatch limitation. No repository-local completion wrapper exists.

The contract work is complete, but task status remains `in_progress`. The task record identifies the helper limitation instead of silently bypassing the wrapper.

TASK-017-1 remains unchanged. The user authorized this specification increment directly, not completion of its broader prerequisite review.

## Evidence impact

The model, configurations, runner, retained TLC results, and staged ledger records are unchanged. No verification timestamp, discharge, or waiver was added.

Earlier pending ledger claim digests describe the earlier specifications. They cannot establish current binding evidence for these expanded contracts.

TASK-017-4 and the profile owners must refresh claim-bound evidence after implementation. This task does not relabel previous results as verification of new requirements.

## Verification

Contract checks passed for eight claim specifications, ten harness fixture IDs, twenty-one profile negative fixture IDs, and twenty-one positive/generation fixture obligations.

YAML parsing, source digests, documentation links, source scope, unrelated task preservation, and index preservation checks passed.

All twelve existing runner unit tests passed. They are regression checks for the existing runner, not executable verification of the new profile contracts.

The deterministic STE Check and `git diff --check` passed. Human STE Review remains necessary.

Strict EPIC-017 CbC discharge returned exit 4. All twenty-one artifact records remain pending, and no node or soak test ran.

## Handoff

The user authorized continuation to TASK-017-3 after the contract work. Prerequisite review can proceed without treating tracker closure as resolved.

The next task must retain separate Git authorization for integration and repinning. The completion helper needs a separate compatibility repair.
