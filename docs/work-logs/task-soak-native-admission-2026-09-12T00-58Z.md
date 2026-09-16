# D2 native admission coordination

## Session

- Implementer: `pi-session-native-admission`.
- Observed base: `1c2679e0643537a7fda2735943e17a37686115b0`.
- Coordination time: `2026-09-12T00:58:00Z`.
- Status: `in_progress`.
- Parent work: [D2 shutdown](task-soak-disk-hygiene-stop-2026-09-08T05-06Z.md).

The user directed both agents to work in parallel through work logs and ToDos.
The agent network is not required for this collaboration.
The TDD plan assigns B45 to `claude-session-e3a67b91`.
That claim covers the driver's required run-domain record and placement check.
This session will not edit the B45 fixture, production driver, TDD plan, formal gate scripts, or shared claim inventory.

## Claimed scope

This session owns `scripts/bench/test-soak-native-admission.py` and this work log.
The fixture tests workload refusal when the external service-status query is unavailable.
The existing native launcher starts the driver before that query completes.
The new fixture has syntax and unprivileged-runner refusal checks only.
No behavioral RED or GREEN exists for this fixture yet.
No new runner was provisioned by this session.

The proposed implementation scope is `scripts/bench/soak-containment.py` and separate `NativeLaunchAdmission` formal files.
This session will wait for a work-log response before changing that shared launcher or provisioning infrastructure.
The other agent can retain the B45 driver change, shared integration files, and publication work.
B45 record validation and this launch barrier are different obligations.
Neither obligation establishes complete Docker containment or a creation fence.

## Message to claude-session-e3a67b91

Please record whether you own launcher changes or diagnostic infrastructure in your work log.
Please identify any concurrent TLC run before either agent uses shared `/tmp/tlc-*` output names.
This session can use separate model output paths and frozen source snapshots.
Please preserve the new Python fixture while you work on `test-soak-run-domain-admission.sh`.
The fixture has not been registered in a passing gate.

## Independent B45 fixture review

The following findings refer to the shell fixture observed at `2026-09-12T00:56Z`.
They are source-review findings, not observed behavioral RED results.

1. The fixture drops all capabilities but adds only `SETUID` and `SETGID`.
   Its first `chown 65534:65534` needs `CHOWN` and precedes the setup-error trap.
   This can produce exit 1 before a behavioral assertion.
   Keep setup failures distinct from the exact admission failure.

2. The transfer uses `tar --mode='a+rX'`.
   This does not add execute permission to shell files that currently have no execute bits.
   Verify normal benchmark and summary execution from the transferred source.
   The previous native subcycle needed a fresh matched pair after a copied-shell permission defect.

3. A live driver after ten seconds currently triggers a behavioral failure after fixture intervention.
   Check retained admission output before classifying this result as the observed admission defect.
   A timeout without an admitted workload is not behavioral RED.

The matching-record case tests record comparison against placement.
It does not establish exclusive run-domain ownership because the unrelated writer shares the fixture cgroup.
This limit must remain explicit in the eventual evidence.

## Native launch barrier review

The launcher must verify containment before it releases the production driver.
A caller-controlled environment flag or an ordinary writable marker is not sufficient authorization.
An unprivileged gate must not accept release through a descriptor that another process with the same user identifier can write.
A trusted gate must drop privilege before it executes any workload code.
Python environment customization must not execute before verification.
These are design constraints, not verified implementation properties.

The missing-cgroup-file success path, early cleanup without invocation identity, successful exits, and observer loss remain separate tests.
The normal workflow must not use the prototype before the full approved containment contract passes.
D2, the D3-dependent reserve argument, claim discharge, hosted enforcement, and acceptance remain pending.

## Verification

The Python fixture's syntax check passes.
Its ordinary-user invocation exits 2 before any privileged operation.
The initial two auxiliary diagnostics concerned the outer exception handler and strict boolean identity guard.

The revised fixture makes parsing failure handling explicit and requires a boolean value before testing for false.
This preserves rejection of numeric zero.
The latest source and work-log probes report no findings or unconfirmed paths.
The fixture still requires a guarded disposable runner for behavioral execution.

The preliminary local check directory is `/tmp/soak-d2-admission-review.bJHG3Rgr`.
The later capability and archive probes are in `/tmp/soak-d2-collaboration-review.nLtZKKtX`.
The isolated capability probe confirms that `SETUID` and `SETGID` alone cannot change file ownership.
The archive probe confirms that `a+rX` preserves mode `0644` for a non-executable shell file.
These are environment checks, not production RED evidence.
The other agent has since removed the original fixture's `chown` calls.

## Driver review at 2026-09-12T01:01Z

The new `run_domain_verified` function checks a pathname with `lstat`, then opens that pathname separately.
It does not verify the opened file with `fstat` or bind lookup to a trusted directory descriptor.
An untrusted ancestor can change between those operations even when the checked immediate parent belongs to root.
This leaves a file-identity race in the proposed record authorization.
A matched public-interface fault test is needed before correction.
This review has not executed that race.

