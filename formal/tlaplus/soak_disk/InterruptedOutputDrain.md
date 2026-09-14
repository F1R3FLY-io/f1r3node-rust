# Interrupted iteration output

B43 tests an owned native writer that retains the iteration output descriptor after detachment.
The fixture compares device and inode identities to confirm that the writer holds the production output FIFO.
It then confirms crash-monitor death through a process file descriptor.
The baseline driver stops its workload client but waits for output EOF before it stops the detached writer.
The writer continues and keeps the output FIFO open.

The correction moves the existing owned-writer stop before the output-drain wait on the interrupted path.
The unchanged fixture confirms owned-writer termination and unrelated-writer progress.
The driver retains one failure across two refused restarts.
The public counters remain `[1,1,0,0]` for iterations, failures, benchmark segments, and benchmark failures.

| Model action | Production or fixture boundary |
| --- | --- |
| `Init` | The interrupted workload client has exited while its detached writer holds the output FIFO. |
| `ChooseOrder` | The driver chooses the stop-before-drain or drain-before-stop ordering. |
| `Stop` | The selected successful owned-writer stop closes its remaining descriptor. |
| `Drain` | The output reader reaches EOF after the writer stops. |

The drain-first control violates `DrainRequiresOwnedStop` with TLC exit 12.
The corrected configuration passes with four distinct states.

The model assumes the selected native stop succeeds.
Failed stops, unowned descriptor holders, successful exits, inaccessible writers, and concurrent creation need separate verification.
This correction does not establish a complete output deadline or durable evidence publication.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.
