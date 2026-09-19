# TASK-017-11: Protocol and Phlo Profile

## Ownership

- Task: TASK-017-11, within EPIC-017.
- Implementer: `pi-soak-carrier-index-linux`.
- Started: 2026-09-19T15:57:08Z.
- Branch: `formal/soak-casper-consensus`.
- Starting revision: `fcc2fb22270d403757785f6955915e40dec3625d`.
- Claim: CLAIM-CASPER-SOAK-007.

The remote agent relayed the explicit user assignment in message `01a0ba62-47e7-7b0e-92cc-c60f15cdea37`. The working tree was clean before this ownership update.

## Scope

This task verifies the controlled-transcript protocol and Phlo profile. It does not change node behavior or prove node accounting.

The [claim](../claims/casper-soak-version-phlo.md) requires separate Casper protocol and accounting authority labels. Generated requests and captured observations must preserve both `phloLimit` and `phloPrice`.

[D-01](../casper/design/decision-ledger/01-protocol-version-authority.md) keeps protocol 7 separate from accounting authority 8. Actual activation requires its independent approval, supported build, and fresh genesis.

[D-12](../casper/design/decision-ledger/12-deploy-cost-limits.md) preserves both signed fields, minimum-price validation, prepayment, refund, and exhaustion behavior. Undefined multi-wallet funding remains blocked.

The profile compares observations with pinned fixture expectations. It does not calculate a new economic policy or grant protocol authority.

## Coordination

This session retains TASK-017-10. The slashing agent owns TASK-017-9 acceptance records and its tracker block. The preparation agent retains TASK-017-12 and TASK-017-14.

The ownership checkpoint changed only the TASK-017-11 tracker fields and this work log. It did not change claim specifications, shared compiled sources, accepted claims, or `.gitattributes`.

Executable implementation started after publication of the ownership claim. The assignment does not authorize a commit or push. Each publication operation requires separate user consent.

The six-workflow bindings-script change remains outside this assignment. Live qualification, node deployment, and soak dispatch remain outside this task.

## Plan

- [x] Read the task, claim, and D-01/D-12 decisions.
- [x] Prepare the ownership claim and notify the remote agent.
- [x] Confirm separately authorized publication of the ownership claim.
- [x] Implement the separate Rust generator, collector, classifier, and binary.
- [x] Add fixtures for version separation, signed-field preservation, field mutation, minimum-price boundaries, and settlement outcomes.
- [x] Preserve unknown measurements and independent product failures.
- [x] Add the bounded model and three named negative controls.
- [x] Verify native and isolated Linux fixtures, exact evidence inventories, and shared regressions.
- [x] Verify hosted CI after separately authorized publication.
- [x] Package and sanitize retained evidence outside Git under the current retention rule.
- [x] Add pending canonical ledger records and their required compatibility symlinks.
- [x] Obtain separate binding acceptance and workflow-tag ratification.
- [x] Upload and verify the two authorized sanitized archives.
- [x] Discharge Claim007 and complete the strict task check.

## Intended files

- `scripts/casper-soak/src/profiles/version_phlo.rs`
- `scripts/casper-soak/src/bin/casper-version-phlo.rs`
- `scripts/casper-soak/tests/version_phlo.rs`
- `scripts/casper-soak/check-version-phlo.sh`
- `.github/workflows/casper-version-phlo.yml`
- `formal/tlaplus/casper_soak/profiles/version_phlo/`
- `docs/claims/casper-soak-version-phlo.md`

The workflow exists, but its CbC tag remains unratified. Cargo configuration, shared library registration, accepted profiles, and the common claim auditor remain unchanged.

## Initial verification boundary

The initial model bound is two scenarios and three observations per scenario. The controls target `VersionLabelsSeparate`, `BothPhloFieldsCaptured`, and `SettlementOutcomeClassified`.

Passing controlled transcripts cannot establish actual node interface support. Live and post-merge requests remain blocked in this implementation scope.

