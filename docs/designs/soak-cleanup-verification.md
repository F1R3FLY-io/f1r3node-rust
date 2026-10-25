# Soak Cleanup Verification

## Objective

Prevent a scheduled soak from using cleanup code that lacks current verification and independent maintainer acceptance.
This design implements [US-014](../UserStories.md#us-014-verified-soak-segment-cleanup) and [EPIC-030](../ToDos.md#epic-030-verified-soak-segment-cleanup).

## Boundary

The control checkout supplies the driver, host controls, segment action, verification tools, and evidence.
The node candidate has a separate revision and cannot supply control approval.
The existing Casper harness claim remains separate from the focused cleanup claim.
The new claim states bounded safety under its process visibility and scheduling assumptions.
It does not establish node correctness or complete a live soak.

## Design

Nested Git attributes declare mandatory coverage, required acceptance, and the evidence directory.
Claim inventories and ledgers declare claim IDs and verification status.
Attributes do not duplicate mutable verification results.

The master-based integration includes the CbC migration from PR #700, merge commit dc285a9958d5ceed69a87cfe37ab86bb71f862b9.
Repository readers support named claims before cleanup records use that format.
Owner files retain existing policy and combine the cleanup requirements with the migrated evidence routes.

Repository checks use Bash.
The shared CbC runtime retains its upstream Python dependencies.
A pinned copy of the shared CbC implementation supplies claim inventories, evidence routing, and signature validation.
The copy retains its upstream source hashes.

The verification runner records test results against source hashes taken before execution.
A changed source invalidates the recorded result.
The report also binds the Rust harness sources, Cargo inputs, and toolchain configuration.

A bounded model separates child-process state from detached writer state and segment ownership.
A Linux fixture runs two segments and checks detached writer termination, unrelated process survival, and restart state.
Negative controls remove cleanup calls, ignore stop failures, and overwrite retained reasons.
Each control must fail at its expected assertion.

A scoped gate validates the cleanup claim and independent signed acceptance for every covered artifact.
Existing campaign and harness claims retain their records and independent gate status.
Cleanup admission does not discharge those other claims.

CI runs the gate without an optional strictness switch.
The soak workflow runs the same gate against ci-control before building or launching the workload.
The segment action repeats admission before each driver invocation.
A node candidate revision never substitutes for the control revision.

## Trust and acceptance

The operator supplies an allowed-signers file outside the worktree through local Git configuration.
Hosted jobs obtain its public contents from the operator-managed CBC_ACCEPTANCE_ALLOWED_SIGNERS variable.
No private key enters CI.
Agents do not enroll keys or sign production approvals.
Missing configuration and unsigned evidence fail closed.
Tests use disposable keys inside isolated fixture repositories only.

Source, claim, and evidence changes require new acceptance.
The implementation prepares unsigned payloads after verification.
The maintainer inspects and signs those exact payloads independently.
Git history retains earlier committed ledgers.
The review package excludes uncommitted ledger drafts.

## Verification

The acceptance report records the bounded model, Linux fixture, negative controls, and admission tests.
Gate tests cover stale control sources, distinct candidate sources, missing approval, invalid signatures, and changed evidence.
The workflow checks require admission before the driver runs.
The task remains in progress until independent acceptance is current.

## Upstream implementation

```yaml cross-reference
reference:
  purpose: evidence
  repository: https://gitlab.com/smart-assets.io/gitlab-profile
  checkout: ../../../../SA/top-level-gitlab-profile
  revision: 04a8badb8399ea7d54c477b153fa957161743072
  path: AItools/scripts/cbc.sh
  url: https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/04a8badb8399ea7d54c477b153fa957161743072/AItools/scripts/cbc.sh
```

## Operator procedure

1. Review the verification report and its source hashes.
2. Configure the external allowed-signers file through cbc.acceptanceAllowedSigners.
3. Export each cleanup approval with scripts/cbc.sh acceptance-payload and the cleanup claim ID.
4. Complete the maintainer identity, timestamp, and review reference in each payload.
5. Sign each completed payload independently in the cbc-acceptance namespace.
6. Import each approval with scripts/cbc.sh accept and the cleanup claim ID.
7. Set CBC_ACCEPTANCE_ALLOWED_SIGNERS to the same approved public signers in the repository configuration.
8. Run scripts/ci/check-soak-cleanup.sh before launch.

The agent prepares unsigned payloads only.
The shared skill requires independent maintainer acceptance.
Existing claims can remain pending even when the focused cleanup claim passes.
The full CbC discharge gate continues to report those independent gaps.

For example, import one independently completed and signed cleanup approval:

```bash
bash scripts/cbc.sh accept scripts/run-merge-recovery-soak.sh \
  --claim-id CLAIM-SOAK-CLEANUP-001 \
  --approval approval.json --signature approval.json.sig
bash scripts/ci/check-soak-cleanup.sh
```

The verification package contains the unsigned payloads for all covered artifacts.
Complete and sign each payload before import.
The scoped admission gate passes only when every cleanup approval is current.