The reader accepts up to 65,536 bytes without checking for an additional byte.
A valid JSON prefix with trailing content beyond that bound can escape full-input validation.
The unit field is not checked, so the current result establishes UID and cgroup matching, not a live service invocation.
Root ownership, record parsing, invocation freshness, exclusive ownership, and creation fencing must remain distinct claims.

The new driver block also adds source comments.
The repository requires an explicit user request for new source comments.
This session has no such request.
This session leaves the other agent's implementation unchanged and reports the findings here.

## Commit review at 2026-09-12T01:28Z

The user requested one commit through the commit skill.
The inventory validator exits 1 because the candidate binding for `docs/Glossary.md` is stale.
The B45 plan references `docs/cbc-evidence/soak-d2-run-domain-2026-09-12/manifest.jsonc`, but that file is absent at this review.
The main work log also fails the whitespace check because it has a new blank line at the end.
This session has not created a commit or changed the index.
The review outputs are in `/tmp/soak-b45-commit-review.B4TDzGCk`.

The B45 ownership response assigns the launcher and launch barrier to this session.
The integration owner retains the B45 evidence package and inventory update.
These updates must preserve historical execution identities and the open D2 status.

## Native cycle coordination at 2026-09-12T03:26Z

This session accepts the split in the main work log.
The integration owner retains B46 through B48, the driver, local fixtures, shared gates, the plan, evidence packaging, and the inventory.
This session owns the native launcher, launch barrier, native fixtures, and guarded diagnostic infrastructure.
The current baseline is `dd1043c70addf40d436500216a89e157080938d1`.
No network peer is visible, so these work logs remain the collaboration channel.

The next cycle tests refusal when the external service-status query stalls before admission.
This session will freeze the committed driver rather than test concurrent B46 edits as if they were the same source.
Any concurrent formal runs will use private output paths outside `/tmp/tlc-*`.
The existing implementation approval covers guarded disposable diagnostics, not an acceptance soak or developer-host privilege changes.
Before provisioning, this session will check for another live diagnostic runner and retain the result.

The proposed barrier starts trusted gate code before the driver, verifies actual manager placement, and then releases the driver after a privilege drop.
Writing a predicted cgroup into a record before unit creation does not verify placement.
The gate must not execute caller-controlled Python or loader customization with root privileges.
The release record must not be writable through another workload process or its descriptors.
The launcher will publish `unit`, `cgroup`, and `uid` for the driver's check only after manager verification.
Service invocation identity remains a separate requirement from the B45 record comparison.

Please preserve the native fixture and launcher while B46 proceeds.
Please report any live diagnostic runner or overlapping infrastructure operation in the main work log.
The final handoff will include exact fixture and source digests, observed exits, raw evidence paths, and runner termination evidence.

## Native admission results, 2026-09-12

The guarded runner is `ci-eph-f1r3node-rust-amd64-d2-20260912-032924-b21b6e`.
Its expiration timer is active, its GitHub registration is disabled, and its source matches the frozen committed baseline.
The raw evidence root is `/home/bf_spark/soak-evidence/f1r3node-rust/d2-native-admission-r3F6zFzg`.
The runner is still live during retrieval, so termination is not yet confirmed.

The unchanged native-query fixture reports RED with exit 1 and GREEN with exit 0.
RED observes one admitted iteration despite launcher refusal status 2.
The unrelated writer remains alive and advances from 174 to 189 bytes before fixture cleanup.
The matching formal control violates `UnavailableQueryPreventsNativeAdmission` with exit 12.
Both the unavailable-query correction and the available-query model pass with exit 0.

The correction starts trusted root gate code without the workload environment.
The launcher verifies the manager observation and gate identity before it publishes the placement record and release.
The gate drops groups, group identity, and user identity before it executes the driver.
The workload receives `SOAK_CONTAINMENT=required` and the root-owned placement record.
The unchanged native controller-loss fixture passes as a positive admission and shutdown regression, including two actual refused restarts.

The tested driver is the frozen B45 baseline, not the concurrent B46 candidate.
Please provide the B46 source digest when its matched cycle is complete so this session can verify the integrated handshake separately.
The native files and private formal outputs remain outside the other session's edit scope.
The result does not complete B44, private Docker containment, creation fencing, storage durability, or D2.

## B46 integration finding

The first frozen B46 integration run passes the unavailable-query fixture but fails the positive native fixture before admission.
The driver records one run-domain refusal, and the fixture exits 2 because it never reaches active work.
This is an integration setup failure, not another behavioral RED for early admission.

The B46 directory walk uses `O_RDONLY` and therefore needs directory read permission.
The native launcher created its control directory with mode `0711`.
A read-only probe against the actual control directory confirms `PermissionError` for uid 65534.
This session will use mode `0755` for that directory while keeping environment and release files private to root.
The driver remains unchanged, and the corrected native source needs fresh fixture results before handoff.

The first GREEN also received a small integer-parsing refactor after four reviewed false-positive diagnostics.
That source has its own successful native-query and B45 controller-loss regression results in `reviewed-source`.
Earlier results retain their original source snapshots.

## Native admission handoff at 2026-09-12T04:01Z

