# Casper Node Observer Shutdown Review

**Branch:** `feature/casper-node-observation`.

**Execution base:** `d021a1d53686098fc8719564c29160a1ad21bb04` with uncommitted source changes.

**Claim:** `CLAIM-CASPER-NODE-OBSERVATION-001` remains pending.

## Defect and correction

Batch A placed observer cleanup after `runtime.main().await` in `start`. However, `NodeRuntime::main` still called the process exit handler.

That handler exits on both success and error. Once execution reached that handler, the caller could not stop the observer or remove its socket.

The correction replaces the inner handler call with `program.await`. The outer handler still selects exit code zero or one after observer cleanup.

The correction changes no consensus rule, storage policy, socket operation, or capability declaration. It stays within the approved Batch A source and test files.

## Retained regression

The new test is `runtime_source_returns_before_observer_cleanup_and_exit` in `node/tests/soak_observer.rs`.

The test rejects the inner exit-handler call. It also checks that `start` awaits the runtime, stops the observer, and then calls the exit handler.

Before the correction, the test failed with exit 101:

```text
assertion failed: !main.contains("handle_unrecoverable_errors("))
```

The failing source and executable remain retained. The corrected source passed the same check.

This regression binds the source call sequence. It does not execute a production node startup or shutdown.

Existing interface tests separately exercise observer cancellation and socket cleanup. Those tests do not replace an end-to-end runtime test.

## Local results

| Check | Result |
| --- | --- |
| Node library tests | 255 passed. |
| Observer interface tests | 18 passed. The parent test also invoked the normally ignored foreign-process helper. |
| Isolated observer tests | 18 passed using the host-built test executable. |
| Formatting | `cargo fmt --all -- --check` passed. |
| Clippy | `cargo clippy --locked --release -p node --all-targets -- -D warnings` passed. |
| Source diagnostics | Both changed Rust files completed active checks. Four existing `nd` identifier spelling findings remain unrelated. |

The first diagnostic probe timed out. The later two-file probe completed without unavailable or incomplete paths.

The isolated test used Linux arm64, a non-root user, no network, no capabilities, and a read-only container filesystem.

The container had 512 MiB memory, two CPUs, 64 process identifiers, and a bounded temporary filesystem. Its terminal state was successful and not out-of-memory.

The isolated run did not rebuild the test executable. It did not qualify an amd64 candidate or a production node image.

## Evidence and claim limits

Bulk evidence is in `target/node-observer-shutdown-d021a1d53-Q2avgR/`.

The [compact report](../cbc-evidence/runs/casper-node-observer-shutdown-d021a1d53-01/report.json) identifies source, specification, executable, and container evidence.

The report records the working-tree execution identity. It does not claim execution at a later commit.

The previous Batch A report and source records remain historical evidence. This correction does not turn those results into runtime shutdown coverage.

The two current observer artifact records remain pending. No prover, hosted verifier, or claim acceptance ran.

No production node or cloud runner launched. TASK-017-12 has no new baseline result.

## Batch B review

The [Batch B proposal](../plans/casper-node-observation-batch-b.md) records constructor, trait, storage, and evaluation findings.

The next proposed implementation is Batch B1: nine files for bounded detached capture. Batch B1 requires file-scope confirmation and four attribute ratifications.

Batch B2 evaluation and Batch C publication controls retain their later approval gates. No Batch B or Batch C executable changes were made.

The assistant did not commit, push, switch branches, merge, publish images, or repin candidates.
