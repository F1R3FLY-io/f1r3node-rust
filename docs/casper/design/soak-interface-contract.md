# Casper Soak Interface Contract

**Owner:** TASK-017-2. **Contract version:** 1.

This document specifies interfaces and fixture expectations. It does not implement adapters, qualify node interfaces, or discharge harness claims.

The [branch plan](../../plans/casper-ratified-soak-2026-09-16.md) defines authority and phase boundaries. The [claim index](../../claims/casper-soak-harness.md) identifies all eight claims.

## Source scope

The current mandatory inventory contains twenty-three harness, model, configuration, and workflow artifacts. TASK-017-2 adds no node artifact or node-proof obligation.

TASK-017-4 owns driver integration, manifest validation, event correlation, and result publication. TASK-017-5 through TASK-017-11 own their profile implementations.

The implementation boundaries are `scripts/run-merge-recovery-soak.sh`, `scripts/bench/test-run-merge-recovery-soak.sh`, and `scripts/bench/write-soak-summary.sh`.

The driver calls `scripts/bench/casper-soak.sh`, which executes the Rust `casper-soak` binary. Rust tests verify manifest handling and real-driver behavior.

Build the binary with `cargo build --locked -p casper-soak`. Set `SOAK_HARNESS_BIN` when the binary is outside `target/debug/`.

New harness and profile code uses Rust and Bash. The existing external Python suite and existing native diagnostic fixtures remain unchanged.

`SOAK_MANIFEST_PATH` enables identity binding only. It does not qualify capabilities, authorize dispatch, or establish a passing profile verdict.

The driver retains exact manifest bytes in `.casper-manifest.json`. Resume requires those bytes and the matching checkpoint digest.

Existing unbound runs cannot acquire a new manifest identity. The seven node profiles remain blocked, and legacy load runs retain their separate behavior.

The lifecycle fixture uses a compiled Rust adapter and a Bash executor. It cannot produce node observations.

`SOAK_INPUT_DIR`, `SOAK_APPROVAL_PATH`, and `SOAK_APPROVAL_SHA256` enable the qualified execution path. The manifest additionally pins `runtime.harness_digest` and selects `runtime.executor_kind` as `bash` or `native`.

The runtime checks source bytes against its compiled source copies. It checks executable bytes against the manifest before dispatch.

Python-era manifests do not qualify the Rust runtime. Historical evidence remains valid only for its recorded source identities.

Proposed Rust profile modules live under `scripts/casper-soak/src/profiles/`: `authority_finality.rs`, `publication.rs`, `recovery.rs`, `merge_accounting.rs`, `slashing.rs`, `version_phlo.rs`, and `carrier_index.rs`.

Each proposed module exports `generate(request)`, `collect(manifest, artifacts)`, and `classify(request, observations, acknowledgments)`.

`generate` returns ordered workload and fault requests. `collect` returns observations and rejected-source records. `classify` returns one scenario result without executing node logic.

These modules do not exist yet. Add each module to the mandatory inventory when its owner implements it. No proposed interface is an existing command-line option.

Existing node claims remain independent. Construction is not applicable. Missing product capabilities block scenarios instead of authorizing Rust changes.

## Audited external interface

The selected system-integration source is `F1R3FLY-io/system-integration` at `b3d14b27e3c6276b1eb4ab9ccef04e02b0c4e283`.

The target tree's `.github/oci-validation.env` selects this suite. The workflow's launcher pin has a separate role, even when both values match.

The local sibling checkout is older. This audit read the selected revision through the GitHub API without changing either checkout or pin.

Paths in this table are relative to that external repository. “Primitive” means source support exists, not that an EPIC-017 binding has passed.

