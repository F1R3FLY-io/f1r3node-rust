# TASK-017-6 Publication and Restart Profile

## Scope

The controlled-transcript implementation is complete. TASK-017-6 remains in progress because claim acceptance and workflow-tag ratification remain pending.

The implementation base is `b5c24da0217465eeef5fb07e3d9126fb759a5b59`. Existing lifecycle and authority/finality sources and acceptance records remain unchanged.

The [claim](../claims/casper-soak-publication.md) remains pending. The [implementation contract](../../formal/tlaplus/casper_soak/profiles/publication/README.md) defines its requests, receipts, snapshots, and commands.

## Implementation

- [x] Bind configuration, fixture expectations, capabilities, and source identities.
- [x] Require matching cut-point, exit, and linked restart receipts.
- [x] Classify atomic tuples, stale generations, unresolved work, and durable verdict retention.
- [x] Exercise complete, blocked, deterministic, and negative fixtures through the real profile binary.
- [x] Run the clean bounded model and three named negative controls.
- [x] Retain verification evidence and identify remaining acceptance gates.

The separate `casper-publication` binary launches no node. It verifies exact manifest bytes and compiled profile, helper, and executable identities.

Capability proofs bind the provider, node revision, node binary, external-harness revision, profile, executable, and evidence kind.

The generator records an ordered publication/crash/restart schedule. The collector preserves raw sources and rejects conflicting copies, malformed data, and unsafe paths.

Unrelated or late observations remain quarantined. They cannot fill required receipt or snapshot slots.

The classifier compares complete tuples. It keeps occurrence identities separate when deploy signatures match and requires durable verdict evidence before accepting work eviction.

Missing measurements remain unknown. They cannot erase independently observed work loss, durable-verdict loss, or earlier tuple failures.

## Verification

The retained package is [casper-publication-20260918-01](../casper/cbc-evidence/runs/casper-publication-20260918-01/report.json).

| Check | Result |
| --- | --- |
| Native Rust profile tests | Six passed. |
| Controlled fixture cases | 57 cases and 61 binary invocations passed. |
| Tuple component combinations | All 16 combinations passed their expected verdict checks. |
| Clean TLC control | Exit 0, 3,281 generated states, and 1,681 distinct states. |
| Missing-acknowledgment control | Exit 12 with `FaultAcknowledged` and a counterexample. |
| Wrong-restart control | Exit 12 with `RestartIdentityMatched` and a counterexample. |
| Torn-tuple control | Exit 12 with `TornTupleReported` and a counterexample. |
| Existing profile and shared host regressions | 19 tests passed. |
| Linux-musl cross-build | Passed for the final source. |
| Linux execution | Not run because the Docker socket is unavailable. |
| Strict CLAIM-001 audit | Passed without changing the accepted claim. |
| Strict CLAIM-003 audit | Refused discharge with exit 4. |

Native tests use test optimization level 0. The Linux cross-build uses level 1. Both use the pinned Rust toolchain.

The bounded model abstracts two scenarios and three collection slots. It assumes other fields are valid and excludes independent earlier failures.

Executable tests separately cover qualification pins, stale generations, receipts, occurrence multiplicity, durable verdicts, missing measurements, duplicate records, artifact corruption, and blocked policies.

The new PR/nightly workflow retains its verification output. No hosted workflow execution is claimed by these local results.

The final language-server checks report no errors or warnings. One informational Rust 2024 suggestion remains deferred because this crate uses Rust 2021.

## Failure history

The initial compilation failed because derive macros were unavailable and one JSON macro expression needed parentheses. The corrected code does not change dependency features.

Three RED/GREEN cycles preserve their source snapshots and failed results:

1. A recovered snapshot with a wrong predecessor incorrectly passed. The collector now rejects that correlation.
2. Unknown inventory fields hid independently observed losses. The classifier now evaluates work loss and durable-verdict loss separately.
3. A qualification proof with a wrong node-binary digest incorrectly passed. Qualification now requires the exact node-binary digest.

The first claim-audit commands used an unsupported flag and exited 2. Corrected `--strict` commands produced the expected results.

Earlier passing runs remain historical evidence. The `final-source` directory identifies the final tested source and all four final model controls.

## Remaining gates

- Human binding acceptance must precede CLAIM-003 discharge.
- The proposed `cbc=mandatory` tag for `.github/workflows/casper-publication.yml` remains unratified and unapplied.
- Linux execution remains unverified despite the successful cross-build.
- Live publication-boundary, atomic-snapshot, and durable-work adapters remain unqualified.
- Parallel-policy and post-merge requests remain blocked.
- Candidate qualification, workload pins, resource approval, and node campaigns remain under TASK-017-12.

Generic restarts do not establish publication-boundary crash coverage. Provider assertions do not independently prove process containment or node storage atomicity.

This work proves neither node correctness nor unbounded liveness. Construction is not applicable, and no Rocq proof or node-code discharge is claimed.

TASK-017-5 acceptance remains pending. TASK-017-4 remains complete. No claim waiver or automatic task completion applies.

The Git index changed outside this session's commands. Those staged entries remain untouched. This assistant did not stage, commit, or push.
