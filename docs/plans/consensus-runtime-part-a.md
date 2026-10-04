# Part A: CBC Casper runtime extraction

Status: Implemented and committed locally. Focused acceptance tests passed. The official network suite has 122 passes and two resource-guard errors.

Baseline: `dev` at `2e7c8e906b0c48b0886f75481bf56efdc15fcc73`.

## What changed

`NodeRuntime` starts one prepared `ConsensusRuntime`. It no longer constructs or schedules Casper engines, block processors, proposers, or maintenance loops.

The `CasperConsensusAdapter` owns those components. The existing Casper algorithms remain in the `casper` crate.

The codebase-design skill guided this boundary. The shared interface manages lifecycle and requests. It does not require a universal block or snapshot.

| Component | Responsibility | Code |
| --- | --- | --- |
| Shared contract | Commands, capabilities, status, opaque identifiers, packet metadata, typed failures | [consensus-api](../../consensus-api/src/lib.rs) |
| Shared runtime | Bounded queues, request deadlines, readiness, supervision, shutdown | [consensus-runtime](../../consensus-runtime/src/lib.rs) |
| Node host | Servers, discovery, transport, process signals | [NodeRuntime](../../node/src/rust/runtime/node_runtime.rs) |
| Factory | Select the configured consensus adapter | [factory](../../node/src/rust/consensus/factory.rs) |
| Casper assembly | Construct native stores, execution facilities, engine, queues, and existing API services | [assembly](../../node/src/rust/consensus/casper/assembly.rs) |
| Casper adapter | Run bootstrap, native workers, maintenance, packet handling, and command handling | [adapter](../../node/src/rust/consensus/casper/mod.rs) |
| API compatibility | Preserve native deploy and proposal responses and error classification | [api_compat](../../node/src/rust/consensus/casper/api_compat.rs) |
| Store identity | Check protocol, schema, network, shard, and genesis before writable store access | [manifest](../../node/src/rust/consensus/manifest.rs) |

## Defaults and compatibility

CBC Casper remains the default and the only selectable consensus in Part A. Rholang remains the execution runtime.

An optional HOCON setting makes the default explicit:

```hocon
consensus {
  protocol = "cbc-casper"
}
```

Unknown protocol names fail during configuration parsing. Cordial selection is not enabled.

Existing packet tags, protobufs, block hashes, genesis rules, and native store layouts stay unchanged.

Existing HTTP and gRPC services use the neutral handle for deploy submission and proposal requests. Native query APIs remain in the Casper compatibility layer.

The native `Casper` and `MultiParentCasper` traits remain available below the adapter. Existing helper constructors remain available for native tests and other callers.

The shared crates have no dependency on Casper, models, Rholang, RSpace, or concrete stores.

## Suggested review order

1. Read `consensus-api/src/lib.rs` and `consensus-runtime/src/lib.rs` for the new boundary.
2. Compare the old `runtime/setup.rs` with `consensus/casper/assembly.rs`. Most construction code moved here.
3. Review `consensus/casper/mod.rs` and the block/proposal worker changes. These contain the task-ownership and shutdown changes.
4. Review the API compatibility module and the manifest checks.
5. Read `node/tests/consensus_casper.rs` for the observable acceptance criteria.

Use `git log --oneline` to identify the extraction commits. Use `git show <commit>` to review each change with its tests.

## Lifecycle

1. Parse the selected protocol and node configuration.
2. Check the manifest and inspect existing Casper identity through read-only database access.
3. Prepare the adapter and application services without starting protocol tasks.
4. Install the packet route and start host servers and discovery.
5. Start the consensus supervisor.
6. Launch the native Casper engine and its supervised workers.
7. Run initialization and keep packet handling available during the peer wait.
8. Record the manifest after native recovery or genesis identifies the chain.
9. Publish `Ready`.

Commands require both a supported capability and `Ready`. Packet handling is available during bootstrap.

Shutdown uses a separate signal. Full command or packet queues cannot block that signal.

The adapter stops new work, stops maintenance, drains active requests and worker queues, and shuts down its store manager.

The runtime enforces a drain deadline. A timeout is a failure, not a successful flush. Forced cancellation relies on native transactional recovery.

The host stops transport after the consensus shutdown attempt. HTTP servers receive an explicit stop signal.

## Task ownership

The adapter supervises the block processor, proposer, heartbeat, dependency recovery, fork-choice maintenance, merge GC, runtime state requester, and report workers.

Casper genesis approval and deploy-triggered proposal tasks use an injected task spawner. Task panics and genesis task errors reach the adapter.

