# Run-domain record identity checks

D2 remains incomplete.
This record retains one matched local repair for B46.
The workload, finalization semantics, and 45-second finalization limit remain unchanged.
No node workload, hosted job, diagnostic virtual machine, or acceptance soak ran for this cycle.

| Cycle | Selected behavior | Production and formal results |
| --- | --- | --- |
| B46 | Trust only a run-domain record opened through a root-owned, unwritable directory chain, and reject a record beyond its size bound. | Matched RED and unchanged-fixture GREEN. The pathname control fails with exit 12. |

The B45 review by the second session found that the record check inspected a pathname and then opened it separately.
It also found that the check read at most 65,536 bytes without detecting trailing content.
The fixture builds four records as root under `/run` and then drops to uid 65534 before any workload code runs.
The untrusted-ancestor record sits below a directory that uid 65534 owns.
The symlink-ancestor record is reached through a symbolic link component.
The oversized record holds a valid object padded to the bound and then trailing content.

The baseline driver admits work through the untrusted-ancestor record, and the matched RED exits 1 on that admission.
The corrected driver opens each path component from the filesystem root with a directory descriptor and no symbolic link following.
All three untrusted records refuse work with the breach record and counters `[0,1,0,0]`.
The matching record admits one iteration, and the unrelated native writer continues.
One earlier attempt failed setup before any behavioral assertion and is retained with its original identity.
The [model note](../../../formal/tlaplus/soak_disk/RunDomainRecordIdentity.md) defines the correspondence table.

The combined checks run against the corrected source.
The soak PR tier of the formal gate, the classifier and routing regressions, the driver suite, the supporting checks, and the isolated emergency suite pass.
The emergency suite ran alone after the other checks completed.
The second session changed the native launcher during this cycle.
No check in this cycle executes the launcher, and the manifest lists both launcher files as drift-exempt with their snapshot digests.
Rerun outputs remain digest-only in the raw archive.

The `unit` field check is a shape check, not a live service invocation check.
Invocation freshness, exclusive ownership, and creation fencing remain distinct claims.
The B46 runtime fixtures do not execute the native launcher, and the normal workflow does not require containment.
Private Docker containment, storage durability, deadline and reserve bounds, hosted enforcement, maintainer ratification, and acceptance remain pending.
