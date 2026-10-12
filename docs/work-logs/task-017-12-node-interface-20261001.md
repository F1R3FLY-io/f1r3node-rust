# TASK-017-12 node interface continuation

## Scope

The user authorized this continuation on `formal/soak-casper-consensus` on 2026-10-01.
The initial checkout was `c9ca128214ce137708f852cdc252840e217df7a2`.
The existing task transfer preserves the ownership history.
Other participants can change this shared checkout.

## Plan

- Register the Batch D and Batch E mapping requirements before implementation.
- Test separate display digests, paired heads, missing values, and work counters.
- Correct the mapper without enabling live qualification.
- Review CodeQL alert 41 and nonce freshness.
- Review candidate pins and the remaining campaign admission gates.

## Initial findings

The mapper combines the display digest with the oracle digest.
Batch E supplies a separate display digest, so the mapper rejects a valid display response.
The live executor still marks paired heads and exact traversal measurements unavailable.
The candidate matrix retains null executable workload pins and blocked admission.
The authenticated GitHub API now permits read access to CodeQL alert 41.
The earlier HTTP 403 remains historical evidence.

## Limits

Controlled checks do not qualify a node candidate or deployed services.
No cloud dispatch, infrastructure deployment, Git staging, commit, push, or branch switch is authorized by this continuation.
The task remains in progress until its required acceptance and live execution gates pass.

## Verification

The [compact report](../casper/cbc-evidence/runs/casper-node-interface-adapter-20261001-01/report.json) binds the final source hashes and retained logs.
The isolated Linux run passes 35 tests.
The host authority profile run passes 12 tests.
Both runs retain their ignored subprocess fixture helpers.
The mapper and live snapshot tests retain the intended failing controls.

Targeted Clippy and Rustfmt pass.
The active language server confirms two files clean and cannot confirm two other files.
The Linux source hashes match before and after verification.
The scoped CbC gate returns exit 4 with four pending records.
The source and claim hashes match the updated pending records.
The STE Check passes, but human STE Review remains necessary.

Earlier preparation failures remain recorded.
Unoptimized parallel hashing exceeded fixture deadlines.
The non-executable temporary filesystem refused fixture driver execution.
The corrected isolated run uses optimized tests, one test thread, and executable temporary storage.

## CodeQL review

The full SARIF trace for analysis `1871610769` follows the zero retry counter into the formatted capture argument.
The capture helper hashes that argument into a request identifier.
The counter is not a cryptographic key, initialization vector, or entropy source.
The normal launcher obtains 32 random bytes from the operating system.

The controlled test observes sixteen distinct capture identifiers across two retained executions.
The test also rejects output-directory reuse before another capture.
The retry discriminator increases on each retry.
No global freshness guarantee applies to caller-supplied execution nonces after callers remove retained evidence.

The assessment is a false positive, pending maintainer review.
The remote alert remains open.
No claim or security acceptance follows from this assessment.

## Remaining gates

The selected candidates still pin revision `6940a5beb` and have null workload configuration digests.
The campaign environment has no activation variables.
Candidate block histories, captured input exports, exact traversal qualification, and Docker fault receipts remain incomplete.
Authoritative storage, supervisor activation, source acceptance, and approved live runs remain necessary.

## Git boundary

Other participants staged some shared files during this continuation.
This agent did not stage, commit, push, or switch branches.
Existing staged changes remain untouched.

## Take-over on 2026-10-01

<!-- claude-session-f3cbc961 -->

The user transferred the remaining work of TASK-017-12 from the Batch E owner to claude-session-f3cbc961 after commit `2bfd6d88d`. The tracker records the claim chain and the hand-off.

### Critical path finding

The CI publish gate in `.github/workflows/ci.yml` releases node images on pushes to `dev`, `master`, or a `v` tag only. No published image contains the node observer of claims 004 and 005. The candidate matrix pins node `6940a5beb`, which predates the observer. The observer reaches `dev` through PR #451 and PR #447 (TASK-019-6), and the `dev` image publication is TASK-019-7. The task now lists both as blockers.

The remaining work has this order:

1. The merges to `dev`.
2. The `dev` image publication.
3. The candidate repin and the workload pin.
4. The controlled and live qualification.
5. The maintainer acceptance.
6. The preflight and the baselines under the user's dispatch authorization.

### Controlled preparation

A local image from `node/Dockerfile` on this branch supports the controlled preparation. That preparation covers the `authority_finality` workload pin, the captured input exports, the candidate block histories, and the exact traversal qualification. That image has no registry digest or artifact identity, so it is not an immutable candidate. The preparation starts after the current merge round reaches branch 4.

### Records

The matrix `dispatch_blockers` list has the image publication fact. The matrix digest changed. The inventory validation of 2026-09-28 names the earlier digest for its own cycle and stays correct for that cycle.

## Controlled preparation results on 2026-10-01

<!-- claude-session-f3cbc961 -->

The user approved the controlled preparation without a published image. The run used a release build of revision `009262781` inside a Linux container. The evidence package is `docs/casper/cbc-evidence/runs/casper-controlled-preparation-009262781-01/`. The package status is `controlled-pinned-unqualified`. It qualifies no candidate.

### What ran

| Step | Result |
|------|--------|
| Linux release builds | `node`, `casper-authority-p2p`, `casper-authority-process`, `casper-authority-live`, and a scratch `block-exporter`. Digests are in the report. |
| Block history | A standalone validator with a pinned single-validator bonds file produced blocks 1 to 5. Three deploys of a one-send contract, one propose after each. The exporter read the LMDB blocks store. |
| Owner launch | `casper-authority-process` started a non-validator target node from the genesis snapshot with the observer bound to the owner identity. |
| Manual captures | One capture before the delivery and one after. Both are `captured` and `mapped`. |
| p2p delivery | `casper-authority-p2p` delivered the 5 blocks over TLS. All 5 deliveries are acknowledged. The driver status is `unknown`. |
| Live executor | `casper-authority-live` ran 4 operations for the members `bounded` and `reference`. 4 receipts, 8 captures, zero errors, status `incomplete`, exit 1. |

### Observations

| Capture | Held blocks | Heads | Target display |
|---------|-------------|-------|----------------|
| Before the delivery | 1 | Genesis block | Not held |
| After the delivery | 6 | Block 5, bounded and reference | f32 bit pattern of 1.0 |

`head_matches` is true in all 10 captures. The equivocation capture has 0 rows.
After the delivery the bounded `score_count` is 2 and the reference `score_count` is 1. Batch D finding 1 reproduces on a live chain.

### Findings for the adapter

The report lists findings C1 to C10. The findings with consequences for the live qualification are these:

- C1: Only the owner process can talk to the observer. The supervisor must own the node.
- C3: A target node with the validator key does not adopt the delivered history. The target must run without a validator key.
- C4: The driver returns at the transport acknowledgment. The capture after `load_fixture` saw 3 of 6 blocks. The step capture needs a settle condition.
- C5: The live executor ends `incomplete` because the p2p driver gives `unknown` and exports no observed inputs. The `applied` path needs a driver with input exports for `dag`, `electorate`, and `justification`.
- C9: A stale `observer.sock` blocks the restarted node's observer. The supervisor must clear the observer directory before each launch.

### Remaining gates

The matrix candidates still pin node `6940a5beb` with null workload configuration digests. The matrix has a `controlled_preparation` section that points to the report. The blockers stay TASK-019-6 and TASK-019-7. Maintainer acceptance, candidate repin, an applied driver with input exports, supervisor activation, and approved soak runs remain necessary.
