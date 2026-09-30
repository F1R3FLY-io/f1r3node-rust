# Transport resource review after the bootstrap log incident

**Status:** Hosted transport regression verification passed. Strict TASK-020-1 closure remains blocked on missing user-flow linkage. Candidate resource qualification remains separate.

Earlier sections retain their original results and limitations. The final section records the hosted verification checkpoint.

## Review gap

The user identified a missing operational boundary in the CbC harness review.
The live lifecycle tests passed, but those tests did not inject descriptor exhaustion or establish a log storage bound.
The existing [disk protection claim](../claims/soak-disk-protection.md) remains proposed and unratified.
Its storage model assumes log caps that the node does not enforce.
The transport accept loop has no CbC artifact registration.

## Observed failure

The bootstrap container wrote approximately 1.2 TB to its log for 2026-09-23.
The sampled error was `TCP listener accept failed` with `Too many open files (os error 24)`.
The user approved stopping the container. Its volume remained intact.
The deployed container uses version `0.4.23`.
The current branch and `dev` also contain the accept loop without a retry delay.

## Correction scope

1. Require a one-second delay before retrying an accept error.
2. Preserve the error delivered to the incoming stream consumer.
3. Stop the listener when its consumer closes or drops the incoming stream.
4. Cancel unfinished TLS handshakes when the listener task ends.
5. Test recovery after the process releases exhausted descriptors.

The regression must call the production accept loop.
A finite injected error stream will test retry spacing without consuming descriptors.
A separate Linux child process will reproduce `EMFILE` with a 64-descriptor limit.
The child will hold at most 128 null-device descriptors and count log bytes without writing a log file.
The parent will terminate the child if it exceeds ten seconds.
No test changes the parent process's descriptor limit or starts Docker containers.

## CbC limits and remaining obligations

The retry rule limits this error path to one event per second after the first event.
That rule assumes Tokio timer progress and preserves the error outcome.
Task cancellation requires executor progress before sockets close.

The initial cause of descriptor exhaustion remains unknown.
Concurrent handshakes have a timeout but no explicit count limit.
Time-based log rotation does not establish a byte limit.
The harness still needs source-bound fault evidence, enforced log limits, and deployment supervision evidence before resource qualification.
Passing regression tests will not discharge the proposed disk protection claim or establish complete CbC coverage.

## Verification

The failing control and corrected results will be recorded here after execution.
No commit, push, PR change, or merge is authorized by this work.

### Correction on 2026-09-30

`claude-session-f3cbc961` made the correction for TASK-020-1 on `fix/node-log-and-accept-backoff` at `6d4c4a1bd`.
The maintainer confirmed the file scope and the method before the change.
The change is in `comm/src/rust/transport/f1r3fly_server.rs` only. The regression tests did not change.

| Correction scope item | Implementation |
| --- | --- |
| Delay before a retry | The delay starts at 10 ms and doubles to a maximum of one second. A successful accept sets it back to 10 ms. |
| Error delivered to the consumer | The listener sends the error first, and then it waits. |
| Stop when the consumer closes or drops the stream | The accept wait and the retry wait end when the channel closes. |
| Cancel unfinished handshakes | The listener task owns the handshakes. It releases the listener socket first, and then it cancels the handshakes. |
| Log limit | The listener writes at most one `ERROR` line in each second. It writes one `WARN` summary in each minute with the field `suppressed_errors`. |

The review text above has a fixed delay of one second. The task record and the tests require the exponential delay, which this correction uses.

### Failing control

| Run | Result |
| --- | --- |
| Hosted runs of PR #451 from 2026-09-24 to 2026-09-29 | `Test (comm)` failed, with four reported failures before the run stopped |
| Local run on macOS before the correction | 6 of 6 portable tests failed |

### Corrected results

| Check | Result |
| --- | --- |
| The six portable regression tests, one process for each test | 6 passed |
| The complete `comm` crate, one process for each test | 399 passed |
| Clippy on the `comm` crate, all targets, warnings denied | Passed |
| Rust format | Passed |
| The Linux descriptor test | Not run. The local container storage had no space for the build. The hosted run is the first result. |

