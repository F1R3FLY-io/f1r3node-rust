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
