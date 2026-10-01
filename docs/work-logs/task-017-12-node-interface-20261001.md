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