| ID | Source boundary at the selected revision | Input and output | Availability and limit |
| --- | --- | --- | --- |
| SI-LOAD | `integration-tests/test/tests/custom/test_load.py::test_deploy_throughput_and_finalization` | Provider, timeouts, resource monitor → pytest result and phase report | Primitive. Fixed load scenarios, not a generic profile dispatcher. |
| SI-DEPLOY | `integration-tests/test/infra/node.py::Node.deploy_string` and `Node.deploy` | Rholang or deploy protobuf, test signer, both Phlo fields → deploy signature ID | Primitive. Envelope capture and mutation need a profile adapter. Private keys must never enter evidence. |
| SI-QUERY | Same file: `get_block`, `get_blocks`, `last_finalized_block`, `is_finalized`, `deploy_status`, `get_event_data` | Block/deploy identifiers → client response or transport exception | Primitive. A response is not a complete committee, publication, settlement, or custody observation. |
| SI-FAULT | `integration-tests/test/infra/providers/base.py::NodeHandle`, with implementations in `docker.py` and `subprocess.py` | `pause`, `unpause`, `restart`, `stop`, `wait_for_exit` → command completion or process status | Primitive. Pause/restart methods do not return a structured fault acknowledgment. |
| SI-METRICS | `Node.http_get`, provider `monitor_output_dir`, and load phase report | `/metrics`, CSV files, text report → raw samples | Primitive. Aggregate samples do not establish exact event identity or path engagement. |
| SI-ADOPTED | `subprocess.py::_AdoptedHandle.restart` | Adopted handle → `NotImplementedError` | Unsupported. An adopted subprocess cannot satisfy a restart scenario through this method. |

`Node.get_current_block_number` returns last-finalized height, not DAG-tip height. Profiles must record these measurements separately.

The selected load test sets three stress options: `--heartbeat-self-propose-cooldown=3seconds`, `--heartbeat-advanced-frontier-chase-max-lag=20`, and `--max-user-deploys-per-block=128`.

These settings are not production defaults. Existing load runs cannot silently become the ratified recovery-policy baseline.

The source audit does not qualify arbitrary-DAG loading, publication cut points, delivery ordering, exact settlement extraction, or carrier-path selection.

Each affected scenario remains blocked until a pinned adapter supplies the required receipt and observations. A generic HTTP helper does not establish endpoint support.

## Current driver boundary

| ID | Exact local boundary | Existing contract | Required addition, owned by TASK-017-4 |
| --- | --- | --- | --- |
| DR-START | Environment initialization and state-file load | Requires `SOAK_DURATION_SECONDS` and `SYSTEM_INTEGRATION_DIR`. Accepts `SOAK_OUTPUT_DIR`, target identity, deadlines, and resource limits | Validate immutable manifest and capabilities before side effects. Do not execute untrusted state as shell input. |
| DR-STATE | `persist_soak_state` | Writes `.soak-state` and `.soak-checkpoint-state.json` through temporary files | Bind state to manifest digest. Preserve history, failures, and immutable artifact references across segments. |
| DR-LAUNCH | Main loop's `poetry run pytest` call | Executes `integration-tests/test/tests/custom/test_load.py` with alternating `docker` and `subprocess` providers | Select the declared profile and pass its pinned request. Record launch count and receipt. |
| DR-OBSERVE | `emit_iteration_metrics`, `iteration_finalization_latency` | Reads pytest text and telemetry roots into `iteration-*/metrics.json` | Emit correlated events and explicit missing/error observations. Existing phase counts are not deploy counts. |
| DR-CAPTURE | `snapshot_iteration_monitor_outputs` | Copies the newest matching CSV or marker from three telemetry roots | Require session identity, complete inventory, and digest verification. Modification time alone is insufficient correlation. |
| DR-STOP | Deadline loop, terminal markers, `cleanup_soak_processes` | Stops workload processes and handles resource markers | Preserve failure evidence before cleanup. Retain PR #431's B44 limitation until separately resolved. |
| DR-SUMMARY | `scripts/bench/write-soak-summary.sh` | Writes `iterations.json` and `summary.json`. The `.ok` field originates from process exit status | Add explicit coverage and scenario verdicts. Neither exit zero nor a fallback summary establishes conformance. |
| DR-PUBLISH | `.github/workflows/merge-recovery-soak.yml` | Selects image/suite, runs segments, and uploads artifacts | Require complete manifest and result inventory. No shared workflow change belongs to TASK-017-2. |

Telemetry roots are `integration-tests/data`, `integration-tests/log-archive`, and `integration-tests/.subprocess-data` within the selected external checkout.

Current metric collection is fail-soft. Missing pytest counts can become zero, and missing finalization phase counts become zero samples.

Contract adapters must distinguish those defaults from observed zero values. Historical summaries without required provenance cannot become conforming profile evidence.

## Common record contract

The following records are required design interfaces, not implemented schemas. JSON objects use UTF-8 and integer schema version `1`.