Proposal rejections retain the existing retry behavior.

Block-processing child tasks belong to a `JoinSet`. The supervisor observes child panics even when no further packet arrives.

Command tasks and packet tasks have separate concurrency limits. Waiting proposal requests cannot occupy every packet-processing slot.

The standalone runtime tests use a fake adapter. The Casper acceptance test uses production Casper assembly, real signatures, Rholang, and LMDB.

## Store manifest

The additive file is `<data-dir>/consensus-manifest.json`.

It records protocol, protocol version, manifest schema version, network, shard, and genesis. Local query capabilities are not chain identity.

A manifest mismatch fails before writable native stores open. Legacy stores without a manifest must contain recognizable Casper identity.

The migration does not replace native databases. An unidentified or truncated store without a recoverable genesis identity fails closed.

A temporary file and a no-replace atomic link publish the manifest. Existing manifests cannot be silently overwritten.

## Verification

The required acceptance test is `casper_runtime_deploy_propose_finalize_recover`.

It must verify real deploy execution and finality beyond genesis. It must then reopen the same data directory and preserve finalized progress.

The test also covers HTTP/gRPC service delegation, duplicate-deploy rejection, manifest mismatch, legacy manifest adoption, and read-only restart. It checks that finalized deploy status survives recovery.

A read-only second adapter starts from a copy of the first adapter's closed genesis-store snapshot. It receives subsequent blocks in reverse order. Its native block processor must recover dependencies, replay execution, and reach the same finalized block. No new blocks are copied into the follower's store.

The fixture uses one bonded validator, which is sufficient for its native quorum. This is not a multi-validator fault-tolerance test.

Malformed packets must not stop the adapter. A block with a changed header and an invalid signature must not enter the follower's native DAG. Valid received blocks must enter that DAG and retain the same post-state hashes.

Additional tests cover peer-wait cancellation, failed initialization, unsupported protocol configuration, and the absence of native Casper types in `NodeRuntime`.

Run these commands from the workspace root:

```bash
rtk cargo test --locked --offline -p consensus-runtime
rtk cargo test --locked --offline --release -p node --test consensus_casper
rtk cargo test --locked --offline --release -p node --lib consensus::
rtk cargo test --locked --offline --release -p node --lib api::
rtk cargo test --locked --offline --release -p node --lib
rtk cargo test --locked --offline --release -p casper --test mod multi_parent_casper_smoke_spec
rtk cargo test --locked --offline --release -p casper --test mod multi_parent_casper_finalization_spec
rtk cargo test --locked --offline --release -p casper --test mod multi_parent_casper_communication_spec
rtk cargo check --locked --offline --release -p node --tests
rtk cargo tree --locked --offline -p consensus-runtime
rtk cargo clippy --locked --offline -p consensus-api -p consensus-runtime --all-targets -- -D warnings
rtk cargo fmt --all -- --check
rtk git diff --check
```

The CI test matrix includes both shared crates. The node test target includes the real Casper acceptance test without an ignore flag.

### Results on 2026-09-29

| Check | Result |
| --- | --- |
| Neutral runtime lifecycle tests | 15 passed |
| Production Casper adapter tests | 6 passed |
| Complete node library tests | 256 passed |
| Existing Casper smoke test | 1 passed |
| Existing Casper finalization tests | 2 passed |
| Existing Casper communication tests | 3 passed |
| Node and test-target compilation | Passed |
| Strict Clippy for both shared crates and their test targets | Passed |
| Workspace formatting check | Passed; existing formatter configuration warnings remain |
| Shared-crate dependency inspection | No Casper, models, VM, or concrete store dependencies |
| Whitespace and patch checks | `git diff --check` passed |

The node library suite was run using its Cargo-built release test binary. The API tests need permission to bind local ephemeral ports. Two initially failed at socket binding inside the sandbox; all 256 passed with that permission.

The acceptance test exposed a native-store codec mismatch during development. The manifest reader now uses the native LMDB `SerdeBincode<Vec<u8>>` key/value codecs. The passing test checks both legacy adoption and rejection of a mismatched genesis before writable store access.

Both `HEAD` and the fetched `origin/dev` remain at the baseline above. These results do not claim remote CI.

### Earlier official network integration results

The official harness ran against the Part A release binary on 2026-09-29. The harness revision was `e3c4e14189f0c6ced2e9674487fcbdeffd93141b`.

The run used real local processes, TLS peers, HTTP, gRPC, Rholang execution, and persistent stores. The official test code stayed unchanged.

All 124 unique cases ran across 38 files. That run contained 120 clean passes and four unsuccessful cases, with no skips. A later complete rerun is recorded below.