The final native-query fixture passes with both the committed B45 driver and the frozen B46 driver.
The unchanged native controller-loss fixture also passes with both drivers, including two actual refused restarts with normal summaries.
The final directory permission correction has fresh results rather than a retrospective source update.
The helper, wrapper, driver, and unchanged fixtures match the final integrated snapshot at the source check.

The guarded runner was retrieved, independently audited, and then observed `TERMINATED`.
The retrieval archive contains 524 regular members from 11 invocations and their control records.
Five special entries were excluded, so the archive is not a complete filesystem image.
The archive digest is `f66bb3afe8b6e626519e40d4b33ac643790ff5063a1fe69a60bacb13d52683e9`.
Its size is 412,882 bytes.
The earlier provisioning record describes the pre-test state, not the final state.

The complete handoff is `/home/bf_spark/soak-evidence/f1r3node-rust/d2-native-admission-r3F6zFzg/HANDOFF.md`.
That file lists each execution, source snapshot, result, archive check, and termination record.
The model note is [NativeLaunchAdmission.md](../../formal/tlaplus/soak_disk/NativeLaunchAdmission.md).
The final helper digest is `35399004d7ac8b9eab14db2fd9f9d1514bd46fbd8993ce530ef3ea94ac69d0c6`.
The tested B46 driver digest is `08e0fa0c07cb8cf4ec3b326052e65de31421b335b9ad7e5ef08dc8499cb69ce7`.

The final five-path diagnostic recheck confirms all five paths clean after an earlier inconclusive probe.
Python, embedded gate, and shell syntax checks pass.
Ordinary-user safety refusals, invalid UID refusal, scoped STE, and scoped whitespace checks pass.
Human STE Review and maintainer ratification remain pending.

The integration owner can now add the native cycle to the plan, shared gates, evidence package, and inventory.
Please review the root gate environment and release authorization before broader integration.
Please preserve the separate B46 setup failure and all earlier source identities when packaging.
The combined candidate still needs the owner's composed checks and publication audit.
No commit or push was made by this session, and external staging remains intact.
B44, private Docker containment, creation fencing, D2, claim discharge, hosted enforcement, and acceptance remain incomplete.

## Completion coordination at 2026-09-12T08:25Z

The user requested completion after staging the current candidate.
This session will finish the native evidence export and its independent audit rather than stop at the raw handoff.
The B46 package and driver remain with the integration owner.
The native export will use a separate package directory and retain B44 as an incomplete parent behavior.
Preparation starts outside the repository so it cannot overwrite an active package build.

Please confirm whether the integration owner will register the native configurations and run the combined candidate checks.
If that work has stopped, please transfer those integration files through this log before this session edits them.
The final native runtime and formal inputs remain frozen at the recorded digests.
No new diagnostic runner is needed for the completed native admission checks.
This session will not infer current hosted verification or full D2 completion from local results.

## Shared TLC log collision at 2026-09-12T08:31Z

The integration owner started the actual formal gate at `08:29:30Z` and the classifier eight seconds later.
Both processes write the same `/tmp/tlc-*` paths.
The completed replay and carrier logs now contain only the classifier's one-line substitute result, not actual TLC output.
Copying those paths after the formal gate ends cannot recover the overwritten execution evidence.
The current overlapping attempt must remain separate from a verified combined result.

Please run the actual formal gate again after the classifier and routing processes stop.
Please retain and verify every actual TLC log before another substitute-based check starts.
The shared gate files remain unchanged by this session.
This session will audit the native package and actual-log provenance after the isolated rerun.

The integration owner has registered this native cycle as B47 and has started its combined checks.
This session will not create a competing native package build.
The external commit `f75af9ed3c5b4fbab049328326642f4484dbffb3` preserves the tested native runtime bytes.
The original runtime and formal execution identities remain unchanged by that commit.

## Independent review, 2026-09-12

The B46 archive audit, packaging regressions, and JSONC regressions pass.
Every B46 package file preserves its Git-filter bytes and matches `f75af9ed3`.
The review records are under the native raw root in `independent-review-8tAIghUl/`.

The B46 README says that the native launcher does not write the record yet.
That statement is stale for the integrated native helper recorded in the same package.
Please replace it with the narrower fact that the B46 runtime fixtures do not execute the native launcher.
The B47 run announcement also needs the observed start time, `2026-09-12T08:29:30Z`, instead of `04:25Z`.

The root-gate review found a separate control-path trust gap.
The launcher checks the immediate control parent, but it does not check all ancestors before it uses control paths again.
An untrusted ancestor can permit directory replacement after the initial check.
The release grant has public manager identity fields, so filesystem protection is essential to its authorization.
The current fixture uses a trusted `/run` chain and does not test this attack.
This is a source review finding, not an observed behavioral RED.

The isolated Python gate treats the workload environment as data and drops groups, GID, and UID before it executes Bash.
That review does not establish control-path authorization, descriptor isolation, or private Docker containment.
Please retain those limits in B47 and keep B44 and D2 open.
This session will prepare a separate control-path fault cycle without changing the source used by the active integration checks.

## Control-directory refusal cycle at 2026-09-12T09:01Z

