# Casper Authority Observer Client Claim

```yaml
claim_id: CLAIM-CASPER-AUTHORITY-CLIENT-001
status: pending
scope: harness-observer-transport
pre_merge_tasks: [TASK-017-12]
artifacts:
  - scripts/casper-soak/src/authority_observer.rs
  - scripts/casper-soak/src/bin/casper-authority-observe.rs
  - scripts/casper-soak/tests/authority_observer.rs
refutation: pending
construction: pending
binding: pending
soak: pending
```

## Scope

The client collects one authority response from the Linux node observer. It does not launch a node or change campaign admission.

The existing authority profile accepts synthetic fixtures only. This claim does not extend that acceptance to live observations.

## Requirements

The client requires an exact node process, process start time, executable digest, declared revision, configuration digest, and approved request digest.

The client verifies the socket peer with Linux credentials. It verifies the process start time and executable through the local process filesystem.

The client binds the request to the observer challenge and incarnation. The response must match the request identifier, request digest, and complete observer identity.

One deadline bounds connection and socket transfers. Each frame has a limit of 1,048,576 bytes. Duplicate JSON keys and invalid frame lengths cause rejection.

The response sequence must exceed the greeting sequence. Its monotonic time must not precede the greeting time.

The response must retain the requested authority inputs. Missing or failed evaluations remain unavailable observations.

The client retains exact greeting, request, and response bytes as separate files. Failure reports retain captured evidence and never replace existing output.

A successful capture establishes transport correspondence only. Qualification, profile verdicts, and campaign acceptance remain pending.

## Verification boundary

Controlled Linux socket fixtures test identity rejection, correlation, frame bounds, timeouts, and retained evidence. These fixtures do not qualify a real node candidate.

The live adapter still needs scenario mapping, node qualification, source-bound verification, and named acceptance. Synthetic fixture injection cannot be inferred from detached evaluation.

Filesystem operations and process scheduling remain external timing assumptions. The deadline does not establish an operating-system response guarantee.
