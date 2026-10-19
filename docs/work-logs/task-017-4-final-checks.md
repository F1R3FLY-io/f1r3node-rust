# TASK-017-4 Final Harness Checks

## Scope

The user selected TASK-017-4, not all of EPIC-017. This work changes only the harness, fixtures, workflow checks, and related records.

New executable code uses Rust and Bash. No node campaign, external repin, waiver, commit, or push occurred.

## Changes

The full binding runner now uses test optimization level 1. The runner records this setting and preserves its resource limits and assertions.

The registry requires 48 case identities and 91 driver invocations. Aggregate counts cannot replace a missing case.

A new fixture checks interrupted capture before container removal. It uses a controlled Docker command boundary and verifies the retained bytes and operation order.

This fixture does not establish Docker daemon behavior or native process containment. The existing HUP fixture separately checks report classification.

The claim auditor checks all eight claim identities. A declared discharge requires matching artifact, specification, evidence, claim, and phase records.

A RED test exposed reuse of old evidence after a ledger digest changed. The GREEN audit checks the evidence report's own digests and tiers.

Discharge reports require `source_digests`, `claim_digests`, `claim_ids`, `phase`, and `tiers`. Ledger records also require a commit identity and verification timestamp.

Additional audit fixtures check canonical ledger names, blank-separated artifact lists, and symlinked ledger directories.

The auditor does not execute a prover. Its ordinary mode can report pending claims successfully. Strict mode rejects pending claims with exit 4.

The `--claim` option selects one claim for a strict gate. Node soak status remains separate from harness verification.

The workflow runs claim-audit tests and retains the audit report. A manual `require_casper_discharge` input activates the strict bundle gate.

## Terminal transitions

The concurrent-transition fixture reproduced a workload launch while another actor held the terminal-transition lock. The RED test exited 101.

The runtime now acquires `.casper-transition-lock` before it checks the terminal marker. It releases the lock after it starts the workload supervisor.

The `stop` action uses the same lock and durably publishes the terminal marker. Lock acquisition has a two-second limit.

The GREEN fixture prevents launch when the terminal transition wins. Another fixture checks `stop` between Bash admission and Rust execution.

An active-stop fixture reproduced an unnecessary second iteration after cancellation. Its RED test exited 101.

The Bash loop now drains an admitted iteration and stops before another iteration. The GREEN result preserves the completed observation and reports cancellation separately.

Use the `stop` action for concurrent terminal requests. Do not replace the lock file or write internal markers concurrently outside this protocol.

```bash
bash scripts/bench/casper-soak.sh stop --output "$SOAK_OUTPUT_DIR"
```

This action prevents further workload admission. It does not replace emergency writer termination.

The lock does not solve simultaneous driver and crash-monitor death. Existing containment assumptions and B44 remain explicit.

## Binding review

These mappings describe the bounded lifecycle fixtures. They do not establish node correctness, profile implementation, or unbounded liveness.

| Obligation | Implementation and executable evidence | Boundary |
| --- | --- | --- |
| H01: Identity | Manifest binding, admission, invalid inputs, and exact-byte resume mutations. | Approved inputs and executable identities remain separate requirements. |
| H02: History | Hash-linked entries, rollback refusal, and failure-counter refusal. | Capture inventories must remain intact. |
| H03: Failures | Resource-stop and cached-success fixtures preserve observed failures. | Resource termination is not a product verdict. |
| H04: Evidence | Missing artifacts, incomplete capture, and publication replay prevent false success. | Synthetic fixtures do not become node observations. |
| H05: Stop | Entry guard, shared transition lock, and active-stop drain fixtures. | Cooperating terminal writers use the lock protocol. |
| H06: Capture | Durable capture and replay checks, plus interrupted-run capture ordering. | Storage and supervisor assumptions still apply. |
| H07: Policy | Configuration, policy, and executable mismatches reject admission. | No fixture authorizes a production policy change. |
| H08: Missing data | Missing-zero, Boolean-counter, missing-artifact, and duplicate fixtures. | Missing data remains distinct from observed zero. |
| H09: Merge | Open-merge refusal and the bounded post-merge model gate. | No actual merged-node campaign occurred. |
| H10: Verdicts | Exact TLC classification, source checks, inventory checks, and interrupted-report refusal. | Inventory and receipt validation are not proof execution. |

## Completion interface

The repository adapter supports TASK-017-4 without changing the installed shared helper. It pins the helper source and calls its existing strict completion function.

The adapter exposes no force or waiver option. Both modes require strict discharge of CLAIM-CASPER-SOAK-001.

The shared structural check alone reported a full chain despite pending claims. The adapter adds the missing claim gap and prevents a completion stamp.

```bash
bash scripts/casper-soak/task-complete.sh TASK-017-4 --check \
  --helper "$SA_TASK_COMPLETE_HELPER"
```

US-006 and FLOW-001 now describe the maintainer verification flow. TASK-017-4 lists its tests and changed mandatory files.

The compatibility check uses `BF_TODO_PARSER_STRICT=0` for existing unrelated `review` states. This setting does not disable the completion integrity gate.

## Verification results

| Check | Result |
| --- | --- |
| Isolated Rust suite | All 22 tests passed. |
| Driver inventory | All 91 invocations across 48 cases matched. |
| Casper TLC controls | One clean configuration and ten negative controls passed their expected checks. |
| Shared TLC gate | All 14 positive configurations and 71 negative controls passed their expected checks. |
| Legacy and disk suites | The legacy suite and all 42 disk scenarios passed. |
| Active language-server checks | Eleven files had no reported errors. |
| Strict claim and CbC gates | Both exited 4 with pending discharge. |
| Completion attempt | The adapter exited 4 and left the tracker unchanged. |

## Evidence and limits

The source-bound package is [casper-task-017-4-checks-20260918-01](../casper/cbc-evidence/runs/casper-task-017-4-checks-20260918-01/report.json).

Earlier successful 89-invocation runs remain historical. The package retains both terminal RED/GREEN cycles and all later verification attempts.

One model setup attempt copied an unrelated build directory and failed on file permissions. A second attempt lacked a workflow input for shared-gate fixtures.

Neither setup failure counts as a passing suite. Their logs remain separate from the successful final model and legacy run.

Claim acceptance remains pending. No inventory result, synthetic transcript, or audit result discharges the complete claim by itself.

The seven profiles, candidate qualification, approved node resources, and node soaks remain separate tasks. Human review and required ratification remain open.
