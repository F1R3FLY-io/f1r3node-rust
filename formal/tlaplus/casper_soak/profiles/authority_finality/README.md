# Authority and Finality Profile

This profile implements CLAIM-CASPER-SOAK-002 for controlled transcripts. It does not execute a node or change the accepted lifecycle implementation.

The [claim](../../../../../docs/claims/casper-soak-authority-finality.md) and [common contract](../../../../../docs/casper/design/soak-interface-contract.md) define the verification boundary.

## Execution boundary

The separate `casper-authority-finality` binary validates inputs, generates paired evaluation requests, collects retained observations, and classifies one scenario.

The existing `casper-soak` runtime still admits only its lifecycle fixture. This profile cannot authorize a live run, production policy change, or post-merge adaptation.

Live requests remain blocked because the required same-DAG, electorate, and fault-tolerance adapters are not qualified.

```mermaid
flowchart LR
  P[Pinned manifest and artifacts] --> V[Validate source and qualification]
  V --> G[Generate paired requests]
  G --> C[Collect digest-checked records]
  C --> R[Classify scenario]
  R --> E[Retain report and sources]
```

## Commands

Build the profile binary:

```bash
cargo build --locked -p casper-soak --bin casper-authority-finality
```

Run the fixture suite and model controls:

```bash
TLA_TOOLS_JAR="$HOME/.tla/tla2tools.jar" \
  bash scripts/casper-soak/check-authority-finality.sh target/authority-checks
```

Set `SOAK_AUTHORITY_JAVA` if the default Java executable is unsuitable. The runner verifies the pinned TLC JAR digest before execution.

Inspect the compiled identity:

```bash
target/debug/casper-authority-finality identity
```

Process a retained fixture with the same binary:

```bash
target/debug/casper-authority-finality run \
  --manifest target/authority-checks/fixtures/authority_finality_complete/manifest.json \
  --request target/authority-checks/fixtures/authority_finality_complete/request.json \
  --artifacts target/authority-checks/fixtures/authority_finality_complete \
  --output target/authority-replay
```

Each output path must be new. The binary retains exact manifest and request bytes, input copies, raw observations, generation requests, collection results, and the scenario report.

Exit codes are `0` for a passed scenario, `1` for failure or incomplete evidence, `2` for invalid input, and `3` for blocked execution.

Every scenario result remains separate from claim discharge and the final soak verdict. A passed synthetic scenario does not become a node observation.

## Concrete record layout

The test fixtures provide complete machine-readable examples. Request records extend the common contract as follows:

| Field | Requirement |
| --- | --- |
| `manifest_digest` | SHA-256 of exact manifest bytes. |
| `segment`, `iteration`, `seed` | Canonical decimal strings. |
| `members` | Two distinct members with `reference` and `bounded` modes. |
| Member identities | Candidate, revision, binary digest, node, incarnation, and DAG/electorate/justification digests. |
| `inputs` | Artifact references for configuration, DAG, electorate, justification, fixture, and expectation. |
| `protocol_context` | Pinned protocol and accounting context. |
| `metadata_availability` | `complete` or `missing_dependencies`. |
| `threshold_inputs` | `q`, `S`, `agreeing_stake`, `n`, and positive `d`. |
| `require_work` | An exact Boolean. Traversal comparisons require counters. |
| `observation_deadline` | Clock identifier and decimal monotonic nanoseconds. |
| `fault_schedule` | At most one pause or restart, with target, trigger, and acknowledgment deadline. |

DAG, electorate, and justification objects are opaque, digest-bound inputs. The synthetic fixtures do not construct valid node blocks or signatures.

The profile preserves supplied committee provenance and duplicate justification entries. It does not introduce floors, sidecars, or certificates.

Each `authority_snapshot` contains `head`, `finality`, `work`, and `evaluation_receipt` measurements. Measurements contain `presence`, `value`, and `reason`.

An observed measurement requires a value and null reason. A missing or error measurement requires a null value and a nonempty reason.

Finality contains a decision, hold reason, exact Boolean threshold result, original fault tolerance, and projection.

Rational measurements use a signed 64-bit numerator and positive unsigned 64-bit denominator. Equivalent fractions compare exactly without floating-point conversion.

Work counters use `visited_vertices` and `traversed_edges`. An observed zero remains distinct from a missing counter.

Evaluation receipts identify the fixture digest, applied status, and ordered steps. Replay, injection, and missing-dependency requests cannot establish coverage without matching receipts.

Restart receipts additionally require Boolean prior-exit and readiness fields. The predecessor and new incarnation must match the request.

A member can instead set `incarnation_binding` to `observed_restart` and `incarnation` to `pending-restart`. This mode requires one scheduled restart with a pinned predecessor.

The applied restart acknowledgment enrolls the observed successor UUID. Its candidate, fault identity, prior exit, readiness, clock, and deadline must match the request.

The successor must differ from the predecessor. Subsequent observations must use that successor and occur at or after readiness.

The executable receipt validator also requires enrollment before it accepts a successor observation. The bounded model assumes these concrete identity checks.

Receipts record provider assertions. They do not independently prove node execution, process ownership, or containment.

The collector reads `observations.json`, which lists raw artifact references. It validates digests, capture status, producer identities, event identities, and producer sequence order.

Identical copies count once but retain all raw sources. Conflicting copies, foreign identities, invalid paths, and malformed records cannot produce a passed scenario.

The initial transport bound is 64 records. A valid scenario has two unique snapshots and at most one fault receipt.

## Threshold and classification rules