The user explicitly requested defensive control-directory validation and regression tests on approved disposable infrastructure.
This session owns the native helper and the new `test-soak-native-control-path.py` fixture.
The integration owner retains the driver, shared gates, plans, and evidence inventory.
The driver baseline for this cycle is committed `f75af9ed3`, not the concurrent B48 edits.

The selected behavior rejects a control directory below an untrusted ancestor before it executes workload code.
The fixture will use a harmless workload initialization hook to record execution and its user identity.
The fixture will invoke the public Python launcher directly, so the hook cannot run in a privileged wrapper shell.
The production driver, admission checks, manager, and privilege drop remain real.
External workload commands remain substitutes, and an unrelated writer must continue.

The cycle will retain production RED before correction and use private output paths for its matching finite model.
The correction will need the unchanged fixture and the existing native positive regressions.
No developer-host privileged test, third-party target, acceptance soak, or Git write is authorized by this cycle.
A new runner must pass the existing identity, expiration, and exclusive-use guards before tests start.

## Control-directory results at 2026-09-12T09:12Z

The guarded runner passed the existing identity, expiration, and exclusive-use checks.
Its name is `ci-eph-f1r3node-rust-amd64-d2-20260912-090605-c92d41`.
The raw root is `/home/bf_spark/soak-evidence/f1r3node-rust/d2-native-control-WkVIMLiZ`.
The runner remains live for integrated regression checks and retrieval.

The unchanged fixture reports behavioral RED with the committed helper and GREEN with the correction.
RED executes the harmless initialization hook as UID 65534 below the untrusted control ancestor.
The unrelated writer advances from 8 to 18 bytes before cleanup.
The matching control violates `UntrustedControlPreventsWorkload` with exit 12.
Both corrected model configurations pass, with two and three distinct states.

The helper now checks every control-parent component from the filesystem root with `lstat`.
Each component must be a root-owned directory with no group or other write permission.
The check occurs before control-directory creation or manager launch.
The helper digest is `2641d0ac90ee8d837e7849038a3ef1f8f66c64fd4a035f5f823a6c1f90287ea7`.
The unchanged query-refusal and native controller-loss fixtures also pass with the committed B46 driver.
This session will test a separate B48 driver snapshot next, without rebinding the earlier runtime results.

The B47 invariant review does not reproduce the reported second violation.
With `QueryAvailable=FALSE`, the model never reaches phase `admitted`, so `VerifiedReleaseAdmits` holds.
The original control reports the named invariant with exit 12.
A separate configuration that omits only that invariant passes all remaining checks with exit 0.
The supporting files are in `native-control-invariant-review/` under this cycle's raw root.
The B47 model files remain unchanged.

## Control-directory handoff, 2026-09-12

The selected control-directory repair and its bounded verification are complete.
The integrated B48 driver also passes the unchanged control-path fixture and both existing native regressions.
Its tested digest is `94b180d0c4cb6b7607284ff111e24512af8109d689e20c674fe15b717a3b8680`.
The current nine runtime files match the integrated snapshot.
The matched fixture and all seven formal execution inputs remain unchanged.

The retrieval audit verifies 298 regular members from seven source-bound invocations.
The archive digest is `7031d6bfbfaca62db377dea37be00eaee2c7eae0d114d00339d9de628a3d5f07`.
Its size is 261,267 bytes, and its expanded payload is 870,688 bytes.
Two FIFO entries are excluded, and no private environment payload is included.
The first retrieval attempt failed on an obsolete directory name and remains separate from the runtime results.
A new retrieval destination passed the audit before the runner was observed `TERMINATED`.

The full handoff is `/home/bf_spark/soak-evidence/f1r3node-rust/d2-native-control-WkVIMLiZ/HANDOFF.md`.
The [model note](../../formal/tlaplus/soak_disk/NativeControlPath.md) documents the selected behavior, runtime correspondence, and remaining limits.
The integration owner can register the two positive `NativeControlPath` configurations and the exact parent-only control.
The owner retains the shared plan, combined gates, publication package, and claim inventory.
Please preserve the earlier B47 helper identity when registering this separate correction.

No privileged test ran on the development host, and no acceptance soak started.
This session did not change the driver, workload, finalization settings, harness pins, or Git index.
The user or another session staged files during this cycle, and those index changes remain intact.
Concurrent directory replacement, mount changes, inherited descriptors, private Docker containment, and creation fencing remain unverified.
B44, D2, all claim discharges, and acceptance remain open.

## Private Docker design handoff, 2026-09-12

The user approved continuation with private Docker containment.
This session prepared the [runtime boundary and regression specification](../plans/soak-private-docker-containment-2026-09-12.md).
The specification defines private engine placement, request enforcement, accepted-request accounting, and independent termination observations.
Its twelve regression cases remain unexecuted.
The first proposed fault cycle extends the controller-loss comparison to a real Docker writer.

HEAD advanced externally to `9a6dacd7492fb7ae88451bd3536fabce3e143d15` during the design review.
That commit contains the B47 and B48 integration, and the B48 package directory now exists.
The integration owner has subsequent driver and test changes for the emergency deadline.
This session did not edit those changes, the shared gates, the shared plan, or the claim inventory.
The owner must include the new design and this log change in the next candidate binding when appropriate.

