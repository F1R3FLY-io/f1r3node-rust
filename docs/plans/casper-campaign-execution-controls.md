# Casper Campaign Execution Controls

**Status:** The user authorized implementation of the execution controls. Approval authority and infrastructure settings require confirmation before integration.

**Task:** TASK-017-12 on `formal/soak-casper-consensus`.

**Reviewed source:** `db4a52fb64ef8e7dbba2469d7c72c115593901e2`.

## Scope

This work covers authenticated approvals, authoritative reservations, immutable image selection, lifetime enforcement, host protection, cleanup, and failure evidence.

The existing local reservation guard remains a tested component, not global launch enforcement. The admission-only workflow still launches nothing.

Node interfaces, live qualification, image publication, candidate repinning, formal acceptance, deployment changes, and actual campaign launches retain separate gates.

## Source and configuration findings

| Reviewed surface | Finding | Consequence |
| --- | --- | --- |
| GitHub `oci-credentials` environment | The API reports a branch rule, no required reviewers, and permitted administrator bypass. | Environment access alone cannot establish campaign approval. |
| Campaign admission job | The job has read-only permissions and no OCI environment. | It currently cannot reserve or launch a cloud instance. |
| Pinned external `launch-runner.sh` | The launcher retries selected errors up to three times, including timeouts. | One wrapper invocation does not establish one launch submission. |
| Pinned external launch request | The inspected request does not attach campaign identity or deadline tags at creation. | A lost response can leave an instance without campaign identity for independent cleanup. |
| Pinned bootstrap | The bootstrap performs self-termination after its runner exits. | This path is not independent of the instance or bootstrap process. |
| `.github/workflows/ci-runner-reaper.yml` | The workflow runs every 30 minutes and accepts a mutable deadline tag. | It does not establish the approved maximum lifetime. |
| Installed OCI Resource Scheduler CLI | The listed actions are start, stop, and backup. | A stop schedule must not be represented as instance termination. |
| Installed OCI Object Storage CLI | Object upload supports `--if-match`. | A pre-provisioned campaign object can support conditional updates, subject to API verification. |

The external source revision is `b3d14b27e3c6276b1eb4ab9ccef04e02b0c4e283`. Fresh downloads match both earlier retained source copies.

The CLI review is local help inspection, not a live service test. The environment query does not establish the full repository authorization policy.

## Decisions required

### Approval authority

The recommended design uses a dedicated campaign environment with named required reviewers. Campaign approval must bind the exact request, control revision, identities, and resource budget.

Required settings include the reviewer identities, self-review policy, administrator-bypass policy, and allowed deployment references. The implementation must reject missing or unsupported approval evidence.

A workflow actor, repository write access, a branch rule, or `status: approved` in a supplied document is insufficient.

The environment does not yet exist as a verified campaign approval mechanism. This plan does not authorize creation or modification of GitHub environments.

### Authoritative reservation store

The recommended backend is one pre-provisioned OCI Object Storage object for the approved campaign budget. Its location must come from trusted controller configuration.

The record contains the campaign binding, all three slots, all used run identifiers, and each slot state. One conditional update checks the complete record.

Separate objects for separate slots cannot independently enforce cross-slot run uniqueness. The controller must not use a local filesystem lock as distributed serialization.

The operator must identify the region, namespace, bucket, object, and permitted principals. Missing state must block execution rather than create a replacement budget.

Deletion, rollback, replacement, and alternate object selection require explicit controls. Object versioning or retention alone does not establish these controls.

The existing local reservation command remains useful for local fixtures. It must not automatically become a fallback when the authoritative service is unavailable.

### Independent lifetime enforcement

A supervisor outside the campaign controller and workload instance must own termination. The operator must identify its deployment, permissions, scheduling bounds, and failure response.

An existing service can satisfy this role only after source and operational verification. No additional supervisor machine is included in the approved three-machine budget.

A GitHub job timeout, runner-local timer, tag, or termination request does not prove that an instance terminated.

The supervisor must accept a durable campaign intent before launch. Creation-time identity must let it find an instance after the launch response is lost.

Its enforcement allowance must fit inside the approved lifetime. Scheduling delay, clock uncertainty, and provider API latency must remain explicit assumptions.

Missing supervisor evidence or unsupported timing guarantees must block launch. A termination failure must remain an infrastructure failure, not successful cleanup.

## Execution contract