Claim 007 remains pending. Local checks provide bounded verification results, not binding acceptance, workflow-tag ratification, a waiver, or task completion.

## Implementation checkpoint

The user published ownership commit `623ee7f54e841295c2538b67e3bcbfee19fea7d5`. The integrated starting revision is `4a7bf8960800d76be82d887c205a898169bdb8fe`.

Implementation started on 2026-09-19 after the publication check. The first increment adds the separate profile, binary, and executable fixtures.

The controlled envelope uses an explicit synthetic encoding. Its retained bytes bind both signed fields, but do not qualify protobuf encoding or cryptographic signature verification.

The profile keeps protocol 7 and accounting authority 8 as separate context labels. Unsupported-version fixtures use a separate proposed protocol version.

The collector requires envelope, admission, and settlement observations. Signed-field mutation also requires an observed fault receipt after signing.

No further TASK-017-11 commit, push, claim acceptance, workflow tag, or node dispatch is authorized.

## Local verification checkpoint

Native Linux and isolated Linux each passed 15 Rust tests, 102 fixture cases, and 103 invocations. The isolated run used host-built binaries, not a container build.

The isolated container had no network access, a read-only root, two CPUs, 512 MiB memory, and 64 process slots. It used an unprivileged user and a bounded temporary filesystem.

The clean model completed with 1,201 generated states and 625 distinct states. Each named negative control produced exit 12 and its configured invariant violation.

The shared bindings, claims, manifest, and model suites passed all 11 tests. Strict audits passed for Claims 001 through 006.

Targeted Clippy, Rust formatting, shell syntax, and the exact-inventory runner passed. LSP diagnostics left one Rust file unconfirmed, so they do not establish complete diagnostic coverage.

The renamed-case runner control returned exit 1. The interruption control returned exit 143. Both controls retained failed summaries.

The initial artifact check verified 1,250 retained references in each execution environment. This check does not replace final evidence review or claim discharge.

Evidence remains under `/tmp/version-phlo-checks/`. The `runner-native-01`, `isolated-01`, `models-01`, and `runner-controls` directories contain the successful checks and runner controls.

The `bootstrap-01` directory retains the first failed fixture attempt. That fixture changed its producer without updating the source reference, so it failed before its intended ordering check.

The `review-red-01` directory retains a demonstrated receipt-binding defect and the affected source snapshot. The repaired classifier requires matching fixture and deploy identities in mutation receipts.

No sanitized TASK-017-11 archive exists yet. Final source-bound packaging, canonical ledgers, compatibility links, hosted verification, and human acceptance remain pending.

## Coordination checkpoint

The new protocol/Phlo workflow adds a seventh missing entry to the separate shared workflow inventory issue. This task does not authorize that script change or Claim 001 renewal.

TASK-017-9 and its closure records remain unchanged. Only the TASK-017-11 tracker block changed during this implementation.

This session retains TASK-017-10. Carrier evidence transfer takes priority while its authorized Mac publisher has access to the existing draft release.

## Completion takeover and bounded review

On 2026-09-19, the user transferred the remaining work to the Mac session because the Linux agent had stopped.

The completion owner is `pi-casper-slashing`. The original implementer and its failed attempts remain recorded above.

The Mac session retrieved `/tmp/version-phlo-checks/` through the existing verified SSH route. It did not change the remote checkout or execute a node.

The remote checkout had moved to another branch. The retained 21-source manifest still matched the reviewed implementation at `807bf94dcb0389fdd57100bc32f64eea20f64ea6`.

Independent checks verified native Linux, isolated Linux, and hosted evidence. Each set contains 15 passing tests, 102 cases, 103 invocations, and 1,250 checked references.

Four bounded model controls passed with exact source, configuration, executable, and log identities. Eleven shared regression tests passed again on the Mac.

Hosted run `35459964874` produced artifact `10588828005`. Its ZIP digest is `f515f4c8f31c07206ba5e333c939f3c2bc387bc24477774aaf466d91f340b8cc`.