The pinned harness requires container restart, exec-based telemetry, network sysctls, and specific mount behavior.
The benchmark shard also requires its existing restart policies and published-port access through `localhost`.
Those requirements cannot be removed to obtain a passing containment result.
Docker documents separate containerd storage and authorization-plugin protocol limits that the implementation must address.
The proposed backend therefore needs guarded capability verification before its behavioral correction.

The private review records are under `$HOME/soak-evidence/f1r3node-rust/d2-private-docker-design-7mQKCfOW`.
They bind twelve node inputs and two reviewed harness files to their source identities.
No daemon, privileged fault test, cloud runner, or acceptance soak started during this design work.
This session made no runtime, harness-pin, index, commit, or push changes.
The design does not complete B44, D2, or correctness-claim discharge.

## B51 review follow-up, 2026-09-12T13:36Z

The user assigned this session all four findings from the B51 commit review.
The reviewed commit is `5bb138dfb68a6561a69b2884fee84246c30f7cb6`.
The review is at `$HOME/soak-evidence/f1r3node-rust/b51-post-commit-review-mTIVp3Wy/REVIEW.md`.
The findings concern blocked record synchronization, producer failure, the atomic-publication verdict, and incomplete publication bindings.

This session will start with a public-driver regression for producer failure in private source snapshots.
The later cycles will test shutdown during blocked synchronization and reject non-atomic publication.
No workload, harness pin, or finalization limit will change.
No commit or push is authorized by this implementation request.

The agent network reports no peers.
The last process inspection found no active shared formal, classifier, or emergency invocation.
The main work log still assigns shared runtime and publication ownership to `claude-session-e3a67b91`.
Please confirm handoff of the follow-up driver, fixture, model, gate, and inventory changes through the main work log.
Please identify the final historical B51 package before this session updates candidate bindings.
Until that handoff, this session will keep implementation changes in private source snapshots and will not replace shared runtime or publication files.

The earlier matched B51 results must keep their original source identities.
The four review findings do not close B44, D2, reserve bounds, or acceptance.

## Containment and telemetry continuation, 2026-09-13

The integration owner confirmed the B51 handoff in the main work log.
The user then extended this session's scope to runtime containment and telemetry corrections.
The shared plan and the main work log remain with the integration owner.
This session will preserve the workload, the three harness pins, and the 45-second finalization wait.

The requested producer-repair commit is `b32a2e9e8`.
Its normal commit hooks passed.
The claim inventory still reports a stale `docs/ToDos.md` input.
No push occurred, and no further commit is authorized.

The private shutdown candidate passes the frozen stalled-sync and post-exit fixtures.
It also passes the producer regression, the existing driver regressions, and two positive `RecordShutdown` configurations.
Three exact formal controls fail with TLC exit 12.
These results do not verify the private Docker engine or complete containment.

The retained repair root is `$HOME/soak-evidence/f1r3node-rust/b51-review-repairs-f9QjbAqI`.
That root retains the first inadequate verdict, a combined-fault failure, a process-substitution setup failure, and the corrected runs.
The candidate uses a guardian pipe, bounded publication workers, and post-loop breach handling.
The candidate remains outside the branch as a retained alternative.
The main work log now reassigns R1, R3, and R4 to the integration owner.
This session will not overwrite that owner's driver, summary writer, publication fixtures, gates, or inventory changes.

The in-place publication control is prepared, but the stronger publication verdict has not run.
The historical B51 package and the current candidate bindings remain incomplete.
The private Docker protocol inventory, guarded capability tests, and real-Docker comparisons remain separate requirements.
The telemetry audit's twelve regression specifications also remain open.

The reserve document's 67-second and 77-second figures remain provisional.
A publication correction alone cannot prove an aggregate bound across probes, scheduling, creators, termination confirmation, storage, and upload.
No accepted reserve bound or acceptance result follows from these local checks.

### Telemetry summary and missing-baseline corrections

The new evidence root is `$HOME/soak-evidence/f1r3node-rust/containment-telemetry-1nzZ4KmS`.
The source baseline is `b32a2e9e84ec7b45289fc661850c150977f6f1b1`.
The harness source comes from the unchanged pin.

The new fixture executes the public driver and imports the complete harness metrics and monitor modules.
A loopback HTTP server supplies synthetic node samples at the external probe boundary.
The real monitor thread writes telemetry, and the real driver retains the monitor files and phase log.
The fixture does not replace production parsing, interval calculation, summary formatting, snapshot selection, or publication.

The frozen summary RED retains all 27 raw families but omits 20 families from the phase summary.
The corrected overlay publishes all 27 families with explicit units.
The unchanged fixture passes against the correction.
The `MetricSummary` control fails with exit 12 and `CollectedRequiredPublished`.
The corrected model passes.

The next frozen case returns HTTP 503 for the baseline, followed by a valid scrape.
The summary-only candidate fails because it treats missing baselines as zero.
The corrected interval function preserves an unavailable result with `reason=missing_baseline`.
The unchanged missing-baseline fixture and its healthy-summary control both pass.

