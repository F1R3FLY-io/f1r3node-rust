# Disk-protection continuation

## Session

- Started: `2026-09-16T02:54:36Z`.
- Implementer: `pi-session-disk-protection-01a050ce`.
- Machine: `spark-f718`.
- Pi session: `01a050ce-aacd-783b-8ee2-28d47bdd08b1`.
- Source: `802bc98d96594e2151afd49b31b8250c86573125`.
- Branch: `fix/soak-disk-hygiene-stop`.
- Status: `in_progress`.

## Assignment and limits

The user transferred D2/B44 from the other-machine agent to this session.
This transfer supersedes the [September 14 assignment](task-soak-disk-hygiene-stop-2026-09-08T05-06Z.md#d2b44-assignment-to-another-machine-2026-09-14).
This session also prepares D3 verification and acceptance prerequisites.
The transfer does not establish containment or authorize a specific infrastructure allocation.
No commit, push, cleanup, or gate bypass is part of this work.

The agent network returned no peers.
That observation does not establish exclusive remote activity or confirm that an earlier assignee stopped.
The earlier assignee's identity and any unpublished implementation remain unknown.
Remote activity must be checked before a diagnostic launch.
The branch integration owner retains shared gate registration, inventory, and publication coordination.

The initial working tree and index were clean.
The local host has a running Docker daemon and containerd.
This session did not query either daemon or use either daemon for a test.
The local host is not the disposable diagnostic runner.
The shared `/tmp/migrationPlan.md` file is absent.

## Protected inputs

The workload, protection settings, and 45-second finalization wait remain unchanged.
All three harness pins remain `962effd17708192627bd249362761c0ccb1fd5fa`.
The sibling harness HEAD is `27d847b1448591c49fb22c9970a943598510b9d2`, not the selected execution source.
Historical storage cleanup is complete and will not be repeated.

The external evidence directory is:

```text
/home/bf_spark/soak-evidence/f1r3node-rust/disk-protection-transfer-20260916-v0HA4srp
```

`source-before.json` identifies the immutable source snapshot.
`protected-before.sha256` binds the driver, launcher, report scripts, pin files, and inventory before this work.
The snapshot also preserves the plans and both reserve models before review.
A snapshot receipt does not establish that tests ran.

## Initial findings

The public contained launcher accepts only `native` mode.
The [private Docker specification](../plans/soak-private-docker-containment-2026-09-12.md) remains a design, not a production backend.
Earlier private-engine capability results do not cover the public launcher or permanent creation closure.
The new real-Docker behavioral baseline and PD-01 through PD-12 remain pending.

The D3 documents overstate the available bounds.
An observed maximum interval rate is not an enforced maximum growth rate.
A net filesystem delta can conceal allocations followed by reclamation within the interval.
The historical 18-to-23-second loss intervals do not guarantee time for a future upload.

The driver also needs further timing verification.
The guardian sleeps between loop bodies, so a five-second sleep is not a five-second sample-period bound.
`emergency_start` and `emergency_remaining` use `date +%s`, not the monotonic guardian-progress clock.
`summary_budget` has a five-second minimum even when the remaining response budget is zero.
`publication_budget` has a one-second minimum, and `session_bounded` adds a one-second termination grace.
These source findings are not executed behavioral RED results.

The driver copies failure evidence without a byte or inode reservation.
A timeout does not limit its allocation rate or total footprint.
The finite reserve models assume their growth and copy bounds.
Their results cannot supply those production bounds.

## Work sequence

1. Correct the reserve and D3 measurement requirements without changing runtime behavior.
2. Confirm shared-file coordination and the earlier assignee's remote activity.
3. Obtain approval for the bounded disposable diagnostic runner.
4. Verify source transfer, isolation, expiry, and independent teardown before workload admission.
5. Complete the request and mount inventory against the installed Docker and Compose identities.
6. Freeze the real-Docker public-driver baseline and observation fixture.
7. Establish the intended controller-loss RED before the corresponding production correction.
8. Complete unchanged-fixture runtime and formal comparisons for each selected behavior.
9. Complete O1 prerequisites before the D3 node diagnostic.
10. Derive enforceable writer limits and verify the measured lifecycle correction.
11. Integrate current-source evidence through the shared gates without promoting pending claims.
12. Request the exact-candidate acceptance run only after all prerequisites pass.

## Proposed first diagnostic allocation

This request covers containment verification with harmless writers, not a node soak or acceptance.
No allocation has been launched or approved by this record.

| Item | Proposed limit or requirement |
| --- | --- |
| Runner count | One exclusive disposable OCI virtual machine. |
| Platform | Linux amd64, cgroup v2, and a supported systemd manager. |
| Host resources | At most 32 virtual CPUs, 64 GiB RAM, and an 80 GiB boot volume. |
| Lifetime | At most two hours from creation, including setup and retrieval. |
| Workload | Bounded harmless writers and the real public driver with declared external workload substitution. |
| Private-engine diagnostic limits | At most 1 GiB memory and 256 tasks for the initial capability case. |
| Individual writer | At most 64 MiB memory and 32 tasks. |
| Storage | A private runtime store with a verified limit before writer admission. No intentional host exhaustion. |
| Observation | A trusted observer outside the workload domain records termination before fixture cleanup. |
| Teardown | Independent cloud teardown and expiry controls survive loss of the diagnostic controllers. |
| Retrieval | Verify archive identities and source bindings before identity-checked instance destruction. |
| Exclusions | No shared-daemon test, live node soak, acceptance run, or host security change. |

The concrete runner identity, region, image, resource configuration, and absolute expiry must be recorded before execution.
An unavailable limit or isolation feature produces a setup refusal, not a fallback to the developer host.
Required compatibility cases can need different reviewed limits.
This initial allocation does not establish an acceptance resource budget.

## Open integration and acceptance requirements

The two report-script hashes in `docs/claims/soak-claim-inventory.jsonc` are stale at the starting source.
The reporting evidence in `report-commit-review-VZSFj5pI/reporting-inputs.json` binds the formatted committed scripts.
Documentation edits from this session also require reviewed inventory updates where those documents are candidate inputs.
Digest updates do not rerun tests or discharge claims.

The four telemetry models and two reserve models still require shared gate integration.
O1, hosted enforcement, claim ratification, and final-source combined checks remain open.
A1 also requires the finalization prerequisites specified in the recurrence plan.
An early protective stop is not successful full-duration acceptance.
The exact-candidate 60-hour acceptance run must retain separate disk and finalization verdicts.

## Progress

- [x] Record the user-directed ownership transfer and source identity.
- [x] Review the existing containment contract and source timing functions.
- [x] Correct the D3 and reserve documentation.
- [x] Verify the documentation changes and current finite reserve configurations.
- [ ] Confirm the first diagnostic allocation and remote exclusivity.
- [ ] Complete private Docker production containment and its verification.
- [ ] Complete D3 attribution and the production lifecycle correction.
- [ ] Complete the acceptance prerequisites and authorized acceptance execution.

## Verification results

The four existing reserve configurations ran sequentially with private TLC state directories.
The inputs came from the immutable 25-file snapshot, not a changing source tree.
Each execution had a 512 MiB Java heap, two workers, and a 120-second outer timeout with five-second termination grace.
The model and configuration bytes remained unchanged.
No classifier substitute ran.

| Configuration | Observed exit | Result |
| --- | --- | --- |
| `MC_ReserveBound` | 0 | The finite safety checks passed. |
| `MC_ReserveBound_unbounded_pre_fix` | 12 | Only `OperatingReserveHeld` supplied the expected invariant failure. |
| `MC_RetentionReserve` | 0 | The finite safety checks passed. |
| `MC_RetentionReserve_unskipped_pre_fix` | 12 | Only `ReserveHeld` supplied the expected invariant failure. |

`reserve-model-checks/results.json` records commands, tool digest, source receipt, elapsed times, and expected invariant identities.
These runs do not establish production containment, wall-clock bounds, copy termination, or acceptance.
The selected configurations check safety, not the unconfigured `AllCopiesResolved` property.

The scoped STE Check and whitespace check passed.
The Markdown language-server check reported seven clean files at warning severity.
Human STE Review remains necessary.
The repository package validator passed for ten packages.

The claim inventory still fails and was not edited.
`integration-bindings.json` identifies five stale inputs: three changed coordination documents and the two previously repaired reporting scripts.
The formatted reporting scripts still match their retained execution bindings.
The updated D3 methodology and reserve argument are not currently candidate inputs in that inventory.
The integration owner must review that coverage as well as the stale digests.

Protected input hashes match the starting source, including the driver, launcher, reporting scripts, pin files, and inventory.
The Git index remains unchanged.
No runtime correction, private-Docker behavioral test, privileged diagnostic, or acceptance soak ran.
No commit or push was made.

## Current blocker

The proposed disposable allocation still needs approval and verified remote exclusivity.
The previous assignee's identity, activity, and unpublished work remain unverified.
This session must not substitute the local shared Docker daemon for the guarded runner.
D2 implementation follows the real-Docker baseline and the implementation gates in the private Docker specification.
