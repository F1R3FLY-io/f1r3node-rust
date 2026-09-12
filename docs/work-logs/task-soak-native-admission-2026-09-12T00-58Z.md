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