The first `MetricBaseline` model attempt fails with exit 76 because it mixes strings and numbers in a set.
The second attempt fails with exit 75 because a Boolean assignment lacks parentheses.
Both setup attempts and their source files remain in the evidence root.
The corrected control fails with exit 12 and `MissingBaselineUnavailable`.
Both corrected configurations pass, including the available-baseline positive case.

The shared driver and summary writer changed during this work.
The telemetry comparisons therefore use frozen source directories, not the changing shared driver.
Final integration needs a new comparison against the owner's completed source.

Shared-gate registration remains pending for `MetricSummary` and `MetricBaseline`.
The remaining telemetry cases include process epochs, resets, labels, invalid samples, timing, authenticated sessions, interrupted artifacts, disk coverage, and remote retrieval.
These local synthetic samples do not establish live-node emission or successful upload and retrieval.
O1 remains open.

Before the capability work, the read-only infrastructure query found no active D2 diagnostic runner.
The queried infrastructure configuration matched the harness pin.
That observation did not include a privileged capability test.

### Guarded private-engine capability results

The first diagnostic runner stopped during bootstrap because its console-history state did not match the provisioner's expected spelling.
The termination helper confirmed that runner as `TERMINATED`.
A corrected provisioner created a second runner with the same identity, isolation, resource, and two-hour expiry controls.
Both runners used the recorded source baseline and unchanged harness pin.
Neither runner started an acceptance soak.

The namespace probe confirms the cgroup-root inode and separate mount and network namespaces.
The private-engine probe starts a separate Docker engine and containerd inside the run domain.
A small BusyBox fixture supplies controlled file writers.
The probe tests automatic restart, container exec, root inside the container, and a published loopback port.

The observer checks eight process identities through host cgroup membership and process descriptors.
Those identities cover the service main process, Docker engine, containerd, shim, proxy, container, exec process, and exec client.
All eight processes exit before fixture cleanup.
Both selected writers stop.
The corrected comparison also preserves the selected shared-engine and firewall metadata.

The record retains the failed setup attempts and intermediate results.
Those attempts expose timestamp parsing, cgroup mounting, PID writing, restart assumptions, and a time-dependent firewall comparison.
Setup failures are not production RED results.
The capability results do not execute the public driver or an API gateway.
They do not prove permanent creation closure, delayed-request accounting, complete compatibility, or unrelated-writer progress.

The retained archive contains 121 regular files, including 116 files bound to the remote content manifest.
Its verified expanded size is 6,698,161 bytes.
Its SHA-256 is `8aaa4ee1d2bd85c4c0009f5b562258c59b18881a35c33beccb1884b1285d816d`.
The archive contains selected evidence and fixture inputs, not a full filesystem image.
After retrieval, the termination helper confirmed the second runner as `TERMINATED`.

The capability records remain under `containment-capability/` in the current evidence root.
The two runner records remain under `containment-runner/` and `containment-runner-v2/`.
No private-engine implementation has entered the branch.
Complete runtime containment and the aggregate reserve bound remain open.

### Decreasing cumulative samples

The next frozen fixture tests decreasing histogram sums, decreasing histogram counts, and decreasing event counters.
Both decreasing cases fail against the missing-baseline candidate.
The healthy-summary, missing-baseline, and genuine-zero controls pass against that same candidate.

The `MetricMonotonicity` control fails with exit 12 and `DecreasingSamplesUnavailable`.
The correction rejects a histogram interval if either cumulative component decreases.
It also rejects a decreasing event counter.
The summary retains `reason=cumulative_decrease` instead of a numeric interval.

Both decreasing cases and all three controls pass against the frozen correction.
The corrected finite model also passes.
The model covers seven selected vectors, not parser correctness or all possible metric values.
A decrease does not establish a process restart.
Process epochs, labels, invalid samples, timing, artifact authentication, disk coverage, and remote retrieval remain open.

The matched records remain under `monotonic/` in the current evidence root.
Shared-gate registration and final integration against the owner's completed driver remain pending.
No further commit or push is authorized.

### Invalid cumulative values

The next fixture supplies numeric overflow and negative values in histogram sums and event counters.
Each fault occurs separately in the baseline and final sample.
All four cases fail against the monotonicity candidate.
The five earlier controls pass against that same candidate.

A fixture-only source change replaces two fixed-literal conversions with `math.inf`.
The record preserves the first fixture and its results.
The revised fixture repeats all four RED cases and all five controls before the production correction.
It also reproduces the earlier summary, missing-baseline, and monotonicity defects against their original source candidates.

The `MetricSampleValidity` control fails with exit 12 and `InvalidSamplesRejected`.
The correction checks both samples before it calculates an interval.
The summary distinguishes `nonfinite_sample`, `negative_sample`, `cumulative_decrease`, and `missing_baseline`.
The negative-value rule applies to the selected nonnegative histogram families and event counters.

The unchanged revised fixture passes all nine cases against the frozen correction.
The corrected finite model, overlay regression, and driver regression also pass.
The model checks sample categories and result classification, not numeric parsing or implementation correctness.
The matched records remain under `sample-validity/` in the current evidence root.

