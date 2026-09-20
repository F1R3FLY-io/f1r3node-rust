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

## Verification requirements

Retain failing controls for configuration, permissions, peer identity, request identity, replay, frame bounds, deadlines, and cleanup.

Verify disabled behavior and existing configuration regressions. Verify that source and dependency inventories contain no unrelated changes.

Unit and integration tests supply evidence but do not discharge this claim. Source-bound verification and explicit acceptance remain pending.

This work does not change the harness claims, approve a campaign, publish images, or merge a pull request.
