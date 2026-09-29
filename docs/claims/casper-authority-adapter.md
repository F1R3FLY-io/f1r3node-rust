# Casper Authority Adapter Claim

```yaml
claim_id: CLAIM-CASPER-AUTHORITY-ADAPTER-001
status: pending
scope: harness-authority-observation-mapping
pre_merge_tasks: [TASK-017-12]
artifacts:
  - scripts/casper-soak/src/authority_mapping.rs
  - scripts/casper-soak/tests/authority_mapping.rs
  - scripts/casper-soak/process_faults.py
  - scripts/casper-soak/tests/test_process_faults.py
  - scripts/casper-soak/check-authority-adapter.sh
  - .github/workflows/casper-authority-adapter.yml
refutation: pending
construction: pending
binding: pending
soak: pending
```

## Observation mapping

The observer client retains `mapping.json` after a successful transport capture. A mapping failure retains the raw response and a separate rejected mapping result.

The mapper preserves binary32 bits. It converts finite values to reduced rational values only when the signed numerator and unsigned denominator fit the profile schema.

The mapper retains nonfinite and unrepresentable values with an explicit reason. It does not replace a rounded value with an ideal mathematical fraction.

The mapper checks oracle decisions against exact stake witnesses. It preserves early returns and distinguishes strict comparisons from inclusive comparisons.

Persisted finality comes from the captured persisted observation. An oracle decision or detached floor result does not establish that persisted state.

The mapper retains unavailable, failed, and unrequested values. Work counters retain their node names and request scope.

Metadata reads do not become distinct visited vertices. Traversal operations do not become traversed edges.

## Process receipts

The Python recorder operates on an owned subprocess through the provider's node wrapper. It rejects adopted handles before any process action.

The recorder verifies the predecessor capture digest, artifact digests, request correlation, node identity, process identifier, and process start time.

Pause requires an observed stopped state. The recorder then requests resume in cleanup and records whether the running state returns.

Restart requires an observed predecessor exit and a different child process. A new observer capture must identify a different incarnation with unchanged candidate identities.

Readiness means that the authority endpoint supplied an available capture. It does not establish network convergence or full candidate qualification.

The recorder binds receipt time to the host boot identifier and Linux monotonic clock. Late observations cannot produce an applied receipt.

Provider calls require separate timing qualification. The recorder checks elapsed time but cannot interrupt a blocked provider call.

The caller supplies the provider node, fault request, predecessor capture, replacement capture callback, and a new output directory. The replacement callback receives the fault deadline.

Capture references consist of a directory path and an expected report digest. The caller must retain both capture directories with the receipt package.

## Verification and limits

Controlled Linux tests use disposable child processes and fixture observer records. They do not run a blockchain node or qualify the integration provider.

The Rust tests cover numeric boundaries, threshold comparisons, input mismatch, unavailable observations, and retained capture evidence.

The workflow runs both test groups and Clippy. Its source inventory must remain unchanged throughout verification.

The accepted authority profile still rejects live inputs. This claim does not authorize fixture substitution, campaign dispatch, or claim discharge.

Paired fork-choice observations, captured display inputs, executable fixtures, and profile input bindings remain required for the live profile.

Source-bound acceptance and live qualification remain pending.
