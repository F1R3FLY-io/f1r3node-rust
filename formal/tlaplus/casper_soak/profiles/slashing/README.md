# Slashing profile verification

This profile verifies controlled transcripts for [CLAIM-CASPER-SOAK-006](../../../../../docs/claims/casper-soak-slashing.md). It does not execute a node or authorize a slash.

## Binding

| Model property | Defect knob | Executable fixture |
| --- | --- | --- |
| EvidenceOrderRecorded | ReuseRequestedOrder | slashing_delivery_order |
| EpochCorrelationRequired | DropEpochIdentity | slashing_epoch_mismatch |
| AuthorizationMismatchReported | SuppressSlashMismatch | slashing_authorization_mismatch |

The model has two scenarios and three observations per scenario. Each scenario has two delivery receipts and one authorization snapshot.

The model permits both delivery orders, matching or unrelated snapshot epochs, and matching or different authorization outcomes. It assumes valid auxiliary fields and receipts.

The executable checks malformed evidence, input pins, missing observations, restart linkage, and independent failures outside those model assumptions.

Construction is not applicable under PR #433. The model does not prove node authorization, canonical reconstruction, bisimilarity, or unbounded liveness.

## Executable contract

The CLI accepts `identity`, `run`, and `models`. Each `run` requires a manifest, request, artifact directory, and new output directory.

Three pinned inputs define the configuration, fixture, and expected outcomes. Qualification records bind the candidate, provider, profile source, executable, and compatibility fields.

The request contains the parent pre-state digest, observation epoch, rebond epoch, evidence fixture, scenario family, and schedule. Its deadline must equal the manifest deadline.

Each evidence descriptor includes its digest, invalid block hash, offender, stake, evidence epoch, validator, sequence, origin, and pinned invalid-hash seed.

The fixture can record an invalid latest message or a merge-rejected slash. Neither label independently grants slash authority.

The classifier compares authorization, recovery outcome, offender deduplication, and the invalid-hash seed with the pinned expectation. It does not compute node RNG output.

The synthetic fixtures check comparison behavior. Their expected outcomes do not establish a reviewed node truth table or an actual node observation.

## Scheduling and correlation

Schedule actions include evidence delivery, pause, delayed delivery, rebond, and restart. Unique step identities and backward dependencies define the requested schedule.

Receipts must record applied actions, exact targets, and Boolean observation flags. Restart receipts require the predecessor, fresh successor, observed exit, exit code, and readiness.

Rebond receipts identify the offender and the old and new epochs. The new epoch cannot exceed the request observation epoch.

Delivery receipts remain separate from requests. Comparable receipts have one producer and clock, increasing sequence numbers, and nondecreasing timestamps.

A different delivery order produces incomplete requested coverage. The report preserves the actual order and any independent authorization failure.

The collector quarantines unrelated epochs, pre-states, candidates, and expired observations. It compares immutable event copies before quarantine and removes only transport record identities.

Snapshots must follow acknowledged steps and identify the final target incarnation. Missing or conflicting evidence inventories produce unknown counts, not zero.

Expected stale-evidence or forged-deploy rejection can pass a scenario. Malformed evidence has verdict precedence without deleting independent product failures.

## Limits

The executable permits 16 evidence descriptors, 24 schedule steps, 32 snapshot entries, and 64 transport records. These bounds differ from the finite model bounds.

Only synthetic protocol-6 unary evidence with the baseline rejected-slash rule is qualified. Live, post-merge, alternate-encoding, alternate-rule, and economic-neglect requests remain blocked.

Workflow execution, human binding acceptance, workflow-tag ratification, and source-bound claim discharge remain separate requirements.

## Commands

Run the checks with a new output directory:

```bash
TLA_TOOLS_JAR=/path/to/tla2tools.jar \
  bash scripts/casper-soak/check-slashing.sh target/slashing-check
```

The runner verifies the pinned TLC JAR, exact fixture inventory, executable identity, model controls, and unchanged source digests.

The clean model requires exit 0 and a completed search. Each negative control requires exit 12 and its exact named invariant.
