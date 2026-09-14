# Run-domain admission checks

D2 remains incomplete.
This record retains one matched local repair for B45.
The workload, finalization semantics, and 45-second finalization limit remain unchanged.
No node workload, hosted job, diagnostic virtual machine, or acceptance soak ran for this cycle.

| Cycle | Selected behavior | Production and formal results |
| --- | --- | --- |
| B45 | Refuse benchmark and iteration admission when containment is required and no trusted run-domain record matches the driver's placement. | Matched RED and unchanged-fixture GREEN in both modes. Both unchecked controls fail with exit 12. |

The fixture runs the production driver in a restricted container as uid 65534.
The container has no host mounts, no network, no Docker socket, and only the `SETUID` and `SETGID` capabilities.
Root writes the trusted records under `/run` and then drops privilege before any workload code runs.
Docker, disk probe, workload, and cloud commands inside the fixture are substitutes.
The unrelated native writer continues in every case.

The absent and mismatched records refuse work, write the breach record, and retain counters `[0,1,0,0]` across two refused restarts.
The matching record admits exactly one unit of work in each mode.
The [model note](../../../formal/tlaplus/soak_disk/RunDomainAdmission.md) defines the record and its correspondence table.

Two earlier fixture attempts failed setup before any behavioral assertion and are retained with their original identities.
A third matched pair with a prior fixture revision is also retained.
The final matched pair uses identical fixture bytes for RED and GREEN.
The manifest binds the baseline and corrected runtime sources, the fixture, the fixture image, and the TLC jar.

The combined checks run against the corrected source.
The soak PR tier of the formal gate, the classifier and routing regressions, the driver suite, the supporting checks, and the isolated emergency suite pass.
The first emergency attempt failed its timing-based stop-deadline case while the machine ran other work at load 30.
That attempt is retained as a load-contention failure, and the retry alone passes.
Rerun outputs remain digest-only in the raw archive.

The fixture container shares one cgroup between the driver, the owned workload, and the unrelated writer.
The matching case therefore tests record comparison, not exclusive run-domain ownership.
The native launcher does not write the record yet, and the normal workflow does not require containment.
Creation fencing, private Docker containment, storage durability, deadline and reserve bounds, hosted enforcement, maintainer ratification, and acceptance remain pending.