SHA-256 fields contain lowercase hexadecimal digests of exact retained bytes. Repository revisions use full commit IDs. Artifact paths are relative to the run directory.

Numbers that represent token amounts, heights, stake, or Phlo use decimal integer strings. This avoids precision loss in JSON consumers.

Unknown observations use `null` with a reason. Missing required identifiers, malformed numbers, duplicate record IDs, and unsupported schema versions cannot pass validation.

IDs are nonempty strings. Each transport record has a unique `record_id`. Repeated observations may share `event_id`, but not `record_id`.

`phase` is `pre_pr216_merge` or `post_pr216_merge`. `seed` is an unsigned decimal integer string. `required_scenarios` is a nonempty array of unique scenario IDs.

`source_digests` maps relative source paths to SHA-256 strings. `tool_versions` maps tool names to observed version strings, not requested versions.

`capabilities` maps capability IDs to status, provider, revision, and qualification references. Status is `qualified`, `unsupported`, or `unknown`.

Source-audited primitives remain `unknown` until their required adapter binding passes. Unsupported and unknown required capabilities prevent node launch.

`resource_limits` names child count, timeout seconds, RSS ceiling MB, host-free floor MB, disk-free floor MB, and artifact-byte budget with explicit units.

`presence` is `observed`, `missing`, or `error`. Missing/error observations require a reason and null value, not a substituted zero.

Observation times carry a clock ID, monotonic nanoseconds as a decimal string, and UTC text. A deadline identifies its clock and numeric time basis.

| Record | Required fields and constraints |
| --- | --- |
| Manifest identity fields | `run_id`, `phase`, `candidate_id`, `node_revision`, `node_binary_digest`, `image_digest`, `harness_revision`, `external_harness_revision`, `source_digests`, `configuration_digest`, `profile_id`, `profile_digest` |
| Additional required manifest fields | `fixture_digest`, `expectation_digest`, `seed`, `provider`, `policy_variant`, `evidence_kind`, `capabilities`, `tool_versions`, `bounds`, `assumptions`, `resource_limits`, `required_scenarios`, `deadline`, `merge_gate` |
| Scenario request | Manifest digest, `scenario_id`, `pair_id`, `member_id`, input fixture references, expected-value references, required observation kinds, fault schedule, observation deadline |
| Fault request | `fault_id`, target node and process incarnation, action, trigger event, bounded acknowledgment deadline, requested ordering constraints |
| Fault acknowledgment | Same identity and `fault_id`, observed action/state, supporting raw artifact reference, producer sequence, observation time, status `applied`, `not_applied`, or `unknown` |
| Observation | Manifest digest, run/scenario/pair/member IDs, node, incarnation, segment, iteration, producer, producer sequence, event ID, event kind, payload, presence state, raw artifact reference |
| Artifact reference | Relative path, byte length, SHA-256, producer, capture state, and associated observation IDs |
| Scenario result | Identity fields, `scenario_verdict`, `product_failures`, required/observed coverage, missing observations, rejected observations, fault receipts, measurements, raw references |
| Run result | Manifest digest, `harness_verification`, `soak_verdict`, `termination`, scenario results, artifact inventory, requested/completed case counts, excluded or blocked scenarios, and reasons |

A subprocess run records `image_digest: null` with an explicit provider reason and a mandatory binary digest. A Docker run requires both build identity and image digest.

`evidence_kind` is `synthetic_fixture` or `node_observation`. The classifier must never promote synthetic fixture results into node observations.

`source_digests` includes harness, model, configuration, fixture, and adapter sources. A dirty source tree requires a retained patch digest, not only a base revision.

`merge_gate` is not applicable before merge. After merge, it names the actual PR #216 merge, selected `dev` revision, and ancestry-check artifact.

The accepted TASK-017-13 handoff is also required after merge. An open candidate head, matching version label, or successful unit test cannot satisfy this gate.

### Correlation and restart

The comparison key includes manifest, scenario, pair member, node, and incarnation. Iteration numbers increase across segments and never identify different immutable inputs.

Paired members share fixture digest, seed, electorate, availability, and applicable protocol context. Allowed differences are declared before launch, such as bounded versus reference mode.

Cross-candidate comparisons declare both candidate identities. Unrelated runs cannot become paired observations merely because their block heights match.

Repeated copies with the same event ID and payload count once. Conflicting payloads for one event ID invalidate evidence and remain in the rejected-observation inventory.

