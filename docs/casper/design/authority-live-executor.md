# Live authority executor

The `casper-authority-live` executable connects a workload driver to the Linux authority observer. It retains captures and emits the existing execution receipt format.

This implementation supports qualification runs. The authority profile still blocks live campaign admission.

## Invocation

Build the executable:

```bash
cargo build --locked -p casper-soak --bin casper-authority-live
```

Run a prepared qualification request:

```bash
casper-authority-live --request execution-request.json --output /absolute/new/results
```

The executable also accepts `CASPER_AUTHORITY_EXECUTION_REQUEST` and `CASPER_AUTHORITY_EXECUTION_OUTPUT`. These variables connect it to the existing executor launch protocol.

The request uses the execution envelope defined in the [scenario binding guide](../../../formal/tlaplus/casper_soak/profiles/authority_finality/README.md#executable-scenario-bindings).

The request must select `node_observation`, `pre_pr216_merge`, and `baseline`. Its operation inventory must exactly match the scenario and sorted paired members.

The input and output roots must be absolute paths. The output directory must be new or empty.

The observation deadline must use the current host clock. Its clock identifier is `linux-monotonic:` followed by `/proc/sys/kernel/random/boot_id`.

The deadline value uses unsigned decimal nanoseconds from `CLOCK_MONOTONIC`. The executor also enforces the envelope timeout, up to 300,000 milliseconds.

## Provider configuration

The manifest contains `runtime.authority_live` with `driver` and `members` fields. The manifest identity must match the prepared request.

The `driver` object contains these fields:

| Field | Meaning |
| --- | --- |
| `path` | Absolute path to the workload driver executable. |
| `sha256` | Digest of the exact executable bytes. |
| `bytes` | Exact executable length, at most 128 MiB. |
| `arguments` | At most 32 literal arguments, each at most 4,096 bytes. |

The executor copies and verifies the driver before invocation. It does not use a shell to expand arguments.

A null driver permits observation capture without fixture application. Every corresponding execution receipt remains unknown, and the execution result remains incomplete.

Each `members` key matches a request member identifier. Its value contains these fields:

| Field | Meaning |
| --- | --- |
| `binding` | The existing observer client binding, including socket, process identity, candidate digests, approval digest, request identifier, and timeout. |
| `configuration_sha256` | Expected node public configuration digest. |
| `authority` | A complete node `AuthorityRequest`, including capture and evaluation limits. |
| `target` | The single target hash selected by `authority.targets`. |

Both `authority.original` and `authority.reference` must be true. `authority.strict` must be false because the profile uses the inclusive threshold comparison.

The executor verifies the observer process before each driver invocation. It checks the candidate revision, executable digest, configuration digest, and incarnation.

Each capture receives a distinct request identifier derived from the execution digest, step, and capture position. The observer challenge provides transport freshness.

## Workload driver protocol

The driver receives two environment variables:

- `CASPER_AUTHORITY_STEP_REQUEST` names an immutable step request file.
- `CASPER_AUTHORITY_STEP_OUTPUT` names the step output directory.

The request includes the execution digest, nonce, exact operation, input references, input root, output root, observer binding, and predecessor capture reference.

The deadline uses the same host monotonic clock. The executor terminates the driver process group on exit or deadline expiration.

The driver must perform the requested operation through its supported provider interface. It must not copy expected results into node observations.

The driver writes `result.json` with these fields:

| Field | Meaning |
| --- | --- |
| `schema_version` | Integer 1. |
| `request_sha256` | Digest of the exact step request bytes. |
| `step` | The exact requested operation and member. |
| `status` | `applied`, `not_applied`, or `unknown`. |
| `observed_inputs` | For applied steps, artifact references for exported DAG, electorate, and justification bytes. |
| `snapshot_digest` | For applied steps, the node snapshot digest associated with those exports. |

Artifact paths are relative to the step output directory. Each reference contains `path`, `bytes`, and `sha256`.

The executor checks exported bytes against the pinned inputs. It then captures the node again and checks the snapshot digest.

These checks bind driver assertions to the observed snapshot. They do not prove that a driver applied a fixture correctly.

A candidate-specific driver must implement fixture loading, replay, justification injection, and dependency controls through supported interfaces. The [provider adapter](authority-provider-adaptation.md) submits prepared block fixtures through the existing peer transport.

Fault schedules require a pinned process owner. The executor connects process receipts to observer captures, including restart incarnation checks.

## Observation handling

The executor builds authority observations from the captured node mapping. Driver responses cannot supply head, finality, or work measurements.

Persisted state determines the reported finalization state. Oracle decisions and detached floor outcomes cannot imply persisted finalization.

Bounded evaluations use the measured oracle result. Reference evaluations use the separate reference comparison and retain any disagreement.

Original fault-tolerance values retain their exact binary32 conversion. Missing display projections remain missing.

The current observer does not supply paired fork-choice heads or exact vertex and edge counters. These fields remain unavailable.

The executor retains every raw capture and its mapping. Evaluation observations include the completed operation history and fixture digest.

Each execution receipt binds its predecessor and exact operation. The executor replaces the receipt inventory atomically after each completed step.

A later driver failure preserves earlier evaluation receipts. A deadline expiration leaves a valid partial inventory and retains the interrupted step directory.

The executor monitors driver stdout and stderr sizes. This polling check is not a hard storage quota or containment boundary.

Process group cleanup does not control processes that deliberately leave the group. Provider lifetime and resource qualification remain necessary.

## Verification and remaining work

The Linux tests run a real Unix socket observer fixture and a pinned native workload fixture. They do not launch blockchain nodes.

The adapter workflow runs the live executor tests and Clippy. The gate retains source inventories and controlled JSON captures.

Live completion still requires candidate fixture preparation, captured input exports, missing node observations, and qualification against the selected candidate.

The [live executor claim](../../claims/casper-authority-live-executor.md) remains pending. These changes do not authorize claim discharge or campaign dispatch.
