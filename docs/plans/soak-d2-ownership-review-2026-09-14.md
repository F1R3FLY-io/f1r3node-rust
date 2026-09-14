# D2 ownership review: memory protection and launch paths

## Scope and source identity

This review corrects the ownership conclusions in commit `2c1bc5241bd8ac2df57932f68977602e7ad156eb`.
It distinguishes source configuration, historical observations, and unverified runtime behavior.
It does not establish complete containment or runner survival.
No new memory-exhaustion test or formal verification ran for this review.

The reviewed node source is commit `2c1bc5241bd8ac2df57932f68977602e7ad156eb`.
The reviewed harness source is revision `962effd17708192627bd249362761c0ccb1fd5fa`.
The source digests are:

| Source | SHA-256 |
| --- | --- |
| `scripts/run-merge-recovery-soak.sh` | `6c3fb7471dd823ef0c657c25803b6d0f44a96faec1af91dcc55b291f95cbef5d` |
| `scripts/bench/soak-containment.py` | `2641d0ac90ee8d837e7849038a3ef1f8f66c64fd4a035f5f823a6c1f90287ea7` |

## Out-of-memory preference is not survival protection

An out-of-memory (OOM) preference changes the kernel's victim selection.
An `oom_score_adj` value does not reserve memory or protect another process from termination.
The [Linux interface description](https://man7.org/linux/man-pages/man5/proc_pid_oom_score_adj.5.html) explains the score adjustment and the applicable memory domain.
A workload outside that domain cannot protect a runner inside it through its preference value.

A control group can restrict the eligible victims during local memory exhaustion.
The [control-group documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html#memory) also describes `memory.oom.group`, which can cause a group kill.
Repeated exhaustion can require additional victims.
A configured workload preference therefore does not establish runner survival, successful shutdown, or evidence retention.
Legacy driver comments that suggest a survival guarantee are not verified claims.

## Selection metadata and configuration

The driver creates a fresh `SOAK_WRITER_OWNER` value for each driver invocation.
It passes the value through labels and process environments on selected launch routes.
These values identify cooperative workload membership, not authenticated ownership or an exclusive containment boundary.

| Mechanism | Source behavior | Limit |
| --- | --- | --- |
| `SOAK_PROCESS_OWNER` | The iteration and benchmark launch environments carry the owner value. | Descendant selection depends on readable, retained metadata and the expected user identity. |
| Host-process preference | The guardian writes `1000` to selected `oom_score_adj` files. | Selection is periodic, and failed writes produce a warning rather than verified preference application. |
| Docker creation preference | Supported `run` and `create` routes receive `--oom-score-adj 1000` when enabled. | The wrapper enables this setting only when `HOST_FREE_FLOOR_MB > 0`. |
| Compose creation preference | Supported `up` and `create` routes receive a service override when enabled. | Configuration injection is not proof of every resulting process value. |
| Stop selection | Host processes use the owner marker, and Docker containers use the owner label. | Stop requests and selection metadata do not establish permanent creation closure. |
| Health tags | `guardian_stamp_health_tag` attempts an external metadata update. | Missing tools, failed requests, and failed updates can return without a retained tag. |

The [driver source][driver] contains the wrapper at lines 1350–1406 and the host preference helper at lines 1488–1527.
Its stop helpers are at lines 424–514, and its health-tag helper is at lines 1528–1552.
Health tags remain best-effort observations, not guaranteed durable last words.

## Host-process and subprocess coverage

The original review omitted `guardian_mark_workload_oom_preferred`.
The helper selects same-user processes with the exact inherited owner marker.
It reads the environment and writes the preference through the same opened process directory.
It does not restrict selection to node executables.
A marked pytest process or another marked descendant can also match the selector.

The helper runs with host-memory protection enabled and an available memory sample.
The loop sleeps before sampling and calls the helper when `sample_n % 3 == 1`.
This mechanism does not establish preference application before a new process allocates memory.
Scheduling, inaccessible metadata, process replacement, and write failures remain relevant.

The [pinned subprocess provider][subprocess] copies the parent environment when it starts nodes at lines 525–539 and 1007–1019.
Its restart path reuses the saved environment at lines 261–272.
The first launch path can override environment entries through `extra_env`.
Source inspection therefore supports conditional marker propagation, not observed preference coverage for every node or restart.

The [B34 evidence](../cbc-evidence/soak-d2-oom-ownership-2026-09-11/README.md) records selected host-process preference writes and preservation of unrelated preferences.
Its restricted fixture used healthy memory samples and did not cause memory exhaustion.
The [B34 model limits](../../formal/tlaplus/soak_disk/HostOomOwnership.md) exclude general race, permission, and scheduling guarantees.
This historical result does not establish complete coverage of the current pinned provider.

## Docker and Compose coverage

The wrapper requests creation-time preferences only on supported routes with host-memory protection enabled.
A later Docker caller option can override the injected preference.
Absolute executable paths, direct APIs, unsupported options, existing containers, and concurrent creation need separate review.
Labels do not prevent forged metadata or access through another Docker client.

The [B36 evidence](../cbc-evidence/soak-d2-container-preference-2026-09-11/README.md) records selected real-Docker `run` and Compose `up` cases.
The cases observed workload preference `1000` and preserved unrelated preference `0` without memory exhaustion.
Separate default-configuration cases left creation preference at `0` when host-memory protection was disabled.
The [B36 contract](../../formal/tlaplus/soak_disk/DockerOomOwnership.md) excludes conflicting options and untested creation variants.
These results do not establish complete Docker ownership, restart coverage, or runner survival.

## Native launcher coverage

The [native launcher][launcher] wraps driver execution rather than defining another harness provider.
It requests a service with `MemoryMax=256M` and selected containment properties at lines 101–111.
It does not explicitly set `OOMScoreAdjust` or verify that property through `unit_properties`.
The launch gate also does not write a process preference before it executes the driver.

The driver can still apply its periodic host-process preference after release when its configured conditions hold.
The absence of a launcher setting does not establish the effective score from the service manager or inherited configuration.
Actual service properties, process scores, memory-domain placement, and group-kill behavior need a guarded runtime check.
The diagnostic service limit is not an acceptance-workload memory budget.

## Incident attribution

The [post-mortem for run 30713818751](../work-logs/soak-oci-image-defects-20260801.md#root-causes-run-30713818751-2026-08-01-weekend-soak) records pytest termination with `oom_score_adj 500`.
It states that Docker-provider containers continued after pytest exited.
It does not establish a subprocess-provider preference defect or current node scores.
This review reads the post-mortem, not a newly retrieved kernel trace.
The adjustment value `500` must not be reported as the complete kernel victim score.

## Remaining obligations

These reviewed routes do not form an exhaustive inventory of workload creation.
Storage cleanup is complete, but the production containment work remains open.
The following checks require coordination with the D2 owner and the applicable diagnostic authorization:

1. Observe effective preferences and process identities at launch, restart, and the first memory allocation boundary.
2. Verify marker propagation, harness-process selection, permission failures, metadata loss, and delayed guardian execution.
3. Test conflicting Docker options, alternate clients, creation variants, and accepted or deferred creation during shutdown.
4. Verify native service properties, actual memory-domain placement, and termination under the declared failure conditions.
5. Verify preservation of unrelated processes.
6. Verify permanent creation closure.
7. Confirm termination through an independent observer.
8. Establish resource bounds without treating a preference value as a survival guarantee.
9. Verify evidence retention under the declared failure conditions.

Observed defects still require production counterexamples, applicable formal counterexamples, corrections, and unchanged-fixture verification.
This review changes no runtime code, harness pin, workload, finalization wait, gate result, or claim status.
D2, D3, and claim discharge remain pending.

[driver]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/2c1bc5241bd8ac2df57932f68977602e7ad156eb/scripts/run-merge-recovery-soak.sh
[launcher]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/2c1bc5241bd8ac2df57932f68977602e7ad156eb/scripts/bench/soak-containment.py
[subprocess]: https://github.com/F1R3FLY-io/system-integration/blob/962effd17708192627bd249362761c0ccb1fd5fa/integration-tests/test/infra/providers/subprocess.py
