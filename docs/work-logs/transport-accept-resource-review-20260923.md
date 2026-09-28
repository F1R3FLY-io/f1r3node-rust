# Transport resource review after the bootstrap log incident

**Status:** Correction and regression verification in progress. Candidate resource qualification remains blocked.

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
