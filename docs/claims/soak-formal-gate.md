# Claim: Soak Formal Gate Evidence

```yaml
claim_id: CLAIM-SOAK-GATE-001
status: pending
artifacts:
  - scripts/ci/check-tla-invariants.sh
  - scripts/ci/test-check-tla-invariants.sh
  - .github/workflows/slashing-tests.yml
  - .github/workflows/ci.yml
references:
  - docs/plans/soak-recurrence-prevention-2026-09-08.md
  - docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md
```

## Required behavior

The formal gate must not report success when a required configuration is absent, skipped, incomplete, or inconsistent with its expected result.

A positive configuration must complete without a verifier error. A negative control must produce its specified counterexample: TLC exit 12 and the exact `Error: Invariant <name> is violated.` line. A clean control, a different invariant, a tool error, a timeout, or a missing configuration fails the gate.

The `NEGATIVE_CONTROLS` array in `scripts/ci/check-tla-invariants.sh` is the single registry of controls. In the areas listed in `REGISTERED_CONTROL_AREAS`, a pre-fix configuration beside a registered positive that is absent from the registry fails the gate. `scripts/ci/test-check-tla-invariants.sh` reads that array, checks every control against seven outcomes, and checks that the workflow routes pull requests and pushes to the bounded `--soak-pr` tier and scheduled or manual runs to the full list.

## JSONC file migration

The claim inventory and claim-evidence files use JSON with Comments (JSONC). The validator accepts line and block comments through Ruby's existing JSON parser.

The [format regression](../../scripts/ci/test-soak-claim-inventory-jsonc.sh) checks the default filename, explicit paths, comments, malformed input, stale bindings, missing claims, and false discharge.

The CI inventory step runs this regression. Renamed evidence files retain their original bytes. Historical source and raw-artifact paths remain unchanged inside those records.

Documentation references use the new filenames. This migration does not rerun historical verifiers or discharge a claim.

## Current evidence

See the [gate evidence record](../cbc-evidence/scripts-ci-check-tla-invariants-sh.md).

## Pending obligations

- `TLA+ invariant check` is not a required status check on `dev`. A maintainer must add it and confirm that skipped, canceled, and stale results cannot satisfy it.
- Hosted runs do not independently attest the workflow-control SHA.
- The Rocq job in the slashing workflow does not build `finalized_floor`. `scripts/check-finalized-floor-ALL.sh` verifies `CLAIM-FINALITY-002` outside CI.