These cases do not test literal NaN parsing, malformed tokens, missing final components, large-integer precision, or every resource sample.
Process epochs, label identity, timing, authenticated artifact selection, interrupted artifacts, disk coverage, and remote retrieval remain open.
The four telemetry models still need shared-gate registration and complete evidence bindings.
No completion or acceptance claim follows from these checks.

### Current-source integration snapshot

The next snapshot captures the six current execution inputs before another comparison.
The capture checks each input before and after copying.
All nine telemetry cases pass against that frozen snapshot with the revised fixture unchanged.
The snapshot remains under `current-integration-20260913T063232Z/` in the current evidence root.

This comparison includes the integration owner's current driver and summary writer.
It does not declare those files final or change their ownership.
The glossary now defines metric series, metric intervals, and process epochs.
The five-file diagnostic check reports four clean files and one inconclusive file, with no reported findings.
Human STE Review and the remaining containment and telemetry obligations remain open.

### Private Docker follow-up

The recorded daemon reports commit `8ec5ab3`.
The upstream API schema at that commit declares version `1.55`.
The retrieved schema contains 98 paths and 159 definitions.
The source records remain under `private-api-schema/` in the current evidence root.
This retrieval does not implement a gateway or prove that the binary matches the source.

The peer query stops after a broker timeout.
That result does not establish peer absence or exclusive ownership.
Shared-file ownership remains unchanged.
The private gateway, creation fence, and remaining telemetry corrections remain incomplete.

### Storage scope and cleanup authorization, 2026-09-14

The user assigned storage completion and cleanup to this session.
The other agent owns D2 and B44.
This session will not change the driver, containment implementation, workflow deadlines, shared gates, or claim inventory.

The storage work uses `scripts/verification-storage.py` and `scripts/cargo-low-disk.sh`.
The plan requires real source capture and reuse, archive coverage checks, and focused regression tests.
The source snapshot must exclude the nested chart cache.
Required source bytes must remain identical after capture.
Each removal must retain its authorization, scope, archive reference when applicable, and measured result.

The cleanup candidates are the inactive incremental cache, historical extractions, and five duplicated chart-cache copies.
Cache removal requires the Cargo build lock and a fresh writer check.
Evidence removal requires verified archive coverage and a retrieval test.
Original manifests and attempt records must remain unchanged.
This work does not authorize a commit, push, or acceptance soak.

### Storage completion and retained archives, 2026-09-14

The operation records remain under `$HOME/soak-evidence/f1r3node-rust/storage-cleanup-20260914-JILmoOsl`.
The earlier storage-development attempts remain under `storage-practicality-Esi6iuNc` in the same evidence directory.
This session removed the following allocations after the required checks.

| Removed scope | Allocated size before removal |
| --- | ---: |
| `target/debug/incremental` | 41.59 GiB |
| `target/soak-evidence/34180346282/attempt-1/extracted` | 13.12 GiB |
| Five frozen `scripts/soak-charts/target` copies | 3.93 GiB |

These figures describe removed allocations, not an exact increase in filesystem free space.
Hardlinks and concurrent work can change the free-space difference.
The existing `target/debug/deps`, `target/release`, and live chart build cache remain untouched.
The operation did not run `cargo clean`.

The incremental removal held the Cargo build lock.
Its inventory contained 60,679 entries and only recognized, non-executable compiler products.
The operation found no active Cargo or Rust compiler process before removal.
The compressed inventory and removal receipt remain in the operation records.

The historical archive matched all 652 extracted files and 14,083,145,744 file bytes.
A separate retrieval recovered `early-exit.txt` before removal.
The archive remains at `$HOME/soak-evidence/f1r3node-rust/34180346282/attempt-1/archive.zip`.
Its digest is `3b25041120055e32a49810ec81f762cdbf570af9b2bdba8bf93c14569489a4cc`.
The original manifest remains unchanged.
The former extraction has a sibling `extracted.archived.json` receipt.

An initial attempt rejected a malformed digest copied from the session summary.
The successful attempts used the authoritative repository manifest instead.

The five chart caches had identical file bytes and modes.
The operation retained one `chart-cache.tar.gz` archive of 284,506,363 bytes.
Its digest is `ef6c7e008757b6e00514f913d3e4d9e3b98aae0b4de882acd872773ec0cf7d22`.
A complete extraction matched the original cache before removal.
This archive preserves file bytes and modes, not a complete filesystem image or original hardlink topology.
All five non-cache source trees and inventoried checksum manifests remained unchanged.

Post-removal verification matched 4,844 original checksum entries from two candidate manifests.
The check retrieved 4,580 entries from the archive and read 264 entries from retained source files.
This integrity check did not rerun the historical tests.

The continuation evidence root now contains `archived-chart-caches.json` with the archive reference and removed paths.
The retained archives are local copies, not verified remote backups.
A replay that checks original source manifests requires restoration of the archived cache paths.

1. Verify the recorded archive digest.
2. Create a new empty cache directory at the required original path.
3. Restore the archived cache into that directory.
4. Verify the original checksum manifest.

### Source workflow validation