The first Mac validator omitted 184 request and manifest checks. Its unchanged 1,250-reference assertion failed. The corrected validator adds those checks and passes.

The original validator, failure, correction, bootstrap failure, receipt-binding regression, renamed-case refusal, and interruption result remain retained.

The isolated run used host-built binaries. The retained evidence identifies the image and executables, but does not retain the original container configuration inspection.

The [review report](../casper/cbc-evidence/runs/casper-version-phlo-20260919-01/report.json) binds twelve unchanged profile artifacts and the current claim specification.

The sanitized archive contains 28,556 manifest-bound files plus its manifest. Nine text files have exact before-and-after redaction records. Fixture and reviewed source bytes remain unchanged.

The archive is `target/task-017-11-final/casper-version-phlo-20260919-01.external.tar.gz`. Its SHA-256 is `330e7609143ffe0eaf839ecf082a61ec02809acdd0bd357b66aac7ecee3616bb`.

Twelve canonical ledgers and twelve relative compatibility links now record pending binding status. The workflow tag remains unapplied.

A concurrent session changed the shared bindings inventory and reopened Claim001. This session neither changed that script nor renewed Claim001.

TASK-017-14 must retain the final archive and supplementary checks before scratch cleanup. Original Linux and hosted archives remain local and require privacy review before publication.

Remaining decisions are bounded Claim007 binding acceptance, workflow-tag ratification, and authorization for the sanitized evidence upload. Strict closure follows those decisions and successful checks.

No commit, push, release publication, policy activation, live qualification, or node execution occurred during this takeover.

## Accepted binding and strict closure

On 2026-09-19, the user accepted Claim007’s bounded protocol/Phlo harness binding and separately ratified the exact mandatory/high workflow tag.

The [acceptance report](../casper/cbc-evidence/runs/casper-version-phlo-acceptance-20260919-01/report.json) preserves both approval quotations and binds the twelve unchanged profile artifacts.

The previous pending ledgers, claim specification, and attributes remain in the acceptance metadata archive. No prior review or failed result was replaced.

The user separately authorized upload of the two sanitized archives. Assets `575328309` and `575328308` contain those exact reviewed bytes.

Both downloads matched the local archives byte-for-byte. The release remains a draft. Its 25 existing assets, including its index and checksum files, remain unchanged.

Fresh native verification passed 15 tests, 102 cases, 103 invocations, and four model controls. Independent checks verified 92 reports and 1,250 retained references.

The first shared check ran between the claim edit and ledger renewal. It refused the inconsistent metadata with exit 101.

After ledger renewal, all eleven shared tests passed without source or assertion changes. The failed check remains retained.

The first acceptance verifier read the shared-test count from the wrong field. Its correction preserves the eleven-test expectation and requires four passing suites.

A separate preparation session changed TASK-017-12 during verification. Tracker checksum checks refused those changes. Subsequent checks preserved that task and compared all unrelated task objects.

Claim007 and its twelve canonical ledgers are discharged. The exact twelve-artifact CbC gate reports zero gaps. Strict audits pass for Claims002 through 008.

The full bundle remains pending only for Claim001. This acceptance does not renew Claim001 or authorize a campaign.

The unchanged completion helper ran on a tracker copy in a fresh Bash process with strict checks and no force option.

Its integrity grade is `full`, with no gaps. The reviewed patch marks only TASK-017-11 complete and updates its acceptance metadata.

The repository completion adapter, tracker backup, unrelated task objects, and previous accepted records remain unchanged.

Fresh acceptance evidence remains under `target/task-017-11-acceptance/`. TASK-017-14 must retain this evidence before scratch cleanup.

The later private supplement retains both original Linux binaries and their verified identities. Neither binary was uploaded. Original container inspection remains unavailable.

No commit, push, release publication, node execution, protocol activation, or post-merge execution occurred during acceptance and closure.
