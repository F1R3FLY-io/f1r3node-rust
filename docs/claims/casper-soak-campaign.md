# Casper campaign planning controls

```yaml
claim_id: CLAIM-CASPER-CAMPAIGN-001
status: pending
artifacts:
  - scripts/casper-soak/campaign.sh
  - scripts/casper-soak/test-campaign.sh
phase: pre_pr216_merge
scope: campaign-planning-and-dispatch-admission
binding: pending
soak: pending
audited_by_claim_checker: false
audit_note: "The check-casper-claims auditor covers CLAIM-CASPER-SOAK-001 through 008 only. This claim is governed by its own review and by the ledger records for its two artifacts."
```

## Scope

This claim covers the draft campaign planner and its fixture runner:

- `scripts/casper-soak/campaign.sh`
- `scripts/casper-soak/test-campaign.sh`

The planner validates supplied records and calculates a proposed workload window. The dispatch command checks manual inputs and prior-run records without executing a campaign.

This claim is separate from the eight accepted Casper harness and profile claims. Their previous discharges do not cover these two helpers.

## Correctness requirements

### C1: Bounded single-object input

Each parsed input must contain exactly one JavaScript Object Notation (JSON) object. Empty input, arrays, malformed input, and multiple documents must fail.

Each input file must be readable, regular, and no larger than 1,048,576 bytes. A rejected input must not produce a successful plan or window.

### C2: Pinned input files

Each referenced path must resolve to a regular file within the supplied source root. Each declared SHA-256 digest must match that file.

The helper must reject path traversal, an escaping symbolic link, missing files, and digest differences. It must reject a source-inventory parser failure.

The source inventory must contain the six required control paths and between 6 and 512 entries. Both campaign helpers must be pinned.

This requirement does not establish inventory completeness.

### C3: Consistent planning records

The helper must require a first-attempt manual dispatch and the declared control revision. It must check the supported candidate fields and resource tuples.

The helper must require the declared workload and qualification identities to agree. Unknown required capabilities and fixture-labeled qualification records must fail.

The resulting plan must retain `requires-external-checks` admission. Supplied qualification declarations do not establish successful live qualification.

The planner supports the `current-dev-load` workload declaration. That legacy declaration does not satisfy the missing Casper-profile requirements.

### C4: Full workload window

The baseline workload must retain 86,400 seconds. The stability workload must retain 216,000 seconds. Cleanup must retain a separate 600-second reserve.

The helper must reject reversed clocks and insufficient remaining lifetime. It must not shorten the requested workload to fit the remaining lifetime.

The helper calculates limits but does not enforce process termination or cloud-instance termination.

### C5: Honest fixture results

The fixture runner must check the expected command exits and its explicit assertions. It must not report a successful suite after a failed check.

The fixture summary must retain `synthetic_fixture`, zero node launches, zero cloud launches, and pending claim discharge. Fixture results must not become live qualification.

### C6: Separate manual dispatch admission

Campaign input selects `campaign-preflight`, `campaign-baseline-24h`, or `campaign-stability-60h`. The selection must agree with the request stage.

The command must reject legacy scheduling, restart, retry, candidate-tag, skipped-preflight, canary, and injection inputs. It must reject reruns and self-referencing prior-run identifiers.

The command must retain a terminal report for each initialized output directory. Invalid input returns exit 2, while valid planning with unresolved execution prerequisites returns exit 3.

The dispatch command must never return successful campaign admission. It must not launch nodes or cloud instances, invoke replacement, or publish a passing campaign result.

### C7: Bound prior-run verification

Prior-run metadata must identify this repository, workflow, control revision, manual event, first attempt, successful conclusion, and completed status.

Exactly one unexpired result artifact must match the prior run. Its retained archive digest must match the GitHub artifact metadata.

The archive must contain exactly one regular result file. Its bounded JSON record must match the campaign identity and the expected stage and candidate.

The prior record must declare successful cleanup and host protection. A baseline record must declare at least 86,400 elapsed workload seconds and passing required profiles.

Failed, incomplete, mismatched, expired, malformed, or unavailable evidence must block advancement. The command must retain download failures and rejected evidence.

These checks authenticate the producer and compare record fields. They do not independently verify node observations, nested evidence, cloud termination, or approval authority.

## Assumptions and exclusions

The caller must provide a trusted source root and stable files during validation. The caller must bind the executing helper to the reviewed source.

The caller must authenticate approvals and qualification evidence. The planner does not verify their external origin or review authority.

The dispatch command trusts GitHub API responses obtained through the GitHub CLI. Local fixtures replace that transport and do not establish hosted qualification.

The caller must enforce launch reservations, total machine limits, retry limits, host protection, cleanup, and instance lifetime. The planner does not enforce these controls.

Clock inputs must use comparable epoch seconds. A calculated deadline does not prove that a workload ran for that duration.

The claim does not establish node correctness, cryptographic correctness, missing-interface availability, or a passing soak. It does not approve additional cloud resources.

## Evidence and remaining verification

The [Linux report](../casper/cbc-evidence/runs/casper-linux-admission-7509c831c-01/report.json) retains the parser regression and 34 native and isolated checks.

Those checks support regression review. They do not constitute formal discharge, a source-binding acceptance, or workflow qualification.

The [continuation report](../casper/cbc-evidence/runs/casper-campaign-dispatch-5e26ba4c5-01/report.json) records 115 native checks and 115 isolated checks. It retains the missing-test-pin regression and the first isolated timeout.

The expanded checks cover both baseline candidates and all three prior records for stability. Consistent fixture records still produce blocked dispatch, not execution approval.

Source-bound verification and explicit acceptance remain pending. No waiver applies.
