# Casper campaign execution controls

```yaml
claim_id: CLAIM-CASPER-CAMPAIGN-003
status: pending
scope: campaign-controller-and-oci-supervisor
binding: pending
refutation: pending
construction: not-applicable
soak: pending
artifacts:
  - scripts/casper-soak/src/campaign_control/mod.rs
  - scripts/casper-soak/src/campaign_control/transport.rs
  - scripts/casper-soak/src/campaign_control/operations.rs
  - scripts/casper-soak/src/campaign_control/results.rs
  - scripts/casper-soak/src/campaign_control/models.rs
  - scripts/casper-soak/src/bin/casper-campaign-control.rs
  - scripts/casper-soak/src/bin/casper-campaign-supervisor.rs
  - scripts/casper-soak/src/bin/casper-campaign-models.rs
  - scripts/casper-soak/tests/campaign_control.rs
  - scripts/casper-soak/tests/campaign_models.rs
  - scripts/casper-soak/campaign-control.sh
  - scripts/casper-soak/campaign-bootstrap.sh
  - scripts/casper-soak/campaign-host.sh
  - scripts/casper-soak/campaign-host-guard.sh
  - scripts/casper-soak/campaign-job.sh
  - scripts/casper-soak/campaign-finish.sh
  - scripts/casper-soak/check-campaign-control.sh
  - scripts/casper-soak/test-campaign-control.sh
  - scripts/casper-soak/supervisor/Dockerfile
  - scripts/casper-soak/supervisor/start.sh
  - scripts/casper-soak/supervisor/func.yaml
  - .github/workflows/casper-campaign-control.yml
  - .github/workflows/merge-recovery-soak.yml
  - formal/tlaplus/casper_soak/campaign/CampaignControl.tla
  - formal/tlaplus/casper_soak/campaign/MC_CampaignControl.cfg
  - formal/tlaplus/casper_soak/campaign/MC_CampaignControl_approval_unsafe.cfg
  - formal/tlaplus/casper_soak/campaign/MC_CampaignControl_reservation_unsafe.cfg
  - formal/tlaplus/casper_soak/campaign/MC_CampaignControl_schedule_unsafe.cfg
  - formal/tlaplus/casper_soak/campaign/MC_CampaignControl_termination_unsafe.cfg
  - formal/tlaplus/casper_soak/campaign/verification-plan.jsonc
  - formal/tlaplus/casper_soak/campaign/README.md
```

## Requirements

The controller must authenticate the exact request digest, control revision, configuration digest, workflow run, and first attempt through GitHub approval history.

The approving account must have the current `maintain` or `admin` repository role. Ordinary write access is insufficient. Self-review and administrator bypass must be disabled.

One trusted OCI Object Storage record must contain the complete three-slot budget. Every write must compare the current entity tag. Missing state must block execution.

A consumed slot must remain consumed after a crash, ambiguous write, failed launch, or lost response. No operation may reset, refund, replace, or repeat a launch.

The controller must persist submission intent before its single Compute launch call. Transport retries must be disabled. Recovery must observe the existing instance without another launch.

The controller must bind the boot image, container image, executable, architecture, memory, workload, and exclusive runner identity before workload admission.

The supervisor must read deadlines from trusted storage. Individual deadline schedules must invoke the pinned OCI Function before launch is permitted.

The supervisor must use Compute termination and then observe `TERMINATED`. A stop state, accepted request, missing instance, or failed lookup must not establish termination.

Scheduling, invocation, clock, API, and termination allowances must fit within the approved instance lifetime. Unsupported timing evidence must block execution.

The controller must preserve product failures when infrastructure or cleanup subsequently fails. Public evidence must not contain credentials or launch metadata.

The finalizer must parse the result from the authenticated archive. A separately supplied JSON record must not replace the archived result.

Late or failed workload evidence must retain authenticated product failures. Missing worker evidence must request cleanup and produce a non-passing result.

## Verification gate

The [campaign gate](../../scripts/casper-soak/check-campaign-control.sh) executes controller fixtures, local reservation tests, planner regressions, and all five model configurations.

The [verification plan](../../formal/tlaplus/casper_soak/campaign/verification-plan.jsonc) binds each negative configuration to one required invariant. Unexpected exits, missing traces, and incomplete positive searches fail.

The [hosted workflow](../../.github/workflows/casper-campaign-control.yml) runs the gate without cloud credentials. Its output remains verification evidence, not an acceptance decision.

The execution workflow requires the configured approval environment, trusted configuration digest, accepted source evidence, and qualified workloads. An unset configuration digest retains the admission-only route.

## Verification boundaries

Controlled provider fixtures can establish state transitions and request construction. They cannot establish live provider availability, scheduling bounds, deployed permissions, or node qualification.

Deployment requires verified configuration and source identities. Live campaign execution retains its workload, adapter, resource, and acceptance requirements.

Formal verification and source-bound acceptance remain pending until evidence identifies the exact implementation and its assumptions. No waiver applies.