Independent executions retain distinct execution or occurrence identities even when their effects match. Transport-level log deduplication cannot merge those identities.

Restart creates a new incarnation linked to its predecessor. Recovery comparisons require an explicit predecessor/successor relation, not equal process names.

A monotonic producer sequence establishes local order. Wall-clock timestamps alone cannot establish cross-node delivery order.

### Fault acknowledgment

A requested pause is not an observed pause. Docker acknowledgment needs observed paused state, and subprocess acknowledgment needs observed stopped state.

A restart acknowledgment requires prior exit, a new incarnation, and readiness evidence. Method return, timeout, or process-name reuse is insufficient.

Emergency process termination may precede capture. Deleting required evidence may not. The model's cleanup property constrains artifact deletion, not emergency termination.

A publication cut-point scenario also needs a matching boundary event. An arbitrary process restart cannot count as a crash at that boundary.

Message-order scenarios need observed delivery receipts for each message. A requested schedule cannot substitute for those receipts.

If capability discovery lacks a required interface, the scenario is `blocked` before launch. If a supported operation lacks its receipt, the launched scenario is `incomplete`.

### Classification

`scenario_verdict` is `passed`, `product_failure`, `incomplete`, `blocked`, or `invalid_input`.

`harness_verification` is `pending`, `passed`, or `failed`. It describes separately retained model and executable-fixture evidence, not the node's behavior.

`termination` is `completed`, `deadline`, `resource_stop`, `cancelled`, `tool_error`, or `infrastructure_failure`. It does not replace a scenario verdict.

`soak_verdict` is `passed` only when all required scenarios pass with complete evidence and applicable harness verification. Otherwise, it is `non_passing`.

A planted mismatch in a synthetic fixture must yield `product_failure` in its scenario result. Correct detection makes the fixture pass, not the product.

A product failure remains recorded after cancellation, resource termination, or missing later observations. Unknown observations cannot erase an earlier failure.

A valid observed zero is distinct from missing data. A transport exception is not an explicit node hold, rejection, pending state, or zero counter.

Zero executed scenarios, skipped prerequisites, missing artifacts, unobserved fault coverage, and missing finality samples cannot yield `passed`.

Expected node rejections can satisfy a positive scenario when the pinned expectation requires rejection. Negative TLC controls are a separate verifier registry.

### Fixture execution interface

Executable fixtures must call the actual driver or profile implementation. A separate reference classifier cannot substitute for that implementation.

Driver fixtures extend `scripts/bench/test-run-merge-recovery-soak.sh`. Controlled executables may replace external tools, but the real driver must launch them.

Profile fixtures must invoke the production generator, collector, and classifier through the same dispatch path selected for node runs.

Each profile requires `<profile>_complete` and `<profile>_capability_missing` positive fixture IDs, using underscore-separated profile names.

The first uses complete known data and expects `passed`. The second advertises an unsupported required capability and expects `blocked` with zero node launches.

Each profile also requires `<profile>_generation`: equal seed and input bytes produce equal ordered workloads, while declared differences remain explicit.

The three negative fixture IDs in each claim test defects in handling those records. An implementation that always reports incomplete cannot pass the positive fixtures.

Each fixture retains input bytes, expected result, actual result, invocation, source digests, and exit status. Failure must return nonzero to the fixture runner.

Synthetic transcripts remain sufficient for harness binding tests, but never for product evidence. No profile fixture is implemented by this specification.

## Harness fixture matrix

All fixture IDs below are required and unimplemented as real-driver bindings. The existing local TLC runner covers only the bounded refutation slice.

Each row needs a matching positive control. Each negative mutation must fail before the corresponding implementation repair and pass after correct handling.

