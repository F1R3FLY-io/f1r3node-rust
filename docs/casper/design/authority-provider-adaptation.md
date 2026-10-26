# Authority provider adaptation

TASK-017-12 uses the existing Rust peer transport to submit prepared block bytes. The legacy integration client supplies the operation pattern.

The optional `p2p` feature builds `casper-authority-p2p`. The executable implements the live executor step protocol.

Each fixture lists the exact block artifacts for each operation. The driver validates the full inventory before the first network send.

The transport uses the current node certificate checks and network identifier. The driver reads its private key from a separate local file.

Delivery receipts retain block digests and transport results. A transport acknowledgment does not prove block validation or DAG admission.

The driver therefore reports an unknown application state until captured node input exports establish the requested state. Expected fixture files cannot serve as observed exports.

A process owner starts a pinned native executable. Only that owner can pause or restart its child through a private Unix socket.

Process requests bind the child PID, start ticks, executable digest, operation, request digest, and host deadline. The owner retains predecessor exit evidence.

The live executor validates the process owner identity before requests. After restart, it captures the replacement node and checks the new incarnation.

Verification must cover actual transport delivery, rejection before side effects, process identity changes, bounded waits, and cleanup. Controlled verification does not qualify a blockchain candidate.

## Build and block configuration

Build the native executables on Linux:

```bash
CARGO_PROFILE_RELEASE_STRIP=symbols cargo build --locked --release -p casper-soak --features p2p \
  --bin casper-authority-p2p --bin casper-authority-process --bin casper-authority-live
```

Pin the resulting driver path, length, and digest in `runtime.authority_live.driver`. The driver must fit the existing 128 MiB executable limit.

The pinned fixture uses this structure. Replace the example artifact values with measured values.

```json
{
  "schema_version": 1,
  "kind": "signed-block-sequence-v1",
  "members": {
    "bounded": {
      "local_peer": "rnode://<client-id>@127.0.0.1?protocol=40410&discovery=40414",
      "target_peer": "rnode://<node-id>@127.0.0.1?protocol=40400&discovery=40404",
      "network_id": "soak-qualification",
      "timeout_ms": 2000,
      "certificate": {"path": "client.pem", "bytes": 1, "sha256": "<digest>"},
      "private_key_path": "/run/soak/private/client-key.pem",
      "operations": {
        "load_fixture": [{"path": "block.pb", "bytes": 1, "sha256": "<digest>"}],
        "replay_fixture": [{"path": "block.pb", "bytes": 1, "sha256": "<digest>"}],
        "evaluate": [],
        "await_restart_receipt": []
      }
    }
  }
}
```

Each requested member needs a configuration. Include block files and certificates in `runtime.authority_executor.assets` when the outer executor stages inputs.

Keep the private key outside the evidence inventory. Its file must belong to the current user and have no group or other permissions.

The `replay_fixture` list must equal the `load_fixture` list. The driver preserves exact protobuf bytes, including invalid signatures and duplicate justifications.

The `load_fixture_with_missing_dependencies` operation submits its listed blocks only. Fixture preparation must omit the intended dependencies and control other peers that could supply them.

The `load_justification_fixture` operation submits its prepared block list. The driver does not construct or sign a valid blockchain history from abstract model inputs.

The driver retains cumulative delivery records after each send. A later transport failure preserves earlier acknowledgments and leaves the application state unknown.

## Process configuration

Create a process configuration with `path`, `sha256`, `arguments`, and `working_directory`. Both paths must be absolute.

Set `observer_socket` to the absolute node socket path. The process owner uses this path for captures and verified restart cleanup.

Use `{owner_pid}` and `{owner_start_ticks}` in the node observer configuration argument. The owner substitutes those tokens before it starts the node.

The node therefore authorizes the persistent owner as its observer client. The live executor obtains captures through that owner.

Start the process owner with a new private output directory:

```bash
casper-authority-process --config process.json --output /run/soak/owner --lifetime-ms 300000
```

The owner writes `owner.json` after it starts the child. Copy its `socket` and `identity` fields into the member `process_owner` configuration.

Use the reported child PID and start ticks in the observer binding. Obtain the initial incarnation through an observer capture.

The owner also accepts capture requests through its client command:

```bash
casper-authority-process --owner /run/soak/owner/owner.json --request capture.json
```

A capture request contains `schema_version: 1`, `action: "capture"`, `binding`, `authority`, `clock_id`, and an integer `deadline_monotonic_ns`.

The response identifies the retained capture directory and report digest. Set the request deadline before `expires_monotonic_ns` from `owner.json`.

The schedule supports one fault. Its `trigger_event` names the exact operation that starts the fault.

For pause, set `pause_hold_ms` in the member configuration. The default is 10 milliseconds and the maximum is 30,000 milliseconds.

The owner verifies the stopped state and resumes the child before it returns. The pause receipt records both transitions.

For restart, use `await_restart_receipt` as the trigger. The owner kills and reaps its child before it starts a replacement.

The owner checks that the socket belongs to the child before termination. After exit, it removes only that same socket device and inode.

The executor updates the PID and start ticks from the owner receipt. It then waits for a verified successor capture.

The production observer generates a random incarnation at startup. A request can explicitly permit successor enrollment with these member fields:

```json
{
  "incarnation": "pending-restart",
  "incarnation_binding": "observed_restart",
  "predecessor_incarnation": "<captured predecessor UUID>"
}
```

The marker requires exactly one scheduled restart for that member. The predecessor incarnation, candidate identity, and fault request remain pinned.

The executor enrolls the successor only after prior exit and a verified capture from the replacement process. The successor must differ from the predecessor.

The restart acknowledgment records the observed incarnation. Receipt validation rejects observations before enrollment and rejects later incarnation changes.

The classifier checks the same enrollment and requires snapshot time at or after successor readiness. A fixed incarnation remains supported without enrollment.

The owner lifetime cannot exceed five minutes. Owner exit kills its direct child, including unexpected owner termination on Linux.

Process groups and parent death signals do not contain descendants that deliberately detach. Docker process controls are outside this implementation.

## Verification scope

The transport test starts the production TLS server and invokes the driver executable. It verifies exact packet bytes and rejection of a foreign network.

The process tests control real native children. The live tests connect those children to controlled observer endpoints and verify pause and restart receipts.

These tests do not start a blockchain node. Selected candidate qualification still requires prepared histories, captured input exports, and complete node observations.
