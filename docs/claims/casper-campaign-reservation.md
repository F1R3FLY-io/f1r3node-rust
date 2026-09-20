# Casper campaign reservation guard

```yaml
claim_id: CLAIM-CASPER-CAMPAIGN-002
status: pending
scope: local-baseline-reservation-store
artifacts:
  - scripts/casper-soak/src/bin/casper-campaign-reservation.rs
  - scripts/casper-soak/tests/campaign_reservation.rs
binding: pending
refutation: pending
construction: not-applicable
soak: pending
```

## Scope and implementation plan

This claim covers a local reservation store for the approved three-machine baseline budget. It does not cover cloud launch enforcement.

The standalone Rust command uses existing crate dependencies. Both new artifacts inherit the existing mandatory, high-weight attribute for `scripts/casper-soak/**`.

The implementation does not change the workflow, planner, accepted profile implementations, node sources, or their claims. Campaign dispatch remains blocked.

The initial interface has two commands:

```text
casper-campaign-reservation init ROOT CONFIG_JSON
casper-campaign-reservation reserve ROOT REQUEST_JSON
```

The root is an absolute path in a trusted local filesystem. Initialization creates one new owner-only directory and an immutable campaign binding.

The configuration binds the campaign identifier, campaign identity digest, approval digest, and selected preflight candidate. These are declarations, not authenticated approval.

A request repeats those bindings and identifies its stage, candidate, run identifier, and first run attempt. Requests do not select resource quantities.

The store has three fixed slots: one preflight and one baseline for each architecture. Stability reservations require a separate implementation and resource decision.

## R1: Bounded and exact inputs

Each input is one JSON object of at most 4,096 bytes. Reject duplicate keys, unknown fields, invalid identifiers, malformed digests, unsupported stages, and unsupported candidates.

Reject missing, nonregular, symbolic-link, oversized, or unreadable input files. Require schema version 1 and run attempt 1.

## R2: Fixed campaign binding

Initialization must reject an existing root. Reservation must reject a missing or invalid binding and any request that differs from that binding.

The implementation must not create a replacement store automatically. A changed campaign identifier, approval digest, or identity digest must not reset the existing budget.

Require an owner-only root owned by the effective user. Reject symbolic links in the root path and noncanonical paths.

## R3: Permanent consumption

A successful reservation consumes exactly one fixed slot. Reject an occupied slot even when the request bytes match.

Reject reuse of a run identifier in another slot. Baseline reservation requires an existing preflight reservation for the configured candidate.

A preflight reservation does not establish a passing preflight. The future controller must verify passing evidence separately.

The implementation must provide no reset, deletion, expiry, retry, replacement, or refund operation. Failed launches and lost responses must not release a slot.

## R4: Serialization and durability

Serialize reservation checks and writes with an exclusive, nonblocking filesystem lock. Contention must reject rather than wait indefinitely.

Write and synchronize the complete record before publishing its fixed slot without replacement. Synchronize the containing directory before returning success.

A visible reservation with an uncertain acknowledgment remains consumed. Partial or corrupt existing records must reject further reservations rather than count as unused slots.

A killed lock holder must not require deletion of a persistent lock file. Storage and synchronization failures must produce nonzero exits.

## R5: Truthful boundaries

Receipts must identify local reservation scope and disabled execution. The command must not invoke a launcher, execute a node, or authenticate approval.

Tests must exercise the executable with real files and competing processes. Retain failed checks and exact source and binary identities.

## Assumptions and exclusions

The caller supplies one stable root for the entire authorized budget. The owner and privileged processes must not modify, replace, restore, or remove the store.

Filesystem locking, exclusive publication, and synchronization must provide their documented local semantics. Network filesystems and distributed controllers are outside this claim.

Finite input and operation bounds do not establish a hard wall-clock bound for operating-system I/O.

The command does not prevent another controller from selecting another root or launching outside this protocol. Global persistent accounting remains an integration requirement.

The future controller must authenticate approvals, validate exact source and candidate identities, verify prior results, and reserve before submitting a launch.

The future controller must retain uncertain launch outcomes without another submission. It must independently enforce instance lifetime and confirm cleanup.

No local fixture proves power-loss recovery, distributed exclusivity, real instance termination, live qualification, or a passing campaign. Source-bound verification and explicit acceptance remain pending.