| ID | Model property | Controlled input and boundary | Required executable-fixture assertion |
| --- | --- | --- | --- |
| resume_changed_identity | IdentityPinned | DR-START/DR-STATE: resume with each immutable identity component changed independently | Reject before launch. Preserve prior state and artifact digests. Matching identity resumes. |
| resume_overwrites_iteration | ResumePreservesHistory | DR-STATE: two segments with existing iterations and failure records | New iteration ID exceeds every old ID. Earlier bytes and failure records remain unchanged. |
| product_failure_then_resource_stop | ProductFailureMonotone | DR-OBSERVE/DR-STOP: observed failure followed by disk or memory stop | Retain failure and `termination=resource_stop`. Require `soak_verdict=non_passing`. |
| missing_artifact_cannot_pass | PassRequiresEvidence | DR-SUMMARY/DR-PUBLISH: complete observations with missing or corrupt required artifact | Reject publication as passing. Complete inventory is the positive control. |
| terminal_marker_prevents_launch | StopPreventsLaunch | DR-START/DR-LAUNCH: terminal marker, expired deadline, or resource stop | Launch counter remains unchanged. Exercise each stop cause separately. |
| capture_before_cleanup | EvidenceBeforeCleanup | DR-CAPTURE/DR-STOP: capture acknowledgment delayed or failed | No evidence deletion before successful capture and digest verification. Retain incomplete outcome on failure. |
| experiment_cannot_change_baseline | PolicyIsolation | DR-START/DR-LAUNCH: experimental policy plus baseline output directory | Refuse mixed identity or use a separate run. Baseline configuration bytes remain unchanged. |
| missing_finalization_samples | MissingIsUnknown | DR-OBSERVE: absent, malformed, explicit zero, and duplicated phase rows | Missing remains unknown, zero remains observed, and copies do not increase counts. |
| open_candidate_is_not_merge | PostMergeGate | DR-START: open candidate, unrelated merge, valid merged ancestry with accepted handoff | Reject the first two before launch. Admit only the verified post-merge case. |
| timeout_is_not_counterexample | ControlVerdictExact | Local runner and DR-PUBLISH: wrong property, exit zero, timeout, cancellation, missing trace | Accept only exit 12 plus the exact registered violation and trace. Clean control separately requires completed exit zero. |

The model has two candidates, two segments, four total iterations, and one active child. Evidence completeness is Boolean, not filesystem durability.

Profile models start with two scenarios and three observations per scenario. Larger fixture families are separate executions, not silent expansion of model coverage.

Payload digests abstract DAGs and node execution. This bound does not prove arbitrary-DAG behavior or arbitrary-length observation histories.

Assumptions require immutable input fixtures, trustworthy source identity, and declared observation deadlines. Durability additionally assumes the documented filesystem contract and available storage.

Missing capability, missing evidence, or an unmet assumption prevents a positive product verdict. None authorizes a policy change.

Property campaigns retain actual counts against the 2,000 PR, 10,000 nightly, and at least 100,000 extended targets. These targets do not count as executed cases.

## Profile obligations

Each linked claim defines its exact additional fields, fixture IDs, expected outcomes, interface blockers, and post-merge adaptation.

| Profile | Claim | Required capabilities beyond common records | Implementation owner | Post-merge owner |
| --- | --- | --- | --- | --- |
| authority-finality | [002](../../claims/casper-soak-authority-finality.md) | Same-DAG paired evaluation, electorate/context records, explicit finality decisions | TASK-017-5 | TASK-018-3 |
| publication | [003](../../claims/casper-soak-publication.md) | Publication cut-point receipts, atomic tuple and durable-work observations | TASK-017-6 | TASK-018-3 |
| recovery | [004](../../claims/casper-soak-recovery.md) | Lane labels, exact occurrences, custody/terminal events, objective heights | TASK-017-7 | TASK-018-3 |
| merge-accounting | [005](../../claims/casper-soak-merge-accounting.md) | Execution identities, admission/effect records, settlement and token domains | TASK-017-8 | TASK-018-4 |
| slashing | [006](../../claims/casper-soak-slashing.md) | Ordered delivery receipts, parent pre-state, epoch-bound authorization results | TASK-017-9 | TASK-018-3 |
| version-phlo | [007](../../claims/casper-soak-version-phlo.md) | Signed-envelope bytes, separate authority labels, acceptance and settlement observations | TASK-017-11 | TASK-018-4 |
| carrier-index | [008](../../claims/casper-soak-carrier-index.md) | Path-selection receipts, matched scan windows, availability and work counters | TASK-017-10 | TASK-018-4 |

All seven end-to-end profile adapters are unimplemented. Existing primitives do not change that status.

TASK-018-2 rebinds the common interfaces. TASK-018-5 reruns compatible profiles with new identities after the actual merge and accepted handoff.

No pre-merge fixture result discharges a post-merge binding. Missing merged-runtime interfaces remain blocked rather than causing node work inside these epics.
