# TASK-017-5 Authority and Finality Profile

## Current review

The [combined review](./task-017-5-7-binding-review.md) records fresh native and isolated Linux checks, model controls, and shared regressions.

Final profile checks passed. Human binding acceptance and workflow-tag ratification remain pending. Live adapters remain unqualified.

The sections below retain the original implementation results. Their unavailable-Linux result is historical.

## Status

The controlled-transcript implementation is ready for review. TASK-017-5 remains in progress because CLAIM-CASPER-SOAK-002 acceptance is pending.

The implementation base is `d871a2df83a69dbb0ddaafffef609612cf8388ea`. No Git staging, commit, push, node campaign, external repin, or claim waiver occurred.

The [evidence package](../casper/cbc-evidence/runs/casper-authority-finality-20260918-01/report.json) records the tested sources, verification results, failures, and remaining limits.

## Implementation

- The separate `casper-authority-finality` binary preserves the accepted lifecycle source files and their evidence.
- The generator produces deterministic reference/bounded requests from identical pinned inputs.
- The collector checks artifact bytes, identities, producer order, duplicate copies, deadlines, and capture state.
- The classifier preserves explicit holds, unknown measurements, product failures, applied-step receipts, and restart relations.
- Checked threshold arithmetic requires strict majority before the inclusive boundary comparison.
- The profile retains raw records and input copies without overwriting an existing output.
- The standalone workflow runs the profile fixtures and its clean model plus three negative controls.

The [profile guide](../../formal/tlaplus/casper_soak/profiles/authority_finality/README.md) defines commands, concrete records, supported bounds, and model assumptions.

DAG and signature fixtures are opaque synthetic inputs. The generator creates evaluation requests, not valid node blocks or cryptographic proofs.

## Verification

| Check | Result |
| --- | --- |
| Profile Rust tests | 8 passed. |
| Controlled fixture cases | 58 cases, 65 binary invocations. |
| Arithmetic generation | 2,000 small-integer cases, plus overflow and denominator controls. |
| Clean TLC model | 3,281 generated states and 1,681 distinct states. |
| Negative TLC controls | All three exited 12 with the named property and trace. |
| Shared host regressions | 11 tests passed across manifests, model results, binding inventory, and claim audits. |
| Linux static build | Passed for the binary and profile tests. |
| Linux execution | Not run because Docker was unavailable. |
| Rust formatting and Bash syntax | Passed. |
| Active language-server checks | Five checks confirmed clean. One remained inconclusive. |
| Strict CLAIM-CASPER-SOAK-001 audit | Passed without changing its accepted artifacts. |
| Strict CLAIM-CASPER-SOAK-002 audit | Exit 4, as required while acceptance remains pending. |

The final local runner used test optimization level 0 and native Java 26.0.2.1. TLC used the existing pinned 2.19 JAR.

The final Linux cross-build used test optimization level 1 and the existing static musl flags. A build result does not establish Linux execution.

The checks do not establish node correctness, live adapter qualification, unbounded liveness, process containment, or a passing soak.

## Binding review map

| Property | Concrete binding |
| --- | --- |
| `MismatchedInputDetected` | Request validation checks paired digests and identities before generation. Input preparation binds the manifest, configuration, fixtures, and executable. |
| `MissingFinalityDetected` | Presence-state validation and classification keep missing finality incomplete. An explicit hold requires an observed finality decision. |
| `HeadMismatchReported` | Classification compares correlated heads and retains both artifact references. Missing work measurements do not erase this failure. |

The model assumes other required fields and applied-step receipts are valid. Executable tests check malformed inputs and additional receipt conditions outside that abstraction.

Construction is not applicable. The retained search and executable bindings require review before claim discharge.

## Failure history

1. A regression test exposed missing fault tolerance as `product_failure` instead of `incomplete`. The test failed with exit 101 before the correction.
2. A regression test accepted a restart receipt from the wrong candidate. The test failed with exit 101 before identity checks corrected the result.
3. A regression test accepted a replay request without an applied-step receipt. The test failed with exit 101 before receipt coverage became mandatory.
4. The default Java executable had an incompatible architecture. The native Java installation could run TLC.
5. Three model-wrapper attempts failed with TLC exit 255 because configurations were outside the model directory.
6. The corrected wrapper rebases only the staged plan's model path. It retains both plans and verifies unchanged model and configuration bytes.
7. The first combined local run failed during optimized dependency compilation with a Rust/LLVM target-feature error.
8. An explicit optimization-level-0 retry passed. No assertion, model bound, or verdict check was removed.

The package retains these logs instead of replacing them with successful retries. Historical RED logs do not contain complete source snapshots for every intermediate edit.

## Remaining gates

- Accept the bounded binding review for CLAIM-CASPER-SOAK-002.
- Ratify the proposed mandatory tag for `.github/workflows/casper-authority-finality.yml` before commit.
- Run the compiled Linux fixtures when Docker or a Linux worker is available.
- Qualify actual same-DAG, electorate, finality, and fault-tolerance adapters before live execution.
- Keep final workload qualification and resource approval under TASK-017-12.

The workflow tag remains unapplied. Existing broad tags cover the new Rust, Bash, and formal files.

TASK-017-4 remains complete. Other profile claims, the candidate matrix, production defaults, and post-merge gates remain unchanged.
