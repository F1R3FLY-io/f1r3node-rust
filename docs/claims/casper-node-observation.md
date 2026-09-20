# Casper Node Observation Interface

```yaml
claim_id: CLAIM-CASPER-NODE-OBSERVATION-001
status: pending
adapter: null
scope: batch-a-local-capability-interface
artifacts:
  - node/src/rust/soak_observer.rs
  - node/src/rust/mod.rs
  - node/src/rust/configuration/model.rs
  - node/src/rust/configuration/commandline/options.rs
  - node/src/rust/configuration/commandline/config_mapper.rs
  - node/src/rust/configuration/mod.rs
  - node/src/rust/runtime/node_runtime.rs
  - node/src/rust/diagnostics/tests.rs
  - node/tests/soak_observer.rs
refutation: pending
construction: pending
binding: pending
soak: pending
```

## Authorization and scope

The user confirmed Batch A and ratified mandatory high-weight tags for the observer module and its dedicated tests.

The approved [file-level plan](https://github.com/F1R3FLY-io/f1r3node-rust/blob/7b8865aaa49fdec69b5b3dea8a239c06b4d399b6/docs/plans/casper-node-interface-prerequisite.md) defines this separate node prerequisite.

Implementation starts from `dev` revision `6940a5beb4aa806d3d75f6df3be9f238512fcc2f` on `feature/casper-node-observation`.

This claim covers local transport, session identity, bounded requests, configuration, and capability reporting. It does not cover consensus correctness or qualify authority and publication profiles.

## Required properties

1. Default startup creates no observer socket, journal, evaluator, or task.
2. Activation requires explicit configuration and valid finite limits.
3. The socket directory has safe ownership and owner-only access. Path components cannot be symbolic links.
4. Peer credentials must match the configured process and its startup identity.
5. Each request matches a fresh challenge, observer incarnation, executable digest, public configuration digest, and configured approval digest.
6. A session accepts at most one request. A new session cannot reuse an earlier challenge.
7. Request and response frames cannot exceed 1 MiB. A deadline bounds the complete session.
8. The interface executes no fault control, evaluation, or storage mutation. Unsupported capabilities remain explicitly unsupported.
9. Cleanup removes only the socket identity created by this observer. Normal shutdown completes cleanup before process exit.
10. Configuration evidence contains only explicit public fields. The interface never serializes the complete node configuration.

## Trust and evidence boundaries

The Linux kernel, process credential interface, executable file, and root-owned filesystem namespace are trusted inputs.

Processes with the node owner's privileges remain trusted. Process matching adds request correlation, not protection against a malicious owner or root process.

The startup configuration supplies the approved request digest and declared source revision. These declarations do not authenticate campaign approval or attest a source-to-binary build.

The executable digest measures the running executable. Integration tests therefore identify their test executable, not a production candidate node.

The configuration digest covers a versioned public allowlist, not the complete node configuration. Its scope must accompany every digest.

Batch A supports only capability discovery. Authority snapshots, publication receipts, durable inventories, fault controls, and recovery remain unavailable.

Batches B and C require their separate reviews. PR #216 must actually merge before occurrence-level recovery qualification.

The peer must share the observer's process namespace. Deadline enforcement depends on runtime scheduling and does not guarantee host survival or scheduling latency.

A filesystem failure can leave a socket after cleanup. The observer reports that failure and refuses to overwrite an existing path on restart.

## Batch A protocol

Activation uses `--soak-observer JSON` or the `soak-observer` configuration section. The JSON option accepts at most 4,096 bytes and rejects unknown fields.

| Configuration field | Required value or limit |
| --- | --- |
| `directory` | An absolute, owner-only directory path of at most 90 bytes. |
| `source-revision` | A declared revision with 40 lowercase hexadecimal digits. |
| `approved-request-sha256` | A configured digest with 64 lowercase hexadecimal digits. |
| `peer-pid` | A positive process identifier within the signed 32-bit range. |
| `peer-start-ticks` | The process start value from Linux `/proc/PID/stat`. |
| `session-timeout-ms` | An integer from 50 through 30,000. |
| `max-sessions` | An integer from 1 through 4,096. |

The socket name is `observer.sock`. Its directory requires mode `0700`, and the socket uses mode `0600`.

Every frame has a four-byte, big-endian length followed by one JSON object. Empty frames and frames above 1 MiB are invalid.

The server checks peer credentials before sending a `hello` record. That record supplies the challenge, identity, permission, clock scope, and configured approval digest.

A request includes `schema_version`, `request_id`, `incarnation`, `challenge`, `executable_sha256`, `configuration_sha256`, `approved_request_sha256`, and `operation`.

The only operation is `capabilities`. The request identifier uses canonical lowercase syntax for a Universally Unique Identifier (UUID).

A successful response includes the original request digest, identity, sequence, and monotonic timestamp. All five listed profile capabilities remain unsupported, and `live_profile_qualified` is false.

Each connection accepts one request. The observer serves one connection at a time and closes rejected or expired sessions.

The session budget counts all accepted connections. Exhaustion stops the observer without terminating the node.

The `batch-a-public-config-v1` digest covers an explicit JSON allowlist:

- `schema_version`
- `network_id`
- `shard_name`
- `standalone`
- `max_parent_depth`
- `max_number_of_parents`
- `fault_tolerance_threshold_bits`

The observer bounds network and shard names at 256 bytes each. Executable measurement rejects files above 512 MiB.

## Local implementation evidence

The [Batch A report](../cbc-evidence/runs/casper-node-observer-batch-a-877cea722-01/report.json) identifies the source bytes and local checks.

All 255 node library tests passed. The interface suite passed 17 tests in both native and isolated execution.

Each suite also invoked its normally ignored peer-test helper in a separate process. The isolated run used the host-built executable, not an independent rebuild.

The final formatting check and all-target node Clippy check passed. Initial compile and Clippy failures remain in the retained development logs.

The interface tests use a test executable. No running blockchain node, live adapter, campaign image, or consensus result was qualified.

## Shutdown correction

A later source review found an early process exit inside `NodeRuntime::main`. That exit bypassed observer cleanup in `start`.

The correction returns the node program result to `start`. The existing exit handler runs after the observer stops.

The [shutdown work log](../work-logs/casper-node-observer-shutdown-review.md) identifies the retained failing check, correction, and new local results.

The new regression checks source ordering. It is not an end-to-end production node shutdown test.

The earlier Batch A report remains unchanged. Its passing transport tests did not establish the missing runtime shutdown property.

Batch B remains unapproved. The [revised proposal](../plans/casper-node-observation-batch-b.md) separates bounded storage capture from later authority evaluation.

## Verification requirements

Retain failing controls for configuration, permissions, peer identity, request identity, replay, frame bounds, deadlines, and cleanup.

Verify disabled behavior and existing configuration regressions. Verify that source and dependency inventories contain no unrelated changes.

Unit and integration tests supply evidence but do not discharge this claim. Source-bound verification and explicit acceptance remain pending.

This work does not change the harness claims, approve a campaign, publish images, or merge a pull request.