1. Validate the exact manual request and trusted control source.
2. Authenticate approval against the configured authority and exact campaign binding.
3. Verify workload, candidate, profile, source, image, and prior-result evidence.
4. Verify authoritative-store access and the independent supervisor contract.
5. Consume one slot through a conditional update of the complete campaign record.
6. Register the instance identity and lifetime intent with the independent supervisor.
7. Persist the submission state before the sole launch submission.
8. Submit the exact launch request with transport retries disabled.
9. Reconcile an uncertain response through bounded observation, without another launch submission.
10. Verify instance identity, platform, memory, image, supervisor coverage, and exclusive runner assignment.
11. Verify host protection and the complete remaining workload window before workload admission.
12. Retain workload results, termination observations, cleanup evidence, and all failed attempts.

A crash between the persistent submission state and the API call can consume a slot without creating an instance. Safety takes priority over retry availability.

A changed run identifier, campaign identifier, approval document, or controller restart must not create a fresh budget.

Any ambiguous conditional update requires reconciliation. A missing acknowledgment must not release a slot or authorize another submission.

Cleanup requests may retry within explicit bounds against the same verified instance. This does not permit repeat launch requests or replacement instances.

## Fixed baseline resources

| Stage | Instances | Memory per instance | Maximum lifetime | Workload duration |
| --- | --- | --- | --- | --- |
| Preflight | One | 64 GB | Four hours | Separate integration preflight |
| Baseline | One per selected architecture | 64 GB | 26 hours | 86,400 seconds each |

Both baselines require passing preflight evidence. A reservation alone cannot satisfy that gate.

The 600-second cleanup reserve remains separate from workload duration. Insufficient lifetime rejects execution rather than shortening a baseline.

The later 60-hour phase remains disabled until its candidate count and runner lifetime receive explicit confirmation.

## Image and host controls

The controller must use exact platform image digests and verify the selected node executable identity. It must not enter the legacy local-rebuild path.

The OCI boot image and node container image are different identities. Both must be pinned and checked for the selected architecture.

The runner requires an exclusive label tied to its reservation. Relabeling a shared runner after registration is insufficient.

The workload must retain the existing memory and disk protection thresholds. Missing protection, incomplete containment, and failed cleanup cannot become passing outcomes.

Open containment obligations remain open until the actual execution path satisfies them. This plan does not treat a configured host guardian as proven containment.

Credentials must remain outside workload-visible files, logs, and retained public evidence. The controller must not print runner registration tokens or private keys.

## Proposed implementation files

The final source list depends on the confirmed approval and supervisor mechanisms.

| File | Intended responsibility |
| --- | --- |
| `scripts/casper-soak/src/bin/casper-campaign-control.rs` | Validate records, bindings, resource limits, and state transitions. |
| `scripts/casper-soak/tests/campaign_control.rs` | Test state transitions, malformed evidence, identity drift, and ambiguous outcomes. |
| `scripts/casper-soak/campaign-control.sh` | Use bounded authenticated API transports and conditional state updates. |
| `scripts/casper-soak/test-campaign-control.sh` | Test competing controllers, transport failures, retries, and retained evidence with isolated fixtures. |
| `scripts/casper-soak/campaign.sh` | Connect qualified execution controls without weakening existing admission guards. |
| `scripts/casper-soak/test-campaign.sh` | Preserve admission, duration, prior-evidence, and legacy-separation regressions. |
| `.github/workflows/merge-recovery-soak.yml` | Connect approved campaign jobs while keeping legacy execution separate. |
| `docs/claims/casper-campaign-execution.md` | Register pending execution-control requirements before executable changes. |

All listed executable paths already have mandatory attributes. Changed accepted artifacts require renewed source-bound verification and explicit acceptance.

Supervisor source and deployment files require an exact inventory after the operator selects the service. The implementation must not silently modify external repositories.

## Acceptance tests

- Reject missing reviewers, unauthorized reviewers, wrong requests, and mismatched control revisions.
- Reject alternate stores, replaced bindings, replayed run identifiers, and invalid resource quantities.
- Exercise competing conditional updates and retain one consumed slot per successful reservation.
- Lose each API response and kill the controller at every persistent state boundary.
- Confirm that uncertain submissions never produce a second launch request.
- Reject unarmed, stale, mismatched, or unavailable supervisor evidence.
- Exercise controller loss and runner loss without relying on either process for termination.
- Reject wrong image, architecture, executable, memory, runner label, and creation-time identity.
- Reject insufficient lifetime without reducing the full baseline workload duration.
- Preserve prior product failures when infrastructure or cleanup subsequently fails.
- Preserve all 115 campaign checks and the local reservation regressions.

Fixtures establish controlled behavior only. They do not establish deployed service availability, live approval, actual node qualification, or successful cloud termination.

## Current boundary

The review made no executable changes and no remote configuration changes. No node, cloud instance, campaign workflow, or infrastructure deployment started.

Implementation can proceed after the approval authority and external service choices are explicit. Campaign execution remains blocked throughout preparation and verification.
