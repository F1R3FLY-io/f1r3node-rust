# Selected shutdown checks

D2 remains incomplete.
This record contains three selected repairs and an unresolved controller-loss counterexample.
The workload, finalization semantics, and 45-second finalization limit remain unchanged.
No node workload, hosted job, diagnostic virtual machine, or acceptance soak ran for these cases.

| Cycle | Selected behavior | Production and formal results |
| --- | --- | --- |
| B41 | Detect monitor death during an active benchmark. | Matched RED and unchanged-fixture GREEN. |
| B42 | Refuse opening benchmark and iteration admission after monitor death during a valid probe. | Both matched RED/GREEN pairs pass. |
| B43 | Stop the owned output descriptor holder before the interrupted iteration waits for output completion. | Matched RED and unchanged-fixture GREEN. |
| B44 | Stop the owned writer after both controllers die. | Production RED and formal RED. No correction or GREEN result. |

The tests use real native processes and files in restricted containers.
The containers have private process namespaces, UID 65534, no host mounts, no network, and no Docker socket.
Docker commands inside each fixture are substitutes.
Fixture cleanup does not count as production termination evidence.
Each selected production observation preserves an unrelated native writer.

B44 requires an ownership and containment design that survives the specified controller failures.
The final diagnostic permits 120 observation intervals before it checks writer progress again.
The owned writer grows from 492 to 512 bytes, and the unrelated writer grows from 532 to 552 bytes.
The model checks a later observation rather than requiring an instantaneous stop at driver death.
The current configuration violates `ControllerLossStopsOwnedWriter` with TLC exit 12.
The passing suite does not register B44 as an accepted negative control.

The original B44 fixtures and models remain retained with their source identities.
The revised fixture does not require a killed driver to publish an initial summary.
It checks recovery through two actual restarts after confirmed writer termination.
The three revised B44 diagnostic files were not executed by the combined passing suite.
The manifest distinguishes the combined snapshot from the current diagnostic inputs.
The production driver and passing-gate inputs remain byte-identical across that distinction.

The B41-only and combined runs have separate source snapshots and result records.
The manifest records their configuration, classifier, routing, and emergency case counts.
The actual TLC logs were saved before the classifier and routing substitutes ran.
Source snapshots identify bytes, not execution coverage.
Another actor created commit `cdc0a57f5` during verification.
This session did not run or attest the commit hooks.

## Retention

Reruns remain digest-only.
The raw archive is `[EVIDENCE_ROOT]/raw-streams.tar.gz`.
The manifest records every archived regular file with its path, SHA-256 digest, and byte count.
It also records original and published stream identities and ordered replacement counts.
No package-local ignore file overrides repository rules.

Unused chart build artifacts remain in the original external B41 snapshot.
Special filesystem entries remain outside the regular-file archive and have explicit exclusion records.
The archive is not a complete filesystem image.
Historical packages and manifests remain unchanged.

```bash
ruby scripts/ci/check-soak-evidence-package.rb docs/cbc-evidence/soak-d2-shutdown-2026-09-11
ruby scripts/ci/check-soak-evidence-package.rb docs/cbc-evidence/soak-d2-shutdown-2026-09-11 \
  --raw-archive /path/to/raw-streams.tar.gz
```

## Limits

B41–B43 do not establish complete discovery, late-creation fencing, recovery from failed stops, storage durability, or an aggregate deadline.
Other descriptor holders, inaccessible writers, monitor progress faults, and remaining launch and shutdown paths require separate evidence.
The new cases do not revalidate the real Docker daemon or establish reserve bounds.
Construction is not applicable to these Bash-driver models.
D2, all claim discharges, hosted enforcement, maintainer review, and acceptance remain pending.

See [B41 correspondence](../../../formal/tlaplus/soak_disk/BenchmarkMonitorDeath.md), [B42 correspondence](../../../formal/tlaplus/soak_disk/MonitorAdmission.md), and [B43 correspondence](../../../formal/tlaplus/soak_disk/InterruptedOutputDrain.md).
The [B44 record](../../../formal/tlaplus/soak_disk/ControllerLoss.md) describes the open containment obligation.