Threshold inputs use unsigned 64-bit integers. Intermediate arithmetic uses checked unsigned 128-bit operations. Overflow produces invalid input, not a threshold result.

The supported domain requires `0 <= q <= agreeing_stake <= S`, `S > 0`, and `0 <= n <= d`.

Strict agreeing majority precedes the inclusive threshold comparison `2qd >= S(d+n)`. A threshold-component pass does not establish finalization.

Pinned expectations can require a hold or rejection despite a passing threshold component. A finalized expectation cannot contradict missing metadata or a failed threshold.

Missing fault tolerance remains incomplete. It does not become a disagreement or zero. Missing counters cannot erase an independently observed head mismatch.

Malformed evidence takes precedence in the scenario verdict. The report still retains independently observed product failures.

## Bounded model and bindings

The model explores two scenarios with three collection slots each. The slots abstract two snapshots and one optional fault receipt.

It models paired-input equality, finality availability, and head agreement. Other required fields and applied-step receipts are assumed valid.

| Property | Unsafe control | Executable fixture |
| --- | --- | --- |
| `MismatchedInputDetected` | `PairDifferentDags` | `authority_pair_mismatch` |
| `MissingFinalityDetected` | `AcceptMissingFinality` | `authority_finality_missing` |
| `HeadMismatchReported` | `SuppressHeadMismatch` | `authority_head_mismatch` |

The clean search must finish without errors. Each unsafe control must exit 12 with its named violation and counterexample trace.

The wrapper uses the unchanged lifecycle model runner. It rebases only the plan's model path because TLC requires configuration files beside the model.

The evidence retains both plan versions and their digests. Model and configuration bytes remain unchanged. Original inputs must retain their digests after execution.

The model does not prove arithmetic, cryptography, node correctness, or unbounded liveness. Executable tests cover checked arithmetic and the concrete record bindings.

## Verification and remaining gates

The independent workflow runs on relevant pull requests and nightly. It does not modify the accepted shared lifecycle workflow or its control inventory.

The new workflow needs human ratification for its proposed `cbc=mandatory` tag. Existing broad tags already cover the new Rust, Bash, and formal files.

Claim discharge remains pending until the bounded binding review is accepted. Linux execution and live adapter qualification remain separate from local fixture success.

## Executable scenario bindings

The `execute` command runs a pinned provider executable before collection and classification:

```bash
casper-authority-finality execute \
  --manifest manifest.json \
  --request request.json \
  --artifacts inputs \
  --output result
```

The existing admission checks run first. Missing capabilities, unqualified live requests, and unsupported phases return `blocked` without invoking the executor.

The manifest adds `runtime.authority_executor` with these fields:

| Field | Contract |
| --- | --- |
| `artifact` | Relative executable path, exact byte length, and SHA-256 digest. The executable limit is 128 MiB. |
| `arguments` | At most 32 literal arguments. Each argument has a limit of 4,096 bytes. No shell expands them. |
| `timeout_ms` | An integer from 1 through 300,000. |
| `assets` | Bounded artifact references for additional provider inputs. |

The harness retains the executable, fixture inputs, qualification records, and provider assets before launch. The manifest digest binds the execution configuration.

The executor receives two environment variables. `CASPER_AUTHORITY_EXECUTION_REQUEST` names its request file, and `CASPER_AUTHORITY_EXECUTION_OUTPUT` names its result directory.

The request contains the prepared profile request, generated workloads, ordered operations, input root, output root, and timeout. A fresh execution nonce prevents receipt reuse.

Each operation contains its index, operation name, and complete member identity. The order comes from the existing profile generator.

The executor writes `execution.json` with schema version 1, the exact execution request digest, and an ordered `receipts` array of artifact references.

The executor should update that inventory atomically after each completed step. A valid partial inventory permits failure preservation after interruption.

Each receipt contains:

| Field | Contract |
| --- | --- |
| `schema_version` | Integer 1. |
| `request_sha256` | Digest of the exact execution request bytes. |
| `previous_receipt_sha256` | Null for the first receipt, then the preceding receipt digest. |
| `step` | The exact generated operation, including its index and member. |
| `elapsed_ns` | Nondecreasing unsigned elapsed nanoseconds within the executor timeout. |
| `status` | `applied`, `not_applied`, or `unknown`. |
| `observed_inputs` | Artifact references for retained DAG, electorate, and justification bytes. Each digest must match its pinned input. |
| `observations` | Artifact references for authority snapshots or scheduled fault acknowledgments. |

An unapplied step may have an empty observed-input inventory. It cannot produce a passing execution binding.

Authority snapshots must belong to evaluation steps. Restart acknowledgments must belong to the matching restart barrier and scheduled member.

The harness retains intermediate observations. It sends each member's final evaluation and scheduled fault acknowledgments to the existing collector and classifier.

A missing final evaluation does not fall back to an earlier result. A missing step or unapplied receipt makes the execution incomplete.

The harness rejects foreign request digests, broken receipt chains, reordered steps, changed input bytes, duplicate identities, and invalid artifact paths.

A failing executor cannot convert a product failure into a pass. A valid partial receipt inventory retains product failures from completed observations.

The harness kills the executor process group after exit or timeout. This control does not establish containment of processes that escape that group.

The top-level report records the executor outcome separately from the profile verdict. It retains stdout, stderr, receipts, input artifacts, and observation references.

Controlled tests execute all nine scenario kinds through this command. The fixture executable supplies synthetic provider records and does not launch blockchain nodes.

Live execution still requires a qualified provider that supplies actual applied inputs and node observations. These bindings do not implement missing node interfaces.