The initial real source capture retained 694 files and 1,653,100 source bytes without the nested chart cache.
A second capture reused the same verified snapshot.
The public-driver summary case passed directly from that snapshot.
The optional local Cargo profile passed 48 `graphz` tests with a 204 MiB external cache.
It did not create another populated incremental cache.

Review found four additional snapshot refusal gaps.
The new cases covered an unmatched selection, a dot extra-file argument, a writable stored directory, and an unrecorded stored directory.
All four failed against the frozen baseline.
The unchanged 33-case fixture passed against the frozen correction.
The correction validates selections and the complete stored directory structure before reuse.

The workflow capture retained 700 files and 1,660,406 source bytes and reused the same source-store entry on a second invocation.
Its snapshot identifier is `84e1c7a27ae0ab7719fc07dd45c22a26cf7cf85d7e34fcf1d7cde56c529649aa`.
All nine existing telemetry cases passed from that snapshot through the public driver.
The fixture retained its declared external workload substitutions.
These tests validate source-store use, not production Docker containment or complete observability.
The scripts README now describes the receipt-based workflow.

A final safety check found that the regression fixture inherited the caller's Git index selection.
The corrected fixture removes inherited Git environment variables and disables external Git configuration for its disposable repositories.
The final 34-case suite passed with normal settings and an inherited index sentinel.
The sentinel remained unchanged.
The nine-case workflow's six execution inputs still match the retained snapshot after this test-only correction.

The scoped STE Check, whitespace check, Python syntax check, and Bash syntax check passed.
The language-server sweep reported three clean files and two inconclusive files, with no findings.
These results do not establish full language-standard conformance or discharge correctness claims.

The source-store and Cargo limits remain admission checks, not runtime filesystem quotas.
The tools do not remove retained evidence automatically or discharge correctness claims.

The other agent changed HEAD during this work.
This session preserved those changes and did not create a commit or push.
D2 and B44 remain outside this storage scope.

## Out-of-memory claim review, 2026-09-14

The user authorized review of the ownership claims in commit `2c1bc5241bd8ac2df57932f68977602e7ad156eb` after storage completion.
This scope covers the ownership document, its plan references, and the correction record.
It does not authorize runtime changes, privileged tests, another cleanup, claim discharge, a commit, or a push.
The other agent retains the production D2 and B44 work.

The review uses the committed driver and native launcher with harness revision `962effd17708192627bd249362761c0ccb1fd5fa`.
The review retains 18 selected source files in the external `oom-ownership-review-j5cltem1` directory.
The generated `inputs.json` records their source revisions, sizes, and digests.
No private environment, process environment, or key material was collected.

The driver already applies host-process preferences through `guardian_mark_workload_oom_preferred`.
The Docker settings depend on the host-memory floor and the supported wrapper route.
Neither mechanism guarantees runner survival or complete containment.
The historical pytest incident concerned Docker-provider containers, not a demonstrated subprocess-provider preference defect.
The corrected ownership review distinguishes these source observations from historical tests and remaining runtime obligations.

The corrected ownership review also records that the host helper can select marked harness processes, not only node processes.
The native launcher sets a diagnostic memory limit but has no explicit `OOMScoreAdjust` setting or corresponding property verification.
No new runtime defect is classified as a behavioral RED in this review.

The repository inventory validator and all 10 JSONC regression cases passed.
A semantic comparison confirms that only the two revised documentation digests changed in the inventory.
Claim statuses, gate results, and acceptance data are unchanged.
The scoped STE Check passed, but human STE Review remains necessary.
The language-server check found one spelling false positive within a historical commit identifier.
The JSONC language-server result was inconclusive, so the repository parser and inventory validator provide the metadata check.

The external review directory retains the source capture, public Linux references, check outputs, and the final documentation diff.
This review leaves the driver, launcher, harness pins, workload, and 45-second finalization wait unchanged.
D2, D3, and acceptance remain pending.

## D2/B44 assignment update, 2026-09-14

The user assigned production containment and real-Docker verification to another agent on a different machine.
The [assignment notes](task-soak-disk-hygiene-stop-2026-09-08T05-06Z.md#d2b44-assignment-to-another-machine-2026-09-14) contain the runner requirements and completion checklist.
The receiving agent and machine identities remain to be recorded.
This session will not duplicate that step or provision its infrastructure.
The branch integration agent retains shared-file coordination.

This note records the assignment only and does not start another agent or a diagnostic run.
Storage cleanup is complete.
D2, D3, and acceptance remain pending.

## D2/B44 transfer update, 2026-09-16

The user transferred D2/B44 back to this session, identified as `pi-session-disk-protection-01a050ce` on `spark-f718`.
This transfer supersedes the September 14 assignment and the corresponding local-work restriction.
The [continuation log](task-soak-disk-protection-2026-09-16T02-54Z.md) records the current source, work sequence, and diagnostic request.
The branch integration owner retains shared gate registration, inventory, and publication coordination.

No peer is visible, but the earlier assignee's remote activity remains unverified.
No runner or acceptance soak has been launched for this transfer.
Diagnostic execution requires approved disposable infrastructure and verified exclusivity.
Storage cleanup remains complete.
D2, D3, and acceptance remain pending.
