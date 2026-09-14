# Run-domain record identity

B46 tests the trust decision of the run-domain record check after the B45 review found two defects.
The B45 check inspected the record by pathname and then opened that pathname separately.
An ancestor outside the checked parent could change between those operations.
The B45 check also read at most 65,536 bytes and did not detect trailing content beyond that bound.

The fixture runs the production driver in Docker isolation as uid 65534 with `SOAK_CONTAINMENT=required`.
Root builds four records before it drops privilege.
The untrusted-ancestor record sits below a directory that uid 65534 owns.
The symlink-ancestor record is reached through a symbolic link component.
The oversized record holds a valid object padded to 65,536 bytes and then trailing content.
The matching record sits below a root-owned chain from the filesystem root.

The baseline driver admits work through the untrusted-ancestor record because its immediate parent belongs to root.
That matched RED exits 1.
The corrected driver opens each path component from the filesystem root with a directory descriptor and no symbolic link following.
It requires every directory and the record to belong to uid 0 without group or other write permission.
It reads one byte past the bound and rejects a longer record.
It also requires a non-empty `unit` string without a path separator.

The corrected driver refuses all three untrusted records with the breach record and counters `[0,1,0,0]`.
The matching record admits exactly one iteration.
The unrelated native writer continues in every case.
The B45 source comments are removed in the same change.

## Correspondence

| Model element | Production or fixture boundary |
| --- | --- |
| `checkedTrusted` | The pathname view that the B45 check inspected. |
| `openedTrusted` | The file the driver actually reads, reached through descriptors from the root. |
| `recordComplete` | The record ends within its size bound. |
| `Admission` with `BindIdentity = TRUE` | The corrected driver admits only when the opened record is trusted and complete. |
| `Admission` with `BindIdentity = FALSE` | The baseline driver admits on the pathname view alone. |
| `AdmissionRequiresOpenedRecordTrust` | No admission without an opened, trusted, complete record. |
| `TrustedRecordAdmits` | A trusted and complete record is never refused. |
| `Completes` | Each case reaches admission or refusal. |

The pathname configuration violates `AdmissionRequiresOpenedRecordTrust` with TLC exit 12.
The corrected configuration passes with 16 distinct states.

## Limits

The fixture builds each untrusted chain before the driver starts.
It does not race a concurrent rename between the check and the read, because the corrected driver no longer has two operations to race.
The `unit` check is a shape check, not a live service invocation check.

Root ownership, record parsing, invocation freshness, exclusive ownership, and creation fencing remain distinct claims.
No native service manager, real Docker daemon, or disposable runner executed this cycle.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.
