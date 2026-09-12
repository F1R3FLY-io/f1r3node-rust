# Durable record publication

B51 tests how the driver publishes its minimal records.
The records are the guardian breach record, the protection breach record, the early-exit record, the persisted state, the summary text, and the summary JSON.
The fixture runs the production driver in Docker isolation as uid 65534.
The disk probe reports free space below the hard floor after the workload starts, so the guardian fires during the first iteration.
A substituted `sync` command logs each call with the target name, the file size, and the content digest.
A poller reads each record every ten milliseconds and logs every size change.

The baseline driver wrote each record in place with a shell redirection or `tee`.
A reader could observe an empty or partial record under its final name, and no call synced the content or the directory.
The matched RED exits 1 because no record had a synced temporary file, a rename, and a directory sync.

The corrected driver publishes every record through one helper.
The helper writes the content to a temporary name beside the target and syncs the temporary file.
It then renames the file into place and syncs the directory.
The summary writer follows the same sequence for the summary JSON.
The matched GREEN exits 0.
Every record shows its final digest in a file sync under its temporary name, followed by a directory sync, and no temporary file remains.

## Correspondence

| Model element | Production or fixture boundary |
| --- | --- |
| `WritePartial`, `WriteFull` | The helper writes the content to the temporary name. The baseline writes it under the final name. |
| `SyncFile` | The helper syncs the temporary file. The fixture logs the call with the content digest. |
| `Rename` | The helper renames the temporary file into place. The visible record is complete and synced. |
| `SyncDir` | The helper syncs the directory. The fixture logs the call after the file sync. |
| `Crash` | Power loss or a killed driver. Unsynced content may be lost. |
| `VisibleImpliesDurable` | A record under its final name is complete and synced, or absent. |
| `NoTemporaryAfterPublish` | No temporary file remains after the directory sync. |
| `Settles` | The publication completes or the process crashes. |

The in-place configuration violates `VisibleImpliesDurable` with TLC exit 12.
The atomic configuration passes with 10 distinct states.

## Limits

The substituted `sync` command records ordering and content and does not prove kernel durability.
The claim is atomic visibility and the fsync ordering that the platform durability contract requires.
The poller log is retained as evidence and is not a verdict, because a partial observation depends on timing.
The model has one record and one publication and does not represent concurrent writers of the same record.

Storage faults during publication, upload acknowledgment, and the guardian's own stop bound are not verified here.
No native service manager, real Docker daemon, or disposable runner executed this cycle.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.