| Test | Result |
| --- | --- |
| Concurrent set-cell updates | Application assertions passed. Teardown found `FinalityDivergence` diagnostics on all five nodes. |
| Deploy throughput and finalization | High load reached 9,759 MB combined node RSS. The 8,192 MB safety guard stopped six nodes. |
| Validator lifecycle | Combined node RSS reached 9,036 MB. The safety guard stopped eight nodes before the full lifecycle completed. |
| Slow-deploy convergence | The uninterrupted rerun reached finalized block 29 on all five nodes. Teardown failed after RSS reached 10,266 MB. |

Host suspend interrupted the first slow-deploy attempt. The complete convergence module then ran again with sleep prevention and unchanged memory limits.

All three convergence test bodies passed on rerun. Its teardown error still prevents a clean pass for the slow-deploy case.

The separate five-case official deployment smoke test passed. A separate TLS handshake check verified TLS 1.3 and certificate validation.

The network acceptance gate remains failed. The run does not establish whether Part A introduced the finality diagnostic or memory growth.

Compare the same workload against unchanged `dev` before assigning a cause. Do not disable the memory guard on this host.

This was a single-host integration run, not a production or WAN deployment. Four earlier orphan test processes remained until final cleanup.

No test nodes, pytest workers, or temporary sleep inhibitors remain. Logs are preserved. No source fixes or commits were made during network testing.

See the [full evidence report](../../../consensus-network-test.dzTsey/evidence/network-deployment-report.md) for commands, exact failures, cleanup details, and linked reports.

### Latest complete official rerun, verified 2026-10-02

The subsequent sleep-protected run completed all 124 cases: **122 passed, two errored,
zero skipped**. The concurrent application-update and slow-deploy cases passed in this run.
The full network acceptance gate is still not green.

| Unsuccessful test | Recorded guard condition |
| --- | --- |
| `test_deploy_throughput_and_finalization` | Host available RAM 1,798 MB below the 2,000 MB floor; six node processes stopped. |
| `test_validator_lifecycle` | Combined node RSS 8,554 MB above the 8,192 MB ceiling and host available RAM 1,153 MB below the floor; eight node processes stopped. |

The harness revision remained `e3c4e14189f0c6ced2e9674487fcbdeffd93141b`.
The release-node SHA-256 before and after the run was
`199af1a4a279cceba09411270cc2c2a65fd9234d59c7541787c205b1430fc4f7`.
No node rebuild or harness-test change occurred during the run.

The runner selected all declared node capabilities and used `--provider=subprocess`,
`--run-all-node-capability-tests`, `--timeout-scale=2`, `--monitor`,
`--rss-ceiling-mb=8192`, `--host-free-floor-mb=2000`, `-n 1`, and `--timeout=1200`.
Each batch retains its complete command and logs.

Evidence: [aggregate results](../../../consensus-network-test.dzTsey/evidence/full-batched-20261001-rerun/report.json),
[JUnit](../../../consensus-network-test.dzTsey/evidence/full-batched-20261001-rerun/junit.xml), and
[binary verification](../../../consensus-network-test.dzTsey/evidence/full-batched-20261001-rerun/binary-verification.json).

Do not classify the two resource-guard errors as passes or remove host protection to obtain a
green run. Repeat on an adequately provisioned isolated host and compare unchanged `dev`
before attributing memory growth to Part A. This remains single-host network integration,
not production or WAN deployment evidence.

## Review boundaries

Part B now contains the pinned native crates and versioned admission corrections. No Cordial
production adapter or protocol stub is registered. See [Native admission v2](../../cordial/ADMISSION-V2.md).

The local acceptance harness uses controlled transport and invokes HTTP/gRPC service implementations directly. The separate official harness tested real multi-node network paths.

The official network gate remains failed for the reasons above. Full workspace tests and coverage measurement were not run.

The process host's broader partial-server-startup cleanup paths were not validated by this harness. Protocol preparation is lazy: it does not start protocol workers before the host starts the runtime.

A request deadline can expire after native work starts. It does not roll back an admitted deploy or block.

Check the deploy ID or proposal-result API before retrying an operation after a timeout.

Casper reports native finalized progress. The adapter does not advertise a universal ordered-output capability.

Execution construction remains in the Casper assembly module for this extraction. Part B will introduce shared execution ports where the Cordial integration needs them.

No production consensus algorithm is intentionally changed. Review the task ownership edits separately from the mostly mechanical assembly move.

No commits, staging, or remote publication are part of this implementation pass.