The hosted checks use one process for each test. The local results use the same method.

### Limits

- The timer of the runtime rounds a deadline up to the next millisecond. The listener requests the delay minus 999,999 ns, so the wait does not exceed the nominal delay. The actual wait is between the nominal delay minus one millisecond and the nominal delay.
- One regression test pauses the clock after real socket operations. The exact times of that test need the compensation above.
- The log test uses a subscriber for one thread. With all tests in one process and parallel threads, that test fails intermittently. The cause is the shared callsite cache of the tracing library, not the listener.
- The summary line is written when an accept error occurs after the minute ends. A suppressed count stays unreported if the errors stop before that time.
- The correction does not limit the number of concurrent handshakes. It does not find the initial cause of the descriptor exhaustion.

## Hosted verification on 2026-09-30

Verification session: `01a0ab62-71b3-7248-a800-37a6fde2e4fa`. The verification claim time is `2026-09-30T01:03:01Z`.

The verification fields are separate from implementation ownership. `claimed_by` remains `claude-session-f3cbc961`. `verification_status` remains `in_progress` as requested.

The user requested verification and closure of TASK-020-1 if its acceptance checks pass. The reviewed branch head is `affbebc6eaa5cac9678fcbded9182423fda0fb67`.

[CI run 36651370411](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/36651370411), attempt 1, ran the `Test (comm)` job on Linux.

The [job](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/36651370411/job/109688126217) completed successfully. All 400 tests passed, with zero skipped tests.

All seven transport resource regressions passed. The real descriptor-exhaustion test completed in 0.679 seconds according to nextest.

The test child reaches its 64-descriptor limit and exercises the production accept loop. The reviewed assertions bound retries and log bytes before checking recovery.

The test requires between one and six accept errors during its 500 ms fault window. It requires positive log output below 1,024 bytes.

The child releases its held descriptors and requires the next connection to reach handshake timeout within two seconds. Its accept-error count must not increase.

Successful nextest output does not expose the child metric line. These values are assertion bounds, not separately observed descriptor or byte counts.

The other tests verify exponential delay, reset after success, bounded error logging, suppressed-count summaries, consumer closure, and stalled-handshake cleanup.

The hosted test command returned `nextest exit=0 doctest exit=0`. No doctest was present. The separate hosted lint job also passed.

### Source and execution identity

The test job checked out synthetic merge commit `b21659ec1ea526b97227157d6cae790310f6013f`, not the PR head directly.

Its second parent is the reviewed head. Its first parent is `aaabb6710ef8136713a9959c4b1929835f30d17f`.

Nine relevant source and build inputs match the GitHub tree at that tested commit. The API response was complete, and checkout logs confirm the tested identity.

The [evidence report](./evidence/task-020-1-hosted-20260930-01/report.json) retains job identities, assertion limits, source hashes, and sanitized result lines.

Raw logs and API responses remain under `target/task-020-1-hosted-36651370411/`. No raw runner log or binary was published.

Overall CI was not complete at the recorded checkpoint. Success of this job does not establish success of other jobs or merge readiness.

### Strict completion refusal

The unchanged completion helper ran against a tracker copy with strict mode enabled and force disabled. It refused with exit 3 and `no_flow_link`.

EPIC-020 has no `user_flow` field. The test link was supplied, but that link cannot substitute for the missing flow relationship.

The [refusal record](./evidence/task-020-1-hosted-20260930-01/strict-completion-refusal.txt) preserves the integrity result. No completion status or completion date was applied.

The live tracker now links the actual regression file and hosted evidence. TASK-020-1 remains in progress until a real flow relationship permits strict completion.

This review does not waive that gap or create a placeholder flow. It changes no production source, test, workflow, artifact attribute, or adjacent task.

The maintainer decision to leave the accept path untagged remains unchanged. Transport test success does not discharge disk-protection claims or finish TASK-020-2 through TASK-020-4.

No commit, push, merge, campaign, or unrelated closure occurred.
