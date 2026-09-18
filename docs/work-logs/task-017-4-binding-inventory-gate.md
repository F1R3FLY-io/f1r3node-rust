---
task: TASK-017-4
claimed_by: pi-casper-harness
handoff_status: in_progress
construction: not-applicable
---

# Binding Inventory Gate

The user authorized continued implementation. Another agent owns commits and pushes.

The starting revision is `774ce338692400ff86f60092fad0c9c73923c80e`.

## Finding

The starting binding runner checked 88 invocation records and matching exit fields. It did not require the expected case identities.

An unrelated case can replace a required case without changing the total. Aggregate counts cannot establish complete binding coverage.

## Plan

1. Expose the existing evidence check without container execution.
2. Reproduce acceptance of an archive with a renamed required case.
3. Require the exact case inventory and expected exit sequences through a Rust checker.
4. Run the production binding suite and retain the results.

This gate checks inventory completeness, not full claim discharge. Semantic binding review and the remaining task gates stay pending.

## Inventory Result

RED reproduced acceptance of a renamed required case. GREEN requires exact case identities, invocation names, commands, exit sequences, and suite totals.

The first isolated full run passed 14 tests and 88 invocations. Those results apply to the inventory-only implementation before the subsequent fixes.

The current inventory contains 46 cases and 89 invocations. Both epic inventories contain 36 artifacts.

The validator unit test uses explicitly synthetic inventory inputs. The production driver suite records actual Bash invocations against synthetic executors.

## Terminal Boundary

The Bash admission check could precede a terminal marker. The Rust execution entry point did not check that marker before executor launch.

A controlled launcher writes the marker between those two boundaries. RED retained an executor launch and returned zero.

GREEN checks the marker after Rust admission. The focused driver test confirms that no launch record or executor sample appears.

This check does not establish atomic exclusion against every later concurrent stop. Supervisor-death and stronger containment obligations remain open.

## Interrupted Reports

A full rerun exceeded the outer tool limit. The container remained active, so its partial evidence was retained before explicit termination.

A retry exceeded another outer tool limit. Its EXIT handler incorrectly wrote a passing report without captured evidence.

The retry report is invalid evidence, not a successful run. The original report remains in the failure history.

A signal-injection test reproduced this reporting defect. GREEN requires explicit completion and handles HUP, INT, and TERM as failures.

The wrapper now attempts interrupted capture before cleanup. It bounds the container internally and captures source hashes before compilation.

## Current Verification

The current host tests pass. Three focused Linux tests pass: inventory substitution, interrupted reporting, and terminal-before-execution handling.

A fresh TLC run passes the clean model and all ten negative controls. The complete current Linux fixture suite remains unverified after the interrupted attempts.

The interruption test covers report classification. Complete interrupted-container capture still needs a dedicated fixture.

The [evidence report](../casper/cbc-evidence/runs/casper-binding-gates-20260918-01/report.json) separates the successful checks, interrupted attempts, and invalid report.

TASK-017-4 remains open. No node campaign, claim waiver, task closure, commit, or push was performed by this agent.
